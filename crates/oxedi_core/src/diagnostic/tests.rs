use super::*;
use crate::Delimiters;
use crate::value::ElementType;

fn path(loops: &[(&str, usize)]) -> Vec<LoopRef> {
    loops
        .iter()
        .map(|&(name, ordinal)| LoopRef {
            name: name.to_string(),
            ordinal,
        })
        .collect()
}

const TRANSACTION: &[(&str, usize)] = &[("interchange", 1), ("group", 1), ("transaction", 1)];

#[test]
fn every_rule_names_its_variant() {
    let id = || b"SE".to_vec();
    let rules = [
        (Rule::ByteOrderMark, "ByteOrderMark"),
        (Rule::UnknownSegment { segment_id: id() }, "UnknownSegment"),
        (
            Rule::ImplicitLoop {
                loop_name: "group".into(),
                expected_trigger: "\"GS\" with no conditions".into(),
                caused_by: b"ST".to_vec(),
            },
            "ImplicitLoop",
        ),
        (
            Rule::UnterminatedLoop {
                loop_name: "transaction".into(),
                expected_end: id(),
                opened_at: None,
            },
            "UnterminatedLoop",
        ),
        (
            Rule::ControlCountMismatch {
                segment_id: id(),
                element: 1,
                expected: 2,
                found: b"3".to_vec(),
            },
            "ControlCountMismatch",
        ),
        (
            Rule::ControlElementMissing {
                segment_id: id(),
                element: 1,
            },
            "ControlElementMissing",
        ),
        (
            Rule::ControlNumberMismatch {
                opener: b"ST".to_vec(),
                opener_element: 2,
                closer: id(),
                closer_element: 2,
                opener_value: b"1".to_vec(),
                closer_value: b"2".to_vec(),
                opened_at: Some(0),
            },
            "ControlNumberMismatch",
        ),
        (
            Rule::RequiredElementMissing {
                segment_id: id(),
                element: 1,
                component: None,
                name: "n".into(),
            },
            "RequiredElementMissing",
        ),
        (
            Rule::TypeMismatch {
                segment_id: id(),
                element: 1,
                component: None,
                name: "n".into(),
                expected: ElementType::N(0),
            },
            "TypeMismatch",
        ),
        (
            Rule::LengthOutOfRange {
                segment_id: id(),
                element: 1,
                component: None,
                name: "n".into(),
                min: Some(1),
                max: Some(2),
                length: 3,
            },
            "LengthOutOfRange",
        ),
        (
            Rule::ValueDropped {
                table: "t".into(),
                column: "c".into(),
                bytes: 1,
            },
            "ValueDropped",
        ),
        (
            Rule::CompositeShape {
                segment_id: id(),
                element: 1,
                name: "n".into(),
                declared: 1,
                found: 2,
            },
            "CompositeShape",
        ),
        (
            Rule::CodeNotInList {
                segment_id: id(),
                element: 1,
                component: None,
                name: "n".into(),
                codes: vec!["A".into(), "B".into()],
                occurrence: None,
            },
            "CodeNotInList",
        ),
        (
            Rule::RequiredOccurrenceMissing {
                loop_name: "l".into(),
                opened_at: None,
                occurrence: "o".into(),
                selector: "\"N3\"".into(),
            },
            "RequiredOccurrenceMissing",
        ),
        (
            Rule::OccurrenceOverMax {
                loop_name: "l".into(),
                occurrence: "o".into(),
                selector: "\"N3\"".into(),
                max: 1,
                count: 2,
            },
            "OccurrenceOverMax",
        ),
        (
            Rule::LoopOverMax {
                loop_name: "l".into(),
                parent: None,
                max: 1,
                count: 2,
            },
            "LoopOverMax",
        ),
        (
            Rule::OutOfOrder {
                loop_name: "l".into(),
                occurrence: "o".into(),
                pos: 1,
                after_loop: "l".into(),
                after: "p".into(),
                after_pos: 2,
            },
            "OutOfOrder",
        ),
        (
            Rule::UnknownOccurrence {
                loop_name: "l".into(),
                segment_id: id(),
                element: 1,
                component: None,
                occurrences: vec!["o".into()],
            },
            "UnknownOccurrence",
        ),
        (
            Rule::RequiredLoopMissing {
                loop_name: "l".into(),
                opened_at: None,
                child: "c".into(),
                expected_trigger: "\"N1\" with no conditions".into(),
            },
            "RequiredLoopMissing",
        ),
        (
            Rule::BalanceMismatch {
                rule: "r".into(),
                loop_name: "l".into(),
                opened_at: None,
                target: "t".into(),
                sum: "s".into(),
                expected: 0,
                computed: 1,
                scale: 2,
                segments: Vec::new(),
            },
            "BalanceMismatch",
        ),
    ];
    for (rule, kind) in rules {
        assert_eq!(rule.kind(), kind);
        assert!(format!("{rule:?}").starts_with(kind), "{rule:?}");
    }
}

