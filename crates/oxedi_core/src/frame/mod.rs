//! Framing: find the next segment's bytes knowing only the delimiters.
//!
//! Pure and allocation-free. It does not know what a segment means; it only
//! knows where one ends. Every byte of the input is accounted for by exactly
//! one [`Frame::raw`], which is what makes the tokenizer lossless. A UTF-8
//! byte order mark at the very start of the input is leading trivia of the
//! first frame ([`first_frame`]); anywhere else it is data.

use crate::delimiters::Delimiters;
use crate::trivia::{BYTE_ORDER_MARK, is_trivia};

/// One segment's worth of bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> {
    /// Every byte this frame accounts for: leading trivia, body and terminator.
    /// Concatenating `raw` over all frames reproduces the input exactly.
    pub raw: &'a [u8],
    /// The segment text: `raw` without leading trivia and without the terminator.
    pub body: &'a [u8],
    /// `false` only for the last frame of an input that does not end with a terminator.
    pub terminated: bool,
}

/// Splits the first frame off the front of a whole input: as [`next_frame`],
/// except that a UTF-8 byte order mark at the very start is leading trivia,
/// kept in the frame's `raw` before the body.
pub fn first_frame<'a>(input: &'a [u8], delims: &Delimiters) -> Option<(Frame<'a>, &'a [u8])> {
    let Some(after) = input.strip_prefix(BYTE_ORDER_MARK) else {
        return next_frame(input, delims);
    };
    let Some((frame, rest)) = next_frame(after, delims) else {
        // `next_frame` returns `None` only for empty input: the input is the mark alone.
        let frame = Frame {
            raw: input,
            body: after,
            terminated: false,
        };
        return Some((frame, after));
    };
    // `frame.raw` is a prefix of `after`, which is `input` past the mark, so the
    // end index is at most `input.len()`.
    let raw = &input[..BYTE_ORDER_MARK.len() + frame.raw.len()];
    let frame = Frame {
        raw,
        body: frame.body,
        terminated: frame.terminated,
    };
    Some((frame, rest))
}

/// Splits the next frame off the front of `input`.
///
/// Returns the frame and the remaining input. Returns `None` only when `input`
/// is empty, so a loop over it always terminates: every call consumes at least
/// one byte.
pub fn next_frame<'a>(input: &'a [u8], delims: &Delimiters) -> Option<(Frame<'a>, &'a [u8])> {
    if input.is_empty() {
        return None;
    }
    let body_start = input
        .iter()
        .position(|&byte| !is_trivia(byte))
        .unwrap_or(input.len());
    // `body_start <= input.len()` by construction of `position`/`unwrap_or`, and
    // `terminator_at < input.len()` because `offset` indexes into `input[body_start..]`,
    // so every slice and split below is in range.
    match find_unescaped(&input[body_start..], delims.segment, delims.release) {
        Some(offset) => {
            let terminator_at = body_start + offset;
            let (raw, rest) = input.split_at(terminator_at + 1);
            let frame = Frame {
                raw,
                body: &input[body_start..terminator_at],
                terminated: true,
            };
            Some((frame, rest))
        }
        None => {
            let frame = Frame {
                raw: input,
                body: &input[body_start..],
                terminated: false,
            };
            Some((frame, &input[input.len()..]))
        }
    }
}

/// Index of the first `target` in `haystack` that is not escaped by `release`.
///
/// A release byte makes the byte after it literal, including another release
/// byte. A release byte at the very end escapes nothing and is never read past.
pub fn find_unescaped(haystack: &[u8], target: u8, release: Option<u8>) -> Option<usize> {
    let Some(release) = release else {
        return haystack.iter().position(|&byte| byte == target);
    };
    let mut at = 0;
    while let Some(&byte) = haystack.get(at) {
        if byte == release {
            at += 2;
        } else if byte == target {
            return Some(at);
        } else {
            at += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests;
