//! MCP tool descriptor registration.

pub mod orchestrator;
pub mod worker;

use crate::mcp::dto::{ToolInputDescriptor, ToolInputType};

/// Construct one required string tool input descriptor.
pub(crate) const fn required_string_input(
    name: &'static str, description: &'static str,
) -> ToolInputDescriptor {
    ToolInputDescriptor::required(name, description, ToolInputType::String)
}

/// Construct one optional string tool input descriptor.
pub(crate) const fn optional_string_input(
    name: &'static str, description: &'static str,
) -> ToolInputDescriptor {
    ToolInputDescriptor::optional(name, description, ToolInputType::String)
}

/// Construct one required integer tool input descriptor.
pub(crate) const fn required_integer_input(
    name: &'static str, description: &'static str,
) -> ToolInputDescriptor {
    ToolInputDescriptor::required(name, description, ToolInputType::Integer)
}

/// Construct one optional integer tool input descriptor.
pub(crate) const fn optional_integer_input(
    name: &'static str, description: &'static str,
) -> ToolInputDescriptor {
    ToolInputDescriptor::optional(name, description, ToolInputType::Integer)
}

/// Construct one optional boolean tool input descriptor.
pub(crate) const fn optional_boolean_input(
    name: &'static str, description: &'static str,
) -> ToolInputDescriptor {
    ToolInputDescriptor::optional(name, description, ToolInputType::Boolean)
}

/// Construct one required string-list tool input descriptor.
pub(crate) const fn required_string_list_input(
    name: &'static str, description: &'static str,
) -> ToolInputDescriptor {
    ToolInputDescriptor::required(name, description, ToolInputType::StringList)
}

/// Construct one optional string-list tool input descriptor.
pub(crate) const fn optional_string_list_input(
    name: &'static str, description: &'static str,
) -> ToolInputDescriptor {
    ToolInputDescriptor::optional(name, description, ToolInputType::StringList)
}

/// Shared tool input descriptor groups used by both MCP surfaces.
///
/// Note: MCP input ordering is part of the published schema surface, so
/// these slices keep the common field sequences in one place.
pub(crate) struct ToolInputSets;

impl ToolInputSets {
    /// Orchestrator inbox/outbox query fields.
    pub(crate) const ORCHESTRATOR_MAILBOX_READ: &'static [ToolInputDescriptor] = &[
        required_string_input("worker", "Runtime worker identity whose mailbox should be read."),
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

    /// Worker inbox/outbox query fields.
    pub(crate) const WORKER_MAILBOX_READ: &'static [ToolInputDescriptor] = &[
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

    /// Worker inbox acknowledgement fields.
    pub(crate) const WORKER_ACK_INBOX: &'static [ToolInputDescriptor] =
        &[required_integer_input("sequence", "Inbox sequence number to acknowledge.")];