#[test]
fn levels_display_as_snip_numbers() {
    assert_eq!(SnipLevel::L1.to_string(), "SNIP 1");
    assert_eq!(SnipLevel::L2.to_string(), "SNIP 2");
    assert_eq!(SnipLevel::L3.to_string(), "SNIP 3");
}

#[test]
fn a_loop_ref_displays_name_and_ordinal() {
    let at = LoopRef {
        name: "2100".into(),
        ordinal: 3,
    };
    assert_eq!(at.to_string(), "2100#3");
}

#[test]
fn a_loop_ref_quotes_a_name_that_holds_a_separator() {
    let at = LoopRef {
        name: "x/y".into(),
        ordinal: 2,
    };
    assert_eq!(at.to_string(), "\"x/y\"#2");
    let at = LoopRef {
        name: "x#2".into(),
        ordinal: 1,
    };
    assert_eq!(at.to_string(), "\"x#2\"#1");
}

#[test]
fn byte_order_mark_displays_the_first_segment_and_the_mark() {
    let diagnostic = Diagnostic::new(
        Rule::ByteOrderMark,
        Some(0),
        None,
        None,
        Vec::new(),
        b"\xEF\xBB\xBF".to_vec(),
    );
    assert_eq!(diagnostic.level, SnipLevel::L1);
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 1 · the input starts with a UTF-8 byte order mark, kept as leading trivia of the first segment · segment #0 · at the root · datum \"\\u{feff}\""
    );
}

#[test]
fn unknown_segment_displays_id_index_path_and_datum() {
    let diagnostic = Diagnostic::new(
        Rule::UnknownSegment {
            segment_id: b"XX".to_vec(),
        },
        Some(7),
        None,
        None,
        path(&[
            ("interchange", 1),
            ("group", 1),
            ("transaction", 1),
            ("1000A", 1),
        ]),
        b"XX".to_vec(),
    );
    assert_eq!(diagnostic.level, SnipLevel::L1);
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 1 · segment \"XX\" is not part of the structure: no open loop holds it and it opens no loop · segment #7 · at interchange#1/group#1/transaction#1/1000A#1 · datum \"XX\""
    );
}

#[test]
fn implicit_loop_displays_the_loop_and_the_segment_that_needed_it() {
    let diagnostic = Diagnostic::new(
        Rule::ImplicitLoop {
            loop_name: "group".into(),
            expected_trigger: "\"GS\" with no conditions".into(),
            caused_by: b"ST".to_vec(),
        },
        Some(0),
        None,
        None,
        path(&[("interchange", 1), ("group", 1)]),
        b"ST".to_vec(),
    );
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 1 · loop \"group\" opened without its own trigger (\"GS\" with no conditions) to hold segment \"ST\" · segment #0 · at interchange#1/group#1 · datum \"ST\""
    );
}

#[test]
fn unterminated_loop_displays_the_opener_the_expected_end_and_the_closing_segment() {
    let diagnostic = Diagnostic::new(
        Rule::UnterminatedLoop {
            loop_name: "transaction".into(),
            expected_end: b"SE".to_vec(),
            opened_at: Some(2),
        },
        Some(4),
        None,
        None,
        path(TRANSACTION),
        b"GE".to_vec(),
    );
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 1 · loop \"transaction\" opened at segment #2 closed without its end segment \"SE\" · segment #4 · at interchange#1/group#1/transaction#1 · datum \"GE\""
    );
    let implicit = Rule::UnterminatedLoop {
        loop_name: "transaction".into(),
        expected_end: b"SE".to_vec(),
        opened_at: None,
    };
    assert_eq!(
        implicit.to_string(),
        "loop \"transaction\" closed without its end segment \"SE\""
    );
}

