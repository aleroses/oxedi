//! Segment and element definitions.

use super::{CLP_ONLY, element_error};
use crate::spec::raw::RawSegment;
use crate::spec::*;
use crate::value::ElementType;

#[test]
fn an_empty_segment_id_in_the_segments_section_is_rejected() {
    let err = Spec::from_json(
        r#"{"name":"t","loops":{"a":{"trigger":{"segment":"AA"}}},"segments":{"":{"elements":{}}}}"#,
    )
    .unwrap_err();
    assert!(
        matches!(&err, SpecError::EmptySegmentId { loop_name: None, key } if key == "segments.\"\""),
        "{err:?}"
    );
}

#[test]
fn non_numeric_zero_or_non_canonical_positions_are_rejected() {
    for position in ["x", "0", "-1", "01", "+1", " 1"] {
        let json = format!(
            r#"{{"name":"t","loops":{{"a":{{"trigger":{{"segment":"AA","where":{{"{position}":"1"}}}}}}}}}}"#
        );
        let err = Spec::from_json(&json).unwrap_err();
        assert!(
            matches!(&err, SpecError::BadPosition { loop_name, position: p } if loop_name == "a" && p == position),
            "{err}"
        );
    }
}

#[test]
fn a_position_spelled_twice_is_rejected_by_its_non_canonical_spelling() {
    let err = Spec::from_json(
        r#"{"name":"t","loops":{"a":{"trigger":{"segment":"AA","where":{"1":"X","01":"Y"}}}}}"#,
    )
    .unwrap_err();
    assert!(
        matches!(&err, SpecError::BadPosition { loop_name, position } if loop_name == "a" && position == "01"),
        "{err:?}"
    );
}

#[test]
fn ambiguous_trigger_message_shows_the_parent_and_the_trigger() {
    let err = Spec::from_json(
        r#"{"name":"t","loops":{
                "transaction":{"trigger":{"segment":"ST"}},
                "a":{"parent":"transaction","trigger":{"segment":"N1","where":{"1":"PR"}}},
                "b":{"parent":"transaction","trigger":{"segment":"N1","where":{"1":"PR"}}}
            }}"#,
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "loops \"a\" and \"b\" under \"transaction\" share the identical trigger \"N1\" where {1: \"PR\"}"
    );
    let err = Spec::from_json(
        r#"{"name":"t","loops":{
                "a":{"trigger":{"segment":"AA"}},
                "b":{"trigger":{"segment":"AA"}}
            }}"#,
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "loops \"a\" and \"b\" under the root share the identical trigger \"AA\""
    );
}

#[test]
fn segments_are_keyed_by_id_and_elements_by_position() {
    let spec = Spec::from_json(CLP_ONLY).unwrap();
    let clp = spec.segment(b"CLP").unwrap();
    let first = &clp.elements[&1];
    assert_eq!(first.name, "claim_submitter_id");
    assert_eq!(first.kind, ElementType::An);
    assert!(first.required);
    assert_eq!((first.min, first.max), (Some(1), Some(38)));
    assert_eq!(clp.elements[&3].kind, ElementType::R { scale: 2 });
    assert!(!clp.elements[&12].required, "required defaults to false");
    assert_eq!(clp.elements[&12].kind, ElementType::R { scale: 4 });
    assert_eq!(
        clp.elements.keys().copied().collect::<Vec<_>>(),
        vec![1, 3, 12]
    );
    let svc = spec.segment(b"SVC").unwrap();
    let procedure = &svc.elements[&1];
    assert_eq!(procedure.composite[&1].kind, ElementType::Id);
    assert_eq!(procedure.composite[&2].name, "code");
    assert_eq!(svc.elements[&5].kind, ElementType::N(0));
    let ids: Vec<&[u8]> = spec.segments().map(|(id, _)| id).collect();
    assert_eq!(ids, vec![&b"CLP"[..], &b"SVC"[..]]);
}

