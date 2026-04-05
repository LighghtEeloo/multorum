//! File set algebra for Multorum rulebooks.
//!
//! This module implements the file set algebra described in the design
//! specification. File sets are a rulebook-level concept: they are
//! compiled into concrete file lists when a rulebook is activated and
//! do not exist at runtime.
//!
//! ## Compilation pipeline
//!
//! 1. Deserialize the `[fileset]` table from TOML.
//! 2. Parse expression strings into an AST.
//! 3. Validate: no cycles, no undefined references.
//! 4. Compile: expand globs, evaluate set operations.
//! 5. Produce a concrete `BTreeSet<PathBuf>` per named file set.

pub mod compile;
pub mod error;
pub mod expr;
pub mod name;
pub mod parse;
pub mod validate;

pub use compile::{Compiler, enumerate_files};
pub use error::*;
pub use expr::{Definition, DirectoryPath, Expr, FileSetTable, GlobPattern};
pub use name::Name;
pub use parse::ExprParser;
pub use validate::Validator;
