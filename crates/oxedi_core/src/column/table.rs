//! Tables: named columns of equal length, appended a row at a time.

use std::fmt;

use super::{Cell, CellError, ColumnData};
use crate::value::ColumnType;

/// Why a row could not be appended to a table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowError {
    /// The row has a different number of cells than the table has columns.
    Arity {
        /// The table.
        table: String,
        /// Columns in the table.
        expected: usize,
        /// Cells in the row.
        found: usize,
    },
    /// One cell does not fit its column.
    Cell {
        /// The table.
        table: String,
        /// The column.
        column: String,
        /// Why the cell does not fit.
        source: CellError,
    },
}

impl fmt::Display for RowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RowError::Arity {
                table,
                expected,
                found,
            } => write!(
                f,
                "table {table:?} has {expected} columns; the row has {found} cells"
            ),
            RowError::Cell {
                table,
                column,
                source,
            } => write!(f, "table {table:?} column {column:?}: {source}"),
        }
    }
}

impl std::error::Error for RowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            // `Cell` prints the cell error in full, so the chain stops here
            // instead of printing it a second time.
            RowError::Arity { .. } | RowError::Cell { .. } => None,
        }
    }
}

/// Named columns of equal length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    name: String,
    columns: Vec<(String, ColumnData)>,
    rows: usize,
}

impl Table {
    /// An empty table with the given columns, in order.
    pub fn new(
        name: impl Into<String>,
        columns: impl IntoIterator<Item = (String, ColumnType)>,
    ) -> Table {
        Table {
            name: name.into(),
            columns: columns
                .into_iter()
                .map(|(name, kind)| (name, ColumnData::new(kind)))
                .collect(),
            rows: 0,
        }
    }

    /// The table's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Every column with its name, in order.
    pub fn columns(&self) -> &[(String, ColumnData)] {
        &self.columns
    }

    /// A column by name.
    pub fn column(&self, name: &str) -> Option<&ColumnData> {
        self.columns
            .iter()
            .find(|(column, _)| column == name)
            .map(|(_, data)| data)
    }

    /// Number of rows; every column has this length.
    pub fn len(&self) -> usize {
        self.rows
    }

    /// `true` when the table has no rows.
    pub fn is_empty(&self) -> bool {
        self.rows == 0
    }

    /// Appends one row, one cell per column in column order. Either every
    /// cell is appended or, on error, none is.
    pub fn push_row(&mut self, cells: &[Cell<'_>]) -> Result<(), RowError> {
        if cells.len() != self.columns.len() {
            return Err(RowError::Arity {
                table: self.name.clone(),
                expected: self.columns.len(),
                found: cells.len(),
            });
        }
        for ((column, data), &cell) in self.columns.iter().zip(cells) {
            data.check(cell).map_err(|source| RowError::Cell {
                table: self.name.clone(),
                column: column.clone(),
                source,
            })?;
        }
        for ((_, data), &cell) in self.columns.iter_mut().zip(cells) {
            data.push_checked(cell);
        }
        self.rows += 1;
        Ok(())
    }

    /// The header line: every column as `name: type`, separated by ` | `.
    pub fn header(&self) -> String {
        self.columns
            .iter()
            .map(|(name, column)| format!("{name}: {}", column.kind()))
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// Row `row` as one line, each cell rendered by [`ColumnData::render`]
    /// and separated by ` | `; `None` past the end.
    pub fn render_row(&self, row: usize) -> Option<String> {
        if row >= self.rows {
            return None;
        }
        Some(
            self.columns
                .iter()
                .map(|(_, column)| column.render(row).unwrap_or_default())
                .collect::<Vec<_>>()
                .join(" | "),
        )
    }

    /// Moves the rows out into a new table and leaves this one empty, with
    /// the same columns and types.
    pub fn take_rows(&mut self) -> Table {
        let columns = self
            .columns
            .iter_mut()
            .map(|(name, data)| {
                let empty = ColumnData::new(data.kind());
                (name.clone(), std::mem::replace(data, empty))
            })
            .collect();
        Table {
            name: self.name.clone(),
            columns,
            rows: std::mem::take(&mut self.rows),
        }
    }
}

/// The table as text: `## <name> (rows: <n>)`, the [`Table::header`], one
/// [`Table::render_row`] line per row, then an empty line.
impl fmt::Display for Table {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "## {} (rows: {})", self.name, self.rows)?;
        writeln!(f, "{}", self.header())?;
        for row in 0..self.rows {
            writeln!(f, "{}", self.render_row(row).unwrap_or_default())?;
        }
        writeln!(f)
    }
}

/// Tables ordered by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tables {
    tables: Vec<Table>,
}

impl Tables {
    /// Collects tables, ordering them by name.
    pub fn new(mut tables: Vec<Table>) -> Tables {
        tables.sort_by(|a, b| a.name.cmp(&b.name));
        Tables { tables }
    }

    /// A table by name.
    pub fn get(&self, name: &str) -> Option<&Table> {
        self.tables.iter().find(|table| table.name == name)
    }

    /// Moves a table out by name, leaving the others in order.
    pub fn remove(&mut self, name: &str) -> Option<Table> {
        let at = self.tables.iter().position(|table| table.name == name)?;
        Some(self.tables.remove(at))
    }

    /// Every table, ordered by name.
    pub fn iter(&self) -> std::slice::Iter<'_, Table> {
        self.tables.iter()
    }

    /// Number of tables.
    pub fn len(&self) -> usize {
        self.tables.len()
    }

    /// `true` when there are no tables.
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }
}

/// Every table as [`Table`] displays it, in order.
impl fmt::Display for Tables {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.tables
            .iter()
            .try_for_each(|table| write!(f, "{table}"))
    }
}

impl<'t> IntoIterator for &'t Tables {
    type Item = &'t Table;
    type IntoIter = std::slice::Iter<'t, Table>;

    fn into_iter(self) -> Self::IntoIter {
        self.tables.iter()
    }
}
