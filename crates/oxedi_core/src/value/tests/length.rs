use super::super::LengthRange;

#[test]
fn length_range_text_and_bounds() {
    let both = LengthRange {
        min: Some(2),
        max: Some(5),
    };
    let at_least = LengthRange {
        min: Some(2),
        max: None,
    };
    let at_most = LengthRange {
        min: None,
        max: Some(5),
    };
    let any = LengthRange::default();
    assert_eq!(both.to_string(), "2 to 5");
    assert_eq!(at_least.to_string(), "at least 2");
    assert_eq!(at_most.to_string(), "at most 5");
    assert_eq!(any.to_string(), "any length");

    assert!(!both.contains(1));
    assert!(both.contains(2));
    assert!(both.contains(5));
    assert!(!both.contains(6));
    assert!(!at_least.contains(1));
    assert!(at_least.contains(2));
    assert!(at_least.contains(usize::MAX));
    assert!(at_most.contains(0));
    assert!(at_most.contains(5));
    assert!(!at_most.contains(6));
    assert!(any.contains(0));
    assert!(any.contains(usize::MAX));
}
