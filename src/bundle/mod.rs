//! General-purpose content bundle.
//!
//! A bundle is a directory containing a `body.md` primary content file
//! and an `artifacts/` subdirectory for supplementary files. Bundles
//! are the shared content container used by both the mailbox protocol
//! and audit entries — anywhere Multorum needs to atomically store
//! structured content with optional supplementary files.
//!
//! This module owns the bundle payload type, filesystem writer, and
//! error type. It is consumed by the runtime layer but has no
//! dependency on runtime types itself.

pub mod error;
pub mod payload;
pub mod writer;

pub use error::BundleError;
pub use payload::BundlePayload;
pub use writer::{ARTIFACTS_DIR_NAME, BODY_FILE_NAME, BundleWriter, WrittenBundle};
