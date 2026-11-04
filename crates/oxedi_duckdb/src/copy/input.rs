//! The query's one column: a STRUCT with one field per table of the spec,
//! each a `LIST(STRUCT(...))` of that table's rows.
//!
//! The C API of a copy function gives the columns' types but not their
//! names; the names of a STRUCT's fields travel in its type, so the tables
//! are named there. Each field of a table's rows must be a column of that
//! table, with a DuckDB type its values convert from.

use libduckdb_sys as ffi;
use oxedi_core::ColumnType;
use oxedi_core::value::TimeUnit;
use oxedi_core::write::WriteError;

use super::error::CopyError;
use crate::builtins::{BuiltinSchema, BuiltinTable};
use crate::types::LogicalType;
use crate::value::{primitive_name, take_text};

/// The DuckDB type of a struct field, as far as conversion cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    /// `NULL`: every value is null.
    Null,
    /// `VARCHAR`.
    Varchar,
    /// `BLOB`.
    Blob,
    /// An `ENUM`, its index stored as this unsigned integer; the field
    /// carries the dictionary.
    Enum(EnumIndex),
    /// An integer type.
    Integer(Integer),
    /// `FLOAT`.
    Float,
    /// `DOUBLE`.
    Double,
    /// `DECIMAL(width, scale)`.
    Decimal {
        /// Total digits.
        width: u8,
        /// Decimal places.
        scale: u8,
    },
    /// `DATE`.
    Date,
    /// `TIME`, microseconds.
    Time,
    /// `TIME_NS`, nanoseconds.
    TimeNs,
    /// A `TIMESTAMP` without a time zone, in the unit given.
    Timestamp(TimeUnit),
    /// Any other type.
    Other,
}

/// The width of an `ENUM`'s index, always unsigned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnumIndex {
    /// `UTINYINT`.
    U8,
    /// `USMALLINT`.
    U16,
    /// `UINTEGER`.
    U32,
}

/// The integer types, by how DuckDB stores them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integer {
    I8,
    I16,
    I32,
    I64,
    I128,
    U8,
    U16,
    U32,
    U64,
    U128,
}

impl FieldType {
    /// Whether a column of the spec's `kind` takes values of this type.
    fn converts_to(self, kind: ColumnType) -> bool {
        matches!(
            (self, kind),
            (FieldType::Null, _)
                | (
                    FieldType::Varchar | FieldType::Blob | FieldType::Enum(_),
                    ColumnType::Binary
                )
                | (
                    FieldType::Integer(_) | FieldType::Float | FieldType::Double,
                    ColumnType::Int64 { .. }
                )
                | (
                    FieldType::Integer(_) | FieldType::Decimal { .. },
                    ColumnType::Decimal128 { .. }
                )
                | (
                    FieldType::Date | FieldType::Timestamp(_),
                    ColumnType::Date32
                )
                | (FieldType::Time | FieldType::TimeNs, ColumnType::Time32)
        )
    }
}

/// One field of an input column's structs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputField {
    /// The field's name.
    pub name: String,
    /// Its type.
    pub kind: FieldType,
    /// Its DuckDB type as SQL writes it, for messages.
    pub sql: String,
    /// The values of an `ENUM`, by index; empty for any other type.
    pub dictionary: Vec<Vec<u8>>,
}

/// One table of the input as its type describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputTable {
    /// A list of structs with these fields.
    Rows(Vec<InputField>),
    /// Any other type, as SQL writes it.
    Other(String),
}

/// The query's column as its type describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputColumn {
    /// A STRUCT: each field's name and type.
    Tables(Vec<(String, InputTable)>),
    /// Any other type, as SQL writes it.
    Other(String),
}

/// A field bound to its table's column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundField {
    /// The field's name, the column's.
    pub name: String,
    /// The spec's type of the column.
    pub kind: ColumnType,
    /// The field's DuckDB type.
    pub input: FieldType,
    /// The values of an `ENUM`, by index; empty for any other type.
    pub dictionary: Vec<Vec<u8>>,
}

