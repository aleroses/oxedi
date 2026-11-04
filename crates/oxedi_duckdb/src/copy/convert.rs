//! DuckDB values to the spec's column types, by the core's rules
//! (`oxedi_core::value`): integers (and whole floats) to integers, decimals
//! rescaled exactly, dates from dates or midnight timestamps, times from
//! whole seconds. A value that would change on the way is refused with the
//! reason.

use oxedi_core::value;

use libduckdb_sys as ffi;

/// A DuckDB `hugeint` as `i128`.
pub fn from_hugeint(value: ffi::duckdb_hugeint) -> i128 {
    (i128::from(value.upper) << 64) | i128::from(value.lower)
}

/// A DuckDB `uhugeint` as `u128`.
pub fn from_uhugeint(value: ffi::duckdb_uhugeint) -> u128 {
    (u128::from(value.upper) << 64) | u128::from(value.lower)
}

/// An integer as `i64`, or why it does not fit.
pub fn wide_integer(value: i128) -> Result<i64, String> {
    value::integer_from_i128(value).map_err(|error| error.to_string())
}

/// An unsigned 128-bit integer as `i64`, or why it does not fit.
pub fn unsigned_integer(value: u128) -> Result<i64, String> {
    value::integer_from_u128(value).map_err(|error| error.to_string())
}

/// A float with no fraction as `i64`, or why it is refused.
pub fn float_integer(value: f64) -> Result<i64, String> {
    value::integer_from_f64(value).map_err(|error| error.to_string())
}

/// `value` at scale `from` rescaled to `scale`, or why it does not rescale
/// exactly.
pub fn rescale(value: i128, from: u8, scale: u8) -> Result<i128, String> {
    value::rescale(value, i32::from(from), scale).map_err(|error| error.to_string())
}

/// Days since 1970-01-01 of a timestamp at exactly midnight.
pub fn timestamp_date(value: i64, unit: value::TimeUnit) -> Result<i32, String> {
    value::date_from_midnight(value, unit).map_err(|error| error.to_string())
}

/// Seconds since midnight of a time counted in `unit`.
pub fn whole_seconds(value: i64, unit: value::TimeUnit) -> Result<i32, String> {
    value::seconds_from(value, unit).map_err(|error| error.to_string())
}
