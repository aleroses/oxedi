//! The JSON shape checks and the paths they report.

use super::{element_error, table_error};
use crate::spec::shape::kind_of;
use crate::spec::*;

#[test]
fn a_missing_required_key_is_rejected_with_its_key_path() {
    let loop_json = |loop_def: &str| format!(r#"{{"name":"t","loops":{{"env":{loop_def}}}}}"#);
    let cases = [
        (
            r#"{"loops":{"a":{"trigger":{"segment":"AA"}}}}"#.to_string(),
            "spec: missing required key \"name\" at the top level",
        ),
        (
            loop_json(r#"{"occurrences":{}}"#),
            "spec: missing required key \"trigger\" at loops.env",
        ),
        (
            loop_json(r#"{"trigger":{"where":{"1":"X"}}}"#),
            "spec: missing required key \"segment\" at loops.env.trigger",
        ),
    ];
    for (json, expected) in cases {
        let err = Spec::from_json(&json).unwrap_err();
        assert!(
            matches!(&err, SpecError::MissingKey { .. }),
            "{json}: {err:?}"
        );
        assert_eq!(err.to_string(), expected, "{json}");
    }
}

#[test]
fn unknown_and_missing_keys_display_the_key_and_the_path() {
    let cases = [
        (
            SpecError::UnknownKey {
                path: "segments.CLP.elements.3".into(),
                key: "lenght".into(),
                allowed: vec!["max", "min", "name"],
            },
            "spec: unknown key \"lenght\" at segments.CLP.elements.3; the allowed keys are: \
             max, min, name",
        ),
        (
            SpecError::UnknownKey {
                path: String::new(),
                key: "tabels".into(),
                allowed: vec!["loops", "name"],
            },
            "spec: unknown key \"tabels\" at the top level; the allowed keys are: loops, name",
        ),
        (
            SpecError::MissingKey {
                path: "loops.env.control".into(),
                key: "count_element",
            },
            "spec: missing required key \"count_element\" at loops.env.control",
        ),
        (
            SpecError::MissingKey {
                path: String::new(),
                key: "loops",
            },
            "spec: missing required key \"loops\" at the top level",
        ),
    ];
    for (err, expected) in cases {
        assert_eq!(err.to_string(), expected);
        assert!(std::error::Error::source(&err).is_none());
    }
}

#[test]
fn unknown_parent_is_rejected_with_both_names() {
    let err = Spec::from_json(
        r#"{"name":"t","loops":{"a":{"parent":"ghost","trigger":{"segment":"AA"}}}}"#,
    )
    .unwrap_err();
    assert!(
        matches!(&err, SpecError::UnknownParent { loop_name, parent } if loop_name == "a" && parent == "ghost"),
        "{err}"
    );
}

#[test]
fn a_top_level_without_loops_names_the_missing_key() {
    let err = Spec::from_json(r#"{"name":"t"}"#).unwrap_err();
    assert!(
        matches!(&err, SpecError::MissingKey { path, key: "loops" } if path.is_empty()),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        "spec: missing required key \"loops\" at the top level"
    );
}

#[test]
fn a_spec_that_is_not_an_object_says_so_in_plain_words() {
    let err = Spec::from_json("[]").unwrap_err();
    assert!(
        matches!(&err, SpecError::NotAnObject { path, found: "an array" } if path.is_empty()),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        "the spec must be a JSON object; found an array"
    );
}

#[test]
fn every_object_of_the_loop_schema_is_checked_with_its_path() {
    let cases = [
        (r#"{"name":"t","loops":[]}"#, "loops", "an array"),
        (r#"{"name":"t","loops":{"a":"AA"}}"#, "loops.a", "a string"),
        (
            r#"{"name":"t","loops":{"a":{"trigger":["AA"]}}}"#,
            "loops.a.trigger",
            "an array",
        ),
        (
            r#"{"name":"t","loops":{"a":{"trigger":{"segment":"AA","where":1}}}}"#,
            "loops.a.trigger.where",
            "a number",
        ),
        (
            r#"{"name":"t","loops":{"a":{"trigger":{"segment":"AA","where":null}}}}"#,
            "loops.a.trigger.where",
            "null",
        ),
    ];
    for (json, expected_path, expected_found) in cases {
        let err = Spec::from_json(json).unwrap_err();
        assert!(
            matches!(&err, SpecError::NotAnObject { path, found } if path == expected_path && *found == expected_found),
            "{json}: {err:?}"
        );
    }
}

#[test]
fn a_patched_spec_goes_through_the_same_shape_check() {
    let err = Spec::builtin_835()
        .merge_patch(r#"{"loops":{"2100":{"trigger":["CLP"]}}}"#)
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "applying patch: spec: the value at loops.2100.trigger must be a JSON object; found an array"
    );
}

#[test]
fn keys_with_a_separator_or_whitespace_are_quoted_in_paths() {
    assert_eq!(render_key("2100"), "2100");
    assert_eq!(render_key("a.b"), "\"a.b\"");
    assert_eq!(render_key("x/y"), "\"x/y\"");
    assert_eq!(render_key("x#2"), "\"x#2\"");
    assert_eq!(render_key("a b"), "\"a b\"");
    assert_eq!(render_key(""), "\"\"");
}

#[test]
fn a_spec_error_path_quotes_a_loop_name_that_holds_a_dot() {
    let plain = Spec::from_json(r#"{"name":"t","loops":{"ab":{"trigger":[]}}}"#).unwrap_err();
    assert!(
        matches!(&plain, SpecError::NotAnObject { path, .. } if path == "loops.ab.trigger"),
        "{plain:?}"
    );
    let quoted = Spec::from_json(r#"{"name":"t","loops":{"a.b":{"trigger":[]}}}"#).unwrap_err();
    assert!(
        matches!(&quoted, SpecError::NotAnObject { path, .. } if path == "loops.\"a.b\".trigger"),
        "{quoted:?}"
    );
    let wrong = Spec::from_json(
        r#"{"name":"t","loops":{"a b":{"trigger":{"segment":"AA","where":{"1":2}}}}}"#,
    )
    .unwrap_err();
    assert_eq!(
        wrong.to_string(),
        "spec: the value at loops.\"a b\".trigger.where.1 must be a string; found a number (2)"
    );
}

#[test]
fn an_empty_segment_id_path_quotes_a_table_or_column_name_that_holds_a_dot() {
    let err = Spec::from_json(
        r#"{"name":"t","loops":{"A":{"trigger":{"segment":"AA"}}},
                "tables":{"a.b":{"loops":["A"],"columns":{"c d":{"segment":"","element":1}}}}}"#,
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "the spec has an empty segment id at tables.\"a.b\".columns.\"c d\".segment"
    );
}

#[test]
fn every_kind_of_json_value_is_named() {
    use serde_json::json;
    assert_eq!(kind_of(&json!(null)), "null");
    assert_eq!(kind_of(&json!(true)), "a boolean");
    assert_eq!(kind_of(&json!(1)), "a number");
    assert_eq!(kind_of(&json!("x")), "a string");
    assert_eq!(kind_of(&json!([])), "an array");
    assert_eq!(kind_of(&json!({})), "an object");
}

#[test]
fn a_spec_without_loops_is_rejected() {
    let err = Spec::from_json(r#"{"name":"t","loops":{}}"#).unwrap_err();
    assert!(
        matches!(&err, SpecError::NoLoops { spec_name } if spec_name == "t"),
        "{err:?}"
    );
}

#[test]
fn a_segment_scalar_of_the_wrong_kind_is_rejected_with_its_key_path() {
    let cases = [
        (
            r#"{"3":{"name":"p","type":"AN","min":"1"}}"#,
            "spec: the value at segments.AA.elements.3.min must be a non-negative integer; found a string (\"1\")",
        ),
        (
            r#"{"3":{"name":"p","type":"R","scale":300}}"#,
            "spec: the value at segments.AA.elements.3.scale must be an integer from 0 to 255; found a number above 255 (300)",
        ),
        (
            r#"{"3":{"name":"p","type":"AN","required":"yes"}}"#,
            "spec: the value at segments.AA.elements.3.required must be a boolean; found a string (\"yes\")",
        ),
        (
            r#"{"3":{"name":5,"type":"AN"}}"#,
            "spec: the value at segments.AA.elements.3.name must be a string; found a number (5)",
        ),
        (
            r#"{"1":{"name":"c","type":"AN","composite":{"2":{"name":"x","type":"AN","max":1.5}}}}"#,
            "spec: the value at segments.AA.elements.1.composite.2.max must be a non-negative integer; found a number with a fractional part (1.5)",
        ),
        (
            r#"{"3":{"name":"p","type":"AN","max":2.0}}"#,
            "spec: the value at segments.AA.elements.3.max must be a non-negative integer; found a floating-point number (2.0)",
        ),
    ];
    for (elements, expected) in cases {
        let err = element_error(elements);
        assert!(
            matches!(&err, SpecError::WrongType { .. }),
            "{elements}: {err:?}"
        );
        assert_eq!(err.to_string(), expected, "{elements}");
    }
}

#[test]
fn a_table_scalar_of_the_wrong_kind_is_rejected_with_its_key_path() {
    let cases = [
        (
            r#"{"claims":{"loops":["A",2]}}"#,
            "spec: the value at tables.claims.loops[1] must be a string; found a number (2)",
        ),
        (
            r#"{"claims":{"loops":"A"}}"#,
            "spec: the value at tables.claims.loops must be an array of strings; found a string (\"A\")",
        ),
        (
            r#"{"claims":{"loops":["A"],"repeat":{"from":"2","step":3}}}"#,
            "spec: the value at tables.claims.repeat.from must be a non-negative integer; found a string (\"2\")",
        ),
        (
            r#"{"claims":{"loops":["A"],"columns":{"x":{"segment":"AA","element":"1"}}}}"#,
            "spec: the value at tables.claims.columns.x.element must be a non-negative integer; found a string (\"1\")",
        ),
        (
            r#"{"claims":{"loops":["A"],"columns":{"x":{"segment_index":1}}}}"#,
            "spec: the value at tables.claims.columns.x.segment_index must be a boolean; found a number (1)",
        ),
        (
            r#"{"claims":{"loops":["A"],"columns":{"x":{"segment":"AA","element":1,"where":{"1":2}}}}}"#,
            "spec: the value at tables.claims.columns.x.where.1 must be a string; found a number (2)",
        ),
        (
            r#"{"claims":{"loops":["A"],"columns":{"x":{"occurrence":7,"element":1}}}}"#,
            "spec: the value at tables.claims.columns.x.occurrence must be a string; found a number (7)",
        ),
        (
            r#"{"claims":{"loops":["A"],"columns":{"x":{"occurrence":"o","pick":true,"element":1}}}}"#,
            "spec: the value at tables.claims.columns.x.pick must be \"first\", \"last\" or a positive integer; found a boolean (true)",
        ),
        (
            r#"{"claims":{"loops":["A"],"columns":{"x":{"occurrence":"o","pick":-2,"element":1}}}}"#,
            "spec: the value at tables.claims.columns.x.pick must be \"first\", \"last\" or a positive integer; found a negative number (-2)",
        ),
    ];
    for (tables, expected) in cases {
        let err = table_error(tables);
        assert!(
            matches!(&err, SpecError::WrongType { .. }),
            "{tables}: {err:?}"
        );
        assert_eq!(err.to_string(), expected, "{tables}");
    }
}

#[test]
fn wrong_type_displays_the_path_what_is_required_and_what_was_found() {
    let err = SpecError::WrongType {
        path: "loops.env.control.opener_element".into(),
        expected: "a non-negative integer",
        found: "a string",
        value: "\"2\"".into(),
    };
    assert_eq!(
        err.to_string(),
        "spec: the value at loops.env.control.opener_element must be a non-negative integer; found a string (\"2\")"
    );
    let null = SpecError::WrongType {
        path: "loops.env.end".into(),
        expected: "a string",
        found: "null",
        value: String::new(),
    };
    assert_eq!(
        null.to_string(),
        "spec: the value at loops.env.end must be a string; found null"
    );
}

#[test]
fn patch_adds_a_loop() {
    let spec = Spec::builtin_835().merge_patch(
        r#"{"loops":{"2100-ZZ":{"parent":"2100","trigger":{"segment":"ZZ1"},"occurrences":{"zz1":{"segment":"ZZ1","pos":0},"zz2":{"segment":"ZZ2","pos":1}}}}}"#,
    )
    .unwrap();
    assert_eq!(spec.loops().len(), 9);
    let zz = spec.loop_id("2100-ZZ").unwrap();
    assert_eq!(spec.get(zz).parent, spec.loop_id("2100"));
    assert!(spec.children(spec.loop_id("2100")).contains(&zz));
}
