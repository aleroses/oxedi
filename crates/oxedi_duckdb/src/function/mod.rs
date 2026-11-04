//! Registration of `read_835` on the C API, and its bind, init and scan
//! callbacks. Every callback catches panics, so none unwinds into DuckDB;
//! errors are reported through the callback's info. The panic guard and the
//! message conversion are shared with the copy format.

use std::any::Any;
use std::ffi::{CString, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};

use libduckdb_sys as ffi;
use oxedi_core::ColumnType;

use crate::builtins::{BuiltinSchema, Builtins};
use crate::diagnostics;
use crate::error::{FUNCTION, ReadError};
use crate::files::{self, CallerSettings, ClientContext, FileSystem};
use crate::options::{NAMED, Options, TableChoice};
use crate::scan::{Bound, Scan};
use crate::types::{self, SqlType};

/// The name of the output column that holds each row's file.
const FILENAME_COLUMN: &str = "filename";

/// Registers `read_835` on `connection`, with `builtins` as the function's
/// extra info.
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
    let any = types::any_type();
    // SAFETY: `function` is the live handle created above; every logical
    // type and string passed lives across the call, and DuckDB copies them.
    unsafe {
        ffi::duckdb_table_function_set_name(function.0, name.as_ptr());
        ffi::duckdb_table_function_add_parameter(function.0, any.raw());
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

/// A table function handle, destroyed on drop.
pub(crate) struct TableFunction(pub(crate) ffi::duckdb_table_function);

impl TableFunction {
    pub(crate) fn new() -> TableFunction {
        // SAFETY: creating a table function has no precondition.
        TableFunction(unsafe { ffi::duckdb_create_table_function() })
    }
}

impl Drop for TableFunction {
    fn drop(&mut self) {
        // SAFETY: the handle was created by DuckDB and is destroyed once.
        unsafe { ffi::duckdb_destroy_table_function(&mut self.0) };
    }
}

/// Drops a `Box<T>` DuckDB held as a raw pointer.
pub(crate) unsafe extern "C" fn drop_box<T>(pointer: *mut c_void) {
    if !pointer.is_null() {
        // SAFETY: `pointer` came from `Box::<T>::into_raw` and DuckDB
        // releases it once.
        drop(unsafe { Box::from_raw(pointer.cast::<T>()) });
    }
}

/// An error a callback can report a caught panic as.
pub(crate) trait FromPanic {
    /// The error for a panic with this message.
    fn from_panic(message: String) -> Self;
}

impl FromPanic for ReadError {
    fn from_panic(message: String) -> Self {
        ReadError::Internal { message }
    }
}

/// Runs `body`, turning a panic into an error.
pub(crate) fn guarded<T, E: FromPanic>(body: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    catch_unwind(AssertUnwindSafe(body))
        .unwrap_or_else(|payload| Err(E::from_panic(panic_message(payload.as_ref()))))
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "a panic without a message".to_owned()
    }
}

/// The error's message as a C string; a NUL byte is written `\0`.
pub(crate) fn c_message(error: &impl std::fmt::Display) -> CString {
    let text = error.to_string().replace('\0', "\\0");
    CString::new(text).unwrap_or_default()
}

unsafe extern "C" fn bind(info: ffi::duckdb_bind_info) {
    // SAFETY: DuckDB passes a live bind info to the bind callback.
    if let Err(error) = guarded(|| unsafe { bind_with(info) }) {
        let message = c_message(&error);
        // SAFETY: as above; DuckDB copies the message.
        unsafe { ffi::duckdb_bind_set_error(info, message.as_ptr()) };
    }
}

/// The table `read_835` returns and its columns, settled by the options.
struct Shape {
    table: String,
    columns: Vec<(String, ColumnType)>,
    types: Vec<SqlType>,
}

