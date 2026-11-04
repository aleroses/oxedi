//! `oxedi_schema(version := '5010', binary := false)`: one row per column of
//! every table `read_835` returns for a built-in spec, without a file.
//!
//! Each row names the table, the column, its DuckDB type (the type
//! `read_835` gives it with the same `version` and `binary`), the table a
//! reference column points at (NULL for the other columns) and the column's
//! 1-based position in its table. The spec's tables come first, in the
//! spec's order, then the `diagnostics` table. `version` follows
//! `read_835`: NULL or left out is the default built-in. Every callback
//! catches panics, so none unwinds into DuckDB.
//!
//! - `mod.rs`: the rows, the registration and the bind, init and scan
//!   callbacks.
//! - `error.rs`: the errors the function reports.

mod error;

use std::ffi::CString;
use std::sync::{Arc, Mutex};

use libduckdb_sys as ffi;

use crate::builtins::{BuiltinSchema, BuiltinTable, Builtins};
use crate::diagnostics;
use crate::function::{TableFunction, c_message, drop_box, guarded};
use crate::options::{Named, macro_parameter, named};
use crate::scan::Vector;
use crate::types::{self, LogicalType, SqlType};
pub use error::{FUNCTION, SchemaError};

/// The output columns, in order: name and whether it is an INTEGER (else
/// a VARCHAR).
const COLUMNS: [(&str, bool); 5] = [
    ("table_name", false),
    ("column_name", false),
    ("column_type", false),
    ("parent", false),
    ("position", true),
];

/// The named parameters, in registration order, with whether each is a
/// boolean (else a VARCHAR).
const NAMED: [(&str, bool); 2] = [("version", false), ("binary", true)];

/// One column of one table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The table's name.
    pub table: String,
    /// The column's name.
    pub column: String,
    /// The column's DuckDB type.
    pub sql: SqlType,
    /// The table a reference column points at.
    pub parent: Option<String>,
    /// The column's 1-based position in its table.
    pub position: i32,
}

/// A 1-based position as an INTEGER; a table never has that many columns.
fn position(index: usize) -> Result<i32, SchemaError> {
    index
        .checked_add(1)
        .and_then(|position| i32::try_from(position).ok())
        .ok_or_else(|| SchemaError::Internal {
            message: format!("column {index} is past INTEGER's range"),
        })
}

impl Row {
    /// The row of a column at 0-based `index` in its table.
    fn at(
        index: usize,
        table: &str,
        column: &str,
        sql: SqlType,
        parent: Option<&str>,
    ) -> Result<Row, SchemaError> {
        Ok(Row {
            table: table.to_owned(),
            column: column.to_owned(),
            sql,
            parent: parent.map(str::to_owned),
            position: position(index)?,
        })
    }
}

/// The rows of one spec table, in column order.
fn table_rows(
    table: &BuiltinTable,
    binary: bool,
) -> impl Iterator<Item = Result<Row, SchemaError>> + '_ {
    let parents = table.parents.iter().map(Option::as_deref);
    table
        .columns
        .iter()
        .zip(parents)
        .enumerate()
        .map(move |(index, ((column, kind), parent))| {
            let sql =
                SqlType::try_of(*kind, binary).ok_or_else(|| SchemaError::UnsupportedType {
                    table: table.name.clone(),
                    column: column.clone(),
                    kind: *kind,
                })?;
            Row::at(index, &table.name, column, sql, parent)
        })
}

/// The rows of the `diagnostics` table, in column order.
fn diagnostic_rows(binary: bool) -> Result<Vec<Row>, SchemaError> {
    let types = diagnostics::types(binary).map_err(|error| SchemaError::Internal {
        message: error.to_string(),
    })?;
    diagnostics::columns()
        .into_iter()
        .zip(types)
        .enumerate()
        .map(|(index, ((column, _), sql))| Row::at(index, diagnostics::TABLE, &column, sql, None))
        .collect()
}

/// Every column of every table `read_835` returns for `builtin`: the spec's
/// tables, then `diagnostics`.
pub fn rows(builtin: &BuiltinSchema, binary: bool) -> Result<Vec<Row>, SchemaError> {
    let mut rows = builtin
        .tables
        .iter()
        .flat_map(|table| table_rows(table, binary))
        .collect::<Result<Vec<_>, _>>()?;
    rows.extend(diagnostic_rows(binary)?);
    Ok(rows)
}