#[test]
fn a_finding_at_the_end_of_the_stream_says_so() {
    let diagnostic = Diagnostic::new(
        Rule::UnterminatedLoop {
            loop_name: "interchange".into(),
            expected_end: b"IEA".to_vec(),
            opened_at: Some(0),
        },
        None,
        None,
        None,
        path(&[("interchange", 1)]),
        Vec::new(),
    );
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 1 · loop \"interchange\" opened at segment #0 closed without its end segment \"IEA\" · end of stream · at interchange#1 · datum \"\""
    );
}

#[test]
fn control_count_mismatch_displays_the_element_the_value_and_the_count() {
    let diagnostic = Diagnostic::new(
        Rule::ControlCountMismatch {
            segment_id: b"SE".to_vec(),
            element: 1,
            expected: 18,
            found: b"15".to_vec(),
        },
        Some(19),
        Some(1),
        None,
        path(TRANSACTION),
        b"15".to_vec(),
    );
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 1 · SE01 declares \"15\" but the count is 18 · segment #19, element 1 · at interchange#1/group#1/transaction#1 · datum \"15\""
    );
}

#[test]
fn control_element_missing_displays_the_segment_and_the_position() {
    let diagnostic = Diagnostic::new(
        Rule::ControlElementMissing {
            segment_id: b"SE".to_vec(),
            element: 2,
        },
        Some(4),
        Some(2),
        None,
        path(TRANSACTION),
        Vec::new(),
    );
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 1 · control element SE02 is missing: the segment has no element 2 · segment #4, element 2 · at interchange#1/group#1/transaction#1 · datum \"\""
    );
}

#[test]
fn control_number_mismatch_displays_both_elements_values_and_the_opener() {
    let diagnostic = Diagnostic::new(
        Rule::ControlNumberMismatch {
            opener: b"ST".to_vec(),
            opener_element: 2,
            closer: b"SE".to_vec(),
            closer_element: 2,
            opener_value: b"0001".to_vec(),
            closer_value: b"0002".to_vec(),
            opened_at: Some(2),
        },
        Some(4),
        Some(2),
        None,
        path(TRANSACTION),
        b"0002".to_vec(),
    );
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 1 · SE02 \"0002\" does not match ST02 \"0001\" of segment #2 · segment #4, element 2 · at interchange#1/group#1/transaction#1 · datum \"0002\""
    );
    let implicit = Rule::ControlNumberMismatch {
        opener: b"ST".to_vec(),
        opener_element: 2,
        closer: b"SE".to_vec(),
        closer_element: 2,
        opener_value: b"0001".to_vec(),
        closer_value: b"0002".to_vec(),
        opened_at: None,
    };
    assert_eq!(
        implicit.to_string(),
        "SE02 \"0002\" does not match ST02 \"0001\""
    );
}

#[test]
fn required_element_missing_displays_the_element_and_its_name() {
    let diagnostic = Diagnostic::new(
        Rule::RequiredElementMissing {
            segment_id: b"CLP".to_vec(),
            element: 1,
            component: None,
            name: "claim_submitter_identifier".into(),
        },
        Some(12),
        Some(1),
        None,
        path(&[("transaction", 1), ("2000", 1), ("2100", 1)]),
        Vec::new(),
    );
    assert_eq!(
        diagnostic.level.to_string(),
        "SNIP 2",
        "element rules are level 2"
    );
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 2 · required element CLP01 (claim_submitter_identifier) is missing or empty · segment #12, element 1 · at transaction#1/2000#1/2100#1 · datum \"\""
    );
}

#[test]
fn type_mismatch_displays_the_component_and_the_declared_type() {
    let diagnostic = Diagnostic::new(
        Rule::TypeMismatch {
            segment_id: b"CLP".to_vec(),
            element: 3,
            component: None,
            name: "total_claim_charge_amount".into(),
            expected: ElementType::R { scale: 2 },
        },
        Some(12),
        Some(3),
        None,
        path(&[("2100", 2)]),
        b"12A".to_vec(),
    );
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 2 · element CLP03 (total_claim_charge_amount) is not a valid R (decimal, scale 2) · segment #12, element 3 · at 2100#2 · datum \"12A\""
    );
    let component = Rule::TypeMismatch {
        segment_id: b"SVC".to_vec(),
        element: 1,
        component: Some(1),
        name: "product_or_service_id_qualifier".into(),
        expected: ElementType::Id,
    };
    assert_eq!(
        component.to_string(),
        "element SVC01-1 (product_or_service_id_qualifier) is not a valid ID (code)"
    );
}

