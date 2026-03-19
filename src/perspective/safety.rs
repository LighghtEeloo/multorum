//! Safety property validation for compiled perspectives.
//!
//! The safety property requires that for any two distinct perspectives
//! P and Q:
//!
//! - `write(P) ∩ write(Q) = ∅` — write sets are pairwise disjoint.
//! - `write(P) ∩ read(Q) = ∅` — no file is written by one and read
//!   by another.
//!
//! Validation runs after compilation and before the result is exposed
//! to callers, so [`CompiledPerspectives`](super::CompiledPerspectives)
//! always satisfies the safety property by construction.

use std::collections::BTreeMap;

use super::compile::CompiledPerspective;
use super::error::SafetyViolation;
use super::name::PerspectiveName;

/// Validates the safety property across a set of compiled perspectives.
pub struct SafetyValidator<'a> {
    perspectives: &'a BTreeMap<PerspectiveName, CompiledPerspective>,
}

impl<'a> SafetyValidator<'a> {
    /// Create a validator for the given compiled perspectives.
    pub fn new(
        perspectives: &'a BTreeMap<PerspectiveName, CompiledPerspective>,
    ) -> Self {
        Self { perspectives }
    }

    /// Validate the safety property.
    ///
    /// Returns `Ok(())` if all pairs satisfy the invariant, or the
    /// first violation found.
    pub fn validate(&self) -> Result<(), SafetyViolation> {
        let entries: Vec<_> = self.perspectives.iter().collect();
        for i in 0..entries.len() {
            for j in (i + 1)..entries.len() {
                let (name_p, p) = entries[i];
                let (name_q, q) = entries[j];
                self.check_pair(name_p, p, name_q, q)?;
            }
        }
        Ok(())
    }

    /// Check the safety property between two distinct perspectives.
    fn check_pair(
        &self,
        name_p: &PerspectiveName,
        p: &CompiledPerspective,
        name_q: &PerspectiveName,
        q: &CompiledPerspective,
    ) -> Result<(), SafetyViolation> {
        // write(P) ∩ write(Q) = ∅
        let ww: std::collections::BTreeSet<_> = p
            .write()
            .intersection(q.write())
            .cloned()
            .collect();
        if !ww.is_empty() {
            return Err(SafetyViolation::WriteWriteOverlap {
                left: name_p.clone(),
                right: name_q.clone(),
                files: ww,
            });
        }

        // write(P) ∩ read(Q) = ∅
        let wr: std::collections::BTreeSet<_> = p
            .write()
            .intersection(q.read())
            .cloned()
            .collect();
        if !wr.is_empty() {
            return Err(SafetyViolation::WriteReadOverlap {
                writer: name_p.clone(),
                reader: name_q.clone(),
                files: wr,
            });
        }

        // write(Q) ∩ read(P) = ∅
        let rw: std::collections::BTreeSet<_> = q
            .write()
            .intersection(p.read())
            .cloned()
            .collect();
        if !rw.is_empty() {
            return Err(SafetyViolation::WriteReadOverlap {
                writer: name_q.clone(),
                reader: name_p.clone(),
                files: rw,
            });
        }

        Ok(())
    }
}
