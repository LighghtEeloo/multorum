//! Mailbox-specific runtime types.

use serde::{Deserialize, Serialize};

use super::bundle::{MessageRef, Sequence};

/// Direction of a mailbox relative to the worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MailboxDirection {
    /// Messages authored by the orchestrator and consumed by the worker.
    Inbox,
    /// Messages authored by the worker and consumed by the orchestrator.
    Outbox,
}

/// Acknowledgement reference for a consumed mailbox bundle.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AckRef {
    /// Message being acknowledged.
    pub message: MessageRef,
    /// Acknowledged sequence number.
    pub sequence: Sequence,
}
