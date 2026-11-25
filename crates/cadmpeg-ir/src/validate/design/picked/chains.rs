// SPDX-License-Identifier: Apache-2.0
//! The rules of §6 that read the chain a selection's edges form in a stored
//! state: a path is one connected chain, a patch loop is closed, a contour
//! flange's profile is open, a wall's side fits whether its profile is open,
//! a chain's start is an end of it, and a vertex bend sits where two pieces
//! meet.
//!
//! A chain is read where every pick of the selection resolves to edges of
//! the state. Two ends are one node where they are one vertex or lie within
//! the linear tolerance of each other.

use std::collections::{BTreeMap, BTreeSet};

use crate::design::feature::FeatureOperation;
use crate::design::ops::dressup::{DraftAngle, FilletSet, FilletSize, Stations};
use crate::design::ops::sheet_metal::{SheetMetalKind, VertexBend};
use crate::design::ops::surface::{PatchBounds, PatchEdge};
use crate::design::ops::swept::{Spine, SweepSection};
use crate::design::options::{BodyForm, CornerRound, SolidOutput, Thickness, WallSide};
use crate::design::selection::{MemberOrder, Selection};
use crate::geometry::SolvedCurveGeometry;
use crate::math::Point3;
use crate::topology::EdgeCarrier;

use super::super::matching::StateTopology;
use super::{all_entities, Entity, InState, Probe};

/// How the edges of a selection join.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Shape {
    /// One chain with two ends.
    Open,
    /// One chain with no end.
    Closed,
    /// One piece in which three or more edges meet at a node.
    Branched,
    /// More than one piece.
    Disconnected,
}

/// The chain the edges of a selection form in one state.
struct Chain<'a> {
    shape: Shape,
    /// The node of each end vertex.
    node: BTreeMap<&'a str, usize>,
    /// The number of edge ends at each node.
    degree: Vec<usize>,
}

impl Chain<'_> {
    /// The number of edge ends at the node of `vertex`. `None` where the
    /// vertex is no end of the chain's edges.
    fn degree_at(&self, vertex: &str) -> Option<usize> {
        self.node.get(vertex).map(|&node| self.degree[node])
    }
}

/// The edges every pick of `selection` resolves to in the state. `None`
/// where a pick does not resolve or resolves to something other than edges.
fn edges<'a, O: MemberOrder>(
    state: &InState<'a>,
    topology: &StateTopology<'a>,
    selection: &'a Selection<O>,
) -> Option<BTreeSet<&'a str>> {
    all_entities(&state.matcher, topology, selection)?
        .into_iter()
        .map(|entity| match entity {
            Entity::Edge(edge) => Some(edge),
            _ => None,
        })
        .collect()
}

/// The chain `edges` form. `None` where an edge or an end's position is not
/// held, or an edge with no ends is one of several.
fn chain<'a>(
    topology: &StateTopology<'a>,
    edges: &BTreeSet<&'a str>,
    tolerance: f64,
) -> Option<Chain<'a>> {
    let mut ends: Vec<(&'a str, &'a str)> = Vec::new();
    for &edge in edges {
        let record = topology.edges.get(edge)?;
        match &record.ends {
            Some(pair) => ends.push((pair.start.as_str(), pair.end.as_str())),
            None if edges.len() == 1 => {
                return Some(Chain {
                    shape: Shape::Closed,
                    node: BTreeMap::new(),
                    degree: Vec::new(),
                });
            }
            None => return None,
        }
    }
    let mut vertices: Vec<(&'a str, Point3)> = Vec::new();
    for vertex in ends.iter().flat_map(|&(start, end)| [start, end]) {
        if vertices.iter().all(|(held, _)| *held != vertex) {
            let point = topology.vertices.get(vertex)?.point.as_str();
            vertices.push((vertex, topology.points.get(point)?.position().get()));
        }
    }
    let mut parent: Vec<usize> = (0..vertices.len()).collect();
    for first in 0..vertices.len() {
        for second in first + 1..vertices.len() {
            if vertices[first].1.vector_from(vertices[second].1).norm() <= tolerance {
                join(&mut parent, first, second);
            }
        }
    }
    let slot = |vertex: &str| vertices.iter().position(|(held, _)| *held == vertex);
    let mut pieces = parent.clone();
    let mut degree = vec![0; vertices.len()];
    for &(start, end) in &ends {
        let (start, end) = (slot(start)?, slot(end)?);
        let (start, end) = (root(&mut parent, start), root(&mut parent, end));
        degree[start] += 1;
        degree[end] += 1;
        join(&mut pieces, start, end);
    }
    let nodes: BTreeSet<usize> = (0..vertices.len())
        .map(|at| root(&mut parent, at))
        .collect();
    let components: BTreeSet<usize> = nodes.iter().map(|&at| root(&mut pieces, at)).collect();
    let shape = if components.len() > 1 {
        Shape::Disconnected
    } else if nodes.iter().any(|&at| degree[at] > 2) {
        Shape::Branched
    } else if nodes.iter().all(|&at| degree[at] == 2) {
        Shape::Closed
    } else {
        Shape::Open
    };
    let node = vertices
        .iter()
        .enumerate()
        .map(|(at, (vertex, _))| (*vertex, root(&mut parent, at)))
        .collect();
    Some(Chain {
        shape,
        node,
        degree,
    })
}

