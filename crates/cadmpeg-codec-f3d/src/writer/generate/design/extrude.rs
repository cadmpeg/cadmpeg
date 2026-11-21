// SPDX-License-Identifier: Apache-2.0
//! The source's one sketch-driven extrude as a Fusion `Extrude` item in
//! place of the Base Feature (cadir-v11.md §3.4, Geometry-true; f3d.md §3.1,
//! "Records of a sketch-driven Extrude", "Side faces" and "Side-face walk"):
//! which extrude the writer writes with its state and its names, and why it
//! writes the Base Feature otherwise.

use std::collections::HashMap;

use cadmpeg_ir::design::container::{Container, Rollback};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::swept::{Extrude, ExtrudeDirection, ExtrudeOrigin};
use cadmpeg_ir::design::options::{BodyForm, Boolean, BooleanOp};
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::design::selection::{PickTarget, Picks};
use cadmpeg_ir::design::units::{Quantity, UnitSymbol};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::ids::{BodyId, CoedgeId, EdgeId, FaceId};
use cadmpeg_ir::topology::Sense;

use super::expression;
use super::parameters::{OwnedParameter, ParameterPlan};
use super::sketch::{CurveGeometry, PlannedSketch, SketchPlan};

use profile::{profile, ProfileLoop};

pub(super) mod profile;

/// The smallest linear tolerance the profile and the faces are checked
/// with, in mm: the decoder's linear resolution.
const MIN_LINEAR_TOLERANCE: f64 = 1e-5;
/// Millimetres per Fusion length unit (cm).
const MM_PER_CM: f64 = 10.0;

/// The extrude the design writes as an `Extrude` item.
#[derive(Debug)]
pub(super) struct ExtrudePlan<'a> {
    pub(super) feature: &'a FeatureId,
    /// The position of its sketch in [`SketchPlan::items`].
    pub(super) sketch: usize,
    /// The number of `Sketch` items before it in the timeline.
    pub(super) after: usize,
    /// The loops of the profile region in stored order: the exterior loop
    /// in ring order from the curve of side token `3`, then the holes.
    pub(super) profile: Vec<ProfileLoop>,
    pub(super) along: OwnedParameter,
    pub(super) taper: OwnedParameter,
    pub(super) body: &'a BodyId,
    pub(super) start_cap: &'a FaceId,
    pub(super) end_cap: &'a FaceId,
    /// The side face of each loop entry, in [`ExtrudePlan::entries`] order.
    pub(super) sides: Vec<&'a FaceId>,
    /// For each loop entry, the coedges of its side face and of the start
    /// cap on the edges the two faces share.
    pub(super) links: Vec<Vec<&'a CoedgeId>>,
}

impl ExtrudePlan<'_> {
    /// Each loop entry with its loop and its position in the loop, in
    /// side-token order.
    pub(super) fn entries(&self) -> impl Iterator<Item = (&ProfileLoop, usize)> {
        entries(&self.profile)
    }
}

/// Each entry of `profile` with its loop and its position in the loop: the
/// loops in stored order, each loop's entries in run order.
fn entries(profile: &[ProfileLoop]) -> impl Iterator<Item = (&ProfileLoop, usize)> {
    profile
        .iter()
        .flat_map(|profile_loop| (0..profile_loop.entries.len()).map(move |at| (profile_loop, at)))
}

/// What the design writes for the scope's extrude.
#[derive(Debug)]
pub(super) enum Decision<'a> {
    /// The scope holds no typed extrude among its geometry features.
    None,
    Extrude(Box<ExtrudePlan<'a>>),
    /// The extrude stays in the Base Feature, for `reason`.
    BaseFeature {
        feature: &'a FeatureId,
        reason: String,
    },
}

impl Decision<'_> {
    /// Whether `feature` is written as the `Extrude` item.
    pub(super) fn writes(&self, feature: &FeatureId) -> bool {
        matches!(self, Self::Extrude(plan) if plan.feature == feature)
    }

    /// The reason the Base Feature holds `feature`, where the extrude is it.
    pub(super) fn reason(&self, feature: &FeatureId) -> Option<&str> {
        match self {
            Self::BaseFeature {
                feature: extrude,
                reason,
            } if *extrude == feature => Some(reason),
            Self::None | Self::Extrude(_) | Self::BaseFeature { .. } => None,
        }
    }
}

