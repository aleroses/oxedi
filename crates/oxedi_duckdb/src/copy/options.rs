//! The options of `COPY … (FORMAT edi835, …)`: the envelope, with the names
//! and defaults of `oxedi.Envelope`, and the spec's version.
//!
//! DuckDB hands the options over as one STRUCT value, upper-cased names to
//! values; [`OptionValue`] is that value read into Rust, so the rules here
//! do not touch the C API.

use oxedi_core::value::{
    ConvertError, TimeUnit, days_from_civil, seconds_from, seconds_of_day, time_of_day,
};
use oxedi_core::write::Envelope;
use oxedi_core::{Builtin, Delimiters};

use super::error::CopyError;

/// Every option, in the order the messages list them.
pub const OPTIONS: &[&str] = &[
    "sender_id",
    "receiver_id",
    "date",
    "time",
    "sender_qualifier",
    "receiver_qualifier",
    "usage_indicator",
    "control_number",
    "application_sender",
    "application_receiver",
    "delimiters",
    "line_break",
    "version",
];

/// The options without a default.
pub const REQUIRED: &[&str] = &["sender_id", "receiver_id", "date", "time"];

/// The fields of `delimiters`, as `oxedi.Delimiters` names them.
pub const DELIMITER_FIELDS: &[&str] = &["element", "component", "segment", "repetition", "release"];

/// File options DuckDB passes on to the format, with why each does not
/// apply.
const FILE_OPTIONS: &[(&str, &str)] = &[(
    "compression",
    "the format writes one uncompressed file; compress it afterwards",
)];

const TEXT: &str = "text (VARCHAR)";
const CONTROL_NUMBER: &str = "a whole number from 0 to 18446744073709551615";
const DATE: &str = "a DATE, or text in the form YYYY-MM-DD";
const TIME: &str = "a TIME or TIME_NS, or text in the form HH:MM, HHMM or HH:MM:SS";
const BOOLEAN: &str = "a BOOLEAN";
const DELIMITERS: &str = "a STRUCT of one-byte texts with any of the fields element, component, \
                          segment, repetition and release, such as {'element': '|'}";
const DELIMITER: &str = "a one-byte VARCHAR or BLOB";
const VERSION: &str = "text (VARCHAR) naming a version";

/// One option value as DuckDB gave it.
#[derive(Debug, Clone, PartialEq)]
pub struct OptionValue {
    /// The value.
    pub kind: Kind,
    /// The value as messages show it: its DuckDB type and its text, e.g.
    /// `INTEGER 42`.
    pub shown: String,
}

/// The values an option can hold.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// No value: the option was given bare, or as NULL.
    Null,
    /// A BOOLEAN.
    Bool(bool),
    /// A VARCHAR.
    Text(String),
    /// A BLOB.
    Bytes(Vec<u8>),
    /// Any integer type.
    Integer(i128),
    /// A DATE, as days since 1970-01-01.
    Date(i32),
    /// A TIME or TIME_NS: `value` counted in `unit` since midnight
    /// (microseconds, or nanoseconds).
    Time { value: i64, unit: TimeUnit },
    /// A STRUCT, its fields in order.
    Struct(Vec<(String, OptionValue)>),
    /// Any other type, or several values.
    Other,
}

/// What the options of one `COPY` asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The envelope to write.
    pub envelope: Envelope,
    /// The version of the built-in spec, as given (checked by the caller
    /// against the built-ins).
    pub version: String,
}

/// A required option's value.
fn required<'a>(
    given: &'a [(String, OptionValue)],
    name: &'static str,
) -> Result<&'a OptionValue, CopyError> {
    lookup(given, name).ok_or(CopyError::MissingOption {
        name,
        required: REQUIRED,
    })
}

fn lookup<'a>(given: &'a [(String, OptionValue)], name: &str) -> Option<&'a OptionValue> {
    given
        .iter()
        .find(|(option, _)| option == name)
        .map(|(_, value)| value)
}

fn wrong(name: &'static str, value: &OptionValue, expected: &'static str) -> CopyError {
    CopyError::OptionValue {
        name,
        value: value.shown.clone(),
        expected,
    }
}

/// The text of a VARCHAR option.
fn text(name: &'static str, value: &OptionValue) -> Result<String, CopyError> {
    match &value.kind {
        Kind::Text(text) => Ok(text.clone()),
        Kind::Null => Err(CopyError::NoValue { name }),
        _ => Err(wrong(name, value, TEXT)),
    }
}

/// Refuses an option that is a file option of `COPY` or that is not known.
fn reject_unknown(given: &[(String, OptionValue)]) -> Result<(), CopyError> {
    for (name, _) in given {
        if let Some((_, reason)) = FILE_OPTIONS.iter().find(|(option, _)| option == name) {
            return Err(CopyError::FileOption {
                name: name.clone(),
                reason,
            });
        }
        if !OPTIONS.contains(&name.as_str()) {
            return Err(CopyError::UnknownOption {
                name: name.clone(),
                known: OPTIONS,
            });
        }
    }
    Ok(())
}

