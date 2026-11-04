use super::*;
use crate::{LoopEngine, Tokenizer};
use proptest::prelude::*;

pub(super) use crate::spec::test_support::{SPEC, spec};

pub(super) use crate::delimiters::test_support::plain as delimiters;

/// Runs the engine and a projector over `input`, `finish` included.
pub(super) fn project(spec: &Spec, input: &str) -> (Tables, Vec<Diagnostic>) {
    let mut engine = LoopEngine::new(spec);
    let mut projector = Projector::new(spec, &delimiters());
    let mut diagnostics = Vec::new();
    for segment in Tokenizer::with_delimiters(input.as_bytes(), delimiters()) {
        let events = engine.feed(&segment);
        diagnostics.extend_from_slice(projector.on(&segment, events));
    }
    engine.finish();
    diagnostics.extend_from_slice(projector.finish());
    (projector.take_tables(), diagnostics)
}

/// A table as text: the column names, then one line per row.
pub(super) fn rows(tables: &Tables, name: &str) -> Vec<String> {
    let table = tables.get(name).unwrap();
    let header = table
        .columns()
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    std::iter::once(header)
        .chain((0..table.len()).map(|row| {
            table
                .columns()
                .iter()
                .map(|(_, column)| column.render(row).unwrap())
                .collect::<Vec<_>>()
                .join(" | ")
        }))
        .collect()
}

pub(super) fn rendered(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics.iter().map(ToString::to_string).collect()
}

#[test]
fn columns_fill_from_the_first_matching_segment_and_rows_close_with_their_loop() {
    let spec = spec();
    let (tables, diagnostics) = project(
        &spec,
        "HD*B1~NM*P*ACME~NM*P*OTHER~CL*C1*12.5*3*HC:99213~DT*151*20240101~DT*150*20240105~DT*150*20240106~RF*Q*X1~LN*L1~DT*472*20240107*1230~TR~",
    );
    assert_eq!(rendered(&diagnostics), Vec::<String>::new());
    assert_eq!(
        rows(&tables, "heads"),
        vec!["row | segment | batch | payer", "0 | 0 | B1 | ACME"]
    );
    assert_eq!(
        rows(&tables, "claims"),
        vec![
            "row | segment | head | charge | claim_id | code | first_line_at | from | procedure | reference | units",
            "0 | 3 | 0 | 12.50 | C1 | 99213 | 8 | 2024-01-05 | HC:99213 | X1 | 3",
        ]
    );
    assert_eq!(
        rows(&tables, "lines"),
        vec![
            "row | segment | head | claim | code | date | time",
            "0 | 8 | 0 | 0 | L1 | 2024-01-07 | 12:30:00",
        ]
    );
}

#[test]
fn a_text_value_past_the_column_limit_is_reported_and_its_cell_is_null() {
    let spec = spec();
    crate::column::OFFSET_LIMIT.with(|limit| limit.set(6));
    let (tables, diagnostics) = project(&spec, "HD*ABCD~TR~HD*EFGH~TR~");
    crate::column::OFFSET_LIMIT.with(|limit| limit.set(i32::MAX as usize));
    assert_eq!(
        rows(&tables, "heads"),
        vec![
            "row | segment | batch | payer",
            "0 | 0 | ABCD | ∅",
            "1 | 2 | ∅ | ∅"
        ],
        "the row is kept, with the cell null"
    );
    assert_eq!(
        rendered(&diagnostics),
        vec![
            "SNIP 2 · text for column \"batch\" of table \"heads\" was not stored: it would bring the column to 8 bytes and a column holds at most 2147483647 · segment #2 · at head#2 · datum \"EFGH\""
        ]
    );
}

#[test]
fn a_text_value_past_the_limit_in_a_segment_row_is_reported() {
    let spec = spec();
    crate::column::OFFSET_LIMIT.with(|limit| limit.set(3));
    let (tables, diagnostics) = project(&spec, "HD*A~CL*C*1~AJ*CO*X*1~AJ*CO*Y*2~TR~");
    crate::column::OFFSET_LIMIT.with(|limit| limit.set(i32::MAX as usize));
    assert_eq!(
        rows(&tables, "adjustments")[1..],
        [
            "0 | 2 | 0 | 0 | ∅ | 1.00 | CO | X",
            "1 | 3 | 0 | 0 | ∅ | 2.00 | ∅ | Y"
        ]
    );
    assert_eq!(
        rendered(&diagnostics),
        vec![
            "SNIP 2 · text for column \"group\" of table \"adjustments\" was not stored: it would bring the column to 4 bytes and a column holds at most 2147483647 · segment #3 · at head#1/claim#1 · datum \"CO\""
        ]
    );
}

