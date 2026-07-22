// SPDX-License-Identifier: Apache-2.0
//! High-level CATPart-to-IR decoding.
//!
//! [`decode`] scans the container, selects a decoder from the identified storage
//! variant, and returns the transferred model with a [`DecodeReport`]. Standard
//! nested streams can produce connected B-rep topology when carrier senses,
//! trim cycles, support rows, and endpoint assignments all resolve. Zero-entity,
//! E5, FBB-only, and object-stream paths transfer the geometry and bindings
//! supported by their record families.
//!
//! Partial paths preserve the reconstructed B-rep stream or complete file as an
//! [`UnknownRecord`]. Their report identifies unresolved model layers.

use cadmpeg_ir::codec::{CodecError, DecodeOptions, DecodeResult, ReadSeek};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, Pcurve, PcurveGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;
use cadmpeg_ir::SourceFidelity;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::container::{self, ContainerScan};
use crate::geometry;
use crate::native::CatiaNative;
use crate::topology;
use crate::variant::Variant;

/// Decodes a `.CATPart` reader into an IR document and decode report.
///
/// When [`DecodeOptions::container_only`] is set, the result contains source
/// metadata and container diagnostics without entity decoding.
pub fn decode(
    reader: &mut dyn ReadSeek,
    options: &DecodeOptions,
) -> Result<DecodeResult, CodecError> {
    let scan = container::scan(reader)?;

    if options.container_only {
        let (ir, annotations, unknowns) = build_metadata_ir(&scan);
        let report = build_container_report(&scan, true);
        return decode_result(ir, report, annotations, &unknowns);
    }

    if matches!(scan.variant, Variant::StandardNested | Variant::FbbOnly) {
        if let Some((ir, report, annotations, unknowns)) = try_decode_standard(&scan) {
            return finish_decode(&scan, ir, report, annotations, &unknowns);
        }
    }

    if scan.variant == Variant::ZeroEntity {
        if let Some((ir, report, annotations, unknowns)) = try_decode_zero_entity(&scan) {
            return finish_decode(&scan, ir, report, annotations, &unknowns);
        }
    }

    if scan.variant == Variant::E5Stream {
        if let Some((ir, report, annotations, unknowns)) = try_decode_e5(&scan) {
            return finish_decode(&scan, ir, report, annotations, &unknowns);
        }
    }

    if matches!(
        scan.variant,
        Variant::FloatPackedInnerNoFbb | Variant::FbbOnly | Variant::InnerNoDirectory
    ) {
        if let Some((ir, report, annotations, unknowns)) = try_decode_freeform_surfaces(&scan) {
            return finish_decode(&scan, ir, report, annotations, &unknowns);
        }
    }

    let (ir, annotations, unknowns) = build_metadata_ir(&scan);
    let report = build_container_report(&scan, false);
    finish_decode(&scan, ir, report, annotations, &unknowns)
}

fn finish_decode(
    scan: &ContainerScan,
    mut ir: CadIr,
    report: DecodeReport,
    annotations: cadmpeg_ir::Annotations,
    unknowns: &[UnknownRecord],
) -> Result<DecodeResult, CodecError> {
    CatiaNative::decode(&scan.data).store(ir.native.namespace_mut("catia"))?;
    decode_result(ir, report, annotations, unknowns)
}

fn decode_result(
    mut ir: CadIr,
    report: DecodeReport,
    annotations: cadmpeg_ir::Annotations,
    unknowns: &[UnknownRecord],
) -> Result<DecodeResult, CodecError> {
    let mut source_fidelity = SourceFidelity {
        annotations,
        ..SourceFidelity::default()
    };
    source_fidelity.attach_native_unknown_records(&mut ir, "catia", unknowns)?;
    Ok(DecodeResult::with_source_fidelity(
        ir,
        report,
        source_fidelity,
    ))
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

type ProjectedDecode = (
    CadIr,
    DecodeReport,
    cadmpeg_ir::Annotations,
    Vec<UnknownRecord>,
);

fn try_decode_zero_entity(scan: &ContainerScan) -> Option<ProjectedDecode> {
    let decoded = geometry::zero_entity_surfaces(&scan.data);
    let points = geometry::vertices(&scan.data);
    if decoded.is_empty() && points.is_empty() {
        return None;
    }
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    ir.source = Some(source_meta(scan));
    preserve_raw_payload(
        &mut unknowns,
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
            source_object: None,
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
            source_object: None,
        });
    }
    link_payload_carriers(&ir, &mut unknowns, &mut annotations);
    let annotations = annotations.build();
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
    Some((ir, report, annotations, unknowns))
}

