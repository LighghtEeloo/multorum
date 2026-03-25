//! Typed rulebook support for `.multorum/rulebook.toml`.
//!
//! This module is the aggregate entry point for rulebook loading and
//! compilation. A [`Rulebook`] models the committed TOML artifact,
//! while [`CompiledRulebook`] exposes the concrete file sets,
//! perspectives, and the validated `[check]` pipeline that runtime code consumes.
//!
//! The rulebook module deliberately stops at typed loading and
//! compilation. Runtime state projection and group-level base commit pinning
//! belong to the orchestrator service.

pub mod check;
pub mod compile;
pub mod decl;
pub mod error;

pub use check::{CheckDecl, CheckName, CheckPolicy, CheckTable, CompiledChecks};
pub use compile::CompiledRulebook;
pub use decl::{RULEBOOK_RELATIVE_PATH, Rulebook};
pub use error::{CheckNameError, CheckValidationError, RulebookError};
