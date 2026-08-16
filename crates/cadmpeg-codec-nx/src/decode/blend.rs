// SPDX-License-Identifier: Apache-2.0
//! Blend-surface evaluation, spine inversion, and closest-pcurve search.

use super::offset::{
    least_squares_step, offset_surface_parameters_with_tolerance_with_index,
    parameter_derivative_step, point_distance,
};
use super::support_uv::{parameterization_equivalent_surfaces, procedural_surface_for_carrier};
use crate::native::vector::{cross_vector, dot_vector, unit_vector};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{
    analytic_surface_parameters, curve_point, curve_second_derivative, curve_tangent,
    model_surface_partials_by_id, model_surface_point_by_id,
    nurbs_surface_parameter_within_tolerance, pcurve_tangent, pcurve_uv, surface_point,
};
use cadmpeg_ir::geometry::{
    knots_nondecreasing, BlendCrossSection, BlendRadiusLaw, CurveGeometry, NurbsCurve,
    PcurveGeometry, ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};

pub(crate) fn decoded_surface_point_inner(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    u: f64,
    v: f64,
    depth: usize,
) -> Option<Point3> {
    (depth < 32).then_some(())?;
    model_surface_point_by_id(index, surface, u, v)
        .or_else(|| blend_surface_point_inner_with_index(index, surface, u, v, depth + 1))
}

pub(crate) fn decoded_surface_point_with_geometry(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
    depth: usize,
) -> Option<Point3> {
    surface_point(geometry, u, v)
        .or_else(|| decoded_surface_point_inner(index, surface, u, v, depth))
}

#[cfg(test)]
pub(crate) fn blend_surface_parameters(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
) -> Option<Point2> {
    let index = cadmpeg_ir::index::ModelIndex::new(ir);
    blend_surface_parameters_inner(
        &index,
        surface,
        point,
        seed,
        None,
        BlendParameterGrid::Build,
        0,
    )
}

#[cfg(test)]
pub(crate) fn blend_surface_parameters_for_fit(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: f64,
) -> Option<Point2> {
    let index = cadmpeg_ir::index::ModelIndex::new(ir);
    blend_surface_parameters_for_fit_with_grid(
        &index,
        surface,
        point,
        seed,
        fit_tolerance,
        BlendParameterGrid::Build,
    )
}

#[derive(Clone, Copy)]
pub(crate) enum BlendParameterGrid {
    Build,
    Disabled,
}

pub(crate) fn blend_surface_parameters_for_fit_with_grid(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: f64,
    grid: BlendParameterGrid,
) -> Option<Point2> {
    blend_surface_parameters_inner(index, surface, point, seed, Some(fit_tolerance), grid, 0)
}

pub(crate) fn blend_surface_parameters_inner(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    seed: Option<Point2>,
    fit_tolerance: Option<f64>,
    grid: BlendParameterGrid,
    depth: usize,
) -> Option<Point2> {
    (depth < 32).then_some(())?;
    let (_, spine, _, _) = blend_surface_definition_with_index(index, surface)?;
    if let (Some(seed), Some(fit_tolerance)) = (seed, fit_tolerance) {
        if let Some(parameters) =
            refine_blend_surface_parameters_with_index(index, surface, point, seed, depth + 1)
                .filter(|parameters| {
                    blend_surface_point_inner_with_index(
                        index,
                        surface,
                        parameters.u,
                        parameters.v,
                        depth + 1,
                    )
                    .is_some_and(|candidate| point_distance(candidate, point) <= fit_tolerance)
                })
        {
            return Some(parameters);
        }
    }
    let angular = closest_spine_parameter_with_index(index, &spine, point, seed.map(|seed| seed.u))
        .and_then(|u| {
            let (center, tangent, first, second, _) =
                blend_surface_frame_with_index(index, surface, u, depth + 1)?;
            let radial = unit_vector(Vector3::new(
                point.x - center.x,
                point.y - center.y,
                point.z - center.z,
            ))?;
            let alpha = signed_angle(first, second, tangent);
            if !alpha.is_finite() || alpha.abs() <= 1.0e-12 {
                return None;
            }
            let theta = signed_angle(first, radial, tangent);
            (-2..=2)
                .filter_map(|turn| {
                    let v = (theta + f64::from(turn) * std::f64::consts::TAU) / alpha;
                    let candidate =
                        blend_surface_point_inner_with_index(index, surface, u, v, depth + 1)?;
                    let branch_distance = seed.map_or(v.abs(), |seed| (v - seed.v).abs());
                    Some((
                        Point2::new(u, v),
                        point_distance(candidate, point),
                        branch_distance,
                    ))
                })
                .min_by(|first, second| {
                    if (first.1 - second.1).abs() <= 1.0e-12 {
                        first.2.total_cmp(&second.2)
                    } else {
                        first.1.total_cmp(&second.1)
                    }
                })
                .map(|(parameters, _, _)| parameters)
        });
    if let Some(initial) = angular {
        let parameters =
            refine_blend_surface_parameters_with_index(index, surface, point, initial, depth + 1)
                .unwrap_or(initial);
        if let Some(candidate) = blend_surface_point_inner_with_index(
            index,
            surface,
            parameters.u,
            parameters.v,
            depth + 1,
        ) {
            let distance = point_distance(candidate, point);
            if fit_tolerance.is_none_or(|tolerance| distance <= tolerance) {
                return Some(parameters);
            }
        }
    }
    let initial = match grid {
        BlendParameterGrid::Build => {
            coarse_blend_surface_parameters_with_index(index, surface, point, depth + 1)
        }
        BlendParameterGrid::Disabled => None,
    };
    if let Some(initial) = initial {
        let parameters =
            refine_blend_surface_parameters_with_index(index, surface, point, initial, depth + 1)
                .unwrap_or(initial);
        if (0.0..=1.0).contains(&parameters.v) {
            let candidate = blend_surface_point_inner_with_index(
                index,
                surface,
                parameters.u,
                parameters.v,
                depth + 1,
            )?;
            let distance = point_distance(candidate, point);
            if fit_tolerance.is_none_or(|tolerance| distance <= tolerance) {
                return Some(parameters);
            }
        }
    }
    if let Some(fit_tolerance) = fit_tolerance {
        let boundary_parameters = [0usize, 1usize].map(|boundary| {
            blend_boundary_parameter_with_index(
                index,
                surface,
                point,
                boundary,
                seed.map(|seed| seed.u),
                fit_tolerance,
                depth + 1,
            )
        });
        if let Some((parameter, boundary)) = match boundary_parameters {
            [Some(parameter), None] => Some((parameter, 0usize)),
            [None, Some(parameter)] => Some((parameter, 1usize)),
            _ => None,
        } {
            return Some(Point2::new(parameter, boundary as f64));
        }
    }
    None
}

#[cfg(test)]
pub(crate) fn coarse_blend_surface_parameters(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    depth: usize,
) -> Option<Point2> {
    let index = cadmpeg_ir::index::ModelIndex::new(ir);
    coarse_blend_surface_parameters_with_index(&index, surface, point, depth)
}

pub(crate) fn coarse_blend_surface_parameters_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    depth: usize,
) -> Option<Point2> {
    let grid = blend_surface_parameter_grid_with_index(index, surface, depth)?;
    closest_blend_surface_grid_parameters(&grid, point)
}

pub(crate) fn blend_surface_parameter_grid_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    depth: usize,
) -> Option<Vec<(Point2, Point3)>> {
    (depth < 32).then_some(())?;
    let (_, spine, _, _) = blend_surface_definition_with_index(index, surface)?;
    let curve = index.curves(spine.0.as_str())?;
    let CurveGeometry::Nurbs(nurbs) = &curve.geometry else {
        return None;
    };
    let degree = usize::try_from(nurbs.degree).ok()?;
    let count = nurbs.control_points.len();
    let domain = [*nurbs.knots.get(degree)?, *nurbs.knots.get(count)?];
    if !domain.into_iter().all(f64::is_finite) || domain[0] >= domain[1] {
        return None;
    }
    let mut grid = Vec::with_capacity(9 * 5);
    for u_index in 0..=8 {
        let u = domain[0] + (domain[1] - domain[0]) * f64::from(u_index) / 8.0;
        let frame = blend_surface_frame_with_index(index, surface, u, depth + 1);
        for v_index in 0..=4 {
            let parameters = Point2::new(u, f64::from(v_index) / 4.0);
            let point = match v_index {
                0 => blend_boundary_point_with_index(index, surface, u, 0, depth + 1),
                4 => blend_boundary_point_with_index(index, surface, u, 1, depth + 1),
                _ => frame.map(|frame| blend_surface_point_from_frame(frame, parameters.v)),
            };
            let Some(point) = point else {
                continue;
            };
            grid.push((parameters, point));
        }
    }
    (!grid.is_empty()).then_some(grid)
}

pub(crate) fn closest_blend_surface_grid_parameters(
    grid: &[(Point2, Point3)],
    point: Point3,
) -> Option<Point2> {
    grid.iter()
        .min_by(|(_, first), (_, second)| {
            point_distance(*first, point).total_cmp(&point_distance(*second, point))
        })
        .map(|(parameters, _)| *parameters)
}

pub(crate) fn blend_surface_parameters_from_grid_for_fit(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    fit_tolerance: f64,
    grid: &[(Point2, Point3)],
) -> Option<Point2> {
    let initial = closest_blend_surface_grid_parameters(grid, point)?;
    let parameters = refine_blend_surface_parameters_with_index(index, surface, point, initial, 0)
        .unwrap_or(initial);
    (0.0..=1.0).contains(&parameters.v).then_some(())?;
    let candidate =
        blend_surface_point_inner_with_index(index, surface, parameters.u, parameters.v, 0)?;
    (point_distance(candidate, point) <= fit_tolerance).then_some(parameters)
}

