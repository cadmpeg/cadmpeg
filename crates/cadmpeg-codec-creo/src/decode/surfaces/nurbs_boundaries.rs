// SPDX-License-Identifier: Apache-2.0
//! NURBS surface boundaries and extrusion-plane generator curves.

use crate::vecmath::normalize;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface},
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

struct JoinedRefusals<'a>(&'a [String]);

impl std::fmt::Display for JoinedRefusals<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, record) in self.0.iter().enumerate() {
            if index != 0 {
                formatter.write_str("; ")?;
            }
            formatter.write_str(record)?;
        }
        Ok(())
    }
}

fn report_cubic_generator_loss(
    ctx: &DecodeContext<'_>,
    surface_id: u32,
    refused: &[String],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let message = ctx.format_retained(
        format_args!(
            "VisibGeom surface row {surface_id} states no cubic-extrusion plane generator \
             carrier: {}",
            JoinedRefusals(refused)
        ),
        "creo cubic generator loss message",
    )?;
    ctx.try_reserve_items(losses, 1, "creo cubic generator loss notes")?;
    losses.push(crate::loss::CreoLossCode::NurbsBoundaryCarrierUnresolved.note(message));
    Ok(())
}

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
    ctx: &DecodeContext<'_>,
    nurbs: &NurbsSurface,
    surface_id: u32,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<[NurbsSurfaceBoundary; 4]>, CodecError> {
    let u_count = nurbs.u_count();
    let v_count = nurbs.v_count();
    let (Some(last_u), Some(last_v)) = (u_count.checked_sub(1), v_count.checked_sub(1)) else {
        return Ok(None);
    };
    let weighted = matches!(nurbs.pole_grid(), NurbsPoleGrid::Rational { .. });
    if weighted
        && (0..u_count).any(|u| {
            (0..v_count).any(|v| nurbs.weight(u, v).is_none_or(|weight| weight.get() <= 0.0))
        })
    {
        return Ok(None);
    }
    let mut boundaries = [None, None, None, None];
    for (ordinal, (along_u, fixed_index)) in
        [(false, 0), (false, last_u), (true, 0), (true, last_v)]
            .into_iter()
            .enumerate()
    {
        let count = if along_u { u_count } else { v_count };
        let mut control_indices = Vec::new();
        ctx.try_reserve_items(
            &mut control_indices,
            count,
            "creo NURBS boundary control indices",
        )?;
        let mut control_points = Vec::new();
        ctx.try_reserve_items(&mut control_points, count, "creo NURBS boundary control points")?;
        let mut weights = if weighted {
            let mut weights = Vec::new();
            ctx.try_reserve_items(&mut weights, count, "creo NURBS boundary weights")?;
            Some(weights)
        } else {
            None
        };
        for position in 0..count {
            let (u, v) = if along_u {
                (position, fixed_index)
            } else {
                (fixed_index, position)
            };
            control_indices.push(u * v_count + v);
            let Some(point) = nurbs.pole(u, v) else {
                return Ok(None);
            };
            control_points.push(point);
            if let Some(weights) = &mut weights {
                let Some(weight) = nurbs.weight(u, v) else {
                    return Ok(None);
                };
                weights.push(weight);
            }
        }
        let (degree, source_knots, periodic, transverse_periodic) = if along_u {
            (
                nurbs.u_degree(),
                nurbs.u_knots(),
                nurbs.u_periodic(),
                nurbs.v_periodic(),
            )
        } else {
            (
                nurbs.v_degree(),
                nurbs.v_knots(),
                nurbs.v_periodic(),
                nurbs.u_periodic(),
            )
        };
        let knots = ctx.try_collection(
            source_knots.len(),
            "creo NURBS boundary knots",
            || source_knots.try_clone(),
        )?;
        let poles = if let Some(weights) = weights {
            let mut paired = Vec::new();
            ctx.try_reserve_items(&mut paired, control_points.len(), "creo NURBS boundary paired poles")?;
            paired.extend(control_points.into_iter().zip(weights).map(|(point, weight)|
                cadmpeg_ir::geometry::nurbs::WeightedPole3 { point, weight }));
            cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points: paired }
        } else {
            cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points: control_points }
        };
        let curve = match NurbsCurve::new_admitted_poles(
            degree,
            knots,
            poles,
            periodic,
        ) {
            Ok(curve) => curve,
            Err(error) => {
                refusal.note_checked(ctx,
                    format_args!("creo VisibGeom surface row {surface_id} boundary curve record"),
                    &error,
                );
                return Ok(None);
            }
        };
        boundaries[ordinal] = Some(NurbsSurfaceBoundary {
            curve,
            control_indices,
            transverse_periodic,
        });
    }
    let [Some(first), Some(second), Some(third), Some(fourth)] = boundaries else {
        return Ok(None);
    };
    Ok(Some([first, second, third, fourth]))
}

