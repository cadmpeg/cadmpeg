// SPDX-License-Identifier: Apache-2.0
//! Topology emission, unresolved carriers, and source metadata.

use super::geometry_work::GeometryWorkBudget;
use super::jpeg::jpeg_dimensions;
use super::pcurves::{
    attach_tolerant_edge_intersections_with_budget,
    complete_exact_boundary_intersection_pcurves_with_budget,
    complete_intersection_pcurves_from_opposite_charts_with_budget,
    complete_tolerant_intersection_pcurves_from_serialized_branches_for_stream_with_budget,
    ordered_parameter_range, pcurve_endpoint_witness_with_index_and_budget,
    pcurve_matches_edge_range_with_index_and_budget, pcurve_parameter_range, EndpointWitnesses,
    IntersectionEntityStarts, IntersectionIncidenceIndex, TransferBudget,
};
use super::{offset_store_control_counts, Scan};
use crate::decode::ids::IdScope;
use crate::framing::node_kind::NodeKind;
use crate::parasolid::{Stream, StreamKind};
use crate::topology::{FaceLoopError, FaceLoopFailure, Graph, Node};
use cadmpeg_core::bytes::assemble_u32_be;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::eval::{curve_point_with_budget, finite_or_refusal};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::{
    pcurve::Pcurve, Curve, CurveGeometry, FitTolerance, IntcurveSupportContext,
    IntcurveSupportSide, ProceduralCurve, ProceduralCurveDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceCurveFamily, SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256;
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{
    Body, Coedge, Edge, Face, Loop, LoopRing, Point, Region, Shell, Vertex,
};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display, Write as _};

const EPS_EMIT_CANONICAL_TRIM_RANGE_E6: f64 = 1.0e-6;

type IntersectionPcurveIndex =
    BTreeMap<(CurveId, SurfaceId), (PcurveGeometry, [f64; 2], Option<FitTolerance>)>;

