// SPDX-License-Identifier: Apache-2.0
//! The NURBS form of B-rep records: one exact rational NURBS surface for
//! each face, one NURBS curve for each edge, and one parameter-space NURBS
//! curve for each oriented edge when each pcurve has an image (spec §17.3).
//!
//! A face's surface covers the parameter box of the face. Angular parameters
//! map to arc parameters whose knots are angles, so an arc parameter equals
//! its angle at every knot; length parameters map unchanged.

use super::{
    AnalyticCurve, AnalyticPcurve, AnalyticSurface, Brep, Catalog, CurveKind, Form,
    NurbsCurveRecord, NurbsSurfaceRecord, PcurveKind, Reference, SurfaceKind,
};

/// The largest angle one rational quadratic arc segment spans.
const MAX_ARC_SEGMENT: f64 = std::f64::consts::FRAC_PI_2;
/// Relative slack of a parameter box over the parameters it holds.
const BOX_MARGIN: f64 = 1e-6;
/// Relative difference within which an arc sweep is a full turn.
const FULL_TURN: f64 = 1e-5;
/// The fewest points at which a pcurve is checked against its edge curve.
const ON_EDGE_SAMPLES: usize = 16;
/// The position of each check point within its span of a pcurve, away from
/// the span ends, where a polygon of points on the edge meets the edge.
const ON_EDGE_OFFSET: f64 = 0.43;
/// Gauss-Newton steps of a point projection onto a NURBS surface.
const PROJECTION_STEPS: usize = 24;
/// Step of a surface difference quotient, as a share of the domain.
const SURFACE_STEP: f64 = 1e-7;
/// Relative parameter change at which a projection has converged.
const PROJECTION_CONVERGED: f64 = 1e-14;

type V3 = [f64; 3];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scaled(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
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

fn finite<const N: usize>(point: [f64; N]) -> bool {
    point.iter().all(|value| value.is_finite())
}

/// The parameter map of one surface direction from a record parameter to
/// the NURBS parameter.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ParamMap {
    /// The NURBS parameter is the record parameter.
    Identity,
    /// An elliptic arc: segments `[knots[k], knots[k + 1]]` of equal sweep,
    /// each a rational quadratic arc.
    Angle { knots: Vec<f64> },
    /// A hyperbolic arc on one segment `[t0, t1]`.
    Hyperbolic { start: f64, end: f64 },
}

impl ParamMap {
    /// The NURBS parameter of record parameter `t`.
    pub(crate) fn apply(&self, t: f64) -> f64 {
        match self {
            Self::Identity => t,
            Self::Angle { knots } => {
                let Some(segment) = knots
                    .windows(2)
                    .position(|pair| t <= pair[1])
                    .or_else(|| knots.len().checked_sub(2))
                else {
                    return t;
                };
                let (start, end) = (knots[segment], knots[segment + 1]);
                if t == start || t == end {
                    return t;
                }
                let half = (end - start) / 2.0;
                let tau = ((t - start) / 2.0).tan();
                let local = tau / (half.sin() + tau * (1.0 - half.cos()));
                within(start + (end - start) * local, start, end, t)
            }
            Self::Hyperbolic { start, end } => {
                if t == *start || t == *end {
                    return t;
                }
                let half = (end - start) / 2.0;
                let tau = ((t - start) / 2.0).tanh();
                let local = tau / (half.sinh() - tau * (half.cosh() - 1.0));
                within(start + (end - start) * local, *start, *end, t)
            }
        }
    }
}

/// `value` clamped to the segment `[start, end]` when the record parameter
/// `t` lies in it.
fn within(value: f64, start: f64, end: f64, t: f64) -> f64 {
    if (start..=end).contains(&t) {
        value.clamp(start, end)
    } else {
        value
    }
}

/// The knots of an arc over `[start, end]`: equal segments of at most
/// [`MAX_ARC_SEGMENT`].
fn arc_knots(start: f64, end: f64) -> Vec<f64> {
    let sweep = end - start;
    let count = (sweep / MAX_ARC_SEGMENT).ceil().max(1.0);
    let segments = cadmpeg_core::convert::truncate_f64_to_usize(count).unwrap_or(1);
    let step = sweep / count;
    (0..=segments)
        .map(|index| {
            if index == segments {
                end
            } else {
                start + step * cadmpeg_core::convert::f64_from_index(index).unwrap_or(0.0)
            }
        })
        .collect()
}

/// The clamped knot vector of a quadratic B-spline whose segments end at
/// `breaks`.
fn quadratic_knots(breaks: &[f64]) -> Vec<f64> {
    let mut knots = Vec::new();
    for (index, value) in breaks.iter().enumerate() {
        let multiplicity = if index == 0 || index + 1 == breaks.len() {
            3
        } else {
            2
        };
        knots.extend(std::iter::repeat_n(*value, multiplicity));
    }
    knots
}

/// The rational quadratic arc `c + cos θ·ex + sin θ·ey` for `θ` over
/// `[start, end]`, on knots that are angles.
fn elliptic_arc<const N: usize>(
    center: [f64; N],
    ex: [f64; N],
    ey: [f64; N],
    start: f64,
    end: f64,
) -> (NurbsCurveRecord<N>, ParamMap) {
    let breaks = arc_knots(start, end);
    let at = |theta: f64, factor: f64| -> [f64; N] {
        std::array::from_fn(|axis| {
            center[axis] + factor * (theta.cos() * ex[axis] + theta.sin() * ey[axis])
        })
    };
    let mut points = vec![at(start, 1.0)];
    let mut weights = vec![1.0];
    for pair in breaks.windows(2) {
        let half = (pair[1] - pair[0]) / 2.0;
        points.push(at(pair[0] + half, 1.0 / half.cos()));
        weights.push(half.cos());
        points.push(at(pair[1], 1.0));
        weights.push(1.0);
    }
    let form = if (end - start - std::f64::consts::TAU).abs() <= FULL_TURN * std::f64::consts::TAU {
        Form::Closed
    } else {
        Form::Open
    };
    (
        NurbsCurveRecord {
            order: 3,
            points,
            weights,
            knots: quadratic_knots(&breaks),
            form,
        },
        ParamMap::Angle { knots: breaks },
    )
}

/// The rational quadratic arc `c + cosh t·ex + sinh t·ey` over `[start, end]`.
fn hyperbolic_arc(
    center: V3,
    ex: V3,
    ey: V3,
    start: f64,
    end: f64,
) -> (NurbsCurveRecord<3>, ParamMap) {
    let at = |t: f64, factor: f64| {
        add(
            center,
            scaled(add(scaled(ex, t.cosh()), scaled(ey, t.sinh())), factor),
        )
    };
    let half = (end - start) / 2.0;
    (
        NurbsCurveRecord {
            order: 3,
            points: vec![
                at(start, 1.0),
                at(start + half, 1.0 / half.cosh()),
                at(end, 1.0),
            ],
            weights: vec![1.0, half.cosh(), 1.0],
            knots: vec![start, start, start, end, end, end],
            form: Form::Open,
        },
        ParamMap::Hyperbolic { start, end },
    )
}

/// The degree-1 curve from `a` to `b` on `[start, end]`.
fn line<const N: usize>(a: [f64; N], b: [f64; N], start: f64, end: f64) -> NurbsCurveRecord<N> {
    NurbsCurveRecord {
        order: 2,
        points: vec![a, b],
        weights: vec![1.0, 1.0],
        knots: vec![start, start, end, end],
        form: Form::Open,
    }
}

/// The frame of an analytic record: origin, `X`, `Y`, `Z`.
fn axes(frame: &super::Frame) -> (V3, V3, V3, V3) {
    let z = frame.axis;
    let x = frame.ref_direction;
    (frame.position, x, cross(z, x), z)
}