/// The decision for the scope's features in timeline order. `failure` is the
/// error of an earlier attempt to write the `Extrude` form. A written
/// extrude whose depth binds a parameter takes that parameter out of
/// `parameters`; its two parameters take the source ordinals `ordinal` and
/// `ordinal + 1`.
pub(super) fn plan<'a>(
    target: &'a CadIr,
    scope: &'a DesignScope,
    features: &[&'a FeatureId],
    sketches: &SketchPlan<'a>,
    parameters: &mut ParameterPlan,
    failure: Option<&str>,
    ordinal: u32,
) -> Decision<'a> {
    let stateful = features
        .iter()
        .enumerate()
        .filter(|(_, feature)| !target.is_stateless(feature, None).unwrap_or(false))
        .collect::<Vec<_>>();
    let Some((position, feature, extrude)) = stateful.iter().find_map(|(position, feature)| {
        typed_extrude(target, feature).map(|extrude| (*position, **feature, extrude))
    }) else {
        return Decision::None;
    };
    let base = |reason: String| Decision::BaseFeature { feature, reason };
    if let Some(failure) = failure {
        return base(format!("the writer cannot write its records ({failure})"));
    }
    if stateful.len() > 1 {
        return base("the timeline holds other geometry features".to_owned());
    }
    let input = Input {
        target,
        scope,
        features,
        sketches,
        feature,
        position,
        extrude,
    };
    match input.plan(parameters, ordinal) {
        Ok(plan) => Decision::Extrude(Box::new(plan)),
        Err(reason) => base(reason.to_owned()),
    }
}

fn typed_extrude<'a>(target: &'a CadIr, feature: &FeatureId) -> Option<&'a Extrude> {
    let record = target.design.features.iter().find(|f| &f.id == feature)?;
    match &record.definition.operation {
        FeatureOperation::Extrude(extrude) => Some(extrude.as_ref()),
        _ => None,
    }
}

/// What the rows read.
struct Input<'a, 'p> {
    target: &'a CadIr,
    scope: &'a DesignScope,
    features: &'p [&'a FeatureId],
    sketches: &'p SketchPlan<'a>,
    feature: &'a FeatureId,
    /// The extrude's position in `features`.
    position: usize,
    extrude: &'a Extrude,
}

/// The distance of a one-sided blind span, in mm, and its binding.
enum Amount<'a> {
    Literal(f64),
    Parameter(&'a cadmpeg_ir::features::ParameterId),
}