/// One table of the input bound to the spec's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundTable {
    /// The table.
    pub table: String,
    /// Each field of the table's structs, in the structs' order.
    pub fields: Vec<BoundField>,
}

/// Binds the query's columns, which must be one STRUCT keyed by table, to
/// the tables of `builtin`; one bound table per field of the STRUCT, in its
/// order.
pub fn bind(
    builtin: &BuiltinSchema,
    columns: &[InputColumn],
) -> Result<Vec<BoundTable>, CopyError> {
    let fields = match columns {
        [InputColumn::Tables(fields)] => fields,
        [InputColumn::Other(found)] => {
            return Err(CopyError::NotStruct {
                found: found.clone(),
            });
        }
        _ => {
            return Err(CopyError::ColumnCount {
                found: columns.len(),
            });
        }
    };
    fields
        .iter()
        .map(|(name, input)| {
            let table = builtin.table(name).ok_or_else(|| {
                CopyError::Write(WriteError::UnknownTable {
                    table: name.clone(),
                    tables: builtin.table_names(),
                })
            })?;
            let fields = match input {
                InputTable::Rows(fields) => fields,
                InputTable::Other(found) => {
                    return Err(CopyError::NotRows {
                        table: name.clone(),
                        found: found.clone(),
                    });
                }
            };
            let fields = fields
                .iter()
                .map(|field| bind_field(table, field))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(BoundTable {
                table: table.name.clone(),
                fields,
            })
        })
        .collect()
}

fn bind_field(table: &BuiltinTable, field: &InputField) -> Result<BoundField, CopyError> {
    let kind = table
        .columns
        .iter()
        .find(|(column, _)| *column == field.name)
        .map(|(_, kind)| *kind)
        .ok_or_else(|| {
            CopyError::Write(WriteError::UnknownColumn {
                table: table.name.clone(),
                column: field.name.clone(),
                columns: table.columns.iter().map(|(name, _)| name.clone()).collect(),
            })
        })?;
    if !field.kind.converts_to(kind) {
        let float = matches!(field.kind, FieldType::Float | FieldType::Double);
        let (table, field_name, found) =
            (table.name.clone(), field.name.clone(), field.sql.clone());
        return Err(if float && matches!(kind, ColumnType::Decimal128 { .. }) {
            CopyError::FloatForDecimal {
                table,
                field: field_name,
                found,
                expected: kind,
            }
        } else {
            CopyError::FieldType {
                table,
                field: field_name,
                found,
                expected: kind,
            }
        });
    }
    Ok(BoundField {
        name: field.name.clone(),
        kind,
        input: field.kind,
        dictionary: field.dictionary.clone(),
    })
}

/// What messages call a type DuckDB did not describe.
const UNDESCRIBED: &str = "a type DuckDB did not describe";

/// The query's column of a logical type.
///
/// # Safety
///
/// `logical_type` must be a live logical type handle.
pub unsafe fn column_of(logical_type: &LogicalType) -> InputColumn {
    let raw = logical_type.raw();
    // SAFETY: the type is live (caller contract).
    if unsafe { ffi::duckdb_get_type_id(raw) } != ffi::DUCKDB_TYPE_DUCKDB_TYPE_STRUCT {
        // SAFETY: as above.
        return InputColumn::Other(unsafe { sql_name(raw) });
    }
    // SAFETY: the type is a live STRUCT.
    let tables = unsafe { children(raw) }
        .into_iter()
        .map(|(name, child)| {
            let table = match child {
                // SAFETY: `child` is live.
                Some(child) => unsafe { table_of(child.raw()) },
                None => InputTable::Other(UNDESCRIBED.to_owned()),
            };
            (name, table)
        })
        .collect();
    InputColumn::Tables(tables)
}

