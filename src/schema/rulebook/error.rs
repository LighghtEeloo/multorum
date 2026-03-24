//! Error types for rulebook loading, validation, and compilation.

use thiserror::Error;

use super::check::CheckName;

/// Errors produced when constructing a [`CheckName`](super::CheckName).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CheckNameError {
    /// The input string was empty.
    #[error("check name is empty")]
    Empty,

    /// The first character is not a lowercase ASCII letter.
    #[error("check name `{name}` must start with a lowercase ASCII letter")]
    InvalidStart { name: String },

    /// A non-alphanumeric character was found.
    #[error("check name `{name}` contains invalid character `{ch}` at byte {pos}")]
    InvalidChar { name: String, ch: char, pos: usize },
}

/// Errors produced while validating the check pipeline in a rulebook.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CheckValidationError {
    /// A pipeline entry names a check that is not declared.
    #[error("check pipeline references undefined check `{name}`")]
    UndefinedPipelineEntry { name: CheckName },

    /// A check name appears more than once in the pipeline.
    #[error("check pipeline contains duplicate check `{name}`")]
    DuplicatePipelineEntry { name: CheckName },

    /// A declared check does not appear in the pipeline.
    #[error("declared check `{name}` is not present in the pipeline")]
    UnusedDeclaredCheck { name: CheckName },

    /// A declared check has an empty command string after trimming.
    #[error("check `{name}` has an empty command")]
    EmptyCommand { name: CheckName },
}

/// Top-level error for rulebook loading and compilation.
#[derive(Debug, Error)]
pub enum RulebookError {
    /// The provided bytes were not valid UTF-8.
    #[error(transparent)]
    Utf8(#[from] std::str::Utf8Error),

    /// Rulebook file I/O failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Rulebook TOML decoding failed.
    #[error(transparent)]
    TomlDecode(#[from] toml::de::Error),

    /// File set compilation failed.
    #[error(transparent)]
    FileSet(#[from] crate::schema::fileset::FileSetError),

    /// Perspective compilation failed.
    #[error(transparent)]
    Perspective(#[from] crate::schema::perspective::PerspectiveError),

    /// Check pipeline validation failed.
    #[error(transparent)]
    CheckValidation(#[from] CheckValidationError),
}
