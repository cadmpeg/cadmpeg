// SPDX-License-Identifier: Apache-2.0
//! Decode a `.CATPart` into an IR document, transferring the standard-nested
//! geometry this codec understands and reporting every other variant and every
//! unrecovered layer as explicit loss.
//!
//! The container layer (outer header, inner directory, BREP-stream reconstruction,
//! variant identification) is decoded by [`crate::container`]. For the
//! standard-nested variant this module reads the exact vertex point cloud and the
//! analytic curved-surface carriers ([`crate::geometry`]) into free carrier
//! arenas; the face→loop→edge topology graph is not reconstructed and is reported.
//! Every other variant is honestly detected, named, and left as container-only,
//! with its BREP/preamble bytes preserved as an [`UnknownRecord`].

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use cadmpeg_ir::codec::{CodecError, DecodeOptions, DecodeResult, ReadSeek};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, LumpId, PointId, ShellId, SurfaceId,
    UnknownId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::provenance::{EntityMeta, Exactness, Provenance};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::topology::{Body, Coedge, Edge, Face, Loop, Lump, Point, Sense, Shell, Vertex};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use sha2::{Digest, Sha256};

use crate::container::{self, ContainerScan};
use crate::geometry;
use crate::topology;
use crate::variant::Variant;

/// Decode a `.CATPart` reader into an IR + report.
pub fn decode(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    let scan = container::scan(reader)?;

    if options.container_only {
        let ir = build_metadata_ir(&scan);
        let report = build_container_report(&scan, true);
        return Ok(DecodeResult { ir, report });
    }

    if matches!(scan.variant, Variant::StandardNested | Variant::FbbOnly) {
        if let Some((ir, report)) = try_decode_standard(&scan) {
            return Ok(DecodeResult { ir, report });
        }
    }

    if scan.variant == Variant::ZeroEntity {
        if let Some((ir, report)) = try_decode_zero_entity(&scan) {
            return Ok(DecodeResult { ir, report });
        }
    }

    if scan.variant == Variant::E5Stream {
        if let Some((ir, report)) = try_decode_e5(&scan) {
            return Ok(DecodeResult { ir, report });
        }
    }

    if matches!(
        scan.variant,
        Variant::FloatPackedInnerNoFbb | Variant::FbbOnly | Variant::InnerNoDirectory
    ) {
        if let Some((ir, report)) = try_decode_freeform_surfaces(&scan) {
            return Ok(DecodeResult { ir, report });
        }
    }

    let ir = build_metadata_ir(&scan);
    let report = build_container_report(&scan, false);
    Ok(DecodeResult { ir, report })
}

/// Decode directly framed analytic carriers in the zero-entity record stream.
/// These records do not yet provide enough byte-bound topology to attach faces,
/// loops, or edges, but their carrier geometry is complete and transferable.
fn try_decode_zero_entity(scan: &ContainerScan) -> Option<(CadIr, DecodeReport)> {
    let decoded = geometry::zero_entity_surfaces(&scan.data);
    let points = geometry::vertices(&scan.data);
    if decoded.is_empty() && points.is_empty() {
        return None;
    }
    let mut ir = CadIr::empty(Units::default());
    ir.source = Some(source_meta(scan));
    preserve_raw_payload(&mut ir, scan, "catia:zero_entity_payload");
    for (index, point) in points.iter().enumerate() {
        let point_id = PointId(format!("catia:zero-entity:pt#{index}"));
        ir.points.push(Point {
            id: point_id.clone(),
            position: *point,
            meta: EntityMeta {
                provenance: Provenance {
                    format: "catia".to_string(),
                    stream: "zero_entity_a9_03".to_string(),
                    offset: 0,
                    tag: Some("vertex_05_08_01".to_string()),
                },
                exactness: Exactness::ByteExact,
            },
        });
        ir.vertices.push(Vertex {
            id: VertexId(format!("catia:zero-entity:v#{index}")),
            point: point_id,
            tolerance: None,
            meta: byte_exact("vertex_05_08_01", 0),
        });
    }
    for (index, surface) in decoded.iter().enumerate() {
        ir.surfaces.push(Surface {
            id: SurfaceId(format!("catia:zero-entity:surf#{index}")),
            geometry: surface.geometry.clone(),
            meta: EntityMeta {
                provenance: Provenance {
                    format: "catia".to_string(),
                    stream: "zero_entity_a9_03".to_string(),
                    offset: surface.pos as u64,
                    tag: Some("analytic_surface".to_string()),
                },
                exactness: Exactness::ByteExact,
            },
        });
    }
    let summary = container::summarize(scan);
    let report = DecodeReport {
        format: "catia".to_string(),
        container_only: false,
        geometry_transferred: true,
        losses: vec![LossNote {
            category: LossCategory::Topology,
            severity: Severity::Blocking,
            message: "Zero-entity analytic surface carriers were decoded, but the face/loop/coedge/edge/vertex graph is not yet transferred."
                .to_string(),
            provenance: None,
        }],
        notes: summary.notes,
    };
    Some((ir, report))
}

