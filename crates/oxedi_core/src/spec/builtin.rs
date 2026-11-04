//! The specs the core ships, found by transaction set and version.

use std::fmt;
use std::sync::{Arc, OnceLock};

use super::Spec;
use super::version::chosen;
use crate::segment::Segment;

/// An X12 release a built-in spec covers. It is not specific to one
/// transaction set: other sets share the releases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Version {
    /// 5010.
    V5010,
    /// 4010.
    V4010,
}

impl Version {
    /// Every version, in the order of the registry.
    pub const ALL: [Version; 2] = [Version::V5010, Version::V4010];

    /// The release as `version` options write it, e.g. `"5010"`.
    pub const fn text(self) -> &'static str {
        match self {
            Version::V5010 => "5010",
            Version::V4010 => "4010",
        }
    }

    /// The version `text` names, if there is one.
    pub fn parse(text: &str) -> Option<Version> {
        Version::ALL
            .into_iter()
            .find(|version| version.text() == text)
    }
}

/// One spec the core ships, with the transaction set and version it covers.
#[derive(Debug)]
pub struct Builtin {
    /// The transaction set, e.g. `"835"`.
    pub transaction: &'static str,
    /// The version the spec covers.
    pub version: Version,
    /// The spec.
    pub spec: Arc<Spec>,
}

static ALL: OnceLock<Vec<Builtin>> = OnceLock::new();

impl Builtin {
    /// The version of the built-in [`Builtin::fallback`] returns.
    pub const DEFAULT_VERSION: Version = Version::V5010;

    /// Every built-in, loaded once per process; the first is the fallback.
    pub fn all() -> &'static [Builtin] {
        ALL.get_or_init(|| {
            vec![
                Builtin {
                    transaction: "835",
                    version: Builtin::DEFAULT_VERSION,
                    spec: Arc::new(Spec::builtin_835()),
                },
                Builtin {
                    transaction: "835",
                    version: Version::V4010,
                    spec: Arc::new(Spec::builtin_835_4010()),
                },
            ]
        })
    }

    /// The built-in used when a file declares no version a built-in covers.
    pub fn fallback() -> &'static Builtin {
        &Builtin::all()[0]
    }

    /// The built-in of `transaction` at `version`.
    pub fn find(transaction: &str, version: Version) -> Result<&'static Builtin, UnknownVersion> {
        Builtin::all()
            .iter()
            .find(|b| b.transaction == transaction && b.version == version)
            .ok_or_else(|| UnknownVersion::new(transaction, version.text()))
    }

    /// The built-in of `transaction` at the version `text` names.
    pub fn find_text(transaction: &str, text: &str) -> Result<&'static Builtin, UnknownVersion> {
        Version::parse(text)
            .ok_or_else(|| UnknownVersion::new(transaction, text))
            .and_then(|version| Builtin::find(transaction, version))
    }

    /// The versions of `transaction`'s built-ins, in registry order.
    pub fn versions(transaction: &str) -> Vec<Version> {
        Builtin::all()
            .iter()
            .filter(|b| b.transaction == transaction)
            .map(|b| b.version)
            .collect()
    }

    /// The built-in whose declared version `segments` carry, else the
    /// fallback; the rule of [`Spec::select`], sharing its code.
    pub fn select<'s>(segments: impl IntoIterator<Item = Segment<'s>>) -> &'static Builtin {
        let all = Builtin::all();
        let specs: Vec<&Spec> = all.iter().map(|b| b.spec.as_ref()).collect();
        chosen(&specs, segments)
            .and_then(|at| all.get(at))
            .unwrap_or_else(|| Builtin::fallback())
    }
}

/// A version no built-in covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownVersion {
    /// The transaction set asked for.
    pub transaction: String,
    /// The version asked for.
    pub version: String,
    /// The versions the transaction's built-ins cover, as text; empty when
    /// no built-in covers the transaction.
    pub known: Vec<&'static str>,
}

impl UnknownVersion {
    fn new(transaction: &str, version: &str) -> UnknownVersion {
        UnknownVersion {
            transaction: transaction.to_owned(),
            version: version.to_owned(),
            known: Builtin::versions(transaction)
                .into_iter()
                .map(Version::text)
                .collect(),
        }
    }
}

impl fmt::Display for UnknownVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown version {:?}; ", self.version)?;
        if self.known.is_empty() {
            return write!(
                f,
                "no built-in spec covers transaction {:?}",
                self.transaction
            );
        }
        let known: Vec<String> = self.known.iter().map(|name| format!("{name:?}")).collect();
        write!(f, "version must be one of {}", known.join(", "))
    }
}

impl std::error::Error for UnknownVersion {}
