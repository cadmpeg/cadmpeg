// SPDX-License-Identifier: Apache-2.0
//! The source's one sketch-driven revolve as a Fusion `Revolve` item in
//! place of the Base Feature (cadir-v11.md §3.4, Geometry-true; f3d.md §3.1,
//! "Records of a sketch-driven Revolve"): the rows of a one-sided full turn
//! about a line of its profile sketch after the shared rows of
//! [`super::item`], and the names of its result.

use std::collections::{BTreeSet, HashMap};
use std::f64::consts::TAU;

use cadmpeg_ir::design::construction::{
    kinds, Construction, ConstructionInput, InputReference, InputRole, PostStep, Relation, Subject,
};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::ops::shared::{Limit, Span};
use cadmpeg_ir::design::ops::swept::Revolve;
use cadmpeg_ir::design::selection::{PickTarget, Picks};
use cadmpeg_ir::design::units::{Quantity, UnitSymbol};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::ids::{BodyId, FaceId};
use cadmpeg_ir::math::portable::Portable;
use cadmpeg_ir::topology::Sense;

use super::extrude::profile::ProfileLoop;
use super::extrude::{self, add, distance_to_axis, dot, point, scale, sub, unique, vector};
use super::item::{new_solid, Profiled, Rows};
use super::parameters::{OwnedParameter, ParameterPlan};
use super::sketch::{CurveGeometry, Entity, PlannedSketch};

/// Millimetres per Fusion length unit (cm).
const MM_PER_CM: f64 = 10.0;
/// The expression of the `AlongAngle` parameter of a full turn (f3d.md
/// §3.1, "Angle owners").
const FULL_TURN: &str = "360.0 deg";

const NOT_A_LINE: &str = "its axis is not a line of its profile sketch";
const NOT_FULL_TURN: &str = "its extent is not one-sided full turn";

/// The revolve the design writes as a `Revolve` item.
#[derive(Debug)]
pub(super) struct RevolvePlan<'a> {
    pub(super) feature: &'a FeatureId,
    /// The position of its sketch in [`super::sketch::SketchPlan::items`].
    pub(super) sketch: usize,
    /// The number of `Sketch` items before it in the timeline.
    pub(super) after: usize,
    /// The number of timeline items before it.
    pub(super) preceding: usize,
    /// The loops of the profile region in stored order.
    pub(super) profile: Vec<ProfileLoop>,
    /// The axis line, by its position in [`PlannedSketch::curves`].
    pub(super) axis: usize,
    /// The profile curves that lie on the axis line: their loop entries take
    /// no side entry and make no face.
    on_axis: BTreeSet<usize>,
    /// The `AlongAngle` parameter.
    pub(super) angle: OwnedParameter,
    pub(super) body: &'a BodyId,
    /// The face of each loop entry off the axis, in [`RevolvePlan::entries`]
    /// order.
    pub(super) sides: Vec<&'a FaceId>,
}

impl RevolvePlan<'_> {
    /// Each loop entry whose curve does not lie on the axis, with its loop
    /// and its position in the loop, in side-token order.
    pub(super) fn entries(&self) -> impl Iterator<Item = (&ProfileLoop, usize)> {
        off_axis(&self.profile, &self.on_axis)
    }
}

fn off_axis<'p>(
    profile: &'p [ProfileLoop],
    on_axis: &'p BTreeSet<usize>,
) -> impl Iterator<Item = (&'p ProfileLoop, usize)> {
    extrude::entries(profile, false)
        .filter(move |(profile_loop, at)| !on_axis.contains(&profile_loop.entries[*at].curve))
}

/// The rows of `revolve` after the shared rows of `rows`, in order; the
/// first that fails names the reason. An angle that binds a user parameter
/// takes it out of `parameters`; the `AlongAngle` parameter takes the source
/// ordinal `ordinal`.
pub(super) fn plan<'a>(
    rows: &Rows<'a, '_>,
    revolve: &'a Revolve,
    parameters: &mut ParameterPlan,
    ordinal: u32,
) -> Result<RevolvePlan<'a>, &'static str> {
    let target = rows.target;
    let Profiled {
        body,
        sketch,
        planned,
        after,
        preceding,
        loops,
        linear,
    } = rows.profiled(&revolve.profile)?;
    if revolve.loops.is_some() || revolve.fit.is_some() {
        return Err("its profile loops or fit are not established");
    }
    let construction = revolve.axis.as_ref().ok_or("its axis is not established")?;
    let frame = Frame::of(planned).ok_or("its sketch lies on a construction plane")?;
    let axis = axis(construction, planned, &frame, linear)?;
    let line = SketchLine::of(planned, axis).ok_or(NOT_A_LINE)?;
    let tolerance = linear / MM_PER_CM;
    let on_axis = beside(planned, &loops, &line, tolerance)?;
    let angular = target.tolerances.angular.get();
    let angle = angle(revolve, parameters, ordinal, angular)?;
    new_solid(&revolve.output)?;
    let entries = off_axis(&loops, &on_axis)
        .map(|(profile_loop, at)| profile_loop.entries[at].curve)
        .collect::<Vec<_>>();
    let sides = Faces::new(target, body, linear, angular)
        .matched(planned, &frame, &entries, &line.model(&frame))
        .ok_or("its faces do not match its profile")?;
    Ok(RevolvePlan {
        feature: rows.feature,
        sketch,
        after,
        preceding,
        profile: loops,
        axis,
        on_axis,
        angle,
        body,
        sides,
    })
}

