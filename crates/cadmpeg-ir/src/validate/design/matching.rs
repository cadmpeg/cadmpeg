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

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use crate::design::names::{Generator, TopoName};
use crate::document::Topology;
use crate::geometry::{Curve, Surface};
use crate::schema::EntitySchema;
use crate::topology::{
    Body, Coedge, Edge, Face, Loop, LoopBoundary, Point, Region, Shell, ShellMember, Vertex,
};

use super::selections::Stored;
use super::states::{Extent, States};

/// One step down a name's tree to a place where an `index` may sit.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Step {
    Generator,
    Of,
    From,
    Between(usize),
    Face(TopoName),
    End(TopoName),
    Merged(TopoName),
}

type Place = Vec<Step>;

/// One table entry's name stripped of every `index`, with the index sets
/// stated at each of its places.
struct Entry {
    stripped: TopoName,
    places: BTreeMap<Place, BTreeSet<u32>>,
}

impl Entry {
    fn of(name: &TopoName) -> Self {
        Self {
            stripped: stripped(name),
            places: index_places(name),
        }
    }
}

/// The topology records of one stored state, by identity, and the adjacency
/// the matching rule reads.
pub(super) struct StateTopology<'a> {
    pub(super) bodies: BTreeMap<&'a str, &'a Body>,
    regions: BTreeMap<&'a str, &'a Region>,
    shells: BTreeMap<&'a str, &'a Shell>,
    pub(super) faces: BTreeMap<&'a str, &'a Face>,
    pub(super) edges: BTreeMap<&'a str, &'a Edge>,
    pub(super) vertices: BTreeMap<&'a str, &'a Vertex>,
    pub(super) points: BTreeMap<&'a str, &'a Point>,
    pub(super) curves: BTreeMap<&'a str, &'a Curve>,
    pub(super) surfaces: BTreeMap<&'a str, &'a Surface>,
    /// The faces that use each edge through a coedge.
    pub(super) edge_faces: BTreeMap<&'a str, BTreeSet<&'a str>>,
    /// The faces on which each vertex lies.
    pub(super) vertex_faces: BTreeMap<&'a str, BTreeSet<&'a str>>,
    /// The edges that each vertex ends.
    vertex_edges: BTreeMap<&'a str, BTreeSet<&'a str>>,
    /// The shells that list each wire edge.
    wire_shells: BTreeMap<&'a str, Vec<&'a Shell>>,
    /// The shells that list each free vertex.
    free_shells: BTreeMap<&'a str, Vec<&'a Shell>>,
    /// The face loops that use each edge through a coedge.
    pub(super) edge_loops: BTreeMap<&'a str, BTreeSet<&'a str>>,
    /// The edges each face loop uses through its coedges.
    pub(super) loop_edges: BTreeMap<&'a str, BTreeSet<&'a str>>,
}

