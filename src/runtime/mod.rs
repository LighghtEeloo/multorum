//! Runtime primitives for Multorum orchestration.
//!
//! This module owns the typed application layer that backs both the CLI
//! and the future MCP transport. Filesystem-backed state remains the
//! source of truth; these types and traits provide the validated surface
//! through which frontends interact with that state.

pub mod bundle;
pub mod error;
pub(crate) mod storage;
pub mod orchestrator;
pub mod paths;
pub(crate) mod project;
pub mod state;
pub mod worker;
pub mod worker_id;

pub use bundle::{
    AckRef, BundleEnvelope, BundlePayload, MailboxDirection, MessageKind, MessageRef,
    PublishedBundle, ReplyReference, Sequence,
};
pub use error::{Result, RuntimeError};
pub use orchestrator::{CreateWorker, FsOrchestratorService, OrchestratorService};
pub use paths::{MultorumPaths, OrchestratorPaths, WorkerPaths};
pub use state::{
    ActivePerspectiveSummary, AuditEntry, CreateResult, DeleteResult, DiscardResult,
    MailboxMessageView, MergeResult, OrchestratorStatus, PerspectiveSummary, RulebookInit,
    RulebookInstall, RulebookUninstall, RulebookValidation, TranscriptView, WorkerContractView,
    WorkerDetail, WorkerState, WorkerStatus, WorkerSummary,
};
pub use worker::{FsWorkerService, WorkerService};
pub use worker_id::{WorkerId, WorkerIdError};
