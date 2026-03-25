//! Validated runtime worker identity.
//!
//! `WorkerId` is a foundational identity type used across the entire
//! runtime layer — mailbox bundles, orchestrator state, paths, storage
//! records, and both service surfaces reference it.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A validated runtime worker identifier.
///
/// Worker ids are path-safe ASCII strings. They may contain ASCII
/// letters, digits, `-`, and `_`.
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
        if !first.is_ascii_alphanumeric() {
            return Err(WorkerIdError::InvalidStart { id: value });
        }

        for (pos, ch) in value.char_indices() {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                continue;
            }
            return Err(WorkerIdError::InvalidChar { id: value, ch, pos });
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

    /// The first character is not ASCII alphanumeric.
    #[error("worker id `{id}` must start with an ASCII letter or digit")]
    InvalidStart { id: String },

    /// The id contained an unsupported character.
    #[error("worker id `{id}` contains invalid character `{ch}` at byte {pos}")]
    InvalidChar { id: String, ch: char, pos: usize },
}
