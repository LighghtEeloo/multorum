//! Compilation of file set definitions into concrete file lists.
//!
//! Takes validated definitions (with a topological ordering) and a
//! pre-enumerated list of file paths, then expands globs and evaluates
//! set operations to produce a `BTreeMap<Name, BTreeSet<PathBuf>>`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use wax::Program as _;
use wax::walk::{Entry as _, PathExt as _};

use super::error::CompileError;
use super::expr::{Definition, DirectoryPath, Expr, GlobPattern};
use super::name::Name;

/// Compiles file set definitions into concrete file lists.
///
/// The compiler takes a pre-enumerated list of relative file paths
/// (the caller is responsible for walking the filesystem) and
/// evaluates each definition in topological order.
///
/// - *Primitive* definitions match their glob pattern against the
///   file list.
/// - *Compound* definitions evaluate their expression using
///   already-resolved sets.
pub struct Compiler<'a> {
    files: &'a [PathBuf],
}

impl<'a> Compiler<'a> {
    /// Create a compiler with the given file list.
    ///
    /// Paths should be relative to the project root (the same root
    /// that glob patterns are written against).
    pub fn new(files: &'a [PathBuf]) -> Self {
        Self { files }
    }

    /// Compile all definitions into concrete file sets.
    ///
    /// Uses a three-phase pipeline:
    ///
    /// 1. **Resolve opaques**: collect every file whose path starts with
    ///    each opaque directory prefix.
    /// 2. **Build reduced file list**: files not claimed by any opaque
    ///    definition, for use by glob expansion.
    /// 3. **Resolve globs and compounds**: expand globs against only the
    ///    reduced file list; evaluate compound expressions against
    ///    already-resolved sets.
    ///
    /// `order` must be a valid topological ordering as produced by
    /// [`Validator::validate`](super::Validator::validate).
    pub fn compile(
        &self, definitions: &BTreeMap<Name, Definition>, order: &[Name],
    ) -> Result<BTreeMap<Name, BTreeSet<PathBuf>>, CompileError> {
        let mut resolved: BTreeMap<Name, BTreeSet<PathBuf>> = BTreeMap::new();

        // Phase 1: resolve opaques.
        let mut opaque_claimed: BTreeSet<PathBuf> = BTreeSet::new();
        for name in order {
            if let Some(Definition::Opaque(dir)) = definitions.get(name) {
                let set = self.expand_opaque(dir);
                opaque_claimed.extend(set.iter().cloned());
                if set.is_empty() {
                    tracing::warn!(name = %name, "opaque file set compiled to empty list");
                }
                resolved.insert(name.clone(), set);
            }
        }

        // Phase 2: build the reduced file list (files not claimed by any opaque).
        let reduced_files: Vec<PathBuf> =
            self.files.iter().filter(|p| !opaque_claimed.contains(*p)).cloned().collect();

        // Phase 3: resolve globs and compounds against reduced files.
        for name in order {
            let def = definitions.get(name).expect("topological order contains only defined names");
            let set = match def {
                | Definition::Opaque(_) => continue,
                | Definition::Primitive(pattern) => Self::expand_glob(&reduced_files, pattern)?,
                | Definition::Compound(expr) => Self::evaluate(expr, &resolved),
            };
            if set.is_empty() {
                tracing::warn!(name = %name, "file set compiled to empty list");
            }
            resolved.insert(name.clone(), set);
        }

        Ok(resolved)
    }

    /// Collect every file whose path starts with the opaque directory prefix.
    fn expand_opaque(&self, dir: &DirectoryPath) -> BTreeSet<PathBuf> {
        self.files
            .iter()
            .filter(|path| path.to_str().is_some_and(|s| s.starts_with(dir.as_str())))
            .cloned()
            .collect()
    }

    /// Expand a glob pattern against a file list.
    fn expand_glob(
        files: &[PathBuf], pattern: &GlobPattern,
    ) -> Result<BTreeSet<PathBuf>, CompileError> {
        let glob = wax::Glob::new(pattern.as_str()).map_err(|err| CompileError::Glob {
            pattern: pattern.as_str().to_owned(),
            reason: err.to_string(),
        })?;
        let matched = files.iter().filter(|path| glob.is_match(path.as_path())).cloned().collect();
        Ok(matched)
    }

    /// Recursively evaluate an expression against already-resolved sets.
    ///
    /// This is also used by the perspective module to resolve read/write
    /// expressions against compiled file sets.
    pub fn evaluate(
        expr: &Expr, resolved: &BTreeMap<Name, BTreeSet<PathBuf>>,
    ) -> BTreeSet<PathBuf> {
        match expr {
            | Expr::Ref(name) => {
                resolved.get(name).expect("topological order guarantees resolved").clone()
            }
            | Expr::Union(a, b) => {
                let mut result = Self::evaluate(a, resolved);
                result.extend(Self::evaluate(b, resolved));
                result
            }
            | Expr::Intersection(a, b) => {
                let set_a = Self::evaluate(a, resolved);
                let set_b = Self::evaluate(b, resolved);
                set_a.intersection(&set_b).cloned().collect()
            }
            | Expr::Difference(a, b) => {
                let set_a = Self::evaluate(a, resolved);
                let set_b = Self::evaluate(b, resolved);
                set_a.difference(&set_b).cloned().collect()
            }
        }
    }
}

