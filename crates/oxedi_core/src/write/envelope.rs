//! The envelope a caller gives: who sends and receives, when, the first
//! control number and the delimiters; and the values of the envelope
//! segments the writer derives from it and from the spec.
//!
//! The envelope loops are the spec's loops with a `control`, outermost
//! first: an interchange, a functional group and a transaction set, the X12
//! envelope. Each element of an envelope trigger takes, in order: the
//! control number the loop's `control` places; the caller's field for that
//! element in the X12 envelope layout; the spec's declared version, for the
//! element that declares it; or the first code the spec lists for the
//! element. An element that allows a fixed width only is padded to it:
//! numbers with leading zeros, text with trailing spaces.

use crate::delimiters::Delimiters;
use crate::spec::{ElementDef, Spec};
use crate::value::{ElementTextError, ElementType};

use super::finding::WriteError;
use super::render::{envelope_date, envelope_time};

/// The caller's part of the envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    /// The sender id's qualifier, e.g. `ZZ`.
    pub sender_qualifier: String,
    /// The sender id; padded to the interchange's fixed width.
    pub sender_id: String,
    /// The receiver id's qualifier.
    pub receiver_qualifier: String,
    /// The receiver id; padded to the interchange's fixed width.
    pub receiver_id: String,
    /// The functional group's sender code; the sender id when `None`.
    pub application_sender: Option<String>,
    /// The functional group's receiver code; the receiver id when `None`.
    pub application_receiver: Option<String>,
    /// The date, as days since 1970-01-01.
    pub date: i32,
    /// The time, as seconds since midnight.
    pub time: i32,
    /// Whether the interchange is production or test data, e.g. `P` or `T`.
    pub usage_indicator: String,
    /// The interchange's and the group's control number, and the first
    /// transaction set's; each later transaction set takes the next one.
    pub control_number: u64,
    /// The delimiters to write with. The repetition separator is written
    /// where the interchange header carries one (its element has no code of
    /// its own); the release byte is never used.
    pub delimiters: Delimiters,
    /// Whether a line break follows each segment terminator.
    pub line_break: bool,
}

impl Envelope {
    /// The qualifier of a sender or receiver id when the caller gives none.
    pub const DEFAULT_QUALIFIER: &'static str = "ZZ";
    /// The usage indicator when the caller gives none: production data.
    pub const DEFAULT_USAGE_INDICATOR: &'static str = "P";
    /// The first control number when the caller gives none.
    pub const DEFAULT_CONTROL_NUMBER: u64 = 1;

    /// An envelope with both qualifiers `ZZ`, otherwise as [`Envelope::new`].
    pub fn with_ids(
        sender_id: impl Into<String>,
        receiver_id: impl Into<String>,
        date: i32,
        time: i32,
    ) -> Envelope {
        Envelope::new(
            Self::DEFAULT_QUALIFIER,
            sender_id,
            Self::DEFAULT_QUALIFIER,
            receiver_id,
            date,
            time,
        )
    }

    /// An envelope for production data with control number 1, the
    /// delimiters `*`, `:`, `~` and `^`, and no line breaks.
    pub fn new(
        sender_qualifier: impl Into<String>,
        sender_id: impl Into<String>,
        receiver_qualifier: impl Into<String>,
        receiver_id: impl Into<String>,
        date: i32,
        time: i32,
    ) -> Envelope {
        Envelope {
            sender_qualifier: sender_qualifier.into(),
            sender_id: sender_id.into(),
            receiver_qualifier: receiver_qualifier.into(),
            receiver_id: receiver_id.into(),
            application_sender: None,
            application_receiver: None,
            date,
            time,
            usage_indicator: Self::DEFAULT_USAGE_INDICATOR.to_string(),
            control_number: Self::DEFAULT_CONTROL_NUMBER,
            delimiters: Delimiters::new(b'*', b':', b'~').with_repetition(b'^'),
            line_break: false,
        }
    }
}

/// A caller's field, by the name a finding gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Field {
    SenderQualifier,
    SenderId,
    ReceiverQualifier,
    ReceiverId,
    ApplicationSender,
    ApplicationReceiver,
    Date,
    Time,
    UsageIndicator,
    ControlNumber,
    Repetition,
    Component,
    Version,
}

