//! Compilation of perspective declarations into concrete file sets.
//!
//! Takes a [`PerspectiveTable`] and a pre-compiled file set map
//! (`BTreeMap<fileset::Name, BTreeSet<PathBuf>>`), evaluates each
//! perspective's read and write expressions, and returns the concrete
//! file lists that runtime conflict checks later consume.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::schema::fileset::{self, Expr};

use super::decl::PerspectiveTable;
use super::error::PerspectiveError;
use super::name::PerspectiveName;
/// A single perspective compiled into concrete file sets.
#[derive(Debug, Clone)]
pub struct CompiledPerspective {
    read: BTreeSet<PathBuf>,
    write: BTreeSet<PathBuf>,
}

impl CompiledPerspective {
    /// Construct one compiled perspective from materialized sets.
    ///
    /// Note: Runtime code uses this when it reconstructs active
    /// candidate-group boundaries from worker-local read/write-set files.
    pub fn from_materialized_sets(read: BTreeSet<PathBuf>, write: BTreeSet<PathBuf>) -> Self {
        Self { read, write }
    }

    /// The concrete read set.
    pub fn read(&self) -> &BTreeSet<PathBuf> {
        &self.read
    }

    /// The concrete write set.
    pub fn write(&self) -> &BTreeSet<PathBuf> {
        &self.write
    }
}

/// A set of compiled perspectives keyed by declaration name.
#[derive(Debug, Clone)]
pub struct CompiledPerspectives {
    inner: BTreeMap<PerspectiveName, CompiledPerspective>,
}

