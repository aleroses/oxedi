//! The `edi835` copy format: `COPY (query) TO 'file' (FORMAT edi835, …)`
//! writes the query's tables as one 835 interchange with the core's writer.
//!
//! The query returns one STRUCT column whose fields are tables of the spec,
//! each a list of structs with the table's columns (`SELECT {'claims':
//! (SELECT list(c ORDER BY c."row") FROM claims c)}`); several query rows concatenate. The
//! bind reads the options (the envelope and the version) and binds each
//! field of the STRUCT to its table; the sink appends every chunk's rows;
//! the finalize writes, and opens the target only after the writer
//! accepted the tables, so a refusal does not open it. What DuckDB then
//! does with a target of a failed `COPY` is its own: with a local path and
//! its temporary file an existing file stays as it was; otherwise it may
//! remove the target depending on its version. Every callback catches panics,
//! so none unwinds into DuckDB.
//!
//! - `mod.rs`: registration and the bind, init, sink and finalize callbacks.
//! - `error.rs`: the errors the format reports.
//! - `options.rs`: the options and the envelope they make.
//! - `read.rs`: the options read from DuckDB's value.
//! - `input.rs`: the query's column bound to the spec's tables.
//! - `sink.rs`: the rows of one input chunk.
//! - `convert.rs`: DuckDB values to the spec's column types.

mod convert;
mod error;
mod input;
mod options;
mod read;
mod sink;

use std::ffi::{CStr, CString};
use std::sync::{Arc, Mutex, MutexGuard};

use libduckdb_sys as ffi;
use oxedi_core::write::{Envelope, write};
use oxedi_core::{Table, Tables};

use crate::builtins::Builtins;
use crate::files::ClientContext;
use crate::function::{c_message, drop_box, guarded};
use crate::types::LogicalType;
use error::{CopyError, FORMAT};
use input::BoundTable;
use options::Settings;

/// Registers the `edi835` copy format on `connection`, with `builtins` as
/// the function's extra info.
///
/// # Safety
///
/// `connection` must be a live connection; the C API must be initialized.
pub unsafe fn register(
    connection: ffi::duckdb_connection,
    builtins: Arc<Builtins>,
) -> Result<(), String> {
    let name = CString::new(FORMAT).map_err(|err| err.to_string())?;
    let function = CopyFunction::new();
    // SAFETY: `function` is the live handle created above; DuckDB copies the
    // name, and owns the extra info from here, freeing it with `drop_box`.
    unsafe {
        ffi::duckdb_copy_function_set_name(function.0, name.as_ptr());
        let extra = Box::into_raw(Box::new(builtins));
        ffi::duckdb_copy_function_set_extra_info(
            function.0,
            extra.cast(),
            Some(drop_box::<Arc<Builtins>>),
        );
        ffi::duckdb_copy_function_set_bind(function.0, Some(bind));
        ffi::duckdb_copy_function_set_global_init(function.0, Some(global_init));
        ffi::duckdb_copy_function_set_sink(function.0, Some(sink));
        ffi::duckdb_copy_function_set_finalize(function.0, Some(finalize));
    }
    // SAFETY: both handles are live; DuckDB copies the function into the
    // catalog, extra info included.
    let state = unsafe { ffi::duckdb_register_copy_function(connection, function.0) };
    if state == ffi::duckdb_state_DuckDBSuccess {
        Ok(())
    } else {
        Err(format!("the copy format {FORMAT} could not be registered"))
    }
}

/// A copy function handle, destroyed on drop.
struct CopyFunction(ffi::duckdb_copy_function);

impl CopyFunction {
    fn new() -> CopyFunction {
        // SAFETY: creating a copy function has no precondition.
        CopyFunction(unsafe { ffi::duckdb_create_copy_function() })
    }
}

impl Drop for CopyFunction {
    fn drop(&mut self) {
        // SAFETY: the handle was created by DuckDB and is destroyed once.
        unsafe { ffi::duckdb_destroy_copy_function(&mut self.0) };
    }
}

/// What the bind settled: the spec, the envelope and the input tables.
struct Bound {
    builtins: Arc<Builtins>,
    version: &'static str,
    envelope: Envelope,
    tables: Vec<BoundTable>,
    /// The first file DuckDB asked this bind for. Asking for another one
    /// is what per-thread and partitioned output do; running the same
    /// statement again asks for the same file.
    target: Mutex<Option<String>>,
}

/// The state of one `COPY`: the target and the tables built so far.
struct State {
    path: String,
    tables: Mutex<Vec<Table>>,
}

