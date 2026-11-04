use oxedi_core::{Cell, Document, DocumentError, SizeError};

use super::{datum, of_unparsable, types};
use crate::error::ReadError;
use crate::types::SqlType;

#[test]
fn only_datum_follows_binary() {
    let Ok(text) = types(false) else {
        panic!("every column has a DuckDB type");
    };
    let Ok(binary) = types(true) else {
        panic!("every column has a DuckDB type");
    };
    let changed: Vec<usize> = text
        .iter()
        .zip(&binary)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(index, _)| index)
        .collect();
    assert_eq!(changed, vec![7]);
    assert_eq!(binary.get(7), Some(&SqlType::Blob));
    assert_eq!(text.first(), Some(&SqlType::Bigint));
}

#[test]
fn a_file_too_long_has_no_datum() {
    let error = DocumentError::Size(SizeError { len: usize::MAX });
    assert_eq!(datum(&error, b"ISA"), b"");
}

#[test]
fn an_unparsable_file_is_one_row() {
    let Err(source) = Document::parse(&b"hello"[..]) else {
        panic!("the input has no ISA");
    };
    let error = ReadError::Parse {
        file: "a.835".to_owned(),
        source,
    };
    let Ok(table) = of_unparsable(&error, b"hello", false) else {
        panic!("the row fits the table");
    };
    assert_eq!(table.len(), 1);
    let cell = |name: &str| table.column(name).and_then(|column| column.get(0));
    assert_eq!(cell("level"), Some(Cell::Int64(1)));
    assert_eq!(cell("kind"), Some(Cell::Binary(b"NotAnInterchange")));
    assert_eq!(
        cell("rule"),
        Some(Cell::Binary(error.to_string().as_bytes()))
    );
    assert_eq!(cell("segment"), Some(Cell::Null));
    assert_eq!(cell("path"), Some(Cell::Binary(b"")));
    assert_eq!(cell("datum"), Some(Cell::Binary(b"hello")));
    assert_eq!(cell("origin"), Some(Cell::Binary(b"read_835")));
    assert_eq!(cell("code"), Some(Cell::Null));
}

fn datum_of_binary_file(binary: bool) -> Vec<u8> {
    let bytes = b"\x1f\x8b\x08\x00\x00\x00\x00\x00rest".to_vec();
    let Err(source) = Document::parse(bytes.as_slice()) else {
        panic!("the input has no ISA");
    };
    let found = datum(&source, &bytes).to_vec();
    let error = ReadError::Parse {
        file: "a.gz".to_owned(),
        source,
    };
    let Ok(table) = of_unparsable(&error, &found, binary) else {
        panic!("the row fits the table");
    };
    match table.column("datum").and_then(|column| column.get(0)) {
        Some(Cell::Binary(bytes)) => bytes.to_vec(),
        other => panic!("the datum is a binary cell, not {other:?}"),
    }
}

#[test]
fn the_datum_of_a_binary_file_is_escaped_text_in_varchar_mode() {
    let escaped = datum_of_binary_file(false);
    assert_eq!(escaped, br"\x1f\x8b\x08\x00\x00\x00\x00\x00");
    assert!(std::str::from_utf8(&escaped).is_ok());
}

#[test]
fn the_datum_of_a_binary_file_is_raw_in_binary_mode() {
    assert_eq!(
        datum_of_binary_file(true),
        b"\x1f\x8b\x08\x00\x00\x00\x00\x00"
    );
}
