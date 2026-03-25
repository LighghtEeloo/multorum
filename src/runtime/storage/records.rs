//! Persisted storage-specific runtime records.
//!
//! These types are internal to the storage backend. They correspond
//! directly to files stored under `.multorum/` and should not be treated
//! as the frontend-facing runtime API.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::runtime::{Sequence, WorkerId, WorkerState};
use crate::schema::perspective::PerspectiveName;
use crate::vcs::CanonicalCommitHash;

/// Active rulebook projection stored under `.multorum/orchestrator/`.
///
/// The rulebook is always the one committed at `base_commit`. There is no
/// separate rulebook pin — the repository-wide rulebook is consistent with
/// the pinned base snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ActiveRulebookRecord {
    /// Canonical commit pinning both the active rulebook and the base
    /// snapshot for newly created workers.
    pub base_commit: CanonicalCommitHash,
    /// Activation timestamp.
    pub activated_at: String,
}

/// Orchestrator-local projection for one live or historical worker.
///
/// The worker's rulebook is the one committed at `base_commit`. There is
/// no separate rulebook pin — the rulebook governing the worker is always
/// the version present in the snapshot the worker was created from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WorkerRecord {
    /// Unique worker identity.
    pub worker_id: WorkerId,
    /// Perspective currently held by the worker.
    pub perspective: PerspectiveName,
    /// Current lifecycle state.
    pub state: WorkerState,
    /// Absolute path to the managed worktree.
    pub worktree_path: PathBuf,
    /// Canonical base commit pinning both the worker's code snapshot and
    /// its governing rulebook.
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
