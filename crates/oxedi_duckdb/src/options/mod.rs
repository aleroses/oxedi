//! The arguments of `read_835`, read from the bind info.

use std::ffi::CString;

use libduckdb_sys as ffi;
use oxedi_core::{DateMode, Mask};

use crate::error::ReadError;
use crate::value::Value;

/// The named parameters, in registration order, with whether each is a
/// boolean (else a VARCHAR).
pub const NAMED: [(&str, bool); 7] = [
    ("table_name", false),
    ("filename", true),
    ("version", false),
    ("binary", true),
    ("ignore_errors", true),
    ("mask_key", false),
    ("mask_dates", false),
];

/// What one call of `read_835` asked for.
#[derive(Debug, Clone)]
pub struct Options {
    /// The paths as given: plain paths or glob patterns.
    pub paths: Vec<String>,
    /// The table to return.
    pub table_name: TableChoice,
    /// Whether to add a `filename` column.
    pub filename: bool,
    /// The forced spec version, if any.
    pub version: Option<String>,
    /// Whether text columns are `BLOB` instead of `VARCHAR`.
    pub binary: bool,
    /// Whether a file that cannot be parsed is reported instead of failing.
    pub ignore_errors: bool,
    /// The mask that hides patient data, when `mask_key` was given.
    pub mask: Option<Mask>,
}

/// Which table `read_835` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableChoice {
    /// `table_name` was not given: the default table of the chosen spec.
    SpecDefault,
    /// The table `table_name` names.
    Named(String),
}

/// A named parameter as the call gave it.
pub(crate) enum Named {
    /// Not given.
    Absent,
    /// Given as SQL NULL.
    Null,
    /// Given with a value.
    Given(Value),
}

impl Options {
    /// Reads the arguments of the bind in progress.
    ///
    /// # Safety
    ///
    /// `info` must be the bind info DuckDB passed to the running bind
    /// callback of a function registered with one positional parameter and
    /// the named parameters of [`NAMED`].
    pub unsafe fn of_bind(info: ffi::duckdb_bind_info) -> Result<Options, ReadError> {
        let bind = Bind { info };
        let paths = bind.paths()?;
        let table_name = bind
            .text("table_name")?
            .map_or(TableChoice::SpecDefault, TableChoice::Named);
        let [filename, binary, ignore_errors] =
            bind.flags(["filename", "binary", "ignore_errors"])?;
        Ok(Options {
            paths,
            table_name,
            filename,
            version: bind.version()?,
            binary,
            ignore_errors,
            mask: bind.mask()?,
        })
    }
}

/// The bind in progress, read one parameter at a time.
struct Bind {
    /// The bind info of a running bind of a function registered with one
    /// positional parameter and the named parameters of [`NAMED`]; the
    /// contract of [`Options::of_bind`].
    info: ffi::duckdb_bind_info,
}

impl Bind {
    fn named(&self, name: &str) -> Named {
        // SAFETY: `info` is a live bind info and every name read is one of
        // NAMED (the contract of `Options::of_bind`).
        unsafe { named(self.info, name) }
    }

    /// The paths of the one positional parameter.
    fn paths(&self) -> Result<Vec<String>, ReadError> {
        // SAFETY: index 0 is the one positional parameter (the contract of
        // `Options::of_bind`); the value is owned by the caller and taken by
        // `owned`.
        let path = unsafe { Value::owned(ffi::duckdb_bind_get_parameter(self.info, 0)) };
        match path {
            Some(path) => paths_of(&path),
            None => Err(ReadError::NullPath { position: None }),
        }
    }

    /// A text parameter: `None` when not given, refused when NULL.
    fn text(&self, name: &'static str) -> Result<Option<String>, ReadError> {
        match self.named(name) {
            Named::Absent => Ok(None),
            Named::Null => Err(ReadError::NullOption { name }),
            Named::Given(value) => constant(name, value.text()).map(Some),
        }
    }

    /// The `version`: not given and NULL both leave the choice to the file.
    fn version(&self) -> Result<Option<String>, ReadError> {
        match self.named("version") {
            Named::Absent | Named::Null => Ok(None),
            Named::Given(value) => constant("version", value.text()).map(Some),
        }
    }

    /// The mask `mask_key` and `mask_dates` ask for.
    fn mask(&self) -> Result<Option<Mask>, ReadError> {
        mask_of(self.text("mask_key")?, self.text("mask_dates")?)
    }

