use super::super::{ColumnType, Parsed};

#[test]
fn parse_each_column_type() {
    let int = ColumnType::Int64 { scale: 0 };
    let decimal = ColumnType::Decimal128 {
        precision: 38,
        scale: 2,
    };
    assert_eq!(ColumnType::Binary.parse(b"x"), Some(Parsed::Text));
    assert_eq!(int.parse(b"-5"), Some(Parsed::Int(-5)));
    assert_eq!(decimal.parse(b"12.5"), Some(Parsed::Decimal(1250)));
    assert_eq!(
        ColumnType::Date32.parse(b"20240229"),
        Some(Parsed::Date(19_782))
    );
    assert_eq!(
        ColumnType::Time32.parse(b"235959"),
        Some(Parsed::Time(86_399))
    );
    assert_eq!(ColumnType::Binary.parse(b"x?"), Some(Parsed::Text));
    for kind in [int, decimal, ColumnType::Date32, ColumnType::Time32] {
        assert_eq!(kind.parse(b"x?"), None, "{kind}");
    }
}

#[test]
fn an_empty_text_is_no_value_of_any_type() {
    for kind in [
        ColumnType::Binary,
        ColumnType::Int64 { scale: 0 },
        ColumnType::Decimal128 {
            precision: 38,
            scale: 2,
        },
        ColumnType::Date32,
        ColumnType::Time32,
    ] {
        assert_eq!(kind.parse(b""), None, "{kind}");
    }
}
