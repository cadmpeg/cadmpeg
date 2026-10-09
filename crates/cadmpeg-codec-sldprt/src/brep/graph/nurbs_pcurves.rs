// SPDX-License-Identifier: Apache-2.0
//! Resolve NURBS isocurves, projection caches and fit bounds.

use cadmpeg_core::convert::f64_from_index;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::decode::index_from_u32;
use cadmpeg_ir::eval::nurbs_curve_parameter_domain;
use cadmpeg_ir::eval::nurbs_pcurve_uv;
use cadmpeg_ir::eval::nurbs_surface_isocurve;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis;
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Edge;
use super::EPS_NURBS_WEIGHT;
use super::inverse::INVERSE_PARAMETER_TOLERANCE;
use super::inverse::InverseResolution;
use super::clamp::clamp_nurbs_curve_to_domain;
use super::inverse::inverse_coordinate_tolerance;
use super::inverse::looser_tolerance;
use super::inverse::nurbs_parameter_at_point;
use super::inverse::surface_poles;

const NURBS_ENDPOINT_TOLERANCE_MM: f64 = 1.0e-6;
const NURBS_CACHE_SAMPLES_PER_SPAN: usize = 8;
const NURBS_POLE_ROUNDOFF_FACTOR: f64 = 256.0 * f64::EPSILON;
pub(super) fn resolve_axis_candidates<T, const N: usize>(
    candidates: [InverseResolution<T>; N],
) -> InverseResolution<T> {
    let mut unique = None;
    for candidate in candidates {
        match candidate {
            InverseResolution::NoMatch => {}
            InverseResolution::Ambiguous => return InverseResolution::Ambiguous,
            InverseResolution::Unique(value) if unique.is_none() => unique = Some(value),
            InverseResolution::Unique(_) => return InverseResolution::Ambiguous,
        }
    }
    unique.map_or(InverseResolution::NoMatch, InverseResolution::Unique)
}

#[derive(Debug)]
pub(super) enum NurbsPcurveFailure {
    Carrier(cadmpeg_ir::geometry::nurbs::NurbsError),
    Resource(cadmpeg_core::CodecError),
}

impl From<cadmpeg_ir::geometry::nurbs::NurbsError> for NurbsPcurveFailure {
    fn from(error: cadmpeg_ir::geometry::nurbs::NurbsError) -> Self {
        match error {
            cadmpeg_ir::geometry::nurbs::NurbsError::ResourceLimit(limit) => {
                Self::Resource(limit.into())
            }
            error => Self::Carrier(error),
        }
    }
}

impl From<cadmpeg_core::decode::ResourceLimit> for NurbsPcurveFailure {
    fn from(limit: cadmpeg_core::decode::ResourceLimit) -> Self {
        Self::Resource(limit.into())
    }
}

impl From<cadmpeg_core::CodecError> for NurbsPcurveFailure {
    fn from(error: cadmpeg_core::CodecError) -> Self {
        Self::Resource(error)
    }
}

