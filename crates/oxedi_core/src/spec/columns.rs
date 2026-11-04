//! Compilation of one table column into its value source.

use super::Spec;
use super::error::SpecError;
use super::loops::LoopId;
use super::occurrences::OccurrenceDef;
use super::raw::{RawColumn, RawPick};
use super::render::render_key;
use super::segments::{ROW_COLUMN, SEGMENT_COLUMN, parse_position};
use super::table_error::{AnchorPositions, LoopSegments, TableDefError};
use super::tables::{ColumnSource, Pick, Repeat};

/// Compiles the column `column` of table `table`, anchored in `anchors` (on
/// `anchor_segment`, when the table has one).
pub(super) fn compile_column(
    spec: &Spec,
    table: &str,
    column: &str,
    raw: &RawColumn,
    anchors: &[LoopId],
    anchor_segment: Option<&[u8]>,
    repeat: Option<Repeat>,
) -> Result<ColumnSource, SpecError> {
    let fail = |reason| SpecError::BadTable {
        table: table.to_string(),
        column: Some(column.to_string()),
        reason,
    };
    if column.is_empty() {
        return Err(fail(TableDefError::EmptyName {
            what: "column name",
        }));
    }
    if column == ROW_COLUMN || column == SEGMENT_COLUMN {
        return Err(fail(TableDefError::ReservedName {
            name: column.to_string(),
        }));
    }
    let found = usize::from(raw.element.is_some())
        + usize::from(raw.group_element.is_some())
        + usize::from(raw.segment_index);
    if found != 1 {
        return Err(fail(TableDefError::SourceCount { found }));
    }
    if raw.segment_index && raw.component.is_some() {
        return Err(fail(TableDefError::ComponentOnIndex));
    }
    if raw.element == Some(0) {
        return Err(fail(TableDefError::ZeroPosition { key: "element" }));
    }
    if raw.component == Some(0) {
        return Err(fail(TableDefError::ZeroPosition { key: "component" }));
    }
    if let Some(anchor) = anchor_segment {
        let keys = [
            ("segment", raw.segment.as_ref().map(json)),
            ("loop", raw.loop_name.as_ref().map(json)),
            (
                "where",
                (!raw.conditions.is_empty()).then(|| json(&raw.conditions)),
            ),
            ("occurrence", raw.occurrence.as_ref().map(json)),
            ("pick", raw.pick.as_ref().map(RawPick::written)),
        ];
        if let Some((key, Some(written))) = keys.into_iter().find(|(_, value)| value.is_some()) {
            return Err(fail(TableDefError::AnchorSegmentOnly {
                key,
                anchor_segment: String::from_utf8_lossy(anchor).into_owned(),
                written,
            }));
        }
    }
    if let Some(offset) = raw.group_element {
        let Some(repeat) = repeat else {
            return Err(fail(TableDefError::GroupWithoutRepeat));
        };
        if offset >= repeat.step {
            return Err(fail(TableDefError::OffsetBeyondStep {
                offset,
                step: repeat.step,
            }));
        }
        return Ok(ColumnSource::GroupElement {
            offset,
            component: raw.component,
        });
    }
    let read = match anchor_segment {
        Some(anchor) => Read {
            loop_id: None,
            segment: anchor.to_vec(),
            conditions: Vec::new(),
            occurrence: None,
            pick: Pick::First,
        },
        None => {
            let loop_id = reading_loop(spec, raw, anchors).map_err(fail)?;
            match &raw.occurrence {
                Some(name) => by_occurrence(spec, raw, name, loop_id, anchors).map_err(fail)?,
                None => by_segment(spec, table, column, raw, loop_id, anchors)?,
            }
        }
    };
    if let Some(id) = read.loop_id {
        check_reached(spec, id, anchors, &read).map_err(fail)?;
    }
    let Read {
        loop_id,
        segment,
        conditions,
        occurrence,
        pick,
    } = read;
    Ok(match raw.element {
        Some(element) => ColumnSource::Element {
            loop_id,
            segment,
            conditions,
            occurrence,
            pick,
            element,
            component: raw.component,
        },
        None => ColumnSource::SegmentIndex {
            loop_id,
            segment,
            conditions,
            occurrence,
            pick,
        },
    })
}

/// What a column of a table without `segment` reads, before its value.
struct Read {
    loop_id: Option<LoopId>,
    segment: Vec<u8>,
    conditions: Vec<(usize, Vec<u8>)>,
    occurrence: Option<String>,
    pick: Pick,
}