impl Field {
    pub(super) fn name(self) -> &'static str {
        match self {
            Field::SenderQualifier => "sender_qualifier",
            Field::SenderId => "sender_id",
            Field::ReceiverQualifier => "receiver_qualifier",
            Field::ReceiverId => "receiver_id",
            Field::ApplicationSender => "application_sender",
            Field::ApplicationReceiver => "application_receiver",
            Field::Date => "date",
            Field::Time => "time",
            Field::UsageIndicator => "usage_indicator",
            Field::ControlNumber => "control_number",
            Field::Repetition => "delimiters.repetition",
            Field::Component => "delimiters.component",
            Field::Version => "version",
        }
    }
}

/// The caller's fields in the X12 envelope layout, by depth among the
/// envelope loops (interchange, functional group, transaction set) and
/// element position. The transaction set header takes none: its optional
/// implementation convention reference is not written, since nothing in the
/// tables or the envelope carries it.
const LAYOUT: [&[(usize, Field)]; 3] = [
    &[
        (5, Field::SenderQualifier),
        (6, Field::SenderId),
        (7, Field::ReceiverQualifier),
        (8, Field::ReceiverId),
        (9, Field::Date),
        (10, Field::Time),
        (11, Field::Repetition),
        (15, Field::UsageIndicator),
        (16, Field::Component),
    ],
    &[
        (2, Field::ApplicationSender),
        (3, Field::ApplicationReceiver),
        (4, Field::Date),
        (5, Field::Time),
    ],
    &[],
];

/// The role of each delimiter, as messages name it.
pub(super) const ELEMENT: &str = "element separator";
pub(super) const COMPONENT: &str = "component separator";
pub(super) const SEGMENT: &str = "segment terminator";
pub(super) const REPETITION: &str = "repetition separator";

/// One value of an envelope trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EnvelopeValue {
    pub(super) element: usize,
    pub(super) bytes: Vec<u8>,
    /// The caller's field it comes from, when one does.
    pub(super) field: Option<Field>,
    /// `false` for a delimiter written as the value itself.
    pub(super) checked: bool,
    /// Why the field has no text the element can hold, when it has none.
    pub(super) refused: Option<ElementTextError>,
}

/// The delimiters the written file uses: those of the envelope, with the
/// repetition separator only when `repetition` is.
pub(super) fn delimiters(
    envelope: &Envelope,
    repetition: bool,
) -> Result<Vec<(u8, &'static str)>, WriteError> {
    let given = &envelope.delimiters;
    let mut all = vec![
        (given.element, ELEMENT),
        (given.component, COMPONENT),
        (given.segment, SEGMENT),
    ];
    if repetition {
        let Some(byte) = given.repetition else {
            return Err(WriteError::NoRepetition);
        };
        all.push((byte, REPETITION));
    }
    for (i, &(byte, role)) in all.iter().enumerate() {
        if byte.is_ascii_alphanumeric() || byte.is_ascii_whitespace() {
            return Err(WriteError::DelimiterNotAllowed { role, byte });
        }
        if let Some(&(_, first)) = all.iter().take(i).find(|(other, _)| *other == byte) {
            return Err(WriteError::SameDelimiter {
                first,
                second: role,
                byte,
            });
        }
    }
    Ok(all)
}

/// `true` when the interchange header carries a repetition separator: its
/// element in the layout has no code of its own in the spec.
pub(super) fn carries_repetition(spec: &Spec, trigger: &[u8]) -> bool {
    spec.element_def(trigger, 11, None)
        .is_some_and(|def| def.codes.is_empty())
}

