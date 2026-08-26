// SPDX-License-Identifier: Apache-2.0
//! Carrier point tests, plane reconciliation, and placed planes.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;

use crate::container::ContainerScan;

use super::super::holes::plane_envelope_corners;
use super::super::sketch::normalized;
use super::super::surfaces::{
    fc05_cap_pair_model_frame, fc05_model_frame, intersect_plane_with_carrier_components,
};

use super::edges::nurbs_intrinsic_parameter_range;
use super::equations::{
    cross, dot, intersect_plane_with_two_quadrics, intersect_two_planes_with_quadric,
    intersect_two_planes_with_torus, solve_planes, CarrierEquation, PlaneEquation, SphereEquation,
};
use super::vertices::model_points_agree;

const EPS_ON_CARRIER: f64 = 1.0e-7;
const EPS_POINT_UNIQUE: f64 = 1.0e-7;
const EPS_AGREE: f64 = 1.0e-9;
const EPS_ORTHO: f64 = 1.0e-10;
const EPS_NEAR_ZERO: f64 = 1.0e-12;
const EPS_STORED_FRAME_NONZERO: f64 = 1.0e-6;
const EPS_STORED_FRAME_RELATIVE: f64 = 1.0e-9;
const EPS_FC05_TANGENT_AXIS: f64 = 1.0e-10;
const EPS_FC05_TANGENT_RESIDUAL: f64 = 1.0e-9;
const EPS_FC05_CAP_AXIS: f64 = 1.0e-9;

pub fn point_on_carrier(point: [f64; 3], carrier: CarrierEquation) -> bool {
    match carrier {
        CarrierEquation::Plane(plane) => {
            let residual = dot(plane.normal, point) - dot(plane.normal, plane.origin);
            residual.abs() <= EPS_ON_CARRIER
        }
        CarrierEquation::Cylinder(cylinder) => {
            let Some(axis) = normalized(cylinder.axis) else {
                return false;
            };
            let relative = std::array::from_fn(|index| point[index] - cylinder.origin[index]);
            let axial = dot(relative, axis);
            let radial = std::array::from_fn(|index| relative[index] - axial * axis[index]);
            (dot(radial, radial).sqrt() - cylinder.radius).abs()
                <= EPS_ON_CARRIER * cylinder.radius.max(1.0)
        }
        CarrierEquation::Cone(cone) => {
            let (Some(axis), Some(x_axis)) =
                (normalized(cone.axis), normalized(cone.ref_direction))
            else {
                return false;
            };
            if cone.ratio <= 0.0 || !cone.ratio.is_finite() || dot(axis, x_axis).abs() > EPS_ORTHO {
                return false;
            }
            let y_axis = cross(axis, x_axis);
            let relative = std::array::from_fn(|index| point[index] - cone.origin[index]);
            let axial = dot(relative, axis);
            let radius = cone.radius + axial * cone.half_angle.tan();
            let radial_x = dot(relative, x_axis);
            let radial_y = dot(relative, y_axis) / cone.ratio;
            (radial_x.hypot(radial_y) - radius.abs()).abs()
                <= EPS_ON_CARRIER * radius.abs().max(1.0)
        }
        CarrierEquation::Sphere(sphere) => {
            let relative = std::array::from_fn(|index| point[index] - sphere.center[index]);
            (dot(relative, relative).sqrt() - sphere.radius).abs()
                <= EPS_ON_CARRIER * sphere.radius.max(1.0)
        }
        CarrierEquation::Torus(torus) => {
            let Some(axis) = normalized(torus.axis) else {
                return false;
            };
            let relative = std::array::from_fn(|index| point[index] - torus.center[index]);
            let axial = dot(relative, axis);
            let radial = std::array::from_fn(|index| relative[index] - axial * axis[index]);
            let tube_distance = (dot(radial, radial).sqrt() - torus.major_radius).hypot(axial);
            (tube_distance - torus.minor_radius).abs()
                <= EPS_ON_CARRIER * torus.minor_radius.max(torus.major_radius).max(1.0)
        }
    }
}

pub fn tangent_sphere_point(first: SphereEquation, second: SphereEquation) -> Option<[f64; 3]> {
    let delta: [f64; 3] = std::array::from_fn(|index| second.center[index] - first.center[index]);
    let distance = dot(delta, delta).sqrt();
    if distance <= EPS_NEAR_ZERO || first.radius <= 0.0 || second.radius <= 0.0 {
        return None;
    }
    let external = first.radius + second.radius;
    let internal = (first.radius - second.radius).abs();
    let scale = external.max(distance).max(1.0);
    if (distance - external).abs() > EPS_AGREE * scale
        && (distance - internal).abs() > EPS_AGREE * scale
    {
        return None;
    }
    let axial = (distance * distance + first.radius * first.radius - second.radius * second.radius)
        / (2.0 * distance);
    Some(std::array::from_fn(|index| {
        first.center[index] + axial * delta[index] / distance
    }))
}

