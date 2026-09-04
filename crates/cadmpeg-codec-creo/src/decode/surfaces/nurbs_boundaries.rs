// SPDX-License-Identifier: Apache-2.0
//! NURBS surface boundaries and extrusion-plane generator curves.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve, NurbsSurface};
use cadmpeg_ir::math::Point3;

use super::super::analytic::{cross, dot, quadratic_real_roots, PlaneEquation};
use super::super::sketch::normalized;

const EPS_CUBIC_PARAM: f64 = 1.0e-11;
const EPS_BOUNDARY_EXTENT: f64 = 1.0e-9;
const EPS_WEIGHT_SYMMETRY: f64 = 1.0e-12;
const EPS_PARAMETER_AGREEMENT: f64 = 1.0e-12;
const EPS_ENDPOINT_AGREEMENT: f64 = 1.0e-12;

#[derive(Clone)]
pub(in super::super) struct NurbsSurfaceBoundary {
    pub(super) curve: NurbsCurve,
    pub(super) control_indices: Vec<usize>,
    pub(super) transverse_periodic: bool,
}

pub(in super::super) fn nurbs_surface_boundaries(
    nurbs: &NurbsSurface,
) -> Option<[NurbsSurfaceBoundary; 4]> {
    let u_count = usize::try_from(nurbs.u_count()).ok()?;
    let v_count = usize::try_from(nurbs.v_count()).ok()?;
    (nurbs
        .control_points()
        .iter()
        .all(|point| point.x.is_finite() && point.y.is_finite() && point.z.is_finite())
        && nurbs.weights().is_none_or(|weights| {
            weights
                .iter()
                .all(|weight| weight.is_finite() && *weight > 0.0)
        }))
    .then_some(())?;
    let boundaries = [
        (false, (0..v_count).collect::<Vec<_>>()),
        (
            false,
            ((u_count - 1) * v_count..u_count * v_count).collect(),
        ),
        (true, (0..u_count).map(|u| u * v_count).collect()),
        (
            true,
            (0..u_count).map(|u| u * v_count + v_count - 1).collect(),
        ),
    ];
    let boundaries = boundaries
        .into_iter()
        .map(|(along_u, control_indices)| {
            let (degree, knots, periodic, transverse_periodic) = if along_u {
                (
                    nurbs.u_degree(),
                    nurbs.u_knots().to_vec(),
                    nurbs.u_periodic(),
                    nurbs.v_periodic(),
                )
            } else {
                (
                    nurbs.v_degree(),
                    nurbs.v_knots().to_vec(),
                    nurbs.v_periodic(),
                    nurbs.u_periodic(),
                )
            };
            Some(NurbsSurfaceBoundary {
                curve: NurbsCurve::new(
                    degree,
                    knots,
                    control_indices
                        .iter()
                        .map(|index| nurbs.control_points()[*index])
                        .collect(),
                    nurbs.weights().map(|weights| {
                        control_indices
                            .iter()
                            .map(|index| weights[*index])
                            .collect()
                    }),
                    periodic,
                )
                .ok()?,
                control_indices,
                transverse_periodic,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    boundaries.try_into().ok()
}

pub(in super::super) fn point_tolerance<'a>(
    points: impl Iterator<Item = &'a Point3>,
) -> Option<f64> {
    let points = points.collect::<Vec<_>>();
    let anchor = **points.first()?;
    let extent = points
        .iter()
        .flat_map(|point| [point.x - anchor.x, point.y - anchor.y, point.z - anchor.z])
        .map(f64::abs)
        .fold(1.0, f64::max);
    let coordinate_scale = points
        .iter()
        .flat_map(|point| [point.x, point.y, point.z])
        .map(f64::abs)
        .fold(1.0, f64::max);
    Some((EPS_BOUNDARY_EXTENT * extent).max(32.0 * f64::EPSILON * coordinate_scale))
}

pub(in super::super) fn nurbs_plane_boundary_curve(
    nurbs: &NurbsSurface,
    plane: PlaneEquation,
) -> Option<CurveGeometry> {
    let boundaries = nurbs_surface_boundaries(nurbs)?;
    let normal = normalized(plane.normal)?;
    let tolerance = point_tolerance(nurbs.control_points().iter())?
        .max(32.0 * f64::EPSILON * plane.origin.into_iter().map(f64::abs).fold(1.0, f64::max));
    let signed_distances = nurbs
        .control_points()
        .iter()
        .map(|point| {
            dot(
                normal,
                [
                    point.x - plane.origin[0],
                    point.y - plane.origin[1],
                    point.z - plane.origin[2],
                ],
            )
        })
        .collect::<Vec<_>>();
    signed_distances
        .iter()
        .all(|distance| distance.is_finite())
        .then_some(())?;
    let candidates = boundaries
        .into_iter()
        .filter(|boundary| {
            !boundary.transverse_periodic
                && boundary
                    .control_indices
                    .iter()
                    .all(|index| signed_distances[*index].abs() <= tolerance)
                && {
                    let outside = signed_distances
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| !boundary.control_indices.contains(index))
                        .map(|(_, distance)| *distance)
                        .collect::<Vec<_>>();
                    !outside.is_empty()
                        && (outside.iter().all(|distance| *distance > tolerance)
                            || outside.iter().all(|distance| *distance < -tolerance))
                }
        })
        .collect::<Vec<_>>();
    let [boundary] = candidates.as_slice() else {
        return None;
    };
    Some(CurveGeometry::Nurbs(boundary.curve.clone()))
}

pub(in super::super) fn scalar_near(left: f64, right: f64, tolerance: f64) -> bool {
    (left - right).abs() <= tolerance
}

pub(in super::super) fn normalized_knot_vector(knots: &[f64]) -> Option<Vec<f64>> {
    let (&minimum, &maximum) = knots.first().zip(knots.last())?;
    let span = maximum - minimum;
    (span.is_finite() && span > 0.0)
        .then(|| knots.iter().map(|knot| (knot - minimum) / span).collect())
}

pub(in super::super) fn nurbs_curves_match(
    left: &NurbsCurve,
    right: &NurbsCurve,
    reversed: bool,
    point_tolerance: f64,
) -> bool {
    if left.degree() != right.degree()
        || left.periodic() != right.periodic()
        || left.control_points().len() != right.control_points().len()
        || left.knots().len() != right.knots().len()
        || left.weights().is_some() != right.weights().is_some()
    {
        return false;
    }
    let right_points = if reversed {
        right.control_points().iter().rev().collect::<Vec<_>>()
    } else {
        right.control_points().iter().collect()
    };
    if !left
        .control_points()
        .iter()
        .zip(right_points)
        .all(|(left, right)| {
            dot(
                [left.x - right.x, left.y - right.y, left.z - right.z],
                [left.x - right.x, left.y - right.y, left.z - right.z],
            )
            .sqrt()
                <= point_tolerance
        })
    {
        return false;
    }
    let Some(left_knots) = normalized_knot_vector(left.knots()) else {
        return false;
    };
    let Some(right_knots) = normalized_knot_vector(right.knots()) else {
        return false;
    };
    let knots_match = if reversed {
        left_knots
            .iter()
            .zip(right_knots.iter().rev())
            .all(|(left, right)| scalar_near(*left, 1.0 - right, EPS_WEIGHT_SYMMETRY))
    } else {
        left_knots
            .iter()
            .zip(&right_knots)
            .all(|(left, right)| scalar_near(*left, *right, EPS_WEIGHT_SYMMETRY))
    };
    if !knots_match {
        return false;
    }
    match (left.weights(), right.weights()) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            let right = if reversed {
                right.iter().rev().collect::<Vec<_>>()
            } else {
                right.iter().collect()
            };
            let Some(scale) = left
                .first()
                .zip(right.first())
                .map(|(left, right)| left / **right)
            else {
                return false;
            };
            scale.is_finite()
                && scale > 0.0
                && left.iter().zip(right).all(|(left, right)| {
                    scalar_near(
                        *left,
                        scale * right,
                        EPS_PARAMETER_AGREEMENT * left.abs().max((scale * right).abs()).max(1.0),
                    )
                })
        }
        _ => false,
    }
}

