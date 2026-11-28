// SPDX-License-Identifier: Apache-2.0
//! The nameable entities and name tables of resolved states (§5.4, §5.5).
//!
//! A stored evaluation's state is its base with its delta applied; a
//! `Current(p)` base is the extent of the scope's current state at `p`: its
//! bodies, the records they reach, and those records' name entries. Lookups
//! walk down an evaluation's chain of bases, so no state is materialized.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use crate::design::evaluation::{CurrentState, Evaluation, StateBase, StateDelta};
use crate::design::names::NameEntry;
use crate::design::refs::EntityRef;
use crate::design::selection::OccurrencePath;
use crate::document::{CadIr, Topology};
use crate::ids::ProductDefinitionId;
use crate::topology::EdgeCarrier;

use super::records::PositionKey;
use super::resolved::{records, Arena};

/// The nameable entities of one `Topology`, by id.
fn entities(topology: &Topology) -> HashMap<&str, Arena> {
    records(topology)
        .into_iter()
        .filter(|(arena, ..)| arena.is_entity())
        .map(|(arena, id, _)| (id, arena))
        .collect()
}

/// One evaluation's own records and removals.
struct Layer<'a> {
    evaluation: &'a Evaluation,
    entities: HashMap<&'a str, Arena>,
    removed: HashSet<&'a str>,
}

/// The extent of one current state: the records its bodies reach.
pub(super) struct Extent<'a> {
    pub(super) scope: &'a str,
    pub(super) state: &'a CurrentState,
    pub(super) records: HashSet<&'a str>,
}

/// The records one `instance_bodies` entry of the current geometry reaches.
pub(super) struct Instance<'a> {
    pub(super) path: &'a OccurrencePath,
    pub(super) records: HashSet<&'a str>,
}

/// The resolved states of a document's evaluations and current states.
pub(in crate::validate) struct States<'a> {
    ir: &'a CadIr,
    geometry: HashMap<&'a str, Arena>,
    layers: HashMap<&'a str, Layer<'a>>,
    pub(super) extents: Vec<Rc<Extent<'a>>>,
    extents_by_scope: HashMap<&'a str, Vec<usize>>,
    extent_by_scope_position: HashMap<(&'a str, PositionKey<'a>), usize>,
    pub(super) instances: Vec<Instance<'a>>,
}

/// A stored evaluation's chain of deltas, from its own down.
pub(super) struct Chain<'s, 'a> {
    /// Each delta's records and the records it removes from its base.
    pub(super) layers: Vec<(&'a Topology, &'s HashSet<&'a str>)>,
    /// The current state's extent the chain ends on, where it ends on one.
    pub(super) bottom: Option<&'s Extent<'a>>,
}

/// Where a lookup in an evaluation's chain stops.
enum Bottom<'s, 'a> {
    Empty,
    Current(&'s Extent<'a>),
}

impl<'a> States<'a> {
    pub(in crate::validate) fn new(ir: &'a CadIr) -> Self {
        let mut layers = HashMap::new();
        for evaluation in &ir.design.evaluations {
            let Some(content) = &evaluation.content else {
                continue;
            };
            layers
                .entry(evaluation.id.as_str())
                .or_insert_with(|| Layer {
                    evaluation,
                    entities: entities(&content.delta.records),
                    removed: content
                        .delta
                        .removed
                        .iter()
                        .map(|record| Arena::of_record(record).1)
                        .collect(),
                });
        }
        let has_extents = ir
            .design
            .scopes
            .iter()
            .any(|scope| !scope.current.is_empty());
        let mut extents: Vec<Rc<Extent<'a>>> = Vec::new();
        let mut instances: Vec<Instance<'a>> = Vec::new();
        if has_extents {
            let reach = Reach::new(&ir.geometry);
            for scope in &ir.design.scopes {
                for state in &scope.current {
                    extents.push(Rc::new(Extent {
                        scope: scope.definition.as_str(),
                        state,
                        records: reach
                            .reached_from(state.bodies.iter().map(crate::ids::BodyId::as_str)),
                    }));
                }
            }
            for definition in &ir.model.product_definitions {
                for entry in &definition.instance_bodies {
                    instances.push(Instance {
                        path: &entry.occurrence,
                        records: reach
                            .reached_from(entry.bodies.iter().map(crate::ids::BodyId::as_str)),
                    });
                }
            }
        }
        let mut extents_by_scope: HashMap<&'a str, Vec<usize>> = HashMap::new();
        let mut extent_by_scope_position: HashMap<(&'a str, PositionKey<'a>), usize> =
            HashMap::new();
        for (i, extent) in extents.iter().enumerate() {
            extents_by_scope.entry(extent.scope).or_default().push(i);
            extent_by_scope_position
                .entry((extent.scope, PositionKey::of(&extent.state.at)))
                .or_insert(i);
        }
        Self {
            ir,
            geometry: entities(&ir.geometry),
            layers,
            extents,
            extents_by_scope,
            extent_by_scope_position,
            instances,
        }
    }