/// The NURBS form of analytic curve `curve` over `[start, end]`, with the
/// map from its parameter.
fn analytic_curve(
    curve: &AnalyticCurve,
    start: f64,
    end: f64,
) -> Result<(NurbsCurveRecord<3>, ParamMap), String> {
    let (origin, x_axis, y_axis, z_axis) = axes(&curve.frame);
    let param = |index: usize| {
        curve
            .params
            .get(index)
            .copied()
            .ok_or_else(|| format!("a {} curve has no parameter {index}", curve.kind.token()))
    };
    Ok(match curve.kind {
        CurveKind::Line => (
            line(
                add(origin, scaled(z_axis, start)),
                add(origin, scaled(z_axis, end)),
                start,
                end,
            ),
            ParamMap::Identity,
        ),
        CurveKind::Circle => {
            let radius = param(0)?;
            elliptic_arc(
                origin,
                scaled(x_axis, radius),
                scaled(y_axis, radius),
                start,
                end,
            )
        }
        CurveKind::Ellipse => elliptic_arc(
            origin,
            scaled(x_axis, param(0)?),
            scaled(y_axis, param(1)?),
            start,
            end,
        ),
        CurveKind::Parabola => {
            let focal = param(0)?;
            let at = |t: f64| {
                add(
                    origin,
                    add(
                        scaled(x_axis, focal * t * t),
                        scaled(y_axis, 2.0 * focal * t),
                    ),
                )
            };
            let tangent = add(
                scaled(x_axis, 2.0 * focal * start),
                scaled(y_axis, 2.0 * focal),
            );
            let middle = add(at(start), scaled(tangent, (end - start) / 2.0));
            (
                NurbsCurveRecord {
                    order: 3,
                    points: vec![at(start), middle, at(end)],
                    weights: vec![1.0, 1.0, 1.0],
                    knots: vec![start, start, start, end, end, end],
                    form: Form::Open,
                },
                ParamMap::Identity,
            )
        }
        CurveKind::Hyperbola => hyperbolic_arc(
            origin,
            scaled(x_axis, param(0)?),
            scaled(y_axis, param(1)?),
            start,
            end,
        ),
    })
}

/// The NURBS form of the curve `reference` over `[start, end]`.
fn curve_nurbs(
    brep: &Brep,
    reference: Reference,
    start: f64,
    end: f64,
) -> Result<(NurbsCurveRecord<3>, ParamMap), String> {
    match reference {
        (Catalog::Analytic, index) => {
            let curve = brep
                .analytic_curves
                .get(index)
                .ok_or_else(|| format!("no analytic curve {index}"))?;
            analytic_curve(curve, start, end)
        }
        (Catalog::Nurbs, index) => brep
            .nurbs_curves
            .get(index)
            .cloned()
            .map(|curve| (curve, ParamMap::Identity))
            .ok_or_else(|| format!("no NURBS curve {index}")),
    }
}

/// The curve with its control points in the opposite order, on the
/// reflected knot vector.
pub(crate) fn reversed<const N: usize>(curve: &NurbsCurveRecord<N>) -> NurbsCurveRecord<N> {
    let first = curve.knots.first().copied().unwrap_or(0.0);
    let last = curve.knots.last().copied().unwrap_or(0.0);
    NurbsCurveRecord {
        order: curve.order,
        points: curve.points.iter().rev().copied().collect(),
        weights: curve.weights.iter().rev().copied().collect(),
        knots: curve
            .knots
            .iter()
            .rev()
            .map(|knot| first + last - knot)
            .collect(),
        form: curve.form,
    }
}

/// The parameter box of a face's surface: `[u, v]` ranges.
type ParamBox = [[f64; 2]; 2];

fn widen(range: &mut [f64; 2], value: f64) {
    range[0] = range[0].min(value);
    range[1] = range[1].max(value);
}

const EMPTY: [f64; 2] = [f64::INFINITY, f64::NEG_INFINITY];

/// The control points of the pcurves of `face`, or `None` when an oriented
/// edge of the face has none.
fn pcurve_points(brep: &Brep, face: usize) -> Option<Vec<[f64; 2]>> {
    let mut out = Vec::new();
    for loop_ in brep
        .faces
        .get(face)?
        .loops
        .iter()
        .filter_map(|index| brep.loops.get(*index))
    {
        for oriented in loop_
            .oriented_edges
            .iter()
            .filter_map(|index| brep.oriented_edges.get(*index))
        {
            match oriented.pcurve? {
                (Catalog::Nurbs, index) => out.extend(brep.nurbs_pcurves.get(index)?.points.iter()),
                (Catalog::Analytic, index) => {
                    let record = brep.analytic_pcurves.get(index)?;
                    let (curve, _) = analytic_pcurve(record).ok()?;
                    out.extend(curve.points);
                }
            }
        }
    }
    Some(out)
}

/// The control points of the exact pcurves of `face` on a surface of
/// revolution, in record parameters, or `None` when an oriented edge has no
/// record pcurve to choose its angle branches.
fn exact_points(
    brep: &Brep,
    face: usize,
    surface: &AnalyticSurface,
    converted: &Brep,
    tolerance: f64,
) -> Option<Vec<[f64; 2]>> {
    if !matches!(
        surface.kind,
        SurfaceKind::Cylinder | SurfaceKind::Cone | SurfaceKind::Sphere | SurfaceKind::Torus
    ) {
        return None;
    }
    let identity = [ParamMap::Identity, ParamMap::Identity];
    let mut out = Vec::new();
    for loop_ in brep
        .faces
        .get(face)?
        .loops
        .iter()
        .filter_map(|index| brep.loops.get(*index))
    {
        for oriented in loop_
            .oriented_edges
            .iter()
            .filter_map(|index| brep.oriented_edges.get(*index))
        {
            let guide = match oriented.pcurve? {
                (Catalog::Nurbs, index) => brep.nurbs_pcurves.get(index)?.clone(),
                (Catalog::Analytic, index) => {
                    analytic_pcurve(brep.analytic_pcurves.get(index)?).ok()?.0
                }
            };
            let traced = projected_pcurve(
                surface,
                &guide,
                converted.nurbs_curves.get(oriented.edge)?,
                converted.edges.get(oriented.edge)?.range,
                oriented.orientation,
                &identity,
                tolerance,
            )?;
            out.extend(traced.points);
        }
        if let Some(vertex) = loop_.vertex.and_then(|vertex| brep.vertices.get(vertex)) {
            out.push(inverse(surface, *vertex, [0.0, 0.0])?);
        }
    }
    Some(out)
}

/// The control points of the edge curves of `face`, in 3D.
fn edge_points(brep: &Brep, face: usize, edges: &[NurbsCurveRecord<3>]) -> Vec<V3> {
    let mut out = Vec::new();
    let Some(face) = brep.faces.get(face) else {
        return out;
    };
    for loop_ in face.loops.iter().filter_map(|index| brep.loops.get(*index)) {
        for oriented in loop_
            .oriented_edges
            .iter()
            .filter_map(|index| brep.oriented_edges.get(*index))
        {
            if let Some(curve) = edges.get(oriented.edge) {
                out.extend(curve.points.iter());
            }
        }
        if let Some(vertex) = loop_.vertex.and_then(|vertex| brep.vertices.get(vertex)) {
            out.push(*vertex);
        }
    }
    out
}

/// The parameter range of `values`, widened by [`BOX_MARGIN`].
fn span(values: impl Iterator<Item = f64>) -> Option<[f64; 2]> {
    let mut range = EMPTY;
    for value in values {
        widen(&mut range, value);
    }
    if !(range[0].is_finite() && range[1].is_finite()) {
        return None;
    }
    let slack = BOX_MARGIN * (range[1] - range[0]).abs().max(range[0].abs()).max(1.0);
    Some([range[0] - slack, range[1] + slack])
}

/// The analytic pcurve as a rational quadratic arc in the direction of its
/// range, with the map from its parameter.
fn analytic_pcurve(record: &AnalyticPcurve) -> Result<(NurbsCurveRecord<2>, ParamMap), String> {
    let x = record.ref_direction;
    let y = [-x[1], x[0]];
    let (a, b) = match record.kind {
        PcurveKind::Circle => {
            let r = *record
                .params
                .first()
                .ok_or("a circle pcurve has no radius")?;
            (r, r)
        }
        PcurveKind::Ellipse => (
            *record
                .params
                .first()
                .ok_or("an ellipse pcurve has no axes")?,
            *record
                .params
                .get(1)
                .ok_or("an ellipse pcurve has no minor axis")?,
        ),
    };
    let [t0, t1] = record.range;
    let ex = [x[0] * a, x[1] * a];
    let ey = [y[0] * b, y[1] * b];
    if t1 >= t0 {
        Ok(elliptic_arc(record.position, ex, ey, t0, t1))
    } else {
        let (curve, map) = elliptic_arc(record.position, ex, ey, t1, t0);
        Ok((reversed(&curve), map))
    }
}

/// One converted surface: its NURBS record and the map of each direction.
struct Surface {
    record: NurbsSurfaceRecord,
    maps: [ParamMap; 2],
}

