//! Version-control backends used by the runtime layer.
//!
//! Multorum stores runtime state on disk under `.multorum/`, but it
//! still relies on a repository backend to resolve revisions, create
//! worker worktrees, and integrate submitted changes. This module
//! defines that repository-facing contract so the runtime can swap Git
//! for another backend such as Jujutsu without reopening the storage
//! implementation.

mod commit;
mod git;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::runtime::RuntimeError;

pub use commit::CanonicalCommitHash;
pub use git::GitVcs;

/// Repository operations required by the Multorum runtime.
///
/// The current runtime persists [`CanonicalCommitHash`] values because
/// Git is the only implemented backend today. Future backends should
/// map their revision model onto the same stable persisted form until
/// the runtime state model is widened.
pub trait VersionControl: std::fmt::Debug + Send + Sync {
    /// Stable backend name used in diagnostics and tracing.
    fn backend_name(&self) -> &'static str;

    /// Resolve the repository root that owns `path`.
    ///
    /// Implementations should return `path` unchanged when no owned
    /// repository root can be discovered.
    fn repository_root(&self, path: &Path) -> PathBuf;

    /// Resolve one user-facing revision into the canonical persisted
    /// commit identifier used by the runtime.
    fn resolve_commit(
        &self, repo_root: &Path, revision: &str, operation: &'static str,
    ) -> Result<CanonicalCommitHash, RuntimeError>;

    /// Return the current `HEAD` commit for one repository view.
    fn head_commit(&self, repo_root: &Path) -> Result<CanonicalCommitHash, RuntimeError>;

    /// Return every changed path between two commits.
    fn changed_files(
        &self, repo_root: &Path, from: &CanonicalCommitHash, to: &CanonicalCommitHash,
    ) -> Result<BTreeSet<PathBuf>, RuntimeError>;

    /// Materialize a worker worktree at one pinned base commit.
    fn create_worktree(
        &self, workspace_root: &Path, worktree_root: &Path, base_commit: &CanonicalCommitHash,
    ) -> Result<(), RuntimeError>;

    /// Remove a previously-managed worker worktree.
    ///
    /// Returns `true` when the backend had an attached worktree entry
    /// to remove, even if the directory was already missing on disk.
    /// `multorum worker delete` relies on the backend instead of raw
    /// filesystem deletion so repository metadata stays consistent.
    fn remove_worktree(
        &self, workspace_root: &Path, worktree_root: &Path,
    ) -> Result<bool, RuntimeError>;

    /// Refuse integration when the canonical workspace already carries
    /// unrelated tracked modifications.
    fn ensure_clean_workspace(&self, workspace_root: &Path) -> Result<(), RuntimeError>;

    /// Integrate one submitted worker commit into the canonical
    /// workspace.
    fn integrate_commit(
        &self, workspace_root: &Path, commit: &CanonicalCommitHash,
    ) -> Result<(), RuntimeError>;

    /// Install backend-local ignore rules and mutation guards inside a
    /// worker worktree created during worker creation.
    fn install_worker_runtime_support(&self, worktree_root: &Path) -> Result<(), RuntimeError>;

    /// Install or update the orchestrator pre-commit hook in the
    /// canonical workspace.
    ///
    /// The hook reads the materialized exclusion set and rejects commits
    /// that touch any listed file.
    fn install_orchestrator_hook(&self, workspace_root: &Path) -> Result<(), RuntimeError>;

    /// Read one repository-relative file from a specific commit.
    fn show_file_at_commit(
        &self, workspace_root: &Path, commit: &CanonicalCommitHash, path: &Path,
    ) -> Result<String, RuntimeError>;

    /// List every repository-relative file visible at one commit.
    fn list_files_at_commit(
        &self, workspace_root: &Path, commit: &CanonicalCommitHash,
    ) -> Result<Vec<PathBuf>, RuntimeError>;
}
