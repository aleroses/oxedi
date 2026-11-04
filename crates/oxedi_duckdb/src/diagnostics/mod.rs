//! The `diagnostics` table: one row per finding of the core about a file,
//! with the attributes of the Python `Diagnostic`, and one row per file that
//! could not be parsed when `ignore_errors` is on.

use oxedi_core::{Cell, ColumnType, Diagnostic, DocumentError, FlatDiagnostic, Table};

use crate::error::ReadError;
use crate::types::SqlType;

/// The name `table_name` gives the table.
pub const TABLE: &str = "diagnostics";

/// Who reports a file that could not be parsed.
const READER_ORIGIN: &str = "read_835";

/// The kind of the row of a file that could not be parsed.
const UNPARSABLE_KIND: &str = "NotAnInterchange";

/// Each column's core type and whether its text is always valid UTF-8 (so a
/// VARCHAR even with `binary := true`), in the order of
/// `FlatDiagnostic::FIELDS`.
const COLUMNS: [(ColumnType, bool); 10] = [
    (ColumnType::Int64 { scale: 0 }, false),
    (ColumnType::Binary, true),
    (ColumnType::Binary, true),
    (ColumnType::Int64 { scale: 0 }, false),
    (ColumnType::Int64 { scale: 0 }, false),
    (ColumnType::Int64 { scale: 0 }, false),
    (ColumnType::Binary, true),
    (ColumnType::Binary, false),
    (ColumnType::Binary, true),
    (ColumnType::Binary, true),
];

/// The table's columns: name and core type.
pub fn columns() -> Vec<(String, ColumnType)> {
    FlatDiagnostic::FIELDS
        .iter()
        .zip(COLUMNS)
        .map(|(name, (kind, _))| ((*name).to_owned(), kind))
        .collect()
}

/// The DuckDB type of each column: `datum` follows `binary`, the other text
/// columns are always VARCHAR.
pub fn types(binary: bool) -> Result<Vec<SqlType>, ReadError> {
    FlatDiagnostic::FIELDS
        .iter()
        .zip(COLUMNS)
        .map(|(name, (kind, text))| {
            if text {
                Ok(SqlType::Varchar)
            } else {
                SqlType::of(TABLE, name, kind, binary)
            }
        })
        .collect()
}

/// An empty `diagnostics` table.
pub fn empty() -> Table {
    Table::new(TABLE, columns())
}

/// A position as a BIGINT cell; one past `i64` cannot come from a file the
/// core can index, and is NULL if it ever did.
fn position(value: Option<usize>) -> Cell<'static> {
    value
        .and_then(|value| i64::try_from(value).ok())
        .map_or(Cell::Null, Cell::Int64)
}

/// Appends one row; a row the table refuses is an internal error naming the
/// table.
fn push(table: &mut Table, cells: &[Cell<'_>]) -> Result<(), ReadError> {
    table.push_row(cells).map_err(|error| ReadError::Internal {
        message: format!("table {TABLE:?}: {error}"),
    })
}

/// The table of a parsed file's findings, in stream order.
pub fn of_findings(diagnostics: &[Diagnostic]) -> Result<Table, ReadError> {
    let mut table = empty();
    for diagnostic in diagnostics {
        let flat = diagnostic.flat();
        push(
            &mut table,
            &[
                Cell::Int64(i64::from(flat.level)),
                Cell::Binary(flat.kind.as_bytes()),
                Cell::Binary(flat.rule.as_bytes()),
                position(flat.segment),
                position(flat.element),
                position(flat.component),
                Cell::Binary(flat.path.as_bytes()),
                Cell::Binary(flat.datum),
                Cell::Binary(flat.origin.as_bytes()),
                flat.code
                    .map_or(Cell::Null, |code| Cell::Binary(code.as_bytes())),
            ],
        )?;
    }
    Ok(table)
}

/// The bytes a parse error is about: the bytes found instead of `ISA`, the
/// start of a truncated ISA, or nothing for a file too long to index.
pub fn datum<'a>(error: &'a DocumentError, bytes: &'a [u8]) -> &'a [u8] {
    match error {
        DocumentError::Isa(error) => error.datum(bytes),
        DocumentError::Size(_) => &[],
    }
}

/// The one-row table of a file that could not be parsed: level 1, kind
/// `NotAnInterchange`, the error's full text (file included) as the rule,
/// no position or path, the offending bytes as the datum, origin
/// `read_835` and no code. The datum is the bytes themselves with `binary`;
/// otherwise it is their `escape_ascii` text, which is always valid UTF-8
/// (the bytes of a binary file are rarely so).
pub fn of_unparsable(error: &ReadError, datum: &[u8], binary: bool) -> Result<Table, ReadError> {
    let mut table = empty();
    let rule = error.to_string();
    let escaped = datum.escape_ascii().to_string();
    let datum = if binary { datum } else { escaped.as_bytes() };
    push(
        &mut table,
        &[
            Cell::Int64(1),
            Cell::Binary(UNPARSABLE_KIND.as_bytes()),
            Cell::Binary(rule.as_bytes()),
            Cell::Null,
            Cell::Null,
            Cell::Null,
            Cell::Binary(b""),
            Cell::Binary(datum),
            Cell::Binary(READER_ORIGIN.as_bytes()),
            Cell::Null,
        ],
    )?;
    Ok(table)
}

#[cfg(test)]
mod tests;
