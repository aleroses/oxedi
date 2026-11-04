use super::*;
use crate::Delimiters;
use crate::delimiters::test_support::plain;

fn with_release() -> Delimiters {
    plain().with_release(b'?')
}

#[test]
fn splits_at_the_terminator() {
    let (frame, rest) = next_frame(b"ST*835~BPR~", &plain()).unwrap();
    assert_eq!(frame.raw, b"ST*835~");
    assert_eq!(frame.body, b"ST*835");
    assert!(frame.terminated);
    assert_eq!(rest, b"BPR~");
}

#[test]
fn leading_trivia_belongs_to_raw_not_body() {
    let (frame, rest) = next_frame(b"\r\n GS*HP~", &plain()).unwrap();
    assert_eq!(frame.raw, b"\r\n GS*HP~");
    assert_eq!(frame.body, b"GS*HP");
    assert_eq!(rest, b"");
}

#[test]
fn unterminated_tail_is_a_frame() {
    let (frame, rest) = next_frame(b"SE*5", &plain()).unwrap();
    assert_eq!(frame.raw, b"SE*5");
    assert_eq!(frame.body, b"SE*5");
    assert!(!frame.terminated);
    assert_eq!(rest, b"");
}

#[test]
fn trivia_only_tail_is_a_frame_with_empty_body() {
    let (frame, rest) = next_frame(b"\n", &plain()).unwrap();
    assert_eq!(frame.raw, b"\n");
    assert_eq!(frame.body, b"");
    assert!(!frame.terminated);
    assert_eq!(rest, b"");
}

#[test]
fn empty_segment_is_a_frame() {
    let (frame, rest) = next_frame(b"~SE~", &plain()).unwrap();
    assert_eq!(frame.raw, b"~");
    assert_eq!(frame.body, b"");
    assert!(frame.terminated);
    assert_eq!(rest, b"SE~");
}

#[test]
fn release_makes_the_terminator_literal() {
    let (frame, rest) = next_frame(b"N1*A?~B~X~", &with_release()).unwrap();
    assert_eq!(frame.body, b"N1*A?~B");
    assert_eq!(rest, b"X~");
}

#[test]
fn escaped_release_does_not_escape_the_terminator() {
    let (frame, _) = next_frame(b"N1*A??~B~", &with_release()).unwrap();
    assert_eq!(frame.body, b"N1*A??");
}

#[test]
fn dangling_release_at_end_does_not_panic() {
    let (frame, rest) = next_frame(b"N1*A?", &with_release()).unwrap();
    assert_eq!(frame.body, b"N1*A?");
    assert!(!frame.terminated);
    assert_eq!(rest, b"");
}

#[test]
fn a_leading_byte_order_mark_is_trivia_of_the_first_frame() {
    let (frame, rest) = first_frame(b"\xEF\xBB\xBF\r\nISA*00~GS~", &plain()).unwrap();
    assert_eq!(frame.raw, b"\xEF\xBB\xBF\r\nISA*00~");
    assert_eq!(frame.body, b"ISA*00");
    assert!(frame.terminated);
    assert_eq!(rest, b"GS~");
}

#[test]
fn a_byte_order_mark_alone_is_a_trivia_only_frame() {
    let (frame, rest) = first_frame(b"\xEF\xBB\xBF", &plain()).unwrap();
    assert_eq!(frame.raw, b"\xEF\xBB\xBF");
    assert_eq!(frame.body, b"");
    assert!(!frame.terminated);
    assert_eq!(rest, b"");
}

#[test]
fn without_a_byte_order_mark_the_first_frame_is_the_next_frame() {
    let input = b"\nST*835~SE~";
    assert_eq!(first_frame(input, &plain()), next_frame(input, &plain()));
    assert_eq!(first_frame(b"", &plain()), None);
}

#[test]
fn a_byte_order_mark_after_the_first_frame_is_data() {
    let (frame, _) = next_frame(b"\xEF\xBB\xBFST~", &plain()).unwrap();
    assert_eq!(frame.body, b"\xEF\xBB\xBFST");
    let (frame, _) = first_frame(b"\xEF\xBB\xBF\xEF\xBB\xBFST~", &plain()).unwrap();
    assert_eq!(frame.body, b"\xEF\xBB\xBFST");
}

#[test]
fn empty_input_yields_no_frame() {
    assert_eq!(next_frame(b"", &plain()), None);
}

#[test]
fn find_unescaped_without_release_is_plain_search() {
    assert_eq!(find_unescaped(b"ab~c", b'~', None), Some(2));
    assert_eq!(find_unescaped(b"abc", b'~', None), None);
}

#[test]
fn find_unescaped_skips_the_byte_after_a_release() {
    assert_eq!(find_unescaped(b"a?~b~", b'~', Some(b'?')), Some(4));
    assert_eq!(find_unescaped(b"a?", b'~', Some(b'?')), None);
}
