// SPDX-License-Identifier: Apache-2.0
//! States as records: the extent of a current state, an evaluation's state
//! resolved through its chain of bases, and a delta composed with its base's
//! (§5.4, §5.5).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value as Json;

use super::{malformed, EditError};
use crate::design::evaluation::{CurrentState, Evaluation, StateBase, StateContent, StateDelta};
use crate::design::ids::EvaluationId;
use crate::design::names::NameEntry;
use crate::design::refs::{EntityRef, Position, RecordRef};
use crate::design::selection::OccurrencePath;
use crate::document::Topology;
use crate::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use crate::schema::ArenaRecord;
use crate::transform::Transform;
use crate::CadIr;

/// Calls `$apply!` with every arena of `Topology` and its `RecordRef` arm.
macro_rules! topology_arenas {
    ($apply:ident) => {
        $apply! {
            bodies Body, regions Region, shells Shell, faces Face, loops Loop,
            coedges Coedge, edges Edge, vertices Vertex, points Point,
            surfaces Surface, curves Curve, subds Subd, pcurves Pcurve,
            procedural_surfaces ProceduralSurface, procedural_curves ProceduralCurve
        }
    };
}

/// The id a record reference names.
pub(crate) fn record_ref_id(record: &RecordRef) -> &str {
    macro_rules! arms {
        ($($field:ident $arm:ident),*) => {
            match record { $(RecordRef::$arm(id) => id.as_str(),)* }
        };
    }
    topology_arenas!(arms)
}

/// A reference to each record of a topology.
pub(crate) fn record_refs(topology: &Topology) -> Vec<RecordRef> {
    let mut found = Vec::new();
    macro_rules! refer {
        ($($field:ident $arm:ident),*) => {
            $(found.extend(topology.$field.iter().map(|record| RecordRef::$arm(record.id.clone())));)*
        };
    }
    topology_arenas!(refer);
    found
}

/// Every record id of a topology.
pub(crate) fn ids(topology: &Topology) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    macro_rules! collect {
        ($($field:ident $arm:ident),*) => {
            $(found.extend(topology.$field.iter().map(|record| record.record_identity().to_owned()));)*
        };
    }
    topology_arenas!(collect);
    found
}

/// The records of a topology whose ids `keep` holds.
pub(crate) fn select(topology: &Topology, keep: &BTreeSet<String>) -> Topology {
    let mut selected = Topology::default();
    macro_rules! copy {
        ($($field:ident $arm:ident),*) => {
            $(selected.$field = topology.$field.iter()
                .filter(|record| keep.contains(record.record_identity()))
                .cloned()
                .collect();)*
        };
    }
    topology_arenas!(copy);
    selected
}

/// Removes the records whose ids `drop` holds.
pub(super) fn remove(topology: &mut Topology, drop: &BTreeSet<String>) {
    macro_rules! retain {
        ($($field:ident $arm:ident),*) => {
            $(topology.$field.retain(|record| !drop.contains(record.record_identity()));)*
        };
    }
    topology_arenas!(retain);
}

