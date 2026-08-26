// SPDX-License-Identifier: Apache-2.0
//! Topology emission, unresolved carriers, and source metadata.

use super::offset::point_distance;
use super::pcurves::{
    attach_tolerant_edge_intersections, complete_exact_boundary_intersection_pcurves,
    complete_intersection_pcurves_from_coedge_incidence,
    complete_intersection_pcurves_from_opposite_charts,
    complete_intersection_supports_from_edge_incidence,
    complete_tolerant_intersection_pcurves_from_serialized_branches, ordered_parameter_range,
    pcurve_matches_edge, pcurve_matches_edge_range_with_index, pcurve_parameter_range,
};
use super::{jpeg_dimensions, offset_store_control_counts, Scan, MISSING_TOLERANCE};
use crate::parasolid::{Stream, StreamKind};
use crate::topology::{Graph, Node};
use cadmpeg_core::bytes::assemble_u32_be;
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::eval::curve_point;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, Pcurve, ProceduralCurve,
    ProceduralCurveDefinition, Surface, SurfaceCurveFamily, SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell, Vertex};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness};
use std::collections::{BTreeMap, BTreeSet};

const EPS_EMIT_CANONICAL_TRIM_RANGE_E6: f64 = 1.0e-6;

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_topology(
    ir: &mut CadIr,
    stream_index: usize,
    graph: &Graph,
    points: &BTreeMap<u32, PointId>,
    surfaces: &BTreeMap<u32, SurfaceId>,
    curves: &BTreeMap<u32, CurveId>,
    pcurves: &BTreeMap<u32, PcurveId>,
    pcurve_supports: &BTreeMap<u32, SurfaceId>,
    trim_ranges: &BTreeMap<u32, [f64; 2]>,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    annotations: &mut AnnotationBuilder,
) {
    let prefix = format!("nx:s{stream_index}");
    let body_shape_shells = graph.body_shape_shells();
    let valid_face_xmts: BTreeSet<u32> = body_shape_shells
        .iter()
        .filter_map(|shell| graph.shell_face_xmts(shell))
        .flatten()
        .collect();
    let valid_loop_rings: BTreeMap<u32, Vec<u32>> = valid_face_xmts
        .iter()
        .filter_map(|face_xmt| graph.face_loop_rings(*face_xmt))
        .flatten()
        .collect();
    let valid_fin_xmts: BTreeSet<u32> = valid_loop_rings
        .values()
        .flat_map(|ring| ring.iter().copied())
        .collect();
    let valid_edge_xmts: BTreeSet<u32> = valid_fin_xmts
        .iter()
        .filter_map(|xmt| graph.get(17, *xmt)?.fin_fields().map(|fields| fields.edge))
        .collect();
    let valid_vertex_xmts: BTreeSet<u32> = valid_fin_xmts
        .iter()
        .flat_map(|xmt| {
            let fields = graph.get(17, *xmt).and_then(Node::fin_fields);
            let partner_vertex = fields
                .filter(|fields| fields.other > 1)
                .and_then(|fields| graph.get(17, fields.other))
                .and_then(Node::fin_fields)
                .map(|fields| fields.vertex);
            [fields.map(|fields| fields.vertex), partner_vertex]
                .into_iter()
                .flatten()
        })
        .filter(|xmt| *xmt > 1)
        .collect();
    let body_xmts: BTreeSet<_> = body_shape_shells
        .iter()
        .filter_map(|shell| shell.shell_fields().map(|fields| fields.body))
        .collect();
    let mut bodies = BTreeMap::new();
    for body_xmt in body_xmts {
        let id = BodyId(format!("{prefix}:body#{body_xmt}"));
        if let Some(node) = graph.get(12, body_xmt) {
            annotate_node(annotations, &id, source_stream, node, "BODY");
        } else if let Some(shell) = body_shape_shells.iter().find(|shell| {
            shell
                .shell_fields()
                .is_some_and(|fields| fields.body == body_xmt)
        }) {
            annotations
                .note(&id, source_stream, shell.pos as u64)
                .tag("UNRESOLVED_BODY_REFERENCE");
            annotations.exactness(&id, Exactness::Unknown);
        }
        bodies.insert(body_xmt, id.clone());
        ir.model.bodies.push(Body {
            id,
            kind: cadmpeg_ir::topology::BodyKind::Solid,
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
    }

    let mut regions: BTreeMap<u32, (RegionId, BodyId)> = BTreeMap::new();
    let mut shells = BTreeMap::new();
    for node in body_shape_shells {
        let Some(fields) = node.shell_fields() else {
            continue;
        };
        let Some(body) = bodies.get(&fields.body).cloned() else {
            continue;
        };
        let region_id = if let Some((region, owner)) = regions.get(&fields.region) {
            if owner != &body {
                continue;
            }
            region.clone()
        } else {
            let region = RegionId(format!("{prefix}:region#{}", fields.region));
            if let Some(region_node) = graph.get(19, fields.region) {
                annotate_node(annotations, &region, source_stream, region_node, "REGION");
            } else {
                annotations
                    .note(&region, source_stream, node.pos as u64)
                    .tag("UNRESOLVED_REGION_REFERENCE");
                annotations.exactness(&region, Exactness::Unknown);
            }
            annotations.derived(&region, "body");
            ir.model.regions.push(Region {
                id: region.clone(),
                body: body.clone(),
                shells: Vec::new(),
            });
            if let Some(parent) = ir
                .model
                .bodies
                .iter_mut()
                .find(|candidate| candidate.id == body)
            {
                parent.regions.push(region.clone());
            }
            regions.insert(fields.region, (region.clone(), body.clone()));
            region
        };
        let shell_id = ShellId(format!("{prefix}:shell#{}", node.xmt));
        annotate_node(annotations, &shell_id, source_stream, node, "SHELL");
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id.clone(),
            faces: Vec::new(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        if let Some(parent) = ir
            .model
            .regions
            .iter_mut()
            .find(|candidate| candidate.id == region_id)
        {
            parent.shells.push(shell_id.clone());
        }
        shells.insert(node.xmt, shell_id);
    }

    let mut vertices = BTreeMap::new();
    for node in graph
        .of_kind(18)
        .filter(|node| valid_vertex_xmts.contains(&node.xmt))
    {
        let Some(fields) = node.vertex_fields() else {
            continue;
        };
        let Some(point) = points.get(&fields.point).cloned() else {
            continue;
        };
        let tolerance = decoded_tolerance(fields.tolerance);
        let vertex = VertexId(format!("{prefix}:vertex#{}", node.xmt));
        annotate_node(annotations, &vertex, source_stream, node, "VERTEX");
        if tolerance.is_some() {
            annotations.derived(&vertex, "tolerance");
        }
        ir.model.vertices.push(Vertex {
            id: vertex.clone(),
            point,
            tolerance,
        });
        vertices.insert(node.xmt, vertex.clone());
    }

    let mut edges = BTreeMap::new();
    for node in graph
        .of_kind(16)
        .filter(|node| valid_edge_xmts.contains(&node.xmt))
    {
        let Some(fields) = node.edge_fields() else {
            continue;
        };
        let Some(fin) = graph.get(17, fields.fin) else {
            continue;
        };
        let Some(fin_fields) = fin.fin_fields() else {
            continue;
        };
        let curve_xmt = [fields.curve, fin_fields.curve_xmt]
            .into_iter()
            .find(|xmt| *xmt > 1);
        let mut curve = curve_xmt.and_then(|xmt| curves.get(&xmt)).cloned();
        let mut param_range = curve_xmt.and_then(|xmt| trim_ranges.get(&xmt)).copied();
        if curve.is_none() {
            let lifted = curve_xmt
                .and_then(|xmt| pcurves.get(&xmt))
                .and_then(|pcurve_id| {
                    let pcurve = ir
                        .model
                        .pcurves
                        .iter()
                        .find(|pcurve| &pcurve.id == pcurve_id)?;
                    let surface = pcurve_supports.get(&curve_xmt?)?.clone();
                    let parameter_range = pcurve
                        .parameter_range
                        .or(param_range)
                        .or_else(|| pcurve_parameter_range(&pcurve.geometry))?;
                    let parameter_range = ordered_parameter_range(parameter_range)?;
                    Some((
                        surface,
                        pcurve.geometry.clone(),
                        parameter_range,
                        pcurve.fit_tolerance,
                    ))
                });
            if let Some((surface, pcurve, parameter_range, _fit_tolerance)) = lifted {
                let carrier = CurveId(format!("{prefix}:edge-parametric-curve#{}", node.xmt));
                let construction = ProceduralCurveId(format!(
                    "{prefix}:edge-parametric-construction#{}",
                    node.xmt
                ));
                annotations
                    .note(&carrier, source_stream, node.pos as u64)
                    .tag("PARAMETRIC_SURFACE_CURVE");
                annotations.derived(&carrier, "geometry");
                ir.model.curves.push(Curve {
                    id: carrier.clone(),
                    geometry: CurveGeometry::Procedural {
                        construction: construction.clone(),
                    },
                    source_object: None,
                });
                ir.model.procedural_curves.push(ProceduralCurve {
                    id: construction,
                    curve: carrier.clone(),
                    definition: ProceduralCurveDefinition::SurfaceCurve {
                        family: SurfaceCurveFamily::Parametric,
                        context: IntcurveSupportContext {
                            sides: [
                                IntcurveSupportSide {
                                    surface: Some(surface),
                                    pcurve: Some(pcurve),
                                    pcurve_parameter_range: None,
                                },
                                IntcurveSupportSide {
                                    surface: None,
                                    pcurve: None,
                                    pcurve_parameter_range: None,
                                },
                            ],
                            parameter_range,
                            discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                        },
                        tail: None,
                    },
                    // The pcurve carries this fit contract; this construction has no
                    // independent solved 3D cache to qualify.
                    cache_fit_tolerance: None,
                });
                curve = Some(carrier);
                param_range = None;
            }
        }
        let start = vertices.get(&fin_fields.vertex).cloned().or_else(|| {
            (fin_fields.vertex == 1
                && fin_fields.forward == fin.xmt
                && fin_fields.backward == fin.xmt)
                .then(|| {
                    synthesize_closed_edge_vertex(
                        ir,
                        annotations,
                        &prefix,
                        node,
                        curve.as_ref()?,
                        param_range,
                        source_stream,
                        decoded_tolerance(fields.tolerance),
                    )
                })
                .flatten()
        });
        let Some(start) = start else {
            continue;
        };
        let end_fin = if fin_fields.other > 1 {
            fin_fields.other
        } else {
            fin_fields.forward
        };
        let Some(end_fields) = graph.get(17, end_fin).and_then(Node::fin_fields) else {
            continue;
        };
        let end = vertices.get(&end_fields.vertex).cloned().or_else(|| {
            // A partnered closed FIN repeats the null vertex and closes its own
            // forward/backward links. Its endpoint is the same analytic point
            // as the current FIN's synthesized start, even when `end_fin` is a
            // distinct radial partner record.
            (end_fields.vertex == 1
                && end_fields.forward == end_fin
                && end_fields.backward == end_fin)
                .then(|| start.clone())
        });
        let Some(end) = end else {
            continue;
        };
        let (mut start, mut end) = (start, end);
        let id = EdgeId(format!("{prefix}:edge#{}", node.xmt));
        annotate_node(annotations, &id, source_stream, node, "EDGE");
        if decoded_tolerance(fields.tolerance).is_some() {
            annotations.derived(&id, "tolerance");
        }
        if let (Some(carrier), Some(range)) = (&curve, param_range) {
            match orient_edge_range(
                ir,
                carrier,
                range,
                &start,
                &end,
                decoded_tolerance(fields.tolerance),
            ) {
                Some((oriented, reverse_edge)) => {
                    param_range = Some(oriented);
                    if reverse_edge {
                        std::mem::swap(&mut start, &mut end);
                    }
                }
                None => {
                    param_range = None;
                }
            }
        }
        ir.model.edges.push(Edge {
            id: id.clone(),
            curve,
            start,
            end,
            param_range,
            tolerance: decoded_tolerance(fields.tolerance),
        });
        edges.insert(node.xmt, id);
    }

    let mut faces = BTreeMap::new();
    for node in graph
        .of_kind(14)
        .filter(|node| valid_face_xmts.contains(&node.xmt))
    {
        let Some(fields) = node.face_fields() else {
            continue;
        };
        let Some(shell) = shells.get(&fields.shell).cloned() else {
            continue;
        };
        let Some(surface) = surfaces.get(&fields.surface).cloned() else {
            continue;
        };
        let id = FaceId(format!("{prefix}:face#{}", node.xmt));
        annotate_node(annotations, &id, source_stream, node, "FACE");
        if decoded_tolerance(fields.tolerance).is_some() {
            annotations.derived(&id, "tolerance");
        }
        ir.model.faces.push(Face {
            id: id.clone(),
            shell: shell.clone(),
            surface,
            sense: sense(Some(fields.sense)),
            loops: Vec::new(),
            name: None,
            color: None,
            tolerance: decoded_tolerance(fields.tolerance),
        });
        if let Some(parent) = ir
            .model
            .shells
            .iter_mut()
            .find(|candidate| candidate.id == shell)
        {
            parent.faces.push(id.clone());
        }
        faces.insert(node.xmt, id);
    }

    let mut loops = BTreeMap::new();
    for &loop_xmt in valid_loop_rings.keys() {
        let ring_resolves = valid_loop_rings[&loop_xmt].iter().all(|fin_xmt| {
            graph
                .get(17, *fin_xmt)
                .and_then(Node::fin_fields)
                .is_some_and(|fields| edges.contains_key(&fields.edge))
        });
        if !ring_resolves {
            continue;
        }
        let Some(node) = graph.get(15, loop_xmt) else {
            continue;
        };
        let Some(fields) = node.loop_fields() else {
            continue;
        };
        let Some(face) = faces.get(&fields.face).cloned() else {
            continue;
        };
        let id = LoopId(format!("{prefix}:loop#{}", node.xmt));
        annotate_node(annotations, &id, source_stream, node, "LOOP");
        ir.model.loops.push(Loop {
            id: id.clone(),
            face: face.clone(),
            boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
            coedges: Vec::new(),
            vertex_uses: Vec::new(),
        });
        if let Some(parent) = ir
            .model
            .faces
            .iter_mut()
            .find(|candidate| candidate.id == face)
        {
            parent.loops.push(id.clone());
        }
        loops.insert(node.xmt, id);
    }

    let fin_ids: BTreeMap<u32, CoedgeId> = valid_fin_xmts
        .iter()
        .filter(|xmt| {
            graph
                .get(17, **xmt)
                .and_then(Node::fin_fields)
                .is_some_and(|fields| loops.contains_key(&fields.loop_xmt))
        })
        .map(|xmt| (*xmt, CoedgeId(format!("{prefix}:fin#{xmt}"))))
        .collect();
    let intersection_pcurves: BTreeMap<_, _> = ir
        .model
        .procedural_curves
        .iter()
        .filter_map(|procedural| {
            let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition
            else {
                return None;
            };
            Some(context.sides.iter().filter_map(move |side| {
                Some((
                    (procedural.curve.clone(), side.surface.clone()?),
                    (
                        side.pcurve.clone()?,
                        context.parameter_range,
                        procedural.cache_fit_tolerance,
                    ),
                ))
            }))
        })
        .flatten()
        .collect();
    let valid_pcurve_fins = {
        let index = cadmpeg_ir::index::ModelIndex::new(ir);
        fin_ids
            .keys()
            .filter_map(|fin_xmt| {
                let fields = graph.get(17, *fin_xmt)?.fin_fields()?;
                let edge = edges.get(&fields.edge)?;
                let support = graph
                    .get(15, fields.loop_xmt)
                    .and_then(Node::loop_fields)
                    .and_then(|loop_| graph.get(14, loop_.face))
                    .and_then(Node::face_fields)
                    .and_then(|face| surfaces.get(&face.surface))?;
                let carrier = pcurves
                    .get(&fields.curve_xmt)
                    .and_then(|id| ir.model.pcurves.iter().find(|carrier| &carrier.id == id))?;
                let use_range = trim_ranges
                    .get(&fields.curve_xmt)
                    .copied()
                    .and_then(ordered_parameter_range);
                pcurve_matches_edge_range_with_index(
                    ir,
                    &index,
                    edge,
                    support,
                    &carrier.geometry,
                    use_range.or(carrier.parameter_range),
                    carrier.fit_tolerance,
                )
                .then_some(*fin_xmt)
            })
            .collect::<BTreeSet<_>>()
    };
    let mut serialized_branch_pcurves = BTreeSet::new();
    for &fin_xmt in fin_ids.keys() {
        let Some(node) = graph.get(17, fin_xmt) else {
            continue;
        };
        let Some(fields) = node.fin_fields() else {
            continue;
        };
        let Some(loop_id) = loops.get(&fields.loop_xmt).cloned() else {
            continue;
        };
        let Some(edge) = edges.get(&fields.edge).cloned() else {
            continue;
        };
        let id = fin_ids.get(&node.xmt).cloned().expect("filtered above");
        annotate_node(annotations, &id, source_stream, node, "FIN");
        let next = fin_ids
            .get(&fields.forward)
            .cloned()
            .expect("validated FIN ring resolves forward link");
        let previous = fin_ids
            .get(&fields.backward)
            .cloned()
            .expect("validated FIN ring resolves backward link");
        let partner = fin_ids.get(&fields.other).cloned();
        let radial_next = partner.clone().unwrap_or_else(|| id.clone());
        let support = graph
            .get(15, fields.loop_xmt)
            .and_then(Node::loop_fields)
            .and_then(|loop_| graph.get(14, loop_.face))
            .and_then(Node::face_fields)
            .and_then(|face| surfaces.get(&face.surface))
            .cloned();
        let pcurve_use_range = trim_ranges
            .get(&fields.curve_xmt)
            .copied()
            .and_then(ordered_parameter_range);
        let mut pcurve = valid_pcurve_fins
            .contains(&node.xmt)
            .then(|| pcurves.get(&fields.curve_xmt).cloned())
            .flatten();
        let edge_curve = ir
            .model
            .edges
            .iter()
            .find(|candidate| candidate.id == edge)
            .and_then(|edge| edge.curve.as_ref());
        if let (Some(pcurve), Some(edge_curve), Some(support)) =
            (pcurve.as_ref(), edge_curve, support.as_ref())
        {
            if curves.get(&fields.curve_xmt) == Some(edge_curve)
                && pcurve_supports.get(&fields.curve_xmt) == Some(support)
            {
                serialized_branch_pcurves.insert((
                    edge_curve.clone(),
                    support.clone(),
                    pcurve.clone(),
                ));
            }
        }
        let attached_pcurve_use_range = pcurve.as_ref().and(pcurve_use_range);
        if pcurve.is_none() {
            let carrier = ir
                .model
                .edges
                .iter()
                .find(|candidate| candidate.id == edge)
                .and_then(|edge| edge.curve.clone());
            if let Some((_support, geometry, parameter_range, fit_tolerance)) = carrier
                .zip(support)
                .and_then(|key| {
                    intersection_pcurves
                        .get(&key)
                        .cloned()
                        .map(|value| (key.1, value.0, value.1, value.2))
                })
                .filter(|(support, geometry, _, fit_tolerance)| {
                    pcurve_matches_edge(ir, &edge, support, geometry, *fit_tolerance)
                })
            {
                let pcurve_id = PcurveId(format!("{prefix}:intersection-pcurve#{fin_xmt}"));
                annotations
                    .note(&pcurve_id, source_stream, node.pos as u64)
                    .tag("INTERSECTION_PCURVE");
                annotations.derived(&pcurve_id, "geometry");
                annotations.derived(&pcurve_id, "parameter_range");
                if fit_tolerance.is_some() {
                    annotations.derived(&pcurve_id, "fit_tolerance");
                }
                ir.model.pcurves.push(Pcurve {
                    id: pcurve_id.clone(),
                    geometry,
                    wrapper_reversed: None,
                    native_tail_flags: None,
                    parameter_range: Some(parameter_range),
                    fit_tolerance,
                });
                pcurve = Some(pcurve_id);
            }
        }
        ir.model.coedges.push(Coedge {
            id: id.clone(),
            owner_loop: loop_id.clone(),
            edge,
            next,
            previous,
            radial_next,
            sense: sense(Some(fields.sense)),
            pcurves: pcurve
                .into_iter()
                .map(|pcurve| cadmpeg_ir::topology::PcurveUse {
                    pcurve,
                    isoparametric: None,
                    parameter_range: attached_pcurve_use_range,
                })
                .collect(),
            use_curve: None,
            use_curve_parameter_range: None,
        });
        if let Some(parent) = ir
            .model
            .loops
            .iter_mut()
            .find(|candidate| candidate.id == loop_id)
        {
            parent.coedges.push(id);
        }
    }

    attach_tolerant_edge_intersections(ir, graph, &edges, &prefix, source_stream, annotations);
    complete_intersection_supports_from_edge_incidence(ir);
    complete_intersection_pcurves_from_coedge_incidence(ir);
    complete_tolerant_intersection_pcurves_from_serialized_branches(
        ir,
        &serialized_branch_pcurves,
        annotations,
    );
    complete_exact_boundary_intersection_pcurves(ir, annotations);
    complete_intersection_pcurves_from_opposite_charts(ir);

    let owned_edges: BTreeSet<_> = ir
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.edge.clone())
        .collect();
    let candidate_edges: BTreeSet<_> = edges.into_values().collect();
    ir.model
        .edges
        .retain(|edge| !candidate_edges.contains(&edge.id) || owned_edges.contains(&edge.id));
    let retained_vertices: BTreeSet<_> = ir
        .model
        .edges
        .iter()
        .flat_map(|edge| [edge.start.clone(), edge.end.clone()])
        .collect();
    ir.model.vertices.retain(|vertex| {
        !vertex.id.0.starts_with(&prefix) || retained_vertices.contains(&vertex.id)
    });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn retain_unresolved_topology_carriers(
    ir: &mut CadIr,
    stream_index: usize,
    graph: &Graph,
    surfaces: &mut BTreeMap<u32, SurfaceId>,
    curves: &mut BTreeMap<u32, CurveId>,
    pcurves: &BTreeMap<u32, PcurveId>,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    annotations: &mut AnnotationBuilder,
) {
    let unknown = UnknownId(format!("nx:container:parasolid#{stream_index}"));
    for face in graph.of_kind(14) {
        let Some(surface_xmt) = face.face_fields().map(|fields| fields.surface) else {
            continue;
        };
        if surface_xmt <= 1 || surfaces.contains_key(&surface_xmt) {
            continue;
        }
        let id = SurfaceId(format!("nx:s{stream_index}:surface#unknown-{surface_xmt}"));
        annotations
            .note(&id, source_stream, face.pos as u64)
            .tag("UNRESOLVED_SURFACE_REFERENCE");
        annotations.exactness(&id, Exactness::Unknown);
        ir.model.surfaces.push(Surface {
            id: id.clone(),
            geometry: SurfaceGeometry::Unknown {
                record: Some(unknown.clone()),
            },
            source_object: None,
        });
        surfaces.insert(surface_xmt, id);
    }

    for edge in graph.of_kind(16) {
        let Some(curve_xmt) = edge.edge_fields().map(|fields| fields.curve) else {
            continue;
        };
        if curve_xmt <= 1 || curves.contains_key(&curve_xmt) || pcurves.contains_key(&curve_xmt) {
            continue;
        }
        let id = CurveId(format!("nx:s{stream_index}:curve#unknown-{curve_xmt}"));
        annotations
            .note(&id, source_stream, edge.pos as u64)
            .tag("UNRESOLVED_CURVE_REFERENCE");
        annotations.exactness(&id, Exactness::Unknown);
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry: CurveGeometry::Unknown {
                record: Some(unknown.clone()),
            },
            source_object: None,
        });
        curves.insert(curve_xmt, id);
    }
}

pub(crate) fn annotate_node(
    annotations: &mut AnnotationBuilder,
    id: impl std::fmt::Display,
    stream: cadmpeg_ir::annotations::StreamHandle,
    node: &Node,
    tag: &str,
) {
    annotations.note(id, stream, node.pos as u64).tag(tag);
}

pub(crate) fn surface_tag(geometry: &SurfaceGeometry) -> &'static str {
    match geometry {
        SurfaceGeometry::Plane { .. } => "PLANE",
        SurfaceGeometry::Cylinder { .. } => "CYLINDER",
        SurfaceGeometry::Cone { .. } => "CONE",
        SurfaceGeometry::Sphere { .. } => "SPHERE",
        SurfaceGeometry::Torus { .. } => "TORUS",
        SurfaceGeometry::Nurbs(_) => "B_SPLINE_SURFACE",
        SurfaceGeometry::Procedural { .. } => "PROCEDURAL_SURFACE",
        SurfaceGeometry::Polygonal { .. } => "POLYGONAL_SURFACE",
        SurfaceGeometry::Transformed { basis, .. } => surface_tag(basis),
        SurfaceGeometry::Unknown { .. } => "UNKNOWN_SURFACE",
    }
}