fn root(parent: &mut [usize], mut at: usize) -> usize {
    while parent[at] != at {
        parent[at] = parent[parent[at]];
        at = parent[at];
    }
    at
}

fn join(parent: &mut [usize], first: usize, second: usize) {
    let (first, second) = (root(parent, first), root(parent, second));
    parent[first] = second;
}

/// The one vertex `selection` resolves to in the state.
fn one_vertex<'a, O: MemberOrder>(
    state: &InState<'a>,
    topology: &StateTopology<'a>,
    selection: &'a Selection<O>,
) -> Option<&'a str> {
    match all_entities(&state.matcher, topology, selection)?.as_slice() {
        [Entity::Vertex(vertex)] => Some(vertex),
        _ => None,
    }
}

/// Whether an edge lies on a line. `None` where its curve is not an analytic
/// line or conic.
fn on_line(topology: &StateTopology<'_>, edge: &str) -> Option<bool> {
    let (EdgeCarrier::Curve(curve) | EdgeCarrier::Bounded(curve, _)) =
        &topology.edges.get(edge)?.carrier
    else {
        return None;
    };
    match topology.curves.get(curve.as_str())?.geometry.solved()? {
        SolvedCurveGeometry::Line(_) => Some(true),
        SolvedCurveGeometry::Circle(_)
        | SolvedCurveGeometry::Ellipse(_)
        | SolvedCurveGeometry::Parabola(_)
        | SolvedCurveGeometry::Hyperbola(_) => Some(false),
        _ => None,
    }
}

