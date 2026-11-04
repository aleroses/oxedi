//! The `summaries` and `default_table` sections: what the built-in specs
//! declare, how a patch that removes what a summary reads fails, and the
//! text of every reason a summary is rejected.

use crate::spec::*;

/// One table `heads` with a text column `id` and a decimal column `amount`,
/// and the `summaries` given.
fn spec_with(summaries: &str) -> Result<Spec, SpecError> {
    Spec::from_json(&format!(
        r#"{{"name":"t",
            "loops":{{"A":{{"trigger":{{"segment":"AA"}},"occurrences":{{"aa":{{"segment":"AA","pos":0}}}}}}}},
            "segments":{{"AA":{{"elements":{{
                "1":{{"name":"id","type":"AN"}},
                "2":{{"name":"amount","type":"R","scale":2}}}}}}}},
            "tables":{{"heads":{{"loops":["A"],"columns":{{
                "id":{{"segment":"AA","element":1}},
                "amount":{{"segment":"AA","element":2}}}}}}}},
            "summaries":{summaries}}}"#
    ))
}

fn text(table: &str) -> String {
    table.to_string()
}

fn builtin_summaries() -> Vec<(String, Summary)> {
    let row = |prefix: &str| Summary::Row {
        table: text("payments"),
        fields: [
            ("name", "name"),
            ("identification_code", "id"),
            ("address", "address"),
            ("city", "city"),
            ("state", "state"),
            ("zip_code", "zip"),
        ]
        .into_iter()
        .map(|(name, suffix)| (text(name), format!("{prefix}_{suffix}")))
        .collect(),
    };
    vec![
        (
            text("count_claims"),
            Summary::Count {
                table: text("claims"),
            },
        ),
        (
            text("count_patients"),
            Summary::Distinct {
                table: text("claims"),
                column: text("patient_id"),
            },
        ),
        (text("payee"), row("payee")),
        (text("payer"), row("payer")),
        (
            text("sum_payments"),
            Summary::Sum {
                table: text("payments"),
                column: text("total_payment_amount"),
            },
        ),
    ]
}

#[test]
fn builtin_835_declares_the_five_summaries() {
    for spec in [Spec::builtin_835(), Spec::builtin_835_4010()] {
        assert_eq!(spec.summaries(), builtin_summaries().as_slice());
        assert_eq!(spec.default_table(), Some("claims"));
        assert_eq!(
            spec.summary("sum_payments"),
            Some(&Summary::Sum {
                table: text("payments"),
                column: text("total_payment_amount"),
            })
        );
        assert_eq!(spec.summary("total"), None);
    }
}

#[test]
fn the_compat_patch_still_loads() {
    let spec = Spec::builtin_835()
        .merge_patch(include_str!("../../../specs/edi_835_parser.json"))
        .unwrap();
    assert!(spec.summaries().is_empty());
    assert_eq!(spec.default_table(), None);
}

#[test]
fn removing_a_summarized_table_names_the_summary() {
    let err = Spec::builtin_835()
        .merge_patch(r#"{"tables":{"payments":null}}"#)
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "applying patch: summary \"payee\": row.table names the table \"payments\", which the \
         spec does not define; its tables are: adjustments, claims, provider_adjustments, services"
    );
}

#[test]
fn removing_a_summarized_column_names_the_summary() {
    let spec = Spec::builtin_835();
    let columns: Vec<&str> = spec
        .table("payments")
        .unwrap()
        .columns
        .iter()
        .map(|(name, _)| name.as_str())
        .filter(|name| *name != "total_payment_amount")
        .collect();
    let err = spec
        .merge_patch(r#"{"tables":{"payments":{"columns":{"total_payment_amount":null}}}}"#)
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "applying patch: summary \"sum_payments\": sum.column names the column \
             \"total_payment_amount\", which the table \"payments\" does not declare; its \
             columns are: {}",
            columns.join(", ")
        )
    );
}

#[test]
fn a_spec_without_summaries_declares_none() {
    let spec = spec_with("{}").unwrap();
    assert!(spec.summaries().is_empty());
    assert_eq!(spec.default_table(), None);
}

