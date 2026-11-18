// SPDX-License-Identifier: Apache-2.0
//! Neutral carriers of native B-rep curves, surfaces, and pcurves.
//!
//! The analytic kinds share the neutral parameterizations: a frame's x axis
//! is the reference direction and x × y the axis, the angle runs about the
//! axis, and a plane's v direction is its normal × u. So a cylinder, cone,
//! sphere, or torus face keeps its pcurves as they are (a torus shifts v by
//! its tube start), and a plane face maps them by the linear map from its
//! stored axes to its orthonormal frame. A line becomes a unit-speed line,
//! and its interval the arc lengths of its vertices.
//!
//! A circle, ellipse, or line edge that carries a NURBS form has its
//! interval in that form's parameter: its neutral interval is the angles, or
//! arc lengths, of its vertices, in the turning sense that the form's middle
//! point shows. An analytic face that carries a NURBS form has its loops in
//! that form's (u, v): the face keeps its analytic carrier, its pcurves are
//! omitted, and its sense compares the form's normal with the carrier's.

use std::f64::consts::{FRAC_PI_2, TAU};

use cadmpeg_ir::geometry::analytic::{
    CircleCurve, ConeSurface, CylinderSurface, EllipseCurve, LineCurve, PlaneSurface,
    SphereSurface, TorusSurface,
};
use cadmpeg_ir::geometry::nurbs::{
    NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface, NurbsSurfaceAxis,
};
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs, PcurveNurbsPoles};
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::math::portable::Portable;
use cadmpeg_ir::math::{Point2, Point3, Vector3};

use super::brep::{Curve, Curve2, Edge, Face, Frame, Surface};

/// A 3-vector.
pub(super) type V3 = [f64; 3];

/// Highest spline degree the evaluator takes, the record schema's own cap.
const MAX_DEGREE: usize = 16;
/// Relative step of a finite-difference partial on a NURBS form.
const DIFFERENCE_STEP: f64 = 1.0e-6;

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: V3, factor: f64) -> V3 {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}

fn unit(a: V3) -> Option<V3> {
    let length = norm(a);
    (length > 0.0 && length.is_finite()).then(|| scale(a, 1.0 / length))
}

fn point(a: V3) -> Point3 {
    Point3::new(a[0], a[1], a[2])
}

fn vector(a: V3) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}

/// The axis of a frame: x × y.
fn frame_axis(frame: &Frame) -> V3 {
    cross(frame.x_axis, frame.y_axis)
}

/// The angle in `[0, 2π)` of `offset` in the plane of `reference` and
/// `transverse`, the coordinates scaled by `[1 / a, 1 / b]`.
fn angle(offset: V3, reference: V3, transverse: V3, radii: [f64; 2]) -> f64 {
    let turn =
        (dot(offset, transverse) / radii[1]).portable_atan2(dot(offset, reference) / radii[0]);
    if turn < 0.0 {
        turn + TAU
    } else {
        turn
    }
}

// ---------------------------------------------------------------------------
// NURBS evaluation.

/// The knot span that holds `t` for `count` control points of `degree`.
fn span(knots: &[f64], degree: usize, count: usize, t: f64) -> Option<usize> {
    if degree > MAX_DEGREE || count <= degree || knots.len() != count + degree + 1 {
        return None;
    }
    let last = count - 1;
    if t >= knots[last + 1] {
        return Some(last);
    }
    if t <= knots[degree] {
        return Some(degree);
    }
    (degree..=last).find(|&index| t >= knots[index] && t < knots[index + 1])
}

/// The nonzero basis functions of `degree`, at most [`MAX_DEGREE`], at `t`
/// on `span`: the first `degree + 1` entries.
fn basis(knots: &[f64], degree: usize, span: usize, t: f64) -> [f64; MAX_DEGREE + 1] {
    let mut values = [0.0; MAX_DEGREE + 1];
    let mut left = [0.0; MAX_DEGREE + 1];
    let mut right = [0.0; MAX_DEGREE + 1];
    values[0] = 1.0;
    for order in 1..=degree {
        left[order] = t - knots[span + 1 - order];
        right[order] = knots[span + order] - t;
        let mut saved = 0.0;
        for index in 0..order {
            let denominator = right[index + 1] + left[order - index];
            let term = if denominator == 0.0 {
                0.0
            } else {
                values[index] / denominator
            };
            values[index] = saved + right[index + 1] * term;
            saved = left[order - index] * term;
        }
        values[order] = saved;
    }
    values
}

