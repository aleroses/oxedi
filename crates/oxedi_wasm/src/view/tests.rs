use super::*;
use oxedi_core::{Document, Event, Processor, Spec};
use std::path::{Path, PathBuf};

fn corpus() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../oxedi_core/tests");
    let mut files = Vec::new();
    for dir in ["fixtures", "samples"] {
        for entry in std::fs::read_dir(root.join(dir)).expect("test corpus") {
            let path = entry.expect("entry").path();
            if path.file_name().is_some_and(|n| n != "README.md") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn parsable(path: &Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).expect("read");
    let parses = Document::parse(&bytes[..]).is_ok();
    parses.then_some(bytes)
}

#[test]
fn ranges_are_contiguous_and_cover_the_file() {
    for path in corpus() {
        let Some(bytes) = parsable(&path) else {
            continue;
        };
        let view = view(&bytes).expect("view");
        let mut at = 0;
        for segment in &view.segments {
            assert_eq!(segment.start, at, "{path:?}");
            assert!(segment.end > segment.start, "{path:?}");
            at = segment.end;
        }
        assert_eq!(at, bytes.len(), "{path:?}");
    }
}

/// The open loops at each segment, outermost first, as `(name, ordinal)`:
/// the engine's stack when the segment is captured, left unmatched or found
/// empty, the way the Python binding's `_loop_paths` reads it.
fn engine_paths(spec: &Spec, document: &Document<'_>) -> Vec<Vec<(String, usize)>> {
    let mut engine = oxedi_core::LoopEngine::new(spec);
    let mut ordinals = vec![0usize; spec.loops().len()];
    let mut open: Vec<(String, usize)> = Vec::new();
    let mut paths = Vec::new();
    for segment in document.segments() {
        let mut path = None;
        for event in engine.feed(&segment) {
            match *event {
                Event::LoopOpened { id, .. } => {
                    ordinals[id.index()] += 1;
                    open.push((spec.loop_name(id).to_string(), ordinals[id.index()]));
                }
                Event::LoopClosed { .. } => {
                    open.pop();
                }
                Event::Captured { .. } | Event::Unmatched { .. } | Event::Empty { .. } => {
                    path.get_or_insert_with(|| open.clone());
                }
            }
        }
        paths.push(path.unwrap_or_else(|| open.clone()));
    }
    paths
}

/// The loops from the root down to `node`, as `(name, ordinal)`.
fn chain(view: &View, node: usize) -> Vec<(String, usize)> {
    let mut chain = Vec::new();
    let mut at = Some(node);
    while let Some(index) = at {
        let node = &view.nodes[index];
        if let Some(name) = &node.loop_name {
            chain.push((name.clone(), node.ordinal));
        }
        at = node.parent;
    }
    chain.reverse();
    chain
}

/// Every segment belongs to exactly one node: a segment with content to the
/// tree node that holds it, every segment to the loops open when it was read.
fn assert_ownership(label: &str, bytes: &[u8]) -> View {
    let view = view(bytes).expect("view");
    let document = Document::parse(bytes).expect("parse");
    let spec = select(&document);
    let tree = oxedi_core::LoopTree::build(spec, document.segments());
    let mut holders: Vec<Vec<usize>> = vec![Vec::new(); view.segments.len()];
    for (index, node) in tree.nodes().iter().enumerate() {
        for &segment in node.segments.iter().chain(&node.unmatched) {
            holders[segment].push(index);
        }
    }
    let paths = engine_paths(spec, &document);
    assert_eq!(paths.len(), view.segments.len(), "{label}");
    for (index, segment) in view.segments.iter().enumerate() {
        assert!(segment.node < view.nodes.len(), "{label} segment {index}");
        let empty = document.span(index).expect("span").body.is_empty();
        let expected: Vec<usize> = if empty { vec![] } else { vec![segment.node] };
        assert_eq!(holders[index], expected, "{label} segment {index}");
        assert_eq!(
            chain(&view, segment.node),
            paths[index],
            "{label} segment {index}"
        );
    }
    view
}

#[test]
fn every_segment_belongs_to_the_loops_open_when_it_was_read() {
    for path in corpus() {
        let Some(bytes) = parsable(&path) else {
            continue;
        };
        assert_ownership(&format!("{path:?}"), &bytes);
    }
}

#[test]
fn an_empty_segment_inside_a_loop_belongs_to_the_open_loop() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../oxedi_core/tests/fixtures/balanced_5010_sample.txt");
    let original = std::fs::read(path).expect("read");
    let at = original
        .windows(5)
        .position(|w| w == b"~\nCLP")
        .expect("a CLP segment");
    // `~~` with no trivia, and a terminator alone after the line break.
    for insert in [&b"~"[..], &b"\n~"[..]] {
        let mut bytes = original.clone();
        bytes.splice(at + 1..at + 1, insert.iter().copied());
        let view = assert_ownership(&format!("{insert:?}"), &bytes);
        let empty = view
            .segments
            .iter()
            .position(|s| s.id.is_empty())
            .expect("the empty segment");
        let open = chain(&view, view.segments[empty].node);
        assert!(!open.is_empty(), "{insert:?}");
        assert_eq!(
            open,
            chain(&view, view.segments[empty - 1].node),
            "{insert:?}"
        );
    }
}