/// A face whose non-loop fields are decoded, held until its loops resolve so
/// the face is constructed once with its complete boundary.
struct PendingFace {
    xmt: u32,
    id: FaceId,
    shell: ShellId,
    surface: SurfaceId,
    sense: cadmpeg_ir::topology::Sense,
    tolerance: Option<cadmpeg_ir::scalar::PositiveReal>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_topology(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    stream_index: usize,
    graph: &Graph,
    points: &BTreeMap<u32, PointId>,
    surfaces: &BTreeMap<u32, SurfaceId>,
    curves: &BTreeMap<u32, CurveId>,
    pcurves: &BTreeMap<u32, PcurveId>,
    pcurve_supports: &BTreeMap<u32, SurfaceId>,
    trim_ranges: &BTreeMap<u32, [f64; 2]>,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
    annotations: &mut AnnotationBuilder,
    intersection_index: &mut IntersectionIncidenceIndex,
    intersection_starts: IntersectionEntityStarts,
    procedural_start: usize,
    exact_transfer_budget: &TransferBudget<'_>,
    completion_transfer_budget: &TransferBudget<'_>,
    adaptive_geometry_budget: &GeometryWorkBudget<'_>,
    completion_geometry_budget: &GeometryWorkBudget<'_>,
    topology_losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<EndpointWitnesses, CodecError> {
    let scope = IdScope::stream_charged(ctx, stream_index)?;
    let mut valid_face_xmts = BTreeSet::new();
    for shell in graph.body_shape_shells() {
        if let Some(faces) = graph.shell_face_xmts(ctx, shell)? {
            for face in faces {
                ctx.charge_collection_items(1, "nx valid topology faces")?;
                valid_face_xmts.insert(face);
            }
        }
    }
    let mut face_loop_rings: BTreeMap<u32, Vec<(u32, Vec<u32>)>> = BTreeMap::new();
    let mut face_loop_failures: BTreeMap<u32, FaceLoopFailure> = BTreeMap::new();
    for face_xmt in &valid_face_xmts {
        match graph.face_loop_rings(ctx, *face_xmt) {
            Ok(rings) => {
                ctx.charge_collection_items(1, "nx face loop ring index")?;
                face_loop_rings.insert(*face_xmt, rings);
            }
            Err(FaceLoopError::Invalid(failure)) => {
                ctx.charge_collection_items(1, "nx face loop failure index")?;
                face_loop_failures.insert(*face_xmt, failure);
            }
            Err(FaceLoopError::Codec(error)) => return Err(error),
        }
    }
    let mut valid_loop_rings: BTreeMap<u32, &[u32]> = BTreeMap::new();
    for rings in face_loop_rings.values() {
        for (loop_xmt, ring) in rings {
            ctx.charge_collection_items(1, "nx valid loop ring index")?;
            valid_loop_rings.insert(*loop_xmt, ring.as_slice());
        }
    }
    let mut valid_fin_xmts = BTreeSet::new();
    for ring in valid_loop_rings.values() {
        for fin in *ring {
            ctx.charge_collection_items(1, "nx valid fin nodes")?;
            valid_fin_xmts.insert(*fin);
        }
    }
    let mut valid_edge_xmts = BTreeSet::new();
    let mut valid_vertex_xmts = BTreeSet::new();
    for xmt in &valid_fin_xmts {
        if let Some(edge) = graph
            .get(NodeKind::Fin, *xmt)
            .and_then(Node::fin_fields)
            .and_then(|fields| fields.edge.map(u32::from))
        {
            ctx.charge_collection_items(1, "nx valid edge nodes")?;
            valid_edge_xmts.insert(edge);
        }
        {
            let fields = graph.get(NodeKind::Fin, *xmt).and_then(Node::fin_fields);
            let partner_vertex = fields
                .filter(|fields| fields.other.is_some_and(|target| u32::from(target) > 1))
                .and_then(|fields| graph.get_target(NodeKind::Fin, fields.other))
                .and_then(Node::fin_fields)
                .and_then(|fields| fields.vertex.map(u32::from));
            for vertex in [
                fields.and_then(|fields| fields.vertex.map(u32::from)),
                partner_vertex,
            ]
            .into_iter()
            .flatten()
            .filter(|vertex| *vertex > 1)
            {
                ctx.charge_collection_items(1, "nx valid vertex nodes")?;
                valid_vertex_xmts.insert(vertex);
            }
        }
    }
    let mut body_xmts = BTreeSet::new();
    for shell in graph.body_shape_shells() {
        if let Some(body) = shell
            .shell_fields()
            .and_then(|fields| fields.body.map(u32::from))
        {
            ctx.charge_collection_items(1, "nx topology body nodes")?;
            body_xmts.insert(body);
        }
    }
    let mut bodies: BTreeMap<u32, BodyId> = BTreeMap::new();
    for body_xmt in body_xmts {
        let id: BodyId =
            scope.id_charged(ctx, &cadmpeg_ir::identity_component!("body"), body_xmt)?;
        if let Some(node) = graph.get(NodeKind::Body, body_xmt) {
            annotate_node(ctx, annotations, id.as_str(), source_stream, node, "BODY")?;
        } else if let Some(shell) = graph.body_shape_shells().find(|shell| {
            shell
                .shell_fields()
                .is_some_and(|fields| fields.body.map(u32::from) == Some(body_xmt))
        }) {
            super::annotations::note(
                ctx,
                annotations,
                id.as_str(),
                source_stream,
                shell.pos as u64,
                "UNRESOLVED_BODY_REFERENCE",
            )?;
            super::annotations::exactness(ctx, annotations, id.as_str(), Exactness::Unknown)?;
        }
        ctx.charge_collection_items(1, "nx emitted body index")?;
        bodies.insert(
            body_xmt,
            id.try_clone_for_decode(ctx, "nx emitted body identity")?,
        );
        ctx.charge_collection_items(1, "nx emitted bodies")?;
        ir.model
            .bodies
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("nx emitted bodies", 0, 1))?;
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
    let mut shells: BTreeMap<u32, ShellId> = BTreeMap::new();
    for node in graph.body_shape_shells() {
        let Some(fields) = node.shell_fields() else {
            continue;
        };
        let Some(body_ref) = fields
            .body
            .and_then(|target| bodies.get(&u32::from(target)))
        else {
            continue;
        };
        let body: BodyId = body_ref.try_clone_for_decode(ctx, "nx shell body identity")?;
        let Some(region_xmt) = fields.region.map(u32::from) else {
            continue;
        };
        let region_id = if let Some((region, owner)) = regions.get(&region_xmt) {
            if owner != &body {
                continue;
            }
            region.try_clone_for_decode(ctx, "nx existing region identity")?
        } else {
            let region: RegionId =
                scope.id_charged(ctx, &cadmpeg_ir::identity_component!("region"), region_xmt)?;
            if let Some(region_node) = graph.get(NodeKind::Region, region_xmt) {
                annotate_node(
                    ctx,
                    annotations,
                    region.as_str(),
                    source_stream,
                    region_node,
                    "REGION",
                )?;
            } else {
                super::annotations::note(
                    ctx,
                    annotations,
                    region.as_str(),
                    source_stream,
                    node.pos as u64,
                    "UNRESOLVED_REGION_REFERENCE",
                )?;
                super::annotations::exactness(
                    ctx,
                    annotations,
                    region.as_str(),
                    Exactness::Unknown,
                )?;
            }
            super::annotations::derived(ctx, annotations, region.as_str(), "body")?;
            ctx.charge_collection_items(1, "nx emitted regions")?;
            ir.model
                .regions
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit("nx emitted regions", 0, 1))?;
            ir.model.regions.push(Region {
                id: region.try_clone_for_decode(ctx, "nx region identity copy")?,
                body: body.try_clone_for_decode(ctx, "nx region body identity")?,
                shells: Vec::new(),
            });
            if let Some(parent) = ir
                .model
                .bodies
                .iter_mut()
                .find(|candidate| candidate.id == body)
            {
                ctx.charge_collection_items(1, "nx body regions")?;
                parent
                    .regions
                    .try_reserve(1)
                    .map_err(|_| ctx.refuse_codec_limit("nx body regions", 0, 1))?;
                parent.regions.push(region.try_clone_for_decode(ctx, "nx body region identity")?);
            }
            ctx.charge_collection_items(1, "nx emitted region index")?;
            regions.insert(
                region_xmt,
                (
                    region.try_clone_for_decode(ctx, "nx indexed region identity")?,
                    body.try_clone_for_decode(ctx, "nx indexed region body identity")?,
                ),
            );
            region
        };
        let shell_id: ShellId =
            scope.id_charged(ctx, &cadmpeg_ir::identity_component!("shell"), node.xmt)?;
        annotate_node(
            ctx,
            annotations,
            shell_id.as_str(),
            source_stream,
            node,
            "SHELL",
        )?;
        let mut shell_faces = Vec::new();
        for face in graph
            .of_kind(NodeKind::Face)
            .filter(|face| valid_face_xmts.contains(&face.xmt))
        {
            let Some(face_fields) = face.face_fields() else {
                continue;
            };
            if face_fields.shell.map(u32::from) != Some(node.xmt)
                || !face_fields
                    .surface
                    .is_some_and(|surface| surfaces.contains_key(&u32::from(surface)))
            {
                continue;
            }
            ctx.charge_collection_items(1, "nx shell faces")?;
            shell_faces
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit("nx shell faces", 0, 1))?;
            shell_faces.push(scope.id_charged::<FaceId>(
                ctx,
                &cadmpeg_ir::identity_component!("face"),
                face.xmt,
            )?);
        }
        ctx.charge_collection_items(1, "nx emitted shells")?;
        ir.model
            .shells
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("nx emitted shells", 0, 1))?;
        ir.model.shells.push(
            Shell::new(
                shell_id.try_clone_for_decode(ctx, "nx shell identity copy")?,
                region_id.try_clone_for_decode(ctx, "nx shell region identity")?,
                shell_faces,
                Vec::new(),
                Vec::new(),
            )
            .map_err(|message| cadmpeg_core::CodecError::Malformed(message.to_string()))?,
        );
        if let Some(parent) = ir
            .model
            .regions
            .iter_mut()
            .find(|candidate| candidate.id == region_id)
        {
            ctx.charge_collection_items(1, "nx region shells")?;
            parent
                .shells
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit("nx region shells", 0, 1))?;
            parent.shells.push(shell_id.try_clone_for_decode(ctx, "nx region shell identity")?);
        }
        ctx.charge_collection_items(1, "nx emitted shell index")?;
        shells.insert(node.xmt, shell_id);
    }
    let mut point_positions: BTreeMap<PointId, Point3> = BTreeMap::new();
    for point in &ir.model.points {
        if !point_positions.contains_key(&point.id) {
            ctx.charge_collection_items(1, "nx point position index")?;
            point_positions.insert(
                point.id.try_clone_for_decode(ctx, "nx indexed point identity")?,
                point.position().get(),
            );
        }
    }
    let mut vertices: BTreeMap<u32, VertexId> = BTreeMap::new();
    let mut vertex_positions: BTreeMap<VertexId, (Point3, Option<f64>)> = BTreeMap::new();
    for node in graph
        .of_kind(NodeKind::Vertex)
        .filter(|node| valid_vertex_xmts.contains(&node.xmt))
    {
        let Some(fields) = node.vertex_fields() else {
            continue;
        };
        let Some(point_ref) = fields
            .point
            .and_then(|target| points.get(&u32::from(target)))
        else {
            continue;
        };
        let Some(point_position) = point_positions.get(point_ref).copied() else {
            continue;
        };
        let tolerance = decoded_tolerance(fields.tolerance);
        let vertex: VertexId =
            scope.id_charged(ctx, &cadmpeg_ir::identity_component!("vertex"), node.xmt)?;
        annotate_node(
            ctx,
            annotations,
            vertex.as_str(),
            source_stream,
            node,
            "VERTEX",
        )?;
        if tolerance.is_some() {
            annotations
                .derived(&vertex, "tolerance")
                .map_err(cadmpeg_core::CodecError::malformed)?;
        }
        ctx.charge_collection_items(1, "nx emitted vertices")?;
        ir.model
            .vertices
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("nx emitted vertices", 0, 1))?;
        ir.model.vertices.push(Vertex {
            id: vertex.try_clone_for_decode(ctx, "nx vertex identity copy")?,
            point: point_ref.try_clone_for_decode(ctx, "nx vertex point identity")?,
            tolerance,
        });
        ctx.charge_collection_items(1, "nx emitted vertex index")?;
        vertices.insert(
            node.xmt,
            vertex.try_clone_for_decode(ctx, "nx indexed vertex identity")?,
        );
        ctx.charge_collection_items(1, "nx vertex position index")?;
        vertex_positions.insert(
            vertex,
            (
                point_position,
                tolerance.map(cadmpeg_ir::scalar::PositiveReal::get),
            ),
        );
    }
    let mut pcurve_indices: BTreeMap<PcurveId, usize> = BTreeMap::new();
    for (index, pcurve) in ir.model.pcurves.iter().enumerate() {
        ctx.charge_collection_items(1, "nx pcurve index")?;
        pcurve_indices.insert(
            pcurve.id.try_clone_for_decode(ctx, "nx indexed pcurve identity")?,
            index,
        );
    }
    let mut curve_indices: BTreeMap<CurveId, usize> = BTreeMap::new();
    for (index, curve) in ir.model.curves.iter().enumerate() {
        if !curve_indices.contains_key(&curve.id) {
            ctx.charge_collection_items(1, "nx curve index")?;
            curve_indices.insert(
                curve.id.try_clone_for_decode(ctx, "nx indexed curve identity")?,
                index,
            );
        }
    }
    let mut procedural_curve_ids: BTreeSet<CurveId> = BTreeSet::new();
    for procedural in &ir.model.procedural_curves {
        if let Some(owner) = ir.model.procedural_curve_owner(&procedural.id) {
            ctx.charge_collection_items(1, "nx procedural curve index")?;
            procedural_curve_ids.insert(owner.try_clone_for_decode(ctx, "nx indexed procedural curve identity")?);
        }
    }
    let mut curve_point_cache = CurvePointCache::default();
    let mut edges: BTreeMap<u32, EdgeId> = BTreeMap::new();
    for node in graph
        .of_kind(NodeKind::Edge)
        .filter(|node| valid_edge_xmts.contains(&node.xmt))
    {
        let Some(fields) = node.edge_fields() else {
            continue;
        };
        let Some(fin) = graph.get_target(NodeKind::Fin, fields.fin) else {
            continue;
        };
        let Some(fin_fields) = fin.fin_fields() else {
            continue;
        };
        let curve_xmt = [fields.curve, fin_fields.curve_xmt]
            .into_iter()
            .flatten()
            .map(u32::from)
            .find(|xmt| *xmt > 1);
        let mut curve = curve_xmt
            .and_then(|xmt| curves.get(&xmt))
            .map(|id| id.try_clone_for_decode(ctx, "nx edge curve identity"))
            .transpose()?;
        let mut param_range = curve_xmt.and_then(|xmt| trim_ranges.get(&xmt)).copied();
        if curve.is_none() {
            let lifted = (|| -> Result<Option<_>, CodecError> {
                let Some(xmt) = curve_xmt else {
                    return Ok(None);
                };
                let Some(pcurve_id) = pcurves.get(&xmt) else {
                    return Ok(None);
                };
                let Some(pcurve_index) = pcurve_indices.get(pcurve_id) else {
                    return Ok(None);
                };
                let Some(pcurve) = ir.model.pcurves.get(*pcurve_index) else {
                    return Ok(None);
                };
                let Some(surface_ref) = pcurve_supports.get(&xmt) else {
                    return Ok(None);
                };
                let surface =
                    surface_ref.try_clone_for_decode(ctx, "nx parametric edge surface")?;
                let parameter_range = pcurve
                    .parameter_range()
                    .map(cadmpeg_ir::units::FiniteVector::get)
                    .or(param_range)
                    .or_else(|| pcurve_parameter_range(&pcurve.geometry));
                let Some(parameter_range) = parameter_range.and_then(ordered_parameter_range)
                else {
                    return Ok(None);
                };
                Ok(Some((
                    surface,
                    pcurve
                        .geometry
                        .try_clone_for_decode(ctx, "nx parametric edge pcurve")?,
                    parameter_range,
                    pcurve.fit_tolerance(),
                )))
            })()?;
            if let Some((surface, pcurve, parameter_range, _fit_tolerance)) = lifted {
                let carrier: CurveId = scope.id_charged(
                    ctx,
                    &cadmpeg_ir::identity_component!("edge-parametric-curve"),
                    node.xmt,
                )?;
                let construction: ProceduralCurveId = scope.id_charged(
                    ctx,
                    &cadmpeg_ir::identity_component!("edge-parametric-construction"),
                    node.xmt,
                )?;
                super::annotations::note(
                    ctx,
                    annotations,
                    carrier.as_str(),
                    source_stream,
                    node.pos as u64,
                    "PARAMETRIC_SURFACE_CURVE",
                )?;
                super::annotations::derived(ctx, annotations, carrier.as_str(), "geometry")?;
                ctx.charge_collection_items(1, "nx parametric edge curves")?;
                ir.model
                    .curves
                    .try_reserve(1)
                    .map_err(|_| ctx.refuse_codec_limit("nx parametric edge curves", 0, 1))?;
                ir.model.curves.push(Curve {
                    id: carrier.try_clone_for_decode(ctx, "nx parametric edge carrier")?,
                    geometry: CurveGeometry::Procedural {
                        construction: construction.try_clone_for_decode(ctx, "nx parametric edge construction")?,
                        cache: None,
                    },
                    source_object: None,
                });
                ctx.charge_collection_items(1, "nx parametric edge constructions")?;
                ir.model.procedural_curves.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("nx parametric edge constructions", 0, 1)
                })?;
                let _attached = ir.model.add_procedural_curve(
                    &carrier.try_clone_for_decode(ctx, "nx parametric construction owner")?,
                    ProceduralCurve::new(
                        construction,
                        ProceduralCurveDefinition::SurfaceCurve {
                            family: SurfaceCurveFamily::Parametric {
                                context: IntcurveSupportContext::try_new(
                                    [
                                        IntcurveSupportSide {
                                            surface: Some(surface),
                                            pcurve: Some(pcurve.into()),
                                        },
                                        IntcurveSupportSide {
                                            surface: None,
                                            pcurve: None,
                                        },
                                    ],
                                    parameter_range,
                                    [Vec::new(), Vec::new(), Vec::new()],
                                )
                                .map_err(CodecError::malformed)?,
                                tail: None,
                            },
                        },
                    ),
                );
                curve = Some(carrier);
                param_range = None;
            }
        }
        let closed_edge = fin_fields.vertex.is_none()
            && fin_fields.forward.map(u32::from) == Some(fin.xmt)
            && fin_fields.backward.map(u32::from) == Some(fin.xmt);
        let start = fin_fields
            .vertex
            .and_then(|target| vertices.get(&u32::from(target)))
            .map(|id| id.try_clone_for_decode(ctx, "nx edge start vertex"))
            .transpose()?;
        let start = if start.is_some() || !closed_edge {
            start
        } else if let Some((curve, curve_index)) = curve.as_ref().and_then(|curve| {
            curve_indices
                .get(curve)
                .copied()
                .map(|index| (curve, index))
        }) {
            synthesize_closed_edge_vertex_with_curve_index_and_budget(
                ctx,
                ir,
                annotations,
                &scope,
                node,
                curve,
                curve_index,
                param_range,
                source_stream,
                decoded_tolerance(fields.tolerance),
                &mut curve_point_cache,
                adaptive_geometry_budget,
            )?
        } else {
            None
        };
        let Some(start) = start else {
            continue;
        };
        let end_fin = fin_fields
            .other
            .filter(|target| u32::from(*target) > 1)
            .or(fin_fields.forward);
        let Some(end_fields) = graph
            .get_target(NodeKind::Fin, end_fin)
            .and_then(Node::fin_fields)
        else {
            continue;
        };
        let mut end = end_fields
            .vertex
            .and_then(|target| vertices.get(&u32::from(target)))
            .map(|id| id.try_clone_for_decode(ctx, "nx edge end vertex"))
            .transpose()?;
        if end.is_none()
            && end_fields.vertex.is_none()
            && end_fields.forward == end_fin
            && end_fields.backward == end_fin
        {
            // A partnered closed FIN repeats the null vertex and closes its own
            // forward/backward links. Its endpoint is the same analytic point
            // as the current FIN's synthesized start, even when `end_fin` is a
            // distinct radial partner record.
            end = Some(start.try_clone_for_decode(ctx, "nx closed edge end vertex")?);
        }
        let Some(end) = end else {
            continue;
        };
        let (mut start, mut end) = (start, end);
        let id: EdgeId =
            scope.id_charged(ctx, &cadmpeg_ir::identity_component!("edge"), node.xmt)?;
        annotate_node(ctx, annotations, id.as_str(), source_stream, node, "EDGE")?;
        if decoded_tolerance(fields.tolerance).is_some() {
            annotations
                .derived(&id, "tolerance")
                .map_err(cadmpeg_core::CodecError::malformed)?;
        }
        if let (Some(carrier), Some(range)) = (&curve, param_range) {
            let oriented = if let Some((
                curve_index,
                (start_position, start_tolerance),
                (end_position, end_tolerance),
            )) = curve_indices
                .get(carrier)
                .copied()
                .zip(vertex_positions.get(&start).copied())
                .zip(vertex_positions.get(&end).copied())
                .map(|((curve_index, start), end)| (curve_index, start, end))
            {
                orient_edge_range_for_geometry_with_budget(
                    ctx,
                    &ir.model.curves[curve_index].geometry,
                    carrier,
                    range,
                    start_position,
                    start_tolerance,
                    end_position,
                    end_tolerance,
                    decoded_tolerance(fields.tolerance).map(cadmpeg_ir::scalar::PositiveReal::get),
                    procedural_curve_ids.contains(carrier),
                    &mut curve_point_cache,
                    adaptive_geometry_budget,
                )?
            } else {
                None
            };
            match oriented {
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
        ctx.charge_collection_items(1, "nx emitted edges")?;
        ir.model
            .edges
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("nx emitted edges", 0, 1))?;
        ir.model.edges.push(Edge {
            id: id.try_clone_for_decode(ctx, "nx edge identity copy")?,
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(curve, param_range)
                .map_err(CodecError::malformed)?,
            start,
            end,
            tolerance: decoded_tolerance(fields.tolerance),
        });
        ctx.charge_collection_items(1, "nx emitted edge index")?;
        edges.insert(node.xmt, id);
    }
    let mut edge_curves_by_id: BTreeMap<EdgeId, CurveId> = BTreeMap::new();
    for edge in &ir.model.edges {
        if let Some(curve) = edge.curve() {
            ctx.charge_collection_items(1, "nx edge curve index")?;
            edge_curves_by_id.insert(
                edge.id.try_clone_for_decode(ctx, "nx indexed edge identity")?,
                curve.try_clone_for_decode(ctx, "nx indexed edge curve identity")?,
            );
        }
    }
    let mut faces: BTreeMap<u32, FaceId> = BTreeMap::new();
    let mut pending_faces: Vec<PendingFace> = Vec::new();
    for node in graph
        .of_kind(NodeKind::Face)
        .filter(|node| valid_face_xmts.contains(&node.xmt))
    {
        let Some(fields) = node.face_fields() else {
            continue;
        };
        let Some(shell_ref) = fields
            .shell
            .and_then(|target| shells.get(&u32::from(target)))
        else {
            continue;
        };
        let Some(surface_ref) = fields
            .surface
            .and_then(|target| surfaces.get(&u32::from(target)))
        else {
            continue;
        };
        let id: FaceId =
            scope.id_charged(ctx, &cadmpeg_ir::identity_component!("face"), node.xmt)?;
        annotate_node(ctx, annotations, id.as_str(), source_stream, node, "FACE")?;
        if decoded_tolerance(fields.tolerance).is_some() {
            annotations
                .derived(&id, "tolerance")
                .map_err(cadmpeg_core::CodecError::malformed)?;
        }
        ctx.charge_collection_items(1, "nx pending faces")?;
        pending_faces
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("nx pending faces", 0, 1))?;
        pending_faces.push(PendingFace {
            xmt: node.xmt,
            id: id.try_clone_for_decode(ctx, "nx pending face identity")?,
            shell: shell_ref.try_clone_for_decode(ctx, "nx pending face shell")?,
            surface: surface_ref.try_clone_for_decode(ctx, "nx pending face surface")?,
            sense: fields.sense,
            tolerance: decoded_tolerance(fields.tolerance),
        });
        ctx.charge_collection_items(1, "nx emitted face index")?;
        faces.insert(node.xmt, id);
    }
    let mut loops: BTreeMap<u32, LoopId> = BTreeMap::new();
    let mut loop_specs: BTreeMap<u32, (LoopId, FaceId)> = BTreeMap::new();
    for &loop_xmt in valid_loop_rings.keys() {
        let Some(node) = graph.get(NodeKind::Loop, loop_xmt) else {
            continue;
        };
        let Some(fields) = node.loop_fields() else {
            continue;
        };
        let Some(face_ref) = fields.face.and_then(|target| faces.get(&u32::from(target))) else {
            continue;
        };
        let id: LoopId =
            scope.id_charged(ctx, &cadmpeg_ir::identity_component!("loop"), node.xmt)?;
        let ring_resolves = valid_loop_rings[&loop_xmt].iter().all(|fin_xmt| {
            graph
                .get(NodeKind::Fin, *fin_xmt)
                .and_then(Node::fin_fields)
                .is_some_and(|fields| {
                    fields
                        .edge
                        .is_some_and(|target| edges.contains_key(&u32::from(target)))
                })
        });
        if !ring_resolves {
            ctx.charge_collection_items(1, "nx topology losses")?;
            topology_losses
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit("nx topology losses", 0, 1))?;
            super::charge_loss_code(ctx, crate::loss::NxLossCode::TopologyLoopRingUnresolved)?;
            topology_losses.push(crate::loss::NxLossCode::TopologyLoopRingUnresolved.note(
                render_retained_text(
                    ctx,
                    format_args!(
                        "parasolid#{stream_index} LOOP {loop_xmt} of {face_ref} states no resolvable coedge ring: loop {id} is omitted from its face"
                    ),
                    "nx unresolved loop loss text",
                )?,
            ));
            continue;
        }
        annotate_node(ctx, annotations, id.as_str(), source_stream, node, "LOOP")?;
        ctx.charge_collection_items(1, "nx loop specification index")?;
        loop_specs.insert(
            node.xmt,
            (
                id.try_clone_for_decode(ctx, "nx loop specification identity")?,
                face_ref.try_clone_for_decode(ctx, "nx loop face identity")?,
            ),
        );
        ctx.charge_collection_items(1, "nx emitted loop index")?;
        loops.insert(node.xmt, id);
    }
    let mut fin_ids = BTreeMap::new();
    for xmt in &valid_fin_xmts {
        if graph
            .get(NodeKind::Fin, *xmt)
            .and_then(Node::fin_fields)
            .is_some_and(|fields| {
                fields
                    .loop_xmt
                    .is_some_and(|target| loops.contains_key(&u32::from(target)))
            })
        {
            ctx.charge_collection_items(1, "nx fin identity index")?;
            fin_ids.insert(
                *xmt,
                scope.id_charged::<CoedgeId>(ctx, &cadmpeg_ir::identity_component!("fin"), xmt)?,
            );
        }
    }
    // Preserve the endpoint proof only when the admitted carrier is the exact
    // intersection candidate consumed by the later attachment pass. A valid
    // unrelated coedge pcurve must not become a general admission shortcut.
    let mut endpoint_witnesses = EndpointWitnesses::new();
    let mut intersection_pcurves = IntersectionPcurveIndex::new();
    for procedural in &ir.model.procedural_curves {
        let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
        else {
            continue;
        };
        let Some(owner) = ir.model.procedural_curve_owner(&procedural.id) else {
            continue;
        };
        for side in context.sides() {
            let (Some(surface), Some(pcurve)) = (&side.surface, &side.pcurve) else {
                continue;
            };
            ctx.charge_collection_items(1, "nx intersection pcurve index")?;
            intersection_pcurves.insert(
                (
                    owner.try_clone_for_decode(ctx, "nx intersection pcurve owner")?,
                    surface.try_clone_for_decode(ctx, "nx intersection pcurve support")?,
                ),
                (
                    pcurve
                        .geometry
                        .try_clone_for_decode(ctx, "nx intersection pcurve geometry")?,
                    context.parameter_range().endpoints(),
                    procedural.cache_fit_tolerance(),
                ),
            );
        }
    }
    let (valid_pcurve_fins, fallback_pcurves) = {
        let index = cadmpeg_ir::index::ModelIndex::try_new_model_only_for_decode(ir, ctx)?;
        let valid_pcurve_fins = fin_ids
            .keys()
            .map(|fin_xmt| -> Result<Option<u32>, CodecError> {
                let candidate = (|| {
                    let fields = graph.get(NodeKind::Fin, *fin_xmt)?.fin_fields()?;
                    let edge = fields
                        .edge
                        .and_then(|target| edges.get(&u32::from(target)))?;
                    let support = graph
                        .get_target(NodeKind::Loop, fields.loop_xmt)
                        .and_then(Node::loop_fields)
                        .and_then(|loop_| graph.get_target(NodeKind::Face, loop_.face))
                        .and_then(Node::face_fields)
                        .and_then(|face| {
                            face.surface
                                .and_then(|target| surfaces.get(&u32::from(target)))
                        })?;
                    let carrier = fields
                        .curve_xmt
                        .and_then(|target| pcurves.get(&u32::from(target)))
                        .and_then(|id| index.pcurves(id.as_str()))?;
                    let use_range = fields
                        .curve_xmt
                        .and_then(|target| trim_ranges.get(&u32::from(target)))
                        .copied()
                        .and_then(ordered_parameter_range);
                    let parameter_range = use_range
                        .or(carrier
                            .parameter_range()
                            .map(cadmpeg_ir::units::FiniteVector::get))
                        .or_else(|| pcurve_parameter_range(&carrier.geometry));
                    Some((edge, support, carrier, parameter_range))
                })();
                let Some((edge, support, carrier, parameter_range)) = candidate else {
                    return Ok(None);
                };
                let Some(endpoints) = pcurve_endpoint_witness_with_index_and_budget(
                    &index,
                    edge,
                    support,
                    &carrier.geometry,
                    parameter_range,
                    carrier
                        .fit_tolerance()
                        .map(cadmpeg_ir::geometry::FitTolerance::get),
                    adaptive_geometry_budget,
                )?
                else {
                    return Ok(None);
                };
                let Some(curve) = index.edges(edge.as_str()).and_then(|edge| edge.curve()) else {
                    return Ok(None);
                };
                let Some(parameter_range) = parameter_range else {
                    return Ok(None);
                };
                let Some((candidate_geometry, candidate_range, _)) = intersection_pcurves.get(&(
                    curve.try_clone_for_decode(ctx, "nx witness lookup curve")?,
                    support.try_clone_for_decode(ctx, "nx witness lookup support")?,
                )) else {
                    return Ok(Some(*fin_xmt));
                };
                if *candidate_geometry != carrier.geometry || *candidate_range != parameter_range {
                    return Ok(Some(*fin_xmt));
                }
                let key = (
                    curve.try_clone_for_decode(ctx, "nx endpoint witness curve")?,
                    support.try_clone_for_decode(ctx, "nx endpoint witness support")?,
                );
                let witnesses = match endpoint_witnesses.entry(key) {
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        ctx.charge_collection_items(1, "nx endpoint witness index")?;
                        entry.insert(Vec::new())
                    }
                };
                ctx.charge_collection_items(1, "nx endpoint witnesses")?;
                witnesses
                    .try_reserve(1)
                    .map_err(|_| ctx.refuse_codec_limit("nx endpoint witnesses", 0, 1))?;
                witnesses.push((
                    carrier
                        .geometry
                        .try_clone_for_decode(ctx, "nx endpoint witness pcurve")?,
                    parameter_range,
                    endpoints,
                ));
                Ok(Some(*fin_xmt))
            })
            .try_fold(BTreeSet::new(), |mut values, candidate| {
                if let Some(fin_xmt) = candidate? {
                    ctx.charge_collection_items(1, "nx valid pcurve fins")?;
                    values.insert(fin_xmt);
                }
                Ok::<_, CodecError>(values)
            })?;
        let fallback_pcurves = fin_ids
            .keys()
            .map(|fin_xmt| -> Result<Option<_>, CodecError> {
                if valid_pcurve_fins.contains(fin_xmt) {
                    return Ok(None);
                }
                let candidate = (|| -> Result<Option<_>, CodecError> {
                    let Some(fields) = graph
                        .get(NodeKind::Fin, *fin_xmt)
                        .and_then(Node::fin_fields)
                    else {
                        return Ok(None);
                    };
                    let Some(edge) = fields.edge.and_then(|target| edges.get(&u32::from(target)))
                    else {
                        return Ok(None);
                    };
                    let Some(support_ref) = graph
                        .get_target(NodeKind::Loop, fields.loop_xmt)
                        .and_then(Node::loop_fields)
                        .and_then(|loop_| graph.get_target(NodeKind::Face, loop_.face))
                        .and_then(Node::face_fields)
                        .and_then(|face| {
                            face.surface
                                .and_then(|target| surfaces.get(&u32::from(target)))
                        })
                    else {
                        return Ok(None);
                    };
                    let Some(carrier) = edge_curves_by_id.get(edge) else {
                        return Ok(None);
                    };
                    let support: SurfaceId =
                        support_ref.try_clone_for_decode(ctx, "nx fallback support identity")?;
                    let Some((geometry, parameter_range, fit_tolerance)) = intersection_pcurves
                        .get(&(
                            carrier.try_clone_for_decode(ctx, "nx fallback lookup curve")?,
                            support.try_clone_for_decode(ctx, "nx fallback lookup support")?,
                        ))
                    else {
                        return Ok(None);
                    };
                    Ok(Some((
                        edge,
                        support,
                        geometry.try_clone_for_decode(ctx, "nx fallback pcurve geometry")?,
                        *parameter_range,
                        *fit_tolerance,
                    )))
                })()?;
                let Some((edge, support, geometry, parameter_range, fit_tolerance)) = candidate
                else {
                    return Ok(None);
                };
                Ok(pcurve_matches_edge_range_with_index_and_budget(
                    &index,
                    edge,
                    &support,
                    &geometry,
                    None,
                    fit_tolerance.map(cadmpeg_ir::geometry::FitTolerance::get),
                    adaptive_geometry_budget,
                )?
                .then_some((
                    *fin_xmt,
                    (support, geometry, parameter_range, fit_tolerance),
                )))
            })
            .try_fold(BTreeMap::new(), |mut values, candidate| {
                if let Some((fin_xmt, pcurve)) = candidate? {
                    ctx.charge_collection_items(1, "nx fallback pcurve index")?;
                    values.insert(fin_xmt, pcurve);
                }
                Ok::<_, CodecError>(values)
            })?;
        (valid_pcurve_fins, fallback_pcurves)
    };
    let mut serialized_branch_pcurves = BTreeSet::new();
    for &fin_xmt in fin_ids.keys() {
        let Some(node) = graph.get(NodeKind::Fin, fin_xmt) else {
            continue;
        };
        let Some(fields) = node.fin_fields() else {
            continue;
        };
        let Some(loop_id_ref) = fields
            .loop_xmt
            .and_then(|target| loops.get(&u32::from(target)))
        else {
            continue;
        };
        let Some(edge_ref) = fields.edge.and_then(|target| edges.get(&u32::from(target))) else {
            continue;
        };
        let Some(id_ref) = fin_ids.get(&node.xmt) else {
            continue;
        };
        let loop_id: LoopId = loop_id_ref.try_clone_for_decode(ctx, "nx fin loop identity")?;
        let edge: EdgeId = edge_ref.try_clone_for_decode(ctx, "nx fin edge identity")?;
        let id: CoedgeId = id_ref.try_clone_for_decode(ctx, "nx fin identity copy")?;
        annotate_node(ctx, annotations, id.as_str(), source_stream, node, "FIN")?;
        let partner = fields
            .other
            .and_then(|target| fin_ids.get(&u32::from(target)))
            .map(|partner| partner.try_clone_for_decode(ctx, "nx fin partner identity"))
            .transpose()?;
        let radial_next = partner.as_ref().unwrap_or(&id).try_clone_for_decode(ctx, "nx fin radial successor identity")?;
        let support = graph
            .get_target(NodeKind::Loop, fields.loop_xmt)
            .and_then(Node::loop_fields)
            .and_then(|loop_| graph.get_target(NodeKind::Face, loop_.face))
            .and_then(Node::face_fields)
            .and_then(|face| {
                face.surface
                    .and_then(|target| surfaces.get(&u32::from(target)))
            })
            .map(|surface| surface.try_clone_for_decode(ctx, "nx fin support identity"))
            .transpose()?;
        let pcurve_use_range = fields
            .curve_xmt
            .and_then(|target| trim_ranges.get(&u32::from(target)))
            .copied()
            .and_then(ordered_parameter_range);
        let mut pcurve = if valid_pcurve_fins.contains(&node.xmt) {
            fields
                .curve_xmt
                .and_then(|target| pcurves.get(&u32::from(target)))
                .map(|id| id.try_clone_for_decode(ctx, "nx fin pcurve identity"))
                .transpose()?
        } else {
            None
        };
        let edge_curve = edge_curves_by_id.get(&edge);
        if let (Some(pcurve), Some(edge_curve), Some(support)) =
            (pcurve.as_ref(), edge_curve, support.as_ref())
        {
            if fields
                .curve_xmt
                .and_then(|target| curves.get(&u32::from(target)))
                == Some(edge_curve)
                && fields
                    .curve_xmt
                    .and_then(|target| pcurve_supports.get(&u32::from(target)))
                    == Some(support)
            {
                ctx.charge_collection_items(1, "nx serialized branch pcurves")?;
                serialized_branch_pcurves.insert((
                    edge_curve.try_clone_for_decode(ctx, "nx branch curve identity")?,
                    support.try_clone_for_decode(ctx, "nx branch support identity")?,
                    pcurve.try_clone_for_decode(ctx, "nx branch pcurve identity")?,
                ));
            }
        }
        let attached_pcurve_use_range = pcurve.as_ref().and(pcurve_use_range);
        if pcurve.is_none() {
            if let Some((_support, geometry, parameter_range, fit_tolerance)) =
                fallback_pcurves.get(&fin_xmt)
            {
                let pcurve_id: PcurveId = scope.id_charged(
                    ctx,
                    &cadmpeg_ir::identity_component!("intersection-pcurve"),
                    fin_xmt,
                )?;
                super::annotations::note(
                    ctx,
                    annotations,
                    pcurve_id.as_str(),
                    source_stream,
                    node.pos as u64,
                    "INTERSECTION_PCURVE",
                )?;
                super::annotations::derived(ctx, annotations, pcurve_id.as_str(), "geometry")?;
                super::annotations::derived(
                    ctx,
                    annotations,
                    pcurve_id.as_str(),
                    "parameter_range",
                )?;
                if fit_tolerance.is_some() {
                    super::annotations::derived(
                        ctx,
                        annotations,
                        pcurve_id.as_str(),
                        "fit_tolerance",
                    )?;
                }
                ctx.charge_collection_items(1, "nx fallback pcurves")?;
                ir.model
                    .pcurves
                    .try_reserve(1)
                    .map_err(|_| ctx.refuse_codec_limit("nx fallback pcurves", 0, 1))?;
                ir.model.pcurves.push(Pcurve {
                    id: pcurve_id.try_clone_for_decode(ctx, "nx fallback pcurve identity")?,
                    geometry: geometry.try_clone_for_decode(ctx, "nx attached fallback pcurve")?,
                    metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                        None,
                        Some(
                            cadmpeg_ir::units::FiniteVector::new(*parameter_range)
                                .ok_or(PcurveMetadata::NON_FINITE_PARAMETER_RANGE)
                                .map_err(cadmpeg_core::CodecError::malformed)?,
                        ),
                        *fit_tolerance,
                    ),
                });
                pcurve = Some(pcurve_id);
            }
        }
        ctx.charge_collection_items(1, "nx emitted coedges")?;
        ir.model
            .coedges
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("nx emitted coedges", 0, 1))?;
        let mut pcurve_uses = Vec::new();
        if let Some(pcurve) = pcurve {
            ctx.charge_collection_items(1, "nx coedge pcurve uses")?;
            pcurve_uses
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit("nx coedge pcurve uses", 0, 1))?;
            pcurve_uses.push(cadmpeg_ir::topology::PcurveUse {
                pcurve,
                isoparametric: None,
                parameter_range: attached_pcurve_use_range
                    .map(cadmpeg_ir::geometry::DirectedParameterRange::new)
                    .transpose()
                    .map_err(CodecError::malformed)?,
            });
        }
        ir.model.coedges.push(Coedge {
            id: id.try_clone_for_decode(ctx, "nx coedge identity copy")?,
            owner_loop: loop_id.try_clone_for_decode(ctx, "nx coedge loop identity")?,
            edge,
            radial_next,
            sense: fields.sense,
            pcurves: pcurve_uses,
            use_curve: None,
        });
    }
    let mut face_loops: BTreeMap<FaceId, Vec<LoopId>> = BTreeMap::new();
    for rings in face_loop_rings.values() {
        for (loop_xmt, fin_xmts) in rings {
            let Some((id, face)) = loop_specs.get(loop_xmt) else {
                continue;
            };
            let mut coedges = Vec::new();
            let mut all_resolved = true;
            for fin_xmt in fin_xmts {
                let Some(fin_id) = fin_ids.get(fin_xmt) else {
                    all_resolved = false;
                    break;
                };
                ctx.charge_collection_items(1, "nx loop coedges")?;
                coedges
                    .try_reserve(1)
                    .map_err(|_| ctx.refuse_codec_limit("nx loop coedges", 0, 1))?;
                coedges.push(fin_id.try_clone_for_decode(ctx, "nx loop coedge identity")?);
            }
            let ring = if all_resolved {
                LoopRing::try_new_for_decode(ctx, coedges, Vec::new())?.ok()
            } else {
                None
            };
            let Some(ring) = ring else {
                ctx.charge_collection_items(1, "nx topology losses")?;
                topology_losses
                    .try_reserve(1)
                    .map_err(|_| ctx.refuse_codec_limit("nx topology losses", 0, 1))?;
                super::charge_loss_code(ctx, crate::loss::NxLossCode::TopologyLoopRingUnresolved)?;
                topology_losses.push(crate::loss::NxLossCode::TopologyLoopRingUnresolved.note(
                    render_retained_text(
                        ctx,
                        format_args!(
                            "parasolid#{stream_index} LOOP {loop_xmt} of {face} states no resolvable coedge ring: loop {id} is omitted from its face"
                        ),
                        "nx unresolved loop loss text",
                    )?,
                ));
                continue;
            };
            ctx.charge_collection_items(1, "nx emitted loops")?;
            ir.model
                .loops
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit("nx emitted loops", 0, 1))?;
            ir.model.loops.push(Loop {
                id: id.try_clone_for_decode(ctx, "nx emitted loop identity")?,
                face: face.try_clone_for_decode(ctx, "nx emitted loop face identity")?,
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
            });
            let key: FaceId = face.try_clone_for_decode(ctx, "nx face loop index identity")?;
            let loops = match face_loops.entry(key) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    ctx.charge_collection_items(1, "nx face loop index")?;
                    entry.insert(Vec::new())
                }
            };
            ctx.charge_collection_items(1, "nx face loops")?;
            loops
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit("nx face loops", 0, 1))?;
            loops.push(id.try_clone_for_decode(ctx, "nx face loop identity")?);
        }
    }
    for pending in pending_faces {
        if let Some(failure) = face_loop_failures.remove(&pending.xmt) {
            ctx.charge_collection_items(1, "nx topology losses")?;
            topology_losses
                .try_reserve(1)
                .map_err(|_| ctx.refuse_codec_limit("nx topology losses", 0, 1))?;
            super::charge_loss_code(ctx, crate::loss::NxLossCode::TopologyFaceLoopUnresolved)?;
            topology_losses.push(crate::loss::NxLossCode::TopologyFaceLoopUnresolved.note(
                render_retained_text(
                    ctx,
                    format_args!(
                        "parasolid#{stream_index} FACE {} has an unresolved boundary: {failure}; face is emitted without loops",
                        pending.xmt
                    ),
                    "nx unresolved face loss text",
                )?,
            ));
        }
        let loops = face_loops.remove(&pending.id).unwrap_or_default();
        ctx.charge_collection_items(1, "nx emitted faces")?;
        ir.model
            .faces
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("nx emitted faces", 0, 1))?;
        ir.model.faces.push(Face {
            id: pending.id,
            shell: pending.shell,
            surface: pending.surface,
            sense: pending.sense,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(loops),
            name: None,
            color: None,
            tolerance: pending.tolerance,
        });
    }
    attach_tolerant_edge_intersections_with_budget(
        ctx,
        ir,
        graph,
        &edges,
        (&scope, source_stream),
        annotations,
        adaptive_geometry_budget,
    )?;
    intersection_index.complete_from_stream(ctx, ir, intersection_starts)?;
    complete_tolerant_intersection_pcurves_from_serialized_branches_for_stream_with_budget(
        ctx,
        ir,
        &serialized_branch_pcurves,
        intersection_starts.coedges,
        intersection_starts.procedural_curves,
        annotations,
        completion_geometry_budget,
    )?;
    complete_exact_boundary_intersection_pcurves_with_budget(
        ctx,
        ir,
        annotations,
        procedural_start,
        exact_transfer_budget,
        completion_geometry_budget,
    )?;
    complete_intersection_pcurves_from_opposite_charts_with_budget(
        ctx,
        ir,
        procedural_start,
        completion_transfer_budget,
        completion_geometry_budget,
    )?;
    intersection_index.complete_new_pcurves_from_stream(ctx, ir, intersection_starts.pcurves)?;

    let mut owned_edges: BTreeSet<EdgeId> = BTreeSet::new();
    for coedge in &ir.model.coedges {
        ctx.charge_collection_items(1, "nx owned edge index")?;
        owned_edges.insert(coedge.edge.try_clone_for_decode(ctx, "nx owned edge identity")?);
    }
    let mut candidate_edges = BTreeSet::new();
    for edge in edges.into_values() {
        ctx.charge_collection_items(1, "nx candidate edge index")?;
        candidate_edges.insert(edge);
    }
    ir.model
        .edges
        .retain(|edge| !candidate_edges.contains(&edge.id) || owned_edges.contains(&edge.id));
    let mut retained_vertices: BTreeSet<VertexId> = BTreeSet::new();
    for edge in &ir.model.edges {
        for vertex in [&edge.start, &edge.end] {
            ctx.charge_collection_items(1, "nx retained vertex index")?;
            retained_vertices.insert(vertex.try_clone_for_decode(ctx, "nx retained vertex identity")?);
        }
    }
    let scope_prefix = scope.prefix_charged(ctx)?;
    ir.model.vertices.retain(|vertex| {
        !vertex.id.as_str().starts_with(&scope_prefix) || retained_vertices.contains(&vertex.id)
    });
    Ok(endpoint_witnesses)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn retain_unresolved_topology_carriers(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    stream_index: usize,
    graph: &Graph,
    surfaces: &mut BTreeMap<u32, SurfaceId>,
    curves: &mut BTreeMap<u32, CurveId>,
    pcurves: &BTreeMap<u32, PcurveId>,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    let scope = IdScope::stream_charged(ctx, stream_index)?;
    let unknown: UnknownId = IdScope::container().id_charged(
        ctx,
        &cadmpeg_ir::identity_component!("parasolid"),
        stream_index,
    )?;
    for face in graph.of_kind(NodeKind::Face) {
        let Some(surface_xmt) = face
            .face_fields()
            .and_then(|fields| fields.surface.map(u32::from))
        else {
            continue;
        };
        if surface_xmt <= 1 || surfaces.contains_key(&surface_xmt) {
            continue;
        }
        let id: SurfaceId = scope.id_charged(
            ctx,
            &cadmpeg_ir::identity_component!("surface"),
            format_args!("unknown-{surface_xmt}"),
        )?;
        super::annotations::note(
            ctx,
            annotations,
            id.as_str(),
            source_stream,
            face.pos as u64,
            "UNRESOLVED_SURFACE_REFERENCE",
        )?;
        super::annotations::exactness(ctx, annotations, id.as_str(), Exactness::Unknown)?;
        ctx.charge_collection_items(1, "nx unresolved surfaces")?;
        ir.model
            .surfaces
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("nx unresolved surfaces", 0, 1))?;
        ir.model.surfaces.push(Surface {
            id: id.try_clone_for_decode(ctx, "nx unresolved surface identity")?,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                record: Some(unknown.try_clone_for_decode(ctx, "nx unresolved surface record")?),
            }),
            source_object: None,
        });
        ctx.charge_collection_items(1, "nx unresolved surface index")?;
        surfaces.insert(surface_xmt, id);
    }

    for edge in graph.of_kind(NodeKind::Edge) {
        let Some(curve_xmt) = edge
            .edge_fields()
            .and_then(|fields| fields.curve.map(u32::from))
        else {
            continue;
        };
        if curve_xmt <= 1 || curves.contains_key(&curve_xmt) || pcurves.contains_key(&curve_xmt) {
            continue;
        }
        let id: CurveId = scope.id_charged(
            ctx,
            &cadmpeg_ir::identity_component!("curve"),
            format_args!("unknown-{curve_xmt}"),
        )?;
        super::annotations::note(
            ctx,
            annotations,
            id.as_str(),
            source_stream,
            edge.pos as u64,
            "UNRESOLVED_CURVE_REFERENCE",
        )?;
        super::annotations::exactness(ctx, annotations, id.as_str(), Exactness::Unknown)?;
        ctx.charge_collection_items(1, "nx unresolved curves")?;
        ir.model
            .curves
            .try_reserve(1)
            .map_err(|_| ctx.refuse_codec_limit("nx unresolved curves", 0, 1))?;
        ir.model.curves.push(Curve {
            id: id.try_clone_for_decode(ctx, "nx unresolved curve identity")?,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                record: Some(unknown.try_clone_for_decode(ctx, "nx unresolved curve record")?),
            }),
            source_object: None,
        });
        ctx.charge_collection_items(1, "nx unresolved curve index")?;
        curves.insert(curve_xmt, id);
    }
    Ok(())
}