impl State {
    /// The tables built so far, locked.
    fn lock(&self) -> Result<MutexGuard<'_, Vec<Table>>, CopyError> {
        self.tables.lock().map_err(|_| CopyError::Internal {
            message: "the tables were poisoned by an earlier panic".to_owned(),
        })
    }
}

/// The bind data and the global state a callback received, both required.
fn parts<'a>(
    bound: Option<&'a Bound>,
    state: Option<&'a State>,
) -> Result<(&'a Bound, &'a State), CopyError> {
    let bound = bound.ok_or_else(|| missing("bind data"))?;
    let state = state.ok_or_else(|| missing("global state"))?;
    Ok((bound, state))
}

fn missing(what: &str) -> CopyError {
    CopyError::Internal {
        message: format!("DuckDB gave no {what}"),
    }
}

unsafe extern "C" fn bind(info: ffi::duckdb_copy_function_bind_info) {
    // SAFETY: DuckDB passes a live bind info to the bind callback.
    if let Err(error) = guarded(|| unsafe { bind_with(info) }) {
        let message = c_message(&error);
        // SAFETY: as above; DuckDB copies the message.
        unsafe { ffi::duckdb_copy_function_bind_set_error(info, message.as_ptr()) };
    }
}

/// The type of each column the `COPY` writes.
///
/// # Safety
///
/// `info` must be the live bind info of the `edi835` copy format.
unsafe fn input_columns(
    info: ffi::duckdb_copy_function_bind_info,
) -> Result<Vec<input::InputColumn>, CopyError> {
    // SAFETY: `info` is live (caller contract).
    let count = unsafe { ffi::duckdb_copy_function_bind_get_column_count(info) };
    (0..count)
        .map(|index| {
            // SAFETY: `index` is below the column count; the type returned is
            // owned by the guard, which destroys it.
            let kind = unsafe {
                LogicalType::owned(ffi::duckdb_copy_function_bind_get_column_type(info, index))
            }
            .ok_or_else(|| missing("column type"))?;
            // SAFETY: `kind` is live.
            Ok(unsafe { input::column_of(&kind) })
        })
        .collect()
}

/// # Safety
///
/// `info` must be the live bind info of the `edi835` copy format.
unsafe fn bind_with(info: ffi::duckdb_copy_function_bind_info) -> Result<(), CopyError> {
    // SAFETY: the extra info is the `Arc<Builtins>` set at registration,
    // alive as long as the function.
    let builtins = unsafe {
        ffi::duckdb_copy_function_bind_get_extra_info(info)
            .cast::<Arc<Builtins>>()
            .as_ref()
    }
    .ok_or_else(|| missing("extra info"))?;
    // SAFETY: `info` is the live bind info (caller contract).
    let given = unsafe { read::options(info) };
    let settings = Settings::parse(&given)?;
    let builtin = builtins.by_version(&settings.version)?;
    // SAFETY: `info` is live (caller contract).
    let columns = unsafe { input_columns(info) }?;
    let tables = input::bind(builtin, &columns)?;
    let bound = Bound {
        builtins: Arc::clone(builtins),
        version: builtin.version(),
        envelope: settings.envelope,
        tables,
        target: Mutex::new(None),
    };
    // SAFETY: `info` is live; DuckDB owns the box from here and frees it
    // with `drop_box::<Bound>`.
    unsafe {
        ffi::duckdb_copy_function_bind_set_bind_data(
            info,
            Box::into_raw(Box::new(bound)).cast(),
            Some(drop_box::<Bound>),
        );
    }
    Ok(())
}

unsafe extern "C" fn global_init(info: ffi::duckdb_copy_function_global_init_info) {
    let result = guarded(|| -> Result<(), CopyError> {
        // SAFETY: the bind data is the `Bound` set by `bind_with`, alive
        // until the copy ends; the path is a C string DuckDB owns during the
        // call, copied here.
        let (bound, path) = unsafe {
            (
                ffi::duckdb_copy_function_global_init_get_bind_data(info)
                    .cast::<Bound>()
                    .as_ref(),
                ffi::duckdb_copy_function_global_init_get_file_path(info),
            )
        };
        let bound = bound.ok_or_else(|| missing("bind data"))?;
        if path.is_null() {
            return Err(missing("file path"));
        }
        // SAFETY: `path` is a live NUL-terminated string (checked non-null).
        let path = unsafe { CStr::from_ptr(path) }
            .to_string_lossy()
            .into_owned();
        {
            let mut target = bound.target.lock().map_err(|_| CopyError::Internal {
                message: "the target was poisoned by an earlier panic".to_owned(),
            })?;
            match target.as_deref() {
                Some(first) if first != path => {
                    return Err(CopyError::SecondFile { path });
                }
                Some(_) => {}
                None => *target = Some(path.clone()),
            }
        }
        let tables = bound
            .tables
            .iter()
            .map(|table| {
                Table::new(
                    table.table.clone(),
                    table
                        .fields
                        .iter()
                        .map(|field| (field.name.clone(), field.kind)),
                )
            })
            .collect();
        let state = State {
            path,
            tables: Mutex::new(tables),
        };
        // SAFETY: `info` is live; DuckDB owns the box from here and frees
        // it with `drop_box::<State>`.
        unsafe {
            ffi::duckdb_copy_function_global_init_set_global_state(
                info,
                Box::into_raw(Box::new(state)).cast(),
                Some(drop_box::<State>),
            );
        }
        Ok(())
    });
    if let Err(error) = result {
        let message = c_message(&error);
        // SAFETY: DuckDB passes a live info and copies the message.
        unsafe { ffi::duckdb_copy_function_global_init_set_error(info, message.as_ptr()) };
    }
}

