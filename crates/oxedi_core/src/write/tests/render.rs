use crate::column::Cell;
use crate::value::{ColumnType, ElementTextError};

use super::super::render::cell_text;

#[test]
fn a_decimal_cell_takes_its_scale_from_a_decimal_column_only() {
    let mut out = Vec::new();
    let decimal = ColumnType::Decimal128 {
        precision: 38,
        scale: 2,
    };
    assert_eq!(
        cell_text(Cell::Decimal128(1250), decimal, || None, &mut out),
        Ok(true)
    );
    assert_eq!(out, b"12.5");
    let integer = ColumnType::Int64 { scale: 0 };
    assert_eq!(
        cell_text(Cell::Decimal128(1250), integer, || None, &mut out),
        Err(ElementTextError::NoDecimalScale { kind: integer })
    );
    assert_eq!(out, b"12.5");
}
