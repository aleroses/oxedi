//! The full text of every finding, origin and write error.

use crate::delimiters::IsaError;
use crate::diagnostic::{Diagnostic, Rule};
use crate::document::DocumentError;
use crate::value::{ColumnType, ElementTextError};
use crate::write::*;

fn cell() -> ValueOrigin {
    ValueOrigin::Cell {
        table: "claims".into(),
        row: 3,
        column: "claim_id".into(),
    }
}

#[test]
fn origins_name_the_cell_the_row_or_the_envelope_field() {
    assert_eq!(
        cell().to_string(),
        "table \"claims\" row 3 column \"claim_id\""
    );
    let row = ValueOrigin::Row {
        table: "claims".into(),
        row: 3,
    };
    assert_eq!(row.to_string(), "table \"claims\" row 3");
    let field = ValueOrigin::Envelope {
        field: "sender_id".into(),
    };
    assert_eq!(field.to_string(), "envelope field \"sender_id\"");
}

#[test]
fn findings_display_their_full_text() {
    let cases = [
        (
            Finding::DelimiterInValue {
                origin: cell(),
                place: "CLP01".into(),
                value: b"A:B".to_vec(),
                delimiter: b':',
                role: "component separator",
            },
            "table \"claims\" row 3 column \"claim_id\" writes CLP01 with \"A:B\", which holds the component separator \":\"; a value cannot hold the interchange's delimiters",
        ),
        (
            Finding::UnwrittenValue {
                origin: cell(),
                value: "\"X\"".into(),
                place: "PER01".into(),
                code: "BL".into(),
                codes: vec!["CX".into(), "IC".into()],
            },
            "table \"claims\" row 3 column \"claim_id\" holds \"X\", but the column reads PER01 \"BL\", which the element's code list (\"CX\", \"IC\") excludes, so no valid file holds it",
        ),
        (
            Finding::MissingParent {
                table: "services".into(),
                row: 2,
                column: "claim".into(),
                value: Some(7),
                parent: "claims".into(),
            },
            "table \"services\" row 2 column \"claim\" refers to row 7, which table \"claims\" lacks, so the row has no place in the file",
        ),
        (
            Finding::MissingParent {
                table: "services".into(),
                row: 2,
                column: "claim".into(),
                value: None,
                parent: "claims".into(),
            },
            "table \"services\" row 2 column \"claim\" is null, so the row has no place in the file",
        ),
        (
            Finding::MismatchedReference {
                table: "services".into(),
                row: 4,
                column: "payment".into(),
                value: 1,
                through: "claim".into(),
                parent: "claims".into(),
                parent_row: 2,
                expected: 0,
            },
            "table \"services\" row 4 column \"payment\" is 1, but the row it belongs to by column \"claim\", row 2 of table \"claims\", has \"payment\" 0",
        ),
        (
            Finding::DuplicateRowNumber {
                table: "claims".into(),
                row: 4,
                value: 1,
                first: 1,
            },
            "table \"claims\" row 4 carries row number 1, which row 1 already carries; references to it are ambiguous",
        ),
        (
            Finding::OutOfOrder {
                table: "services".into(),
                row: 0,
                column: "claim".into(),
                value: 2,
                previous: 6,
            },
            "table \"services\" row 0 (column \"claim\" = 2) is written after row 6; the rows of one parent must be together and in their parents' order",
        ),
        (
            Finding::NotWritable {
                origin: cell(),
                place: "DTM02".into(),
                value: "date32(2147483647)".into(),
                reason: ElementTextError::YearOutOfRange,
            },
            "table \"claims\" row 3 column \"claim_id\" holds date32(2147483647), which DTM02 cannot hold: a date needs a year from 1 to 9999",
        ),
        (
            Finding::ReadBack {
                origin: Some(cell()),
                diagnostic: Diagnostic::new(
                    Rule::UnknownSegment {
                        segment_id: b"ZZ".to_vec(),
                    },
                    Some(4),
                    None,
                    None,
                    Vec::new(),
                    b"ZZ".to_vec(),
                ),
            },
            "table \"claims\" row 3 column \"claim_id\": SNIP 1 · segment \"ZZ\" is not part of the structure: no open loop holds it and it opens no loop · segment #4 · at the root · datum \"ZZ\"",
        ),
        (
            Finding::ReadBack {
                origin: None,
                diagnostic: Diagnostic::new(
                    Rule::UnknownSegment {
                        segment_id: b"ZZ".to_vec(),
                    },
                    None,
                    None,
                    None,
                    Vec::new(),
                    Vec::new(),
                ),
            },
            "the written file: SNIP 1 · segment \"ZZ\" is not part of the structure: no open loop holds it and it opens no loop · end of stream · at the root · datum \"\"",
        ),
    ];
    for (finding, text) in cases {
        assert_eq!(finding.to_string(), text);
    }
}

