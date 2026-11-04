use super::super::{
    ColumnType, ElementTextError, date_display, date_element, days_from_civil, decimal_display,
    decimal_element, time_display, time_element,
};

/// The amount text messages and summaries used, kept as the reference the
/// display form must equal.
fn reference_amount(value: i128, scale: u8) -> String {
    let digits = value.unsigned_abs().to_string();
    let scale = usize::from(scale);
    let sign = if value < 0 { "-" } else { "" };
    if scale == 0 {
        return format!("{sign}{digits}");
    }
    let padded = format!("{digits:0>width$}", width = scale + 1);
    let (whole, fraction) = padded.split_at(padded.len() - scale);
    format!("{sign}{whole}.{fraction}")
}

/// The decimal text tables used, kept as the second reference for the
/// display form.
fn reference_table_decimal(value: i128, scale: usize) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let digits = format!("{:0>width$}", value.unsigned_abs(), width = scale + 1);
    let (whole, fraction) = digits.split_at(digits.len().saturating_sub(scale));
    if fraction.is_empty() {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{fraction}")
    }
}

/// The decimal text the writer used, kept as the reference the element form
/// must equal.
fn reference_element(value: i128, scale: u8) -> Vec<u8> {
    let scale = usize::from(scale);
    let mut out = Vec::new();
    if value < 0 {
        out.push(b'-');
    }
    let mut digits = [0u8; 48];
    let mut count = 0;
    let mut rest = value.unsigned_abs();
    while (rest > 0 || count <= scale)
        && let Some(slot) = digits.get_mut(count)
    {
        *slot = b'0' + (rest % 10) as u8;
        rest /= 10;
        count += 1;
    }
    let digits = digits.get(..count).unwrap_or_default();
    let (fraction, whole) = digits.split_at(scale.min(digits.len()));
    out.extend(whole.iter().rev());
    let zeros = fraction.iter().take_while(|&&digit| digit == b'0').count();
    let kept = fraction.get(zeros..).unwrap_or_default();
    if !kept.is_empty() {
        out.push(b'.');
        out.extend(kept.iter().rev());
    }
    out
}

fn element(value: i128, scale: u8) -> String {
    let mut out = b"prefix ".to_vec();
    decimal_element(value, scale, &mut out);
    let text = out
        .strip_prefix(b"prefix ")
        .expect("decimal_element appends");
    String::from_utf8(text.to_vec()).expect("decimal text is ASCII")
}

#[test]
fn decimal_forms_match_the_copies_they_replace() {
    let values = [0, 5, -5, 1250, -1250, 1200, i128::MIN, i128::MAX];
    let grid = [0u8, 1, 2, 4, 38]
        .into_iter()
        .flat_map(|scale| values.map(|value| (value, scale)));
    for (value, scale) in grid {
        let display = decimal_display(value, scale);
        let references = (
            reference_amount(value, scale),
            reference_table_decimal(value, usize::from(scale)),
            reference_element(value, scale),
        );
        assert_eq!(
            (display.clone(), display, element(value, scale).into_bytes()),
            references,
            "{value} at scale {scale}"
        );
    }
    let cases: [(i128, u8, &str, &str); 12] = [
        (1250, 2, "12.50", "12.5"),
        (12345, 2, "123.45", "123.45"),
        (-120, 0, "-120", "-120"),
        (12345, 0, "12345", "12345"),
        (5, 2, "0.05", "0.05"),
        (-5, 2, "-0.05", "-0.05"),
        (0, 2, "0.00", "0"),
        (1200, 2, "12.00", "12"),
        (-1250, 0, "-1250", "-1250"),
        (
            i128::MIN,
            2,
            "-1701411834604692317316873037158841057.28",
            "-1701411834604692317316873037158841057.28",
        ),
        (
            5,
            38,
            "0.00000000000000000000000000000000000005",
            "0.00000000000000000000000000000000000005",
        ),
        (
            i128::MAX,
            38,
            "1.70141183460469231731687303715884105727",
            "1.70141183460469231731687303715884105727",
        ),
    ];
    for (value, scale, display, element_text) in cases {
        assert_eq!(
            (decimal_display(value, scale), element(value, scale)),
            (display.to_string(), element_text.to_string()),
            "{value} at scale {scale}"
        );
    }
}

fn day(year: i32, month: i32, day: i32) -> i32 {
    days_from_civil(year, month, day).expect("the date exists")
}

#[test]
fn date_and_time_forms() {
    assert_eq!(day(2024, 2, 29), 19782);
    let displays = [
        (date_display(19782), "2024-02-29"),
        (date_display(i32::MAX), "date32(2147483647)"),
        (time_display(3661), "01:01:01"),
        (time_display(0), "00:00:00"),
        (time_display(86_399), "23:59:59"),
        (time_display(90_000), "time32(90000)"),
        (time_display(-1), "time32(-1)"),
    ];
    for (shown, expected) in displays {
        assert_eq!(shown, expected);
    }
    let elements = [
        (date_element(19782, None), Ok("20240229")),
        (date_element(19782, Some(8)), Ok("20240229")),
        (date_element(19782, Some(6)), Ok("240229")),
        (date_element(day(1950, 1, 1), Some(6)), Ok("500101")),
        (date_element(day(2049, 12, 31), Some(6)), Ok("491231")),
        (
            date_element(day(1949, 12, 31), Some(6)),
            Err(ElementTextError::TwoDigitYear { year: 1949 }),
        ),
        (
            date_element(day(2050, 1, 1), Some(6)),
            Err(ElementTextError::TwoDigitYear { year: 2050 }),
        ),
        (
            date_element(day(9999, 12, 31) + 1, None),
            Err(ElementTextError::YearOutOfRange),
        ),
        (
            date_element(day(1, 1, 1) - 1, None),
            Err(ElementTextError::YearOutOfRange),
        ),
        (
            date_element(i32::MAX, None),
            Err(ElementTextError::YearOutOfRange),
        ),
        (time_element(3600, None), Ok("0100")),
        (time_element(3600, Some(4)), Ok("0100")),
        (time_element(3661, None), Ok("010101")),
        (time_element(3661, Some(6)), Ok("010101")),
        (
            time_element(3661, Some(4)),
            Err(ElementTextError::SecondsNotHeld),
        ),
        (
            time_element(86_400, None),
            Err(ElementTextError::TimeOutsideDay),
        ),
        (
            time_element(-1, None),
            Err(ElementTextError::TimeOutsideDay),
        ),
    ];
    for (written, expected) in elements {
        assert_eq!(written, expected.map(|text| text.as_bytes().to_vec()));
    }
}

#[test]
fn element_text_errors_explain_themselves() {
    let texts = [
        (
            ElementTextError::YearOutOfRange,
            "a date needs a year from 1 to 9999",
        ),
        (
            ElementTextError::TwoDigitYear { year: 2050 },
            "the element holds a two-digit year, read as 1950 to 2049; the year is 2050",
        ),
        (
            ElementTextError::TimeOutsideDay,
            "a time must fall within one day",
        ),
        (
            ElementTextError::SecondsNotHeld,
            "the element holds hours and minutes only",
        ),
        (
            ElementTextError::NoDecimalScale {
                kind: ColumnType::Int64 { scale: 0 },
            },
            "a decimal needs the scale of a decimal128 column; the column is int64",
        ),
    ];
    for (error, text) in texts {
        assert_eq!(error.to_string(), text);
        assert!(std::error::Error::source(&error).is_none());
    }
}
