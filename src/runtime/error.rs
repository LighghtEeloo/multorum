//! Runtime errors for Multorum orchestration services.

use thiserror::Error;

/// Result alias for runtime operations.
pub type Result<T> = std::result::Result<T, RuntimeError>;

/// Errors produced by the runtime application layer.
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// The runtime has not activated a rulebook yet.
    #[error("no active rulebook; run `multorum rulebook switch <commit>` first")]
    MissingActiveRulebook,

    /// The requested perspective does not exist in the active rulebook
    /// or runtime state.
    #[error("unknown perspective: {0}")]
    UnknownPerspective(String),

    /// The worker state machine does not permit the requested action.
    #[error("invalid worker state for operation")]
    InvalidState,

    /// The requested message bundle does not exist.
    #[error("message not found")]
    MessageNotFound,

    /// The requested message has already been acknowledged.
    #[error("message already acknowledged")]
    AlreadyAcknowledged,

    /// The requested rulebook switch conflicts with active workers.
    #[error("rulebook switch conflicts with active workers")]
    RulebookConflict,

    /// A pre-merge or lifecycle check failed.
    #[error("check failed: {0}")]
    CheckFailed(String),

    /// The worker touched files outside its compiled write set.
    #[error("write set violation")]
    WriteSetViolation,

    /// A mailbox operation observed inconsistent or conflicting state.
    #[error("mailbox conflict")]
    MailboxConflict,

    /// The runtime surface for the requested perspective was not found.
    #[error("worker runtime is missing for perspective: {0}")]
    MissingWorkerRuntime(String),

    /// The operation is intentionally stubbed during the current
    /// scaffolding phase.
    #[error("operation is not implemented yet: {0}")]
    Unimplemented(&'static str),

    /// The caller supplied an invalid bundle payload.
    #[error("invalid bundle payload: {0}")]
    InvalidPayload(&'static str),

    /// The current worktree does not belong to the named perspective.
    #[error("worker perspective mismatch: expected `{expected}`, found `{found}`")]
    PerspectiveMismatch { expected: String, found: String },

    /// Git command execution failed.
    #[error("git command failed: {0}")]
    Git(String),

    /// Rulebook loading or compilation failed.
    #[error(transparent)]
    Rulebook(#[from] crate::rulebook::RulebookError),

    /// Filesystem I/O failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// TOML decoding failure.
    #[error(transparent)]
    TomlDecode(#[from] toml::de::Error),

    /// TOML encoding failure.
    #[error(transparent)]
    TomlEncode(#[from] toml::ser::Error),
}
