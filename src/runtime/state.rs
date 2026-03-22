//! Shared runtime view types.
//!
//! These types represent orchestrator projections, worker contract
//! snapshots, and mailbox summaries. They are designed to be reused by
//! both the CLI and MCP surfaces.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::perspective::PerspectiveName;
use crate::vcs::CanonicalCommitHash;

use super::worker_id::WorkerId;
use super::{Sequence, bundle::MessageKind, mailbox::MailboxDirection};

/// Worker lifecycle state as projected by Multorum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkerState {
    /// The worktree and runtime surface have been created and the worker may run.
    ///
    /// Note: Provisioning transitions directly into `ACTIVE`; Multorum does
    /// not model a separate idle post-provisioning state.
    Active,
    /// The worker is blocked on orchestrator input.
    Blocked,
    /// The worker has submitted a commit and is frozen pending review.
    Committed,
    /// The worker has been merged into the canonical codebase.
    Merged,
    /// The worker has been discarded without integration.
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

/// Result of validating a rulebook switch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RulebookValidation {
    /// `true` if the target rulebook may be activated.
    pub ok: bool,
    /// Bidding groups currently blocking the switch.
    pub blocking_bidding_groups: Vec<PerspectiveName>,
}

/// Result of activating a rulebook switch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RulebookSwitch {
    /// Activated canonical rulebook commit hash.
    pub active_commit: CanonicalCommitHash,
}

/// Summary of one active bidding group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BiddingGroupSummary {
    /// Stable bidding-group identifier.
    pub bidding_group: PerspectiveName,
    /// Perspective instantiated by workers in the group.
    pub perspective: PerspectiveName,
    /// Live workers currently competing in the group.
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

/// Result of provisioning a worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProvisionResult {
    /// New worker identity.
    pub worker_id: WorkerId,
    /// Bidding group joined by the new worker.
    pub bidding_group: PerspectiveName,
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
    /// Bidding group from which the worker was discarded.
    pub bidding_group: PerspectiveName,
    /// Perspective held by the worker.
    pub perspective: PerspectiveName,
    /// Final worker state.
    pub state: WorkerState,
}

/// Result of integrating a worker submission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IntegrateResult {
    /// Merged worker identity.
    pub worker_id: WorkerId,
    /// Bidding group selected for integration.
    pub bidding_group: PerspectiveName,
    /// Perspective held by the merged worker.
    pub perspective: PerspectiveName,
    /// Final worker state.
    pub state: WorkerState,
    /// Checks that executed during integration.
    pub ran_checks: Vec<String>,
    /// Checks skipped due to trusted evidence.
    pub skipped_checks: Vec<String>,
}

/// Projected orchestrator view of all active workers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrchestratorStatus {
    /// Active canonical rulebook commit hash.
    pub active_rulebook_commit: CanonicalCommitHash,
    /// Current bidding-group summaries.
    pub bidding_groups: Vec<BiddingGroupSummary>,
    /// Current worker summaries.
    pub workers: Vec<WorkerSummary>,
}

/// Summary of one worker in orchestrator status output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkerSummary {
    /// Worker identity.
    pub worker_id: WorkerId,
    /// Bidding group to which the worker belongs.
    pub bidding_group: PerspectiveName,
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
    /// Bidding group to which the worker belongs.
    pub bidding_group: PerspectiveName,
    /// Perspective held by the worker.
    pub perspective: PerspectiveName,
    /// Current projected lifecycle state.
    pub state: WorkerState,
    /// Absolute path to the managed worker worktree.
    pub worktree_path: PathBuf,
    /// Canonical rulebook commit governing the worker.
    pub rulebook_commit: CanonicalCommitHash,
    /// Canonical base code commit from which the worker was provisioned.
    pub base_commit: CanonicalCommitHash,
    /// Canonical submitted worker head commit when present.
    pub submitted_head_commit: Option<CanonicalCommitHash>,
}

/// Worker-local status output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkerStatus {
    /// Worker identity.
    pub worker_id: WorkerId,
    /// Bidding group to which the worker belongs.
    pub bidding_group: PerspectiveName,
    /// Perspective held by the worker.
    pub perspective: PerspectiveName,
    /// Current projected lifecycle state.
    pub state: WorkerState,
}

/// Worker contract view exported to frontends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerContractView {
    /// Worker identity.
    pub worker_id: WorkerId,
    /// Bidding group to which the worker belongs.
    pub bidding_group: PerspectiveName,
    /// Perspective instantiated by the worker.
    pub perspective: PerspectiveName,
    /// Canonical rulebook commit governing the worker.
    pub rulebook_commit: CanonicalCommitHash,
    /// Canonical base code commit from which the worktree was provisioned.
    pub base_commit: CanonicalCommitHash,
    /// Path to the compiled read set file.
    pub read_set_path: PathBuf,
    /// Path to the compiled write set file.
    pub write_set_path: PathBuf,
}

/// Normalized mailbox message view for resource projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MailboxMessageView {
    /// Worker identity that owns the mailbox.
    pub worker_id: WorkerId,
    /// Bidding group to which the worker belongs.
    pub bidding_group: PerspectiveName,
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
