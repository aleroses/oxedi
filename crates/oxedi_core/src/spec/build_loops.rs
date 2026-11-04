//! The compilation of one loop: the checks of its own keys, its parent, its
//! trigger conditions and its control, the parent cycle check over every
//! loop, and the segments its occurrences hold.

use std::collections::{BTreeMap, BTreeSet};

use super::error::SpecError;
use super::loops::{Control, ControlCount, ControlError, LoopDef, LoopId, Trigger};
use super::occurrences::parse_usage;
use super::raw::{RawControl, RawLoop};
use super::segments::parse_position;

/// A check of a loop's own keys: the error when the loop breaks it.
type HeaderRule = fn(&str, &RawLoop) -> Option<SpecError>;

/// The checks of a loop's keys that need nothing but the loop, in the order they run.
const HEADER_RULES: [HeaderRule; 4] = [empty_trigger, empty_end, zero_max, end_is_trigger];

fn empty_trigger(name: &str, def: &RawLoop) -> Option<SpecError> {
    def.trigger
        .segment
        .is_empty()
        .then(|| empty_segment_id(name, "trigger.segment"))
}

fn empty_end(name: &str, def: &RawLoop) -> Option<SpecError> {
    (def.end.as_deref() == Some("")).then(|| empty_segment_id(name, "end"))
}

fn empty_segment_id(name: &str, key: &str) -> SpecError {
    SpecError::EmptySegmentId {
        loop_name: Some(name.to_string()),
        key: key.to_string(),
    }
}

fn zero_max(name: &str, def: &RawLoop) -> Option<SpecError> {
    (def.max == Some(0)).then(|| SpecError::ZeroLoopMax {
        loop_name: name.to_string(),
    })
}

fn end_is_trigger(name: &str, def: &RawLoop) -> Option<SpecError> {
    (def.end.as_deref() == Some(def.trigger.segment.as_str())).then(|| SpecError::EndIsTrigger {
        loop_name: name.to_string(),
        segment: def.trigger.segment.clone(),
    })
}

/// A loop as written, without its occurrences and children, which later steps add.
pub(super) fn compile_loop(
    name: &str,
    def: &RawLoop,
    ids: &BTreeMap<&str, LoopId>,
) -> Result<LoopDef, SpecError> {
    if let Some(error) = HEADER_RULES.iter().find_map(|rule| rule(name, def)) {
        return Err(error);
    }
    let usage = parse_usage(def.usage.as_deref()).map_err(|found| SpecError::UnknownLoopUsage {
        loop_name: name.to_string(),
        found,
    })?;
    Ok(LoopDef {
        name: name.to_string(),
        parent: parent_id(name, def, ids)?,
        trigger: Trigger {
            segment: def.trigger.segment.as_bytes().to_vec(),
            conditions: compile_conditions(name, &def.trigger.conditions)?,
        },
        occurrences: Vec::new(),
        usage,
        max: def.max,
        segments: Vec::new(),
        end: def.end.as_ref().map(|s| s.as_bytes().to_vec()),
        control: loop_control(name, def)?,
        children: Vec::new(),
    })
}

fn parent_id(
    name: &str,
    def: &RawLoop,
    ids: &BTreeMap<&str, LoopId>,
) -> Result<Option<LoopId>, SpecError> {
    let Some(parent) = &def.parent else {
        return Ok(None);
    };
    match ids.get(parent.as_str()) {
        Some(&id) => Ok(Some(id)),
        None => Err(SpecError::UnknownParent {
            loop_name: name.to_string(),
            parent: parent.clone(),
        }),
    }
}

/// The trigger's `where` conditions as 1-based positions, sorted.
fn compile_conditions(
    name: &str,
    raw: &BTreeMap<String, String>,
) -> Result<Vec<(usize, Vec<u8>)>, SpecError> {
    let mut conditions = raw
        .iter()
        .map(|(position, value)| {
            parse_position(position)
                .map(|parsed| (parsed, value.as_bytes().to_vec()))
                .ok_or_else(|| SpecError::BadPosition {
                    loop_name: name.to_string(),
                    position: position.clone(),
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    conditions.sort();
    Ok(conditions)
}

fn loop_control(name: &str, def: &RawLoop) -> Result<Option<Control>, SpecError> {
    def.control
        .as_ref()
        .map(|raw| {
            compile_control(raw, def.end.is_some()).map_err(|reason| SpecError::BadControl {
                loop_name: name.to_string(),
                reason,
            })
        })
        .transpose()
}

/// The segment ids a loop's occurrences hold, other than the one it opens
/// on, each once, in occurrence order.
pub(super) fn held_segments(def: &LoopDef) -> Vec<Vec<u8>> {
    let mut seen = BTreeSet::new();
    def.occurrences
        .iter()
        .filter(|occurrence| !occurrence.opens_on(&def.trigger))
        .filter(|occurrence| seen.insert(occurrence.segment.as_slice()))
        .map(|occurrence| occurrence.segment.clone())
        .collect()
}

/// Validates a loop's `control`; `has_end` says whether the loop declares an end segment.
fn compile_control(raw: &RawControl, has_end: bool) -> Result<Control, ControlError> {
    if !has_end {
        return Err(ControlError::NoEnd);
    }
    let positions = [
        ("opener_element", raw.opener_element),
        ("closer_element", raw.closer_element),
        ("count_element", raw.count_element),
    ];
    if let Some((key, _)) = positions.iter().find(|(_, position)| *position == 0) {
        return Err(ControlError::ZeroPosition { key });
    }
    let count = match raw.count.as_str() {
        "segments" => ControlCount::Segments,
        "children" => ControlCount::Children,
        _ => {
            return Err(ControlError::UnknownCount {
                found: raw.count.clone(),
            });
        }
    };
    Ok(Control {
        opener_element: raw.opener_element,
        closer_element: raw.closer_element,
        count_element: raw.count_element,
        count,
    })
}

/// Fails with the first parent cycle found, walking from each loop in spec
/// order. The members start at the loop the walk reaches twice.
pub(super) fn check_cycles(loops: &[LoopDef]) -> Result<(), SpecError> {
    for start in 0..loops.len() {
        let mut walk: Vec<LoopId> = Vec::new();
        let mut current = Some(LoopId(start));
        while let Some(id) = current {
            if let Some(repeat) = walk.iter().position(|&seen| seen == id) {
                let members = walk[repeat..]
                    .iter()
                    .map(|&member| loops[member.0].name.clone())
                    .collect();
                return Err(SpecError::Cycle { members });
            }
            walk.push(id);
            current = loops[id.0].parent;
        }
    }
    Ok(())
}
