//! Perspectives for Multorum rulebooks.
//!
//! A perspective is a named declaration that binds a role to a write
//! set and a read set, both expressed as file set algebra expressions.
//! Perspectives are compiled against a pre-compiled file set table to
//! produce concrete file lists, then validated against the safety
//! property.
//!
//! ## Safety property
//!
//! For any two distinct perspectives P and Q:
//!
//! - `write(P) ∩ write(Q) = ∅` — write sets are pairwise disjoint.
//! - `write(P) ∩ read(Q) = ∅` — no file is written by one and read
//!   by another.
//!
//! This is enforced statically at compile time by [`SafetyValidator`].

pub mod compile;
pub mod decl;
pub mod error;
pub mod name;
pub mod safety;

pub use compile::{CompiledPerspective, CompiledPerspectives};
pub use decl::{PerspectiveDecl, PerspectiveTable};
pub use error::*;
pub use name::PerspectiveName;
pub use safety::SafetyValidator;
