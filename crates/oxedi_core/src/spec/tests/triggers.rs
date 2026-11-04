//! Triggers: matching, specificity and ambiguity between siblings.

use super::segs;
use crate::spec::*;

#[test]
fn matching_any_finds_a_loop_regardless_of_parent() {
    let spec = Spec::builtin_835();
    let segments = segs(b"SVC*HC:1*10*10~ZZZ*1~");
    assert_eq!(spec.matching_any(&segments[0]), spec.loop_id("2110"));
    assert_eq!(spec.matching_any(&segments[1]), None);
}

#[test]
fn more_specific_trigger_wins_over_a_bare_one() {
    let spec = Spec::from_json(
        r#"{"name":"t","loops":{
                "any":{"trigger":{"segment":"N1"}},
                "payer":{"trigger":{"segment":"N1","where":{"1":"PR"}}}
            }}"#,
    )
    .unwrap();
    let segments = segs(b"N1*PR~N1*PE~");
    assert_eq!(
        spec.matching_child(None, &segments[0]),
        spec.loop_id("payer")
    );
    assert_eq!(spec.matching_child(None, &segments[1]), spec.loop_id("any"));
}

#[test]
fn accepts_covers_segments_and_end() {
    let spec = Spec::builtin_835();
    let transaction = spec.get(spec.loop_id("transaction").unwrap());
    assert!(transaction.accepts(b"BPR"));
    assert!(transaction.accepts(b"SE"));
    assert!(!transaction.accepts(b"CLP"));
}

#[test]
fn an_unknown_key_is_rejected_with_its_key_path() {
    let loop_json = |loop_def: &str| format!(r#"{{"name":"t","loops":{{"env":{loop_def}}}}}"#);
    let cases = [
        (
            r#"{"name":"t","loops":{"a":{"trigger":{"segment":"AA"}}},"tabels":{}}"#.to_string(),
            "spec: unknown key \"tabels\" at the top level; the allowed keys are: balancing, default_table, loops, name, segments, summaries, tables, version",
        ),
        (
            loop_json(r#"{"trigger":{"segment":"AA"},"typo":1}"#),
            "spec: unknown key \"typo\" at loops.env; the allowed keys are: control, end, max, occurrences, parent, trigger, usage",
        ),
        (
            loop_json(r#"{"trigger":{"segment":"AA","wher":{}}}"#),
            "spec: unknown key \"wher\" at loops.env.trigger; the allowed keys are: segment, where",
        ),
        (
            loop_json(
                r#"{"trigger":{"segment":"HD"},"end":"TR","control":{"opener_element":2,"closer_element":2,"count_element":1,"count":"segments","extra":0}}"#,
            ),
            "spec: unknown key \"extra\" at loops.env.control; the allowed keys are: closer_element, count, count_element, opener_element",
        ),
    ];
    for (json, expected) in cases {
        let err = Spec::from_json(&json).unwrap_err();
        assert!(
            matches!(&err, SpecError::UnknownKey { .. }),
            "{json}: {err:?}"
        );
        assert_eq!(err.to_string(), expected, "{json}");
    }
}

#[test]
fn ambiguous_sibling_triggers_are_rejected() {
    let err = Spec::from_json(
        r#"{"name":"t","loops":{
                "a":{"trigger":{"segment":"AA","where":{"1":"X"}}},
                "b":{"trigger":{"segment":"AA","where":{"1":"X"}}}
            }}"#,
    )
    .unwrap_err();
    assert!(
        matches!(&err, SpecError::AmbiguousTrigger { first, second, parent: None, segment, .. } if first == "a" && second == "b" && segment == "AA"),
        "{err}"
    );
    let ok = Spec::from_json(
        r#"{"name":"t","loops":{
                "a":{"trigger":{"segment":"AA","where":{"1":"X"}}},
                "b":{"trigger":{"segment":"AA","where":{"1":"Y"}}}
            }}"#,
    );
    assert!(ok.is_ok(), "different conditions are not ambiguous");
}

#[test]
fn siblings_testing_different_positions_overlap_and_are_rejected() {
    let err = Spec::from_json(
        r#"{"name":"t","loops":{
                "transaction":{"trigger":{"segment":"ST"}},
                "payer":{"parent":"transaction","trigger":{"segment":"N1","where":{"1":"PR"}}},
                "other":{"parent":"transaction","trigger":{"segment":"N1","where":{"2":"X"}}}
            }}"#,
    )
    .unwrap_err();
    assert!(
        matches!(&err, SpecError::OverlappingTriggers { parent: Some(parent), a, b, .. } if parent == "transaction" && a == "other" && b == "payer"),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        "loops \"other\" and \"payer\" under \"transaction\" can open on the same segment: \"other\" on \"N1\" where {2: \"X\"}, \"payer\" on \"N1\" where {1: \"PR\"}, no position they both test requires different values, and neither trigger is more specific than the other"
    );
}

