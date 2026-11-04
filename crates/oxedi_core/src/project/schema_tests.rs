//! The schema of a spec's tables agrees with what the projector fills.

use super::Projector;
use super::tests::{delimiters, spec};
use crate::spec::{Spec, TableSchema, spec_schema};
use crate::value::ColumnType;

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