/// Decode direct E5 circle carriers.  Their edge and face references are a
/// separate record layer, so curves remain unattached until that layer is
/// decoded rather than being assigned speculatively.
fn try_decode_e5(scan: &ContainerScan) -> Option<ProjectedDecode> {
    let stream_range = container::e5_record_stream(&scan.data)?;
    let stream = &scan.data[stream_range];
    let circles = geometry::e5_circles(stream);
    let surfaces = geometry::e5_surfaces(stream);
    let topology = crate::e5::parse_topology(stream);
    let points = geometry::vertices(&scan.data);
    if circles.is_empty() && surfaces.is_empty() && points.is_empty() {
        return None;
    }
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    ir.source = Some(source_meta(scan));
    preserve_raw_payload(
        &mut unknowns,
        &mut annotations,
        scan,
        "catia:payload:unknown#e5",
    );
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
            source_object: None,
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
    for (index, circle) in circles.iter().enumerate() {
        let id = CurveId(format!("catia:e5:curve#{index}"));
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
            source_object: None,
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
            source_object: None,
        });
    }
    let topology_transferred = topology.as_ref().is_some_and(|topology| {
        transfer_e5_topology(&mut ir, &mut annotations, topology, &surfaces)
    });
    if !topology_transferred && !ir.model.vertices.is_empty() {
        attach_e5_free_vertices(&mut ir, &mut annotations);
    }
    link_payload_carriers(&ir, &mut unknowns, &mut annotations);
    let annotations = annotations.build();
    let losses = if topology_transferred {
        Vec::new()
    } else {
        vec![LossNote {
            category: LossCategory::Topology,
            severity: Severity::Blocking,
            message: "E5 analytic carriers were decoded, but the reference graph could not be transferred with a closed surface/pcurve/vertex binding."
                .to_string(),
            provenance: None,
        }]
    };
    Some((
        ir,
        DecodeReport {
            format: "catia".to_string(),
            container_only: false,
            geometry_transferred: true,
            losses,
            notes: container::summarize(scan).notes,
        },
        annotations,
        unknowns,
    ))
}

