//! Summaries: named figures of a parse that the spec declares over its
//! tables (a row count, a count of distinct values, a sum, the fields of the
//! one row of a table), their compilation against the tables, and the
//! table a reader shows when none is named (`default_table`).

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use super::Spec;
use super::error::SpecError;
use super::phi::ColumnKind;
use super::raw::{RawSummary, RawSummaryColumn, RawSummaryRow, RawSummaryTable};
use super::summary_error::SummaryDefError;
use super::tables::{ColumnSource, TableDef};
use crate::value::ElementType;

/// What a summary computes over the tables of a parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Summary {
    /// The number of rows of the table.
    Count {
        /// The table.
        table: String,
    },
    /// The number of distinct non-null values of the column, compared as text.
    Distinct {
        /// The table.
        table: String,
        /// The column.
        column: String,
    },
    /// The sum of a decimal column; null cells are skipped.
    Sum {
        /// The table.
        table: String,
        /// The column, which reads a decimal (`R`) element.
        column: String,
    },
    /// Named fields of the one row of the table.
    Row {
        /// The table.
        table: String,
        /// `(name, column)` pairs, in the order written.
        fields: Vec<(String, String)>,
    },
}

/// Compiles the `summaries` section against the tables of `spec`, in name order.
pub(super) fn compile_summaries(
    spec: &Spec,
    raw: BTreeMap<&str, &Value>,
) -> Result<Vec<(String, Summary)>, SpecError> {
    raw.into_iter()
        .map(|(name, value)| {
            let def = RawSummary::deserialize(value).map_err(|e| SpecError::SummarySchema {
                summary: name.to_string(),
                source: e,
            })?;
            let summary =
                compile_summary(spec, name, def).map_err(|reason| SpecError::BadSummary {
                    summary: name.to_string(),
                    reason: Box::new(reason),
                })?;
            Ok((name.to_string(), summary))
        })
        .collect()
}

/// The `default_table`, which must name a table of `spec`.
pub(super) fn compile_default_table(
    spec: &Spec,
    raw: Option<String>,
) -> Result<Option<String>, SpecError> {
    match raw {
        Some(table) if spec.table(&table).is_none() => Err(SpecError::UnknownDefaultTable {
            table,
            known: table_names(spec),
        }),
        raw => Ok(raw),
    }
}

/// One operation of a summary, as written.
enum Operation {
    Count(RawSummaryTable),
    Distinct(RawSummaryColumn),
    Sum(RawSummaryColumn),
    Row(RawSummaryRow),
}

fn compile_summary(spec: &Spec, name: &str, def: RawSummary) -> Result<Summary, SummaryDefError> {
    if name.is_empty() {
        return Err(SummaryDefError::EmptyName);
    }
    let mut held: Vec<(&str, Operation)> = [
        ("count", def.count.map(Operation::Count)),
        ("distinct", def.distinct.map(Operation::Distinct)),
        ("row", def.row.map(Operation::Row)),
        ("sum", def.sum.map(Operation::Sum)),
    ]
    .into_iter()
    .filter_map(|(key, operation)| operation.map(|operation| (key, operation)))
    .collect();
    match held.pop() {
        Some((_, operation)) if held.is_empty() => operation.compile(spec),
        last => {
            held.extend(last);
            Err(SummaryDefError::Operations {
                found: held.iter().map(|(key, _)| key.to_string()).collect(),
            })
        }
    }
}

impl Operation {
    fn compile(self, spec: &Spec) -> Result<Summary, SummaryDefError> {
        match self {
            Operation::Count(raw) => {
                table_at(spec, "count.table", &raw.table)?;
                Ok(Summary::Count { table: raw.table })
            }
            Operation::Distinct(raw) => {
                column_at(spec, "distinct", &raw)?;
                Ok(Summary::Distinct {
                    table: raw.table,
                    column: raw.column,
                })
            }
            Operation::Sum(raw) => compile_sum(spec, raw),
            Operation::Row(raw) => compile_row(spec, raw),
        }
    }
}

fn compile_sum(spec: &Spec, raw: RawSummaryColumn) -> Result<Summary, SummaryDefError> {
    let (table, source) = column_at(spec, "sum", &raw)?;
    match spec.column_kind(table, source) {
        ColumnKind::Element(Some(ElementType::R { .. })) => Ok(Summary::Sum {
            table: raw.table,
            column: raw.column,
        }),
        found => Err(SummaryDefError::NotDecimal {
            key: "sum.column".into(),
            table: raw.table,
            column: raw.column,
            found: found.to_string(),
        }),
    }
}

fn compile_row(spec: &Spec, raw: RawSummaryRow) -> Result<Summary, SummaryDefError> {
    let table = table_at(spec, "row.table", &raw.table)?;
    if raw.fields.is_empty() {
        return Err(SummaryDefError::NoFields {
            key: "row.fields".into(),
        });
    }
    let mut named: BTreeMap<&str, String> = BTreeMap::new();
    for (i, field) in raw.fields.iter().enumerate() {
        let key = format!("row.fields[{i}].name");
        if let Some(first) = named.get(field.name.as_str()) {
            return Err(SummaryDefError::RepeatedField {
                key,
                name: field.name.clone(),
                first: first.clone(),
            });
        }
        column_of(table, &format!("row.fields[{i}].column"), &field.column)?;
        named.insert(&field.name, key);
    }
    Ok(Summary::Row {
        fields: raw
            .fields
            .into_iter()
            .map(|field| (field.name, field.column))
            .collect(),
        table: raw.table,
    })
}

/// The table `name`, or why `key` cannot name it.
fn table_at<'s>(spec: &'s Spec, key: &str, name: &str) -> Result<&'s TableDef, SummaryDefError> {
    spec.table(name)
        .ok_or_else(|| SummaryDefError::UnknownTable {
            key: key.to_string(),
            table: name.to_string(),
            known: table_names(spec),
        })
}

/// The table and the source of the column an operation names, at
/// `<operation>.table` and `<operation>.column`.
fn column_at<'s>(
    spec: &'s Spec,
    operation: &str,
    raw: &RawSummaryColumn,
) -> Result<(&'s TableDef, &'s ColumnSource), SummaryDefError> {
    let table = table_at(spec, &format!("{operation}.table"), &raw.table)?;
    let source = column_of(table, &format!("{operation}.column"), &raw.column)?;
    Ok((table, source))
}

/// The source of `column` among the declared columns of `table`, or why
/// `key` cannot name it.
fn column_of<'t>(
    table: &'t TableDef,
    key: &str,
    column: &str,
) -> Result<&'t ColumnSource, SummaryDefError> {
    table
        .columns
        .iter()
        .find_map(|(name, source)| (name == column).then_some(source))
        .ok_or_else(|| SummaryDefError::UnknownColumn {
            key: key.to_string(),
            table: table.name.clone(),
            column: column.to_string(),
            known: table.columns.iter().map(|(name, _)| name.clone()).collect(),
        })
}

fn table_names(spec: &Spec) -> Vec<String> {
    spec.tables()
        .iter()
        .map(|table| table.name.clone())
        .collect()
}
