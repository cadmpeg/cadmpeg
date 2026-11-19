// SPDX-License-Identifier: Apache-2.0
//! Resolved states (§5.4, §5.5): an evaluation with content is its chain of
//! bases with each delta applied, down to `Empty` or to the extent of a
//! current state. Every check of a state's names, owners, members, instance
//! bodies, poses, and closure runs against the resolved state.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::evaluations::check_path_steps;
use super::records::{DesignRecords, MemberKey, OfKey, PositionKey};
use crate::design::evaluation::{Evaluation, StateBase, StateDelta};
use crate::design::feature::FeatureOperation;
use crate::design::names::NameEntry;
use crate::design::refs::{EntityRef, RecordRef};
use crate::design::selection::OccurrencePath;
use crate::document::Topology;
use crate::features::ConfigurationId;
use crate::ids::{BodyId, ProductDefinitionId};
use crate::index::identities::IdentitySet;
use crate::index::ModelIndex;
use crate::report::check::Finding;
use crate::schema::EntitySchema;
use crate::topology::ChildLists;
use crate::transform::Transform;

macro_rules! topology_arenas {
    ($target:ident) => {
        $target! {
            bodies: Body,
            regions: Region,
            shells: Shell,
            faces: Face,
            loops: Loop,
            coedges: Coedge,
            edges: Edge,
            vertices: Vertex,
            points: Point,
            surfaces: Surface,
            curves: Curve,
            subds: Subd,
            pcurves: Pcurve,
            procedural_surfaces: ProceduralSurface,
            procedural_curves: ProceduralCurve,
        }
    };
}

macro_rules! declare_arenas {
    ($($field:ident: $arena:ident,)*) => {
        /// A `Topology` arena.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum Arena {
            $($arena,)*
        }

        impl Arena {
            const fn label(self) -> &'static str {
                match self {
                    $(Self::$arena => stringify!($field),)*
                }
            }

            fn of_record(record: &RecordRef) -> (Self, &str) {
                match record {
                    $(RecordRef::$arena(id) => (Self::$arena, id.as_str()),)*
                }
            }
        }

        /// Each record of `topology`: its arena, identity, and index.
        fn records(topology: &Topology) -> Vec<(Arena, &str, usize)> {
            let mut records = Vec::new();
            $(for (at, record) in topology.$field.iter().enumerate() {
                records.push((Arena::$arena, EntitySchema::identity(record), at));
            })*
            records
        }

        impl Located<'_> {
            fn visit_references(
                self,
                ctx: &DecodeContext<'_>,
                visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
            ) -> Result<(), CodecError> {
                match self.arena {
                    $(Arena::$arena => match self.topology.$field.get(self.index) {
                        Some(record) => record.visit_references(ctx, visitor),
                        None => Ok(()),
                    },)*
                }
            }
        }
    };
}
topology_arenas!(declare_arenas);

impl Arena {
    fn of_entity(entity: &EntityRef) -> (Self, &str) {
        match entity {
            EntityRef::Body(id) => (Self::Body, id.as_str()),
            EntityRef::Face(id) => (Self::Face, id.as_str()),
            EntityRef::Edge(id) => (Self::Edge, id.as_str()),
            EntityRef::Vertex(id) => (Self::Vertex, id.as_str()),
            EntityRef::Surface(id) => (Self::Surface, id.as_str()),
            EntityRef::Curve(id) => (Self::Curve, id.as_str()),
            EntityRef::Subd(id) => (Self::Subd, id.as_str()),
        }
    }
}

impl fmt::Display for Arena {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A topology record of a state: the `Topology` that holds it and its slot.
#[derive(Clone, Copy)]
struct Located<'a> {
    arena: Arena,
    topology: &'a Topology,
    index: usize,
}

/// An evaluation's resolved state, and the state of its base: the same chain
/// without the evaluation's own delta.
struct Resolved<'a> {
    base: ResolvedState<'a>,
    state: ResolvedState<'a>,
}

/// One state with every delta of its chain applied.
#[derive(Default, Clone)]
struct ResolvedState<'a> {
    records: BTreeMap<&'a str, Located<'a>>,
    names: BTreeMap<&'a EntityRef, &'a NameEntry>,
    owners: BTreeMap<&'a BodyId, &'a ProductDefinitionId>,
    members: BTreeMap<&'a str, &'a ProductDefinitionId>,
    instance_bodies: BTreeMap<&'a OccurrencePath, &'a [BodyId]>,
    poses: BTreeMap<&'a OccurrencePath, &'a Transform>,
}

