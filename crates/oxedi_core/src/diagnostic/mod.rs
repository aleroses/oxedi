//! Findings about a file's data, each one readable on its own.
//!
//! A diagnostic names the rule that failed, where it failed (segment index,
//! element and component position, and the open loops with the ordinal of
//! each instance) and the offending value as it appears in the file. It holds
//! owned values only, so it can be printed, stored or sent elsewhere without
//! the spec or the document that produced it.
//!
//! The folder holds: this file (the loop and diagnostic types, the SNIP
//! levels and the byte quoting helper), `flat.rs` (the flat record of a
//! diagnostic), `rule.rs` (the rules, their levels
//! and variant names), `display.rs` (the message text of each rule), `text.rs`
//! (the pieces those messages are written from) and `tests.rs`.

use std::fmt;

use crate::document::{Document, Span};
use crate::spec::render_key;

pub use flat::{FlatDiagnostic, PARSER_ORIGIN};
pub use rule::Rule;

/// The SNIP validation level a rule belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SnipLevel {
    /// Integrity: envelopes, control numbers and counts, segment structure.
    L1,
    /// Requirements: required elements, types and lengths.
    L2,
    /// Balancing: amounts that must add up.
    L3,
}

impl fmt::Display for SnipLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SNIP {}", self.number())
    }
}

/// One open loop instance: the loop's name and the 1-based ordinal of the
/// instance among every instance of that loop in the stream.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LoopRef {
    /// Loop name as the spec writes it, e.g. `2100`.
    pub name: String,
    /// 1 for the first instance of the loop, 2 for the second, and so on.
    pub ordinal: usize,
}

impl fmt::Display for LoopRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", render_key(&self.name), self.ordinal)
    }
}

/// Bytes from the file, quoted on one line: valid text is escaped as a Rust
/// string literal is, and each invalid byte is written as `\xNN`.
pub(crate) struct Quoted<'a>(pub(crate) &'a [u8]);

impl fmt::Display for Quoted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("\"")?;
        for chunk in self.0.utf8_chunks() {
            let escaped = format!("{:?}", chunk.valid());
            let inner = escaped
                .strip_prefix('"')
                .and_then(|text| text.strip_suffix('"'))
                .unwrap_or(&escaped);
            f.write_str(inner)?;
            for byte in chunk.invalid() {
                write!(f, "\\x{byte:02X}")?;
            }
        }
        f.write_str("\"")
    }
}

/// One finding about the data of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// What failed.
    pub rule: Rule,
    /// The SNIP level of `rule`.
    pub level: SnipLevel,
    /// Index of the segment at fault; `None` when the finding is about the
    /// end of the stream.
    pub segment: Option<usize>,
    /// 1-based element position inside the segment, when the finding is.
    pub element: Option<usize>,
    /// 1-based component position inside the element, when the finding is.
    pub component: Option<usize>,
    /// Open loops at the time, outermost first.
    pub path: Vec<LoopRef>,
    /// The offending value as it appears in the file.
    pub datum: Vec<u8>,
}

impl Diagnostic {
    /// A diagnostic whose level is the rule's own.
    pub fn new(
        rule: Rule,
        segment: Option<usize>,
        element: Option<usize>,
        component: Option<usize>,
        path: Vec<LoopRef>,
        datum: Vec<u8>,
    ) -> Diagnostic {
        Diagnostic {
            level: rule.level(),
            rule,
            segment,
            element,
            component,
            path,
            datum,
        }
    }

    /// Where the segment at fault lives in `document`; `None` when the
    /// diagnostic names no segment or the document has no such segment.
    pub fn span(&self, document: &Document<'_>) -> Option<Span> {
        self.segment.and_then(|index| document.span(index))
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} · {} · ", self.level, self.rule)?;
        match self.segment {
            Some(segment) => write!(f, "segment #{segment}")?,
            None => write!(f, "end of stream")?,
        }
        if let Some(element) = self.element {
            write!(f, ", element {element}")?;
            if let Some(component) = self.component {
                write!(f, ", component {component}")?;
            }
        }
        write!(f, " · at ")?;
        if self.path.is_empty() {
            write!(f, "the root")?;
        }
        for (i, open) in self.path.iter().enumerate() {
            if i > 0 {
                write!(f, "/")?;
            }
            write!(f, "{open}")?;
        }
        write!(f, " · datum {}", Quoted(&self.datum))
    }
}

mod display;
mod flat;
mod rule;
mod text;

#[cfg(test)]
mod tests;
