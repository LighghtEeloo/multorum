//! Canonical filesystem paths for Multorum runtime data.
//!
//! These helpers centralize `.multorum/` path construction so the rest
//! of the runtime layer does not duplicate stringly-typed path logic.

use std::path::{Path, PathBuf};

use crate::perspective::PerspectiveName;

/// Root path helper for a Multorum workspace.
#[derive(Debug, Clone)]
pub struct MultorumPaths {
    workspace_root: PathBuf,
}

impl MultorumPaths {
    /// Construct path helpers for a workspace root.
    pub fn new(workspace_root: impl Into<PathBuf>) -> std::io::Result<Self> {
        Ok(Self { workspace_root: workspace_root.into().canonicalize()? })
    }

    /// Absolute path to the workspace root.
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Absolute path to the workspace `.multorum/` directory.
    pub fn multorum_root(&self) -> PathBuf {
        self.workspace_root.join(".multorum")
    }

    /// Absolute path helper for orchestrator-local runtime state.
    pub fn orchestrator(&self) -> std::io::Result<OrchestratorPaths> {
        OrchestratorPaths::new(self.multorum_root().join("orchestrator"))
    }

    /// Absolute path helper for the managed worker worktree.
    pub fn worker(&self, perspective: &PerspectiveName) -> std::io::Result<WorkerPaths> {
        WorkerPaths::new(self.multorum_root().join("worktrees").join(perspective.as_str()))
    }
}

/// Paths under `.multorum/orchestrator/`.
#[derive(Debug, Clone)]
pub struct OrchestratorPaths {
    root: PathBuf,
}

impl OrchestratorPaths {
    /// Construct orchestrator path helpers from the runtime root.
    pub fn new(root: impl Into<PathBuf>) -> std::io::Result<Self> {
        Ok(Self { root: root.into().canonicalize()? })
    }

    /// Absolute path to `.multorum/orchestrator/`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Active rulebook commit projection.
    pub fn active_rulebook(&self) -> PathBuf {
        self.root.join("active-rulebook.toml")
    }

    /// Worker state projection directory.
    pub fn workers(&self) -> PathBuf {
        self.root.join("workers")
    }

    /// Worker-specific projection directory.
    pub fn worker(&self, perspective: &PerspectiveName) -> PathBuf {
        self.workers().join(perspective.as_str())
    }

    /// Audit log directory.
    pub fn audit(&self) -> PathBuf {
        self.root.join("audit")
    }
}

/// Paths under a worker worktree.
#[derive(Debug, Clone)]
pub struct WorkerPaths {
    worktree_root: PathBuf,
}

impl WorkerPaths {
    /// Construct worker path helpers from the worktree root.
    pub fn new(worktree_root: impl Into<PathBuf>) -> std::io::Result<Self> {
        Ok(Self { worktree_root: worktree_root.into().canonicalize()? })
    }

    /// Absolute path to the worker worktree root.
    pub fn worktree_root(&self) -> &Path {
        &self.worktree_root
    }

    /// Absolute path to the worker-local `.multorum/` directory.
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

    /// Runtime artifacts root.
    pub fn artifacts(&self) -> PathBuf {
        self.multorum_root().join("artifacts")
    }
}