#[cfg(test)]
pub(crate) fn refine_blend_surface_parameters(
    ir: &CadIr,
    surface: &SurfaceId,
    point: Point3,
    parameters: Point2,
    depth: usize,
) -> Option<Point2> {
    let index = cadmpeg_ir::index::ModelIndex::new(ir);
    refine_blend_surface_parameters_with_index(&index, surface, point, parameters, depth)
}

pub(crate) fn refine_blend_surface_parameters_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    mut parameters: Point2,
    depth: usize,
) -> Option<Point2> {
    (depth < 32).then_some(())?;
    let (_, spine, _, _) = blend_surface_definition_with_index(index, surface)?;
    let u_domain = index
        .curves(spine.0.as_str())
        .and_then(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(nurbs) => {
                let degree = usize::try_from(nurbs.degree).ok()?;
                let count = nurbs.control_points.len();
                Some([*nurbs.knots.get(degree)?, *nurbs.knots.get(count)?])
            }
            _ => None,
        });
    if let Some(domain) = u_domain {
        parameters.u = parameters.u.clamp(domain[0], domain[1]);
    }
    let squared_distance = |candidate: Point3| {
        (candidate.x - point.x).powi(2)
            + (candidate.y - point.y).powi(2)
            + (candidate.z - point.z).powi(2)
    };
    for _ in 0..16 {
        let position = blend_surface_point_inner_with_index(
            index,
            surface,
            parameters.u,
            parameters.v,
            depth + 1,
        )?;
        let residual = Vector3::new(
            position.x - point.x,
            position.y - point.y,
            position.z - point.z,
        );
        let current_distance = squared_distance(position);
        let u_step = parameter_derivative_step(parameters.u, u_domain);
        let derivative = |step: f64| {
            let mut before = parameters;
            let mut after = parameters;
            before.u -= step;
            after.u += step;
            if let Some(domain) = u_domain {
                before.u = before.u.clamp(domain[0], domain[1]);
                after.u = after.u.clamp(domain[0], domain[1]);
            }
            let width = after.u - before.u;
            if !width.is_finite() || width == 0.0 {
                return None;
            }
            let first = blend_surface_point_inner_with_index(
                index,
                surface,
                before.u,
                before.v,
                depth + 1,
            )?;
            let second =
                blend_surface_point_inner_with_index(index, surface, after.u, after.v, depth + 1)?;
            Some(Vector3::new(
                (second.x - first.x) / width,
                (second.y - first.y) / width,
                (second.z - first.z) / width,
            ))
        };
        let du = blend_surface_u_derivative_with_index(
            index,
            surface,
            parameters.u,
            parameters.v,
            depth + 1,
        )
        .or_else(|| derivative(u_step))?;
        let (_, tangent, first, second, radius) =
            blend_surface_frame_with_index(index, surface, parameters.u, depth + 1)?;
        let alpha = signed_angle(first, second, tangent);
        let radial = rodrigues_rotate(first, tangent, parameters.v * alpha);
        let section_tangent = cross_vector(tangent, radial);
        let dv = Vector3::new(
            radius * alpha * section_tangent.x,
            radius * alpha * section_tangent.y,
            radius * alpha * section_tangent.z,
        );
        let Some((step_u, step_v)) = least_squares_step(du, dv, residual) else {
            break;
        };
        let mut scale = 1.0;
        let mut accepted = None;
        for _ in 0..8 {
            let mut candidate =
                Point2::new(parameters.u - scale * step_u, parameters.v - scale * step_v);
            if let Some(domain) = u_domain {
                candidate.u = candidate.u.clamp(domain[0], domain[1]);
            }
            if let Some(position) = blend_surface_point_inner_with_index(
                index,
                surface,
                candidate.u,
                candidate.v,
                depth + 1,
            ) {
                if squared_distance(position) < current_distance {
                    accepted = Some(candidate);
                    break;
                }
            }
            scale *= 0.5;
        }
        let Some(candidate) = accepted else {
            break;
        };
        let converged = (candidate.u - parameters.u).abs() <= 1.0e-12 * (1.0 + parameters.u.abs())
            && (candidate.v - parameters.v).abs() <= 1.0e-12 * (1.0 + parameters.v.abs());
        parameters = candidate;
        if converged {
            break;
        }
    }
    Some(parameters)
}

#[cfg(test)]
pub(crate) fn blend_surface_point(
    ir: &CadIr,
    surface: &SurfaceId,
    u: f64,
    v: f64,
) -> Option<Point3> {
    blend_surface_point_inner(ir, surface, u, v, 0)
}

#[cfg(test)]
pub(crate) fn blend_surface_point_inner(
    ir: &CadIr,
    surface: &SurfaceId,
    u: f64,
    v: f64,
    depth: usize,
) -> Option<Point3> {
    let index = cadmpeg_ir::index::ModelIndex::new(ir);
    blend_surface_point_inner_with_index(&index, surface, u, v, depth)
}

pub(crate) fn blend_surface_point_inner_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    u: f64,
    v: f64,
    depth: usize,
) -> Option<Point3> {
    (depth < 32).then_some(())?;
    if v.to_bits() == 0.0f64.to_bits() {
        return blend_boundary_point_with_index(index, surface, u, 0, depth + 1);
    }
    if v.to_bits() == 1.0f64.to_bits() {
        return blend_boundary_point_with_index(index, surface, u, 1, depth + 1);
    }
    let frame = blend_surface_frame_with_index(index, surface, u, depth + 1)?;
    Some(blend_surface_point_from_frame(frame, v))
}

pub(crate) type BlendSurfaceFrame = (Point3, Vector3, Vector3, Vector3, f64);

pub(crate) fn blend_surface_point_from_frame(
    (center, tangent, first, second, radius): BlendSurfaceFrame,
    v: f64,
) -> Point3 {
    let alpha = signed_angle(first, second, tangent);
    let radial = rodrigues_rotate(first, tangent, v * alpha);
    Point3::new(
        center.x + radius * radial.x,
        center.y + radius * radial.y,
        center.z + radius * radial.z,
    )
}

#[cfg(test)]
pub(crate) fn blend_surface_u_derivative(
    ir: &CadIr,
    surface: &SurfaceId,
    u: f64,
    v: f64,
    depth: usize,
) -> Option<Vector3> {
    let index = cadmpeg_ir::index::ModelIndex::new(ir);
    blend_surface_u_derivative_with_index(&index, surface, u, v, depth)
}

pub(crate) fn blend_surface_u_derivative_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    u: f64,
    v: f64,
    depth: usize,
) -> Option<Vector3> {
    (depth < 32).then_some(())?;
    let (supports, spine, radius, _) = blend_surface_definition_with_index(index, surface)?;
    let carrier = index.curves(spine.0.as_str())?;
    let center = curve_point(&carrier.geometry, u)?;
    let velocity = curve_tangent(&carrier.geometry, u)?;
    let acceleration = curve_second_derivative(&carrier.geometry, u)?;
    let speed = velocity.norm();
    if !speed.is_finite() || speed == 0.0 {
        return None;
    }
    let tangent = Vector3::new(velocity.x / speed, velocity.y / speed, velocity.z / speed);
    let tangential_acceleration = dot_vector(tangent, acceleration);
    let tangent_derivative = Vector3::new(
        (acceleration.x - tangential_acceleration * tangent.x) / speed,
        (acceleration.y - tangential_acceleration * tangent.y) / speed,
        (acceleration.z - tangential_acceleration * tangent.z) / speed,
    );
    let contact_context = BlendContactDerivativeContext {
        index,
        spine: &spine,
        parameter: u,
        center,
        center_derivative: velocity,
        radius,
        depth: depth + 1,
    };
    let (first, first_derivative) = contact_context.direction_derivative(&supports[0])?;
    let (second, second_derivative) = contact_context.direction_derivative(&supports[1])?;

    let cross = cross_vector(first, second);
    let cosine = dot_vector(first, second);
    let sine = dot_vector(cross, tangent);
    let cosine_derivative =
        dot_vector(first_derivative, second) + dot_vector(first, second_derivative);
    let cross_derivative =
        cross_vector(first_derivative, second) + cross_vector(first, second_derivative);
    let sine_derivative =
        dot_vector(cross_derivative, tangent) + dot_vector(cross, tangent_derivative);
    let angle_denominator = cosine * cosine + sine * sine;
    if !angle_denominator.is_finite() || angle_denominator == 0.0 {
        return None;
    }
    let alpha = sine.atan2(cosine);
    let alpha_derivative =
        (cosine * sine_derivative - sine * cosine_derivative) / angle_denominator;
    let theta = v * alpha;
    let theta_derivative = v * alpha_derivative;
    let theta_cosine = theta.cos();
    let theta_sine = theta.sin();
    let tangent_cross_first = cross_vector(tangent, first);
    let tangent_cross_first_derivative =
        cross_vector(tangent_derivative, first) + cross_vector(tangent, first_derivative);
    let tangent_dot_first = dot_vector(tangent, first);
    let tangent_dot_first_derivative =
        dot_vector(tangent_derivative, first) + dot_vector(tangent, first_derivative);
    let radial_component = |first: f64,
                            first_derivative: f64,
                            tangent_cross_first: f64,
                            tangent_cross_first_derivative: f64,
                            tangent: f64,
                            tangent_derivative: f64| {
        first_derivative * theta_cosine - first * theta_sine * theta_derivative
            + tangent_cross_first_derivative * theta_sine
            + tangent_cross_first * theta_cosine * theta_derivative
            + tangent_derivative * tangent_dot_first * (1.0 - theta_cosine)
            + tangent * tangent_dot_first_derivative * (1.0 - theta_cosine)
            + tangent * tangent_dot_first * theta_sine * theta_derivative
    };
    let radial_derivative = Vector3::new(
        radial_component(
            first.x,
            first_derivative.x,
            tangent_cross_first.x,
            tangent_cross_first_derivative.x,
            tangent.x,
            tangent_derivative.x,
        ),
        radial_component(
            first.y,
            first_derivative.y,
            tangent_cross_first.y,
            tangent_cross_first_derivative.y,
            tangent.y,
            tangent_derivative.y,
        ),
        radial_component(
            first.z,
            first_derivative.z,
            tangent_cross_first.z,
            tangent_cross_first_derivative.z,
            tangent.z,
            tangent_derivative.z,
        ),
    );
    Some(Vector3::new(
        velocity.x + radius * radial_derivative.x,
        velocity.y + radius * radial_derivative.y,
        velocity.z + radius * radial_derivative.z,
    ))
}