impl<'a> Input<'a, '_> {
    /// Rows 1 to 10 of the rule, in order; the first that fails names the
    /// reason.
    fn plan(
        &self,
        parameters: &mut ParameterPlan,
        ordinal: u32,
    ) -> Result<ExtrudePlan<'a>, &'static str> {
        let target = self.target;
        let [body] = target.geometry.bodies.as_slice() else {
            return Err("the design holds more than one body");
        };
        self.rollback()?;
        self.current(&body.id)?;
        let (sketch, after) = self.sketch()?;
        let planned = &self.sketches.items[sketch];
        let linear = target.tolerances.linear.get().max(MIN_LINEAR_TOLERANCE);
        let loops = profile(planned, linear / MM_PER_CM)?;
        if planned.mirrored {
            return Err("its sketch is written mirrored on its origin plane");
        }
        self.direction()?;
        let amount = self.amount()?;
        let distance = match amount {
            Amount::Literal(distance) => distance,
            Amount::Parameter(parameter) => parameters
                .length(parameter)
                .ok_or("its depth parameter is not written as a length")?,
        };
        if distance.is_nan() || distance <= 0.0 {
            return Err("its extent is not one-sided blind");
        }
        self.output()?;
        let faces = Faces::new(target, &body.id, linear, target.tolerances.angular.get())
            .matched(planned, &loops, distance)
            .ok_or("its faces do not match its profile")?;
        let taper_ordinal = ordinal
            .checked_add(1)
            .ok_or("its parameters exceed the source ordinals")?;
        let taper_literal = expression::quantity_literal(0.0, Some(UnitSymbol::Deg))
            .ok_or("its taper has no Fusion literal")?;
        let along = match amount {
            Amount::Literal(distance) => parameters.owned(
                ordinal,
                expression::quantity_literal(distance, Some(UnitSymbol::Mm))
                    .ok_or("its depth has no Fusion literal")?,
                UnitSymbol::Mm,
                distance / MM_PER_CM,
                false,
            ),
            Amount::Parameter(parameter) => {
                match parameters.claim(parameter, Quantity::LENGTH, ordinal) {
                    Some(claimed) => claimed,
                    None => {
                        let named = parameters
                            .name(parameter)
                            .ok_or("its depth parameter is not written as a length")?
                            .to_owned();
                        parameters.owned(ordinal, named, UnitSymbol::Mm, distance / MM_PER_CM, true)
                    }
                }
            }
        };
        let taper = parameters.owned(taper_ordinal, taper_literal, UnitSymbol::Deg, 0.0, false);
        Ok(ExtrudePlan {
            feature: self.feature,
            sketch,
            after,
            profile: loops,
            along,
            taper,
            body: &body.id,
            start_cap: faces.start_cap,
            end_cap: faces.end_cap,
            sides: faces.sides,
            links: faces.links,
        })
    }

    /// Row 2: the extrude's timeline is rolled to its end.
    fn rollback(&self) -> Result<(), &'static str> {
        let member = Member::Feature(self.feature.clone());
        let container = self
            .target
            .design
            .containers
            .iter()
            .find(|container| match container {
                Container::Ordered { members, .. } => members.contains(&member),
                Container::Unordered { .. } => false,
            })
            .ok_or("it is in no ordered timeline")?;
        match container {
            Container::Ordered {
                rollback: Rollback::End,
                ..
            } => Ok(()),
            Container::Ordered { .. } | Container::Unordered { .. } => {
                Err("its timeline's rollback is not at the end")
            }
        }
    }

    /// Row 3: the current state is the state after the extrude or a later
    /// member, and holds the one body.
    fn current(&self, body: &BodyId) -> Result<(), &'static str> {
        const NOT_ITS_RESULT: &str = "the current state is not its result";
        let [state] = self.scope.current.as_slice() else {
            return Err(NOT_ITS_RESULT);
        };
        let Position::After(Member::Feature(at)) = &state.at else {
            return Err(NOT_ITS_RESULT);
        };
        let later = self
            .features
            .iter()
            .position(|feature| *feature == at)
            .is_some_and(|at| at >= self.position);
        if !later || !matches!(state.bodies.as_slice(), [only] if only == body) {
            return Err(NOT_ITS_RESULT);
        }
        Ok(())
    }

    /// Row 4: the profile is one written sketch before the extrude. Returns
    /// its position in the plan and the number of written sketches before
    /// the extrude.
    fn sketch(&self) -> Result<(usize, usize), &'static str> {
        const NOT_WRITTEN: &str = "its sketch is not written";
        let Picks::Named(members) = &self.extrude.profile.picks else {
            return Err(NOT_WRITTEN);
        };
        let [picked] = members.as_slice() else {
            return Err(NOT_WRITTEN);
        };
        let PickTarget::Sketch(sketch) = &picked.target else {
            return Err(NOT_WRITTEN);
        };
        let owner = self
            .target
            .design
            .sketches
            .iter()
            .find(|record| &record.id == sketch)
            .map(|record| &record.owner)
            .ok_or(NOT_WRITTEN)?;
        let position_of = |feature: &FeatureId| {
            self.features
                .iter()
                .position(|candidate| *candidate == feature)
        };
        let index = self
            .sketches
            .items
            .iter()
            .position(|item| item.feature == owner)
            .ok_or(NOT_WRITTEN)?;
        if position_of(owner).is_none_or(|at| at >= self.position) {
            return Err(NOT_WRITTEN);
        }
        let after = self
            .sketches
            .items
            .iter()
            .filter(|item| position_of(item.feature).is_some_and(|at| at < self.position))
            .count();
        Ok((index, after))
    }

    /// Row 7: along the profile normal from the profile plane, with no
    /// draft, fit, outside region or sheet-metal cut.
    fn direction(&self) -> Result<(), &'static str> {
        let extrude = self.extrude;
        let normal = match &extrude.direction {
            None => true,
            Some(ExtrudeDirection::ProfileNormal { reversed }) => {
                matches!(reversed, None | Some(Dimension::Literal(false)))
            }
            Some(ExtrudeDirection::Along { .. }) => false,
        };
        let plane = matches!(extrude.origin, None | Some(ExtrudeOrigin::ProfilePlane));
        if normal
            && plane
            && extrude.loops.is_none()
            && extrude.draft.is_none()
            && extrude.fit.is_none()
            && extrude.outside.is_none()
            && extrude.sheet_cut.is_none()
        {
            Ok(())
        } else {
            Err("its direction, start or draft is not established")
        }
    }

    /// Row 8: one side, from the profile plane, a blind distance.
    fn amount(&self) -> Result<Amount<'a>, &'static str> {
        const NOT_BLIND: &str = "its extent is not one-sided blind";
        let Some(Span::Between { start, end }) = &self.extrude.span else {
            return Err(NOT_BLIND);
        };
        match (start.as_ref(), end.as_ref()) {
            (Limit::AtOrigin, Limit::Amount(Dimension::Literal(length))) => {
                Ok(Amount::Literal(length.get()))
            }
            (Limit::AtOrigin, Limit::Amount(Dimension::Parameter(parameter))) => {
                Ok(Amount::Parameter(parameter))
            }
            _ => Err(NOT_BLIND),
        }
    }

    /// Row 9: one new solid; row 1 leaves no body for a join to act on.
    fn output(&self) -> Result<(), &'static str> {
        let output = &self.extrude.output;
        let new = matches!(
            output.boolean,
            Some(
                Boolean::NewBody
                    | Boolean::Apply {
                        op: BooleanOp::Join,
                        ..
                    }
            )
        );
        if new && matches!(output.body, None | Some(BodyForm::Solid)) {
            Ok(())
        } else {
            Err("it does not make one new solid")
        }
    }
}