#[test]
fn length_out_of_range_displays_the_length_and_the_bounds() {
    let rule = |min, max| Rule::LengthOutOfRange {
        segment_id: b"CLP".to_vec(),
        element: 1,
        component: None,
        name: "claim_submitter_identifier".into(),
        min,
        max,
        length: 40,
    };
    let diagnostic = Diagnostic::new(
        rule(Some(1), Some(38)),
        Some(12),
        Some(1),
        None,
        Vec::new(),
        b"0123456789012345678901234567890123456789".to_vec(),
    );
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 2 · element CLP01 (claim_submitter_identifier) has length 40; the spec allows 1 to 38 · segment #12, element 1 · at the root · datum \"0123456789012345678901234567890123456789\""
    );
    assert!(
        rule(Some(41), None)
            .to_string()
            .ends_with("allows at least 41")
    );
    assert!(
        rule(None, Some(38))
            .to_string()
            .ends_with("allows at most 38")
    );
    assert!(rule(None, None).to_string().ends_with("allows any length"));
}

fn codes(count: usize) -> Vec<String> {
    ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"][..count]
        .iter()
        .map(|c| c.to_string())
        .collect()
}

#[test]
fn code_not_in_list_displays_the_element_the_codes_and_the_value() {
    let rule = |component, codes| Rule::CodeNotInList {
        segment_id: b"SVC".to_vec(),
        element: 1,
        component,
        name: "qualifier".into(),
        codes,
        occurrence: None,
    };
    let diagnostic = Diagnostic::new(
        rule(Some(1), codes(3)),
        Some(17),
        Some(1),
        Some(1),
        path(&[("2110", 3)]),
        b"ZZ".to_vec(),
    );
    assert_eq!(diagnostic.level, SnipLevel::L2);
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 2 · element SVC01-1 (qualifier) is not one of the 3 codes the spec lists for it (\"A\", \"B\", \"C\") · segment #17, element 1, component 1 · at 2110#3 · datum \"ZZ\""
    );
    assert_eq!(
        rule(None, vec!["005010X221A1".into()]).to_string(),
        "element SVC01 (qualifier) is not the one code the spec lists for it (\"005010X221A1\")"
    );
    assert_eq!(
        rule(None, codes(5)).to_string(),
        "element SVC01 (qualifier) is not one of the 5 codes the spec lists for it (\"A\", \"B\", \"C\", \"D\", \"E\")"
    );
    assert_eq!(
        rule(None, codes(6)).to_string(),
        "element SVC01 (qualifier) is not one of the 6 codes the spec lists for it"
    );
    assert_eq!(
        rule(None, vec!["a\"b".into(), "\n".into()]).to_string(),
        "element SVC01 (qualifier) is not one of the 2 codes the spec lists for it (\"a\\\"b\", \"\\n\")"
    );
}

#[test]
fn code_not_in_list_names_the_occurrence_whose_list_it_is() {
    let rule = |codes| Rule::CodeNotInList {
        segment_id: b"NM1".to_vec(),
        element: 8,
        component: None,
        name: "identification_code_qualifier".into(),
        codes,
        occurrence: Some("patient_name".into()),
    };
    let diagnostic = Diagnostic::new(
        rule(codes(2)),
        Some(23),
        Some(8),
        None,
        path(&[("2000", 1), ("2100", 4)]),
        b"ZZ".to_vec(),
    );
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 2 · element NM108 (identification_code_qualifier) is not one of the 2 codes the spec lists for it in occurrence \"patient_name\" (\"A\", \"B\") · segment #23, element 8 · at 2000#1/2100#4 · datum \"ZZ\""
    );
    assert_eq!(
        rule(vec!["MI".into()]).to_string(),
        "element NM108 (identification_code_qualifier) is not the one code the spec lists for it in occurrence \"patient_name\" (\"MI\")"
    );
}

