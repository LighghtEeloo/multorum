//! The [`PerspectiveName`] newtype — a validated perspective identifier.

use std::{fmt, str::FromStr};

use serde::de;

use super::error::PerspectiveNameError;

/// A validated perspective identifier (e.g. `AuthImplementor`, `AuthTester`).
///
/// ## Invariants
///
/// - Non-empty.
/// - Starts with an uppercase ASCII letter (`A`–`Z`).
/// - Contains only ASCII alphanumeric characters (`A`–`Z`, `a`–`z`, `0`–`9`).
///
/// Note: These are the same lexical rules as [`fileset::Name`](crate::fileset::Name),
/// but a distinct type to prevent mixing file set names with perspective names.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
pub struct PerspectiveName(String);

impl PerspectiveName {
    /// Create a new `PerspectiveName`, validating the identifier invariants.
    pub fn new(s: &str) -> Result<Self, PerspectiveNameError> {
        let first = s.chars().next().ok_or(PerspectiveNameError::Empty)?;
        if !first.is_ascii_uppercase() {
            return Err(PerspectiveNameError::InvalidStart { name: s.to_owned() });
        }
        for (pos, ch) in s.char_indices().skip(1) {
            if !ch.is_ascii_alphanumeric() {
                return Err(PerspectiveNameError::InvalidChar { name: s.to_owned(), ch, pos });
            }
        }
        Ok(Self(s.to_owned()))
    }

    /// The identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for PerspectiveName {
    type Err = PerspectiveNameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl fmt::Display for PerspectiveName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> de::Deserialize<'de> for PerspectiveName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        PerspectiveName::new(&s).map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_names() {
        assert!(PerspectiveName::new("A").is_ok());
        assert!(PerspectiveName::new("AuthImplementor").is_ok());
        assert!(PerspectiveName::new("AuthTester2").is_ok());
    }

    #[test]
    fn empty_is_rejected() {
        assert_eq!(PerspectiveName::new(""), Err(PerspectiveNameError::Empty));
    }

    #[test]
    fn lowercase_start_is_rejected() {
        let err = PerspectiveName::new("authImpl").unwrap_err();
        assert!(matches!(err, PerspectiveNameError::InvalidStart { .. }));
    }

    #[test]
    fn underscore_is_rejected() {
        let err = PerspectiveName::new("Auth_Impl").unwrap_err();
        assert!(matches!(err, PerspectiveNameError::InvalidChar { ch: '_', pos: 4, .. }));
    }

    #[test]
    fn display_roundtrip() {
        let name = PerspectiveName::new("AuthImplementor").unwrap();
        assert_eq!(name.to_string(), "AuthImplementor");
        assert_eq!(name.as_str(), "AuthImplementor");
    }

    #[test]
    fn distinct_from_fileset_name() {
        // Both types accept the same string but are not interchangeable.
        let fs_name = crate::fileset::Name::new("AuthFiles").unwrap();
        let ps_name = PerspectiveName::new("AuthFiles").unwrap();
        // They share the same string representation but different types.
        assert_eq!(fs_name.as_str(), ps_name.as_str());
    }
}
