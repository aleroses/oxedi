//! The writer on tables edited from parsed files: the envelope it writes,
//! how values become elements, and every finding and error it reports,
//! each with its full text.

mod common;

use oxedi_core::write::{Envelope, Finding, WriteError, write, write_with_findings};
use oxedi_core::{Cell, ColumnType, Delimiters, Document, Processor, Spec, Table, Tables};

/// 2024-01-01 at 12:30.
fn envelope() -> Envelope {
    Envelope::new("ZZ", "SENDER", "ZZ", "RECEIVER", 19_723, 45_000)
}

fn parse(spec: &Spec, bytes: &[u8]) -> Tables {
    let document = Document::parse(bytes).unwrap();
    Processor::run(spec, &document, None).0
}

/// emedny (5010): 3 claims, services, adjustments.
fn emedny() -> (Spec, Tables) {
    let spec = Spec::builtin_835();
    let tables = parse(&spec, &common::load_fixture("emedny_sample.txt"));
    (spec, tables)
}

/// The tables with `table` rebuilt from `rows` (positions, repeats
/// allowed) and each cell passed through `edit(column, new row, cell)`.
fn rebuild<'a>(
    tables: &'a Tables,
    table: &str,
    rows: Option<&[usize]>,
    edit: impl Fn(&str, usize, Cell<'a>) -> Cell<'a>,
) -> Tables {
    let mut out = Vec::new();
    for original in tables {
        if original.name() != table {
            out.push(original.clone());
            continue;
        }
        let columns: Vec<(String, ColumnType)> = original
            .columns()
            .iter()
            .map(|(name, data)| (name.clone(), data.kind()))
            .collect();
        let mut rebuilt = Table::new(original.name(), columns);
        let all: Vec<usize> = (0..original.len()).collect();
        for (at, &row) in rows.unwrap_or(&all).iter().enumerate() {
            let cells: Vec<Cell<'a>> = original
                .columns()
                .iter()
                .map(|(name, data)| edit(name, at, data.get(row).unwrap()))
                .collect();
            rebuilt.push_row(&cells).unwrap();
        }
        out.push(rebuilt);
    }
    Tables::new(out)
}

/// One cell replaced.
fn set<'a>(tables: &'a Tables, table: &str, column: &str, row: usize, cell: Cell<'a>) -> Tables {
    rebuild(tables, table, None, |name, at, old| {
        if name == column && at == row {
            cell
        } else {
            old
        }
    })
}