impl<'a> Probe<'_, 'a> {
    /// The chains a selection's edges form in each stored state it anchors
    /// in, with the state.
    fn chains<O: MemberOrder>(&self, selection: &'a Selection<O>) -> Vec<(InState<'a>, Chain<'a>)> {
        let mut found = Vec::new();
        for state in self.states(&selection.at) {
            let Some(topology) = state.matcher.topology() else {
                continue;
            };
            let Some(chain) = edges(&state, topology, selection)
                .and_then(|edges| chain(topology, &edges, self.linear))
            else {
                continue;
            };
            found.push((state, chain));
        }
        found
    }

    /// The shapes a selection's edges form across its stored states.
    fn shapes<O: MemberOrder>(&self, selection: &'a Selection<O>) -> BTreeSet<Shape> {
        self.chains(selection)
            .into_iter()
            .map(|(_, chain)| chain.shape)
            .collect()
    }

    pub(super) fn chain_operation(&mut self, operation: &'a FeatureOperation) {
        match operation {
            FeatureOperation::Extrude(extrude) => self.wall(&extrude.profile, &extrude.output),
            FeatureOperation::Revolve(revolve) => self.wall(&revolve.profile, &revolve.output),
            FeatureOperation::Sweep(sweep) => {
                if let SweepSection::Profile(profile) = &sweep.section {
                    self.wall(profile, &sweep.output);
                }
                self.path(&sweep.path, "Sweep.path");
                for guide in &sweep.guides {
                    self.path(&guide.path, "SweepGuide.path");
                }
            }
            FeatureOperation::Loft(loft) => {
                for guide in &loft.guides {
                    self.path(&guide.path, "Guide.path");
                }
                match &loft.spine {
                    Some(Spine::Path { path, .. }) => self.path(path, "Spine::Path.path"),
                    Some(Spine::Centerline { path, .. }) => {
                        self.path(path, "Spine::Centerline.path");
                    }
                    _ => {}
                }
            }
            FeatureOperation::SheetMetal(sheet) => {
                if let SheetMetalKind::ContourFlange {
                    profile,
                    vertex_bends,
                    ..
                } = &sheet.kind
                {
                    self.contour(profile, vertex_bends);
                }
            }
            FeatureOperation::Patch(patch) => {
                if let PatchBounds::Loops { outer, inner } = &patch.bounds {
                    for edges in std::iter::once(outer).chain(inner) {
                        self.patch_loop(edges);
                    }
                }
            }
            FeatureOperation::Fillet(fillet) => {
                for set in &fillet.sets {
                    if let FilletSet::Edges { edges, section, .. } = set {
                        if let Some(FilletSize::Variable(stations)) = &section.size {
                            self.chain_start(edges, stations);
                        }
                    }
                }
            }
            FeatureOperation::Draft(draft) => {
                for set in &draft.sets {
                    if let (Some(fixed), Some(DraftAngle::Variable(stations))) =
                        (&set.fixed, &set.angle)
                    {
                        self.chain_start(fixed, stations);
                    }
                }
            }
            _ => {}
        }
    }

    /// A path's resolved edges form one connected chain (§6).
    fn path<O: MemberOrder>(&mut self, path: &'a Selection<O>, field: &str) {
        let shapes = self.shapes(path);
        if shapes.contains(&Shape::Branched) || shapes.contains(&Shape::Disconnected) {
            self.error(crate::rule!(
                "design.path-picks-not-one-chain",
                "the edges {field} picks do not form one connected chain"
            ));
        }
    }

    /// A thin wall's side and corners fit whether its profile's edges form
    /// an open or a closed chain, and a round corner meets only lines (§6).
    fn wall<O: MemberOrder>(&mut self, profile: &'a Selection<O>, output: &'a SolidOutput) {
        let Some(BodyForm::Thin(wall)) = &output.body else {
            return;
        };
        let shapes = self.shapes(profile);
        let closed = shapes.contains(&Shape::Closed);
        let open = shapes.contains(&Shape::Open);
        if let Thickness::OneSide {
            side: Some(side), ..
        } = &wall.thickness
        {
            let wrong = match side {
                WallSide::Left | WallSide::Right => closed,
                WallSide::Inside | WallSide::Outside => open,
                WallSide::Native(_) => false,
            };
            if wrong {
                let profile = if closed { "a closed" } else { "an open" };
                self.error(crate::rule!(
                    "design.wall-side-profile-closure",
                    "ThinWall's side {side:?} occurs with {profile} profile chain"
                ));
            }
        }
        if wall.corner_round.is_some() && closed {
            self.error(crate::rule!(
                "design.corner-round-closed-profile",
                "ThinWall.corner_round occurs with a closed profile chain"
            ));
        }
        if let Some(CornerRound::Round(_)) = &wall.corner_round {
            self.round_corners(profile);
        }
    }

    /// `CornerRound::Round` occurs only with a profile whose picked curves
    /// are all line segments (§6).
    fn round_corners<O: MemberOrder>(&mut self, profile: &'a Selection<O>) {
        let mut curved = BTreeSet::new();
        for state in self.states(&profile.at) {
            let Some(topology) = state.matcher.topology() else {
                continue;
            };
            for edge in edges(&state, topology, profile).into_iter().flatten() {
                if on_line(topology, edge) == Some(false) {
                    curved.insert(edge);
                }
            }
        }
        for edge in curved {
            self.error(crate::rule!(
                "design.round-corner-profile-not-lines",
                "CornerRound::Round occurs with a profile that picks edge {edge}, which is no line segment"
            ));
        }
    }

    /// Each patch loop's curves form one closed chain (§6.5).
    fn patch_loop(&mut self, edges: &'a [PatchEdge]) {
        let [first, ..] = edges else {
            return;
        };
        if edges.iter().any(|edge| edge.curve.at != first.curve.at) {
            return;
        }
        let mut shapes = BTreeSet::new();
        for state in self.states(&first.curve.at) {
            let Some(topology) = state.matcher.topology() else {
                continue;
            };
            let found: Option<BTreeSet<&str>> = edges
                .iter()
                .map(|edge| self::edges(&state, topology, &edge.curve))
                .try_fold(BTreeSet::new(), |mut all, some| {
                    all.extend(some?);
                    Some(all)
                });
            if let Some(chain) = found.and_then(|found| chain(topology, &found, self.linear)) {
                shapes.insert(chain.shape);
            }
        }
        if shapes.iter().any(|shape| *shape != Shape::Closed) {
            self.error(crate::rule!(
                "design.patch-loop-not-closed-chain",
                "a PatchBounds::Loops loop's curves do not form one closed chain"
            ));
        }
    }

    /// A `ContourFlange.profile`'s picks form one open chain, and each
    /// `VertexBend.vertex` picks a vertex of it where two pieces meet (§6.6).
    fn contour<O: MemberOrder>(&mut self, profile: &'a Selection<O>, bends: &'a [VertexBend]) {
        let chains = self.chains(profile);
        if chains.iter().any(|(_, chain)| chain.shape != Shape::Open) {
            self.error(crate::rule!(
                "design.contour-flange-profile-not-open-chain",
                "ContourFlange.profile's picks do not form one open chain"
            ));
        }
        let mut apart = BTreeSet::new();
        let vertices = bends.iter().map(|bend| &bend.vertex);
        for vertex in vertices.filter(|vertex| vertex.at == profile.at) {
            for (state, chain) in &chains {
                if !matches!(chain.shape, Shape::Open | Shape::Closed) || chain.node.is_empty() {
                    continue;
                }
                let Some(topology) = state.matcher.topology() else {
                    continue;
                };
                let Some(id) = one_vertex(state, topology, vertex) else {
                    continue;
                };
                if chain.degree_at(id) != Some(2) {
                    apart.insert(id);
                }
            }
        }
        for vertex in apart {
            self.error(crate::rule!(
                "design.vertex-bend-not-chain-joint",
                "VertexBend.vertex picks {vertex}, which is no vertex where two pieces of the profile chain meet"
            ));
        }
    }

    /// A `chain_start` resolves to an end vertex of the chain (§6.4).
    fn chain_start<O: MemberOrder, Q>(
        &mut self,
        edges: &'a Selection<O>,
        stations: &'a Stations<Q>,
    ) {
        let Some(start) = &stations.chain_start else {
            return;
        };
        if start.at != edges.at {
            return;
        }
        let mut off = BTreeSet::new();
        for (state, chain) in self.chains(edges) {
            let Some(topology) = state.matcher.topology() else {
                continue;
            };
            if chain.node.is_empty() {
                continue;
            }
            let Some(vertex) = one_vertex(&state, topology, start) else {
                continue;
            };
            let end = match (chain.shape, chain.degree_at(vertex)) {
                (Shape::Open, degree) => degree == Some(1),
                (Shape::Closed, degree) => degree.is_some(),
                (Shape::Branched | Shape::Disconnected, _) => true,
            };
            if !end {
                off.insert(vertex);
            }
        }
        for vertex in off {
            self.error(crate::rule!(
                "design.chain-start-not-chain-end",
                "Stations.chain_start picks {vertex}, which is no end vertex of the chain"
            ));
        }
    }
}
