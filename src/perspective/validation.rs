//! Conflict-free validation for compiled perspectives.
//!
//! The conflict-free invariant requires that for any two distinct
//! boundaries `P` and `Q`:
//!
//! - `write(P) ∩ write(Q) = ∅` — write sets are pairwise disjoint.
//! - `write(P) ∩ read(Q) = ∅` — no file is written by one and read
//!   by another.
//!
//! Runtime code uses this helper when it needs to compare concrete
//! perspective or bidding-group boundaries.

use std::collections::BTreeMap;

use super::compile::CompiledPerspective;
use super::error::ConflictViolation;
use super::name::PerspectiveName;

/// Validates the conflict-free invariant across compiled boundaries.
pub struct ConflictFreeValidator<'a> {
    perspectives: &'a BTreeMap<PerspectiveName, CompiledPerspective>,
}

impl<'a> ConflictFreeValidator<'a> {
    /// Create a validator for the given compiled perspectives.
    pub fn new(perspectives: &'a BTreeMap<PerspectiveName, CompiledPerspective>) -> Self {
        Self { perspectives }
    }

    /// Validate the conflict-free invariant.
    ///
    /// Returns `Ok(())` if all pairs satisfy the invariant, or the
    /// first violation found.
    pub fn validate(&self) -> Result<(), ConflictViolation> {
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

    /// Check the conflict-free invariant between two perspectives.
    fn check_pair(
        &self, name_p: &PerspectiveName, p: &CompiledPerspective, name_q: &PerspectiveName,
        q: &CompiledPerspective,
    ) -> Result<(), ConflictViolation> {
        // write(P) ∩ write(Q) = ∅
        let ww: std::collections::BTreeSet<_> =
            p.write().intersection(q.write()).cloned().collect();
        if !ww.is_empty() {
            return Err(ConflictViolation::WriteWriteOverlap {
                left: name_p.clone(),
                right: name_q.clone(),
                files: ww,
            });
        }

        // write(P) ∩ read(Q) = ∅
        let wr: std::collections::BTreeSet<_> = p.write().intersection(q.read()).cloned().collect();
        if !wr.is_empty() {
            return Err(ConflictViolation::WriteReadOverlap {
                writer: name_p.clone(),
                reader: name_q.clone(),
                files: wr,
            });
        }

        // write(Q) ∩ read(P) = ∅
        let rw: std::collections::BTreeSet<_> = q.write().intersection(p.read()).cloned().collect();
        if !rw.is_empty() {
            return Err(ConflictViolation::WriteReadOverlap {
                writer: name_q.clone(),
                reader: name_p.clone(),
                files: rw,
            });
        }

        Ok(())
    }
}
