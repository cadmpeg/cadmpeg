// SPDX-License-Identifier: Apache-2.0
//! The nameable entities and name tables of resolved states (§5.4, §5.5).
//!
//! A stored evaluation's state is its base with its delta applied; a
//! `Current(p)` base is the extent of the scope's current state at `p`: its
//! bodies, the records they reach, and those records' name entries. Lookups
//! walk down an evaluation's chain of bases, so no state is materialized.

use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use crate::design::evaluation::{CurrentState, Evaluation, StateBase};
use crate::design::names::{NameEntry, TopoName};
use crate::design::refs::EntityRef;
use crate::design::selection::OccurrencePath;
use crate::document::{CadIr, Topology};
use crate::topology::{EdgeCarrier, ShellMember};

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
    pub(super) index: usize,
    pub(super) scope: &'a str,
    pub(super) state: &'a CurrentState,
    pub(super) records: HashSet<&'a str>,
}

/// The resolved states of a document's evaluations and current states.
pub(super) struct States<'a> {
    ir: &'a CadIr,
    geometry: HashMap<&'a str, Arena>,
    layers: HashMap<&'a str, Layer<'a>>,
    pub(super) extents: Vec<Extent<'a>>,
    extent_names: Vec<OnceCell<BTreeSet<&'a TopoName>>>,
    eval_names: RefCell<HashMap<&'a str, Option<Rc<BTreeSet<&'a TopoName>>>>>,
}

/// Where a lookup in an evaluation's chain stops.
enum Bottom<'a> {
    Empty,
    Current(&'a Extent<'a>),
}

impl<'a> States<'a> {
    pub(super) fn new(ir: &'a CadIr) -> Self {
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
        let mut extents: Vec<Extent<'a>> = Vec::new();
        if has_extents {
            let reach = Reach::new(&ir.geometry);
            for scope in &ir.design.scopes {
                for state in &scope.current {
                    let index = extents.len();
                    extents.push(Extent {
                        index,
                        scope: scope.definition.as_str(),
                        state,
                        records: reach
                            .reached_from(state.bodies.iter().map(crate::ids::BodyId::as_str)),
                    });
                }
            }
        }
        let extent_names = (0..extents.len()).map(|_| OnceCell::new()).collect();
        Self {
            ir,
            geometry: entities(&ir.geometry),
            layers,
            extents,
            extent_names,
            eval_names: RefCell::new(HashMap::new()),
        }
    }

    /// The extent of `scope`'s current state at `at`.
    pub(super) fn extent(&self, scope: &str, at: PositionKey<'_>) -> Option<&Extent<'a>> {
        self.extents
            .iter()
            .find(|extent| extent.scope == scope && PositionKey::of(&extent.state.at) == at)
    }

    /// Whether any entity in `extents` carries `name` in `Design.names`.
    pub(super) fn extents_contain_name(&self, extents: &[&Extent<'a>], name: &TopoName) -> bool {
        extents.iter().any(|extent| {
            let names = self.extent_names[extent.index].get_or_init(|| {
                self.ir
                    .design
                    .names
                    .entries()
                    .iter()
                    .filter(|(entity, _)| extent.records.contains(entity.id()))
                    .filter_map(|(_, entry)| entry.name.as_ref())
                    .collect()
            });
            names.contains(name)
        })
    }

    /// Whether the resolved state of `evaluation` carries `name`; `None` where
    /// the chain of bases does not resolve.
    pub(super) fn evaluation_contains_name(
        &self,
        evaluation: &str,
        name: &TopoName,
    ) -> Option<bool> {
        if let Some(cached) = self.eval_names.borrow().get(evaluation) {
            return cached.as_ref().map(|names| names.contains(name));
        }
        let (&key, _) = self.layers.get_key_value(evaluation)?;
        let table = self.table(key).map(|table| {
            Rc::new(
                table
                    .into_values()
                    .filter_map(|entry| entry.name.as_ref())
                    .collect::<BTreeSet<_>>(),
            )
        });
        let found = table.as_ref().map(|names| names.contains(name));
        self.eval_names.borrow_mut().insert(key, table);
        found
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
    fn table(&self, evaluation: &str) -> Option<BTreeMap<&'a EntityRef, &'a NameEntry>> {
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

    /// The instance paths with bodies in the resolved state of a stored
    /// evaluation, down to the first base outside the evaluations.
    pub(super) fn instance_paths(&self, evaluation: &str) -> Vec<&'a OccurrencePath> {
        let mut paths = BTreeMap::new();
        let Some(mut at) = self.layers.get(evaluation) else {
            return Vec::new();
        };
        for _ in 0..=self.layers.len() {
            if let Some(content) = &at.evaluation.content {
                for (path, bodies) in &content.delta.instance_bodies {
                    paths.entry(path).or_insert(bodies.is_some());
                }
            }
            match self.bottom_or_next(at) {
                Some(Ok(next)) => at = next,
                Some(Err(_)) | None => break,
            }
        }
        paths
            .into_iter()
            .filter_map(|(path, held)| held.then_some(path))
            .collect()
    }

    fn bottom_or_next(&self, layer: &Layer<'a>) -> Option<Result<&Layer<'a>, Bottom<'_>>> {
        let evaluation = layer.evaluation;
        let content = evaluation.content.as_ref()?;
        Some(match &content.base {
            StateBase::Empty => Err(Bottom::Empty),
            StateBase::Evaluation(base) => Ok(self.layers.get(base.as_str())?),
            StateBase::Current(position) => Err(Bottom::Current(
                self.extent(evaluation.scope.as_str(), PositionKey::of(position))?,
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
                match member {
                    ShellMember::WireEdge { id } => add(shell.id.as_str(), id.as_str()),
                    ShellMember::FreeVertex { id } => add(shell.id.as_str(), id.as_str()),
                }
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
