// SPDX-License-Identifier: Apache-2.0
//! Exact rational NURBS forms of analytic conic and periodic edge carriers.
//!
//! The 3DM `ARC` class holds only circles, so ellipse, parabola, and
//! hyperbola carriers convert to the rational quadratic NURBS form each
//! conic family admits exactly: an ellipse arc is a rational quadratic
//! Bezier with shoulder weight `cos(half-span)`, a parabola span is a
//! polynomial quadratic Bezier through its endpoint tangents, and a
//! hyperbola span is a rational quadratic Bezier whose weight is solved
//! from its midpoint. Converted curves keep the edge's own parameter
//! values as knots, so the edge domain check reads the admitted range.
//!
//! A periodic NURBS edge converts by clamping: Boehm knot insertion raises
//! the used interval's endpoints to full multiplicity and the clamped span
//! is extracted. The operation is exact up to floating-point rounding.

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{EllipseCurve, HyperbolaCurve, ParabolaCurve};
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoles3, WeightedPole3};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteReal, NonZeroReal};

use super::{WriteError, MAX_ARC_SPAN_EXCESS};

/// Tolerance on the hyperbola midpoint proof, relative to the span's
/// coordinate scale: knot insertion and the weight solve are exact up to
/// floating-point rounding, so a larger residual refuses the conversion.
const HYPERBOLA_MIDPOINT_TOLERANCE: f64 = 1.0e-9;

/// Largest hyperbola span converted as one rational quadratic piece. The
/// shoulder weight is `cosh(span / 2)`, which stays near one here, and the
/// tangent intersection stays local to the span.
const HYPERBOLA_SPLIT_SPAN: f64 = 1.0;

fn omit(message: String) -> WriteError {
    WriteError::Omit(message)
}

fn finite_point(point: Point3, what: &str) -> Result<FinitePoint3, WriteError> {
    FinitePoint3::new(point).ok_or_else(|| omit(format!("{what} is not finite")))
}

fn finite_weight(value: f64, what: &str) -> Result<NonZeroReal, WriteError> {
    FiniteReal::new(value)
        .and_then(NonZeroReal::from_finite)
        .ok_or_else(|| omit(format!("{what} is not a nonzero finite weight")))
}

fn pole(point: Point3, weight: f64, what: &str) -> Result<WeightedPole3<FinitePoint3>, WriteError> {
    Ok(WeightedPole3 {
        point: finite_point(point, what)?,
        weight: finite_weight(weight, what)?,
    })
}

fn build_rational(
    id: &str,
    degree: u32,
    knots: Vec<f64>,
    poles: Vec<WeightedPole3<FinitePoint3>>,
    what: &str,
) -> Result<NurbsCurve, WriteError> {
    NurbsCurve::new(
        degree,
        knots,
        NurbsPoles3::Rational { points: poles },
        false,
    )
    .map_err(|error| {
        omit(format!(
            "{what} {id} does not admit a NURBS carrier: {error}"
        ))
    })
}

fn build_polynomial(
    id: &str,
    degree: u32,
    knots: Vec<f64>,
    poles: Vec<FinitePoint3>,
    what: &str,
) -> Result<NurbsCurve, WriteError> {
    NurbsCurve::new(
        degree,
        knots,
        NurbsPoles3::Polynomial { points: poles },
        false,
    )
    .map_err(|error| {
        omit(format!(
            "{what} {id} does not admit a NURBS carrier: {error}"
        ))
    })
}

/// Point and unit frame directions of an ellipse carrier.
struct ConicFrame {
    center: Point3,
    major: Vector3,
    minor: Vector3,
}

fn ellipse_frame(ellipse: &EllipseCurve) -> ConicFrame {
    let center = ellipse.center().get();
    let major = *ellipse.frame().reference().as_raw();
    let minor = ellipse.frame().axis().as_raw().cross(major);
    ConicFrame {
        center,
        major,
        minor,
    }
}