pub fn tangent_plane_sphere_point(
    plane: PlaneEquation,
    sphere: SphereEquation,
) -> Option<[f64; 3]> {
    let normal = normalized(plane.normal)?;
    let signed_distance = dot(
        normal,
        std::array::from_fn(|index| sphere.center[index] - plane.origin[index]),
    );
    let scale = sphere.radius.max(1.0);
    if sphere.radius <= 0.0 || (signed_distance.abs() - sphere.radius).abs() > EPS_AGREE * scale {
        return None;
    }
    Some(std::array::from_fn(|index| {
        sphere.center[index] - signed_distance * normal[index]
    }))
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CarrierSolveDiagnostics {
    pub pair_intersections: usize,
    pub triple_intersections: usize,
    pub valid_candidates: usize,
    pub unique_solutions: usize,
}

pub fn solve_carriers_with_diagnostics(
    carriers: &[CarrierEquation],
) -> (Option<[f64; 3]>, CarrierSolveDiagnostics) {
    let mut candidates = Vec::new();
    let mut diagnostics = CarrierSolveDiagnostics::default();
    for first in 0..carriers.len() {
        for second in first + 1..carriers.len() {
            let candidate_start = candidates.len();
            match (carriers[first], carriers[second]) {
                (CarrierEquation::Plane(plane), CarrierEquation::Sphere(sphere))
                | (CarrierEquation::Sphere(sphere), CarrierEquation::Plane(plane)) => {
                    if let Some(point) = tangent_plane_sphere_point(plane, sphere) {
                        candidates.push(point);
                    }
                }
                (CarrierEquation::Sphere(first), CarrierEquation::Sphere(second)) => {
                    if let Some(point) = tangent_sphere_point(first, second) {
                        candidates.push(point);
                    }
                }
                _ => {}
            }
            diagnostics.pair_intersections += candidates.len() - candidate_start;
        }
    }
    for first in 0..carriers.len() {
        for second in first + 1..carriers.len() {
            for third in second + 1..carriers.len() {
                let candidate_start = candidates.len();
                let triple = [carriers[first], carriers[second], carriers[third]];
                let mut planes = Vec::new();
                let mut cylinders = Vec::new();
                let mut cones = Vec::new();
                let mut spheres = Vec::new();
                let mut tori = Vec::new();
                for carrier in triple {
                    match carrier {
                        CarrierEquation::Plane(plane) => planes.push(plane),
                        CarrierEquation::Cylinder(cylinder) => cylinders.push(cylinder),
                        CarrierEquation::Cone(cone) => cones.push(cone),
                        CarrierEquation::Sphere(sphere) => spheres.push(sphere),
                        CarrierEquation::Torus(torus) => tori.push(torus),
                    }
                }
                if planes.len() == 3 {
                    if let Some(point) = solve_planes(&planes) {
                        candidates.push(point);
                    }
                } else if planes.len() == 1
                    && tori.is_empty()
                    && cylinders.len() + cones.len() + spheres.len() == 2
                {
                    let reduced = if let [first, second] = cones.as_slice() {
                        intersect_plane_with_carrier_components(
                            planes[0],
                            CarrierEquation::Cone(*first),
                            CarrierEquation::Cone(*second),
                        )
                    } else {
                        Vec::new()
                    };
                    if reduced.is_empty() {
                        let quadrics = cylinders
                            .iter()
                            .copied()
                            .map(CarrierEquation::Cylinder)
                            .chain(cones.iter().copied().map(CarrierEquation::Cone))
                            .chain(spheres.iter().copied().map(CarrierEquation::Sphere))
                            .collect::<Vec<_>>();
                        candidates.extend(intersect_plane_with_two_quadrics(
                            planes[0],
                            quadrics[0],
                            quadrics[1],
                        ));
                    } else {
                        candidates.extend(reduced);
                    }
                } else if planes.len() == 2
                    && tori.is_empty()
                    && cylinders.len() + cones.len() + spheres.len() == 1
                {
                    let quadric = cylinders
                        .first()
                        .copied()
                        .map(CarrierEquation::Cylinder)
                        .or_else(|| cones.first().copied().map(CarrierEquation::Cone))
                        .or_else(|| spheres.first().copied().map(CarrierEquation::Sphere))
                        .expect("one quadric carrier");
                    candidates.extend(intersect_two_planes_with_quadric(
                        planes[0], planes[1], quadric,
                    ));
                } else if let ([first, second], [torus]) = (planes.as_slice(), tori.as_slice()) {
                    if cylinders.is_empty() && cones.is_empty() && spheres.is_empty() {
                        candidates.extend(intersect_two_planes_with_torus(*first, *second, *torus));
                    }
                } else if let ([plane], [cylinder], [torus]) =
                    (planes.as_slice(), cylinders.as_slice(), tori.as_slice())
                {
                    if cones.is_empty() && spheres.is_empty() {
                        candidates.extend(intersect_plane_with_carrier_components(
                            *plane,
                            CarrierEquation::Cylinder(*cylinder),
                            CarrierEquation::Torus(*torus),
                        ));
                    }
                } else if let ([plane], [cone], [sphere]) =
                    (planes.as_slice(), cones.as_slice(), spheres.as_slice())
                {
                    if cylinders.is_empty() && tori.is_empty() {
                        candidates.extend(intersect_plane_with_carrier_components(
                            *plane,
                            CarrierEquation::Cone(*cone),
                            CarrierEquation::Sphere(*sphere),
                        ));
                    }
                } else if let ([plane], [cone], [torus]) =
                    (planes.as_slice(), cones.as_slice(), tori.as_slice())
                {
                    if cylinders.is_empty() && spheres.is_empty() {
                        candidates.extend(intersect_plane_with_carrier_components(
                            *plane,
                            CarrierEquation::Cone(*cone),
                            CarrierEquation::Torus(*torus),
                        ));
                    }
                } else if let ([plane], [sphere], [torus]) =
                    (planes.as_slice(), spheres.as_slice(), tori.as_slice())
                {
                    if cylinders.is_empty() && cones.is_empty() {
                        candidates.extend(intersect_plane_with_carrier_components(
                            *plane,
                            CarrierEquation::Sphere(*sphere),
                            CarrierEquation::Torus(*torus),
                        ));
                    }
                } else if let ([plane], [first, second]) = (planes.as_slice(), tori.as_slice()) {
                    if cylinders.is_empty() && cones.is_empty() && spheres.is_empty() {
                        candidates.extend(intersect_plane_with_carrier_components(
                            *plane,
                            CarrierEquation::Torus(*first),
                            CarrierEquation::Torus(*second),
                        ));
                    }
                }
                diagnostics.triple_intersections += candidates.len() - candidate_start;
            }
        }
    }
    candidates.retain(|point| {
        carriers
            .iter()
            .all(|carrier| point_on_carrier(*point, *carrier))
    });
    diagnostics.valid_candidates = candidates.len();
    let mut unique = Vec::<[f64; 3]>::new();
    for candidate in candidates {
        if !unique.iter().any(|known| {
            known
                .iter()
                .zip(candidate)
                .all(|(left, right)| (left - right).abs() <= EPS_POINT_UNIQUE)
        }) {
            unique.push(candidate);
        }
    }
    diagnostics.unique_solutions = unique.len();
    let point = match unique.as_slice() {
        [point] => Some(*point),
        _ => None,
    };
    (point, diagnostics)
}

#[cfg(test)]
pub fn solve_carriers(carriers: &[CarrierEquation]) -> Option<[f64; 3]> {
    solve_carriers_with_diagnostics(carriers).0
}

pub fn is_axis_aligned(vector: [f64; 3]) -> bool {
    vector
        .iter()
        .filter(|value| value.abs() > EPS_AGREE)
        .count()
        == 1
}

pub fn canonical_plane(plane: PlaneEquation) -> Option<PlaneEquation> {
    let mut normal = normalized(plane.normal)?;
    let mut distance = dot(normal, plane.origin);
    if !distance.is_finite() {
        return None;
    }
    let sign = normal
        .iter()
        .find(|coordinate| coordinate.abs() > EPS_NEAR_ZERO)?
        .signum();
    if sign < 0.0 {
        normal = normal.map(|coordinate| -coordinate);
        distance = -distance;
    }
    Some(PlaneEquation {
        origin: normal.map(|coordinate| coordinate * distance),
        normal,
    })
}

pub fn agreed_plane(candidates: &[PlaneEquation]) -> Option<PlaneEquation> {
    let planes = candidates
        .iter()
        .copied()
        .map(canonical_plane)
        .collect::<Option<Vec<_>>>()?;
    let first = *planes.first()?;
    let first_distance = dot(first.normal, first.origin);
    planes
        .iter()
        .all(|plane| {
            let distance = dot(plane.normal, plane.origin);
            let scale = first_distance.abs().max(distance.abs()).max(1.0);
            first
                .normal
                .iter()
                .zip(plane.normal)
                .all(|(left, right)| (left - right).abs() <= EPS_AGREE)
                && (first_distance - distance).abs() <= EPS_AGREE * scale
        })
        .then_some(first)
}

pub fn reconciled_model_plane(
    local_planes: &BTreeMap<u32, PlaneEquation>,
    ir: &CadIr,
    surface_id: u32,
) -> Option<PlaneEquation> {
    let model_id = SurfaceId(format!("creo:visibgeom:surface#{surface_id}"));
    let model_surfaces = ir
        .model
        .surfaces
        .iter()
        .filter(|surface| surface.id == model_id)
        .collect::<Vec<_>>();
    let model_plane = match model_surfaces.as_slice() {
        [] => None,
        [surface] => match &surface.geometry {
            SurfaceGeometry::Plane { origin, normal, .. } => Some(PlaneEquation {
                origin: [origin.x, origin.y, origin.z],
                normal: [normal.x, normal.y, normal.z],
            }),
            SurfaceGeometry::Unknown { .. } => None,
            _ => return None,
        },
        _ => return None,
    };
    match (local_planes.get(&surface_id).copied(), model_plane) {
        (Some(local), Some(model)) => agreed_plane(&[local, model]),
        (Some(local), None) => Some(local),
        (None, Some(model)) => Some(model),
        (None, None) => None,
    }
}

#[derive(Clone, Copy)]
pub struct PlaneCandidate {
    pub equation: PlaneEquation,
    pub chart: Option<PlaneChart>,
    pub offset: usize,
}

#[derive(Clone, Copy)]
pub struct PlaneChart {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
    pub u_axis: [f64; 3],
}

pub fn agreed_plane_surface(
    candidates: &[PlaneCandidate],
) -> Option<(PlaneEquation, [f64; 3], usize)> {
    agreed_plane(
        &candidates
            .iter()
            .map(|candidate| candidate.equation)
            .collect::<Vec<_>>(),
    )?;
    let charts = candidates
        .iter()
        .filter_map(|candidate| {
            let chart = candidate.chart?;
            let normal = normalized(chart.normal)?;
            let u_axis = normalized(chart.u_axis)?;
            (dot(normal, u_axis).abs() <= EPS_AGREE).then_some((
                chart.origin,
                normal,
                u_axis,
                candidate.offset,
            ))
        })
        .collect::<Vec<_>>();
    let representative = charts.iter().min_by_key(|(_, _, _, offset)| *offset)?;
    charts
        .iter()
        .all(|(origin, normal, u_axis, _)| {
            representative.0.iter().zip(origin).all(|(left, right)| {
                (left - right).abs() <= EPS_AGREE * left.abs().max(right.abs()).max(1.0)
            }) && representative
                .1
                .iter()
                .zip(normal)
                .all(|(left, right)| (left - right).abs() <= EPS_AGREE)
                && representative
                    .2
                    .iter()
                    .zip(u_axis)
                    .all(|(left, right)| (left - right).abs() <= EPS_AGREE)
        })
        .then_some((
            PlaneEquation {
                origin: representative.0,
                normal: representative.1,
            },
            representative.2,
            representative.3,
        ))
}

fn stored_parameter_normal_candidate(
    frame: &crate::surface::PlaneLocalSystem,
    mirror_z: bool,
    mirror_origin_z: bool,
) -> Option<PlaneCandidate> {
    let slots: [f64; 12] = frame
        .slots
        .iter()
        .copied()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    if slots[3..6].iter().any(|value| *value != 0.0) {
        return None;
    }
    let mut origin: [f64; 3] = slots[9..12].try_into().ok()?;
    let mut u_axis: [f64; 3] = slots[0..3].try_into().ok()?;
    let mut normal: [f64; 3] = slots[6..9].try_into().ok()?;
    if mirror_z {
        u_axis[2] = -u_axis[2];
        normal[2] = -normal[2];
    }
    if mirror_origin_z {
        origin[2] = -origin[2];
    }
    let u_magnitude = dot(u_axis, u_axis).sqrt();
    let normal_magnitude = dot(normal, normal).sqrt();
    let scale = u_magnitude.max(normal_magnitude).max(1.0);
    if !u_magnitude.is_finite()
        || !normal_magnitude.is_finite()
        || u_magnitude <= EPS_STORED_FRAME_NONZERO
        || normal_magnitude <= EPS_STORED_FRAME_NONZERO
        || (u_magnitude - normal_magnitude).abs() > EPS_STORED_FRAME_RELATIVE * scale
        || dot(u_axis, normal).abs() > EPS_STORED_FRAME_RELATIVE * u_magnitude * normal_magnitude
    {
        return None;
    }
    u_axis = u_axis.map(|value| value / u_magnitude);
    normal = normal.map(|value| value / normal_magnitude);
    Some(PlaneCandidate {
        equation: PlaneEquation { origin, normal },
        chart: Some(PlaneChart {
            origin,
            normal,
            u_axis,
        }),
        offset: frame.offset,
    })
}

fn stored_parameter_origin_sign_candidates(base: PlaneCandidate) -> Vec<PlaneCandidate> {
    let nonzero_axes = base
        .equation
        .origin
        .into_iter()
        .enumerate()
        .filter_map(|(axis, value)| {
            (value.abs() > EPS_STORED_FRAME_NONZERO
                && base.equation.normal[axis].abs() > EPS_STORED_FRAME_NONZERO)
                .then_some(axis)
        })
        .collect::<Vec<_>>();
    if nonzero_axes.is_empty() {
        return vec![base];
    }
    let mut candidates = Vec::with_capacity(1usize << nonzero_axes.len());
    for mask in 0..(1usize << nonzero_axes.len()) {
        let mut candidate = base;
        for (bit, axis) in nonzero_axes.iter().copied().enumerate() {
            if mask & (1usize << bit) == 0 {
                continue;
            }
            candidate.equation.origin[axis] = -candidate.equation.origin[axis];
            if let Some(chart) = &mut candidate.chart {
                chart.origin[axis] = -chart.origin[axis];
            }
        }
        if candidate
            .equation
            .origin
            .iter()
            .all(|value| value.is_finite())
        {
            candidates.push(candidate);
        }
    }
    candidates
}

fn stored_parameter_normal_candidates_with_origin_branches(
    frame: &crate::surface::PlaneLocalSystem,
    include_origin_z_branches: bool,
) -> Option<Vec<PlaneCandidate>> {
    if frame.classification == crate::surface::LocalSystemClassification::Simple {
        return None;
    }
    let slots: [f64; 12] = frame
        .slots
        .iter()
        .copied()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    if slots[3..6].iter().any(|value| *value != 0.0) {
        return None;
    }
    let mut candidates = Vec::new();
    let origin_branches: &[bool] = if include_origin_z_branches {
        &[false, true]
    } else {
        &[false]
    };
    for mirror_z in [false, true] {
        for mirror_origin_z in origin_branches {
            let candidate = stored_parameter_normal_candidate(frame, mirror_z, *mirror_origin_z)?;
            if !candidates
                .iter()
                .any(|known| plane_candidates_equivalent(*known, candidate))
            {
                candidates.push(candidate);
            }
        }
    }
    (candidates.len() > 1).then_some(candidates)
}

pub(crate) fn stored_parameter_normal_candidates(
    frame: &crate::surface::PlaneLocalSystem,
) -> Option<Vec<PlaneCandidate>> {
    stored_parameter_normal_candidates_with_origin_branches(frame, false)
}

fn coordinate_vectors_agree(first: [f64; 3], second: [f64; 3]) -> bool {
    first.into_iter().zip(second).all(|(first, second)| {
        (first - second).abs() <= EPS_AGREE * first.abs().max(second.abs()).max(1.0)
    })
}

fn plane_candidates_equivalent(first: PlaneCandidate, second: PlaneCandidate) -> bool {
    agreed_plane(&[first.equation, second.equation]).is_some()
        && match (first.chart, second.chart) {
            (Some(first), Some(second)) => {
                coordinate_vectors_agree(first.origin, second.origin)
                    && coordinate_vectors_agree(first.normal, second.normal)
                    && coordinate_vectors_agree(first.u_axis, second.u_axis)
            }
            (None, None) => true,
            _ => false,
        }
}

fn plane_chart_point(candidate: PlaneCandidate, uv: [f64; 2]) -> Option<[f64; 3]> {
    let chart = candidate.chart?;
    let normal = normalized(chart.normal)?;
    let u_axis = normalized(chart.u_axis)?;
    (dot(normal, u_axis).abs() <= EPS_ORTHO).then_some(())?;
    let v_axis = cross(normal, u_axis);
    let point = std::array::from_fn(|axis| {
        chart.origin[axis] + uv[0] * u_axis[axis] + uv[1] * v_axis[axis]
    });
    point.iter().all(|value| value.is_finite()).then_some(point)
}

fn pcurve_candidate_endpoint_witness(
    candidate: PlaneCandidate,
    adjacent: PlaneCandidate,
    endpoints: [[f64; 2]; 2],
) -> bool {
    if candidate.chart.is_none() {
        return false;
    }
    let Some(adjacent_normal) = normalized(adjacent.equation.normal) else {
        return false;
    };
    let Some(candidate_normal) = normalized(candidate.equation.normal) else {
        return false;
    };
    let cross_normals = cross(candidate_normal, adjacent_normal);
    if dot(cross_normals, cross_normals) <= EPS_ORTHO * EPS_ORTHO {
        return false;
    }
    let Some(points) = endpoints
        .map(|uv| plane_chart_point(candidate, uv))
        .into_iter()
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    if model_points_agree(points[0], points[1]) {
        return false;
    }
    points
        .into_iter()
        .all(|point| point_on_carrier(point, CarrierEquation::Plane(adjacent.equation)))
}

fn pcurve_candidates_agree(
    first: PlaneCandidate,
    second: PlaneCandidate,
    endpoint_sets: [[[f64; 2]; 2]; 2],
) -> bool {
    pcurve_candidate_endpoint_witness(first, second, endpoint_sets[0])
        || pcurve_candidate_endpoint_witness(second, first, endpoint_sets[1])
}

#[derive(Clone, Copy)]
struct PlaneBranchConstraint {
    faces: [u32; 2],
    endpoint_sets: [[[f64; 2]; 2]; 2],
}

fn stored_frame_branch_constraints(
    scan: &ContainerScan,
    domains: &BTreeMap<u32, Vec<PlaneCandidate>>,
) -> Vec<PlaneBranchConstraint> {
    let mut constraints = Vec::new();
    let mut add = |faces: [u32; 2], endpoint_sets: [[[f64; 2]; 2]; 2]| {
        if faces[0] == faces[1] {
            return;
        }
        let (Some(first), Some(second)) = (domains.get(&faces[0]), domains.get(&faces[1])) else {
            return;
        };
        let compatible = first
            .iter()
            .filter(|first| {
                second
                    .iter()
                    .any(|second| pcurve_candidates_agree(**first, *second, endpoint_sets))
            })
            .count();
        if compatible != 0 {
            constraints.push(PlaneBranchConstraint {
                faces,
                endpoint_sets,
            });
        }
    };
    for pcurve in &scan.curves.pcurves {
        add(
            pcurve.faces,
            super::pcurves::canonicalized_pcurve_endpoints(
                scan,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            ),
        );
    }
    for pcurve in &scan.curves.bound_prototype_pcurves {
        add(
            pcurve.faces,
            super::pcurves::canonicalized_pcurve_endpoints(
                scan,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            ),
        );
    }
    for pcurve in &scan.curves.two_chart_pcurves {
        let (Some(first), Some(last)) = (pcurve.samples.first(), pcurve.samples.last()) else {
            continue;
        };
        add(
            pcurve.faces,
            super::pcurves::canonicalized_pcurve_endpoints(
                scan,
                pcurve.faces,
                [first[0], last[0]],
                [first[1], last[1]],
            ),
        );
    }
    constraints
}

fn fc05_cylinder_branch_witnesses(
    scan: &ContainerScan,
) -> BTreeMap<u32, Vec<super::equations::CylinderEquation>> {
    let mut cylinder_frames = scan
        .curves
        .fc05_cylinder_cap_pairs
        .iter()
        .filter_map(|pair| {
            let frame = fc05_cap_pair_model_frame(scan, pair)?;
            let legacy = super::equations::CylinderEquation {
                origin: frame.origin,
                axis: frame.axis,
                ref_direction: frame.ref_direction,
                radius: pair.radius_mm,
            };
            Some((
                pair.surface_id,
                fc05_cylinder_model_witness(scan, pair.surface_id, legacy),
            ))
        })
        .collect::<BTreeMap<_, _>>();

    for circle in &scan.curves.fc05_circles {
        let topologies = scan
            .curves
            .topology_rows
            .iter()
            .find(|row| row.id == circle.curve_id)
            .into_iter()
            .collect::<Vec<_>>();
        let [topology] = topologies.as_slice() else {
            continue;
        };
        let planes = topology
            .faces
            .into_iter()
            .filter(|face| {
                crate::surface::unique_surface_row(&scan.surfaces.rows, *face)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Plane)
            })
            .filter_map(|face| {
                crate::surface::unique_outline_plane(&scan.planes.outlines, face)
                    .map(|plane| (face, plane))
            })
            .collect::<Vec<_>>();
        let cylinders = topology
            .faces
            .into_iter()
            .filter(|face| {
                crate::surface::unique_surface_row(&scan.surfaces.rows, *face)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
            })
            .collect::<Vec<_>>();
        let ([(_, cap)], [cylinder_id]) = (planes.as_slice(), cylinders.as_slice()) else {
            continue;
        };
        let Some(axis_index) =
            (0..3).find(|axis| cap.normal[*axis].abs() > 1.0 - EPS_FC05_CAP_AXIS)
        else {
            continue;
        };
        let (reference, axis_sign) = circle
            .reference_direction_row_frame
            .zip(circle.parameter_sign)
            .map_or(
                (
                    circle.sample_direction_row_frame,
                    cap.normal[axis_index].signum(),
                ),
                |(reference, parameter_sign)| (reference, -f64::from(parameter_sign)),
            );
        let (origin, axis, ref_direction) = fc05_model_frame(
            axis_index,
            cap.origin[axis_index],
            circle.center_row_frame,
            reference,
            axis_sign,
        );
        if cylinder_frames.contains_key(cylinder_id) {
            continue;
        }
        let legacy = super::equations::CylinderEquation {
            origin,
            axis,
            ref_direction,
            radius: circle.radius_mm,
        };
        let witness = fc05_cylinder_model_witness(scan, *cylinder_id, legacy);
        cylinder_frames.insert(*cylinder_id, witness);
    }

    let mut witnesses = BTreeMap::<u32, Vec<super::equations::CylinderEquation>>::new();
    for topology in &scan.curves.topology_rows {
        let Some((cylinder_id, plane_id)) = topology.faces.into_iter().find_map(|first| {
            let second = topology
                .faces
                .into_iter()
                .find(|candidate| *candidate != first)?;
            if cylinder_frames.contains_key(&first)
                && crate::surface::unique_surface_row(&scan.surfaces.rows, second)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Plane)
            {
                Some((first, second))
            } else if cylinder_frames.contains_key(&second)
                && crate::surface::unique_surface_row(&scan.surfaces.rows, first)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Plane)
            {
                Some((second, first))
            } else {
                None
            }
        }) else {
            continue;
        };
        let Some(cylinder) = cylinder_frames.get(&cylinder_id).copied() else {
            continue;
        };
        let entries = witnesses.entry(plane_id).or_default();
        if !entries.iter().any(|known| {
            known.origin == cylinder.origin
                && known.axis == cylinder.axis
                && known.radius.to_bits() == cylinder.radius.to_bits()
        }) {
            entries.push(cylinder);
        }
    }
    witnesses
}

