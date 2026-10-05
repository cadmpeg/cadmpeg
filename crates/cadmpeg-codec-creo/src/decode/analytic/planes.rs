// SPDX-License-Identifier: Apache-2.0
//! Carrier point tests, plane reconciliation, and placed planes.

use crate::axis::{Axis, Sign};
use crate::decode::sketch_transfer::recipe::feature_schema_class;
use crate::feature::schema::SchemaClass;
use crate::vecmath::normalize;
use crate::vecmath::unit_length;
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::NurbsCurve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};

use crate::container::ContainerScan;

use super::super::holes::placement::plane_envelope_corners;
use super::super::surfaces::intersection_resolve::intersect_plane_with_carrier_components;
use super::super::surfaces::{fc05_cap_pair_model_frame, fc05_model_frame};

use super::edges::{
    nurbs_intrinsic_parameter_range, nurbs_weights_positive, try_fold_nurbs_points,
};
use super::equations::{
    intersect_plane_with_two_quadrics, intersect_two_planes_with_quadric,
    intersect_two_planes_with_torus, solve_planes, CarrierEquation, PlaneEquation, SphereEquation,
};
use super::vertices::{finite_model_point, model_points_agree};
use crate::vecmath::{cross, dot, local_system_lanes};

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

pub(in crate::decode) fn point_on_carrier(point: [f64; 3], carrier: CarrierEquation) -> bool {
    match carrier {
        CarrierEquation::Plane(plane) => {
            let residual = dot(plane.normal, point) - dot(plane.normal, plane.origin);
            residual.abs() <= EPS_ON_CARRIER
        }
        CarrierEquation::Cylinder(cylinder) => {
            let Some(axis) = normalize(cylinder.axis) else {
                return false;
            };
            let relative = std::array::from_fn(|index| point[index] - cylinder.origin[index]);
            let axial = dot(relative, axis);
            let radial: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * axis[index]);
            (radial[0].hypot(radial[1]).hypot(radial[2]) - cylinder.radius).abs()
                <= EPS_ON_CARRIER * cylinder.radius
        }
        CarrierEquation::Cone(cone) => {
            let (Some(axis), Some(x_axis)) =
                (normalize(cone.axis()), normalize(cone.ref_direction()))
            else {
                return false;
            };
            if dot(axis, x_axis).abs() > EPS_ORTHO {
                return false;
            }
            let y_axis = cross(axis, x_axis);
            let relative = std::array::from_fn(|index| point[index] - cone.origin()[index]);
            let axial = dot(relative, axis);
            let radius = cone.radius() + axial * cone.half_angle().tan();
            let radial_x = dot(relative, x_axis);
            let radial_y = dot(relative, y_axis) / cone.ratio();
            (radial_x.hypot(radial_y) - radius.abs()).abs() <= EPS_ON_CARRIER * radius.abs()
        }
        CarrierEquation::Sphere(sphere) => {
            let relative: [f64; 3] =
                std::array::from_fn(|index| point[index] - sphere.center[index]);
            (relative[0].hypot(relative[1]).hypot(relative[2]) - sphere.radius).abs()
                <= EPS_ON_CARRIER * sphere.radius
        }
        CarrierEquation::Torus(torus) => {
            let Some(axis) = normalize(torus.axis) else {
                return false;
            };
            let relative = std::array::from_fn(|index| point[index] - torus.center[index]);
            let axial = dot(relative, axis);
            let radial: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * axis[index]);
            let tube_distance =
                (radial[0].hypot(radial[1]).hypot(radial[2]) - torus.major_radius).hypot(axial);
            (tube_distance - torus.minor_radius).abs()
                <= EPS_ON_CARRIER * torus.minor_radius.max(torus.major_radius)
        }
    }
}

fn tangent_sphere_point(first: SphereEquation, second: SphereEquation) -> Option<[f64; 3]> {
    let delta: [f64; 3] = std::array::from_fn(|index| second.center[index] - first.center[index]);
    let distance = delta[0].hypot(delta[1]).hypot(delta[2]);
    if !distance.is_finite() || distance == 0.0 || first.radius <= 0.0 || second.radius <= 0.0 {
        return None;
    }
    let scale = first.radius.max(second.radius).max(distance);
    let first_radius = first.radius / scale;
    let second_radius = second.radius / scale;
    let separation = distance / scale;
    let external = first_radius + second_radius;
    let internal = (first_radius - second_radius).abs();
    let external_tangent = (separation - external).abs() <= EPS_AGREE * external.max(separation);
    let internal_tangent = (separation - internal).abs() <= EPS_AGREE * internal.max(separation);
    if !external_tangent && !internal_tangent {
        return None;
    }
    let signed_radius = if !external_tangent && second.radius > first.radius {
        -first.radius
    } else {
        first.radius
    };
    let point = std::array::from_fn(|index| {
        first.center[index] + signed_radius * (delta[index] / distance)
    });
    point.iter().all(|value| value.is_finite()).then_some(point)
}

