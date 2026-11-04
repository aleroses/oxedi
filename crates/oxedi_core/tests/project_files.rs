//! Tables and diagnostics of the processor over every real-shaped file.

mod common;

use std::collections::BTreeMap;

use oxedi_core::{
    Cell, Column, Document, Element, Event, LoopId, LoopTree, Processor, Segment, SnipLevel, Spec,
    Table, Tokenizer,
};

/// Fed segment by segment from a tokenizer and drained after each
/// transaction, the processor gives the same rows and diagnostics as one run
/// over the whole document.
#[test]
fn draining_after_each_transaction_adds_up_to_one_run_over_the_document() {
    let spec = Spec::builtin_835();
    let transaction = spec.loop_id("transaction").unwrap();
    for (name, bytes, delims) in common::all_files() {
        let document = Document::with_delimiters(&bytes[..], delims).unwrap();
        let (whole, diagnostics) = Processor::run(&spec, &document, None);

        let mut processor = Processor::new(&spec, &delims);
        let mut drained = Vec::new();
        let mut incremental = Vec::new();
        for segment in Tokenizer::with_delimiters(&bytes, delims) {
            let output = processor.feed(&segment);
            incremental.extend_from_slice(output.diagnostics());
            if output
                .events()
                .contains(&Event::LoopClosed { id: transaction })
            {
                drained.push(processor.take_tables());
            }
        }
        incremental.extend_from_slice(processor.finish().diagnostics());
        drained.push(processor.take_tables());

        assert_eq!(incremental, diagnostics, "{name}");
        assert_eq!(whole.len(), 5, "{name}");
        for table in &whole {
            let pieces: Vec<String> = drained
                .iter()
                .flat_map(|tables| rows(tables.get(table.name()).unwrap()))
                .collect();
            assert_eq!(pieces, rows(table), "{name}: {}", table.name());
        }
    }
}

/// The cell of a row-index column as a number; `None` when null.
/// Every row of a table as one rendered line.
fn rows(table: &Table) -> Vec<String> {
    (0..table.len())
        .filter_map(|row| table.render_row(row))
        .collect()
}

fn index_at(table: &Table, column: &str, row: usize) -> Option<usize> {
    match table.column(column).and_then(|data| data.get(row)) {
        Some(Cell::Int64(value)) => usize::try_from(value).ok(),
        _ => None,
    }
}

/// `true` when an element has a non-empty value or component.
fn has_content(element: &Element<'_>) -> bool {
    match element {
        Element::Simple(value) => !value.is_empty(),
        Element::Composite(parts) => parts.iter().any(|part| !part.is_empty()),
    }
}

/// Element groups of a segment that start with content, from `from` every `step`.
fn groups(segment: &Segment<'_>, from: usize, step: usize) -> usize {
    (from..=segment.elements.len())
        .step_by(step)
        .filter(|&position| segment.element(position).is_some_and(has_content))
        .count()
}

/// One payment per transaction, one claim per CLP, one service per SVC, one
/// adjustment per CAS group with a reason code and one provider adjustment
/// per PLB group with an adjustment identifier.
#[test]
fn row_counts_follow_the_segments_of_each_file() {
    let spec = Spec::builtin_835();
    for (name, bytes, delims) in common::all_files() {
        let segments: Vec<Segment<'_>> = Tokenizer::with_delimiters(&bytes, delims).collect();
        let document = Document::with_delimiters(&bytes[..], delims).unwrap();
        let (tables, _) = Processor::run(&spec, &document, None);
        let rows = |table: &str| tables.get(table).unwrap().len();
        let of = |id: &'static [u8]| segments.iter().filter(move |segment| segment.id == id);
        assert_eq!(rows("payments"), of(b"ST").count(), "{name}: payments");
        assert_eq!(rows("claims"), of(b"CLP").count(), "{name}: claims");
        assert_eq!(rows("services"), of(b"SVC").count(), "{name}: services");
        assert_eq!(
            rows("adjustments"),
            of(b"CAS").map(|cas| groups(cas, 2, 3)).sum::<usize>(),
            "{name}: adjustments"
        );
        assert_eq!(
            rows("provider_adjustments"),
            of(b"PLB").map(|plb| groups(plb, 3, 2)).sum::<usize>(),
            "{name}: provider_adjustments"
        );
    }
}