/// A required VARCHAR option.
fn required_text(given: &[(String, OptionValue)], name: &'static str) -> Result<String, CopyError> {
    text(name, required(given, name)?)
}

fn required_date(given: &[(String, OptionValue)]) -> Result<i32, CopyError> {
    date(required(given, "date")?)
}

fn required_time(given: &[(String, OptionValue)]) -> Result<i32, CopyError> {
    time(required(given, "time")?)
}

/// Reads one optional option into the envelope.
type Apply = fn(&mut Envelope, &OptionValue) -> Result<(), CopyError>;

/// The optional options and where each one goes in the envelope.
const OPTIONAL: [(&str, Apply); 8] = [
    ("sender_qualifier", |envelope, value| {
        envelope.sender_qualifier = text("sender_qualifier", value)?;
        Ok(())
    }),
    ("receiver_qualifier", |envelope, value| {
        envelope.receiver_qualifier = text("receiver_qualifier", value)?;
        Ok(())
    }),
    ("usage_indicator", |envelope, value| {
        envelope.usage_indicator = text("usage_indicator", value)?;
        Ok(())
    }),
    ("control_number", |envelope, value| {
        envelope.control_number = control_number(value)?;
        Ok(())
    }),
    ("application_sender", |envelope, value| {
        envelope.application_sender = Some(text("application_sender", value)?);
        Ok(())
    }),
    ("application_receiver", |envelope, value| {
        envelope.application_receiver = Some(text("application_receiver", value)?);
        Ok(())
    }),
    ("delimiters", |envelope, value| {
        envelope.delimiters = delimiters(value)?;
        Ok(())
    }),
    ("line_break", |envelope, value| {
        envelope.line_break = line_break(value)?;
        Ok(())
    }),
];

/// The envelope of the required options, with the defaults of every other
/// field.
fn parties(given: &[(String, OptionValue)]) -> Result<Envelope, CopyError> {
    Ok(Envelope::with_ids(
        required_text(given, "sender_id")?,
        required_text(given, "receiver_id")?,
        required_date(given)?,
        required_time(given)?,
    ))
}

/// The envelope the options describe: the parties, then every optional
/// field that is given.
fn envelope(given: &[(String, OptionValue)]) -> Result<Envelope, CopyError> {
    let mut envelope = parties(given)?;
    for (name, apply) in OPTIONAL {
        if let Some(value) = lookup(given, name) {
            apply(&mut envelope, value)?;
        }
    }
    Ok(envelope)
}

/// A bare boolean option means true, as DuckDB's own options (HEADER) do.
fn line_break(value: &OptionValue) -> Result<bool, CopyError> {
    match value.kind {
        Kind::Null => Ok(true),
        Kind::Bool(flag) => Ok(flag),
        _ => Err(wrong("line_break", value, BOOLEAN)),
    }
}

/// The version option, the fallback built-in's version when it is not given.
fn version(given: &[(String, OptionValue)]) -> Result<String, CopyError> {
    match lookup(given, "version").map(|value| (&value.kind, value)) {
        None => Ok(Builtin::fallback().version.text().to_owned()),
        Some((Kind::Text(version), _)) => Ok(version.clone()),
        Some((Kind::Null, _)) => Err(CopyError::NoValue { name: "version" }),
        Some((_, value)) => Err(wrong("version", value, VERSION)),
    }
}

impl Settings {
    /// Reads the options, each name lower case.
    pub fn parse(given: &[(String, OptionValue)]) -> Result<Settings, CopyError> {
        reject_unknown(given)?;
        Ok(Settings {
            envelope: envelope(given)?,
            version: version(given)?,
        })
    }
}

fn control_number(value: &OptionValue) -> Result<u64, CopyError> {
    match value.kind {
        Kind::Integer(number) => {
            u64::try_from(number).map_err(|_| wrong("control_number", value, CONTROL_NUMBER))
        }
        Kind::Null => Err(CopyError::NoValue {
            name: "control_number",
        }),
        _ => Err(wrong("control_number", value, CONTROL_NUMBER)),
    }
}

/// Days since 1970-01-01 of a DATE or of `YYYY-MM-DD` text.
fn date(value: &OptionValue) -> Result<i32, CopyError> {
    match &value.kind {
        // DuckDB writes the infinite dates as the extremes of `i32`.
        Kind::Date(days) if *days != i32::MAX && *days != -i32::MAX => Ok(*days),
        Kind::Text(text) => parse_date(text).ok_or_else(|| wrong("date", value, DATE)),
        Kind::Null => Err(CopyError::NoValue { name: "date" }),
        _ => Err(wrong("date", value, DATE)),
    }
}

