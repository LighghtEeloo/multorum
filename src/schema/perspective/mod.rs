//! Perspectives for Multorum rulebooks.
//!
//! A perspective is a named declaration that binds a role to a write
//! set and a read set, both expressed as file set algebra expressions.
//! Perspectives are compiled against a pre-compiled file set table to
//! produce concrete file lists. Runtime services later use those
//! compiled lists when they validate bidding-group conflict freedom
//! against the active workers that already exist.

pub mod compile;
pub mod decl;
pub mod error;
pub mod name;

pub use compile::{CompiledPerspective, CompiledPerspectives};
pub use decl::{PerspectiveDecl, PerspectiveTable};
pub use error::*;
pub use name::PerspectiveName;