#[test]
fn every_type_code_is_read() {
    let cases = [
        ("AN", None, ElementType::An),
        ("ID", None, ElementType::Id),
        ("N0", None, ElementType::N(0)),
        ("N2", None, ElementType::N(2)),
        ("N9", None, ElementType::N(9)),
        ("R", None, ElementType::R { scale: 2 }),
        ("R", Some(6), ElementType::R { scale: 6 }),
        ("DT", None, ElementType::Dt),
        ("TM", None, ElementType::Tm),
    ];
    for (code, scale, expected) in cases {
        assert_eq!(ElementType::parse(code, scale), Ok(expected), "{code}");
    }
    for code in ["an", "N", "N10", "NA", "R2", "", "B"] {
        assert_eq!(
            ElementType::parse(code, None).map_err(ElementDefError::from),
            Err(ElementDefError::UnknownType {
                found: code.to_string()
            }),
            "{code:?}"
        );
    }
}

#[test]
fn element_types_display_their_code_and_meaning() {
    assert_eq!(ElementType::An.to_string(), "AN (string)");
    assert_eq!(ElementType::Id.to_string(), "ID (code)");
    assert_eq!(
        ElementType::N(2).to_string(),
        "N2 (integer with 2 implied decimals)"
    );
    assert_eq!(
        ElementType::R { scale: 2 }.to_string(),
        "R (decimal, scale 2)"
    );
    assert_eq!(ElementType::Dt.to_string(), "DT (date CCYYMMDD or YYMMDD)");
    assert_eq!(
        ElementType::Tm.to_string(),
        "TM (time HHMM, HHMMSS or HHMMSSD..)"
    );
}

#[test]
fn a_segment_a_loop_lists_without_a_definition_stays_opaque() {
    let spec = Spec::from_json(CLP_ONLY).unwrap();
    assert!(spec.get(spec.loop_id("2100").unwrap()).accepts(b"ZZ1"));
    assert_eq!(spec.segment(b"ZZ1"), None);
}

#[test]
fn a_spec_without_segments_has_none() {
    let spec =
        Spec::from_json(r#"{"name":"t","loops":{"a":{"trigger":{"segment":"AA"}}}}"#).unwrap();
    assert_eq!(spec.segments().count(), 0);
}

#[test]
fn bad_element_definitions_are_rejected_with_segment_position_and_reason() {
    let cases = [
        (
            r#"{"01":{"name":"a","type":"AN"}}"#,
            "01",
            ElementDefError::NonCanonicalPosition,
        ),
        (
            r#"{"0":{"name":"a","type":"AN"}}"#,
            "0",
            ElementDefError::NonCanonicalPosition,
        ),
        (
            r#"{"1":{"name":"","type":"AN"}}"#,
            "1",
            ElementDefError::EmptyName,
        ),
        (
            r#"{"1":{"name":"a","type":"AN"},"2":{"name":"a","type":"ID"}}"#,
            "2",
            ElementDefError::DuplicateName {
                name: "a".into(),
                first: "1".into(),
            },
        ),
        (
            r#"{"1":{"name":"a","type":"XX"}}"#,
            "1",
            ElementDefError::UnknownType { found: "XX".into() },
        ),
        (
            r#"{"1":{"name":"a","type":"N2","scale":2}}"#,
            "1",
            ElementDefError::ScaleWithoutR { kind: "N2".into() },
        ),
        (
            r#"{"1":{"name":"a","type":"AN","min":5,"max":2}}"#,
            "1",
            ElementDefError::MinAboveMax { min: 5, max: 2 },
        ),
        (
            r#"{"1":{"name":"a","type":"ID","composite":{"1":{"name":"b","type":"AN"}}}}"#,
            "1",
            ElementDefError::CompositeOnNonAn { kind: "ID".into() },
        ),
        (
            r#"{"1":{"name":"a","type":"AN","composite":{"2":{"name":"b","type":"AN","composite":{"1":{"name":"c","type":"AN"}}}}}}"#,
            "1.composite.2",
            ElementDefError::NestedComposite,
        ),
        (
            r#"{"1":{"name":"a","type":"AN","composite":{"1":{"name":"b","type":"AN"},"2":{"name":"b","type":"AN"}}}}"#,
            "1.composite.2",
            ElementDefError::DuplicateName {
                name: "b".into(),
                first: "1".into(),
            },
        ),
    ];
    for (elements, expected_position, expected_reason) in cases {
        let err = element_error(elements);
        assert!(
            matches!(&err, SpecError::BadElementDef { segment, position, reason } if segment == "AA" && position == expected_position && *reason == expected_reason),
            "{elements}: {err:?}"
        );
    }
}

#[test]
fn names_only_need_to_be_unique_among_siblings() {
    let ok = Spec::from_json(
        r#"{"name":"t","loops":{"a":{"trigger":{"segment":"AA"}}},"segments":{"AA":{"elements":{
                "1":{"name":"code","type":"AN","composite":{"1":{"name":"code","type":"AN"}}}
            }}}}"#,
    );
    assert!(ok.is_ok(), "{ok:?}");
}