/// Decode direct E5 circle carriers.  Their edge and face references are a
/// separate record layer, so curves remain unattached until that layer is
/// decoded rather than being assigned speculatively.
fn try_decode_e5(scan: &ContainerScan) -> Option<(CadIr, DecodeReport)> {
    let circles = geometry::e5_circles(&scan.data);
    let surfaces = geometry::e5_surfaces(&scan.data);
    let edges = geometry::e5_edges(&scan.data);
    let points = geometry::vertices(&scan.data);
    if circles.is_empty() && surfaces.is_empty() && points.is_empty() {
        return None;
    }
    let mut ir = CadIr::empty(Units::default());
    ir.source = Some(source_meta(scan));
    preserve_raw_payload(&mut ir, scan, "catia:e5_payload");
    for (index, point) in points.iter().enumerate() {
        let point_id = PointId(format!("catia:e5:pt#{index}"));
        ir.points.push(Point {
            id: point_id.clone(),
            position: *point,
            meta: EntityMeta {
                provenance: Provenance {
                    format: "catia".to_string(),
                    stream: "e5_0d_03".to_string(),
                    offset: 0,
                    tag: Some("vertex_05_08_01".to_string()),
                },
                exactness: Exactness::ByteExact,
            },
        });
        ir.vertices.push(Vertex {
            id: VertexId(format!("catia:e5:v#{index}")),
            point: point_id,
            tolerance: None,
            meta: byte_exact("vertex_05_08_01", 0),
        });
    }
    let mut circle_ids = HashMap::new();
    for (index, circle) in circles.iter().enumerate() {
        let id = CurveId(format!("catia:e5:curve#{index}"));
        circle_ids.insert(circle.record_id, id.clone());
        ir.curves.push(Curve {
            id,
            geometry: circle.geometry.clone(),
            meta: EntityMeta {
                provenance: Provenance {
                    format: "catia".to_string(),
                    stream: "e5_0d_03".to_string(),
                    offset: circle.pos as u64,
                    tag: Some("circle_carrier".to_string()),
                },
                exactness: Exactness::ByteExact,
            },
        });
    }
    for (index, surface) in surfaces.iter().enumerate() {
        ir.surfaces.push(Surface {
            id: SurfaceId(format!("catia:e5:surf#{index}")),
            geometry: surface.geometry.clone(),
            meta: EntityMeta {
                provenance: Provenance {
                    format: "catia".to_string(),
                    stream: "e5_0d_03".to_string(),
                    offset: surface.pos as u64,
                    tag: Some("analytic_surface".to_string()),
                },
                exactness: Exactness::ByteExact,
            },
        });
    }
    attach_e5_edges(&mut ir, &edges, &circle_ids);
    Some((
        ir,
        DecodeReport {
            format: "catia".to_string(),
            container_only: false,
            geometry_transferred: true,
            losses: vec![LossNote {
                category: LossCategory::Topology,
                severity: Severity::Blocking,
                message: "E5 analytic carriers were decoded, but their edge/face reference graph is not yet transferred."
                    .to_string(),
                provenance: None,
            }],
            notes: container::summarize(scan).notes,
        },
    ))
}

fn attach_e5_edges(ir: &mut CadIr, edges: &[geometry::E5Edge], circles: &HashMap<u32, CurveId>) {
    let refs: std::collections::BTreeSet<u32> = edges
        .iter()
        .flat_map(|edge| [edge.start_vertex_id, edge.end_vertex_id])
        .collect();
    if refs.len() != ir.vertices.len() || refs.is_empty() {
        return;
    }
    let vertex_for_ref: HashMap<u32, VertexId> = refs
        .iter()
        .enumerate()
        .map(|(index, reference)| (*reference, VertexId(format!("catia:e5:v#{index}"))))
        .collect();
    // The rank mapping is admitted only when every edge is a decoded circle and
    // both mapped endpoints lie on that exact carrier.
    for edge in edges {
        let Some(curve_id) = circles.get(&edge.support_id) else {
            return;
        };
        let Some((center, radius)) = ir.curves.iter().find_map(|curve| {
            (curve.id == *curve_id)
                .then_some(match &curve.geometry {
                    CurveGeometry::Circle { center, radius, .. } => Some((*center, *radius)),
                    _ => None,
                })
                .flatten()
        }) else {
            return;
        };
        for reference in [edge.start_vertex_id, edge.end_vertex_id] {
            let Some(vertex_id) = vertex_for_ref.get(&reference) else {
                return;
            };
            let Some(point) = ir
                .vertices
                .iter()
                .find(|vertex| vertex.id == *vertex_id)
                .and_then(|vertex| ir.points.iter().find(|point| point.id == vertex.point))
            else {
                return;
            };
            let dx = point.position.x - center.x;
            let dy = point.position.y - center.y;
            let dz = point.position.z - center.z;
            if ((dx * dx + dy * dy + dz * dz).sqrt() - radius).abs() > 1e-5 {
                return;
            }
        }
    }
    for (index, edge) in edges.iter().enumerate() {
        ir.edges.push(Edge {
            id: EdgeId(format!("catia:e5:edge#{index}")),
            curve: circles.get(&edge.support_id).cloned(),
            start: vertex_for_ref[&edge.start_vertex_id].clone(),
            end: vertex_for_ref[&edge.end_vertex_id].clone(),
            param_range: None,
            tolerance: None,
            meta: byte_exact("e5_ff_edge_use", edge.pos as u64),
        });
    }
}

