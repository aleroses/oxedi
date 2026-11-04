//! Computing the summaries a spec declares over the tables of a parse.
//!
//! [`summarize`] reads one summary by name: a row count, a count of distinct
//! values, a sum of a decimal column, or the fields of a table's one row.
//! Each table and column it reads is looked up by the name the spec gives,
//! so tables that do not hold one are an error, never a panic. A count or a
//! sum walks its column once.
//!
//! `error` holds why a summary cannot be computed.

mod error;
#[cfg(test)]
mod tests;

pub use error::SummaryError;

use std::borrow::Cow;
use std::collections::HashSet;

use crate::column::{Cell, ColumnData, Table, Tables};
use crate::spec::{Spec, Summary};
use crate::value::{ColumnType, decimal_display};

/// The value of a summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummaryValue {
    /// What a `count` or a `distinct` summary counts.
    Count(usize),
    /// What a `sum` summary adds up: `value` scaled by `10^scale`, the
    /// column's scale; [`decimal_display`] writes it as decimal text.
    Decimal {
        /// The sum, as an integer of the column's scale.
        value: i128,
        /// Decimal places of the column.
        scale: u8,
    },
    /// The fields of a `row` summary, by name in the order the spec writes
    /// them: each the cell's text, or `None` when the cell is null. The
    /// whole row is `None` when every field is null (the file holds none of
    /// what the row reads).
    Row(Option<Vec<(String, Option<String>)>>),
}

/// Computes the summary `name` of `spec` over `tables`.
pub fn summarize(spec: &Spec, name: &str, tables: &Tables) -> Result<SummaryValue, SummaryError> {
    let read = Reader {
        summary: name,
        tables,
    };
    match spec.summary(name) {
        None => Err(SummaryError::Undeclared {
            summary: name.to_string(),
            declared: spec
                .summaries()
                .iter()
                .map(|(declared, _)| declared.clone())
                .collect(),
        }),
        Some(Summary::Count { table }) => Ok(SummaryValue::Count(read.table(table)?.len())),
        Some(Summary::Distinct { table, column }) => {
            Ok(SummaryValue::Count(distinct(read.column(table, column)?)))
        }
        Some(Summary::Sum { table, column }) => read.sum(table, column),
        Some(Summary::Row { table, fields }) => read.row(table, fields),
    }
}

/// The tables a summary reads, with the summary's name for the errors.
struct Reader<'a> {
    summary: &'a str,
    tables: &'a Tables,
}

impl<'a> Reader<'a> {
    fn table(&self, table: &str) -> Result<&'a Table, SummaryError> {
        self.tables
            .get(table)
            .ok_or_else(|| SummaryError::MissingTable {
                summary: self.summary.to_string(),
                table: table.to_string(),
                present: self
                    .tables
                    .iter()
                    .map(|table| table.name().to_owned())
                    .collect(),
            })
    }

    fn column(&self, table: &str, column: &str) -> Result<&'a ColumnData, SummaryError> {
        self.table(table)?
            .column(column)
            .ok_or_else(|| SummaryError::MissingColumn {
                summary: self.summary.to_string(),
                table: table.to_string(),
                column: column.to_string(),
            })
    }

    /// The sum of the non-null cells of a decimal column, checked at every row.
    fn sum(&self, table: &str, column: &str) -> Result<SummaryValue, SummaryError> {
        let data = self.column(table, column)?;
        let ColumnType::Decimal128 { scale, .. } = data.kind() else {
            return Err(SummaryError::NotDecimal {
                summary: self.summary.to_string(),
                table: table.to_string(),
                column: column.to_string(),
                found: data.kind(),
            });
        };
        let overflow = |row: usize, amount: i128| SummaryError::Overflow {
            summary: self.summary.to_string(),
            table: table.to_string(),
            column: column.to_string(),
            row,
            amount: decimal_display(amount, scale),
        };
        let value = (0..data.len()).try_fold(0i128, |sum, row| match data.get(row) {
            Some(Cell::Decimal128(amount)) => {
                sum.checked_add(amount).ok_or_else(|| overflow(row, amount))
            }
            _ => Ok(sum),
        })?;
        Ok(SummaryValue::Decimal { value, scale })
    }

    /// The named fields of the table's one row.
    fn row(&self, table: &str, fields: &[(String, String)]) -> Result<SummaryValue, SummaryError> {
        let data = self.table(table)?;
        if data.len() != 1 {
            return Err(SummaryError::NotOneRow {
                summary: self.summary.to_string(),
                table: table.to_string(),
                rows: data.len(),
            });
        }
        let values = fields
            .iter()
            .map(|(name, column)| Ok((name.clone(), first_text(self.column(table, column)?))))
            .collect::<Result<Vec<_>, SummaryError>>()?;
        let any = values.iter().any(|(_, value)| value.is_some());
        Ok(SummaryValue::Row(any.then_some(values)))
    }
}

/// The text of the column's first cell; `None` when it is null or absent.
fn first_text(data: &ColumnData) -> Option<String> {
    match data.get(0)? {
        Cell::Null => None,
        _ => data.render(0),
    }
}

/// The number of distinct non-null cells, in one pass: bytes compare as they
/// are, other cells by their text, so `0123` and `123` are two values.
fn distinct(data: &ColumnData) -> usize {
    (0..data.len())
        .filter_map(|row| distinct_key(data, row))
        .collect::<HashSet<_>>()
        .len()
}

fn distinct_key(data: &ColumnData, row: usize) -> Option<Cow<'_, [u8]>> {
    match data.get(row)? {
        Cell::Null => None,
        Cell::Binary(bytes) => Some(Cow::Borrowed(bytes)),
        _ => data.render(row).map(|text| Cow::Owned(text.into_bytes())),
    }
}
