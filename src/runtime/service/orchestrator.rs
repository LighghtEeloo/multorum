//! Orchestrator-facing runtime service surface.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::perspective::PerspectiveName;

use super::super::{
    MultorumPaths,
    bundle::{BundlePayload, MessageKind, PublishedBundle, ReplyReference},
    error::{Result, RuntimeError},
    state::{
        DiscardResult, IntegrateResult, OrchestratorStatus, PerspectiveSummary, ProvisionResult,
        RulebookSwitch, RulebookValidation, WorkerState, WorkerSummary,
    },
};
use super::filesystem::{
    ActiveRulebookRecord, RuntimeFileSystem, WorkerRecord, compiled_rulebook_paths,
    is_live_worker_state, validate_skip_request,
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
        &self, perspective: PerspectiveName, reply: ReplyReference, payload: BundlePayload,
    ) -> Result<PublishedBundle>;

    /// Publish a `revise` bundle to the worker inbox.
    ///
    /// Any path-backed payload files are moved into `.multorum/`
    /// storage if publication succeeds.
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

/// Filesystem-backed orchestrator runtime service.
///
/// The canonical `.multorum/` tree under the workspace root remains the
/// source of truth. This service simply validates operations, projects
/// their effects into runtime files, and delegates repository actions to
/// git where required by the design.
#[derive(Debug, Clone)]
pub struct FilesystemOrchestratorService {
    fs: RuntimeFileSystem,
}

impl FilesystemOrchestratorService {
    /// Construct the orchestrator service for a workspace root.
    pub fn new(workspace_root: impl Into<PathBuf>) -> Result<Self> {
        Ok(Self { fs: RuntimeFileSystem::new(workspace_root.into())? })
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

    fn ensure_live_worker_slot_is_free(&self, perspective: &PerspectiveName) -> Result<()> {
        match self.fs.load_worker_record(perspective) {
            | Ok(record) if is_live_worker_state(record.state) => Err(RuntimeError::InvalidState),
            | Ok(_) | Err(RuntimeError::UnknownPerspective(_)) => Ok(()),
            | Err(error) => Err(error),
        }
    }

    fn active_workers(&self) -> Result<Vec<WorkerRecord>> {
        Ok(self
            .fs
            .list_worker_records()?
            .into_iter()
            .filter(|record| is_live_worker_state(record.state))
            .collect())
    }

    fn publish_worker_inbox(
        &self, perspective: &PerspectiveName, kind: MessageKind, reply: ReplyReference,
        payload: BundlePayload,
    ) -> Result<PublishedBundle> {
        let record = self.fs.load_worker_record(perspective)?;
        let expected_state = match kind {
            | MessageKind::Resolve if record.state == WorkerState::Blocked => {
                Some(WorkerState::Blocked)
            }
            | MessageKind::Revise if record.state == WorkerState::Committed => {
                Some(WorkerState::Committed)
            }
            | _ => None,
        };
        if expected_state.is_none() {
            return Err(RuntimeError::InvalidState);
        }

        self.fs.publish_bundle(
            &record.worktree_path,
            crate::runtime::MailboxDirection::Inbox,
            kind,
            perspective,
            reply,
            None,
            payload,
        )
    }
}

impl OrchestratorService for FilesystemOrchestratorService {
    fn rulebook_validate(&self, commit: String) -> Result<RulebookValidation> {
        let compiled = self.fs.load_compiled_rulebook(&commit)?;
        let target_paths = compiled_rulebook_paths(&compiled);

        let mut blocking_workers = Vec::new();
        for worker in self.active_workers()? {
            let write_set = RuntimeFileSystem::read_path_list(
                &self.fs.worker_paths(&worker.perspective).write_set(),
            )?;
            if !write_set.is_disjoint(&target_paths) {
                blocking_workers.push(worker.perspective);
            }
        }

        Ok(RulebookValidation { ok: blocking_workers.is_empty(), blocking_workers })
    }

    fn rulebook_switch(&self, commit: String) -> Result<RulebookSwitch> {
        let validation = self.rulebook_validate(commit.clone())?;
        if !validation.ok {
            return Err(RuntimeError::RulebookConflict);
        }

        let record = ActiveRulebookRecord {
            rulebook_commit: commit.clone(),
            base_commit: commit.clone(),
            activated_at: super::filesystem::timestamp_now(),
        };
        self.fs.store_active_rulebook(&record)?;
        tracing::info!(rulebook_commit = commit, "activated rulebook");
        Ok(RulebookSwitch { active_commit: record.rulebook_commit })
    }

    fn list_perspectives(&self) -> Result<Vec<PerspectiveSummary>> {
        let (_, compiled) = self.fs.load_active_compiled_rulebook()?;
        Ok(compiled.perspective_summaries())
    }

