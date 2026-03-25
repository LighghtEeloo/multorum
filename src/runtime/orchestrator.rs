//! Orchestrator-facing runtime service surface.
//!
//! This module defines the typed operations available to orchestrator
//! frontends and the default storage-backed implementation used by
//! the CLI.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use crate::{
    bundle::BundlePayload,
    schema::{
        perspective::{CompiledPerspective, PerspectiveName},
        rulebook::CompiledRulebook,
    },
    vcs::{CanonicalCommitHash, VersionControl},
};

use super::{
    error::{Result, RuntimeError},
    mailbox::{AckRef, MailboxDirection, MessageKind, PublishedBundle, ReplyReference, Sequence},
    project::CurrentProject,
    state::{
        ActivePerspectiveSummary, CreateResult, DeleteResult, DiscardResult, MailboxMessageView,
        MergeResult, OrchestratorStatus, PerspectiveForwardResult, PerspectiveSummary,
        RulebookInit, RulebookInstall, RulebookUninstall, RulebookValidation, WorkerDetail,
        WorkerState, WorkerSummary,
    },
    storage::{
        ActiveRulebookRecord, RuntimeFs, WorkerRecord, is_live_worker_state, timestamp_now,
        validate_skip_request,
    },
    worker_id::WorkerId,
};

/// Request to create one worker from a compiled perspective.
///
/// The orchestrator may provide `worker_id` to pin the runtime identity
/// used for mailbox routing and filesystem placement. When `worker_id`
/// is `None`, Multorum allocates the default perspective-based worker
/// id automatically.
///
/// Note: Explicit worker ids may be reused after a previous worker with
/// the same id reaches a finalized state. Reuse requires either a prior
/// workspace deletion or an explicit overwrite request when a preserved
/// finalized workspace still exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateWorker {
    /// Perspective to instantiate.
    pub perspective: PerspectiveName,
    /// Optional orchestrator-selected worker identity.
    pub worker_id: Option<WorkerId>,
    /// Optional initial `task` bundle to seed in the worker inbox.
    pub task: Option<BundlePayload>,
    /// Whether to replace an existing finalized workspace for the same
    /// explicit worker id.
    pub overwriting_worktree: bool,
}

impl CreateWorker {
    /// Construct a create request for one perspective.
    pub fn new(perspective: PerspectiveName) -> Self {
        Self { perspective, worker_id: None, task: None, overwriting_worktree: false }
    }

    /// Pin the worker to one orchestrator-selected runtime identity.
    pub fn with_worker_id(mut self, worker_id: WorkerId) -> Self {
        self.worker_id = Some(worker_id);
        self
    }

    /// Seed the new worker inbox with one initial `task` bundle.
    pub fn with_task(mut self, task: BundlePayload) -> Self {
        self.task = Some(task);
        self
    }

    /// Allow worker creation to replace an existing finalized workspace
    /// for the same explicit worker id.
    pub fn with_overwriting_worktree(mut self) -> Self {
        self.overwriting_worktree = true;
        self
    }
}

/// Typed operations available to the orchestrator frontend.
pub trait OrchestratorService {
    /// Initialize `.multorum/` with the default committed artifacts.
    fn rulebook_init(&self) -> Result<RulebookInit>;

    /// Dry-run validation of a rulebook install against HEAD.
    fn rulebook_validate(&self) -> Result<RulebookValidation>;

    /// Activate the HEAD rulebook after validation succeeds.
    fn rulebook_install(&self) -> Result<RulebookInstall>;

    /// Deactivate the active rulebook.
    ///
    /// Refused when any live bidding group still depends on the active
    /// rulebook.
    fn rulebook_uninstall(&self) -> Result<RulebookUninstall>;

    /// List compiled perspective summaries from the active rulebook.
    fn list_perspectives(&self) -> Result<Vec<PerspectiveSummary>>;

    /// List active workers.
    fn list_workers(&self) -> Result<Vec<WorkerSummary>>;

    /// Load one worker detail view.
    fn get_worker(&self, worker_id: WorkerId) -> Result<WorkerDetail>;

    /// Read one worker outbox after the provided sequence number.
    ///
    /// Note: The outbox remains worker-owned storage. The orchestrator
    /// addresses it by worker id rather than through a separate global
    /// mailbox.
    fn read_outbox(
        &self, worker_id: WorkerId, after: Option<Sequence>,
    ) -> Result<Vec<MailboxMessageView>>;

    /// Acknowledge one consumed worker outbox bundle.
    ///
    /// Note: Acknowledging outbox traffic records orchestrator receipt
    /// only; it does not change worker lifecycle state.
    fn ack_outbox(&self, worker_id: WorkerId, sequence: Sequence) -> Result<AckRef>;

    /// Create a worker workspace and optional initial task bundle.
    ///
    /// Any path-backed payload files are moved into `.multorum/` storage
    /// if publication succeeds.
    fn create_worker(&self, request: CreateWorker) -> Result<CreateResult>;

    /// Move one blocked bidding group to the active rulebook commit.
    ///
    /// Note: The whole live bidding group for `perspective` must be in
    /// `BLOCKED`. Multorum preserves progress only from the commit
    /// recorded in each worker's latest blocking report.
    fn forward_perspective(&self, perspective: PerspectiveName)
    -> Result<PerspectiveForwardResult>;

    /// Publish a `resolve` bundle to the worker inbox.
    ///
    /// Any path-backed payload files are moved into `.multorum/`
    /// storage if publication succeeds.
    fn resolve_worker(
        &self, worker_id: WorkerId, reply: ReplyReference, payload: BundlePayload,
    ) -> Result<PublishedBundle>;

    /// Publish a `revise` bundle to the worker inbox.
    ///
    /// Any path-backed payload files are moved into `.multorum/`
    /// storage if publication succeeds.
    fn revise_worker(
        &self, worker_id: WorkerId, reply: ReplyReference, payload: BundlePayload,
    ) -> Result<PublishedBundle>;

    /// Finalize a worker without integration while preserving its workspace.
    ///
    /// Note: Discard is allowed from `ACTIVE`, `BLOCKED`, and `COMMITTED`.
    fn discard_worker(&self, worker_id: WorkerId) -> Result<DiscardResult>;

    /// Delete one finalized worker workspace.
    fn delete_worker(&self, worker_id: WorkerId) -> Result<DeleteResult>;

