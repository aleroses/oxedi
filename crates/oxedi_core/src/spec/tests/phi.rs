//! The `phi` and `phi_seed` keys: which columns hold patient data.

use crate::spec::*;
use crate::value::ElementType;
use crate::{Phi, SpecError};

fn spec_with(tables: &str) -> Result<Spec, SpecError> {
    Spec::from_json(&format!(
        r#"{{"name":"t","loops":{{"A":{{"trigger":{{"segment":"AA"}}}}}},
            "segments":{{"AA":{{"elements":{{
                "1":{{"name":"text","type":"AN"}},
                "2":{{"name":"day","type":"DT"}},
                "3":{{"name":"count","type":"N0"}}}}}}}},
            "tables":{tables}}}"#
    ))
}

fn reason(tables: &str) -> (Option<String>, TableDefError) {
    match spec_with(tables).unwrap_err() {
        SpecError::BadTable { column, reason, .. } => (column, reason),
        other => panic!("expected a table error, got {other}"),
    }
}

#[test]
fn flags_and_the_seed_are_read() {
    let spec = spec_with(
        r#"{"t":{"loops":["A"],"phi_seed":"id","columns":{
            "day":{"segment":"AA","element":2,"phi":"date"},
            "id":{"segment":"AA","element":1,"phi":"id"},
            "name":{"segment":"AA","element":1,"phi":"name"},
            "plain":{"segment":"AA","element":1}}}}"#,
    )
    .unwrap();
    let table = spec.table("t").unwrap();
    let names: Vec<&str> = table.columns.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["day", "id", "name", "plain"]);
    let flags: Vec<Option<Phi>> = (0..4).map(|i| table.phi(i)).collect();
    assert_eq!(
        flags,
        [Some(Phi::Date), Some(Phi::Id), Some(Phi::Name), None]
    );
    assert_eq!(table.phi(9), None);
    assert_eq!(table.phi_seed(), Some(1));
    assert!(spec.masks_anything());
}

