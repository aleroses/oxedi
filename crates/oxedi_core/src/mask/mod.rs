//! A keyed mask for the columns that hold patient data.
//!
//! Text is replaced by a keyed digest, so the same value under the same key
//! always gives the same token and nothing can be recovered without the key.
//! Dates are reduced to their year, shifted by a per-seed number of days, or
//! kept.
//!
//! - `error.rs`: [`MaskError`].
//! - `tests.rs`: known answers for the digest and the date arithmetic.

mod error;
#[cfg(test)]
mod tests;

use std::fmt;

use hmac::{Hmac, Mac};
use sha2::Sha256;

pub use error::MaskError;

/// What a flagged column holds, which decides how it is masked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phi {
    /// A person's name.
    Name,
    /// An identifier of a person or of a claim.
    Id,
    /// A date.
    Date,
}

impl Phi {
    /// The word a spec writes for the kind, also mixed into the digest so the
    /// same value masks differently per kind.
    pub fn word(self) -> &'static str {
        match self {
            Phi::Name => "name",
            Phi::Id => "id",
            Phi::Date => "date",
        }
    }

    /// The kind a spec's word names, or `None`.
    pub fn from_word(word: &str) -> Option<Phi> {
        match word {
            "name" => Some(Phi::Name),
            "id" => Some(Phi::Id),
            "date" => Some(Phi::Date),
            _ => None,
        }
    }
}

/// What happens to a date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateMode {
    /// January 1 of the same year.
    Year,
    /// The whole row moves by the same number of days.
    Shift,
    /// Unchanged.
    Keep,
}

impl DateMode {
    /// Every mode.
    const ALL: [DateMode; 3] = [DateMode::Year, DateMode::Shift, DateMode::Keep];

    /// Reads `year`, `shift` or `keep`.
    pub fn parse(text: &str) -> Result<DateMode, MaskError> {
        DateMode::ALL
            .into_iter()
            .find(|mode| mode.word() == text)
            .ok_or_else(|| MaskError::UnknownDateMode {
                given: text.to_string(),
            })
    }

    /// The word that names the mode: `year`, `shift` or `keep`.
    pub fn word(self) -> &'static str {
        match self {
            DateMode::Year => "year",
            DateMode::Shift => "shift",
            DateMode::Keep => "keep",
        }
    }
}

/// A key and a date mode.
#[derive(Clone)]
pub struct Mask {
    key: Vec<u8>,
    dates: DateMode,
}

impl fmt::Debug for Mask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Mask {{ key: <{} bytes>, dates: {:?} }}",
            self.key.len(),
            self.dates
        )
    }
}

impl Mask {
    /// The shortest key accepted.
    pub const MIN_KEY: usize = 16;

    /// Builds a mask; a key shorter than [`Mask::MIN_KEY`] is refused.
    pub fn new(key: &[u8], dates: DateMode) -> Result<Mask, MaskError> {
        if key.len() < Self::MIN_KEY {
            return Err(MaskError::ShortKey { length: key.len() });
        }
        Ok(Mask {
            key: key.to_vec(),
            dates,
        })
    }

    /// The date mode.
    pub fn dates(&self) -> DateMode {
        self.dates
    }

    /// The token for a text value: the first 8 bytes of
    /// `HMAC-SHA256(key, kind 0x00 value)` as 16 lowercase hex characters.
    pub fn text(&self, kind: Phi, value: &[u8]) -> [u8; 16] {
        let digest = self.digest(kind.word().as_bytes(), value);
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let nibbles = digest
            .iter()
            .take(8)
            .flat_map(|byte| [byte >> 4, byte & 0x0f]);
        let mut out = [b'0'; 16];
        for (slot, nibble) in out.iter_mut().zip(nibbles) {
            *slot = HEX[usize::from(nibble)];
        }
        out
    }

    /// The days a row's dates move by, in `-182..=182`, from a seed.
    pub fn shift_days(&self, seed: &[u8]) -> i32 {
        let digest = self.digest(b"shift", seed);
        let head = [digest[0], digest[1], digest[2], digest[3]];
        // Remainder is below 365, so the conversion cannot fail.
        i32::try_from(u32::from_be_bytes(head) % 365).unwrap_or(0) - 182
    }

    /// January 1 of the year containing `days` (days since 1970-01-01), as
    /// days; `None` when that January 1 is before `i32::MIN`.
    pub fn year_start(days: i32) -> Option<i32> {
        crate::value::year_start(days)
    }

    fn digest(&self, tag: &[u8], value: &[u8]) -> [u8; 32] {
        let mut message = Vec::with_capacity(tag.len() + 1 + value.len());
        message.extend_from_slice(tag);
        message.push(0);
        message.extend_from_slice(value);
        hmac_sha256(&self.key, &message)
    }
}

/// HMAC-SHA256 of `message` under `key`.
fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    // HMAC accepts a key of any length, so construction cannot fail.
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(key) else {
        return [0; 32];
    };
    mac.update(message);
    mac.finalize().into_bytes().into()
}
