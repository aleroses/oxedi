use super::super::{
    ConvertError, DECIMAL_PRECISION, MAX_SCALE, TimeUnit, date_from_midnight, date_from_millis,
    integer_from_f64, integer_from_i128, integer_from_u128, rescale, seconds_from,
};
use std::error::Error;

#[test]
fn every_variant_explains_itself() {
    let cases = [
        (
            ConvertError::TooWide { value: 1 << 70 },
            "1180591620717411303424 does not fit a 64-bit integer",
        ),
        (
            ConvertError::TooWideUnsigned { value: u128::MAX },
            "340282366920938463463374607431768211455 does not fit a 64-bit integer",
        ),
        (
            ConvertError::NotWhole { value: 1.5 },
            "1.5 is not a whole number",
        ),
        (
            ConvertError::ScaleOverflow {
                value: 7,
                from: 0,
                scale: 39,
            },
            "7 at scale 0 overflows scale 39",
        ),
        (
            ConvertError::TooManyDecimals {
                value: 1234,
                from: 3,
                scale: 2,
            },
            "1234 at scale 3 has more decimals than the column's scale 2",
        ),
        (
            ConvertError::NotMidnight { value: 1 },
            "timestamp 1 is not at midnight; give dates",
        ),
        (
            ConvertError::NotWholeDay { millis: 5 },
            "5 ms is not a whole day",
        ),
        (
            ConvertError::DayOutOfRange { millis: 5 },
            "5 ms is out of range",
        ),
        (ConvertError::OutOfRange { value: 9 }, "9 is out of range"),
        (
            ConvertError::NotWholeSecond { value: 1_500_000 },
            "1500000 is not a whole second",
        ),
    ];
    for (error, text) in cases {
        assert_eq!(error.to_string(), text);
    }
}

#[test]
fn a_convert_error_has_no_source() {
    assert!(ConvertError::OutOfRange { value: 1 }.source().is_none());
}

#[test]
fn integers_fit_or_say_why() {
    assert_eq!(integer_from_i128(-5), Ok(-5));
    assert_eq!(
        integer_from_i128(i128::from(i64::MAX) + 1),
        Err(ConvertError::TooWide {
            value: i128::from(i64::MAX) + 1
        })
    );
    assert_eq!(integer_from_u128(7), Ok(7));
    assert_eq!(
        integer_from_u128(u128::MAX),
        Err(ConvertError::TooWideUnsigned { value: u128::MAX })
    );
}

#[test]
fn floats_must_be_whole() {
    assert_eq!(integer_from_f64(42.0), Ok(42));
    assert_eq!(
        integer_from_f64(1.5),
        Err(ConvertError::NotWhole { value: 1.5 })
    );
    assert_eq!(
        integer_from_f64(f64::NAN).unwrap_err().to_string(),
        "NaN is not a whole number"
    );
    assert!(integer_from_f64(f64::INFINITY).is_err());
    assert!(integer_from_f64(1e19).is_err());
}

#[test]
fn rescale_cases() {
    let overflow = |value, from, scale| Err(ConvertError::ScaleOverflow { value, from, scale });
    let decimals = |value, from, scale| Err(ConvertError::TooManyDecimals { value, from, scale });
    let cases: [(i128, i32, u8, Result<i128, ConvertError>); 12] = [
        (0, 0, 39, Ok(0)),
        (1, 0, 39, overflow(1, 0, 39)),
        (0, -40, 2, Ok(0)),
        (1, -40, 2, overflow(1, -40, 2)),
        (12345, 2, 2, Ok(12345)),
        (5, 0, 2, Ok(500)),
        (1230, 3, 2, Ok(123)),
        (1234, 3, 2, decimals(1234, 3, 2)),
        (i128::MAX, 0, 2, overflow(i128::MAX, 0, 2)),
        (i128::MAX, 38, 0, decimals(i128::MAX, 38, 0)),
        (0, 50, 0, Ok(0)),
        (0, -50, 0, Ok(0)),
    ];
    for (value, from, scale, expected) in cases {
        assert_eq!(
            rescale(value, from, scale),
            expected,
            "{value} {from} {scale}"
        );
    }
}

#[test]
fn every_reachable_shift_has_a_power() {
    for from in 0..=i32::from(DECIMAL_PRECISION) {
        for scale in 0..=MAX_SCALE {
            let shift = (i32::from(scale) - from).unsigned_abs();
            assert!(10i128.checked_pow(shift).is_some(), "{from} {scale}");
        }
    }
}

#[test]
fn timestamps_must_be_at_midnight() {
    assert_eq!(date_from_midnight(86_400, TimeUnit::Second), Ok(1));
    assert_eq!(date_from_midnight(86_400_000, TimeUnit::Millisecond), Ok(1));
    assert_eq!(
        date_from_midnight(-86_400_000_000, TimeUnit::Microsecond),
        Ok(-1)
    );
    assert_eq!(
        date_from_midnight(1, TimeUnit::Microsecond),
        Err(ConvertError::NotMidnight { value: 1 })
    );
    assert_eq!(
        date_from_midnight(i64::MIN, TimeUnit::Second),
        Err(ConvertError::NotMidnight { value: i64::MIN })
    );
    let far = i64::MAX / 86_400 * 86_400;
    assert_eq!(
        date_from_midnight(far, TimeUnit::Second),
        Err(ConvertError::OutOfRange { value: far })
    );
}

#[test]
fn millisecond_dates_must_be_whole_days() {
    assert_eq!(date_from_millis(86_400_000), Ok(1));
    assert_eq!(
        date_from_millis(5),
        Err(ConvertError::NotWholeDay { millis: 5 })
    );
    let far = i64::MAX / 86_400_000 * 86_400_000;
    assert_eq!(
        date_from_millis(far),
        Err(ConvertError::DayOutOfRange { millis: far })
    );
}

#[test]
fn times_must_be_whole_seconds() {
    assert_eq!(
        seconds_from(37_800_000_000, TimeUnit::Microsecond),
        Ok(37_800)
    );
    assert_eq!(
        seconds_from(1_500_000, TimeUnit::Microsecond),
        Err(ConvertError::NotWholeSecond { value: 1_500_000 })
    );
    let far = i64::MAX - i64::MAX % 1_000;
    assert_eq!(
        seconds_from(far, TimeUnit::Millisecond),
        Err(ConvertError::OutOfRange { value: far })
    );
}

#[test]
fn every_unit_counts_its_ticks() {
    let cases = [
        (TimeUnit::Second, 1, 86_400),
        (TimeUnit::Millisecond, 1_000, 86_400_000),
        (TimeUnit::Microsecond, 1_000_000, 86_400_000_000),
        (TimeUnit::Nanosecond, 1_000_000_000, 86_400_000_000_000),
    ];
    for (unit, per_second, per_day) in cases {
        assert_eq!(unit.ticks_per_second(), per_second, "{unit:?}");
        assert_eq!(unit.ticks_per_day(), per_day, "{unit:?}");
    }
}
