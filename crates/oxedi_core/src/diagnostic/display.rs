//! The `Display` text of every rule, one sentence naming the element, its
//! name and the offending value, written in one `write!` per rule over the
//! pieces of `text.rs`.

use std::fmt;

use super::text::{
    CodeCount, Cut, ElementRef, InOccurrence, MAX_LISTED_SEGMENTS, OfSegment, OffBy, OpenedAt,
    Opener, Parent, Reporter, ShortList, debug_name, quoted_code, segment_index,
};
use super::{Quoted, Rule};
use crate::value::{LengthRange, decimal_display};

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rule::ByteOrderMark => write!(
                f,
                "the input starts with a UTF-8 byte order mark, kept as leading trivia of the first segment"
            ),
            Rule::UnknownSegment { segment_id } => write!(
                f,
                "segment {} is not part of the structure: no open loop holds it and it opens no loop",
                Quoted(segment_id)
            ),
            Rule::ImplicitLoop {
                loop_name,
                expected_trigger,
                caused_by,
            } => write!(
                f,
                "loop {loop_name:?} opened without its own trigger ({expected_trigger}) to hold segment {}",
                Quoted(caused_by)
            ),
            Rule::UnterminatedLoop {
                loop_name,
                expected_end,
                opened_at,
            } => write!(
                f,
                "loop {loop_name:?} {}closed without its end segment {}",
                OpenedAt(*opened_at),
                Quoted(expected_end)
            ),
            Rule::ControlCountMismatch {
                segment_id,
                element,
                expected,
                found,
            } => write!(
                f,
                "{} declares {} but the count is {expected}",
                ElementRef::simple(segment_id, *element),
                Quoted(found)
            ),
            Rule::ControlElementMissing {
                segment_id,
                element,
            } => write!(
                f,
                "control element {} is missing: the segment has no element {element}",
                ElementRef::simple(segment_id, *element)
            ),
            Rule::ControlNumberMismatch {
                opener,
                opener_element,
                closer,
                closer_element,
                opener_value,
                closer_value,
                opened_at,
            } => write!(
                f,
                "{} {} does not match {} {}{}",
                ElementRef::simple(closer, *closer_element),
                Quoted(closer_value),
                ElementRef::simple(opener, *opener_element),
                Quoted(opener_value),
                OfSegment(*opened_at)
            ),
            Rule::RequiredElementMissing {
                segment_id,
                element,
                component,
                name,
            } => write!(
                f,
                "required element {} ({name}) is missing or empty",
                ElementRef::new(segment_id, *element, *component)
            ),
            Rule::TypeMismatch {
                segment_id,
                element,
                component,
                name,
                expected,
            } => write!(
                f,
                "element {} ({name}) is not a valid {expected}",
                ElementRef::new(segment_id, *element, *component)
            ),
            Rule::LengthOutOfRange {
                segment_id,
                element,
                component,
                name,
                min,
                max,
                length,
            } => write!(
                f,
                "element {} ({name}) has length {length}; the spec allows {}",
                ElementRef::new(segment_id, *element, *component),
                LengthRange {
                    min: *min,
                    max: *max
                }
            ),
            Rule::ValueDropped {
                table,
                column,
                bytes,
            } => write!(
                f,
                "text for column {column:?} of table {table:?} was not stored: it would bring the column to {bytes} bytes and a column holds at most {}",
                i32::MAX
            ),
            Rule::CompositeShape {
                segment_id,
                element,
                name,
                declared,
                found,
            } => write!(
                f,
                "element {} ({name}) has {found} components; the spec declares {declared}",
                ElementRef::simple(segment_id, *element)
            ),
            Rule::CodeNotInList {
                segment_id,
                element,
                component,
                name,
                codes,
                occurrence,
            } => write!(
                f,
                "element {} ({name}) is not {} the spec lists for it{}{}",
                ElementRef::new(segment_id, *element, *component),
                CodeCount(codes.len()),
                InOccurrence(occurrence.as_ref()),
                ShortList {
                    items: codes,
                    item: quoted_code
                }
            ),
            Rule::RequiredOccurrenceMissing {
                loop_name,
                opened_at,
                occurrence,
                selector,
            } => write!(
                f,
                "loop {loop_name:?} {}closed without its required occurrence {occurrence:?} ({selector})",
                OpenedAt(*opened_at)
            ),
            Rule::OccurrenceOverMax {
                loop_name,
                occurrence,
                selector,
                max,
                count,
            } => write!(
                f,
                "occurrence {occurrence:?} ({selector}) of loop {loop_name:?} appears {count} times in one instance; the spec allows at most {max}"
            ),
            Rule::LoopOverMax {
                loop_name,
                parent,
                max,
                count,
            } => write!(
                f,
                "loop {loop_name:?} has {count} instances {}; the spec allows at most {max}",
                Parent(parent.as_ref())
            ),
            Rule::OutOfOrder {
                loop_name,
                occurrence,
                pos,
                after_loop,
                after,
                after_pos,
            } => write!(
                f,
                "occurrence {occurrence:?} (position {pos}) of loop {loop_name:?} comes after occurrence {after:?} (position {after_pos}) of loop {after_loop:?}"
            ),
            Rule::UnknownOccurrence {
                loop_name,
                segment_id,
                element,
                component,
                occurrences,
            } => write!(
                f,
                "segment {} matches none of the {} occurrences loop {loop_name:?} declares for it{}: {} holds none of their qualifier codes",
                Quoted(segment_id),
                occurrences.len(),
                ShortList {
                    items: occurrences,
                    item: debug_name
                },
                ElementRef::new(segment_id, *element, *component)
            ),
            Rule::RequiredLoopMissing {
                loop_name,
                opened_at,
                child,
                expected_trigger,
            } => write!(
                f,
                "loop {loop_name:?} {}closed without its required child loop {child:?} (trigger {expected_trigger})",
                OpenedAt(*opened_at)
            ),
            Rule::BalanceMismatch {
                rule,
                loop_name,
                opened_at,
                target,
                sum,
                expected,
                computed,
                scale,
                segments,
            } => write!(
                f,
                "balancing rule {rule:?} fails in loop {loop_name:?} {}: {target} is {}, but {sum} adds up to {}{}; read from segments {}",
                Opener(*opened_at),
                decimal_display(*expected, *scale),
                decimal_display(*computed, *scale),
                OffBy {
                    expected: *expected,
                    computed: *computed,
                    scale: *scale
                },
                Cut {
                    items: segments,
                    limit: MAX_LISTED_SEGMENTS,
                    item: segment_index
                }
            ),
            Rule::External {
                origin,
                code,
                message,
                ..
            } => write!(
                f,
                "{message} {}",
                Reporter {
                    origin,
                    code: code.as_deref()
                }
            ),
        }
    }
}