/// The built-in `version` names, the default for `None`.
pub fn builtin<'b>(
    builtins: &'b Builtins,
    version: Option<&str>,
) -> Result<&'b BuiltinSchema, SchemaError> {
    Ok(builtins.pick(version)?)
}

/// Registers `oxedi_schema` on `connection`, with `builtins` as the
/// function's extra info.
///
/// # Safety
///
/// `connection` must be a live connection; the C API must be initialized.
pub unsafe fn register(
    connection: ffi::duckdb_connection,
    builtins: Arc<Builtins>,
) -> Result<(), String> {
    let name = CString::new(FUNCTION).map_err(|err| err.to_string())?;
    let function = TableFunction::new();
    // SAFETY: `function` is the live handle created above; every logical
    // type and string passed lives across the call, and DuckDB copies them.
    unsafe {
        ffi::duckdb_table_function_set_name(function.0, name.as_ptr());
        for (parameter, boolean) in NAMED {
            let parameter = CString::new(parameter).map_err(|err| err.to_string())?;
            let kind = if boolean {
                types::boolean_type()
            } else {
                types::varchar_type()
            };
            ffi::duckdb_table_function_add_named_parameter(
                function.0,
                parameter.as_ptr(),
                kind.raw(),
            );
        }
        let extra = Box::into_raw(Box::new(builtins));
        ffi::duckdb_table_function_set_extra_info(
            function.0,
            extra.cast(),
            Some(drop_box::<Arc<Builtins>>),
        );
        ffi::duckdb_table_function_set_bind(function.0, Some(bind));
        ffi::duckdb_table_function_set_init(function.0, Some(init));
        ffi::duckdb_table_function_set_function(function.0, Some(scan));
    }
    // SAFETY: both handles are live; DuckDB copies the function into the
    // catalog, extra info included.
    let state = unsafe { ffi::duckdb_register_table_function(connection, function.0) };
    if state == ffi::duckdb_state_DuckDBSuccess {
        Ok(())
    } else {
        Err(format!("{FUNCTION} could not be registered"))
    }
}

unsafe extern "C" fn bind(info: ffi::duckdb_bind_info) {
    // SAFETY: DuckDB passes a live bind info to the bind callback.
    if let Err(error) = guarded(|| unsafe { bind_with(info) }) {
        let message = c_message(&error);
        // SAFETY: as above; DuckDB copies the message.
        unsafe { ffi::duckdb_bind_set_error(info, message.as_ptr()) };
    }
}

/// # Safety
///
/// `info` must be the live bind info of `oxedi_schema`.
unsafe fn bind_with(info: ffi::duckdb_bind_info) -> Result<(), SchemaError> {
    // SAFETY: the extra info of `oxedi_schema` is the `Arc<Builtins>` set at
    // registration, alive as long as the function.
    let builtins = unsafe {
        ffi::duckdb_bind_get_extra_info(info)
            .cast::<Arc<Builtins>>()
            .as_ref()
    }
    .ok_or_else(|| SchemaError::Internal {
        message: "the function has no extra info".to_owned(),
    })?;
    // SAFETY: `info` is live and both names are among the registered
    // named parameters.
    let (version, binary) = unsafe { (named(info, "version"), named(info, "binary")) };
    let version = match version {
        Named::Absent | Named::Null => None,
        Named::Given(value) => {
            let text = value.text();
            if let Some(parameter) = macro_parameter(&text) {
                return Err(SchemaError::MacroParameter {
                    parameter,
                    value: text,
                });
            }
            Some(text)
        }
    };
    let binary = match binary {
        Named::Absent => false,
        Named::Null => return Err(SchemaError::NullOption { name: "binary" }),
        Named::Given(value) => value.boolean(),
    };
    let rows = rows(builtin(builtins, version.as_deref())?, binary)?;
    for (column, integer) in COLUMNS {
        let name = CString::new(column).map_err(|err| SchemaError::Internal {
            message: err.to_string(),
        })?;
        let kind: LogicalType = if integer {
            types::integer_type()
        } else {
            types::varchar_type()
        };
        // SAFETY: `info` is live; DuckDB copies the name and type.
        unsafe { ffi::duckdb_bind_add_result_column(info, name.as_ptr(), kind.raw()) };
    }
    // SAFETY: `info` is live; DuckDB owns the box from here and frees it
    // with `drop_box::<Vec<Row>>`.
    unsafe {
        ffi::duckdb_bind_set_bind_data(
            info,
            Box::into_raw(Box::new(rows)).cast(),
            Some(drop_box::<Vec<Row>>),
        );
    }
    Ok(())
}