impl StateDelta {
    /// The delta that takes `base` to `state`, both whole record sets in one
    /// id space (§2.2): each record of `state` that `base` lacks or holds
    /// with other content, the carrier in `state` of each procedural record
    /// among them, and a reference to each record of `base` that `state`
    /// lacks. The maps stay empty.
    #[must_use]
    pub fn between(base: &Topology, state: &Topology) -> Self {
        let mut delta = Self::default();
        macro_rules! diff {
            ($($field:ident $arm:ident),*) => {$(
                let held: BTreeMap<&str, _> = base.$field.iter()
                    .map(|record| (record.record_identity(), record))
                    .collect();
                let kept: BTreeSet<&str> = state.$field.iter()
                    .map(ArenaRecord::record_identity)
                    .collect();
                delta.records.$field = state.$field.iter()
                    .filter(|record| held.get(record.record_identity()) != Some(record))
                    .cloned()
                    .collect();
                delta.removed.extend(base.$field.iter()
                    .filter(|record| !kept.contains(record.record_identity()))
                    .map(|record| RecordRef::$arm(record.id.clone())));
            )*};
        }
        topology_arenas!(diff);
        // The wire writes a procedural record with the carrier that owns it.
        macro_rules! carriers {
            ($($carriers:ident $procedural:ident),*) => {$(
                let constructions: BTreeSet<&str> = delta.records.$procedural.iter()
                    .map(ArenaRecord::record_identity)
                    .collect();
                if !constructions.is_empty() {
                    let changed: BTreeSet<&str> = delta.records.$carriers.iter()
                        .map(ArenaRecord::record_identity)
                        .collect();
                    let carriers = state.$carriers.iter()
                        .filter(|carrier| {
                            changed.contains(carrier.record_identity())
                                || carrier.geometry.procedural_construction()
                                    .is_some_and(|construction| constructions.contains(construction.as_str()))
                        })
                        .cloned()
                        .collect();
                    delta.records.$carriers = carriers;
                }
            )*};
        }
        carriers!(curves procedural_curves, surfaces procedural_surfaces);
        delta
    }
}

/// Inserts each record of `records`, or replaces the record of the same id.
pub(crate) fn upsert(topology: &mut Topology, records: Topology) {
    macro_rules! merge {
        ($($field:ident $arm:ident),*) => {
            $(
                let mut at: BTreeMap<String, usize> = topology.$field.iter().enumerate()
                    .map(|(index, record)| (record.record_identity().to_owned(), index))
                    .collect();
                for record in records.$field {
                    match at.get(record.record_identity()) {
                        Some(&index) => topology.$field[index] = record,
                        None => {
                            at.insert(record.record_identity().to_owned(), topology.$field.len());
                            topology.$field.push(record);
                        }
                    }
                }
            )*
        };
    }
    topology_arenas!(merge);
}

/// The ids of the records the bodies reach: each body, the records whose
/// parent pointer names a reached record (a region's body, a shell's region,
/// a face's shell, a loop's face, a coedge's loop), and every record a
/// reached record names in any other field.
pub(crate) fn reached(
    topology: &Topology,
    bodies: &[BodyId],
) -> Result<BTreeSet<String>, EditError> {
    let known = ids(topology);
    let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for region in &topology.regions {
        children
            .entry(region.body.as_str())
            .or_default()
            .push(region.id.as_str());
    }
    for shell in &topology.shells {
        children
            .entry(shell.region.as_str())
            .or_default()
            .push(shell.id.as_str());
    }
    for face in &topology.faces {
        children
            .entry(face.shell.as_str())
            .or_default()
            .push(face.id.as_str());
    }
    for item in &topology.loops {
        children
            .entry(item.face.as_str())
            .or_default()
            .push(item.id.as_str());
    }
    for coedge in &topology.coedges {
        children
            .entry(coedge.owner_loop.as_str())
            .or_default()
            .push(coedge.id.as_str());
    }
    let mut names: BTreeMap<String, Vec<String>> = BTreeMap::new();
    macro_rules! mentions {
        ($($field:ident $arm:ident),*) => {
            $(
                for record in &topology.$field {
                    let json = crate::hash::finite_json::to_json_value(record).map_err(malformed)?;
                    let own = record.record_identity();
                    let mut found = Vec::new();
                    let mut stack = vec![&json];
                    while let Some(node) = stack.pop() {
                        match node {
                            Json::String(text) if text != own && known.contains(text) => {
                                found.push(text.clone());
                            }
                            Json::Array(items) => stack.extend(items),
                            Json::Object(object) => stack.extend(object.values()),
                            _ => {}
                        }
                    }
                    names.insert(own.to_owned(), found);
                }
            )*
        };
    }
    topology_arenas!(mentions);
    let parents: BTreeSet<(&str, &str)> = children
        .iter()
        .flat_map(|(parent, items)| items.iter().map(move |child| (*child, *parent)))
        .collect();
    let mut held: BTreeSet<String> = BTreeSet::new();
    let mut stack: Vec<String> = bodies
        .iter()
        .map(|body| body.as_str().to_owned())
        .filter(|body| known.contains(body))
        .collect();
    while let Some(id) = stack.pop() {
        if !held.insert(id.clone()) {
            continue;
        }
        if let Some(items) = children.get(id.as_str()) {
            stack.extend(items.iter().map(|child| (*child).to_owned()));
        }
        for named in names.get(&id).into_iter().flatten() {
            if !parents.contains(&(id.as_str(), named.as_str())) {
                stack.push(named.clone());
            }
        }
    }
    Ok(held)
}

