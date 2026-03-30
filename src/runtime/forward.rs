//! Typed auto-forward metadata shared across runtime surfaces.
//!
//! These types describe what the orchestrator proved before moving a
//! candidate group and how caller-visible operations report executed or
//! skipped auto-forward decisions.

use serde::{Deserialize, Serialize};

use crate::{schema::perspective::PerspectiveName, vcs::CanonicalCommitHash};

use super::worker_id::WorkerId;

/// Describe repository drift that makes forwarding meaningful.
///
/// Note: Auto-forward no longer needs a dedicated "intent" enum after
/// worker-authored forward requests were removed. The proof cares only
/// about these two facts.
pub(crate) const fn forward_change_description(
    base_changed: bool, boundary_changed: bool,
) -> &'static str {
    match (base_changed, boundary_changed) {
        | (true, false) => "current HEAD moved ahead of the live candidate group",
        | (false, true) => "current rulebook expanded the live candidate group boundary",
        | (true, true) => "current HEAD and rulebook both moved ahead of the live candidate group",
        | (false, false) => "the live candidate group already matches current HEAD and rulebook",
    }
}

/// Caller action that triggered an auto-forward attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AutoForwardTrigger {
    /// The orchestrator is creating another worker for the perspective.
    CreateWorker,
    /// The orchestrator is resolving a blocked worker.
    ResolveWorker,
}

impl AutoForwardTrigger {
    /// Stable kebab-case spelling for user-facing projections.
    pub const fn as_str(self) -> &'static str {
        match self {
            | Self::CreateWorker => "create-worker",
            | Self::ResolveWorker => "resolve-worker",
        }
    }
}

impl std::fmt::Display for AutoForwardTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Proven forward plan for one live candidate group.
///
/// This proof is constructed before any worktree moves. The runtime may
/// execute the matching forward only after this proof has established
/// that the whole group can move together under the normal manual
/// `perspective forward` rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerspectiveForwardProof {
    /// Perspective whose live candidate group was proven movable.
    pub perspective: PerspectiveName,
    /// Live workers that would move together.
    #[serde(rename = "workers")]
    pub worker_ids: Vec<WorkerId>,
    /// Base commit pinned by the live candidate group before replay.
    pub previous_base_commit: CanonicalCommitHash,
    /// Target base commit (HEAD at proof time).
    pub new_base_commit: CanonicalCommitHash,
    /// Whether current HEAD moved ahead of the live candidate group's pinned base.
    pub base_changed: bool,
    /// Whether the working-tree rulebook expanded the live candidate group's boundary.
    pub boundary_changed: bool,
}

/// High-level outcome of one auto-forward decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AutoForwardNoticeKind {
    /// Multorum proved the group movable and executed the forward.
    Executed,
    /// Multorum left the group untouched and manual forward remains available.
    Skipped,
}

/// Caller-visible note about one auto-forward decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoForwardNotice {
    /// Whether Multorum executed or skipped auto-forward.
    pub kind: AutoForwardNoticeKind,
    /// Caller action that triggered the decision.
    pub trigger: AutoForwardTrigger,
    /// Perspective whose live candidate group was considered.
    pub perspective: PerspectiveName,
    /// Whether current HEAD moved ahead of the live candidate group's pinned base.
    pub base_changed: bool,
    /// Whether the working-tree rulebook expanded the live candidate group's boundary.
    pub boundary_changed: bool,
    /// Live workers in the considered candidate group.
    #[serde(rename = "workers")]
    pub worker_ids: Vec<WorkerId>,
    /// Proven forward plan when Multorum executed the move.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof: Option<PerspectiveForwardProof>,
    /// Human-readable explanation suitable for CLI or transport output.
    pub message: String,
    /// Manual command the user may run when auto-forward was skipped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual_command: Option<String>,
}

impl AutoForwardNotice {
    /// Construct the notice emitted after a successful auto-forward.
    pub fn executed(trigger: AutoForwardTrigger, proof: PerspectiveForwardProof) -> Self {
        let worker_ids = proof.worker_ids.clone();
        let perspective = proof.perspective.clone();
        Self {
            kind: AutoForwardNoticeKind::Executed,
            trigger,
            perspective,
            base_changed: proof.base_changed,
            boundary_changed: proof.boundary_changed,
            worker_ids,
            message: format!(
                "auto-forwarded perspective `{}` before `{}` after proving the whole candidate group could move because {}",
                proof.perspective,
                trigger,
                forward_change_description(proof.base_changed, proof.boundary_changed)
            ),
            proof: Some(proof),
            manual_command: None,
        }
    }

    /// Construct the notice emitted when Multorum intentionally leaves
    /// forwarding to the user.
    pub fn skipped(
        trigger: AutoForwardTrigger, perspective: PerspectiveName, base_changed: bool,
        boundary_changed: bool, worker_ids: Vec<WorkerId>, message: String,
    ) -> Self {
        Self {
            kind: AutoForwardNoticeKind::Skipped,
            trigger,
            perspective: perspective.clone(),
            base_changed,
            boundary_changed,
            worker_ids,
            proof: None,
            manual_command: Some(format!("multorum perspective forward {perspective}")),
            message,
        }
    }
}
