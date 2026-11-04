//! Row filling: columns read from captured segments, the rows appended to the
//! tables, the diagnostic helpers and the text read of an element.

use crate::column::{Cell, CellError, RowError, Table};
use crate::diagnostic::{Diagnostic, LoopRef, Rule};
use crate::element::Element;
use crate::segment::Segment;
use crate::spec::{ColumnSource, LoopId, Pick};
use crate::value::{ColumnType, Parsed};

use super::check::Checked;
use super::plan::Watcher;
use super::{Projector, Row, Slot};

impl<'s> Projector<'s> {
    /// Fills the columns that read this segment: in the open rows, or in
    /// the values carried for rows still to open. `watchers` come from the
    /// plan of the segment's id, so only the occurrence the segment matched
    /// (or the column's conditions) and the pick are left to check.
    pub(super) fn fill(
        &mut self,
        watchers: &[Watcher],
        segment: &Segment<'_>,
        joined: &mut Vec<u8>,
    ) {
        let spec = self.spec;
        for watcher in watchers {
            let state = &mut self.tables[watcher.table];
            if !state.collect {
                continue;
            }
            let (slot, seen, bytes) = match watcher.carried {
                Some(at) => {
                    let Some(carried) = state.carried.get_mut(at) else {
                        continue;
                    };
                    (&mut carried.slot, &mut carried.seen, &mut carried.bytes)
                }
                None => {
                    let row = &mut state.row;
                    let (Some(slot), Some(seen), true) = (
                        row.cells.get_mut(watcher.column),
                        row.seen.get_mut(watcher.column),
                        state.open,
                    ) else {
                        continue;
                    };
                    (slot, seen, &mut row.bytes)
                }
            };
            if watcher.pick == Pick::First && *slot != Slot::Unset {
                continue;
            }
            let Some((_, source)) = spec.tables()[watcher.table].columns.get(watcher.column) else {
                continue;
            };
            let (conditions, at) = match source {
                ColumnSource::Element {
                    conditions,
                    element,
                    component,
                    ..
                } => (conditions, Some((*element, *component))),
                ColumnSource::SegmentIndex { conditions, .. } => (conditions, None),
                ColumnSource::GroupElement { .. } => continue,
            };
            let matches = match watcher.occurrence {
                Some(wanted) => self.matched == Some(wanted),
                None => segment.holds(conditions),
            };
            if !matches {
                continue;
            }
            if let Pick::Nth(nth) = watcher.pick {
                *seen = seen.saturating_add(1);
                if *seen != nth {
                    continue;
                }
            }
            if watcher.carried.is_some() {
                bytes.clear();
            }
            *slot = match at {
                Some((element, component)) => {
                    let kind = state
                        .kinds
                        .get(watcher.column)
                        .copied()
                        .unwrap_or(ColumnType::Binary);
                    let at = Place {
                        element,
                        component,
                        kind,
                    };
                    read(&self.checked, segment, at, self.separator, joined, bytes)
                }
                None => i64::try_from(segment.index).map_or(Slot::Null, Slot::Int),
            };
        }
    }

    /// Appends the rows of the tables anchored on this segment.
    pub(super) fn segment_rows(&mut self, id: LoopId, segment: &Segment<'_>, joined: &mut Vec<u8>) {
        let spec = self.spec;
        for slot in 0..self.segment_tables[id.index()].len() {
            let index = self.segment_tables[id.index()][slot];
            let def = &spec.tables()[index];
            if !self.tables[index].kept || def.segment.as_deref() != Some(segment.id) {
                continue;
            }
            let Some(repeat) = def.repeat else {
                self.segment_row(index, segment, None, joined);
                continue;
            };
            let mut start = Some(repeat.from);
            while let Some(position) = start.filter(|&at| at <= segment.elements.len()) {
                if segment.element(position).is_some_and(has_content) {
                    self.segment_row(index, segment, Some(position), joined);
                }
                start = position.checked_add(repeat.step);
            }
        }
    }

