//! What keeps tables from becoming a valid file: the findings about their
//! data, where each value came from, and the errors that stop a write.

use std::fmt;

use crate::diagnostic::{Diagnostic, Quoted};
use crate::document::DocumentError;
use crate::value::{ColumnType, ElementTextError};

use super::refusal::PlanError;

/// Where a written value comes from. Rows are 0-based positions in the
/// table as given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueOrigin {
    /// One cell of a table.
    Cell {
        /// The table.
        table: String,
        /// The row's position.
        row: usize,
        /// The column.
        column: String,
    },
    /// A row of a table, when no single column is at fault.
    Row {
        /// The table.
        table: String,
        /// The row's position.
        row: usize,
    },
    /// A field of the envelope.
    Envelope {
        /// The field, e.g. `sender_id`.
        field: String,
    },
}

impl fmt::Display for ValueOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueOrigin::Cell { table, row, column } => {
                write!(f, "table {table:?} row {row} column {column:?}")
            }
            ValueOrigin::Row { table, row } => write!(f, "table {table:?} row {row}"),
            ValueOrigin::Envelope { field } => write!(f, "envelope field {field:?}"),
        }
    }
}

/// One reason the tables do not make a valid file. Rows are 0-based
/// positions in the table as given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// A value holds one of the interchange's delimiters, so written as it
    /// is it would split its element, segment or repeat.
    DelimiterInValue {
        /// Where the value comes from.
        origin: ValueOrigin,
        /// The element or component it fills, e.g. `NM103` or `SVC01-2`.
        place: String,
        /// The value.
        value: Vec<u8>,
        /// The delimiter byte it holds.
        delimiter: u8,
        /// What the delimiter separates, e.g. `element separator`.
        role: &'static str,
    },
    /// A column that no valid file of the spec fills holds a value.
    UnwrittenValue {
        /// Where the value comes from.
        origin: ValueOrigin,
        /// The value as text.
        value: String,
        /// The element whose code list excludes the column's `where` code.
        place: String,
        /// The excluded code.
        code: String,
        /// The element's code list.
        codes: Vec<String>,
    },
    /// A row's reference to the table above it is null or names no row of
    /// that table, so the row has no place in the file.
    MissingParent {
        /// The table.
        table: String,
        /// The row.
        row: usize,
        /// The reference column.
        column: String,
        /// The reference; `None` when null.
        value: Option<i64>,
        /// The table the reference points into.
        parent: String,
    },
    /// A row's reference to a table further up contradicts the row it
    /// belongs to: that row refers to another one.
    MismatchedReference {
        /// The table.
        table: String,
        /// The row.
        row: usize,
        /// The contradicting reference column.
        column: String,
        /// Its value.
        value: i64,
        /// The column that places the row.
        through: String,
        /// The table the row belongs to a row of.
        parent: String,
        /// That row.
        parent_row: usize,
        /// That row's reference in `column`.
        expected: i64,
    },
    /// Two rows of a table carry the same row number, so a reference to it
    /// cannot tell them apart.
    DuplicateRowNumber {
        /// The table.
        table: String,
        /// The second row.
        row: usize,
        /// The row number both carry.
        value: i64,
        /// The first row that carries it.
        first: usize,
    },
    /// A row's place in the file comes before a row its table lists ahead
    /// of it: the rows of one parent are not together, or not in their
    /// parents' order.
    OutOfOrder {
        /// The table.
        table: String,
        /// The row written late.
        row: usize,
        /// The reference column that places it.
        column: String,
        /// Its reference.
        value: i64,
        /// The row of the same table already written.
        previous: usize,
    },
    /// A value has no text its element can hold.
    NotWritable {
        /// Where the value comes from.
        origin: ValueOrigin,
        /// The element or component it fills.
        place: String,
        /// The value as text.
        value: String,
        /// Why, e.g. `a date needs a year from 1 to 9999`.
        reason: ElementTextError,
    },
    /// A diagnostic from reading the written file back, with where the
    /// value at fault came from when the writer knows it.
    ReadBack {
        /// Where the value or row at fault comes from.
        origin: Option<ValueOrigin>,
        /// The diagnostic about the written file.
        diagnostic: Diagnostic,
    },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Finding::DelimiterInValue {
                origin,
                place,
                value,
                delimiter,
                role,
            } => write!(
                f,
                "{origin} writes {place} with {}, which holds the {role} {}; a value cannot \
                 hold the interchange's delimiters",
                Quoted(value),
                Quoted(&[*delimiter])
            ),
            Finding::UnwrittenValue {
                origin,
                value,
                place,
                code,
                codes,
            } => {
                let quoted: Vec<String> = codes.iter().map(|code| format!("{code:?}")).collect();
                write!(
                    f,
                    "{origin} holds {value}, but the column reads {place} {code:?}, which the \
                     element's code list ({}) excludes, so no valid file holds it",
                    quoted.join(", ")
                )
            }
            Finding::MissingParent {
                table,
                row,
                column,
                value,
                parent,
            } => {
                write!(f, "table {table:?} row {row} column {column:?} ")?;
                match value {
                    Some(value) => {
                        write!(f, "refers to row {value}, which table {parent:?} lacks")?
                    }
                    None => write!(f, "is null")?,
                }
                write!(f, ", so the row has no place in the file")
            }
            Finding::MismatchedReference {
                table,
                row,
                column,
                value,
                through,
                parent,
                parent_row,
                expected,
            } => write!(
                f,
                "table {table:?} row {row} column {column:?} is {value}, but the row it belongs \
                 to by column {through:?}, row {parent_row} of table {parent:?}, has {column:?} \
                 {expected}"
            ),
            Finding::DuplicateRowNumber {
                table,
                row,
                value,
                first,
            } => write!(
                f,
                "table {table:?} row {row} carries row number {value}, which row {first} \
                 already carries; references to it are ambiguous"
            ),
            Finding::OutOfOrder {
                table,
                row,
                column,
                value,
                previous,
            } => write!(
                f,
                "table {table:?} row {row} (column {column:?} = {value}) is written after row \
                 {previous}; the rows of one parent must be together and in their parents' order"
            ),
            Finding::NotWritable {
                origin,
                place,
                value,
                reason,
            } => write!(
                f,
                "{origin} holds {value}, which {place} cannot hold: {reason}"
            ),
            Finding::ReadBack { origin, diagnostic } => {
                match origin {
                    Some(origin) => write!(f, "{origin}: ")?,
                    None => write!(f, "the written file: ")?,
                }
                write!(f, "{diagnostic}")
            }
        }
    }
}

