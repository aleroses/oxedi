//! Anonymized real payer files through the whole crate: lossless, identical
//! between document and tokenizer, and writable back byte for byte.

mod common;

use common::concat_raw;

use oxedi_core::{Document, Tokenizer, trivia::is_trivia};

/// (file, segments including a trailing trivia-only one, repetition separator).
const SAMPLES: &[(&str, usize, Option<u8>)] = &[
    ("edi835_test_davisvision.RMT", 33, None),
    ("edi835_test_eyemed.RMT", 1206, None),
    ("edi835_test_file.RMT", 80, None),
    ("edi835_test_not_available_claim_id.RMT", 259, None),
    ("edi835_test_united.rmt", 30302, Some(b'^')),
    ("edi835_test_versant.RMT", 10177, None),
];

#[test]
fn the_expectations_name_every_sample() {
    let named: Vec<&str> = SAMPLES.iter().map(|&(name, _, _)| name).collect();
    common::assert_same_names("SAMPLES here vs common::SAMPLES", &named, common::SAMPLES);
}

#[test]
fn samples_tokenize_losslessly_from_their_isa() {
    for &(name, count, repetition) in SAMPLES {
        let bytes = common::load_sample(name);
        let tokenizer = Tokenizer::new(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        let delims = *tokenizer.delimiters();
        assert_eq!(
            (delims.element, delims.component, delims.segment),
            (b'*', b':', b'~'),
            "{name}"
        );
        assert_eq!(delims.repetition, repetition, "{name}");
        let segments: Vec<_> = tokenizer.collect();
        assert_eq!(segments.len(), count, "{name}: segment count");
        assert_eq!(
            concat_raw(&segments),
            bytes,
            "{name}: concatenated raw must equal the file"
        );
        let tildes = bytes.iter().filter(|&&b| b == b'~').count();
        assert_eq!(
            segments.iter().filter(|s| s.terminated).count(),
            tildes,
            "{name}"
        );
        assert_eq!(segments[0].id, b"ISA", "{name}");
        assert_eq!(
            segments.iter().rev().find(|s| !s.is_empty()).map(|s| s.id),
            Some(&b"IEA"[..]),
            "{name}"
        );
        for (i, segment) in segments.iter().enumerate() {
            assert_eq!(segment.index, i, "{name}");
        }
    }
}

#[test]
fn samples_document_equals_tokenizer() {
    for &(name, count, _) in SAMPLES {
        let bytes = common::load_sample(name);
        let doc = Document::parse(&bytes[..]).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(doc.len(), count, "{name}");
        assert_eq!(doc.as_bytes(), &bytes[..], "{name}");
        let expected: Vec<_> = Tokenizer::new(&bytes).unwrap().collect();
        let from_doc: Vec<_> = doc.segments().collect();
        assert_eq!(
            from_doc, expected,
            "{name}: document must yield the tokenizer's segments"
        );
    }
}

#[test]
fn samples_only_contain_segment_ids_an_835_can_carry() {
    const KNOWN: &[&[u8]] = &[
        b"ISA", b"GS", b"ST", b"BPR", b"TRN", b"CUR", b"REF", b"DTM", b"N1", b"N2", b"N3", b"N4",
        b"PER", b"RDM", b"LX", b"TS3", b"TS2", b"CLP", b"NM1", b"MIA", b"MOA", b"AMT", b"QTY",
        b"SVC", b"CAS", b"LQ", b"PLB", b"SE", b"GE", b"IEA",
    ];
    for &(name, _, _) in SAMPLES {
        let bytes = common::load_sample(name);
        for segment in Tokenizer::new(&bytes).unwrap().filter(|s| !s.is_empty()) {
            assert!(
                KNOWN.contains(&segment.id),
                "{name}: unexpected segment id {:?}",
                segment.id
            );
        }
    }
}

#[test]
fn writer_reproduces_every_sample_segment_byte_for_byte() {
    for &(name, _, _) in SAMPLES {
        let bytes = common::load_sample(name);
        let tokenizer = Tokenizer::new(&bytes).unwrap();
        let delims = *tokenizer.delimiters();
        for segment in tokenizer.filter(|s| !s.is_empty()) {
            let mut written = Vec::new();
            segment
                .write_to(&delims, &mut written)
                .unwrap_or_else(|e| panic!("{name} segment {}: {e}", segment.index));
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
