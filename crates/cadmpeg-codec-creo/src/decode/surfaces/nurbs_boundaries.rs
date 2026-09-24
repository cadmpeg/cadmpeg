// SPDX-License-Identifier: Apache-2.0
//! NURBS surface boundaries and extrusion-plane generator curves.

use crate::vecmath::normalize;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsSurface},
    CurveGeometry, SolvedCurveGeometry,
};
use cadmpeg_ir::math::Point3;

use crate::decode::analytic::equations::PlaneEquation;
use crate::decode::quadratic::{real_roots, Coefficient};
use crate::vecmath::{cross, dot};

const EPS_CUBIC_PARAM: f64 = 1.0e-11;
const EPS_BOUNDARY_EXTENT: f64 = 1.0e-9;
const EPS_WEIGHT_SYMMETRY: f64 = 1.0e-12;
const EPS_PARAMETER_AGREEMENT: f64 = 1.0e-12;
const EPS_ENDPOINT_AGREEMENT: f64 = 1.0e-12;

#[derive(Clone)]
struct NurbsSurfaceBoundary {
    curve: NurbsCurve,
    control_indices: Vec<usize>,
    transverse_periodic: bool,
}

/// The four boundary curves of one NURBS surface carrier.
///
/// `surface_id` is the `VisibGeom` surface row that stated the surface. Every
/// refusal names that row, so N refused rows stay N named records.
fn nurbs_surface_boundaries(
    nurbs: &NurbsSurface,
    surface_id: u32,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Option<[NurbsSurfaceBoundary; 4]> {
    let u_count = nurbs.u_count();
    let v_count = nurbs.v_count();
    let poles = nurbs.poles();
    let pole_weights = nurbs.pole_weights();
    pole_weights
        .as_ref()
        .is_none_or(|weights| weights.iter().all(|weight| weight.get() > 0.0))
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
            let curve = match NurbsCurve::from_checked_lanes(
                degree,
                knots,
                control_indices
                    .iter()
                    .map(|index| poles[*index])
                    .collect::<Vec<_>>(),
                pole_weights.as_ref().map(|weights| {
                    control_indices
                        .iter()
                        .map(|index| weights[*index])
                        .collect()
                }),
                periodic,
            ) {
                Ok(curve) => curve,
                Err(error) => {
                    refusal.note(
                        format!("creo VisibGeom surface row {surface_id} boundary curve record"),
                        &error,
                    );
                    return None;
                }
            };
            Some(NurbsSurfaceBoundary {
                curve,
                control_indices,
                transverse_periodic,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    boundaries.try_into().ok()
}

fn point_tolerance<'a>(points: impl Iterator<Item = &'a FinitePoint3>) -> Option<f64> {
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
    surface_id: u32,
    plane: PlaneEquation,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Option<CurveGeometry> {
    let boundaries = nurbs_surface_boundaries(nurbs, surface_id, refusal)?;
    let normal = normalize(plane.normal)?;
    let poles = nurbs.poles();
    let tolerance = point_tolerance(poles.iter())?
        .max(32.0 * f64::EPSILON * plane.origin.into_iter().map(f64::abs).fold(1.0, f64::max));
    let signed_distances = poles
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
    Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        boundary.curve.clone(),
    )))
}

fn scalar_near(left: f64, right: f64, tolerance: f64) -> bool {
    (left - right).abs() <= tolerance
}

fn normalized_knot_vector(knots: &[f64]) -> Option<Vec<f64>> {
    let (&minimum, &maximum) = knots.first().zip(knots.last())?;
    let span = maximum - minimum;
    (span.is_finite() && span > 0.0)
        .then(|| knots.iter().map(|knot| (knot - minimum) / span).collect())
}

