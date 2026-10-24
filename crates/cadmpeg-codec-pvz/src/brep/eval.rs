// SPDX-License-Identifier: Apache-2.0
//! Evaluation of native surfaces, edge curves, and pcurves in the
//! parameterisations of §8.4 to §8.16.

use std::f64::consts::TAU;

use super::model::{BsplineCurve, BsplineSurface, CurveGeometry, SurfaceGeometry, Vec2, Vec3};

/// Uniform samples of the nearest-point search of [`nearest_distance`].
const NEAREST_SAMPLES: usize = 16;
/// Golden-section steps that refine the nearest-point search.
const NEAREST_REFINEMENT_STEPS: usize = 24;
/// Curve evaluations of one [`nearest_distance`] search.
pub(crate) const NEAREST_POINT_WORK: usize = NEAREST_SAMPLES + 1 + 2 * NEAREST_REFINEMENT_STEPS;

/// `a + b`.
pub(crate) fn add<const N: usize>(a: [f64; N], b: [f64; N]) -> [f64; N] {
    std::array::from_fn(|index| a[index] + b[index])
}

/// `a - b`.
pub(crate) fn sub<const N: usize>(a: [f64; N], b: [f64; N]) -> [f64; N] {
    std::array::from_fn(|index| a[index] - b[index])
}

/// `s·a`.
pub(crate) fn scale<const N: usize>(a: [f64; N], s: f64) -> [f64; N] {
    a.map(|value| value * s)
}

