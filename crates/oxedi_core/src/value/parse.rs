//! Parsers from X12 element text to column values.

use super::DECIMAL_PRECISION;
use super::calendar::{days_from_civil_unchecked, is_civil, seconds_of_day};

/// Splits an optional leading minus sign off `text`.
fn sign(text: &[u8]) -> (bool, &[u8]) {
    match text {
        [b'-', rest @ ..] => (true, rest),
        _ => (false, text),
    }
}

/// Appends ASCII digits to `negated`, a value kept negative so the most
/// negative number still fits; `None` on any other byte or on overflow.
fn accumulate(negated: i128, digits: &[u8]) -> Option<i128> {
    digits.iter().try_fold(negated, |value, &byte| {
        if !byte.is_ascii_digit() {
            return None;
        }
        value.checked_mul(10)?.checked_sub(i128::from(byte - b'0'))
    })
}

/// An `N` value: an optional `-` and at least one ASCII digit, nothing
/// else (no `+`, no spaces, no decimal point). The implied decimals are the
/// column's scale, so the integer is returned as written.
pub fn parse_n(text: &[u8]) -> Option<i64> {
    let (negative, digits) = sign(text);
    if digits.is_empty() {
        return None;
    }
    let negated = accumulate(0, digits)?;
    let value = if negative {
        negated
    } else {
        negated.checked_neg()?
    };
    i64::try_from(value).ok()
}

/// A count: at least one ASCII digit, leading zeros allowed, nothing else
/// (no sign); `None` past `u64`.
pub(crate) fn parse_unsigned(text: &[u8]) -> Option<u64> {
    if text.is_empty() {
        return None;
    }
    u64::try_from(accumulate(0, text)?.checked_neg()?).ok()
}

/// An `R` value scaled by `10^scale`: an optional `-`, digits, and at most
/// one `.` followed by no more than `scale` digits; at least one digit in
/// all. `None` for anything else, including spaces, a `+`, an exponent, or
/// a value of more than [`DECIMAL_PRECISION`] digits once scaled.
pub fn parse_r(text: &[u8], scale: u8) -> Option<i128> {
    let (negative, body) = sign(text);
    let (whole, fraction) = match body.iter().position(|&byte| byte == b'.') {
        Some(at) => {
            let (whole, rest) = body.split_at(at);
            (whole, rest.get(1..).unwrap_or_default())
        }
        None => (body, &[][..]),
    };
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    let missing = usize::from(scale).checked_sub(fraction.len())?;
    let mut negated = accumulate(accumulate(0, whole)?, fraction)?;
    for _ in 0..missing {
        negated = negated.checked_mul(10)?;
    }
    let limit = 10i128.checked_pow(u32::from(DECIMAL_PRECISION))?;
    if negated <= -limit {
        return None;
    }
    if negative {
        Some(negated)
    } else {
        negated.checked_neg()
    }
}

/// Up to four ASCII digits as a number.
fn small_number(digits: &[u8]) -> Option<i32> {
    if digits.is_empty() || digits.len() > 4 {
        return None;
    }
    digits.iter().try_fold(0i32, |value, &byte| {
        byte.is_ascii_digit()
            .then(|| value * 10 + i32::from(byte - b'0'))
    })
}

/// A reader of fixed-width digit fields, front to back.
struct Fields<'a> {
    rest: &'a [u8],
}

impl<'a> Fields<'a> {
    fn new(text: &'a [u8]) -> Fields<'a> {
        Fields { rest: text }
    }

    /// The next `width` bytes as a number; `None` when fewer remain or they
    /// are not all digits.
    fn take(&mut self, width: usize) -> Option<i32> {
        let (digits, rest) = self.rest.split_at_checked(width)?;
        self.rest = rest;
        small_number(digits)
    }

    /// The next `width` bytes as a number, or 0 when nothing remains.
    fn take_or_zero(&mut self, width: usize) -> Option<i32> {
        if self.rest.is_empty() {
            return Some(0);
        }
        self.take(width)
    }

    /// Whether what remains is nothing or one or two digits.
    fn ends_in_short_fraction(&self) -> bool {
        self.rest.is_empty() || (self.rest.len() <= 2 && small_number(self.rest).is_some())
    }
}

/// A `DT` value as days since 1970-01-01: `CCYYMMDD`, or `YYMMDD` with
/// years 00–49 read as 20xx and 50–99 as 19xx. The date must exist, and the
/// year `0000` is refused (so is the all-zero date some payers write for "no
/// date"): it is not a meaningful `CCYY`.
pub fn parse_dt(text: &[u8]) -> Option<i32> {
    let (year, month, day) = civil_date(text)?;
    days_from_civil_unchecked(year, month, day)
}

/// The year, month and day of a `DT` value, checked as [`parse_dt`] describes.
fn civil_date(text: &[u8]) -> Option<(i32, i32, i32)> {
    let mut fields = Fields::new(text);
    let year = match text.len() {
        8 => fields.take(4),
        6 => fields.take(2).map(full_year),
        _ => None,
    }?;
    let month = fields.take(2)?;
    let day = fields.take(2)?;
    is_civil(year, month, day).then_some((year, month, day))
}

/// A two-digit year: 00–49 in the 2000s, 50–99 in the 1900s.
fn full_year(year: i32) -> i32 {
    if year < 50 { 2000 + year } else { 1900 + year }
}

/// A `TM` value as seconds since midnight: `HHMM`, `HHMMSS`, or `HHMMSS`
/// followed by one or two decimal-second digits, which are ignored.
pub fn parse_tm(text: &[u8]) -> Option<i32> {
    let mut fields = Fields::new(text);
    let hour = fields.take(2)?;
    let minute = fields.take(2)?;
    let second = fields.take_or_zero(2)?;
    if !fields.ends_in_short_fraction() {
        return None;
    }
    seconds_of_day(hour, minute, second)
}
