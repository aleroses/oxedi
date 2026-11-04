//! Segment occurrences: the named places a segment takes inside a loop, with
//! their usage, maximum repeat, position, qualifier and own code lists, and
//! their compilation against the `segments` section.

use std::collections::BTreeMap;

use super::error::SpecError;
use super::loops::Trigger;
use super::occurrence_error::{OccurrenceError, place};
use super::raw::{RawOccurrence, RawQualifier};
use super::render::{render_key, render_trigger};
use super::segments::{
    ElementDef, ElementDefError, SegmentDef, compile_codes, holds_code, parse_position,
};
use crate::segment::Segment;

/// Whether an occurrence must appear in every instance of its loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Usage {
    /// Written `required`: every instance of the loop holds it.
    Required,
    /// Written `situational`: an instance may hold it or not.
    Situational,
}

/// The element (or component) and the codes that tell an occurrence apart
/// from the other occurrences of its segment in the same loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Qualifier {
    /// 1-based element position.
    pub element: usize,
    /// 1-based component position, when the qualifier is a component.
    pub component: Option<usize>,
    /// The codes that identify the occurrence, sorted by their bytes.
    pub codes: Vec<String>,
}

impl Qualifier {
    /// `true` when the segment holds one of the codes at the qualifier's place.
    /// A simple element counts as its own first component, and a composite
    /// element named without a component is read at its first component.
    #[inline]
    pub fn matches(&self, segment: &Segment<'_>) -> bool {
        segment
            .leaf(self.element, self.component)
            .is_some_and(|value| holds_code(&self.codes, value))
    }
}

/// One named place a segment takes inside a loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OccurrenceDef {
    /// Name used in the JSON, unique within the loop, e.g. `patient_name`.
    pub name: String,
    /// Segment id, e.g. `NM1`.
    pub segment: Vec<u8>,
    /// Position in the loop; occurrences sharing a position may appear in
    /// any order among themselves, and the occurrence the loop's trigger
    /// opens on has the lowest position of its loop. A child loop whose
    /// trigger occurrence has a higher position than its parent's shares the
    /// parent's position space, so its instances are ordered among the
    /// parent's occurrences (in the built-in specs, the transaction and every
    /// loop below it). A child whose trigger occurrence does not come after
    /// its parent's numbers its own positions (the envelope loops above the
    /// transaction).
    pub pos: usize,
    /// Whether every instance of the loop holds it.
    pub usage: Usage,
    /// Most times it may repeat in one instance of the loop; `None` for no limit.
    pub max: Option<usize>,
    /// What tells it apart from the other occurrences of its segment.
    pub qualifier: Option<Qualifier>,
    /// Code lists of its own, by `(element, component)`, each sorted by bytes.
    pub codes: BTreeMap<(usize, Option<usize>), Vec<String>>,
}

impl OccurrenceDef {
    /// `true` when the segment has this occurrence's id and, when it has a
    /// qualifier, one of its codes.
    pub fn matches(&self, segment: &Segment<'_>) -> bool {
        segment.id == self.segment.as_slice()
            && self
                .qualifier
                .as_ref()
                .is_none_or(|qualifier| qualifier.matches(segment))
    }

    /// `true` when the trigger opens the loop on this occurrence: same
    /// segment and, when the occurrence has a qualifier on an element, a
    /// trigger condition on that element with one of its codes.
    pub(super) fn opens_on(&self, trigger: &Trigger) -> bool {
        if self.segment != trigger.segment {
            return false;
        }
        let Some(qualifier) = &self.qualifier else {
            return true;
        };
        qualifier.component.is_none()
            && trigger.conditions.iter().any(|(position, value)| {
                *position == qualifier.element && holds_code(&qualifier.codes, value)
            })
    }
}

/// The definition of an element or component of `segment`, if any. It takes
/// the `segments` section rather than a [`Spec`](super::Spec) because it also
/// runs while the spec is being built; [`Spec::element_def`](super::Spec::element_def) calls it.
pub(super) fn element_def<'d>(
    segments: &'d BTreeMap<Vec<u8>, SegmentDef>,
    segment: &[u8],
    element: usize,
    component: Option<usize>,
) -> Option<&'d ElementDef> {
    let def = segments.get(segment)?.elements.get(&element)?;
    match component {
        None => Some(def),
        Some(component) => def.composite.get(&component),
    }
}

