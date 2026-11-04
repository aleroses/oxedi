//! Table definitions and their errors.

use super::{TABLED, table_error};
use crate::spec::*;

#[test]
fn tables_are_read_in_name_order_with_their_sources() {
    let spec = Spec::from_json(TABLED).unwrap();
    let names: Vec<&str> = spec.tables().iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, vec!["adjustments", "bodies", "heads", "lines"]);
    let id = |name: &str| spec.loop_id(name).unwrap();
    let heads = spec.table("heads").unwrap();
    assert_eq!(heads.reference, "head");
    assert_eq!(heads.loops, vec![id("A")]);
    assert_eq!((heads.segment.as_ref(), heads.repeat), (None, None));
    assert_eq!(
        heads.columns,
        vec![
            (
                "code".to_string(),
                ColumnSource::Element {
                    loop_id: None,
                    segment: b"AA".to_vec(),
                    conditions: Vec::new(),
                    occurrence: None,
                    pick: Pick::First,
                    element: 1,
                    component: None,
                }
            ),
            (
                "note".to_string(),
                ColumnSource::Element {
                    loop_id: Some(id("D")),
                    segment: b"DD".to_vec(),
                    conditions: vec![(1, b"N".to_vec())],
                    occurrence: None,
                    pick: Pick::First,
                    element: 2,
                    component: None,
                }
            ),
        ]
    );
    assert_eq!(
        spec.table("bodies").unwrap().columns[1].1,
        ColumnSource::SegmentIndex {
            loop_id: Some(id("C")),
            segment: b"C1".to_vec(),
            conditions: Vec::new(),
            occurrence: None,
            pick: Pick::First,
        }
    );
    let adjustments = spec.table("adjustments").unwrap();
    assert_eq!(
        adjustments.reference, "adjustments",
        "ref defaults to the name"
    );
    assert_eq!(adjustments.loops, vec![id("B"), id("C")]);
    assert_eq!(adjustments.segment.as_deref(), Some(&b"AJ"[..]));
    assert_eq!(adjustments.repeat, Some(Repeat { from: 2, step: 2 }));
    assert_eq!(
        adjustments.columns,
        vec![
            (
                "amount".to_string(),
                ColumnSource::GroupElement {
                    offset: 1,
                    component: None
                }
            ),
            (
                "kind".to_string(),
                ColumnSource::Element {
                    loop_id: None,
                    segment: b"AJ".to_vec(),
                    conditions: Vec::new(),
                    occurrence: None,
                    pick: Pick::First,
                    element: 1,
                    component: None,
                }
            ),
            (
                "reason".to_string(),
                ColumnSource::GroupElement {
                    offset: 0,
                    component: None
                }
            ),
        ]
    );
    assert_eq!(spec.table("missing"), None);
}

#[test]
fn a_table_hangs_from_the_tables_anchored_above_it() {
    let spec = Spec::from_json(TABLED).unwrap();
    let index = |name: &str| spec.tables().iter().position(|t| t.name == name).unwrap();
    let (adjustments, bodies, heads, lines) = (
        index("adjustments"),
        index("bodies"),
        index("heads"),
        index("lines"),
    );
    let table = |i: usize| &spec.tables()[i];
    assert_eq!(
        (table(heads).parent, table(heads).ancestors.clone()),
        (None, vec![])
    );
    assert_eq!(table(bodies).parent, Some(heads));
    assert_eq!(table(lines).ancestors, vec![heads, bodies]);
    assert_eq!(
        table(adjustments).ancestors,
        vec![heads, bodies, lines],
        "a segment table anchored in B and C hangs from the deepest chain"
    );
    assert_eq!(table(adjustments).parent, Some(lines));
}

