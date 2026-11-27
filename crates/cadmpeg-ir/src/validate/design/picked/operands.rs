// SPDX-License-Identifier: Apache-2.0
//! The rules of §6.5 and §6.6 that read what an operation's operands
//! resolve to in a stored state: a split's piece picks lie in its targets or
//! elements, a constrained mate's first connector picks a moved body, a
//! knit's federation seeds are its elements, and a break corner's edges are
//! no shorter than the sheet is thick.

use std::collections::{BTreeMap, BTreeSet};

use crate::design::ops::body::{Kept, SplitMode, TransformBodies, TransformStep};
use crate::design::ops::shared::{JointConnector, JointKind};
use crate::design::ops::surface::Knit;
use crate::design::selection::{
    Anchor, MemberOrder, OrderedSelection, Picked, Picks, Selection, SetSelection,
};

use super::super::constructions::input_selections;
use super::super::matching::{Matcher, StateTopology};
use super::super::sheets;
use super::apart::straight_length;
use super::{all_entities, entities, resolved_entities, Entity, Probe};

/// The bodies and faces an operand selection resolves to in a state.
struct Operands<'a> {
    bodies: BTreeSet<&'a str>,
    faces: BTreeSet<&'a str>,
}

impl<'a> Operands<'a> {
    /// The operands, where every pick resolves to bodies and faces.
    fn of<O: MemberOrder>(
        matcher: &Matcher<'a>,
        topology: &StateTopology<'a>,
        selection: &'a Selection<O>,
    ) -> Option<Self> {
        let mut operands = Self {
            bodies: BTreeSet::new(),
            faces: BTreeSet::new(),
        };
        for entity in all_entities(matcher, topology, selection)? {
            match entity {
                Entity::Body(body) => operands.bodies.insert(body),
                Entity::Face(face) => operands.faces.insert(face),
                Entity::Edge(_) | Entity::Vertex(_) | Entity::Other => return None,
            };
        }
        Some(operands)
    }

    /// Whether a face of the state is an operand face or a face of an
    /// operand body.
    fn holds_face(&self, topology: &StateTopology<'a>, face: &str) -> bool {
        self.faces.contains(face)
            || topology
                .face_body(face)
                .is_some_and(|body| self.bodies.contains(body.id.as_str()))
    }

    /// `entity` where it is no entity of the operands: a face neither picked
    /// nor of a picked body, a body neither picked nor holding a picked face,
    /// or an edge none of whose faces is an operand's.
    fn outside(&self, topology: &StateTopology<'a>, entity: Entity<'a>) -> Option<&'a str> {
        match entity {
            Entity::Face(face) => (topology.face_body(face).is_some()
                && !self.holds_face(topology, face))
            .then_some(face),
            Entity::Body(body) => {
                let holds = self.bodies.contains(body)
                    || self.faces.iter().any(|face| {
                        topology
                            .face_body(face)
                            .is_some_and(|owner| owner.id.as_str() == body)
                    });
                (!holds).then_some(body)
            }
            Entity::Edge(edge) => {
                let faces = topology.edge_faces.get(edge)?;
                let apart = faces.iter().all(|face| {
                    topology.face_body(face).is_some() && !self.holds_face(topology, face)
                });
                (!faces.is_empty() && apart).then_some(edge)
            }
            Entity::Vertex(_) | Entity::Other => None,
        }
    }
}

/// The bodies that hold `entity` in the state. `None` where the state does
/// not say.
fn holding_bodies<'a>(
    topology: &StateTopology<'a>,
    entity: Entity<'a>,
) -> Option<BTreeSet<&'a str>> {
    let bodies: BTreeSet<&'a str> = match entity {
        Entity::Body(body) => BTreeSet::from([body]),
        Entity::Face(face) => BTreeSet::from([topology.face_body(face)?.id.as_str()]),
        Entity::Edge(edge) => topology.edge_bodies(edge),
        Entity::Vertex(vertex) => topology
            .vertex_faces
            .get(vertex)?
            .iter()
            .map(|face| Some(topology.face_body(face)?.id.as_str()))
            .collect::<Option<_>>()?,
        Entity::Other => return None,
    };
    (!bodies.is_empty()).then_some(bodies)
}

/// The selections a joint connector picks through.
fn connector_selections(connector: &JointConnector) -> Vec<&OrderedSelection> {
    match connector {
        JointConnector::Frame(frame) => input_selections(&frame.inputs),
        JointConnector::Plane(plane) => input_selections(&plane.inputs),
        JointConnector::Axis(axis) => input_selections(&axis.inputs),
        JointConnector::Point(point) => input_selections(&point.inputs),
        JointConnector::Entity(selection) => vec![selection],
    }
}

/// The seed picks of a selection: its named picks or its rule terms' seeds.
fn seeds<O: MemberOrder>(selection: &Selection<O>) -> Vec<&Picked> {
    match &selection.picks {
        Picks::Named(members) => members.as_slice().iter().collect(),
        Picks::Rule { terms, .. } => terms
            .iter()
            .flat_map(|term| term.seeds.as_slice())
            .collect(),
        Picks::Unread => Vec::new(),
    }
}

