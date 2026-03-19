//! Check pipeline declarations from the `[checks]` rulebook table.
//!
//! Checks are project-defined commands that run after the mandatory
//! file-set enforcement gate. The rulebook declares the ordered
//! pipeline and a named table for each check.

use std::collections::{BTreeMap, BTreeSet};
use std::{fmt, str::FromStr};

use serde::de;

use super::error::{CheckNameError, CheckValidationError};

/// A validated check identifier (e.g. `lint`, `build`, `test`).
///
/// ## Invariants
///
/// - Non-empty.
/// - Starts with a lowercase ASCII letter (`a`–`z`).
/// - Contains only ASCII alphanumeric characters (`a`–`z`, `A`–`Z`,
///   `0`–`9`).
///
/// Note: Check names use a separate type from file set and perspective
/// names because rulebook checks are command labels rather than
/// perspective identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CheckName(String);

impl CheckName {
    /// Create a new `CheckName`, validating the identifier invariants.
    pub fn new(s: &str) -> Result<Self, CheckNameError> {
        let first = s.chars().next().ok_or(CheckNameError::Empty)?;
        if !first.is_ascii_lowercase() {
            return Err(CheckNameError::InvalidStart { name: s.to_owned() });
        }
        for (pos, ch) in s.char_indices().skip(1) {
            if !ch.is_ascii_alphanumeric() {
                return Err(CheckNameError::InvalidChar { name: s.to_owned(), ch, pos });
            }
        }
        Ok(Self(s.to_owned()))
    }

    /// The identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for CheckName {
    type Err = CheckNameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl fmt::Display for CheckName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> de::Deserialize<'de> for CheckName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::new(&s).map_err(de::Error::custom)
    }
}

/// The skip policy for a user-defined check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckPolicy {
    /// The check always runs during integration.
    Always,
    /// The check may be skipped when the orchestrator accepts worker
    /// evidence for that specific integration.
    Skippable,
}

impl Default for CheckPolicy {
    fn default() -> Self {
        Self::Always
    }
}

/// One declared check from the rulebook.
///
/// `command` is intentionally kept as an opaque shell string in v1.
/// The runtime check executor will decide how to interpret it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckDecl {
    command: String,
    policy: CheckPolicy,
}

impl CheckDecl {
    /// Create a new check declaration.
    pub fn new(command: impl Into<String>, policy: CheckPolicy) -> Self {
        Self { command: command.into(), policy }
    }

    /// The raw command string.
    pub fn command(&self) -> &str {
        &self.command
    }

    /// The check policy.
    pub fn policy(&self) -> CheckPolicy {
        self.policy
    }
}

/// The complete set of check declarations from a rulebook's `[checks]`
/// table.
///
/// The table contains an ordered `pipeline` plus a named declaration
/// for every check referenced by that pipeline.
#[derive(Debug, Clone, Default)]
pub struct CheckTable {
    pipeline: Vec<CheckName>,
    declarations: BTreeMap<CheckName, CheckDecl>,
}

impl CheckTable {
    /// The ordered check pipeline as declared in the rulebook.
    pub fn pipeline(&self) -> &[CheckName] {
        &self.pipeline
    }

    /// The declared checks keyed by name.
    pub fn declarations(&self) -> &BTreeMap<CheckName, CheckDecl> {
        &self.declarations
    }

    /// Compile and validate the check pipeline.
    ///
    /// Validation ensures that every declared check appears exactly
    /// once in the pipeline and that every command string is non-empty
    /// after trimming.
    pub fn compile(&self) -> Result<CompiledChecks, CheckValidationError> {
        let mut seen = BTreeSet::new();
        for name in &self.pipeline {
            if !seen.insert(name.clone()) {
                return Err(CheckValidationError::DuplicatePipelineEntry { name: name.clone() });
            }
            if !self.declarations.contains_key(name) {
                return Err(CheckValidationError::UndefinedPipelineEntry { name: name.clone() });
            }
        }

        for (name, decl) in &self.declarations {
            if decl.command.trim().is_empty() {
                return Err(CheckValidationError::EmptyCommand { name: name.clone() });
            }
            if !seen.contains(name) {
                return Err(CheckValidationError::UnusedDeclaredCheck { name: name.clone() });
            }
        }

        tracing::debug!(check_count = self.declarations.len(), "validated rulebook checks");

        Ok(CompiledChecks {
            pipeline: self.pipeline.clone(),
            declarations: self.declarations.clone(),
        })
    }
}

/// A validated check pipeline.
///
/// ## Invariant
///
/// Every declared check appears exactly once in `pipeline`, every
/// pipeline entry exists in `declarations`, and every command string is
/// non-empty after trimming.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledChecks {
    pipeline: Vec<CheckName>,
    declarations: BTreeMap<CheckName, CheckDecl>,
}

