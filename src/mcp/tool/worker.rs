//! Worker MCP tool surface.

use crate::mcp::dto::{ToolDescriptor, ToolInputDescriptor, ToolInputType};

const READ_INBOX_INPUTS: &[ToolInputDescriptor] = &[ToolInputDescriptor {
    name: "after",
    description: "Optional sequence number; only inbox bundles after it are returned.",
    kind: ToolInputType::Integer,
    required: false,
}];

const ACK_INPUTS: &[ToolInputDescriptor] = &[ToolInputDescriptor {
    name: "sequence",
    description: "Inbox sequence number to acknowledge.",
    kind: ToolInputType::Integer,
    required: true,
}];

const REPORT_INPUTS: &[ToolInputDescriptor] = &[
    ToolInputDescriptor {
        name: "head_commit",
        description: "Optional git commit hash relevant to the blocker report.",
        kind: ToolInputType::String,
        required: false,
    },
    ToolInputDescriptor {
        name: "reply_to",
        description: "Optional mailbox sequence number answered by this report.",
        kind: ToolInputType::Integer,
        required: false,
    },
    ToolInputDescriptor {
        name: "body",
        description: "Optional Markdown file to move into the report body.",
        kind: ToolInputType::String,
        required: false,
    },
    ToolInputDescriptor {
        name: "artifacts",
        description: "Optional files to move into the report artifacts directory.",
        kind: ToolInputType::StringList,
        required: false,
    },
];

const COMMIT_INPUTS: &[ToolInputDescriptor] = &[
    ToolInputDescriptor {
        name: "head_commit",
        description: "Git commit hash submitted by the worker.",
        kind: ToolInputType::String,
        required: true,
    },
    ToolInputDescriptor {
        name: "body",
        description: "Optional Markdown file to move into the commit bundle body.",
        kind: ToolInputType::String,
        required: false,
    },
    ToolInputDescriptor {
        name: "artifacts",
        description: "Optional files to move into the commit bundle artifacts directory.",
        kind: ToolInputType::StringList,
        required: false,
    },
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