/// `a · b`.
pub(crate) fn dot<const N: usize>(a: [f64; N], b: [f64; N]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// `|a|`.
pub(crate) fn norm<const N: usize>(a: [f64; N]) -> f64 {
    dot(a, a).sqrt()
}

/// `|a - b|`.
pub(crate) fn distance<const N: usize>(a: [f64; N], b: [f64; N]) -> f64 {
    norm(sub(a, b))
}

/// `a × b`.
pub(crate) fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// `Σ wᵢ·vᵢ` over weighted vectors.
fn combine(terms: &[(f64, Vec3)]) -> Vec3 {
    terms.iter().fold([0.0; 3], |sum, (weight, vector)| {
        add(sum, scale(*vector, *weight))
    })
}

/// The knot span and the `order` nonzero basis values at `t`, with `t`
/// clamped to the knot domain `[knots[order - 1], knots[pole_count]]`.
fn basis(knots: &[f64], order: usize, t: f64) -> Option<(usize, Vec<f64>)> {
    let degree = order.checked_sub(1)?;
    let pole_count = knots.len().checked_sub(order)?;
    let low = *knots.get(degree)?;
    let high = *knots.get(pole_count)?;
    if !(low.is_finite() && high.is_finite()) {
        return None;
    }
    let t = t.clamp(low.min(high), high.max(low));
    let interior = knots.get(degree + 1..pole_count)?;
    let span = degree + interior.partition_point(|knot| *knot <= t);
    let mut values: Vec<f64> = (0..order).map(|_| 0.0).collect();
    let mut left = values.clone();
    let mut right = values.clone();
    *values.first_mut()? = 1.0;
    for j in 1..order {
        left[j] = t - knots.get(span + 1 - j)?;
        right[j] = knots.get(span + j)? - t;
        let mut saved = 0.0;
        for r in 0..j {
            let denominator = right[r + 1] + left[j - r];
            let term = if denominator == 0.0 {
                0.0
            } else {
                values[r] / denominator
            };
            values[r] = saved + right[r + 1] * term;
            saved = left[j - r] * term;
        }
        values[j] = saved;
    }
    Some((span, values))
}

/// Knot domain of a curve.
pub(crate) fn knot_domain(knots: &[f64], order: usize) -> Option<Vec2> {
    let pole_count = knots.len().checked_sub(order)?;
    Some([*knots.get(order.checked_sub(1)?)?, *knots.get(pole_count)?])
}

/// Point of a rational B-spline curve at `t`.
pub(crate) fn bspline_curve_point<const N: usize>(
    curve: &BsplineCurve<N>,
    t: f64,
) -> Option<[f64; N]> {
    let (span, values) = basis(&curve.knots, curve.order, t)?;
    let first = span.checked_sub(curve.order - 1)?;
    let mut weighted = [0.0; N];
    let mut weight = 0.0;
    for (offset, value) in values.iter().enumerate() {
        let index = first + offset;
        weighted = add(weighted, scale(*curve.weighted.get(index)?, *value));
        weight += value * curve.weights.get(index)?;
    }
    (weight != 0.0).then(|| scale(weighted, weight.recip()))
}

/// `t` moved by whole periods into the knot domain when `periodic`.
fn wrap(knots: &[f64], order: usize, periodic: bool, t: f64) -> Option<f64> {
    if !periodic {
        return Some(t);
    }
    let [low, high] = knot_domain(knots, order)?;
    let period = high - low;
    Some(if period > 0.0 {
        low + (t - low).rem_euclid(period)
    } else {
        t
    })
}

/// Point of a rational B-spline surface at `(u, v)`.
pub(crate) fn bspline_surface_point(surface: &BsplineSurface, uv: Vec2) -> Option<Vec3> {
    let [u_periodic, v_periodic] = surface.periodic;
    let u_at = wrap(&surface.u_knots, surface.u_order, u_periodic, uv[0])?;
    let v_at = wrap(&surface.v_knots, surface.v_order, v_periodic, uv[1])?;
    let (u_span, u_values) = basis(&surface.u_knots, surface.u_order, u_at)?;
    let (v_span, v_values) = basis(&surface.v_knots, surface.v_order, v_at)?;
    let u_first = u_span.checked_sub(surface.u_order - 1)?;
    let v_first = v_span.checked_sub(surface.v_order - 1)?;
    let mut sum = [0.0; 4];
    for (i, u_value) in u_values.iter().enumerate() {
        for (j, v_value) in v_values.iter().enumerate() {
            let index = (u_first + i)
                .checked_mul(surface.v_count)?
                .checked_add(v_first + j)?;
            sum = add(sum, scale(*surface.poles.get(index)?, u_value * v_value));
        }
    }
    let [weighted @ .., weight] = sum;
    (weight != 0.0).then(|| scale(weighted, weight.recip()))
}

/// Point of a native surface at `(u, v)`.
pub(crate) fn surface_point(surface: &SurfaceGeometry, uv: Vec2) -> Option<Vec3> {
    let [u, v] = uv;
    let (cos_u, sin_u) = (u.cos(), u.sin());
    match surface {
        SurfaceGeometry::Plane {
            origin,
            u_axis,
            v_axis,
            ..
        } => Some(add(*origin, combine(&[(u, *u_axis), (v, *v_axis)]))),
        SurfaceGeometry::Cylinder { origin, x, y, axis } => Some(add(
            *origin,
            combine(&[(cos_u, *x), (sin_u, *y), (v, *axis)]),
        )),
        SurfaceGeometry::Cone {
            origin,
            x,
            y,
            axis,
            a,
            b,
        } => {
            let radius = a + b * v;
            Some(add(
                *origin,
                combine(&[(v, *axis), (radius * cos_u, *x), (radius * sin_u, *y)]),
            ))
        }
        SurfaceGeometry::Sphere { center, x, y, z } => Some(add(
            *center,
            combine(&[(v.cos() * cos_u, *x), (v.cos() * sin_u, *y), (v.sin(), *z)]),
        )),
        SurfaceGeometry::Torus(torus) => {
            let minor_radius = norm(torus.minor_cos);
            Some(add(
                torus.origin,
                combine(&[
                    (cos_u, torus.major_cos),
                    (sin_u, torus.major_sin),
                    (v.cos() * cos_u, torus.minor_cos),
                    (-v.cos() * minor_radius * sin_u, torus.minor_sin),
                    (v.sin(), torus.minor_axial),
                ]),
            ))
        }
        SurfaceGeometry::Bspline(surface) => bspline_surface_point(surface, uv),
        SurfaceGeometry::Undefined { .. } => None,
    }
}

/// Natural parameter interval of a curve (§8.15, §8.16). A polyline is
/// parameterised by point index.
pub(crate) fn curve_interval<const N: usize>(curve: &CurveGeometry<N>) -> Option<Vec2> {
    match curve {
        CurveGeometry::Circle(conic) | CurveGeometry::Ellipse(conic) => {
            Some(conic.range.unwrap_or([0.0, TAU]))
        }
        CurveGeometry::Line { .. } => Some([0.0, 1.0]),
        CurveGeometry::Point(_) => Some([0.0, 0.0]),
        CurveGeometry::Bspline(curve) => curve
            .range
            .or_else(|| knot_domain(&curve.knots, curve.order)),
        CurveGeometry::Polyline(points) => {
            let last = points.len().checked_sub(1)?;
            Some([0.0, cadmpeg_core::convert::f64_from_index(last)?])
        }
        CurveGeometry::Composite(_) | CurveGeometry::Undefined { .. } => None,
    }
}

/// Point of a curve at `t`.
pub(crate) fn curve_point<const N: usize>(curve: &CurveGeometry<N>, t: f64) -> Option<[f64; N]> {
    match curve {
        CurveGeometry::Circle(conic) | CurveGeometry::Ellipse(conic) => Some(add(
            conic.center,
            add(scale(conic.x, t.cos()), scale(conic.y, t.sin())),
        )),
        CurveGeometry::Line { origin, direction } => Some(add(*origin, scale(*direction, t))),
        CurveGeometry::Point(point) => Some(*point),
        CurveGeometry::Bspline(curve) => bspline_curve_point(curve, t),
        CurveGeometry::Polyline(points) => {
            let last = points.len().checked_sub(1)?;
            let clamped = t.clamp(0.0, cadmpeg_core::convert::f64_from_index(last)?);
            let index = cadmpeg_core::convert::truncate_f64_to_usize(clamped.floor())?.min(last);
            let next = (index + 1).min(last);
            let fraction = clamped - cadmpeg_core::convert::f64_from_index(index)?;
            let start = *points.get(index)?;
            Some(add(start, scale(sub(*points.get(next)?, start), fraction)))
        }
        CurveGeometry::Composite(_) | CurveGeometry::Undefined { .. } => None,
    }
}

/// Start and end points of a curve over its natural interval.
pub(crate) fn curve_ends<const N: usize>(curve: &CurveGeometry<N>) -> Option<[[f64; N]; 2]> {
    let [start, end] = curve_interval(curve)?;
    Some([curve_point(curve, start)?, curve_point(curve, end)?])
}

/// Distance from `point` to `curve` over `interval`: the nearest of
/// [`NEAREST_SAMPLES`] samples, refined by a golden-section search in
/// the sample intervals next to it.
pub(crate) fn nearest_distance<const N: usize>(
    curve: &CurveGeometry<N>,
    interval: Vec2,
    point: [f64; N],
) -> Option<f64> {
    let [low, high] = interval;
    let samples = cadmpeg_core::convert::f64_from_index(NEAREST_SAMPLES)?;
    let parameter = |step: usize| {
        Some(low + (high - low) * cadmpeg_core::convert::f64_from_index(step)? / samples)
    };
    let gap = |t: f64| Some(distance(curve_point(curve, t)?, point));
    let mut nearest = (0, f64::INFINITY);
    for step in 0..=NEAREST_SAMPLES {
        let value = gap(parameter(step)?)?;
        if value < nearest.1 {
            nearest = (step, value);
        }
    }
    let (step, mut best) = nearest;
    let mut a = parameter(step.max(1) - 1)?;
    let mut b = parameter((step + 1).min(NEAREST_SAMPLES))?;
    let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
    for _ in 0..NEAREST_REFINEMENT_STEPS {
        let c = b - (b - a) * ratio;
        let d = a + (b - a) * ratio;
        let (gap_c, gap_d) = (gap(c)?, gap(d)?);
        best = best.min(gap_c).min(gap_d);
        if gap_c < gap_d {
            b = d;
        } else {
            a = c;
        }
    }
    Some(best)
}

#[cfg(test)]
mod tests;