impl CompiledChecks {
    /// The ordered pipeline.
    pub fn pipeline(&self) -> &[CheckName] {
        &self.pipeline
    }

    /// The validated declarations.
    pub fn declarations(&self) -> &BTreeMap<CheckName, CheckDecl> {
        &self.declarations
    }

    /// Look up one compiled check by name.
    pub fn get(&self, name: &CheckName) -> Option<&CheckDecl> {
        self.declarations.get(name)
    }

    /// The number of compiled checks.
    pub fn len(&self) -> usize {
        self.declarations.len()
    }

    /// Whether there are no compiled checks.
    pub fn is_empty(&self) -> bool {
        self.declarations.is_empty()
    }
}

#[derive(serde::Deserialize)]
struct RawCheckDecl {
    command: String,
    #[serde(default)]
    policy: CheckPolicy,
}

#[derive(serde::Deserialize)]
struct RawCheckTable {
    #[serde(default)]
    pipeline: Vec<String>,
    #[serde(flatten)]
    declarations: BTreeMap<String, RawCheckDecl>,
}

impl<'de> de::Deserialize<'de> for CheckTable {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let raw = RawCheckTable::deserialize(deserializer)?;

        let pipeline = raw
            .pipeline
            .into_iter()
            .map(|name| CheckName::new(&name).map_err(de::Error::custom))
            .collect::<Result<Vec<_>, _>>()?;

        let mut declarations = BTreeMap::new();
        for (name, decl) in raw.declarations {
            let name = CheckName::new(&name).map_err(de::Error::custom)?;
            declarations.insert(name, CheckDecl::new(decl.command, decl.policy));
        }

        Ok(Self { pipeline, declarations })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(s: &str) -> CheckName {
        CheckName::new(s).unwrap()
    }

    #[test]
    fn valid_check_name() {
        assert!(CheckName::new("lint").is_ok());
        assert!(CheckName::new("build2").is_ok());
    }

    #[test]
    fn uppercase_start_is_rejected() {
        let err = CheckName::new("Lint").unwrap_err();
        assert!(matches!(err, CheckNameError::InvalidStart { .. }));
    }

    #[test]
    fn policy_defaults_to_always() {
        let checks: CheckTable = toml::from_str(
            r#"
            pipeline = ["lint"]

            [lint]
            command = "cargo clippy"
        "#,
        )
        .unwrap();

        let compiled = checks.compile().unwrap();
        assert_eq!(compiled.get(&name("lint")).unwrap().policy(), CheckPolicy::Always);
    }

    #[test]
    fn skippable_policy_parses() {
        let checks: CheckTable = toml::from_str(
            r#"
            pipeline = ["test"]

            [test]
            command = "cargo test"
            policy = "skippable"
        "#,
        )
        .unwrap();

        let compiled = checks.compile().unwrap();
        assert_eq!(compiled.get(&name("test")).unwrap().policy(), CheckPolicy::Skippable);
    }

    #[test]
    fn undefined_pipeline_entry_is_rejected() {
        let checks: CheckTable = toml::from_str(
            r#"
            pipeline = ["lint"]
        "#,
        )
        .unwrap();

        let err = checks.compile().unwrap_err();
        assert_eq!(err, CheckValidationError::UndefinedPipelineEntry { name: name("lint") });
    }

    #[test]
    fn duplicate_pipeline_entry_is_rejected() {
        let checks: CheckTable = toml::from_str(
            r#"
            pipeline = ["lint", "lint"]

            [lint]
            command = "cargo clippy"
        "#,
        )
        .unwrap();

        let err = checks.compile().unwrap_err();
        assert_eq!(err, CheckValidationError::DuplicatePipelineEntry { name: name("lint") });
    }

    #[test]
    fn unused_declared_check_is_rejected() {
        let checks: CheckTable = toml::from_str(
            r#"
            pipeline = []

            [lint]
            command = "cargo clippy"
        "#,
        )
        .unwrap();

        let err = checks.compile().unwrap_err();
        assert_eq!(err, CheckValidationError::UnusedDeclaredCheck { name: name("lint") });
    }

    #[test]
    fn empty_command_is_rejected() {
        let checks: CheckTable = toml::from_str(
            r#"
            pipeline = ["lint"]

            [lint]
            command = "   "
        "#,
        )
        .unwrap();

        let err = checks.compile().unwrap_err();
        assert_eq!(err, CheckValidationError::EmptyCommand { name: name("lint") });
    }

    #[test]
    fn empty_table_is_valid() {
        let checks: CheckTable = toml::from_str("").unwrap();
        let compiled = checks.compile().unwrap();
        assert!(compiled.is_empty());
        assert!(compiled.pipeline().is_empty());
    }
}