/// Why tables could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// The spec's tables cannot make a valid file.
    Plan(PlanError),
    /// A table the spec does not define.
    UnknownTable {
        /// The table.
        table: String,
        /// The spec's tables, in name order.
        tables: Vec<String>,
    },
    /// A column the spec's table does not have.
    UnknownColumn {
        /// The table.
        table: String,
        /// The column.
        column: String,
        /// The table's columns, in schema order.
        columns: Vec<String>,
    },
    /// A column whose type is not the one the spec gives it.
    ColumnType {
        /// The table.
        table: String,
        /// The column.
        column: String,
        /// The spec's type.
        expected: ColumnType,
        /// The column's type.
        found: ColumnType,
    },
    /// Two delimiters are the same byte.
    SameDelimiter {
        /// The first delimiter's role, e.g. `element separator`.
        first: &'static str,
        /// The second delimiter's role.
        second: &'static str,
        /// The byte both use.
        byte: u8,
    },
    /// A delimiter is a letter, a digit or white space, which values hold.
    DelimiterNotAllowed {
        /// The delimiter's role.
        role: &'static str,
        /// The byte.
        byte: u8,
    },
    /// The interchange header carries a repetition separator, and the
    /// envelope's delimiters have none.
    NoRepetition,
    /// The written file could not be read back.
    Unreadable(DocumentError),
    /// The tables do not make a valid file; nothing was written.
    Findings(Vec<Finding>),
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::Plan(error) => write!(f, "{error}"),
            WriteError::UnknownTable { table, tables } => {
                let quoted: Vec<String> = tables.iter().map(|name| format!("{name:?}")).collect();
                write!(
                    f,
                    "table {table:?} is not a table of the spec, whose tables are {}",
                    quoted.join(", ")
                )
            }
            WriteError::UnknownColumn {
                table,
                column,
                columns,
            } => {
                let quoted: Vec<String> = columns.iter().map(|name| format!("{name:?}")).collect();
                write!(
                    f,
                    "table {table:?} has no column {column:?} in the spec; its columns are {}",
                    quoted.join(", ")
                )
            }
            WriteError::ColumnType {
                table,
                column,
                expected,
                found,
            } => write!(
                f,
                "table {table:?} column {column:?} is {found}; the spec makes it {expected}"
            ),
            WriteError::SameDelimiter {
                first,
                second,
                byte,
            } => write!(
                f,
                "the {first} and the {second} are both {}; each delimiter needs its own byte",
                Quoted(&[*byte])
            ),
            WriteError::DelimiterNotAllowed { role, byte } => write!(
                f,
                "the {role} {} is a letter, a digit or white space, which values hold",
                Quoted(&[*byte])
            ),
            WriteError::NoRepetition => write!(
                f,
                "the interchange header carries a repetition separator, but the envelope field \
                 \"delimiters.repetition\" is not set"
            ),
            WriteError::Unreadable(error) => {
                write!(f, "the written file could not be read back: {error}")
            }
            WriteError::Findings(findings) => {
                let count = findings.len();
                let noun = if count == 1 { "finding" } else { "findings" };
                write!(
                    f,
                    "the tables do not make a valid file ({count} {noun}); nothing was written"
                )?;
                for (i, finding) in findings.iter().enumerate() {
                    write!(f, "\n{}. {finding}", i + 1)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for WriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // `Plan` and `Unreadable` print their inner error in full, so the
        // chain stops here instead of printing it a second time.
        match self {
            WriteError::Plan(_)
            | WriteError::UnknownTable { .. }
            | WriteError::UnknownColumn { .. }
            | WriteError::ColumnType { .. }
            | WriteError::SameDelimiter { .. }
            | WriteError::DelimiterNotAllowed { .. }
            | WriteError::NoRepetition
            | WriteError::Unreadable(_)
            | WriteError::Findings(_) => None,
        }
    }
}

impl From<PlanError> for WriteError {
    fn from(error: PlanError) -> Self {
        WriteError::Plan(error)
    }
}