fn try_decode_freeform_surfaces(scan: &ContainerScan) -> Option<(CadIr, DecodeReport)> {
    let mut surfaces = geometry::a8_surfaces(&scan.data);
    surfaces.extend(geometry::a5_surfaces(&scan.data));
    if surfaces.is_empty() {
        return None;
    }
    let mut ir = CadIr::empty(Units::default());
    ir.source = Some(source_meta(scan));
    preserve_raw_payload(&mut ir, scan, "catia:freeform_payload");
    for (index, surface) in surfaces.iter().enumerate() {
        ir.surfaces.push(Surface {
            id: SurfaceId(format!("catia:a8:surf#{index}")),
            geometry: surface.geometry.clone(),
            meta: EntityMeta {
                provenance: Provenance {
                    format: "catia".to_string(),
                    stream: "object_stream_a8_03".to_string(),
                    offset: surface.pos as u64,
                    tag: Some(format!("object_id:{:08x}", surface.object_id)),
                },
                exactness: Exactness::ByteExact,
            },
        });
    }
    Some((
        ir,
        DecodeReport {
            format: "catia".to_string(),
            container_only: false,
            geometry_transferred: true,
            losses: vec![LossNote {
                category: LossCategory::Topology,
                severity: Severity::Blocking,
                message: "Object-stream and consolidated NURBS carriers were decoded, but carrier-to-face binding is not yet transferred."
                    .to_string(),
                provenance: None,
            }],
            notes: container::summarize(scan).notes,
        },
    ))
}

fn append_freeform_surface_pools(ir: &mut CadIr, data: &[u8]) {
    let mut surfaces = geometry::a8_surfaces(data);
    surfaces.extend(geometry::a5_surfaces(data));
    for surface in surfaces {
        let index = ir.surfaces.len();
        ir.surfaces.push(Surface {
            id: SurfaceId(format!("catia:freeform:surf#{index}")),
            geometry: surface.geometry,
            meta: EntityMeta {
                provenance: Provenance {
                    format: "catia".to_string(),
                    stream: "object_stream_a8_03_or_consolidated_a5_03".to_string(),
                    offset: surface.pos as u64,
                    tag: Some(format!("object_id:{:08x}", surface.object_id)),
                },
                exactness: Exactness::ByteExact,
            },
        });
    }
}

