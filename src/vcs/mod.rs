//! Version-control backends used by the runtime layer.
//!
//! Multorum stores runtime state on disk under `.multorum/`, but it
//! still relies on a repository backend to resolve revisions, create
//! worker worktrees, and integrate submitted changes. This module
//! defines that repository-facing contract so the runtime can swap Git
//! for another backend such as Jujutsu without reopening the storage
//! implementation.

mod commit;
pub mod error;
mod git;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use error::Result;

pub use commit::CanonicalCommitHash;
pub use error::VcsError;
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
    ) -> Result<CanonicalCommitHash>;

    /// Return the current `HEAD` commit for one repository view.
    fn head_commit(&self, repo_root: &Path) -> Result<CanonicalCommitHash>;

    /// Return every changed path between two commits.
    fn changed_files(
        &self, repo_root: &Path, from: &CanonicalCommitHash, to: &CanonicalCommitHash,
    ) -> Result<BTreeSet<PathBuf>>;

    /// Materialize a worker worktree at one pinned base commit.
    fn create_worktree(
        &self, workspace_root: &Path, worktree_root: &Path, base_commit: &CanonicalCommitHash,
    ) -> Result<()>;

    /// Remove a previously-managed worker worktree.
    ///
    /// Returns `true` when the backend had an attached worktree entry
    /// to remove, even if the directory was already missing on disk.
    /// `multorum worker delete` relies on the backend instead of raw
    /// filesystem deletion so repository metadata stays consistent.
    fn remove_worktree(&self, workspace_root: &Path, worktree_root: &Path) -> Result<bool>;

    /// Refuse integration when the canonical workspace already carries
    /// unrelated tracked modifications.
    fn ensure_clean_workspace(&self, workspace_root: &Path) -> Result<()>;

    /// Refuse worker forwarding when the worktree carries staged,
    /// unstaged, or untracked changes.
    fn ensure_clean_worktree(&self, worktree_root: &Path) -> Result<()>;

    /// Integrate one submitted worker commit into the canonical
    /// workspace.
    fn integrate_commit(&self, workspace_root: &Path, commit: &CanonicalCommitHash) -> Result<()>;

    /// Move a detached worktree head to a specific commit without
    /// changing persisted runtime metadata.
    fn checkout_detached(&self, worktree_root: &Path, commit: &CanonicalCommitHash) -> Result<()>;

    /// Replay the current detached worktree commit range from
    /// `from_base` onto `to_base`.
    ///
    /// Implementations should leave the worktree detached at the
    /// replayed head and return that new canonical commit hash.
    fn forward_worktree(
        &self, worktree_root: &Path, from_base: &CanonicalCommitHash, to_base: &CanonicalCommitHash,
    ) -> Result<CanonicalCommitHash>;

    /// Install backend-local ignore rules and mutation guards inside a
    /// worker worktree created during worker creation.
    fn install_worker_runtime_support(&self, worktree_root: &Path) -> Result<()>;

    /// Install or update the orchestrator pre-commit hook in the
    /// canonical workspace.
    ///
    /// The hook reads the materialized exclusion set and rejects commits
    /// that touch any listed file.
    fn install_orchestrator_hook(&self, workspace_root: &Path) -> Result<()>;

    /// Read one repository-relative file from a specific commit.
    fn show_file_at_commit(
        &self, workspace_root: &Path, commit: &CanonicalCommitHash, path: &Path,
    ) -> Result<String>;

    /// List every repository-relative file visible at one commit.
    fn list_files_at_commit(
        &self, workspace_root: &Path, commit: &CanonicalCommitHash,
    ) -> Result<Vec<PathBuf>>;
}
