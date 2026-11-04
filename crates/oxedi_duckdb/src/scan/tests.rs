use std::path::PathBuf;

use oxedi_core::{Document, Processor};

use super::project;
use super::write::{cell_bytes, hugeint};
use crate::builtins::Builtins;

#[test]
fn hugeint_splits_low_and_high_words() {
    for value in [0i128, 1, -1, 25_264_000, i128::MAX, i128::MIN] {
        let split = hugeint(value);
        let joined = (i128::from(split.upper) << 64) | i128::from(split.lower);
        assert_eq!(joined, value);
    }
}

#[test]
fn cell_bytes_reads_between_offsets() {
    let offsets = [0, 3, 3, 5];
    let bytes = b"abcde";
    assert_eq!(cell_bytes(&offsets, bytes, 0), Some(&b"abc"[..]));
    assert_eq!(cell_bytes(&offsets, bytes, 1), Some(&b""[..]));
    assert_eq!(cell_bytes(&offsets, bytes, 2), Some(&b"de"[..]));
    assert_eq!(cell_bytes(&offsets, bytes, 3), None);
}

#[test]
fn cell_bytes_refuses_offsets_past_the_data() {
    assert_eq!(cell_bytes(&[0, 9], b"abc", 0), None);
    assert_eq!(cell_bytes(&[-1, 2], b"abc", 0), None);
}

/// Every fixture and sample of the core that parses, with its name.
fn core_files() -> Vec<(String, Vec<u8>)> {
    let core = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../oxedi_core/tests");
    let mut files = Vec::new();
    for dir in ["fixtures", "samples"] {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(core.join(dir))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext != "md"))
            .collect();
        paths.sort();
        for path in paths {
            let bytes = std::fs::read(&path).unwrap();
            files.push((path.display().to_string(), bytes));
        }
    }
    files
}

#[test]
fn the_bound_table_alone_holds_the_rows_of_a_full_run_on_every_core_file() {
    let builtins = Builtins::load();
    let mut checked = 0;
    for (name, bytes) in core_files() {
        let Ok(document) = Document::parse(bytes.as_slice()) else {
            continue;
        };
        for builtin in builtins.iter() {
            let (whole, _) = Processor::run(builtin.spec(), &document, None);
            for table in &whole {
                let alone = project(builtin, &document, table.name(), &name, None).unwrap();
                assert_eq!(
                    &alone,
                    table,
                    "{name}, {}: {}",
                    builtin.version(),
                    table.name()
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= 14 * 2 * 5, "{checked} tables compared");
}
