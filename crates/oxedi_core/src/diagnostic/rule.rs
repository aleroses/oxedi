//! The rules a diagnostic reports, their SNIP levels and variant names.

use crate::value::ElementType;

use super::SnipLevel;

/// The rule a diagnostic reports, with the values its message needs.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Rule {
    /// The input starts with a UTF-8 byte order mark, read as leading trivia
    /// of the first segment. Informational: nothing else changes.
    ByteOrderMark,
    /// No open loop holds the segment and it opens no loop.
    UnknownSegment {
        /// The segment id.
        segment_id: Vec<u8>,
    },
    /// A loop was opened without its own trigger, to hold a descendant.
    ImplicitLoop {
        /// The loop that was opened.
        loop_name: String,
        /// The loop's own trigger as the spec writes it, e.g.
        /// `"GS" with no conditions` or `"N1" where {1: "PR"}`.
        expected_trigger: String,
        /// Id of the segment whose loop needed it.
        caused_by: Vec<u8>,
    },
    /// A loop that declares an end segment closed without capturing it.
    UnterminatedLoop {
        /// The loop.
        loop_name: String,
        /// The end segment the spec declares for it.
        expected_end: Vec<u8>,
        /// Index of the segment that opened the instance; `None` for an
        /// instance opened implicitly.
        opened_at: Option<usize>,
    },
    /// A closing segment's count element does not match what it counts.
    ControlCountMismatch {
        /// The closing segment id, e.g. `SE`.
        segment_id: Vec<u8>,
        /// 1-based position of the count element.
        element: usize,
        /// The count observed in the stream.
        expected: usize,
        /// The count element as written.
        found: Vec<u8>,
    },
    /// A control or count element the spec names is absent from its segment.
    ControlElementMissing {
        /// The segment id, e.g. `SE`.
        segment_id: Vec<u8>,
        /// 1-based position of the absent element.
        element: usize,
    },
    /// A closing segment's control number differs from its opener's.
    ControlNumberMismatch {
        /// The opening segment id, e.g. `ST`.
        opener: Vec<u8>,
        /// 1-based position of the control number in the opener.
        opener_element: usize,
        /// The closing segment id, e.g. `SE`.
        closer: Vec<u8>,
        /// 1-based position of the control number in the closer.
        closer_element: usize,
        /// The opener's control number as written.
        opener_value: Vec<u8>,
        /// The closer's control number as written.
        closer_value: Vec<u8>,
        /// Index of the opening segment; `None` for an instance opened implicitly.
        opened_at: Option<usize>,
    },
    /// A required element (or component) is absent or empty.
    RequiredElementMissing {
        /// The segment id.
        segment_id: Vec<u8>,
        /// 1-based element position.
        element: usize,
        /// 1-based component position, when the definition is a component.
        component: Option<usize>,
        /// The element's name in the spec.
        name: String,
    },
    /// A value does not parse as its declared type.
    TypeMismatch {
        /// The segment id.
        segment_id: Vec<u8>,
        /// 1-based element position.
        element: usize,
        /// 1-based component position, when the definition is a component.
        component: Option<usize>,
        /// The element's name in the spec.
        name: String,
        /// The declared type.
        expected: ElementType,
    },
    /// A value is shorter or longer than its definition allows.
    LengthOutOfRange {
        /// The segment id.
        segment_id: Vec<u8>,
        /// 1-based element position.
        element: usize,
        /// 1-based component position, when the definition is a component.
        component: Option<usize>,
        /// The element's name in the spec.
        name: String,
        /// Declared minimum length.
        min: Option<usize>,
        /// Declared maximum length.
        max: Option<usize>,
        /// The value's length.
        length: usize,
    },
    /// A text value was not stored: its column cannot address more bytes.
    ValueDropped {
        /// The table.
        table: String,
        /// The column.
        column: String,
        /// The column's byte length the value would have produced.
        bytes: usize,
    },
    /// A composite element has more components than its definition declares.
    CompositeShape {
        /// The segment id.
        segment_id: Vec<u8>,
        /// 1-based element position.
        element: usize,
        /// The element's name in the spec.
        name: String,
        /// Highest component position the definition declares.
        declared: usize,
        /// Components found in the file.
        found: usize,
    },
    /// A value is not in its element's code list.
    CodeNotInList {
        /// The segment id.
        segment_id: Vec<u8>,
        /// 1-based element position.
        element: usize,
        /// 1-based component position, when the definition is a component.
        component: Option<usize>,
        /// The element's name in the spec.
        name: String,
        /// The codes the spec lists, sorted by their bytes.
        codes: Vec<String>,
        /// The occurrence whose own list it is; `None` for the element's list.
        occurrence: Option<String>,
    },
    /// A loop instance closed without an occurrence its loop requires.
    RequiredOccurrenceMissing {
        /// The loop.
        loop_name: String,
        /// Index of the segment that opened the instance.
        opened_at: Option<usize>,
        /// The occurrence's name.
        occurrence: String,
        /// The segment and qualifier that select it, e.g.
        /// `"PER" where PER01 is "BL"`.
        selector: String,
    },
    /// An occurrence repeats more times in one loop instance than its maximum.
    OccurrenceOverMax {
        /// The loop.
        loop_name: String,
        /// The occurrence's name.
        occurrence: String,
        /// The segment and qualifier that select it.
        selector: String,
        /// The maximum the spec declares.
        max: usize,
        /// Times the occurrence has appeared in the instance, this one included.
        count: usize,
    },
    /// A loop has more instances under one parent instance than its maximum.
    LoopOverMax {
        /// The loop.
        loop_name: String,
        /// The parent loop; `None` for a loop at the root.
        parent: Option<String>,
        /// The maximum the spec declares.
        max: usize,
        /// Instances under the parent instance so far, this one included.
        count: usize,
    },
    /// A segment comes after one whose occurrence has a higher position.
    OutOfOrder {
        /// The loop of the segment's occurrence.
        loop_name: String,
        /// The segment's occurrence.
        occurrence: String,
        /// The occurrence's position.
        pos: usize,
        /// The loop of the occurrence it follows.
        after_loop: String,
        /// The occurrence it follows: the highest position seen so far.
        after: String,
        /// That occurrence's position.
        after_pos: usize,
    },
    /// A segment the loop holds matches none of the loop's occurrences of it.
    UnknownOccurrence {
        /// The loop.
        loop_name: String,
        /// The segment id.
        segment_id: Vec<u8>,
        /// 1-based position of the qualifier element.
        element: usize,
        /// 1-based position of the qualifier component, when it is one.
        component: Option<usize>,
        /// The names of the loop's occurrences of the segment, by position.
        occurrences: Vec<String>,
    },
    /// A loop instance closed without a child loop its spec requires.
    RequiredLoopMissing {
        /// The loop that closed.
        loop_name: String,
        /// Index of the segment that opened the instance.
        opened_at: Option<usize>,
        /// The required child loop.
        child: String,
        /// The child's trigger as the spec writes it.
        expected_trigger: String,
    },
    /// The amounts of a balancing rule do not add up in one loop instance.
    BalanceMismatch {
        /// The rule's name in the spec.
        rule: String,
        /// The loop whose instance the rule checks.
        loop_name: String,
        /// Index of the segment that opened the instance; `None` for an
        /// instance opened implicitly.
        opened_at: Option<usize>,
        /// The rule's target values as the spec declares them.
        target: String,
        /// The rule's summed values as the spec declares them.
        sum: String,
        /// The target's amount, scaled by `10^scale`.
        expected: i128,
        /// The sum's amount, scaled by `10^scale`.
        computed: i128,
        /// Decimal places of both amounts.
        scale: u8,
        /// Indexes of every segment whose values were read, in stream order.
        segments: Vec<usize>,
    },
    /// A finding reported by a validator outside this crate, carried as that
    /// validator states it.
    External {
        /// Who reported the finding, e.g. the validator's name.
        origin: String,
        /// The validator's own code for the finding, when it gives one.
        code: Option<String>,
        /// The validator's description of the finding.
        message: String,
        /// The SNIP level the caller assigns to the finding.
        level: SnipLevel,
    },
}

