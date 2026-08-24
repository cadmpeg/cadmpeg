// SPDX-License-Identifier: Apache-2.0
//! Multi-component intersection candidates and FC14 axis selection.

use cadmpeg_ir::geometry::CurveGeometry;

use super::super::analytic::{
    circle_parameters, cross, dot, intersect_plane_with_circle, nonperiodic_conic_parameter,
    periodic_conic_frame, CarrierEquation, PeriodicConicFrame, PlaneEquation,
};
use super::super::sketch::normalized;

use super::intersection_candidates::{
    apex_plane_cone_generator_candidates, axis_containing_plane_torus_circle_candidates,
    axis_normal_plane_torus_circle_candidates, coaxial_cone_cylinder_circle_candidates,
    coaxial_cone_sphere_circle_candidates, coaxial_cone_torus_circle_candidates,
    coaxial_cones_section_candidates, coaxial_cylinder_sphere_circle_candidates,
    coaxial_cylinder_torus_circle_candidates, coaxial_sphere_torus_circle_candidates,
    coaxial_tori_circle_candidates, parallel_cylinder_generator_candidates,
    parallel_plane_cylinder_generator_candidates,
};
use super::intersections::carrier_intersection_curve;

const EPS_ON_CURVE: f64 = 1e-7;
const EPS_AXIS_COMPONENT: f64 = 1e-10;
const EPS_CENTER_AGREEMENT: f64 = 1e-9;

pub(in super::super) fn multi_component_intersection_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let mut candidates = parallel_plane_cylinder_generator_candidates(first, second);
    candidates.extend(parallel_cylinder_generator_candidates(first, second));
    candidates.extend(coaxial_cylinder_sphere_circle_candidates(first, second));
    candidates.extend(coaxial_cone_cylinder_circle_candidates(first, second));
    candidates.extend(coaxial_cones_section_candidates(first, second));
    candidates.extend(apex_plane_cone_generator_candidates(first, second));
    candidates.extend(coaxial_cone_sphere_circle_candidates(first, second));
    candidates.extend(coaxial_cone_torus_circle_candidates(first, second));
    candidates.extend(coaxial_cylinder_torus_circle_candidates(first, second));
    candidates.extend(coaxial_sphere_torus_circle_candidates(first, second));
    candidates.extend(coaxial_tori_circle_candidates(first, second));
    candidates.extend(axis_normal_plane_torus_circle_candidates(first, second));
    candidates.extend(axis_containing_plane_torus_circle_candidates(first, second));
    candidates
}

pub(in super::super) fn carrier_intersection_components(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    carrier_intersection_curve(first, second)
        .into_iter()
        .chain(multi_component_intersection_candidates(first, second))
        .collect()
}

pub(in super::super) fn intersect_plane_with_carrier_components(
    plane: PlaneEquation,
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<[f64; 3]> {
    carrier_intersection_components(first, second)
        .into_iter()
        .filter_map(|(geometry, _)| circle_parameters(&geometry))
        .flat_map(|(center, axis, radius)| intersect_plane_with_circle(plane, center, axis, radius))
        .collect()
}

pub(in super::super) fn curve_contains_points(
    geometry: &CurveGeometry,
    points: [[f64; 3]; 2],
) -> bool {
    match geometry {
        CurveGeometry::Line { origin, direction } => {
            let origin = [origin.x, origin.y, origin.z];
            let Some(direction) = normalized([direction.x, direction.y, direction.z]) else {
                return false;
            };
            points.into_iter().all(|point| {
                let relative: [f64; 3] = std::array::from_fn(|index| point[index] - origin[index]);
                let residual = cross(relative, direction);
                let scale = dot(relative, relative).sqrt().max(1.0);
                dot(residual, residual).sqrt() <= EPS_ON_CURVE * scale
            })
        }
        CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => {
            let Some(PeriodicConicFrame {
                center,
                normal,
                x_axis,
                y_axis,
                radii,
            }) = periodic_conic_frame(geometry)
            else {
                return false;
            };
            points.into_iter().all(|point| {
                let relative: [f64; 3] = std::array::from_fn(|index| point[index] - center[index]);
                let scale = radii.into_iter().fold(1.0, f64::max);
                let x = dot(relative, x_axis) / radii[0];
                let y = dot(relative, y_axis) / radii[1];
                dot(relative, normal).abs() <= EPS_ON_CURVE * scale
                    && x.mul_add(x, y * y).is_finite()
                    && (x.mul_add(x, y * y) - 1.0).abs() <= EPS_ON_CURVE
            })
        }
        CurveGeometry::Parabola { .. } | CurveGeometry::Hyperbola { .. } => points
            .into_iter()
            .all(|point| nonperiodic_conic_parameter(geometry, point).is_some()),
        _ => false,
    }
}

pub(in super::super) fn select_unique_curve_candidate(
    candidates: Vec<(CurveGeometry, &'static str)>,
    points: [[f64; 3]; 2],
) -> Option<(CurveGeometry, &'static str)> {
    let candidates = candidates
        .into_iter()
        .filter(|(geometry, _)| curve_contains_points(geometry, points))
        .collect::<Vec<_>>();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

pub(in super::super) fn resolve_curve_candidates(
    candidates: Vec<(CurveGeometry, &'static str)>,
    points: Option<[[f64; 3]; 2]>,
) -> Option<(CurveGeometry, &'static str)> {
    if let Some(points) = points {
        return select_unique_curve_candidate(candidates, points);
    }
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}

pub(in super::super) fn fc14_held_coordinate(
    coordinates: &[crate::curve::FcCurveCoordinates],
    curve_id: u32,
) -> Option<f64> {
    let mut records = coordinates
        .iter()
        .filter(|record| record.curve_id == curve_id && record.subtype == 0x14);
    let record = records.next()?;
    records.next().is_none().then_some(())?;
    let tokens = record
        .tokens
        .iter()
        .filter(|token| token.raw.first() == Some(&0x2d))
        .collect::<Vec<_>>();
    (tokens.len() >= 4).then_some(())?;
    let first = tokens[0];
    (first.value_mm.is_finite()
        && tokens
            .iter()
            .all(|token| token.raw == first.raw && token.value_mm == first.value_mm))
    .then_some(first.value_mm)
}

pub(in super::super) fn select_fc14_axis_coordinate_candidate(
    candidates: Vec<(CurveGeometry, &'static str)>,
    held_coordinate: f64,
) -> Option<(CurveGeometry, &'static str)> {
    let matching = candidates
        .into_iter()
        .filter(|(geometry, tag)| {
            if *tag != "coaxial_cone_cylinder_secant_circle" {
                return false;
            }
            let CurveGeometry::Circle { center, axis, .. } = geometry else {
                return false;
            };
            let axis = [axis.x, axis.y, axis.z];
            let Some(axis_index) = axis.iter().enumerate().find_map(|(index, value)| {
                ((value.abs() - 1.0).abs() <= EPS_AXIS_COMPONENT).then_some(index)
            }) else {
                return false;
            };
            if axis
                .iter()
                .enumerate()
                .any(|(index, value)| index != axis_index && value.abs() > EPS_AXIS_COMPONENT)
            {
                return false;
            }
            let center = [center.x, center.y, center.z];
            let scale = center[axis_index].abs().max(held_coordinate.abs()).max(1.0);
            (center[axis_index] - held_coordinate).abs() <= EPS_CENTER_AGREEMENT * scale
        })
        .collect::<Vec<_>>();
    let [candidate] = matching.as_slice() else {
        return None;
    };
    Some(candidate.clone())
}
