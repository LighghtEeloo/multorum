//! Orchestrator-facing runtime service surface.
//!
//! This module defines the typed operations available to orchestrator
//! frontends and the default storage-backed implementation used by
//! the CLI.
//!
//! The rulebook is read from disk (the working tree) whenever needed.
//! There is no activation or pinning step — the rulebook is a
//! declaration file that Multorum consults at operation time.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use crate::{
    bundle::BundlePayload,
    schema::perspective::{CompiledPerspective, PerspectiveName},
    vcs::{CanonicalCommitHash, VersionControl},
};

use super::{
    error::{Result, RuntimeError},
    mailbox::{AckRef, MailboxDirection, MessageKind, PublishedBundle, ReplyReference, Sequence},
    project::CurrentProject,
    state::{
        ActivePerspectiveSummary, CreateResult, DeleteResult, DiscardResult, MailboxMessageView,
        MergeResult, OrchestratorStatus, PerspectiveConflict, PerspectiveForwardResult,
        PerspectiveSummary, PerspectiveValidation, RulebookInit, WorkerDetail, WorkerState,
        WorkerSummary,
    },
    storage::{BiddingGroupRecord, RuntimeFs, StateFile, WorkerEntry, validate_skip_request},
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

    /// List compiled perspective summaries from the current rulebook.
    fn list_perspectives(&self) -> Result<Vec<PerspectiveSummary>>;

    /// Validate a set of perspectives for conflict-freedom.
    ///
    /// Checks the named perspectives against each other and against
    /// active bidding groups. With `no_live = true`, active groups
    /// are ignored.
    fn validate_perspectives(
        &self, perspectives: Vec<PerspectiveName>, no_live: bool,
    ) -> Result<PerspectiveValidation>;

    /// List active workers.
    fn list_workers(&self) -> Result<Vec<WorkerSummary>>;

    /// Load one worker detail view.
    fn get_worker(&self, worker_id: WorkerId) -> Result<WorkerDetail>;

    /// Read one worker outbox after the provided sequence number.
    fn read_outbox(
        &self, worker_id: WorkerId, after: Option<Sequence>,
    ) -> Result<Vec<MailboxMessageView>>;

    /// Acknowledge one consumed worker outbox bundle.
    fn ack_outbox(&self, worker_id: WorkerId, sequence: Sequence) -> Result<AckRef>;

    /// Create a worker workspace and optional initial task bundle.
    fn create_worker(&self, request: CreateWorker) -> Result<CreateResult>;

    /// Move one blocked bidding group to HEAD.
    fn forward_perspective(&self, perspective: PerspectiveName)
    -> Result<PerspectiveForwardResult>;

    /// Publish a `resolve` bundle to the worker inbox.
    fn resolve_worker(
        &self, worker_id: WorkerId, reply: ReplyReference, payload: BundlePayload,
    ) -> Result<PublishedBundle>;

    /// Publish an advisory `hint` bundle to an active worker inbox.
    ///
    /// Hints carry new context without forcing a lifecycle transition.
    /// Note: If the orchestrator wants the worker to stop gracefully, it
    /// should send a hint asking the worker to publish a blocker report.
    fn hint_worker(
        &self, worker_id: WorkerId, reply: ReplyReference, payload: BundlePayload,
    ) -> Result<PublishedBundle>;

    /// Publish a `revise` bundle to the worker inbox.
    fn revise_worker(
        &self, worker_id: WorkerId, reply: ReplyReference, payload: BundlePayload,
    ) -> Result<PublishedBundle>;

    /// Finalize a worker without integration while preserving its workspace.
    fn discard_worker(&self, worker_id: WorkerId) -> Result<DiscardResult>;

    /// Delete one finalized worker workspace.
    fn delete_worker(&self, worker_id: WorkerId) -> Result<DeleteResult>;

    /// Run the pre-merge pipeline and merge the worker submission.
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

/// Forwarding checkpoint for one blocked worker.
#[derive(Debug, Clone)]
struct ForwardWorker {
    worker: WorkerEntry,
    reported_head_commit: CanonicalCommitHash,
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
    pub fn from_current_dir() -> Result<Self> {
        let project = CurrentProject::from_current_dir()?;
        Self::new(project.orchestrator_workspace_root()?.to_path_buf())
    }

    fn allocate_worker_id(
        &self, perspective: &PerspectiveName, state: &StateFile,
    ) -> Result<WorkerId> {
        let prefix = format!("{}-", camel_to_kebab(perspective.as_str()));
        let next = state
            .groups
            .iter()
            .flat_map(|g| g.workers.iter())
            .filter(|w| {
                state
                    .groups
                    .iter()
                    .any(|g| g.perspective == *perspective && g.find_worker(&w.worker_id).is_some())
            })
            .filter_map(|w| w.worker_id.as_str().strip_prefix(&prefix)?.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            + 1;

        WorkerId::new(format!("{prefix}{next}"))
            .map_err(|_| RuntimeError::CheckFailed("failed to allocate worker id".to_owned()))
    }

    fn resolve_create_worker_id(
        &self, perspective: &PerspectiveName, worker_id: Option<WorkerId>, state: &StateFile,
    ) -> Result<(WorkerId, Option<WorkerEntry>)> {
        if let Some(worker_id) = worker_id {
            for group in &state.groups {
                if let Some(entry) = group.find_worker(&worker_id) {
                    if entry.state.is_live() {
                        return Err(RuntimeError::WorkerExists(worker_id));
                    }
                    return Ok((worker_id, Some(entry.clone())));
                }
            }
            return Ok((worker_id, None));
        }

        Ok((self.allocate_worker_id(perspective, state)?, None))
    }

    fn validate_create_boundary(
        &self, perspective: &PerspectiveName, candidate: &CompiledPerspective, state: &StateFile,
    ) -> Result<()> {
        for group in state.live_groups() {
            if group.perspective == *perspective {
                // Joining existing group — boundary is inherited, no conflict check.
                continue;
            }
            let active_boundary = CompiledPerspective::from_materialized_sets(
                group.read_set.clone(),
                group.write_set.clone(),
            );
            if let Some(conflict) =
                boundary_conflict(perspective, candidate, &group.perspective, &active_boundary)
            {
                return Err(conflict);
            }
        }

        Ok(())
    }

    fn cleanup_workspace_before_create(&self, worker: &WorkerEntry) -> Result<()> {
        self.fs.vcs().remove_worktree(self.fs.workspace_root(), &worker.worktree_path)?;
        Ok(())
    }

    fn publish_worker_inbox(
        &self, worker_id: &WorkerId, kind: MessageKind, reply: ReplyReference,
        payload: BundlePayload,
    ) -> Result<PublishedBundle> {
        let state = self.fs.load_state()?;
        let (group, worker) = state
            .find_worker(worker_id)
            .ok_or_else(|| RuntimeError::UnknownWorker(worker_id.to_string()))?;
        let Some(expected_state) = kind.required_worker_state_for_inbox_publication() else {
            unreachable!(
                "worker inbox publication only accepts orchestrator-authored follow-up bundles"
            );
        };
        if worker.state == expected_state {
            return self.fs.publish_bundle(
                &worker.worktree_path,
                MailboxDirection::Inbox,
                kind,
                &worker.worker_id,
                &group.perspective,
                reply,
                None,
                payload,
            );
        }

        Err(RuntimeError::InvalidState {
            operation: inbox_publish_operation(kind),
            expected: expected_state.as_str(),
            actual: worker.state,
        })
    }

    fn load_forward_workers(
        &self, perspective: &PerspectiveName, state: &StateFile,
    ) -> Result<Vec<ForwardWorker>> {
        let group = state.find_live_group(perspective).ok_or_else(|| {
            RuntimeError::PerspectiveForwardMissingGroup { perspective: perspective.clone() }
        })?;

        let live_workers: Vec<&WorkerEntry> =
            group.workers.iter().filter(|w| w.state.is_live()).collect();

        let mut non_blocked: Vec<(WorkerId, WorkerState)> = live_workers
            .iter()
            .filter(|w| w.state != WorkerState::Blocked)
            .map(|w| (w.worker_id.clone(), w.state))
            .collect();
        if !non_blocked.is_empty() {
            non_blocked.sort_by(|left, right| left.0.cmp(&right.0));
            return Err(RuntimeError::PerspectiveForwardRequiresBlocked {
                perspective: perspective.clone(),
                workers: non_blocked,
            });
        }

        let mut prepared = Vec::new();
        for worker in &live_workers {
            let messages = self.fs.list_mailbox_messages(
                &worker.worktree_path,
                &worker.worker_id,
                MailboxDirection::Outbox,
                None,
            )?;
            let report = messages
                .into_iter()
                .rev()
                .find(|message| message.kind == MessageKind::Report)
                .ok_or_else(|| RuntimeError::PerspectiveForwardMissingReport {
                    worker_id: worker.worker_id.clone(),
                    perspective: perspective.clone(),
                })?;
            let reported_head_commit = report.head_commit.ok_or_else(|| {
                RuntimeError::PerspectiveForwardMissingReportedHead {
                    worker_id: worker.worker_id.clone(),
                    perspective: perspective.clone(),
                }
            })?;

            self.fs.vcs().ensure_clean_worktree(&worker.worktree_path)?;
            let current_head_commit = self.fs.vcs().head_commit(&worker.worktree_path)?;
            if current_head_commit != reported_head_commit {
                return Err(RuntimeError::PerspectiveForwardHeadMismatch {
                    worker_id: worker.worker_id.clone(),
                    perspective: perspective.clone(),
                    reported_head_commit,
                    current_head_commit,
                });
            }

            prepared.push(ForwardWorker {
                worker: (*worker).clone(),
                reported_head_commit: current_head_commit,
            });
        }
        prepared.sort_by(|left, right| left.worker.worker_id.cmp(&right.worker.worker_id));
        Ok(prepared)
    }

    fn rollback_forward_worktrees(
        &self, workers: &[ForwardWorker], forwarded_worker_ids: &BTreeSet<WorkerId>,
    ) {
        for worker in workers.iter().filter(|w| forwarded_worker_ids.contains(&w.worker.worker_id))
        {
            if let Err(error) = self
                .fs
                .vcs()
                .checkout_detached(&worker.worker.worktree_path, &worker.reported_head_commit)
            {
                tracing::error!(
                    worker_id = %worker.worker.worker_id,
                    error = %error,
                    "failed to roll back forwarded worker worktree"
                );
            }
        }
    }
}

impl OrchestratorService for FsOrchestratorService {
    fn rulebook_init(&self) -> Result<RulebookInit> {
        self.fs.initialize_rulebook()
    }

    fn list_perspectives(&self) -> Result<Vec<PerspectiveSummary>> {
        let compiled = self.fs.load_working_tree_rulebook()?;
        Ok(compiled.perspective_summaries())
    }

    fn validate_perspectives(
        &self, perspectives: Vec<PerspectiveName>, no_live: bool,
    ) -> Result<PerspectiveValidation> {
        let compiled = self.fs.load_working_tree_rulebook()?;
        let mut summaries = Vec::new();
        let mut named = Vec::new();

        for name in &perspectives {
            let perspective = compiled
                .perspectives()
                .get(name)
                .ok_or_else(|| RuntimeError::UnknownPerspective(name.to_string()))?;
            summaries.push(PerspectiveSummary {
                name: name.clone(),
                read_count: perspective.read().len(),
                write_count: perspective.write().len(),
            });
            named.push((name.clone(), perspective.clone()));
        }

        let mut conflicts = Vec::new();

        // Check named perspectives against each other.
        for i in 0..named.len() {
            for j in (i + 1)..named.len() {
                if let Some(conflict) =
                    boundary_conflict_info(&named[i].0, &named[i].1, &named[j].0, &named[j].1)
                {
                    conflicts.push(conflict);
                }
            }
        }

        // Check against active bidding groups unless --no-live.
        if !no_live {
            let state = self.fs.load_state()?;
            for (name, perspective) in &named {
                for group in state.live_groups() {
                    if group.perspective == *name {
                        continue;
                    }
                    let active_boundary = CompiledPerspective::from_materialized_sets(
                        group.read_set.clone(),
                        group.write_set.clone(),
                    );
                    if let Some(conflict) = boundary_conflict_info(
                        name,
                        perspective,
                        &group.perspective,
                        &active_boundary,
                    ) {
                        conflicts.push(conflict);
                    }
                }
            }
        }

        Ok(PerspectiveValidation { ok: conflicts.is_empty(), perspectives: summaries, conflicts })
    }

    fn list_workers(&self) -> Result<Vec<WorkerSummary>> {
        let state = self.fs.load_state()?;
        let mut workers: Vec<WorkerSummary> = state
            .groups
            .iter()
            .flat_map(|group| {
                group.workers.iter().filter(|w| w.state.is_live()).map(|w| WorkerSummary {
                    worker_id: w.worker_id.clone(),
                    perspective: group.perspective.clone(),
                    state: w.state,
                })
            })
            .collect();
        workers.sort_by(|left, right| left.worker_id.cmp(&right.worker_id));
        Ok(workers)
    }

    fn get_worker(&self, worker_id: WorkerId) -> Result<WorkerDetail> {
        let state = self.fs.load_state()?;
        let (group, worker) = state
            .find_worker(&worker_id)
            .ok_or_else(|| RuntimeError::UnknownWorker(worker_id.to_string()))?;
        Ok(WorkerDetail {
            worker_id: worker.worker_id.clone(),
            perspective: group.perspective.clone(),
            state: worker.state,
            worktree_path: worker.worktree_path.clone(),
            base_commit: group.base_commit.clone(),
            submitted_head_commit: worker.submitted_head_commit.clone(),
        })
    }

    fn read_outbox(
        &self, worker_id: WorkerId, after: Option<Sequence>,
    ) -> Result<Vec<MailboxMessageView>> {
        let state = self.fs.load_state()?;
        let (_, worker) = state
            .find_worker(&worker_id)
            .ok_or_else(|| RuntimeError::UnknownWorker(worker_id.to_string()))?;
        self.fs.list_mailbox_messages(
            &worker.worktree_path,
            &worker.worker_id,
            MailboxDirection::Outbox,
            after,
        )
    }

    fn ack_outbox(&self, worker_id: WorkerId, sequence: Sequence) -> Result<AckRef> {
        tracing::trace!(worker_id = %worker_id, sequence = sequence.0, "acknowledging worker outbox message");
        let state = self.fs.load_state()?;
        let (_, worker) = state
            .find_worker(&worker_id)
            .ok_or_else(|| RuntimeError::UnknownWorker(worker_id.to_string()))?;
        let ack = self.fs.acknowledge_message(
            &worker.worktree_path,
            MailboxDirection::Outbox,
            sequence,
        )?;
        tracing::info!(worker_id = %worker_id, sequence = sequence.0, "acknowledged worker outbox message");
        Ok(ack)
    }

    fn create_worker(&self, request: CreateWorker) -> Result<CreateResult> {
        let CreateWorker { perspective, worker_id, task, overwriting_worktree } = request;
        let mut state = self.fs.load_state()?;
        let compiled = self.fs.load_working_tree_rulebook()?;
        let compiled_perspective = compiled
            .perspectives()
            .get(&perspective)
            .ok_or_else(|| RuntimeError::UnknownPerspective(perspective.to_string()))?
            .clone();

        // Check if there's an existing live bidding group for this perspective.
        let existing_group = state.find_live_group(&perspective);

        let (base_commit, read_set, write_set) = if let Some(group) = existing_group {
            // Join existing group — use group's boundary.
            (group.base_commit.clone(), group.read_set.clone(), group.write_set.clone())
        } else {
            // Form new group — compile from working tree, pin base to HEAD.
            self.validate_create_boundary(&perspective, &compiled_perspective, &state)?;

            let base_commit = self.fs.vcs().head_commit(self.fs.workspace_root())?;
            (base_commit, compiled_perspective.read().clone(), compiled_perspective.write().clone())
        };

        let (worker_id, previous_finalized) =
            self.resolve_create_worker_id(&perspective, worker_id, &state)?;
        if let Some(entry) = previous_finalized.as_ref() {
            if entry.worktree_path.exists() {
                if !overwriting_worktree {
                    return Err(RuntimeError::ExistingWorkerWorkspace {
                        worker_id: entry.worker_id.clone(),
                        state: entry.state,
                        worktree_path: entry.worktree_path.clone(),
                    });
                }
                self.cleanup_workspace_before_create(entry)?;
            }
            let removed = state.remove_worker(&entry.worker_id);
            debug_assert!(removed, "resolved finalized worker id must still exist in state");
        }

        let worktree_path = self.fs.worker_paths(&worker_id).worktree_root().to_path_buf();
        self.fs.vcs().create_worktree(self.fs.workspace_root(), &worktree_path, &base_commit)?;

        let new_worker = WorkerEntry {
            worker_id: worker_id.clone(),
            state: WorkerState::Active,
            worktree_path: worktree_path.clone(),
            submitted_head_commit: None,
        };

        // Find or create the group in state, then add the worker.
        let group_idx = if let Some(idx) =
            state.groups.iter().position(|g| g.perspective == perspective && g.has_live_workers())
        {
            state.groups[idx].workers.push(new_worker.clone());
            idx
        } else {
            let group = BiddingGroupRecord {
                perspective: perspective.clone(),
                base_commit: base_commit.clone(),
                read_set: read_set.clone(),
                write_set: write_set.clone(),
                workers: vec![new_worker.clone()],
            };
            state.groups.push(group);
            state.groups.len() - 1
        };

        self.fs.prepare_worker_runtime(&new_worker, &state.groups[group_idx])?;
        self.fs.store_state(&state)?;

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

        self.fs.rewrite_exclusion_set(&state)?;

        tracing::info!(
            worker_id = %worker_id,
            perspective = %perspective,
            root = %worktree_path.display(),
            "created active worker"
        );

        Ok(CreateResult {
            worker_id,
            perspective,
            worktree_path,
            state: WorkerState::Active,
            seeded_task_path,
        })
    }

    fn forward_perspective(
        &self, perspective: PerspectiveName,
    ) -> Result<PerspectiveForwardResult> {
        let state = self.fs.load_state()?;
        let workers = self.load_forward_workers(&perspective, &state)?;
        let group = state.find_live_group(&perspective).ok_or_else(|| {
            RuntimeError::PerspectiveForwardMissingGroup { perspective: perspective.clone() }
        })?;
        let previous_base_commit = group.base_commit.clone();
        let worker_ids: Vec<WorkerId> =
            workers.iter().map(|w| w.worker.worker_id.clone()).collect();

        let new_base_commit = self.fs.vcs().head_commit(self.fs.workspace_root())?;

        if previous_base_commit == new_base_commit {
            return Ok(PerspectiveForwardResult {
                perspective,
                worker_ids,
                previous_base_commit,
                new_base_commit,
            });
        }

        // Recompile the perspective from the current rulebook.
        let compiled = self.fs.load_working_tree_rulebook()?;
        let target = compiled
            .perspectives()
            .get(&perspective)
            .ok_or_else(|| RuntimeError::UnknownPerspective(perspective.to_string()))?
            .clone();

        // Continuity check: new boundary must be a superset of the old one.
        if !target.write().is_superset(&group.write_set)
            || !target.read().is_superset(&group.read_set)
        {
            return Err(RuntimeError::BiddingGroupBoundaryMismatch {
                perspective: perspective.clone(),
            });
        }

        // Forward each worker's worktree.
        let mut forwarded_worker_ids = BTreeSet::new();
        for fw in &workers {
            if let Err(error) = self.fs.vcs().forward_worktree(
                &fw.worker.worktree_path,
                &previous_base_commit,
                &new_base_commit,
            ) {
                self.rollback_forward_worktrees(&workers, &forwarded_worker_ids);
                return Err(RuntimeError::from(error));
            }
            forwarded_worker_ids.insert(fw.worker.worker_id.clone());
        }

        // Update state.
        let mut updated_state = state.clone();
        let group_mut = updated_state.find_live_group_mut(&perspective).ok_or_else(|| {
            RuntimeError::PerspectiveForwardMissingGroup { perspective: perspective.clone() }
        })?;
        group_mut.base_commit = new_base_commit.clone();
        group_mut.read_set = target.read().clone();
        group_mut.write_set = target.write().clone();

        // Refresh worker contracts and boundaries.
        for fw in &workers {
            let worker_entry = group_mut.find_worker(&fw.worker.worker_id).unwrap().clone();
            if let Err(error) = self.fs.refresh_worker_contract(&worker_entry, group_mut) {
                self.rollback_forward_worktrees(&workers, &forwarded_worker_ids);
                return Err(error);
            }
            if let Err(error) = self.fs.refresh_worker_boundary(&worker_entry, group_mut) {
                self.rollback_forward_worktrees(&workers, &forwarded_worker_ids);
                return Err(error);
            }
        }

        self.fs.store_state(&updated_state)?;
        self.fs.rewrite_exclusion_set(&updated_state)?;

        tracing::info!(
            perspective = %perspective,
            worker_count = worker_ids.len(),
            previous_base_commit = %previous_base_commit,
            new_base_commit = %new_base_commit,
            "forwarded blocked bidding group to HEAD"
        );

        Ok(PerspectiveForwardResult {
            perspective,
            worker_ids,
            previous_base_commit,
            new_base_commit,
        })
    }

    fn resolve_worker(
        &self, worker_id: WorkerId, reply: ReplyReference, payload: BundlePayload,
    ) -> Result<PublishedBundle> {
        tracing::trace!(worker_id = %worker_id, "publishing resolve bundle to worker inbox");
        self.publish_worker_inbox(&worker_id, MessageKind::Resolve, reply, payload)
    }

    fn hint_worker(
        &self, worker_id: WorkerId, reply: ReplyReference, payload: BundlePayload,
    ) -> Result<PublishedBundle> {
        tracing::trace!(worker_id = %worker_id, "publishing hint bundle to worker inbox");
        self.publish_worker_inbox(&worker_id, MessageKind::Hint, reply, payload)
    }

    fn revise_worker(
        &self, worker_id: WorkerId, reply: ReplyReference, payload: BundlePayload,
    ) -> Result<PublishedBundle> {
        tracing::trace!(worker_id = %worker_id, "publishing revise bundle to worker inbox");
        self.publish_worker_inbox(&worker_id, MessageKind::Revise, reply, payload)
    }

    fn discard_worker(&self, worker_id: WorkerId) -> Result<DiscardResult> {
        let mut state = self.fs.load_state()?;
        let group = state
            .find_worker_group_mut(&worker_id)
            .ok_or_else(|| RuntimeError::UnknownWorker(worker_id.to_string()))?;
        let perspective = group.perspective.clone();
        {
            let worker = group
                .find_worker_mut(&worker_id)
                .ok_or_else(|| RuntimeError::UnknownWorker(worker_id.to_string()))?;

            if !matches!(
                worker.state,
                WorkerState::Active | WorkerState::Blocked | WorkerState::Committed
            ) {
                return Err(RuntimeError::InvalidState {
                    operation: "discard worker",
                    expected: "ACTIVE, BLOCKED, or COMMITTED",
                    actual: worker.state,
                });
            }

            worker.state = WorkerState::Discarded;
            worker.submitted_head_commit = None;
        }

        if !group.has_live_workers() {
            group.clear_boundary();
        }

        self.fs.store_state(&state)?;
        self.fs.rewrite_exclusion_set(&state)?;

        tracing::info!(worker_id = %worker_id, perspective = %perspective, "discarded worker");
        Ok(DiscardResult { worker_id, perspective, state: WorkerState::Discarded })
    }

    fn delete_worker(&self, worker_id: WorkerId) -> Result<DeleteResult> {
        let mut state = self.fs.load_state()?;
        let (group, worker) = state
            .find_worker(&worker_id)
            .ok_or_else(|| RuntimeError::UnknownWorker(worker_id.to_string()))?;

        if !matches!(worker.state, WorkerState::Merged | WorkerState::Discarded) {
            return Err(RuntimeError::InvalidState {
                operation: "delete worker workspace",
                expected: "MERGED or DISCARDED",
                actual: worker.state,
            });
        }

        let perspective = group.perspective.clone();
        let worker_state = worker.state;
        let worktree_path = worker.worktree_path.clone();

        let deleted_workspace =
            self.fs.vcs().remove_worktree(self.fs.workspace_root(), &worktree_path)?;

        // Remove the worker entry from its group.
        let group_mut = state.find_worker_group_mut(&worker_id).unwrap();
        group_mut.workers.retain(|w| w.worker_id != worker_id);

        // Remove empty groups.
        state.gc_empty_groups();
        self.fs.store_state(&state)?;
        self.fs.rewrite_exclusion_set(&state)?;

        tracing::info!(
            worker_id = %worker_id,
            perspective = %perspective,
            deleted_workspace,
            "deleted worker workspace"
        );
        Ok(DeleteResult {
            worker_id,
            perspective,
            state: worker_state,
            worktree_path,
            deleted_workspace,
        })
    }

    fn merge_worker(
        &self, worker_id: WorkerId, skip_checks: Vec<String>, audit_payload: BundlePayload,
    ) -> Result<MergeResult> {
        tracing::trace!(worker_id = %worker_id, "starting worker merge");
        let state = self.fs.load_state()?;

        let (group, worker) = state
            .find_worker(&worker_id)
            .ok_or_else(|| RuntimeError::UnknownWorker(worker_id.to_string()))?;

        if worker.state != WorkerState::Committed {
            return Err(RuntimeError::InvalidState {
                operation: "merge worker",
                expected: "COMMITTED",
                actual: worker.state,
            });
        }

        let head_commit = worker.submitted_head_commit.clone().ok_or_else(|| {
            RuntimeError::MissingSubmittedHeadCommit {
                worker_id: worker_id.clone(),
                state: worker.state,
            }
        })?;
        let head_commit = self.fs.vcs().resolve_commit(
            &worker.worktree_path,
            head_commit.as_str(),
            "verify submitted worker commit",
        )?;

        let base_commit = group.base_commit.clone();
        let perspective = group.perspective.clone();
        let worktree_path = worker.worktree_path.clone();
        let write_set = group.write_set.clone();

        if head_commit == base_commit {
            return Err(RuntimeError::NoNewCommit {
                worker_id: worker_id.clone(),
                head_commit: head_commit.clone(),
            });
        }
        let worker_head = self.fs.vcs().head_commit(&worktree_path)?;
        if worker_head != head_commit {
            return Err(RuntimeError::WorkerHeadMismatch {
                worker_id: worker_id.clone(),
                submitted_head_commit: head_commit,
                current_head_commit: worker_head,
            });
        }

        tracing::trace!(worker_id = %worker_id, head_commit = %head_commit, "verified submitted commit");

        let worker_rulebook = self.fs.load_compiled_rulebook(&base_commit)?;
        let allowed_skips = validate_skip_request(&worker_rulebook, &skip_checks)?;
        let changed_files =
            self.fs.vcs().changed_files(&worktree_path, &base_commit, &head_commit)?;
        let violations = changed_files.difference(&write_set).cloned().collect::<BTreeSet<_>>();
        if !violations.is_empty() {
            tracing::warn!(worker_id = %worker_id, count = violations.len(), "write-set violation");
            return Err(RuntimeError::WriteSetViolation {
                worker_id: worker_id.clone(),
                perspective: perspective.clone(),
                base_commit: base_commit.clone(),
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
            self.fs.run_check(&worktree_path, check_name, decl.command())?;
            ran_checks.push(check_name.to_string());
        }

        // Prepare the post-merge runtime projection before touching the
        // canonical branch so deterministic audit/state failures happen
        // while the worker is still `COMMITTED`.
        let worker_entry_for_audit = worker.clone();
        let group_for_audit = group.clone();
        let mut updated_state = state.clone();
        let group_mut = updated_state.find_worker_group_mut(&worker_id).unwrap();
        for entry in &mut group_mut.workers {
            if entry.worker_id == worker_id {
                entry.state = WorkerState::Merged;
            } else if entry.state.is_live() {
                entry.state = WorkerState::Discarded;
                entry.submitted_head_commit = None;
            }
        }
        group_mut.clear_boundary();

        let mut staged_merge = self.fs.prepare_merge_artifacts(
            &updated_state,
            &worker_entry_for_audit,
            &group_for_audit,
            &head_commit,
            &changed_files,
            &ran_checks,
            &skipped_checks,
            audit_payload,
        )?;

        self.fs.vcs().ensure_clean_workspace(self.fs.workspace_root())?;
        let empty_submission = changed_files.is_empty();
        if !empty_submission {
            self.fs.vcs().begin_integration(self.fs.workspace_root(), &head_commit)?;
        }
        if let Err(error) = staged_merge.promote() {
            let abort_error = if empty_submission {
                None
            } else {
                self.fs.vcs().abort_integration(self.fs.workspace_root()).err()
            };
            staged_merge.cleanup();
            return Err(abort_error.map(RuntimeError::from).unwrap_or(error));
        }
        if let Err(error) =
            self.fs.vcs().finalize_integration(self.fs.workspace_root(), &head_commit)
        {
            let rollback_error = staged_merge.rollback().err();
            let abort_error = if empty_submission {
                None
            } else {
                self.fs.vcs().abort_integration(self.fs.workspace_root()).err()
            };
            staged_merge.cleanup();
            if let Some(abort_error) = abort_error {
                tracing::error!(
                    worker_id = %worker_id,
                    original_error = %error,
                    "failed to abort canonical integration after finalize failure"
                );
                return Err(RuntimeError::from(abort_error));
            }
            if let Some(rollback_error) = rollback_error {
                tracing::error!(
                    worker_id = %worker_id,
                    original_error = %error,
                    "failed to roll back staged runtime artifacts after finalize failure"
                );
                return Err(rollback_error);
            }
            return Err(RuntimeError::from(error));
        }
        staged_merge.cleanup();

        tracing::info!(
            worker_id = %worker_id,
            perspective = %perspective,
            head_commit = %head_commit,
            "merged worker"
        );
        Ok(MergeResult {
            worker_id,
            perspective,
            state: WorkerState::Merged,
            ran_checks,
            skipped_checks,
        })
    }

    fn status(&self) -> Result<OrchestratorStatus> {
        let state = self.fs.load_state()?;
        let mut active_perspectives: Vec<ActivePerspectiveSummary> = state
            .live_groups()
            .map(|group| ActivePerspectiveSummary {
                perspective: group.perspective.clone(),
                worker_ids: group
                    .workers
                    .iter()
                    .filter(|w| w.state.is_live())
                    .map(|w| w.worker_id.clone())
                    .collect(),
                read_count: group.read_set.len(),
                write_count: group.write_set.len(),
            })
            .collect();
        active_perspectives.sort_by(|left, right| left.perspective.cmp(&right.perspective));
        let workers = self.list_workers()?;

        Ok(OrchestratorStatus { active_perspectives, workers })
    }
}

fn boundary_conflict(
    candidate_name: &PerspectiveName, candidate: &CompiledPerspective,
    active_name: &PerspectiveName, active: &CompiledPerspective,
) -> Option<RuntimeError> {
    BoundaryOverlap::detect(candidate, active).map(|overlap| {
        RuntimeError::ConflictWithActiveBiddingGroup {
            perspective: candidate_name.clone(),
            blocking_perspective: active_name.clone(),
            relation: overlap.relation.runtime_relation(),
            files: overlap.files,
        }
    })
}

/// Return a `PerspectiveConflict` if two boundaries overlap.
fn boundary_conflict_info(
    a_name: &PerspectiveName, a: &CompiledPerspective, b_name: &PerspectiveName,
    b: &CompiledPerspective,
) -> Option<PerspectiveConflict> {
    BoundaryOverlap::detect(a, b).map(|overlap| PerspectiveConflict {
        perspective: a_name.clone(),
        blocking_perspective: b_name.clone(),
        relation: overlap.relation.validation_relation(),
        files: overlap.files,
    })
}

/// Stable operation label for orchestrator-authored inbox bundles.
fn inbox_publish_operation(kind: MessageKind) -> &'static str {
    match kind {
        | MessageKind::Hint => "publish hint bundle",
        | MessageKind::Resolve => "publish resolve bundle",
        | MessageKind::Revise => "publish revise bundle",
        | MessageKind::Task | MessageKind::Report | MessageKind::Commit => "publish inbox bundle",
    }
}

/// One concrete overlap that violates the bidding-group invariant.
///
/// Note: Worker creation and perspective validation both stop at the
/// first detected overlap because any single overlap is already enough
/// to reject the candidate concurrency shape.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BoundaryOverlap {
    files: Vec<PathBuf>,
    relation: BoundaryOverlapRelation,
}

impl BoundaryOverlap {
    /// Detect the first overlap between two compiled boundaries.
    fn detect(left: &CompiledPerspective, right: &CompiledPerspective) -> Option<Self> {
        Self::from_sets(
            BoundaryOverlapRelation::WriteWrite,
            left.write().intersection(right.write()).cloned().collect(),
        )
        .or_else(|| {
            Self::from_sets(
                BoundaryOverlapRelation::LeftWriteRightRead,
                left.write().intersection(right.read()).cloned().collect(),
            )
        })
        .or_else(|| {
            Self::from_sets(
                BoundaryOverlapRelation::LeftReadRightWrite,
                left.read().intersection(right.write()).cloned().collect(),
            )
        })
    }

    /// Materialize one overlap from the matching file set.
    fn from_sets(relation: BoundaryOverlapRelation, files: BTreeSet<PathBuf>) -> Option<Self> {
        if files.is_empty() {
            return None;
        }
        Some(Self { relation, files: files.into_iter().collect() })
    }
}

/// Direction of one boundary overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundaryOverlapRelation {
    /// Both perspectives attempt to write the same files.
    WriteWrite,
    /// The left perspective writes files the right perspective reads.
    LeftWriteRightRead,
    /// The left perspective reads files the right perspective writes.
    LeftReadRightWrite,
}

impl BoundaryOverlapRelation {
    /// Relation string used by runtime create-worker errors.
    const fn runtime_relation(self) -> &'static str {
        match self {
            | Self::WriteWrite => "write/write overlap",
            | Self::LeftWriteRightRead => "candidate write overlaps active read",
            | Self::LeftReadRightWrite => "candidate read overlaps active write",
        }
    }

    /// Relation string used by perspective validation output.
    const fn validation_relation(self) -> &'static str {
        match self {
            | Self::WriteWrite => "write/write overlap",
            | Self::LeftWriteRightRead => "write overlaps read",
            | Self::LeftReadRightWrite => "read overlaps write",
        }
    }
}

