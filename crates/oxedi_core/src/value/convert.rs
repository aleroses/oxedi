//! Conversions from the numbers a caller holds to the values of a column
//! type. Each one either keeps the value exactly or says why it cannot.

use super::TimeUnit;
use super::calendar::DAY_SECONDS;
use std::fmt;

/// The largest decimal scale a spec's `R` element takes;
/// `ElementType::MAX_SCALE` is this value. An `R` value is held as an `i128`
/// scaled by `10^scale` in a column of precision 38, so a scale of 18 still
/// leaves 20 digits for the integer part.
pub const MAX_SCALE: u8 = 18;

/// Milliseconds in a day.
const DAY_MILLIS: i64 = DAY_SECONDS * 1000;

/// Why a value cannot become a cell of its column type without changing.
#[derive(Debug, Clone, PartialEq)]
pub enum ConvertError {
    /// A signed integer outside the 64-bit range.
    TooWide { value: i128 },
    /// An unsigned integer outside the 64-bit range.
    TooWideUnsigned { value: u128 },
    /// A float that is not finite, has a fraction or is beyond the 64-bit range.
    NotWhole { value: f64 },
    /// A decimal scaled up from `from` to `scale` that no longer fits 128 bits.
    ScaleOverflow { value: i128, from: i32, scale: u8 },
    /// A decimal scaled down from `from` to `scale` that would lose digits.
    TooManyDecimals { value: i128, from: i32, scale: u8 },
    /// A timestamp that is not at midnight.
    NotMidnight { value: i64 },
    /// A millisecond date that is not a whole number of days.
    NotWholeDay { millis: i64 },
    /// A millisecond date whose day count does not fit 32 bits.
    DayOutOfRange { millis: i64 },
    /// A count of days or seconds that does not fit 32 bits.
    OutOfRange { value: i64 },
    /// A time that is not a whole number of seconds.
    NotWholeSecond { value: i64 },
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooWide { value } => write!(f, "{value} does not fit a 64-bit integer"),
            Self::TooWideUnsigned { value } => write!(f, "{value} does not fit a 64-bit integer"),
            Self::NotWhole { value } => write!(f, "{value} is not a whole number"),
            Self::ScaleOverflow { value, from, scale } => {
                write!(f, "{value} at scale {from} overflows scale {scale}")
            }
            Self::TooManyDecimals { value, from, scale } => write!(
                f,
                "{value} at scale {from} has more decimals than the column's scale {scale}"
            ),
            Self::NotMidnight { value } => {
                write!(f, "timestamp {value} is not at midnight; give dates")
            }
            Self::NotWholeDay { millis } => write!(f, "{millis} ms is not a whole day"),
            Self::DayOutOfRange { millis } => write!(f, "{millis} ms is out of range"),
            Self::OutOfRange { value } => write!(f, "{value} is out of range"),
            Self::NotWholeSecond { value } => write!(f, "{value} is not a whole second"),
        }
    }
}

impl std::error::Error for ConvertError {}

/// A signed integer as `i64`, or why it does not fit.
pub fn integer_from_i128(value: i128) -> Result<i64, ConvertError> {
    i64::try_from(value).map_err(|_| ConvertError::TooWide { value })
}

/// An unsigned integer as `i64`, or why it does not fit.
pub fn integer_from_u128(value: u128) -> Result<i64, ConvertError> {
    i64::try_from(value).map_err(|_| ConvertError::TooWideUnsigned { value })
}

/// A finite float with no fraction, below 9.2e18 in magnitude, as `i64`.
pub fn integer_from_f64(value: f64) -> Result<i64, ConvertError> {
    let whole = value.is_finite() && value.fract() == 0.0 && value.abs() < 9.2e18;
    if whole {
        Ok(value as i64)
    } else {
        Err(ConvertError::NotWhole { value })
    }
}

/// `value` at scale `from` (decimal places, negative for powers of ten)
/// rescaled to `scale`, or why it does not rescale exactly. Zero rescales to
/// zero at any distance, up or down.
pub fn rescale(value: i128, from: i32, scale: u8) -> Result<i128, ConvertError> {
    let shift = i32::from(scale).saturating_sub(from);
    let factor = 10i128.checked_pow(shift.unsigned_abs());
    if shift >= 0 {
        scaled_up(value, factor).ok_or(ConvertError::ScaleOverflow { value, from, scale })
    } else {
        scaled_down(value, factor).ok_or(ConvertError::TooManyDecimals { value, from, scale })
    }
}

/// Zero, which keeps its value at any scale and needs no factor.
fn zero(value: i128) -> Option<i128> {
    (value == 0).then_some(0)
}

/// `value` times `factor`.
fn scaled_up(value: i128, factor: Option<i128>) -> Option<i128> {
    zero(value).or_else(|| factor.and_then(|factor| value.checked_mul(factor)))
}

/// `value` divided by `factor` when `factor` divides it.
fn scaled_down(value: i128, factor: Option<i128>) -> Option<i128> {
    zero(value).or_else(|| {
        factor
            .filter(|factor| value.checked_rem(*factor) == Some(0))
            .map(|factor| value / factor)
    })
}

/// `value` divided by `unit` when `unit` divides it; `unit` is a positive
/// constant, the ticks of a [`TimeUnit`] or the milliseconds of a day.
fn whole_units(value: i64, unit: i64) -> Option<i64> {
    (value.checked_rem(unit) == Some(0)).then(|| value / unit)
}

/// Days since 1970-01-01 of a timestamp counted in `unit` at exactly midnight.
pub fn date_from_midnight(value: i64, unit: TimeUnit) -> Result<i32, ConvertError> {
    let days =
        whole_units(value, unit.ticks_per_day()).ok_or(ConvertError::NotMidnight { value })?;
    i32::try_from(days).map_err(|_| ConvertError::OutOfRange { value })
}

/// Days since 1970-01-01 of a millisecond date (Arrow `Date64`).
pub fn date_from_millis(millis: i64) -> Result<i32, ConvertError> {
    let days = whole_units(millis, DAY_MILLIS).ok_or(ConvertError::NotWholeDay { millis })?;
    i32::try_from(days).map_err(|_| ConvertError::DayOutOfRange { millis })
}

/// Seconds since midnight of a time counted in `unit`.
pub fn seconds_from(value: i64, unit: TimeUnit) -> Result<i32, ConvertError> {
    let seconds = whole_units(value, unit.ticks_per_second())
        .ok_or(ConvertError::NotWholeSecond { value })?;
    i32::try_from(seconds).map_err(|_| ConvertError::OutOfRange { value })
}
