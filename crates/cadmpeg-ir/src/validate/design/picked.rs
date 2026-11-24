// SPDX-License-Identifier: Apache-2.0
//! The rules of §6.4 to §6.6 that read what a feature's picks resolve to in
//! their stored anchored states: a face is planar, cylindrical, or a bend
//! face; a body is solid or a sheet; an entity belongs to a picked body; an
//! edge meets a corner's vertex; edges share one vertex or divide a sheet; a
//! setback fits its edge; a point lies on a face.
//!
//! A pick is read in each stored state its anchor names, where the feature's
//! definition in that state's configuration is the one that holds it, and
//! only where it resolves without a context: by its `resolved` entities, else
//! by the entities its name matches. A surface or curve the document does not
//! state analytically is not judged.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use crate::design::construction::{kinds, Construction};
use crate::design::dimension::Dimension;
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation};
use crate::design::ops::dressup::{BlendControls, ChamferSize, EdgeSetback, Setbacks};
use crate::design::ops::sheet_metal::{
    BendSet, Convert, ConvertBend, ConvertFrom, FlangeWidth, SheetMetalKind, StampForm,
    StampOutline,
};
use crate::design::refs::EntityRef;
use crate::design::selection::{Anchor, MemberOrder, PickTarget, Picked, Picks, Selection};
use crate::features::ConfigurationId;
use crate::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use crate::ids::VertexId;
use crate::math::Point3;
use crate::report::check::{Finding, Ruled};
use crate::scalar::NonNegativeLength;
use crate::topology::{BodyKind, EdgeCarrier};

use super::index::DesignIndex;
use super::matching::{Matcher, Matchers, StateTopology};
use super::selections::{anchored_states, Site};
use super::Holder;

/// Checks the picks of every feature definition in the stored states they
/// anchor in.
pub(super) fn check<'a>(
    index: &DesignIndex<'a>,
    matchers: &Matchers<'a>,
    findings: &mut Vec<Finding>,
) {
    let ir = index.records.ir;
    for feature in &ir.design.features {
        let overrides = ir.design.configurations.iter().filter_map(|configuration| {
            configuration
                .overrides
                .features
                .get(&feature.id)?
                .definition
                .as_ref()
        });
        let mut probe = Probe {
            index,
            matchers,
            feature,
            definition: &feature.definition,
            linear: ir.tolerances.linear.get(),
            findings: Vec::new(),
        };
        for definition in std::iter::once(&feature.definition).chain(overrides) {
            probe.definition = definition;
            probe.operation(&definition.operation);
        }
        for finding in probe.findings {
            if !findings.contains(&finding) {
                findings.push(finding);
            }
        }
    }
}

/// An entity a pick resolves to in a state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entity<'a> {
    Body(&'a str),
    Face(&'a str),
    Edge(&'a str),
    Vertex(&'a str),
    Other,
}

impl<'a> Entity<'a> {
    fn of_ref(entity: &'a EntityRef) -> Self {
        match entity {
            EntityRef::Body(id) => Self::Body(id.as_str()),
            EntityRef::Face(id) => Self::Face(id.as_str()),
            EntityRef::Edge(id) => Self::Edge(id.as_str()),
            EntityRef::Vertex(id) => Self::Vertex(id.as_str()),
            EntityRef::Surface(_) | EntityRef::Curve(_) | EntityRef::Subd(_) => Self::Other,
        }
    }

    /// Whether the state holds the entity; every entity other than a body,
    /// face, edge, or vertex is taken as held.
    fn held(self, topology: &StateTopology<'_>) -> bool {
        match self {
            Self::Body(id) => topology.bodies.contains_key(id),
            Self::Face(id) => topology.faces.contains_key(id),
            Self::Edge(id) => topology.edges.contains_key(id),
            Self::Vertex(id) => topology.vertices.contains_key(id),
            Self::Other => true,
        }
    }

    /// The entity of the state with identity `id`.
    fn of_id(topology: &StateTopology<'a>, id: &'a str) -> Self {
        if topology.bodies.contains_key(id) {
            Self::Body(id)
        } else if topology.faces.contains_key(id) {
            Self::Face(id)
        } else if topology.edges.contains_key(id) {
            Self::Edge(id)
        } else if topology.vertices.contains_key(id) {
            Self::Vertex(id)
        } else {
            Self::Other
        }
    }
}

/// One stored state a selection anchors in.
struct InState<'a> {
    matcher: Rc<Matcher<'a>>,
    configuration: Option<&'a str>,
}

/// The picks of a selection, where it lists them.
fn picks<O: MemberOrder>(selection: &Selection<O>) -> &[Picked] {
    match &selection.picks {
        Picks::Named(members) => members.as_slice(),
        Picks::Unread | Picks::Rule { .. } => &[],
    }
}

/// The entities `pick` resolves to in the state, where it resolves there
/// without a context: its `resolved` entities that the state holds, else the
/// entities its name matches. `None` where neither gives one.
fn entities<'a>(
    matcher: &Matcher<'a>,
    topology: &StateTopology<'a>,
    pick: &'a Picked,
) -> Option<Vec<Entity<'a>>> {
    if pick.context.is_some() {
        return None;
    }
    let found: Vec<Entity<'a>> = match (&pick.resolved, &pick.target) {
        (Some(resolved), _) => resolved
            .iter()
            .map(Entity::of_ref)
            .filter(|entity| entity.held(topology))
            .collect(),
        (None, PickTarget::Entity(name)) => matcher
            .matched(name)
            .into_iter()
            .map(|id| Entity::of_id(topology, id))
            .collect(),
        (None, _) => return None,
    };
    (!found.is_empty()).then_some(found)
}

