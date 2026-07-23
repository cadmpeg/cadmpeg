// SPDX-License-Identifier: Apache-2.0
//! Standard nested-stream decode route: B-rep topology attach and geometry.

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, Pcurve, PcurveGeometry,
    ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition,
    Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::assemble::cgm_source;
use crate::assemble::{
    annotate, attach_free_vertices, build_geometry_report,
    circle_parameter_range_from_surface_branch, link_payload_carriers, neutral_model_is_admissible,
    ordered_range, preserve_raw_payload, rational_pcurve_arc, source_meta, unit_vector,
    unwrap_angle, TypedCounts,
};
use crate::container::{self, ContainerScan};
use crate::families::freeform::append_freeform_surface_pools;
use crate::families::standard::{fbb, topology};
use crate::families::FamilyOutput;
use crate::solve::{mesh_quotient, missing_edge};

/// Decode the standard-nested vertex cloud and analytic surface carriers. Returns
/// `None` when the reconstructed stream yields neither vertices nor surfaces, so
/// the caller falls back to the container-metadata path.
pub(crate) fn emit_standard_extrusion_definition(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    surfaces: &mut Vec<Surface>,
    procedural_supports: &mut HashMap<u32, SurfaceId>,
    extrusion_definitions: &mut HashMap<u32, ProceduralSurfaceDefinition>,
    extrusion: crate::families::b5::transfer::ResolvedExtrusionSurface,
) -> ProceduralSurfaceDefinition {
    if let Some(definition) = extrusion_definitions.get(&extrusion.surface_object_id) {
        return definition.clone();
    }
    let surface_object_id = extrusion.surface_object_id;
    let sides = extrusion.supports.map(|side| {
        let support_id = procedural_supports
            .entry(side.surface_object_id)
            .or_insert_with(|| {
                let id = SurfaceId(format!(
                    "catia:standard:procedural-support#{}",
                    side.surface_object_id
                ));
                annotate(
                    annotations,
                    &id,
                    "object_stream_b5_03",
                    0,
                    format!("surface:{:08x}", side.surface_object_id),
                    Exactness::ByteExact,
                );
                surfaces.push(Surface {
                    id: id.clone(),
                    geometry: side.surface,
                    source_object: Some(cgm_source("surface", side.surface_object_id)),
                });
                id
            })
            .clone();
        IntcurveSupportSide {
            surface: Some(support_id),
            pcurve: Some(side.pcurve),
            pcurve_parameter_range: (side.pcurve_parameter_range
                != extrusion.directrix_parameter_range)
                .then_some(side.pcurve_parameter_range),
        }
    });
    let directrix_id = CurveId(format!(
        "catia:standard:extrusion-directrix#{}",
        extrusion.directrix_object_id
    ));
    annotate(
        annotations,
        &directrix_id,
        "object_stream_a8_03_25",
        0,
        "two_support_directrix",
        Exactness::Unknown,
    );
    ir.model.curves.push(Curve {
        id: directrix_id.clone(),
        geometry: CurveGeometry::Unknown { record: None },
        source_object: Some(cgm_source("curve", extrusion.directrix_object_id)),
    });
    let directrix_procedure = ProceduralCurveId(format!(
        "catia:standard:extrusion-directrix-procedure#{}",
        extrusion.directrix_object_id
    ));
    annotate(
        annotations,
        &directrix_procedure,
        "object_stream_a8_03_25",
        0,
        "two_surface_pcurve_intersection",
        Exactness::Derived,
    );
    ir.model.procedural_curves.push(ProceduralCurve {
        id: directrix_procedure,
        curve: directrix_id.clone(),
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides,
                parameter_range: extrusion.directrix_parameter_range,
                discontinuities: std::array::from_fn(|_| Vec::new()),
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: Some(extrusion.cache_fit_tolerance),
    });
    let definition = ProceduralSurfaceDefinition::Extrusion {
        directrix: directrix_id,
        parameter_interval: Some(extrusion.directrix_parameter_range),
        direction: extrusion.direction,
        native_position: None,
    };
    extrusion_definitions.insert(surface_object_id, definition.clone());
    definition
}

pub(crate) fn try_decode_standard(scan: &ContainerScan) -> Option<FamilyOutput> {
    let brep = scan.brep.as_ref()?;
    let points = fbb::standard_vertex_points(brep)
        .unwrap_or_default()
        .into_iter()
        .map(|[x, y, z]| Point3::new(x, y, z))
        .collect::<Vec<_>>();
    let vertex_roster =
        crate::families::standard::records::standard_vertex_roster(&scan.data, points.len());
    let face_count = fbb::standard_face_count(brep).unwrap_or_default();
    let records = crate::families::standard::records::standard_surface_records(brep, face_count)
        .unwrap_or_else(|| {
            crate::families::standard::records::surface_prefixes(brep)
                .into_iter()
                .map(crate::families::standard::records::StandardSurfaceRecord::Analytic)
                .collect()
        });
    let analytic_record_count = records
        .iter()
        .filter(|record| {
            matches!(
                record,
                crate::families::standard::records::StandardSurfaceRecord::Analytic(_)
            )
        })
        .count();
    let freeform_tags = records
        .iter()
        .filter_map(|record| match record {
            crate::families::standard::records::StandardSurfaceRecord::Freeform { tag, .. } => {
                Some(*tag)
            }
            crate::families::standard::records::StandardSurfaceRecord::Analytic(_) => None,
        })
        .collect::<HashSet<_>>();
    let object_evidence = standard_object_evidence(scan, &freeform_tags);
    let freeform_geometries = &object_evidence.surface_geometries;
    let freeform_procedural_surfaces = &object_evidence.procedural_surfaces;
    let unresolved_freeform_record_count = records
        .iter()
        .filter(|record| {
            matches!(
                record,
                crate::families::standard::records::StandardSurfaceRecord::Freeform { tag, .. }
                    if !freeform_geometries.contains_key(tag)
                        && !freeform_procedural_surfaces.contains_key(tag)
            )
        })
        .count();
    let face_frame_vectors = fbb::standard_face_frame_vectors(brep);
    let curve_supports =
        crate::families::standard::records::standard_curve_supports(brep, face_count);
    let curved_surfaces = records
        .iter()
        .map(|record| match record {
            crate::families::standard::records::StandardSurfaceRecord::Analytic(prefix)
                if prefix.kind != 0x32 =>
            {
                crate::families::standard::records::decode_curved(brep, prefix)
            }
            crate::families::standard::records::StandardSurfaceRecord::Analytic(_)
            | crate::families::standard::records::StandardSurfaceRecord::Freeform { .. } => None,
        })
        .collect::<Vec<_>>();
    let mut plane_normal_candidates = HashMap::<u32, Option<[f64; 3]>>::new();
    let mut derived_plane_targets = HashSet::new();
    let mut exact_plane_targets = HashSet::new();
    for (face, record) in records.iter().enumerate() {
        let crate::families::standard::records::StandardSurfaceRecord::Analytic(prefix) = record
        else {
            continue;
        };
        if prefix.kind != 0x32 {
            continue;
        }
        let frame_normal = face_frame_vectors.get(face).copied().flatten();
        let normal = frame_normal
            .or_else(|| {
                standard_plane_normal_from_adjacent_circle_carriers(
                    &curve_supports,
                    &curved_surfaces,
                    face,
                )
            })
            .or_else(|| standard_plane_normal_from_circle_centers(&curve_supports, face));
        let Some(normal) = normal else {
            continue;
        };
        if frame_normal.is_none() {
            derived_plane_targets.insert(prefix.target);
        } else {
            exact_plane_targets.insert(prefix.target);
        }
        plane_normal_candidates
            .entry(prefix.target)
            .and_modify(|stored| {
                if stored.is_some_and(|stored| stored != normal) {
                    *stored = None;
                }
            })
            .or_insert(Some(normal));
    }
    let plane_normals = plane_normal_candidates
        .into_iter()
        .filter_map(|(target, normal)| Some((target, normal?)))
        .collect::<HashMap<_, _>>();
    let planes: HashMap<u32, crate::families::standard::records::PlaneParams> =
        crate::families::standard::records::plane_params(brep, &plane_normals)
            .into_iter()
            .map(|plane| (plane.target, plane))
            .collect();

    let mut surfaces = Vec::new();
    let mut surface_annotations = Vec::new();
    let mut face_bindings = Vec::new();
    let mut procedural_surface_plans = Vec::new();
    let mut decoded_plane_targets = HashSet::new();
    let mut plane_faces = 0usize;
    let mut typed = TypedCounts::default();
    for (i, record) in records.iter().enumerate() {
        let crate::families::standard::records::StandardSurfaceRecord::Analytic(prefix) = record
        else {
            let crate::families::standard::records::StandardSurfaceRecord::Freeform {
                pos,
                tag,
                forward,
                ..
            } = record
            else {
                unreachable!()
            };
            let id = SurfaceId(format!("catia:standard:surf#{i}"));
            let geometry = freeform_geometries
                .get(tag)
                .cloned()
                .unwrap_or(SurfaceGeometry::Unknown { record: None });
            face_bindings.push((id.clone(), *forward, *pos));
            surface_annotations.push((
                id.clone(),
                *pos,
                None,
                if freeform_procedural_surfaces.contains_key(tag) {
                    Exactness::ByteExact
                } else if matches!(geometry, SurfaceGeometry::Unknown { .. }) {
                    Exactness::Unknown
                } else {
                    Exactness::ByteExact
                },
            ));
            surfaces.push(Surface {
                id: id.clone(),
                geometry,
                source_object: Some(cgm_source("carrier", *tag)),
            });
            if let Some(procedure) = freeform_procedural_surfaces.get(tag).cloned() {
                procedural_surface_plans.push((i, id, *tag, procedure));
            }
            continue;
        };
        // A bridged plane parameter record contains the same `00 33 32`
        // marker as its SurfacicReps carrier.  One carrier exists per tag.
        if prefix.kind == 0x32 && !decoded_plane_targets.insert(prefix.target) {
            continue;
        }
        let decoded = if prefix.kind == 0x32 {
            planes
                .get(&prefix.target)
                .and_then(crate::families::standard::records::decode_plane)
        } else {
            curved_surfaces[i].clone()
        };
        match decoded {
            Some(geom) => {
                typed.record(&geom);
                let id = SurfaceId(format!("catia:standard:surf#{i}"));
                if let Some(forward) = crate::families::standard::records::face_sense(brep, prefix)
                {
                    face_bindings.push((id.clone(), forward, prefix.pos));
                }
                surface_annotations.push((
                    id.clone(),
                    prefix.pos,
                    Some(prefix.kind),
                    if derived_plane_targets.contains(&prefix.target)
                        && !exact_plane_targets.contains(&prefix.target)
                    {
                        Exactness::Derived
                    } else {
                        Exactness::ByteExact
                    },
                ));
                surfaces.push(Surface {
                    id,
                    geometry: geom,
                    source_object: Some(cgm_source("carrier", prefix.target)),
                });
            }
            None => {
                if prefix.kind == 0x32 {
                    plane_faces += 1;
                }
                let id = SurfaceId(format!("catia:standard:surf#{i}"));
                if let Some(forward) = crate::families::standard::records::face_sense(brep, prefix)
                {
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
                    source_object: Some(cgm_source("carrier", prefix.target)),
                });
            }
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
    let mut procedural_supports = HashMap::<u32, SurfaceId>::new();
    let mut extrusion_definitions = HashMap::<u32, ProceduralSurfaceDefinition>::new();
    for (index, surface, tag, procedure) in procedural_surface_plans {
        let procedural_id = ProceduralSurfaceId(format!("catia:standard:procedural-surf#{index}"));
        let (source, carrier, definition, exactness) = match procedure {
            StandardSurfaceProcedure::RollingBall {
                carrier_object_id,
                definition,
            } => (
                "object_stream_a8_03_32",
                carrier_object_id,
                definition,
                Exactness::ByteExact,
            ),
            StandardSurfaceProcedure::Offset {
                carrier_object_id,
                support_object_id,
                support,
                distance,
            } => {
                let support_id = match support {
                    crate::families::b5::transfer::ResolvedOffsetSupport::Geometry(support) => {
                        procedural_supports
                            .entry(support_object_id)
                            .or_insert_with(|| {
                                let id = SurfaceId(format!(
                                    "catia:standard:procedural-support#{support_object_id}"
                                ));
                                annotate(
                                    &mut annotations,
                                    &id,
                                    "object_stream_b5_03",
                                    0,
                                    format!("surface:{support_object_id:08x}"),
                                    Exactness::ByteExact,
                                );
                                surfaces.push(Surface {
                                    id: id.clone(),
                                    geometry: support,
                                    source_object: Some(cgm_source("surface", support_object_id)),
                                });
                                id
                            })
                            .clone()
                    }
                    crate::families::b5::transfer::ResolvedOffsetSupport::Extrusion(extrusion) => {
                        let support_id = SurfaceId(format!(
                            "catia:standard:procedural-support#{support_object_id}"
                        ));
                        annotate(
                            &mut annotations,
                            &support_id,
                            "object_stream_b5_03_2c",
                            0,
                            format!("surface:{support_object_id:08x}"),
                            Exactness::Derived,
                        );
                        surfaces.push(Surface {
                            id: support_id.clone(),
                            geometry: SurfaceGeometry::Unknown { record: None },
                            source_object: Some(cgm_source("surface", support_object_id)),
                        });
                        let definition = emit_standard_extrusion_definition(
                            &mut ir,
                            &mut annotations,
                            &mut surfaces,
                            &mut procedural_supports,
                            &mut extrusion_definitions,
                            *extrusion,
                        );
                        ir.model.procedural_surfaces.push(ProceduralSurface {
                            id: ProceduralSurfaceId(format!(
                                "catia:standard:procedural-support-definition#{support_object_id}"
                            )),
                            surface: support_id.clone(),
                            definition,
                            cache_fit_tolerance: None,
                            record_bounds: None,
                        });
                        procedural_supports.insert(support_object_id, support_id.clone());
                        support_id
                    }
                };
                annotations.derived(&procedural_id, "definition.u_sense");
                annotations.derived(&procedural_id, "definition.v_sense");
                (
                    "object_stream_b5_03_30",
                    carrier_object_id,
                    ProceduralSurfaceDefinition::Offset {
                        support: support_id,
                        distance,
                        u_sense: Some(0),
                        v_sense: Some(0),
                        extension_flags: Vec::new(),
                        revision_form: None,
                    },
                    Exactness::Derived,
                )
            }
            StandardSurfaceProcedure::Extrusion(extrusion) => {
                let carrier = extrusion.surface_object_id;
                let definition = emit_standard_extrusion_definition(
                    &mut ir,
                    &mut annotations,
                    &mut surfaces,
                    &mut procedural_supports,
                    &mut extrusion_definitions,
                    *extrusion,
                );
                (
                    "object_stream_b5_03_2c",
                    carrier,
                    definition,
                    Exactness::Derived,
                )
            }
        };
        annotate(
            &mut annotations,
            &procedural_id,
            source,
            0,
            format!("face_object_id:{tag:08x}:result_carrier:{carrier:08x}"),
            exactness,
        );
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: procedural_id,
            surface,
            definition,
            cache_fit_tolerance: None,
            record_bounds: None,
        });
    }

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
            source_object: vertex_roster
                .as_ref()
                .map(|roster| cgm_source("vertex", roster[i])),
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
    let mut topology_ir = ir.clone();
    let mut topology_annotations = annotations.clone();
    attach_standard_faces(
        &mut topology_ir,
        &mut topology_annotations,
        &face_bindings,
        brep,
    );
    let topology_attached = attach_standard_topology(
        &mut topology_ir,
        &mut topology_annotations,
        &face_bindings,
        &records,
        brep,
        &scan.data,
        &object_evidence.edge_owner_faces,
    ) && neutral_model_is_admissible(&topology_ir, &unknowns);
    if topology_attached {
        ir = topology_ir;
        annotations = topology_annotations;
    } else {
        attach_standard_circles(&mut ir, &mut annotations, &face_bindings, brep);
        attach_standard_lines(&mut ir, &mut annotations, &face_bindings, brep);
        if !ir.model.vertices.is_empty() {
            attach_free_vertices(
                &mut ir,
                &mut annotations,
                "standard",
                "MainDataStream+SurfacicReps",
            );
        }
    }
    append_freeform_surface_pools(&mut ir, &mut annotations, &scan.data);
    link_payload_carriers(&ir, &mut unknowns, &mut annotations);
    let annotations = annotations.build();

    let report = build_geometry_report(
        &ir,
        scan,
        &typed,
        plane_faces,
        analytic_record_count,
        unresolved_freeform_record_count,
        topology_attached,
    );
    Some(FamilyOutput {
        ir,
        report,
        annotations,
        unknowns,
    })
}