fn nurbs_curves_match(
    left: &NurbsCurve,
    right: &NurbsCurve,
    reversed: bool,
    point_tolerance: f64,
) -> bool {
    if left.degree() != right.degree()
        || left.periodic() != right.periodic()
        || left.pole_count() != right.pole_count()
        || left.knots().len() != right.knots().len()
        || left.weights().is_some() != right.weights().is_some()
    {
        return false;
    }
    let left_points = left.control_points();
    let right_control_points = right.control_points();
    let right_points = if reversed {
        right_control_points.iter().rev().collect::<Vec<_>>()
    } else {
        right_control_points.iter().collect()
    };
    if !left_points.iter().zip(right_points).all(|(left, right)| {
        dot(
            [left.x - right.x, left.y - right.y, left.z - right.z],
            [left.x - right.x, left.y - right.y, left.z - right.z],
        )
        .sqrt()
            <= point_tolerance
    }) {
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
    match (left.pole_rows().weights(), right.pole_rows().weights()) {
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

fn generator_separates_control_nets(
    first: &NurbsSurface,
    first_boundary: &NurbsSurfaceBoundary,
    second: &NurbsSurface,
    second_boundary: &NurbsSurfaceBoundary,
) -> bool {
    let boundary_control_points = first_boundary.curve.control_points();
    let [origin, end] = boundary_control_points.as_slice() else {
        return false;
    };
    let generator = [end.x - origin.x, end.y - origin.y, end.z - origin.z];
    let Some(generator) = normalize(generator) else {
        return false;
    };
    let seed = if generator[0].abs() < 0.8 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let Some(first_axis) = normalize(cross(generator, seed)) else {
        return false;
    };
    let second_axis = cross(generator, first_axis);
    let first_poles = first.poles();
    let second_poles = second.poles();
    let first_outside = first_poles
        .iter()
        .enumerate()
        .filter(|(index, _)| !first_boundary.control_indices.contains(index))
        .map(|(_, point)| point)
        .collect::<Vec<_>>();
    let second_outside = second_poles
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
    let tolerance =
        point_tolerance(first_poles.iter().chain(second_poles.iter())).unwrap_or(f64::INFINITY);
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
    first_surface_id: u32,
    second: &NurbsSurface,
    second_surface_id: u32,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Option<CurveGeometry> {
    let first_boundaries = nurbs_surface_boundaries(first, first_surface_id, refusal)?;
    let second_boundaries = nurbs_surface_boundaries(second, second_surface_id, refusal)?;
    let first_poles = first.poles();
    let second_poles = second.poles();
    let tolerance = point_tolerance(first_poles.iter().chain(second_poles.iter()))?;
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
    Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        curve.clone(),
    )))
}

/// The power-basis coefficients `[cubic, quadratic, linear, constant]` of the
/// weighted plane distance along a cubic Bezier control polygon.
///
/// The third and second differences are the sums that state the degree: they
/// cancel to zero where the plane distance along the polygon is exactly
/// quadratic or exactly affine, which a degenerate span or a degree-elevated
/// quadratic polygon reaches, so both carry the magnitudes of their terms. The
/// first difference is one subtraction of two terms, which is exactly zero
/// whenever the exact difference is zero, and the constant is one distance, so
/// each is a single value.
fn plane_distance_coefficients(signed: [f64; 4]) -> [Coefficient; 4] {
    let [first, second, third, fourth] = signed;
    [
        Coefficient::summed(
            -first + 3.0 * second - 3.0 * third + fourth,
            first.abs() + 3.0 * second.abs() + 3.0 * third.abs() + fourth.abs(),
        ),
        Coefficient::summed(
            3.0 * first - 6.0 * second + 3.0 * third,
            3.0 * first.abs() + 6.0 * second.abs() + 3.0 * third.abs(),
        ),
        Coefficient::single(-3.0 * first + 3.0 * second),
        Coefficient::single(first),
    ]
}

/// The unit-interval parameter a root of the cubic states, or `None` where the
/// root lies outside `[0, 1]` by more than `EPS_CUBIC_PARAM`.
///
/// `EPS_CUBIC_PARAM` is the excess this problem admits, which is the parameter
/// rounding of the root solve. A root inside the excess differs from the
/// endpoint by that rounding alone, so the endpoint is the parameter it states.
/// A root outside the excess is not a parameter of this problem and states
/// nothing, so no value outside `[0, 1]` reaches the caller as a parameter.
fn unit_interval_parameter(root: f64) -> Option<f64> {
    if !(-EPS_CUBIC_PARAM..=1.0 + EPS_CUBIC_PARAM).contains(&root) {
        return None;
    }
    if root < 0.0 {
        return Some(0.0);
    }
    if root > 1.0 {
        return Some(1.0);
    }
    Some(root)
}

pub(in super::super) fn cubic_unit_interval_roots(
    cubic: Coefficient,
    quadratic: Coefficient,
    linear: Coefficient,
    constant: Coefficient,
    value_tolerance: f64,
) -> Vec<f64> {
    let [cubic_value, quadratic_value, linear_value, constant_value] =
        [cubic, quadratic, linear, constant].map(Coefficient::stated);
    let scale = cubic_value
        .abs()
        .max(quadratic_value.abs())
        .max(linear_value.abs())
        .max(constant_value.abs());
    if scale <= value_tolerance {
        return Vec::new();
    }
    let evaluate = |parameter: f64| {
        ((cubic_value * parameter + quadratic_value) * parameter + linear_value) * parameter
            + constant_value
    };
    // The cubic coefficient states the degree: it is zero where the third
    // difference of the plane distances cancels inside the error of its own
    // terms, and the problem is then the quadratic the remaining coefficients
    // state.
    if cubic_value == 0.0 {
        let mut roots = real_roots(quadratic, linear, constant)
            .into_iter()
            .filter_map(|root| {
                let parameter = unit_interval_parameter(root)?;
                (evaluate(root).abs() <= value_tolerance).then_some(parameter)
            })
            .collect::<Vec<_>>();
        roots.sort_by(f64::total_cmp);
        roots.dedup_by(|left, right| (*left - *right).abs() <= EPS_CUBIC_PARAM);
        return roots;
    }
    let mut stations = vec![0.0, 1.0];
    stations.extend(
        real_roots(
            Coefficient::summed(3.0 * cubic_value, 3.0 * cubic.terms()),
            Coefficient::summed(2.0 * quadratic_value, 2.0 * quadratic.terms()),
            linear,
        )
        .into_iter()
        .filter(|root| *root > EPS_CUBIC_PARAM && *root < 1.0 - EPS_CUBIC_PARAM),
    );
    stations.sort_by(f64::total_cmp);
    stations.dedup_by(|left, right| (*left - *right).abs() <= EPS_CUBIC_PARAM);
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
    roots.dedup_by(|left, right| (*left - *right).abs() <= EPS_CUBIC_PARAM);
    roots
}

/// Generator curve where a cubic extrusion surface meets a plane.
///
/// A refused boundary lane states no generator curve. The model carries the
/// surface and the plane without it, so the refusal is a loss note naming the
/// `VisibGeom` surface row and the decode continues with `Ok(None)`.
pub(in super::super) fn cubic_extrusion_plane_generator_curve(
    ctx: &DecodeContext<'_>,
    nurbs: &NurbsSurface,
    surface_id: u32,
    plane: PlaneEquation,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<Option<CurveGeometry>, CodecError> {
    fn recognize(
        ctx: &DecodeContext<'_>,
        nurbs: &NurbsSurface,
        surface_id: u32,
        plane: PlaneEquation,
        refusal: &mut crate::lane_refusal::LaneRefusals,
    ) -> Option<Result<CurveGeometry, CodecError>> {
        let boundaries = nurbs_surface_boundaries(nurbs, surface_id, refusal)?;
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
        let poles = nurbs.poles();
        let weights = match nurbs.pole_grid().weights() {
            Some(weights) => weights.concat(),
            None => match ctx.alloc_filled(poles.len(), 1.0, "creo_nurbs_weights") {
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
            poles[1].x - poles[0].x,
            poles[1].y - poles[0].y,
            poles[1].z - poles[0].z,
        ];
        normalize(generator)?;
        let normal = normalize(plane.normal)?;
        let tolerance = point_tolerance(poles.iter())?
            .max(32.0 * f64::EPSILON * plane.origin.into_iter().map(f64::abs).fold(1.0, f64::max));
        let structural_tolerance = 64.0
            * f64::EPSILON
            * poles
                .iter()
                .flat_map(|point| [point.x, point.y, point.z])
                .chain(plane.origin)
                .map(f64::abs)
                .fold(1.0, f64::max);
        (generator.iter().copied().all(f64::is_finite)
            && dot(normal, generator).abs() <= structural_tolerance
            && (0..4).all(|u| {
                let current = [
                    poles[2 * u + 1].x - poles[2 * u].x,
                    poles[2 * u + 1].y - poles[2 * u].y,
                    poles[2 * u + 1].z - poles[2 * u].z,
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
        let signed: [f64; 4] = std::array::from_fn(|u| {
            let point = poles[2 * u];
            weights[2 * u]
                * dot(
                    normal,
                    [
                        point.x - plane.origin[0],
                        point.y - plane.origin[1],
                        point.z - plane.origin[2],
                    ],
                )
        });
        let [cubic, quadratic, linear, constant] = plane_distance_coefficients(signed);
        let weight_scale = weights.iter().copied().fold(1.0, f64::max);
        let roots =
            cubic_unit_interval_roots(cubic, quadratic, linear, constant, tolerance * weight_scale);
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
        let evaluated = |v: usize| {
            let weight = (0..4)
                .map(|u| bernstein[u] * weights[2 * u + v])
                .sum::<f64>();
            let coordinate = |coordinate: fn(&Point3) -> f64| {
                (0..4)
                    .map(|u| {
                        bernstein[u] * weights[2 * u + v] * coordinate(poles[2 * u + v].as_raw())
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
        let curve = match NurbsCurve::from_lanes(
            curve.degree(),
            curve.knots().to_vec(),
            vec![first.0, second.0],
            nurbs.pole_weights().map(|_| vec![first.1, second.1]),
            curve.periodic(),
        ) {
            Ok(curve) => curve,
            Err(error) => {
                refusal.note(
                    format!(
                        "creo VisibGeom surface row {surface_id} cubic-extrusion plane generator \
                         curve"
                    ),
                    &error,
                );
                return None;
            }
        };
        Some(Ok(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve))))
    }
    let mut refusal = crate::lane_refusal::LaneRefusals::new();
    let recognized = recognize(ctx, nurbs, surface_id, plane, &mut refusal).transpose();
    let refused = refusal.take_records();
    if refused.is_empty() {
        return recognized;
    }
    losses.push(
        crate::loss::CreoLossCode::NurbsBoundaryCarrierUnresolved.note(format!(
            "VisibGeom surface row {surface_id} states no cubic-extrusion plane generator \
             carrier: {}",
            refused.join("; ")
        )),
    );
    Ok(None)
}

#[cfg(test)]
mod tests {
    const EPS_TEST_VALUE: f64 = 1.0e-11;
    const EPS_TEST_ROOT: f64 = 1.0e-12;

    #[test]
    fn numerical_followup_degenerate_bezier_plane_distances_state_a_zero_difference() {
        // Plane distances in arithmetic progression. The exact second and third
        // differences of -8.9, -2.0, 4.9, 11.8 are zero, and the second
        // difference computes as -1.7763568394002505e-15.
        let [cubic, quadratic, linear, constant] =
            super::plane_distance_coefficients([-8.9, -2.0, 4.9, 11.8]);
        assert_eq!([cubic.stated(), quadratic.stated()], [0.0, 0.0]);
        assert_eq!(
            [linear.stated(), constant.stated()],
            [20.700_000_000_000_003, -8.9]
        );
        assert_eq!(
            super::cubic_unit_interval_roots(cubic, quadratic, linear, constant, EPS_TEST_VALUE),
            [-constant.stated() / linear.stated()]
        );

        // Plane distances that are exactly quadratic in the polygon index. The
        // exact third difference of -0.1, -0.1, 0.0, 0.2 is zero and it computes
        // as -2.7755575615628914e-17; the second difference stays.
        let [cubic, quadratic, linear, constant] =
            super::plane_distance_coefficients([-0.1, -0.1, 0.0, 0.2]);
        assert_eq!(cubic.stated(), 0.0);
        assert_eq!(
            [quadratic.stated(), linear.stated(), constant.stated()],
            [0.300_000_000_000_000_04, 0.0, -0.1]
        );
        let roots =
            super::cubic_unit_interval_roots(cubic, quadratic, linear, constant, EPS_TEST_VALUE);
        assert_eq!(roots.len(), 1);
        assert!((roots[0] - (1.0f64 / 3.0).sqrt()).abs() <= EPS_TEST_ROOT);
    }
}
