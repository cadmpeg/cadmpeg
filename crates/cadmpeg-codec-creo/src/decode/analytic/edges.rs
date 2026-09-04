// SPDX-License-Identifier: Apache-2.0
//! Edge parameter ranges for lines, NURBS, and conics.

use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve};
use cadmpeg_ir::math::{Point3, Vector3};

use super::super::sketch::normalized;
use super::super::surfaces::curve_contains_points;

use super::equations::{cross, dot};
use super::planes::valid_positive_nurbs_curve;

const EPS_ON_CONIC: f64 = 1.0e-7;
const EPS_AGREE: f64 = 1.0e-9;
const EPS_NEAR_ZERO: f64 = 1.0e-12;

pub fn orient_line_edge_carrier(
    geometry: &mut CurveGeometry,
    points: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    if !curve_contains_points(geometry, points) {
        return None;
    }
    let CurveGeometry::Line { origin, direction } = geometry else {
        return None;
    };
    let delta: [f64; 3] = std::array::from_fn(|index| points[1][index] - points[0][index]);
    let length = dot(delta, delta).sqrt();
    let oriented = normalized(delta)?;
    *origin = Point3::new(points[0][0], points[0][1], points[0][2]);
    *direction = Vector3::new(oriented[0], oriented[1], oriented[2]);
    Some([0.0, length])
}

pub fn exact_line_edge_parameter_range(
    geometry: &CurveGeometry,
    points: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    if !curve_contains_points(geometry, points) {
        return None;
    }
    let CurveGeometry::Line { origin, direction } = geometry else {
        return None;
    };
    let direction = [direction.x, direction.y, direction.z];
    let denominator = dot(direction, direction);
    if !denominator.is_finite() || denominator <= 0.0 {
        return None;
    }
    let origin = [origin.x, origin.y, origin.z];
    let parameters = points.map(|point| {
        dot(
            std::array::from_fn(|index| point[index] - origin[index]),
            direction,
        ) / denominator
    });
    parameters
        .into_iter()
        .all(f64::is_finite)
        .then_some(if parameters[0] <= parameters[1] {
            parameters
        } else {
            [parameters[1], parameters[0]]
        })
}

pub fn point_pair_alignments(mapped: [[f64; 3]; 2], target: [[f64; 3]; 2]) -> [bool; 2] {
    let mismatch = |left: [f64; 3], right: [f64; 3]| {
        dot(
            std::array::from_fn(|index| left[index] - right[index]),
            std::array::from_fn(|index| left[index] - right[index]),
        )
        .sqrt()
    };
    let scale = mapped
        .into_iter()
        .flatten()
        .chain(target.into_iter().flatten())
        .map(f64::abs)
        .fold(1.0, f64::max);
    let tolerance = EPS_AGREE * scale;
    [
        mismatch(mapped[0], target[0]).max(mismatch(mapped[1], target[1])) <= tolerance,
        mismatch(mapped[0], target[1]).max(mismatch(mapped[1], target[0])) <= tolerance,
    ]
}

pub fn nurbs_control_extent(nurbs: &NurbsCurve) -> Option<f64> {
    let bounds = nurbs.control_points().iter().try_fold(
        [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]],
        |mut bounds, point| {
            for (index, coordinate) in [point.x, point.y, point.z].into_iter().enumerate() {
                coordinate.is_finite().then_some(())?;
                bounds[0][index] = bounds[0][index].min(coordinate);
                bounds[1][index] = bounds[1][index].max(coordinate);
            }
            Some(bounds)
        },
    )?;
    Some(
        (0..3)
            .map(|index| bounds[1][index] - bounds[0][index])
            .fold(1.0, f64::max),
    )
}

pub fn nurbs_intrinsic_parameter_range(nurbs: &NurbsCurve) -> Option<[f64; 2]> {
    let degree = usize::try_from(nurbs.degree()).ok()?;
    (nurbs_control_extent(nurbs).is_some()
        && nurbs.knots().iter().all(|knot| knot.is_finite())
        && nurbs.knots().windows(2).all(|pair| pair[0] <= pair[1]))
    .then_some(())?;
    let range = [
        *nurbs.knots().get(degree)?,
        *nurbs.knots().get(nurbs.control_points().len())?,
    ];
    (range[0] < range[1]).then_some(range)
}