impl ResolvedState<'_> {
    fn holds(&self, arena: Arena, id: &str) -> bool {
        self.records
            .get(id)
            .is_some_and(|located| located.arena == arena)
    }
}

/// Each record of `topology`, by identity, keeping the first of a repeated one.
fn locate(topology: &Topology) -> BTreeMap<&str, Located<'_>> {
    let mut located = BTreeMap::new();
    for (arena, id, index) in records(topology) {
        located.entry(id).or_insert(Located {
            arena,
            topology,
            index,
        });
    }
    located
}

struct Resolver<'a, 'c, 'd> {
    ctx: &'c DecodeContext<'d>,
    index: &'c DesignRecords<'a>,
    geometry: BTreeMap<&'a str, Located<'a>>,
    children: ChildLists<'a>,
}

impl<'a> Resolver<'a, '_, '_> {
    /// The children of a current-geometry record that only their parent
    /// pointers list (D29): a body's regions and a shell's faces.
    fn derived_children(&self, located: Located<'a>) -> Vec<&'a str> {
        match located.arena {
            Arena::Body => located
                .topology
                .bodies
                .get(located.index)
                .map(|body| self.children.body_regions(&body.id))
                .unwrap_or_default()
                .iter()
                .map(|region| region.id.as_str())
                .collect(),
            Arena::Shell => located
                .topology
                .shells
                .get(located.index)
                .map(|shell| self.children.shell_faces(&shell.id))
                .unwrap_or_default()
                .iter()
                .map(|face| face.id.as_str())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The extent of a scope's current state at `position`: its bodies and the
    /// records of the current geometry they reach, through references and
    /// through the derived child lists, their name entries, and its poses.
    /// `None` where the scope has no current state there.
    fn extent(
        &self,
        scope: &str,
        position: PositionKey<'_>,
    ) -> Result<Option<ResolvedState<'a>>, CodecError> {
        let Some(current) = self.index.scopes.get(scope).and_then(|scope| {
            scope
                .current
                .iter()
                .find(|current| PositionKey::of(&current.at) == position)
        }) else {
            return Ok(None);
        };
        let mut state = ResolvedState::default();
        let mut pending: Vec<&'a str> = current
            .bodies
            .iter()
            .filter_map(|body| {
                self.geometry
                    .get_key_value(body.as_str())
                    .map(|(id, _)| *id)
            })
            .collect();
        while let Some(id) = pending.pop() {
            let Some(located) = self.geometry.get(id).copied() else {
                continue;
            };
            if state.records.insert(id, located).is_some() {
                continue;
            }
            pending.extend(self.derived_children(located));
            located.visit_references(self.ctx, &mut |target| {
                if let Some((target, _)) = self.geometry.get_key_value(target) {
                    if !state.records.contains_key(target) {
                        pending.push(target);
                    }
                }
                Ok(())
            })?;
        }
        for (entity, entry) in self.index.ir.design.names.entries() {
            let (arena, id) = Arena::of_entity(entity);
            if state.holds(arena, id) {
                state.names.insert(entity, entry);
            }
        }
        state.poses = current.poses.iter().collect();
        Ok(Some(state))
    }

    /// The resolved state of an evaluation with content, and of its base. A
    /// repeated identity in the evaluation's own delta is reported. `None`
    /// where the chain of bases does not resolve (an absent base, a base with
    /// no content, a cycle, or a `Current` base that names no current state),
    /// which the evaluation rules report.
    fn resolve(
        &self,
        evaluation: &'a Evaluation,
        findings: &mut Vec<Finding>,
    ) -> Result<Option<Resolved<'a>>, CodecError> {
        let mut chain: Vec<&'a Evaluation> = vec![evaluation];
        let mut seen = BTreeSet::from([evaluation.id.as_str()]);
        let start = loop {
            let last = chain[chain.len() - 1];
            let Some(content) = &last.content else {
                return Ok(None);
            };
            match &content.base {
                StateBase::Empty => break ResolvedState::default(),
                StateBase::Current(position) => {
                    match self.extent(last.scope.as_str(), PositionKey::of(position))? {
                        Some(state) => break state,
                        None => return Ok(None),
                    }
                }
                StateBase::Evaluation(base) => {
                    let Some(base) = self.index.evaluations.get(base.as_str()).copied() else {
                        return Ok(None);
                    };
                    if !seen.insert(base.id.as_str()) {
                        return Ok(None);
                    }
                    chain.push(base);
                }
            }
        };
        let mut state = start;
        for link in chain[1..].iter().rev() {
            if let Some(content) = &link.content {
                apply(&mut state, &content.delta, None);
            }
        }
        let base = state.clone();
        if let Some(content) = &evaluation.content {
            apply(
                &mut state,
                &content.delta,
                Some((evaluation.id.as_str(), &mut *findings)),
            );
        }
        Ok(Some(Resolved { base, state }))
    }
}

/// Applies `delta` to `state`. With `report`, a record of the delta whose
/// identity another record of the delta holds, or a base record of another
/// arena holds, is reported against the named evaluation.
fn apply<'a>(
    state: &mut ResolvedState<'a>,
    delta: &'a StateDelta,
    mut report: Option<(&str, &mut Vec<Finding>)>,
) {
    for removed in &delta.removed {
        let (arena, id) = Arena::of_record(removed);
        if state.holds(arena, id) {
            state.records.remove(id);
        }
    }
    let mut inserted = BTreeSet::new();
    for (arena, id, index) in records(&delta.records) {
        let previous = state.records.insert(
            id,
            Located {
                arena,
                topology: &delta.records,
                index,
            },
        );
        let repeated = !inserted.insert(id);
        let clash = previous.filter(|previous| repeated || previous.arena != arena);
        if let (Some(previous), Some((evaluation, findings))) = (clash, report.as_mut()) {
            super::error(
                findings,
                evaluation,
                format_args!(
                    "two records of the resolved state share id {id}: a {} record and a {arena} record",
                    previous.arena
                ),
            );
        }
    }
    merge(&mut state.names, &delta.names, |entry| entry.as_ref());
    merge(&mut state.owners, &delta.owners, |owner| owner.as_ref());
    for (occurrence, owner) in &delta.members {
        match owner {
            Some(owner) => state.members.insert(occurrence.as_str(), owner),
            None => state.members.remove(occurrence.as_str()),
        };
    }
    merge(
        &mut state.instance_bodies,
        &delta.instance_bodies,
        |bodies| bodies.as_deref(),
    );
    merge(&mut state.poses, &delta.poses, |pose| pose.as_ref());
}

