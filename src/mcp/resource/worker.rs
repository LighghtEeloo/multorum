//! Worker MCP resource surface.

use crate::mcp::dto::{ResourceDescriptor, ResourceTemplateDescriptor};
use crate::methodology::{MethodologyDocument, MethodologyRole};

/// Return concrete worker MCP resource descriptors.
pub fn descriptors() -> Vec<ResourceDescriptor> {
    vec![
        ResourceDescriptor {
            uri: MethodologyDocument::new(MethodologyRole::Worker).resource_uri(),
            description: "High-level worker operating methodology shipped with Multorum.",
            mime_type: "text/markdown",
        },
        ResourceDescriptor {
            uri: "multorum://worker/contract",
            description: "Immutable worker contract for the active perspective.",
            mime_type: "application/json",
        },
        ResourceDescriptor {
            uri: "multorum://worker/inbox",
            description: "Inbox mailbox listing for the active worker.",
            mime_type: "application/json",
        },
        ResourceDescriptor {
            uri: "multorum://worker/status",
            description: "Projected worker lifecycle status.",
            mime_type: "application/json",
        },
    ]
}

/// Return parameterized worker MCP resource templates.
pub fn templates() -> Vec<ResourceTemplateDescriptor> {
    Vec::new()
}
