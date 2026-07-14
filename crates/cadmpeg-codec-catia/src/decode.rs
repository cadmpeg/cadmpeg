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
    Curve, CurveGeometry, Pcurve, PcurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition,
    RollingBallJetDerivative, RollingBallJetSite, Surface, SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralSurfaceId,
    RegionId, ShellId, SurfaceId, UnknownId, VertexId,
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
        let ir = build_metadata_ir(&scan)?;
        let report = build_container_report(&scan, true);
        return Ok(DecodeResult::new(ir, report));
    }

    if matches!(scan.variant, Variant::StandardNested | Variant::FbbOnly) {
        if let Some((ir, report)) = try_decode_standard(&scan) {
            return finish_decode(&scan, ir, report);
        }
    }

    if scan.variant == Variant::ZeroEntity {
        if let Some((ir, report)) = try_decode_zero_entity(&scan) {
            return finish_decode(&scan, ir, report);
        }
    }

    if scan.variant == Variant::E5Stream {
        if let Some((ir, report)) = try_decode_e5(&scan) {
            return finish_decode(&scan, ir, report);
        }
    }

    if matches!(
        scan.variant,
        Variant::FloatPackedInnerNoFbb | Variant::FbbOnly | Variant::InnerNoDirectory
    ) {
        if let Some((ir, report)) = try_decode_freeform_surfaces(&scan) {
            return finish_decode(&scan, ir, report);
        }
    }

    let ir = build_metadata_ir(&scan)?;
    let report = build_container_report(&scan, false);
    finish_decode(&scan, ir, report)
}

fn finish_decode(
    scan: &ContainerScan,
    mut ir: CadIr,
    report: DecodeReport,
) -> Result<DecodeResult, CodecError> {
    CatiaNative::decode(&scan.data).store(ir.native.namespace_mut("catia"))?;
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
    let topology = crate::zero_entity::parse(&scan.data);
    if decoded.is_empty() && points.is_empty() && topology.is_none() {
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
    )
    .ok()?;
    let topology_transferred = topology
        .as_ref()
        .is_some_and(|topology| transfer_zero_entity_topology(&mut ir, &mut annotations, topology));
    if topology_transferred {
        link_payload_carriers(&mut ir, &mut annotations).ok()?;
        ir.annotations = annotations.build();
        let unresolved_pcurves = topology
            .as_ref()
            .map(|topology| {
                topology
                    .supports
                    .iter()
                    .filter(|support| {
                        matches!(
                            topology.records[support.record_ordinal].tag,
                            [0x21, 0x45 | 0x72 | 0x9f]
                        ) && support.pcurve.is_none()
                    })
                    .count()
            })
            .unwrap_or_default();
        let losses = (unresolved_pcurves != 0)
            .then(|| LossNote {
                category: LossCategory::Topology,
                severity: Severity::Blocking,
                message: format!(
                    "The zero-entity B-rep graph and face orientation are reconstructed; {unresolved_pcurves} referenced-pole pcurve occurrences remain unresolved."
                ),
                provenance: None,
            })
            .into_iter()
            .collect();
        return Some((
            ir,
            DecodeReport {
                format: "catia".to_string(),
                container_only: false,
                geometry_transferred: true,
                losses,
                notes: container::summarize(scan).notes,
            },
        ));
    }
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
            source_object: None,
        });
    }
    link_payload_carriers(&mut ir, &mut annotations).ok()?;
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