fn nurbs_boundary_pcurve(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    fixed_axis: SurfaceParameterAxis,
) -> Result<InverseResolution<PcurveGeometry>, cadmpeg_core::CodecError> {
    let (fixed_degree, fixed_count, fixed_knots) = match fixed_axis {
        SurfaceParameterAxis::U => (
            index_from_u32(surface.u_degree()),
            surface.u_count(),
            surface.u_knots(),
        ),
        SurfaceParameterAxis::V => (
            index_from_u32(surface.v_degree()),
            surface.v_count(),
            surface.v_knots(),
        ),
    };
    let (varying_degree, varying_knots) = match fixed_axis {
        SurfaceParameterAxis::U => (index_from_u32(surface.v_degree()), surface.v_knots()),
        SurfaceParameterAxis::V => (index_from_u32(surface.u_degree()), surface.u_knots()),
    };
    let (Some(&fixed_min), Some(&fixed_max), Some(&varying_min)) = (
        fixed_knots.get(fixed_degree),
        fixed_knots.get(fixed_count),
        varying_knots.get(varying_degree),
    ) else {
        return Ok(InverseResolution::NoMatch);
    };
    if !fixed_min.is_finite()
        || !fixed_max.is_finite()
        || fixed_min >= fixed_max
        || !varying_min.is_finite()
    {
        return Ok(InverseResolution::NoMatch);
    }
    let tolerance = inverse_coordinate_tolerance(
        ctx,
        surface_poles(ctx, surface)?
            .chain(
                (0..curve.pole_count())
                    .filter_map(|index| curve.pole_rows().point_at(index)),
            )
            .map(FinitePoint3::get),
    )?;
    let same_curve = |candidate: &cadmpeg_ir::geometry::nurbs::NurbsCurve| -> Result<bool, cadmpeg_core::CodecError> {
        Ok(candidate.degree() == curve.degree()
            && candidate.periodic() == curve.periodic()
            && candidate.pole_count() == curve.pole_count()
            && candidate.knots().len() == curve.knots().len()
            && ctx.equal(candidate.knots().as_slice(), curve.knots().as_slice(), "compare Parasolid boundary curve knots")?
            && ctx.all_by(0..curve.pole_count(), |index| {
                let (Some(candidate), Some(actual)) = (
                    candidate.pole_rows().point_at(index),
                    curve.pole_rows().point_at(index),
                ) else {
                    return Ok(false);
                };
                Ok((candidate.x - actual.x).powi(2)
                    + (candidate.y - actual.y).powi(2)
                    + (candidate.z - actual.z).powi(2)
                    <= tolerance * tolerance)
            }, "compare Parasolid boundary curve poles")?
            && ctx.all_by(0..curve.pole_count(), |index| {
                Ok(match (
                    candidate.pole_rows().weight_at(index),
                    curve.pole_rows().weight_at(index),
                ) {
                    (None, None) => true,
                    (Some(candidate), Some(actual)) => {
                        (candidate - actual).abs() <= EPS_NURBS_WEIGHT
                    }
                    _ => false,
                })
            }, "compare Parasolid boundary curve weights")?)
    };
    let mut fixed = None;
    for parameter in [fixed_min, fixed_max]
        .into_iter()
        .filter(|parameter| parameter.is_finite())
    {
        if nurbs_surface_isocurve(ctx, surface, fixed_axis, parameter)?
            .as_ref()
            .map(same_curve)
            .transpose()?
            .unwrap_or(false)
        {
            if fixed.is_some() {
                return Ok(InverseResolution::Ambiguous);
            }
            fixed = Some(parameter);
        }
    }
    let Some(fixed) = fixed else {
        return Ok(InverseResolution::NoMatch);
    };
    Ok(InverseResolution::Unique(match fixed_axis {
        SurfaceParameterAxis::U => PcurveGeometry::Line(
            match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(fixed, varying_min),
                cadmpeg_ir::math::Point2::new(0.0, 1.0),
            ) {
                Ok(payload) => payload,
                Err(_) => return Ok(InverseResolution::NoMatch),
            },
        ),
        SurfaceParameterAxis::V => PcurveGeometry::Line(
            match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(varying_min, fixed),
                cadmpeg_ir::math::Point2::new(1.0, 0.0),
            ) {
                Ok(payload) => payload,
                Err(_) => return Ok(InverseResolution::NoMatch),
            },
        ),
    }))
}