/// The entities of every pick of `selection` that resolves in the state.
fn resolved_entities<'a, O: MemberOrder>(
    matcher: &Matcher<'a>,
    topology: &StateTopology<'a>,
    selection: &'a Selection<O>,
) -> Vec<Entity<'a>> {
    picks(selection)
        .iter()
        .filter_map(|pick| entities(matcher, topology, pick))
        .flatten()
        .collect()
}

/// The entities of `selection` where every pick resolves in the state.
fn all_entities<'a, O: MemberOrder>(
    matcher: &Matcher<'a>,
    topology: &StateTopology<'a>,
    selection: &'a Selection<O>,
) -> Option<Vec<Entity<'a>>> {
    let picks = picks(selection);
    if picks.is_empty() {
        return None;
    }
    let mut found = Vec::new();
    for pick in picks {
        found.extend(entities(matcher, topology, pick)?);
    }
    Some(found)
}

/// The analytic shape of a face's surface, through every placement of an
/// inline basis, and whether a placement was passed.
fn surface_shape<'a>(
    topology: &StateTopology<'a>,
    face: &str,
) -> Option<(&'a SolvedSurfaceGeometry, bool)> {
    let face = topology.faces.get(face)?;
    let mut geometry = topology
        .surfaces
        .get(face.surface.as_str())?
        .geometry
        .solved()?;
    let mut placed = false;
    while let SolvedSurfaceGeometry::Transformed(inner) = geometry {
        geometry = inner.basis();
        placed = true;
    }
    Some((geometry, placed))
}

/// Whether a face is planar. `None` where its surface is not analytic.
fn planar(topology: &StateTopology<'_>, face: &str) -> Option<bool> {
    match surface_shape(topology, face)?.0 {
        SolvedSurfaceGeometry::Plane(_) => Some(true),
        SolvedSurfaceGeometry::Cylinder(_)
        | SolvedSurfaceGeometry::Cone(_)
        | SolvedSurfaceGeometry::Sphere(_)
        | SolvedSurfaceGeometry::Torus(_) => Some(false),
        SolvedSurfaceGeometry::Nurbs(_)
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Transformed(_)
        | SolvedSurfaceGeometry::Unknown { .. } => None,
    }
}

/// Whether a face is cylindrical. `None` where its surface is not analytic,
/// is a cone, or places a cylinder by an affine map.
fn cylindrical(topology: &StateTopology<'_>, face: &str) -> Option<bool> {
    let (geometry, placed) = surface_shape(topology, face)?;
    match geometry {
        SolvedSurfaceGeometry::Cylinder(_) => (!placed).then_some(true),
        SolvedSurfaceGeometry::Plane(_)
        | SolvedSurfaceGeometry::Sphere(_)
        | SolvedSurfaceGeometry::Torus(_) => Some(false),
        SolvedSurfaceGeometry::Cone(_)
        | SolvedSurfaceGeometry::Nurbs(_)
        | SolvedSurfaceGeometry::Polygonal(_)
        | SolvedSurfaceGeometry::Transformed(_)
        | SolvedSurfaceGeometry::Unknown { .. } => None,
    }
}

