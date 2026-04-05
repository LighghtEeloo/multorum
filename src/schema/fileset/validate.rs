//! Validation of file set definitions: cycle detection and undefined
//! reference checking.
//!
//! Takes a `BTreeMap<Name, Definition>` and verifies structural
//! correctness before compilation. On success, returns a topological
//! ordering of names (dependencies before dependents).

use std::collections::{BTreeMap, BTreeSet};

use super::error::ValidationError;
use super::expr::{Definition, DirectoryPath, Expr};
use super::name::Name;

/// Validates a set of file set definitions for structural correctness.
///
/// ## Checks
///
/// - **No undefined references**: every name used in an expression is
///   defined in the map.
/// - **No cycles**: no name references itself directly or transitively.
///
/// On success, returns a topological ordering suitable for
/// compilation (dependencies before dependents).
pub struct Validator<'a> {
    definitions: &'a BTreeMap<Name, Definition>,
}

impl<'a> Validator<'a> {
    /// Create a validator for the given definitions.
    pub fn new(definitions: &'a BTreeMap<Name, Definition>) -> Self {
        Self { definitions }
    }

    /// Validate and return the topological ordering.
    pub fn validate(self) -> Result<Vec<Name>, ValidationError> {
        self.check_undefined()?;
        self.check_overlapping_opaques()?;
        self.topological_sort()
    }

