//! The summaries of the built-in 835 over real files, and the text of every
//! reason a summary cannot be computed.

use super::*;
use crate::{Cell, ColumnType, Document, Processor, Spec, Table, Tables};

const BALANCED: &[u8] = include_bytes!("../../tests/fixtures/balanced_5010_sample.txt");

fn tables_of(spec: &Spec, input: &[u8]) -> Tables {
    let document = Document::parse(input).unwrap();
    Processor::run(spec, &document, None).0
}

fn text(value: &str) -> String {
    value.to_string()
}

#[test]
fn the_builtin_summaries_of_a_file() {
    let spec = Spec::builtin_835();
    let tables = tables_of(&spec, BALANCED);
    let summary = |name: &str| summarize(&spec, name, &tables).unwrap();
    assert_eq!(summary("count_claims"), SummaryValue::Count(4));
    assert_eq!(summary("count_patients"), SummaryValue::Count(4));
    assert_eq!(
        summary("sum_payments"),
        SummaryValue::Decimal {
            value: 69_000,
            scale: 2
        }
    );
    let fields = |values: [Option<&str>; 6]| {
        let names = [
            "name",
            "identification_code",
            "address",
            "city",
            "state",
            "zip_code",
        ];
        SummaryValue::Row(Some(
            names
                .into_iter()
                .zip(values)
                .map(|(name, value)| (text(name), value.map(text)))
                .collect(),
        ))
    };
    assert_eq!(
        summary("payer"),
        fields([
            Some("ACME HEALTH PLAN"),
            None,
            Some("100 MAIN ST"),
            Some("SPRINGFIELD"),
            Some("IL"),
            Some("62701"),
        ])
    );
    assert_eq!(
        summary("payee"),
        fields([
            Some("SUNRISE CLINIC"),
            Some("1234567893"),
            Some("200 OAK AVE"),
            Some("SPRINGFIELD"),
            Some("IL"),
            Some("62702"),
        ])
    );
}

/// One transaction whose claims name the patients given, in order.
fn claims_of(patients: &[&str]) -> Vec<u8> {
    let mut segments = vec![
        text(
            "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       \
             *240101*1200*^*00501*000000001*0*P*:",
        ),
        text("GS*HP*S*R*20240101*1200*1*X*005010X221A1"),
        text("ST*835*0001"),
        text("BPR*I*100*C*CHK************20240102"),
        text("TRN*1*12345*1512345678"),
        text("N1*PR*PAYER"),
        text("N1*PE*CLINIC*XX*1234567890"),
        text("LX*1"),
    ];
    for (number, patient) in patients.iter().enumerate() {
        segments.push(format!("CLP*C{number}*1*100*80**MC*ICN{number}"));
        segments.push(format!("NM1*QC*1*DOE*JANE****MI*{patient}"));
    }
    segments.push(format!("SE*{}*0001", segments.len() - 1));
    segments.push(text("GE*1*1"));
    segments.push(text("IEA*1*000000001"));
    let mut bytes = segments.join("~").into_bytes();
    bytes.push(b'~');
    bytes
}

#[test]
fn distinct_values_are_compared_as_text() {
    let spec = Spec::builtin_835();
    let tables = tables_of(&spec, &claims_of(&["0123", "123", "0123"]));
    let summary = |name: &str| summarize(&spec, name, &tables).unwrap();
    assert_eq!(summary("count_claims"), SummaryValue::Count(3));
    assert_eq!(summary("count_patients"), SummaryValue::Count(2));
}

#[test]
fn a_row_summary_reads_the_one_row() {
    let spec = Spec::builtin_835();
    let tables = tables_of(&spec, &[BALANCED, BALANCED].concat());
    let err = summarize(&spec, "payer", &tables).unwrap_err();
    assert_eq!(
        err,
        SummaryError::NotOneRow {
            summary: text("payer"),
            table: text("payments"),
            rows: 2,
        }
    );
}

#[test]
fn a_row_whose_every_field_is_null_is_absent() {
    let spec = Spec::builtin_835();
    let without_payee = String::from_utf8_lossy(BALANCED).replace("N1*PE*", "N1*XX*");
    let tables = tables_of(&spec, without_payee.as_bytes());
    assert_eq!(
        summarize(&spec, "payee", &tables).unwrap(),
        SummaryValue::Row(None)
    );
}

#[test]
fn tables_without_the_table_give_an_error() {
    let spec = Spec::builtin_835();
    let empty = Tables::new(Vec::new());
    for (name, table) in [("count_claims", "claims"), ("payer", "payments")] {
        assert_eq!(
            summarize(&spec, name, &empty).unwrap_err(),
            SummaryError::MissingTable {
                summary: text(name),
                table: text(table),
                present: Vec::new(),
            }
        );
    }
    let services = Tables::new(vec![Table::new("services", Vec::new())]);
    assert_eq!(
        summarize(&spec, "count_claims", &services).unwrap_err(),
        SummaryError::MissingTable {
            summary: text("count_claims"),
            table: text("claims"),
            present: vec![text("services")],
        }
    );
}