#[test]
fn required_occurrence_missing_displays_the_loop_its_opener_and_the_occurrence() {
    let rule = |opened_at| Rule::RequiredOccurrenceMissing {
        loop_name: "1000A".into(),
        opened_at,
        occurrence: "payer_technical_contact_information".into(),
        selector: "\"PER\" where PER01 is \"BL\"".into(),
    };
    let diagnostic = Diagnostic::new(
        rule(Some(5)),
        Some(9),
        None,
        None,
        path(&[
            ("interchange", 1),
            ("group", 1),
            ("transaction", 1),
            ("1000A", 1),
        ]),
        b"N1".to_vec(),
    );
    assert_eq!(diagnostic.level, SnipLevel::L2);
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 2 · loop \"1000A\" opened at segment #5 closed without its required occurrence \"payer_technical_contact_information\" (\"PER\" where PER01 is \"BL\") · segment #9 · at interchange#1/group#1/transaction#1/1000A#1 · datum \"N1\""
    );
    assert_eq!(
        rule(None).to_string(),
        "loop \"1000A\" closed without its required occurrence \"payer_technical_contact_information\" (\"PER\" where PER01 is \"BL\")"
    );
}

#[test]
fn occurrence_over_max_displays_the_occurrence_the_count_and_the_maximum() {
    let diagnostic = Diagnostic::new(
        Rule::OccurrenceOverMax {
            loop_name: "2100".into(),
            occurrence: "other_claim_related_identification".into(),
            selector: "\"REF\" where REF01 is one of 14 codes".into(),
            max: 5,
            count: 6,
        },
        Some(40),
        None,
        None,
        path(&[("2000", 1), ("2100", 2)]),
        b"REF".to_vec(),
    );
    assert_eq!(diagnostic.level, SnipLevel::L2);
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 2 · occurrence \"other_claim_related_identification\" (\"REF\" where REF01 is one of 14 codes) of loop \"2100\" appears 6 times in one instance; the spec allows at most 5 · segment #40 · at 2000#1/2100#2 · datum \"REF\""
    );
}

#[test]
fn loop_over_max_displays_the_parent_or_the_root() {
    let rule = |parent: Option<&str>| Rule::LoopOverMax {
        loop_name: "1000A".into(),
        parent: parent.map(Into::into),
        max: 1,
        count: 2,
    };
    let diagnostic = Diagnostic::new(
        rule(Some("transaction")),
        Some(12),
        None,
        None,
        path(&[("transaction", 1), ("1000A", 2)]),
        b"N1".to_vec(),
    );
    assert_eq!(diagnostic.level, SnipLevel::L2);
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 2 · loop \"1000A\" has 2 instances under one instance of loop \"transaction\"; the spec allows at most 1 · segment #12 · at transaction#1/1000A#2 · datum \"N1\""
    );
    assert_eq!(
        rule(None).to_string(),
        "loop \"1000A\" has 2 instances at the root; the spec allows at most 1"
    );
}

#[test]
fn out_of_order_displays_both_occurrences_their_positions_and_loops() {
    let diagnostic = Diagnostic::new(
        Rule::OutOfOrder {
            loop_name: "transaction".into(),
            occurrence: "receiver_identification".into(),
            pos: 10600,
            after_loop: "1000A".into(),
            after: "payer_identification".into(),
            after_pos: 10800,
        },
        Some(8),
        None,
        None,
        path(&[("transaction", 1)]),
        b"REF".to_vec(),
    );
    assert_eq!(diagnostic.level, SnipLevel::L2);
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 2 · occurrence \"receiver_identification\" (position 10600) of loop \"transaction\" comes after occurrence \"payer_identification\" (position 10800) of loop \"1000A\" · segment #8 · at transaction#1 · datum \"REF\""
    );
}

#[test]
fn unknown_occurrence_displays_the_qualifier_and_the_occurrences() {
    let rule = |names: &[&str], component| Rule::UnknownOccurrence {
        loop_name: "2100".into(),
        segment_id: b"NM1".to_vec(),
        element: 1,
        component,
        occurrences: names.iter().map(|name| name.to_string()).collect(),
    };
    let diagnostic = Diagnostic::new(
        rule(&["patient_name", "insured_name"], None),
        Some(21),
        Some(1),
        None,
        path(&[("2000", 1), ("2100", 1)]),
        b"XX".to_vec(),
    );
    assert_eq!(diagnostic.level, SnipLevel::L2);
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 2 · segment \"NM1\" matches none of the 2 occurrences loop \"2100\" declares for it (\"patient_name\", \"insured_name\"): NM101 holds none of their qualifier codes · segment #21, element 1 · at 2000#1/2100#1 · datum \"XX\""
    );
    assert_eq!(
        rule(&["a", "b", "c", "d", "e", "f"], Some(2)).to_string(),
        "segment \"NM1\" matches none of the 6 occurrences loop \"2100\" declares for it: NM101-2 holds none of their qualifier codes"
    );
}

