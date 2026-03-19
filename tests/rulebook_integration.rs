//! Integration tests for the public rulebook pipeline.
//!
//! Exercises the full path: filesystem-backed rulebook loading from the
//! canonical workspace location → aggregate compilation against a real
//! temporary directory → validation of compiled checks and perspectives.

use std::fs;
use std::path::PathBuf;

use multorum::perspective::PerspectiveError;
use multorum::rulebook::{CheckPolicy, Rulebook, RulebookError};

/// Create a temporary workspace containing a canonical
/// `.multorum/rulebook.toml` and a small project tree.
fn setup_workspace(rulebook_toml: &str, file_paths: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    for path in file_paths {
        let full = root.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, "").unwrap();
    }

    let rulebook_path = Rulebook::rulebook_path(root);
    fs::create_dir_all(rulebook_path.parent().unwrap()).unwrap();
    fs::write(rulebook_path, rulebook_toml).unwrap();

    dir
}

fn design_doc_rulebook() -> &'static str {
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
    "#
}

fn design_doc_files() -> &'static [&'static str] {
    &[
        "auth/login.rs",
        "auth/logout.rs",
        "auth/auth.spec.md",
        "auth/test/login_test.rs",
        "api/handler.rs",
        "api/api.spec.md",
        "api/test/api_test.rs",
    ]
}

#[test]
fn full_pipeline_from_workspace_root() {
    let workspace = setup_workspace(design_doc_rulebook(), design_doc_files());

    let rulebook = Rulebook::from_workspace_root(workspace.path()).unwrap();
    let compiled = rulebook.compile_for_root(workspace.path()).unwrap();

    assert_eq!(compiled.filesets().len(), 5);
    assert_eq!(compiled.perspectives().len(), 2);
    assert_eq!(compiled.checks().len(), 2);
    assert_eq!(
        compiled
            .checks()
            .get(&multorum::rulebook::CheckName::new("test").unwrap())
            .unwrap()
            .policy(),
        CheckPolicy::Skippable
    );

    let summaries = compiled.perspective_summaries();
    assert_eq!(summaries.len(), 2);
    assert_eq!(summaries[0].name.as_str(), "AuthImplementor");
    assert_eq!(summaries[0].read_count, 1);
    assert_eq!(summaries[0].write_count, 2);
    assert_eq!(summaries[1].name.as_str(), "AuthTester");
    assert_eq!(summaries[1].read_count, 1);
    assert_eq!(summaries[1].write_count, 1);
}

#[test]
fn compile_rejects_duplicate_check_pipeline_entries() {
    let workspace = setup_workspace(
        r#"
            [checks]
            pipeline = ["lint", "lint"]

            [checks.lint]
            command = "cargo clippy"
        "#,
        &["src/main.rs"],
    );

    let rulebook = Rulebook::from_workspace_root(workspace.path()).unwrap();
    let err = rulebook.compile_for_root(workspace.path()).unwrap_err();

    assert!(matches!(err, RulebookError::CheckValidation(_)));
}

#[test]
fn compile_rejects_unused_declared_check() {
    let workspace = setup_workspace(
        r#"
            [checks]
            pipeline = []

            [checks.lint]
            command = "cargo clippy"
        "#,
        &["src/main.rs"],
    );

    let rulebook = Rulebook::from_workspace_root(workspace.path()).unwrap();
    let err = rulebook.compile_for_root(workspace.path()).unwrap_err();

    assert!(matches!(err, RulebookError::CheckValidation(_)));
}

#[test]
fn compile_surfaces_perspective_undefined_fileset_errors() {
    let workspace = setup_workspace(
        r#"
            [filesets]
            AuthFiles.path = "auth/**"

            [perspectives.AuthImplementor]
            read  = "MissingFiles"
            write = "AuthFiles"

            [checks]
            pipeline = []
        "#,
        &["auth/login.rs"],
    );

    let rulebook = Rulebook::from_workspace_root(workspace.path()).unwrap();
    let err = rulebook.compile_for_root(workspace.path()).unwrap_err();

    assert!(matches!(err, RulebookError::Perspective(PerspectiveError::UndefinedFileSet { .. })));
}

#[test]
fn compile_with_explicit_file_list_matches_workspace_compilation() {
    let workspace = setup_workspace(design_doc_rulebook(), design_doc_files());
    let rulebook = Rulebook::from_workspace_root(workspace.path()).unwrap();

    let explicit_files = design_doc_files().iter().map(PathBuf::from).collect::<Vec<_>>();

    let from_root = rulebook.compile_for_root(workspace.path()).unwrap();
    let from_explicit_files = rulebook.compile(&explicit_files).unwrap();

    assert_eq!(from_root.filesets(), from_explicit_files.filesets());
    assert_eq!(from_root.perspectives().len(), from_explicit_files.perspectives().len());
    assert_eq!(from_root.checks().pipeline(), from_explicit_files.checks().pipeline());
    assert_eq!(from_root.perspective_summaries(), from_explicit_files.perspective_summaries());
}