/// The surface swept by rotating `profile` about the axis through `origin`
/// along `axis` over angles `[start, end]`. The rotation is the U direction.
fn revolved(
    profile: &NurbsCurveRecord<3>,
    origin: V3,
    axis: V3,
    start: f64,
    end: f64,
) -> (NurbsSurfaceRecord, ParamMap) {
    let breaks = arc_knots(start, end);
    let mut angles = vec![(start, 1.0)];
    for pair in breaks.windows(2) {
        let half = (pair[1] - pair[0]) / 2.0;
        angles.push((pair[0] + half, half.cos()));
        angles.push((pair[1], 1.0));
    }
    let mut points = Vec::new();
    let mut weights = Vec::new();
    for (angle, arc_weight) in &angles {
        let factor = 1.0 / arc_weight;
        for (point, weight) in profile.points.iter().zip(&profile.weights) {
            let offset = sub(*point, origin);
            let height = dot(offset, axis);
            let radial = sub(offset, scaled(axis, height));
            let normal = cross(axis, radial);
            let rotated = add(scaled(radial, angle.cos()), scaled(normal, angle.sin()));
            points.push(add(
                add(origin, scaled(axis, height)),
                scaled(rotated, factor),
            ));
            weights.push(arc_weight * weight);
        }
    }
    let closed = (end - start - std::f64::consts::TAU).abs() <= FULL_TURN * std::f64::consts::TAU;
    (
        NurbsSurfaceRecord {
            u_order: 3,
            v_order: profile.order,
            u_count: angles.len(),
            v_count: profile.points.len(),
            u_form: if closed { Form::Closed } else { Form::Open },
            v_form: profile.form,
            points,
            weights,
            u_knots: quadratic_knots(&breaks),
            v_knots: profile.knots.clone(),
        },
        ParamMap::Angle { knots: breaks },
    )
}

/// The surface that translates `profile` along `direction` over `[start, end]`.
/// The profile is the U direction.
fn extruded(
    profile: &NurbsCurveRecord<3>,
    direction: V3,
    start: f64,
    end: f64,
) -> NurbsSurfaceRecord {
    let mut points = Vec::new();
    let mut weights = Vec::new();
    for (point, weight) in profile.points.iter().zip(&profile.weights) {
        points.push(add(*point, scaled(direction, start)));
        points.push(add(*point, scaled(direction, end)));
        weights.push(*weight);
        weights.push(*weight);
    }
    NurbsSurfaceRecord {
        u_order: profile.order,
        v_order: 2,
        u_count: profile.points.len(),
        v_count: 2,
        u_form: profile.form,
        v_form: Form::Open,
        points,
        weights,
        u_knots: profile.knots.clone(),
        v_knots: vec![start, start, end, end],
    }
}

/// The parameter box of `face` on `surface`, from its pcurves when every
/// oriented edge has one, and otherwise from the surface's natural range and
/// the edge curves.
fn face_box(
    brep: &Brep,
    face: usize,
    surface: &AnalyticSurface,
    converted: &Brep,
    tolerance: f64,
) -> Result<ParamBox, String> {
    let (origin, x_axis, y_axis, z_axis) = axes(&surface.frame);
    let points = edge_points(brep, face, &converted.nurbs_curves);
    let projected = |direction: V3| {
        span(
            points
                .iter()
                .map(|point| dot(sub(*point, origin), direction)),
        )
    };
    let pcurves = exact_points(brep, face, surface, converted, tolerance)
        .or_else(|| pcurve_points(brep, face));
    let from_pcurves = |axis: usize| {
        pcurves
            .as_ref()
            .and_then(|points| span(points.iter().map(|point| point[axis])))
    };
    let full = [0.0, std::f64::consts::TAU];
    let latitude = [-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2];
    let missing = || format!("face {face} has no parameter range");
    Ok(match surface.kind {
        SurfaceKind::Plane => [
            projected(x_axis).ok_or_else(missing)?,
            projected(y_axis).ok_or_else(missing)?,
        ],
        SurfaceKind::Cylinder | SurfaceKind::Cone => [
            from_pcurves(0).unwrap_or(full),
            from_pcurves(1)
                .or_else(|| projected(z_axis))
                .ok_or_else(missing)?,
        ],
        SurfaceKind::Sphere => {
            let mut v_range = from_pcurves(1).unwrap_or(latitude);
            v_range = [v_range[0].max(latitude[0]), v_range[1].min(latitude[1])];
            [from_pcurves(0).unwrap_or(full), v_range]
        }
        SurfaceKind::Torus => [
            from_pcurves(0).unwrap_or(full),
            from_pcurves(1).unwrap_or(full),
        ],
        SurfaceKind::Extrusion => {
            let profile = surface.profile.ok_or("an extrusion has no profile curve")?;
            let profile_points: Vec<V3> = match profile {
                (Catalog::Nurbs, index) => brep
                    .nurbs_curves
                    .get(index)
                    .map(|curve| curve.points.clone())
                    .unwrap_or_default(),
                (Catalog::Analytic, _) => Vec::new(),
            };
            let along = |direction: V3| {
                profile_points
                    .iter()
                    .map(move |point| dot(*point, direction))
            };
            let v_range = from_pcurves(1).or_else(|| {
                let edge = span(points.iter().map(|point| dot(*point, z_axis)))?;
                let base = span(along(z_axis))?;
                Some([edge[0] - base[1], edge[1] - base[0]])
            });
            [
                from_pcurves(0)
                    .or_else(|| profile_domain(brep, profile))
                    .ok_or_else(missing)?,
                v_range.ok_or_else(missing)?,
            ]
        }
        SurfaceKind::Revolution => {
            let profile = surface
                .profile
                .ok_or("a surface of revolution has no profile curve")?;
            [
                from_pcurves(0).unwrap_or(full),
                from_pcurves(1)
                    .or_else(|| profile_domain(brep, profile))
                    .ok_or_else(missing)?,
            ]
        }
    })
}

/// The parameter domain of a NURBS profile curve.
fn profile_domain(brep: &Brep, profile: Reference) -> Option<[f64; 2]> {
    let (Catalog::Nurbs, index) = profile else {
        return None;
    };
    let curve = brep.nurbs_curves.get(index)?;
    Some([
        *curve.knots.get(curve.order - 1)?,
        *curve.knots.get(curve.points.len())?,
    ])
}

/// The NURBS surface of `face`, whose carrier is `surface`, over its
/// parameter box.
fn analytic_surface(
    brep: &Brep,
    face: usize,
    surface: &AnalyticSurface,
    converted: &Brep,
    tolerance: f64,
) -> Result<Surface, String> {
    let (origin, x_axis, y_axis, z_axis) = axes(&surface.frame);
    let [u_range, v_range] = face_box(brep, face, surface, converted, tolerance)?;
    let param = |index: usize| {
        surface.params.get(index).copied().ok_or_else(|| {
            format!(
                "a {} surface has no parameter {index}",
                surface.kind.token()
            )
        })
    };
    let profile_curve = |range: [f64; 2]| -> Result<(NurbsCurveRecord<3>, ParamMap), String> {
        let profile = surface
            .profile
            .ok_or_else(|| format!("a {} surface has no profile curve", surface.kind.token()))?;
        curve_nurbs(brep, profile, range[0], range[1])
    };
    Ok(match surface.kind {
        SurfaceKind::Plane => {
            let corner = |a: f64, b: f64| add(origin, add(scaled(x_axis, a), scaled(y_axis, b)));
            Surface {
                record: NurbsSurfaceRecord {
                    u_order: 2,
                    v_order: 2,
                    u_count: 2,
                    v_count: 2,
                    u_form: Form::Open,
                    v_form: Form::Open,
                    points: vec![
                        corner(u_range[0], v_range[0]),
                        corner(u_range[0], v_range[1]),
                        corner(u_range[1], v_range[0]),
                        corner(u_range[1], v_range[1]),
                    ],
                    weights: vec![1.0; 4],
                    u_knots: vec![u_range[0], u_range[0], u_range[1], u_range[1]],
                    v_knots: vec![v_range[0], v_range[0], v_range[1], v_range[1]],
                },
                maps: [ParamMap::Identity, ParamMap::Identity],
            }
        }
        SurfaceKind::Cylinder | SurfaceKind::Cone => {
            let radius = param(0)?;
            let slope = if surface.kind == SurfaceKind::Cone {
                param(1)?.tan()
            } else {
                0.0
            };
            let at = |height: f64| {
                add(
                    origin,
                    add(
                        scaled(x_axis, radius + height * slope),
                        scaled(z_axis, height),
                    ),
                )
            };
            let profile = line(at(v_range[0]), at(v_range[1]), v_range[0], v_range[1]);
            let (record, map) = revolved(&profile, origin, z_axis, u_range[0], u_range[1]);
            Surface {
                record,
                maps: [map, ParamMap::Identity],
            }
        }
        SurfaceKind::Sphere => {
            let radius = param(0)?;
            let (profile, profile_map) = elliptic_arc(
                origin,
                scaled(x_axis, radius),
                scaled(z_axis, radius),
                v_range[0],
                v_range[1],
            );
            let (record, map) = revolved(&profile, origin, z_axis, u_range[0], u_range[1]);
            Surface {
                record,
                maps: [map, profile_map],
            }
        }
        SurfaceKind::Torus => {
            let (major, minor) = (param(0)?, param(1)?);
            let (profile, profile_map) = elliptic_arc(
                add(origin, scaled(x_axis, major)),
                scaled(x_axis, minor),
                scaled(z_axis, minor),
                v_range[0],
                v_range[1],
            );
            let (record, map) = revolved(&profile, origin, z_axis, u_range[0], u_range[1]);
            Surface {
                record,
                maps: [map, profile_map],
            }
        }
        SurfaceKind::Extrusion => {
            let (profile, map) = profile_curve(u_range)?;
            Surface {
                record: extruded(&profile, z_axis, v_range[0], v_range[1]),
                maps: [map, ParamMap::Identity],
            }
        }
        SurfaceKind::Revolution => {
            let (profile, profile_map) = profile_curve(v_range)?;
            let (record, map) = revolved(&profile, origin, z_axis, u_range[0], u_range[1]);
            Surface {
                record,
                maps: [map, profile_map],
            }
        }
    })
}