pub(crate) struct BlendContactDerivativeContext<'a> {
    pub(crate) index: &'a cadmpeg_ir::index::ModelIndex<'a>,
    pub(crate) spine: &'a CurveId,
    pub(crate) parameter: f64,
    pub(crate) center: Point3,
    pub(crate) center_derivative: Vector3,
    pub(crate) radius: f64,
    pub(crate) depth: usize,
}

impl BlendContactDerivativeContext<'_> {
    fn direction_derivative(&self, support: &SurfaceId) -> Option<(Vector3, Vector3)> {
        (self.depth < 32).then_some(())?;
        let pcurve = spine_contact_pcurve(
            self.index.ir(),
            support,
            self.spine,
            self.radius,
            self.depth + 1,
        )?;
        let uv = pcurve_uv(pcurve, self.parameter)?;
        let uv_derivative = pcurve_tangent(pcurve, self.parameter)?;
        let support = model_surface_partials_by_id(self.index, support, uv.u, uv.v)?;
        let contact_derivative = Vector3::new(
            support.du.x * uv_derivative.u + support.dv.x * uv_derivative.v,
            support.du.y * uv_derivative.u + support.dv.y * uv_derivative.v,
            support.du.z * uv_derivative.u + support.dv.z * uv_derivative.v,
        );
        let offset = Vector3::new(
            support.point.x - self.center.x,
            support.point.y - self.center.y,
            support.point.z - self.center.z,
        );
        let magnitude = offset.norm();
        if !magnitude.is_finite() || magnitude == 0.0 {
            return None;
        }
        let direction = Vector3::new(
            offset.x / magnitude,
            offset.y / magnitude,
            offset.z / magnitude,
        );
        let offset_derivative = Vector3::new(
            contact_derivative.x - self.center_derivative.x,
            contact_derivative.y - self.center_derivative.y,
            contact_derivative.z - self.center_derivative.z,
        );
        let radial_derivative = dot_vector(direction, offset_derivative);
        let direction_derivative = Vector3::new(
            (offset_derivative.x - radial_derivative * direction.x) / magnitude,
            (offset_derivative.y - radial_derivative * direction.y) / magnitude,
            (offset_derivative.z - radial_derivative * direction.z) / magnitude,
        );
        Some((direction, direction_derivative))
    }
}

pub(crate) fn blend_surface_frame_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    u: f64,
    depth: usize,
) -> Option<BlendSurfaceFrame> {
    (depth < 32).then_some(())?;
    let (supports, spine, radius, _) = blend_surface_definition_with_index(index, surface)?;
    let center = model_curve_point_with_index(index, &spine, u)?;
    let tangent = model_curve_tangent_with_index(index, &spine, u)?;
    let first = spine_contact_direction_with_index(
        index,
        &supports[0],
        &spine,
        u,
        center,
        radius,
        depth + 1,
    )
    .or_else(|| {
        surface_contact_direction_with_index(index, &supports[0], center, radius, depth + 1)
    })?;
    let second = spine_contact_direction_with_index(
        index,
        &supports[1],
        &spine,
        u,
        center,
        radius,
        depth + 1,
    )
    .or_else(|| {
        surface_contact_direction_with_index(index, &supports[1], center, radius, depth + 1)
    })?;
    Some((center, tangent, first, second, radius))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spine_contact_direction_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    support: &SurfaceId,
    spine: &CurveId,
    parameter: f64,
    center: Point3,
    radius: f64,
    depth: usize,
) -> Option<Vector3> {
    let contact =
        spine_contact_point_with_index(index, support, spine, parameter, radius, depth + 1)?;
    unit_vector(Vector3::new(
        contact.x - center.x,
        contact.y - center.y,
        contact.z - center.z,
    ))
}

pub(crate) fn blend_boundary_point_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    parameter: f64,
    boundary: usize,
    depth: usize,
) -> Option<Point3> {
    (depth < 32).then_some(())?;
    let (supports, spine, radius, _) = blend_surface_definition_with_index(index, surface)?;
    spine_contact_point_with_index(
        index,
        supports.get(boundary)?,
        &spine,
        parameter,
        radius,
        depth + 1,
    )
}

pub(crate) fn blend_boundary_parameter_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    boundary: usize,
    seed: Option<f64>,
    fit_tolerance: f64,
    depth: usize,
) -> Option<f64> {
    (depth < 32).then_some(())?;
    let (_, spine, _, _) = blend_surface_definition_with_index(index, surface)?;
    // A circular blend's u parameter is its spine parameter. Invert that
    // defining carrier directly, then certify the requested boundary point.
    closest_spine_parameter_with_index(index, &spine, point, seed).filter(|parameter| {
        blend_boundary_point_with_index(index, surface, *parameter, boundary, depth + 1)
            .is_some_and(|candidate| point_distance(candidate, point) <= fit_tolerance)
    })
}

#[derive(Clone, Copy)]
pub(crate) struct BoundaryInverseTarget {
    pub(crate) point: Point3,
    pub(crate) seed: Option<Point2>,
    pub(crate) tolerance: f64,
}

pub(crate) fn blend_boundary_parameter_from_support_pcurve(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    ir: &CadIr,
    blend: &SurfaceId,
    support: &SurfaceId,
    support_pcurve: &PcurveGeometry,
    curve_parameter: f64,
    target: BoundaryInverseTarget,
) -> Option<Point2> {
    let support_geometry = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == support)
        .map(|surface| &surface.geometry)?;
    blend_boundary_parameter_from_support_pcurve_with_geometry(
        index,
        ir,
        blend,
        support,
        support_geometry,
        support_pcurve,
        curve_parameter,
        target,
    )
}

// Keep the support carrier explicit so repeated support-UV samples avoid index lookups.
#[allow(clippy::too_many_arguments)]
pub(crate) fn blend_boundary_parameter_from_support_pcurve_with_geometry(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    ir: &CadIr,
    blend: &SurfaceId,
    support: &SurfaceId,
    support_geometry: &SurfaceGeometry,
    support_pcurve: &PcurveGeometry,
    curve_parameter: f64,
    target: BoundaryInverseTarget,
) -> Option<Point2> {
    let (supports, spine, radius, _) = blend_surface_definition_with_index(index, blend)?;
    let matches = supports
        .iter()
        .enumerate()
        .filter(|(_, candidate)| parameterization_equivalent_surfaces(ir, candidate, support))
        .map(|(boundary, _)| boundary)
        .collect::<Vec<_>>();
    let [boundary] = matches.as_slice() else {
        return None;
    };
    let contact_pcurve = spine_contact_pcurve(ir, support, &spine, radius, 0)?;
    blend_boundary_parameter_from_contact_pcurve_with_geometry(
        index,
        support,
        support_geometry,
        contact_pcurve,
        *boundary,
        support_pcurve,
        curve_parameter,
        target,
    )
}

// Keep the support carrier explicit so repeated transfer samples avoid index lookups.
#[allow(clippy::too_many_arguments)]
pub(crate) fn blend_boundary_parameter_from_contact_pcurve_with_geometry(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    support: &SurfaceId,
    support_geometry: &SurfaceGeometry,
    contact_pcurve: &PcurveGeometry,
    boundary: usize,
    support_pcurve: &PcurveGeometry,
    curve_parameter: f64,
    target: BoundaryInverseTarget,
) -> Option<Point2> {
    let support_uv = pcurve_uv(support_pcurve, curve_parameter)?;
    let parameter = target
        .seed
        .and_then(|seed| closest_pcurve_parameter_from_seed(contact_pcurve, support_uv, seed.u))
        .or_else(|| closest_pcurve_parameter_from_coarse_grid(contact_pcurve, support_uv))?;
    [parameter]
        .into_iter()
        .find(|parameter| {
            let Some(uv) = pcurve_uv(contact_pcurve, *parameter) else {
                return false;
            };
            decoded_surface_point_with_geometry(index, support, support_geometry, uv.u, uv.v, 0)
                .is_some_and(|candidate| {
                    point_distance(candidate, target.point) <= target.tolerance
                })
        })
        .map(|parameter| Point2::new(parameter, boundary as f64))
}

const LOCAL_PCURVE_SEARCH_STEPS: usize = 12;
const COARSE_PCURVE_SEARCH_INTERVALS: usize = 16;

fn pcurve_domain(pcurve: &PcurveGeometry) -> Option<([f64; 2], bool)> {
    let PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        periodic,
        ..
    } = pcurve
    else {
        return None;
    };
    let degree = usize::try_from(*degree).ok()?;
    let count = control_points.len();
    if count <= degree || knots.len() != count.checked_add(degree)?.checked_add(1)? {
        return None;
    }
    let domain = [*knots.get(degree)?, *knots.get(count)?];
    (domain[0].is_finite() && domain[1].is_finite() && domain[0] < domain[1])
        .then_some((domain, *periodic))
}