/// A homogeneous B-spline curve: knots and `[x w, y w, z w, w]` rows.
pub(super) struct Homogeneous3 {
    knots: Vec<f64>,
    rows: Vec<[f64; 4]>,
}

impl Homogeneous3 {
    /// The form of a spline or rational spline curve.
    pub(super) fn of(curve: &Curve) -> Option<Self> {
        match curve {
            Curve::Spline { knots, points } => Some(Self {
                knots: knots.clone(),
                rows: points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect(),
            }),
            Curve::RationalSpline { knots, homogeneous } => Some(Self {
                knots: knots.clone(),
                rows: homogeneous.clone(),
            }),
            _ => None,
        }
    }

    /// The point at `t`.
    pub(super) fn at(&self, t: f64) -> Option<V3> {
        let degree = self.knots.len().checked_sub(self.rows.len() + 1)?;
        let span = span(&self.knots, degree, self.rows.len(), t)?;
        let values = basis(&self.knots, degree, span, t);
        let mut sum = [0.0; 4];
        for (offset, weight) in values.iter().take(degree + 1).enumerate() {
            let row = self.rows.get(span + offset - degree)?;
            for (slot, value) in sum.iter_mut().zip(row) {
                *slot += weight * value;
            }
        }
        (sum[3] != 0.0).then(|| [sum[0] / sum[3], sum[1] / sum[3], sum[2] / sum[3]])
    }
}

/// A homogeneous tensor-product B-spline surface.
struct HomogeneousNet {
    u_knots: Vec<f64>,
    v_knots: Vec<f64>,
    rows: Vec<Vec<[f64; 4]>>,
}

impl HomogeneousNet {
    fn of(surface: &Surface) -> Option<Self> {
        match surface {
            Surface::Spline {
                u_knots,
                v_knots,
                points,
            } => Some(Self {
                u_knots: u_knots.clone(),
                v_knots: v_knots.clone(),
                rows: points
                    .iter()
                    .map(|row| row.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect())
                    .collect(),
            }),
            Surface::RationalSpline {
                u_knots,
                v_knots,
                homogeneous,
            } => Some(Self {
                u_knots: u_knots.clone(),
                v_knots: v_knots.clone(),
                rows: homogeneous.clone(),
            }),
            _ => None,
        }
    }

    fn at(&self, u: f64, v: f64) -> Option<V3> {
        let columns = self.rows.first()?.len();
        let u_degree = self.u_knots.len().checked_sub(self.rows.len() + 1)?;
        let v_degree = self.v_knots.len().checked_sub(columns + 1)?;
        let u_span = span(&self.u_knots, u_degree, self.rows.len(), u)?;
        let v_span = span(&self.v_knots, v_degree, columns, v)?;
        let u_values = basis(&self.u_knots, u_degree, u_span, u);
        let v_values = basis(&self.v_knots, v_degree, v_span, v);
        let mut sum = [0.0; 4];
        for (i, u_weight) in u_values.iter().take(u_degree + 1).enumerate() {
            let row = self.rows.get(u_span + i - u_degree)?;
            for (j, v_weight) in v_values.iter().take(v_degree + 1).enumerate() {
                let pole = row.get(v_span + j - v_degree)?;
                for (slot, value) in sum.iter_mut().zip(pole) {
                    *slot += u_weight * v_weight * value;
                }
            }
        }
        (sum[3] != 0.0).then(|| [sum[0] / sum[3], sum[1] / sum[3], sum[2] / sum[3]])
    }

    /// The point and natural normal at `(u, v)`, by central differences.
    fn normal(&self, u: f64, v: f64, steps: [f64; 2]) -> Option<(V3, V3)> {
        let du = sub(self.at(u + steps[0], v)?, self.at(u - steps[0], v)?);
        let dv = sub(self.at(u, v + steps[1])?, self.at(u, v - steps[1])?);
        Some((self.at(u, v)?, cross(du, dv)))
    }
}

// ---------------------------------------------------------------------------
// Curves.

