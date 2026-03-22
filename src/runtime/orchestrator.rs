//! Orchestrator-facing runtime service surface.
//!
//! This module defines the typed operations available to orchestrator
//! frontends and the default storage-backed implementation used by
//! the CLI.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use crate::perspective::{CompiledPerspective, PerspectiveName};
use crate::vcs::{CanonicalCommitHash, VersionControl};

use super::{
    MailboxDirection, MultorumPaths,
    bundle::{BundlePayload, MessageKind, PublishedBundle, ReplyReference},
    error::{Result, RuntimeError},
    state::{
        BiddingGroupSummary, DiscardResult, IntegrateResult, OrchestratorStatus,
        PerspectiveSummary, ProvisionResult, RulebookInit, RulebookSwitch, RulebookValidation,
        WorkerDetail, WorkerState, WorkerSummary,
    },
    storage::{
        ActiveRulebookRecord, RuntimeFs, WorkerRecord, is_live_worker_state, timestamp_now,
        validate_skip_request,
    },
    worker_id::WorkerId,
};

/// Typed operations available to the orchestrator frontend.
pub trait OrchestratorService {
    /// Initialize `.multorum/` with the default committed artifacts.
    fn rulebook_init(&self) -> Result<RulebookInit>;

    /// Dry-run validation of a rulebook switch.
    fn rulebook_validate(&self, commit: String) -> Result<RulebookValidation>;

    /// Activate a rulebook commit after validation succeeds.
    fn rulebook_switch(&self, commit: String) -> Result<RulebookSwitch>;

    /// List compiled perspective summaries from the active rulebook.
    fn list_perspectives(&self) -> Result<Vec<PerspectiveSummary>>;

    /// List active bidding groups.
    fn list_bidding_groups(&self) -> Result<Vec<BiddingGroupSummary>>;

    /// List active workers.
    fn list_workers(&self) -> Result<Vec<WorkerSummary>>;

    /// Load one worker detail view.
    fn get_worker(&self, worker_id: WorkerId) -> Result<WorkerDetail>;

    /// Provision a worker worktree and optional initial task bundle.
    ///
    /// Any path-backed payload files are moved into `.multorum/`
    /// storage if publication succeeds.
    fn provision_worker(
        &self, perspective: PerspectiveName, task: Option<BundlePayload>,
    ) -> Result<ProvisionResult>;

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

    /// Tear down a worker without integration.
    fn discard_worker(&self, worker_id: WorkerId) -> Result<DiscardResult>;

    /// Run the pre-merge pipeline and integrate the worker submission.
    fn integrate_worker(
        &self, worker_id: WorkerId, skip_checks: Vec<String>,
    ) -> Result<IntegrateResult>;

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
    bidding_group: PerspectiveName,
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
    /// If the current directory is a managed worker worktree, the
    /// canonical workspace above `.multorum/worktrees/` is used.
    pub fn from_current_dir() -> Result<Self> {
        let cwd = std::env::current_dir()?;
        let workspace_root = MultorumPaths::canonical_workspace_root(&cwd);
        Self::new(workspace_root)
    }

    fn validate_rulebook_commit(&self, commit: &CanonicalCommitHash) -> Result<RulebookValidation> {
        let compiled = self.fs.load_compiled_rulebook(commit)?;
        let active_groups = self.active_bidding_groups()?;

        let mut blocking_bidding_groups = BTreeSet::new();
        for active_group in &active_groups {
            for (candidate_name, candidate) in compiled.perspectives().perspectives() {
                if boundary_conflict(
                    &active_group.bidding_group,
                    &active_group.boundary,
                    candidate_name,
                    candidate,
                )
                .is_some()
                {
                    blocking_bidding_groups.insert(active_group.bidding_group.clone());
                    break;
                }
            }
        }

        Ok(RulebookValidation {
            ok: blocking_bidding_groups.is_empty(),
            blocking_bidding_groups: blocking_bidding_groups.into_iter().collect(),
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
            if !seen.insert(record.bidding_group.clone()) {
                continue;
            }

            let worker_paths = self.fs.worker_paths(&record.worker_id);
            let read = RuntimeFs::read_path_list(&worker_paths.read_set())?;
            let write = RuntimeFs::read_path_list(&worker_paths.write_set())?;
            groups.push(ActiveBiddingGroup {
                bidding_group: record.bidding_group.clone(),
                perspective: record.perspective.clone(),
                worker_ids: active_workers
                    .iter()
                    .filter(|worker| worker.bidding_group == record.bidding_group)
                    .map(|worker| worker.worker_id.clone())
                    .collect(),
                boundary: CompiledPerspective::from_materialized_sets(read, write),
            });
        }
        Ok(groups)
    }

    fn allocate_worker_id(&self, perspective: &PerspectiveName) -> Result<WorkerId> {
        let prefix = format!("{}-", perspective.as_str());
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

    fn validate_provision_boundary(
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
                &active_group.bidding_group,
                &active_group.boundary,
            ) {
                return Err(conflict);
            }
        }

        Ok(())
    }

