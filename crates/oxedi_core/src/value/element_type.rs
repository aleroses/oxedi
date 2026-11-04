//! The data type of an element as the X12 standard names it, the code a spec
//! writes for it and the questions the checks ask of it.

use std::fmt;

use super::convert::MAX_SCALE;
use super::length::LengthRange;

/// The data type of an element, as the X12 standard names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementType {
    /// `AN`: a string.
    An,
    /// `ID`: a code from a list.
    Id,
    /// `N0` to `N9`: an integer with that many implied decimal places.
    N(u8),
    /// `R`: a decimal number with an explicit point, kept at `scale` places.
    R {
        /// Decimal places; 2 unless the spec says otherwise.
        scale: u8,
    },
    /// `DT`: a date, `CCYYMMDD` or `YYMMDD`.
    Dt,
    /// `TM`: a time, `HHMM` optionally followed by seconds and decimal seconds.
    Tm,
}

/// Why a type code was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeCodeError {
    /// The code is not one of the known codes.
    UnknownType {
        /// The code as written.
        found: String,
    },
    /// A `scale` was given for a type other than `R`.
    ScaleWithoutR {
        /// The type code as written.
        kind: String,
    },
}

impl TypeCodeError {
    /// Writes the text of [`TypeCodeError::UnknownType`] for `found`, for the
    /// errors that carry the same rule.
    pub(crate) fn write_unknown_type(f: &mut fmt::Formatter<'_>, found: &str) -> fmt::Result {
        write!(
            f,
            "type {found:?} is not one of AN, ID, N0 to N9, R, DT, TM"
        )
    }

    /// Writes the text of [`TypeCodeError::ScaleWithoutR`] for `kind`, for the
    /// errors that carry the same rule.
    pub(crate) fn write_scale_without_r(f: &mut fmt::Formatter<'_>, kind: &str) -> fmt::Result {
        write!(f, "\"scale\" applies only to type R; found type {kind:?}")
    }
}

impl fmt::Display for TypeCodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeCodeError::UnknownType { found } => TypeCodeError::write_unknown_type(f, found),
            TypeCodeError::ScaleWithoutR { kind } => TypeCodeError::write_scale_without_r(f, kind),
        }
    }
}

impl std::error::Error for TypeCodeError {}

impl ElementType {
    /// Largest `scale` an `R` element may declare: [`crate::value::MAX_SCALE`].
    pub const MAX_SCALE: u8 = MAX_SCALE;

    /// Reads the type code of an element definition; `scale` is the
    /// definition's `scale` key, which only `R` accepts.
    pub fn parse(code: &str, scale: Option<u8>) -> Result<ElementType, TypeCodeError> {
        let kind = match code.as_bytes() {
            b"AN" => ElementType::An,
            b"ID" => ElementType::Id,
            b"R" => ElementType::R {
                scale: scale.unwrap_or(2),
            },
            b"DT" => ElementType::Dt,
            b"TM" => ElementType::Tm,
            [b'N', digit @ b'0'..=b'9'] => ElementType::N(digit - b'0'),
            _ => {
                return Err(TypeCodeError::UnknownType {
                    found: code.to_string(),
                });
            }
        };
        if scale.is_some() && !matches!(kind, ElementType::R { .. }) {
            return Err(TypeCodeError::ScaleWithoutR {
                kind: code.to_string(),
            });
        }
        Ok(kind)
    }

    /// The type code as a spec writes it. `R` is written `"R"`; its scale is
    /// the separate [`scale`](ElementType::scale).
    pub fn code(self) -> String {
        match self {
            ElementType::An => "AN".into(),
            ElementType::Id => "ID".into(),
            ElementType::N(places) => format!("N{places}"),
            ElementType::R { .. } => "R".into(),
            ElementType::Dt => "DT".into(),
            ElementType::Tm => "TM".into(),
        }
    }

    /// The `scale` a spec writes next to the code: `Some` only for `R`.
    pub const fn scale(self) -> Option<u8> {
        match self {
            ElementType::R { scale } => Some(scale),
            _ => None,
        }
    }

    /// `true` for the string types, `AN` and `ID`.
    pub const fn is_text(self) -> bool {
        matches!(self, ElementType::An | ElementType::Id)
    }

    /// `true` for the number types, `N0` to `N9` and `R`.
    pub const fn is_numeric(self) -> bool {
        matches!(self, ElementType::N(_) | ElementType::R { .. })
    }

    /// The length of a value as X12 counts it: numeric types count digits
    /// only (no sign, no point), every other type counts bytes.
    pub fn length_of(self, text: &[u8]) -> usize {
        if self.is_numeric() {
            text.iter().filter(|byte| byte.is_ascii_digit()).count()
        } else {
            text.len()
        }
    }

    /// `true` when the length of `text`, as [`length_of`](ElementType::length_of)
    /// counts it, is in `range`.
    pub fn length_in_range(self, text: &[u8], range: LengthRange) -> bool {
        range.contains(self.length_of(text))
    }
}

impl fmt::Display for ElementType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElementType::An => write!(f, "AN (string)"),
            ElementType::Id => write!(f, "ID (code)"),
            ElementType::N(places) => {
                write!(f, "N{places} (integer with {places} implied decimals)")
            }
            ElementType::R { scale } => write!(f, "R (decimal, scale {scale})"),
            ElementType::Dt => write!(f, "DT (date CCYYMMDD or YYMMDD)"),
            ElementType::Tm => write!(f, "TM (time HHMM, HHMMSS or HHMMSSD..)"),
        }
    }
}
