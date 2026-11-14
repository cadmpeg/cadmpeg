// SPDX-License-Identifier: Apache-2.0
//! Circular and elliptic branch admission from face-local spatial bounds.

use cadmpeg_ir::geometry::analytic::EllipseCurve;
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
    if !radius.is_finite()
        || radius <= 0.0
        || endpoints.into_iter().any(|point| {
            let delta = point.vector_from(center);
            !point.is_finite()
                || delta.dot(*axis.as_raw()).abs() > ANALYTIC_CURVE_ENDPOINT_TOLERANCE
                || (delta.norm() - radius).abs() > ANALYTIC_CURVE_ENDPOINT_TOLERANCE
        })
    {
        return None;
    }
    let frames = [axis, axis.reversed()].map(|axis| {
        (
            axis,
            cadmpeg_ir::geometry::derive_reference_direction(*axis.as_raw()),
        )
    });
    let (slot, range, BranchEvidence::Contained) =
        bounded_conic_branch(center, [radius; 2], frames, endpoints, bounds)?
    else {
        return None;
    };
    let (axis, reference) = frames[slot];
    Some((axis, reference, range))
}

/// Selects the arc of an ellipse between two endpoints by its sweep against
/// every source face box. The flag is true when the arc runs against the
/// ellipse's own parameterization.
pub(in crate::families::standard::decode) fn bounded_ellipse_branch(
    ellipse: &EllipseCurve,
    endpoints: [Point3; 2],
    bounds: &[Option<StandardFaceBounds>; 2],
) -> Option<(bool, [f64; 2], BranchEvidence)> {
    let center = ellipse.center().get();
    let frame = ellipse.frame();
    let radii = [ellipse.major_radius().get(), ellipse.minor_radius().get()];
    let axis = *frame.axis();
    let reference = *frame.reference().as_raw();
    let binormal = axis.as_raw().cross(reference);
    if endpoints.into_iter().any(|point| {
        let delta = point.vector_from(center);
        let first = delta.dot(reference) / radii[0];
        let second = delta.dot(binormal) / radii[1];
        let (sine, cosine) = second.atan2(first).sin_cos();
        let fitted = Vector3::new(
            reference.x * radii[0] * cosine + binormal.x * radii[1] * sine,
            reference.y * radii[0] * cosine + binormal.y * radii[1] * sine,
            reference.z * radii[0] * cosine + binormal.z * radii[1] * sine,
        );
        let residual =
            Vector3::new(delta.x - fitted.x, delta.y - fitted.y, delta.z - fitted.z).norm();
        !point.is_finite() || !residual.is_finite() || residual > ANALYTIC_CURVE_ENDPOINT_TOLERANCE
    }) {
        return None;
    }
    let (slot, range, evidence) = bounded_conic_branch(
        center,
        radii,
        [(axis, reference), (axis.reversed(), reference)],
        endpoints,
        bounds,
    )?;
    Some((slot == 1, range, evidence))
}

/// How a face-box branch selection was established.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::families::standard::decode) enum BranchEvidence {
    /// Exactly one arc fits every face box within the coordinate allowance.
    Contained,
    /// Neither arc fits; the selected arc's largest box excess, in
    /// millimetres, is at most [`LEAST_EXCESS_RATIO`] of the other arc's.
    LeastExcess(f64),
}

/// Largest ratio between the selected and the rejected arc's face-box excess
/// that still selects the smaller-excess arc.
const LEAST_EXCESS_RATIO: f64 = 0.01;

fn bounded_conic_branch(
    center: Point3,
    radii: [f64; 2],
    frames: [(UnitVector3, Vector3); 2],
    endpoints: [Point3; 2],
    bounds: &[Option<StandardFaceBounds>; 2],
) -> Option<(usize, [f64; 2], BranchEvidence)> {
    if !center.is_finite()
        || radii
            .iter()
            .any(|radius| !radius.is_finite() || *radius <= 0.0)
        || bounds.iter().all(Option::is_none)
    {
        return None;
    }
    let mut candidates = [None, None];
    for (slot, (axis, reference)) in frames.into_iter().enumerate() {
        let tangent = axis.as_raw().cross(reference);
        let angle = |point: Point3| {
            let delta = point.vector_from(center);
            (delta.dot(tangent) / radii[1]).atan2(delta.dot(reference) / radii[0])
        };
        let start = angle(endpoints[0]);
        let sweep = (angle(endpoints[1]) - start).rem_euclid(std::f64::consts::TAU);
        if !start.is_finite() || !sweep.is_finite() || sweep <= 0.0 {
            continue;
        }
        let Some(range) = crate::nurbs::canonical_periodic_range([start, start + sweep]) else {
            continue;
        };
        let center_lanes = [center.x, center.y, center.z];
        let reference_lanes = [reference.x, reference.y, reference.z].map(|lane| lane * radii[0]);
        let tangent_lanes = [tangent.x, tangent.y, tangent.z].map(|lane| lane * radii[1]);
        let mut excess = 0.0_f64;
        for bounds in bounds.iter().flatten() {
            for i in 0..3 {
                let point = |angle: f64| {
                    center_lanes[i]
                        + reference_lanes[i] * angle.cos()
                        + tangent_lanes[i] * angle.sin()
                };
                let ends = [point(range[0]), point(range[1])];
                let mut lower = ends[0].min(ends[1]);
                let mut upper = ends[0].max(ends[1]);
                let phase = tangent_lanes[i].atan2(reference_lanes[i]);
                for extremum in [phase, phase + std::f64::consts::PI] {
                    if (extremum - range[0]).rem_euclid(std::f64::consts::TAU)
                        <= range[1] - range[0]
                    {
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
                let tolerance = STANDARD_FACE_BOUNDS_TOLERANCE.max(
                    ARC_BOUND_SOURCE_ROUNDOFF_FACTOR * f64::from(f32::EPSILON) * roundoff_scale,
                );
                let box_lower = bounds.aabb_center[i].get() - bounds.aabb_half_extents[i].get();
                let box_upper = bounds.aabb_center[i].get() + bounds.aabb_half_extents[i].get();
                let lane_excess =
                    (box_lower - tolerance - lower).max(upper - box_upper - tolerance);
                excess = if lane_excess.is_finite() {
                    excess.max(lane_excess)
                } else {
                    f64::INFINITY
                };
            }
        }
        candidates[slot] = Some((range, excess));
    }
    let contained =
        |candidate: Option<([f64; 2], f64)>| candidate.is_some_and(|(_, excess)| excess <= 0.0);
    match candidates {
        [Some((range, _)), other] if contained(candidates[0]) && !contained(other) => {
            Some((0, range, BranchEvidence::Contained))
        }
        [other, Some((range, _))] if contained(candidates[1]) && !contained(other) => {
            Some((1, range, BranchEvidence::Contained))
        }
        [Some((first, first_excess)), Some((second, second_excess))]
            if first_excess > 0.0 && second_excess > 0.0 =>
        {
            if first_excess.is_finite() && first_excess <= LEAST_EXCESS_RATIO * second_excess {
                Some((0, first, BranchEvidence::LeastExcess(first_excess)))
            } else if second_excess.is_finite()
                && second_excess <= LEAST_EXCESS_RATIO * first_excess
            {
                Some((1, second, BranchEvidence::LeastExcess(second_excess)))
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
