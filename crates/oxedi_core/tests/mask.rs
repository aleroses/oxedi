//! Masked projections of every fixture and sample, each under the built-in
//! spec it selects: the same tables, columns and rows as without a mask, the
//! columns the spec does not flag unchanged, the flagged ones deterministic
//! per key, and no original value of a flagged column left in any cell or
//! diagnostic text.

mod common;

use std::collections::BTreeSet;

use oxedi_core::{Cell, DateMode, Diagnostic, Document, Mask, Phi, Processor, Spec, Table, Tables};

const KEY: &[u8] = b"oxedi-test-key-0";
const OTHER_KEY: &[u8] = b"oxedi-test-key-1";

/// Every file with its bytes, its spec, and its unmasked and masked runs.
fn runs(
    dates: DateMode,
    mut check: impl FnMut(&str, &Spec, (&Tables, &[Diagnostic]), (&Tables, &[Diagnostic])),
) {
    let (five, four) = common::builtins();
    let mask = Mask::new(KEY, dates).unwrap();
    for (name, bytes, delims) in common::all_files() {
        let spec = common::select(&five, &four, &bytes, delims);
        let document = Document::with_delimiters(&bytes[..], delims).unwrap();
        let (plain, plain_diagnostics) = Processor::run(spec, &document, None);
        let (masked, masked_diagnostics) = Processor::run(spec, &document, Some(&mask));
        check(
            &name,
            spec,
            (&plain, &plain_diagnostics),
            (&masked, &masked_diagnostics),
        );
    }
}

/// The kind of patient data each column of `table` holds, by column name.
fn flag(spec: &Spec, table: &Table, column: &str) -> Option<Phi> {
    let def = spec.tables().iter().find(|def| def.name == table.name())?;
    let index = def.columns.iter().position(|(name, _)| name == column)?;
    def.phi(index)
}

#[test]
fn a_mask_keeps_the_shape_and_every_column_it_does_not_flag() {
    runs(
        DateMode::Year,
        |name, spec, (plain, plain_d), (masked, masked_d)| {
            assert_eq!(plain.len(), masked.len(), "{name}");
            for (before, after) in plain.iter().zip(masked) {
                assert_eq!(before.name(), after.name(), "{name}");
                assert_eq!(before.len(), after.len(), "{name} {}", before.name());
                let kinds = |table: &Table| -> Vec<String> {
                    table
                        .columns()
                        .iter()
                        .map(|(column, data)| format!("{column}: {}", data.kind()))
                        .collect()
                };
                assert_eq!(kinds(before), kinds(after), "{name} {}", before.name());
                for ((column, was), (_, now)) in before.columns().iter().zip(after.columns()) {
                    for row in 0..before.len() {
                        let (was, now) = (was.get(row).unwrap(), now.get(row).unwrap());
                        let at = format!("{name} {}.{column} row {row}", before.name());
                        match (flag(spec, before, column), was) {
                            (None, _) => assert_eq!(was, now, "{at}"),
                            (Some(_), Cell::Null) => assert_eq!(now, Cell::Null, "{at}"),
                            (Some(_), Cell::Binary(b"")) => {
                                assert_eq!(now, Cell::Binary(b""), "{at}")
                            }
                            (Some(Phi::Name | Phi::Id), Cell::Binary(_)) => {
                                let Cell::Binary(token) = now else {
                                    panic!("{at}: {now:?}")
                                };
                                assert_eq!(token.len(), 16, "{at}");
                                assert!(
                                    token
                                        .iter()
                                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
                                    "{at}"
                                );
                            }
                            (Some(Phi::Date), Cell::Date32(days)) => {
                                assert_eq!(
                                    now,
                                    Cell::Date32(Mask::year_start(days).unwrap()),
                                    "{at}"
                                );
                            }
                            (Some(kind), other) => panic!("{at}: {kind:?} column holds {other:?}"),
                        }
                    }
                }
            }
            assert_eq!(plain_d.len(), masked_d.len(), "{name}");
            for (before, after) in plain_d.iter().zip(masked_d) {
                assert_eq!(
                    (
                        &before.rule,
                        before.segment,
                        before.element,
                        before.component,
                        &before.path
                    ),
                    (
                        &after.rule,
                        after.segment,
                        after.element,
                        after.component,
                        &after.path
                    ),
                    "{name}: only the datum may change"
                );
            }
        },
    );
}