/// A value as JSON, for an error that shows what the spec wrote.
fn json<T: serde::Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// The column's `loop`: `None` when it names none or names the anchor loop
/// itself; otherwise a loop in line with each anchor loop. Each anchor is
/// checked on its own here; that the loop is then inside every anchor or
/// above every one follows from anchors never nesting (`NestedAnchors`) and
/// from this check rejecting a loop out of line with any anchor
/// (`UnrelatedLoop`).
fn reading_loop(
    spec: &Spec,
    raw: &RawColumn,
    anchors: &[LoopId],
) -> Result<Option<LoopId>, TableDefError> {
    let Some(name) = &raw.loop_name else {
        return Ok(None);
    };
    let id = spec
        .loop_id(name)
        .ok_or_else(|| TableDefError::UnknownLoop { name: name.clone() })?;
    let in_line = |anchor: LoopId| {
        id == anchor || spec.ancestors(id).contains(&anchor) || spec.ancestors(anchor).contains(&id)
    };
    if let Some(&anchor) = anchors.iter().find(|&&anchor| !in_line(anchor)) {
        return Err(TableDefError::UnrelatedLoop {
            loop_name: name.clone(),
            anchor: spec.loop_name(anchor).to_string(),
        });
    }
    Ok((!anchors.contains(&id)).then_some(id))
}

/// The occurrence a loop's trigger opens on, if the loop declares one.
fn trigger_occurrence(spec: &Spec, id: LoopId) -> Option<&OccurrenceDef> {
    let def = spec.get(id);
    def.trigger_occurrence()
        .and_then(|index| def.occurrences.get(index))
}

/// Rejects a source in loop `id` above an anchor whose segments all come
/// after the position where the anchor opens: they arrive once the anchor's
/// rows are appended, so the column would never fill. Positions compare
/// only when every loop from `id` down to the anchor shares one position
/// space (each trigger occurrence after its parent's).
fn check_reached(
    spec: &Spec,
    id: LoopId,
    anchors: &[LoopId],
    read: &Read,
) -> Result<(), TableDefError> {
    let def = spec.get(id);
    let candidates: Vec<&OccurrenceDef> = match &read.occurrence {
        Some(name) => def.occurrences.iter().filter(|o| o.name == *name).collect(),
        None => def
            .occurrences
            .iter()
            .filter(|o| o.segment == read.segment)
            .collect(),
    };
    let Some(earliest) = candidates.into_iter().min_by_key(|o| o.pos) else {
        return Ok(());
    };
    for &anchor in anchors {
        if !spec.ancestors(anchor).contains(&id) {
            continue;
        }
        let Some(anchor_pos) = shared_trigger_pos(spec, id, anchor) else {
            continue;
        };
        if earliest.pos > anchor_pos {
            return Err(TableDefError::AfterAnchor {
                occurrence: earliest.name.clone(),
                positions: Box::new(AnchorPositions {
                    loop_name: def.name.clone(),
                    pos: earliest.pos,
                    anchor: spec.loop_name(anchor).to_string(),
                    anchor_pos,
                }),
            });
        }
    }
    Ok(())
}

/// The position of `anchor`'s trigger occurrence when every loop from
/// `above` down to `anchor` shares one position space; `None` otherwise.
fn shared_trigger_pos(spec: &Spec, above: LoopId, anchor: LoopId) -> Option<usize> {
    let anchor_pos = trigger_occurrence(spec, anchor)?.pos;
    let mut child = anchor;
    let mut child_pos = anchor_pos;
    while child != above {
        let parent = spec.get(child).parent?;
        let parent_pos = trigger_occurrence(spec, parent)?.pos;
        if child_pos <= parent_pos {
            return None;
        }
        child = parent;
        child_pos = parent_pos;
    }
    Some(anchor_pos)
}

/// The pick as compiled: `first`, `last` or a position from 1.
fn compile_pick(raw: Option<&RawPick>) -> Result<Pick, TableDefError> {
    match raw {
        None => Ok(Pick::First),
        Some(RawPick::Named(name)) if name == "first" => Ok(Pick::First),
        Some(RawPick::Named(name)) if name == "last" => Ok(Pick::Last),
        Some(RawPick::Nth(nth)) if *nth > 0 => Ok(Pick::Nth(*nth)),
        Some(other) => Err(TableDefError::BadPick {
            written: other.written(),
        }),
    }
}

