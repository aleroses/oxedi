//! Balancing checks: the spec's balancing rules, added up in every instance
//! of each rule's loop and compared when the instance closes.
//!
//! Every captured segment whose occurrence a rule reads adds the amounts it
//! holds to the innermost open instance of the rule's loop, on the rule's
//! target or sum side and with the value's sign. An absent or empty amount
//! counts as zero when its element is optional, as the situational amounts
//! of an adjustment segment are. The rule is not evaluated for an instance
//! when it cannot be exact: the target occurrence never appeared, a required
//! amount is absent or empty, an amount is not a decimal of the rule's scale
//! or the total overflows. A missing occurrence or element raises its own
//! finding, so the instance is left to it.
//!
//! An amount that is not a decimal is left to the type check, which runs only
//! where a table column projects that element: with a spec whose tables do
//! not read it (a custom spec without tables, for one), the instance is
//! skipped with no finding at all. A total that overflows 128 bits is skipped
//! with no finding either.
//!
//! The claim rule adds only the CAS amounts of the claim and its services:
//! claim-level interest (`AMT*I`) and prompt-pay discounts are not part of
//! it, as the guide's text for them has not been verified.

use crate::diagnostic::Rule;
use crate::segment::Segment;
use crate::spec::{LoopId, Spec, render_terms};
use crate::value::parse_r;

use super::StructureChecker;

/// What one appearance of an occurrence adds to one rule.
#[derive(Debug, Clone)]
struct Feed {
    /// Index of the rule in the spec's balancing rules.
    rule: usize,
    /// `true` for the target side, `false` for the sum.
    target: bool,
    negative: bool,
    component: Option<usize>,
    /// `(element, required)` for every amount read.
    elements: Vec<(usize, bool)>,
}

/// The running totals of one rule in one open instance of its loop.
#[derive(Debug, Clone)]
struct Pending {
    rule: usize,
    /// Index of the instance in the checker's open stack.
    depth: usize,
    target: i128,
    sum: i128,
    target_seen: bool,
    /// Set once the instance cannot be evaluated exactly.
    blocked: bool,
    /// Segments read, in stream order.
    segments: Vec<usize>,
    /// The first target amount: segment, element, component and its text.
    anchor: Option<(usize, usize, Option<usize>, Vec<u8>)>,
}

/// The balancing rules laid out for the checker.
#[derive(Debug, Clone, Default)]
pub(super) struct Balances {
    /// Per occurrence of every loop, loop after loop (as the checker's
    /// `limits`): what its appearances add.
    feeds: Vec<Vec<Feed>>,
    /// Per loop: the rules that check its instances.
    rules_of: Vec<Vec<usize>>,
    pending: Vec<Pending>,
}

impl Balances {
    /// Lays out the rules of `spec`; `first` gives, per loop, where its
    /// occurrences start in the per-occurrence tables.
    pub(super) fn new(spec: &Spec, first: &[usize]) -> Self {
        let total = spec.loops().iter().map(|def| def.occurrences.len()).sum();
        let mut feeds: Vec<Vec<Feed>> = vec![Vec::new(); total];
        let mut rules_of = vec![Vec::new(); spec.loops().len()];
        for (index, rule) in spec.balancing().iter().enumerate() {
            if let Some(rules) = rules_of.get_mut(rule.per.index()) {
                rules.push(index);
            }
            let sides = rule
                .target
                .iter()
                .map(|term| (true, term))
                .chain(rule.sum.iter().map(|term| (false, term)));
            for (target, term) in sides {
                let def = spec.get(term.loop_id);
                let Some(occurrence) = def.occurrences.get(term.occurrence) else {
                    continue;
                };
                let elements = term
                    .elements
                    .iter()
                    .map(|&element| {
                        let required = spec
                            .element_def(&occurrence.segment, element, term.component)
                            .is_some_and(|def| def.required);
                        (element, required)
                    })
                    .collect();
                let slot = first
                    .get(term.loop_id.index())
                    .and_then(|&start| start.checked_add(term.occurrence))
                    .and_then(|at| feeds.get_mut(at));
                if let Some(slot) = slot {
                    slot.push(Feed {
                        rule: index,
                        target,
                        negative: term.negative,
                        component: term.component,
                        elements,
                    });
                }
            }
        }
        Self {
            feeds,
            rules_of,
            pending: Vec::new(),
        }
    }
}

