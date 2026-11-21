// SPDX-License-Identifier: Apache-2.0
//! The matching rule of §5.7 in a stored state. A name matches each entity
//! whose table entry carries it. A `Bounded` name also matches the edges the
//! state's adjacency places between faces matching its `faces`, and a
//! `Meeting` name the vertices where faces matching its `faces` meet. Names
//! match with every `index` left out, at any depth, and the entities so
//! matched are the candidates. Each index the pick states then keeps, at its
//! place in the name, the candidates whose entries state the same index
//! there; each place is judged against all the candidates, and names of one
//! set that are equal with their indexes left out are one place. A `Bounded`
//! name's `ends` narrows the candidates its indexes keep.
//!
//! Interim: this module matches each name under two readings. Against table
//! entries, only the outer `index` narrows: under `Loose` every nested index
//! is left out of the comparison; under `Exact` a nested name compares whole,
//! its index included. Through adjacency, `Bounded` and `Meeting` resolve
//! each name in `faces` (and `ends`) recursively through `resolve`, so a
//! face or end name's own `index` narrows its matches independently, rather
//! than against the outer candidates. A pick is lost only where it matches
//! nothing under either reading, and an entity is selected only where both
//! readings select it. `ends` narrows a `Bounded` name's adjacency candidates
//! before the outer `index` narrows.

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use crate::design::names::{Generator, TopoName};
use crate::document::Topology;
use crate::geometry::Curve;
use crate::schema::EntitySchema;
use crate::topology::{Coedge, Edge, Loop, LoopBoundary, Point, Vertex};

use super::selections::Stored;
use super::states::{Extent, States};

/// How an `index` nested inside a name compares.
#[derive(Clone, Copy)]
enum Reading {
    /// A nested name compares whole, its index included.
    Exact,
    /// Every index, nested ones included, is left out of the comparison.
    Loose,
}

/// The topology records of one stored state, by identity, and the adjacency
/// the matching rule reads.
pub(super) struct StateTopology<'a> {
    pub(super) edges: BTreeMap<&'a str, &'a Edge>,
    pub(super) vertices: BTreeMap<&'a str, &'a Vertex>,
    pub(super) points: BTreeMap<&'a str, &'a Point>,
    pub(super) curves: BTreeMap<&'a str, &'a Curve>,
    /// The faces that use each edge through a coedge.
    edge_faces: BTreeMap<&'a str, BTreeSet<&'a str>>,
    /// The faces on which each vertex lies.
    vertex_faces: BTreeMap<&'a str, BTreeSet<&'a str>>,
}

/// The records of one stored state, collected before adjacency is derived.
#[derive(Default)]
struct Collected<'a> {
    faces: BTreeSet<&'a str>,
    loops: BTreeMap<&'a str, &'a Loop>,
    coedges: BTreeMap<&'a str, &'a Coedge>,
    edges: BTreeMap<&'a str, &'a Edge>,
    vertices: BTreeMap<&'a str, &'a Vertex>,
    points: BTreeMap<&'a str, &'a Point>,
    curves: BTreeMap<&'a str, &'a Curve>,
}

impl<'a> Collected<'a> {
    /// Adds the records of `topology` that `keep` admits and that no record
    /// added before holds the identity of. Carriers are admitted by `carry`.
    fn add(
        &mut self,
        topology: &'a Topology,
        keep: &dyn Fn(&str) -> bool,
        carry: &dyn Fn(&str) -> bool,
    ) {
        fn put<'a, T: EntitySchema>(
            into: &mut BTreeMap<&'a str, &'a T>,
            records: &'a [T],
            keep: &dyn Fn(&str) -> bool,
        ) {
            for record in records {
                let id = record.identity();
                if keep(id) {
                    into.entry(id).or_insert(record);
                }
            }
        }
        for face in &topology.faces {
            let id = face.identity();
            if keep(id) {
                self.faces.insert(id);
            }
        }
        put(&mut self.loops, &topology.loops, keep);
        put(&mut self.coedges, &topology.coedges, keep);
        put(&mut self.edges, &topology.edges, keep);
        put(&mut self.vertices, &topology.vertices, keep);
        put(&mut self.points, &topology.points, carry);
        put(&mut self.curves, &topology.curves, carry);
    }

    fn finish(self) -> StateTopology<'a> {
        let mut edge_faces: BTreeMap<&'a str, BTreeSet<&'a str>> = BTreeMap::new();
        let mut vertex_faces: BTreeMap<&'a str, BTreeSet<&'a str>> = BTreeMap::new();
        for coedge in self.coedges.values() {
            let Some(record) = self.loops.get(coedge.owner_loop.as_str()) else {
                continue;
            };
            let face = record.face.as_str();
            if self.faces.contains(face) && self.edges.contains_key(coedge.edge.as_str()) {
                edge_faces
                    .entry(coedge.edge.as_str())
                    .or_default()
                    .insert(face);
            }
        }
        for (edge, faces) in &edge_faces {
            for vertex in self.edges[edge].vertices() {
                vertex_faces
                    .entry(vertex.as_str())
                    .or_default()
                    .extend(faces.iter().copied());
            }
        }
        for record in self.loops.values() {
            if let LoopBoundary::Vertex { vertex, .. } = &record.boundary {
                if self.faces.contains(record.face.as_str()) {
                    vertex_faces
                        .entry(vertex.as_str())
                        .or_default()
                        .insert(record.face.as_str());
                }
            }
        }
        StateTopology {
            edges: self.edges,
            vertices: self.vertices,
            points: self.points,
            curves: self.curves,
            edge_faces,
            vertex_faces,
        }
    }
}

