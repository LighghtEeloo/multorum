//! Worker MCP tool surface.

use crate::mcp::dto::{ToolDescriptor, ToolInputDescriptor};

use super::{
    optional_integer_input, optional_string_input, optional_string_list_input,
    required_integer_input, required_string_input,
};

const SET_WORKING_DIRECTORY_INPUTS: &[ToolInputDescriptor] = &[required_string_input(
    "path",
    "Absolute path to the managed worker worktree root.",
)];

const READ_INBOX_INPUTS: &[ToolInputDescriptor] = &[optional_integer_input(
    "after",
    "Optional sequence number; only inbox bundles after it are returned.",
)];

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
            description: "List inbox bundles after an optional sequence number.",
            inputs: READ_INBOX_INPUTS,
        },
        ToolDescriptor {
            name: "ack_inbox_message",
            description: "Acknowledge a consumed inbox bundle.",
            inputs: ACK_INPUTS,
        },
        ToolDescriptor {
            name: "send_report",
            description: "Publish a worker blocker report bundle to the outbox; path-backed payload files are moved into .multorum storage.",
            inputs: REPORT_INPUTS,
        },
        ToolDescriptor {
            name: "send_commit",
            description: "Publish a completed worker submission bundle to the outbox; path-backed payload files are moved into .multorum storage.",
            inputs: COMMIT_INPUTS,
        },
        ToolDescriptor {
            name: "get_status",
            description: "Return the worker status projection.",
            inputs: &[],
        },
    ]
}
