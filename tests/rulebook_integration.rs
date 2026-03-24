//! Integration tests for the public rulebook pipeline.
//!
//! Exercises the full path: filesystem-backed rulebook loading from the
//! canonical workspace location → aggregate compilation against a real
//! temporary directory → validation of compiled checks and perspectives.

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use multorum::schema::perspective::PerspectiveError;
use multorum::schema::rulebook::{CheckName, CheckPolicy, Rulebook, RulebookError};

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

/// Combined rulebook assembled from the documented examples in `DESIGN.md`.
///
/// The Named Definitions and Check Pipeline sections each show their own
/// fragment. This function merges them into a single valid rulebook so the
/// integration tests exercise the exact syntax documented in the design
/// reference.
fn design_doc_rulebook() -> String {
    let design_doc =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("DESIGN.md")).unwrap();

    let extract = |heading: &str| -> String {
        let (_, after) = design_doc
            .split_once(heading)
            .unwrap_or_else(|| panic!("DESIGN.md must contain {heading}"));
        let (_, after_fence) =
            after.split_once("```toml").expect("section must contain a TOML fence");
        let (block, _) = after_fence.split_once("```").expect("TOML fence must close");
        block.trim().to_owned()
    };

    let filesets_and_perspectives = extract("### Named Definitions");
    let checks = extract("### Check Pipeline");

    format!("{filesets_and_perspectives}\n\n{checks}\n")
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
    let rulebook_toml = design_doc_rulebook();
    let workspace = setup_workspace(&rulebook_toml, design_doc_files());

    let rulebook = Rulebook::from_workspace_root(workspace.path()).unwrap();
    let compiled = rulebook.compile_for_root(workspace.path()).unwrap();

    assert_eq!(compiled.filesets().len(), 5);
    assert_eq!(compiled.perspectives().len(), 2);
    assert_eq!(compiled.check().len(), 3);
    assert_eq!(
        compiled.check().pipeline(),
        &[
            CheckName::new("fmt").unwrap(),
            CheckName::new("clippy").unwrap(),
            CheckName::new("test").unwrap()
        ]
    );

    let summaries = compiled.perspective_summaries();
    assert_eq!(summaries.len(), 2);
    assert_eq!(summaries[0].name.as_str(), "AuthImplementor");
    assert_eq!(summaries[0].read_count, 1);
    assert_eq!(summaries[0].write_count, 2);
    assert_eq!(summaries[1].name.as_str(), "AuthTester");
    assert_eq!(summaries[1].read_count, 2);
    assert_eq!(summaries[1].write_count, 1);
}

#[test]
fn compile_rejects_duplicate_check_pipeline_entries() {
    let workspace = setup_workspace(
        r#"
            [check]
            pipeline = ["lint", "lint"]

            [check.command]
            lint = "cargo clippy"
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
            [check]
            pipeline = []

            [check.command]
            lint = "cargo clippy"
        "#,
        &["src/main.rs"],
    );

    let rulebook = Rulebook::from_workspace_root(workspace.path()).unwrap();
    let err = rulebook.compile_for_root(workspace.path()).unwrap_err();

    assert!(matches!(err, RulebookError::CheckValidation(_)));
}

#[test]
fn compile_defaults_omitted_policy_entries_to_always() {
    let workspace = setup_workspace(
        r#"
            [check]
            pipeline = ["test"]

            [check.command]
            test = "cargo test --workspace"
        "#,
        &["src/main.rs"],
    );

    let rulebook = Rulebook::from_workspace_root(workspace.path()).unwrap();
    let compiled = rulebook.compile_for_root(workspace.path()).unwrap();
    let policy = compiled
        .check()
        .get(&CheckName::new("test").unwrap())
        .expect("compiled checks should contain the pipeline entry")
        .policy();

    assert_eq!(policy, CheckPolicy::Always);
}

#[test]
fn compile_surfaces_perspective_undefined_fileset_errors() {
    let workspace = setup_workspace(
        r#"
            [fileset]
            AuthFiles.path = "auth/**"

            [perspective.AuthImplementor]
            read  = "MissingFiles"
            write = "AuthFiles"

            [check]
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
    let rulebook_toml = design_doc_rulebook();
    let workspace = setup_workspace(&rulebook_toml, design_doc_files());
    let rulebook = Rulebook::from_workspace_root(workspace.path()).unwrap();

    let explicit_files = design_doc_files().iter().map(PathBuf::from).collect::<Vec<_>>();

    let from_root = rulebook.compile_for_root(workspace.path()).unwrap();
    let from_explicit_files = rulebook.compile(&explicit_files).unwrap();

    assert_eq!(from_root.filesets(), from_explicit_files.filesets());
    assert_eq!(from_root.perspectives().len(), from_explicit_files.perspectives().len());
    assert_eq!(from_root.check().pipeline(), from_explicit_files.check().pipeline());
    assert_eq!(from_root.perspective_summaries(), from_explicit_files.perspective_summaries());
}