/// The distance of `point` from the surface of `face`, where the surface is
/// an unplaced plane or cylinder of a body with no placement of its own.
fn surface_distance(topology: &StateTopology<'_>, face: &str, point: Point3) -> Option<f64> {
    if topology.face_body(face)?.transform.is_some() {
        return None;
    }
    let (geometry, false) = surface_shape(topology, face)? else {
        return None;
    };
    match geometry {
        SolvedSurfaceGeometry::Plane(plane) => {
            let normal = *plane.frame().axis().as_raw();
            Some(point.vector_from(plane.origin().get()).dot(normal).abs())
        }
        SolvedSurfaceGeometry::Cylinder(cylinder) => {
            let axis = *cylinder.frame().axis().as_raw();
            let offset = point.vector_from(cylinder.origin().get());
            Some((offset.cross(axis).norm() - cylinder.radius().get()).abs())
        }
        _ => None,
    }
}

/// The length of a straight edge: one on an unplaced line with two ends, of
/// bodies with no placement of their own.
fn straight_length(topology: &StateTopology<'_>, edge: &str) -> Option<f64> {
    let placed = topology.edge_bodies(edge).into_iter().any(|body| {
        topology
            .bodies
            .get(body)
            .is_some_and(|body| body.transform.is_some())
    });
    if placed {
        return None;
    }
    let edge = topology.edges.get(edge)?;
    let (EdgeCarrier::Curve(curve) | EdgeCarrier::Bounded(curve, _)) = &edge.carrier else {
        return None;
    };
    let curve = topology.curves.get(curve.as_str())?.geometry.solved()?;
    if !matches!(curve, SolvedCurveGeometry::Line(_)) {
        return None;
    }
    let position = |vertex: &str| -> Option<Point3> {
        let point = topology.vertices.get(vertex)?.point.as_str();
        Some(topology.points.get(point)?.position().get())
    };
    let ends = edge.ends.as_ref()?;
    let start = position(ends.start.as_str())?;
    Some(position(ends.end.as_str())?.vector_from(start).norm())
}

fn literal<T: Copy>(dimension: &Dimension<T>) -> Option<T> {
    match dimension {
        Dimension::Literal(value) => Some(*value),
        Dimension::Parameter(_) | Dimension::Negated(_) => None,
    }
}

/// The checks of one feature definition.
struct Probe<'p, 'a> {
    index: &'p DesignIndex<'a>,
    matchers: &'p Matchers<'a>,
    feature: &'a Feature,
    definition: &'a FeatureDefinition,
    linear: f64,
    findings: Vec<Finding>,
}