fn tangent_plane_sphere_point(plane: PlaneEquation, sphere: SphereEquation) -> Option<[f64; 3]> {
    let normal = normalize(plane.normal)?;
    let signed_distance = dot(
        normal,
        std::array::from_fn(|index| sphere.center[index] - plane.origin[index]),
    );
    let scale = sphere.radius;
    if !signed_distance.is_finite()
        || sphere.radius <= 0.0
        || (signed_distance.abs() - sphere.radius).abs() > EPS_AGREE * scale
    {
        return None;
    }
    Some(std::array::from_fn(|index| {
        sphere.center[index] - signed_distance * normal[index]
    }))
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct CarrierSolveDiagnostics {
    pub(super) pair_intersections: usize,
    pub(super) triple_intersections: usize,
    pub(super) valid_candidates: usize,
    pub(super) unique_solutions: usize,
}

pub(super) fn solve_carriers_with_diagnostics(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    carriers: &[CarrierEquation],
) -> Result<(Option<[f64; 3]>, CarrierSolveDiagnostics), cadmpeg_core::CodecError> {
    let mut candidates = Vec::new();
    let mut diagnostics = CarrierSolveDiagnostics::default();
    for (first_index, first_carrier) in ctx
        .admit_iter(carriers, "creo carrier pair first equations")?
        .enumerate()
    {
        let second_start = first_index + 1;
        for second_carrier in ctx.admit_iter(
            &carriers[second_start..],
            "creo carrier pair second equations",
        )? {
            let candidate_start = candidates.len();
            match (*first_carrier, *second_carrier) {
                (CarrierEquation::Plane(plane), CarrierEquation::Sphere(sphere))
                | (CarrierEquation::Sphere(sphere), CarrierEquation::Plane(plane)) => {
                    if let Some(point) = tangent_plane_sphere_point(plane, sphere) {
                        ctx.reserve_vec(&mut candidates, 1, "creo carrier pair candidates")?;
                        candidates.push(point);
                    }
                }
                (CarrierEquation::Sphere(first), CarrierEquation::Sphere(second)) => {
                    if let Some(point) = tangent_sphere_point(first, second) {
                        ctx.reserve_vec(&mut candidates, 1, "creo carrier pair candidates")?;
                        candidates.push(point);
                    }
                }
                _ => {}
            }
            diagnostics.pair_intersections += candidates.len() - candidate_start;
        }
    }
    for (first_index, first_carrier) in ctx
        .admit_iter(carriers, "creo carrier triple first equations")?
        .enumerate()
    {
        let second_start = first_index + 1;
        for (second_offset, second_carrier) in ctx
            .admit_iter(
                &carriers[second_start..],
                "creo carrier triple second equations",
            )?
            .enumerate()
        {
            let third_start = second_start + second_offset + 1;
            for third_carrier in ctx.admit_iter(
                &carriers[third_start..],
                "creo carrier triple third equations",
            )? {
                let candidate_start = candidates.len();
                let triple = [*first_carrier, *second_carrier, *third_carrier];
                let mut planes = Vec::new();
                let mut cylinders = Vec::new();
                let mut cones = Vec::new();
                let mut spheres = Vec::new();
                let mut tori = Vec::new();
                for carrier in triple {
                    match carrier {
                        CarrierEquation::Plane(plane) => {
                            ctx.reserve_vec(&mut planes, 1, "creo carrier triple groups")?;
                            planes.push(plane);
                        }
                        CarrierEquation::Cylinder(cylinder) => {
                            ctx.reserve_vec(&mut cylinders, 1, "creo carrier triple groups")?;
                            cylinders.push(cylinder);
                        }
                        CarrierEquation::Cone(cone) => {
                            ctx.reserve_vec(&mut cones, 1, "creo carrier triple groups")?;
                            cones.push(cone);
                        }
                        CarrierEquation::Sphere(sphere) => {
                            ctx.reserve_vec(&mut spheres, 1, "creo carrier triple groups")?;
                            spheres.push(sphere);
                        }
                        CarrierEquation::Torus(torus) => {
                            ctx.reserve_vec(&mut tori, 1, "creo carrier triple groups")?;
                            tori.push(torus);
                        }
                    }
                }
                if planes.len() == 3 {
                    if let Some(point) = solve_planes(ctx, &planes)? {
                        ctx.reserve_vec(&mut candidates, 1, "creo carrier triple candidates")?;
                        candidates.push(point);
                    }
                } else if planes.len() == 1
                    && tori.is_empty()
                    && cylinders.len() + cones.len() + spheres.len() == 2
                {
                    let reduced = if let [first, second] = cones.as_slice() {
                        intersect_plane_with_carrier_components(
                            ctx,
                            planes[0],
                            CarrierEquation::Cone(*first),
                            CarrierEquation::Cone(*second),
                        )?
                    } else {
                        Vec::new()
                    };
                    if reduced.is_empty() {
                        let mut quadrics = cylinders
                            .iter()
                            .copied()
                            .map(CarrierEquation::Cylinder)
                            .chain(cones.iter().copied().map(CarrierEquation::Cone))
                            .chain(spheres.iter().copied().map(CarrierEquation::Sphere));
                        let Some(first_quadric) = quadrics.next() else {
                            continue;
                        };
                        let Some(second_quadric) = quadrics.next() else {
                            continue;
                        };
                        let intersections = intersect_plane_with_two_quadrics(
                            ctx,
                            planes[0],
                            first_quadric,
                            second_quadric,
                        )?;
                        ctx.reserve_vec(
                            &mut candidates,
                            intersections.len(),
                            "creo carrier triple candidates",
                        )?;
                        candidates.extend(intersections);
                    } else {
                        ctx.reserve_vec(
                            &mut candidates,
                            reduced.len(),
                            "creo carrier triple candidates",
                        )?;
                        candidates.extend(reduced);
                    }
                } else if let ([first, second], []) = (planes.as_slice(), tori.as_slice()) {
                    let intersections =
                        match (cylinders.as_slice(), cones.as_slice(), spheres.as_slice()) {
                            ([cylinder], [], []) => intersect_two_planes_with_quadric(
                                ctx,
                                *first,
                                *second,
                                CarrierEquation::Cylinder(*cylinder),
                            ),
                            ([], [cone], []) => intersect_two_planes_with_quadric(
                                ctx,
                                *first,
                                *second,
                                CarrierEquation::Cone(*cone),
                            ),
                            ([], [], [sphere]) => intersect_two_planes_with_quadric(
                                ctx,
                                *first,
                                *second,
                                CarrierEquation::Sphere(*sphere),
                            ),
                            _ => Ok(Vec::new()),
                        }?;
                    ctx.reserve_vec(
                        &mut candidates,
                        intersections.len(),
                        "creo carrier triple candidates",
                    )?;
                    candidates.extend(intersections);
                } else if let ([first, second], [torus]) = (planes.as_slice(), tori.as_slice()) {
                    if cylinders.is_empty() && cones.is_empty() && spheres.is_empty() {
                        let intersections =
                            intersect_two_planes_with_torus(ctx, *first, *second, *torus)?;
                        ctx.reserve_vec(
                            &mut candidates,
                            intersections.len(),
                            "creo carrier triple candidates",
                        )?;
                        candidates.extend(intersections);
                    }
                } else if let ([plane], [cylinder], [torus]) =
                    (planes.as_slice(), cylinders.as_slice(), tori.as_slice())
                {
                    if cones.is_empty() && spheres.is_empty() {
                        let intersections = intersect_plane_with_carrier_components(
                            ctx,
                            *plane,
                            CarrierEquation::Cylinder(*cylinder),
                            CarrierEquation::Torus(*torus),
                        )?;
                        ctx.reserve_vec(
                            &mut candidates,
                            intersections.len(),
                            "creo carrier triple candidates",
                        )?;
                        candidates.extend(intersections);
                    }
                } else if let ([plane], [cone], [sphere]) =
                    (planes.as_slice(), cones.as_slice(), spheres.as_slice())
                {
                    if cylinders.is_empty() && tori.is_empty() {
                        let intersections = intersect_plane_with_carrier_components(
                            ctx,
                            *plane,
                            CarrierEquation::Cone(*cone),
                            CarrierEquation::Sphere(*sphere),
                        )?;
                        ctx.reserve_vec(
                            &mut candidates,
                            intersections.len(),
                            "creo carrier triple candidates",
                        )?;
                        candidates.extend(intersections);
                    }
                } else if let ([plane], [cone], [torus]) =
                    (planes.as_slice(), cones.as_slice(), tori.as_slice())
                {
                    if cylinders.is_empty() && spheres.is_empty() {
                        let intersections = intersect_plane_with_carrier_components(
                            ctx,
                            *plane,
                            CarrierEquation::Cone(*cone),
                            CarrierEquation::Torus(*torus),
                        )?;
                        ctx.reserve_vec(
                            &mut candidates,
                            intersections.len(),
                            "creo carrier triple candidates",
                        )?;
                        candidates.extend(intersections);
                    }
                } else if let ([plane], [sphere], [torus]) =
                    (planes.as_slice(), spheres.as_slice(), tori.as_slice())
                {
                    if cylinders.is_empty() && cones.is_empty() {
                        let intersections = intersect_plane_with_carrier_components(
                            ctx,
                            *plane,
                            CarrierEquation::Sphere(*sphere),
                            CarrierEquation::Torus(*torus),
                        )?;
                        ctx.reserve_vec(
                            &mut candidates,
                            intersections.len(),
                            "creo carrier triple candidates",
                        )?;
                        candidates.extend(intersections);
                    }
                } else if let ([plane], [first, second]) = (planes.as_slice(), tori.as_slice()) {
                    if cylinders.is_empty() && cones.is_empty() && spheres.is_empty() {
                        let intersections = intersect_plane_with_carrier_components(
                            ctx,
                            *plane,
                            CarrierEquation::Torus(*first),
                            CarrierEquation::Torus(*second),
                        )?;
                        ctx.reserve_vec(
                            &mut candidates,
                            intersections.len(),
                            "creo carrier triple candidates",
                        )?;
                        candidates.extend(intersections);
                    }
                }
                diagnostics.triple_intersections += candidates.len() - candidate_start;
            }
        }
    }
    ctx.retain_vec(
        &mut candidates,
        |point| {
            Ok(ctx
                .admit_iter(carriers, "creo carrier candidate equations")?
                .all(|carrier| point_on_carrier(*point, *carrier)))
        },
        "creo carrier candidate retention",
    )?;
    diagnostics.valid_candidates = candidates.len();
    let mut unique = Vec::<[f64; 3]>::new();
    for candidate in ctx.admit_iter(&candidates, "creo unique carrier candidates")? {
        if !ctx
            .admit_iter(&unique, "creo unique carrier solution search")?
            .any(|known| {
                known
                    .iter()
                    .zip(candidate)
                    .all(|(left, right)| (left - right).abs() <= EPS_POINT_UNIQUE)
            })
        {
            ctx.reserve_vec(&mut unique, 1, "creo carrier unique candidates")?;
            unique.push(*candidate);
        }
    }
    diagnostics.unique_solutions = unique.len();
    let point = match unique.as_slice() {
        [point] => Some(*point),
        _ => None,
    };
    Ok((point, diagnostics))
}

#[cfg(test)]
pub(in crate::decode) fn solve_carriers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    carriers: &[CarrierEquation],
) -> Result<Option<[f64; 3]>, cadmpeg_core::CodecError> {
    Ok(solve_carriers_with_diagnostics(ctx, carriers)?.0)
}

pub(in crate::decode) fn is_axis_aligned(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    vector: [f64; 3],
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(ctx
        .admit_iter(&vector, "creo axis alignment components")?
        .filter(|value| value.abs() > EPS_AGREE)
        .count()
        == 1)
}

pub(in crate::decode) fn canonical_plane(plane: PlaneEquation) -> Option<PlaneEquation> {
    let mut normal = normalize(plane.normal)?;
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

pub(super) fn agreed_plane<T>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &[T],
) -> Result<Option<PlaneEquation>, cadmpeg_core::CodecError>
where
    T: Copy + Into<PlaneEquation>,
{
    let mut planes = ctx.admit_iter(candidates, "creo plane agreement candidates")?;
    let Some(first) = planes.next() else {
        return Ok(None);
    };
    let Some(first) = canonical_plane((*first).into()) else {
        return Ok(None);
    };
    let first_distance = dot(first.normal, first.origin);
    for plane in planes {
        let Some(plane) = canonical_plane((*plane).into()) else {
            return Ok(None);
        };
        let distance = dot(plane.normal, plane.origin);
        let scale = first_distance.abs().max(distance.abs()).max(1.0);
        if !first
            .normal
            .iter()
            .zip(plane.normal)
            .all(|(left, right)| (left - right).abs() <= EPS_AGREE)
            || !((first_distance - distance).abs() <= EPS_AGREE * scale)
        {
            return Ok(None);
        }
    }
    Ok(Some(first))
}

pub(in crate::decode) fn reconciled_model_plane(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    local_planes: &BTreeMap<u32, PlaneEquation>,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    surface_id: u32,
) -> Result<Option<PlaneEquation>, cadmpeg_core::CodecError> {
    let mut model_surfaces = ctx
        .admit_iter(&ir.model.surfaces, "creo reconciled model surfaces")?
        .filter(|surface| {
            crate::identity::matches_numbered_identity(
                surface.id.as_str(),
                "creo:visibgeom:surface#",
                surface_id,
            )
        });
    let first = model_surfaces.next();
    let second = model_surfaces.next();
    let model_plane = match (first, second) {
        (None, None) => None,
        (Some(surface), None) => match source_carriers.surface_geometry(surface) {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
                let origin = plane_surface.origin().get();
                let normal = plane_surface.frame().axis().as_raw();
                Some(PlaneEquation {
                    origin: [origin.x, origin.y, origin.z],
                    normal: [normal.x, normal.y, normal.z],
                })
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. }) => None,
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };
    Ok(
        match (local_planes.get(&surface_id).copied(), model_plane) {
            (Some(local), Some(model)) => agreed_plane(ctx, &[local, model])?,
            (Some(local), None) => Some(local),
            (None, Some(model)) => Some(model),
            (None, None) => None,
        },
    )
}

#[derive(Clone, Copy)]
struct PlaneCandidate {
    pub(in crate::decode) equation: PlaneEquation,
    pub(in crate::decode) chart: Option<PlaneChart>,
    pub(in crate::decode) offset: usize,
}

impl From<PlaneCandidate> for PlaneEquation {
    fn from(candidate: PlaneCandidate) -> Self {
        candidate.equation
    }
}

#[derive(Clone, Copy)]
struct PlaneChart {
    pub(in crate::decode) origin: [f64; 3],
    pub(in crate::decode) normal: [f64; 3],
    pub(in crate::decode) u_axis: [f64; 3],
}

