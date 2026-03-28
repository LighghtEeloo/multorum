//! Worker MCP tool surface.

use crate::mcp::dto::{ToolDescriptor, ToolInputDescriptor};

use super::{ToolInputSets, required_string_input};

const SET_WORKING_DIRECTORY_INPUTS: &[ToolInputDescriptor] =
    &[required_string_input("path", "Absolute path to the managed worker worktree root.")];

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
            inputs: ToolInputSets::WORKER_MAILBOX_READ,
        },
        ToolDescriptor {
            name: "read_outbox",
            description: "Read messages sent by this worker to the orchestrator, optionally filtering to bundles after a given sequence number.",
            inputs: ToolInputSets::WORKER_MAILBOX_READ,
        },
        ToolDescriptor {
            name: "ack_inbox_message",
            description: "Acknowledge a message received from the orchestrator, marking the inbox bundle as consumed.",
            inputs: ToolInputSets::WORKER_ACK_INBOX,
        },
        ToolDescriptor {
            name: "send_report",
            description: "Send a blocker report to the orchestrator, signaling that the worker needs input before continuing; path-backed payload files are moved into .multorum storage.",
            inputs: ToolInputSets::WORKER_REPORT,
        },
        ToolDescriptor {
            name: "send_commit",
            description: "Send a completed submission to the orchestrator for review; path-backed payload files are moved into .multorum storage.",
            inputs: ToolInputSets::WORKER_COMMIT,
        },
        ToolDescriptor {
            name: "get_status",
            description: "Return the worker status projection.",
            inputs: &[],
        },
    ]
}
