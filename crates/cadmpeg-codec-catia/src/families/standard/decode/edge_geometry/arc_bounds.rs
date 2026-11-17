// SPDX-License-Identifier: Apache-2.0
//! Conic branch admission from face-local spatial bounds.

use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::UnitVector3;

use crate::families::standard::records::{StandardCurveSupport, StandardFaceBounds};

use super::{ANALYTIC_CURVE_ENDPOINT_TOLERANCE, STANDARD_FACE_BOUNDS_TOLERANCE};

const ARC_BOUND_SOURCE_ROUNDOFF_FACTOR: f64 = 3.0;

pub(in crate::families::standard::decode) fn support_bounds(
    face_bounds: &[Option<StandardFaceBounds>],
    support: &StandardCurveSupport,
) -> [Option<StandardFaceBounds>; 2] {
    support
        .faces
        .map(|face| face_bounds.get(face).copied().flatten())
}

pub(in crate::families::standard::decode) fn bounded_circle_branch(
    center: Point3,
    radius: f64,
    axis: UnitVector3,
    endpoints: [Point3; 2],
    bounds: &[Option<StandardFaceBounds>; 2],
) -> Option<(UnitVector3, Vector3, [f64; 2])> {
    if !center.is_finite()
        || !radius.is_finite()
        || radius <= 0.0
        || bounds.iter().all(Option::is_none)
    {
        return None;
    }
    if endpoints.into_iter().any(|point| {
        let delta = point.vector_from(center);
        !point.is_finite()
            || delta.dot(*axis.as_raw()).abs() > ANALYTIC_CURVE_ENDPOINT_TOLERANCE
            || (delta.norm() - radius).abs() > ANALYTIC_CURVE_ENDPOINT_TOLERANCE
    }) {
        return None;
    }
    let mut candidates = [None, None];
    for (slot, axis) in [axis, axis.reversed()].into_iter().enumerate() {
        let reference = cadmpeg_ir::geometry::derive_reference_direction(*axis.as_raw());
        let tangent = axis.as_raw().cross(reference);
        let angle = |point: Point3| {
            let delta = point.vector_from(center);
            delta.dot(tangent).atan2(delta.dot(reference))
        };
        let start = angle(endpoints[0]);
        let sweep = (angle(endpoints[1]) - start).rem_euclid(std::f64::consts::TAU);
        if !start.is_finite() || !sweep.is_finite() || sweep <= 0.0 {
            continue;
        }
        let Some(range) = crate::nurbs::canonical_periodic_range([start, start + sweep]) else {
            continue;
        };
        let fits = branch_fits_bounds(
            center,
            reference.scale(radius),
            tangent.scale(radius),
            range,
            bounds,
        );
        if fits {
            candidates[slot] = Some((axis, reference, range));
        }
    }
    match candidates {
        [Some(candidate), None] | [None, Some(candidate)] => Some(candidate),
        _ => None,
    }
}

fn branch_fits_bounds(
    center: Point3,
    cosine: Vector3,
    sine: Vector3,
    range: [f64; 2],
    bounds: &[Option<StandardFaceBounds>; 2],
) -> bool {
    let center_lanes = [center.x, center.y, center.z];
    let reference_lanes = [cosine.x, cosine.y, cosine.z];
    let tangent_lanes = [sine.x, sine.y, sine.z];
    bounds.iter().flatten().all(|bounds| {
        (0..3).all(|i| {
            let point = |angle: f64| {
                center_lanes[i]
                    + (reference_lanes[i] * angle.cos() + tangent_lanes[i] * angle.sin())
            };
            let ends = [point(range[0]), point(range[1])];
            let mut lower = ends[0].min(ends[1]);
            let mut upper = ends[0].max(ends[1]);
            let phase = tangent_lanes[i].atan2(reference_lanes[i]);
            for extremum in [phase, phase + std::f64::consts::PI] {
                if (extremum - range[0]).rem_euclid(std::f64::consts::TAU) <= range[1] - range[0] {
                    let value = point(extremum);
                    lower = lower.min(value);
                    upper = upper.max(value);
                }
            }
            let roundoff_scale = bounds.aabb_center[i]
                .get()
                .abs()
                .max(bounds.aabb_half_extents[i].get())
                .max(lower.abs())
                .max(upper.abs())
                .max(1.0);
            let tolerance = STANDARD_FACE_BOUNDS_TOLERANCE
                .max(ARC_BOUND_SOURCE_ROUNDOFF_FACTOR * f64::from(f32::EPSILON) * roundoff_scale);
            lower.is_finite()
                && upper.is_finite()
                && lower
                    >= bounds.aabb_center[i].get() - bounds.aabb_half_extents[i].get() - tolerance
                && upper
                    <= bounds.aabb_center[i].get() + bounds.aabb_half_extents[i].get() + tolerance
        })
    })
}