/// Decode the standard-nested vertex cloud and analytic surface carriers. Returns
/// `None` when the reconstructed stream yields neither vertices nor surfaces, so
/// the caller falls back to the container-metadata path.
fn try_decode_standard(scan: &ContainerScan) -> Option<(CadIr, DecodeReport)> {
    let brep = scan.brep.as_ref()?;
    let points = geometry::vertices(brep);
    let prefixes = geometry::surface_prefixes(brep);
    let planes: HashMap<u32, geometry::PlaneParams> = geometry::plane_params(brep)
        .into_iter()
        .map(|plane| (plane.target, plane))
        .collect();

    let mut surfaces = Vec::new();
    let mut face_bindings = Vec::new();
    let mut decoded_plane_targets = HashSet::new();
    let mut plane_faces = 0usize;
    let mut typed = TypedCounts::default();
    for (i, prefix) in prefixes.iter().enumerate() {
        // A bridged plane parameter record contains the same `00 33 32`
        // marker as its SurfacicReps carrier.  One carrier exists per tag.
        if prefix.kind == 0x32 && !decoded_plane_targets.insert(prefix.target) {
            continue;
        }
        let decoded = if prefix.kind == 0x32 {
            planes.get(&prefix.target).map(geometry::decode_plane)
        } else {
            geometry::decode_curved(brep, prefix)
        };
        match decoded {
            Some(geom) => {
                typed.record(&geom);
                let id = SurfaceId(format!("catia:surf#{i}"));
                if let Some(forward) = geometry::face_sense(brep, prefix) {
                    face_bindings.push((id.clone(), forward, prefix.pos));
                }
                surfaces.push(Surface {
                    id,
                    geometry: geom,
                    meta: byte_exact("surfacic_reps_analytic", prefix.pos as u64),
                });
            }
            None if prefix.kind == 0x32 => plane_faces += 1,
            None => {}
        }
    }

    if points.is_empty() && surfaces.is_empty() {
        return None;
    }

    let mut ir = CadIr::empty(Units::default());
    ir.source = Some(source_meta(scan));
    preserve_raw_payload(&mut ir, scan, "catia:brep_stream");

    for (i, p) in points.iter().enumerate() {
        ir.points.push(Point {
            id: PointId(format!("catia:pt#{i}")),
            position: *p,
            meta: byte_exact("vertex_05_08_01", 0),
        });
        ir.vertices.push(Vertex {
            id: VertexId(format!("catia:v#{i}")),
            point: PointId(format!("catia:pt#{i}")),
            tolerance: None,
            meta: byte_exact("vertex_05_08_01", 0),
        });
    }
    ir.surfaces = surfaces;
    attach_standard_faces(&mut ir, &face_bindings, brep);
    let topology_attached = attach_standard_topology(&mut ir, &face_bindings, brep);
    if !topology_attached {
        attach_standard_circles(&mut ir, &face_bindings, brep);
        attach_standard_lines(&mut ir, &face_bindings, brep);
    }
    append_freeform_surface_pools(&mut ir, &scan.data);

    let report = build_geometry_report(
        scan,
        points.len(),
        &typed,
        plane_faces,
        prefixes.len(),
        topology_attached,
    );
    Some((ir, report))
}

/// Attach standard analytic carriers to faces only when every FBB face has a
/// decoded carrier and its stored sense byte.  FBB runs delimit bodies.
fn attach_standard_faces(ir: &mut CadIr, bindings: &[(SurfaceId, bool, usize)], brep: &[u8]) {
    let groups = fbb_groups(brep);
    let face_count: usize = groups.iter().sum();
    if face_count == 0 || face_count != bindings.len() {
        return;
    }
    let mut face_index = 0usize;
    for (body_index, &count) in groups.iter().enumerate() {
        let body_id = BodyId(format!("catia:body#{body_index}"));
        let lump_id = LumpId(format!("catia:lump#{body_index}"));
        let shell_id = ShellId(format!("catia:shell#{body_index}"));
        let mut face_ids = Vec::with_capacity(count);
        for _ in 0..count {
            let (surface, forward, offset) = &bindings[face_index];
            let face_id = FaceId(format!("catia:face#{face_index}"));
            face_ids.push(face_id.clone());
            ir.faces.push(Face {
                id: face_id,
                shell: shell_id.clone(),
                surface: surface.clone(),
                sense: if *forward {
                    Sense::Forward
                } else {
                    Sense::Reversed
                },
                loops: Vec::new(),
                name: None,
                color: None,
                tolerance: None,
                meta: byte_exact("surfacic_reps_face_sense", *offset as u64),
            });
            face_index += 1;
        }
        ir.bodies.push(Body {
            id: body_id.clone(),
            kind: cadmpeg_ir::topology::BodyKind::Solid,
            lumps: vec![lump_id.clone()],
            transform: None,
            name: None,
            color: None,
            meta: byte_exact("fbb_body_run", 0),
        });
        ir.lumps.push(Lump {
            id: lump_id.clone(),
            body: body_id,
            shells: vec![shell_id.clone()],
            meta: byte_exact("fbb_body_run", 0),
        });
        ir.shells.push(Shell {
            id: shell_id,
            lump: lump_id,
            faces: face_ids,
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
            meta: byte_exact("fbb_face_run", 0),
        });
    }
}

fn fbb_groups(brep: &[u8]) -> Vec<usize> {
    const MARKER: &[u8; 4] = b"\x30\x04\x04\xff";
    let mut groups = Vec::new();
    let mut at = 0usize;
    while at + 8 <= brep.len() {
        if &brep[at..at + 4] != MARKER {
            at += 1;
            continue;
        }
        let mut count = 0usize;
        while at + 8 <= brep.len() && &brep[at..at + 4] == MARKER {
            count += 1;
            at += 8;
        }
        groups.push(count);
    }
    groups
}

