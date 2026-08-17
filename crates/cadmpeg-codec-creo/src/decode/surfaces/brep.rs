// SPDX-License-Identifier: Apache-2.0
//! Native B-rep transfer and FC05 cap-pair cylinders.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Pcurve, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop as IrLoop, PcurveUse, Point, Region, Sense, Shell,
    Vertex,
};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};

use crate::container::ContainerScan;
use crate::topology::HalfEdgeId;

use super::super::analytic::{
    exact_line_edge_parameter_range, full_periodic_conic_edge_parameter_range,
    full_periodic_nurbs_edge_parameter_range, geometry_section_record, meridian_circle_pcurve,
    native_face_orientations, nonperiodic_conic_edge_parameter_range, ordered_face_loops,
    ordered_parameter_face_loops, orient_line_edge_carrier, orient_nonperiodic_nurbs_edge_carrier,
    pcurve_backed_periodic_conic_parameter_range, placed_carriers, planar_curve_pcurve,
    ruled_generator_line_pcurve, solved_topological_vertices,
    surface_of_revolution_parallel_pcurve, unique_oriented_native_pcurve, CarrierEquation,
    NativePcurveCandidates,
};
use super::super::native::annotate;
use super::super::sweep::line_pcurve;
use super::super::uniqueness::exactly_one;

use super::fc05_model_frame;

const EPS_PARAMETER_AGREE: f64 = 1e-9;

#[derive(Debug, PartialEq, Eq)]
struct NeutralShellSpec {
    faces: Vec<u32>,
    wire_curves: BTreeSet<u32>,
}

/// Partition one native component into valid neutral shells.
///
/// Face shells follow admitted face connectivity through edges or vertices.
/// Solved curves excluded from a face loop remain wire topology, attached to
/// a face shell only when exactly one shell touches an endpoint and otherwise
/// grouped in a wire shell.
fn split_neutral_component_shells(
    faces: &[u32],
    wire_curves: &BTreeSet<u32>,
    face_adjacency: &BTreeMap<u32, BTreeSet<u32>>,
    face_vertices: &BTreeMap<u32, BTreeSet<u32>>,
    edge_vertices: &BTreeMap<u32, [u32; 2]>,
) -> Vec<NeutralShellSpec> {
    let mut remaining_faces = faces.iter().copied().collect::<BTreeSet<_>>();
    let mut face_groups = Vec::<Vec<u32>>::new();
    while let Some(start) = remaining_faces.pop_first() {
        let mut group = BTreeSet::from([start]);
        let mut pending = vec![start];
        while let Some(face_id) = pending.pop() {
            for neighbour in face_adjacency.get(&face_id).into_iter().flatten().copied() {
                if remaining_faces.remove(&neighbour) {
                    group.insert(neighbour);
                    pending.push(neighbour);
                }
            }
        }
        face_groups.push(group.into_iter().collect());
    }

    let mut shell_specs = face_groups
        .into_iter()
        .map(|faces| NeutralShellSpec {
            faces,
            wire_curves: BTreeSet::new(),
        })
        .collect::<Vec<_>>();
    let mut unattached_wire_curves = BTreeSet::new();
    for curve_id in wire_curves {
        let curve_vertices = edge_vertices[curve_id].into_iter().collect::<BTreeSet<_>>();
        let matching_shell = exactly_one(
            shell_specs
                .iter()
                .enumerate()
                .filter(|(_, shell)| {
                    shell
                        .faces
                        .iter()
                        .any(|face_id| !face_vertices[face_id].is_disjoint(&curve_vertices))
                })
                .map(|(index, _)| index),
        );
        if let Some(index) = matching_shell {
            shell_specs[index].wire_curves.insert(*curve_id);
        } else {
            unattached_wire_curves.insert(*curve_id);
        }
    }
    if !unattached_wire_curves.is_empty() {
        shell_specs.push(NeutralShellSpec {
            faces: Vec::new(),
            wire_curves: unattached_wire_curves,
        });
    }
    shell_specs
}

fn component_is_closed(
    component_face_curves: &BTreeSet<u32>,
    emitted_half_edges: &BTreeSet<HalfEdgeId>,
    half_edges: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdge>,
    faces: &[u32],
) -> bool {
    component_face_curves.iter().all(|curve_id| {
        let face_uses = emitted_half_edges
            .iter()
            .filter(|half_edge| half_edge.curve_id == *curve_id)
            .filter_map(|half_edge| half_edges.get(half_edge))
            .map(|half_edge| half_edge.face_id)
            .collect::<Vec<_>>();
        face_uses.len() == 2
            && face_uses
                .iter()
                .all(|face_id| *face_id != 0 && faces.contains(face_id))
    })
}

