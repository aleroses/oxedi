//! Decimals, dates and times as text, in two forms: the display form tables
//! and messages show (`12.50`, `2024-02-29`, `01:01:01`) and the element form
//! an X12 element holds (`12.5`, `20240229`, `010101`).

use std::fmt;
use std::iter;

use super::ColumnType;
use super::calendar::{civil_from_days, time_of_day};

/// The most decimal digits a `u128` has, and so an `i128`'s magnitude.
const MAX_DIGITS: usize = 39;

/// The decimal digits of a magnitude, most significant first, in a buffer of
/// their own: at least one digit, `0` for zero.
struct Digits {
    buffer: [u8; MAX_DIGITS],
    start: usize,
}

impl Digits {
    fn of(mut magnitude: u128) -> Digits {
        let mut buffer = [b'0'; MAX_DIGITS];
        let mut start = MAX_DIGITS;
        for slot in buffer.iter_mut().rev() {
            // The remainder of a division by ten is one digit.
            *slot = b'0' + (magnitude % 10) as u8;
            magnitude /= 10;
            start -= 1;
            if magnitude == 0 {
                break;
            }
        }
        Digits { buffer, start }
    }

    fn get(&self) -> &[u8] {
        self.buffer.get(self.start..).unwrap_or_default()
    }
}

/// A scaled decimal laid out in fixed point: its sign, the digits before the
/// point, the zeros that open the fraction and the fraction's other digits.
/// The point is written when the fraction has digits.
struct FixedPoint<'a> {
    negative: bool,
    whole: &'a [u8],
    zeros: usize,
    fraction: &'a [u8],
}

impl<'a> FixedPoint<'a> {
    /// `value` (a sign and `digits`, its magnitude) with `scale` fraction
    /// digits, every one of them kept.
    fn new(negative: bool, digits: &'a [u8], scale: u8) -> FixedPoint<'a> {
        let scale = usize::from(scale);
        let split = digits.len().saturating_sub(scale);
        let (whole, fraction) = digits.split_at(split);
        FixedPoint {
            negative,
            whole: if whole.is_empty() { b"0" } else { whole },
            zeros: scale - fraction.len(),
            fraction,
        }
    }

    /// The same value with the fraction's trailing zeros left out, and the
    /// fraction left out when only zeros remain.
    fn trimmed(self) -> FixedPoint<'a> {
        let kept = self
            .fraction
            .iter()
            .rposition(|&digit| digit != b'0')
            .map_or(0, |last| last + 1);
        let fraction = self.fraction.get(..kept).unwrap_or_default();
        FixedPoint {
            zeros: if fraction.is_empty() { 0 } else { self.zeros },
            fraction,
            ..self
        }
    }

    fn bytes(&self) -> impl Iterator<Item = u8> + '_ {
        let point = (!self.fraction.is_empty()).then_some(b'.');
        self.negative
            .then_some(b'-')
            .into_iter()
            .chain(self.whole.iter().copied())
            .chain(point)
            .chain(iter::repeat_n(b'0', self.zeros))
            .chain(self.fraction.iter().copied())
    }
}

/// A scaled decimal (`value` is the number times `10^scale`) as tables and
/// messages show it: fixed point with every fraction digit the scale gives,
/// `12.50` at scale 2 and `-0.05` for `-5`.
pub fn decimal_display(value: i128, scale: u8) -> String {
    let digits = Digits::of(value.unsigned_abs());
    FixedPoint::new(value < 0, digits.get(), scale)
        .bytes()
        .map(char::from)
        .collect()
}

/// Appends a scaled decimal as an element holds it: fixed point without the
/// fraction's trailing zeros and without a point when nothing follows it,
/// `12.5` for `1250` at scale 2 and `0` for zero.
pub fn decimal_element(value: i128, scale: u8, out: &mut Vec<u8>) {
    let digits = Digits::of(value.unsigned_abs());
    out.extend(
        FixedPoint::new(value < 0, digits.get(), scale)
            .trimmed()
            .bytes(),
    );
}

/// A date (days since 1970-01-01) as `YYYY-MM-DD`, or `date32(<n>)` when the
/// day is too far from 1970 to have a calendar date.
pub fn date_display(days: i32) -> String {
    let Some((year, month, day)) = civil_from_days(days) else {
        return format!("date32({days})");
    };
    format!("{year:04}-{month:02}-{day:02}")
}

/// A time (seconds since midnight) as `HH:MM:SS`, or `time32(<n>)` when it
/// falls outside one day.
pub fn time_display(seconds: i32) -> String {
    if time_of_day(i64::from(seconds)).is_none() {
        return format!("time32({seconds})");
    }
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// A date (days since 1970-01-01) as an element holds it: `CCYYMMDD`, or
/// `YYMMDD` when the element holds six bytes at most (`max`, its maximum
/// length), which reads its two-digit year as 1950 to 2049.
pub fn date_element(days: i32, max: Option<usize>) -> Result<Vec<u8>, ElementTextError> {
    let Some((year, month, day)) = civil_from_days(days) else {
        return Err(ElementTextError::YearOutOfRange);
    };
    if max.is_some_and(|max| max < 8) {
        if !(1950..=2049).contains(&year) {
            return Err(ElementTextError::TwoDigitYear { year });
        }
        return Ok(format!("{:02}{month:02}{day:02}", year % 100).into_bytes());
    }
    if !(1..=9999).contains(&year) {
        return Err(ElementTextError::YearOutOfRange);
    }
    Ok(format!("{year:04}{month:02}{day:02}").into_bytes())
}

/// A time (seconds since midnight) as an element holds it: `HHMM`, or
/// `HHMMSS` when it has seconds and the element (`max`, its maximum length)
/// holds them.
pub fn time_element(seconds: i32, max: Option<usize>) -> Result<Vec<u8>, ElementTextError> {
    if time_of_day(i64::from(seconds)).is_none() {
        return Err(ElementTextError::TimeOutsideDay);
    }
    let (hours, minutes, rest) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if rest == 0 {
        return Ok(format!("{hours:02}{minutes:02}").into_bytes());
    }
    if max.is_some_and(|max| max < 6) {
        return Err(ElementTextError::SecondsNotHeld);
    }
    Ok(format!("{hours:02}{minutes:02}{rest:02}").into_bytes())
}

/// Why a value has no text its element can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementTextError {
    /// The date's year is outside 1 to 9999.
    YearOutOfRange,
    /// The element holds a two-digit year, read as 1950 to 2049, and the
    /// date's year is outside them.
    TwoDigitYear {
        /// The date's year.
        year: i32,
    },
    /// The time is negative or past 23:59:59.
    TimeOutsideDay,
    /// The time has seconds and the element holds hours and minutes only.
    SecondsNotHeld,
    /// A decimal cell in a column whose type has no decimal scale.
    NoDecimalScale {
        /// The column's type.
        kind: ColumnType,
    },
}

impl fmt::Display for ElementTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElementTextError::YearOutOfRange => {
                write!(f, "a date needs a year from 1 to 9999")
            }
            ElementTextError::TwoDigitYear { year } => write!(
                f,
                "the element holds a two-digit year, read as 1950 to 2049; the year is {year}"
            ),
            ElementTextError::TimeOutsideDay => write!(f, "a time must fall within one day"),
            ElementTextError::SecondsNotHeld => {
                write!(f, "the element holds hours and minutes only")
            }
            ElementTextError::NoDecimalScale { kind } => write!(
                f,
                "a decimal needs the scale of a decimal128 column; the column is {kind}"
            ),
        }
    }
}

impl std::error::Error for ElementTextError {}
