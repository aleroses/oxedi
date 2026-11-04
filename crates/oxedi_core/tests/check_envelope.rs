//! Structural diagnostics over every real-shaped file: the known envelope
//! anomalies (SNIP 1), rendered in full, and nothing else. The occurrence
//! findings (SNIP 2) of each file under its own version's spec are in the
//! project goldens.

mod common;

use std::collections::BTreeMap;

use oxedi_core::{Document, SnipLevel, Spec};

#[test]
fn the_known_anomalies_are_reported_exactly_and_nothing_else() {
    let spec = Spec::builtin_835();
    let mut found: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, bytes, delims) in common::all_files() {
        let rendered: Vec<String> = common::diagnostics_of(&spec, &bytes, delims)
            .iter()
            .filter(|diagnostic| diagnostic.level == SnipLevel::L1)
            .map(ToString::to_string)
            .collect();
        if !rendered.is_empty() {
            found.insert(name, rendered);
        }
    }
    let transaction = "interchange#1/group#1/transaction#1";
    let unknown = |id: &str, index: usize, path: &str| {
        format!(
            "SNIP 1 · segment \"{id}\" is not part of the structure: no open loop holds it and it opens no loop · segment #{index} · at {path} · datum \"{id}\""
        )
    };
    let se01 = |declared: &str, counted: usize, index: usize| {
        format!(
            "SNIP 1 · SE01 declares \"{declared}\" but the count is {counted} · segment #{index}, element 1 · at {transaction} · datum \"{declared}\""
        )
    };
    let expected = BTreeMap::from([
        (
            "blue_cross_nc_sample.txt".to_string(),
            vec![
                "SNIP 1 · loop \"interchange\" opened without its own trigger (\"ISA\" with no conditions) to hold segment \"ST\" · segment #0 · at interchange#1 · datum \"ST\"".to_string(),
                "SNIP 1 · loop \"group\" opened without its own trigger (\"GS\" with no conditions) to hold segment \"ST\" · segment #0 · at interchange#1/group#1 · datum \"ST\"".to_string(),
                se01("33", 32, 31),
            ],
        ),
        (
            "edi835_test_file.RMT".to_string(),
            vec![se01("1202", 76, 77)],
        ),
        (
            "edi835_test_not_available_claim_id.RMT".to_string(),
            vec![se01("302", 255, 256)],
        ),
        (
            "multi_claim_sample.txt".to_string(),
            vec![
                unknown("N3", 19, &format!("{transaction}/2000#1/2100#1")),
                unknown("N4", 20, &format!("{transaction}/2000#1/2100#1")),
                unknown("N3", 34, &format!("{transaction}/2000#2/2100#2")),
                unknown("N4", 35, &format!("{transaction}/2000#2/2100#2")),
            ],
        ),
        (
            "trizetto_sample.rmt".to_string(),
            vec![
                unknown("XX", 7, &format!("{transaction}/1000A#1")),
                se01("15", 18, 19),
            ],
        ),
    ]);
    assert_eq!(found, expected);
}

/// The checker's own SNIP 2 findings are the occurrence rules.
const OCCURRENCE_RULES: &[&str] = &[
    "RequiredOccurrenceMissing",
    "OccurrenceOverMax",
    "LoopOverMax",
    "OutOfOrder",
    "UnknownOccurrence",
    "RequiredLoopMissing",
];

#[test]
fn every_diagnostic_points_at_a_segment_holding_its_datum() {
    let (five, four) = common::builtins();
    for (name, bytes, delims) in common::all_files() {
        let document = Document::with_delimiters(&bytes[..], delims).unwrap();
        for spec in [&five, &four] {
            for diagnostic in common::diagnostics_of(spec, &bytes, delims) {
                let kind = diagnostic.rule.kind();
                let expected = if kind == "BalanceMismatch" {
                    SnipLevel::L3
                } else if OCCURRENCE_RULES.contains(&kind) {
                    SnipLevel::L2
                } else {
                    SnipLevel::L1
                };
                assert_eq!(diagnostic.level, expected, "{name}: {diagnostic}");
                if diagnostic.segment.is_none() {
                    assert!(diagnostic.datum.is_empty(), "{name}: {diagnostic}");
                    continue;
                }
                points_at_its_datum(&name, &document, &diagnostic);
            }
        }
    }
}

fn points_at_its_datum(name: &str, document: &Document<'_>, diagnostic: &oxedi_core::Diagnostic) {
    let span = diagnostic
        .span(document)
        .unwrap_or_else(|| panic!("{name}: {diagnostic} names no segment"));
    let body = &document.as_bytes()[span.body];
    assert!(
        body.windows(diagnostic.datum.len())
            .any(|window| window == diagnostic.datum.as_slice()),
        "{name}: {diagnostic} points at {:?}",
        String::from_utf8_lossy(body)
    );
}

/// The processor shares its occurrence match with the checker; the checker
/// alone matches by itself. Both paths raise the same findings.
#[test]
fn the_checker_alone_and_inside_the_processor_raise_the_same_findings() {
    let (five, four) = common::builtins();
    let mut occurrence_findings = 0;
    for (name, bytes, delims) in common::all_files() {
        let document = Document::with_delimiters(&bytes[..], delims).unwrap();
        for spec in [&five, &four] {
            let alone = common::diagnostics_of(spec, &bytes, delims);
            let (_, all) = oxedi_core::Processor::run(spec, &document, None);
            let inside: Vec<_> = all
                .into_iter()
                .filter(|diagnostic| {
                    diagnostic.level != SnipLevel::L2
                        || OCCURRENCE_RULES.contains(&diagnostic.rule.kind())
                })
                .collect();
            assert_eq!(alone, inside, "{name} under {}", spec.name());
            occurrence_findings += alone
                .iter()
                .filter(|diagnostic| diagnostic.level == SnipLevel::L2)
                .count();
        }
    }
    // Some files raise occurrence findings, so the comparison covers them.
    assert!(occurrence_findings > 0);
}
