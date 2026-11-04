use super::tests::interchange;
use super::*;
use crate::delimiters::test_support::plain as delimiters;
use crate::{Cell, DateMode, Mask, Phi, Rule, Tables, parse_dt};

const KEY: &[u8] = b"oxedi-test-key-0";

/// Payer, payee and the header of one claim group, with `paid` as BPR02.
fn header(paid: &str) -> String {
    format!(
        "BPR*I*{paid}*C*CHK************20240101~TRN*1*1*1234567890~\
         N1*PR*PAYER~N3*1 MAIN ST~N4*CITY*NC*27601~PER*BL*DESK*TE*5555555555~\
         N1*PE*PAYEE*XX*1234567893~LX*1~"
    )
}

/// Two claims: the first with every patient field, the second with an empty
/// first name and no patient id.
fn two_claims() -> String {
    format!(
        "{}CLP*C1*1*10*10**MC*X1~NM1*QC*1*DOE*JANE****MI*M1~DTM*232*20240105~\
         SVC*HC:99213*10*10~DTM*472*20240110~\
         CLP*C2*1*10*10**MC*X2~NM1*QC*1*ROE*~DTM*232*20240305~\
         SVC*HC:99213*10*10~DTM*472*20240310~",
        header("20")
    )
}

const LONG_ID: &str = "CLAIMNUMBERTHATISFARTOOLONGFORITSELEMENT";

/// A claim id over its maximum length, a date that is not one, a six-digit
/// date and a claim that does not balance.
fn bad_claims() -> String {
    format!(
        "{}CLP*{LONG_ID}*1*10*8**MC*X1~NM1*QC*1*DOE*JANE****MI*M1~\
         SVC*HC:99213*10*10~DTM*472*2024AB10~\
         CLP*C2*1*10*10**MC*X2~NM1*QC*1*ROE*ANN****MI*M2~\
         SVC*HC:99213*10*10~DTM*472*240310~",
        header("18")
    )
}

fn run(input: &str, mask: Option<&Mask>) -> (Tables, Vec<Diagnostic>) {
    let spec = Spec::builtin_835();
    let text = interchange(&[input]);
    let document = Document::with_delimiters(text.as_bytes(), delimiters()).unwrap();
    Processor::run(&spec, &document, mask)
}

fn cell<'t>(tables: &'t Tables, table: &str, column: &str, row: usize) -> Cell<'t> {
    tables
        .get(table)
        .unwrap()
        .column(column)
        .unwrap()
        .get(row)
        .unwrap()
}

fn day(text: &str) -> i32 {
    parse_dt(text.as_bytes()).unwrap()
}

fn mask(dates: DateMode) -> Mask {
    Mask::new(KEY, dates).unwrap()
}

#[test]
fn patient_text_is_replaced_and_empty_or_absent_values_stay_as_they_are() {
    let mask = mask(DateMode::Year);
    let (plain, _) = run(&two_claims(), None);
    let (masked, _) = run(&two_claims(), Some(&mask));
    let claims = |column, row| cell(&masked, "claims", column, row);
    assert_eq!(
        claims("patient_id", 0),
        Cell::Binary(&mask.text(Phi::Id, b"M1"))
    );
    assert_eq!(claims("patient_id", 1), Cell::Null, "absent stays null");
    assert_eq!(
        claims("patient_first_name", 0),
        Cell::Binary(&mask.text(Phi::Name, b"JANE"))
    );
    assert_eq!(
        cell(&plain, "claims", "patient_first_name", 1),
        Cell::Binary(b"")
    );
    assert_eq!(
        claims("patient_first_name", 1),
        Cell::Binary(b""),
        "written-empty stays written-empty"
    );
    assert_eq!(
        claims("patient_last_name", 1),
        Cell::Binary(&mask.text(Phi::Name, b"ROE"))
    );
    assert_eq!(
        claims("claim_id", 0),
        Cell::Binary(&mask.text(Phi::Id, b"C1"))
    );
    assert_eq!(
        claims("payer_control_number", 1),
        Cell::Binary(&mask.text(Phi::Id, b"X2"))
    );
    for column in [
        "charge_amount",
        "claim_status",
        "payment_amount",
        "patient_id_qualifier",
    ] {
        for row in 0..2 {
            assert_eq!(
                claims(column, row),
                cell(&plain, "claims", column, row),
                "{column} {row}"
            );
        }
    }
    let rows = |tables: &Tables| -> Vec<(String, usize)> {
        tables
            .iter()
            .map(|table| (table.name().to_string(), table.len()))
            .collect()
    };
    assert_eq!(rows(&masked), rows(&plain));
}

