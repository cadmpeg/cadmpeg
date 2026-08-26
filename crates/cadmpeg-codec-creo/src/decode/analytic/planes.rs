// SPDX-License-Identifier: Apache-2.0
//! Carrier point tests, plane reconciliation, and placed planes.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::geometry::{CurveGeometry, NurbsCurve};

use crate::container::ContainerScan;

use super::super::holes::plane_envelope_corners;
use super::super::sketch::normalized;
use super::super::surfaces::intersect_plane_with_carrier_components;

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

pub fn solve_carriers(carriers: &[CarrierEquation]) -> Option<[f64; 3]> {
    let mut candidates = Vec::new();
    for first in 0..carriers.len() {
        for second in first + 1..carriers.len() {
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
        }
    }
    for first in 0..carriers.len() {
        for second in first + 1..carriers.len() {
            for third in second + 1..carriers.len() {
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
            }
        }
    }
    candidates.retain(|point| {
        carriers
            .iter()
            .all(|carrier| point_on_carrier(*point, *carrier))
    });
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
    let [point] = unique.as_slice() else {
        return None;
    };
    Some(*point)
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

pub fn plane_candidates(scan: &ContainerScan) -> BTreeMap<u32, Vec<PlaneCandidate>> {
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
