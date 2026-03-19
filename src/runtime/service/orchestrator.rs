//! Orchestrator-facing runtime service surface.

use crate::perspective::PerspectiveName;

use super::super::{
    bundle::{BundlePayload, PublishedBundle, ReplyReference},
    error::{Result, RuntimeError},
    state::{
        DiscardResult, IntegrateResult, OrchestratorStatus, PerspectiveSummary, ProvisionResult,
        RulebookSwitch, RulebookValidation,
    },
};

/// Typed operations available to the orchestrator frontend.
pub trait OrchestratorService {
    /// Dry-run validation of a rulebook switch.
    fn rulebook_validate(&self, commit: String) -> Result<RulebookValidation>;

    /// Activate a rulebook commit after validation succeeds.
    fn rulebook_switch(&self, commit: String) -> Result<RulebookSwitch>;

    /// List compiled perspective summaries from the active rulebook.
    fn list_perspectives(&self) -> Result<Vec<PerspectiveSummary>>;

    /// Provision a worker worktree and optional initial task bundle.
    fn provision_worker(
        &self, perspective: PerspectiveName, task: Option<BundlePayload>,
    ) -> Result<ProvisionResult>;

    /// Publish a `resolve` bundle to the worker inbox.
    fn resolve_worker(
        &self, perspective: PerspectiveName, reply: ReplyReference, payload: BundlePayload,
    ) -> Result<PublishedBundle>;

    /// Publish a `revise` bundle to the worker inbox.
    fn revise_worker(
        &self, perspective: PerspectiveName, reply: ReplyReference, payload: BundlePayload,
    ) -> Result<PublishedBundle>;

    /// Tear down a worker without integration.
    fn discard_worker(&self, perspective: PerspectiveName) -> Result<DiscardResult>;

    /// Run the pre-merge pipeline and integrate the worker submission.
    fn integrate_worker(
        &self, perspective: PerspectiveName, skip_checks: Vec<String>,
    ) -> Result<IntegrateResult>;

    /// Return the current orchestrator status projection.
    fn status(&self) -> Result<OrchestratorStatus>;
}

/// Placeholder orchestrator service used while the runtime is scaffolded.
#[derive(Debug, Default)]
pub struct NoopOrchestratorService;

impl OrchestratorService for NoopOrchestratorService {
    fn rulebook_validate(&self, _commit: String) -> Result<RulebookValidation> {
        Err(RuntimeError::Unimplemented("rulebook_validate"))
    }

    fn rulebook_switch(&self, _commit: String) -> Result<RulebookSwitch> {
        Err(RuntimeError::Unimplemented("rulebook_switch"))
    }

    fn list_perspectives(&self) -> Result<Vec<PerspectiveSummary>> {
        Err(RuntimeError::Unimplemented("list_perspectives"))
    }

    fn provision_worker(
        &self, _perspective: PerspectiveName, _task: Option<BundlePayload>,
    ) -> Result<ProvisionResult> {
        Err(RuntimeError::Unimplemented("provision_worker"))
    }

    fn resolve_worker(
        &self, _perspective: PerspectiveName, _reply: ReplyReference, _payload: BundlePayload,
    ) -> Result<PublishedBundle> {
        Err(RuntimeError::Unimplemented("resolve_worker"))
    }

    fn revise_worker(
        &self, _perspective: PerspectiveName, _reply: ReplyReference, _payload: BundlePayload,
    ) -> Result<PublishedBundle> {
        Err(RuntimeError::Unimplemented("revise_worker"))
    }

    fn discard_worker(&self, _perspective: PerspectiveName) -> Result<DiscardResult> {
        Err(RuntimeError::Unimplemented("discard_worker"))
    }

    fn integrate_worker(
        &self, _perspective: PerspectiveName, _skip_checks: Vec<String>,
    ) -> Result<IntegrateResult> {
        Err(RuntimeError::Unimplemented("integrate_worker"))
    }

    fn status(&self) -> Result<OrchestratorStatus> {
        Err(RuntimeError::Unimplemented("orchestrator_status"))
    }
}
