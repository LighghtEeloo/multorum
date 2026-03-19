//! Orchestrator MCP resource surface.

use crate::mcp::dto::ResourceDescriptor;

/// Return the orchestrator MCP resource descriptors.
pub fn descriptors() -> Vec<ResourceDescriptor> {
    vec![
        ResourceDescriptor {
            uri: "multorum://orchestrator/status",
            description: "Projected orchestrator status for all active workers.",
        },
        ResourceDescriptor {
            uri: "multorum://orchestrator/rulebook/active",
            description: "Active rulebook commit governing the current runtime.",
        },
        ResourceDescriptor {
            uri: "multorum://orchestrator/perspectives",
            description: "Compiled perspective summaries from the active rulebook.",
        },
        ResourceDescriptor {
            uri: "multorum://orchestrator/workers",
            description: "Worker summary listing for the current runtime.",
        },
        ResourceDescriptor {
            uri: "multorum://orchestrator/workers/{perspective}/contract",
            description: "Worker contract projection for one perspective.",
        },
        ResourceDescriptor {
            uri: "multorum://orchestrator/workers/{perspective}/transcript",
            description: "Normalized transcript view for one perspective.",
        },
        ResourceDescriptor {
            uri: "multorum://orchestrator/workers/{perspective}/checks",
            description: "Integration and pre-merge check results for one perspective.",
        },
    ]
}