impl<'a> StateTopology<'a> {
    fn of_extents(geometry: &'a Topology, extents: &[&Extent<'a>]) -> Self {
        let mut collected = Collected::default();
        collected.add(
            geometry,
            &|id| extents.iter().any(|extent| extent.records.contains(id)),
            &|_| true,
        );
        collected.finish()
    }

    fn of_evaluation(states: &States<'a>, evaluation: &str) -> Option<Self> {
        let chain = states.chain(evaluation)?;
        let mut collected = Collected::default();
        let mut hidden: BTreeSet<&'a str> = BTreeSet::new();
        for &(records, removed) in &chain.layers {
            collected.add(records, &|id| !hidden.contains(id), &|id| {
                !hidden.contains(id)
            });
            hidden.extend(removed.iter().copied());
        }
        if let Some(extent) = chain.bottom {
            collected.add(
                states.geometry(),
                &|id| !hidden.contains(id) && extent.records.contains(id),
                &|id| !hidden.contains(id),
            );
        }
        Some(collected.finish())
    }

    /// The faces on which an end vertex of `edge` lies.
    fn end_faces(&self, edge: &str) -> BTreeSet<&'a str> {
        self.edges
            .get(edge)
            .into_iter()
            .flat_map(|edge| edge.vertices())
            .filter_map(|vertex| self.vertex_faces.get(vertex.as_str()))
            .flatten()
            .copied()
            .collect()
    }
}