/// `record` as a curve of the NURBS helpers.
fn helper<const N: usize>(record: &NurbsCurveRecord<N>) -> Option<super::nurbs::Nurbs<N>> {
    Some(super::nurbs::Nurbs {
        degree: record.order.checked_sub(1)?,
        knots: record.knots.clone(),
        points: record.points.clone(),
        weights: record.weights.clone(),
    })
}

/// The point at `t` of a NURBS curve record.
pub(crate) fn evaluate<const N: usize>(curve: &NurbsCurveRecord<N>, t: f64) -> Option<[f64; N]> {
    helper(curve)?.point_at(t)
}

/// The part of a NURBS curve record on `[start, end]`.
fn segment<const N: usize>(
    curve: &NurbsCurveRecord<N>,
    start: f64,
    end: f64,
) -> Option<NurbsCurveRecord<N>> {
    let part = helper(curve)?.segment(start, end)?;
    Some(NurbsCurveRecord {
        order: part.degree + 1,
        points: part.points,
        weights: part.weights,
        knots: part.knots,
        form: Form::Open,
    })
}

/// The deepest bisection of a traced curve.
const MAX_TRACE_DEPTH: usize = 12;
/// Spans of a traced curve before bisection.
const FIRST_SPANS: usize = 4;
/// Step of a difference quotient, as a share of the span.
const DIFFERENCE_STEP: f64 = 1e-5;
/// The factor by which a traced cubic may lag or lead its image along the
/// image, over the deviation it may have across the image.
const ALONG_SLACK: f64 = 64.0;
/// Fractions of a span at which a traced cubic is compared with its image.
const CHECK_FRACTIONS: [f64; 3] = [0.25, 0.5, 0.75];

/// One span of a traced curve: its parameter interval and its cubic
/// Bezier control points.
type Span = ([f64; 2], [[f64; 2]; 4]);

fn bezier(points: &[[f64; 2]; 4], s: f64) -> [f64; 2] {
    let r = 1.0 - s;
    let basis = [r * r * r, 3.0 * r * r * s, 3.0 * r * s * s, s * s * s];
    std::array::from_fn(|axis| (0..4).map(|index| basis[index] * points[index][axis]).sum())
}

fn bezier_tangent(points: &[[f64; 2]; 4], s: f64) -> [f64; 2] {
    let r = 1.0 - s;
    let basis = [
        -3.0 * r * r,
        3.0 * r * r - 6.0 * r * s,
        6.0 * r * s - 3.0 * s * s,
        3.0 * s * s,
    ];
    std::array::from_fn(|axis| (0..4).map(|index| basis[index] * points[index][axis]).sum())
}

/// Appends the Hermite spans of `image` on `[a, b]` to `out`, bisected until
/// each is within `tolerance` of the image.
fn trace_span(
    image: &dyn Fn(f64) -> Option<[f64; 2]>,
    derivative: &dyn Fn(f64, f64) -> Option<[f64; 2]>,
    (a, b): (f64, f64),
    tolerance: f64,
    depth: usize,
    out: &mut Vec<Span>,
) -> Option<()> {
    let width = b - a;
    let (pa, pb) = (image(a)?, image(b)?);
    let (da, db) = (derivative(a, width)?, derivative(b, width)?);
    let points = [
        pa,
        [pa[0] + da[0] * width / 3.0, pa[1] + da[1] * width / 3.0],
        [pb[0] - db[0] * width / 3.0, pb[1] - db[1] * width / 3.0],
        pb,
    ];
    let mut deviation: f64 = 0.0;
    for fraction in CHECK_FRACTIONS {
        let exact = image(a + width * fraction)?;
        let fitted = bezier(&points, fraction);
        let tangent = bezier_tangent(&points, fraction);
        let offset = [exact[0] - fitted[0], exact[1] - fitted[1]];
        let speed = tangent[0].hypot(tangent[1]);
        let distance = offset[0].hypot(offset[1]);
        let across = if speed > 0.0 {
            (offset[0] * tangent[1] - offset[1] * tangent[0]).abs() / speed
        } else {
            distance
        };
        deviation = deviation.max(across).max(distance / ALONG_SLACK);
    }
    if depth < MAX_TRACE_DEPTH && deviation > tolerance {
        let middle = a + width / 2.0;
        trace_span(image, derivative, (a, middle), tolerance, depth + 1, out)?;
        trace_span(image, derivative, (middle, b), tolerance, depth + 1, out)
    } else {
        out.push(([a, b], points));
        Some(())
    }
}

/// The degree-3 curve, C1 at its knots, through `image(t)` for `t` over
/// `[start, end]`: Hermite spans whose tangents are difference quotients,
/// bisected until each span is within `tolerance` of the image across its
/// tangent and within [`ALONG_SLACK`] times that along it.
fn traced(
    image: &dyn Fn(f64) -> Option<[f64; 2]>,
    start: f64,
    end: f64,
    tolerance: f64,
) -> Option<NurbsCurveRecord<2>> {
    let derivative = |t: f64, width: f64| -> Option<[f64; 2]> {
        let step = width * DIFFERENCE_STEP;
        let (low, high) = ((t - step).max(start), (t + step).min(end));
        let (a, b) = (image(low)?, image(high)?);
        Some([(b[0] - a[0]) / (high - low), (b[1] - a[1]) / (high - low)])
    };
    let mut spans = Vec::new();
    let count = cadmpeg_core::convert::f64_from_index(FIRST_SPANS)?;
    for index in 0..FIRST_SPANS {
        let at = |k: usize| {
            start + (end - start) * cadmpeg_core::convert::f64_from_index(k).unwrap_or(0.0) / count
        };
        trace_span(
            image,
            &derivative,
            (at(index), at(index + 1)),
            tolerance,
            0,
            &mut spans,
        )?;
    }
    let mut points = vec![spans.first()?.1[0]];
    let mut knots = vec![start; 4];
    for (index, ([_, b], control)) in spans.iter().enumerate() {
        points.extend_from_slice(&control[1..]);
        let knot = if index + 1 == spans.len() { end } else { *b };
        knots.extend(std::iter::repeat_n(
            knot,
            if index + 1 == spans.len() { 4 } else { 3 },
        ));
    }
    Some(NurbsCurveRecord {
        order: 4,
        weights: vec![1.0; points.len()],
        points,
        knots,
        form: Form::Open,
    })
}

fn mapped_point(point: [f64; 2], maps: &[ParamMap; 2]) -> [f64; 2] {
    [maps[0].apply(point[0]), maps[1].apply(point[1])]
}

/// The image of a pcurve under the surface's parameter maps: exact when
/// each mapped coordinate is the same on all control points or the maps are
/// identities, the polygon of mapped control points for a degree-1 curve,
/// and otherwise a traced curve within `tolerance`.
fn mapped_pcurve(
    curve: &NurbsCurveRecord<2>,
    maps: &[ParamMap; 2],
    tolerance: f64,
) -> Option<NurbsCurveRecord<2>> {
    let exact = maps.iter().enumerate().all(|(axis, map)| {
        *map == ParamMap::Identity
            || curve
                .points
                .iter()
                .all(|point| point[axis] == curve.points[0][axis])
    });
    let out = if exact || curve.order == 2 {
        let mut out = curve.clone();
        for point in &mut out.points {
            *point = mapped_point(*point, maps);
        }
        out
    } else {
        let domain = [
            *curve.knots.get(curve.order - 1)?,
            *curve.knots.get(curve.points.len())?,
        ];
        traced(
            &|t| evaluate(curve, t).map(|point| mapped_point(point, maps)),
            domain[0],
            domain[1],
            tolerance,
        )?
    };
    out.points.iter().all(|point| finite(*point)).then_some(out)
}

