//! The options of a `COPY` read from DuckDB's STRUCT value into
//! [`OptionValue`]s.

use libduckdb_sys as ffi;
use oxedi_core::value::TimeUnit;

use super::convert::from_hugeint;
use super::options::{Kind, OptionValue};
use crate::value::{Value, take_text};

/// The options of the bind in progress, each name lower case, in the order
/// DuckDB gives them.
///
/// # Safety
///
/// `info` must be the bind info DuckDB passed to the running copy bind
/// callback.
pub unsafe fn options(info: ffi::duckdb_copy_function_bind_info) -> Vec<(String, OptionValue)> {
    // SAFETY: `info` is live (caller contract); the value returned is owned
    // here. DuckDB returns a NULL value when no option was given.
    let Some(all) = (unsafe { Value::owned(ffi::duckdb_copy_function_bind_get_options(info)) })
    else {
        return Vec::new();
    };
    // SAFETY: `all` is a live value.
    match unsafe { struct_children(&all) } {
        Some(children) => children
            .into_iter()
            .map(|(name, value)| (name.to_lowercase(), value))
            .collect(),
        None => Vec::new(),
    }
}

/// The named children of a STRUCT value; `None` for another type.
///
/// # Safety
///
/// `value` must be live.
unsafe fn struct_children(value: &Value) -> Option<Vec<(String, OptionValue)>> {
    if value.type_id() != ffi::DUCKDB_TYPE_DUCKDB_TYPE_STRUCT {
        return None;
    }
    // SAFETY: the value is live; its type is borrowed from it, not
    // destroyed here.
    let kind = unsafe { ffi::duckdb_get_value_type(value.raw()) };
    // SAFETY: `kind` is the live STRUCT type of the value.
    let count = unsafe { ffi::duckdb_struct_type_child_count(kind) };
    let children = (0..count)
        .map(|index| {
            // SAFETY: `index` is below the child count; the name is owned
            // here and freed after it is copied, and the child value is
            // owned and taken by `Value::owned`.
            let (name, child) = unsafe {
                (
                    take_text(ffi::duckdb_struct_type_child_name(kind, index)),
                    Value::owned(ffi::duckdb_get_struct_child(value.raw(), index)),
                )
            };
            let child = match child {
                // SAFETY: `child` is live.
                Some(child) => unsafe { option_value(&child) },
                None => OptionValue {
                    kind: Kind::Null,
                    shown: "NULL".to_owned(),
                },
            };
            (name, child)
        })
        .collect();
    Some(children)
}

/// A value as the messages show it: a VARCHAR quoted, any other type as its
/// type name and text.
fn shown(value: &Value, text: &str) -> String {
    if value.type_id() == ffi::DUCKDB_TYPE_DUCKDB_TYPE_VARCHAR {
        format!("VARCHAR {text:?}")
    } else {
        format!("{} {text}", value.type_name())
    }
}

/// The bytes of a BLOB value, copied out and DuckDB's copy freed; `None`
/// for a size this platform cannot address.
///
/// # Safety
///
/// `raw` must be a live BLOB value.
unsafe fn blob_bytes(raw: ffi::duckdb_value) -> Option<Vec<u8>> {
    // SAFETY: caller contract; the blob's data holds `size` bytes and is
    // freed once, after the copy.
    unsafe {
        let blob = ffi::duckdb_get_blob(raw);
        if blob.data.is_null() {
            return Some(Vec::new());
        }
        let bytes = usize::try_from(blob.size)
            .ok()
            .map(|len| std::slice::from_raw_parts(blob.data.cast::<u8>(), len).to_vec());
        ffi::duckdb_free(blob.data);
        bytes
    }
}

/// One value as an [`OptionValue`].
///
/// # Safety
///
/// `value` must be live.
unsafe fn option_value(value: &Value) -> OptionValue {
    let id = value.type_id();
    let raw = value.raw();
    let text = value.text();
    let shown = shown(value, &text);
    // SAFETY: each getter is called on a live value of its own type.
    let kind = unsafe {
        match id {
            ffi::DUCKDB_TYPE_DUCKDB_TYPE_VARCHAR => Kind::Text(text),
            ffi::DUCKDB_TYPE_DUCKDB_TYPE_BOOLEAN => Kind::Bool(ffi::duckdb_get_bool(raw)),
            ffi::DUCKDB_TYPE_DUCKDB_TYPE_BLOB => blob_bytes(raw).map_or(Kind::Other, Kind::Bytes),
            ffi::DUCKDB_TYPE_DUCKDB_TYPE_TINYINT
            | ffi::DUCKDB_TYPE_DUCKDB_TYPE_SMALLINT
            | ffi::DUCKDB_TYPE_DUCKDB_TYPE_INTEGER
            | ffi::DUCKDB_TYPE_DUCKDB_TYPE_BIGINT
            | ffi::DUCKDB_TYPE_DUCKDB_TYPE_UTINYINT
            | ffi::DUCKDB_TYPE_DUCKDB_TYPE_USMALLINT
            | ffi::DUCKDB_TYPE_DUCKDB_TYPE_UINTEGER => {
                Kind::Integer(i128::from(ffi::duckdb_get_int64(raw)))
            }
            ffi::DUCKDB_TYPE_DUCKDB_TYPE_UBIGINT => {
                Kind::Integer(i128::from(ffi::duckdb_get_uint64(raw)))
            }
            ffi::DUCKDB_TYPE_DUCKDB_TYPE_HUGEINT => {
                Kind::Integer(from_hugeint(ffi::duckdb_get_hugeint(raw)))
            }
            ffi::DUCKDB_TYPE_DUCKDB_TYPE_DATE => Kind::Date(ffi::duckdb_get_date(raw).days),
            ffi::DUCKDB_TYPE_DUCKDB_TYPE_TIME => Kind::Time {
                value: ffi::duckdb_get_time(raw).micros,
                unit: TimeUnit::Microsecond,
            },
            ffi::DUCKDB_TYPE_DUCKDB_TYPE_TIME_NS => Kind::Time {
                value: ffi::duckdb_get_time_ns(raw).nanos,
                unit: TimeUnit::Nanosecond,
            },
            ffi::DUCKDB_TYPE_DUCKDB_TYPE_STRUCT => {
                struct_children(value).map_or(Kind::Other, Kind::Struct)
            }
            _ => Kind::Other,
        }
    };
    OptionValue { kind, shown }
}