pub(super) fn annotate_node(
    ctx: &DecodeContext<'_>,
    annotations: &mut AnnotationBuilder,
    id: &str,
    stream: &cadmpeg_ir::annotations::StreamHandle,
    node: &Node,
    tag: &str,
) -> Result<(), CodecError> {
    super::annotations::note(ctx, annotations, id, stream, node.pos as u64, tag)
}

pub(super) fn surface_tag(geometry: &SolvedSurfaceGeometry) -> &'static str {
    match geometry {
        SolvedSurfaceGeometry::Plane(_) => "PLANE",
        SolvedSurfaceGeometry::Cylinder(_) => "CYLINDER",
        SolvedSurfaceGeometry::Cone(_) => "CONE",
        SolvedSurfaceGeometry::Sphere(_) => "SPHERE",
        SolvedSurfaceGeometry::Torus(_) => "TORUS",
        SolvedSurfaceGeometry::Nurbs(_) => "B_SPLINE_SURFACE",
        SolvedSurfaceGeometry::Polygonal(_) => "POLYGONAL_SURFACE",
        SolvedSurfaceGeometry::Transformed(placed) => surface_tag(placed.basis()),
        SolvedSurfaceGeometry::Unknown { .. } => "UNKNOWN_SURFACE",
    }
}