fn surface_poles(nurbs: &NurbsSurface) -> impl Iterator<Item = FinitePoint3> + '_ {
    (0..nurbs.u_count()).flat_map(move |u| {
        (0..nurbs.v_count()).filter_map(move |v| nurbs.pole(u, v))
    })
}

fn point_tolerance(mut points: impl Iterator<Item = FinitePoint3>) -> Option<f64> {
    let anchor = points.next()?;
    let mut extent = 1.0_f64;
    let mut coordinate_scale = 1.0_f64;
    for point in std::iter::once(anchor).chain(points) {
        for component in [point.x - anchor.x, point.y - anchor.y, point.z - anchor.z] {
            extent = extent.max(component.abs());
        }
        for component in [point.x, point.y, point.z] {
            coordinate_scale = coordinate_scale.max(component.abs());
        }
    }
    Some((EPS_BOUNDARY_EXTENT * extent).max(32.0 * f64::EPSILON * coordinate_scale))
}

pub(in super::super) fn nurbs_plane_boundary_curve(
    ctx: &DecodeContext<'_>,
    nurbs: &NurbsSurface,
    surface_id: u32,
    plane: PlaneEquation,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<CurveGeometry>, CodecError> {
    let Some(boundaries) = nurbs_surface_boundaries(ctx, nurbs, surface_id, refusal)? else {
        return Ok(None);
    };
    Ok((|| {
    let normal = normalize(plane.normal)?;
    let tolerance = point_tolerance(surface_poles(nurbs))?
        .max(32.0 * f64::EPSILON * plane.origin.into_iter().map(f64::abs).fold(1.0, f64::max));
    let signed_distance = |point: &Point3| {
        dot(
            normal,
            [
                point.x - plane.origin[0],
                point.y - plane.origin[1],
                point.z - plane.origin[2],
            ],
        )
    };
    surface_poles(nurbs)
        .all(|point| signed_distance(&point).is_finite())
        .then_some(())?;
    let v_count = nurbs.v_count();
    (v_count > 0).then_some(())?;
    let mut candidates = boundaries
        .into_iter()
        .filter(|boundary| {
            !boundary.transverse_periodic
                && boundary
                    .control_indices
                    .iter()
                    .all(|index| nurbs.pole(*index / v_count, *index % v_count)
                        .is_some_and(|point| signed_distance(&point).abs() <= tolerance))
                && {
                    let outside = || surface_poles(nurbs)
                        .enumerate()
                        .filter(|(index, _)| !boundary.control_indices.contains(index))
                        .map(|(_, point)| signed_distance(&point));
                    outside().next().is_some()
                        && (outside().all(|distance| distance > tolerance)
                            || outside().all(|distance| distance < -tolerance))
                }
        });
    let boundary = candidates.next()?;
    candidates.next().is_none().then_some(())?;
    Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        boundary.curve,
    )))
    })())
}

fn scalar_near(left: f64, right: f64, tolerance: f64) -> bool {
    (left - right).abs() <= tolerance
}

fn normalized_knot_bounds(knots: &[f64]) -> Option<(f64, f64)> {
    let (&minimum, &maximum) = knots.first().zip(knots.last())?;
    let span = maximum - minimum;
    (span.is_finite() && span > 0.0).then_some((minimum, span))
}

