//! MCP-facing descriptors for tools and resources.
//!
//! These descriptors are transport-neutral. A real MCP adapter can map
//! them to protocol registrations later.
//!
//! Bundle-publishing tools inherit Multorum's ownership-transfer
//! semantics for path-backed payloads: successful publication moves the
//! supplied files into `.multorum/` storage.

/// Description of one exposed MCP tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDescriptor {
    /// Stable protocol-visible tool name.
    pub name: &'static str,
    /// Human-readable summary of the tool's purpose.
    pub description: &'static str,
}

/// Description of one exposed MCP resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceDescriptor {
    /// Stable resource URI template.
    pub uri: &'static str,
    /// Human-readable summary of the resource contents.
    pub description: &'static str,
}
