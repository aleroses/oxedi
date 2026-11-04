//! Loading a spec and the accessors over it.

use super::segs;
use crate::spec::*;
use crate::value::ElementType;
use std::collections::BTreeMap;

#[test]
fn builtin_835_loads() {
    let spec = Spec::builtin_835();
    assert_eq!(spec.name(), "835");
    assert_eq!(spec.loops().len(), 8);
    let interchange = spec.loop_id("interchange").unwrap();
    assert_eq!(spec.roots(), &[interchange]);
    let transaction = spec.loop_id("transaction").unwrap();
    let names: Vec<_> = spec
        .children(Some(transaction))
        .iter()
        .map(|&c| spec.loop_name(c))
        .collect();
    assert_eq!(names, vec!["1000A", "1000B", "2000"]);
    assert_eq!(
        spec.get(spec.loop_id("2110").unwrap()).parent,
        spec.loop_id("2100")
    );
    assert_eq!(spec.get(transaction).end.as_deref(), Some(&b"SE"[..]));
}

#[test]
fn builtin_835_defines_every_segment_its_loops_name() {
    let spec = Spec::builtin_835();
    for def in spec.loops() {
        let ids = std::iter::once(&def.trigger.segment)
            .chain(&def.segments)
            .chain(def.end.as_ref());
        for id in ids {
            assert!(
                spec.segment(id).is_some(),
                "loop {} names {} with no definition",
                def.name,
                String::from_utf8_lossy(id)
            );
        }
    }
    let ids: Vec<String> = spec
        .segments()
        .map(|(id, _)| String::from_utf8_lossy(id).into_owned())
        .collect();
    assert_eq!(
        ids,
        vec![
            "AMT", "BPR", "CAS", "CLP", "CUR", "DTM", "GE", "GS", "IEA", "ISA", "LQ", "LX", "MIA",
            "MOA", "N1", "N3", "N4", "NM1", "PER", "PLB", "QTY", "RDM", "REF", "SE", "ST", "SVC",
            "TA1", "TRN", "TS2", "TS3"
        ]
    );
}

#[test]
fn builtin_835_element_names_are_unique_among_siblings() {
    fn check(at: &str, elements: &BTreeMap<usize, ElementDef>) {
        let mut seen = std::collections::BTreeSet::new();
        for (position, def) in elements {
            assert!(
                seen.insert(def.name.as_str()),
                "{at}: name {} repeats at position {position}",
                def.name
            );
            check(&format!("{at}{position:02}"), &def.composite);
        }
    }
    for (id, def) in Spec::builtin_835().segments() {
        check(&String::from_utf8_lossy(id), &def.elements);
    }
}

#[test]
fn builtin_835_types_the_elements_the_envelope_checks_rely_on() {
    let spec = Spec::builtin_835();
    let element = |id: &[u8], position: usize| &spec.segment(id).unwrap().elements[&position];
    assert_eq!(element(b"CLP", 1).name, "claim_submitter_identifier");
    assert_eq!(element(b"CLP", 3).kind, ElementType::R { scale: 2 });
    assert_eq!(element(b"SE", 1).kind, ElementType::N(0));
    assert_eq!(element(b"ISA", 13).kind, ElementType::N(0));
    assert_eq!(element(b"DTM", 2).kind, ElementType::Dt);
    let procedure = element(b"SVC", 1);
    assert!(procedure.required);
    assert_eq!(procedure.composite.len(), 8);
    assert_eq!(procedure.composite[&2].name, "procedure_code");
}

