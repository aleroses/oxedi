use oxedi_core::Delimiters;
use oxedi_core::value::TimeUnit;
use oxedi_core::write::Envelope;

use super::super::error::CopyError;
use super::super::options::{Kind, OptionValue, Settings, parse_date, parse_time};

fn value(kind: Kind, shown: &str) -> OptionValue {
    OptionValue {
        kind,
        shown: shown.to_owned(),
    }
}

fn varchar(text: &str) -> OptionValue {
    value(Kind::Text(text.to_owned()), &format!("VARCHAR {text:?}"))
}

/// The required options, then `extra`.
fn given(extra: Vec<(&str, OptionValue)>) -> Vec<(String, OptionValue)> {
    let mut all = vec![
        ("sender_id", varchar("SENDER")),
        ("receiver_id", varchar("RECEIVER")),
        ("date", varchar("2024-01-02")),
        ("time", varchar("10:30")),
    ];
    all.extend(extra);
    all.into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect()
}

fn error(extra: Vec<(&str, OptionValue)>) -> String {
    match Settings::parse(&given(extra)) {
        Ok(settings) => format!("parsed: {settings:?}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn defaults_are_those_of_python() {
    let settings = Settings::parse(&given(Vec::new())).ok();
    let expected = Envelope::new("ZZ", "SENDER", "ZZ", "RECEIVER", 19_724, 37_800);
    assert_eq!(
        settings,
        Some(Settings {
            envelope: expected,
            version: "5010".to_owned(),
        })
    );
}

#[test]
fn every_option_reaches_the_envelope() {
    let delimiters = value(
        Kind::Struct(vec![
            ("element".to_owned(), varchar("|")),
            ("repetition".to_owned(), varchar("!")),
            (
                "release".to_owned(),
                value(Kind::Bytes(b"?".to_vec()), "BLOB ?"),
            ),
        ]),
        "STRUCT {...}",
    );
    let settings = Settings::parse(&given(vec![
        ("sender_qualifier", varchar("01")),
        ("receiver_qualifier", varchar("14")),
        ("usage_indicator", varchar("T")),
        ("control_number", value(Kind::Integer(42), "INTEGER 42")),
        ("application_sender", varchar("APPS")),
        ("application_receiver", varchar("APPR")),
        ("delimiters", delimiters),
        ("line_break", value(Kind::Bool(true), "BOOLEAN true")),
        ("version", varchar("4010")),
    ]))
    .ok();
    let mut expected = Envelope::new("01", "SENDER", "14", "RECEIVER", 19_724, 37_800);
    expected.usage_indicator = "T".to_owned();
    expected.control_number = 42;
    expected.application_sender = Some("APPS".to_owned());
    expected.application_receiver = Some("APPR".to_owned());
    let mut chosen = Delimiters::new(b'|', b':', b'~');
    chosen.repetition = Some(b'!');
    chosen.release = Some(b'?');
    expected.delimiters = chosen;
    expected.line_break = true;
    assert_eq!(
        settings,
        Some(Settings {
            envelope: expected,
            version: "4010".to_owned(),
        })
    );
}

#[test]
fn typed_date_and_time_and_a_bare_line_break() {
    let all = vec![
        ("sender_id".to_owned(), varchar("SENDER")),
        ("receiver_id".to_owned(), varchar("RECEIVER")),
        (
            "date".to_owned(),
            value(Kind::Date(19_724), "DATE 2024-01-02"),
        ),
        (
            "time".to_owned(),
            value(
                Kind::Time {
                    value: 37_815_000_000,
                    unit: TimeUnit::Microsecond,
                },
                "TIME 10:30:15",
            ),
        ),
        ("line_break".to_owned(), value(Kind::Null, "NULL")),
    ];
    let envelope = Settings::parse(&all).map(|settings| settings.envelope).ok();
    assert_eq!(
        envelope.as_ref().map(|e| (e.date, e.time)),
        Some((19_724, 37_815))
    );
    assert_eq!(envelope.map(|e| e.line_break), Some(true));
}

#[test]
fn an_unknown_option_lists_the_options() {
    assert!(
        error(vec![("header", value(Kind::Bool(true), "BOOLEAN true"))])
            .starts_with("edi835: unknown option \"header\"; the options are \"sender_id\"")
    );
}

#[test]
fn compression_does_not_apply() {
    assert_eq!(
        error(vec![("compression", varchar("gzip"))]),
        "edi835: the option \"compression\" does not apply: the format writes one uncompressed \
         file; compress it afterwards"
    );
}

#[test]
fn a_required_option_must_be_given() {
    let all = vec![("sender_id".to_owned(), varchar("SENDER"))];
    assert_eq!(
        Settings::parse(&all).map_err(|error| error.to_string()),
        Err(
            "edi835: the option \"receiver_id\" is required; every one of \"sender_id\", \
             \"receiver_id\", \"date\", \"time\" must be given"
                .to_owned()
        )
    );
}

#[test]
fn a_bare_text_option_needs_a_value() {
    assert_eq!(
        error(vec![("usage_indicator", value(Kind::Null, "NULL"))]),
        "edi835: the option \"usage_indicator\" needs a value"
    );
}

#[test]
fn wrong_types_name_the_option_value_and_form() {
    assert_eq!(
        error(vec![(
            "sender_qualifier",
            value(Kind::Integer(1), "INTEGER 1")
        )]),
        "edi835: the option \"sender_qualifier\" is INTEGER 1; it must be text (VARCHAR)"
    );
    assert_eq!(
        error(vec![(
            "control_number",
            value(Kind::Integer(-1), "INTEGER -1")
        )]),
        "edi835: the option \"control_number\" is INTEGER -1; it must be a whole number from 0 \
         to 18446744073709551615"
    );
    assert_eq!(
        error(vec![("line_break", varchar("yes"))]),
        "edi835: the option \"line_break\" is VARCHAR \"yes\"; it must be a BOOLEAN"
    );
    assert_eq!(
        error(vec![(
            "version",
            value(Kind::Integer(5010), "INTEGER 5010")
        )]),
        "edi835: the option \"version\" is INTEGER 5010; it must be text (VARCHAR) naming a \
         version"
    );
    assert_eq!(
        error(vec![("delimiters", varchar("|"))]),
        "edi835: the option \"delimiters\" is VARCHAR \"|\"; it must be a STRUCT of one-byte \
         texts with any of the fields element, component, segment, repetition and release, such \
         as {'element': '|'}"
    );
}

#[test]
fn a_date_is_a_date_or_iso_text() {
    let mut all = given(Vec::new());
    if let Some(slot) = all.get_mut(2) {
        slot.1 = varchar("2024-02-30");
    }
    assert_eq!(
        Settings::parse(&all).map_err(|error| error.to_string()),
        Err(
            "edi835: the option \"date\" is VARCHAR \"2024-02-30\"; it must be a DATE, or text \
             in the form YYYY-MM-DD"
                .to_owned()
        )
    );
}

#[test]
fn a_time_is_whole_seconds() {
    let mut all = given(Vec::new());
    if let Some(slot) = all.get_mut(3) {
        slot.1 = value(
            Kind::Time {
                value: 37_800_500_000,
                unit: TimeUnit::Microsecond,
            },
            "TIME 10:30:00.5",
        );
    }
    assert_eq!(
        Settings::parse(&all).map_err(|error| error.to_string()),
        Err(
            "edi835: the option \"time\" is TIME 10:30:00.5, which has a fraction of a second; \
             the envelope holds whole seconds"
                .to_owned()
        )
    );
    if let Some(slot) = all.get_mut(3) {
        slot.1 = value(
            Kind::Time {
                value: 86_400_000_000,
                unit: TimeUnit::Microsecond,
            },
            "TIME 24:00:00",
        );
    }
    assert_eq!(
        Settings::parse(&all).map_err(|error| error.to_string()),
        Err(
            "edi835: the option \"time\" is TIME 24:00:00, which is out of range for an X12 \
             time; it must be from 00:00:00 to 23:59:59"
                .to_owned()
        )
    );
    if let Some(slot) = all.get_mut(3) {
        slot.1 = varchar("25:00");
    }
    assert_eq!(
        Settings::parse(&all).map_err(|error| error.to_string()),
        Err(
            "edi835: the option \"time\" is VARCHAR \"25:00\"; it must be a TIME or TIME_NS, or text in \
             the form HH:MM, HHMM or HH:MM:SS"
                .to_owned()
        )
    );
}

#[test]
fn a_time_ns_is_whole_seconds_too() {
    let ns = |value_ns: i64, shown: &str| {
        value(
            Kind::Time {
                value: value_ns,
                unit: TimeUnit::Nanosecond,
            },
            shown,
        )
    };
    let mut all = given(Vec::new());
    if let Some(slot) = all.get_mut(3) {
        slot.1 = ns(37_815_000_000_000, "TIME_NS 10:30:15");
    }
    let envelope = Settings::parse(&all).map(|settings| settings.envelope).ok();
    assert_eq!(envelope.map(|envelope| envelope.time), Some(37_815));
    if let Some(slot) = all.get_mut(3) {
        slot.1 = ns(37_800_500_000_000, "TIME_NS 10:30:00.5");
    }
    assert_eq!(
        Settings::parse(&all).map_err(|error| error.to_string()),
        Err(
            "edi835: the option \"time\" is TIME_NS 10:30:00.5, which has a fraction of a \
             second; the envelope holds whole seconds"
                .to_owned()
        )
    );
    if let Some(slot) = all.get_mut(3) {
        slot.1 = ns(86_400_000_000_000, "TIME_NS 24:00:00");
    }
    assert_eq!(
        Settings::parse(&all).map_err(|error| error.to_string()),
        Err(
            "edi835: the option \"time\" is TIME_NS 24:00:00, which is out of range for an X12 \
             time; it must be from 00:00:00 to 23:59:59"
                .to_owned()
        )
    );
}

#[test]
fn delimiters_are_one_byte_each() {
    let delimiters = |field: &str, given: OptionValue| {
        value(
            Kind::Struct(vec![(field.to_owned(), given)]),
            "STRUCT {...}",
        )
    };
    assert_eq!(
        error(vec![("delimiters", delimiters("element", varchar("**")))]),
        "edi835: delimiter element must be exactly one byte, got 2 bytes: b'**'"
    );
    assert_eq!(
        error(vec![("delimiters", delimiters("sub", varchar("|")))]),
        "edi835: delimiters has no field \"sub\"; its fields are \"element\", \"component\", \
         \"segment\", \"repetition\", \"release\""
    );
    assert_eq!(
        error(vec![(
            "delimiters",
            delimiters("segment", value(Kind::Integer(7), "INTEGER 7"))
        )]),
        "edi835: the option \"delimiters\" is INTEGER 7 in the field \"segment\"; it must be a \
         one-byte VARCHAR or BLOB"
    );
    let settings = Settings::parse(&given(vec![(
        "delimiters",
        delimiters("repetition", value(Kind::Null, "NULL")),
    )]));
    assert_eq!(
        settings.map(|settings| settings.envelope.delimiters).ok(),
        Some(Delimiters::new(b'*', b':', b'~'))
    );
}

#[test]
fn dates_parse_strictly() {
    assert_eq!(parse_date("1970-01-01"), Some(0));
    assert_eq!(parse_date("2024-01-02"), Some(19_724));
    assert_eq!(parse_date("2024-02-29"), Some(19_782));
    assert_eq!(parse_date("1969-12-31"), Some(-1));
    assert_eq!(parse_date("2023-02-29"), None);
    assert_eq!(parse_date("2024-1-02"), None);
    assert_eq!(parse_date("2024-13-01"), None);
    assert_eq!(parse_date("20240102"), None);
}

#[test]
fn times_parse_in_three_forms() {
    assert_eq!(parse_time("10:30"), Some(37_800));
    assert_eq!(parse_time("1030"), Some(37_800));
    assert_eq!(parse_time("10:30:15"), Some(37_815));
    assert_eq!(parse_time("23:59:59"), Some(86_399));
    assert_eq!(parse_time("24:00"), None);
    assert_eq!(parse_time("10:60"), None);
    assert_eq!(parse_time("10-30"), None);
    assert_eq!(parse_time("1:30"), None);
}

#[test]
fn unknown_options_are_reported_before_missing_ones() {
    let all = vec![("bogus".to_owned(), varchar("x"))];
    assert!(matches!(
        Settings::parse(&all),
        Err(CopyError::UnknownOption { .. })
    ));
}
