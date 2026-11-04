//! The unit a timestamp or a time counts its ticks in.

use super::calendar::DAY_SECONDS;

/// The unit a timestamp or a time of day is counted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeUnit {
    Second,
    Millisecond,
    Microsecond,
    Nanosecond,
}

impl TimeUnit {
    /// Ticks in one second.
    pub const fn ticks_per_second(self) -> i64 {
        match self {
            Self::Second => 1,
            Self::Millisecond => 1_000,
            Self::Microsecond => 1_000_000,
            Self::Nanosecond => 1_000_000_000,
        }
    }

    /// Ticks in one day.
    pub const fn ticks_per_day(self) -> i64 {
        DAY_SECONDS * self.ticks_per_second()
    }
}