fn agreed_plane_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &[PlaneCandidate],
) -> Result<Option<(PlaneEquation, [f64; 3], usize)>, cadmpeg_core::CodecError> {
    let mut planes = ctx.admit_iter(candidates, "creo plane surface candidates")?;
    let Some(first) = planes.next() else {
        return Ok(None);
    };
    let Some(first) = canonical_plane(first.equation) else {
        return Ok(None);
    };
    let first_distance = dot(first.normal, first.origin);
    for candidate in planes {
        let Some(plane) = canonical_plane(candidate.equation) else {
            return Ok(None);
        };
        let distance = dot(plane.normal, plane.origin);
        let scale = first_distance.abs().max(distance.abs()).max(1.0);
        if !first
            .normal
            .iter()
            .zip(plane.normal)
            .all(|(left, right)| (left - right).abs() <= EPS_AGREE)
            || !((first_distance - distance).abs() <= EPS_AGREE * scale)
        {
            return Ok(None);
        }
    }
    let mut charts = ctx
        .admit_iter(candidates, "creo plane surface chart candidates")?
        .filter_map(|candidate| {
            let chart = candidate.chart?;
            let normal = normalize(chart.normal)?;
            let u_axis = normalize(chart.u_axis)?;
            (dot(normal, u_axis).abs() <= EPS_AGREE).then_some((
                chart.origin,
                normal,
                u_axis,
                candidate.offset,
            ))
        });
    let Some(representative) = charts.by_ref().min_by_key(|(_, _, _, offset)| *offset) else {
        return Ok(None);
    };
    let compatible = ctx
        .admit_iter(candidates, "creo plane surface chart agreement")?
        .filter_map(|candidate| {
            let chart = candidate.chart?;
            let normal = normalize(chart.normal)?;
            let u_axis = normalize(chart.u_axis)?;
            (dot(normal, u_axis).abs() <= EPS_AGREE).then_some((
                chart.origin,
                normal,
                u_axis,
                candidate.offset,
            ))
        })
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
        });
    Ok(compatible.then_some((
        PlaneEquation {
            origin: representative.0,
            normal: representative.1,
        },
        representative.2,
        representative.3,
    )))
}

fn stored_parameter_normal_candidate(
    frame: &crate::surface::PlaneLocalSystem,
    mirror_z: bool,
    mirror_origin_z: bool,
) -> Option<PlaneCandidate> {
    let [mut u_axis, second, mut normal, mut origin] = local_system_lanes(frame.complete_slots()?);
    if second.iter().any(|value| *value != 0.0) {
        return None;
    }
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

fn stored_parameter_origin_sign_candidates(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    base: PlaneCandidate,
) -> Result<([PlaneCandidate; 8], usize), cadmpeg_core::CodecError> {
    let mut nonzero_axes = [0; 3];
    let mut axis_count = 0;
    for (axis, value) in base.equation.origin.into_iter().enumerate() {
        if value.abs() > EPS_STORED_FRAME_NONZERO
            && base.equation.normal[axis].abs() > EPS_STORED_FRAME_NONZERO
        {
            nonzero_axes[axis_count] = axis;
            axis_count += 1;
        }
    }
    let mut candidates = [base; 8];
    if axis_count == 0 {
        return Ok((candidates, 1));
    }
    let mut count = 0;
    let mask_count = 1usize << axis_count;
    let mask_range = 0..mask_count;
    for mask in ctx.admit_iter(&mask_range, "creo stored plane origin sign mask traversal")? {
        let mut candidate = base;
        for (bit, axis) in ctx
            .admit_iter(&nonzero_axes[..axis_count], "creo stored plane axes")?
            .copied()
            .enumerate()
        {
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
            candidates[count] = candidate;
            count += 1;
        }
    }
    Ok((candidates, count))
}

fn stored_parameter_normal_candidates_with_origin_branches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    frame: &crate::surface::PlaneLocalSystem,
    include_origin_z_branches: bool,
) -> Result<Option<([PlaneCandidate; 4], usize)>, cadmpeg_core::CodecError> {
    if frame.classification == crate::surface::LocalSystemClassification::Simple {
        return Ok(None);
    }
    let Some(slots) = frame.complete_slots() else {
        return Ok(None);
    };
    if slots[3..6].iter().any(|value| *value != 0.0) {
        return Ok(None);
    }
    let Some(first) = stored_parameter_normal_candidate(frame, false, false) else {
        return Ok(None);
    };
    let mut candidates = [first; 4];
    let mut count = 0;
    let origin_branches: &[bool] = if include_origin_z_branches {
        &[false, true]
    } else {
        &[false]
    };
    for mirror_z in [false, true] {
        for mirror_origin_z in origin_branches {
            let Some(candidate) =
                stored_parameter_normal_candidate(frame, mirror_z, *mirror_origin_z)
            else {
                return Ok(None);
            };
            let mut duplicate = false;
            for known in ctx.admit_iter(&candidates[..count], "creo stored plane candidates")? {
                if plane_candidates_equivalent(ctx, *known, candidate)? {
                    duplicate = true;
                    break;
                }
            }
            if !duplicate {
                candidates[count] = candidate;
                count += 1;
            }
        }
    }
    Ok((count > 1).then_some((candidates, count)))
}

fn stored_parameter_normal_candidates(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    frame: &crate::surface::PlaneLocalSystem,
) -> Result<Option<([PlaneCandidate; 4], usize)>, cadmpeg_core::CodecError> {
    stored_parameter_normal_candidates_with_origin_branches(ctx, frame, false)
}

fn coordinate_vectors_agree(first: [f64; 3], second: [f64; 3]) -> bool {
    first.into_iter().zip(second).all(|(first, second)| {
        (first - second).abs() <= EPS_AGREE * first.abs().max(second.abs()).max(1.0)
    })
}

fn plane_candidates_equivalent(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    first: PlaneCandidate,
    second: PlaneCandidate,
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(
        agreed_plane(ctx, &[first.equation, second.equation])?.is_some()
            && match (first.chart, second.chart) {
                (Some(first), Some(second)) => {
                    coordinate_vectors_agree(first.origin, second.origin)
                        && coordinate_vectors_agree(first.normal, second.normal)
                        && coordinate_vectors_agree(first.u_axis, second.u_axis)
                }
                (None, None) => true,
                _ => false,
            },
    )
}

fn plane_chart_point(candidate: PlaneCandidate, uv: [f64; 2]) -> Option<FinitePoint3> {
    let chart = candidate.chart?;
    let normal = normalize(chart.normal)?;
    let u_axis = normalize(chart.u_axis)?;
    (dot(normal, u_axis).abs() <= EPS_ORTHO).then_some(())?;
    let v_axis = cross(normal, u_axis);
    finite_model_point(std::array::from_fn(|axis| {
        chart.origin[axis] + uv[0] * u_axis[axis] + uv[1] * v_axis[axis]
    }))
}

fn pcurve_candidate_endpoint_witness(
    candidate: PlaneCandidate,
    adjacent: PlaneCandidate,
    endpoints: [[f64; 2]; 2],
) -> bool {
    if candidate.chart.is_none() {
        return false;
    }
    let Some(adjacent_normal) = normalize(adjacent.equation.normal) else {
        return false;
    };
    let Some(candidate_normal) = normalize(candidate.equation.normal) else {
        return false;
    };
    let cross_normals = cross(candidate_normal, adjacent_normal);
    if dot(cross_normals, cross_normals) <= EPS_ORTHO * EPS_ORTHO {
        return false;
    }
    let [Some(first), Some(second)] = endpoints.map(|uv| plane_chart_point(candidate, uv)) else {
        return false;
    };
    if model_points_agree(first, second) {
        return false;
    }
    [first, second].into_iter().all(|point| {
        point_on_carrier(
            <[f64; 3]>::from(point.get()),
            CarrierEquation::Plane(adjacent.equation),
        )
    })
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    domains: &BTreeMap<u32, Vec<PlaneCandidate>>,
) -> Result<Vec<PlaneBranchConstraint>, cadmpeg_core::CodecError> {
    let mut constraints = Vec::new();
    let mut add = |faces: [Option<NonZeroU32>; 2],
                   endpoint_sets: [[[f64; 2]; 2]; 2]|
     -> Result<(), cadmpeg_core::CodecError> {
        let [Some(first), Some(second)] = faces else {
            return Ok(());
        };
        let faces = [first.get(), second.get()];
        if faces[0] == faces[1] {
            return Ok(());
        }
        let (Some(first), Some(second)) = (domains.get(&faces[0]), domains.get(&faces[1])) else {
            return Ok(());
        };
        let mut compatible = false;
        for first_candidate in ctx.admit_iter(first, "creo first plane branch candidates")? {
            if ctx
                .admit_iter(second, "creo second plane branch candidates")?
                .any(|second_candidate| {
                    pcurve_candidates_agree(*first_candidate, *second_candidate, endpoint_sets)
                })
            {
                compatible = true;
                break;
            }
        }
        if compatible {
            ctx.reserve_vec(&mut constraints, 1, "creo plane branch constraints")?;
            constraints.push(PlaneBranchConstraint {
                faces,
                endpoint_sets,
            });
        }
        Ok(())
    };
    for pcurve in ctx.admit_iter(
        &scan.curves.pcurves,
        "creo select stored frame carrier pcurve branches pcurves traversal",
    )? {
        add(
            pcurve.faces,
            super::pcurves::canonicalized_pcurve_endpoints(
                scan,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            ),
        )?;
    }
    for pcurve in ctx.admit_iter(
        &scan.curves.bound_prototype_pcurves,
        "creo select stored frame carrier pcurve branches bound prototype pcurves traversal",
    )? {
        add(
            pcurve.faces,
            super::pcurves::canonicalized_pcurve_endpoints(
                scan,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            ),
        )?;
    }
    for pcurve in ctx.admit_iter(
        &scan.curves.two_chart_pcurves,
        "creo select stored frame carrier pcurve branches two chart pcurves traversal",
    )? {
        let faces = pcurve.faces.map(NonZeroU32::new);
        let (Some(first), Some(last)) = (pcurve.samples.first(), pcurve.samples.last()) else {
            continue;
        };
        add(
            faces,
            super::pcurves::canonicalized_pcurve_endpoints(
                scan,
                faces,
                [first[0], last[0]],
                [first[1], last[1]],
            ),
        )?;
    }
    Ok(constraints)
}

