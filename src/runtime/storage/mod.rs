//! Storage helpers shared by the runtime entry points.
//!
//! The runtime model is intentionally filesystem-first: `.multorum/`
//! stores the authoritative control plane, worker contract, compiled
//! file sets, and mailbox bundles. This module centralizes that on-disk
//! layout and the small amount of version-control orchestration needed
//! to create worktrees, delete finalized workspaces, and integrate
//! submitted commits.

mod mailbox;
mod records;
mod state;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::schema::rulebook::{CheckName, CheckPolicy, CompiledRulebook};
use crate::runtime::{
    MessageKind, MultorumPaths, RuntimeError, WorkerId, WorkerPaths, WorkerState,
};
use crate::vcs::{GitVcs, VersionControl};

pub(crate) use records::{AckRecord, ActiveRulebookRecord, WorkerRecord};

/// Protocol version written into persisted mailbox envelopes.
pub(crate) const PROTOCOL_VERSION: u32 = 1;

/// Canonical mailbox envelope file name within one bundle directory.
pub(crate) const ENVELOPE_FILE_NAME: &str = "envelope.toml";
/// Canonical mailbox body file name within one bundle directory.
pub(crate) const BODY_FILE_NAME: &str = "body.md";
/// Canonical artifacts directory name within one bundle directory.
pub(crate) const ARTIFACTS_DIR_NAME: &str = "artifacts";
/// Canonical acknowledgement file extension for mailbox bundles.
pub(crate) const ACK_EXTENSION: &str = "ack";

/// Storage access rooted at the canonical workspace.
#[derive(Debug, Clone)]
pub(crate) struct RuntimeFs {
    paths: MultorumPaths,
    vcs: Arc<dyn VersionControl>,
}

impl RuntimeFs {
    /// Build runtime helpers for the canonical workspace root.
    pub(crate) fn new(workspace_root: impl Into<PathBuf>) -> Result<Self, RuntimeError> {
        Self::with_vcs(workspace_root, Arc::new(GitVcs::new()))
    }

    /// Build runtime helpers for the canonical workspace root with one
    /// explicit repository backend.
    pub(crate) fn with_vcs(
        workspace_root: impl Into<PathBuf>, vcs: Arc<dyn VersionControl>,
    ) -> Result<Self, RuntimeError> {
        Ok(Self { paths: MultorumPaths::new_canonical(workspace_root.into())?, vcs })
    }

    /// The canonical workspace root.
    pub(crate) fn workspace_root(&self) -> &Path {
        self.paths.workspace_root()
    }

    /// Deterministic worktree-local runtime paths for one worker.
    pub(crate) fn worker_paths(&self, worker_id: &WorkerId) -> WorkerPaths {
        self.paths.worker(worker_id)
    }

    /// Repository backend bound to the current workspace.
    pub(crate) fn vcs(&self) -> &dyn VersionControl {
        self.vcs.as_ref()
    }

    /// Run one shell-based rulebook check in a worktree.
    pub(crate) fn run_check(
        &self, worktree_root: &Path, name: &CheckName, command_text: &str,
    ) -> Result<(), RuntimeError> {
        tracing::trace!(
            check = %name,
            command = command_text,
            root = %worktree_root.display(),
            "running pre-merge check"
        );

        let output =
            Command::new("sh").arg("-lc").arg(command_text).current_dir(worktree_root).output()?;

        if output.status.success() {
            return Ok(());
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let details = if stderr.trim().is_empty() {
            stdout.trim().to_owned()
        } else {
            stderr.trim().to_owned()
        };
        Err(RuntimeError::CheckFailed(format!("{name}: {details}")))
    }
}

impl MessageKind {
    /// The storage slug for bundle directory names.
    ///
    /// Note: Mailbox bundles use stable directory names so they can be
    /// inspected directly from disk and safely referenced by tests.
    pub(crate) fn slug(self) -> &'static str {
        match self {
            | Self::Task => "task",
            | Self::Report => "report",
            | Self::Resolve => "resolve",
            | Self::Revise => "revise",
            | Self::Commit => "commit",
            | Self::Audit => "audit",
        }
    }
}

/// Return `true` when a worker still participates in runtime conflict checks.
pub(super) fn is_live_worker_state(state: WorkerState) -> bool {
    !matches!(state, WorkerState::Merged | WorkerState::Discarded)
}

/// Return `true` when a worker may still produce mailbox submissions.
pub(super) fn can_submit_from_state(state: WorkerState) -> bool {
    matches!(state, WorkerState::Active)
}

/// Validate that a skip request only targets skippable declared checks.
///
/// Checks omitted from the optional `[check.policy]` table default to
/// `always`, so skip requests for them are rejected here.
pub(super) fn validate_skip_request(
    rulebook: &CompiledRulebook, skip_checks: &[String],
) -> Result<BTreeSet<CheckName>, RuntimeError> {
    let mut accepted = BTreeSet::new();
    for requested in skip_checks {
        let name = CheckName::new(requested)
            .map_err(|_| RuntimeError::CheckFailed(format!("unknown check `{requested}`")))?;
        let Some(decl) = rulebook.check().get(&name) else {
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
