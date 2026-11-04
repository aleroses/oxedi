//! The summaries a spec declares, read for the `Result` methods of the same
//! name: the core computes them, this module turns the value and the errors
//! into Python's.

use oxedi_core::value::decimal_display;
use oxedi_core::{SummaryError, SummaryValue, summarize};
use pyo3::exceptions::{PyKeyError, PyTypeError, PyValueError};
use pyo3::prelude::*;

use crate::tables::PyTables;

/// The fields of a row summary by name, each the cell's text or `None` for
/// a null cell; `None` as a whole when every field is null.
pub type Row = Option<Vec<(String, Option<String>)>>;

/// The summary named like `method`, computed over the tables.
fn summary(tables: &PyTables, method: &str) -> PyResult<SummaryValue> {
    summarize(tables.spec(), method, tables.tables()).map_err(|error| {
        let text = format!("Result.{method}: {error}");
        match error {
            SummaryError::Undeclared { .. }
            | SummaryError::MissingTable { .. }
            | SummaryError::MissingColumn { .. } => PyKeyError::new_err(text),
            SummaryError::NotDecimal { .. } => PyTypeError::new_err(text),
            SummaryError::NotOneRow { .. } | SummaryError::Overflow { .. } => {
                PyValueError::new_err(text)
            }
        }
    })
}

/// What a summary value is, in the words of the method that reads it.
fn kind(value: &SummaryValue) -> &'static str {
    match value {
        SummaryValue::Count(_) => "a count",
        SummaryValue::Decimal { .. } => "a decimal",
        SummaryValue::Row(_) => "a row",
    }
}

/// The `TypeError` of a method that reads a summary as `wanted` when the
/// spec declares another kind.
fn other_kind(method: &str, wanted: &str, found: &SummaryValue) -> PyErr {
    PyTypeError::new_err(format!(
        "Result.{method} reads the summary \"{method}\" as {wanted}; the spec declares {}",
        kind(found)
    ))
}

/// A `count` or `distinct` summary.
pub fn count(tables: &PyTables, method: &str) -> PyResult<usize> {
    match summary(tables, method)? {
        SummaryValue::Count(count) => Ok(count),
        other => Err(other_kind(method, "a count", &other)),
    }
}

/// A `sum` summary as fixed-point text at its column's scale.
pub fn decimal(tables: &PyTables, method: &str) -> PyResult<String> {
    match summary(tables, method)? {
        SummaryValue::Decimal { value, scale } => Ok(decimal_display(value, scale)),
        other => Err(other_kind(method, "a decimal", &other)),
    }
}

/// A `row` summary.
pub fn row(tables: &PyTables, method: &str) -> PyResult<Row> {
    match summary(tables, method)? {
        SummaryValue::Row(fields) => Ok(fields),
        other => Err(other_kind(method, "a row", &other)),
    }
}