/// Select an FC05 cylinder frame only when reference geometry improves the
/// independent stored-plane tangency score. A validated cap pair remains the
/// primary frame source; this witness does not turn an ID match into geometry.
pub(crate) fn fc05_cylinder_model_witness(
    scan: &ContainerScan,
    cylinder_id: u32,
    legacy: super::equations::CylinderEquation,
) -> super::equations::CylinderEquation {
    let curve_ids = scan
        .curves
        .fc05_circles
        .iter()
        .filter(|circle| {
            scan.curves.topology_rows.iter().any(|topology| {
                topology.id == circle.curve_id && topology.faces.contains(&cylinder_id)
            })
        })
        .map(|circle| circle.curve_id)
        .collect::<BTreeSet<_>>();
    let circles = curve_ids
        .iter()
        .flat_map(|curve_id| {
            scan.references
                .circles
                .iter()
                .filter(move |circle| circle.entity_id == *curve_id)
        })
        .collect::<Vec<_>>();
    let Some(frame) = fc05_reference_circle_frame(&circles) else {
        return legacy;
    };
    if (frame.radius - legacy.radius).abs() > EPS_FC05_TANGENT_RESIDUAL
        || dot(frame.axis, legacy.axis).abs() < 1.0 - EPS_FC05_TANGENT_AXIS
    {
        return legacy;
    }
    let legacy_score = fc05_tangent_plane_score(scan, cylinder_id, legacy);
    let mut reference_origin = frame.origin;
    if let Some(axis_index) = (0..3).find(|axis| legacy.axis[*axis].abs() > 1.0 - EPS_FC05_CAP_AXIS)
    {
        reference_origin[axis_index] = legacy.origin[axis_index];
    }
    let reference = super::equations::CylinderEquation {
        origin: reference_origin,
        axis: legacy.axis,
        ref_direction: legacy.ref_direction,
        radius: legacy.radius,
    };
    if fc05_tangent_plane_score(scan, cylinder_id, reference) > legacy_score {
        reference
    } else {
        legacy
    }
}