/// Every row's `segment` is captured by an instance of an anchor loop (its
/// trigger, for a table without `segment`), and every row index of a table
/// above points at the row of the instance that encloses that one, or is
/// null when no instance of it does.
#[test]
fn every_row_points_at_its_anchor_and_at_the_rows_that_enclose_it() {
    let spec = Spec::builtin_835();
    for (name, bytes, delims) in common::all_files() {
        assert_rows_point_at_their_anchors(&spec, &name, &bytes, delims);
    }
}

/// A fragment that starts below the loop a table anchors on opens that loop
/// implicitly; its rows still sit in an anchor loop and point at the rows
/// that enclose them.
#[test]
fn a_fragment_that_opens_its_anchor_loops_implicitly_passes_the_same_invariants() {
    let spec = Spec::builtin_835();
    let delims = oxedi_core::Delimiters::new(b'*', b':', b'~');
    for fragment in [
        &b"CLP*C1*1*100*80*20*12*R1*11*1~SVC*HC:99213*100*80~CAS*CO*45*20~"[..],
        &b"SVC*HC:99213*100*80~CAS*CO*45*20~"[..],
    ] {
        let document = Document::with_delimiters(fragment, delims).unwrap();
        let (tables, _) = Processor::run(&spec, &document, None);
        assert!(
            tables.iter().any(|table| !table.is_empty()),
            "the fragment projects rows"
        );
        assert_rows_point_at_their_anchors(&spec, "fragment", fragment, delims);
    }
}

fn assert_rows_point_at_their_anchors(
    spec: &Spec,
    name: &str,
    bytes: &[u8],
    delims: oxedi_core::Delimiters,
) {
    let segments: Vec<Segment<'_>> = Tokenizer::with_delimiters(bytes, delims).collect();
    let tree = LoopTree::build(spec, segments.iter().cloned());
    let nodes = tree.nodes();
    let mut owner = vec![None; segments.len()];
    for (index, node) in nodes.iter().enumerate() {
        for &segment in &node.segments {
            owner[segment] = Some(index);
        }
    }
    let enclosing = |node: usize, loops: &[LoopId]| {
        let mut at = Some(node);
        while let Some(index) = at {
            if nodes[index].loop_id.is_some_and(|id| loops.contains(&id)) {
                return Some(index);
            }
            at = nodes[index].parent.map(|parent| parent.index());
        }
        None
    };
    let document = Document::with_delimiters(bytes, delims).unwrap();
    let (tables, _) = Processor::run(spec, &document, None);
    for def in spec.tables() {
        let table = tables.get(&def.name).unwrap();
        for row in 0..table.len() {
            assert_eq!(
                index_at(table, "row", row),
                Some(row),
                "{name}: {}",
                def.name
            );
            let at = index_at(table, "segment", row).unwrap();
            let node = owner[at].unwrap_or_else(|| panic!("{name}: segment #{at} is not captured"));
            let anchor = enclosing(node, &def.loops).unwrap();
            // An implicitly opened anchor captures no segment of its own: the
            // row's segment is the trigger of the descendant that needed it,
            // which the anchor records as `opened_by` and a descendant owns.
            if !nodes[anchor].implicit {
                assert_eq!(
                    anchor, node,
                    "{name}: {} row {row} sits in its anchor",
                    def.name
                );
            }
            if def.segment.is_none() {
                assert_eq!(
                    nodes[anchor].opened_by,
                    Some(at),
                    "{name}: {} row {row}",
                    def.name
                );
            }
            for &above in &def.ancestors {
                let parent = &spec.tables()[above];
                let expected = enclosing(node, &parent.loops).and_then(|n| nodes[n].opened_by);
                let found = index_at(table, &parent.reference, row).map(|r| {
                    let parent_rows = tables.get(&parent.name).unwrap();
                    assert!(
                        r < parent_rows.len(),
                        "{name}: {} row {row} out of range",
                        def.name
                    );
                    index_at(parent_rows, "segment", r).unwrap()
                });
                assert_eq!(
                    found, expected,
                    "{name}: {}.{} row {row}",
                    def.name, parent.reference
                );
            }
        }
    }
}

