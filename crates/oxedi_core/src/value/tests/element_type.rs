use super::super::{ElementType, LengthRange, TypeCodeError};
use std::error::Error;

#[test]
fn every_code_reads_back() {
    for t in [
        ElementType::An,
        ElementType::Id,
        ElementType::N(0),
        ElementType::N(9),
        ElementType::R { scale: 2 },
        ElementType::R { scale: 4 },
        ElementType::Dt,
        ElementType::Tm,
    ] {
        assert_eq!(ElementType::parse(&t.code(), t.scale()), Ok(t), "{t:?}");
    }
}

#[test]
fn r_keeps_its_scale() {
    let r = ElementType::R { scale: 4 };
    assert_eq!(r.code(), "R");
    assert_eq!(r.scale(), Some(4));
    assert_eq!(ElementType::An.scale(), None);
}

#[test]
fn type_code_errors_display_in_full() {
    let unknown = ElementType::parse("X1", None).unwrap_err();
    assert_eq!(unknown, TypeCodeError::UnknownType { found: "X1".into() });
    assert_eq!(
        unknown.to_string(),
        "type \"X1\" is not one of AN, ID, N0 to N9, R, DT, TM"
    );
    assert!(unknown.source().is_none());
    let scaled = ElementType::parse("AN", Some(2)).unwrap_err();
    assert_eq!(
        scaled.to_string(),
        "\"scale\" applies only to type R; found type \"AN\""
    );
    assert!(scaled.source().is_none());
}

#[test]
fn classification() {
    for text in [ElementType::An, ElementType::Id] {
        assert!(text.is_text() && !text.is_numeric(), "{text:?}");
    }
    for number in [ElementType::N(2), ElementType::R { scale: 2 }] {
        assert!(number.is_numeric() && !number.is_text(), "{number:?}");
    }
    for other in [ElementType::Dt, ElementType::Tm] {
        assert!(!other.is_text() && !other.is_numeric(), "{other:?}");
    }
}

#[test]
fn length_counts_digits_for_numbers() {
    let r = ElementType::R { scale: 2 };
    assert_eq!(r.length_of(b"-12.5"), 3);
    assert_eq!(ElementType::An.length_of(b"-12.5"), 5);
    let an = ElementType::An;
    let range = |min, max| LengthRange { min, max };
    assert!(!an.length_in_range(b"12345", range(Some(6), None)));
    assert!(an.length_in_range(b"123456", range(Some(6), Some(6))));
    assert!(!an.length_in_range(b"1234567", range(None, Some(6))));
    assert!(an.length_in_range(b"", range(None, None)));
    assert!(r.length_in_range(b"-12.5", range(Some(3), Some(3))));
    assert!(!r.length_in_range(b"-12.5", range(Some(4), None)));
    assert!(!r.length_in_range(b"-12.5", range(None, Some(2))));
}