fn at(frame: &ConicFrame, major: f64, minor: f64) -> Point3 {
    Point3::new(
        major.mul_add(frame.major.x, minor.mul_add(frame.minor.x, frame.center.x)),
        major.mul_add(frame.major.y, minor.mul_add(frame.minor.y, frame.center.y)),
        major.mul_add(frame.major.z, minor.mul_add(frame.minor.z, frame.center.z)),
    )
}

/// Exact rational NURBS form of an ellipse arc over `range` in the ellipse's
/// own angular parameter.
pub(super) fn ellipse_over_range(
    id: &str,
    ellipse: &EllipseCurve,
    range: [FiniteReal; 2],
) -> Result<NurbsCurve, WriteError> {
    let [lo, hi] = FiniteReal::raw_array(range);
    let span = hi - lo;
    if !(span > 0.0) || !span.is_finite() || span > std::f64::consts::TAU + MAX_ARC_SPAN_EXCESS {
        return Err(omit(format!(
            "edge {id} ellipse carrier is not a writable arc"
        )));
    }
    let frame = ellipse_frame(ellipse);
    let (major_radius, minor_radius) = (ellipse.major_radius().get(), ellipse.minor_radius().get());
    let parts = (span / std::f64::consts::FRAC_PI_2).ceil() as usize;
    let parts = parts.max(1);
    let width = span / parts as f64;
    let half = width / 2.0;
    let shoulder = finite_weight(half.cos(), &format!("edge {id} ellipse shoulder weight"))?;
    let point_at = |parameter: f64| {
        let (sine, cosine) = parameter.sin_cos();
        at(&frame, major_radius * cosine, minor_radius * sine)
    };
    let mut knots = Vec::with_capacity(3 * (parts + 1));
    let mut poles = Vec::with_capacity(2 * parts + 1);
    knots.extend([lo, lo, lo]);
    for part in 0..parts {
        let start = lo + width * part as f64;
        let end = start + width;
        let middle = start + half;
        let (sine, cosine) = middle.sin_cos();
        let shoulder_point = at(
            &frame,
            major_radius * cosine / half.cos(),
            minor_radius * sine / half.cos(),
        );
        if part == 0 {
            poles.push(WeightedPole3 {
                point: finite_point(point_at(start), &format!("edge {id} ellipse pole"))?,
                weight: NonZeroReal::ONE,
            });
        }
        poles.push(WeightedPole3 {
            point: finite_point(shoulder_point, &format!("edge {id} ellipse pole"))?,
            weight: shoulder,
        });
        poles.push(WeightedPole3 {
            point: finite_point(point_at(end), &format!("edge {id} ellipse pole"))?,
            weight: NonZeroReal::ONE,
        });
        if part + 1 < parts {
            knots.extend([end, end]);
        }
    }
    knots.extend([hi, hi, hi]);
    build_rational(id, 2, knots, poles, "edge")
}

/// Exact polynomial NURBS form of a parabola span over `range` in the
/// parabola's STEP parameter.
pub(super) fn parabola_over_range(
    id: &str,
    parabola: &ParabolaCurve,
    range: [FiniteReal; 2],
) -> Result<NurbsCurve, WriteError> {
    let [lo, hi] = FiniteReal::raw_array(range);
    let span = hi - lo;
    if !(span > 0.0) || !span.is_finite() {
        return Err(omit(format!(
            "edge {id} parabola carrier is not a writable span"
        )));
    }
    let vertex = parabola.vertex().get();
    let major = *parabola.frame().reference().as_raw();
    let transverse = parabola.frame().axis().as_raw().cross(major);
    let focal = parabola.focal_distance().get();
    let point_at = |parameter: f64| {
        vertex
            .translated(major, focal * parameter * parameter)
            .translated(transverse, 2.0 * focal * parameter)
    };
    let tangent_at = |parameter: f64| {
        Vector3::new(
            (2.0 * focal * parameter).mul_add(major.x, (2.0 * focal) * transverse.x),
            (2.0 * focal * parameter).mul_add(major.y, (2.0 * focal) * transverse.y),
            (2.0 * focal * parameter).mul_add(major.z, (2.0 * focal) * transverse.z),
        )
    };
    let start = point_at(lo);
    let end = point_at(hi);
    let shoulder = start.translated(tangent_at(lo), span / 2.0);
    let poles = [
        finite_point(start, &format!("edge {id} parabola pole"))?,
        finite_point(shoulder, &format!("edge {id} parabola pole"))?,
        finite_point(end, &format!("edge {id} parabola pole"))?,
    ];
    build_polynomial(id, 2, vec![lo, lo, lo, hi, hi, hi], poles.to_vec(), "edge")
}