/// Every column of a table has the table's length; validity bitmaps and
/// binary offsets are laid out as Arrow expects.
#[test]
fn columns_have_equal_lengths_and_arrow_buffers() {
    let spec = Spec::builtin_835();
    for (name, bytes, delims) in common::all_files() {
        let document = Document::with_delimiters(&bytes[..], delims).unwrap();
        let (tables, _) = Processor::run(&spec, &document, None);
        for table in &tables {
            let rows = table.len();
            for (column, data) in table.columns() {
                let at = format!("{name}: {}.{column}", table.name());
                assert_eq!(data.len(), rows, "{at}");
                assert_eq!(data.validity().len(), rows, "{at}");
                assert_eq!(data.validity().as_bytes().len(), rows.div_ceil(8), "{at}");
                match data.column() {
                    Column::Binary {
                        offsets,
                        data: bytes,
                    } => {
                        assert_eq!(offsets.len(), rows + 1, "{at}");
                        assert_eq!(offsets.first(), Some(&0), "{at}");
                        assert_eq!(
                            offsets.last().map(|&end| end as usize),
                            Some(bytes.len()),
                            "{at}"
                        );
                        for row in 0..rows {
                            assert!(offsets[row] <= offsets[row + 1], "{at}");
                            if data.validity().get(row) == Some(false) {
                                assert_eq!(offsets[row], offsets[row + 1], "{at}: null row {row}");
                            }
                        }
                    }
                    Column::Int64 { values, .. } => assert_eq!(values.len(), rows, "{at}"),
                    Column::Decimal128 { values, .. } => assert_eq!(values.len(), rows, "{at}"),
                    Column::Date32(values) | Column::Time32(values) => {
                        assert_eq!(values.len(), rows, "{at}")
                    }
                }
            }
        }
    }
}

/// Each file under the built-in spec of the version it declares: the element
/// findings over the thirteen files are all defects of the synthetic fixtures;
/// the six anonymized payer files raise none.
#[test]
fn only_the_synthetic_fixtures_raise_element_findings() {
    let (five, four) = common::builtins();
    let mut found = BTreeMap::new();
    for (name, bytes, delims) in common::all_files() {
        let spec = common::select(&five, &four, &bytes, delims);
        let document = Document::with_delimiters(&bytes[..], delims).unwrap();
        let (_, diagnostics) = Processor::run(spec, &document, None);
        let level_two = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.level == SnipLevel::L2)
            .count();
        if level_two > 0 {
            found.insert(name, level_two);
        }
    }
    assert_eq!(
        found,
        BTreeMap::from([
            ("blue_cross_nc_sample.txt".to_string(), 5),
            ("masked_findings_4010_sample.txt".to_string(), 5),
            ("masked_findings_5010_sample.txt".to_string(), 5),
            ("multi_claim_sample.txt".to_string(), 18),
            ("trizetto_sample.rmt".to_string(), 5),
        ])
    );
}