fn fc05_reference_circle_frame(
    circles: &[&crate::reference::ReferenceCircle],
) -> Option<crate::surface::PositionalCylinderFrame> {
    if let Some(frame) = super::super::surfaces::reference_circle_pair_cylinder_frame(circles) {
        return Some(frame);
    }
    let [circle] = circles else {
        return None;
    };
    if !circle.center_stored || !circle.radius.is_finite() || circle.radius <= 0.0 {
        return None;
    }
    let axis = normalized(circle.axis)?;
    let radial = std::array::from_fn(|index| circle.start[index] - circle.center[index]);
    let end_radial = std::array::from_fn(|index| circle.end[index] - circle.center[index]);
    let radial_length = dot(radial, radial).sqrt();
    let end_radial_length = dot(end_radial, end_radial).sqrt();
    let scale = circle
        .center
        .into_iter()
        .chain(circle.start)
        .chain(circle.end)
        .map(f64::abs)
        .fold(circle.radius.max(1.0), f64::max);
    if !radial_length.is_finite()
        || !end_radial_length.is_finite()
        || (radial_length - circle.radius).abs() > EPS_FC05_TANGENT_RESIDUAL * scale
        || (end_radial_length - circle.radius).abs() > EPS_FC05_TANGENT_RESIDUAL * scale
        || dot(axis, radial).abs() > EPS_FC05_TANGENT_RESIDUAL * scale
        || dot(axis, end_radial).abs() > EPS_FC05_TANGENT_RESIDUAL * scale
    {
        return None;
    }
    Some(crate::surface::PositionalCylinderFrame {
        origin: circle.center,
        axis,
        ref_direction: radial.map(|value| value / radial_length),
        radius: circle.radius,
        length: None,
    })
    .filter(crate::surface::PositionalCylinderFrame::is_valid)
}