    /// Appends one row of a table anchored on a segment; `group` is the
    /// position of the first element of the row's group.
    fn segment_row(
        &mut self,
        index: usize,
        segment: &Segment<'_>,
        group: Option<usize>,
        joined: &mut Vec<u8>,
    ) {
        let spec = self.spec;
        let def = &spec.tables()[index];
        let mut row = std::mem::take(&mut self.tables[index].row);
        row.parents.clear();
        row.parents
            .extend(def.ancestors.iter().map(|&above| self.open_row(above)));
        row.cells.clear();
        row.bytes.clear();
        for (column, (_, source)) in def.columns.iter().enumerate() {
            let kind = self.tables[index]
                .kinds
                .get(column)
                .copied()
                .unwrap_or(ColumnType::Binary);
            let at = match source {
                ColumnSource::Element {
                    element, component, ..
                } => Some((*element, *component)),
                ColumnSource::GroupElement { offset, component } => group
                    .and_then(|start| start.checked_add(*offset))
                    .map(|element| (element, *component)),
                ColumnSource::SegmentIndex { .. } => {
                    row.cells
                        .push(i64::try_from(segment.index).map_or(Slot::Null, Slot::Int));
                    continue;
                }
            };
            let slot = match at {
                Some((element, component)) => read(
                    &self.checked,
                    segment,
                    Place {
                        element,
                        component,
                        kind,
                    },
                    self.separator,
                    joined,
                    &mut row.bytes,
                ),
                None => Slot::Null,
            };
            row.cells.push(slot);
        }
        row.segment = segment.index;
        if self.tables[index].masked {
            self.mask_row(index, &mut row);
        }
        let state = &mut self.tables[index];
        row.ordinal = state.next;
        state.next = state.next.saturating_add(1);
        let dropped = append(&mut state.table, &row, &mut self.cells);
        state.row = row;
        self.report_dropped(dropped, index, segment.index);
    }

    /// Reports each text value an append could not store.
    pub(super) fn report_dropped(&mut self, dropped: Vec<Dropped>, table: usize, segment: usize) {
        for (column, bytes, value) in dropped {
            let rule = Rule::ValueDropped {
                table: self.spec.tables()[table].name.clone(),
                column,
                bytes,
            };
            self.push_diagnostic(rule, segment, None, None, &value);
        }
    }

    pub(super) fn report(
        &mut self,
        rule: Rule,
        segment: usize,
        element: usize,
        component: Option<usize>,
        datum: &[u8],
    ) {
        self.push_diagnostic(rule, segment, Some(element), component, datum);
    }

    fn push_diagnostic(
        &mut self,
        rule: Rule,
        segment: usize,
        element: Option<usize>,
        component: Option<usize>,
        datum: &[u8],
    ) {
        let spec = self.spec;
        let path = self
            .open
            .iter()
            .map(|&(id, ordinal)| LoopRef {
                name: spec.loop_name(id).to_string(),
                ordinal,
            })
            .collect();
        self.diagnostics.push(Diagnostic::new(
            rule,
            Some(segment),
            element,
            component,
            path,
            datum.to_vec(),
        ));
    }
}

/// `true` when an element has a non-empty value or component.
fn has_content(element: &Element<'_>) -> bool {
    match element {
        Element::Simple(value) => !value.is_empty(),
        Element::Composite(parts) => parts.iter().any(|part| !part.is_empty()),
    }
}

/// The text of an element, or of one of its components. An element read
/// whole that the file split into components is written back into
/// `joined` with the component separator between them; component 1 of a
/// simple element is the element itself.
pub(super) fn leaf_text<'a>(
    segment: &'a Segment<'_>,
    element: usize,
    component: Option<usize>,
    separator: u8,
    joined: &'a mut Vec<u8>,
) -> &'a [u8] {
    match component {
        None => segment.text(element, separator, joined),
        Some(_) => segment.leaf(element, component),
    }
    .unwrap_or_default()
}

/// `true` when the segment has the element, or the component, a column
/// reads, even if it is empty. Component 1 of a simple element is the
/// element itself; no other component of it is present.
fn is_present(segment: &Segment<'_>, at: Place) -> bool {
    match at.component {
        None => segment.element(at.element).is_some(),
        Some(_) => segment.leaf(at.element, at.component).is_some(),
    }
}

/// Where a column reads inside a segment, and as what.
#[derive(Debug, Clone, Copy)]
struct Place {
    element: usize,
    component: Option<usize>,
    kind: ColumnType,
}