fn fc05_cylinder_branch_witnesses(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<BTreeMap<u32, Vec<super::equations::CylinderEquation>>, cadmpeg_core::CodecError> {
    let mut cylinder_frames = BTreeMap::new();
    for pair in ctx.admit_iter(
        &scan.curves.fc05_cylinder_cap_pairs,
        "creo fc05 cylinder branch witnesses fc05 cylinder cap pairs traversal",
    )? {
        let Some(frame) = fc05_cap_pair_model_frame(ctx, scan, pair)? else {
            continue;
        };
        let legacy = super::equations::CylinderEquation {
            origin: frame.origin,
            axis: frame.unit_vector(),
            ref_direction: frame.ref_direction,
            radius: pair.radius_mm,
        };
        let witness = fc05_cylinder_model_witness(ctx, scan, pair.surface_id, legacy)?;
        ctx.insert_btree_map(
            &mut cylinder_frames,
            pair.surface_id,
            witness,
            "creo FC05 cylinder frame nodes",
        )?;
    }

    for circle in ctx.admit_iter(
        &scan.curves.fc05_circles,
        "creo fc05 cylinder branch witnesses fc05 circles traversal",
    )? {
        let Some(topology) = ctx
            .admit_iter(
                &scan.curves.topology_rows,
                "creo FC05 circle topology search",
            )?
            .find(|row| row.id == circle.curve_id)
        else {
            continue;
        };
        let mut planes = ctx
            .admit_iter(&topology.faces, "creo FC05 circle bounded face planes")?
            .flatten()
            .map(|face| face.get())
            .filter(|face| {
                crate::surface::unique_surface_row(&scan.surfaces.rows, *face)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Plane)
            })
            .filter_map(|face| {
                crate::surface::unique_outline_plane(&scan.planes.outlines, face)
                    .map(|plane| (face, plane))
            });
        let mut cylinders = ctx
            .admit_iter(&topology.faces, "creo FC05 circle bounded face cylinders")?
            .flatten()
            .map(|face| face.get())
            .filter(|face| {
                crate::surface::unique_surface_row(&scan.surfaces.rows, *face)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
            });
        let (Some((_, cap)), None, Some(cylinder_id), None) = (
            planes.next(),
            planes.next(),
            cylinders.next(),
            cylinders.next(),
        ) else {
            continue;
        };
        let Some(axis_index) = Axis::ALL
            .into_iter()
            .find(|axis| cap.normal()[axis.index()].abs() > 1.0 - EPS_FC05_CAP_AXIS)
        else {
            continue;
        };
        let (reference, axis_sign) = match circle.angle_parameter {
            crate::curve::Fc05AngleParameterRelation::Inconsistent => (
                circle.sample_direction_row_frame.get(),
                Sign::of_component(cap.normal()[axis_index.index()]),
            ),
            crate::curve::Fc05AngleParameterRelation::Consistent {
                sense,
                reference_direction_row_frame,
            } => (reference_direction_row_frame, Sign::from(sense).reversed()),
        };
        let (origin, axis, ref_direction) = fc05_model_frame(
            axis_index,
            cap.origin[axis_index.index()],
            circle.center_row_frame,
            reference,
            axis_sign,
        );
        if cylinder_frames.contains_key(&cylinder_id) {
            continue;
        }
        let legacy = super::equations::CylinderEquation {
            origin,
            axis,
            ref_direction,
            radius: circle.radius_mm,
        };
        let witness = fc05_cylinder_model_witness(ctx, scan, cylinder_id, legacy)?;
        ctx.insert_btree_map(
            &mut cylinder_frames,
            cylinder_id,
            witness,
            "creo FC05 cylinder frame nodes",
        )?;
    }

    let mut witnesses = BTreeMap::<u32, Vec<super::equations::CylinderEquation>>::new();
    for topology in ctx.admit_iter(
        &scan.curves.topology_rows,
        "creo fc05 cylinder branch witnesses topology rows traversal",
    )? {
        let mut face_ids = [None; 2];
        for (index, face) in ctx
            .admit_iter(&topology.faces, "creo FC05 bounded topology faces")?
            .enumerate()
        {
            face_ids[index] = face.map(|face| face.get());
        }
        let pair = match face_ids {
            [Some(first), Some(second)]
                if first != second
                    && cylinder_frames.contains_key(&first)
                    && crate::surface::unique_surface_row(&scan.surfaces.rows, second)
                        .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Plane) =>
            {
                Some((first, second))
            }
            [Some(first), Some(second)]
                if first != second
                    && cylinder_frames.contains_key(&second)
                    && crate::surface::unique_surface_row(&scan.surfaces.rows, first)
                        .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Plane) =>
            {
                Some((second, first))
            }
            _ => None,
        };
        let Some((cylinder_id, plane_id)) = pair else {
            continue;
        };
        let Some(cylinder) = cylinder_frames.get(&cylinder_id).copied() else {
            continue;
        };
        let entries = ctx
            .entry_btree_map(&mut witnesses, plane_id, "creo FC05 witness plane nodes")?
            .or_default();
        let known_witness = ctx.any_by(
            entries.iter(),
            |known| {
                Ok(known.origin == cylinder.origin
                    && known.axis == cylinder.axis
                    && known.radius.to_bits() == cylinder.radius.to_bits())
            },
            "creo FC05 cylinder witness duplicates",
        )?;
        if !known_witness {
            ctx.reserve_vec(entries, 1, "creo FC05 cylinder witnesses")?;
            entries.push(cylinder);
        }
    }
    Ok(witnesses)
}

/// Select an FC05 cylinder frame only when reference geometry improves the
/// independent stored-plane tangency score. A validated cap pair remains the
/// primary frame source; this witness does not turn an ID match into geometry.
pub(in crate::decode) fn fc05_cylinder_model_witness(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    cylinder_id: u32,
    legacy: super::equations::CylinderEquation,
) -> Result<super::equations::CylinderEquation, cadmpeg_core::CodecError> {
    let mut curve_ids = BTreeSet::new();
    for circle in ctx.admit_iter(
        &scan.curves.fc05_circles,
        "creo FC05 witness native circles",
    )? {
        let mut bounded = false;
        for topology in ctx.admit_iter(
            &scan.curves.topology_rows,
            "creo FC05 witness topology rows",
        )? {
            if topology.id == circle.curve_id && topology.bounds_face(cylinder_id) {
                bounded = true;
                break;
            }
        }
        if bounded {
            ctx.insert_btree_set(
                &mut curve_ids,
                circle.curve_id,
                "creo FC05 witness curve ID nodes",
            )?;
        }
    }
    let mut circles = Vec::new();
    for curve_id in ctx.admit_iter(
        &curve_ids,
        "creo fc05 cylinder model witness curve ids traversal",
    )? {
        for circle in ctx
            .admit_iter(
                &scan.references.circles,
                "creo FC05 reference circle candidates",
            )?
            .filter(|circle| circle.entity_id == *curve_id)
        {
            ctx.reserve_vec(&mut circles, 1, "creo FC05 witness circles")?;
            circles.push(circle);
        }
    }
    let Some(frame) = fc05_reference_circle_frame(&circles) else {
        return Ok(legacy);
    };
    if (frame.radius().get() - legacy.radius).abs() > EPS_FC05_TANGENT_RESIDUAL
        || dot(frame.frame().axis(), legacy.axis).abs() < 1.0 - EPS_FC05_TANGENT_AXIS
    {
        return Ok(legacy);
    }
    let legacy_score = fc05_tangent_plane_score(ctx, scan, cylinder_id, legacy)?;
    let mut reference_origin = frame.frame().origin();
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
    if fc05_tangent_plane_score(ctx, scan, cylinder_id, reference)? > legacy_score {
        Ok(reference)
    } else {
        Ok(legacy)
    }
}

fn fc05_reference_circle_frame(
    circles: &[&crate::reference::ReferenceCircle],
) -> Option<crate::surface::PositionalCylinderFrame> {
    if let Some(frame) =
        super::super::surfaces::cylinders::reference_circle_pair_cylinder_frame(circles)
    {
        return Some(frame);
    }
    let [circle] = circles else {
        return None;
    };
    if !circle.center_stored() {
        return None;
    }
    let radius = circle.radius().get();
    let axis = crate::vecmath::unit_length(circle.axis());
    let start: [f64; 3] = circle.start().get().into();
    let end: [f64; 3] = circle.end().get().into();
    let center: [f64; 3] = circle.center().get().into();
    let radial = std::array::from_fn(|index| start[index] - center[index]);
    let end_radial = std::array::from_fn(|index| end[index] - center[index]);
    let radial_length = dot(radial, radial).sqrt();
    let end_radial_length = dot(end_radial, end_radial).sqrt();
    let scale = center
        .into_iter()
        .chain(start)
        .chain(end)
        .map(f64::abs)
        .fold(radius.max(1.0), f64::max);
    if !radial_length.is_finite()
        || !end_radial_length.is_finite()
        || (radial_length - radius).abs() > EPS_FC05_TANGENT_RESIDUAL * scale
        || (end_radial_length - radius).abs() > EPS_FC05_TANGENT_RESIDUAL * scale
        || dot(axis, radial).abs() > EPS_FC05_TANGENT_RESIDUAL * scale
        || dot(axis, end_radial).abs() > EPS_FC05_TANGENT_RESIDUAL * scale
    {
        return None;
    }
    crate::surface::PositionalCylinderFrame::new(
        center,
        axis,
        radial.map(|value| value / radial_length),
        radius,
        None,
    )
}

fn fc05_tangent_plane_score(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    cylinder_id: u32,
    cylinder: super::equations::CylinderEquation,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut plane_ids = BTreeSet::new();
    for topology in ctx.admit_iter(
        &scan.curves.topology_rows,
        "creo FC05 tangent topology rows",
    )? {
        if !topology.bounds_face(cylinder_id) {
            continue;
        }
        for face in ctx.admit_iter(&topology.faces, "creo FC05 tangent bounded faces")? {
            let Some(face) = face else {
                continue;
            };
            let face = face.get();
            if face != cylinder_id
                && crate::surface::unique_surface_row(&scan.surfaces.rows, face)
                    .is_some_and(|row| row.kind == crate::surface::SurfaceKind::Plane)
            {
                ctx.insert_btree_set(&mut plane_ids, face, "creo FC05 tangent plane ID nodes")?;
            }
        }
    }
    let mut score: usize = 0;
    for plane_id in ctx.admit_iter(&plane_ids, "creo FC05 tangent plane IDs")? {
        let mut tangent = false;
        for frame in ctx.admit_iter(&scan.planes.local_systems, "creo FC05 tangent plane frames")? {
            if frame.surface_id != *plane_id {
                continue;
            }
            if let Some((candidates, count)) = stored_parameter_normal_candidates(ctx, frame)? {
                if ctx
                    .admit_iter(&candidates[..count], "creo FC05 stored plane candidates")?
                    .copied()
                    .any(|candidate| plane_candidate_is_fc05_tangent(candidate, cylinder))
                {
                    tangent = true;
                    break;
                }
            }
        }
        if tangent {
            score = score.checked_add(1).ok_or_else(|| {
                cadmpeg_core::decode::refuse_local_limit(
                    "creo FC05 tangent plane score",
                    u64::MAX,
                    u64::MAX,
                )
            })?;
        }
    }
    Ok(score)
}