    fn provision_worker(
        &self, perspective: PerspectiveName, task: Option<BundlePayload>,
    ) -> Result<ProvisionResult> {
        self.ensure_live_worker_slot_is_free(&perspective)?;

        let (active, compiled) = self.fs.load_active_compiled_rulebook()?;
        let compiled_perspective = compiled
            .perspectives()
            .get(&perspective)
            .ok_or_else(|| RuntimeError::UnknownPerspective(perspective.to_string()))?;

        let worktree_path = self.fs.worker_paths(&perspective).worktree_root().to_path_buf();
        self.fs.add_worktree(&worktree_path, &active.base_commit)?;

        let record = WorkerRecord {
            perspective: perspective.clone(),
            state: WorkerState::Provisioned,
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
                        crate::runtime::MailboxDirection::Inbox,
                        MessageKind::Task,
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

        tracing::info!(perspective = %perspective, root = %worktree_path.display(), "provisioned worker");

        Ok(ProvisionResult {
            perspective,
            worktree_path,
            state: WorkerState::Provisioned,
            seeded_task_path,
        })
    }

    fn resolve_worker(
        &self, perspective: PerspectiveName, reply: ReplyReference, payload: BundlePayload,
    ) -> Result<PublishedBundle> {
        self.publish_worker_inbox(&perspective, MessageKind::Resolve, reply, payload)
    }

    fn revise_worker(
        &self, perspective: PerspectiveName, reply: ReplyReference, payload: BundlePayload,
    ) -> Result<PublishedBundle> {
        self.publish_worker_inbox(&perspective, MessageKind::Revise, reply, payload)
    }

    fn discard_worker(&self, perspective: PerspectiveName) -> Result<DiscardResult> {
        let mut record = self.fs.load_worker_record(&perspective)?;
        if !matches!(
            record.state,
            WorkerState::Provisioned | WorkerState::Active | WorkerState::Committed
        ) {
            return Err(RuntimeError::InvalidState);
        }

        self.fs.remove_worktree(&record.worktree_path)?;
        record.state = WorkerState::Discarded;
        record.submitted_head_commit = None;
        self.fs.store_worker_record(&record)?;

        tracing::info!(perspective = %perspective, "discarded worker");
        Ok(DiscardResult { perspective, state: record.state })
    }

    fn integrate_worker(
        &self, perspective: PerspectiveName, skip_checks: Vec<String>,
    ) -> Result<IntegrateResult> {
        let mut record = self.fs.load_worker_record(&perspective)?;
        if record.state != WorkerState::Committed {
            return Err(RuntimeError::InvalidState);
        }

        let head_commit = record.submitted_head_commit.clone().ok_or_else(|| {
            RuntimeError::CheckFailed("worker has no submitted commit".to_owned())
        })?;
        self.fs.ensure_commit_exists(&record.worktree_path, &head_commit)?;

        let worker_head = self.fs.git_head(&record.worktree_path)?;
        if worker_head != head_commit {
            return Err(RuntimeError::CheckFailed(
                "worker worktree head changed after commit submission".to_owned(),
            ));
        }

        let worker_rulebook = self.fs.load_compiled_rulebook(&record.rulebook_commit)?;
        let allowed_skips = validate_skip_request(&worker_rulebook, &skip_checks)?;
        let changed_files =
            self.fs.git_changed_files(&record.worktree_path, &record.base_commit, &head_commit)?;
        let write_set = RuntimeFileSystem::read_path_list(
            &self.fs.worker_paths(&record.perspective).write_set(),
        )?;
        let violations = changed_files.difference(&write_set).cloned().collect::<BTreeSet<_>>();
        if !violations.is_empty() {
            tracing::warn!(perspective = %perspective, count = violations.len(), "write-set violation");
            return Err(RuntimeError::WriteSetViolation);
        }

        let mut ran_checks = Vec::new();
        let mut skipped_checks = Vec::new();
        for check_name in worker_rulebook.checks().pipeline() {
            let decl = worker_rulebook
                .checks()
                .get(check_name)
                .expect("compiled checks contain every pipeline entry");
            if allowed_skips.contains(check_name) {
                skipped_checks.push(check_name.to_string());
                continue;
            }

            self.fs.run_check(&record.worktree_path, check_name, decl.command())?;
            ran_checks.push(check_name.to_string());
        }

        self.fs.ensure_clean_workspace()?;
        self.fs.cherry_pick(&head_commit)?;
        self.fs.remove_worktree(&record.worktree_path)?;

        record.state = WorkerState::Integrated;
        self.fs.store_worker_record(&record)?;

        tracing::info!(perspective = %perspective, head_commit = head_commit, "integrated worker");
        Ok(IntegrateResult { perspective, state: record.state, ran_checks, skipped_checks })
    }

    fn status(&self) -> Result<OrchestratorStatus> {
        let active_rulebook_commit = self.fs.load_active_rulebook()?.rulebook_commit;
        let workers = self
            .active_workers()?
            .into_iter()
            .map(|record| WorkerSummary { perspective: record.perspective, state: record.state })
            .collect();

        Ok(OrchestratorStatus { active_rulebook_commit, workers })
    }
}
