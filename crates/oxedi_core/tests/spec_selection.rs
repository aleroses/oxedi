//! Every sample and fixture gets the built-in spec of the version its group
//! header declares, and the 5010 spec when it declares none the specs know.

mod common;

use oxedi_core::{Diagnostic, Document, Processor, Rule, Spec};

fn selected(bytes: Vec<u8>) -> String {
    let five = Spec::builtin_835();
    let four = Spec::builtin_835_4010();
    let document = Document::parse(bytes).unwrap();
    let chosen = Spec::select(&[&five, &four], &five, document.segments());
    if std::ptr::eq(chosen, &four) {
        "4010".into()
    } else {
        assert!(std::ptr::eq(chosen, &five));
        "5010".into()
    }
}

#[test]
fn samples_get_the_spec_of_their_declared_version() {
    let cases = [
        ("edi835_test_davisvision.RMT", "4010"),
        ("edi835_test_eyemed.RMT", "4010"),
        ("edi835_test_file.RMT", "4010"),
        ("edi835_test_not_available_claim_id.RMT", "4010"),
        ("edi835_test_versant.RMT", "4010"),
        ("edi835_test_united.rmt", "5010"),
    ];
    let named: Vec<&str> = cases.iter().map(|(name, _)| *name).collect();
    common::assert_same_names("the cases here vs common::SAMPLES", &named, common::SAMPLES);
    for (name, expected) in cases {
        assert_eq!(selected(common::load_sample(name)), expected, "{name}");
    }
}

#[test]
fn fixtures_get_the_spec_of_their_declared_version() {
    // `004010` alone (no implementation guide) is still the 4010 835.
    let cases = [
        ("emedny_sample.txt", "5010"),
        ("united_healthcare_legacy_sample.txt", "5010"),
        ("multi_claim_sample.txt", "4010"),
        ("trizetto_sample.rmt", "4010"),
        ("balanced_5010_sample.txt", "5010"),
        ("balanced_4010_sample.txt", "4010"),
        ("edge_cases_5010_sample.txt", "5010"),
        ("edge_cases_4010_sample.txt", "4010"),
        ("masked_findings_5010_sample.txt", "5010"),
        ("masked_findings_4010_sample.txt", "4010"),
    ];
    let named: Vec<&str> = cases.iter().map(|(name, _)| *name).collect();
    common::assert_same_names(
        "the cases here vs common::FIXTURES",
        &named,
        common::FIXTURES,
    );
    for (name, expected) in cases {
        assert_eq!(selected(common::load_fixture(name)), expected, "{name}");
    }
}

#[test]
fn the_4010_spec_carries_the_4010_codes_and_structure() {
    let five = Spec::builtin_835();
    let four = Spec::builtin_835_4010();
    let codes = |spec: &Spec, segment: &[u8], element| {
        spec.element_def(segment, element, None)
            .unwrap()
            .codes
            .clone()
    };
    assert_eq!(codes(&five, b"ISA", 12), vec!["00501"]);
    assert_eq!(codes(&four, b"ISA", 12), vec!["00401"]);
    assert_eq!(
        codes(&four, b"GS", 8),
        vec!["004010", "004010X091", "004010X091A1"]
    );
    assert!(five.segment(b"RDM").is_some());
    assert!(four.segment(b"RDM").is_none());
}

/// The emedny fixture with one PLB carrying six adjustment composites, the
/// first valid and the other five with `code`, optionally declared as 4010.
fn with_plb(code: &str, declared_4010: bool) -> Vec<u8> {
    let text = String::from_utf8(common::load_fixture("emedny_sample.txt")).unwrap();
    let mut plb = String::from("PLB*9999999995*20101231*CV:REF1*1.00");
    for n in 2..=6 {
        plb.push_str(&format!("*{code}:REF{n}*{n}.00"));
    }
    plb.push('~');
    let mut text = text.replace("SE*", &format!("{plb}SE*"));
    if declared_4010 {
        text = text.replace("005010X221A1", "004010X091A1");
    }
    text.into_bytes()
}

/// Code-list findings on PLB segments only.
fn code_findings(bytes: Vec<u8>) -> Vec<Diagnostic> {
    let five = Spec::builtin_835();
    let four = Spec::builtin_835_4010();
    let document = Document::parse(bytes).unwrap();
    let chosen = Spec::select(&[&five, &four], &five, document.segments());
    let (_, diagnostics) = Processor::run(chosen, &document, None);
    diagnostics
        .into_iter()
        .filter(
            |d| matches!(&d.rule, Rule::CodeNotInList { segment_id, .. } if segment_id == b"PLB"),
        )
        .collect()
}

#[test]
fn a_5010_plb_checks_the_reason_code_of_every_adjustment_composite() {
    let found = code_findings(with_plb("ZZ", false));
    let places: Vec<_> = found.iter().map(|d| (d.element, d.component)).collect();
    let expected: Vec<_> = [5, 7, 9, 11, 13]
        .into_iter()
        .map(|e| (Some(e), Some(1)))
        .collect();
    assert_eq!(places, expected);
    assert!(found.iter().all(|d| d.datum == b"ZZ"));
    assert!(code_findings(with_plb("OA", false)).is_empty());
}

#[test]
fn a_4010_plb_leaves_every_reason_code_open() {
    let found = code_findings(with_plb("ZZ", true));
    assert!(found.is_empty(), "{found:?}");
}
