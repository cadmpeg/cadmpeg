// SPDX-License-Identifier: Apache-2.0
//! Placed carriers, topology-bound plane transfer, and face orientations.

use cadmpeg_core::container::ContainerRole;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, SurfaceId, UnknownId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;
use crate::legacy_geometry::LegacySurfaceNamespace;
use crate::topology::HalfEdgeId;

use super::super::native::annotate;
use super::super::surfaces::{native_surface_id, rowless_round_cylinder_pairs};

use super::super::uniqueness::exactly_one;
use super::equations::{
    CarrierEquation, ConeEquation, CylinderEquation, PlaneEquation, SphereEquation, TorusEquation,
};
use super::planes::{
    agreed_plane, agreed_topology_bound_plane, analytic_boundary_line, analytic_curve_plane,
    placed_planes, topology_bound_plane,
};
use super::vertices::solved_topological_vertices;

const EPS_AGREE: f64 = 1.0e-9;
const EPS_NEAR_ZERO: f64 = 1.0e-12;

fn existing_plane_agrees_with_topology(
    geometry: &SurfaceGeometry,
    topology: PlaneEquation,
) -> Option<bool> {
    match geometry {
        SurfaceGeometry::Plane { origin, normal, .. } => Some(
            agreed_plane(&[
                PlaneEquation {
                    origin: [origin.x, origin.y, origin.z],
                    normal: [normal.x, normal.y, normal.z],
                },
                topology,
            ])
            .is_some(),
        ),
        SurfaceGeometry::Unknown { .. } => None,
        _ => Some(false),
    }
}

pub fn transfer_topology_bound_planes(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    nurbs_endpoint_witnesses: &BTreeSet<CurveId>,
) -> usize {
    let carriers = placed_carriers(scan, ir);
    let solved_vertices =
        solved_topological_vertices(scan, ir, &carriers, nurbs_endpoint_witnesses);
    let vertex_faces =
        crate::topology::vertex_incident_faces(&scan.topology.vertices, &scan.topology.half_edges);
    let unique_rows = crate::surface::uniquely_identified_rows(&scan.surfaces.rows);
    let unique_curve_ids = crate::topology::uniquely_identified_rows(&scan.curves.topology_rows)
        .into_iter()
        .map(|row| row.id)
        .collect::<BTreeSet<_>>();
    let mut transferred = 0;
    for row in unique_rows
        .into_iter()
        .filter(|row| row.kind == crate::surface::SurfaceKind::Plane)
    {
        let id = SurfaceId::mint(format!("creo:visibgeom:surface#{}", row.id))
            .expect("identity grammar");
        let points = solved_vertices
            .iter()
            .filter_map(|(vertex_id, point)| {
                vertex_faces
                    .get(vertex_id)
                    .is_some_and(|faces| faces.contains(&row.id))
                    .then_some(*point)
            })
            .collect::<Vec<_>>();
        let boundary_curves = scan
            .topology
            .loops
            .iter()
            .filter(|lp| lp.face_id == row.id)
            .flat_map(|lp| lp.half_edges.iter())
            .filter_map(|half_edge| {
                unique_curve_ids
                    .contains(&half_edge.curve_id)
                    .then_some(())?;
                let id = CurveId::mint(format!("creo:visibgeom:curve#{}", half_edge.curve_id))
                    .expect("identity grammar");
                let curve = exactly_one(ir.model.curves.iter().filter(|curve| curve.id == id))?;
                Some(&curve.geometry)
            })
            .collect::<Vec<_>>();
        let curve_planes = boundary_curves
            .iter()
            .filter_map(|geometry| analytic_curve_plane(geometry));
        let lines = boundary_curves
            .iter()
            .filter_map(|geometry| analytic_boundary_line(geometry));
        let Some(plane) = agreed_topology_bound_plane(points, curve_planes, lines) else {
            continue;
        };
        let existing_count = ir
            .model
            .surfaces
            .iter()
            .filter(|surface| surface.id == id)
            .count();
        if existing_count != 0 {
            let conflict = existing_count != 1
                || ir
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == id)
                    .is_some_and(|surface| {
                        existing_plane_agrees_with_topology(&surface.geometry, plane) == Some(false)
                    });
            if !conflict {
                continue;
            }
            for surface in ir
                .model
                .surfaces
                .iter_mut()
                .filter(|surface| surface.id == id)
            {
                surface.geometry = SurfaceGeometry::Unknown {
                    record: geometry_section_record(scan, row.offset),
                };
            }
            annotate(
                annotations,
                &id,
                "VisibGeom",
                row.offset as u64,
                if existing_count == 1 {
                    "conflicting_topology_plane_carrier"
                } else {
                    "duplicate_topology_plane_carrier"
                },
                Exactness::Unknown,
            );
            continue;
        }
        let normal = Vector3::new(plane.normal[0], plane.normal[1], plane.normal[2]);
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            "plane_topology_boundary",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(plane.origin[0], plane.origin[1], plane.origin[2]),
                normal,
                u_axis: cadmpeg_ir::geometry::derive_reference_direction(normal),
            },
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::Creo,
                object_id: format!("VisibGeom:{}", row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        transferred += 1;
    }
    transferred
}