#[test]
fn tables_and_diagnostics_match_the_core() {
    for path in corpus() {
        let Some(bytes) = parsable(&path) else {
            continue;
        };
        let view = view(&bytes).expect("view");
        let document = Document::parse(&bytes[..]).expect("parse");
        let (tables, diagnostics) = Processor::run(select(&document), &document, None);
        assert_eq!(view.tables.len(), tables.len(), "{path:?}");
        for (mine, theirs) in view.tables.iter().zip(tables.iter()) {
            assert_eq!(mine.name, theirs.name());
            assert_eq!(mine.rows, theirs.len());
            assert_eq!(mine.columns.len(), theirs.columns().len());
        }
        let texts: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
        let mine: Vec<&str> = view.diagnostics.iter().map(|d| d.text.as_str()).collect();
        assert_eq!(mine, texts, "{path:?}");
    }
}

#[test]
fn a_4010_file_uses_the_4010_spec() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../oxedi_core/tests/fixtures/balanced_4010_sample.txt");
    let view = view(&std::fs::read(path).expect("read")).expect("view");
    let expected = Spec::builtin_835_4010();
    let declared = expected
        .version()
        .and_then(|v| v.values.first())
        .expect("declared");
    assert_eq!(
        view.spec_version.as_deref(),
        Some(&*String::from_utf8_lossy(declared))
    );
    assert!(view.diagnostics.is_empty(), "{:?}", view.diagnostics);
}

#[test]
fn a_null_cell_is_none_and_a_value_is_its_text() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../oxedi_core/tests/fixtures/balanced_5010_sample.txt");
    let view = view(&std::fs::read(path).expect("read")).expect("view");
    let payments = view
        .tables
        .iter()
        .find(|t| t.name == "payments")
        .expect("payments");
    let total = payments
        .columns
        .iter()
        .find(|c| c.name == "total_payment_amount")
        .expect("col");
    assert!(total.values[0].as_deref().is_some_and(|v| v.contains('.')));
    assert!(
        payments
            .columns
            .iter()
            .any(|c| c.values.iter().any(Option::is_none))
    );
}

#[test]
fn bytes_that_are_not_an_835_give_the_core_error() {
    for bytes in [
        &b""[..],
        &b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n"[..],
        &b"a,b,c\n1,2,3\n"[..],
    ] {
        let expected = Document::parse(bytes).expect_err("not an 835").to_string();
        assert_eq!(view(bytes).expect_err("error"), expected);
    }
}

#[test]
fn the_view_serializes_with_the_documented_keys() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../oxedi_core/tests/fixtures/balanced_5010_sample.txt");
    let json = to_json(&std::fs::read(path).expect("read"), None).expect("json");
    let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    for key in [
        "spec_version",
        "delimiters",
        "segments",
        "nodes",
        "tables",
        "diagnostics",
    ] {
        assert!(value.get(key).is_some(), "{key}");
    }
    assert_eq!(value["nodes"][0]["loop_name"], serde_json::Value::Null);
    assert_eq!(value["delimiters"]["element"], "*");
}

const KEY: &[u8] = b"oxedi-test-key-0";

/// Every cell of a view, as `(table, column, row, value)`.
fn cells(view: &View) -> Vec<(String, String, usize, Option<String>)> {
    let mut all = Vec::new();
    for table in &view.tables {
        for column in &table.columns {
            for (row, value) in column.values.iter().enumerate() {
                all.push((table.name.clone(), column.name.clone(), row, value.clone()));
            }
        }
    }
    all
}

#[test]
fn a_masked_view_differs_from_the_plain_one_only_in_flagged_cells() {
    let spec = Spec::builtin_835();
    let flagged: Vec<(String, String)> = spec
        .tables()
        .iter()
        .flat_map(|table| {
            (0..table.columns.len())
                .filter(|&column| table.phi(column).is_some())
                .map(|column| (table.name.clone(), table.columns[column].0.clone()))
        })
        .collect();
    assert!(!flagged.is_empty());
    let mut changed = 0;
    for path in corpus() {
        let Some(bytes) = parsable(&path) else {
            continue;
        };
        let plain = view(&bytes).expect("view");
        let masked = view_masked(&bytes, Some(KEY)).expect("masked");
        assert_eq!(plain.segments.len(), masked.segments.len(), "{path:?}");
        assert_eq!(
            plain.diagnostics.len(),
            masked.diagnostics.len(),
            "{path:?}"
        );
        for (a, b) in cells(&plain).into_iter().zip(cells(&masked)) {
            if a != b {
                assert!(
                    flagged.contains(&(a.0.clone(), a.1.clone())),
                    "{path:?}: {a:?} vs {b:?}"
                );
                changed += 1;
            }
        }
    }
    assert!(changed > 0);
}

#[test]
fn a_masked_name_is_sixteen_hex_characters() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../oxedi_core/tests/fixtures/balanced_5010_sample.txt");
    let view = view_masked(&std::fs::read(path).expect("read"), Some(KEY)).expect("masked");
    let claims = view
        .tables
        .iter()
        .find(|t| t.name == "claims")
        .expect("claims");
    let last = claims
        .columns
        .iter()
        .find(|c| c.name == "patient_last_name")
        .expect("col");
    let value = last.values[0].as_deref().expect("name");
    assert_eq!(value.len(), 16);
    assert!(
        value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );
    assert!(
        !to_json(
            &std::fs::read(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../oxedi_core/tests/fixtures/balanced_5010_sample.txt")
            )
            .expect("read"),
            Some(KEY)
        )
        .expect("json")
        .contains("\"DOE\"")
    );
}

#[test]
fn a_short_key_gives_the_core_message() {
    let err = view_masked(b"ISA", Some(b"short")).expect_err("short key");
    assert!(err.contains("at least 16 bytes"), "{err}");
}
