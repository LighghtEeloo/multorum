//! Worker MCP tool surface.

use crate::mcp::dto::{ToolDescriptor, ToolInputDescriptor, ToolInputType};

const READ_INBOX_INPUTS: &[ToolInputDescriptor] = &[ToolInputDescriptor::optional(
    "after",
    "Optional sequence number; only inbox bundles after it are returned.",
    ToolInputType::Integer,
)];

const ACK_INPUTS: &[ToolInputDescriptor] = &[ToolInputDescriptor::required(
    "sequence",
    "Inbox sequence number to acknowledge.",
    ToolInputType::Integer,
)];

const REPORT_INPUTS: &[ToolInputDescriptor] = &[
    ToolInputDescriptor::optional(
        "head_commit",
        "Optional git commit hash relevant to the blocker report.",
        ToolInputType::String,
    ),
    ToolInputDescriptor::optional(
        "reply_to",
        "Optional mailbox sequence number answered by this report.",
        ToolInputType::Integer,
    ),
    ToolInputDescriptor::optional(
        "body_text",
        "Optional inline Markdown content written into the report body.",
        ToolInputType::String,
    ),
    ToolInputDescriptor::optional(
        "body_path",
        "Optional Markdown file to move into the report body.",
        ToolInputType::String,
    ),
    ToolInputDescriptor::optional(
        "artifacts",
        "Optional files to move into the report artifacts directory.",
        ToolInputType::StringList,
    ),
];

const COMMIT_INPUTS: &[ToolInputDescriptor] = &[
    ToolInputDescriptor::required(
        "head_commit",
        "Git commit hash submitted by the worker.",
        ToolInputType::String,
    ),
    ToolInputDescriptor::optional(
        "body_text",
        "Optional inline Markdown content written into the commit bundle body.",
        ToolInputType::String,
    ),
    ToolInputDescriptor::optional(
        "body_path",
        "Optional Markdown file to move into the commit bundle body.",
        ToolInputType::String,
    ),
    ToolInputDescriptor::optional(
        "artifacts",
        "Optional files to move into the commit bundle artifacts directory.",
        ToolInputType::StringList,
    ),
];

/// Return the worker MCP tool descriptors.
pub fn descriptors() -> Vec<ToolDescriptor> {
    vec![
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