    fn discard_worker_record(&self, record: &mut WorkerRecord) -> Result<()> {
        if record.worktree_path.exists() {
            self.fs.vcs().remove_worktree(self.fs.workspace_root(), &record.worktree_path)?;
        }
        record.state = WorkerState::Discarded;
        record.submitted_head_commit = None;
        self.fs.store_worker_record(record)
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
                &record.bidding_group,
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

    fn rulebook_validate(&self, commit: String) -> Result<RulebookValidation> {
        let commit = self.fs.vcs().resolve_commit(
            self.fs.workspace_root(),
            &commit,
            "resolve rulebook commit",
        )?;
        self.validate_rulebook_commit(&commit)
    }

    fn rulebook_switch(&self, commit: String) -> Result<RulebookSwitch> {
        let commit = self.fs.vcs().resolve_commit(
            self.fs.workspace_root(),
            &commit,
            "resolve rulebook commit",
        )?;
        let validation = self.validate_rulebook_commit(&commit)?;
        if !validation.ok {
            return Err(RuntimeError::RulebookConflict {
                commit,
                blocking_bidding_groups: validation.blocking_bidding_groups,
            });
        }

        let record = ActiveRulebookRecord {
            rulebook_commit: commit.clone(),
            base_commit: commit.clone(),
            activated_at: timestamp_now(),
        };
        self.fs.store_active_rulebook(&record)?;
        tracing::info!(rulebook_commit = %record.rulebook_commit, "activated rulebook");
        Ok(RulebookSwitch { active_commit: record.rulebook_commit })
    }

    fn list_perspectives(&self) -> Result<Vec<PerspectiveSummary>> {
        let (_, compiled) = self.fs.load_active_compiled_rulebook()?;
        Ok(compiled.perspective_summaries())
    }

    fn list_bidding_groups(&self) -> Result<Vec<BiddingGroupSummary>> {
        let mut groups = self
            .active_bidding_groups()?
            .into_iter()
            .map(|group| BiddingGroupSummary {
                bidding_group: group.bidding_group,
                perspective: group.perspective,
                worker_ids: group.worker_ids,
                read_count: group.boundary.read().len(),
                write_count: group.boundary.write().len(),
            })
            .collect::<Vec<_>>();
        groups.sort_by(|left, right| left.bidding_group.cmp(&right.bidding_group));
        Ok(groups)
    }

    fn list_workers(&self) -> Result<Vec<WorkerSummary>> {
        let mut workers = self
            .active_workers()?
            .into_iter()
            .map(|record| WorkerSummary {
                worker_id: record.worker_id,
                bidding_group: record.bidding_group,
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
            bidding_group: record.bidding_group,
            perspective: record.perspective,
            state: record.state,
            worktree_path: record.worktree_path,
            rulebook_commit: record.rulebook_commit,
            base_commit: record.base_commit,
            submitted_head_commit: record.submitted_head_commit,
        })
    }

    fn provision_worker(
        &self, perspective: PerspectiveName, task: Option<BundlePayload>,
    ) -> Result<ProvisionResult> {
        let (active, compiled) = self.fs.load_active_compiled_rulebook()?;
        let compiled_perspective = compiled
            .perspectives()
            .get(&perspective)
            .ok_or_else(|| RuntimeError::UnknownPerspective(perspective.to_string()))?;
        self.validate_provision_boundary(&perspective, compiled_perspective)?;

        let worker_id = self.allocate_worker_id(&perspective)?;
        let bidding_group = perspective.clone();
        let worktree_path = self.fs.worker_paths(&worker_id).worktree_root().to_path_buf();
        self.fs.vcs().create_worktree(
            self.fs.workspace_root(),
            &worktree_path,
            &active.base_commit,
        )?;

        let record = WorkerRecord {
            worker_id: worker_id.clone(),
            bidding_group: bidding_group.clone(),
            perspective: perspective.clone(),
            state: WorkerState::Active,
            worktree_path: worktree_path.clone(),
            rulebook_commit: active.rulebook_commit,
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
                        &bidding_group,
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

        tracing::info!(
            worker_id = %worker_id,
            perspective = %perspective,
            root = %worktree_path.display(),
            "provisioned active worker"
        );

        Ok(ProvisionResult {
            worker_id,
            bidding_group,
            perspective,
            worktree_path,
            state: WorkerState::Active,
            seeded_task_path,
        })
    }

    fn resolve_worker(
        &self, worker_id: WorkerId, reply: ReplyReference, payload: BundlePayload,
    ) -> Result<PublishedBundle> {
        self.publish_worker_inbox(&worker_id, MessageKind::Resolve, reply, payload)
    }

    fn revise_worker(
        &self, worker_id: WorkerId, reply: ReplyReference, payload: BundlePayload,
    ) -> Result<PublishedBundle> {
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

        self.discard_worker_record(&mut record)?;

        tracing::info!(worker_id = %record.worker_id, perspective = %record.perspective, "discarded worker");
        Ok(DiscardResult {
            worker_id: record.worker_id,
            bidding_group: record.bidding_group,
            perspective: record.perspective,
            state: record.state,
        })
    }

    fn integrate_worker(
        &self, worker_id: WorkerId, skip_checks: Vec<String>,
    ) -> Result<IntegrateResult> {
        let mut record = self.fs.load_worker_record(&worker_id)?;
        if record.state != WorkerState::Committed {
            return Err(RuntimeError::InvalidState {
                operation: "integrate worker",
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

        let worker_head = self.fs.vcs().head_commit(&record.worktree_path)?;
        if worker_head != head_commit {
            return Err(RuntimeError::WorkerHeadMismatch {
                worker_id: worker_id.clone(),
                submitted_head_commit: head_commit,
                current_head_commit: worker_head,
            });
        }

        let worker_rulebook = self.fs.load_compiled_rulebook(&record.rulebook_commit)?;
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
                continue;
            }

            self.fs.run_check(&record.worktree_path, check_name, decl.command())?;
            ran_checks.push(check_name.to_string());
        }

        self.fs.vcs().ensure_clean_workspace(self.fs.workspace_root())?;
        self.fs.vcs().integrate_commit(self.fs.workspace_root(), &head_commit)?;
        self.fs.vcs().remove_worktree(self.fs.workspace_root(), &record.worktree_path)?;

        record.state = WorkerState::Merged;
        self.fs.store_worker_record(&record)?;

        for mut sibling in self.active_workers()?.into_iter().filter(|sibling| {
            sibling.worker_id != record.worker_id && sibling.bidding_group == record.bidding_group
        }) {
            self.discard_worker_record(&mut sibling)?;
        }

        tracing::info!(
            worker_id = %record.worker_id,
            perspective = %record.perspective,
            head_commit = %head_commit,
            "merged worker"
        );
        Ok(IntegrateResult {
            worker_id: record.worker_id,
            bidding_group: record.bidding_group,
            perspective: record.perspective,
            state: record.state,
            ran_checks,
            skipped_checks,
        })
    }

    fn status(&self) -> Result<OrchestratorStatus> {
        let active_rulebook_commit = self.fs.load_active_rulebook()?.rulebook_commit;
        let bidding_groups = self.list_bidding_groups()?;
        let workers = self.list_workers()?;

        Ok(OrchestratorStatus { active_rulebook_commit, bidding_groups, workers })
    }
}

fn boundary_conflict(
    candidate_name: &PerspectiveName, candidate: &CompiledPerspective,
    active_name: &PerspectiveName, active: &CompiledPerspective,
) -> Option<RuntimeError> {
    let write_write =
        candidate.write().intersection(active.write()).cloned().collect::<BTreeSet<_>>();
    if !write_write.is_empty() {
        return Some(RuntimeError::SafetyConflict {
            perspective: candidate_name.clone(),
            blocking_group: active_name.clone(),
            relation: "write/write overlap",
            files: write_write.into_iter().collect(),
        });
    }

    let candidate_write_active_read =
        candidate.write().intersection(active.read()).cloned().collect::<BTreeSet<_>>();
    if !candidate_write_active_read.is_empty() {
        return Some(RuntimeError::SafetyConflict {
            perspective: candidate_name.clone(),
            blocking_group: active_name.clone(),
            relation: "candidate write overlaps active read",
            files: candidate_write_active_read.into_iter().collect(),
        });
    }

    let candidate_read_active_write =
        candidate.read().intersection(active.write()).cloned().collect::<BTreeSet<_>>();
    if !candidate_read_active_write.is_empty() {
        return Some(RuntimeError::SafetyConflict {
            perspective: candidate_name.clone(),
            blocking_group: active_name.clone(),
            relation: "candidate read overlaps active write",
            files: candidate_read_active_write.into_iter().collect(),
        });
    }

    None
}