/// A face of the body, as the matching reads it.
#[derive(Clone, Copy)]
enum Shape {
    /// A plane through `origin` whose outward normal is `normal`.
    Plane {
        origin: [f64; 3],
        normal: [f64; 3],
    },
    Cylinder {
        origin: [f64; 3],
        axis: [f64; 3],
        radius: f64,
    },
    Other,
}

/// The body's faces and the edges they share.
struct Faces<'a> {
    shapes: Vec<(&'a FaceId, Shape)>,
    /// Each edge's coedges with their faces.
    edges: HashMap<&'a EdgeId, Vec<(&'a CoedgeId, &'a FaceId)>>,
    linear: f64,
    angular: f64,
}

/// The faces of the extrude's result, each found once.
struct Matched<'a> {
    start_cap: &'a FaceId,
    end_cap: &'a FaceId,
    sides: Vec<&'a FaceId>,
    links: Vec<Vec<&'a CoedgeId>>,
}

impl<'a> Faces<'a> {
    fn new(target: &'a CadIr, body: &BodyId, linear: f64, angular: f64) -> Self {
        let geometry = &target.geometry;
        let children = geometry.child_lists();
        let surfaces = geometry
            .surfaces
            .iter()
            .map(|surface| (&surface.id, surface))
            .collect::<HashMap<_, _>>();
        let mut shapes = Vec::new();
        for region in children.body_regions(body) {
            for shell in &region.shells {
                for face in children.shell_faces(shell) {
                    let sign = match face.sense {
                        Sense::Forward => 1.0,
                        Sense::Reversed => -1.0,
                    };
                    let shape = match surfaces
                        .get(&face.surface)
                        .and_then(|surface| surface.geometry.solved())
                    {
                        Some(SolvedSurfaceGeometry::Plane(plane)) => Shape::Plane {
                            origin: point(plane.origin().get()),
                            normal: scale(vector(plane.frame().axis().as_raw()), sign),
                        },
                        Some(SolvedSurfaceGeometry::Cylinder(cylinder)) => Shape::Cylinder {
                            origin: point(cylinder.origin().get()),
                            axis: vector(cylinder.frame().axis().as_raw()),
                            radius: cylinder.radius().get(),
                        },
                        _ => Shape::Other,
                    };
                    shapes.push((&face.id, shape));
                }
            }
        }
        let face_of_loop = geometry
            .loops
            .iter()
            .map(|face_loop| (&face_loop.id, &face_loop.face))
            .collect::<HashMap<_, _>>();
        let mut edges = HashMap::<_, Vec<_>>::new();
        for coedge in &geometry.coedges {
            if let Some(face) = face_of_loop.get(&coedge.owner_loop) {
                edges
                    .entry(&coedge.edge)
                    .or_default()
                    .push((&coedge.id, *face));
            }
        }
        Self {
            shapes,
            edges,
            linear,
            angular,
        }
    }