/// Adds `value` to `total` with its sign; `None` on overflow.
fn add(total: i128, value: i128, negative: bool) -> Option<i128> {
    if negative {
        total.checked_sub(value)
    } else {
        total.checked_add(value)
    }
}

impl<'s> StructureChecker<'s> {
    /// Starts the totals of every rule that checks the instance of `id` now
    /// on top of the open stack.
    pub(super) fn balance_opened(&mut self, id: LoopId) {
        let Some(depth) = self.open.len().checked_sub(1) else {
            return;
        };
        let Some(rules) = self.balances.rules_of.get(id.index()) else {
            return;
        };
        for &rule in rules {
            self.balances.pending.push(Pending {
                rule,
                depth,
                target: 0,
                sum: 0,
                target_seen: false,
                blocked: false,
                segments: Vec::new(),
                anchor: None,
            });
        }
    }

    /// Adds the amounts of a segment captured in loop `id` as the occurrence
    /// it `matched` to the open instances whose rules read it.
    #[inline]
    pub(super) fn balance_captured(
        &mut self,
        id: LoopId,
        segment: &Segment<'_>,
        matched: Option<usize>,
    ) {
        let Some(index) = matched else {
            return;
        };
        let spec = self.spec;
        let balances = &mut self.balances;
        let Some(feeds) = self
            .first
            .get(id.index())
            .and_then(|&start| start.checked_add(index))
            .and_then(|at| balances.feeds.get(at))
        else {
            return;
        };
        for feed in feeds {
            let Some(pending) = balances
                .pending
                .iter_mut()
                .rev()
                .find(|pending| pending.rule == feed.rule)
            else {
                continue;
            };
            let scale = spec.balancing().get(feed.rule).map_or(0, |rule| rule.scale);
            if pending.segments.last() != Some(&segment.index) {
                pending.segments.push(segment.index);
            }
            for &(element, required) in &feed.elements {
                let text = segment.leaf(element, feed.component).unwrap_or_default();
                if feed.target && pending.anchor.is_none() {
                    pending.anchor = Some((segment.index, element, feed.component, text.to_vec()));
                }
                let value = if text.is_empty() {
                    if required {
                        pending.blocked = true;
                    }
                    0
                } else if let Some(value) = parse_r(text, scale) {
                    value
                } else {
                    pending.blocked = true;
                    0
                };
                let total = if feed.target {
                    &mut pending.target
                } else {
                    &mut pending.sum
                };
                match add(*total, value, feed.negative) {
                    Some(sum) => *total = sum,
                    None => pending.blocked = true,
                }
            }
            if feed.target {
                pending.target_seen = true;
            }
        }
    }

    /// Compares the totals of every rule of the instance on top of the open
    /// stack, which is closing, and reports each one that does not add up.
    pub(super) fn balance_closed(&mut self) {
        let Some(depth) = self.open.len().checked_sub(1) else {
            return;
        };
        let opened_at = self.open.last().and_then(|top| top.opened_at);
        while self
            .balances
            .pending
            .last()
            .is_some_and(|pending| pending.depth >= depth)
        {
            let Some(pending) = self.balances.pending.pop() else {
                break;
            };
            if pending.blocked || !pending.target_seen || pending.target == pending.sum {
                continue;
            }
            self.balance_mismatch(pending, opened_at);
        }
    }

    #[cold]
    fn balance_mismatch(&mut self, pending: Pending, opened_at: Option<usize>) {
        let spec = self.spec;
        let Some(rule) = spec.balancing().get(pending.rule) else {
            return;
        };
        let Some((segment, element, component, datum)) = pending.anchor else {
            return;
        };
        let finding = Rule::BalanceMismatch {
            rule: rule.name.clone(),
            loop_name: spec.loop_name(rule.per).to_string(),
            opened_at,
            target: render_terms(spec, rule.per, &rule.target),
            sum: render_terms(spec, rule.per, &rule.sum),
            expected: pending.target,
            computed: pending.sum,
            scale: rule.scale,
            segments: pending.segments,
        };
        self.report_at(finding, Some(segment), Some(element), component, datum);
    }
}