#[test]
fn segment_ids_of_any_length_are_checked_and_read() {
    let spec = Spec::from_json(
        r#"{"name":"t",
            "loops":{"head":{"trigger":{"segment":"A"},"occurrences":{"a":{"segment":"A","pos":0},"longsegment":{"segment":"LONGSEGMENT","pos":1},"\u0000a":{"segment":"\u0000A","pos":2}},"end":"Z"}},
            "segments":{
                "LONGSEGMENT":{"elements":{"1":{"name":"n","type":"N0","required":true}}},
                "\u0000A":{"elements":{"1":{"name":"m","type":"AN"}}}
            },
            "tables":{"heads":{"loops":["head"],"ref":"head","columns":{
                "n":{"segment":"LONGSEGMENT","element":1},
                "m":{"segment":"\u0000A","element":1}
            }}}}"#,
    )
    .unwrap();
    let (tables, diagnostics) = project(&spec, "A~LONGSEGMENT*12~\0A*x~Z~A~LONGSEGMENT*q~Z~");
    assert_eq!(
        rows(&tables, "heads"),
        vec!["row | segment | m | n", "0 | 0 | x | 12", "1 | 4 | ∅ | ∅"]
    );
    assert_eq!(
        rendered(&diagnostics),
        vec![
            "SNIP 2 · element LONGSEGMENT01 (n) is not a valid N0 (integer with 0 implied decimals) · segment #5, element 1 · at head#2 · datum \"q\""
        ]
    );
}

#[test]
fn a_column_no_segment_matches_is_null() {
    let spec = spec();
    let (tables, _) = project(&spec, "HD*B1~CL*C1*1~TR~");
    assert_eq!(
        rows(&tables, "claims")[1],
        "0 | 1 | 0 | 1.00 | C1 | ∅ | ∅ | ∅ | ∅ | ∅ | ∅"
    );
    assert_eq!(rows(&tables, "heads")[1], "0 | 0 | B1 | ∅");
}

#[test]
fn a_text_cell_is_empty_when_its_element_is_written_empty_and_null_when_absent() {
    let spec = spec();
    // CL01, CL04-2 and RF02 are written empty: empty text. CL03 is written
    // empty too, but a number has no empty value: null.
    let (tables, _) = project(&spec, "HD*B1~CL**1**HC:~RF*Q*~TR~");
    assert_eq!(
        rows(&tables, "claims")[1],
        "0 | 1 | 0 | 1.00 |  |  | ∅ | ∅ | HC: |  | ∅"
    );
    // The segments stop before CL04-2 and RF02, and CL has no CL05: null.
    let (tables, _) = project(&spec, "HD*B1~CL*C1*1**HC~RF*Q~TR~");
    assert_eq!(
        rows(&tables, "claims")[1],
        "0 | 1 | 0 | 1.00 | C1 | ∅ | ∅ | ∅ | HC | ∅ | ∅"
    );
    let table = tables.get("claims").unwrap();
    let (_, code) = &table.columns()[5];
    assert_eq!(code.get(0), Some(crate::column::Cell::Null));
    let (tables, _) = project(&spec, "HD*B1~CL**1~TR~");
    let table = tables.get("claims").unwrap();
    let (_, claim_id) = &table.columns()[4];
    assert_eq!(claim_id.get(0), Some(crate::column::Cell::Binary(&b""[..])));
}

#[test]
fn every_row_names_the_open_row_of_each_table_above_it() {
    let spec = spec();
    let (tables, _) = project(
        &spec,
        "HD*B1~CL*C1*1~AJ*CO*45*10~LN*L1~AJ*PR*1*2~LN*L2~CL*C2*2~LN*L3~AJ*OA*3*4*5*6~TR~",
    );
    assert_eq!(
        rows(&tables, "lines"),
        vec![
            "row | segment | head | claim | code | date | time",
            "0 | 3 | 0 | 0 | L1 | ∅ | ∅",
            "1 | 5 | 0 | 0 | L2 | ∅ | ∅",
            "2 | 7 | 0 | 1 | L3 | ∅ | ∅",
        ]
    );
    assert_eq!(
        rows(&tables, "adjustments"),
        vec![
            "row | segment | head | claim | line | amount | group | reason",
            "0 | 2 | 0 | 0 | ∅ | 10.00 | CO | 45",
            "1 | 4 | 0 | 0 | 0 | 2.00 | PR | 1",
            "2 | 8 | 0 | 1 | 2 | 4.00 | OA | 3",
            "3 | 8 | 0 | 1 | 2 | 6.00 | OA | 5",
        ]
    );
}

