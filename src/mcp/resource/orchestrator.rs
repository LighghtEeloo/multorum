//! Orchestrator MCP resource surface.

use crate::mcp::dto::{ResourceDescriptor, ResourceTemplateDescriptor};
use crate::methodology::MethodologyRole;

use super::ResourceDescriptors;

/// Return concrete orchestrator MCP resource descriptors.
pub fn descriptors() -> Vec<ResourceDescriptor> {
    vec![
        ResourceDescriptors::methodology(
            MethodologyRole::Orchestrator,
            "High-level orchestrator operating methodology shipped with Multorum.",
        ),
        ResourceDescriptors::json(
            "multorum://orchestrator/status",
            "Projected orchestrator snapshot including active perspectives and workers.",
        ),
        ResourceDescriptors::json(
            "multorum://orchestrator/perspectives",
            "Compiled perspective summaries from the current rulebook.",
        ),
        ResourceDescriptors::json(
            "multorum://orchestrator/workers",
            "Worker summary listing for the current runtime.",
        ),
    ]
}

/// Return parameterized orchestrator MCP resource templates.
pub fn templates() -> Vec<ResourceTemplateDescriptor> {
    vec![
        ResourceDescriptors::template(
            "multorum://orchestrator/workers/{worker}",
            "Detailed orchestrator-side view of one worker.",
        ),
        ResourceDescriptors::template(
            "multorum://orchestrator/workers/{worker}/outbox",
            "Outbox mailbox listing for one worker from the orchestrator view.",
        ),
    ]
}
