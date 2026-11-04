//! Values as element text, and segments as bytes.
//!
//! A segment is built from its parts, one per element or component, each
//! holding a value or nothing. A part with nothing and a part with empty text
//! both write an empty element; trailing empty elements, and trailing empty
//! components of a composite, are left out.

use std::io::Write as _;
use std::ops::Range;

use crate::column::{Cell, ColumnData};
use crate::value::{
    ColumnType, ElementTextError, date_element, decimal_element, time_element, time_of_day,
};

/// An envelope date as `CCYYMMDD`, or as `YYMMDD` (the century dropped)
/// when the element holds six bytes at most; the reason when the date has
/// no four-digit year.
pub(super) fn envelope_date(days: i32, max: Option<usize>) -> Result<Vec<u8>, ElementTextError> {
    let full = date_element(days, None)?;
    match max {
        Some(max) if max < 8 => Ok(full.get(2..).unwrap_or_default().to_vec()),
        _ => Ok(full),
    }
}

/// An envelope time as `HHMM`, or `HHMMSS` when it has seconds and the
/// element holds them (an element of four bytes drops them); the reason
/// when the time falls outside one day.
pub(super) fn envelope_time(seconds: i32, max: Option<usize>) -> Result<Vec<u8>, ElementTextError> {
    if time_of_day(i64::from(seconds)).is_none() {
        return time_element(seconds, max);
    }
    let held = match max {
        Some(max) if max < 6 => seconds - seconds.rem_euclid(60),
        _ => seconds,
    };
    time_element(held, max)
}

/// Appends a cell's text for an element; `Ok(false)` for a null cell, the
/// reason when the value has no text the element can hold. `max` gives the
/// element's maximum length, which only dates and times read; `kind` gives
/// a decimal its scale.
pub(super) fn cell_text(
    cell: Cell<'_>,
    kind: ColumnType,
    max: impl FnOnce() -> Option<usize>,
    out: &mut Vec<u8>,
) -> Result<bool, ElementTextError> {
    let written = match cell {
        Cell::Null => return Ok(false),
        Cell::Binary(bytes) => {
            out.extend_from_slice(bytes);
            Ok(())
        }
        Cell::Int64(value) => {
            let _ = write!(out, "{value}");
            Ok(())
        }
        Cell::Decimal128(value) => kind
            .decimal_scale()
            .ok_or(ElementTextError::NoDecimalScale { kind })
            .map(|scale| decimal_element(value, scale, out)),
        Cell::Date32(days) => date_element(days, max()).map(|text| out.extend_from_slice(&text)),
        Cell::Time32(seconds) => {
            time_element(seconds, max()).map(|text| out.extend_from_slice(&text))
        }
    };
    written.map(|()| true)
}

/// A cell's value as a message shows it.
pub(super) fn cell_display(data: &ColumnData, row: usize) -> String {
    match data.get(row) {
        Some(Cell::Binary(bytes)) => format!("{:?}", String::from_utf8_lossy(bytes)),
        _ => data.render(row).unwrap_or_default(),
    }
}

/// One element or component of a segment being built: its place and the
/// range of its text in the builder's buffer, or nothing.
#[derive(Debug, Clone)]
pub(super) struct Part {
    pub(super) element: usize,
    pub(super) component: Option<usize>,
    pub(super) text: Option<Range<usize>>,
}

/// The bytes a segment is written with.
#[derive(Debug, Clone, Copy)]
pub(super) struct Separators {
    pub(super) element: u8,
    pub(super) component: u8,
    pub(super) segment: u8,
    pub(super) line_break: bool,
}

/// Writes segment `id` from `parts` (sorted by element, then component),
/// whose texts live in `texts`.
pub(super) fn write_segment(
    id: &[u8],
    parts: &[Part],
    texts: &[u8],
    separators: Separators,
    out: &mut Vec<u8>,
) {
    let text = |part: &Part| {
        part.text
            .as_ref()
            .and_then(|range| texts.get(range.clone()))
            .unwrap_or_default()
    };
    let last = parts
        .iter()
        .filter(|part| !text(part).is_empty())
        .map(|part| part.element)
        .max()
        .unwrap_or(0);
    out.extend_from_slice(id);
    let mut rest = parts;
    for position in 1..=last {
        out.push(separators.element);
        let count = rest
            .iter()
            .take_while(|part| part.element == position)
            .count();
        let (here, after) = rest.split_at(count);
        rest = after;
        if here.iter().all(|part| part.component.is_none()) {
            if let Some(part) = here.first() {
                out.extend_from_slice(text(part));
            }
            continue;
        }
        let last_component = here
            .iter()
            .filter(|part| !text(part).is_empty())
            .map(|part| part.component.unwrap_or(1))
            .max()
            .unwrap_or(0);
        for component in 1..=last_component {
            if component > 1 {
                out.push(separators.component);
            }
            if let Some(part) = here
                .iter()
                .find(|part| part.component.unwrap_or(1) == component)
            {
                out.extend_from_slice(text(part));
            }
        }
    }
    out.push(separators.segment);
    if separators.line_break {
        out.push(b'\n');
    }
}