pub(crate) fn curve_tag(geometry: &CurveGeometry) -> &'static str {
    match geometry {
        CurveGeometry::Line { .. } => "LINE",
        CurveGeometry::Circle { .. } => "CIRCLE",
        CurveGeometry::Ellipse { .. } => "ELLIPSE",
        CurveGeometry::Parabola { .. } => "PARABOLA",
        CurveGeometry::Hyperbola { .. } => "HYPERBOLA",
        CurveGeometry::Degenerate { .. } => "DEGENERATE_CURVE",
        CurveGeometry::Nurbs(_) => "B_SPLINE_CURVE",
        CurveGeometry::Procedural { .. } => "PROCEDURAL_CURVE",
        CurveGeometry::Composite { .. } => "COMPOSITE_CURVE",
        CurveGeometry::Polyline { .. } => "POLYLINE",
        CurveGeometry::Transformed { basis, .. } => curve_tag(basis),
        CurveGeometry::Unknown { .. } => "UNKNOWN_CURVE",
    }
}

pub(crate) fn decoded_tolerance(value: f64) -> Option<f64> {
    match value {
        MISSING_TOLERANCE => None,
        value if value.is_finite() && value > 0.0 && (value * 1000.0).is_finite() => {
            Some(value * 1000.0)
        }
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn synthesize_closed_edge_vertex(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    prefix: &str,
    edge: &Node,
    curve: &CurveId,
    range: Option<[f64; 2]>,
    source_stream: cadmpeg_ir::annotations::StreamHandle,
    tolerance: Option<f64>,
) -> Option<VertexId> {
    let geometry = &ir
        .model
        .curves
        .iter()
        .find(|candidate| candidate.id == *curve)?
        .geometry;
    let parameter = range.map_or_else(
        || match geometry {
            CurveGeometry::Nurbs(nurbs) => nurbs.knots.first().copied().unwrap_or(0.0),
            _ => 0.0,
        },
        |range| range[0],
    );
    let position = curve_point(geometry, parameter)?;
    let point = PointId(format!("{prefix}:point#closed-edge-{}", edge.xmt));
    let vertex = VertexId(format!("{prefix}:vertex#closed-edge-{}", edge.xmt));
    annotations
        .note(&point, source_stream, edge.pos as u64)
        .tag("CLOSED_EDGE_POINT");
    annotations.exactness(&point, Exactness::Inferred);
    annotations
        .note(&vertex, source_stream, edge.pos as u64)
        .tag("CLOSED_EDGE_VERTEX");
    annotations.exactness(&vertex, Exactness::Inferred);
    ir.model.points.push(Point {
        id: point.clone(),
        position,
        source_object: None,
    });
    ir.model.vertices.push(Vertex {
        id: vertex.clone(),
        point,
        tolerance,
    });
    Some(vertex)
}

pub(crate) fn canonical_trim_range(ir: &CadIr, basis: &CurveId, raw: [f64; 2]) -> Option<[f64; 2]> {
    let curve = ir.model.curves.iter().find(|curve| curve.id == *basis)?;
    match &curve.geometry {
        CurveGeometry::Line { .. } => {
            let range = [raw[0] * 1000.0, raw[1] * 1000.0];
            range.into_iter().all(f64::is_finite).then_some(range)
        }
        CurveGeometry::Nurbs(nurbs) => {
            let domain = [*nurbs.knots.first()?, *nurbs.knots.last()?];
            let epsilon =
                EPS_EMIT_CANONICAL_TRIM_RANGE_E6 * (1.0 + domain[0].abs().max(domain[1].abs()));
            if raw
                .iter()
                .any(|value| *value < domain[0] - epsilon || *value > domain[1] + epsilon)
            {
                None
            } else {
                Some([
                    raw[0].clamp(domain[0], domain[1]),
                    raw[1].clamp(domain[0], domain[1]),
                ])
            }
        }
        _ => Some(raw),
    }
}

pub(crate) fn orient_edge_range(
    ir: &CadIr,
    curve: &CurveId,
    range: [f64; 2],
    start: &VertexId,
    end: &VertexId,
    edge_tolerance: Option<f64>,
) -> Option<([f64; 2], bool)> {
    let geometry = &ir
        .model
        .curves
        .iter()
        .find(|candidate| candidate.id == *curve)?
        .geometry;
    let range = if range[0] <= range[1] {
        range
    } else {
        [range[1], range[0]]
    };
    let range = match geometry {
        CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => {
            let sweep = range[1] - range[0];
            (0.0..=std::f64::consts::TAU)
                .contains(&sweep)
                .then_some(())?;
            let start = range[0].rem_euclid(std::f64::consts::TAU);
            [start, start + sweep]
        }
        _ => range,
    };
    let at = match (
        curve_point(geometry, range[0]),
        curve_point(geometry, range[1]),
    ) {
        (Some(start), Some(end)) => [start, end],
        _ if ir
            .model
            .procedural_curves
            .iter()
            .any(|procedural| procedural.curve == *curve) =>
        {
            return Some((range, false));
        }
        _ => return None,
    };
    let vertex_position = |vertex: &VertexId| {
        let vertex = ir
            .model
            .vertices
            .iter()
            .find(|candidate| candidate.id == *vertex)?;
        let point = ir
            .model
            .points
            .iter()
            .find(|candidate| candidate.id == vertex.point)?;
        Some((point.position, vertex.tolerance))
    };
    let (start_position, start_tolerance) = vertex_position(start)?;
    let (end_position, end_tolerance) = vertex_position(end)?;
    let allowance = [edge_tolerance, start_tolerance, end_tolerance]
        .into_iter()
        .flatten()
        .fold(0.0_f64, f64::max);
    if point_distance(at[0], start_position) <= allowance
        && point_distance(at[1], end_position) <= allowance
    {
        Some((range, false))
    } else if point_distance(at[1], start_position) <= allowance
        && point_distance(at[0], end_position) <= allowance
    {
        Some((range, true))
    } else {
        None
    }
}

pub(crate) fn sense(byte: Option<u8>) -> Sense {
    if byte == Some(b'-') {
        Sense::Reversed
    } else {
        Sense::Forward
    }
}

pub(crate) fn unknown_stream(si: usize, stream: &Stream) -> UnknownRecord {
    UnknownRecord {
        id: UnknownId(format!("nx:container:parasolid#{si}")),
        offset: stream.file_offset as u64,
        byte_len: stream.inflated.len() as u64,
        sha256: sha256_hex(&stream.inflated),
        data: Some(stream.inflated.clone()),
        links: Vec::new(),
    }
}

pub(crate) fn source_meta(scan: &Scan) -> SourceMeta {
    let mut attributes = BTreeMap::new();
    attributes.insert(
        "file_size".to_string(),
        scan.container.data.len().to_string(),
    );
    attributes.insert(
        "footer_offset".to_string(),
        scan.container.footer_offset.to_string(),
    );
    attributes.insert(
        "directory_entries".to_string(),
        scan.container.entries.len().to_string(),
    );
    attributes.insert(
        "header_entry_count".to_string(),
        scan.container.header_entry_count.to_string(),
    );
    attributes.insert(
        "footer_entry_count".to_string(),
        scan.container.footer_entry_count.to_string(),
    );
    attributes.insert(
        "footer_fingerprint".to_string(),
        format!("{:08x}", assemble_u32_be(scan.container.footer_fingerprint)),
    );
    let (control_count, classified_control_count) = offset_store_control_counts(&scan.container);
    if control_count != 0 {
        attributes.insert(
            "offset_store_control_count".to_string(),
            control_count.to_string(),
        );
        attributes.insert(
            "classified_offset_store_control_count".to_string(),
            classified_control_count.to_string(),
        );
        attributes.insert(
            "unclassified_offset_store_control_count".to_string(),
            (control_count - classified_control_count).to_string(),
        );
    }
    attributes.insert(
        "partition_streams".to_string(),
        scan.count(StreamKind::Partition).to_string(),
    );
    attributes.insert(
        "deltas_streams".to_string(),
        scan.count(StreamKind::Deltas).to_string(),
    );
    attributes.insert(
        "plain_streams".to_string(),
        scan.count(StreamKind::Plain).to_string(),
    );
    if let Some(schema) = scan.streams.iter().find_map(|s| s.schema.as_deref()) {
        attributes.insert("parasolid_schema".to_string(), schema.to_string());
    }
    for (index, path) in scan
        .container
        .external_reference_paths()
        .into_iter()
        .enumerate()
    {
        attributes.insert(format!("external_reference.{index}"), path);
    }
    if let Some((_, table)) = scan.container.rmfastload_object_id_table() {
        attributes.insert(
            "rmfastload_active_object_count".to_string(),
            table.object_ids.len().to_string(),
        );
    }
    let mut preview_count = 0usize;
    for entry in scan
        .container
        .entries
        .iter()
        .filter(|entry| entry.name == "/Root/images/preview")
    {
        let Some((offset, size)) = entry.file_span else {
            continue;
        };
        let (Ok(start), Ok(size)) = (usize::try_from(offset), usize::try_from(size)) else {
            continue;
        };
        let Some(payload) = scan.container.data.get(start..start.saturating_add(size)) else {
            continue;
        };
        let Some((width, height, precision, components)) = jpeg_dimensions(payload) else {
            continue;
        };
        let prefix = format!("jpeg_preview_{preview_count}");
        attributes.insert(format!("{prefix}_width"), width.to_string());
        attributes.insert(format!("{prefix}_height"), height.to_string());
        attributes.insert(format!("{prefix}_precision"), precision.to_string());
        attributes.insert(format!("{prefix}_components"), components.to_string());
        attributes.insert(format!("{prefix}_byte_len"), payload.len().to_string());
        attributes.insert(format!("{prefix}_sha256"), sha256_hex(payload));
        preview_count += 1;
    }
    attributes.insert("jpeg_preview_count".to_string(), preview_count.to_string());
    for (index, stream) in scan
        .streams
        .iter()
        .filter(|stream| stream.kind == StreamKind::Deltas)
        .enumerate()
    {
        let census = crate::deltas::walk(&stream.inflated);
        if census.transmit_header.is_some() {
            attributes.insert(format!("deltas.{index}.transmit_headers"), "1".to_string());
        }
        attributes.insert(
            format!("deltas.{index}.grammar"),
            "typed_status_framed_records".to_string(),
        );
        attributes.insert(
            format!("deltas.{index}.bytes_decoded"),
            census.bytes_decoded.to_string(),
        );
        if !census.body_revisions.is_empty() {
            attributes.insert(
                format!("deltas.{index}.body_revisions"),
                census.body_revisions.len().to_string(),
            );
        }
        if !census.term_use_numeric_tails.is_empty() {
            attributes.insert(
                format!("deltas.{index}.term_use_numeric_tails"),
                census.term_use_numeric_tails.len().to_string(),
            );
        }
        if !census.tagged_reference_lanes.is_empty() {
            attributes.insert(
                format!("deltas.{index}.tagged_reference_lanes"),
                census.tagged_reference_lanes.len().to_string(),
            );
        }
        if !census.reference_type_maps.is_empty() {
            attributes.insert(
                format!("deltas.{index}.reference_type_maps"),
                census.reference_type_maps.len().to_string(),
            );
        }
        if !census.reference_state_packets.is_empty() {
            attributes.insert(
                format!("deltas.{index}.reference_state_packets"),
                census.reference_state_packets.len().to_string(),
            );
        }
        if !census.reference_marker_packets.is_empty() {
            attributes.insert(
                format!("deltas.{index}.reference_marker_packets"),
                census.reference_marker_packets.len().to_string(),
            );
        }
        if !census.inline_schema_declarations.is_empty() {
            attributes.insert(
                format!("deltas.{index}.inline_schema_declarations"),
                census.inline_schema_declarations.len().to_string(),
            );
        }
        for (name, count) in census.full_counts {
            attributes.insert(format!("deltas.{index}.full.{name}"), count.to_string());
        }
        for (name, count) in census.tombstone_counts {
            attributes.insert(
                format!("deltas.{index}.tombstone.{name}"),
                count.to_string(),
            );
        }
    }
    SourceMeta {
        format: "nx".to_string(),
        attributes,
    }
}
