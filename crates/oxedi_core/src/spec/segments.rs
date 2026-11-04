//! Segment and element definitions, and the compilation of a segment's elements.

use std::collections::BTreeMap;
use std::fmt;

use super::error::SpecError;
use super::raw::RawElement;
use crate::value::{ElementType, LengthRange, TypeCodeError};

/// One element of a segment, or one component of a composite element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementDef {
    /// Name used for the element downstream, e.g. `claim_submitter_id`.
    pub name: String,
    /// Data type.
    pub kind: ElementType,
    /// `true` when the element must be present and non-empty.
    pub required: bool,
    /// Minimum length, when the spec sets one.
    pub min: Option<usize>,
    /// Maximum length, when the spec sets one.
    pub max: Option<usize>,
    /// Components by 1-based position; empty for a simple element.
    pub composite: BTreeMap<usize, ElementDef>,
    /// The values the element may hold, sorted by their bytes; empty when
    /// any value of its type is accepted.
    pub codes: Vec<String>,
}

impl ElementDef {
    /// `true` when the element has a code list and `value` is not in it.
    pub(crate) fn rejects_code(&self, value: &[u8]) -> bool {
        rejects_code(&self.codes, value)
    }
}

/// `true` when `value` is one of `codes`, a list sorted by bytes.
#[inline]
pub(crate) fn holds_code(codes: &[String], value: &[u8]) -> bool {
    codes
        .binary_search_by(|code| code.as_bytes().cmp(value))
        .is_ok()
}

/// `true` when `codes` (sorted by bytes) is a code list and `value` is not
/// in it; an empty list rejects nothing.
pub(crate) fn rejects_code(codes: &[String], value: &[u8]) -> bool {
    !codes.is_empty() && !holds_code(codes, value)
}

/// The elements of one segment id.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SegmentDef {
    /// Elements by 1-based position. Positions with no entry are opaque.
    pub elements: BTreeMap<usize, ElementDef>,
}

/// Why an element definition was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementDefError {
    /// The position key is not a 1-based integer in canonical form.
    NonCanonicalPosition,
    /// `name` is the empty string.
    EmptyName,
    /// Another element at the same level already has this name.
    DuplicateName {
        /// The repeated name.
        name: String,
        /// Position key of the element that used it first, as written.
        first: String,
    },
    /// `type` is not one of the known codes.
    UnknownType {
        /// The code as written.
        found: String,
    },
    /// `scale` was given for a type other than `R`.
    ScaleWithoutR {
        /// The type code as written.
        kind: String,
    },
    /// `min` is greater than `max`.
    MinAboveMax {
        /// The minimum as written.
        min: usize,
        /// The maximum as written.
        max: usize,
    },
    /// `composite` was given for a type other than `AN`.
    CompositeOnNonAn {
        /// The type code as written.
        kind: String,
    },
    /// A component declares a `composite` of its own.
    NestedComposite,
    /// `scale` is above [`ElementType::MAX_SCALE`].
    ScaleAboveMaximum {
        /// The scale as written.
        scale: u8,
    },
    /// `max` is 0, so no value could ever be valid.
    ZeroMax,
    /// `codes` is an empty list.
    EmptyCodes,
    /// A code is the empty string.
    EmptyCode {
        /// The code's 0-based index in `codes` as written.
        index: usize,
    },
    /// A code's length is outside the element's `min` and `max`.
    CodeLength {
        /// The code's 0-based index in `codes` as written.
        index: usize,
        /// The code as written.
        code: String,
        /// Its length, counted as the element check counts it.
        length: usize,
        /// The element's minimum length.
        min: Option<usize>,
        /// The element's maximum length.
        max: Option<usize>,
    },
    /// A code is listed more than once.
    DuplicateCode {
        /// The code as written.
        code: String,
        /// 0-based index of its first listing.
        first: usize,
        /// 0-based index of its repeat.
        second: usize,
    },
    /// `codes` was given on an element that declares a `composite`, whose
    /// value is checked component by component.
    CodesOnComposite,
    /// `codes` was given on a numeric type (`N0` to `N9`, `R`), whose values
    /// are numbers: `1`, `01` and `1.0` are one value written three ways, so
    /// a list compared byte for byte would reject some of them.
    CodesOnNumeric {
        /// The type code as written.
        kind: String,
    },
}

impl From<TypeCodeError> for ElementDefError {
    fn from(error: TypeCodeError) -> ElementDefError {
        match error {
            TypeCodeError::UnknownType { found } => ElementDefError::UnknownType { found },
            TypeCodeError::ScaleWithoutR { kind } => ElementDefError::ScaleWithoutR { kind },
        }
    }
}