pub(super) fn curve_tag(geometry: &SolvedCurveGeometry) -> &'static str {
    match geometry {
        SolvedCurveGeometry::Line(_) => "LINE",
        SolvedCurveGeometry::Circle(_) => "CIRCLE",
        SolvedCurveGeometry::Ellipse(_) => "ELLIPSE",
        SolvedCurveGeometry::Parabola(_) => "PARABOLA",
        SolvedCurveGeometry::Hyperbola(_) => "HYPERBOLA",
        SolvedCurveGeometry::Degenerate(_) => "DEGENERATE_CURVE",
        SolvedCurveGeometry::Nurbs(_) => "B_SPLINE_CURVE",
        SolvedCurveGeometry::Composite { .. } => "COMPOSITE_CURVE",
        SolvedCurveGeometry::Polyline(_) => "POLYLINE",
        SolvedCurveGeometry::Transformed(placed) => curve_tag(placed.basis()),
        SolvedCurveGeometry::Unknown { .. } => "UNKNOWN_CURVE",
    }
}

/// Admit a metre tolerance in millimetres. The scaled value is finite and
/// positive exactly when the metre value is and scaling does not overflow,
/// and the negative missing-tolerance sentinel is refused by its sign.
pub(crate) fn decoded_tolerance(value: f64) -> Option<cadmpeg_ir::scalar::PositiveReal> {
    cadmpeg_ir::scalar::PositiveReal::new(value * 1000.0)
}