/// A state as records and maps.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct State {
    pub(super) records: Topology,
    pub(super) names: BTreeMap<EntityRef, NameEntry>,
    pub(super) owners: BTreeMap<BodyId, ProductDefinitionId>,
    pub(super) members: BTreeMap<OccurrenceId, ProductDefinitionId>,
    pub(super) instance_bodies: BTreeMap<OccurrencePath, Vec<BodyId>>,
    pub(super) poses: BTreeMap<OccurrencePath, Transform>,
}

fn apply_map<K: Ord + Clone, V: Clone>(
    target: &mut BTreeMap<K, V>,
    delta: &BTreeMap<K, Option<V>>,
) {
    for (key, value) in delta {
        match value {
            Some(value) => {
                target.insert(key.clone(), value.clone());
            }
            None => {
                target.remove(key);
            }
        }
    }
}

fn set_all<K: Ord + Clone, V: Clone>(map: &BTreeMap<K, V>) -> BTreeMap<K, Option<V>> {
    map.iter()
        .map(|(key, value)| (key.clone(), Some(value.clone())))
        .collect()
}

impl State {
    /// The state as a delta over `Empty`.
    pub(super) fn delta(&self) -> StateDelta {
        StateDelta {
            records: self.records.clone(),
            removed: Vec::new(),
            names: set_all(&self.names),
            owners: set_all(&self.owners),
            members: set_all(&self.members),
            instance_bodies: set_all(&self.instance_bodies),
            poses: set_all(&self.poses),
        }
    }

    fn apply(&mut self, delta: &StateDelta) {
        let removed: BTreeSet<String> = delta
            .removed
            .iter()
            .map(|record| record_ref_id(record).to_owned())
            .collect();
        remove(&mut self.records, &removed);
        upsert(&mut self.records, delta.records.clone());
        apply_map(&mut self.names, &delta.names);
        apply_map(&mut self.owners, &delta.owners);
        apply_map(&mut self.members, &delta.members);
        apply_map(&mut self.instance_bodies, &delta.instance_bodies);
        apply_map(&mut self.poses, &delta.poses);
    }
}

/// `second` applied after `first`, as one delta over `first`'s base, whose
/// records are `base_ids`.
pub(super) fn compose(
    first: &StateDelta,
    second: &StateDelta,
    base_ids: &BTreeSet<String>,
) -> StateDelta {
    let mut records = first.records.clone();
    let second_removed: BTreeSet<String> = second
        .removed
        .iter()
        .map(|record| record_ref_id(record).to_owned())
        .collect();
    remove(&mut records, &second_removed);
    upsert(&mut records, second.records.clone());
    let inserted = ids(&records);
    let mut removed = Vec::new();
    let mut seen = BTreeSet::new();
    for record in first.removed.iter().chain(&second.removed) {
        let id = record_ref_id(record);
        if base_ids.contains(id) && !inserted.contains(id) && seen.insert(id.to_owned()) {
            removed.push(record.clone());
        }
    }
    let mut composed = StateDelta {
        records,
        removed,
        ..first.clone()
    };
    composed.names.extend(second.names.clone());
    composed.owners.extend(second.owners.clone());
    composed.members.extend(second.members.clone());
    composed
        .instance_bodies
        .extend(second.instance_bodies.clone());
    composed.poses.extend(second.poses.clone());
    composed
}