/// The records of one stored state, collected before adjacency is derived.
#[derive(Default)]
struct Collected<'a> {
    bodies: BTreeMap<&'a str, &'a Body>,
    regions: BTreeMap<&'a str, &'a Region>,
    shells: BTreeMap<&'a str, &'a Shell>,
    faces: BTreeMap<&'a str, &'a Face>,
    loops: BTreeMap<&'a str, &'a Loop>,
    coedges: BTreeMap<&'a str, &'a Coedge>,
    edges: BTreeMap<&'a str, &'a Edge>,
    vertices: BTreeMap<&'a str, &'a Vertex>,
    points: BTreeMap<&'a str, &'a Point>,
    curves: BTreeMap<&'a str, &'a Curve>,
    surfaces: BTreeMap<&'a str, &'a Surface>,
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
        put(&mut self.bodies, &topology.bodies, keep);
        put(&mut self.regions, &topology.regions, keep);
        put(&mut self.shells, &topology.shells, keep);
        put(&mut self.faces, &topology.faces, keep);
        put(&mut self.loops, &topology.loops, keep);
        put(&mut self.coedges, &topology.coedges, keep);
        put(&mut self.edges, &topology.edges, keep);
        put(&mut self.vertices, &topology.vertices, keep);
        put(&mut self.points, &topology.points, carry);
        put(&mut self.curves, &topology.curves, carry);
        put(&mut self.surfaces, &topology.surfaces, carry);
    }

    fn finish(self) -> StateTopology<'a> {
        let mut edge_faces: BTreeMap<&'a str, BTreeSet<&'a str>> = BTreeMap::new();
        let mut vertex_faces: BTreeMap<&'a str, BTreeSet<&'a str>> = BTreeMap::new();
        let mut edge_loops: BTreeMap<&'a str, BTreeSet<&'a str>> = BTreeMap::new();
        let mut loop_edges: BTreeMap<&'a str, BTreeSet<&'a str>> = BTreeMap::new();
        for coedge in self.coedges.values() {
            let Some(record) = self.loops.get(coedge.owner_loop.as_str()) else {
                continue;
            };
            let face = record.face.as_str();
            let edge = coedge.edge.as_str();
            if self.faces.contains_key(face) && self.edges.contains_key(edge) {
                edge_faces.entry(edge).or_default().insert(face);
                let id = record.id.as_str();
                edge_loops.entry(edge).or_default().insert(id);
                loop_edges.entry(id).or_default().insert(edge);
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
        let mut vertex_edges: BTreeMap<&'a str, BTreeSet<&'a str>> = BTreeMap::new();
        for (edge, record) in &self.edges {
            for vertex in record.vertices() {
                vertex_edges
                    .entry(vertex.as_str())
                    .or_default()
                    .insert(edge);
            }
        }
        let mut wire_shells: BTreeMap<&'a str, Vec<&'a Shell>> = BTreeMap::new();
        let mut free_shells: BTreeMap<&'a str, Vec<&'a Shell>> = BTreeMap::new();
        for &shell in self.shells.values() {
            for member in &shell.members {
                match member {
                    ShellMember::WireEdge { id } => {
                        wire_shells.entry(id.as_str()).or_default().push(shell);
                    }
                    ShellMember::FreeVertex { id } => {
                        free_shells.entry(id.as_str()).or_default().push(shell);
                    }
                    ShellMember::Subd { .. } => {}
                }
            }
        }
        for record in self.loops.values() {
            if let LoopBoundary::Vertex { vertex, .. } = &record.boundary {
                if self.faces.contains_key(record.face.as_str()) {
                    vertex_faces
                        .entry(vertex.as_str())
                        .or_default()
                        .insert(record.face.as_str());
                }
            }
        }
        StateTopology {
            bodies: self.bodies,
            regions: self.regions,
            shells: self.shells,
            faces: self.faces,
            edges: self.edges,
            vertices: self.vertices,
            points: self.points,
            curves: self.curves,
            surfaces: self.surfaces,
            edge_faces,
            vertex_faces,
            vertex_edges,
            wire_shells,
            free_shells,
            edge_loops,
            loop_edges,
        }
    }
}

impl<'a> StateTopology<'a> {
    /// Every record of `geometry`.
    pub(super) fn of_geometry(geometry: &'a Topology) -> Self {
        let mut collected = Collected::default();
        collected.add(geometry, &|_| true, &|_| true);
        collected.finish()
    }

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

