//! A diagnostic as flat fields, the shape every binding's table or class
//! of diagnostics shares.

use std::fmt::Write;

use super::{Diagnostic, Rule, SnipLevel};

/// Who reports the parser's own diagnostics.
pub const PARSER_ORIGIN: &str = "oxedi";

impl SnipLevel {
    /// The level as a number: 1, 2 or 3.
    pub fn number(self) -> u8 {
        match self {
            SnipLevel::L1 => 1,
            SnipLevel::L2 => 2,
            SnipLevel::L3 => 3,
        }
    }
}

/// A diagnostic as ten flat fields, borrowed from it; the columns a table of
/// diagnostics has, in [`FlatDiagnostic::FIELDS`] order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatDiagnostic<'a> {
    /// The SNIP level: 1, 2 or 3.
    pub level: u8,
    /// The name of the rule's variant.
    pub kind: &'static str,
    /// The rule as a sentence with its values.
    pub rule: String,
    /// 0-based index of the segment at fault; `None` at the end of the stream.
    pub segment: Option<usize>,
    /// 1-based element position, when the diagnostic has one.
    pub element: Option<usize>,
    /// 1-based component position, when the diagnostic has one.
    pub component: Option<usize>,
    /// The open loops joined by `/`, e.g. `2000#1/2100#3`; empty at the root.
    pub path: String,
    /// The offending value as it appears in the file.
    pub datum: &'a [u8],
    /// [`PARSER_ORIGIN`] for the parser's own rules, the validator's name for
    /// an external diagnostic.
    pub origin: &'a str,
    /// The external validator's own code, when it gives one.
    pub code: Option<&'a str>,
}

impl FlatDiagnostic<'_> {
    /// The field names, in the order of the struct.
    pub const FIELDS: [&'static str; 10] = [
        "level",
        "kind",
        "rule",
        "segment",
        "element",
        "component",
        "path",
        "datum",
        "origin",
        "code",
    ];
}

impl Diagnostic {
    /// Who reported the diagnostic: [`PARSER_ORIGIN`] for the parser's own
    /// rules, the validator's name for an external one.
    pub fn origin(&self) -> &str {
        match &self.rule {
            Rule::External { origin, .. } => origin,
            _ => PARSER_ORIGIN,
        }
    }

    /// The external validator's own code, when the diagnostic has one.
    pub fn code(&self) -> Option<&str> {
        match &self.rule {
            Rule::External { code, .. } => code.as_deref(),
            _ => None,
        }
    }

    /// The open loops joined by `/`, outermost first, e.g. `2000#1/2100#3`;
    /// empty at the root.
    pub fn path_text(&self) -> String {
        let mut text = String::new();
        for (i, open) in self.path.iter().enumerate() {
            let separator = if i == 0 { "" } else { "/" };
            // Writing to a `String` cannot fail.
            let _ = write!(text, "{separator}{open}");
        }
        text
    }

    /// The diagnostic as flat fields.
    pub fn flat(&self) -> FlatDiagnostic<'_> {
        FlatDiagnostic {
            level: self.level.number(),
            kind: self.rule.kind(),
            rule: self.rule.to_string(),
            segment: self.segment,
            element: self.element,
            component: self.component,
            path: self.path_text(),
            datum: &self.datum,
            origin: self.origin(),
            code: self.code(),
        }
    }
}