/// Exact rational NURBS form of a hyperbola span over `range` in the
/// hyperbola's STEP parameter. Long spans split so each piece keeps a local
/// tangent intersection and a modest shoulder weight.
pub(super) fn hyperbola_over_range(
    id: &str,
    hyperbola: &HyperbolaCurve,
    range: [FiniteReal; 2],
) -> Result<NurbsCurve, WriteError> {
    let [lo, hi] = FiniteReal::raw_array(range);
    let span = hi - lo;
    if !(span > 0.0) || !span.is_finite() {
        return Err(omit(format!(
            "edge {id} hyperbola carrier is not a writable span"
        )));
    }
    let frame = ConicFrame {
        center: hyperbola.center().get(),
        major: *hyperbola.frame().reference().as_raw(),
        minor: hyperbola
            .frame()
            .axis()
            .as_raw()
            .cross(*hyperbola.frame().reference().as_raw()),
    };
    let (major_radius, minor_radius) = (
        hyperbola.major_radius().get(),
        hyperbola.minor_radius().get(),
    );
    let parts = (span / HYPERBOLA_SPLIT_SPAN).ceil() as usize;
    let parts = parts.max(1);
    let width = span / parts as f64;
    let mut knots = Vec::with_capacity(3 * (parts + 1));
    let mut poles: Vec<WeightedPole3<FinitePoint3>> = Vec::with_capacity(2 * parts + 1);
    knots.extend([lo, lo, lo]);
    for part in 0..parts {
        let start = lo + width * part as f64;
        let end = start + width;
        let middle = start + width / 2.0;
        let piece = hyperbola_piece(id, &frame, major_radius, minor_radius, [start, middle, end])?;
        if part == 0 {
            poles.push(pole(piece.0[0], 1.0, &format!("edge {id} hyperbola pole"))?);
        }
        poles.push(WeightedPole3 {
            point: finite_point(piece.0[1], &format!("edge {id} hyperbola pole"))?,
            weight: finite_weight(piece.1, &format!("edge {id} hyperbola shoulder weight"))?,
        });
        poles.push(pole(piece.0[2], 1.0, &format!("edge {id} hyperbola pole"))?);
        if part + 1 < parts {
            knots.extend([end, end]);
        }
    }
    knots.extend([hi, hi, hi]);
    build_rational(id, 2, knots, poles, "edge")
}

