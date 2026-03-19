//! Filesystem-backed runtime helpers shared by the service layer.
//!
//! The runtime model in `DESIGN.md` is intentionally filesystem-first:
//! `.multorum/` stores the authoritative control plane, worker contract,
//! compiled file sets, and mailbox bundles. These helpers centralize the
//! on-disk layout and the small amount of git orchestration needed to
//! provision worktrees and integrate submitted commits.

mod git;
mod state;

use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::perspective::PerspectiveName;
use crate::rulebook::{CheckName, CheckPolicy, CompiledRulebook};
use crate::runtime::{
    MessageKind, MultorumPaths, RuntimeError, Sequence, WorkerPaths, WorkerState,
};

pub(crate) const PROTOCOL_VERSION: u32 = 1;

const STATE_FILE_NAME: &str = "state.toml";
pub(crate) const ENVELOPE_FILE_NAME: &str = "envelope.toml";
pub(crate) const BODY_FILE_NAME: &str = "body.md";
pub(crate) const ARTIFACTS_DIR_NAME: &str = "artifacts";
pub(crate) const ACK_EXTENSION: &str = "ack";

/// Filesystem-backed runtime access rooted at the canonical workspace.
#[derive(Debug, Clone)]
pub(crate) struct RuntimeFileSystem {
    paths: MultorumPaths,
}

impl RuntimeFileSystem {
    /// Build runtime helpers for the canonical workspace root.
    pub(crate) fn new(workspace_root: impl Into<PathBuf>) -> Result<Self, RuntimeError> {
        Ok(Self { paths: MultorumPaths::new_canonical(workspace_root.into())? })
    }

    /// The canonical workspace root.
    pub(crate) fn workspace_root(&self) -> &Path {
        self.paths.workspace_root()
    }

    /// Deterministic worktree-local runtime paths for one perspective.
    pub(crate) fn worker_paths(&self, perspective: &PerspectiveName) -> WorkerPaths {
        self.paths.worker(perspective)
    }
}

/// Active rulebook projection stored under `.multorum/orchestrator/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ActiveRulebookRecord {
    /// Git commit that owns the active committed rulebook.
    pub rulebook_commit: String,
    /// Pinned base commit for newly provisioned workers.
    pub base_commit: String,
    /// Activation timestamp.
    pub activated_at: String,
}

/// Orchestrator-local projection for one provisioned worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WorkerRecord {
    /// Perspective currently held by the worker.
    pub perspective: PerspectiveName,
    /// Current lifecycle state.
    pub state: WorkerState,
    /// Absolute path to the managed worktree.
    pub worktree_path: PathBuf,
    /// Rulebook commit pinned into the worker contract.
    pub rulebook_commit: String,
    /// Base code commit from which the worker was provisioned.
    pub base_commit: String,
    /// Submitted worker commit when the worker is in `COMMITTED`.
    pub submitted_head_commit: Option<String>,
}

/// Acknowledgement metadata written to mailbox `ack/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AckRecord {
    pub(crate) sequence: Sequence,
    pub(crate) acknowledged_at: String,
}

impl MessageKind {
    /// The filesystem slug for bundle directory names.
    pub(crate) fn slug(self) -> &'static str {
        match self {
            | Self::Task => "task",
            | Self::Report => "report",
            | Self::Resolve => "resolve",
            | Self::Revise => "revise",
            | Self::Commit => "commit",
        }
    }
}

/// Return the union of every read and write path in a compiled rulebook.
pub(super) fn compiled_rulebook_paths(rulebook: &CompiledRulebook) -> BTreeSet<PathBuf> {
    let mut paths = BTreeSet::new();
    for perspective in rulebook.perspectives().perspectives().values() {
        paths.extend(perspective.read().iter().cloned());
        paths.extend(perspective.write().iter().cloned());
    }
    paths
}

/// Return `true` when a worker still participates in runtime conflict checks.
pub(super) fn is_live_worker_state(state: WorkerState) -> bool {
    !matches!(state, WorkerState::Integrated | WorkerState::Discarded)
}

/// Return `true` when a worker may still produce mailbox submissions.
pub(super) fn can_submit_from_state(state: WorkerState) -> bool {
    matches!(state, WorkerState::Provisioned | WorkerState::Active)
}

/// Validate that a skip request only targets skippable declared checks.
pub(super) fn validate_skip_request(
    rulebook: &CompiledRulebook, skip_checks: &[String],
) -> Result<BTreeSet<CheckName>, RuntimeError> {
    let mut accepted = BTreeSet::new();
    for requested in skip_checks {
        let name = CheckName::new(requested)
            .map_err(|_| RuntimeError::CheckFailed(format!("unknown check `{requested}`")))?;
        let Some(decl) = rulebook.checks().get(&name) else {
            return Err(RuntimeError::CheckFailed(format!("unknown check `{requested}`")));
        };
        if decl.policy() != CheckPolicy::Skippable {
            return Err(RuntimeError::CheckFailed(format!("check `{requested}` is not skippable")));
        }
        accepted.insert(name);
    }
    Ok(accepted)
}

/// Return a monotonic string timestamp.
pub(crate) fn timestamp_now() -> String {
    let now =
        SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock is after unix epoch");
    format!("{}.{}", now.as_secs(), now.subsec_nanos())
}