/// Validates a code list against the element it is for, with the checks an
/// element's own `codes` gets, and returns it sorted by bytes.
fn element_codes(def: &ElementDef, codes: &[String]) -> Result<Vec<String>, ElementDefError> {
    compile_codes(codes, def.kind, def.min, def.max, !def.composite.is_empty())
}

/// Reads a `codes` key: `<element>` or `<element>-<component>`.
fn parse_codes_key(key: &str) -> Option<(usize, Option<usize>)> {
    match key.split_once('-') {
        None => Some((parse_position(key)?, None)),
        Some((element, component)) => {
            Some((parse_position(element)?, Some(parse_position(component)?)))
        }
    }
}

/// Reads a `usage`: `required`, or `situational` (also when absent); the
/// value as written when it is neither.
pub(super) fn parse_usage(usage: Option<&str>) -> Result<Usage, String> {
    match usage {
        None | Some("situational") => Ok(Usage::Situational),
        Some("required") => Ok(Usage::Required),
        Some(found) => Err(found.to_string()),
    }
}

/// Checks the occurrence a loop's trigger opens on, when the loop declares
/// occurrences: there is one, its qualifier selects no code the trigger's
/// conditions leave out, and it comes before every other occurrence.
pub(super) fn check_trigger(
    loop_name: &str,
    occurrences: &[OccurrenceDef],
    trigger: &Trigger,
) -> Result<(), SpecError> {
    if occurrences.is_empty() {
        return Ok(());
    }
    let Some(first) = occurrences.iter().find(|o| o.opens_on(trigger)) else {
        return Err(SpecError::UnmatchedTrigger {
            loop_name: loop_name.to_string(),
            trigger: render_trigger(trigger),
            segment_held: occurrences.iter().any(|o| o.segment == trigger.segment),
        });
    };
    if let Some(qualifier) = &first.qualifier {
        let selected = trigger
            .conditions
            .iter()
            .find(|(position, _)| *position == qualifier.element)
            .map(|(_, value)| value.as_slice());
        if let Some(code) = qualifier
            .codes
            .iter()
            .find(|code| Some(code.as_bytes()) != selected)
        {
            return Err(SpecError::TriggerQualifierWider {
                loop_name: loop_name.to_string(),
                occurrence: first.name.clone(),
                trigger: render_trigger(trigger),
                code: code.clone(),
            });
        }
    }
    if let Some(other) = occurrences
        .iter()
        .find(|o| o.name != first.name && o.pos <= first.pos)
    {
        return Err(SpecError::TriggerNotFirst {
            loop_name: loop_name.to_string(),
            occurrence: first.name.clone(),
            pos: first.pos,
            other: other.name.clone(),
            other_pos: other.pos,
        });
    }
    Ok(())
}

/// Compiles the occurrences of the loop `loop_name` against the segment
/// definitions, sorted by position (ties keep name order), and checks that
/// every two occurrences of one segment can be told apart.
pub(super) fn compile_occurrences(
    loop_name: &str,
    raw: &BTreeMap<String, RawOccurrence>,
    segments: &BTreeMap<Vec<u8>, SegmentDef>,
) -> Result<Vec<OccurrenceDef>, SpecError> {
    let mut occurrences = Vec::with_capacity(raw.len());
    for (name, def) in raw {
        occurrences.push(compile_occurrence(loop_name, name, def, segments)?);
    }
    occurrences.sort_by_key(|occurrence| occurrence.pos);
    for (i, first) in occurrences.iter().enumerate() {
        for second in occurrences.iter().skip(i + 1) {
            if first.segment != second.segment {
                continue;
            }
            let fail = |reason| SpecError::BadOccurrence {
                loop_name: loop_name.to_string(),
                occurrence: second.name.clone(),
                reason: Box::new(reason),
            };
            let segment = String::from_utf8_lossy(&second.segment).into_owned();
            let (Some(a), Some(b)) = (&first.qualifier, &second.qualifier) else {
                return Err(fail(OccurrenceError::Indistinct {
                    other: first.name.clone(),
                    segment,
                }));
            };
            if (a.element, a.component) != (b.element, b.component) {
                return Err(fail(OccurrenceError::QualifierPlaces {
                    other: first.name.clone(),
                    place: place(&segment, b.element, b.component),
                    other_place: place(&segment, a.element, a.component),
                }));
            }
            if let Some(code) = a.codes.iter().find(|code| b.codes.contains(code)) {
                return Err(fail(OccurrenceError::SharedCode {
                    other: first.name.clone(),
                    segment,
                    code: code.clone(),
                }));
            }
        }
    }
    Ok(occurrences)
}

