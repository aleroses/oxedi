//! Properties of the document: it yields exactly what the tokenizer yields, its
//! spans partition the input, and the spans it derives from its compact index
//! equal the ones recorded straight from the framing pass.

mod common;

use common::delimiters;

use oxedi_core::frame::{first_frame, next_frame};
use oxedi_core::trivia::BYTE_ORDER_MARK;
use oxedi_core::{Delimiters, Document, Span, Tokenizer};
use proptest::prelude::*;

/// The spans as recorded straight from each frame, without deriving anything:
/// the reference the compact index must reproduce.
fn reference_spans(bytes: &[u8], delims: &Delimiters) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut rest = bytes;
    let mut offset = 0;
    loop {
        let split = if offset == 0 {
            first_frame(rest, delims)
        } else {
            next_frame(rest, delims)
        };
        let Some((frame, next)) = split else {
            break;
        };
        let trivia = frame.raw.len() - frame.body.len() - usize::from(frame.terminated);
        let raw = offset..offset + frame.raw.len();
        let body = raw.start + trivia..raw.start + trivia + frame.body.len();
        spans.push(Span {
            raw: raw.clone(),
            body,
            terminated: frame.terminated,
        });
        offset = raw.end;
        rest = next;
    }
    spans
}

fn assert_spans_match_reference(bytes: &[u8], delims: Delimiters, name: &str) {
    let doc = Document::with_delimiters(bytes, delims).unwrap();
    let derived: Vec<Span> = doc.spans().collect();
    let reference = reference_spans(bytes, &delims);
    assert_eq!(derived, reference, "{name}: derived spans");
    for (i, span) in reference.iter().enumerate() {
        assert_eq!(doc.span(i).as_ref(), Some(span), "{name}: span({i})");
    }
    assert_eq!(doc.span(reference.len()), None, "{name}: past the end");
}

#[test]
fn derived_spans_equal_recorded_spans_on_every_file() {
    for (name, bytes, delims) in common::all_files() {
        assert_spans_match_reference(&bytes, delims, &name);
        let mut marked = BYTE_ORDER_MARK.to_vec();
        marked.extend_from_slice(&bytes);
        assert_spans_match_reference(&marked, delims, &format!("{name} with a byte order mark"));
    }
}

#[test]
fn derived_spans_equal_recorded_spans_at_the_edges() {
    let edges: &[(&str, &[u8])] = &[
        ("empty input", b""),
        ("byte order mark alone", b"\xEF\xBB\xBF"),
        ("byte order mark then trivia", b"\xEF\xBB\xBF\r\n"),
        ("byte order mark then a segment", b"\xEF\xBB\xBFST*835~"),
        ("byte order mark then a terminator", b"\xEF\xBB\xBF~"),
        ("byte order mark not at the start", b"ST~\xEF\xBB\xBFSE~"),
        ("trivia alone", b"\r\n \t"),
        ("trailing trivia only", b"ST*835~\r\n"),
        ("final segment with no terminator", b"ST*835~SE*2"),
        ("single unterminated segment", b"ST"),
        ("terminator alone", b"~"),
        ("empty segment between terminators", b"ST~~SE~"),
        ("only terminators", b"~~~"),
        ("trivia before a terminator", b"ST~\n~"),
        ("ISA-less fragment", b"ST*835*1~BPR*I~SE*3*1~\n"),
    ];
    for &(name, bytes) in edges {
        assert_spans_match_reference(bytes, delimiters(false), name);
        assert_spans_match_reference(bytes, delimiters(true), &format!("{name}, release"));
    }
    assert_spans_match_reference(b"ST?~~SE~?", delimiters(true), "escaped terminators");
}

proptest! {
    #[test]
    fn document_segments_equal_tokenizer_segments(
        input in prop::collection::vec(any::<u8>(), 0..256),
        use_release in any::<bool>(),
    ) {
        let delims = delimiters(use_release);
        let doc = Document::with_delimiters(&input[..], delims).unwrap();
        let from_doc: Vec<_> = doc.segments().collect();
        let from_tokenizer: Vec<_> = Tokenizer::with_delimiters(&input, delims).collect();
        prop_assert_eq!(from_doc, from_tokenizer);
    }

    #[test]
    fn spans_partition_the_input(
        input in prop::collection::vec(any::<u8>(), 0..256),
        use_release in any::<bool>(),
    ) {
        let doc = Document::with_delimiters(&input[..], delimiters(use_release)).unwrap();
        let mut next = 0;
        for span in doc.spans() {
            prop_assert_eq!(span.raw.start, next);
            prop_assert!(span.raw.start <= span.body.start);
            prop_assert!(span.body.end <= span.raw.end);
            next = span.raw.end;
        }
        prop_assert_eq!(next, input.len());
    }

    #[test]
    fn derived_spans_equal_recorded_spans(
        input in prop::collection::vec(
            prop::sample::select(&b"ST*:~?\r\n \xEF\xBB\xBFx"[..]),
            0..256,
        ),
        marked in any::<bool>(),
        use_release in any::<bool>(),
    ) {
        let delims = delimiters(use_release);
        let mut bytes = if marked { BYTE_ORDER_MARK.to_vec() } else { Vec::new() };
        bytes.extend_from_slice(&input);
        let doc = Document::with_delimiters(&bytes[..], delims).unwrap();
        let derived: Vec<Span> = doc.spans().collect();
        prop_assert_eq!(derived, reference_spans(&bytes, &delims));
    }

    #[test]
    fn owned_document_equals_borrowed_document(
        input in prop::collection::vec(any::<u8>(), 0..256),
    ) {
        // The expectation borrows `input`, not the document, so the document
        // can be moved into `into_owned` while it is alive.
        let delims = delimiters(false);
        let expected: Vec<_> = Tokenizer::with_delimiters(&input, delims).collect();
        let owned = Document::with_delimiters(&input[..], delims).unwrap().into_owned();
        let from_owned: Vec<_> = owned.segments().collect();
        prop_assert_eq!(from_owned, expected);
        prop_assert_eq!(owned.as_bytes(), &input[..]);
    }
}
