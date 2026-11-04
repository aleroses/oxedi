//! Balancing rules: amounts that must add up inside one instance of a loop,
//! their compilation against the loops and the `segments` section, and the
//! text that describes them in messages. Their JSON shape is in `schema`.
//!
//! A rule names the loop whose every instance it checks (`per`), a `target`
//! and a `sum`, each a list of signed values. A value reads one or more
//! elements (or one component of each) of every appearance of an occurrence,
//! in the `per` instance itself or in a loop below it; every appearance adds
//! to the side it belongs to. The rule holds when both sides are equal, to
//! the last decimal place.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use super::Spec;
use super::balance_error::BalanceError;
use super::error::SpecError;
use super::loops::LoopId;
use super::occurrence_error::place;
use super::occurrences::element_def;
use super::raw::{RawBalance, RawTerm};
use super::render::render_key;
use super::segments::ElementDef;
use crate::value::ElementType;

/// One signed value of a balancing rule: elements of an occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceTerm {
    /// The loop of the occurrence: the rule's `per` loop or one below it.
    pub loop_id: LoopId,
    /// Index of the occurrence in its loop's occurrences.
    pub occurrence: usize,
    /// 1-based positions of the elements read, in the order written.
    pub elements: Vec<usize>,
    /// 1-based component read in each element, when the amounts are components.
    pub component: Option<usize>,
    /// `true` when the value is subtracted (`"sign": "-"`).
    pub negative: bool,
}

/// A balancing rule: in every instance of `per`, the values of `target` add
/// up to the values of `sum`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceRule {
    /// Name used in the JSON, e.g. `claim_balance`.
    pub name: String,
    /// The loop whose every instance the rule checks.
    pub per: LoopId,
    /// The amounts the rule expects.
    pub target: Vec<BalanceTerm>,
    /// The amounts that must add up to the target.
    pub sum: Vec<BalanceTerm>,
    /// Decimal places the rule adds at: the largest scale of its elements.
    pub scale: u8,
}

/// Compiles the `balancing` section against the loops and segments of `spec`.
pub(super) fn compile_balancing(
    spec: &Spec,
    raw: BTreeMap<&str, &Value>,
) -> Result<Vec<BalanceRule>, SpecError> {
    let mut rules = Vec::with_capacity(raw.len());
    for (name, value) in raw {
        let def = RawBalance::deserialize(value).map_err(|e| SpecError::BalanceSchema {
            rule: name.to_string(),
            source: e,
        })?;
        rules.push(
            compile_rule(spec, name, &def).map_err(|reason| SpecError::BadBalance {
                rule: name.to_string(),
                reason: Box::new(reason),
            })?,
        );
    }
    Ok(rules)
}

/// The elements a rule has counted so far, with where each was named.
type Counted = Vec<((LoopId, usize, usize, Option<usize>), String)>;

fn compile_rule(spec: &Spec, name: &str, def: &RawBalance) -> Result<BalanceRule, BalanceError> {
    if name.is_empty() {
        return Err(BalanceError::EmptyName);
    }
    let per = spec
        .loop_id(&def.per)
        .ok_or_else(|| BalanceError::UnknownLoop {
            key: "per".into(),
            name: def.per.clone(),
        })?;
    let mut counted = Counted::new();
    let mut scale = 0;
    let mut sides = [Vec::new(), Vec::new()];
    for ((key, raw), side) in [("target", &def.target), ("sum", &def.sum)]
        .into_iter()
        .zip(sides.iter_mut())
    {
        if raw.is_empty() {
            return Err(BalanceError::NoTerms { key });
        }
        for (i, term) in raw.iter().enumerate() {
            let at = format!("{key}[{i}]");
            side.push(compile_term(
                spec,
                per,
                &at,
                term,
                &mut counted,
                &mut scale,
            )?);
        }
    }
    let [target, sum] = sides;
    Ok(BalanceRule {
        name: name.to_string(),
        per,
        target,
        sum,
        scale,
    })
}

fn compile_term(
    spec: &Spec,
    per: LoopId,
    at: &str,
    term: &RawTerm,
    counted: &mut Counted,
    scale: &mut u8,
) -> Result<BalanceTerm, BalanceError> {
    let loop_id = term_loop(spec, per, at, term.loop_name.as_deref())?;
    let def = spec.get(loop_id);
    let (occurrence, segment) = def
        .occurrences
        .iter()
        .enumerate()
        .find(|(_, occurrence)| occurrence.name == term.occurrence)
        .map(|(index, occurrence)| (index, occurrence.segment.as_slice()))
        .ok_or_else(|| BalanceError::UnknownOccurrence {
            key: format!("{at}.occurrence"),
            loop_name: def.name.clone(),
            occurrence: term.occurrence.clone(),
        })?;
    let read = BalanceTerm {
        loop_id,
        occurrence,
        elements: term.elements.clone(),
        component: term.component,
        negative: false,
    };
    read_elements(spec, at, &read, segment, counted, scale)?;
    Ok(BalanceTerm {
        negative: is_negative(at, term.sign.as_deref())?,
        ..read
    })
}