pub fn nonperiodic_nurbs_endpoint_points(geometry: &CurveGeometry) -> Option<[[f64; 3]; 2]> {
    let CurveGeometry::Nurbs(nurbs) = geometry else {
        return None;
    };
    (!nurbs.periodic()).then_some(())?;
    valid_positive_nurbs_curve(nurbs)?;
    let range = nurbs_intrinsic_parameter_range(nurbs)?;
    let points = range.map(|parameter| {
        cadmpeg_ir::eval::curve_point(geometry, parameter).map(|point| [point.x, point.y, point.z])
    });
    let [Some(first), Some(second)] = points else {
        return None;
    };
    first
        .into_iter()
        .chain(second)
        .all(f64::is_finite)
        .then_some([first, second])
}

pub fn nonperiodic_nurbs_edge_parameter_range(
    geometry: &CurveGeometry,
    points: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    let CurveGeometry::Nurbs(nurbs) = geometry else {
        return None;
    };
    if nurbs.periodic() {
        return None;
    }
    let degree = usize::try_from(nurbs.degree()).ok()?;
    let range = nurbs_intrinsic_parameter_range(nurbs)?;

    if degree == 1 {
        nurbs
            .weights()
            .is_none_or(|weights| {
                weights
                    .iter()
                    .all(|weight| weight.is_finite() && *weight > 0.0)
            })
            .then_some(())?;
        let scale = nurbs_control_extent(nurbs)?;
        let tolerance = EPS_AGREE * scale;
        let first = degree_one_nurbs_point_parameter(geometry, nurbs, points[0], range, tolerance)?;
        let second =
            degree_one_nurbs_point_parameter(geometry, nurbs, points[1], range, tolerance)?;
        let parameters = if first <= second {
            [first, second]
        } else {
            [second, first]
        };
        return (parameters[1] - parameters[0] > EPS_NEAR_ZERO * (range[1] - range[0]).max(1.0))
            .then_some(parameters);
    }

    let mapped = range.map(|parameter| {
        cadmpeg_ir::eval::curve_point(geometry, parameter).map(|point| [point.x, point.y, point.z])
    });
    let [Some(first), Some(second)] = mapped else {
        return None;
    };
    match point_pair_alignments([first, second], points) {
        [true, false] | [false, true] => Some(range),
        _ => None,
    }
}

/// Orient a non-periodic NURBS carrier to the topological edge direction.
///
/// Edge parameter ranges are canonical and therefore increasing. When the
/// native edge reverses the carrier direction, reverse the NURBS definition
/// and keep the same geometric parameter domain.
pub fn orient_nonperiodic_nurbs_edge_carrier(
    geometry: &mut CurveGeometry,
    points: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    let range = nonperiodic_nurbs_edge_parameter_range(geometry, points)?;
    let CurveGeometry::Nurbs(nurbs) = &*geometry else {
        return None;
    };
    let degree = usize::try_from(nurbs.degree()).ok()?;
    let intrinsic_range = nurbs_intrinsic_parameter_range(nurbs)?;
    if degree == 1 {
        let (first, second) = {
            let CurveGeometry::Nurbs(nurbs) = &*geometry else {
                return None;
            };
            let tolerance = EPS_AGREE * nurbs_control_extent(nurbs)?;
            let first = degree_one_nurbs_point_parameter(
                &*geometry,
                nurbs,
                points[0],
                intrinsic_range,
                tolerance,
            )?;
            let second = degree_one_nurbs_point_parameter(
                &*geometry,
                nurbs,
                points[1],
                intrinsic_range,
                tolerance,
            )?;
            (first, second)
        };
        if first <= second {
            return Some([first, second]);
        }
        let CurveGeometry::Nurbs(nurbs) = geometry else {
            return None;
        };
        reverse_nonperiodic_nurbs(nurbs, intrinsic_range);
        let sum = intrinsic_range[0] + intrinsic_range[1];
        return Some([sum - first, sum - second]);
    }

    let mapped = intrinsic_range.map(|parameter| {
        cadmpeg_ir::eval::curve_point(&*geometry, parameter)
            .map(|point| [point.x, point.y, point.z])
    });
    let [Some(first), Some(second)] = mapped else {
        return None;
    };
    match point_pair_alignments([first, second], points) {
        [true, false] => Some(range),
        [false, true] => {
            let CurveGeometry::Nurbs(nurbs) = geometry else {
                return None;
            };
            reverse_nonperiodic_nurbs(nurbs, intrinsic_range);
            Some(range)
        }
        _ => None,
    }
}

