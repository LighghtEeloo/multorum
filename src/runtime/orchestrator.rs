//! Orchestrator-facing runtime service surface.
//!
//! This module defines the typed operations available to orchestrator
//! frontends and the default storage-backed implementation used by
//! the CLI.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use crate::schema::perspective::{CompiledPerspective, PerspectiveName};
use crate::vcs::{CanonicalCommitHash, VersionControl};

use crate::bundle::BundlePayload;

use super::{
    error::{Result, RuntimeError},
    mailbox::{AckRef, MailboxDirection, MessageKind, PublishedBundle, ReplyReference, Sequence},
    project::CurrentProject,
    state::{
        ActivePerspectiveSummary, CreateResult, DeleteResult, DiscardResult, MailboxMessageView,
        MergeResult, OrchestratorStatus, PerspectiveSummary, RulebookInit, RulebookInstall,
        RulebookUninstall, RulebookValidation, WorkerDetail, WorkerState, WorkerSummary,
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
    boundary: CompiledPerspective,
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
    /// **Conflict-freedom** — every candidate perspective must satisfy
    /// the conflict-free invariant against every active group whose
    /// name differs from the candidate.
    fn validate_rulebook_commit(&self, commit: &CanonicalCommitHash) -> Result<RulebookValidation> {
        let compiled = self.fs.load_compiled_rulebook(commit)?;
        let active_groups = self.active_bidding_groups()?;

        // Continuity: each active group's perspective must still exist
        // with a boundary that is a superset of the materialized one.
        for active_group in &active_groups {
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
        }

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
            groups.push(ActiveBiddingGroup {
                perspective: record.perspective.clone(),
                worker_ids: active_workers
                    .iter()
                    .filter(|worker| worker.perspective == record.perspective)
                    .map(|worker| worker.worker_id.clone())
                    .collect(),
                boundary: CompiledPerspective::from_materialized_sets(read, write),
            });
        }
        Ok(groups)
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
    ) -> Result<()> {
        let active_groups = self.active_bidding_groups()?;
        if let Some(existing_group) =
            active_groups.iter().find(|group| group.perspective == *perspective)
        {
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

        let record =
            ActiveRulebookRecord { base_commit: commit.clone(), activated_at: timestamp_now() };
        self.fs.store_active_rulebook(&record)?;
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
        self.validate_create_boundary(&perspective, compiled_perspective)?;

        let (worker_id, previous_finalized_record) =
            self.resolve_create_worker_id(&perspective, worker_id)?;
        if let Some(record) = previous_finalized_record.as_ref()
            && record.worktree_path.exists() {
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
        if !matches!(record.state, WorkerState::Active | WorkerState::Committed) {
            return Err(RuntimeError::InvalidState {
                operation: "discard worker",
                expected: "ACTIVE or COMMITTED",
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