fn findings(result: Result<Vec<u8>, WriteError>) -> Vec<String> {
    match result {
        Ok(_) => panic!("written without findings"),
        Err(WriteError::Findings(findings)) => findings.iter().map(ToString::to_string).collect(),
        Err(other) => panic!("not a finding: {other}"),
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn the_envelope_has_fixed_widths_counts_and_matching_control_numbers() {
    let (spec, tables) = emedny();
    let mut envelope = envelope();
    envelope.control_number = 42;
    envelope.usage_indicator = "T".into();
    envelope.application_sender = Some("APP".into());
    envelope.line_break = true;
    let written = text(&write(&spec, &tables, &envelope).unwrap());
    let lines: Vec<&str> = written.lines().collect();
    assert_eq!(
        lines[0],
        "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1230*^*00501*000000042*0*T*:~"
    );
    assert_eq!(lines[0].len(), 106);
    assert_eq!(
        lines[1],
        "GS*HP*APP*RECEIVER*20240101*1230*42*X*005010X221A1~"
    );
    assert_eq!(lines[2], "ST*835*0042~");
    let transaction = lines.len() - 4;
    assert_eq!(lines[lines.len() - 3], format!("SE*{transaction}*0042~"));
    assert_eq!(lines[lines.len() - 2], "GE*1*42~");
    assert_eq!(lines[lines.len() - 1], "IEA*1*000000042~");
}

#[test]
fn the_4010_interchange_writes_its_own_codes() {
    let spec = Spec::builtin_835_4010();
    let tables = parse(&spec, &common::load_sample("edi835_test_davisvision.RMT"));
    let written = text(&write(&spec, &tables, &envelope()).unwrap());
    assert!(written.starts_with(
        "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1230*U*00401*000000001*0*P*:~GS*HP*SENDER*RECEIVER*20240101*1230*1*X*004010X091A1~ST*835*0001~"
    ));
    let segments = written.matches('~').count();
    // Every segment but ISA, GS, GE and IEA is in the transaction.
    assert!(written.ends_with(&format!(
        "~SE*{}*0001~GE*1*1~IEA*1*000000001~",
        segments - 4
    )));
}

#[test]
fn values_become_elements_with_empty_text_kept_and_trailing_ones_trimmed() {
    let spec = Spec::builtin_835_4010();
    let tables = parse(&spec, &common::load_sample("edi835_test_davisvision.RMT"));
    let written = text(&write(&spec, &tables, &envelope()).unwrap());
    // BPR05 is empty text between values; decimals lose trailing zeros;
    // a composite joins its components; groups of a repeat follow one another.
    assert!(written.contains("~BPR*H*0*C*NON************20240103~"));
    assert!(written.contains("~CLP*9051716610428903704*1*-55*-170**HM*DRN28407545.1*11~"));
    assert!(written.contains("~SVC*HC:V2020*145*-170~"));
    assert!(written.contains("~PLB*878324429*20240101*FB:YMUTWU 04459 VUQ URE 8.61 NJX*-170~"));
    let null = set(&tables, "claims", "frequency_code", 0, Cell::Null);
    let null = set(&null, "claims", "facility_type", 0, Cell::Null);
    let written = text(&write(&spec, &null, &envelope()).unwrap());
    assert!(written.contains("~CLP*9051716610428903704*1*-55*-170**HM*DRN28407545.1~"));
}

#[test]
fn one_group_per_run_of_equal_header_numbers() {
    let (spec, tables) = emedny();
    let count = |tables: &Tables| {
        let written = write_with_findings(&spec, tables, &envelope()).unwrap().0;
        text(&written).matches("~LX*").count()
    };
    assert_eq!(count(&tables), 1);
    let split = set(&tables, "claims", "header_number", 1, Cell::Int64(2));
    assert_eq!(count(&split), 3);
}

#[test]
fn a_repeat_segment_holds_as_many_groups_as_its_elements_allow() {
    let (spec, tables) = emedny();
    let adjustments = tables.get("adjustments").unwrap();
    // Seven copies of the first adjustment in one segment's place.
    let rows = vec![0; 7];
    let seven = rebuild(&tables, "adjustments", Some(&rows), |_, _, cell| cell);
    let written = text(&write_with_findings(&spec, &seven, &envelope()).unwrap().0);
    let group = adjustments.column("group_code").unwrap().render(0).unwrap();
    let reason = adjustments
        .column("reason_code")
        .unwrap()
        .render(0)
        .unwrap();
    let amount = adjustments.column("amount").unwrap().render(0).unwrap();
    // Each group is reason, amount and an empty quantity.
    let one = format!("{reason}*{}", amount.trim_end_matches(".00"));
    let six = vec![one.clone(); 6].join("**");
    assert!(
        written.contains(&format!("~CAS*{group}*{six}~")),
        "{written}"
    );
    assert!(written.contains(&format!("~CAS*{group}*{one}~")));
}

#[test]
fn a_value_holding_a_delimiter_is_a_finding() {
    let (spec, tables) = emedny();
    let edited = set(&tables, "claims", "claim_id", 0, Cell::Binary(b"A*B"));
    assert_eq!(
        findings(write(&spec, &edited, &envelope())),
        vec![
            "table \"claims\" row 0 column \"claim_id\" writes CLP01 with \"A*B\", which holds the element separator \"*\"; a value cannot hold the interchange's delimiters"
        ]
    );
    let mut envelope = envelope();
    envelope.sender_id = "SEND~ER".into();
    assert_eq!(
        findings(write(&spec, &tables, &envelope)),
        vec![
            "envelope field \"sender_id\" writes ISA06 with \"SEND~ER        \", which holds the segment terminator \"~\"; a value cannot hold the interchange's delimiters",
            "envelope field \"application_sender\" writes GS02 with \"SEND~ER\", which holds the segment terminator \"~\"; a value cannot hold the interchange's delimiters"
        ]
    );
}

#[test]
fn references_to_no_row_and_rows_out_of_order_are_findings() {
    let (spec, tables) = emedny();
    let lost = set(&tables, "services", "claim", 0, Cell::Int64(99));
    let lost = set(&lost, "adjustments", "claim", 0, Cell::Null);
    let lost = set(&lost, "adjustments", "service", 0, Cell::Null);
    let found = findings(write(&spec, &lost, &envelope()));
    assert_eq!(
        &found[..2],
        [
            "table \"adjustments\" row 0 column \"claim\" is null, so the row has no place in the file",
            "table \"services\" row 0 column \"claim\" refers to row 99, which table \"claims\" lacks, so the row has no place in the file",
        ]
    );
    // The last service (of the last claim) listed first.
    let services = tables.get("services").unwrap().len();
    let mut order: Vec<usize> = vec![services - 1];
    order.extend(0..services - 1);
    let moved = rebuild(&tables, "services", Some(&order), |_, _, cell| cell);
    let found = findings(write(&spec, &moved, &envelope()));
    // It is written first among its claim's (the last claim), after the
    // services of the claims before, the last of them at row 6.
    assert_eq!(
        found[0],
        "table \"services\" row 0 (column \"claim\" = 2) is written after row 6; the rows of one parent must be together and in their parents' order"
    );
}

#[test]
fn two_rows_with_one_row_number_are_a_finding() {
    let (spec, tables) = emedny();
    let twice = set(&tables, "claims", "row", 1, Cell::Int64(0));
    let found = findings(write(&spec, &twice, &envelope()));
    assert_eq!(
        found[0],
        "table \"claims\" row 1 carries row number 0, which row 0 already carries; references to it are ambiguous"
    );
}

#[test]
fn a_value_no_valid_file_holds_is_a_finding() {
    let spec = Spec::builtin_835_4010();
    let tables = parse(&spec, &common::load_sample("edi835_test_davisvision.RMT"));
    let edited = set(
        &tables,
        "payments",
        "payer_technical_contact_name",
        0,
        Cell::Binary(b"DESK"),
    );
    assert_eq!(
        findings(write(&spec, &edited, &envelope())),
        vec![
            "table \"payments\" row 0 column \"payer_technical_contact_name\" holds \"DESK\", but the column reads PER01 \"BL\", which the element's code list (\"CX\") excludes, so no valid file holds it"
        ]
    );
}

#[test]
fn a_date_without_a_four_digit_year_is_not_writable() {
    let (spec, tables) = emedny();
    let edited = set(
        &tables,
        "payments",
        "payment_date",
        0,
        Cell::Date32(3_000_000),
    );
    let found = findings(write(&spec, &edited, &envelope()));
    assert_eq!(
        found[0],
        "table \"payments\" row 0 column \"payment_date\" holds 10183-09-21, which BPR16 cannot hold: a date needs a year from 1 to 9999"
    );
}

#[test]
fn strict_writing_refuses_what_allow_findings_writes() {
    let (spec, tables) = emedny();
    let off = set(
        &tables,
        "payments",
        "total_payment_amount",
        0,
        Cell::Decimal128(100),
    );
    let refused = findings(write(&spec, &off, &envelope()));
    assert_eq!(
        refused,
        vec![
            "table \"payments\" row 0 column \"total_payment_amount\": SNIP 3 · balancing rule \"transaction_balance\" fails in loop \"transaction\" opened at segment #2: BPR02 of transaction \"financial_information\" is 1.00, but sum of CLP04 of 2100 \"claim_payment_information\" - sum of PLB04, PLB06, PLB08, PLB10, PLB12, PLB14 of transaction \"provider_adjustment\" adds up to 45.75 (off by -44.75); read from segments #3, #13, #30, #40 · segment #3, element 2 · at interchange#1/group#1/transaction#1 · datum \"1\""
        ]
    );
    let (bytes, allowed) = write_with_findings(&spec, &off, &envelope()).unwrap();
    assert_eq!(
        allowed.iter().map(ToString::to_string).collect::<Vec<_>>(),
        refused
    );
    assert!(text(&bytes).contains("~BPR*I*1*C*ACH*"));
}

#[test]
fn a_required_loop_without_data_is_written_and_its_gaps_reported() {
    let (spec, tables) = emedny();
    let empty = rebuild(&tables, "payments", None, |name, _, cell| {
        if name.starts_with("payee_") {
            Cell::Null
        } else {
            cell
        }
    });
    let (bytes, found) = write_with_findings(&spec, &empty, &envelope()).unwrap();
    assert!(text(&bytes).contains("~N1*PE~"));
    let found: Vec<String> = found.iter().map(ToString::to_string).collect();
    assert!(
        found.iter().any(|f| f.starts_with(
            "table \"payments\" row 0 column \"payee_name\": SNIP 2 · required element N102"
        )),
        "{found:#?}"
    );
}

#[test]
fn tables_and_delimiters_that_do_not_fit_are_errors() {
    let (spec, tables) = emedny();
    let error = |tables: &Tables, envelope: &Envelope| {
        write(&spec, tables, envelope).unwrap_err().to_string()
    };
    let stray = Tables::new(vec![Table::new("notes", Vec::new())]);
    assert_eq!(
        error(&stray, &envelope()),
        "table \"notes\" is not a table of the spec, whose tables are \"adjustments\", \"claims\", \"payments\", \"provider_adjustments\", \"services\""
    );
    let extra = Tables::new(vec![Table::new(
        "claims",
        vec![("color".to_string(), ColumnType::Binary)],
    )]);
    assert!(
        error(&extra, &envelope())
            .starts_with("table \"claims\" has no column \"color\" in the spec; its columns are \"row\", \"segment\", \"payment\", ")
    );
    let typed = Tables::new(vec![Table::new(
        "claims",
        vec![("charge_amount".to_string(), ColumnType::Binary)],
    )]);
    assert_eq!(
        error(&typed, &envelope()),
        "table \"claims\" column \"charge_amount\" is binary; the spec makes it decimal128(38, 2)"
    );
    let mut same = envelope();
    same.delimiters = Delimiters::new(b'*', b'*', b'~').with_repetition(b'^');
    assert_eq!(
        error(&tables, &same),
        "the element separator and the component separator are both \"*\"; each delimiter needs its own byte"
    );
    let mut letter = envelope();
    letter.delimiters = Delimiters::new(b'*', b':', b'A').with_repetition(b'^');
    assert_eq!(
        error(&tables, &letter),
        "the segment terminator \"A\" is a letter, a digit or white space, which values hold"
    );
    let plan = Spec::builtin_835()
        .merge_patch(r#"{"tables":{"claims":{"columns":{"header_number":null}}}}"#)
        .unwrap();
    let error = write(&plan, &Tables::default(), &envelope()).unwrap_err();
    assert!(matches!(error, WriteError::Plan(_)));
    assert!(std::error::Error::source(&error).is_none());
}

#[test]
fn a_finding_names_where_its_value_came_from() {
    let (spec, tables) = emedny();
    let edited = set(&tables, "claims", "claim_status", 0, Cell::Binary(b"ZZ"));
    let (_, found) = write_with_findings(&spec, &edited, &envelope()).unwrap();
    let Finding::ReadBack { origin, diagnostic } = &found[0] else {
        panic!("{found:?}");
    };
    assert_eq!(
        origin.as_ref().unwrap().to_string(),
        "table \"claims\" row 0 column \"claim_status\""
    );
    assert_eq!(diagnostic.element, Some(2));
}

#[test]
fn a_null_header_number_is_reported_on_its_column() {
    let (spec, tables) = emedny();
    let null = rebuild(&tables, "claims", None, |name, _, cell| {
        if name == "header_number" {
            Cell::Null
        } else {
            cell
        }
    });
    let found = findings(write(&spec, &null, &envelope()));
    assert!(
        found[0].starts_with(
            "table \"claims\" row 0 column \"header_number\": SNIP 2 · required element LX01"
        ),
        "{found:#?}"
    );
}

#[test]
fn a_missing_occurrence_names_the_row_that_opened_its_instance() {
    let (spec, tables) = emedny();
    let nameless = rebuild(&tables, "claims", None, |name, at, cell| {
        if at == 0 && name.starts_with("patient_") {
            Cell::Null
        } else {
            cell
        }
    });
    let found = findings(write(&spec, &nameless, &envelope()));
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(
        found[0].starts_with(
            "table \"claims\" row 0: SNIP 2 · loop \"2100\" opened at segment #13 closed without its required occurrence \"patient_name\""
        ),
        "{found:#?}"
    );
}

#[test]
fn envelope_dates_and_times_drop_what_the_interchange_header_cannot_hold() {
    let (spec, tables) = emedny();
    let mut envelope = envelope();
    // 2050-06-15 at 08:05:09.
    envelope.date = 29385;
    envelope.time = 8 * 3600 + 5 * 60 + 9;
    let written = text(&write(&spec, &tables, &envelope).unwrap());
    assert!(written.contains("*500615*0805*^*"));
    assert!(written.contains("~GS*HP*SENDER*RECEIVER*20500615*080509*1*X*"));
    envelope.date = 3_000_000;
    let found = findings(write(&spec, &tables, &envelope));
    assert_eq!(
        &found[..2],
        [
            "envelope field \"date\" holds 10183-09-21, which ISA09 cannot hold: a date needs a year from 1 to 9999",
            "envelope field \"date\" holds 10183-09-21, which GS04 cannot hold: a date needs a year from 1 to 9999",
        ]
    );
}

#[test]
fn an_envelope_time_far_outside_one_day_is_a_finding() {
    let (spec, tables) = emedny();
    let mut envelope = envelope();
    envelope.time = i32::MIN;
    let found = findings(write(&spec, &tables, &envelope));
    assert!(
        found
            .iter()
            .any(|f| f.starts_with("envelope field \"time\"")
                && f.ends_with("which ISA10 cannot hold: a time must fall within one day")),
        "{found:#?}"
    );
}

#[test]
fn a_control_number_too_long_is_named_on_every_envelope_segment() {
    let (spec, tables) = emedny();
    let mut envelope = envelope();
    envelope.control_number = 1_000_000_000;
    let (_, found) = write_with_findings(&spec, &tables, &envelope).unwrap();
    assert!(!found.is_empty());
    for finding in &found {
        let Finding::ReadBack { origin, .. } = finding else {
            panic!("{finding}");
        };
        assert_eq!(
            origin.as_ref().map(ToString::to_string).as_deref(),
            Some("envelope field \"control_number\""),
            "{finding}"
        );
    }
    let closers = found
        .iter()
        .filter(|f| {
            f.to_string().contains("element IEA02") || f.to_string().contains("element GE02")
        })
        .count();
    assert_eq!(closers, 2, "{found:#?}");
}

#[test]
fn a_5010_interchange_needs_a_repetition_separator() {
    let (spec, tables) = emedny();
    let mut envelope = envelope();
    envelope.delimiters = Delimiters::new(b'*', b':', b'~');
    assert!(matches!(
        write(&spec, &tables, &envelope),
        Err(WriteError::NoRepetition)
    ));
}

#[test]
fn a_reference_that_contradicts_the_parent_row_is_a_finding() {
    let (spec, tables) = emedny();
    let edited = set(&tables, "services", "payment", 0, Cell::Int64(5));
    let found = findings(write(&spec, &edited, &envelope()));
    assert_eq!(
        found,
        vec![
            "table \"services\" row 0 column \"payment\" is 5, but the row it belongs to by column \"claim\", row 0 of table \"claims\", has \"payment\" 0"
        ]
    );
    // An adjustment of a service under claim 0 that names claim 1.
    let adjustments = tables.get("adjustments").unwrap();
    let at = (0..adjustments.len())
        .find(|&row| adjustments.column("service").unwrap().get(row) != Some(Cell::Null))
        .unwrap();
    let claim = match adjustments.column("claim").unwrap().get(at) {
        Some(Cell::Int64(claim)) => claim,
        other => panic!("{other:?}"),
    };
    let edited = set(&tables, "adjustments", "claim", at, Cell::Int64(claim + 1));
    let found = findings(write(&spec, &edited, &envelope()));
    assert!(
        found[0].starts_with(&format!(
            "table \"adjustments\" row {at} column \"claim\" is {}, but the row it belongs to by column \"service\"",
            claim + 1
        )),
        "{found:#?}"
    );
}

#[test]
fn two_payments_write_two_transactions_with_their_own_counts() {
    let (spec, tables) = emedny();
    let row_of = |at: usize| Cell::Int64(i64::try_from(at).unwrap());
    // A second payment, and the last claim (row 2) with its rows moved to it.
    let two = rebuild(&tables, "payments", Some(&[0, 0]), |name, at, cell| {
        if name == "row" { row_of(at) } else { cell }
    });
    let claim_of = |tables: &Tables, table: &str, row: usize| {
        tables.get(table).unwrap().column("claim").unwrap().get(row) == Some(Cell::Int64(2))
    };
    let moved = rebuild(&two, "claims", None, |name, at, cell| {
        if name == "payment" && at == 2 {
            Cell::Int64(1)
        } else {
            cell
        }
    });
    let moved = rebuild(&moved, "services", None, |name, at, cell| {
        if name == "payment" && claim_of(&tables, "services", at) {
            Cell::Int64(1)
        } else {
            cell
        }
    });
    let moved = rebuild(&moved, "adjustments", None, |name, at, cell| {
        if name == "payment" && claim_of(&tables, "adjustments", at) {
            Cell::Int64(1)
        } else {
            cell
        }
    });
    // Three provider adjustments of the first payment: one PLB, three groups.
    let mut plb = Table::new(
        "provider_adjustments",
        tables
            .get("provider_adjustments")
            .unwrap()
            .columns()
            .iter()
            .map(|(name, data)| (name.clone(), data.kind()))
            .collect::<Vec<_>>(),
    );
    for (at, reason) in [b"WO", b"FB", b"L6"].iter().enumerate() {
        let cells: Vec<Cell> = plb
            .columns()
            .iter()
            .map(|(name, _)| match name.as_str() {
                "row" => row_of(at),
                "segment" => Cell::Int64(1),
                "payment" => Cell::Int64(0),
                "provider_id" => Cell::Binary(b"P1"),
                "fiscal_period_date" => Cell::Date32(19_723),
                "reason_code" => Cell::Binary(&reason[..]),
                "amount" => Cell::Decimal128(100),
                _ => Cell::Null,
            })
            .collect();
        plb.push_row(&cells).unwrap();
    }
    let all: Vec<Table> = moved
        .iter()
        .map(|table| {
            if table.name() == "provider_adjustments" {
                plb.clone()
            } else {
                table.clone()
            }
        })
        .collect();
    let all = Tables::new(all);
    let mut envelope = envelope();
    envelope.line_break = true;
    let (bytes, found) = write_with_findings(&spec, &all, &envelope).unwrap();
    // Only the amounts no longer balance.
    assert!(
        found
            .iter()
            .all(|f| f.to_string().contains("balancing rule")),
        "{found:#?}"
    );
    let written = text(&bytes);
    let lines: Vec<&str> = written.lines().collect();
    let at = |prefix: &str| -> Vec<usize> {
        lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.starts_with(prefix))
            .map(|(i, _)| i)
            .collect()
    };
    let (starts, ends) = (at("ST*"), at("SE*"));
    assert_eq!(lines[starts[0]], "ST*835*0001~");
    assert_eq!(lines[starts[1]], "ST*835*0002~");
    assert_eq!(
        lines[ends[0]],
        format!("SE*{}*0001~", ends[0] - starts[0] + 1)
    );
    assert_eq!(
        lines[ends[1]],
        format!("SE*{}*0002~", ends[1] - starts[1] + 1)
    );
    assert_eq!(lines[lines.len() - 2], "GE*2*1~");
    assert_eq!(lines[lines.len() - 1], "IEA*1*000000001~");
    assert_eq!(at("PLB*"), vec![ends[0] - 1]);
    assert_eq!(lines[ends[0] - 1], "PLB*P1*20240101*WO*1*FB*1*L6*1~");
    assert_eq!(at("CLP*").iter().filter(|&&i| i > starts[1]).count(), 1);
}