fn fc05_tangent_plane_score(
    scan: &ContainerScan,
    cylinder_id: u32,
    cylinder: super::equations::CylinderEquation,
) -> usize {
    scan.curves
        .topology_rows
        .iter()
        .filter(|topology| topology.faces.contains(&cylinder_id))
        .flat_map(|topology| topology.faces.into_iter())
        .filter(|face_id| *face_id != cylinder_id)
        .filter(|face_id| {
            crate::surface::unique_surface_row(&scan.surfaces.rows, *face_id)
                .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Plane)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|plane_id| {
            scan.planes
                .local_systems
                .iter()
                .filter(|frame| frame.surface_id == *plane_id)
                .filter_map(stored_parameter_normal_candidates)
                .flatten()
                .any(|candidate| plane_candidate_is_fc05_tangent(candidate, cylinder))
        })
        .count()
}

fn plane_candidate_is_fc05_tangent(
    candidate: PlaneCandidate,
    cylinder: super::equations::CylinderEquation,
) -> bool {
    let Some(normal) = normalized(candidate.equation.normal) else {
        return false;
    };
    let Some(axis) = normalized(cylinder.axis) else {
        return false;
    };
    if dot(normal, axis).abs() > EPS_FC05_TANGENT_AXIS {
        return false;
    }
    let relative =
        std::array::from_fn(|index| cylinder.origin[index] - candidate.equation.origin[index]);
    let signed_distance = dot(normal, relative);
    (signed_distance.abs() - cylinder.radius).abs()
        <= EPS_FC05_TANGENT_RESIDUAL * cylinder.radius.max(1.0)
}

pub(crate) fn plane_candidate_pcurve_lies_on_carrier(
    candidate: PlaneCandidate,
    endpoints: [[f64; 2]; 2],
    carrier: CarrierEquation,
) -> bool {
    let Some(points) = endpoints
        .map(|uv| plane_chart_point(candidate, uv))
        .into_iter()
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    !model_points_agree(points[0], points[1])
        && points
            .into_iter()
            .all(|point| point_on_carrier(point, carrier))
}

