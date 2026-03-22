//! Runtime errors for Multorum orchestration services.

use std::path::PathBuf;

use thiserror::Error;

use crate::perspective::PerspectiveName;
use crate::runtime::WorkerId;
use crate::vcs::CanonicalCommitHash;

use super::state::WorkerState;

/// Result alias for runtime operations.
pub type Result<T> = std::result::Result<T, RuntimeError>;

/// Errors produced by the runtime application layer.
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// The runtime has not activated a rulebook yet.
    #[error("no active rulebook; run `multorum rulebook switch <commit>` first")]
    MissingActiveRulebook,

    /// The workspace already has a committed rulebook.
    #[error("rulebook already exists at {0}")]
    RulebookExists(std::path::PathBuf),

    /// The requested perspective does not exist in the active rulebook
    /// or runtime state.
    #[error("unknown perspective: {0}")]
    UnknownPerspective(String),

    /// The requested worker does not exist in runtime state.
    #[error("unknown worker: {0}")]
    UnknownWorker(String),

    /// The requested worker id is already present in runtime state.
    #[error("worker id already exists: {0}")]
    WorkerIdExists(WorkerId),

    /// The worker state machine does not permit the requested action.
    #[error(
        "{operation} requires worker state {expected}; found {actual}",
        actual = worker_state_name(*actual)
    )]
    InvalidState {
        /// Operation that rejected the current state.
        operation: &'static str,
        /// Worker state or state set required by the operation.
        expected: &'static str,
        /// Projected worker state observed at the time of the failure.
        actual: WorkerState,
    },

    /// The requested message bundle does not exist.
    #[error("message not found")]
    MessageNotFound,

    /// The requested message has already been acknowledged.
    #[error("message already acknowledged")]
    AlreadyAcknowledged,

    /// The requested rulebook switch conflicts with active workers.
    #[error(
        "cannot activate rulebook commit `{commit}` while active perspectives are still live: {blocking_perspectives}",
        blocking_perspectives = format_perspectives(blocking_perspectives)
    )]
    RulebookConflict {
        /// Canonical rulebook commit the caller attempted to activate.
        commit: CanonicalCommitHash,
        /// Live perspectives that still depend on the current rulebook.
        blocking_perspectives: Vec<PerspectiveName>,
    },

    /// A candidate bidding group conflicts with active runtime state.
    #[error(
        "cannot provision perspective `{perspective}` because active perspective `{blocking_perspective}` has a {relation}: {files}",
        files = format_paths(files)
    )]
    ConflictWithActiveBiddingGroup {
        /// Perspective being provisioned.
        perspective: PerspectiveName,
        /// Active perspective that blocks the candidate boundary.
        blocking_perspective: PerspectiveName,
        /// Human-readable description of the overlap relation.
        relation: &'static str,
        /// Overlapping files.
        files: Vec<PathBuf>,
    },

    /// A worker attempted to join an existing bidding group with a
    /// different compiled boundary.
    #[error(
        "compiled boundary for perspective `{perspective}` no longer matches its active bidding group"
    )]
    BiddingGroupBoundaryMismatch {
        /// Perspective whose compiled boundary drifted from runtime.
        perspective: PerspectiveName,
    },

    /// A pre-merge or lifecycle check failed.
    #[error("check failed: {0}")]
    CheckFailed(String),

    /// The worker touched files outside its compiled write set.
    #[error(
        "write-set violation for worker `{worker_id}` (`{perspective}`) between `{base_commit}` and `{head_commit}`: {violations}",
        violations = format_paths(violations)
    )]
    WriteSetViolation {
        /// Worker whose submission touched unauthorized paths.
        worker_id: WorkerId,
        /// Perspective instantiated by the worker.
        perspective: PerspectiveName,
        /// Canonical base commit from which the worker was provisioned.
        base_commit: CanonicalCommitHash,
        /// Canonical submitted worker head commit.
        head_commit: CanonicalCommitHash,
        /// Paths changed outside the compiled write set.
        violations: Vec<PathBuf>,
    },

    /// A mailbox operation observed inconsistent or conflicting state.
    #[error("mailbox conflict")]
    MailboxConflict,

    /// The runtime surface for the requested perspective was not found.
    #[error("worker runtime is missing for perspective: {0}")]
    MissingWorkerRuntime(String),

    /// A worker submission expected a recorded head commit, but the
    /// worker record did not contain one.
    #[error(
        "worker `{worker_id}` is in state {state} but has no submitted head commit recorded",
        state = worker_state_name(*state)
    )]
    MissingSubmittedHeadCommit {
        /// Worker whose committed submission lost its recorded head.
        worker_id: WorkerId,
        /// Worker state observed when the missing head was detected.
        state: WorkerState,
    },

    /// A worker worktree moved away from the submitted commit before
    /// integration started.
    #[error(
        "worker `{worker_id}` head changed after submission: submitted `{submitted_head_commit}`, current `{current_head_commit}`"
    )]
    WorkerHeadMismatch {
        /// Worker whose worktree head changed unexpectedly.
        worker_id: WorkerId,
        /// Canonical commit hash recorded in the worker submission.
        submitted_head_commit: CanonicalCommitHash,
        /// Canonical commit hash currently checked out in the worker worktree.
        current_head_commit: CanonicalCommitHash,
    },

    /// A referenced commit is not reachable from the repository view
    /// used for one operation.
    #[error(
        "cannot {operation}: commit `{commit}` is not available from `{worktree_root}` ({details})",
        worktree_root = worktree_root.display()
    )]
    CommitNotFound {
        /// Operation that required the commit to exist.
        operation: &'static str,
        /// Repository or worktree root used to resolve the commit.
        worktree_root: PathBuf,
        /// Commit hash that could not be resolved.
        commit: String,
        /// Git-provided failure details.
        details: String,
    },

    /// The operation is intentionally stubbed during the current
    /// scaffolding phase.
    #[error("operation is not implemented yet: {0}")]
    Unimplemented(&'static str),

    /// The caller supplied an invalid bundle payload.
    #[error("invalid bundle payload: {0}")]
    InvalidPayload(&'static str),

    /// Repository backend command execution failed.
    #[error(
        "{backend} command failed while attempting to {action} in `{cwd}`: {details}",
        cwd = cwd.display()
    )]
    Vcs {
        /// Repository backend that reported the failure.
        backend: &'static str,
        /// Human-readable description of the repository action.
        action: &'static str,
        /// Working directory used for the repository command.
        cwd: PathBuf,
        /// Backend-provided failure details.
        details: String,
    },

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

fn worker_state_name(state: WorkerState) -> &'static str {
    match state {
        | WorkerState::Active => "ACTIVE",
        | WorkerState::Blocked => "BLOCKED",
        | WorkerState::Committed => "COMMITTED",
        | WorkerState::Merged => "MERGED",
        | WorkerState::Discarded => "DISCARDED",
    }
}

fn format_perspectives(perspectives: &[PerspectiveName]) -> String {
    perspectives.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
}

fn format_paths(paths: &[PathBuf]) -> String {
    paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join(", ")
}