/// The values of an envelope loop's trigger segment, at `depth` among the
/// envelope loops, with `control` the instance's control number.
pub(super) fn trigger_values(
    spec: &Spec,
    trigger: &[u8],
    depth: usize,
    opener_element: usize,
    control: &[u8],
    envelope: &Envelope,
) -> Vec<EnvelopeValue> {
    let mut values = Vec::new();
    let Some(segment) = spec.segment(trigger) else {
        return values;
    };
    let layout = LAYOUT.get(depth).copied().unwrap_or_default();
    let version = spec.version().and_then(|version| {
        (version.segment == trigger)
            .then_some(version)
            .map(|v| (v.element, v.values.first().cloned().unwrap_or_default()))
    });
    for (&position, def) in &segment.elements {
        let field = layout
            .iter()
            .find(|(at, _)| *at == position)
            .map(|(_, field)| *field);
        let mut refused = None;
        let (bytes, field, checked) = if position == opener_element {
            (control.to_vec(), Some(Field::ControlNumber), true)
        } else if let Some(field) = field {
            match field_value(field, def, envelope) {
                Some(Ok((bytes, checked))) => (bytes, Some(field), checked),
                Some(Err(reason)) => {
                    refused = Some(reason);
                    (Vec::new(), Some(field), true)
                }
                None => continue,
            }
        } else if let Some((_, value)) = version.as_ref().filter(|(at, _)| *at == position) {
            (value.clone(), Some(Field::Version), true)
        } else if let Some(code) = def.codes.first() {
            (code.as_bytes().to_vec(), None, true)
        } else {
            (Vec::new(), None, true)
        };
        values.push(EnvelopeValue {
            element: position,
            bytes: if refused.is_some() {
                bytes
            } else {
                pad(bytes, def)
            },
            field,
            checked,
            refused,
        });
    }
    values
}

/// The value of a caller's field for an element, and whether it is checked
/// for delimiters; `None` when the field has nothing to write there, the
/// reason when the field has no text the element can hold.
fn field_value(
    field: Field,
    def: &ElementDef,
    envelope: &Envelope,
) -> Option<Result<(Vec<u8>, bool), ElementTextError>> {
    let text = |value: &str| Some(Ok((value.as_bytes().to_vec(), true)));
    let delimiters = &envelope.delimiters;
    match field {
        Field::SenderQualifier => text(&envelope.sender_qualifier),
        Field::SenderId => text(&envelope.sender_id),
        Field::ReceiverQualifier => text(&envelope.receiver_qualifier),
        Field::ReceiverId => text(&envelope.receiver_id),
        Field::ApplicationSender => text(
            envelope
                .application_sender
                .as_deref()
                .unwrap_or(envelope.sender_id.trim_end()),
        ),
        Field::ApplicationReceiver => text(
            envelope
                .application_receiver
                .as_deref()
                .unwrap_or(envelope.receiver_id.trim_end()),
        ),
        Field::Date => Some(envelope_date(envelope.date, def.max).map(|text| (text, true))),
        Field::Time => Some(envelope_time(envelope.time, def.max).map(|text| (text, true))),
        Field::UsageIndicator => text(&envelope.usage_indicator),
        Field::Repetition => match def.codes.first() {
            Some(code) => Some(Ok((code.as_bytes().to_vec(), true))),
            None => delimiters.repetition.map(|byte| Ok((vec![byte], false))),
        },
        Field::Component => Some(Ok((vec![delimiters.component], false))),
        Field::Version | Field::ControlNumber => None,
    }
}

/// Pads a value to its element's width when the element allows one width
/// only, or when it is a number shorter than its minimum.
pub(super) fn pad(mut bytes: Vec<u8>, def: &ElementDef) -> Vec<u8> {
    let Some(min) = def.min else {
        return bytes;
    };
    let numeric = matches!(def.kind, ElementType::N(_));
    let fixed = def.max == Some(min);
    if bytes.len() >= min || !(numeric || fixed) {
        return bytes;
    }
    let missing = min - bytes.len();
    if numeric {
        let mut padded = vec![b'0'; missing];
        padded.append(&mut bytes);
        padded
    } else {
        bytes.resize(min, b' ');
        bytes
    }
}

/// A control number as its element writes it: digits padded with leading
/// zeros to the element's minimum.
pub(super) fn control_text(number: u64, def: Option<&ElementDef>) -> Vec<u8> {
    let digits = number.to_string().into_bytes();
    let min = def.and_then(|def| def.min).unwrap_or(0);
    if digits.len() >= min {
        return digits;
    }
    let mut padded = vec![b'0'; min - digits.len()];
    padded.extend_from_slice(&digits);
    padded
}