    /// Row 10: the start cap, the end cap at `distance` mm and one side face
    /// per loop entry of the sketch on its origin plane, each the one face
    /// of its shape (a plane for a line, a cylinder for a circle or an
    /// arc); `None` where a face is missing, ambiguous or left over.
    fn matched(
        &self,
        sketch: &PlannedSketch<'_>,
        profile: &[ProfileLoop],
        distance: f64,
    ) -> Option<Matched<'a>> {
        let [u_axis, v_axis, normal] = sketch.plane.axes();
        let model = |index: usize| {
            let [u, v] = *sketch.points.get(index)?;
            Some(add(
                scale(u_axis, u * MM_PER_CM),
                scale(v_axis, v * MM_PER_CM),
            ))
        };
        let parallel = |a: [f64; 3], b: [f64; 3]| (dot(a, b) - 1.0).abs() <= self.angular;
        let across = |a: [f64; 3], b: [f64; 3]| dot(a, b).abs() <= self.angular;
        let cap = |outward: [f64; 3], offset: f64| {
            self.one(|shape| match shape {
                Shape::Plane { origin, normal: n } => {
                    parallel(n, outward) && (dot(origin, normal) - offset).abs() <= self.linear
                }
                Shape::Cylinder { .. } | Shape::Other => false,
            })
        };
        let start_cap = cap(scale(normal, -1.0), 0.0)?;
        let end_cap = cap(normal, distance)?;
        let mut sides = Vec::new();
        for (profile_loop, at) in entries(profile) {
            let curve = sketch.curves.get(profile_loop.entries[at].curve)?;
            let side = match curve.geometry {
                CurveGeometry::Line { start, end } => {
                    let (a, b) = (model(start)?, model(end)?);
                    self.one(|shape| match shape {
                        Shape::Plane { origin, normal: n } => {
                            across(n, normal)
                                && dot(sub(a, origin), n).abs() <= self.linear
                                && dot(sub(b, origin), n).abs() <= self.linear
                        }
                        Shape::Cylinder { .. } | Shape::Other => false,
                    })?
                }
                CurveGeometry::Circle { center, radius }
                | CurveGeometry::Arc { center, radius, .. } => {
                    let c = model(center)?;
                    let radius = radius * MM_PER_CM;
                    self.one(|shape| match shape {
                        Shape::Cylinder {
                            origin,
                            axis,
                            radius: r,
                        } => {
                            (dot(axis, normal).abs() - 1.0).abs() <= self.angular
                                && distance_to_axis(c, origin, axis) <= self.linear
                                && (r - radius).abs() <= self.linear
                        }
                        Shape::Plane { .. } | Shape::Other => false,
                    })?
                }
            };
            sides.push(side);
        }
        let mut all = sides.clone();
        all.extend([start_cap, end_cap]);
        let distinct = all.iter().collect::<std::collections::BTreeSet<_>>().len();
        if distinct != all.len() || all.len() != self.shapes.len() {
            return None;
        }
        let links = sides
            .iter()
            .map(|side| {
                let shared = self
                    .edges
                    .values()
                    .filter(|coedges| {
                        coedges.iter().any(|(_, face)| face == side)
                            && coedges.iter().any(|(_, face)| *face == start_cap)
                    })
                    .flat_map(|coedges| {
                        coedges
                            .iter()
                            .filter(|(_, face)| face == side || *face == start_cap)
                            .map(|(coedge, _)| *coedge)
                    })
                    .collect::<Vec<_>>();
                (!shared.is_empty()).then_some(shared)
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Matched {
            start_cap,
            end_cap,
            sides,
            links,
        })
    }

    /// The one face whose shape `test` accepts.
    fn one(&self, test: impl Fn(Shape) -> bool) -> Option<&'a FaceId> {
        let mut found = self.shapes.iter().filter(|(_, shape)| test(*shape));
        let (face, _) = found.next()?;
        found.next().is_none().then_some(*face)
    }
}

fn point(point: cadmpeg_ir::math::Point3) -> [f64; 3] {
    [point.x, point.y, point.z]
}

