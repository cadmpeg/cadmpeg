// SPDX-License-Identifier: Apache-2.0
//! One resolved state, changed in place. A walk applies a record's delta to
//! the state of its base, checks the record, and undoes the delta on the way
//! back, so a record's checks read its state without a copy of the base. The
//! state keeps the records that reference each id and, for each record that
//! references an id it does not hold, the first such id, so that the checks
//! of a delta read what the delta changes and the records that reach it.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::{records, Arena, Located};
use crate::design::evaluation::StateDelta;
use crate::design::names::NameEntry;
use crate::design::refs::EntityRef;
use crate::design::selection::OccurrencePath;
use crate::document::Topology;
use crate::ids::{BodyId, ProductDefinitionId};
use crate::index::identities::IdentitySet;
use crate::index::ModelIndex;
use crate::report::check::Finding;
use crate::transform::Transform;

/// What a reference check reads besides the state: the ids of every
/// `Topology` of the document and the document's identities.
pub(super) struct Closure<'s, 'a, 'd, 'm> {
    pub(super) ctx: &'s DecodeContext<'d>,
    pub(super) topology: &'s BTreeSet<&'a str>,
    pub(super) ids: &'s ModelIndex<'m>,
}

impl Closure<'_, '_, '_, '_> {
    /// The first id that `located` references and that `records` does not
    /// hold, where the id is a topology record of the document or no record
    /// of it.
    fn outside(
        &self,
        located: Located<'_>,
        records: &BTreeMap<&str, Located<'_>>,
    ) -> Result<Option<String>, CodecError> {
        let mut outside = None;
        located.visit_references(self.ctx, &mut |target| {
            if outside.is_none()
                && !records.contains_key(target)
                && (self.topology.contains(target) || !IdentitySet::contains(self.ids, target))
            {
                outside = Some(target.to_owned());
            }
            Ok(())
        })?;
        Ok(outside)
    }
}

/// A key for an id that may be a record of no `Topology`.
fn key(id: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

/// One change to a state, holding the value it replaced.
enum Change<'a> {
    Record(&'a str, Option<Located<'a>>),
    Entry(Entry<'a>),
}

/// A change to a state's tables other than its records.
enum Entry<'a> {
    Name(&'a EntityRef, Option<&'a NameEntry>),
    Owner(&'a BodyId, Option<&'a ProductDefinitionId>),
    Member(&'a str, Option<&'a ProductDefinitionId>),
    InstanceBodies(&'a OccurrencePath, Option<&'a [BodyId]>),
    Pose(&'a OccurrencePath, Option<&'a Transform>),
    Open(&'a str, Option<String>),
    Unheld(&'a EntityRef, bool),
}

/// What applying one delta changed, read before the delta was applied.
pub(super) struct Applied<'a> {
    /// The ids whose record the delta replaces or removes, with the base's
    /// record of each.
    pub(super) base: BTreeMap<&'a str, Option<Located<'a>>>,
    /// The ids that the base's records of `base` reference and the records of
    /// the base that reference them.
    seeds: Vec<String>,
}

/// A node of the reference graph of a state: a record, or an id that no
/// record of the state holds.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Node<'a> {
    Record(&'a str),
    Missing(u64),
}

#[derive(Default)]
pub(super) struct State<'a> {
    pub(super) records: BTreeMap<&'a str, Located<'a>>,
    /// The records that reference each id, by the id's key, once per
    /// reference.
    referrers: HashMap<u64, Vec<&'a str>>,
    pub(super) names: BTreeMap<&'a EntityRef, &'a NameEntry>,
    names_by_id: HashMap<&'a str, Vec<&'a EntityRef>>,
    pub(super) owners: BTreeMap<&'a BodyId, &'a ProductDefinitionId>,
    pub(super) members: BTreeMap<&'a str, &'a ProductDefinitionId>,
    pub(super) instance_bodies: BTreeMap<&'a OccurrencePath, &'a [BodyId]>,
    pub(super) poses: BTreeMap<&'a OccurrencePath, &'a Transform>,
    /// Each record that references an id the state does not hold, with the
    /// first such id.
    pub(super) open: BTreeMap<&'a str, String>,
    /// The name entries that name no record of the state in their arena.
    pub(super) unheld: BTreeSet<&'a EntityRef>,
    undo: Vec<Change<'a>>,
}

