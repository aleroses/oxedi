use std::error::Error;

use oxedi_core::ColumnType;

use super::{Row, SchemaError, builtin, rows};
use crate::builtins::Builtins;
use crate::types::SqlType;

fn of(table: &str, binary: bool) -> Vec<Row> {
    let builtins = Builtins::load();
    let Ok(rows) = rows(builtins.default(), binary) else {
        panic!("the default built-in is described");
    };
    rows.into_iter().filter(|row| row.table == table).collect()
}

fn find<'r>(rows: &'r [Row], column: &str) -> &'r Row {
    let Some(row) = rows.iter().find(|row| row.column == column) else {
        panic!("no column {column}");
    };
    row
}

#[test]
fn every_table_of_read_835_is_described_in_order() {
    let builtins = Builtins::load();
    for built in builtins.iter() {
        let Ok(rows) = rows(built, false) else {
            panic!("version {} is described", built.version());
        };
        let mut tables: Vec<&str> = rows.iter().map(|row| row.table.as_str()).collect();
        tables.dedup();
        let mut expected = built.table_names();
        expected.push("diagnostics".to_owned());
        assert_eq!(tables, expected, "version {}", built.version());
        for table in &built.tables {
            let described: Vec<(String, i32)> = rows
                .iter()
                .filter(|row| row.table == table.name)
                .map(|row| (row.column.clone(), row.position))
                .collect();
            let declared: Vec<(String, i32)> = table
                .columns
                .iter()
                .zip(1..)
                .map(|((name, _), position)| (name.clone(), position))
                .collect();
            assert_eq!(described, declared, "{}", table.name);
        }
    }
}

#[test]
fn reference_columns_name_their_table() {
    let services = of("services", false);
    assert_eq!(
        find(&services, "payment").parent.as_deref(),
        Some("payments")
    );
    assert_eq!(find(&services, "claim").parent.as_deref(), Some("claims"));
    assert_eq!(find(&services, "row").parent, None);
    assert_eq!(find(&services, "procedure_code").parent, None);
    assert!(
        of("diagnostics", false)
            .iter()
            .all(|row| row.parent.is_none())
    );
}

#[test]
fn types_are_the_ones_read_835_gives() {
    let claims = of("claims", false);
    assert_eq!(find(&claims, "row").sql, SqlType::Bigint);
    assert_eq!(find(&claims, "claim_id").sql, SqlType::Varchar);
    assert_eq!(
        find(&claims, "charge_amount").sql,
        SqlType::Decimal {
            width: 38,
            scale: 2
        }
    );
    assert_eq!(find(&claims, "statement_from").sql, SqlType::Date);
    let binary = of("claims", true);
    assert_eq!(find(&binary, "claim_id").sql, SqlType::Blob);
    let diagnostics = of("diagnostics", true);
    assert_eq!(find(&diagnostics, "datum").sql, SqlType::Blob);
    assert_eq!(find(&diagnostics, "rule").sql, SqlType::Varchar);
    assert_eq!(find(&diagnostics, "level").position, 1);
}

#[test]
fn the_version_picks_the_builtin() {
    let builtins = Builtins::load();
    assert_eq!(
        builtin(&builtins, None).map(|b| b.version()).ok(),
        Some("5010")
    );
    assert_eq!(
        builtin(&builtins, Some("4010")).map(|b| b.version()).ok(),
        Some("4010")
    );
    let Err(error) = builtin(&builtins, Some("3070")) else {
        panic!("3070 is not a built-in");
    };
    assert_eq!(
        error.to_string(),
        "oxedi_schema: unknown version \"3070\"; version must be one of \"5010\", \"4010\""
    );
}

#[test]
fn null_option() {
    assert_eq!(
        SchemaError::NullOption { name: "binary" }.to_string(),
        "oxedi_schema: binary must not be NULL; leave it out to use its default"
    );
}

#[test]
fn macro_parameter() {
    assert_eq!(
        SchemaError::MacroParameter {
            parameter: Some("ver".to_owned()),
            value: "0_macro_parametersm.ver".to_owned(),
        }
        .to_string(),
        "oxedi_schema: version must be a constant string; a table macro passed its parameter \
         \"ver\" as \"0_macro_parametersm.ver\"; DuckDB does not substitute a macro parameter \
         given with := or =>, so write version = ver"
    );
}

#[test]
fn macro_parameter_without_a_name() {
    assert_eq!(
        SchemaError::MacroParameter {
            parameter: None,
            value: "0_macro_parametersm.a.b".to_owned(),
        }
        .to_string(),
        "oxedi_schema: version must be a constant string; a table macro passed a parameter as \
         \"0_macro_parametersm.a.b\"; DuckDB does not substitute a macro parameter given with \
         := or =>, so write version = <the parameter>"
    );
}

#[test]
fn unknown_version() {
    assert_eq!(
        SchemaError::UnknownVersion {
            source: oxedi_core::Builtin::find_text("835", "6020").unwrap_err(),
        }
        .to_string(),
        "oxedi_schema: unknown version \"6020\"; version must be one of \"5010\", \"4010\""
    );
}

#[test]
fn unsupported_type() {
    assert_eq!(
        SchemaError::UnsupportedType {
            table: "claims".to_owned(),
            column: "charge_amount".to_owned(),
            kind: ColumnType::Decimal128 {
                precision: 18,
                scale: 2
            },
        }
        .to_string(),
        "oxedi_schema: table \"claims\", column \"charge_amount\": the core type \
         decimal128(18, 2) has no DuckDB type"
    );
}

#[test]
fn internal() {
    assert_eq!(
        SchemaError::Internal {
            message: "the scan has no bind data".to_owned()
        }
        .to_string(),
        "oxedi_schema: internal error: the scan has no bind data"
    );
}

#[test]
fn no_schema_error_has_a_source() {
    let version = SchemaError::UnknownVersion {
        source: oxedi_core::Builtin::find_text("835", "3070").unwrap_err(),
    };
    let null = SchemaError::NullOption { name: "binary" };
    for error in [version, null] {
        assert!(error.source().is_none(), "{error}");
    }
}
