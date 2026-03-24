//! Error types for the perspective module.

use thiserror::Error;

use crate::fileset;

use super::name::PerspectiveName;

/// Errors produced when constructing a [`PerspectiveName`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PerspectiveNameError {
    /// The input string was empty.
    #[error("perspective name is empty")]
    Empty,

    /// The first character is not an uppercase ASCII letter.
    #[error("perspective name `{name}` must start with an uppercase ASCII letter")]
    InvalidStart { name: String },

    /// A non-alphanumeric character was found.
    #[error("perspective name `{name}` contains invalid character `{ch}` at byte {pos}")]
    InvalidChar { name: String, ch: char, pos: usize },
}

/// Top-level error for the perspective pipeline.
#[derive(Debug, Error)]
pub enum PerspectiveError {
    /// A file set expression failed to parse.
    #[error("in perspective `{perspective}`: {source}")]
    Parse { perspective: PerspectiveName, source: crate::fileset::ParseError },

    /// A perspective references a file set name that is not defined in
    /// the compiled rulebook.
    #[error("perspective `{perspective}` references undefined file set `{name}`")]
    UndefinedFileSet { perspective: PerspectiveName, name: fileset::Name },
}