fn curve_point(curve: &NurbsCurve, index: usize) -> Option<FinitePoint3> {
    match curve.pole_rows() {
        NurbsPoles3::Polynomial { points } => points.get(index).copied(),
        NurbsPoles3::Rational { points } => points.get(index).map(|pole| pole.point),
    }
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
        || matches!(left.pole_rows(), NurbsPoles3::Rational { .. })
            != matches!(right.pole_rows(), NurbsPoles3::Rational { .. })
    {
        return false;
    }
    let points_match = |left: &FinitePoint3, right: &FinitePoint3| {
        dot(
            [left.x - right.x, left.y - right.y, left.z - right.z],
            [left.x - right.x, left.y - right.y, left.z - right.z],
        )
        .sqrt()
            <= point_tolerance
    };
    if !(0..left.pole_count()).all(|index| {
        let right_index = if reversed {
            right.pole_count() - 1 - index
        } else {
            index
        };
        curve_point(left, index)
            .zip(curve_point(right, right_index))
            .is_some_and(|(left, right)| points_match(&left, &right))
    }) {
        return false;
    }
    let Some((left_minimum, left_span)) = normalized_knot_bounds(left.knots()) else {
        return false;
    };
    let Some((right_minimum, right_span)) = normalized_knot_bounds(right.knots()) else {
        return false;
    };
    let normalized = |left: &f64, right: &f64| {
        (
            (*left - left_minimum) / left_span,
            (*right - right_minimum) / right_span,
        )
    };
    let knots_match = if reversed {
        left.knots()
            .iter()
            .zip(right.knots().iter().rev())
            .all(|(left, right)| {
                let (left, right) = normalized(left, right);
                scalar_near(left, 1.0 - right, EPS_WEIGHT_SYMMETRY)
            })
    } else {
        left.knots()
            .iter()
            .zip(right.knots().iter())
            .all(|(left, right)| {
                let (left, right) = normalized(left, right);
                scalar_near(left, right, EPS_WEIGHT_SYMMETRY)
            })
    };
    if !knots_match {
        return false;
    }
    match (left.pole_rows(), right.pole_rows()) {
        (NurbsPoles3::Polynomial { .. }, NurbsPoles3::Polynomial { .. }) => true,
        (NurbsPoles3::Rational { points: left }, NurbsPoles3::Rational { points: right }) => {
            let right_first = if reversed {
                right.last()
            } else {
                right.first()
            };
            let Some(scale) = left
                .first()
                .zip(right_first)
                .map(|(left, right)| left.weight.get() / right.weight.get())
            else {
                return false;
            };
            scale.is_finite()
                && scale > 0.0
                && if reversed {
                    left.iter().zip(right.iter().rev()).all(|(left, right)| {
                        let left = left.weight.get();
                        let right = right.weight.get();
                        scalar_near(
                            left,
                            scale * right,
                            EPS_PARAMETER_AGREEMENT * left.abs().max((scale * right).abs()).max(1.0),
                        )
                    })
                } else {
                    left.iter().zip(right.iter()).all(|(left, right)| {
                    let left = left.weight.get();
                    let right = right.weight.get();
                    scalar_near(
                        left,
                        scale * right,
                        EPS_PARAMETER_AGREEMENT * left.abs().max((scale * right).abs()).max(1.0),
                    )
                    })
                }
        }
        _ => false,
    }
}

fn control_net_on_side(
    ctx: &DecodeContext<'_>,
    surface: &NurbsSurface,
    boundary: &NurbsSurfaceBoundary,
    origin: FinitePoint3,
    normal: [f64; 3],
    tolerance: f64,
    positive: bool,
) -> Result<bool, CodecError> {
    for (index, point) in surface_poles(surface).enumerate() {
        if boundary.control_indices.contains(&index) {
            continue;
        }
        ctx.charge_work(1, "creo generator separation distance tests")?;
        let offset = [point.x - origin.x, point.y - origin.y, point.z - origin.z];
        let distance = dot(normal, offset);
        let on_side = if positive {
            distance > tolerance
        } else {
            distance < -tolerance
        };
        if !on_side {
            return Ok(false);
        }
    }
    Ok(true)
}