#[allow(clippy::too_many_arguments)]
fn synthesize_closed_edge_vertex_with_curve_index_and_budget(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    scope: &IdScope,
    edge: &Node,
    curve: &CurveId,
    curve_index: usize,
    range: Option<[f64; 2]>,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
    tolerance: Option<cadmpeg_ir::scalar::PositiveReal>,
    curve_point_cache: &mut CurvePointCache,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<VertexId>, CodecError> {
    let parameter = {
        let geometry = &ir.model.curves[curve_index].geometry;
        range.map_or_else(
            || match geometry {
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
                    nurbs.knots().first().copied().unwrap_or(0.0)
                }
                _ => 0.0,
            },
            |range| range[0],
        )
    };
    let Some(position) = ({
        let geometry = &ir.model.curves[curve_index].geometry;
        curve_point_cache.point_with_budget(ctx, curve, geometry, parameter, geometry_budget)?
    }) else {
        return Ok(None);
    };
    let point: PointId = scope.id_charged(
        ctx,
        &cadmpeg_ir::identity_component!("point"),
        format_args!("closed-edge-{}", edge.xmt),
    )?;
    let vertex: VertexId = scope.id_charged(
        ctx,
        &cadmpeg_ir::identity_component!("vertex"),
        format_args!("closed-edge-{}", edge.xmt),
    )?;
    super::annotations::note(
        ctx,
        annotations,
        point.as_str(),
        source_stream,
        edge.pos as u64,
        "CLOSED_EDGE_POINT",
    )?;
    super::annotations::exactness(ctx, annotations, point.as_str(), Exactness::Inferred)?;
    super::annotations::note(
        ctx,
        annotations,
        vertex.as_str(),
        source_stream,
        edge.pos as u64,
        "CLOSED_EDGE_VERTEX",
    )?;
    super::annotations::exactness(ctx, annotations, vertex.as_str(), Exactness::Inferred)?;
    ctx.charge_collection_items(1, "nx closed edge points")?;
    ir.model
        .points
        .try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit("nx closed edge points", 0, 1))?;
    ir.model.points.push(Point::new(
        point.try_clone_for_decode(ctx, "nx closed edge point identity")?,
        position,
        None,
    ));
    ctx.charge_collection_items(1, "nx closed edge vertices")?;
    ir.model
        .vertices
        .try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit("nx closed edge vertices", 0, 1))?;
    ir.model.vertices.push(Vertex {
        id: vertex.try_clone_for_decode(ctx, "nx closed edge vertex identity")?,
        point,
        tolerance,
    });
    Ok(Some(vertex))
}