    /// The body that holds `face` through its shell and region, where the
    /// state holds them.
    pub(super) fn face_body(&self, face: &str) -> Option<&'a Body> {
        let face = self.faces.get(face)?;
        let shell = self.shells.get(face.shell.as_str())?;
        self.shell_body(shell)
    }

    fn shell_body(&self, shell: &Shell) -> Option<&'a Body> {
        let region = self.regions.get(shell.region.as_str())?;
        self.bodies.get(region.body.as_str()).copied()
    }

    /// The bodies that hold `edge`: those of the faces that use it and of the
    /// shells that own it as a wire edge.
    pub(super) fn edge_bodies(&self, edge: &str) -> BTreeSet<&'a str> {
        let mut bodies: BTreeSet<&'a str> = self
            .edge_faces
            .get(edge)
            .into_iter()
            .flatten()
            .filter_map(|face| self.face_body(face))
            .map(|body| body.id.as_str())
            .collect();
        for &shell in self.wire_shells.get(edge).into_iter().flatten() {
            bodies.extend(self.shell_body(shell).map(|body| body.id.as_str()));
        }
        bodies
    }

    /// The bodies that hold `vertex`: those of the faces it lies on, of the
    /// edges it ends, and of the shells that own it as a free vertex.
    pub(super) fn vertex_bodies(&self, vertex: &str) -> BTreeSet<&'a str> {
        let mut bodies: BTreeSet<&'a str> = self
            .vertex_faces
            .get(vertex)
            .into_iter()
            .flatten()
            .filter_map(|face| self.face_body(face))
            .map(|body| body.id.as_str())
            .collect();
        for edge in self.vertex_edges.get(vertex).into_iter().flatten() {
            bodies.extend(self.edge_bodies(edge));
        }
        for &shell in self.free_shells.get(vertex).into_iter().flatten() {
            bodies.extend(self.shell_body(shell).map(|body| body.id.as_str()));
        }
        bodies
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
    entries: BTreeMap<&'a str, Entry>,
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
                    .filter_map(|(entity, entry)| {
                        Some((entity.id(), Entry::of(entry.name.as_ref()?)))
                    })
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
                    .filter_map(|(entity, entry)| {
                        Some((entity.id(), Entry::of(entry.name.as_ref()?)))
                    })
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

    /// Whether `name` matches an entity of the state.
    pub(super) fn matches_any(&self, name: &TopoName) -> bool {
        !self.matched(name).is_empty()
    }

    /// The entities `name` matches in the state by the rule of §5.7.
    pub(super) fn matched(&self, name: &TopoName) -> BTreeSet<&'a str> {
        let target = stripped(name);
        let mut candidates: BTreeSet<&'a str> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.stripped == target)
            .map(|(entity, _)| *entity)
            .collect();
        match name {
            TopoName::Bounded { faces, .. } => {
                candidates.extend(self.bounded_candidates(faces));
            }
            TopoName::Meeting { faces, .. } => {
                candidates.extend(self.meeting(faces));
            }
            _ => {}
        }
        let mut kept = candidates.clone();
        for (place, wanted) in index_places(name) {
            let stated = |entity: &str| {
                self.entries
                    .get(entity)
                    .and_then(|entry| entry.places.get(&place))
            };
            if candidates.iter().any(|&entity| stated(entity).is_some()) {
                kept.retain(|&entity| stated(entity) == Some(&wanted));
            }
        }
        if let TopoName::Bounded { ends, .. } = name {
            if kept.len() > 1 && !ends.is_empty() {
                if let Some(topology) = self.topology() {
                    let named: Vec<BTreeSet<&'a str>> = ends
                        .iter()
                        .map(|end| self.matched(end))
                        .filter(|end| !end.is_empty())
                        .collect();
                    if !named.is_empty() {
                        let narrowed: BTreeSet<&'a str> = kept
                            .iter()
                            .copied()
                            .filter(|edge| {
                                let at = topology.end_faces(edge);
                                named.iter().all(|end| !end.is_disjoint(&at))
                            })
                            .collect();
                        if !narrowed.is_empty() {
                            kept = narrowed;
                        }
                    }
                }
            }
        }
        kept
    }

    /// The edges between faces matching every name of `faces`.
    fn bounded_candidates(&self, faces: &BTreeSet<TopoName>) -> Vec<&'a str> {
        let Some(topology) = self.topology() else {
            return Vec::new();
        };
        if faces.is_empty() {
            return topology.edges.keys().copied().collect();
        }
        let sides: Vec<BTreeSet<&'a str>> = faces.iter().map(|face| self.matched(face)).collect();
        topology
            .edge_faces
            .iter()
            .filter(|(_, beside)| sides.iter().all(|side| !side.is_disjoint(beside)))
            .map(|(edge, _)| *edge)
            .collect()
    }

    /// The vertices where faces matching every name of `faces` meet.
    fn meeting(&self, faces: &BTreeSet<TopoName>) -> Vec<&'a str> {
        let Some(topology) = self.topology() else {
            return Vec::new();
        };
        let sides: Vec<BTreeSet<&'a str>> = faces.iter().map(|face| self.matched(face)).collect();
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

