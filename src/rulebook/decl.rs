//! Raw rulebook declarations and loading helpers.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::fileset::FileSetTable;
use crate::perspective::PerspectiveTable;

use super::check::CheckTable;
use super::error::RulebookError;

/// Canonical relative path to the committed project rulebook.
pub const RULEBOOK_RELATIVE_PATH: &str = ".multorum/rulebook.toml";

/// Checked-in default rulebook template used by `rulebook init`.
///
/// Note: This template stays in `src/rulebook.default.toml` so design
/// edits do not require touching Rust string literals.
pub const DEFAULT_RULEBOOK_TEMPLATE: &str = include_str!("../rulebook.default.toml");

/// The raw `.multorum/rulebook.toml` artifact.
///
/// A rulebook is the single committed configuration file that defines
/// file sets, perspectives, and the pre-merge check pipeline for a
/// Multorum project.
#[derive(Debug, Clone, Deserialize)]
pub struct Rulebook {
    #[serde(default)]
    filesets: FileSetTable,
    #[serde(default)]
    perspectives: PerspectiveTable,
    #[serde(default)]
    checks: CheckTable,
}

impl Rulebook {
    /// Return the default rulebook template used during initialization.
    pub fn default_template() -> &'static str {
        DEFAULT_RULEBOOK_TEMPLATE
    }

    /// Parse a rulebook from a TOML string.
    pub fn from_toml_str(input: &str) -> Result<Self, RulebookError> {
        tracing::debug!(bytes = input.len(), "decoding rulebook from string");
        Ok(toml::from_str(input)?)
    }

    /// Parse a rulebook from TOML bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, RulebookError> {
        let input = std::str::from_utf8(bytes)?;
        Self::from_toml_str(input)
    }

    /// Read and parse a rulebook from a filesystem path.
    pub fn from_path(path: &Path) -> Result<Self, RulebookError> {
        tracing::debug!(path = %path.display(), "loading rulebook from path");
        let contents = fs::read(path)?;
        Self::from_bytes(&contents)
    }

    /// Read the canonical `.multorum/rulebook.toml` under a workspace root.
    pub fn from_workspace_root(workspace_root: &Path) -> Result<Self, RulebookError> {
        Self::from_path(&Self::rulebook_path(workspace_root))
    }

    /// Return the canonical rulebook path for a workspace root.
    pub fn rulebook_path(workspace_root: &Path) -> PathBuf {
        workspace_root.join(RULEBOOK_RELATIVE_PATH)
    }

    /// The raw file set declarations.
    pub fn filesets(&self) -> &FileSetTable {
        &self.filesets
    }

    /// The raw perspective declarations.
    pub fn perspectives(&self) -> &PerspectiveTable {
        &self.perspectives
    }

    /// The raw check declarations.
    pub fn checks(&self) -> &CheckTable {
        &self.checks
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::rulebook::CheckPolicy;

    #[test]
    fn full_rulebook_deserializes() {
        let rulebook = Rulebook::from_toml_str(
            r#"
            [filesets]
            SpecFiles.path = "**/*.spec.md"
            TestFiles.path = "**/test/**"
            AuthFiles.path = "auth/**"
            AuthSpecs = "AuthFiles & SpecFiles"
            AuthTests = "AuthFiles & TestFiles"

            [perspectives.AuthImplementor]
            read  = "AuthSpecs"
            write = "AuthFiles - AuthSpecs - AuthTests"

            [perspectives.AuthTester]
            read  = "AuthSpecs"
            write = "AuthTests"

            [checks]
            pipeline = ["lint", "test"]

            [checks.lint]
            command = "cargo clippy"

            [checks.test]
            command = "cargo test"
            policy = "skippable"
        "#,
        )
        .unwrap();

        assert_eq!(rulebook.filesets().definitions().len(), 5);
        assert_eq!(rulebook.perspectives().declarations().len(), 2);
        assert_eq!(rulebook.checks().pipeline().len(), 2);
        assert_eq!(
            rulebook.checks().declarations()[&crate::rulebook::CheckName::new("test").unwrap()]
                .policy(),
            CheckPolicy::Skippable
        );
    }

    #[test]
    fn rulebook_loads_from_workspace_root() {
        let dir = tempdir().unwrap();
        let rulebook_path = Rulebook::rulebook_path(dir.path());
        fs::create_dir_all(rulebook_path.parent().unwrap()).unwrap();
        fs::write(
            &rulebook_path,
            r#"
            [checks]
            pipeline = []
        "#,
        )
        .unwrap();

        let rulebook = Rulebook::from_workspace_root(dir.path()).unwrap();
        assert!(rulebook.filesets().definitions().is_empty());
        assert!(rulebook.perspectives().declarations().is_empty());
        assert!(rulebook.checks().pipeline().is_empty());
    }

    #[test]
    fn default_template_is_a_valid_empty_rulebook() {
        let rulebook = Rulebook::from_toml_str(Rulebook::default_template()).unwrap();

        assert!(rulebook.filesets().definitions().is_empty());
        assert!(rulebook.perspectives().declarations().is_empty());
        assert!(rulebook.checks().pipeline().is_empty());
    }
}