/// Clamp a periodic NURBS curve to its used interval.
///
/// Boehm knot insertion raises each endpoint of `range` to full
/// multiplicity, then the clamped span is extracted as a nonperiodic
/// curve over the same parameter values. The operation is exact up to
/// floating-point rounding; the extracted knots keep the edge's own
/// parameter values so the edge domain check reads the admitted range.
pub(super) fn unperiod_curve(
    id: &str,
    curve: &NurbsCurve,
    range: [FiniteReal; 2],
) -> Result<NurbsCurve, WriteError> {
    let fail = |reason: &str| omit(format!("edge {id} periodic curve {reason}"));
    if !curve.periodic() {
        return Err(fail("is not periodic"));
    }
    let degree =
        usize::try_from(curve.degree()).map_err(|_| fail("degree exceeds address space"))?;
    if degree < 1 {
        return Err(fail("degree is below one"));
    }
    let [lo, hi] = FiniteReal::raw_array(range);
    if !(hi > lo) || !lo.is_finite() || !hi.is_finite() {
        return Err(fail("used interval is not a finite increasing range"));
    }
    let knots = curve.knots().as_slice().to_vec();
    let count = curve.control_points().len();
    if knots.len() != count + degree + 1 {
        return Err(fail("knot and pole counts disagree"));
    }
    let (domain_lo, domain_hi) = (knots[degree], knots[count]);
    if !(domain_lo <= lo && hi <= domain_hi) {
        return Err(fail("used interval leaves the evaluable domain"));
    }
    if hi - lo > domain_hi - domain_lo || !(hi - lo).is_finite() {
        return Err(fail("used interval spans more than one period"));
    }
    // Homogeneous control points; polynomial poles carry unit weights,
    // one per pole already held, so the repeat cannot outgrow the input.
    let points = curve.control_points();
    let weights: Vec<f64> = curve
        .weights()
        .map(|lane| lane.iter().map(|weight| weight.get()).collect())
        .unwrap_or_else(|| vec![1.0; points.len()]);
    if weights.len() != count {
        return Err(fail("weight and pole counts disagree"));
    }
    let mut homogeneous: Vec<[f64; 4]> = points
        .iter()
        .zip(weights.iter())
        .map(|(point, weight)| {
            [
                point.x * weight,
                point.y * weight,
                point.z * weight,
                *weight,
            ]
        })
        .collect();
    let mut knots = knots;
    for endpoint in [lo, hi] {
        let mut multiplicity = knots.iter().filter(|knot| **knot == endpoint).count();
        if multiplicity > degree {
            return Err(fail("endpoint multiplicity exceeds the degree"));
        }
        while multiplicity < degree {
            let span = locate_span(&knots, degree, homogeneous.len(), endpoint)
                .ok_or_else(|| fail("endpoint is not over a knot span"))?;
            insert_knot(&mut knots, &mut homogeneous, degree, span, endpoint);
            multiplicity += 1;
        }
    }
    let first = knots
        .iter()
        .position(|knot| *knot == lo)
        .ok_or_else(|| fail("clamped start is missing"))?;
    let last = knots
        .iter()
        .rposition(|knot| *knot == hi)
        .ok_or_else(|| fail("clamped end is missing"))?;
    if first == 0
        || last + 1 >= knots.len()
        || knots[first..first + degree].iter().any(|knot| *knot != lo)
        || knots[first + degree] == lo
        || knots[last + 1 - degree..=last]
            .iter()
            .any(|knot| *knot != hi)
        || knots[last - degree] == hi
    {
        return Err(fail("clamped endpoints lack exact full multiplicity"));
    }
    if last <= first {
        return Err(fail("clamped span holds no knots"));
    }
    // The clamped span is a B-spline over the same blossom: endpoint
    // multiplicity `degree + 1` around the interior knots, with control
    // points `Q[first - 1 ..= last - degree]`. `Q[first - 1]` is the
    // blossom at `(lo, ..., lo)` and `Q[last - degree]` the blossom at
    // `(hi, ..., hi)`, so the span starts and ends exactly where the
    // periodic curve does.
    // The endpoint multiplicity is `degree + 1` around the interior knots.
    // The repeat count follows the admitted degree, which the cardinality
    // rule keeps below the held pole count, so it cannot outgrow the input.
    let end_mult = degree
        .checked_add(1)
        .ok_or_else(|| fail("degree overflows its end multiplicity"))?;
    let mut segment_knots = Vec::with_capacity(end_mult);
    for _ in 0..end_mult {
        segment_knots.push(lo);
    }
    if first + degree <= last - degree {
        segment_knots.extend_from_slice(&knots[first + degree..=last - degree]);
    }
    segment_knots.extend(std::iter::repeat(hi).take(end_mult));
    let segment_points = homogeneous[first - 1..=last - degree].to_vec();
    if segment_knots.len() != segment_points.len() + degree + 1 {
        return Err(fail("clamped span counts disagree"));
    }
    let mut poles = Vec::with_capacity(segment_points.len());
    let mut lane = Vec::with_capacity(segment_points.len());
    for (index, point) in segment_points.iter().enumerate() {
        if point[3] == 0.0 || !point[3].is_finite() {
            return Err(fail("clamped pole weight is not nonzero finite"));
        }
        let position = Point3::new(
            point[0] / point[3],
            point[1] / point[3],
            point[2] / point[3],
        );
        poles.push(finite_point(
            position,
            &format!("edge {id} clamped pole {index}"),
        )?);
        lane.push(
            FiniteReal::new(point[3])
                .and_then(NonZeroReal::from_finite)
                .ok_or_else(|| fail("clamped pole weight is not nonzero finite"))?,
        );
    }
    let rational = lane.iter().any(|weight| weight.get() != 1.0) || curve.weights().is_some();
    let converted = if rational {
        NurbsCurve::new(
            curve.degree(),
            segment_knots,
            NurbsPoles3::Rational {
                points: poles
                    .into_iter()
                    .zip(lane)
                    .map(|(point, weight)| WeightedPole3 { point, weight })
                    .collect(),
            },
            false,
        )
    } else {
        NurbsCurve::new(
            curve.degree(),
            segment_knots,
            NurbsPoles3::Polynomial { points: poles },
            false,
        )
    }
    .map_err(|error| fail(&format!("does not admit a clamped carrier: {error}")))?;
    super::check_nurbs_curve(id, &converted).map_err(|error| match error {
        WriteError::Omit(reason) => fail(&format!(
            "clamped carrier is not native-canonical: {reason}"
        )),
        error => error,
    })?;
    Ok(converted)
}