#[test]
fn masked_cells_depend_on_the_key_only() {
    let (five, four) = common::builtins();
    let mask = Mask::new(KEY, DateMode::Shift).unwrap();
    let other = Mask::new(OTHER_KEY, DateMode::Shift).unwrap();
    for (name, bytes, delims) in common::all_files() {
        let spec = common::select(&five, &four, &bytes, delims);
        let document = Document::with_delimiters(&bytes[..], delims).unwrap();
        let once = Processor::run(spec, &document, Some(&mask));
        let again = Processor::run(spec, &document, Some(&mask));
        assert_eq!(once, again, "{name}: the same key gives the same output");
        let (elsewhere, _) = Processor::run(spec, &document, Some(&other));
        for (table, changed) in once.0.iter().zip(&elsewhere) {
            for ((column, a), (_, b)) in table.columns().iter().zip(changed.columns()) {
                if !matches!(flag(spec, table, column), Some(Phi::Name | Phi::Id)) {
                    continue;
                }
                for row in 0..table.len() {
                    if let Some(Cell::Binary(value)) = a.get(row)
                        && !value.is_empty()
                    {
                        assert_ne!(
                            a.get(row),
                            b.get(row),
                            "{name} {}.{column} row {row}: another key gives another token",
                            table.name()
                        );
                    }
                }
            }
        }
    }
}

/// An original value of a flagged column, as the file writes it, and the
/// first place it was read at.
struct Original {
    kind: Phi,
    text: String,
    table: String,
    column: String,
    row: usize,
}

