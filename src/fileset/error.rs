//! Error types for the file set algebra.

use thiserror::Error;

/// Errors produced when constructing a [`Name`](super::Name).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum NameError {
    /// The input string was empty.
    #[error("file set name is empty")]
    Empty,

    /// The first character is not an uppercase ASCII letter.
    #[error("file set name `{name}` must start with an uppercase ASCII letter")]
    InvalidStart { name: String },

    /// A non-alphanumeric character was found.
    #[error("file set name `{name}` contains invalid character `{ch}` at byte {pos}")]
    InvalidChar { name: String, ch: char, pos: usize },
}

/// Errors produced when constructing a [`GlobPattern`](super::GlobPattern).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum GlobPatternError {
    /// The pattern is not valid according to [`wax::Glob`].
    #[error("invalid glob pattern `{pattern}`: {reason}")]
    Invalid { pattern: String, reason: String },
}

/// Errors produced when parsing a file set expression string.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    /// The expression string was empty or all whitespace.
    #[error("unexpected end of expression")]
    UnexpectedEof,

    /// An unexpected character was encountered.
    #[error("unexpected character `{ch}` at byte {pos}")]
    UnexpectedChar { ch: char, pos: usize },

    /// A `(` was opened but never closed.
    #[error("expected closing parenthesis at byte {pos}")]
    UnclosedParen { pos: usize },

    /// Content remained after the expression was fully parsed.
    #[error("trailing content at byte {pos}")]
    TrailingContent { pos: usize },

    /// A name within the expression failed validation.
    #[error("invalid name: {0}")]
    InvalidName(#[from] NameError),
}

/// Errors produced during validation of file set definitions.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    /// A compound expression references a name not defined in the table.
    #[error("undefined file set `{name}`, referenced by `{referenced_by}`")]
    Undefined { name: super::Name, referenced_by: super::Name },

    /// A cycle was detected in the dependency graph.
    #[error("cycle detected in file set definitions: {cycle}")]
    Cycle { cycle: String },
}

/// Errors produced during compilation of file set definitions.
#[derive(Debug, Error)]
pub enum CompileError {
    /// A glob pattern failed to compile.
    #[error("invalid glob pattern `{pattern}`: {reason}")]
    Glob { pattern: String, reason: String },

    /// Filesystem walking failed.
    #[error("failed to walk directory `{}`: {reason}", root.display())]
    Walk { root: std::path::PathBuf, reason: String },
}

/// Top-level error for the file set pipeline
/// (validation or compilation).
#[derive(Debug, Error)]
pub enum FileSetError {
    /// Validation failed.
    #[error("{0}")]
    Validation(#[from] ValidationError),

    /// Compilation failed.
    #[error("{0}")]
    Compile(#[from] CompileError),
}