    /// Run the pre-merge pipeline and merge the worker submission.
    ///
    /// The optional `audit_payload` attaches an orchestrator rationale
    /// to the audit entry written on success.
    fn merge_worker(
        &self, worker_id: WorkerId, skip_checks: Vec<String>, audit_payload: BundlePayload,
    ) -> Result<MergeResult>;

    /// Return the current orchestrator status projection.
    fn status(&self) -> Result<OrchestratorStatus>;
}

/// Storage-backed orchestrator runtime service.
///
/// The canonical `.multorum/` tree under the workspace root remains the
/// source of truth. This service validates operations, projects their
/// effects into runtime files, and delegates repository actions to the
/// configured version-control backend where required by the design.
#[derive(Debug, Clone)]
pub struct FsOrchestratorService {
    fs: RuntimeFs,
}

/// One active bidding-group boundary materialized from runtime state.
#[derive(Debug, Clone)]
struct ActiveBiddingGroup {
    perspective: PerspectiveName,
    worker_ids: Vec<WorkerId>,
    base_commit: CanonicalCommitHash,
    boundary: CompiledPerspective,
}

/// Forwarding checkpoint for one blocked worker.
#[derive(Debug, Clone)]
struct ForwardWorker {
    record: WorkerRecord,
    reported_head_commit: CanonicalCommitHash,
    original_boundary: CompiledPerspective,
}

impl FsOrchestratorService {
    /// Construct the orchestrator service for a workspace root.
    pub fn new(workspace_root: impl Into<PathBuf>) -> Result<Self> {
        Ok(Self { fs: RuntimeFs::new(workspace_root.into())? })
    }

    /// Construct the orchestrator service for a workspace root with an
    /// explicit repository backend.
    pub fn with_vcs(
        workspace_root: impl Into<PathBuf>, vcs: Arc<dyn VersionControl>,
    ) -> Result<Self> {
        Ok(Self { fs: RuntimeFs::with_vcs(workspace_root.into(), vcs)? })
    }

    /// Construct the orchestrator service from the current directory.
    ///
    /// The current path may be anywhere inside the canonical workspace,
    /// but it must resolve to the orchestrator repository rather than a
    /// managed worker worktree.
    pub fn from_current_dir() -> Result<Self> {
        let project = CurrentProject::from_current_dir()?;
        Self::new(project.orchestrator_workspace_root()?.to_path_buf())
    }

    /// Validate that a target rulebook commit is safe to activate.
    ///
    /// Enforces two conditions:
    ///
    /// **Continuity** — every active bidding group's perspective must
    /// exist in the target with a boundary that is a superset of (or
    /// equal to) the group's materialized boundary.
    ///
    /// Note: Compatible installs refresh each live worker's
    /// `read-set.txt` and `write-set.txt` to the target boundary while
    /// keeping the worker's base snapshot pinned. Conflict validation
    /// therefore must reason about the post-install boundary, not the
    /// stale pre-install projection.
    ///
    /// **Conflict-freedom** — every candidate perspective must satisfy
    /// the conflict-free invariant against every active group whose
    /// name differs from the candidate.
    fn validate_rulebook_commit(&self, commit: &CanonicalCommitHash) -> Result<RulebookValidation> {
        let compiled = self.fs.load_compiled_rulebook(commit)?;
        let active_groups = self.effective_active_bidding_groups(&compiled, commit)?;

        // Conflict-freedom: each candidate must not conflict with any
        // active group of a different name.
        let mut blocking_perspectives = BTreeSet::new();
        for active_group in &active_groups {
            for (candidate_name, candidate) in compiled.perspectives().perspectives() {
                if *candidate_name == active_group.perspective {
                    continue;
                }
                if boundary_conflict(
                    candidate_name,
                    candidate,
                    &active_group.perspective,
                    &active_group.boundary,
                )
                .is_some()
                {
                    blocking_perspectives.insert(active_group.perspective.clone());
                    break;
                }
            }
        }

        Ok(RulebookValidation {
            ok: blocking_perspectives.is_empty(),
            blocking_perspectives: blocking_perspectives.into_iter().collect(),
        })
    }

    /// Resolve the effective active-group boundaries under one target rulebook.
    ///
    /// Boundary reductions remain invalid, but compatible installs adopt
    /// the target boundary immediately for the live bidding group so the
    /// runtime exclusion set and future conflict checks stay aligned.
    fn effective_active_bidding_groups(
        &self, compiled: &CompiledRulebook, commit: &CanonicalCommitHash,
    ) -> Result<Vec<ActiveBiddingGroup>> {
        let mut active_groups = self.active_bidding_groups()?;

        for active_group in &mut active_groups {
            let Some(target) = compiled.perspectives().get(&active_group.perspective) else {
                return Err(RuntimeError::ActivePerspectiveIncompatible {
                    commit: commit.clone(),
                    perspective: active_group.perspective.clone(),
                    reason: "does not exist in the target rulebook",
                });
            };

            if !target.write().is_superset(active_group.boundary.write()) {
                return Err(RuntimeError::ActivePerspectiveIncompatible {
                    commit: commit.clone(),
                    perspective: active_group.perspective.clone(),
                    reason: "has a reduced write set in the target rulebook",
                });
            }

            if !target.read().is_superset(active_group.boundary.read()) {
                return Err(RuntimeError::ActivePerspectiveIncompatible {
                    commit: commit.clone(),
                    perspective: active_group.perspective.clone(),
                    reason: "has a reduced read set in the target rulebook",
                });
            }

            active_group.boundary = target.clone();
        }

        Ok(active_groups)
    }

    fn active_workers(&self) -> Result<Vec<WorkerRecord>> {
        Ok(self
            .fs
            .list_worker_records()?
            .into_iter()
            .filter(|record| is_live_worker_state(record.state))
            .collect())
    }

