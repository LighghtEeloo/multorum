//! Typed rulebook support for `.multorum/rulebook.toml`.
//!
//! A rulebook is the single committed configuration file that defines
//! ownership boundaries and the merge pipeline for a Multorum project.
//! It is the authoritative policy document that governs which workers
//! can write to which files and what checks must pass before code merges.
//!
//! ## Structure
//!
//! A rulebook contains three TOML sections:
//!
//! - **`[fileset]`** — Named file sets built from glob primitives and set
//!   algebra expressions (union `|`, intersection `&`, difference `-`).
//!   These form the project's vocabulary for describing regions of the
//!   repository.
//!
//! - **`[perspective.<Name>]`** — Named roles, each declaring a `write`
//!   set (files the role may modify) and a `read` set (files that must
//!   remain stable while the role is active). Perspectives are static
//!   policy; workers are their runtime instantiations.
//!
//! - **`[check]`** — An ordered pipeline of project-defined merge gates
//!   (formatting, linting, testing), each backed by a shell command and
//!   an optional skip policy.
//!
//! ## Lifecycle
//!
//! [`Rulebook`] models the raw committed TOML artifact.
//! [`CompiledRulebook`] is produced by compiling a `Rulebook` against a
//! concrete file list, expanding globs and evaluating set algebra into
//! `BTreeSet<PathBuf>` per file set, concrete read/write sets per
//! perspective, and a validated check pipeline.
//!
//! The rulebook module deliberately stops at typed loading and
//! compilation. Runtime state projection and group-level base commit
//! pinning belong to the orchestrator service.

pub mod check;
pub mod compile;
pub mod decl;
pub mod error;

pub use check::{CheckDecl, CheckName, CheckPolicy, CheckTable, CompiledChecks};
pub use compile::CompiledRulebook;
pub use decl::{RULEBOOK_RELATIVE_PATH, Rulebook};
pub use error::{CheckNameError, CheckValidationError, RulebookError};
