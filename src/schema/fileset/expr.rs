//! File set expressions, definitions, and the `[fileset]` table.

use std::collections::BTreeMap;

use serde::de;

use super::Name;
use super::error::{DirectoryPathError, GlobPatternError};
use super::parse::ExprParser;

/// A validated glob pattern string (e.g. `auth/**`, `**/*.spec.md`).
///
/// ## Invariant
///
/// The pattern is parseable by [`wax::Glob`]. Validated eagerly at
/// construction time so that errors surface during deserialization
/// rather than during the expensive compilation step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobPattern(String);

impl GlobPattern {
    /// Create a new `GlobPattern`, validating that [`wax::Glob`] can parse it.
    pub fn new(pattern: &str) -> Result<Self, GlobPatternError> {
        // Validate by attempting to parse. We discard the compiled glob
        // here — compilation will re-parse when it needs a matcher.
        wax::Glob::new(pattern).map_err(|err| GlobPatternError::Invalid {
            pattern: pattern.to_owned(),
            reason: err.to_string(),
        })?;
        Ok(Self(pattern.to_owned()))
    }

    /// The raw pattern string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A validated literal directory path for opaque directory definitions.
///
/// ## Invariants
///
/// - Contains no glob metacharacters (`*`, `?`, `[`, `{`).
/// - Normalized to end with `/`.
/// - Not empty or root-only (`""`, `"/"`).
///
/// Construct via [`DirectoryPath::new`], which validates and normalizes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryPath(String);

impl DirectoryPath {
    /// Glob metacharacters forbidden in directory paths.
    const META_CHARS: &[char] = &['*', '?', '[', '{'];

    /// Create a new `DirectoryPath`, validating invariants and normalizing
    /// the trailing `/`.
    pub fn new(path: &str) -> Result<Self, DirectoryPathError> {
        if path.is_empty() {
            return Err(DirectoryPathError::Empty);
        }

        let normalized = if path.ends_with('/') { path.to_owned() } else { format!("{path}/") };

        if normalized == "/" {
            return Err(DirectoryPathError::RootOnly);
        }

        if let Some(&ch) = Self::META_CHARS.iter().find(|&&ch| normalized.contains(ch)) {
            return Err(DirectoryPathError::ContainsMetacharacter { path: path.to_owned(), ch });
        }

        Ok(Self(normalized))
    }

    /// The normalized directory path string (always ends with `/`).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A file set expression in the algebra.
///
/// Expressions are parsed from strings in compound definitions and
/// represent set operations over named file sets. They are compiled
/// away during rulebook activation and do not exist at runtime.
///
/// ## Syntax
///
/// ```text
/// expr  ::= name                        reference
///         | expr "|" expr               union
///         | expr "&" expr               intersection
///         | expr "-" expr               difference
///         | "(" expr ")"                grouping
/// ```
///
/// All binary operators have equal precedence with left-to-right
/// associativity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// Reference to a named file set.
    Ref(Name),
    /// Union: every file in either set.
    Union(Box<Expr>, Box<Expr>),
    /// Intersection: only files present in both sets.
    Intersection(Box<Expr>, Box<Expr>),
    /// Difference: files in the left set not in the right.
    Difference(Box<Expr>, Box<Expr>),
}

/// A file set definition: a primitive glob, an opaque directory, or a
/// compound expression.
///
/// In the TOML rulebook:
/// - Primitives use the `.glob` key: `AuthFiles.glob = "auth/**"`
/// - Opaques use the `.opaque` key: `Vendor.opaque = "third_party/vendor"`
/// - Compounds are expression strings: `AuthSpecs = "AuthFiles & SpecFiles"`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Definition {
    /// A primitive file set bound to a glob pattern.
    Primitive(GlobPattern),
    /// An opaque directory: every file under the given prefix.
    Opaque(DirectoryPath),
    /// A compound file set defined by a set expression.
    Compound(Expr),
}