impl<'a> Probe<'_, 'a> {
    /// A split's piece picks target entities of its targets or elements
    /// (§6.5).
    pub(super) fn split_pieces(&mut self, mode: &'a SplitMode) {
        match mode {
            SplitMode::ByTools {
                targets: Some(targets),
                kept: Some(Kept::Pieces { keep, remove }),
                ..
            } => self.pieces(targets, [keep, remove]),
            SplitMode::Mutual {
                elements,
                kept: Some(Kept::Pieces { keep, remove }),
            } => self.pieces(elements, [keep, remove]),
            SplitMode::ByTools { .. } | SplitMode::Mutual { .. } => {}
        }
    }

    fn pieces<O: MemberOrder>(
        &mut self,
        operands: &'a Selection<O>,
        lists: [&'a Option<Box<SetSelection>>; 2],
    ) {
        let mut outside = BTreeSet::new();
        for list in lists.into_iter().flatten() {
            if list.at != operands.at {
                continue;
            }
            for state in self.states(&operands.at) {
                let Some(topology) = state.matcher.topology() else {
                    continue;
                };
                let Some(found) = Operands::of(&state.matcher, topology, operands) else {
                    continue;
                };
                for entity in resolved_entities(&state.matcher, topology, list) {
                    outside.extend(found.outside(topology, entity));
                }
            }
        }
        for entity in outside {
            self.error(crate::rule!(
                "design.piece-pick-outside-split-operands",
                "a Kept::Pieces piece pick resolves to {entity}, which is no entity of the split's targets or elements"
            ));
        }
    }

    /// A `Constrained` mate's first connector picks a moved body (§6.5).
    pub(super) fn mates(&mut self, transform: &'a TransformBodies) {
        let mut apart = BTreeSet::new();
        let moved_at = &transform.bodies.at;
        for (step_at, step) in transform.transformation.steps.iter().enumerate() {
            let TransformStep::Constrained { mates } = step else {
                continue;
            };
            for (mate_at, mate) in mates.iter().enumerate() {
                if !matches!(
                    mate.kind,
                    JointKind::Coincident { .. }
                        | JointKind::Parallel
                        | JointKind::Perpendicular
                        | JointKind::Tangent { .. }
                        | JointKind::Concentric
                        | JointKind::Distance { .. }
                        | JointKind::Angle { .. }
                ) {
                    continue;
                }
                let [first, _] = &mate.connectors;
                let selections = connector_selections(first);
                if selections.is_empty() || selections.iter().any(|s| s.at != *moved_at) {
                    continue;
                }
                for state in self.states(moved_at) {
                    let Some(topology) = state.matcher.topology() else {
                        continue;
                    };
                    let moved: Option<BTreeSet<&str>> =
                        all_entities(&state.matcher, topology, &transform.bodies).and_then(
                            |found| {
                                found
                                    .into_iter()
                                    .map(|entity| match entity {
                                        Entity::Body(body) => Some(body),
                                        _ => None,
                                    })
                                    .collect()
                            },
                        );
                    let Some(moved) = moved else {
                        continue;
                    };
                    let held =
                        selections
                            .iter()
                            .try_fold(BTreeSet::new(), |mut held, selection| {
                                for entity in all_entities(&state.matcher, topology, *selection)? {
                                    held.extend(holding_bodies(topology, entity)?);
                                }
                                Some(held)
                            });
                    if held.is_some_and(|held| held.is_disjoint(&moved)) {
                        apart.insert((step_at, mate_at));
                    }
                }
            }
        }
        for (step, mate) in apart {
            self.error(crate::rule!(
                "design.constrained-mate-connector-off-moved-body",
                "the first connector of mate {mate} of transformation step {step} picks no moved body"
            ));
        }
    }

    /// `Knit.federation`'s seeds pick elements of the knit's result (§6.6),
    /// read where the federation anchors with the elements: in that state
    /// the result's elements are the faces of the knit's elements.
    pub(super) fn federation(&mut self, knit: &'a Knit) {
        let Some(federation) = &knit.federation else {
            return;
        };
        if federation.at != knit.elements.at {
            return;
        }
        let picked = seeds(federation);
        let mut outside = BTreeSet::new();
        for state in self.states(&knit.elements.at) {
            let Some(topology) = state.matcher.topology() else {
                continue;
            };
            let Some(elements) = Operands::of(&state.matcher, topology, &knit.elements) else {
                continue;
            };
            for pick in &picked {
                for entity in entities(&state.matcher, topology, pick)
                    .into_iter()
                    .flatten()
                {
                    outside.extend(elements.outside(topology, entity));
                }
            }
        }
        for entity in outside {
            self.error(crate::rule!(
                "design.federation-seed-outside-knit-elements",
                "a Knit.federation seed resolves to {entity}, which is no element of the knit"
            ));
        }
    }

    /// A `BreakCorner.corners` picks edges of the sheet's thickness (§6.6):
    /// a straight edge shorter than every sheet's thickness entering the
    /// feature is none.
    pub(super) fn break_corner(&mut self, corners: &'a SetSelection) {
        if corners.at != Anchor::Input {
            return;
        }
        let mut short = BTreeMap::new();
        for state in self.states(&corners.at) {
            let Some(thinnest) = sheets::entering(
                self.index,
                self.feature.id.as_str(),
                sheets::Reading::of(state.configuration),
            )
            .and_then(|entering| entering.thicknesses)
            .and_then(|thicknesses| thicknesses.into_iter().reduce(f64::min)) else {
                continue;
            };
            let Some(topology) = state.matcher.topology() else {
                continue;
            };
            for entity in resolved_entities(&state.matcher, topology, corners) {
                let Entity::Edge(edge) = entity else {
                    continue;
                };
                if let Some(length) = straight_length(topology, edge) {
                    if length < thinnest - self.linear {
                        short.insert(edge, (length, thinnest));
                    }
                }
            }
        }
        for (edge, (length, thinnest)) in short {
            self.error(crate::rule!(
                "design.break-corner-edge-shorter-than-sheet",
                "BreakCorner.corners picks edge {edge} of length {length}, shorter than the sheet's thickness {thinnest}"
            ));
        }
    }
}
