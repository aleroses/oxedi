//! `oxedi`, a DuckDB extension over the oxedi EDI 835 parser core.
//!
//! It registers two table functions and a copy format. The first:
//!
//! ```sql
//! read_835(path, table_name := <the spec's default table>, filename := false, version := NULL,
//!          binary := false, ignore_errors := false)
//! ```
//!
//! `path` is a path, a glob pattern or a list of them, opened through
//! DuckDB's file system. Each file is parsed with the built-in spec of the
//! version it declares (or `version`), and the rows of one projected table
//! are returned with the column types the core declares. Without
//! `table_name`, the table is the one the spec declares as its
//! `default_table`.
//!
//! `table_name := 'diagnostics'` returns the core's findings instead, one
//! row per finding (`level`, `kind`, `rule`, `segment`, `element`,
//! `component`, `path`, `datum`, `origin`, `code`). A file that is not an X12
//! interchange fails the query, unless `ignore_errors := true`: then it adds
//! one `diagnostics` row whose `rule` is the error's text and no rows to any
//! other table, and the scan goes on with the next file.
//!
//! Glob patterns are expanded by a private in-memory DuckDB that follows the
//! caller's settings (see `files::glob`): with `enable_external_access` off a
//! pattern is an error, and a remote pattern (`s3://bucket/*.835`) sees the
//! caller's persistent secrets only, not temporary `CREATE SECRET` ones.
//! `http://` and `https://` paths are never patterns. Plain paths and lists
//! of them are read through the caller's own file system and secrets.
//!
//! The second, `oxedi_schema(version := '5010', binary := false)`, needs no
//! file: it returns one row per column of every table `read_835` returns
//! for that version (`diagnostics` included): `table_name`, `column_name`,
//! `column_type` (the DuckDB type `read_835` gives it with the same
//! `binary`), `parent` (the table a reference column points at, else NULL)
//! and `position` (1-based, in the table).
//!
//! The copy format `edi835` writes the tables of the spec back into an 835:
//!
//! ```sql
//! COPY (SELECT {'payments': (SELECT list(p ORDER BY p."row") FROM payments p),
//!               'claims': (SELECT list(c ORDER BY c."row") FROM claims c)})
//! TO 'out.835' (FORMAT edi835, sender_id 'SENDER', receiver_id 'RECEIVER',
//!               date '2024-01-02', time '10:30')
//! ```
//!
//! The query returns one STRUCT column whose fields are named after the
//! spec's tables, each a list of structs, one struct per row with the
//! table's columns; the lists of several query rows concatenate. The
//! options are the fields of `oxedi.Envelope` and `version` (`'5010'` by
//! default, `'4010'`). The file is written with the core's writer in strict
//! mode: any finding fails the `COPY` and nothing is written.
//!
//! The extension is built on DuckDB's stable C API; it keeps no process-wide
//! state of its own: the built-in specs live in the functions' extra info.
//!
//! - `builtins`: the built-in specs and the schema of their tables.
//! - `copy`: the `edi835` copy format.
//! - `diagnostics`: the `diagnostics` table.
//! - `error`: the errors `read_835` reports.
//! - `files`: reading and writing files through DuckDB's file system.
//! - `function`: `read_835`'s registration and callbacks, and the panic
//!   guard every callback uses.
//! - `options`: the arguments of a call.
//! - `oxedi_schema`: the `oxedi_schema` table function.
//! - `scan`: emitting one file's rows at a time.
//! - `types`: the DuckDB type of each core column type; owned logical types.
//! - `value`: owned DuckDB values and C strings read through the C API.

mod builtins;
mod copy;
mod diagnostics;
mod error;
mod files;
mod function;
mod options;
mod oxedi_schema;
mod scan;
mod types;
mod value;

use std::ffi::CString;
use std::sync::Arc;

use libduckdb_sys as ffi;

/// The oldest DuckDB C API the extension asks for; the build passes the
/// target version.
const MIN_DUCKDB_VERSION: &str = match option_env!("DUCKDB_EXTENSION_MIN_DUCKDB_VERSION") {
    Some(version) => version,
    None => "v1.5.6",
};

/// Initializes the C API and registers the functions on a short-lived
/// connection to the loading database.
///
/// # Safety
///
/// `info` and `access` must be the pointers DuckDB passed to the entry point.
unsafe fn load(
    info: ffi::duckdb_extension_info,
    access: *const ffi::duckdb_extension_access,
) -> Result<bool, String> {
    // SAFETY: `info` and `access` come from DuckDB (caller contract).
    let ready = unsafe { ffi::duckdb_rs_extension_api_init(info, access, MIN_DUCKDB_VERSION) }
        .map_err(str::to_owned)?;
    if !ready {
        return Ok(false);
    }
    // SAFETY: `access` is DuckDB's access struct (caller contract).
    let get_database = unsafe { access.as_ref() }
        .and_then(|access| access.get_database)
        .ok_or("DuckDB gave no get_database callback")?;
    // SAFETY: `get_database` is DuckDB's; the database it returns is valid
    // during the load only, so it is used here and not kept.
    let database = unsafe { get_database(info).as_ref() }
        .copied()
        .ok_or("DuckDB gave no database")?;
    let mut connection: ffi::duckdb_connection = std::ptr::null_mut();
    // SAFETY: `database` is live during the load; DuckDB writes a new
    // connection into `connection`.
    if unsafe { ffi::duckdb_connect(database, &mut connection) } != ffi::duckdb_state_DuckDBSuccess
    {
        return Err("could not connect to the loading database".to_owned());
    }
    let builtins = Arc::new(builtins::Builtins::load());
    // SAFETY: `connection` is the live connection created above.
    let registered = unsafe { function::register(connection, Arc::clone(&builtins)) }
        // SAFETY: as above.
        .and_then(|()| unsafe { oxedi_schema::register(connection, Arc::clone(&builtins)) })
        // SAFETY: as above.
        .and_then(|()| unsafe { copy::register(connection, builtins) });
    // SAFETY: `connection` was created above and is closed once; the
    // registered function does not refer to it.
    unsafe { ffi::duckdb_disconnect(&mut connection) };
    registered.map(|()| true)
}

/// The entry point DuckDB calls on `LOAD oxedi`.
///
/// # Safety
///
/// Called by DuckDB with valid `info` and `access` pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxedi_init_c_api(
    info: ffi::duckdb_extension_info,
    access: *const ffi::duckdb_extension_access,
) -> bool {
    // SAFETY: `info` and `access` come from DuckDB (caller contract).
    let outcome = std::panic::catch_unwind(|| unsafe { load(info, access) })
        .unwrap_or_else(|_| Err("a panic while loading oxedi".to_owned()));
    match outcome {
        Ok(loaded) => loaded,
        Err(message) => {
            // SAFETY: `access` is DuckDB's access struct (caller contract).
            if let Some(set_error) = unsafe { access.as_ref() }.and_then(|access| access.set_error)
            {
                let message = CString::new(format!("oxedi: {message}").replace('\0', "\\0"))
                    .unwrap_or_default();
                // SAFETY: DuckDB copies the message during the call.
                unsafe { set_error(info, message.as_ptr()) };
            }
            false
        }
    }
}