/// The origin plane of a sketch in model coordinates.
struct Frame {
    u: [f64; 3],
    v: [f64; 3],
}

impl Frame {
    fn of(sketch: &PlannedSketch<'_>) -> Option<Self> {
        let [u, v, _] = sketch.origin_plane()?.axes();
        Some(Self { u, v })
    }

    /// The model point, in mm, of the sketch point `at`, in cm.
    fn model(&self, [u, v]: [f64; 2]) -> [f64; 3] {
        add(scale(self.u, u * MM_PER_CM), scale(self.v, v * MM_PER_CM))
    }
}

/// A sketch line in sketch coordinates, in cm.
struct SketchLine {
    start: [f64; 2],
    end: [f64; 2],
    direction: [f64; 2],
}

impl SketchLine {
    /// The line `curve` of `sketch`, where it is a line of positive length.
    fn of(sketch: &PlannedSketch<'_>, curve: usize) -> Option<SketchLine> {
        let CurveGeometry::Line { start, end } = sketch.curves.get(curve)?.geometry else {
            return None;
        };
        let (start, end) = (*sketch.points.get(start)?, *sketch.points.get(end)?);
        let run = [end[0] - start[0], end[1] - start[1]];
        let length = run[0].portable_hypot(run[1]);
        (length > 0.0).then_some(SketchLine {
            start,
            end,
            direction: [run[0] / length, run[1] / length],
        })
    }
    /// The signed distance of `at` from the line, positive on its left.
    fn side(&self, at: [f64; 2]) -> f64 {
        let offset = [at[0] - self.start[0], at[1] - self.start[1]];
        self.direction[0] * offset[1] - self.direction[1] * offset[0]
    }

    /// The line in model coordinates, in mm.
    fn model(&self, frame: &Frame) -> ModelLine {
        let origin = frame.model(self.start);
        let end = frame.model(self.end);
        let run = sub(end, origin);
        ModelLine {
            origin,
            direction: scale(run, 1.0 / dot(run, run).sqrt()),
        }
    }
}

/// A line in model coordinates: a point and a unit direction, in mm.
struct ModelLine {
    origin: [f64; 3],
    direction: [f64; 3],
}

/// Row 5: the axis is a line of the profile sketch, by a `Coincident`
/// pick of the whole line or by a resolved value collinear with one, a
/// construction line before a profile line. Returns its curve.
fn axis(
    construction: &Construction<kinds::Axis>,
    sketch: &PlannedSketch<'_>,
    frame: &Frame,
    linear: f64,
) -> Result<usize, &'static str> {
    let established = construction.solution.is_none()
        && construction.mode.is_none()
        && construction.resolved_in.is_empty()
        && construction
            .after
            .iter()
            .all(|step| matches!(step, PostStep::Flip(Dimension::Literal(_))));
    if !established {
        return Err(NOT_A_LINE);
    }
    let picked = match construction.inputs.as_slice() {
        [] => None,
        [input] => Some(picked_line(input, sketch)?),
        _ => return Err(NOT_A_LINE),
    };
    let lies_on = |curve: usize, value: &cadmpeg_ir::design::construction::AxisValue| {
        let origin = point(value.origin.get());
        let direction = vector(value.direction.as_raw());
        SketchLine::of(sketch, curve).is_some_and(|line| {
            [line.start, line.end]
                .into_iter()
                .all(|at| distance_to_axis(frame.model(at), origin, direction) <= linear)
        })
    };
    match (picked, &construction.resolved) {
        (Some(curve), Some(value)) if !lies_on(curve, value) => {
            Err("its axis value does not lie on its picked line")
        }
        (Some(curve), _) => Ok(curve),
        (None, Some(value)) => {
            let mut lines = (0..sketch.curves.len())
                .filter(|curve| lies_on(*curve, value))
                .collect::<Vec<_>>();
            lines.sort_by_key(|curve| !sketch.curves[*curve].construction);
            lines.first().copied().ok_or(NOT_A_LINE)
        }
        (None, None) => Err("its axis is not established"),
    }
}