fn reverse_nonperiodic_nurbs(nurbs: &mut NurbsCurve, range: [f64; 2]) {
    let sum = range[0] + range[1];
    nurbs.control_points_mut().reverse();
    if let Some(weights) = nurbs.weights_mut() {
        weights.reverse();
    }
    let knots = nurbs
        .knots()
        .iter()
        .rev()
        .map(|knot| sum - knot)
        .collect::<Vec<_>>();
    nurbs.knots_mut().copy_from_slice(&knots);
}

pub fn full_periodic_nurbs_edge_parameter_range(
    geometry: &CurveGeometry,
    point: [f64; 3],
) -> Option<[f64; 2]> {
    let CurveGeometry::Nurbs(nurbs) = geometry else {
        return None;
    };
    nurbs.periodic().then_some(())?;
    nurbs
        .weights()
        .is_none_or(|weights| {
            weights
                .iter()
                .all(|weight| weight.is_finite() && *weight > 0.0)
        })
        .then_some(())?;
    let range = nurbs_intrinsic_parameter_range(nurbs)?;
    let mapped = range.map(|parameter| {
        cadmpeg_ir::eval::curve_point(geometry, parameter).map(|point| [point.x, point.y, point.z])
    });
    let [Some(first), Some(second)] = mapped else {
        return None;
    };
    let tolerance = EPS_AGREE * nurbs_control_extent(nurbs)?;
    [first, second]
        .into_iter()
        .all(|mapped| {
            let delta: [f64; 3] = std::array::from_fn(|index| mapped[index] - point[index]);
            dot(delta, delta).sqrt() <= tolerance
        })
        .then_some(range)
}

pub fn degree_one_nurbs_point_parameter(
    geometry: &CurveGeometry,
    nurbs: &NurbsCurve,
    point: [f64; 3],
    range: [f64; 2],
    tolerance: f64,
) -> Option<f64> {
    let parameter_tolerance = EPS_AGREE * (range[1] - range[0]).max(1.0);
    let mut candidates = Vec::<f64>::new();
    for span in 1..nurbs.control_points().len() {
        let lower = nurbs.knots()[span];
        let upper = nurbs.knots()[span + 1];
        if !lower.is_finite() || !upper.is_finite() || upper <= lower {
            continue;
        }
        let first = nurbs.control_points()[span - 1];
        let second = nurbs.control_points()[span];
        let delta = [second.x - first.x, second.y - first.y, second.z - first.z];
        let denominator = dot(delta, delta);
        if !denominator.is_finite() {
            continue;
        }
        let relative = [point[0] - first.x, point[1] - first.y, point[2] - first.z];
        if denominator <= tolerance * tolerance {
            if dot(relative, relative).sqrt() <= tolerance {
                return None;
            }
            continue;
        }
        let fraction = dot(relative, delta) / denominator;
        if !(-EPS_AGREE..=1.0 + EPS_AGREE).contains(&fraction) {
            continue;
        }
        let fraction = fraction.clamp(0.0, 1.0);
        let projected = [
            first.x + fraction * delta[0],
            first.y + fraction * delta[1],
            first.z + fraction * delta[2],
        ];
        let mismatch: [f64; 3] = std::array::from_fn(|index| projected[index] - point[index]);
        if dot(mismatch, mismatch).sqrt() > tolerance {
            continue;
        }
        let first_weight = nurbs.weights().map_or(1.0, |weights| weights[span - 1]);
        let second_weight = nurbs.weights().map_or(1.0, |weights| weights[span]);
        let rational_denominator = second_weight * (1.0 - fraction) + fraction * first_weight;
        if rational_denominator <= 0.0 || !rational_denominator.is_finite() {
            continue;
        }
        let local = fraction * first_weight / rational_denominator;
        let parameter = lower + local * (upper - lower);
        let Some(mapped) = cadmpeg_ir::eval::curve_point(geometry, parameter) else {
            continue;
        };
        let mismatch = [
            mapped.x - point[0],
            mapped.y - point[1],
            mapped.z - point[2],
        ];
        if dot(mismatch, mismatch).sqrt() <= tolerance
            && !candidates
                .iter()
                .any(|known| (parameter - known).abs() <= parameter_tolerance)
        {
            candidates.push(parameter);
        }
    }
    let [parameter] = candidates.as_slice() else {
        return None;
    };
    Some(*parameter)
}

#[derive(Clone, Copy)]
pub struct PeriodicConicFrame {
    pub center: [f64; 3],
    pub normal: [f64; 3],
    pub x_axis: [f64; 3],
    pub y_axis: [f64; 3],
    pub radii: [f64; 2],
}

