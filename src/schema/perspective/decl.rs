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
#[derive(Debug, Clone)]
pub struct PerspectiveDecl {
    read: Expr,
    write: Expr,
}

impl PerspectiveDecl {
    /// The read set expression.
    pub fn read(&self) -> &Expr {
        &self.read
    }

    /// The write set expression.
    pub fn write(&self) -> &Expr {
        &self.write
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
/// ```toml
/// [perspective.AuthImplementor]
/// read  = "AuthSpecs | AuthTests"
/// write = "AuthFiles - AuthSpecs - AuthTests"
/// ```
#[derive(serde::Deserialize)]
struct RawPerspective {
    read: String,
    write: String,
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

            let read = ExprParser::new(&value.read).parse().map_err(|e| {
                de::Error::custom(PerspectiveError::Parse { perspective: name.clone(), source: e })
            })?;

            let write = ExprParser::new(&value.write).parse().map_err(|e| {
                de::Error::custom(PerspectiveError::Parse { perspective: name.clone(), source: e })
            })?;

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
    fn deserialize_rejects_missing_write() {
        let toml_str = r#"
            [Bad]
            read = "A"
        "#;
        assert!(toml::from_str::<PerspectiveTable>(toml_str).is_err());
    }

    #[test]
    fn deserialize_rejects_missing_read() {
        let toml_str = r#"
            [Bad]
            write = "A"
        "#;
        assert!(toml::from_str::<PerspectiveTable>(toml_str).is_err());
    }

    #[test]
    fn empty_table_is_valid() {
        let toml_str = "";
        let table: PerspectiveTable = toml::from_str(toml_str).unwrap();
        assert!(table.declarations().is_empty());
    }
}
