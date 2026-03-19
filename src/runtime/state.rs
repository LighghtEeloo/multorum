//! Shared runtime view types.
//!
//! These types represent orchestrator projections, worker contract
//! snapshots, and mailbox summaries. They are designed to be reused by
//! both the CLI and MCP surfaces.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::perspective::PerspectiveName;

use super::{Sequence, bundle::MessageKind, mailbox::MailboxDirection};

/// Worker lifecycle state as projected by Multorum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkerState {
    /// The worktree and runtime surface have been created.
    Provisioned,
    /// The worker is actively executing its task.
    Active,
    /// The worker is blocked on orchestrator input.
    Blocked,
    /// The worker has submitted a commit and is frozen pending review.
    Committed,
    /// The worker has been integrated into the canonical codebase.
    Integrated,
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
    /// Perspectives currently blocking the switch.
    pub blocking_workers: Vec<PerspectiveName>,
}

/// Result of activating a rulebook switch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RulebookSwitch {
    /// Activated rulebook commit hash.
    pub active_commit: String,
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
    /// Provisioned worker identity.
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
    pub perspective: PerspectiveName,
    /// Final worker state.
    pub state: WorkerState,
}

/// Result of integrating a worker submission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IntegrateResult {
    /// Integrated worker identity.
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
    /// Active rulebook commit hash.
    pub active_rulebook_commit: String,
    /// Current worker summaries.
    pub workers: Vec<WorkerSummary>,
}

/// Summary of one worker in orchestrator status output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkerSummary {
    /// Worker identity.
    pub perspective: PerspectiveName,
    /// Current projected lifecycle state.
    pub state: WorkerState,
}

/// Worker-local status output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkerStatus {
    /// Worker identity.
    pub perspective: PerspectiveName,
    /// Current projected lifecycle state.
    pub state: WorkerState,
}

/// Worker contract view exported to frontends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerContractView {
    /// Worker identity.
    pub perspective: PerspectiveName,
    /// Rulebook commit governing the worker.
    pub rulebook_commit: String,
    /// Base code commit from which the worktree was provisioned.
    pub base_commit: String,
    /// Path to the compiled read set file.
    pub read_set_path: PathBuf,
    /// Path to the compiled write set file.
    pub write_set_path: PathBuf,
}

/// Normalized mailbox message view for resource projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MailboxMessageView {
    /// Worker identity that owns the mailbox.
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
    /// Optional commit hash attached to the message.
    pub head_commit: Option<String>,
    /// Short summary for compact listings.
    pub summary: String,
    /// Absolute path to the bundle directory.
    pub bundle_path: PathBuf,
}
