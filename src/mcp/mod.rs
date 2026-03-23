//! Model Context Protocol scaffolding for Multorum.
//!
//! This module defines a dependency-free MCP facade that describes the
//! tool and resource surface Multorum intends to expose. The real MCP
//! transport can be added later without changing the runtime service
//! layer or the CLI.
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

pub use dto::{ResourceDescriptor, ToolDescriptor, ToolInputDescriptor};
pub use error::{McpErrorCode, McpToolError};
pub use mode::McpMode;
pub use server::McpServer;
