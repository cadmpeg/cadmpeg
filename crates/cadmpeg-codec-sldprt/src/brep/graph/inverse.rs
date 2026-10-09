// SPDX-License-Identifier: Apache-2.0
//! Resolve scalar inverse candidates and coordinate-scale tolerances.

use cadmpeg_core::convert::f64_from_index;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::eval::nurbs_curve_parameter_domain;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::SurfaceParameterAxis;
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use super::EPS_NURBS_WEIGHT;

pub(super) enum InverseResolution<T> {
    NoMatch,
    Unique(T),
    Ambiguous,
}

const INVERSE_SAMPLE_COUNT: usize = 32;
pub(super) const INVERSE_PARAMETER_TOLERANCE: f64 = 1.0e-10;
pub(super) const INVERSE_ABSOLUTE_TOLERANCE_MM: f64 = 1.0e-6;
pub(super) const INVERSE_RELATIVE_TOLERANCE: f64 = 8.0 * f64::EPSILON;

/// The looser of two acceptance tolerances.
///
/// Two independently derived bounds gate the same acceptance, and the
/// acceptance holds when either of them does, so the looser bound is the gate.
/// This is not a floor under a value: both arguments are tolerances.
pub(super) fn looser_tolerance(left: f64, right: f64) -> f64 {
    if left >= right {
        left
    } else {
        right
    }
}

/// The larger of the absolute inverse tolerance and the coordinate-scaled
/// relative tolerance. An empty point set uses the absolute tolerance.
pub(super) fn inverse_coordinate_tolerance(
    ctx: &DecodeContext<'_>,
    points: impl IntoIterator<Item = cadmpeg_ir::math::Point3>,
) -> Result<f64, cadmpeg_core::CodecError> {
    let mut largest = None;
    let mut points = points.into_iter();
    while let Some(point) =
        ctx.next_charged(&mut points, "measure Parasolid inverse coordinate scale")?
    {
        for magnitude in [point.x.abs(), point.y.abs(), point.z.abs()] {
            if magnitude.is_finite() && largest.is_none_or(|known| magnitude > known) {
                largest = Some(magnitude);
            }
        }
    }
    Ok(match largest {
        Some(scale) => looser_tolerance(
            INVERSE_ABSOLUTE_TOLERANCE_MM,
            scale * INVERSE_RELATIVE_TOLERANCE,
        ),
        None => INVERSE_ABSOLUTE_TOLERANCE_MM,
    })
}

/// Borrow the rectangular surface poles in u-major order.
/// Each in-bounds index yields one pole, so the consumer can admit each
/// fixed-step projection before advancing this iterator.
pub(super) fn surface_poles<'s>(
    ctx: &DecodeContext<'_>,
    surface: &'s cadmpeg_ir::geometry::nurbs::NurbsSurface,
) -> Result<impl Iterator<Item = FinitePoint3> + 's, cadmpeg_core::CodecError> {
    let v_count = surface.v_count();
    let count = surface.u_count().checked_mul(v_count).ok_or_else(|| {
        ctx.refuse_codec_limit("scan Parasolid surface poles", u64::MAX - 1, u64::MAX)
    })?;
    Ok((0..count)
        .filter_map(move |index| surface.pole(index / v_count, index % v_count)))
}

fn golden_section_minimum<F>(
    mut left: f64,
    mut right: f64,
    objective: &mut F,
) -> Result<Option<(f64, f64)>, cadmpeg_core::CodecError>
where
    F: FnMut(f64) -> Result<Option<f64>, cadmpeg_core::CodecError>,
{
    let ratio = (5.0_f64.sqrt() - 1.0) / 2.0;
    let Some(a) = cadmpeg_ir::math::interpolate(left, right, 1.0 - ratio) else {
        return Ok(None);
    };
    let Some(b) = cadmpeg_ir::math::interpolate(left, right, ratio) else {
        return Ok(None);
    };
    let mut a = a.get();
    let mut b = b.get();
    let Some(mut da) = objective(a)? else {
        return Ok(None);
    };
    let Some(mut db) = objective(b)? else {
        return Ok(None);
    };
    for _ in 0..80 {
        if da <= db {
            right = b;
            b = a;
            db = da;
            let Some(next) = cadmpeg_ir::math::interpolate(left, right, 1.0 - ratio) else {
                return Ok(None);
            };
            a = next.get();
            let Some(next) = objective(a)? else {
                return Ok(None);
            };
            da = next;
        } else {
            left = a;
            a = b;
            da = db;
            let Some(next) = cadmpeg_ir::math::interpolate(left, right, ratio) else {
                return Ok(None);
            };
            b = next.get();
            let Some(next) = objective(b)? else {
                return Ok(None);
            };
            db = next;
        }
    }
    let Some(parameter) = cadmpeg_ir::math::interpolate(left, right, 0.5) else {
        return Ok(None);
    };
    let parameter = parameter.get();
    Ok(objective(parameter)?.map(|distance| (parameter, distance)))
}

