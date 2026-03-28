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
use super::timestamp::Timestamp;
use super::worker_id::WorkerId;

/// Worker lifecycle state as projected by Multorum.
///
/// Serde encodes these states as lowercase identifiers so persisted
/// runtime files and machine-facing APIs share one stable wire format.
/// Human-facing diagnostics continue to use uppercase spellings via
/// [`WorkerState::as_str`] and [`std::fmt::Display`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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

impl WorkerState {
    /// Stable screaming-case name for diagnostics and display.
    pub fn as_str(self) -> &'static str {
        match self {
            | Self::Active => "ACTIVE",
            | Self::Blocked => "BLOCKED",
            | Self::Committed => "COMMITTED",
            | Self::Merged => "MERGED",
            | Self::Discarded => "DISCARDED",
        }
    }

    /// Whether the worker still participates in runtime conflict checks.
    ///
    /// Live workers hold a bidding-group slot and contribute to the
    /// orchestrator exclusion set. Finalized workers (`MERGED` or
    /// `DISCARDED`) do not.
    pub fn is_live(self) -> bool {
        !matches!(self, Self::Merged | Self::Discarded)
    }

    /// Whether the worker may still produce mailbox submissions.
    ///
    /// Only `ACTIVE` workers may publish report or commit bundles.
    pub fn can_submit(self) -> bool {
        matches!(self, Self::Active)
    }

    /// Whether the worker can participate in `perspective forward`.
    ///
    /// Forwarding is allowed only after execution has paused at a
    /// durable checkpoint: either a blocker report (`BLOCKED`) or a
    /// submitted commit (`COMMITTED`). `ACTIVE` workers may still be
    /// mutating their worktrees, and finalized workers no longer belong
    /// to the live bidding group.
    pub fn can_forward_perspective(self) -> bool {
        matches!(self, Self::Blocked | Self::Committed)
    }
}

impl MessageKind {
    /// Worker state required before the orchestrator may publish this
    /// message kind to the worker inbox.
    ///
    /// Note: Only orchestrator-authored follow-up bundles participate
    /// in this gate. Worker-authored kinds and the initial `TASK`
    /// bundle are handled elsewhere.
    pub(crate) const fn required_worker_state_for_inbox_publication(self) -> Option<WorkerState> {
        match self {
            | Self::Hint => Some(WorkerState::Active),
            | Self::Resolve => Some(WorkerState::Blocked),
            | Self::Revise => Some(WorkerState::Committed),
            | Self::Task | Self::Report | Self::Commit => None,
        }
    }

    /// Worker state transition applied after the worker acknowledges an
    /// inbox message of this kind.
    ///
    /// Note: Only inbox messages that explicitly unblock or re-activate
    /// work carry lifecycle transitions. Advisory hints leave the
    /// current worker state unchanged.
    pub(crate) const fn worker_state_after_inbox_ack(self) -> Option<WorkerState> {
        match self {
            | Self::Task | Self::Resolve | Self::Revise => Some(WorkerState::Active),
            | Self::Hint | Self::Report | Self::Commit => None,
        }
    }

    /// Worker state transition applied after the worker publishes an
    /// outbox message of this kind.
    ///
    /// Note: Only worker-authored progress reports and commit
    /// submissions advance the persisted lifecycle state.
    pub(crate) const fn worker_state_after_outbox_publication(self) -> Option<WorkerState> {
        match self {
            | Self::Report => Some(WorkerState::Blocked),
            | Self::Commit => Some(WorkerState::Committed),
            | Self::Task | Self::Hint | Self::Resolve | Self::Revise => None,
        }
    }
}

impl std::fmt::Display for WorkerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
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

/// Result of validating a set of perspectives for conflict-freedom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PerspectiveValidation {
    /// `true` if the named perspectives satisfy the conflict-free invariant.
    pub ok: bool,
    /// Compiled summaries for the validated perspectives.
    pub perspectives: Vec<PerspectiveSummary>,
    /// Detected boundary conflicts.
    pub conflicts: Vec<PerspectiveConflict>,
}

/// One boundary conflict between two perspectives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PerspectiveConflict {
    /// Perspective on one side of the conflict.
    pub perspective: PerspectiveName,
    /// Perspective on the other side.
    pub blocking_perspective: PerspectiveName,
    /// Human-readable description of the overlap relation.
    pub relation: &'static str,
    /// Overlapping files.
    pub files: Vec<std::path::PathBuf>,
}

/// Summary of one active perspective in the current runtime.
///
/// Note: Derived from persisted group and worker state under
/// `.multorum/orchestrator/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActivePerspectiveSummary {
    /// Perspective identifier.
    pub perspective: PerspectiveName,
    /// Live workers currently instantiating this perspective.
    #[serde(rename = "workers")]
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
    /// Non-fatal findings observed while initializing.
    ///
    /// Note: Callers should surface these to users because they often
    /// indicate an unusual repository/runtime layout that Multorum
    /// repaired or worked around.
    pub warnings: Vec<String>,
}

/// Result of creating a worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateResult {
    /// New worker identity.
    #[serde(rename = "worker")]
    pub worker_id: WorkerId,
    /// Perspective instantiated by the worker.
    pub perspective: PerspectiveName,
    /// Absolute path to the worker worktree.
    pub worktree_path: PathBuf,
    /// Initial projected state.
    pub state: WorkerState,
    /// Absolute path to the created initial `task` bundle.
    ///
    /// Note: Every worker starts with sequence `1` in its inbox so the
    /// worker transcript has one stable bootstrap shape even when the
    /// orchestrator supplied no body text or artifacts.
    pub created_task_path: PathBuf,
}