/// Seconds since midnight of a TIME, a TIME_NS or of `HH:MM`, `HHMM` or `HH:MM:SS`
/// text.
fn time(value: &OptionValue) -> Result<i32, CopyError> {
    match &value.kind {
        Kind::Time { value: ticks, unit } => clock(*ticks, *unit, &value.shown),
        Kind::Text(text) => parse_time(text).ok_or_else(|| wrong("time", value, TIME)),
        Kind::Null => Err(CopyError::NoValue { name: "time" }),
        _ => Err(wrong("time", value, TIME)),
    }
}

/// Seconds since midnight of a time of `ticks` counted in `unit`; `shown` is
/// the value as the caller wrote it.
fn clock(ticks: i64, unit: TimeUnit, shown: &str) -> Result<i32, CopyError> {
    let out_of_range = || CopyError::TimeOutOfRange {
        value: shown.to_owned(),
    };
    seconds_from(ticks, unit)
        .map_err(|error| match error {
            ConvertError::NotWholeSecond { .. } => CopyError::FractionalTime {
                value: shown.to_owned(),
            },
            _ => out_of_range(),
        })
        .and_then(|seconds| time_of_day(i64::from(seconds)).ok_or_else(out_of_range))
}

/// The delimiters of a STRUCT, with `oxedi.Delimiters`' defaults: `*`, `:`,
/// `~`, and no repetition or release byte.
fn delimiters(value: &OptionValue) -> Result<Delimiters, CopyError> {
    let fields = match &value.kind {
        Kind::Struct(fields) => fields,
        Kind::Null => return Err(CopyError::NoValue { name: "delimiters" }),
        _ => return Err(wrong("delimiters", value, DELIMITERS)),
    };
    let mut bytes: [Option<u8>; 5] = [Some(b'*'), Some(b':'), Some(b'~'), None, None];
    for (name, field) in fields {
        let Some(index) = DELIMITER_FIELDS.iter().position(|known| known == name) else {
            return Err(CopyError::UnknownDelimiter {
                name: name.clone(),
                known: DELIMITER_FIELDS,
            });
        };
        let given: &[u8] = match &field.kind {
            Kind::Text(text) => text.as_bytes(),
            Kind::Bytes(bytes) => bytes,
            // A NULL leaves the delimiter out, as `None` does in Python;
            // the writer refuses a missing one it needs.
            Kind::Null if index >= 3 => {
                if let Some(slot) = bytes.get_mut(index) {
                    *slot = None;
                }
                continue;
            }
            _ => {
                return Err(CopyError::OptionValue {
                    name: "delimiters",
                    value: format!("{} in the field {name:?}", field.shown),
                    expected: DELIMITER,
                });
            }
        };
        let [byte] = given else {
            return Err(CopyError::DelimiterLength {
                name: name.clone(),
                bytes: given.to_vec(),
            });
        };
        if let Some(slot) = bytes.get_mut(index) {
            *slot = Some(*byte);
        }
    }
    let [
        Some(element),
        Some(component),
        Some(segment),
        repetition,
        release,
    ] = bytes
    else {
        return Err(wrong("delimiters", value, DELIMITERS));
    };
    let mut delimiters = Delimiters::new(element, component, segment);
    delimiters.repetition = repetition;
    delimiters.release = release;
    Ok(delimiters)
}

/// A number of exactly `digits` ASCII digits.
fn number(text: &str, digits: usize) -> Option<i32> {
    (text.len() == digits && text.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// Days since 1970-01-01 of a valid `YYYY-MM-DD` date.
pub fn parse_date(text: &str) -> Option<i32> {
    let fields: Vec<&str> = text.split('-').collect();
    let [year, month, day] = fields.as_slice() else {
        return None;
    };
    days_from_civil(number(year, 4)?, number(month, 2)?, number(day, 2)?)
}

/// The hour, minute and second fields of `HH:MM`, `HHMM` or `HH:MM:SS`.
fn clock_fields(text: &str) -> Option<[&str; 3]> {
    let parts: Vec<&str> = text.split(':').collect();
    match parts.as_slice() {
        [hour_minute] => Some([hour_minute.get(..2)?, hour_minute.get(2..)?, "00"]),
        [hour, minute] => Some([hour, minute, "00"]),
        [hour, minute, second] => Some([hour, minute, second]),
        _ => None,
    }
}

/// Seconds since midnight of `HH:MM`, `HHMM` or `HH:MM:SS`.
pub fn parse_time(text: &str) -> Option<i32> {
    let [Some(hour), Some(minute), Some(second)] =
        clock_fields(text)?.map(|field| number(field, 2))
    else {
        return None;
    };
    seconds_of_day(hour, minute, second)
}