pub(super) fn nurbs_strict_isocurve_pcurve(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> Result<InverseResolution<PcurveGeometry>, cadmpeg_core::CodecError> {
    let axis_candidate = |fixed_axis| -> Result<
        InverseResolution<PcurveGeometry>,
        cadmpeg_core::CodecError,
    > {
        let (uc, vc) = (surface.u_count(), surface.v_count());
        let (fixed_degree, fixed_count, fixed_knots, fixed_periodic) = match fixed_axis {
            SurfaceParameterAxis::U => (
                surface.u_degree(),
                uc,
                surface.u_knots(),
                surface.u_periodic(),
            ),
            SurfaceParameterAxis::V => (
                surface.v_degree(),
                vc,
                surface.v_knots(),
                surface.v_periodic(),
            ),
        };
        let (varying_degree, varying_count, varying_knots, varying_periodic) = match fixed_axis {
            SurfaceParameterAxis::U => (
                surface.v_degree(),
                vc,
                surface.v_knots(),
                surface.v_periodic(),
            ),
            SurfaceParameterAxis::V => (
                surface.u_degree(),
                uc,
                surface.u_knots(),
                surface.u_periodic(),
            ),
        };
        if curve.degree() != varying_degree
            || curve.periodic() != varying_periodic
            || curve.pole_count() != varying_count
            || curve.knots().len() != varying_knots.len()
            || !ctx.equal(
                curve.knots().as_slice(),
                varying_knots.as_slice(),
                "compare Parasolid ruled curve knots",
            )?
        {
            return Ok(InverseResolution::NoMatch);
        }
        let (Some(&fixed_min), Some(&fixed_max)) = (fixed_knots.get(1), fixed_knots.get(2)) else {
            return nurbs_boundary_pcurve(ctx, surface, curve, fixed_axis);
        };
        if fixed_degree != 1
            || fixed_count != 2
            || fixed_periodic
            || fixed_knots.as_slice() != [fixed_min, fixed_min, fixed_max, fixed_max]
            || fixed_min >= fixed_max
        {
            return nurbs_boundary_pcurve(ctx, surface, curve, fixed_axis);
        }
        let pole_indices = |varying: usize| match fixed_axis {
            SurfaceParameterAxis::U => (varying, vc + varying),
            SurfaceParameterAxis::V => (varying * vc, varying * vc + 1),
        };
        let weight_mismatch = ctx.any_by(
            0..varying_count,
            |varying| {
                let (a, b) = pole_indices(varying);
                Ok(
                    match (
                        surface.weight(a / vc, a % vc),
                        surface.weight(b / vc, b % vc),
                        curve.pole_rows().weight_at(varying),
                    ) {
                        (None, None, None) => false,
                        (Some(a), Some(b), Some(actual)) => {
                            (a.get() - b.get()).abs() > EPS_NURBS_WEIGHT
                                || (a.get() - actual).abs() > EPS_NURBS_WEIGHT
                        }
                        _ => true,
                    },
                )
            },
            "compare Parasolid ruled curve weights",
        )?;
        if weight_mismatch {
            return Ok(InverseResolution::NoMatch);
        }
        let pole_at = |index: usize| surface.pole(index / vc, index % vc);
        let mut delta_squared = 0.0;
        let mut relative_dot_delta = 0.0;
        for varying in ctx.admit_iter(0..varying_count, "project Parasolid ruled curve poles")? {
            let (a_index, b_index) = pole_indices(varying);
            let (Some(a), Some(b), Some(point)) = (
                pole_at(a_index),
                pole_at(b_index),
                curve.pole_rows().point_at(varying),
            ) else {
                return Ok(InverseResolution::NoMatch);
            };
            let delta = [b.x - a.x, b.y - a.y, b.z - a.z];
            let relative = [point.x - a.x, point.y - a.y, point.z - a.z];
            delta_squared += delta.iter().map(|value| value * value).sum::<f64>();
            relative_dot_delta += relative
                .iter()
                .zip(delta)
                .map(|(relative, delta)| relative * delta)
                .sum::<f64>();
        }
        let tolerance = inverse_coordinate_tolerance(
            ctx,
            surface_poles(ctx, surface)?
                .chain(
                    (0..curve.pole_count())
                        .filter_map(|index| curve.pole_rows().point_at(index)),
                )
                .map(FinitePoint3::get),
        )?;
        if delta_squared <= f64::EPSILON {
            let all_equal = ctx.all_by(
                0..varying_count,
                |varying| {
                    let (Some(a), Some(point)) = (
                        pole_at(pole_indices(varying).0),
                        curve.pole_rows().point_at(varying),
                    ) else {
                        return Ok(false);
                    };
                    Ok(
                        (point.x - a.x).powi(2) + (point.y - a.y).powi(2) + (point.z - a.z).powi(2)
                            <= tolerance * tolerance,
                    )
                },
                "compare Parasolid degenerate ruling poles",
            )?;
            return Ok(if all_equal {
                InverseResolution::Ambiguous
            } else {
                InverseResolution::NoMatch
            });
        }
        let factor = relative_dot_delta / delta_squared;
        let residual_squared = ctx
            .admit_iter(0..varying_count, "measure Parasolid ruling residual")?
            .filter_map(|varying| {
                let (a_index, b_index) = pole_indices(varying);
                let a = pole_at(a_index)?;
                let b = pole_at(b_index)?;
                let point = curve.pole_rows().point_at(varying)?;
                Some(
                    (point.x - (a.x + factor * (b.x - a.x))).powi(2)
                        + (point.y - (a.y + factor * (b.y - a.y))).powi(2)
                        + (point.z - (a.z + factor * (b.z - a.z))).powi(2),
                )
            })
            .fold(0.0_f64, f64::max);
        let parameter_tolerance = INVERSE_PARAMETER_TOLERANCE;
        if !factor.is_finite()
            || factor < -parameter_tolerance
            || factor > 1.0 + parameter_tolerance
            || residual_squared > tolerance * tolerance
        {
            return Ok(InverseResolution::NoMatch);
        }
        let fixed = fixed_min + factor.clamp(0.0, 1.0) * (fixed_max - fixed_min);
        let Some(varying_degree) = usize::try_from(varying_degree).ok() else {
            return Ok(InverseResolution::NoMatch);
        };
        let (Some(&varying_min), Some(&varying_max)) = (
            varying_knots.get(varying_degree),
            varying_knots.get(varying_count),
        ) else {
            return Ok(InverseResolution::NoMatch);
        };
        if !varying_min.is_finite() || !varying_max.is_finite() || varying_min >= varying_max {
            return Ok(InverseResolution::NoMatch);
        }
        Ok(InverseResolution::Unique(match fixed_axis {
            SurfaceParameterAxis::U => PcurveGeometry::Line(
                match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    cadmpeg_ir::math::Point2::new(fixed, varying_min),
                    cadmpeg_ir::math::Point2::new(0.0, 1.0),
                ) {
                    Ok(payload) => payload,
                    Err(_) => return Ok(InverseResolution::NoMatch),
                },
            ),
            SurfaceParameterAxis::V => PcurveGeometry::Line(
                match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    cadmpeg_ir::math::Point2::new(varying_min, fixed),
                    cadmpeg_ir::math::Point2::new(1.0, 0.0),
                ) {
                    Ok(payload) => payload,
                    Err(_) => return Ok(InverseResolution::NoMatch),
                },
            ),
        }))
    };
    Ok(resolve_axis_candidates([
        axis_candidate(SurfaceParameterAxis::U)?,
        axis_candidate(SurfaceParameterAxis::V)?,
    ]))
}

