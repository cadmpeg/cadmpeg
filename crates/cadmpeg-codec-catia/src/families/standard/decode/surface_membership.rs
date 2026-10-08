// SPDX-License-Identifier: Apache-2.0
//! Surface point witnesses for standard NURBS membership.

use cadmpeg_core::convert::f64_from_index;
use std::num::NonZeroUsize;

use super::{
    nurbs_surface_parameter_domain, NURBS_SURFACE_BACKTRACK_STEPS, NURBS_SURFACE_MAX_SEEDS,
    NURBS_SURFACE_MEMBERSHIP_TOLERANCE, NURBS_SURFACE_REFINEMENT_ITERATIONS,
    NURBS_SURFACE_SEEDS_PER_SPAN,
};
use cadmpeg_ir::geometry::nurbs::NurbsSurface;
use cadmpeg_ir::math::{Point2, Point3};

fn nurbs_surface_point_distance(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface: &NurbsSurface,
    point: Point3,
    uv: Point2,
) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit> {
    let Some(position) = cadmpeg_ir::eval::finite_or_refusal(
        cadmpeg_ir::eval::decode::nurbs_surface_point(ctx, surface, uv.u, uv.v),
    )?
    else {
        return Ok(None);
    };
    let distance = position.distance(point);
    Ok(distance.is_finite().then_some(distance))
}

fn refine_nurbs_surface_point(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface: &NurbsSurface,
    point: Point3,
    seed: Point2,
    domains: [[f64; 2]; 2],
) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit> {
    ctx.charge_work_limit(0, "catia surface refinement boundary")?;
    let mut parameters = seed;
    for _ in 0..NURBS_SURFACE_REFINEMENT_ITERATIONS {
        ctx.charge_work_limit(1, "catia surface refinement step")?;
        let Some(partials) = cadmpeg_ir::eval::finite_or_refusal(
            cadmpeg_ir::eval::nurbs_surface_partials(ctx, surface, parameters.u, parameters.v),
        )?
        else {
            return Ok(None);
        };
        let residual = partials.point.vector_from(point);
        let Some((u, v)) =
            cadmpeg_ir::math::solve::least_squares_step(partials.du, partials.dv, residual)
        else {
            break;
        };
        let step = Point2::new(u.get(), v.get());
        let Some(current) = nurbs_surface_point_distance(ctx, surface, point, parameters)? else {
            return Ok(None);
        };
        let mut scale = 1.0;
        let mut accepted = None;
        for _ in 0..NURBS_SURFACE_BACKTRACK_STEPS {
            ctx.charge_work_limit(1, "catia surface refinement backtrack")?;
            let candidate = Point2::new(
                (parameters.u - scale * step.u).clamp(domains[0][0], domains[0][1]),
                (parameters.v - scale * step.v).clamp(domains[1][0], domains[1][1]),
            );
            let Some(distance) = nurbs_surface_point_distance(ctx, surface, point, candidate)?
            else {
                return Ok(None);
            };
            if distance <= current {
                accepted = Some((candidate, distance));
                break;
            }
            scale *= 0.5;
        }
        let Some((candidate, distance)) = accepted else {
            break;
        };
        parameters = candidate;
        if distance <= NURBS_SURFACE_MEMBERSHIP_TOLERANCE {
            return Ok(Some(distance));
        }
    }
    nurbs_surface_point_distance(ctx, surface, point, parameters)
}