/// Candidate pair backing lives through scalar inverse selection.
pub(super) struct InverseCandidates<'ctx, 'arena> {
    context: &'ctx DecodeContext<'arena>,
    pub(super) values: Vec<(f64, f64)>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl InverseCandidates<'_, '_> {
    pub(super) fn resolve(self, tolerance: f64, domain: [f64; 2])
        -> Result<InverseResolution<f64>, cadmpeg_core::CodecError>
    {
        // The remaining lease stays live until the consumed values have been
        // destroyed inside selection, including any refusal or unwind.
        unique_inverse_parameter(self.context, self.values, tolerance, domain)
    }
}

/// Find one representative for every sampled local minimum of an objective on
/// each nonzero knot span. Endpoints are always retained, so a candidate at a
/// knot is not lost when adjacent spans share it.
pub(super) fn sampled_parameter_minima<'ctx, 'arena, F>(
    ctx: &'ctx DecodeContext<'arena>,
    knots: &[f64],
    domain: [f64; 2],
    mut objective: F,
) -> Result<Option<InverseCandidates<'ctx, 'arena>>, cadmpeg_core::CodecError>
where
    F: FnMut(f64) -> Result<Option<f64>, cadmpeg_core::CodecError>,
{
    let (candidates, storage) = ctx.with_scoped_storage("hold Parasolid inverse candidates", || {
    let mut candidates = Vec::new();
    for span in ctx
        .admit_iter(knots, "sample Parasolid inverse knot spans")?
        .windows(const { crate::nonzero(2) })
        .filter(|span| span[0] < span[1])
    {
        let start = span[0].max(domain[0]);
        let end = span[1].min(domain[1]);
        if start >= end {
            continue;
        }
        let mut samples_storage = ctx.reserve_scoped(0, "SLDPRT temporary vector storage")?;
        let mut samples = Vec::new();
        samples_storage.with_storage(|| {
            ctx.reserve_capacity(
                &mut samples,
                INVERSE_SAMPLE_COUNT + 1,
                "sample Parasolid inverse span",
            )
        })?;
        for index in 0..=INVERSE_SAMPLE_COUNT {
            ctx.charge_work(1, "sample Parasolid inverse span")?;
            let (Some(position), Some(span_count)) =
                (f64_from_index(index), f64_from_index(INVERSE_SAMPLE_COUNT))
            else {
                return Ok(None);
            };
            let Some(parameter) = cadmpeg_ir::math::interpolate(start, end, position / span_count)
            else {
                return Ok(None);
            };
            let parameter = parameter.get();
            let Some(distance) = objective(parameter)? else {
                return Ok(None);
            };
            samples_storage.with_storage(|| {
                ctx.push_vec(
                    &mut (samples),
                    (parameter, distance),
                    "sample Parasolid inverse span",
                )
            })?;
        }
        ctx.extend_from_slice(
            &mut candidates,
            &samples,
            "collect Parasolid inverse candidates",
        )?;
        for index in 1..INVERSE_SAMPLE_COUNT {
            if samples[index].1 <= samples[index - 1].1 && samples[index].1 <= samples[index + 1].1
            {
                let Some(minimum) = golden_section_minimum(
                    samples[index - 1].0,
                    samples[index + 1].0,
                    &mut objective,
                )?
                else {
                    return Ok(None);
                };
                ctx.reserve_vec(&mut candidates, 1, "collect Parasolid inverse minima")?;
                candidates.push(minimum);
            }
        }
    }
    Ok::<_, cadmpeg_core::CodecError>(Some(candidates))
    })?;
    Ok(candidates.map(|values| InverseCandidates { context: ctx, values, _storage: storage }))
}

pub(super) fn unique_inverse_parameter(
    ctx: &DecodeContext<'_>,
    mut candidates: Vec<(f64, f64)>,
    tolerance: f64,
    parameter_domain: [f64; 2],
) -> Result<InverseResolution<f64>, cadmpeg_core::CodecError> {
    let tolerance_squared = tolerance * tolerance;
    ctx.retain_vec(
        &mut candidates,
        |(parameter, error)| {
            Ok(parameter.is_finite() && error.is_finite() && *error <= tolerance_squared)
        },
        "select Parasolid inverse parameters",
    )?;
    ctx.stable_sort_by(
        &mut candidates,
        |value| &value.0,
        f64::total_cmp,
        "sort Parasolid inverse parameters",
    )?;
    let parameter_tolerance = (INVERSE_PARAMETER_TOLERANCE * parameter_domain[1]
        - INVERSE_PARAMETER_TOLERANCE * parameter_domain[0])
        .abs();
    let mut unique_len = 0;
    for index in ctx.admit_iter(0..candidates.len(), "merge Parasolid inverse parameters")? {
        let candidate = candidates[index];
        if unique_len > 0
            && (candidate.0 - candidates[unique_len - 1].0).abs() <= parameter_tolerance
        {
            if candidate.1 < candidates[unique_len - 1].1 {
                candidates[unique_len - 1] = candidate;
            }
        } else {
            candidates[unique_len] = candidate;
            unique_len += 1;
        }
    }
    candidates.truncate(unique_len);
    Ok(match candidates.as_slice() {
        [] => InverseResolution::NoMatch,
        [(parameter, _)] => InverseResolution::Unique(*parameter),
        _ => InverseResolution::Ambiguous,
    })
}

pub(super) fn nurbs_parameter_at_point(
    ctx: &DecodeContext<'_>,
    nurbs: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    target: cadmpeg_ir::math::Point3,
) -> Result<InverseResolution<f64>, cadmpeg_core::CodecError> {
    let squared_distance = |parameter: f64| {
        let Some(point) = crate::brep::evaluation::nurbs_curve_point(ctx, nurbs, parameter)? else {
            return Ok(None);
        };
        Ok(Some(
            (point.x - target.x).powi(2)
                + (point.y - target.y).powi(2)
                + (point.z - target.z).powi(2),
        ))
    };
    let Some(domain) = nurbs_curve_parameter_domain(nurbs)
        .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints)
    else {
        return Ok(InverseResolution::NoMatch);
    };
    let Some(candidates) = sampled_parameter_minima(ctx, nurbs.knots(), domain, squared_distance)?
    else {
        return Ok(InverseResolution::NoMatch);
    };
    let tolerance = match nurbs.pole_rows() {
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => {
            inverse_coordinate_tolerance(
                ctx,
                points.iter()
                    .copied()
                    .map(FinitePoint3::get)
                    .chain(std::iter::once(target)),
            )?
        }
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => {
            inverse_coordinate_tolerance(
                ctx,
                points.iter()
                    .map(|pole| pole.point.get())
                    .chain(std::iter::once(target)),
            )?
        }
    };
    candidates.resolve(tolerance, domain)
}

