use super::super::calendar::days_from_civil_unchecked;
use super::super::{civil_from_days, days_from_civil, seconds_of_day, time_of_day, year_start};

#[test]
fn the_epoch_and_a_leap_day_are_found() {
    assert_eq!(days_from_civil(1970, 1, 1), Some(0));
    assert_eq!(days_from_civil(2024, 2, 29), Some(19_782));
}

#[test]
fn a_date_that_does_not_exist_is_none() {
    assert_eq!(days_from_civil(2023, 2, 29), None);
    assert_eq!(days_from_civil(0, 1, 1), None);
    assert_eq!(days_from_civil(10_000, 1, 1), None);
    assert_eq!(days_from_civil(2024, 13, 1), None);
    assert_eq!(days_from_civil(2024, 0, 1), None);
    assert_eq!(days_from_civil(2024, 4, 31), None);
    assert_eq!(days_from_civil(2024, 4, 0), None);
}

#[test]
fn seconds_of_the_day_stop_before_midnight() {
    assert_eq!(seconds_of_day(0, 0, 0), Some(0));
    assert_eq!(seconds_of_day(23, 59, 59), Some(86_399));
    assert_eq!(seconds_of_day(24, 0, 0), None);
    assert_eq!(seconds_of_day(0, 60, 0), None);
    assert_eq!(seconds_of_day(0, 0, 60), None);
    assert_eq!(seconds_of_day(-1, 0, 0), None);
}

#[test]
fn a_time_of_day_is_below_one_day() {
    assert_eq!(time_of_day(0), Some(0));
    assert_eq!(time_of_day(86_399), Some(86_399));
    assert_eq!(time_of_day(86_400), None);
    assert_eq!(time_of_day(-1), None);
}

/// An independent `i64` year start, the reference the checked path must equal.
fn reference_year_start(days: i32) -> i32 {
    let days_i64 = i64::from(days);
    let z = days_i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let year = yoe + era * 400 + i64::from(mp >= 10);
    let y = year - 1;
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + (153 * 10 + 2) / 5;
    i32::try_from(era * 146_097 + doe - 719_468).unwrap_or(days)
}

#[test]
fn year_start_matches_the_mask_on_every_day() {
    let first = days_from_civil(1, 1, 1).unwrap();
    let last = days_from_civil(9999, 12, 31).unwrap();
    for days in first..=last {
        assert_eq!(
            year_start(days).unwrap_or(days),
            reference_year_start(days),
            "day {days}"
        );
    }
}

#[test]
fn year_start_at_the_edges() {
    assert_eq!(year_start(i32::MIN), None);
    for days in [i32::MAX, 2_146_764_293] {
        assert_eq!(
            year_start(days).unwrap_or(days),
            reference_year_start(days),
            "day {days}"
        );
    }
}

#[test]
fn a_day_whose_year_does_not_fit_is_none() {
    assert_eq!(days_from_civil_unchecked(i32::MAX, 12, 31), None);
    assert_eq!(civil_from_days(i32::MAX), None);
    assert_eq!(civil_from_days(i32::MAX - 719_467), None);
    assert_eq!(civil_from_days(i32::MAX - 719_468), Some((5_879_610, 9, 9)));
}
