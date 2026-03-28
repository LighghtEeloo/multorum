//! Worker MCP tool surface.

use crate::mcp::dto::{ToolDescriptor, ToolInputDescriptor};

use super::{
    optional_boolean_input, optional_integer_input, optional_string_input,
    optional_string_list_input, required_integer_input, required_string_input,
};

const SET_WORKING_DIRECTORY_INPUTS: &[ToolInputDescriptor] = &[required_string_input(
    "path",
    "Absolute path to the managed worker worktree root.",
)];

const READ_INBOX_INPUTS: &[ToolInputDescriptor] = &[
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

const READ_OUTBOX_INPUTS: &[ToolInputDescriptor] = &[
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

const ACK_INPUTS: &[ToolInputDescriptor] =
    &[required_integer_input("sequence", "Inbox sequence number to acknowledge.")];

const REPORT_INPUTS: &[ToolInputDescriptor] = &[
    optional_string_input(
        "head_commit",
        "Optional git commit hash relevant to the blocker report.",
    ),
    optional_integer_input("reply_to", "Optional mailbox sequence number answered by this report."),
    optional_string_input(
        "body_text",
        "Optional inline Markdown content written into the report body.",
    ),
    optional_string_input("body_path", "Optional Markdown file to move into the report body."),
    optional_string_list_input(
        "artifacts",
        "Optional files to move into the report artifacts directory.",
    ),
];

const COMMIT_INPUTS: &[ToolInputDescriptor] = &[
    required_string_input("head_commit", "Git commit hash submitted by the worker."),
    optional_string_input(
        "body_text",
        "Optional inline Markdown content written into the commit bundle body.",
    ),
    optional_string_input(
        "body_path",
        "Optional Markdown file to move into the commit bundle body.",
    ),
    optional_string_list_input(
        "artifacts",
        "Optional files to move into the commit bundle artifacts directory.",
    ),
];

/// Return the worker MCP tool descriptors.
pub fn descriptors() -> Vec<ToolDescriptor> {
    vec![
        ToolDescriptor {
            name: "set_working_directory",
            description: "Rebind the worker server to a different worktree root directory. The server defaults to the process working directory at startup.",
            inputs: SET_WORKING_DIRECTORY_INPUTS,
        },
        ToolDescriptor {
            name: "get_contract",
            description: "Load the worker contract view.",
            inputs: &[],
        },
        ToolDescriptor {
            name: "read_inbox",
            description: "Read messages sent by the orchestrator to this worker, optionally filtering to bundles after a given sequence number.",
            inputs: READ_INBOX_INPUTS,
        },
        ToolDescriptor {
            name: "read_outbox",
            description: "Read messages sent by this worker to the orchestrator, optionally filtering to bundles after a given sequence number.",
            inputs: READ_OUTBOX_INPUTS,
        },
        ToolDescriptor {
            name: "ack_inbox_message",
            description: "Acknowledge a message received from the orchestrator, marking the inbox bundle as consumed.",
            inputs: ACK_INPUTS,
        },
        ToolDescriptor {
            name: "send_report",
            description: "Send a blocker report to the orchestrator, signaling that the worker needs input before continuing; path-backed payload files are moved into .multorum storage.",
            inputs: REPORT_INPUTS,
        },
        ToolDescriptor {
            name: "send_commit",
            description: "Send a completed submission to the orchestrator for review; path-backed payload files are moved into .multorum storage.",
            inputs: COMMIT_INPUTS,
        },
        ToolDescriptor {
            name: "get_status",
            description: "Return the worker status projection.",
            inputs: &[],
        },
    ]
}
