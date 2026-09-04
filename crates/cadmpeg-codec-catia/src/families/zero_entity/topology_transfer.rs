//! Lower complete zero-entity endpoint relations into neutral B-rep topology.

use std::collections::HashMap;

use cadmpeg_core::decode::WorkBudget;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::curve_point;
use cadmpeg_ir::geometry::{Pcurve, PcurveGeometry};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{
    AnchoredVertexUse, Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, PcurveUse,
    Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use crate::assemble::annotate;
use crate::nurbs::canonical_model_curve_range;

use super::records::{ZeroEntityOwnershipRoot, ZeroEntitySupportRun};
use super::topology::{
    endpoint_locus_candidates_with_budget, zero_entity_endpoint_pair_candidates_with_budget,
};

const MODEL_POINT_TOLERANCE: f64 = 2e-3;

/// Counts one complete geometry-derived zero-entity topology transfer.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ZeroEntityTopologyCounts {
    pub(crate) bodies: usize,
    pub(crate) faces: usize,
    pub(crate) loops: usize,
    pub(crate) coedges: usize,
    pub(crate) edges: usize,
    pub(crate) vertices: usize,
    pub(crate) points: usize,
    pub(crate) pcurves: usize,
}

#[derive(Debug, Clone)]
struct Occurrence {
    support_record_ordinal: u32,
    raw_endpoints: [Point3; 2],
    oriented_endpoints: [Point3; 2],
    model_parameters: Option<[f64; 2]>,
    curve: CurveId,
    oriented_curve: Option<CurveId>,
    oriented_curve_parameter_range: Option<[f64; 2]>,
    pcurve: Option<OccurrencePcurve>,
}

#[derive(Debug, Clone)]
struct OccurrencePcurve {
    id: PcurveId,
    geometry: PcurveGeometry,
    parameter_range: [f64; 2],
}

