//! Orchestrator MCP resource surface.

use crate::mcp::dto::{ResourceDescriptor, ResourceTemplateDescriptor};

/// Return concrete orchestrator MCP resource descriptors.
pub fn descriptors() -> Vec<ResourceDescriptor> {
    vec![
        ResourceDescriptor {
            uri: "multorum://orchestrator/status",
            description: "Projected orchestrator snapshot including active perspectives and workers.",
        },
        ResourceDescriptor {
            uri: "multorum://orchestrator/perspectives",
            description: "Compiled perspective summaries from the current rulebook.",
        },
        ResourceDescriptor {
            uri: "multorum://orchestrator/workers",
            description: "Worker summary listing for the current runtime.",
        },
    ]
}

/// Return parameterized orchestrator MCP resource templates.
pub fn templates() -> Vec<ResourceTemplateDescriptor> {
    vec![
        ResourceTemplateDescriptor {
            uri_template: "multorum://orchestrator/workers/{worker}",
            description: "Detailed orchestrator-side view of one worker.",
        },
        ResourceTemplateDescriptor {
            uri_template: "multorum://orchestrator/workers/{worker}/outbox",
            description: "Outbox mailbox listing for one worker from the orchestrator view.",
        },
    ]
}