pub(super) fn nurbs_surface_witness_distance(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface: &NurbsSurface,
    point: Point3,
) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit> {
    ctx.charge_work_limit(0, "catia surface witness boundary")?;
    let Some(domains) = nurbs_surface_parameter_domain(surface) else {
        return Ok(None);
    };
    let Ok(u_degree) = usize::try_from(surface.u_degree()) else {
        return Ok(None);
    };
    let Ok(v_degree) = usize::try_from(surface.v_degree()) else {
        return Ok(None);
    };
    let Some(u_knots) = surface.u_knots().get(u_degree..=surface.u_count()) else {
        return Ok(None);
    };
    let Some(v_knots) = surface.v_knots().get(v_degree..=surface.v_count()) else {
        return Ok(None);
    };
    let Some(knot_window_size) = NonZeroUsize::new(2) else {
        return Ok(None);
    };
    let mut u_spans = 0_usize;
    for pair in ctx
        .admit_iter(u_knots, "catia surface knot span count")?
        .windows(knot_window_size)
    {
        u_spans += usize::from(pair[0] != pair[1]);
    }
    let mut v_spans = 0_usize;
    for pair in ctx
        .admit_iter(v_knots, "catia surface knot span count")?
        .windows(knot_window_size)
    {
        v_spans += usize::from(pair[0] != pair[1]);
    }
    if u_spans == 0 || v_spans == 0 {
        return Ok(None);
    }
    let Some(samples) = u_spans
        .checked_mul(NURBS_SURFACE_SEEDS_PER_SPAN)
        .and_then(|count| count.checked_mul(v_spans))
        .and_then(|count| count.checked_mul(NURBS_SURFACE_SEEDS_PER_SPAN))
    else {
        return Ok(None);
    };
    let mut best: Option<f64> = None;
    let mut consider = |seed: Point2| -> Result<(), cadmpeg_core::decode::ResourceLimit> {
        ctx.charge_work_limit(1, "catia surface witness seed")?;
        if let Some(distance) = refine_nurbs_surface_point(ctx, surface, point, seed, domains)? {
            best = Some(best.map_or(distance, |previous| {
                if previous.total_cmp(&distance).is_le() {
                    previous
                } else {
                    distance
                }
            }));
        }
        Ok(())
    };
    if samples > NURBS_SURFACE_MAX_SEEDS {
        const SIDE: usize = 16;
        for knots in [u_knots, v_knots] {
            for pair in ctx
                .admit_iter(knots, "catia surface knot span visit")?
                .windows(knot_window_size)
            {
                if pair[0] == pair[1] {
                    continue;
                }
                for step in 0..NURBS_SURFACE_SEEDS_PER_SPAN {
                    ctx.charge_work_limit(1, "catia surface knot sample")?;
                    let fraction = match f64_from_index(step) {
                        Some(value) => value,
                        None => return Ok(None),
                    } / match f64_from_index(NURBS_SURFACE_SEEDS_PER_SPAN - 1) {
                        Some(value) => value,
                        None => return Ok(None),
                    };
                    if cadmpeg_ir::math::interpolate(pair[0], pair[1], fraction).is_none() {
                        return Ok(None);
                    }
                }
            }
        }
        for u in 0..SIDE {
            for v in 0..SIDE {
                let u_fraction = match f64_from_index(u) {
                    Some(value) => value,
                    None => return Ok(None),
                } / match f64_from_index(SIDE - 1) {
                    Some(value) => value,
                    None => return Ok(None),
                };
                let v_fraction = match f64_from_index(v) {
                    Some(value) => value,
                    None => return Ok(None),
                } / match f64_from_index(SIDE - 1) {
                    Some(value) => value,
                    None => return Ok(None),
                };
                let Some(u) =
                    cadmpeg_ir::math::interpolate(domains[0][0], domains[0][1], u_fraction)
                else {
                    return Ok(None);
                };
                let Some(v) =
                    cadmpeg_ir::math::interpolate(domains[1][0], domains[1][1], v_fraction)
                else {
                    return Ok(None);
                };
                consider(Point2::new(u.get(), v.get()))?;
            }
        }
    } else {
        for u_pair in ctx
            .admit_iter(u_knots, "catia surface knot span visit")?
            .windows(knot_window_size)
        {
            if u_pair[0] == u_pair[1] {
                continue;
            }
            for u_step in 0..NURBS_SURFACE_SEEDS_PER_SPAN {
                ctx.charge_work_limit(1, "catia surface u seed")?;
                let u_fraction = match f64_from_index(u_step) {
                    Some(value) => value,
                    None => return Ok(None),
                } / match f64_from_index(NURBS_SURFACE_SEEDS_PER_SPAN - 1) {
                    Some(value) => value,
                    None => return Ok(None),
                };
                let Some(u) = cadmpeg_ir::math::interpolate(u_pair[0], u_pair[1], u_fraction)
                else {
                    return Ok(None);
                };
                for v_pair in ctx
                    .admit_iter(v_knots, "catia surface knot span visit")?
                    .windows(knot_window_size)
                {
                    if v_pair[0] == v_pair[1] {
                        continue;
                    }
                    for v_step in 0..NURBS_SURFACE_SEEDS_PER_SPAN {
                        let v_fraction = match f64_from_index(v_step) {
                            Some(value) => value,
                            None => return Ok(None),
                        } / match f64_from_index(NURBS_SURFACE_SEEDS_PER_SPAN - 1)
                        {
                            Some(value) => value,
                            None => return Ok(None),
                        };
                        let Some(v) =
                            cadmpeg_ir::math::interpolate(v_pair[0], v_pair[1], v_fraction)
                        else {
                            return Ok(None);
                        };
                        consider(Point2::new(u.get(), v.get()))?;
                    }
                }
            }
        }
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::{
        nurbs_surface_point_distance, nurbs_surface_witness_distance, refine_nurbs_surface_point,
        NurbsSurface, Point2, Point3,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_ir::geometry::nurbs::{NurbsSurfaceAxis, NurbsSurfaceLanes};

    fn surface() -> NurbsSurface {
        NurbsSurface::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
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
        .expect("fixture admission")
        .expect("unit square")
    }

    #[test]
    fn surface_witness_refuses_knot_scan_work() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty fixture root");
        let original = nurbs_surface_witness_distance(&ctx, &surface(), Point3::new(0.3, 0.7, 0.0))
            .expect_err("knot span work refuses");
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, "catia surface knot span count");
        // The two-knot span lane is admitted before its one window is visited.
        assert_eq!(
            (original.limit, original.used, original.additional),
            (0, 0, 2)
        );
        assert_eq!(ctx.resource_refusal(), Some(original));
    }

    #[test]
    fn surface_refinement_refuses_step_work() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty fixture root");
        let surface = surface();
        let original = refine_nurbs_surface_point(
            &ctx,
            &surface,
            Point3::new(0.3, 0.7, 0.0),
            Point2::new(0.0, 0.0),
            [[0.0, 1.0]; 2],
        )
        .expect_err("refinement work refuses");
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, "catia surface refinement step");
        // Each refinement iteration charges one step before it evaluates.
        assert_eq!(
            (original.limit, original.used, original.additional),
            (0, 0, 1)
        );
        assert_eq!(
            nurbs_surface_point_distance(
                &ctx,
                &surface,
                Point3::new(0.0, 0.0, 0.0),
                Point2::new(f64::NAN, 0.0)
            ),
            Err(original)
        );
        assert_eq!(
            nurbs_surface_witness_distance(&ctx, &surface, Point3::new(0.0, 0.0, 0.0)),
            Err(original)
        );
        assert_eq!(ctx.resource_refusal(), Some(original));
    }

    #[test]
    fn surface_bounds_refuses_pole_work() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty fixture root");
        let original =
            super::super::point_on_nurbs_surface(&ctx, Point3::new(2.0, 2.0, 2.0), &surface())
                .expect_err("pole work refuses");
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, "catia surface control bounds");
        // The two pole rows of the control net are admitted before the first visit.
        assert_eq!(
            (original.limit, original.used, original.additional),
            (0, 0, 2)
        );
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}