fn closest_pcurve_parameter_from_coarse_grid(
    pcurve: &PcurveGeometry,
    point: Point2,
) -> Option<f64> {
    let (domain, _) = pcurve_domain(pcurve)?;
    let mut closest = None;
    for index in 0..=COARSE_PCURVE_SEARCH_INTERVALS {
        let parameter = domain[0]
            + (domain[1] - domain[0]) * index as f64 / COARSE_PCURVE_SEARCH_INTERVALS as f64;
        let candidate = pcurve_uv(pcurve, parameter)?;
        let distance = (candidate.u - point.u).powi(2) + (candidate.v - point.v).powi(2);
        if !distance.is_finite() {
            continue;
        }
        if closest.is_none_or(|(_, current)| distance < current) {
            closest = Some((parameter, distance));
        }
    }
    closest.and_then(|(parameter, _)| closest_pcurve_parameter_from_seed(pcurve, point, parameter))
}

fn closest_pcurve_parameter_from_seed(
    pcurve: &PcurveGeometry,
    point: Point2,
    seed: f64,
) -> Option<f64> {
    let (domain, periodic) = pcurve_domain(pcurve)?;
    let mut parameter = if periodic {
        canonical_periodic_parameter(domain, true, seed)
    } else {
        seed.clamp(domain[0], domain[1])
    };
    for _ in 0..LOCAL_PCURVE_SEARCH_STEPS {
        let candidate = pcurve_uv(pcurve, parameter)?;
        let tangent = pcurve_tangent(pcurve, parameter)?;
        let speed_squared = tangent.u * tangent.u + tangent.v * tangent.v;
        if !speed_squared.is_finite() || speed_squared <= f64::EPSILON {
            return None;
        }
        let gradient = (candidate.u - point.u) * tangent.u + (candidate.v - point.v) * tangent.v;
        let step = gradient / speed_squared;
        if !step.is_finite() {
            return None;
        }
        let next = if periodic {
            canonical_periodic_parameter(domain, true, parameter - step)
        } else {
            (parameter - step).clamp(domain[0], domain[1])
        };
        if (next - parameter).abs() <= 64.0 * f64::EPSILON * (1.0 + parameter.abs()) {
            return Some(next);
        }
        parameter = next;
    }
    Some(parameter)
}

#[cfg(test)]
pub(crate) fn closest_pcurve_parameters(
    pcurve: &PcurveGeometry,
    point: Point2,
    seed: Option<f64>,
) -> Option<Vec<f64>> {
    let PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    } = pcurve
    else {
        return None;
    };
    let degree = usize::try_from(*degree).ok()?;
    let count = control_points.len();
    if count <= degree || knots.len() != count.checked_add(degree)?.checked_add(1)? {
        return None;
    }
    let domain = [*knots.get(degree)?, *knots.get(count)?];
    if !domain[0].is_finite() || !domain[1].is_finite() || domain[0] >= domain[1] {
        return None;
    }
    if seed.is_some_and(|seed| !seed.is_finite()) {
        return None;
    }
    let search_seed = seed.map(|seed| canonical_periodic_parameter(domain, *periodic, seed));
    let homogeneous =
        homogeneous_pcurve_spans(degree, knots, control_points, weights.as_deref(), point)?;
    let candidates = if degree != 1 || weights.is_some() {
        closest_parameter_candidates(
            stationary_rational_distance_candidates(&homogeneous, search_seed)?,
            search_seed,
        )?
    } else {
        let candidates = control_points
            .windows(2)
            .enumerate()
            .filter_map(|(index, segment)| {
                let start = segment[0];
                let end = segment[1];
                let direction = Point2::new(end.u - start.u, end.v - start.v);
                let squared_length = direction.u * direction.u + direction.v * direction.v;
                if !squared_length.is_finite() || squared_length == 0.0 {
                    return None;
                }
                let fraction = (((point.u - start.u) * direction.u
                    + (point.v - start.v) * direction.v)
                    / squared_length)
                    .clamp(0.0, 1.0);
                let span_start = *knots.get(index + 1)?;
                let span_end = *knots.get(index + 2)?;
                if !span_start.is_finite() || !span_end.is_finite() || span_start >= span_end {
                    return None;
                }
                let projected = Point2::new(
                    start.u + fraction * direction.u,
                    start.v + fraction * direction.v,
                );
                let squared_distance =
                    (projected.u - point.u).powi(2) + (projected.v - point.v).powi(2);
                Some((
                    span_start + fraction * (span_end - span_start),
                    squared_distance,
                ))
            })
            .collect::<Vec<_>>();
        closest_parameter_candidates(candidates, search_seed)?
    };
    Some(lift_periodic_parameters(
        candidates, domain, *periodic, seed,
    ))
}

pub(crate) struct HomogeneousCurveSpans<const DIMENSION: usize> {
    pub(crate) spans: Vec<BezierSpan<DIMENSION>>,
    pub(crate) coordinate_tolerance: f64,
}

#[cfg(test)]
fn homogeneous_pcurve_spans(
    degree: usize,
    knots: &[f64],
    control_points: &[Point2],
    weights: Option<&[f64]>,
    point: Point2,
) -> Option<HomogeneousCurveSpans<3>> {
    let count = control_points.len();
    if degree == 0
        || count <= degree
        || knots.len() != count.checked_add(degree)?.checked_add(1)?
        || knots.iter().any(|knot| !knot.is_finite())
        || !knots_nondecreasing(knots)
        || control_points
            .iter()
            .any(|control| !control.u.is_finite() || !control.v.is_finite())
        || !point.u.is_finite()
        || !point.v.is_finite()
    {
        return None;
    }
    let weights = match weights {
        Some(weights)
            if weights.len() == count
                && weights
                    .iter()
                    .all(|weight| weight.is_finite() && *weight > 0.0) =>
        {
            weights.to_vec()
        }
        Some(_) => return None,
        None => vec![1.0; count],
    };
    let coordinate_scale = control_points
        .iter()
        .flat_map(|control| [control.u, control.v])
        .chain([point.u, point.v])
        .fold(1.0_f64, |scale, value| scale.max(value.abs()));
    let controls = control_points
        .iter()
        .zip(weights)
        .map(|(control, weight)| {
            [
                weight * (control.u - point.u),
                weight * (control.v - point.v),
                weight,
            ]
        })
        .collect::<Vec<_>>();
    if controls.iter().flatten().any(|value| !value.is_finite()) {
        return None;
    }
    let spans = bezier_spans(degree, knots, controls)?;
    Some(HomogeneousCurveSpans {
        spans,
        coordinate_tolerance: 64.0 * f64::EPSILON * coordinate_scale,
    })
}

pub(crate) fn stationary_rational_distance_candidates<const DIMENSION: usize>(
    homogeneous: &HomogeneousCurveSpans<DIMENSION>,
    seed: Option<f64>,
) -> Option<Vec<(f64, f64)>> {
    let mut candidates = Vec::new();
    for span in &homogeneous.spans {
        let derivative = rational_squared_distance_derivative(&span.controls)?;
        let roots = scalar_bezier_roots(ScalarBezierSpan {
            domain: span.domain,
            controls: derivative,
        })?;
        let mut parameters = vec![span.domain[0], span.domain[1]];
        match roots {
            ScalarBezierRoots::Constant => parameters
                .extend(seed.filter(|seed| (span.domain[0]..=span.domain[1]).contains(seed))),
            ScalarBezierRoots::Isolated(roots) => parameters.extend(roots),
        }
        candidates.extend(parameters.into_iter().map(|parameter| {
            let distance = homogeneous_residual_distance(&span.controls, parameter, span.domain);
            (
                parameter,
                if distance <= homogeneous.coordinate_tolerance {
                    0.0
                } else {
                    distance * distance
                },
            )
        }));
    }
    Some(candidates)
}

