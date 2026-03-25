//! Orchestrator MCP tool surface.

use crate::mcp::dto::{ToolDescriptor, ToolInputDescriptor, ToolInputType};

const GET_WORKER_INPUTS: &[ToolInputDescriptor] = &[ToolInputDescriptor::required(
    "worker_id",
    "Runtime worker identity to inspect.",
    ToolInputType::String,
)];

const READ_OUTBOX_INPUTS: &[ToolInputDescriptor] = &[
    ToolInputDescriptor::required(
        "worker_id",
        "Runtime worker identity whose outbox should be read.",
        ToolInputType::String,
    ),
    ToolInputDescriptor::optional(
        "after",
        "Optional sequence number; only outbox bundles after it are returned.",
        ToolInputType::Integer,
    ),
];

const ACK_OUTBOX_INPUTS: &[ToolInputDescriptor] = &[
    ToolInputDescriptor::required(
        "worker_id",
        "Runtime worker identity whose outbox owns the message.",
        ToolInputType::String,
    ),
    ToolInputDescriptor::required(
        "sequence",
        "Outbox sequence number to acknowledge.",
        ToolInputType::Integer,
    ),
];

const CREATE_WORKER_INPUTS: &[ToolInputDescriptor] = &[
    ToolInputDescriptor::required(
        "perspective",
        "Compiled perspective to instantiate.",
        ToolInputType::String,
    ),
    ToolInputDescriptor::optional(
        "worker_id",
        "Optional orchestrator-selected runtime worker identity. When omitted, Multorum allocates the default perspective-based worker id.",
        ToolInputType::String,
    ),
    ToolInputDescriptor::optional(
        "overwriting_worktree",
        "Optional flag to replace an existing finalized workspace for the same explicit worker id.",
        ToolInputType::Boolean,
    ),
    ToolInputDescriptor::optional(
        "body",
        "Optional Markdown file to move into the seeded task bundle body.",
        ToolInputType::String,
    ),
    ToolInputDescriptor::optional(
        "artifacts",
        "Optional files to move into the seeded task bundle artifacts directory.",
        ToolInputType::StringList,
    ),
];

const FORWARD_PERSPECTIVE_INPUTS: &[ToolInputDescriptor] = &[ToolInputDescriptor::required(
    "perspective",
    "Perspective whose blocked bidding group should move to the active rulebook commit.",
    ToolInputType::String,
)];

const REPLY_BUNDLE_INPUTS: &[ToolInputDescriptor] = &[
    ToolInputDescriptor::required(
        "worker_id",
        "Runtime worker identity that owns the inbox.",
        ToolInputType::String,
    ),
    ToolInputDescriptor::optional(
        "reply_to",
        "Optional mailbox sequence number answered by this bundle.",
        ToolInputType::Integer,
    ),
    ToolInputDescriptor::optional(
        "body",
        "Optional Markdown file to move into the bundle body.",
        ToolInputType::String,
    ),
    ToolInputDescriptor::optional(
        "artifacts",
        "Optional files to move into the bundle artifacts directory.",
        ToolInputType::StringList,
    ),
];

const FINALIZED_WORKER_INPUTS: &[ToolInputDescriptor] = &[ToolInputDescriptor::required(
    "worker_id",
    "Runtime worker identity to act on.",
    ToolInputType::String,
)];

const MERGE_WORKER_INPUTS: &[ToolInputDescriptor] = &[
    ToolInputDescriptor::required(
        "worker_id",
        "Runtime worker identity to merge.",
        ToolInputType::String,
    ),
    ToolInputDescriptor::optional(
        "skip_checks",
        "Optional project-defined checks to skip based on trusted worker evidence.",
        ToolInputType::StringList,
    ),
    ToolInputDescriptor::optional(
        "body",
        "Optional Markdown file to move into the audit rationale body.",
        ToolInputType::String,
    ),
    ToolInputDescriptor::optional(
        "artifacts",
        "Optional files to move into the audit rationale artifacts directory.",
        ToolInputType::StringList,
    ),
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
            name: "forward_perspective",
            description: "Move one blocked bidding group to the active rulebook commit.",
            inputs: FORWARD_PERSPECTIVE_INPUTS,
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