fn transfer_zero_entity_topology(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    topology: &crate::zero_entity::ZeroEntityTopology,
) -> bool {
    const TOLERANCE: f64 = 2e-3;
    let edges = crate::zero_entity::resolve_occurrence_edges(topology);
    if edges.len() != topology.physical_edges.len()
        || topology.faces.len() != topology.carrier_runs.len()
        || topology
            .carrier_runs
            .iter()
            .any(|run| run.geometry.is_none())
    {
        return false;
    }
    let mut occurrence_edges = HashMap::new();
    for (edge_index, edge) in edges.iter().enumerate() {
        for (side, occurrence) in edge.occurrences.iter().enumerate() {
            if occurrence_edges
                .insert(
                    (occurrence.loop_index, occurrence.member_index),
                    (edge_index, side),
                )
                .is_some()
            {
                return false;
            }
        }
    }
    if topology
        .loops
        .iter()
        .enumerate()
        .any(|(loop_index, loop_)| {
            (0..loop_.member_ids.len())
                .any(|member| !occurrence_edges.contains_key(&(loop_index, member)))
        })
    {
        return false;
    }
    let loop_owner: HashMap<usize, usize> = topology
        .faces
        .iter()
        .enumerate()
        .flat_map(|(face, value)| value.loop_indices.iter().map(move |loop_| (*loop_, face)))
        .collect();
    if loop_owner.len() != topology.loops.len() {
        return false;
    }
    let mut face_parents: Vec<usize> = (0..topology.faces.len()).collect();
    for edge in &edges {
        let left = loop_owner[&edge.occurrences[0].loop_index];
        let right = loop_owner[&edge.occurrences[1].loop_index];
        union_indices(&mut face_parents, left, right);
    }
    let mut component_by_root = BTreeMap::<usize, usize>::new();
    let mut face_components = Vec::with_capacity(topology.faces.len());
    for face_index in 0..topology.faces.len() {
        let root = index_root(&mut face_parents, face_index);
        let next = component_by_root.len();
        let component = *component_by_root.entry(root).or_insert(next);
        face_components.push(component);
    }
    let component_count = component_by_root.len();

    let mut points = Vec::<Point3>::new();
    let mut edge_vertices = Vec::with_capacity(edges.len());
    for edge in &edges {
        let mut pair = [0usize; 2];
        for (slot, coordinates) in edge.endpoints.iter().enumerate() {
            let point = Point3::new(coordinates[0], coordinates[1], coordinates[2]);
            let matches: Vec<usize> = points
                .iter()
                .enumerate()
                .filter_map(|(index, existing)| {
                    (point_distance(point, *existing) <= TOLERANCE).then_some(index)
                })
                .collect();
            pair[slot] = match matches.as_slice() {
                [index] => *index,
                [] => {
                    points.push(point);
                    points.len() - 1
                }
                _ => return false,
            };
        }
        if pair[0] == pair[1] {
            return false;
        }
        edge_vertices.push(pair);
    }
    if points.len() != topology.vertices.len() {
        return false;
    }

    for (index, point) in points.into_iter().enumerate() {
        let point_id = PointId(format!("catia:zero-entity:pt#{index}"));
        let vertex_id = VertexId(format!("catia:zero-entity:v#{index}"));
        annotate(
            annotations,
            &point_id,
            "zero_entity_a9_03",
            0,
            "lifted_logical_vertex",
            Exactness::Derived,
        );
        ir.model.points.push(Point {
            id: point_id.clone(),
            position: point,
        });
        annotate(
            annotations,
            &vertex_id,
            "zero_entity_a9_03",
            0,
            "vertex_incidence_class",
            Exactness::Derived,
        );
        annotations.derived(&vertex_id, "point");
        ir.model.vertices.push(Vertex {
            id: vertex_id,
            point: point_id,
            tolerance: None,
        });
    }
    for (index, run) in topology.carrier_runs.iter().enumerate() {
        let id = SurfaceId(format!("catia:zero-entity:surf#{index}"));
        let record = &topology.records[run.carrier_ordinal];
        annotate(
            annotations,
            &id,
            "zero_entity_a9_03",
            record.offset as u64,
            "face_carrier_run",
            Exactness::ByteExact,
        );
        ir.model.surfaces.push(Surface {
            id,
            geometry: run.geometry.clone().unwrap_or_else(|| unreachable!()),
            source_object: None,
        });
    }
    for (support_index, support) in topology.supports.iter().enumerate() {
        let Some(geometry) = support.pcurve.clone() else {
            continue;
        };
        let parameter_range = match &geometry {
            PcurveGeometry::Nurbs { degree, knots, .. } => {
                let degree = usize::try_from(*degree).ok();
                degree.and_then(|degree| {
                    Some([
                        *knots.get(degree)?,
                        *knots.get(knots.len().checked_sub(degree + 1)?)?,
                    ])
                })
            }
            PcurveGeometry::Line { .. } => None,
        };
        let Some(parameter_range) = parameter_range else {
            return false;
        };
        let id = PcurveId(format!("catia:zero-entity:pcurve#{support_index}"));
        let record = &topology.records[support.record_ordinal];
        annotate(
            annotations,
            &id,
            "zero_entity_a9_03",
            record.offset as u64,
            "inline_support_pcurve",
            Exactness::Derived,
        );
        annotations
            .derived(&id, "geometry")
            .derived(&id, "parameter_range");
        ir.model.pcurves.push(Pcurve {
            id,
            geometry,
            wrapper_reversed: None,
            parameter_range: Some(parameter_range),
            fit_tolerance: None,
            native_tail_flags: None,
        });
    }

    for component in 0..component_count {
        let body_id = BodyId(format!("catia:zero-entity:body#{component}"));
        let region_id = RegionId(format!("catia:zero-entity:region#{component}"));
        let shell_id = ShellId(format!("catia:zero-entity:shell#{component}"));
        for (id, tag) in [
            (&body_id.0, "derived_body"),
            (&region_id.0, "derived_region"),
            (&shell_id.0, "derived_shell"),
        ] {
            annotate(
                annotations,
                id,
                "zero_entity_a9_03",
                0,
                tag,
                Exactness::Inferred,
            );
        }
        annotations
            .derived(&body_id, "kind")
            .derived(&body_id, "regions");
        ir.model.bodies.push(Body {
            id: body_id.clone(),
            kind: BodyKind::Solid,
            regions: vec![region_id.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        annotations
            .derived(&region_id, "body")
            .derived(&region_id, "shells");
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id,
            shells: vec![shell_id.clone()],
        });
        annotations
            .derived(&shell_id, "region")
            .derived(&shell_id, "faces");
        ir.model.shells.push(Shell {
            id: shell_id,
            region: region_id,
            faces: face_components
                .iter()
                .enumerate()
                .filter(|(_, owner)| **owner == component)
                .map(|(face, _)| FaceId(format!("catia:zero-entity:face#{face}")))
                .collect(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
    }

    for (edge_index, pair) in edge_vertices.iter().enumerate() {
        let id = EdgeId(format!("catia:zero-entity:edge#{edge_index}"));
        annotate(
            annotations,
            &id,
            "zero_entity_a9_03",
            0,
            "radial_endpoint_pair",
            Exactness::Derived,
        );
        annotations.derived(&id, "start").derived(&id, "end");
        ir.model.edges.push(Edge {
            id,
            curve: None,
            start: VertexId(format!("catia:zero-entity:v#{}", pair[0])),
            end: VertexId(format!("catia:zero-entity:v#{}", pair[1])),
            param_range: None,
            tolerance: None,
        });
    }

    for (face_index, face) in topology.faces.iter().enumerate() {
        let id = FaceId(format!("catia:zero-entity:face#{face_index}"));
        let carrier = face.carrier_run.unwrap_or(face_index);
        let outer_classes: Vec<u8> = face
            .loop_indices
            .iter()
            .filter_map(|index| {
                let loop_ = &topology.loops[*index];
                (!loop_.inner).then_some(loop_.loop_class)
            })
            .collect();
        let sense = match outer_classes.as_slice() {
            [0x41] => Sense::Forward,
            [0xc1] => Sense::Reversed,
            _ => return false,
        };
        annotate(
            annotations,
            &id,
            "zero_entity_a9_03",
            topology.records[face.record_ordinal].offset as u64,
            "face_loop_carrier_binding",
            Exactness::Derived,
        );
        for field in ["shell", "surface", "sense", "loops"] {
            annotations.derived(&id, field);
        }
        ir.model.faces.push(Face {
            id,
            shell: ShellId(format!(
                "catia:zero-entity:shell#{}",
                face_components[face_index]
            )),
            surface: SurfaceId(format!("catia:zero-entity:surf#{carrier}")),
            sense,
            loops: face
                .loop_indices
                .iter()
                .map(|index| LoopId(format!("catia:zero-entity:loop#{index}")))
                .collect(),
            name: None,
            color: None,
            tolerance: None,
        });
    }
    for (loop_index, loop_) in topology.loops.iter().enumerate() {
        let id = LoopId(format!("catia:zero-entity:loop#{loop_index}"));
        let coedges: Vec<CoedgeId> = (0..loop_.member_ids.len())
            .map(|member| CoedgeId(format!("catia:zero-entity:coedge#{loop_index}:{member}")))
            .collect();
        annotate(
            annotations,
            &id,
            "zero_entity_a9_03",
            topology.records[loop_.record_ordinal].offset as u64,
            "serialized_loop",
            Exactness::ByteExact,
        );
        annotations.derived(&id, "face").derived(&id, "coedges");
        ir.model.loops.push(Loop {
            id: id.clone(),
            face: FaceId(format!(
                "catia:zero-entity:face#{}",
                loop_owner[&loop_index]
            )),
            coedges: coedges.clone(),
        });
        for member in 0..coedges.len() {
            let (edge_index, side) = occurrence_edges[&(loop_index, member)];
            let edge = &edges[edge_index];
            let occurrence = edge.occurrence_endpoints[side];
            let reversed = point_distance(
                Point3::new(occurrence[0][0], occurrence[0][1], occurrence[0][2]),
                Point3::new(
                    edge.endpoints[1][0],
                    edge.endpoints[1][1],
                    edge.endpoints[1][2],
                ),
            ) <= TOLERANCE;
            let radial = edge.occurrences[1 - side];
            annotate(
                annotations,
                &coedges[member],
                "zero_entity_a9_03",
                topology.records[loop_.record_ordinal].offset as u64,
                "serialized_loop_member",
                Exactness::Derived,
            );
            for field in [
                "owner_loop",
                "edge",
                "next",
                "previous",
                "radial_next",
                "sense",
            ] {
                annotations.derived(&coedges[member], field);
            }
            let pcurve =
                topology.loops[loop_index].support_indices[member].and_then(|support_index| {
                    topology.supports[support_index]
                        .pcurve
                        .as_ref()
                        .map(|_| PcurveId(format!("catia:zero-entity:pcurve#{support_index}")))
                });
            if pcurve.is_some() {
                annotations.derived(&coedges[member], "pcurve");
            }
            ir.model.coedges.push(Coedge {
                id: coedges[member].clone(),
                owner_loop: id.clone(),
                edge: EdgeId(format!("catia:zero-entity:edge#{edge_index}")),
                next: coedges[(member + 1) % coedges.len()].clone(),
                previous: coedges[(member + coedges.len() - 1) % coedges.len()].clone(),
                radial_next: CoedgeId(format!(
                    "catia:zero-entity:coedge#{}:{}",
                    radial.loop_index, radial.member_index
                )),
                sense: if reversed {
                    Sense::Reversed
                } else {
                    Sense::Forward
                },
                pcurve,
            });
        }
    }
    true
}

/// Decode direct E5 circle carriers.  Their edge and face references are a
/// separate record layer, so curves remain unattached until that layer is
/// decoded rather than being assigned speculatively.
fn try_decode_e5(scan: &ContainerScan) -> Option<(CadIr, DecodeReport)> {
    let stream_range = container::e5_record_stream(&scan.data)?;
    let stream = &scan.data[stream_range];
    let circles = geometry::e5_circles(stream);
    let mut surfaces = geometry::e5_surfaces(stream);
    let topology = crate::e5::parse_topology(stream);
    let points = geometry::vertices(&scan.data);
    if let Some(topology) = &topology {
        append_e5_planes(stream, topology, &points, &mut surfaces);
    }
    if circles.is_empty() && surfaces.is_empty() && points.is_empty() {
        return None;
    }
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    ir.source = Some(source_meta(scan));
    preserve_raw_payload(&mut ir, &mut annotations, scan, "catia:payload:unknown#e5").ok()?;
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
            if matches!(surface.geometry, SurfaceGeometry::Plane { .. }) {
                Exactness::Derived
            } else {
                Exactness::ByteExact
            },
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
    link_payload_carriers(&mut ir, &mut annotations).ok()?;
    ir.annotations = annotations.build();
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
    ))
}

fn append_e5_planes(
    stream: &[u8],
    topology: &crate::e5::E5Topology,
    points: &[Point3],
    surfaces: &mut Vec<geometry::E5Surface>,
) {
    let carrier_axes: HashMap<u32, Vector3> = surfaces
        .iter()
        .filter_map(|surface| {
            let (SurfaceGeometry::Cylinder { axis, .. }
            | SurfaceGeometry::Cone { axis, .. }
            | SurfaceGeometry::Torus { axis, .. }) = surface.geometry
            else {
                return None;
            };
            Some((surface.record_id, axis))
        })
        .collect();
    for plane in geometry::e5_planes(stream) {
        let mut normal: Option<Vector3> = None;
        let mut consistent = true;
        for face in topology
            .faces
            .iter()
            .filter(|face| face.surface == plane.record_id)
        {
            for loop_ in &face.loops {
                for edge_ref in &loop_.edge_uses {
                    let Some(edge) = topology.edges.get(edge_ref) else {
                        consistent = false;
                        continue;
                    };
                    let Some(support) = topology.curve_supports.get(&edge.support) else {
                        consistent = false;
                        continue;
                    };
                    for pcurve_ref in &support.pcurves {
                        let Some(crate::e5::E5Pcurve::Line {
                            surface, direction, ..
                        }) = topology.pcurves.get(pcurve_ref)
                        else {
                            continue;
                        };
                        if direction[0].abs() <= 1e-9 || direction[1].abs() > 1e-9 {
                            continue;
                        }
                        let Some(&candidate) = carrier_axes.get(surface) else {
                            continue;
                        };
                        let candidate = canonical_direction(candidate);
                        if normal.is_some_and(|value| {
                            value.x * candidate.x + value.y * candidate.y + value.z * candidate.z
                                < 1.0 - 1e-10
                        }) {
                            consistent = false;
                        } else {
                            normal = Some(candidate);
                        }
                    }
                }
            }
        }
        let expected_normal = normal.filter(|_| consistent);
        let Some((normal, u_axis)) = solve_e5_plane_frame(
            plane.record_id,
            plane.origin,
            topology,
            points,
            expected_normal,
        ) else {
            continue;
        };
        surfaces.push(geometry::E5Surface {
            pos: plane.pos,
            record_id: plane.record_id,
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(plane.origin[0], plane.origin[1], plane.origin[2]),
                normal,
                u_axis,
            },
            uv_scale: [1.0, 1.0],
        });
    }
}

fn solve_e5_plane_frame(
    surface_ref: u32,
    origin: [f64; 3],
    topology: &crate::e5::E5Topology,
    points: &[Point3],
    expected_normal: Option<Vector3>,
) -> Option<(Vector3, Vector3)> {
    if topology.vertex_refs.len() != points.len() {
        return None;
    }
    let point_by_ref: HashMap<u32, Point3> = topology
        .vertex_refs
        .iter()
        .copied()
        .zip(points.iter().copied())
        .collect();
    let mut segments = Vec::new();
    for face in topology
        .faces
        .iter()
        .filter(|face| face.surface == surface_ref)
    {
        for loop_ in &face.loops {
            for (&pcurve_ref, &edge_ref) in loop_.pcurves.iter().zip(&loop_.edge_uses) {
                let edge = topology.edges.get(&edge_ref)?;
                let pcurve = topology.pcurves.get(&pcurve_ref)?;
                let uv = e5_native_uv_endpoints(pcurve)?;
                let (Some(start), Some(end)) = (
                    point_by_ref.get(&edge.start_vertex),
                    point_by_ref.get(&edge.end_vertex),
                ) else {
                    return None;
                };
                segments.push((uv, [*start, *end]));
            }
        }
    }
    if segments.is_empty() || segments.len() > 16 {
        return None;
    }
    let mut candidates = Vec::new();
    for mask in 0usize..(1usize << segments.len()) {
        let mut pairs = Vec::with_capacity(2 * segments.len());
        for (index, (uv, xyz)) in segments.iter().enumerate() {
            let reversed = mask & (1 << index) != 0;
            pairs.push((uv[0], xyz[usize::from(reversed)]));
            pairs.push((uv[1], xyz[usize::from(!reversed)]));
        }
        let Some((u_axis, v_axis, residual)) = fit_e5_plane_axes(origin, &pairs).or_else(|| {
            expected_normal.and_then(|normal| fit_rank_one_e5_plane_axes(origin, &pairs, normal))
        }) else {
            continue;
        };
        let Some(u_axis) = unit_vector(u_axis) else {
            continue;
        };
        let Some(v_axis) = unit_vector(v_axis) else {
            continue;
        };
        if residual > 2e-3 || scalar_product(u_axis, v_axis).abs() > 1e-6 {
            continue;
        }
        let Some(normal) = unit_vector(Vector3::new(
            u_axis.y * v_axis.z - u_axis.z * v_axis.y,
            u_axis.z * v_axis.x - u_axis.x * v_axis.z,
            u_axis.x * v_axis.y - u_axis.y * v_axis.x,
        )) else {
            continue;
        };
        if expected_normal
            .is_some_and(|expected| scalar_product(normal, expected).abs() < 1.0 - 1e-6)
        {
            continue;
        }
        if !candidates.iter().any(|(_, existing): &(Vector3, Vector3)| {
            scalar_product(*existing, u_axis) > 1.0 - 1e-8
        }) {
            candidates.push((normal, u_axis));
        }
    }
    if candidates.len() == 1 {
        return Some(candidates[0]);
    }
    let canonical: Vec<_> = candidates
        .into_iter()
        .filter(|(_, u_axis)| {
            [u_axis.x, u_axis.y, u_axis.z]
                .into_iter()
                .find(|value| value.abs() > 1e-12)
                .is_some_and(|value| value > 0.0)
        })
        .collect();
    (canonical.len() == 1).then(|| canonical[0])
}

fn e5_native_uv_endpoints(pcurve: &crate::e5::E5Pcurve) -> Option<[[f64; 2]; 2]> {
    match pcurve {
        crate::e5::E5Pcurve::Line {
            origin,
            direction,
            range,
            ..
        } => Some(range.map(|parameter| {
            [
                origin[0] + parameter * direction[0],
                origin[1] + parameter * direction[1],
            ]
        })),
        crate::e5::E5Pcurve::Circle {
            center,
            radius,
            range,
            ..
        } => Some(range.map(|parameter| {
            let angle = parameter / radius;
            [
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
            ]
        })),
        crate::e5::E5Pcurve::Jet { points, .. } => Some([*points.first()?, *points.last()?]),
    }
}

fn fit_e5_plane_axes(
    origin: [f64; 3],
    pairs: &[([f64; 2], Point3)],
) -> Option<(Vector3, Vector3, f64)> {
    let suu = pairs.iter().map(|(uv, _)| uv[0] * uv[0]).sum::<f64>();
    let suv = pairs.iter().map(|(uv, _)| uv[0] * uv[1]).sum::<f64>();
    let svv = pairs.iter().map(|(uv, _)| uv[1] * uv[1]).sum::<f64>();
    let determinant = suu * svv - suv * suv;
    if determinant.abs() <= 1e-18 {
        return None;
    }
    let mut u = [0.0; 3];
    let mut v = [0.0; 3];
    for axis in 0..3 {
        let bu = pairs
            .iter()
            .map(|(uv, point)| uv[0] * ([point.x, point.y, point.z][axis] - origin[axis]))
            .sum::<f64>();
        let bv = pairs
            .iter()
            .map(|(uv, point)| uv[1] * ([point.x, point.y, point.z][axis] - origin[axis]))
            .sum::<f64>();
        u[axis] = (bu * svv - bv * suv) / determinant;
        v[axis] = (suu * bv - suv * bu) / determinant;
    }
    let mut residual = 0.0f64;
    for (uv, point) in pairs {
        let predicted = [
            origin[0] + uv[0] * u[0] + uv[1] * v[0],
            origin[1] + uv[0] * u[1] + uv[1] * v[1],
            origin[2] + uv[0] * u[2] + uv[1] * v[2],
        ];
        residual = residual.max(
            ((predicted[0] - point.x).powi(2)
                + (predicted[1] - point.y).powi(2)
                + (predicted[2] - point.z).powi(2))
            .sqrt(),
        );
    }
    Some((
        Vector3::new(u[0], u[1], u[2]),
        Vector3::new(v[0], v[1], v[2]),
        residual,
    ))
}

fn fit_rank_one_e5_plane_axes(
    origin: [f64; 3],
    pairs: &[([f64; 2], Point3)],
    normal: Vector3,
) -> Option<(Vector3, Vector3, f64)> {
    let (uv, point) = pairs.iter().find(|(uv, _)| uv[0].hypot(uv[1]) > 1e-9)?;
    let uv_norm = uv[0].hypot(uv[1]);
    let q = [uv[0] / uv_norm, uv[1] / uv_norm];
    let displacement = Vector3::new(
        point.x - origin[0],
        point.y - origin[1],
        point.z - origin[2],
    );
    if (displacement.norm() - uv_norm).abs() > 2e-3 {
        return None;
    }
    let mapped_q = unit_vector(displacement)?;
    let mapped_r = unit_vector(Vector3::new(
        normal.y * mapped_q.z - normal.z * mapped_q.y,
        normal.z * mapped_q.x - normal.x * mapped_q.z,
        normal.x * mapped_q.y - normal.y * mapped_q.x,
    ))?;
    let u_axis = Vector3::new(
        q[0] * mapped_q.x - q[1] * mapped_r.x,
        q[0] * mapped_q.y - q[1] * mapped_r.y,
        q[0] * mapped_q.z - q[1] * mapped_r.z,
    );
    let v_axis = Vector3::new(
        q[1] * mapped_q.x + q[0] * mapped_r.x,
        q[1] * mapped_q.y + q[0] * mapped_r.y,
        q[1] * mapped_q.z + q[0] * mapped_r.z,
    );
    let residual = plane_frame_residual(origin, pairs, u_axis, v_axis);
    Some((u_axis, v_axis, residual))
}

fn plane_frame_residual(
    origin: [f64; 3],
    pairs: &[([f64; 2], Point3)],
    u_axis: Vector3,
    v_axis: Vector3,
) -> f64 {
    pairs.iter().fold(0.0f64, |residual, (uv, point)| {
        let predicted = [
            origin[0] + uv[0] * u_axis.x + uv[1] * v_axis.x,
            origin[1] + uv[0] * u_axis.y + uv[1] * v_axis.y,
            origin[2] + uv[0] * u_axis.z + uv[1] * v_axis.z,
        ];
        residual.max(
            ((predicted[0] - point.x).powi(2)
                + (predicted[1] - point.y).powi(2)
                + (predicted[2] - point.z).powi(2))
            .sqrt(),
        )
    })
}

fn scalar_product(left: Vector3, right: Vector3) -> f64 {
    left.x * right.x + left.y * right.y + left.z * right.z
}

#[cfg(test)]
mod chart_tests {
    use super::{
        build_standard_edge_curve, fit_rank_one_e5_plane_axes, intersection_line_direction,
        ordered_range, point_on_known_surface, quintic_jet_pcurve, rational_pcurve_arc,
        standard_pcurve_geometry,
    };
    use crate::geometry::{StandardCurveGeometry, StandardCurveSupport};
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::eval::pcurve_uv;
    use cadmpeg_ir::geometry::{CurveGeometry, PcurveGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::units::Units;
    use cadmpeg_ir::AnnotationBuilder;
    use std::collections::HashMap;

    #[test]
    fn rational_arc_preserves_angular_parameterization() {
        let arc =
            rational_pcurve_arc([2.0, -3.0], 4.0, [0.0, std::f64::consts::PI]).expect("semicircle");
        for (parameter, expected) in [
            (0.0, [6.0, -3.0]),
            (std::f64::consts::FRAC_PI_2, [2.0, 1.0]),
            (std::f64::consts::PI, [-2.0, -3.0]),
        ] {
            let point = pcurve_uv(&arc, parameter).expect("arc evaluation");
            assert!((point.u - expected[0]).abs() < 1e-12);
            assert!((point.v - expected[1]).abs() < 1e-12);
        }
    }

    #[test]
    fn standard_spline_edge_retains_an_unknown_curve_carrier() {
        let mut ir = CadIr::empty(Units::default());
        let mut annotations = AnnotationBuilder::new();
        let support = StandardCurveSupport {
            pos: 12,
            tag: 7,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Bspline,
        };
        let id = build_standard_edge_curve(
            &mut ir,
            &mut annotations,
            &[],
            &HashMap::new(),
            &support,
            0,
            0,
        )
        .expect("spline support identifies a curve carrier");
        assert_eq!(ir.model.curves[0].id, id);
        assert!(matches!(
            ir.model.curves[0].geometry,
            CurveGeometry::Unknown { ref record }
                if record.as_ref().is_some_and(|id| id.0 == "catia:payload:unknown#brep-stream")
        ));
    }

    #[test]
    fn reverse_angular_interval_becomes_an_increasing_nurbs_domain() {
        let range = ordered_range([0.0, -std::f64::consts::PI]);
        let arc = rational_pcurve_arc([0.0, 0.0], 2.0, range).expect("reverse semicircle");
        let PcurveGeometry::Nurbs { knots, .. } = &arc else {
            panic!("expected rational NURBS arc");
        };
        assert!(knots.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(range, [-std::f64::consts::PI, 0.0]);
        let start = pcurve_uv(&arc, range[0]).expect("start evaluation");
        let end = pcurve_uv(&arc, range[1]).expect("end evaluation");
        assert!((start.u + 2.0).abs() < 1e-12);
        assert!(start.v.abs() < 1e-12);
        assert!((end.u - 2.0).abs() < 1e-12);
        assert!(end.v.abs() < 1e-12);
    }

    #[test]
    fn quintic_jet_reproduces_endpoint_second_order_data() {
        let curve = quintic_jet_pcurve(
            5,
            &[0.0, 2.0],
            &[[0.0, 0.0], [2.0, 0.0]],
            &[[1.0, 0.0], [1.0, 0.0]],
            &[[0.0, 0.0], [0.0, 0.0]],
        )
        .expect("linear quintic segment");
        for parameter in [0.0, 0.5, 1.0, 2.0] {
            let point = pcurve_uv(&curve, parameter).expect("jet evaluation");
            assert!((point.u - parameter).abs() < 1e-12);
            assert!(point.v.abs() < 1e-12);
        }
    }

    #[test]
    fn rank_one_plane_endpoints_complete_with_known_normal() {
        let pairs = [
            ([0.0, -2.0], Point3::new(-2.0, 0.0, 0.0)),
            ([0.0, 2.0], Point3::new(2.0, 0.0, 0.0)),
        ];
        let (u_axis, v_axis, residual) =
            fit_rank_one_e5_plane_axes([0.0; 3], &pairs, Vector3::new(0.0, 1.0, 0.0))
                .expect("rank-one frame");
        assert!(residual < 1e-12);
        assert!((v_axis.x - 1.0).abs() < 1e-12);
        assert!((u_axis.z - 1.0).abs() < 1e-12);
    }

    #[test]
    fn coincident_planes_do_not_impose_a_line_direction() {
        let plane = SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        assert!(intersection_line_direction(&plane, &plane).is_none());
    }

    #[test]
    fn unknown_surface_does_not_reject_endpoint_candidates() {
        assert!(point_on_known_surface(
            Point3::new(100.0, -50.0, 7.0),
            &SurfaceGeometry::Unknown { record: None }
        ));
    }

    #[test]
    fn standard_plane_line_inverts_to_exact_parameter_line() {
        let surface = SurfaceGeometry::Plane {
            origin: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let support = StandardCurveSupport {
            pos: 0,
            tag: 1,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Line,
        };
        let (geometry, range) = standard_pcurve_geometry(
            &surface,
            &support,
            Point3::new(2.0, 4.0, 3.0),
            Point3::new(5.0, 8.0, 3.0),
        )
        .expect("plane line pcurve");
        assert_eq!(range, [0.0, 1.0]);
        assert_eq!(
            geometry,
            PcurveGeometry::Line {
                origin: cadmpeg_ir::math::Point2::new(1.0, 2.0),
                direction: cadmpeg_ir::math::Point2::new(3.0, 4.0),
            }
        );
    }

    #[test]
    fn standard_cone_latitude_inverts_to_isoparametric_line() {
        let half_angle = 0.25f64;
        let radius = 3.0 + 2.0 * half_angle.tan();
        let surface = SurfaceGeometry::Cone {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 3.0,
            ratio: 1.0,
            half_angle,
        };
        let support = StandardCurveSupport {
            pos: 0,
            tag: 1,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Circle {
                center: Point3::new(0.0, 0.0, 2.0),
                radius,
            },
        };
        let (geometry, range) = standard_pcurve_geometry(
            &surface,
            &support,
            Point3::new(radius, 0.0, 2.0),
            Point3::new(0.0, radius, 2.0),
        )
        .expect("cone latitude pcurve");
        assert_eq!(range, [0.0, 1.0]);
        assert_eq!(
            geometry,
            PcurveGeometry::Line {
                origin: cadmpeg_ir::math::Point2::new(0.0, 2.0),
                direction: cadmpeg_ir::math::Point2::new(std::f64::consts::FRAC_PI_2, 0.0),
            }
        );
    }

    #[test]
    fn standard_sphere_latitude_inverts_to_isoparametric_line() {
        let latitude = 0.4f64;
        let radius = 5.0;
        let ring = radius * latitude.cos();
        let height = radius * latitude.sin();
        let surface = SurfaceGeometry::Sphere {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius,
        };
        let support = StandardCurveSupport {
            pos: 0,
            tag: 1,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Circle {
                center: Point3::new(0.0, 0.0, height),
                radius: ring,
            },
        };
        let (geometry, _) = standard_pcurve_geometry(
            &surface,
            &support,
            Point3::new(ring, 0.0, height),
            Point3::new(0.0, ring, height),
        )
        .expect("sphere latitude pcurve");
        let PcurveGeometry::Line { origin, direction } = geometry else {
            panic!("expected line pcurve");
        };
        assert!(origin.u.abs() < 1e-12);
        assert!((origin.v - latitude).abs() < 1e-12);
        assert!((direction.u - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert!(direction.v.abs() < 1e-12);
    }
}

fn canonical_direction(mut direction: Vector3) -> Vector3 {
    let first = [direction.x, direction.y, direction.z]
        .into_iter()
        .find(|value| value.abs() > 1e-12)
        .unwrap_or(1.0);
    if first < 0.0 {
        direction = Vector3::new(-direction.x, -direction.y, -direction.z);
    }
    direction
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

    let surface_for_ref: HashMap<u32, (SurfaceId, &geometry::E5Surface)> = decoded_surfaces
        .iter()
        .enumerate()
        .map(|(index, surface)| {
            (
                surface.record_id,
                (SurfaceId(format!("catia:e5:surf#{index}")), surface),
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
        let Some((_, decoded_surface)) = surface_for_ref.get(&face.surface) else {
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
                let _ = support;
                let Some(pcurve) = topology.pcurves.get(&pcurve_ref) else {
                    return false;
                };
                let Some((geometry, range, endpoints)) =
                    e5_pcurve_on_surface(pcurve, decoded_surface)
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
                coedges: coedge_ids.clone(),
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
                for field in ["owner_loop", "edge", "next", "previous", "sense", "pcurve"] {
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
                    pcurve: Some(PcurveId(format!("catia:e5:pcurve#{pcurve_ref}"))),
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
    decoded_surface: &geometry::E5Surface,
) -> Option<(PcurveGeometry, [f64; 2], [Point3; 2])> {
    let surface = &decoded_surface.geometry;
    match pcurve {
        crate::e5::E5Pcurve::Line {
            origin: raw_origin,
            direction,
            range,
            ..
        } => {
            let origin = e5_surface_uv(decoded_surface, *raw_origin);
            let direction = Point2::new(
                direction[0] * decoded_surface.uv_scale[0],
                direction[1] * decoded_surface.uv_scale[1],
            );
            let uv = range.map(|parameter| {
                Point2::new(
                    origin.u + parameter * direction.u,
                    origin.v + parameter * direction.v,
                )
            });
            Some((
                PcurveGeometry::Line { origin, direction },
                *range,
                uv.map(|point| cadmpeg_ir::eval::surface_point(surface, point.u, point.v))
                    .into_iter()
                    .collect::<Option<Vec<_>>>()?
                    .try_into()
                    .ok()?,
            ))
        }
        crate::e5::E5Pcurve::Circle {
            center,
            radius,
            range,
            ..
        } if matches!(surface, SurfaceGeometry::Plane { .. }) => {
            let angular_range = ordered_range([range[0] / radius, range[1] / radius]);
            let geometry = rational_pcurve_arc(*center, *radius, angular_range)?;
            let endpoints = angular_range.map(|angle| {
                cadmpeg_ir::eval::surface_point(
                    surface,
                    center[0] + radius * angle.cos(),
                    center[1] + radius * angle.sin(),
                )
            });
            Some((
                geometry,
                angular_range,
                endpoints
                    .into_iter()
                    .collect::<Option<Vec<_>>>()?
                    .try_into()
                    .ok()?,
            ))
        }
        crate::e5::E5Pcurve::Jet {
            degree,
            knots,
            points,
            first_derivatives,
            second_derivatives,
            range,
            ..
        } if matches!(surface, SurfaceGeometry::Plane { .. }) => {
            let geometry = quintic_jet_pcurve(
                *degree,
                knots,
                points,
                first_derivatives,
                second_derivatives,
            )?;
            let endpoints = [*points.first()?, *points.last()?]
                .map(|uv| cadmpeg_ir::eval::surface_point(surface, uv[0], uv[1]));
            Some((
                geometry,
                *range,
                endpoints
                    .into_iter()
                    .collect::<Option<Vec<_>>>()?
                    .try_into()
                    .ok()?,
            ))
        }
        _ => None,
    }
}

fn rational_pcurve_arc(center: [f64; 2], radius: f64, range: [f64; 2]) -> Option<PcurveGeometry> {
    let span = range[1] - range[0];
    if !radius.is_finite() || radius <= 0.0 || !span.is_finite() || span.abs() <= 1e-12 {
        return None;
    }
    let segment_count = usize::try_from((span.abs() / std::f64::consts::FRAC_PI_2).ceil() as u64)
        .ok()?
        .max(1);
    let step = span / segment_count as f64;
    let mut control_points = Vec::with_capacity(2 * segment_count + 1);
    let mut weights = Vec::with_capacity(2 * segment_count + 1);
    let mut knots = vec![range[0]; 3];
    for index in 0..segment_count {
        let start = range[0] + index as f64 * step;
        let end = start + step;
        let middle = (start + end) * 0.5;
        let middle_weight = (step * 0.5).cos();
        if middle_weight.abs() <= 1e-12 {
            return None;
        }
        if index == 0 {
            control_points.push(Point2::new(
                center[0] + radius * start.cos(),
                center[1] + radius * start.sin(),
            ));
            weights.push(1.0);
        }
        control_points.push(Point2::new(
            center[0] + radius / middle_weight * middle.cos(),
            center[1] + radius / middle_weight * middle.sin(),
        ));
        control_points.push(Point2::new(
            center[0] + radius * end.cos(),
            center[1] + radius * end.sin(),
        ));
        weights.extend([middle_weight, 1.0]);
        if index + 1 < segment_count {
            knots.extend([end; 2]);
        }
    }
    knots.extend([range[1]; 3]);
    Some(PcurveGeometry::Nurbs {
        degree: 2,
        knots,
        control_points,
        weights: Some(weights),
        periodic: false,
    })
}

fn quintic_jet_pcurve(
    degree: u32,
    knots: &[f64],
    points: &[[f64; 2]],
    first: &[[f64; 2]],
    second: &[[f64; 2]],
) -> Option<PcurveGeometry> {
    let (full_knots, controls) =
        geometry::quintic_jet_bspline(degree, knots, points, first, second)?;
    Some(PcurveGeometry::Nurbs {
        degree,
        knots: full_knots,
        control_points: controls
            .into_iter()
            .map(|point| Point2::new(point[0], point[1]))
            .collect(),
        weights: None,
        periodic: false,
    })
}

fn e5_surface_uv(surface: &geometry::E5Surface, raw: [f64; 2]) -> Point2 {
    Point2::new(raw[0] * surface.uv_scale[0], raw[1] * surface.uv_scale[1])
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

fn index_root(parents: &mut [usize], mut index: usize) -> usize {
    while parents[index] != index {
        parents[index] = parents[parents[index]];
        index = parents[index];
    }
    index
}

fn union_indices(parents: &mut [usize], left: usize, right: usize) {
    let left = index_root(parents, left);
    let right = index_root(parents, right);
    parents[left] = right;
}

fn try_decode_freeform_surfaces(scan: &ContainerScan) -> Option<(CadIr, DecodeReport)> {
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
    ir.source = Some(source_meta(scan));
    let payload_id = UnknownId("catia:payload:unknown#freeform".to_string());
    preserve_raw_payload(&mut ir, &mut annotations, scan, &payload_id.0).ok()?;
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
    link_payload_carriers(&mut ir, &mut annotations).ok()?;
    ir.annotations = annotations.build();
    Some((
        ir,
        DecodeReport {
            format: "catia".to_string(),
            container_only: false,
            geometry_transferred: true,
            losses: if topology_transferred && b5_graph.as_ref().is_some_and(|graph| graph.complete)
            {
                vec![LossNote {
                    category: LossCategory::Topology,
                    severity: Severity::Warning,
                    message: "The B5 reference graph is closed; face sense and body kind use a deterministic topology gauge because their source fields remain unresolved."
                        .to_string(),
                    provenance: None,
                }]
            } else if topology_transferred {
                vec![LossNote {
                    category: LossCategory::Topology,
                    severity: Severity::Blocking,
                    message: "A maximal reference-closed B5 face/loop/pcurve/edge subset was transferred; variant nodes and unresolved endpoint lifts remain outside the connected graph."
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
    ))
}

fn append_freeform_surface_pools(ir: &mut CadIr, annotations: &mut AnnotationBuilder, data: &[u8]) {
    let mut surfaces = geometry::a8_surfaces(data);
    surfaces.extend(geometry::a5_surfaces(data));
    let mut carrier_ids = Vec::with_capacity(surfaces.len());
    for surface in &surfaces {
        let index = ir.model.surfaces.len();
        let id = SurfaceId(format!("catia:freeform:surf#{index}"));
        carrier_ids.push(id.clone());
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
            geometry: surface.geometry.clone(),
            source_object: None,
        });
    }

    let offsets = geometry::b2_offset_supports(data);
    let bindings = geometry::offset_support_carriers(&offsets, &surfaces);
    for (offset, carrier) in offsets
        .iter()
        .zip(bindings)
        .filter_map(|(offset, carrier)| Some((offset, carrier?)))
    {
        let surface_index = ir.model.surfaces.len();
        let surface_id = SurfaceId(format!("catia:offset:surf#{surface_index}"));
        annotate(
            annotations,
            &surface_id,
            "consolidated_b2_03_31_cache",
            offset.pos as u64,
            format!("support_ref:{:08x}", offset.support_id),
            Exactness::Unknown,
        );
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        });

        let procedural_id = ProceduralSurfaceId(format!(
            "catia:offset:construction#{}",
            ir.model.procedural_surfaces.len()
        ));
        annotate(
            annotations,
            &procedural_id,
            "consolidated_b2_03_31",
            offset.pos as u64,
            format!("support_ref:{:08x}", offset.support_id),
            Exactness::ByteExact,
        );
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: procedural_id,
            surface: surface_id,
            definition: ProceduralSurfaceDefinition::Offset {
                support: carrier_ids[carrier].clone(),
                distance: offset.distance,
                u_sense: 1,
                v_sense: 1,
                extension_flags: Vec::new(),
            },
            cache_fit_tolerance: None,
        });
    }

    for jet in geometry::a5_freeform_curves(data) {
        let sites = jet
            .sites
            .iter()
            .zip(&jet.first_derivatives)
            .zip(&jet.second_derivatives)
            .map(|((site, first), second)| RollingBallJetSite {
                first_limit: Point3::new(site.limit1[0], site.limit1[1], site.limit1[2]),
                second_limit: Point3::new(site.limit2[0], site.limit2[1], site.limit2[2]),
                center: Point3::new(site.center[0], site.center[1], site.center[2]),
                angle: site.theta,
                first_derivative: rolling_ball_derivative(*first),
                second_derivative: rolling_ball_derivative(*second),
            })
            .collect::<Vec<_>>();
        if sites.len() != jet.knots.len() {
            continue;
        }
        let surface_index = ir.model.surfaces.len();
        let surface_id = SurfaceId(format!("catia:rolling-ball:surf#{surface_index}"));
        annotate(
            annotations,
            &surface_id,
            "consolidated_a5_03_32_cache",
            jet.pos as u64,
            format!("header_token:{:08x}", jet.header_token),
            Exactness::Unknown,
        );
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        });

        let procedural_id = ProceduralSurfaceId(format!(
            "catia:rolling-ball:construction#{}",
            ir.model.procedural_surfaces.len()
        ));
        annotate(
            annotations,
            &procedural_id,
            "consolidated_a5_03_32",
            jet.pos as u64,
            format!("header_token:{:08x}", jet.header_token),
            Exactness::ByteExact,
        );
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: procedural_id,
            surface: surface_id,
            definition: ProceduralSurfaceDefinition::RollingBallJet {
                degree: jet.degree,
                knots: jet.knots,
                sites,
            },
            cache_fit_tolerance: None,
        });
    }
}

fn rolling_ball_derivative(values: [f64; 10]) -> RollingBallJetDerivative {
    RollingBallJetDerivative {
        first_limit: Vector3::new(values[0], values[1], values[2]),
        second_limit: Vector3::new(values[3], values[4], values[5]),
        center: Vector3::new(values[6], values[7], values[8]),
        angle: values[9],
    }
}

/// Decode the standard-nested vertex cloud and analytic surface carriers. Returns
/// `None` when the reconstructed stream yields neither vertices nor surfaces, so
/// the caller falls back to the container-metadata path.
fn try_decode_standard(scan: &ContainerScan) -> Option<(CadIr, DecodeReport)> {
    let brep = scan.brep.as_ref()?;
    let points = topology::standard_vertex_points(brep).map_or_else(
        || geometry::vertices(brep),
        |points| {
            points
                .into_iter()
                .map(|[x, y, z]| Point3::new(x, y, z))
                .collect()
        },
    );
    let face_count: usize = fbb_groups(brep).into_iter().sum();
    let records = geometry::standard_surface_records(brep, face_count).unwrap_or_else(|| {
        geometry::surface_prefixes(brep)
            .into_iter()
            .map(geometry::StandardSurfaceRecord::Analytic)
            .collect()
    });
    let analytic_record_count = records
        .iter()
        .filter(|record| matches!(record, geometry::StandardSurfaceRecord::Analytic(_)))
        .count();
    let freeform_record_count = records.len() - analytic_record_count;
    let plane_normals = topology::standard_plane_normals(brep);
    let planes: HashMap<u32, geometry::PlaneParams> = geometry::plane_params(brep, &plane_normals)
        .into_iter()
        .map(|plane| (plane.target, plane))
        .collect();

    let mut surfaces = Vec::new();
    let mut surface_annotations = Vec::new();
    let mut face_bindings = Vec::new();
    let mut decoded_plane_targets = HashSet::new();
    let mut plane_faces = 0usize;
    let mut typed = TypedCounts::default();
    for (i, record) in records.iter().enumerate() {
        let geometry::StandardSurfaceRecord::Analytic(prefix) = record else {
            let geometry::StandardSurfaceRecord::Freeform { pos, forward, .. } = record else {
                unreachable!()
            };
            let id = SurfaceId(format!("catia:standard:surf#{i}"));
            face_bindings.push((id.clone(), *forward, *pos));
            surface_annotations.push((id.clone(), *pos, None));
            surfaces.push(Surface {
                id,
                geometry: SurfaceGeometry::Unknown { record: None },
                source_object: None,
            });
            continue;
        };
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
                surface_annotations.push((id.clone(), prefix.pos, Some(prefix.kind)));
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
    ir.source = Some(source_meta(scan));
    preserve_raw_payload(
        &mut ir,
        &mut annotations,
        scan,
        "catia:payload:unknown#brep-stream",
    )
    .ok()?;

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
            kind.map_or_else(
                || "surfacic_reps_freeform_alias".to_string(),
                |kind| format!("surfacic_reps_{kind:02x}"),
            ),
            if kind.is_some() {
                Exactness::ByteExact
            } else {
                Exactness::Unknown
            },
        );
    }
    ir.model.surfaces = surfaces;
    attach_standard_faces(&mut ir, &mut annotations, &face_bindings, brep);
    let topology_attached =
        attach_standard_topology(&mut ir, &mut annotations, &face_bindings, brep, &scan.data);
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
    link_payload_carriers(&mut ir, &mut annotations).ok()?;
    ir.annotations = annotations.build();

    let report = build_geometry_report(
        scan,
        points.len(),
        &typed,
        plane_faces,
        analytic_record_count,
        freeform_record_count,
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
    source: &[u8],
) -> bool {
    let face_count = ir.model.faces.len();
    let supports = geometry::standard_curve_supports(brep, face_count);
    if supports.is_empty() {
        return false;
    }
    let surface_indices = ir
        .model
        .surfaces
        .iter()
        .enumerate()
        .map(|(index, surface)| (surface.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut endpoint_candidates = Vec::with_capacity(supports.len());
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
                let incident = match &support.geometry {
                    geometry::StandardCurveGeometry::Circle { center, radius } => {
                        (point_distance_squared(point.position, *center).sqrt() - *radius).abs()
                            <= 1e-3
                            && point_on_known_surface(point.position, &surface0.geometry)
                            && point_on_known_surface(point.position, &surface1.geometry)
                    }
                    geometry::StandardCurveGeometry::Line
                    | geometry::StandardCurveGeometry::Bspline => {
                        point_on_known_surface(point.position, &surface0.geometry)
                            && point_on_known_surface(point.position, &surface1.geometry)
                    }
                };
                incident.then_some(index)
            })
            .collect();
        endpoint_candidates.push(candidates);
    }
    let edge_faces: Vec<[usize; 2]> = supports.iter().map(|support| support.faces).collect();
    let endpoint_options = resolve_standard_endpoint_pairs(
        ir,
        bindings,
        &surface_indices,
        &supports,
        &endpoint_candidates,
    );
    let native_edges = crate::b5::edge_vertex_references(source);
    let native_ports = supports
        .iter()
        .map(|support| native_edges.get(&support.tag).copied())
        .collect::<Option<Vec<_>>>();
    let native_endpoint_pairs = endpoint_options.as_ref().and_then(|options| {
        native_ports
            .as_ref()
            .and_then(|ports| topology::bind_edge_port_candidates(ports, options))
    });
    let propagated_endpoint_pairs = endpoint_options
        .as_ref()
        .zip(topology::standard_edge_port_identities(brep))
        .and_then(|(options, ports)| {
            let pairs = options
                .iter()
                .map(|pairs| {
                    <[[usize; 2]; 1]>::try_from(pairs.as_slice())
                        .ok()
                        .map(|pair| pair[0])
                })
                .collect::<Vec<_>>();
            topology::propagate_edge_port_points(&ports, &pairs)
        })
        .zip(endpoint_options.as_ref())
        .map(|(propagated, options)| {
            propagated
                .into_iter()
                .zip(options)
                .map(|(pair, candidates)| {
                    pair.filter(|pair| {
                        candidates.iter().any(|candidate| {
                            *candidate == *pair || *candidate == [pair[1], pair[0]]
                        })
                    })
                })
                .collect::<Vec<_>>()
        });
    let mesh_propagated_endpoint_pairs = endpoint_options
        .as_ref()
        .zip(topology::standard_mesh_edge_ports(brep))
        .and_then(|(options, ports)| {
            let pairs = options
                .iter()
                .map(|pairs| {
                    <[[usize; 2]; 1]>::try_from(pairs.as_slice())
                        .ok()
                        .map(|pair| pair[0])
                })
                .collect::<Vec<_>>();
            topology::propagate_edge_port_points(&ports, &pairs)
        });
    let propagated_endpoint_pairs =
        match (propagated_endpoint_pairs, mesh_propagated_endpoint_pairs) {
            (Some(raw), Some(mesh)) => raw
                .into_iter()
                .zip(mesh)
                .map(|(raw, mesh)| match (raw, mesh) {
                    (Some(raw), Some(mesh)) if raw == mesh || raw == [mesh[1], mesh[0]] => {
                        Some(raw)
                    }
                    (Some(_), Some(_)) => None,
                    (Some(pair), None) | (None, Some(pair)) => Some(pair),
                    (None, None) => None,
                })
                .collect::<Vec<_>>(),
            (Some(pairs), None) | (None, Some(pairs)) => pairs,
            (None, None) => Vec::new(),
        };
    let propagated_endpoint_pairs =
        (!propagated_endpoint_pairs.is_empty()).then_some(propagated_endpoint_pairs);
    let constrained_endpoint_options = endpoint_options.as_ref().map(|options| {
        options
            .iter()
            .enumerate()
            .map(|(edge, pairs)| {
                propagated_endpoint_pairs
                    .as_ref()
                    .and_then(|propagated| propagated[edge])
                    .map_or_else(|| pairs.clone(), |pair| vec![pair])
            })
            .collect::<Vec<_>>()
    });
    let resolved_endpoint_pairs = propagated_endpoint_pairs
        .and_then(|pairs| pairs.into_iter().collect::<Option<Vec<[usize; 2]>>>());
    let mesh_bound = topology::parse_standard(brep)
        .or_else(|| topology::parse_fbb_with_native_vertices(brep, native_ports.as_ref()?))
        .and_then(|topology| {
            let endpoint_pairs = resolved_endpoint_pairs
                .clone()
                .or_else(|| {
                    endpoint_candidates
                        .iter()
                        .map(|candidates| <[usize; 2]>::try_from(candidates.as_slice()).ok())
                        .collect::<Option<Vec<[usize; 2]>>>()
                })
                .or_else(|| {
                    let ports = topology
                        .edge_vertices()?
                        .into_iter()
                        .map(|[left, right]| {
                            Some([u32::try_from(left).ok()?, u32::try_from(right).ok()?])
                        })
                        .collect::<Option<Vec<_>>>()?;
                    topology::bind_edge_port_candidates(
                        &ports,
                        constrained_endpoint_options.as_ref()?,
                    )
                })?;
            let point_assignment = topology.bind_vertex_points(&endpoint_pairs)?;
            Some((topology, point_assignment))
        });
    let circle_anchors: Vec<Option<[usize; 2]>> = supports
        .iter()
        .zip(&endpoint_candidates)
        .map(|(support, candidates)| match &support.geometry {
            geometry::StandardCurveGeometry::Circle { .. } => {
                <[usize; 2]>::try_from(candidates.as_slice()).ok()
            }
            geometry::StandardCurveGeometry::Line | geometry::StandardCurveGeometry::Bspline => {
                None
            }
        })
        .collect();
    let motif_topology = topology::parse_standard_motif(brep, &edge_faces, &circle_anchors);
    let (topology, point_assignment) = if let Some(bound) = mesh_bound {
        bound
    } else if let Some(topology) = native_endpoint_pairs
        .as_ref()
        .and_then(|pairs| topology::parse_standard_endpoints(brep, &edge_faces, pairs))
    {
        let point_assignment = (0..ir.model.points.len()).collect();
        (topology, point_assignment)
    } else if let Some(topology) = constrained_endpoint_options.as_ref().and_then(|options| {
        topology::parse_standard_endpoint_candidates(brep, &edge_faces, options)
    }) {
        let point_assignment = (0..ir.model.points.len()).collect();
        (topology, point_assignment)
    } else if let Some(topology) = motif_topology {
        let point_assignment = (0..ir.model.points.len()).collect();
        (topology, point_assignment)
    } else {
        return false;
    };
    if topology.face_count() != face_count
        || topology.edge_rows().len() != supports.len()
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
    let Some(edge_vertices) = topology.edge_vertices() else {
        return false;
    };
    if edge_vertices.iter().enumerate().any(|(edge, vertices)| {
        let start = point_assignment[vertices[0]];
        let end = point_assignment[vertices[1]];
        !endpoint_candidates[edge].contains(&start) || !endpoint_candidates[edge].contains(&end)
    }) {
        return false;
    }

    for (edge_index, (support, logical_vertices)) in supports.iter().zip(&edge_vertices).enumerate()
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
        if curve.is_some() {
            annotations.derived(&id, "curve");
        }
        annotations.derived(&id, "start").derived(&id, "end");
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
                let support = &supports[edge_use.edge_row];
                let logical_vertices = edge_vertices[edge_use.edge_row];
                let start = ir.model.points[point_assignment[logical_vertices[0]]].position;
                let end = ir.model.points[point_assignment[logical_vertices[1]]].position;
                let pcurve_id = standard_pcurve_geometry(
                    &ir.model.surfaces[surface_indices[&bindings[face_index].0]].geometry,
                    support,
                    start,
                    end,
                )
                .map(|(geometry, range)| {
                    let id = PcurveId(format!(
                        "catia:standard:pcurve#{face_index}:{loop_index}:{coedge_index}"
                    ));
                    annotate(
                        annotations,
                        &id,
                        "MainDataStream+SurfacicReps",
                        support.pos as u64,
                        "derived_surface_parameter_curve",
                        Exactness::Derived,
                    );
                    annotations.derived(&id, "geometry");
                    ir.model.pcurves.push(Pcurve {
                        id: id.clone(),
                        geometry,
                        wrapper_reversed: None,
                        parameter_range: Some(range),
                        fit_tolerance: None,
                        native_tail_flags: None,
                    });
                    id
                });
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
                    "pcurve",
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
                    pcurve: pcurve_id,
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

fn resolve_standard_endpoint_pairs(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    supports: &[geometry::StandardCurveSupport],
    candidates: &[Vec<usize>],
) -> Option<Vec<Vec<[usize; 2]>>> {
    const MAX_PAIR_RELATIONS_PER_EDGE: usize = 65_536;

    let mut resolved: Vec<Vec<[usize; 2]>> = candidates
        .iter()
        .map(|points| {
            <[usize; 2]>::try_from(points.as_slice())
                .map(|pair| vec![pair])
                .unwrap_or_default()
        })
        .collect();
    for (edge, support) in supports.iter().enumerate() {
        if resolved[edge].is_empty()
            && matches!(
                support.geometry,
                geometry::StandardCurveGeometry::Circle { .. }
                    | geometry::StandardCurveGeometry::Bspline
            )
        {
            let count = candidates[edge].len();
            if count
                .checked_mul(count.saturating_sub(1))
                .and_then(|value| value.checked_div(2))
                .is_some_and(|relations| relations <= MAX_PAIR_RELATIONS_PER_EDGE)
            {
                resolved[edge] = candidates[edge]
                    .iter()
                    .enumerate()
                    .flat_map(|(left, &start)| {
                        candidates[edge][left + 1..]
                            .iter()
                            .map(move |&end| [start, end])
                    })
                    .collect();
            }
        }
    }
    let mut line_groups = HashMap::<[usize; 2], Vec<usize>>::new();
    for (edge, support) in supports.iter().enumerate() {
        if resolved[edge].is_empty()
            && matches!(support.geometry, geometry::StandardCurveGeometry::Line)
        {
            let mut faces = support.faces;
            faces.sort_unstable();
            line_groups.entry(faces).or_default().push(edge);
        }
    }
    for (faces, edges) in line_groups {
        let surface0 = face_surface(ir, bindings, surface_indices, faces[0])?;
        let surface1 = face_surface(ir, bindings, surface_indices, faces[1])?;
        let direction = intersection_line_direction(&surface0.geometry, &surface1.geometry);
        let points = candidates.get(*edges.first()?)?;
        let mut pairs = Vec::new();
        for (left, &start) in points.iter().enumerate() {
            for &end_index in &points[left + 1..] {
                let start_point = ir.model.points.get(start)?.position;
                let end_point = ir.model.points.get(end_index)?.position;
                let segment = Vector3::new(
                    end_point.x - start_point.x,
                    end_point.y - start_point.y,
                    end_point.z - start_point.z,
                );
                let segment_norm = axis_dot(segment, segment).sqrt();
                let midpoint = Point3::new(
                    (start_point.x + end_point.x) * 0.5,
                    (start_point.y + end_point.y) * 0.5,
                    (start_point.z + end_point.z) * 0.5,
                );
                let follows_direction = direction.is_none_or(|direction| {
                    let direction_norm = axis_dot(direction, direction).sqrt();
                    direction_norm > f64::EPSILON
                        && axis_dot(
                            cross_vector(segment, direction),
                            cross_vector(segment, direction),
                        )
                        .sqrt()
                            <= 1e-2 * segment_norm * direction_norm
                });
                if segment_norm > f64::EPSILON
                    && follows_direction
                    && point_on_surface(midpoint, &surface0.geometry)
                    && point_on_surface(midpoint, &surface1.geometry)
                {
                    pairs.push([points[left], end_index]);
                }
            }
        }
        pairs.sort_unstable();
        pairs.dedup();
        if pairs.len() < edges.len() {
            continue;
        }
        for (rank, edge) in edges.into_iter().enumerate() {
            resolved[edge].clone_from(&pairs);
            resolved[edge].rotate_left(rank % pairs.len());
        }
    }
    Some(resolved)
}

fn intersection_line_direction(left: &SurfaceGeometry, right: &SurfaceGeometry) -> Option<Vector3> {
    match (left, right) {
        (
            SurfaceGeometry::Plane { normal: left, .. },
            SurfaceGeometry::Plane { normal: right, .. },
        ) => {
            let direction = cross_vector(*left, *right);
            (axis_dot(direction, direction) > f64::EPSILON).then_some(direction)
        }
        (SurfaceGeometry::Plane { .. }, SurfaceGeometry::Cylinder { axis, .. })
        | (SurfaceGeometry::Cylinder { axis, .. }, SurfaceGeometry::Plane { .. })
        | (SurfaceGeometry::Cylinder { axis, .. }, SurfaceGeometry::Cylinder { .. }) => Some(*axis),
        _ => None,
    }
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

fn point_on_known_surface(point: Point3, surface: &SurfaceGeometry) -> bool {
    matches!(surface, SurfaceGeometry::Unknown { .. }) || point_on_surface(point, surface)
}

fn standard_pcurve_geometry(
    surface: &SurfaceGeometry,
    support: &geometry::StandardCurveSupport,
    start: Point3,
    end: Point3,
) -> Option<(PcurveGeometry, [f64; 2])> {
    let mut uv = [
        analytic_surface_uv(surface, start)?,
        analytic_surface_uv(surface, end)?,
    ];
    let reference_uv = uv[0];
    unwrap_standard_uv(surface, &mut uv[1], reference_uv);

    if let (
        SurfaceGeometry::Plane { .. },
        geometry::StandardCurveGeometry::Circle { center, radius },
    ) = (surface, &support.geometry)
    {
        let center_uv = analytic_surface_uv(surface, *center)?;
        let range = uv.map(|point| (point.v - center_uv.v).atan2(point.u - center_uv.u));
        let range = ordered_range([range[0], unwrap_angle(range[1], range[0])]);
        let geometry = rational_pcurve_arc([center_uv.u, center_uv.v], *radius, range)?;
        return Some((geometry, range));
    }

    let direction = Point2::new(uv[1].u - uv[0].u, uv[1].v - uv[0].v);
    let midpoint_uv = Point2::new(uv[0].u + 0.5 * direction.u, uv[0].v + 0.5 * direction.v);
    let midpoint = cadmpeg_ir::eval::surface_point(surface, midpoint_uv.u, midpoint_uv.v)?;
    let on_curve = match &support.geometry {
        geometry::StandardCurveGeometry::Line => {
            let chord = point_delta(end, start);
            let offset = point_delta(midpoint, start);
            vector_norm(cross_vector(chord, offset)) <= 2e-3 * vector_norm(chord).max(1.0)
        }
        geometry::StandardCurveGeometry::Circle { center, radius } => {
            (point_distance_squared(midpoint, *center).sqrt() - radius).abs() <= 2e-3
        }
        geometry::StandardCurveGeometry::Bspline => false,
    };
    on_curve.then_some((
        PcurveGeometry::Line {
            origin: uv[0],
            direction,
        },
        [0.0, 1.0],
    ))
}

fn ordered_range(range: [f64; 2]) -> [f64; 2] {
    if range[0] <= range[1] {
        range
    } else {
        [range[1], range[0]]
    }
}

fn analytic_surface_uv(surface: &SurfaceGeometry, point: Point3) -> Option<Point2> {
    match surface {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            let offset = point_delta(point, *origin);
            let v_axis = cross_vector(*normal, *u_axis);
            Some(Point2::new(
                axis_dot(offset, *u_axis),
                axis_dot(offset, v_axis),
            ))
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            ..
        } => {
            let offset = point_delta(point, *origin);
            let tangent = cross_vector(*axis, *ref_direction);
            Some(Point2::new(
                axis_dot(offset, tangent).atan2(axis_dot(offset, *ref_direction)),
                axis_dot(offset, *axis),
            ))
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            ratio,
            ..
        } => {
            if ratio.abs() <= f64::EPSILON {
                return None;
            }
            let offset = point_delta(point, *origin);
            let tangent = cross_vector(*axis, *ref_direction);
            Some(Point2::new(
                (axis_dot(offset, tangent) / ratio).atan2(axis_dot(offset, *ref_direction)),
                axis_dot(offset, *axis),
            ))
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            if *radius <= f64::EPSILON {
                return None;
            }
            let offset = point_delta(point, *center);
            let tangent = cross_vector(*axis, *ref_direction);
            Some(Point2::new(
                axis_dot(offset, tangent).atan2(axis_dot(offset, *ref_direction)),
                (axis_dot(offset, *axis) / radius).clamp(-1.0, 1.0).asin(),
            ))
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            ..
        } => {
            let offset = point_delta(point, *center);
            let tangent = cross_vector(*axis, *ref_direction);
            let u = axis_dot(offset, tangent).atan2(axis_dot(offset, *ref_direction));
            let radial = Vector3::new(
                u.cos() * ref_direction.x + u.sin() * tangent.x,
                u.cos() * ref_direction.y + u.sin() * tangent.y,
                u.cos() * ref_direction.z + u.sin() * tangent.z,
            );
            Some(Point2::new(
                u,
                axis_dot(offset, *axis).atan2(axis_dot(offset, radial) - major_radius),
            ))
        }
        _ => None,
    }
}

fn unwrap_standard_uv(surface: &SurfaceGeometry, value: &mut Point2, reference: Point2) {
    match surface {
        SurfaceGeometry::Cylinder { .. }
        | SurfaceGeometry::Cone { .. }
        | SurfaceGeometry::Sphere { .. } => value.u = unwrap_angle(value.u, reference.u),
        SurfaceGeometry::Torus { .. } => {
            value.u = unwrap_angle(value.u, reference.u);
            value.v = unwrap_angle(value.v, reference.v);
        }
        _ => {}
    }
}

fn unwrap_angle(value: f64, reference: f64) -> f64 {
    reference + (value - reference + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
        - std::f64::consts::PI
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
                .filter_map(|surface| circle_axis_from_carrier(*center, *radius, &surface.geometry))
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
        geometry::StandardCurveGeometry::Bspline => CurveGeometry::Unknown {
            record: Some(UnknownId("catia:payload:unknown#brep-stream".to_string())),
        },
    };
    let id = CurveId(format!("catia:standard:curve#{}", support.pos));
    annotate(
        annotations,
        &id,
        "MainDataStream+SurfacicReps",
        support.pos as u64,
        "curve_support_60",
        if matches!(geometry, CurveGeometry::Unknown { .. }) {
            Exactness::Unknown
        } else {
            Exactness::ByteExact
        },
    );
    if matches!(&support.geometry, geometry::StandardCurveGeometry::Line) {
        annotations
            .derived(&id, "geometry.origin")
            .derived(&id, "geometry.direction");
    } else if matches!(
        &support.geometry,
        geometry::StandardCurveGeometry::Circle { .. }
    ) {
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
            .filter_map(|surface| {
                circle_axis_from_carrier(circle.center, circle.radius, &surface.geometry)
            })
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

fn circle_axis_from_carrier(
    center: Point3,
    circle_radius: f64,
    surface: &SurfaceGeometry,
) -> Option<Vector3> {
    match surface {
        SurfaceGeometry::Plane { origin, normal, .. } => {
            close_length(dot_point_vector(center, *origin, *normal), 0.0).then_some(*normal)
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        } => {
            let offset = point_delta(center, *origin);
            let axial = axis_dot(offset, *axis);
            let radial = subtract_vector(offset, scale_vector(*axis, axial));
            (close_length(vector_norm(radial), 0.0) && close_length(circle_radius, *radius))
                .then_some(*axis)
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            radius,
            half_angle,
            ..
        } => {
            let offset = point_delta(center, *origin);
            let axial = axis_dot(offset, *axis);
            let radial = subtract_vector(offset, scale_vector(*axis, axial));
            let section_radius = (radius + axial * half_angle.tan()).abs();
            (close_length(vector_norm(radial), 0.0) && close_length(circle_radius, section_radius))
                .then_some(*axis)
        }
        SurfaceGeometry::Sphere {
            center: sphere_center,
            radius: sphere_radius,
            ..
        } => {
            let offset = point_delta(center, *sphere_center);
            let distance = vector_norm(offset);
            (distance > f64::EPSILON
                && close_squared(
                    distance * distance + circle_radius * circle_radius,
                    sphere_radius * sphere_radius,
                ))
            .then(|| scale_vector(offset, 1.0 / distance))
        }
        SurfaceGeometry::Torus {
            center: torus_center,
            axis,
            major_radius,
            minor_radius,
            ..
        } => {
            let offset = point_delta(center, *torus_center);
            let axial = axis_dot(offset, *axis);
            let radial = subtract_vector(offset, scale_vector(*axis, axial));
            let radial_distance = vector_norm(radial);
            if close_length(axial, 0.0)
                && close_length(radial_distance, *major_radius)
                && close_length(circle_radius, *minor_radius)
            {
                unit_vector(cross_vector(*axis, radial))
            } else if close_length(radial_distance, 0.0)
                && close_squared(
                    (circle_radius - major_radius).powi(2) + axial * axial,
                    minor_radius * minor_radius,
                )
            {
                Some(*axis)
            } else {
                None
            }
        }
        SurfaceGeometry::Nurbs(_) | SurfaceGeometry::Unknown { .. } => None,
    }
}

fn point_delta(left: Point3, right: Point3) -> Vector3 {
    Vector3::new(left.x - right.x, left.y - right.y, left.z - right.z)
}

fn subtract_vector(left: Vector3, right: Vector3) -> Vector3 {
    Vector3::new(left.x - right.x, left.y - right.y, left.z - right.z)
}

fn scale_vector(vector: Vector3, scalar: f64) -> Vector3 {
    Vector3::new(vector.x * scalar, vector.y * scalar, vector.z * scalar)
}

fn vector_norm(vector: Vector3) -> f64 {
    axis_dot(vector, vector).sqrt()
}

fn unit_vector(vector: Vector3) -> Option<Vector3> {
    let norm = vector_norm(vector);
    (norm > f64::EPSILON).then(|| scale_vector(vector, 1.0 / norm))
}

fn close_length(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-5 * (1.0 + left.abs().max(right.abs()))
}

fn close_squared(left: f64, right: f64) -> bool {
    (left - right).abs() <= 2e-5 * (1.0 + left.abs().max(right.abs()))
}

#[cfg(test)]
mod circle_axis_tests {
    use super::circle_axis_from_carrier;
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};

    fn x() -> Vector3 {
        Vector3::new(1.0, 0.0, 0.0)
    }

    fn y() -> Vector3 {
        Vector3::new(0.0, 1.0, 0.0)
    }

    fn z() -> Vector3 {
        Vector3::new(0.0, 0.0, 1.0)
    }

    fn origin() -> Point3 {
        Point3::new(0.0, 0.0, 0.0)
    }

    #[test]
    fn circle_axes_follow_exact_carrier_sections() {
        let plane = SurfaceGeometry::Plane {
            origin: origin(),
            normal: z(),
            u_axis: x(),
        };
        assert_eq!(circle_axis_from_carrier(origin(), 2.0, &plane), Some(z()));

        let cylinder = SurfaceGeometry::Cylinder {
            origin: origin(),
            axis: z(),
            ref_direction: x(),
            radius: 2.0,
        };
        assert_eq!(
            circle_axis_from_carrier(origin(), 2.0, &cylinder),
            Some(z())
        );
        assert_eq!(circle_axis_from_carrier(origin(), 3.0, &cylinder), None);

        let sphere = SurfaceGeometry::Sphere {
            center: origin(),
            axis: z(),
            ref_direction: x(),
            radius: 5.0,
        };
        assert_eq!(
            circle_axis_from_carrier(Point3::new(0.0, 0.0, 3.0), 4.0, &sphere),
            Some(z())
        );
        assert_eq!(circle_axis_from_carrier(origin(), 5.0, &sphere), None);

        let torus = SurfaceGeometry::Torus {
            center: origin(),
            axis: z(),
            ref_direction: x(),
            major_radius: 10.0,
            minor_radius: 2.0,
        };
        assert_eq!(
            circle_axis_from_carrier(Point3::new(10.0, 0.0, 0.0), 2.0, &torus),
            Some(y())
        );
        assert_eq!(
            circle_axis_from_carrier(Point3::new(0.0, 0.0, 2.0), 10.0, &torus),
            Some(z())
        );
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
    attributes.insert("preview_count".to_string(), scan.previews.len().to_string());
    for (index, preview) in scan.previews.iter().enumerate() {
        attributes.insert(format!("preview_{index}_width"), preview.width.to_string());
        attributes.insert(
            format!("preview_{index}_height"),
            preview.height.to_string(),
        );
        attributes.insert(
            format!("preview_{index}_components"),
            preview.components.to_string(),
        );
    }
    if let Some(version) = &scan.last_save_version {
        attributes.insert("catia_version".to_string(), version.version.to_string());
        attributes.insert("catia_release".to_string(), version.release.to_string());
        attributes.insert(
            "catia_service_pack".to_string(),
            version.service_pack.to_string(),
        );
        attributes.insert("catia_hot_fix".to_string(), version.hot_fix.to_string());
        attributes.insert("catia_build_date".to_string(), version.build_date.clone());
    }
    attributes.insert(
        "external_reference_count".to_string(),
        scan.external_references.len().to_string(),
    );
    for (index, reference) in scan.external_references.iter().enumerate() {
        attributes.insert(
            format!("external_reference_{index}"),
            reference.target.clone(),
        );
    }
    attributes.insert(
        "finjpl_segment_count".to_string(),
        scan.finjpl_segments.len().to_string(),
    );
    for (index, segment) in scan.finjpl_segments.iter().enumerate() {
        if let Some(name) = &segment.name {
            attributes.insert(format!("finjpl_segment_{index}_name"), name.clone());
        }
        attributes.insert(
            format!("finjpl_segment_{index}_type"),
            format!("0x{:08x}", segment.type_word),
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
    analytic_record_count: usize,
    freeform_record_count: usize,
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

    let invalid_analytic = analytic_record_count.saturating_sub(typed.total() + plane_faces);
    if invalid_analytic > 0 {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{invalid_analytic} analytic surface record(s) had a non-finite or out-of-range \
                 inline payload and were not decoded."
            ),
            provenance: None,
        });
    }
    if freeform_record_count > 0 {
        losses.push(LossNote {
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{freeform_record_count} face-local free-form carrier record(s) retain their \
                 tag, bounds, and orientation, but their aliased surface geometry is not yet \
                 transferred."
            ),
            provenance: None,
        });
    }

    losses.push(LossNote {
        category: LossCategory::Attribute,
        severity: Severity::Warning,
        message: "Standard circles with an exact adjacent-carrier section normal and plane-plane \
                  lines are transferred as curves. Spline edge curves, persistent object tags, \
                  materials, and document metadata are not yet transferred."
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

fn build_metadata_ir(scan: &ContainerScan) -> Result<CadIr, CodecError> {
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
        ir.push_native_unknown(
            "catia",
            UnknownRecord {
                id,
                offset: 0,
                byte_len: brep.len() as u64,
                sha256: sha256_hex(brep),
                data: Some(brep.clone()),
                links: Vec::new(),
            },
        )?;
    }
    ir.annotations = annotations.build();
    Ok(ir)
}

/// Preserve the native payload for every partial decode.  Typed entities are
/// additive views; unrecovered record families must remain byte-addressable.
fn preserve_raw_payload(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    scan: &ContainerScan,
    id: &str,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
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
    ir.push_native_unknown(
        "catia",
        UnknownRecord {
            id,
            offset: 0,
            byte_len: bytes.len() as u64,
            sha256: sha256_hex(bytes),
            data: Some(bytes.to_vec()),
            links: Vec::new(),
        },
    )?;
    Ok(())
}

/// Attribute typed carrier views to the preserved payload when CATIA's binding
/// layer was not recovered. The raw payload is their byte-backed owner; this
/// avoids inventing topology or procedural relationships.
fn link_payload_carriers(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let links = ir
        .model
        .surfaces
        .iter()
        .map(|surface| surface.id.0.clone())
        .chain(ir.model.curves.iter().map(|curve| curve.id.0.clone()))
        .collect::<Vec<_>>();
    if links.is_empty() {
        return Ok(());
    }
    let mut unknowns = ir.native_unknowns("catia")?;
    let payload = unknowns
        .last_mut()
        .expect("partial CATIA decode preserves its source payload");
    payload.links = links;
    annotations.derived(&payload.id, "links");
    ir.set_native_unknowns("catia", &unknowns)?;
    Ok(())
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
