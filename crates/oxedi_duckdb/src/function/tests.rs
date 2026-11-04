use super::{c_message, guarded, panic_message, table_name};
use crate::builtins::Builtins;
use crate::error::ReadError;
use crate::options::TableChoice;

#[test]
fn a_panic_becomes_an_internal_error() {
    let result: Result<(), ReadError> = guarded(|| panic!("boom {}", 1));
    assert_eq!(
        result.map_err(|error| error.to_string()),
        Err("read_835: internal error: boom 1".to_owned())
    );
}

#[test]
fn an_error_passes_through() {
    let result: Result<(), ReadError> = guarded(|| Err(ReadError::EmptyPathList));
    assert!(matches!(result, Err(ReadError::EmptyPathList)));
}

#[test]
fn panic_payloads_without_text() {
    assert_eq!(panic_message(&42u8), "a panic without a message");
    assert_eq!(panic_message(&"static"), "static");
}

#[test]
fn nul_bytes_are_written_out() {
    let error = ReadError::Internal {
        message: "a\0b".to_owned(),
    };
    assert_eq!(
        c_message(&error).to_str().ok(),
        Some("read_835: internal error: a\\0b")
    );
}

#[test]
fn without_table_name_the_spec_default_table_is_read() {
    let builtins = Builtins::load();
    for builtin in builtins.iter() {
        let expected = builtin.spec().default_table();
        assert!(expected.is_some(), "{}", builtin.version());
        assert_eq!(
            table_name(builtin, &TableChoice::SpecDefault).ok(),
            expected
        );
        assert!(builtin.table(expected.unwrap_or_default()).is_some());
    }
}

#[test]
fn a_named_table_is_read_as_named() {
    let builtins = Builtins::load();
    let choice = TableChoice::Named("diagnostics".to_owned());
    assert_eq!(
        table_name(builtins.default(), &choice).ok(),
        Some("diagnostics")
    );
}
