//! Tables and diagnostics compared against committed golden files in
//! `tests/golden/project/`, each file under the built-in spec of the version
//! it declares. Small files keep every row of every table; the
//! two large samples keep a row count per table. Every file keeps its whole
//! diagnostic list, one `Display` line each. Regenerate with
//! `UPDATE_GOLDEN=1 cargo test --test project_golden`, inspect the diff,
//! commit. The tables of the `edi_835_parser.json` patch have their own
//! goldens in `tests/golden/project/edi_835_parser/`, in the same two formats.
//! The masked tables and diagnostics of every file, under a fixed key with
//! dates reduced to their year, have theirs in `tests/golden/project/masked/`,
//! with one run of `balanced_5010_sample.txt` with dates shifted.

mod common;

use std::fmt::Write as _;
use std::path::PathBuf;

use oxedi_core::{DateMode, Diagnostic, Document, Mask, Processor, Spec, Tables};

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/project")
}

/// One line per table with its row count.
fn row_counts(tables: &Tables) -> String {
    tables.iter().fold(String::new(), |mut out, table| {
        let _ = writeln!(out, "{} rows: {}", table.name(), table.len());
        out
    })
}

fn diagnostic_lines(diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .fold(String::new(), |mut out, diagnostic| {
            let _ = writeln!(out, "{diagnostic}");
            out
        })
}

/// Each file is projected with the built-in spec of the version it declares.
#[test]
fn tables_and_diagnostics_match_the_golden_files() {
    let (five, four) = common::builtins();
    let mut outputs = Vec::new();
    for (name, bytes, delims) in common::all_files() {
        let spec = common::select(&five, &four, &bytes, delims);
        let document = Document::with_delimiters(&bytes[..], delims).unwrap();
        let (tables, diagnostics) = Processor::run(spec, &document, None);
        outputs.push(if common::SUMMARY_ONLY.contains(&name.as_str()) {
            (
                golden_dir().join(format!("{name}.tables.summary.txt")),
                row_counts(&tables),
            )
        } else {
            (
                golden_dir().join(format!("{name}.tables.txt")),
                tables.to_string(),
            )
        });
        outputs.push((
            golden_dir().join(format!("{name}.diagnostics.txt")),
            diagnostic_lines(&diagnostics),
        ));
    }
    let failures = common::compare_goldens(&golden_dir(), &outputs, &["edi_835_parser", "masked"]);
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The compat layer passes its spec explicitly, built on the 5010 built-in,
/// so every file is projected with it whatever version it declares.
#[test]
fn edi_835_parser_tables_match_the_golden_files() {
    let spec = Spec::builtin_835()
        .merge_patch(include_str!("../specs/edi_835_parser.json"))
        .unwrap();
    let dir = golden_dir().join("edi_835_parser");
    let mut outputs = Vec::new();
    for (name, bytes, delims) in common::all_files() {
        let document = Document::with_delimiters(&bytes[..], delims).unwrap();
        let (tables, _) = Processor::run(&spec, &document, None);
        outputs.push(if common::SUMMARY_ONLY.contains(&name.as_str()) {
            (
                dir.join(format!("{name}.tables.summary.txt")),
                row_counts(&tables),
            )
        } else {
            (dir.join(format!("{name}.tables.txt")), tables.to_string())
        });
    }
    let failures = common::compare_goldens(&dir, &outputs, &[]);
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Each file is projected with the built-in spec of the version it declares,
/// masked with a fixed key and dates reduced to their year; one file is also
/// projected with dates shifted.
#[test]
fn masked_tables_and_diagnostics_match_the_golden_files() {
    let (five, four) = common::builtins();
    let key = b"oxedi-test-key-0";
    let year = Mask::new(key, DateMode::Year).unwrap();
    let shift = Mask::new(key, DateMode::Shift).unwrap();
    let dir = golden_dir().join("masked");
    let mut outputs = Vec::new();
    for (name, bytes, delims) in common::all_files() {
        let spec = common::select(&five, &four, &bytes, delims);
        let document = Document::with_delimiters(&bytes[..], delims).unwrap();
        let mut runs = vec![(name.clone(), &year)];
        if name == "balanced_5010_sample.txt" {
            runs.push((format!("{name}.shift"), &shift));
        }
        for (label, mask) in runs {
            let (tables, diagnostics) = Processor::run(spec, &document, Some(mask));
            outputs.push(if common::SUMMARY_ONLY.contains(&name.as_str()) {
                (
                    dir.join(format!("{label}.tables.summary.txt")),
                    row_counts(&tables),
                )
            } else {
                (dir.join(format!("{label}.tables.txt")), tables.to_string())
            });
            outputs.push((
                dir.join(format!("{label}.diagnostics.txt")),
                diagnostic_lines(&diagnostics),
            ));
        }
    }
    let failures = common::compare_goldens(&dir, &outputs, &[]);
    assert!(failures.is_empty(), "{failures:#?}");
}
