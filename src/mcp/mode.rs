//! MCP server operating modes.

use crate::perspective::PerspectiveName;

/// MCP server mode.
///
/// The mode determines which tools and resources are exposed and which
/// side of the orchestrator-worker boundary the server may mutate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpMode {
    /// Main-workspace orchestrator server.
    Orchestrator,
    /// Worker-local server scoped to one provisioned perspective.
    Worker { perspective: PerspectiveName },
}