#[test]
fn builtin_835_declares_five_tables_and_how_they_nest() {
    let spec = Spec::builtin_835();
    let names: Vec<&str> = spec.tables().iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "adjustments",
            "claims",
            "payments",
            "provider_adjustments",
            "services"
        ]
    );
    let above = |name: &str| -> Vec<&str> {
        spec.table(name)
            .unwrap()
            .ancestors
            .iter()
            .map(|&i| spec.tables()[i].name.as_str())
            .collect()
    };
    assert!(above("payments").is_empty());
    assert_eq!(above("claims"), vec!["payments"]);
    assert_eq!(above("services"), vec!["payments", "claims"]);
    assert_eq!(above("adjustments"), vec!["payments", "claims", "services"]);
    assert_eq!(above("provider_adjustments"), vec!["payments"]);
    let references: Vec<&str> = ["payments", "claims", "services"]
        .iter()
        .map(|name| spec.table(name).unwrap().reference.as_str())
        .collect();
    assert_eq!(references, vec!["payment", "claim", "service"]);
    let loops = |name: &str| -> Vec<&str> {
        spec.table(name)
            .unwrap()
            .loops
            .iter()
            .map(|&id| spec.loop_name(id))
            .collect()
    };
    assert_eq!(loops("adjustments"), vec!["2100", "2110"]);
    assert_eq!(loops("provider_adjustments"), vec!["transaction"]);
    let adjustments = spec.table("adjustments").unwrap();
    assert_eq!(adjustments.segment.as_deref(), Some(&b"CAS"[..]));
    assert_eq!(adjustments.repeat, Some(Repeat { from: 2, step: 3 }));
    let plb = spec.table("provider_adjustments").unwrap();
    assert_eq!(plb.segment.as_deref(), Some(&b"PLB"[..]));
    assert_eq!(plb.repeat, Some(Repeat { from: 3, step: 2 }));
    let counts: Vec<usize> = spec.tables().iter().map(|t| t.columns.len()).collect();
    assert_eq!(counts, vec![4, 25, 27, 5, 9]);
}

#[test]
fn builtin_835_columns_read_elements_the_spec_defines_in_loops_that_hold_them() {
    let spec = Spec::builtin_835();
    for table in spec.tables() {
        for (column, source) in &table.columns {
            let at = format!("{}.{column}", table.name);
            match source {
                ColumnSource::Element {
                    loop_id,
                    segment,
                    element,
                    component,
                    ..
                } => {
                    assert!(
                        spec.element_def(segment, *element, *component).is_some(),
                        "{at} reads an element the spec does not define"
                    );
                    let readers = loop_id.map_or(table.loops.clone(), |id| vec![id]);
                    for id in readers {
                        let def = spec.get(id);
                        assert!(
                            def.trigger.segment == *segment || def.accepts(segment),
                            "{at}: loop {} does not hold {}",
                            def.name,
                            String::from_utf8_lossy(segment)
                        );
                    }
                }
                ColumnSource::SegmentIndex { .. } => {}
                ColumnSource::GroupElement { offset, component } => {
                    let (Some(segment), Some(repeat)) = (&table.segment, table.repeat) else {
                        panic!("{at}: a group column outside a repeating table");
                    };
                    assert!(
                        spec.element_def(segment, repeat.from + offset, *component)
                            .is_some(),
                        "{at} reads a group element the spec does not define"
                    );
                }
            }
        }
    }
}

#[test]
fn ancestors_into_reuses_the_buffer_and_matches_ancestors() {
    let spec = Spec::builtin_835();
    let mut chain = vec![spec.loop_id("2110").unwrap(); 9];
    for name in ["2110", "interchange", "transaction"] {
        let id = spec.loop_id(name).unwrap();
        spec.ancestors_into(id, &mut chain);
        assert_eq!(chain, spec.ancestors(id), "{name}");
    }
}

#[test]
fn ancestors_are_listed_root_first() {
    let spec = Spec::builtin_835();
    let chain: Vec<_> = spec
        .ancestors(spec.loop_id("2110").unwrap())
        .iter()
        .map(|&c| spec.loop_name(c))
        .collect();
    assert_eq!(
        chain,
        vec!["interchange", "group", "transaction", "2000", "2100"]
    );
    assert!(
        spec.ancestors(spec.loop_id("interchange").unwrap())
            .is_empty()
    );
}

