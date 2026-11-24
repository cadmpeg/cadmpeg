// SPDX-License-Identifier: Apache-2.0
//! The source's one sketch-driven extrude as a Fusion `Extrude` item in
//! place of the Base Feature (cadir-v11.md §3.4, Geometry-true; f3d.md §3.1,
//! "Records of a sketch-driven Extrude", "Side faces" and "Side-face walk"):
//! the rows of an extrude after the shared rows of [`super::item`], and the
//! names of its result.

use std::collections::HashMap;

use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::swept::{Extrude, ExtrudeDirection, ExtrudeOrigin};
use cadmpeg_ir::design::units::{Quantity, UnitSymbol};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::ids::{BodyId, CoedgeId, EdgeId, FaceId};
use cadmpeg_ir::topology::Sense;

use super::expression;
use super::item::{new_solid, Profiled, Rows};
use super::parameters::{OwnedParameter, ParameterPlan};
use super::sketch::{CurveGeometry, PlannedSketch};

use profile::ProfileLoop;

pub(super) mod profile;

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
    /// The number of timeline items before it: the `Sketch` items and the
    /// `WorkPlane` items that precede them.
    pub(super) preceding: usize,
    /// The loops of the profile region in stored order: the exterior loop
    /// in ring order from the curve of side token `3`, then the holes.
    pub(super) profile: Vec<ProfileLoop>,
    /// Whether the extrude travels against its sketch's normal: its
    /// `AlongDistance` is negative and its side tokens run against each
    /// loop's ring (f3d.md §3.1, "Side-face walk").
    pub(super) reversed: bool,
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
        entries(&self.profile, self.reversed)
    }
}

/// Each entry of `profile` with its loop and its position in the loop, in
/// side-token order: the loops in stored order, each loop's entries from its
/// first in run order, or against it where `reversed`.
pub(super) fn entries(
    profile: &[ProfileLoop],
    reversed: bool,
) -> impl Iterator<Item = (&ProfileLoop, usize)> {
    profile.iter().flat_map(move |profile_loop| {
        let count = profile_loop.entries.len();
        (0..count).map(move |step| {
            let at = if reversed {
                (count - step) % count
            } else {
                step
            };
            (profile_loop, at)
        })
    })
}

/// The distance of a one-sided blind span, in mm, and its binding.
enum Amount<'a> {
    Literal(f64),
    Parameter(&'a cadmpeg_ir::features::ParameterId),
}

/// The rows of `extrude` after the shared rows of `rows`, in order; the
/// first that fails names the reason. A depth that binds a user parameter
/// takes it out of `parameters`; the two owned parameters take the source
/// ordinals `ordinal` and `ordinal + 1`.
pub(super) fn plan<'a>(
    rows: &Rows<'a, '_>,
    extrude: &'a Extrude,
    parameters: &mut ParameterPlan,
    ordinal: u32,
) -> Result<ExtrudePlan<'a>, &'static str> {
    let target = rows.target;
    let Profiled {
        body,
        sketch,
        planned,
        after,
        preceding,
        loops,
        linear,
    } = rows.profiled(&extrude.profile)?;
    let reversed = direction(extrude)?;
    let sign = if reversed { -1.0 } else { 1.0 };
    let amount = amount(extrude)?;
    let distance = match amount {
        Amount::Literal(distance) => distance,
        Amount::Parameter(parameter) => parameters
            .length(parameter)
            .ok_or("its depth parameter is not written as a length")?,
    };
    if distance.is_nan() || distance <= 0.0 {
        return Err("its extent is not one-sided blind");
    }
    new_solid(&extrude.output)?;
    let faces = Faces::new(target, body, linear, target.tolerances.angular.get())
        .matched(planned, &loops, distance, reversed)
        .ok_or("its faces do not match its profile")?;
    let taper_ordinal = ordinal
        .checked_add(1)
        .ok_or("its parameters exceed the source ordinals")?;
    let taper_literal = expression::quantity_literal(0.0, Some(UnitSymbol::Deg))
        .ok_or("its taper has no Fusion literal")?;
    let along = match amount {
        Amount::Literal(distance) => parameters.owned(
            ordinal,
            expression::quantity_literal(sign * distance, Some(UnitSymbol::Mm))
                .ok_or("its depth has no Fusion literal")?,
            UnitSymbol::Mm,
            sign * distance / MM_PER_CM,
            false,
        ),
        Amount::Parameter(parameter) if reversed => {
            let named = parameters
                .name(parameter)
                .ok_or("its depth parameter is not written as a length")?;
            // `-D1`, the form Fusion files hold (f3d.md §3.1).
            let negated = format!("-{named}");
            parameters.owned(
                ordinal,
                negated,
                UnitSymbol::Mm,
                -distance / MM_PER_CM,
                true,
            )
        }
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
        feature: rows.feature,
        sketch,
        after,
        preceding,
        profile: loops,
        reversed,
        along,
        taper,
        body,
        start_cap: faces.start_cap,
        end_cap: faces.end_cap,
        sides: faces.sides,
        links: faces.links,
    })
}

