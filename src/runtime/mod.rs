//! Runtime primitives for Multorum orchestration.
//!
//! This module owns the typed application layer that backs both the CLI
//! and the future MCP transport. Filesystem-backed state remains the
//! source of truth; these types and traits provide the validated surface
//! through which frontends interact with that state.

pub mod bundle;
pub mod error;
pub mod mailbox;
pub mod paths;
pub mod projection;
pub mod service;
pub mod state;

pub use bundle::{
    BundleEnvelope, BundlePayload, MessageKind, MessageRef, PublishedBundle, ReplyReference,
    Sequence,
};
pub use error::{Result, RuntimeError};
pub use mailbox::{AckRef, MailboxDirection};
pub use paths::{MultorumPaths, OrchestratorPaths, WorkerPaths};
pub use projection::TranscriptView;
pub use state::{
    DiscardResult, IntegrateResult, MailboxMessageView, OrchestratorStatus, PerspectiveSummary,
    ProvisionResult, RulebookSwitch, RulebookValidation, WorkerContractView, WorkerState,
    WorkerStatus, WorkerSummary,
};