/// The record parameters of the point on `surface` nearest `point`, with
/// each angle on the branch nearest the matching coordinate of `guide`.
/// The angle about the axis at a point on the axis is the guide's.
fn inverse(surface: &AnalyticSurface, point: V3, guide: [f64; 2]) -> Option<[f64; 2]> {
    let (origin, x_axis, y_axis, z_axis) = axes(&surface.frame);
    let d = sub(point, origin);
    let (dx, dy, dz) = (dot(d, x_axis), dot(d, y_axis), dot(d, z_axis));
    let radial = dx.hypot(dy);
    let branch = |angle: f64, near: f64| {
        let turns = ((near - angle) / std::f64::consts::TAU).round();
        angle + turns * std::f64::consts::TAU
    };
    let on_axis =
        radial <= surface.params.first().copied().unwrap_or(1.0).abs() * f64::EPSILON.sqrt();
    let u = if on_axis {
        guide[0]
    } else {
        branch(dy.atan2(dx), guide[0])
    };
    Some(match surface.kind {
        SurfaceKind::Cylinder | SurfaceKind::Cone => [u, dz],
        SurfaceKind::Sphere => [u, dz.atan2(radial)],
        SurfaceKind::Torus => {
            let major = *surface.params.first()?;
            [u, branch(dz.atan2(radial - major), guide[1])]
        }
        SurfaceKind::Plane | SurfaceKind::Extrusion | SurfaceKind::Revolution => return None,
    })
}

/// The pcurve of an oriented edge on an analytic surface of revolution: the
/// inverse image of its edge curve, with each angle on the branch of the
/// record pcurve `guide`, traced within `tolerance`.
fn projected_pcurve(
    surface: &AnalyticSurface,
    guide: &NurbsCurveRecord<2>,
    curve: &NurbsCurveRecord<3>,
    range: [f64; 2],
    forward: bool,
    maps: &[ParamMap; 2],
    tolerance: f64,
) -> Option<NurbsCurveRecord<2>> {
    let guide_domain = [
        *guide.knots.get(guide.order - 1)?,
        *guide.knots.get(guide.points.len())?,
    ];
    let image = |fraction: f64| {
        let t = if forward {
            range[0] + (range[1] - range[0]) * fraction
        } else {
            range[1] - (range[1] - range[0]) * fraction
        };
        let along = guide_domain[0] + (guide_domain[1] - guide_domain[0]) * fraction;
        let near = evaluate(guide, along)?;
        let point = evaluate(curve, t)?;
        inverse(surface, point, near).map(|uv| mapped_point(uv, maps))
    };
    traced(&image, 0.0, 1.0, tolerance)
}

/// The span index and the nonzero basis functions of degree `degree` at
/// `t` on `knots`, for `count` control points.
fn basis(knots: &[f64], degree: usize, count: usize, t: f64) -> Option<(usize, Vec<f64>)> {
    let low = *knots.get(degree)?;
    let high = *knots.get(count)?;
    let t = t.clamp(low, high);
    let span = (degree..count)
        .rev()
        .find(|index| knots[*index] <= t && knots[*index] < knots[*index + 1])
        .unwrap_or(degree);
    let zeros = || (0..=degree).map(|_| 0.0).collect::<Vec<f64>>();
    let mut values = zeros();
    values[0] = 1.0;
    let mut left = zeros();
    let mut right = zeros();
    for level in 1..=degree {
        left[level] = t - knots[span + 1 - level];
        right[level] = knots[span + level] - t;
        let mut saved = 0.0;
        for index in 0..level {
            let denominator = right[index + 1] + left[level - index];
            let term = if denominator == 0.0 {
                0.0
            } else {
                values[index] / denominator
            };
            values[index] = saved + right[index + 1] * term;
            saved = left[level - index] * term;
        }
        values[level] = saved;
    }
    Some((span, values))
}

/// The point at `(u, v)` of a NURBS surface record.
pub(crate) fn surface_point(surface: &NurbsSurfaceRecord, [u, v]: [f64; 2]) -> Option<V3> {
    let (p, q) = (
        surface.u_order.checked_sub(1)?,
        surface.v_order.checked_sub(1)?,
    );
    let (u_span, u_basis) = basis(&surface.u_knots, p, surface.u_count, u)?;
    let (v_span, v_basis) = basis(&surface.v_knots, q, surface.v_count, v)?;
    let mut sum = [0.0; 4];
    for (a, nu) in u_basis.iter().enumerate() {
        for (b, nv) in v_basis.iter().enumerate() {
            let index = (u_span - p + a) * surface.v_count + (v_span - q + b);
            let weight = surface.weights.get(index)? * nu * nv;
            let point = surface.points.get(index)?;
            for axis in 0..3 {
                sum[axis] += point[axis] * weight;
            }
            sum[3] += weight;
        }
    }
    (sum[3] != 0.0).then(|| [sum[0] / sum[3], sum[1] / sum[3], sum[2] / sum[3]])
}

/// The parameter domain of a NURBS surface record.
fn surface_domain(surface: &NurbsSurfaceRecord) -> Option<ParamBox> {
    Some([
        [
            *surface.u_knots.get(surface.u_order.checked_sub(1)?)?,
            *surface.u_knots.get(surface.u_count)?,
        ],
        [
            *surface.v_knots.get(surface.v_order.checked_sub(1)?)?,
            *surface.v_knots.get(surface.v_count)?,
        ],
    ])
}

/// The parameters of the point on `surface` nearest `point`, by
/// Gauss-Newton steps from `start` within the surface domain.
fn project(surface: &NurbsSurfaceRecord, point: V3, start: [f64; 2]) -> Option<[f64; 2]> {
    let domain = surface_domain(surface)?;
    let width = [domain[0][1] - domain[0][0], domain[1][1] - domain[1][0]];
    let clamp = |uv: [f64; 2]| {
        [
            uv[0].clamp(domain[0][0], domain[0][1]),
            uv[1].clamp(domain[1][0], domain[1][1]),
        ]
    };
    let mut uv = clamp(start);
    for _ in 0..PROJECTION_STEPS {
        let at = surface_point(surface, uv)?;
        let partial = |axis: usize| -> Option<V3> {
            let step = width[axis] * SURFACE_STEP;
            let mut low = uv;
            let mut high = uv;
            low[axis] = (uv[axis] - step).max(domain[axis][0]);
            high[axis] = (uv[axis] + step).min(domain[axis][1]);
            let (a, b) = (surface_point(surface, low)?, surface_point(surface, high)?);
            Some(scaled(sub(b, a), 1.0 / (high[axis] - low[axis])))
        };
        let (su, sv) = (partial(0)?, partial(1)?);
        let residual = sub(at, point);
        let (a, b, c) = (dot(su, su), dot(su, sv), dot(sv, sv));
        let (g0, g1) = (dot(su, residual), dot(sv, residual));
        let determinant = a * c - b * b;
        if determinant == 0.0 || !determinant.is_finite() {
            return None;
        }
        let du = -(c * g0 - b * g1) / determinant;
        let dv = -(a * g1 - b * g0) / determinant;
        let next = clamp([uv[0] + du, uv[1] + dv]);
        let moved = ((next[0] - uv[0]) / width[0].max(f64::MIN_POSITIVE)).abs()
            + ((next[1] - uv[1]) / width[1].max(f64::MIN_POSITIVE)).abs();
        uv = next;
        if moved <= PROJECTION_CONVERGED {
            break;
        }
    }
    finite(uv).then_some(uv)
}

/// Whether the image of `pcurve` on `surface` lies within `tolerance` of
/// the part of `curve` on `range`.
fn on_edge(
    surface: &NurbsSurfaceRecord,
    pcurve: &NurbsCurveRecord<2>,
    curve: &NurbsCurveRecord<3>,
    range: [f64; 2],
    tolerance: f64,
) -> bool {
    let Some(part) = segment(curve, range[0], range[1]).or_else(|| Some(curve.clone())) else {
        return false;
    };
    let Some(edge) = helper(&part) else {
        return false;
    };
    let (Some(low), Some(high)) = (
        pcurve.knots.get(pcurve.order - 1),
        pcurve.knots.get(pcurve.points.len()),
    ) else {
        return false;
    };
    let spans = pcurve.points.len().max(ON_EDGE_SAMPLES);
    (0..spans).all(|index| {
        let fraction = (cadmpeg_core::convert::f64_from_index(index).unwrap_or(0.0)
            + ON_EDGE_OFFSET)
            / cadmpeg_core::convert::f64_from_index(spans).unwrap_or(1.0);
        let Some(point) = evaluate(pcurve, low + (high - low) * fraction)
            .and_then(|uv| surface_point(surface, uv))
        else {
            return false;
        };
        edge.nearest(point)
            .and_then(|t| edge.point_at(t))
            .is_some_and(|nearest| {
                let gap = sub(nearest, point);
                dot(gap, gap).sqrt() <= tolerance
            })
    })
}