#[derive(Clone, Copy)]
pub struct PlanarConicEquation {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
    pub x_axis: [f64; 3],
    pub y_axis: [f64; 3],
    pub quadratic: [f64; 2],
    pub linear: [f64; 2],
    pub constant: f64,
    pub scale: f64,
}

#[derive(Clone, Copy)]
pub enum NonperiodicConicFamily {
    Parabola,
    Hyperbola,
}

#[derive(Clone, Copy)]
pub struct NonperiodicConicFrame {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
    pub x_axis: [f64; 3],
    pub y_axis: [f64; 3],
    pub x_scale: f64,
    pub y_scale: f64,
    pub family: NonperiodicConicFamily,
}

pub fn planar_conic_equation(geometry: &CurveGeometry) -> Option<PlanarConicEquation> {
    if let Some(frame) = periodic_conic_frame(geometry) {
        return Some(PlanarConicEquation {
            origin: frame.center,
            normal: frame.normal,
            x_axis: frame.x_axis,
            y_axis: frame.y_axis,
            quadratic: [1.0 / frame.radii[0].powi(2), 1.0 / frame.radii[1].powi(2)],
            linear: [0.0, 0.0],
            constant: -1.0,
            scale: frame.radii.into_iter().fold(1.0, f64::max),
        });
    }
    let NonperiodicConicFrame {
        origin,
        normal,
        x_axis,
        y_axis,
        x_scale,
        y_scale,
        family,
    } = nonperiodic_conic_frame(geometry)?;
    let (quadratic, linear, constant) = match family {
        NonperiodicConicFamily::Parabola => ([0.0, -1.0 / (2.0 * y_scale)], [1.0, 0.0], 0.0),
        NonperiodicConicFamily::Hyperbola => (
            [1.0 / x_scale.powi(2), -1.0 / y_scale.powi(2)],
            [0.0, 0.0],
            -1.0,
        ),
    };
    Some(PlanarConicEquation {
        origin,
        normal,
        x_axis,
        y_axis,
        quadratic,
        linear,
        constant,
        scale: x_scale.max(y_scale),
    })
}

pub fn nonperiodic_conic_frame(geometry: &CurveGeometry) -> Option<NonperiodicConicFrame> {
    let (origin, normal, x_axis, x_scale, y_scale, family) = match geometry {
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => (
            [vertex.x, vertex.y, vertex.z],
            [axis.x, axis.y, axis.z],
            [major_direction.x, major_direction.y, major_direction.z],
            *focal_distance,
            2.0 * *focal_distance,
            NonperiodicConicFamily::Parabola,
        ),
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => (
            [center.x, center.y, center.z],
            [axis.x, axis.y, axis.z],
            [major_direction.x, major_direction.y, major_direction.z],
            *major_radius,
            *minor_radius,
            NonperiodicConicFamily::Hyperbola,
        ),
        _ => return None,
    };
    let normal = normalized(normal)?;
    let x_axis = normalized(x_axis)?;
    (dot(normal, x_axis).abs() <= EPS_AGREE).then_some(())?;
    let y_axis = normalized(cross(normal, x_axis))?;
    (origin.into_iter().all(f64::is_finite)
        && x_scale > 0.0
        && x_scale.is_finite()
        && y_scale > 0.0
        && y_scale.is_finite())
    .then_some(())?;
    Some(NonperiodicConicFrame {
        origin,
        normal,
        x_axis,
        y_axis,
        x_scale,
        y_scale,
        family,
    })
}

pub fn periodic_conic_frame(geometry: &CurveGeometry) -> Option<PeriodicConicFrame> {
    let (center, axis, x_axis, radii) = match geometry {
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => (
            [center.x, center.y, center.z],
            [axis.x, axis.y, axis.z],
            [ref_direction.x, ref_direction.y, ref_direction.z],
            [*radius, *radius],
        ),
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => (
            [center.x, center.y, center.z],
            [axis.x, axis.y, axis.z],
            [major_direction.x, major_direction.y, major_direction.z],
            [*major_radius, *minor_radius],
        ),
        _ => return None,
    };
    let axis = normalized(axis)?;
    let x_axis = normalized(x_axis)?;
    (dot(axis, x_axis).abs() <= EPS_AGREE).then_some(())?;
    let y_axis = normalized(cross(axis, x_axis))?;
    (center.into_iter().all(f64::is_finite)
        && radii
            .into_iter()
            .all(|radius| radius > 0.0 && radius.is_finite()))
    .then_some(PeriodicConicFrame {
        center,
        normal: axis,
        x_axis,
        y_axis,
        radii,
    })
}

