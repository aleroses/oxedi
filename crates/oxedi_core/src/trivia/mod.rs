//! Trivia: the bytes tolerated between segments that carry no data.
//!
//! Trivia is a UTF-8 byte order mark at the very start of the input and
//! whitespace (space, tab, carriage return, line feed) around segments. It is
//! never dropped: it stays in the `raw` of the segment that follows it.
//! [`LeadingTrivia`] splits the start of an input into its two parts once, so
//! the framing and the delimiter reading agree on what precedes the first
//! segment.

#[cfg(test)]
mod tests;

/// The UTF-8 byte order mark some editors write at the start of a file.
pub const BYTE_ORDER_MARK: &[u8] = b"\xEF\xBB\xBF";

/// Bytes tolerated between a terminator and the next segment.
pub const fn is_trivia(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\r' | b'\n')
}

/// What precedes the first segment's text: an optional byte order mark, then
/// a run of whitespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeadingTrivia {
    /// `true` when the input starts with a byte order mark. A mark anywhere
    /// else is data.
    pub byte_order_mark: bool,
    /// Bytes of whitespace after the mark (or from the start without one).
    pub whitespace: usize,
}

impl LeadingTrivia {
    /// Splits the start of `input` into its byte order mark and whitespace.
    pub fn of(input: &[u8]) -> Self {
        let (byte_order_mark, rest) = match input.strip_prefix(BYTE_ORDER_MARK) {
            Some(rest) => (true, rest),
            None => (false, input),
        };
        let whitespace = rest
            .iter()
            .position(|&byte| !is_trivia(byte))
            .unwrap_or(rest.len());
        Self {
            byte_order_mark,
            whitespace,
        }
    }

    /// Total bytes before the first segment's text: the mark (3 bytes) plus
    /// the whitespace.
    pub const fn len(&self) -> usize {
        let mark = if self.byte_order_mark {
            BYTE_ORDER_MARK.len()
        } else {
            0
        };
        mark.saturating_add(self.whitespace)
    }

    /// `true` when nothing precedes the first segment's text.
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
