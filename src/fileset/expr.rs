//! File set expressions, definitions, and the `[filesets]` table.

use std::collections::BTreeMap;

use serde::de;

use super::Name;
use super::error::GlobPatternError;
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

/// A file set definition: either a primitive glob binding or a
/// compound expression.
///
/// In the TOML rulebook:
/// - Primitives use the `.path` key: `AuthFiles.path = "auth/**"`
/// - Compounds are expression strings: `AuthSpecs = "AuthFiles & SpecFiles"`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Definition {
    /// A primitive file set bound to a glob pattern.
    Primitive(GlobPattern),
    /// A compound file set defined by a set expression.
    Compound(Expr),
}

/// The complete set of file set definitions from a rulebook's
/// `[filesets]` table.
///
/// Wraps `BTreeMap<Name, Definition>` for deterministic ordering.
#[derive(Debug, Clone)]
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
/// In the `[filesets]` table, each entry is either:
/// - A sub-table with a `path` key: `SpecFiles.path = "**/*.spec.md"`
/// - A plain string expression: `AuthSpecs = "AuthFiles & SpecFiles"`
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum RawDefinition {
    Primitive { path: String },
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
                | RawDefinition::Primitive { path } => {
                    let pattern = GlobPattern::new(&path).map_err(de::Error::custom)?;
                    Definition::Primitive(pattern)
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
    fn definition_variants() {
        let prim = Definition::Primitive(GlobPattern::new("**/*.rs").unwrap());
        let compound = Definition::Compound(Expr::Ref(Name::new("X").unwrap()));
        assert_ne!(prim, compound);
    }

    #[test]
    fn deserialize_design_doc_example() {
        let toml_str = r#"
            SpecFiles.path = "**/*.spec.md"
            TestFiles.path = "**/test/**"
            AuthFiles.path = "auth/**"
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
    fn deserialize_rejects_invalid_name() {
        let toml_str = r#"lowercaseName.path = "**/*.rs""#;
        let result: Result<FileSetTable, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_rejects_invalid_glob() {
        let toml_str = r#"Bad.path = "[unclosed""#;
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