/// One stored state's name table, with its topology derived on first use.
pub(super) struct Matcher<'a> {
    states: &'a States<'a>,
    state: StateKey<'a>,
    entries: BTreeMap<&'a str, &'a TopoName>,
    topology: OnceCell<Option<StateTopology<'a>>>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum StateKey<'a> {
    Extents(Vec<*const Extent<'a>>),
    Evaluation(&'a str),
}

impl<'a> Matcher<'a> {
    /// The matcher of a stored state. `None` where an evaluation's chain of
    /// bases does not resolve, which the evaluation rules report.
    fn new(states: &'a States<'a>, state: &Stored<'a>) -> Option<Self> {
        let (entries, key) = match state {
            Stored::Extents(extents) => (
                states
                    .extent_table(extents)
                    .into_iter()
                    .filter_map(|(entity, entry)| Some((entity.id(), entry.name.as_ref()?)))
                    .collect(),
                StateKey::Extents(
                    extents
                        .iter()
                        .map(|extent| std::ptr::from_ref(*extent))
                        .collect(),
                ),
            ),
            Stored::Evaluation(evaluation) => (
                states
                    .table(evaluation)?
                    .into_iter()
                    .filter_map(|(entity, entry)| Some((entity.id(), entry.name.as_ref()?)))
                    .collect(),
                StateKey::Evaluation(evaluation),
            ),
        };
        Some(Self {
            states,
            state: key,
            entries,
            topology: OnceCell::new(),
        })
    }

    /// The state's topology records. `None` where the chain does not resolve.
    pub(super) fn topology(&self) -> Option<&StateTopology<'a>> {
        self.topology
            .get_or_init(|| match &self.state {
                StateKey::Extents(extents) => {
                    let extents: Vec<&Extent<'a>> = self
                        .states
                        .extents
                        .iter()
                        .filter(|extent| extents.contains(&std::ptr::from_ref(*extent)))
                        .collect();
                    Some(StateTopology::of_extents(self.states.geometry(), &extents))
                }
                StateKey::Evaluation(evaluation) => {
                    StateTopology::of_evaluation(self.states, evaluation)
                }
            })
            .as_ref()
    }

    /// Whether `name` matches an entity of the state under some reading.
    pub(super) fn matches_any(&self, name: &TopoName) -> bool {
        !self.resolve(name, Reading::Loose).is_empty()
            || !self.resolve(name, Reading::Exact).is_empty()
    }

    /// The entities `name` matches under every reading.
    pub(super) fn matched(&self, name: &TopoName) -> BTreeSet<&'a str> {
        let loose = self.resolve(name, Reading::Loose);
        let exact = self.resolve(name, Reading::Exact);
        loose.intersection(&exact).copied().collect()
    }

    /// The entities `name` matches under any reading.
    pub(super) fn possible(&self, name: &TopoName) -> BTreeSet<&'a str> {
        let mut found = self.resolve(name, Reading::Loose);
        found.extend(self.resolve(name, Reading::Exact));
        found
    }

    fn resolve(&self, name: &TopoName, reading: Reading) -> BTreeSet<&'a str> {
        let mut found: BTreeSet<&'a str> = self
            .entries
            .iter()
            .filter(|(_, entry)| same(entry, name, reading, true))
            .map(|(entity, _)| *entity)
            .collect();
        match name {
            TopoName::Bounded { faces, ends, .. } => {
                found.extend(self.bounded(faces, ends, reading));
            }
            TopoName::Meeting { faces, .. } => found.extend(self.meeting(faces, reading)),
            _ => {}
        }
        if let Some(index) = own_index(name) {
            let index_of =
                |entity: &&str| self.entries.get(entity).and_then(|entry| own_index(entry));
            if found.iter().any(|entity| index_of(entity).is_some()) {
                found.retain(|entity| index_of(entity) == Some(index));
            }
        }
        found
    }

    /// The edges between faces matching every name of `faces`, narrowed by
    /// `ends` where there are several and the narrowing keeps one.
    fn bounded(
        &self,
        faces: &BTreeSet<TopoName>,
        ends: &BTreeSet<TopoName>,
        reading: Reading,
    ) -> Vec<&'a str> {
        let Some(topology) = self.topology() else {
            return Vec::new();
        };
        let sides: Vec<BTreeSet<&'a str>> = faces
            .iter()
            .map(|face| self.resolve(face, reading))
            .collect();
        let mut candidates: Vec<&'a str> = if faces.is_empty() {
            topology.edges.keys().copied().collect()
        } else {
            topology
                .edge_faces
                .iter()
                .filter(|(_, beside)| sides.iter().all(|side| !side.is_disjoint(beside)))
                .map(|(edge, _)| *edge)
                .collect()
        };
        if candidates.len() > 1 {
            let named: Vec<BTreeSet<&'a str>> = ends
                .iter()
                .map(|end| self.resolve(end, reading))
                .filter(|end| !end.is_empty())
                .collect();
            let narrowed: Vec<&'a str> = candidates
                .iter()
                .copied()
                .filter(|edge| {
                    let at = topology.end_faces(edge);
                    named.iter().all(|end| !end.is_disjoint(&at))
                })
                .collect();
            if !narrowed.is_empty() {
                candidates = narrowed;
            }
        }
        candidates
    }

    /// The vertices where faces matching every name of `faces` meet.
    fn meeting(&self, faces: &BTreeSet<TopoName>, reading: Reading) -> Vec<&'a str> {
        let Some(topology) = self.topology() else {
            return Vec::new();
        };
        let sides: Vec<BTreeSet<&'a str>> = faces
            .iter()
            .map(|face| self.resolve(face, reading))
            .collect();
        topology
            .vertices
            .keys()
            .copied()
            .filter(|vertex| {
                let at = topology.vertex_faces.get(vertex);
                sides
                    .iter()
                    .all(|side| at.is_some_and(|at| !side.is_disjoint(at)))
            })
            .collect()
    }
}

/// The index a name states of its own, not one nested in it.
const fn own_index(name: &TopoName) -> Option<u32> {
    match name {
        TopoName::Generated { index, .. }
        | TopoName::Blend { index, .. }
        | TopoName::Bounded { index, .. }
        | TopoName::Meeting { index, .. } => *index,
        TopoName::Instance { .. }
        | TopoName::Modified { .. }
        | TopoName::Split { .. }
        | TopoName::Merged { .. }
        | TopoName::Root { .. } => None,
    }
}