pub fn retain_unresolved_surface_carriers(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    for (rows, namespace) in [
        (&scan.surfaces.rows, LegacySurfaceNamespace::Visible),
        (
            &scan.surfaces.nonvisible_rows,
            LegacySurfaceNamespace::NonVisible,
        ),
    ] {
        for row in crate::surface::uniquely_identified_rows(rows) {
            let id = SurfaceId::mint(format!("{}{}", namespace.ir_prefix(), row.id))
                .expect("identity grammar");
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                if namespace.is_visible() {
                    "VisibGeom"
                } else {
                    "NovisGeom"
                },
                row.offset as u64,
                if namespace.is_visible() {
                    "unresolved_visible_surface_carrier"
                } else {
                    "unresolved_nonvisible_surface_carrier"
                },
                Exactness::Unknown,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry: SurfaceGeometry::Unknown {
                    record: geometry_section_record(scan, row.offset),
                },
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: format!("{}{}", namespace.source_prefix(), row.id),
                    name: None,
                    color: None,
                    visible: if namespace.is_visible() {
                        None
                    } else {
                        Some(false)
                    },
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
    }
    for row in crate::topology::uniquely_identified_rows(&scan.curves.topology_rows) {
        let id =
            CurveId::mint(format!("creo:visibgeom:curve#{}", row.id)).expect("identity grammar");
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row.offset as u64,
            "unresolved_visible_curve_carrier",
            Exactness::Unknown,
        );
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Unknown {
                record: geometry_section_record(scan, row.offset),
            },
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::Creo,
                object_id: format!("VisibGeom:{}", row.id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
}

pub fn placed_carriers(scan: &ContainerScan, ir: &CadIr) -> BTreeMap<u32, CarrierEquation> {
    let mut carriers = placed_planes(scan)
        .into_iter()
        .map(|(id, plane)| (id, CarrierEquation::Plane(plane)))
        .collect::<BTreeMap<_, _>>();
    let rows = scan
        .surfaces
        .rows
        .iter()
        .chain(&scan.surfaces.nonvisible_rows)
        .collect::<Vec<_>>();
    let row_ids = rows.iter().map(|row| row.id).collect::<BTreeSet<_>>();
    let mut row_counts = BTreeMap::<u32, usize>::new();
    for row in &rows {
        *row_counts.entry(row.id).or_default() += 1;
    }
    for (namespace_rows, parameters) in [
        (&scan.surfaces.rows, &scan.surfaces.parameters),
        (
            &scan.surfaces.nonvisible_rows,
            &scan.surfaces.nonvisible_parameters,
        ),
    ] {
        for row in namespace_rows
            .iter()
            .filter(|row| row_counts.get(&row.id) == Some(&1))
        {
            if let Some(carrier) = positional_cylinder_carrier(scan, row, parameters, ir) {
                carriers.insert(row.id, carrier);
                continue;
            }
            let id = native_surface_id(scan, row.id);
            let model_surfaces = ir
                .model
                .surfaces
                .iter()
                .filter(|surface| surface.id == id)
                .collect::<Vec<_>>();
            let surface = match model_surfaces.as_slice() {
                [] => continue,
                [surface] => surface,
                _ => {
                    carriers.remove(&row.id);
                    continue;
                }
            };
            if let SurfaceGeometry::Plane { origin, normal, .. } = &surface.geometry {
                let plane = PlaneEquation {
                    origin: [origin.x, origin.y, origin.z],
                    normal: [normal.x, normal.y, normal.z],
                };
                let agreed = match carriers.get(&row.id) {
                    Some(CarrierEquation::Plane(existing)) => agreed_plane(&[*existing, plane]),
                    Some(_) => None,
                    None => Some(plane),
                };
                if let Some(plane) = agreed {
                    carriers.insert(row.id, CarrierEquation::Plane(plane));
                } else {
                    carriers.remove(&row.id);
                }
            } else if let Some(carrier) = surface_carrier(&surface.geometry) {
                carriers.insert(row.id, carrier);
            }
        }
    }
    for datum in &scan.planes.datum_cylinders {
        let id = native_surface_id(scan, datum.id);
        let model_surfaces = ir
            .model
            .surfaces
            .iter()
            .filter(|surface| surface.id == id)
            .collect::<Vec<_>>();
        let Some(surface) = exactly_one(model_surfaces.into_iter()) else {
            carriers.remove(&datum.id);
            continue;
        };
        if let Some(carrier) = surface_carrier(&surface.geometry) {
            carriers.insert(datum.id, carrier);
        } else {
            carriers.remove(&datum.id);
        }
    }
    let mut model_surfaces_by_id = BTreeMap::<u32, Vec<&Surface>>::new();
    for surface in &ir.model.surfaces {
        let Some(id) = surface
            .id
            .0
            .strip_prefix("creo:visibgeom:surface#")
            .or_else(|| surface.id.as_str().strip_prefix("creo:novisgeom:surface#"))
            .and_then(|id| id.parse().ok())
        else {
            continue;
        };
        model_surfaces_by_id.entry(id).or_default().push(surface);
    }
    for (id, model_surfaces) in model_surfaces_by_id {
        if row_ids.contains(&id) {
            continue;
        }
        let Some(surface) = exactly_one(model_surfaces.into_iter()) else {
            carriers.remove(&id);
            continue;
        };
        if let Some(carrier) = surface_carrier(&surface.geometry) {
            carriers.insert(id, carrier);
        }
    }
    carriers
}

fn positional_cylinder_carrier(
    scan: &ContainerScan,
    row: &crate::surface::SurfaceRow,
    parameters: &[crate::surface::SurfaceParameterRecord],
    ir: &CadIr,
) -> Option<CarrierEquation> {
    (row.kind == crate::surface::SurfaceKind::Cylinder).then_some(())?;
    let record = crate::surface::unique_surface_parameter(parameters, row.id)?;
    let inline = record.has_inline_non_plane_envelope()
        || record.has_inline_non_plane_local_system_suffix(row.type_byte)
        || record
            .selector_corner_interval_cylinder_frame(row.type_byte)
            .is_some();
    if crate::decode::sketch_transfer::feature_schema_class(scan, row.feature_id) == Some(913)
        && !inline
    {
        return None;
    }
    if crate::decode::sketch_transfer::feature_schema_class(scan, row.feature_id) == Some(913)
        && inline
    {
        let id = native_surface_id(scan, row.id);
        let model_surfaces = ir
            .model
            .surfaces
            .iter()
            .filter(|surface| surface.id == id)
            .collect::<Vec<_>>();
        if let [surface] = model_surfaces.as_slice() {
            if let Some(carrier) = surface_carrier(&surface.geometry) {
                return Some(carrier);
            }
        }
    }
    let frame = record.positional_cylinder_frame?;
    frame
        .is_valid()
        .then_some(CarrierEquation::Cylinder(CylinderEquation {
            origin: frame.origin,
            axis: frame.axis,
            ref_direction: frame.ref_direction,
            radius: frame.radius,
        }))
}

fn surface_carrier(geometry: &SurfaceGeometry) -> Option<CarrierEquation> {
    match geometry {
        SurfaceGeometry::Plane { origin, normal, .. } => {
            Some(CarrierEquation::Plane(PlaneEquation {
                origin: [origin.x, origin.y, origin.z],
                normal: [normal.x, normal.y, normal.z],
            }))
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => Some(CarrierEquation::Cylinder(CylinderEquation {
            origin: [origin.x, origin.y, origin.z],
            axis: [axis.x, axis.y, axis.z],
            ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
            radius: *radius,
        })),
        SurfaceGeometry::Sphere {
            center,
            axis: _,
            ref_direction,
            radius,
        } => Some(CarrierEquation::Sphere(SphereEquation {
            center: [center.x, center.y, center.z],
            ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
            radius: *radius,
        })),
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } if ratio.is_finite() && *ratio > 0.0 => Some(CarrierEquation::Cone(ConeEquation {
            origin: [origin.x, origin.y, origin.z],
            axis: [axis.x, axis.y, axis.z],
            ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
            radius: *radius,
            ratio: *ratio,
            half_angle: *half_angle,
        })),
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => Some(CarrierEquation::Torus(TorusEquation {
            center: [center.x, center.y, center.z],
            axis: [axis.x, axis.y, axis.z],
            ref_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        })),
        _ => None,
    }
}

pub fn geometry_section_record(scan: &ContainerScan, offset: usize) -> Option<UnknownId> {
    scan.framing
        .sections
        .iter()
        .filter(|section| section.role == ContainerRole::PsbGeometry)
        .find(|section| {
            offset >= section.offset && offset < section.offset.saturating_add(section.length)
        })
        .map(|section| {
            UnknownId::mint(format!("creo:{}:section#{}", section.name, section.offset))
                .expect("identity grammar")
        })
}

#[cfg(test)]
mod tests;

pub fn projected_loop_polygon(
    lp: &crate::topology::Loop,
    plane: PlaneEquation,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Option<Vec<[f64; 2]>> {
    let dropped_axis = (0..3).max_by(|left, right| {
        plane.normal[*left]
            .abs()
            .total_cmp(&plane.normal[*right].abs())
    })?;
    let polygon = lp
        .half_edges
        .iter()
        .map(|half_edge| {
            let vertex = incidence.get(half_edge)?.start_vertex_id;
            let point = solved_vertices.get(&vertex)?;
            Some(match dropped_axis {
                0 => [point[1], point[2]],
                1 => [point[0], point[2]],
                _ => [point[0], point[1]],
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let area_twice = (0..polygon.len())
        .map(|index| {
            let first = polygon[index];
            let second = polygon[(index + 1) % polygon.len()];
            first[0].mul_add(second[1], -(first[1] * second[0]))
        })
        .sum::<f64>();
    let scale = polygon
        .iter()
        .flat_map(|point| point.iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    (polygon.len() >= 3 && area_twice.abs() > EPS_NEAR_ZERO * scale * scale).then_some(polygon)
}

pub fn polygon_strictly_contains(polygon: &[[f64; 2]], point: [f64; 2]) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut inside = false;
    for index in 0..polygon.len() {
        let first = polygon[index];
        let second = polygon[(index + 1) % polygon.len()];
        let edge = [second[0] - first[0], second[1] - first[1]];
        let relative = [point[0] - first[0], point[1] - first[1]];
        let cross = edge[0].mul_add(relative[1], -(edge[1] * relative[0]));
        let scale = edge[0].abs().max(edge[1].abs()).max(1.0);
        if cross.abs() <= EPS_AGREE * scale
            && point[0] >= first[0].min(second[0]) - EPS_AGREE * scale
            && point[0] <= first[0].max(second[0]) + EPS_AGREE * scale
            && point[1] >= first[1].min(second[1]) - EPS_AGREE * scale
            && point[1] <= first[1].max(second[1]) + EPS_AGREE * scale
        {
            return false;
        }
        if (first[1] > point[1]) != (second[1] > point[1]) {
            let intersection = edge[0].mul_add((point[1] - first[1]) / edge[1], first[0]);
            if point[0] < intersection {
                inside = !inside;
            }
        }
    }
    inside
}

fn segments_intersect(first: [[f64; 2]; 2], second: [[f64; 2]; 2]) -> bool {
    let orientation = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        let edge = [b[0] - a[0], b[1] - a[1]];
        let relative = [c[0] - a[0], c[1] - a[1]];
        edge[0].mul_add(relative[1], -(edge[1] * relative[0]))
    };
    let points = [first[0], first[1], second[0], second[1]];
    let scale = (0..points.len())
        .flat_map(|first| {
            (first + 1..points.len()).map(move |second| {
                (0..2)
                    .map(|axis| (points[first][axis] - points[second][axis]).abs())
                    .fold(0.0, f64::max)
            })
        })
        .fold(1.0, f64::max);
    let tolerance = EPS_AGREE * scale * scale;
    let orientations = [
        orientation(first[0], first[1], second[0]),
        orientation(first[0], first[1], second[1]),
        orientation(second[0], second[1], first[0]),
        orientation(second[0], second[1], first[1]),
    ];
    let on_segment = |a: [f64; 2], b: [f64; 2], point: [f64; 2]| {
        orientation(a, b, point).abs() <= tolerance
            && point[0] >= a[0].min(b[0]) - EPS_AGREE * scale
            && point[0] <= a[0].max(b[0]) + EPS_AGREE * scale
            && point[1] >= a[1].min(b[1]) - EPS_AGREE * scale
            && point[1] <= a[1].max(b[1]) + EPS_AGREE * scale
    };
    orientations[0].abs() <= tolerance && on_segment(first[0], first[1], second[0])
        || orientations[1].abs() <= tolerance && on_segment(first[0], first[1], second[1])
        || orientations[2].abs() <= tolerance && on_segment(second[0], second[1], first[0])
        || orientations[3].abs() <= tolerance && on_segment(second[0], second[1], first[1])
        || ((orientations[0] > tolerance && orientations[1] < -tolerance)
            || (orientations[0] < -tolerance && orientations[1] > tolerance))
            && ((orientations[2] > tolerance && orientations[3] < -tolerance)
                || (orientations[2] < -tolerance && orientations[3] > tolerance))
}

fn polygon_strictly_contains_polygon(outer: &[[f64; 2]], inner: &[[f64; 2]]) -> bool {
    inner
        .iter()
        .copied()
        .all(|point| polygon_strictly_contains(outer, point))
        && (0..inner.len()).all(|index| {
            let inner_edge = [inner[index], inner[(index + 1) % inner.len()]];
            (0..outer.len()).all(|outer_index| {
                let outer_edge = [outer[outer_index], outer[(outer_index + 1) % outer.len()]];
                !segments_intersect(inner_edge, outer_edge)
            })
        })
}

fn valid_parameter_polygon(polygon: &[[f64; 2]]) -> bool {
    if polygon.len() < 3 || polygon.iter().flatten().any(|value| !value.is_finite()) {
        return false;
    }
    let area_twice = (0..polygon.len())
        .map(|index| {
            let first = polygon[index];
            let second = polygon[(index + 1) % polygon.len()];
            first[0].mul_add(second[1], -(first[1] * second[0]))
        })
        .sum::<f64>();
    let scale = polygon
        .iter()
        .flat_map(|point| point.iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    area_twice.abs() > EPS_NEAR_ZERO * scale * scale
}

fn ordered_contained_face_loops<'a>(
    loops: Vec<&'a crate::topology::Loop>,
    polygons: &[Vec<[f64; 2]>],
) -> Option<Vec<&'a crate::topology::Loop>> {
    if loops.len() < 2
        || loops.len() != polygons.len()
        || polygons
            .iter()
            .any(|polygon| !valid_parameter_polygon(polygon))
    {
        return None;
    }
    let outer = polygons
        .iter()
        .enumerate()
        .filter(|(candidate, polygon)| {
            polygons.iter().enumerate().all(|(index, inner)| {
                index == *candidate || polygon_strictly_contains_polygon(polygon, inner)
            })
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [outer] = outer.as_slice() else {
        return None;
    };
    let mut ordered = Vec::with_capacity(loops.len());
    ordered.push(loops[*outer]);
    ordered.extend(
        loops
            .into_iter()
            .enumerate()
            .filter_map(|(index, lp)| (index != *outer).then_some(lp)),
    );
    Some(ordered)
}

pub fn ordered_planar_face_loops<'a>(
    loops: Vec<&'a crate::topology::Loop>,
    plane: PlaneEquation,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Option<Vec<&'a crate::topology::Loop>> {
    if loops.len() == 1 {
        return Some(loops);
    }
    let polygons = loops
        .iter()
        .map(|lp| projected_loop_polygon(lp, plane, incidence, solved_vertices))
        .collect::<Option<Vec<_>>>()?;
    ordered_contained_face_loops(loops, &polygons)
}

pub fn ordered_parameter_face_loops<'a>(
    loops: Vec<&'a crate::topology::Loop>,
    polygons: &[Vec<[f64; 2]>],
) -> Option<Vec<&'a crate::topology::Loop>> {
    if loops.len() == 1 {
        return Some(loops);
    }
    ordered_contained_face_loops(loops, polygons)
}

pub fn face_boundary_plane(
    loops: &[&crate::topology::Loop],
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Option<PlaneEquation> {
    topology_bound_plane(loops.iter().flat_map(|lp| {
        lp.half_edges
            .iter()
            .filter_map(|half_edge| incidence.get(half_edge))
            .filter_map(|binding| solved_vertices.get(&binding.start_vertex_id).copied())
    }))
}

pub fn ordered_face_loops<'a>(
    loops: Vec<&'a crate::topology::Loop>,
    plane: Option<PlaneEquation>,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
) -> Option<Vec<&'a crate::topology::Loop>> {
    let plane = plane.or_else(|| face_boundary_plane(&loops, incidence, solved_vertices));
    if let Some(plane) = plane {
        ordered_planar_face_loops(loops, plane, incidence, solved_vertices)
    } else {
        let [single] = loops.as_slice() else {
            return None;
        };
        Some(vec![*single])
    }
}

pub fn rowless_round_face_orientations(
    round_feature_ids: &BTreeSet<u32>,
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
    available_surfaces: &BTreeSet<u32>,
) -> BTreeMap<u32, bool> {
    let mut orientations = BTreeMap::new();
    for (rowless_id, sibling_id, _) in rowless_round_cylinder_pairs(round_feature_ids, tables, rows)
    {
        if !available_surfaces.contains(&rowless_id) {
            continue;
        }
        let Some(reversed) =
            crate::surface::unique_surface_row(rows, sibling_id).map(|row| row.reversed)
        else {
            continue;
        };
        orientations.insert(rowless_id, reversed);
    }
    orientations
}

pub fn native_face_orientations(scan: &ContainerScan, ir: &CadIr) -> BTreeMap<u32, bool> {
    let mut orientations = scan
        .surfaces
        .rows
        .iter()
        .chain(&scan.surfaces.nonvisible_rows)
        .map(|row| row.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|id| {
            crate::decode::surfaces::unique_native_surface_row(scan, id)
                .map(|row| (id, row.reversed))
        })
        .collect::<BTreeMap<_, _>>();
    orientations.extend(
        scan.planes
            .datum_cylinders
            .iter()
            .map(|datum| (datum.id, datum.reversed)),
    );
    let round_feature_ids = scan
        .features
        .rows
        .iter()
        .filter(|row| row.root_schema_class == Some(913))
        .map(|row| row.feature_id)
        .collect::<BTreeSet<_>>();
    let available_surfaces = ir
        .model
        .surfaces
        .iter()
        .filter_map(|surface| {
            surface
                .id
                .0
                .strip_prefix("creo:visibgeom:surface#")?
                .parse()
                .ok()
        })
        .collect::<BTreeSet<_>>();
    orientations.extend(rowless_round_face_orientations(
        &round_feature_ids,
        &scan.features.entity_tables,
        &scan.surfaces.rows,
        &available_surfaces,
    ));
    orientations
}

#[cfg(test)]
mod namespace_tests {
    use super::native_face_orientations;
    use cadmpeg_ir::document::CadIr;

    #[test]
    fn native_face_orientations_reads_nonvisible_rows() {
        let mut scan = crate::container::scan_bytes(Vec::new());
        scan.surfaces
            .nonvisible_rows
            .push(crate::surface::SurfaceRow {
                id: 17,
                type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
                kind: crate::surface::SurfaceKind::Plane,
                feature_id: 1,
                reversed: true,
                boundary_type: 0,
                next_surface: 0,
                offset: 0,
            });

        let orientations = native_face_orientations(&scan, &CadIr::empty());

        assert_eq!(orientations.get(&17), Some(&true));
    }
}