fn parameter_points_agree(first: [f64; 2], second: [f64; 2]) -> bool {
    let scale = first
        .into_iter()
        .chain(second)
        .map(f64::abs)
        .fold(1.0, f64::max);
    first
        .into_iter()
        .zip(second)
        .all(|(first, second)| (first - second).abs() <= EPS_PARAMETER_AGREE * scale)
}

fn native_parameter_loop_polygon(
    lp: &crate::topology::Loop,
    face_id: u32,
    surface: &SurfaceGeometry,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
    native_pcurves: &NativePcurveCandidates,
) -> Option<Vec<[f64; 2]>> {
    let segments = lp
        .half_edges
        .iter()
        .map(|half_edge| {
            let binding = incidence.get(half_edge)?;
            let end_vertex_id = binding.end_vertex_id?;
            let candidates = native_pcurves.get(&(half_edge.curve_id, face_id))?;
            let traversal = [
                solved_vertices.get(&binding.start_vertex_id).copied()?,
                solved_vertices.get(&end_vertex_id).copied()?,
            ];
            unique_oriented_native_pcurve(surface, candidates, traversal)
                .map(|(endpoints, _)| endpoints)
        })
        .collect::<Option<Vec<_>>>()?;
    if segments.len() < 3
        || segments
            .iter()
            .flatten()
            .flatten()
            .any(|value| !value.is_finite())
        || segments.iter().enumerate().any(|(index, segment)| {
            let next = segments[(index + 1) % segments.len()];
            !parameter_points_agree(segment[1], next[0])
        })
    {
        return None;
    }
    Some(segments.into_iter().map(|segment| segment[0]).collect())
}

fn ordered_native_parameter_face_loops<'a>(
    loops: Vec<&'a crate::topology::Loop>,
    face_id: u32,
    surface: &SurfaceGeometry,
    incidence: &BTreeMap<HalfEdgeId, &crate::topology::HalfEdgeVertexIncidence>,
    solved_vertices: &BTreeMap<u32, [f64; 3]>,
    native_pcurves: &NativePcurveCandidates,
) -> Option<Vec<&'a crate::topology::Loop>> {
    let polygons = loops
        .iter()
        .map(|lp| {
            native_parameter_loop_polygon(
                lp,
                face_id,
                surface,
                incidence,
                solved_vertices,
                native_pcurves,
            )
        })
        .collect::<Option<Vec<_>>>()?;
    ordered_parameter_face_loops(loops, &polygons)
}

#[cfg(test)]
mod tests;