#[test]
fn tables_without_the_column_give_an_error() {
    let spec = Spec::builtin_835();
    let claims = Tables::new(vec![Table::new("claims", Vec::new())]);
    assert_eq!(
        summarize(&spec, "count_patients", &claims).unwrap_err(),
        SummaryError::MissingColumn {
            summary: text("count_patients"),
            table: text("claims"),
            column: text("patient_id"),
        }
    );
}

#[test]
fn a_sum_over_a_column_that_holds_no_decimal_is_refused() {
    let spec = Spec::builtin_835();
    let payments = Tables::new(vec![Table::new(
        "payments",
        [(text("total_payment_amount"), ColumnType::Binary)],
    )]);
    assert_eq!(
        summarize(&spec, "sum_payments", &payments).unwrap_err(),
        SummaryError::NotDecimal {
            summary: text("sum_payments"),
            table: text("payments"),
            column: text("total_payment_amount"),
            found: ColumnType::Binary,
        }
    );
}

#[test]
fn a_sum_that_overflows_names_the_row_and_the_amount() {
    let spec = Spec::builtin_835();
    let mut payments = Table::new(
        "payments",
        [(
            text("total_payment_amount"),
            ColumnType::Decimal128 {
                precision: 38,
                scale: 2,
            },
        )],
    );
    for cell in [Cell::Null, Cell::Decimal128(i128::MAX), Cell::Decimal128(1)] {
        payments.push_row(&[cell]).unwrap();
    }
    let err = summarize(&spec, "sum_payments", &Tables::new(vec![payments])).unwrap_err();
    assert_eq!(
        err,
        SummaryError::Overflow {
            summary: text("sum_payments"),
            table: text("payments"),
            column: text("total_payment_amount"),
            row: 2,
            amount: text("0.01"),
        }
    );
}

#[test]
fn an_undeclared_summary_lists_the_declared_ones() {
    let spec = Spec::builtin_835();
    let tables = tables_of(&spec, BALANCED);
    assert_eq!(
        summarize(&spec, "total", &tables).unwrap_err(),
        SummaryError::Undeclared {
            summary: text("total"),
            declared: [
                "count_claims",
                "count_patients",
                "payee",
                "payer",
                "sum_payments"
            ]
            .map(text)
            .to_vec(),
        }
    );
}

#[test]
fn every_summary_error_displays_in_full() {
    let cases = [
        (
            SummaryError::Undeclared {
                summary: text("sum_payments"),
                declared: vec![text("count_claims"), text("count_patients")],
            },
            "the spec declares no summary \"sum_payments\"; it declares: count_claims, \
             count_patients",
        ),
        (
            SummaryError::Undeclared {
                summary: text("payer"),
                declared: Vec::new(),
            },
            "the spec declares no summary \"payer\"; it declares none",
        ),
        (
            SummaryError::MissingTable {
                summary: text("count_claims"),
                table: text("claims"),
                present: vec![text("payments"), text("services")],
            },
            "summary \"count_claims\" reads the table \"claims\", which the tables do not hold; \
             they hold: payments, services",
        ),
        (
            SummaryError::MissingTable {
                summary: text("payer"),
                table: text("payments"),
                present: Vec::new(),
            },
            "summary \"payer\" reads the table \"payments\", which the tables do not hold; they \
             hold none",
        ),
        (
            SummaryError::MissingColumn {
                summary: text("count_patients"),
                table: text("claims"),
                column: text("patient_id"),
            },
            "summary \"count_patients\" reads the column \"patient_id\" of the table \"claims\", \
             which the table does not hold",
        ),
        (
            SummaryError::NotDecimal {
                summary: text("sum_payments"),
                table: text("payments"),
                column: text("total_payment_amount"),
                found: ColumnType::Binary,
            },
            "summary \"sum_payments\" sums the column \"total_payment_amount\" of the table \
             \"payments\", which holds binary, not a decimal",
        ),
        (
            SummaryError::NotOneRow {
                summary: text("payer"),
                table: text("payments"),
                rows: 2,
            },
            "summary \"payer\" reads the one row of the table \"payments\", which has 2 rows",
        ),
        (
            SummaryError::Overflow {
                summary: text("sum_payments"),
                table: text("payments"),
                column: text("total_payment_amount"),
                row: 1,
                amount: text("99.00"),
            },
            "summary \"sum_payments\": the sum of \"total_payment_amount\" in the table \
             \"payments\" overflows a 128-bit decimal at row 1 (amount 99.00)",
        ),
    ];
    for (err, expected) in cases {
        assert_eq!(err.to_string(), expected);
        assert!(std::error::Error::source(&err).is_none(), "{err}");
    }
}