unsafe extern "C" fn init(info: ffi::duckdb_init_info) {
    let result = guarded(|| -> Result<(), SchemaError> {
        let state = Box::new(Mutex::new(0_usize));
        // SAFETY: DuckDB passes a live init info; it owns the box from here
        // and frees it with `drop_box`.
        unsafe {
            ffi::duckdb_init_set_max_threads(info, 1);
            ffi::duckdb_init_set_init_data(
                info,
                Box::into_raw(state).cast(),
                Some(drop_box::<Mutex<usize>>),
            );
        }
        Ok(())
    });
    if let Err(error) = result {
        let message = c_message(&error);
        // SAFETY: as above; DuckDB copies the message.
        unsafe { ffi::duckdb_init_set_error(info, message.as_ptr()) };
    }
}

unsafe extern "C" fn scan(info: ffi::duckdb_function_info, output: ffi::duckdb_data_chunk) {
    // SAFETY: DuckDB passes the live function info and output chunk of an
    // `oxedi_schema` scan.
    let result = guarded(|| unsafe { scan_with(info, output) });
    match result {
        Ok(len) => {
            // SAFETY: as above.
            unsafe { ffi::duckdb_data_chunk_set_size(output, len as ffi::idx_t) };
        }
        Err(error) => {
            let message = c_message(&error);
            // SAFETY: as above; DuckDB copies the message.
            unsafe { ffi::duckdb_function_set_error(info, message.as_ptr()) };
        }
    }
}

/// Emits the next rows, at most DuckDB's vector size; 0 once all are out.
///
/// # Safety
///
/// `info` and `output` must be the live function info and chunk of an
/// `oxedi_schema` scan.
unsafe fn scan_with(
    info: ffi::duckdb_function_info,
    output: ffi::duckdb_data_chunk,
) -> Result<usize, SchemaError> {
    let missing = |what: &str| SchemaError::Internal {
        message: format!("the scan has no {what}"),
    };
    // SAFETY: the bind data is the `Vec<Row>` set by `bind_with` and the
    // init data the `Mutex<usize>` set by `init`; both live until the scan
    // ends.
    let (rows, state) = unsafe {
        (
            ffi::duckdb_function_get_bind_data(info)
                .cast::<Vec<Row>>()
                .as_ref(),
            ffi::duckdb_function_get_init_data(info)
                .cast::<Mutex<usize>>()
                .as_ref(),
        )
    };
    let rows = rows.ok_or_else(|| missing("bind data"))?;
    let state = state.ok_or_else(|| missing("init data"))?;
    let mut next = state.lock().map_err(|_| SchemaError::Internal {
        message: "the scan state was poisoned by an earlier panic".to_owned(),
    })?;
    // SAFETY: the vector size is a constant of the running DuckDB.
    let capacity = usize::try_from(unsafe { ffi::duckdb_vector_size() }).unwrap_or(0);
    let batch = rows.get(*next..).unwrap_or_default();
    let batch = batch.get(..capacity.min(batch.len())).unwrap_or_default();
    // SAFETY: `output` is the scan's chunk with the five columns of
    // `COLUMNS`, each with `capacity` rows (caller contract).
    let mut vectors: Vec<Vector> = (0..COLUMNS.len())
        .map(|index| unsafe { Vector::of(output, index, capacity) })
        .collect();
    if let [table, column, kind, parent, place] = vectors.as_mut_slice() {
        for (index, row) in batch.iter().enumerate() {
            table.set_text(index, &row.table);
            column.set_text(index, &row.column);
            kind.set_text(index, &row.sql.to_string());
            match &row.parent {
                Some(name) => parent.set_text(index, name),
                None => parent.set_null(index),
            }
            // SAFETY: `position` is the INTEGER column of `COLUMNS`.
            unsafe { place.set_integer(index, row.position) };
        }
    }
    *next += batch.len();
    Ok(batch.len())
}

#[cfg(test)]
mod tests;