fn nurbs_active_domain(knots: &[f64], degree: u32, count: usize) -> Option<[f64; 2]> {
    let degree = usize::try_from(degree).ok()?;
    if count <= degree || knots.len() != count.checked_add(degree)?.checked_add(1)? {
        return None;
    }
    let domain = [*knots.get(degree)?, *knots.get(count)?];
    (domain[0] < domain[1]).then_some(domain)
}

fn nurbs_roundoff_equal(left: f64, right: f64, scale: f64) -> bool {
    left.is_finite()
        && right.is_finite()
        && (left - right).abs()
            <= NURBS_POLE_ROUNDOFF_FACTOR
                * 1.0_f64.max(scale.abs()).max(left.abs()).max(right.abs())
}

pub(super) fn nurbs_representation_matches(
    ctx: &DecodeContext<'_>,
    expected: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    actual: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> Result<bool, cadmpeg_core::CodecError> {
    if expected.degree() != actual.degree()
        || expected.periodic() != actual.periodic()
        || expected.knots().len() != actual.knots().len()
        || expected.pole_count() != actual.pole_count()
    {
        return Ok(false);
    }
    let mut scale = 1.0_f64;
    for index in ctx.admit_iter(
        0..expected.pole_count(),
        "scale Parasolid NURBS representation",
    )? {
        let (Some(expected_point), Some(actual_point)) = (
            expected.pole_rows().point_at(index),
            actual.pole_rows().point_at(index),
        ) else {
            return Ok(false);
        };
        for value in [
            expected_point.x,
            expected_point.y,
            expected_point.z,
            actual_point.x,
            actual_point.y,
            actual_point.z,
        ] {
            scale = scale.max(value.abs());
        }
        if let Some(weight) = expected.pole_rows().weight_at(index) {
            scale = scale.max(weight.abs());
        }
        if let Some(weight) = actual.pole_rows().weight_at(index) {
            scale = scale.max(weight.abs());
        }
    }
    Ok(ctx.all_by(
        expected.knots().iter().zip(actual.knots().iter()),
        |(left, right)| Ok(nurbs_roundoff_equal(*left, *right, scale)),
        "compare Parasolid knot representation",
    )? && ctx.all_by(
        0..expected.pole_count(),
        |index| {
            let (Some(left), Some(right)) = (
                expected.pole_rows().point_at(index),
                actual.pole_rows().point_at(index),
            ) else {
                return Ok(false);
            };
            Ok(nurbs_roundoff_equal(left.x, right.x, scale)
                && nurbs_roundoff_equal(left.y, right.y, scale)
                && nurbs_roundoff_equal(left.z, right.z, scale)
                && match (
                    expected.pole_rows().weight_at(index),
                    actual.pole_rows().weight_at(index),
                ) {
                    (None, None) => true,
                    (Some(left), Some(right)) => nurbs_roundoff_equal(left, right, scale),
                    _ => false,
                })
        },
        "compare Parasolid pole representation",
    )?)
}

fn extended_nurbs_isocurve_axis_candidate(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    fixed_axis: SurfaceParameterAxis,
) -> Result<InverseResolution<PcurveGeometry>, NurbsPcurveFailure> {
    let (fixed_degree, fixed_count, fixed_knots, fixed_periodic) = match fixed_axis {
        SurfaceParameterAxis::U => (
            surface.u_degree(),
            surface.u_count(),
            surface.u_knots(),
            surface.u_periodic(),
        ),
        SurfaceParameterAxis::V => (
            surface.v_degree(),
            surface.v_count(),
            surface.v_knots(),
            surface.v_periodic(),
        ),
    };
    let (varying_degree, varying_count, varying_knots, varying_periodic) = match fixed_axis {
        SurfaceParameterAxis::U => (
            surface.v_degree(),
            surface.v_count(),
            surface.v_knots(),
            surface.v_periodic(),
        ),
        SurfaceParameterAxis::V => (
            surface.u_degree(),
            surface.u_count(),
            surface.u_knots(),
            surface.u_periodic(),
        ),
    };
    if fixed_periodic || varying_periodic || curve.periodic() || curve.degree() != varying_degree {
        return Ok(InverseResolution::NoMatch);
    }
    let Some(fixed_domain) = nurbs_active_domain(fixed_knots, fixed_degree, fixed_count) else {
        return Ok(InverseResolution::NoMatch);
    };
    let Some(varying_domain) = nurbs_active_domain(varying_knots, varying_degree, varying_count)
    else {
        return Ok(InverseResolution::NoMatch);
    };
    let Some(curve_domain) = nurbs_curve_parameter_domain(curve)
        .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
    else {
        return Ok(InverseResolution::NoMatch);
    };
    if curve_domain[0] > varying_domain[0] || curve_domain[1] < varying_domain[1] {
        return Ok(InverseResolution::NoMatch);
    }

    // At most three fixed parameters are candidates: both domain ends and the
    // parameter the curve midpoint projects to.
    let mut fixed_values = [Some(fixed_domain[0]), Some(fixed_domain[1]), None];
    let overlap = [
        curve_domain[0].max(varying_domain[0]),
        curve_domain[1].min(varying_domain[1]),
    ];
    if overlap[0] < overlap[1] {
        let parameter = overlap[0].midpoint(overlap[1]);
        let point = crate::brep::evaluation::nurbs_curve_point(ctx, curve, parameter)?;
        if let Some(point) = point {
            let tolerance = inverse_coordinate_tolerance(
                ctx,
                surface_poles(ctx, surface)?
                    .map(FinitePoint3::get)
                    .chain(std::iter::once(point.get())),
            )?;
            if let Some(parameters) = crate::brep::evaluation::nurbs_surface_parameter_near_point(
                ctx,
                surface,
                point.get(),
                None,
            )? {
                let mapped = crate::brep::evaluation::nurbs_surface_point(
                    ctx,
                    surface,
                    parameters.u,
                    parameters.v,
                )?;
                if mapped
                    .is_some_and(|mapped| Point3::distance(point.get(), mapped.get()) <= tolerance)
                {
                    fixed_values[2] = Some(match fixed_axis {
                        SurfaceParameterAxis::U => parameters.u,
                        SurfaceParameterAxis::V => parameters.v,
                    });
                }
            }
        }
    }
    let parameter_tolerance = (INVERSE_PARAMETER_TOLERANCE * fixed_domain[1]
        - INVERSE_PARAMETER_TOLERANCE * fixed_domain[0])
        .abs();
    let mut unique_fixed_values = [0.0_f64; 3];
    let mut unique_count = 0;
    for value in fixed_values.into_iter().flatten() {
        if value.is_finite()
            && value >= fixed_domain[0] - parameter_tolerance
            && value <= fixed_domain[1] + parameter_tolerance
            && !unique_fixed_values[..unique_count]
                .iter()
                .any(|known| (value - *known).abs() <= parameter_tolerance)
        {
            unique_fixed_values[unique_count] = value.clamp(fixed_domain[0], fixed_domain[1]);
            unique_count += 1;
        }
    }
    let unique_fixed_values = &unique_fixed_values[..unique_count];
    // The clamp does not vary with the candidate value, and the first candidate
    // already reaches it, so it is minted once here.
    let clamped = if unique_fixed_values.is_empty() {
        None
    } else {
        clamp_nurbs_curve_to_domain(ctx, curve, varying_domain)?
    };
    let Some(clamped) = clamped else {
        return Ok(InverseResolution::NoMatch);
    };
    let mut matched = None;
    for &fixed in unique_fixed_values {
        if nurbs_surface_isocurve(ctx, surface, fixed_axis, fixed)?
            .map(|expected| nurbs_representation_matches(ctx, &expected, &clamped))
            .transpose()?
            .unwrap_or(false)
        {
            if matched.is_some() {
                return Ok(InverseResolution::Ambiguous);
            }
            matched = Some(fixed);
        }
    }
    let Some(fixed) = matched else {
        return Ok(InverseResolution::NoMatch);
    };
    Ok(InverseResolution::Unique(match fixed_axis {
        SurfaceParameterAxis::U => PcurveGeometry::Line(
            match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(fixed, varying_domain[0]),
                cadmpeg_ir::math::Point2::new(0.0, 1.0),
            ) {
                Ok(payload) => payload,
                Err(_) => return Ok(InverseResolution::NoMatch),
            },
        ),
        SurfaceParameterAxis::V => PcurveGeometry::Line(
            match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(varying_domain[0], fixed),
                cadmpeg_ir::math::Point2::new(1.0, 0.0),
            ) {
                Ok(payload) => payload,
                Err(_) => return Ok(InverseResolution::NoMatch),
            },
        ),
    }))
}

