// SPDX-License-Identifier: Apache-2.0
//! Zero-entity decode route: analytic surface carriers and reconstructed B-rep.

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, Pcurve, ProceduralCurve,
    ProceduralCurveDefinition, Surface,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    RegionId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;
use std::collections::{BTreeMap, HashMap};

use crate::assemble::{
    annotate, attach_free_vertices, insert_unresolved_carrier_loss, link_payload_carriers,
    neutral_model_is_admissible, preserve_raw_payload, source_meta,
};
use crate::container::{self, ContainerScan};
use crate::families::FamilyOutput;
use crate::solve::UnionFind;

pub(crate) fn try_decode_zero_entity(scan: &ContainerScan) -> Option<FamilyOutput> {
    let decoded = crate::families::zero_entity::records::zero_entity_surfaces(&scan.data);
    let points = crate::families::zero_entity::graph::unframed_vertices(&scan.data);
    let topology = crate::families::zero_entity::graph::parse(&scan.data);
    if decoded.is_empty() && points.is_empty() && topology.is_none() {
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
    let mut topology_ir = ir.clone();
    let mut topology_annotations = annotations.clone();
    let topology_transferred = topology.as_ref().is_some_and(|topology| {
        transfer_zero_entity_topology(&mut topology_ir, &mut topology_annotations, topology)
            && neutral_model_is_admissible(&topology_ir, &unknowns)
    });
    if topology_transferred {
        ir = topology_ir;
        annotations = topology_annotations;
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
        let mut losses = Vec::new();
        insert_unresolved_carrier_loss(&ir, &mut losses);
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
        return Some(FamilyOutput {
            ir,
            report: DecodeReport {
                format: "catia".to_string(),
                container_only: false,
                geometry_transferred: true,
                coverage: std::collections::BTreeMap::new(),
                losses,
                notes: container::summarize(scan).notes,
            },
            annotations: annotations.build(),
            unknowns,
        });
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
            source_object: None,
        });
        let vertex_id = VertexId(format!("catia:zero-entity:v#{index}"));
        annotate(
            &mut annotations,
            &vertex_id,
            "zero_entity_a9_03",
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
    if !ir.model.vertices.is_empty() {
        attach_free_vertices(
            &mut ir,
            &mut annotations,
            "zero-entity",
            "zero_entity_a9_03",
        );
    }
    link_payload_carriers(&ir, &mut unknowns, &mut annotations);
    let annotations = annotations.build();
    let summary = container::summarize(scan);
    let report = DecodeReport {
        format: "catia".to_string(),
        container_only: false,
        geometry_transferred: true,
        coverage: std::collections::BTreeMap::new(),
        losses: vec![LossNote {
            category: LossCategory::Topology,
            severity: Severity::Blocking,
            message: "Zero-entity analytic surface carriers were decoded, but the face/loop/coedge/edge/vertex graph is not yet transferred."
                .to_string(),
            provenance: None,
        }],
        notes: summary.notes,
    };
    Some(FamilyOutput {
        ir,
        report,
        annotations,
        unknowns,
    })
}

/// Intermediate tables derived from a zero-entity topology before IR emission.
///
/// Built once by [`plan_zero_entity_topology`] from the parsed topology; each
/// field crosses into one or more of the per-layer emit passes below. `None`
/// from the planner means the graph failed admission and nothing is emitted.
struct ZeroEntityPlan {
    loop_owner: Vec<usize>,
    edges: Vec<crate::families::zero_entity::graph::ZeroResolvedEdge>,
    occurrence_edges: HashMap<(usize, usize), (usize, usize)>,
    face_components: Vec<usize>,
    component_count: usize,
    points: Vec<Point3>,
    edge_vertices: Vec<[usize; 2]>,
    pcurve_ranges: Vec<Option<[f64; 2]>>,
    face_senses: Vec<Sense>,
}

pub(crate) fn transfer_zero_entity_topology(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    topology: &crate::families::zero_entity::graph::ZeroEntityTopology,
) -> bool {
    let Some(plan) = plan_zero_entity_topology(topology) else {
        return false;
    };
    let ZeroEntityPlan {
        loop_owner,
        edges,
        occurrence_edges,
        face_components,
        component_count,
        points,
        edge_vertices,
        pcurve_ranges,
        face_senses,
    } = plan;
    emit_zero_entity_vertices(ir, annotations, points);
    emit_zero_entity_surfaces(ir, annotations, topology);
    emit_zero_entity_pcurves(ir, annotations, topology, &pcurve_ranges);
    emit_zero_entity_bodies(ir, annotations, component_count, &face_components);
    emit_zero_entity_edges(ir, annotations, topology, &edges, &edge_vertices);
    emit_zero_entity_faces(ir, annotations, topology, &face_senses, &face_components);
    emit_zero_entity_loops_coedges(
        ir,
        annotations,
        topology,
        &edges,
        &occurrence_edges,
        &loop_owner,
    );
    true
}

/// Validates the zero-entity graph and builds the derived tables, or returns
/// `None` when any admission check fails.
#[allow(clippy::question_mark)]
fn plan_zero_entity_topology(
    topology: &crate::families::zero_entity::graph::ZeroEntityTopology,
) -> Option<ZeroEntityPlan> {
    const TOLERANCE: f64 = 2e-3;
    let Some(loop_owner) = unique_index_owners(
        &topology
            .faces
            .iter()
            .map(|face| face.loop_indices.clone())
            .collect::<Vec<_>>(),
        topology.loops.len(),
    ) else {
        return None;
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
        return None;
    }
    let edges = crate::families::zero_entity::graph::resolve_occurrence_edges(topology);
    if edges.len() != topology.physical_edges.len()
        || topology.faces.len() != topology.carrier_runs.len()
        || topology
            .carrier_runs
            .iter()
            .any(|run| run.geometry.is_none())
    {
        return None;
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
                return None;
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
        return None;
    }
    let mut face_parents = UnionFind::new(topology.faces.len());
    for edge in &edges {
        let left = loop_owner[edge.occurrences[0].loop_index];
        let right = loop_owner[edge.occurrences[1].loop_index];
        face_parents.union(left, right);
    }
    let mut component_by_root = BTreeMap::<usize, usize>::new();
    let mut face_components = Vec::with_capacity(topology.faces.len());
    for face_index in 0..topology.faces.len() {
        let root = face_parents.find(face_index);
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
                    (point.distance(*existing) <= TOLERANCE).then_some(index)
                })
                .collect();
            pair[slot] = match matches.as_slice() {
                [index] => *index,
                [] => {
                    points.push(point);
                    points.len() - 1
                }
                _ => return None,
            };
        }
        if pair[0] == pair[1] {
            return None;
        }
        edge_vertices.push(pair);
    }
    if points.len() != topology.vertices.len() {
        return None;
    }
    let Some(pcurve_ranges) = topology
        .supports
        .iter()
        .map(|support| match support.pcurve.as_ref() {
            Some(geometry) => Some(Some(
                crate::families::zero_entity::graph::pcurve_parameter_range(
                    geometry,
                    support.uv_endpoints,
                )?,
            )),
            None => Some(None),
        })
        .collect::<Option<Vec<_>>>()
    else {
        return None;
    };
    let Some(face_senses) = topology
        .faces
        .iter()
        .map(|face| {
            let outer_classes = face.loop_indices.iter().filter_map(|index| {
                let loop_ = &topology.loops[*index];
                (!loop_.inner).then_some(loop_.loop_class)
            });
            match outer_classes.collect::<Vec<_>>().as_slice() {
                [0x41] => Some(Sense::Forward),
                [0xc1] => Some(Sense::Reversed),
                _ => None,
            }
        })
        .collect::<Option<Vec<_>>>()
    else {
        return None;
    };
    Some(ZeroEntityPlan {
        loop_owner,
        edges,
        occurrence_edges,
        face_components,
        component_count,
        points,
        edge_vertices,
        pcurve_ranges,
        face_senses,
    })
}