/// The pcurve of an oriented edge on a NURBS surface: the projection of
/// its edge curve, from the points of the pcurve `guide`, traced within
/// `tolerance`.
fn projected_nurbs_pcurve(
    surface: &NurbsSurfaceRecord,
    guide: &NurbsCurveRecord<2>,
    curve: &NurbsCurveRecord<3>,
    range: [f64; 2],
    forward: bool,
    tolerance: f64,
) -> Option<NurbsCurveRecord<2>> {
    let guide_domain = [
        *guide.knots.get(guide.order - 1)?,
        *guide.knots.get(guide.points.len())?,
    ];
    let image = |fraction: f64| {
        let t = if forward {
            range[0] + (range[1] - range[0]) * fraction
        } else {
            range[1] - (range[1] - range[0]) * fraction
        };
        let along = guide_domain[0] + (guide_domain[1] - guide_domain[0]) * fraction;
        project(surface, evaluate(curve, t)?, evaluate(guide, along)?)
    };
    traced(&image, 0.0, 1.0, tolerance)
}

/// Samples of an edge that a derived pcurve follows from one point to the
/// next.
const GUIDE_SAMPLES: usize = 32;
/// Grid cells along each direction of a first nearest-point search.
const SEARCH_CELLS: usize = 16;

/// The parameters on `surface` of the grid point nearest `point`.
fn grid_nearest(surface: &NurbsSurfaceRecord, point: V3) -> Option<[f64; 2]> {
    let [u, v] = surface_domain(surface)?;
    let cells = cadmpeg_core::convert::f64_from_index(SEARCH_CELLS)?;
    let mut best: Option<(f64, [f64; 2])> = None;
    for i in 0..=SEARCH_CELLS {
        for j in 0..=SEARCH_CELLS {
            let uv = [
                u[0] + (u[1] - u[0]) * cadmpeg_core::convert::f64_from_index(i)? / cells,
                v[0] + (v[1] - v[0]) * cadmpeg_core::convert::f64_from_index(j)? / cells,
            ];
            let gap = sub(surface_point(surface, uv)?, point);
            let distance = dot(gap, gap);
            if best.is_none_or(|(least, _)| distance < least) {
                best = Some((distance, uv));
            }
        }
    }
    best.map(|(_, uv)| uv)
}

/// Whether `surface` closes along `axis`: its two boundary curves across
/// that direction coincide within `tolerance`.
fn closes(surface: &NurbsSurfaceRecord, axis: usize, tolerance: f64) -> bool {
    let Some(domain) = surface_domain(surface) else {
        return false;
    };
    let other = 1 - axis;
    (0..=SEARCH_CELLS).all(|step| {
        let along = domain[other][0]
            + (domain[other][1] - domain[other][0])
                * cadmpeg_core::convert::f64_from_index(step).unwrap_or(0.0)
                / cadmpeg_core::convert::f64_from_index(SEARCH_CELLS).unwrap_or(1.0);
        let mut low = [0.0; 2];
        let mut high = [0.0; 2];
        low[axis] = domain[axis][0];
        high[axis] = domain[axis][1];
        low[other] = along;
        high[other] = along;
        match (surface_point(surface, low), surface_point(surface, high)) {
            (Some(a), Some(b)) => dot(sub(a, b), sub(a, b)).sqrt() <= tolerance,
            _ => false,
        }
    })
}

/// The guide of a derived pcurve: the projections of samples along the
/// oriented edge, each from the last. On a closed surface, a guide that
/// runs along the seam takes the side that leaves the face to its left.
fn derived_guide(
    surface: &NurbsSurfaceRecord,
    curve: &NurbsCurveRecord<3>,
    range: [f64; 2],
    forward: bool,
    counterclockwise: bool,
    tolerance: f64,
) -> Option<NurbsCurveRecord<2>> {
    let count = cadmpeg_core::convert::f64_from_index(GUIDE_SAMPLES)?;
    let mut points: Vec<[f64; 2]> = Vec::new();
    for index in 0..=GUIDE_SAMPLES {
        let fraction = cadmpeg_core::convert::f64_from_index(index)? / count;
        let t = if forward {
            range[0] + (range[1] - range[0]) * fraction
        } else {
            range[1] - (range[1] - range[0]) * fraction
        };
        let point = evaluate(curve, t)?;
        let start = match points.last() {
            Some(previous) => *previous,
            None => grid_nearest(surface, point)?,
        };
        points.push(project(surface, point, start)?);
    }
    let domain = surface_domain(surface)?;
    for axis in 0..2 {
        let other = 1 - axis;
        let width = (domain[axis][1] - domain[axis][0]).abs();
        let on_seam = points.iter().all(|point| {
            (point[axis] - domain[axis][0]).abs() <= BOX_MARGIN * width
                || (domain[axis][1] - point[axis]).abs() <= BOX_MARGIN * width
        });
        if !on_seam || !closes(surface, axis, tolerance) {
            continue;
        }
        let rising = points.last()?[other] > points.first()?[other];
        // Seen from the side toward N, a counterclockwise boundary runs up
        // the high-u side and along the low-v side.
        let high = if axis == 0 { rising } else { !rising };
        let side = if high == counterclockwise {
            domain[axis][1]
        } else {
            domain[axis][0]
        };
        for point in &mut points {
            point[axis] = side;
        }
    }
    let n = points.len();
    let mut knots = vec![0.0, 0.0];
    for index in 1..n - 1 {
        knots.push(
            cadmpeg_core::convert::f64_from_index(index)?
                / cadmpeg_core::convert::f64_from_index(n - 1)?,
        );
    }
    knots.extend([1.0, 1.0]);
    Some(NurbsCurveRecord {
        order: 2,
        weights: vec![1.0; points.len()],
        points,
        knots,
        form: Form::Open,
    })
}

/// Adds a pcurve for each oriented edge of `brep`, a B-rep on NURBS
/// carriers with no pcurves: the projection of its edge curve onto its
/// face surface. Leaves `brep` as it is when a projection fails.
pub(crate) fn derive_pcurves(brep: &mut Brep) {
    let tolerance = trace_tolerance(brep);
    let mut pcurves: Vec<(usize, NurbsCurveRecord<2>)> = Vec::new();
    for face in &brep.faces {
        let (Catalog::Nurbs, surface) = face.surface else {
            return;
        };
        let Some(surface) = brep.nurbs_surfaces.get(surface) else {
            return;
        };
        for (position, loop_index) in face.loops.iter().enumerate() {
            let Some(loop_) = brep.loops.get(*loop_index) else {
                return;
            };
            let counterclockwise = (position == 0) == face.same_sense;
            for oriented_index in &loop_.oriented_edges {
                let Some(oriented) = brep.oriented_edges.get(*oriented_index) else {
                    return;
                };
                let Some(edge) = brep.edges.get(oriented.edge) else {
                    return;
                };
                let Some((Catalog::Nurbs, curve)) = edge.curve else {
                    return;
                };
                let Some(curve) = brep.nurbs_curves.get(curve) else {
                    return;
                };
                let forward = oriented.orientation == edge.same_sense;
                let traced = derived_guide(
                    surface,
                    curve,
                    edge.range,
                    forward,
                    counterclockwise,
                    brep.tolerance,
                )
                .and_then(|guide| {
                    projected_nurbs_pcurve(surface, &guide, curve, edge.range, forward, tolerance)
                });
                let Some(traced) = traced else {
                    return;
                };
                pcurves.push((*oriented_index, traced));
            }
        }
    }
    for (oriented, curve) in pcurves {
        brep.nurbs_pcurves.push(curve);
        let index = brep.nurbs_pcurves.len() - 1;
        if let Some(record) = brep.oriented_edges.get_mut(oriented) {
            record.pcurve = Some((Catalog::Nurbs, index));
        }
    }
}

