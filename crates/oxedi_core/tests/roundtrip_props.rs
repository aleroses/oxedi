//! Properties of the symmetric writer: write ∘ tokenize is the identity on
//! (id, elements), and on real files the writer reproduces each segment's bytes.

mod common;

use std::borrow::Cow;

use oxedi_core::{Delimiters, Element, Segment, SegmentWriteError, Tokenizer, trivia::is_trivia};
use proptest::prelude::*;

const ELEMENT: u8 = b'*';
const COMPONENT: u8 = b':';
const SEGMENT: u8 = b'~';
const RELEASE: u8 = b'?';

/// Bytes that are never a delimiter, so a value survives with or without a release byte.
fn plain_value() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(
        any::<u8>().prop_filter("not a delimiter", |b| {
            !matches!(*b, ELEMENT | COMPONENT | SEGMENT | RELEASE)
        }),
        0..8,
    )
}

/// Any bytes at all, delimiters included: only a release byte can carry these.
fn any_value() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 0..8)
}

fn segment_id() -> impl Strategy<Value = Vec<u8>> {
    "[A-Z][A-Z0-9]{1,2}".prop_map(String::into_bytes)
}

/// One value → simple element; two or more → composite.
fn elements_from(values: Vec<Vec<Vec<u8>>>) -> Vec<Element<'static>> {
    values
        .into_iter()
        .map(|mut vs| {
            if vs.len() == 1 {
                Element::Simple(Cow::Owned(vs.remove(0)))
            } else {
                Element::Composite(vs.into_iter().map(Cow::Owned).collect())
            }
        })
        .collect()
}

fn roundtrip(
    id: &[u8],
    elements: Vec<Element<'static>>,
    delims: Delimiters,
) -> Result<(), TestCaseError> {
    let segment = Segment {
        index: 0,
        raw: b"",
        id,
        elements: elements.clone(),
        terminated: true,
    };
    let mut written = Vec::new();
    segment
        .write_to(&delims, &mut written)
        .map_err(|e| TestCaseError::fail(e.to_string()))?;
    let back: Vec<Segment<'_>> = Tokenizer::with_delimiters(&written, delims).collect();
    prop_assert_eq!(back.len(), 1, "written bytes: {:?}", written);
    prop_assert_eq!(back[0].id, id);
    prop_assert_eq!(&back[0].elements, &elements);
    Ok(())
}

proptest! {
    #[test]
    fn write_then_tokenize_restores_id_and_elements(
        id in segment_id(),
        values in prop::collection::vec(prop::collection::vec(plain_value(), 1..3), 0..6),
    ) {
        roundtrip(&id, elements_from(values), Delimiters::new(ELEMENT, COMPONENT, SEGMENT))?;
    }

    #[test]
    fn with_a_release_byte_any_value_survives(
        id in segment_id(),
        values in prop::collection::vec(prop::collection::vec(any_value(), 1..3), 0..6),
    ) {
        let delims = Delimiters::new(ELEMENT, COMPONENT, SEGMENT).with_release(RELEASE);
        roundtrip(&id, elements_from(values), delims)?;
    }
}

proptest! {
    /// Every segment the tokenizer produces from *arbitrary* bytes either writes
    /// back to something that tokenizes to the same id and elements, or is
    /// rejected because its id holds a byte the writer cannot represent.
    #[test]
    fn every_tokenized_segment_round_trips_or_is_rejected(
        input in prop::collection::vec(any::<u8>(), 0..64),
        use_release in any::<bool>(),
    ) {
        let mut delims = Delimiters::new(ELEMENT, COMPONENT, SEGMENT);
        if use_release {
            delims = delims.with_release(RELEASE);
        }
        for segment in Tokenizer::with_delimiters(&input, delims) {
            let mut written = Vec::new();
            match segment.write_to(&delims, &mut written) {
                Ok(()) => {
                    let back: Vec<Segment<'_>> = Tokenizer::with_delimiters(&written, delims).collect();
                    prop_assert_eq!(back.len(), 1, "written {:?}", written);
                    prop_assert_eq!(back[0].id, segment.id, "written {:?}", written);
                    prop_assert_eq!(&back[0].elements, &segment.elements, "written {:?}", written);
                }
                Err(SegmentWriteError::DelimiterInValue { byte, element, component }) => {
                    prop_assert_eq!(element, 0, "rejected byte must come from the id");
                    prop_assert_eq!(component, None);
                    prop_assert!(segment.id.contains(&byte), "rejected byte must come from the id");
                }
                Err(e) => prop_assert!(false, "unexpected error {e}"),
            }
        }
    }
}

/// On the real files the writer reproduces every non-empty segment's bytes exactly
/// (its `raw` minus leading trivia): nothing is normalised on the way out.
#[test]
fn writer_reproduces_every_fixture_segment_byte_for_byte() {
    for name in [
        "emedny_sample.txt",
        "united_healthcare_legacy_sample.txt",
        "multi_claim_sample.txt",
        "trizetto_sample.rmt",
    ] {
        let bytes = common::load_fixture(name);
        let tokenizer = Tokenizer::new(&bytes).unwrap();
        let delims = *tokenizer.delimiters();
        for segment in tokenizer.filter(|s| !s.is_empty()) {
            let mut written = Vec::new();
            segment.write_to(&delims, &mut written).unwrap();
            let body_start = segment.raw.iter().position(|&b| !is_trivia(b)).unwrap_or(0);
            assert_eq!(
                written,
                &segment.raw[body_start..],
                "{name} segment {}",
                segment.index
            );
        }
    }
}