/// Emits the vertex and point IR layer.
fn emit_zero_entity_vertices(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    points: Vec<Point3>,
) {
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
            source_object: None,
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
}

/// Emits the surface carrier IR layer.
fn emit_zero_entity_surfaces(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    topology: &crate::families::zero_entity::graph::ZeroEntityTopology,
) {
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
}

/// Emits the support pcurve IR layer.
fn emit_zero_entity_pcurves(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    topology: &crate::families::zero_entity::graph::ZeroEntityTopology,
    pcurve_ranges: &[Option<[f64; 2]>],
) {
    for (support_index, support) in topology.supports.iter().enumerate() {
        let Some(geometry) = support.pcurve.clone() else {
            continue;
        };
        let parameter_range =
            pcurve_ranges[support_index].expect("every emitted pcurve passed topology admission");
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
}

/// Emits the body/region/shell IR layer.
fn emit_zero_entity_bodies(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    component_count: usize,
    face_components: &[usize],
) {
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
}

/// Emits the edge and curve IR layer.
fn emit_zero_entity_edges(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    topology: &crate::families::zero_entity::graph::ZeroEntityTopology,
    edges: &[crate::families::zero_entity::graph::ZeroResolvedEdge],
    edge_vertices: &[[usize; 2]],
) {
    for (edge_index, pair) in edge_vertices.iter().enumerate() {
        let id = EdgeId(format!("catia:zero-entity:edge#{edge_index}"));
        let direct = edges[edge_index].occurrences.iter().find_map(|occurrence| {
            crate::families::zero_entity::graph::direct_support_curve(
                topology,
                *occurrence,
                edges[edge_index].endpoints,
            )
        });
        let direct_range = direct.as_ref().and_then(|curve| curve.parameter_range);
        let direct_tolerance = direct.as_ref().and_then(|curve| curve.cache_fit_tolerance);
        let direct_construction = direct.as_ref().and_then(|curve| curve.construction.clone());
        let intersection = direct
            .is_none()
            .then(|| {
                crate::families::zero_entity::graph::intersection_curve(
                    topology,
                    &edges[edge_index],
                )
            })
            .flatten();
        let intersection_range = intersection
            .as_ref()
            .map(|intersection| intersection.parameter_range);
        let intersection_tolerance = intersection
            .as_ref()
            .map(|intersection| intersection.fit_tolerance);
        let geometry = direct.map(|curve| curve.geometry).or_else(|| {
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
                if direct_construction.is_some() {
                    "support_pcurve_construction_cache"
                } else if intersection.is_some() {
                    "radial_surface_intersection_cache"
                } else {
                    "support_pcurve_lift"
                },
                Exactness::Derived,
            );
            annotations.derived(&curve_id, "geometry");
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry,
                source_object: None,
            });
            if let Some(definition) = direct_construction {
                let procedural_id =
                    ProceduralCurveId(format!("catia:zero-entity:helix#{edge_index}"));
                annotate(
                    annotations,
                    &procedural_id,
                    "zero_entity_a9_03",
                    0,
                    "support_pcurve_helix",
                    Exactness::Derived,
                );
                annotations
                    .derived(&procedural_id, "curve")
                    .derived(&procedural_id, "definition")
                    .derived(&procedural_id, "cache_fit_tolerance");
                ir.model.procedural_curves.push(ProceduralCurve {
                    id: procedural_id,
                    curve: curve_id.clone(),
                    definition,
                    cache_fit_tolerance: direct_tolerance,
                });
            } else if let Some(intersection) = intersection {
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
                        pcurve_parameter_range: None,
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
        let edge_range = direct_range.or(intersection_range);
        if edge_range.is_some() {
            annotations.derived(&id, "param_range");
        }
        let edge_tolerance = direct_tolerance.or(intersection_tolerance);
        if edge_tolerance.is_some() {
            annotations.derived(&id, "tolerance");
        }
        ir.model.edges.push(Edge {
            id,
            curve,
            start: VertexId(format!("catia:zero-entity:v#{}", pair[0])),
            end: VertexId(format!("catia:zero-entity:v#{}", pair[1])),
            param_range: edge_range,
            tolerance: edge_tolerance,
        });
    }
}

/// Emits the face IR layer.
fn emit_zero_entity_faces(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    topology: &crate::families::zero_entity::graph::ZeroEntityTopology,
    face_senses: &[Sense],
    face_components: &[usize],
) {
    for (face_index, face) in topology.faces.iter().enumerate() {
        let id = FaceId(format!("catia:zero-entity:face#{face_index}"));
        let carrier = face.carrier_run.unwrap_or(face_index);
        let sense = face_senses[face_index];
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
}

/// Emits the loop and coedge IR layer.
fn emit_zero_entity_loops_coedges(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    topology: &crate::families::zero_entity::graph::ZeroEntityTopology,
    edges: &[crate::families::zero_entity::graph::ZeroResolvedEdge],
    occurrence_edges: &HashMap<(usize, usize), (usize, usize)>,
    loop_owner: &[usize],
) {
    const TOLERANCE: f64 = 2e-3;
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
            boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
            coedges: coedges.clone(),
            vertex_uses: Vec::new(),
        });
        for member in 0..coedges.len() {
            let (edge_index, side) = occurrence_edges[&(loop_index, member)];
            let edge = &edges[edge_index];
            let occurrence = edge.occurrence_endpoints[side];
            let reversed = Point3::new(occurrence[0][0], occurrence[0][1], occurrence[0][2])
                .distance(Point3::new(
                    edge.endpoints[1][0],
                    edge.endpoints[1][1],
                    edge.endpoints[1][2],
                ))
                <= TOLERANCE;
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
                annotations.derived(&coedges[member], "pcurves");
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
                pcurves: pcurve
                    .map(|pcurve| cadmpeg_ir::topology::PcurveUse {
                        pcurve,
                        isoparametric: None,
                    })
                    .into_iter()
                    .collect(),
            });
        }
    }
}