fn extended_nurbs_isocurve_pcurve(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> Result<InverseResolution<PcurveGeometry>, NurbsPcurveFailure> {
    Ok(resolve_axis_candidates([
        extended_nurbs_isocurve_axis_candidate(ctx, surface, curve, SurfaceParameterAxis::U)?,
        extended_nurbs_isocurve_axis_candidate(ctx, surface, curve, SurfaceParameterAxis::V)?,
    ]))
}

pub(super) fn nurbs_isocurve_pcurve(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
) -> Result<InverseResolution<PcurveGeometry>, NurbsPcurveFailure> {
    match nurbs_strict_isocurve_pcurve(ctx, surface, curve)? {
        InverseResolution::NoMatch => extended_nurbs_isocurve_pcurve(ctx, surface, curve),
        other => Ok(other),
    }
}

pub(super) enum NurbsPcurveResolution {
    Exact(PcurveGeometry),
    Cache {
        geometry: PcurveGeometry,
        fit_tolerance: f64,
    },
    OffSurface,
    NoMatch,
    Ambiguous,
}

/// Parameter samples remain temporary through fit and deviation checks.
pub(super) struct CurveSampleParameters<'ctx> {
    pub(super) values: Vec<f64>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

pub(super) fn nurbs_curve_sample_parameters<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    range: [f64; 2],
) -> Result<Option<CurveSampleParameters<'ctx>>, NurbsPcurveFailure> {
    if let Some(limit) = ctx.resource_refusal() {
        return Err(limit.into());
    }
    let Some(domain) = nurbs_curve_parameter_domain(curve) else {
        return Ok(None);
    };
    let domain = domain.endpoints();
    if !range[0].is_finite()
        || !range[1].is_finite()
        || range[0] >= range[1]
        || range[0] < domain[0]
        || range[1] > domain[1]
    {
        return Ok(None);
    }
    let mut storage = ctx.reserve_scoped(0, "hold NURBS sample parameters")?;
    let mut parameters = storage.with_storage(|| {
        ctx.collection_vec(2, "collect NURBS sample endpoints")
    })?;
    parameters.extend_from_slice(&range);
    for span in ctx
        .admit_iter(
            curve.knots().as_slice(),
            "sample Parasolid curve knot spans",
        )?
        .windows(const { crate::nonzero(2) })
    {
        let start = span[0].max(range[0]);
        let end = span[1].min(range[1]);
        if !span[0].is_finite() || !span[1].is_finite() || start >= end {
            continue;
        }
        for index in 0..=NURBS_CACHE_SAMPLES_PER_SPAN {
            let (Some(position), Some(span_count)) = (
                f64_from_index(index),
                f64_from_index(NURBS_CACHE_SAMPLES_PER_SPAN),
            ) else {
                return Ok(None);
            };
            let fraction = position / span_count;
            let Some(parameter) = cadmpeg_ir::math::interpolate(start, end, fraction) else {
                return Ok(None);
            };
            ctx.charge_work(1, "sample NURBS curve span")?;
            storage.with_storage(|| {
                ctx.reserve_vec(&mut parameters, 1, "collect NURBS sample parameters")
            })?;
            parameters.push(parameter.get());
        }
    }
    ctx.stable_sort_by(
        &mut parameters,
        |value| value,
        f64::total_cmp,
        "sort NURBS sample parameters",
    )?;
    // Every distinct sample participates in the fit bound, including tiny spans.
    ctx.dedup_vec(&mut parameters, "deduplicate NURBS sample parameters")?;
    Ok((!parameters.is_empty()).then_some(CurveSampleParameters {
        values: parameters,
        _storage: storage,
    }))
}