fn attach_e5_free_vertices(ir: &mut CadIr, annotations: &mut AnnotationBuilder) {
    let body_id = BodyId("catia:e5:body#unbound-points".to_string());
    let region_id = RegionId("catia:e5:region#unbound-points".to_string());
    let shell_id = ShellId("catia:e5:shell#unbound-points".to_string());
    for id in [&body_id.0, &region_id.0, &shell_id.0] {
        annotate(
            annotations,
            id,
            "e5_0d_03",
            0,
            "unbound_point_owner",
            Exactness::Inferred,
        );
    }
    ir.model.bodies.push(Body {
        id: body_id.clone(),
        kind: BodyKind::Wire,
        regions: vec![region_id.clone()],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(Region {
        id: region_id.clone(),
        body: body_id,
        shells: vec![shell_id.clone()],
    });
    ir.model.shells.push(Shell {
        id: shell_id,
        region: region_id,
        faces: Vec::new(),
        wire_edges: Vec::new(),
        free_vertices: ir
            .model
            .vertices
            .iter()
            .map(|vertex| vertex.id.clone())
            .collect(),
    });
}

fn transfer_e5_topology(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    topology: &crate::e5::E5Topology,
    decoded_surfaces: &[geometry::E5Surface],
) -> bool {
    if topology.vertex_refs.len() != ir.model.vertices.len()
        || topology.vertex_refs.len() != ir.model.points.len()
        || topology.vertex_refs.is_empty()
    {
        return false;
    }

    let surface_for_ref: HashMap<u32, (SurfaceId, &SurfaceGeometry)> = decoded_surfaces
        .iter()
        .enumerate()
        .map(|(index, surface)| {
            (
                surface.record_id,
                (
                    SurfaceId(format!("catia:e5:surf#{index}")),
                    &surface.geometry,
                ),
            )
        })
        .collect();
    let vertex_for_ref: HashMap<u32, VertexId> = topology
        .vertex_refs
        .iter()
        .enumerate()
        .map(|(index, reference)| (*reference, VertexId(format!("catia:e5:v#{index}"))))
        .collect();
    let point_for_ref: HashMap<u32, Point3> = topology
        .vertex_refs
        .iter()
        .zip(&ir.model.points)
        .map(|(reference, point)| (*reference, point.position))
        .collect();

    let mut pcurve_plan = BTreeMap::<u32, (PcurveGeometry, [f64; 2])>::new();
    for face in &topology.faces {
        let Some((_, surface)) = surface_for_ref.get(&face.surface) else {
            return false;
        };
        for loop_ in &face.loops {
            for (&pcurve_ref, &edge_ref) in loop_.pcurves.iter().zip(&loop_.edge_uses) {
                let Some(edge) = topology.edges.get(&edge_ref) else {
                    return false;
                };
                let Some(support) = topology.curve_supports.get(&edge.support) else {
                    return false;
                };
                if !support.pcurves.contains(&pcurve_ref) {
                    return false;
                }
                let Some(pcurve) = topology.pcurves.get(&pcurve_ref) else {
                    return false;
                };
                let Some((geometry, range, endpoints)) = e5_pcurve_on_surface(pcurve, surface)
                else {
                    return false;
                };
                let (Some(start), Some(end)) = (
                    point_for_ref.get(&edge.start_vertex),
                    point_for_ref.get(&edge.end_vertex),
                ) else {
                    return false;
                };
                let forward =
                    point_distance(endpoints[0], *start).max(point_distance(endpoints[1], *end));
                let reversed =
                    point_distance(endpoints[0], *end).max(point_distance(endpoints[1], *start));
                if forward.min(reversed) > 2e-3 {
                    return false;
                }
                if let Some((existing, existing_range)) = pcurve_plan.get(&pcurve_ref) {
                    if existing != &geometry || existing_range != &range {
                        return false;
                    }
                } else {
                    pcurve_plan.insert(pcurve_ref, (geometry, range));
                }
            }
        }
    }

    let body_faces: Vec<(Option<u32>, Vec<u32>)> = if topology.bodies.is_empty() {
        vec![(
            None,
            topology.faces.iter().map(|face| face.record_id).collect(),
        )]
    } else {
        topology
            .bodies
            .iter()
            .map(|body| (Some(body.record_id), body.faces.clone()))
            .collect()
    };
    let mut face_shell = HashMap::new();
    for (index, (_, faces)) in body_faces.iter().enumerate() {
        let shell = ShellId(format!("catia:e5:shell#{index}"));
        for face in faces {
            if face_shell.insert(*face, shell.clone()).is_some() {
                return false;
            }
        }
    }
    if face_shell.len() != topology.faces.len()
        || topology
            .faces
            .iter()
            .any(|face| !face_shell.contains_key(&face.record_id))
    {
        return false;
    }

    let edge_ids: HashMap<u32, EdgeId> = topology
        .edges
        .keys()
        .map(|record_id| (*record_id, EdgeId(format!("catia:e5:edge#{record_id}"))))
        .collect();
    for (&record_id, edge) in &topology.edges {
        let id = edge_ids[&record_id].clone();
        annotate(
            annotations,
            &id,
            "e5_0d_03",
            0,
            "ff_edge_use",
            Exactness::ByteExact,
        );
        for field in ["start", "end"] {
            annotations.derived(&id, field);
        }
        ir.model.edges.push(Edge {
            id,
            curve: None,
            start: vertex_for_ref[&edge.start_vertex].clone(),
            end: vertex_for_ref[&edge.end_vertex].clone(),
            param_range: None,
            tolerance: None,
        });
    }

    for (record_id, (geometry, range)) in pcurve_plan {
        let id = PcurveId(format!("catia:e5:pcurve#{record_id}"));
        annotate(
            annotations,
            &id,
            "e5_0d_03",
            0,
            "surface_parameter_curve",
            Exactness::ByteExact,
        );
        annotations.derived(&id, "geometry");
        ir.model.pcurves.push(Pcurve {
            id,
            geometry,
            wrapper_reversed: None,
            parameter_range: Some(range),
            fit_tolerance: None,
            native_tail_flags: None,
        });
    }

    for (body_index, (record_id, faces)) in body_faces.iter().enumerate() {
        let body_id = BodyId(record_id.map_or_else(
            || format!("catia:e5:body#inferred-{body_index}"),
            |id| format!("catia:e5:body#{id}"),
        ));
        let region_id = RegionId(format!("catia:e5:region#{body_index}"));
        let shell_id = ShellId(format!("catia:e5:shell#{body_index}"));
        let kind = e5_body_kind(topology, faces);
        annotate(
            annotations,
            &body_id,
            "e5_0d_03",
            0,
            "01_body",
            if record_id.is_some() {
                Exactness::ByteExact
            } else {
                Exactness::Inferred
            },
        );
        annotations
            .derived(&body_id, "kind")
            .derived(&body_id, "regions");
        ir.model.bodies.push(Body {
            id: body_id.clone(),
            kind,
            regions: vec![region_id.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        annotate(
            annotations,
            &region_id,
            "e5_0d_03",
            0,
            "derived_region",
            Exactness::Inferred,
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
            "e5_0d_03",
            0,
            "derived_shell",
            Exactness::Inferred,
        );
        annotations
            .derived(&shell_id, "region")
            .derived(&shell_id, "faces");
        ir.model.shells.push(Shell {
            id: shell_id,
            region: region_id,
            faces: faces
                .iter()
                .map(|face| FaceId(format!("catia:e5:face#{face}")))
                .collect(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
    }

    let mut coedges_by_edge = HashMap::<u32, Vec<usize>>::new();
    for face in &topology.faces {
        let face_id = FaceId(format!("catia:e5:face#{}", face.record_id));
        let loop_ids: Vec<LoopId> = face
            .loops
            .iter()
            .map(|loop_| LoopId(format!("catia:e5:loop#{}", loop_.record_id)))
            .collect();
        annotate(
            annotations,
            &face_id,
            "e5_0d_03",
            0,
            "00_advanced_face",
            Exactness::ByteExact,
        );
        for field in ["shell", "surface", "sense", "loops"] {
            annotations.derived(&face_id, field);
        }
        ir.model.faces.push(Face {
            id: face_id.clone(),
            shell: face_shell[&face.record_id].clone(),
            surface: surface_for_ref[&face.surface].0.clone(),
            sense: if face.trailer_sign > 0 {
                Sense::Forward
            } else {
                Sense::Reversed
            },
            loops: loop_ids,
            name: None,
            color: None,
            tolerance: None,
        });

        for loop_ in &face.loops {
            let loop_id = LoopId(format!("catia:e5:loop#{}", loop_.record_id));
            let coedge_ids: Vec<CoedgeId> = (0..loop_.edge_uses.len())
                .map(|index| CoedgeId(format!("catia:e5:coedge#{}-{index}", loop_.record_id)))
                .collect();
            annotate(
                annotations,
                &loop_id,
                "e5_0d_03",
                0,
                "09_loop",
                Exactness::ByteExact,
            );
            annotations
                .derived(&loop_id, "face")
                .derived(&loop_id, "coedges");
            ir.model.loops.push(Loop {
                id: loop_id.clone(),
                face: face_id.clone(),
                boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
                coedges: coedge_ids.clone(),
                vertex_uses: Vec::new(),
            });
            for (index, ((&edge_ref, &pcurve_ref), &reversed)) in loop_
                .edge_uses
                .iter()
                .zip(&loop_.pcurves)
                .zip(&loop_.reversed)
                .enumerate()
            {
                let id = coedge_ids[index].clone();
                annotate(
                    annotations,
                    &id,
                    "e5_0d_03",
                    0,
                    "serialized_loop_member",
                    Exactness::ByteExact,
                );
                for field in ["owner_loop", "edge", "next", "previous", "sense", "pcurves"] {
                    annotations.derived(&id, field);
                }
                let arena_index = ir.model.coedges.len();
                coedges_by_edge
                    .entry(edge_ref)
                    .or_default()
                    .push(arena_index);
                ir.model.coedges.push(Coedge {
                    id: id.clone(),
                    owner_loop: loop_id.clone(),
                    edge: edge_ids[&edge_ref].clone(),
                    next: coedge_ids[(index + 1) % coedge_ids.len()].clone(),
                    previous: coedge_ids[(index + coedge_ids.len() - 1) % coedge_ids.len()].clone(),
                    radial_next: id,
                    sense: if reversed {
                        Sense::Reversed
                    } else {
                        Sense::Forward
                    },
                    pcurves: vec![cadmpeg_ir::topology::PcurveUse {
                        pcurve: PcurveId(format!("catia:e5:pcurve#{pcurve_ref}")),
                        isoparametric: None,
                        parameter_range: None,
                    }],
                    use_curve: None,
                    use_curve_parameter_range: None,
                });
            }
        }
    }
    for occurrences in coedges_by_edge.values() {
        for (position, &arena_index) in occurrences.iter().enumerate() {
            let radial = occurrences[(position + 1) % occurrences.len()];
            ir.model.coedges[arena_index].radial_next = ir.model.coedges[radial].id.clone();
        }
    }
    true
}

fn e5_pcurve_on_surface(
    pcurve: &crate::e5::E5Pcurve,
    surface: &SurfaceGeometry,
) -> Option<(PcurveGeometry, [f64; 2], [Point3; 2])> {
    let crate::e5::E5Pcurve::Line {
        origin: raw_origin,
        direction,
        range,
        ..
    } = pcurve
    else {
        return None;
    };
    let origin = e5_surface_uv(surface, *raw_origin)?;
    let tip = e5_surface_uv(
        surface,
        [raw_origin[0] + direction[0], raw_origin[1] + direction[1]],
    )?;
    let direction = Point2::new(tip.u - origin.u, tip.v - origin.v);
    let uv0 = Point2::new(
        origin.u + range[0] * direction.u,
        origin.v + range[0] * direction.v,
    );
    let uv1 = Point2::new(
        origin.u + range[1] * direction.u,
        origin.v + range[1] * direction.v,
    );
    Some((
        PcurveGeometry::Line { origin, direction },
        *range,
        [
            cadmpeg_ir::eval::surface_point(surface, uv0.u, uv0.v)?,
            cadmpeg_ir::eval::surface_point(surface, uv1.u, uv1.v)?,
        ],
    ))
}

fn e5_surface_uv(surface: &SurfaceGeometry, raw: [f64; 2]) -> Option<Point2> {
    match surface {
        SurfaceGeometry::Cylinder { radius, .. } => Some(Point2::new(raw[0] / radius, raw[1])),
        SurfaceGeometry::Torus {
            major_radius,
            minor_radius,
            ..
        } => Some(Point2::new(raw[0] / major_radius, raw[1] / minor_radius)),
        _ => None,
    }
}

fn e5_body_kind(topology: &crate::e5::E5Topology, faces: &[u32]) -> BodyKind {
    let face_ids: HashSet<u32> = faces.iter().copied().collect();
    let mut uses = HashMap::<u32, usize>::new();
    for face in topology
        .faces
        .iter()
        .filter(|face| face_ids.contains(&face.record_id))
    {
        for edge in face.loops.iter().flat_map(|loop_| &loop_.edge_uses) {
            *uses.entry(*edge).or_default() += 1;
        }
    }
    if uses.values().any(|count| *count > 2) {
        BodyKind::General
    } else if !uses.is_empty() && uses.values().all(|count| *count == 2) {
        BodyKind::Solid
    } else {
        BodyKind::Sheet
    }
}

fn point_distance(a: Point3, b: Point3) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

fn try_decode_freeform_surfaces(scan: &ContainerScan) -> Option<ProjectedDecode> {
    let b5_graph = crate::b5::parse(&scan.data);
    let mut surfaces: Vec<(usize, u32, SurfaceGeometry, &str)> = geometry::a8_surfaces(&scan.data)
        .into_iter()
        .chain(geometry::a5_surfaces(&scan.data))
        .map(|surface| (surface.pos, surface.object_id, surface.geometry, "freeform"))
        .collect();
    surfaces.extend(
        geometry::b2_cylinders(&scan.data)
            .into_iter()
            .filter_map(|surface| {
                surface
                    .geometry
                    .map(|geometry| (surface.pos, 0, geometry, "b2_03_28"))
            }),
    );
    surfaces.extend(
        geometry::b2_embedded_cylinders(&scan.data)
            .into_iter()
            .filter_map(|surface| {
                surface
                    .cylinder
                    .geometry
                    .map(|geometry| (surface.pos, surface.object_id, geometry, "b2_03_60"))
            }),
    );
    surfaces.extend(geometry::b2_cones(&scan.data).into_iter().map(|surface| {
        (
            surface.pos,
            0,
            geometry::b2_cone_geometry(&surface),
            "b2_03_29",
        )
    }));
    if surfaces.is_empty() && b5_graph.is_none() {
        return None;
    }
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    ir.source = Some(source_meta(scan));
    let payload_id = UnknownId("catia:payload:unknown#freeform".to_string());
    preserve_raw_payload(&mut unknowns, &mut annotations, scan, &payload_id.0);
    let topology_transferred = b5_graph.as_ref().is_some_and(|graph| {
        crate::b5_transfer::transfer(&mut ir, &mut annotations, graph, &payload_id)
    });
    if !topology_transferred {
        for (index, (pos, object_id, geometry, kind)) in surfaces.iter().enumerate() {
            let id = SurfaceId(format!("catia:a8:surf#{index}"));
            annotate(
                &mut annotations,
                &id,
                "object_stream_a8_03",
                *pos as u64,
                format!("{kind}:object_id:{object_id:08x}"),
                Exactness::ByteExact,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry: geometry.clone(),
                source_object: None,
            });
        }
    }
    link_payload_carriers(&ir, &mut unknowns, &mut annotations);
    let annotations = annotations.build();
    Some((
        ir,
        DecodeReport {
            format: "catia".to_string(),
            container_only: false,
            geometry_transferred: true,
            losses: if topology_transferred {
                vec![LossNote {
                    category: LossCategory::Topology,
                    severity: Severity::Warning,
                    message: "The B5 reference graph is closed; face sense and body kind use a deterministic topology gauge because their source fields remain unresolved."
                        .to_string(),
                    provenance: None,
                }]
            } else {
                vec![LossNote {
                    category: LossCategory::Topology,
                    severity: Severity::Blocking,
                    message: "Object-stream and consolidated NURBS carriers were decoded, but the face/loop/pcurve/edge graph did not close."
                        .to_string(),
                    provenance: None,
                }]
            },
            notes: container::summarize(scan).notes,
        },
        annotations,
        unknowns,
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
            source_object: None,
        });
    }
}

/// Decode the standard-nested vertex cloud and analytic surface carriers. Returns
/// `None` when the reconstructed stream yields neither vertices nor surfaces, so
/// the caller falls back to the container-metadata path.
fn try_decode_standard(scan: &ContainerScan) -> Option<ProjectedDecode> {
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
                surfaces.push(Surface {
                    id,
                    geometry: geom,
                    source_object: None,
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
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    ir.source = Some(source_meta(scan));
    preserve_raw_payload(
        &mut unknowns,
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
            source_object: None,
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
    link_payload_carriers(&ir, &mut unknowns, &mut annotations);
    let annotations = annotations.build();

    let report = build_geometry_report(
        scan,
        points.len(),
        &typed,
        plane_faces,
        prefixes.len(),
        topology_attached,
    );
    Some((ir, report, annotations, unknowns))
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
            visible: None,
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
    let surface_indices = ir
        .model
        .surfaces
        .iter()
        .enumerate()
        .map(|(index, surface)| (surface.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut endpoint_pairs = Vec::with_capacity(supports.len());
    for support in &supports {
        let Some(surface0) = face_surface(ir, bindings, &surface_indices, support.faces[0]) else {
            return false;
        };
        let Some(surface1) = face_surface(ir, bindings, &surface_indices, support.faces[1]) else {
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
        let curve = build_standard_edge_curve(
            ir,
            annotations,
            bindings,
            &surface_indices,
            support,
            start_point,
            end_point,
        );
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
                    pcurves: Vec::new(),
                    use_curve: None,
                    use_curve_parameter_range: None,
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
                boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
                coedges: coedge_ids,
                vertex_uses: Vec::new(),
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
    surface_indices: &HashMap<SurfaceId, usize>,
    face: usize,
) -> Option<&'a Surface> {
    let id = &bindings.get(face)?.0;
    ir.model.surfaces.get(*surface_indices.get(id)?)
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
        SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Transformed { .. }
        | SurfaceGeometry::Unknown { .. } => return false,
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
    surface_indices: &HashMap<SurfaceId, usize>,
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
                .filter_map(|face| face_surface(ir, bindings, surface_indices, *face))
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
        source_object: None,
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
            source_object: None,
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
            source_object: None,
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

fn build_metadata_ir(scan: &ContainerScan) -> (CadIr, cadmpeg_ir::Annotations, Vec<UnknownRecord>) {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
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
        unknowns.push(UnknownRecord {
            id,
            offset: 0,
            byte_len: brep.len() as u64,
            sha256: sha256_hex(brep),
            data: Some(brep.clone()),
            links: Vec::new(),
        });
    }
    (ir, annotations.build(), unknowns)
}

/// Preserve the native payload for every partial decode.  Typed entities are
/// additive views; unrecovered record families must remain byte-addressable.
fn preserve_raw_payload(
    unknowns: &mut Vec<UnknownRecord>,
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
    unknowns.push(UnknownRecord {
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
fn link_payload_carriers(
    ir: &CadIr,
    unknowns: &mut [UnknownRecord],
    annotations: &mut AnnotationBuilder,
) {
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
    let payload = unknowns
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
