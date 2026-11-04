//! The schema of the tables a spec projects: each table's columns, their
//! types and the tables their reference columns point at, known from the
//! spec alone.

use super::plan::column_type;
use crate::spec::{ROW_COLUMN, SEGMENT_COLUMN, Spec, TableDef};
use crate::value::ColumnType;

/// One column of a projected table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnSchema {
    /// The column's name.
    pub name: String,
    /// The type of its values.
    pub kind: ColumnType,
    /// For a reference column, the table whose `row` it holds; `None`
    /// for every other column.
    pub parent: Option<String>,
}

/// The columns of one projected table, in the order the projector fills
/// them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSchema {
    /// The table's name.
    pub name: String,
    /// Every column, in order.
    pub columns: Vec<ColumnSchema>,
}

/// The schema of one table of `spec`: the row number, the anchor segment's
/// index, a reference per table above (outermost first, each naming that
/// table as its parent), then the declared columns in name order.
pub fn table_schema(spec: &Spec, table: &TableDef) -> TableSchema {
    let index_column = ColumnType::Int64 { scale: 0 };
    let plain = |name: &str, kind| ColumnSchema {
        name: name.to_string(),
        kind,
        parent: None,
    };
    let mut columns = vec![
        plain(ROW_COLUMN, index_column),
        plain(SEGMENT_COLUMN, index_column),
    ];
    for &above in &table.ancestors {
        if let Some(def) = spec.tables().get(above) {
            columns.push(ColumnSchema {
                name: def.reference.clone(),
                kind: index_column,
                parent: Some(def.name.clone()),
            });
        }
    }
    columns.extend(
        table
            .columns
            .iter()
            .map(|(name, source)| plain(name, column_type(spec, table, source))),
    );
    TableSchema {
        name: table.name.clone(),
        columns,
    }
}

/// The schema of every table of `spec`, in the spec's table order.
pub fn spec_schema(spec: &Spec) -> Vec<TableSchema> {
    spec.tables()
        .iter()
        .map(|table| table_schema(spec, table))
        .collect()
}

/// Every column of a table the projector fills, with its type, in order;
/// see [`table_schema`].
pub fn table_columns(spec: &Spec, table: &TableDef) -> Vec<(String, ColumnType)> {
    table_schema(spec, table)
        .columns
        .into_iter()
        .map(|column| (column.name, column.kind))
        .collect()
}