    /// Orchestrator worker outbox acknowledgement fields.
    pub(crate) const ORCHESTRATOR_ACK_OUTBOX: &'static [ToolInputDescriptor] = &[
        required_string_input("worker", "Runtime worker identity whose outbox owns the message."),
        required_integer_input("sequence", "Outbox sequence number to acknowledge."),
    ];

    /// Worker task bundle fields for orchestrator worker creation.
    pub(crate) const ORCHESTRATOR_TASK_BUNDLE: &'static [ToolInputDescriptor] = &[
        required_string_input("perspective", "Compiled perspective to instantiate."),
        optional_string_input(
            "worker",
            "Optional orchestrator-selected runtime worker identity. When omitted, Multorum allocates a default perspective-based identity.",
        ),
        optional_boolean_input(
            "overwriting_worktree",
            "Optional flag to replace an existing finalized workspace for the same explicit worker.",
        ),
        optional_boolean_input(
            "no_auto_forward",
            "Disable auto-forward. When false (default), Multorum may forward the candidate group to HEAD before creation if all live workers are non-ACTIVE.",
        ),
        optional_string_input(
            "body_text",
            "Required when body_path is absent: inline Markdown content written into the created task bundle body.",
        ),
        optional_string_input(
            "body_path",
            "Required when body_text is absent: Markdown file to move into the created task bundle body.",
        ),
        optional_string_list_input(
            "artifacts",
            "Optional files to move into the created task bundle artifacts directory.",
        ),
    ];

    /// Orchestrator follow-up bundle fields for hint and revise.
    pub(crate) const ORCHESTRATOR_REPLY_BUNDLE: &'static [ToolInputDescriptor] = &[
        required_string_input("worker", "Runtime worker identity that owns the inbox."),
        optional_integer_input(
            "reply_to",
            "Optional mailbox sequence number answered by this bundle.",
        ),
        optional_string_input(
            "body_text",
            "Required when body_path is absent: inline Markdown content written into the bundle body.",
        ),
        optional_string_input(
            "body_path",
            "Required when body_text is absent: Markdown file to move into the bundle body.",
        ),
        optional_string_list_input(
            "artifacts",
            "Optional files to move into the bundle artifacts directory.",
        ),
    ];

    /// Orchestrator resolve bundle fields.
    pub(crate) const ORCHESTRATOR_RESOLVE_BUNDLE: &'static [ToolInputDescriptor] = &[
        required_string_input("worker", "Runtime worker identity that owns the inbox."),
        optional_boolean_input(
            "no_auto_forward",
            "Disable auto-forward. When false (default), Multorum may forward the candidate group to HEAD before resolving if all live workers are non-ACTIVE.",
        ),
        optional_integer_input(
            "reply_to",
            "Optional mailbox sequence number answered by this bundle.",
        ),
        optional_string_input(
            "body_text",
            "Required when body_path is absent: inline Markdown content written into the bundle body.",
        ),
        optional_string_input(
            "body_path",
            "Required when body_text is absent: Markdown file to move into the bundle body.",
        ),
        optional_string_list_input(
            "artifacts",
            "Optional files to move into the bundle artifacts directory.",
        ),
    ];

    /// Worker blocker report fields.
    pub(crate) const WORKER_REPORT: &'static [ToolInputDescriptor] = &[
        optional_string_input(
            "head_commit",
            "Git commit hash of the worker's current progress. Without this, perspective forward cannot verify the worktree and will reject the forward.",
        ),
        optional_integer_input(
            "reply_to",
            "Optional mailbox sequence number answered by this report.",
        ),
        optional_string_input(
            "body_text",
            "Required when body_path is absent: inline Markdown content written into the report body.",
        ),
        optional_string_input(
            "body_path",
            "Required when body_text is absent: Markdown file to move into the report body.",
        ),
        optional_string_list_input(
            "artifacts",
            "Optional files to move into the report artifacts directory.",
        ),
    ];

    /// Worker commit submission fields.
    pub(crate) const WORKER_COMMIT: &'static [ToolInputDescriptor] = &[
        required_string_input("head_commit", "Git commit hash submitted by the worker."),
        optional_string_input(
            "body_text",
            "Required when body_path is absent: inline Markdown content written into the commit bundle body.",
        ),
        optional_string_input(
            "body_path",
            "Required when body_text is absent: Markdown file to move into the commit bundle body.",
        ),
        optional_string_list_input(
            "artifacts",
            "Optional files to move into the commit bundle artifacts directory.",
        ),
    ];

    /// Orchestrator worker merge fields.
    pub(crate) const ORCHESTRATOR_MERGE: &'static [ToolInputDescriptor] = &[
        required_string_input("worker", "Runtime worker identity to merge."),
        optional_string_list_input(
            "skip_checks",
            "Project-defined checks to skip. Only checks marked 'skippable' in the rulebook are allowed; the write-set scope check is never skippable.",
        ),
        optional_string_input(
            "body_text",
            "Required when body_path is absent: inline Markdown content written into the audit rationale body. Prefer self-contained findings instead of references to worker outbox paths.",
        ),
        optional_string_input(
            "body_path",
            "Required when body_text is absent: Markdown file to move into the audit rationale body. Prefer self-contained findings instead of references to worker outbox paths.",
        ),
        optional_string_list_input(
            "artifacts",
            "Optional files to move into the audit rationale artifacts directory.",
        ),
    ];
}