    fn active_bidding_groups(&self) -> Result<Vec<ActiveBiddingGroup>> {
        let active_workers = self.active_workers()?;
        let mut groups = Vec::new();
        let mut seen = BTreeSet::<PerspectiveName>::new();
        for record in active_workers.iter() {
            if !seen.insert(record.perspective.clone()) {
                continue;
            }

            let worker_paths = self.fs.worker_paths(&record.worker_id);
            let read = RuntimeFs::read_path_list(&worker_paths.read_set())?;
            let write = RuntimeFs::read_path_list(&worker_paths.write_set())?;
            let boundary = CompiledPerspective::from_materialized_sets(read, write);
            let group_workers = active_workers
                .iter()
                .filter(|worker| worker.perspective == record.perspective)
                .cloned()
                .collect::<Vec<_>>();
            for worker in &group_workers {
                if worker.base_commit != record.base_commit {
                    return Err(RuntimeError::BiddingGroupBaseMismatch {
                        perspective: record.perspective.clone(),
                    });
                }

                let worker_paths = self.fs.worker_paths(&worker.worker_id);
                let worker_read = RuntimeFs::read_path_list(&worker_paths.read_set())?;
                let worker_write = RuntimeFs::read_path_list(&worker_paths.write_set())?;
                if worker_read != *boundary.read() || worker_write != *boundary.write() {
                    return Err(RuntimeError::BiddingGroupBoundaryMismatch {
                        perspective: record.perspective.clone(),
                    });
                }
            }

            groups.push(ActiveBiddingGroup {
                perspective: record.perspective.clone(),
                worker_ids: group_workers.iter().map(|worker| worker.worker_id.clone()).collect(),
                base_commit: record.base_commit.clone(),
                boundary,
            });
        }
        Ok(groups)
    }

    /// Refresh live worker boundary files to match the installed rulebook.
    ///
    /// The worker keeps its pinned base commit. Only the materialized
    /// read/write-set files change.
    fn refresh_live_worker_boundaries(&self, compiled: &CompiledRulebook) -> Result<()> {
        for record in self.active_workers()? {
            let target = compiled
                .perspectives()
                .get(&record.perspective)
                .ok_or_else(|| RuntimeError::UnknownPerspective(record.perspective.to_string()))?;
            let worker_paths = self.fs.worker_paths(&record.worker_id);
            let current_read = RuntimeFs::read_path_list(&worker_paths.read_set())?;
            let current_write = RuntimeFs::read_path_list(&worker_paths.write_set())?;
            if current_read == *target.read() && current_write == *target.write() {
                continue;
            }

            self.fs.refresh_worker_boundary(&record, target)?;
            tracing::info!(
                worker_id = %record.worker_id,
                perspective = %record.perspective,
                read_count = target.read().len(),
                write_count = target.write().len(),
                "refreshed live worker boundary after rulebook install"
            );
        }

        Ok(())
    }

    fn allocate_worker_id(&self, perspective: &PerspectiveName) -> Result<WorkerId> {
        let prefix = format!("{}-", camel_to_kebab(perspective.as_str()));
        let next = self
            .fs
            .list_worker_records()?
            .into_iter()
            .filter(|record| record.perspective == *perspective)
            .filter_map(|record| {
                record.worker_id.as_str().strip_prefix(&prefix)?.parse::<u64>().ok()
            })
            .max()
            .unwrap_or(0)
            + 1;

        WorkerId::new(format!("{prefix}{next}"))
            .map_err(|_| RuntimeError::CheckFailed("failed to allocate worker id".to_owned()))
    }

    fn resolve_create_worker_id(
        &self, perspective: &PerspectiveName, worker_id: Option<WorkerId>,
    ) -> Result<(WorkerId, Option<WorkerRecord>)> {
        if let Some(worker_id) = worker_id {
            if let Some(record) = self
                .fs
                .list_worker_records()?
                .into_iter()
                .find(|record| record.worker_id == worker_id)
            {
                if is_live_worker_state(record.state) {
                    return Err(RuntimeError::WorkerIdExists(worker_id));
                }
                return Ok((worker_id, Some(record)));
            }
            return Ok((worker_id, None));
        }

        Ok((self.allocate_worker_id(perspective)?, None))
    }

    fn validate_create_boundary(
        &self, perspective: &PerspectiveName, candidate: &CompiledPerspective,
        active_base_commit: &CanonicalCommitHash,
    ) -> Result<()> {
        let active_groups = self.active_bidding_groups()?;
        if let Some(existing_group) =
            active_groups.iter().find(|group| group.perspective == *perspective)
        {
            if &existing_group.base_commit != active_base_commit {
                return Err(RuntimeError::PerspectiveRequiresForwardBeforeCreate {
                    perspective: perspective.clone(),
                    active_base_commit: active_base_commit.clone(),
                    live_base_commit: existing_group.base_commit.clone(),
                });
            }
            if existing_group.boundary.read() == candidate.read()
                && existing_group.boundary.write() == candidate.write()
            {
                return Ok(());
            }

            return Err(RuntimeError::BiddingGroupBoundaryMismatch {
                perspective: perspective.clone(),
            });
        }

        for active_group in &active_groups {
            if let Some(conflict) = boundary_conflict(
                perspective,
                candidate,
                &active_group.perspective,
                &active_group.boundary,
            ) {
                return Err(conflict);
            }
        }

        Ok(())
    }

    fn cleanup_workspace_before_create(&self, record: &WorkerRecord) -> Result<()> {
        self.fs.vcs().remove_worktree(self.fs.workspace_root(), &record.worktree_path)?;
        Ok(())
    }

    fn finalize_discarded_worker(&self, record: &mut WorkerRecord) -> Result<()> {
        // Note: Discard finalizes worker lifecycle state but preserves the
        // workspace so the orchestrator can inspect or delete it explicitly later.
        if record.worktree_path.exists() {
            tracing::trace!(
                worker_id = %record.worker_id,
                root = %record.worktree_path.display(),
                "preserving discarded worker workspace"
            );
        }
        record.state = WorkerState::Discarded;
        record.submitted_head_commit = None;
        self.fs.store_worker_record(record)
    }

    fn delete_worker_workspace(&self, record: &WorkerRecord) -> Result<bool> {
        // Note: finalized worktrees must be deleted through the VCS
        // backend so Git drops the administrative entry even when the
        // worktree directory has already been removed manually.
        Ok(self.fs.vcs().remove_worktree(self.fs.workspace_root(), &record.worktree_path)?)
    }