#[test]
fn a_repeated_group_without_its_first_element_gives_no_row() {
    let spec = spec();
    let (tables, _) = project(&spec, "HD*B1~CL*C1*1~AJ*CO*45*10**7~AJ*PR~TR~");
    assert_eq!(
        rows(&tables, "adjustments"),
        vec![
            "row | segment | head | claim | line | amount | group | reason",
            "0 | 2 | 0 | 0 | ∅ | 10.00 | CO | 45",
        ]
    );
}

#[test]
fn row_numbers_keep_counting_across_drains() {
    let spec = spec();
    let mut engine = LoopEngine::new(&spec);
    let mut projector = Projector::new(&spec, &delimiters());
    let mut drains = Vec::new();
    let input = "HD*B1~CL*C1*1~LN*L1~CL*C2*2~LN*L2~TR~";
    for segment in Tokenizer::with_delimiters(input.as_bytes(), delimiters()) {
        let events = engine.feed(&segment).to_vec();
        projector.on(&segment, &events);
        if events.contains(&Event::LoopClosed {
            id: spec.loop_id("claim").unwrap(),
        }) {
            drains.push(projector.take_tables());
        }
    }
    engine.finish();
    projector.finish();
    drains.push(projector.take_tables());
    let lines: Vec<Vec<String>> = drains
        .iter()
        .map(|tables| rows(tables, "lines").split_off(1))
        .collect();
    assert_eq!(
        lines,
        vec![
            vec!["0 | 2 | 0 | 0 | L1 | ∅ | ∅".to_string()],
            vec!["1 | 4 | 0 | 1 | L2 | ∅ | ∅".to_string()],
            vec![],
        ]
    );
    assert_eq!(
        rows(&drains[1], "heads").split_off(1),
        vec!["0 | 0 | B1 | ∅"],
        "the end segment closes the head right after the last claim"
    );
    assert!(drains[2].iter().all(Table::is_empty));
}

#[test]
fn finishing_appends_the_rows_still_open() {
    let spec = spec();
    let (tables, _) = project(&spec, "HD*B1~CL*C1*1~LN*L1~");
    assert_eq!(tables.get("heads").unwrap().len(), 1);
    assert_eq!(tables.get("claims").unwrap().len(), 1);
    assert_eq!(tables.get("lines").unwrap().len(), 1);
}

#[test]
fn a_value_that_is_not_its_type_is_reported_and_null() {
    let spec = spec();
    let (tables, diagnostics) = project(&spec, "HD*B1~CL*C1*12A~TR~");
    assert_eq!(
        rendered(&diagnostics),
        vec![
            "SNIP 2 · element CL02 (charge) is not a valid R (decimal, scale 2) · segment #1, element 2 · at head#1/claim#1 · datum \"12A\""
        ]
    );
    assert_eq!(
        tables
            .get("claims")
            .unwrap()
            .column("charge")
            .unwrap()
            .get(0),
        Some(Cell::Null)
    );
}

#[test]
fn a_decimal_with_more_places_than_its_scale_is_a_type_mismatch() {
    let spec = spec();
    let (_, diagnostics) = project(&spec, "HD*B1~CL*C1*1.234~TR~");
    assert_eq!(
        rendered(&diagnostics),
        vec![
            "SNIP 2 · element CL02 (charge) is not a valid R (decimal, scale 2) · segment #1, element 2 · at head#1/claim#1 · datum \"1.234\""
        ]
    );
}

#[test]
fn an_empty_required_element_is_reported_with_its_name() {
    let spec = spec();
    let (_, diagnostics) = project(&spec, "HD*B1~CL**5~TR~");
    assert_eq!(
        rendered(&diagnostics),
        vec![
            "SNIP 2 · required element CL01 (claim_id) is missing or empty · segment #1, element 1 · at head#1/claim#1 · datum \"\""
        ]
    );
}

