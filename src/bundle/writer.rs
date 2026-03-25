//! Bundle filesystem I/O.
//!
//! `BundleWriter` materializes a [`BundlePayload`] into a target
//! directory, producing a `body.md` and moving artifacts into
//! `artifacts/`. The writer validates payload invariants and handles
//! cross-device file moves transparently.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use super::error::BundleError;
use super::payload::BundlePayload;

/// Canonical body file name within a bundle directory.
pub const BODY_FILE_NAME: &str = "body.md";
/// Canonical artifacts subdirectory name within a bundle directory.
pub const ARTIFACTS_DIR_NAME: &str = "artifacts";

/// Result of writing a bundle payload into a target directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenBundle {
    /// Path to the body file if content was written.
    pub body_path: Option<PathBuf>,
    /// Destination paths of moved artifacts.
    pub artifact_paths: Vec<PathBuf>,
}

/// Namespace for bundle filesystem operations.
///
/// Note: `BundleWriter` is a zero-sized namespace struct. Bundle I/O is
/// stateless — all context is passed through method arguments.
pub struct BundleWriter;

impl BundleWriter {
    /// Write a complete bundle into `target_dir`.
    ///
    /// Creates `body.md` from the payload body (if present) and moves
    /// artifacts into `artifacts/`. Returns the paths of written files.
    ///
    /// The caller must ensure `target_dir` exists before calling this
    /// method. When no body content is supplied, `body.md` is not
    /// created and `body_path` in the result is `None`.
    pub fn write(target_dir: &Path, payload: BundlePayload) -> Result<WrittenBundle, BundleError> {
        let body_target = target_dir.join(BODY_FILE_NAME);
        let body_path =
            if Self::write_body(&body_target, payload.body_text, payload.body_path)? {
                Some(body_target)
            } else {
                None
            };

        let artifact_paths = if !payload.artifacts.is_empty() {
            let artifacts_dir = target_dir.join(ARTIFACTS_DIR_NAME);
            fs::create_dir_all(&artifacts_dir)?;
            Self::move_artifacts(&artifacts_dir, payload.artifacts)?
        } else {
            Vec::new()
        };

        Ok(WrittenBundle { body_path, artifact_paths })
    }

    /// Write the body content of a bundle.
    ///
    /// Returns `true` when content was written. When both `body_text`
    /// and `body_path` are `None`, no file is created and the method
    /// returns `Ok(false)`. Callers that require `body.md` to always
    /// exist (e.g. mailbox bundles) should write an empty file when
    /// this returns `false`.
    pub fn write_body(
        target: &Path, body_text: Option<String>, body_path: Option<PathBuf>,
    ) -> Result<bool, BundleError> {
        match (body_text, body_path) {
            | (Some(text), None) => {
                fs::write(target, text)?;
                Ok(true)
            }
            | (None, Some(path)) => {
                move_file(&path, target)?;
                Ok(true)
            }
            | (None, None) => Ok(false),
            | (Some(_), Some(_)) => Err(BundleError::ConflictingBody),
        }
    }

    /// Move artifact files into a bundle's artifacts directory.
    ///
    /// Returns the destination paths of the moved artifacts. The caller
    /// is responsible for creating `target_dir` before calling this
    /// method.
    pub fn move_artifacts(
        target_dir: &Path, artifacts: Vec<PathBuf>,
    ) -> Result<Vec<PathBuf>, BundleError> {
        let mut seen = BTreeSet::new();
        let mut destinations = Vec::new();
        for artifact in artifacts {
            let Some(name) = artifact.file_name() else {
                return Err(BundleError::InvalidArtifactPath);
            };
            let name: OsString = name.to_owned();
            if !seen.insert(name.clone()) {
                return Err(BundleError::DuplicateArtifactName);
            }
            let dest = target_dir.join(&name);
            move_file(&artifact, &dest)?;
            destinations.push(dest);
        }
        Ok(destinations)
    }
}

/// Move a file into a runtime-managed location.
///
/// Falls back to copy-then-delete when the source and target reside on
/// different filesystems.
pub fn move_file(source: &Path, target: &Path) -> Result<(), BundleError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    match fs::rename(source, target) {
        | Ok(()) => Ok(()),
        | Err(error) if error.kind() == std::io::ErrorKind::CrossesDevices => {
            fs::copy(source, target)?;
            fs::remove_file(source)?;
            Ok(())
        }
        | Err(error) => Err(BundleError::Io(error)),
    }
}
