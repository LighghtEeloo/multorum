//! Orchestrator MCP tool surface.

use crate::mcp::dto::{ToolDescriptor, ToolInputDescriptor};

use super::{
    optional_boolean_input, optional_integer_input, optional_string_input,
    optional_string_list_input, required_integer_input, required_string_input,
    required_string_list_input,
};

const SET_WORKING_DIRECTORY_INPUTS: &[ToolInputDescriptor] = &[required_string_input(
    "path",
    "Absolute path to the Multorum workspace root.",
)];

const GET_WORKER_INPUTS: &[ToolInputDescriptor] =
    &[required_string_input("worker", "Runtime worker identity to inspect.")];

const READ_OUTBOX_INPUTS: &[ToolInputDescriptor] = &[
    required_string_input("worker", "Runtime worker identity whose outbox should be read."),
    optional_integer_input(
        "from",
        "Inclusive lower-bound sequence number. Mutually exclusive with exact.",
    ),
    optional_integer_input(
        "to",
        "Inclusive upper-bound sequence number. Mutually exclusive with exact.",
    ),
    optional_integer_input(
        "exact",
        "Return exactly one message by sequence number. Mutually exclusive with from/to.",
    ),
    optional_boolean_input(
        "include_body",
        "Include full body.md content for each returned message.",
    ),
];

const READ_INBOX_INPUTS: &[ToolInputDescriptor] = &[
    required_string_input("worker", "Runtime worker identity whose inbox should be read."),
    optional_integer_input(
        "from",
        "Inclusive lower-bound sequence number. Mutually exclusive with exact.",
    ),
    optional_integer_input(
        "to",
        "Inclusive upper-bound sequence number. Mutually exclusive with exact.",
    ),
    optional_integer_input(
        "exact",
        "Return exactly one message by sequence number. Mutually exclusive with from/to.",
    ),
    optional_boolean_input(
        "include_body",
        "Include full body.md content for each returned message.",
    ),
];

const ACK_OUTBOX_INPUTS: &[ToolInputDescriptor] = &[
    required_string_input("worker", "Runtime worker identity whose outbox owns the message."),
    required_integer_input("sequence", "Outbox sequence number to acknowledge."),
];

const CREATE_WORKER_INPUTS: &[ToolInputDescriptor] = &[
    required_string_input("perspective", "Compiled perspective to instantiate."),
    optional_string_input(
        "worker",
        "Optional orchestrator-selected runtime worker identity. When omitted, Multorum allocates a default perspective-based identity.",
    ),
    optional_boolean_input(
        "overwriting_worktree",
        "Optional flag to replace an existing finalized workspace for the same explicit worker.",
    ),
    optional_string_input(
        "body_text",
        "Optional inline Markdown content written into the created task bundle body.",
    ),
    optional_string_input(
        "body_path",
        "Optional Markdown file to move into the created task bundle body.",
    ),
    optional_string_list_input(
        "artifacts",
        "Optional files to move into the created task bundle artifacts directory.",
    ),
];

const VALIDATE_PERSPECTIVES_INPUTS: &[ToolInputDescriptor] = &[
    required_string_list_input(
        "perspectives",
        "Perspective names to validate for conflict-freedom.",
    ),
    optional_boolean_input("no_live", "Skip checking against active bidding groups."),
];

const FORWARD_PERSPECTIVE_INPUTS: &[ToolInputDescriptor] = &[required_string_input(
    "perspective",
    "Perspective whose blocked bidding group should move to HEAD.",
)];

const REPLY_BUNDLE_INPUTS: &[ToolInputDescriptor] = &[
    required_string_input("worker", "Runtime worker identity that owns the inbox."),
    optional_integer_input("reply_to", "Optional mailbox sequence number answered by this bundle."),
    optional_string_input(
        "body_text",
        "Optional inline Markdown content written into the bundle body.",
    ),
    optional_string_input("body_path", "Optional Markdown file to move into the bundle body."),
    optional_string_list_input(
        "artifacts",
        "Optional files to move into the bundle artifacts directory.",
    ),
];

const FINALIZED_WORKER_INPUTS: &[ToolInputDescriptor] =
    &[required_string_input("worker", "Runtime worker identity to act on.")];