#[test]
fn an_invalid_date_names_the_element_and_the_text() {
    let spec = spec();
    let (tables, diagnostics) = project(&spec, "HD*B1~CL*C1*1~DT*150*20240230~TR~");
    assert_eq!(
        rendered(&diagnostics),
        vec![
            "SNIP 2 · element DT02 (date) is not a valid DT (date CCYYMMDD or YYMMDD) · segment #2, element 2 · at head#1/claim#1 · datum \"20240230\""
        ]
    );
    assert_eq!(
        tables.get("claims").unwrap().column("from").unwrap().get(0),
        Some(Cell::Null),
        "the first matching DTM decides the column, even when its value is invalid"
    );
}

#[test]
fn lengths_count_bytes_for_text_and_digits_for_numbers() {
    let spec = spec();
    let (tables, diagnostics) = project(
        &spec,
        "HD*B1~CL*ABCDEFGHIJK*-12345678.90*-12~CL*C2*1*123~TR~",
    );
    assert_eq!(
        rendered(&diagnostics),
        vec![
            "SNIP 2 · element CL01 (claim_id) has length 11; the spec allows 1 to 10 · segment #1, element 1 · at head#1/claim#1 · datum \"ABCDEFGHIJK\"",
            "SNIP 2 · element CL03 (units) has length 3; the spec allows at most 2 · segment #2, element 3 · at head#1/claim#2 · datum \"123\"",
        ]
    );
    let claims = tables.get("claims").unwrap();
    assert_eq!(
        claims.column("claim_id").unwrap().get(0),
        Some(Cell::Binary(b"ABCDEFGHIJK")),
        "a value out of range is reported and kept"
    );
    assert_eq!(
        claims.column("charge").unwrap().get(0),
        Some(Cell::Decimal128(-1_234_567_890))
    );
    assert_eq!(
        claims.column("units").unwrap().get(0),
        Some(Cell::Int64(-12))
    );
}

#[test]
fn a_composite_with_more_components_than_declared_names_the_first_extra_one() {
    let spec = spec();
    let (_, diagnostics) = project(&spec, "HD*B1~CL*C1*1**HC:1:X~TR~");
    assert_eq!(
        rendered(&diagnostics),
        vec![
            "SNIP 2 · element CL04 (procedure) has 3 components; the spec declares 2 · segment #1, element 4, component 3 · at head#1/claim#1 · datum \"X\""
        ]
    );
}

#[test]
fn components_are_checked_where_the_definition_declares_them() {
    let spec = spec();
    let (tables, diagnostics) = project(&spec, "HD*B1~CL*C1*1**HCPC~TR~");
    assert_eq!(
        rendered(&diagnostics),
        vec![
            "SNIP 2 · element CL04-1 (qualifier) has length 4; the spec allows 2 to 2 · segment #1, element 4, component 1 · at head#1/claim#1 · datum \"HCPC\"",
            "SNIP 2 · required element CL04-2 (code) is missing or empty · segment #1, element 4, component 2 · at head#1/claim#1 · datum \"\"",
        ]
    );
    assert_eq!(
        tables
            .get("claims")
            .unwrap()
            .column("procedure")
            .unwrap()
            .get(0),
        Some(Cell::Binary(b"HCPC"))
    );
}

#[test]
fn an_element_defined_without_components_is_read_as_one_text() {
    let spec = spec();
    let (tables, diagnostics) = project(&spec, "HD*A:B~TR~");
    assert_eq!(rendered(&diagnostics), Vec::<String>::new());
    assert_eq!(
        tables.get("heads").unwrap().column("batch").unwrap().get(0),
        Some(Cell::Binary(b"A:B"))
    );
    let (_, diagnostics) = project(&spec, "HD*A:BCDE~TR~");
    assert_eq!(
        rendered(&diagnostics),
        vec![
            "SNIP 2 · element HD01 (batch) has length 6; the spec allows 1 to 5 · segment #0, element 1 · at head#1 · datum \"A:BCDE\""
        ]
    );
}