#[test]
fn year_mode_moves_every_patient_date_to_january_first() {
    let (masked, _) = run(&two_claims(), Some(&mask(DateMode::Year)));
    let first = Cell::Date32(day("20240101"));
    for row in 0..2 {
        assert_eq!(cell(&masked, "claims", "statement_from", row), first);
        assert_eq!(cell(&masked, "services", "service_date", row), first);
    }
}

#[test]
fn keep_mode_leaves_dates_and_still_masks_text() {
    let mask = mask(DateMode::Keep);
    let (plain, _) = run(&two_claims(), None);
    let (masked, _) = run(&two_claims(), Some(&mask));
    for row in 0..2 {
        assert_eq!(
            cell(&masked, "services", "service_date", row),
            cell(&plain, "services", "service_date", row)
        );
    }
    assert_eq!(
        cell(&masked, "claims", "patient_id", 0),
        Cell::Binary(&mask.text(Phi::Id, b"M1"))
    );
}

#[test]
fn shift_mode_moves_a_claim_and_its_services_by_the_same_days() {
    let mask = mask(DateMode::Shift);
    let shift = mask.shift_days(b"M1");
    let (masked, _) = run(&two_claims(), Some(&mask));
    assert_eq!(
        cell(&masked, "claims", "statement_from", 0),
        Cell::Date32(day("20240105") + shift)
    );
    assert_eq!(
        cell(&masked, "services", "service_date", 0),
        Cell::Date32(day("20240110") + shift),
        "the gap between the claim and its service is kept"
    );
    let first = Cell::Date32(day("20240101"));
    assert_eq!(
        cell(&masked, "claims", "statement_from", 1),
        first,
        "a claim with no patient id falls back to the year"
    );
    assert_eq!(
        cell(&masked, "services", "service_date", 1),
        first,
        "and so do its services"
    );
}

#[test]
fn a_masked_run_of_some_tables_gives_the_rows_of_a_full_masked_run() {
    let spec = Spec::builtin_835();
    let mask = mask(DateMode::Shift);
    let text = interchange(&[&two_claims()]);
    let document = Document::with_delimiters(text.as_bytes(), delimiters()).unwrap();
    let (whole, _) = Processor::run(&spec, &document, Some(&mask));
    let some = Processor::run_tables(&spec, &document, &["services"], Some(&mask)).unwrap();
    assert_eq!(
        some.get("services"),
        whole.get("services"),
        "services shift by their claim's patient id although claims is not kept"
    );
    let mut processor = Processor::with_tables(&spec, &delimiters(), &["claims"])
        .unwrap()
        .with_mask(Some(&mask));
    let mut segment = Segment::empty();
    let mut index = 0;
    while document.segment_into(index, &mut segment) {
        processor.feed(&segment);
        index += 1;
    }
    processor.finish();
    assert_eq!(processor.take_tables().get("claims"), whole.get("claims"));
}

