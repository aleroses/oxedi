use super::super::calendar::days_in_month;
use super::super::{civil_from_days, parse_dt, parse_n, parse_r, parse_tm, parse_unsigned};
use proptest::prelude::*;

/// An `R` value written the way X12 writes it, with exactly `scale` decimals.
fn format_r(value: i128, scale: u8) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let digits = value.unsigned_abs().to_string();
    let scale = usize::from(scale);
    if scale == 0 {
        return format!("{sign}{digits}");
    }
    let padded = format!("{digits:0>width$}", width = scale + 1);
    let (whole, fraction) = padded.split_at(padded.len() - scale);
    format!("{sign}{whole}.{fraction}")
}

#[test]
fn n_values_are_digits_with_an_optional_minus() {
    assert_eq!(parse_n(b"0"), Some(0));
    assert_eq!(parse_n(b"007"), Some(7));
    assert_eq!(parse_n(b"-5"), Some(-5));
    assert_eq!(parse_n(b"9223372036854775807"), Some(i64::MAX));
    assert_eq!(parse_n(b"-9223372036854775808"), Some(i64::MIN));
    for text in [
        &b""[..],
        b"-",
        b"+5",
        b"1.0",
        b" 1",
        b"1 ",
        b"1a",
        b"9223372036854775808",
    ] {
        assert_eq!(parse_n(text), None, "{:?}", String::from_utf8_lossy(text));
    }
}

#[test]
fn r_values_are_scaled_and_never_lose_a_decimal() {
    assert_eq!(parse_r(b"12.34", 2), Some(1234));
    assert_eq!(parse_r(b"12.3", 2), Some(1230));
    assert_eq!(parse_r(b"12", 2), Some(1200));
    assert_eq!(parse_r(b"-0.5", 2), Some(-50));
    assert_eq!(parse_r(b".5", 2), Some(50));
    assert_eq!(parse_r(b"5.", 2), Some(500));
    assert_eq!(parse_r(b"12", 0), Some(12));
    assert_eq!(
        parse_r(b"99999999999999999999.999999999999999999", 18),
        Some(99_999_999_999_999_999_999_999_999_999_999_999_999)
    );
    for (text, scale) in [
        (&b"12.345"[..], 2),
        (b"12.0", 0),
        (b"", 2),
        (b".", 2),
        (b"-", 2),
        (b"1.2.3", 2),
        (b"1e5", 2),
        (b" 1", 2),
        (b"1 ", 2),
        (b"+1", 2),
        (b"1,5", 2),
        (b"100000000000000000000", 18),
    ] {
        assert_eq!(
            parse_r(text, scale),
            None,
            "{:?} at scale {scale}",
            String::from_utf8_lossy(text)
        );
    }
}

#[test]
fn dt_values_are_real_calendar_dates() {
    assert_eq!(parse_dt(b"19700101"), Some(0));
    assert_eq!(parse_dt(b"700101"), Some(0));
    assert_eq!(parse_dt(b"19691231"), Some(-1));
    assert_eq!(parse_dt(b"20240229"), Some(19_782));
    assert_eq!(parse_dt(b"20000229"), Some(11_016));
    assert_eq!(parse_dt(b"491231"), parse_dt(b"20491231"));
    assert_eq!(parse_dt(b"500101"), parse_dt(b"19500101"));
    for text in [
        &b"20230229"[..],
        b"19000229",
        b"20241301",
        b"20240100",
        b"20240431",
        b"2024011",
        b"2024-01-01",
        b"",
        b"240229 ",
        b"00000101",
        b"00000000",
    ] {
        assert_eq!(parse_dt(text), None, "{:?}", String::from_utf8_lossy(text));
    }
}

#[test]
fn tm_values_are_seconds_since_midnight() {
    assert_eq!(parse_tm(b"0000"), Some(0));
    assert_eq!(parse_tm(b"1230"), Some(45_000));
    assert_eq!(parse_tm(b"123045"), Some(45_045));
    assert_eq!(parse_tm(b"1230459"), Some(45_045));
    assert_eq!(parse_tm(b"12304599"), Some(45_045));
    assert_eq!(parse_tm(b"235959"), Some(86_399));
    for text in [
        &b"2400"[..],
        b"1260",
        b"123060",
        b"123",
        b"12304",
        b"123045999",
        b"12a0",
        b"",
        b"12:30",
    ] {
        assert_eq!(parse_tm(text), None, "{:?}", String::from_utf8_lossy(text));
    }
}

#[test]
fn dates_and_times_at_their_bounds() {
    assert_eq!(parse_dt(b"240229"), Some(19_782));
    for text in [&b"20240230"[..], b"2024021"] {
        assert_eq!(parse_dt(text), None, "{:?}", String::from_utf8_lossy(text));
    }
    assert_eq!(parse_tm(b"2359"), Some(86_340));
    assert_eq!(parse_tm(b"2359599"), Some(86_399));
    assert_eq!(parse_tm(b"23595999"), Some(86_399));
    for text in [&b"2360"[..], b"235960", b"23595"] {
        assert_eq!(parse_tm(text), None, "{:?}", String::from_utf8_lossy(text));
    }
}

#[test]
fn parse_unsigned_cases() {
    assert_eq!(parse_unsigned(b"0012"), Some(12));
    assert_eq!(parse_unsigned(b"-5"), None);
    assert_eq!(parse_unsigned(b""), None);
    assert_eq!(
        parse_unsigned(b"9223372036854775808"),
        Some(9_223_372_036_854_775_808)
    );
    assert_eq!(parse_unsigned(b"18446744073709551616"), None);
    assert_eq!(parse_unsigned(b"12a"), None);
}

proptest! {
    #[test]
    fn valid_n_values_round_trip(value in any::<i64>()) {
        prop_assert_eq!(parse_n(value.to_string().as_bytes()), Some(value));
    }

    #[test]
    fn valid_r_values_round_trip(
        scale in 0u8..=18,
        value in -(10i128.pow(30))..10i128.pow(30),
    ) {
        prop_assert_eq!(parse_r(format_r(value, scale).as_bytes(), scale), Some(value));
    }

    #[test]
    fn valid_dates_round_trip(year in 1i32..=9999, month in 1i32..=12, day in 1i32..=31) {
        prop_assume!(day <= days_in_month(year, month));
        let text = format!("{year:04}{month:02}{day:02}");
        let days = parse_dt(text.as_bytes());
        prop_assert!(days.is_some(), "{}", text);
        prop_assert_eq!(days.and_then(civil_from_days), Some((year, month, day)));
    }

    #[test]
    fn valid_times_round_trip(seconds in 0i32..86_400) {
        let text = format!(
            "{:02}{:02}{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        );
        prop_assert_eq!(parse_tm(text.as_bytes()), Some(seconds));
    }

    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..24), scale in 0u8..=40) {
        let _ = parse_n(&bytes);
        let _ = parse_r(&bytes, scale);
        let _ = parse_dt(&bytes);
        let _ = parse_tm(&bytes);
    }
}