pub(crate) fn rational_squared_distance_derivative<const DIMENSION: usize>(
    controls: &[[f64; DIMENSION]],
) -> Option<Vec<f64>> {
    // For residual R/W, half the squared-distance derivative has numerator
    // ((R·R')W - (R·R)W'). Positive weights make its roots exactly the finite
    // stationary parameters of the rational span.
    let weight = controls
        .iter()
        .map(|control| control[DIMENSION - 1])
        .collect::<Vec<_>>();
    let derivative = |values: &[f64]| {
        values
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .collect::<Vec<_>>()
    };
    let weight_derivative = derivative(&weight);
    let residuals = (0..DIMENSION - 1)
        .map(|axis| {
            controls
                .iter()
                .map(|control| control[axis])
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let residual_squared = sum_bernstein_polynomials(
        residuals
            .iter()
            .map(|residual| bernstein_product(residual, residual)),
    )?;
    let residual_derivative = sum_bernstein_polynomials(
        residuals
            .iter()
            .map(|residual| bernstein_product(residual, &derivative(residual))),
    )?;
    let first = bernstein_product(&residual_derivative, &weight)?;
    let second = bernstein_product(&residual_squared, &weight_derivative)?;
    subtract_bernstein_polynomials(first, second)
}

pub(crate) fn bernstein_product(first: &[f64], second: &[f64]) -> Option<Vec<f64>> {
    let first_degree = first.len().checked_sub(1)?;
    let second_degree = second.len().checked_sub(1)?;
    let degree = first_degree.checked_add(second_degree)?;
    (0..=degree)
        .map(|index| {
            let denominator = binomial_coefficient(degree, index)?;
            let lower = index.saturating_sub(second_degree);
            let upper = index.min(first_degree);
            (lower..=upper)
                .map(|first_index| {
                    let second_index = index - first_index;
                    Some(
                        first[first_index]
                            * second[second_index]
                            * binomial_coefficient(first_degree, first_index)?
                            * binomial_coefficient(second_degree, second_index)?
                            / denominator,
                    )
                })
                .sum::<Option<f64>>()
                .filter(|value| value.is_finite())
        })
        .collect()
}

pub(crate) fn binomial_coefficient(n: usize, k: usize) -> Option<f64> {
    let k = k.min(n.checked_sub(k)?);
    (1..=k).try_fold(1.0, |value, index| {
        let next = value * (n - k + index) as f64 / index as f64;
        next.is_finite().then_some(next)
    })
}

pub(crate) fn add_bernstein_polynomials(first: Vec<f64>, second: Vec<f64>) -> Option<Vec<f64>> {
    let result = (first.len() == second.len()).then(|| {
        first
            .into_iter()
            .zip(second)
            .map(|(a, b)| a + b)
            .collect::<Vec<_>>()
    })?;
    result
        .iter()
        .all(|value| value.is_finite())
        .then_some(result)
}

pub(crate) fn sum_bernstein_polynomials(
    polynomials: impl IntoIterator<Item = Option<Vec<f64>>>,
) -> Option<Vec<f64>> {
    polynomials.into_iter().try_fold(None, |sum, polynomial| {
        let polynomial = polynomial?;
        Some(Some(match sum {
            Some(sum) => add_bernstein_polynomials(sum, polynomial)?,
            None => polynomial,
        }))
    })?
}

pub(crate) fn subtract_bernstein_polynomials(
    first: Vec<f64>,
    second: Vec<f64>,
) -> Option<Vec<f64>> {
    let result = (first.len() == second.len()).then(|| {
        first
            .into_iter()
            .zip(second)
            .map(|(a, b)| a - b)
            .collect::<Vec<_>>()
    })?;
    result
        .iter()
        .all(|value| value.is_finite())
        .then_some(result)
}

pub(crate) enum ScalarBezierRoots {
    Constant,
    Isolated(Vec<f64>),
}

#[derive(Clone)]
pub(crate) struct ScalarBezierSpan {
    pub(crate) domain: [f64; 2],
    pub(crate) controls: Vec<f64>,
}

pub(crate) fn scalar_bezier_roots(span: ScalarBezierSpan) -> Option<ScalarBezierRoots> {
    const MAX_INTERVALS: usize = 100_000;

    let scale = span
        .controls
        .iter()
        .fold(1.0_f64, |scale, value| scale.max(value.abs()));
    let tolerance = 64.0 * f64::EPSILON * scale;
    let constant = span.controls.iter().all(|value| *value == 0.0);
    if constant {
        return Some(ScalarBezierRoots::Constant);
    }
    let mut parameters = Vec::new();
    if span
        .controls
        .first()
        .is_some_and(|value| value.abs() <= tolerance)
    {
        parameters.push(span.domain[0]);
    }
    if span
        .controls
        .last()
        .is_some_and(|value| value.abs() <= tolerance)
    {
        parameters.push(span.domain[1]);
    }
    let mut intervals = vec![span];
    let mut examined = 0usize;
    while let Some(span) = intervals.pop() {
        examined += 1;
        if examined > MAX_INTERVALS {
            return None;
        }
        if scalar_bernstein_sign_variations(&span.controls) == 0 {
            continue;
        }
        let middle = span.domain[0] + (span.domain[1] - span.domain[0]) * 0.5;
        if middle == span.domain[0] || middle == span.domain[1] {
            let parameter =
                [span.domain[0], span.domain[1]]
                    .into_iter()
                    .min_by(|first, second| {
                        scalar_bezier_value(&span.controls, *first, span.domain)
                            .abs()
                            .total_cmp(
                                &scalar_bezier_value(&span.controls, *second, span.domain).abs(),
                            )
                    })?;
            if scalar_bezier_value(&span.controls, parameter, span.domain).abs() <= tolerance {
                parameters.push(parameter);
            }
            continue;
        }
        let (first, second) = subdivide_scalar_bezier_span(span, middle);
        if first.controls.last().is_some_and(|value| *value == 0.0) {
            parameters.push(middle);
        }
        intervals.push(second);
        intervals.push(first);
    }
    parameters.sort_by(f64::total_cmp);
    parameters.dedup_by(|first, second| {
        (*first - *second).abs() <= 64.0 * f64::EPSILON * first.abs().max(second.abs()).max(1.0)
    });
    Some(ScalarBezierRoots::Isolated(parameters))
}

pub(crate) fn scalar_bernstein_sign_variations(controls: &[f64]) -> usize {
    // Bernstein-form Descartes variation bounds the roots in the open span.
    // Exact zero controls do not contribute a sign.
    controls
        .iter()
        .copied()
        .filter(|value| *value != 0.0)
        .map(f64::is_sign_positive)
        .fold((None, 0), |(previous, variations), positive| {
            (
                Some(positive),
                variations + usize::from(previous.is_some_and(|previous| previous != positive)),
            )
        })
        .1
}

pub(crate) fn subdivide_scalar_bezier_span(
    span: ScalarBezierSpan,
    middle: f64,
) -> (ScalarBezierSpan, ScalarBezierSpan) {
    let mut levels = vec![span.controls];
    while levels.last().is_some_and(|level| level.len() > 1) {
        let next = levels
            .last()
            .expect("nonempty Bézier subdivision level")
            .windows(2)
            .map(|pair| (pair[0] + pair[1]) * 0.5)
            .collect();
        levels.push(next);
    }
    let first = levels.iter().map(|level| level[0]).collect();
    let second = levels
        .iter()
        .rev()
        .map(|level| *level.last().expect("nonempty Bézier subdivision level"))
        .collect();
    (
        ScalarBezierSpan {
            domain: [span.domain[0], middle],
            controls: first,
        },
        ScalarBezierSpan {
            domain: [middle, span.domain[1]],
            controls: second,
        },
    )
}

pub(crate) fn scalar_bezier_value(controls: &[f64], parameter: f64, domain: [f64; 2]) -> f64 {
    let fraction = (parameter - domain[0]) / (domain[1] - domain[0]);
    let mut values = controls.to_vec();
    while values.len() > 1 {
        values = values
            .windows(2)
            .map(|pair| (1.0 - fraction) * pair[0] + fraction * pair[1])
            .collect();
    }
    values[0]
}

#[derive(Clone)]
pub(crate) struct BezierSpan<const DIMENSION: usize> {
    pub(crate) domain: [f64; 2],
    pub(crate) controls: Vec<[f64; DIMENSION]>,
}

pub(crate) fn bezier_spans<const DIMENSION: usize>(
    degree: usize,
    knots: &[f64],
    mut controls: Vec<[f64; DIMENSION]>,
) -> Option<Vec<BezierSpan<DIMENSION>>> {
    let mut knots = knots.to_vec();
    let domain = [*knots.get(degree)?, *knots.get(controls.len())?];
    let mut internal = knots[degree + 1..controls.len()]
        .iter()
        .copied()
        .filter(|knot| domain[0] < *knot && *knot < domain[1])
        .collect::<Vec<_>>();
    internal.sort_by(f64::total_cmp);
    internal.dedup();
    for knot in internal {
        while knots.iter().filter(|candidate| **candidate == knot).count() < degree {
            insert_homogeneous_curve_knot(degree, &mut knots, &mut controls, knot)?;
        }
    }
    let mut boundaries = knots[degree..=controls.len()].to_vec();
    boundaries.sort_by(f64::total_cmp);
    boundaries.dedup();
    let spans = boundaries
        .windows(2)
        .enumerate()
        .filter_map(|(index, domain)| {
            (domain[0] < domain[1]).then(|| {
                let start = index.checked_mul(degree)?;
                Some(BezierSpan {
                    domain: [domain[0], domain[1]],
                    controls: controls.get(start..=start + degree)?.to_vec(),
                })
            })?
        })
        .collect::<Vec<_>>();
    (!spans.is_empty()).then_some(spans)
}

pub(crate) fn insert_homogeneous_curve_knot<const DIMENSION: usize>(
    degree: usize,
    knots: &mut Vec<f64>,
    controls: &mut Vec<[f64; DIMENSION]>,
    knot: f64,
) -> Option<()> {
    let count = controls.len();
    let span = knots
        .windows(2)
        .position(|pair| pair[0] <= knot && knot < pair[1])?;
    let multiplicity = knots.iter().filter(|candidate| **candidate == knot).count();
    if multiplicity >= degree {
        return Some(());
    }
    let mut inserted = vec![[0.0; DIMENSION]; count + 1];
    inserted[..=span - degree].copy_from_slice(&controls[..=span - degree]);
    inserted[span - multiplicity + 1..].copy_from_slice(&controls[span - multiplicity..]);
    for index in span - degree + 1..=span - multiplicity {
        let denominator = knots[index + degree] - knots[index];
        if !denominator.is_finite() || denominator <= 0.0 {
            return None;
        }
        let alpha = (knot - knots[index]) / denominator;
        inserted[index] = std::array::from_fn(|axis| {
            alpha * controls[index][axis] + (1.0 - alpha) * controls[index - 1][axis]
        });
    }
    knots.insert(span + 1, knot);
    *controls = inserted;
    Some(())
}

pub(crate) fn homogeneous_residual_distance<const DIMENSION: usize>(
    controls: &[[f64; DIMENSION]],
    parameter: f64,
    domain: [f64; 2],
) -> f64 {
    let fraction = (parameter - domain[0]) / (domain[1] - domain[0]);
    let mut values = controls.to_vec();
    while values.len() > 1 {
        values = values
            .windows(2)
            .map(|pair| {
                std::array::from_fn(|axis| {
                    (1.0 - fraction) * pair[0][axis] + fraction * pair[1][axis]
                })
            })
            .collect();
    }
    values[0][..DIMENSION - 1]
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt()
        / values[0][DIMENSION - 1]
}

pub(crate) fn closest_parameter_candidates(
    candidates: impl IntoIterator<Item = (f64, f64)>,
    seed: Option<f64>,
) -> Option<Vec<f64>> {
    let candidates = candidates.into_iter().collect::<Vec<_>>();
    let minimum_distance = candidates
        .iter()
        .map(|candidate| candidate.1)
        .min_by(f64::total_cmp)?;
    let mut nearest = candidates
        .into_iter()
        .filter(|candidate| {
            let scale = candidate
                .1
                .abs()
                .max(minimum_distance.abs())
                .max(f64::MIN_POSITIVE);
            (candidate.1 - minimum_distance).abs() <= 128.0 * f64::EPSILON * scale
        })
        .map(|candidate| candidate.0)
        .collect::<Vec<_>>();
    nearest.sort_by(|first, second| {
        seed.map_or_else(
            || first.total_cmp(second),
            |seed| {
                (first - seed)
                    .abs()
                    .total_cmp(&(second - seed).abs())
                    .then_with(|| first.total_cmp(second))
            },
        )
    });
    nearest.dedup_by(|first, second| first.to_bits() == second.to_bits());
    (!nearest.is_empty()).then_some(nearest)
}

pub(crate) fn canonical_periodic_parameter(
    domain: [f64; 2],
    periodic: bool,
    parameter: f64,
) -> f64 {
    if !periodic {
        return parameter;
    }
    let period = domain[1] - domain[0];
    domain[0] + (parameter - domain[0]).rem_euclid(period)
}

pub(crate) fn lift_periodic_parameters(
    mut parameters: Vec<f64>,
    domain: [f64; 2],
    periodic: bool,
    seed: Option<f64>,
) -> Vec<f64> {
    let Some(seed) = seed.filter(|_| periodic) else {
        return parameters;
    };
    let period = domain[1] - domain[0];
    for parameter in &mut parameters {
        *parameter += ((seed - *parameter) / period).round() * period;
    }
    parameters.sort_by(|first, second| {
        (first - seed)
            .abs()
            .total_cmp(&(second - seed).abs())
            .then_with(|| first.total_cmp(second))
    });
    parameters.dedup_by(|first, second| first.to_bits() == second.to_bits());
    parameters
}

pub(crate) fn spine_contact_point_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    support: &SurfaceId,
    spine: &CurveId,
    parameter: f64,
    radius: f64,
    depth: usize,
) -> Option<Point3> {
    (depth < 32).then_some(())?;
    let ir = index.ir();
    let pcurve = spine_contact_pcurve(ir, support, spine, radius, depth + 1)?;
    let uv = pcurve_uv(pcurve, parameter)?;
    decoded_surface_point_inner(index, support, uv.u, uv.v, depth + 1)
}

pub(crate) fn spine_contact_pcurve<'a>(
    ir: &'a CadIr,
    support: &SurfaceId,
    spine: &CurveId,
    radius: f64,
    depth: usize,
) -> Option<&'a PcurveGeometry> {
    (depth < 32).then_some(())?;
    let procedural = ir.model.procedural_curves.iter().find(|candidate| {
        candidate.curve == *spine
            && matches!(
                candidate.definition,
                ProceduralCurveDefinition::Intersection { .. }
            )
    })?;
    let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition else {
        unreachable!("definition selected above");
    };
    let candidates = context.sides.iter().filter_map(|side| {
        let side_surface = side.surface.as_ref()?;
        let pcurve = side.pcurve.as_ref()?;
        let offset = constant_surface_offset_between(ir, support, side_surface, depth + 1)?;
        if !blend_contact_offset_matches(0.0, offset, radius) {
            return None;
        }
        Some(pcurve)
    });
    let candidates = candidates.collect::<Vec<_>>();
    let [pcurve] = candidates.as_slice() else {
        return None;
    };
    Some(*pcurve)
}

