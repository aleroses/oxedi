use std::error::Error;

use oxedi_core::{ColumnType, Document, MaskError};

use super::ReadError;

fn text(error: ReadError) -> String {
    error.to_string()
}

#[test]
fn path_type() {
    assert_eq!(
        text(ReadError::PathType {
            found: "INTEGER".to_owned()
        }),
        "read_835: the path must be a VARCHAR or a list of VARCHAR; found INTEGER"
    );
}

#[test]
fn null_path() {
    assert_eq!(
        text(ReadError::NullPath { position: None }),
        "read_835: the path must not be NULL"
    );
}

#[test]
fn null_path_in_list() {
    assert_eq!(
        text(ReadError::NullPath { position: Some(2) }),
        "read_835: every path of the list must be a string; path 2 is NULL"
    );
}

#[test]
fn empty_path_list() {
    assert_eq!(
        text(ReadError::EmptyPathList),
        "read_835: the list of paths must not be empty"
    );
}

#[test]
fn macro_parameter() {
    assert_eq!(
        text(ReadError::MacroParameter {
            name: "table_name",
            parameter: Some("tbl".to_owned()),
            value: "0_macro_parametersone.tbl".to_owned(),
        }),
        "read_835: table_name must be a constant string; a table macro passed its parameter \
         \"tbl\" as \"0_macro_parametersone.tbl\"; DuckDB does not substitute a macro \
         parameter given with := or =>, so write table_name = tbl"
    );
}

#[test]
fn macro_parameter_without_a_name() {
    assert_eq!(
        text(ReadError::MacroParameter {
            name: "table_name",
            parameter: None,
            value: "0_macro_parametersm.a.b".to_owned(),
        }),
        "read_835: table_name must be a constant string; a table macro passed a parameter as \
         \"0_macro_parametersm.a.b\"; DuckDB does not substitute a macro parameter given with \
         := or =>, so write table_name = <the parameter>"
    );
}

#[test]
fn macro_parameter_that_needs_quoting() {
    assert_eq!(
        text(ReadError::MacroParameter {
            name: "version",
            parameter: Some("My \"Ver\"".to_owned()),
            value: "0_macro_parametersone.My \"Ver\"".to_owned(),
        }),
        "read_835: version must be a constant string; a table macro passed its parameter \
         \"My \\\"Ver\\\"\" as \"0_macro_parametersone.My \\\"Ver\\\"\"; DuckDB does not \
         substitute a macro parameter given with := or =>, so write version = \"My \"\"Ver\"\"\""
    );
}

#[test]
fn identifier_quoting() {
    assert_eq!(super::identifier("tbl_2"), "tbl_2");
    assert_eq!(super::identifier("_x"), "_x");
    for needs_quotes in ["Tbl", "my table", "a.b", "2x", "", "select\"", "ünï"] {
        assert!(
            super::identifier(needs_quotes).starts_with('"'),
            "{needs_quotes}"
        );
    }
    assert_eq!(super::identifier("a\"b"), "\"a\"\"b\"");
    assert_eq!(super::identifier("order"), "\"order\"");
    assert_eq!(super::identifier("ordering"), "ordering");
}

#[test]
fn unknown_table() {
    assert_eq!(
        text(ReadError::UnknownTable {
            table: "nope".to_owned(),
            known: vec!["claims".to_owned(), "services".to_owned()],
        }),
        "read_835: unknown table \"nope\"; table_name must be one of \"claims\", \"services\""
    );
}

#[test]
fn unknown_version() {
    assert_eq!(
        text(ReadError::UnknownVersion {
            source: oxedi_core::Builtin::find_text("835", "3070").unwrap_err(),
        }),
        "read_835: unknown version \"3070\"; version must be one of \"5010\", \"4010\""
    );
}

#[test]
fn open() {
    assert_eq!(
        text(ReadError::Open {
            file: "a.835".to_owned(),
            message: "No such file or directory".to_owned(),
        }),
        "read_835: \"a.835\" could not be opened: No such file or directory"
    );
}

#[test]
fn read() {
    assert_eq!(
        text(ReadError::Read {
            file: "a.835".to_owned(),
            message: "connection reset".to_owned(),
        }),
        "read_835: \"a.835\" could not be read: connection reset"
    );
}

#[test]
fn parse_names_the_file_and_does_not_chain_the_core_error() {
    let Err(source) = Document::parse(b"not an interchange".to_vec()) else {
        panic!("the input has no ISA");
    };
    let core = source.to_string();
    let error = ReadError::Parse {
        file: "a.835".to_owned(),
        source,
    };
    assert_eq!(
        error.to_string(),
        format!("read_835: \"a.835\" is not an X12 interchange: {core}")
    );
    assert!(error.source().is_none());
}

