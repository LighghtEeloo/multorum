//! Runtime service traits.
//!
//! Frontends such as the CLI and MCP adapters call these traits instead
//! of manipulating runtime files directly.

pub mod orchestrator;
pub mod worker;

pub use orchestrator::{NoopOrchestratorService, OrchestratorService};
pub use worker::{NoopWorkerService, WorkerService};