#[test]
fn every_reason_a_summary_is_rejected_is_displayed_in_full() {
    let cases = [
        (
            r#"{"":{"count":{"table":"heads"}}}"#,
            "summary \"\": the name is empty",
        ),
        (
            r#"{"s":{}}"#,
            "summary \"s\": holds no operation; a summary holds exactly one of count, distinct, \
             sum, row",
        ),
        (
            r#"{"s":{"sum":{"table":"heads","column":"amount"},"count":{"table":"heads"}}}"#,
            "summary \"s\": holds 2 operations (count, sum); a summary holds exactly one of \
             count, distinct, sum, row",
        ),
        (
            r#"{"s":{"count":{"table":"head"}}}"#,
            "summary \"s\": count.table names the table \"head\", which the spec does not \
             define; its tables are: heads",
        ),
        (
            r#"{"s":{"distinct":{"table":"heads","column":"code"}}}"#,
            "summary \"s\": distinct.column names the column \"code\", which the table \"heads\" \
             does not declare; its columns are: amount, id",
        ),
        (
            r#"{"s":{"sum":{"table":"heads","column":"id"}}}"#,
            "summary \"s\": sum.column names the column \"id\" of the table \"heads\", which \
             reads AN (string); sum needs a decimal (R) column",
        ),
        (
            r#"{"s":{"row":{"table":"heads","fields":[]}}}"#,
            "summary \"s\": row.fields lists no field",
        ),
        (
            r#"{"s":{"row":{"table":"heads","fields":[{"name":"a","column":"id"},{"name":"b","column":"x"}]}}}"#,
            "summary \"s\": row.fields[1].column names the column \"x\", which the table \
             \"heads\" does not declare; its columns are: amount, id",
        ),
        (
            r#"{"s":{"row":{"table":"heads","fields":[{"name":"a","column":"id"},{"name":"a","column":"amount"}]}}}"#,
            "summary \"s\": row.fields[1].name repeats the field \"a\" first named at \
             row.fields[0].name",
        ),
    ];
    for (summaries, expected) in cases {
        let err = spec_with(summaries).unwrap_err();
        let SpecError::BadSummary { reason, .. } = &err else {
            panic!("{err:?}");
        };
        assert!(
            std::error::Error::source(reason.as_ref()).is_none(),
            "{reason}"
        );
        assert_eq!(err.to_string(), expected, "{summaries}");
    }
}

#[test]
fn a_summary_that_compiles_keeps_what_it_names() {
    let spec = spec_with(
        r#"{"n":{"count":{"table":"heads"}},
            "r":{"row":{"table":"heads","fields":[{"name":"b","column":"id"},{"name":"a","column":"amount"}]}}}"#,
    )
    .unwrap();
    assert_eq!(
        spec.summary("r"),
        Some(&Summary::Row {
            table: text("heads"),
            fields: vec![(text("b"), text("id")), (text("a"), text("amount"))],
        })
    );
    assert_eq!(
        spec.summaries()
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["n", "r"]
    );
}

#[test]
fn a_default_table_the_spec_does_not_define_is_rejected() {
    let spec = spec_with("{}").unwrap();
    let err = spec.merge_patch(r#"{"default_table":"nope"}"#).unwrap_err();
    assert_eq!(
        err.to_string(),
        "applying patch: default_table names the table \"nope\", which the spec does not \
         define; its tables are: heads"
    );
    let spec = spec.merge_patch(r#"{"default_table":"heads"}"#).unwrap();
    assert_eq!(spec.default_table(), Some("heads"));
}

#[test]
fn the_shape_of_the_sections_is_checked_with_its_key_path() {
    let cases = [
        (
            r#"{"summaries":[]}"#,
            "applying patch: spec: the value at summaries must be a JSON object; found an array",
        ),
        (
            r#"{"default_table":3}"#,
            "applying patch: spec: the value at default_table must be a string; found a number (3)",
        ),
        (
            r#"{"summaries":{"s":{"avg":{}}}}"#,
            "applying patch: spec: unknown key \"avg\" at summaries.s; the allowed keys are: \
             count, distinct, row, sum",
        ),
        (
            r#"{"summaries":{"s":{"row":{"table":"heads","fields":{}}}}}"#,
            "applying patch: spec: the value at summaries.s.row.fields must be an array of \
             objects; found an object (0 members)",
        ),
        (
            r#"{"summaries":{"s":{"distinct":{"table":"heads"}}}}"#,
            "applying patch: spec: missing required key \"column\" at summaries.s.distinct",
        ),
    ];
    let spec = spec_with("{}").unwrap();
    for (patch, expected) in cases {
        let err = spec.merge_patch(patch).unwrap_err();
        assert_eq!(err.to_string(), expected, "{patch}");
    }
}