fn attach_standard_topology(
    ir: &mut CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    brep: &[u8],
) -> bool {
    let Some(topology) = topology::parse_standard(brep) else {
        return false;
    };
    if topology.face_count() != ir.faces.len()
        || topology.vertex_points().len() != ir.points.len()
        || !topology
            .vertex_points()
            .iter()
            .zip(&ir.points)
            .all(|(stored, point)| {
                stored[0] == point.position.x
                    && stored[1] == point.position.y
                    && stored[2] == point.position.z
            })
    {
        return false;
    }
    let supports = geometry::standard_curve_supports(brep, topology.face_count());
    if supports.len() != topology.edge_rows().len() {
        return false;
    }
    let mut endpoint_pairs = Vec::with_capacity(supports.len());
    for support in &supports {
        let Some(surface0) = face_surface(ir, bindings, support.faces[0]) else {
            return false;
        };
        let Some(surface1) = face_surface(ir, bindings, support.faces[1]) else {
            return false;
        };
        let candidates: Vec<usize> = ir
            .points
            .iter()
            .enumerate()
            .filter_map(|(index, point)| {
                (point_on_surface(point.position, &surface0.geometry)
                    && point_on_surface(point.position, &surface1.geometry))
                .then_some(index)
            })
            .collect();
        let Ok(pair) = <[usize; 2]>::try_from(candidates) else {
            return false;
        };
        endpoint_pairs.push(pair);
    }
    let Some(point_assignment) = topology.bind_vertex_points(&endpoint_pairs) else {
        return false;
    };
    let Some(edge_vertices) = topology.edge_vertices() else {
        return false;
    };

    for (edge_index, (support, logical_vertices)) in supports.iter().zip(edge_vertices).enumerate()
    {
        let start_point = point_assignment[logical_vertices[0]];
        let end_point = point_assignment[logical_vertices[1]];
        let curve = build_standard_edge_curve(ir, bindings, support, start_point, end_point);
        ir.edges.push(Edge {
            id: EdgeId(format!("catia:edge#{edge_index}")),
            curve,
            start: VertexId(format!("catia:v#{start_point}")),
            end: VertexId(format!("catia:v#{end_point}")),
            param_range: None,
            tolerance: None,
            meta: byte_exact("standard_spine_edge_row", support.pos as u64),
        });
    }

    let mut edge_coedges = vec![Vec::new(); ir.edges.len()];
    for (face_index, face_topology) in topology.faces().iter().enumerate() {
        for (loop_index, boundary) in face_topology.boundaries.iter().enumerate() {
            let loop_id = LoopId(format!("catia:loop#{face_index}:{loop_index}"));
            let coedge_ids: Vec<CoedgeId> = (0..boundary.coedges.len())
                .map(|coedge_index| {
                    CoedgeId(format!(
                        "catia:coedge#{face_index}:{loop_index}:{coedge_index}"
                    ))
                })
                .collect();
            for (coedge_index, edge_use) in boundary.coedges.iter().enumerate() {
                let arena_index = ir.coedges.len();
                edge_coedges[edge_use.edge_row].push(arena_index);
                ir.coedges.push(Coedge {
                    id: coedge_ids[coedge_index].clone(),
                    owner_loop: loop_id.clone(),
                    edge: EdgeId(format!("catia:edge#{}", edge_use.edge_row)),
                    next: coedge_ids[(coedge_index + 1) % coedge_ids.len()].clone(),
                    previous: coedge_ids[(coedge_index + coedge_ids.len() - 1) % coedge_ids.len()]
                        .clone(),
                    partner: None,
                    radial_next: None,
                    sense: if edge_use.reversed {
                        Sense::Reversed
                    } else {
                        Sense::Forward
                    },
                    pcurve: None,
                    meta: byte_exact("trim_mesh_boundary_run", 0),
                });
            }
            ir.loops.push(Loop {
                id: loop_id.clone(),
                face: FaceId(format!("catia:face#{face_index}")),
                coedges: coedge_ids,
                meta: byte_exact("trim_mesh_boundary_cycle", 0),
            });
            ir.faces[face_index].loops.push(loop_id);
        }
    }
    for uses in edge_coedges {
        if let [left, right] = uses.as_slice() {
            ir.coedges[*left].partner = Some(ir.coedges[*right].id.clone());
            ir.coedges[*right].partner = Some(ir.coedges[*left].id.clone());
        }
    }
    true
}

fn face_surface<'a>(
    ir: &'a CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    face: usize,
) -> Option<&'a Surface> {
    let id = &bindings.get(face)?.0;
    ir.surfaces.iter().find(|surface| surface.id == *id)
}