/// The written line of `sketch` that `input`, a `Coincident` pick of a
/// whole sketch entity, names, in either sense: a full turn about the line
/// makes the same solid in both.
fn picked_line(
    input: &ConstructionInput,
    sketch: &PlannedSketch<'_>,
) -> Result<usize, &'static str> {
    let ConstructionInput {
        role:
            Some(InputRole {
                subject: Subject::Whole,
                relation: Relation::Coincident,
            }),
        reference: Some(InputReference::Picks(selection)),
        value: None,
        reversed: _,
    } = input
    else {
        return Err(NOT_A_LINE);
    };
    let Picks::Named(members) = &selection.picks else {
        return Err(NOT_A_LINE);
    };
    let [picked] = members.as_slice() else {
        return Err(NOT_A_LINE);
    };
    let PickTarget::SketchEntity(entity) = &picked.target else {
        return Err(NOT_A_LINE);
    };
    match sketch.entities.get(entity) {
        Some(Entity::Curve(curve))
            if matches!(
                sketch.curves.get(*curve).map(|planned| &planned.geometry),
                Some(CurveGeometry::Line { .. })
            ) =>
        {
            Ok(*curve)
        }
        _ => Err(NOT_A_LINE),
    }
}

/// Row 5: the profile lies on one side of the axis `line`, within
/// `tolerance` cm. Returns the profile curves that lie on the line.
fn beside(
    sketch: &PlannedSketch<'_>,
    profile: &[ProfileLoop],
    line: &SketchLine,
    tolerance: f64,
) -> Result<BTreeSet<usize>, &'static str> {
    let mut least = f64::INFINITY;
    let mut most = f64::NEG_INFINITY;
    let mut on_axis = BTreeSet::new();
    for entry in profile
        .iter()
        .flat_map(|profile_loop| &profile_loop.entries)
    {
        let curve = sketch.curves.get(entry.curve).ok_or(NOT_A_LINE)?;
        let at = |index: usize| sketch.points.get(index).copied().ok_or(NOT_A_LINE);
        let (low, high) = match curve.geometry {
            CurveGeometry::Line { start, end } => {
                let (a, b) = (line.side(at(start)?), line.side(at(end)?));
                if a.abs() <= tolerance && b.abs() <= tolerance {
                    on_axis.insert(entry.curve);
                }
                (a.min(b), a.max(b))
            }
            CurveGeometry::Arc {
                center,
                start,
                end,
                radius,
                angles,
            } => {
                let (a, b) = (line.side(at(start)?), line.side(at(end)?));
                let center = line.side(at(center)?);
                // The arc reaches its extremes across the line where it
                // passes the left and the right normal of the line.
                let left = line.direction[0].portable_atan2(-line.direction[1]);
                let spans =
                    |angle: f64| (angle - angles[0]).rem_euclid(TAU) <= angles[1] - angles[0];
                let mut low = a.min(b);
                let mut high = a.max(b);
                if spans(left) {
                    high = high.max(center + radius);
                }
                if spans(left + TAU / 2.0) {
                    low = low.min(center - radius);
                }
                (low, high)
            }
            CurveGeometry::Circle { center, radius } => {
                let center = line.side(at(center)?);
                (center - radius, center + radius)
            }
        };
        least = least.min(low);
        most = most.max(high);
    }
    if least < -tolerance && most > tolerance {
        return Err("its profile crosses its axis");
    }
    Ok(on_axis)
}

