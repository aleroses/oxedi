use super::super::convert::{
    float_integer, rescale, timestamp_date, unsigned_integer, whole_seconds, wide_integer,
};
use oxedi_core::value::TimeUnit;

#[test]
fn integers_fit_or_say_why() {
    assert_eq!(wide_integer(-5), Ok(-5));
    assert_eq!(
        wide_integer(i128::from(i64::MAX) + 1),
        Err("9223372036854775808 does not fit a 64-bit integer".to_owned())
    );
    assert_eq!(unsigned_integer(7), Ok(7));
    assert_eq!(
        unsigned_integer(u128::MAX),
        Err(format!("{} does not fit a 64-bit integer", u128::MAX))
    );
}

#[test]
fn floats_must_be_whole() {
    assert_eq!(float_integer(42.0), Ok(42));
    assert_eq!(
        float_integer(1.5),
        Err("1.5 is not a whole number".to_owned())
    );
    assert_eq!(
        float_integer(f64::NAN),
        Err("NaN is not a whole number".to_owned())
    );
}

#[test]
fn decimals_rescale_exactly() {
    assert_eq!(rescale(12345, 2, 2), Ok(12345));
    assert_eq!(rescale(5, 0, 2), Ok(500));
    assert_eq!(rescale(1230, 3, 2), Ok(123));
    assert_eq!(
        rescale(1234, 3, 2),
        Err("1234 at scale 3 has more decimals than the column's scale 2".to_owned())
    );
    assert_eq!(
        rescale(i128::MAX, 0, 2),
        Err(format!("{} at scale 0 overflows scale 2", i128::MAX))
    );
    assert_eq!(rescale(0, 0, 39), Ok(0));
    assert_eq!(
        rescale(1, 0, 39),
        Err("1 at scale 0 overflows scale 39".to_owned())
    );
}

#[test]
fn timestamps_must_be_at_midnight() {
    assert_eq!(timestamp_date(86_400, TimeUnit::Second), Ok(1));
    assert_eq!(timestamp_date(86_400_000, TimeUnit::Millisecond), Ok(1));
    assert_eq!(
        timestamp_date(-86_400_000_000, TimeUnit::Microsecond),
        Ok(-1)
    );
    assert_eq!(
        timestamp_date(86_400_000_000_000, TimeUnit::Nanosecond),
        Ok(1)
    );
    assert_eq!(
        timestamp_date(1, TimeUnit::Microsecond),
        Err("timestamp 1 is not at midnight; give dates".to_owned())
    );
    assert_eq!(
        timestamp_date(i64::MAX / 86_400 * 86_400, TimeUnit::Second),
        Err(format!("{} is out of range", i64::MAX / 86_400 * 86_400))
    );
}

#[test]
fn times_must_be_whole_seconds() {
    assert_eq!(
        whole_seconds(37_800_000_000, TimeUnit::Microsecond),
        Ok(37_800)
    );
    assert_eq!(
        whole_seconds(37_800_000_000_000, TimeUnit::Nanosecond),
        Ok(37_800)
    );
    assert_eq!(
        whole_seconds(37_800_500_000_000, TimeUnit::Nanosecond),
        Err("37800500000000 is not a whole second".to_owned())
    );
    assert_eq!(
        whole_seconds(1_500_000, TimeUnit::Microsecond),
        Err("1500000 is not a whole second".to_owned())
    );
    assert_eq!(
        whole_seconds(i64::MAX - i64::MAX % 1_000, TimeUnit::Millisecond),
        Err(format!("{} is out of range", i64::MAX - i64::MAX % 1_000))
    );
}