/// The name and type of each field of a STRUCT type.
///
/// # Safety
///
/// `logical_type` must be a live STRUCT type.
unsafe fn children(logical_type: ffi::duckdb_logical_type) -> Vec<(String, Option<LogicalType>)> {
    // SAFETY: the type is a live STRUCT (caller contract).
    let count = unsafe { ffi::duckdb_struct_type_child_count(logical_type) };
    (0..count)
        // SAFETY: `index` is below the child count; the name and type
        // returned are owned here and released after use.
        .map(|index| unsafe {
            (
                take_text(ffi::duckdb_struct_type_child_name(logical_type, index)),
                LogicalType::owned(ffi::duckdb_struct_type_child_type(logical_type, index)),
            )
        })
        .collect()
}

/// The child type of a LIST type.
///
/// # Safety
///
/// `logical_type` must be a live LIST type.
unsafe fn list_child(logical_type: ffi::duckdb_logical_type) -> Option<LogicalType> {
    // SAFETY: the type is a live LIST (caller contract); its child type is
    // owned by the result.
    unsafe { LogicalType::owned(ffi::duckdb_list_type_child_type(logical_type)) }
}

/// One table of the input: a LIST of STRUCT, else its SQL type.
///
/// # Safety
///
/// `logical_type` must be a live logical type handle.
unsafe fn table_of(logical_type: ffi::duckdb_logical_type) -> InputTable {
    // SAFETY: the type is live (caller contract).
    let id = unsafe { ffi::duckdb_get_type_id(logical_type) };
    if id == ffi::DUCKDB_TYPE_DUCKDB_TYPE_LIST
        // SAFETY: the type is a live LIST.
        && let Some(child) = unsafe { list_child(logical_type) }
        // SAFETY: `child` is live.
        && unsafe { ffi::duckdb_get_type_id(child.raw()) } == ffi::DUCKDB_TYPE_DUCKDB_TYPE_STRUCT
    {
        // SAFETY: `child` is a live STRUCT type.
        return InputTable::Rows(unsafe { struct_fields(child.raw()) });
    }
    // SAFETY: the type is live (caller contract).
    InputTable::Other(unsafe { sql_name(logical_type) })
}

/// The fields of a STRUCT type.
///
/// # Safety
///
/// `logical_type` must be a live STRUCT type.
unsafe fn struct_fields(logical_type: ffi::duckdb_logical_type) -> Vec<InputField> {
    // SAFETY: the type is a live STRUCT (caller contract).
    unsafe { children(logical_type) }
        .into_iter()
        .map(|(name, child)| match child {
            // SAFETY: `child` is live.
            Some(child) => unsafe {
                InputField {
                    name,
                    kind: field_type(child.raw()),
                    sql: sql_name(child.raw()),
                    dictionary: dictionary(child.raw()),
                }
            },
            None => InputField {
                name,
                kind: FieldType::Other,
                sql: UNDESCRIBED.to_owned(),
                dictionary: Vec::new(),
            },
        })
        .collect()
}

/// The values of an ENUM type, by index; empty for any other type.
///
/// # Safety
///
/// `logical_type` must be a live logical type handle.
unsafe fn dictionary(logical_type: ffi::duckdb_logical_type) -> Vec<Vec<u8>> {
    // SAFETY: the type is live (caller contract).
    if unsafe { ffi::duckdb_get_type_id(logical_type) } != ffi::DUCKDB_TYPE_DUCKDB_TYPE_ENUM {
        return Vec::new();
    }
    // SAFETY: the type is a live ENUM.
    let size = unsafe { ffi::duckdb_enum_dictionary_size(logical_type) };
    (0..u64::from(size))
        // SAFETY: `index` is below the dictionary size; the string returned
        // is owned here and freed by `take_text`.
        .map(|index| unsafe {
            take_text(ffi::duckdb_enum_dictionary_value(logical_type, index)).into_bytes()
        })
        .collect()
}

/// An `ENUM` field by the width of its index.
///
/// # Safety
///
/// `logical_type` must be a live `ENUM` type.
unsafe fn enum_type(logical_type: ffi::duckdb_logical_type) -> FieldType {
    // SAFETY: the type is a live ENUM (caller contract).
    match unsafe { ffi::duckdb_enum_internal_type(logical_type) } {
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_UTINYINT => FieldType::Enum(EnumIndex::U8),
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_USMALLINT => FieldType::Enum(EnumIndex::U16),
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_UINTEGER => FieldType::Enum(EnumIndex::U32),
        _ => FieldType::Other,
    }
}

