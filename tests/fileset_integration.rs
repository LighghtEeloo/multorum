//! Integration test for the file set algebra pipeline.
//!
//! Exercises the full path: TOML deserialization → validation →
//! compilation against a real temporary directory.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use multorum::schema::fileset::DirectoryPath;
use multorum::schema::fileset::{
    FileSetError, FileSetTable, Name, ValidationError, enumerate_files,
};

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
        SpecFiles.glob = "**/*.spec.md"
        TestFiles.glob = "**/test/**"
        AuthFiles.glob = "auth/**"
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
        AuthFiles.glob = "auth/**"
        ApiFiles.glob  = "api/**"
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
        AuthFiles.glob = "auth/**"
        SpecFiles.glob = "**/*.spec.md"
        TestFiles.glob = "**/test/**"
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
        ApiFiles.glob  = "api/**"
        TestFiles.glob = "**/test/**"
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
        AuthFiles.glob = "auth/**"
        ApiFiles.glob  = "api/**"
        SpecFiles.glob = "**/*.spec.md"
        Flat = "AuthFiles | ApiFiles & SpecFiles"
    "#;
    // With parens: AuthFiles | (ApiFiles & SpecFiles)
    let grouped = r#"
        AuthFiles.glob = "auth/**"
        ApiFiles.glob  = "api/**"
        SpecFiles.glob = "**/*.spec.md"
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
        Nothing.glob = "nonexistent/**"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let result = table.compile(&files).unwrap();

    assert!(result[&n("Nothing")].is_empty());
}

#[test]
fn all_primitives_no_compounds() {
    let (_dir, files) = setup_tempdir();

    let toml_str = r#"
        RustFiles.glob = "**/*.rs"
        MarkdownFiles.glob = "**/*.md"
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
        AllFiles.glob   = "**/*"
        SpecFiles.glob  = "**/*.spec.md"
        NonSpec = "AllFiles - SpecFiles"
        TestFiles.glob  = "**/test/**"
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

// --- Opaque directory tests ---

/// Create a file tree with a vendor directory alongside src.
fn setup_tempdir_with_vendor() -> (tempfile::TempDir, Vec<PathBuf>) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let file_paths = [
        "src/main.rs",
        "src/util.rs",
        "src/test/main_test.rs",
        "vendor/lib/a.rs",
        "vendor/lib/b.rs",
        "vendor/other/c.rs",
        "docs/readme.md",
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
fn basic_opaque_collects_files_under_prefix() {
    let (_dir, files) = setup_tempdir_with_vendor();

    let toml_str = r#"
        Vendor.opaque = "vendor/"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let result = table.compile(&files).unwrap();

    assert_eq!(
        result[&n("Vendor")],
        path_set(&["vendor/lib/a.rs", "vendor/lib/b.rs", "vendor/other/c.rs"])
    );
}

#[test]
fn opaque_excludes_files_from_glob() {
    let (_dir, files) = setup_tempdir_with_vendor();

    let toml_str = r#"
        Vendor.opaque = "vendor/"
        AllRust.glob  = "**/*.rs"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let result = table.compile(&files).unwrap();

    // Glob should only see non-vendor files.
    assert_eq!(
        result[&n("AllRust")],
        path_set(&["src/main.rs", "src/util.rs", "src/test/main_test.rs"])
    );
}

#[test]
fn opaque_plus_glob_plus_compound() {
    let (_dir, files) = setup_tempdir_with_vendor();

    let toml_str = r#"
        Vendor.opaque = "vendor/"
        Src.glob      = "src/**"
        Everything = "Vendor | Src"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let result = table.compile(&files).unwrap();

    assert_eq!(
        result[&n("Everything")],
        path_set(&[
            "src/main.rs",
            "src/util.rs",
            "src/test/main_test.rs",
            "vendor/lib/a.rs",
            "vendor/lib/b.rs",
            "vendor/other/c.rs",
        ])
    );
}

#[test]
fn validation_rejects_overlapping_opaques() {
    let (_dir, files) = setup_tempdir_with_vendor();

    let toml_str = r#"
        AllVendor.opaque = "vendor/"
        VendorLib.opaque = "vendor/lib/"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let err = table.compile(&files).unwrap_err();
    assert!(matches!(err, FileSetError::Validation(ValidationError::OverlappingOpaques { .. })));
}

#[test]
fn validation_rejects_glob_metacharacters_in_opaque() {
    let toml_str = r#"Bad.opaque = "vendor/*""#;
    let result: Result<FileSetTable, _> = toml::from_str(toml_str);
    assert!(result.is_err(), "metacharacter `*` should be rejected");
}

#[test]
fn validation_rejects_empty_opaque_path() {
    let toml_str = r#"Bad.opaque = """#;
    let result: Result<FileSetTable, _> = toml::from_str(toml_str);
    assert!(result.is_err(), "empty opaque path should be rejected");
}

#[test]
fn deserialization_of_opaque_key() {
    let toml_str = r#"
        Vendor.opaque = "third_party/vendor"
        AuthFiles.glob = "auth/**"
        Combined = "Vendor | AuthFiles"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let defs = table.definitions();
    assert_eq!(defs.len(), 3);

    use multorum::schema::fileset::Definition;
    assert!(matches!(defs.get(&n("Vendor")), Some(Definition::Opaque(_))));
    assert!(matches!(defs.get(&n("AuthFiles")), Some(Definition::Primitive(_))));
    assert!(matches!(defs.get(&n("Combined")), Some(Definition::Compound(_))));
}

#[test]
fn directory_path_type_validates() {
    assert!(DirectoryPath::new("vendor/lib").is_ok());
    assert!(DirectoryPath::new("vendor/lib/").is_ok());
    assert!(DirectoryPath::new("").is_err());
    assert!(DirectoryPath::new("/").is_err());
    assert!(DirectoryPath::new("vendor/*").is_err());
    assert!(DirectoryPath::new("vendor/?").is_err());
    assert!(DirectoryPath::new("vendor/[a]").is_err());
    assert!(DirectoryPath::new("vendor/{a}").is_err());
}

#[test]
fn opaque_prefix_does_not_match_similar_directory_names() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file_paths = ["vendor/lib/a.rs", "vendorized/lib/b.rs", "vendor-lib/c.rs", "src/main.rs"];
    for path in &file_paths {
        let full = root.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(&full, "").unwrap();
    }

    let files = enumerate_files(root).unwrap();
    let toml_str = r#"
        Vendor.opaque = "vendor"
        AllRust.glob = "**/*.rs"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let result = table.compile(&files).unwrap();

    assert_eq!(result[&n("Vendor")], path_set(&["vendor/lib/a.rs"]));
    assert_eq!(
        result[&n("AllRust")],
        path_set(&["src/main.rs", "vendor-lib/c.rs", "vendorized/lib/b.rs"])
    );
}

#[test]
fn compound_intersection_with_opaque_and_glob_is_empty() {
    let (_dir, files) = setup_tempdir_with_vendor();

    let toml_str = r#"
        Vendor.opaque = "vendor/"
        Rust.glob = "**/*.rs"
        VendorRust = "Vendor & Rust"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let result = table.compile(&files).unwrap();

    // Opaque files are removed before glob expansion, so intersection is empty.
    assert!(result[&n("VendorRust")].is_empty());
}

#[test]
fn multiple_disjoint_opaques_partition_glob_visibility() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file_paths =
        ["vendor/a.rs", "generated/x.rs", "src/main.rs", "src/lib.rs", "tests/main_test.rs"];
    for path in &file_paths {
        let full = root.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(&full, "").unwrap();
    }

    let files = enumerate_files(root).unwrap();
    let toml_str = r#"
        Vendor.opaque = "vendor/"
        Generated.opaque = "generated/"
        AllRust.glob = "**/*.rs"
        Everything = "Vendor | Generated | AllRust"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let result = table.compile(&files).unwrap();

    assert_eq!(result[&n("Vendor")], path_set(&["vendor/a.rs"]));
    assert_eq!(result[&n("Generated")], path_set(&["generated/x.rs"]));
    assert_eq!(
        result[&n("AllRust")],
        path_set(&["src/lib.rs", "src/main.rs", "tests/main_test.rs"])
    );
    assert_eq!(
        result[&n("Everything")],
        path_set(&[
            "generated/x.rs",
            "src/lib.rs",
            "src/main.rs",
            "tests/main_test.rs",
            "vendor/a.rs",
        ])
    );
}

