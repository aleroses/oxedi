//! The version a spec declares, and the choice of a spec by that declaration.

use std::cell::Cell;

use super::segs;
use crate::spec::*;

fn declaring(name: &str, version: &str) -> Spec {
    let version = if version.is_empty() {
        String::new()
    } else {
        format!(r#","version":{version}"#)
    };
    Spec::from_json(&format!(
        r#"{{"name":"{name}","loops":{{"a":{{"trigger":{{"segment":"GS"}}}}}}{version}}}"#
    ))
    .unwrap()
}

fn version_error(version: &str) -> SpecError {
    Spec::from_json(&format!(
        r#"{{"name":"t","loops":{{"a":{{"trigger":{{"segment":"GS"}}}}}},"version":{version}}}"#
    ))
    .unwrap_err()
}

#[test]
fn a_declared_version_loads_and_survives_a_patch() {
    let spec = declaring(
        "t",
        r#"{"segment":"GS","element":8,"values":["005010X221A1"]}"#,
    );
    let version = spec.version().unwrap();
    assert_eq!(version.segment, b"GS");
    assert_eq!(version.element, 8);
    assert_eq!(version.values, vec![b"005010X221A1".to_vec()]);
    let patched = spec
        .merge_patch(r#"{"version":{"values":["004010X091A1","004010"]}}"#)
        .unwrap();
    let version = patched.version().unwrap();
    assert_eq!(version.segment, b"GS");
    assert_eq!(
        version.values,
        vec![b"004010X091A1".to_vec(), b"004010".to_vec()]
    );
    assert!(declaring("t", "").version().is_none());
    assert!(
        spec.merge_patch(r#"{"version":null}"#)
            .unwrap()
            .version()
            .is_none()
    );
}

#[test]
fn every_version_fault_is_named() {
    let err = version_error(r#"{"segment":"","element":8,"values":["X"]}"#);
    assert!(
        matches!(&err, SpecError::EmptySegmentId { loop_name: None, key } if key == "version.segment"),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        "the spec has an empty segment id at version.segment"
    );
    let cases = [
        (
            r#"{"segment":"GS","element":0,"values":["X"]}"#,
            VersionError::ZeroElement,
            "the spec's \"version\" is invalid: \"element\" is 0; positions are 1-based",
        ),
        (
            r#"{"segment":"GS","element":8,"values":[]}"#,
            VersionError::NoValues,
            "the spec's \"version\" is invalid: \"values\" is empty; list at least one value the element may hold",
        ),
        (
            r#"{"segment":"GS","element":8,"values":["X",""]}"#,
            VersionError::EmptyValue { index: 1 },
            "the spec's \"version\" is invalid: \"values[1]\" is empty",
        ),
        (
            r#"{"segment":"GS","element":8,"values":["004010","005010","004010"]}"#,
            VersionError::DuplicateValue {
                value: "004010".into(),
                first: 0,
                second: 2,
            },
            "the spec's \"version\" is invalid: value \"004010\" is listed twice, at values[0] and values[2]",
        ),
    ];
    for (version, expected_reason, expected_text) in cases {
        let err = version_error(version);
        assert!(
            matches!(&err, SpecError::BadVersion { reason } if *reason == expected_reason),
            "{version}: {err:?}"
        );
        assert_eq!(err.to_string(), expected_text);
        assert!(std::error::Error::source(&err).is_none());
    }
}

#[test]
fn a_version_of_the_wrong_shape_is_named_with_its_path() {
    let cases = [
        (
            r#""GS08""#,
            "spec: the value at version must be a JSON object; found a string",
        ),
        (
            r#"{"segment":"GS","element":8}"#,
            "spec: missing required key \"values\" at version",
        ),
        (
            r#"{"segment":"GS","element":8,"values":["X"],"value":"X"}"#,
            "spec: unknown key \"value\" at version; the allowed keys are: element, segment, values",
        ),
        (
            r#"{"segment":"GS","element":"8","values":["X"]}"#,
            "spec: the value at version.element must be a non-negative integer; found a string (\"8\")",
        ),
        (
            r#"{"segment":"GS","element":8,"values":[8]}"#,
            "spec: the value at version.values[0] must be a string; found a number (8)",
        ),
    ];
    for (version, expected) in cases {
        assert_eq!(version_error(version).to_string(), expected, "{version}");
    }
}

const ISA: &str = "ISA*00*          *00*          *ZZ*S              *ZZ*R              *240101*1200*^*00501*000000001*0*P*:~";

#[test]
fn select_picks_the_first_candidate_whose_declared_element_matches() {
    let five = declaring(
        "5010",
        r#"{"segment":"GS","element":8,"values":["005010X221A1"]}"#,
    );
    let four = declaring(
        "4010",
        r#"{"segment":"GS","element":8,"values":["004010X091A1","004010"]}"#,
    );
    let plain = declaring("plain", "");
    let candidates = [&plain, &five, &four];
    let pick = |gs08: &str| {
        let input = format!("{ISA}GS*HP*S*R*20240101*1200*1*X*{gs08}~ST*835*0001~");
        Spec::select(&candidates, &five, segs(input.as_bytes())).name()
    };
    assert_eq!(pick("004010X091A1"), "4010");
    assert_eq!(pick("004010"), "4010");
    assert_eq!(pick("005010X221A1"), "5010");
    // No candidate matches: the default.
    assert_eq!(pick("003070"), "5010");
    // A composite value is not one of the declared values.
    assert_eq!(pick("004010:X"), "5010");
    // No segment carries the declaration: the default.
    let no_group = format!("{ISA}ST*835*0001~");
    assert_eq!(
        Spec::select(&candidates, &plain, segs(no_group.as_bytes())).name(),
        "plain"
    );
}

#[test]
fn select_honours_candidate_order_when_declarations_name_different_segments() {
    let by_st = declaring("st", r#"{"segment":"ST","element":3,"values":["V"]}"#);
    let by_gs = declaring("gs", r#"{"segment":"GS","element":8,"values":["G"]}"#);
    let default = declaring("default", "");
    let input = format!("{ISA}GS*HP*S*R*20240101*1200*1*X*G~ST*835*0001*V~");
    let segments = segs(input.as_bytes());
    // `gs` resolves first in the stream, but `st` comes first in the list.
    assert_eq!(
        Spec::select(&[&by_st, &by_gs], &default, segments.clone()).name(),
        "st"
    );
    assert_eq!(
        Spec::select(&[&by_gs, &by_st], &default, segments).name(),
        "gs"
    );
}

#[test]
fn select_stops_reading_once_the_choice_is_settled() {
    let five = declaring("5010", r#"{"segment":"GS","element":8,"values":["A"]}"#);
    let four = declaring("4010", r#"{"segment":"GS","element":8,"values":["B"]}"#);
    let input = format!("{ISA}GS*HP*S*R*20240101*1200*1*X*B~ST*835*0001~SE*2*0001~GE*1*1~");
    let read = Cell::new(0);
    let segments = segs(input.as_bytes()).into_iter().inspect(|_| {
        read.set(read.get() + 1);
    });
    assert_eq!(
        Spec::select(&[&five, &four], &five, segments).name(),
        "4010"
    );
    assert_eq!(read.get(), 2, "only ISA and GS are read");
}

#[test]
fn the_builtins_declare_5010_and_4010_on_gs08() {
    let five = Spec::builtin_835();
    let four = Spec::builtin_835_4010();
    let declared = |spec: &Spec| {
        let version = spec.version().unwrap();
        (
            String::from_utf8_lossy(&version.segment).into_owned(),
            version.element,
            version
                .values
                .iter()
                .map(|value| String::from_utf8_lossy(value).into_owned())
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(
        declared(&five),
        ("GS".into(), 8, vec!["005010X221A1".to_string()])
    );
    assert_eq!(
        declared(&four),
        (
            "GS".into(),
            8,
            vec![
                "004010X091A1".to_string(),
                "004010X091".to_string(),
                "004010".to_string()
            ]
        )
    );
    // The 4010 spec is the 5010 one with the patch over it.
    let patched = five.merge_patch(Spec::BUILTIN_835_4010_PATCH).unwrap();
    assert_eq!(patched.to_json(), four.to_json());
}
