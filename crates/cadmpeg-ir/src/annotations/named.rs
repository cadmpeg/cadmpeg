// SPDX-License-Identifier: Apache-2.0
//! The records of a document that annotation keys name (§3.2), and the
//! removal of the annotations of records that leave a document.

use std::collections::BTreeSet;

use super::{AnnotationKey, AnnotationTarget, Annotations};
use crate::document::{ArenaHome, CadIr, Topology};
use crate::index::identities::IdentitySet;
use crate::index::{ModelIndex, StandardIndex};

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

/// The ids of the records of the design and process arenas of `ir`. The
/// model index holds the records of the other arenas and the native records.
pub(crate) fn design_and_process_records(ir: &CadIr) -> BTreeSet<&str> {
    ir.arenas()
        .record_ids()
        .into_iter()
        .filter(|(arena, _)| matches!(arena.home(), ArenaHome::Design | ArenaHome::Process))
        .flat_map(|(_, (_, ids))| ids)
        .collect()
}

/// The records of one document that an annotation key can name.
struct Named<'ir> {
    index: ModelIndex<'ir>,
    records: BTreeSet<&'ir str>,
    deltas: BTreeSet<(&'ir str, &'ir str)>,
}

impl<'ir> Named<'ir> {
    fn of(ir: &'ir CadIr) -> Self {
        Self {
            index: ModelIndex::build(ir),
            records: design_and_process_records(ir),
            deltas: delta_records(ir),
        }
    }

    fn names_entity(&self, id: &str) -> bool {
        IdentitySet::contains(&self.index, id) || self.records.contains(id)
    }
}

/// The records that one removal took out of a document, as annotation keys
/// name them: each id and each delta record that the document named before
/// the removal and does not name after it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Removed {
    entities: BTreeSet<String>,
    deltas: BTreeSet<(String, String)>,
}

impl Removed {
    /// What `ir` names, before a removal.
    pub(crate) fn before(ir: &CadIr) -> Self {
        let named = Named::of(ir);
        Self {
            entities: named
                .index
                .identities(StandardIndex)
                .chain(named.records.iter().copied())
                .map(str::to_owned)
                .collect(),
            deltas: named
                .deltas
                .iter()
                .map(|(qualifier, record)| ((*qualifier).to_owned(), (*record).to_owned()))
                .collect(),
        }
    }

    /// What of `self` and of `lost`, the ids of the records that left before
    /// the document was built, `ir`, the document after the removal, does not
    /// name.
    pub(crate) fn after(mut self, lost: impl IntoIterator<Item = String>, ir: &CadIr) -> Self {
        let named = Named::of(ir);
        self.entities.extend(lost);
        self.entities.retain(|id| !named.names_entity(id));
        self.deltas.retain(|(qualifier, record)| {
            !named
                .deltas
                .contains(&(qualifier.as_str(), record.as_str()))
        });
        self
    }

    /// Whether `key` names a removed record, a record of the delta of a
    /// removed evaluation or process state, or a removed record of a delta.
    pub(crate) fn names(&self, key: &AnnotationKey) -> bool {
        match key.target() {
            AnnotationTarget::Entity(id) => self.entities.contains(id),
            AnnotationTarget::Delta { qualifier, record } => {
                self.entities.contains(qualifier)
                    || self
                        .deltas
                        .contains(&(qualifier.to_owned(), record.to_owned()))
            }
        }
    }
}

impl Annotations {
    /// Whether the tables hold no key.
    pub(crate) fn is_empty(&self) -> bool {
        self.provenance.is_empty() && self.exactness.is_empty()
    }

    /// Removes each key that names a record of `removed`. Returns whether it
    /// removed a key.
    pub(crate) fn remove(&mut self, removed: &Removed) -> bool {
        let count = self.provenance.len() + self.exactness.len();
        self.provenance.retain(|key, _| !removed.names(key));
        self.exactness.retain(|key, _| !removed.names(key));
        count != self.provenance.len() + self.exactness.len()
    }
}
