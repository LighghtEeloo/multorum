//! Projection helpers for derived runtime views.
//!
//! The runtime design treats mailbox bundles and orchestrator control
//! plane files as the authoritative data sources. Projection types group
//! normalized views that can be regenerated from those sources.

use serde::Serialize;

use super::state::MailboxMessageView;

/// Ordered transcript view for a worker interaction history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TranscriptView {
    /// Messages in logical transcript order.
    pub messages: Vec<MailboxMessageView>,
}

impl TranscriptView {
    /// Construct an empty transcript.
    pub fn empty() -> Self {
        Self { messages: Vec::new() }
    }
}
