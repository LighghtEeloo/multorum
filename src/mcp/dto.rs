//! MCP-facing descriptors for tools and resources.
//!
//! These descriptors are transport-neutral. A real MCP adapter can map
//! them to protocol registrations later.
//!
//! Bundle-publishing tools inherit Multorum's ownership-transfer
//! semantics for path-backed payloads: successful publication moves the
//! supplied files into `.multorum/` storage.

/// Description of one MCP tool input field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInputDescriptor {
    /// Stable field name exposed by the transport adapter.
    pub name: &'static str,
    /// Human-readable summary of the field's meaning.
    pub description: &'static str,
    /// Whether the field must be supplied by the caller.
    pub required: bool,
}

/// Description of one exposed MCP tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDescriptor {
    /// Stable protocol-visible tool name.
    pub name: &'static str,
    /// Human-readable summary of the tool's purpose.
    pub description: &'static str,
    /// Structured input fields exposed by the tool.
    pub inputs: &'static [ToolInputDescriptor],
}

/// Description of one exposed MCP resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceDescriptor {
    /// Stable resource URI template.
    pub uri: &'static str,
    /// Human-readable summary of the resource contents.
    pub description: &'static str,
}
