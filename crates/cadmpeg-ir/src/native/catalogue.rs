// SPDX-License-Identifier: Apache-2.0
//! Declarative native-family catalogues.

use super::NativeConvertError;

/// Ordered processing phase and annotation function for a native record family.
pub enum Phase<M, A, N, E> {
    /// Families handled before the first codec semantic island.
    GroupA {
        /// Optional standard annotation tag.
        tag: Option<&'static str>,
        /// Annotation function for this family.
        note: NoteFn<M, A, N, E>,
    },
    /// Families handled between codec semantic islands.
    GroupB {
        /// Optional standard annotation tag.
        tag: Option<&'static str>,
        /// Annotation function for this family.
        note: NoteFn<M, A, N, E>,
    },
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
pub type NoteFn<M, A, N, E> = fn(&M, &FamilyRow<M, A, N, E>, Option<&'static str>, &mut A);

/// Namespace-emission function carried by a family row.
pub type EmitFn<M, A, N, E> =
    fn(&M, &FamilyRow<M, A, N, E>, &mut N) -> Result<(), NativeConvertError>;

/// One codec-owned native record family.
pub struct FamilyRow<M, A, N, E> {
    /// Native namespace arena name.
    pub arena: &'static str,
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
}

impl<'a, M, A, N, E> Catalogue<'a, M, A, N, E> {
    /// Wraps a statically declared family table.
    pub const fn new(rows: &'a [FamilyRow<M, A, N, E>]) -> Self {
        Self { rows }
    }

    /// Returns the declared rows in stable order.
    pub const fn rows(&self) -> &'a [FamilyRow<M, A, N, E>] {
        self.rows
    }

    /// Emits every family through its row function, empty families included.
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
                (Phase::GroupA { tag, note }, NotePhase::GroupA)
                | (Phase::GroupB { tag, note }, NotePhase::GroupB) => {
                    note(model, row, *tag, annotations);
                }
                (Phase::GroupA { .. } | Phase::GroupB { .. } | Phase::ArenaOnly, _) => {}
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