/// The neutral carrier of an edge.
pub(super) struct CurveCarrier {
    /// The carrier.
    pub(super) geometry: SolvedCurveGeometry,
    /// The edge's interval on the carrier, min < max.
    pub(super) interval: [f64; 2],
    /// Whether the carrier is the leader of a surface intersection.
    pub(super) approximated: bool,
}

/// Why an edge has no neutral carrier.
pub(super) type Refusal = &'static str;

fn nurbs_curve(curve: &Curve) -> Result<SolvedCurveGeometry, Refusal> {
    let (knots, points, weights) = match curve {
        Curve::Spline { knots, points } => (
            knots.clone(),
            points.iter().copied().map(point).collect::<Vec<_>>(),
            None,
        ),
        Curve::RationalSpline { knots, homogeneous } => {
            let mut points = Vec::with_capacity(homogeneous.len());
            let mut weights = Vec::with_capacity(homogeneous.len());
            for row in homogeneous {
                if row[3] == 0.0 {
                    return Err("a rational spline pole has weight zero");
                }
                points.push(Point3::new(
                    row[0] / row[3],
                    row[1] / row[3],
                    row[2] / row[3],
                ));
                weights.push(row[3]);
            }
            (knots.clone(), points, Some(weights))
        }
        Curve::Polyline { points } => {
            let count = points.len();
            if count < 2 {
                return Err("a polyline has fewer than two points");
            }
            let mut knots = Vec::with_capacity(count + 2);
            // Segment i spans [i, i + 1]: a degree-one knot vector clamped
            // at 0 and at count - 1.
            knots.push(0.0);
            let mut knot = 0.0;
            for _ in 0..count {
                knots.push(knot);
                knot += 1.0;
            }
            knots.push(knot - 1.0);
            (knots, points.iter().copied().map(point).collect(), None)
        }
        _ => return Err("the curve is not a spline"),
    };
    let degree = knots
        .len()
        .checked_sub(points.len() + 1)
        .and_then(|degree| u32::try_from(degree).ok())
        .ok_or("a spline has more control points than its knots allow")?;
    let poles =
        NurbsPoles3::from_lanes(points, weights).map_err(|_| "a spline pole is not admitted")?;
    NurbsCurve::new(degree, knots, poles, false)
        .map(SolvedCurveGeometry::Nurbs)
        .map_err(|_| "a spline is not an admitted NURBS curve")
}

