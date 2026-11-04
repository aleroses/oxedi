//! The lengths an element allows: the bounds a spec declares, the check
//! against them and the text a message spells them as.

use std::fmt;

/// The lengths an element allows, each bound inclusive; a bound that is
/// `None` is not checked. The default allows any length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LengthRange {
    /// Shortest length allowed, when the spec declares one.
    pub min: Option<usize>,
    /// Longest length allowed, when the spec declares one.
    pub max: Option<usize>,
}

impl LengthRange {
    /// `true` when `length` is at least `min` and at most `max`.
    pub const fn contains(self, length: usize) -> bool {
        let above_min = match self.min {
            Some(min) => length >= min,
            None => true,
        };
        let below_max = match self.max {
            Some(max) => length <= max,
            None => true,
        };
        above_min && below_max
    }
}

/// The range as a message spells it: `2 to 5`, `at least 2`, `at most 5`
/// or `any length`.
impl fmt::Display for LengthRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.min, self.max) {
            (Some(min), Some(max)) => write!(f, "{min} to {max}"),
            (Some(min), None) => write!(f, "at least {min}"),
            (None, Some(max)) => write!(f, "at most {max}"),
            (None, None) => write!(f, "any length"),
        }
    }
}
