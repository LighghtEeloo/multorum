//! Runtime primitives for Multorum orchestration.
//!
//! This module owns the typed application layer that backs both the CLI
//! and the future MCP transport. Filesystem-backed state remains the
//! source of truth; these types and traits provide the validated surface
//! through which frontends interact with that state.

pub mod error;
pub mod mailbox;
pub(crate) mod storage;
pub mod orchestrator;
pub mod paths;
pub(crate) mod project;
pub mod state;
pub mod timestamp;
pub mod worker;
pub mod worker_id;

pub use crate::bundle::BundlePayload;
pub use error::{Result, RuntimeError};
pub use mailbox::{
    AckRef, BundleEnvelope, MailboxDirection, MessageKind, MessageRef, PublishedBundle,
    ReplyReference, Sequence,
};
pub use orchestrator::{CreateWorker, FsOrchestratorService, OrchestratorService};
pub use paths::{MultorumPaths, OrchestratorPaths, WorkerPaths};
pub use state::{
    ActivePerspectiveSummary, AuditEntry, CreateResult, DeleteResult, DiscardResult,
    MailboxMessageView, MergeResult, OrchestratorStatus, PerspectiveConflict,
    PerspectiveForwardResult, PerspectiveSummary, PerspectiveValidation, RulebookInit,
    TranscriptView, WorkerContractView, WorkerDetail, WorkerState, WorkerStatus, WorkerSummary,
};
pub use timestamp::Timestamp;
pub use worker::{FsWorkerService, WorkerService};
pub use worker_id::{WorkerId, WorkerIdError};
