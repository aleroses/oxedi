use super::*;
use crate::delimiters::test_support::plain as delimiters;
use crate::{Cell, SnipLevel, Tokenizer};

const ISA: &str = "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1200*^*00501*000000001*0*P*>~";

/// A complete interchange holding the given transactions, each written
/// between its `ST` and its `SE`.
pub(super) fn interchange(transactions: &[&str]) -> String {
    let mut text = format!("{ISA}GS*HP*SENDER*RECEIVER*20240101*1200*7*X*005010X221A1~");
    for (i, body) in transactions.iter().enumerate() {
        let count = body.matches('~').count() + 2;
        text.push_str(&format!(
            "ST*835*{:04}~{body}SE*{count}*{:04}~",
            i + 1,
            i + 1
        ));
    }
    text.push_str(&format!("GE*{}*7~IEA*1*000000001~", transactions.len()));
    text
}

/// A transaction body with every required occurrence and loop.
const CLAIM: &str = "BPR*I*10*C*CHK************20240101~TRN*1*1*1234567890~\
    N1*PR*PAYER~N3*1 MAIN ST~N4*CITY*NC*27601~PER*BL*DESK*TE*5555555555~\
    N1*PE*PAYEE*XX*1234567893~\
    LX*1~CLP*C1*1*10*10**MC*X1~NM1*QC*1*DOE*JANE****MI*M1~SVC*HC:99213*10*10~";

