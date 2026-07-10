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
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
    UnknownId, VertexId,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;
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
        return Ok(DecodeResult::new(ir, report));
    }

    if matches!(scan.variant, Variant::StandardNested | Variant::FbbOnly) {
        if let Some((ir, report)) = try_decode_standard(&scan) {
            return Ok(DecodeResult::new(ir, report));
        }
    }

    if scan.variant == Variant::ZeroEntity {
        if let Some((ir, report)) = try_decode_zero_entity(&scan) {
            return Ok(DecodeResult::new(ir, report));
        }
    }

    if scan.variant == Variant::E5Stream {
        if let Some((ir, report)) = try_decode_e5(&scan) {
            return Ok(DecodeResult::new(ir, report));
        }
    }

    if matches!(
        scan.variant,
        Variant::FloatPackedInnerNoFbb | Variant::FbbOnly | Variant::InnerNoDirectory
    ) {
        if let Some((ir, report)) = try_decode_freeform_surfaces(&scan) {
            return Ok(DecodeResult::new(ir, report));
        }
    }

    let ir = build_metadata_ir(&scan);
    let report = build_container_report(&scan, false);
    Ok(DecodeResult::new(ir, report))
}

fn annotate(
    annotations: &mut AnnotationBuilder,
    id: impl std::fmt::Display,
    stream_name: &str,
    offset: u64,
    tag: impl Into<String>,
    exactness: Exactness,
) {
    let id = id.to_string();
    let stream = annotations.stream(format!("catia:{stream_name}"));
    annotations.note(&id, stream, offset).tag(tag);
    annotations.exactness(id, exactness);
}