#[test]
fn a_spec_without_flags_masks_nothing() {
    let spec =
        spec_with(r#"{"t":{"loops":["A"],"columns":{"c":{"segment":"AA","element":1}}}}"#).unwrap();
    assert!(!spec.masks_anything());
    assert_eq!(spec.table("t").unwrap().phi_seed(), None);
}

#[test]
fn bad_phi_declarations_are_refused() {
    let cases = [
        (
            r#"{"t":{"loops":["A"],"columns":{"c":{"segment":"AA","element":1,"phi":"ssn"}}}}"#,
            Some("c"),
            TableDefError::UnknownPhi {
                given: "ssn".into(),
            },
        ),
        (
            r#"{"t":{"loops":["A"],"columns":{"c":{"segment":"AA","element":3,"phi":"name"}}}}"#,
            Some("c"),
            TableDefError::PhiNeedsText {
                kind: "name",
                found: ColumnKind::Element(Some(ElementType::N(0))),
            },
        ),
        (
            r#"{"t":{"loops":["A"],"columns":{"c":{"segment":"AA","element":2,"phi":"id"}}}}"#,
            Some("c"),
            TableDefError::PhiNeedsText {
                kind: "id",
                found: ColumnKind::Element(Some(ElementType::Dt)),
            },
        ),
        (
            r#"{"t":{"loops":["A"],"columns":{"c":{"segment":"AA","element":1,"phi":"date"}}}}"#,
            Some("c"),
            TableDefError::PhiNeedsDate {
                found: ColumnKind::Element(Some(ElementType::An)),
            },
        ),
        (
            r#"{"t":{"loops":["A"],"phi_seed":"nope","columns":{"c":{"segment":"AA","element":1,"phi":"id"}}}}"#,
            None,
            TableDefError::UnknownPhiSeed {
                name: "nope".into(),
            },
        ),
        (
            r#"{"t":{"loops":["A"],"phi_seed":"c","columns":{"c":{"segment":"AA","element":1,"phi":"name"}}}}"#,
            Some("c"),
            TableDefError::PhiSeedNotId { name: "c".into() },
        ),
    ];
    for (tables, column, expected) in cases {
        assert_eq!(
            reason(tables),
            (column.map(str::to_string), expected),
            "{tables}"
        );
    }
}

#[test]
fn phi_errors_display_the_rule_and_the_datum() {
    let cases = [
        (
            TableDefError::UnknownPhi {
                given: "ssn".into(),
            },
            "\"phi\" must be \"name\", \"id\" or \"date\"; got \"ssn\"",
        ),
        (
            TableDefError::PhiNeedsText {
                kind: "name",
                found: ColumnKind::Element(Some(ElementType::Dt)),
            },
            "\"phi\": \"name\" applies to a text column; this column is DT (date CCYYMMDD or YYMMDD)",
        ),
        (
            TableDefError::PhiNeedsDate {
                found: ColumnKind::Element(Some(ElementType::An)),
            },
            "\"phi\": \"date\" applies to a date column; this column is AN (string)",
        ),
        (
            TableDefError::UnknownPhiSeed {
                name: "nope".into(),
            },
            "\"phi_seed\" names column \"nope\", which the table does not have; name an existing column flagged \"phi\": \"id\", or set \"phi_seed\": null",
        ),
        (
            TableDefError::PhiSeedNotId { name: "c".into() },
            "\"phi_seed\" names column \"c\", which is not flagged \"phi\": \"id\"; flag it \"id\", name another \"id\" column, or set \"phi_seed\": null",
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }
}

#[test]
fn a_segment_index_and_an_undefined_element_are_judged_by_the_spec() {
    let (column, error) = reason(
        r#"{"t":{"loops":["A"],"segment":"AA","columns":{"c":{"segment_index":true,"phi":"id"}}}}"#,
    );
    assert_eq!(column.as_deref(), Some("c"));
    assert_eq!(
        error,
        TableDefError::PhiNeedsText {
            kind: "id",
            found: ColumnKind::SegmentIndex
        }
    );
    assert_eq!(
        error.to_string(),
        "\"phi\": \"id\" applies to a text column; this column is a segment index"
    );
    assert!(
        spec_with(
            r#"{"t":{"loops":["A"],"columns":{"c":{"segment":"AA","element":9,"phi":"name"}}}}"#
        )
        .is_ok()
    );
    let (_, error) = reason(
        r#"{"t":{"loops":["A"],"columns":{"c":{"segment":"AA","element":9,"phi":"date"}}}}"#,
    );
    assert_eq!(
        error.to_string(),
        "\"phi\": \"date\" applies to a date column; this column is an element the spec does not define"
    );
}

fn control_spec(segment: &str, element: usize) -> Result<Spec, SpecError> {
    Spec::from_json(&format!(
        r#"{{"name":"t","loops":{{
            "GS":{{"trigger":{{"segment":"GS"}},"end":"GE","control":{{"opener_element":6,"closer_element":2,"count_element":1,"count":"children"}}}},
            "ST":{{"parent":"GS","trigger":{{"segment":"ST"}},"end":"SE","control":{{"opener_element":2,"closer_element":2,"count_element":1,"count":"segments"}}}}}},
            "tables":{{"t":{{"loops":["{}"],"columns":{{"ctl":{{"segment":"{segment}","element":{element},"phi":"id"}}}}}}}}}}"#,
        if segment.starts_with('G') { "GS" } else { "ST" }
    ))
}

#[test]
fn envelope_control_elements_cannot_be_flagged() {
    for (segment, element) in [
        ("ST", 2),
        ("SE", 1),
        ("SE", 2),
        ("GS", 6),
        ("GE", 1),
        ("GE", 2),
    ] {
        let Err(SpecError::BadTable { column, reason, .. }) = control_spec(segment, element) else {
            panic!("{segment}{element} was accepted");
        };
        assert_eq!(column.as_deref(), Some("ctl"));
        assert_eq!(
            reason,
            TableDefError::PhiOnControlElement {
                segment: segment.into(),
                element
            }
        );
    }
    assert!(control_spec("ST", 1).is_ok());
    assert!(control_spec("GS", 1).is_ok());
}