fn point_on_surface(point: Point3, surface: &SurfaceGeometry) -> bool {
    const TOLERANCE: f64 = 1e-3;
    let residual = match surface {
        SurfaceGeometry::Plane { origin, normal, .. } => {
            dot_point_vector(point, *origin, *normal).abs()
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        } => {
            let axial = dot_point_vector(point, *origin, *axis);
            let radial = point_distance_squared(point, *origin) - axial * axial;
            (radial.max(0.0).sqrt() - *radius).abs()
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            radius,
            half_angle,
            ..
        } => {
            let axial = dot_point_vector(point, *origin, *axis);
            let radial = (point_distance_squared(point, *origin) - axial * axial)
                .max(0.0)
                .sqrt();
            (radial - (radius + axial * half_angle.tan()).abs()).abs()
        }
        SurfaceGeometry::Sphere { center, radius, .. } => {
            (point_distance_squared(point, *center).sqrt() - *radius).abs()
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            major_radius,
            minor_radius,
            ..
        } => {
            let axial = dot_point_vector(point, *center, *axis);
            let radial = (point_distance_squared(point, *center) - axial * axial)
                .max(0.0)
                .sqrt();
            (((radial - major_radius).powi(2) + axial * axial).sqrt() - *minor_radius).abs()
        }
        SurfaceGeometry::Nurbs(_) | SurfaceGeometry::Unknown { .. } => return false,
    };
    residual <= TOLERANCE
}

fn dot_point_vector(point: Point3, origin: Point3, vector: Vector3) -> f64 {
    (point.x - origin.x) * vector.x
        + (point.y - origin.y) * vector.y
        + (point.z - origin.z) * vector.z
}

fn point_distance_squared(left: Point3, right: Point3) -> f64 {
    (left.x - right.x).powi(2) + (left.y - right.y).powi(2) + (left.z - right.z).powi(2)
}

fn build_standard_edge_curve(
    ir: &mut CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    support: &geometry::StandardCurveSupport,
    start_point: usize,
    end_point: usize,
) -> Option<CurveId> {
    let geometry = match &support.geometry {
        geometry::StandardCurveGeometry::Line => {
            let start = ir.points[start_point].position;
            let end = ir.points[end_point].position;
            let delta = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
            let length = axis_dot(delta, delta).sqrt();
            if length <= f64::EPSILON {
                return None;
            }
            CurveGeometry::Line {
                origin: start,
                direction: Vector3::new(delta.x / length, delta.y / length, delta.z / length),
            }
        }
        geometry::StandardCurveGeometry::Circle { center, radius } => {
            let axes: Vec<Vector3> = support
                .faces
                .iter()
                .filter_map(|face| face_surface(ir, bindings, *face))
                .filter_map(surface_axis)
                .collect();
            let axis = *axes.first()?;
            if axes
                .iter()
                .skip(1)
                .any(|other| axis_dot(axis, *other).abs() < 0.9999)
            {
                return None;
            }
            CurveGeometry::Circle {
                center: *center,
                axis,
                radius: *radius,
            }
        }
        geometry::StandardCurveGeometry::Bspline => return None,
    };
    let id = CurveId(format!("catia:standard:curve#{}", support.pos));
    ir.curves.push(Curve {
        id: id.clone(),
        geometry,
        meta: byte_exact("curve_support_60", support.pos as u64),
    });
    Some(id)
}

fn attach_standard_circles(ir: &mut CadIr, bindings: &[(SurfaceId, bool, usize)], brep: &[u8]) {
    for circle in geometry::standard_circles(brep, bindings.len()) {
        let axes: Vec<Vector3> = circle
            .faces
            .iter()
            .filter_map(|face| bindings.get(*face))
            .filter_map(|(surface_id, _, _)| {
                ir.surfaces.iter().find(|surface| surface.id == *surface_id)
            })
            .filter_map(surface_axis)
            .collect();
        let Some(axis) = axes.first().copied() else {
            continue;
        };
        if axes
            .iter()
            .skip(1)
            .any(|other| axis_dot(axis, *other).abs() < 0.9999)
        {
            continue;
        }
        let index = ir.curves.len();
        ir.curves.push(Curve {
            id: CurveId(format!("catia:standard:circle#{index}")),
            geometry: CurveGeometry::Circle {
                center: circle.center,
                axis,
                radius: circle.radius,
            },
            meta: byte_exact("curve_support_60_circle", circle.pos as u64),
        });
    }
}

fn surface_axis(surface: &Surface) -> Option<Vector3> {
    match &surface.geometry {
        SurfaceGeometry::Plane { normal, .. } => Some(*normal),
        SurfaceGeometry::Cylinder { axis, .. }
        | SurfaceGeometry::Cone { axis, .. }
        | SurfaceGeometry::Torus { axis, .. } => Some(*axis),
        _ => None,
    }
}