pub(in super::super) fn transfer_native_brep(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    derived_intersection_curves: &BTreeSet<CurveId>,
    analytic_pcurve_carriers: &BTreeSet<CurveId>,
    nurbs_endpoint_witnesses: &BTreeSet<CurveId>,
) -> (usize, usize) {
    let carriers = placed_carriers(scan, ir);
    let planes = carriers
        .iter()
        .filter_map(|(id, carrier)| match carrier {
            CarrierEquation::Plane(plane) => Some((*id, *plane)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let face_orientations = native_face_orientations(scan, ir);
    let half_edges = scan
        .topology
        .half_edges
        .iter()
        .map(|half_edge| (half_edge.id, half_edge))
        .collect::<BTreeMap<_, _>>();
    let incidence = scan
        .topology
        .half_edge_vertex_incidence
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect::<BTreeMap<_, _>>();
    let solved_vertices =
        solved_topological_vertices(scan, ir, &carriers, nurbs_endpoint_witnesses);
    let mut native_pcurves = NativePcurveCandidates::new();
    for (curve_id, faces, face_0_endpoints, face_1_endpoints, offset) in scan
        .curves
        .pcurves
        .iter()
        .map(|pcurve| {
            (
                pcurve.curve_id,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
                pcurve.offset,
            )
        })
        .chain(scan.curves.bound_prototype_pcurves.iter().map(|pcurve| {
            (
                pcurve.curve_id,
                pcurve.faces,
                pcurve.face_0_endpoints,
                pcurve.face_1_endpoints,
                pcurve.offset,
            )
        }))
    {
        native_pcurves
            .entry((curve_id, faces[0]))
            .or_default()
            .push((face_0_endpoints, offset));
        native_pcurves
            .entry((curve_id, faces[1]))
            .or_default()
            .push((face_1_endpoints, offset));
    }
    let native_edge_vertices =
        crate::topology::edge_vertex_pairs(&scan.topology.half_edge_vertex_incidence);
    let edge_vertices = crate::topology::uniquely_identified_rows(&scan.curves.topology_rows)
        .into_iter()
        .filter_map(|row| {
            let vertices = native_edge_vertices.get(&row.id).copied()?;
            vertices
                .iter()
                .all(|vertex| solved_vertices.contains_key(vertex))
                .then_some((row.id, vertices))
        })
        .collect::<BTreeMap<_, _>>();
    let model_curve_counts = edge_vertices
        .keys()
        .map(|curve_id| {
            let id = CurveId(format!("creo:visibgeom:curve#{curve_id}"));
            let count = ir
                .model
                .curves
                .iter()
                .filter(|curve| curve.id == id)
                .count();
            (*curve_id, count)
        })
        .collect::<BTreeMap<_, _>>();
    let admitted_edge_curves = edge_vertices
        .keys()
        .copied()
        .filter(|curve_id| model_curve_counts[curve_id] <= 1)
        .collect::<BTreeSet<_>>();
    let model_surface_counts = face_orientations
        .keys()
        .map(|face_id| {
            let id = SurfaceId(format!("creo:visibgeom:surface#{face_id}"));
            let count = ir
                .model
                .surfaces
                .iter()
                .filter(|surface| surface.id == id)
                .count();
            (*face_id, count)
        })
        .collect::<BTreeMap<_, _>>();
    let admitted_face_ids = face_orientations
        .keys()
        .copied()
        .filter(|face_id| model_surface_counts[face_id] <= 1)
        .collect::<BTreeSet<_>>();
    let mut loops_by_face = BTreeMap::<u32, Vec<&crate::topology::Loop>>::new();
    for lp in &scan.topology.loops {
        loops_by_face.entry(lp.face_id).or_default().push(lp);
    }
    let eligible_faces = loops_by_face
        .into_iter()
        .filter_map(|(face_id, loops)| {
            admitted_face_ids.contains(&face_id).then_some(())?;
            loops
                .iter()
                .all(|lp| {
                    lp.half_edges
                        .iter()
                        .all(|half_edge| admitted_edge_curves.contains(&half_edge.curve_id))
                })
                .then_some(())?;
            let ordered = ordered_face_loops(
                loops.clone(),
                planes.get(&face_id).copied(),
                &incidence,
                &solved_vertices,
            )
            .or_else(|| {
                let surface_id = SurfaceId(format!("creo:visibgeom:surface#{face_id}"));
                let surface = exactly_one(
                    ir.model
                        .surfaces
                        .iter()
                        .filter(|candidate| candidate.id == surface_id),
                )?;
                ordered_native_parameter_face_loops(
                    loops,
                    face_id,
                    &surface.geometry,
                    &incidence,
                    &solved_vertices,
                    &native_pcurves,
                )
            })?;
            Some((face_id, ordered))
        })
        .collect::<BTreeMap<_, _>>();
    let eligible_loops = eligible_faces
        .values()
        .flatten()
        .copied()
        .collect::<Vec<_>>();

    let emitted_half_edges = eligible_loops
        .iter()
        .flat_map(|lp| lp.half_edges.iter().copied())
        .collect::<BTreeSet<_>>();
    let face_curves = emitted_half_edges
        .iter()
        .map(|half_edge| half_edge.curve_id)
        .collect::<BTreeSet<_>>();
    let closed_single_edge_curves = face_curves
        .iter()
        .filter(|curve_id| {
            let uses = eligible_loops
                .iter()
                .filter(|lp| {
                    lp.half_edges
                        .iter()
                        .any(|half_edge| half_edge.curve_id == **curve_id)
                })
                .collect::<Vec<_>>();
            !uses.is_empty() && uses.iter().all(|lp| lp.half_edges.len() == 1)
        })
        .copied()
        .collect::<BTreeSet<_>>();
    let row_offsets = scan
        .curves
        .topology_rows
        .iter()
        .map(|row| (row.id, row.offset))
        .collect::<BTreeMap<_, _>>();
    let curve_faces = crate::topology::uniquely_identified_rows(&scan.curves.topology_rows)
        .into_iter()
        .map(|row| (row.id, row.faces))
        .collect::<BTreeMap<_, _>>();

    let neutral_edge_curves = scan
        .topology
        .face_components
        .iter()
        .flat_map(|component| component.curve_ids.iter().copied())
        .filter(|curve_id| admitted_edge_curves.contains(curve_id))
        .collect::<BTreeSet<_>>();
    let body_components = scan
        .topology
        .face_components
        .iter()
        .map(|component| {
            let faces = component
                .face_ids
                .iter()
                .copied()
                .filter(|face_id| eligible_faces.contains_key(face_id))
                .collect::<Vec<_>>();
            let curves = component
                .curve_ids
                .iter()
                .copied()
                .filter(|curve_id| neutral_edge_curves.contains(curve_id))
                .collect::<BTreeSet<_>>();
            (faces, curves)
        })
        .collect::<Vec<_>>();
    let selected_body_count = crate::topology::selected_body_count(
        scan.framing.declared_body_count,
        scan.framing.first_quilt_ptr,
        scan.topology.face_components.len(),
    );
    let solved_point_count = solved_vertices.len();
    for (vertex_id, position) in &solved_vertices {
        let point_id = PointId(format!("creo:visibgeom:point#{vertex_id}"));
        if ir.model.points.iter().any(|item| item.id == point_id) {
            continue;
        }
        annotate(
            annotations,
            &point_id,
            "VisibGeom",
            0,
            "topological_vertex_point",
            Exactness::Derived,
        );
        ir.model.points.push(Point {
            id: point_id,
            position: Point3::new(position[0], position[1], position[2]),
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("topology:vertex#{vertex_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
    }
    if selected_body_count != Some(body_components.len())
        || body_components
            .iter()
            .any(|(faces, curves)| faces.is_empty() && curves.is_empty())
    {
        return (solved_point_count, 0);
    }

    let used_vertices = neutral_edge_curves
        .iter()
        .filter_map(|curve| edge_vertices.get(curve))
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>();
    for vertex_id in used_vertices {
        let vertex = VertexId(format!("creo:visibgeom:vertex#{vertex_id}"));
        if ir.model.vertices.iter().any(|item| item.id == vertex) {
            continue;
        }
        let point_id = PointId(format!("creo:visibgeom:point#{vertex_id}"));
        annotate(
            annotations,
            &vertex,
            "VisibGeom",
            0,
            "topological_vertex_orbit",
            Exactness::Derived,
        );
        ir.model.vertices.push(Vertex {
            id: vertex,
            point: point_id,
            tolerance: None,
        });
    }
    for curve_id in &neutral_edge_curves {
        let [start, end] = edge_vertices[curve_id];
        let curve = CurveId(format!("creo:visibgeom:curve#{curve_id}"));
        let points = [solved_vertices[&start], solved_vertices[&end]];
        let unbacked_closed_edge = start == end
            && closed_single_edge_curves.contains(curve_id)
            && curve_faces.get(curve_id).is_some_and(|face_ids| {
                !face_ids
                    .iter()
                    .any(|face_id| native_pcurves.contains_key(&(*curve_id, *face_id)))
            });
        let model_curve_count = model_curve_counts[curve_id];
        let derived_line = (derived_intersection_curves.contains(&curve)
            || analytic_pcurve_carriers.contains(&curve))
            && exactly_one(
                ir.model
                    .curves
                    .iter()
                    .filter(|candidate| candidate.id == curve),
            )
            .is_some_and(|candidate| matches!(&candidate.geometry, CurveGeometry::Line { .. }));
        let param_range = if model_curve_count == 0 {
            None
        } else if derived_line {
            exactly_one(
                ir.model
                    .curves
                    .iter_mut()
                    .filter(|candidate| candidate.id == curve),
            )
            .and_then(|candidate| orient_line_edge_carrier(&mut candidate.geometry, points))
        } else {
            exactly_one(
                ir.model
                    .curves
                    .iter_mut()
                    .filter(|candidate| candidate.id == curve),
            )
            .and_then(|candidate| {
                orient_nonperiodic_nurbs_edge_carrier(&mut candidate.geometry, points).or_else(
                    || {
                        exact_line_edge_parameter_range(&candidate.geometry, points).or_else(|| {
                            nonperiodic_conic_edge_parameter_range(&candidate.geometry, points)
                                .or_else(|| {
                                    pcurve_backed_periodic_conic_parameter_range(
                                        &candidate.geometry,
                                        *curve_id,
                                        *curve_faces.get(curve_id)?,
                                        &native_pcurves,
                                        &ir.model.surfaces,
                                        points,
                                    )
                                })
                                .or_else(|| {
                                    unbacked_closed_edge.then_some(()).and_then(|()| {
                                        full_periodic_conic_edge_parameter_range(
                                            &candidate.geometry,
                                            points[0],
                                        )
                                    })
                                })
                                .or_else(|| {
                                    unbacked_closed_edge.then_some(()).and_then(|()| {
                                        full_periodic_nurbs_edge_parameter_range(
                                            &candidate.geometry,
                                            points[0],
                                        )
                                    })
                                })
                        })
                    },
                )
            })
        };
        let id = EdgeId(format!("creo:visibgeom:edge#{curve_id}"));
        annotate(
            annotations,
            &id,
            "VisibGeom",
            row_offsets.get(curve_id).copied().unwrap_or(0) as u64,
            "curve_topology_edge",
            Exactness::Derived,
        );
        ir.model.edges.push(Edge {
            id,
            curve: Some(curve.clone()),
            start: VertexId(format!("creo:visibgeom:vertex#{start}")),
            end: VertexId(format!("creo:visibgeom:vertex#{end}")),
            param_range,
            tolerance: None,
        });
        if !ir.model.curves.iter().any(|item| item.id == curve) {
            let offset = row_offsets.get(curve_id).copied().unwrap_or(0);
            annotate(
                annotations,
                &curve,
                "VisibGeom",
                offset as u64,
                "opaque_native_curve_carrier",
                Exactness::Unknown,
            );
            ir.model.curves.push(Curve {
                id: curve,
                geometry: CurveGeometry::Unknown {
                    record: geometry_section_record(scan, offset),
                },
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
        }
    }

    for (component_index, (faces, component_curves)) in body_components.iter().enumerate() {
        let body_id = BodyId(format!("creo:visibgeom:body#{}", component_index + 1));
        let region_id = RegionId(format!("creo:visibgeom:region#{}", component_index + 1));
        for (id, tag) in [
            (body_id.to_string(), "native_component_body"),
            (region_id.to_string(), "native_component_region"),
        ] {
            annotate(annotations, id, "VisibGeom", 0, tag, Exactness::Derived);
        }
        let component_face_curves = component_curves
            .intersection(&face_curves)
            .copied()
            .collect::<BTreeSet<_>>();
        let wire_curves = component_curves
            .difference(&face_curves)
            .copied()
            .collect::<BTreeSet<_>>();
        let closed = component_is_closed(
            &component_face_curves,
            &emitted_half_edges,
            &half_edges,
            faces,
        );

        let mut face_adjacency = faces
            .iter()
            .copied()
            .map(|face_id| (face_id, BTreeSet::new()))
            .collect::<BTreeMap<_, _>>();
        let mut face_vertices = BTreeMap::<u32, BTreeSet<u32>>::new();
        let mut faces_by_curve = BTreeMap::<u32, BTreeSet<u32>>::new();
        let mut faces_by_vertex = BTreeMap::<u32, BTreeSet<u32>>::new();
        for face_id in faces {
            let vertices = face_vertices.entry(*face_id).or_default();
            for native_loop in &eligible_faces[face_id] {
                for half_edge in &native_loop.half_edges {
                    faces_by_curve
                        .entry(half_edge.curve_id)
                        .or_default()
                        .insert(*face_id);
                    let [start, end] = edge_vertices[&half_edge.curve_id];
                    vertices.extend([start, end]);
                    faces_by_vertex.entry(start).or_default().insert(*face_id);
                    faces_by_vertex.entry(end).or_default().insert(*face_id);
                }
            }
        }
        for incident_faces in faces_by_curve.values().chain(faces_by_vertex.values()) {
            let incident_faces = incident_faces.iter().copied().collect::<Vec<_>>();
            for (index, first) in incident_faces.iter().enumerate() {
                face_adjacency
                    .entry(*first)
                    .or_default()
                    .extend(incident_faces.iter().skip(index + 1).copied());
                for second in incident_faces.iter().skip(index + 1) {
                    face_adjacency.entry(*second).or_default().insert(*first);
                }
            }
        }
        let shell_specs = split_neutral_component_shells(
            faces,
            &wire_curves,
            &face_adjacency,
            &face_vertices,
            &edge_vertices,
        );

        let mut face_shell_ids = BTreeMap::<u32, ShellId>::new();
        let shell_ids = shell_specs
            .iter()
            .enumerate()
            .map(|(shell_index, shell)| {
                let shell_id = if shell_index == 0 {
                    ShellId(format!("creo:visibgeom:shell#{}", component_index + 1))
                } else {
                    ShellId(format!(
                        "creo:visibgeom:shell#{}:{}",
                        component_index + 1,
                        shell_index + 1
                    ))
                };
                annotate(
                    annotations,
                    shell_id.to_string(),
                    "VisibGeom",
                    0,
                    "native_component_shell",
                    Exactness::Derived,
                );
                for face_id in &shell.faces {
                    face_shell_ids.insert(*face_id, shell_id.clone());
                }
                ir.model.shells.push(Shell {
                    id: shell_id.clone(),
                    region: region_id.clone(),
                    faces: shell
                        .faces
                        .iter()
                        .map(|face| FaceId(format!("creo:visibgeom:face#{face}")))
                        .collect(),
                    wire_edges: shell
                        .wire_curves
                        .iter()
                        .map(|curve_id| EdgeId(format!("creo:visibgeom:edge#{curve_id}")))
                        .collect(),
                    free_vertices: Vec::new(),
                });
                shell_id
            })
            .collect::<Vec<_>>();
        ir.model.bodies.push(Body {
            id: body_id.clone(),
            kind: if !wire_curves.is_empty() {
                BodyKind::General
            } else if closed {
                BodyKind::Solid
            } else {
                BodyKind::Sheet
            },
            regions: vec![region_id.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id,
            shells: shell_ids,
        });
        for face_id in faces {
            let native_loops = &eligible_faces[face_id];
            let face = FaceId(format!("creo:visibgeom:face#{face_id}"));
            let shell_id = face_shell_ids[face_id].clone();
            let loop_ids = (0..native_loops.len())
                .map(|index| {
                    if index == 0 {
                        LoopId(format!("creo:visibgeom:loop#{face_id}"))
                    } else {
                        LoopId(format!("creo:visibgeom:loop#{face_id}:{index}"))
                    }
                })
                .collect::<Vec<_>>();
            let face_offset = crate::surface::unique_surface_row(&scan.surfaces.rows, *face_id)
                .map_or(0, |row| row.offset);
            let surface = SurfaceId(format!("creo:visibgeom:surface#{face_id}"));
            if !ir.model.surfaces.iter().any(|item| item.id == surface) {
                annotate(
                    annotations,
                    &surface,
                    "VisibGeom",
                    face_offset as u64,
                    "opaque_native_surface_carrier",
                    Exactness::Unknown,
                );
                ir.model.surfaces.push(Surface {
                    id: surface.clone(),
                    geometry: SurfaceGeometry::Unknown {
                        record: geometry_section_record(scan, face_offset),
                    },
                    source_object: Some(SourceObjectAssociation {
                        format: "creo".to_string(),
                        object_id: format!("VisibGeom:{face_id}"),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                });
            }
            let face_sense = if face_orientations[face_id] {
                Sense::Reversed
            } else {
                Sense::Forward
            };
            annotate(
                annotations,
                &face,
                "VisibGeom",
                face_offset as u64,
                "native_face",
                Exactness::Derived,
            );
            for loop_id in &loop_ids {
                annotate(
                    annotations,
                    loop_id,
                    "VisibGeom",
                    face_offset as u64,
                    "native_face_loop",
                    Exactness::Derived,
                );
            }
            ir.model.faces.push(Face {
                id: face.clone(),
                shell: shell_id.clone(),
                surface,
                sense: face_sense,
                loops: loop_ids.clone(),
                name: None,
                color: None,
                tolerance: None,
            });
            for (boundary_index, (native_loop, loop_id)) in
                native_loops.iter().zip(loop_ids).enumerate()
            {
                let coedge_ids = native_loop
                    .half_edges
                    .iter()
                    .map(|half_edge| {
                        CoedgeId(format!(
                            "creo:visibgeom:coedge#{}:{}",
                            half_edge.curve_id, half_edge.side
                        ))
                    })
                    .collect::<Vec<_>>();
                ir.model.loops.push(IrLoop {
                    id: loop_id.clone(),
                    face: face.clone(),
                    boundary_role: if boundary_index == 0 {
                        cadmpeg_ir::topology::LoopBoundaryRole::Outer
                    } else {
                        cadmpeg_ir::topology::LoopBoundaryRole::Inner
                    },
                    coedges: coedge_ids.clone(),
                    vertex_uses: Vec::new(),
                });
                for (index, half_edge) in native_loop.half_edges.iter().enumerate() {
                    let id = coedge_ids[index].clone();
                    let twin = HalfEdgeId {
                        curve_id: half_edge.curve_id,
                        side: 1 - half_edge.side,
                    };
                    let radial_next = if emitted_half_edges.contains(&twin) {
                        CoedgeId(format!(
                            "creo:visibgeom:coedge#{}:{}",
                            twin.curve_id, twin.side
                        ))
                    } else {
                        id.clone()
                    };
                    annotate(
                        annotations,
                        &id,
                        "VisibGeom",
                        row_offsets.get(&half_edge.curve_id).copied().unwrap_or(0) as u64,
                        "native_half_edge",
                        Exactness::Derived,
                    );
                    let native_candidates = native_pcurves.get(&(half_edge.curve_id, *face_id));
                    let pcurve_geometry = native_candidates
                        .and_then(|candidates| {
                            let incidence = incidence.get(half_edge)?;
                            let end = incidence.end_vertex_id?;
                            let traversal = [
                                solved_vertices[&incidence.start_vertex_id],
                                solved_vertices[&end],
                            ];
                            let surface_id = SurfaceId(format!("creo:visibgeom:surface#{face_id}"));
                            let surface = exactly_one(
                                ir.model
                                    .surfaces
                                    .iter()
                                    .filter(|candidate| candidate.id == surface_id),
                            )?;
                            unique_oriented_native_pcurve(&surface.geometry, candidates, traversal)
                        })
                        .map(|(endpoints, offset)| {
                            (
                                line_pcurve(endpoints[0], endpoints[1]),
                                Some([0.0, 1.0]),
                                offset,
                                "native_endpoint_pcurve",
                            )
                        })
                        .or_else(|| {
                            native_candidates.is_none().then_some(())?;
                            let surface_id = SurfaceId(format!("creo:visibgeom:surface#{face_id}"));
                            let surface = exactly_one(
                                ir.model
                                    .surfaces
                                    .iter()
                                    .filter(|candidate| candidate.id == surface_id),
                            )?;
                            let curve_id =
                                CurveId(format!("creo:visibgeom:curve#{}", half_edge.curve_id));
                            let curve = exactly_one(
                                ir.model
                                    .curves
                                    .iter()
                                    .filter(|candidate| candidate.id == curve_id),
                            )?;
                            let edge_id =
                                EdgeId(format!("creo:visibgeom:edge#{}", half_edge.curve_id));
                            let edge = exactly_one(
                                ir.model
                                    .edges
                                    .iter()
                                    .filter(|candidate| candidate.id == edge_id),
                            )?;
                            let (geometry, tag) =
                                planar_curve_pcurve(&surface.geometry, &curve.geometry)
                                    .map(|geometry| (geometry, "projected_planar_pcurve"))
                                    .or_else(|| {
                                        surface_of_revolution_parallel_pcurve(
                                            &surface.geometry,
                                            &curve.geometry,
                                        )
                                        .map(|geometry| {
                                            (geometry, "projected_parallel_conic_pcurve")
                                        })
                                    })
                                    .or_else(|| {
                                        meridian_circle_pcurve(&surface.geometry, &curve.geometry)
                                            .map(|geometry| (geometry, "projected_meridian_pcurve"))
                                    })
                                    .or_else(|| {
                                        ruled_generator_line_pcurve(
                                            &surface.geometry,
                                            &curve.geometry,
                                        )
                                        .map(|geometry| {
                                            (geometry, "projected_ruled_generator_pcurve")
                                        })
                                    })?;
                            Some((
                                geometry,
                                edge.param_range,
                                row_offsets.get(&half_edge.curve_id).copied().unwrap_or(0),
                                tag,
                            ))
                        });
                    let pcurves = pcurve_geometry
                        .map(|(geometry, parameter_range, offset, tag)| {
                            let pcurve = PcurveId(format!(
                                "creo:visibgeom:pcurve#{}:{face_id}",
                                half_edge.curve_id
                            ));
                            if !ir.model.pcurves.iter().any(|item| item.id == pcurve) {
                                annotate(
                                    annotations,
                                    &pcurve,
                                    "VisibGeom",
                                    offset as u64,
                                    tag,
                                    Exactness::Derived,
                                );
                                ir.model.pcurves.push(Pcurve {
                                    id: pcurve.clone(),
                                    geometry,
                                    wrapper_reversed: None,
                                    native_tail_flags: None,
                                    parameter_range,
                                    fit_tolerance: None,
                                });
                            }
                            PcurveUse {
                                pcurve,
                                isoparametric: None,
                                parameter_range: None,
                            }
                        })
                        .into_iter()
                        .collect();
                    ir.model.coedges.push(Coedge {
                        id,
                        owner_loop: loop_id.clone(),
                        edge: EdgeId(format!("creo:visibgeom:edge#{}", half_edge.curve_id)),
                        next: coedge_ids[(index + 1) % coedge_ids.len()].clone(),
                        previous: coedge_ids[(index + coedge_ids.len() - 1) % coedge_ids.len()]
                            .clone(),
                        radial_next,
                        sense: if half_edge.side == 0 {
                            Sense::Forward
                        } else {
                            Sense::Reversed
                        },
                        pcurves,
                        use_curve: None,
                        use_curve_parameter_range: None,
                    });
                }
            }
        }
    }
    (solved_point_count, neutral_edge_curves.len())
}

pub(in super::super) fn transfer_cap_pair_cylinders(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) {
    for pair in &scan.curves.fc05_cylinder_cap_pairs {
        let placed_caps = pair
            .cap_plane_ids
            .iter()
            .zip(&pair.curve_cap_ordinates_row_frame)
            .filter_map(|(id, ordinate)| {
                crate::surface::unique_outline_plane(&scan.planes.outlines, *id)
                    .map(|plane| (plane, *ordinate))
            })
            .collect::<Vec<_>>();
        let Some((first_cap, first_ordinate)) = placed_caps.first().copied() else {
            continue;
        };
        let Some(axis_index) = (0..3).find(|axis| first_cap.normal[*axis].abs() > 1.0 - 1e-9)
        else {
            continue;
        };
        if placed_caps
            .iter()
            .any(|(plane, _)| plane.normal != first_cap.normal)
        {
            continue;
        }
        let translations = placed_caps
            .iter()
            .map(|(plane, ordinate)| plane.origin[axis_index] - ordinate)
            .collect::<Vec<_>>();
        if translations
            .iter()
            .any(|translation| (translation - translations[0]).abs() > 1e-9)
        {
            continue;
        }
        let axis_origin = first_ordinate + translations[0];
        let axis_sign = -f64::from(pair.parameter_sign);
        let (origin, axis, ref_direction) = fc05_model_frame(
            axis_index,
            axis_origin,
            pair.center_row_frame,
            pair.reference_direction_row_frame,
            axis_sign,
        );
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", pair.surface_id));
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "VisibGeom",
            pair.offset as u64,
            "fc05_cap_pair_cylinder",
            Exactness::Derived,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                axis: Vector3::new(axis[0], axis[1], axis[2]),
                ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
                radius: pair.radius_mm,
            },
            source_object: Some(SourceObjectAssociation {
                format: "creo".to_string(),
                object_id: format!("VisibGeom:{}", pair.surface_id),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
        });
        for ((curve_id, ordinate), cap_plane_id) in pair
            .curve_ids
            .iter()
            .zip(&pair.curve_cap_ordinates_row_frame)
            .zip(&pair.cap_plane_ids)
        {
            let cap_offset =
                crate::surface::unique_outline_plane(&scan.planes.outlines, *cap_plane_id)
                    .map_or_else(
                        || ordinate + translations[0],
                        |plane| plane.origin[axis_index],
                    );
            let (center, _, _) = fc05_model_frame(
                axis_index,
                cap_offset,
                pair.center_row_frame,
                pair.reference_direction_row_frame,
                axis_sign,
            );
            let id = CurveId(format!("creo:visibgeom:curve#{curve_id}"));
            if ir.model.curves.iter().any(|curve| curve.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "VisibGeom",
                scan.curves
                    .fc05_circles
                    .iter()
                    .find(|circle| circle.curve_id == *curve_id)
                    .map_or(pair.offset, |circle| circle.offset) as u64,
                "fc05_cap_circle",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id,
                geometry: CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(
                        ref_direction[0],
                        ref_direction[1],
                        ref_direction[2],
                    ),
                    radius: pair.radius_mm,
                },
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
        }
    }
}
