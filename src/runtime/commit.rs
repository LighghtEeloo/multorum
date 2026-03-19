//! Canonical git commit identifiers stored by Multorum.
//!
//! Frontends may accept symbolic revisions such as `HEAD~1` or
//! abbreviated hashes as user input, but the runtime must resolve them
//! to a stable commit id before persisting any state. This module owns
//! that persisted representation.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A canonical git commit identifier resolved by the runtime.
///
/// ## Invariants
///
/// - Identifies exactly one commit at the time it was resolved.
/// - Stores `git rev-parse --verify <rev>^{commit}` output.
/// - Never stores symbolic revisions such as `HEAD` or abbreviated
///   hashes.
///
/// Note: User-facing APIs may accept symbolic or abbreviated revisions,
/// but Multorum must convert them into `CanonicalCommitHash` before they
/// become persisted runtime state.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CanonicalCommitHash(String);

impl CanonicalCommitHash {
    /// Construct a canonical commit hash from an already-resolved value.
    ///
    /// Note: This constructor is crate-private so callers cannot bypass
    /// runtime git resolution with raw user input.
    pub(crate) fn new(resolved: impl Into<String>) -> Self {
        Self(resolved.into())
    }

    /// Return the canonical commit hash as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for CanonicalCommitHash {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for CanonicalCommitHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for CanonicalCommitHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
