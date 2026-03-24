//! Schema definitions for Multorum policy.
//!
//! This module aggregates the three core policy artifacts:
//! - [`fileset`] — file-set algebra and compilation
//! - [`perspective`] — role declarations and compilation
//! - [`rulebook`] — the aggregate policy document and check pipeline

pub mod fileset;
pub mod perspective;
pub mod rulebook;