/// The neutral carrier of `edge`, whose start and end vertices are at
/// `ends`; `closed` when they are one vertex.
pub(super) fn edge_carrier(
    edge: &Edge,
    ends: [V3; 2],
    closed: bool,
) -> Result<CurveCarrier, Refusal> {
    let form = edge.nurbs.as_ref().and_then(Homogeneous3::of);
    let middle = || {
        form.as_ref()
            .and_then(|form| form.at(0.5 * (edge.interval[0] + edge.interval[1])))
    };
    let carrier = |geometry, interval| CurveCarrier {
        geometry,
        interval,
        approximated: false,
    };
    match &edge.curve {
        Curve::Line { start, end } => {
            let direction = unit(sub(*end, *start)).ok_or("a line has equal ends")?;
            let interval = [
                dot(sub(ends[0], *start), direction),
                dot(sub(ends[1], *start), direction),
            ];
            let line = LineCurve::try_new(point(*start), vector(direction))?;
            Ok(carrier(SolvedCurveGeometry::Line(line), interval))
        }
        Curve::Circle { frame, radius } => {
            let axis = frame_axis(frame);
            if form.is_none() {
                let circle = CircleCurve::try_new(
                    point(frame.origin),
                    vector(axis),
                    vector(frame.x_axis),
                    *radius,
                )?;
                return Ok(carrier(SolvedCurveGeometry::Circle(circle), edge.interval));
            }
            let middle = middle().ok_or("a circle's NURBS form does not evaluate")?;
            // The reference direction runs to the start vertex, so the edge
            // starts at angle zero.
            let reference =
                unit(sub(ends[0], frame.origin)).ok_or("a circle edge starts at its centre")?;
            let plane = Turn {
                axis,
                reference,
                centre: frame.origin,
                radii: [*radius; 2],
            };
            let (axis, sweep) = plane
                .through(0.0, middle, ends[1], closed)
                .ok_or("a circle edge's middle point is off its plane")?;
            let circle = CircleCurve::try_new(
                point(frame.origin),
                vector(axis),
                vector(reference),
                *radius,
            )?;
            Ok(carrier(SolvedCurveGeometry::Circle(circle), [0.0, sweep]))
        }
        Curve::Ellipse { frame, radii } => {
            let axis = frame_axis(frame);
            // The neutral ellipse turns from its major direction.
            let (major, minor_direction, major_radius, minor_radius, shift) =
                if radii[0] >= radii[1] {
                    (frame.x_axis, frame.y_axis, radii[0], radii[1], 0.0)
                } else {
                    (
                        frame.y_axis,
                        scale(frame.x_axis, -1.0),
                        radii[1],
                        radii[0],
                        FRAC_PI_2,
                    )
                };
            if form.is_none() {
                let ellipse = EllipseCurve::try_new(
                    point(frame.origin),
                    vector(axis),
                    vector(major),
                    major_radius,
                    minor_radius,
                )?;
                return Ok(carrier(
                    SolvedCurveGeometry::Ellipse(ellipse),
                    [edge.interval[0] - shift, edge.interval[1] - shift],
                ));
            }
            let middle = middle().ok_or("an ellipse's NURBS form does not evaluate")?;
            let radii = [major_radius, minor_radius];
            let start = angle(sub(ends[0], frame.origin), major, minor_direction, radii);
            let plane = Turn {
                axis,
                reference: major,
                centre: frame.origin,
                radii,
            };
            let (axis, sweep) = plane
                .through(start, middle, ends[1], closed)
                .ok_or("an ellipse edge's middle point is off its plane")?;
            // About a reversed axis the angles run the other way.
            let start = angle(sub(ends[0], frame.origin), major, cross(axis, major), radii);
            let ellipse = EllipseCurve::try_new(
                point(frame.origin),
                vector(axis),
                vector(major),
                major_radius,
                minor_radius,
            )?;
            Ok(carrier(
                SolvedCurveGeometry::Ellipse(ellipse),
                [start, start + sweep],
            ))
        }
        Curve::Polyline { .. } | Curve::Spline { .. } | Curve::RationalSpline { .. } => {
            Ok(carrier(nurbs_curve(&edge.curve)?, edge.interval))
        }
        Curve::SurfaceIntersection { leader, .. } => match leader.as_ref() {
            Curve::Polyline { .. } | Curve::Spline { .. } | Curve::RationalSpline { .. } => {
                Ok(CurveCarrier {
                    geometry: nurbs_curve(leader)?,
                    interval: edge.interval,
                    approximated: true,
                })
            }
            _ => Err("a surface intersection's leader is not a spline or polyline"),
        },
    }
}

/// The plane of a circular or elliptic turn.
struct Turn {
    /// The axis the angles turn about.
    axis: V3,
    /// The direction at angle zero.
    reference: V3,
    /// The centre.
    centre: V3,
    /// The radii along the reference and the transverse directions.
    radii: [f64; 2],
}

impl Turn {
    /// The axis and sweep of the turn from angle `start` through `middle`
    /// to `end`: the axis is this plane's or its reverse, whichever the turn
    /// runs counterclockwise about.
    fn through(&self, start: f64, middle: V3, end: V3, closed: bool) -> Option<(V3, f64)> {
        let axis = unit(self.axis)?;
        let transverse = cross(axis, self.reference);
        let relative = |p: V3| {
            let turn = angle(sub(p, self.centre), self.reference, transverse, self.radii) - start;
            if turn < 0.0 {
                turn + TAU
            } else {
                turn
            }
        };
        let middle_turn = relative(middle);
        let mut end_turn = relative(end);
        if closed || end_turn == 0.0 {
            end_turn = TAU;
        }
        if middle_turn > 0.0 && middle_turn < end_turn {
            return Some((axis, end_turn));
        }
        // The turn runs the other way: about the reversed axis, the end sits
        // at the remaining angle.
        let sweep = if closed { TAU } else { TAU - end_turn };
        (sweep > 0.0).then_some((scale(axis, -1.0), sweep))
    }
}

// ---------------------------------------------------------------------------
// Surfaces.

