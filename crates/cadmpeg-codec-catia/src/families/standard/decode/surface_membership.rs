// SPDX-License-Identifier: Apache-2.0
//! Surface point witnesses for standard NURBS membership.

use super::{
    nurbs_surface_parameter_domain, NURBS_SURFACE_BACKTRACK_STEPS, NURBS_SURFACE_MAX_SEEDS,
    NURBS_SURFACE_MEMBERSHIP_TOLERANCE, NURBS_SURFACE_REFINEMENT_ITERATIONS,
    NURBS_SURFACE_SEEDS_PER_SPAN,
};
use cadmpeg_ir::geometry::nurbs::NurbsSurface;
use cadmpeg_ir::math::{Point2, Point3};

fn nurbs_surface_point_distance(
    surface: &NurbsSurface,
    point: Point3,
    uv: Point2,
) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit> {
    let Some(position) = cadmpeg_ir::eval::finite_or_refusal(
        cadmpeg_ir::eval::nurbs_surface_point(surface, uv.u, uv.v),
    )?
    else {
        return Ok(None);
    };
    let distance = position.distance(point);
    Ok(distance.is_finite().then_some(distance))
}

fn refine_nurbs_surface_point(
    surface: &NurbsSurface,
    point: Point3,
    seed: Point2,
    domains: [[f64; 2]; 2],
) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit> {
    let mut parameters = seed;
    for _ in 0..NURBS_SURFACE_REFINEMENT_ITERATIONS {
        let Some(partials) = cadmpeg_ir::eval::finite_or_refusal(
            cadmpeg_ir::eval::nurbs_surface_partials(surface, parameters.u, parameters.v),
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
        let Some(current) = nurbs_surface_point_distance(surface, point, parameters)? else {
            return Ok(None);
        };
        let mut scale = 1.0;
        let mut accepted = None;
        for _ in 0..NURBS_SURFACE_BACKTRACK_STEPS {
            let candidate = Point2::new(
                (parameters.u - scale * step.u).clamp(domains[0][0], domains[0][1]),
                (parameters.v - scale * step.v).clamp(domains[1][0], domains[1][1]),
            );
            let Some(distance) = nurbs_surface_point_distance(surface, point, candidate)? else {
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
    nurbs_surface_point_distance(surface, point, parameters)
}

pub(super) fn nurbs_surface_witness_distance(
    surface: &NurbsSurface,
    point: Point3,
) -> Result<Option<f64>, cadmpeg_core::decode::ResourceLimit> {
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
    let u_spans = u_knots.windows(2).filter(|pair| pair[0] != pair[1]).count();
    let v_spans = v_knots.windows(2).filter(|pair| pair[0] != pair[1]).count();
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
        if let Some(distance) = refine_nurbs_surface_point(surface, point, seed, domains)? {
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
            for pair in knots.windows(2).filter(|pair| pair[0] != pair[1]) {
                for step in 0..NURBS_SURFACE_SEEDS_PER_SPAN {
                    let fraction = match cadmpeg_core::convert::f64_from_index(step) { Some(value) => value, None => return Ok(None) } / match cadmpeg_core::convert::f64_from_index(NURBS_SURFACE_SEEDS_PER_SPAN - 1) { Some(value) => value, None => return Ok(None) };
                    if cadmpeg_ir::math::interpolate(pair[0], pair[1], fraction).is_none() {
                        return Ok(None);
                    }
                }
            }
        }
        for u in 0..SIDE {
            for v in 0..SIDE {
                let u_fraction = match cadmpeg_core::convert::f64_from_index(u) { Some(value) => value, None => return Ok(None) } / match cadmpeg_core::convert::f64_from_index(SIDE - 1) { Some(value) => value, None => return Ok(None) };
                let v_fraction = match cadmpeg_core::convert::f64_from_index(v) { Some(value) => value, None => return Ok(None) } / match cadmpeg_core::convert::f64_from_index(SIDE - 1) { Some(value) => value, None => return Ok(None) };
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
        for u_pair in u_knots.windows(2).filter(|pair| pair[0] != pair[1]) {
            for u_step in 0..NURBS_SURFACE_SEEDS_PER_SPAN {
                let u_fraction = match cadmpeg_core::convert::f64_from_index(u_step) { Some(value) => value, None => return Ok(None) } / match cadmpeg_core::convert::f64_from_index(NURBS_SURFACE_SEEDS_PER_SPAN - 1) { Some(value) => value, None => return Ok(None) };
                let Some(u) = cadmpeg_ir::math::interpolate(u_pair[0], u_pair[1], u_fraction)
                else {
                    return Ok(None);
                };
                for v_pair in v_knots.windows(2).filter(|pair| pair[0] != pair[1]) {
                    for v_step in 0..NURBS_SURFACE_SEEDS_PER_SPAN {
                        let v_fraction = match cadmpeg_core::convert::f64_from_index(v_step) { Some(value) => value, None => return Ok(None) } / match cadmpeg_core::convert::f64_from_index(NURBS_SURFACE_SEEDS_PER_SPAN - 1) { Some(value) => value, None => return Ok(None) };
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
