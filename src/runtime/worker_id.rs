//! Validated runtime worker identity.
//!
//! `WorkerId` is a foundational identity type used across the entire
//! runtime layer — mailbox bundles, orchestrator state, paths, storage
//! records, and both service surfaces reference it.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A validated runtime worker identifier in kebab-case.
///
/// ## Invariants
///
/// - Non-empty.
/// - Starts with a lowercase ASCII letter (`a`–`z`).
/// - Contains only lowercase ASCII letters, digits, and hyphens.
/// - Does not end with a hyphen.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct WorkerId(String);

impl WorkerId {
    /// Construct and validate one worker identifier.
    pub fn new(value: impl Into<String>) -> std::result::Result<Self, WorkerIdError> {
        let value = value.into();
        if value.is_empty() {
            return Err(WorkerIdError::Empty);
        }

        let first = value.chars().next().expect("checked empty worker id");
        if !first.is_ascii_lowercase() {
            return Err(WorkerIdError::InvalidStart { id: value });
        }

        for (pos, ch) in value.char_indices() {
            if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' {
                continue;
            }
            return Err(WorkerIdError::InvalidChar { id: value, ch, pos });
        }

        if value.ends_with('-') {
            return Err(WorkerIdError::TrailingHyphen { id: value });
        }

        Ok(Self(value))
    }

    /// Borrow the worker id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for WorkerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for WorkerId {
    type Err = WorkerIdError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::new(s)
    }
}

/// Errors produced when constructing a [`WorkerId`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkerIdError {
    /// The worker id was empty.
    #[error("worker id is empty")]
    Empty,

    /// The first character is not a lowercase ASCII letter.
    #[error("worker id `{id}` must start with a lowercase ASCII letter")]
    InvalidStart { id: String },

    /// The id contained a character outside the kebab-case alphabet.
    #[error(
        "worker id `{id}` contains invalid character `{ch}` at byte {pos}; only lowercase letters, digits, and hyphens are allowed"
    )]
    InvalidChar { id: String, ch: char, pos: usize },

    /// The id ends with a hyphen.
    #[error("worker id `{id}` must not end with a hyphen")]
    TrailingHyphen { id: String },
}
