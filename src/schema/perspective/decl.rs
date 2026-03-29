//! Perspective declarations and TOML deserialization.
//!
//! A [`PerspectiveDecl`] holds the parsed read and write expressions
//! for a single perspective. A [`PerspectiveTable`] collects all
//! declarations from the `[perspective]` TOML table.

use std::collections::BTreeMap;

use serde::de;

use crate::schema::fileset::{Expr, ExprParser};

use super::error::PerspectiveError;
use super::name::PerspectiveName;

/// A single perspective declaration with parsed read and write
/// expressions.
///
/// Created during deserialization of the `[perspective]` table.
/// Not yet compiled against concrete file sets.
///
/// Either field may be `None`, meaning the empty set: the perspective
/// claims no read dependencies or no write permissions respectively.
#[derive(Debug, Clone)]
pub struct PerspectiveDecl {
    read: Option<Expr>,
    write: Option<Expr>,
}

impl PerspectiveDecl {
    /// The read set expression, or `None` for the empty set.
    pub fn read(&self) -> Option<&Expr> {
        self.read.as_ref()
    }

    /// The write set expression, or `None` for the empty set.
    pub fn write(&self) -> Option<&Expr> {
        self.write.as_ref()
    }
}

/// The complete set of perspective declarations from a rulebook's
/// `[perspective]` table.
///
/// Wraps `BTreeMap<PerspectiveName, PerspectiveDecl>` for
/// deterministic ordering.
#[derive(Debug, Clone, Default)]
pub struct PerspectiveTable {
    declarations: BTreeMap<PerspectiveName, PerspectiveDecl>,
}

impl PerspectiveTable {
    /// The underlying declarations map.
    pub fn declarations(&self) -> &BTreeMap<PerspectiveName, PerspectiveDecl> {
        &self.declarations
    }

    /// Consume the table and return the inner map.
    pub fn into_declarations(self) -> BTreeMap<PerspectiveName, PerspectiveDecl> {
        self.declarations
    }
}

/// Raw TOML shape for a single perspective entry.
///
/// Both fields are optional. A missing field or an empty string
/// means the perspective claims no files for that role (empty set).
///
/// ```toml
/// [perspective.AuthImplementor]
/// read  = "AuthSpecs | AuthTests"
/// write = "AuthFiles - AuthSpecs - AuthTests"
///
/// [perspective.Observer]
/// read  = "AuthSpecs"
/// ```
#[derive(serde::Deserialize)]
struct RawPerspective {
    #[serde(default)]
    read: Option<String>,
    #[serde(default)]
    write: Option<String>,
}

impl PerspectiveTable {
    /// Parse an optional expression string into `Option<Expr>`.
    ///
    /// `None` or an empty/whitespace-only string yields `None` (empty set).
    /// A non-empty string is parsed as a file-set expression.
    fn parse_optional_expr(
        perspective: &PerspectiveName, raw: Option<&str>,
    ) -> Result<Option<Expr>, PerspectiveError> {
        match raw {
            | None => Ok(None),
            | Some(s) if s.trim().is_empty() => Ok(None),
            | Some(s) => {
                let expr = ExprParser::new(s).parse().map_err(|source| {
                    PerspectiveError::Parse { perspective: perspective.clone(), source }
                })?;
                Ok(Some(expr))
            }
        }
    }
}

impl<'de> de::Deserialize<'de> for PerspectiveTable {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let raw: BTreeMap<String, RawPerspective> = BTreeMap::deserialize(deserializer)?;

        let mut declarations = BTreeMap::new();
        for (key, value) in raw {
            let name = PerspectiveName::new(&key).map_err(de::Error::custom)?;

            let read = Self::parse_optional_expr(&name, value.read.as_deref())
                .map_err(de::Error::custom)?;

            let write = Self::parse_optional_expr(&name, value.write.as_deref())
                .map_err(de::Error::custom)?;

            declarations.insert(name, PerspectiveDecl { read, write });
        }

        Ok(PerspectiveTable { declarations })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::fileset::ParseError;
    use crate::schema::perspective::PerspectiveNameError;

    #[test]
    fn deserialize_design_doc_example() {
        let toml_str = r#"
            [AuthImplementor]
            read  = "AuthSpecs"
            write = "AuthFiles - AuthSpecs - AuthTests"

            [AuthTester]
            read  = "AuthSpecs | AuthTests"
            write = "AuthTests"
        "#;
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        let decls = table.declarations();

        assert_eq!(decls.len(), 2);
        assert!(decls.contains_key(&PerspectiveName::new("AuthImplementor").unwrap()));
        assert!(decls.contains_key(&PerspectiveName::new("AuthTester").unwrap()));
    }

    #[test]
    fn rejects_invalid_name() {
        assert!(matches!(
            PerspectiveName::new("lowercase"),
            Err(PerspectiveNameError::InvalidStart { .. })
        ));
    }

    #[test]
    fn rejects_invalid_read_expr() {
        assert!(matches!(ExprParser::new("A |").parse(), Err(ParseError::UnexpectedEof)));
    }

    #[test]
    fn rejects_invalid_write_expr() {
        assert!(matches!(
            ExprParser::new("| B").parse(),
            Err(ParseError::UnexpectedChar { ch: '|', .. })
        ));
    }

    #[test]
    fn missing_write_defaults_to_empty_set() {
        let toml_str = r#"
            [ReadOnly]
            read = "A"
        "#;
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        let decl = &table.declarations()[&PerspectiveName::new("ReadOnly").unwrap()];
        assert!(decl.read().is_some());
        assert!(decl.write().is_none());
    }

    #[test]
    fn missing_read_defaults_to_empty_set() {
        let toml_str = r#"
            [WriteOnly]
            write = "A"
        "#;
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        let decl = &table.declarations()[&PerspectiveName::new("WriteOnly").unwrap()];
        assert!(decl.read().is_none());
        assert!(decl.write().is_some());
    }

    #[test]
    fn empty_string_read_is_empty_set() {
        let toml_str = r#"
            [P]
            read = ""
            write = "A"
        "#;
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        let decl = &table.declarations()[&PerspectiveName::new("P").unwrap()];
        assert!(decl.read().is_none());
        assert!(decl.write().is_some());
    }

    #[test]
    fn empty_string_write_is_empty_set() {
        let toml_str = r#"
            [P]
            read = "A"
            write = ""
        "#;
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        let decl = &table.declarations()[&PerspectiveName::new("P").unwrap()];
        assert!(decl.read().is_some());
        assert!(decl.write().is_none());
    }

    #[test]
    fn both_empty_is_valid() {
        let toml_str = r#"
            [Bare]
        "#;
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        let decl = &table.declarations()[&PerspectiveName::new("Bare").unwrap()];
        assert!(decl.read().is_none());
        assert!(decl.write().is_none());
    }

    #[test]
    fn empty_table_is_valid() {
        let toml_str = "";
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        assert!(table.declarations().is_empty());
    }
}