#[derive(Default)]
pub(crate) struct StandardObjectEvidence {
    pub(crate) surface_geometries: HashMap<u32, SurfaceGeometry>,
    pub(crate) procedural_surfaces: HashMap<u32, StandardSurfaceProcedure>,
    pub(crate) edge_owner_faces: HashMap<u32, HashSet<u32>>,
}

#[derive(Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum StandardSurfaceProcedure {
    RollingBall {
        carrier_object_id: u32,
        definition: ProceduralSurfaceDefinition,
    },
    Offset {
        carrier_object_id: u32,
        support_object_id: u32,
        support: crate::families::b5::transfer::ResolvedOffsetSupport,
        distance: f64,
    },
    Extrusion(Box<crate::families::b5::transfer::ResolvedExtrusionSurface>),
}

#[derive(PartialEq)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum StandardSurfaceEvidence {
    Geometry(SurfaceGeometry),
    Procedural(StandardSurfaceProcedure),
}

pub(crate) fn standard_object_evidence(
    scan: &ContainerScan,
    tags: &HashSet<u32>,
) -> StandardObjectEvidence {
    let streams = [scan.outer.as_ref(), scan.inner.as_ref()]
        .into_iter()
        .flatten()
        .flat_map(|directory| {
            directory.descriptors.iter().map(|descriptor| {
                container::reconstruct_logical_stream(&scan.data, descriptor, directory.inner)
            })
        });
    standard_object_evidence_from_streams(streams, tags)
}

pub(crate) fn standard_object_evidence_from_streams(
    streams: impl IntoIterator<Item = Vec<u8>>,
    tags: &HashSet<u32>,
) -> StandardObjectEvidence {
    let mut surface_candidates = HashMap::<u32, Option<StandardSurfaceEvidence>>::new();
    let mut support_candidates =
        HashMap::<u32, Option<crate::families::b5::transfer::ResolvedOffsetSupport>>::new();
    let mut edge_face_candidates = HashMap::<u32, Option<HashSet<u32>>>::new();
    for stream in streams {
        let face_surfaces = crate::families::b5::graph::face_surface_references(&stream);
        let Some(graph) = crate::families::b5::graph::parse(&stream) else {
            continue;
        };
        let mut stream_edge_faces = HashMap::<u32, HashSet<u32>>::new();
        for face in &graph.faces {
            for edge in face
                .loops
                .iter()
                .filter_map(|loop_id| graph.loops.get(loop_id))
                .flat_map(|loop_| loop_.edges.iter().copied())
            {
                stream_edge_faces
                    .entry(edge)
                    .or_default()
                    .insert(face.object_id);
            }
        }
        for (edge, owners) in stream_edge_faces {
            edge_face_candidates
                .entry(edge)
                .and_modify(|stored| {
                    if stored.as_ref().is_some_and(|stored| *stored != owners) {
                        *stored = None;
                    }
                })
                .or_insert(Some(owners));
        }
        for &(face_id, surface_id) in face_surfaces
            .iter()
            .filter(|(face_id, _)| tags.contains(face_id))
        {
            let evidence =
                crate::families::b5::transfer::resolved_surface_geometry(&graph, surface_id)
                    .map(StandardSurfaceEvidence::Geometry)
                    .or_else(|| {
                        crate::families::b5::transfer::resolved_surface_procedural_definition(
                            &graph, surface_id,
                        )
                        .map(|(carrier_object_id, definition)| {
                            StandardSurfaceEvidence::Procedural(
                                StandardSurfaceProcedure::RollingBall {
                                    carrier_object_id,
                                    definition,
                                },
                            )
                        })
                    })
                    .or_else(|| {
                        crate::families::b5::transfer::resolved_offset_surface(&graph, surface_id)
                            .map(|offset| {
                                StandardSurfaceEvidence::Procedural(
                                    StandardSurfaceProcedure::Offset {
                                        carrier_object_id: offset.carrier_object_id,
                                        support_object_id: offset.support_object_id,
                                        support: offset.support,
                                        distance: offset.distance,
                                    },
                                )
                            })
                    })
                    .or_else(|| {
                        crate::families::b5::transfer::resolved_extrusion_surface(
                            &graph, surface_id,
                        )
                        .map(Box::new)
                        .map(StandardSurfaceProcedure::Extrusion)
                        .map(StandardSurfaceEvidence::Procedural)
                    });
            let Some(evidence) = evidence else { continue };
            if let StandardSurfaceEvidence::Procedural(StandardSurfaceProcedure::Offset {
                support_object_id,
                support,
                ..
            }) = &evidence
            {
                support_candidates
                    .entry(*support_object_id)
                    .and_modify(|stored| {
                        if stored.as_ref().is_some_and(|stored| stored != support) {
                            *stored = None;
                        }
                    })
                    .or_insert_with(|| Some(support.clone()));
            }
            if let StandardSurfaceEvidence::Procedural(StandardSurfaceProcedure::Extrusion(
                extrusion,
            )) = &evidence
            {
                for side in &extrusion.supports {
                    support_candidates
                        .entry(side.surface_object_id)
                        .and_modify(|stored| {
                            if stored.as_ref().is_some_and(|stored| {
                                stored
                                    != &crate::families::b5::transfer::ResolvedOffsetSupport::Geometry(
                                        side.surface.clone(),
                                    )
                            }) {
                                *stored = None;
                            }
                        })
                        .or_insert_with(|| {
                            Some(crate::families::b5::transfer::ResolvedOffsetSupport::Geometry(
                                side.surface.clone(),
                            ))
                        });
                }
            }
            surface_candidates
                .entry(face_id)
                .and_modify(|stored| {
                    if stored.as_ref().is_some_and(|stored| *stored != evidence) {
                        *stored = None;
                    }
                })
                .or_insert(Some(evidence));
        }
    }
    StandardObjectEvidence {
        surface_geometries: surface_candidates
            .iter()
            .filter_map(|(&tag, evidence)| match evidence.as_ref()? {
                StandardSurfaceEvidence::Geometry(geometry) => Some((tag, geometry.clone())),
                StandardSurfaceEvidence::Procedural(_) => None,
            })
            .collect(),
        procedural_surfaces: surface_candidates
            .into_iter()
            .filter_map(|(tag, evidence)| match evidence? {
                StandardSurfaceEvidence::Procedural(procedure) => {
                    let valid = match &procedure {
                        StandardSurfaceProcedure::Offset {
                            support_object_id,
                            support,
                            ..
                        } => {
                            support_candidates
                                .get(support_object_id)
                                .and_then(Option::as_ref)
                                == Some(support)
                        }
                        StandardSurfaceProcedure::RollingBall { .. } => true,
                        StandardSurfaceProcedure::Extrusion(extrusion) => {
                            extrusion.supports.iter().all(|side| {
                                support_candidates
                                    .get(&side.surface_object_id)
                                    .and_then(Option::as_ref)
                                    == Some(&crate::families::b5::transfer::ResolvedOffsetSupport::Geometry(
                                        side.surface.clone(),
                                    ))
                            })
                        }
                    };
                    valid.then_some((tag, procedure))
                }
                StandardSurfaceEvidence::Geometry(_) => None,
            })
            .collect(),
        edge_owner_faces: edge_face_candidates
            .into_iter()
            .filter_map(|(edge, owners)| Some((edge, owners?)))
            .collect(),
    }
}