/// A source that names the occurrence `name` of the reading loops: `loop_id`,
/// or every anchor loop.
fn by_occurrence(
    spec: &Spec,
    raw: &RawColumn,
    name: &str,
    loop_id: Option<LoopId>,
    anchors: &[LoopId],
) -> Result<Read, TableDefError> {
    if let Some(segment) = &raw.segment {
        return Err(TableDefError::OccurrenceAndSegment {
            key: "segment",
            written: json(segment),
        });
    }
    if !raw.conditions.is_empty() {
        return Err(TableDefError::OccurrenceAndSegment {
            key: "where",
            written: json(&raw.conditions),
        });
    }
    let pick = compile_pick(raw.pick.as_ref())?;
    let readers = loop_id.map_or_else(|| anchors.to_vec(), |id| vec![id]);
    let mut first: Option<(LoopId, &[u8])> = None;
    for reader in readers {
        let def = spec.get(reader);
        let Some(found) = def.occurrences.iter().find(|o| o.name == name) else {
            let mut known: Box<[String]> = def.occurrences.iter().map(|o| o.name.clone()).collect();
            known.sort();
            return Err(TableDefError::UnknownOccurrence {
                loop_name: def.name.clone(),
                occurrence: name.to_string(),
                known,
            });
        };
        match first {
            None => first = Some((reader, &found.segment)),
            Some((loop_one, segment)) if segment != found.segment.as_slice() => {
                return Err(TableDefError::OccurrenceSegments {
                    occurrence: name.to_string(),
                    segments: Box::new(LoopSegments {
                        first_loop: spec.loop_name(loop_one).to_string(),
                        first_segment: String::from_utf8_lossy(segment).into_owned(),
                        loop_name: def.name.clone(),
                        segment: String::from_utf8_lossy(&found.segment).into_owned(),
                    }),
                });
            }
            Some(_) => {}
        }
        if raw.pick.is_some() {
            match (pick, found.max) {
                (_, Some(1)) => {
                    return Err(TableDefError::PickOnSingle {
                        loop_name: def.name.clone(),
                        occurrence: name.to_string(),
                        pick,
                    });
                }
                (Pick::Nth(nth), Some(max)) if nth > max => {
                    return Err(TableDefError::PickBeyondMax {
                        nth,
                        loop_name: def.name.clone(),
                        occurrence: name.to_string(),
                        max,
                    });
                }
                _ => {}
            }
        }
    }
    Ok(Read {
        loop_id,
        segment: first
            .map(|(_, segment)| segment.to_vec())
            .unwrap_or_default(),
        conditions: Vec::new(),
        occurrence: Some(name.to_string()),
        pick,
    })
}

/// A source that names a segment and the values some of its elements hold.
fn by_segment(
    spec: &Spec,
    table: &str,
    column: &str,
    raw: &RawColumn,
    loop_id: Option<LoopId>,
    anchors: &[LoopId],
) -> Result<Read, SpecError> {
    let fail = |reason| SpecError::BadTable {
        table: table.to_string(),
        column: Some(column.to_string()),
        reason,
    };
    if let Some(pick) = &raw.pick {
        return Err(fail(TableDefError::PickNeedsOccurrence {
            written: pick.written(),
        }));
    }
    let segment = match raw.segment.as_deref() {
        None => return Err(fail(TableDefError::NeedsSegment)),
        Some("") => {
            return Err(SpecError::EmptySegmentId {
                loop_name: None,
                key: format!(
                    "tables.{}.columns.{}.segment",
                    render_key(table),
                    render_key(column)
                ),
            });
        }
        Some(id) => id.as_bytes().to_vec(),
    };
    let mut conditions = Vec::with_capacity(raw.conditions.len());
    for (key, value) in &raw.conditions {
        let position = parse_position(key)
            .ok_or_else(|| fail(TableDefError::BadPosition { key: key.clone() }))?;
        conditions.push((position, value.as_bytes().to_vec()));
    }
    conditions.sort();
    let readers = loop_id.map_or_else(|| anchors.to_vec(), |id| vec![id]);
    check_held(spec, &readers, &segment).map_err(fail)?;
    Ok(Read {
        loop_id,
        segment,
        conditions,
        occurrence: None,
        pick: Pick::First,
    })
}

/// Fails with the first loop of `readers` that neither triggers on `segment`
/// nor holds it.
pub(super) fn check_held(
    spec: &Spec,
    readers: &[LoopId],
    segment: &[u8],
) -> Result<(), TableDefError> {
    for &id in readers {
        let def = spec.get(id);
        if def.trigger.segment != segment && !def.accepts(segment) {
            return Err(TableDefError::SegmentNotHeld {
                segment: String::from_utf8_lossy(segment).into_owned(),
                loop_name: def.name.clone(),
            });
        }
    }
    Ok(())
}