fn axis_dot(a: Vector3, b: Vector3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn attach_standard_lines(ir: &mut CadIr, bindings: &[(SurfaceId, bool, usize)], brep: &[u8]) {
    for line in geometry::standard_lines(brep, bindings.len()) {
        let Some((origin_a, normal_a)) = plane_for_face(ir, bindings, line.faces[0]) else {
            continue;
        };
        let Some((origin_b, normal_b)) = plane_for_face(ir, bindings, line.faces[1]) else {
            continue;
        };
        let direction = cross_vector(normal_a, normal_b);
        let denom = axis_dot(direction, direction);
        if denom <= f64::EPSILON {
            continue;
        }
        let d_a = axis_dot(normal_a, Vector3::new(origin_a.x, origin_a.y, origin_a.z));
        let d_b = axis_dot(normal_b, Vector3::new(origin_b.x, origin_b.y, origin_b.z));
        let numerator = Vector3::new(
            d_a * normal_b.x - d_b * normal_a.x,
            d_a * normal_b.y - d_b * normal_a.y,
            d_a * normal_b.z - d_b * normal_a.z,
        );
        let point = cross_vector(numerator, direction);
        let index = ir.curves.len();
        ir.curves.push(Curve {
            id: CurveId(format!("catia:standard:line#{index}")),
            geometry: CurveGeometry::Line {
                origin: cadmpeg_ir::math::Point3::new(
                    point.x / denom,
                    point.y / denom,
                    point.z / denom,
                ),
                direction: Vector3::new(
                    direction.x / denom.sqrt(),
                    direction.y / denom.sqrt(),
                    direction.z / denom.sqrt(),
                ),
            },
            meta: byte_exact("curve_support_60_line", line.pos as u64),
        });
    }
}

fn plane_for_face(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    face: usize,
) -> Option<(cadmpeg_ir::math::Point3, Vector3)> {
    let (surface_id, _, _) = bindings.get(face)?;
    let surface = ir
        .surfaces
        .iter()
        .find(|surface| surface.id == *surface_id)?;
    match &surface.geometry {
        SurfaceGeometry::Plane { origin, normal, .. } => Some((*origin, *normal)),
        _ => None,
    }
}

fn cross_vector(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

/// Counts of each typed analytic surface kind decoded.
#[derive(Debug, Default)]
struct TypedCounts {
    plane: usize,
    cylinder: usize,
    cone: usize,
    sphere: usize,
    torus: usize,
}

impl TypedCounts {
    fn record(&mut self, g: &SurfaceGeometry) {
        match g {
            SurfaceGeometry::Plane { .. } => self.plane += 1,
            SurfaceGeometry::Cylinder { .. } => self.cylinder += 1,
            SurfaceGeometry::Cone { .. } => self.cone += 1,
            SurfaceGeometry::Sphere { .. } => self.sphere += 1,
            SurfaceGeometry::Torus { .. } => self.torus += 1,
            _ => {}
        }
    }

    fn total(&self) -> usize {
        self.plane + self.cylinder + self.cone + self.sphere + self.torus
    }
}

fn source_meta(scan: &ContainerScan) -> SourceMeta {
    let mut attributes = BTreeMap::new();
    attributes.insert("variant".to_string(), scan.variant.token().to_string());
    attributes.insert("file_size".to_string(), scan.data.len().to_string());
    attributes.insert(
        "outer_dir_offset".to_string(),
        scan.outer_dir_offset.to_string(),
    );
    if let Some(dir) = &scan.inner {
        attributes.insert("inner_offset".to_string(), dir.inner.to_string());
        attributes.insert(
            "stream_count".to_string(),
            dir.descriptors.len().to_string(),
        );
    }
    if let Some(brep) = &scan.brep {
        attributes.insert("brep_stream_len".to_string(), brep.len().to_string());
        attributes.insert("brep_stream_sha256".to_string(), sha256_hex(brep));
        attributes.insert("fbb_runs".to_string(), scan.census.fbb_runs.to_string());
        attributes.insert(
            "vertex_records".to_string(),
            scan.census.vertex_markers.to_string(),
        );
    }
    SourceMeta {
        format: "catia".to_string(),
        attributes,
    }
}

fn build_geometry_report(
    scan: &ContainerScan,
    vertex_count: usize,
    typed: &TypedCounts,
    plane_faces: usize,
    prefix_count: usize,
    topology_attached: bool,
) -> DecodeReport {
    let mut losses = Vec::new();

    losses.push(LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Info,
        message: format!(
            "{vertex_count} vertex point(s) were decoded verbatim from `05 08 01` records (3×f32 \
             LE, millimetres, identity world placement) and {} analytic surface carrier(s) were \
             decoded from `SurfacicReps` `00 33` records: {} plane, {} cylinder, {} cone, {} \
             sphere, {} torus.",
            typed.total(),
            typed.plane,
            typed.cylinder,
            typed.cone,
            typed.sphere,
            typed.torus
        ),
        provenance: None,
    });

    if !topology_attached {
        losses.push(LossNote {
            category: LossCategory::Topology,
            severity: Severity::Blocking,
            message: format!(
                "The B-rep boundary graph was not emitted: {} face outer-bound run(s) were \
                 detected, but a complete trim/spine/support-table parse and unique \
                 surface-constrained logical-vertex assignment were not all available.",
                scan.census.fbb_runs
            ),
            provenance: None,
        });
    }

    if plane_faces > 0 {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{plane_faces} plane surface record(s) were located but not decoded because their \
                 tag-bridged parameter records were absent or invalid."
            ),
            provenance: None,
        });
    }

    // `00 33 32` also appears inside a plane parameter record, so raw marker
    // counts cannot distinguish free-form records from bridged-plane data.
    let freeform = prefix_count.saturating_sub(typed.total() + plane_faces + typed.plane);
    if freeform > 0 {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{freeform} analytic surface record(s) had a non-finite or out-of-range inline \
                 payload and were not decoded. Free-form NURBS surface cores (the 47-byte \
                 `SurfacicReps` freeform records and the consolidated/object-stream pole grids) are \
                 not decoded on this path."
            ),
            provenance: None,
        });
    }

    losses.push(LossNote {
        category: LossCategory::Attribute,
        severity: Severity::Warning,
        message: "Standard circles with a consistent adjacent-carrier axis and plane-plane lines \
                  are transferred as curves. Spline edge curves, persistent object tags, materials, \
                  and document metadata are not yet transferred."
            .to_string(),
        provenance: None,
    });

    DecodeReport {
        format: "catia".to_string(),
        container_only: false,
        geometry_transferred: true,
        losses,
        notes: container::summarize(scan).notes,
    }
}