fn plane_candidate_is_fc05_tangent(
    candidate: PlaneCandidate,
    cylinder: super::equations::CylinderEquation,
) -> bool {
    let Some(normal) = normalize(candidate.equation.normal) else {
        return false;
    };
    let Some(axis) = normalize(cylinder.axis) else {
        return false;
    };
    if dot(normal, axis).abs() > EPS_FC05_TANGENT_AXIS {
        return false;
    }
    let relative =
        std::array::from_fn(|index| cylinder.origin[index] - candidate.equation.origin[index]);
    let signed_distance = dot(normal, relative);
    (signed_distance.abs() - cylinder.radius).abs() <= EPS_FC05_TANGENT_RESIDUAL * cylinder.radius
}

fn plane_candidate_pcurve_lies_on_carrier(
    candidate: PlaneCandidate,
    endpoints: [[f64; 2]; 2],
    carrier: CarrierEquation,
) -> bool {
    let [Some(first), Some(second)] = endpoints.map(|uv| plane_chart_point(candidate, uv)) else {
        return false;
    };
    !model_points_agree(first, second)
        && [first, second]
            .into_iter()
            .all(|point| point_on_carrier(<[f64; 3]>::from(point.get()), carrier))
}

fn native_positional_cylinder_carriers(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<BTreeMap<u32, CarrierEquation>, cadmpeg_core::CodecError> {
    let mut carriers = BTreeMap::new();
    let unique_rows =
        crate::identity::uniquely_identified_rows_checked(ctx, &scan.surfaces.rows, |row| row.id)?;
    for row in ctx.admit_iter(&unique_rows, "creo native positional cylinder rows")? {
        if row.kind != crate::surface::SurfaceKind::Cylinder {
            continue;
        }
        let Some(frame) =
            crate::surface::unique_surface_parameter(&scan.surfaces.parameters, row.id)
                .and_then(crate::surface::SurfaceParameterRecord::positional_cylinder_frame)
        else {
            continue;
        };
        ctx.insert_btree_map(
            &mut carriers,
            row.id,
            CarrierEquation::Cylinder(super::equations::CylinderEquation {
                origin: frame.frame().origin(),
                axis: frame.frame().axis(),
                ref_direction: frame.frame().ref_direction(),
                radius: frame.radius().get(),
            }),
            "creo plane branch cylinder carrier nodes",
        )?;
    }
    Ok(carriers)
}

fn select_stored_frame_carrier_pcurve_branches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    variable_domains: &BTreeMap<u32, Vec<PlaneCandidate>>,
    domains: &mut BTreeMap<u32, Vec<PlaneCandidate>>,
) -> Result<(), cadmpeg_core::CodecError> {
    let carriers = native_positional_cylinder_carriers(ctx, scan)?;
    let mut apply = |faces: [Option<NonZeroU32>; 2],
                     endpoint_sets: [[[f64; 2]; 2]; 2]|
     -> Result<(), cadmpeg_core::CodecError> {
        let [Some(first), Some(second)] = faces else {
            return Ok(());
        };
        let faces = [first.get(), second.get()];
        for face_index in 0..2 {
            let plane_id = faces[face_index];
            let Some(options) = variable_domains.get(&plane_id) else {
                continue;
            };
            let Some(carrier) = carriers.get(&faces[1 - face_index]).copied() else {
                continue;
            };
            let mut retained = options.iter().copied().filter(|candidate| {
                plane_candidate_pcurve_lies_on_carrier(
                    *candidate,
                    endpoint_sets[face_index],
                    carrier,
                )
            });
            if let (Some(candidate), None) = (retained.next(), retained.next()) {
                let mut selected = Vec::new();
                ctx.reserve_vec(&mut selected, 1, "creo carrier pcurve plane branch")?;
                selected.push(candidate);
                domains.insert(plane_id, selected);
            }
        }
        Ok(())
    };
    for pcurve in ctx.admit_iter(
        &scan.curves.pcurves,
        "creo stored frame branch constraints pcurves traversal",
    )? {
        apply(
            pcurve.faces,
            super::pcurves::canonicalized_pcurve_endpoints(
                scan,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            ),
        )?;
    }
    for pcurve in ctx.admit_iter(
        &scan.curves.bound_prototype_pcurves,
        "creo stored frame branch constraints bound prototype pcurves traversal",
    )? {
        apply(
            pcurve.faces,
            super::pcurves::canonicalized_pcurve_endpoints(
                scan,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            ),
        )?;
    }
    for pcurve in ctx.admit_iter(
        &scan.curves.two_chart_pcurves,
        "creo stored frame branch constraints two chart pcurves traversal",
    )? {
        let faces = pcurve.faces.map(NonZeroU32::new);
        let (Some(first), Some(last)) = (pcurve.samples.first(), pcurve.samples.last()) else {
            continue;
        };
        apply(
            faces,
            super::pcurves::canonicalized_pcurve_endpoints(
                scan,
                faces,
                [first[0], last[0]],
                [first[1], last[1]],
            ),
        )?;
    }
    Ok(())
}