#[test]
fn a_segment_key_the_schema_does_not_define_is_named_with_its_path() {
    let cases = [
        (
            r#"{"AA":{"elements":{"3":{"name":"a","type":"AN","lenght":3}}}}"#,
            "spec: unknown key \"lenght\" at segments.AA.elements.3; the allowed keys are: codes, composite, max, min, name, required, scale, type",
        ),
        (
            r#"{"AA":{"elemnts":{}}}"#,
            "spec: unknown key \"elemnts\" at segments.AA; the allowed keys are: elements",
        ),
        (
            r#"{"AA":{"elements":{"1":{"name":"c","type":"AN","composite":{"2":{"name":"x","typ":"AN"}}}}}}"#,
            "spec: unknown key \"typ\" at segments.AA.elements.1.composite.2; the allowed keys are: codes, composite, max, min, name, required, scale, type",
        ),
        (
            r#"{"AA":{"elements":{"3":{"name":"a"}}}}"#,
            "spec: missing required key \"type\" at segments.AA.elements.3",
        ),
        (
            r#"{"AA":{"elements":{"1":{"name":"c","type":"AN","composite":{"2":{"type":"AN"}}}}}}"#,
            "spec: missing required key \"name\" at segments.AA.elements.1.composite.2",
        ),
    ];
    for (segments, expected) in cases {
        let json = format!(
            r#"{{"name":"t","loops":{{"a":{{"trigger":{{"segment":"AA"}}}}}},"segments":{segments}}}"#
        );
        let err = Spec::from_json(&json).unwrap_err();
        assert!(
            matches!(
                &err,
                SpecError::UnknownKey { .. } | SpecError::MissingKey { .. }
            ),
            "{segments}: {err:?}"
        );
        assert_eq!(err.to_string(), expected, "{segments}");
    }
}

#[test]
fn every_object_of_the_segment_schema_is_checked_with_its_path() {
    let cases = [
        (r#"[]"#, "segments", "an array"),
        (r#"{"CLP":[]}"#, "segments.CLP", "an array"),
        (
            r#"{"CLP":{"elements":[]}}"#,
            "segments.CLP.elements",
            "an array",
        ),
        (
            r#"{"CLP":{"elements":{"1":"claim_id"}}}"#,
            "segments.CLP.elements.1",
            "a string",
        ),
        (
            r#"{"SVC":{"elements":{"1":{"name":"p","type":"AN","composite":{"2":7}}}}}"#,
            "segments.SVC.elements.1.composite.2",
            "a number",
        ),
    ];
    for (segments, expected_path, expected_found) in cases {
        let json = format!(
            r#"{{"name":"t","loops":{{"a":{{"trigger":{{"segment":"AA"}}}}}},"segments":{segments}}}"#
        );
        let err = Spec::from_json(&json).unwrap_err();
        assert!(
            matches!(&err, SpecError::NotAnObject { path, found } if path == expected_path && *found == expected_found),
            "{segments}: {err:?}"
        );
    }
}

#[test]
fn to_json_round_trips_the_segments_section() {
    let spec = Spec::from_json(CLP_ONLY).unwrap();
    let again = Spec::from_json(&spec.to_json()).unwrap();
    assert!(spec.segments().eq(again.segments()));
    assert_eq!(again.segments().count(), 2);
}

#[test]
fn segment_schema_error_displays_the_segment_and_the_serde_message() {
    let err = SpecError::SegmentSchema {
        segment: "CLP".into(),
        source: serde_json::from_value::<RawSegment>(serde_json::json!(1)).unwrap_err(),
    };
    assert_eq!(
        err.to_string(),
        "segment \"CLP\" does not match the schema: invalid type: integer `1`, expected a segment object"
    );
    assert!(std::error::Error::source(&err).is_none());
}
