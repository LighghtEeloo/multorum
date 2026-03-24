//! Worker-facing runtime service surface.
//!
//! This module defines the typed operations available to worker
//! frontends and the default storage-backed implementation used by
//! the CLI.

use std::path::PathBuf;
use std::sync::Arc;

use crate::vcs::{CanonicalCommitHash, GitVcs, VersionControl};

use super::{
    MailboxDirection, WorkerPaths,
    bundle::{BundlePayload, MessageKind, PublishedBundle, ReplyReference, Sequence},
    error::{Result, RuntimeError},
    mailbox::AckRef,
    project::CurrentProject,
    state::{MailboxMessageView, WorkerContractView, WorkerState, WorkerStatus},
    storage::{RuntimeFs, can_submit_from_state},
};

/// Typed operations available to a worker frontend.
pub trait WorkerService {
    /// Load the immutable worker contract.
    fn contract(&self) -> Result<WorkerContractView>;

    /// Read inbox messages after the provided sequence number.
    fn read_inbox(&self, after: Option<Sequence>) -> Result<Vec<MailboxMessageView>>;

    /// Acknowledge an inbox message.
    fn ack_inbox(&self, sequence: Sequence) -> Result<AckRef>;

    /// Publish a worker blocker report.
    ///
    /// Any path-backed payload files are moved into `.multorum/`
    /// storage if publication succeeds.
    fn send_report(
        &self, head_commit: Option<String>, reply: ReplyReference, payload: BundlePayload,
    ) -> Result<PublishedBundle>;

    /// Publish a completed worker commit submission.
    ///
    /// Any path-backed payload files are moved into `.multorum/`
    /// storage if publication succeeds.
    fn send_commit(&self, head_commit: String, payload: BundlePayload) -> Result<PublishedBundle>;

    /// Return the current worker status projection.
    fn status(&self) -> Result<WorkerStatus>;
}

/// Storage-backed worker runtime service.
///
/// The service is bound to one active worker worktree and derives the
/// canonical orchestrator control plane from the managed
/// `.multorum/tr/<worker-id>` location created during worker
/// creation. Repository-specific discovery is delegated to the
/// configured version-control backend.
#[derive(Debug, Clone)]
pub struct FsWorkerService {
    fs: RuntimeFs,
    worktree_root: PathBuf,
}

impl FsWorkerService {
    /// Construct the worker service for an explicit worktree root.
    pub fn new(worktree_root: impl Into<PathBuf>) -> Result<Self> {
        let worktree_root = worktree_root.into().canonicalize()?;
        let workspace_root = WorkerPaths::new(worktree_root.clone()).workspace_root()?;
        Ok(Self { fs: RuntimeFs::new(workspace_root)?, worktree_root })
    }

    /// Construct the worker service for an explicit worktree root with
    /// one repository backend.
    pub fn with_vcs(
        worktree_root: impl Into<PathBuf>, vcs: Arc<dyn VersionControl>,
    ) -> Result<Self> {
        let worktree_root = worktree_root.into().canonicalize()?;
        let workspace_root = WorkerPaths::new(worktree_root.clone()).workspace_root()?;
        Ok(Self { fs: RuntimeFs::with_vcs(workspace_root, vcs)?, worktree_root })
    }

    /// Construct the worker service from the current directory.
    pub fn from_current_dir() -> Result<Self> {
        let vcs: Arc<dyn VersionControl> = Arc::new(GitVcs::new());
        let project = CurrentProject::with_vcs(&std::env::current_dir()?, Arc::clone(&vcs))?;
        let worktree_root = project.worker_repo_root()?.to_path_buf();
        Self::with_vcs(worktree_root, vcs)
    }

    fn contract_view(&self) -> Result<WorkerContractView> {
        self.fs.load_worker_contract(&self.worktree_root)
    }

    fn update_state_after_ack(&self, message: &AckRef) -> Result<()> {
        let mut record = self.fs.load_worker_record(&message.message.worker_id)?;
        if record.worktree_path != self.worktree_root {
            return Err(RuntimeError::MissingWorkerRuntime(
                self.worktree_root.display().to_string(),
            ));
        }

        match message.message.kind {
            | MessageKind::Task | MessageKind::Resolve | MessageKind::Revise => {
                record.state = WorkerState::Active;
                self.fs.store_worker_record(&record)?;
            }
            | MessageKind::Report | MessageKind::Commit => {}
        }

        Ok(())
    }

    fn update_submission_state(
        &self, state: WorkerState, head_commit: Option<CanonicalCommitHash>,
    ) -> Result<()> {
        let contract = self.contract_view()?;
        let mut record = self.fs.load_worker_record(&contract.worker_id)?;
        if !can_submit_from_state(record.state) {
            return Err(RuntimeError::InvalidState {
                operation: "publish worker submission",
                expected: "ACTIVE",
                actual: record.state,
            });
        }
        record.state = state;
        record.submitted_head_commit = head_commit;
        self.fs.store_worker_record(&record)
    }
}

impl WorkerService for FsWorkerService {
    fn contract(&self) -> Result<WorkerContractView> {
        self.contract_view()
    }

    fn read_inbox(&self, after: Option<Sequence>) -> Result<Vec<MailboxMessageView>> {
        let contract = self.contract_view()?;
        self.fs.list_mailbox_messages(
            &self.worktree_root,
            &contract.worker_id,
            MailboxDirection::Inbox,
            after,
        )
    }

    fn ack_inbox(&self, sequence: Sequence) -> Result<AckRef> {
        let ack =
            self.fs.acknowledge_message(&self.worktree_root, MailboxDirection::Inbox, sequence)?;
        self.update_state_after_ack(&ack)?;
        Ok(ack)
    }

    fn send_report(
        &self, head_commit: Option<String>, reply: ReplyReference, payload: BundlePayload,
    ) -> Result<PublishedBundle> {
        let contract = self.contract_view()?;
        let head_commit = head_commit
            .as_deref()
            .map(|revision| {
                self.fs.vcs().resolve_commit(
                    &self.worktree_root,
                    revision,
                    "verify reported worker commit",
                )
            })
            .transpose()?;
        let message = self.fs.publish_bundle(
            &self.worktree_root,
            MailboxDirection::Outbox,
            MessageKind::Report,
            &contract.worker_id,
            &contract.perspective,
            reply,
            head_commit,
            payload,
        )?;
        self.update_submission_state(WorkerState::Blocked, None)?;
        Ok(message)
    }

    fn send_commit(&self, head_commit: String, payload: BundlePayload) -> Result<PublishedBundle> {
        let head_commit = self.fs.vcs().resolve_commit(
            &self.worktree_root,
            &head_commit,
            "verify submitted worker commit",
        )?;
        let contract = self.contract_view()?;
        let message = self.fs.publish_bundle(
            &self.worktree_root,
            MailboxDirection::Outbox,
            MessageKind::Commit,
            &contract.worker_id,
            &contract.perspective,
            ReplyReference::default(),
            Some(head_commit.clone()),
            payload,
        )?;
        self.update_submission_state(WorkerState::Committed, Some(head_commit))?;
        Ok(message)
    }

    fn status(&self) -> Result<WorkerStatus> {
        let contract = self.contract_view()?;
        let record = self.fs.load_worker_record(&contract.worker_id)?;
        Ok(WorkerStatus {
            worker_id: contract.worker_id,
            perspective: contract.perspective,
            state: record.state,
        })
    }
}