#[test]
fn one_feed_returns_the_events_and_every_diagnostic_of_its_segment() {
    let spec = Spec::builtin_835();
    let mut processor = Processor::new(&spec, &delimiters());
    let input = "ST*835*0001~CLP*C1*1*12A*0**MC*X1~";
    let segments: Vec<_> = Tokenizer::with_delimiters(input.as_bytes(), delimiters()).collect();
    processor.feed(&segments[0]);
    let output = processor.feed(&segments[1]);
    let names: Vec<String> = output
        .events()
        .iter()
        .map(|event| match *event {
            Event::LoopOpened { id, implicit, .. } => {
                format!(
                    "open{} {}",
                    if implicit { "!" } else { "" },
                    spec.loop_name(id)
                )
            }
            Event::Captured { id, segment } => format!("cap {} #{segment}", spec.loop_name(id)),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(names, vec!["open! 2000", "open 2100", "cap 2100 #1"]);
    let levels: Vec<SnipLevel> = output.diagnostics().iter().map(|d| d.level).collect();
    assert_eq!(levels, vec![SnipLevel::L1, SnipLevel::L2]);
    assert_eq!(
        output.diagnostics()[1].to_string(),
        "SNIP 2 · element CLP03 (total_claim_charge_amount) is not a valid R (decimal, scale 2) · segment #1, element 3 · at interchange#1/group#1/transaction#1/2000#1/2100#1 · datum \"12A\""
    );
    let latest = output.diagnostics().to_vec();
    assert_eq!(processor.diagnostics(), latest);
}

#[test]
fn finishing_reports_what_is_left_open_and_appends_its_rows() {
    let spec = Spec::builtin_835();
    let mut processor = Processor::new(&spec, &delimiters());
    let input = format!("{ISA}GS*HP*S*R*20240101*1200*7*X*005010X221A1~ST*835*0001~{CLAIM}");
    for segment in Tokenizer::with_delimiters(input.as_bytes(), delimiters()) {
        processor.feed(&segment);
    }
    let output = processor.finish();
    assert_eq!(output.events().len(), 6, "six loops close");
    assert_eq!(
        output.diagnostics().len(),
        3,
        "three envelopes lack their end"
    );
    let tables = processor.take_tables();
    for name in ["payments", "claims", "services"] {
        assert_eq!(tables.get(name).unwrap().len(), 1, "{name}");
    }
}

#[test]
fn run_over_a_document_is_feeding_every_segment_then_finishing() {
    let spec = Spec::builtin_835();
    let input = interchange(&[
        CLAIM,
        "BPR*I*0*C*CHK************20240101~TRN*1*2*1234567890~ZZZ~",
    ]);
    let document = Document::with_delimiters(input.as_bytes(), delimiters()).unwrap();
    let (tables, diagnostics) = Processor::run(&spec, &document, None);
    let mut processor = Processor::new(&spec, &delimiters());
    let mut by_hand = Vec::new();
    for segment in Tokenizer::with_delimiters(input.as_bytes(), delimiters()) {
        by_hand.extend_from_slice(processor.feed(&segment).diagnostics());
    }
    by_hand.extend_from_slice(processor.finish().diagnostics());
    assert_eq!(diagnostics, by_hand);
    assert_eq!(tables, processor.take_tables());
    let kinds: Vec<&str> = diagnostics.iter().map(|d| d.rule.kind()).collect();
    assert_eq!(
        kinds,
        vec![
            "UnknownSegment",
            "RequiredLoopMissing",
            "RequiredLoopMissing"
        ],
        "the unknown ZZZ, then the payer and payee loops the second transaction lacks"
    );
    assert_eq!(tables.get("payments").unwrap().len(), 2);
}

#[test]
fn tables_drained_after_each_transaction_add_up_to_one_run() {
    let spec = Spec::builtin_835();
    let input = interchange(&[CLAIM, CLAIM, CLAIM]);
    let transaction = spec.loop_id("transaction").unwrap();
    let mut processor = Processor::new(&spec, &delimiters());
    let mut drained = Vec::new();
    for segment in Tokenizer::with_delimiters(input.as_bytes(), delimiters()) {
        let closes = processor
            .feed(&segment)
            .events()
            .contains(&Event::LoopClosed { id: transaction });
        if closes {
            drained.push(processor.take_tables());
        }
    }
    processor.finish();
    drained.push(processor.take_tables());
    let document = Document::with_delimiters(input.as_bytes(), delimiters()).unwrap();
    let (whole, _) = Processor::run(&spec, &document, None);
    for table in &whole {
        let rows = |tables: &Tables| -> Vec<String> {
            let part = tables.get(table.name()).unwrap();
            (0..part.len())
                .map(|row| {
                    part.columns()
                        .iter()
                        .map(|(_, column)| column.render(row).unwrap())
                        .collect::<Vec<_>>()
                        .join("|")
                })
                .collect()
        };
        let pieces: Vec<String> = drained.iter().flat_map(rows).collect();
        assert_eq!(pieces, rows(&whole), "{}", table.name());
    }
    assert_eq!(
        drained
            .iter()
            .map(|t| t.get("claims").unwrap().len())
            .collect::<Vec<_>>(),
        vec![1, 1, 1, 0]
    );
}

#[test]
fn a_processor_with_some_tables_fills_those_with_the_rows_of_a_full_run() {
    let spec = Spec::builtin_835();
    let input = interchange(&[
        CLAIM,
        "BPR*I*0*C*CHK************20240101~TRN*1*2*1234567890~ZZZ~",
        CLAIM,
    ]);
    let document = Document::with_delimiters(input.as_bytes(), delimiters()).unwrap();
    let (whole, diagnostics) = Processor::run(&spec, &document, None);

    let mut processor = Processor::with_tables(&spec, &delimiters(), &["services"]).unwrap();
    let mut by_hand = Vec::new();
    for segment in Tokenizer::with_delimiters(input.as_bytes(), delimiters()) {
        by_hand.extend_from_slice(processor.feed(&segment).diagnostics());
    }
    by_hand.extend_from_slice(processor.finish().diagnostics());
    assert_eq!(by_hand, diagnostics, "every element is still checked");
    let some = processor.take_tables();
    let names: Vec<&str> = some.iter().map(|table| table.name()).collect();
    assert_eq!(names, vec!["services"], "only the named table is kept");
    assert_eq!(some.get("services"), whole.get("services"));
    let services = some.get("services").unwrap();
    assert_eq!(services.len(), 2);
    let claim = |row| services.column("claim").and_then(|c| c.get(row));
    assert_eq!(
        (claim(0), claim(1)),
        (Some(Cell::Int64(0)), Some(Cell::Int64(1))),
        "a service refers to its claim's row although claims is not filled"
    );

    let only = Processor::run_tables(&spec, &document, &["claims", "payments"], None).unwrap();
    assert_eq!(only.len(), 2);
    for name in ["claims", "payments"] {
        assert_eq!(only.get(name), whole.get(name), "{name}");
    }
    assert!(
        Processor::run_tables(&spec, &document, &[], None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn an_unknown_table_name_is_an_error_naming_it_and_the_spec_tables() {
    let spec = Spec::builtin_835();
    let input = interchange(&[CLAIM]);
    let document = Document::with_delimiters(input.as_bytes(), delimiters()).unwrap();
    let error = Processor::run_tables(&spec, &document, &["claims", "claim"], None).unwrap_err();
    assert_eq!(error.name, "claim");
    assert_eq!(
        error.tables,
        spec.tables()
            .iter()
            .map(|def| def.name.clone())
            .collect::<Vec<_>>()
    );
    assert!(Processor::with_tables(&spec, &delimiters(), &["claim"]).is_err());
    assert!(Projector::with_tables(&spec, &delimiters(), &["claim"]).is_err());
}