impl fmt::Display for ElementDefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElementDefError::NonCanonicalPosition => write!(
                f,
                "positions are 1-based integers written in canonical form"
            ),
            ElementDefError::EmptyName => write!(f, "\"name\" is empty"),
            ElementDefError::DuplicateName { name, first } => {
                write!(f, "name {name:?} is already used by position {first:?}")
            }
            ElementDefError::UnknownType { found } => TypeCodeError::write_unknown_type(f, found),
            ElementDefError::ScaleWithoutR { kind } => {
                TypeCodeError::write_scale_without_r(f, kind)
            }
            ElementDefError::MinAboveMax { min, max } => {
                write!(f, "\"min\" {min} is greater than \"max\" {max}")
            }
            ElementDefError::CompositeOnNonAn { kind } => {
                write!(f, "\"composite\" requires type AN; found type {kind:?}")
            }
            ElementDefError::NestedComposite => {
                write!(f, "a component cannot declare its own \"composite\"")
            }
            ElementDefError::ScaleAboveMaximum { scale } => write!(
                f,
                "\"scale\" {scale} is above the maximum of {}",
                ElementType::MAX_SCALE
            ),
            ElementDefError::ZeroMax => {
                write!(f, "\"max\" is 0; an element holds at least one character")
            }
            ElementDefError::EmptyCodes => write!(
                f,
                "\"codes\" is empty; leave the key out to accept any value"
            ),
            ElementDefError::EmptyCode { index } => {
                write!(f, "the code at codes[{index}] is empty")
            }
            ElementDefError::CodeLength {
                index,
                code,
                length,
                min,
                max,
            } => write!(
                f,
                "code {code:?} at codes[{index}] has length {length}; the element allows {}",
                LengthRange {
                    min: *min,
                    max: *max
                }
            ),
            ElementDefError::DuplicateCode {
                code,
                first,
                second,
            } => write!(
                f,
                "code {code:?} is listed twice, at codes[{first}] and codes[{second}]"
            ),
            ElementDefError::CodesOnComposite => write!(
                f,
                "\"codes\" applies to a simple element or a component; this element declares a \"composite\""
            ),
            ElementDefError::CodesOnNumeric { kind } => write!(
                f,
                "\"codes\" applies to non-numeric types; type {kind:?} holds numbers, which one value can write several ways (1, 01, 1.0)"
            ),
        }
    }
}

/// Name of the automatic column that numbers a table's rows from 0, across
/// the whole stream.
pub const ROW_COLUMN: &str = "row";

/// Name of the automatic column that holds the index of a row's anchor
/// segment: the segment that opened the anchor loop instance, or the
/// anchored segment itself.
pub const SEGMENT_COLUMN: &str = "segment";

/// A 1-based element position written in canonical form (`"1"`, never
/// `"01"` or `"+1"`), so no two keys can name the same position.
pub(super) fn parse_position(key: &str) -> Option<usize> {
    key.parse::<usize>()
        .ok()
        .filter(|&p| p >= 1 && p.to_string() == key)
}

/// Compiles the elements of `segment`, or the components of the element at
/// `parent` (its key as written) when one is given. Each definition is read
/// one part at a time, in the order its errors are reported: position, name,
/// type, lengths, codes, composite.
pub(super) fn compile_elements(
    segment: &str,
    raw: &BTreeMap<String, RawElement>,
    parent: Option<&str>,
) -> Result<BTreeMap<usize, ElementDef>, SpecError> {
    let mut elements = BTreeMap::new();
    let mut names: BTreeMap<&str, &str> = BTreeMap::new();
    for (key, def) in raw {
        let fail = |reason| SpecError::BadElementDef {
            segment: segment.to_string(),
            position: position_text(parent, key),
            reason,
        };
        let position =
            parse_position(key).ok_or_else(|| fail(ElementDefError::NonCanonicalPosition))?;
        claim_name(&mut names, &def.name, key).map_err(fail)?;
        let element = compile_element(def, parent.is_some()).map_err(fail)?;
        // A simple element has no components, which compile to an empty map.
        let composite = compile_elements(segment, &def.composite, Some(key))?;
        elements.insert(
            position,
            ElementDef {
                composite,
                ..element
            },
        );
    }
    Ok(elements)
}