#[test]
fn write_errors_display_their_full_text() {
    let plan = PlanError {
        spec: "s".into(),
        refusals: vec![Refusal::SeveralAnchorLoops {
            table: "names".into(),
            loops: vec!["1000A".into(), "1000B".into()],
        }],
    };
    let cases = [
        (
            WriteError::Plan(plan),
            "spec \"s\" cannot be written (1 reason)\n1. table \"names\" is anchored on 2 loops (\"1000A\", \"1000B\"); a written row opens one loop instance and nothing in the row says which",
        ),
        (
            WriteError::UnknownTable {
                table: "notes".into(),
                tables: vec!["claims".into(), "payments".into()],
            },
            "table \"notes\" is not a table of the spec, whose tables are \"claims\", \"payments\"",
        ),
        (
            WriteError::UnknownColumn {
                table: "claims".into(),
                column: "color".into(),
                columns: vec!["row".into(), "segment".into()],
            },
            "table \"claims\" has no column \"color\" in the spec; its columns are \"row\", \"segment\"",
        ),
        (
            WriteError::ColumnType {
                table: "claims".into(),
                column: "charge_amount".into(),
                expected: ColumnType::Decimal128 {
                    precision: 38,
                    scale: 2,
                },
                found: ColumnType::Int64 { scale: 0 },
            },
            "table \"claims\" column \"charge_amount\" is int64; the spec makes it decimal128(38, 2)",
        ),
        (
            WriteError::SameDelimiter {
                first: "element separator",
                second: "repetition separator",
                byte: b'^',
            },
            "the element separator and the repetition separator are both \"^\"; each delimiter needs its own byte",
        ),
        (
            WriteError::DelimiterNotAllowed {
                role: "component separator",
                byte: b' ',
            },
            "the component separator \" \" is a letter, a digit or white space, which values hold",
        ),
        (
            WriteError::NoRepetition,
            "the interchange header carries a repetition separator, but the envelope field \"delimiters.repetition\" is not set",
        ),
        (
            WriteError::Unreadable(DocumentError::Isa(IsaError::NotIsa {
                found: b"GS".to_vec(),
                byte_order_mark: false,
                whitespace: 0,
            })),
            "the written file could not be read back: input does not start with an ISA segment (found bytes [47 53])",
        ),
        (
            WriteError::Findings(vec![
                Finding::DuplicateRowNumber {
                    table: "claims".into(),
                    row: 4,
                    value: 1,
                    first: 1,
                },
                Finding::OutOfOrder {
                    table: "services".into(),
                    row: 0,
                    column: "claim".into(),
                    value: 2,
                    previous: 6,
                },
            ]),
            "the tables do not make a valid file (2 findings); nothing was written\n1. table \"claims\" row 4 carries row number 1, which row 1 already carries; references to it are ambiguous\n2. table \"services\" row 0 (column \"claim\" = 2) is written after row 6; the rows of one parent must be together and in their parents' order",
        ),
    ];
    for (error, text) in cases {
        assert_eq!(error.to_string(), text);
    }
    let one = WriteError::Findings(vec![Finding::DuplicateRowNumber {
        table: "claims".into(),
        row: 4,
        value: 1,
        first: 1,
    }]);
    assert!(
        one.to_string().starts_with(
            "the tables do not make a valid file (1 finding); nothing was written\n1. "
        )
    );
    assert!(std::error::Error::source(&one).is_none());
}

#[test]
fn errors_that_print_their_inner_error_do_not_chain_to_it() {
    use std::error::Error;
    let plan = WriteError::Plan(PlanError {
        spec: "s".into(),
        refusals: Vec::new(),
    });
    assert!(plan.source().is_none());
    let unreadable = WriteError::Unreadable(DocumentError::Isa(IsaError::NotIsa {
        found: b"GS".to_vec(),
        byte_order_mark: false,
        whitespace: 0,
    }));
    assert!(unreadable.to_string().contains("found bytes [47 53]"));
    assert!(unreadable.source().is_none());
}