/// Whether two names are one name with the index of each left out, and the
/// indexes nested in them left out too under the loose reading.
fn same(left: &TopoName, right: &TopoName, reading: Reading, outer: bool) -> bool {
    if !outer && matches!(reading, Reading::Exact) {
        return left == right;
    }
    let inner = |left: &TopoName, right: &TopoName| same(left, right, reading, false);
    let sets = |left: &BTreeSet<TopoName>, right: &BTreeSet<TopoName>| {
        left.len() == right.len()
            && left
                .iter()
                .all(|left| right.iter().any(|right| inner(left, right)))
            && right
                .iter()
                .all(|right| left.iter().any(|left| inner(left, right)))
    };
    match (left, right) {
        (
            TopoName::Generated {
                kind,
                feature,
                role,
                generator,
                index: _,
            },
            TopoName::Generated {
                kind: other_kind,
                feature: other_feature,
                role: other_role,
                generator: other_generator,
                index: _,
            },
        ) => {
            kind == other_kind
                && feature == other_feature
                && role == other_role
                && match (generator, other_generator) {
                    (Some(Generator::Entity(left)), Some(Generator::Entity(right))) => {
                        inner(left, right)
                    }
                    (left, right) => left == right,
                }
        }
        (
            TopoName::Instance {
                feature,
                instance,
                of,
            },
            TopoName::Instance {
                feature: other_feature,
                instance: other_instance,
                of: other_of,
            },
        ) => feature == other_feature && instance == other_instance && inner(of, other_of),
        (
            TopoName::Blend {
                feature,
                between,
                index: _,
            },
            TopoName::Blend {
                feature: other_feature,
                between: other_between,
                index: _,
            },
        ) => {
            feature == other_feature
                && between.len() == other_between.len()
                && between
                    .iter()
                    .zip(other_between)
                    .all(|(left, right)| inner(left, right))
        }
        (
            TopoName::Bounded {
                faces,
                ends,
                index: _,
            },
            TopoName::Bounded {
                faces: other_faces,
                ends: other_ends,
                index: _,
            },
        ) => sets(faces, other_faces) && sets(ends, other_ends),
        (
            TopoName::Meeting { faces, index: _ },
            TopoName::Meeting {
                faces: other_faces,
                index: _,
            },
        ) => sets(faces, other_faces),
        (
            TopoName::Modified { feature, from },
            TopoName::Modified {
                feature: other_feature,
                from: other_from,
            },
        ) => feature == other_feature && inner(from, other_from),
        (
            TopoName::Split {
                feature,
                from,
                piece,
            },
            TopoName::Split {
                feature: other_feature,
                from: other_from,
                piece: other_piece,
            },
        ) => feature == other_feature && piece == other_piece && inner(from, other_from),
        (
            TopoName::Merged { feature, from },
            TopoName::Merged {
                feature: other_feature,
                from: other_from,
            },
        ) => feature == other_feature && sets(from, other_from),
        (TopoName::Root { container }, TopoName::Root { container: other }) => container == other,
        _ => false,
    }
}

/// The matchers of the stored states one validation reads, each built once.
pub(super) struct Matchers<'a> {
    states: &'a States<'a>,
    built: std::cell::RefCell<BTreeMap<StateKey<'a>, Option<Rc<Matcher<'a>>>>>,
}

impl<'a> Matchers<'a> {
    pub(super) fn new(states: &'a States<'a>) -> Self {
        Self {
            states,
            built: std::cell::RefCell::new(BTreeMap::new()),
        }
    }

    /// The resolved states the matchers read.
    pub(super) const fn states(&self) -> &'a States<'a> {
        self.states
    }

    /// The matcher of a stored state. `None` where an evaluation's chain of
    /// bases does not resolve.
    pub(super) fn get(&self, state: &Stored<'a>) -> Option<Rc<Matcher<'a>>> {
        let key = match state {
            Stored::Extents(extents) => StateKey::Extents(
                extents
                    .iter()
                    .map(|extent| std::ptr::from_ref(*extent))
                    .collect(),
            ),
            Stored::Evaluation(evaluation) => StateKey::Evaluation(evaluation),
        };
        if let Some(found) = self.built.borrow().get(&key) {
            return found.clone();
        }
        let matcher = Matcher::new(self.states, state).map(Rc::new);
        self.built.borrow_mut().insert(key, matcher.clone());
        matcher
    }
}

#[cfg(test)]
mod tests;
