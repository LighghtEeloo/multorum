//! Persisted storage-specific runtime records.
//!
//! These types are internal to the storage backend. They correspond
//! directly to files stored under `.multorum/` and should not be treated
//! as the frontend-facing runtime API.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::perspective::PerspectiveName;
use crate::runtime::{Sequence, WorkerState};
use crate::vcs::CanonicalCommitHash;

/// Active rulebook projection stored under `.multorum/orchestrator/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ActiveRulebookRecord {
    /// Canonical commit that owns the active committed rulebook.
    pub rulebook_commit: CanonicalCommitHash,
    /// Canonical pinned base commit for newly provisioned workers.
    pub base_commit: CanonicalCommitHash,
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
    /// Canonical rulebook commit pinned into the worker contract.
    pub rulebook_commit: CanonicalCommitHash,
    /// Canonical base code commit from which the worker was provisioned.
    pub base_commit: CanonicalCommitHash,
    /// Canonical submitted worker commit when the worker is in `COMMITTED`.
    pub submitted_head_commit: Option<CanonicalCommitHash>,
}

/// Acknowledgement metadata written to mailbox `ack/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AckRecord {
    /// The acknowledged mailbox sequence number.
    pub(crate) sequence: Sequence,
    /// Monotonic timestamp recorded when the acknowledgement was written.
    pub(crate) acknowledged_at: String,
}
