//! Canonical filesystem paths for Multorum runtime data.
//!
//! These helpers centralize `.multorum/` path construction so the rest
//! of the runtime layer does not duplicate stringly-typed path logic.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::{MailboxDirection, RuntimeError, WorkerId};

/// Canonical gitignored directory name for managed worker worktrees.
const WORKTREE_DIRECTORY_NAME: &str = "tr";

/// Root path helper for a Multorum workspace.
#[derive(Debug, Clone)]
pub struct MultorumPaths {
    workspace_root: PathBuf,
}

impl MultorumPaths {
    /// Construct path helpers for a workspace root.
    ///
    /// Note: This type only centralizes deterministic path construction.
    /// Callers that require an absolute or canonical root must normalize
    /// `workspace_root` before constructing `MultorumPaths`.
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self { workspace_root: workspace_root.into() }
    }

    /// Construct path helpers from a canonicalized workspace root.
    pub fn new_canonical(workspace_root: impl Into<PathBuf>) -> std::io::Result<Self> {
        Ok(Self::new(workspace_root.into().canonicalize()?))
    }

    /// Workspace root path used for derived runtime locations.
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Path to the workspace `.multorum/` directory.
    pub fn multorum_root(&self) -> PathBuf {
        self.workspace_root.join(".multorum")
    }

    /// Path to the committed `.multorum/.gitignore`.
    pub fn multorum_gitignore(&self) -> PathBuf {
        self.multorum_root().join(".gitignore")
    }

    /// Path helper for orchestrator-local runtime state.
    pub fn orchestrator(&self) -> OrchestratorPaths {
        OrchestratorPaths::new(self.multorum_root().join("orchestrator"))
    }

    /// Audit log directory.
    ///
    /// Audit entries are append-only project history.
    /// Lives at `.multorum/audit/` and is tracked by version control.
    pub fn audit(&self) -> PathBuf {
        self.multorum_root().join("audit")
    }

    /// Audit entry for one merged worker.
    pub fn audit_entry(&self, worker_id: &WorkerId) -> PathBuf {
        self.audit().join(format!("{}.toml", worker_id.as_str()))
    }

    /// Path helper for the managed worker worktree.
    ///
    /// Note: The on-disk directory is abbreviated to `tr/` because the
    /// runtime creates these paths frequently and the shorter name
    /// keeps managed worktree paths compact.
    pub fn worker(&self, worker_id: &WorkerId) -> WorkerPaths {
        WorkerPaths::new(
            self.multorum_root().join(WORKTREE_DIRECTORY_NAME).join(worker_id.as_str()),
        )
    }
}

/// Paths under `.multorum/orchestrator/`.
#[derive(Debug, Clone)]
pub struct OrchestratorPaths {
    root: PathBuf,
}

impl OrchestratorPaths {
    /// Construct orchestrator path helpers from the runtime root.
    ///
    /// Note: This constructor does not canonicalize because the runtime
    /// directory is created lazily during initialization and worker
    /// creation.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Path to `.multorum/orchestrator/`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Orchestrator runtime state file.
    ///
    /// Records every bidding group and every worker within it. This is
    /// the single source of truth for runtime state — there are no
    /// per-worker state files.
    pub fn state(&self) -> PathBuf {
        self.root.join("state.toml")
    }

    /// Materialized orchestrator exclusion set.
    ///
    /// A flat projection of `state.toml`: the union of all read and
    /// write sets from groups that still have live workers. A pre-commit
    /// hook reads this file to reject orchestrator commits that touch
    /// protected files.
    pub fn exclusion_set(&self) -> PathBuf {
        self.root.join("exclusion-set.txt")
    }
}

/// Paths under a worker worktree.
#[derive(Debug, Clone)]
pub struct WorkerPaths {
    worktree_root: PathBuf,
}

impl WorkerPaths {
    /// Construct worker path helpers from the worktree root.
    ///
    /// Note: This constructor does not canonicalize because the managed
    /// worktree path is reserved before `git worktree add` creates it.
    pub fn new(worktree_root: impl Into<PathBuf>) -> Self {
        Self { worktree_root: worktree_root.into() }
    }

    /// Derive the canonical workspace root from a managed worker
    /// worktree path.
    pub(crate) fn workspace_root(&self) -> Result<PathBuf, RuntimeError> {
        let worktree_root = self.worktree_root.parent().ok_or_else(|| {
            RuntimeError::MissingWorkerRuntime(self.worktree_root.display().to_string())
        })?;
        let multorum_root = worktree_root.parent().ok_or_else(|| {
            RuntimeError::MissingWorkerRuntime(self.worktree_root.display().to_string())
        })?;
        let workspace_root = multorum_root.parent().ok_or_else(|| {
            RuntimeError::MissingWorkerRuntime(self.worktree_root.display().to_string())
        })?;

        if worktree_root.file_name() != Some(OsStr::new(WORKTREE_DIRECTORY_NAME))
            || multorum_root.file_name() != Some(OsStr::new(".multorum"))
        {
            return Err(RuntimeError::MissingWorkerRuntime(
                self.worktree_root.display().to_string(),
            ));
        }

        Ok(workspace_root.to_path_buf())
    }

    /// Path to the worker worktree root.
    pub fn worktree_root(&self) -> &Path {
        &self.worktree_root
    }

    /// Path to the worker-local `.multorum/` directory.
    pub fn multorum_root(&self) -> PathBuf {
        self.worktree_root.join(".multorum")
    }

    /// Runtime contract path.
    pub fn contract(&self) -> PathBuf {
        self.multorum_root().join("contract.toml")
    }

    /// Compiled read set path.
    pub fn read_set(&self) -> PathBuf {
        self.multorum_root().join("read-set.txt")
    }

    /// Compiled write set path.
    pub fn write_set(&self) -> PathBuf {
        self.multorum_root().join("write-set.txt")
    }

    /// Worker inbox root.
    pub fn inbox(&self) -> PathBuf {
        self.multorum_root().join("inbox")
    }

    /// Worker outbox root.
    pub fn outbox(&self) -> PathBuf {
        self.multorum_root().join("outbox")
    }

    /// Mailbox root for a direction relative to the worker.
    pub fn mailbox(&self, direction: MailboxDirection) -> PathBuf {
        match direction {
            | MailboxDirection::Inbox => self.inbox(),
            | MailboxDirection::Outbox => self.outbox(),
        }
    }

    /// Pending inbox bundles.
    pub fn inbox_new(&self) -> PathBuf {
        self.inbox().join("new")
    }

    /// Inbox acknowledgement directory.
    pub fn inbox_ack(&self) -> PathBuf {
        self.inbox().join("ack")
    }

    /// Pending outbox bundles.
    pub fn outbox_new(&self) -> PathBuf {
        self.outbox().join("new")
    }

    /// Outbox acknowledgement directory.
    pub fn outbox_ack(&self) -> PathBuf {
        self.outbox().join("ack")
    }

    /// Pending bundles for a mailbox direction.
    pub fn mailbox_new(&self, direction: MailboxDirection) -> PathBuf {
        self.mailbox(direction).join("new")
    }

    /// Acknowledgement directory for a mailbox direction.
    pub fn mailbox_ack(&self, direction: MailboxDirection) -> PathBuf {
        self.mailbox(direction).join("ack")
    }

    /// Runtime artifacts root.
    pub fn artifacts(&self) -> PathBuf {
        self.multorum_root().join("artifacts")
    }
}