fn vector(vector: &cadmpeg_ir::math::Vector3) -> [f64; 3] {
    [vector.x, vector.y, vector.z]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f64; 3], factor: f64) -> [f64; 3] {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

/// The distance of `at` from the line through `origin` along the unit
/// `axis`.
fn distance_to_axis(at: [f64; 3], origin: [f64; 3], axis: [f64; 3]) -> f64 {
    let offset = sub(at, origin);
    let along = scale(axis, dot(offset, axis));
    let off = sub(offset, along);
    dot(off, off).sqrt()
}

/// The token of the start cap, the end cap and the first side face (f3d.md
/// §3.1, "Side faces"; cadir-v11.md §3.4, Names).
const START_TOKEN: u64 = 1;
const END_TOKEN: u64 = 2;
const FIRST_SIDE_TOKEN: u64 = 3;
/// The `role` and `closure` words of the sketch link of a side face whose
/// curve is one piece.
const LINK_ROLE: i64 = 1;
const LINK_CLOSURE: i64 = 1;
/// The Design reference of the result body's group.
const BODY_REFERENCE: i64 = 1;

/// Name the result body and its faces in `native` (f3d.md §3.1, "Side
/// faces", "Side-face walk" and "Profile with circular holes";
/// f3d-open-items.md DR-916): the body group `(3, "E", 1, 0, 0)`, the caps
/// `(1, "1", [E])` and `(1, "2", [E])`, side token `t` `(digits(t), t, [E])`
/// in entry order from `3`, and on each side face's edge with the start cap
/// the sketch link of its curve, whose sense is the traversal flag of an
/// exterior loop entry and the all-ones word of a hole entry (f3d.md §3.1,
/// "Sketch links of the start cap").
pub(super) fn stamp(
    plan: &ExtrudePlan<'_>,
    sketches: &[PlannedSketch<'_>],
    native: &mut crate::native::F3dNative,
) -> Result<(), cadmpeg_core::CodecError> {
    use crate::records::sketch_links::{
        DesignPersistentIdText, PersistentDesignLink, PersistentSubentityTag, SketchCurveLink,
        SketchLinkSense,
    };
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::attributes::AttributeTarget;
    let identity = super::roster::extrude::persistent_id(plan.after)?;
    let reference =
        i64::try_from(identity).map_err(|_| CodecError::Malformed("Extrude identity".into()))?;
    native.persistent_design_links.push(PersistentDesignLink {
        id: "f3d:generated:extrude-body#0".to_owned(),
        target: AttributeTarget::Body(plan.body.clone()),
        design_id: DesignPersistentIdText::try_from(identity.to_string())
            .map_err(CodecError::Malformed)?,
        design_reference: BODY_REFERENCE,
        ordinal: 0,
    });
    let faces = [(START_TOKEN, plan.start_cap), (END_TOKEN, plan.end_cap)]
        .into_iter()
        .chain((FIRST_SIDE_TOKEN..).zip(plan.sides.iter().copied()));
    for (token, face) in faces {
        let text = token.to_string();
        native
            .persistent_subentity_tags
            .push(PersistentSubentityTag {
                id: format!("f3d:generated:extrude-face#{token}"),
                target: AttributeTarget::Face(face.clone()),
                selector: i64::try_from(text.len())
                    .map_err(|_| CodecError::NotImplemented("face token".into()))?,
                token: cadmpeg_core::text::NonBlankString::new(text)
                    .ok_or_else(|| CodecError::Malformed("face token".into()))?,
                token_integer: 0,
                design_references: vec![reference],
                ordinal: 0,
            });
    }
    for ((profile_loop, at), coedges) in plan.entries().zip(&plan.links) {
        let entry = profile_loop.entries[at];
        let sense = if profile_loop.exterior {
            Some(
                SketchLinkSense::try_from(i64::from(entry.traversal_flag()))
                    .map_err(|error| CodecError::Malformed(error.into()))?,
            )
        } else {
            None
        };
        let curve = super::roster::sketches::curve_identity(sketches, plan.sketch, entry.curve)?;
        let curve = i64::try_from(curve)
            .map_err(|_| CodecError::NotImplemented("sketch curve identity".into()))?;
        for coedge in coedges {
            native.sketch_curve_links.push(SketchCurveLink {
                id: format!("f3d:generated:extrude-link#{coedge}"),
                target: AttributeTarget::Coedge((*coedge).clone()),
                sketch_curve_id: curve,
                ref_b: super::roster::sketches::CURVE_SECONDARY_IDENTITY,
                sense,
                role: LINK_ROLE,
                closure: LINK_CLOSURE,
            });
        }
    }
    Ok(())
}
