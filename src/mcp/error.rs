//! MCP-facing error mapping.

use crate::runtime::RuntimeError;

/// Stable MCP-facing error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpErrorCode {
    /// Unknown perspective identifier.
    UnknownPerspective,
    /// Unknown worker identifier.
    UnknownWorker,
    /// Requested worker id is already allocated.
    WorkerIdExists,
    /// Invalid state transition for the requested operation.
    InvalidState,
    /// Message bundle not found.
    MessageNotFound,
    /// Message bundle already acknowledged.
    AlreadyAcknowledged,
    /// Rulebook install or uninstall conflicts with active workers.
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

impl McpErrorCode {
    /// Return the stable wire representation of the error code.
    pub const fn as_str(self) -> &'static str {
        match self {
            | Self::UnknownPerspective => "unknown_perspective",
            | Self::UnknownWorker => "unknown_worker",
            | Self::WorkerIdExists => "worker_id_exists",
            | Self::InvalidState => "invalid_state",
            | Self::MessageNotFound => "message_not_found",
            | Self::AlreadyAcknowledged => "already_acknowledged",
            | Self::RulebookConflict => "rulebook_conflict",
            | Self::CheckFailed => "check_failed",
            | Self::WriteSetViolation => "write_set_violation",
            | Self::MailboxConflict => "mailbox_conflict",
            | Self::MissingWorkerRuntime => "missing_worker_runtime",
            | Self::Unimplemented => "unimplemented",
            | Self::Internal => "internal",
        }
    }
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
            | RuntimeError::UnmanagedProject(_) => McpErrorCode::InvalidState,
            | RuntimeError::AmbiguousRuntimeRole { .. } => McpErrorCode::InvalidState,
            | RuntimeError::RuntimeRoleMismatch { .. } => McpErrorCode::InvalidState,
            | RuntimeError::MissingActiveRulebook => McpErrorCode::InvalidState,
            | RuntimeError::RulebookExists(_) => McpErrorCode::InvalidState,
            | RuntimeError::UnknownPerspective(_) => McpErrorCode::UnknownPerspective,
            | RuntimeError::UnknownWorker(_) => McpErrorCode::UnknownWorker,
            | RuntimeError::WorkerIdExists(_) => McpErrorCode::WorkerIdExists,
            | RuntimeError::ExistingWorkerWorkspace { .. } => McpErrorCode::InvalidState,
            | RuntimeError::InvalidState { .. } => McpErrorCode::InvalidState,
            | RuntimeError::MessageNotFound => McpErrorCode::MessageNotFound,
            | RuntimeError::AlreadyAcknowledged => McpErrorCode::AlreadyAcknowledged,
            | RuntimeError::RulebookConflict { .. }
            | RuntimeError::ActivePerspectiveIncompatible { .. } => McpErrorCode::RulebookConflict,
            | RuntimeError::ConflictWithActiveBiddingGroup { .. }
            | RuntimeError::BiddingGroupBoundaryMismatch { .. }
            | RuntimeError::BiddingGroupBaseMismatch { .. }
            | RuntimeError::PerspectiveForwardRequiresBlocked { .. }
            | RuntimeError::PerspectiveForwardMissingGroup { .. }
            | RuntimeError::PerspectiveForwardMissingReport { .. }
            | RuntimeError::PerspectiveForwardMissingReportedHead { .. }
            | RuntimeError::PerspectiveForwardHeadMismatch { .. }
            | RuntimeError::PerspectiveRequiresForwardBeforeCreate { .. }
            | RuntimeError::CheckFailed(_) => McpErrorCode::CheckFailed,
            | RuntimeError::WriteSetViolation { .. } => McpErrorCode::WriteSetViolation,
            | RuntimeError::MailboxConflict => McpErrorCode::MailboxConflict,
            | RuntimeError::MissingWorkerRuntime(_) => McpErrorCode::MissingWorkerRuntime,
            | RuntimeError::Unimplemented(_) => McpErrorCode::Unimplemented,
            | RuntimeError::Bundle(_)
            | RuntimeError::MissingSubmittedHeadCommit { .. }
            | RuntimeError::NoNewCommit { .. }
            | RuntimeError::WorkerHeadMismatch { .. }
            | RuntimeError::Vcs(_)
            | RuntimeError::Rulebook(_)
            | RuntimeError::Io(_)
            | RuntimeError::TomlDecode(_)
            | RuntimeError::TomlEncode(_) => McpErrorCode::Internal,
        };
        Self { code, message: value.to_string() }
    }
}
