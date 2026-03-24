//! Orchestrator MCP tool surface.

use crate::mcp::dto::{ToolDescriptor, ToolInputDescriptor, ToolInputType};

const GET_WORKER_INPUTS: &[ToolInputDescriptor] = &[ToolInputDescriptor {
    name: "worker_id",
    description: "Runtime worker identity to inspect.",
    kind: ToolInputType::String,
    required: true,
}];

const READ_OUTBOX_INPUTS: &[ToolInputDescriptor] = &[
    ToolInputDescriptor {
        name: "worker_id",
        description: "Runtime worker identity whose outbox should be read.",
        kind: ToolInputType::String,
        required: true,
    },
    ToolInputDescriptor {
        name: "after",
        description: "Optional sequence number; only outbox bundles after it are returned.",
        kind: ToolInputType::Integer,
        required: false,
    },
];

const ACK_OUTBOX_INPUTS: &[ToolInputDescriptor] = &[
    ToolInputDescriptor {
        name: "worker_id",
        description: "Runtime worker identity whose outbox owns the message.",
        kind: ToolInputType::String,
        required: true,
    },
    ToolInputDescriptor {
        name: "sequence",
        description: "Outbox sequence number to acknowledge.",
        kind: ToolInputType::Integer,
        required: true,
    },
];

const CREATE_WORKER_INPUTS: &[ToolInputDescriptor] = &[
    ToolInputDescriptor {
        name: "perspective",
        description: "Compiled perspective to instantiate.",
        kind: ToolInputType::String,
        required: true,
    },
    ToolInputDescriptor {
        name: "worker_id",
        description: "Optional orchestrator-selected runtime worker identity. When omitted, Multorum allocates the default perspective-based worker id.",
        kind: ToolInputType::String,
        required: false,
    },
    ToolInputDescriptor {
        name: "overwriting_worktree",
        description: "Optional flag to replace an existing finalized workspace for the same explicit worker id.",
        kind: ToolInputType::Boolean,
        required: false,
    },
    ToolInputDescriptor {
        name: "body",
        description: "Optional Markdown file to move into the seeded task bundle body.",
        kind: ToolInputType::String,
        required: false,
    },
    ToolInputDescriptor {
        name: "artifacts",
        description: "Optional files to move into the seeded task bundle artifacts directory.",
        kind: ToolInputType::StringList,
        required: false,
    },
];

const REPLY_BUNDLE_INPUTS: &[ToolInputDescriptor] = &[
    ToolInputDescriptor {
        name: "worker_id",
        description: "Runtime worker identity that owns the inbox.",
        kind: ToolInputType::String,
        required: true,
    },
    ToolInputDescriptor {
        name: "reply_to",
        description: "Optional mailbox sequence number answered by this bundle.",
        kind: ToolInputType::Integer,
        required: false,
    },
    ToolInputDescriptor {
        name: "body",
        description: "Optional Markdown file to move into the bundle body.",
        kind: ToolInputType::String,
        required: false,
    },
    ToolInputDescriptor {
        name: "artifacts",
        description: "Optional files to move into the bundle artifacts directory.",
        kind: ToolInputType::StringList,
        required: false,
    },
];

const FINALIZED_WORKER_INPUTS: &[ToolInputDescriptor] = &[ToolInputDescriptor {
    name: "worker_id",
    description: "Runtime worker identity to act on.",
    kind: ToolInputType::String,
    required: true,
}];

const MERGE_WORKER_INPUTS: &[ToolInputDescriptor] = &[
    ToolInputDescriptor {
        name: "worker_id",
        description: "Runtime worker identity to merge.",
        kind: ToolInputType::String,
        required: true,
    },
    ToolInputDescriptor {
        name: "skip_checks",
        description: "Optional project-defined checks to skip based on trusted worker evidence.",
        kind: ToolInputType::StringList,
        required: false,
    },
    ToolInputDescriptor {
        name: "body",
        description: "Optional Markdown file to move into the audit rationale body.",
        kind: ToolInputType::String,
        required: false,
    },
    ToolInputDescriptor {
        name: "artifacts",
        description: "Optional files to move into the audit rationale artifacts directory.",
        kind: ToolInputType::StringList,
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
            name: "read_worker_outbox",
            description: "List one worker outbox after an optional sequence number.",
            inputs: READ_OUTBOX_INPUTS,
        },
        ToolDescriptor {
            name: "ack_worker_outbox_message",
            description: "Acknowledge a consumed worker outbox bundle.",
            inputs: ACK_OUTBOX_INPUTS,
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
