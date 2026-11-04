//! Why a summary of the spec is rejected.

use std::fmt;

/// The operations a summary may hold, as the message lists them.
const OPERATIONS: &str = "count, distinct, sum, row";

/// Why a summary is rejected. `key` is where inside the summary the fault
/// is, as written, e.g. `sum.column` or `row.fields[2].name` (0-based index).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummaryDefError {
    /// The summary's name is the empty string.
    EmptyName,
    /// The summary holds no operation, or more than one.
    Operations {
        /// The operations it holds, in alphabetical order.
        found: Vec<String>,
    },
    /// The summary names a table the spec does not define.
    UnknownTable {
        /// Where the table is named.
        key: String,
        /// The table as written.
        table: String,
        /// The tables the spec defines, in name order.
        known: Vec<String>,
    },
    /// The summary names a column its table does not declare.
    UnknownColumn {
        /// Where the column is named.
        key: String,
        /// The table.
        table: String,
        /// The column as written.
        column: String,
        /// The columns the table declares, in name order.
        known: Vec<String>,
    },
    /// A `sum` names a column that does not read a decimal element.
    NotDecimal {
        /// Where the column is named.
        key: String,
        /// The table.
        table: String,
        /// The column.
        column: String,
        /// What the column reads, e.g. `AN (string)`.
        found: String,
    },
    /// A `row` lists no field.
    NoFields {
        /// Where the list is.
        key: String,
    },
    /// A `row` names one field twice.
    RepeatedField {
        /// Where the second one is named.
        key: String,
        /// The field name.
        name: String,
        /// Where the first one is named.
        first: String,
    },
}

impl fmt::Display for SummaryDefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SummaryDefError::EmptyName => write!(f, "the name is empty"),
            SummaryDefError::Operations { found } => write!(f, "{}", Held(found)),
            SummaryDefError::UnknownTable { key, table, known } => write!(
                f,
                "{key} names the table {table:?}, which the spec does not define; \
                 its tables are: {}",
                known.join(", ")
            ),
            SummaryDefError::UnknownColumn {
                key,
                table,
                column,
                known,
            } => write!(
                f,
                "{key} names the column {column:?}, which the table {table:?} does not \
                 declare; its columns are: {}",
                known.join(", ")
            ),
            SummaryDefError::NotDecimal {
                key,
                table,
                column,
                found,
            } => write!(
                f,
                "{key} names the column {column:?} of the table {table:?}, which reads \
                 {found}; sum needs a decimal (R) column"
            ),
            SummaryDefError::NoFields { key } => write!(f, "{key} lists no field"),
            SummaryDefError::RepeatedField { key, name, first } => {
                write!(f, "{key} repeats the field {name:?} first named at {first}")
            }
        }
    }
}

impl std::error::Error for SummaryDefError {}

/// The operations a summary holds, in words.
struct Held<'a>(&'a [String]);

impl fmt::Display for Held<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            [] => write!(f, "holds no operation"),
            found => write!(f, "holds {} operations ({})", found.len(), found.join(", ")),
        }?;
        write!(f, "; a summary holds exactly one of {OPERATIONS}")
    }
}
