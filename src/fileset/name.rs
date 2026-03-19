//! The [`Name`] newtype — a validated file set identifier.

use std::fmt;

use serde::de;

use super::error::NameError;

/// A validated file set identifier (e.g. `AuthFiles`, `SpecFiles`).
///
/// ## Invariants
///
/// - Non-empty.
/// - Starts with an uppercase ASCII letter (`A`–`Z`).
/// - Contains only ASCII alphanumeric characters (`A`–`Z`, `a`–`z`, `0`–`9`).
///
/// Construct via [`Name::new`], which validates these invariants.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Name(String);

impl Name {
    /// Create a new `Name`, validating the identifier invariants.
    pub fn new(s: &str) -> Result<Self, NameError> {
        let first = s.chars().next().ok_or(NameError::Empty)?;
        if !first.is_ascii_uppercase() {
            return Err(NameError::InvalidStart {
                name: s.to_owned(),
            });
        }
        for (pos, ch) in s.char_indices().skip(1) {
            if !ch.is_ascii_alphanumeric() {
                return Err(NameError::InvalidChar {
                    name: s.to_owned(),
                    ch,
                    pos,
                });
            }
        }
        Ok(Self(s.to_owned()))
    }

    /// The identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> de::Deserialize<'de> for Name {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Name::new(&s).map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_names() {
        assert!(Name::new("A").is_ok());
        assert!(Name::new("AuthFiles").is_ok());
        assert!(Name::new("SpecFiles2").is_ok());
        assert!(Name::new("X99").is_ok());
    }

    #[test]
    fn empty_is_rejected() {
        assert_eq!(Name::new(""), Err(NameError::Empty));
    }

    #[test]
    fn lowercase_start_is_rejected() {
        let err = Name::new("authFiles").unwrap_err();
        assert!(matches!(err, NameError::InvalidStart { .. }));
    }

    #[test]
    fn digit_start_is_rejected() {
        let err = Name::new("1Files").unwrap_err();
        assert!(matches!(err, NameError::InvalidStart { .. }));
    }

    #[test]
    fn underscore_is_rejected() {
        let err = Name::new("Auth_Files").unwrap_err();
        assert!(matches!(
            err,
            NameError::InvalidChar {
                ch: '_',
                pos: 4,
                ..
            }
        ));
    }

    #[test]
    fn space_is_rejected() {
        let err = Name::new("Auth Files").unwrap_err();
        assert!(matches!(
            err,
            NameError::InvalidChar {
                ch: ' ',
                pos: 4,
                ..
            }
        ));
    }

    #[test]
    fn display_roundtrip() {
        let name = Name::new("AuthFiles").unwrap();
        assert_eq!(name.to_string(), "AuthFiles");
        assert_eq!(name.as_str(), "AuthFiles");
    }
}
