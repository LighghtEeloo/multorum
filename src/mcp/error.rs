//! MCP-facing error mapping.

use crate::runtime::RuntimeError;

/// Stable MCP-facing error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpErrorCode {
    /// Unknown perspective identifier.
    UnknownPerspective,
    /// Unknown worker identifier.
    UnknownWorker,
    /// Invalid state transition for the requested operation.
    InvalidState,
    /// Message bundle not found.
    MessageNotFound,
    /// Message bundle already acknowledged.
    AlreadyAcknowledged,
    /// Rulebook switch conflicts with active workers.
    RulebookConflict,
    /// Requested check failed.
    CheckFailed,
    /// Worker touched files outside its write set.
    WriteSetViolation,
    /// Mailbox state was inconsistent.
    MailboxConflict,
    /// Requested worker runtime was missing.
    MissingWorkerRuntime,
    /// The requested operation is not implemented.
    Unimplemented,
    /// Unexpected internal failure.
    Internal,
}

/// MCP-facing tool error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolError {
    /// Stable machine-readable error code.
    pub code: McpErrorCode,
    /// Human-readable error message.
    pub message: String,
}

impl From<RuntimeError> for McpToolError {
    fn from(value: RuntimeError) -> Self {
        let code = match value {
            | RuntimeError::MissingActiveRulebook => McpErrorCode::InvalidState,
            | RuntimeError::RulebookExists(_) => McpErrorCode::InvalidState,
            | RuntimeError::UnknownPerspective(_) => McpErrorCode::UnknownPerspective,
            | RuntimeError::UnknownWorker(_) => McpErrorCode::UnknownWorker,
            | RuntimeError::InvalidState { .. } => McpErrorCode::InvalidState,
            | RuntimeError::MessageNotFound => McpErrorCode::MessageNotFound,
            | RuntimeError::AlreadyAcknowledged => McpErrorCode::AlreadyAcknowledged,
            | RuntimeError::RulebookConflict { .. } => McpErrorCode::RulebookConflict,
            | RuntimeError::SafetyConflict { .. }
            | RuntimeError::BiddingGroupBoundaryMismatch { .. }
            | RuntimeError::CheckFailed(_) => McpErrorCode::CheckFailed,
            | RuntimeError::WriteSetViolation { .. } => McpErrorCode::WriteSetViolation,
            | RuntimeError::MailboxConflict => McpErrorCode::MailboxConflict,
            | RuntimeError::MissingWorkerRuntime(_) => McpErrorCode::MissingWorkerRuntime,
            | RuntimeError::Unimplemented(_) => McpErrorCode::Unimplemented,
            | RuntimeError::InvalidPayload(_)
            | RuntimeError::MissingSubmittedHeadCommit { .. }
            | RuntimeError::WorkerHeadMismatch { .. }
            | RuntimeError::CommitNotFound { .. }
            | RuntimeError::Vcs { .. }
            | RuntimeError::Rulebook(_)
            | RuntimeError::Io(_)
            | RuntimeError::TomlDecode(_)
            | RuntimeError::TomlEncode(_) => McpErrorCode::Internal,
        };
        Self { code, message: value.to_string() }
    }
}