    /// Check that every name referenced in compound expressions exists
    /// in the definitions map.
    fn check_undefined(&self) -> Result<(), ValidationError> {
        for (name, def) in self.definitions {
            if let Definition::Compound(expr) = def {
                let refs = Self::collect_references(expr);
                for r in refs {
                    if !self.definitions.contains_key(&r) {
                        return Err(ValidationError::Undefined {
                            name: r,
                            referenced_by: name.clone(),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// Verify that no two opaque directory prefixes overlap (neither is
    /// a prefix of the other).
    fn check_overlapping_opaques(&self) -> Result<(), ValidationError> {
        let opaques: Vec<(&Name, &DirectoryPath)> = self
            .definitions
            .iter()
            .filter_map(|(name, def)| match def {
                | Definition::Opaque(dir) => Some((name, dir)),
                | _ => None,
            })
            .collect();

        for i in 0..opaques.len() {
            for j in (i + 1)..opaques.len() {
                let (name_a, dir_a) = opaques[i];
                let (name_b, dir_b) = opaques[j];
                if dir_a.as_str().starts_with(dir_b.as_str())
                    || dir_b.as_str().starts_with(dir_a.as_str())
                {
                    return Err(ValidationError::OverlappingOpaques {
                        a: name_a.clone(),
                        b: name_b.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Topological sort via DFS with three-color marking.
    ///
    /// White = unvisited, Gray = in current DFS path, Black = finished.
    /// A gray→gray edge means a cycle.
    fn topological_sort(&self) -> Result<Vec<Name>, ValidationError> {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Color {
            White,
            Gray,
            Black,
        }

        let mut color: BTreeMap<&Name, Color> =
            self.definitions.keys().map(|n| (n, Color::White)).collect();
        let mut order = Vec::with_capacity(self.definitions.len());
        // Track the DFS path for cycle reporting.
        let mut path: Vec<&Name> = Vec::new();

        fn visit<'a>(
            name: &'a Name, definitions: &'a BTreeMap<Name, Definition>,
            color: &mut BTreeMap<&'a Name, Color>, order: &mut Vec<Name>, path: &mut Vec<&'a Name>,
        ) -> Result<(), ValidationError> {
            match color.get(name) {
                | Some(Color::Black) => return Ok(()),
                | Some(Color::Gray) => {
                    // Build the cycle string from the path.
                    let start = path.iter().position(|n| *n == name).unwrap();
                    let cycle: Vec<&str> = path[start..]
                        .iter()
                        .map(|n| n.as_str())
                        .chain(std::iter::once(name.as_str()))
                        .collect();
                    return Err(ValidationError::Cycle { cycle: cycle.join(" -> ") });
                }
                | _ => {}
            }

            color.insert(name, Color::Gray);
            path.push(name);

            if let Some(Definition::Compound(expr)) = definitions.get(name) {
                let refs = Validator::collect_references(expr);
                for r in refs {
                    // Look up the key in the map to obtain a reference
                    // with lifetime 'a (the local `r` is owned and
                    // would not live long enough).
                    let r = definitions.keys().find(|k| **k == r).unwrap();
                    visit(r, definitions, color, order, path)?;
                }
            }

            path.pop();
            color.insert(name, Color::Black);
            order.push(name.clone());
            Ok(())
        }

        let names: Vec<&Name> = self.definitions.keys().collect();
        for name in names {
            if color[name] == Color::White {
                visit(name, self.definitions, &mut color, &mut order, &mut path)?;
            }
        }

        Ok(order)
    }

    /// Collect all name references in an expression.
    fn collect_references(expr: &Expr) -> BTreeSet<Name> {
        let mut refs = BTreeSet::new();
        Self::collect_refs_inner(expr, &mut refs);
        refs
    }

    fn collect_refs_inner(expr: &Expr, refs: &mut BTreeSet<Name>) {
        match expr {
            | Expr::Ref(name) => {
                refs.insert(name.clone());
            }
            | Expr::Union(a, b) | Expr::Intersection(a, b) | Expr::Difference(a, b) => {
                Self::collect_refs_inner(a, refs);
                Self::collect_refs_inner(b, refs);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::fileset::{ExprParser, GlobPattern};

    fn n(s: &str) -> Name {
        Name::new(s).unwrap()
    }

    fn prim(pattern: &str) -> Definition {
        Definition::Primitive(GlobPattern::new(pattern).unwrap())
    }

    fn opaque(path: &str) -> Definition {
        Definition::Opaque(DirectoryPath::new(path).unwrap())
    }

    fn compound(expr: &str) -> Definition {
        Definition::Compound(ExprParser::new(expr).parse().unwrap())
    }

    #[test]
    fn valid_dag() {
        let defs = BTreeMap::from([
            (n("AuthFiles"), prim("auth/**")),
            (n("SpecFiles"), prim("**/*.spec.md")),
            (n("AuthSpecs"), compound("AuthFiles & SpecFiles")),
        ]);
        let order = Validator::new(&defs).validate().unwrap();
        // AuthSpecs must come after both AuthFiles and SpecFiles.
        let pos = |name: &str| order.iter().position(|n| n.as_str() == name).unwrap();
        assert!(pos("AuthFiles") < pos("AuthSpecs"));
        assert!(pos("SpecFiles") < pos("AuthSpecs"));
    }

    #[test]
    fn all_primitives() {
        let defs = BTreeMap::from([(n("A"), prim("a/**")), (n("B"), prim("b/**"))]);
        let order = Validator::new(&defs).validate().unwrap();
        assert_eq!(order.len(), 2);
    }

    #[test]
    fn undefined_reference() {
        let defs = BTreeMap::from([(n("X"), compound("Y & Z"))]);
        let err = Validator::new(&defs).validate().unwrap_err();
        assert!(matches!(err, ValidationError::Undefined { .. }));
    }

    #[test]
    fn direct_cycle() {
        // A depends on A.
        let defs = BTreeMap::from([(n("A"), compound("A"))]);
        let err = Validator::new(&defs).validate().unwrap_err();
        assert!(matches!(err, ValidationError::Cycle { .. }));
    }

    #[test]
    fn transitive_cycle() {
        // A -> B -> C -> A
        let defs = BTreeMap::from([
            (n("A"), compound("B")),
            (n("B"), compound("C")),
            (n("C"), compound("A")),
        ]);
        let err = Validator::new(&defs).validate().unwrap_err();
        match &err {
            | ValidationError::Cycle { cycle } => {
                // The cycle should contain A, B, C.
                assert!(cycle.contains("A"));
                assert!(cycle.contains("B"));
                assert!(cycle.contains("C"));
            }
            | _ => panic!("expected Cycle error"),
        }
    }

    #[test]
    fn opaques_are_treated_as_primitives() {
        let defs = BTreeMap::from([
            (n("Vendor"), opaque("vendor/")),
            (n("AuthFiles"), prim("auth/**")),
            (n("Combined"), compound("AuthFiles | Vendor")),
        ]);
        let order = Validator::new(&defs).validate().unwrap();
        let pos = |name: &str| order.iter().position(|n| n.as_str() == name).unwrap();
        assert!(pos("Vendor") < pos("Combined"));
        assert!(pos("AuthFiles") < pos("Combined"));
    }

    #[test]
    fn disjoint_opaques_are_valid() {
        let defs = BTreeMap::from([
            (n("VendorA"), opaque("vendor/a/")),
            (n("VendorB"), opaque("vendor/b/")),
        ]);
        assert!(Validator::new(&defs).validate().is_ok());
    }

    #[test]
    fn overlapping_opaques_rejected() {
        let defs =
            BTreeMap::from([(n("Outer"), opaque("vendor/")), (n("Inner"), opaque("vendor/sub/"))]);
        let err = Validator::new(&defs).validate().unwrap_err();
        assert!(matches!(err, ValidationError::OverlappingOpaques { .. }));
    }

    #[test]
    fn equivalent_opaques_rejected_after_normalization() {
        let defs =
            BTreeMap::from([(n("VendorA"), opaque("vendor")), (n("VendorB"), opaque("vendor/"))]);
        let err = Validator::new(&defs).validate().unwrap_err();
        assert!(matches!(err, ValidationError::OverlappingOpaques { .. }));
    }

    #[test]
    fn undefined_reference_reports_referenced_by_name() {
        let defs = BTreeMap::from([
            (n("Known"), prim("src/**")),
            (n("UsesMissing"), compound("Known | Missing")),
        ]);
        let err = Validator::new(&defs).validate().unwrap_err();
        match err {
            | ValidationError::Undefined { name, referenced_by } => {
                assert_eq!(name.as_str(), "Missing");
                assert_eq!(referenced_by.as_str(), "UsesMissing");
            }
            | other => panic!("expected Undefined, got {other:?}"),
        }
    }

    #[test]
    fn design_doc_example() {
        let defs = BTreeMap::from([
            (n("SpecFiles"), prim("**/*.spec.md")),
            (n("TestFiles"), prim("**/test/**")),
            (n("AuthFiles"), prim("auth/**")),
            (n("AuthSpecs"), compound("AuthFiles & SpecFiles")),
            (n("AuthTests"), compound("AuthFiles & TestFiles")),
        ]);
        let order = Validator::new(&defs).validate().unwrap();
        assert_eq!(order.len(), 5);

        let pos = |name: &str| order.iter().position(|n| n.as_str() == name).unwrap();
        assert!(pos("AuthFiles") < pos("AuthSpecs"));
        assert!(pos("SpecFiles") < pos("AuthSpecs"));
        assert!(pos("AuthFiles") < pos("AuthTests"));
        assert!(pos("TestFiles") < pos("AuthTests"));
    }
}
