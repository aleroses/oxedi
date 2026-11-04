//! The scan of `read_835`: one file at a time is read, parsed and kept
//! until all its rows of the bound table have been emitted, in chunks of at
//! most DuckDB's vector size.
//!
//! - `mod.rs`: what the bind produced ([`Bound`]) and the scan state
//!   ([`Scan`]).
//! - `write.rs`: copying a core column into a DuckDB vector.

mod write;

use std::sync::Arc;

use libduckdb_sys as ffi;
use oxedi_core::{ColumnType, Document, DocumentError, Mask, Processor, Table};

use crate::builtins::{BuiltinSchema, Builtins};
use crate::diagnostics;
use crate::error::ReadError;
use crate::files::FileSystem;
use crate::types::SqlType;
use write::Place;
pub(crate) use write::Vector;

/// Everything the bind settled, read by the scan.
#[derive(Debug)]
pub struct Bound {
    /// The files to read, in order.
    pub files: Vec<String>,
    /// The client context's file system.
    pub file_system: FileSystem,
    /// The built-in specs.
    pub builtins: Arc<Builtins>,
    /// The table to emit.
    pub table: String,
    /// The table's columns as bound: name and core type.
    pub columns: Vec<(String, ColumnType)>,
    /// The DuckDB type of each column.
    pub types: Vec<SqlType>,
    /// The version whose spec the columns were bound with.
    pub bound_version: &'static str,
    /// Whether every file uses the bound version instead of the one it
    /// declares.
    pub forced: bool,
    /// Whether a `filename` column follows the table's columns.
    pub filename: bool,
    /// Whether the table is `diagnostics`, which every spec shares.
    pub diagnostics: bool,
    /// Whether a file that cannot be parsed is reported instead of failing:
    /// as one row of `diagnostics`, and as no rows of any other table.
    pub ignore_errors: bool,
    /// Whether text columns are BLOB instead of VARCHAR.
    pub binary: bool,
    /// The mask that hides patient data, when one was asked for.
    pub mask: Option<Mask>,
}

/// The bound table of the file being emitted.
#[derive(Debug)]
struct Loaded {
    /// Index of the file in [`Bound::files`].
    file: usize,
    table: Table,
    /// Rows already emitted.
    emitted: usize,
}

/// Where the scan is.
#[derive(Debug, Default)]
pub struct Scan {
    /// Index of the next file to load.
    next_file: usize,
    current: Option<Loaded>,
}

impl Scan {
    /// Fills `output` with the next rows, or leaves it empty when every file
    /// has been emitted.
    ///
    /// # Safety
    ///
    /// `output` must be the chunk DuckDB passed to the running scan callback
    /// of the bind that produced `bound`.
    pub unsafe fn next_chunk(
        &mut self,
        bound: &Bound,
        output: ffi::duckdb_data_chunk,
    ) -> Result<usize, ReadError> {
        // SAFETY: reading the vector size has no precondition.
        let capacity = usize::try_from(unsafe { ffi::duckdb_vector_size() }).unwrap_or(0);
        loop {
            let pending = match &self.current {
                Some(loaded) => loaded.table.len().saturating_sub(loaded.emitted),
                None => 0,
            };
            if pending > 0 {
                break;
            }
            let Some(file) = bound.files.get(self.next_file) else {
                self.current = None;
                return Ok(0);
            };
            let table = load(bound, file)?;
            self.current = Some(Loaded {
                file: self.next_file,
                table,
                emitted: 0,
            });
            self.next_file += 1;
        }
        let Some(loaded) = self.current.as_mut() else {
            return Ok(0);
        };
        let table = &loaded.table;
        let start = loaded.emitted;
        let len = table.len().saturating_sub(start).min(capacity);
        let file = bound.files.get(loaded.file).map_or("", String::as_str);
        for (index, ((name, data), sql)) in table.columns().iter().zip(&bound.types).enumerate() {
            // SAFETY: `output` is the scan's chunk (caller contract), whose
            // columns are the bound ones: table columns first.
            let mut vector = unsafe { Vector::of(output, index, capacity) };
            let place = Place {
                file,
                table: &bound.table,
                column: name,
                rows: table.column(oxedi_core::spec::ROW_COLUMN),
            };
            vector.fill(data, *sql, start, len, &place)?;
        }
        if bound.filename {
            // SAFETY: as above; the `filename` column follows the table's.
            let mut vector = unsafe { Vector::of(output, bound.columns.len(), capacity) };
            vector.fill_text(len, file);
        }
        loaded.emitted = start + len;
        Ok(len)
    }
}