/// Attach standard analytic carriers to faces only when every FBB face has a
/// decoded carrier and its stored sense byte.
pub(crate) fn attach_standard_faces(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    bindings: &[(SurfaceId, bool, usize)],
    brep: &[u8],
) {
    let face_count = fbb::standard_face_count(brep).unwrap_or_default();
    if face_count == 0 || face_count != bindings.len() {
        return;
    }
    let body_id = BodyId("catia:standard:body#0".to_string());
    let region_id = RegionId("catia:standard:region#0-0".to_string());
    let shell_id = ShellId("catia:standard:shell#0-0".to_string());
    let mut face_ids = Vec::with_capacity(face_count);
    for (face_index, (surface, forward, offset)) in bindings.iter().enumerate() {
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
    }
    annotate(
        annotations,
        &body_id,
        "MainDataStream+SurfacicReps",
        0,
        "standard_body",
        Exactness::Inferred,
    );
    annotations
        .derived(&body_id, "kind")
        .derived(&body_id, "regions");
    ir.model.bodies.push(Body {
        id: body_id.clone(),
        kind: BodyKind::Sheet,
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
        "MainDataStream+SurfacicReps",
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
        faces: face_ids,
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
}

pub(crate) fn partition_standard_face_components(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    components: &[Vec<usize>],
) -> bool {
    if components.is_empty()
        || components.iter().any(Vec::is_empty)
        || components.iter().flatten().count() != ir.model.faces.len()
    {
        return false;
    }
    let body_id = BodyId("catia:standard:body#0".to_string());
    let Some(body) = ir.model.bodies.iter_mut().find(|body| body.id == body_id) else {
        return false;
    };
    let region_ids: Vec<RegionId> = (0..components.len())
        .map(|component| RegionId(format!("catia:standard:region#0-{component}")))
        .collect();
    body.regions.clone_from(&region_ids);
    annotations.derived(&body_id, "regions");

    for (component, faces) in components.iter().enumerate() {
        let region_id = region_ids[component].clone();
        let shell_id = ShellId(format!("catia:standard:shell#0-{component}"));
        let face_ids: Vec<FaceId> = faces
            .iter()
            .map(|face| FaceId(format!("catia:standard:face#{face}")))
            .collect();
        for &face in faces {
            let Some(face) = ir.model.faces.get_mut(face) else {
                return false;
            };
            face.shell = shell_id.clone();
            annotations.derived(&face.id, "shell");
        }
        if component == 0 {
            let Some(region) = ir
                .model
                .regions
                .iter_mut()
                .find(|region| region.id == region_id)
            else {
                return false;
            };
            region.shells = vec![shell_id.clone()];
            let Some(shell) = ir
                .model
                .shells
                .iter_mut()
                .find(|shell| shell.id == shell_id)
            else {
                return false;
            };
            shell.faces = face_ids;
            continue;
        }
        for (id, tag) in [
            (&region_id.0, "derived_region"),
            (&shell_id.0, "derived_shell"),
        ] {
            annotate(
                annotations,
                id,
                "MainDataStream+SurfacicReps",
                0,
                tag,
                Exactness::Inferred,
            );
        }
        annotations
            .derived(&region_id, "body")
            .derived(&region_id, "shells");
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: vec![shell_id.clone()],
        });
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
    true
}

pub(crate) fn apply_standard_native_edge_faces(
    edge_faces: &mut [[usize; 2]],
    supports: &[crate::families::standard::records::StandardCurveSupport],
    records: &[crate::families::standard::records::StandardSurfaceRecord],
    native_edge_faces: &HashMap<u32, HashSet<u32>>,
) {
    if edge_faces.len() != supports.len() {
        return;
    }
    let mut face_by_carrier = HashMap::<u32, Option<usize>>::new();
    for (face, record) in records.iter().enumerate() {
        let carrier = match record {
            crate::families::standard::records::StandardSurfaceRecord::Analytic(prefix) => {
                prefix.target
            }
            crate::families::standard::records::StandardSurfaceRecord::Freeform { tag, .. } => *tag,
        };
        face_by_carrier
            .entry(carrier)
            .and_modify(|stored| *stored = None)
            .or_insert(Some(face));
    }
    for (faces, support) in edge_faces.iter_mut().zip(supports) {
        if faces[0] != faces[1] {
            continue;
        }
        let Some(owner_ids) = native_edge_faces.get(&support.tag) else {
            continue;
        };
        let candidates = owner_ids
            .iter()
            .filter_map(|owner| face_by_carrier.get(owner).copied().flatten())
            .filter(|face| *face != faces[0])
            .collect::<HashSet<_>>();
        if let Some(&face) = candidates.iter().next().filter(|_| candidates.len() == 1) {
            faces[1] = face;
        }
    }
}

pub(crate) fn attach_standard_topology(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    bindings: &[(SurfaceId, bool, usize)],
    records: &[crate::families::standard::records::StandardSurfaceRecord],
    brep: &[u8],
    source: &[u8],
    native_edge_faces: &HashMap<u32, HashSet<u32>>,
) -> bool {
    let face_count = ir.model.faces.len();
    let mut supports =
        crate::families::standard::records::standard_curve_supports(brep, face_count);
    if supports.is_empty() {
        return false;
    }
    let serialized_edge_faces = supports
        .iter()
        .map(|support| support.faces)
        .collect::<Vec<_>>();
    let Some(mut edge_faces) =
        missing_edge::resolve_standard_edge_faces(brep, &serialized_edge_faces)
    else {
        return false;
    };
    apply_standard_native_edge_faces(&mut edge_faces, &supports, records, native_edge_faces);
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
    let face_bounds = (records.len() == face_count).then(|| {
        records
            .iter()
            .map(|record| match record {
                crate::families::standard::records::StandardSurfaceRecord::Freeform {
                    bounds,
                    ..
                } => Some(*bounds),
                crate::families::standard::records::StandardSurfaceRecord::Analytic(_) => None,
            })
            .collect::<Vec<_>>()
    });
    let mut endpoint_candidates = Vec::with_capacity(supports.len());
    let mut incidence_candidates = HashMap::<[usize; 2], Vec<usize>>::new();
    let mut face_incidence_candidates = HashMap::<usize, Vec<usize>>::new();
    for support in &supports {
        let Some(surface0) = face_surface(ir, bindings, &surface_indices, support.faces[0]) else {
            return false;
        };
        let Some(surface1) = face_surface(ir, bindings, &surface_indices, support.faces[1]) else {
            return false;
        };
        let candidates = match &support.geometry {
            crate::families::standard::records::StandardCurveGeometry::Circle {
                center,
                radius,
            } => standard_circle_endpoint_candidates(
                &ir.model.points,
                *center,
                *radius,
                Some((&surface0.geometry, &surface1.geometry)),
            ),
            crate::families::standard::records::StandardCurveGeometry::Line
            | crate::families::standard::records::StandardCurveGeometry::Bspline => {
                let mut faces = support.faces;
                faces.sort_unstable();
                for (face, surface) in [
                    (support.faces[0], &surface0.geometry),
                    (support.faces[1], &surface1.geometry),
                ] {
                    face_incidence_candidates.entry(face).or_insert_with(|| {
                        ir.model
                            .points
                            .iter()
                            .enumerate()
                            .filter_map(|(index, point)| {
                                point_on_standard_face(
                                    point.position,
                                    surface,
                                    face_bounds.as_ref().and_then(|bounds| bounds[face]),
                                )
                                .then_some(index)
                            })
                            .collect()
                    });
                }
                incidence_candidates
                    .entry(faces)
                    .or_insert_with(|| {
                        let right = face_incidence_candidates[&faces[1]]
                            .iter()
                            .copied()
                            .collect::<HashSet<_>>();
                        face_incidence_candidates[&faces[0]]
                            .iter()
                            .copied()
                            .filter(|point| right.contains(point))
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
                            crate::families::standard::records::StandardCurveGeometry::Circle {
                                center: left_center,
                                radius: left_radius,
                            },
                            crate::families::standard::records::StandardCurveGeometry::Circle {
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
                            crate::families::standard::records::StandardCurveGeometry::Line,
                            crate::families::standard::records::StandardCurveGeometry::Line,
                        ) => true,
                        _ => false,
                    }
            })
            .map_or(edge, |candidate| edge_classes[candidate]);
        edge_classes.push(class);
    }
    let native_edges = crate::families::b5::graph::edge_vertex_references(source);
    let graph_endpoint_pairs = standard_native_graph_endpoint_pairs(
        source,
        &supports,
        &native_edges,
        &ir.model.points,
        &endpoint_candidates,
    );
    let native_port_options = supports
        .iter()
        .map(|support| native_edges.get(&support.tag).copied())
        .collect::<Vec<_>>();
    let native_ports = native_port_options
        .iter()
        .copied()
        .collect::<Option<Vec<_>>>();
    let vertex_roster =
        crate::families::standard::records::standard_vertex_roster(source, ir.model.points.len());
    let roster_endpoint_pairs = vertex_roster.as_ref().map(|roster| {
        let point_by_identity = roster
            .iter()
            .copied()
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
    let allocation_endpoint_pairs = vertex_roster
        .as_ref()
        .map(|roster| standard_successor_endpoint_pairs(&supports, roster));
    let Ok(native_endpoint_evidence) = merge_native_endpoint_evidence(
        graph_endpoint_pairs.as_deref(),
        roster_endpoint_pairs.as_deref(),
    )
    .and_then(|evidence| {
        merge_native_endpoint_evidence(evidence.as_deref(), allocation_endpoint_pairs.as_deref())
    }) else {
        return false;
    };
    if let Some(pairs) = &native_endpoint_evidence {
        include_native_endpoint_pairs(&mut endpoint_candidates, pairs);
    }
    let mut endpoint_options = resolve_standard_endpoint_pairs(
        ir,
        bindings,
        &surface_indices,
        &supports,
        &endpoint_candidates,
    );
    if let (Some(options), Some(pairs)) = (&mut endpoint_options, &native_endpoint_evidence) {
        for (options, pair) in options.iter_mut().zip(pairs) {
            if let Some(pair) = pair {
                *options = vec![*pair];
            }
        }
    }
    let graph_propagated_endpoint_pairs = match native_endpoint_evidence.as_ref() {
        Some(pairs) => {
            let Some(propagated) =
                missing_edge::propagate_partial_edge_port_points(&native_port_options, pairs)
            else {
                return false;
            };
            Some(propagated)
        }
        None => None,
    };
    if let (Some(options), Some(pairs)) = (&mut endpoint_options, &graph_propagated_endpoint_pairs)
    {
        for (options, pair) in options.iter_mut().zip(pairs) {
            if let Some(pair) = pair {
                *options = vec![*pair];
            }
        }
    }
    if let Some(pairs) = &graph_propagated_endpoint_pairs {
        include_native_endpoint_pairs(&mut endpoint_candidates, pairs);
    }
    if let Some(options) = &mut endpoint_options {
        let mut allowed_faces = supports
            .iter()
            .enumerate()
            .map(|(edge, support)| {
                if support.faces[0] != support.faces[1] {
                    return Vec::new();
                }
                (0..face_count)
                    .filter(|face| *face != support.faces[0])
                    .filter(|face| {
                        let Some(surface) = face_surface(ir, bindings, &surface_indices, *face)
                        else {
                            return false;
                        };
                        options[edge].iter().any(|pair| {
                            pair.iter().all(|point| {
                                ir.model.points.get(*point).is_some_and(|point| {
                                    point_on_standard_face(
                                        point.position,
                                        &surface.geometry,
                                        face_bounds.as_ref().and_then(|bounds| bounds[*face]),
                                    )
                                })
                            })
                        })
                    })
                    .collect()
            })
            .collect::<Vec<_>>();
        for edge in 0..allowed_faces.len() {
            allowed_faces[edge].retain(|face| {
                let mut trial = edge_faces.clone();
                trial[edge][1] = *face;
                missing_edge::face_endpoint_candidates_close(&trial, options, *face)
            });
        }
        if let Some(completed) =
            missing_edge::resolve_standard_duplicate_edge_faces(brep, &edge_faces, &allowed_faces)
        {
            edge_faces = completed;
            for (edge, (support, faces)) in supports.iter_mut().zip(&edge_faces).enumerate() {
                if support.faces == *faces {
                    continue;
                }
                support.faces = *faces;
                let Some(surface) = face_surface(ir, bindings, &surface_indices, faces[1]) else {
                    return false;
                };
                options[edge].retain(|pair| {
                    pair.iter().all(|point| {
                        ir.model.points.get(*point).is_some_and(|point| {
                            point_on_standard_face(
                                point.position,
                                &surface.geometry,
                                face_bounds.as_ref().and_then(|bounds| bounds[faces[1]]),
                            )
                        })
                    })
                });
                if options[edge].is_empty() {
                    return false;
                }
            }
        }
    }
    if let Some(options) = &mut endpoint_options {
        for (edge, pairs) in options.iter_mut().enumerate() {
            let support = &supports[edge];
            if matches!(
                support.geometry,
                crate::families::standard::records::StandardCurveGeometry::Bspline
            ) {
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
                        crate::families::standard::records::standard_face_witness(
                            brep,
                            bindings[face].2,
                        ),
                        None,
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
                missing_edge::standard_mesh_placement_endpoint_pairs(brep, &edge_faces, &seeds)
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
                            domain.iter().any(|candidate| {
                                missing_edge::same_unordered_pair(*pair, *candidate)
                            })
                        });
                    }
                    changed |= options[edge] != previous;
                }
            }
            if let Some(boundary_domains) = options
                .iter()
                .all(|domain| !domain.is_empty())
                .then(|| {
                    missing_edge::standard_mesh_prune_endpoint_candidates(
                        brep,
                        &edge_faces,
                        options,
                    )
                })
                .flatten()
            {
                for (edge, domain) in boundary_domains.into_iter().enumerate() {
                    let previous = options[edge].clone();
                    if options[edge].is_empty() {
                        options[edge] = domain;
                    } else {
                        options[edge].retain(|pair| {
                            domain.iter().any(|candidate| {
                                missing_edge::same_unordered_pair(*pair, *candidate)
                            })
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
    let graph_propagated_pairs = graph_propagated_endpoint_pairs
        .as_ref()
        .and_then(|pairs| pairs.iter().copied().collect::<Option<Vec<_>>>());
    let native_endpoint_pairs = graph_propagated_pairs.or_else(|| {
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
            let propagated = missing_edge::propagate_edge_port_points(ports, &seeds)?;
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
            .then(|| missing_edge::bind_edge_port_candidates(ports, options))?
        })
    });
    let propagated_endpoint_pairs = endpoint_options
        .as_ref()
        .zip(missing_edge::standard_edge_port_identities(brep))
        .and_then(|(options, ports)| {
            let pairs = options
                .iter()
                .map(|pairs| {
                    <[[usize; 2]; 1]>::try_from(pairs.as_slice())
                        .ok()
                        .map(|pair| pair[0])
                })
                .collect::<Vec<_>>();
            missing_edge::propagate_edge_port_points(&ports, &pairs)
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
        .zip(missing_edge::standard_mesh_edge_ports(brep))
        .and_then(|(options, ports)| {
            let pairs = options
                .iter()
                .map(|pairs| {
                    <[[usize; 2]; 1]>::try_from(pairs.as_slice())
                        .ok()
                        .map(|pair| pair[0])
                })
                .collect::<Vec<_>>();
            missing_edge::propagate_edge_port_points(&ports, &pairs)
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
        missing_edge::standard_mesh_edge_ports(brep),
    ) {
        if let Some(pruned) = fbb::prune_edge_candidates_by_port_domains(&ports, options) {
            *options = pruned;
        }
    }
    if let Some(options) = &mut constrained_endpoint_options {
        bind_ordered_standard_curve_branches(&supports, options);
    }
    let resolved_endpoint_pairs = propagated_endpoint_pairs
        .and_then(|pairs| pairs.into_iter().collect::<Option<Vec<[usize; 2]>>>());
    if let Some(pairs) = &resolved_endpoint_pairs {
        let pairs = pairs.iter().copied().map(Some).collect::<Vec<_>>();
        include_native_endpoint_pairs(&mut endpoint_candidates, &pairs);
    }
    let mesh_bound = fbb::parse_standard(brep)
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
                    missing_edge::bind_edge_port_candidates(
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
            crate::families::standard::records::StandardCurveGeometry::Circle { .. } => {
                <[usize; 2]>::try_from(candidates.as_slice()).ok()
            }
            crate::families::standard::records::StandardCurveGeometry::Line
            | crate::families::standard::records::StandardCurveGeometry::Bspline => None,
        })
        .collect();
    let circle_constraint_edges = supports
        .iter()
        .enumerate()
        .map(|(edge, support)| {
            matches!(
                support.geometry,
                crate::families::standard::records::StandardCurveGeometry::Circle { .. }
            ) && constrained_endpoint_options
                .as_ref()
                .is_some_and(|options| options[edge].len() > 1)
        })
        .collect::<Vec<_>>();
    let (mut topology, point_assignment) = if let Some(bound) = mesh_bound {
        bound
    } else if let Some(topology) = native_endpoint_pairs.as_ref().and_then(|pairs| {
        fbb::parse_standard_endpoints_with_edge_classes(
            brep,
            &edge_faces,
            pairs,
            Some(&edge_classes),
        )
    }) {
        let point_assignment = (0..ir.model.points.len()).collect();
        (topology, point_assignment)
    } else if let Some(bound) = constrained_endpoint_options.as_ref().and_then(|options| {
        mesh_quotient::parse_standard_mesh_candidates(
            brep,
            &edge_faces,
            options,
            &circle_constraint_edges,
            |pairs| {
                standard_circle_pair_solution_is_simple(
                    ir,
                    bindings,
                    &surface_indices,
                    brep,
                    &supports,
                    options,
                    pairs,
                )
            },
        )
    }) {
        bound
    } else if let Some(topology) = constrained_endpoint_options.as_ref().and_then(|options| {
        missing_edge::standard_mesh_edge_ports(brep)
            .and_then(|ports| {
                fbb::parse_standard_port_endpoint_candidates(brep, &edge_faces, options, &ports)
            })
            .or_else(|| fbb::parse_standard_endpoint_candidates(brep, &edge_faces, options))
    }) {
        let point_assignment = (0..ir.model.points.len()).collect();
        (topology, point_assignment)
    } else if let Some(topology) = fbb::parse_standard_motif(brep, &edge_faces, &circle_anchors) {
        let point_assignment = (0..ir.model.points.len()).collect();
        (topology, point_assignment)
    } else {
        return false;
    };
    let Some(edge_vertices) = validate_standard_topology(
        ir,
        annotations,
        &mut topology,
        &point_assignment,
        &supports,
        &endpoint_candidates,
        face_count,
    ) else {
        return false;
    };
    emit_standard_topology(
        ir,
        annotations,
        bindings,
        brep,
        &surface_indices,
        &supports,
        &edge_vertices,
        &point_assignment,
        &topology,
    );
    true
}

/// Validates the solved topology against the decoded model, applies body kinds
/// and face partitioning, and returns the per-edge logical vertex pairs.
#[allow(clippy::question_mark)]
fn validate_standard_topology(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    topology: &mut crate::families::standard::topology::StandardTopology,
    point_assignment: &[usize],
    supports: &[crate::families::standard::records::StandardCurveSupport],
    endpoint_candidates: &[Vec<usize>],
    face_count: usize,
) -> Option<Vec<[usize; 2]>> {
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
        return None;
    }
    let face_groups = vec![topology.face_count()];
    if topology.orient_solid_body_cycles(&face_groups).is_none() {
        return None;
    }
    let Some(body_kinds) = topology.body_kinds(&face_groups) else {
        return None;
    };
    let Some(edge_vertices) = topology.edge_vertices() else {
        return None;
    };
    if edge_vertices.iter().enumerate().any(|(edge, vertices)| {
        let start = point_assignment[vertices[0]];
        let end = point_assignment[vertices[1]];
        !endpoint_candidates[edge].is_empty()
            && (!endpoint_candidates[edge].contains(&start)
                || !endpoint_candidates[edge].contains(&end))
    }) {
        return None;
    }
    let Some(body_arena_indices) = (0..body_kinds.len())
        .map(|body_index| {
            let id = BodyId(format!("catia:standard:body#{body_index}"));
            ir.model.bodies.iter().position(|body| body.id == id)
        })
        .collect::<Option<Vec<_>>>()
    else {
        return None;
    };
    for (&arena_index, &kind) in body_arena_indices.iter().zip(&body_kinds) {
        ir.model.bodies[arena_index].kind = kind;
    }
    if !partition_standard_face_components(ir, annotations, &topology.face_components()) {
        return None;
    }
    Some(edge_vertices)
}

/// Emits the edge, loop, coedge, and pcurve IR layers for the solved topology.
#[allow(clippy::too_many_arguments)]
fn emit_standard_topology(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    bindings: &[(SurfaceId, bool, usize)],
    brep: &[u8],
    surface_indices: &HashMap<SurfaceId, usize>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
    edge_vertices: &[[usize; 2]],
    point_assignment: &[usize],
    topology: &crate::families::standard::topology::StandardTopology,
) {
    for (edge_index, (support, logical_vertices)) in supports.iter().zip(edge_vertices).enumerate()
    {
        let start_point = point_assignment[logical_vertices[0]];
        let end_point = point_assignment[logical_vertices[1]];
        let (curve, param_range) = build_standard_edge_curve(
            ir,
            annotations,
            bindings,
            surface_indices,
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

    let curve_indices = ir
        .model
        .curves
        .iter()
        .enumerate()
        .map(|(index, curve)| (curve.id.clone(), index))
        .collect::<HashMap<_, _>>();
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
                let edge_curve = ir.model.edges[edge_use.edge_row]
                    .curve
                    .as_ref()
                    .and_then(|id| curve_indices.get(id))
                    .map(|index| &ir.model.curves[*index].geometry);
                let pcurve_id = standard_pcurve_geometry(
                    &ir.model.surfaces[surface_indices[&bindings[face_index].0]].geometry,
                    support,
                    start,
                    end,
                    crate::families::standard::records::standard_face_witness(
                        brep,
                        bindings[face_index].2,
                    ),
                    edge_curve,
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
                    annotations.derived(&id, "pcurves");
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
                    pcurves: pcurve_id
                        .map(|pcurve| cadmpeg_ir::topology::PcurveUse {
                            pcurve,
                            isoparametric: None,
                            parameter_range: None,
                        })
                        .into_iter()
                        .collect(),
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
}

pub(crate) fn resolve_standard_endpoint_pairs(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    supports: &[crate::families::standard::records::StandardCurveSupport],
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
                crate::families::standard::records::StandardCurveGeometry::Circle { .. }
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
        if !resolved[edge].is_empty() {
            continue;
        }
        let mut faces = support.faces;
        faces.sort_unstable();
        let line_like = match support.geometry {
            crate::families::standard::records::StandardCurveGeometry::Line => true,
            crate::families::standard::records::StandardCurveGeometry::Bspline => {
                let surfaces = faces.map(|face| {
                    face_surface(ir, bindings, surface_indices, face)
                        .map(|surface| &surface.geometry)
                });
                matches!(surfaces, [Some(left), Some(right)] if intersection_line_direction(left, right).is_some())
            }
            crate::families::standard::records::StandardCurveGeometry::Circle { .. } => false,
        };
        if line_like {
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
                let segment_norm = segment.dot(segment).sqrt();
                let midpoint = Point3::new(
                    (start_point.x + end_point.x) * 0.5,
                    (start_point.y + end_point.y) * 0.5,
                    (start_point.z + end_point.z) * 0.5,
                );
                let follows_direction = direction.is_none_or(|direction| {
                    let direction_norm = direction.dot(direction).sqrt();
                    direction_norm > f64::EPSILON
                        && segment
                            .cross(direction)
                            .dot(segment.cross(direction))
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

/// Bind same-incidence curve branches when their surviving endpoint relation
/// and serialized cardinality establish corresponding allocation ranks.
pub(crate) fn bind_ordered_standard_curve_branches(
    supports: &[crate::families::standard::records::StandardCurveSupport],
    candidates: &mut [Vec<[usize; 2]>],
) {
    if supports.len() != candidates.len() {
        return;
    }
    let normalized = candidates
        .iter()
        .map(|pairs| {
            let mut pairs = pairs
                .iter()
                .copied()
                .map(|mut pair| {
                    pair.sort_unstable();
                    pair
                })
                .collect::<Vec<_>>();
            pairs.sort_unstable();
            pairs.dedup();
            pairs
        })
        .collect::<Vec<_>>();
    let mut grouped = vec![false; supports.len()];
    for first in 0..supports.len() {
        if grouped[first]
            || !matches!(
                supports[first].geometry,
                crate::families::standard::records::StandardCurveGeometry::Circle { .. }
            )
            || normalized[first].len() < 2
        {
            continue;
        }
        let mut faces = supports[first].faces;
        faces.sort_unstable();
        let edges = (first..supports.len())
            .filter(|edge| {
                let mut candidate_faces = supports[*edge].faces;
                candidate_faces.sort_unstable();
                let same_circle = match (&supports[*edge].geometry, &supports[first].geometry) {
                    (
                        crate::families::standard::records::StandardCurveGeometry::Circle {
                            center: candidate_center,
                            radius: candidate_radius,
                        },
                        crate::families::standard::records::StandardCurveGeometry::Circle {
                            center,
                            radius,
                        },
                    ) => candidate_center == center && candidate_radius == radius,
                    _ => false,
                };
                !grouped[*edge]
                    && same_circle
                    && candidate_faces == faces
                    && normalized[*edge] == normalized[first]
            })
            .collect::<Vec<_>>();
        if edges.len() != normalized[first].len() {
            continue;
        }
        for (pair, edge) in normalized[first].iter().copied().zip(edges) {
            candidates[edge] = vec![pair];
            grouped[edge] = true;
        }
    }
    for first in 0..supports.len() {
        if grouped[first]
            || !matches!(
                supports[first].geometry,
                crate::families::standard::records::StandardCurveGeometry::Bspline
            )
            || normalized[first].len() < 4
        {
            continue;
        }
        let mut faces = supports[first].faces;
        faces.sort_unstable();
        let edges = (first..supports.len())
            .filter(|edge| {
                let mut candidate_faces = supports[*edge].faces;
                candidate_faces.sort_unstable();
                !grouped[*edge]
                    && matches!(
                        supports[*edge].geometry,
                        crate::families::standard::records::StandardCurveGeometry::Bspline
                    )
                    && candidate_faces == faces
                    && normalized[*edge] == normalized[first]
            })
            .collect::<Vec<_>>();
        let branch_count = edges.len();
        if branch_count < 2 {
            continue;
        }
        let vertices = normalized[first]
            .iter()
            .flatten()
            .copied()
            .collect::<HashSet<_>>();
        if vertices.len() != branch_count.saturating_mul(2) {
            continue;
        }
        let fixed_relations = normalized
            .iter()
            .enumerate()
            .filter(|(edge, pairs)| {
                !edges.contains(edge)
                    && pairs.len() == 1
                    && supports[*edge]
                        .faces
                        .iter()
                        .any(|face| faces.contains(face))
                    && pairs[0].iter().all(|point| vertices.contains(point))
            })
            .map(|(_, pairs)| pairs[0])
            .collect::<HashSet<_>>();
        let relation = normalized[first]
            .iter()
            .copied()
            .filter(|pair| !fixed_relations.contains(pair))
            .collect::<Vec<_>>();
        if relation.len() != branch_count.saturating_mul(branch_count) {
            continue;
        }
        let mut adjacency = HashMap::<usize, Vec<usize>>::new();
        for &[left, right] in &relation {
            if left == right {
                adjacency.clear();
                break;
            }
            adjacency.entry(left).or_default().push(right);
            adjacency.entry(right).or_default().push(left);
        }
        if adjacency.len() != branch_count.saturating_mul(2)
            || adjacency
                .values()
                .any(|neighbors| neighbors.len() != branch_count)
        {
            continue;
        }
        let Some(&root) = adjacency.keys().min() else {
            continue;
        };
        let mut colors = HashMap::from([(root, false)]);
        let mut stack = vec![root];
        let mut valid = true;
        while let Some(vertex) = stack.pop() {
            let color = colors[&vertex];
            for &neighbor in &adjacency[&vertex] {
                match colors.get(&neighbor) {
                    Some(stored) if *stored == color => valid = false,
                    Some(_) => {}
                    None => {
                        colors.insert(neighbor, !color);
                        stack.push(neighbor);
                    }
                }
            }
        }
        if !valid || colors.len() != adjacency.len() {
            continue;
        }
        let mut sides = [Vec::new(), Vec::new()];
        for (vertex, color) in colors {
            sides[usize::from(color)].push(vertex);
        }
        sides[0].sort_unstable();
        sides[1].sort_unstable();
        if sides.iter().any(|side| side.len() != branch_count) {
            continue;
        }
        let cross_relations = sides[0]
            .iter()
            .flat_map(|left| {
                sides[1].iter().map(move |right| {
                    let mut pair = [*left, *right];
                    pair.sort_unstable();
                    pair
                })
            })
            .collect::<HashSet<_>>();
        if relation.iter().copied().collect::<HashSet<_>>() != cross_relations
            || normalized[first]
                .iter()
                .any(|pair| !cross_relations.contains(pair) && !fixed_relations.contains(pair))
        {
            continue;
        }
        for (rank, edge) in edges.into_iter().enumerate() {
            candidates[edge] = vec![[sides[0][rank], sides[1][rank]]];
            grouped[edge] = true;
        }
    }
}

pub(crate) fn standard_circle_endpoint_candidates(
    points: &[Point],
    center: Point3,
    radius: f64,
    surfaces: Option<(&SurfaceGeometry, &SurfaceGeometry)>,
) -> Vec<usize> {
    points
        .iter()
        .enumerate()
        .filter_map(|(index, point)| {
            let on_circle = (point.position.distance_squared(center).sqrt() - radius).abs() <= 1e-3;
            let incident = surfaces.is_none_or(|(left, right)| {
                point_on_known_surface(point.position, left)
                    && point_on_known_surface(point.position, right)
            });
            (on_circle && incident).then_some(index)
        })
        .collect()
}

/// Resolve standard-row endpoints from native edge identities. Exact local-tag
/// matches take precedence; otherwise an unused native edge may contribute
/// only when its logical point pair is unique inside the row's geometric domain.
pub(crate) fn standard_native_graph_endpoint_pairs(
    source: &[u8],
    supports: &[crate::families::standard::records::StandardCurveSupport],
    native_edges: &BTreeMap<u32, [u32; 2]>,
    points: &[Point],
    endpoint_candidates: &[Vec<usize>],
) -> Option<Vec<Option<[usize; 2]>>> {
    if supports.len() != endpoint_candidates.len() {
        return None;
    }
    let graph = crate::families::b5::graph::parse(source)?;
    let identity_points = unique_native_identity_points(
        &graph.logical_vertex_refs,
        &graph.logical_vertex_points,
        graph.vertex_points.len(),
        &graph.vertex_tolerances,
        points,
    );
    let directly_bound_edges = supports
        .iter()
        .filter_map(|support| {
            native_edges
                .contains_key(&support.tag)
                .then_some(support.tag)
        })
        .collect::<HashSet<_>>();
    let unbound_edge_points = native_edges
        .iter()
        .filter(|(edge, _)| !directly_bound_edges.contains(edge))
        .filter_map(|(_, [start_identity, end_identity])| {
            let mut pair = [
                *identity_points.get(start_identity)?,
                *identity_points.get(end_identity)?,
            ];
            pair.sort_unstable();
            Some(pair)
        })
        .collect::<Vec<_>>();
    Some(
        supports
            .iter()
            .zip(endpoint_candidates)
            .map(|(support, candidates)| {
                if let Some([start_identity, end_identity]) = native_edges.get(&support.tag) {
                    return Some([
                        *identity_points.get(start_identity)?,
                        *identity_points.get(end_identity)?,
                    ]);
                }
                unique_unbound_native_endpoint_pair(candidates, &unbound_edge_points)
            })
            .collect(),
    )
}

/// Return the sole distinct unordered native pair contained in a geometric
/// endpoint domain.
pub(crate) fn unique_unbound_native_endpoint_pair(
    candidates: &[usize],
    native_edge_points: &[[usize; 2]],
) -> Option<[usize; 2]> {
    let mut pairs = native_edge_points
        .iter()
        .copied()
        .filter(|pair| candidates.contains(&pair[0]) && candidates.contains(&pair[1]))
        .collect::<Vec<_>>();
    for pair in &mut pairs {
        pair.sort_unstable();
    }
    pairs.sort_unstable();
    pairs.dedup();
    <[[usize; 2]; 1]>::try_from(pairs).ok().map(|[pair]| pair)
}

pub(crate) fn include_native_endpoint_pairs(
    candidates: &mut [Vec<usize>],
    pairs: &[Option<[usize; 2]>],
) {
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

pub(crate) fn combine_propagated_endpoint_pairs(
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

pub(crate) fn merge_native_endpoint_evidence(
    graph: Option<&[Option<[usize; 2]>]>,
    roster: Option<&[Option<[usize; 2]>]>,
) -> Result<Option<Vec<Option<[usize; 2]>>>, &'static str> {
    match (graph, roster) {
        (Some(graph), Some(roster)) => {
            if graph.len() != roster.len() {
                return Err("native endpoint evidence length mismatch");
            }
            // The roster is the standard BREP's serialized identity-to-point
            // relation. Graph coordinates are reconstructed from independent
            // object records and only supply identities absent from the roster.
            if roster.iter().all(Option::is_some) {
                return Ok(Some(roster.to_vec()));
            }
            graph
                .iter()
                .zip(roster)
                .map(|(graph, roster)| match (graph, roster) {
                    (Some(graph), Some(roster)) if graph != roster => {
                        Err("conflicting native endpoint evidence")
                    }
                    (Some(pair), _) | (_, Some(pair)) => Ok(Some(*pair)),
                    (None, None) => Ok(None),
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some)
        }
        (Some(pairs), None) | (None, Some(pairs)) => Ok(Some(pairs.to_vec())),
        (None, None) => Ok(None),
    }
}

pub(crate) fn standard_successor_endpoint_pairs(
    supports: &[crate::families::standard::records::StandardCurveSupport],
    vertex_roster: &[u32],
) -> Vec<Option<[usize; 2]>> {
    let point_by_identity = vertex_roster
        .iter()
        .copied()
        .enumerate()
        .map(|(point, identity)| (identity, point))
        .collect::<HashMap<_, _>>();
    supports
        .iter()
        .map(|support| {
            Some([
                *point_by_identity.get(&support.tag.checked_add(1)?)?,
                *point_by_identity.get(&support.tag.checked_add(2)?)?,
            ])
        })
        .collect()
}

pub(crate) fn unique_native_identity_points(
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
                    (point
                        .position
                        .distance_squared(Point3::new(coordinate[0], coordinate[1], coordinate[2]))
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

pub(crate) fn intersection_line_direction(
    left: &SurfaceGeometry,
    right: &SurfaceGeometry,
) -> Option<Vector3> {
    const ANGULAR_TOLERANCE: f64 = 1e-9;

    match (left, right) {
        (
            SurfaceGeometry::Plane { normal: left, .. },
            SurfaceGeometry::Plane { normal: right, .. },
        ) => {
            let direction = (*left).cross(*right);
            (direction.dot(direction) > f64::EPSILON).then_some(direction)
        }
        (SurfaceGeometry::Plane { normal, .. }, SurfaceGeometry::Cylinder { axis, .. })
        | (SurfaceGeometry::Cylinder { axis, .. }, SurfaceGeometry::Plane { normal, .. }) => {
            ((*normal).dot(*axis).abs() <= ANGULAR_TOLERANCE).then_some(*axis)
        }
        (
            SurfaceGeometry::Cylinder {
                axis: left_axis, ..
            },
            SurfaceGeometry::Cylinder {
                axis: right_axis, ..
            },
        ) => ((*left_axis).cross(*right_axis).norm() <= ANGULAR_TOLERANCE).then_some(*left_axis),
        _ => None,
    }
}

pub(crate) fn standard_plane_normal_from_circle_centers(
    supports: &[crate::families::standard::records::StandardCurveSupport],
    face: usize,
) -> Option<[f64; 3]> {
    const TOLERANCE: f64 = 1e-5;

    let mut centers = supports
        .iter()
        .filter(|support| support.faces.contains(&face))
        .filter_map(|support| match &support.geometry {
            crate::families::standard::records::StandardCurveGeometry::Circle {
                center, ..
            } => Some(*center),
            crate::families::standard::records::StandardCurveGeometry::Line
            | crate::families::standard::records::StandardCurveGeometry::Bspline => None,
        })
        .collect::<Vec<_>>();
    let mut distinct: Vec<Point3> = Vec::new();
    for center in centers {
        if !distinct
            .iter()
            .any(|stored| (*stored).distance_squared(center) <= TOLERANCE.powi(2))
        {
            distinct.push(center);
        }
    }
    centers = distinct;
    let origin = *centers.first()?;
    let normal = centers[1..]
        .iter()
        .enumerate()
        .flat_map(|(left, &left_center)| {
            centers[left + 2..].iter().map(move |&right_center| {
                left_center
                    .vector_from(origin)
                    .cross(right_center.vector_from(origin))
            })
        })
        .find(|normal| (*normal).norm() > TOLERANCE)?;
    let norm = normal.norm();
    let mut normal = [normal.x / norm, normal.y / norm, normal.z / norm];
    if centers.iter().any(|center| {
        let offset = (*center).vector_from(origin);
        (offset.x * normal[0] + offset.y * normal[1] + offset.z * normal[2]).abs() > TOLERANCE
    }) {
        return None;
    }
    if normal
        .iter()
        .find(|component| component.abs() > TOLERANCE)
        .is_some_and(|component| *component < 0.0)
    {
        normal = normal.map(|component| -component);
    }
    Some(normal)
}

pub(crate) fn standard_plane_normal_from_adjacent_circle_carriers(
    supports: &[crate::families::standard::records::StandardCurveSupport],
    surfaces: &[Option<SurfaceGeometry>],
    face: usize,
) -> Option<[f64; 3]> {
    let mut axes = supports.iter().filter_map(|support| {
        let crate::families::standard::records::StandardCurveGeometry::Circle { center, radius } =
            &support.geometry
        else {
            return None;
        };
        let adjacent = if support.faces[0] == face {
            support.faces[1]
        } else if support.faces[1] == face {
            support.faces[0]
        } else {
            return None;
        };
        circle_axis_from_carrier(*center, *radius, surfaces.get(adjacent)?.as_ref()?)
    });
    let axis = axes.next()?;
    if axes.any(|other| axis.dot(other).abs() < 0.9999) {
        return None;
    }
    let mut normal = [axis.x, axis.y, axis.z];
    if normal
        .iter()
        .find(|component| component.abs() > 1e-5)
        .is_some_and(|component| *component < 0.0)
    {
        normal = normal.map(|component| -component);
    }
    Some(normal)
}

pub(crate) fn face_surface<'a>(
    ir: &'a CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    face: usize,
) -> Option<&'a Surface> {
    let id = &bindings.get(face)?.0;
    ir.model.surfaces.get(*surface_indices.get(id)?)
}

pub(crate) fn point_on_known_surface(point: Point3, surface: &SurfaceGeometry) -> bool {
    matches!(surface, SurfaceGeometry::Unknown { .. }) || point_on_surface(point, surface)
}

pub(crate) fn point_on_standard_face(
    point: Point3,
    surface: &SurfaceGeometry,
    bounds: Option<crate::families::standard::records::FreeformFaceBounds>,
) -> bool {
    const TOLERANCE: f64 = 2e-3;

    if !point_on_known_surface(point, surface) {
        return false;
    }
    bounds.is_none_or(|bounds| {
        let coordinates = [point.x, point.y, point.z];
        let inside_aabb = coordinates.iter().enumerate().all(|(axis, coordinate)| {
            (*coordinate - bounds.aabb_center[axis]).abs()
                <= bounds.aabb_half_extents[axis] + TOLERANCE
        });
        let distance_squared = coordinates
            .iter()
            .enumerate()
            .map(|(axis, coordinate)| (*coordinate - bounds.sphere_center[axis]).powi(2))
            .sum::<f64>();
        inside_aabb && distance_squared.sqrt() <= bounds.sphere_radius + TOLERANCE
    })
}

pub(crate) fn standard_pcurve_geometry(
    surface: &SurfaceGeometry,
    support: &crate::families::standard::records::StandardCurveSupport,
    start: Point3,
    end: Point3,
    witness: Option<Point3>,
    edge_curve: Option<&CurveGeometry>,
) -> Option<(PcurveGeometry, [f64; 2])> {
    if !point_on_surface(start, surface) || !point_on_surface(end, surface) {
        return None;
    }
    let mut uv = [
        analytic_surface_uv(surface, start)?,
        analytic_surface_uv(surface, end)?,
    ];
    if let SurfaceGeometry::Cone {
        origin,
        axis,
        radius,
        half_angle,
        ..
    } = surface
    {
        let tangent = half_angle.tan();
        if tangent.abs() > f64::EPSILON {
            let apex_offset = -*radius / tangent;
            let apex = Point3::new(
                origin.x + apex_offset * axis.x,
                origin.y + apex_offset * axis.y,
                origin.z + apex_offset * axis.z,
            );
            if start.distance_squared(apex) <= 1e-6 {
                uv[0].u = uv[1].u;
            }
            if end.distance_squared(apex) <= 1e-6 {
                uv[1].u = uv[0].u;
            }
        }
    }
    let reference_uv = uv[0];
    unwrap_standard_uv(surface, &mut uv[1], reference_uv);

    if let (
        crate::families::standard::records::StandardCurveGeometry::Circle { center, radius },
        Some(witness),
    ) = (&support.geometry, witness)
    {
        if let Some(end) = witnessed_surface_circle_end(surface, *center, *radius, uv, witness) {
            uv[1] = end;
        }
    }

    if let (
        SurfaceGeometry::Plane { .. },
        crate::families::standard::records::StandardCurveGeometry::Circle { center, radius },
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
        crate::families::standard::records::StandardCurveGeometry::Line => {
            let chord = end.vector_from(start);
            let offset = midpoint.vector_from(start);
            chord.cross(offset).norm() <= 2e-3 * chord.norm().max(1.0)
        }
        crate::families::standard::records::StandardCurveGeometry::Circle { center, radius } => {
            (midpoint.distance_squared(*center).sqrt() - radius).abs() <= 2e-3
        }
        crate::families::standard::records::StandardCurveGeometry::Bspline => match edge_curve {
            Some(CurveGeometry::Line { origin, direction }) => {
                let offset = midpoint.vector_from(*origin);
                (*direction).cross(offset).norm() <= 2e-3 * (*direction).norm().max(1.0)
            }
            _ => false,
        },
    };
    on_curve.then_some((
        PcurveGeometry::Line {
            origin: uv[0],
            direction,
        },
        [0.0, 1.0],
    ))
}

pub(crate) fn witness_arc_end(start: f64, short_end: f64, witness: f64) -> Option<f64> {
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

pub(crate) fn witnessed_surface_circle_end(
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
            ((midpoint.distance_squared(center).sqrt() - radius).abs() <= 2e-3).then_some(candidate)
        })
        .collect::<Vec<_>>();
    <[Point2; 1]>::try_from(candidates).ok().map(|[end]| end)
}

pub(crate) fn analytic_surface_uv(surface: &SurfaceGeometry, point: Point3) -> Option<Point2> {
    match surface {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            let offset = point.vector_from(*origin);
            let v_axis = (*normal).cross(*u_axis);
            Some(Point2::new(offset.dot(*u_axis), offset.dot(v_axis)))
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            ..
        } => {
            let offset = point.vector_from(*origin);
            let tangent = (*axis).cross(*ref_direction);
            Some(Point2::new(
                offset.dot(tangent).atan2(offset.dot(*ref_direction)),
                offset.dot(*axis),
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
            let offset = point.vector_from(*origin);
            let tangent = (*axis).cross(*ref_direction);
            Some(Point2::new(
                (offset.dot(tangent) / ratio).atan2(offset.dot(*ref_direction)),
                offset.dot(*axis),
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
            let offset = point.vector_from(*center);
            let tangent = (*axis).cross(*ref_direction);
            Some(Point2::new(
                offset.dot(tangent).atan2(offset.dot(*ref_direction)),
                (offset.dot(*axis) / radius).clamp(-1.0, 1.0).asin(),
            ))
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            ..
        } => {
            let offset = point.vector_from(*center);
            let tangent = (*axis).cross(*ref_direction);
            let u = offset.dot(tangent).atan2(offset.dot(*ref_direction));
            let radial = Vector3::new(
                u.cos() * ref_direction.x + u.sin() * tangent.x,
                u.cos() * ref_direction.y + u.sin() * tangent.y,
                u.cos() * ref_direction.z + u.sin() * tangent.z,
            );
            Some(Point2::new(
                u,
                offset.dot(*axis).atan2(offset.dot(radial) - major_radius),
            ))
        }
        _ => None,
    }
}

pub(crate) fn unwrap_standard_uv(surface: &SurfaceGeometry, value: &mut Point2, reference: Point2) {
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

pub(crate) fn point_on_surface(point: Point3, surface: &SurfaceGeometry) -> bool {
    const TOLERANCE: f64 = 1e-3;
    let residual = match surface {
        SurfaceGeometry::Plane { origin, normal, .. } => {
            point.vector_from(*origin).dot(*normal).abs()
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        } => {
            let axial = point.vector_from(*origin).dot(*axis);
            let radial = point.distance_squared(*origin) - axial * axial;
            (radial.max(0.0).sqrt() - *radius).abs()
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            radius,
            half_angle,
            ..
        } => {
            let axial = point.vector_from(*origin).dot(*axis);
            let radial = (point.distance_squared(*origin) - axial * axial)
                .max(0.0)
                .sqrt();
            (radial - (radius + axial * half_angle.tan()).abs()).abs()
        }
        SurfaceGeometry::Sphere { center, radius, .. } => {
            (point.distance_squared(*center).sqrt() - *radius).abs()
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            major_radius,
            minor_radius,
            ..
        } => {
            let axial = point.vector_from(*center).dot(*axis);
            let radial = (point.distance_squared(*center) - axial * axial)
                .max(0.0)
                .sqrt();
            (((radial - major_radius).powi(2) + axial * axial).sqrt() - *minor_radius).abs()
        }
        SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Transformed { .. }
        | SurfaceGeometry::Unknown { .. } => return false,
    };
    residual <= TOLERANCE
}

pub(crate) fn standard_spline_plane_line(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    support: &crate::families::standard::records::StandardCurveSupport,
    points: [usize; 2],
) -> Option<(CurveGeometry, [f64; 2])> {
    const TOLERANCE: f64 = 2e-3;

    let surfaces = support
        .faces
        .map(|face| face_surface(ir, bindings, surface_indices, face));
    let [Some(left), Some(right)] = surfaces else {
        return None;
    };
    let (
        SurfaceGeometry::Plane {
            normal: left_normal,
            ..
        },
        SurfaceGeometry::Plane {
            normal: right_normal,
            ..
        },
    ) = (&left.geometry, &right.geometry)
    else {
        return None;
    };
    let intersection = (*left_normal).cross(*right_normal);
    let intersection_norm = intersection.norm();
    if !intersection_norm.is_finite() || intersection_norm <= f64::EPSILON {
        return None;
    }
    let start = ir.model.points.get(points[0])?.position;
    let end = ir.model.points.get(points[1])?.position;
    if !point_on_surface(start, &left.geometry)
        || !point_on_surface(start, &right.geometry)
        || !point_on_surface(end, &left.geometry)
        || !point_on_surface(end, &right.geometry)
    {
        return None;
    }
    let direction = end.vector_from(start);
    let length = direction.norm();
    if !length.is_finite() || length <= f64::EPSILON {
        return None;
    }
    let intersection = intersection.scale(1.0 / intersection_norm);
    if direction.cross(intersection).norm() > TOLERANCE {
        return None;
    }
    Some((
        CurveGeometry::Line {
            origin: start,
            direction: direction.scale(1.0 / length),
        },
        [0.0, length],
    ))
}

pub(crate) fn build_standard_edge_curve(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    brep: &[u8],
    support: &crate::families::standard::records::StandardCurveSupport,
    points: [usize; 2],
) -> (Option<CurveId>, Option<[f64; 2]>) {
    let (geometry, mut param_range) = match &support.geometry {
        crate::families::standard::records::StandardCurveGeometry::Line => {
            let start = ir.model.points[points[0]].position;
            let end = ir.model.points[points[1]].position;
            let delta = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
            let length = delta.dot(delta).sqrt();
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
        crate::families::standard::records::StandardCurveGeometry::Circle { center, radius } => {
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
                .any(|other| axis.dot(*other).abs() < 0.9999)
            {
                return (None, None);
            }
            let candidates = [axis, axis.scale(-1.0)]
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
                    Some((
                        axis,
                        ref_direction,
                        crate::nurbs::canonical_periodic_range(range)?,
                    ))
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
        crate::families::standard::records::StandardCurveGeometry::Bspline => {
            match standard_spline_plane_line(ir, bindings, surface_indices, support, points) {
                Some((geometry, range)) => (geometry, Some(range)),
                None => (
                    CurveGeometry::Unknown {
                        record: Some(UnknownId("catia:payload:unknown#brep-stream".to_string())),
                    },
                    None,
                ),
            }
        }
    };
    let id = CurveId(format!("catia:standard:curve#{}", support.pos));
    annotate(
        annotations,
        &id,
        "MainDataStream+SurfacicReps",
        support.pos as u64,
        "curve_support_60",
        match (&support.geometry, &geometry) {
            (_, CurveGeometry::Unknown { .. }) => Exactness::Unknown,
            (crate::families::standard::records::StandardCurveGeometry::Bspline, _) => {
                Exactness::Derived
            }
            _ => Exactness::ByteExact,
        },
    );
    if matches!(&geometry, CurveGeometry::Line { .. }) {
        annotations
            .derived(&id, "geometry.origin")
            .derived(&id, "geometry.direction");
    } else if matches!(
        &support.geometry,
        crate::families::standard::records::StandardCurveGeometry::Circle { .. }
    ) {
        annotations.derived(&id, "geometry.axis");
    }
    ir.model.curves.push(Curve {
        id: id.clone(),
        geometry,
        source_object: Some(cgm_source("edge-support", support.tag)),
    });
    if matches!(
        &support.geometry,
        crate::families::standard::records::StandardCurveGeometry::Bspline
    ) {
        let sides = support.faces.map(|face| {
            let surface = bindings
                .get(face)
                .and_then(|(id, _, _)| surface_indices.get(id).map(|_| id.clone()));
            IntcurveSupportSide {
                surface,
                pcurve: None,
                pcurve_parameter_range: None,
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
            let parameter_range = param_range.unwrap_or([0.0, 1.0]);
            ir.model.procedural_curves.push(ProceduralCurve {
                id: procedural_id,
                curve: id.clone(),
                definition: ProceduralCurveDefinition::Intersection {
                    context: IntcurveSupportContext {
                        sides,
                        parameter_range,
                        discontinuities: std::array::from_fn(|_| Vec::new()),
                    },
                    discontinuity_flag: false,
                },
                cache_fit_tolerance: None,
            });
            param_range = Some(parameter_range);
        }
    }
    (Some(id), param_range)
}

pub(crate) fn standard_circle_pair_solution_is_simple(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    brep: &[u8],
    supports: &[crate::families::standard::records::StandardCurveSupport],
    endpoint_options: &[Vec<[usize; 2]>],
    pairs: &[Option<[usize; 2]>],
) -> bool {
    type CircleFaceKey = (u64, u64, u64, u64, usize);

    let mut ranges = HashMap::<CircleFaceKey, Vec<[f64; 2]>>::new();
    for ((support, options), pair) in supports.iter().zip(endpoint_options).zip(pairs) {
        let Some(pair) = pair else {
            continue;
        };
        if options.len() <= 1 {
            continue;
        }
        let crate::families::standard::records::StandardCurveGeometry::Circle { center, radius } =
            &support.geometry
        else {
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
            .any(|other| axis.dot(*other).abs() < 0.9999)
        {
            return false;
        }
        let candidates = [axis, axis.scale(-1.0)]
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
                crate::nurbs::canonical_periodic_range(range)
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

pub(crate) fn circular_ranges_are_nonoverlapping_or_coincident(ranges: &[[f64; 2]]) -> bool {
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

    ranges.iter().enumerate().all(|(left_index, left)| {
        ranges[left_index + 1..].iter().all(|right| {
            let coincident =
                (right[0] - left[0]).abs() <= 1e-9 && (right[1] - left[1]).abs() <= 1e-9;
            coincident
                || !segments(*left).iter().any(|left| {
                    segments(*right)
                        .iter()
                        .any(|right| left[1].min(right[1]) - left[0].max(right[0]) > 1e-6)
                })
        })
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn standard_circle_param_range(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    brep: &[u8],
    support: &crate::families::standard::records::StandardCurveSupport,
    center: Point3,
    radius: f64,
    axis: Vector3,
    ref_direction: Vector3,
    start: Point3,
    end: Point3,
) -> Option<[f64; 2]> {
    let mut ranges = support.faces.iter().filter_map(|face| {
        let surface = face_surface(ir, bindings, surface_indices, *face)?;
        let witness = crate::families::standard::records::standard_face_witness(
            brep,
            bindings.get(*face)?.2,
        )?;
        let (PcurveGeometry::Line { origin, direction }, _) =
            standard_pcurve_geometry(&surface.geometry, support, start, end, Some(witness), None)?
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

pub(crate) fn attach_standard_circles(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    bindings: &[(SurfaceId, bool, usize)],
    brep: &[u8],
) {
    for circle in crate::families::standard::records::standard_circles(brep, bindings.len()) {
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
            .any(|other| axis.dot(*other).abs() < 0.9999)
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
            source_object: Some(cgm_source("edge-support", circle.tag)),
        });
    }
}

pub(crate) fn circle_axis_from_carrier(
    center: Point3,
    circle_radius: f64,
    surface: &SurfaceGeometry,
) -> Option<Vector3> {
    match surface {
        SurfaceGeometry::Plane { origin, normal, .. } => {
            close_length(center.vector_from(*origin).dot(*normal), 0.0).then_some(*normal)
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            radius,
            ..
        } => {
            let offset = center.vector_from(*origin);
            let axial = offset.dot(*axis);
            let radial = offset - (*axis).scale(axial);
            (close_length(radial.norm(), 0.0) && close_length(circle_radius, *radius))
                .then_some(*axis)
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            radius,
            half_angle,
            ..
        } => {
            let offset = center.vector_from(*origin);
            let axial = offset.dot(*axis);
            let radial = offset - (*axis).scale(axial);
            let section_radius = (radius + axial * half_angle.tan()).abs();
            (close_length(radial.norm(), 0.0) && close_length(circle_radius, section_radius))
                .then_some(*axis)
        }
        SurfaceGeometry::Sphere {
            center: sphere_center,
            radius: sphere_radius,
            ..
        } => {
            let offset = center.vector_from(*sphere_center);
            let distance = offset.norm();
            (distance > f64::EPSILON
                && close_squared(
                    distance * distance + circle_radius * circle_radius,
                    sphere_radius * sphere_radius,
                ))
            .then(|| offset.scale(1.0 / distance))
        }
        SurfaceGeometry::Torus {
            center: torus_center,
            axis,
            major_radius,
            minor_radius,
            ..
        } => {
            let offset = center.vector_from(*torus_center);
            let axial = offset.dot(*axis);
            let radial = offset - (*axis).scale(axial);
            let radial_distance = radial.norm();
            if close_length(axial, 0.0)
                && close_length(radial_distance, *major_radius)
                && close_length(circle_radius, *minor_radius)
            {
                unit_vector((*axis).cross(radial))
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
        SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Transformed { .. }
        | SurfaceGeometry::Unknown { .. } => None,
    }
}

pub(crate) fn close_length(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-5 * (1.0 + left.abs().max(right.abs()))
}

pub(crate) fn close_squared(left: f64, right: f64) -> bool {
    (left - right).abs() <= 2e-5 * (1.0 + left.abs().max(right.abs()))
}

pub(crate) fn attach_standard_lines(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    bindings: &[(SurfaceId, bool, usize)],
    brep: &[u8],
) {
    for line in crate::families::standard::records::standard_lines(brep, bindings.len()) {
        let Some((origin_a, normal_a)) = plane_for_face(ir, bindings, line.faces[0]) else {
            continue;
        };
        let Some((origin_b, normal_b)) = plane_for_face(ir, bindings, line.faces[1]) else {
            continue;
        };
        let direction = normal_a.cross(normal_b);
        let denom = direction.dot(direction);
        if denom <= f64::EPSILON {
            continue;
        }
        let d_a = normal_a.dot(Vector3::new(origin_a.x, origin_a.y, origin_a.z));
        let d_b = normal_b.dot(Vector3::new(origin_b.x, origin_b.y, origin_b.z));
        let numerator = Vector3::new(
            d_a * normal_b.x - d_b * normal_a.x,
            d_a * normal_b.y - d_b * normal_a.y,
            d_a * normal_b.z - d_b * normal_a.z,
        );
        let point = numerator.cross(direction);
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
            source_object: Some(cgm_source("edge-support", line.tag)),
        });
    }
}

pub(crate) fn plane_for_face(
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

#[cfg(test)]
mod route_tests {
    use crate::assemble::{
        attach_free_vertices, circle_parameter_range_from_surface_branch, ordered_range,
        rational_pcurve_arc,
    };
    use crate::families::e5::decode::reverse_e5_pcurve_geometry;
    use crate::families::standard::decode::{
        bind_ordered_standard_curve_branches, build_standard_edge_curve,
        circular_ranges_are_nonoverlapping_or_coincident, combine_propagated_endpoint_pairs,
        include_native_endpoint_pairs, intersection_line_direction, merge_native_endpoint_evidence,
        point_on_known_surface, point_on_standard_face, resolve_standard_endpoint_pairs,
        standard_circle_endpoint_candidates, standard_circle_param_range, standard_pcurve_geometry,
        standard_plane_normal_from_adjacent_circle_carriers,
        standard_plane_normal_from_circle_centers, standard_successor_endpoint_pairs,
        unique_native_identity_points,
    };

    use crate::families::standard::records::{
        FreeformFaceBounds, StandardCurveGeometry, StandardCurveSupport,
    };

    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::eval::pcurve_uv;
    use cadmpeg_ir::geometry::{
        CurveGeometry, PcurveGeometry, ProceduralCurve, ProceduralCurveDefinition, Surface,
        SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{PointId, SurfaceId, VertexId};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::topology::{Point, Vertex};
    use cadmpeg_ir::units::Units;

    use cadmpeg_ir::AnnotationBuilder;
    use std::collections::BTreeMap;
    use std::collections::HashMap;

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
            [std::f64::consts::PI, tau],
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
    fn incident_circle_centers_determine_a_unique_plane_normal() {
        let circle = |center| StandardCurveSupport {
            pos: 0,
            tag: 0,
            faces: [2, 3],
            geometry: StandardCurveGeometry::Circle {
                center,
                radius: 1.0,
            },
        };
        let mut supports = vec![
            circle(Point3::new(0.0, 0.0, 2.0)),
            circle(Point3::new(2.0, 0.0, 2.0)),
            circle(Point3::new(0.0, 3.0, 2.0)),
        ];

        assert_eq!(
            standard_plane_normal_from_circle_centers(&supports, 2),
            Some([0.0, 0.0, 1.0])
        );
        supports.push(circle(Point3::new(1.0, 1.0, 2.5)));
        assert!(standard_plane_normal_from_circle_centers(&supports, 2).is_none());
    }

    #[test]
    fn adjacent_cone_latitudes_determine_a_cap_plane_normal() {
        let cone = SurfaceGeometry::Cone {
            origin: Point3::new(0.0, 0.0, 52.5),
            axis: Vector3::new(0.0, 0.0, -1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 0.0,
            ratio: 1.0,
            half_angle: std::f64::consts::FRAC_PI_4,
        };
        let supports = vec![StandardCurveSupport {
            pos: 0,
            tag: 0,
            faces: [2, 3],
            geometry: StandardCurveGeometry::Circle {
                center: Point3::new(0.0, 0.0, 50.0),
                radius: 2.5,
            },
        }];
        let surfaces = vec![None, None, None, Some(cone)];

        assert_eq!(
            standard_plane_normal_from_adjacent_circle_carriers(&supports, &surfaces, 2),
            Some([0.0, 0.0, 1.0])
        );

        let mut mismatched = supports;
        let StandardCurveGeometry::Circle { radius, .. } = &mut mismatched[0].geometry else {
            unreachable!()
        };
        *radius = 3.0;
        assert!(
            standard_plane_normal_from_adjacent_circle_carriers(&mismatched, &surfaces, 2)
                .is_none()
        );
    }

    #[test]
    fn standard_planar_spline_edge_solves_line_and_retains_intersection_construction() {
        let mut ir = CadIr::empty(Units::default());
        let mut annotations = AnnotationBuilder::new();
        for (index, position) in [Point3::new(1.0, 0.0, 0.0), Point3::new(4.0, 0.0, 0.0)]
            .into_iter()
            .enumerate()
        {
            ir.model.points.push(Point {
                id: PointId(format!("p{index}")),
                position,
                source_object: None,
            });
        }
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
            [0, 1],
        );
        let id = id.expect("spline support identifies a curve carrier");
        assert_eq!(range, Some([0.0, 3.0]));
        assert_eq!(ir.model.curves[0].id, id);
        assert_eq!(
            ir.model.curves[0].geometry,
            CurveGeometry::Line {
                origin: Point3::new(1.0, 0.0, 0.0),
                direction: Vector3::new(1.0, 0.0, 0.0),
            }
        );
        assert!(matches!(
            ir.model.procedural_curves.as_slice(),
            [ProceduralCurve {
                curve,
                definition: ProceduralCurveDefinition::Intersection { context, .. },
                ..
            }] if curve == &id
                && context.sides[0].surface.as_ref().is_some_and(|id| id.0 == "surface-0")
                && context.sides[1].surface.as_ref().is_some_and(|id| id.0 == "surface-1")
                && context.parameter_range == [0.0, 3.0]
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
                source_object: None,
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
        attach_free_vertices(
            &mut ir,
            &mut annotations,
            "standard",
            "MainDataStream+SurfacicReps",
        );
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
                source_object: None,
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
    fn standard_planar_intersection_spline_uses_the_common_line_domain() {
        let mut ir = CadIr::empty(Units::default());
        for (index, position) in [
            Point3::new(-2.0, 0.0, 0.0),
            Point3::new(3.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.0, 0.0, 1.0),
        ]
        .into_iter()
        .enumerate()
        {
            ir.model.points.push(Point {
                id: PointId(format!("p{index}")),
                position,
                source_object: None,
            });
        }
        for (index, normal) in [Vector3::new(0.0, 0.0, 1.0), Vector3::new(0.0, 1.0, 0.0)]
            .into_iter()
            .enumerate()
        {
            ir.model.surfaces.push(Surface {
                id: SurfaceId(format!("s{index}")),
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal,
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
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
            &[vec![0, 1, 2, 3]],
        )
        .expect("endpoint option pass");

        assert_eq!(choices, [vec![[0, 1]]]);
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
                source_object: None,
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
    fn standard_spline_rows_bind_complete_bipartite_domains_by_allocation_rank() {
        let supports = [10, 11].map(|tag| StandardCurveSupport {
            pos: tag as usize,
            tag,
            faces: [3, 7],
            geometry: StandardCurveGeometry::Bspline,
        });
        let domain = vec![[2, 8], [2, 9], [3, 8], [3, 9]];
        let mut candidates = [domain.clone(), domain];

        bind_ordered_standard_curve_branches(&supports, &mut candidates);

        assert_eq!(candidates, [vec![[2, 8]], vec![[3, 9]]]);
    }

    #[test]
    fn standard_circle_rows_bind_equal_domains_by_allocation_rank() {
        let supports = [10, 11].map(|tag| StandardCurveSupport {
            pos: tag as usize,
            tag,
            faces: [3, 7],
            geometry: StandardCurveGeometry::Circle {
                center: Point3::new(0.0, 0.0, 2.0),
                radius: 4.0,
            },
        });
        let domain = vec![[2, 8], [2, 9]];
        let mut candidates = [domain.clone(), domain];

        bind_ordered_standard_curve_branches(&supports, &mut candidates);

        assert_eq!(candidates, [vec![[2, 8]], vec![[2, 9]]]);
    }

    #[test]
    fn standard_edge_allocation_binds_two_successor_vertices() {
        let supports = [
            StandardCurveSupport {
                pos: 8,
                tag: 100,
                faces: [1, 2],
                geometry: StandardCurveGeometry::Bspline,
            },
            StandardCurveSupport {
                pos: 9,
                tag: 200,
                faces: [2, 3],
                geometry: StandardCurveGeometry::Bspline,
            },
        ];

        assert_eq!(
            standard_successor_endpoint_pairs(&supports, &[99, 101, 102, 202]),
            [Some([1, 2]), None]
        );
    }

    #[test]
    fn standard_spline_rows_exclude_adjacent_fixed_boundary_relations() {
        let supports = [
            StandardCurveSupport {
                pos: 8,
                tag: 8,
                faces: [1, 3],
                geometry: StandardCurveGeometry::Circle {
                    center: Point3::new(0.0, 0.0, 0.0),
                    radius: 1.0,
                },
            },
            StandardCurveSupport {
                pos: 9,
                tag: 9,
                faces: [3, 4],
                geometry: StandardCurveGeometry::Circle {
                    center: Point3::new(0.0, 0.0, 1.0),
                    radius: 1.0,
                },
            },
            StandardCurveSupport {
                pos: 10,
                tag: 10,
                faces: [3, 7],
                geometry: StandardCurveGeometry::Bspline,
            },
            StandardCurveSupport {
                pos: 11,
                tag: 11,
                faces: [3, 7],
                geometry: StandardCurveGeometry::Bspline,
            },
        ];
        let complete = vec![[2, 3], [2, 8], [2, 9], [3, 8], [3, 9], [8, 9]];
        let mut candidates = [vec![[2, 3]], vec![[8, 9]], complete.clone(), complete];

        bind_ordered_standard_curve_branches(&supports, &mut candidates);

        assert_eq!(candidates[2], [[2, 8]]);
        assert_eq!(candidates[3], [[3, 9]]);
    }

    #[test]
    fn standard_spline_branch_rank_leaves_incomplete_relations_unresolved() {
        let supports = [10, 11].map(|tag| StandardCurveSupport {
            pos: tag as usize,
            tag,
            faces: [3, 7],
            geometry: StandardCurveGeometry::Bspline,
        });
        let domain = vec![[2, 8], [2, 9], [3, 9]];
        let mut candidates = [domain.clone(), domain.clone()];

        bind_ordered_standard_curve_branches(&supports, &mut candidates);

        assert_eq!(candidates, [domain.clone(), domain]);
    }

    #[test]
    fn standard_circle_endpoint_domain_uses_the_explicit_curve_carrier() {
        let points = [
            Point {
                id: PointId("on".to_string()),
                position: Point3::new(3.0, 4.0, 7.0),
                source_object: None,
            },
            Point {
                id: PointId("off".to_string()),
                position: Point3::new(3.0, 4.01, 7.0),
                source_object: None,
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
                source_object: None,
            },
            Point {
                id: PointId("other-occurrence".to_string()),
                position: Point3::new(3.0, -4.0, 0.0),
                source_object: None,
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
    fn native_endpoint_evidence_rejects_directed_pair_conflicts() {
        let graph = [Some([0, 1]), None];
        let roster = [Some([0, 1]), Some([2, 3])];
        assert_eq!(
            merge_native_endpoint_evidence(Some(&graph), Some(&roster)),
            Ok(Some(vec![Some([0, 1]), Some([2, 3])]))
        );
        assert_eq!(
            merge_native_endpoint_evidence(Some(&graph), Some(&[Some([1, 0]), None])),
            Err("conflicting native endpoint evidence")
        );
    }

    #[test]
    fn complete_vertex_roster_supersedes_partial_graph_coordinates() {
        let graph = [Some([4, 5]), None];
        let roster = [Some([0, 1]), Some([2, 3])];
        assert_eq!(
            merge_native_endpoint_evidence(Some(&graph), Some(&roster)),
            Ok(Some(roster.to_vec()))
        );
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
                source_object: None,
            },
            Point {
                id: PointId("b".to_string()),
                position: Point3::new(1.01, 0.0, 0.0),
                source_object: None,
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
        let range = crate::nurbs::canonical_periodic_range([tau - 1e-14, tau + 0.25])
            .expect("canonical seam range");
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
        let reversed = reverse_e5_pcurve_geometry(&geometry, range).expect("reversible line");
        for (parameter, source_parameter) in [(5.0, 9.0), (9.0, 5.0)] {
            let actual = pcurve_uv(&reversed, parameter).expect("reversed evaluation");
            let expected = pcurve_uv(&geometry, source_parameter).expect("source evaluation");
            assert!((actual.u - expected.u).abs() < 1e-12);
            assert!((actual.v - expected.v).abs() < 1e-12);
        }
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
    fn cylinder_generator_direction_requires_compatible_support_axes() {
        let cylinder = |axis| SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis,
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 1.0,
        };
        let containing_plane = SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let transverse_plane = SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let axial = cylinder(Vector3::new(0.0, 0.0, 1.0));
        let oblique = cylinder(Vector3::new(0.0, 1.0, 0.0));

        assert_eq!(
            intersection_line_direction(&containing_plane, &axial),
            Some(Vector3::new(0.0, 0.0, 1.0))
        );
        assert!(intersection_line_direction(&transverse_plane, &axial).is_none());
        assert!(intersection_line_direction(&axial, &oblique).is_none());
    }

    #[test]
    fn unknown_surface_does_not_reject_endpoint_candidates() {
        assert!(point_on_known_surface(
            Point3::new(100.0, -50.0, 7.0),
            &SurfaceGeometry::Unknown { record: None }
        ));
    }

    #[test]
    fn freeform_face_bounds_constrain_unknown_surface_endpoints() {
        let bounds = FreeformFaceBounds {
            aabb_center: [2.0, 3.0, 4.0],
            aabb_half_extents: [1.0, 2.0, 3.0],
            sphere_center: [2.0, 3.0, 4.0],
            sphere_radius: 3.5,
        };
        let surface = SurfaceGeometry::Unknown { record: None };
        assert!(point_on_standard_face(
            Point3::new(2.0, 4.0, 6.0),
            &surface,
            Some(bounds),
        ));
        assert!(!point_on_standard_face(
            Point3::new(3.01, 3.0, 4.0),
            &surface,
            Some(bounds),
        ));
        assert!(!point_on_standard_face(
            Point3::new(3.0, 5.0, 7.0),
            &surface,
            Some(bounds),
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
    fn solved_planar_spline_line_inverts_to_exact_parameter_line() {
        let surface = SurfaceGeometry::Plane {
            origin: Point3::new(1.0, 2.0, 3.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let support = StandardCurveSupport {
            pos: 0,
            tag: 1,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Bspline,
        };
        let start = Point3::new(2.0, 4.0, 3.0);
        let end = Point3::new(5.0, 8.0, 3.0);
        let carrier = CurveGeometry::Line {
            origin: start,
            direction: Vector3::new(3.0, 4.0, 0.0),
        };
        let (geometry, range) =
            standard_pcurve_geometry(&surface, &support, start, end, None, Some(&carrier))
                .expect("solved spline line pcurve");

        assert_eq!(range, [0.0, 1.0]);
        assert_eq!(
            geometry,
            PcurveGeometry::Line {
                origin: Point2::new(1.0, 2.0),
                direction: Point2::new(3.0, 4.0),
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
            None,
        )
        .is_none());
    }

    #[test]
    fn standard_cone_apex_uses_the_other_endpoint_angular_gauge() {
        let half_angle = 0.25f64;
        let surface = SurfaceGeometry::Cone {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 0.0,
            ratio: 1.0,
            half_angle,
        };
        let support = StandardCurveSupport {
            pos: 0,
            tag: 1,
            faces: [0, 1],
            geometry: StandardCurveGeometry::Line,
        };
        let height = 4.0;
        let radius = height * half_angle.tan();
        let (geometry, range) = standard_pcurve_geometry(
            &surface,
            &support,
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, radius, height),
            None,
            None,
        )
        .expect("cone generator through the apex");
        assert_eq!(range, [0.0, 1.0]);
        assert_eq!(
            geometry,
            PcurveGeometry::Line {
                origin: cadmpeg_ir::math::Point2::new(std::f64::consts::FRAC_PI_2, 0.0),
                direction: cadmpeg_ir::math::Point2::new(0.0, height),
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
            None,
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
            None,
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
    fn standard_cylinder_endpoint_witness_preserves_geometric_arc() {
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
            Point3::new(-2.0, 0.0, 3.0),
            Point3::new(0.0, -2.0, 3.0),
            Some(Point3::new(-1.0, 0.0, 4.0)),
            None,
        )
        .expect("endpoint-aligned witness does not reject the arc");
        assert_eq!(
            geometry,
            PcurveGeometry::Line {
                origin: cadmpeg_ir::math::Point2::new(std::f64::consts::PI, 3.0),
                direction: cadmpeg_ir::math::Point2::new(std::f64::consts::FRAC_PI_2, 0.0),
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
            None,
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