impl Rule {
    /// The SNIP level the rule belongs to.
    pub fn level(&self) -> SnipLevel {
        match self {
            Rule::ByteOrderMark
            | Rule::UnknownSegment { .. }
            | Rule::ImplicitLoop { .. }
            | Rule::UnterminatedLoop { .. }
            | Rule::ControlCountMismatch { .. }
            | Rule::ControlElementMissing { .. }
            | Rule::ControlNumberMismatch { .. } => SnipLevel::L1,
            Rule::RequiredElementMissing { .. }
            | Rule::TypeMismatch { .. }
            | Rule::LengthOutOfRange { .. }
            | Rule::CompositeShape { .. }
            | Rule::CodeNotInList { .. }
            | Rule::ValueDropped { .. }
            | Rule::RequiredOccurrenceMissing { .. }
            | Rule::OccurrenceOverMax { .. }
            | Rule::LoopOverMax { .. }
            | Rule::OutOfOrder { .. }
            | Rule::UnknownOccurrence { .. }
            | Rule::RequiredLoopMissing { .. } => SnipLevel::L2,
            Rule::BalanceMismatch { .. } => SnipLevel::L3,
            Rule::External { level, .. } => *level,
        }
    }

    /// The id of the segment the rule names, for a rule about one segment's
    /// elements.
    pub(crate) fn segment_id(&self) -> Option<&[u8]> {
        match self {
            Rule::ControlCountMismatch { segment_id, .. }
            | Rule::ControlElementMissing { segment_id, .. }
            | Rule::RequiredElementMissing { segment_id, .. }
            | Rule::TypeMismatch { segment_id, .. }
            | Rule::LengthOutOfRange { segment_id, .. }
            | Rule::CompositeShape { segment_id, .. }
            | Rule::CodeNotInList { segment_id, .. }
            | Rule::UnknownOccurrence { segment_id, .. } => Some(segment_id),
            _ => None,
        }
    }