pub(crate) fn constant_surface_offset_between(
    ir: &CadIr,
    support: &SurfaceId,
    offset_surface: &SurfaceId,
    depth: usize,
) -> Option<f64> {
    let (support_base, support_offset) = surface_offset_lineage(ir, support, depth + 1)?;
    let (offset_base, offset_distance) = surface_offset_lineage(ir, offset_surface, depth + 1)?;
    if support_base == offset_base {
        return Some(offset_distance - support_offset);
    }
    let support_geometry = &ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == support_base)?
        .geometry;
    let offset_geometry = &ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == offset_base)?
        .geometry;
    let base_offset = analytic_surface_offset(support_geometry, offset_geometry)
        .or_else(|| blend_surface_offset(ir, &support_base, &offset_base, depth + 1))?;
    Some(base_offset + offset_distance - support_offset)
}

pub(crate) fn blend_surface_offset(
    ir: &CadIr,
    support: &SurfaceId,
    offset: &SurfaceId,
    depth: usize,
) -> Option<f64> {
    (depth < 32).then_some(())?;
    let (support_carriers, support_spine, support_radius, support_reversed) =
        blend_surface_definition(ir, support)?;
    let (offset_carriers, offset_spine, offset_radius, offset_reversed) =
        blend_surface_definition(ir, offset)?;
    (support_spine == offset_spine).then_some(())?;

    let distance = offset_radius - support_radius;
    let magnitude = distance.abs();
    let matches = [[0usize, 1usize], [1usize, 0usize]]
        .into_iter()
        .filter(|permutation| {
            permutation
                .iter()
                .enumerate()
                .all(|(support_index, &offset_index)| {
                    support_reversed[support_index] == offset_reversed[offset_index]
                        && constant_surface_offset_between(
                            ir,
                            &support_carriers[support_index],
                            &offset_carriers[offset_index],
                            depth + 1,
                        )
                        .is_some_and(|carrier_distance| {
                            blend_contact_offset_matches(0.0, carrier_distance, magnitude)
                        })
                })
        })
        .count();
    (matches == 1).then_some(distance)
}

pub(crate) fn analytic_surface_offset(
    support: &SurfaceGeometry,
    offset: &SurfaceGeometry,
) -> Option<f64> {
    match (support, offset) {
        (
            SurfaceGeometry::Plane {
                origin: support_origin,
                normal: support_normal,
                u_axis: support_u,
            },
            SurfaceGeometry::Plane {
                origin: offset_origin,
                normal: offset_normal,
                u_axis: offset_u,
            },
        ) if support_normal == offset_normal && support_u == offset_u => {
            let delta = Vector3::new(
                offset_origin.x - support_origin.x,
                offset_origin.y - support_origin.y,
                offset_origin.z - support_origin.z,
            );
            let distance = dot_vector(delta, *support_normal);
            let residual = Vector3::new(
                delta.x - distance * support_normal.x,
                delta.y - distance * support_normal.y,
                delta.z - distance * support_normal.z,
            );
            let scale = [
                support_origin.x,
                support_origin.y,
                support_origin.z,
                offset_origin.x,
                offset_origin.y,
                offset_origin.z,
                distance,
            ]
            .into_iter()
            .fold(1.0_f64, |scale, value| scale.max(value.abs()));
            let tolerance = 64.0 * f64::EPSILON * scale;
            (dot_vector(residual, residual) <= tolerance * tolerance).then_some(distance)
        }
        (
            SurfaceGeometry::Cylinder {
                origin: support_origin,
                axis: support_axis,
                ref_direction: support_ref,
                radius: support_radius,
            },
            SurfaceGeometry::Cylinder {
                origin: offset_origin,
                axis: offset_axis,
                ref_direction: offset_ref,
                radius: offset_radius,
            },
        ) if support_origin == offset_origin
            && support_axis == offset_axis
            && support_ref == offset_ref =>
        {
            Some(offset_radius - support_radius)
        }
        (
            SurfaceGeometry::Cone {
                origin: support_origin,
                axis: support_axis,
                ref_direction: support_ref,
                radius: support_radius,
                ratio: support_ratio,
                half_angle: support_angle,
            },
            SurfaceGeometry::Cone {
                origin: offset_origin,
                axis: offset_axis,
                ref_direction: offset_ref,
                radius: offset_radius,
                ratio: offset_ratio,
                half_angle: offset_angle,
            },
        ) if support_axis == offset_axis
            && support_ref == offset_ref
            && support_ratio.to_bits() == 1.0_f64.to_bits()
            && offset_ratio.to_bits() == 1.0_f64.to_bits()
            && support_angle.to_bits() == offset_angle.to_bits() =>
        {
            let delta = Vector3::new(
                offset_origin.x - support_origin.x,
                offset_origin.y - support_origin.y,
                offset_origin.z - support_origin.z,
            );
            let axial_delta = dot_vector(delta, *support_axis);
            let residual = Vector3::new(
                delta.x - axial_delta * support_axis.x,
                delta.y - axial_delta * support_axis.y,
                delta.z - axial_delta * support_axis.z,
            );
            let radial_delta = offset_radius - support_radius;
            let distance = radial_delta * support_angle.cos() - axial_delta * support_angle.sin();
            let tangent_residual =
                radial_delta * support_angle.sin() + axial_delta * support_angle.cos();
            let scale = [
                support_origin.x,
                support_origin.y,
                support_origin.z,
                offset_origin.x,
                offset_origin.y,
                offset_origin.z,
                *support_radius,
                *offset_radius,
                axial_delta,
                distance,
                tangent_residual,
            ]
            .into_iter()
            .fold(1.0_f64, |scale, value| scale.max(value.abs()));
            let tolerance = 64.0 * f64::EPSILON * scale;
            (distance.is_finite()
                && dot_vector(residual, residual) <= tolerance * tolerance
                && tangent_residual.abs() <= tolerance)
                .then_some(distance)
        }
        (
            SurfaceGeometry::Sphere {
                center: support_center,
                axis: support_axis,
                ref_direction: support_ref,
                radius: support_radius,
            },
            SurfaceGeometry::Sphere {
                center: offset_center,
                axis: offset_axis,
                ref_direction: offset_ref,
                radius: offset_radius,
            },
        ) if support_center == offset_center
            && support_axis == offset_axis
            && support_ref == offset_ref
            && support_radius.signum().to_bits() == offset_radius.signum().to_bits() =>
        {
            Some((offset_radius - support_radius) * support_radius.signum())
        }
        (
            SurfaceGeometry::Torus {
                center: support_center,
                axis: support_axis,
                ref_direction: support_ref,
                major_radius: support_major,
                minor_radius: support_minor,
            },
            SurfaceGeometry::Torus {
                center: offset_center,
                axis: offset_axis,
                ref_direction: offset_ref,
                major_radius: offset_major,
                minor_radius: offset_minor,
            },
        ) if support_center == offset_center
            && support_axis == offset_axis
            && support_ref == offset_ref
            && support_major.to_bits() == offset_major.to_bits()
            && support_minor.signum().to_bits() == offset_minor.signum().to_bits()
            && *support_major > support_minor.abs()
            && *offset_major > offset_minor.abs() =>
        {
            Some((offset_minor - support_minor) * support_minor.signum())
        }
        _ => None,
    }
}

