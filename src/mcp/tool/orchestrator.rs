//! Orchestrator MCP tool surface.

use crate::mcp::dto::{ToolDescriptor, ToolInputDescriptor};

const GET_WORKER_INPUTS: &[ToolInputDescriptor] = &[ToolInputDescriptor {
    name: "worker_id",
    description: "Runtime worker identity to inspect.",
    required: true,
}];

const CREATE_WORKER_INPUTS: &[ToolInputDescriptor] = &[
    ToolInputDescriptor {
        name: "perspective",
        description: "Compiled perspective to instantiate.",
        required: true,
    },
    ToolInputDescriptor {
        name: "worker_id",
        description: "Optional orchestrator-selected runtime worker identity. When omitted, Multorum allocates the default perspective-based worker id.",
        required: false,
    },
    ToolInputDescriptor {
        name: "overwriting_worktree",
        description: "Optional flag to replace an existing finalized workspace for the same explicit worker id.",
        required: false,
    },
    ToolInputDescriptor {
        name: "body",
        description: "Optional Markdown file to move into the seeded task bundle body.",
        required: false,
    },
    ToolInputDescriptor {
        name: "artifacts",
        description: "Optional files to move into the seeded task bundle artifacts directory.",
        required: false,
    },
];

const REPLY_BUNDLE_INPUTS: &[ToolInputDescriptor] = &[
    ToolInputDescriptor {
        name: "worker_id",
        description: "Runtime worker identity that owns the inbox.",
        required: true,
    },
    ToolInputDescriptor {
        name: "reply_to",
        description: "Optional mailbox sequence number answered by this bundle.",
        required: false,
    },
    ToolInputDescriptor {
        name: "body",
        description: "Optional Markdown file to move into the bundle body.",
        required: false,
    },
    ToolInputDescriptor {
        name: "artifacts",
        description: "Optional files to move into the bundle artifacts directory.",
        required: false,
    },
];

const FINALIZED_WORKER_INPUTS: &[ToolInputDescriptor] = &[ToolInputDescriptor {
    name: "worker_id",
    description: "Runtime worker identity to act on.",
    required: true,
}];

const MERGE_WORKER_INPUTS: &[ToolInputDescriptor] = &[
    ToolInputDescriptor {
        name: "worker_id",
        description: "Runtime worker identity to merge.",
        required: true,
    },
    ToolInputDescriptor {
        name: "skip_checks",
        description: "Optional project-defined checks to skip based on trusted worker evidence.",
        required: false,
    },
];

/// Return the orchestrator MCP tool descriptors.
pub fn descriptors() -> Vec<ToolDescriptor> {
    vec![
        ToolDescriptor {
            name: "rulebook_init",
            description: "Initialize .multorum with the default committed rulebook artifacts.",
            inputs: &[],
        },
        ToolDescriptor {
            name: "rulebook_validate",
            description: "Dry-run validation of the HEAD rulebook against active bidding groups.",
            inputs: &[],
        },
        ToolDescriptor {
            name: "rulebook_install",
            description: "Activate the HEAD rulebook after validation.",
            inputs: &[],
        },
        ToolDescriptor {
            name: "rulebook_uninstall",
            description: "Deactivate the active rulebook. Rejected if any live bidding group still depends on it.",
            inputs: &[],
        },
        ToolDescriptor {
            name: "list_perspectives",
            description: "List compiled perspectives from the active rulebook.",
            inputs: &[],
        },
        ToolDescriptor {
            name: "list_workers",
            description: "List active workers in the current runtime.",
            inputs: &[],
        },
        ToolDescriptor {
            name: "get_worker",
            description: "Load one worker detail view by worker id.",
            inputs: GET_WORKER_INPUTS,
        },
        ToolDescriptor {
            name: "create_worker",
            description: "Create a worker workspace and optional initial task bundle; path-backed payload files are moved into .multorum storage.",
            inputs: CREATE_WORKER_INPUTS,
        },
        ToolDescriptor {
            name: "resolve_worker",
            description: "Publish a resolve bundle to a blocked worker inbox; path-backed payload files are moved into .multorum storage.",
            inputs: REPLY_BUNDLE_INPUTS,
        },
        ToolDescriptor {
            name: "revise_worker",
            description: "Publish a revise bundle to a committed worker inbox; path-backed payload files are moved into .multorum storage.",
            inputs: REPLY_BUNDLE_INPUTS,
        },
        ToolDescriptor {
            name: "discard_worker",
            description: "Finalize a worker without integration while preserving its workspace.",
            inputs: FINALIZED_WORKER_INPUTS,
        },
        ToolDescriptor {
            name: "delete_worker",
            description: "Delete one finalized worker workspace.",
            inputs: FINALIZED_WORKER_INPUTS,
        },
        ToolDescriptor {
            name: "merge_worker",
            description: "Run the pre-merge pipeline and merge a worker submission.",
            inputs: MERGE_WORKER_INPUTS,
        },
        ToolDescriptor {
            name: "get_status",
            description: "Return the full orchestrator runtime snapshot.",
            inputs: &[],
        },
    ]
}
