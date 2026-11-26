// SPDX-License-Identifier: Apache-2.0
//! The records of a document that annotation keys name (§3.2), and the
//! removal of the annotations of records that leave a document.

use std::collections::BTreeSet;

use super::{AnnotationKey, AnnotationTarget, Annotations};
use crate::document::{ArenaHome, ArenaName, CadIr, Topology};
use crate::native::NativeRecord;

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

/// The ids of the records of the design and process arenas of `ir`, which
/// the model index leaves out.
pub(crate) fn design_and_process_records(ir: &CadIr) -> BTreeSet<&str> {
    ir.arenas()
        .record_ids()
        .into_iter()
        .filter(|(arena, _)| matches!(arena.home(), ArenaHome::Design | ArenaHome::Process))
        .flat_map(|(_, (_, ids))| ids)
        .collect()
}

/// The ids of the records of `ir` that an annotation key names as an entity:
/// the records of each arena, other than those of the arenas the CADIR wire
/// omits where `wire` is set, and the native records.
fn entities(ir: &CadIr, wire: bool) -> BTreeSet<&str> {
    let arenas = ir
        .arenas()
        .record_ids()
        .into_iter()
        .filter(|(arena, _)| !(wire && ArenaName::WIRE_OMITTED.contains(arena)))
        .flat_map(|(_, (_, ids))| ids);
    let native = ir
        .native
        .0
        .values()
        .flat_map(|namespace| namespace.arenas().values())
        .flatten()
        .map(NativeRecord::id);
    arenas.chain(native).collect()
}

/// The records of one document that an annotation key can name.
struct Named<'ir> {
    entities: BTreeSet<&'ir str>,
    deltas: BTreeSet<(&'ir str, &'ir str)>,
}

impl<'ir> Named<'ir> {
    /// The records of `ir` in memory.
    fn of(ir: &'ir CadIr) -> Self {
        Self {
            entities: entities(ir, false),
            deltas: delta_records(ir),
        }
    }

    /// The records of the CADIR text of `ir` (§3.4).
    fn written(ir: &'ir CadIr) -> Self {
        Self {
            entities: entities(ir, true),
            deltas: delta_records(ir),
        }
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
        Self::of_records(
            named.entities.into_iter().map(str::to_owned),
            named
                .deltas
                .into_iter()
                .map(|(qualifier, record)| (qualifier.to_owned(), record.to_owned())),
        )
    }

    /// The records `entities` and the delta records `deltas`, each a
    /// `(qualifier, record)` pair, as a removal that took them out.
    pub(crate) fn of_records(
        entities: impl IntoIterator<Item = String>,
        deltas: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        Self {
            entities: entities.into_iter().collect(),
            deltas: deltas.into_iter().collect(),
        }
    }

    /// What of `self` `ir`, the document after the removal, does not name.
    pub(crate) fn after(self, ir: &CadIr) -> Self {
        if self.is_empty() {
            return self;
        }
        self.left(&Named::of(ir))
    }

    /// What of `self` the CADIR text of `ir`, the document a write wrote,
    /// does not name.
    pub(crate) fn after_writing(self, ir: &CadIr) -> Self {
        if self.is_empty() {
            return self;
        }
        self.left(&Named::written(ir))
    }

    fn left(mut self, named: &Named<'_>) -> Self {
        self.entities
            .retain(|id| !named.entities.contains(id.as_str()));
        self.deltas.retain(|(qualifier, record)| {
            !named
                .deltas
                .contains(&(qualifier.as_str(), record.as_str()))
        });
        self
    }

    fn is_empty(&self) -> bool {
        self.entities.is_empty() && self.deltas.is_empty()
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

    /// Removes each key that names a record of `removed`.
    pub(crate) fn remove(&mut self, removed: &Removed) {
        self.provenance.retain(|key, _| !removed.names(key));
        self.exactness.retain(|key, _| !removed.names(key));
    }
}