/// Every non-empty value of a flagged column, as the file writes it: text as
/// is, dates as `CCYYMMDD`, and dates also as tables render them unless
/// masking leaves them unchanged (already January 1). A value read at
/// several places is listed once, at the first.
fn originals(spec: &Spec, plain: &Tables) -> Vec<Original> {
    let mut found = Vec::new();
    for table in plain {
        for (column, data) in table.columns() {
            let Some(kind) = flag(spec, table, column) else {
                continue;
            };
            let mut push = |text: String, row: usize| {
                found.push(Original {
                    kind,
                    text,
                    table: table.name().to_string(),
                    column: column.to_string(),
                    row,
                });
            };
            for row in 0..table.len() {
                match data.get(row) {
                    Some(Cell::Binary(value)) if !value.is_empty() => {
                        push(String::from_utf8_lossy(value).into_owned(), row);
                    }
                    Some(Cell::Date32(days)) => {
                        let rendered = data.render(row).unwrap();
                        push(rendered.replace('-', ""), row);
                        if Mask::year_start(days) != Some(days) {
                            push(rendered, row);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    found.sort_by(|a, b| a.text.cmp(&b.text));
    found.dedup_by(|later, first| later.kind == first.kind && later.text == first.text);
    found
}

/// Whether `text` is a token: 16 lowercase hex characters.
fn is_token(text: &str) -> bool {
    text.len() == 16
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A place whose values equal values of a flagged column in the data itself:
/// the cell is not masked and reads the same without a mask, so the masking
/// did not put it there. It names the file, the table and column of the
/// originals, and the table and column of the cell.
struct Coincidence {
    file: &'static str,
    original: (&'static str, &'static str),
    cell: (&'static str, &'static str),
    rows: usize,
    values: usize,
    reason: &'static str,
}

/// The known coincidences of the anonymized files. Every other original is
/// searched in every cell it can leak into, and a cell of these columns
/// counts only when it reads the same as in the unmasked output. A test fails
/// when an entry occurs in more or fewer rows than `rows`, or with more or
/// fewer distinct cell texts than `values`, so the list can neither outlive
/// the data it describes nor cover a new copy of patient data.
const COINCIDENCES: &[Coincidence] = &[
    Coincidence {
        file: "united_healthcare_legacy_sample.txt",
        original: ("claims", "patient_id"),
        cell: ("payments", "payer_identifier"),
        rows: 1,
        values: 1,
        reason: "a nine-digit patient id is a prefix of the ten-digit payer identifier",
    },
    Coincidence {
        file: "united_healthcare_legacy_sample.txt",
        original: ("claims", "patient_id"),
        cell: ("payments", "trace_number"),
        rows: 1,
        values: 1,
        reason: "a nine-digit patient id is a prefix of the ten-digit trace number",
    },
    Coincidence {
        file: "edi835_test_eyemed.RMT",
        original: ("claims", "patient_last_name"),
        cell: ("claims", "rendering_provider_name"),
        rows: 1,
        values: 1,
        reason: "a rendering provider is named like a patient",
    },
    Coincidence {
        file: "edi835_test_united.rmt",
        original: ("claims", "patient_last_name"),
        cell: ("claims", "rendering_provider_name"),
        rows: 1332,
        values: 30,
        reason: "rendering providers are named like patients",
    },
];

/// The shortest id searched in a cell of a column the spec does not flag.
const ID_MIN: usize = 6;

/// The shortest name searched in a cell of a column the spec does not flag.
const NAME_MIN: usize = 3;

/// A masked cell leaks an original of its own kind when it holds it; a
/// token is hex, so a short original made of hex digits can appear in one by
/// chance, and a token leaks only by being the original. A diagnostic's text
/// also holds positions and counts, so an original shorter than six bytes
/// leaks there only as a whole quoted datum.
///
/// Every cell of every table, flagged or not, is scanned for an `id`
/// original of six or more characters and a `name` original of three or more
/// (a patient control number can ride in an unflagged column, such as a
/// provider adjustment reference): no cell contains it, but for the entries
/// of [`COINCIDENCES`]. A shorter `id` or `name` is checked against flagged
/// cells only, where its kind of data is. Dates are compared with flagged
/// cells when they reduce to the year, and by row in
/// [`shifted_dates_equal_their_own_date_plus_the_rows_offset`] when they shift.
#[test]
fn no_original_value_of_a_flagged_column_survives_masking() {
    let mut used = Used::new();
    for dates in [DateMode::Year, DateMode::Shift] {
        no_original_survives(dates, &mut used);
    }
    for (index, entry) in COINCIDENCES.iter().enumerate() {
        let rows: BTreeSet<usize> = used
            .iter()
            .filter(|(at, ..)| *at == index)
            .map(|(_, row, _)| *row)
            .collect();
        let values: BTreeSet<&str> = used
            .iter()
            .filter(|(at, ..)| *at == index)
            .map(|(.., cell)| cell.as_str())
            .collect();
        assert_eq!(
            (rows.len(), values.len()),
            (entry.rows, entry.values),
            "{}: {}.{} coincides with a cell of {}.{} in {} rows with {} distinct texts, \
             not {} rows with {} ({})",
            entry.file,
            entry.original.0,
            entry.original.1,
            entry.cell.0,
            entry.cell.1,
            rows.len(),
            values.len(),
            entry.rows,
            entry.values,
            entry.reason
        );
    }
}

/// Each entry of [`COINCIDENCES`] that occurs, with the row and text of its
/// cell.
type Used = BTreeSet<(usize, usize, String)>;

fn no_original_survives(dates: DateMode, used: &mut Used) {
    runs(dates, |name, spec, (plain, _), (masked, diagnostics)| {
        let originals = originals(spec, plain);
        assert!(!originals.is_empty(), "{name} has patient data to mask");
        let mut cells = Vec::new();
        for table in masked {
            for (column, data) in table.columns() {
                let kind = flag(spec, table, column);
                cells.extend((0..table.len()).filter_map(|row| {
                    Some((kind, data.render(row)?, table.name(), column.as_str(), row))
                }));
            }
        }
        let texts: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
        for original in &originals {
            let is_date = original.kind == Phi::Date;
            // A shifted date can land on the original date of another row.
            if !(is_date && dates == DateMode::Shift) {
                for (own, cell, table, column, row) in &cells {
                    let searched = match own {
                        Some(own) => (*own == Phi::Date) == is_date,
                        None => match original.kind {
                            Phi::Id => original.text.len() >= ID_MIN,
                            Phi::Name => original.text.len() >= NAME_MIN,
                            Phi::Date => false,
                        },
                    };
                    let leaks = searched
                        && if is_token(cell) {
                            *cell == original.text
                        } else {
                            cell.contains(original.text.as_str())
                        };
                    if !leaks {
                        continue;
                    }
                    let known = COINCIDENCES.iter().position(|entry| {
                        entry.file == name
                            && entry.original == (&*original.table, &*original.column)
                            && entry.cell == (*table, *column)
                    });
                    match known {
                        Some(index)
                            if rendered(plain, table, column, *row) == Some(cell.clone()) =>
                        {
                            used.insert((index, *row, cell.clone()));
                        }
                        _ => panic!(
                            "{name}: {:?} read at {}.{} row {} leaks in {table}.{column} row {row} cell {cell:?}",
                            original.text, original.table, original.column, original.row
                        ),
                    }
                }
            }
            let quoted = format!("{:?}", original.text);
            for text in &texts {
                let leaks = text.contains(&quoted)
                    || (original.text.len() >= 6 && text.contains(original.text.as_str()));
                assert!(!leaks, "{name}: {:?} leaks in {text:?}", original.text);
            }
        }
    });
}

/// The text of the cell of `column` in `row` of the table `table`.
fn rendered(tables: &Tables, table: &str, column: &str, row: usize) -> Option<String> {
    named(tables, table)
        .columns()
        .iter()
        .find(|(name, _)| name == column)?
        .1
        .render(row)
}

/// The table named `name`.
fn named<'t>(tables: &'t Tables, name: &str) -> &'t Table {
    tables
        .iter()
        .find(|table| table.name() == name)
        .unwrap_or_else(|| panic!("no table {name}"))
}

/// The cell of `column` in `row` of `table`.
fn cell_at<'t>(table: &'t Table, column: &str, row: usize) -> Option<Cell<'t>> {
    table
        .columns()
        .iter()
        .find(|(name, _)| name == column)?
        .1
        .get(row)
}

/// The unmasked seed of `row` of the table `index` of the spec: its own seed
/// column, or that of the nearest table above it that has one, read at the
/// row the reference column points to.
fn seed<'t>(spec: &Spec, plain: &'t Tables, index: usize, row: usize) -> Option<&'t [u8]> {
    let tables = spec.tables();
    let table = named(plain, &tables[index].name);
    let (owner, owner_row) = match tables[index].phi_seed() {
        Some(_) => (index, row),
        None => {
            let above = tables[index]
                .ancestors
                .iter()
                .rev()
                .copied()
                .find(|&above| tables[above].phi_seed().is_some())?;
            let Some(Cell::Int64(at)) = cell_at(table, &tables[above].reference, row) else {
                return None;
            };
            (above, usize::try_from(at).ok()?)
        }
    };
    let column = &tables[owner].columns[tables[owner].phi_seed()?].0;
    match cell_at(named(plain, &tables[owner].name), column, owner_row)? {
        Cell::Binary(value) if !value.is_empty() => Some(value),
        _ => None,
    }
}

/// In `shift` mode every masked date of a flagged column is its unmasked date
/// plus the days its row's seed gives, or January 1 of its year when the row
/// has no seed.
#[test]
fn shifted_dates_equal_their_own_date_plus_the_rows_offset() {
    let mask = Mask::new(KEY, DateMode::Shift).unwrap();
    let mut seeded = 0;
    runs(DateMode::Shift, |name, spec, (plain, _), (masked, _)| {
        for (index, def) in spec.tables().iter().enumerate() {
            let (before, after) = (named(plain, &def.name), named(masked, &def.name));
            for (column, _) in before.columns() {
                if flag(spec, before, column) != Some(Phi::Date) {
                    continue;
                }
                for row in 0..before.len() {
                    let Some(Cell::Date32(was)) = cell_at(before, column, row) else {
                        continue;
                    };
                    let Some(Cell::Date32(now)) = cell_at(after, column, row) else {
                        panic!(
                            "{name} {}.{column} row {row}: a date became {:?}",
                            def.name,
                            cell_at(after, column, row)
                        );
                    };
                    let expected = match seed(spec, plain, index, row) {
                        Some(seed) => {
                            seeded += 1;
                            was + mask.shift_days(seed)
                        }
                        None => Mask::year_start(was).unwrap(),
                    };
                    assert_eq!(
                        now, expected,
                        "{name} {}.{column} row {row}: the date moved by the wrong amount",
                        def.name
                    );
                }
            }
        }
    });
    assert!(seeded > 0, "some row has a seed to shift its dates by");
}

/// The fixtures built to carry findings on flagged places: one finding each on
/// a claim id, a member id, a group number, a statement date and a line item
/// control number, under both versions.
#[test]
fn a_finding_on_a_flagged_place_has_its_datum_replaced() {
    let (five, four) = common::builtins();
    for dates in [DateMode::Year, DateMode::Shift] {
        let mask = Mask::new(KEY, dates).unwrap();
        for (name, date) in [
            ("masked_findings_5010_sample.txt", "20241341"),
            ("masked_findings_4010_sample.txt", "241341"),
        ] {
            let bytes = common::load_fixture(name);
            let delims = oxedi_core::Delimiters::from_isa(&bytes).unwrap();
            let spec = common::select(&five, &four, &bytes, delims);
            let document = Document::with_delimiters(&bytes[..], delims).unwrap();
            let (_, plain) = Processor::run(spec, &document, None);
            let (_, masked) = Processor::run(spec, &document, Some(&mask));
            let datums = |found: &[Diagnostic]| -> Vec<String> {
                found
                    .iter()
                    .map(|d| String::from_utf8_lossy(&d.datum).into_owned())
                    .collect()
            };
            let plain = datums(&plain);
            let masked = datums(&masked);
            assert_eq!(plain.len(), 5, "{name}: {plain:?}");
            assert_eq!(masked.len(), plain.len(), "{name}");
            assert!(plain.iter().any(|d| d == date), "{name}");
            for (was, now) in plain.iter().zip(&masked) {
                assert_ne!(was, now, "{name} {dates:?}");
                assert!(is_token(now), "{name} {dates:?}: {now:?}");
            }
        }
    }
}