#[test]
fn required_loop_missing_displays_the_loop_its_opener_and_the_child() {
    let rule = |opened_at| Rule::RequiredLoopMissing {
        loop_name: "transaction".into(),
        opened_at,
        child: "1000A".into(),
        expected_trigger: "\"N1\" where {1: \"PR\"}".into(),
    };
    let diagnostic = Diagnostic::new(
        rule(Some(2)),
        None,
        None,
        None,
        path(&[("interchange", 1), ("group", 1), ("transaction", 1)]),
        Vec::new(),
    );
    assert_eq!(diagnostic.level, SnipLevel::L2);
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 2 · loop \"transaction\" opened at segment #2 closed without its required child loop \"1000A\" (trigger \"N1\" where {1: \"PR\"}) · end of stream · at interchange#1/group#1/transaction#1 · datum \"\""
    );
    assert_eq!(
        rule(None).to_string(),
        "loop \"transaction\" closed without its required child loop \"1000A\" (trigger \"N1\" where {1: \"PR\"})"
    );
}

#[test]
fn balance_mismatch_displays_the_rule_both_amounts_and_the_segments_read() {
    let rule = |opened_at, segments| {
        Rule::BalanceMismatch {
        rule: "claim_balance".into(),
        loop_name: "2100".into(),
        opened_at,
        target: "CLP03 of 2100 \"claim_payment_information\" - CLP04 of 2100 \"claim_payment_information\"".into(),
        sum: "sum of CAS03, CAS06 of 2100 \"claim_adjustment\"".into(),
        expected: 30000,
        computed: -1250,
        scale: 2,
        segments,
    }
    };
    let diagnostic = Diagnostic::new(
        rule(Some(16), vec![16, 17]),
        Some(16),
        Some(3),
        None,
        path(&[
            ("interchange", 1),
            ("group", 1),
            ("transaction", 1),
            ("2000", 1),
            ("2100", 1),
        ]),
        b"800.00".to_vec(),
    );
    assert_eq!(diagnostic.level, SnipLevel::L3);
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 3 · balancing rule \"claim_balance\" fails in loop \"2100\" opened at segment #16: CLP03 of 2100 \"claim_payment_information\" - CLP04 of 2100 \"claim_payment_information\" is 300.00, but sum of CAS03, CAS06 of 2100 \"claim_adjustment\" adds up to -12.50 (off by 312.50); read from segments #16, #17 · segment #16, element 3 · at interchange#1/group#1/transaction#1/2000#1/2100#1 · datum \"800.00\""
    );
    assert_eq!(
        rule(None, (1..=12).collect()).to_string(),
        "balancing rule \"claim_balance\" fails in loop \"2100\" opened implicitly: CLP03 of 2100 \"claim_payment_information\" - CLP04 of 2100 \"claim_payment_information\" is 300.00, but sum of CAS03, CAS06 of 2100 \"claim_adjustment\" adds up to -12.50 (off by 312.50); read from segments #1, #2, #3, #4, #5, #6, #7, #8, #9, #10 and 2 more"
    );
}

#[test]
fn composite_shape_displays_found_and_declared_components() {
    let diagnostic = Diagnostic::new(
        Rule::CompositeShape {
            segment_id: b"SVC".to_vec(),
            element: 1,
            name: "composite_medical_procedure".into(),
            declared: 8,
            found: 9,
        },
        Some(17),
        Some(1),
        Some(9),
        path(&[("2110", 1)]),
        b"X".to_vec(),
    );
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 2 · element SVC01 (composite_medical_procedure) has 9 components; the spec declares 8 · segment #17, element 1, component 9 · at 2110#1 · datum \"X\""
    );
}

#[test]
fn value_dropped_displays_the_table_the_column_the_byte_total_and_the_value() {
    let diagnostic = Diagnostic::new(
        Rule::ValueDropped {
            table: "claims".into(),
            column: "note".into(),
            bytes: 2147483650,
        },
        Some(9),
        None,
        None,
        path(&[("2100", 4)]),
        b"a long note".to_vec(),
    );
    assert_eq!(
        diagnostic.level.to_string(),
        "SNIP 2",
        "a dropped value is a level 2 finding"
    );
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 2 · text for column \"note\" of table \"claims\" was not stored: it would bring the column to 2147483650 bytes and a column holds at most 2147483647 · segment #9 · at 2100#4 · datum \"a long note\""
    );
}

