use oxedi_core::Document;

use super::Builtins;

const TABLES: [&str; 5] = [
    "adjustments",
    "claims",
    "payments",
    "provider_adjustments",
    "services",
];

#[test]
fn every_registry_entry_has_a_schema_at_its_position() {
    let builtins = Builtins::load();
    let schemas: Vec<_> = builtins.iter().collect();
    assert_eq!(schemas.len(), oxedi_core::Builtin::all().len());
    for (schema, known) in schemas.iter().zip(oxedi_core::Builtin::all()) {
        assert_eq!(schema.version(), known.version.text());
        assert!(std::ptr::eq(builtins.of(known).unwrap(), *schema));
    }
}

#[test]
fn the_default_is_5010_and_4010_follows() {
    let builtins = Builtins::load();
    assert_eq!(
        builtins.default().version(),
        oxedi_core::Builtin::DEFAULT_VERSION.text()
    );
    assert_eq!(
        builtins.by_version("4010").map(|b| b.version()).ok(),
        Some("4010")
    );
    let error = builtins.by_version("3070").unwrap_err();
    assert_eq!(
        error.to_string(),
        "unknown version \"3070\"; version must be one of \"5010\", \"4010\""
    );
}

#[test]
fn every_builtin_projects_the_same_tables_with_the_same_columns() {
    let builtins = Builtins::load();
    let default = builtins.default();
    let mut names = default.table_names();
    names.sort();
    assert_eq!(names, TABLES);
    for builtin in builtins.iter() {
        assert_eq!(
            builtin.tables,
            default.tables,
            "version {}",
            builtin.version()
        );
    }
}

#[test]
fn a_table_is_found_by_name() {
    let builtins = Builtins::load();
    let claims = builtins.default().table("claims");
    assert_eq!(claims.map(|t| t.name.as_str()), Some("claims"));
    assert!(claims.is_some_and(|t| t.columns.iter().any(|(name, _)| name == "claim_id")));
    assert!(builtins.default().table("nope").is_none());
}

fn interchange(version: &str) -> Vec<u8> {
    format!(
        "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *240101*1200*^*00501*000000001*0*P*:~\
         GS*HP*SENDER*RECEIVER*20240101*1200*1*X*{version}~ST*835*0001~SE*2*0001~GE*1*1~IEA*1*000000001~"
    )
    .into_bytes()
}

#[test]
fn select_follows_the_declared_version() {
    let builtins = Builtins::load();
    for (declared, expected) in [
        ("004010X091A1", "4010"),
        ("005010X221A1", "5010"),
        ("999999", "5010"),
    ] {
        let Ok(document) = Document::parse(interchange(declared)) else {
            panic!("the interchange indexes");
        };
        assert_eq!(
            builtins.select(document.segments()).map(|b| b.version()),
            Some(expected),
            "{declared}"
        );
    }
}