/// Walk a directory tree and collect all file paths relative to `root`.
///
/// This is a convenience function for callers that want to enumerate
/// the filesystem before compiling.
///
/// Uses [`wax::walk::PathExt`] so traversal and glob matching rely on
/// the same path semantics.
///
/// Note: if `root` names a regular file instead of a directory, the
/// returned relative path is empty because it is relative to the walked
/// file itself.
pub fn enumerate_files(root: &Path) -> Result<Vec<PathBuf>, CompileError> {
    let mut files = Vec::new();
    for entry in root.walk() {
        let entry = entry
            .map_err(|err| CompileError::Walk { root: root.to_owned(), reason: err.to_string() })?;
        if entry.file_type().is_file() {
            let (_, relative) = entry.root_relative_paths();
            files.push(relative.to_owned());
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::schema::fileset::{DirectoryPath, ExprParser, Validator};

    fn n(s: &str) -> Name {
        Name::new(s).unwrap()
    }

    fn prim(pattern: &str) -> Definition {
        Definition::Primitive(GlobPattern::new(pattern).unwrap())
    }

    fn opaque(path: &str) -> Definition {
        Definition::Opaque(DirectoryPath::new(path).unwrap())
    }

    fn compound(expr: &str) -> Definition {
        Definition::Compound(ExprParser::new(expr).parse().unwrap())
    }

    fn paths(strs: &[&str]) -> Vec<PathBuf> {
        strs.iter().map(PathBuf::from).collect()
    }

    fn path_set(strs: &[&str]) -> BTreeSet<PathBuf> {
        strs.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn primitive_glob_matching() {
        let files = paths(&["auth/login.rs", "auth/logout.rs", "api/handler.rs"]);
        let defs = BTreeMap::from([(n("AuthFiles"), prim("auth/**"))]);
        let order = Validator::new(&defs).validate().unwrap();
        let result = Compiler::new(&files).compile(&defs, &order).unwrap();

        assert_eq!(result[&n("AuthFiles")], path_set(&["auth/login.rs", "auth/logout.rs"]));
    }

    #[test]
    fn union_operation() {
        let files = paths(&["a/x.rs", "b/y.rs", "c/z.rs"]);
        let defs = BTreeMap::from([
            (n("A"), prim("a/**")),
            (n("B"), prim("b/**")),
            (n("AB"), compound("A | B")),
        ]);
        let order = Validator::new(&defs).validate().unwrap();
        let result = Compiler::new(&files).compile(&defs, &order).unwrap();

        assert_eq!(result[&n("AB")], path_set(&["a/x.rs", "b/y.rs"]));
    }

    #[test]
    fn intersection_operation() {
        let files = paths(&["auth/login.rs", "auth/test/login_test.rs", "api/test/api_test.rs"]);
        let defs = BTreeMap::from([
            (n("AuthFiles"), prim("auth/**")),
            (n("TestFiles"), prim("**/test/**")),
            (n("AuthTests"), compound("AuthFiles & TestFiles")),
        ]);
        let order = Validator::new(&defs).validate().unwrap();
        let result = Compiler::new(&files).compile(&defs, &order).unwrap();

        assert_eq!(result[&n("AuthTests")], path_set(&["auth/test/login_test.rs"]));
    }

    #[test]
    fn difference_operation() {
        let files = paths(&["auth/login.rs", "auth/test/login_test.rs"]);
        let defs = BTreeMap::from([
            (n("AuthFiles"), prim("auth/**")),
            (n("TestFiles"), prim("**/test/**")),
            (n("AuthImpl"), compound("AuthFiles - TestFiles")),
        ]);
        let order = Validator::new(&defs).validate().unwrap();
        let result = Compiler::new(&files).compile(&defs, &order).unwrap();

        assert_eq!(result[&n("AuthImpl")], path_set(&["auth/login.rs"]));
    }

    #[test]
    fn opaque_collects_prefix_files() {
        let files = paths(&["vendor/lib/a.rs", "vendor/lib/b.rs", "src/main.rs"]);
        let defs = BTreeMap::from([(n("Vendor"), opaque("vendor/lib"))]);
        let order = Validator::new(&defs).validate().unwrap();
        let result = Compiler::new(&files).compile(&defs, &order).unwrap();
        assert_eq!(result[&n("Vendor")], path_set(&["vendor/lib/a.rs", "vendor/lib/b.rs"]));
    }

    #[test]
    fn glob_excludes_opaque_files() {
        let files = paths(&["vendor/lib/a.rs", "src/main.rs", "src/util.rs"]);
        let defs =
            BTreeMap::from([(n("Vendor"), opaque("vendor/")), (n("AllRust"), prim("**/*.rs"))]);
        let order = Validator::new(&defs).validate().unwrap();
        let result = Compiler::new(&files).compile(&defs, &order).unwrap();
        assert_eq!(result[&n("Vendor")], path_set(&["vendor/lib/a.rs"]));
        assert_eq!(result[&n("AllRust")], path_set(&["src/main.rs", "src/util.rs"]));
    }

    #[test]
    fn opaque_in_compound_expression() {
        let files = paths(&["vendor/a.rs", "src/main.rs"]);
        let defs = BTreeMap::from([
            (n("Vendor"), opaque("vendor/")),
            (n("Src"), prim("src/**")),
            (n("All"), compound("Vendor | Src")),
        ]);
        let order = Validator::new(&defs).validate().unwrap();
        let result = Compiler::new(&files).compile(&defs, &order).unwrap();
        assert_eq!(result[&n("All")], path_set(&["vendor/a.rs", "src/main.rs"]));
    }

    #[test]
    fn opaque_prefix_boundary_does_not_capture_similar_names() {
        let files = paths(&["vendor/a.rs", "vendorized/b.rs", "vendor-lib/c.rs", "src/main.rs"]);
        let defs =
            BTreeMap::from([(n("Vendor"), opaque("vendor")), (n("AllRust"), prim("**/*.rs"))]);
        let order = Validator::new(&defs).validate().unwrap();
        let result = Compiler::new(&files).compile(&defs, &order).unwrap();

        assert_eq!(result[&n("Vendor")], path_set(&["vendor/a.rs"]));
        assert_eq!(
            result[&n("AllRust")],
            path_set(&["src/main.rs", "vendor-lib/c.rs", "vendorized/b.rs"])
        );
    }

    #[test]
    fn opaque_glob_intersection_is_empty_by_construction() {
        let files = paths(&["vendor/a.rs", "vendor/b.rs", "src/main.rs"]);
        let defs = BTreeMap::from([
            (n("Vendor"), opaque("vendor/")),
            (n("Rust"), prim("**/*.rs")),
            (n("VendorRust"), compound("Vendor & Rust")),
        ]);
        let order = Validator::new(&defs).validate().unwrap();
        let result = Compiler::new(&files).compile(&defs, &order).unwrap();

        assert!(result[&n("VendorRust")].is_empty());
    }

    #[test]
    fn unmatched_opaque_keeps_glob_results_intact() {
        let files = paths(&["vendor/a.rs", "src/main.rs"]);
        let defs =
            BTreeMap::from([(n("Missing"), opaque("third_party/")), (n("Rust"), prim("**/*.rs"))]);
        let order = Validator::new(&defs).validate().unwrap();
        let result = Compiler::new(&files).compile(&defs, &order).unwrap();

        assert!(result[&n("Missing")].is_empty());
        assert_eq!(result[&n("Rust")], path_set(&["src/main.rs", "vendor/a.rs"]));
    }

    #[test]
    fn design_doc_example() {
        let files = paths(&[
            "auth/login.rs",
            "auth/logout.rs",
            "auth/auth.spec.md",
            "auth/test/login_test.rs",
            "api/handler.rs",
            "api/api.spec.md",
            "api/test/api_test.rs",
        ]);
        let defs = BTreeMap::from([
            (n("SpecFiles"), prim("**/*.spec.md")),
            (n("TestFiles"), prim("**/test/**")),
            (n("AuthFiles"), prim("auth/**")),
            (n("AuthSpecs"), compound("AuthFiles & SpecFiles")),
            (n("AuthTests"), compound("AuthFiles & TestFiles")),
        ]);
        let order = Validator::new(&defs).validate().unwrap();
        let result = Compiler::new(&files).compile(&defs, &order).unwrap();

        assert_eq!(result[&n("AuthSpecs")], path_set(&["auth/auth.spec.md"]));
        assert_eq!(result[&n("AuthTests")], path_set(&["auth/test/login_test.rs"]));

        // Simulate the AuthImplementor write set:
        // AuthFiles - AuthSpecs - AuthTests
        let auth_impl: BTreeSet<PathBuf> = result[&n("AuthFiles")]
            .difference(&result[&n("AuthSpecs")])
            .cloned()
            .collect::<BTreeSet<_>>()
            .difference(&result[&n("AuthTests")])
            .cloned()
            .collect();
        assert_eq!(auth_impl, path_set(&["auth/login.rs", "auth/logout.rs"]));
    }

    #[test]
    fn enumerate_files_returns_root_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("auth/test")).unwrap();
        fs::write(dir.path().join("auth/login.rs"), "").unwrap();
        fs::write(dir.path().join("auth/test/login_test.rs"), "").unwrap();

        let files = enumerate_files(dir.path()).unwrap();

        assert_eq!(
            files.into_iter().collect::<BTreeSet<_>>(),
            path_set(&["auth/login.rs", "auth/test/login_test.rs"])
        );
    }
}
