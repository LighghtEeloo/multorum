//! Integration test for the file set algebra pipeline.
//!
//! Exercises the full path: TOML deserialization → validation →
//! compilation against a real temporary directory.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use multorum::schema::fileset::{FileSetError, FileSetTable, Name, ValidationError, enumerate_files};

fn path_set(strs: &[&str]) -> BTreeSet<PathBuf> {
    strs.iter().map(PathBuf::from).collect()
}

fn n(s: &str) -> Name {
    Name::new(s).unwrap()
}

/// Create the design-doc file tree in a temporary directory and
/// return the tempdir handle plus the enumerated file list.
fn setup_tempdir() -> (tempfile::TempDir, Vec<PathBuf>) {
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

    let files = enumerate_files(root).unwrap();
    (dir, files)
}

#[test]
fn full_pipeline_with_tempdir() {
    let (_dir, files) = setup_tempdir();

    let toml_str = r#"
        SpecFiles.path = "**/*.spec.md"
        TestFiles.path = "**/test/**"
        AuthFiles.path = "auth/**"
        AuthSpecs = "AuthFiles & SpecFiles"
        AuthTests = "AuthFiles & TestFiles"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let result = table.compile(&files).unwrap();

    assert_eq!(result[&n("SpecFiles")], path_set(&["api/api.spec.md", "auth/auth.spec.md"]));
    assert_eq!(
        result[&n("TestFiles")],
        path_set(&["api/test/api_test.rs", "auth/test/login_test.rs"])
    );
    assert_eq!(
        result[&n("AuthFiles")],
        path_set(&[
            "auth/auth.spec.md",
            "auth/login.rs",
            "auth/logout.rs",
            "auth/test/login_test.rs",
        ])
    );
    assert_eq!(result[&n("AuthSpecs")], path_set(&["auth/auth.spec.md"]));
    assert_eq!(result[&n("AuthTests")], path_set(&["auth/test/login_test.rs"]));
}

#[test]
fn union_merges_disjoint_sets() {
    let (_dir, files) = setup_tempdir();

    let toml_str = r#"
        AuthFiles.path = "auth/**"
        ApiFiles.path  = "api/**"
        All = "AuthFiles | ApiFiles"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let result = table.compile(&files).unwrap();

    // Union should contain every file from both modules.
    assert_eq!(result[&n("All")].len(), 7);
    assert!(result[&n("All")].contains(&PathBuf::from("auth/login.rs")));
    assert!(result[&n("All")].contains(&PathBuf::from("api/handler.rs")));
}

#[test]
fn difference_subtracts_correctly() {
    let (_dir, files) = setup_tempdir();

    let toml_str = r#"
        AuthFiles.path = "auth/**"
        SpecFiles.path = "**/*.spec.md"
        TestFiles.path = "**/test/**"
        AuthImpl = "AuthFiles - SpecFiles - TestFiles"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let result = table.compile(&files).unwrap();

    // Only production auth files remain.
    assert_eq!(result[&n("AuthImpl")], path_set(&["auth/login.rs", "auth/logout.rs"]));
}

#[test]
fn intersection_narrows_correctly() {
    let (_dir, files) = setup_tempdir();

    let toml_str = r#"
        ApiFiles.path  = "api/**"
        TestFiles.path = "**/test/**"
        ApiTests = "ApiFiles & TestFiles"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let result = table.compile(&files).unwrap();

    assert_eq!(result[&n("ApiTests")], path_set(&["api/test/api_test.rs"]));
}