impl Shape {
    /// The shape of the table `options` names: the diagnostics, or one of
    /// `builtin`'s tables.
    fn of(builtin: &BuiltinSchema, options: &Options) -> Result<Shape, ReadError> {
        let name = table_name(builtin, &options.table_name)?;
        if name == diagnostics::TABLE {
            return Ok(Shape {
                table: diagnostics::TABLE.to_owned(),
                columns: diagnostics::columns(),
                types: diagnostics::types(options.binary)?,
            });
        }
        let table = builtin.table(name).ok_or_else(|| ReadError::UnknownTable {
            table: name.to_owned(),
            known: builtin
                .table_names()
                .into_iter()
                .chain([diagnostics::TABLE.to_owned()])
                .collect(),
        })?;
        let types = table
            .columns
            .iter()
            .map(|(column, kind)| SqlType::of(&table.name, column, *kind, options.binary))
            .collect::<Result<_, _>>()?;
        Ok(Shape {
            table: table.name.clone(),
            columns: table.columns.clone(),
            types,
        })
    }
}

/// The table `table_name` names, or the default table of `builtin`'s spec.
fn table_name<'a>(
    builtin: &'a BuiltinSchema,
    choice: &'a TableChoice,
) -> Result<&'a str, ReadError> {
    match choice {
        TableChoice::Named(name) => Ok(name),
        TableChoice::SpecDefault => {
            builtin
                .spec()
                .default_table()
                .ok_or_else(|| ReadError::Internal {
                    message: format!(
                        "the built-in spec for version {} declares no default table",
                        builtin.version()
                    ),
                })
        }
    }
}

fn no_context(what: &str) -> ReadError {
    ReadError::Internal {
        message: format!("the bind has no {what}"),
    }
}

/// The built-ins `read_835` was registered with.
///
/// # Safety
///
/// `info` must be the live bind info of `read_835`.
unsafe fn extra_info<'a>(info: ffi::duckdb_bind_info) -> Result<&'a Arc<Builtins>, ReadError> {
    // SAFETY: the extra info of `read_835` is the `Arc<Builtins>` set at
    // registration, alive as long as the function.
    unsafe {
        ffi::duckdb_bind_get_extra_info(info)
            .cast::<Arc<Builtins>>()
            .as_ref()
    }
    .ok_or_else(|| ReadError::Internal {
        message: "the function has no extra info".to_owned(),
    })
}

/// The files the call reads, and the file system that reads them.
///
/// # Safety
///
/// `info` must be the live bind info of `read_835`.
unsafe fn caller_files(
    info: ffi::duckdb_bind_info,
    options: &Options,
) -> Result<(Vec<String>, FileSystem), ReadError> {
    // SAFETY: `info` is the live bind info (caller contract); the context
    // is dropped at the end of this function.
    let context =
        unsafe { ClientContext::of_bind(info) }.ok_or_else(|| no_context("client context"))?;
    // The bind data holding the file system is dropped before the client
    // context it refers to.
    let file_system = context
        .file_system()
        .ok_or_else(|| no_context("file system"))?;
    let files = files::resolve(&options.paths, &CallerSettings::of(&context), &file_system)?;
    Ok((files, file_system))
}

/// # Safety
///
/// `info` must be the live bind info of `read_835`.
unsafe fn bind_with(info: ffi::duckdb_bind_info) -> Result<(), ReadError> {
    // SAFETY: `info` is the live bind info (caller contract).
    let builtins = unsafe { extra_info(info) }?;
    // SAFETY: as above.
    let options = unsafe { Options::of_bind(info) }?;
    let builtin = builtins.pick(options.version.as_deref())?;
    let shape = Shape::of(builtin, &options)?;
    // SAFETY: as above.
    unsafe { add_columns(info, &shape, options.filename) }?;
    // SAFETY: as above.
    let (files, file_system) = unsafe { caller_files(info, &options) }?;
    let bound = Bound {
        files,
        file_system,
        builtins: Arc::clone(builtins),
        diagnostics: shape.table == diagnostics::TABLE,
        table: shape.table,
        columns: shape.columns,
        types: shape.types,
        bound_version: builtin.version(),
        forced: options.version.is_some(),
        filename: options.filename,
        ignore_errors: options.ignore_errors,
        binary: options.binary,
        mask: options.mask,
    };
    // SAFETY: `info` is live; DuckDB owns the box from here and frees it
    // with `drop_box::<Bound>`.
    unsafe {
        ffi::duckdb_bind_set_bind_data(
            info,
            Box::into_raw(Box::new(bound)).cast(),
            Some(drop_box::<Bound>),
        );
    }
    Ok(())
}