/// The complete set of file set definitions from a rulebook's
/// `[fileset]` table.
///
/// Wraps `BTreeMap<Name, Definition>` for deterministic ordering.
#[derive(Debug, Clone, Default)]
pub struct FileSetTable {
    definitions: BTreeMap<Name, Definition>,
}

impl FileSetTable {
    /// The underlying definitions map.
    pub fn definitions(&self) -> &BTreeMap<Name, Definition> {
        &self.definitions
    }

    /// Consume the table and return the inner map.
    pub fn into_definitions(self) -> BTreeMap<Name, Definition> {
        self.definitions
    }

    /// Validate and compile all definitions against a file list.
    ///
    /// This is the primary entry point for the file set pipeline.
    /// It validates structural correctness (no cycles, no undefined
    /// references), then compiles each definition into a concrete
    /// `BTreeSet<PathBuf>`.
    pub fn compile(
        &self, files: &[std::path::PathBuf],
    ) -> Result<
        BTreeMap<Name, std::collections::BTreeSet<std::path::PathBuf>>,
        super::error::FileSetError,
    > {
        let order = super::validate::Validator::new(&self.definitions).validate()?;
        let result = super::compile::Compiler::new(files).compile(&self.definitions, &order)?;
        Ok(result)
    }
}

/// Raw TOML value for a single file set entry.
///
/// In the `[fileset]` table, each entry is one of:
/// - A sub-table with a `glob` key: `SpecFiles.glob = "**/*.spec.md"`
/// - A sub-table with an `opaque` key: `Vendor.opaque = "third_party/vendor"`
/// - A plain string expression: `AuthSpecs = "AuthFiles & SpecFiles"`
///
/// Note: `Primitive` and `Opaque` must precede `Compound` so that
/// `#[serde(untagged)]` tries the structured variants before falling
/// back to the plain string.
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum RawDefinition {
    Primitive { glob: String },
    Opaque { opaque: String },
    Compound(String),
}

impl<'de> de::Deserialize<'de> for FileSetTable {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let raw: BTreeMap<String, RawDefinition> = BTreeMap::deserialize(deserializer)?;

        let mut definitions = BTreeMap::new();
        for (key, value) in raw {
            let name = Name::new(&key).map_err(de::Error::custom)?;
            let def = match value {
                | RawDefinition::Primitive { glob } => {
                    let pattern = GlobPattern::new(&glob).map_err(de::Error::custom)?;
                    Definition::Primitive(pattern)
                }
                | RawDefinition::Opaque { opaque } => {
                    let dir = DirectoryPath::new(&opaque).map_err(de::Error::custom)?;
                    Definition::Opaque(dir)
                }
                | RawDefinition::Compound(expr_str) => {
                    let expr = ExprParser::new(&expr_str).parse().map_err(de::Error::custom)?;
                    Definition::Compound(expr)
                }
            };
            definitions.insert(name, def);
        }

        Ok(FileSetTable { definitions })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_pattern_valid() {
        assert!(GlobPattern::new("**/*.spec.md").is_ok());
        assert!(GlobPattern::new("auth/**").is_ok());
        assert!(GlobPattern::new("src/main.rs").is_ok());
    }

    #[test]
    fn glob_pattern_invalid() {
        let err = GlobPattern::new("[unclosed").unwrap_err();
        assert!(matches!(err, GlobPatternError::Invalid { .. }));
    }

    #[test]
    fn expr_construction() {
        let a = Expr::Ref(Name::new("A").unwrap());
        let b = Expr::Ref(Name::new("B").unwrap());
        let union = Expr::Union(Box::new(a.clone()), Box::new(b.clone()));
        let inter = Expr::Intersection(Box::new(a.clone()), Box::new(b.clone()));
        let diff = Expr::Difference(Box::new(a), Box::new(b));

        // Verify they are distinct variants.
        assert_ne!(union, inter);
        assert_ne!(inter, diff);
    }