#[test]
fn diagnostic_datums_read_by_a_patient_column_are_masked() {
    let mask = mask(DateMode::Year);
    let (_, plain) = run(&bad_claims(), None);
    let (_, masked) = run(&bad_claims(), Some(&mask));
    assert_eq!(masked.len(), plain.len());
    let find = |diagnostics: &[Diagnostic], kind: &str, id: &[u8]| -> Vec<Vec<u8>> {
        diagnostics
            .iter()
            .filter(|d| d.rule.kind() == kind && d.rule.segment_id() == Some(id))
            .map(|d| d.datum.clone())
            .collect()
    };
    assert_eq!(
        find(&plain, "TypeMismatch", b"DTM"),
        vec![b"2024AB10".to_vec()]
    );
    assert_eq!(
        find(&masked, "TypeMismatch", b"DTM"),
        vec![mask.text(Phi::Date, b"2024AB10").to_vec()],
        "a value that is no date is masked as text"
    );
    assert_eq!(
        find(&plain, "LengthOutOfRange", b"DTM"),
        vec![b"240310".to_vec()]
    );
    assert_eq!(
        find(&masked, "LengthOutOfRange", b"DTM"),
        vec![b"20240101".to_vec()],
        "a date is masked as a date, written CCYYMMDD"
    );
    assert_eq!(
        find(&masked, "LengthOutOfRange", b"CLP"),
        vec![mask.text(Phi::Id, LONG_ID.as_bytes()).to_vec()]
    );
    let balance: Vec<&Diagnostic> = masked
        .iter()
        .filter(|d| matches!(d.rule, Rule::BalanceMismatch { .. }))
        .collect();
    assert_eq!(balance.len(), 1);
    assert_eq!(balance[0].datum, b"10", "an amount is not patient data");
    for (masked, plain) in masked.iter().zip(&plain) {
        assert_eq!(
            (
                &masked.rule,
                masked.segment,
                masked.element,
                masked.component,
                &masked.path
            ),
            (
                &plain.rule,
                plain.segment,
                plain.element,
                plain.component,
                &plain.path
            ),
            "only the datum changes"
        );
        let text = masked.to_string();
        for original in ["2024AB10", "240310", LONG_ID] {
            assert!(!text.contains(original), "{original} leaks in {text}");
        }
    }
}

#[test]
fn keep_mode_leaves_date_datums_as_written() {
    let (_, masked) = run(&bad_claims(), Some(&mask(DateMode::Keep)));
    let datums: Vec<&[u8]> = masked
        .iter()
        .filter(|d| d.rule.segment_id() == Some(b"DTM".as_slice()))
        .map(|d| d.datum.as_slice())
        .collect();
    assert_eq!(datums, vec![b"2024AB10".as_slice(), b"240310".as_slice()]);
}

/// `Processor::run` without a mask, checked against feeding the processor
/// by hand; the tables it fills.
fn plain_run_matches_feeding(spec: &Spec, document: &Document) -> Tables {
    let (tables, diagnostics) = Processor::run(spec, document, None);
    let mut processor = Processor::new(spec, document.delimiters());
    let mut by_hand = Vec::new();
    let mut segment = Segment::empty();
    let mut index = 0;
    while document.segment_into(index, &mut segment) {
        by_hand.extend_from_slice(processor.feed(&segment).diagnostics());
        index += 1;
    }
    by_hand.extend_from_slice(processor.finish().diagnostics());
    assert_eq!(by_hand, diagnostics);
    assert_eq!(processor.take_tables(), tables);
    tables
}

#[test]
fn without_a_mask_nothing_changes() {
    let spec = Spec::builtin_835();
    let text = interchange(&[&bad_claims()]);
    let document = Document::with_delimiters(text.as_bytes(), delimiters()).unwrap();
    let tables = plain_run_matches_feeding(&spec, &document);
    assert_eq!(
        cell(&tables, "claims", "claim_id", 1),
        Cell::Binary(b"C2"),
        "the plain run keeps the values"
    );
    let bytes = include_bytes!("../../tests/fixtures/balanced_5010_sample.txt");
    plain_run_matches_feeding(&spec, &Document::parse(&bytes[..]).unwrap());
}