/// Result of forwarding one live bidding group to HEAD.
///
/// Note: This is a group-scoped operation. Every live worker for the
/// perspective moves together or the command fails without persisting
/// the new base snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PerspectiveForwardResult {
    /// Perspective whose live bidding group moved forward.
    pub perspective: PerspectiveName,
    /// Live workers forwarded together.
    #[serde(rename = "workers")]
    pub worker_ids: Vec<WorkerId>,
    /// Base commit previously pinned by the live bidding group.
    pub previous_base_commit: CanonicalCommitHash,
    /// New base commit (HEAD at the time of forwarding).
    pub new_base_commit: CanonicalCommitHash,
}

/// Result of discarding a worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscardResult {
    /// Discarded worker identity.
    #[serde(rename = "worker")]
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
    #[serde(rename = "worker")]
    pub worker_id: WorkerId,
    /// Perspective held by the worker.
    pub perspective: PerspectiveName,
    /// Final worker state preserved after workspace deletion.
    pub state: WorkerState,
    /// Absolute path to the worker workspace.
    pub worktree_path: PathBuf,
    /// Whether the repository backend removed a managed worktree.
    pub deleted_workspace: bool,
}

/// Result of merging a worker submission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MergeResult {
    /// Merged worker identity.
    #[serde(rename = "worker")]
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
/// rationale. Stored under
/// `.multorum/audit/<worker>-<head-prefix6>/entry.toml` and committed to
/// version control alongside the repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Merged worker identity.
    #[serde(rename = "worker")]
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
    pub merged_at: Timestamp,
    /// Orchestrator-supplied rationale body, if any.
    pub rationale_body: Option<PathBuf>,
    /// Orchestrator-supplied rationale artifacts, if any.
    pub rationale_artifacts: Vec<PathBuf>,
}

/// Projected orchestrator view of all active workers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrchestratorStatus {
    /// Current active perspective summaries (bidding groups with live workers).
    pub active_perspectives: Vec<ActivePerspectiveSummary>,
    /// Current worker summaries.
    pub workers: Vec<WorkerSummary>,
}

/// Summary of one worker in orchestrator status output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkerSummary {
    /// Worker identity.
    #[serde(rename = "worker")]
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
    #[serde(rename = "worker")]
    pub worker_id: WorkerId,
    /// Perspective held by the worker.
    pub perspective: PerspectiveName,
    /// Current projected lifecycle state.
    pub state: WorkerState,
    /// Absolute path to the managed worker worktree.
    pub worktree_path: PathBuf,
    /// Canonical base commit pinning the worker's code snapshot.
    pub base_commit: CanonicalCommitHash,
    /// Canonical submitted worker head commit when present.
    pub submitted_head_commit: Option<CanonicalCommitHash>,
}

/// Worker-local status output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkerStatus {
    /// Worker identity.
    #[serde(rename = "worker")]
    pub worker_id: WorkerId,
    /// Perspective held by the worker.
    pub perspective: PerspectiveName,
    /// Current projected lifecycle state.
    pub state: WorkerState,
}

/// Worker contract view exported to frontends.
///
/// `base_commit` pins the worker's code snapshot. The referenced
/// read/write-set files are the authoritative materialized boundary.
/// Both change only when the orchestrator explicitly forwards the
/// whole bidding group to HEAD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerContractView {
    /// Worker identity.
    #[serde(rename = "worker")]
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
    #[serde(rename = "worker")]
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
    pub created_at: Timestamp,
    /// Whether the message has been acknowledged.
    pub acknowledged: bool,
    /// Optional canonical commit hash attached to the message.
    pub head_commit: Option<CanonicalCommitHash>,
    /// Short summary for compact listings.
    pub summary: String,
    /// Full body content, present when the caller requests it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{MessageKind, WorkerState};

    #[test]
    fn worker_state_serializes_as_lowercase() {
        assert_eq!(serde_json::to_value(WorkerState::Active).unwrap(), json!("active"));
    }

    #[test]
    fn worker_state_deserializes_from_lowercase() {
        assert_eq!(
            serde_json::from_value::<WorkerState>(json!("discarded")).unwrap(),
            WorkerState::Discarded
        );
    }

    #[test]
    fn inbox_publication_policy_matches_worker_lifecycle() {
        assert_eq!(
            MessageKind::Hint.required_worker_state_for_inbox_publication(),
            Some(WorkerState::Active)
        );
        assert_eq!(
            MessageKind::Resolve.required_worker_state_for_inbox_publication(),
            Some(WorkerState::Blocked)
        );
        assert_eq!(
            MessageKind::Revise.required_worker_state_for_inbox_publication(),
            Some(WorkerState::Committed)
        );
        assert_eq!(MessageKind::Task.required_worker_state_for_inbox_publication(), None);
    }

    #[test]
    fn mailbox_lifecycle_transitions_are_centralized_by_message_kind() {
        assert_eq!(MessageKind::Task.worker_state_after_inbox_ack(), Some(WorkerState::Active));
        assert_eq!(MessageKind::Resolve.worker_state_after_inbox_ack(), Some(WorkerState::Active));
        assert_eq!(MessageKind::Revise.worker_state_after_inbox_ack(), Some(WorkerState::Active));
        assert_eq!(MessageKind::Hint.worker_state_after_inbox_ack(), None);

        assert_eq!(
            MessageKind::Report.worker_state_after_outbox_publication(),
            Some(WorkerState::Blocked)
        );
        assert_eq!(
            MessageKind::Commit.worker_state_after_outbox_publication(),
            Some(WorkerState::Committed)
        );
        assert_eq!(MessageKind::Hint.worker_state_after_outbox_publication(), None);
    }
}
