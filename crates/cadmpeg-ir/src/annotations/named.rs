// SPDX-License-Identifier: Apache-2.0
//! The records of a document that annotation keys name (§3.2), and the
//! removal of the annotations of records that leave a document.

use std::collections::BTreeSet;

use super::{AnnotationKey, AnnotationTarget, Annotations};
use crate::document::{CadIr, Topology};
use crate::index::ModelIndex;

/// The id and the delta records of each evaluation and process state of `ir`
/// that has content.
pub(crate) fn deltas(ir: &CadIr) -> impl Iterator<Item = (&str, &Topology)> {
    let evaluations = ir.design.evaluations.iter().filter_map(|evaluation| {
        Some((
            evaluation.id.as_str(),
            &evaluation.content.as_ref()?.delta.records,
        ))
    });
    let states = ir
        .process
        .states
        .iter()
        .filter_map(|state| Some((state.id.as_str(), &state.content.as_ref()?.delta.records)));
    evaluations.chain(states)
}

/// The `(qualifier, record)` ids of each record of each delta of [`deltas`].
pub(crate) fn delta_records(ir: &CadIr) -> BTreeSet<(&str, &str)> {
    deltas(ir)
        .flat_map(|(qualifier, records)| {
            records
                .identities()
                .into_iter()
                .map(move |record| (qualifier, record))
        })
        .collect()
}

/// The records of one document that an annotation key can name: the
/// records of each arena, the native records, and the records of each delta.
struct Named<'ir> {
    records: BTreeSet<&'ir str>,
    native: ModelIndex<'ir>,
    deltas: BTreeSet<(&'ir str, &'ir str)>,
}

impl<'ir> Named<'ir> {
    fn of(ir: &'ir CadIr) -> Self {
        Self {
            records: ir
                .arenas()
                .record_ids()
                .into_values()
                .flat_map(|(_, ids)| ids)
                .collect(),
            native: ModelIndex::build(ir),
            deltas: delta_records(ir),
        }
    }

    fn names(&self, key: &AnnotationKey) -> bool {
        match key.target() {
            AnnotationTarget::Entity(id) => {
                self.records.contains(id) || self.native.native_record_owns(id)
            }
            AnnotationTarget::Delta { qualifier, record } => {
                self.deltas.contains(&(qualifier, record))
            }
        }
    }
}

impl Annotations {
    fn is_empty(&self) -> bool {
        self.provenance.is_empty() && self.exactness.is_empty()
    }

    /// The keys that name a record of `ir`: a top-level record by its id, or
    /// a record of the delta of an evaluation or process state of `ir`.
    pub(crate) fn named_in(&self, ir: &CadIr) -> BTreeSet<AnnotationKey> {
        if self.is_empty() {
            return BTreeSet::new();
        }
        let named = Named::of(ir);
        self.provenance
            .keys()
            .chain(self.exactness.keys())
            .filter(|key| named.names(key))
            .cloned()
            .collect()
    }

    /// Removes each key of `before`, the keys [`Self::named_in`] gave for a
    /// document, that names no record of `ir`, that document after a removal
    /// of records: the annotations of each record that left it, of each
    /// record that left a delta, and of each evaluation or process state that
    /// left it or lost its delta. Returns whether it removed a key.
    pub(crate) fn remove_left(&mut self, before: &BTreeSet<AnnotationKey>, ir: &CadIr) -> bool {
        if before.is_empty() {
            return false;
        }
        let named = Named::of(ir);
        let left = |key: &AnnotationKey| before.contains(key) && !named.names(key);
        let count = self.provenance.len() + self.exactness.len();
        self.provenance.retain(|key, _| !left(key));
        self.exactness.retain(|key, _| !left(key));
        count != self.provenance.len() + self.exactness.len()
    }
}