#[test]
fn invalid_utf8_in_a_datum_is_shown_as_hex_escapes() {
    let diagnostic = Diagnostic::new(
        Rule::UnknownSegment {
            segment_id: vec![b'Z', 0xFF],
        },
        Some(1),
        None,
        None,
        Vec::new(),
        vec![b'Z', 0xFF],
    );
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 1 · segment \"Z\\xFF\" is not part of the structure: no open loop holds it and it opens no loop · segment #1 · at the root · datum \"Z\\xFF\""
    );
    let mixed = Quoted(b"a\"b\n\xC3\xA9\xE9\x80c");
    assert_eq!(mixed.to_string(), "\"a\\\"b\\n\u{e9}\\xE9\\x80c\"");
}

#[test]
fn span_resolves_the_segment_bytes_from_the_document() {
    let document =
        Document::with_delimiters(&b"AA*1~BB*2~"[..], Delimiters::new(b'*', b':', b'~')).unwrap();
    let at = |segment| {
        Diagnostic::new(
            Rule::UnknownSegment {
                segment_id: b"BB".to_vec(),
            },
            segment,
            None,
            None,
            Vec::new(),
            b"BB".to_vec(),
        )
    };
    let span = at(Some(1)).span(&document).unwrap();
    assert_eq!(&document.as_bytes()[span.raw], b"BB*2~");
    assert_eq!(at(Some(9)).span(&document), None);
    assert_eq!(at(None).span(&document), None);
}

#[test]
fn external_displays_the_message_the_origin_and_the_code() {
    let rule = |code: Option<&str>| Rule::External {
        origin: "pyx12".into(),
        code: code.map(Into::into),
        message: "Mandatory data element missing".into(),
        level: SnipLevel::L2,
    };
    let diagnostic = Diagnostic::new(
        rule(Some("1")),
        Some(17),
        Some(2),
        None,
        Vec::new(),
        b"".to_vec(),
    );
    assert_eq!(diagnostic.level, SnipLevel::L2);
    assert_eq!(diagnostic.rule.kind(), "External");
    assert_eq!(
        diagnostic.to_string(),
        "SNIP 2 · Mandatory data element missing (reported by pyx12, code 1) · segment #17, element 2 · at the root · datum \"\""
    );
    assert_eq!(
        rule(None).to_string(),
        "Mandatory data element missing (reported by pyx12)"
    );
}

#[test]
fn flat_of_a_parser_rule() {
    let diagnostic = Diagnostic::new(
        Rule::UnknownSegment {
            segment_id: b"XX".to_vec(),
        },
        Some(7),
        None,
        None,
        path(&[("1000A", 1), ("2100", 2)]),
        b"XX".to_vec(),
    );
    let flat = diagnostic.flat();
    assert_eq!(flat.level, 1);
    assert_eq!(flat.kind, "UnknownSegment");
    assert_eq!(flat.rule, diagnostic.rule.to_string());
    assert_eq!(flat.segment, Some(7));
    assert_eq!(flat.path, "1000A#1/2100#2");
    assert_eq!(flat.datum, b"XX");
    assert_eq!(flat.origin, "oxedi");
    assert_eq!(flat.code, None);
}

#[test]
fn flat_of_an_external_finding() {
    let diagnostic = Diagnostic::new(
        Rule::External {
            origin: "pyx12".into(),
            code: Some("8".into()),
            message: "Mandatory data element missing".into(),
            level: SnipLevel::L2,
        },
        Some(3),
        Some(2),
        Some(1),
        Vec::new(),
        b"".to_vec(),
    );
    let flat = diagnostic.flat();
    assert_eq!(flat.level, 2);
    assert_eq!(flat.kind, "External");
    assert_eq!((flat.element, flat.component), (Some(2), Some(1)));
    assert_eq!(flat.path, "");
    assert_eq!(flat.origin, "pyx12");
    assert_eq!(flat.code, Some("8"));
}

#[test]
fn origin_and_code_read_the_rule_without_formatting_it() {
    let own = Diagnostic::new(
        Rule::ByteOrderMark,
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
    );
    assert_eq!((own.origin(), own.code()), (PARSER_ORIGIN, None));
    let external = Diagnostic::new(
        Rule::External {
            origin: "pyx12".into(),
            code: Some("8".into()),
            message: "m".into(),
            level: SnipLevel::L2,
        },
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
    );
    assert_eq!((external.origin(), external.code()), ("pyx12", Some("8")));
}

