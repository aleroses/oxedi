use super::*;

#[test]
fn leading_trivia_splits_a_mark_and_whitespace() {
    let trivia = LeadingTrivia::of(b"\xEF\xBB\xBF \r\nISA");
    assert_eq!(
        trivia,
        LeadingTrivia {
            byte_order_mark: true,
            whitespace: 3
        }
    );
    assert_eq!(trivia.len(), 6);
}

#[test]
fn leading_trivia_without_a_mark() {
    let crlf = LeadingTrivia::of(b"\r\nISA");
    assert_eq!(
        crlf,
        LeadingTrivia {
            byte_order_mark: false,
            whitespace: 2
        }
    );
    assert_eq!(crlf.len(), 2);
    let none = LeadingTrivia::of(b"ISA");
    assert_eq!(
        none,
        LeadingTrivia {
            byte_order_mark: false,
            whitespace: 0
        }
    );
    assert_eq!(none.len(), 0);
    assert!(none.is_empty());
    assert!(!crlf.is_empty());
}

#[test]
fn a_mark_after_whitespace_is_data() {
    let trivia = LeadingTrivia::of(b" \xEF\xBB\xBFISA");
    assert_eq!(
        trivia,
        LeadingTrivia {
            byte_order_mark: false,
            whitespace: 1
        }
    );
    assert_eq!(trivia.len(), 1);
}

#[test]
fn only_trivia() {
    let mark = LeadingTrivia::of(b"\xEF\xBB\xBF");
    assert_eq!(
        mark,
        LeadingTrivia {
            byte_order_mark: true,
            whitespace: 0
        }
    );
    assert_eq!(mark.len(), 3);
    let empty = LeadingTrivia::of(b"");
    assert_eq!(
        empty,
        LeadingTrivia {
            byte_order_mark: false,
            whitespace: 0
        }
    );
    assert_eq!(empty.len(), 0);
    let both = LeadingTrivia::of(b"\xEF\xBB\xBF \r\n");
    assert_eq!(
        both,
        LeadingTrivia {
            byte_order_mark: true,
            whitespace: 3
        }
    );
    assert_eq!(both.len(), 6);
}

#[test]
fn len_saturates_instead_of_wrapping() {
    let trivia = LeadingTrivia {
        byte_order_mark: true,
        whitespace: usize::MAX,
    };
    assert_eq!(trivia.len(), usize::MAX);
}