#[test]
fn trigger_conditions_are_checked_against_elements() {
    let spec = Spec::builtin_835();
    let segments = segs(b"N1*PR*PAYER~N1*PE*PAYEE~N1*TT*OTHER~");
    let transaction = spec.loop_id("transaction");
    assert_eq!(
        spec.matching_child(transaction, &segments[0]),
        spec.loop_id("1000A")
    );
    assert_eq!(
        spec.matching_child(transaction, &segments[1]),
        spec.loop_id("1000B")
    );
    assert_eq!(spec.matching_child(transaction, &segments[2]), None);
    assert_eq!(
        spec.matching_child(None, &segments[0]),
        None,
        "N1 is not a root trigger"
    );
}

#[test]
fn parent_cycle_is_rejected() {
    let err = Spec::from_json(
        r#"{"name":"t","loops":{
                "a":{"parent":"b","trigger":{"segment":"AA"}},
                "b":{"parent":"a","trigger":{"segment":"BB"}}
            }}"#,
    )
    .unwrap_err();
    assert!(matches!(err, SpecError::Cycle { .. }), "{err}");
}

#[test]
fn cycle_lists_its_members_from_the_repeated_loop() {
    let err = Spec::from_json(
        r#"{"name":"t","loops":{
                "a":{"parent":"b","trigger":{"segment":"AA"}},
                "b":{"parent":"c","trigger":{"segment":"BB"}},
                "c":{"parent":"b","trigger":{"segment":"CC"}}
            }}"#,
    )
    .unwrap_err();
    assert!(
        matches!(&err, SpecError::Cycle { members } if members == &["b", "c"]),
        "{err:?}"
    );
    assert_eq!(err.to_string(), "loops form a parent cycle: b -> c -> b");
}

#[test]
fn a_loop_that_is_its_own_parent_is_a_cycle() {
    let err =
        Spec::from_json(r#"{"name":"t","loops":{"a":{"parent":"a","trigger":{"segment":"AA"}}}}"#)
            .unwrap_err();
    assert!(
        matches!(&err, SpecError::Cycle { members } if members == &["a"]),
        "{err:?}"
    );
    assert_eq!(err.to_string(), "loops form a parent cycle: a -> a");
}

#[test]
fn a_misspelt_loop_key_is_named_with_the_loop() {
    let err = Spec::from_json(
        r#"{"name":"t","loops":{
                "2000":{"trigger":{"segment":"LX"}},
                "2100":{"parent":"2000","trigger":{"segment":"CLP"},"segmnts":["DTM"]}
            }}"#,
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "spec: unknown key \"segmnts\" at loops.2100; the allowed keys are: control, end, max, occurrences, parent, trigger, usage"
    );
}

#[test]
fn several_top_level_loops_are_allowed() {
    let spec = Spec::from_json(
        r#"{"name":"t","loops":{"a":{"trigger":{"segment":"AA"}},"b":{"trigger":{"segment":"BB"}}}}"#,
    )
    .unwrap();
    assert_eq!(spec.roots().len(), 2);
}

#[test]
fn empty_segment_ids_are_rejected_with_the_loop_and_the_key() {
    let cases = [
        (r#"{"trigger":{"segment":""}}"#, "trigger.segment"),
        (
            r#"{"trigger":{"segment":"AA"},"occurrences":{"aa":{"segment":"AA","pos":0},"a1":{"segment":"","pos":1}}}"#,
            "occurrences.a1.segment",
        ),
        (r#"{"trigger":{"segment":"AA"},"end":""}"#, "end"),
    ];
    for (def, expected_key) in cases {
        let json = format!(r#"{{"name":"t","loops":{{"a":{def}}}}}"#);
        let err = Spec::from_json(&json).unwrap_err();
        assert!(
            matches!(&err, SpecError::EmptySegmentId { loop_name: Some(name), key } if name == "a" && key == expected_key),
            "{def}: {err:?}"
        );
    }
}

#[test]
fn the_builtin_specs_define_the_same_loop_names() {
    let names = |spec: &Spec| -> std::collections::BTreeSet<String> {
        spec.loops().iter().map(|l| l.name.clone()).collect()
    };
    let five = names(&Spec::builtin_835());
    let four = names(&Spec::builtin_835_4010());
    assert!(!five.is_empty());
    assert_eq!(five, four);
}
