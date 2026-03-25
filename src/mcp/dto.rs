//! MCP-facing descriptors for tools and resources.
//!
//! These descriptors are transport-neutral. The stdio transport maps
//! them to `rmcp` registrations without coupling the runtime service
//! layer to one protocol implementation.
//!
//! Bundle-publishing tools inherit Multorum's ownership-transfer
//! semantics for path-backed payloads: successful publication moves the
//! supplied files into `.multorum/` storage.

/// JSON-schema-visible type of one MCP tool input field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolInputType {
    /// JSON string input.
    String,
    /// JSON integer input.
    Integer,
    /// JSON boolean input.
    Boolean,
    /// JSON array whose items are strings.
    StringList,
}

/// Description of one MCP tool input field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInputDescriptor {
    /// Stable field name exposed by the transport adapter.
    pub name: &'static str,
    /// Human-readable summary of the field's meaning.
    pub description: &'static str,
    /// JSON-schema-visible field type.
    pub kind: ToolInputType,
    /// Whether the field must be supplied by the caller.
    pub required: bool,
}

impl ToolInputDescriptor {
    /// Construct one MCP tool input descriptor with explicit requiredness.
    pub const fn new(
        name: &'static str, description: &'static str, kind: ToolInputType, required: bool,
    ) -> Self {
        Self { name, description, kind, required }
    }

    /// Construct one required MCP tool input descriptor.
    pub const fn required(
        name: &'static str, description: &'static str, kind: ToolInputType,
    ) -> Self {
        Self::new(name, description, kind, true)
    }

    /// Construct one optional MCP tool input descriptor.
    pub const fn optional(
        name: &'static str, description: &'static str, kind: ToolInputType,
    ) -> Self {
        Self::new(name, description, kind, false)
    }
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

/// Description of one parameterized exposed MCP resource template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceTemplateDescriptor {
    /// Stable URI template exposed by the transport adapter.
    pub uri_template: &'static str,
    /// Human-readable summary of the template contents.
    pub description: &'static str,
}
