//! Worker MCP resource surface.

use crate::mcp::dto::{ResourceDescriptor, ResourceTemplateDescriptor};

/// Return concrete worker MCP resource descriptors.
pub fn descriptors() -> Vec<ResourceDescriptor> {
    vec![
        ResourceDescriptor {
            uri: "multorum://worker/contract",
            description: "Immutable worker contract for the active perspective.",
        },
        ResourceDescriptor {
            uri: "multorum://worker/inbox",
            description: "Inbox mailbox listing for the active worker.",
        },
        ResourceDescriptor {
            uri: "multorum://worker/status",
            description: "Projected worker lifecycle status.",
        },
    ]
}

/// Return parameterized worker MCP resource templates.
pub fn templates() -> Vec<ResourceTemplateDescriptor> {
    Vec::new()
}
