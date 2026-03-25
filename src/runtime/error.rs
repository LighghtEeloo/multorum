//! Runtime errors for Multorum orchestration services.

use std::path::PathBuf;

use thiserror::Error;

use crate::runtime::WorkerId;
use crate::schema::perspective::PerspectiveName;
use crate::vcs::CanonicalCommitHash;

use super::state::WorkerState;

/// Result alias for runtime operations.
pub type Result<T> = std::result::Result<T, RuntimeError>;

/// Errors produced by the runtime application layer.
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// The current repository is not managed by Multorum.
    #[error("current repository is not a Multorum-managed project: `{0}`")]
    UnmanagedProject(PathBuf),

    /// The runtime markers for the current repository disagree.
    #[error("cannot determine Multorum runtime role for `{repo_root}`: {details}", repo_root = repo_root.display())]
    AmbiguousRuntimeRole {
        /// Repository root whose runtime markers disagree.
        repo_root: PathBuf,
        /// Short explanation of the conflicting markers.
        details: &'static str,
    },

    /// The caller requested the wrong runtime surface for the current repository.
    #[error(
        "current repository `{repo_root}` uses the {actual} runtime, not the {expected} runtime; {hint}",
        repo_root = repo_root.display(),
        hint = runtime_role_mismatch_hint(expected, actual)
    )]
    RuntimeRoleMismatch {
        /// Runtime role required by the caller.
        expected: &'static str,
        /// Runtime role discovered for the current repository.
        actual: &'static str,
        /// Repository root used for role detection.
        repo_root: PathBuf,
    },

    /// The runtime has not activated a rulebook yet.
    #[error("no active rulebook; run `multorum rulebook install` first")]
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

    /// The requested worker id is already held by a live worker.
    #[error("worker id already belongs to a live worker: {0}")]
    WorkerIdExists(WorkerId),

    /// A finalized worker still has a preserved workspace at the
    /// managed path for the requested id.
    #[error(
        "worker `{worker_id}` already has a preserved {state} workspace at `{worktree_path}`; delete it first or request overwrite",
        state = worker_state_name(*state),
        worktree_path = worktree_path.display()
    )]
    ExistingWorkerWorkspace {
        /// Worker whose preserved finalized workspace still exists.
        worker_id: WorkerId,
        /// Finalized state currently recorded for the worker.
        state: WorkerState,
        /// Managed path to the preserved workspace.
        worktree_path: PathBuf,
    },

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

    /// The requested rulebook install or uninstall conflicts with active workers.
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

    /// An active bidding group's perspective is missing or reduced in
    /// the target rulebook.
    #[error(
        "cannot activate rulebook commit `{commit}`: active perspective `{perspective}` {reason}"
    )]
    ActivePerspectiveIncompatible {
        /// Canonical rulebook commit the caller attempted to activate.
        commit: CanonicalCommitHash,
        /// Active perspective that is incompatible with the target.
        perspective: PerspectiveName,
        /// Human-readable explanation of the incompatibility.
        reason: &'static str,
    },

    /// A candidate bidding group conflicts with active runtime state.
    #[error(
        "cannot create worker for perspective `{perspective}` because active perspective `{blocking_perspective}` has a {relation}: {files}",
        files = format_paths(files)
    )]
    ConflictWithActiveBiddingGroup {
        /// Perspective being created.
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
        /// Canonical base commit from which the worker was created.
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

    /// A worker submission head commit is the same as its base commit,
    /// indicating no new work was done.
    #[error(
        "worker `{worker_id}` has no new commits to merge (head `{head_commit}` is the same as base)"
    )]
    NoNewCommit {
        /// Worker with no new commits.
        worker_id: WorkerId,
        /// The commit that was supposed to be new but wasn't.
        head_commit: CanonicalCommitHash,
    },

    /// The operation is intentionally stubbed during the current
    /// scaffolding phase.
    #[error("operation is not implemented yet: {0}")]
    Unimplemented(&'static str),

    /// The caller supplied an invalid bundle payload.
    #[error("invalid bundle payload: {0}")]
    InvalidPayload(&'static str),

    /// Version-control backend failure.
    #[error(transparent)]
    Vcs(#[from] crate::vcs::VcsError),

    /// Rulebook loading or compilation failed.
    #[error(transparent)]
    Rulebook(#[from] crate::schema::rulebook::RulebookError),

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

/// Return the most useful CLI hint for one runtime-role mismatch.
fn runtime_role_mismatch_hint(expected: &str, actual: &str) -> &'static str {
    match (expected, actual) {
        | ("worker", "orchestrator") => {
            "this looks like running `local` command in orchestrator workspace"
        }
        | ("orchestrator", "worker") => {
            "this looks like running `worker` command in worker workspace"
        }
        | _ => "the current workspace does not support that runtime operation",
    }
}

fn format_perspectives(perspectives: &[PerspectiveName]) -> String {
    perspectives.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
}

fn format_paths(paths: &[PathBuf]) -> String {
    paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::RuntimeError;

    #[test]
    fn runtime_role_mismatch_mentions_local_command_in_orchestrator_workspace() {
        let error = RuntimeError::RuntimeRoleMismatch {
            expected: "worker",
            actual: "orchestrator",
            repo_root: PathBuf::from("/repo"),
        };

        assert_eq!(
            error.to_string(),
            "current repository `/repo` uses the orchestrator runtime, not the worker runtime; this looks like running `local` command in orchestrator workspace"
        );
    }

    #[test]
    fn runtime_role_mismatch_mentions_worker_command_in_worker_workspace() {
        let error = RuntimeError::RuntimeRoleMismatch {
            expected: "orchestrator",
            actual: "worker",
            repo_root: PathBuf::from("/repo"),
        };

        assert_eq!(
            error.to_string(),
            "current repository `/repo` uses the worker runtime, not the orchestrator runtime; this looks like running `worker` command in worker workspace"
        );
    }
}