fn try_decode_zero_entity(scan: &ContainerScan) -> Option<(CadIr, DecodeReport)> {
    let decoded = geometry::zero_entity_surfaces(&scan.data);
    let points = geometry::vertices(&scan.data);
    if decoded.is_empty() && points.is_empty() {
        return None;
    }
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    ir.source = Some(source_meta(scan));
    preserve_raw_payload(
        &mut ir,
        &mut annotations,
        scan,
        "catia:payload:unknown#zero-entity",
    );
    for (index, point) in points.iter().enumerate() {
        let point_id = PointId(format!("catia:zero-entity:pt#{index}"));
        annotate(
            &mut annotations,
            &point_id,
            "zero_entity_a9_03",
            0,
            "vertex_05_08_01",
            Exactness::ByteExact,
        );
        ir.model.points.push(Point {
            id: point_id.clone(),
            position: *point,
        });
        let vertex_id = VertexId(format!("catia:zero-entity:v#{index}"));
        annotate(
            &mut annotations,
            &vertex_id,
            "MainDataStream+SurfacicReps",
            0,
            "vertex_05_08_01",
            Exactness::ByteExact,
        );
        annotations.derived(&vertex_id, "point");
        ir.model.vertices.push(Vertex {
            id: vertex_id,
            point: point_id,
            tolerance: None,
        });
    }
    for (index, surface) in decoded.iter().enumerate() {
        let id = SurfaceId(format!("catia:zero-entity:surf#{index}"));
        annotate(
            &mut annotations,
            &id,
            "zero_entity_a9_03",
            surface.pos as u64,
            "analytic_surface",
            Exactness::ByteExact,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: surface.geometry.clone(),
        });
    }
    link_payload_carriers(&mut ir, &mut annotations);
    ir.annotations = annotations.build();
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
    let mut annotations = AnnotationBuilder::new();
    ir.source = Some(source_meta(scan));
    preserve_raw_payload(&mut ir, &mut annotations, scan, "catia:payload:unknown#e5");
    for (index, point) in points.iter().enumerate() {
        let point_id = PointId(format!("catia:e5:pt#{index}"));
        annotate(
            &mut annotations,
            &point_id,
            "e5_0d_03",
            0,
            "vertex_05_08_01",
            Exactness::ByteExact,
        );
        ir.model.points.push(Point {
            id: point_id.clone(),
            position: *point,
        });
        let vertex_id = VertexId(format!("catia:e5:v#{index}"));
        annotate(
            &mut annotations,
            &vertex_id,
            "MainDataStream+SurfacicReps",
            0,
            "vertex_05_08_01",
            Exactness::ByteExact,
        );
        annotations.derived(&vertex_id, "point");
        ir.model.vertices.push(Vertex {
            id: vertex_id,
            point: point_id,
            tolerance: None,
        });
    }
    let mut circle_ids = HashMap::new();
    for (index, circle) in circles.iter().enumerate() {
        let id = CurveId(format!("catia:e5:curve#{index}"));
        circle_ids.insert(circle.record_id, id.clone());
        annotate(
            &mut annotations,
            &id,
            "e5_0d_03",
            circle.pos as u64,
            "circle_carrier",
            Exactness::ByteExact,
        );
        ir.model.curves.push(Curve {
            id,
            geometry: circle.geometry.clone(),
        });
    }
    for (index, surface) in surfaces.iter().enumerate() {
        let id = SurfaceId(format!("catia:e5:surf#{index}"));
        annotate(
            &mut annotations,
            &id,
            "e5_0d_03",
            surface.pos as u64,
            "analytic_surface",
            Exactness::ByteExact,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: surface.geometry.clone(),
        });
    }
    attach_e5_edges(&mut ir, &mut annotations, &edges, &circle_ids);
    if !ir.model.edges.is_empty() {
        let body_id = BodyId("catia:e5:body#0".to_string());
        let region_id = RegionId("catia:e5:region#0".to_string());
        let shell_id = ShellId("catia:e5:shell#0".to_string());
        for id in [&body_id.0, &region_id.0, &shell_id.0] {
            annotate(
                &mut annotations,
                id,
                "MainDataStream+SurfacicReps",
                0,
                "derived_wire_owner",
                Exactness::Inferred,
            );
        }
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id.clone(),
            faces: Vec::new(),
            wire_edges: ir.model.edges.iter().map(|edge| edge.id.clone()).collect(),
            free_vertices: Vec::new(),
        });
        ir.model.regions.push(Region {
            id: region_id,
            body: body_id.clone(),
            shells: vec![shell_id],
        });
        ir.model.bodies.push(Body {
            id: body_id,
            kind: BodyKind::Wire,
            regions: vec!["catia:e5:region#0".into()],
            transform: None,
            name: None,
            color: None,
        });
    }
    link_payload_carriers(&mut ir, &mut annotations);
    ir.annotations = annotations.build();
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