#[test]
fn parenthesized_grouping_changes_result() {
    let (_dir, files) = setup_tempdir();

    // Without parens: (AuthFiles | ApiFiles) & SpecFiles
    let flat = r#"
        AuthFiles.path = "auth/**"
        ApiFiles.path  = "api/**"
        SpecFiles.path = "**/*.spec.md"
        Flat = "AuthFiles | ApiFiles & SpecFiles"
    "#;
    // With parens: AuthFiles | (ApiFiles & SpecFiles)
    let grouped = r#"
        AuthFiles.path = "auth/**"
        ApiFiles.path  = "api/**"
        SpecFiles.path = "**/*.spec.md"
        Grouped = "AuthFiles | (ApiFiles & SpecFiles)"
    "#;

    let flat_result = toml::from_str::<FileSetTable>(flat).unwrap().compile(&files).unwrap();
    let grouped_result = toml::from_str::<FileSetTable>(grouped).unwrap().compile(&files).unwrap();

    // Flat precedence: union first, then intersect with SpecFiles.
    // Result: all files in auth + api, intersected with spec files.
    assert_eq!(flat_result[&n("Flat")], path_set(&["api/api.spec.md", "auth/auth.spec.md"]));

    // Grouped: ApiFiles & SpecFiles first, then union with all auth.
    // Result: api/api.spec.md plus all auth files.
    assert_eq!(
        grouped_result[&n("Grouped")],
        path_set(&[
            "api/api.spec.md",
            "auth/auth.spec.md",
            "auth/login.rs",
            "auth/logout.rs",
            "auth/test/login_test.rs",
        ])
    );
}

#[test]
fn glob_matching_no_files_produces_empty_set() {
    let (_dir, files) = setup_tempdir();

    let toml_str = r#"
        Nothing.path = "nonexistent/**"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let result = table.compile(&files).unwrap();

    assert!(result[&n("Nothing")].is_empty());
}

#[test]
fn all_primitives_no_compounds() {
    let (_dir, files) = setup_tempdir();

    let toml_str = r#"
        RustFiles.path = "**/*.rs"
        MarkdownFiles.path = "**/*.md"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let result = table.compile(&files).unwrap();

    assert_eq!(
        result[&n("RustFiles")],
        path_set(&[
            "api/handler.rs",
            "api/test/api_test.rs",
            "auth/login.rs",
            "auth/logout.rs",
            "auth/test/login_test.rs",
        ])
    );
    assert_eq!(result[&n("MarkdownFiles")], path_set(&["api/api.spec.md", "auth/auth.spec.md"]));
}

#[test]
fn chained_compounds() {
    let (_dir, files) = setup_tempdir();

    // A depends on B which depends on C — three levels deep.
    let toml_str = r#"
        AllFiles.path   = "**/*"
        SpecFiles.path  = "**/*.spec.md"
        NonSpec = "AllFiles - SpecFiles"
        TestFiles.path  = "**/test/**"
        NonSpecNonTest = "NonSpec - TestFiles"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let result = table.compile(&files).unwrap();

    // Only production source files remain.
    assert_eq!(
        result[&n("NonSpecNonTest")],
        path_set(&["api/handler.rs", "auth/login.rs", "auth/logout.rs",])
    );
}

#[test]
fn validation_rejects_undefined_reference() {
    let (_dir, files) = setup_tempdir();

    let toml_str = r#"
        Bad = "Nonexistent & AlsoMissing"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let err = table.compile(&files).unwrap_err();
    assert!(matches!(err, FileSetError::Validation(ValidationError::Undefined { .. })));
}

#[test]
fn validation_rejects_cycle() {
    let toml_str = r#"
        A = "B"
        B = "A"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let (_dir, files) = setup_tempdir();
    let err = table.compile(&files).unwrap_err();
    assert!(matches!(err, FileSetError::Validation(ValidationError::Cycle { .. })));
}

#[test]
fn enumerate_files_finds_all() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let paths = ["a.txt", "sub/b.txt", "sub/deep/c.txt"];
    for path in &paths {
        let full = root.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(&full, "").unwrap();
    }

    let files: BTreeSet<PathBuf> = enumerate_files(root).unwrap().into_iter().collect();
    assert_eq!(files, path_set(&["a.txt", "sub/b.txt", "sub/deep/c.txt"]));
}
