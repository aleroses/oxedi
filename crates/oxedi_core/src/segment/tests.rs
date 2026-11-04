use super::*;
use crate::{Delimiters, Frame};
use std::borrow::Cow;

fn frame(body: &[u8]) -> Frame<'_> {
    Frame {
        raw: body,
        body,
        terminated: true,
    }
}

#[test]
fn parse_splits_id_and_elements() {
    let delims = Delimiters::new(b'*', b':', b'~');
    let segment = Segment::parse(7, frame(b"ST*835*1234"), &delims);
    assert_eq!(segment.index, 7);
    assert_eq!(segment.id, b"ST");
    assert_eq!(
        segment.elements,
        vec![
            Element::Simple(Cow::Borrowed(b"835")),
            Element::Simple(Cow::Borrowed(b"1234"))
        ]
    );
    assert!(segment.terminated);
}

#[test]
fn parse_of_empty_body_is_an_empty_segment() {
    let delims = Delimiters::new(b'*', b':', b'~');
    let segment = Segment::parse(0, frame(b""), &delims);
    assert_eq!(segment.id, b"");
    assert!(segment.elements.is_empty());
    assert!(segment.is_empty());
}

#[test]
fn parse_keeps_trailing_empty_elements() {
    let delims = Delimiters::new(b'*', b':', b'~');
    let segment = Segment::parse(0, frame(b"BPR*I**C"), &delims);
    assert_eq!(segment.elements.len(), 3);
    assert_eq!(segment.element(2).and_then(Element::simple), Some(&b""[..]));
}

#[test]
fn element_is_one_based_like_x12() {
    let delims = Delimiters::new(b'*', b':', b'~');
    let segment = Segment::parse(0, frame(b"CLP*123*1"), &delims);
    assert_eq!(
        segment.element(1).and_then(Element::simple),
        Some(&b"123"[..])
    );
    assert_eq!(
        segment.element(2).and_then(Element::simple),
        Some(&b"1"[..])
    );
    assert_eq!(segment.element(0), None);
    assert_eq!(segment.element(3), None);
}

#[test]
fn component_is_one_based_and_component_one_of_a_simple_element_is_the_element() {
    let delims = Delimiters::new(b'*', b':', b'~');
    let segment = Segment::parse(0, frame(b"XX*a*b:c"), &delims);
    assert_eq!(segment.component(1, 1), Some(&b"a"[..]));
    assert_eq!(segment.component(1, 2), None);
    assert_eq!(segment.component(2, 2), Some(&b"c"[..]));
    assert_eq!(segment.component(2, 3), None);
    assert_eq!(segment.component(2, 0), None);
    assert_eq!(segment.component(0, 1), None);
    assert_eq!(segment.component(3, 1), None);
}

fn written(segment: &Segment<'_>, delims: &Delimiters) -> Result<Vec<u8>, SegmentWriteError> {
    let mut out = Vec::new();
    segment.write_to(delims, &mut out)?;
    Ok(out)
}

#[test]
fn write_to_rebuilds_id_elements_and_terminator() {
    let delims = Delimiters::new(b'*', b':', b'~');
    let segment = Segment::parse(0, frame(b"SVC*HC:99213*100**12"), &delims);
    assert_eq!(
        written(&segment, &delims).unwrap(),
        b"SVC*HC:99213*100**12~"
    );
}

#[test]
fn write_to_escapes_delimiters_inside_values_when_release_is_set() {
    let delims = Delimiters::new(b'*', b':', b'~').with_release(b'?');
    let segment = Segment {
        index: 0,
        raw: b"",
        id: b"N1",
        elements: vec![Element::Simple(Cow::Owned(b"A*B~C?D".to_vec()))],
        terminated: true,
    };
    assert_eq!(written(&segment, &delims).unwrap(), b"N1*A?*B?~C??D~");
}