/// A payer's proprietary reference becomes a column with a three-line patch.
#[test]
fn a_three_line_patch_adds_a_column_the_table_then_shows() {
    let bytes = common::load_sample("edi835_test_not_available_claim_id.RMT");
    let builtin = Spec::builtin_835();
    let patched = builtin
        .merge_patch(
            r#"{"tables":{"claims":{"columns":{
                "contract_class":{"segment":"REF","where":{"1":"CE"},"element":2}
            }}}}"#,
        )
        .unwrap();
    let document = Document::parse(&bytes[..]).unwrap();
    let (before, _) = Processor::run(&builtin, &document, None);
    let (after, _) = Processor::run(&patched, &document, None);
    assert!(
        before
            .get("claims")
            .unwrap()
            .column("contract_class")
            .is_none()
    );
    let claims = after.get("claims").unwrap();
    let column = claims.column("contract_class").unwrap();
    let expected: Vec<String> = Tokenizer::new(&bytes)
        .unwrap()
        .filter(|segment| {
            segment.id == b"REF" && segment.element(1).and_then(Element::simple) == Some(b"CE")
        })
        .map(|segment| {
            String::from_utf8_lossy(segment.element(2).and_then(Element::simple).unwrap())
                .into_owned()
        })
        .collect();
    assert_eq!(expected.len(), 18);
    let values: Vec<String> = (0..claims.len())
        .map(|row| column.render(row).unwrap())
        .collect();
    assert_eq!(values, expected);
    for (name, data) in before.get("claims").unwrap().columns() {
        assert_eq!(claims.column(name), Some(data), "{name} is unchanged");
    }
}

/// Filling one table, or every one, gives each the rows of a full run, parent
/// references included; the diagnostics do not change.
#[test]
fn filling_some_tables_gives_each_the_rows_of_a_full_run() {
    let (five, four) = common::builtins();
    each_table_alone_equals_the_full_run(&five, &four);
}

/// The same holds for a spec patched with a new table and a new column, which
/// the built-in goldens do not cover.
#[test]
fn filling_some_tables_of_a_patched_spec_gives_each_the_rows_of_a_full_run() {
    let patch = r#"{"tables":{
        "claim_dates":{"loops":["2100"],"segment":"DTM","columns":{
            "qualifier":{"element":1},"date":{"element":2}}},
        "claims":{"columns":{"first_amount":{"segment":"AMT","element":2}}}
    }}"#;
    let (five, four) = common::builtins();
    let (five, four) = (
        five.merge_patch(patch).unwrap(),
        four.merge_patch(patch).unwrap(),
    );
    each_table_alone_equals_the_full_run(&five, &four);
    let (mut any_dates, mut any_amount) = (false, false);
    for (_, bytes, delims) in common::all_files() {
        let spec = common::select(&five, &four, &bytes, delims);
        let document = Document::with_delimiters(&bytes[..], delims).unwrap();
        let (whole, _) = Processor::run(spec, &document, None);
        any_dates |= whole
            .get("claim_dates")
            .is_some_and(|table| !table.is_empty());
        let amounts = whole.get("claims").unwrap().column("first_amount").unwrap();
        any_amount |= amounts.null_count() < amounts.len();
    }
    assert!(any_dates, "no file fills the new table");
    assert!(any_amount, "no file fills the new column");
}

fn each_table_alone_equals_the_full_run(five: &Spec, four: &Spec) {
    for (name, bytes, delims) in common::all_files() {
        let spec = common::select(five, four, &bytes, delims);
        let document = Document::with_delimiters(&bytes[..], delims).unwrap();
        let (whole, diagnostics) = Processor::run(spec, &document, None);
        let names: Vec<&str> = spec.tables().iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            Processor::run_tables(spec, &document, &names, None).unwrap(),
            whole,
            "{name}"
        );
        for table in &whole {
            let one = Processor::run_tables(spec, &document, &[table.name()], None).unwrap();
            assert_eq!(one.len(), 1, "{name}: {}", table.name());
            assert_eq!(
                one.get(table.name()),
                Some(table),
                "{name}: {}",
                table.name()
            );

            let mut processor = Processor::with_tables(spec, &delims, &[table.name()]).unwrap();
            let mut seen = Vec::new();
            for segment in Tokenizer::with_delimiters(&bytes, delims) {
                seen.extend_from_slice(processor.feed(&segment).diagnostics());
            }
            seen.extend_from_slice(processor.finish().diagnostics());
            assert_eq!(seen, diagnostics, "{name}: {}", table.name());
        }
    }
}