pub fn nonperiodic_conic_parameter(geometry: &CurveGeometry, point: [f64; 3]) -> Option<f64> {
    let NonperiodicConicFrame {
        origin,
        normal,
        x_axis,
        y_axis,
        x_scale,
        y_scale,
        family,
    } = nonperiodic_conic_frame(geometry)?;
    let relative = std::array::from_fn(|index| point[index] - origin[index]);
    let scale = dot(relative, relative)
        .sqrt()
        .max(x_scale)
        .max(y_scale)
        .max(1.0);
    (dot(relative, normal).abs() <= EPS_ON_CONIC * scale).then_some(())?;
    let x = dot(relative, x_axis);
    let y = dot(relative, y_axis);
    let parameter = match family {
        NonperiodicConicFamily::Parabola => y / y_scale,
        NonperiodicConicFamily::Hyperbola => (y / y_scale).asinh(),
    };
    let expected_x = match family {
        NonperiodicConicFamily::Parabola => x_scale * parameter * parameter,
        NonperiodicConicFamily::Hyperbola => x_scale * parameter.cosh(),
    };
    (parameter.is_finite() && (x - expected_x).abs() <= EPS_ON_CONIC * scale).then_some(parameter)
}

pub fn nonperiodic_conic_edge_parameter_range(
    geometry: &CurveGeometry,
    points: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    let [Some(first), Some(second)] =
        points.map(|point| nonperiodic_conic_parameter(geometry, point))
    else {
        return None;
    };
    let parameters = if first <= second {
        [first, second]
    } else {
        [second, first]
    };
    (parameters[1] - parameters[0] > EPS_NEAR_ZERO).then_some(parameters)
}

pub fn periodic_conic_edge_parameter_range(
    geometry: &CurveGeometry,
    points: [[f64; 3]; 2],
    interior: [f64; 3],
) -> Option<[f64; 2]> {
    if !curve_contains_points(geometry, points)
        || !curve_contains_points(geometry, [interior, interior])
    {
        return None;
    }
    let PeriodicConicFrame {
        center,
        x_axis,
        y_axis,
        radii,
        ..
    } = periodic_conic_frame(geometry)?;
    let parameter = |point: [f64; 3]| {
        let relative = std::array::from_fn(|index| point[index] - center[index]);
        (dot(relative, y_axis) / radii[1])
            .atan2(dot(relative, x_axis) / radii[0])
            .rem_euclid(std::f64::consts::TAU)
    };
    let [first, second] = points.map(parameter);
    let increasing = |start: f64, end: f64| {
        [
            start,
            if end < start {
                end + std::f64::consts::TAU
            } else {
                end
            },
        ]
    };
    let first_arc = increasing(first, second);
    let second_arc = if (first - second).abs() <= EPS_NEAR_ZERO {
        [first, first + std::f64::consts::TAU]
    } else {
        increasing(second, first)
    };
    let scale = radii.into_iter().fold(1.0, f64::max);
    let matches_interior = |range: [f64; 2]| {
        cadmpeg_ir::eval::curve_point(geometry, f64::midpoint(range[0], range[1])).is_some_and(
            |point| {
                let point = [point.x, point.y, point.z];
                dot(
                    std::array::from_fn(|index| point[index] - interior[index]),
                    std::array::from_fn(|index| point[index] - interior[index]),
                )
                .sqrt()
                    <= EPS_AGREE * scale
            },
        )
    };
    let selected = match (matches_interior(first_arc), matches_interior(second_arc)) {
        (true, false) => first_arc,
        (false, true) => second_arc,
        _ => return None,
    };
    (selected[1] - selected[0] > EPS_NEAR_ZERO).then_some(selected)
}

pub fn full_periodic_conic_edge_parameter_range(
    geometry: &CurveGeometry,
    point: [f64; 3],
) -> Option<[f64; 2]> {
    curve_contains_points(geometry, [point, point]).then_some(())?;
    let PeriodicConicFrame {
        center,
        x_axis,
        y_axis,
        radii,
        ..
    } = periodic_conic_frame(geometry)?;
    let relative = std::array::from_fn(|index| point[index] - center[index]);
    let start = (dot(relative, y_axis) / radii[1])
        .atan2(dot(relative, x_axis) / radii[0])
        .rem_euclid(std::f64::consts::TAU);
    Some([start, start + std::f64::consts::TAU])
}
