use super::*;

// Verbatim ISA segments from the fixtures. Note the non-standard padding of
// ISA06/ISA08 in the trizetto one: it is 102 bytes long, not 106.
const ISA_5010: &[u8] =
    b"ISA*00*          *00*          *ZZ*EMEDNYBAT      *ZZ*ETIN           *100101*1000*^*00501*006000600*0*T*:~";
const ISA_4010_SHORT: &[u8] =
    b"ISA*00*          *00*          *ZZ*SENDER       *ZZ*RECEIVER     *240416*0930*U*00401*000001234*0*P*>~";

#[test]
fn from_isa_reads_all_four_delimiters_of_a_5010_file() {
    let d = Delimiters::from_isa(ISA_5010).unwrap();
    assert_eq!(d.element, b'*');
    assert_eq!(d.component, b':');
    assert_eq!(d.segment, b'~');
    assert_eq!(d.repetition, Some(b'^'));
    assert_eq!(d.release, None);
}

#[test]
fn from_isa_counts_separators_on_short_isa() {
    assert_eq!(
        ISA_4010_SHORT.len(),
        102,
        "fixture-derived ISA must be short"
    );
    let d = Delimiters::from_isa(ISA_4010_SHORT).unwrap();
    assert_eq!(d.component, b'>');
    assert_eq!(d.segment, b'~');
}

#[test]
fn isa11_is_not_a_repetition_separator_before_version_00402() {
    let d = Delimiters::from_isa(ISA_4010_SHORT).unwrap();
    assert_eq!(d.repetition, None);
}

#[test]
fn from_isa_ignores_bytes_after_the_terminator() {
    let mut input = ISA_5010.to_vec();
    input.extend_from_slice(b"GS*HP*X~");
    assert_eq!(Delimiters::from_isa(&input), Delimiters::from_isa(ISA_5010));
}

#[test]
fn from_isa_rejects_input_that_does_not_start_with_isa() {
    assert_eq!(
        Delimiters::from_isa(b"ST*835*1234~"),
        Err(IsaError::NotIsa {
            found: b"ST*835*1".to_vec(),
            byte_order_mark: false,
            whitespace: 0,
        })
    );
    assert_eq!(
        Delimiters::from_isa(b"GS"),
        Err(IsaError::NotIsa {
            found: b"GS".to_vec(),
            byte_order_mark: false,
            whitespace: 0,
        })
    );
    assert_eq!(
        Delimiters::from_isa(b""),
        Err(IsaError::NotIsa {
            found: Vec::new(),
            byte_order_mark: false,
            whitespace: 0,
        })
    );
}

#[test]
fn from_isa_reports_truncated_isa() {
    assert_eq!(
        Delimiters::from_isa(b"ISA"),
        Err(IsaError::Truncated {
            len: 3,
            separators_found: 0,
            byte_order_mark: false,
            whitespace: 0
        })
    );
    assert_eq!(
        Delimiters::from_isa(b"ISA*00*"),
        Err(IsaError::Truncated {
            len: 7,
            separators_found: 2,
            byte_order_mark: false,
            whitespace: 0
        })
    );
    let cut = &ISA_5010[..ISA_5010.len() - 1]; // terminator missing
    assert_eq!(
        Delimiters::from_isa(cut),
        Err(IsaError::Truncated {
            len: 105,
            separators_found: 16,
            byte_order_mark: false,
            whitespace: 0
        })
    );
    let cut = &ISA_5010[..ISA_5010.len() - 2]; // ISA16 and terminator missing
    assert_eq!(
        Delimiters::from_isa(cut),
        Err(IsaError::Truncated {
            len: 104,
            separators_found: 16,
            byte_order_mark: false,
            whitespace: 0
        })
    );
}

#[test]
fn builders_set_optional_delimiters() {
    let d = Delimiters::new(b'|', b':', b'~')
        .with_repetition(b'^')
        .with_release(b'?');
    assert_eq!(d.element, b'|');
    assert_eq!(d.repetition, Some(b'^'));
    assert_eq!(d.release, Some(b'?'));
}

