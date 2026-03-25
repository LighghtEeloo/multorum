//! Worker-facing runtime service surface.
//!
//! This module defines the typed operations available to worker
//! frontends and the default storage-backed implementation used by
//! the CLI.

use std::path::PathBuf;
use std::sync::Arc;

use crate::vcs::{CanonicalCommitHash, GitVcs, VersionControl};

use crate::bundle::BundlePayload;

use super::{
    WorkerPaths,
    error::{Result, RuntimeError},
    mailbox::{AckRef, MailboxDirection, MessageKind, PublishedBundle, ReplyReference, Sequence},
    project::CurrentProject,
    state::{MailboxMessageView, WorkerContractView, WorkerState, WorkerStatus},
    storage::RuntimeFs,
};

/// Typed operations available to a worker frontend.
pub trait WorkerService {
    /// Load the worker contract view.
    fn contract(&self) -> Result<WorkerContractView>;

    /// Read inbox messages after the provided sequence number.
    fn read_inbox(&self, after: Option<Sequence>) -> Result<Vec<MailboxMessageView>>;

    /// Acknowledge an inbox message.
    fn ack_inbox(&self, sequence: Sequence) -> Result<AckRef>;

    /// Publish a worker blocker report.
    ///
    /// Any path-backed payload files are moved into `.multorum/`
    /// storage if publication succeeds.
    ///
    /// Note: A later perspective-forward operation can preserve worker
    /// progress only from the `head_commit` recorded here.
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
        tracing::trace!(worktree_root = %self.worktree_root.display(), "loading worker contract");
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
        if !record.state.can_submit() {
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
        tracing::trace!(
            worktree_root = %self.worktree_root.display(),
            worker_id = %contract.worker_id,
            after_sequence = ?after.map(|s| s.0),
            "reading worker inbox"
        );
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
        tracing::trace!(
            worktree_root = %self.worktree_root.display(),
            sequence = sequence.0,
            "acknowledged inbox message"
        );
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
        tracing::info!(
            worktree_root = %self.worktree_root.display(),
            kind = ?MessageKind::Report,
            "published worker report"
        );
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
        self.update_submission_state(WorkerState::Committed, Some(head_commit.clone()))?;
        tracing::info!(
            worktree_root = %self.worktree_root.display(),
            head_commit = %head_commit,
            "published worker commit"
        );
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