fn select_stored_frame_branches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    candidates: &mut BTreeMap<u32, Vec<PlaneCandidate>>,
) -> Result<(), cadmpeg_core::CodecError> {
    let cylinder_witnesses = fc05_cylinder_branch_witnesses(ctx, scan)?;
    let mut variable_domains = BTreeMap::<u32, Vec<PlaneCandidate>>::new();
    let mut origin_domains = BTreeMap::<u32, Vec<PlaneCandidate>>::new();
    for frame in ctx.admit_iter(
        &scan.planes.local_systems,
        "creo select stored frame branches local systems traversal",
    )? {
        let Some((options, option_count)) = stored_parameter_normal_candidates(ctx, frame)? else {
            continue;
        };
        if cylinder_witnesses.contains_key(&frame.surface_id) {
            if let Some((origin_options, origin_count)) =
                stored_parameter_normal_candidates_with_origin_branches(ctx, frame, true)?
            {
                let known = ctx
                    .entry_btree_map(
                        &mut origin_domains,
                        frame.surface_id,
                        "creo plane origin domain nodes",
                    )?
                    .or_default();
                for option in origin_options.into_iter().take(origin_count) {
                    let duplicate = ctx.any_by(
                        known.iter(),
                        |candidate| plane_candidates_equivalent(ctx, *candidate, option),
                        "creo plane origin domain candidates",
                    )?;
                    if !duplicate {
                        ctx.reserve_vec(known, 1, "creo plane origin domain candidates")?;
                        known.push(option);
                    }
                }
            }
        }
        let known = ctx
            .entry_btree_map(
                &mut variable_domains,
                frame.surface_id,
                "creo plane variable domain nodes",
            )?
            .or_default();
        for option in options.into_iter().take(option_count) {
            let duplicate = ctx.any_by(
                known.iter(),
                |candidate| plane_candidates_equivalent(ctx, *candidate, option),
                "creo plane variable domain candidates",
            )?;
            if !duplicate {
                ctx.reserve_vec(known, 1, "creo plane variable domain candidates")?;
                known.push(option);
            }
        }
    }
    for (surface_id, options) in
        ctx.admit_iter(&origin_domains, "creo FC05 origin plane domains")?
    {
        let Some(witnesses) = cylinder_witnesses.get(surface_id) else {
            continue;
        };
        let mut retained = None;
        let mut ambiguous = false;
        for candidate in ctx.admit_iter(options, "creo FC05 origin plane candidates")? {
            if ctx
                .admit_iter(witnesses, "creo FC05 origin cylinder witnesses")?
                .copied()
                .any(|cylinder| plane_candidate_is_fc05_tangent(*candidate, cylinder))
            {
                if retained.is_some() {
                    ambiguous = true;
                    break;
                }
                retained = Some(*candidate);
            }
        }
        if let Some(candidate) = retained.filter(|_| !ambiguous) {
            let mut selected = Vec::new();
            ctx.reserve_vec(&mut selected, 1, "creo FC05 origin plane branch")?;
            selected.push(candidate);
            ctx.insert_btree_map(
                &mut variable_domains,
                *surface_id,
                selected,
                "creo FC05 origin plane branch nodes",
            )?;
        }
    }
    if variable_domains.is_empty() {
        return Ok(());
    }

    let mut domains = BTreeMap::new();
    for (surface_id, options) in ctx.admit_iter(
        &variable_domains,
        "creo select stored frame branches variable domains traversal",
    )? {
        let mut copied = Vec::new();
        ctx.reserve_vec(
            &mut copied,
            options.len(),
            "creo copied plane domain candidates",
        )?;
        copied.extend_from_slice(options);
        ctx.insert_btree_map(
            &mut domains,
            *surface_id,
            copied,
            "creo copied plane domain nodes",
        )?;
    }
    select_stored_frame_carrier_pcurve_branches(ctx, scan, &variable_domains, &mut domains)?;
    for (surface_id, options) in ctx.admit_iter(
        &variable_domains,
        "creo select stored frame branches variable domains traversal",
    )? {
        let Some(witnesses) = cylinder_witnesses.get(surface_id) else {
            continue;
        };
        let mut retained = None;
        let mut ambiguous = false;
        for candidate in ctx.admit_iter(options, "creo FC05 tangent plane branch candidates")? {
            if ctx
                .admit_iter(witnesses, "creo FC05 tangent plane branch witnesses")?
                .copied()
                .any(|cylinder| plane_candidate_is_fc05_tangent(*candidate, cylinder))
            {
                if retained.is_some() {
                    ambiguous = true;
                    break;
                }
                retained = Some(*candidate);
            }
        }
        if let Some(candidate) = retained.filter(|_| !ambiguous) {
            let mut selected = Vec::new();
            ctx.reserve_vec(&mut selected, 1, "creo FC05 tangent plane branch")?;
            selected.push(candidate);
            ctx.insert_btree_map(
                &mut domains,
                *surface_id,
                selected,
                "creo FC05 tangent plane branch nodes",
            )?;
        }
    }
    for (surface_id, known) in ctx.admit_iter(&*candidates, "creo fixed plane candidate domains")? {
        let fixed = if known.len() == 1 {
            known
                .first()
                .copied()
                .filter(|candidate| candidate.chart.is_some())
        } else {
            agreed_plane_surface(ctx, known)?.map(|(equation, u_axis, offset)| PlaneCandidate {
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
            if !ctx.contains_key_btree_map(
                &domains,
                surface_id,
                "creo fixed plane domain lookup",
            )? {
                let mut selected = Vec::new();
                ctx.reserve_vec(&mut selected, 1, "creo fixed plane domain candidates")?;
                selected.push(fixed);
                ctx.insert_btree_map(
                    &mut domains,
                    *surface_id,
                    selected,
                    "creo fixed plane domain nodes",
                )?;
            }
        }
    }
    let constraints = stored_frame_branch_constraints(ctx, scan, &domains)?;

    let mut filtered = domains;
    loop {
        ctx.charge_work(1, "creo plane branch propagation rounds")?;
        let mut changed = false;
        for constraint in ctx.admit_iter(&constraints, "creo plane branch constraints")? {
            for (target, other, operation) in [
                (0, 1, "creo filtered first plane candidates"),
                (1, 0, "creo filtered second plane candidates"),
            ] {
                let target_face = constraint.faces[target];
                let other_face = constraint.faces[other];
                if !ctx.contains_key_btree_map(
                    &filtered,
                    &other_face,
                    "creo constrained plane domain lookup",
                )? {
                    break;
                }
                let Some(target_candidates) = ctx.get_btree_map(
                    &filtered,
                    &target_face,
                    "creo constrained plane domain lookup",
                )?
                else {
                    break;
                };
                if !ctx.contains_key_btree_map(
                    &variable_domains,
                    &target_face,
                    "creo variable plane domain lookup",
                )? {
                    continue;
                }
                let Some(other_candidates) = ctx.get_btree_map(
                    &filtered,
                    &other_face,
                    "creo constrained plane domain lookup",
                )?
                else {
                    break;
                };
                let mut retained = Vec::new();
                for candidate in
                    ctx.admit_iter(target_candidates, "creo constrained plane candidates")?
                {
                    let agrees = ctx.any_by(
                        other_candidates.iter(),
                        |other| {
                            Ok(if target == 0 {
                                pcurve_candidates_agree(
                                    *candidate,
                                    *other,
                                    constraint.endpoint_sets,
                                )
                            } else {
                                pcurve_candidates_agree(
                                    *other,
                                    *candidate,
                                    constraint.endpoint_sets,
                                )
                            })
                        },
                        "creo opposite constrained plane candidates",
                    )?;
                    if agrees {
                        ctx.reserve_vec(&mut retained, 1, operation)?;
                        retained.push(*candidate);
                    }
                }
                if retained.is_empty() {
                    break;
                }
                changed |= retained.len() != target_candidates.len();
                ctx.insert_btree_map(
                    &mut filtered,
                    target_face,
                    retained,
                    "creo filtered plane domain nodes",
                )?;
            }
        }
        if !changed {
            break;
        }
    }
    for (surface_id, _) in ctx.admit_iter(
        &variable_domains,
        "creo final stored plane variable domains",
    )? {
        let Some([candidate]) = filtered.get(surface_id).map(Vec::as_slice) else {
            continue;
        };
        let mut selected = Vec::new();
        ctx.reserve_vec(&mut selected, 1, "creo selected plane branch")?;
        selected.push(*candidate);
        ctx.insert_btree_map(
            candidates,
            *surface_id,
            selected,
            "creo selected plane branch nodes",
        )?;
    }
    Ok(())
}

fn round_edge_endpoint_plane_score(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    candidate: PlaneCandidate,
    envelopes: &[crate::surface::Type24RoundEdgeEnvelope],
) -> Result<usize, cadmpeg_core::CodecError> {
    Ok(ctx
        .admit_iter(envelopes, "creo round-edge endpoint plane score envelopes")?
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
        .count())
}

fn unique_round_edge_origin_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &[PlaneCandidate],
    envelopes: &[crate::surface::Type24RoundEdgeEnvelope],
) -> Result<Option<PlaneCandidate>, cadmpeg_core::CodecError> {
    let mut best = None;
    let mut maximum = 0;
    let mut tied = false;
    for candidate in ctx.admit_iter(candidates, "creo round-edge origin plane candidates")? {
        let candidate = *candidate;
        let score = round_edge_endpoint_plane_score(ctx, candidate, envelopes)?;
        if score > maximum {
            maximum = score;
            best = Some(candidate);
            tied = false;
        } else if score == maximum && score > 0 {
            tied = true;
        }
    }
    Ok(if maximum > 0 && !tied { best } else { None })
}

fn round_edge_envelopes_for_plane(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    plane_id: u32,
) -> Result<Vec<crate::surface::Type24RoundEdgeEnvelope>, cadmpeg_core::CodecError> {
    let mut rows = BTreeMap::new();
    let unique_rows =
        crate::identity::uniquely_identified_rows_checked(ctx, &scan.surfaces.rows, |row| row.id)?;
    for row in ctx.admit_iter(&unique_rows, "creo round-edge unique surface rows")? {
        ctx.insert_btree_map(&mut rows, row.id, row, "creo round-edge surface row nodes")?;
    }
    let unique_topologies = crate::identity::uniquely_identified_rows_checked(
        ctx,
        &scan.curves.topology_rows,
        |row| row.id,
    )?;
    let mut envelopes = Vec::new();
    for topology in ctx.admit_iter(
        &unique_topologies,
        "creo round-edge unique curve topologies",
    )? {
        let mut cylinder_id = None;
        for face in ctx.admit_iter(&topology.faces, "creo round-edge bounded topology faces")? {
            let Some(face) = face else {
                continue;
            };
            let face = face.get();
            if face == plane_id {
                continue;
            }
            let Some(row) = rows.get(&face) else {
                continue;
            };
            if row.kind == crate::surface::SurfaceKind::Cylinder
                && feature_schema_class(ctx, scan, row.feature_id)? == Some(SchemaClass::Round)
            {
                cylinder_id = Some(face);
                break;
            }
        }
        let Some(cylinder_id) = cylinder_id else {
            continue;
        };
        if !topology.bounds_face(plane_id) {
            continue;
        }
        let Some(record) =
            crate::surface::unique_surface_parameter(&scan.surfaces.parameters, cylinder_id)
        else {
            continue;
        };
        if let Some(envelope) = record.type24_round_edge_envelope() {
            ctx.reserve_vec(&mut envelopes, 1, "creo round-edge plane envelopes")?;
            envelopes.push(envelope);
        }
    }
    Ok(envelopes)
}

fn select_round_edge_origin_branches(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    candidates: &mut BTreeMap<u32, Vec<PlaneCandidate>>,
) -> Result<(), cadmpeg_core::CodecError> {
    for frame in ctx.admit_iter(
        &scan.planes.local_systems,
        "creo select round edge origin branches local systems traversal",
    )? {
        let decoded_frame = frame.frame();
        if frame.classification != crate::surface::LocalSystemClassification::Simple {
            continue;
        }
        let Some(existing) = candidates.get(&frame.surface_id) else {
            continue;
        };
        let (Some(origin), Some(normal), Some(u_axis)) = (
            decoded_frame.origin,
            decoded_frame.normal(),
            decoded_frame.u_axis(),
        ) else {
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
        if existing.len() != 1 || !plane_candidates_equivalent(ctx, existing[0], base)? {
            continue;
        }
        let envelopes = round_edge_envelopes_for_plane(ctx, scan, frame.surface_id)?;
        let (options, count) = stored_parameter_origin_sign_candidates(ctx, base)?;
        let Some(selected) =
            unique_round_edge_origin_candidate(ctx, &options[..count], &envelopes)?
        else {
            continue;
        };
        if let Some(existing) = candidates.get_mut(&frame.surface_id) {
            existing[0] = selected;
        }
    }
    Ok(())
}

fn plane_candidates(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<BTreeMap<u32, Vec<PlaneCandidate>>, cadmpeg_core::CodecError> {
    let mut matrix_frame_ids = BTreeSet::new();
    for id in ctx
        .admit_iter(&scan.planes.local_systems, "creo matrix plane frame IDs")?
        .filter(|frame| crate::surface::uses_matrix_column_frame(frame))
        .map(|frame| frame.surface_id)
    {
        ctx.insert_btree_set(
            &mut matrix_frame_ids,
            id,
            "creo matrix plane frame ID nodes",
        )?;
    }
    let mut held_plane_groups = BTreeMap::<u32, Vec<PlaneEquation>>::new();
    for (surface_id, plane) in ctx
        .admit_iter(&scan.planes.envelopes, "creo held plane envelope groups")?
        .filter_map(|envelope| Some((envelope.surface_id, held_coordinate_plane(envelope)?)))
    {
        let planes = ctx
            .entry_btree_map(
                &mut held_plane_groups,
                surface_id,
                "creo held plane group nodes",
            )?
            .or_default();
        ctx.reserve_vec(planes, 1, "creo held plane equations")?;
        planes.push(plane);
    }
    let mut held_planes = BTreeMap::new();
    for (surface_id, planes) in
        ctx.admit_iter(&held_plane_groups, "creo held plane candidate groups")?
    {
        if let Some(plane) = agreed_plane(ctx, planes)? {
            ctx.insert_btree_map(
                &mut held_planes,
                *surface_id,
                plane,
                "creo agreed held plane nodes",
            )?;
        }
    }
    let mut frame_bound_outlines = BTreeMap::<u32, Vec<crate::surface::OutlinePlane>>::new();
    for outline in ctx
        .admit_iter(
            &scan.planes.envelopes,
            "creo frame-bound outline envelope search",
        )?
        .filter_map(|record| {
            crate::surface::frame_bound_outline_plane(record, &scan.planes.local_systems)
        })
    {
        let outlines = ctx
            .entry_btree_map(
                &mut frame_bound_outlines,
                outline.surface_id,
                "creo frame-bound outline nodes",
            )?
            .or_default();
        ctx.reserve_vec(outlines, 1, "creo frame-bound outlines")?;
        outlines.push(outline);
    }
    let mut candidates = BTreeMap::<u32, Vec<PlaneCandidate>>::new();
    for frame in ctx.admit_iter(
        &scan.planes.local_systems,
        "creo plane candidates local systems traversal",
    )? {
        let decoded_frame = frame.frame();
        let (Some(origin), Some(normal)) = (decoded_frame.origin, decoded_frame.normal()) else {
            continue;
        };
        let Some(u_axis) = decoded_frame.u_axis() else {
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
        let mut candidate = frame_bound_outlines
            .get(&frame.surface_id)
            .and_then(|outlines| {
                let [outline] = outlines.as_slice() else {
                    return None;
                };
                frame_bound_outline_plane_candidate(frame, outline)
            });
        if candidate.is_none() {
            if let Some(held) = held_planes.get(&frame.surface_id) {
                if agreed_plane(ctx, &[frame_candidate.equation, *held])?.is_none() {
                    candidate = envelope_reconciled_plane_candidate(ctx, frame, *held)?;
                }
            }
        }
        let candidate = match candidate {
            Some(candidate) => candidate,
            None => frame_candidate,
        };
        let options = ctx
            .entry_btree_map(
                &mut candidates,
                frame.surface_id,
                "creo plane candidate nodes",
            )?
            .or_default();
        ctx.reserve_vec(options, 1, "creo plane candidates")?;
        options.push(candidate);
    }
    let mut local_chart_ids = BTreeSet::new();
    for id in ctx
        .admit_iter(&scan.planes.local_systems, "creo local plane chart IDs")?
        .filter(|frame| {
            let decoded_frame = frame.frame();
            decoded_frame.origin.is_some()
                && decoded_frame.normal.is_some()
                && decoded_frame.u_axis.is_some()
        })
        .map(|frame| frame.surface_id)
    {
        ctx.insert_btree_set(&mut local_chart_ids, id, "creo local plane chart ID nodes")?;
    }
    for outline in ctx.admit_iter(
        &scan.planes.outlines,
        "creo plane candidates outlines traversal",
    )? {
        if matrix_frame_ids.contains(&outline.surface_id) {
            continue;
        }
        let candidate = PlaneCandidate {
            equation: PlaneEquation {
                origin: outline.origin,
                normal: outline.normal(),
            },
            chart: (!local_chart_ids.contains(&outline.surface_id)).then_some(PlaneChart {
                origin: outline.origin,
                normal: outline.normal(),
                u_axis: outline.u_axis(),
            }),
            offset: outline.offset,
        };
        let options = ctx
            .entry_btree_map(
                &mut candidates,
                outline.surface_id,
                "creo plane candidate nodes",
            )?
            .or_default();
        ctx.reserve_vec(options, 1, "creo plane candidates")?;
        options.push(candidate);
    }
    for envelope in ctx.admit_iter(
        &scan.planes.envelopes,
        "creo plane candidates envelopes traversal",
    )? {
        if matrix_frame_ids.contains(&envelope.surface_id) {
            continue;
        }
        let Some(equation) = held_coordinate_plane(envelope) else {
            continue;
        };
        let candidate = PlaneCandidate {
            equation,
            chart: None,
            offset: envelope.offset,
        };
        let options = ctx
            .entry_btree_map(
                &mut candidates,
                envelope.surface_id,
                "creo plane candidate nodes",
            )?
            .or_default();
        ctx.reserve_vec(options, 1, "creo plane candidates")?;
        options.push(candidate);
    }
    for plane in ctx.admit_iter(
        &scan.planes.positional_frames,
        "creo plane candidates positional frames traversal",
    )? {
        if candidates.contains_key(&plane.surface_id) {
            continue;
        }
        let mut options = Vec::new();
        ctx.reserve_vec(&mut options, 1, "creo plane candidates")?;
        options.push(PlaneCandidate {
            equation: PlaneEquation {
                origin: plane.origin,
                normal: plane.normal(),
            },
            chart: Some(PlaneChart {
                origin: plane.origin,
                normal: plane.normal(),
                u_axis: plane.u_axis(),
            }),
            offset: plane.offset,
        });
        ctx.insert_btree_map(
            &mut candidates,
            plane.surface_id,
            options,
            "creo plane candidate nodes",
        )?;
    }
    select_stored_frame_branches(ctx, scan, &mut candidates)?;
    select_round_edge_origin_branches(ctx, scan, &mut candidates)?;
    let mut surface_search_refusal = None;
    candidates.retain(|id, _| {
        if surface_search_refusal.is_some() {
            return true;
        }
        match ctx.admit_iter(
            &*scan.surfaces.rows,
            "creo plane candidate surface identity count",
        ) {
            Ok(rows) => rows.filter(|row| row.id == *id).take(2).count() < 2,
            Err(error) => {
                surface_search_refusal = Some(error.into());
                true
            }
        }
    });
    if let Some(error) = surface_search_refusal {
        return Err(error);
    }
    Ok(candidates)
}

fn frame_bound_outline_plane_candidate(
    frame: &crate::surface::PlaneLocalSystem,
    outline: &crate::surface::OutlinePlane,
) -> Option<PlaneCandidate> {
    (frame.surface_id == outline.surface_id).then_some(())?;
    let decoded_frame = frame.frame();
    let frame_normal = decoded_frame.normal()?;
    let frame_u_axis = decoded_frame.u_axis()?;
    let outline_normal = outline.normal();
    let outline_u_axis = outline.u_axis();
    (dot(frame_normal, outline_normal) >= 1.0 - EPS_AGREE).then_some(())?;
    (dot(frame_u_axis, outline_u_axis) >= 1.0 - EPS_AGREE).then_some(())?;
    let frame_origin = decoded_frame.origin?;
    let displacement = dot(outline_normal, outline.origin) - dot(outline_normal, frame_origin);
    let chart_origin =
        std::array::from_fn(|axis| displacement.mul_add(outline_normal[axis], frame_origin[axis]));
    Some(PlaneCandidate {
        equation: PlaneEquation {
            origin: outline.origin,
            normal: outline_normal,
        },
        chart: Some(PlaneChart {
            origin: chart_origin,
            normal: frame_normal,
            u_axis: frame_u_axis,
        }),
        offset: frame.offset,
    })
}

fn envelope_reconciled_plane_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    frame: &crate::surface::PlaneLocalSystem,
    equation: PlaneEquation,
) -> Result<Option<PlaneCandidate>, cadmpeg_core::CodecError> {
    let decoded_frame = frame.frame();
    let Some(origin) = decoded_frame.origin else {
        return Ok(None);
    };
    let Some(normal) = normalize(equation.normal) else {
        return Ok(None);
    };
    let origin_scale = origin
        .iter()
        .chain(equation.origin.iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    if !((dot(normal, origin) - dot(normal, equation.origin)).abs() <= EPS_AGREE * origin_scale) {
        return Ok(None);
    }
    let Some(slots) = frame.complete_slots() else {
        return Ok(None);
    };
    let (Ok(first), Ok(second), Ok(third)) = (
        <[f64; 3]>::try_from(&slots[0..3]),
        <[f64; 3]>::try_from(&slots[3..6]),
        <[f64; 3]>::try_from(&slots[6..9]),
    ) else {
        return Ok(None);
    };
    let supports = [first, second, third];
    let mut support_scale = 1.0_f64;
    for support in ctx.admit_iter(&supports, "creo envelope plane support scale")? {
        for value in ctx.admit_iter(support, "creo envelope plane support coordinates")? {
            support_scale = support_scale.max(value.abs());
        }
    }
    let mut nonzero = supports.into_iter().filter_map(|support| {
        let magnitude = dot(support, support).sqrt();
        (magnitude > EPS_AGREE * support_scale).then_some((support, magnitude))
    });
    let (Some(first), Some(second), None) = (nonzero.next(), nonzero.next(), nonzero.next()) else {
        return Ok(None);
    };
    let role = |(support, magnitude): ([f64; 3], f64)| {
        let alignment = dot(support, normal).abs() / magnitude;
        if alignment <= EPS_AGREE {
            Some((false, support.map(|value| value / magnitude)))
        } else if (alignment - 1.0).abs() <= EPS_AGREE {
            Some((true, support.map(|value| value / magnitude)))
        } else {
            None
        }
    };
    let Some((first_parallel, first_direction)) = role(first) else {
        return Ok(None);
    };
    let Some((second_parallel, second_direction)) = role(second) else {
        return Ok(None);
    };
    if first_parallel == second_parallel {
        return Ok(None);
    }
    let u_axis = if first_parallel {
        second_direction
    } else {
        first_direction
    };
    Ok(Some(PlaneCandidate {
        equation,
        chart: Some(PlaneChart {
            origin,
            normal,
            u_axis,
        }),
        offset: frame.offset,
    }))
}

fn held_coordinate_plane(envelope: &crate::surface::PlaneEnvelopeRecord) -> Option<PlaneEquation> {
    let corners = plane_envelope_corners(&envelope.envelope)?;
    let mut held = envelope
        .corner_coordinate_equal
        .iter()
        .enumerate()
        .filter_map(|(axis, equal)| (*equal == Some(true)).then_some(axis));
    let (Some(axis), None) = (held.next(), held.next()) else {
        return None;
    };
    envelope
        .corner_coordinate_equal
        .iter()
        .enumerate()
        .all(|(candidate, equal)| candidate == axis || *equal == Some(false))
        .then_some(())?;
    let mut normal = [0.0; 3];
    normal[axis] = 1.0;
    Some(PlaneEquation {
        origin: corners[0],
        normal,
    })
}

pub(in crate::decode) fn placed_planes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<BTreeMap<u32, PlaneEquation>, cadmpeg_core::CodecError> {
    let mut placed = BTreeMap::new();
    let candidates = plane_candidates(ctx, scan)?;
    for (id, options) in ctx.admit_iter(&candidates, "creo placed plane candidate groups")? {
        if let Some(plane) = agreed_plane(ctx, options)? {
            ctx.insert_btree_map(&mut placed, *id, plane, "creo placed plane nodes")?;
        }
    }
    Ok(placed)
}

#[derive(Debug)]
pub(in crate::decode) struct PlacedPlaneSurface {
    pub(in crate::decode) plane: PlaneEquation,
    pub(in crate::decode) u_axis: [f64; 3],
    pub(in crate::decode) offset: usize,
}

pub(in crate::decode) fn placed_plane_surfaces(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<BTreeMap<u32, PlacedPlaneSurface>, cadmpeg_core::CodecError> {
    let mut placed = BTreeMap::new();
    let candidates = plane_candidates(ctx, scan)?;
    for (id, options) in ctx.admit_iter(&candidates, "creo placed plane surface groups")? {
        if let Some(surface) = agreed_plane_surface(ctx, options)? {
            let (plane, u_axis, offset) = surface;
            ctx.insert_btree_map(
                &mut placed,
                *id,
                PlacedPlaneSurface {
                    plane,
                    u_axis,
                    offset,
                },
                "creo placed plane surface nodes",
            )?;
        }
    }
    Ok(placed)
}

pub(super) fn topology_bound_plane(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    collect_points: impl FnOnce(&mut Vec<[f64; 3]>) -> Result<(), cadmpeg_core::CodecError>,
) -> Result<Option<PlaneEquation>, cadmpeg_core::CodecError> {
    let mut points = Vec::new();
    collect_points(&mut points)?;
    ctx.stable_sort_by(
        points.as_mut_slice(),
        |value| value,
        |left, right| {
            left.iter()
                .zip(right)
                .find_map(|(left, right)| {
                    let ordering = left.total_cmp(right);
                    (ordering != std::cmp::Ordering::Equal).then_some(ordering)
                })
                .unwrap_or(std::cmp::Ordering::Equal)
        },
        "creo topology bound plane points ordering",
    )?;
    // A point outside the finite range agrees with no point.
    points.dedup_by(|left, right| {
        finite_model_point(*left)
            .zip(finite_model_point(*right))
            .is_some_and(|(left, right)| model_points_agree(left, right))
    });
    let Some(&origin) = points.first() else {
        return Ok(None);
    };
    let mut scale = 1.0_f64;
    for point in ctx.admit_iter(&points, "creo topology plane point scale")? {
        for value in ctx.admit_iter(point, "creo topology plane point coordinates")? {
            scale = scale.max(value.abs());
        }
    }
    let mut normal = None;
    'candidate: for (first, first_point) in ctx
        .admit_iter(&points[1..], "creo topology plane first point candidates")?
        .enumerate()
    {
        let first = first + 1;
        for second_point in ctx.admit_iter(
            &points[first + 1..],
            "creo topology plane second point candidates",
        )? {
            let first_direction = std::array::from_fn(|axis| first_point[axis] - origin[axis]);
            let second_direction = std::array::from_fn(|axis| second_point[axis] - origin[axis]);
            let Some(candidate) = normalize(cross(first_direction, second_direction)) else {
                continue;
            };
            normal = Some(candidate);
            break 'candidate;
        }
    }
    let Some(mut normal) = normal else {
        return Ok(None);
    };
    let Some(leading) = normal
        .iter()
        .find(|coordinate| coordinate.abs() > EPS_NEAR_ZERO)
    else {
        return Ok(None);
    };
    if *leading < 0.0 {
        normal = normal.map(|coordinate| -coordinate);
    }
    Ok(ctx
        .admit_iter(&points, "creo topology plane point agreement")?
        .all(|point| {
            let displacement = std::array::from_fn(|axis| point[axis] - origin[axis]);
            dot(displacement, normal).abs() <= EPS_AGREE * scale
        })
        .then_some(PlaneEquation { origin, normal }))
}

pub(super) fn analytic_curve_plane(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &CurveGeometry,
) -> Result<Option<PlaneEquation>, cadmpeg_core::CodecError> {
    let (origin, normal) = match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
            let center = circle_curve.center().get();
            (
                [center.x, center.y, center.z],
                unit_length(*circle_curve.frame().axis()),
            )
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
            let center = ellipse_curve.center().get();
            (
                [center.x, center.y, center.z],
                unit_length(*ellipse_curve.frame().axis()),
            )
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
            if valid_positive_nurbs_curve(ctx, nurbs)?.is_none() {
                return Ok(None);
            }
            let Some(plane) = topology_bound_plane(ctx, |points| {
                try_fold_nurbs_points(ctx, nurbs, (), |(), point| {
                    let point = point.get();
                    ctx.reserve_vec(points, 1, "creo topology plane candidate points")?;
                    points.push([point.x, point.y, point.z]);
                    Ok(())
                })
            })?
            else {
                return Ok(None);
            };
            (plane.origin, plane.normal)
        }
        _ => return Ok(None),
    };
    Ok(Some(PlaneEquation { origin, normal }))
}

#[derive(Debug, Clone, Copy)]
pub(in crate::decode::analytic) struct BoundaryLine {
    pub(in crate::decode) origin: [f64; 3],
    pub(in crate::decode) direction: [f64; 3],
}

pub(super) fn analytic_boundary_line(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &CurveGeometry,
) -> Result<Option<BoundaryLine>, cadmpeg_core::CodecError> {
    let (origin, direction) = match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            let origin = line_curve.origin().get();
            (
                [origin.x, origin.y, origin.z],
                unit_length(line_curve.direction()),
            )
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
            if nurbs.degree() != 1 || nurbs.periodic() {
                return Ok(None);
            }
            if valid_positive_nurbs_curve(ctx, nurbs)?.is_none() {
                return Ok(None);
            }
            let Some(first) = nurbs.pole_rows().point_at(0) else {
                return Ok(None);
            };
            let Some(last_index) = nurbs.pole_count().checked_sub(1) else {
                return Ok(None);
            };
            let Some(last) = nurbs.pole_rows().point_at(last_index) else {
                return Ok(None);
            };
            let origin = [first.x, first.y, first.z];
            let Some(direction) = normalize([last.x - first.x, last.y - first.y, last.z - first.z])
            else {
                return Ok(None);
            };
            let scale = try_fold_nurbs_points(ctx, nurbs, 1.0_f64, |scale, point| {
                let point = point.get();
                Ok([point.x, point.y, point.z]
                    .into_iter()
                    .map(f64::abs)
                    .fold(scale, f64::max))
            })?;
            let aligned_point = |point: cadmpeg_ir::features::FinitePoint3| {
                let point = point.get();
                let relative = [
                    point.x - origin[0],
                    point.y - origin[1],
                    point.z - origin[2],
                ];
                let residual = cross(relative, direction);
                dot(residual, residual).sqrt() <= EPS_AGREE * scale
            };
            let aligned = match nurbs.pole_rows() {
                cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => ctx
                    .admit_iter(points, "creo NURBS polynomial poles")?
                    .all(|point| aligned_point(*point)),
                cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => ctx
                    .admit_iter(points, "creo NURBS rational poles")?
                    .all(|pole| aligned_point(pole.point)),
            };
            if !aligned {
                return Ok(None);
            }
            (origin, direction)
        }
        _ => return Ok(None),
    };
    Ok(Some(BoundaryLine { origin, direction }))
}

pub(in crate::decode) fn valid_positive_nurbs_curve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    nurbs: &NurbsCurve,
) -> Result<Option<()>, cadmpeg_core::CodecError> {
    if nurbs_intrinsic_parameter_range(nurbs).is_none() || !nurbs_weights_positive(ctx, nurbs)? {
        return Ok(None);
    }
    Ok(Some(()))
}

