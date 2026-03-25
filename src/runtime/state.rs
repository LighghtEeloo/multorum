//! Shared runtime view types.
//!
//! These types represent orchestrator projections, worker contract
//! snapshots, and mailbox summaries. They are designed to be reused by
//! both the CLI and MCP surfaces.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::schema::perspective::PerspectiveName;
use crate::vcs::CanonicalCommitHash;

use super::mailbox::{MailboxDirection, MessageKind, Sequence};
use super::worker_id::WorkerId;

/// Worker lifecycle state as projected by Multorum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkerState {
    /// The worktree and runtime surface have been created and the worker may run.
    ///
    /// Note: Worker creation transitions directly into `ACTIVE`; Multorum does
    /// not model a separate idle post-creation state.
    Active,
    /// The worker is blocked on orchestrator input.
    ///
    /// Note: A blocked worker may either return to `ACTIVE` after the
    /// orchestrator resolves the report, or be finalized directly as
    /// `DISCARDED` when the current contract should be retired.
    Blocked,
    /// The worker has submitted a commit and is frozen pending review.
    Committed,
    /// The worker has been merged into the canonical codebase.
    ///
    /// Note: Finalization does not tear down the worker workspace.
    /// Workspace deletion is a separate explicit orchestrator action.
    Merged,
    /// The worker has been discarded without integration.
    ///
    /// Note: Finalization does not tear down the worker workspace.
    /// Workspace deletion is a separate explicit orchestrator action.
    Discarded,
}

/// Summary of a compiled perspective known to the runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PerspectiveSummary {
    /// Perspective identifier.
    pub name: PerspectiveName,
    /// Number of files in the compiled read set.
    pub read_count: usize,
    /// Number of files in the compiled write set.
    pub write_count: usize,
}

/// Result of validating a rulebook install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RulebookValidation {
    /// `true` if the target rulebook may be activated.
    pub ok: bool,
    /// Perspectives with live runtime boundaries that still block the install.
    pub blocking_perspectives: Vec<PerspectiveName>,
}

/// Result of activating a rulebook install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RulebookInstall {
    /// Activated canonical rulebook commit hash.
    pub active_commit: CanonicalCommitHash,
}

/// Result of deactivating the active rulebook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RulebookUninstall {
    /// Previously active canonical rulebook commit hash.
    pub previous_commit: CanonicalCommitHash,
}

/// Summary of one active perspective in the current runtime.
///
/// Note: Derived from live workers rather than persisted as a separate
/// entity. Each perspective with at least one active worker produces
/// one summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActivePerspectiveSummary {
    /// Perspective identifier.
    pub perspective: PerspectiveName,
    /// Live workers currently instantiating this perspective.
    pub worker_ids: Vec<WorkerId>,
    /// Number of files in the materialized stable context.
    pub read_count: usize,
    /// Number of files in the materialized write boundary.
    pub write_count: usize,
}

/// Result of initializing `.multorum/` for a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RulebookInit {
    /// Absolute path to the created or reused `.multorum/` directory.
    pub multorum_root: PathBuf,
    /// Absolute path to the committed `.multorum/rulebook.toml`.
    pub rulebook_path: PathBuf,
    /// Absolute path to the committed `.multorum/.gitignore`.
    pub gitignore_path: PathBuf,
}

/// Result of creating a worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateResult {
    /// New worker identity.
    pub worker_id: WorkerId,
    /// Perspective instantiated by the worker.
    pub perspective: PerspectiveName,
    /// Absolute path to the worker worktree.
    pub worktree_path: PathBuf,
    /// Initial projected state.
    pub state: WorkerState,
    /// Optional seeded task bundle path.
    pub seeded_task_path: Option<PathBuf>,
}

/// Result of discarding a worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscardResult {
    /// Discarded worker identity.
    pub worker_id: WorkerId,
    /// Perspective held by the worker.
    pub perspective: PerspectiveName,
    /// Final worker state.
    pub state: WorkerState,
}

/// Result of deleting one finalized worker workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeleteResult {
    /// Deleted worker identity.
    pub worker_id: WorkerId,
    /// Perspective held by the worker.
    pub perspective: PerspectiveName,
    /// Final worker state preserved after workspace deletion.
    pub state: WorkerState,
    /// Absolute path to the worker workspace.
    pub worktree_path: PathBuf,
    /// Whether the repository backend removed a managed worktree.
    pub deleted_workspace: bool,
    /// Whether the worker state file was deleted.
    pub deleted_state_file: bool,
}

/// Result of merging a worker submission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MergeResult {
    /// Merged worker identity.
    pub worker_id: WorkerId,
    /// Perspective held by the merged worker.
    pub perspective: PerspectiveName,
    /// Final worker state.
    pub state: WorkerState,
    /// Checks that executed during integration.
    pub ran_checks: Vec<String>,
    /// Checks skipped due to trusted evidence.
    pub skipped_checks: Vec<String>,
}

