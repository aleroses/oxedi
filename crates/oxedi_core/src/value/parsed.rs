//! An element's text read as a value of a column type.

use super::{ColumnType, parse_dt, parse_n, parse_r, parse_tm};

/// The value a text holds as a column type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parsed {
    /// Text: the bytes are the value, read from where they are.
    Text,
    /// An `N` value as written; the column's scale gives its decimals.
    Int(i64),
    /// An `R` value scaled by `10^scale` of the column.
    Decimal(i128),
    /// A `DT` value as days since 1970-01-01.
    Date(i32),
    /// A `TM` value as seconds since midnight.
    Time(i32),
}

impl ColumnType {
    /// `text` as a value of this column type, with the parser of its
    /// element type ([`parse_n`], [`parse_r`], [`parse_dt`], [`parse_tm`]);
    /// any text is a `Binary` value. `None` for an empty text, which is no
    /// value, and for a text the parser refuses.
    pub fn parse(self, text: &[u8]) -> Option<Parsed> {
        if text.is_empty() {
            return None;
        }
        match self {
            ColumnType::Binary => Some(Parsed::Text),
            ColumnType::Int64 { .. } => parse_n(text).map(Parsed::Int),
            ColumnType::Decimal128 { scale, .. } => parse_r(text, scale).map(Parsed::Decimal),
            ColumnType::Date32 => parse_dt(text).map(Parsed::Date),
            ColumnType::Time32 => parse_tm(text).map(Parsed::Time),
        }
    }
}