/// Adds the result columns of `shape` to the bind, then the file name column
/// when `filename` is set.
///
/// # Safety
///
/// `info` must be the live bind info of the running bind callback.
unsafe fn add_columns(
    info: ffi::duckdb_bind_info,
    shape: &Shape,
    filename: bool,
) -> Result<(), ReadError> {
    for ((column, _), sql) in shape.columns.iter().zip(&shape.types) {
        // SAFETY: `info` is the live bind info (caller contract).
        unsafe { add_column(info, column, *sql) }?;
    }
    if filename {
        // SAFETY: as above.
        unsafe { add_column(info, FILENAME_COLUMN, SqlType::Varchar) }?;
    }
    Ok(())
}

/// Adds one result column to the bind.
///
/// # Safety
///
/// `info` must be the live bind info of the running bind callback.
unsafe fn add_column(
    info: ffi::duckdb_bind_info,
    name: &str,
    sql: SqlType,
) -> Result<(), ReadError> {
    let c_name = CString::new(name).map_err(|_| ReadError::Internal {
        message: format!("the column name {name:?} holds a NUL byte"),
    })?;
    let kind = sql.logical_type();
    // SAFETY: `info` is the live bind info (caller contract); DuckDB copies
    // the name and type.
    unsafe { ffi::duckdb_bind_add_result_column(info, c_name.as_ptr(), kind.raw()) };
    Ok(())
}

unsafe extern "C" fn init(info: ffi::duckdb_init_info) {
    let result = guarded(|| -> Result<(), ReadError> {
        let state = Box::new(Mutex::new(Scan::default()));
        // SAFETY: DuckDB passes a live init info; it owns the box from here
        // and frees it with `drop_box`.
        unsafe {
            ffi::duckdb_init_set_max_threads(info, 1);
            ffi::duckdb_init_set_init_data(
                info,
                Box::into_raw(state).cast(),
                Some(drop_box::<Mutex<Scan>>),
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
    // SAFETY: DuckDB passes the live function info and output chunk of a
    // `read_835` scan.
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

/// # Safety
///
/// `info` and `output` must be the live function info and chunk of a
/// `read_835` scan.
unsafe fn scan_with(
    info: ffi::duckdb_function_info,
    output: ffi::duckdb_data_chunk,
) -> Result<usize, ReadError> {
    let missing = |what: &str| ReadError::Internal {
        message: format!("the scan has no {what}"),
    };
    // SAFETY: the bind data of `read_835` is the `Bound` set by `bind_with`
    // and its init data the `Mutex<Scan>` set by `init`; both live until the
    // scan ends.
    let (bound, state) = unsafe {
        (
            ffi::duckdb_function_get_bind_data(info)
                .cast::<Bound>()
                .as_ref(),
            ffi::duckdb_function_get_init_data(info)
                .cast::<Mutex<Scan>>()
                .as_ref(),
        )
    };
    let bound = bound.ok_or_else(|| missing("bind data"))?;
    let state = state.ok_or_else(|| missing("init data"))?;
    let mut scan = state.lock().map_err(|_| ReadError::Internal {
        message: "the scan state was poisoned by an earlier panic".to_owned(),
    })?;
    // SAFETY: `output` is the scan's chunk (caller contract).
    unsafe { scan.next_chunk(bound, output) }
}

#[cfg(test)]
mod tests;