impl CadIr {
    /// The ids that the records of `state` reference and that keep `state`
    /// from closing as the state of an evaluation of this document (§5.4):
    /// each id that `state` does not hold and that is a `Topology` record of
    /// the document or no record of it.
    ///
    /// # Errors
    ///
    /// Returns a codec error where `ctx` refuses the work.
    pub fn open_references(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        state: &Topology,
    ) -> Result<BTreeSet<String>, cadmpeg_core::CodecError> {
        use crate::schema::rewrite::typed::RewriteIdentities;

        let mut held = BTreeSet::new();
        let mut topology = BTreeSet::new();
        let deltas = self
            .design
            .evaluations
            .iter()
            .filter_map(|evaluation| evaluation.content.as_ref())
            .map(|content| &content.delta.records);
        macro_rules! ids {
            ($($field:ident $arm:ident),*) => {$(
                held.extend(state.$field.iter().map(ArenaRecord::record_identity));
                topology.extend(self.geometry.$field.iter().map(ArenaRecord::record_identity));
                for records in deltas.clone() {
                    topology.extend(records.$field.iter().map(ArenaRecord::record_identity));
                }
            )*};
        }
        topology_arenas!(ids);
        let index = crate::index::ModelIndex::build(self);
        let mut open = BTreeSet::new();
        macro_rules! scan {
            ($($field:ident $arm:ident),*) => {$(
                for record in &state.$field {
                    record.visit_identity_references(ctx, &mut |target| {
                        if crate::validate::design::resolved::leaves_state(
                            held.contains(target),
                            target,
                            &topology,
                            &index,
                        ) {
                            open.insert(target.to_owned());
                        }
                        Ok(())
                    })?;
                }
            )*};
        }
        topology_arenas!(scan);
        Ok(open)
    }

    /// The definition that lists a body among its bodies.
    pub(super) fn body_owner(&self, body: &BodyId) -> Option<&ProductDefinitionId> {
        self.model
            .product_definitions
            .iter()
            .find(|definition| definition.bodies.contains(body))
            .map(|definition| &definition.id)
    }

    /// The extent of a current state of `scope`: its bodies and the records
    /// they reach, their names, the owner of each body that another
    /// definition than the scope's owns, and its poses.
    pub(super) fn extent(
        &self,
        scope: &ProductDefinitionId,
        current: &CurrentState,
    ) -> Result<State, EditError> {
        let reached = reached(&self.geometry, &current.bodies)?;
        let records = select(&self.geometry, &reached);
        let names = self
            .design
            .names
            .entries()
            .iter()
            .filter(|(entity, _)| reached.contains(entity.id()))
            .map(|(entity, entry)| (entity.clone(), entry.clone()))
            .collect();
        let owners = current
            .bodies
            .iter()
            .filter_map(|body| Some((body.clone(), self.body_owner(body)?.clone())))
            .filter(|(_, owner)| owner != scope)
            .collect();
        let bodies: std::collections::BTreeSet<&BodyId> = current.bodies.iter().collect();
        let spanned = self.spanned_definitions(scope);
        let instance_bodies = self
            .model
            .product_definitions
            .iter()
            .filter(|definition| spanned.contains(definition.id.as_str()))
            .flat_map(|definition| &definition.instance_bodies)
            .filter_map(|entry| {
                let kept: Vec<BodyId> = entry
                    .bodies
                    .iter()
                    .filter(|body| bodies.contains(body))
                    .cloned()
                    .collect();
                (!kept.is_empty()).then(|| (entry.occurrence.clone(), kept))
            })
            .collect();
        Ok(State {
            records,
            names,
            owners,
            instance_bodies,
            poses: current.poses.clone(),
            ..State::default()
        })
    }

    /// The current state of a scope at a position.
    pub(super) fn current_state(
        &self,
        scope: &ProductDefinitionId,
        at: &Position,
    ) -> Option<&CurrentState> {
        self.design
            .design_scope(scope)?
            .current
            .iter()
            .find(|current| &current.at == at)
    }

