//! Version-control backend errors.
//!
//! `VcsError` captures failures that originate in the repository
//! backend layer. The runtime wraps it as `RuntimeError::Vcs` so
//! higher layers never import this module directly.

use std::path::PathBuf;

use thiserror::Error;

/// Result alias for version-control operations.
pub type Result<T> = std::result::Result<T, VcsError>;

/// Errors produced by the version-control backend.
#[derive(Debug, Error)]
pub enum VcsError {
    /// A backend command execution failed.
    #[error(
        "{backend} command failed while attempting to {action} in `{cwd}`: {details}",
        cwd = cwd.display()
    )]
    CommandFailed {
        /// Repository backend that reported the failure.
        backend: &'static str,
        /// Human-readable description of the repository action.
        action: &'static str,
        /// Working directory used for the repository command.
        cwd: PathBuf,
        /// Backend-provided failure details.
        details: String,
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

    /// The workspace has uncommitted tracked changes that block an
    /// operation requiring a clean state.
    #[error("workspace has uncommitted tracked changes: {changed_paths}")]
    DirtyWorkspace {
        /// Human-readable summary of the changed paths.
        changed_paths: String,
    },

    /// Filesystem I/O failure within the backend.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
