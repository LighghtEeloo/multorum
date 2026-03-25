//! Bundle content errors.
//!
//! `BundleError` captures failures that originate in the bundle I/O
//! layer. The runtime wraps it as `RuntimeError::Bundle` so higher
//! layers never import this module directly.

use thiserror::Error;

/// Errors produced by the bundle content layer.
#[derive(Debug, Error)]
pub enum BundleError {
    /// The caller supplied both inline text and a file path for the body.
    #[error("body_text and body_path are mutually exclusive")]
    ConflictingBody,

    /// An artifact path does not name a file.
    #[error("artifact path must name a file")]
    InvalidArtifactPath,

    /// Two artifacts share the same file name.
    #[error("artifact names must be unique")]
    DuplicateArtifactName,

    /// Filesystem I/O failure within the bundle layer.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
