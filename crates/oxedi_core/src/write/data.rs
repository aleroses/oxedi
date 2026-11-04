//! The caller's tables bound to the spec's: each spec table's columns found
//! by name and checked against the types the spec gives them.

use crate::column::{Cell, ColumnData, Tables};
use crate::project::table_columns;
use crate::spec::{ROW_COLUMN, SEGMENT_COLUMN, Spec};
use crate::value::ColumnType;

use super::finding::WriteError;

/// One spec table's data: absent tables have no rows, absent columns are
/// null.
#[derive(Debug, Clone)]
pub(super) struct Data<'t> {
    /// The table's name.
    pub(super) name: String,
    /// Its rows.
    pub(super) rows: usize,
    /// The row number column.
    pub(super) row: Option<&'t ColumnData>,
    /// The anchor segment's index column.
    pub(super) segment: Option<&'t ColumnData>,
    /// The reference to each table above, by that table's index.
    pub(super) refs: Vec<(usize, Option<&'t ColumnData>)>,
    /// The declared columns, by index into the spec's columns.
    pub(super) columns: Vec<Option<&'t ColumnData>>,
}

impl<'t> Data<'t> {
    /// The cell of a declared column.
    pub(super) fn cell(&self, column: usize, row: usize) -> Cell<'t> {
        self.columns
            .get(column)
            .copied()
            .flatten()
            .and_then(|data| data.get(row))
            .unwrap_or(Cell::Null)
    }

    /// A declared column, when the caller gave it.
    pub(super) fn column(&self, column: usize) -> Option<&'t ColumnData> {
        self.columns.get(column).copied().flatten()
    }

    /// The reference to table `above` of a row; `None` when null or absent.
    pub(super) fn reference(&self, above: usize, row: usize) -> Option<i64> {
        let data = self
            .refs
            .iter()
            .find(|(table, _)| *table == above)
            .and_then(|(_, data)| *data)?;
        match data.get(row)? {
            Cell::Int64(value) => Some(value),
            _ => None,
        }
    }

    /// The row number of a row: its row number column, or its position
    /// when that is null or absent.
    pub(super) fn key(&self, row: usize) -> i64 {
        match self.row.and_then(|data| data.get(row)) {
            Some(Cell::Int64(value)) => value,
            _ => i64::try_from(row).unwrap_or(i64::MAX),
        }
    }
}

/// Binds every spec table to the caller's table of the same name.
pub(super) fn bind<'t>(spec: &Spec, tables: &'t Tables) -> Result<Vec<Data<'t>>, WriteError> {
    for table in tables {
        if spec.table(table.name()).is_none() {
            return Err(WriteError::UnknownTable {
                table: table.name().to_string(),
                tables: spec.tables().iter().map(|def| def.name.clone()).collect(),
            });
        }
    }
    let mut bound = Vec::with_capacity(spec.tables().len());
    for def in spec.tables() {
        let schema = table_columns(spec, def);
        let given = tables.get(&def.name);
        let mut found: Vec<Option<&ColumnData>> = vec![None; schema.len()];
        if let Some(table) = given {
            for (name, data) in table.columns() {
                let Some(at) = schema.iter().position(|(column, _)| column == name) else {
                    return Err(WriteError::UnknownColumn {
                        table: def.name.clone(),
                        column: name.clone(),
                        columns: schema.iter().map(|(column, _)| column.clone()).collect(),
                    });
                };
                let expected = schema.get(at).map_or(ColumnType::Binary, |(_, kind)| *kind);
                if data.kind() != expected {
                    return Err(WriteError::ColumnType {
                        table: def.name.clone(),
                        column: name.clone(),
                        expected,
                        found: data.kind(),
                    });
                }
                if let Some(slot) = found.get_mut(at) {
                    *slot = Some(data);
                }
            }
        }
        let by_name = |name: &str| {
            schema
                .iter()
                .position(|(column, _)| column == name)
                .and_then(|at| found.get(at).copied().flatten())
        };
        bound.push(Data {
            name: def.name.clone(),
            rows: given.map_or(0, |table| table.len()),
            row: by_name(ROW_COLUMN),
            segment: by_name(SEGMENT_COLUMN),
            refs: def
                .ancestors
                .iter()
                .map(|&above| {
                    let reference = spec
                        .tables()
                        .get(above)
                        .map(|table| table.reference.as_str())
                        .unwrap_or_default();
                    (above, by_name(reference))
                })
                .collect(),
            columns: def.columns.iter().map(|(name, _)| by_name(name)).collect(),
        });
    }
    Ok(bound)
}
