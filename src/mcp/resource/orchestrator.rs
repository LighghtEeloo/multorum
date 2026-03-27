//! Orchestrator MCP resource surface.

use crate::mcp::dto::{ResourceDescriptor, ResourceTemplateDescriptor};
use crate::methodology::{MethodologyDocument, MethodologyRole};

/// Return concrete orchestrator MCP resource descriptors.
pub fn descriptors() -> Vec<ResourceDescriptor> {
    vec![
        ResourceDescriptor {
            uri: MethodologyDocument::new(MethodologyRole::Orchestrator).resource_uri(),
            description: "High-level orchestrator operating methodology shipped with Multorum.",
            mime_type: "text/markdown",
        },
        ResourceDescriptor {
            uri: "multorum://orchestrator/status",
            description: "Projected orchestrator snapshot including active perspectives and workers.",
            mime_type: "application/json",
        },
        ResourceDescriptor {
            uri: "multorum://orchestrator/perspectives",
            description: "Compiled perspective summaries from the current rulebook.",
            mime_type: "application/json",
        },
        ResourceDescriptor {
            uri: "multorum://orchestrator/workers",
            description: "Worker summary listing for the current runtime.",
            mime_type: "application/json",
        },
    ]
}

/// Return parameterized orchestrator MCP resource templates.
pub fn templates() -> Vec<ResourceTemplateDescriptor> {
    vec![
        ResourceTemplateDescriptor {
            uri_template: "multorum://orchestrator/workers/{worker}",
            description: "Detailed orchestrator-side view of one worker.",
            mime_type: "application/json",
        },
        ResourceTemplateDescriptor {
            uri_template: "multorum://orchestrator/workers/{worker}/outbox",
            description: "Outbox mailbox listing for one worker from the orchestrator view.",
            mime_type: "application/json",
        },
    ]
}