pub(super) fn canonical_trim_range(geometry: &CurveGeometry, raw: [f64; 2]) -> Option<[f64; 2]> {
    match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(_)) => {
            let range = [raw[0] * 1000.0, raw[1] * 1000.0];
            range.into_iter().all(f64::is_finite).then_some(range)
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
            let domain = [*nurbs.knots().first()?, *nurbs.knots().last()?];
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

#[cfg(test)]
pub(super) fn orient_edge_range(
    ir: &CadIr,
    curve: &CurveId,
    range: [f64; 2],
    start: &VertexId,
    end: &VertexId,
    edge_tolerance: Option<f64>,
) -> Option<([f64; 2], bool)> {
    let geometry_budget = GeometryWorkBudget::new(super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK);
    crate::test_support::with_decode_context(|ctx| {
        orient_edge_range_with_budget(
            ctx,
            ir,
            curve,
            range,
            (start, end),
            edge_tolerance,
            &geometry_budget,
        )
    })
}

#[cfg(test)]
fn orient_edge_range_with_budget(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    curve: &CurveId,
    range: [f64; 2],
    (start, end): (&VertexId, &VertexId),
    edge_tolerance: Option<f64>,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Option<([f64; 2], bool)> {
    let geometry = &ir
        .model
        .curves
        .iter()
        .find(|candidate| candidate.id == *curve)?
        .geometry;
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
        Some((
            point.position().get(),
            vertex.tolerance.map(cadmpeg_ir::scalar::PositiveReal::get),
        ))
    };
    let (start_position, start_tolerance) = vertex_position(start)?;
    let (end_position, end_tolerance) = vertex_position(end)?;
    let procedural_curve = ir
        .model
        .procedural_curves
        .iter()
        .any(|procedural| ir.model.procedural_curve_owner(&procedural.id) == Some(curve));
    let mut curve_point_cache = CurvePointCache::default();
    orient_edge_range_for_geometry_with_budget(
        ctx,
        geometry,
        curve,
        range,
        start_position,
        start_tolerance,
        end_position,
        end_tolerance,
        edge_tolerance,
        procedural_curve,
        &mut curve_point_cache,
        geometry_budget,
    )
    .expect("evaluator allocation succeeds")
}

const MAX_CURVE_POINT_CACHE_ENTRIES: usize = 131_072;

#[derive(Default)]
struct CurvePointCache {
    entries: BTreeMap<CurveId, BTreeMap<u64, Option<FinitePoint3>>>,
    len: usize,
}

impl CurvePointCache {
    fn point_with_budget(
        &mut self,
        ctx: &DecodeContext<'_>,
        curve: &CurveId,
        geometry: &CurveGeometry,
        parameter: f64,
        geometry_budget: &GeometryWorkBudget<'_>,
    ) -> Result<Option<FinitePoint3>, CodecError> {
        let bits = parameter.to_bits();
        if let Some(point) = self.entries.get(curve).and_then(|values| values.get(&bits)) {
            return Ok(*point);
        }
        let point = finite_or_refusal(curve_point_with_budget(
            geometry,
            parameter,
            geometry_budget,
        ))?;
        if self.len < MAX_CURVE_POINT_CACHE_ENTRIES {
            if !self.entries.contains_key(curve) {
                ctx.charge_collection_items(1, "nx curve point cache curves")?;
                self.entries.insert(
                    curve.try_clone_for_decode(ctx, "nx curve point cache identity")?,
                    BTreeMap::new(),
                );
            }
            if let Some(values) = self.entries.get_mut(curve) {
                ctx.charge_collection_items(1, "nx curve point cache values")?;
                values.insert(bits, point);
                self.len += 1;
            }
        }
        Ok(point)
    }
}

#[allow(clippy::too_many_arguments)]
fn orient_edge_range_for_geometry_with_budget(
    ctx: &DecodeContext<'_>,
    geometry: &CurveGeometry,
    curve: &CurveId,
    range: [f64; 2],
    start_position: Point3,
    start_tolerance: Option<f64>,
    end_position: Point3,
    end_tolerance: Option<f64>,
    edge_tolerance: Option<f64>,
    procedural_curve: bool,
    curve_point_cache: &mut CurvePointCache,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<([f64; 2], bool)>, CodecError> {
    let range = if range[0] <= range[1] {
        range
    } else {
        [range[1], range[0]]
    };
    let range = match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(_)) => {
            let sweep = range[1] - range[0];
            if !(0.0..=std::f64::consts::TAU).contains(&sweep) {
                return Ok(None);
            }
            let start = range[0].rem_euclid(std::f64::consts::TAU);
            [start, start + sweep]
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(_)) => {
            let sweep = range[1] - range[0];
            if !(0.0..=std::f64::consts::TAU).contains(&sweep) {
                return Ok(None);
            }
            let start = range[0].rem_euclid(std::f64::consts::TAU);
            [start, start + sweep]
        }
        _ => range,
    };
    let at = match (
        curve_point_cache.point_with_budget(ctx, curve, geometry, range[0], geometry_budget)?,
        curve_point_cache.point_with_budget(ctx, curve, geometry, range[1], geometry_budget)?,
    ) {
        (Some(start), Some(end)) => [start.get(), end.get()],
        _ if procedural_curve => {
            return Ok(Some((range, false)));
        }
        _ => return Ok(None),
    };
    let allowance = [edge_tolerance, start_tolerance, end_tolerance]
        .into_iter()
        .flatten()
        .fold(0.0_f64, f64::max);
    if Point3::distance(at[0], start_position) <= allowance
        && Point3::distance(at[1], end_position) <= allowance
    {
        Ok(Some((range, false)))
    } else if Point3::distance(at[1], start_position) <= allowance
        && Point3::distance(at[0], end_position) <= allowance
    {
        Ok(Some((range, true)))
    } else {
        Ok(None)
    }
}