/// Convert an `UpperCamelCase` name to `kebab-case`.
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
    use std::path::{Path, PathBuf};
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
        fs::write(dir.path().join("src/owned.rs"), "pub fn owned() -> i32 { 1 }\n").unwrap();
        fs::write(dir.path().join("src/other.rs"), "pub fn other() -> i32 { 2 }\n").unwrap();
        FsOrchestratorService::new(dir.path()).unwrap().rulebook_init().unwrap();
        fs::write(dir.path().join(".multorum/rulebook.toml"), initial_rulebook()).unwrap();

        git(dir.path(), &["init"]);
        git(dir.path(), &["config", "user.name", "Multorum Test"]);
        git(dir.path(), &["config", "user.email", "multorum@test.invalid"]);
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "feat: initialize runtime fixture"]);

        let orchestrator = FsOrchestratorService::new(dir.path()).unwrap();
        (dir, orchestrator)
    }

    fn expand_rulebook(dir: &TempDir) {
        fs::write(dir.path().join("src/new.rs"), "pub fn new_owned() -> i32 { 3 }\n").unwrap();
        fs::write(dir.path().join(".multorum/rulebook.toml"), expanded_rulebook()).unwrap();
        git(dir.path(), &["add", "src/new.rs", ".multorum/rulebook.toml"]);
        git(dir.path(), &["commit", "-m", "incr: expand perspective write set"]);
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

        expand_rulebook(&dir);
        let result = orchestrator.forward_perspective(perspective()).unwrap();

        assert_eq!(result.worker_ids, vec![worker.worker_id.clone()]);
        assert_ne!(result.previous_base_commit, result.new_base_commit);

        let worker_detail = orchestrator.get_worker(worker.worker_id.clone()).unwrap();
        assert_eq!(worker_detail.base_commit, result.new_base_commit);
        assert_eq!(worker_detail.state, WorkerState::Blocked);

        let contract: WorkerContractView = worker_service.contract().unwrap();
        assert_eq!(contract.base_commit, result.new_base_commit);

        let forwarded_head = git(&worker.worktree_path, &["rev-parse", "HEAD"]);
        assert_ne!(forwarded_head, reported_head);

        let write_set =
            fs::read_to_string(worker.worktree_path.join(".multorum/write-set.txt")).unwrap();
        assert!(write_set.lines().any(|line| line == "src/new.rs"));
    }

    #[test]
    fn forward_perspective_rejects_active_workers() {
        let (dir, orchestrator) = setup_repo();
        let worker = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
        expand_rulebook(&dir);

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
        expand_rulebook(&dir);

        let error = orchestrator.forward_perspective(perspective()).unwrap_err();
        assert!(matches!(
            error,
            RuntimeError::PerspectiveForwardMissingReportedHead { worker_id, .. }
                if worker_id == worker.worker_id
        ));
    }

    #[test]
    fn create_worker_joins_existing_bidding_group() {
        let (_dir, orchestrator) = setup_repo();
        let first = orchestrator.create_worker(CreateWorker::new(perspective())).unwrap();
        let second = orchestrator
            .create_worker(
                CreateWorker::new(perspective())
                    .with_worker_id(WorkerId::new("second-worker").unwrap()),
            )
            .unwrap();

        let first_detail = orchestrator.get_worker(first.worker_id).unwrap();
        let second_detail = orchestrator.get_worker(second.worker_id).unwrap();
        assert_eq!(first_detail.base_commit, second_detail.base_commit);
    }

    fn compiled_perspective(read: &[&str], write: &[&str]) -> CompiledPerspective {
        CompiledPerspective::from_materialized_sets(
            read.iter().map(PathBuf::from).collect(),
            write.iter().map(PathBuf::from).collect(),
        )
    }

    #[test]
    fn boundary_overlap_detects_runtime_relations_once() {
        let left = compiled_perspective(&["src/stable.rs"], &["src/shared.rs"]);
        let right = compiled_perspective(&["src/shared.rs"], &["src/other.rs"]);

        let overlap = BoundaryOverlap::detect(&left, &right).unwrap();
        assert_eq!(overlap.relation, BoundaryOverlapRelation::LeftWriteRightRead);
        assert_eq!(overlap.files, vec![PathBuf::from("src/shared.rs")]);
    }

    #[test]
    fn boundary_conflict_info_uses_shared_overlap_detector() {
        let left = compiled_perspective(&["src/stable.rs"], &["src/shared.rs"]);
        let right = compiled_perspective(&["src/shared.rs"], &["src/other.rs"]);

        let conflict = boundary_conflict_info(
            &PerspectiveName::new("Left").unwrap(),
            &left,
            &PerspectiveName::new("Right").unwrap(),
            &right,
        )
        .unwrap();
        assert_eq!(conflict.relation, "write overlaps read");
        assert_eq!(conflict.files, vec![PathBuf::from("src/shared.rs")]);
    }
}
