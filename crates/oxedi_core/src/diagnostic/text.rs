//! The pieces the rule messages are written from: an element reference, the
//! segment that opened a loop, lists cut at a limit, counts and amounts, and
//! who reported an external finding. Each is a small value with its own
//! `Display`, so a message is one `write!` over them.

use std::fmt;

use super::Quoted;
use crate::value::decimal_display;

/// The longest code or name list a message spells out; longer lists keep
/// the count.
pub(super) const MAX_LISTED_CODES: usize = 5;

/// The most segment indexes a balancing message spells out; a longer list
/// ends with the count of the rest.
pub(super) const MAX_LISTED_SEGMENTS: usize = 10;

/// An element reference in X12 style: `CLP01`, or `SVC01-2` for a component.
pub(super) struct ElementRef<'a> {
    segment_id: &'a [u8],
    element: usize,
    component: Option<usize>,
}

impl<'a> ElementRef<'a> {
    /// A simple element, or a component when `component` is `Some`.
    pub(super) fn new(segment_id: &'a [u8], element: usize, component: Option<usize>) -> Self {
        ElementRef {
            segment_id,
            element,
            component,
        }
    }

    /// A simple element.
    pub(super) fn simple(segment_id: &'a [u8], element: usize) -> Self {
        ElementRef::new(segment_id, element, None)
    }
}

impl fmt::Display for ElementRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{:02}",
            String::from_utf8_lossy(self.segment_id),
            self.element
        )?;
        match self.component {
            Some(component) => write!(f, "-{component}"),
            None => Ok(()),
        }
    }
}

/// The segment that opened a loop, before the rest of the sentence:
/// `opened at segment #2 `, or nothing when the loop opened implicitly.
pub(super) struct OpenedAt(pub(super) Option<usize>);

impl fmt::Display for OpenedAt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(index) => write!(f, "opened at segment #{index} "),
            None => Ok(()),
        }
    }
}

/// The segment that opened a loop, at the end of a sentence:
/// ` of segment #2`, or nothing when the loop opened implicitly.
pub(super) struct OfSegment(pub(super) Option<usize>);

impl fmt::Display for OfSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(index) => write!(f, " of segment #{index}"),
            None => Ok(()),
        }
    }
}

/// How a loop opened: `opened at segment #2` or `opened implicitly`.
pub(super) struct Opener(pub(super) Option<usize>);

impl fmt::Display for Opener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(index) => write!(f, "opened at segment #{index}"),
            None => write!(f, "opened implicitly"),
        }
    }
}

/// The first `limit` items, separated by commas, then ` and N more` for the
/// items past the limit.
pub(super) struct Cut<'a, T> {
    pub(super) items: &'a [T],
    pub(super) limit: usize,
    pub(super) item: fn(&T, &mut fmt::Formatter<'_>) -> fmt::Result,
}

impl<T> fmt::Display for Cut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, item) in self.items.iter().take(self.limit).enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            (self.item)(item, f)?;
        }
        match self.items.len().saturating_sub(self.limit) {
            0 => Ok(()),
            rest => write!(f, " and {rest} more"),
        }
    }
}

/// A short list in parentheses after a space, ` ("A", "B")`; a list longer
/// than [`MAX_LISTED_CODES`] is left out, as its count already says enough.
pub(super) struct ShortList<'a> {
    pub(super) items: &'a [String],
    pub(super) item: fn(&String, &mut fmt::Formatter<'_>) -> fmt::Result,
}

impl fmt::Display for ShortList<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.items.len() > MAX_LISTED_CODES {
            return Ok(());
        }
        let all = Cut {
            items: self.items,
            limit: MAX_LISTED_CODES,
            item: self.item,
        };
        write!(f, " ({all})")
    }
}

/// A code as the file would hold it, quoted and escaped.
pub(super) fn quoted_code(code: &String, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", Quoted(code.as_bytes()))
}

/// A name from the spec, as a Rust string literal.
pub(super) fn debug_name(name: &String, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{name:?}")
}

/// A segment index, `#7`.
pub(super) fn segment_index(index: &usize, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "#{index}")
}

/// How many codes a list holds: `the one code` or `one of the 3 codes`.
pub(super) struct CodeCount(pub(super) usize);

impl fmt::Display for CodeCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            1 => write!(f, "the one code"),
            count => write!(f, "one of the {count} codes"),
        }
    }
}

/// The occurrence whose own code list applies, ` in occurrence "name"`, or
/// nothing for the element's list.
pub(super) struct InOccurrence<'a>(pub(super) Option<&'a String>);

impl fmt::Display for InOccurrence<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(occurrence) => write!(f, " in occurrence {occurrence:?}"),
            None => Ok(()),
        }
    }
}

/// Where a count of loop instances is taken: under one instance of the
/// parent loop, or at the root.
pub(super) struct Parent<'a>(pub(super) Option<&'a String>);

impl fmt::Display for Parent<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(parent) => write!(f, "under one instance of loop {parent:?}"),
            None => write!(f, "at the root"),
        }
    }
}

/// How far a balance is off, ` (off by 312.50)`, or nothing when the
/// difference does not fit an `i128`.
pub(super) struct OffBy {
    pub(super) expected: i128,
    pub(super) computed: i128,
    pub(super) scale: u8,
}

impl fmt::Display for OffBy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.expected.checked_sub(self.computed) {
            Some(difference) => write!(f, " (off by {})", decimal_display(difference, self.scale)),
            None => Ok(()),
        }
    }
}

/// Who reported an external finding: `(reported by pyx12, code E1)`, or
/// without the code when there is none.
pub(super) struct Reporter<'a> {
    pub(super) origin: &'a str,
    pub(super) code: Option<&'a str>,
}

impl fmt::Display for Reporter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.code {
            Some(code) => write!(f, "(reported by {}, code {code})", self.origin),
            None => write!(f, "(reported by {})", self.origin),
        }
    }
}