/// Transfer a closed, geometry-resolved zero-entity B-rep subset.
///
/// The source allocation registries remain native. This route uses only the
/// settled geometric relations: a unique radial pair is one physical edge,
/// and a complete endpoint clique is one physical vertex. It therefore
/// refuses the whole candidate when any support occurrence or endpoint lacks
/// a unique relation. The caller keeps the existing wire transfer as the
/// atomic fallback.
pub(crate) fn transfer_closed_face_topology(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    support_runs: &[ZeroEntitySupportRun],
    surface_ids_by_position: &HashMap<usize, SurfaceId>,
    support_curve_ids: &HashMap<u32, CurveId>,
    ownership_root: Option<&ZeroEntityOwnershipRoot>,
    topology_budget: &WorkBudget<'_>,
) -> Option<ZeroEntityTopologyCounts> {
    if support_runs.is_empty() || support_runs.iter().any(|run| run.face.is_none()) {
        return None;
    }
    if let Some(ownership_root) = ownership_root {
        if ownership_root.face_slots.len() != support_runs.len()
            || ownership_root.face_slots
                != (1..=u32::try_from(support_runs.len()).ok()?)
                    .rev()
                    .collect::<Vec<_>>()
        {
            return None;
        }
    }

    let mut occurrences = Vec::new();
    let mut occurrence_by_support = HashMap::<u32, usize>::new();
    let mut face_ids = Vec::with_capacity(support_runs.len());
    let mut face_id_by_ordinal = HashMap::<u32, FaceId>::new();

    for run in support_runs {
        let face = run.face.as_ref()?;
        let surface_id = surface_ids_by_position.get(&run.carrier_pos)?;
        let surface_geometry = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *surface_id)
            .map(|surface| &surface.geometry)?;
        let face_id = FaceId::mint(format!(
            "catia:zero-entity:topology-face#{}",
            face.record_ordinal
        ))
        .expect("identity grammar");
        if face_id_by_ordinal
            .insert(face.record_ordinal, face_id.clone())
            .is_some()
        {
            return None;
        }
        face_ids.push(face_id);

        let supports_by_ordinal = run
            .supports
            .iter()
            .map(|support| (support.record_ordinal, support))
            .collect::<HashMap<_, _>>();
        for loop_record in &face.loops {
            if loop_record.support_record_ordinals.len() != loop_record.forward_senses.len()
                || loop_record.support_record_ordinals.len()
                    != loop_record.oriented_model_endpoints.len()
                || loop_record.support_record_ordinals.is_empty()
            {
                return None;
            }
            for (member_index, support_record_ordinal) in loop_record
                .support_record_ordinals
                .iter()
                .copied()
                .enumerate()
            {
                let support = *supports_by_ordinal.get(&support_record_ordinal)?;
                let curve = support_curve_ids.get(&support_record_ordinal).cloned()?;
                if !ir
                    .model
                    .curves
                    .iter()
                    .any(|candidate| candidate.id == curve)
                {
                    return None;
                }
                let raw_endpoints = support.model_endpoints?;
                let oriented_endpoints = loop_record.oriented_model_endpoints[member_index];
                let pcurve = match support.pcurve.as_ref() {
                    Some(pcurve) => {
                        let geometry =
                            super::records::zero_entity_neutral_pcurve(surface_geometry, pcurve)?;
                        let parameter_range = pcurve_parameter_range(&geometry)?;
                        Some(OccurrencePcurve {
                            id: PcurveId::mint(format!(
                                "catia:zero-entity:topology-pcurve#{support_record_ordinal}"
                            ))
                            .expect("identity grammar"),
                            geometry,
                            parameter_range,
                        })
                    }
                    None => None,
                };
                let occurrence_index = occurrences.len();
                if occurrence_by_support
                    .insert(support_record_ordinal, occurrence_index)
                    .is_some()
                {
                    return None;
                }
                occurrences.push(Occurrence {
                    support_record_ordinal,
                    raw_endpoints,
                    oriented_endpoints,
                    model_parameters: support.model_parameters,
                    curve,
                    oriented_curve: None,
                    oriented_curve_parameter_range: None,
                    pcurve,
                });
            }
        }
    }

    for occurrence in &mut occurrences {
        let curve_geometry = ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == occurrence.curve)
            .map(|curve| curve.geometry.clone())?;
        let source_range = occurrence
            .model_parameters
            .and_then(increasing_range)
            .or_else(|| {
                occurrence
                    .pcurve
                    .as_ref()
                    .map(|pcurve| pcurve.parameter_range)
            })
            .and_then(|range| canonical_model_curve_range(&curve_geometry, range));
        let raw_indices =
            endpoint_indices(occurrence.oriented_endpoints, occurrence.raw_endpoints)?;
        let direct_orientation = if matches!(
            &curve_geometry,
            cadmpeg_ir::geometry::CurveGeometry::Procedural { .. }
        ) {
            source_range.map(|range| (range, false))
        } else {
            source_range.and_then(|range| {
                curve_orientation(&curve_geometry, range, occurrence.raw_endpoints)
                    .map(|reversed| (range, reversed))
            })
        };
        let raw_is_oriented = raw_indices == [0, 1];
        let (oriented_curve, oriented_curve_parameter_range) =
            if let Some((parameter_range, reversed)) = direct_orientation {
                let needs_reverse = reversed == raw_is_oriented;
                if needs_reverse
                    && !matches!(
                        &curve_geometry,
                        cadmpeg_ir::geometry::CurveGeometry::Procedural { .. }
                    )
                {
                    let reversed_geometry =
                        crate::nurbs::reverse_curve_geometry(&curve_geometry, parameter_range);
                    let curve = ir
                        .model
                        .curves
                        .iter_mut()
                        .find(|curve| curve.id == occurrence.curve)?;
                    match reversed_geometry {
                        Some((geometry, parameter_range)) => {
                            if let Some(parameter_range) =
                                canonical_model_curve_range(&geometry, parameter_range)
                            {
                                curve.geometry = geometry;
                                annotations.derived(&occurrence.curve, "geometry");
                                (occurrence.curve.clone(), parameter_range)
                            } else {
                                curve.geometry =
                                    cadmpeg_ir::geometry::CurveGeometry::Unknown { record: None };
                                annotations.derived(&occurrence.curve, "geometry");
                                (occurrence.curve.clone(), parameter_range)
                            }
                        }
                        None => {
                            curve.geometry =
                                cadmpeg_ir::geometry::CurveGeometry::Unknown { record: None };
                            annotations.derived(&occurrence.curve, "geometry");
                            (occurrence.curve.clone(), parameter_range)
                        }
                    }
                } else {
                    (occurrence.curve.clone(), parameter_range)
                }
            } else {
                let parameter_range = source_range.or_else(|| {
                    occurrence
                        .pcurve
                        .as_ref()
                        .map(|pcurve| pcurve.parameter_range)
                })?;
                let curve = ir
                    .model
                    .curves
                    .iter_mut()
                    .find(|curve| curve.id == occurrence.curve)?;
                if !matches!(
                    &curve.geometry,
                    cadmpeg_ir::geometry::CurveGeometry::Procedural { .. }
                ) {
                    curve.geometry = cadmpeg_ir::geometry::CurveGeometry::Unknown { record: None };
                }
                annotations.derived(&occurrence.curve, "geometry");
                (occurrence.curve.clone(), parameter_range)
            };
        occurrence.oriented_curve = Some(oriented_curve);
        occurrence.oriented_curve_parameter_range = Some(oriented_curve_parameter_range);
    }

    let support_count = support_runs
        .iter()
        .map(|run| run.supports.len())
        .sum::<usize>();
    if support_count != occurrences.len() {
        return None;
    }

    let edge_candidates =
        zero_entity_endpoint_pair_candidates_with_budget(support_runs, topology_budget)?;
    if edge_candidates.len().checked_mul(2)? != occurrences.len() {
        return None;
    }
    let mut edge_for_support = HashMap::<u32, usize>::new();
    for (edge_index, candidate) in edge_candidates.iter().enumerate() {
        for support_record_ordinal in candidate.support_record_ordinals {
            if !occurrence_by_support.contains_key(&support_record_ordinal)
                || edge_for_support
                    .insert(support_record_ordinal, edge_index)
                    .is_some()
            {
                return None;
            }
        }
    }
    if edge_for_support.len() != occurrences.len() {
        return None;
    }

    let endpoint_loci = endpoint_locus_candidates_with_budget(&edge_candidates, topology_budget)?;
    let mut vertex_for_endpoint = HashMap::<(usize, usize), usize>::new();
    for (vertex_index, locus) in endpoint_loci.iter().enumerate() {
        for &(edge_index, endpoint_index) in &locus.incident_endpoint_pair_endpoints {
            let endpoint_index = usize::from(endpoint_index);
            if edge_index >= edge_candidates.len()
                || endpoint_index >= 2
                || vertex_for_endpoint
                    .insert((edge_index, endpoint_index), vertex_index)
                    .is_some()
            {
                return None;
            }
        }
    }
    if vertex_for_endpoint.len() != edge_candidates.len().checked_mul(2)? {
        return None;
    }

    let first_face = support_runs.first()?.face.as_ref()?;
    let topology_scope = ownership_root.map_or_else(
        || {
            format!(
                "inferred-{}-{}",
                first_face.record_ordinal,
                support_runs.len()
            )
        },
        |root| root.body_record_ordinal.to_string(),
    );
    let body_id = BodyId::mint(format!("catia:zero-entity:topology-body#{topology_scope}"))
        .expect("identity grammar");
    let region_id = RegionId::mint(format!(
        "catia:zero-entity:topology-region#{topology_scope}"
    ))
    .expect("identity grammar");
    let shell_scope = ownership_root.map_or_else(
        || topology_scope.clone(),
        |root| root.shell_record_ordinal.to_string(),
    );
    let shell_id = ShellId::mint(format!("catia:zero-entity:topology-shell#{shell_scope}"))
        .expect("identity grammar");

    let point_ids = endpoint_loci
        .iter()
        .enumerate()
        .map(|(index, _)| {
            PointId::mint(format!("catia:zero-entity:topology-point#{index}"))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let vertex_ids = endpoint_loci
        .iter()
        .enumerate()
        .map(|(index, _)| {
            VertexId::mint(format!("catia:zero-entity:topology-vertex#{index}"))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();

    for (index, locus) in endpoint_loci.iter().enumerate() {
        annotate(
            annotations,
            &point_ids[index],
            "zero_entity_a9_03",
            ownership_root.map_or(first_face.pos, |root| root.face_roster_pos) as u64,
            "endpoint_locus_point",
            Exactness::Inferred,
        );
        annotations.derived(&point_ids[index], "position");
        ir.model.points.push(Point {
            id: point_ids[index].clone(),
            position: locus.representative_point,
            source_object: None,
        });
        annotate(
            annotations,
            &vertex_ids[index],
            "zero_entity_a9_03",
            ownership_root.map_or(first_face.pos, |root| root.face_roster_pos) as u64,
            "endpoint_locus_vertex",
            Exactness::Inferred,
        );
        annotations.derived(&vertex_ids[index], "point");
        ir.model.vertices.push(Vertex {
            id: vertex_ids[index].clone(),
            point: point_ids[index].clone(),
            tolerance: Some(MODEL_POINT_TOLERANCE),
        });
    }

    for occurrence in &occurrences {
        let Some(pcurve) = &occurrence.pcurve else {
            continue;
        };
        annotate(
            annotations,
            &pcurve.id,
            "zero_entity_a9_03",
            occurrence.support_record_ordinal as u64,
            "topology_pcurve",
            Exactness::Derived,
        );
        annotations.derived(&pcurve.id, "geometry");
        ir.model.pcurves.push(Pcurve {
            id: pcurve.id.clone(),
            geometry: pcurve.geometry.clone(),
            metadata: cadmpeg_ir::geometry::PcurveMetadata::general(
                None,
                Some(pcurve.parameter_range),
                None,
            ),
        });
    }

    let occurrence_vertex_pairs = occurrences
        .iter()
        .map(|occurrence| {
            let edge_index = *edge_for_support.get(&occurrence.support_record_ordinal)?;
            let candidate = &edge_candidates[edge_index];
            let oriented_indices =
                endpoint_indices(candidate.model_endpoints, occurrence.oriented_endpoints)?;
            let raw_indices =
                endpoint_indices(occurrence.oriented_endpoints, occurrence.raw_endpoints)?;
            Some((
                [
                    vertex_ids[vertex_for_endpoint[&(edge_index, oriented_indices[0])]].clone(),
                    vertex_ids[vertex_for_endpoint[&(edge_index, oriented_indices[1])]].clone(),
                ],
                [
                    vertex_ids
                        [vertex_for_endpoint[&(edge_index, oriented_indices[raw_indices[0]])]]
                        .clone(),
                    vertex_ids
                        [vertex_for_endpoint[&(edge_index, oriented_indices[raw_indices[1]])]]
                        .clone(),
                ],
                raw_indices == [0, 1],
            ))
        })
        .collect::<Option<Vec<_>>>()?;

    let mut edge_ids = Vec::with_capacity(edge_candidates.len());
    let mut coedges_by_support = HashMap::<u32, CoedgeId>::new();
    for (edge_index, candidate) in edge_candidates.iter().enumerate() {
        let first_occurrence =
            &occurrences[*occurrence_by_support.get(&candidate.support_record_ordinals[0])?];
        let edge_id = EdgeId::mint(format!(
            "catia:zero-entity:topology-edge#{}-{}",
            candidate.support_record_ordinals[0], candidate.support_record_ordinals[1]
        ))
        .expect("identity grammar");
        let oriented_curve = first_occurrence.oriented_curve.as_ref()?;
        let param_range = first_occurrence.oriented_curve_parameter_range;
        let oriented_vertices = &occurrence_vertex_pairs
            [*occurrence_by_support.get(&candidate.support_record_ordinals[0])?]
        .0;
        annotate(
            annotations,
            &edge_id,
            "zero_entity_a9_03",
            first_occurrence.support_record_ordinal as u64,
            "topology_physical_edge_candidate",
            Exactness::Inferred,
        );
        annotations
            .derived(&edge_id, "curve")
            .derived(&edge_id, "start")
            .derived(&edge_id, "end");
        if param_range.is_some() {
            annotations.derived(&edge_id, "param_range");
        }
        ir.model.edges.push(Edge {
            id: edge_id.clone(),
            curve: Some(oriented_curve.clone()),
            start: oriented_vertices[0].clone(),
            end: oriented_vertices[1].clone(),
            param_range,
            tolerance: Some(MODEL_POINT_TOLERANCE),
        });
        edge_ids.push(edge_id);
        debug_assert_eq!(edge_index, edge_ids.len() - 1);
    }

    for (run_index, run) in support_runs.iter().enumerate() {
        let face = run.face.as_ref()?;
        let face_id = &face_ids[run_index];
        let loop_ids = face
            .loops
            .iter()
            .map(|loop_record| {
                LoopId::mint(format!(
                    "catia:zero-entity:topology-loop#{}",
                    loop_record.record_ordinal
                ))
                .expect("identity grammar")
            })
            .collect::<Vec<_>>();
        let outer_sense = match face.loops.first()?.loop_class {
            0x41 => Sense::Forward,
            0xc1 => Sense::Reversed,
            _ => return None,
        };
        annotate(
            annotations,
            face_id,
            "zero_entity_a9_03",
            face.record_ordinal as u64,
            "topology_face",
            Exactness::Inferred,
        );
        annotations
            .derived(face_id, "shell")
            .derived(face_id, "surface")
            .derived(face_id, "sense")
            .derived(face_id, "loops");
        ir.model.faces.push(Face {
            id: face_id.clone(),
            shell: shell_id.clone(),
            surface: surface_ids_by_position[&run.carrier_pos].clone(),
            sense: outer_sense,
            loops: loop_ids.clone(),
            name: None,
            color: None,
            tolerance: None,
        });

        for (loop_index, loop_record) in face.loops.iter().enumerate() {
            let loop_id = &loop_ids[loop_index];
            let coedge_ids = loop_record
                .support_record_ordinals
                .iter()
                .map(|support_record_ordinal| {
                    CoedgeId::mint(format!(
                        "catia:zero-entity:topology-coedge#{support_record_ordinal}"
                    ))
                    .expect("identity grammar")
                })
                .collect::<Vec<_>>();
            let vertex_uses = loop_record
                .support_record_ordinals
                .iter()
                .enumerate()
                .map(|(member_index, support_record_ordinal)| {
                    let occurrence_index = *occurrence_by_support.get(support_record_ordinal)?;
                    Some(AnchoredVertexUse {
                        vertex: occurrence_vertex_pairs[occurrence_index].0[1].clone(),
                        after: coedge_ids[member_index].clone(),
                        pcurves: Vec::new(),
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            let boundary_role = if loop_index == 0 {
                LoopBoundaryRole::Outer
            } else {
                LoopBoundaryRole::Inner
            };
            annotate(
                annotations,
                loop_id,
                "zero_entity_a9_03",
                loop_record.record_ordinal as u64,
                "topology_loop",
                Exactness::Inferred,
            );
            annotations
                .derived(loop_id, "face")
                .derived(loop_id, "coedges")
                .derived(loop_id, "vertex_uses");
            ir.model.loops.push(Loop {
                id: loop_id.clone(),
                face: face_id.clone(),
                boundary_role,
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring {
                    coedges: coedge_ids.clone(),
                    vertex_uses,
                },
            });

            for (member_index, support_record_ordinal) in loop_record
                .support_record_ordinals
                .iter()
                .copied()
                .enumerate()
            {
                let occurrence_index = *occurrence_by_support.get(&support_record_ordinal)?;
                let occurrence = &occurrences[occurrence_index];
                let edge_index = *edge_for_support.get(&support_record_ordinal)?;
                let oriented_vertices = &occurrence_vertex_pairs[occurrence_index].0;
                let edge = &edge_candidates[edge_index];
                let first_occurrence =
                    &occurrences[*occurrence_by_support.get(&edge.support_record_ordinals[0])?];
                let first_oriented_vertices = &occurrence_vertex_pairs
                    [*occurrence_by_support.get(&edge.support_record_ordinals[0])?]
                .0;
                let sense = if oriented_vertices == first_oriented_vertices {
                    Sense::Forward
                } else if oriented_vertices
                    == &[
                        first_oriented_vertices[1].clone(),
                        first_oriented_vertices[0].clone(),
                    ]
                {
                    Sense::Reversed
                } else {
                    return None;
                };
                let use_curve = if occurrence.oriented_curve.as_ref()
                    == first_occurrence.oriented_curve.as_ref()
                {
                    None
                } else {
                    Some(occurrence.oriented_curve.as_ref()?.clone())
                };
                let use_curve = match use_curve {
                    Some(curve) => Some(cadmpeg_ir::topology::CoedgeUseCurve {
                        curve,
                        parameter_range: occurrence.oriented_curve_parameter_range?,
                    }),
                    None => None,
                };
                let pcurves = occurrence
                    .pcurve
                    .as_ref()
                    .map(|pcurve| PcurveUse {
                        pcurve: pcurve.id.clone(),
                        isoparametric: None,
                        parameter_range: if occurrence_vertex_pairs[occurrence_index].2 {
                            Some(pcurve.parameter_range)
                        } else {
                            Some([pcurve.parameter_range[1], pcurve.parameter_range[0]])
                        },
                    })
                    .into_iter()
                    .collect();
                let coedge_id = coedge_ids[member_index].clone();
                annotate(
                    annotations,
                    &coedge_id,
                    "zero_entity_a9_03",
                    occurrence.support_record_ordinal as u64,
                    "topology_coedge",
                    Exactness::Inferred,
                );
                annotations
                    .derived(&coedge_id, "owner_loop")
                    .derived(&coedge_id, "edge")
                    .derived(&coedge_id, "next")
                    .derived(&coedge_id, "previous")
                    .derived(&coedge_id, "radial_next")
                    .derived(&coedge_id, "sense")
                    .derived(&coedge_id, "pcurves");
                if use_curve.is_some() {
                    annotations
                        .derived(&coedge_id, "use_curve")
                        .derived(&coedge_id, "use_curve_parameter_range");
                }
                ir.model.coedges.push(Coedge {
                    id: coedge_id.clone(),
                    owner_loop: loop_id.clone(),
                    edge: edge_ids[edge_index].clone(),
                    next: coedge_ids[(member_index + 1) % coedge_ids.len()].clone(),
                    previous: coedge_ids[(member_index + coedge_ids.len() - 1) % coedge_ids.len()]
                        .clone(),
                    radial_next: coedge_id.clone(),
                    sense,
                    pcurves,
                    use_curve,
                });
                coedges_by_support.insert(support_record_ordinal, coedge_id);
            }
        }
    }

    for candidate in &edge_candidates {
        let first = coedges_by_support.get(&candidate.support_record_ordinals[0])?;
        let second = coedges_by_support.get(&candidate.support_record_ordinals[1])?;
        ir.model
            .coedges
            .iter_mut()
            .find(|coedge| coedge.id == *first)
            .map(|coedge| coedge.radial_next = second.clone())?;
        ir.model
            .coedges
            .iter_mut()
            .find(|coedge| coedge.id == *second)
            .map(|coedge| coedge.radial_next = first.clone())?;
    }

    annotate(
        annotations,
        &body_id,
        "zero_entity_a9_03",
        ownership_root.map_or(first_face.pos, |root| root.body_pos) as u64,
        "topology_body",
        Exactness::Derived,
    );
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
    annotate(
        annotations,
        &region_id,
        "zero_entity_a9_03",
        ownership_root.map_or(first_face.pos, |root| root.shell_pos) as u64,
        "topology_region",
        Exactness::Derived,
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
        "zero_entity_a9_03",
        ownership_root.map_or(first_face.pos, |root| root.shell_pos) as u64,
        "topology_shell",
        Exactness::Derived,
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

    Some(ZeroEntityTopologyCounts {
        bodies: 1,
        faces: support_runs.len(),
        loops: support_runs
            .iter()
            .map(|run| run.face.as_ref().map_or(0, |face| face.loops.len()))
            .sum(),
        coedges: occurrences.len(),
        edges: edge_candidates.len(),
        vertices: endpoint_loci.len(),
        points: endpoint_loci.len(),
        pcurves: occurrences
            .iter()
            .filter(|occurrence| occurrence.pcurve.is_some())
            .count(),
    })
}

fn pcurve_parameter_range(pcurve: &PcurveGeometry) -> Option<[f64; 2]> {
    let PcurveGeometry::Nurbs { nurbs } = pcurve else {
        return None;
    };
    let degree = usize::try_from(nurbs.degree()).ok()?;
    let range = [
        *nurbs.knots().get(degree)?,
        *nurbs.knots().get(nurbs.control_points().len())?,
    ];
    (range.iter().copied().all(f64::is_finite) && range[0] < range[1]).then_some(range)
}

fn curve_orientation(
    geometry: &cadmpeg_ir::geometry::CurveGeometry,
    parameter_range: [f64; 2],
    endpoints: [Point3; 2],
) -> Option<bool> {
    let evaluated = [
        curve_point(geometry, parameter_range[0])?,
        curve_point(geometry, parameter_range[1])?,
    ];
    let direct = evaluated[0].distance(endpoints[0]) <= MODEL_POINT_TOLERANCE
        && evaluated[1].distance(endpoints[1]) <= MODEL_POINT_TOLERANCE;
    let reversed = evaluated[0].distance(endpoints[1]) <= MODEL_POINT_TOLERANCE
        && evaluated[1].distance(endpoints[0]) <= MODEL_POINT_TOLERANCE;
    match (direct, reversed) {
        (true, false) => Some(false),
        (false, true) => Some(true),
        _ => None,
    }
}

fn increasing_range(parameters: [f64; 2]) -> Option<[f64; 2]> {
    if !parameters.into_iter().all(f64::is_finite) || parameters[0] == parameters[1] {
        return None;
    }
    Some([
        parameters[0].min(parameters[1]),
        parameters[0].max(parameters[1]),
    ])
}

fn endpoint_indices(reference: [Point3; 2], target: [Point3; 2]) -> Option<[usize; 2]> {
    let direct = reference[0].distance(target[0]) <= MODEL_POINT_TOLERANCE
        && reference[1].distance(target[1]) <= MODEL_POINT_TOLERANCE;
    let reversed = reference[0].distance(target[1]) <= MODEL_POINT_TOLERANCE
        && reference[1].distance(target[0]) <= MODEL_POINT_TOLERANCE;
    match (direct, reversed) {
        (true, false) => Some([0, 1]),
        (false, true) => Some([1, 0]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use cadmpeg_core::decode::WorkBudget;
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface, SurfaceGeometry};
    use cadmpeg_ir::math::Vector3;

    use super::super::records::ZeroEntitySupportOccurrence;
    use super::*;

    fn support(ordinal: u32, start: Point3, end: Point3) -> ZeroEntitySupportOccurrence {
        ZeroEntitySupportOccurrence {
            pos: ordinal as usize,
            record_ordinal: ordinal,
            tag: [0x21, 0x71],
            face_local_slot: ordinal,
            uv_endpoints: None,
            pcurve: None,
            model_curve: Some(CurveGeometry::Line {
                origin: start,
                direction: end
                    .vector_from(start)
                    .unit()
                    .expect("non-degenerate test edge"),
            }),
            model_curve_construction: None,
            model_parameters: Some([0.0, end.distance(start)]),
            model_midpoint: Some(Point3::new(
                (start.x + end.x) * 0.5,
                (start.y + end.y) * 0.5,
                (start.z + end.z) * 0.5,
            )),
            model_endpoints: Some([start, end]),
        }
    }

    fn run(
        face_ordinal: u32,
        support_base: u32,
        points: [Point3; 3],
        reversed: bool,
    ) -> ZeroEntitySupportRun {
        let order = if reversed {
            [
                (points[1], points[0]),
                (points[2], points[1]),
                (points[0], points[2]),
            ]
        } else {
            [
                (points[0], points[1]),
                (points[1], points[2]),
                (points[2], points[0]),
            ]
        };
        let supports = order
            .into_iter()
            .enumerate()
            .map(|(index, (start, end))| {
                support(
                    support_base + u32::try_from(index).expect("small test index"),
                    start,
                    end,
                )
            })
            .collect::<Vec<_>>();
        let support_record_ordinals = supports
            .iter()
            .map(|support| support.record_ordinal)
            .collect::<Vec<_>>();
        ZeroEntitySupportRun {
            carrier_pos: 100,
            carrier_record_ordinal: face_ordinal,
            face: Some(super::super::records::ZeroEntityFace {
                pos: face_ordinal as usize,
                record_ordinal: face_ordinal,
                tag: [0x5f, 0x0c],
                allocations: vec![10, 3],
                loop_terminals: vec![7],
                loops: vec![super::super::records::ZeroEntityLoop {
                    pos: face_ordinal as usize + 1,
                    record_ordinal: face_ordinal + 100,
                    tag: [0x62, 0x14],
                    member_ids: vec![6, 5, 4],
                    typed_references: vec![1, 2, 3],
                    support_record_ordinals,
                    terminal_id: 7,
                    gap: 1,
                    loop_class: 0x41,
                    forward_senses: vec![true, true, true],
                    oriented_model_endpoints: order
                        .into_iter()
                        .map(|(start, end)| [start, end])
                        .collect(),
                }],
                terminal_control: 0x05,
            }),
            supports,
        }
    }

    #[test]
    fn complete_radial_pairs_lower_to_connected_face_topology() {
        let points = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ];
        let runs = vec![run(10, 1, points, false), run(11, 4, points, true)];
        let curve_ids = runs
            .iter()
            .flat_map(|run| run.supports.iter())
            .map(|support| {
                (
                    support.record_ordinal,
                    CurveId::mint(format!("catia:test:curve#{}", support.record_ordinal))
                        .expect("identity grammar"),
                )
            })
            .collect::<HashMap<_, _>>();
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint("catia:test:surface#0").expect("identity grammar"),
            geometry: SurfaceGeometry::Plane {
                origin: points[0],
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
        for run in &runs {
            for support in &run.supports {
                let [start, end] = support.model_endpoints.expect("test endpoints");
                ir.model.curves.push(Curve {
                    id: curve_ids[&support.record_ordinal].clone(),
                    geometry: CurveGeometry::Line {
                        origin: start,
                        direction: end
                            .vector_from(start)
                            .unit()
                            .expect("non-degenerate test edge"),
                    },
                    source_object: None,
                });
            }
        }
        let mut no_root_ir = ir.clone();
        let mut no_root_annotations = AnnotationBuilder::new();
        let topology_budget =
            WorkBudget::new(super::super::topology::MAX_ZERO_ENTITY_TOPOLOGY_OPERATIONS);
        let no_root_counts = transfer_closed_face_topology(
            &mut no_root_ir,
            &mut no_root_annotations,
            &runs,
            &HashMap::from([(
                100,
                SurfaceId::mint("catia:test:surface#0").expect("identity grammar"),
            )]),
            &curve_ids,
            None,
            &topology_budget,
        )
        .expect("complete topology without native ownership root");
        assert_eq!(no_root_counts.faces, 2);
        assert_eq!(no_root_ir.model.bodies[0].kind, BodyKind::Solid);
        assert!(crate::assemble::neutral_model_is_admissible(
            &mut no_root_ir,
            &[]
        ));
        let mut annotations = AnnotationBuilder::new();
        let root = ZeroEntityOwnershipRoot {
            face_roster_pos: 1,
            face_roster_record_ordinal: 20,
            face_slots: vec![2, 1],
            shell_pos: 2,
            shell_record_ordinal: 21,
            body_pos: 3,
            body_record_ordinal: 22,
        };
        let counts = transfer_closed_face_topology(
            &mut ir,
            &mut annotations,
            &runs,
            &HashMap::from([(
                100,
                SurfaceId::mint("catia:test:surface#0").expect("identity grammar"),
            )]),
            &curve_ids,
            Some(&root),
            &topology_budget,
        )
        .expect("complete topology");
        assert_eq!(counts.faces, 2);
        assert_eq!(counts.edges, 3);
        assert_eq!(counts.vertices, 3);
        assert_eq!(counts.coedges, 6);
        assert_eq!(ir.model.bodies[0].kind, BodyKind::Solid);
        assert_eq!(ir.model.shells[0].faces.len(), 2);
        assert!(ir.model.coedges.iter().all(|coedge| {
            ir.model
                .coedges
                .iter()
                .any(|candidate| candidate.id == coedge.radial_next)
        }));
        assert!(crate::assemble::neutral_model_is_admissible(&mut ir, &[]));
    }

    #[test]
    fn unsupported_curve_reversal_keeps_topology_with_unknown_carrier() {
        let points = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ];
        let mut runs = vec![run(10, 1, points, false), run(11, 4, points, true)];
        runs[1].supports[1].model_parameters = Some([0.0, std::f64::consts::FRAC_PI_2]);
        let curve_ids = runs
            .iter()
            .flat_map(|run| run.supports.iter())
            .map(|support| {
                (
                    support.record_ordinal,
                    CurveId::mint(format!("catia:test:curve#{}", support.record_ordinal))
                        .expect("identity grammar"),
                )
            })
            .collect::<HashMap<_, _>>();
        let mut ir = CadIr::empty();
        ir.model.surfaces.push(Surface {
            id: SurfaceId::mint("catia:test:surface#0").expect("identity grammar"),
            geometry: SurfaceGeometry::Plane {
                origin: points[0],
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
        for run in &runs {
            for support in &run.supports {
                let [start, end] = support.model_endpoints.expect("test endpoints");
                let geometry = if support.record_ordinal == 5 {
                    CurveGeometry::Ellipse {
                        center: points[0],
                        axis: Vector3::new(0.0, 0.0, 1.0),
                        major_direction: Vector3::new(1.0, 0.0, 0.0),
                        major_radius: 1.0,
                        minor_radius: 1.0,
                    }
                } else {
                    CurveGeometry::Line {
                        origin: start,
                        direction: end
                            .vector_from(start)
                            .unit()
                            .expect("non-degenerate test edge"),
                    }
                };
                ir.model.curves.push(Curve {
                    id: curve_ids[&support.record_ordinal].clone(),
                    geometry,
                    source_object: None,
                });
            }
        }
        let mut annotations = AnnotationBuilder::new();
        let budget = WorkBudget::new(super::super::topology::MAX_ZERO_ENTITY_TOPOLOGY_OPERATIONS);
        let counts = transfer_closed_face_topology(
            &mut ir,
            &mut annotations,
            &runs,
            &HashMap::from([(
                100,
                SurfaceId::mint("catia:test:surface#0").expect("identity grammar"),
            )]),
            &curve_ids,
            None,
            &budget,
        )
        .expect("topology remains transferable");

        assert_eq!(counts.edges, 3);
        assert!(matches!(
            ir.model
                .curves
                .iter()
                .find(|curve| curve.id == curve_ids[&5])
                .expect("reversed carrier")
                .geometry,
            CurveGeometry::Unknown { .. }
        ));
        assert!(crate::assemble::neutral_model_is_admissible(&mut ir, &[]));
    }
}
