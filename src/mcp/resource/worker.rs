//! Worker MCP resource surface.

use crate::mcp::dto::ResourceDescriptor;

/// Return the worker MCP resource descriptors.
pub fn descriptors() -> Vec<ResourceDescriptor> {
    vec![
        ResourceDescriptor {
            uri: "multorum://worker/contract",
            description: "Immutable worker contract for the active perspective.",
        },
        ResourceDescriptor {
            uri: "multorum://worker/read-set",
            description: "Compiled read set for the active worker.",
        },
        ResourceDescriptor {
            uri: "multorum://worker/write-set",
            description: "Compiled write set for the active worker.",
        },
        ResourceDescriptor {
            uri: "multorum://worker/inbox",
            description: "Inbox mailbox listing for the active worker.",
        },
        ResourceDescriptor {
            uri: "multorum://worker/outbox",
            description: "Outbox mailbox listing for the active worker.",
        },
        ResourceDescriptor {
            uri: "multorum://worker/transcript",
            description: "Normalized transcript view for the active worker.",
        },
        ResourceDescriptor {
            uri: "multorum://worker/status",
            description: "Projected worker lifecycle status.",
        },
    ]
}