#[test]
fn element_definitions_are_found_by_segment_position_and_component() {
    let spec = Spec::from_json(TABLED).unwrap();
    assert_eq!(spec.element_def(b"BB", 2, None).unwrap().name, "amount");
    assert_eq!(spec.element_def(b"CC", 1, Some(2)).unwrap().name, "value");
    assert_eq!(spec.element_def(b"CC", 1, Some(3)), None);
    assert_eq!(spec.element_def(b"BB", 9, None), None);
    assert_eq!(spec.element_def(b"ZZ", 1, None), None);
}

#[test]
fn bad_tables_are_rejected_with_the_table_the_column_and_the_reason() {
    let cases: Vec<(&str, &str, Option<&str>, TableDefError)> = vec![
        (
            r#"{"":{"loops":["A"]}}"#,
            "",
            None,
            TableDefError::EmptyName { what: "table name" },
        ),
        (r#"{"t":{"loops":[]}}"#, "t", None, TableDefError::NoLoops),
        (
            r#"{"t":{"loops":["A","B","A"]}}"#,
            "t",
            None,
            TableDefError::DuplicateLoop {
                loop_name: "A".into(),
            },
        ),
        (
            r#"{"t":{"loops":["A"],"segment":"AA","repeat":{"from":2,"step":2},"columns":{"c":{"group_element":0,"where":{"1":"X"}}}}}"#,
            "t",
            Some("c"),
            TableDefError::AnchorSegmentOnly {
                key: "where",
                anchor_segment: "AA".into(),
                written: r#"{"1":"X"}"#.into(),
            },
        ),
        (
            r#"{"t":{"loops":["Z"]}}"#,
            "t",
            None,
            TableDefError::UnknownLoop { name: "Z".into() },
        ),
        (
            r#"{"t":{"loops":["A"],"ref":""}}"#,
            "t",
            None,
            TableDefError::EmptyName { what: "ref" },
        ),
        (
            r#"{"t":{"loops":["A"],"ref":"segment"}}"#,
            "t",
            None,
            TableDefError::ReservedName {
                name: "segment".into(),
            },
        ),
        (
            r#"{"a":{"loops":["A"],"ref":"x"},"b":{"loops":["B"],"ref":"x"}}"#,
            "b",
            None,
            TableDefError::RefTaken {
                name: "x".into(),
                table: "a".into(),
            },
        ),
        (
            r#"{"t":{"loops":["A"],"repeat":{"from":1,"step":2}}}"#,
            "t",
            None,
            TableDefError::RepeatWithoutSegment,
        ),
        (
            r#"{"t":{"loops":["A"],"segment":"AA","repeat":{"from":0,"step":2}}}"#,
            "t",
            None,
            TableDefError::ZeroPosition { key: "repeat.from" },
        ),
        (
            r#"{"t":{"loops":["A"],"segment":"AA","repeat":{"from":1,"step":0}}}"#,
            "t",
            None,
            TableDefError::ZeroStep,
        ),
        (
            r#"{"t":{"loops":["C","A"]}}"#,
            "t",
            None,
            TableDefError::NestedAnchors {
                outer: "A".into(),
                inner: "C".into(),
            },
        ),
        (
            r#"{"a":{"loops":["B"]},"b":{"loops":["D","B"]}}"#,
            "b",
            None,
            TableDefError::SharedAnchor {
                loop_name: "B".into(),
                other: "a".into(),
            },
        ),
        (
            r#"{"b":{"loops":["B"]},"d":{"loops":["D"]},"x":{"loops":["C","D"],"segment":"XX"}}"#,
            "x",
            None,
            TableDefError::UnrelatedAnchors {
                first: "C".into(),
                second: "D".into(),
                chains: Box::new(AnchorChains {
                    first: vec!["b".into()],
                    second: vec!["d".into()],
                }),
            },
        ),
        (
            r#"{"t":{"loops":["A"],"columns":{"":{"segment":"AA","element":1}}}}"#,
            "t",
            Some(""),
            TableDefError::EmptyName {
                what: "column name",
            },
        ),
        (
            r#"{"t":{"loops":["A"],"columns":{"row":{"segment":"AA","element":1}}}}"#,
            "t",
            Some("row"),
            TableDefError::ReservedName { name: "row".into() },
        ),
        (
            r#"{"a":{"loops":["A"],"ref":"head"},"b":{"loops":["B"],"columns":{"head":{"segment":"BB","element":1}}}}"#,
            "b",
            Some("head"),
            TableDefError::ReservedName {
                name: "head".into(),
            },
        ),
        (
            r#"{"t":{"loops":["A"],"columns":{"c":{"segment":"AA"}}}}"#,
            "t",
            Some("c"),
            TableDefError::SourceCount { found: 0 },
        ),
        (
            r#"{"t":{"loops":["A"],"columns":{"c":{"segment":"AA","element":1,"segment_index":true}}}}"#,
            "t",
            Some("c"),
            TableDefError::SourceCount { found: 2 },
        ),
        (
            r#"{"t":{"loops":["A"],"columns":{"c":{"segment":"AA","segment_index":true,"component":1}}}}"#,
            "t",
            Some("c"),
            TableDefError::ComponentOnIndex,
        ),
        (
            r#"{"t":{"loops":["A"],"columns":{"c":{"segment":"AA","element":0}}}}"#,
            "t",
            Some("c"),
            TableDefError::ZeroPosition { key: "element" },
        ),
        (
            r#"{"t":{"loops":["A"],"columns":{"c":{"segment":"AA","element":1,"component":0}}}}"#,
            "t",
            Some("c"),
            TableDefError::ZeroPosition { key: "component" },
        ),
        (
            r#"{"t":{"loops":["A"],"columns":{"c":{"group_element":0}}}}"#,
            "t",
            Some("c"),
            TableDefError::GroupWithoutRepeat,
        ),
        (
            r#"{"t":{"loops":["A"],"segment":"AA","repeat":{"from":2,"step":3},"columns":{"c":{"group_element":3}}}}"#,
            "t",
            Some("c"),
            TableDefError::OffsetBeyondStep { offset: 3, step: 3 },
        ),
        (
            r#"{"t":{"loops":["A"],"segment":"AA","columns":{"c":{"loop":"B","element":1}}}}"#,
            "t",
            Some("c"),
            TableDefError::AnchorSegmentOnly {
                key: "loop",
                anchor_segment: "AA".into(),
                written: r#""B""#.into(),
            },
        ),
        (
            r#"{"t":{"loops":["A"],"columns":{"c":{"element":1}}}}"#,
            "t",
            Some("c"),
            TableDefError::NeedsSegment,
        ),
        (
            r#"{"t":{"loops":["A"],"columns":{"c":{"loop":"Z","segment":"ZZ","element":1}}}}"#,
            "t",
            Some("c"),
            TableDefError::UnknownLoop { name: "Z".into() },
        ),
        (
            r#"{"t":{"loops":["B","D"],"columns":{"c":{"loop":"C","segment":"CC","element":1}}}}"#,
            "t",
            Some("c"),
            TableDefError::UnrelatedLoop {
                loop_name: "C".into(),
                anchor: "D".into(),
            },
        ),
        (
            r#"{"t":{"loops":["C"],"columns":{"c":{"loop":"D","segment":"DD","element":1}}}}"#,
            "t",
            Some("c"),
            TableDefError::UnrelatedLoop {
                loop_name: "D".into(),
                anchor: "C".into(),
            },
        ),
        (
            r#"{"t":{"loops":["A"],"columns":{"c":{"segment":"AA","where":{"01":"X"},"element":1}}}}"#,
            "t",
            Some("c"),
            TableDefError::BadPosition { key: "01".into() },
        ),
    ];
    for (tables, expected_table, expected_column, expected_reason) in cases {
        let err = table_error(tables);
        assert!(
            matches!(&err, SpecError::BadTable { table, column, reason } if table == expected_table && column.as_deref() == expected_column && *reason == expected_reason),
            "{tables}: {err:?}"
        );
    }
}

#[test]
fn a_column_or_table_reading_a_segment_its_loop_never_holds_is_rejected() {
    let json = |tables: &str| {
        format!(
            r#"{{"name":"t","loops":{{
                    "A":{{"trigger":{{"segment":"AA"}},"occurrences":{{"aa":{{"segment":"AA","pos":0}},"a1":{{"segment":"A1","pos":1}}}},"end":"AE"}},
                    "B":{{"parent":"A","trigger":{{"segment":"BB"}}}}
                }},"tables":{tables}}}"#
        )
    };
    let held = json(
        r#"{"t":{"loops":["A"],"columns":{
                "a":{"segment":"AA","element":1},
                "b":{"segment":"A1","element":1},
                "c":{"segment":"AE","element":1}}},
               "u":{"loops":["A"],"segment":"A1","columns":{"x":{"element":1}}}}"#,
    );
    assert!(Spec::from_json(&held).is_ok());
    let bad_column =
        json(r#"{"t":{"loops":["A"],"columns":{"c":{"segment":"REFF","element":1}}}}"#);
    assert_eq!(
        Spec::from_json(&bad_column).unwrap_err().to_string(),
        "table \"t\" column \"c\": segment \"REFF\" is neither the trigger nor a segment of loop \"A\", so it is never read there"
    );
    let bad_loop =
        json(r#"{"t":{"loops":["A"],"columns":{"c":{"loop":"B","segment":"A1","element":1}}}}"#);
    assert!(matches!(
        Spec::from_json(&bad_loop).unwrap_err(),
        SpecError::BadTable {
            reason: TableDefError::SegmentNotHeld { ref loop_name, .. },
            ..
        } if loop_name == "B"
    ));
    let bad_table = json(r#"{"t":{"loops":["A"],"segment":"REFF","columns":{"c":{"element":1}}}}"#);
    assert!(matches!(
        Spec::from_json(&bad_table).unwrap_err(),
        SpecError::BadTable {
            column: None,
            reason: TableDefError::SegmentNotHeld { ref segment, ref loop_name },
            ..
        } if segment == "REFF" && loop_name == "A"
    ));
}

#[test]
fn a_segment_index_column_must_name_a_segment_its_loop_holds() {
    let json = |tables: &str| {
        format!(
            r#"{{"name":"t","loops":{{
                    "A":{{"trigger":{{"segment":"AA"}},"occurrences":{{"aa":{{"segment":"AA","pos":0}},"a1":{{"segment":"A1","pos":1}}}}}},
                    "B":{{"parent":"A","trigger":{{"segment":"BB"}}}}
                }},"tables":{tables}}}"#
        )
    };
    let held = json(
        r#"{"t":{"loops":["A"],"columns":{
                "a":{"segment":"AA","segment_index":true},
                "b":{"segment":"A1","segment_index":true}}}}"#,
    );
    assert!(Spec::from_json(&held).is_ok());
    let bad_anchor =
        json(r#"{"t":{"loops":["A"],"columns":{"c":{"segment":"REFF","segment_index":true}}}}"#);
    assert_eq!(
        Spec::from_json(&bad_anchor).unwrap_err().to_string(),
        "table \"t\" column \"c\": segment \"REFF\" is neither the trigger nor a segment of loop \"A\", so it is never read there"
    );
    let bad_loop = json(
        r#"{"t":{"loops":["A"],"columns":{"c":{"loop":"B","segment":"A1","segment_index":true}}}}"#,
    );
    assert_eq!(
        Spec::from_json(&bad_loop).unwrap_err().to_string(),
        "table \"t\" column \"c\": segment \"A1\" is neither the trigger nor a segment of loop \"B\", so it is never read there"
    );
}

