//! The version a spec declares it covers, and the choice of a spec by it.

use std::fmt;

use super::Spec;
use super::error::SpecError;
use super::raw::RawVersion;
use crate::element::Element;
use crate::segment::Segment;

/// The element of the first segment with a given id whose value says which
/// version of the format a document follows, and the values a spec covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredVersion {
    /// Id of the segment that carries the version.
    pub segment: Vec<u8>,
    /// 1-based position of the element that holds it.
    pub element: usize,
    /// The values this spec covers, in the order written.
    pub values: Vec<Vec<u8>>,
}

impl DeclaredVersion {
    /// Whether `segment` settles this declaration: `None` when it is not the
    /// segment the declaration reads, otherwise whether its element holds one
    /// of the values. A composite element holds none of them.
    pub fn matches(&self, segment: &Segment<'_>) -> Option<bool> {
        if segment.id != self.segment.as_slice() {
            return None;
        }
        Some(match segment.element(self.element) {
            Some(Element::Simple(value)) => self.values.iter().any(|v| v == value.as_ref()),
            _ => false,
        })
    }
}

/// Why a spec's `version` was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum VersionError {
    /// `element` is 0.
    ZeroElement,
    /// `values` is an empty list.
    NoValues,
    /// A value is the empty string.
    EmptyValue {
        /// The value's 0-based index in `values` as written.
        index: usize,
    },
    /// A value is listed more than once.
    DuplicateValue {
        /// The value as written.
        value: String,
        /// 0-based index of its first listing.
        first: usize,
        /// 0-based index of its repeat.
        second: usize,
    },
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VersionError::ZeroElement => write!(f, "\"element\" is 0; positions are 1-based"),
            VersionError::NoValues => write!(
                f,
                "\"values\" is empty; list at least one value the element may hold"
            ),
            VersionError::EmptyValue { index } => write!(f, "\"values[{index}]\" is empty"),
            VersionError::DuplicateValue {
                value,
                first,
                second,
            } => write!(
                f,
                "value {value:?} is listed twice, at values[{first}] and values[{second}]"
            ),
        }
    }
}

/// Validates a spec's `version`.
pub(super) fn compile_version(raw: &RawVersion) -> Result<DeclaredVersion, SpecError> {
    if raw.segment.is_empty() {
        return Err(SpecError::EmptySegmentId {
            loop_name: None,
            key: "version.segment".into(),
        });
    }
    let bad = |reason| SpecError::BadVersion { reason };
    if raw.element == 0 {
        return Err(bad(VersionError::ZeroElement));
    }
    if raw.values.is_empty() {
        return Err(bad(VersionError::NoValues));
    }
    if let Some(index) = raw.values.iter().position(String::is_empty) {
        return Err(bad(VersionError::EmptyValue { index }));
    }
    let mut seen = std::collections::HashMap::with_capacity(raw.values.len());
    for (second, value) in raw.values.iter().enumerate() {
        if let Some(first) = seen.insert(value.as_str(), second) {
            return Err(bad(VersionError::DuplicateValue {
                value: value.clone(),
                first,
                second,
            }));
        }
    }
    Ok(DeclaredVersion {
        segment: raw.segment.as_bytes().to_vec(),
        element: raw.element,
        values: raw.values.iter().map(|v| v.as_bytes().to_vec()).collect(),
    })
}

/// How far a candidate's declaration has been read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reading {
    Open,
    Matched,
    Missed,
}

impl Reading {
    /// Where a candidate starts: open when it declares a version.
    fn start(spec: &Spec) -> Reading {
        match spec.version {
            Some(_) => Reading::Open,
            None => Reading::Missed,
        }
    }

    /// The reading after `segment`: an open candidate settles when the
    /// segment is the one its declaration names.
    fn after(self, spec: &Spec, segment: &Segment<'_>) -> Reading {
        let found = spec.version.as_ref().and_then(|v| v.matches(segment));
        match (self, found) {
            (Reading::Open, Some(true)) => Reading::Matched,
            (Reading::Open, Some(false)) => Reading::Missed,
            (reading, _) => reading,
        }
    }
}

/// The choice once every candidate before the first unmissed one has
/// missed: that one wins if it matched (`Some(Some(at))`), and none left
/// means no choice (`Some(None)`); `None` while it is still open.
fn settled(readings: &[Reading]) -> Option<Option<usize>> {
    match readings
        .iter()
        .enumerate()
        .find(|(_, reading)| **reading != Reading::Missed)
    {
        None => Some(None),
        Some((at, Reading::Matched)) => Some(Some(at)),
        Some(_) => None,
    }
}

/// The position of the first of `candidates` whose declared version the
/// `segments` carry; `None` when none does. Segments are read only until the
/// choice is settled.
pub(super) fn chosen<'s>(
    candidates: &[&Spec],
    segments: impl IntoIterator<Item = Segment<'s>>,
) -> Option<usize> {
    let mut readings: Vec<Reading> = candidates.iter().map(|spec| Reading::start(spec)).collect();
    let mut segments = segments.into_iter();
    loop {
        if let Some(choice) = settled(&readings) {
            return choice;
        }
        let Some(segment) = segments.next() else {
            return readings.iter().position(|&r| r == Reading::Matched);
        };
        for (reading, spec) in readings.iter_mut().zip(candidates) {
            *reading = reading.after(spec, &segment);
        }
    }
}

impl Spec {
    /// The first of `candidates` whose declared version the document's
    /// `segments` carry, or `default` when none does.
    ///
    /// Each candidate is settled by the first segment with its declared
    /// segment id; a candidate that declares no version never matches.
    /// Segments are read only until the choice is settled, so for a
    /// declaration in the group header only the first few are read.
    pub fn select<'a, 's>(
        candidates: &[&'a Spec],
        default: &'a Spec,
        segments: impl IntoIterator<Item = Segment<'s>>,
    ) -> &'a Spec {
        chosen(candidates, segments)
            .and_then(|at| candidates.get(at).copied())
            .unwrap_or(default)
    }

    /// The version this spec declares it covers, if it declares one.
    pub fn version(&self) -> Option<&DeclaredVersion> {
        self.version.as_ref()
    }
}
