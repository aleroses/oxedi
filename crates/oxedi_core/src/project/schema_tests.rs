//! The schema of a spec's tables, known without a file.

use super::tests::{delimiters, spec};
use super::{ColumnSchema, Projector, TableSchema, spec_schema, table_columns};
use crate::spec::Spec;
use crate::value::ColumnType;

const INDEX: ColumnType = ColumnType::Int64 { scale: 0 };

fn column(name: &str, kind: ColumnType, parent: Option<&str>) -> ColumnSchema {
    ColumnSchema {
        name: name.to_string(),
        kind,
        parent: parent.map(str::to_string),
    }
}

/// The names and types of the tables a projector that saw no segment holds.
fn projected(spec: &Spec) -> Vec<(String, Vec<(String, ColumnType)>)> {
    Projector::new(spec, &delimiters())
        .take_tables()
        .iter()
        .map(|table| {
            let columns = table
                .columns()
                .iter()
                .map(|(name, data)| (name.clone(), data.kind()))
                .collect();
            (table.name().to_string(), columns)
        })
        .collect()
}

fn names_and_kinds(schema: &[TableSchema]) -> Vec<(String, Vec<(String, ColumnType)>)> {
    schema
        .iter()
        .map(|table| {
            let columns = table
                .columns
                .iter()
                .map(|column| (column.name.clone(), column.kind))
                .collect();
            (table.name.clone(), columns)
        })
        .collect()
}

#[test]
fn the_schema_is_what_the_projector_fills() {
    let patched = Spec::builtin_835()
        .merge_patch(
            r#"{"tables":{"claims":{"columns":{"received":{"segment":"DTM","where":{"1":"050"},"element":2}}}}}"#,
        )
        .unwrap();
    for spec in [
        spec(),
        Spec::builtin_835(),
        Spec::builtin_835_4010(),
        patched,
    ] {
        assert_eq!(
            names_and_kinds(&spec_schema(&spec)),
            projected(&spec),
            "{}",
            spec.name()
        );
    }
}

#[test]
fn reference_columns_name_the_table_they_point_at() {
    let schema = spec_schema(&spec());
    let lines = schema.iter().find(|table| table.name == "lines").unwrap();
    assert_eq!(
        lines.columns,
        vec![
            column("row", INDEX, None),
            column("segment", INDEX, None),
            column("head", INDEX, Some("heads")),
            column("claim", INDEX, Some("claims")),
            column("code", ColumnType::Binary, None),
            column("date", ColumnType::Date32, None),
            column("time", ColumnType::Time32, None),
        ]
    );
    let heads = schema.iter().find(|table| table.name == "heads").unwrap();
    assert!(heads.columns.iter().all(|column| column.parent.is_none()));
}

#[test]
fn the_tables_come_in_spec_order_with_their_columns() {
    let spec = spec();
    let schema = spec_schema(&spec);
    let names: Vec<&str> = schema.iter().map(|table| table.name.as_str()).collect();
    assert_eq!(names, ["adjustments", "claims", "heads", "lines"]);
    for (table, def) in schema.iter().zip(spec.tables()) {
        let pairs: Vec<(String, ColumnType)> = table
            .columns
            .iter()
            .map(|column| (column.name.clone(), column.kind))
            .collect();
        assert_eq!(pairs, table_columns(&spec, def));
    }
}

#[test]
fn a_patched_column_takes_its_element_type() {
    let spec = Spec::builtin_835()
        .merge_patch(
            r#"{"tables":{"payments":{"columns":{"check_date":{"segment":"BPR","element":16}}}}}"#,
        )
        .unwrap();
    let schema = spec_schema(&spec);
    let payments = schema
        .iter()
        .find(|table| table.name == "payments")
        .unwrap();
    let check_date = payments
        .columns
        .iter()
        .find(|column| column.name == "check_date")
        .unwrap();
    assert_eq!(check_date, &column("check_date", ColumnType::Date32, None));
}