unsafe extern "C" fn sink(
    info: ffi::duckdb_copy_function_sink_info,
    input: ffi::duckdb_data_chunk,
) {
    let result = guarded(|| -> Result<(), CopyError> {
        // SAFETY: the bind data and global state are the `Bound` and `State`
        // set above, alive until the copy ends.
        let (bound, state) = unsafe {
            (
                ffi::duckdb_copy_function_sink_get_bind_data(info)
                    .cast::<Bound>()
                    .as_ref(),
                ffi::duckdb_copy_function_sink_get_global_state(info)
                    .cast::<State>()
                    .as_ref(),
            )
        };
        let (bound, state) = parts(bound, state)?;
        let mut tables = state.lock()?;
        // SAFETY: `input` is the flattened chunk of the bound tables.
        unsafe { sink::append(input, &bound.tables, &mut tables) }
    });
    if let Err(error) = result {
        let message = c_message(&error);
        // SAFETY: DuckDB passes a live info and copies the message.
        unsafe { ffi::duckdb_copy_function_sink_set_error(info, message.as_ptr()) };
    }
}

unsafe extern "C" fn finalize(info: ffi::duckdb_copy_function_finalize_info) {
    // SAFETY: DuckDB passes a live finalize info.
    if let Err(error) = guarded(|| unsafe { finalize_with(info) }) {
        let message = c_message(&error);
        // SAFETY: as above; DuckDB copies the message.
        unsafe { ffi::duckdb_copy_function_finalize_set_error(info, message.as_ptr()) };
    }
}

/// # Safety
///
/// `info` must be the live finalize info of the `edi835` copy format.
unsafe fn finalize_with(info: ffi::duckdb_copy_function_finalize_info) -> Result<(), CopyError> {
    // SAFETY: the bind data and global state are the `Bound` and `State`
    // set above, alive until the copy ends.
    let (bound, state) = unsafe {
        (
            ffi::duckdb_copy_function_finalize_get_bind_data(info)
                .cast::<Bound>()
                .as_ref(),
            ffi::duckdb_copy_function_finalize_get_global_state(info)
                .cast::<State>()
                .as_ref(),
        )
    };
    let (bound, state) = parts(bound, state)?;
    let tables = std::mem::take(&mut *state.lock()?);
    let builtin = bound
        .builtins
        .by_version(bound.version)
        .map_err(|_| missing("built-in spec of the bound version"))?;
    let bytes =
        write(builtin.spec(), &Tables::new(tables), &bound.envelope).map_err(CopyError::Write)?;
    // SAFETY: `info` is live (caller contract).
    unsafe { write_file(info, &state.path, &bytes) }
}

/// Writes `bytes` to `path` through the file system of the call.
///
/// # Safety
///
/// `info` must be the live finalize info of the `edi835` copy format.
unsafe fn write_file(
    info: ffi::duckdb_copy_function_finalize_info,
    path: &str,
    bytes: &[u8],
) -> Result<(), CopyError> {
    // SAFETY: `info` is live (caller contract); the wrapper is owned here
    // and dropped before this function returns.
    let context = unsafe {
        ClientContext::owned(ffi::duckdb_copy_function_finalize_get_client_context(info))
    }
    .ok_or_else(|| missing("client context"))?;
    let file_system = context
        .file_system()
        .ok_or_else(|| missing("file system"))?;
    file_system
        .write_all(path, bytes)
        .map_err(|failure| CopyError::Output {
            path: path.to_owned(),
            step: failure.step,
            message: failure.message,
        })
}

#[cfg(test)]
mod tests;