impl CompiledPerspectives {
    /// The compiled perspectives.
    pub fn perspectives(&self) -> &BTreeMap<PerspectiveName, CompiledPerspective> {
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
    /// sets.
    ///
    /// `compiled_filesets` is the output of
    /// [`FileSetTable::compile`](crate::schema::fileset::FileSetTable::compile).
    ///
    /// Returns [`CompiledPerspectives`] on success.
    pub fn compile(
        &self, compiled_filesets: &BTreeMap<fileset::Name, BTreeSet<PathBuf>>,
    ) -> Result<CompiledPerspectives, PerspectiveError> {
        self.validate_fileset_references(compiled_filesets)?;

        let mut compiled = BTreeMap::new();

        for (name, decl) in self.declarations() {
            let read = decl
                .read()
                .map(|expr| fileset::Compiler::evaluate(expr, compiled_filesets))
                .unwrap_or_default();
            let write = decl
                .write()
                .map(|expr| fileset::Compiler::evaluate(expr, compiled_filesets))
                .unwrap_or_default();

            if write.is_empty() {
                tracing::warn!(
                    perspective = %name,
                    "perspective write set compiled to empty list"
                );
            }

            compiled.insert(name.clone(), CompiledPerspective { read, write });
        }

        Ok(CompiledPerspectives { inner: compiled })
    }

    /// Validate that every read/write expression references a defined
    /// compiled file set before evaluation.
    fn validate_fileset_references(
        &self, compiled_filesets: &BTreeMap<fileset::Name, BTreeSet<PathBuf>>,
    ) -> Result<(), PerspectiveError> {
        for (perspective, decl) in self.declarations() {
            if let Some(expr) = decl.read() {
                Self::validate_expr_references(perspective, expr, compiled_filesets)?;
            }
            if let Some(expr) = decl.write() {
                Self::validate_expr_references(perspective, expr, compiled_filesets)?;
            }
        }
        Ok(())
    }

    fn validate_expr_references(
        perspective: &PerspectiveName, expr: &Expr,
        compiled_filesets: &BTreeMap<fileset::Name, BTreeSet<PathBuf>>,
    ) -> Result<(), PerspectiveError> {
        match expr {
            | Expr::Ref(name) => {
                if compiled_filesets.contains_key(name) {
                    Ok(())
                } else {
                    Err(PerspectiveError::UndefinedFileSet {
                        perspective: perspective.clone(),
                        name: name.clone(),
                    })
                }
            }
            | Expr::Union(left, right)
            | Expr::Intersection(left, right)
            | Expr::Difference(left, right) => {
                Self::validate_expr_references(perspective, left, compiled_filesets)?;
                Self::validate_expr_references(perspective, right, compiled_filesets)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::fileset;

    /// Build compiled file sets from the design doc example.
    fn design_doc_filesets() -> BTreeMap<fileset::Name, BTreeSet<PathBuf>> {
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
            SpecFiles.glob = "**/*.spec.md"
            TestFiles.glob = "**/test/**"
            AuthFiles.glob = "auth/**"
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

        let impl_p = compiled.get(&PerspectiveName::new("AuthImplementor").unwrap()).unwrap();
        assert_eq!(*impl_p.write(), path_set(&["auth/login.rs", "auth/logout.rs"]));
        assert_eq!(*impl_p.read(), path_set(&["auth/auth.spec.md"]));

        let test_p = compiled.get(&PerspectiveName::new("AuthTester").unwrap()).unwrap();
        assert_eq!(*test_p.write(), path_set(&["auth/test/login_test.rs"]));
    }

    #[test]
    fn overlapping_perspectives_still_compile() {
        let filesets = design_doc_filesets();
        // Runtime code, not compilation, decides whether these may be
        // active at the same time.
        let toml_str = r#"
            [P]
            read  = "SpecFiles"
            write = "AuthFiles"

            [Q]
            read  = "SpecFiles"
            write = "AuthFiles"
        "#;
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        let compiled = table.compile(&filesets).unwrap();
        assert_eq!(compiled.len(), 2);
    }

    #[test]
    fn write_read_overlap_still_compiles() {
        let filesets = design_doc_filesets();
        // Runtime code, not compilation, decides whether these may be
        // active at the same time.
        let toml_str = r#"
            [P]
            read  = "SpecFiles"
            write = "AuthTests"

            [Q]
            read  = "AuthTests"
            write = "SpecFiles"
        "#;
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        let compiled = table.compile(&filesets).unwrap();
        assert_eq!(compiled.len(), 2);
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

    #[test]
    fn empty_read_compiles_to_empty_set() {
        let filesets = design_doc_filesets();
        let toml_str = r#"
            [WriteOnly]
            write = "AuthFiles"
        "#;
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        let compiled = table.compile(&filesets).unwrap();
        let p = compiled.get(&PerspectiveName::new("WriteOnly").unwrap()).unwrap();
        assert!(p.read().is_empty());
        assert!(!p.write().is_empty());
    }

    #[test]
    fn empty_write_compiles_to_empty_set() {
        let filesets = design_doc_filesets();
        let toml_str = r#"
            [ReadOnly]
            read = "AuthSpecs"
        "#;
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        let compiled = table.compile(&filesets).unwrap();
        let p = compiled.get(&PerspectiveName::new("ReadOnly").unwrap()).unwrap();
        assert!(!p.read().is_empty());
        assert!(p.write().is_empty());
    }

    #[test]
    fn both_empty_compiles_to_empty_sets() {
        let filesets = design_doc_filesets();
        let toml_str = r#"
            [Bare]
        "#;
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        let compiled = table.compile(&filesets).unwrap();
        let p = compiled.get(&PerspectiveName::new("Bare").unwrap()).unwrap();
        assert!(p.read().is_empty());
        assert!(p.write().is_empty());
    }

    #[test]
    fn undefined_fileset_reference_is_rejected() {
        let filesets = design_doc_filesets();
        let toml_str = r#"
            [P]
            read  = "UnknownFiles"
            write = "AuthFiles"
        "#;
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        let err = table.compile(&filesets).unwrap_err();

        assert!(matches!(err, PerspectiveError::UndefinedFileSet { .. }));
    }
}
