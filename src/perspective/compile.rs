//! Compilation of perspective declarations into concrete file sets.
//!
//! Takes a [`PerspectiveTable`] and a pre-compiled file set map
//! (`BTreeMap<fileset::Name, BTreeSet<PathBuf>>`), evaluates each
//! perspective's read and write expressions, then validates the
//! safety property. The result is a [`CompiledPerspectives`] that
//! is guaranteed to satisfy the safety invariant.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::fileset;

use super::decl::PerspectiveTable;
use super::error::PerspectiveError;
use super::name::PerspectiveName;
use super::safety::SafetyValidator;

/// A single perspective compiled into concrete file sets.
#[derive(Debug, Clone)]
pub struct CompiledPerspective {
    read: BTreeSet<PathBuf>,
    write: BTreeSet<PathBuf>,
}

impl CompiledPerspective {
    /// The concrete read set.
    pub fn read(&self) -> &BTreeSet<PathBuf> {
        &self.read
    }

    /// The concrete write set.
    pub fn write(&self) -> &BTreeSet<PathBuf> {
        &self.write
    }
}

/// A set of compiled perspectives that satisfies the safety property.
///
/// ## Invariant
///
/// For any two distinct perspectives P and Q:
/// - `write(P) ∩ write(Q) = ∅`
/// - `write(P) ∩ read(Q) = ∅`
///
/// This invariant is established by [`PerspectiveTable::compile`]
/// and cannot be violated through the public API.
#[derive(Debug, Clone)]
pub struct CompiledPerspectives {
    inner: BTreeMap<PerspectiveName, CompiledPerspective>,
}

impl CompiledPerspectives {
    /// The compiled perspectives.
    pub fn perspectives(
        &self,
    ) -> &BTreeMap<PerspectiveName, CompiledPerspective> {
        &self.inner
    }

    /// Look up a compiled perspective by name.
    pub fn get(&self, name: &PerspectiveName) -> Option<&CompiledPerspective> {
        self.inner.get(name)
    }