pub(super) fn unknown_stream(
    ctx: &DecodeContext<'_>,
    si: usize,
    stream: &Stream,
) -> Result<UnknownRecord, CodecError> {
    let data = ctx.copy_retained(&stream.inflated, "retain NX unknown stream")?;
    unknown_stream_record(ctx, si, stream, Some(data))
}

pub(super) fn unknown_stream_metadata(
    ctx: &DecodeContext<'_>,
    si: usize,
    stream: &Stream,
) -> Result<UnknownRecord, CodecError> {
    unknown_stream_record(ctx, si, stream, None)
}

pub(super) fn retain_unknown_stream_data(
    ctx: &DecodeContext<'_>,
    stream: &Stream,
    unknown: &mut UnknownRecord,
) -> Result<(), CodecError> {
    if unknown.data().is_none() {
        unknown.retain_data(ctx.copy_retained(&stream.inflated, "retain NX unknown stream")?);
    }
    Ok(())
}

fn unknown_stream_record(
    ctx: &DecodeContext<'_>,
    si: usize,
    stream: &Stream,
    data: Option<Vec<u8>>,
) -> Result<UnknownRecord, CodecError> {
    let id = render_retained_text(
        ctx,
        format_args!("nx:container:parasolid#{si}"),
        "nx unknown stream id",
    )?;
    let id = UnknownId::mint(id).map_err(|error| CodecError::Malformed(error.to_string()))?;
    let offset = stream.file_offset as u64;
    match data {
        Some(data) => Ok(UnknownRecord::retained(id, offset, data, Vec::new())),
        None => {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(stream.inflated.len()),
                "hash NX unknown stream",
            )?;
            let digest = render_retained_text(
                ctx,
                HexDigest(sha256(&stream.inflated)),
                "nx unknown stream digest",
            )?;
            Ok(UnknownRecord::unavailable(
                id,
                offset,
                cadmpeg_core::decode::u64_from_index(stream.inflated.len()),
                digest,
                Vec::new(),
            ))
        }
    }
}

/// Builds source metadata from classified layers and the container scan.
pub(super) fn source_meta(
    ctx: &DecodeContext<'_>,
    scan: &Scan,
    dialects: &DialectLayers,
) -> Result<SourceMeta, cadmpeg_core::CodecError> {
    let mut attributes = BTreeMap::new();
    insert_source_attribute(
        ctx,
        &mut attributes,
        "file_size",
        scan.container.physical_size,
    )?;
    insert_source_attribute(
        ctx,
        &mut attributes,
        "directory_entries",
        scan.container.entries.len(),
    )?;
    insert_source_attribute(
        ctx,
        &mut attributes,
        "header_entry_count",
        scan.container.entry_count(crate::container::Region::Header),
    )?;
    if let crate::container::ContainerLayout::Modern {
        footer_offset,
        footer_fingerprint,
        ..
    } = scan.container.layout
    {
        insert_source_attribute(ctx, &mut attributes, "footer_offset", footer_offset)?;
        insert_source_attribute(
            ctx,
            &mut attributes,
            "footer_entry_count",
            scan.container.entry_count(crate::container::Region::Footer),
        )?;
        insert_source_attribute(
            ctx,
            &mut attributes,
            "footer_fingerprint",
            format_args!("{:08x}", assemble_u32_be(footer_fingerprint)),
        )?;
    }
    let (control_count, classified_control_count) =
        offset_store_control_counts(ctx, &scan.container)?;
    if control_count != 0 {
        insert_source_attribute(
            ctx,
            &mut attributes,
            "offset_store_control_count",
            control_count,
        )?;
        insert_source_attribute(
            ctx,
            &mut attributes,
            "classified_offset_store_control_count",
            classified_control_count,
        )?;
        insert_source_attribute(
            ctx,
            &mut attributes,
            "unclassified_offset_store_control_count",
            control_count - classified_control_count,
        )?;
    }
    insert_source_attribute(
        ctx,
        &mut attributes,
        "partition_streams",
        scan.count(StreamKind::Partition),
    )?;
    insert_source_attribute(
        ctx,
        &mut attributes,
        "deltas_streams",
        scan.count(StreamKind::Deltas),
    )?;
    insert_source_attribute(
        ctx,
        &mut attributes,
        "plain_streams",
        scan.count(StreamKind::Plain),
    )?;
    for (index, path) in scan
        .container
        .external_reference_paths(ctx)?
        .into_iter()
        .enumerate()
    {
        insert_source_attribute(
            ctx,
            &mut attributes,
            format_args!("external_reference.{index}"),
            path,
        )?;
    }
    if let Some((_, table)) = scan.container.rmfastload_object_id_table() {
        insert_source_attribute(
            ctx,
            &mut attributes,
            "rmfastload_active_object_count",
            table.object_ids.as_slice().len(),
        )?;
    }
    let mut preview_count = 0usize;
    for entry in scan
        .container
        .entries
        .iter()
        .filter(|entry| entry.name == "/Root/images/preview")
    {
        let Some((offset, size)) = entry.file_span() else {
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
        insert_source_attribute(
            ctx,
            &mut attributes,
            format_args!("jpeg_preview_{preview_count}_width"),
            width,
        )?;
        insert_source_attribute(
            ctx,
            &mut attributes,
            format_args!("jpeg_preview_{preview_count}_height"),
            height,
        )?;
        insert_source_attribute(
            ctx,
            &mut attributes,
            format_args!("jpeg_preview_{preview_count}_precision"),
            precision,
        )?;
        insert_source_attribute(
            ctx,
            &mut attributes,
            format_args!("jpeg_preview_{preview_count}_components"),
            components,
        )?;
        insert_source_attribute(
            ctx,
            &mut attributes,
            format_args!("jpeg_preview_{preview_count}_byte_len"),
            payload.len(),
        )?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(payload.len()),
            "hash NX source preview",
        )?;
        insert_source_attribute(
            ctx,
            &mut attributes,
            format_args!("jpeg_preview_{preview_count}_sha256"),
            HexDigest(sha256(payload)),
        )?;
        preview_count += 1;
    }
    insert_source_attribute(ctx, &mut attributes, "jpeg_preview_count", preview_count)?;
    for (index, stream) in scan
        .streams
        .iter()
        .filter(|stream| stream.kind() == StreamKind::Deltas)
        .enumerate()
    {
        let census = crate::deltas::census::walk(ctx, &stream.inflated)?;
        if census.transmit_header.is_some() {
            insert_source_attribute(
                ctx,
                &mut attributes,
                format_args!("deltas.{index}.transmit_headers"),
                "1",
            )?;
        }
        insert_source_attribute(
            ctx,
            &mut attributes,
            format_args!("deltas.{index}.grammar"),
            "typed_status_framed_records",
        )?;
        insert_source_attribute(
            ctx,
            &mut attributes,
            format_args!("deltas.{index}.bytes_decoded"),
            census.bytes_decoded(),
        )?;
        if !census.body_revisions.is_empty() {
            insert_source_attribute(
                ctx,
                &mut attributes,
                format_args!("deltas.{index}.body_revisions"),
                census.body_revisions.len(),
            )?;
        }
        if !census.term_use_numeric_tails.is_empty() {
            insert_source_attribute(
                ctx,
                &mut attributes,
                format_args!("deltas.{index}.term_use_numeric_tails"),
                census.term_use_numeric_tails.len(),
            )?;
        }
        if !census.tagged_reference_lanes.is_empty() {
            insert_source_attribute(
                ctx,
                &mut attributes,
                format_args!("deltas.{index}.tagged_reference_lanes"),
                census.tagged_reference_lanes.len(),
            )?;
        }
        if !census.reference_type_maps.is_empty() {
            insert_source_attribute(
                ctx,
                &mut attributes,
                format_args!("deltas.{index}.reference_type_maps"),
                census.reference_type_maps.len(),
            )?;
        }
        if !census.reference_state_packets.is_empty() {
            insert_source_attribute(
                ctx,
                &mut attributes,
                format_args!("deltas.{index}.reference_state_packets"),
                census.reference_state_packets.len(),
            )?;
        }
        if !census.reference_marker_packets.is_empty() {
            insert_source_attribute(
                ctx,
                &mut attributes,
                format_args!("deltas.{index}.reference_marker_packets"),
                census.reference_marker_packets.len(),
            )?;
        }
        if !census.inline_schema_declarations.is_empty() {
            insert_source_attribute(
                ctx,
                &mut attributes,
                format_args!("deltas.{index}.inline_schema_declarations"),
                census.inline_schema_declarations.len(),
            )?;
        }
        for (name, count) in census.full_counts(ctx)? {
            insert_source_attribute(
                ctx,
                &mut attributes,
                format_args!("deltas.{index}.full.{name}"),
                count,
            )?;
        }
        for (name, count) in census.tombstone_counts(ctx)? {
            insert_source_attribute(
                ctx,
                &mut attributes,
                format_args!("deltas.{index}.tombstone.{name}"),
                count,
            )?;
        }
    }
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(attributes.len()),
        "nx source attribute names",
    )?;
    Ok(SourceMeta::classified(
        dialects.try_clone_for_decode(ctx)?,
        cadmpeg_core::text::named_entries("the nx part", attributes)?,
    ))
}