#[test]
fn write_to_without_release_rejects_a_delimiter_in_a_value() {
    let delims = Delimiters::new(b'*', b':', b'~');
    let segment = Segment {
        index: 0,
        raw: b"",
        id: b"N1",
        elements: vec![Element::Simple(Cow::Borrowed(b"A*B"))],
        terminated: true,
    };
    assert!(matches!(
        written(&segment, &delims),
        Err(SegmentWriteError::DelimiterInValue {
            byte: b'*',
            element: 1,
            component: None
        })
    ));
}

#[test]
fn write_to_reports_the_element_and_component_holding_the_delimiter() {
    let delims = Delimiters::new(b'*', b':', b'~');
    let segment = Segment {
        index: 0,
        raw: b"",
        id: b"SVC",
        elements: vec![
            Element::Simple(Cow::Borrowed(b"1")),
            Element::Simple(Cow::Borrowed(b"2")),
            Element::Composite(vec![Cow::Borrowed(b"HC"), Cow::Borrowed(b"A~B")]),
        ],
        terminated: true,
    };
    assert!(matches!(
        written(&segment, &delims),
        Err(SegmentWriteError::DelimiterInValue {
            byte: b'~',
            element: 3,
            component: Some(2)
        })
    ));
}

#[test]
fn write_to_does_not_write_trivia_from_raw() {
    let delims = Delimiters::new(b'*', b':', b'~');
    let segment = Segment::parse(
        0,
        Frame {
            raw: b"\nSE*2*1~",
            body: b"SE*2*1",
            terminated: true,
        },
        &delims,
    );
    assert_eq!(written(&segment, &delims).unwrap(), b"SE*2*1~");
}

#[test]
fn delimiter_in_value_displays_where_the_byte_is() {
    assert_eq!(
        SegmentWriteError::DelimiterInValue {
            byte: b'*',
            element: 3,
            component: Some(2)
        }
        .to_string(),
        "element 3 component 2 contains delimiter byte 0x2A and no release byte is configured"
    );
    assert_eq!(
        SegmentWriteError::DelimiterInValue {
            byte: b'*',
            element: 1,
            component: None
        }
        .to_string(),
        "element 1 contains delimiter byte 0x2A and no release byte is configured"
    );
    assert_eq!(
        SegmentWriteError::DelimiterInValue {
            byte: b'*',
            element: 0,
            component: None
        }
        .to_string(),
        "segment id contains delimiter byte 0x2A and no release byte is configured"
    );
}

#[test]
fn io_error_displays_the_sink_failure() {
    let err = SegmentWriteError::Io(io::Error::other("disk full"));
    assert_eq!(err.to_string(), "write failed: disk full");
    assert!(std::error::Error::source(&err).is_none());
}

#[test]
fn write_to_rejects_an_id_containing_a_delimiter_or_release_byte() {
    let delims = Delimiters::new(b'*', b':', b'~');
    for (id, byte) in [(&b"A*B"[..], b'*'), (b"A~B", b'~'), (b"A:B", b':')] {
        let segment = Segment {
            index: 0,
            raw: b"",
            id,
            elements: vec![],
            terminated: true,
        };
        assert!(
            matches!(written(&segment, &delims), Err(SegmentWriteError::DelimiterInValue { byte: b, element: 0, component: None }) if b == byte),
            "id {:?}",
            id
        );
    }
    let with_release = delims.with_release(b'?');
    let segment = Segment {
        index: 0,
        raw: b"",
        id: b"AB?",
        elements: vec![],
        terminated: true,
    };
    assert!(matches!(
        written(&segment, &with_release),
        Err(SegmentWriteError::DelimiterInValue {
            byte: b'?',
            element: 0,
            component: None
        })
    ));
}