fn nurbs_edge_endpoint_parameters(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    range: [f64; 2],
) -> Result<Option<[cadmpeg_ir::math::Point2; 2]>, cadmpeg_core::CodecError> {
    let Some(first) = crate::brep::evaluation::nurbs_curve_point(ctx, curve, range[0])? else {
        return Ok(None);
    };
    let Some(last) = crate::brep::evaluation::nurbs_curve_point(ctx, curve, range[1])? else {
        return Ok(None);
    };
    // The inverse-projection tolerance of this surface's coordinates against
    // the NURBS endpoint tolerance.
    let tolerance = looser_tolerance(
        inverse_coordinate_tolerance(
            ctx,
            surface_poles(ctx, surface)?
                .chain([first, last])
                .map(FinitePoint3::get),
        )?,
        NURBS_ENDPOINT_TOLERANCE_MM,
    );
    let project = |point| -> Result<Option<cadmpeg_ir::math::Point2>, cadmpeg_core::CodecError> {
        let Some(parameters) =
            crate::brep::evaluation::nurbs_surface_parameter_near_point(ctx, surface, point, None)?
        else {
            return Ok(None);
        };
        let Some(mapped) =
            crate::brep::evaluation::nurbs_surface_point(ctx, surface, parameters.u, parameters.v)?
        else {
            return Ok(None);
        };
        Ok((Point3::distance(point, mapped.get()) <= tolerance).then_some(parameters.get()))
    };
    let Some(start) = project(first.get())? else {
        return Ok(None);
    };
    let Some(end) = project(last.get())? else {
        return Ok(None);
    };
    Ok(Some([start, end]))
}