fn topology_bound_line_plane(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    lines: &[BoundaryLine],
) -> Result<Option<PlaneEquation>, cadmpeg_core::CodecError> {
    let mut candidate = None;
    'pairs: for (first_index, first) in ctx
        .admit_iter(lines, "creo topology boundary lines")?
        .enumerate()
    {
        for second in ctx.admit_iter(
            &lines[first_index + 1..],
            "creo topology boundary line pairs",
        )? {
            let direction_cross = cross(first.direction, second.direction);
            let displacement = std::array::from_fn(|axis| second.origin[axis] - first.origin[axis]);
            let normal = normalize(direction_cross)
                .or_else(|| normalize(cross(first.direction, displacement)));
            if let Some(normal) = normal {
                candidate = Some(PlaneEquation {
                    origin: first.origin,
                    normal,
                });
                break 'pairs;
            }
        }
    }
    let Some(candidate) = candidate else {
        return Ok(None);
    };
    let Some(canonical) = agreed_plane(ctx, &[candidate])? else {
        return Ok(None);
    };
    let agrees = ctx
        .admit_iter(lines, "creo topology boundary line agreement")?
        .all(|line| {
            point_on_carrier(line.origin, CarrierEquation::Plane(canonical))
                && dot(line.direction, canonical.normal).abs() <= EPS_AGREE
        });
    Ok(agrees.then_some(canonical))
}