#[test]
fn reparse_into_one_buffer_equals_a_fresh_parse_of_each_frame() {
    let delims = Delimiters::new(b'*', b':', b'~').with_release(b'?');
    // Long, then short, then composite, then simple at the composite's
    // position, then a composite with fewer components after a longer one,
    // an escaped value, an empty frame, and an unterminated last frame.
    let bodies: [&[u8]; 9] = [
        b"CLP*1*2*100*80**12*CLM*11*1*X*Y*Z",
        b"LX*1",
        b"SVC*HC:99213:25:59*100*80**1*HC:99214",
        b"SVC*99213*100",
        b"SVC*HC:99213:25:59:XU*1",
        b"SVC*AD:1",
        b"NM1*QC*1*O?*NEIL*A?:B",
        b"",
        b"SE*3*0001",
    ];
    let mut buffer = Segment::empty();
    for (index, body) in bodies.iter().enumerate() {
        let frame = Frame {
            raw: body,
            body,
            terminated: index + 1 < bodies.len(),
        };
        buffer.reparse(index, frame, &delims);
        assert_eq!(
            buffer,
            Segment::parse(index, frame, &delims),
            "frame {index}"
        );
    }
}

#[test]
fn reparse_overwrites_a_composite_slot_with_a_simple_value_and_back() {
    let delims = Delimiters::new(b'*', b':', b'~');
    let mut buffer = Segment::empty();
    buffer.reparse(0, frame(b"SVC*HC:1:2:3"), &delims);
    buffer.reparse(1, frame(b"SVC*HC"), &delims);
    assert_eq!(buffer.elements, vec![Element::Simple(Cow::Borrowed(b"HC"))]);
    buffer.reparse(2, frame(b"SVC*AD:9"), &delims);
    assert_eq!(
        buffer.elements,
        vec![Element::Composite(vec![
            Cow::Borrowed(&b"AD"[..]),
            Cow::Borrowed(&b"9"[..])
        ])]
    );
}

proptest::proptest! {
    #[test]
    fn reparse_of_any_frame_sequence_equals_fresh_parses(
        bodies in proptest::collection::vec(
            proptest::collection::vec(proptest::sample::select(b"AB1*:?".to_vec()), 0..24),
            1..12,
        ),
        release in proptest::bool::ANY,
    ) {
        let delims = if release {
            Delimiters::new(b'*', b':', b'~').with_release(b'?')
        } else {
            Delimiters::new(b'*', b':', b'~')
        };
        let mut buffer = Segment::empty();
        for (index, body) in bodies.iter().enumerate() {
            buffer.reparse(index, frame(body), &delims);
            proptest::prop_assert_eq!(&buffer, &Segment::parse(index, frame(body), &delims));
        }
    }
}

#[test]
fn leaf_reads_an_element_or_a_component_and_a_composite_at_its_first_component() {
    let delims = Delimiters::new(b'*', b':', b'~');
    let segment = Segment::parse(0, frame(b"SV*HC:99213:*A**"), &delims);
    assert_eq!(segment.leaf(1, None), Some(&b"HC"[..]));
    assert_eq!(segment.leaf(1, Some(1)), Some(&b"HC"[..]));
    assert_eq!(segment.leaf(1, Some(2)), Some(&b"99213"[..]));
    assert_eq!(segment.leaf(1, Some(3)), Some(&b""[..]));
    assert_eq!(segment.leaf(1, Some(4)), None);
    assert_eq!(segment.leaf(2, None), Some(&b"A"[..]));
    assert_eq!(segment.leaf(2, Some(1)), Some(&b"A"[..]));
    assert_eq!(segment.leaf(2, Some(2)), None);
    assert_eq!(segment.leaf(4, None), Some(&b""[..]));
    assert_eq!(segment.leaf(5, None), None);
    assert_eq!(segment.leaf(0, None), None);
}

#[test]
fn text_joins_a_composite_and_conditions_hold_on_simple_elements_only() {
    let delims = Delimiters::new(b'*', b':', b'~');
    let segment = Segment::parse(0, frame(b"SV*HC:99213*A"), &delims);
    let mut joined = Vec::new();
    assert_eq!(segment.text(1, b':', &mut joined), Some(&b"HC:99213"[..]));
    assert_eq!(segment.text(2, b':', &mut joined), Some(&b"A"[..]));
    assert_eq!(segment.text(3, b':', &mut joined), None);
    assert!(segment.holds(&[(2, b"A".to_vec())]));
    assert!(segment.holds(&[]));
    assert!(!segment.holds(&[(1, b"HC".to_vec())]));
    assert!(!segment.holds(&[(3, b"".to_vec())]));
}
