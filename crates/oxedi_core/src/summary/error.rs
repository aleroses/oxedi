//! Why a summary cannot be computed over a set of tables.

use std::fmt;

use crate::value::ColumnType;

/// Why [`summarize`](super::summarize) cannot compute a summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummaryError {
    /// The spec declares no summary of that name.
    Undeclared {
        /// The name asked for.
        summary: String,
        /// The summaries the spec declares, in name order.
        declared: Vec<String>,
    },
    /// The tables do not hold a table the summary reads.
    MissingTable {
        /// The summary.
        summary: String,
        /// The table.
        table: String,
        /// The names of the tables that are present, in their order.
        present: Vec<String>,
    },
    /// The table does not hold a column the summary reads.
    MissingColumn {
        /// The summary.
        summary: String,
        /// The table.
        table: String,
        /// The column.
        column: String,
    },
    /// The column a `sum` reads holds something other than decimals.
    NotDecimal {
        /// The summary.
        summary: String,
        /// The table.
        table: String,
        /// The column.
        column: String,
        /// What the column holds.
        found: ColumnType,
    },
    /// The table a `row` summary reads has other than one row.
    NotOneRow {
        /// The summary.
        summary: String,
        /// The table.
        table: String,
        /// How many rows it has.
        rows: usize,
    },
    /// Adding a row's amount to the sum overflows a 128-bit decimal.
    Overflow {
        /// The summary.
        summary: String,
        /// The table.
        table: String,
        /// The column.
        column: String,
        /// 0-based row whose amount overflows the sum.
        row: usize,
        /// That amount, as decimal text at the column's scale.
        amount: String,
    },
}

impl fmt::Display for SummaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SummaryError::Undeclared { summary, declared } => write!(
                f,
                "the spec declares no summary {summary:?}; it declares{}",
                Names(declared)
            ),
            SummaryError::MissingTable {
                summary,
                table,
                present,
            } => write!(
                f,
                "summary {summary:?} reads the table {table:?}, which the tables do not hold; \
                 they hold{}",
                Names(present)
            ),
            SummaryError::MissingColumn {
                summary,
                table,
                column,
            } => write!(
                f,
                "summary {summary:?} reads the column {column:?} of the table {table:?}, which \
                 the table does not hold"
            ),
            SummaryError::NotDecimal {
                summary,
                table,
                column,
                found,
            } => write!(
                f,
                "summary {summary:?} sums the column {column:?} of the table {table:?}, which \
                 holds {found}, not a decimal"
            ),
            SummaryError::NotOneRow {
                summary,
                table,
                rows,
            } => write!(
                f,
                "summary {summary:?} reads the one row of the table {table:?}, which has {rows} \
                 rows"
            ),
            SummaryError::Overflow {
                summary,
                table,
                column,
                row,
                amount,
            } => write!(
                f,
                "summary {summary:?}: the sum of {column:?} in the table {table:?} overflows a \
                 128-bit decimal at row {row} (amount {amount})"
            ),
        }
    }
}

impl std::error::Error for SummaryError {}

/// Names as a message lists them after a verb: `: a, b`, or ` none`.
struct Names<'a>(&'a [String]);

impl fmt::Display for Names<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            [] => write!(f, " none"),
            names => write!(f, ": {}", names.join(", ")),
        }
    }
}