pub(in super::super) fn generator_separates_control_nets(
    first: &NurbsSurface,
    first_boundary: &NurbsSurfaceBoundary,
    second: &NurbsSurface,
    second_boundary: &NurbsSurfaceBoundary,
) -> bool {
    let [origin, end] = first_boundary.curve.control_points() else {
        return false;
    };
    let generator = [end.x - origin.x, end.y - origin.y, end.z - origin.z];
    let Some(generator) = normalized(generator) else {
        return false;
    };
    let seed = if generator[0].abs() < 0.8 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let Some(first_axis) = normalized(cross(generator, seed)) else {
        return false;
    };
    let second_axis = cross(generator, first_axis);
    let first_outside = first
        .control_points()
        .iter()
        .enumerate()
        .filter(|(index, _)| !first_boundary.control_indices.contains(index))
        .map(|(_, point)| point)
        .collect::<Vec<_>>();
    let second_outside = second
        .control_points()
        .iter()
        .enumerate()
        .filter(|(index, _)| !second_boundary.control_indices.contains(index))
        .map(|(_, point)| point)
        .collect::<Vec<_>>();
    if first_outside.is_empty() || second_outside.is_empty() {
        return false;
    }
    let offset = |point: &Point3| [point.x - origin.x, point.y - origin.y, point.z - origin.z];
    let mut boundary_angles = first_outside
        .iter()
        .chain(&second_outside)
        .flat_map(|point| {
            let offset = offset(point);
            let angle = dot(second_axis, offset).atan2(dot(first_axis, offset));
            [
                (angle + std::f64::consts::FRAC_PI_2).rem_euclid(std::f64::consts::TAU),
                (angle - std::f64::consts::FRAC_PI_2).rem_euclid(std::f64::consts::TAU),
            ]
        })
        .collect::<Vec<_>>();
    boundary_angles.sort_by(f64::total_cmp);
    let tolerance = point_tolerance(first.control_points().iter().chain(second.control_points()))
        .unwrap_or(f64::INFINITY);
    (0..boundary_angles.len()).any(|index| {
        let start = boundary_angles[index];
        let end = if index + 1 == boundary_angles.len() {
            boundary_angles[0] + std::f64::consts::TAU
        } else {
            boundary_angles[index + 1]
        };
        let angle = f64::midpoint(start, end);
        let normal = [
            angle.cos() * first_axis[0] + angle.sin() * second_axis[0],
            angle.cos() * first_axis[1] + angle.sin() * second_axis[1],
            angle.cos() * first_axis[2] + angle.sin() * second_axis[2],
        ];
        let first_distances = first_outside
            .iter()
            .map(|point| dot(normal, offset(point)))
            .collect::<Vec<_>>();
        let second_distances = second_outside
            .iter()
            .map(|point| dot(normal, offset(point)))
            .collect::<Vec<_>>();
        (first_distances.iter().all(|distance| *distance > tolerance)
            && second_distances
                .iter()
                .all(|distance| *distance < -tolerance))
            || (first_distances
                .iter()
                .all(|distance| *distance < -tolerance)
                && second_distances
                    .iter()
                    .all(|distance| *distance > tolerance))
    })
}