impl<'a> Probe<'_, 'a> {
    fn error(&mut self, message: Ruled<'_>) {
        super::error(&mut self.findings, self.feature.id.as_str(), message);
    }

    fn warning(&mut self, message: Ruled<'_>) {
        super::warning(&mut self.findings, self.feature.id.as_str(), message);
    }

    /// The stored states `anchor` names in which the feature's definition is
    /// the one being checked.
    fn states(&self, anchor: &'a Anchor) -> Vec<InState<'a>> {
        let records = &self.index.records;
        let site = Site {
            holder: Holder::Definition(self.feature),
            anchor,
            owner: self.index.owner_definition(self.feature.id.as_str()),
        };
        anchored_states(self.index, self.matchers.states(), site)
            .into_iter()
            .filter_map(|state| {
                let configuration = state.configuration(records);
                let applies = std::ptr::eq(
                    records.definition_in(self.feature, configuration),
                    self.definition,
                );
                applies.then_some(())?;
                Some(InState {
                    matcher: self.matchers.get(&state)?,
                    configuration,
                })
            })
            .collect()
    }

    /// The value of a point construction in a state's configuration: the
    /// active configuration's is `resolved`.
    fn point_in(
        &self,
        construction: &'a Construction<kinds::Point>,
        configuration: Option<&str>,
    ) -> Option<Point3> {
        let active = self
            .index
            .owner_definition(self.feature.id.as_str())
            .and_then(|owner| self.index.records.scopes.get(owner.as_str()))
            .and_then(|scope| scope.active_configuration.as_ref())
            .map(ConfigurationId::as_str);
        let value = match configuration {
            None => construction.resolved.as_ref(),
            Some(configuration) if Some(configuration) == active => construction.resolved.as_ref(),
            Some(configuration) => construction
                .resolved_in
                .iter()
                .find(|(id, _)| id.as_str() == configuration)
                .map(|(_, value)| value),
        };
        value.map(|point| point.get())
    }

    fn operation(&mut self, operation: &'a FeatureOperation) {
        match operation {
            FeatureOperation::SheetMetal(sheet) => self.sheet_metal(&sheet.kind),
            FeatureOperation::Graft(graft) => {
                self.body_kinds(&graft.body, "Graft.body", "a solid body", |kind| {
                    matches!(kind, BodyKind::Sheet | BodyKind::Wire)
                });
                self.body_kinds(&graft.sheet, "Graft.sheet", "a sheet body", |kind| {
                    matches!(kind, BodyKind::Solid | BodyKind::Wire)
                });
            }
            FeatureOperation::Unsew(unsew) => self.unsew(&unsew.edges),
            FeatureOperation::Fillet(fillet) => self.corners(&fillet.controls),
            FeatureOperation::Chamfer(chamfer) => {
                self.corners(&chamfer.controls);
                for set in &chamfer.sets {
                    if let Some(ChamferSize::Vertex(setbacks)) = &set.size {
                        self.vertex_chamfer(setbacks);
                    }
                }
            }
            _ => {}
        }
    }

    fn sheet_metal(&mut self, kind: &'a SheetMetalKind) {
        match kind {
            SheetMetalKind::Unfold { fixed, bends } => {
                if let Some(fixed) = fixed {
                    self.planar_faces(fixed, "Unfold.fixed");
                }
                if let Some(BendSet::Selected(bends)) = bends {
                    self.bend_faces(bends, "BendSet::Selected");
                }
            }
            SheetMetalKind::Refold {
                bends: Some(BendSet::Selected(bends)),
            } => self.bend_faces(bends, "BendSet::Selected"),
            SheetMetalKind::FlatPattern { fixed: Some(fixed) } => {
                self.planar_faces(fixed, "FlatPattern.fixed");
            }
            SheetMetalKind::ExtrudedHole {
                bore,
                entry: Some(entry),
                ..
            } => {
                self.planar_faces(entry, "ExtrudedHole.entry");
                self.circle_center(entry, bore);
            }
            SheetMetalKind::Stamp { entry, form, .. } => {
                if let Some(entry) = entry {
                    self.planar_faces(entry, "Stamp.entry");
                    if let StampForm::Raised { outline, .. } = form {
                        self.circle_center(entry, outline);
                    }
                }
                if let StampForm::StiffeningRib { bend, at, .. } = form {
                    self.bend_faces(bend, "StiffeningRib.bend");
                    self.rib_point(bend, at);
                }
            }
            SheetMetalKind::Convert(convert) => self.convert(convert),
            SheetMetalKind::Flange(flange) => {
                if let Some(FlangeWidth::FromEnds { first, second }) = &flange.width {
                    self.setbacks_fit(&flange.edges, literal(first), literal(second));
                }
            }
            SheetMetalKind::ContourFlange {
                edges: Some(edges),
                width: Some(FlangeWidth::FromEnds { first, second }),
                ..
            } => self.setbacks_fit(edges, literal(first), literal(second)),
            _ => {}
        }
    }

    /// `Unfold.fixed`, `FlatPattern.fixed`, `ExtrudedHole.entry`, and
    /// `Stamp.entry` select a planar face (§6.6).
    fn planar_faces<O: MemberOrder>(&mut self, selection: &'a Selection<O>, field: &str) {
        let mut curved = BTreeSet::new();
        for state in self.states(&selection.at) {
            let Some(topology) = state.matcher.topology() else {
                continue;
            };
            for entity in resolved_entities(&state.matcher, topology, selection) {
                if let Entity::Face(face) = entity {
                    if planar(topology, face) == Some(false) {
                        curved.insert(face);
                    }
                }
            }
        }
        for face in curved {
            self.error(crate::rule!(
                "design.picked-face-not-planar",
                "{field} picks face {face}, which is not planar"
            ));
        }
    }

    /// `BendSet::Selected` and `StiffeningRib.bend` select bend faces, which
    /// are not planar (§6.6).
    fn bend_faces<O: MemberOrder>(&mut self, selection: &'a Selection<O>, field: &str) {
        let mut flat = BTreeSet::new();
        for state in self.states(&selection.at) {
            let Some(topology) = state.matcher.topology() else {
                continue;
            };
            for entity in resolved_entities(&state.matcher, topology, selection) {
                if let Entity::Face(face) = entity {
                    if planar(topology, face) == Some(true) {
                        flat.insert(face);
                    }
                }
            }
        }
        for face in flat {
            self.error(crate::rule!(
                "design.bend-pick-planar-face",
                "{field} picks planar face {face}, which is no bend face"
            ));
        }
    }

    /// The one face a selection resolves to in a state.
    fn one_face<O: MemberOrder>(
        state: &InState<'a>,
        topology: &StateTopology<'a>,
        selection: &'a Selection<O>,
    ) -> Option<&'a str> {
        match all_entities(&state.matcher, topology, selection)?.as_slice() {
            [Entity::Face(face)] => Some(face),
            _ => None,
        }
    }

    /// A `Circle` outline's `center` lies on the entry face's plane (§6.6,
    /// Warning).
    fn circle_center<O: MemberOrder>(
        &mut self,
        entry: &'a Selection<O>,
        outline: &'a StampOutline,
    ) {
        let StampOutline::Circle { center, .. } = outline else {
            return;
        };
        let mut off = BTreeSet::new();
        for state in self.states(&entry.at) {
            let Some(topology) = state.matcher.topology() else {
                continue;
            };
            let Some(face) = Self::one_face(&state, topology, entry) else {
                continue;
            };
            if planar(topology, face) != Some(true) {
                continue;
            }
            let Some(point) = self.point_in(center, state.configuration) else {
                continue;
            };
            let tolerance = self.face_tolerance(topology, face);
            if surface_distance(topology, face, point).is_some_and(|distance| distance > tolerance)
            {
                off.insert(face);
            }
        }
        for face in off {
            self.warning(crate::rule!(
                "design.circle-outline-center-off-entry-plane",
                "a Circle outline's center does not lie on the plane of entry face {face}"
            ));
        }
    }

    /// A `StiffeningRib.at` lies on the bend face (§6.6, Warning).
    fn rib_point<O: MemberOrder>(
        &mut self,
        bend: &'a Selection<O>,
        at: &'a Construction<kinds::Point>,
    ) {
        let mut off = BTreeSet::new();
        for state in self.states(&bend.at) {
            let Some(topology) = state.matcher.topology() else {
                continue;
            };
            let Some(face) = Self::one_face(&state, topology, bend) else {
                continue;
            };
            let Some(point) = self.point_in(at, state.configuration) else {
                continue;
            };
            let tolerance = self.face_tolerance(topology, face);
            if surface_distance(topology, face, point).is_some_and(|distance| distance > tolerance)
            {
                off.insert(face);
            }
        }
        for face in off {
            self.warning(crate::rule!(
                "design.stiffening-rib-point-off-bend",
                "a StiffeningRib's at does not lie on bend face {face}"
            ));
        }
    }

    fn face_tolerance(&self, topology: &StateTopology<'_>, face: &str) -> f64 {
        topology
            .faces
            .get(face)
            .and_then(|face| face.tolerance)
            .map_or(self.linear, |tolerance| tolerance.get().max(self.linear))
    }

    /// A `Convert.fixed` picks a face of the solid `body` picks, each
    /// `ConvertBend::Edge.edge` an edge of it, and each `ConvertBend::Face.face`
    /// a cylindrical face of it (§6.6).
    fn convert(&mut self, convert: &'a Convert) {
        let bends: &[ConvertBend] = match &convert.from {
            ConvertFrom::Faces { bends, .. } => bends,
            ConvertFrom::ThinSolid => &[],
        };
        let mut outside: BTreeSet<(&'static str, &'a str)> = BTreeSet::new();
        for state in self.states(&convert.body.at) {
            let Some(topology) = state.matcher.topology() else {
                continue;
            };
            let Some(entities) = all_entities(&state.matcher, topology, &convert.body) else {
                continue;
            };
            let bodies: Option<BTreeSet<&str>> = entities
                .iter()
                .map(|entity| match entity {
                    Entity::Body(body) => Some(*body),
                    _ => None,
                })
                .collect();
            let Some(bodies) = bodies else { continue };
            let face_outside = |face: &str| {
                topology
                    .face_body(face)
                    .is_some_and(|body| !bodies.contains(body.id.as_str()))
            };
            let mut faces = Vec::new();
            if convert.fixed.at == convert.body.at {
                faces.push(("Convert.fixed", &convert.fixed));
            }
            for bend in bends {
                match bend {
                    ConvertBend::Face { face } if face.at == convert.body.at => {
                        faces.push(("ConvertBend::Face.face", face));
                    }
                    ConvertBend::Edge { edge, .. } if edge.at == convert.body.at => {
                        for entity in resolved_entities(&state.matcher, topology, edge) {
                            if let Entity::Edge(id) = entity {
                                let held = topology.edge_bodies(id);
                                if !held.is_empty() && held.is_disjoint(&bodies) {
                                    outside.insert(("ConvertBend::Edge.edge", id));
                                }
                            }
                        }
                    }
                    ConvertBend::Face { .. } | ConvertBend::Edge { .. } => {}
                }
            }
            for (field, selection) in faces {
                for entity in resolved_entities(&state.matcher, topology, selection) {
                    if let Entity::Face(face) = entity {
                        if face_outside(face) {
                            outside.insert((field, face));
                        }
                    }
                }
            }
        }
        for (field, entity) in outside {
            self.error(crate::rule!(
                "design.convert-pick-outside-solid",
                "{field} picks {entity}, which is no entity of the solid Convert.body picks"
            ));
        }
        let mut flat = BTreeSet::new();
        for bend in bends {
            let ConvertBend::Face { face } = bend else {
                continue;
            };
            for state in self.states(&face.at) {
                let Some(topology) = state.matcher.topology() else {
                    continue;
                };
                for entity in resolved_entities(&state.matcher, topology, face) {
                    if let Entity::Face(id) = entity {
                        if cylindrical(topology, id) == Some(false) {
                            flat.insert(id);
                        }
                    }
                }
            }
        }
        for face in flat {
            self.error(crate::rule!(
                "design.convert-bend-face-not-cylindrical",
                "ConvertBend::Face.face picks face {face}, which is not cylindrical"
            ));
        }
    }

    /// `Graft.body` picks a solid body and `Graft.sheet` sheet bodies or
    /// faces (§6.5): a picked body of a kind `refused` holds is reported.
    fn body_kinds<O: MemberOrder>(
        &mut self,
        selection: &'a Selection<O>,
        field: &str,
        wanted: &str,
        refused: impl Fn(&BodyKind) -> bool,
    ) {
        let mut wrong = BTreeSet::new();
        for state in self.states(&selection.at) {
            let Some(topology) = state.matcher.topology() else {
                continue;
            };
            for entity in resolved_entities(&state.matcher, topology, selection) {
                if let Entity::Body(id) = entity {
                    if topology
                        .bodies
                        .get(id)
                        .is_some_and(|body| refused(&body.kind))
                    {
                        wrong.insert(id);
                    }
                }
            }
        }
        for body in wrong {
            self.error(crate::rule!(
                "design.graft-pick-body-wrong-kind",
                "{field} picks body {body}, which is not {wanted}"
            ));
        }
    }

    /// Each `Unsew.edges` pick is an edge that two faces of one sheet body
    /// share, and the picked edges divide that sheet into two pieces (§6.6).
    fn unsew<O: MemberOrder>(&mut self, selection: &'a Selection<O>) {
        let mut unshared = BTreeSet::new();
        let mut undivided = BTreeSet::new();
        for state in self.states(&selection.at) {
            let Some(topology) = state.matcher.topology() else {
                continue;
            };
            let entities = all_entities(&state.matcher, topology, selection);
            let found = entities
                .clone()
                .unwrap_or_else(|| resolved_entities(&state.matcher, topology, selection));
            let mut sheets = BTreeSet::new();
            let mut every_shared = true;
            for entity in &found {
                let Entity::Edge(edge) = *entity else {
                    every_shared = false;
                    continue;
                };
                match shared_sheet(topology, edge) {
                    Shared::Sheet(body) => {
                        sheets.insert(body);
                    }
                    Shared::No => {
                        every_shared = false;
                        unshared.insert(edge);
                    }
                    Shared::Unknown => every_shared = false,
                }
            }
            let complete = entities.is_some() && every_shared;
            if let (true, [sheet]) = (complete, Vec::from_iter(sheets).as_slice()) {
                let picked: BTreeSet<&str> = found
                    .iter()
                    .filter_map(|entity| match entity {
                        Entity::Edge(edge) => Some(*edge),
                        _ => None,
                    })
                    .collect();
                if pieces(topology, sheet, &picked) != 2 {
                    undivided.insert(*sheet);
                }
            }
        }
        for edge in unshared {
            self.error(crate::rule!(
                "design.unsew-edge-not-shared-sheet",
                "Unsew.edges picks {edge}, which is no edge that two faces of one sheet body share"
            ));
        }
        for sheet in undivided {
            self.error(crate::rule!(
                "design.unsew-edges-do-not-divide-sheet",
                "the edges Unsew.edges picks do not divide sheet body {sheet} into two pieces"
            ));
        }
    }

    /// Each `EdgeSetback.edge` of a corner's `PerEdge` setbacks is incident to
    /// the corner's vertex (§6.4).
    fn corners(&mut self, controls: &'a BlendControls) {
        let mut apart = BTreeSet::new();
        for corner in &controls.corners {
            let Some(Setbacks::PerEdge(setbacks)) = &corner.setbacks else {
                continue;
            };
            for state in self.states(&corner.vertex.at) {
                let Some(topology) = state.matcher.topology() else {
                    continue;
                };
                let vertices: BTreeSet<&str> =
                    resolved_entities(&state.matcher, topology, &corner.vertex)
                        .into_iter()
                        .filter_map(|entity| match entity {
                            Entity::Vertex(vertex) => Some(vertex),
                            _ => None,
                        })
                        .collect();
                let [vertex] = Vec::from_iter(vertices)[..] else {
                    continue;
                };
                for setback in setbacks
                    .iter()
                    .filter(|setback| setback.edge.at == corner.vertex.at)
                {
                    for entity in resolved_entities(&state.matcher, topology, &setback.edge) {
                        let Entity::Edge(edge) = entity else {
                            continue;
                        };
                        let incident = topology
                            .edges
                            .get(edge)
                            .is_some_and(|edge| edge.vertices().any(|end| end.as_str() == vertex));
                        if !incident {
                            apart.insert((edge, vertex));
                        }
                    }
                }
            }
        }
        for (edge, vertex) in apart {
            self.error(crate::rule!(
                "design.edge-setback-edge-not-incident",
                "EdgeSetback.edge picks {edge}, which does not end at the corner's vertex {vertex}"
            ));
        }
    }

    /// A `ChamferSize::Vertex`'s three edges meet at one vertex (§6.4).
    fn vertex_chamfer(&mut self, setbacks: &'a [EdgeSetback]) {
        let [first, ..] = setbacks else {
            return;
        };
        if setbacks.len() != 3
            || setbacks
                .iter()
                .any(|setback| setback.edge.at != first.edge.at)
        {
            return;
        }
        let mut apart = false;
        for state in self.states(&first.edge.at) {
            let Some(topology) = state.matcher.topology() else {
                continue;
            };
            let edges: Option<Vec<&str>> = setbacks
                .iter()
                .map(|setback| {
                    match all_entities(&state.matcher, topology, &setback.edge)?.as_slice() {
                        [Entity::Edge(edge)] => Some(*edge),
                        _ => None,
                    }
                })
                .collect();
            let Some(edges) = edges else { continue };
            let mut shared: Option<BTreeSet<&str>> = None;
            for edge in edges {
                let ends: BTreeSet<&str> = topology
                    .edges
                    .get(edge)
                    .into_iter()
                    .flat_map(|edge| edge.vertices())
                    .map(VertexId::as_str)
                    .collect();
                shared = Some(match shared {
                    None => ends,
                    Some(shared) => shared.intersection(&ends).copied().collect(),
                });
            }
            apart |= shared.is_none_or(|shared| shared.is_empty());
        }
        if apart {
            self.error(crate::rule!(
                "design.chamfer-vertex-edges-meet-no-vertex",
                "a ChamferSize::Vertex's three edges do not meet at one vertex"
            ));
        }
    }

    /// A `FlangeWidth::FromEnds`'s `first` and `second` together do not exceed
    /// the edge's length (§6.6, Warning). Only a straight edge's length is
    /// read.
    fn setbacks_fit<O: MemberOrder>(
        &mut self,
        edges: &'a Selection<O>,
        first: Option<NonNegativeLength>,
        second: Option<NonNegativeLength>,
    ) {
        let (Some(first), Some(second)) = (first, second) else {
            return;
        };
        let setback = first.get() + second.get();
        let mut short = BTreeMap::new();
        for state in self.states(&edges.at) {
            let Some(topology) = state.matcher.topology() else {
                continue;
            };
            for entity in resolved_entities(&state.matcher, topology, edges) {
                let Entity::Edge(edge) = entity else {
                    continue;
                };
                if let Some(length) = straight_length(topology, edge) {
                    if setback > length + self.linear {
                        short.insert(edge, length);
                    }
                }
            }
        }
        for (edge, length) in short {
            self.warning(crate::rule!(
                "design.from-ends-setbacks-exceed-edge",
                "FlangeWidth::FromEnds setbacks ({setback}) exceed length {length} of edge {edge}"
            ));
        }
    }
}