    /// The extent of `scope`'s current state at `at`.
    fn extent_ref(&self, scope: &str, at: PositionKey<'_>) -> Option<&Extent<'a>> {
        self.extent_by_scope_position
            .get(&(scope, at))
            .map(|&i| self.extents[i].as_ref())
    }

    /// The extent of `scope`'s current state at `at`.
    pub(super) fn extent(&self, scope: &str, at: PositionKey<'_>) -> Option<Rc<Extent<'a>>> {
        self.extent_by_scope_position
            .get(&(scope, at))
            .map(|&i| self.extents[i].clone())
    }

    /// Every current-state extent of `scope`, in arena order.
    pub(super) fn extents_for_scope(&self, scope: &str) -> Vec<Rc<Extent<'a>>> {
        self.extents_by_scope
            .get(scope)
            .map_or_else(Vec::new, |indices| {
                indices.iter().map(|&i| self.extents[i].clone()).collect()
            })
    }

    /// Every entity of the current geometry within `extents` that
    /// `Design.names` names, with its entry.
    pub(super) fn extent_table(
        &self,
        extents: &[Rc<Extent<'a>>],
    ) -> Vec<(&'a EntityRef, &'a NameEntry)> {
        self.ir
            .design
            .names
            .entries()
            .iter()
            .filter(|(entity, _)| {
                extents
                    .iter()
                    .any(|extent| extent.records.contains(entity.id()))
            })
            .collect()
    }

    /// Whether the geometry holds the entity in its arena.
    pub(super) fn geometry_holds(&self, entity: &EntityRef) -> bool {
        self.geometry.get(entity.id()) == Some(&Arena::of_entity(entity).0)
    }

    /// Whether the resolved state of a stored evaluation holds the entity in
    /// its arena. `None` where the chain of bases does not resolve.
    pub(super) fn holds(&self, evaluation: &str, entity: &EntityRef) -> Option<bool> {
        let id = entity.id();
        let arena = Arena::of_entity(entity).0;
        let mut at = self.layers.get(evaluation)?;
        for _ in 0..=self.layers.len() {
            if at.removed.contains(id) {
                return Some(false);
            }
            if let Some(found) = at.entities.get(id) {
                return Some(*found == arena);
            }
            match self.bottom_or_next(at)? {
                Ok(next) => at = next,
                Err(Bottom::Empty) => return Some(false),
                Err(Bottom::Current(extent)) => {
                    return Some(
                        extent.records.contains(id) && self.geometry.get(id) == Some(&arena),
                    );
                }
            }
        }
        None
    }

    /// Every entity the resolved state of a stored evaluation names, with its
    /// entry. `None` where the chain of bases does not resolve.
    pub(super) fn table(&self, evaluation: &str) -> Option<BTreeMap<&'a EntityRef, &'a NameEntry>> {
        let mut seen: BTreeMap<&'a EntityRef, Option<&'a NameEntry>> = BTreeMap::new();
        let mut at = self.layers.get(evaluation)?;
        for _ in 0..=self.layers.len() {
            if let Some(content) = &at.evaluation.content {
                for (entity, entry) in &content.delta.names {
                    seen.entry(entity).or_insert(entry.as_ref());
                }
            }
            match self.bottom_or_next(at)? {
                Ok(next) => at = next,
                Err(Bottom::Empty) => break,
                Err(Bottom::Current(extent)) => {
                    for (entity, entry) in self.ir.design.names.entries() {
                        if extent.records.contains(entity.id()) {
                            seen.entry(entity).or_insert(Some(entry));
                        }
                    }
                    break;
                }
            }
        }
        Some(
            seen.into_iter()
                .filter_map(|(entity, entry)| Some((entity, entry?)))
                .filter(|(entity, _)| self.holds(evaluation, entity) == Some(true))
                .collect(),
        )
    }

    /// The current geometry.
    pub(super) const fn geometry(&self) -> &'a Topology {
        &self.ir.geometry
    }

    /// The deltas of a stored evaluation's chain of bases, its own first, each
    /// with the records it removes from its base, and the extent the chain
    /// ends on where it ends on a current state. `None` where the chain does
    /// not resolve.
    pub(super) fn chain(&self, evaluation: &str) -> Option<Chain<'_, 'a>> {
        let mut chain = Chain {
            layers: Vec::new(),
            bottom: None,
        };
        let mut at = self.layers.get(evaluation)?;
        for _ in 0..=self.layers.len() {
            let evaluation = at.evaluation;
            let content = evaluation.content.as_ref()?;
            chain.layers.push((&content.delta.records, &at.removed));
            match &content.base {
                StateBase::Empty => return Some(chain),
                StateBase::Evaluation(base) => at = self.layers.get(base.as_str())?,
                StateBase::Current(position) => {
                    chain.bottom = Some(
                        self.extent_ref(evaluation.scope.as_str(), PositionKey::of(position))?,
                    );
                    return Some(chain);
                }
            }
        }
        None
    }

    /// The instance paths with bodies in the resolved state of a stored
    /// evaluation, down to the first base outside the evaluations.
    pub(super) fn instance_paths(&self, evaluation: &str) -> Vec<&'a OccurrencePath> {
        let (paths, _) = self.merged(evaluation, |delta| &delta.instance_bodies);
        paths
            .into_iter()
            .filter_map(|(path, bodies)| bodies.map(|_| path))
            .collect()
    }