#[test]
fn not_isa_displays_the_leading_bytes_in_hex() {
    assert_eq!(
        IsaError::NotIsa {
            found: b"\xEF\xBB\xBFISA*0".to_vec(),
            byte_order_mark: false,
            whitespace: 0,
        }
        .to_string(),
        "input does not start with an ISA segment (found bytes [ef bb bf 49 53 41 2a 30])"
    );
    assert_eq!(
        IsaError::NotIsa {
            found: Vec::new(),
            byte_order_mark: false,
            whitespace: 0,
        }
        .to_string(),
        "input does not start with an ISA segment (input is empty)"
    );
}

#[test]
fn not_isa_displays_what_was_skipped_before_the_found_bytes() {
    let not_isa = |found: &[u8], byte_order_mark, whitespace| IsaError::NotIsa {
        found: found.to_vec(),
        byte_order_mark,
        whitespace,
    };
    assert_eq!(
        not_isa(b"", true, 0).to_string(),
        "input does not start with an ISA segment (input holds only a UTF-8 byte order mark)"
    );
    assert_eq!(
        not_isa(b"", true, 1).to_string(),
        "input does not start with an ISA segment (input holds only a UTF-8 byte order mark and 1 byte of whitespace)"
    );
    assert_eq!(
        not_isa(b"", false, 4).to_string(),
        "input does not start with an ISA segment (input holds only 4 bytes of whitespace)"
    );
    assert_eq!(
        not_isa(b"GS*HP~", true, 0).to_string(),
        "input does not start with an ISA segment (found bytes [47 53 2a 48 50 7e] after skipping a UTF-8 byte order mark)"
    );
    assert_eq!(
        not_isa(b"GS", true, 2).to_string(),
        "input does not start with an ISA segment (found bytes [47 53] after skipping a UTF-8 byte order mark and 2 bytes of whitespace)"
    );
    assert_eq!(
        not_isa(b"GS", false, 1).to_string(),
        "input does not start with an ISA segment (found bytes [47 53] after skipping 1 byte of whitespace)"
    );
}

#[test]
fn from_isa_after_leading_trivia_names_the_skipped_bytes() {
    let read = |input: &[u8]| {
        Delimiters::from_isa_after_leading_trivia(input)
            .err()
            .map(|error| error.to_string())
    };
    assert_eq!(
        read(b"\xEF\xBB\xBF").as_deref(),
        Some("input does not start with an ISA segment (input holds only a UTF-8 byte order mark)")
    );
    assert_eq!(
        read(b" \r\n\t").as_deref(),
        Some("input does not start with an ISA segment (input holds only 4 bytes of whitespace)")
    );
    assert_eq!(
        read(b"\xEF\xBB\xBF\r\n").as_deref(),
        Some(
            "input does not start with an ISA segment (input holds only a UTF-8 byte order mark and 2 bytes of whitespace)"
        )
    );
    assert_eq!(
        read(b"\xEF\xBB\xBFGS*HP*X~").as_deref(),
        Some(
            "input does not start with an ISA segment (found bytes [47 53 2a 48 50 2a 58 7e] after skipping a UTF-8 byte order mark)"
        )
    );
    assert_eq!(
        read(b"").as_deref(),
        Some("input does not start with an ISA segment (input is empty)")
    );
    let mut input = b"\xEF\xBB\xBF\n".to_vec();
    input.extend_from_slice(ISA_5010);
    assert_eq!(
        Delimiters::from_isa_after_leading_trivia(&input),
        Delimiters::from_isa(ISA_5010)
    );
    assert_eq!(
        Delimiters::from_isa_after_leading_trivia(b"\nISA*00*"),
        Err(IsaError::Truncated {
            len: 7,
            separators_found: 2,
            byte_order_mark: false,
            whitespace: 1
        })
    );
    assert_eq!(
        Delimiters::from_isa_after_leading_trivia(b"\xEF\xBB\xBF ISA*00*"),
        Err(IsaError::Truncated {
            len: 7,
            separators_found: 2,
            byte_order_mark: true,
            whitespace: 1
        })
    );
}