/// Whether two faces of one sheet body share an edge.
enum Shared<'a> {
    /// They do; the sheet body.
    Sheet(&'a str),
    /// They do not.
    No,
    /// The state does not say: a face's body is not held, or the body is of
    /// general kind.
    Unknown,
}

fn shared_sheet<'a>(topology: &StateTopology<'a>, edge: &str) -> Shared<'a> {
    let faces: Vec<&str> = topology
        .edge_faces
        .get(edge)
        .into_iter()
        .flatten()
        .copied()
        .collect();
    let [first, second] = faces[..] else {
        return Shared::No;
    };
    let (Some(first), Some(second)) = (topology.face_body(first), topology.face_body(second))
    else {
        return Shared::Unknown;
    };
    if first.id != second.id {
        return Shared::No;
    }
    match first.kind {
        BodyKind::Sheet => Shared::Sheet(first.id.as_str()),
        BodyKind::Solid | BodyKind::Wire => Shared::No,
        BodyKind::General => Shared::Unknown,
    }
}

/// The number of pieces the faces of `sheet` form, joined across every edge
/// they share but those in `cut`.
fn pieces(topology: &StateTopology<'_>, sheet: &str, cut: &BTreeSet<&str>) -> usize {
    fn root(parent: &mut [usize], mut at: usize) -> usize {
        while parent[at] != at {
            parent[at] = parent[parent[at]];
            at = parent[at];
        }
        at
    }
    let faces: Vec<&str> = topology
        .faces
        .keys()
        .copied()
        .filter(|face| {
            topology
                .face_body(face)
                .is_some_and(|body| body.id.as_str() == sheet)
        })
        .collect();
    let slot: BTreeMap<&str, usize> = faces.iter().enumerate().map(|(at, f)| (*f, at)).collect();
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    for (edge, beside) in &topology.edge_faces {
        if cut.contains(edge) {
            continue;
        }
        let slots: Vec<usize> = beside
            .iter()
            .filter_map(|face| slot.get(face).copied())
            .collect();
        for pair in slots.windows(2) {
            let (first, second) = (root(&mut parent, pair[0]), root(&mut parent, pair[1]));
            parent[first] = second;
        }
    }
    (0..faces.len())
        .filter(|&at| root(&mut parent, at) == at)
        .count()
}

#[cfg(test)]
mod tests;