    fn publish_worker_inbox(
        &self, worker_id: &WorkerId, kind: MessageKind, reply: ReplyReference,
        payload: BundlePayload,
    ) -> Result<PublishedBundle> {
        let record = self.fs.load_worker_record(worker_id)?;
        let expected_state = match kind {
            | MessageKind::Resolve if record.state == WorkerState::Blocked => {
                Some(WorkerState::Blocked)
            }
            | MessageKind::Revise if record.state == WorkerState::Committed => {
                Some(WorkerState::Committed)
            }
            | _ => None,
        };
        if expected_state.is_some() {
            return self.fs.publish_bundle(
                &record.worktree_path,
                MailboxDirection::Inbox,
                kind,
                &record.worker_id,
                &record.perspective,
                reply,
                None,
                payload,
            );
        }

        let expected = match kind {
            | MessageKind::Resolve => "BLOCKED",
            | MessageKind::Revise => "COMMITTED",
            | _ => "a state that accepts inbox publication",
        };
        Err(RuntimeError::InvalidState {
            operation: match kind {
                | MessageKind::Resolve => "publish resolve bundle",
                | MessageKind::Revise => "publish revise bundle",
                | _ => "publish inbox bundle",
            },
            expected,
            actual: record.state,
        })
    }

    fn load_forward_workers(&self, perspective: &PerspectiveName) -> Result<Vec<ForwardWorker>> {
        let workers = self
            .active_workers()?
            .into_iter()
            .filter(|record| record.perspective == *perspective)
            .collect::<Vec<_>>();
        if workers.is_empty() {
            return Err(RuntimeError::PerspectiveForwardMissingGroup {
                perspective: perspective.clone(),
            });
        }

        let mut non_blocked = workers
            .iter()
            .filter(|record| record.state != WorkerState::Blocked)
            .map(|record| (record.worker_id.clone(), record.state))
            .collect::<Vec<_>>();
        if !non_blocked.is_empty() {
            non_blocked.sort_by(|left, right| left.0.cmp(&right.0));
            return Err(RuntimeError::PerspectiveForwardRequiresBlocked {
                perspective: perspective.clone(),
                workers: non_blocked,
            });
        }

        let mut prepared = Vec::new();
        for record in workers {
            let messages = self.fs.list_mailbox_messages(
                &record.worktree_path,
                &record.worker_id,
                MailboxDirection::Outbox,
                None,
            )?;
            let report = messages
                .into_iter()
                .rev()
                .find(|message| message.kind == MessageKind::Report)
                .ok_or_else(|| RuntimeError::PerspectiveForwardMissingReport {
                    worker_id: record.worker_id.clone(),
                    perspective: record.perspective.clone(),
                })?;
            let reported_head_commit = report.head_commit.ok_or_else(|| {
                RuntimeError::PerspectiveForwardMissingReportedHead {
                    worker_id: record.worker_id.clone(),
                    perspective: record.perspective.clone(),
                }
            })?;

            self.fs.vcs().ensure_clean_worktree(&record.worktree_path)?;
            let current_head_commit = self.fs.vcs().head_commit(&record.worktree_path)?;
            if current_head_commit != reported_head_commit {
                return Err(RuntimeError::PerspectiveForwardHeadMismatch {
                    worker_id: record.worker_id.clone(),
                    perspective: record.perspective.clone(),
                    reported_head_commit,
                    current_head_commit,
                });
            }

            let worker_paths = self.fs.worker_paths(&record.worker_id);
            let read = RuntimeFs::read_path_list(&worker_paths.read_set())?;
            let write = RuntimeFs::read_path_list(&worker_paths.write_set())?;
            prepared.push(ForwardWorker {
                record,
                reported_head_commit: current_head_commit,
                original_boundary: CompiledPerspective::from_materialized_sets(read, write),
            });
        }
        prepared.sort_by(|left, right| left.record.worker_id.cmp(&right.record.worker_id));
        Ok(prepared)
    }

    fn rollback_forward_worktrees(
        &self, workers: &[ForwardWorker], forwarded_worker_ids: &BTreeSet<WorkerId>,
    ) {
        for worker in
            workers.iter().filter(|worker| forwarded_worker_ids.contains(&worker.record.worker_id))
        {
            if let Err(error) = self
                .fs
                .vcs()
                .checkout_detached(&worker.record.worktree_path, &worker.reported_head_commit)
            {
                tracing::error!(
                    worker_id = %worker.record.worker_id,
                    error = %error,
                    "failed to roll back forwarded worker worktree"
                );
            }
        }
    }

    fn restore_forward_metadata(
        &self, workers: &[ForwardWorker], restored_worker_ids: &BTreeSet<WorkerId>,
    ) {
        for worker in
            workers.iter().filter(|worker| restored_worker_ids.contains(&worker.record.worker_id))
        {
            if let Err(error) = self.fs.store_worker_record(&worker.record) {
                tracing::error!(
                    worker_id = %worker.record.worker_id,
                    error = %error,
                    "failed to restore worker record after forward error"
                );
            }
            if let Err(error) = self.fs.refresh_worker_contract(&worker.record) {
                tracing::error!(
                    worker_id = %worker.record.worker_id,
                    error = %error,
                    "failed to restore worker contract after forward error"
                );
            }
            if let Err(error) =
                self.fs.refresh_worker_boundary(&worker.record, &worker.original_boundary)
            {
                tracing::error!(
                    worker_id = %worker.record.worker_id,
                    error = %error,
                    "failed to restore worker boundary after forward error"
                );
            }
        }
    }
}

impl OrchestratorService for FsOrchestratorService {
    fn rulebook_init(&self) -> Result<RulebookInit> {
        self.fs.initialize_rulebook()
    }

    fn rulebook_validate(&self) -> Result<RulebookValidation> {
        let commit = self.fs.vcs().head_commit(self.fs.workspace_root())?;
        self.validate_rulebook_commit(&commit)
    }

    fn rulebook_install(&self) -> Result<RulebookInstall> {
        let commit = self.fs.vcs().head_commit(self.fs.workspace_root())?;
        let validation = self.validate_rulebook_commit(&commit)?;
        if !validation.ok {
            return Err(RuntimeError::RulebookConflict {
                commit,
                blocking_perspectives: validation.blocking_perspectives,
            });
        }
        let compiled = self.fs.load_compiled_rulebook(&commit)?;

        let record =
            ActiveRulebookRecord { base_commit: commit.clone(), activated_at: timestamp_now() };
        self.fs.store_active_rulebook(&record)?;
        self.refresh_live_worker_boundaries(&compiled)?;
        self.fs.rewrite_exclusion_set()?;
        self.fs.vcs().install_orchestrator_hook(self.fs.workspace_root())?;
        tracing::info!(base_commit = %record.base_commit, "installed rulebook");
        Ok(RulebookInstall { active_commit: record.base_commit })
    }