#[test]
fn opaque_definition_with_no_matching_files_compiles_to_empty() {
    let (_dir, files) = setup_tempdir_with_vendor();

    let toml_str = r#"
        Missing.opaque = "third_party/"
        Rust.glob = "**/*.rs"
    "#;
    let table: FileSetTable = toml::from_str(toml_str).unwrap();
    let result = table.compile(&files).unwrap();

    assert!(result[&n("Missing")].is_empty());
    assert_eq!(
        result[&n("Rust")],
        path_set(&[
            "src/main.rs",
            "src/test/main_test.rs",
            "src/util.rs",
            "vendor/lib/a.rs",
            "vendor/lib/b.rs",
            "vendor/other/c.rs",
        ])
    );
}

#[test]
fn stress_many_opaques_many_compounds_and_many_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    for i in 0..40 {
        for j in 0..5 {
            let opaque_file = root.join(format!("opaque{i}/f{j}.rs"));
            fs::create_dir_all(opaque_file.parent().unwrap()).unwrap();
            fs::write(opaque_file, "").unwrap();

            let src_file = root.join(format!("src/module{i}/m{j}.rs"));
            fs::create_dir_all(src_file.parent().unwrap()).unwrap();
            fs::write(src_file, "").unwrap();
        }
    }

    let files = enumerate_files(root).unwrap();
    let mut toml_str = String::new();
    for i in 0..40 {
        toml_str.push_str(&format!("Opaque{i}.opaque = \"opaque{i}/\"\n"));
    }
    toml_str.push_str("Src.glob = \"src/**\"\n");
    toml_str.push_str("AllOpaque = \"Opaque0");
    for i in 1..40 {
        toml_str.push_str(&format!(" | Opaque{i}"));
    }
    toml_str.push_str("\"\n");
    toml_str.push_str("Everything = \"AllOpaque | Src\"\n");

    let table: FileSetTable = toml::from_str(&toml_str).unwrap();
    let result = table.compile(&files).unwrap();

    assert_eq!(result[&n("Src")].len(), 200);
    assert_eq!(result[&n("AllOpaque")].len(), 200);
    assert_eq!(result[&n("Everything")].len(), 400);
}