/// `name` with every `index` at any depth left out.
fn stripped(name: &TopoName) -> TopoName {
    match name {
        TopoName::Generated {
            kind,
            feature,
            role,
            generator,
            index: _,
        } => TopoName::Generated {
            kind: *kind,
            feature: feature.clone(),
            role: role.clone(),
            generator: generator.as_ref().map(|generator| match generator {
                Generator::Entity(inner) => Generator::Entity(Box::new(stripped(inner))),
                other => other.clone(),
            }),
            index: None,
        },
        TopoName::Instance {
            feature,
            instance,
            of,
        } => TopoName::Instance {
            feature: feature.clone(),
            instance: instance.clone(),
            of: Box::new(stripped(of)),
        },
        TopoName::Blend {
            feature,
            between,
            index: _,
        } => TopoName::Blend {
            feature: feature.clone(),
            between: between.iter().map(stripped).collect(),
            index: None,
        },
        TopoName::Bounded {
            faces,
            ends,
            index: _,
        } => TopoName::Bounded {
            faces: faces.iter().map(stripped).collect(),
            ends: ends.iter().map(stripped).collect(),
            index: None,
        },
        TopoName::Meeting { faces, index: _ } => TopoName::Meeting {
            faces: faces.iter().map(stripped).collect(),
            index: None,
        },
        TopoName::Modified { feature, from } => TopoName::Modified {
            feature: feature.clone(),
            from: Box::new(stripped(from)),
        },
        TopoName::Split {
            feature,
            from,
            piece,
        } => TopoName::Split {
            feature: feature.clone(),
            from: Box::new(stripped(from)),
            piece: *piece,
        },
        TopoName::Merged { feature, from } => TopoName::Merged {
            feature: feature.clone(),
            from: from.iter().map(stripped).collect(),
        },
        TopoName::Root { container } => TopoName::Root {
            container: container.clone(),
        },
    }
}

/// Every place in `name` where one or more indexes are stated, mapped to the
/// set of indexes stated there. Names of one set equal without their indexes
/// share one place.
fn index_places(name: &TopoName) -> BTreeMap<Place, BTreeSet<u32>> {
    fn collect(name: &TopoName, path: &mut Place, into: &mut BTreeMap<Place, BTreeSet<u32>>) {
        if let Some(index) = own_index(name) {
            into.entry(path.clone()).or_default().insert(index);
        }
        match name {
            TopoName::Generated {
                generator: Some(Generator::Entity(inner)),
                ..
            } => {
                path.push(Step::Generator);
                collect(inner, path, into);
                path.pop();
            }
            TopoName::Instance { of: inner, .. } => {
                path.push(Step::Of);
                collect(inner, path, into);
                path.pop();
            }
            TopoName::Modified { from: inner, .. } | TopoName::Split { from: inner, .. } => {
                path.push(Step::From);
                collect(inner, path, into);
                path.pop();
            }
            TopoName::Blend { between, .. } => {
                for (position, inner) in between.iter().enumerate() {
                    path.push(Step::Between(position));
                    collect(inner, path, into);
                    path.pop();
                }
            }
            TopoName::Bounded { faces, ends, .. } => {
                for face in faces {
                    path.push(Step::Face(stripped(face)));
                    collect(face, path, into);
                    path.pop();
                }
                for end in ends {
                    path.push(Step::End(stripped(end)));
                    collect(end, path, into);
                    path.pop();
                }
            }
            TopoName::Meeting { faces, .. } => {
                for face in faces {
                    path.push(Step::Face(stripped(face)));
                    collect(face, path, into);
                    path.pop();
                }
            }
            TopoName::Merged { from, .. } => {
                for inner in from {
                    path.push(Step::Merged(stripped(inner)));
                    collect(inner, path, into);
                    path.pop();
                }
            }
            TopoName::Generated { .. } | TopoName::Root { .. } => {}
        }
    }
    let mut found = BTreeMap::new();
    collect(name, &mut Vec::new(), &mut found);
    found
}

/// The matchers of the stored states one validation reads, each built once.
pub(in crate::validate) struct Matchers<'a> {
    states: &'a States<'a>,
    built: std::cell::RefCell<BTreeMap<StateKey<'a>, Option<Rc<Matcher<'a>>>>>,
}

impl<'a> Matchers<'a> {
    pub(in crate::validate) fn new(states: &'a States<'a>) -> Self {
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