    /// The number of perspectives.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Whether there are no perspectives.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl PerspectiveTable {
    /// Compile all perspective declarations against pre-compiled file
    /// sets and validate the safety property.
    ///
    /// `compiled_filesets` is the output of
    /// [`FileSetTable::compile`](crate::fileset::FileSetTable::compile).
    ///
    /// Returns [`CompiledPerspectives`] on success, which is
    /// guaranteed to satisfy the safety property.
    pub fn compile(
        &self,
        compiled_filesets: &BTreeMap<fileset::Name, BTreeSet<PathBuf>>,
    ) -> Result<CompiledPerspectives, PerspectiveError> {
        let mut compiled = BTreeMap::new();

        for (name, decl) in self.declarations() {
            let read = fileset::Compiler::evaluate(
                decl.read(),
                compiled_filesets,
            );
            let write = fileset::Compiler::evaluate(
                decl.write(),
                compiled_filesets,
            );

            if write.is_empty() {
                tracing::warn!(
                    perspective = %name,
                    "perspective write set compiled to empty list"
                );
            }

            compiled.insert(name.clone(), CompiledPerspective { read, write });
        }

        SafetyValidator::new(&compiled).validate()?;

        Ok(CompiledPerspectives { inner: compiled })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fileset;

    /// Build compiled file sets from the design doc example.
    fn design_doc_filesets(
    ) -> BTreeMap<fileset::Name, BTreeSet<PathBuf>> {
        let files: Vec<PathBuf> = [
            "auth/login.rs",
            "auth/logout.rs",
            "auth/auth.spec.md",
            "auth/test/login_test.rs",
            "api/handler.rs",
            "api/api.spec.md",
            "api/test/api_test.rs",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();

        let toml_str = r#"
            SpecFiles.path = "**/*.spec.md"
            TestFiles.path = "**/test/**"
            AuthFiles.path = "auth/**"
            AuthSpecs = "AuthFiles & SpecFiles"
            AuthTests = "AuthFiles & TestFiles"
        "#;
        let table: fileset::FileSetTable = toml::from_str(toml_str).unwrap();
        table.compile(&files).unwrap()
    }

    fn path_set(strs: &[&str]) -> BTreeSet<PathBuf> {
        strs.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn two_disjoint_perspectives_compile() {
        let filesets = design_doc_filesets();
        // AuthImplementor reads only specs (not tests), so
        // AuthTester's write set does not overlap its read set.
        let toml_str = r#"
            [AuthImplementor]
            read  = "AuthSpecs"
            write = "AuthFiles - AuthSpecs - AuthTests"

            [AuthTester]
            read  = "AuthSpecs"
            write = "AuthTests"
        "#;
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        let compiled = table.compile(&filesets).unwrap();

        assert_eq!(compiled.len(), 2);

        let impl_p = compiled
            .get(&PerspectiveName::new("AuthImplementor").unwrap())
            .unwrap();
        assert_eq!(
            *impl_p.write(),
            path_set(&["auth/login.rs", "auth/logout.rs"])
        );
        assert_eq!(
            *impl_p.read(),
            path_set(&["auth/auth.spec.md"])
        );

        let test_p = compiled
            .get(&PerspectiveName::new("AuthTester").unwrap())
            .unwrap();
        assert_eq!(
            *test_p.write(),
            path_set(&["auth/test/login_test.rs"])
        );
    }

    #[test]
    fn write_write_overlap_detected() {
        let filesets = design_doc_filesets();
        // Both perspectives write to AuthFiles (overlapping).
        let toml_str = r#"
            [P]
            read  = "SpecFiles"
            write = "AuthFiles"

            [Q]
            read  = "SpecFiles"
            write = "AuthFiles"
        "#;
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        let err = table.compile(&filesets).unwrap_err();
        assert!(
            matches!(err, PerspectiveError::Safety(
                super::super::SafetyViolation::WriteWriteOverlap { .. }
            )),
            "expected WriteWriteOverlap, got: {err:?}"
        );
    }

    #[test]
    fn write_read_overlap_detected() {
        let filesets = design_doc_filesets();
        // P writes AuthTests, Q reads AuthTests — disjoint writes
        // but write-read overlap.
        let toml_str = r#"
            [P]
            read  = "SpecFiles"
            write = "AuthTests"

            [Q]
            read  = "AuthTests"
            write = "SpecFiles"
        "#;
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        let err = table.compile(&filesets).unwrap_err();
        assert!(
            matches!(err, PerspectiveError::Safety(
                super::super::SafetyViolation::WriteReadOverlap { .. }
            )),
            "expected WriteReadOverlap, got: {err:?}"
        );
    }

    #[test]
    fn empty_table_is_valid() {
        let filesets = design_doc_filesets();
        let toml_str = "";
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        let compiled = table.compile(&filesets).unwrap();
        assert!(compiled.is_empty());
    }

    #[test]
    fn single_perspective_always_valid() {
        let filesets = design_doc_filesets();
        let toml_str = r#"
            [Solo]
            read  = "SpecFiles"
            write = "AuthFiles"
        "#;
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        let compiled = table.compile(&filesets).unwrap();
        assert_eq!(compiled.len(), 1);
    }

    #[test]
    fn shared_reads_are_allowed() {
        let filesets = design_doc_filesets();
        // Both perspectives read AuthSpecs, write to disjoint sets
        // that do not overlap with AuthSpecs.
        let toml_str = r#"
            [P]
            read  = "AuthSpecs"
            write = "AuthTests"

            [Q]
            read  = "AuthSpecs"
            write = "AuthFiles - AuthSpecs - AuthTests"
        "#;
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        let compiled = table.compile(&filesets).unwrap();
        assert_eq!(compiled.len(), 2);
    }
}