    /// The owner of each occurrence that the resolved state of a stored
    /// evaluation restates in `members`.
    pub(super) fn members(&self, evaluation: &str) -> BTreeMap<&'a str, &'a ProductDefinitionId> {
        let (members, _) = self.merged(evaluation, |delta| &delta.members);
        members
            .into_iter()
            .filter_map(|(occurrence, owner)| owner.map(|owner| (occurrence.as_str(), owner)))
            .collect()
    }

    /// The paths with a pose in the resolved state of a stored evaluation,
    /// the poses of the current state its chain ends on included.
    pub(super) fn pose_paths(&self, evaluation: &str) -> Vec<&'a OccurrencePath> {
        let (mut poses, bottom) = self.merged(evaluation, |delta| &delta.poses);
        if let Some(extent) = bottom {
            for (path, pose) in &extent.state.poses {
                poses.entry(path).or_insert(Some(pose));
            }
        }
        poses
            .into_iter()
            .filter_map(|(path, pose)| pose.map(|_| path))
            .collect()
    }

    /// Each key of one delta map in a stored evaluation's chain, with its
    /// entry in the topmost delta that states it (`None` removes it), and the
    /// current state's extent the chain ends on, where it ends on one.
    fn merged<K: Ord, V>(
        &self,
        evaluation: &str,
        map: impl Fn(&'a StateDelta) -> &'a BTreeMap<K, Option<V>>,
    ) -> (BTreeMap<&'a K, Option<&'a V>>, Option<&Extent<'a>>) {
        let mut entries = BTreeMap::new();
        let Some(mut at) = self.layers.get(evaluation) else {
            return (entries, None);
        };
        for _ in 0..=self.layers.len() {
            if let Some(content) = &at.evaluation.content {
                for (key, value) in map(&content.delta) {
                    entries.entry(key).or_insert(value.as_ref());
                }
            }
            match self.bottom_or_next(at) {
                Some(Ok(next)) => at = next,
                Some(Err(Bottom::Current(extent))) => return (entries, Some(extent)),
                Some(Err(Bottom::Empty)) | None => break,
            }
        }
        (entries, None)
    }

    fn bottom_or_next(&self, layer: &Layer<'a>) -> Option<Result<&Layer<'a>, Bottom<'_, 'a>>> {
        let evaluation = layer.evaluation;
        let content = evaluation.content.as_ref()?;
        Some(match &content.base {
            StateBase::Empty => Err(Bottom::Empty),
            StateBase::Evaluation(base) => Ok(self.layers.get(base.as_str())?),
            StateBase::Current(position) => Err(Bottom::Current(
                self.extent_ref(evaluation.scope.as_str(), PositionKey::of(position))?,
            )),
        })
    }
}

/// The child records of each record of one `Topology`, through the parent
/// pointers and the edges, vertices, and carriers they use.
struct Reach<'a> {
    children: HashMap<&'a str, Vec<&'a str>>,
}

impl<'a> Reach<'a> {
    fn new(topology: &'a Topology) -> Self {
        let mut children: HashMap<&'a str, Vec<&'a str>> = HashMap::new();
        let mut add =
            |parent: &'a str, child: &'a str| children.entry(parent).or_default().push(child);
        for region in &topology.regions {
            add(region.body.as_str(), region.id.as_str());
        }
        for shell in &topology.shells {
            add(shell.region.as_str(), shell.id.as_str());
            for member in &shell.members {
                add(shell.id.as_str(), member.as_str());
            }
        }
        for face in &topology.faces {
            add(face.shell.as_str(), face.id.as_str());
            add(face.id.as_str(), face.surface.as_str());
        }
        for record in &topology.loops {
            add(record.face.as_str(), record.id.as_str());
        }
        for coedge in &topology.coedges {
            add(coedge.owner_loop.as_str(), coedge.id.as_str());
            add(coedge.id.as_str(), coedge.edge.as_str());
        }
        for edge in &topology.edges {
            if let Some(ends) = &edge.ends {
                add(edge.id.as_str(), ends.start.as_str());
                add(edge.id.as_str(), ends.end.as_str());
            }
            if let EdgeCarrier::Curve(curve) | EdgeCarrier::Bounded(curve, _) = &edge.carrier {
                add(edge.id.as_str(), curve.as_str());
            }
        }
        Self { children }
    }

    fn reached_from(&self, bodies: impl Iterator<Item = &'a str>) -> HashSet<&'a str> {
        let mut found = HashSet::new();
        let mut stack: Vec<&'a str> = bodies.collect();
        while let Some(record) = stack.pop() {
            if found.insert(record) {
                stack.extend(self.children.get(record).into_iter().flatten().copied());
            }
        }
        found
    }
}
