//! The errors `read_835` reports. Each message names the rule that failed,
//! where it failed (the parameter, or the file, table, column and row) and
//! the offending value.

use std::fmt;

use oxedi_core::{ColumnType, DocumentError, MaskError};

/// The name every message starts with, as DuckDB shows it to the user.
pub const FUNCTION: &str = "read_835";

/// Why `read_835` could not bind or scan.
#[derive(Debug)]
pub enum ReadError {
    /// The path argument is neither a string nor a list of strings.
    PathType {
        /// The DuckDB type of the argument.
        found: String,
    },
    /// The path argument, or one entry of the list, is NULL.
    NullPath {
        /// The 1-based position in the list, or `None` for a single path.
        position: Option<usize>,
    },
    /// The list of paths is empty.
    EmptyPathList,
    /// A named parameter that has no NULL meaning was given as NULL.
    NullOption {
        /// The parameter's name.
        name: &'static str,
    },
    /// A text parameter received a table macro's parameter, which DuckDB
    /// passed as the parameter's qualified name instead of a value.
    MacroParameter {
        /// The `read_835` parameter.
        name: &'static str,
        /// The macro parameter forwarded to it, when its name can be told
        /// from the macro's.
        parameter: Option<String>,
        /// The text DuckDB passed.
        value: String,
    },
    /// `mask_key` or `mask_dates` does not build a mask.
    Mask {
        /// The `read_835` parameter at fault.
        name: &'static str,
        /// The core's error.
        source: MaskError,
    },
    /// `mask_dates` was given without `mask_key`, so there is nothing to
    /// choose the date mode of.
    MaskDatesWithoutKey {
        /// The mode given.
        dates: String,
    },
    /// `table_name` names no table of the spec.
    UnknownTable {
        /// The name given.
        table: String,
        /// The tables the spec projects.
        known: Vec<String>,
    },
    /// `version` names no built-in spec.
    UnknownVersion {
        /// The core's account of the version given and the versions there are.
        source: oxedi_core::UnknownVersion,
    },
    /// A glob pattern was given while the caller's `enable_external_access`
    /// is off, so it may not be expanded.
    PatternWithoutExternalAccess {
        /// The pattern as given.
        pattern: String,
    },
    /// A glob pattern matched no file.
    NoFiles {
        /// The pattern as given.
        pattern: String,
    },
    /// DuckDB could not expand a glob pattern.
    Glob {
        /// The pattern as given.
        pattern: String,
        /// DuckDB's message.
        message: String,
    },
    /// DuckDB's file system could not open a file.
    Open {
        /// The path as given or expanded.
        file: String,
        /// DuckDB's message.
        message: String,
    },
    /// DuckDB's file system failed while reading a file.
    Read {
        /// The path as given or expanded.
        file: String,
        /// DuckDB's message.
        message: String,
    },
    /// The file is not an X12 interchange the core can index.
    Parse {
        /// The path as given or expanded.
        file: String,
        /// The core's error.
        source: DocumentError,
    },
    /// A text cell is not valid UTF-8, so it cannot be a VARCHAR.
    InvalidUtf8 {
        /// The path as given or expanded.
        file: String,
        /// The table read.
        table: String,
        /// The column of the cell.
        column: String,
        /// The value of the table's `row` column for the cell's row, when
        /// the table has one.
        row: Option<i64>,
        /// The 0-based position of the cell's row in the file's table.
        index: usize,
        /// The cell's bytes.
        bytes: Vec<u8>,
    },
    /// The spec a file selected projects the table with other columns than
    /// the ones bound.
    SchemaMismatch {
        /// The path as given or expanded.
        file: String,
        /// The table read.
        table: String,
        /// The version of the spec the file selected.
        version: &'static str,
        /// The version of the spec the columns were bound with.
        bound: &'static str,
    },
    /// A column type of the core has no DuckDB type here.
    UnsupportedType {
        /// The table read.
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

/// Quotes each name and joins them with commas.
fn quoted<S: AsRef<str>>(names: &[S]) -> String {
    names
        .iter()
        .map(|name| format!("{:?}", name.as_ref()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// DuckDB's reserved keywords (`duckdb_keywords()` with category `reserved`),
/// which cannot be written as a bare identifier.
const RESERVED_KEYWORDS: &[&str] = &[
    "all",
    "analyse",
    "analyze",
    "and",
    "any",
    "array",
    "as",
    "asc",
    "asymmetric",
    "both",
    "case",
    "cast",
    "check",
    "collate",
    "column",
    "constraint",
    "create",
    "default",
    "deferrable",
    "desc",
    "describe",
    "distinct",
    "do",
    "else",
    "end",
    "except",
    "false",
    "fetch",
    "for",
    "foreign",
    "from",
    "group",
    "having",
    "in",
    "initially",
    "intersect",
    "into",
    "lambda",
    "lateral",
    "leading",
    "limit",
    "not",
    "null",
    "offset",
    "on",
    "only",
    "or",
    "order",
    "pivot",
    "pivot_longer",
    "pivot_wider",
    "placing",
    "primary",
    "qualify",
    "references",
    "returning",
    "select",
    "show",
    "some",
    "summarize",
    "symmetric",
    "table",
    "then",
    "to",
    "trailing",
    "true",
    "union",
    "unique",
    "unpivot",
    "using",
    "variadic",
    "when",
    "where",
    "window",
    "with",
];

/// The SQL spelling of an identifier: as is when it is a plain lowercase name
/// that is not a reserved keyword, otherwise in double quotes with inner
/// double quotes doubled.
pub(crate) fn identifier(name: &str) -> String {
    let mut chars = name.chars();
    let plain = chars
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first == '_')
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if plain && !RESERVED_KEYWORDS.contains(&name) {
        name.to_owned()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

/// How an error about a forwarded macro parameter names it: what the macro
/// passed, and what to write instead of `:=`. Without a name the hint says
/// to write the parameter itself.
pub(crate) fn macro_hint(parameter: Option<&str>, value: &str) -> (String, String) {
    match parameter {
        Some(name) => (
            format!("its parameter {name:?} as {value:?}"),
            identifier(name),
        ),
        None => (
            format!("a parameter as {value:?}"),
            "<the parameter>".to_owned(),
        ),
    }
}

impl From<oxedi_core::UnknownVersion> for ReadError {
    fn from(source: oxedi_core::UnknownVersion) -> ReadError {
        ReadError::UnknownVersion { source }
    }
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::PathType { found } => write!(
                f,
                "{FUNCTION}: the path must be a VARCHAR or a list of VARCHAR; found {found}"
            ),
            ReadError::NullPath { position: None } => {
                write!(f, "{FUNCTION}: the path must not be NULL")
            }
            ReadError::NullPath {
                position: Some(position),
            } => write!(
                f,
                "{FUNCTION}: every path of the list must be a string; path {position} is NULL"
            ),
            ReadError::EmptyPathList => {
                write!(f, "{FUNCTION}: the list of paths must not be empty")
            }
            ReadError::NullOption { name: "mask_key" } => write!(
                f,
                "{FUNCTION}: mask_key must not be NULL; leave it out to read without masking, \
                 or give a key of at least 16 bytes (a NULL often means a variable is not set)"
            ),
            ReadError::NullOption { name } => write!(
                f,
                "{FUNCTION}: {name} must not be NULL; leave it out to use its default"
            ),
            ReadError::MacroParameter {
                name,
                parameter,
                value,
            } => {
                let (passed, written) = macro_hint(parameter.as_deref(), value);
                write!(
                    f,
                    "{FUNCTION}: {name} must be a constant string; a table macro passed \
                     {passed}; DuckDB does not substitute a macro parameter given with := or \
                     =>, so write {name} = {written}"
                )
            }
            ReadError::Mask { name, source } => write!(f, "{FUNCTION}: {name}: {source}"),
            ReadError::MaskDatesWithoutKey { dates } => write!(
                f,
                "{FUNCTION}: mask_dates {dates:?} was given without mask_key; mask_dates only \
                 chooses how a mask treats dates, so give mask_key as well"
            ),
            ReadError::UnknownTable { table, known } => write!(
                f,
                "{FUNCTION}: unknown table {table:?}; table_name must be one of {}",
                quoted(known)
            ),
            ReadError::UnknownVersion { source } => write!(f, "{FUNCTION}: {source}"),
            ReadError::PatternWithoutExternalAccess { pattern } => write!(
                f,
                "{FUNCTION}: the pattern {pattern:?} cannot be expanded while \
                 enable_external_access is false; list the files instead"
            ),
            ReadError::NoFiles { pattern } => {
                write!(f, "{FUNCTION}: no file matches the pattern {pattern:?}")
            }
            ReadError::Glob { pattern, message } => write!(
                f,
                "{FUNCTION}: the pattern {pattern:?} could not be expanded: {message}"
            ),
            ReadError::Open { file, message } => {
                write!(f, "{FUNCTION}: {file:?} could not be opened: {message}")
            }
            ReadError::Read { file, message } => {
                write!(f, "{FUNCTION}: {file:?} could not be read: {message}")
            }
            ReadError::Parse { file, source } => {
                write!(
                    f,
                    "{FUNCTION}: {file:?} is not an X12 interchange: {source}"
                )
            }
            ReadError::InvalidUtf8 {
                file,
                table,
                column,
                row,
                index,
                bytes,
            } => write!(
                f,
                "{FUNCTION}: {file:?}, table {table:?}, column {column:?}, {}: \
                 a VARCHAR must be valid UTF-8; found b\"{}\"; \
                 pass binary := true to read text columns as BLOB",
                match row {
                    Some(row) => format!("row {row}"),
                    None => format!("row index {index}"),
                },
                bytes.escape_ascii()
            ),
            ReadError::SchemaMismatch {
                file,
                table,
                version,
                bound,
            } => write!(
                f,
                "{FUNCTION}: {file:?} declares version {version}, whose table {table:?} has \
                 other columns than version {bound}, which the query was bound with; \
                 pass version := '{version}' to read such files on their own"
            ),
            ReadError::UnsupportedType {
                table,
                column,
                kind,
            } => write!(
                f,
                "{FUNCTION}: table {table:?}, column {column:?}: the core type {kind} has no DuckDB type"
            ),
            ReadError::Internal { message } => {
                write!(f, "{FUNCTION}: internal error: {message}")
            }
        }
    }
}

impl std::error::Error for ReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // `Parse`, `Mask` and `UnknownVersion` print the core error in
        // full, so the chain stops here instead of printing it a second time.
        match self {
            ReadError::PathType { .. }
            | ReadError::NullPath { .. }
            | ReadError::EmptyPathList
            | ReadError::NullOption { .. }
            | ReadError::MacroParameter { .. }
            | ReadError::Mask { .. }
            | ReadError::MaskDatesWithoutKey { .. }
            | ReadError::UnknownTable { .. }
            | ReadError::UnknownVersion { .. }
            | ReadError::PatternWithoutExternalAccess { .. }
            | ReadError::NoFiles { .. }
            | ReadError::Glob { .. }
            | ReadError::Open { .. }
            | ReadError::Read { .. }
            | ReadError::Parse { .. }
            | ReadError::InvalidUtf8 { .. }
            | ReadError::SchemaMismatch { .. }
            | ReadError::UnsupportedType { .. }
            | ReadError::Internal { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests;