    fn rulebook_uninstall(&self) -> Result<RulebookUninstall> {
        let active = self.fs.load_active_rulebook()?;
        let active_workers = self.active_workers()?;
        if !active_workers.is_empty() {
            let blocking_perspectives = active_workers
                .iter()
                .map(|record| record.perspective.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            return Err(RuntimeError::RulebookConflict {
                commit: active.base_commit,
                blocking_perspectives,
            });
        }

        self.fs.remove_active_rulebook()?;
        self.fs.rewrite_exclusion_set()?;
        tracing::info!(base_commit = %active.base_commit, "uninstalled rulebook");
        Ok(RulebookUninstall { previous_commit: active.base_commit })
    }

    fn list_perspectives(&self) -> Result<Vec<PerspectiveSummary>> {
        let (_, compiled) = self.fs.load_active_compiled_rulebook()?;
        Ok(compiled.perspective_summaries())
    }

    fn list_workers(&self) -> Result<Vec<WorkerSummary>> {
        let mut workers = self
            .active_workers()?
            .into_iter()
            .map(|record| WorkerSummary {
                worker_id: record.worker_id,
                perspective: record.perspective,
                state: record.state,
            })
            .collect::<Vec<_>>();
        workers.sort_by(|left, right| left.worker_id.cmp(&right.worker_id));
        Ok(workers)
    }

    fn get_worker(&self, worker_id: WorkerId) -> Result<WorkerDetail> {
        let record = self.fs.load_worker_record(&worker_id)?;
        Ok(WorkerDetail {
            worker_id: record.worker_id,
            perspective: record.perspective,
            state: record.state,
            worktree_path: record.worktree_path,
            base_commit: record.base_commit,
            submitted_head_commit: record.submitted_head_commit,
        })
    }

    fn read_outbox(
        &self, worker_id: WorkerId, after: Option<Sequence>,
    ) -> Result<Vec<MailboxMessageView>> {
        let record = self.fs.load_worker_record(&worker_id)?;
        self.fs.list_mailbox_messages(
            &record.worktree_path,
            &record.worker_id,
            MailboxDirection::Outbox,
            after,
        )
    }

    fn ack_outbox(&self, worker_id: WorkerId, sequence: Sequence) -> Result<AckRef> {
        tracing::trace!(worker_id = %worker_id, sequence = sequence.0, "acknowledging worker outbox message");
        let record = self.fs.load_worker_record(&worker_id)?;
        let ack = self.fs.acknowledge_message(
            &record.worktree_path,
            MailboxDirection::Outbox,
            sequence,
        )?;
        tracing::info!(worker_id = %record.worker_id, sequence = sequence.0, "acknowledged worker outbox message");
        Ok(ack)
    }

    fn create_worker(&self, request: CreateWorker) -> Result<CreateResult> {
        let CreateWorker { perspective, worker_id, task, overwriting_worktree } = request;
        let (active, compiled) = self.fs.load_active_compiled_rulebook()?;
        let compiled_perspective = compiled
            .perspectives()
            .get(&perspective)
            .ok_or_else(|| RuntimeError::UnknownPerspective(perspective.to_string()))?;
        self.validate_create_boundary(&perspective, compiled_perspective, &active.base_commit)?;

        let (worker_id, previous_finalized_record) =
            self.resolve_create_worker_id(&perspective, worker_id)?;
        if let Some(record) = previous_finalized_record.as_ref()
            && record.worktree_path.exists()
        {
            if !overwriting_worktree {
                return Err(RuntimeError::ExistingWorkerWorkspace {
                    worker_id: record.worker_id.clone(),
                    state: record.state,
                    worktree_path: record.worktree_path.clone(),
                });
            }
            self.cleanup_workspace_before_create(record)?;
        }
        let worktree_path = self.fs.worker_paths(&worker_id).worktree_root().to_path_buf();
        self.fs.vcs().create_worktree(
            self.fs.workspace_root(),
            &worktree_path,
            &active.base_commit,
        )?;

        let record = WorkerRecord {
            worker_id: worker_id.clone(),
            perspective: perspective.clone(),
            state: WorkerState::Active,
            worktree_path: worktree_path.clone(),
            base_commit: active.base_commit,
            submitted_head_commit: None,
        };
        self.fs.prepare_worker_runtime(&record, compiled_perspective)?;
        self.fs.store_worker_record(&record)?;

        let seeded_task_path = if let Some(payload) = task {
            Some(
                self.fs
                    .publish_bundle(
                        &worktree_path,
                        MailboxDirection::Inbox,
                        MessageKind::Task,
                        &worker_id,
                        &perspective,
                        ReplyReference::default(),
                        None,
                        payload,
                    )?
                    .bundle_path,
            )
        } else {
            None
        };

        self.fs.rewrite_exclusion_set()?;

        tracing::trace!(
            worker_id = %worker_id,
            perspective = %perspective,
            root = %worktree_path.display(),
            "creating worker worktree"
        );

        let result = CreateResult {
            worker_id,
            perspective,
            worktree_path: worktree_path.clone(),
            state: WorkerState::Active,
            seeded_task_path,
        };

        tracing::info!(
            worker_id = %result.worker_id,
            perspective = %result.perspective,
            root = %result.worktree_path.display(),
            "created active worker"
        );

        Ok(result)
    }

    fn forward_perspective(
        &self, perspective: PerspectiveName,
    ) -> Result<PerspectiveForwardResult> {
        let (active, compiled) = self.fs.load_active_compiled_rulebook()?;
        let target = compiled
            .perspectives()
            .get(&perspective)
            .ok_or_else(|| RuntimeError::UnknownPerspective(perspective.to_string()))?
            .clone();
        let workers = self.load_forward_workers(&perspective)?;
        let previous_base_commit =
            workers.first().map(|worker| worker.record.base_commit.clone()).ok_or_else(|| {
                RuntimeError::PerspectiveForwardMissingGroup { perspective: perspective.clone() }
            })?;
        let worker_ids =
            workers.iter().map(|worker| worker.record.worker_id.clone()).collect::<Vec<_>>();

        if previous_base_commit == active.base_commit {
            return Ok(PerspectiveForwardResult {
                perspective,
                worker_ids,
                previous_base_commit,
                active_base_commit: active.base_commit,
            });
        }

        let mut forwarded_worker_ids = BTreeSet::new();
        for worker in &workers {
            if let Err(error) = self.fs.vcs().forward_worktree(
                &worker.record.worktree_path,
                &worker.record.base_commit,
                &active.base_commit,
            ) {
                self.rollback_forward_worktrees(&workers, &forwarded_worker_ids);
                return Err(RuntimeError::from(error));
            }
            forwarded_worker_ids.insert(worker.record.worker_id.clone());
        }

        let mut restored_worker_ids = BTreeSet::new();
        for worker in &workers {
            let mut updated = worker.record.clone();
            updated.base_commit = active.base_commit.clone();
            updated.submitted_head_commit = None;
            if let Err(error) = self.fs.store_worker_record(&updated) {
                self.rollback_forward_worktrees(&workers, &forwarded_worker_ids);
                self.restore_forward_metadata(&workers, &restored_worker_ids);
                return Err(error);
            }
            if let Err(error) = self.fs.refresh_worker_contract(&updated) {
                self.rollback_forward_worktrees(&workers, &forwarded_worker_ids);
                self.restore_forward_metadata(&workers, &restored_worker_ids);
                return Err(error);
            }
            if let Err(error) = self.fs.refresh_worker_boundary(&updated, &target) {
                self.rollback_forward_worktrees(&workers, &forwarded_worker_ids);
                self.restore_forward_metadata(&workers, &restored_worker_ids);
                return Err(error);
            }
            restored_worker_ids.insert(updated.worker_id.clone());
        }

        if let Err(error) = self.fs.rewrite_exclusion_set() {
            self.rollback_forward_worktrees(&workers, &forwarded_worker_ids);
            self.restore_forward_metadata(&workers, &restored_worker_ids);
            return Err(error);
        }

        tracing::info!(
            perspective = %perspective,
            worker_count = worker_ids.len(),
            previous_base_commit = %previous_base_commit,
            active_base_commit = %active.base_commit,
            "forwarded blocked bidding group to active rulebook commit"
        );

        Ok(PerspectiveForwardResult {
            perspective,
            worker_ids,
            previous_base_commit,
            active_base_commit: active.base_commit,
        })
    }

    fn resolve_worker(
        &self, worker_id: WorkerId, reply: ReplyReference, payload: BundlePayload,
    ) -> Result<PublishedBundle> {
        tracing::trace!(worker_id = %worker_id, "publishing resolve bundle to worker inbox");
        self.publish_worker_inbox(&worker_id, MessageKind::Resolve, reply, payload)
    }

    fn revise_worker(
        &self, worker_id: WorkerId, reply: ReplyReference, payload: BundlePayload,
    ) -> Result<PublishedBundle> {
        tracing::trace!(worker_id = %worker_id, "publishing revise bundle to worker inbox");
        self.publish_worker_inbox(&worker_id, MessageKind::Revise, reply, payload)
    }

    fn discard_worker(&self, worker_id: WorkerId) -> Result<DiscardResult> {
        let mut record = self.fs.load_worker_record(&worker_id)?;
        if !matches!(
            record.state,
            WorkerState::Active | WorkerState::Blocked | WorkerState::Committed
        ) {
            return Err(RuntimeError::InvalidState {
                operation: "discard worker",
                expected: "ACTIVE, BLOCKED, or COMMITTED",
                actual: record.state,
            });
        }

        self.finalize_discarded_worker(&mut record)?;
        self.fs.rewrite_exclusion_set()?;

        tracing::info!(worker_id = %record.worker_id, perspective = %record.perspective, "discarded worker");
        Ok(DiscardResult {
            worker_id: record.worker_id,
            perspective: record.perspective,
            state: record.state,
        })
    }

    fn delete_worker(&self, worker_id: WorkerId) -> Result<DeleteResult> {
        let record = self.fs.load_worker_record(&worker_id)?;
        if !matches!(record.state, WorkerState::Merged | WorkerState::Discarded) {
            return Err(RuntimeError::InvalidState {
                operation: "delete worker workspace",
                expected: "MERGED or DISCARDED",
                actual: record.state,
            });
        }

        let deleted_workspace = self.delete_worker_workspace(&record)?;
        let deleted_state_file = self.fs.delete_worker_record(&record.worker_id)?;
        tracing::info!(
            worker_id = %record.worker_id,
            perspective = %record.perspective,
            deleted_workspace,
            deleted_state_file,
            "deleted worker workspace"
        );
        Ok(DeleteResult {
            worker_id: record.worker_id,
            perspective: record.perspective,
            state: record.state,
            worktree_path: record.worktree_path,
            deleted_workspace,
            deleted_state_file,
        })
    }

    fn merge_worker(
        &self, worker_id: WorkerId, skip_checks: Vec<String>, audit_payload: BundlePayload,
    ) -> Result<MergeResult> {
        tracing::trace!(worker_id = %worker_id, "starting worker merge");
        let mut record = self.fs.load_worker_record(&worker_id)?;
        if record.state != WorkerState::Committed {
            return Err(RuntimeError::InvalidState {
                operation: "merge worker",
                expected: "COMMITTED",
                actual: record.state,
            });
        }

        let head_commit = record.submitted_head_commit.clone().ok_or_else(|| {
            RuntimeError::MissingSubmittedHeadCommit {
                worker_id: worker_id.clone(),
                state: record.state,
            }
        })?;
        let head_commit = self.fs.vcs().resolve_commit(
            &record.worktree_path,
            head_commit.as_str(),
            "verify submitted worker commit",
        )?;

        if head_commit == record.base_commit {
            return Err(RuntimeError::NoNewCommit {
                worker_id: worker_id.clone(),
                head_commit: head_commit.clone(),
            });
        }
        let worker_head = self.fs.vcs().head_commit(&record.worktree_path)?;
        if worker_head != head_commit {
            return Err(RuntimeError::WorkerHeadMismatch {
                worker_id: worker_id.clone(),
                submitted_head_commit: head_commit,
                current_head_commit: worker_head,
            });
        }

        tracing::trace!(worker_id = %worker_id, head_commit = %head_commit, "verified submitted commit");

        let worker_rulebook = self.fs.load_compiled_rulebook(&record.base_commit)?;
        let allowed_skips = validate_skip_request(&worker_rulebook, &skip_checks)?;
        let changed_files = self.fs.vcs().changed_files(
            &record.worktree_path,
            &record.base_commit,
            &head_commit,
        )?;
        let write_set =
            RuntimeFs::read_path_list(&self.fs.worker_paths(&record.worker_id).write_set())?;
        let violations = changed_files.difference(&write_set).cloned().collect::<BTreeSet<_>>();
        if !violations.is_empty() {
            tracing::warn!(worker_id = %worker_id, count = violations.len(), "write-set violation");
            return Err(RuntimeError::WriteSetViolation {
                worker_id: worker_id.clone(),
                perspective: record.perspective.clone(),
                base_commit: record.base_commit.clone(),
                head_commit: head_commit.clone(),
                violations: violations.into_iter().collect(),
            });
        }

        let mut ran_checks = Vec::new();
        let mut skipped_checks = Vec::new();
        for check_name in worker_rulebook.check().pipeline() {
            let decl = worker_rulebook
                .check()
                .get(check_name)
                .expect("compiled checks contain every pipeline entry");
            if allowed_skips.contains(check_name) {
                skipped_checks.push(check_name.to_string());
                tracing::trace!(worker_id = %worker_id, check = %check_name, "skipping check");
                continue;
            }

            tracing::trace!(worker_id = %worker_id, check = %check_name, "running pre-merge check");
            self.fs.run_check(&record.worktree_path, check_name, decl.command())?;
            ran_checks.push(check_name.to_string());
        }

        tracing::trace!(worker_id = %worker_id, "ensuring clean workspace before merge");
        self.fs.vcs().ensure_clean_workspace(self.fs.workspace_root())?;
        tracing::trace!(worker_id = %worker_id, head_commit = %head_commit, "integrating commit");
        self.fs.vcs().integrate_commit(self.fs.workspace_root(), &head_commit)?;

        // Note: Merge finalizes worker lifecycle state but preserves the
        // workspace so the orchestrator can inspect or delete it explicitly later.
        record.state = WorkerState::Merged;
        self.fs.store_worker_record(&record)?;

        for mut sibling in self.active_workers()?.into_iter().filter(|sibling| {
            sibling.worker_id != record.worker_id && sibling.perspective == record.perspective
        }) {
            tracing::trace!(
                worker_id = %sibling.worker_id,
                perspective = %sibling.perspective,
                "finalizing sibling worker"
            );
            self.finalize_discarded_worker(&mut sibling)?;
        }
        self.fs.rewrite_exclusion_set()?;
        self.fs.write_audit_entry(
            &record,
            &head_commit,
            &changed_files,
            &ran_checks,
            &skipped_checks,
            audit_payload,
        )?;

        tracing::info!(
            worker_id = %record.worker_id,
            perspective = %record.perspective,
            head_commit = %head_commit,
            "merged worker"
        );
        Ok(MergeResult {
            worker_id: record.worker_id,
            perspective: record.perspective,
            state: record.state,
            ran_checks,
            skipped_checks,
        })
    }

    fn status(&self) -> Result<OrchestratorStatus> {
        let active_rulebook_commit = self.fs.load_active_rulebook()?.base_commit;
        let mut active_perspectives = self
            .active_bidding_groups()?
            .into_iter()
            .map(|group| ActivePerspectiveSummary {
                perspective: group.perspective,
                worker_ids: group.worker_ids,
                read_count: group.boundary.read().len(),
                write_count: group.boundary.write().len(),
            })
            .collect::<Vec<_>>();
        active_perspectives.sort_by(|left, right| left.perspective.cmp(&right.perspective));
        let workers = self.list_workers()?;

        Ok(OrchestratorStatus { active_rulebook_commit, active_perspectives, workers })
    }
}

fn boundary_conflict(
    candidate_name: &PerspectiveName, candidate: &CompiledPerspective,
    active_name: &PerspectiveName, active: &CompiledPerspective,
) -> Option<RuntimeError> {
    let write_write =
        candidate.write().intersection(active.write()).cloned().collect::<BTreeSet<_>>();
    if !write_write.is_empty() {
        return Some(RuntimeError::ConflictWithActiveBiddingGroup {
            perspective: candidate_name.clone(),
            blocking_perspective: active_name.clone(),
            relation: "write/write overlap",
            files: write_write.into_iter().collect(),
        });
    }

    let candidate_write_active_read =
        candidate.write().intersection(active.read()).cloned().collect::<BTreeSet<_>>();
    if !candidate_write_active_read.is_empty() {
        return Some(RuntimeError::ConflictWithActiveBiddingGroup {
            perspective: candidate_name.clone(),
            blocking_perspective: active_name.clone(),
            relation: "candidate write overlaps active read",
            files: candidate_write_active_read.into_iter().collect(),
        });
    }

    let candidate_read_active_write =
        candidate.read().intersection(active.write()).cloned().collect::<BTreeSet<_>>();
    if !candidate_read_active_write.is_empty() {
        return Some(RuntimeError::ConflictWithActiveBiddingGroup {
            perspective: candidate_name.clone(),
            blocking_perspective: active_name.clone(),
            relation: "candidate read overlaps active write",
            files: candidate_read_active_write.into_iter().collect(),
        });
    }

    None
}

/// Convert an `UpperCamelCase` name to `kebab-case`.
///
/// Inserts a hyphen before each uppercase letter (except the first)
/// and lowercases the entire result.
fn camel_to_kebab(name: &str) -> String {
    let mut result = String::with_capacity(name.len() + 4);
    for (i, ch) in name.char_indices() {
        if ch.is_ascii_uppercase() && i > 0 {
            result.push('-');
        }
        result.push(ch.to_ascii_lowercase());
    }
    result
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use tempfile::TempDir;

    use crate::bundle::BundlePayload;
    use crate::runtime::{FsWorkerService, ReplyReference, WorkerContractView, WorkerService};

    use super::*;

    fn perspective() -> PerspectiveName {
        PerspectiveName::new("AuthImplementor").unwrap()
    }

    fn initial_rulebook() -> &'static str {
        r#"
            [fileset]
            Owned.path = "src/owned.rs"
            Other.path = "src/other.rs"

            [perspective.AuthImplementor]
            read = "Other"
            write = "Owned"

            [check]
            pipeline = []
        "#
    }