/// Row 6: one side from the profile, a full turn: `ThroughAll`, or an
/// amount of 2π. Returns the `AlongAngle` parameter.
fn angle(
    revolve: &Revolve,
    parameters: &mut ParameterPlan,
    ordinal: u32,
    angular: f64,
) -> Result<OwnedParameter, &'static str> {
    let Some(Span::Between { start, end }) = &revolve.span else {
        return Err(NOT_FULL_TURN);
    };
    if !matches!(start.as_ref(), Limit::AtOrigin) {
        return Err(NOT_FULL_TURN);
    }
    let full_turn = |parameters: &mut ParameterPlan| {
        parameters.owned(ordinal, FULL_TURN.to_owned(), UnitSymbol::Deg, TAU, false)
    };
    match end.as_ref() {
        Limit::ThroughAll => Ok(full_turn(parameters)),
        Limit::Amount(Dimension::Literal(angle)) if (angle.get() - TAU).abs() <= angular => {
            Ok(full_turn(parameters))
        }
        Limit::Amount(Dimension::Parameter(parameter)) => {
            let value = parameters
                .value(parameter, Quantity::ANGLE)
                .ok_or("its angle parameter is not written as an angle")?;
            if (value - TAU).abs() > angular {
                return Err(NOT_FULL_TURN);
            }
            if let Some(claimed) = parameters.claim(parameter, Quantity::ANGLE, ordinal) {
                return Ok(claimed);
            }
            let named = parameters
                .name(parameter)
                .ok_or("its angle parameter is not written as an angle")?
                .to_owned();
            Ok(parameters.owned(ordinal, named, UnitSymbol::Deg, TAU, true))
        }
        _ => Err(NOT_FULL_TURN),
    }
}

/// A face of the body, as the matching reads it; points and lengths in mm.
#[derive(Clone, Copy)]
enum Shape {
    Plane {
        origin: [f64; 3],
        normal: [f64; 3],
    },
    Cylinder {
        origin: [f64; 3],
        axis: [f64; 3],
        radius: f64,
    },
    /// A right circular cone whose radius is `radius` at `origin` and
    /// changes by `slope` per unit of travel along `axis`.
    Cone {
        origin: [f64; 3],
        axis: [f64; 3],
        radius: f64,
        slope: f64,
    },
    Sphere {
        center: [f64; 3],
        radius: f64,
    },
    Torus {
        center: [f64; 3],
        axis: [f64; 3],
        major: f64,
        minor: f64,
    },
    Other,
}

/// The kind of surface a profile curve sweeps about the axis.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
}