#[test]
fn path_text_joins_the_open_loops() {
    let at = |loops: &[(&str, usize)]| {
        Diagnostic::new(
            Rule::UnknownSegment {
                segment_id: b"XX".to_vec(),
            },
            Some(0),
            None,
            None,
            path(loops),
            b"XX".to_vec(),
        )
        .path_text()
    };
    assert_eq!(at(&[]), "");
    assert_eq!(at(&[("a", 1)]), "a#1");
    assert_eq!(at(&[("a", 1), ("b", 2)]), "a#1/b#2");
}

#[test]
fn code_and_occurrence_lists_are_spelled_out_up_to_the_limit() {
    let code_rule = |count| Rule::CodeNotInList {
        segment_id: b"SVC".to_vec(),
        element: 1,
        component: None,
        name: "qualifier".into(),
        codes: codes(count),
        occurrence: None,
    };
    assert_eq!(
        code_rule(4).to_string(),
        "element SVC01 (qualifier) is not one of the 4 codes the spec lists for it (\"A\", \"B\", \"C\", \"D\")"
    );
    assert_eq!(
        code_rule(5).to_string(),
        "element SVC01 (qualifier) is not one of the 5 codes the spec lists for it (\"A\", \"B\", \"C\", \"D\", \"E\")"
    );
    assert_eq!(
        code_rule(6).to_string(),
        "element SVC01 (qualifier) is not one of the 6 codes the spec lists for it"
    );
    let occurrence_rule = |count| Rule::UnknownOccurrence {
        loop_name: "2100".into(),
        segment_id: b"NM1".to_vec(),
        element: 1,
        component: None,
        occurrences: codes(count),
    };
    assert_eq!(
        occurrence_rule(4).to_string(),
        "segment \"NM1\" matches none of the 4 occurrences loop \"2100\" declares for it (\"A\", \"B\", \"C\", \"D\"): NM101 holds none of their qualifier codes"
    );
    assert_eq!(
        occurrence_rule(5).to_string(),
        "segment \"NM1\" matches none of the 5 occurrences loop \"2100\" declares for it (\"A\", \"B\", \"C\", \"D\", \"E\"): NM101 holds none of their qualifier codes"
    );
    assert_eq!(
        occurrence_rule(6).to_string(),
        "segment \"NM1\" matches none of the 6 occurrences loop \"2100\" declares for it: NM101 holds none of their qualifier codes"
    );
}

#[test]
fn a_balance_lists_ten_segments_then_counts_the_rest() {
    let rule = |count: usize| Rule::BalanceMismatch {
        rule: "r".into(),
        loop_name: "2100".into(),
        opened_at: Some(1),
        target: "CLP03".into(),
        sum: "CAS03".into(),
        expected: 5,
        computed: 0,
        scale: 2,
        segments: (1..=count).collect(),
    };
    let head = "balancing rule \"r\" fails in loop \"2100\" opened at segment #1: CLP03 is 0.05, but CAS03 adds up to 0.00 (off by 0.05); read from segments ";
    assert_eq!(
        rule(9).to_string(),
        format!("{head}#1, #2, #3, #4, #5, #6, #7, #8, #9")
    );
    assert_eq!(
        rule(10).to_string(),
        format!("{head}#1, #2, #3, #4, #5, #6, #7, #8, #9, #10")
    );
    assert_eq!(
        rule(11).to_string(),
        format!("{head}#1, #2, #3, #4, #5, #6, #7, #8, #9, #10 and 1 more")
    );
}

#[test]
fn a_balance_whose_difference_overflows_leaves_it_out() {
    let rule = Rule::BalanceMismatch {
        rule: "r".into(),
        loop_name: "2100".into(),
        opened_at: None,
        target: "CLP03".into(),
        sum: "CAS03".into(),
        expected: i128::MIN,
        computed: 1,
        scale: 0,
        segments: vec![3],
    };
    assert_eq!(
        rule.to_string(),
        format!(
            "balancing rule \"r\" fails in loop \"2100\" opened implicitly: CLP03 is {}, but CAS03 adds up to 1; read from segments #3",
            i128::MIN
        )
    );
}
