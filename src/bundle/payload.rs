//! General-purpose content bundle.
//!
//! A bundle is a directory containing a `body.md` primary content file
//! and an `artifacts/` subdirectory for supplementary files. Bundles
//! are the shared content container used by both the mailbox protocol
//! and audit entries — anywhere Multorum needs to atomically store
//! structured content with optional supplementary files.
//!
//! Path-backed payload inputs transfer ownership into Multorum. When a
//! bundle is published, the runtime moves the referenced body file and
//! artifact files into the bundle directory under `.multorum/` instead
//! of copying them.

use std::path::PathBuf;

/// User-supplied content to place into a bundle directory.
///
/// `body_text` and `body_path` are mutually exclusive.
///
/// Path-backed fields are consumed on successful publication. Multorum
/// moves those files into its managed `.multorum/` bundle storage so the
/// runtime, not the caller, becomes responsible for retaining them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BundlePayload {
    /// Inline Markdown content for `body.md`.
    pub body_text: Option<String>,
    /// Existing file to move into `body.md`.
    pub body_path: Option<PathBuf>,
    /// Existing files to move into `artifacts/`.
    pub artifacts: Vec<PathBuf>,
}

impl BundlePayload {
    /// Return `true` if the payload carries no body or artifacts.
    pub fn is_empty(&self) -> bool {
        self.body_text.is_none() && self.body_path.is_none() && self.artifacts.is_empty()
    }
}