pub(crate) fn unique_index_owners(
    groups: &[Vec<usize>],
    member_count: usize,
) -> Option<Vec<usize>> {
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

#[cfg(test)]
mod route_tests {

    use crate::families::zero_entity::decode::unique_index_owners;

    use crate::families::zero_entity::graph::pcurve_parameter_range;

    use cadmpeg_ir::geometry::PcurveGeometry;

    use cadmpeg_ir::math::Point2;

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
    fn zero_entity_line_pcurve_range_uses_stored_uv_endpoints() {
        let geometry = PcurveGeometry::Line {
            origin: Point2::new(2.0, -1.0),
            direction: Point2::new(3.0, 4.0),
        };

        assert_eq!(
            pcurve_parameter_range(&geometry, Some([[5.0, 3.0], [-4.0, -9.0]])),
            Some([1.0, -2.0])
        );
    }

    #[test]
    fn zero_entity_line_pcurve_range_rejects_off_line_endpoints() {
        let geometry = PcurveGeometry::Line {
            origin: Point2::new(2.0, -1.0),
            direction: Point2::new(3.0, 4.0),
        };

        assert_eq!(
            pcurve_parameter_range(&geometry, Some([[5.0, 3.0], [-4.0, -8.0]])),
            None
        );
        assert_eq!(pcurve_parameter_range(&geometry, None), None);
        assert_eq!(
            pcurve_parameter_range(
                &PcurveGeometry::Line {
                    origin: Point2::new(0.0, 0.0),
                    direction: Point2::new(0.0, 0.0),
                },
                Some([[0.0, 0.0], [1.0, 0.0]])
            ),
            None
        );
    }
}