struct CountBytes(usize);

impl fmt::Write for CountBytes {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0 = self.0.checked_add(text.len()).ok_or(fmt::Error)?;
        Ok(())
    }
}

struct HexDigest([u8; 32]);

impl Display for HexDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

pub(super) fn render_retained_text(
    ctx: &DecodeContext<'_>,
    value: impl Display,
    operation: &'static str,
) -> Result<String, CodecError> {
    let mut count = CountBytes(0);
    write!(&mut count, "{value}").map_err(|_| ctx.refuse_codec_limit(operation, 0, u64::MAX))?;
    ctx.charge_retained(cadmpeg_core::decode::u64_from_index(count.0), operation)?;
    let mut text = String::new();
    text.try_reserve_exact(count.0)
        .map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    write!(&mut text, "{value}").map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    Ok(text)
}

fn insert_source_attribute(
    ctx: &DecodeContext<'_>,
    attributes: &mut BTreeMap<String, String>,
    key: impl Display,
    value: impl Display,
) -> Result<(), CodecError> {
    let mut key_len = CountBytes(0);
    let mut value_len = CountBytes(0);
    write!(&mut key_len, "{key}")
        .map_err(|_| ctx.refuse_codec_limit("nx source attribute text", 0, u64::MAX))?;
    write!(&mut value_len, "{value}")
        .map_err(|_| ctx.refuse_codec_limit("nx source attribute text", 0, u64::MAX))?;
    let text_len = key_len
        .0
        .checked_add(value_len.0)
        .ok_or_else(|| ctx.refuse_codec_limit("nx source attribute text", 0, u64::MAX))?;
    ctx.charge_collection_items(1, "nx source attributes")?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(text_len),
        "nx source attribute text",
    )?;
    let mut key_text = String::new();
    key_text
        .try_reserve_exact(key_len.0)
        .map_err(|_| ctx.refuse_codec_limit("nx source attribute text", 0, 1))?;
    write!(&mut key_text, "{key}")
        .map_err(|_| ctx.refuse_codec_limit("nx source attribute text", 0, 1))?;
    let mut value_text = String::new();
    value_text
        .try_reserve_exact(value_len.0)
        .map_err(|_| ctx.refuse_codec_limit("nx source attribute text", 0, 1))?;
    write!(&mut value_text, "{value}")
        .map_err(|_| ctx.refuse_codec_limit("nx source attribute text", 0, 1))?;
    attributes.insert(key_text, value_text);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::geometry_work::GeometryWorkBudget;
    use super::{source_meta, unknown_stream, unknown_stream_metadata, CurvePointCache};
    use crate::container::Container;
    use crate::decode::Scan;
    use crate::parasolid::Stream;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::geometry::CurveGeometry;
    use cadmpeg_ir::geometry::SolvedCurveGeometry;
    use cadmpeg_ir::ids::CurveId;
    use cadmpeg_ir::math::Point3;

    fn empty_source_scan() -> Scan<'static> {
        Scan {
            container: Container {
                data: Vec::new().into(),
                physical_size: 0,
                layout: crate::container::test_modern_layout(0x06),
                entries: Vec::new(),
                fastload_table: None,
                indexed_section_layouts: std::sync::OnceLock::new(),
                om_section_cache: std::sync::OnceLock::new(),
            },
            streams: Vec::new(),
        }
    }

    fn preview_stream(bytes: Vec<u8>) -> Stream {
        Stream {
            file_offset: 0,
            consumed: 0,
            inflated: bytes,
            body: crate::parasolid::StreamBody::Preview,
        }
    }

    #[test]
    fn unknown_stream_metadata_refuses_identity_text_at_retained_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            unknown_stream_metadata(&ctx, 0, &preview_stream(Vec::new())),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "nx unknown stream id"
        ));
    }

    #[test]
    fn unknown_stream_metadata_refuses_digest_text_at_retained_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = "nx:container:parasolid#0".len() as u64;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            unknown_stream_metadata(&ctx, 0, &preview_stream(Vec::new())),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "nx unknown stream digest"
        ));
    }

    #[test]
    fn unknown_stream_metadata_refuses_digest_work_at_caller_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            unknown_stream_metadata(&ctx, 0, &preview_stream(vec![7])),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "hash NX unknown stream"
        ));
    }

    #[test]
    fn unknown_stream_metadata_preserves_identity_and_digest_under_service_profile() {
        let stream = preview_stream(vec![7]);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let unknown = unknown_stream_metadata(&ctx, 0, &stream).unwrap();
        assert_eq!(unknown.id().as_str(), "nx:container:parasolid#0");
        assert_eq!(unknown.offset(), 0);
        assert_eq!(unknown.data(), None);
        let wire = serde_json::to_value(&unknown).unwrap();
        assert_eq!(
            wire["retention"]["sha256"],
            cadmpeg_ir::hash::sha256_hex(&stream.inflated)
        );
    }

    #[test]
    fn source_meta_refuses_first_attribute_node_at_collection_limit() {
        let scan = empty_source_scan();
        let (dialects, _) = crate::test_support::with_decode_context(|ctx| {
            crate::dialect::classify_layers(ctx, &scan)
        })
        .unwrap()
        .into_report_parts();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            source_meta(&ctx, &scan, &dialects),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx source attributes"
        ));
    }

    #[test]
    fn source_meta_refuses_first_attribute_text_at_retained_limit() {
        let scan = empty_source_scan();
        let (dialects, _) = crate::test_support::with_decode_context(|ctx| {
            crate::dialect::classify_layers(ctx, &scan)
        })
        .unwrap()
        .into_report_parts();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(matches!(
            source_meta(&ctx, &scan, &dialects),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "nx source attribute text"
        ));
    }

    #[test]
    fn source_meta_refuses_second_map_nodes_at_collection_limit() {
        let scan = empty_source_scan();
        let (dialects, _) = crate::test_support::with_decode_context(|ctx| {
            crate::dialect::classify_layers(ctx, &scan)
        })
        .unwrap()
        .into_report_parts();
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (service_ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let expected = source_meta(&service_ctx, &scan, &dialects).unwrap();
        assert_eq!(expected.attributes["file_size"], "0");
        let mut limited_policy = DecodePolicy::service();
        limited_policy.limits.max_collection_items = expected.attributes.len() as u64;
        let (limited_ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &limited_policy).unwrap();
        assert!(matches!(
            source_meta(&limited_ctx, &scan, &dialects),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx source attribute names"
        ));
    }

    #[test]
    fn unknown_stream_copy_refuses_when_retained_budget_is_exhausted() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::default();
        policy.limits.max_retained_bytes = 2;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("bounded test input");
        let stream = Stream {
            file_offset: 0,
            consumed: 0,
            inflated: vec![1, 2, 3],
            body: crate::parasolid::StreamBody::Parasolid {
                subtype: crate::parasolid::ParasolidSubtype::Partition,
                schema: None,
            },
        };

        assert!(matches!(
            unknown_stream(&ctx, 0, &stream),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                    && limit.operation == "retain NX unknown stream"
        ));
    }

    #[test]
    fn curve_point_cache_reuses_an_exact_parameter_evaluation() {
        let curve = CurveId::mint("test:model:entity#synthetic:curve").expect("identity grammar");
        let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(1.0, 2.0, 3.0), Point3::new(5.0, 7.0, 9.0)],
                None,
                false,
            )
            .expect("valid test curve"),
        ));
        let geometry_budget = GeometryWorkBudget::new(1024);
        let mut cache = CurvePointCache::default();

        crate::test_support::with_decode_context(|ctx| {
            let first = cache
                .point_with_budget(ctx, &curve, &geometry, 0.25, &geometry_budget)
                .expect("evaluator allocation succeeds")
                .expect("NURBS evaluation");
            let remaining_after_first = geometry_budget.remaining();
            let second = cache
                .point_with_budget(ctx, &curve, &geometry, 0.25, &geometry_budget)
                .expect("evaluator allocation succeeds")
                .expect("cached NURBS evaluation");

            assert_eq!(first, second);
            assert_eq!(geometry_budget.remaining(), remaining_after_first);
        });
    }
}