fn native_positional_cylinder_carriers(scan: &ContainerScan) -> BTreeMap<u32, CarrierEquation> {
    crate::surface::uniquely_identified_rows(&scan.surfaces.rows)
        .into_iter()
        .filter(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
        .filter_map(|row| {
            let frame =
                crate::surface::unique_surface_parameter(&scan.surfaces.parameters, row.id)?
                    .positional_cylinder_frame?;
            Some((
                row.id,
                CarrierEquation::Cylinder(super::equations::CylinderEquation {
                    origin: frame.origin,
                    axis: frame.axis,
                    ref_direction: frame.ref_direction,
                    radius: frame.radius,
                }),
            ))
        })
        .collect()
}

fn select_stored_frame_carrier_pcurve_branches(
    scan: &ContainerScan,
    variable_domains: &BTreeMap<u32, Vec<PlaneCandidate>>,
    domains: &mut BTreeMap<u32, Vec<PlaneCandidate>>,
) {
    let carriers = native_positional_cylinder_carriers(scan);
    let mut apply = |faces: [u32; 2], endpoint_sets: [[[f64; 2]; 2]; 2]| {
        for face_index in 0..2 {
            let plane_id = faces[face_index];
            let Some(options) = variable_domains.get(&plane_id) else {
                continue;
            };
            let Some(carrier) = carriers.get(&faces[1 - face_index]).copied() else {
                continue;
            };
            let retained = options
                .iter()
                .copied()
                .filter(|candidate| {
                    plane_candidate_pcurve_lies_on_carrier(
                        *candidate,
                        endpoint_sets[face_index],
                        carrier,
                    )
                })
                .collect::<Vec<_>>();
            if retained.len() == 1 {
                domains.insert(plane_id, retained);
            }
        }
    };
    for pcurve in &scan.curves.pcurves {
        apply(
            pcurve.faces,
            super::pcurves::canonicalized_pcurve_endpoints(
                scan,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            ),
        );
    }
    for pcurve in &scan.curves.bound_prototype_pcurves {
        apply(
            pcurve.faces,
            super::pcurves::canonicalized_pcurve_endpoints(
                scan,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            ),
        );
    }
    for pcurve in &scan.curves.two_chart_pcurves {
        let (Some(first), Some(last)) = (pcurve.samples.first(), pcurve.samples.last()) else {
            continue;
        };
        apply(
            pcurve.faces,
            super::pcurves::canonicalized_pcurve_endpoints(
                scan,
                pcurve.faces,
                [first[0], last[0]],
                [first[1], last[1]],
            ),
        );
    }
}

fn select_stored_frame_branches(
    scan: &ContainerScan,
    candidates: &mut BTreeMap<u32, Vec<PlaneCandidate>>,
) {
    let cylinder_witnesses = fc05_cylinder_branch_witnesses(scan);
    let mut variable_domains = BTreeMap::<u32, Vec<PlaneCandidate>>::new();
    let mut origin_domains = BTreeMap::<u32, Vec<PlaneCandidate>>::new();
    for frame in &scan.planes.local_systems {
        let Some(options) = stored_parameter_normal_candidates(frame) else {
            continue;
        };
        if cylinder_witnesses.contains_key(&frame.surface_id) {
            if let Some(origin_options) =
                stored_parameter_normal_candidates_with_origin_branches(frame, true)
            {
                let known = origin_domains.entry(frame.surface_id).or_default();
                for option in origin_options {
                    if !known
                        .iter()
                        .any(|candidate| plane_candidates_equivalent(*candidate, option))
                    {
                        known.push(option);
                    }
                }
            }
        }
        let known = variable_domains.entry(frame.surface_id).or_default();
        for option in options {
            if !known
                .iter()
                .any(|candidate| plane_candidates_equivalent(*candidate, option))
            {
                known.push(option);
            }
        }
    }
    for (surface_id, options) in origin_domains {
        let Some(witnesses) = cylinder_witnesses.get(&surface_id) else {
            continue;
        };
        let retained = options
            .into_iter()
            .filter(|candidate| {
                witnesses
                    .iter()
                    .copied()
                    .any(|cylinder| plane_candidate_is_fc05_tangent(*candidate, cylinder))
            })
            .collect::<Vec<_>>();
        if retained.len() == 1 {
            variable_domains.insert(surface_id, retained);
        }
    }
    if variable_domains.is_empty() {
        return;
    }

    let mut domains = variable_domains.clone();
    select_stored_frame_carrier_pcurve_branches(scan, &variable_domains, &mut domains);
    for (surface_id, options) in &variable_domains {
        let Some(witnesses) = cylinder_witnesses.get(surface_id) else {
            continue;
        };
        let retained = options
            .iter()
            .copied()
            .filter(|candidate| {
                witnesses
                    .iter()
                    .copied()
                    .any(|cylinder| plane_candidate_is_fc05_tangent(*candidate, cylinder))
            })
            .collect::<Vec<_>>();
        if retained.len() == 1 {
            domains.insert(*surface_id, retained);
        }
    }
    for (surface_id, known) in candidates.iter() {
        let fixed = if known.len() == 1 {
            known
                .first()
                .copied()
                .filter(|candidate| candidate.chart.is_some())
        } else {
            agreed_plane_surface(known).map(|(equation, u_axis, offset)| PlaneCandidate {
                equation,
                chart: Some(PlaneChart {
                    origin: equation.origin,
                    normal: equation.normal,
                    u_axis,
                }),
                offset,
            })
        };
        if let Some(fixed) = fixed {
            domains.entry(*surface_id).or_insert_with(|| vec![fixed]);
        }
    }
    let constraints = stored_frame_branch_constraints(scan, &domains);

    let variable_ids = variable_domains.keys().copied().collect::<BTreeSet<_>>();
    let mut filtered = domains;
    loop {
        let mut changed = false;
        for constraint in &constraints {
            let Some(first) = filtered.get(&constraint.faces[0]).cloned() else {
                continue;
            };
            let Some(second) = filtered.get(&constraint.faces[1]).cloned() else {
                continue;
            };
            if variable_ids.contains(&constraint.faces[0]) {
                let retained = first
                    .into_iter()
                    .filter(|first| {
                        second.iter().any(|second| {
                            pcurve_candidates_agree(*first, *second, constraint.endpoint_sets)
                        })
                    })
                    .collect::<Vec<_>>();
                if retained.is_empty() {
                    continue;
                }
                changed |= retained.len() != filtered[&constraint.faces[0]].len();
                filtered.insert(constraint.faces[0], retained);
            }
            if variable_ids.contains(&constraint.faces[1]) {
                let Some(first) = filtered.get(&constraint.faces[0]).cloned() else {
                    continue;
                };
                let retained = second
                    .into_iter()
                    .filter(|second| {
                        first.iter().any(|first| {
                            pcurve_candidates_agree(*first, *second, constraint.endpoint_sets)
                        })
                    })
                    .collect::<Vec<_>>();
                if retained.is_empty() {
                    continue;
                }
                changed |= retained.len() != filtered[&constraint.faces[1]].len();
                filtered.insert(constraint.faces[1], retained);
            }
        }
        if !changed {
            break;
        }
    }
    for surface_id in variable_ids {
        let Some([candidate]) = filtered.get(&surface_id).map(Vec::as_slice) else {
            continue;
        };
        candidates.insert(surface_id, vec![*candidate]);
    }
}

fn round_edge_endpoint_plane_score(
    candidate: PlaneCandidate,
    envelopes: &[crate::surface::Type24RoundEdgeEnvelope],
) -> usize {
    envelopes
        .iter()
        .filter(|envelope| {
            envelope.vertices.into_iter().any(|point| {
                let scale = point
                    .into_iter()
                    .chain(candidate.equation.origin)
                    .map(f64::abs)
                    .fold(1.0, f64::max);
                (dot(candidate.equation.normal, point)
                    - dot(candidate.equation.normal, candidate.equation.origin))
                .abs()
                    <= EPS_ON_CARRIER * scale
            })
        })
        .count()
}

pub(crate) fn unique_round_edge_origin_candidate(
    candidates: &[PlaneCandidate],
    envelopes: &[crate::surface::Type24RoundEdgeEnvelope],
) -> Option<PlaneCandidate> {
    let scores = candidates
        .iter()
        .copied()
        .map(|candidate| {
            (
                candidate,
                round_edge_endpoint_plane_score(candidate, envelopes),
            )
        })
        .collect::<Vec<_>>();
    let maximum = scores.iter().map(|(_, score)| *score).max()?;
    (maximum > 0).then_some(())?;
    let mut best = scores
        .into_iter()
        .filter_map(|(candidate, score)| (score == maximum).then_some(candidate));
    let candidate = best.next()?;
    best.next().is_none().then_some(candidate)
}

fn round_edge_envelopes_for_plane(
    scan: &ContainerScan,
    plane_id: u32,
) -> Vec<crate::surface::Type24RoundEdgeEnvelope> {
    let rows = crate::surface::uniquely_identified_rows(&scan.surfaces.rows)
        .into_iter()
        .map(|row| (row.id, row))
        .collect::<BTreeMap<_, _>>();
    crate::topology::uniquely_identified_rows(&scan.curves.topology_rows)
        .into_iter()
        .filter_map(|topology| {
            let cylinder_id = topology.faces.into_iter().find(|face_id| {
                *face_id != plane_id
                    && rows.get(face_id).is_some_and(|row| {
                        row.kind == crate::surface::SurfaceKind::Cylinder
                            && row.type_byte == 0x24
                            && crate::decode::sketch_transfer::feature_schema_class(
                                scan,
                                row.feature_id,
                            ) == Some(913)
                    })
            })?;
            if !topology.faces.contains(&plane_id) {
                return None;
            }
            let record =
                crate::surface::unique_surface_parameter(&scan.surfaces.parameters, cylinder_id)?;
            record.type24_round_edge_envelope(0x24)
        })
        .collect()
}

fn select_round_edge_origin_branches(
    scan: &ContainerScan,
    candidates: &mut BTreeMap<u32, Vec<PlaneCandidate>>,
) {
    for frame in &scan.planes.local_systems {
        if frame.classification != crate::surface::LocalSystemClassification::Simple {
            continue;
        }
        let Some(existing) = candidates.get(&frame.surface_id) else {
            continue;
        };
        let (Some(origin), Some(normal), Some(u_axis)) = (frame.origin, frame.normal, frame.u_axis)
        else {
            continue;
        };
        let base = PlaneCandidate {
            equation: PlaneEquation { origin, normal },
            chart: Some(PlaneChart {
                origin,
                normal,
                u_axis,
            }),
            offset: frame.offset,
        };
        if existing.len() != 1 || !plane_candidates_equivalent(existing[0], base) {
            continue;
        }
        let envelopes = round_edge_envelopes_for_plane(scan, frame.surface_id);
        let options = stored_parameter_origin_sign_candidates(base);
        let Some(selected) = unique_round_edge_origin_candidate(&options, &envelopes) else {
            continue;
        };
        candidates.insert(frame.surface_id, vec![selected]);
    }
}

pub fn plane_candidates(scan: &ContainerScan) -> BTreeMap<u32, Vec<PlaneCandidate>> {
    let matrix_frame_ids = scan
        .planes
        .local_systems
        .iter()
        .filter(|frame| crate::surface::uses_matrix_column_frame(frame))
        .map(|frame| frame.surface_id)
        .collect::<BTreeSet<_>>();
    let held_planes = scan
        .planes
        .envelopes
        .iter()
        .filter_map(|envelope| Some((envelope.surface_id, held_coordinate_plane(envelope)?)))
        .fold(
            BTreeMap::<u32, Vec<PlaneEquation>>::new(),
            |mut planes, (surface_id, plane)| {
                planes.entry(surface_id).or_default().push(plane);
                planes
            },
        )
        .into_iter()
        .filter_map(|(surface_id, planes)| agreed_plane(&planes).map(|plane| (surface_id, plane)))
        .collect::<BTreeMap<_, _>>();
    let frame_bound_outlines = crate::surface::frame_bound_outline_planes(
        &scan.planes.envelopes,
        &scan.planes.local_systems,
    )
    .into_iter()
    .fold(
        BTreeMap::<u32, Vec<crate::surface::OutlinePlane>>::new(),
        |mut outlines, outline| {
            outlines
                .entry(outline.surface_id)
                .or_default()
                .push(outline);
            outlines
        },
    );
    let mut candidates = BTreeMap::<u32, Vec<PlaneCandidate>>::new();
    for frame in &scan.planes.local_systems {
        let (Some(origin), Some(normal)) = (frame.origin, frame.normal) else {
            continue;
        };
        let Some(u_axis) = frame.u_axis else {
            continue;
        };
        let frame_candidate = PlaneCandidate {
            equation: PlaneEquation { origin, normal },
            chart: Some(PlaneChart {
                origin,
                normal,
                u_axis,
            }),
            offset: frame.offset,
        };
        let candidate = frame_bound_outlines
            .get(&frame.surface_id)
            .and_then(|outlines| {
                let [outline] = outlines.as_slice() else {
                    return None;
                };
                frame_bound_outline_plane_candidate(frame, outline)
            })
            .or_else(|| {
                held_planes
                    .get(&frame.surface_id)
                    .filter(|held| agreed_plane(&[frame_candidate.equation, **held]).is_none())
                    .and_then(|held| envelope_reconciled_plane_candidate(frame, *held))
            })
            .unwrap_or(frame_candidate);
        candidates
            .entry(frame.surface_id)
            .or_default()
            .push(candidate);
    }
    let local_chart_ids = scan
        .planes
        .local_systems
        .iter()
        .filter(|frame| frame.origin.is_some() && frame.normal.is_some() && frame.u_axis.is_some())
        .map(|frame| frame.surface_id)
        .collect::<BTreeSet<_>>();
    for outline in &scan.planes.outlines {
        if matrix_frame_ids.contains(&outline.surface_id) {
            continue;
        }
        candidates
            .entry(outline.surface_id)
            .or_default()
            .push(PlaneCandidate {
                equation: PlaneEquation {
                    origin: outline.origin,
                    normal: outline.normal,
                },
                chart: (!local_chart_ids.contains(&outline.surface_id)).then_some(PlaneChart {
                    origin: outline.origin,
                    normal: outline.normal,
                    u_axis: outline.u_axis,
                }),
                offset: outline.offset,
            });
    }
    for envelope in &scan.planes.envelopes {
        if matrix_frame_ids.contains(&envelope.surface_id) {
            continue;
        }
        let Some(equation) = held_coordinate_plane(envelope) else {
            continue;
        };
        candidates
            .entry(envelope.surface_id)
            .or_default()
            .push(PlaneCandidate {
                equation,
                chart: None,
                offset: envelope.offset,
            });
    }
    for plane in &scan.planes.positional_frames {
        if candidates.contains_key(&plane.surface_id) {
            continue;
        }
        candidates.insert(
            plane.surface_id,
            vec![PlaneCandidate {
                equation: PlaneEquation {
                    origin: plane.origin,
                    normal: plane.normal,
                },
                chart: Some(PlaneChart {
                    origin: plane.origin,
                    normal: plane.normal,
                    u_axis: plane.u_axis,
                }),
                offset: plane.offset,
            }],
        );
    }
    select_stored_frame_branches(scan, &mut candidates);
    select_round_edge_origin_branches(scan, &mut candidates);
    candidates
        .into_iter()
        .filter(|(id, _)| {
            scan.surfaces
                .rows
                .iter()
                .filter(|row| row.id == *id)
                .take(2)
                .count()
                < 2
        })
        .collect()
}

pub fn frame_bound_outline_plane_candidate(
    frame: &crate::surface::PlaneLocalSystem,
    outline: &crate::surface::OutlinePlane,
) -> Option<PlaneCandidate> {
    (frame.surface_id == outline.surface_id).then_some(())?;
    let frame_normal = normalized(frame.normal?)?;
    let frame_u_axis = normalized(frame.u_axis?)?;
    let outline_normal = normalized(outline.normal)?;
    let outline_u_axis = normalized(outline.u_axis)?;
    (dot(frame_normal, outline_normal) >= 1.0 - EPS_AGREE).then_some(())?;
    (dot(frame_u_axis, outline_u_axis) >= 1.0 - EPS_AGREE).then_some(())?;
    let frame_origin = frame.origin?;
    let displacement = dot(outline_normal, outline.origin) - dot(outline_normal, frame_origin);
    let chart_origin =
        std::array::from_fn(|axis| displacement.mul_add(outline_normal[axis], frame_origin[axis]));
    Some(PlaneCandidate {
        equation: PlaneEquation {
            origin: outline.origin,
            normal: outline.normal,
        },
        chart: Some(PlaneChart {
            origin: chart_origin,
            normal: frame.normal?,
            u_axis: frame.u_axis?,
        }),
        offset: frame.offset,
    })
}

pub fn envelope_reconciled_plane_candidate(
    frame: &crate::surface::PlaneLocalSystem,
    equation: PlaneEquation,
) -> Option<PlaneCandidate> {
    let origin = frame.origin?;
    let normal = normalized(equation.normal)?;
    let origin_scale = origin
        .iter()
        .chain(equation.origin.iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    ((dot(normal, origin) - dot(normal, equation.origin)).abs() <= EPS_AGREE * origin_scale)
        .then_some(())?;
    let slots: [f64; 12] = frame
        .slots
        .iter()
        .copied()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    let supports = [
        <[f64; 3]>::try_from(&slots[0..3]).ok()?,
        <[f64; 3]>::try_from(&slots[3..6]).ok()?,
        <[f64; 3]>::try_from(&slots[6..9]).ok()?,
    ];
    let support_scale = supports
        .iter()
        .flatten()
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let nonzero = supports
        .into_iter()
        .filter_map(|support| {
            let magnitude = dot(support, support).sqrt();
            (magnitude > EPS_AGREE * support_scale).then_some((support, magnitude))
        })
        .collect::<Vec<_>>();
    let [first, second] = nonzero.as_slice() else {
        return None;
    };
    let role = |(support, magnitude): &([f64; 3], f64)| {
        let alignment = dot(*support, normal).abs() / *magnitude;
        if alignment <= EPS_AGREE {
            Some((false, support.map(|value| value / *magnitude)))
        } else if (alignment - 1.0).abs() <= EPS_AGREE {
            Some((true, support.map(|value| value / *magnitude)))
        } else {
            None
        }
    };
    let (first_parallel, first_direction) = role(first)?;
    let (second_parallel, second_direction) = role(second)?;
    (first_parallel != second_parallel).then_some(())?;
    let u_axis = if first_parallel {
        second_direction
    } else {
        first_direction
    };
    Some(PlaneCandidate {
        equation,
        chart: Some(PlaneChart {
            origin,
            normal,
            u_axis,
        }),
        offset: frame.offset,
    })
}

pub fn held_coordinate_plane(
    envelope: &crate::surface::PlaneEnvelopeRecord,
) -> Option<PlaneEquation> {
    let corners = plane_envelope_corners(&envelope.envelope)?;
    let held = envelope
        .corner_coordinate_equal
        .iter()
        .enumerate()
        .filter_map(|(axis, equal)| (*equal == Some(true)).then_some(axis))
        .collect::<Vec<_>>();
    let [axis] = held.as_slice() else {
        return None;
    };
    envelope
        .corner_coordinate_equal
        .iter()
        .enumerate()
        .all(|(candidate, equal)| candidate == *axis || *equal == Some(false))
        .then_some(())?;
    let mut normal = [0.0; 3];
    normal[*axis] = 1.0;
    Some(PlaneEquation {
        origin: corners[0],
        normal,
    })
}

pub fn placed_planes(scan: &ContainerScan) -> BTreeMap<u32, PlaneEquation> {
    plane_candidates(scan)
        .into_iter()
        .filter_map(|(id, candidates)| {
            agreed_plane(
                &candidates
                    .iter()
                    .map(|candidate| candidate.equation)
                    .collect::<Vec<_>>(),
            )
            .map(|plane| (id, plane))
        })
        .collect()
}

pub fn placed_plane_surfaces(
    scan: &ContainerScan,
) -> BTreeMap<u32, (PlaneEquation, [f64; 3], usize)> {
    plane_candidates(scan)
        .into_iter()
        .filter_map(|(id, candidates)| {
            agreed_plane_surface(&candidates).map(|surface| (id, surface))
        })
        .collect()
}

pub fn topology_bound_plane(points: impl IntoIterator<Item = [f64; 3]>) -> Option<PlaneEquation> {
    let mut points = points.into_iter().collect::<Vec<_>>();
    points.sort_by(|left, right| {
        left.iter()
            .zip(right)
            .find_map(|(left, right)| {
                let ordering = left.total_cmp(right);
                (ordering != std::cmp::Ordering::Equal).then_some(ordering)
            })
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    points.dedup_by(|left, right| model_points_agree(*left, *right));
    let origin = *points.first()?;
    let scale = points
        .iter()
        .flatten()
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let mut normal = None;
    'candidate: for first in 1..points.len() {
        for second in first + 1..points.len() {
            let first_direction = std::array::from_fn(|axis| points[first][axis] - origin[axis]);
            let second_direction = std::array::from_fn(|axis| points[second][axis] - origin[axis]);
            let Some(candidate) = normalized(cross(first_direction, second_direction)) else {
                continue;
            };
            normal = Some(candidate);
            break 'candidate;
        }
    }
    let mut normal = normal?;
    let leading = normal
        .iter()
        .find(|coordinate| coordinate.abs() > EPS_NEAR_ZERO)?;
    if *leading < 0.0 {
        normal = normal.map(|coordinate| -coordinate);
    }
    points
        .iter()
        .all(|point| {
            let displacement = std::array::from_fn(|axis| point[axis] - origin[axis]);
            dot(displacement, normal).abs() <= EPS_AGREE * scale
        })
        .then_some(PlaneEquation { origin, normal })
}

pub fn analytic_curve_plane(geometry: &CurveGeometry) -> Option<PlaneEquation> {
    let (origin, normal) = match geometry {
        CurveGeometry::Circle { center, axis, .. }
        | CurveGeometry::Ellipse { center, axis, .. } => (
            [center.x, center.y, center.z],
            normalized([axis.x, axis.y, axis.z])?,
        ),
        CurveGeometry::Nurbs(nurbs) => {
            valid_positive_nurbs_curve(nurbs)?;
            let plane = topology_bound_plane(
                nurbs
                    .control_points
                    .iter()
                    .map(|point| [point.x, point.y, point.z]),
            )?;
            (plane.origin, plane.normal)
        }
        _ => return None,
    };
    Some(PlaneEquation { origin, normal })
}

#[derive(Debug, Clone, Copy)]
pub struct BoundaryLine {
    pub origin: [f64; 3],
    pub direction: [f64; 3],
}

pub fn analytic_boundary_line(geometry: &CurveGeometry) -> Option<BoundaryLine> {
    let (origin, direction) = match geometry {
        CurveGeometry::Line { origin, direction } => (
            [origin.x, origin.y, origin.z],
            normalized([direction.x, direction.y, direction.z])?,
        ),
        CurveGeometry::Nurbs(nurbs) => {
            (nurbs.degree == 1 && !nurbs.periodic).then_some(())?;
            valid_positive_nurbs_curve(nurbs)?;
            let first = *nurbs.control_points.first()?;
            let last = *nurbs.control_points.last()?;
            let origin = [first.x, first.y, first.z];
            let direction = normalized([last.x - first.x, last.y - first.y, last.z - first.z])?;
            let scale = nurbs
                .control_points
                .iter()
                .flat_map(|point| [point.x, point.y, point.z])
                .map(f64::abs)
                .fold(1.0, f64::max);
            nurbs
                .control_points
                .iter()
                .map(|point| {
                    let relative = [
                        point.x - origin[0],
                        point.y - origin[1],
                        point.z - origin[2],
                    ];
                    let residual = cross(relative, direction);
                    dot(residual, residual).sqrt()
                })
                .all(|residual| residual <= EPS_AGREE * scale)
                .then_some(())?;
            (origin, direction)
        }
        _ => return None,
    };
    Some(BoundaryLine { origin, direction })
}

pub fn valid_positive_nurbs_curve(nurbs: &NurbsCurve) -> Option<()> {
    nurbs_intrinsic_parameter_range(nurbs)?;
    nurbs
        .weights
        .as_ref()
        .is_none_or(|weights| {
            weights
                .iter()
                .all(|weight| weight.is_finite() && *weight > 0.0)
        })
        .then_some(())
}

pub fn topology_bound_line_plane(lines: &[BoundaryLine]) -> Option<PlaneEquation> {
    let mut candidate = None;
    'pairs: for first in 0..lines.len() {
        for second in first + 1..lines.len() {
            let direction_cross = cross(lines[first].direction, lines[second].direction);
            let displacement =
                std::array::from_fn(|axis| lines[second].origin[axis] - lines[first].origin[axis]);
            let normal = normalized(direction_cross)
                .or_else(|| normalized(cross(lines[first].direction, displacement)));
            if let Some(normal) = normal {
                candidate = Some(PlaneEquation {
                    origin: lines[first].origin,
                    normal,
                });
                break 'pairs;
            }
        }
    }
    let candidate = candidate?;
    let canonical = agreed_plane(&[candidate])?;
    lines
        .iter()
        .all(|line| {
            point_on_carrier(line.origin, CarrierEquation::Plane(canonical))
                && dot(line.direction, canonical.normal).abs() <= EPS_AGREE
        })
        .then_some(canonical)
}

pub fn agreed_topology_bound_plane(
    points: impl IntoIterator<Item = [f64; 3]>,
    curve_planes: impl IntoIterator<Item = PlaneEquation>,
    lines: impl IntoIterator<Item = BoundaryLine>,
) -> Option<PlaneEquation> {
    let points = points.into_iter().collect::<Vec<_>>();
    let lines = lines.into_iter().collect::<Vec<_>>();
    let candidates = topology_bound_plane(points.iter().copied())
        .into_iter()
        .chain(curve_planes)
        .chain(topology_bound_line_plane(&lines))
        .collect::<Vec<_>>();
    let plane = agreed_plane(&candidates)?;
    let points_agree = points
        .iter()
        .all(|point| point_on_carrier(*point, CarrierEquation::Plane(plane)));
    let lines_agree = lines.iter().all(|line| {
        point_on_carrier(line.origin, CarrierEquation::Plane(plane))
            && dot(line.direction, plane.normal).abs() <= EPS_AGREE
    });
    (points_agree && lines_agree).then_some(plane)
}