pub(super) fn ruled_surface_line_pcurve(
    ctx: &DecodeContext<'_>,
    surface: &cadmpeg_ir::geometry::nurbs::NurbsSurface,
    fixed_axis: SurfaceParameterAxis,
    line_origin: cadmpeg_ir::math::Point3,
    line_direction: cadmpeg_ir::math::Vector3,
) -> Result<InverseResolution<PcurveGeometry>, cadmpeg_core::CodecError> {
    let (uc, vc) = (surface.u_count(), surface.v_count());
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
    let (fixed_degree, fixed_count, fixed_knots) = match fixed_axis {
        SurfaceParameterAxis::U => (surface.u_degree(), uc, surface.u_knots()),
        SurfaceParameterAxis::V => (surface.v_degree(), vc, surface.v_knots()),
    };
    let (Some(&varying_min), Some(&varying_max)) = (varying_knots.get(1), varying_knots.get(2))
    else {
        return Ok(InverseResolution::NoMatch);
    };
    if varying_degree != 1
        || varying_count != 2
        || varying_periodic
        || varying_knots.as_slice() != [varying_min, varying_min, varying_max, varying_max]
        || varying_min >= varying_max
    {
        return Ok(InverseResolution::NoMatch);
    }
    if matches!(
        surface.pole_grid(),
        cadmpeg_ir::geometry::nurbs::NurbsPoleGrid::Rational { .. }
    ) && ctx.any_by(
        0..fixed_count,
        |fixed| {
            let ((a_u, a_v), (b_u, b_v)) = match fixed_axis {
                SurfaceParameterAxis::U => ((fixed, 0), (fixed, 1)),
                SurfaceParameterAxis::V => ((0, fixed), (1, fixed)),
            };
            Ok(match (surface.weight(a_u, a_v), surface.weight(b_u, b_v)) {
                (Some(a), Some(b)) => (a.get() - b.get()).abs() > EPS_NURBS_WEIGHT,
                _ => true,
            })
        },
        "compare ruled surface weights",
    )? {
        return Ok(InverseResolution::NoMatch);
    }
    let Some(fixed_degree) = usize::try_from(fixed_degree).ok() else {
        return Ok(InverseResolution::NoMatch);
    };
    let (Some(&fixed_min), Some(&fixed_max)) =
        (fixed_knots.get(fixed_degree), fixed_knots.get(fixed_count))
    else {
        return Ok(InverseResolution::NoMatch);
    };
    if !fixed_min.is_finite() || !fixed_max.is_finite() || fixed_min >= fixed_max {
        return Ok(InverseResolution::NoMatch);
    }
    let evaluate_ruling =
        |fixed: f64| -> Result<Option<(FinitePoint3, FinitePoint3)>, cadmpeg_core::CodecError> {
            let parameters = |varying| match fixed_axis {
                SurfaceParameterAxis::U => (fixed, varying),
                SurfaceParameterAxis::V => (varying, fixed),
            };
            let (u0, v0) = parameters(varying_min);
            let (u1, v1) = parameters(varying_max);
            let Some(first) = crate::brep::evaluation::nurbs_surface_point(ctx, surface, u0, v0)? else {
                return Ok(None);
            };
            let Some(second) = crate::brep::evaluation::nurbs_surface_point(ctx, surface, u1, v1)? else {
                return Ok(None);
            };
            Ok(Some((first, second)))
        };
    let direction_squared = line_direction.x * line_direction.x
        + line_direction.y * line_direction.y
        + line_direction.z * line_direction.z;
    if direction_squared <= f64::EPSILON {
        return Ok(InverseResolution::NoMatch);
    }
    let perpendicular_squared = |point: cadmpeg_ir::math::Point3| {
        let relative = [
            point.x - line_origin.x,
            point.y - line_origin.y,
            point.z - line_origin.z,
        ];
        let along = (relative[0] * line_direction.x
            + relative[1] * line_direction.y
            + relative[2] * line_direction.z)
            / direction_squared;
        (relative[0] - along * line_direction.x).powi(2)
            + (relative[1] - along * line_direction.y).powi(2)
            + (relative[2] - along * line_direction.z).powi(2)
    };
    let objective = |parameter: f64| {
        let Some((a, b)) = evaluate_ruling(parameter)? else {
            return Ok(None);
        };
        Ok(Some(
            perpendicular_squared(a.get()).max(perpendicular_squared(b.get())),
        ))
    };
    let Some(candidates) =
        sampled_parameter_minima(ctx, fixed_knots, [fixed_min, fixed_max], objective)?
    else {
        return Ok(InverseResolution::NoMatch);
    };
    let resolution = candidates.resolve(
        inverse_coordinate_tolerance(
            ctx,
            surface_poles(ctx, surface)?
                .map(FinitePoint3::get)
                .chain(std::iter::once(line_origin)),
        )?,
        [fixed_min, fixed_max],
    )?;
    let fixed = match resolution {
        InverseResolution::Unique(fixed) => fixed,
        InverseResolution::NoMatch => return Ok(InverseResolution::NoMatch),
        InverseResolution::Ambiguous => return Ok(InverseResolution::Ambiguous),
    };
    let Some((a, b)) = evaluate_ruling(fixed)? else {
        return Ok(InverseResolution::NoMatch);
    };
    let delta = [b.x - a.x, b.y - a.y, b.z - a.z];
    let delta_squared = delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2];
    if delta_squared <= f64::EPSILON {
        return Ok(InverseResolution::NoMatch);
    }
    let project = |value: [f64; 3]| {
        (value[0] * delta[0] + value[1] * delta[1] + value[2] * delta[2]) / delta_squared
    };
    let offset = project([
        line_origin.x - a.x,
        line_origin.y - a.y,
        line_origin.z - a.z,
    ]);
    let rate = project([line_direction.x, line_direction.y, line_direction.z]);
    if rate == 0.0 {
        return Ok(InverseResolution::NoMatch);
    }
    let domain = varying_max - varying_min;
    Ok(InverseResolution::Unique(match fixed_axis {
        SurfaceParameterAxis::U => PcurveGeometry::Line(
            match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(fixed, varying_min + offset * domain),
                cadmpeg_ir::math::Point2::new(0.0, rate * domain),
            ) {
                Ok(payload) => payload,
                Err(_) => return Ok(InverseResolution::NoMatch),
            },
        ),
        SurfaceParameterAxis::V => PcurveGeometry::Line(
            match cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(varying_min + offset * domain, fixed),
                cadmpeg_ir::math::Point2::new(rate * domain, 0.0),
            ) {
                Ok(payload) => payload,
                Err(_) => return Ok(InverseResolution::NoMatch),
            },
        ),
    }))
}

#[cfg(test)]
mod tests;