/// The body's faces.
struct Faces<'a> {
    shapes: Vec<(&'a FaceId, Shape)>,
    linear: f64,
    angular: f64,
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
                        Some(SolvedSurfaceGeometry::Cone(cone))
                            if (cone.ratio().get() - 1.0).abs() <= angular =>
                        {
                            Shape::Cone {
                                origin: point(cone.origin().get()),
                                axis: vector(cone.frame().axis().as_raw()),
                                radius: cone.radius().get(),
                                slope: cone.half_angle().get().portable_tan(),
                            }
                        }
                        Some(SolvedSurfaceGeometry::Sphere(sphere)) => Shape::Sphere {
                            center: point(sphere.center().get()),
                            radius: sphere.radius().get().abs(),
                        },
                        Some(SolvedSurfaceGeometry::Torus(torus)) => Shape::Torus {
                            center: point(torus.center().get()),
                            axis: vector(torus.frame().axis().as_raw()),
                            major: torus.major_radius().get(),
                            minor: torus.minor_radius().get().abs(),
                        },
                        _ => Shape::Other,
                    };
                    shapes.push((&face.id, shape));
                }
            }
        }
        Self {
            shapes,
            linear,
            angular,
        }
    }

    /// Row 8: for each curve of `curves`, the one face of the surface it
    /// sweeps about `axis` that holds three of its points; every face found
    /// once. `None` where a face is missing, ambiguous or left over.
    fn matched(
        &self,
        sketch: &PlannedSketch<'_>,
        frame: &Frame,
        curves: &[usize],
        axis: &ModelLine,
    ) -> Option<Vec<&'a FaceId>> {
        let mut sides = Vec::with_capacity(curves.len());
        for &curve in curves {
            let (kind, samples) = self.sweep(sketch, frame, curve, axis)?;
            let side = unique(
                self.shapes
                    .iter()
                    .filter(|(_, shape)| {
                        self.about(*shape, kind, axis)
                            && samples.iter().all(|at| self.holds(*shape, *at))
                    })
                    .map(|(face, _)| *face),
            )?;
            sides.push(side);
        }
        let distinct = sides.iter().collect::<BTreeSet<_>>().len();
        (distinct == sides.len() && sides.len() == self.shapes.len()).then_some(sides)
    }

    /// The surface `curve` sweeps about `axis`, and three of its points in
    /// model coordinates.
    fn sweep(
        &self,
        sketch: &PlannedSketch<'_>,
        frame: &Frame,
        curve: usize,
        axis: &ModelLine,
    ) -> Option<(Kind, [[f64; 3]; 3])> {
        let at = |index: usize| sketch.points.get(index).map(|point| frame.model(*point));
        let on_circle = |center: [f64; 2], radius: f64, angle: f64| {
            let (sin, cos) = angle.portable_sin_cos();
            frame.model([center[0] + radius * cos, center[1] + radius * sin])
        };
        let radial = |p: [f64; 3]| distance_to_axis(p, axis.origin, axis.direction);
        let axial = |p: [f64; 3]| dot(sub(p, axis.origin), axis.direction);
        match sketch.curves.get(curve)?.geometry {
            CurveGeometry::Line { start, end } => {
                let (a, b) = (at(start)?, at(end)?);
                let middle = scale(add(a, b), 0.5);
                let kind = if (axial(a) - axial(b)).abs() <= self.linear {
                    Kind::Plane
                } else if (radial(a) - radial(b)).abs() <= self.linear {
                    Kind::Cylinder
                } else {
                    Kind::Cone
                };
                Some((kind, [a, middle, b]))
            }
            CurveGeometry::Arc {
                center,
                start,
                end,
                radius,
                angles,
            } => {
                let middle = on_circle(
                    *sketch.points.get(center)?,
                    radius,
                    f64::midpoint(angles[0], angles[1]),
                );
                Some((
                    self.round(radial(at(center)?)),
                    [at(start)?, middle, at(end)?],
                ))
            }
            CurveGeometry::Circle { center, radius } => {
                let middle = *sketch.points.get(center)?;
                let point = |turn: f64| on_circle(middle, radius, turn * TAU);
                Some((
                    self.round(radial(at(center)?)),
                    [point(0.0), point(1.0 / 3.0), point(2.0 / 3.0)],
                ))
            }
        }
    }

    /// The surface a circular curve whose centre lies `offset` mm from the
    /// axis sweeps.
    fn round(&self, offset: f64) -> Kind {
        if offset <= self.linear {
            Kind::Sphere
        } else {
            Kind::Torus
        }
    }

    /// Whether `shape` is a surface of kind `kind` about `line`.
    fn about(&self, shape: Shape, kind: Kind, line: &ModelLine) -> bool {
        let along = |direction: [f64; 3]| {
            (dot(direction, line.direction).abs() - 1.0).abs() <= self.angular
        };
        let on = |at: [f64; 3]| distance_to_axis(at, line.origin, line.direction) <= self.linear;
        match (shape, kind) {
            (Shape::Plane { normal, .. }, Kind::Plane) => along(normal),
            (Shape::Cylinder { origin, axis, .. }, Kind::Cylinder)
            | (Shape::Cone { origin, axis, .. }, Kind::Cone)
            | (
                Shape::Torus {
                    center: origin,
                    axis,
                    ..
                },
                Kind::Torus,
            ) => along(axis) && on(origin),
            (Shape::Sphere { center, .. }, Kind::Sphere) => on(center),
            _ => false,
        }
    }

    /// Whether `shape` holds the point `at`.
    fn holds(&self, shape: Shape, at: [f64; 3]) -> bool {
        let near = |gap: f64| gap.abs() <= self.linear;
        match shape {
            Shape::Plane { origin, normal } => near(dot(sub(at, origin), normal)),
            Shape::Cylinder {
                origin,
                axis,
                radius,
            } => near(distance_to_axis(at, origin, axis) - radius),
            Shape::Cone {
                origin,
                axis,
                radius,
                slope,
            } => {
                let travel = dot(sub(at, origin), axis);
                let off = distance_to_axis(at, origin, axis);
                near(off - (radius + travel * slope)) || near(off - (radius - travel * slope))
            }
            Shape::Sphere { center, radius } => {
                let offset = sub(at, center);
                near(dot(offset, offset).sqrt() - radius)
            }
            Shape::Torus {
                center,
                axis,
                major,
                minor,
            } => {
                let height = dot(sub(at, center), axis);
                let off = distance_to_axis(at, center, axis) - major;
                near(off.portable_hypot(height) - minor)
            }
            Shape::Other => false,
        }
    }
}

/// The token of the first side face (f3d.md §3.1, "Revolve face names").
const FIRST_SIDE_TOKEN: u64 = 3;

/// Name the result body and its faces in `native` (f3d.md §3.1, "Revolve
/// face names"): the body group `(3, "E", 1, 0, 0)` and side token `t`
/// `(digits(t), t, [E])` in entry order from `3`. A full turn has no cap.
pub(super) fn stamp(
    plan: &RevolvePlan<'_>,
    native: &mut crate::native::F3dNative,
) -> Result<(), cadmpeg_core::CodecError> {
    extrude::name_result(
        "revolve",
        plan.preceding,
        plan.body,
        (FIRST_SIDE_TOKEN..).zip(plan.sides.iter().copied()),
        native,
    )
}