fn nurbs_curve_surface_deviation(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    range: [f64; 2],
) -> Result<Option<f64>, NurbsPcurveFailure> {
    let Some(parameters) = nurbs_curve_sample_parameters(ctx, curve, range)? else {
        return Ok(None);
    };
    let mut seed = None;
    let mut maximum = 0.0_f64;
    for &parameter in ctx.admit_iter(&parameters.values, "measure NURBS curve surface deviation")? {
        let Some(point) = crate::brep::evaluation::nurbs_curve_point(ctx, curve, parameter)? else {
            return Ok(None);
        };
        let projected = match seed {
            Some(seed) => crate::brep::evaluation::nurbs_surface_parameter_near_point(
                ctx,
                surface,
                point.get(),
                Some(seed),
            )?,
            None => None,
        };
        let parameters = match projected {
            Some(parameters) => Some(parameters),
            None => crate::brep::evaluation::nurbs_surface_parameter_near_point(
                ctx,
                surface,
                point.get(),
                None,
            )?,
        };
        let Some(parameters) = parameters else {
            return Ok(None);
        };
        let parameters = parameters.get();
        let Some(surface_point) =
            crate::brep::evaluation::nurbs_surface_point(ctx, surface, parameters.u, parameters.v)?
        else {
            return Ok(None);
        };
        seed = Some(parameters);
        maximum = maximum.max(Point3::distance(point.get(), surface_point.get()));
    }
    Ok(maximum.is_finite().then_some(maximum))
}