/// The column value at `at`: the checked value when the definition there
/// maps to the column's type, otherwise the text parsed as that type (an
/// element the spec does not define, or a group whose definition differs).
/// Text is copied into `bytes`.
fn read(
    checked: &[Checked],
    segment: &Segment<'_>,
    at: Place,
    separator: u8,
    joined: &mut Vec<u8>,
    bytes: &mut Vec<u8>,
) -> Slot {
    let known = checked
        .iter()
        .find(|c| (c.element, c.component, c.kind) == (at.element, at.component, at.kind));
    let value = match known {
        Some(checked) => checked.value,
        None => {
            let text = leaf_text(segment, at.element, at.component, separator, joined);
            at.kind.parse(text)
        }
    };
    match value {
        Some(Parsed::Text) => {
            let text = leaf_text(segment, at.element, at.component, separator, joined);
            let start = bytes.len();
            bytes.extend_from_slice(text);
            Slot::Bytes(start, bytes.len())
        }
        Some(Parsed::Int(value)) => Slot::Int(value),
        Some(Parsed::Decimal(value)) => Slot::Decimal(value),
        Some(Parsed::Date(value)) => Slot::Date(value),
        Some(Parsed::Time(value)) => Slot::Time(value),
        None => no_value(segment, at, bytes),
    }
}

/// The slot of an element with no value: a text column tells a present but
/// empty value (`""`) from an absent one (null); other types have no empty
/// value.
fn no_value(segment: &Segment<'_>, at: Place, bytes: &[u8]) -> Slot {
    if at.kind == ColumnType::Binary && is_present(segment, at) {
        Slot::Bytes(bytes.len(), bytes.len())
    } else {
        Slot::Null
    }
}

/// A text value an append could not store: its column, the byte total the
/// column would have reached, and the value.
type Dropped = (String, usize, Vec<u8>);

/// Appends a collected row: its number, its anchor segment, the rows above
/// it, then its columns; a column no segment matched is null. A text value
/// that would take its column past what `i32` offsets address is stored as
/// null and returned with its column name, the byte total it would have
/// produced and the value itself, so row numbers stay aligned and nothing
/// disappears unreported.
pub(super) fn append(
    table: &mut Table,
    row: &Row,
    scratch: &mut Vec<Cell<'static>>,
) -> Vec<Dropped> {
    let index = |value: usize| i64::try_from(value).map_or(Cell::Null, Cell::Int64);
    // The scratch vector is empty; collecting an empty iterator out of it
    // gives its allocation back under the row's lifetime when std reuses the
    // allocation in place; otherwise this is an ordinary allocation.
    scratch.clear();
    let mut cells: Vec<Cell<'_>> = std::mem::take(scratch)
        .into_iter()
        .map_while(|_| None)
        .collect();
    cells.reserve(2 + row.parents.len() + row.cells.len());
    cells.push(index(row.ordinal));
    cells.push(index(row.segment));
    cells.extend(
        row.parents
            .iter()
            .map(|parent| parent.map_or(Cell::Null, index)),
    );
    cells.extend(row.cells.iter().map(|slot| match *slot {
        Slot::Unset | Slot::Null => Cell::Null,
        Slot::Bytes(start, end) => row.bytes.get(start..end).map_or(Cell::Null, Cell::Binary),
        Slot::Int(value) => Cell::Int64(value),
        Slot::Decimal(value) => Cell::Decimal128(value),
        Slot::Date(value) => Cell::Date32(value),
        Slot::Time(value) => Cell::Time32(value),
    }));
    let mut dropped = Vec::new();
    // Every cell has its column's type, so only text can fail to fit; each
    // retry nulls one cell, and nulls always fit.
    while let Err(error) = table.push_row(&cells) {
        let RowError::Cell {
            column,
            source: CellError::BinaryOverflow { bytes },
            ..
        } = error
        else {
            break;
        };
        let at = table.columns().iter().position(|(name, _)| *name == column);
        let Some(cell) = at.and_then(|at| cells.get_mut(at)) else {
            break;
        };
        let value = match std::mem::replace(cell, Cell::Null) {
            Cell::Binary(value) => value.to_vec(),
            _ => Vec::new(),
        };
        dropped.push((column, bytes, value));
    }
    cells.clear();
    *scratch = cells.into_iter().map_while(|_| None).collect();
    dropped
}