fn merge<'a, K: Ord, V, T: ?Sized>(
    into: &mut BTreeMap<&'a K, &'a T>,
    entries: &'a BTreeMap<K, V>,
    value: impl Fn(&'a V) -> Option<&'a T>,
) {
    for (key, entry) in entries {
        match value(entry) {
            Some(value) => into.insert(key, value),
            None => into.remove(key),
        };
    }
}

pub(super) fn check_resolved_states(
    ctx: &DecodeContext<'_>,
    index: &DesignRecords<'_>,
    ids: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let ir = index.ir;
    let resolver = Resolver {
        ctx,
        index,
        geometry: locate(&ir.geometry),
        children: ir.geometry.child_lists(),
    };
    check_current_names(&resolver, findings);
    let mut topology: BTreeSet<&str> = resolver.geometry.keys().copied().collect();
    for evaluation in &ir.design.evaluations {
        if let Some(content) = &evaluation.content {
            topology.extend(
                records(&content.delta.records)
                    .into_iter()
                    .map(|(_, id, _)| id),
            );
        }
    }
    for evaluation in &ir.design.evaluations {
        if evaluation.content.is_none() {
            continue;
        }
        let Some(Resolved { base, state }) = resolver.resolve(evaluation, findings)? else {
            continue;
        };
        let id = evaluation.id.as_str();
        check_entries(index, &state, id, findings);
        check_closure(ctx, &state, &topology, ids, id, findings)?;
        if let Some(content) = &evaluation.content {
            state_topology::check_state_topology(
                ctx,
                ir.tolerances.linear,
                &base,
                &state,
                &content.delta.records,
                id,
                findings,
            )?;
        }
        check_stored_bodies(index, evaluation, &state, findings);
    }
    Ok(())
}

/// Every name entry of the current geometry names an entity of it.
fn check_current_names(resolver: &Resolver<'_, '_, '_>, findings: &mut Vec<Finding>) {
    for entity in resolver.index.ir.design.names.entries().keys() {
        let (arena, id) = Arena::of_entity(entity);
        let held = resolver
            .geometry
            .get(id)
            .is_some_and(|located| located.arena == arena);
        if !held {
            super::error(
                findings,
                id,
                format_args!(
                    "name entry names {id}, which is no record of the current geometry's {arena}"
                ),
            );
        }
    }
}

