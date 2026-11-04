//! The physical type of a column and the element type each one holds.

use std::fmt;

use super::ElementType;

/// Precision of every `Decimal128` column: the most digits an `i128` holds in full.
pub const DECIMAL_PRECISION: u8 = 38;

/// The physical type of a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    /// Raw bytes, as they appear in the file.
    Binary,
    /// A 64-bit integer; `scale` implied decimal places (the `n` of `Nn`).
    Int64 {
        /// Implied decimal places.
        scale: u8,
    },
    /// A 128-bit integer scaled by `10^scale`.
    Decimal128 {
        /// Total significant digits.
        precision: u8,
        /// Decimal places.
        scale: u8,
    },
    /// Days since 1970-01-01.
    Date32,
    /// Seconds since midnight.
    Time32,
}

impl ColumnType {
    /// The column type that holds values of an element type; an element
    /// with no definition is held as raw bytes.
    pub fn of(kind: Option<ElementType>) -> ColumnType {
        match kind {
            None | Some(ElementType::An | ElementType::Id) => ColumnType::Binary,
            Some(ElementType::N(scale)) => ColumnType::Int64 { scale },
            Some(ElementType::R { scale }) => ColumnType::Decimal128 {
                precision: DECIMAL_PRECISION,
                scale,
            },
            Some(ElementType::Dt) => ColumnType::Date32,
            Some(ElementType::Tm) => ColumnType::Time32,
        }
    }

    /// The decimal places of a `Decimal128` column; `None` for every other
    /// type, an `Int64`'s implied places included.
    pub const fn decimal_scale(self) -> Option<u8> {
        match self {
            ColumnType::Decimal128 { scale, .. } => Some(scale),
            ColumnType::Binary
            | ColumnType::Int64 { .. }
            | ColumnType::Date32
            | ColumnType::Time32 => None,
        }
    }
}

impl fmt::Display for ColumnType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ColumnType::Binary => write!(f, "binary"),
            ColumnType::Int64 { scale: 0 } => write!(f, "int64"),
            ColumnType::Int64 { scale } => write!(f, "int64 (scale {scale})"),
            ColumnType::Decimal128 { precision, scale } => {
                write!(f, "decimal128({precision}, {scale})")
            }
            ColumnType::Date32 => write!(f, "date32"),
            ColumnType::Time32 => write!(f, "time32 (seconds)"),
        }
    }
}