/// How the pcurves of a face reach its neutral carrier's (u, v).
#[derive(Debug, Clone, Copy)]
pub(super) enum Transfer {
    /// `(u', v') = M (u, v) + c`.
    Map([[f64; 2]; 2], [f64; 2]),
    /// The pcurves are in a NURBS form's parameters and are omitted.
    Omit,
}

const IDENTITY: Transfer = Transfer::Map([[1.0, 0.0], [0.0, 1.0]], [0.0, 0.0]);

/// The neutral carrier of a face.
pub(super) struct SurfaceCarrier {
    /// The carrier.
    pub(super) geometry: SolvedSurfaceGeometry,
    /// How the pcurves transfer.
    pub(super) transfer: Transfer,
    /// Whether the carrier's normal is the face's natural normal.
    pub(super) agrees: bool,
    /// Whether the face is a blend whose definition the carrier drops.
    pub(super) blend: bool,
}

fn nurbs_surface(surface: &Surface) -> Result<SolvedSurfaceGeometry, Refusal> {
    let (u_knots, v_knots, rows, weights) = match surface {
        Surface::Spline {
            u_knots,
            v_knots,
            points,
        } => (
            u_knots.clone(),
            v_knots.clone(),
            points
                .iter()
                .map(|row| row.iter().copied().map(point).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            None,
        ),
        Surface::RationalSpline {
            u_knots,
            v_knots,
            homogeneous,
        } => {
            let mut rows = Vec::with_capacity(homogeneous.len());
            let mut weights = Vec::with_capacity(homogeneous.len());
            for row in homogeneous {
                let mut points = Vec::with_capacity(row.len());
                let mut lane = Vec::with_capacity(row.len());
                for pole in row {
                    if pole[3] == 0.0 {
                        return Err("a rational spline pole has weight zero");
                    }
                    points.push(Point3::new(
                        pole[0] / pole[3],
                        pole[1] / pole[3],
                        pole[2] / pole[3],
                    ));
                    lane.push(pole[3]);
                }
                rows.push(points);
                weights.push(lane);
            }
            (u_knots.clone(), v_knots.clone(), rows, Some(weights))
        }
        _ => return Err("the surface is not a spline"),
    };
    let columns = rows.first().map_or(0, Vec::len);
    let degree = |knots: &[f64], count: usize| {
        knots
            .len()
            .checked_sub(count + 1)
            .and_then(|degree| u32::try_from(degree).ok())
            .ok_or("a spline surface has more control points than its knots allow")
    };
    let u = NurbsSurfaceAxis::new(degree(&u_knots, rows.len())?, u_knots, false);
    let v = NurbsSurfaceAxis::new(degree(&v_knots, columns)?, v_knots, false);
    let poles = NurbsPoleGrid::from_lanes(rows, weights)
        .map_err(|_| "a spline surface pole is not admitted")?;
    NurbsSurface::new(u, v, poles, false)
        .map(SolvedSurfaceGeometry::Nurbs)
        .map_err(|_| "a spline surface is not an admitted NURBS surface")
}

/// The neutral natural normal of an analytic carrier at `at`, a point on it.
fn analytic_normal(surface: &Surface, at: V3) -> Option<V3> {
    match surface {
        Surface::Cylinder { frame, .. } => {
            let axis = unit(frame_axis(frame))?;
            let offset = sub(at, frame.origin);
            Some(sub(offset, scale(axis, dot(offset, axis))))
        }
        Surface::Cone {
            frame, half_angle, ..
        } => {
            let axis = unit(frame_axis(frame))?;
            let offset = sub(at, frame.origin);
            let radial = unit(sub(offset, scale(axis, dot(offset, axis))))?;
            Some(sub(radial, scale(axis, half_angle.portable_tan())))
        }
        Surface::Sphere { frame, .. } => Some(sub(at, frame.origin)),
        Surface::Torus {
            frame,
            major_radius,
            ..
        } => {
            let axis = unit(frame_axis(frame))?;
            let offset = sub(at, frame.origin);
            let radial = unit(sub(offset, scale(axis, dot(offset, axis))))?;
            Some(sub(offset, scale(radial, *major_radius)))
        }
        _ => None,
    }
}

/// Whether the natural normal of `form` agrees with the analytic carrier's,
/// voted at the centre and the quarter points of `domain`.
fn form_agrees(surface: &Surface, form: &Surface, domain: [[f64; 2]; 2]) -> Option<bool> {
    let net = HomogeneousNet::of(form)?;
    let steps = [
        DIFFERENCE_STEP * (domain[0][1] - domain[0][0]).abs().max(1.0),
        DIFFERENCE_STEP * (domain[1][1] - domain[1][0]).abs().max(1.0),
    ];
    let mut votes = 0_i32;
    for [a, b] in [
        [0.5, 0.5],
        [0.25, 0.25],
        [0.75, 0.25],
        [0.25, 0.75],
        [0.75, 0.75],
    ] {
        let u = domain[0][0] + a * (domain[0][1] - domain[0][0]);
        let v = domain[1][0] + b * (domain[1][1] - domain[1][0]);
        let Some((at, normal)) = net.normal(u, v, steps) else {
            continue;
        };
        let Some(analytic) = analytic_normal(surface, at) else {
            continue;
        };
        let agreement = dot(normal, analytic);
        if agreement > 0.0 {
            votes += 1;
        } else if agreement < 0.0 {
            votes -= 1;
        }
    }
    (votes != 0).then_some(votes > 0)
}

fn plane(origin: V3, u_axis: V3, v_axis: V3) -> Result<SurfaceCarrier, Refusal> {
    let normal = unit(cross(u_axis, v_axis)).ok_or("a plane's axes are parallel")?;
    let reference = unit(u_axis).ok_or("a plane's u axis is zero")?;
    let transverse = cross(normal, reference);
    let geometry = SolvedSurfaceGeometry::Plane(PlaneSurface::try_new(
        point(origin),
        vector(normal),
        vector(reference),
    )?);
    let map = [
        [dot(u_axis, reference), dot(v_axis, reference)],
        [dot(u_axis, transverse), dot(v_axis, transverse)],
    ];
    Ok(SurfaceCarrier {
        geometry,
        transfer: Transfer::Map(map, [0.0, 0.0]),
        agrees: true,
        blend: false,
    })
}

/// The neutral carrier of `face`.
pub(super) fn face_carrier(face: &Face) -> Result<SurfaceCarrier, Refusal> {
    let analytic = |geometry| SurfaceCarrier {
        geometry,
        transfer: IDENTITY,
        agrees: true,
        blend: false,
    };
    let mut carrier = match &face.surface {
        Surface::Plane {
            origin,
            u_axis,
            v_axis,
        } => plane(*origin, *u_axis, *v_axis)?,
        Surface::PlaneThrough {
            origin,
            u_point,
            v_point,
        } => plane(*origin, sub(*u_point, *origin), sub(*v_point, *origin))?,
        Surface::Cylinder { frame, radius } => {
            analytic(SolvedSurfaceGeometry::Cylinder(CylinderSurface::try_new(
                point(frame.origin),
                vector(frame_axis(frame)),
                vector(frame.x_axis),
                *radius,
            )?))
        }
        Surface::Cone {
            frame,
            radius_at_origin,
            half_angle,
        } => analytic(SolvedSurfaceGeometry::Cone(ConeSurface::try_new(
            point(frame.origin),
            vector(frame_axis(frame)),
            vector(frame.x_axis),
            *radius_at_origin,
            1.0,
            *half_angle,
        )?)),
        Surface::Sphere { frame, radius } => {
            analytic(SolvedSurfaceGeometry::Sphere(SphereSurface::try_new(
                point(frame.origin),
                vector(frame_axis(frame)),
                vector(frame.x_axis),
                *radius,
            )?))
        }
        Surface::Torus {
            frame,
            major_radius,
            minor_radius,
            tube_start,
        } => SurfaceCarrier {
            transfer: Transfer::Map([[1.0, 0.0], [0.0, 1.0]], [0.0, *tube_start]),
            ..analytic(SolvedSurfaceGeometry::Torus(TorusSurface::try_new(
                point(frame.origin),
                vector(frame_axis(frame)),
                vector(frame.x_axis),
                *major_radius,
                *minor_radius,
            )?))
        },
        Surface::Spline { .. } | Surface::RationalSpline { .. } => {
            analytic(nurbs_surface(&face.surface)?)
        }
        Surface::BallPatch { .. } | Surface::Blend { .. } | Surface::PlacedRevolution { .. } => {
            let form = face
                .nurbs
                .as_ref()
                .ok_or("the surface has no neutral form and the face carries no NURBS form")?;
            return Ok(SurfaceCarrier {
                blend: matches!(face.surface, Surface::Blend { .. }),
                ..analytic(nurbs_surface(form)?)
            });
        }
    };
    if let Some(form) = &face.nurbs {
        carrier.transfer = Transfer::Omit;
        carrier.agrees = form_agrees(&face.surface, form, face.domain)
            .ok_or("the face's NURBS form gives no normal to compare")?;
    }
    Ok(carrier)
}

// ---------------------------------------------------------------------------
// Pcurves.

/// The neutral pcurve of `pcurve` through `map` and `offset`.
pub(super) fn pcurve(
    pcurve: &Curve2,
    map: [[f64; 2]; 2],
    offset: [f64; 2],
) -> Result<PcurveGeometry, Refusal> {
    let mapped = |u: f64, v: f64| {
        Point2::new(
            map[0][0] * u + map[0][1] * v + offset[0],
            map[1][0] * u + map[1][1] * v + offset[1],
        )
    };
    let (knots, points, weights) = match pcurve {
        Curve2::Spline { knots, points } => (
            knots.clone(),
            points
                .iter()
                .map(|p| mapped(p[0], p[1]))
                .collect::<Vec<_>>(),
            None,
        ),
        Curve2::RationalSpline { knots, homogeneous } => {
            let mut points = Vec::with_capacity(homogeneous.len());
            let mut weights = Vec::with_capacity(homogeneous.len());
            for row in homogeneous {
                if row[2] == 0.0 {
                    return Err("a rational pcurve pole has weight zero");
                }
                points.push(mapped(row[0] / row[2], row[1] / row[2]));
                weights.push(row[2]);
            }
            (knots.clone(), points, Some(weights))
        }
    };
    let degree = knots
        .len()
        .checked_sub(points.len() + 1)
        .and_then(|degree| u32::try_from(degree).ok())
        .ok_or("a pcurve has more control points than its knots allow")?;
    let poles = PcurveNurbsPoles::from_lanes(points, weights)
        .map_err(|_| "a pcurve pole is not admitted")?;
    PcurveNurbs::new(degree, knots, poles, false)
        .map(|nurbs| PcurveGeometry::Nurbs { nurbs })
        .map_err(|_| "a pcurve is not an admitted NURBS curve")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quarter_arc_form_evaluates_on_its_circle() {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let form = Homogeneous3 {
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            rows: vec![[1.0, 0.0, 0.0, 1.0], [s, s, 0.0, s], [0.0, 1.0, 0.0, 1.0]],
        };
        let middle = form.at(0.5).unwrap();
        assert!((norm(middle) - 1.0).abs() < 1e-12);
        assert!((middle[0] - middle[1]).abs() < 1e-12);
    }

    #[test]
    fn a_turn_against_the_axis_reverses_it() {
        let axis = [0.0, 0.0, 1.0];
        let reference = [1.0, 0.0, 0.0];
        let s = std::f64::consts::FRAC_1_SQRT_2;
        // From +x through -y to -x: clockwise about +z.
        let plane = Turn {
            axis,
            reference,
            centre: [0.0; 3],
            radii: [1.0; 2],
        };
        let (turned, sweep) = plane
            .through(0.0, [s, -s, 0.0], [-1.0, 0.0, 0.0], false)
            .unwrap();
        assert_eq!(turned, [0.0, 0.0, -1.0]);
        assert!((sweep - std::f64::consts::PI).abs() < 1e-12);
        // From +x through +y to -y: three quarters counterclockwise.
        let (turned, sweep) = plane
            .through(0.0, [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], false)
            .unwrap();
        assert_eq!(turned, axis);
        assert!((sweep - 1.5 * std::f64::consts::PI).abs() < 1e-12);
    }

    #[test]
    fn plane_axes_map_pcurves_onto_the_orthonormal_frame() {
        let carrier = plane([0.0; 3], [2.0, 0.0, 0.0], [0.0, 3.0, 0.0]).unwrap();
        let Transfer::Map(map, offset) = carrier.transfer else {
            panic!("a plane maps its pcurves");
        };
        assert_eq!(map, [[2.0, 0.0], [0.0, 3.0]]);
        assert_eq!(offset, [0.0, 0.0]);
    }
}
