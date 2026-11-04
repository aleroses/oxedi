//! The value types (the element type a spec declares and the column type it
//! becomes), the parsers that read X12 element text into column values, the
//! rules that turn a caller's values into the cells of a column, and the
//! calendar they share with the readers.
//!
//! The folder holds: `parse.rs` (the X12 element text as column values
//! without allocating: [`parse_n`], [`parse_r`], [`parse_dt`] and
//! [`parse_tm`]), `parsed.rs` (a text as a value of a column type,
//! [`Parsed`], through [`ColumnType::parse`]), `convert.rs` (integers, decimals, dates and times that
//! either keep their value on the way to a column type or say why not,
//! [`ConvertError`]), `unit.rs` (the units a time is counted in), `calendar.rs` (days since 1970-01-01 and seconds of the
//! day, checked), `format.rs` (decimals, dates and times as text, in the
//! display form tables and messages show and the element form an X12 element
//! holds, [`ElementTextError`]), `length.rs` (the lengths an element allows,
//! [`LengthRange`]), `column_type.rs` (the physical type of a column), `element_type.rs` (the element type, its code and its
//! classification, [`TypeCodeError`]) and `tests/`.

mod calendar;
mod column_type;
mod convert;
mod element_type;
mod format;
mod length;
mod parse;
mod parsed;
mod unit;

pub(crate) use calendar::{civil_from_days, year_start};
pub use calendar::{days_from_civil, seconds_of_day, time_of_day};
pub use column_type::{ColumnType, DECIMAL_PRECISION};
pub use convert::{
    ConvertError, MAX_SCALE, date_from_midnight, date_from_millis, integer_from_f64,
    integer_from_i128, integer_from_u128, rescale, seconds_from,
};
pub use element_type::{ElementType, TypeCodeError};
pub use format::{
    ElementTextError, date_display, date_element, decimal_display, decimal_element, time_display,
    time_element,
};
pub use length::LengthRange;
pub(crate) use parse::parse_unsigned;
pub use parse::{parse_dt, parse_n, parse_r, parse_tm};
pub use parsed::Parsed;
pub use unit::TimeUnit;

#[cfg(test)]
mod tests;