fn compile_occurrence(
    loop_name: &str,
    name: &str,
    def: &RawOccurrence,
    segments: &BTreeMap<Vec<u8>, SegmentDef>,
) -> Result<OccurrenceDef, SpecError> {
    let fail = |reason| SpecError::BadOccurrence {
        loop_name: loop_name.to_string(),
        occurrence: name.to_string(),
        reason: Box::new(reason),
    };
    if name.is_empty() {
        return Err(fail(OccurrenceError::EmptyName));
    }
    if def.segment.is_empty() {
        return Err(SpecError::EmptySegmentId {
            loop_name: Some(loop_name.to_string()),
            key: format!("occurrences.{}.segment", render_key(name)),
        });
    }
    let segment = def.segment.as_bytes();
    let usage = parse_usage(def.usage.as_deref())
        .map_err(|found| fail(OccurrenceError::UnknownUsage { found }))?;
    if def.max == Some(0) {
        return Err(fail(OccurrenceError::ZeroMax));
    }
    let undefined = |key: String, element, component| OccurrenceError::UndefinedElement {
        key,
        segment: def.segment.clone(),
        element,
        component,
    };
    let qualifier = match &def.qualifier {
        None => None,
        Some(RawQualifier {
            element,
            component,
            codes,
        }) => {
            let target = element_def(segments, segment, *element, *component)
                .ok_or_else(|| fail(undefined("qualifier".into(), *element, *component)))?;
            if codes.is_empty() {
                return Err(fail(OccurrenceError::EmptyCodes {
                    key: "qualifier.codes".into(),
                }));
            }
            let codes = element_codes(target, codes).map_err(|reason| {
                fail(OccurrenceError::BadCodes {
                    key: "qualifier.codes".into(),
                    reason,
                })
            })?;
            if let Some(code) = codes
                .iter()
                .find(|code| target.rejects_code(code.as_bytes()))
            {
                return Err(fail(OccurrenceError::QualifierCodeOutsideElement {
                    code: code.clone(),
                    place: place(&def.segment, *element, *component),
                    allowed: target.codes.clone(),
                }));
            }
            Some(Qualifier {
                element: *element,
                component: *component,
                codes,
            })
        }
    };
    let mut codes = BTreeMap::new();
    for (key, list) in &def.codes {
        let (element, component) = parse_codes_key(key)
            .ok_or_else(|| fail(OccurrenceError::BadCodesKey { key: key.clone() }))?;
        if qualifier
            .as_ref()
            .is_some_and(|q| (q.element, q.component) == (element, component))
        {
            return Err(fail(OccurrenceError::QualifierInCodes { key: key.clone() }));
        }
        let at = format!("codes.{key}");
        let target = element_def(segments, segment, element, component)
            .ok_or_else(|| fail(undefined(at.clone(), element, component)))?;
        if list.is_empty() {
            return Err(fail(OccurrenceError::EmptyCodes { key: at }));
        }
        let list = element_codes(target, list)
            .map_err(|reason| fail(OccurrenceError::BadCodes { key: at, reason }))?;
        codes.insert((element, component), list);
    }
    Ok(OccurrenceDef {
        name: name.to_string(),
        segment: segment.to_vec(),
        pos: def.pos,
        usage,
        max: def.max,
        qualifier,
        codes,
    })
}