impl<'a> State<'a> {
    /// The state of `records`, their name entries `names`, and `poses`.
    pub(super) fn bottom(
        closure: &Closure<'_, 'a, '_, '_>,
        records: BTreeMap<&'a str, Located<'a>>,
        names: BTreeMap<&'a EntityRef, &'a NameEntry>,
        poses: BTreeMap<&'a OccurrencePath, &'a Transform>,
    ) -> Result<Self, CodecError> {
        let mut state = Self {
            poses,
            ..Self::default()
        };
        for (&id, &located) in &records {
            state.refer(closure.ctx, id, located, true)?;
        }
        for (&id, &located) in &records {
            if let Some(target) = closure.outside(located, &records)? {
                state.open.insert(id, target);
            }
        }
        state.records = records;
        for (entity, entry) in names {
            state.index_name(entity, true);
            if !state.holds_entity(entity) {
                state.unheld.insert(entity);
            }
            state.names.insert(entity, entry);
        }
        Ok(state)
    }

    pub(super) fn holds(&self, arena: Arena, id: &str) -> bool {
        self.records
            .get(id)
            .is_some_and(|located| located.arena == arena)
    }

    fn holds_entity(&self, entity: &EntityRef) -> bool {
        let (arena, id) = Arena::of_entity(entity);
        self.holds(arena, id)
    }

    /// The length of the undo log, which [`Self::undo`] returns to.
    pub(super) fn mark(&self) -> usize {
        self.undo.len()
    }

