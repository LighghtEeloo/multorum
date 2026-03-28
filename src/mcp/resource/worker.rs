//! Worker MCP resource surface.

use crate::mcp::dto::{ResourceDescriptor, ResourceTemplateDescriptor};
use crate::methodology::MethodologyRole;

use super::ResourceDescriptors;

/// Return concrete worker MCP resource descriptors.
pub fn descriptors() -> Vec<ResourceDescriptor> {
    vec![
        ResourceDescriptors::methodology(
            MethodologyRole::Worker,
            "High-level worker operating methodology shipped with Multorum.",
        ),
        ResourceDescriptors::json(
            "multorum://worker/contract",
            "Immutable worker contract for the active perspective.",
        ),
        ResourceDescriptors::json(
            "multorum://worker/inbox",
            "Inbox mailbox listing for the active worker.",
        ),
        ResourceDescriptors::json("multorum://worker/status", "Projected worker lifecycle status."),
    ]
}

/// Return parameterized worker MCP resource templates.
pub fn templates() -> Vec<ResourceTemplateDescriptor> {
    Vec::new()
}