/// Reads and parses one file, checks that its spec projects the bound table
/// with the bound columns, and fills that table only (or collects its
/// findings, for `diagnostics`).
fn load(bound: &Bound, file: &str) -> Result<Table, ReadError> {
    let bytes = bound.file_system.read_all(file)?;
    let document = match Document::parse(bytes.as_slice()) {
        Ok(document) => document,
        Err(source) => return unparsable(bound, file, &bytes, source),
    };
    let builtin = builtin_of(bound, file, &document)?;
    if bound.diagnostics {
        // Every table is filled: a text value too large to store is a
        // finding of the table that would hold it.
        let (_tables, findings) = Processor::run(builtin.spec(), &document, bound.mask.as_ref());
        return diagnostics::of_findings(&findings);
    }
    check_schema(bound, file, builtin)?;
    project(builtin, &document, &bound.table, file, bound.mask.as_ref())
}

/// What a file that cannot be parsed gives: its error, or with
/// `ignore_errors` an empty table, or for `diagnostics` the one row that
/// reports it.
fn unparsable(
    bound: &Bound,
    file: &str,
    bytes: &[u8],
    source: DocumentError,
) -> Result<Table, ReadError> {
    if bound.ignore_errors && !bound.diagnostics {
        return Ok(Table::new(bound.table.clone(), bound.columns.clone()));
    }
    let datum = diagnostics::datum(&source, bytes).to_vec();
    let error = ReadError::Parse {
        file: file.to_owned(),
        source,
    };
    if bound.ignore_errors {
        return diagnostics::of_unparsable(&error, &datum, bound.binary);
    }
    Err(error)
}

/// The built-in that reads `document`: the bound version when it was named,
/// else the one the file declares.
fn builtin_of<'b>(
    bound: &'b Bound,
    file: &str,
    document: &Document<'_>,
) -> Result<&'b BuiltinSchema, ReadError> {
    let picked = if bound.forced {
        bound.builtins.by_version(bound.bound_version).ok()
    } else {
        bound.builtins.select(document.segments())
    };
    picked.ok_or_else(|| ReadError::Internal {
        message: format!(
            "{file:?}: no built-in spec for version {}",
            bound.bound_version
        ),
    })
}

/// Refuses a built-in that does not project the bound table with the bound
/// columns.
fn check_schema(bound: &Bound, file: &str, builtin: &BuiltinSchema) -> Result<(), ReadError> {
    let same = builtin
        .table(&bound.table)
        .is_some_and(|table| table.columns == bound.columns);
    if same {
        return Ok(());
    }
    Err(ReadError::SchemaMismatch {
        file: file.to_owned(),
        table: bound.table.clone(),
        version: builtin.version(),
        bound: bound.bound_version,
    })
}

/// Fills the table `name` alone from the document, with the rows a run that
/// fills every table gives it.
fn project(
    builtin: &BuiltinSchema,
    document: &Document<'_>,
    name: &str,
    file: &str,
    mask: Option<&Mask>,
) -> Result<Table, ReadError> {
    Processor::run_tables(builtin.spec(), document, &[name], mask)
        .map_err(|error| ReadError::Internal {
            message: format!("{file:?}: {error}"),
        })?
        .remove(name)
        .ok_or_else(|| ReadError::Internal {
            message: format!("{file:?}: the parsed file has no table {name:?}"),
        })
}

#[cfg(test)]
mod tests;