/// The field type of a logical type.
///
/// # Safety
///
/// `logical_type` must be a live logical type handle.
unsafe fn field_type(logical_type: ffi::duckdb_logical_type) -> FieldType {
    // SAFETY: the type is live (caller contract).
    let id = unsafe { ffi::duckdb_get_type_id(logical_type) };
    match id {
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_SQLNULL => FieldType::Null,
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_VARCHAR => FieldType::Varchar,
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_BLOB => FieldType::Blob,
        // SAFETY: the type is a live ENUM.
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_ENUM => unsafe { enum_type(logical_type) },
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_TINYINT => FieldType::Integer(Integer::I8),
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_SMALLINT => FieldType::Integer(Integer::I16),
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_INTEGER => FieldType::Integer(Integer::I32),
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_BIGINT => FieldType::Integer(Integer::I64),
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_HUGEINT => FieldType::Integer(Integer::I128),
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_UTINYINT => FieldType::Integer(Integer::U8),
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_USMALLINT => FieldType::Integer(Integer::U16),
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_UINTEGER => FieldType::Integer(Integer::U32),
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_UBIGINT => FieldType::Integer(Integer::U64),
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_UHUGEINT => FieldType::Integer(Integer::U128),
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_FLOAT => FieldType::Float,
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_DOUBLE => FieldType::Double,
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_DECIMAL => FieldType::Decimal {
            // SAFETY: the type is a live DECIMAL.
            width: unsafe { ffi::duckdb_decimal_width(logical_type) },
            // SAFETY: as above.
            scale: unsafe { ffi::duckdb_decimal_scale(logical_type) },
        },
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_DATE => FieldType::Date,
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_TIME => FieldType::Time,
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_TIME_NS => FieldType::TimeNs,
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_TIMESTAMP_S => FieldType::Timestamp(TimeUnit::Second),
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_TIMESTAMP_MS => FieldType::Timestamp(TimeUnit::Millisecond),
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_TIMESTAMP => FieldType::Timestamp(TimeUnit::Microsecond),
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_TIMESTAMP_NS => FieldType::Timestamp(TimeUnit::Nanosecond),
        _ => FieldType::Other,
    }
}

/// A logical type as SQL writes it: `DECIMAL(18,3)`, `INTEGER[]`,
/// `STRUCT(a INTEGER)`, else its primitive name.
///
/// # Safety
///
/// `logical_type` must be a live logical type handle.
unsafe fn sql_name(logical_type: ffi::duckdb_logical_type) -> String {
    // SAFETY: the type is live (caller contract).
    let id = unsafe { ffi::duckdb_get_type_id(logical_type) };
    match id {
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_DECIMAL => {
            // SAFETY: the type is a live DECIMAL.
            let (width, scale) = unsafe {
                (
                    ffi::duckdb_decimal_width(logical_type),
                    ffi::duckdb_decimal_scale(logical_type),
                )
            };
            format!("DECIMAL({width},{scale})")
        }
        // SAFETY: the type is a live LIST.
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_LIST => match unsafe { list_child(logical_type) } {
            // SAFETY: `child` is live.
            Some(child) => format!("{}[]", unsafe { sql_name(child.raw()) }),
            None => UNDESCRIBED.to_owned(),
        },
        ffi::DUCKDB_TYPE_DUCKDB_TYPE_STRUCT => {
            // SAFETY: the type is a live STRUCT.
            let fields = unsafe { struct_fields(logical_type) };
            let parts: Vec<String> = fields
                .iter()
                .map(|field| format!("{} {}", field.name, field.sql))
                .collect();
            format!("STRUCT({})", parts.join(", "))
        }
        _ => primitive_name(id).to_owned(),
    }
}
