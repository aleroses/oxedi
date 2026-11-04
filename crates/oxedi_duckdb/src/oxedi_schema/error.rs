//! The errors `oxedi_schema` reports. Each message names the rule that
//! failed, the parameter or the table and column, and the offending value.

use std::fmt;

use oxedi_core::ColumnType;

use crate::error::macro_hint;
use crate::function::FromPanic;

/// The name every message starts with, as DuckDB shows it to the user.
pub const FUNCTION: &str = "oxedi_schema";

/// Why `oxedi_schema` could not bind or scan.
#[derive(Debug)]
pub enum SchemaError {
    /// A named parameter that has no NULL meaning was given as NULL.
    NullOption {
        /// The parameter's name.
        name: &'static str,
    },
    /// `version` received a table macro's parameter, which DuckDB passed as
    /// the parameter's qualified name instead of a value.
    MacroParameter {
        /// The macro parameter forwarded to it, when its name can be told
        /// from the macro's.
        parameter: Option<String>,
        /// The text DuckDB passed.
        value: String,
    },
    /// `version` names no built-in spec.
    UnknownVersion {
        /// The core's account of the version given and the versions there are.
        source: oxedi_core::UnknownVersion,
    },
    /// A column type of the core has no DuckDB type here.
    UnsupportedType {
        /// The table described.
        table: String,
        /// The column.
        column: String,
        /// The core's type.
        kind: ColumnType,
    },
    /// Something that should not happen: a panic caught before it could
    /// cross into DuckDB, or a handle or state DuckDB did not provide.
    Internal {
        /// What went wrong.
        message: String,
    },
}

impl From<oxedi_core::UnknownVersion> for SchemaError {
    fn from(source: oxedi_core::UnknownVersion) -> SchemaError {
        SchemaError::UnknownVersion { source }
    }
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemaError::NullOption { name } => write!(
                f,
                "{FUNCTION}: {name} must not be NULL; leave it out to use its default"
            ),
            SchemaError::MacroParameter { parameter, value } => {
                let (passed, written) = macro_hint(parameter.as_deref(), value);
                write!(
                    f,
                    "{FUNCTION}: version must be a constant string; a table macro passed \
                     {passed}; DuckDB does not substitute a macro parameter given with := or \
                     =>, so write version = {written}"
                )
            }
            SchemaError::UnknownVersion { source } => write!(f, "{FUNCTION}: {source}"),
            SchemaError::UnsupportedType {
                table,
                column,
                kind,
            } => write!(
                f,
                "{FUNCTION}: table {table:?}, column {column:?}: the core type {kind} has no DuckDB type"
            ),
            SchemaError::Internal { message } => {
                write!(f, "{FUNCTION}: internal error: {message}")
            }
        }
    }
}

impl std::error::Error for SchemaError {}

impl FromPanic for SchemaError {
    fn from_panic(message: String) -> Self {
        SchemaError::Internal { message }
    }
}