fn build_metadata_ir(scan: &ContainerScan) -> CadIr {
    let mut ir = CadIr::empty(Units::default());
    ir.source = Some(source_meta(scan));

    // Preserve the reconstructed BREP stream (or, absent one, the whole file) as
    // an unknown passthrough so no recognized data is silently dropped.
    if let Some(brep) = &scan.brep {
        ir.unknowns.push(UnknownRecord {
            id: UnknownId("catia:brep_stream".to_string()),
            offset: 0,
            byte_len: brep.len() as u64,
            sha256: sha256_hex(brep),
            data: Some(brep.clone()),
            links: Vec::new(),
            meta: EntityMeta {
                provenance: Provenance {
                    format: "catia".to_string(),
                    stream: "MainDataStream+SurfacicReps".to_string(),
                    offset: 0,
                    tag: Some(scan.variant.token().to_string()),
                },
                exactness: Exactness::Unknown,
            },
        });
    }
    ir
}

/// Preserve the native payload for every partial decode.  Typed entities are
/// additive views; unrecovered record families must remain byte-addressable.
fn preserve_raw_payload(ir: &mut CadIr, scan: &ContainerScan, id: &str) {
    let (bytes, stream) = match scan.brep.as_ref() {
        Some(brep) => (brep.as_slice(), "MainDataStream+SurfacicReps"),
        None => (scan.data.as_slice(), "CATPart"),
    };
    ir.unknowns.push(UnknownRecord {
        id: UnknownId(id.to_string()),
        offset: 0,
        byte_len: bytes.len() as u64,
        sha256: sha256_hex(bytes),
        data: Some(bytes.to_vec()),
        links: Vec::new(),
        meta: EntityMeta {
            provenance: Provenance {
                format: "catia".to_string(),
                stream: stream.to_string(),
                offset: 0,
                tag: Some(scan.variant.token().to_string()),
            },
            exactness: Exactness::Unknown,
        },
    });
}

fn build_container_report(scan: &ContainerScan, container_only: bool) -> DecodeReport {
    let summary = container::summarize(scan);
    let mut losses = vec![LossNote {
        category: LossCategory::Geometry,
        severity: Severity::Blocking,
        message: format!(
            "No B-rep geometry was transferred. This file's storage variant is `{}` ({}); the \
             applicable decoded record families transfer geometry in this codec.",
            scan.variant.token(),
            scan.variant.description()
        ),
        provenance: None,
    }];

    if container_only {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: "Container-only decode requested; entity decode was not attempted."
                .to_string(),
            provenance: None,
        });
    }

    losses.push(LossNote {
        category: LossCategory::Topology,
        severity: Severity::Blocking,
        message:
            "B-rep topology graph (body/lump/shell/face/loop/coedge/edge/vertex) was not built \
                  for this file."
                .to_string(),
        provenance: None,
    });

    DecodeReport {
        format: "catia".to_string(),
        container_only,
        geometry_transferred: false,
        losses,
        notes: summary.notes,
    }
}

fn byte_exact(tag: &str, offset: u64) -> EntityMeta {
    EntityMeta {
        provenance: Provenance {
            format: "catia".to_string(),
            stream: "MainDataStream+SurfacicReps".to_string(),
            offset,
            tag: Some(tag.to_string()),
        },
        exactness: Exactness::ByteExact,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        let _ = write!(s, "{b:02x}");
    }
    s
}
