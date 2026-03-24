//! Aggregate compilation of a rulebook into runtime-ready structures.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::fileset::{self, enumerate_files};
use crate::perspective::CompiledPerspectives;
use crate::runtime::PerspectiveSummary;

use super::check::CompiledChecks;
use super::decl::Rulebook;
use super::error::RulebookError;

/// A fully compiled rulebook.
///
/// ## Invariant
///
/// - `filesets` is the compiled output of the raw file set algebra.
/// - `perspectives` contains concrete read/write sets for each declared perspective.
/// - `check` has already passed pipeline validation.
#[derive(Debug, Clone)]
pub struct CompiledRulebook {
    filesets: BTreeMap<fileset::Name, BTreeSet<PathBuf>>,
    perspectives: CompiledPerspectives,
    check: CompiledChecks,
}

impl CompiledRulebook {
    /// The compiled file sets keyed by file set name.
    pub fn filesets(&self) -> &BTreeMap<fileset::Name, BTreeSet<PathBuf>> {
        &self.filesets
    }

    /// The compiled perspectives.
    pub fn perspectives(&self) -> &CompiledPerspectives {
        &self.perspectives
    }

    /// The validated check pipeline.
    pub fn check(&self) -> &CompiledChecks {
        &self.check
    }

    /// Build runtime-friendly summaries of the compiled perspectives.
    pub fn perspective_summaries(&self) -> Vec<PerspectiveSummary> {
        self.perspectives
            .perspectives()
            .iter()
            .map(|(name, perspective)| PerspectiveSummary {
                name: name.clone(),
                read_count: perspective.read().len(),
                write_count: perspective.write().len(),
            })
            .collect()
    }
}

impl Rulebook {
    /// Compile this rulebook against an explicit project file list.
    pub fn compile(&self, files: &[PathBuf]) -> Result<CompiledRulebook, RulebookError> {
        tracing::trace!(file_count = files.len(), "compiling rulebook");

        let check = self.check().compile()?;
        let filesets = self.fileset().compile(files)?;
        let perspectives = self.perspective().compile(&filesets)?;

        tracing::trace!(
            fileset_count = filesets.len(),
            perspective_count = perspectives.len(),
            check_count = check.len(),
            "compiled rulebook"
        );

        Ok(CompiledRulebook { filesets, perspectives, check })
    }

    /// Compile this rulebook by enumerating files under a project root.
    pub fn compile_for_root(&self, root: &Path) -> Result<CompiledRulebook, RulebookError> {
        tracing::trace!(root = %root.display(), "enumerating files for rulebook compilation");
        let files = enumerate_files(root).map_err(fileset::FileSetError::from)?;
        self.compile(&files)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;
    use crate::rulebook::{CheckName, CheckPolicy, Rulebook};

    fn design_rulebook() -> Rulebook {
        Rulebook::from_toml_str(
            r#"
            [fileset]
            SpecFiles.path = "**/*.spec.md"
            TestFiles.path = "**/test/**"
            AuthFiles.path = "auth/**"
            AuthSpecs = "AuthFiles & SpecFiles"
            AuthTests = "AuthFiles & TestFiles"

            [perspective.AuthImplementor]
            read  = "AuthSpecs"
            write = "AuthFiles - AuthSpecs - AuthTests"

            [perspective.AuthTester]
            read  = "AuthSpecs"
            write = "AuthTests"

            [check]
            pipeline = ["lint", "test"]

            [check.command]
            lint = "cargo clippy"
            test = "cargo test"

            [check.policy]
            test = "skippable"
        "#,
        )
        .unwrap()
    }

    fn design_files() -> Vec<PathBuf> {
        [
            "auth/login.rs",
            "auth/logout.rs",
            "auth/auth.spec.md",
            "auth/test/login_test.rs",
            "api/handler.rs",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect()
    }

    #[test]
    fn compile_with_explicit_file_list() {
        let compiled = design_rulebook().compile(&design_files()).unwrap();

        assert_eq!(compiled.filesets().len(), 5);
        assert_eq!(compiled.perspectives().len(), 2);
        assert_eq!(compiled.check().len(), 2);
        assert_eq!(
            compiled.check().get(&CheckName::new("test").unwrap()).unwrap().policy(),
            CheckPolicy::Skippable
        );
    }

    #[test]
    fn compile_for_root_uses_enumerated_files() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("auth/test")).unwrap();
        fs::write(dir.path().join("auth/login.rs"), "").unwrap();
        fs::write(dir.path().join("auth/logout.rs"), "").unwrap();
        fs::write(dir.path().join("auth/auth.spec.md"), "").unwrap();
        fs::write(dir.path().join("auth/test/login_test.rs"), "").unwrap();
        fs::write(dir.path().join("api.txt"), "").unwrap();

        let compiled = design_rulebook().compile_for_root(dir.path()).unwrap();
        let summaries = compiled.perspective_summaries();

        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].name.as_str(), "AuthImplementor");
        assert_eq!(summaries[0].read_count, 1);
        assert_eq!(summaries[0].write_count, 2);
        assert_eq!(summaries[1].name.as_str(), "AuthTester");
        assert_eq!(summaries[1].write_count, 1);
    }

    #[test]
    fn compile_surfaces_fileset_validation_failures() {
        let rulebook = Rulebook::from_toml_str(
            r#"
            [fileset]
            Broken = "MissingFiles"

            [check]
            pipeline = []
        "#,
        )
        .unwrap();

        let err = rulebook.compile(&[]).unwrap_err();
        assert!(matches!(err, RulebookError::FileSet(_)));
    }

    #[test]
    fn compile_allows_overlapping_perspectives() {
        let rulebook = Rulebook::from_toml_str(
            r#"
            [fileset]
            SpecFiles.path = "**/*.spec.md"
            AuthFiles.path = "auth/**"

            [perspective.P]
            read  = "SpecFiles"
            write = "AuthFiles"

            [perspective.Q]
            read  = "SpecFiles"
            write = "AuthFiles"

            [check]
            pipeline = []
        "#,
        )
        .unwrap();

        let compiled = rulebook.compile(&design_files()).unwrap();
        assert_eq!(compiled.perspectives().len(), 2);
    }

    #[test]
    fn perspective_summaries_match_compiled_sets() {
        let compiled = design_rulebook().compile(&design_files()).unwrap();
        let summaries = compiled.perspective_summaries();

        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].name.as_str(), "AuthImplementor");
        assert_eq!(summaries[0].read_count, 1);
        assert_eq!(summaries[0].write_count, 2);
        assert_eq!(summaries[1].name.as_str(), "AuthTester");
        assert_eq!(summaries[1].read_count, 1);
        assert_eq!(summaries[1].write_count, 1);
    }
}
