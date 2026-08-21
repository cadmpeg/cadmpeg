// SPDX-License-Identifier: Apache-2.0
//! Analytic pcurve carrier transfer and native pcurve helpers.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, PcurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;

use super::super::native::annotate;
use super::super::sketch::normalized;
use super::super::surfaces::curve_contains_points;

use super::edges::{
    nurbs_control_extent, nurbs_intrinsic_parameter_range, periodic_conic_edge_parameter_range,
    point_pair_alignments,
};
use super::equations::{cross, dot};
use super::vertices::model_points_agree;

const EPS_AGREE: f64 = 1e-9;
const EPS_ORTHO: f64 = 1e-10;
const EPS_NEAR_ZERO: f64 = 1e-12;
const PCURVE_MISMATCH_SAMPLE_LIMIT: usize = 4;

fn unique_model_surface(surfaces: &[Surface], face_id: u32) -> Option<&Surface> {
    let id = SurfaceId(format!("creo:visibgeom:surface#{face_id}"));
    let mut matches = surfaces.iter().filter(|surface| surface.id == id);
    let surface = matches.next()?;
    matches.next().is_none().then_some(surface)
}

fn topology_ignored_surface_ids(
    layout: crate::container::Layout,
    rows: &[crate::surface::SurfaceRow],
) -> BTreeSet<u32> {
    // The interpolation carrier makes a legacy spline surface evaluable, but
    // its trim/intersection join is still unresolved. Keep that surface from
    // vetoing endpoint evidence supplied by a proven adjacent analytic face.
    if layout != crate::container::Layout::LegacyAscii {
        return BTreeSet::new();
    }
    rows.iter()
        .filter(|row| row.kind == crate::surface::SurfaceKind::Spline)
        .map(|row| row.id)
        .collect()
}

pub fn canonicalized_pcurve_endpoints(
    scan: &ContainerScan,
    faces: [u32; 2],
    face_0_endpoints: [[f64; 2]; 2],
    face_1_endpoints: [[f64; 2]; 2],
) -> [[[f64; 2]; 2]; 2] {
    [
        crate::legacy_geometry::canonicalize_legacy_cone_pcurve_endpoints(
            &scan.surfaces.legacy_carriers,
            faces[0],
            face_0_endpoints,
        ),
        crate::legacy_geometry::canonicalize_legacy_cone_pcurve_endpoints(
            &scan.surfaces.legacy_carriers,
            faces[1],
            face_1_endpoints,
        ),
    ]
}

#[allow(dead_code)] // Kept as a focused endpoint-mapping test helper.
pub fn mapped_pcurve_endpoints(
    ir: &CadIr,
    faces: [u32; 2],
    endpoint_sets: [[[f64; 2]; 2]; 2],
) -> Option<[[f64; 3]; 2]> {
    mapped_pcurve_endpoint_evidence(ir, faces, endpoint_sets).map(|evidence| evidence.points)
}