pub(in super::super) fn shared_extrusion_generator_curve(
    first: &NurbsSurface,
    second: &NurbsSurface,
) -> Option<CurveGeometry> {
    let first_boundaries = nurbs_surface_boundaries(first)?;
    let second_boundaries = nurbs_surface_boundaries(second)?;
    let tolerance = point_tolerance(first.control_points().iter().chain(second.control_points()))?;
    let candidates = first_boundaries
        .iter()
        .flat_map(|first_boundary| {
            second_boundaries
                .iter()
                .filter(|second_boundary| {
                    first_boundary.curve.degree() == 1
                        && !first_boundary.curve.periodic()
                        && !first_boundary.transverse_periodic
                        && !second_boundary.transverse_periodic
                        && first_boundary.curve.control_points().len() == 2
                        && [false, true].into_iter().any(|reversed| {
                            nurbs_curves_match(
                                &first_boundary.curve,
                                &second_boundary.curve,
                                reversed,
                                tolerance,
                            )
                        })
                        && generator_separates_control_nets(
                            first,
                            first_boundary,
                            second,
                            second_boundary,
                        )
                })
                .map(|_| first_boundary.curve.clone())
        })
        .collect::<Vec<_>>();
    let [curve] = candidates.as_slice() else {
        return None;
    };
    Some(CurveGeometry::Nurbs(curve.clone()))
}

