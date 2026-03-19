//! Worker-facing runtime service surface.

use crate::runtime::mailbox::AckRef;

use super::super::{
    bundle::{BundlePayload, PublishedBundle, ReplyReference, Sequence},
    error::{Result, RuntimeError},
    state::{MailboxMessageView, WorkerContractView, WorkerStatus},
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

/// Placeholder worker service used while the runtime is scaffolded.
#[derive(Debug, Default)]
pub struct NoopWorkerService;

impl WorkerService for NoopWorkerService {
    fn contract(&self) -> Result<WorkerContractView> {
        Err(RuntimeError::Unimplemented("worker_contract"))
    }

    fn read_inbox(&self, _after: Option<Sequence>) -> Result<Vec<MailboxMessageView>> {
        Err(RuntimeError::Unimplemented("read_inbox"))
    }

    fn ack_inbox(&self, _sequence: Sequence) -> Result<AckRef> {
        Err(RuntimeError::Unimplemented("ack_inbox"))
    }

    fn send_report(
        &self, _head_commit: Option<String>, _reply: ReplyReference, _payload: BundlePayload,
    ) -> Result<PublishedBundle> {
        Err(RuntimeError::Unimplemented("send_report"))
    }

    fn send_commit(
        &self, _head_commit: String, _payload: BundlePayload,
    ) -> Result<PublishedBundle> {
        Err(RuntimeError::Unimplemented("send_commit"))
    }

    fn status(&self) -> Result<WorkerStatus> {
        Err(RuntimeError::Unimplemented("worker_status"))
    }
}