/// Checks the elements `term` reads, written at `{at}.elements`, from its
/// occurrence of `segment`: there is at least one, each is a decimal and
/// none is read by the rule already. Records each in `counted` and raises
/// `scale` to the largest scale among them.
fn read_elements(
    spec: &Spec,
    at: &str,
    term: &BalanceTerm,
    segment: &[u8],
    counted: &mut Counted,
    scale: &mut u8,
) -> Result<(), BalanceError> {
    if term.elements.is_empty() {
        return Err(BalanceError::NoElements {
            key: format!("{at}.elements"),
        });
    }
    let segment_text = String::from_utf8_lossy(segment);
    for (j, &element) in term.elements.iter().enumerate() {
        let key = format!("{at}.elements[{j}]");
        let place = place(&segment_text, element, term.component);
        let found = element_def(&spec.segments, segment, element, term.component);
        *scale = (*scale).max(decimal_scale(found, &key, &place)?);
        let address = (term.loop_id, term.occurrence, element, term.component);
        if let Some((_, first)) = counted.iter().find(|(seen, _)| *seen == address) {
            return Err(BalanceError::RepeatedValue {
                key,
                first: first.clone(),
                place,
            });
        }
        counted.push((address, key));
    }
    Ok(())
}

/// The loop a term reads, written at `{at}.loop`: `per` when the term names
/// none, otherwise the named loop, which must be `per` or one below it.
fn term_loop(
    spec: &Spec,
    per: LoopId,
    at: &str,
    name: Option<&str>,
) -> Result<LoopId, BalanceError> {
    let Some(name) = name else {
        return Ok(per);
    };
    let key = format!("{at}.loop");
    let id = spec
        .loop_id(name)
        .ok_or_else(|| BalanceError::UnknownLoop {
            key: key.clone(),
            name: name.to_string(),
        })?;
    if id != per && !spec.ancestors(id).contains(&per) {
        return Err(BalanceError::LoopOutsidePer {
            key,
            loop_name: name.to_string(),
            per: spec.loop_name(per).to_string(),
        });
    }
    Ok(id)
}

/// The scale of an element a term reads, named `place` at `key`: the element
/// must be defined and be a simple `R` element or an `R` component.
fn decimal_scale(def: Option<&ElementDef>, key: &str, place: &str) -> Result<u8, BalanceError> {
    match def {
        None => Err(BalanceError::UndefinedElement {
            key: key.to_string(),
            place: place.to_string(),
        }),
        Some(ElementDef {
            kind: ElementType::R { scale },
            composite,
            ..
        }) if composite.is_empty() => Ok(*scale),
        Some(def) => Err(BalanceError::NotDecimal {
            key: key.to_string(),
            place: place.to_string(),
            found: def.kind.code(),
        }),
    }
}

/// Reads a term's `sign`, written at `{at}.sign`: `true` for `-`, `false` for
/// `+` or no sign.
fn is_negative(at: &str, sign: Option<&str>) -> Result<bool, BalanceError> {
    match sign {
        None | Some("+") => Ok(false),
        Some("-") => Ok(true),
        Some(found) => Err(BalanceError::UnknownSign {
            key: format!("{at}.sign"),
            found: found.to_string(),
        }),
    }
}

/// The signed values of one side of a rule, e.g. `sum of CLP04 of 2100
/// "claim_payment_information" - sum of PLB04, PLB06 of transaction
/// "provider_adjustment"`. A term reads as a sum when it adds several
/// elements, reads a loop below `per`, or reads an occurrence that may
/// appear more than once.
pub(crate) fn render_terms(spec: &Spec, per: LoopId, terms: &[BalanceTerm]) -> String {
    let mut out = String::new();
    for (i, term) in terms.iter().enumerate() {
        match (i, term.negative) {
            (0, true) => out.push_str("- "),
            (0, false) => {}
            (_, true) => out.push_str(" - "),
            (_, false) => out.push_str(" + "),
        }
        let def = spec.get(term.loop_id);
        let (segment, occurrence) = def
            .occurrences
            .get(term.occurrence)
            .map(|occurrence| {
                (
                    String::from_utf8_lossy(&occurrence.segment).into_owned(),
                    occurrence.name.as_str(),
                )
            })
            .unwrap_or_default();
        let places: Vec<String> = term
            .elements
            .iter()
            .map(|&element| place(&segment, element, term.component))
            .collect();
        let repeats = term.loop_id != per
            || def
                .occurrences
                .get(term.occurrence)
                .is_none_or(|occurrence| occurrence.max != Some(1));
        if places.len() > 1 || repeats {
            out.push_str("sum of ");
        }
        out.push_str(&places.join(", "));
        out.push_str(&format!(" of {} {occurrence:?}", render_key(&def.name)));
    }
    out
}