pub(in crate::families::standard::decode) fn bounded_ellipse_branch(
    ellipse: cadmpeg_ir::geometry::analytic::EllipseCurve,
    endpoints: [Point3; 2],
    bounds: &[Option<StandardFaceBounds>; 2],
) -> Option<(cadmpeg_ir::geometry::analytic::EllipseCurve, [f64; 2])> {
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    if bounds.iter().all(Option::is_none) || endpoints[0] == endpoints[1] {
        return None;
    }
    let mut candidates = [None, None];
    for (slot, reversed) in [false, true].into_iter().enumerate() {
        let mut candidate = ellipse;
        if reversed {
            candidate.reverse_parameterization();
        }
        let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(candidate));
        let start = super::standard_analytic_curve_angle(&geometry, endpoints[0])?;
        let end = super::standard_analytic_curve_angle(&geometry, endpoints[1])?;
        let sweep = (end - start).rem_euclid(std::f64::consts::TAU);
        if sweep <= 0.0 {
            continue;
        }
        let Some(range) = crate::nurbs::canonical_periodic_range([start, start + sweep]) else {
            continue;
        };
        let reference = *candidate.frame().reference().as_raw();
        let tangent = candidate.frame().axis().as_raw().cross(reference);
        if branch_fits_bounds(
            candidate.center().get(),
            reference.scale(candidate.major_radius().get()),
            tangent.scale(candidate.minor_radius().get()),
            range,
            bounds,
        ) {
            candidates[slot] = Some((candidate, range));
        }
    }
    match candidates {
        [Some(candidate), None] | [None, Some(candidate)] => Some(candidate),
        _ => None,
    }
}

#[cfg(test)]
mod tests;

pub(in crate::families::standard::decode) fn witnessed_cylinder_ellipse_branch(
    ellipse: cadmpeg_ir::geometry::analytic::EllipseCurve,
    cylinder: &cadmpeg_ir::geometry::analytic::CylinderSurface,
    endpoints: [Point3; 2],
    witness: Point3,
) -> Option<(cadmpeg_ir::geometry::analytic::EllipseCurve, [f64; 2])> {
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    let axis = *cylinder.frame().axis().as_raw();
    let normal = *ellipse.frame().axis().as_raw();
    let axis_dot_normal = axis.dot(normal);
    if axis_dot_normal.abs() <= super::CYLINDER_PLANE_CONIC_TOLERANCE {
        return None;
    }
    let origin = cylinder.origin().get();
    let offset = witness.vector_from(origin);
    let radial = offset - axis.scale(offset.dot(axis));
    let norm = radial.norm();
    if !norm.is_finite() || norm == 0.0 {
        return None;
    }
    let direction = radial.scale(cylinder.radius().get() / norm);
    let radial_point = Point3::new(
        origin.x + direction.x,
        origin.y + direction.y,
        origin.z + direction.z,
    );
    let axial = ellipse.center().get().vector_from(radial_point).dot(normal) / axis_dot_normal;
    let point = radial_point.translated(axis, axial);
    let mut geometry = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse));
    let range = super::standard_oriented_analytic_curve_parameter_range(
        &mut geometry,
        endpoints[0],
        endpoints[1],
        point,
    )?;
    let CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(selected)) = geometry else {
        return None;
    };
    Some((selected, range))
}