/// The parameter-space deviation of a traced pcurve: the model tolerance,
/// divided by the box diagonal when the box is larger than one unit, so that
/// a deviation in an angle stays within the tolerance on the largest radius.
fn trace_tolerance(brep: &Brep) -> f64 {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for point in brep.vertices.iter().chain(
        brep.nurbs_curves
            .iter()
            .flat_map(|curve| curve.points.iter()),
    ) {
        for axis in 0..3 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    let diagonal = (0..3)
        .map(|axis| (high[axis] - low[axis]).powi(2))
        .sum::<f64>()
        .sqrt();
    if diagonal.is_finite() && diagonal > 1.0 {
        brep.tolerance / diagonal
    } else {
        brep.tolerance
    }
}

/// The NURBS form of `brep`.
pub(crate) struct Converted {
    pub(crate) brep: Brep,
    /// Whether each oriented edge has its pcurve.
    pub(crate) trimmed: bool,
}

/// The NURBS form of `brep`: surface `i` is the surface of face `i`, curve
/// `i` the curve of edge `i`, and, when every pcurve maps exactly, pcurve
/// `i` the pcurve of oriented edge `i`. Each edge curve runs from the start
/// vertex to the end vertex.
pub(crate) fn nurbs_only(brep: &Brep) -> Result<Converted, String> {
    if brep.angle_scale != 1.0 {
        return Err("angular parameters are not in radians".to_owned());
    }
    let mut out = Brep {
        vertices: brep.vertices.clone(),
        loops: brep.loops.clone(),
        shells: brep.shells.clone(),
        solids: brep.solids.clone(),
        tolerance: brep.tolerance,
        angle_scale: 1.0,
        valid: brep.valid,
        identities: brep.identities.clone(),
        tolerances: brep.tolerances.clone(),
        ..Brep::default()
    };
    for (index, edge) in brep.edges.iter().enumerate() {
        let curve = edge
            .curve
            .ok_or_else(|| format!("edge {index} has no curve"))?;
        let [start, end] = edge.range;
        let (mut record, map) = curve_nurbs(brep, curve, start, end)?;
        let mut range = [map.apply(start), map.apply(end)];
        let domain = [
            record.knots.get(record.order - 1).copied(),
            record.knots.get(record.points.len()).copied(),
        ];
        if domain != [Some(range[0]), Some(range[1])] {
            if let Some(part) = segment(&record, range[0], range[1]) {
                record = part;
            }
        }
        if !edge.same_sense {
            let first = record.knots.first().copied().unwrap_or(0.0);
            let last = record.knots.last().copied().unwrap_or(0.0);
            record = reversed(&record);
            range = [first + last - range[1], first + last - range[0]];
        }
        let mut written = edge.clone();
        written.curve = Some((Catalog::Nurbs, index));
        written.range = range;
        written.same_sense = true;
        out.edges.push(written);
        out.nurbs_curves.push(record);
    }
    let tolerance = trace_tolerance(&out);
    let mut maps = Vec::new();
    for (index, face) in brep.faces.iter().enumerate() {
        let surface = match face.surface {
            (Catalog::Nurbs, surface) => Surface {
                record: brep
                    .nurbs_surfaces
                    .get(surface)
                    .cloned()
                    .ok_or_else(|| format!("no NURBS surface {surface}"))?,
                maps: [ParamMap::Identity, ParamMap::Identity],
            },
            (Catalog::Analytic, surface) => {
                let record = brep
                    .analytic_surfaces
                    .get(surface)
                    .ok_or_else(|| format!("no analytic surface {surface}"))?;
                analytic_surface(brep, index, record, &out, tolerance)?
            }
        };
        if !surface.record.points.iter().all(|point| finite(*point)) {
            return Err(format!("the surface of face {index} is not finite"));
        }
        let mut written = face.clone();
        written.surface = (Catalog::Nurbs, index);
        out.faces.push(written);
        out.nurbs_surfaces.push(surface.record);
        maps.push((face.surface, surface.maps));
    }
    let mut pcurves = Vec::new();
    let mut trimmed = true;
    for (index, oriented) in brep.oriented_edges.iter().enumerate() {
        let face = brep.faces.iter().position(|face| {
            face.loops.iter().any(|loop_| {
                brep.loops
                    .get(*loop_)
                    .is_some_and(|record| record.oriented_edges.contains(&index))
            })
        });
        let mapped = face.and_then(|face| {
            let (carrier, surface_maps) = maps.get(face)?;
            let curve = match oriented.pcurve {
                Some((Catalog::Nurbs, pcurve)) => brep.nurbs_pcurves.get(pcurve)?.clone(),
                Some((Catalog::Analytic, pcurve)) => analytic_pcurve(brep.analytic_pcurves.get(pcurve)?).ok()?.0,
                None => planar_pcurve(brep, *carrier, oriented, &out, tolerance)?,
            };
            let revolved = match carrier {
                (Catalog::Analytic, index) => brep.analytic_surfaces.get(*index).filter(|surface| {
                    matches!(
                        surface.kind,
                        SurfaceKind::Cylinder | SurfaceKind::Cone | SurfaceKind::Sphere | SurfaceKind::Torus
                    )
                }),
                (Catalog::Nurbs, _) => None,
            };
            let plane = matches!(carrier, (Catalog::Analytic, index)
                if brep.analytic_surfaces.get(*index).is_some_and(|surface| surface.kind == SurfaceKind::Plane));
            let edge_curve = out.nurbs_curves.get(oriented.edge)?;
            let range = out.edges.get(oriented.edge)?.range;
            match revolved {
                Some(surface) => projected_pcurve(
                    surface,
                    &curve,
                    edge_curve,
                    range,
                    oriented.orientation,
                    surface_maps,
                    tolerance,
                ),
                None if plane => mapped_pcurve(&curve, surface_maps, tolerance),
                None => {
                    let guide = mapped_pcurve(&curve, surface_maps, tolerance)?;
                    let surface = out.nurbs_surfaces.get(face)?;
                    if on_edge(surface, &guide, edge_curve, range, out.tolerance) {
                        return Some(guide);
                    }
                    projected_nurbs_pcurve(
                        &out.nurbs_surfaces.get(face)?.clone(),
                        &guide,
                        edge_curve,
                        range,
                        oriented.orientation,
                        tolerance,
                    )
                    .or(Some(guide))
                }
            }
        });
        match mapped {
            Some(curve) => pcurves.push(curve),
            None => trimmed = false,
        }
    }
    for (index, oriented) in brep.oriented_edges.iter().enumerate() {
        let mut written = oriented.clone();
        written.pcurve = trimmed.then_some((Catalog::Nurbs, index));
        out.oriented_edges.push(written);
    }
    if trimmed {
        out.nurbs_pcurves = pcurves;
        wind(&mut out);
    }
    for face in 0..out.faces.len() {
        if out.faces[face].loops.is_empty() {
            boundary_loop(&mut out, face, trimmed)?;
        }
    }
    Ok(Converted { brep: out, trimmed })
}

/// The boundary curve of `surface` along `axis` at the low or high end of
/// the other direction: the control row or column there, which the curve
/// interpolates on a clamped knot vector.
fn boundary_curve(
    surface: &NurbsSurfaceRecord,
    axis: usize,
    high: bool,
) -> Option<NurbsCurveRecord<3>> {
    let (order, count, knots, other_count) = if axis == 0 {
        (
            surface.u_order,
            surface.u_count,
            &surface.u_knots,
            surface.v_count,
        )
    } else {
        (
            surface.v_order,
            surface.v_count,
            &surface.v_knots,
            surface.u_count,
        )
    };
    let other_knots = if axis == 0 {
        &surface.v_knots
    } else {
        &surface.u_knots
    };
    let other_order = if axis == 0 {
        surface.v_order
    } else {
        surface.u_order
    };
    let clamped = |knots: &[f64], order: usize| {
        knots.len() >= 2 * order
            && knots[..order].iter().all(|knot| *knot == knots[0])
            && knots[knots.len() - order..]
                .iter()
                .all(|knot| *knot == knots[knots.len() - 1])
    };
    if !clamped(knots, order) || !clamped(other_knots, other_order) {
        return None;
    }
    let fixed = if high { other_count.checked_sub(1)? } else { 0 };
    let index = |along: usize| {
        if axis == 0 {
            along * surface.v_count + fixed
        } else {
            fixed * surface.v_count + along
        }
    };
    Some(NurbsCurveRecord {
        order,
        points: (0..count)
            .map(|along| surface.points.get(index(along)).copied())
            .collect::<Option<_>>()?,
        weights: (0..count)
            .map(|along| surface.weights.get(index(along)).copied())
            .collect::<Option<_>>()?,
        knots: knots.clone(),
        form: Form::Open,
    })
}

/// Whether every control point of `curve` lies within `tolerance` of its
/// first.
fn collapsed(curve: &NurbsCurveRecord<3>, tolerance: f64) -> bool {
    curve.points.iter().all(|point| {
        let gap = sub(*point, curve.points[0]);
        dot(gap, gap).sqrt() <= tolerance
    })
}

/// Gives face `face`, which has no loop, the boundary of its surface domain
/// as its outer loop: an edge for each side that does not collapse to a
/// point, one seam edge for two sides that coincide, and, when `trimmed`,
/// the sides of the domain as pcurves.
fn boundary_loop(brep: &mut Brep, face: usize, trimmed: bool) -> Result<(), String> {
    let refused = || format!("face {face} has no loop and no clamped boundary");
    let surface = brep.nurbs_surfaces.get(face).cloned().ok_or_else(refused)?;
    let domain = surface_domain(&surface).ok_or_else(refused)?;
    let tolerance = brep.tolerance;
    // The sides in counterclockwise order about N: (axis, high end of the
    // other direction, runs forward along its axis).
    let sides = [
        (0, false, true),
        (1, true, true),
        (0, true, false),
        (1, false, false),
    ];
    let curves: Vec<NurbsCurveRecord<3>> = sides
        .iter()
        .map(|(axis, high, _)| boundary_curve(&surface, *axis, *high))
        .collect::<Option<_>>()
        .ok_or_else(refused)?;
    let same = |a: &NurbsCurveRecord<3>, b: &NurbsCurveRecord<3>| {
        a.points.len() == b.points.len()
            && a.points.iter().zip(&b.points).all(|(p, q)| {
                let gap = sub(*p, *q);
                dot(gap, gap).sqrt() <= tolerance
            })
    };
    let face_identity = brep.identities.faces.get(face).cloned().flatten();
    let vertex = |brep: &mut Brep, point: V3| -> usize {
        if let Some(found) = brep.vertices.iter().position(|known| {
            let gap = sub(*known, point);
            dot(gap, gap).sqrt() <= tolerance
        }) {
            return found;
        }
        brep.vertices.push(point);
        let index = brep.vertices.len() - 1;
        if let Some(face) = &face_identity {
            brep.identities.vertices.resize(index, None);
            brep.identities
                .vertices
                .push(Some(crate::ids::brep::vertex(face, index).to_string()));
        }
        index
    };
    let same_sense = brep.faces[face].same_sense;
    // The record loop goes counterclockwise about the outward normal: the
    // sides in reverse when the face normal opposes N.
    let order: [usize; 4] = if same_sense {
        [0, 1, 2, 3]
    } else {
        [3, 2, 1, 0]
    };
    let mut edges: [Option<usize>; 4] = [None; 4];
    for position in order {
        let curve = &curves[position];
        if collapsed(curve, tolerance) {
            continue;
        }
        let opposite = (position + 2) % 4;
        if let Some(seam) = edges[opposite].filter(|_| same(curve, &curves[opposite])) {
            edges[position] = Some(seam);
            continue;
        }
        let start = vertex(brep, *curve.points.first().ok_or_else(refused)?);
        let end = vertex(brep, *curve.points.last().ok_or_else(refused)?);
        let range = [
            *curve.knots.get(curve.order - 1).ok_or_else(refused)?,
            *curve.knots.get(curve.points.len()).ok_or_else(refused)?,
        ];
        brep.nurbs_curves.push(curve.clone());
        if let Some(face) = &face_identity {
            let index = brep.edges.len();
            brep.identities.edges.resize(index, None);
            brep.identities
                .edges
                .push(Some(crate::ids::brep::edge(face, position).to_string()));
        }
        brep.edges.push(super::EdgeRecord {
            vertices: [start, end],
            curve: Some((Catalog::Nurbs, brep.nurbs_curves.len() - 1)),
            range,
            same_sense: true,
            tolerance: None,
        });
        edges[position] = Some(brep.edges.len() - 1);
    }
    let [u, v] = domain;
    let corners = [[u[0], v[0]], [u[1], v[0]], [u[1], v[1]], [u[0], v[1]]];
    let mut oriented = Vec::new();
    for position in order {
        let (_, _, forward) = sides[position];
        let Some(edge) = edges[position] else {
            continue;
        };
        let pcurve = trimmed.then(|| {
            let (start, end) = (corners[position], corners[(position + 1) % 4]);
            let (from, to) = if same_sense {
                (start, end)
            } else {
                (end, start)
            };
            brep.nurbs_pcurves.push(NurbsCurveRecord {
                order: 2,
                points: vec![from, to],
                weights: vec![1.0, 1.0],
                knots: vec![0.0, 0.0, 1.0, 1.0],
                form: Form::Open,
            });
            (Catalog::Nurbs, brep.nurbs_pcurves.len() - 1)
        });
        brep.oriented_edges.push(super::OrientedEdgeRecord {
            edge,
            orientation: forward == same_sense,
            pcurve,
        });
        oriented.push(brep.oriented_edges.len() - 1);
    }
    if oriented.is_empty() {
        return Err(refused());
    }
    brep.loops.push(super::LoopRecord {
        oriented_edges: oriented,
        vertex: None,
    });
    let loop_index = brep.loops.len() - 1;
    brep.faces[face].loops.push(loop_index);
    Ok(())
}

/// Twice the signed area of the polygon of control points of the pcurves
/// of `loop_`, or `None` when the pcurves do not close in parameter space.
fn winding(brep: &Brep, loop_: &super::LoopRecord) -> Option<f64> {
    let mut points: Vec<[f64; 2]> = Vec::new();
    for oriented in &loop_.oriented_edges {
        let (_, pcurve) = brep.oriented_edges.get(*oriented)?.pcurve?;
        points.extend(brep.nurbs_pcurves.get(pcurve)?.points.iter());
    }
    let (first, last) = (*points.first()?, *points.last()?);
    let extent = points
        .iter()
        .flat_map(|point| point.iter())
        .fold(0.0_f64, |most, value| most.max(value.abs()))
        .max(1.0);
    if (first[0] - last[0]).hypot(first[1] - last[1]) > BOX_MARGIN * extent {
        return None;
    }
    Some(
        points
            .windows(2)
            .map(|pair| pair[0][0] * pair[1][1] - pair[1][0] * pair[0][1])
            .sum(),
    )
}

/// Reverses each loop of seam pairs only whose pcurves close and wind
/// against its role: an outer loop counterclockwise and an inner loop
/// clockwise, seen from the face's outward normal.
fn wind(brep: &mut Brep) {
    let mut flips = Vec::new();
    for face in &brep.faces {
        for (position, loop_index) in face.loops.iter().enumerate() {
            let Some(record) = brep.loops.get(*loop_index) else {
                continue;
            };
            let edges: Vec<usize> = record
                .oriented_edges
                .iter()
                .filter_map(|oriented| brep.oriented_edges.get(*oriented).map(|record| record.edge))
                .collect();
            let seams_only = !edges.is_empty()
                && edges
                    .iter()
                    .all(|edge| edges.iter().filter(|other| *other == edge).count() == 2);
            if !seams_only {
                continue;
            }
            let Some(area) = winding(brep, record) else {
                continue;
            };
            let outer = position == 0;
            let positive = outer == face.same_sense;
            if area != 0.0 && (area > 0.0) != positive {
                flips.push(*loop_index);
            }
        }
    }
    for loop_index in flips {
        let Some(record) = brep.loops.get_mut(loop_index) else {
            continue;
        };
        record.oriented_edges.reverse();
        for oriented in record.oriented_edges.clone() {
            if let Some(edge) = brep.oriented_edges.get_mut(oriented) {
                edge.orientation = !edge.orientation;
                if let Some((_, pcurve)) = edge.pcurve {
                    if let Some(curve) = brep.nurbs_pcurves.get_mut(pcurve) {
                        *curve = reversed(curve);
                    }
                }
            }
        }
    }
}

/// The pcurve of an oriented edge on a plane: the projection of its edge
/// curve onto the plane's axes, in the direction of the oriented edge.
fn planar_pcurve(
    brep: &Brep,
    carrier: Reference,
    oriented: &super::OrientedEdgeRecord,
    converted: &Brep,
    tolerance: f64,
) -> Option<NurbsCurveRecord<2>> {
    let (Catalog::Analytic, index) = carrier else {
        return None;
    };
    let surface = brep.analytic_surfaces.get(index)?;
    if surface.kind != SurfaceKind::Plane {
        return None;
    }
    let (origin, x_axis, y_axis, _) = axes(&surface.frame);
    let curve = converted.nurbs_curves.get(oriented.edge)?;
    let range = converted.edges.get(oriented.edge)?.range;
    let project = |point: V3| {
        [
            dot(sub(point, origin), x_axis),
            dot(sub(point, origin), y_axis),
        ]
    };
    let domain = [
        *curve.knots.get(curve.order - 1)?,
        *curve.knots.get(curve.points.len())?,
    ];
    let projected = if range == domain {
        NurbsCurveRecord {
            order: curve.order,
            points: curve.points.iter().map(|point| project(*point)).collect(),
            weights: curve.weights.clone(),
            knots: curve.knots.clone(),
            form: curve.form,
        }
    } else {
        traced(
            &|t| evaluate(curve, t).map(project),
            range[0],
            range[1],
            tolerance,
        )?
    };
    Some(if oriented.orientation {
        projected
    } else {
        reversed(&projected)
    })
}

#[cfg(test)]
mod tests;