#[test]
fn a_bare_trigger_beside_a_conditioned_sibling_is_a_catch_all_and_loads() {
    let spec = Spec::from_json(
        r#"{"name":"t","loops":{
                "transaction":{"trigger":{"segment":"ST"}},
                "any":{"parent":"transaction","trigger":{"segment":"N1"}},
                "payer":{"parent":"transaction","trigger":{"segment":"N1","where":{"1":"PR"}}}
            }}"#,
    );
    assert!(spec.is_ok(), "{spec:?}");
}

#[test]
fn a_strict_superset_of_conditions_does_not_overlap() {
    let spec = Spec::from_json(
        r#"{"name":"t","loops":{
                "payer":{"trigger":{"segment":"N1","where":{"1":"PR"}}},
                "acme":{"trigger":{"segment":"N1","where":{"1":"PR","2":"X"}}}
            }}"#,
    )
    .unwrap();
    let segments = segs(b"N1*PR*X~N1*PR*Y~");
    assert_eq!(
        spec.matching_child(None, &segments[0]),
        spec.loop_id("acme"),
        "the more specific trigger wins"
    );
    assert_eq!(
        spec.matching_child(None, &segments[1]),
        spec.loop_id("payer")
    );
}

#[test]
fn siblings_that_differ_at_a_shared_position_do_not_overlap() {
    let ok = Spec::from_json(
        r#"{"name":"t","loops":{
                "payer":{"trigger":{"segment":"N1","where":{"1":"PR","2":"X"}}},
                "payee":{"trigger":{"segment":"N1","where":{"1":"PE"}}},
                "other":{"trigger":{"segment":"N3"}}
            }}"#,
    );
    assert!(ok.is_ok(), "{ok:?}");
    let builtin = Spec::builtin_835();
    assert!(builtin.loop_id("1000A").is_some() && builtin.loop_id("1000B").is_some());
}

#[test]
fn overlapping_triggers_display_both_loops_and_their_triggers() {
    let err = SpecError::OverlappingTriggers {
        parent: Some("transaction".into()),
        a: "1000A".into(),
        b: "1000C".into(),
        conditions_a: "\"N1\" where {1: \"PR\"}".into(),
        conditions_b: "\"N1\" where {2: \"X\"}".into(),
    };
    assert_eq!(
        err.to_string(),
        "loops \"1000A\" and \"1000C\" under \"transaction\" can open on the same segment: \"1000A\" on \"N1\" where {1: \"PR\"}, \"1000C\" on \"N1\" where {2: \"X\"}, no position they both test requires different values, and neither trigger is more specific than the other"
    );
    assert!(std::error::Error::source(&err).is_none());
    let err = SpecError::OverlappingTriggers {
        parent: None,
        a: "a".into(),
        b: "b".into(),
        conditions_a: "\"AA\" where {1: \"X\"}".into(),
        conditions_b: "\"AA\" where {2: \"Y\"}".into(),
    };
    assert_eq!(
        err.to_string(),
        "loops \"a\" and \"b\" under the root can open on the same segment: \"a\" on \"AA\" where {1: \"X\"}, \"b\" on \"AA\" where {2: \"Y\"}, no position they both test requires different values, and neither trigger is more specific than the other"
    );
}

#[test]
fn triggers_render_with_their_conditions_in_position_order() {
    let bare = Trigger {
        segment: b"N1".to_vec(),
        conditions: Vec::new(),
    };
    assert_eq!(render_trigger(&bare), "\"N1\" with no conditions");
    let conditioned = Trigger {
        segment: b"N1".to_vec(),
        conditions: vec![(1, b"PR".to_vec()), (3, b"X".to_vec())],
    };
    assert_eq!(
        render_trigger(&conditioned),
        "\"N1\" where {1: \"PR\", 3: \"X\"}"
    );
}

#[test]
fn to_json_round_trips_through_from_json() {
    let spec = Spec::builtin_835();
    let again = Spec::from_json(&spec.to_json()).unwrap();
    assert_eq!(again.loops(), spec.loops());
}