pub(in super::super) fn cubic_unit_interval_roots(
    cubic: f64,
    quadratic: f64,
    linear: f64,
    constant: f64,
    value_tolerance: f64,
) -> Vec<f64> {
    let scale = cubic
        .abs()
        .max(quadratic.abs())
        .max(linear.abs())
        .max(constant.abs());
    if scale <= value_tolerance {
        return Vec::new();
    }
    let parameter_tolerance = EPS_CUBIC_PARAM;
    let evaluate = |parameter: f64| {
        ((cubic * parameter + quadratic) * parameter + linear) * parameter + constant
    };
    if cubic.abs() <= 1e-14 * scale {
        let mut roots = quadratic_real_roots(quadratic, linear, constant)
            .into_iter()
            .filter(|root| {
                *root >= -parameter_tolerance
                    && *root <= 1.0 + parameter_tolerance
                    && evaluate(*root).abs() <= value_tolerance
            })
            .map(|root| root.clamp(0.0, 1.0))
            .collect::<Vec<_>>();
        roots.sort_by(f64::total_cmp);
        roots.dedup_by(|left, right| (*left - *right).abs() <= parameter_tolerance);
        return roots;
    }
    let mut stations = vec![0.0, 1.0];
    stations.extend(
        quadratic_real_roots(3.0 * cubic, 2.0 * quadratic, linear)
            .into_iter()
            .filter(|root| *root > parameter_tolerance && *root < 1.0 - parameter_tolerance),
    );
    stations.sort_by(f64::total_cmp);
    stations.dedup_by(|left, right| (*left - *right).abs() <= parameter_tolerance);
    let mut roots = stations
        .iter()
        .copied()
        .filter(|station| evaluate(*station).abs() <= value_tolerance)
        .collect::<Vec<_>>();
    for interval in stations.windows(2) {
        let [mut left, mut right] = *interval else {
            continue;
        };
        let mut left_value = evaluate(left);
        let right_value = evaluate(right);
        if left_value.abs() <= value_tolerance
            || right_value.abs() <= value_tolerance
            || left_value.is_sign_positive() == right_value.is_sign_positive()
        {
            continue;
        }
        for _ in 0..64 {
            let middle = f64::midpoint(left, right);
            let middle_value = evaluate(middle);
            if middle_value == 0.0 {
                left = middle;
                right = middle;
                break;
            }
            if left_value.is_sign_positive() == middle_value.is_sign_positive() {
                left = middle;
                left_value = middle_value;
            } else {
                right = middle;
            }
        }
        roots.push(f64::midpoint(left, right));
    }
    roots.sort_by(f64::total_cmp);
    roots.dedup_by(|left, right| (*left - *right).abs() <= parameter_tolerance);
    roots
}

