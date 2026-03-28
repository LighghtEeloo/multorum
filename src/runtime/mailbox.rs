//! Mailbox protocol types for orchestrator-worker communication.
//!
//! The mailbox protocol uses directory bundles (see [`crate::bundle`]) as
//! its transport unit, extending each bundle with an `envelope.toml` that
//! carries routing metadata. Types in this module describe the protocol
//! envelope, message classification, sequencing, acknowledgement, and
//! routing direction. The general bundle payload type lives in
//! [`crate::bundle::BundlePayload`].

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::schema::perspective::PerspectiveName;
use crate::vcs::CanonicalCommitHash;

use super::timestamp::Timestamp;
use super::worker_id::WorkerId;

/// Monotonic per-mailbox sequence number.
///
/// Note: Sequence numbers are local to a single mailbox direction for a
/// specific worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Sequence(pub u64);

/// Kind of message bundle recognized by the mailbox protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MessageKind {
    /// Initial task assignment published during worker creation.
    Task,
    /// Orchestrator hint for an active worker.
    ///
    /// Note: Hints are advisory follow-up context. They do not change
    /// worker lifecycle state on publication or acknowledgement.
    Hint,
    /// Worker blocker report.
    Report,
    /// Orchestrator response to a blocker.
    Resolve,
    /// Orchestrator revision request for a committed worker.
    Revise,
    /// Worker submission of a completed commit.
    Commit,
}

impl MessageKind {
    /// The storage slug for bundle directory names.
    ///
    /// Note: Mailbox bundles use stable directory names so they can be
    /// inspected directly from disk and safely referenced by tests.
    pub(crate) fn slug(self) -> &'static str {
        match self {
            | Self::Task => "task",
            | Self::Hint => "hint",
            | Self::Report => "report",
            | Self::Resolve => "resolve",
            | Self::Revise => "revise",
            | Self::Commit => "commit",
        }
    }
}

/// Stable reference to a published mailbox bundle.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageRef {
    /// Worker that owns the mailbox where the bundle was published.
    #[serde(rename = "worker")]
    pub worker_id: WorkerId,
    /// Kind of published bundle.
    pub kind: MessageKind,
    /// Monotonic mailbox-local sequence number.
    pub sequence: Sequence,
}

/// Reply metadata for bundles that answer an earlier message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplyReference {
    /// Sequence number of the message this bundle answers.
    pub in_reply_to: Option<Sequence>,
}

/// Mailbox protocol version tag.
///
/// Stored internally as an integer but serialized as `"multorum/v{n}"`
/// so the wire format is self-describing and forward-compatible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProtocolVersion(pub u32);

impl ProtocolVersion {
    /// Prefix used in the serialized representation.
    const PREFIX: &str = "multorum/v";
}

impl std::fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", Self::PREFIX, self.0)
    }
}

impl Serialize for ProtocolVersion {
    fn serialize<S: serde::Serializer>(
        &self, serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ProtocolVersion {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        let version_str = s.strip_prefix(Self::PREFIX).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "protocol version must start with {:?}, got {s:?}",
                Self::PREFIX,
            ))
        })?;
        let version: u32 = version_str.parse().map_err(|_| {
            serde::de::Error::custom(format!(
                "protocol version number must be a non-negative integer, got {version_str:?}",
            ))
        })?;
        Ok(Self(version))
    }
}

/// Envelope persisted in `envelope.toml` inside a mailbox bundle.
///
/// The envelope is the only file Multorum interprets inside a mailbox
/// bundle. `body.md` and `artifacts/` are opaque payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleEnvelope {
    /// Mailbox protocol version, serialized as `"multorum/v1"`.
    pub protocol: ProtocolVersion,
    /// Active worker identity.
    #[serde(rename = "worker")]
    pub worker_id: WorkerId,
    /// Perspective instantiated by the worker.
    pub perspective: PerspectiveName,
    /// Kind of bundle.
    pub kind: MessageKind,
    /// Monotonic mailbox-local sequence number.
    pub sequence: Sequence,
    /// Timestamp recorded by the publisher.
    pub created_at: Timestamp,
    /// Optional answered message sequence number.
    pub in_reply_to: Option<Sequence>,
    /// Optional canonical commit hash relevant to the message.
    pub head_commit: Option<CanonicalCommitHash>,
}

/// Result of publishing a mailbox bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishedBundle {
    /// Stable reference to the published bundle.
    pub message: MessageRef,
    /// Filesystem path to the published bundle directory.
    pub bundle_path: PathBuf,
}

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
}

/// Filter for mailbox message listing.
///
/// Either a half-open range (`from`/`to`, both optional) or an exact
/// match on a single sequence number. The two modes are mutually
/// exclusive — `exact` may not be combined with `from`/`to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceFilter {
    /// Return all messages whose sequence falls within
    /// `[from, to]` (inclusive on both ends). Either bound may be
    /// `None` for an unbounded side.
    Range {
        /// Inclusive lower bound. `None` means no lower bound.
        from: Option<Sequence>,
        /// Inclusive upper bound. `None` means no upper bound.
        to: Option<Sequence>,
    },
    /// Return exactly the message with this sequence number.
    Exact(Sequence),
}

impl Default for SequenceFilter {
    fn default() -> Self {
        Self::Range { from: None, to: None }
    }
}

impl SequenceFilter {
    /// Test whether a sequence passes this filter.
    pub fn matches(self, seq: Sequence) -> bool {
        match self {
            | Self::Range { from, to } => {
                if let Some(lo) = from {
                    if seq < lo {
                        return false;
                    }
                }
                if let Some(hi) = to {
                    if seq > hi {
                        return false;
                    }
                }
                true
            }
            | Self::Exact(target) => seq == target,
        }
    }
}