    /// The name of the rule's variant, e.g. `RequiredElementMissing`, so a
    /// caller can filter findings without parsing messages.
    pub fn kind(&self) -> &'static str {
        match self {
            Rule::ByteOrderMark => "ByteOrderMark",
            Rule::UnknownSegment { .. } => "UnknownSegment",
            Rule::ImplicitLoop { .. } => "ImplicitLoop",
            Rule::UnterminatedLoop { .. } => "UnterminatedLoop",
            Rule::ControlCountMismatch { .. } => "ControlCountMismatch",
            Rule::ControlElementMissing { .. } => "ControlElementMissing",
            Rule::ControlNumberMismatch { .. } => "ControlNumberMismatch",
            Rule::RequiredElementMissing { .. } => "RequiredElementMissing",
            Rule::TypeMismatch { .. } => "TypeMismatch",
            Rule::LengthOutOfRange { .. } => "LengthOutOfRange",
            Rule::ValueDropped { .. } => "ValueDropped",
            Rule::CompositeShape { .. } => "CompositeShape",
            Rule::CodeNotInList { .. } => "CodeNotInList",
            Rule::RequiredOccurrenceMissing { .. } => "RequiredOccurrenceMissing",
            Rule::OccurrenceOverMax { .. } => "OccurrenceOverMax",
            Rule::LoopOverMax { .. } => "LoopOverMax",
            Rule::OutOfOrder { .. } => "OutOfOrder",
            Rule::UnknownOccurrence { .. } => "UnknownOccurrence",
            Rule::RequiredLoopMissing { .. } => "RequiredLoopMissing",
            Rule::BalanceMismatch { .. } => "BalanceMismatch",
            Rule::External { .. } => "External",
        }
    }
}
