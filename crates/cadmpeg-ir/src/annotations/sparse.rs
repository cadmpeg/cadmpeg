// SPDX-License-Identifier: Apache-2.0
//! Transactions that copy only the identities selected for editing.

use super::{AnnotationBuilder, Annotations};
use crate::provenance::Exactness;
use std::collections::BTreeSet;

/// A sparse copy of selected annotation identities.
/// The base remains borrowed and unchanged until preparation succeeds.
#[derive(Debug)]
pub struct SparseAnnotationTransaction<'base> {
    base: &'base Annotations,
    changes: Annotations,
    selected: BTreeSet<String>,
}

/// An admitted annotation replacement with no remaining fallible application work.
#[derive(Debug)]
pub struct PreparedAnnotationDelta {
    changes: Annotations,
    selected: BTreeSet<String>,
}

impl Annotations {
    /// Start a transaction without scanning or copying the existing tables.
    pub fn sparse_transaction(&self) -> SparseAnnotationTransaction<'_> {
        SparseAnnotationTransaction {
            base: self,
            changes: Annotations::default(),
            selected: BTreeSet::new(),
        }
    }
    /// Prepare owned complete replacements without copying the prior tables.
    /// Entries absent from `changes` remain unchanged.
    pub fn prepare_overlay(&self, changes: Annotations) -> PreparedAnnotationDelta {
        let mut transaction = self.sparse_transaction();
        for id in changes.provenance.keys().chain(changes.exactness.keys()) {
            transaction.selected.insert(id.to_owned());
        }
        transaction.changes = changes;
        transaction.prepare()
    }
}

impl<'base> SparseAnnotationTransaction<'base> {
    /// Borrow the unchanged base tables.
    pub fn base(&self) -> &'base Annotations {
        self.base
    }

    /// Read only the selected candidate annotations.
    pub fn annotations(&self) -> &Annotations {
        &self.changes
    }

    /// Copy one identity's annotations once, then edit them.
    /// The callback may change or remove only entries at `id`.
    fn edit<T>(&mut self, id: &str, apply: impl FnOnce(&mut Annotations) -> T) -> T {
        if self.selected.insert(id.to_owned()) {
            copy_identity(self.base, &mut self.changes, id);
        }
        apply(&mut self.changes)
    }

    pub(crate) fn select(&mut self, id: &str) {
        self.edit(id, |_| ());
    }

    pub(crate) fn selected(&self, id: &str) -> bool {
        self.selected.contains(id)
    }

    /// Set a field override without copying other entity annotations.
    pub fn field_exactness(
        &mut self,
        id: &str,
        field: &str,
        exactness: Exactness,
    ) -> Result<(), super::AnnotationFieldError> {
        self.edit(id, |annotations| {
            let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
            let result = builder.field_exactness(id, field, exactness).map(|_| ());
            *annotations = builder.build();
            result
        })
    }

    /// Replace provenance for one selected identity.
    pub fn note(&mut self, id: &str, stream: &super::StreamHandle, offset: u64, tag: Option<&str>) {
        self.edit(id, |annotations| {
            let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
            builder.note(id, stream, offset, tag);
            *annotations = builder.build();
        });
    }

    /// Remove both annotation entries at one selected identity.
    pub fn remove_entity(&mut self, id: &str) {
        self.edit(id, |annotations| {
            annotations.provenance.remove(id);
            annotations.exactness.remove(id);
        });
    }

    /// Set exactness while preserving existing field overrides at the selected identity.
    pub fn exactness(&mut self, id: &str, exactness: Exactness) {
        self.edit(id, |annotations| {
            let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
            builder.exactness(id, exactness);
            *annotations = builder.build();
        });
    }

    /// Finish the transaction before either table changes.
    pub fn prepare(self) -> PreparedAnnotationDelta {
        PreparedAnnotationDelta {
            changes: self.changes,
            selected: self.selected,
        }
    }
}

impl PreparedAnnotationDelta {
    /// Apply the admitted replacements and deletions without rebuilding either table.
    /// The destination must be the unchanged base used for preparation. Moving
    /// the base is permitted; intervening annotation edits are not.
    pub fn apply(mut self, annotations: &mut Annotations) {
        for id in self.selected {
            if let Some((key, value)) = self.changes.provenance.remove_entry(&id) {
                annotations.provenance.insert(key, value);
            } else {
                annotations.provenance.remove(&id);
            }
            if let Some((key, value)) = self.changes.exactness.remove_entry(&id) {
                annotations.exactness.insert(key, value);
            } else {
                annotations.exactness.remove(&id);
            }
        }
    }
}

fn copy_identity(base: &Annotations, changes: &mut Annotations, id: &str) {
    if let Some(source) = base.provenance.get(id) {
        changes.provenance.insert(id.to_owned(), source.clone());
    }
    if let Some(note) = base.exactness.get(id) {
        changes.exactness.insert(id.to_owned(), note.clone());
    }
}

#[cfg(test)]
mod tests;