#[test]
fn invalid_utf8() {
    assert_eq!(
        text(ReadError::InvalidUtf8 {
            file: "a.835".to_owned(),
            table: "claims".to_owned(),
            column: "patient_last_name".to_owned(),
            row: Some(3),
            index: 2,
            bytes: b"M\xfcLLER".to_vec(),
        }),
        "read_835: \"a.835\", table \"claims\", column \"patient_last_name\", row 3: \
         a VARCHAR must be valid UTF-8; found b\"M\\xfcLLER\"; \
         pass binary := true to read text columns as BLOB"
    );
}

#[test]
fn schema_mismatch() {
    assert_eq!(
        text(ReadError::SchemaMismatch {
            file: "a.835".to_owned(),
            table: "claims".to_owned(),
            version: "4010",
            bound: "5010",
        }),
        "read_835: \"a.835\" declares version 4010, whose table \"claims\" has other columns \
         than version 5010, which the query was bound with; pass version := '4010' to read \
         such files on their own"
    );
}

#[test]
fn unsupported_type() {
    assert_eq!(
        text(ReadError::UnsupportedType {
            table: "claims".to_owned(),
            column: "charge_amount".to_owned(),
            kind: ColumnType::Decimal128 {
                precision: 9,
                scale: 2
            },
        }),
        "read_835: table \"claims\", column \"charge_amount\": the core type \
         decimal128(9, 2) has no DuckDB type"
    );
}

#[test]
fn internal() {
    assert_eq!(
        text(ReadError::Internal {
            message: "boom".to_owned()
        }),
        "read_835: internal error: boom"
    );
}

#[test]
fn no_read_error_has_a_source() {
    let mask = ReadError::Mask {
        name: "mask_key",
        source: MaskError::ShortKey { length: 5 },
    };
    let parse = ReadError::Parse {
        file: "a.835".to_owned(),
        source: Document::parse(b"not an interchange".to_vec()).unwrap_err(),
    };
    let open = ReadError::Open {
        file: "a.835".to_owned(),
        message: "denied".to_owned(),
    };
    let version = ReadError::UnknownVersion {
        source: oxedi_core::Builtin::find_text("835", "3070").unwrap_err(),
    };
    for error in [ReadError::EmptyPathList, mask, parse, open, version] {
        assert!(error.source().is_none(), "{error}");
    }
}

#[test]
fn no_files() {
    assert_eq!(
        text(ReadError::NoFiles {
            pattern: "data/*.835".to_owned()
        }),
        "read_835: no file matches the pattern \"data/*.835\""
    );
}

#[test]
fn glob() {
    assert_eq!(
        text(ReadError::Glob {
            pattern: "s3://bucket/*.835".to_owned(),
            message: "HTTP 403".to_owned(),
        }),
        "read_835: the pattern \"s3://bucket/*.835\" could not be expanded: HTTP 403"
    );
}

#[test]
fn invalid_utf8_without_a_row_column() {
    assert_eq!(
        text(ReadError::InvalidUtf8 {
            file: "a.835".to_owned(),
            table: "t".to_owned(),
            column: "c".to_owned(),
            row: None,
            index: 2,
            bytes: b"\xff".to_vec(),
        }),
        "read_835: \"a.835\", table \"t\", column \"c\", row index 2: \
         a VARCHAR must be valid UTF-8; found b\"\\xff\"; \
         pass binary := true to read text columns as BLOB"
    );
}

#[test]
fn null_option() {
    assert_eq!(
        text(ReadError::NullOption { name: "table_name" }),
        "read_835: table_name must not be NULL; leave it out to use its default"
    );
    assert_eq!(
        text(ReadError::NullOption { name: "mask_key" }),
        "read_835: mask_key must not be NULL; leave it out to read without masking, \
         or give a key of at least 16 bytes (a NULL often means a variable is not set)"
    );
}

#[test]
fn pattern_without_external_access() {
    assert_eq!(
        text(ReadError::PatternWithoutExternalAccess {
            pattern: "data/*.835".to_owned()
        }),
        "read_835: the pattern \"data/*.835\" cannot be expanded while \
         enable_external_access is false; list the files instead"
    );
}

#[test]
fn mask_key_too_short() {
    assert_eq!(
        text(ReadError::Mask {
            name: "mask_key",
            source: MaskError::ShortKey { length: 5 },
        }),
        "read_835: mask_key: the mask key must be at least 16 bytes long; got 5"
    );
}

#[test]
fn mask_dates_unknown() {
    assert_eq!(
        text(ReadError::Mask {
            name: "mask_dates",
            source: MaskError::UnknownDateMode {
                given: "month".to_owned()
            },
        }),
        "read_835: mask_dates: the mask date mode must be \"year\", \"shift\" or \"keep\"; \
         got \"month\""
    );
}

#[test]
fn mask_dates_without_key() {
    assert_eq!(
        text(ReadError::MaskDatesWithoutKey {
            dates: "year".to_owned()
        }),
        "read_835: mask_dates \"year\" was given without mask_key; mask_dates only chooses how \
         a mask treats dates, so give mask_key as well"
    );
}
