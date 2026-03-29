//! Typed auto-forward metadata shared across runtime surfaces.
//!
//! These types describe why a worker asked for perspective replay, what
//! the orchestrator proved before moving a bidding group, and how
//! caller-visible operations report executed or skipped auto-forward
//! decisions.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{schema::perspective::PerspectiveName, vcs::CanonicalCommitHash};

use super::worker_id::WorkerId;

/// Typed reason a worker or orchestrator action needs perspective replay.
///
/// This metadata is intentionally structured so Multorum never has to
/// infer "needs forward" from free-form bundle prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ForwardIntent {
    /// Refresh the bidding group's pinned base commit to current HEAD.
    RefreshBase,
    /// Refresh the bidding group's materialized boundary from the current rulebook.
    ExpandBoundary,
    /// Refresh both the pinned base commit and the materialized boundary.
    RefreshBaseAndExpandBoundary,
}

impl ForwardIntent {
    /// Stable kebab-case spelling used by CLI flags and mailbox envelopes.
    pub const fn as_str(self) -> &'static str {
        match self {
            | Self::RefreshBase => "refresh-base",
            | Self::ExpandBoundary => "expand-boundary",
            | Self::RefreshBaseAndExpandBoundary => "refresh-base-and-expand-boundary",
        }
    }

    /// Derive the forward intent implied by repository drift.
    pub const fn from_changes(base_changed: bool, boundary_changed: bool) -> Option<Self> {
        match (base_changed, boundary_changed) {
            | (false, false) => None,
            | (true, false) => Some(Self::RefreshBase),
            | (false, true) => Some(Self::ExpandBoundary),
            | (true, true) => Some(Self::RefreshBaseAndExpandBoundary),
        }
    }

    /// Whether this requested intent is satisfied by the observed change set.
    ///
    /// Note: `RefreshBaseAndExpandBoundary` is the strongest request. The
    /// single-purpose variants accept the combined change because manual
    /// `perspective forward` would move the whole group to current HEAD
    /// anyway once the proof succeeds.
    pub const fn is_satisfied_by(self, actual: Self) -> bool {
        matches!(
            (self, actual),
            (Self::RefreshBase, Self::RefreshBase | Self::RefreshBaseAndExpandBoundary)
                | (Self::ExpandBoundary, Self::ExpandBoundary | Self::RefreshBaseAndExpandBoundary)
                | (Self::RefreshBaseAndExpandBoundary, Self::RefreshBaseAndExpandBoundary)
        )
    }
}

impl std::fmt::Display for ForwardIntent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ForwardIntent {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            | "refresh-base" => Ok(Self::RefreshBase),
            | "expand-boundary" => Ok(Self::ExpandBoundary),
            | "refresh-base-and-expand-boundary" => Ok(Self::RefreshBaseAndExpandBoundary),
            | _ => Err(format!("unknown forward intent `{value}`")),
        }
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

/// Proven forward plan for one live bidding group.
///
/// This proof is constructed before any worktree moves. The runtime may
/// execute the matching forward only after this proof has established
/// that the whole group can move together under the normal manual
/// `perspective forward` rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerspectiveForwardProof {
    /// Perspective whose live bidding group was proven movable.
    pub perspective: PerspectiveName,
    /// Live workers that would move together.
    #[serde(rename = "workers")]
    pub worker_ids: Vec<WorkerId>,
    /// Base commit pinned by the live bidding group before replay.
    pub previous_base_commit: CanonicalCommitHash,
    /// Target base commit (HEAD at proof time).
    pub new_base_commit: CanonicalCommitHash,
    /// Repository drift that made forwarding necessary.
    pub intent: ForwardIntent,
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
    /// Perspective whose live bidding group was considered.
    pub perspective: PerspectiveName,
    /// Repository drift or typed blocker intent behind the decision.
    pub intent: ForwardIntent,
    /// Live workers in the considered bidding group.
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
        let intent = proof.intent;
        Self {
            kind: AutoForwardNoticeKind::Executed,
            trigger,
            perspective,
            intent,
            worker_ids,
            message: format!(
                "auto-forwarded perspective `{}` before `{}` after proving the whole bidding group could move",
                proof.perspective, trigger
            ),
            proof: Some(proof),
            manual_command: None,
        }
    }

    /// Construct the notice emitted when Multorum intentionally leaves
    /// forwarding to the user.
    pub fn skipped(
        trigger: AutoForwardTrigger, perspective: PerspectiveName, intent: ForwardIntent,
        worker_ids: Vec<WorkerId>, message: String,
    ) -> Self {
        Self {
            kind: AutoForwardNoticeKind::Skipped,
            trigger,
            perspective: perspective.clone(),
            intent,
            worker_ids,
            proof: None,
            manual_command: Some(format!("multorum perspective forward {perspective}")),
            message,
        }
    }
}
