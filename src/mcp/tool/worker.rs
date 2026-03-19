//! Worker MCP tool surface.

use crate::mcp::dto::ToolDescriptor;

/// Return the worker MCP tool descriptors.
pub fn descriptors() -> Vec<ToolDescriptor> {
    vec![
        ToolDescriptor { name: "get_contract", description: "Load the immutable worker contract." },
        ToolDescriptor {
            name: "read_inbox",
            description: "List inbox bundles after an optional sequence number.",
        },
        ToolDescriptor {
            name: "ack_inbox_message",
            description: "Acknowledge a consumed inbox bundle.",
        },
        ToolDescriptor {
            name: "send_report",
            description: "Publish a worker blocker report bundle to the outbox.",
        },
        ToolDescriptor {
            name: "send_commit",
            description: "Publish a completed worker submission bundle to the outbox.",
        },
        ToolDescriptor { name: "get_status", description: "Return the worker status projection." },
    ]
}