    /// Boolean parameters, `false` when not given, refused when NULL.
    fn flags<const N: usize>(&self, names: [&'static str; N]) -> Result<[bool; N], ReadError> {
        let mut flags = [false; N];
        for (flag, name) in flags.iter_mut().zip(names) {
            *flag = match self.named(name) {
                Named::Absent => false,
                Named::Null => return Err(ReadError::NullOption { name }),
                Named::Given(value) => value.boolean(),
            };
        }
        Ok(flags)
    }
}

/// The mask the key and date mode ask for: none without a key, and a date
/// mode without a key is refused.
fn mask_of(key: Option<String>, dates: Option<String>) -> Result<Option<Mask>, ReadError> {
    let Some(key) = key else {
        return match dates {
            Some(dates) => Err(ReadError::MaskDatesWithoutKey { dates }),
            None => Ok(None),
        };
    };
    let dates = match dates {
        Some(dates) => DateMode::parse(&dates).map_err(|source| ReadError::Mask {
            name: "mask_dates",
            source,
        })?,
        None => DateMode::Year,
    };
    Mask::new(key.as_bytes(), dates)
        .map(Some)
        .map_err(|source| ReadError::Mask {
            name: "mask_key",
            source,
        })
}

/// The named parameter `name` of the bind.
///
/// # Safety
///
/// `info` must be a live bind info.
pub(crate) unsafe fn named(info: ffi::duckdb_bind_info, name: &str) -> Named {
    let Ok(name) = CString::new(name) else {
        return Named::Absent;
    };
    // SAFETY: `info` is live (caller contract) and `name` is a C string; the
    // value returned (null when the parameter was not given) is owned here.
    let raw = unsafe { ffi::duckdb_bind_get_named_parameter(info, name.as_ptr()) };
    if raw.is_null() {
        return Named::Absent;
    }
    // SAFETY: `raw` is a live value handle owned here.
    match unsafe { Value::owned(raw) } {
        Some(value) => Named::Given(value),
        None => Named::Null,
    }
}

/// The prefix of the name DuckDB gives a table macro's parameters while it
/// binds the macro's body.
const MACRO_PARAMETERS: &str = "0_macro_parameters";

/// The text of the named parameter `name`, refused when it is a table
/// macro's parameter rather than a value.
///
/// When a table macro is created, DuckDB binds its body once. A table
/// function argument that refers to a macro parameter normally defers that
/// bind to the macro's call, but a bare parameter given to a named parameter
/// with `:=` or `=>` is not recognized: DuckDB passes its qualified name,
/// `0_macro_parameters<macro>.<parameter>`, as a string constant. The C API
/// hands the bind only that string, and a bind error cannot ask DuckDB to
/// defer, so the prefix is the only sign that the value is not a constant.
/// A real table name or version never starts with it.
fn constant(name: &'static str, text: String) -> Result<String, ReadError> {
    match macro_parameter(&text) {
        Some(parameter) => Err(ReadError::MacroParameter {
            name,
            parameter,
            value: text,
        }),
        None => Ok(text),
    }
}

/// Whether `text` stands for a table macro parameter (see [`constant`]): `None`
/// when it is a value, `Some(Some(name))` with the parameter's name when
/// exactly one dot separates the macro from the parameter, and `Some(None)`
/// when more dots make the split ambiguous (the macro's name may contain a
/// dot too).
pub(crate) fn macro_parameter(text: &str) -> Option<Option<String>> {
    let qualified = text.strip_prefix(MACRO_PARAMETERS)?;
    let (_, parameter) = qualified.split_once('.')?;
    Some((!parameter.contains('.')).then(|| parameter.to_owned()))
}

/// The paths of a VARCHAR or a list of VARCHAR.
fn paths_of(value: &Value) -> Result<Vec<String>, ReadError> {
    match value.type_id() {
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_VARCHAR => Ok(vec![value.text()]),
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_LIST => {
            let items = value.list();
            if items.is_empty() {
                return Err(ReadError::EmptyPathList);
            }
            items
                .iter()
                .enumerate()
                .map(|(index, item)| match item {
                    None => Err(ReadError::NullPath {
                        position: Some(index + 1),
                    }),
                    Some(item) if item.type_id() == ffi::DUCKDB_TYPE_DUCKDB_TYPE_VARCHAR => {
                        Ok(item.text())
                    }
                    Some(_) => Err(ReadError::PathType {
                        found: value.type_name(),
                    }),
                })
                .collect()
        }
        _ => Err(ReadError::PathType {
            found: value.type_name(),
        }),
    }
}

#[cfg(test)]
mod tests;