pub(super) fn agreed_topology_bound_plane(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    input_points: &[[f64; 3]],
    curve_planes: &[PlaneEquation],
    collect_lines: impl FnOnce(&mut Vec<BoundaryLine>) -> Result<(), cadmpeg_core::CodecError>,
) -> Result<Option<PlaneEquation>, cadmpeg_core::CodecError> {
    let mut admitted_points = Vec::new();
    for point in ctx.admit_iter(input_points, "creo plane boundary point source")? {
        ctx.reserve_vec(&mut admitted_points, 1, "creo plane boundary points")?;
        admitted_points.push(*point);
    }
    let mut admitted_lines = Vec::new();
    collect_lines(&mut admitted_lines)?;
    let topology_candidate = topology_bound_plane(ctx, |points| {
        for point in ctx.admit_iter(&admitted_points, "creo topology plane input points")? {
            ctx.reserve_vec(points, 1, "creo topology plane candidate points")?;
            points.push(*point);
        }
        Ok(())
    })?;
    let line_candidate = topology_bound_line_plane(ctx, &admitted_lines)?;
    let mut candidates = Vec::new();
    if let Some(candidate) = topology_candidate {
        ctx.reserve_vec(&mut candidates, 1, "creo plane boundary candidates")?;
        candidates.push(candidate);
    }
    for plane in ctx.admit_iter(curve_planes, "creo plane boundary curve planes")? {
        ctx.reserve_vec(&mut candidates, 1, "creo plane boundary candidates")?;
        candidates.push(*plane);
    }
    if let Some(candidate) = line_candidate {
        ctx.reserve_vec(&mut candidates, 1, "creo plane boundary candidates")?;
        candidates.push(candidate);
    }
    let Some(plane) = agreed_plane(ctx, &candidates)? else {
        return Ok(None);
    };
    let points_agree = ctx
        .admit_iter(&admitted_points, "creo plane boundary point agreement")?
        .all(|point| point_on_carrier(*point, CarrierEquation::Plane(plane)));
    let lines_agree = ctx
        .admit_iter(&admitted_lines, "creo plane boundary line agreement")?
        .all(|line| {
            point_on_carrier(line.origin, CarrierEquation::Plane(plane))
                && dot(line.direction, plane.normal).abs() <= EPS_AGREE
        });
    Ok((points_agree && lines_agree).then_some(plane))
}

#[cfg(test)]
mod reconciliation_tests;

#[cfg(test)]
mod solver_tests;