    /// Applies `delta`. With `report`, a record of the delta whose identity
    /// another record of the delta holds, or a base record of another arena
    /// holds, is reported against the named record.
    pub(super) fn apply(
        &mut self,
        closure: &Closure<'_, 'a, '_, '_>,
        delta: &'a StateDelta,
        mut report: Option<(&str, &mut Vec<Finding>)>,
    ) -> Result<Applied<'a>, CodecError> {
        let mut changed: Vec<&'a str> = Vec::new();
        for removed in &delta.removed {
            let (arena, id) = Arena::of_record(removed);
            if self.holds(arena, id) {
                changed.push(id);
            }
        }
        let inserted = records(&delta.records);
        changed.extend(inserted.iter().map(|&(_, id, _)| id));
        let mut seeds = Vec::new();
        let mut base = BTreeMap::new();
        for &id in &changed {
            let located = self.records.get(id).copied();
            if base.insert(id, located).is_some() {
                continue;
            }
            seeds.push(id.to_owned());
            if let Some(located) = located {
                located.visit_references(closure.ctx, &mut |target| {
                    seeds.push(target.to_owned());
                    Ok(())
                })?;
            }
            for referrer in self.referrers.get(&key(id)).into_iter().flatten() {
                seeds.push((*referrer).to_owned());
            }
        }
        for removed in &delta.removed {
            let (arena, id) = Arena::of_record(removed);
            if self.holds(arena, id) {
                self.set_record(closure.ctx, id, None)?;
            }
        }
        let mut repeated = BTreeSet::new();
        for (arena, id, index) in inserted {
            let previous = self.set_record(
                closure.ctx,
                id,
                Some(Located {
                    arena,
                    topology: &delta.records,
                    index,
                }),
            )?;
            let repeated = !repeated.insert(id);
            let clash = previous.filter(|previous| repeated || previous.arena != arena);
            if let (Some(previous), Some((evaluation, findings))) = (clash, report.as_mut()) {
                super::super::error(
                    findings,
                    evaluation,
                    crate::rule!("design.two-records-resolved-state-share-id",
                        "two records of the resolved state share id {id}: a {} record and a {arena} record",
                        previous.arena
                    ),
                );
            }
        }
        for (entity, entry) in &delta.names {
            self.change(Entry::Name(entity, entry.as_ref()));
        }
        for (body, owner) in &delta.owners {
            self.change(Entry::Owner(body, owner.as_ref()));
        }
        for (occurrence, owner) in &delta.members {
            self.change(Entry::Member(occurrence.as_str(), owner.as_ref()));
        }
        for (path, bodies) in &delta.instance_bodies {
            self.change(Entry::InstanceBodies(path, bodies.as_deref()));
        }
        for (path, pose) in &delta.poses {
            self.change(Entry::Pose(path, pose.as_ref()));
        }
        self.reopen(closure, base.keys().copied())?;
        let mut names: BTreeSet<&'a EntityRef> = delta.names.keys().collect();
        for id in base.keys() {
            names.extend(self.names_by_id.get(id).into_iter().flatten().copied());
        }
        for entity in names {
            let unheld = self.names.contains_key(entity) && !self.holds_entity(entity);
            if unheld != self.unheld.contains(entity) {
                self.change(Entry::Unheld(entity, unheld));
            }
        }
        Ok(Applied { base, seeds })
    }

    /// Recomputes the first outside reference of each record in `changed`
    /// and of each record that references one of them.
    fn reopen(
        &mut self,
        closure: &Closure<'_, 'a, '_, '_>,
        changed: impl Iterator<Item = &'a str>,
    ) -> Result<(), CodecError> {
        let mut affected = BTreeSet::new();
        for id in changed {
            affected.insert(id);
            affected.extend(self.referrers.get(&key(id)).into_iter().flatten().copied());
        }
        for id in affected {
            let outside = match self.records.get(id) {
                Some(&located) => closure.outside(located, &self.records)?,
                None => None,
            };
            if self.open.get(id) != outside.as_ref() {
                self.change(Entry::Open(id, outside));
            }
        }
        Ok(())
    }

    /// Undoes every change made since `mark`.
    pub(super) fn undo(&mut self, ctx: &DecodeContext<'_>, mark: usize) -> Result<(), CodecError> {
        while self.undo.len() > mark {
            let Some(change) = self.undo.pop() else {
                break;
            };
            self.set(ctx, change)?;
        }
        Ok(())
    }

    fn set_record(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: &'a str,
        located: Option<Located<'a>>,
    ) -> Result<Option<Located<'a>>, CodecError> {
        let previous = self.records.get(id).copied();
        self.set(ctx, Change::Record(id, located))?;
        self.undo.push(Change::Record(id, previous));
        Ok(previous)
    }

    /// Makes `entry` and logs the value it replaces.
    fn change(&mut self, entry: Entry<'a>) {
        let previous = match &entry {
            Entry::Name(entity, _) => Entry::Name(entity, self.names.get(entity).copied()),
            Entry::Owner(body, _) => Entry::Owner(body, self.owners.get(body).copied()),
            Entry::Member(occurrence, _) => {
                Entry::Member(occurrence, self.members.get(occurrence).copied())
            }
            Entry::InstanceBodies(path, _) => {
                Entry::InstanceBodies(path, self.instance_bodies.get(path).copied())
            }
            Entry::Pose(path, _) => Entry::Pose(path, self.poses.get(path).copied()),
            Entry::Open(id, _) => Entry::Open(id, self.open.get(id).cloned()),
            Entry::Unheld(entity, _) => Entry::Unheld(entity, self.unheld.contains(entity)),
        };
        self.set_entry(entry);
        self.undo.push(Change::Entry(previous));
    }

    fn set(&mut self, ctx: &DecodeContext<'_>, change: Change<'a>) -> Result<(), CodecError> {
        match change {
            Change::Record(id, located) => {
                if let Some(previous) = self.records.remove(id) {
                    self.refer(ctx, id, previous, false)?;
                }
                if let Some(located) = located {
                    self.refer(ctx, id, located, true)?;
                    self.records.insert(id, located);
                }
            }
            Change::Entry(entry) => self.set_entry(entry),
        }
        Ok(())
    }

    fn set_entry(&mut self, entry: Entry<'a>) {
        fn put<K: Ord, V>(map: &mut BTreeMap<K, V>, key: K, value: Option<V>) {
            match value {
                Some(value) => map.insert(key, value),
                None => map.remove(&key),
            };
        }
        match entry {
            Entry::Name(entity, entry) => {
                let held = self.names.contains_key(entity);
                if held != entry.is_some() {
                    self.index_name(entity, entry.is_some());
                }
                put(&mut self.names, entity, entry);
            }
            Entry::Owner(body, owner) => put(&mut self.owners, body, owner),
            Entry::Member(occurrence, owner) => put(&mut self.members, occurrence, owner),
            Entry::InstanceBodies(path, bodies) => put(&mut self.instance_bodies, path, bodies),
            Entry::Pose(path, pose) => put(&mut self.poses, path, pose),
            Entry::Open(id, target) => put(&mut self.open, id, target),
            Entry::Unheld(entity, unheld) => {
                if unheld {
                    self.unheld.insert(entity);
                } else {
                    self.unheld.remove(entity);
                }
            }
        }
    }

    /// Adds, or with `add` false removes, the references of `located`, the
    /// record `id`, to the index of referrers.
    fn refer(
        &mut self,
        ctx: &DecodeContext<'_>,
        id: &'a str,
        located: Located<'a>,
        add: bool,
    ) -> Result<(), CodecError> {
        let referrers = &mut self.referrers;
        located.visit_references(ctx, &mut |target| {
            let list = referrers.entry(key(target)).or_default();
            if add {
                list.push(id);
            } else if let Some(at) = list.iter().position(|referrer| *referrer == id) {
                list.swap_remove(at);
            }
            Ok(())
        })
    }

    fn index_name(&mut self, entity: &'a EntityRef, add: bool) {
        let list = self.names_by_id.entry(entity.id()).or_default();
        if add {
            list.push(entity);
        } else if let Some(at) = list.iter().position(|named| *named == entity) {
            list.swap_remove(at);
        }
    }

    /// The ids of the component of the changed records in the union of the
    /// base's and this state's reference graphs: the records of the state it
    /// holds, and the base's records that the state removes. Every other
    /// record has the same record and the same neighbours in both states.
    fn region(
        &self,
        ctx: &DecodeContext<'_>,
        applied: &Applied<'a>,
    ) -> Result<BTreeSet<&'a str>, CodecError> {
        let node = |id: &str| match self.records.get_key_value(id) {
            Some((&id, _)) => Node::Record(id),
            None => Node::Missing(key(id)),
        };
        let mut seen: HashSet<Node<'a>> = HashSet::new();
        let mut pending: Vec<Node<'a>> = applied.seeds.iter().map(|id| node(id)).collect();
        while let Some(at) = pending.pop() {
            if !seen.insert(at) {
                continue;
            }
            let id_key = match at {
                Node::Record(id) => {
                    if let Some(&located) = self.records.get(id) {
                        located.visit_references(ctx, &mut |target| {
                            pending.push(node(target));
                            Ok(())
                        })?;
                    }
                    key(id)
                }
                Node::Missing(id_key) => id_key,
            };
            pending.extend(
                self.referrers
                    .get(&id_key)
                    .into_iter()
                    .flatten()
                    .map(|&referrer| Node::Record(referrer)),
            );
        }
        let mut region: BTreeSet<&'a str> = seen
            .into_iter()
            .filter_map(|node| match node {
                Node::Record(id) => Some(id),
                Node::Missing(_) => None,
            })
            .collect();
        region.extend(
            applied
                .base
                .iter()
                .filter(|(_, located)| located.is_some())
                .map(|(&id, _)| id),
        );
        Ok(region)
    }

    /// The region of the changed records as a `Topology` of the base and one
    /// of this state, each with its records in id order.
    pub(super) fn region_topologies(
        &self,
        ctx: &DecodeContext<'_>,
        applied: &Applied<'a>,
    ) -> Result<(Topology, Topology), CodecError> {
        let region = self.region(ctx, applied)?;
        let base = region.iter().filter_map(|id| match applied.base.get(id) {
            Some(located) => *located,
            None => self.records.get(id).copied(),
        });
        let state = region.iter().filter_map(|id| self.records.get(id).copied());
        Ok((topology(base), topology(state)))
    }

    /// The base and this state, each as a `Topology` with its records in id
    /// order.
    pub(super) fn whole_topologies(&self, applied: &Applied<'a>) -> (Topology, Topology) {
        let mut base = self.records.clone();
        for (&id, located) in &applied.base {
            match located {
                Some(located) => base.insert(id, *located),
                None => base.remove(id),
            };
        }
        (topology(base.into_values()), self.topology())
    }

    /// The state's records of the region of the changed records, as a
    /// `Topology` with its records in id order.
    pub(super) fn region_topology(
        &self,
        ctx: &DecodeContext<'_>,
        applied: &Applied<'a>,
    ) -> Result<Topology, CodecError> {
        let region = self.region(ctx, applied)?;
        Ok(topology(
            region.iter().filter_map(|id| self.records.get(id).copied()),
        ))
    }

    /// The state as a `Topology` with its records in id order.
    pub(super) fn topology(&self) -> Topology {
        topology(self.records.values().copied())
    }
}

/// `records`, in their order, as a `Topology` of their own.
fn topology<'a>(records: impl Iterator<Item = Located<'a>>) -> Topology {
    let mut topology = Topology::default();
    for located in records {
        located.copy_into(&mut topology);
    }
    topology.finalize();
    topology
}
