//! Code lists on element and component definitions.

use super::element_error;
use crate::spec::*;

#[test]
fn codes_load_sorted_on_elements_and_components() {
    let spec = Spec::from_json(
        r#"{"name":"t","loops":{"a":{"trigger":{"segment":"AA"}}},"segments":{"AA":{"elements":{
            "1":{"name":"qualifier","type":"ID","min":2,"max":2,"codes":["PR","PE"]},
            "2":{"name":"free","type":"AN"},
            "3":{"name":"pair","type":"AN","composite":{
                "1":{"name":"kind","type":"ID","codes":["HC","AD","N4"]}
            }},
            "4":{"name":"stamp","type":"TM","min":4,"max":4,"codes":["1200","0000"]}
        }}}}"#,
    )
    .unwrap();
    let codes = |element, component| {
        spec.element_def(b"AA", element, component)
            .unwrap()
            .codes
            .clone()
    };
    assert_eq!(codes(1, None), vec!["PE", "PR"]);
    assert!(codes(2, None).is_empty());
    assert_eq!(codes(3, Some(1)), vec!["AD", "HC", "N4"]);
    assert_eq!(codes(4, None), vec!["0000", "1200"]);
}

#[test]
fn every_code_list_fault_names_the_position_and_the_code() {
    let cases = [
        (
            r#"{"1":{"name":"a","type":"ID","codes":[]}}"#,
            "1",
            ElementDefError::EmptyCodes,
        ),
        (
            r#"{"1":{"name":"a","type":"ID","codes":["A",""]}}"#,
            "1",
            ElementDefError::EmptyCode { index: 1 },
        ),
        (
            r#"{"1":{"name":"a","type":"ID","min":2,"max":3,"codes":["AB","ABCD"]}}"#,
            "1",
            ElementDefError::CodeLength {
                index: 1,
                code: "ABCD".into(),
                length: 4,
                min: Some(2),
                max: Some(3),
            },
        ),
        (
            r#"{"1":{"name":"a","type":"ID","min":2,"codes":["A"]}}"#,
            "1",
            ElementDefError::CodeLength {
                index: 0,
                code: "A".into(),
                length: 1,
                min: Some(2),
                max: None,
            },
        ),
        (
            r#"{"1":{"name":"a","type":"N0","max":2,"codes":["1"]}}"#,
            "1",
            ElementDefError::CodesOnNumeric { kind: "N0".into() },
        ),
        (
            r#"{"1":{"name":"a","type":"AN","composite":{"3":{"name":"b","type":"R","codes":["1.0"]}}}}"#,
            "1.composite.3",
            ElementDefError::CodesOnNumeric { kind: "R".into() },
        ),
        (
            r#"{"1":{"name":"a","type":"ID","codes":["AB","CD","AB"]}}"#,
            "1",
            ElementDefError::DuplicateCode {
                code: "AB".into(),
                first: 0,
                second: 2,
            },
        ),
        (
            r#"{"1":{"name":"a","type":"AN","codes":["X"],"composite":{"1":{"name":"b","type":"AN"}}}}"#,
            "1",
            ElementDefError::CodesOnComposite,
        ),
        (
            r#"{"1":{"name":"a","type":"AN","composite":{"2":{"name":"b","type":"ID","codes":[]}}}}"#,
            "1.composite.2",
            ElementDefError::EmptyCodes,
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
fn code_list_faults_display_segment_position_and_code() {
    let at = |position: &str, reason| {
        SpecError::BadElementDef {
            segment: "N1".into(),
            position: position.into(),
            reason,
        }
        .to_string()
    };
    assert_eq!(
        at("1", ElementDefError::EmptyCodes),
        "segment \"N1\" element \"1\": \"codes\" is empty; leave the key out to accept any value"
    );
    assert_eq!(
        at("1", ElementDefError::EmptyCode { index: 3 }),
        "segment \"N1\" element \"1\": the code at codes[3] is empty"
    );
    let length = |min, max| ElementDefError::CodeLength {
        index: 1,
        code: "ABCD".into(),
        length: 4,
        min,
        max,
    };
    assert_eq!(
        at("1", length(Some(2), Some(3))),
        "segment \"N1\" element \"1\": code \"ABCD\" at codes[1] has length 4; the element allows 2 to 3"
    );
    assert!(at("1", length(Some(5), None)).ends_with("the element allows at least 5"));
    assert!(at("1", length(None, Some(3))).ends_with("the element allows at most 3"));
    assert_eq!(
        at(
            "1.composite.2",
            ElementDefError::DuplicateCode {
                code: "PR".into(),
                first: 0,
                second: 2,
            }
        ),
        "segment \"N1\" element \"1.composite.2\": code \"PR\" is listed twice, at codes[0] and codes[2]"
    );
    assert_eq!(
        at("4", ElementDefError::CodesOnComposite),
        "segment \"N1\" element \"4\": \"codes\" applies to a simple element or a component; this element declares a \"composite\""
    );
    assert_eq!(
        at("2", ElementDefError::CodesOnNumeric { kind: "N0".into() }),
        "segment \"N1\" element \"2\": \"codes\" applies to non-numeric types; type \"N0\" holds numbers, which one value can write several ways (1, 01, 1.0)"
    );
}

#[test]
fn a_code_list_of_the_wrong_shape_is_named_with_its_path() {
    let cases = [
        (
            r#"{"1":{"name":"a","type":"ID","codes":"PR"}}"#,
            "spec: the value at segments.AA.elements.1.codes must be an array of strings; found a string (\"PR\")",
        ),
        (
            r#"{"1":{"name":"a","type":"AN","composite":{"1":{"name":"b","type":"ID","codes":["A",7]}}}}"#,
            "spec: the value at segments.AA.elements.1.composite.1.codes[1] must be a string; found a number (7)",
        ),
    ];
    for (elements, expected) in cases {
        assert_eq!(element_error(elements).to_string(), expected, "{elements}");
    }
}

#[test]
fn codes_survive_a_round_trip_and_a_patch() {
    let spec = Spec::from_json(
        r#"{"name":"t","loops":{"a":{"trigger":{"segment":"AA"}}},"segments":{"AA":{"elements":{
            "1":{"name":"qualifier","type":"ID","codes":["B","A"]}
        }}}}"#,
    )
    .unwrap();
    let again = Spec::from_json(&spec.to_json()).unwrap();
    assert!(spec.segments().eq(again.segments()));
    let patched = spec
        .merge_patch(r#"{"segments":{"AA":{"elements":{"1":{"codes":["C"]}}}}}"#)
        .unwrap();
    assert_eq!(
        patched.element_def(b"AA", 1, None).unwrap().codes,
        vec!["C"]
    );
    let opened = spec
        .merge_patch(r#"{"segments":{"AA":{"elements":{"1":{"codes":null}}}}}"#)
        .unwrap();
    assert!(opened.element_def(b"AA", 1, None).unwrap().codes.is_empty());
}

#[test]
fn holds_code_on_a_sorted_list() {
    use crate::spec::segments::holds_code;
    let codes: Vec<String> = vec!["A".into(), "C".into()];
    assert!(holds_code(&codes, b"C"));
    assert!(!holds_code(&codes, b"B"));
    assert!(!holds_code(&[], b"A"));
}

#[test]
fn rejects_code_keeps_an_empty_list_open() {
    assert!(!rejects_code(&[], b"A"));
    assert!(rejects_code(&["A".into()], b"B"));
    assert!(!rejects_code(&["A".into()], b"A"));
}