/// The position of an element as an error names it: its key, or
/// `<parent>.composite.<key>` for a component.
fn position_text(parent: Option<&str>, key: &str) -> String {
    match parent {
        Some(parent) => format!("{parent}.composite.{key}"),
        None => key.to_string(),
    }
}

/// Records `name` for the element at `key`; refused when it is empty or
/// another element at the same level already has it.
fn claim_name<'r>(
    names: &mut BTreeMap<&'r str, &'r str>,
    name: &'r str,
    key: &'r str,
) -> Result<(), ElementDefError> {
    if name.is_empty() {
        return Err(ElementDefError::EmptyName);
    }
    match names.insert(name, key) {
        None => Ok(()),
        Some(first) => Err(ElementDefError::DuplicateName {
            name: name.to_string(),
            first: first.to_string(),
        }),
    }
}

/// Compiles one definition but its components: its type, lengths and codes,
/// and whether it may declare a composite (`component` when it is one).
fn compile_element(def: &RawElement, component: bool) -> Result<ElementDef, ElementDefError> {
    let kind = element_type(&def.kind, def.scale)?;
    check_lengths(def.min, def.max)?;
    let composite = !def.composite.is_empty();
    let codes = match &def.codes {
        None => Vec::new(),
        Some(codes) => compile_codes(codes, kind, def.min, def.max, composite)?,
    };
    if composite {
        composite_allowed(kind, component)?;
    }
    Ok(ElementDef {
        name: def.name.clone(),
        kind,
        required: def.required,
        min: def.min,
        max: def.max,
        composite: BTreeMap::new(),
        codes,
    })
}

/// Reads a `type` and its `scale`; an `R` scale must not pass
/// [`ElementType::MAX_SCALE`].
fn element_type(code: &str, scale: Option<u8>) -> Result<ElementType, ElementDefError> {
    match ElementType::parse(code, scale)? {
        ElementType::R { scale } if scale > ElementType::MAX_SCALE => {
            Err(ElementDefError::ScaleAboveMaximum { scale })
        }
        kind => Ok(kind),
    }
}

/// Checks `min` and `max`: a `max` of 0 admits no value, and `min` may not
/// pass `max`.
fn check_lengths(min: Option<usize>, max: Option<usize>) -> Result<(), ElementDefError> {
    match (min, max) {
        (_, Some(0)) => Err(ElementDefError::ZeroMax),
        (Some(min), Some(max)) if min > max => Err(ElementDefError::MinAboveMax { min, max }),
        _ => Ok(()),
    }
}

/// Whether an element of type `kind` may declare a composite: only an `AN`
/// element that is not itself a component.
fn composite_allowed(kind: ElementType, component: bool) -> Result<(), ElementDefError> {
    if component {
        Err(ElementDefError::NestedComposite)
    } else if kind != ElementType::An {
        Err(ElementDefError::CompositeOnNonAn { kind: kind.code() })
    } else {
        Ok(())
    }
}

/// Validates a code list against its element (of type `kind`, with lengths
/// `min` and `max`, declaring a composite when `composite`) and returns it
/// sorted by bytes, the order the element check searches.
pub(super) fn compile_codes(
    codes: &[String],
    kind: ElementType,
    min: Option<usize>,
    max: Option<usize>,
    composite: bool,
) -> Result<Vec<String>, ElementDefError> {
    codes_apply(kind, composite)?;
    if codes.is_empty() {
        return Err(ElementDefError::EmptyCodes);
    }
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, code) in codes.iter().enumerate() {
        if code.is_empty() {
            return Err(ElementDefError::EmptyCode { index });
        }
        if !kind.length_in_range(code.as_bytes(), LengthRange { min, max }) {
            return Err(ElementDefError::CodeLength {
                index,
                code: code.clone(),
                length: kind.length_of(code.as_bytes()),
                min,
                max,
            });
        }
        if let Some(first) = seen.insert(code.as_str(), index) {
            return Err(ElementDefError::DuplicateCode {
                code: code.clone(),
                first,
                second: index,
            });
        }
    }
    Ok(seen.into_keys().map(str::to_string).collect())
}

/// Whether a code list applies to an element of type `kind` that declares a
/// composite when `composite`: only to a simple element or a component, of
/// a type that is not numeric.
fn codes_apply(kind: ElementType, composite: bool) -> Result<(), ElementDefError> {
    if composite {
        Err(ElementDefError::CodesOnComposite)
    } else if kind.is_numeric() {
        Err(ElementDefError::CodesOnNumeric { kind: kind.code() })
    } else {
        Ok(())
    }
}
