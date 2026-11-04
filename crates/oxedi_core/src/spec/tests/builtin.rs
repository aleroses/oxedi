//! The registry of built-in specs.

use std::error::Error;

use crate::Document;
use crate::spec::{Builtin, UnknownVersion, Version};

fn selected(bytes: &[u8]) -> Version {
    let document = Document::parse(bytes.to_vec()).unwrap();
    Builtin::select(document.segments()).version
}

#[test]
fn the_fallback_is_the_first_and_named_by_default_version() {
    assert_eq!(Builtin::all()[0].version, Builtin::DEFAULT_VERSION);
    assert_eq!(Builtin::DEFAULT_VERSION, Version::ALL[0]);
    let first = &Builtin::all()[0];
    let fallback = Builtin::fallback();
    assert_eq!(
        (fallback.transaction, fallback.version),
        (first.transaction, first.version)
    );
}

#[test]
fn find_835_4010_gives_the_4010_patch() {
    let found = Builtin::find("835", Version::V4010).unwrap();
    let declared = found.spec.version().unwrap();
    assert!(declared.values.iter().any(|v| v == b"004010X091A1"));
}

#[test]
fn select_reads_the_declared_version() {
    let four = include_bytes!("../../../tests/fixtures/balanced_4010_sample.txt");
    let five = include_bytes!("../../../tests/fixtures/balanced_5010_sample.txt");
    assert_eq!(selected(four), Version::V4010);
    assert_eq!(selected(five), Version::V5010);
    assert_eq!(
        selected(b"ISA*00*          *00*          *ZZ*A              *ZZ*B              *200101*1200*^*00501*000000001*0*P*:~"),
        Builtin::fallback().version
    );
}

#[test]
fn unknown_version_display_lists_the_known() {
    let error = Builtin::find_text("835", "3070").unwrap_err();
    assert_eq!(
        error.to_string(),
        "unknown version \"3070\"; version must be one of \"5010\", \"4010\""
    );
}

#[test]
fn unknown_version_display_of_an_unknown_transaction() {
    let error = Builtin::find_text("277", "3070").unwrap_err();
    assert_eq!(
        error.to_string(),
        "unknown version \"3070\"; no built-in spec covers transaction \"277\""
    );
}

#[test]
fn unknown_version_has_no_source() {
    let error: UnknownVersion = Builtin::find_text("835", "3070").unwrap_err();
    assert!(error.source().is_none());
}

#[test]
fn version_text_and_parse_round_trip() {
    for version in Version::ALL {
        assert_eq!(Version::parse(version.text()), Some(version));
    }
    assert_eq!(Version::V5010.text(), "5010");
    assert_eq!(Version::V4010.text(), "4010");
    assert_eq!(Version::parse("6020"), None);
}

#[test]
fn find_text_names_the_versions_as_text() {
    assert_eq!(
        Builtin::find_text("835", "4010").unwrap().version,
        Version::V4010
    );
    assert_eq!(
        Builtin::find_text("835", "6020").unwrap_err().to_string(),
        "unknown version \"6020\"; version must be one of \"5010\", \"4010\""
    );
    assert_eq!(
        Builtin::find_text("277", "5010").unwrap_err().to_string(),
        "unknown version \"5010\"; no built-in spec covers transaction \"277\""
    );
}

#[test]
fn find_takes_a_typed_version() {
    let found = Builtin::find("835", Version::V4010).unwrap();
    assert_eq!(found.version, Version::V4010);
    assert_eq!(
        Builtin::versions("835"),
        vec![Version::V5010, Version::V4010]
    );
    assert_eq!(Builtin::DEFAULT_VERSION, Version::V5010);
}

#[test]
fn every_version_is_listed_in_all_at_its_index() {
    // The exhaustive match stops compiling when a variant is added, until the
    // index and the count here, and so `Version::ALL`, name it.
    const VARIANTS: usize = 2;
    fn index(version: Version) -> usize {
        match version {
            Version::V5010 => 0,
            Version::V4010 => 1,
        }
    }
    assert_eq!(Version::ALL.len(), VARIANTS);
    for (at, version) in Version::ALL.into_iter().enumerate() {
        assert_eq!(index(version), at);
    }
}
