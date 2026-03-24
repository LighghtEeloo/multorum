//! Integration tests for the perspective pipeline.
//!
//! Exercises the full path: TOML deserialization of file sets and
//! perspectives → file set compilation against a real temporary
//! directory → perspective compilation.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use multorum::fileset::{FileSetTable, enumerate_files};
use multorum::perspective::{CompiledPerspectives, PerspectiveName, PerspectiveTable};

fn path_set(strs: &[&str]) -> BTreeSet<PathBuf> {
    strs.iter().map(PathBuf::from).collect()
}

/// Create the design-doc file tree in a temporary directory and
/// return compiled file sets ready for perspective compilation.
fn setup_design_doc_filesets()
-> (tempfile::TempDir, std::collections::BTreeMap<multorum::fileset::Name, BTreeSet<PathBuf>>) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let file_paths = [
        "auth/login.rs",
        "auth/logout.rs",
        "auth/auth.spec.md",
        "auth/test/login_test.rs",
        "api/handler.rs",
        "api/api.spec.md",
        "api/test/api_test.rs",
    ];
    for path in &file_paths {
        let full = root.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(&full, "").unwrap();
    }

    let toml_str = r#"
        SpecFiles.path = "**/*.spec.md"
        TestFiles.path = "**/test/**"
        AuthFiles.path = "auth/**"
        AuthSpecs = "AuthFiles & SpecFiles"
        AuthTests = "AuthFiles & TestFiles"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let files = enumerate_files(root).unwrap();
    let compiled = table.compile(&files).unwrap();

    (dir, compiled)
}

#[test]
fn full_pipeline_with_tempdir() {
    let (_dir, filesets) = setup_design_doc_filesets();

    let toml_str = r#"
        [AuthImplementor]
        read  = "AuthSpecs"
        write = "AuthFiles - AuthSpecs - AuthTests"

        [AuthTester]
        read  = "AuthSpecs | AuthTests"
        write = "AuthTests"
    "#;
    let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
    let compiled = table.compile(&filesets).unwrap();

    let impl_p = compiled.get(&PerspectiveName::new("AuthImplementor").unwrap()).unwrap();
    assert_eq!(*impl_p.write(), path_set(&["auth/login.rs", "auth/logout.rs"]));
    assert_eq!(*impl_p.read(), path_set(&["auth/auth.spec.md"]));

    let test_p = compiled.get(&PerspectiveName::new("AuthTester").unwrap()).unwrap();
    assert_eq!(*test_p.write(), path_set(&["auth/test/login_test.rs"]));
    assert_eq!(*test_p.read(), path_set(&["auth/auth.spec.md", "auth/test/login_test.rs"]));
}

#[test]
fn empty_perspectives_are_valid() {
    let (_dir, filesets) = setup_design_doc_filesets();

    let table: PerspectiveTable = toml::from_str("").unwrap();
    let compiled = table.compile(&filesets).unwrap();
    assert!(compiled.is_empty());
}

#[test]
fn single_perspective_always_passes_conflict_validation() {
    let (_dir, filesets) = setup_design_doc_filesets();

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
fn three_disjoint_perspectives() {
    let (_dir, filesets) = setup_design_doc_filesets();

    // Three perspectives with pairwise-disjoint writes. Each reads
    // from the API side of the tree so no read set overlaps any
    // other perspective's write set.
    let toml_str = r#"
        [AuthImplementor]
        read  = "SpecFiles - AuthSpecs"
        write = "AuthFiles - AuthSpecs - AuthTests"

        [AuthTester]
        read  = "SpecFiles - AuthSpecs"
        write = "AuthTests"

        [SpecWriter]
        read  = "TestFiles - AuthTests"
        write = "AuthSpecs"
    "#;
    let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
    let compiled = table.compile(&filesets).unwrap();
    assert_eq!(compiled.len(), 3);

    // Verify write sets are disjoint by checking sizes sum correctly.
    let total_write_files: usize = compiled.perspectives().values().map(|p| p.write().len()).sum();
    let union_size: usize = compiled
        .perspectives()
        .values()
        .fold(BTreeSet::new(), |mut acc, p| {
            acc.extend(p.write().iter().cloned());
            acc
        })
        .len();
    assert_eq!(total_write_files, union_size);
}

#[test]
fn shared_reads_are_allowed() {
    let (_dir, filesets) = setup_design_doc_filesets();

    // Multiple perspectives reading the same file sets is fine,
    // as long as no write set overlaps any other perspective's
    // read or write set.
    let toml_str = r#"
        [P]
        read  = "AuthSpecs"
        write = "AuthTests"

        [Q]
        read  = "AuthSpecs"
        write = "AuthFiles - AuthSpecs - AuthTests"
    "#;
    let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
    let compiled: CompiledPerspectives = table.compile(&filesets).unwrap();

    let p = compiled.get(&PerspectiveName::new("P").unwrap()).unwrap();
    let q = compiled.get(&PerspectiveName::new("Q").unwrap()).unwrap();

    // Both read AuthSpecs.
    assert_eq!(*p.read(), path_set(&["auth/auth.spec.md"]));
    assert_eq!(*q.read(), path_set(&["auth/auth.spec.md"]));

    // Write sets are disjoint.
    assert!(p.write().intersection(q.write()).next().is_none());
}
