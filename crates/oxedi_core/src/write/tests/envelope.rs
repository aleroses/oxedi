use super::super::Envelope;

#[test]
fn with_ids_uses_the_default_qualifiers() {
    let envelope = Envelope::with_ids("SENDER", "RECEIVER", 19_782, 3_600);
    assert_eq!(envelope.sender_qualifier, "ZZ");
    assert_eq!(envelope.receiver_qualifier, "ZZ");
    assert_eq!(envelope.sender_id, "SENDER");
    assert_eq!(envelope.receiver_id, "RECEIVER");
    assert_eq!(envelope.usage_indicator, "P");
    assert_eq!(envelope.control_number, 1);
    assert_eq!((envelope.date, envelope.time), (19_782, 3_600));
}

#[test]
fn the_defaults_are_the_constants() {
    assert_eq!(Envelope::DEFAULT_QUALIFIER, "ZZ");
    assert_eq!(Envelope::DEFAULT_USAGE_INDICATOR, "P");
    assert_eq!(Envelope::DEFAULT_CONTROL_NUMBER, 1);
}