/// The degree-one pcurve lanes a cached projection states, and the fit
/// tolerance they reach. `None` when the curve is not a degree-one cache
/// candidate or a projection does not land on the surface.
pub(super) fn nurbs_degree_one_cache_lanes(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    range: [f64; 2],
) -> Result<Option<(Vec<cadmpeg_ir::math::Point2>, f64)>, NurbsPcurveFailure> {
    if curve.degree() != 1 || curve.pole_rows().weight_at(0).is_some() || curve.periodic() {
        return Ok(None);
    }
    let mut control_points = Vec::new();
    ctx.reserve_capacity(
        &mut control_points,
        curve.pole_count(),
        "collect NURBS cache pcurve controls",
    )?;
    let mut seed = None;
    for index in ctx.admit_iter(0..curve.pole_count(), "collect NURBS cache pcurve controls")? {
        let Some(point) = curve.pole_rows().point_at(index) else {
            return Ok(None);
        };
        let point = point.get();
        let projected = match seed {
            Some(seed) => crate::brep::evaluation::nurbs_surface_parameter_near_point(
                ctx,
                surface,
                point,
                Some(seed),
            )?,
            None => None,
        };
        let parameters = match projected {
            Some(parameters) => Some(parameters),
            None => {
                crate::brep::evaluation::nurbs_surface_parameter_near_point(ctx, surface, point, None)?
            }
        };
        let Some(parameters) = parameters else {
            return Ok(None);
        };
        let parameters = parameters.get();
        seed = Some(parameters);
        ctx.push_vec(
            &mut (control_points),
            parameters,
            "collect NURBS cache pcurve controls",
        )?;
    }
    let Some(parameters) = nurbs_curve_sample_parameters(ctx, curve, range)? else {
        return Ok(None);
    };
    let mut fit_tolerance = 0.0_f64;
    for &parameter in ctx.admit_iter(&parameters.values, "measure NURBS cache pcurve fit")? {
        let Some(model_point) = crate::brep::evaluation::nurbs_curve_point(ctx, curve, parameter)? else {
            return Ok(None);
        };
        let Some(uv) = cadmpeg_ir::eval::finite_or_refusal(nurbs_pcurve_uv(
            ctx,
            1,
            curve.knots(),
            &control_points,
            None,
            parameter,
        ))?
        else {
            return Ok(None);
        };
        let Some(mapped_point) = crate::brep::evaluation::nurbs_surface_point(ctx, surface, uv.u, uv.v)?
        else {
            return Ok(None);
        };
        fit_tolerance = fit_tolerance.max(Point3::distance(model_point.get(), mapped_point.get()));
    }
    if !fit_tolerance.is_finite() {
        return Ok(None);
    }
    Ok(Some((control_points, fit_tolerance)))
}

fn nurbs_degree_one_cache_pcurve(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    range: [f64; 2],
) -> Result<Option<(PcurveGeometry, f64)>, NurbsPcurveFailure> {
    let Some((control_points, fit_tolerance)) =
        nurbs_degree_one_cache_lanes(ctx, surface, curve, range)?
    else {
        return Ok(None);
    };
    let knots = ctx.copy_slice(curve.knots(), "copy NURBS cache pcurve knots")?;
    let nurbs = PcurveNurbs::from_lanes(ctx, 1, knots, control_points, None, false)??;
    Ok(Some((PcurveGeometry::Nurbs { nurbs }, fit_tolerance)))
}

pub(super) fn nurbs_edge_parameter_range(
    ctx: &DecodeContext<'_>,
    edge: &Edge,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    endpoints: Option<[cadmpeg_ir::math::Point3; 2]>,
) -> Result<Option<[f64; 2]>, cadmpeg_core::CodecError> {
    let Some(domain) = nurbs_curve_parameter_domain(curve) else {
        return Ok(None);
    };
    let domain = domain.endpoints();
    let range = if let Some(range) = edge.param_range() {
        range.get()
    } else {
        let Some([start, end]) = endpoints else {
            return Ok(None);
        };
        match (
            nurbs_parameter_at_point(ctx, curve, start)?,
            nurbs_parameter_at_point(ctx, curve, end)?,
        ) {
            (InverseResolution::Unique(start), InverseResolution::Unique(end)) => [start, end],
            _ => return Ok(None),
        }
    };
    if !range[0].is_finite()
        || !range[1].is_finite()
        || range[0] == range[1]
        || range
            .iter()
            .any(|parameter| *parameter < domain[0] || *parameter > domain[1])
    {
        return Ok(None);
    }
    Ok(Some([range[0].min(range[1]), range[0].max(range[1])]))
}

pub(super) fn derive_nurbs_edge_pcurve(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    range: [f64; 2],
) -> Result<NurbsPcurveResolution, NurbsPcurveFailure> {
    if nurbs_edge_endpoint_parameters(ctx, surface, curve, range)?.is_none() {
        return Ok(NurbsPcurveResolution::OffSurface);
    }
    if nurbs_curve_surface_deviation(ctx, surface, curve, range)?.is_none() {
        return Ok(NurbsPcurveResolution::NoMatch);
    }
    Ok(match nurbs_isocurve_pcurve(ctx, surface, curve)? {
        InverseResolution::Unique(geometry) => NurbsPcurveResolution::Exact(geometry),
        InverseResolution::Ambiguous => NurbsPcurveResolution::Ambiguous,
        InverseResolution::NoMatch => {
            match nurbs_degree_one_cache_pcurve(ctx, surface, curve, range)? {
                Some((geometry, fit_tolerance)) => NurbsPcurveResolution::Cache {
                    geometry,
                    fit_tolerance,
                },
                None => NurbsPcurveResolution::NoMatch,
            }
        }
    })
}

#[cfg(test)]
mod tests;