pub(crate) fn blend_contact_offset_matches(
    support_offset: f64,
    spine_side_offset: f64,
    radius: f64,
) -> bool {
    let actual = (spine_side_offset - support_offset).abs();
    let expected = radius.abs();
    let scale = actual.max(expected).max(1.0);
    actual.is_finite()
        && expected.is_finite()
        && (actual - expected).abs() <= 64.0 * f64::EPSILON * scale
}

pub(crate) fn surface_offset_lineage(
    ir: &CadIr,
    surface: &SurfaceId,
    depth: usize,
) -> Option<(SurfaceId, f64)> {
    (depth < 32).then_some(())?;
    ir.model
        .surfaces
        .iter()
        .any(|candidate| &candidate.id == surface)
        .then_some(())?;
    let Some(procedural) = procedural_surface_for_carrier(ir, surface) else {
        return Some((surface.clone(), 0.0));
    };
    let ProceduralSurfaceDefinition::Offset {
        support, distance, ..
    } = &procedural.definition
    else {
        return Some((surface.clone(), 0.0));
    };
    let (base, accumulated) = surface_offset_lineage(ir, support, depth + 1)?;
    Some((base, accumulated + distance))
}

pub(crate) fn blend_surface_definition(
    ir: &CadIr,
    surface: &SurfaceId,
) -> Option<([SurfaceId; 2], CurveId, f64, [bool; 2])> {
    let procedural = procedural_surface_for_carrier(ir, surface)?;
    blend_surface_definition_from_procedural(procedural)
}

pub(crate) fn blend_surface_definition_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
) -> Option<([SurfaceId; 2], CurveId, f64, [bool; 2])> {
    let procedural = index.procedural_surface_for_surface(surface.0.as_str())?;
    blend_surface_definition_from_procedural(procedural)
}

fn blend_surface_definition_from_procedural(
    procedural: &ProceduralSurface,
) -> Option<([SurfaceId; 2], CurveId, f64, [bool; 2])> {
    let ProceduralSurfaceDefinition::Blend {
        supports: [Some(first), Some(second)],
        spine: Some(spine),
        radius: BlendRadiusLaw::Constant { signed_radius },
        cross_section: BlendCrossSection::Circular,
        ..
    } = &procedural.definition
    else {
        return None;
    };
    let radius = signed_radius.abs();
    (radius.is_finite() && radius > 0.0).then(|| {
        (
            [first.surface.clone(), second.surface.clone()],
            spine.clone(),
            radius,
            [first.reversed, second.reversed],
        )
    })
}

#[cfg(test)]
pub(crate) fn surface_contact_direction(
    ir: &CadIr,
    surface: &SurfaceId,
    center: Point3,
    radius: f64,
    depth: usize,
) -> Option<Vector3> {
    let index = cadmpeg_ir::index::ModelIndex::new(ir);
    surface_contact_direction_with_index(&index, surface, center, radius, depth)
}

pub(crate) fn surface_contact_direction_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    center: Point3,
    radius: f64,
    depth: usize,
) -> Option<Vector3> {
    (depth < 32).then_some(())?;
    let ir = index.ir();
    if let Some(direction) = blend_surface_contact_direction(index, surface, center, depth + 1) {
        return Some(direction);
    }
    let carrier = ir
        .model
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)?;
    let tolerance = ir.tolerances.linear;
    if !radius.is_finite() || radius <= 0.0 || !tolerance.is_finite() || tolerance <= 0.0 {
        return None;
    }
    let requires_radius_certificate = matches!(
        &carrier.geometry,
        SurfaceGeometry::Nurbs(_) | SurfaceGeometry::Procedural { .. }
    );
    let parameters = match &carrier.geometry {
        // The blend definition asks for a support contact at `radius` from the
        // spine center. A tolerance-bounded inverse can stop as soon as it has
        // constructed a point inside the enclosing radius band; the radial
        // check below then certifies the actual contact shell. A global
        // closest-point proof is neither required nor sufficient for this
        // incidence question.
        SurfaceGeometry::Nurbs(nurbs) => {
            nurbs_surface_parameter_within_tolerance(nurbs, center, None, radius + tolerance)
        }
        SurfaceGeometry::Procedural { .. } => offset_surface_parameters_with_tolerance_with_index(
            index,
            surface,
            center,
            None,
            Some(radius + tolerance),
        )
        .or_else(|| {
            blend_surface_parameters_inner(
                index,
                surface,
                center,
                None,
                None,
                BlendParameterGrid::Disabled,
                depth + 1,
            )
        }),
        geometry => analytic_surface_parameters(geometry, center),
    }?;
    let contact =
        decoded_surface_point_inner(index, surface, parameters.u, parameters.v, depth + 1)?;
    let offset = Vector3::new(
        contact.x - center.x,
        contact.y - center.y,
        contact.z - center.z,
    );
    (!requires_radius_certificate || (offset.norm() - radius).abs() <= tolerance)
        .then(|| unit_vector(offset))
        .flatten()
}

pub(crate) fn blend_surface_contact_direction(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    surface: &SurfaceId,
    point: Point3,
    depth: usize,
) -> Option<Vector3> {
    (depth < 32).then_some(())?;
    let ir = index.ir();
    let (_, spine, _, _) = blend_surface_definition(ir, surface)?;
    let u = closest_spine_parameter_with_index(index, &spine, point, None)?;
    let frame = blend_surface_frame_with_index(index, surface, u, depth + 1)?;
    let radial = unit_vector(Vector3::new(
        point.x - frame.0.x,
        point.y - frame.0.y,
        point.z - frame.0.z,
    ))?;
    let sweep = signed_angle(frame.2, frame.3, frame.1);
    if !sweep.is_finite() || sweep.abs() <= 1.0e-12 {
        return None;
    }
    let angle = signed_angle(frame.2, radial, frame.1);
    let candidate = (-2..=2)
        .map(|turn| (angle + f64::from(turn) * std::f64::consts::TAU) / sweep)
        .filter(|v| (0.0..=1.0).contains(v))
        .map(|v| blend_surface_point_from_frame(frame, v))
        .chain([
            blend_surface_point_from_frame(frame, 0.0),
            blend_surface_point_from_frame(frame, 1.0),
        ])
        .min_by(|first, second| {
            point_distance(*first, point).total_cmp(&point_distance(*second, point))
        })?;
    unit_vector(Vector3::new(
        candidate.x - point.x,
        candidate.y - point.y,
        candidate.z - point.z,
    ))
}

pub(crate) fn model_curve_point_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    curve: &CurveId,
    parameter: f64,
) -> Option<Point3> {
    let carrier = index.curves(curve.0.as_str())?;
    curve_point(&carrier.geometry, parameter)
}

pub(crate) fn model_curve_tangent_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    curve: &CurveId,
    parameter: f64,
) -> Option<Vector3> {
    let carrier = index.curves(curve.0.as_str())?;
    unit_vector(curve_tangent(&carrier.geometry, parameter)?)
}

#[cfg(test)]
pub(crate) fn closest_spine_parameter(
    ir: &CadIr,
    curve: &CurveId,
    point: Point3,
    seed: Option<f64>,
) -> Option<f64> {
    let index = cadmpeg_ir::index::ModelIndex::new(ir);
    closest_spine_parameter_with_index(&index, curve, point, seed)
}

pub(crate) fn closest_spine_parameter_with_index(
    index: &cadmpeg_ir::index::ModelIndex<'_>,
    curve: &CurveId,
    point: Point3,
    seed: Option<f64>,
) -> Option<f64> {
    let carrier = index.curves(curve.0.as_str())?;
    match &carrier.geometry {
        CurveGeometry::Line { origin, direction } => Some(
            (point.x - origin.x) * direction.x
                + (point.y - origin.y) * direction.y
                + (point.z - origin.z) * direction.z,
        ),
        CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => {
            closest_periodic_analytic_curve_parameter(&carrier.geometry, point, seed)
        }
        CurveGeometry::Nurbs(nurbs) => closest_nurbs_curve_parameter(nurbs, point, seed),
        _ => None,
    }
}