const MERGE_WORKER_INPUTS: &[ToolInputDescriptor] = &[
    required_string_input("worker", "Runtime worker identity to merge."),
    optional_string_list_input(
        "skip_checks",
        "Optional project-defined checks to skip based on trusted worker evidence.",
    ),
    optional_string_input(
        "body_text",
        "Optional inline Markdown content written into the audit rationale body. Prefer self-contained findings instead of references to worker outbox paths.",
    ),
    optional_string_input(
        "body_path",
        "Optional Markdown file to move into the audit rationale body. Prefer self-contained findings instead of references to worker outbox paths.",
    ),
    optional_string_list_input(
        "artifacts",
        "Optional files to move into the audit rationale artifacts directory.",
    ),
];

/// Return the orchestrator MCP tool descriptors.
pub fn descriptors() -> Vec<ToolDescriptor> {
    vec![
        ToolDescriptor {
            name: "set_working_directory",
            description: "Rebind the orchestrator server to a different workspace root directory. The server defaults to the process working directory at startup.",
            inputs: SET_WORKING_DIRECTORY_INPUTS,
        },
        ToolDescriptor {
            name: "rulebook_init",
            description: "Initialize .multorum with the default committed rulebook artifacts.",
            inputs: &[],
        },
        ToolDescriptor {
            name: "list_perspectives",
            description: "List compiled perspectives from the current rulebook.",
            inputs: &[],
        },
        ToolDescriptor {
            name: "validate_perspectives",
            description: "Validate a set of perspectives for conflict-freedom against each other and active bidding groups.",
            inputs: VALIDATE_PERSPECTIVES_INPUTS,
        },
        ToolDescriptor {
            name: "forward_perspective",
            description: "Move one blocked bidding group to HEAD.",
            inputs: FORWARD_PERSPECTIVE_INPUTS,
        },
        ToolDescriptor {
            name: "list_workers",
            description: "List active workers in the current runtime.",
            inputs: &[],
        },
        ToolDescriptor {
            name: "get_worker",
            description: "Load one worker detail view.",
            inputs: GET_WORKER_INPUTS,
        },
        ToolDescriptor {
            name: "read_worker_outbox",
            description: "Read messages sent by a worker to the orchestrator, optionally filtering to bundles after a given sequence number.",
            inputs: READ_OUTBOX_INPUTS,
        },
        ToolDescriptor {
            name: "read_worker_inbox",
            description: "Read messages sent by the orchestrator to a worker, optionally filtering to bundles after a given sequence number.",
            inputs: READ_INBOX_INPUTS,
        },
        ToolDescriptor {
            name: "ack_worker_outbox_message",
            description: "Acknowledge a message received from a worker, marking the outbox bundle as consumed.",
            inputs: ACK_OUTBOX_INPUTS,
        },
        ToolDescriptor {
            name: "create_worker",
            description: "Create a worker workspace and send it an initial task bundle; path-backed payload files are moved into .multorum storage.",
            inputs: CREATE_WORKER_INPUTS,
        },
        ToolDescriptor {
            name: "resolve_worker",
            description: "Send a resolve bundle to a blocked worker, unblocking it to continue work; path-backed payload files are moved into .multorum storage.",
            inputs: REPLY_BUNDLE_INPUTS,
        },
        ToolDescriptor {
            name: "hint_worker",
            description: "Send an advisory hint bundle to an active worker without changing its lifecycle state; path-backed payload files are moved into .multorum storage.",
            inputs: REPLY_BUNDLE_INPUTS,
        },
        ToolDescriptor {
            name: "revise_worker",
            description: "Send a revision request bundle to a committed worker, returning it to active state; path-backed payload files are moved into .multorum storage.",
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
            description: "Run the pre-merge pipeline and merge a worker submission. Audit rationale should be self-contained because worker runtime state may be deleted after merge confirmation.",
            inputs: MERGE_WORKER_INPUTS,
        },
        ToolDescriptor {
            name: "get_status",
            description: "Return the full orchestrator runtime snapshot.",
            inputs: &[],
        },
    ]
}