#[test]
fn anchor_conflicts_show_the_datum_as_the_spec_writes_it() {
    let err = table_error(
        r#"{"t":{"loops":["A"],"segment":"AA","columns":{"c":{"element":1,"where":{"2":"X"}}}}}"#,
    );
    assert_eq!(
        err.to_string(),
        "table \"t\" column \"c\": \"where\" ({\"2\":\"X\"}) does not apply in a table anchored on segment \"AA\": its columns read that segment"
    );
    let err = table_error(
        r#"{"b":{"loops":["B"]},"d":{"loops":["D"]},"x":{"loops":["C","D"],"segment":"XX"}}"#,
    );
    assert_eq!(
        err.to_string(),
        "table \"x\": anchor loops \"C\" and \"D\" sit under tables that are not one chain: \"C\" under b, \"D\" under d"
    );
}

#[test]
fn empty_segment_ids_in_tables_name_their_key() {
    let cases = [
        (r#"{"t":{"loops":["A"],"segment":""}}"#, "tables.t.segment"),
        (
            r#"{"t":{"loops":["A"],"columns":{"c":{"segment":"","element":1}}}}"#,
            "tables.t.columns.c.segment",
        ),
    ];
    for (tables, expected_key) in cases {
        let err = table_error(tables);
        assert!(
            matches!(&err, SpecError::EmptySegmentId { loop_name: None, key } if key == expected_key),
            "{tables}: {err:?}"
        );
    }
}

#[test]
fn a_table_key_the_schema_does_not_define_or_requires_is_named_with_its_path() {
    let cases = [
        (
            r#"{"t":{"loops":["A"],"anchor":"x"}}"#,
            "spec: unknown key \"anchor\" at tables.t; the allowed keys are: columns, loops, phi_seed, ref, repeat, segment",
        ),
        (
            r#"{"t":{"loops":["A"],"columns":{"c":{"elemnt":1}}}}"#,
            "spec: unknown key \"elemnt\" at tables.t.columns.c; the allowed keys are: component, element, group_element, loop, occurrence, phi, pick, segment, segment_index, where",
        ),
        (
            r#"{"t":{"loops":["A"],"repeat":{"from":1,"step":2,"to":3}}}"#,
            "spec: unknown key \"to\" at tables.t.repeat; the allowed keys are: from, step",
        ),
        (
            r#"{"t":{"segment":"AA"}}"#,
            "spec: missing required key \"loops\" at tables.t",
        ),
        (
            r#"{"t":{"loops":["A"],"repeat":{"from":1}}}"#,
            "spec: missing required key \"step\" at tables.t.repeat",
        ),
    ];
    for (tables, expected) in cases {
        let err = table_error(tables);
        assert!(
            matches!(
                &err,
                SpecError::UnknownKey { .. } | SpecError::MissingKey { .. }
            ),
            "{tables}: {err:?}"
        );
        assert_eq!(err.to_string(), expected, "{tables}");
    }
}

#[test]
fn every_object_of_the_table_schema_is_checked_with_its_path() {
    let cases = [
        (r#"[]"#, "tables", "an array"),
        (r#"{"t":[]}"#, "tables.t", "an array"),
        (
            r#"{"t":{"loops":["A"],"repeat":[2,3]}}"#,
            "tables.t.repeat",
            "an array",
        ),
        (
            r#"{"t":{"loops":["A"],"columns":[]}}"#,
            "tables.t.columns",
            "an array",
        ),
        (
            r#"{"t":{"loops":["A"],"columns":{"c":"CLP01"}}}"#,
            "tables.t.columns.c",
            "a string",
        ),
        (
            r#"{"t":{"loops":["A"],"columns":{"c":{"segment":"AA","element":1,"where":["X"]}}}}"#,
            "tables.t.columns.c.where",
            "an array",
        ),
    ];
    for (tables, expected_path, expected_found) in cases {
        let err = table_error(tables);
        assert!(
            matches!(&err, SpecError::NotAnObject { path, found } if path == expected_path && *found == expected_found),
            "{tables}: {err:?}"
        );
    }
}

#[test]
fn to_json_round_trips_the_tables_section() {
    let spec = Spec::from_json(TABLED).unwrap();
    let again = Spec::from_json(&spec.to_json()).unwrap();
    assert_eq!(again.tables(), spec.tables());
    assert_eq!(again.tables().len(), 4);
}

#[test]
fn table_errors_display_the_table_the_column_and_every_reason() {
    let cases = [
        (TableDefError::EmptyName { what: "ref" }, "the ref is empty"),
        (
            TableDefError::NoLoops,
            "\"loops\" is empty; a table anchors in at least one loop",
        ),
        (
            TableDefError::DuplicateLoop {
                loop_name: "2100".into(),
            },
            "loop \"2100\" is listed more than once in \"loops\"",
        ),
        (
            TableDefError::UnknownLoop {
                name: "2101".into(),
            },
            "loop \"2101\" does not exist",
        ),
        (
            TableDefError::ReservedName { name: "row".into() },
            "the name \"row\" is taken by an automatic column",
        ),
        (
            TableDefError::RefTaken {
                name: "claim".into(),
                table: "claims".into(),
            },
            "\"ref\" \"claim\" is already used by table \"claims\"",
        ),
        (
            TableDefError::RepeatWithoutSegment,
            "\"repeat\" requires \"segment\": only a segment's elements repeat",
        ),
        (TableDefError::ZeroStep, "\"repeat.step\" is 0"),
        (
            TableDefError::ZeroPosition { key: "element" },
            "\"element\" must be a 1-based position; found 0",
        ),
        (
            TableDefError::BadPosition { key: "01".into() },
            "\"where\" position \"01\" is not a 1-based integer in canonical form",
        ),
        (
            TableDefError::OffsetBeyondStep { offset: 3, step: 3 },
            "\"group_element\" 3 is outside a group of 3 elements (offsets start at 0)",
        ),
        (
            TableDefError::NestedAnchors {
                outer: "2100".into(),
                inner: "2110".into(),
            },
            "anchor loops \"2100\" and \"2110\" nest; a table without \"segment\" anchors in loops that do not",
        ),
        (
            TableDefError::SharedAnchor {
                loop_name: "2100".into(),
                other: "claims".into(),
            },
            "loop \"2100\" already anchors table \"claims\"; a loop anchors at most one table without \"segment\"",
        ),
        (
            TableDefError::UnrelatedAnchors {
                first: "2110".into(),
                second: "1000A".into(),
                chains: Box::new(AnchorChains {
                    first: vec!["payments".into(), "claims".into()],
                    second: vec!["payers".into()],
                }),
            },
            "anchor loops \"2110\" and \"1000A\" sit under tables that are not one chain: \"2110\" under payments/claims, \"1000A\" under payers",
        ),
        (
            TableDefError::UnrelatedAnchors {
                first: "2110".into(),
                second: "1000A".into(),
                chains: Box::new(AnchorChains {
                    first: vec!["a b".into()],
                    second: Vec::new(),
                }),
            },
            "anchor loops \"2110\" and \"1000A\" sit under tables that are not one chain: \"2110\" under \"a b\", \"1000A\" under no table",
        ),
        (
            TableDefError::UnrelatedLoop {
                loop_name: "1000A".into(),
                anchor: "2100".into(),
            },
            "loop \"1000A\" is neither anchor loop \"2100\", a loop inside it nor a loop above it",
        ),
        (
            TableDefError::UnknownOccurrence {
                loop_name: "2100".into(),
                occurrence: "patient".into(),
                known: Box::new(["claim".into(), "patient_name".into()]),
            },
            "loop \"2100\" has no occurrence \"patient\"; it declares \"claim\", \"patient_name\"",
        ),
        (
            TableDefError::UnknownOccurrence {
                loop_name: "2100".into(),
                occurrence: "patient".into(),
                known: Box::new([]),
            },
            "loop \"2100\" has no occurrence \"patient\"; it declares none",
        ),
        (
            TableDefError::OccurrenceAndSegment {
                key: "where",
                written: r#"{"1":"QC"}"#.into(),
            },
            "\"occurrence\" already names the segment and its qualifier; \"where\" ({\"1\":\"QC\"}) does not go with it",
        ),
        (
            TableDefError::OccurrenceSegments {
                occurrence: "amount".into(),
                segments: Box::new(LoopSegments {
                    first_loop: "2100".into(),
                    first_segment: "AMT".into(),
                    loop_name: "2110".into(),
                    segment: "QTY".into(),
                }),
            },
            "occurrence \"amount\" is segment \"AMT\" in anchor loop \"2100\" but segment \"QTY\" in anchor loop \"2110\"; name the loop to read with \"loop\"",
        ),
        (
            TableDefError::AfterAnchor {
                occurrence: "provider_adjustment".into(),
                positions: Box::new(AnchorPositions {
                    loop_name: "transaction".into(),
                    pos: 30100,
                    anchor: "2100".into(),
                    anchor_pos: 20100,
                }),
            },
            "occurrence \"provider_adjustment\" (position 30100) of loop \"transaction\" comes after anchor loop \"2100\" opens (position 20100): its segments arrive after the rows are appended, so the column never fills",
        ),
        (
            TableDefError::PickNeedsOccurrence {
                written: r#""last""#.into(),
            },
            "\"pick\" (\"last\") requires \"occurrence\": only a named occurrence has a known repeat",
        ),
        (
            TableDefError::BadPick {
                written: r#""middle""#.into(),
            },
            "\"pick\" must be \"first\", \"last\" or a 1-based position; found \"middle\"",
        ),
        (
            TableDefError::PickOnSingle {
                loop_name: "2100".into(),
                occurrence: "patient_name".into(),
                pick: Pick::Nth(2),
            },
            "occurrence \"patient_name\" of loop \"2100\" appears at most once, so \"pick\" (2) has nothing to choose from",
        ),
        (
            TableDefError::PickBeyondMax {
                nth: 3,
                loop_name: "2110".into(),
                occurrence: "service_date".into(),
                max: 2,
            },
            "\"pick\" 3 is past occurrence \"service_date\" of loop \"2110\", which repeats at most 2 times",
        ),
        (
            TableDefError::SegmentNotHeld {
                segment: "REFF".into(),
                loop_name: "2100".into(),
            },
            "segment \"REFF\" is neither the trigger nor a segment of loop \"2100\", so it is never read there",
        ),
        (
            TableDefError::AnchorSegmentOnly {
                key: "where",
                anchor_segment: "CLP".into(),
                written: r#"{"1":"X"}"#.into(),
            },
            "\"where\" ({\"1\":\"X\"}) does not apply in a table anchored on segment \"CLP\": its columns read that segment",
        ),
        (
            TableDefError::AnchorSegmentOnly {
                key: "segment",
                anchor_segment: "CLP".into(),
                written: r#""SVC""#.into(),
            },
            "\"segment\" (\"SVC\") does not apply in a table anchored on segment \"CLP\": its columns read that segment",
        ),
        (
            TableDefError::NeedsSegment,
            "the column names no \"segment\" or \"occurrence\" to read",
        ),
        (
            TableDefError::GroupWithoutRepeat,
            "\"group_element\" requires the table's \"repeat\"",
        ),
        (
            TableDefError::SourceCount { found: 2 },
            "a column takes exactly one of \"element\", \"group_element\" or \"segment_index\"; found 2",
        ),
        (
            TableDefError::ComponentOnIndex,
            "\"component\" does not apply to \"segment_index\"",
        ),
    ];
    for (reason, expected) in cases {
        let in_column = SpecError::BadTable {
            table: "claims".into(),
            column: Some("charge".into()),
            reason: reason.clone(),
        };
        assert_eq!(
            in_column.to_string(),
            format!("table \"claims\" column \"charge\": {expected}")
        );
        let in_table = SpecError::BadTable {
            table: "claims".into(),
            column: None,
            reason,
        };
        assert_eq!(
            in_table.to_string(),
            format!("table \"claims\": {expected}")
        );
        assert!(std::error::Error::source(&in_table).is_none());
    }
}
