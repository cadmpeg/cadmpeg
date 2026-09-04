// SPDX-License-Identifier: Apache-2.0
//! Declarative native-family catalogues and version contracts.

use std::num::NonZeroU32;
use std::ops::RangeInclusive;

/// Ordered processing phase and annotation function for a native record family.
pub enum Phase<M, A, N, E> {
    /// Families handled before the first codec semantic island.
    GroupA(NoteFn<M, A, N, E>),
    /// Families handled between codec semantic islands.
    GroupB(NoteFn<M, A, N, E>),
    /// Families emitted without catalogue-driven annotations.
    ArenaOnly,
}

/// Annotation pass selected by a codec semantic island.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotePhase {
    /// Emit annotations before the first semantic island.
    GroupA,
    /// Emit annotations between semantic islands.
    GroupB,
}

/// Annotation function carried by a family row.
pub type NoteFn<M, A, N, E> = fn(&M, &FamilyRow<M, A, N, E>, &mut A);

/// Namespace-emission function carried by a family row.
pub type EmitFn<M, A, N, E> =
    fn(&M, &FamilyRow<M, A, N, E>, &mut N) -> Result<(), NativeConvertError>;

/// One codec-owned native record family.
pub struct FamilyRow<M, A, N, E> {
    /// Native namespace arena name.
    pub arena: &'static str,
    /// Optional standard annotation tag.
    pub tag: Option<&'static str>,
    /// Codec-selected exactness metadata.
    pub exactness: E,
    /// Ordered processing phase.
    pub phase: Phase<M, A, N, E>,
    /// Serializes this family into a native namespace.
    pub emit: EmitFn<M, A, N, E>,
    /// Returns this family's record count.
    pub len: fn(&M) -> usize,
    /// Whether this family contributes to the codec's historical emptiness predicate.
    pub counts_toward_emptiness: bool,
}

/// A complete ordered native-family catalogue.
pub struct Catalogue<'a, M, A, N, E> {
    rows: &'a [FamilyRow<M, A, N, E>],
    version: Option<VersionContract>,
}

impl<'a, M, A, N, E> Catalogue<'a, M, A, N, E> {
    /// Wraps a statically declared family table.
    ///
    /// `None` is an unbounded codec: every stored nonzero namespace version
    /// is accepted.
    pub const fn new(rows: &'a [FamilyRow<M, A, N, E>], version: Option<VersionContract>) -> Self {
        Self { rows, version }
    }

    /// Returns the declared rows in stable order.
    pub const fn rows(&self) -> &'a [FamilyRow<M, A, N, E>] {
        self.rows
    }

    /// Checks a native namespace version against this catalogue's contract.
    pub const fn check_version(&self, version: u32) -> Result<(), NativeVersionError> {
        match self.version {
            Some(contract) => contract.check_version(version),
            None => Ok(()),
        }
    }

    /// Emits every non-empty family through its row function.
    pub fn emit_all(&self, model: &M, namespace: &mut N) -> Result<(), NativeConvertError> {
        for row in self.rows {
            (row.emit)(model, row, namespace)?;
        }
        Ok(())
    }

    /// Emits annotations for every family in one phase.
    pub fn note_phase(&self, phase: NotePhase, model: &M, annotations: &mut A) {
        for row in self.rows {
            match (&row.phase, phase) {
                (Phase::GroupA(note), NotePhase::GroupA)
                | (Phase::GroupB(note), NotePhase::GroupB) => note(model, row, annotations),
                (Phase::GroupA(_) | Phase::GroupB(_) | Phase::ArenaOnly, _) => {}
            }
        }
    }

    /// Returns whether every family participating in emptiness is empty.
    pub fn is_empty(&self, model: &M) -> bool {
        self.rows
            .iter()
            .filter(|row| row.counts_toward_emptiness)
            .all(|row| (row.len)(model) == 0)
    }
}

/// Inclusive native namespace version range of nonzero bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionContract {
    minimum: NonZeroU32,
    maximum: NonZeroU32,
}

impl VersionContract {
    /// Inclusive range whose lower bound is at most its upper bound.
    ///
    /// # Panics
    ///
    /// Panics when `minimum` is greater than `maximum`.
    pub const fn new(minimum: NonZeroU32, maximum: NonZeroU32) -> Self {
        assert!(
            minimum.get() <= maximum.get(),
            "native version contract minimum must not exceed maximum"
        );
        Self { minimum, maximum }
    }

    /// Oldest accepted version.
    #[must_use]
    pub const fn minimum(self) -> NonZeroU32 {
        self.minimum
    }

    /// Newest accepted version.
    #[must_use]
    pub const fn maximum(self) -> NonZeroU32 {
        self.maximum
    }

    /// Inclusive accepted range.
    #[must_use]
    pub const fn range(self) -> RangeInclusive<NonZeroU32> {
        self.minimum..=self.maximum
    }

    /// Accepts `version` when it lies inside the inclusive contract.
    pub const fn check_version(self, version: u32) -> Result<(), NativeVersionError> {
        let minimum = self.minimum.get();
        let maximum = self.maximum.get();
        if version < minimum || version > maximum {
            Err(NativeVersionError::Unsupported {
                version,
                minimum,
                maximum,
            })
        } else {
            Ok(())
        }
    }
}

/// Native namespace version refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NativeVersionError {
    /// Version lies outside the codec's declared inclusive range.
    #[error("unsupported native version {version}; accepted range is {minimum}..={maximum}")]
    Unsupported {
        /// Refused version.
        version: u32,
        /// Oldest accepted version.
        minimum: u32,
        /// Newest accepted version.
        maximum: u32,
    },
}

use super::NativeConvertError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_contract_accepts_only_its_inclusive_range() {
        let contract =
            VersionContract::new(NonZeroU32::new(4).unwrap(), NonZeroU32::new(13).unwrap());
        assert!(contract.check_version(4).is_ok());
        assert!(contract.check_version(13).is_ok());
        assert!(matches!(
            contract.check_version(3),
            Err(NativeVersionError::Unsupported {
                version: 3,
                minimum: 4,
                maximum: 13
            })
        ));
        assert!(matches!(
            contract.check_version(14),
            Err(NativeVersionError::Unsupported {
                version: 14,
                minimum: 4,
                maximum: 13
            })
        ));
    }
}
