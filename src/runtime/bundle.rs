//! Typed mailbox bundle metadata.
//!
//! Message bundles are the unit of orchestrator-worker communication in
//! Multorum. The filesystem layout is authoritative; these types capture
//! the stable metadata that both the runtime services and transport
//! adapters share.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::perspective::PerspectiveName;

/// Monotonic per-mailbox sequence number.
///
/// Note: Sequence numbers are local to a single mailbox direction for a
/// specific perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Sequence(pub u64);

/// Kind of message bundle recognized by Multorum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MessageKind {
    /// Initial task assignment published during provisioning.
    Task,
    /// Worker blocker report.
    Report,
    /// Orchestrator response to a blocker.
    Resolve,
    /// Orchestrator revision request for a committed worker.
    Revise,
    /// Worker submission of a completed commit.
    Commit,
}

/// Stable reference to a published bundle.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageRef {
    /// Perspective that owns the mailbox where the bundle was published.
    pub perspective: PerspectiveName,
    /// Kind of published bundle.
    pub kind: MessageKind,
    /// Monotonic mailbox-local sequence number.
    pub sequence: Sequence,
}

/// User-supplied content to place into a bundle.
///
/// `body_text` and `body_path` are mutually exclusive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BundlePayload {
    /// Inline Markdown content for `body.md`.
    pub body_text: Option<String>,
    /// Existing file to copy into `body.md`.
    pub body_path: Option<PathBuf>,
    /// Files to copy into `artifacts/`.
    pub artifacts: Vec<PathBuf>,
}

impl BundlePayload {
    /// Return `true` if the payload carries no body or artifacts.
    pub fn is_empty(&self) -> bool {
        self.body_text.is_none() && self.body_path.is_none() && self.artifacts.is_empty()
    }
}

/// Reply metadata for bundles that answer an earlier message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplyReference {
    /// Sequence number of the message this bundle answers.
    pub in_reply_to: Option<Sequence>,
}

/// Envelope persisted in `envelope.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleEnvelope {
    /// Mailbox protocol version.
    pub protocol: u32,
    /// Active worker identity.
    pub perspective: PerspectiveName,
    /// Kind of bundle.
    pub kind: MessageKind,
    /// Monotonic mailbox-local sequence number.
    pub sequence: Sequence,
    /// Timestamp recorded by the publisher.
    pub created_at: String,
    /// Optional answered message sequence number.
    pub in_reply_to: Option<Sequence>,
    /// Optional commit hash relevant to the message.
    pub head_commit: Option<String>,
}

/// Result of publishing a mailbox bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishedBundle {
    /// Stable reference to the published bundle.
    pub message: MessageRef,
    /// Filesystem path to the published bundle directory.
    pub bundle_path: PathBuf,
}
