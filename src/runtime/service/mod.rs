//! Runtime service traits.
//!
//! Frontends such as the CLI and MCP adapters call these traits instead
//! of manipulating runtime files directly.

pub(crate) mod filesystem;
pub mod orchestrator;
pub mod worker;

pub use orchestrator::{FilesystemOrchestratorService, OrchestratorService};
pub use worker::{FilesystemWorkerService, WorkerService};