fn generator_separates_control_nets(
    ctx: &DecodeContext<'_>,
    first: &NurbsSurface,
    first_boundary: &NurbsSurfaceBoundary,
    second: &NurbsSurface,
    second_boundary: &NurbsSurfaceBoundary,
) -> Result<bool, CodecError> {
    let (Some(origin), Some(end)) = (
        curve_point(&first_boundary.curve, 0),
        curve_point(&first_boundary.curve, 1),
    ) else {
        return Ok(false);
    };
    let generator = [end.x - origin.x, end.y - origin.y, end.z - origin.z];
    let Some(generator) = normalize(generator) else {
        return Ok(false);
    };
    let seed = if generator[0].abs() < 0.8 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let Some(first_axis) = normalize(cross(generator, seed)) else {
        return Ok(false);
    };
    let second_axis = cross(generator, first_axis);
    let first_poles = || surface_poles(first);
    let second_poles = || surface_poles(second);
    let first_outside = || {
        first_poles()
            .enumerate()
            .filter(|(index, _)| !first_boundary.control_indices.contains(index))
            .map(|(_, point)| point)
    };
    let second_outside = || {
        second_poles()
            .enumerate()
            .filter(|(index, _)| !second_boundary.control_indices.contains(index))
            .map(|(_, point)| point)
    };
    if first_outside().next().is_none() || second_outside().next().is_none() {
        return Ok(false);
    }
    let offset = |point: &Point3| [point.x - origin.x, point.y - origin.y, point.z - origin.z];
    let mut boundary_angles = Vec::new();
    for angle in first_outside()
        .chain(second_outside())
        .flat_map(|point| {
            let offset = offset(&point);
            let angle = dot(second_axis, offset).atan2(dot(first_axis, offset));
            [
                (angle + std::f64::consts::FRAC_PI_2).rem_euclid(std::f64::consts::TAU),
                (angle - std::f64::consts::FRAC_PI_2).rem_euclid(std::f64::consts::TAU),
            ]
        })
    {
        ctx.try_reserve_items(&mut boundary_angles, 1, "creo generator separation boundary angles")?;
        boundary_angles.push(angle);
    }
    crate::sort::stable_sort_by(ctx, boundary_angles.as_mut_slice(), f64::total_cmp, "creo generator separates control nets boundary angles ordering")?;
    let tolerance =
        point_tolerance(first_poles().chain(second_poles())).unwrap_or(f64::INFINITY);
    for index in 0..boundary_angles.len() {
        ctx.charge_work(1, "creo generator separation angle evaluations")?;
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
        if (control_net_on_side(ctx, first, first_boundary, origin, normal, tolerance, true)?
            && control_net_on_side(ctx, second, second_boundary, origin, normal, tolerance, false)?)
            || (control_net_on_side(ctx, first, first_boundary, origin, normal, tolerance, false)?
                && control_net_on_side(ctx, second, second_boundary, origin, normal, tolerance, true)?)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(in super::super) fn shared_extrusion_generator_curve(
    ctx: &DecodeContext<'_>,
    first: &NurbsSurface,
    first_surface_id: u32,
    second: &NurbsSurface,
    second_surface_id: u32,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<CurveGeometry>, CodecError> {
    let Some(first_boundaries) = nurbs_surface_boundaries(ctx, first, first_surface_id, refusal)? else {
        return Ok(None);
    };
    let Some(second_boundaries) = nurbs_surface_boundaries(ctx, second, second_surface_id, refusal)? else {
        return Ok(None);
    };
    let Some(tolerance) = point_tolerance(surface_poles(first).chain(surface_poles(second))) else {
        return Ok(None);
    };
    let mut selected_index = None;
    for (index, first_boundary) in first_boundaries.iter().enumerate() {
        for second_boundary in &second_boundaries {
            if first_boundary.curve.degree() == 1
                        && !first_boundary.curve.periodic()
                        && !first_boundary.transverse_periodic
                        && !second_boundary.transverse_periodic
                        && first_boundary.curve.pole_count() == 2
                        && [false, true].into_iter().any(|reversed| {
                            nurbs_curves_match(
                                &first_boundary.curve,
                                &second_boundary.curve,
                                reversed,
                                tolerance,
                            )
                        })
                        && generator_separates_control_nets(
                            ctx,
                            first,
                            first_boundary,
                            second,
                            second_boundary,
                        )?
            {
                if selected_index.is_some() {
                    return Ok(None);
                }
                selected_index = Some(index);
            }
        }
    }
    let Some(selected_index) = selected_index else {
        return Ok(None);
    };
    let Some(boundary) = first_boundaries.into_iter().nth(selected_index) else {
        return Ok(None);
    };
    let curve = boundary.curve;
    Ok(Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        curve,
    ))))
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

/// At most four stationary parameters and three interval roots of a cubic.
#[derive(Debug)]
pub(in super::super) struct CubicRoots {
    values: [f64; 7],
    len: usize,
}

impl CubicRoots {
    fn new() -> Self {
        Self {
            values: [0.0; 7],
            len: 0,
        }
    }

    fn push(&mut self, value: f64) {
        if let Some(slot) = self.values.get_mut(self.len) {
            *slot = value;
            self.len += 1;
        }
    }

    fn sort_and_dedup(&mut self) {
        self.values[..self.len].sort_by(f64::total_cmp);
        let mut unique = 0;
        for index in 0..self.len {
            if unique == 0
                || !((self.values[index] - self.values[unique - 1]).abs() <= EPS_CUBIC_PARAM)
            {
                self.values[unique] = self.values[index];
                unique += 1;
            }
        }
        self.len = unique;
    }

    pub(in super::super) fn as_slice(&self) -> &[f64] {
        &self.values[..self.len]
    }

    #[cfg(test)]
    pub(in super::super) fn len(&self) -> usize {
        self.len
    }
}

impl std::ops::Index<usize> for CubicRoots {
    type Output = f64;

    fn index(&self, index: usize) -> &Self::Output {
        &self.as_slice()[index]
    }
}

pub(in super::super) fn cubic_unit_interval_roots(
    cubic: Coefficient,
    quadratic: Coefficient,
    linear: Coefficient,
    constant: Coefficient,
    value_tolerance: f64,
) -> CubicRoots {
    let [cubic_value, quadratic_value, linear_value, constant_value] =
        [cubic, quadratic, linear, constant].map(Coefficient::stated);
    let scale = cubic_value
        .abs()
        .max(quadratic_value.abs())
        .max(linear_value.abs())
        .max(constant_value.abs());
    if scale <= value_tolerance {
        return CubicRoots::new();
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
        let mut roots = CubicRoots::new();
        for root in real_roots(quadratic, linear, constant) {
            if let Some(parameter) = unit_interval_parameter(root) {
                if evaluate(root).abs() <= value_tolerance {
                    roots.push(parameter);
                }
            }
        }
        roots.sort_and_dedup();
        return roots;
    }
    let mut stations = CubicRoots::new();
    stations.push(0.0);
    stations.push(1.0);
    for root in real_roots(
            Coefficient::summed(3.0 * cubic_value, 3.0 * cubic.terms()),
            Coefficient::summed(2.0 * quadratic_value, 2.0 * quadratic.terms()),
            linear,
        ) {
        if root > EPS_CUBIC_PARAM && root < 1.0 - EPS_CUBIC_PARAM {
            stations.push(root);
        }
    }
    stations.sort_and_dedup();
    let mut roots = CubicRoots::new();
    for &station in stations.as_slice() {
        if evaluate(station).abs() <= value_tolerance {
            roots.push(station);
        }
    }
    for interval in stations.as_slice().windows(2) {
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
    roots.sort_and_dedup();
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
        let boundaries = match nurbs_surface_boundaries(ctx, nurbs, surface_id, refusal) {
            Ok(Some(boundaries)) => boundaries,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        (nurbs.u_degree() == 3
            && nurbs.v_degree() == 1
            && nurbs.u_count() == 4
            && nurbs.v_count() == 2
            && !nurbs.u_periodic()
            && !nurbs.v_periodic())
        .then_some(())?;
        let (u_minimum, u_span) = normalized_knot_bounds(nurbs.u_knots())?;
        let (v_minimum, v_span) = normalized_knot_bounds(nurbs.v_knots())?;
        (nurbs.u_knots().len() == 8
            && nurbs.u_knots()
                .iter()
                .zip([0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0])
                .all(|(actual, expected)| scalar_near((*actual - u_minimum) / u_span, expected, EPS_ENDPOINT_AGREEMENT))
            && nurbs.v_knots().len() == 4
            && nurbs.v_knots()
                .iter()
                .zip([0.0, 0.0, 1.0, 1.0])
                .all(|(actual, expected)| scalar_near((*actual - v_minimum) / v_span, expected, EPS_ENDPOINT_AGREEMENT)))
        .then_some(())?;
        let first_pole = nurbs.pole(0, 0)?;
        let mut poles = [first_pole; 8];
        let mut weights = [1.0; 8];
        for (index, pole) in poles.iter_mut().enumerate() {
            let (u, v) = (index / 2, index % 2);
            *pole = nurbs.pole(u, v)?;
            if matches!(nurbs.pole_grid(), NurbsPoleGrid::Rational { .. }) {
                weights[index] = nurbs.weight(u, v)?.get();
            }
        }
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
        let tolerance = point_tolerance(poles.iter().copied())?
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
        let mut knots = Vec::new();
        if let Err(error) = ctx.try_reserve_items(
            &mut knots,
            curve.knots().len(),
            "creo cubic generator knots",
        ) {
            return Some(Err(error));
        }
        knots.extend_from_slice(curve.knots());
        let mut control_points = Vec::new();
        if let Err(error) = ctx.try_reserve_items(
            &mut control_points,
            2,
            "creo cubic generator control points",
        ) {
            return Some(Err(error));
        }
        control_points.extend([first.0, second.0]);
        let weights = if matches!(nurbs.pole_grid(), NurbsPoleGrid::Rational { .. }) {
            let mut weights = Vec::new();
            if let Err(error) =
                ctx.try_reserve_items(&mut weights, 2, "creo cubic generator weights")
            {
                return Some(Err(error));
            }
            weights.extend([first.1, second.1]);
            Some(weights)
        } else {
            None
        };
        let curve = match NurbsCurve::from_lanes_admitted(ctx,
            curve.degree(),
            knots,
            control_points,
            weights,
            curve.periodic(),
        ) {
            Ok(result) => result,
            Err(error) => return Some(Err(error)),
        };
        let curve = match curve {
            Ok(curve) => curve,
            Err(error) => {
                refusal.note_checked(ctx,
                    format_args!(
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
    let refused = match refusal.take_records_checked() {
        Ok(records) => records,
        Err(error) => return Err(error),
    };
    if refused.is_empty() {
        return recognized;
    }
    report_cubic_generator_loss(ctx, surface_id, &refused, losses)?;
    Ok(None)
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use cadmpeg_ir::math::Point3;

    const EPS_TEST_VALUE: f64 = 1.0e-11;
    const EPS_TEST_ROOT: f64 = 1.0e-12;

    fn generator_loss_with_policy(
        policy: DecodePolicy,
    ) -> Result<Vec<cadmpeg_ir::report::loss::LossNote>, cadmpeg_core::CodecError> {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root admitted");
        let refused = ["first lane".to_owned(), "second lane".to_owned()];
        let mut losses = Vec::new();
        super::report_cubic_generator_loss(&ctx, 7, &refused, &mut losses)?;
        Ok(losses)
    }

    #[test]
    fn cubic_generator_loss_message_refuses_retained_limit() {
        let service = generator_loss_with_policy(DecodePolicy::service())
            .expect("service profile admits the loss note");
        assert_eq!(service.len(), 1);
        assert_eq!(
            service[0].message,
            "VisibGeom surface row 7 states no cubic-extrusion plane generator carrier: first lane; second lane"
        );
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = service[0].message.len() as u64 - 1;
        let error = generator_loss_with_policy(policy).expect_err("message exceeds retained cap");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "creo cubic generator loss message"));
    }

    #[test]
    fn cubic_generator_loss_note_refuses_collection_limit() {
        assert_eq!(generator_loss_with_policy(DecodePolicy::service()).expect("service note").len(), 1);
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let error = generator_loss_with_policy(policy).expect_err("one loss exceeds item cap");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "creo cubic generator loss notes"));
    }

    fn boundary_count_with_limit(limit: u64) -> Result<usize, cadmpeg_core::CodecError> {
        let surface = NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                    vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
                ],
                Some(vec![vec![1.0, 2.0], vec![3.0, 4.0]]),
            ),
            false,
        )
        .expect("valid rational boundary surface");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root fits the collection policy");
        super::nurbs_surface_boundaries(
            &ctx,
            &surface,
            7,
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .map(|boundaries| boundaries.map_or(0, |boundaries| boundaries.len()))
    }

    fn cubic_generator_with_collection_limit(
        limit: u64,
    ) -> Result<Option<cadmpeg_ir::geometry::CurveGeometry>, cadmpeg_core::CodecError> {
        let surface = NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                [-1.0, -0.5, 0.5, 1.0]
                    .into_iter()
                    .map(|x| vec![Point3::new(x, 0.0, 0.0), Point3::new(x, 0.0, 2.0)])
                    .collect(),
                Some(vec![
                    vec![1.0, 1.0],
                    vec![2.0, 2.0],
                    vec![3.0, 3.0],
                    vec![4.0, 4.0],
                ]),
            ),
            false,
        )
        .expect("valid cubic generator surface");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root fits the collection policy");
        super::cubic_extrusion_plane_generator_curve(
            &ctx,
            &surface,
            7,
            super::PlaneEquation {
                origin: [0.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
            &mut Vec::new(),
        )
    }

    #[test]
    fn cubic_generator_knots_refuse_before_vec_copy() {
        assert!(matches!(cubic_generator_with_collection_limit(75),
            Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "creo cubic generator knots"));
        assert!(cubic_generator_with_collection_limit(84)
            .expect("collection limit admits the cubic generator")
            .is_some());
    }

    #[test]
    fn cubic_generator_control_points_refuse_before_vec_growth() {
        assert!(matches!(cubic_generator_with_collection_limit(77),
            Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "creo cubic generator control points"));
    }

    #[test]
    fn cubic_generator_weights_refuse_before_vec_growth() {
        assert!(matches!(cubic_generator_with_collection_limit(79),
            Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "creo cubic generator weights"));
    }

    #[test]
    fn cubic_generator_constructor_refuses_pairing_and_typed_poles() {
        for (cap, operation) in [(81, "IR NURBS paired poles"), (83, "IR NURBS admitted poles")] {
            assert!(matches!(cubic_generator_with_collection_limit(cap),
                Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) if refusal.operation == operation));
        }
    }

    macro_rules! boundary_collection_limit_test {
        ($name:ident, $operation:literal) => {
            #[test]
            fn $name() {
                let limit = (0..128)
                    .find(|limit| matches!(boundary_count_with_limit(*limit), Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
                        if refusal.dimension == ResourceDimension::CollectionItems
                            && refusal.operation == $operation))
                    .expect("the rational boundary reaches the named collection allocation");
                assert!(matches!(boundary_count_with_limit(limit), Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
                    if refusal.dimension == ResourceDimension::CollectionItems
                        && refusal.operation == $operation));
                assert_eq!(boundary_count_with_limit(u64::MAX).expect("service budget admits all four boundaries"), 4);
            }
        };
    }

    boundary_collection_limit_test!(nurbs_boundary_control_indices_refuse_before_vec_growth, "creo NURBS boundary control indices");
    boundary_collection_limit_test!(nurbs_boundary_control_points_refuse_before_vec_growth, "creo NURBS boundary control points");
    boundary_collection_limit_test!(nurbs_boundary_weights_refuse_before_vec_growth, "creo NURBS boundary weights");
    boundary_collection_limit_test!(nurbs_boundary_knots_refuse_before_fallible_clone, "creo NURBS boundary knots");

    #[test]
    fn nurbs_boundary_rational_pairing_refuses_collection_limit() {
        // Two indices, two points, two weights and four knots precede pairing.
        assert!(matches!(boundary_count_with_limit(11),
            Err(cadmpeg_core::CodecError::ResourceLimit(resource))
                if resource.operation == "creo NURBS boundary paired poles"));
        assert_eq!(boundary_count_with_limit(48).expect("four admitted rational boundaries"), 4);
    }

    #[test]
    fn nurbs_plane_boundary_preserves_control_index_refusal() {
        let surface = NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                    vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
                ],
                None,
            ),
            false,
        )
        .expect("valid plane boundary surface");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root fits the collection policy");
        let error = super::nurbs_plane_boundary_curve(
            &ctx,
            &surface,
            7,
            super::PlaneEquation {
                origin: [0.0; 3],
                normal: [1.0, 0.0, 0.0],
            },
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
        .expect_err("boundary allocation refusal must remain a resource error");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(ref refusal)
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "creo NURBS boundary control indices"));
    }

    fn shared_generator_surfaces() -> (NurbsSurface, NurbsSurface) {
        let first = NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(-1.0, 0.0, 0.0), Point3::new(-1.0, 0.0, 1.0)],
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 1.0)],
                ],
                Some(vec![vec![2.0, 2.0], vec![3.0, 4.0]]),
            ),
            false,
        )
        .expect("valid first extrusion surface");
        let second = NurbsSurface::from_lanes(
            NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
            NurbsSurfaceAxis::new(1, vec![4.0, 4.0, 8.0, 8.0], false),
            NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 1.0)],
                    vec![Point3::new(0.0, 1.0, 0.0), Point3::new(0.0, 1.0, 1.0)],
                ],
                Some(vec![vec![6.0, 8.0], vec![8.0, 8.0]]),
            ),
            false,
        )
        .expect("valid second extrusion surface");
        (first, second)
    }

    fn shared_generator_with_limits(
        collection_limit: u64,
        work_limit: u64,
    ) -> Result<Option<cadmpeg_ir::geometry::CurveGeometry>, cadmpeg_core::CodecError> {
        let (first, second) = shared_generator_surfaces();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = collection_limit;
        policy.limits.max_work_units = work_limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root fits the collection policy");
        super::shared_extrusion_generator_curve(
            &ctx,
            &first,
            7,
            &second,
            9,
            &mut crate::lane_refusal::LaneRefusals::new(),
        )
    }

    #[test]
    fn generator_separation_angles_refuse_before_vec_growth() {
        let run = |limit| {
            shared_generator_with_limits(limit, u64::MAX)
        };
        let limit = (0..512)
            .find(|limit| matches!(run(*limit), Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == "creo generator separation boundary angles"))
            .expect("the shared generator reaches the angle collection");
        assert!(matches!(run(limit), Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
            if refusal.dimension == ResourceDimension::CollectionItems
                && refusal.operation == "creo generator separation boundary angles"));
        assert!(run(u64::MAX).expect("service budget admits the shared generator").is_some());
    }

    #[test]
    fn generator_separation_angle_work_refuses_before_evaluation() {
        let run = |limit| shared_generator_with_limits(u64::MAX, limit);
        let limit = (0..128)
            .find(|limit| matches!(run(*limit), Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
                if refusal.dimension == ResourceDimension::WorkUnits
                    && refusal.operation == "creo generator separation angle evaluations"))
            .expect("the shared generator reaches angle evaluation work");
        assert!(matches!(run(limit), Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
            if refusal.dimension == ResourceDimension::WorkUnits
                && refusal.operation == "creo generator separation angle evaluations"));
        assert!(run(u64::MAX).expect("service work budget admits the shared generator").is_some());
    }

    #[test]
    fn generator_separation_distance_work_refuses_before_evaluation() {
        let run = |limit| shared_generator_with_limits(u64::MAX, limit);
        let limit = (0..128)
            .find(|limit| matches!(run(*limit), Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
                if refusal.dimension == ResourceDimension::WorkUnits
                    && refusal.operation == "creo generator separation distance tests"))
            .expect("the shared generator reaches distance work");
        assert!(matches!(run(limit), Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
            if refusal.dimension == ResourceDimension::WorkUnits
                && refusal.operation == "creo generator separation distance tests"));
        assert!(run(u64::MAX).expect("service work budget admits the shared generator").is_some());
    }

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
            super::cubic_unit_interval_roots(cubic, quadratic, linear, constant, EPS_TEST_VALUE)
                .as_slice(),
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
