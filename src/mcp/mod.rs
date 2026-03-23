//! Model Context Protocol scaffolding for Multorum.
//!
//! This module defines the dependency-light MCP facade and the stdio
//! transport used by Multorum. The runtime service layer remains
//! protocol-agnostic, while this module owns the protocol-visible tool,
//! resource, and error contracts.
//!
//! Path-backed bundle payloads follow the same ownership-transfer model
//! as the CLI: once publication succeeds, Multorum moves those files
//! into `.multorum/` storage instead of copying them.

pub mod dto;
pub mod error;
pub mod mode;
pub mod resource;
pub mod server;
pub mod tool;
pub mod transport;

pub use dto::{
    ResourceDescriptor, ResourceTemplateDescriptor, ToolDescriptor, ToolInputDescriptor,
    ToolInputType,
};
pub use error::{McpErrorCode, McpToolError};
pub use mode::McpMode;
pub use server::McpServer;
