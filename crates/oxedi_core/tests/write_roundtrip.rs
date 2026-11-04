//! Parse, write the tables back, parse again: the tables match on every
//! column the writer consumes, and the written file reads without a
//! diagnostic, for every sample and fixture that reads clean and balances.
//! The others are refused when written strictly, with exactly the findings
//! that `allow_findings` returns along with a file that still parses into the
//! spec's tables.

mod common;

use oxedi_core::write::{Envelope, WriteError, write, write_with_findings};
use oxedi_core::{Document, Processor, Spec, Tables};

/// The files that read without a diagnostic.
const CLEAN: &[&str] = &[
    "edi835_test_davisvision.RMT",
    "edi835_test_eyemed.RMT",
    "edi835_test_united.rmt",
    "edi835_test_versant.RMT",
    "emedny_sample.txt",
    "united_healthcare_legacy_sample.txt",
    "balanced_5010_sample.txt",
    "balanced_4010_sample.txt",
    "edge_cases_5010_sample.txt",
    "edge_cases_4010_sample.txt",
];

/// 2024-01-01 at 12:30.
fn envelope() -> Envelope {
    Envelope::new("ZZ", "SENDER", "ZZ", "RECEIVER", 19_723, 45_000)
}

/// Every difference between two projections, on every column but the
/// anchor segment's index (the written file holds fewer segments).
fn differences(before: &Tables, after: &Tables) -> Vec<String> {
    let mut found = Vec::new();
    for table in before {
        let Some(other) = after.get(table.name()) else {
            found.push(format!("{} is missing", table.name()));
            continue;
        };
        if table.len() != other.len() {
            found.push(format!(
                "{}: {} rows, then {}",
                table.name(),
                table.len(),
                other.len()
            ));
            continue;
        }
        for (name, column) in table.columns() {
            if name == "segment" {
                continue;
            }
            let Some(written) = other.column(name) else {
                found.push(format!("{}.{name} is missing", table.name()));
                continue;
            };
            for row in 0..table.len() {
                if column.get(row) != written.get(row) {
                    found.push(format!(
                        "{}.{name} row {row}: {:?}, then {:?}",
                        table.name(),
                        column.render(row),
                        written.render(row)
                    ));
                }
            }
        }
    }
    found
}

fn read(spec: &Spec, bytes: &[u8]) -> (Tables, Vec<String>) {
    let document = Document::parse(bytes).expect("the written file has an ISA");
    let (tables, diagnostics) = Processor::run(spec, &document, None);
    (
        tables,
        diagnostics.iter().map(ToString::to_string).collect(),
    )
}

#[test]
fn clean_files_write_back_to_the_same_tables() {
    let (five, four) = common::builtins();
    for (name, bytes, delims) in common::all_files() {
        if !CLEAN.contains(&name.as_str()) {
            continue;
        }
        let spec = common::select(&five, &four, &bytes, delims);
        let document = Document::with_delimiters(&bytes[..], delims).unwrap();
        let (tables, diagnostics) = Processor::run(spec, &document, None);
        assert!(diagnostics.is_empty(), "{name} reads clean");
        let written = match write(spec, &tables, &envelope()) {
            Ok(written) => written,
            Err(error) => panic!("{name}: {error}"),
        };
        let (again, diagnostics) = read(spec, &written);
        assert_eq!(diagnostics, Vec::<String>::new(), "{name}");
        assert_eq!(differences(&tables, &again), Vec::<String>::new(), "{name}");
        let rows: usize = tables.iter().map(|table| table.len()).sum();
        println!("{name}: {rows} rows, {} bytes written", written.len());
    }
}

#[test]
fn files_with_findings_write_only_when_allowed() {
    let (five, four) = common::builtins();
    for (name, bytes, delims) in common::all_files() {
        if CLEAN.contains(&name.as_str()) {
            continue;
        }
        let spec = common::select(&five, &four, &bytes, delims);
        let document = Document::with_delimiters(&bytes[..], delims).unwrap();
        let (tables, _) = Processor::run(spec, &document, None);
        let (written, findings) = write_with_findings(spec, &tables, &envelope()).unwrap();
        assert!(!findings.is_empty(), "{name}");
        match write(spec, &tables, &envelope()) {
            Err(WriteError::Findings(refused)) => assert_eq!(refused, findings, "{name}"),
            other => panic!("{name}: {other:?}"),
        }
        let (again, _) = read(spec, &written);
        assert_eq!(again.len(), tables.len(), "{name}");
    }
}