#[test]
fn a_control_element_error_is_displayed_in_full() {
    let error = control_spec("ST", 2).unwrap_err();
    assert_eq!(
        error.to_string(),
        "table \"t\" column \"ctl\": \"phi\" cannot flag ST02: it is an envelope control number or count; findings about envelope controls quote it unmasked, so it cannot be flagged; remove \"phi\" from this column"
    );
}

/// The table error a patch over a built-in spec gives, or `None` when the
/// patched spec loads.
fn patched_error(base: &Spec, table: &str) -> Option<(Option<String>, TableDefError)> {
    let error = match base.merge_patch(&format!(r#"{{"tables":{{"x":{table}}}}}"#)) {
        Ok(_) => return None,
        Err(SpecError::Patch { source }) => *source,
        Err(other) => panic!("expected a patch error, got {other}"),
    };
    match error {
        SpecError::BadTable { column, reason, .. } => Some((column, reason)),
        other => panic!("expected a table error, got {other}"),
    }
}

#[test]
fn patched_built_in_specs_refuse_flags_on_envelope_controls() {
    let cases = [
        (
            r#"{"loops":["interchange"],"columns":{"c":{"segment":"ISA","element":13,"phi":"id"}}}"#,
            "ISA",
            13,
        ),
        (
            r#"{"loops":["interchange"],"columns":{"c":{"segment":"IEA","element":2,"phi":"id"}}}"#,
            "IEA",
            2,
        ),
        (
            r#"{"loops":["transaction"],"columns":{"c":{"segment":"ST","element":2,"component":1,"phi":"id"}}}"#,
            "ST",
            2,
        ),
        (
            r#"{"loops":["transaction"],"segment":"ST","repeat":{"from":1,"step":2},"columns":{"c":{"group_element":1,"phi":"id"}}}"#,
            "ST",
            2,
        ),
    ];
    for (name, base) in [
        ("5010", Spec::builtin_835()),
        ("4010", Spec::builtin_835_4010()),
    ] {
        for (table, segment, element) in cases {
            assert_eq!(
                patched_error(&base, table),
                Some((
                    Some("c".to_string()),
                    TableDefError::PhiOnControlElement {
                        segment: segment.into(),
                        element
                    }
                )),
                "{name}: {table}"
            );
        }
        let allowed =
            r#"{"loops":["transaction"],"segment":"ST","columns":{"c":{"element":1,"phi":"id"}}}"#;
        assert_eq!(patched_error(&base, allowed), None, "{name}");
    }
}

#[test]
fn a_repeated_group_is_refused_when_a_later_group_reads_a_control_element() {
    for (name, base) in [
        ("5010", Spec::builtin_835()),
        ("4010", Spec::builtin_835_4010()),
    ] {
        let table = r#"{"loops":["transaction"],"segment":"ST","repeat":{"from":1,"step":1},"columns":{"c":{"group_element":0,"phi":"id"}}}"#;
        assert_eq!(
            patched_error(&base, table),
            Some((
                Some("c".to_string()),
                TableDefError::PhiOnControlElement {
                    segment: "ST".into(),
                    element: 2
                }
            )),
            "{name}"
        );
        let clear = r#"{"loops":["transaction"],"segment":"ST","repeat":{"from":1,"step":2},"columns":{"c":{"group_element":0,"phi":"id"}}}"#;
        assert_eq!(patched_error(&base, clear), None, "{name}");
    }
}

fn flagged(spec: &Spec) -> Vec<String> {
    let mut found = Vec::new();
    for table in spec.tables() {
        for (index, (column, _)) in table.columns.iter().enumerate() {
            if let Some(kind) = table.phi(index) {
                found.push(format!("{}.{column}={}", table.name, kind.word()));
            }
        }
    }
    found.sort();
    found
}

#[test]
fn the_built_in_specs_flag_the_eleven_patient_columns() {
    let expected = [
        "claims.claim_id=id",
        "claims.group_number=id",
        "claims.patient_first_name=name",
        "claims.patient_id=id",
        "claims.patient_last_name=name",
        "claims.payer_control_number=id",
        "claims.statement_from=date",
        "claims.statement_to=date",
        "provider_adjustments.reference_id=id",
        "services.line_item_control_number=id",
        "services.service_date=date",
    ];
    for spec in [Spec::builtin_835(), Spec::builtin_835_4010()] {
        assert_eq!(flagged(&spec), expected);
        let claims = spec.table("claims").unwrap();
        let seed = claims.phi_seed().unwrap();
        assert_eq!(claims.columns[seed].0, "patient_id");
        assert_eq!(spec.table("services").unwrap().phi_seed(), None);
        assert!(spec.masks_anything());
    }
}

#[test]
fn the_flags_survive_a_json_round_trip() {
    let spec = Spec::builtin_835();
    let again = Spec::from_json(&spec.to_json()).unwrap();
    assert_eq!(flagged(&again), flagged(&spec));
    assert_eq!(
        again.table("claims").unwrap().phi_seed(),
        spec.table("claims").unwrap().phi_seed()
    );
}

#[test]
fn phi_declarations_of_the_wrong_shape_show_the_spec_error() {
    let cases = [
        (
            r#"{"t":{"loops":["A"],"columns":{"c":{"segment":"AA","element":1,"phi":1}}}}"#,
            "spec: the value at tables.t.columns.c.phi must be a string; found a number (1)",
        ),
        (
            r#"{"t":{"loops":["A"],"phi_seed":3,"columns":{"c":{"segment":"AA","element":1,"phi":"id"}}}}"#,
            "spec: the value at tables.t.phi_seed must be a string; found a number (3)",
        ),
        (
            r#"{"t":{"loops":["A"],"phi_seed":["c"],"columns":{"c":{"segment":"AA","element":1,"phi":"id"}}}}"#,
            "spec: the value at tables.t.phi_seed must be a string; found an array (1 items)",
        ),
    ];
    for (tables, expected) in cases {
        assert_eq!(spec_with(tables).unwrap_err().to_string(), expected);
    }
    let spec = spec_with(
        r#"{"t":{"loops":["A"],"columns":{"c":{"segment":"AA","element":1,"phi":null}}}}"#,
    )
    .unwrap();
    assert_eq!(spec.table("t").unwrap().phi(0), None);
}

#[test]
fn phi_accepts_only_text_elements() {
    let spec = |element: usize| {
        Spec::from_json(&format!(
            r#"{{"name":"t","loops":{{"A":{{"trigger":{{"segment":"AA"}}}}}},
                "segments":{{"AA":{{"elements":{{
                    "1":{{"name":"text","type":"AN"}},
                    "2":{{"name":"code","type":"ID"}},
                    "3":{{"name":"amount","type":"N2"}}}}}}}},
                "tables":{{"t":{{"loops":["A"],"columns":{{
                    "c":{{"segment":"AA","element":{element},"phi":"name"}}}}}}}}}}"#
        ))
    };
    let Err(SpecError::BadTable { column, reason, .. }) = spec(3) else {
        panic!("a name flag over an N2 element is refused");
    };
    assert_eq!(column.as_deref(), Some("c"));
    assert_eq!(
        reason,
        TableDefError::PhiNeedsText {
            kind: "name",
            found: ColumnKind::Element(Some(ElementType::N(2))),
        }
    );
    assert_eq!(
        reason.to_string(),
        "\"phi\": \"name\" applies to a text column; this column is N2 (integer with 2 implied decimals)"
    );
    for element in [1, 2, 9] {
        assert!(
            spec(element).is_ok(),
            "element {element} is text or undefined"
        );
    }
}