/// Insertion span for `parameter`: the largest knot index at or below it,
/// capped at the last span start for the standard domain-end insertion.
/// Any span touching the value raises its multiplicity by exactly one.
fn locate_span(knots: &[f64], degree: usize, poles: usize, parameter: f64) -> Option<usize> {
    let mut span = None;
    for (index, knot) in knots.iter().enumerate() {
        if *knot <= parameter {
            span = Some(index);
        }
    }
    // Insertion reads poles through `span` and knots through
    // `span + degree`; the last span start holds the domain-end insertion.
    let cap = poles.checked_sub(1)?;
    let span = span?.min(cap);
    (span >= degree && span + degree < knots.len()).then_some(span)
}

/// One Boehm knot insertion at `parameter` over `span`, in homogeneous
/// coordinates.
fn insert_knot(
    knots: &mut Vec<f64>,
    homogeneous: &mut Vec<[f64; 4]>,
    degree: usize,
    span: usize,
    parameter: f64,
) {
    let blend = |index: usize| {
        let denominator = knots[index + degree] - knots[index];
        if denominator == 0.0 || !denominator.is_finite() {
            0.0
        } else {
            (parameter - knots[index]) / denominator
        }
    };
    let mut next = Vec::with_capacity(homogeneous.len() + 1);
    next.extend_from_slice(&homogeneous[..span + 1 - degree]);
    for index in span + 1 - degree..=span {
        let alpha = blend(index);
        let left = homogeneous[index - 1];
        let right = homogeneous[index];
        next.push([
            alpha.mul_add(right[0], (1.0 - alpha) * left[0]),
            alpha.mul_add(right[1], (1.0 - alpha) * left[1]),
            alpha.mul_add(right[2], (1.0 - alpha) * left[2]),
            alpha.mul_add(right[3], (1.0 - alpha) * left[3]),
        ]);
    }
    next.extend_from_slice(&homogeneous[span..]);
    *homogeneous = next;
    knots.insert(span + 1, parameter);
}
///
/// The weight is solved from the midpoint condition, then the other
/// coordinate proves it: two conics sharing endpoints, end tangents, and a
/// midpoint coincide, so the check makes the conversion self-proving.
/// The two endpoints, the tangent-intersection shoulder, and the solved
/// shoulder weight of one hyperbola piece, in model space.
///
/// The weight is solved from the midpoint condition, then the other
/// coordinate proves it: two conics sharing endpoints, end tangents, and a
/// midpoint coincide, so the check makes the conversion self-proving.
fn hyperbola_piece(
    id: &str,
    frame: &ConicFrame,
    major_radius: f64,
    minor_radius: f64,
    knots: [f64; 3],
) -> Result<([Point3; 3], f64), WriteError> {
    let fail = |reason: &str| omit(format!("edge {id} hyperbola {reason}"));
    let [start, middle, end] = knots;
    let point_2d = |parameter: f64| {
        [
            major_radius * parameter.cosh(),
            minor_radius * parameter.sinh(),
        ]
    };
    let tangent_2d = |parameter: f64| {
        [
            major_radius * parameter.sinh(),
            minor_radius * parameter.cosh(),
        ]
    };
    let [p0u, p0w] = point_2d(start);
    let [p2u, p2w] = point_2d(end);
    let [d0u, d0w] = tangent_2d(start);
    let [d1u, d1w] = tangent_2d(end);
    for value in [p0u, p0w, p2u, p2w, d0u, d0w, d1u, d1w] {
        if !value.is_finite() {
            return Err(fail("span leaves the finite range"));
        }
    }
    let determinant = d0u * d1w - d0w * d1u;
    if !determinant.is_finite() || determinant == 0.0 {
        return Err(fail("span has parallel end tangents"));
    }
    let lambda = ((p2u - p0u) * d1w - (p2w - p0w) * d1u) / determinant;
    if !lambda.is_finite() {
        return Err(fail("span shoulder is not finite"));
    }
    let shoulder = [p0u + lambda * d0u, p0w + lambda * d0w];
    let mid = point_2d(middle);
    if !mid.iter().all(|value| value.is_finite()) || !shoulder.iter().all(|value| value.is_finite())
    {
        return Err(fail("span leaves the finite range"));
    }
    // Solve the midpoint condition on the coordinate with the larger
    // shoulder separation, then prove the solved weight on the other one.
    let numerators = [p0u + p2u - 2.0 * mid[0], p0w + p2w - 2.0 * mid[1]];
    let denominators = [mid[0] - shoulder[0], mid[1] - shoulder[1]];
    let coordinate = if denominators[0].abs() >= denominators[1].abs() {
        0
    } else {
        1
    };
    if denominators[coordinate] == 0.0 || !denominators[coordinate].is_finite() {
        return Err(fail("span midpoint meets its shoulder"));
    }
    let weight = numerators[coordinate] / (2.0 * denominators[coordinate]);
    if !weight.is_finite() || weight <= 0.0 {
        return Err(fail("span shoulder weight is not positive finite"));
    }
    let other = 1 - coordinate;
    let reproduced = (if other == 0 { p0u } else { p0w }
        + 2.0 * weight * shoulder[other]
        + if other == 0 { p2u } else { p2w })
        / (2.0 + 2.0 * weight);
    let scale = mid[other].abs().max(p0u.abs()).max(p2u.abs()).max(1.0);
    if !reproduced.is_finite()
        || (reproduced - mid[other]).abs() > HYPERBOLA_MIDPOINT_TOLERANCE * scale
    {
        return Err(fail("span midpoint does not prove its weight"));
    }
    let to_model = |pair: [f64; 2]| at(frame, pair[0], pair[1]);
    Ok((
        [
            to_model([p0u, p0w]),
            to_model(shoulder),
            to_model([p2u, p2w]),
        ],
        weight,
    ))
}