    fn expanded_rulebook() -> &'static str {
        r#"
            [fileset]
            Owned.path = "src/owned.rs"
            NewOwned.path = "src/new.rs"
            Other.path = "src/other.rs"

            [perspective.AuthImplementor]
            read = "Other"
            write = "Owned | NewOwned"

            [check]
            pipeline = []
        "#
    }

    fn git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git").args(args).current_dir(root).output().unwrap();
        if !output.status.success() {
            panic!("git {:?} failed: {}", args, String::from_utf8_lossy(&output.stderr));
        }
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn setup_repo() -> (TempDir, FsOrchestratorService) {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::create_dir_all(dir.path().join(".multorum")).unwrap();
        fs::write(dir.path().join("src/owned.rs"), "pub fn owned() -> i32 { 1 }\n").unwrap();
        fs::write(dir.path().join("src/other.rs"), "pub fn other() -> i32 { 2 }\n").unwrap();
        fs::write(dir.path().join(".multorum/.gitignore"), "orchestrator/\ntr/\n").unwrap();
        fs::write(dir.path().join(".multorum/rulebook.toml"), initial_rulebook()).unwrap();

        git(dir.path(), &["init"]);
        git(dir.path(), &["config", "user.name", "Multorum Test"]);
        git(dir.path(), &["config", "user.email", "multorum@test.invalid"]);
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "feat: initialize runtime fixture"]);

        let orchestrator = FsOrchestratorService::new(dir.path()).unwrap();
        orchestrator.rulebook_install().unwrap();
        (dir, orchestrator)
    }

    fn expand_rulebook(dir: &TempDir, orchestrator: &FsOrchestratorService) -> CanonicalCommitHash {
        fs::write(dir.path().join("src/new.rs"), "pub fn new_owned() -> i32 { 3 }\n").unwrap();
        fs::write(dir.path().join(".multorum/rulebook.toml"), expanded_rulebook()).unwrap();
        git(dir.path(), &["add", "src/new.rs", ".multorum/rulebook.toml"]);
        git(dir.path(), &["commit", "-m", "incr: expand perspective write set"]);
        orchestrator.rulebook_install().unwrap().active_commit
    }

    fn worker_service(worktree_root: &Path) -> FsWorkerService {
        FsWorkerService::new(worktree_root).unwrap()
    }

    #[test]
    fn forward_perspective_replays_blocked_worker_and_updates_runtime_state() {
        let (dir, orchestrator) = setup_repo();
        let worker = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
        let worker_service = worker_service(&worker.worktree_path);

        fs::write(worker.worktree_path.join("src/owned.rs"), "pub fn owned() -> i32 { 11 }\n")
            .unwrap();
        git(&worker.worktree_path, &["add", "src/owned.rs"]);
        git(&worker.worktree_path, &["commit", "-m", "incr: update owned worker code"]);
        let reported_head = git(&worker.worktree_path, &["rev-parse", "HEAD"]);
        worker_service
            .send_report(
                Some(reported_head.clone()),
                ReplyReference::default(),
                BundlePayload::default(),
            )
            .unwrap();

        let active_commit = expand_rulebook(&dir, &orchestrator);
        let result = orchestrator.forward_perspective(perspective()).unwrap();

        assert_eq!(result.worker_ids, vec![worker.worker_id.clone()]);
        assert_eq!(result.active_base_commit, active_commit);
        assert_ne!(result.previous_base_commit, result.active_base_commit);

        let worker_detail = orchestrator.get_worker(worker.worker_id.clone()).unwrap();
        assert_eq!(worker_detail.base_commit, active_commit);
        assert_eq!(worker_detail.state, WorkerState::Blocked);

        let contract: WorkerContractView = worker_service.contract().unwrap();
        assert_eq!(contract.base_commit, active_commit);

        let forwarded_head = git(&worker.worktree_path, &["rev-parse", "HEAD"]);
        assert_ne!(forwarded_head, reported_head);

        let changed_files = git(
            &worker.worktree_path,
            &["diff", "--name-only", &format!("{}..HEAD", active_commit)],
        );
        assert_eq!(changed_files, "src/owned.rs");

        let write_set =
            fs::read_to_string(worker.worktree_path.join(".multorum/write-set.txt")).unwrap();
        assert!(write_set.lines().any(|line| line == "src/new.rs"));
    }

    #[test]
    fn forward_perspective_rejects_active_workers() {
        let (dir, orchestrator) = setup_repo();
        let worker = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
        let _active_commit = expand_rulebook(&dir, &orchestrator);

        let error = orchestrator.forward_perspective(perspective()).unwrap_err();
        assert!(matches!(
            error,
            RuntimeError::PerspectiveForwardRequiresBlocked { ref workers, .. }
                if workers == &vec![(worker.worker_id, WorkerState::Active)]
        ));
    }

    #[test]
    fn forward_perspective_requires_reported_head_commit() {
        let (dir, orchestrator) = setup_repo();
        let worker = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
        worker_service(&worker.worktree_path)
            .send_report(None, ReplyReference::default(), BundlePayload::default())
            .unwrap();
        let _active_commit = expand_rulebook(&dir, &orchestrator);

        let error = orchestrator.forward_perspective(perspective()).unwrap_err();
        assert!(matches!(
            error,
            RuntimeError::PerspectiveForwardMissingReportedHead { worker_id, .. }
                if worker_id == worker.worker_id
        ));
    }

    #[test]
    fn create_worker_rejects_same_perspective_until_group_is_forwarded() {
        let (dir, orchestrator) = setup_repo();
        let worker = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
        let worker_detail = orchestrator.get_worker(worker.worker_id.clone()).unwrap();
        worker_service(&worker.worktree_path)
            .send_report(None, ReplyReference::default(), BundlePayload::default())
            .unwrap();
        let active_commit = expand_rulebook(&dir, &orchestrator);

        let error = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap_err();
        assert!(matches!(
            error,
            RuntimeError::PerspectiveRequiresForwardBeforeCreate {
                ref perspective,
                ref active_base_commit,
                ref live_base_commit,
            } if perspective == &worker.perspective
                && active_base_commit == &active_commit
                && live_base_commit == &worker_detail.base_commit
        ));
    }
}
