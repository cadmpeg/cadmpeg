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
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, NurbsCurve, Pcurve,
    PcurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, RollingBallJetDerivative, RollingBallJetSite, Surface,
    SurfaceCurveFamily, SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId, VertexId,
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
    CatiaNative::decode(&scan.data).store_owned(ir.native.namespace_mut("catia"))?;
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
        let unresolved_curves = ir
            .model
            .edges
            .iter()
            .filter(|edge| edge.curve.is_none())
            .count();
        let mut losses = (unresolved_curves != 0)
            .then(|| LossNote {
                category: LossCategory::Geometry,
                severity: Severity::Blocking,
                message: format!(
                    "The zero-entity B-rep graph is reconstructed; {unresolved_curves} physical edge carriers remain unresolved."
                ),
                provenance: None,
            })
            .into_iter()
            .collect::<Vec<_>>();
        if unresolved_pcurves != 0 {
            losses.push(LossNote {
                category: LossCategory::Topology,
                severity: Severity::Warning,
                message: format!(
                    "The zero-entity B-rep graph is reconstructed; {unresolved_pcurves} referenced-pole pcurve occurrences remain unresolved."
                ),
                provenance: None,
            });
        }
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
    let Some(loop_owner) = unique_index_owners(
        &topology
            .faces
            .iter()
            .map(|face| face.loop_indices.clone())
            .collect::<Vec<_>>(),
        topology.loops.len(),
    ) else {
        return false;
    };
    if topology.carrier_runs.iter().any(|run| {
        run.carrier_ordinal >= topology.records.len()
            || run
                .support_ordinals
                .iter()
                .any(|ordinal| *ordinal >= topology.records.len())
    }) || topology.supports.iter().any(|support| {
        support.record_ordinal >= topology.records.len()
            || support.owner_carrier_ordinal >= topology.records.len()
    }) || topology.faces.iter().any(|face| {
        face.record_ordinal >= topology.records.len()
            || face
                .carrier_run
                .is_some_and(|run| run >= topology.carrier_runs.len())
            || !matches!(
                face.loop_indices
                    .iter()
                    .filter_map(|loop_index| {
                        let loop_ = &topology.loops[*loop_index];
                        (!loop_.inner).then_some(loop_.loop_class)
                    })
                    .collect::<Vec<_>>()
                    .as_slice(),
                [0x41 | 0xc1]
            )
    }) || topology.loops.iter().any(|loop_| {
        loop_.record_ordinal >= topology.records.len()
            || loop_.member_ids.is_empty()
            || loop_.member_ids.len() != loop_.support_indices.len()
            || loop_
                .support_indices
                .iter()
                .flatten()
                .any(|support| *support >= topology.supports.len())
    }) {
        return false;
    }
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
    let mut face_parents: Vec<usize> = (0..topology.faces.len()).collect();
    for edge in &edges {
        let left = loop_owner[edge.occurrences[0].loop_index];
        let right = loop_owner[edge.occurrences[1].loop_index];
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
        let direct_geometry = edges[edge_index]
            .occurrences
            .iter()
            .find_map(|occurrence| crate::zero_entity::support_curve(topology, *occurrence));
        let intersection = direct_geometry
            .is_none()
            .then(|| crate::zero_entity::intersection_curve(topology, &edges[edge_index]))
            .flatten();
        let geometry = direct_geometry.or_else(|| {
            intersection
                .as_ref()
                .map(|intersection| CurveGeometry::Nurbs(intersection.cache.clone()))
        });
        let curve = geometry.map(|geometry| {
            let curve_id = CurveId(format!("catia:zero-entity:curve#{edge_index}"));
            annotate(
                annotations,
                &curve_id,
                "zero_entity_a9_03",
                0,
                "support_pcurve_lift",
                Exactness::Derived,
            );
            annotations.derived(&curve_id, "geometry");
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry,
                source_object: None,
            });
            if let Some(intersection) = intersection {
                let sides = intersection.supports.map(|support_index| {
                    let support = &topology.supports[support_index];
                    let surface_index = topology
                        .carrier_runs
                        .iter()
                        .position(|run| run.carrier_ordinal == support.owner_carrier_ordinal)
                        .expect("support owner is a parsed carrier run");
                    IntcurveSupportSide {
                        surface: Some(SurfaceId(format!("catia:zero-entity:surf#{surface_index}"))),
                        pcurve: support.pcurve.clone(),
                    }
                });
                let procedural_id =
                    ProceduralCurveId(format!("catia:zero-entity:intersection#{edge_index}"));
                annotate(
                    annotations,
                    &procedural_id,
                    "zero_entity_a9_03",
                    0,
                    "radial_support_intersection",
                    Exactness::Derived,
                );
                annotations
                    .derived(&procedural_id, "curve")
                    .derived(&procedural_id, "definition")
                    .derived(&procedural_id, "cache_fit_tolerance");
                ir.model.procedural_curves.push(ProceduralCurve {
                    id: procedural_id,
                    curve: curve_id.clone(),
                    definition: ProceduralCurveDefinition::Intersection {
                        context: IntcurveSupportContext {
                            sides,
                            parameter_range: intersection.parameter_range,
                            discontinuities: std::array::from_fn(|_| Vec::new()),
                        },
                        discontinuity_flag: false,
                    },
                    cache_fit_tolerance: Some(intersection.fit_tolerance),
                });
            }
            curve_id
        });
        annotate(
            annotations,
            &id,
            "zero_entity_a9_03",
            0,
            "radial_endpoint_pair",
            Exactness::Derived,
        );
        annotations.derived(&id, "start").derived(&id, "end");
        if curve.is_some() {
            annotations.derived(&id, "curve");
        }
        ir.model.edges.push(Edge {
            id,
            curve,
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
            face: FaceId(format!("catia:zero-entity:face#{}", loop_owner[loop_index])),
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

fn unique_index_owners(groups: &[Vec<usize>], member_count: usize) -> Option<Vec<usize>> {
    let mut owners = vec![None; member_count];
    for (owner, members) in groups.iter().enumerate() {
        if members.is_empty() {
            return None;
        }
        for member in members {
            let slot = owners.get_mut(*member)?;
            if slot.replace(owner).is_some() {
                return None;
            }
        }
    }
    owners.into_iter().collect()
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
        attach_standard_free_vertices, build_standard_edge_curve, canonical_periodic_range,
        circle_parameter_range_from_surface_branch,
        circular_ranges_are_nonoverlapping_or_coincident, combine_propagated_endpoint_pairs,
        e5_body_kinds, e5_boundary_curve, e5_occurrence_intersection_context, e5_pcurve_on_surface,
        equivalent_e5_curve_carriers, fit_rank_one_e5_plane_axes, include_native_endpoint_pairs,
        intersection_line_direction, ordered_range, parameter_ranges_reversed, point_distance,
        point_on_known_surface, quintic_jet_pcurve, rational_pcurve_arc,
        resolve_standard_endpoint_pairs, reverse_e5_pcurve_geometry,
        standard_circle_endpoint_candidates, standard_circle_param_range, standard_pcurve_geometry,
        unique_index_owners, unique_native_identity_points,
    };
    use crate::e5::{E5Edge, E5Face, E5Loop, E5Topology};
    use crate::geometry::{StandardCurveGeometry, StandardCurveSupport};
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::eval::pcurve_uv;
    use cadmpeg_ir::geometry::{
        CurveGeometry, PcurveGeometry, ProceduralCurve, ProceduralCurveDefinition, Surface,
        SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{PointId, SurfaceId, VertexId};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::topology::{BodyKind, Point, Vertex};
    use cadmpeg_ir::units::Units;
    use cadmpeg_ir::AnnotationBuilder;
    use std::collections::BTreeMap;
    use std::collections::HashMap;

    #[test]
    fn index_ownership_requires_a_complete_partition() {
        assert_eq!(
            unique_index_owners(&[vec![0, 2], vec![1]], 3),
            Some(vec![0, 1, 0])
        );
        assert!(unique_index_owners(&[vec![0], vec![0]], 1).is_none());
        assert!(unique_index_owners(&[vec![0]], 2).is_none());
        assert!(unique_index_owners(&[vec![1]], 1).is_none());
        assert!(unique_index_owners(&[Vec::new()], 0).is_none());
    }

    #[test]
    fn circular_face_intervals_allow_seams_but_reject_crossing_boundaries() {
        let tau = std::f64::consts::TAU;
        assert!(circular_ranges_are_nonoverlapping_or_coincident(&[
            [0.0, 1.0],
            [1.0, 3.0],
            [3.0, tau],
        ]));
        assert!(circular_ranges_are_nonoverlapping_or_coincident(&[
            [0.0, std::f64::consts::PI],
            [0.0, std::f64::consts::PI],
        ]));
        assert!(!circular_ranges_are_nonoverlapping_or_coincident(&[
            [0.0, 4.0],
            [2.0, 5.0],
        ]));
        assert!(circular_ranges_are_nonoverlapping_or_coincident(&[
            [5.0, 7.0],
            [7.0 - tau, 5.0],
        ]));
    }

    #[test]
    fn affine_bound_parameters_preserve_or_reverse_native_direction() {
        assert_eq!(
            parameter_ranges_reversed([0.0, 13.0], [0.0, 122.0]),
            Some(false)
        );
        assert_eq!(
            parameter_ranges_reversed([13.0, 0.0], [0.0, 122.0]),
            Some(true)
        );
        assert_eq!(parameter_ranges_reversed([1.0, 1.0], [0.0, 1.0]), None);
    }

    #[test]
    fn e5_body_kinds_require_complete_single_body_edge_ownership() {
        let face = |record_id| E5Face {
            record_id,
            surface: 100 + record_id,
            trailer_sign: 1,
            loops: vec![E5Loop {
                record_id: 200 + record_id,
                surface: 100 + record_id,
                pcurves: vec![300 + record_id],
                edge_uses: vec![10],
                reversed: vec![false],
                absolute_reversed: Some(vec![false]),
                outer: Some(true),
                orientation_signs: Vec::new(),
            }],
        };
        let edge = |record_id| E5Edge {
            record_id,
            support: 20,
            start_vertex: 30,
            end_vertex: 31,
            parameter_start: 40,
            parameter_end: 41,
            tail: Vec::new(),
        };
        let topology = |faces: Vec<E5Face>, edges: Vec<u32>| E5Topology {
            bodies: Vec::new(),
            faces,
            edges: edges
                .into_iter()
                .map(|record_id| (record_id, edge(record_id)))
                .collect(),
            pcurves: BTreeMap::new(),
            bounds: BTreeMap::new(),
            curve_supports: BTreeMap::new(),
            vertex_refs: Vec::new(),
        };

        assert_eq!(
            e5_body_kinds(&topology(vec![face(1)], vec![10]), &[(None, vec![1])]),
            Some(vec![BodyKind::Sheet])
        );
        assert_eq!(
            e5_body_kinds(
                &topology(vec![face(1), face(2)], vec![10]),
                &[(None, vec![1, 2])],
            ),
            Some(vec![BodyKind::Solid])
        );
        assert!(e5_body_kinds(
            &topology(vec![face(1), face(2)], vec![10]),
            &[(Some(1), vec![1]), (Some(2), vec![2])],
        )
        .is_none());
        assert!(
            e5_body_kinds(&topology(vec![face(1)], vec![10, 11]), &[(None, vec![1])],).is_none()
        );
    }

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
    fn standard_spline_edge_retains_cache_and_exact_intersection_construction() {
        let mut ir = CadIr::empty(Units::default());
        let mut annotations = AnnotationBuilder::new();
        for index in 0..2 {
            ir.model.surfaces.push(Surface {
                id: SurfaceId(format!("surface-{index}")),
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: if index == 0 {
                        Vector3::new(0.0, 0.0, 1.0)
                    } else {
                        Vector3::new(0.0, 1.0, 0.0)
                    },
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
                source_object: None,
            });
        }
        let support = StandardCurveSupport {
            pos: 12,
            tag: 7,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Bspline,
        };
        let (id, range) = build_standard_edge_curve(
            &mut ir,
            &mut annotations,
            &[
                (SurfaceId("surface-0".to_string()), false, 0),
                (SurfaceId("surface-1".to_string()), false, 1),
            ],
            &HashMap::from([
                (SurfaceId("surface-0".to_string()), 0),
                (SurfaceId("surface-1".to_string()), 1),
            ]),
            &[],
            &support,
            [0, 0],
        );
        let id = id.expect("spline support identifies a curve carrier");
        assert_eq!(range, Some([0.0, 1.0]));
        assert_eq!(ir.model.curves[0].id, id);
        assert!(matches!(
            ir.model.curves[0].geometry,
            CurveGeometry::Unknown { ref record }
                if record.as_ref().is_some_and(|id| id.0 == "catia:payload:unknown#brep-stream")
        ));
        assert!(matches!(
            ir.model.procedural_curves.as_slice(),
            [ProceduralCurve {
                curve,
                definition: ProceduralCurveDefinition::Intersection { context, .. },
                ..
            }] if curve == &id
                && context.sides[0].surface.as_ref().is_some_and(|id| id.0 == "surface-0")
                && context.sides[1].surface.as_ref().is_some_and(|id| id.0 == "surface-1")
                && context.parameter_range == [0.0, 1.0]
        ));
    }

    #[test]
    fn standard_line_edge_uses_distance_parameterization() {
        let mut ir = CadIr::empty(Units::default());
        for (index, position) in [Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 6.0, 3.0)]
            .into_iter()
            .enumerate()
        {
            ir.model.points.push(Point {
                id: PointId(format!("p{index}")),
                position,
            });
        }
        let support = StandardCurveSupport {
            pos: 12,
            tag: 7,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Line,
        };
        let (_, range) = build_standard_edge_curve(
            &mut ir,
            &mut AnnotationBuilder::new(),
            &[],
            &HashMap::new(),
            &[],
            &support,
            [0, 1],
        );
        assert_eq!(range, Some([0.0, 5.0]));
    }

    #[test]
    fn e5_cylinder_isoparametric_boundary_lifts_to_circle_carrier() {
        let surface = SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        };
        let pcurve = PcurveGeometry::Line {
            origin: cadmpeg_ir::math::Point2::new(0.0, 3.0),
            direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
        };
        let native = crate::e5::E5Pcurve::Line {
            surface: 0,
            origin: [0.0, 3.0],
            direction: [1.0, 0.0],
            range: [0.0, std::f64::consts::FRAC_PI_2],
        };
        let (curve, range) = e5_boundary_curve(
            &surface,
            &native,
            &pcurve,
            [0.0, std::f64::consts::FRAC_PI_2],
            [Point3::new(2.0, 0.0, 3.0), Point3::new(0.0, 2.0, 3.0)],
        )
        .expect("cylinder boundary circle");
        assert!(matches!(
            curve,
            CurveGeometry::Circle {
                center,
                radius,
                ..
            } if center == Point3::new(0.0, 0.0, 3.0) && radius == 2.0
        ));
        assert!((range[1] - range[0] - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn e5_plane_circle_boundary_lifts_to_world_circle_carrier() {
        let surface = SurfaceGeometry::Plane {
            origin: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let native = crate::e5::E5Pcurve::Circle {
            surface: 0,
            center: [4.0, 5.0],
            codes: [0, 0],
            radius: 2.0,
            range: [0.0, std::f64::consts::PI],
            tail: [0.0, 0.0],
        };
        let pcurve = rational_pcurve_arc([4.0, 5.0], 2.0, [0.0, std::f64::consts::FRAC_PI_2])
            .expect("plane pcurve");
        let (curve, range) = e5_boundary_curve(
            &surface,
            &native,
            &pcurve,
            [0.0, std::f64::consts::FRAC_PI_2],
            [Point3::new(7.0, 7.0, 3.0), Point3::new(5.0, 9.0, 3.0)],
        )
        .expect("plane boundary circle");
        assert_eq!(range, [0.0, std::f64::consts::FRAC_PI_2]);
        assert!(matches!(
            curve,
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } if center == Point3::new(5.0, 7.0, 3.0)
                && axis == Vector3::new(0.0, 0.0, 1.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 2.0
        ));
    }

    #[test]
    fn e5_plane_jet_boundary_lifts_control_net_affinely() {
        let surface = SurfaceGeometry::Plane {
            origin: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let points = vec![[0.0, 0.0], [1.0, 2.0]];
        let first = vec![[1.0, 2.0], [1.0, 2.0]];
        let second = vec![[0.0, 0.0], [0.0, 0.0]];
        let native = crate::e5::E5Pcurve::Jet {
            surface: 0,
            degree: 5,
            knots: vec![0.0, 1.0],
            multiplicities: vec![6, 6],
            points: points.clone(),
            first_derivatives: first.clone(),
            second_derivatives: second.clone(),
            range: [0.0, 1.0],
        };
        let pcurve =
            quintic_jet_pcurve(5, &[0.0, 1.0], &points, &first, &second).expect("quintic pcurve");
        let (curve, range) = e5_boundary_curve(
            &surface,
            &native,
            &pcurve,
            [0.0, 1.0],
            [Point3::new(1.0, 2.0, 3.0), Point3::new(2.0, 4.0, 3.0)],
        )
        .expect("plane jet curve");
        assert_eq!(range, [0.0, 1.0]);
        let CurveGeometry::Nurbs(nurbs) = curve else {
            panic!("expected NURBS curve");
        };
        assert_eq!(
            nurbs.control_points.first(),
            Some(&Point3::new(1.0, 2.0, 3.0))
        );
        assert_eq!(
            nurbs.control_points.last(),
            Some(&Point3::new(2.0, 4.0, 3.0))
        );
    }

    #[test]
    fn e5_intersection_requires_equivalent_two_sided_carriers() {
        let left = CurveGeometry::Circle {
            center: Point3::new(1.0, 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 4.0,
        };
        let right = CurveGeometry::Circle {
            center: Point3::new(1.0, 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, -1.0),
            ref_direction: Vector3::new(0.0, 1.0, 0.0),
            radius: 4.0,
        };
        assert!(equivalent_e5_curve_carriers(&left, &right));
        let displaced = CurveGeometry::Circle {
            center: Point3::new(1.0, 2.0, 3.01),
            axis: Vector3::new(0.0, 0.0, -1.0),
            ref_direction: Vector3::new(0.0, 1.0, 0.0),
            radius: 4.0,
        };
        assert!(!equivalent_e5_curve_carriers(&left, &displaced));
    }

    #[test]
    fn e5_nonplanar_jet_normalizes_positions_and_derivatives() {
        let surface = crate::geometry::E5Surface {
            pos: 0,
            record_id: 7,
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
            },
            uv_scale: [0.5, 1.0],
        };
        let pcurve = crate::e5::E5Pcurve::Jet {
            surface: 7,
            degree: 5,
            knots: vec![0.0, 1.0],
            multiplicities: vec![6, 6],
            points: vec![[0.0, 3.0], [std::f64::consts::PI, 3.0]],
            first_derivatives: vec![[std::f64::consts::PI, 0.0], [std::f64::consts::PI, 0.0]],
            second_derivatives: vec![[0.0, 0.0], [0.0, 0.0]],
            range: [0.0, 1.0],
        };
        let (geometry, range, endpoints) =
            e5_pcurve_on_surface(&pcurve, &surface).expect("normalized cylinder jet");
        assert_eq!(range, [0.0, 1.0]);
        let PcurveGeometry::Nurbs { control_points, .. } = geometry else {
            panic!("expected NURBS pcurve");
        };
        assert_eq!(
            control_points.first(),
            Some(&cadmpeg_ir::math::Point2::new(0.0, 3.0))
        );
        assert_eq!(
            control_points.last(),
            Some(&cadmpeg_ir::math::Point2::new(
                std::f64::consts::FRAC_PI_2,
                3.0
            ))
        );
        assert!(point_distance(endpoints[0], Point3::new(2.0, 0.0, 3.0)) < 1e-12);
        assert!(point_distance(endpoints[1], Point3::new(0.0, 2.0, 3.0)) < 1e-12);
    }

    #[test]
    fn e5_nonplanar_circle_scales_its_rational_uv_control_net() {
        let surface = crate::geometry::E5Surface {
            pos: 0,
            record_id: 7,
            geometry: SurfaceGeometry::Torus {
                center: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                major_radius: 5.0,
                minor_radius: 2.0,
            },
            uv_scale: [0.2, 0.5],
        };
        let pcurve = crate::e5::E5Pcurve::Circle {
            surface: 7,
            center: [10.0, 4.0],
            codes: [0, 0],
            radius: 2.0,
            range: [0.0, std::f64::consts::PI],
            tail: [0.0, 0.0],
        };
        let (geometry, range, endpoints) =
            e5_pcurve_on_surface(&pcurve, &surface).expect("normalized torus circle");
        assert_eq!(range, [0.0, std::f64::consts::FRAC_PI_2]);
        let PcurveGeometry::Nurbs { control_points, .. } = geometry else {
            panic!("expected rational NURBS pcurve");
        };
        let first = control_points.first().expect("first control");
        let last = control_points.last().expect("last control");
        assert!((first.u - 2.4).abs() < 1e-12 && (first.v - 2.0).abs() < 1e-12);
        assert!((last.u - 2.0).abs() < 1e-12 && (last.v - 3.0).abs() < 1e-12);
        let expected = [Point2::new(2.4, 2.0), Point2::new(2.0, 3.0)].map(|uv| {
            cadmpeg_ir::eval::surface_point(&surface.geometry, uv.u, uv.v).expect("torus point")
        });
        assert!(point_distance(endpoints[0], expected[0]) < 1e-12);
        assert!(point_distance(endpoints[1], expected[1]) < 1e-12);
    }

    #[test]
    fn witnessed_cylinder_circle_edge_uses_complementary_angular_range() {
        let mut ir = CadIr::empty(Units::default());
        let surface_id = SurfaceId("cylinder".to_string());
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
            },
            source_object: None,
        });
        let bindings = [(surface_id.clone(), true, 0), (surface_id.clone(), true, 0)];
        let indices = [(surface_id, 0)].into_iter().collect();
        let support = StandardCurveSupport {
            pos: 0,
            tag: 1,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Circle {
                center: Point3::new(0.0, 0.0, 3.0),
                radius: 2.0,
            },
        };
        let mut brep = vec![0; 39];
        brep[..3].copy_from_slice(&[0x00, 0x33, 0x33]);
        brep[27..31].copy_from_slice(&(-2.0f32).to_le_bytes());
        let axis = Vector3::new(0.0, 0.0, 1.0);
        let reference = cadmpeg_ir::geometry::derive_reference_direction(axis);
        let range = standard_circle_param_range(
            &ir,
            &bindings,
            &indices,
            &brep,
            &support,
            Point3::new(0.0, 0.0, 3.0),
            2.0,
            axis,
            reference,
            Point3::new(2.0, 0.0, 3.0),
            Point3::new(0.0, 2.0, 3.0),
        )
        .expect("witnessed circle range");
        assert!(((range[1] - range[0]).abs() - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn standard_unbound_vertices_receive_one_free_vertex_owner() {
        let mut ir = CadIr::empty(Units::default());
        ir.model.vertices.push(Vertex {
            id: VertexId("v".to_string()),
            point: PointId("p".to_string()),
            tolerance: None,
        });
        let mut annotations = AnnotationBuilder::new();
        attach_standard_free_vertices(&mut ir, &mut annotations);
        assert_eq!(ir.model.bodies.len(), 1);
        assert_eq!(ir.model.regions.len(), 1);
        assert_eq!(ir.model.shells.len(), 1);
        assert_eq!(
            ir.model.shells[0].free_vertices,
            [VertexId("v".to_string())]
        );
    }

    #[test]
    fn standard_spline_retains_complete_surface_incidence_pair_domain() {
        let mut ir = CadIr::empty(Units::default());
        for index in 0..138 {
            ir.model.points.push(Point {
                id: PointId(format!("p{index}")),
                position: Point3::new(index as f64, 0.0, 0.0),
            });
        }
        for index in 0..2 {
            ir.model.surfaces.push(Surface {
                id: SurfaceId(format!("s{index}")),
                geometry: SurfaceGeometry::Unknown { record: None },
                source_object: None,
            });
        }
        let bindings = [
            (SurfaceId("s0".to_string()), true, 0),
            (SurfaceId("s1".to_string()), true, 0),
        ];
        let indices = [
            (SurfaceId("s0".to_string()), 0),
            (SurfaceId("s1".to_string()), 1),
        ]
        .into_iter()
        .collect();
        let support = StandardCurveSupport {
            pos: 0,
            tag: 1,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Bspline,
        };
        let choices = resolve_standard_endpoint_pairs(
            &ir,
            &bindings,
            &indices,
            &[support],
            &[(0..138).collect()],
        )
        .expect("endpoint option pass");
        assert_eq!(choices[0].len(), 9_453);
        assert_eq!(choices[0].first(), Some(&[0, 1]));
        assert_eq!(choices[0].last(), Some(&[136, 137]));
    }

    #[test]
    fn standard_parallel_line_rows_bind_by_serialized_branch_rank() {
        let mut ir = CadIr::empty(Units::default());
        for (index, position) in [
            Point3::new(-2.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(-2.0, 0.0, 1.0),
            Point3::new(2.0, 0.0, 1.0),
        ]
        .into_iter()
        .enumerate()
        {
            ir.model.points.push(Point {
                id: PointId(format!("p{index}")),
                position,
            });
        }
        for index in 0..2 {
            ir.model.surfaces.push(Surface {
                id: SurfaceId(format!("s{index}")),
                geometry: SurfaceGeometry::Cylinder {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    axis: Vector3::new(0.0, 0.0, 1.0),
                    ref_direction: Vector3::new(1.0, 0.0, 0.0),
                    radius: 2.0,
                },
                source_object: None,
            });
        }
        let bindings = [
            (SurfaceId("s0".to_string()), true, 0),
            (SurfaceId("s1".to_string()), true, 0),
        ];
        let indices = [
            (SurfaceId("s0".to_string()), 0),
            (SurfaceId("s1".to_string()), 1),
        ]
        .into_iter()
        .collect();
        let supports = [0, 1].map(|index| StandardCurveSupport {
            pos: index,
            tag: index as u32,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Line,
        });

        let choices = resolve_standard_endpoint_pairs(
            &ir,
            &bindings,
            &indices,
            &supports,
            &[vec![0, 1, 2, 3], vec![0, 1, 2, 3]],
        )
        .expect("endpoint option pass");

        assert_eq!(choices, [vec![[0, 2]], vec![[1, 3]]]);
    }

    #[test]
    fn standard_circle_endpoint_domain_uses_the_explicit_curve_carrier() {
        let points = [
            Point {
                id: PointId("on".to_string()),
                position: Point3::new(3.0, 4.0, 7.0),
            },
            Point {
                id: PointId("off".to_string()),
                position: Point3::new(3.0, 4.01, 7.0),
            },
        ];
        assert_eq!(
            standard_circle_endpoint_candidates(&points, Point3::new(0.0, 0.0, 7.0), 5.0, None,),
            [0]
        );
    }

    #[test]
    fn standard_circle_endpoint_domain_requires_both_face_carriers() {
        let points = [
            Point {
                id: PointId("incident".to_string()),
                position: Point3::new(3.0, 4.0, 0.0),
            },
            Point {
                id: PointId("other-occurrence".to_string()),
                position: Point3::new(3.0, -4.0, 0.0),
            },
        ];
        let left = SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 4.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let right = SurfaceGeometry::Unknown { record: None };
        assert_eq!(
            standard_circle_endpoint_candidates(
                &points,
                Point3::new(0.0, 0.0, 0.0),
                5.0,
                Some((&left, &right)),
            ),
            [0]
        );
    }

    #[test]
    fn native_endpoint_pairs_extend_geometric_candidate_domains() {
        let mut candidates = vec![vec![1], Vec::new()];
        include_native_endpoint_pairs(&mut candidates, &[Some([1, 2]), Some([3, 4])]);
        assert_eq!(candidates, [vec![1, 2], vec![3, 4]]);
    }

    #[test]
    fn complete_mesh_endpoint_quotient_overrides_table_local_ports() {
        let raw = Some(vec![Some([0, 1]), Some([2, 3])]);
        let mesh = Some(vec![Some([0, 1]), Some([1, 2])]);
        assert_eq!(
            combine_propagated_endpoint_pairs(raw, mesh),
            Some(vec![Some([0, 1]), Some([1, 2])])
        );
    }

    #[test]
    fn native_identity_locus_binds_only_one_coordinate_row_within_tolerance() {
        let points = [
            Point {
                id: PointId("a".to_string()),
                position: Point3::new(1.0, 0.0, 0.0),
            },
            Point {
                id: PointId("b".to_string()),
                position: Point3::new(1.01, 0.0, 0.0),
            },
        ];
        let tolerances = [(2usize, 0.02)].into_iter().collect();
        let ambiguous =
            unique_native_identity_points(&[7], &[[1.0, 0.0, 0.0]], 2, &tolerances, &points);
        assert!(ambiguous.is_empty());

        let exact =
            unique_native_identity_points(&[7], &[[1.0, 0.0, 0.0]], 2, &BTreeMap::new(), &points);
        assert_eq!(exact.get(&7), Some(&0));
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
    fn canonical_periodic_range_snaps_roundoff_at_the_turn_seam() {
        let tau = std::f64::consts::TAU;
        let range =
            canonical_periodic_range([tau - 1e-14, tau + 0.25]).expect("canonical seam range");
        assert_eq!(range[0], 0.0);
        assert!((range[1] - 0.25).abs() < 2e-14);
    }

    #[test]
    fn reversed_surface_pcurve_preserves_domain_and_swaps_endpoints() {
        let geometry = PcurveGeometry::Line {
            origin: Point2::new(2.0, -1.0),
            direction: Point2::new(3.0, 4.0),
        };
        let range = [5.0, 9.0];
        let reversed = reverse_e5_pcurve_geometry(&geometry, range);
        for (parameter, source_parameter) in [(5.0, 9.0), (9.0, 5.0)] {
            let actual = pcurve_uv(&reversed, parameter).expect("reversed evaluation");
            let expected = pcurve_uv(&geometry, source_parameter).expect("source evaluation");
            assert!((actual.u - expected.u).abs() < 1e-12);
            assert!((actual.v - expected.v).abs() < 1e-12);
        }
    }

    #[test]
    fn occurrence_intersection_accepts_roundoff_equivalent_side_ranges() {
        let sides = vec![
            (
                SurfaceId("left".to_string()),
                PcurveGeometry::Line {
                    origin: Point2::new(0.0, 0.0),
                    direction: Point2::new(1.0, 0.0),
                },
                [-2.0, 3.0],
            ),
            (
                SurfaceId("right".to_string()),
                PcurveGeometry::Line {
                    origin: Point2::new(0.0, 1.0),
                    direction: Point2::new(1.0, 0.0),
                },
                [-2.0 - 1e-14, 3.0 + 1e-14],
            ),
        ];
        let context = e5_occurrence_intersection_context(&sides).expect("intersection context");
        assert_eq!(context.parameter_range, [-2.0, 3.0]);
        assert_eq!(
            context.sides[0].surface.as_ref().expect("left surface").0,
            "left"
        );
        assert_eq!(
            context.sides[1].surface.as_ref().expect("right surface").0,
            "right"
        );
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
            None,
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
    fn standard_pcurve_rejects_endpoints_outside_the_face_carrier() {
        let surface = SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let support = StandardCurveSupport {
            pos: 0,
            tag: 1,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Line,
        };
        assert!(standard_pcurve_geometry(
            &surface,
            &support,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 2.0, 0.0),
            None,
        )
        .is_none());
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
            None,
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
    fn standard_cylinder_witness_selects_complementary_arc() {
        let surface = SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        };
        let support = StandardCurveSupport {
            pos: 0,
            tag: 1,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Circle {
                center: Point3::new(0.0, 0.0, 3.0),
                radius: 2.0,
            },
        };
        let (geometry, _) = standard_pcurve_geometry(
            &surface,
            &support,
            Point3::new(2.0, 0.0, 3.0),
            Point3::new(0.0, 2.0, 3.0),
            Some(Point3::new(-2.0, 0.0, 3.0)),
        )
        .expect("witnessed cylinder section");
        assert_eq!(
            geometry,
            PcurveGeometry::Line {
                origin: cadmpeg_ir::math::Point2::new(0.0, 3.0),
                direction: cadmpeg_ir::math::Point2::new(-3.0 * std::f64::consts::FRAC_PI_2, 0.0,),
            }
        );
    }

    #[test]
    fn standard_torus_witness_selects_complementary_latitude_arc() {
        let surface = SurfaceGeometry::Torus {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            major_radius: 5.0,
            minor_radius: 2.0,
        };
        let support = StandardCurveSupport {
            pos: 0,
            tag: 1,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Circle {
                center: Point3::new(0.0, 0.0, 0.0),
                radius: 7.0,
            },
        };
        let (geometry, _) = standard_pcurve_geometry(
            &surface,
            &support,
            Point3::new(7.0, 0.0, 0.0),
            Point3::new(0.0, 7.0, 0.0),
            Some(Point3::new(-7.0, 0.0, 0.0)),
        )
        .expect("witnessed torus latitude");
        let PcurveGeometry::Line { origin, direction } = geometry else {
            panic!("expected torus chart line");
        };
        assert_eq!(origin, cadmpeg_ir::math::Point2::new(0.0, 0.0));
        assert_eq!(
            direction,
            cadmpeg_ir::math::Point2::new(-3.0 * std::f64::consts::FRAC_PI_2, 0.0)
        );
        let range = circle_parameter_range_from_surface_branch(
            &surface,
            Point3::new(0.0, 0.0, 0.0),
            7.0,
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            Point3::new(7.0, 0.0, 0.0),
            Point3::new(0.0, 7.0, 0.0),
            origin,
            direction,
        )
        .expect("torus circle range");
        assert!(((range[1] - range[0]).abs() - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-12);
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
            None,
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
    let mut edge_curve_plan = BTreeMap::<u32, (CurveGeometry, [f64; 2])>::new();
    let mut surface_curve_plan = BTreeMap::<u32, (SurfaceId, PcurveGeometry, [f64; 2])>::new();
    let mut occurrence_intersection_sides =
        BTreeMap::<u32, Vec<(SurfaceId, PcurveGeometry, [f64; 2])>>::new();
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
                let reverse_error =
                    point_distance(endpoints[0], *end).max(point_distance(endpoints[1], *start));
                let reversed = e5_stored_pcurve_reversed(topology, edge_ref, pcurve_ref, range)
                    .or_else(|| {
                        ((forward - reverse_error).abs() > 1e-9).then_some(reverse_error < forward)
                    });
                let Some(reversed) = reversed else {
                    return false;
                };
                if if reversed { reverse_error } else { forward } > 2e-3 {
                    return false;
                }
                let oriented_pcurve = if reversed {
                    reverse_e5_pcurve_geometry(&geometry, range)
                } else {
                    geometry.clone()
                };
                if support.intersection {
                    let side = (
                        surface_for_ref[&face.surface].0.clone(),
                        oriented_pcurve.clone(),
                        range,
                    );
                    let sides = occurrence_intersection_sides.entry(edge_ref).or_default();
                    if !sides.contains(&side) {
                        sides.push(side);
                    }
                }
                if let Some((mut curve, mut curve_range)) = e5_boundary_curve(
                    &decoded_surface.geometry,
                    pcurve,
                    &geometry,
                    range,
                    endpoints,
                ) {
                    if reversed {
                        let Some(reversed_curve) = reverse_e5_boundary_curve(&curve, curve_range)
                        else {
                            return false;
                        };
                        (curve, curve_range) = reversed_curve;
                    }
                    if !support.intersection {
                        if let Some(existing) = edge_curve_plan.get(&edge_ref) {
                            if existing != &(curve, curve_range) {
                                return false;
                            }
                        } else {
                            edge_curve_plan.insert(edge_ref, (curve, curve_range));
                        }
                    }
                } else if !support.intersection {
                    surface_curve_plan.entry(edge_ref).or_insert_with(|| {
                        (
                            surface_for_ref[&face.surface].0.clone(),
                            oriented_pcurve,
                            range,
                        )
                    });
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
    let mut intersection_sides =
        BTreeMap::<u32, BTreeMap<u32, (SurfaceId, PcurveGeometry, CurveGeometry, [f64; 2])>>::new();
    for (&edge_ref, edge) in &topology.edges {
        let Some(support) = topology.curve_supports.get(&edge.support) else {
            return false;
        };
        if !support.intersection {
            continue;
        }
        let (Some(start), Some(end)) = (
            point_for_ref.get(&edge.start_vertex),
            point_for_ref.get(&edge.end_vertex),
        ) else {
            return false;
        };
        for pcurve_ref in &support.pcurves {
            let Some(pcurve) = topology.pcurves.get(pcurve_ref) else {
                continue;
            };
            let surface_ref = match pcurve {
                crate::e5::E5Pcurve::Line { surface, .. }
                | crate::e5::E5Pcurve::Circle { surface, .. }
                | crate::e5::E5Pcurve::Jet { surface, .. } => *surface,
            };
            let Some((surface_id, decoded_surface)) = surface_for_ref.get(&surface_ref) else {
                continue;
            };
            let Some((geometry, range, endpoints)) = e5_pcurve_on_surface(pcurve, decoded_surface)
            else {
                continue;
            };
            let forward =
                point_distance(endpoints[0], *start).max(point_distance(endpoints[1], *end));
            let reverse_error =
                point_distance(endpoints[0], *end).max(point_distance(endpoints[1], *start));
            let reversed = e5_stored_pcurve_reversed(topology, edge_ref, *pcurve_ref, range)
                .or_else(|| {
                    ((forward - reverse_error).abs() > 1e-9).then_some(reverse_error < forward)
                });
            let Some(reversed) = reversed else {
                continue;
            };
            if if reversed { reverse_error } else { forward } > 2e-3 {
                continue;
            }
            let Some((mut curve, mut curve_range)) = e5_boundary_curve(
                &decoded_surface.geometry,
                pcurve,
                &geometry,
                range,
                endpoints,
            ) else {
                continue;
            };
            if reversed {
                let Some(reversed_curve) = reverse_e5_boundary_curve(&curve, curve_range) else {
                    continue;
                };
                (curve, curve_range) = reversed_curve;
            }
            intersection_sides.entry(edge_ref).or_default().insert(
                *pcurve_ref,
                (surface_id.clone(), geometry, curve, curve_range),
            );
        }
    }

    let mut intersection_plan = BTreeMap::<u32, IntcurveSupportContext>::new();
    for (&edge_ref, sides) in &intersection_sides {
        let Some(edge) = topology.edges.get(&edge_ref) else {
            return false;
        };
        let Some(support) = topology.curve_supports.get(&edge.support) else {
            return false;
        };
        let [left_ref, right_ref] = support.pcurves.as_slice() else {
            continue;
        };
        let (Some(left), Some(right)) = (sides.get(left_ref), sides.get(right_ref)) else {
            continue;
        };
        if !equivalent_e5_curve_carriers(&left.2, &right.2)
            || ((left.3[1] - left.3[0]) - (right.3[1] - right.3[0])).abs() > 1e-9
        {
            continue;
        }
        edge_curve_plan.insert(edge_ref, (left.2.clone(), left.3));
        intersection_plan.insert(
            edge_ref,
            IntcurveSupportContext {
                sides: [left, right].map(|side| IntcurveSupportSide {
                    surface: Some(side.0.clone()),
                    pcurve: Some(side.1.clone()),
                }),
                parameter_range: left.3,
                discontinuities: std::array::from_fn(|_| Vec::new()),
            },
        );
    }
    for (&edge_ref, sides) in &occurrence_intersection_sides {
        if intersection_plan.contains_key(&edge_ref) {
            continue;
        }
        let Some(context) = e5_occurrence_intersection_context(sides) else {
            if let Some(side) = sides.first() {
                surface_curve_plan
                    .entry(edge_ref)
                    .or_insert_with(|| side.clone());
            }
            continue;
        };
        edge_curve_plan.insert(
            edge_ref,
            (
                CurveGeometry::Unknown { record: None },
                context.parameter_range,
            ),
        );
        intersection_plan.insert(edge_ref, context);
    }

    for (&edge_ref, (_, _, range)) in &surface_curve_plan {
        edge_curve_plan
            .entry(edge_ref)
            .or_insert((CurveGeometry::Unknown { record: None }, *range));
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
    let Some(body_kinds) = e5_body_kinds(topology, &body_faces) else {
        return false;
    };

    let edge_ids: HashMap<u32, EdgeId> = topology
        .edges
        .keys()
        .map(|record_id| (*record_id, EdgeId(format!("catia:e5:edge#{record_id}"))))
        .collect();
    let edge_curve_ids: HashMap<u32, CurveId> = edge_curve_plan
        .iter()
        .map(|(&record_id, (geometry, _))| {
            let id = CurveId(format!("catia:e5:curve#{record_id}"));
            annotate(
                annotations,
                &id,
                "e5_0d_03",
                0,
                "lifted_boundary_curve",
                Exactness::Derived,
            );
            annotations.derived(&id, "geometry");
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry: geometry.clone(),
                source_object: None,
            });
            (record_id, id)
        })
        .collect();
    for (&record_id, context) in &intersection_plan {
        let Some(curve) = edge_curve_ids.get(&record_id) else {
            return false;
        };
        let id = ProceduralCurveId(format!("catia:e5:intersection#{record_id}"));
        annotate(
            annotations,
            &id,
            "e5_0d_03",
            0,
            "c1_surface_intersection",
            Exactness::Derived,
        );
        annotations.derived(&id, "curve").derived(&id, "definition");
        ir.model.procedural_curves.push(ProceduralCurve {
            id,
            curve: curve.clone(),
            definition: ProceduralCurveDefinition::Intersection {
                context: context.clone(),
                discontinuity_flag: false,
            },
            cache_fit_tolerance: None,
        });
    }
    for (&record_id, (surface, pcurve, range)) in &surface_curve_plan {
        if intersection_plan.contains_key(&record_id) {
            continue;
        }
        let Some(curve) = edge_curve_ids.get(&record_id) else {
            return false;
        };
        let id = ProceduralCurveId(format!("catia:e5:surface-curve#{record_id}"));
        annotate(
            annotations,
            &id,
            "e5_0d_03",
            0,
            "parametric_surface_curve",
            Exactness::Derived,
        );
        annotations.derived(&id, "curve").derived(&id, "definition");
        ir.model.procedural_curves.push(ProceduralCurve {
            id,
            curve: curve.clone(),
            definition: ProceduralCurveDefinition::SurfaceCurve {
                family: SurfaceCurveFamily::Parametric,
                context: IntcurveSupportContext {
                    sides: [
                        IntcurveSupportSide {
                            surface: Some(surface.clone()),
                            pcurve: Some(pcurve.clone()),
                        },
                        IntcurveSupportSide {
                            surface: None,
                            pcurve: None,
                        },
                    ],
                    parameter_range: *range,
                    discontinuities: std::array::from_fn(|_| Vec::new()),
                },
            },
            cache_fit_tolerance: None,
        });
    }
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
        if edge_curve_ids.contains_key(&record_id) {
            annotations
                .derived(&id, "curve")
                .derived(&id, "param_range");
        }
        ir.model.edges.push(Edge {
            id,
            curve: edge_curve_ids.get(&record_id).cloned(),
            start: vertex_for_ref[&edge.start_vertex].clone(),
            end: vertex_for_ref[&edge.end_vertex].clone(),
            param_range: edge_curve_plan.get(&record_id).map(|(_, range)| *range),
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
        let kind = body_kinds[body_index];
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
            let Some(senses) = loop_.resolved_reversed() else {
                return false;
            };
            for (index, ((&edge_ref, &pcurve_ref), &reversed)) in loop_
                .edge_uses
                .iter()
                .zip(&loop_.pcurves)
                .zip(senses)
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

fn e5_stored_pcurve_reversed(
    topology: &crate::e5::E5Topology,
    edge_ref: u32,
    pcurve_ref: u32,
    native_range: [f64; 2],
) -> Option<bool> {
    let parameters = topology.edge_representation_parameters(edge_ref, pcurve_ref)?;
    parameter_ranges_reversed(parameters, native_range)
}

fn parameter_ranges_reversed(parameters: [f64; 2], native_range: [f64; 2]) -> Option<bool> {
    let bound_span = parameters[1] - parameters[0];
    let native_span = native_range[1] - native_range[0];
    (bound_span.abs() > f64::EPSILON && native_span.abs() > f64::EPSILON)
        .then_some(bound_span.is_sign_negative() != native_span.is_sign_negative())
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
        } => {
            let angular_range = ordered_range([range[0] / radius, range[1] / radius]);
            let geometry = rational_pcurve_arc(*center, *radius, angular_range)?;
            let PcurveGeometry::Nurbs {
                degree,
                knots,
                control_points,
                weights,
                periodic,
            } = geometry
            else {
                return None;
            };
            let scale = decoded_surface.uv_scale;
            let geometry = PcurveGeometry::Nurbs {
                degree,
                knots,
                control_points: control_points
                    .into_iter()
                    .map(|point| Point2::new(point.u * scale[0], point.v * scale[1]))
                    .collect(),
                weights,
                periodic,
            };
            let endpoints = angular_range.map(|angle| {
                cadmpeg_ir::eval::surface_point(
                    surface,
                    (center[0] + radius * angle.cos()) * scale[0],
                    (center[1] + radius * angle.sin()) * scale[1],
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
        } => {
            let scale = decoded_surface.uv_scale;
            let points = points
                .iter()
                .map(|point| [point[0] * scale[0], point[1] * scale[1]])
                .collect::<Vec<_>>();
            let first_derivatives = first_derivatives
                .iter()
                .map(|value| [value[0] * scale[0], value[1] * scale[1]])
                .collect::<Vec<_>>();
            let second_derivatives = second_derivatives
                .iter()
                .map(|value| [value[0] * scale[0], value[1] * scale[1]])
                .collect::<Vec<_>>();
            let geometry = quintic_jet_pcurve(
                *degree,
                knots,
                &points,
                &first_derivatives,
                &second_derivatives,
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
    }
}

fn e5_boundary_curve(
    surface: &SurfaceGeometry,
    native_pcurve: &crate::e5::E5Pcurve,
    pcurve: &PcurveGeometry,
    range: [f64; 2],
    endpoints: [Point3; 2],
) -> Option<(CurveGeometry, [f64; 2])> {
    if let (
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        },
        crate::e5::E5Pcurve::Circle { center, radius, .. },
    ) = (surface, native_pcurve)
    {
        let v_axis = cross_vector(*normal, *u_axis);
        let center = add_scaled_point(
            add_scaled_point(*origin, *u_axis, center[0]),
            v_axis,
            center[1],
        );
        return Some((
            CurveGeometry::Circle {
                center,
                axis: *normal,
                ref_direction: *u_axis,
                radius: *radius,
            },
            canonical_periodic_range(range)?,
        ));
    }
    if let (
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        },
        crate::e5::E5Pcurve::Jet { .. },
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic,
        },
    ) = (surface, native_pcurve, pcurve)
    {
        let v_axis = cross_vector(*normal, *u_axis);
        return Some((
            CurveGeometry::Nurbs(NurbsCurve {
                degree: *degree,
                knots: knots.clone(),
                control_points: control_points
                    .iter()
                    .map(|point| {
                        add_scaled_point(
                            add_scaled_point(*origin, *u_axis, point.u),
                            v_axis,
                            point.v,
                        )
                    })
                    .collect(),
                weights: weights.clone(),
                periodic: *periodic,
            }),
            range,
        ));
    }
    let PcurveGeometry::Line { origin, direction } = pcurve else {
        return None;
    };
    let start_uv = Point2::new(
        origin.u + range[0] * direction.u,
        origin.v + range[0] * direction.v,
    );
    let span = range[1] - range[0];
    let span_direction = Point2::new(span * direction.u, span * direction.v);

    let circle = if direction.v.abs() <= 1e-12 && direction.u.abs() > 1e-12 {
        e5_constant_v_circle(surface, start_uv.v)
    } else if direction.u.abs() <= 1e-12 && direction.v.abs() > 1e-12 {
        e5_constant_u_circle(surface, start_uv.u)
    } else {
        None
    };
    if let Some((center, radius, axis)) = circle {
        let candidates = [axis, scale_vector(axis, -1.0)]
            .into_iter()
            .filter_map(|axis| {
                let ref_direction = cadmpeg_ir::geometry::derive_reference_direction(axis);
                let range = circle_parameter_range_from_surface_branch(
                    surface,
                    center,
                    radius,
                    axis,
                    ref_direction,
                    endpoints[0],
                    endpoints[1],
                    start_uv,
                    span_direction,
                )?;
                Some((axis, ref_direction, canonical_periodic_range(range)?))
            })
            .collect::<Vec<_>>();
        let [(axis, ref_direction, curve_range)] = candidates.as_slice() else {
            return None;
        };
        return Some((
            CurveGeometry::Circle {
                center,
                axis: *axis,
                ref_direction: *ref_direction,
                radius,
            },
            *curve_range,
        ));
    }

    if !(matches!(surface, SurfaceGeometry::Plane { .. })
        || (direction.u.abs() <= 1e-12
            && matches!(
                surface,
                SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. }
            )))
    {
        return None;
    }
    let delta = point_delta(endpoints[1], endpoints[0]);
    let length = vector_norm(delta);
    (length > f64::EPSILON).then_some((
        CurveGeometry::Line {
            origin: endpoints[0],
            direction: scale_vector(delta, 1.0 / length),
        },
        [0.0, length],
    ))
}

fn reverse_e5_boundary_curve(
    curve: &CurveGeometry,
    range: [f64; 2],
) -> Option<(CurveGeometry, [f64; 2])> {
    match curve {
        CurveGeometry::Line { origin, direction } => {
            let length = range[1] - range[0];
            (length >= 0.0).then_some((
                CurveGeometry::Line {
                    origin: add_scaled_point(*origin, *direction, range[1]),
                    direction: scale_vector(*direction, -1.0),
                },
                [0.0, length],
            ))
        }
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            let sweep = range[1] - range[0];
            if sweep < 0.0 {
                return None;
            }
            let tangent = cross_vector(*axis, *ref_direction);
            let end = range[1];
            let ref_direction = add_vectors(
                scale_vector(*ref_direction, end.cos()),
                scale_vector(tangent, end.sin()),
            );
            Some((
                CurveGeometry::Circle {
                    center: *center,
                    axis: scale_vector(*axis, -1.0),
                    ref_direction,
                    radius: *radius,
                },
                [0.0, sweep],
            ))
        }
        CurveGeometry::Nurbs(nurbs) => {
            let first = *nurbs.knots.first()?;
            let last = *nurbs.knots.last()?;
            let mut knots = nurbs
                .knots
                .iter()
                .rev()
                .map(|knot| first + last - knot)
                .collect::<Vec<_>>();
            for knot in &mut knots {
                if knot.abs() <= 1e-15 {
                    *knot = 0.0;
                }
            }
            Some((
                CurveGeometry::Nurbs(NurbsCurve {
                    degree: nurbs.degree,
                    knots,
                    control_points: nurbs.control_points.iter().rev().copied().collect(),
                    weights: nurbs
                        .weights
                        .as_ref()
                        .map(|weights| weights.iter().rev().copied().collect()),
                    periodic: nurbs.periodic,
                }),
                range,
            ))
        }
        _ => None,
    }
}

fn reverse_e5_pcurve_geometry(geometry: &PcurveGeometry, range: [f64; 2]) -> PcurveGeometry {
    match geometry {
        PcurveGeometry::Line { origin, direction } => PcurveGeometry::Line {
            origin: Point2::new(
                origin.u + (range[0] + range[1]) * direction.u,
                origin.v + (range[0] + range[1]) * direction.v,
            ),
            direction: Point2::new(-direction.u, -direction.v),
        },
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic,
        } => {
            let sum = range[0] + range[1];
            let mut reversed_knots = knots
                .iter()
                .rev()
                .map(|knot| sum - knot)
                .collect::<Vec<_>>();
            for knot in &mut reversed_knots {
                if *knot == -0.0 {
                    *knot = 0.0;
                }
            }
            PcurveGeometry::Nurbs {
                degree: *degree,
                knots: reversed_knots,
                control_points: control_points.iter().rev().copied().collect(),
                weights: weights
                    .as_ref()
                    .map(|weights| weights.iter().rev().copied().collect()),
                periodic: *periodic,
            }
        }
    }
}

fn e5_occurrence_intersection_context(
    sides: &[(SurfaceId, PcurveGeometry, [f64; 2])],
) -> Option<IntcurveSupportContext> {
    let [left, right] = sides else {
        return None;
    };
    if (left.2[0] - right.2[0]).abs() > 1e-9 || (left.2[1] - right.2[1]).abs() > 1e-9 {
        return None;
    }
    Some(IntcurveSupportContext {
        sides: [left, right].map(|side| IntcurveSupportSide {
            surface: Some(side.0.clone()),
            pcurve: Some(side.1.clone()),
        }),
        parameter_range: left.2,
        discontinuities: std::array::from_fn(|_| Vec::new()),
    })
}

fn equivalent_e5_curve_carriers(left: &CurveGeometry, right: &CurveGeometry) -> bool {
    match (left, right) {
        (
            CurveGeometry::Line {
                origin: left_origin,
                direction: left_direction,
            },
            CurveGeometry::Line {
                origin: right_origin,
                direction: right_direction,
            },
        ) => {
            point_distance(*left_origin, *right_origin) <= 2e-3
                && axis_dot(*left_direction, *right_direction).abs() >= 1.0 - 1e-9
        }
        (
            CurveGeometry::Circle {
                center: left_center,
                axis: left_axis,
                radius: left_radius,
                ..
            },
            CurveGeometry::Circle {
                center: right_center,
                axis: right_axis,
                radius: right_radius,
                ..
            },
        ) => {
            point_distance(*left_center, *right_center) <= 2e-3
                && (left_radius - right_radius).abs() <= 2e-3
                && axis_dot(*left_axis, *right_axis).abs() >= 1.0 - 1e-9
        }
        (CurveGeometry::Nurbs(left), CurveGeometry::Nurbs(right)) => left == right,
        _ => false,
    }
}

fn e5_constant_v_circle(surface: &SurfaceGeometry, v: f64) -> Option<(Point3, f64, Vector3)> {
    match surface {
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        } => Some((add_scaled_point(*origin, *axis, v), *radius, *axis)),
        SurfaceGeometry::Cone {
            origin,
            axis,
            radius,
            half_angle,
            ..
        } => Some((
            add_scaled_point(*origin, *axis, v),
            (radius + v * half_angle.tan()).abs(),
            *axis,
        )),
        SurfaceGeometry::Sphere {
            center,
            axis,
            radius,
            ..
        } => Some((
            add_scaled_point(*center, *axis, radius * v.sin()),
            radius * v.cos().abs(),
            *axis,
        )),
        SurfaceGeometry::Torus {
            center,
            axis,
            major_radius,
            minor_radius,
            ..
        } => Some((
            add_scaled_point(*center, *axis, minor_radius * v.sin()),
            (major_radius + minor_radius * v.cos()).abs(),
            *axis,
        )),
        _ => None,
    }
}

fn e5_constant_u_circle(surface: &SurfaceGeometry, u: f64) -> Option<(Point3, f64, Vector3)> {
    match surface {
        SurfaceGeometry::Sphere {
            center,
            axis,
            radius,
            ref_direction,
        } => {
            let tangent = cross_vector(*axis, *ref_direction);
            let radial = add_vectors(
                scale_vector(*ref_direction, u.cos()),
                scale_vector(tangent, u.sin()),
            );
            Some((*center, *radius, cross_vector(*axis, radial)))
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            let tangent = cross_vector(*axis, *ref_direction);
            let radial = add_vectors(
                scale_vector(*ref_direction, u.cos()),
                scale_vector(tangent, u.sin()),
            );
            Some((
                add_scaled_point(*center, radial, *major_radius),
                *minor_radius,
                cross_vector(*axis, radial),
            ))
        }
        _ => None,
    }
}

fn add_scaled_point(point: Point3, vector: Vector3, scale: f64) -> Point3 {
    Point3::new(
        point.x + scale * vector.x,
        point.y + scale * vector.y,
        point.z + scale * vector.z,
    )
}

fn add_vectors(left: Vector3, right: Vector3) -> Vector3 {
    Vector3::new(left.x + right.x, left.y + right.y, left.z + right.z)
}

fn canonical_periodic_range(range: [f64; 2]) -> Option<[f64; 2]> {
    let sweep = range[1] - range[0];
    if !sweep.is_finite() || sweep <= 0.0 || sweep > std::f64::consts::TAU + 1e-9 {
        return None;
    }
    let mut start = range[0].rem_euclid(std::f64::consts::TAU);
    if std::f64::consts::TAU - start <= 1e-9 {
        start = 0.0;
    }
    Some([start, start + sweep])
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

fn e5_body_kinds(
    topology: &crate::e5::E5Topology,
    body_faces: &[(Option<u32>, Vec<u32>)],
) -> Option<Vec<BodyKind>> {
    let mut body_by_face = HashMap::new();
    for (body, (_, faces)) in body_faces.iter().enumerate() {
        for face in faces {
            if body_by_face.insert(*face, body).is_some() {
                return None;
            }
        }
    }
    let mut uses = vec![HashMap::<u32, usize>::new(); body_faces.len()];
    let mut bodies_by_edge = topology
        .edges
        .keys()
        .map(|edge| (*edge, HashSet::new()))
        .collect::<HashMap<_, _>>();
    for face in &topology.faces {
        let body = *body_by_face.get(&face.record_id)?;
        for edge in face.loops.iter().flat_map(|loop_| &loop_.edge_uses) {
            bodies_by_edge.get_mut(edge)?.insert(body);
            *uses[body].entry(*edge).or_default() += 1;
        }
    }
    if body_by_face.len() != topology.faces.len()
        || bodies_by_edge.values().any(|bodies| bodies.len() != 1)
    {
        return None;
    }
    Some(
        uses.into_iter()
            .map(|uses| {
                if uses.values().any(|count| *count > 2) {
                    BodyKind::General
                } else if !uses.is_empty() && uses.values().all(|count| *count == 2) {
                    BodyKind::Solid
                } else {
                    BodyKind::Sheet
                }
            })
            .collect(),
    )
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
    let mut b5_graph = crate::b5::parse(&scan.data);
    let mut fallback_surfaces = b5_graph
        .is_none()
        .then(|| freeform_surface_carriers(&scan.data));
    if fallback_surfaces.as_ref().is_some_and(Vec::is_empty)
        && geometry::a8_freeform_curves(&scan.data).is_empty()
    {
        return None;
    }
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    ir.source = Some(source_meta(scan));
    let payload_id = UnknownId("catia:payload:unknown#freeform".to_string());
    preserve_raw_payload(&mut ir, &mut annotations, scan, &payload_id.0).ok()?;
    let b5_complete = b5_graph.as_ref().is_some_and(|graph| graph.complete);
    let topology_transferred = b5_graph.take().is_some_and(|graph| {
        crate::b5_transfer::transfer(&mut ir, &mut annotations, graph, &payload_id)
    });
    if !topology_transferred {
        let surfaces = fallback_surfaces
            .take()
            .unwrap_or_else(|| freeform_surface_carriers(&scan.data));
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
    append_a8_rolling_ball_pools(&mut ir, &mut annotations, &scan.data);
    link_payload_carriers(&mut ir, &mut annotations).ok()?;
    ir.annotations = annotations.build();
    Some((
        ir,
        DecodeReport {
            format: "catia".to_string(),
            container_only: false,
            geometry_transferred: true,
            losses: if topology_transferred && b5_complete {
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

fn freeform_surface_carriers(data: &[u8]) -> Vec<(usize, u32, SurfaceGeometry, &'static str)> {
    let mut surfaces: Vec<(usize, u32, SurfaceGeometry, &str)> = geometry::a8_surfaces(data)
        .into_iter()
        .chain(geometry::a5_surfaces(data))
        .map(|surface| (surface.pos, surface.object_id, surface.geometry, "freeform"))
        .collect();
    surfaces.extend(
        geometry::b2_cylinders(data)
            .into_iter()
            .filter_map(|surface| {
                surface
                    .geometry
                    .map(|geometry| (surface.pos, 0, geometry, "b2_03_28"))
            }),
    );
    surfaces.extend(
        geometry::b2_embedded_cylinders(data)
            .into_iter()
            .filter_map(|surface| {
                surface
                    .cylinder
                    .geometry
                    .map(|geometry| (surface.pos, surface.object_id, geometry, "b2_03_60"))
            }),
    );
    surfaces.extend(geometry::b2_cones(data).into_iter().map(|surface| {
        (
            surface.pos,
            0,
            geometry::b2_cone_geometry(&surface),
            "b2_03_29",
        )
    }));
    surfaces
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

    for guide in geometry::a5_guide_curves(data) {
        let points = guide
            .sites
            .iter()
            .map(|site| site.point)
            .collect::<Vec<_>>();
        let first = guide
            .first_derivatives
            .iter()
            .map(|value| [value[0], value[1], value[2]])
            .collect::<Vec<_>>();
        let second = guide
            .second_derivatives
            .iter()
            .map(|value| [value[0], value[1], value[2]])
            .collect::<Vec<_>>();
        let Some((knots, control_points)) =
            geometry::quintic_jet_bspline3(guide.degree, &guide.knots, &points, &first, &second)
        else {
            continue;
        };
        let id = CurveId(format!("catia:guide:curve#{}", ir.model.curves.len()));
        annotate(
            annotations,
            &id,
            "consolidated_a5_03_39",
            guide.pos as u64,
            format!("header_token:{:08x}", guide.header_token),
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id,
            geometry: CurveGeometry::Nurbs(NurbsCurve {
                degree: guide.degree,
                knots,
                control_points: control_points
                    .into_iter()
                    .map(|point| Point3::new(point[0], point[1], point[2]))
                    .collect(),
                weights: None,
                periodic: false,
            }),
            source_object: None,
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
                multiplicities: vec![jet.degree + 1; jet.knots.len()],
                knots: jet.knots,
                sites,
            },
            cache_fit_tolerance: None,
        });
    }

    append_a8_rolling_ball_pools(ir, annotations, data);
}

fn append_a8_rolling_ball_pools(ir: &mut CadIr, annotations: &mut AnnotationBuilder, data: &[u8]) {
    for jet in geometry::a8_freeform_curves(data) {
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
        if jet.degree != 5
            || sites.len() != jet.knots.len()
            || jet.multiplicities.len() != jet.knots.len()
        {
            continue;
        }
        let surface_id = SurfaceId(format!(
            "catia:a8-rolling-ball:surf#{}",
            ir.model.surfaces.len()
        ));
        annotate(
            annotations,
            &surface_id,
            "object_stream_a8_03_32_cache",
            jet.pos as u64,
            format!("object_id:{:08x}", jet.object_id),
            Exactness::Unknown,
        );
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        });

        let procedural_id = ProceduralSurfaceId(format!(
            "catia:a8-rolling-ball:construction#{}",
            ir.model.procedural_surfaces.len()
        ));
        annotate(
            annotations,
            &procedural_id,
            "object_stream_a8_03_32",
            jet.pos as u64,
            format!(
                "object_id:{:08x}:multiplicities:{:?}",
                jet.object_id, jet.multiplicities
            ),
            Exactness::ByteExact,
        );
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: procedural_id,
            surface: surface_id,
            definition: ProceduralSurfaceDefinition::RollingBallJet {
                degree: jet.degree,
                multiplicities: jet.multiplicities,
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
    let face_count = topology::standard_face_count(brep).unwrap_or_default();
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
            surface_annotations.push((id.clone(), *pos, None, Exactness::Unknown));
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
                surface_annotations.push((
                    id.clone(),
                    prefix.pos,
                    Some(prefix.kind),
                    Exactness::ByteExact,
                ));
                surfaces.push(Surface {
                    id,
                    geometry: geom,
                    source_object: None,
                });
            }
            None => {
                if prefix.kind == 0x32 {
                    plane_faces += 1;
                }
                let id = SurfaceId(format!("catia:standard:surf#{i}"));
                if let Some(forward) = geometry::face_sense(brep, prefix) {
                    face_bindings.push((id.clone(), forward, prefix.pos));
                }
                surface_annotations.push((
                    id.clone(),
                    prefix.pos,
                    Some(prefix.kind),
                    Exactness::Unknown,
                ));
                surfaces.push(Surface {
                    id,
                    geometry: SurfaceGeometry::Unknown {
                        record: Some(UnknownId("catia:payload:unknown#brep-stream".to_string())),
                    },
                    source_object: None,
                });
            }
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
    for (id, offset, kind, exactness) in surface_annotations {
        annotate(
            &mut annotations,
            &id,
            "MainDataStream+SurfacicReps",
            offset as u64,
            kind.map_or_else(
                || "surfacic_reps_freeform_alias".to_string(),
                |kind| format!("surfacic_reps_{kind:02x}"),
            ),
            exactness,
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
        } else if !ir.model.vertices.is_empty() {
            attach_standard_free_vertices(&mut ir, &mut annotations);
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
    let groups = topology::standard_face_count(brep)
        .into_iter()
        .collect::<Vec<_>>();
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
            kind: cadmpeg_ir::topology::BodyKind::Sheet,
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

fn attach_standard_free_vertices(ir: &mut CadIr, annotations: &mut AnnotationBuilder) {
    let body_id = BodyId("catia:standard:body#unbound-points".to_string());
    let region_id = RegionId("catia:standard:region#unbound-points".to_string());
    let shell_id = ShellId("catia:standard:shell#unbound-points".to_string());
    for id in [&body_id.0, &region_id.0, &shell_id.0] {
        annotate(
            annotations,
            id,
            "MainDataStream+SurfacicReps",
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

fn attach_standard_topology(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    bindings: &[(SurfaceId, bool, usize)],
    brep: &[u8],
    source: &[u8],
) -> bool {
    let face_count = ir.model.faces.len();
    let mut supports = geometry::standard_curve_supports(brep, face_count);
    if supports.is_empty() {
        return false;
    }
    let serialized_edge_faces = supports
        .iter()
        .map(|support| support.faces)
        .collect::<Vec<_>>();
    let Some(edge_faces) = topology::resolve_standard_edge_faces(brep, &serialized_edge_faces)
    else {
        return false;
    };
    for (support, faces) in supports.iter_mut().zip(&edge_faces) {
        support.faces = *faces;
    }
    let surface_indices = ir
        .model
        .surfaces
        .iter()
        .enumerate()
        .map(|(index, surface)| (surface.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let mut endpoint_candidates = Vec::with_capacity(supports.len());
    let mut incidence_candidates = HashMap::<[usize; 2], Vec<usize>>::new();
    for support in &supports {
        let Some(surface0) = face_surface(ir, bindings, &surface_indices, support.faces[0]) else {
            return false;
        };
        let Some(surface1) = face_surface(ir, bindings, &surface_indices, support.faces[1]) else {
            return false;
        };
        let candidates = match &support.geometry {
            geometry::StandardCurveGeometry::Circle { center, radius } => {
                standard_circle_endpoint_candidates(
                    &ir.model.points,
                    *center,
                    *radius,
                    Some((&surface0.geometry, &surface1.geometry)),
                )
            }
            geometry::StandardCurveGeometry::Line | geometry::StandardCurveGeometry::Bspline => {
                let mut faces = support.faces;
                faces.sort_unstable();
                incidence_candidates
                    .entry(faces)
                    .or_insert_with(|| {
                        ir.model
                            .points
                            .iter()
                            .enumerate()
                            .filter_map(|(index, point)| {
                                let incident =
                                    point_on_known_surface(point.position, &surface0.geometry)
                                        && point_on_known_surface(
                                            point.position,
                                            &surface1.geometry,
                                        );
                                incident.then_some(index)
                            })
                            .collect()
                    })
                    .clone()
            }
        };
        endpoint_candidates.push(candidates);
    }
    let mut edge_classes = Vec::with_capacity(supports.len());
    for (edge, support) in supports.iter().enumerate() {
        let class = supports[..edge]
            .iter()
            .position(|candidate| {
                let mut candidate_faces = candidate.faces;
                candidate_faces.sort_unstable();
                let mut support_faces = support.faces;
                support_faces.sort_unstable();
                candidate_faces == support_faces
                    && match (&candidate.geometry, &support.geometry) {
                        (
                            geometry::StandardCurveGeometry::Circle {
                                center: left_center,
                                radius: left_radius,
                            },
                            geometry::StandardCurveGeometry::Circle {
                                center: right_center,
                                radius: right_radius,
                            },
                        ) => {
                            left_center.x.to_bits() == right_center.x.to_bits()
                                && left_center.y.to_bits() == right_center.y.to_bits()
                                && left_center.z.to_bits() == right_center.z.to_bits()
                                && left_radius.to_bits() == right_radius.to_bits()
                        }
                        (
                            geometry::StandardCurveGeometry::Line,
                            geometry::StandardCurveGeometry::Line,
                        ) => true,
                        _ => false,
                    }
            })
            .map_or(edge, |candidate| edge_classes[candidate]);
        edge_classes.push(class);
    }
    let native_edges = crate::b5::edge_vertex_references(source);
    let graph_endpoint_pairs =
        standard_native_graph_endpoint_pairs(source, &supports, &native_edges, &ir.model.points);
    let native_ports = supports
        .iter()
        .map(|support| native_edges.get(&support.tag).copied())
        .collect::<Option<Vec<_>>>();
    let roster_endpoint_pairs = geometry::standard_vertex_roster(source, ir.model.points.len())
        .map(|roster| {
            let point_by_identity = roster
                .into_iter()
                .enumerate()
                .map(|(point, identity)| (identity, point))
                .collect::<HashMap<_, _>>();
            supports
                .iter()
                .map(|support| {
                    let identities = native_edges.get(&support.tag)?;
                    Some([
                        *point_by_identity.get(&identities[0])?,
                        *point_by_identity.get(&identities[1])?,
                    ])
                })
                .collect::<Vec<_>>()
        });
    if let Some(pairs) = &graph_endpoint_pairs {
        include_native_endpoint_pairs(&mut endpoint_candidates, pairs);
    }
    if let Some(pairs) = &roster_endpoint_pairs {
        include_native_endpoint_pairs(&mut endpoint_candidates, pairs);
    }
    let mut endpoint_options = resolve_standard_endpoint_pairs(
        ir,
        bindings,
        &surface_indices,
        &supports,
        &endpoint_candidates,
    );
    if let (Some(options), Some(pairs)) = (&mut endpoint_options, &graph_endpoint_pairs) {
        for (options, pair) in options.iter_mut().zip(pairs) {
            if let Some(pair) = pair {
                *options = vec![*pair];
            }
        }
    }
    if let (Some(options), Some(pairs)) = (&mut endpoint_options, &roster_endpoint_pairs) {
        for (options, pair) in options.iter_mut().zip(pairs) {
            if let Some(pair) = pair {
                *options = vec![*pair];
            }
        }
    }
    if let Some(options) = &mut endpoint_options {
        for (edge, pairs) in options.iter_mut().enumerate() {
            let support = &supports[edge];
            if matches!(support.geometry, geometry::StandardCurveGeometry::Bspline) {
                continue;
            }
            let unfiltered = pairs.clone();
            pairs.retain(|pair| {
                let Some(start) = ir.model.points.get(pair[0]).map(|point| point.position) else {
                    return false;
                };
                let Some(end) = ir.model.points.get(pair[1]).map(|point| point.position) else {
                    return false;
                };
                support.faces.iter().all(|&face| {
                    let Some(surface) = face_surface(ir, bindings, &surface_indices, face) else {
                        return false;
                    };
                    standard_pcurve_geometry(
                        &surface.geometry,
                        support,
                        start,
                        end,
                        geometry::standard_face_witness(brep, bindings[face].2),
                    )
                    .is_some()
                })
            });
            if pairs.is_empty() {
                *pairs = unfiltered;
            }
        }
    }
    if let Some(options) = &mut endpoint_options {
        loop {
            let seeds = options
                .iter()
                .map(|pairs| {
                    <[[usize; 2]; 1]>::try_from(pairs.as_slice())
                        .ok()
                        .map(|[pair]| pair)
                })
                .collect::<Vec<_>>();
            let mut changed = false;
            if let Some(placement_domains) =
                topology::standard_mesh_placement_endpoint_pairs(brep, &edge_faces, &seeds)
            {
                for (edge, domain) in placement_domains.into_iter().enumerate() {
                    if domain.is_empty() {
                        continue;
                    }
                    let previous = options[edge].clone();
                    if options[edge].is_empty() {
                        options[edge] = domain;
                    } else {
                        options[edge].retain(|pair| {
                            domain
                                .iter()
                                .any(|candidate| topology::same_unordered_pair(*pair, *candidate))
                        });
                    }
                    changed |= options[edge] != previous;
                }
            }
            if let Some(boundary_domains) = options
                .iter()
                .all(|domain| !domain.is_empty())
                .then(|| {
                    topology::standard_mesh_prune_endpoint_candidates(brep, &edge_faces, options)
                })
                .flatten()
            {
                for (edge, domain) in boundary_domains.into_iter().enumerate() {
                    let previous = options[edge].clone();
                    if options[edge].is_empty() {
                        options[edge] = domain;
                    } else {
                        options[edge].retain(|pair| {
                            domain
                                .iter()
                                .any(|candidate| topology::same_unordered_pair(*pair, *candidate))
                        });
                    }
                    changed |= options[edge] != previous;
                }
            }
            if !changed {
                break;
            }
        }
        for (candidates, options) in endpoint_candidates.iter_mut().zip(options) {
            for point in options.iter().flatten() {
                if !candidates.contains(point) {
                    candidates.push(*point);
                }
            }
        }
    }
    let graph_propagated_pairs = native_ports
        .as_ref()
        .zip(graph_endpoint_pairs.as_ref())
        .and_then(|(ports, pairs)| topology::propagate_edge_port_points(ports, pairs))
        .and_then(|pairs| pairs.into_iter().collect::<Option<Vec<_>>>());
    let native_endpoint_pairs = graph_propagated_pairs
        .or_else(|| {
            roster_endpoint_pairs
                .as_ref()?
                .iter()
                .copied()
                .collect::<Option<Vec<_>>>()
        })
        .or_else(|| {
            endpoint_options.as_ref().and_then(|options| {
                const MAX_NATIVE_PORT_CHOICES: usize = 65_536;
                const MAX_NATIVE_PORT_WORK: usize = 20_000_000;

                let ports = native_ports.as_ref()?;
                let seeds = options
                    .iter()
                    .map(|choices| {
                        <[[usize; 2]; 1]>::try_from(choices.as_slice())
                            .ok()
                            .map(|[pair]| pair)
                    })
                    .collect::<Vec<_>>();
                let propagated = topology::propagate_edge_port_points(ports, &seeds)?;
                if let Some(complete) = propagated.iter().copied().collect::<Option<Vec<_>>>() {
                    return Some(complete);
                }
                // Exhaustive binding is a fallback after exact identity propagation.
                // Large symmetric choice sets remain unresolved and continue through
                // trim-mesh and incidence paths instead of making decode unbounded.
                let choice_count = options.iter().map(Vec::len).sum::<usize>();
                (choice_count <= MAX_NATIVE_PORT_CHOICES
                    && options
                        .len()
                        .checked_mul(choice_count)
                        .is_some_and(|work| work <= MAX_NATIVE_PORT_WORK))
                .then(|| topology::bind_edge_port_candidates(ports, options))?
            })
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
    let propagated_endpoint_pairs = combine_propagated_endpoint_pairs(
        propagated_endpoint_pairs,
        mesh_propagated_endpoint_pairs,
    );
    let mut constrained_endpoint_options = endpoint_options.as_ref().map(|options| {
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
    if let (Some(options), Some(ports)) = (
        constrained_endpoint_options.as_mut(),
        topology::standard_mesh_edge_ports(brep),
    ) {
        if let Some(pruned) = topology::prune_edge_candidates_by_port_domains(&ports, options) {
            *options = pruned;
        }
    }
    let resolved_endpoint_pairs = propagated_endpoint_pairs
        .and_then(|pairs| pairs.into_iter().collect::<Option<Vec<[usize; 2]>>>());
    if let Some(pairs) = &resolved_endpoint_pairs {
        let pairs = pairs.iter().copied().map(Some).collect::<Vec<_>>();
        include_native_endpoint_pairs(&mut endpoint_candidates, &pairs);
    }
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
    let (mut topology, point_assignment) = if let Some(bound) = mesh_bound {
        bound
    } else if let Some(topology) = native_endpoint_pairs.as_ref().and_then(|pairs| {
        topology::parse_standard_endpoints_with_edge_classes(
            brep,
            &edge_faces,
            pairs,
            Some(&edge_classes),
        )
    }) {
        let point_assignment = (0..ir.model.points.len()).collect();
        (topology, point_assignment)
    } else if let Some(bound) = constrained_endpoint_options.as_ref().and_then(|options| {
        topology::parse_standard_mesh_incidence_candidates(brep, &edge_faces, options, |pairs| {
            standard_circle_pair_solution_is_simple(
                ir,
                bindings,
                &surface_indices,
                brep,
                &supports,
                pairs,
            )
        })
        .or_else(|| topology::parse_standard_mesh_endpoint_candidates(brep, &edge_faces, options))
    }) {
        bound
    } else if let Some(topology) = constrained_endpoint_options.as_ref().and_then(|options| {
        const MAX_INCIDENCE_SEARCH_WORK: usize = 10_000_000;

        let choice_count = options.iter().map(Vec::len).sum::<usize>();
        options
            .len()
            .checked_mul(choice_count)
            .filter(|work| *work <= MAX_INCIDENCE_SEARCH_WORK)?;
        topology::standard_mesh_edge_ports(brep)
            .and_then(|ports| {
                topology::parse_standard_port_endpoint_candidates(
                    brep,
                    &edge_faces,
                    options,
                    &ports,
                )
            })
            .or_else(|| topology::parse_standard_endpoint_candidates(brep, &edge_faces, options))
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
    let face_groups = vec![topology.face_count()];
    if topology.orient_solid_body_cycles(&face_groups).is_none() {
        return false;
    }
    let Some(body_kinds) = topology.body_kinds(&face_groups) else {
        return false;
    };
    let Some(edge_vertices) = topology.edge_vertices() else {
        return false;
    };
    if edge_vertices.iter().enumerate().any(|(edge, vertices)| {
        let start = point_assignment[vertices[0]];
        let end = point_assignment[vertices[1]];
        !endpoint_candidates[edge].is_empty()
            && (!endpoint_candidates[edge].contains(&start)
                || !endpoint_candidates[edge].contains(&end))
    }) {
        return false;
    }

    for (edge_index, (support, logical_vertices)) in supports.iter().zip(&edge_vertices).enumerate()
    {
        let start_point = point_assignment[logical_vertices[0]];
        let end_point = point_assignment[logical_vertices[1]];
        let (curve, param_range) = build_standard_edge_curve(
            ir,
            annotations,
            bindings,
            &surface_indices,
            brep,
            support,
            [start_point, end_point],
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
        if param_range.is_some() {
            annotations.derived(&id, "param_range");
        }
        ir.model.edges.push(Edge {
            id,
            curve,
            start: VertexId(format!("catia:standard:v#{start_point}")),
            end: VertexId(format!("catia:standard:v#{end_point}")),
            param_range,
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
                    geometry::standard_face_witness(brep, bindings[face_index].2),
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
                ] {
                    annotations.derived(&id, field);
                }
                if pcurve_id.is_some() {
                    annotations.derived(&id, "pcurve");
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
    for (body_index, kind) in body_kinds.into_iter().enumerate() {
        let id = BodyId(format!("catia:standard:body#{body_index}"));
        let Some(body) = ir.model.bodies.iter_mut().find(|body| body.id == id) else {
            return false;
        };
        body.kind = kind;
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
        let relation_count = points
            .len()
            .checked_mul(points.len().saturating_sub(1))
            .and_then(|value| value.checked_div(2));
        if relation_count.is_none_or(|count| count > MAX_PAIR_RELATIONS_PER_EDGE) {
            continue;
        }
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
        if pairs.len() == edges.len() {
            for (edge, pair) in edges.into_iter().zip(pairs) {
                resolved[edge] = vec![pair];
            }
        } else {
            for edge in edges {
                resolved[edge].clone_from(&pairs);
            }
        }
    }
    let mut fallback_relation_budget = 65_536usize;
    for (edge, pairs) in resolved.iter_mut().enumerate() {
        if !pairs.is_empty() {
            continue;
        }
        let points = &candidates[edge];
        let relation_count = points
            .len()
            .checked_mul(points.len().saturating_sub(1))
            .and_then(|value| value.checked_div(2));
        let Some(relation_count) = relation_count.filter(|count| {
            *count <= MAX_PAIR_RELATIONS_PER_EDGE && *count <= fallback_relation_budget
        }) else {
            continue;
        };
        fallback_relation_budget -= relation_count;
        *pairs = points
            .iter()
            .enumerate()
            .flat_map(|(left, &start)| points[left + 1..].iter().map(move |&end| [start, end]))
            .collect();
    }
    Some(resolved)
}

fn standard_circle_endpoint_candidates(
    points: &[Point],
    center: Point3,
    radius: f64,
    surfaces: Option<(&SurfaceGeometry, &SurfaceGeometry)>,
) -> Vec<usize> {
    points
        .iter()
        .enumerate()
        .filter_map(|(index, point)| {
            let on_circle =
                (point_distance_squared(point.position, center).sqrt() - radius).abs() <= 1e-3;
            let incident = surfaces.is_none_or(|(left, right)| {
                point_on_known_surface(point.position, left)
                    && point_on_known_surface(point.position, right)
            });
            (on_circle && incident).then_some(index)
        })
        .collect()
}

fn standard_native_graph_endpoint_pairs(
    source: &[u8],
    supports: &[geometry::StandardCurveSupport],
    native_edges: &BTreeMap<u32, [u32; 2]>,
    points: &[Point],
) -> Option<Vec<Option<[usize; 2]>>> {
    let graph = crate::b5::parse(source)?;
    let identity_points = unique_native_identity_points(
        &graph.logical_vertex_refs,
        &graph.logical_vertex_points,
        graph.vertex_points.len(),
        &graph.vertex_tolerances,
        points,
    );
    Some(
        supports
            .iter()
            .map(|support| {
                let identities = native_edges.get(&support.tag)?;
                Some([
                    *identity_points.get(&identities[0])?,
                    *identity_points.get(&identities[1])?,
                ])
            })
            .collect(),
    )
}

fn include_native_endpoint_pairs(candidates: &mut [Vec<usize>], pairs: &[Option<[usize; 2]>]) {
    for (candidates, pair) in candidates.iter_mut().zip(pairs) {
        if let Some(pair) = pair {
            for point in pair {
                if !candidates.contains(point) {
                    candidates.push(*point);
                }
            }
        }
    }
}

fn combine_propagated_endpoint_pairs(
    raw: Option<Vec<Option<[usize; 2]>>>,
    mesh: Option<Vec<Option<[usize; 2]>>>,
) -> Option<Vec<Option<[usize; 2]>>> {
    let pairs = match (raw, mesh) {
        (_, Some(mesh)) if mesh.iter().all(Option::is_some) => mesh,
        (Some(raw), _) if raw.iter().all(Option::is_some) => raw,
        (Some(raw), Some(mesh)) => raw
            .into_iter()
            .zip(mesh)
            .map(|(raw, mesh)| match (raw, mesh) {
                (Some(raw), Some(mesh)) if raw == mesh || raw == [mesh[1], mesh[0]] => Some(raw),
                (Some(_), Some(_)) => None,
                (Some(pair), None) | (None, Some(pair)) => Some(pair),
                (None, None) => None,
            })
            .collect(),
        (Some(pairs), None) | (None, Some(pairs)) => pairs,
        (None, None) => return None,
    };
    (!pairs.is_empty()).then_some(pairs)
}

fn unique_native_identity_points(
    identities: &[u32],
    coordinates: &[[f64; 3]],
    raw_point_count: usize,
    tolerances: &BTreeMap<usize, f64>,
    points: &[Point],
) -> HashMap<u32, usize> {
    const MATCH_TOLERANCE: f64 = 2e-3;

    identities
        .iter()
        .copied()
        .zip(coordinates)
        .enumerate()
        .filter_map(|(rank, (identity, coordinate))| {
            let tolerance = tolerances
                .get(&(raw_point_count + rank))
                .copied()
                .unwrap_or(MATCH_TOLERANCE)
                .max(MATCH_TOLERANCE);
            let matches = points
                .iter()
                .enumerate()
                .filter_map(|(index, point)| {
                    (point_distance_squared(
                        point.position,
                        Point3::new(coordinate[0], coordinate[1], coordinate[2]),
                    )
                    .sqrt()
                        <= tolerance)
                        .then_some(index)
                })
                .collect::<Vec<_>>();
            <[usize; 1]>::try_from(matches)
                .ok()
                .map(|[point]| (identity, point))
        })
        .collect()
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
    witness: Option<Point3>,
) -> Option<(PcurveGeometry, [f64; 2])> {
    if !point_on_surface(start, surface) || !point_on_surface(end, surface) {
        return None;
    }
    let mut uv = [
        analytic_surface_uv(surface, start)?,
        analytic_surface_uv(surface, end)?,
    ];
    let reference_uv = uv[0];
    unwrap_standard_uv(surface, &mut uv[1], reference_uv);

    if let (geometry::StandardCurveGeometry::Circle { center, radius }, Some(witness)) =
        (&support.geometry, witness)
    {
        uv[1] = witnessed_surface_circle_end(surface, *center, *radius, uv, witness)?;
    }

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

fn witness_arc_end(start: f64, short_end: f64, witness: f64) -> Option<f64> {
    let delta = short_end - start;
    if delta.abs() <= 1e-9 {
        return None;
    }
    let long_end = short_end - delta.signum() * std::f64::consts::TAU;
    let contains = |end: f64| {
        (-2..=2).any(|turn| {
            let witness = witness + f64::from(turn) * std::f64::consts::TAU;
            witness >= start.min(end) + 1e-6 && witness <= start.max(end) - 1e-6
        })
    };
    match (contains(short_end), contains(long_end)) {
        (true, false) => Some(short_end),
        (false, true) => Some(long_end),
        _ => None,
    }
}

fn witnessed_surface_circle_end(
    surface: &SurfaceGeometry,
    center: Point3,
    radius: f64,
    uv: [Point2; 2],
    witness: Point3,
) -> Option<Point2> {
    let witness_uv = analytic_surface_uv(surface, witness)?;
    let lanes: &[usize] = match surface {
        SurfaceGeometry::Cylinder { .. }
        | SurfaceGeometry::Cone { .. }
        | SurfaceGeometry::Sphere { .. } => &[0],
        SurfaceGeometry::Torus { .. } => &[0, 1],
        _ => return None,
    };
    let candidates = lanes
        .iter()
        .filter_map(|lane| {
            let mut candidate = uv[1];
            let (start, short_end, witness) = if *lane == 0 {
                (uv[0].u, uv[1].u, witness_uv.u)
            } else {
                (uv[0].v, uv[1].v, witness_uv.v)
            };
            let selected = witness_arc_end(start, short_end, witness)?;
            if *lane == 0 {
                candidate.u = selected;
            } else {
                candidate.v = selected;
            }
            let midpoint = cadmpeg_ir::eval::surface_point(
                surface,
                0.5 * (uv[0].u + candidate.u),
                0.5 * (uv[0].v + candidate.v),
            )?;
            ((point_distance_squared(midpoint, center).sqrt() - radius).abs() <= 2e-3)
                .then_some(candidate)
        })
        .collect::<Vec<_>>();
    <[Point2; 1]>::try_from(candidates).ok().map(|[end]| end)
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
    brep: &[u8],
    support: &geometry::StandardCurveSupport,
    points: [usize; 2],
) -> (Option<CurveId>, Option<[f64; 2]>) {
    let (geometry, mut param_range) = match &support.geometry {
        geometry::StandardCurveGeometry::Line => {
            let start = ir.model.points[points[0]].position;
            let end = ir.model.points[points[1]].position;
            let delta = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
            let length = axis_dot(delta, delta).sqrt();
            if length <= f64::EPSILON {
                return (None, None);
            }
            (
                CurveGeometry::Line {
                    origin: start,
                    direction: Vector3::new(delta.x / length, delta.y / length, delta.z / length),
                },
                Some([0.0, length]),
            )
        }
        geometry::StandardCurveGeometry::Circle { center, radius } => {
            let start = ir.model.points[points[0]].position;
            let end = ir.model.points[points[1]].position;
            let axes: Vec<Vector3> = support
                .faces
                .iter()
                .filter_map(|face| face_surface(ir, bindings, surface_indices, *face))
                .filter_map(|surface| circle_axis_from_carrier(*center, *radius, &surface.geometry))
                .collect();
            let Some(axis) = axes.first().copied() else {
                return (None, None);
            };
            if axes
                .iter()
                .skip(1)
                .any(|other| axis_dot(axis, *other).abs() < 0.9999)
            {
                return (None, None);
            }
            let candidates = [axis, scale_vector(axis, -1.0)]
                .into_iter()
                .filter_map(|axis| {
                    let ref_direction = cadmpeg_ir::geometry::derive_reference_direction(axis);
                    let range = standard_circle_param_range(
                        ir,
                        bindings,
                        surface_indices,
                        brep,
                        support,
                        *center,
                        *radius,
                        axis,
                        ref_direction,
                        start,
                        end,
                    )?;
                    Some((axis, ref_direction, canonical_periodic_range(range)?))
                })
                .collect::<Vec<_>>();
            let (axis, ref_direction, param_range) = match candidates.as_slice() {
                [(axis, reference, range)] => (*axis, *reference, Some(*range)),
                _ => (
                    axis,
                    cadmpeg_ir::geometry::derive_reference_direction(axis),
                    None,
                ),
            };
            (
                CurveGeometry::Circle {
                    center: *center,
                    axis,
                    ref_direction,
                    radius: *radius,
                },
                param_range,
            )
        }
        geometry::StandardCurveGeometry::Bspline => (
            CurveGeometry::Unknown {
                record: Some(UnknownId("catia:payload:unknown#brep-stream".to_string())),
            },
            None,
        ),
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
    if matches!(&support.geometry, geometry::StandardCurveGeometry::Bspline) {
        let sides = support.faces.map(|face| {
            let surface = bindings
                .get(face)
                .and_then(|(id, _, _)| surface_indices.get(id).map(|_| id.clone()));
            IntcurveSupportSide {
                surface,
                pcurve: None,
            }
        });
        if sides.iter().all(|side| side.surface.is_some()) && sides[0].surface != sides[1].surface {
            let procedural_id =
                ProceduralCurveId(format!("catia:standard:intersection#{}", support.pos));
            annotate(
                annotations,
                &procedural_id,
                "MainDataStream+SurfacicReps",
                support.pos as u64,
                "standard_radial_surface_intersection",
                Exactness::Derived,
            );
            annotations
                .derived(&procedural_id, "curve")
                .derived(&procedural_id, "definition");
            ir.model.procedural_curves.push(ProceduralCurve {
                id: procedural_id,
                curve: id.clone(),
                definition: ProceduralCurveDefinition::Intersection {
                    context: IntcurveSupportContext {
                        sides,
                        parameter_range: [0.0, 1.0],
                        discontinuities: std::array::from_fn(|_| Vec::new()),
                    },
                    discontinuity_flag: false,
                },
                cache_fit_tolerance: None,
            });
            param_range = Some([0.0, 1.0]);
        }
    }
    (Some(id), param_range)
}

fn standard_circle_pair_solution_is_simple(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    brep: &[u8],
    supports: &[geometry::StandardCurveSupport],
    pairs: &[[usize; 2]],
) -> bool {
    type CircleFaceKey = (u64, u64, u64, u64, usize);

    let mut ranges = HashMap::<CircleFaceKey, Vec<[f64; 2]>>::new();
    for (support, pair) in supports.iter().zip(pairs) {
        let geometry::StandardCurveGeometry::Circle { center, radius } = &support.geometry else {
            continue;
        };
        let Some(start) = ir.model.points.get(pair[0]).map(|point| point.position) else {
            return false;
        };
        let Some(end) = ir.model.points.get(pair[1]).map(|point| point.position) else {
            return false;
        };
        let axes = support
            .faces
            .iter()
            .filter_map(|face| face_surface(ir, bindings, surface_indices, *face))
            .filter_map(|surface| circle_axis_from_carrier(*center, *radius, &surface.geometry))
            .collect::<Vec<_>>();
        let Some(axis) = axes.first().copied() else {
            continue;
        };
        if axes
            .iter()
            .skip(1)
            .any(|other| axis_dot(axis, *other).abs() < 0.9999)
        {
            return false;
        }
        let candidates = [axis, scale_vector(axis, -1.0)]
            .into_iter()
            .filter_map(|axis| {
                let reference = cadmpeg_ir::geometry::derive_reference_direction(axis);
                let range = standard_circle_param_range(
                    ir,
                    bindings,
                    surface_indices,
                    brep,
                    support,
                    *center,
                    *radius,
                    axis,
                    reference,
                    start,
                    end,
                )?;
                canonical_periodic_range(range)
            })
            .collect::<Vec<_>>();
        let [range] = candidates.as_slice() else {
            continue;
        };
        for &face in &support.faces {
            let key = (
                center.x.to_bits(),
                center.y.to_bits(),
                center.z.to_bits(),
                radius.to_bits(),
                face,
            );
            ranges.entry(key).or_default().push(*range);
        }
    }
    ranges
        .values()
        .all(|ranges| circular_ranges_are_nonoverlapping_or_coincident(ranges))
}

fn circular_ranges_are_nonoverlapping_or_coincident(ranges: &[[f64; 2]]) -> bool {
    fn segments(range: [f64; 2]) -> Vec<[f64; 2]> {
        let span = range[1] - range[0];
        let start = range[0].rem_euclid(std::f64::consts::TAU);
        let end = start + span;
        if end <= std::f64::consts::TAU {
            vec![[start, end]]
        } else {
            vec![
                [start, std::f64::consts::TAU],
                [0.0, end - std::f64::consts::TAU],
            ]
        }
    }

    let coincident = ranges.iter().skip(1).all(|range| {
        (range[0] - ranges[0][0]).abs() <= 1e-9 && (range[1] - ranges[0][1]).abs() <= 1e-9
    });
    coincident
        || ranges.iter().enumerate().all(|(left_index, left)| {
            ranges[left_index + 1..].iter().all(|right| {
                !segments(*left).iter().any(|left| {
                    segments(*right)
                        .iter()
                        .any(|right| left[1].min(right[1]) - left[0].max(right[0]) > 1e-6)
                })
            })
        })
}

#[allow(clippy::too_many_arguments)]
fn standard_circle_param_range(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    brep: &[u8],
    support: &geometry::StandardCurveSupport,
    center: Point3,
    radius: f64,
    axis: Vector3,
    ref_direction: Vector3,
    start: Point3,
    end: Point3,
) -> Option<[f64; 2]> {
    let mut ranges = support.faces.iter().filter_map(|face| {
        let surface = face_surface(ir, bindings, surface_indices, *face)?;
        let witness = geometry::standard_face_witness(brep, bindings.get(*face)?.2)?;
        let (PcurveGeometry::Line { origin, direction }, _) =
            standard_pcurve_geometry(&surface.geometry, support, start, end, Some(witness))?
        else {
            return None;
        };
        circle_parameter_range_from_surface_branch(
            &surface.geometry,
            center,
            radius,
            axis,
            ref_direction,
            start,
            end,
            origin,
            direction,
        )
    });
    let range = ranges.next()?;
    if ranges.any(|other| (other[0] - range[0]).abs() > 1e-9 || (other[1] - range[1]).abs() > 1e-9)
    {
        return None;
    }
    Some(range)
}

#[allow(clippy::too_many_arguments)]
fn circle_parameter_range_from_surface_branch(
    surface: &SurfaceGeometry,
    center: Point3,
    radius: f64,
    axis: Vector3,
    ref_direction: Vector3,
    start: Point3,
    end: Point3,
    pcurve_origin: Point2,
    pcurve_direction: Point2,
) -> Option<[f64; 2]> {
    let tangent = cross_vector(axis, ref_direction);
    let angle = |point: Point3| {
        let offset = point_delta(point, center);
        axis_dot(offset, tangent).atan2(axis_dot(offset, ref_direction))
    };
    let start = angle(start);
    let short_end = unwrap_angle(angle(end), start);
    let delta = short_end - start;
    if delta.abs() <= 1e-9 {
        return None;
    }
    let long_end = short_end - delta.signum() * std::f64::consts::TAU;
    let surface_midpoint = cadmpeg_ir::eval::surface_point(
        surface,
        pcurve_origin.u + 0.5 * pcurve_direction.u,
        pcurve_origin.v + 0.5 * pcurve_direction.v,
    )?;
    let candidates = [short_end, long_end]
        .into_iter()
        .filter(|end| {
            let parameter = 0.5 * (start + end);
            let circle_midpoint = Point3::new(
                center.x
                    + radius * (parameter.cos() * ref_direction.x + parameter.sin() * tangent.x),
                center.y
                    + radius * (parameter.cos() * ref_direction.y + parameter.sin() * tangent.y),
                center.z
                    + radius * (parameter.cos() * ref_direction.z + parameter.sin() * tangent.z),
            );
            point_distance_squared(circle_midpoint, surface_midpoint).sqrt() <= 2e-3
        })
        .collect::<Vec<_>>();
    <[f64; 1]>::try_from(candidates)
        .ok()
        .map(|[end]| [start, end])
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
                  lines are transferred as curves. Standard spline edges retain exact \
                  two-surface intersection constructions, but their serialized NURBS caches, \
                  persistent object tags, materials, and document metadata are not yet \
                  transferred."
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