#[test]
fn truncated_displays_the_skipped_leading_bytes() {
    let message = |input: &[u8]| {
        Delimiters::from_isa_after_leading_trivia(input)
            .err()
            .map(|error| error.to_string())
    };
    assert_eq!(
        message(b"\xEF\xBB\xBFISA*00*").as_deref(),
        Some(
            "ISA segment truncated after 7 bytes (counted after skipping a UTF-8 byte order mark): found 2 of 16 element separators"
        )
    );
    assert_eq!(
        message(b"\n\n ISA*00*").as_deref(),
        Some(
            "ISA segment truncated after 7 bytes (counted after skipping 3 bytes of whitespace): found 2 of 16 element separators"
        )
    );
    assert_eq!(
        message(b" ISA*00*").as_deref(),
        Some(
            "ISA segment truncated after 7 bytes (counted after skipping 1 byte of whitespace): found 2 of 16 element separators"
        )
    );
    assert_eq!(
        message(b"\xEF\xBB\xBF\r\nISA").as_deref(),
        Some(
            "ISA segment truncated after 3 bytes (counted after skipping a UTF-8 byte order mark and 2 bytes of whitespace): found 0 of 16 element separators"
        )
    );
    let mut cut = b"\xEF\xBB\xBF".to_vec();
    cut.extend_from_slice(&ISA_5010[..ISA_5010.len() - 1]);
    assert_eq!(
        message(&cut).as_deref(),
        Some(
            "ISA segment truncated after 105 bytes (counted after skipping a UTF-8 byte order mark): ISA16 or the terminator is missing"
        )
    );
}

#[test]
fn truncated_displays_how_many_separators_were_found() {
    assert_eq!(
        IsaError::Truncated {
            len: 7,
            separators_found: 2,
            byte_order_mark: false,
            whitespace: 0
        }
        .to_string(),
        "ISA segment truncated after 7 bytes: found 2 of 16 element separators"
    );
    assert_eq!(
        IsaError::Truncated {
            len: 105,
            separators_found: 16,
            byte_order_mark: false,
            whitespace: 0
        }
        .to_string(),
        "ISA segment truncated after 105 bytes: ISA16 or the terminator is missing"
    );
}

#[test]
fn is_special_covers_the_delimiters_the_tokenizer_splits_on() {
    let d = Delimiters::new(b'*', b':', b'~')
        .with_repetition(b'^')
        .with_release(b'?');
    for &byte in b"*:~?" {
        assert!(d.is_special(byte), "{}", byte as char);
    }
    assert!(!d.is_special(b'A'));
    assert!(
        !d.is_special(b'^'),
        "repetition is not split, so it is data"
    );
}

#[test]
fn isa_datum_of_not_isa() {
    let input = b"hello, this is not an interchange";
    let Err(error) = Delimiters::from_isa(input) else {
        panic!("the input has no ISA");
    };
    assert_eq!(error.datum(input), b"hello, t");
    assert_eq!(error.skipped(), 0);
}

#[test]
fn isa_datum_of_a_truncated_isa_after_a_bom() {
    let input = b"\xEF\xBB\xBF  ISA*00*";
    let Err(error) = Delimiters::from_isa_after_leading_trivia(input) else {
        panic!("the input is a truncated ISA");
    };
    assert!(
        matches!(
            &error,
            IsaError::Truncated {
                byte_order_mark: true,
                whitespace: 2,
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.skipped(), 5);
    assert_eq!(error.datum(input), b"ISA*00*");
}

#[test]
fn isa_datum_of_a_truncated_isa_keeps_at_most_the_nominal_length() {
    let mut input = b"ISA*".to_vec();
    input.resize(IsaError::ISA_LEN + 10, b'x');
    let Err(error) = Delimiters::from_isa(&input) else {
        panic!("the input has no 16 separators");
    };
    assert_eq!(error.datum(&input), &input[..IsaError::ISA_LEN]);
}

#[test]
fn isa_datum_of_input_shorter_than_the_skipped_bytes_is_empty() {
    let error = IsaError::Truncated {
        len: 0,
        separators_found: 0,
        byte_order_mark: true,
        whitespace: 4,
    };
    assert_eq!(error.datum(b"\xEF"), b"");
}
