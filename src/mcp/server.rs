//! Dependency-free MCP server facade.
//!
//! This module assembles the tool and resource surfaces for a given mode.
//! The real protocol transport can wrap this facade later.

use crate::runtime::WorkerId;

use super::{
    dto::{ResourceDescriptor, ToolDescriptor},
    mode::McpMode,
    resource, tool,
};

/// Assembled MCP surface for one server mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServer {
    /// Active server mode.
    pub mode: McpMode,
    /// Registered tools for the mode.
    pub tools: Vec<ToolDescriptor>,
    /// Registered resources for the mode.
    pub resources: Vec<ResourceDescriptor>,
}

impl McpServer {
    /// Construct the orchestrator MCP surface.
    pub fn orchestrator() -> Self {
        Self {
            mode: McpMode::Orchestrator,
            tools: tool::orchestrator::descriptors(),
            resources: resource::orchestrator::descriptors(),
        }
    }

    /// Construct the worker MCP surface for one worker.
    pub fn worker(worker_id: WorkerId) -> Self {
        Self {
            mode: McpMode::Worker { worker_id },
            tools: tool::worker::descriptors(),
            resources: resource::worker::descriptors(),
        }
    }
}
