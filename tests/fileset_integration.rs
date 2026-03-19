//! Integration test for the file set algebra pipeline.
//!
//! Exercises the full path: TOML deserialization → validation →
//! compilation against a real temporary directory.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use multorum::fileset::{FileSetTable, enumerate_files};

fn path_set(strs: &[&str]) -> BTreeSet<PathBuf> {
    strs.iter().map(PathBuf::from).collect()
}

#[test]
fn full_pipeline_with_tempdir() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Create a file tree that mirrors the design doc example.
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

    // Deserialize the design doc TOML.
    let toml_str = r#"
        SpecFiles.path = "**/*.spec.md"
        TestFiles.path = "**/test/**"
        AuthFiles.path = "auth/**"
        AuthSpecs = "AuthFiles & SpecFiles"
        AuthTests = "AuthFiles & TestFiles"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();

    // Enumerate files from disk and compile.
    let files = enumerate_files(root).unwrap();
    let result = table.compile(&files).unwrap();

    // Verify concrete file sets.
    assert_eq!(
        result[&multorum::fileset::Name::new("SpecFiles").unwrap()],
        path_set(&["api/api.spec.md", "auth/auth.spec.md"])
    );
    assert_eq!(
        result[&multorum::fileset::Name::new("TestFiles").unwrap()],
        path_set(&["api/test/api_test.rs", "auth/test/login_test.rs"])
    );
    assert_eq!(
        result[&multorum::fileset::Name::new("AuthFiles").unwrap()],
        path_set(&[
            "auth/auth.spec.md",
            "auth/login.rs",
            "auth/logout.rs",
            "auth/test/login_test.rs",
        ])
    );
    assert_eq!(
        result[&multorum::fileset::Name::new("AuthSpecs").unwrap()],
        path_set(&["auth/auth.spec.md"])
    );
    assert_eq!(
        result[&multorum::fileset::Name::new("AuthTests").unwrap()],
        path_set(&["auth/test/login_test.rs"])
    );
}