    fn evaluation(&self, id: &EvaluationId) -> Option<&Evaluation> {
        self.design
            .evaluations
            .iter()
            .find(|evaluation| &evaluation.id == id)
    }

    /// The state an evaluation's base holds.
    pub(super) fn base_state(
        &self,
        scope: &ProductDefinitionId,
        base: &StateBase,
    ) -> Result<State, EditError> {
        match base {
            StateBase::Empty => Ok(State::default()),
            StateBase::Current(at) => match self.current_state(scope, at) {
                Some(current) => self.extent(scope, current),
                None => Ok(State::default()),
            },
            StateBase::Evaluation(id) => self.resolved(id),
        }
    }

    /// The records of the state the delta of the evaluation `id` applies to,
    /// or `None` where the evaluation stores no state or its base does not
    /// resolve.
    pub(crate) fn evaluation_base_records(&self, id: &str) -> Option<Topology> {
        let evaluation = self
            .design
            .evaluations
            .iter()
            .find(|evaluation| evaluation.id.as_str() == id)?;
        let content = evaluation.content.as_ref()?;
        let base = self.base_state(&evaluation.scope, &content.base).ok()?;
        Some(base.records)
    }

    /// The records of the state the delta of the process state `id` applies
    /// to: its chain of process bases with each delta applied over the
    /// design state the chain starts from. `None` where a state of the chain
    /// stores no content, a base does not resolve, or the chain is a cycle.
    /// A current design state is the one of the scope whose current states
    /// hold its position.
    pub(crate) fn process_base_records(&self, id: &str) -> Option<Topology> {
        use crate::process::state::ProcessBase;
        let content = |id: &str| {
            self.process
                .states
                .iter()
                .find(|state| state.id.as_str() == id)
                .and_then(|state| state.content.as_ref())
        };
        let mut deltas = Vec::new();
        let mut base = &content(id)?.base;
        let mut state = loop {
            match base {
                ProcessBase::State(parent) => {
                    if deltas.len() >= self.process.states.len() {
                        return None;
                    }
                    let parent = content(parent.as_str())?;
                    deltas.push(&parent.delta);
                    base = &parent.base;
                }
                ProcessBase::Design(StateBase::Current(at)) => {
                    let mut holders = self.design.scopes.iter().filter_map(|scope| {
                        let current = scope.current.iter().find(|current| &current.at == at)?;
                        Some((scope, current))
                    });
                    let (scope, current) = holders.next()?;
                    if holders.next().is_some() {
                        return None;
                    }
                    break self.extent(&scope.definition, current).ok()?;
                }
                ProcessBase::Design(StateBase::Empty) => break State::default(),
                ProcessBase::Design(StateBase::Evaluation(evaluation)) => {
                    break self.resolved(evaluation).ok()?;
                }
            }
        };
        for delta in deltas.into_iter().rev() {
            state.apply(delta);
        }
        Some(state.records)
    }

    /// An evaluation's state: its chain of bases with each delta applied.
    pub(super) fn resolved(&self, id: &EvaluationId) -> Result<State, EditError> {
        let mut chain: Vec<&Evaluation> = Vec::new();
        let mut next = self.evaluation(id);
        let mut root = State::default();
        while let Some(evaluation) = next {
            if chain.iter().any(|seen| seen.id == evaluation.id) {
                return Err(malformed(format!(
                    "evaluation {} is its own ancestor",
                    evaluation.id
                )));
            }
            chain.push(evaluation);
            next = match &evaluation.content {
                Some(StateContent {
                    base: StateBase::Evaluation(base),
                    ..
                }) => {
                    let found = self.evaluation(base);
                    if found.is_none() {
                        root = State::default();
                    }
                    found
                }
                Some(StateContent { base, .. }) => {
                    root = self.base_state(&evaluation.scope, base)?;
                    None
                }
                None => None,
            };
        }
        for evaluation in chain.into_iter().rev() {
            if let Some(content) = &evaluation.content {
                root.apply(&content.delta);
            }
        }
        Ok(root)
    }
}