fn attach_e5_edges(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    edges: &[geometry::E5Edge],
    circles: &HashMap<u32, CurveId>,
) {
    let refs: std::collections::BTreeSet<u32> = edges
        .iter()
        .flat_map(|edge| [edge.start_vertex_id, edge.end_vertex_id])
        .collect();
    if refs.len() != ir.model.vertices.len() || refs.is_empty() {
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
        let Some((center, radius)) = ir.model.curves.iter().find_map(|curve| {
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
                .model
                .vertices
                .iter()
                .find(|vertex| vertex.id == *vertex_id)
                .and_then(|vertex| {
                    ir.model
                        .points
                        .iter()
                        .find(|point| point.id == vertex.point)
                })
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
        let id = EdgeId(format!("catia:e5:edge#{index}"));
        annotate(
            annotations,
            &id,
            "MainDataStream+SurfacicReps",
            edge.pos as u64,
            "e5_ff_edge_use",
            Exactness::ByteExact,
        );
        for field in ["curve", "start", "end"] {
            annotations.derived(&id, field);
        }
        ir.model.edges.push(Edge {
            id,
            curve: circles.get(&edge.support_id).cloned(),
            start: vertex_for_ref[&edge.start_vertex_id].clone(),
            end: vertex_for_ref[&edge.end_vertex_id].clone(),
            param_range: None,
            tolerance: None,
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
    let mut annotations = AnnotationBuilder::new();
    ir.source = Some(source_meta(scan));
    preserve_raw_payload(
        &mut ir,
        &mut annotations,
        scan,
        "catia:payload:unknown#freeform",
    );
    for (index, surface) in surfaces.iter().enumerate() {
        let id = SurfaceId(format!("catia:a8:surf#{index}"));
        annotate(
            &mut annotations,
            &id,
            "object_stream_a8_03",
            surface.pos as u64,
            format!("object_id:{:08x}", surface.object_id),
            Exactness::ByteExact,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: surface.geometry.clone(),
        });
    }
    link_payload_carriers(&mut ir, &mut annotations);
    ir.annotations = annotations.build();
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

fn append_freeform_surface_pools(ir: &mut CadIr, annotations: &mut AnnotationBuilder, data: &[u8]) {
    let mut surfaces = geometry::a8_surfaces(data);
    surfaces.extend(geometry::a5_surfaces(data));
    for surface in surfaces {
        let index = ir.model.surfaces.len();
        let id = SurfaceId(format!("catia:freeform:surf#{index}"));
        annotate(
            annotations,
            &id,
            "object_stream_a8_03_or_consolidated_a5_03",
            surface.pos as u64,
            format!("object_id:{:08x}", surface.object_id),
            Exactness::ByteExact,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: surface.geometry,
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
    let mut surface_annotations = Vec::new();
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
                let id = SurfaceId(format!("catia:standard:surf#{i}"));
                if let Some(forward) = geometry::face_sense(brep, prefix) {
                    face_bindings.push((id.clone(), forward, prefix.pos));
                }
                surface_annotations.push((id.clone(), prefix.pos, prefix.kind));
                surfaces.push(Surface { id, geometry: geom });
            }
            None if prefix.kind == 0x32 => plane_faces += 1,
            None => {}
        }
    }

    if points.is_empty() && surfaces.is_empty() {
        return None;
    }

    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    ir.source = Some(source_meta(scan));
    preserve_raw_payload(
        &mut ir,
        &mut annotations,
        scan,
        "catia:payload:unknown#brep-stream",
    );

    for (i, p) in points.iter().enumerate() {
        let point_id = PointId(format!("catia:standard:pt#{i}"));
        annotate(
            &mut annotations,
            &point_id,
            "MainDataStream+SurfacicReps",
            0,
            "vertex_05_08_01",
            Exactness::ByteExact,
        );
        ir.model.points.push(Point {
            id: point_id.clone(),
            position: *p,
        });
        let vertex_id = VertexId(format!("catia:standard:v#{i}"));
        annotate(
            &mut annotations,
            &vertex_id,
            "MainDataStream+SurfacicReps",
            0,
            "vertex_05_08_01",
            Exactness::ByteExact,
        );
        annotations.derived(&vertex_id, "point");
        ir.model.vertices.push(Vertex {
            id: vertex_id,
            point: point_id,
            tolerance: None,
        });
    }
    for (id, offset, kind) in surface_annotations {
        annotate(
            &mut annotations,
            &id,
            "MainDataStream+SurfacicReps",
            offset as u64,
            format!("surfacic_reps_{kind:02x}"),
            Exactness::ByteExact,
        );
    }
    ir.model.surfaces = surfaces;
    attach_standard_faces(&mut ir, &mut annotations, &face_bindings, brep);
    let topology_attached =
        attach_standard_topology(&mut ir, &mut annotations, &face_bindings, brep);
    if !topology_attached {
        attach_standard_circles(&mut ir, &mut annotations, &face_bindings, brep);
        attach_standard_lines(&mut ir, &mut annotations, &face_bindings, brep);
        if let Some(shell) = ir.model.shells.first_mut() {
            shell.free_vertices = ir
                .model
                .vertices
                .iter()
                .map(|vertex| vertex.id.clone())
                .collect();
            annotations.derived(&shell.id, "free_vertices");
        }
    }
    append_freeform_surface_pools(&mut ir, &mut annotations, &scan.data);
    link_payload_carriers(&mut ir, &mut annotations);
    ir.annotations = annotations.build();

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
fn attach_standard_faces(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    bindings: &[(SurfaceId, bool, usize)],
    brep: &[u8],
) {
    let groups = fbb_groups(brep);
    let face_count: usize = groups.iter().sum();
    if face_count == 0 || face_count != bindings.len() {
        return;
    }
    let mut face_index = 0usize;
    for (body_index, &count) in groups.iter().enumerate() {
        let body_id = BodyId(format!("catia:standard:body#{body_index}"));
        let region_id = RegionId(format!("catia:standard:region#{body_index}"));
        let shell_id = ShellId(format!("catia:standard:shell#{body_index}"));
        let mut face_ids = Vec::with_capacity(count);
        for _ in 0..count {
            let (surface, forward, offset) = &bindings[face_index];
            let face_id = FaceId(format!("catia:standard:face#{face_index}"));
            annotate(
                annotations,
                &face_id,
                "MainDataStream+SurfacicReps",
                *offset as u64,
                "surfacic_reps_face_sense",
                Exactness::ByteExact,
            );
            for field in ["shell", "surface", "sense"] {
                annotations.derived(&face_id, field);
            }
            face_ids.push(face_id.clone());
            ir.model.faces.push(Face {
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
            });
            face_index += 1;
        }
        annotate(
            annotations,
            &body_id,
            "MainDataStream+SurfacicReps",
            0,
            "fbb_body_run",
            Exactness::ByteExact,
        );
        annotations
            .derived(&body_id, "kind")
            .derived(&body_id, "regions");
        ir.model.bodies.push(Body {
            id: body_id.clone(),
            kind: cadmpeg_ir::topology::BodyKind::Solid,
            regions: vec![region_id.clone()],
            transform: None,
            name: None,
            color: None,
        });
        annotate(
            annotations,
            &region_id,
            "MainDataStream+SurfacicReps",
            0,
            "fbb_body_run",
            Exactness::ByteExact,
        );
        annotations
            .derived(&region_id, "body")
            .derived(&region_id, "shells");
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id,
            shells: vec![shell_id.clone()],
        });
        annotate(
            annotations,
            &shell_id,
            "MainDataStream+SurfacicReps",
            0,
            "fbb_face_run",
            Exactness::ByteExact,
        );
        annotations
            .derived(&shell_id, "region")
            .derived(&shell_id, "faces");
        ir.model.shells.push(Shell {
            id: shell_id,
            region: region_id,
            faces: face_ids,
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
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
    annotations: &mut AnnotationBuilder,
    bindings: &[(SurfaceId, bool, usize)],
    brep: &[u8],
) -> bool {
    let Some(topology) = topology::parse_standard(brep) else {
        return false;
    };
    if topology.face_count() != ir.model.faces.len()
        || topology.vertex_points().len() != ir.model.points.len()
        || !topology
            .vertex_points()
            .iter()
            .zip(&ir.model.points)
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
            .model
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
        let curve =
            build_standard_edge_curve(ir, annotations, bindings, support, start_point, end_point);
        let id = EdgeId(format!("catia:standard:edge#{edge_index}"));
        annotate(
            annotations,
            &id,
            "MainDataStream+SurfacicReps",
            support.pos as u64,
            "standard_spine_edge_row",
            Exactness::ByteExact,
        );
        for field in ["curve", "start", "end"] {
            annotations.derived(&id, field);
        }
        ir.model.edges.push(Edge {
            id,
            curve,
            start: VertexId(format!("catia:standard:v#{start_point}")),
            end: VertexId(format!("catia:standard:v#{end_point}")),
            param_range: None,
            tolerance: None,
        });
    }

    let mut edge_coedges = vec![Vec::new(); ir.model.edges.len()];
    for (face_index, face_topology) in topology.faces().iter().enumerate() {
        for (loop_index, boundary) in face_topology.boundaries.iter().enumerate() {
            let loop_id = LoopId(format!("catia:standard:loop#{face_index}:{loop_index}"));
            let coedge_ids: Vec<CoedgeId> = (0..boundary.coedges.len())
                .map(|coedge_index| {
                    CoedgeId(format!(
                        "catia:standard:coedge#{face_index}:{loop_index}:{coedge_index}"
                    ))
                })
                .collect();
            for (coedge_index, edge_use) in boundary.coedges.iter().enumerate() {
                let arena_index = ir.model.coedges.len();
                edge_coedges[edge_use.edge_row].push(arena_index);
                let id = coedge_ids[coedge_index].clone();
                annotate(
                    annotations,
                    &id,
                    "MainDataStream+SurfacicReps",
                    0,
                    "trim_mesh_boundary_run",
                    Exactness::ByteExact,
                );
                for field in [
                    "owner_loop",
                    "edge",
                    "next",
                    "previous",
                    "radial_next",
                    "sense",
                ] {
                    annotations.derived(&id, field);
                }
                ir.model.coedges.push(Coedge {
                    id,
                    owner_loop: loop_id.clone(),
                    edge: EdgeId(format!("catia:standard:edge#{}", edge_use.edge_row)),
                    next: coedge_ids[(coedge_index + 1) % coedge_ids.len()].clone(),
                    previous: coedge_ids[(coedge_index + coedge_ids.len() - 1) % coedge_ids.len()]
                        .clone(),
                    radial_next: coedge_ids[coedge_index].clone(),
                    sense: if edge_use.reversed {
                        Sense::Reversed
                    } else {
                        Sense::Forward
                    },
                    pcurve: None,
                });
            }
            annotate(
                annotations,
                &loop_id,
                "MainDataStream+SurfacicReps",
                0,
                "trim_mesh_boundary_cycle",
                Exactness::ByteExact,
            );
            annotations
                .derived(&loop_id, "face")
                .derived(&loop_id, "coedges");
            ir.model.loops.push(Loop {
                id: loop_id.clone(),
                face: FaceId(format!("catia:standard:face#{face_index}")),
                coedges: coedge_ids,
            });
            ir.model.faces[face_index].loops.push(loop_id);
        }
    }
    for uses in edge_coedges {
        for (position, current) in uses.iter().enumerate() {
            let next = uses[(position + 1) % uses.len()];
            ir.model.coedges[*current].radial_next = ir.model.coedges[next].id.clone();
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
    ir.model.surfaces.iter().find(|surface| surface.id == *id)
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
    annotations: &mut AnnotationBuilder,
    bindings: &[(SurfaceId, bool, usize)],
    support: &geometry::StandardCurveSupport,
    start_point: usize,
    end_point: usize,
) -> Option<CurveId> {
    let geometry = match &support.geometry {
        geometry::StandardCurveGeometry::Line => {
            let start = ir.model.points[start_point].position;
            let end = ir.model.points[end_point].position;
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
                ref_direction: cadmpeg_ir::geometry::derive_reference_direction(axis),
                radius: *radius,
            }
        }
        geometry::StandardCurveGeometry::Bspline => return None,
    };
    let id = CurveId(format!("catia:standard:curve#{}", support.pos));
    annotate(
        annotations,
        &id,
        "MainDataStream+SurfacicReps",
        support.pos as u64,
        "curve_support_60",
        Exactness::ByteExact,
    );
    if matches!(&support.geometry, geometry::StandardCurveGeometry::Line) {
        annotations
            .derived(&id, "geometry.origin")
            .derived(&id, "geometry.direction");
    } else {
        annotations.derived(&id, "geometry.axis");
    }
    ir.model.curves.push(Curve {
        id: id.clone(),
        geometry,
    });
    Some(id)
}

fn attach_standard_circles(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    bindings: &[(SurfaceId, bool, usize)],
    brep: &[u8],
) {
    for circle in geometry::standard_circles(brep, bindings.len()) {
        let axes: Vec<Vector3> = circle
            .faces
            .iter()
            .filter_map(|face| bindings.get(*face))
            .filter_map(|(surface_id, _, _)| {
                ir.model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == *surface_id)
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
        let index = ir.model.curves.len();
        let id = CurveId(format!("catia:standard:circle#{index}"));
        annotate(
            annotations,
            &id,
            "MainDataStream+SurfacicReps",
            circle.pos as u64,
            "curve_support_60_circle",
            Exactness::ByteExact,
        );
        annotations.derived(&id, "geometry.axis");
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Circle {
                center: circle.center,
                axis,
                ref_direction: cadmpeg_ir::geometry::derive_reference_direction(axis),
                radius: circle.radius,
            },
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

fn attach_standard_lines(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    bindings: &[(SurfaceId, bool, usize)],
    brep: &[u8],
) {
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
        let index = ir.model.curves.len();
        let id = CurveId(format!("catia:standard:line#{index}"));
        annotate(
            annotations,
            &id,
            "MainDataStream+SurfacicReps",
            line.pos as u64,
            "curve_support_60_line",
            Exactness::ByteExact,
        );
        annotations
            .derived(&id, "geometry.origin")
            .derived(&id, "geometry.direction");
        ir.model.curves.push(Curve {
            id,
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
        .model
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
    let mut annotations = AnnotationBuilder::new();
    ir.source = Some(source_meta(scan));

    // Preserve the reconstructed BREP stream (or, absent one, the whole file) as
    // an unknown passthrough so no recognized data is silently dropped.
    if let Some(brep) = &scan.brep {
        let id = UnknownId("catia:payload:unknown#brep-stream".to_string());
        annotate(
            &mut annotations,
            &id,
            "MainDataStream+SurfacicReps",
            0,
            scan.variant.token(),
            Exactness::Unknown,
        );
        ir.unknowns.push(UnknownRecord {
            id,
            offset: 0,
            byte_len: brep.len() as u64,
            sha256: sha256_hex(brep),
            data: Some(brep.clone()),
            links: Vec::new(),
        });
    }
    ir.annotations = annotations.build();
    ir
}

/// Preserve the native payload for every partial decode.  Typed entities are
/// additive views; unrecovered record families must remain byte-addressable.
fn preserve_raw_payload(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    scan: &ContainerScan,
    id: &str,
) {
    let (bytes, stream) = match scan.brep.as_ref() {
        Some(brep) => (brep.as_slice(), "MainDataStream+SurfacicReps"),
        None => (scan.data.as_slice(), "CATPart"),
    };
    let id = UnknownId(id.to_string());
    annotate(
        annotations,
        &id,
        stream,
        0,
        scan.variant.token(),
        Exactness::Unknown,
    );
    ir.unknowns.push(UnknownRecord {
        id,
        offset: 0,
        byte_len: bytes.len() as u64,
        sha256: sha256_hex(bytes),
        data: Some(bytes.to_vec()),
        links: Vec::new(),
    });
}

/// Attribute typed carrier views to the preserved payload when CATIA's binding
/// layer was not recovered. The raw payload is their byte-backed owner; this
/// avoids inventing topology or procedural relationships.
fn link_payload_carriers(ir: &mut CadIr, annotations: &mut AnnotationBuilder) {
    let links = ir
        .model
        .surfaces
        .iter()
        .map(|surface| surface.id.0.clone())
        .chain(ir.model.curves.iter().map(|curve| curve.id.0.clone()))
        .collect::<Vec<_>>();
    if links.is_empty() {
        return;
    }
    let payload = ir
        .unknowns
        .last_mut()
        .expect("partial CATIA decode preserves its source payload");
    payload.links = links;
    annotations.derived(&payload.id, "links");
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
            "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not built \
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