#[derive(Debug, Clone, Copy)]
pub struct PcurveEndpointEvidence {
    pub points: [[f64; 3]; 2],
    pub complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PcurveMismatchDetail {
    pub curve_id: u32,
    pub faces: [u32; 2],
    pub same_order_error: f64,
    pub reverse_order_error: f64,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct PcurveEndpointDiagnostics {
    pub records: usize,
    pub paths: usize,
    pub inactive_paths: usize,
    pub inactive_records: usize,
    pub partial_records: usize,
    pub topology_mismatch_records: usize,
    pub missing_surfaces: usize,
    pub unevaluable_paths: usize,
    pub mapped_paths: usize,
    pub unmapped_records: usize,
    pub inconsistent_records: usize,
    pub accepted_records: usize,
    pub complete_records: usize,
    pub conflicting_curves: usize,
    pub evidence: usize,
    pub complete_evidence: usize,
    pub mismatch_samples: Vec<PcurveMismatchDetail>,
}

#[derive(Debug, Default)]
struct PcurvePathActivity {
    active_paths: BTreeSet<(u32, u32)>,
    topology_faces: BTreeMap<u32, [u32; 2]>,
    prototype_faces: BTreeMap<u32, [u32; 2]>,
}

impl PcurvePathActivity {
    fn from_scan(scan: &ContainerScan) -> Self {
        let active_paths = scan
            .topology
            .loops
            .iter()
            .flat_map(|loop_| {
                loop_
                    .half_edges
                    .iter()
                    .map(move |half_edge| (loop_.face_id, half_edge.curve_id))
            })
            .collect();
        let topology_faces = crate::topology::uniquely_identified_rows(&scan.curves.topology_rows)
            .into_iter()
            .map(|row| (row.id, row.faces))
            .collect();
        let mut prototype_counts = BTreeMap::<u32, usize>::new();
        for row in &scan.curves.prototype_topology {
            *prototype_counts.entry(row.curve_id).or_default() += 1;
        }
        let prototype_faces = scan
            .curves
            .prototype_topology
            .iter()
            .filter(|row| prototype_counts.get(&row.curve_id) == Some(&1))
            .map(|row| (row.curve_id, row.faces))
            .collect();
        Self {
            active_paths,
            topology_faces,
            prototype_faces,
        }
    }

    fn selected_paths(&self, curve_id: u32, faces: [u32; 2], prototype: bool) -> Option<[bool; 2]> {
        let topology_faces = if prototype {
            &self.prototype_faces
        } else {
            &self.topology_faces
        };
        (topology_faces.get(&curve_id) == Some(&faces)).then_some([
            self.active_paths.contains(&(faces[0], curve_id)),
            self.active_paths.contains(&(faces[1], curve_id)),
        ])
    }
}

#[derive(Debug, Clone, Copy)]
struct MappedPcurvePath {
    face_id: u32,
    endpoints: [[f64; 3]; 2],
}

struct MappedPcurvePaths {
    mapped: Vec<MappedPcurvePath>,
    paths: usize,
    missing_surfaces: usize,
    unevaluable_paths: usize,
}

fn map_pcurve_paths(
    ir: &CadIr,
    paths: impl IntoIterator<Item = (u32, [[f64; 2]; 2])>,
) -> MappedPcurvePaths {
    let mut result = MappedPcurvePaths {
        mapped: Vec::new(),
        paths: 0,
        missing_surfaces: 0,
        unevaluable_paths: 0,
    };
    for (face_id, endpoints) in paths {
        result.paths += 1;
        let Some(surface) = unique_model_surface(&ir.model.surfaces, face_id) else {
            result.missing_surfaces += 1;
            continue;
        };
        let [first, second] = endpoints.map(|uv| {
            cadmpeg_ir::eval::surface_point(&surface.geometry, uv[0], uv[1])
                .map(|point| [point.x, point.y, point.z])
        });
        let [Some(first), Some(second)] = [first, second] else {
            result.unevaluable_paths += 1;
            continue;
        };
        result.mapped.push(MappedPcurvePath {
            face_id,
            endpoints: [first, second],
        });
    }
    result
}

fn pcurve_endpoint_evidence_from_mapped(
    mapped: &[MappedPcurvePath],
) -> Option<PcurveEndpointEvidence> {
    let first = mapped.first()?.endpoints;
    mapped
        .iter()
        .all(|candidate| {
            model_points_agree(first[0], candidate.endpoints[0])
                && model_points_agree(first[1], candidate.endpoints[1])
        })
        .then_some(PcurveEndpointEvidence {
            points: first,
            complete: mapped.len() == 2,
        })
}

fn point_coordinate_error(first: [f64; 3], second: [f64; 3]) -> f64 {
    first
        .into_iter()
        .zip(second)
        .map(|(first, second)| {
            let error = (first - second).abs();
            if error.is_finite() {
                error
            } else {
                f64::INFINITY
            }
        })
        .fold(0.0, f64::max)
}

fn endpoint_pair_error(first: [[f64; 3]; 2], second: [[f64; 3]; 2], reverse: bool) -> f64 {
    if reverse {
        point_coordinate_error(first[0], second[1]).max(point_coordinate_error(first[1], second[0]))
    } else {
        point_coordinate_error(first[0], second[0]).max(point_coordinate_error(first[1], second[1]))
    }
}

fn pcurve_mismatch_detail(
    curve_id: u32,
    mapped: &[MappedPcurvePath],
) -> Option<PcurveMismatchDetail> {
    let first = mapped.first()?.endpoints;
    let second = mapped.get(1)?;
    (mapped.len() > 1).then(|| PcurveMismatchDetail {
        curve_id,
        faces: [mapped[0].face_id, second.face_id],
        same_order_error: mapped
            .iter()
            .skip(1)
            .map(|candidate| endpoint_pair_error(first, candidate.endpoints, false))
            .fold(0.0, f64::max),
        reverse_order_error: mapped
            .iter()
            .skip(1)
            .map(|candidate| endpoint_pair_error(first, candidate.endpoints, true))
            .fold(0.0, f64::max),
    })
}

fn mapped_pcurve_endpoint_evidence(
    ir: &CadIr,
    faces: [u32; 2],
    endpoint_sets: [[[f64; 2]; 2]; 2],
) -> Option<PcurveEndpointEvidence> {
    mapped_pcurve_endpoint_evidence_for_paths(ir, faces.into_iter().zip(endpoint_sets))
}

fn mapped_pcurve_endpoint_evidence_for_paths(
    ir: &CadIr,
    paths: impl IntoIterator<Item = (u32, [[f64; 2]; 2])>,
) -> Option<PcurveEndpointEvidence> {
    let mapped = map_pcurve_paths(ir, paths);
    pcurve_endpoint_evidence_from_mapped(&mapped.mapped)
}

pub fn pcurve_edge_endpoint_evidence(
    scan: &ContainerScan,
    ir: &CadIr,
) -> BTreeMap<u32, PcurveEndpointEvidence> {
    pcurve_edge_endpoint_evidence_with_diagnostics(scan, ir).0
}

pub fn pcurve_edge_endpoint_evidence_with_diagnostics(
    scan: &ContainerScan,
    ir: &CadIr,
) -> (
    BTreeMap<u32, PcurveEndpointEvidence>,
    PcurveEndpointDiagnostics,
) {
    let ignored_surface_ids =
        topology_ignored_surface_ids(scan.framing.layout, &scan.surfaces.rows);
    let path_activity = PcurvePathActivity::from_scan(scan);
    let mut candidates = BTreeMap::<u32, Vec<PcurveEndpointEvidence>>::new();
    let mut diagnostics = PcurveEndpointDiagnostics::default();
    for (curve_id, faces, first, second, prototype) in scan
        .curves
        .pcurves
        .iter()
        .map(|pcurve| {
            let [first, second] = canonicalized_pcurve_endpoints(
                scan,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            );
            (pcurve.curve_id, pcurve.faces, first, second, false)
        })
        .chain(scan.curves.bound_prototype_pcurves.iter().map(|pcurve| {
            let [first, second] = canonicalized_pcurve_endpoints(
                scan,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            );
            (pcurve.curve_id, pcurve.faces, first, second, true)
        }))
    {
        diagnostics.records += 1;
        if let Some(active) = path_activity.selected_paths(curve_id, faces, prototype) {
            let active_count = active.into_iter().filter(|is_active| *is_active).count();
            diagnostics.inactive_paths += 2 - active_count;
            match active_count {
                0 => diagnostics.inactive_records += 1,
                1 => diagnostics.partial_records += 1,
                _ => {}
            }
        } else {
            diagnostics.topology_mismatch_records += 1;
        }
        let paths = faces
            .into_iter()
            .zip([first, second])
            .filter(|(face_id, _)| !ignored_surface_ids.contains(face_id));
        let mapped = map_pcurve_paths(ir, paths);
        diagnostics.paths += mapped.paths;
        diagnostics.missing_surfaces += mapped.missing_surfaces;
        diagnostics.unevaluable_paths += mapped.unevaluable_paths;
        diagnostics.mapped_paths += mapped.mapped.len();
        match pcurve_endpoint_evidence_from_mapped(&mapped.mapped) {
            Some(evidence) => {
                diagnostics.accepted_records += 1;
                diagnostics.complete_records += usize::from(evidence.complete);
                candidates.entry(curve_id).or_default().push(evidence);
            }
            None if mapped.mapped.is_empty() => diagnostics.unmapped_records += 1,
            None => {
                diagnostics.inconsistent_records += 1;
                if diagnostics.mismatch_samples.len() < PCURVE_MISMATCH_SAMPLE_LIMIT {
                    if let Some(detail) = pcurve_mismatch_detail(curve_id, &mapped.mapped) {
                        diagnostics.mismatch_samples.push(detail);
                    }
                }
            }
        }
    }
    let mut evidence = BTreeMap::new();
    for (curve_id, candidates) in candidates {
        let Some(first) = candidates.first().copied() else {
            continue;
        };
        if candidates.iter().all(|candidate| {
            model_points_agree(first.points[0], candidate.points[0])
                && model_points_agree(first.points[1], candidate.points[1])
        }) {
            evidence.insert(
                curve_id,
                PcurveEndpointEvidence {
                    points: first.points,
                    complete: candidates.iter().any(|candidate| candidate.complete),
                },
            );
        } else {
            diagnostics.conflicting_curves += 1;
        }
    }
    diagnostics.evidence = evidence.len();
    diagnostics.complete_evidence = evidence.values().filter(|value| value.complete).count();
    (evidence, diagnostics)
}

pub fn pcurve_edge_endpoints(scan: &ContainerScan, ir: &CadIr) -> BTreeMap<u32, [[f64; 3]; 2]> {
    pcurve_edge_endpoint_evidence(scan, ir)
        .into_iter()
        .map(|(curve_id, evidence)| (curve_id, evidence.points))
        .collect()
}

pub fn linear_pcurve_carrier(
    surface: &SurfaceGeometry,
    endpoints: [[f64; 2]; 2],
) -> Option<CurveGeometry> {
    let scaled_vector = |vector: Vector3, scale: f64| {
        Vector3::new(vector.x * scale, vector.y * scale, vector.z * scale)
    };
    let offset_point = |point: Point3, vector: Vector3, scale: f64| {
        Point3::new(
            point.x + vector.x * scale,
            point.y + vector.y * scale,
            point.z + vector.z * scale,
        )
    };
    let [start, end] = endpoints;
    if start == end {
        return None;
    }
    match surface {
        SurfaceGeometry::Plane { .. } => {
            let [first, second] = endpoints.map(|uv| {
                cadmpeg_ir::eval::surface_point(surface, uv[0], uv[1])
                    .map(|point| [point.x, point.y, point.z])
            });
            let [first, second] = [first?, second?];
            let direction = normalized(std::array::from_fn(|axis| second[axis] - first[axis]))?;
            Some(CurveGeometry::Line {
                origin: Point3::new(first[0], first[1], first[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            })
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } if start[0] == end[0] => {
            let transverse = cross(
                [axis.x, axis.y, axis.z],
                [ref_direction.x, ref_direction.y, ref_direction.z],
            );
            let reference = [ref_direction.x, ref_direction.y, ref_direction.z];
            let radial: [f64; 3] = std::array::from_fn(|coordinate| {
                start[0].cos() * reference[coordinate] + start[0].sin() * transverse[coordinate]
            });
            let point = [
                origin.x + radius * radial[0] + start[1] * axis.x,
                origin.y + radius * radial[1] + start[1] * axis.y,
                origin.z + radius * radial[2] + start[1] * axis.z,
            ];
            let direction = normalized([axis.x, axis.y, axis.z])?;
            Some(CurveGeometry::Line {
                origin: Point3::new(point[0], point[1], point[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            })
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } if start[1] == end[1] && radius.is_finite() && *radius > 0.0 => {
            Some(CurveGeometry::Circle {
                center: offset_point(*origin, *axis, start[1]),
                axis: *axis,
                ref_direction: *ref_direction,
                radius: *radius,
            })
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            ratio,
            half_angle,
            ..
        } if start[0] == end[0] => {
            let [first, second] = endpoints.map(|uv| {
                cadmpeg_ir::eval::surface_point(surface, uv[0], uv[1])
                    .map(|point| [point.x, point.y, point.z])
            });
            let [first, second] = [first?, second?];
            let direction = normalized(std::array::from_fn(|axis| second[axis] - first[axis]))?;
            (ratio.is_finite() && *ratio > 0.0 && half_angle.is_finite()).then_some(())?;
            Some(CurveGeometry::Line {
                origin: Point3::new(first[0], first[1], first[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            })
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } if start[1] == end[1] && ratio.is_finite() && *ratio > 0.0 => {
            let local_radius = radius + start[1] * half_angle.tan();
            let first_radius = local_radius.abs();
            let second_radius = (local_radius * ratio).abs();
            if !first_radius.is_finite() || !second_radius.is_finite() {
                return None;
            }
            let center = offset_point(*origin, *axis, start[1]);
            if (first_radius - second_radius).abs()
                <= EPS_NEAR_ZERO * first_radius.max(second_radius).max(1.0)
            {
                (first_radius > 0.0).then_some(CurveGeometry::Circle {
                    center,
                    axis: *axis,
                    ref_direction: scaled_vector(*ref_direction, local_radius.signum()),
                    radius: first_radius,
                })
            } else {
                let transverse = cross(
                    [axis.x, axis.y, axis.z],
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                );
                let transverse = Vector3::new(transverse[0], transverse[1], transverse[2]);
                let (major_direction, major_radius, minor_radius) = if first_radius > second_radius
                {
                    (
                        scaled_vector(*ref_direction, local_radius.signum()),
                        first_radius,
                        second_radius,
                    )
                } else {
                    (
                        scaled_vector(transverse, (local_radius * ratio).signum()),
                        second_radius,
                        first_radius,
                    )
                };
                (minor_radius > 0.0).then_some(CurveGeometry::Ellipse {
                    center,
                    axis: *axis,
                    major_direction,
                    major_radius,
                    minor_radius,
                })
            }
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } if start[1] == end[1] && radius.is_finite() && *radius > 0.0 => {
            let ring = radius * start[1].cos();
            (ring.abs() > 0.0).then_some(CurveGeometry::Circle {
                center: offset_point(*center, *axis, radius * start[1].sin()),
                axis: *axis,
                ref_direction: scaled_vector(*ref_direction, ring.signum()),
                radius: ring.abs(),
            })
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } if start[0] == end[0] && radius.is_finite() && *radius > 0.0 => {
            let transverse = cross(
                [axis.x, axis.y, axis.z],
                [ref_direction.x, ref_direction.y, ref_direction.z],
            );
            let radial = Vector3::new(
                start[0].cos() * ref_direction.x + start[0].sin() * transverse[0],
                start[0].cos() * ref_direction.y + start[0].sin() * transverse[1],
                start[0].cos() * ref_direction.z + start[0].sin() * transverse[2],
            );
            let normal = cross([radial.x, radial.y, radial.z], [axis.x, axis.y, axis.z]);
            Some(CurveGeometry::Circle {
                center: *center,
                axis: Vector3::new(normal[0], normal[1], normal[2]),
                ref_direction: radial,
                radius: *radius,
            })
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if start[1] == end[1]
            && major_radius.is_finite()
            && minor_radius.is_finite()
            && *minor_radius > 0.0 =>
        {
            let ring = major_radius + minor_radius * start[1].cos();
            (ring.abs() > 0.0).then_some(CurveGeometry::Circle {
                center: offset_point(*center, *axis, minor_radius * start[1].sin()),
                axis: *axis,
                ref_direction: scaled_vector(*ref_direction, ring.signum()),
                radius: ring.abs(),
            })
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if start[0] == end[0]
            && major_radius.is_finite()
            && minor_radius.is_finite()
            && *minor_radius > 0.0 =>
        {
            let transverse = cross(
                [axis.x, axis.y, axis.z],
                [ref_direction.x, ref_direction.y, ref_direction.z],
            );
            let radial = Vector3::new(
                start[0].cos() * ref_direction.x + start[0].sin() * transverse[0],
                start[0].cos() * ref_direction.y + start[0].sin() * transverse[1],
                start[0].cos() * ref_direction.z + start[0].sin() * transverse[2],
            );
            let normal = cross([radial.x, radial.y, radial.z], [axis.x, axis.y, axis.z]);
            Some(CurveGeometry::Circle {
                center: offset_point(*center, radial, *major_radius),
                axis: Vector3::new(normal[0], normal[1], normal[2]),
                ref_direction: radial,
                radius: *minor_radius,
            })
        }
        _ => None,
    }
}

pub fn transfer_analytic_pcurve_carriers(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> BTreeSet<CurveId> {
    let reconciled_endpoints = pcurve_edge_endpoints(scan, ir);
    let ignored_surface_ids =
        topology_ignored_surface_ids(scan.framing.layout, &scan.surfaces.rows);
    let mut candidates = BTreeMap::<u32, Vec<(CurveGeometry, usize)>>::new();
    let mut evaluable_path_counts = BTreeMap::<u32, usize>::new();
    for (curve_id, faces, endpoint_sets, offset) in scan
        .curves
        .pcurves
        .iter()
        .map(|pcurve| {
            let endpoint_sets = canonicalized_pcurve_endpoints(
                scan,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            );
            (pcurve.curve_id, pcurve.faces, endpoint_sets, pcurve.offset)
        })
        .chain(scan.curves.bound_prototype_pcurves.iter().map(|pcurve| {
            let endpoint_sets = canonicalized_pcurve_endpoints(
                scan,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
            );
            (pcurve.curve_id, pcurve.faces, endpoint_sets, pcurve.offset)
        }))
    {
        for (face_id, endpoints) in faces.into_iter().zip(endpoint_sets) {
            if ignored_surface_ids.contains(&face_id) {
                continue;
            }
            let Some(surface) = unique_model_surface(&ir.model.surfaces, face_id) else {
                continue;
            };
            if endpoints.iter().all(|uv| {
                cadmpeg_ir::eval::surface_point(&surface.geometry, uv[0], uv[1]).is_some()
            }) {
                *evaluable_path_counts.entry(curve_id).or_default() += 1;
            }
            if let Some(carrier) = linear_pcurve_carrier(&surface.geometry, endpoints) {
                candidates
                    .entry(curve_id)
                    .or_default()
                    .push((carrier, offset));
            }
        }
    }
    let mut transferred = BTreeSet::new();
    for (curve_id, candidates) in candidates {
        if evaluable_path_counts.get(&curve_id).copied() != Some(candidates.len()) {
            continue;
        }
        let Some(points) = reconciled_endpoints.get(&curve_id).copied() else {
            continue;
        };
        let Some((geometry, offset)) = candidates.first() else {
            continue;
        };
        if !curve_contains_points(geometry, points)
            || !candidates.iter().all(|(candidate, _)| {
                curve_contains_points(candidate, points)
                    && [0.0, 0.25, 0.5, 0.75, 1.0].into_iter().all(|parameter| {
                        let point = cadmpeg_ir::eval::curve_point(candidate, parameter);
                        point.is_some_and(|point| {
                            curve_contains_points(geometry, [[point.x, point.y, point.z]; 2])
                        })
                    })
            })
        {
            continue;
        }
        let offset = candidates
            .iter()
            .map(|(_, offset)| *offset)
            .min()
            .unwrap_or(*offset);
        let id = CurveId(format!("creo:visibgeom:curve#{curve_id}"));
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            offset as u64,
            "analytic_pcurve_carrier",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry: geometry.clone(),
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{curve_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred.insert(id);
    }
    transferred
}

pub type PcurveVertexConstraint = ([u32; 2], [[f64; 3]; 2]);

pub fn directed_pcurve_points(directions: [u8; 2], points: [[f64; 3]; 2]) -> Option<[[f64; 3]; 2]> {
    match directions {
        [0x01, 0xf6] => Some(points),
        [0xf6, 0x01] => Some([points[1], points[0]]),
        _ => None,
    }
}

pub fn solve_pcurve_vertex_domains(
    constraints: &[PcurveVertexConstraint],
    fixed_points: &BTreeMap<u32, Option<[f64; 3]>>,
    analytic_domains: &BTreeMap<u32, Vec<[f64; 3]>>,
    incident_curves: &BTreeMap<u32, Vec<&CurveGeometry>>,
) -> BTreeMap<u32, [f64; 3]> {
    let mut domains = BTreeMap::<u32, Vec<[f64; 3]>>::new();
    for (vertices, points) in constraints {
        if vertices[0] == vertices[1] {
            match domains.entry(vertices[0]) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(if model_points_agree(points[0], points[1]) {
                        vec![points[0]]
                    } else {
                        Vec::new()
                    });
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let domain = entry.get_mut();
                    if model_points_agree(points[0], points[1]) {
                        domain.retain(|candidate| model_points_agree(*candidate, points[0]));
                    } else {
                        domain.clear();
                    }
                }
            }
            continue;
        }
        for vertex in vertices {
            let domain = domains.entry(*vertex).or_insert_with(|| points.to_vec());
            domain.retain(|candidate| {
                points
                    .iter()
                    .any(|point| model_points_agree(*candidate, *point))
            });
        }
    }
    for (vertex, candidates) in analytic_domains {
        match domains.entry(*vertex) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(candidates.clone());
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.get_mut().retain(|point| {
                    candidates
                        .iter()
                        .any(|candidate| model_points_agree(*point, *candidate))
                });
            }
        }
    }
    for (vertex, point) in fixed_points {
        match domains.entry(*vertex) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(point.iter().copied().collect());
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if let Some(point) = point {
                    entry
                        .get_mut()
                        .retain(|candidate| model_points_agree(*candidate, *point));
                } else {
                    entry.get_mut().clear();
                }
            }
        }
    }
    for (vertex, curves) in incident_curves {
        if let Some(domain) = domains.get_mut(vertex) {
            domain.retain(|candidate| {
                curves
                    .iter()
                    .all(|curve| curve_contains_points(curve, [*candidate, *candidate]))
            });
        }
    }
    let compatible = |first: [f64; 3], second: [f64; 3], points: [[f64; 3]; 2]| {
        (model_points_agree(first, points[0]) && model_points_agree(second, points[1]))
            || (model_points_agree(first, points[1]) && model_points_agree(second, points[0]))
    };
    loop {
        let mut changed = false;
        for (vertices, points) in constraints {
            if vertices[0] == vertices[1] {
                continue;
            }
            let first = domains.get(&vertices[0]).cloned().unwrap_or_default();
            let second = domains.get(&vertices[1]).cloned().unwrap_or_default();
            let retained_first = first
                .iter()
                .copied()
                .filter(|first| {
                    second
                        .iter()
                        .any(|second| compatible(*first, *second, *points))
                })
                .collect::<Vec<_>>();
            let retained_second = second
                .iter()
                .copied()
                .filter(|second| {
                    first
                        .iter()
                        .any(|first| compatible(*first, *second, *points))
                })
                .collect::<Vec<_>>();
            changed |= retained_first.len() != first.len() || retained_second.len() != second.len();
            domains.insert(vertices[0], retained_first);
            domains.insert(vertices[1], retained_second);
        }
        if !changed {
            break;
        }
    }
    domains
        .into_iter()
        .filter_map(|(vertex, mut domain)| {
            domain.dedup_by(|first, second| model_points_agree(*first, *second));
            let [point] = domain.as_slice() else {
                return None;
            };
            Some((vertex, *point))
        })
        .collect()
}

pub fn native_pcurve_midpoint(
    surface: &SurfaceGeometry,
    endpoints: [[f64; 2]; 2],
    edge_points: [[f64; 3]; 2],
) -> Option<[f64; 3]> {
    let mapped = endpoints.map(|uv| {
        cadmpeg_ir::eval::surface_point(surface, uv[0], uv[1])
            .map(|point| [point.x, point.y, point.z])
    });
    let [Some(first), Some(second)] = mapped else {
        return None;
    };
    point_pair_alignments([first, second], edge_points)
        .into_iter()
        .any(|matches| matches)
        .then_some(())?;
    let uv = [
        f64::midpoint(endpoints[0][0], endpoints[1][0]),
        f64::midpoint(endpoints[0][1], endpoints[1][1]),
    ];
    cadmpeg_ir::eval::surface_point(surface, uv[0], uv[1]).map(|point| [point.x, point.y, point.z])
}

pub type NativePcurveCandidates = BTreeMap<(u32, u32), Vec<([[f64; 2]; 2], usize)>>;

pub fn pcurve_backed_periodic_conic_parameter_range(
    geometry: &CurveGeometry,
    curve_id: u32,
    faces: [u32; 2],
    candidates: &NativePcurveCandidates,
    surfaces: &[Surface],
    points: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    let mut selected = None;
    for face_id in faces {
        let Some(surface) =
            unique_model_surface(surfaces, face_id).map(|surface| &surface.geometry)
        else {
            continue;
        };
        for (endpoints, _) in candidates.get(&(curve_id, face_id)).into_iter().flatten() {
            let Some(interior) = native_pcurve_midpoint(surface, *endpoints, points) else {
                continue;
            };
            let candidate = periodic_conic_edge_parameter_range(geometry, points, interior)?;
            if selected.is_some_and(|selected: [f64; 2]| {
                candidate
                    .into_iter()
                    .zip(selected)
                    .any(|(candidate, selected)| (candidate - selected).abs() > EPS_AGREE)
            }) {
                return None;
            }
            selected = Some(candidate);
        }
    }
    selected
}

pub fn oriented_native_pcurve_endpoints(
    surface: &SurfaceGeometry,
    endpoints: [[f64; 2]; 2],
    traversal: [[f64; 3]; 2],
) -> Option<[[f64; 2]; 2]> {
    let mapped = endpoints.map(|uv| {
        cadmpeg_ir::eval::surface_point(surface, uv[0], uv[1])
            .map(|point| [point.x, point.y, point.z])
    });
    let [Some(first), Some(second)] = mapped else {
        return None;
    };
    match point_pair_alignments([first, second], traversal) {
        [true, false] => Some(endpoints),
        [false, true] => Some([endpoints[1], endpoints[0]]),
        _ => None,
    }
}

pub fn unique_oriented_native_pcurve(
    surface: &SurfaceGeometry,
    candidates: &[([[f64; 2]; 2], usize)],
    traversal: [[f64; 3]; 2],
) -> Option<([[f64; 2]; 2], usize)> {
    let mut matching = candidates.iter().filter_map(|(endpoints, offset)| {
        oriented_native_pcurve_endpoints(surface, *endpoints, traversal)
            .map(|oriented| (oriented, *offset))
    });
    let mut selected = matching.next()?;
    for candidate in matching {
        if candidate.0 != selected.0 {
            return None;
        }
        selected.1 = selected.1.min(candidate.1);
    }
    Some(selected)
}

pub fn planar_curve_pcurve(
    surface: &SurfaceGeometry,
    geometry: &CurveGeometry,
) -> Option<PcurveGeometry> {
    let SurfaceGeometry::Plane {
        origin,
        normal,
        u_axis,
    } = surface
    else {
        return None;
    };
    let origin = [origin.x, origin.y, origin.z];
    let normal = normalized([normal.x, normal.y, normal.z])?;
    let u_axis = normalized([u_axis.x, u_axis.y, u_axis.z])?;
    (dot(normal, u_axis).abs() <= EPS_ORTHO).then_some(())?;
    let v_axis = normalized(cross(normal, u_axis))?;
    let project_point = |point: [f64; 3], tolerance: f64| {
        let relative: [f64; 3] = std::array::from_fn(|index| point[index] - origin[index]);
        (dot(relative, normal).abs() <= tolerance)
            .then_some(Point2::new(dot(relative, u_axis), dot(relative, v_axis)))
    };
    let project_direction = |direction: [f64; 3]| {
        let length = dot(direction, direction).sqrt();
        (length.is_finite() && length > 0.0 && dot(direction, normal).abs() <= EPS_ORTHO * length)
            .then_some(Point2::new(dot(direction, u_axis), dot(direction, v_axis)))
    };
    let conic_frame = |center: [f64; 3], axis: [f64; 3], x_axis: [f64; 3], scale: f64| {
        let axis = normalized(axis)?;
        let x_axis = normalized(x_axis)?;
        ((dot(axis, normal).abs() - 1.0).abs() <= EPS_ORTHO
            && dot(axis, x_axis).abs() <= EPS_ORTHO)
            .then_some(())?;
        let y_axis = normalized(cross(axis, x_axis))?;
        Some((
            project_point(center, EPS_AGREE * scale.max(1.0))?,
            project_direction(x_axis)?,
            project_direction(y_axis)?,
        ))
    };

    match geometry {
        CurveGeometry::Line { origin, direction } => {
            let direction = [direction.x, direction.y, direction.z];
            Some(PcurveGeometry::Line {
                origin: project_point([origin.x, origin.y, origin.z], EPS_AGREE)?,
                direction: project_direction(direction)?,
            })
        }
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } if radius.is_finite() && *radius > 0.0 => {
            let (center, x_axis, y_axis) = conic_frame(
                [center.x, center.y, center.z],
                [axis.x, axis.y, axis.z],
                [ref_direction.x, ref_direction.y, ref_direction.z],
                *radius,
            )?;
            Some(PcurveGeometry::Circle {
                center,
                x_axis,
                y_axis,
                radius: *radius,
            })
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } if major_radius.is_finite()
            && minor_radius.is_finite()
            && *major_radius > 0.0
            && *minor_radius > 0.0 =>
        {
            let (center, x_axis, y_axis) = conic_frame(
                [center.x, center.y, center.z],
                [axis.x, axis.y, axis.z],
                [major_direction.x, major_direction.y, major_direction.z],
                major_radius.max(*minor_radius),
            )?;
            Some(PcurveGeometry::Ellipse {
                center,
                x_axis,
                y_axis,
                major_radius: *major_radius,
                minor_radius: *minor_radius,
            })
        }
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } if focal_distance.is_finite() && *focal_distance > 0.0 => {
            let (vertex, x_axis, y_axis) = conic_frame(
                [vertex.x, vertex.y, vertex.z],
                [axis.x, axis.y, axis.z],
                [major_direction.x, major_direction.y, major_direction.z],
                *focal_distance,
            )?;
            Some(PcurveGeometry::Parabola {
                vertex,
                x_axis,
                y_axis,
                focal_distance: *focal_distance,
            })
        }
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } if major_radius.is_finite()
            && minor_radius.is_finite()
            && *major_radius > 0.0
            && *minor_radius > 0.0 =>
        {
            let (center, x_axis, y_axis) = conic_frame(
                [center.x, center.y, center.z],
                [axis.x, axis.y, axis.z],
                [major_direction.x, major_direction.y, major_direction.z],
                major_radius.max(*minor_radius),
            )?;
            Some(PcurveGeometry::Hyperbola {
                center,
                x_axis,
                y_axis,
                major_radius: *major_radius,
                minor_radius: *minor_radius,
            })
        }
        CurveGeometry::Nurbs(nurbs) => {
            nurbs_intrinsic_parameter_range(nurbs)?;
            nurbs
                .weights
                .as_ref()
                .is_none_or(|weights| weights.iter().all(|weight| weight.is_finite()))
                .then_some(())?;
            let tolerance = EPS_AGREE * nurbs_control_extent(nurbs)?;
            let control_points = nurbs
                .control_points
                .iter()
                .map(|point| project_point([point.x, point.y, point.z], tolerance))
                .collect::<Option<Vec<_>>>()?;
            Some(PcurveGeometry::Nurbs {
                degree: nurbs.degree,
                knots: nurbs.knots.clone(),
                control_points,
                weights: nurbs.weights.clone(),
                periodic: nurbs.periodic,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::units::Units;
    use std::collections::BTreeSet;

    #[test]
    fn legacy_spline_topology_filter_is_layout_scoped() {
        let rows = vec![
            crate::surface::SurfaceRow {
                id: 7,
                type_byte: crate::surface::SurfaceKind::Spline.canonical_type_byte(),
                kind: crate::surface::SurfaceKind::Spline,
                feature_id: 0,
                reversed: false,
                boundary_type: 0,
                next_surface: 0,
                offset: 0,
            },
            crate::surface::SurfaceRow {
                id: 8,
                type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
                kind: crate::surface::SurfaceKind::Plane,
                feature_id: 0,
                reversed: false,
                boundary_type: 0,
                next_surface: 0,
                offset: 0,
            },
        ];

        assert_eq!(
            topology_ignored_surface_ids(crate::container::Layout::LegacyAscii, &rows),
            BTreeSet::from([7]),
        );
        assert!(topology_ignored_surface_ids(crate::container::Layout::Nd, &rows).is_empty());
    }

    #[test]
    fn mapped_pcurve_endpoints_reject_duplicate_face_surfaces() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.surfaces.extend([
            Surface {
                id: SurfaceId("creo:visibgeom:surface#7".to_string()),
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: Vector3::new(0.0, 0.0, 1.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
                source_object: None,
            },
            Surface {
                id: SurfaceId("creo:visibgeom:surface#7".to_string()),
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 1.0),
                    normal: Vector3::new(0.0, 0.0, 1.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
                source_object: None,
            },
        ]);

        assert!(mapped_pcurve_endpoints(
            &ir,
            [7, 7],
            [[[0.0, 0.0], [1.0, 0.0]], [[0.0, 0.0], [1.0, 0.0]]],
        )
        .is_none());
    }

    #[test]
    fn pcurve_mismatch_diagnostics_measure_both_endpoint_orders() {
        const EPS_TEST_ERROR: f64 = 1e-12;
        let mapped = vec![
            MappedPcurvePath {
                face_id: 7,
                endpoints: [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
            },
            MappedPcurvePath {
                face_id: 8,
                endpoints: [[1.0, 0.0, 0.0], [3.0, 0.0, 0.0]],
            },
        ];

        let detail = pcurve_mismatch_detail(11, &mapped).expect("two mapped paths");
        assert_eq!(detail.curve_id, 11);
        assert_eq!(detail.faces, [7, 8]);
        assert!((detail.same_order_error - 1.0).abs() < EPS_TEST_ERROR);
        assert!((detail.reverse_order_error - 3.0).abs() < EPS_TEST_ERROR);
    }

    #[test]
    fn pcurve_diagnostics_count_inactive_face_paths() {
        let mut scan = crate::container::scan_bytes(Vec::new());
        scan.curves
            .topology_rows
            .push(crate::curve::CurveTopologyRow {
                id: 7,
                type_byte: 0,
                feature_id: 0,
                directions: [0x01, 0xf6],
                faces: [10, 11],
                next_edges: [7, 7],
                offset: 0,
            });
        scan.curves.pcurves.push(crate::curve::PcurveEndpoints {
            curve_id: 7,
            faces: [10, 11],
            face_0_endpoints: [[1.0, 2.0], [3.0, 4.0]],
            face_1_endpoints: [[1.0, 2.0], [3.0, 4.0]],
            offset: 0,
        });
        scan.topology.loops.push(crate::topology::Loop {
            face_id: 10,
            half_edges: vec![crate::topology::HalfEdgeId {
                curve_id: 7,
                side: 0,
            }],
        });
        let mut ir = CadIr::empty(Units::default());
        ir.model.surfaces.extend([
            Surface {
                id: SurfaceId("creo:visibgeom:surface#10".to_string()),
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: Vector3::new(0.0, 0.0, 1.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
                source_object: None,
            },
            Surface {
                id: SurfaceId("creo:visibgeom:surface#11".to_string()),
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: Vector3::new(0.0, 0.0, 1.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
                source_object: None,
            },
        ]);

        let (evidence, diagnostics) = pcurve_edge_endpoint_evidence_with_diagnostics(&scan, &ir);
        assert_eq!(
            evidence.get(&7).map(|value| (value.points, value.complete)),
            Some(([[1.0, 2.0, 0.0], [3.0, 4.0, 0.0]], true))
        );
        assert_eq!(diagnostics.records, 1);
        assert_eq!(diagnostics.paths, 2);
        assert_eq!(diagnostics.inactive_paths, 1);
        assert_eq!(diagnostics.inactive_records, 0);
        assert_eq!(diagnostics.partial_records, 1);
        assert_eq!(diagnostics.topology_mismatch_records, 0);
        assert_eq!(diagnostics.accepted_records, 1);
        assert_eq!(diagnostics.complete_records, 1);
    }
}
