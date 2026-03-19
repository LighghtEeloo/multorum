//! Orchestrator MCP tool surface.

use crate::mcp::dto::ToolDescriptor;

/// Return the orchestrator MCP tool descriptors.
pub fn descriptors() -> Vec<ToolDescriptor> {
    vec![
        ToolDescriptor {
            name: "rulebook_validate",
            description: "Dry-run validation of a rulebook commit switch.",
        },
        ToolDescriptor {
            name: "rulebook_switch",
            description: "Activate a new rulebook commit after validation.",
        },
        ToolDescriptor {
            name: "list_perspectives",
            description: "List compiled perspectives from the active rulebook.",
        },
        ToolDescriptor {
            name: "provision_worker",
            description: "Provision a worker worktree and optional initial task bundle; path-backed payload files are moved into .multorum storage.",
        },
        ToolDescriptor {
            name: "resolve_worker",
            description: "Publish a resolve bundle to a blocked worker inbox; path-backed payload files are moved into .multorum storage.",
        },
        ToolDescriptor {
            name: "revise_worker",
            description: "Publish a revise bundle to a committed worker inbox; path-backed payload files are moved into .multorum storage.",
        },
        ToolDescriptor {
            name: "discard_worker",
            description: "Tear down a worker without integration.",
        },
        ToolDescriptor {
            name: "integrate_worker",
            description: "Run the pre-merge pipeline and integrate a worker submission.",
        },
        ToolDescriptor {
            name: "get_status",
            description: "Return the orchestrator status projection.",
        },
    ]
}