pub(in super::super) fn cubic_extrusion_plane_generator_curve(
    ctx: &DecodeContext<'_>,
    nurbs: &NurbsSurface,
    plane: PlaneEquation,
) -> Result<Option<CurveGeometry>, CodecError> {
    fn recognize(
        ctx: &DecodeContext<'_>,
        nurbs: &NurbsSurface,
        plane: PlaneEquation,
    ) -> Option<Result<CurveGeometry, CodecError>> {
        let boundaries = nurbs_surface_boundaries(nurbs)?;
        (nurbs.u_degree() == 3
            && nurbs.v_degree() == 1
            && nurbs.u_count() == 4
            && nurbs.v_count() == 2
            && !nurbs.u_periodic()
            && !nurbs.v_periodic())
        .then_some(())?;
        let u_knots = normalized_knot_vector(nurbs.u_knots())?;
        let v_knots = normalized_knot_vector(nurbs.v_knots())?;
        (u_knots.len() == 8
            && u_knots
                .iter()
                .zip([0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0])
                .all(|(actual, expected)| scalar_near(*actual, expected, EPS_ENDPOINT_AGREEMENT))
            && v_knots.len() == 4
            && v_knots
                .iter()
                .zip([0.0, 0.0, 1.0, 1.0])
                .all(|(actual, expected)| scalar_near(*actual, expected, EPS_ENDPOINT_AGREEMENT)))
        .then_some(())?;
        let weights = match nurbs.weights() {
            Some(weights) => weights.to_vec(),
            None => match ctx.alloc_filled(nurbs.control_points().len(), 1.0, "creo_nurbs_weights")
            {
                Ok(weights) => weights,
                Err(error) => return Some(Err(error)),
            },
        };
        (0..4)
            .all(|u| {
                scalar_near(
                    weights[2 * u],
                    weights[2 * u + 1],
                    EPS_WEIGHT_SYMMETRY * weights[2 * u],
                )
            })
            .then_some(())?;
        let generator = [
            nurbs.control_points()[1].x - nurbs.control_points()[0].x,
            nurbs.control_points()[1].y - nurbs.control_points()[0].y,
            nurbs.control_points()[1].z - nurbs.control_points()[0].z,
        ];
        normalized(generator)?;
        let normal = normalized(plane.normal)?;
        let tolerance = point_tolerance(nurbs.control_points().iter())?
            .max(32.0 * f64::EPSILON * plane.origin.into_iter().map(f64::abs).fold(1.0, f64::max));
        let structural_tolerance = 64.0
            * f64::EPSILON
            * nurbs
                .control_points()
                .iter()
                .flat_map(|point| [point.x, point.y, point.z])
                .chain(plane.origin)
                .map(f64::abs)
                .fold(1.0, f64::max);
        (generator.iter().copied().all(f64::is_finite)
            && dot(normal, generator).abs() <= structural_tolerance
            && (0..4).all(|u| {
                let current = [
                    nurbs.control_points()[2 * u + 1].x - nurbs.control_points()[2 * u].x,
                    nurbs.control_points()[2 * u + 1].y - nurbs.control_points()[2 * u].y,
                    nurbs.control_points()[2 * u + 1].z - nurbs.control_points()[2 * u].z,
                ];
                dot(
                    [
                        current[0] - generator[0],
                        current[1] - generator[1],
                        current[2] - generator[2],
                    ],
                    [
                        current[0] - generator[0],
                        current[1] - generator[1],
                        current[2] - generator[2],
                    ],
                )
                .sqrt()
                    <= structural_tolerance
            }))
        .then_some(())?;
        let signed = (0..4)
            .map(|u| {
                let point = nurbs.control_points()[2 * u];
                weights[2 * u]
                    * dot(
                        normal,
                        [
                            point.x - plane.origin[0],
                            point.y - plane.origin[1],
                            point.z - plane.origin[2],
                        ],
                    )
            })
            .collect::<Vec<_>>();
        let cubic = -signed[0] + 3.0 * signed[1] - 3.0 * signed[2] + signed[3];
        let quadratic = 3.0 * signed[0] - 6.0 * signed[1] + 3.0 * signed[2];
        let linear = -3.0 * signed[0] + 3.0 * signed[1];
        let weight_scale = weights.iter().copied().fold(1.0, f64::max);
        let roots = cubic_unit_interval_roots(
            cubic,
            quadratic,
            linear,
            signed[0],
            tolerance * weight_scale,
        );
        let [parameter] = roots.as_slice() else {
            return None;
        };
        let parameter = *parameter;
        let bernstein = [
            (1.0 - parameter).powi(3),
            3.0 * parameter * (1.0 - parameter).powi(2),
            3.0 * parameter.powi(2) * (1.0 - parameter),
            parameter.powi(3),
        ];
        let evaluated = |v| {
            let weight = (0..4)
                .map(|u| bernstein[u] * weights[2 * u + v])
                .sum::<f64>();
            let coordinate = |coordinate: fn(&Point3) -> f64| {
                (0..4)
                    .map(|u| {
                        bernstein[u]
                            * weights[2 * u + v]
                            * coordinate(&nurbs.control_points()[2 * u + v])
                    })
                    .sum::<f64>()
                    / weight
            };
            (
                Point3::new(
                    coordinate(|point| point.x),
                    coordinate(|point| point.y),
                    coordinate(|point| point.z),
                ),
                weight,
            )
        };
        let first = evaluated(0);
        let second = evaluated(1);
        let curve = &boundaries[0].curve;
        let curve = NurbsCurve::new(
            curve.degree(),
            curve.knots().to_vec(),
            vec![first.0, second.0],
            nurbs.weights().map(|_| vec![first.1, second.1]),
            curve.periodic(),
        )
        .ok()?;
        Some(Ok(CurveGeometry::Nurbs(curve)))
    }
    recognize(ctx, nurbs, plane).transpose()
}