/// The resolved state's name, owner, member, instance-body, and pose entries
/// name what the state holds.
fn check_entries(
    index: &DesignRecords<'_>,
    state: &ResolvedState<'_>,
    evaluation: &str,
    findings: &mut Vec<Finding>,
) {
    for entity in state.names.keys() {
        let (arena, id) = Arena::of_entity(entity);
        if !state.holds(arena, id) {
            super::error(
                findings,
                evaluation,
                format_args!(
                    "name entry names {id}, which is no record of the resolved state's {arena}"
                ),
            );
        }
    }
    for (body, owner) in &state.owners {
        if !state.holds(Arena::Body, body.as_str()) {
            super::error(
                findings,
                evaluation,
                format_args!(
                    "owner entry names {}, which is no body of the resolved state",
                    body.as_str()
                ),
            );
        }
        if !index.definitions.contains_key(owner.as_str()) {
            super::error(
                findings,
                evaluation,
                format_args!(
                    "owner entry names {}, which is no product definition",
                    owner.as_str()
                ),
            );
        }
    }
    for (occurrence, owner) in &state.members {
        if !index.occurrences.contains_key(occurrence) {
            super::error(
                findings,
                evaluation,
                format_args!(
                    "member entry names {occurrence}, which is no occurrence of the model"
                ),
            );
        }
        if !index.definitions.contains_key(owner.as_str()) {
            super::error(
                findings,
                evaluation,
                format_args!(
                    "member entry names {}, which is no product definition",
                    owner.as_str()
                ),
            );
        }
    }
    for (path, bodies) in &state.instance_bodies {
        check_path_steps(index, path, evaluation, "instance-body", findings);
        for body in *bodies {
            if !state.holds(Arena::Body, body.as_str()) {
                super::error(
                    findings,
                    evaluation,
                    format_args!(
                        "instance-body entry lists {}, which is no body of the resolved state",
                        body.as_str()
                    ),
                );
            }
        }
    }
    for path in state.poses.keys() {
        check_path_steps(index, path, evaluation, "pose", findings);
    }
}

/// Each record of the resolved state references only topology records of the
/// state: a reference to an id that the state does not hold is reported
/// unless the id is a record of the document outside every `Topology`, which
/// the reference checks resolve.
fn check_closure(
    ctx: &DecodeContext<'_>,
    state: &ResolvedState<'_>,
    topology: &BTreeSet<&str>,
    ids: &ModelIndex<'_>,
    evaluation: &str,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    for (id, located) in &state.records {
        let mut outside = None;
        located.visit_references(ctx, &mut |target| {
            if outside.is_none()
                && !state.records.contains_key(target)
                && (topology.contains(target) || !IdentitySet::contains(ids, target))
            {
                outside = Some(target.to_owned());
            }
            Ok(())
        })?;
        if let Some(target) = outside {
            super::error(
                findings,
                evaluation,
                format_args!("record {id} of the resolved state references {target}, which the state does not hold"),
            );
        }
    }
    Ok(())
}

/// Where the state after a stored-geometry feature is stored, it holds each
/// body that the feature's definition in the evaluation's configuration
/// names (§5.10).
fn check_stored_bodies(
    index: &DesignRecords<'_>,
    evaluation: &Evaluation,
    state: &ResolvedState<'_>,
    findings: &mut Vec<Finding>,
) {
    let OfKey::State(PositionKey::After(MemberKey::Feature(feature))) = OfKey::of(&evaluation.of)
    else {
        return;
    };
    let Some(feature) = index.features.get(feature) else {
        return;
    };
    let configuration = evaluation
        .configuration
        .as_ref()
        .map(ConfigurationId::as_str);
    let FeatureOperation::Stored {
        bodies: Some(bodies),
        ..
    } = &index.definition_in(feature, configuration).operation
    else {
        return;
    };
    for body in bodies {
        if !state.holds(Arena::Body, body.as_str()) {
            super::error(
                findings,
                evaluation.id.as_str(),
                format_args!(
                    "stored feature {} names {}, which is no body of the state after it",
                    feature.id.as_str(),
                    body.as_str()
                ),
            );
        }
    }
}

mod state_topology;
#[cfg(test)]
mod tests;