/// Row 7: along or against the profile normal from the profile plane, with
/// no draft, fit, outside region or sheet-metal cut. Returns whether it runs
/// against the normal.
fn direction(extrude: &Extrude) -> Result<bool, &'static str> {
    let reversed = match &extrude.direction {
        None => Some(false),
        Some(ExtrudeDirection::ProfileNormal { reversed }) => match reversed {
            None => Some(false),
            Some(Dimension::Literal(reversed)) => Some(*reversed),
            Some(Dimension::Parameter(_) | Dimension::Negated(_)) => None,
        },
        Some(ExtrudeDirection::Along { .. }) => None,
    };
    let established = matches!(extrude.origin, None | Some(ExtrudeOrigin::ProfilePlane))
        && extrude.loops.is_none()
        && extrude.draft.is_none()
        && extrude.fit.is_none()
        && extrude.outside.is_none()
        && extrude.sheet_cut.is_none();
    match reversed {
        Some(reversed) if established => Ok(reversed),
        _ => Err("its direction, start or draft is not established"),
    }
}

/// Row 8: one side, from the profile plane, a blind distance.
fn amount(extrude: &Extrude) -> Result<Amount<'_>, &'static str> {
    const NOT_BLIND: &str = "its extent is not one-sided blind";
    let Some(Span::Between { start, end }) = &extrude.span else {
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
    /// The model points of each edge's start and end vertex.
    ends: HashMap<&'a EdgeId, [[f64; 3]; 2]>,
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
        for region in children.stated_body_regions(body) {
            for shell in region.stated_shells() {
                for face in children.stated_shell_faces(shell) {
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
        let points = geometry
            .points
            .iter()
            .map(|at| (&at.id, point(at.position().get())))
            .collect::<HashMap<_, _>>();
        let vertices = geometry
            .vertices
            .iter()
            .filter_map(|vertex| Some((&vertex.id, *points.get(&vertex.point)?)))
            .collect::<HashMap<_, _>>();
        let ends = geometry
            .edges
            .iter()
            .filter_map(|edge| {
                let ends = edge.ends.as_ref()?;
                Some((
                    &edge.id,
                    [*vertices.get(&ends.start)?, *vertices.get(&ends.end)?],
                ))
            })
            .collect();
        Self {
            shapes,
            edges,
            ends,
            linear,
            angular,
        }
    }

    /// Row 10: the start cap on the sketch's origin plane, the end cap
    /// `distance` mm along the travel (the plane's normal, or against it
    /// where `reversed`) and one side face per loop entry in side-token
    /// order, each the one face of its shape (a plane for a line, a cylinder
    /// for a circle or an arc); where several faces have the shape of a line
    /// or an arc, the one whose edge on the start cap runs between the
    /// curve's ends. `None` where a face is missing, ambiguous or left over.
    fn matched(
        &self,
        sketch: &PlannedSketch<'_>,
        profile: &[ProfileLoop],
        distance: f64,
        reversed: bool,
    ) -> Option<Matched<'a>> {
        let [u_axis, v_axis, normal] = sketch.origin_plane()?.axes();
        let sign = if reversed { -1.0 } else { 1.0 };
        let travel = scale(normal, sign);
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
        let start_cap = cap(scale(travel, -1.0), 0.0)?;
        let end_cap = cap(travel, sign * distance)?;
        let mut on_start_cap = HashMap::<&'a FaceId, Vec<&'a EdgeId>>::new();
        for (edge, coedges) in &self.edges {
            if coedges.iter().any(|(_, face)| *face == start_cap) {
                for (_, face) in coedges.iter().filter(|(_, face)| *face != start_cap) {
                    let shared = on_start_cap.entry(*face).or_default();
                    if !shared.contains(edge) {
                        shared.push(*edge);
                    }
                }
            }
        }
        let near = |a: [f64; 3], b: [f64; 3]| {
            let offset = sub(a, b);
            dot(offset, offset).sqrt() <= self.linear
        };
        let between = |face: &FaceId, [a, b]: [[f64; 3]; 2]| {
            on_start_cap.get(face).is_some_and(|shared| {
                shared.iter().any(|edge| {
                    self.ends.get(edge).is_some_and(|&[p, q]| {
                        (near(p, a) && near(q, b)) || (near(p, b) && near(q, a))
                    })
                })
            })
        };
        let mut sides = Vec::new();
        for (profile_loop, at) in entries(profile, reversed) {
            let curve = sketch.curves.get(profile_loop.entries[at].curve)?;
            let (shaped, ends) = match curve.geometry {
                CurveGeometry::Line { start, end } => {
                    let (a, b) = (model(start)?, model(end)?);
                    let shaped = self.all(|shape| match shape {
                        Shape::Plane { origin, normal: n } => {
                            across(n, normal)
                                && dot(sub(a, origin), n).abs() <= self.linear
                                && dot(sub(b, origin), n).abs() <= self.linear
                        }
                        Shape::Cylinder { .. } | Shape::Other => false,
                    });
                    (shaped, Some([a, b]))
                }
                CurveGeometry::Circle { center, radius } => {
                    (self.on_cylinder(model(center)?, radius, normal), None)
                }
                CurveGeometry::Arc {
                    center,
                    start,
                    end,
                    radius,
                    ..
                } => (
                    self.on_cylinder(model(center)?, radius, normal),
                    Some([model(start)?, model(end)?]),
                ),
            };
            let side = if let [only] = shaped[..] {
                only
            } else {
                let ends = ends?;
                unique(shaped.into_iter().filter(|face| between(face, ends)))?
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
                let shared = on_start_cap
                    .get(side)?
                    .iter()
                    .filter_map(|edge| self.edges.get(edge))
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
        unique(self.all(test).into_iter())
    }

    /// The faces whose shape `test` accepts.
    fn all(&self, test: impl Fn(Shape) -> bool) -> Vec<&'a FaceId> {
        self.shapes
            .iter()
            .filter(|(_, shape)| test(*shape))
            .map(|(face, _)| *face)
            .collect()
    }

    /// The faces on the cylinder of radius `radius` cm about the model
    /// point `center` along `normal`.
    fn on_cylinder(&self, center: [f64; 3], radius: f64, normal: [f64; 3]) -> Vec<&'a FaceId> {
        let radius = radius * MM_PER_CM;
        self.all(|shape| match shape {
            Shape::Cylinder {
                origin,
                axis,
                radius: r,
            } => {
                (dot(axis, normal).abs() - 1.0).abs() <= self.angular
                    && distance_to_axis(center, origin, axis) <= self.linear
                    && (r - radius).abs() <= self.linear
            }
            Shape::Plane { .. } | Shape::Other => false,
        })
    }
}

/// The only item of `items`.
pub(super) fn unique<T>(mut items: impl Iterator<Item = T>) -> Option<T> {
    let found = items.next()?;
    items.next().is_none().then_some(found)
}

pub(super) fn point(point: cadmpeg_ir::math::Point3) -> [f64; 3] {
    [point.x, point.y, point.z]
}

pub(super) fn vector(vector: &cadmpeg_ir::math::Vector3) -> [f64; 3] {
    [vector.x, vector.y, vector.z]
}

pub(super) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(super) fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(super) fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(super) fn scale(a: [f64; 3], factor: f64) -> [f64; 3] {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

/// The distance of `at` from the line through `origin` along the unit
/// `axis`.
pub(super) fn distance_to_axis(at: [f64; 3], origin: [f64; 3], axis: [f64; 3]) -> f64 {
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
    use crate::records::sketch_links::{SketchCurveLink, SketchLinkSense};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::attributes::AttributeTarget;
    let faces = [(START_TOKEN, plan.start_cap), (END_TOKEN, plan.end_cap)]
        .into_iter()
        .chain((FIRST_SIDE_TOKEN..).zip(plan.sides.iter().copied()));
    name_result("extrude", plan.preceding, plan.body, faces, native)?;
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

/// Name the result `body` of the `family` item after `preceding` timeline
/// items and its `faces` in `native` (f3d.md §3.1, "Side faces"): the body
/// group `(3, "E", 1, 0, 0)` and face token `t` `(digits(t), t, [E])`.
pub(super) fn name_result<'f>(
    family: &str,
    preceding: usize,
    body: &BodyId,
    faces: impl Iterator<Item = (u64, &'f FaceId)>,
    native: &mut crate::native::F3dNative,
) -> Result<(), cadmpeg_core::CodecError> {
    use crate::records::sketch_links::{
        DesignPersistentIdText, PersistentDesignLink, PersistentSubentityTag,
    };
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::attributes::AttributeTarget;
    let identity = super::roster::swept::persistent_id(preceding)?;
    let reference = i64::try_from(identity)
        .map_err(|_| CodecError::malformed(format_args!("{family} identity")))?;
    native.persistent_design_links.push(PersistentDesignLink {
        id: format!("f3d:generated:{family}-body#0"),
        target: AttributeTarget::Body(body.clone()),
        design_id: DesignPersistentIdText::try_from(identity.to_string())
            .map_err(CodecError::Malformed)?,
        design_reference: BODY_REFERENCE,
        ordinal: 0,
    });
    for (token, face) in faces {
        let text = token.to_string();
        native
            .persistent_subentity_tags
            .push(PersistentSubentityTag {
                id: format!("f3d:generated:{family}-face#{token}"),
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
    Ok(())
}