pub(crate) fn closest_periodic_analytic_curve_parameter(
    geometry: &CurveGeometry,
    point: Point3,
    seed: Option<f64>,
) -> Option<f64> {
    let (center, axis, reference) = match geometry {
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            ..
        } => (*center, *axis, *ref_direction),
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            ..
        } => (*center, *axis, *major_direction),
        _ => return None,
    };
    let transverse = cross_vector(axis, reference);
    let delta = Vector3::new(point.x - center.x, point.y - center.y, point.z - center.z);
    let phase = dot_vector(delta, transverse).atan2(dot_vector(delta, reference));
    phase.is_finite().then_some(())?;
    let circle_parameter = seed.map_or(phase, |seed| {
        phase + ((seed - phase) / std::f64::consts::TAU).round() * std::f64::consts::TAU
    });
    if matches!(geometry, CurveGeometry::Circle { .. }) {
        return Some(circle_parameter);
    }
    let anchor = seed.unwrap_or(phase);
    let CurveGeometry::Ellipse {
        major_radius,
        minor_radius,
        ..
    } = geometry
    else {
        unreachable!("periodic analytic curve is a circle or ellipse");
    };
    let x = dot_vector(delta, reference);
    let y = dot_vector(delta, transverse);
    let difference = minor_radius * minor_radius - major_radius * major_radius;
    let coefficients = [
        -*minor_radius * y,
        2.0 * (difference + major_radius * x),
        0.0,
        2.0 * (major_radius * x - difference),
        *minor_radius * y,
    ];
    let constant_distance = coefficients.iter().all(|coefficient| *coefficient == 0.0);
    let roots = real_polynomial_roots(&coefficients)?;
    let parameters = roots
        .into_iter()
        .map(|root| 2.0 * root.atan())
        .chain([0.0, std::f64::consts::PI])
        .chain(constant_distance.then_some(anchor))
        .map(|parameter| {
            parameter
                + ((anchor - parameter) / std::f64::consts::TAU).round() * std::f64::consts::TAU
        });
    let squared_distance = |parameter| {
        let position = curve_point(geometry, parameter)?;
        Some(
            (position.x - point.x).powi(2)
                + (position.y - point.y).powi(2)
                + (position.z - point.z).powi(2),
        )
    };
    closest_parameter_candidates(
        parameters
            .map(|parameter| Some((parameter, squared_distance(parameter)?)))
            .collect::<Option<Vec<_>>>()?,
        Some(anchor),
    )?
    .into_iter()
    .next()
}

pub(crate) fn real_polynomial_roots(coefficients: &[f64]) -> Option<Vec<f64>> {
    if coefficients
        .iter()
        .any(|coefficient| !coefficient.is_finite())
    {
        return None;
    }
    let mut roots = polynomial_roots_in_unit_interval(coefficients)?;
    let reversed = coefficients.iter().rev().copied().collect::<Vec<_>>();
    roots.extend(
        polynomial_roots_in_unit_interval(&reversed)?
            .into_iter()
            .filter(|root| *root != 0.0)
            .map(f64::recip),
    );
    roots.sort_by(f64::total_cmp);
    roots.dedup_by(|first, second| {
        (*first - *second).abs() <= 256.0 * f64::EPSILON * first.abs().max(second.abs()).max(1.0)
    });
    Some(roots)
}

pub(crate) fn polynomial_roots_in_unit_interval(coefficients: &[f64]) -> Option<Vec<f64>> {
    let mut coefficients = coefficients.to_vec();
    while coefficients
        .last()
        .is_some_and(|coefficient| *coefficient == 0.0)
    {
        coefficients.pop();
    }
    if coefficients.is_empty() {
        return Some(Vec::new());
    }
    let degree = coefficients.len().checked_sub(1)?;
    if degree == 0 {
        return Some(Vec::new());
    }
    let scale = coefficients
        .iter()
        .fold(0.0_f64, |scale, coefficient| scale.max(coefficient.abs()));
    if !scale.is_finite() || scale == 0.0 {
        return Some(Vec::new());
    }
    for coefficient in &mut coefficients {
        *coefficient /= scale;
    }
    if degree == 1 {
        let root = -coefficients[0] / coefficients[1];
        return root.is_finite().then(|| {
            if (-1.0..=1.0).contains(&root) {
                vec![root]
            } else {
                Vec::new()
            }
        });
    }
    let derivative = coefficients
        .iter()
        .enumerate()
        .skip(1)
        .map(|(degree, coefficient)| *coefficient * degree as f64)
        .collect::<Vec<_>>();
    let mut critical = polynomial_roots_in_unit_interval(&derivative)?;
    critical.sort_by(f64::total_cmp);
    critical.dedup_by(|first, second| {
        (*first - *second).abs() <= 64.0 * f64::EPSILON * first.abs().max(second.abs()).max(1.0)
    });
    let value = |parameter| polynomial_value(&coefficients, parameter);
    let tolerance = |parameter: f64| {
        256.0
            * f64::EPSILON
            * coefficients.iter().rev().fold(0.0, |bound, coefficient| {
                bound * parameter.abs() + coefficient.abs()
            })
    };
    let mut roots = critical
        .iter()
        .copied()
        .filter(|root| value(*root).abs() <= tolerance(*root))
        .collect::<Vec<_>>();
    let partitions = std::iter::once(-1.0)
        .chain(critical)
        .chain(std::iter::once(1.0))
        .collect::<Vec<_>>();
    for pair in partitions.windows(2) {
        let mut lower = pair[0];
        let mut upper = pair[1];
        let mut lower_value = value(lower);
        let upper_value = value(upper);
        if lower_value.abs() <= tolerance(lower) {
            roots.push(lower);
            continue;
        }
        if upper_value.abs() <= tolerance(upper) {
            roots.push(upper);
            continue;
        }
        if lower_value.is_sign_positive() == upper_value.is_sign_positive() {
            continue;
        }
        for _ in 0..128 {
            let middle = lower + (upper - lower) * 0.5;
            if middle == lower || middle == upper {
                break;
            }
            let middle_value = value(middle);
            if middle_value.abs() <= tolerance(middle) {
                lower = middle;
                upper = middle;
                break;
            }
            if middle_value.is_sign_positive() == lower_value.is_sign_positive() {
                lower = middle;
                lower_value = middle_value;
            } else {
                upper = middle;
            }
        }
        roots.push(lower + (upper - lower) * 0.5);
    }
    roots.sort_by(f64::total_cmp);
    roots.dedup_by(|first, second| {
        (*first - *second).abs() <= 256.0 * f64::EPSILON * first.abs().max(second.abs()).max(1.0)
    });
    Some(roots)
}

pub(crate) fn polynomial_value(coefficients: &[f64], parameter: f64) -> f64 {
    coefficients
        .iter()
        .rev()
        .fold(0.0, |value, coefficient| value * parameter + coefficient)
}

pub(crate) fn closest_nurbs_curve_parameter(
    curve: &NurbsCurve,
    point: Point3,
    seed: Option<f64>,
) -> Option<f64> {
    let degree = usize::try_from(curve.degree).ok()?;
    let count = curve.control_points.len();
    if degree == 0
        || count <= degree
        || curve.knots.len() != count.checked_add(degree)?.checked_add(1)?
        || curve.knots.iter().any(|knot| !knot.is_finite())
        || !knots_nondecreasing(&curve.knots)
        || curve.control_points.iter().any(|control| {
            !control.x.is_finite() || !control.y.is_finite() || !control.z.is_finite()
        })
        || !point.x.is_finite()
        || !point.y.is_finite()
        || !point.z.is_finite()
    {
        return None;
    }
    let domain = [*curve.knots.get(degree)?, *curve.knots.get(count)?];
    if !domain[0].is_finite() || !domain[1].is_finite() || domain[0] >= domain[1] {
        return None;
    }
    if seed.is_some_and(|seed| !seed.is_finite()) {
        return None;
    }
    let search_seed = seed.map(|seed| canonical_periodic_parameter(domain, curve.periodic, seed));
    let weights = match &curve.weights {
        Some(weights)
            if weights.len() == count
                && weights
                    .iter()
                    .all(|weight| weight.is_finite() && *weight > 0.0) =>
        {
            weights.clone()
        }
        Some(_) => return None,
        None => vec![1.0; count],
    };
    let coordinate_scale = curve
        .control_points
        .iter()
        .flat_map(|control| [control.x, control.y, control.z])
        .chain([point.x, point.y, point.z])
        .fold(1.0_f64, |scale, value| scale.max(value.abs()));
    let controls = curve
        .control_points
        .iter()
        .zip(weights)
        .map(|(control, weight)| {
            [
                weight * (control.x - point.x),
                weight * (control.y - point.y),
                weight * (control.z - point.z),
                weight,
            ]
        })
        .collect::<Vec<_>>();
    if controls.iter().flatten().any(|value| !value.is_finite()) {
        return None;
    }
    let homogeneous = HomogeneousCurveSpans {
        spans: bezier_spans(degree, &curve.knots, controls)?,
        coordinate_tolerance: 64.0 * f64::EPSILON * coordinate_scale,
    };
    let parameters = closest_parameter_candidates(
        stationary_rational_distance_candidates(&homogeneous, search_seed)?,
        search_seed,
    )?;
    lift_periodic_parameters(parameters, domain, curve.periodic, seed)
        .into_iter()
        .next()
}

pub(crate) fn signed_angle(first: Vector3, second: Vector3, axis: Vector3) -> f64 {
    dot_vector(cross_vector(first, second), axis).atan2(dot_vector(first, second))
}

pub(crate) fn rodrigues_rotate(vector: Vector3, axis: Vector3, angle: f64) -> Vector3 {
    let cross = cross_vector(axis, vector);
    let dot = dot_vector(axis, vector);
    Vector3::new(
        vector.x * angle.cos() + cross.x * angle.sin() + axis.x * dot * (1.0 - angle.cos()),
        vector.y * angle.cos() + cross.y * angle.sin() + axis.y * dot * (1.0 - angle.cos()),
        vector.z * angle.cos() + cross.z * angle.sin() + axis.z * dot * (1.0 - angle.cos()),
    )
}