/// Persisted audit entry written after a successful merge.
///
/// Each entry records the full merge context and the orchestrator's
/// rationale. Stored under `.multorum/orchestrator/audit/<worker-id>.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Merged worker identity.
    pub worker_id: WorkerId,
    /// Perspective held by the merged worker.
    pub perspective: PerspectiveName,
    /// Commit the worker was pinned to at creation.
    pub base_commit: CanonicalCommitHash,
    /// Integrated head commit from the worker submission.
    pub head_commit: CanonicalCommitHash,
    /// Files changed by the worker relative to the base commit.
    pub changed_files: Vec<PathBuf>,
    /// Checks that executed during integration.
    pub ran_checks: Vec<String>,
    /// Checks skipped due to trusted evidence.
    pub skipped_checks: Vec<String>,
    /// Timestamp when the merge was recorded.
    pub merged_at: String,
    /// Orchestrator-supplied rationale body, if any.
    pub rationale_body: Option<PathBuf>,
    /// Orchestrator-supplied rationale artifacts, if any.
    pub rationale_artifacts: Vec<PathBuf>,
}

/// Projected orchestrator view of all active workers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrchestratorStatus {
    /// Active canonical rulebook commit hash.
    pub active_rulebook_commit: CanonicalCommitHash,
    /// Current active perspective summaries.
    pub active_perspectives: Vec<ActivePerspectiveSummary>,
    /// Current worker summaries.
    pub workers: Vec<WorkerSummary>,
}

/// Summary of one worker in orchestrator status output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkerSummary {
    /// Worker identity.
    pub worker_id: WorkerId,
    /// Perspective held by the worker.
    pub perspective: PerspectiveName,
    /// Current projected lifecycle state.
    pub state: WorkerState,
}

/// Detailed orchestrator-side view of one worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkerDetail {
    /// Worker identity.
    pub worker_id: WorkerId,
    /// Perspective held by the worker.
    pub perspective: PerspectiveName,
    /// Current projected lifecycle state.
    pub state: WorkerState,
    /// Absolute path to the managed worker worktree.
    pub worktree_path: PathBuf,
    /// Canonical base commit pinning the worker's code snapshot.
    ///
    /// Note: A compatible `rulebook install` may expand the materialized
    /// read/write-set files for this worker without changing `base_commit`.
    pub base_commit: CanonicalCommitHash,
    /// Canonical submitted worker head commit when present.
    pub submitted_head_commit: Option<CanonicalCommitHash>,
}

/// Worker-local status output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkerStatus {
    /// Worker identity.
    pub worker_id: WorkerId,
    /// Perspective held by the worker.
    pub perspective: PerspectiveName,
    /// Current projected lifecycle state.
    pub state: WorkerState,
}

/// Worker contract view exported to frontends.
///
/// `base_commit` pins the worker's code snapshot. The referenced
/// read/write-set files are the authoritative materialized boundary and
/// may be refreshed by a compatible `rulebook install`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerContractView {
    /// Worker identity.
    pub worker_id: WorkerId,
    /// Perspective instantiated by the worker.
    pub perspective: PerspectiveName,
    /// Canonical base commit pinning the worker's code snapshot.
    pub base_commit: CanonicalCommitHash,
    /// Path to the materialized read set file.
    pub read_set_path: PathBuf,
    /// Path to the materialized write set file.
    pub write_set_path: PathBuf,
}

/// Normalized mailbox message view for resource projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MailboxMessageView {
    /// Worker identity that owns the mailbox.
    pub worker_id: WorkerId,
    /// Perspective instantiated by the worker.
    pub perspective: PerspectiveName,
    /// Direction of the mailbox containing the message.
    pub direction: MailboxDirection,
    /// Message kind.
    pub kind: MessageKind,
    /// Mailbox-local sequence number.
    pub sequence: Sequence,
    /// Publisher-provided timestamp.
    pub created_at: String,
    /// Whether the message has been acknowledged.
    pub acknowledged: bool,
    /// Optional canonical commit hash attached to the message.
    pub head_commit: Option<CanonicalCommitHash>,
    /// Short summary for compact listings.
    pub summary: String,
    /// Absolute path to the bundle directory.
    pub bundle_path: PathBuf,
}

/// Ordered transcript view for a worker interaction history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TranscriptView {
    /// Messages in logical transcript order.
    pub messages: Vec<MailboxMessageView>,
}

impl TranscriptView {
    /// Construct an empty transcript.
    pub fn empty() -> Self {
        Self { messages: Vec::new() }
    }
}
