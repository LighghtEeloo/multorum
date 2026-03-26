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

use crate::bundle::BundleError;

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

    /// Validate that the payload is well-formed before Multorum starts
    /// moving any path-backed inputs into managed storage.
    ///
    /// Note: Merge-time audit staging calls this before canonical
    /// integration starts so deterministic bundle validation failures
    /// cannot leave the canonical branch partially updated.
    pub fn validate(&self) -> Result<(), BundleError> {
        let mut seen_artifact_names = std::collections::BTreeSet::new();

        match (&self.body_text, &self.body_path) {
            | (Some(_), Some(_)) => return Err(BundleError::ConflictingBody),
            | (_, Some(path)) => {
                if !path.is_file() {
                    return Err(BundleError::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("bundle body path is not a file: {}", path.display()),
                    )));
                }
            }
            | _ => {}
        }

        for artifact in &self.artifacts {
            let Some(name) = artifact.file_name() else {
                return Err(BundleError::InvalidArtifactPath);
            };
            if !seen_artifact_names.insert(name.to_owned()) {
                return Err(BundleError::DuplicateArtifactName);
            }
            if !artifact.is_file() {
                return Err(BundleError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("bundle artifact path is not a file: {}", artifact.display()),
                )));
            }
        }

        Ok(())
    }
}