#[test]
fn only_captured_segments_with_a_definition_are_checked() {
    let spec = spec();
    let (_, diagnostics) = project(&spec, "HD*B1~ZZ*whatever~QQ*!~CL*C1*1~RF~TR~");
    assert_eq!(rendered(&diagnostics), Vec::<String>::new());

    // `AJ` is defined, but no open loop holds it here: it is unmatched,
    // so its empty required `AJ01` is not reported and it gives no row.
    let (tables, diagnostics) = project(&spec, "HD*B1~AJ~TR~");
    assert_eq!(rendered(&diagnostics), Vec::<String>::new());
    assert_eq!(tables.get("adjustments").unwrap().len(), 0);

    // `CL04` is read by two columns; its one bad component is reported once.
    let (tables, diagnostics) = project(&spec, "HD*B1~CL*C1*1**HC:TOOLONG~TR~");
    assert_eq!(
        rendered(&diagnostics),
        vec![
            "SNIP 2 · element CL04-2 (code) has length 7; the spec allows at most 5 · segment #1, element 4, component 2 · at head#1/claim#1 · datum \"TOOLONG\""
        ]
    );
    let claims = tables.get("claims").unwrap();
    assert_eq!(
        claims.column("procedure").unwrap().get(0),
        Some(Cell::Binary(b"HC:TOOLONG"))
    );
    assert_eq!(
        claims.column("code").unwrap().get(0),
        Some(Cell::Binary(b"TOOLONG"))
    );
}

#[test]
fn the_spec_tables_give_the_columns_and_their_types() {
    let spec = spec();
    let (tables, _) = project(&spec, "");
    let kinds: Vec<(&str, ColumnType)> = tables
        .get("lines")
        .unwrap()
        .columns()
        .iter()
        .map(|(name, column)| (name.as_str(), column.kind()))
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("row", ColumnType::Int64 { scale: 0 }),
            ("segment", ColumnType::Int64 { scale: 0 }),
            ("head", ColumnType::Int64 { scale: 0 }),
            ("claim", ColumnType::Int64 { scale: 0 }),
            ("code", ColumnType::Binary),
            ("date", ColumnType::Date32),
            ("time", ColumnType::Time32),
        ]
    );
    let names: Vec<&str> = tables.iter().map(Table::name).collect();
    assert_eq!(names, vec!["adjustments", "claims", "heads", "lines"]);
}

/// An `R` value written the way X12 writes it, with two decimals.
fn money(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let cents = cents.unsigned_abs();
    format!("{sign}{}.{:02}", cents / 100, cents % 100)
}

proptest! {
    #[test]
    fn valid_values_never_raise_a_diagnostic(
        claim_id in "[A-Z0-9]{1,10}",
        cents in -99_999_999i64..99_999_999,
        units in 0i64..99,
        (year, month, day) in (1900i32..2100, 1i32..=12, 1i32..=28),
        seconds in 0i32..86_400,
    ) {
        let spec = spec();
        let input = format!(
            "HD*B1~CL*{claim_id}*{}*{units}*HC:X1~LN*L1~DT*472*{year:04}{month:02}{day:02}*{:02}{:02}{:02}~TR~",
            money(cents),
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60,
        );
        let (tables, diagnostics) = project(&spec, &input);
        prop_assert_eq!(rendered(&diagnostics), Vec::<String>::new());
        let claims = tables.get("claims").unwrap();
        prop_assert_eq!(claims.column("charge").unwrap().get(0), Some(Cell::Decimal128(i128::from(cents))));
        prop_assert_eq!(claims.column("units").unwrap().get(0), Some(Cell::Int64(units)));
        let lines = tables.get("lines").unwrap();
        prop_assert_eq!(lines.column("time").unwrap().get(0), Some(Cell::Time32(seconds)));
    }

    #[test]
    fn random_elements_never_panic_and_rows_stay_aligned(
        bodies in proptest::collection::vec("[A-Z0-9*:.\\-]{0,24}", 0..8),
    ) {
        let spec = spec();
        let mut input = String::from("HD*B1~");
        for (i, body) in bodies.iter().enumerate() {
            let id = ["CL", "LN", "DT", "AJ", "RF", "NM"][i % 6];
            input.push_str(&format!("{id}*{body}~"));
        }
        input.push_str("TR~");
        let (tables, _) = project(&spec, &input);
        for table in &tables {
            for (_, column) in table.columns() {
                prop_assert_eq!(column.len(), table.len());
            }
        }
    }
}

#[test]
fn an_unknown_table_displays_the_rule_the_name_and_the_spec_tables() {
    let spec = Spec::builtin_835();
    let error = UnknownTable::new(&spec, "claim");
    let names: Vec<String> = spec
        .tables()
        .iter()
        .map(|def| format!("{:?}", def.name))
        .collect();
    assert_eq!(
        error.to_string(),
        format!(
            "table \"claim\" is not defined by the spec, so it cannot be filled; its tables are {}",
            names.join(", ")
        )
    );
}
