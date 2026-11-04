//! The load error's place in an error chain, and the parts of its text that
//! are built from lists.

use std::fmt;

use super::error::SpecError;

/// The parent of sibling loops as a message names it: quoted, or `the root`.
pub(super) struct Under<'a>(pub(super) Option<&'a str>);

impl fmt::Display for Under<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(parent) => write!(f, "{parent:?}"),
            None => write!(f, "the root"),
        }
    }
}

/// The loops of a parent cycle in walk order, back to the first, e.g.
/// `loops form a parent cycle: a -> b -> a`.
pub(super) fn write_cycle(f: &mut fmt::Formatter<'_>, members: &[String]) -> fmt::Result {
    let mut walk: Vec<&str> = members.iter().map(String::as_str).collect();
    walk.extend(members.first().map(String::as_str));
    write!(f, "loops form a parent cycle: {}", walk.join(" -> "))
}

/// Two sibling loops that share one trigger, with its conditions when it has any.
pub(super) fn write_ambiguous(
    f: &mut fmt::Formatter<'_>,
    [first, second]: [&String; 2],
    parent: Option<&str>,
    segment: &str,
    conditions: &[(usize, String)],
) -> fmt::Result {
    write!(
        f,
        "loops {first:?} and {second:?} under {} share the identical trigger {segment:?}",
        Under(parent)
    )?;
    if conditions.is_empty() {
        return Ok(());
    }
    let conditions: Vec<String> = conditions
        .iter()
        .map(|(position, value)| format!("{position}: {value:?}"))
        .collect();
    write!(f, " where {{{}}}", conditions.join(", "))
}

impl std::error::Error for SpecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // Every variant that holds an inner error prints it in full, so the
        // chain stops here instead of printing it a second time.
        match self {
            SpecError::Json(_)
            | SpecError::Schema { .. }
            | SpecError::Patch { .. }
            | SpecError::UnknownParent { .. }
            | SpecError::Cycle { .. }
            | SpecError::NoLoops { .. }
            | SpecError::EmptySegmentId { .. }
            | SpecError::EndIsTrigger { .. }
            | SpecError::BadPosition { .. }
            | SpecError::NotAnObject { .. }
            | SpecError::WrongType { .. }
            | SpecError::UnknownKey { .. }
            | SpecError::MissingKey { .. }
            | SpecError::SegmentSchema { .. }
            | SpecError::BadElementDef { .. }
            | SpecError::TableSchema { .. }
            | SpecError::BadTable { .. }
            | SpecError::BadVersion { .. }
            | SpecError::BadControl { .. }
            | SpecError::BadOccurrence { .. }
            | SpecError::BalanceSchema { .. }
            | SpecError::BadBalance { .. }
            | SpecError::UnmatchedTrigger { .. }
            | SpecError::TriggerQualifierWider { .. }
            | SpecError::TriggerNotFirst { .. }
            | SpecError::UnknownLoopUsage { .. }
            | SpecError::ZeroLoopMax { .. }
            | SpecError::AmbiguousTrigger { .. }
            | SpecError::OverlappingTriggers { .. }
            | SpecError::SummarySchema { .. }
            | SpecError::BadSummary { .. }
            | SpecError::UnknownDefaultTable { .. } => None,
        }
    }
}