    #[test]
    fn directory_path_normalizes_trailing_slash() {
        let dp = DirectoryPath::new("vendor/lib").unwrap();
        assert_eq!(dp.as_str(), "vendor/lib/");
    }

    #[test]
    fn directory_path_preserves_existing_slash() {
        let dp = DirectoryPath::new("vendor/lib/").unwrap();
        assert_eq!(dp.as_str(), "vendor/lib/");
    }

    #[test]
    fn directory_path_rejects_empty() {
        assert!(matches!(DirectoryPath::new(""), Err(DirectoryPathError::Empty)));
    }

    #[test]
    fn directory_path_rejects_root_only() {
        assert!(matches!(DirectoryPath::new("/"), Err(DirectoryPathError::RootOnly)));
    }

    #[test]
    fn directory_path_rejects_metacharacters() {
        for (input, ch) in
            [("vendor/*", '*'), ("vendor/?", '?'), ("vendor/[a]", '['), ("vendor/{a}", '{')]
        {
            let err = DirectoryPath::new(input).unwrap_err();
            assert!(
                matches!(err, DirectoryPathError::ContainsMetacharacter { ch: c, .. } if c == ch)
            );
        }
    }

    #[test]
    fn definition_variants() {
        let prim = Definition::Primitive(GlobPattern::new("**/*.rs").unwrap());
        let opaque = Definition::Opaque(DirectoryPath::new("vendor/").unwrap());
        let compound = Definition::Compound(Expr::Ref(Name::new("X").unwrap()));
        assert_ne!(prim, compound);
        assert_ne!(prim, opaque);
        assert_ne!(opaque, compound);
    }

    #[test]
    fn deserialize_design_doc_example() {
        let toml_str = r#"
            SpecFiles.glob = "**/*.spec.md"
            TestFiles.glob = "**/test/**"
            AuthFiles.glob = "auth/**"
            AuthSpecs = "AuthFiles & SpecFiles"
            AuthTests = "AuthFiles & TestFiles"
        "#;
        let table: FileSetTable = toml::from_str(toml_str).unwrap();
        let defs = table.definitions();

        assert_eq!(defs.len(), 5);

        // Primitives.
        assert!(matches!(
            defs.get(&Name::new("SpecFiles").unwrap()),
            Some(Definition::Primitive(_))
        ));
        assert!(matches!(
            defs.get(&Name::new("AuthFiles").unwrap()),
            Some(Definition::Primitive(_))
        ));

        // Compounds.
        assert!(matches!(
            defs.get(&Name::new("AuthSpecs").unwrap()),
            Some(Definition::Compound(_))
        ));
        assert!(matches!(
            defs.get(&Name::new("AuthTests").unwrap()),
            Some(Definition::Compound(_))
        ));
    }

    #[test]
    fn deserialize_opaque_key() {
        let toml_str = r#"
            Vendor.opaque = "third_party/vendor"
            AuthFiles.glob = "auth/**"
        "#;
        let table: FileSetTable = toml::from_str(toml_str).unwrap();
        let defs = table.definitions();
        assert_eq!(defs.len(), 2);

        match defs.get(&Name::new("Vendor").unwrap()) {
            | Some(Definition::Opaque(dp)) => assert_eq!(dp.as_str(), "third_party/vendor/"),
            | other => panic!("expected Opaque, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_rejects_invalid_opaque() {
        let toml_str = r#"Bad.opaque = "vendor/*""#;
        let result: Result<FileSetTable, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_rejects_invalid_name() {
        let toml_str = r#"lowercaseName.glob = "**/*.rs""#;
        let result: Result<FileSetTable, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_rejects_invalid_glob() {
        let toml_str = r#"Bad.glob = "[unclosed""#;
        let result: Result<FileSetTable, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_rejects_invalid_expr() {
        let toml_str = r#"Bad = "A |""#;
        let result: Result<FileSetTable, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }
}
