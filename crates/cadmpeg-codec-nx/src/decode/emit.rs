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
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::eval::finite_or_refusal;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::{
    pcurve::Pcurve, Curve, CurveGeometry, FitTolerance, IntcurveSupportContext,
    IntcurveSupportSide, ProceduralCurve, ProceduralCurveDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, SupportPcurve, Surface, SurfaceCurveFamily, SurfaceGeometry,
};
use cadmpeg_ir::hash::{sha256, LowerHex};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display, Write as _};

const EPS_EMIT_CANONICAL_TRIM_RANGE_E6: f64 = 1.0e-6;

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

pub(super) struct TopologyStream<'inputs> {
    pub(super) stream_index: usize,
    pub(super) graph: &'inputs Graph,
    pub(super) points: &'inputs BTreeMap<u32, PointId>,
    pub(super) surfaces: &'inputs BTreeMap<u32, SurfaceId>,
    pub(super) curves: &'inputs BTreeMap<u32, CurveId>,
    pub(super) pcurves: &'inputs BTreeMap<u32, PcurveId>,
    pub(super) pcurve_supports: &'inputs BTreeMap<u32, SurfaceId>,
    pub(super) trim_ranges: &'inputs BTreeMap<u32, [f64; 2]>,
    pub(super) source_stream: &'inputs cadmpeg_ir::annotations::StreamHandle,
    pub(super) intersection_starts: IntersectionEntityStarts,
    pub(super) procedural_start: usize,
}

pub(super) struct TopologyBudgets<'inputs> {
    pub(super) exact_transfer: &'inputs TransferBudget<'inputs>,
    pub(super) completion_transfer: &'inputs TransferBudget<'inputs>,
    pub(super) adaptive_geometry: &'inputs GeometryWorkBudget<'inputs>,
    pub(super) completion_geometry: &'inputs GeometryWorkBudget<'inputs>,
}

pub(super) fn emit_topology(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    topology_stream: &TopologyStream<'_>,
    annotations: &mut AnnotationBuilder,
    intersection_index: &mut IntersectionIncidenceIndex,
    topology_budgets: &TopologyBudgets<'_>,
    topology_losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<EndpointWitnesses, CodecError> {
    let &TopologyBudgets {
        exact_transfer: exact_transfer_budget,
        completion_transfer: completion_transfer_budget,
        adaptive_geometry: adaptive_geometry_budget,
        completion_geometry: completion_geometry_budget,
    } = topology_budgets;

    let &TopologyStream {
        stream_index,
        graph,
        points,
        surfaces,
        curves,
        pcurves,
        pcurve_supports,
        trim_ranges,
        source_stream,
        intersection_starts,
        procedural_start,
    } = topology_stream;

    let scope = IdScope::stream_charged(ctx, stream_index)?;
    let mut storage = ctx.reserve_scoped(0, "nx topology emission scratch")?;
    let body_shells = storage.with_storage(|| graph.body_shape_shells(ctx))?;
    let mut valid_face_xmts = BTreeSet::new();
    for shell in ctx.admit_iter(&body_shells, "nx valid topology faces")? {
        for &face in ctx.admit_iter(&shell.faces, "nx valid topology faces")? {
            storage.with_storage(|| {
                ctx.insert_btree_set(&mut valid_face_xmts, face, "nx valid topology faces")
            })?;
        }
    }
    let mut face_loop_rings: BTreeMap<u32, Vec<(u32, Vec<u32>)>> = BTreeMap::new();
    let mut face_loop_failures: BTreeMap<u32, FaceLoopFailure> = BTreeMap::new();
    for &face_xmt in ctx.admit_iter(&valid_face_xmts, "nx face loop ring index")? {
        let rings = storage.with_storage(|| match graph.face_loop_rings(ctx, face_xmt) {
            Ok(rings) => Ok(Ok(rings)),
            Err(FaceLoopError::Invalid(failure)) => Ok(Err(failure)),
            Err(FaceLoopError::Codec(error)) => Err(error),
        })?;
        match rings {
            Ok(rings) => {
                storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut face_loop_rings,
                        face_xmt,
                        rings,
                        "nx face loop ring index",
                    )
                })?;
            }
            Err(failure) => {
                storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut face_loop_failures,
                        face_xmt,
                        failure,
                        "nx face loop failure index",
                    )
                })?;
            }
        }
    }
    let mut valid_loop_rings: BTreeMap<u32, &[u32]> = BTreeMap::new();
    let mut valid_fin_xmts = BTreeSet::new();
    for rings in ctx
        .admit_iter(&face_loop_rings, "nx valid loop ring index")?
        .map(|(_, rings)| rings)
    {
        for (loop_xmt, ring) in ctx.admit_iter(rings, "nx valid loop ring index")? {
            storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut valid_loop_rings,
                    *loop_xmt,
                    ring.as_slice(),
                    "nx valid loop ring index",
                )
            })?;
        }
    }
    for ring in ctx
        .admit_iter(&valid_loop_rings, "nx valid fin nodes")?
        .map(|(_, ring)| *ring)
    {
        for &fin in ctx.admit_iter(ring, "nx valid fin nodes")? {
            storage.with_storage(|| {
                ctx.insert_btree_set(&mut valid_fin_xmts, fin, "nx valid fin nodes")
            })?;
        }
    }
    let mut valid_edge_xmts = BTreeSet::new();
    let mut valid_vertex_xmts = BTreeSet::new();
    for &xmt in ctx.admit_iter(&valid_fin_xmts, "nx valid edge and vertex nodes")? {
        let fields = graph
            .get(ctx, NodeKind::Fin, xmt)?
            .and_then(Node::fin_fields);
        if let Some(edge) = fields.and_then(|fields| fields.edge.map(u32::from)) {
            storage.with_storage(|| {
                ctx.insert_btree_set(&mut valid_edge_xmts, edge, "nx valid edge nodes")
            })?;
        }
        let partner = match fields
            .filter(|fields| fields.other.is_some_and(|target| u32::from(target) > 1))
        {
            Some(fields) => graph.get_target(ctx, NodeKind::Fin, fields.other)?,
            None => None,
        };
        let partner_vertex = partner
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
            storage.with_storage(|| {
                ctx.insert_btree_set(&mut valid_vertex_xmts, vertex, "nx valid vertex nodes")
            })?;
        }
    }
    // The first body-shape shell of each body locates a body that has no
    // BODY record of its own.
    let mut body_shell_positions: BTreeMap<u32, usize> = BTreeMap::new();
    for shell in ctx.admit_iter(&body_shells, "nx topology body nodes")? {
        let shell = shell.node;
        let Some(body) = shell
            .shell_fields()
            .and_then(|fields| fields.body.map(u32::from))
        else {
            continue;
        };
        if !ctx.contains_key_btree_map(&body_shell_positions, &body, "nx topology body nodes")? {
            storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut body_shell_positions,
                    body,
                    shell.pos(),
                    "nx topology body nodes",
                )
            })?;
        }
    }
    // Emitted bodies by record identity, with their arena positions.
    let mut bodies: BTreeMap<u32, usize> = BTreeMap::new();
    for (&body_xmt, &shell_pos) in ctx.admit_iter(&body_shell_positions, "nx emitted bodies")? {
        let id: BodyId =
            scope.id_charged(ctx, &cadmpeg_ir::identity_component!("body"), body_xmt)?;
        if let Some(node) = graph.get(ctx, NodeKind::Body, body_xmt)? {
            annotate_node(ctx, annotations, id.as_str(), source_stream, node, "BODY")?;
        } else {
            annotations.note(
                ctx,
                id.as_str(),
                source_stream,
                cadmpeg_core::decode::u64_from_index(shell_pos),
                Some("UNRESOLVED_BODY_REFERENCE"),
            )?;
            annotations.exactness(ctx, id.as_str(), Exactness::Unknown)?;
        }
        storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut bodies,
                body_xmt,
                ir.model.bodies.len(),
                "nx emitted body index",
            )
        })?;
        let body = Body {
            id,
            kind: cadmpeg_ir::topology::BodyKind::Solid,
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        };
        ctx.push_vec(&mut ir.model.bodies, body, "nx emitted bodies")?;
    }

    // Valid faces of each shell record, in record order, whose surface
    // resolved to a carrier.
    let mut shell_faces_by_xmt: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for face in graph.of_kind(NodeKind::Face) {
        ctx.charge_work(1, "nx shell faces")?;
        if !ctx.contains_btree_set(&valid_face_xmts, &face.xmt(), "nx shell faces")? {
            continue;
        }
        let Some(face_fields) = face.face_fields() else {
            continue;
        };
        let Some(shell) = face_fields.shell.map(u32::from) else {
            continue;
        };
        let has_surface = match face_fields.surface {
            Some(surface) => {
                ctx.contains_key_btree_map(surfaces, &u32::from(surface), "nx shell faces")?
            }
            None => false,
        };
        if has_surface {
            storage.with_storage(|| {
                ctx.push_btree_group(
                    &mut shell_faces_by_xmt,
                    shell,
                    face.xmt(),
                    "nx shell faces",
                    "nx shell faces",
                )
            })?;
        }
    }
    // Regions by record identity: identity, owning body record and arena
    // position.
    let mut regions: BTreeMap<u32, (RegionId, u32, usize)> = BTreeMap::new();
    let mut shells: BTreeMap<u32, ShellId> = BTreeMap::new();
    for shell in ctx.admit_iter(&body_shells, "nx emitted shells")? {
        let node = shell.node;
        let Some(fields) = node.shell_fields() else {
            continue;
        };
        let Some(body_xmt) = fields.body.map(u32::from) else {
            continue;
        };
        let Some(&body_index) = ctx.get_btree_map(&bodies, &body_xmt, "nx emitted body index")?
        else {
            continue;
        };
        let Some(region_xmt) = fields.region.map(u32::from) else {
            continue;
        };
        let region_index =
            match ctx.get_btree_map(&regions, &region_xmt, "nx emitted region index")? {
                Some(&(_, owner, index)) => {
                    if owner != body_xmt {
                        continue;
                    }
                    index
                }
                None => {
                    let region: RegionId = scope.id_charged(
                        ctx,
                        &cadmpeg_ir::identity_component!("region"),
                        region_xmt,
                    )?;
                    if let Some(region_node) = graph.get(ctx, NodeKind::Region, region_xmt)? {
                        annotate_node(
                            ctx,
                            annotations,
                            region.as_str(),
                            source_stream,
                            region_node,
                            "REGION",
                        )?;
                    } else {
                        annotations.note(
                            ctx,
                            region.as_str(),
                            source_stream,
                            cadmpeg_core::decode::u64_from_index(node.pos()),
                            Some("UNRESOLVED_REGION_REFERENCE"),
                        )?;
                        annotations.exactness(ctx, region.as_str(), Exactness::Unknown)?;
                    }
                    annotations.derived(ctx, region.as_str(), "body")?;
                    let Some(body) = ir.model.bodies.get(body_index) else {
                        continue;
                    };
                    let body_region =
                        region.try_clone_for_decode(ctx, "nx body region identity")?;
                    let indexed = storage.with_storage(|| {
                        region.try_clone_for_decode(ctx, "nx indexed region identity")
                    })?;
                    let region_index = ir.model.regions.len();
                    let record = Region {
                        id: region,
                        body: body
                            .id
                            .try_clone_for_decode(ctx, "nx region body identity")?,
                        shells: Vec::new(),
                    };
                    ctx.push_vec(&mut ir.model.regions, record, "nx emitted regions")?;
                    if let Some(parent) = ir.model.bodies.get_mut(body_index) {
                        ctx.push_vec(&mut parent.regions, body_region, "nx body regions")?;
                    }
                    storage.with_storage(|| {
                        ctx.insert_btree_map(
                            &mut regions,
                            region_xmt,
                            (indexed, body_xmt, region_index),
                            "nx emitted region index",
                        )
                    })?;
                    region_index
                }
            };
        let shell_id: ShellId =
            scope.id_charged(ctx, &cadmpeg_ir::identity_component!("shell"), node.xmt())?;
        annotate_node(
            ctx,
            annotations,
            shell_id.as_str(),
            source_stream,
            node,
            "SHELL",
        )?;
        let mut shell_faces = Vec::new();
        if let Some(face_xmts) =
            ctx.get_btree_map(&shell_faces_by_xmt, &node.xmt(), "nx shell faces")?
        {
            for &face_xmt in ctx.admit_iter(face_xmts, "nx shell faces")? {
                let face: FaceId =
                    scope.id_charged(ctx, &cadmpeg_ir::identity_component!("face"), face_xmt)?;
                ctx.push_vec(&mut shell_faces, face, "nx shell faces")?;
            }
        }
        let Some(region) = ir.model.regions.get(region_index) else {
            continue;
        };
        let shell = match Shell::new(
            shell_id.try_clone_for_decode(ctx, "nx shell identity copy")?,
            region
                .id
                .try_clone_for_decode(ctx, "nx shell region identity")?,
            shell_faces,
            Vec::new(),
            Vec::new(),
        ) {
            Ok(shell) => shell,
            Err(message) => {
                return Err(CodecError::Malformed(ctx.format_retained(
                    format_args!("{message}"),
                    "nx shell refusal text",
                )?));
            }
        };
        ctx.push_vec(&mut ir.model.shells, shell, "nx emitted shells")?;
        let indexed = storage
            .with_storage(|| shell_id.try_clone_for_decode(ctx, "nx emitted shell index"))?;
        if let Some(parent) = ir.model.regions.get_mut(region_index) {
            ctx.push_vec(&mut parent.shells, shell_id, "nx region shells")?;
        }
        storage.with_storage(|| {
            ctx.insert_btree_map(&mut shells, node.xmt(), indexed, "nx emitted shell index")
        })?;
    }
    let mut vertices: BTreeMap<u32, VertexId> = BTreeMap::new();
    let mut vertex_positions: BTreeMap<VertexId, (Point3, Option<f64>)> = BTreeMap::new();
    {
        let mut point_positions: BTreeMap<&PointId, Point3> = BTreeMap::new();
        for point in ctx.admit_iter(&ir.model.points, "nx point position index")? {
            if !ctx.contains_key_btree_map(
                &point_positions,
                &point.id,
                "nx point position index",
            )? {
                storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut point_positions,
                        &point.id,
                        point.position().get(),
                        "nx point position index",
                    )
                })?;
            }
        }
        for node in graph.of_kind(NodeKind::Vertex) {
            ctx.charge_work(1, "nx emitted vertices")?;
            if !ctx.contains_btree_set(&valid_vertex_xmts, &node.xmt(), "nx emitted vertices")? {
                continue;
            }
            let Some(fields) = node.vertex_fields() else {
                continue;
            };
            let Some(point_ref) = (match fields.point {
                Some(target) => {
                    ctx.get_btree_map(points, &u32::from(target), "nx point node index")?
                }
                None => None,
            }) else {
                continue;
            };
            let Some(&point_position) =
                ctx.get_btree_map(&point_positions, point_ref, "nx point position index")?
            else {
                continue;
            };
            let tolerance = decoded_tolerance(fields.tolerance());
            let vertex: VertexId =
                scope.id_charged(ctx, &cadmpeg_ir::identity_component!("vertex"), node.xmt())?;
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
                    .derived(ctx, &vertex, "tolerance")
                    .map_err(cadmpeg_core::CodecError::from)?;
            }
            storage.with_storage(|| {
                let indexed = vertex.try_clone_for_decode(ctx, "nx indexed vertex identity")?;
                ctx.insert_btree_map(
                    &mut vertices,
                    node.xmt(),
                    indexed,
                    "nx emitted vertex index",
                )?;
                let positioned = vertex.try_clone_for_decode(ctx, "nx vertex position index")?;
                ctx.insert_btree_map(
                    &mut vertex_positions,
                    positioned,
                    (
                        point_position,
                        tolerance.map(cadmpeg_ir::scalar::PositiveReal::get),
                    ),
                    "nx vertex position index",
                )
            })?;
            let record = Vertex {
                id: vertex,
                point: point_ref.try_clone_for_decode(ctx, "nx vertex point identity")?,
                tolerance,
            };
            ctx.push_vec(&mut ir.model.vertices, record, "nx emitted vertices")?;
        }
    }
    let mut pcurve_indices: BTreeMap<PcurveId, usize> = BTreeMap::new();
    for (index, pcurve) in ctx
        .admit_iter(&ir.model.pcurves, "nx pcurve index")?
        .enumerate()
    {
        storage.with_storage(|| {
            let id = pcurve
                .id
                .try_clone_for_decode(ctx, "nx indexed pcurve identity")?;
            ctx.insert_btree_map(&mut pcurve_indices, id, index, "nx pcurve index")
        })?;
    }
    let mut curve_indices: BTreeMap<CurveId, usize> = BTreeMap::new();
    for (index, curve) in ctx
        .admit_iter(&ir.model.curves, "nx curve index")?
        .enumerate()
    {
        if !ctx.contains_key_btree_map(&curve_indices, &curve.id, "nx curve index")? {
            storage.with_storage(|| {
                let id = curve
                    .id
                    .try_clone_for_decode(ctx, "nx indexed curve identity")?;
                ctx.insert_btree_map(&mut curve_indices, id, index, "nx curve index")
            })?;
        }
    }
    let mut procedural_curve_ids: BTreeSet<CurveId> = BTreeSet::new();
    {
        let (owners, _owner_storage) = procedural_curve_owners(ctx, &ir.model.curves)?;
        for procedural in
            ctx.admit_iter(&ir.model.procedural_curves, "nx procedural curve index")?
        {
            if let Some(Some(owner)) =
                ctx.get_hash_map(&owners, &procedural.id, "nx procedural curve owners")?
            {
                storage.with_storage(|| {
                    let owner =
                        owner.try_clone_for_decode(ctx, "nx indexed procedural curve identity")?;
                    ctx.insert_btree_set(
                        &mut procedural_curve_ids,
                        owner,
                        "nx procedural curve index",
                    )
                })?;
            }
        }
    }
    let mut curve_point_cache = CurvePointCache::new(ctx)?;
    let mut edges: BTreeMap<u32, EdgeId> = BTreeMap::new();
    for node in graph.of_kind(NodeKind::Edge) {
        ctx.charge_work(1, "nx emitted edges")?;
        if !ctx.contains_btree_set(&valid_edge_xmts, &node.xmt(), "nx emitted edges")? {
            continue;
        }
        let Some(fields) = node.edge_fields() else {
            continue;
        };
        let Some(fin) = graph.get_target(ctx, NodeKind::Fin, fields.fin)? else {
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
        let mut curve = match curve_xmt {
            Some(xmt) => ctx
                .get_btree_map(curves, &xmt, "nx curve node index")?
                .map(|id| id.try_clone_for_decode(ctx, "nx edge curve identity"))
                .transpose()?,
            None => None,
        };
        let mut param_range = match curve_xmt {
            Some(xmt) => ctx
                .get_btree_map(trim_ranges, &xmt, "nx curve trim ranges")?
                .copied(),
            None => None,
        };
        if curve.is_none() {
            let lifted = match curve_xmt {
                Some(xmt) => lift_parametric_edge(
                    ctx,
                    ir,
                    &ParametricEdgeLookups {
                        pcurves,
                        pcurve_supports,
                        pcurve_indices: &pcurve_indices,
                    },
                    xmt,
                    param_range,
                )?,
                None => None,
            };
            if let Some((surface, pcurve, parameter_range)) = lifted {
                let carrier: CurveId = scope.id_charged(
                    ctx,
                    &cadmpeg_ir::identity_component!("edge-parametric-curve"),
                    node.xmt(),
                )?;
                let construction: ProceduralCurveId = scope.id_charged(
                    ctx,
                    &cadmpeg_ir::identity_component!("edge-parametric-construction"),
                    node.xmt(),
                )?;
                annotations.note(
                    ctx,
                    carrier.as_str(),
                    source_stream,
                    cadmpeg_core::decode::u64_from_index(node.pos()),
                    Some("PARAMETRIC_SURFACE_CURVE"),
                )?;
                annotations.derived(ctx, carrier.as_str(), "geometry")?;
                let carrier_curve = Curve {
                    id: carrier.try_clone_for_decode(ctx, "nx parametric edge carrier")?,
                    geometry: CurveGeometry::Procedural {
                        construction: construction
                            .try_clone_for_decode(ctx, "nx parametric edge construction")?,
                        cache: None,
                    },
                    source_object: None,
                };
                ctx.push_vec(
                    &mut ir.model.curves,
                    carrier_curve,
                    "nx parametric edge curves",
                )?;

                let _attached = ir.model.add_procedural_curve(
                    ctx,
                    &carrier,
                    ProceduralCurve::new(
                        construction,
                        ProceduralCurveDefinition::SurfaceCurve {
                            family: SurfaceCurveFamily::Parametric {
                                context: IntcurveSupportContext::try_new(
                                    [
                                        IntcurveSupportSide {
                                            surface: Some(surface),
                                            pcurve: Some(SupportPcurve::new(pcurve, None)),
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
                )?;
                curve = Some(carrier);
                param_range = None;
            }
        }
        let closed_edge = fin_fields.vertex.is_none()
            && fin_fields.forward.map(u32::from) == Some(fin.xmt())
            && fin_fields.backward.map(u32::from) == Some(fin.xmt());
        let start = match fin_fields.vertex {
            Some(target) => ctx
                .get_btree_map(&vertices, &u32::from(target), "nx emitted vertex index")?
                .map(|id| id.try_clone_for_decode(ctx, "nx edge start vertex"))
                .transpose()?,
            None => None,
        };
        let closed_curve = match (&start, closed_edge, &curve) {
            (None, true, Some(curve)) => ctx
                .get_btree_map(&curve_indices, curve, "nx curve index")?
                .map(|&index| (curve, index)),
            _ => None,
        };
        let start = if start.is_some() || !closed_edge {
            start
        } else if let Some((curve, curve_index)) = closed_curve {
            synthesize_closed_edge_vertex_with_curve_index_and_budget(
                ctx,
                ir,
                annotations,
                &ClosedEdge {
                    scope: &scope,
                    edge: node,
                    curve,
                    curve_index,
                    range: param_range,
                    source_stream,
                    tolerance: decoded_tolerance(fields.tolerance()),
                },
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
            .get_target(ctx, NodeKind::Fin, end_fin)?
            .and_then(Node::fin_fields)
        else {
            continue;
        };
        let mut end = match end_fields.vertex {
            Some(target) => ctx
                .get_btree_map(&vertices, &u32::from(target), "nx emitted vertex index")?
                .map(|id| id.try_clone_for_decode(ctx, "nx edge end vertex"))
                .transpose()?,
            None => None,
        };
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
            scope.id_charged(ctx, &cadmpeg_ir::identity_component!("edge"), node.xmt())?;
        annotate_node(ctx, annotations, id.as_str(), source_stream, node, "EDGE")?;
        if decoded_tolerance(fields.tolerance()).is_some() {
            annotations
                .derived(ctx, &id, "tolerance")
                .map_err(cadmpeg_core::CodecError::from)?;
        }
        if let (Some(carrier), Some(range)) = (&curve, param_range) {
            let curve_index = ctx
                .get_btree_map(&curve_indices, carrier, "nx curve index")?
                .copied();
            let start_position = ctx
                .get_btree_map(&vertex_positions, &start, "nx vertex position index")?
                .copied();
            let end_position = ctx
                .get_btree_map(&vertex_positions, &end, "nx vertex position index")?
                .copied();
            let oriented = if let (
                Some(curve_index),
                Some((start_position, start_tolerance)),
                Some((end_position, end_tolerance)),
            ) = (curve_index, start_position, end_position)
            {
                orient_edge_range_for_geometry_with_budget(
                    ctx,
                    &EdgeGeometryRange {
                        geometry: &ir.model.curves[curve_index].geometry,
                        curve: carrier,
                        range,
                        start_position,
                        start_tolerance,
                        end_position,
                        end_tolerance,
                        edge_tolerance: decoded_tolerance(fields.tolerance())
                            .map(cadmpeg_ir::scalar::PositiveReal::get),
                        procedural_curve: ctx.contains_btree_set(
                            &procedural_curve_ids,
                            carrier,
                            "nx procedural curve index",
                        )?,
                    },
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
        storage.with_storage(|| {
            let indexed = id.try_clone_for_decode(ctx, "nx emitted edge index")?;
            ctx.insert_btree_map(&mut edges, node.xmt(), indexed, "nx emitted edge index")
        })?;
        let edge = Edge {
            id,
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(curve, param_range)
                .map_err(CodecError::malformed)?,
            start,
            end,
            tolerance: decoded_tolerance(fields.tolerance()),
        };
        ctx.push_vec(&mut ir.model.edges, edge, "nx emitted edges")?;
    }
    drop(curve_point_cache);
    let mut edge_curves_by_id: BTreeMap<&EdgeId, &CurveId> = BTreeMap::new();
    for edge in ctx.admit_iter(&ir.model.edges, "nx edge curve index")? {
        if let Some(curve) = edge.curve() {
            storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut edge_curves_by_id,
                    &edge.id,
                    curve,
                    "nx edge curve index",
                )
            })?;
        }
    }
    let mut faces: BTreeMap<u32, FaceId> = BTreeMap::new();
    let mut pending_faces: Vec<PendingFace> = Vec::new();
    for node in graph.of_kind(NodeKind::Face) {
        ctx.charge_work(1, "nx pending faces")?;
        if !ctx.contains_btree_set(&valid_face_xmts, &node.xmt(), "nx pending faces")? {
            continue;
        }
        let Some(fields) = node.face_fields() else {
            continue;
        };
        let Some(shell_ref) = (match fields.shell {
            Some(target) => {
                ctx.get_btree_map(&shells, &u32::from(target), "nx emitted shell index")?
            }
            None => None,
        }) else {
            continue;
        };
        let Some(surface_ref) = (match fields.surface {
            Some(target) => {
                ctx.get_btree_map(surfaces, &u32::from(target), "nx surface node index")?
            }
            None => None,
        }) else {
            continue;
        };
        let id: FaceId =
            scope.id_charged(ctx, &cadmpeg_ir::identity_component!("face"), node.xmt())?;
        annotate_node(ctx, annotations, id.as_str(), source_stream, node, "FACE")?;
        if decoded_tolerance(fields.tolerance()).is_some() {
            annotations
                .derived(ctx, &id, "tolerance")
                .map_err(cadmpeg_core::CodecError::from)?;
        }
        storage.with_storage(|| {
            let indexed = id.try_clone_for_decode(ctx, "nx emitted face index")?;
            ctx.insert_btree_map(&mut faces, node.xmt(), indexed, "nx emitted face index")
        })?;
        let pending = PendingFace {
            xmt: node.xmt(),
            id,
            shell: shell_ref.try_clone_for_decode(ctx, "nx pending face shell")?,
            surface: surface_ref.try_clone_for_decode(ctx, "nx pending face surface")?,
            sense: fields.sense,
            tolerance: decoded_tolerance(fields.tolerance()),
        };
        ctx.push_scoped_vec(
            &mut storage,
            &mut pending_faces,
            pending,
            "nx pending faces",
        )?;
    }
    let mut loops: BTreeMap<u32, LoopId> = BTreeMap::new();
    let mut loop_specs: BTreeMap<u32, (LoopId, FaceId)> = BTreeMap::new();
    for (&loop_xmt, &ring) in ctx.admit_iter(&valid_loop_rings, "nx emitted loop index")? {
        let Some(node) = graph.get(ctx, NodeKind::Loop, loop_xmt)? else {
            continue;
        };
        let Some(fields) = node.loop_fields() else {
            continue;
        };
        let Some(face_ref) = (match fields.face {
            Some(target) => {
                ctx.get_btree_map(&faces, &u32::from(target), "nx emitted face index")?
            }
            None => None,
        }) else {
            continue;
        };
        let id: LoopId = storage.with_storage(|| {
            scope.id_charged(ctx, &cadmpeg_ir::identity_component!("loop"), node.xmt())
        })?;
        let ring_resolves = ctx.all_by(
            ring,
            |fin_xmt| match graph
                .get(ctx, NodeKind::Fin, *fin_xmt)?
                .and_then(Node::fin_fields)
                .and_then(|fields| fields.edge)
            {
                Some(target) => {
                    ctx.contains_key_btree_map(&edges, &u32::from(target), "nx emitted edge index")
                }
                None => Ok(false),
            },
            "nx loop ring resolution",
        )?;
        if !ring_resolves {
            super::charge_loss_code(ctx, crate::loss::NxLossCode::TopologyLoopRingUnresolved)?;
            let note = crate::loss::NxLossCode::TopologyLoopRingUnresolved.note(
                ctx.format_retained(format_args!(
                        "parasolid#{stream_index} LOOP {loop_xmt} of {face_ref} states no resolvable coedge ring: loop {id} is omitted from its face"
                    ), "nx unresolved loop loss text")?,
            );
            ctx.push_vec(topology_losses, note, "nx topology losses")?;
            continue;
        }
        annotate_node(ctx, annotations, id.as_str(), source_stream, node, "LOOP")?;
        storage.with_storage(|| {
            let spec = (
                id.try_clone_for_decode(ctx, "nx loop specification identity")?,
                face_ref.try_clone_for_decode(ctx, "nx loop face identity")?,
            );
            ctx.insert_btree_map(
                &mut loop_specs,
                node.xmt(),
                spec,
                "nx loop specification index",
            )?;
            ctx.insert_btree_map(&mut loops, node.xmt(), id, "nx emitted loop index")
        })?;
    }
    let mut fin_ids = BTreeMap::new();
    for &xmt in ctx.admit_iter(&valid_fin_xmts, "nx fin identity index")? {
        let Some(loop_xmt) = graph
            .get(ctx, NodeKind::Fin, xmt)?
            .and_then(Node::fin_fields)
            .and_then(|fields| fields.loop_xmt)
        else {
            continue;
        };
        if ctx.contains_key_btree_map(&loops, &u32::from(loop_xmt), "nx emitted loop index")? {
            storage.with_storage(|| {
                let id = scope.id_charged::<CoedgeId>(
                    ctx,
                    &cadmpeg_ir::identity_component!("fin"),
                    xmt,
                )?;
                ctx.insert_btree_map(&mut fin_ids, xmt, id, "nx fin identity index")
            })?;
        }
    }
    // Preserve the endpoint proof only when the admitted carrier is the exact
    // intersection candidate consumed by the later attachment pass. A valid
    // unrelated coedge pcurve must not become a general admission shortcut.
    let mut endpoint_witnesses = EndpointWitnesses::new();
    let (valid_pcurve_fins, fallback_pcurves) = {
        let intersection_pcurves = intersection_pcurve_index(
            ctx,
            &mut storage,
            &ir.model.curves,
            &ir.model.procedural_curves,
        )?;
        let mut index_storage = ctx.reserve_scoped(0, "nx coedge pcurve model index")?;
        let index = index_storage
            .with_storage_limit(|| cadmpeg_ir::index::ModelIndex::new_model_only(ir, ctx))?;
        let mut valid_pcurve_fins = BTreeSet::new();
        for &fin_xmt in ctx
            .admit_iter(&fin_ids, "nx valid pcurve fins")?
            .map(|(fin_xmt, _)| fin_xmt)
        {
            let Some(candidate) = fin_pcurve_candidate(
                ctx,
                graph,
                &FinCarrierLookups {
                    edges: &edges,
                    surfaces,
                    pcurves,
                    trim_ranges,
                },
                fin_xmt,
            )?
            else {
                continue;
            };
            let FinPcurveCandidate {
                edge,
                support,
                carrier: carrier_id,
                use_range,
            } = candidate;
            let Some(carrier) = index.pcurves(carrier_id.as_str(), ctx)? else {
                continue;
            };
            let parameter_range = use_range
                .or(carrier
                    .parameter_range()
                    .map(cadmpeg_ir::units::FiniteVector::get))
                .or_else(|| pcurve_parameter_range(&carrier.geometry));
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
                continue;
            };
            let Some(curve) = index
                .edges(edge.as_str(), ctx)?
                .and_then(|edge| edge.curve())
            else {
                continue;
            };
            let Some(parameter_range) = parameter_range else {
                continue;
            };
            storage.with_storage(|| {
                ctx.insert_btree_set(&mut valid_pcurve_fins, fin_xmt, "nx valid pcurve fins")
            })?;
            let Some(&(candidate_geometry, candidate_range, _)) = ctx.get_btree_map(
                &intersection_pcurves,
                &(curve, support),
                "nx intersection pcurve index",
            )?
            else {
                continue;
            };
            if *candidate_geometry != carrier.geometry || candidate_range != parameter_range {
                continue;
            }
            let witness = (
                carrier
                    .geometry
                    .try_clone_for_decode(ctx, "nx endpoint witness pcurve")?,
                parameter_range,
                endpoints,
            );
            let key = (
                curve.try_clone_for_decode(ctx, "nx endpoint witness curve")?,
                support.try_clone_for_decode(ctx, "nx endpoint witness support")?,
            );
            match ctx.get_mut_btree_map(
                &mut endpoint_witnesses,
                &key,
                "nx endpoint witness index",
            )? {
                Some(witnesses) => ctx.push_vec(witnesses, witness, "nx endpoint witnesses")?,
                None => {
                    let mut witnesses = Vec::new();
                    ctx.push_vec(&mut witnesses, witness, "nx endpoint witnesses")?;
                    ctx.insert_btree_map(
                        &mut endpoint_witnesses,
                        key,
                        witnesses,
                        "nx endpoint witness index",
                    )?;
                }
            }
        }
        let mut fallback_pcurves = BTreeMap::new();
        for &fin_xmt in ctx
            .admit_iter(&fin_ids, "nx fallback pcurve index")?
            .map(|(fin_xmt, _)| fin_xmt)
        {
            if ctx.contains_btree_set(&valid_pcurve_fins, &fin_xmt, "nx valid pcurve fins")? {
                continue;
            }
            let Some(fields) = graph
                .get(ctx, NodeKind::Fin, fin_xmt)?
                .and_then(Node::fin_fields)
            else {
                continue;
            };
            let Some(edge) = (match fields.edge {
                Some(target) => {
                    ctx.get_btree_map(&edges, &u32::from(target), "nx emitted edge index")?
                }
                None => None,
            }) else {
                continue;
            };
            let Some(support) = fin_support(ctx, graph, surfaces, fields)? else {
                continue;
            };
            let Some(&carrier) =
                ctx.get_btree_map(&edge_curves_by_id, edge, "nx edge curve index")?
            else {
                continue;
            };
            let Some(&(geometry, parameter_range, fit_tolerance)) = ctx.get_btree_map(
                &intersection_pcurves,
                &(carrier, support),
                "nx intersection pcurve index",
            )?
            else {
                continue;
            };
            if pcurve_matches_edge_range_with_index_and_budget(
                &index,
                edge,
                support,
                geometry,
                None,
                fit_tolerance.map(cadmpeg_ir::geometry::FitTolerance::get),
                adaptive_geometry_budget,
            )? {
                storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut fallback_pcurves,
                        fin_xmt,
                        (geometry, parameter_range, fit_tolerance),
                        "nx fallback pcurve index",
                    )
                })?;
            }
        }
        (valid_pcurve_fins, fallback_pcurves)
    };
    let mut serialized_branch_pcurves = BTreeSet::new();
    for (&fin_xmt, id) in ctx.admit_iter(&fin_ids, "nx emitted coedges")? {
        let Some(node) = graph.get(ctx, NodeKind::Fin, fin_xmt)? else {
            continue;
        };
        let Some(fields) = node.fin_fields() else {
            continue;
        };
        let Some(loop_id) = (match fields.loop_xmt {
            Some(target) => {
                ctx.get_btree_map(&loops, &u32::from(target), "nx emitted loop index")?
            }
            None => None,
        }) else {
            continue;
        };
        let Some(edge) = (match fields.edge {
            Some(target) => {
                ctx.get_btree_map(&edges, &u32::from(target), "nx emitted edge index")?
            }
            None => None,
        }) else {
            continue;
        };
        annotate_node(ctx, annotations, id.as_str(), source_stream, node, "FIN")?;
        let partner = match fields.other {
            Some(target) => ctx
                .get_btree_map(&fin_ids, &u32::from(target), "nx fin identity index")?
                .map(|partner| partner.try_clone_for_decode(ctx, "nx fin partner identity"))
                .transpose()?,
            None => None,
        };
        let radial_next = match partner {
            Some(partner) => partner,
            None => id.try_clone_for_decode(ctx, "nx fin radial successor identity")?,
        };
        let support = fin_support(ctx, graph, surfaces, fields)?;
        let pcurve_use_range = match fields.curve_xmt {
            Some(target) => ctx
                .get_btree_map(trim_ranges, &u32::from(target), "nx curve trim ranges")?
                .copied()
                .and_then(ordered_parameter_range),
            None => None,
        };
        let mut pcurve = match fields.curve_xmt {
            Some(target)
                if ctx.contains_btree_set(
                    &valid_pcurve_fins,
                    &fin_xmt,
                    "nx valid pcurve fins",
                )? =>
            {
                ctx.get_btree_map(pcurves, &u32::from(target), "nx pcurve node index")?
                    .map(|id| id.try_clone_for_decode(ctx, "nx fin pcurve identity"))
                    .transpose()?
            }
            _ => None,
        };
        let edge_curve = ctx
            .get_btree_map(&edge_curves_by_id, edge, "nx edge curve index")?
            .copied();
        if let (Some(pcurve), Some(edge_curve), Some(support), Some(target)) =
            (pcurve.as_ref(), edge_curve, support, fields.curve_xmt)
        {
            let target = u32::from(target);
            let same_curve = match ctx.get_btree_map(curves, &target, "nx curve node index")? {
                Some(curve) => ctx.equal(curve, edge_curve, "nx serialized branch curves")?,
                None => false,
            };
            let same_support = same_curve
                && match ctx.get_btree_map(pcurve_supports, &target, "nx pcurve supports")? {
                    Some(pcurve_support) => {
                        ctx.equal(pcurve_support, support, "nx serialized branch supports")?
                    }
                    None => false,
                };
            if same_support {
                storage.with_storage(|| {
                    let branch = (
                        edge_curve.try_clone_for_decode(ctx, "nx branch curve identity")?,
                        support.try_clone_for_decode(ctx, "nx branch support identity")?,
                        pcurve.try_clone_for_decode(ctx, "nx branch pcurve identity")?,
                    );
                    ctx.insert_btree_set(
                        &mut serialized_branch_pcurves,
                        branch,
                        "nx serialized branch pcurves",
                    )
                })?;
            }
        }
        let attached_pcurve_use_range = pcurve.as_ref().and(pcurve_use_range);
        if pcurve.is_none() {
            if let Some(&(geometry, parameter_range, fit_tolerance)) =
                ctx.get_btree_map(&fallback_pcurves, &fin_xmt, "nx fallback pcurve index")?
            {
                let pcurve_id: PcurveId = scope.id_charged(
                    ctx,
                    &cadmpeg_ir::identity_component!("intersection-pcurve"),
                    fin_xmt,
                )?;
                annotations.note(
                    ctx,
                    pcurve_id.as_str(),
                    source_stream,
                    cadmpeg_core::decode::u64_from_index(node.pos()),
                    Some("INTERSECTION_PCURVE"),
                )?;
                annotations.derived(ctx, pcurve_id.as_str(), "geometry")?;
                annotations.derived(ctx, pcurve_id.as_str(), "parameter_range")?;
                if fit_tolerance.is_some() {
                    annotations.derived(ctx, pcurve_id.as_str(), "fit_tolerance")?;
                }
                let fallback = Pcurve {
                    id: pcurve_id.try_clone_for_decode(ctx, "nx fallback pcurve identity")?,
                    geometry: geometry.try_clone_for_decode(ctx, "nx attached fallback pcurve")?,
                    metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                        None,
                        Some(
                            cadmpeg_ir::units::FiniteVector::new(parameter_range)
                                .ok_or(PcurveMetadata::NON_FINITE_PARAMETER_RANGE)
                                .map_err(cadmpeg_core::CodecError::malformed)?,
                        ),
                        fit_tolerance,
                    ),
                };
                ctx.push_vec(&mut ir.model.pcurves, fallback, "nx fallback pcurves")?;
                pcurve = Some(pcurve_id);
            }
        }
        let mut pcurve_uses = Vec::new();
        if let Some(pcurve) = pcurve {
            let pcurve_use = cadmpeg_ir::topology::PcurveUse {
                pcurve,
                isoparametric: None,
                parameter_range: attached_pcurve_use_range
                    .map(cadmpeg_ir::geometry::DirectedParameterRange::new)
                    .transpose()
                    .map_err(CodecError::malformed)?,
            };
            ctx.push_vec(&mut pcurve_uses, pcurve_use, "nx coedge pcurve uses")?;
        }
        let coedge = Coedge {
            id: id.try_clone_for_decode(ctx, "nx coedge identity copy")?,
            owner_loop: loop_id.try_clone_for_decode(ctx, "nx coedge loop identity")?,
            edge: edge.try_clone_for_decode(ctx, "nx fin edge identity")?,
            radial_next,
            sense: fields.sense,
            pcurves: pcurve_uses,
            use_curve: None,
        };
        ctx.push_vec(&mut ir.model.coedges, coedge, "nx emitted coedges")?;
    }
    drop((fallback_pcurves, edge_curves_by_id));
    let mut face_loops: BTreeMap<FaceId, Vec<LoopId>> = BTreeMap::new();
    for rings in ctx
        .admit_iter(&face_loop_rings, "nx emitted loops")?
        .map(|(_, rings)| rings)
    {
        for (loop_xmt, fin_xmts) in ctx.admit_iter(rings, "nx emitted loops")? {
            let Some((id, face)) =
                ctx.get_btree_map(&loop_specs, loop_xmt, "nx loop specification index")?
            else {
                continue;
            };
            let mut coedges = Vec::new();
            let mut all_resolved = true;
            for fin_xmt in fin_xmts {
                ctx.charge_work(1, "nx loop coedges")?;
                let Some(fin_id) = ctx.get_btree_map(&fin_ids, fin_xmt, "nx fin identity index")?
                else {
                    all_resolved = false;
                    break;
                };
                let coedge = fin_id.try_clone_for_decode(ctx, "nx loop coedge identity")?;
                ctx.push_vec(&mut coedges, coedge, "nx loop coedges")?;
            }
            let ring = if all_resolved {
                cadmpeg_ir::topology::LoopRing::new(ctx, coedges, Vec::new())
                    .map_err(cadmpeg_core::CodecError::from)?
                    .ok()
            } else {
                None
            };
            let Some(ring) = ring else {
                super::charge_loss_code(ctx, crate::loss::NxLossCode::TopologyLoopRingUnresolved)?;
                let note = crate::loss::NxLossCode::TopologyLoopRingUnresolved.note(
                    ctx.format_retained(format_args!(
                            "parasolid#{stream_index} LOOP {loop_xmt} of {face} states no resolvable coedge ring: loop {id} is omitted from its face"
                        ), "nx unresolved loop loss text")?,
                );
                ctx.push_vec(topology_losses, note, "nx topology losses")?;
                continue;
            };
            let record = Loop {
                id: id.try_clone_for_decode(ctx, "nx emitted loop identity")?,
                face: face.try_clone_for_decode(ctx, "nx emitted loop face identity")?,
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
            };
            ctx.push_vec(&mut ir.model.loops, record, "nx emitted loops")?;
            let loop_id = id.try_clone_for_decode(ctx, "nx face loop identity")?;
            match ctx.get_mut_btree_map(&mut face_loops, face, "nx face loop index")? {
                Some(loops) => ctx.push_vec(loops, loop_id, "nx face loops")?,
                None => {
                    let mut loops = Vec::new();
                    ctx.push_vec(&mut loops, loop_id, "nx face loops")?;
                    storage.with_storage(|| {
                        let key = face.try_clone_for_decode(ctx, "nx face loop index identity")?;
                        ctx.insert_btree_map(&mut face_loops, key, loops, "nx face loop index")
                    })?;
                }
            }
        }
    }
    for pending in ctx.admit_iter(pending_faces, "nx emitted faces")? {
        if let Some(failure) = ctx.remove_btree_map(
            &mut face_loop_failures,
            &pending.xmt,
            "nx face loop failure index",
        )? {
            super::charge_loss_code(ctx, crate::loss::NxLossCode::TopologyFaceLoopUnresolved)?;
            let note = crate::loss::NxLossCode::TopologyFaceLoopUnresolved.note(
                ctx.format_retained(format_args!(
                        "parasolid#{stream_index} FACE {} has an unresolved boundary: {failure}; face is emitted without loops",
                        pending.xmt
                    ), "nx unresolved face loss text")?,
            );
            ctx.push_vec(topology_losses, note, "nx topology losses")?;
        }
        let loops = ctx
            .remove_btree_map(&mut face_loops, &pending.id, "nx face loop index")?
            .unwrap_or_default();
        let face = Face {
            id: pending.id,
            shell: pending.shell,
            surface: pending.surface,
            sense: pending.sense,
            loops: cadmpeg_ir::topology::FaceLoops::unspecified(loops),
            name: None,
            color: None,
            tolerance: pending.tolerance,
        };
        ctx.push_vec(&mut ir.model.faces, face, "nx emitted faces")?;
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

    let mut owned_edges = BTreeSet::new();
    for coedge in ctx.admit_iter(&ir.model.coedges, "nx owned edge index")? {
        storage.with_storage(|| {
            ctx.insert_btree_set(&mut owned_edges, &coedge.edge, "nx owned edge index")
        })?;
    }
    let mut candidate_edges = BTreeSet::new();
    for edge in ctx
        .admit_iter(&edges, "nx candidate edge index")?
        .map(|(_, edge)| edge)
    {
        storage.with_storage(|| {
            ctx.insert_btree_set(&mut candidate_edges, edge, "nx candidate edge index")
        })?;
    }
    let edge_marks = keep_marks(
        ctx,
        &mut storage,
        &ir.model.edges,
        |edge| {
            Ok(
                !ctx.contains_btree_set(&candidate_edges, &edge.id, "nx candidate edge index")?
                    || ctx.contains_btree_set(&owned_edges, &edge.id, "nx owned edge index")?,
            )
        },
        "nx retained edges",
    )?;
    drop((owned_edges, candidate_edges));
    retain_marked(&mut ir.model.edges, edge_marks);
    let mut retained_vertices = BTreeSet::new();
    for edge in ctx.admit_iter(&ir.model.edges, "nx retained vertex index")? {
        storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut retained_vertices,
                &edge.start,
                "nx retained vertex index",
            )?;
            ctx.insert_btree_set(
                &mut retained_vertices,
                &edge.end,
                "nx retained vertex index",
            )
        })?;
    }
    let scope_prefix = storage.with_storage(|| scope.prefix_charged(ctx))?;
    let vertex_marks = keep_marks(
        ctx,
        &mut storage,
        &ir.model.vertices,
        |vertex| {
            Ok(
                !ctx.starts_with(vertex.id.as_str(), &scope_prefix, "nx retained vertices")?
                    || ctx.contains_btree_set(
                        &retained_vertices,
                        &vertex.id,
                        "nx retained vertex index",
                    )?,
            )
        },
        "nx retained vertices",
    )?;
    drop(retained_vertices);
    retain_marked(&mut ir.model.vertices, vertex_marks);
    Ok(endpoint_witnesses)
}

/// Stream lookups that lift an edge without a 3D carrier to its pcurve.
struct ParametricEdgeLookups<'inputs> {
    pcurves: &'inputs BTreeMap<u32, PcurveId>,
    pcurve_supports: &'inputs BTreeMap<u32, SurfaceId>,
    pcurve_indices: &'inputs BTreeMap<PcurveId, usize>,
}

/// The support surface, pcurve geometry and ordered parameter range of an
/// edge whose curve reference names a supported pcurve.
fn lift_parametric_edge(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    lookups: &ParametricEdgeLookups<'_>,
    xmt: u32,
    param_range: Option<[f64; 2]>,
) -> Result<Option<(SurfaceId, PcurveGeometry, [f64; 2])>, CodecError> {
    let Some(pcurve_id) = ctx.get_btree_map(lookups.pcurves, &xmt, "nx pcurve node index")? else {
        return Ok(None);
    };
    let Some(&pcurve_index) =
        ctx.get_btree_map(lookups.pcurve_indices, pcurve_id, "nx pcurve index")?
    else {
        return Ok(None);
    };
    let Some(pcurve) = ir.model.pcurves.get(pcurve_index) else {
        return Ok(None);
    };
    let Some(surface_ref) =
        ctx.get_btree_map(lookups.pcurve_supports, &xmt, "nx pcurve supports")?
    else {
        return Ok(None);
    };
    let parameter_range = pcurve
        .parameter_range()
        .map(cadmpeg_ir::units::FiniteVector::get)
        .or(param_range)
        .or_else(|| pcurve_parameter_range(&pcurve.geometry));
    let Some(parameter_range) = parameter_range.and_then(ordered_parameter_range) else {
        return Ok(None);
    };
    Ok(Some((
        surface_ref.try_clone_for_decode(ctx, "nx parametric edge surface")?,
        pcurve
            .geometry
            .try_clone_for_decode(ctx, "nx parametric edge pcurve")?,
        parameter_range,
    )))
}

/// Pcurves carried by intersection constructions, keyed by the construction's
/// owning curve and the side's support surface.
type IntersectionPcurveIndex<'ir> =
    BTreeMap<(&'ir CurveId, &'ir SurfaceId), (&'ir PcurveGeometry, [f64; 2], Option<FitTolerance>)>;

fn intersection_pcurve_index<'ir>(
    ctx: &DecodeContext<'_>,
    storage: &mut ScopedReservation<'_>,
    curves: &'ir [Curve],
    procedural_curves: &'ir [ProceduralCurve],
) -> Result<IntersectionPcurveIndex<'ir>, CodecError> {
    let (owners, _owner_storage) = procedural_curve_owners(ctx, curves)?;
    let mut index = IntersectionPcurveIndex::new();
    for procedural in ctx.admit_iter(procedural_curves, "nx intersection pcurve index")? {
        let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
        else {
            continue;
        };
        let Some(Some(owner)) =
            ctx.get_hash_map(&owners, &procedural.id, "nx procedural curve owners")?
        else {
            continue;
        };
        for side in context.sides() {
            let (Some(surface), Some(pcurve)) = (&side.surface, &side.pcurve) else {
                continue;
            };
            storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut index,
                    (*owner, surface),
                    (
                        &pcurve.geometry,
                        context.parameter_range().endpoints(),
                        procedural.cache_fit_tolerance(),
                    ),
                    "nx intersection pcurve index",
                )
            })?;
        }
    }
    Ok(index)
}

/// Index each procedural curve construction by the curve that carries it. A
/// construction carried by more than one curve has no owner.
pub(super) fn procedural_curve_owners<'ir, 'c>(
    ctx: &'c DecodeContext<'_>,
    curves: &'ir [Curve],
) -> Result<
    (
        std::collections::HashMap<&'ir ProceduralCurveId, Option<&'ir CurveId>>,
        ScopedReservation<'c>,
    ),
    CodecError,
> {
    ctx.unique_index(
        ctx.admit_iter(curves, "nx procedural curve owners")?
            .filter_map(|curve| Some((curve.geometry.procedural_construction()?, &curve.id))),
        "nx procedural curve owners",
    )
}

/// Stream lookups that resolve a fin's edge, support and pcurve carrier.
struct FinCarrierLookups<'inputs> {
    edges: &'inputs BTreeMap<u32, EdgeId>,
    surfaces: &'inputs BTreeMap<u32, SurfaceId>,
    pcurves: &'inputs BTreeMap<u32, PcurveId>,
    trim_ranges: &'inputs BTreeMap<u32, [f64; 2]>,
}

struct FinPcurveCandidate<'inputs> {
    edge: &'inputs EdgeId,
    support: &'inputs SurfaceId,
    carrier: &'inputs PcurveId,
    use_range: Option<[f64; 2]>,
}

/// The support surface of the face that owns a fin's loop.
fn fin_support<'s>(
    ctx: &DecodeContext<'_>,
    graph: &Graph,
    surfaces: &'s BTreeMap<u32, SurfaceId>,
    fields: crate::topology::FinFields,
) -> Result<Option<&'s SurfaceId>, CodecError> {
    let face = match graph
        .get_target(ctx, NodeKind::Loop, fields.loop_xmt)?
        .and_then(Node::loop_fields)
    {
        Some(loop_) => graph.get_target(ctx, NodeKind::Face, loop_.face)?,
        None => None,
    };
    let surface = face
        .and_then(Node::face_fields)
        .and_then(|face| face.surface);
    match surface {
        Some(target) => ctx.get_btree_map(surfaces, &u32::from(target), "nx surface node index"),
        None => Ok(None),
    }
}

fn fin_pcurve_candidate<'inputs>(
    ctx: &DecodeContext<'_>,
    graph: &Graph,
    lookups: &FinCarrierLookups<'inputs>,
    fin_xmt: u32,
) -> Result<Option<FinPcurveCandidate<'inputs>>, CodecError> {
    let Some(fields) = graph
        .get(ctx, NodeKind::Fin, fin_xmt)?
        .and_then(Node::fin_fields)
    else {
        return Ok(None);
    };
    let Some(edge) = (match fields.edge {
        Some(target) => {
            ctx.get_btree_map(lookups.edges, &u32::from(target), "nx emitted edge index")?
        }
        None => None,
    }) else {
        return Ok(None);
    };
    let Some(support) = fin_support(ctx, graph, lookups.surfaces, fields)? else {
        return Ok(None);
    };
    let Some(curve_xmt) = fields.curve_xmt.map(u32::from) else {
        return Ok(None);
    };
    let Some(carrier) = ctx.get_btree_map(lookups.pcurves, &curve_xmt, "nx pcurve node index")?
    else {
        return Ok(None);
    };
    let use_range = ctx
        .get_btree_map(lookups.trim_ranges, &curve_xmt, "nx curve trim ranges")?
        .copied()
        .and_then(ordered_parameter_range);
    Ok(Some(FinPcurveCandidate {
        edge,
        support,
        carrier,
        use_range,
    }))
}

/// Visit values once and record which of them `keep` admits. The marks let a
/// caller release its borrows before [`retain_marked`] edits the arena.
pub(super) fn keep_marks<T>(
    ctx: &DecodeContext<'_>,
    storage: &mut ScopedReservation<'_>,
    values: &[T],
    mut keep: impl FnMut(&T) -> Result<bool, CodecError>,
    operation: &'static str,
) -> Result<Vec<bool>, CodecError> {
    let mut marks = Vec::new();
    for value in ctx.admit_iter(values, operation)? {
        let mark = keep(value)?;
        ctx.push_scoped_vec(storage, &mut marks, mark, operation)?;
    }
    Ok(marks)
}

/// Keep the arena values whose mark is set, in order. The marks were produced
/// by one admitted pass over the same arena.
pub(super) fn retain_marked<T>(values: &mut Vec<T>, marks: Vec<bool>) {
    let mut marks = marks.into_iter();
    values.retain(|_| marks.next().unwrap_or(false));
}

pub(super) struct UnresolvedTopologyStream<'inputs, 'storage> {
    pub(super) stream_index: usize,
    pub(super) graph: &'inputs Graph,
    pub(super) surfaces: &'inputs mut BTreeMap<u32, SurfaceId>,
    pub(super) curves: &'inputs mut BTreeMap<u32, CurveId>,
    pub(super) pcurves: &'inputs BTreeMap<u32, PcurveId>,
    pub(super) source_stream: &'inputs cadmpeg_ir::annotations::StreamHandle,
    /// Scoped storage of the stream's carrier tables.
    pub(super) storage: &'inputs mut ScopedReservation<'storage>,
}

pub(super) fn retain_unresolved_topology_carriers(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    unresolved_topology_stream: UnresolvedTopologyStream<'_, '_>,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    let UnresolvedTopologyStream {
        stream_index,
        graph,
        surfaces,
        curves,
        pcurves,
        source_stream,
        storage,
    } = unresolved_topology_stream;
    let scope = IdScope::stream_charged(ctx, stream_index)?;
    let mut unknown: Option<UnknownId> = None;
    let mut unknown_record =
        |ctx: &DecodeContext<'_>, operation: &'static str| -> Result<UnknownId, CodecError> {
            if unknown.is_none() {
                unknown = Some(IdScope::container().id_charged(
                    ctx,
                    &cadmpeg_ir::identity_component!("parasolid"),
                    stream_index,
                )?);
            }
            match &unknown {
                Some(unknown) => unknown.try_clone_for_decode(ctx, operation),
                None => Err(CodecError::malformed(
                    "NX unresolved carrier record is missing",
                )),
            }
        };
    for face in graph.of_kind(NodeKind::Face) {
        ctx.charge_work(1, "nx unresolved surfaces")?;
        let Some(surface_xmt) = face
            .face_fields()
            .and_then(|fields| fields.surface.map(u32::from))
        else {
            continue;
        };
        if surface_xmt <= 1
            || ctx.contains_key_btree_map(surfaces, &surface_xmt, "nx surface node index")?
        {
            continue;
        }
        let id: SurfaceId = scope.id_charged(
            ctx,
            &cadmpeg_ir::identity_component!("surface"),
            format_args!("unknown-{surface_xmt}"),
        )?;
        annotations.note(
            ctx,
            id.as_str(),
            source_stream,
            cadmpeg_core::decode::u64_from_index(face.pos()),
            Some("UNRESOLVED_SURFACE_REFERENCE"),
        )?;
        annotations.exactness(ctx, id.as_str(), Exactness::Unknown)?;
        storage.with_storage(|| {
            let indexed = id.try_clone_for_decode(ctx, "nx unresolved surface index")?;
            ctx.insert_btree_map(
                surfaces,
                surface_xmt,
                indexed,
                "nx unresolved surface index",
            )
        })?;
        let surface = Surface {
            id,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                record: Some(unknown_record(ctx, "nx unresolved surface record")?),
            }),
            source_object: None,
        };
        ctx.push_vec(&mut ir.model.surfaces, surface, "nx unresolved surfaces")?;
    }

    for edge in graph.of_kind(NodeKind::Edge) {
        ctx.charge_work(1, "nx unresolved curves")?;
        let Some(curve_xmt) = edge
            .edge_fields()
            .and_then(|fields| fields.curve.map(u32::from))
        else {
            continue;
        };
        if curve_xmt <= 1
            || ctx.contains_key_btree_map(curves, &curve_xmt, "nx curve node index")?
            || ctx.contains_key_btree_map(pcurves, &curve_xmt, "nx pcurve node index")?
        {
            continue;
        }
        let id: CurveId = scope.id_charged(
            ctx,
            &cadmpeg_ir::identity_component!("curve"),
            format_args!("unknown-{curve_xmt}"),
        )?;
        annotations.note(
            ctx,
            id.as_str(),
            source_stream,
            cadmpeg_core::decode::u64_from_index(edge.pos()),
            Some("UNRESOLVED_CURVE_REFERENCE"),
        )?;
        annotations.exactness(ctx, id.as_str(), Exactness::Unknown)?;
        storage.with_storage(|| {
            let indexed = id.try_clone_for_decode(ctx, "nx unresolved curve index")?;
            ctx.insert_btree_map(curves, curve_xmt, indexed, "nx unresolved curve index")
        })?;
        let curve = Curve {
            id,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                record: Some(unknown_record(ctx, "nx unresolved curve record")?),
            }),
            source_object: None,
        };
        ctx.push_vec(&mut ir.model.curves, curve, "nx unresolved curves")?;
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
    annotations.note(
        ctx,
        id,
        stream,
        cadmpeg_core::decode::u64_from_index(node.pos()),
        Some(tag),
    )
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

struct ClosedEdge<'inputs> {
    scope: &'inputs IdScope,
    edge: &'inputs Node,
    curve: &'inputs CurveId,
    curve_index: usize,
    range: Option<[f64; 2]>,
    source_stream: &'inputs cadmpeg_ir::annotations::StreamHandle,
    tolerance: Option<cadmpeg_ir::scalar::PositiveReal>,
}

fn synthesize_closed_edge_vertex_with_curve_index_and_budget(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    closed_edge: &ClosedEdge<'_>,
    curve_point_cache: &mut CurvePointCache,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<VertexId>, CodecError> {
    let &ClosedEdge {
        scope,
        edge,
        curve,
        curve_index,
        range,
        source_stream,
        tolerance,
    } = closed_edge;

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
        format_args!("closed-edge-{}", edge.xmt()),
    )?;
    let vertex: VertexId = scope.id_charged(
        ctx,
        &cadmpeg_ir::identity_component!("vertex"),
        format_args!("closed-edge-{}", edge.xmt()),
    )?;
    annotations.note(
        ctx,
        point.as_str(),
        source_stream,
        cadmpeg_core::decode::u64_from_index(edge.pos()),
        Some("CLOSED_EDGE_POINT"),
    )?;
    annotations.exactness(ctx, point.as_str(), Exactness::Inferred)?;
    annotations.note(
        ctx,
        vertex.as_str(),
        source_stream,
        cadmpeg_core::decode::u64_from_index(edge.pos()),
        Some("CLOSED_EDGE_VERTEX"),
    )?;
    annotations.exactness(ctx, vertex.as_str(), Exactness::Inferred)?;
    ctx.reserve_vec(&mut ir.model.points, 1, "nx closed edge points")?;
    ir.model.points.push(Point::new(
        point.try_clone_for_decode(ctx, "nx closed edge point identity")?,
        position,
        None,
    ));
    ctx.reserve_vec(&mut ir.model.vertices, 1, "nx closed edge vertices")?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    curve: &CurveId,
    range: [f64; 2],
    start: &VertexId,
    end: &VertexId,
    edge_tolerance: Option<f64>,
) -> Option<([f64; 2], bool)> {
    let geometry_budget = GeometryWorkBudget::from_context(
        ctx,
        cadmpeg_core::decode::u64_from_index(super::geometry_work::MAX_ADAPTIVE_GEOMETRY_WORK),
    );

    orient_edge_range_with_budget(
        ctx,
        ir,
        curve,
        range,
        (start, end),
        edge_tolerance,
        &geometry_budget,
    )
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
    let mut curve_point_cache = CurvePointCache::new(ctx).expect("cache reservation fits");
    orient_edge_range_for_geometry_with_budget(
        ctx,
        &EdgeGeometryRange {
            geometry,
            curve,
            range,
            start_position,
            start_tolerance,
            end_position,
            end_tolerance,
            edge_tolerance,
            procedural_curve,
        },
        &mut curve_point_cache,
        geometry_budget,
    )
    .expect("evaluator allocation succeeds")
}

const MAX_CURVE_POINT_CACHE_ENTRIES: usize = 131_072;

/// Exact curve evaluations reused within one stream, held as scoped storage.
struct CurvePointCache<'s> {
    entries: BTreeMap<CurveId, BTreeMap<u64, Option<FinitePoint3>>>,
    len: usize,
    storage: ScopedReservation<'s>,
}

impl<'s> CurvePointCache<'s> {
    fn new(ctx: &'s DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            entries: BTreeMap::new(),
            len: 0,
            storage: ctx.reserve_scoped(0, "nx curve point cache")?,
        })
    }

    fn point_with_budget(
        &mut self,
        ctx: &DecodeContext<'_>,
        curve: &CurveId,
        geometry: &CurveGeometry,
        parameter: f64,
        geometry_budget: &GeometryWorkBudget<'_>,
    ) -> Result<Option<FinitePoint3>, CodecError> {
        let bits = parameter.to_bits();
        if let Some(values) =
            ctx.get_btree_map(&self.entries, curve, "nx curve point cache curves")?
        {
            if let Some(point) = ctx.get_btree_map(values, &bits, "nx curve point cache values")? {
                return Ok(*point);
            }
        }
        let point = finite_or_refusal(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(geometry_budget.charges)
                .within_work_slice(geometry_budget, |admission| {
                    cadmpeg_ir::eval::decode::curve_point(admission, geometry, parameter)
                }),
        )?;
        if self.len < MAX_CURVE_POINT_CACHE_ENTRIES {
            let Self {
                entries,
                len,
                storage,
            } = self;
            match ctx.get_mut_btree_map(entries, curve, "nx curve point cache curves")? {
                Some(values) => {
                    storage.with_storage(|| {
                        ctx.insert_btree_map(values, bits, point, "nx curve point cache values")
                    })?;
                }
                None => {
                    storage.with_storage(|| {
                        let mut values = BTreeMap::new();
                        ctx.insert_btree_map(
                            &mut values,
                            bits,
                            point,
                            "nx curve point cache values",
                        )?;
                        let key =
                            curve.try_clone_for_decode(ctx, "nx curve point cache identity")?;
                        ctx.insert_btree_map(entries, key, values, "nx curve point cache curves")
                    })?;
                }
            }
            *len += 1;
        }
        Ok(point)
    }
}

struct EdgeGeometryRange<'inputs> {
    geometry: &'inputs CurveGeometry,
    curve: &'inputs CurveId,
    range: [f64; 2],
    start_position: Point3,
    start_tolerance: Option<f64>,
    end_position: Point3,
    end_tolerance: Option<f64>,
    edge_tolerance: Option<f64>,
    procedural_curve: bool,
}

fn orient_edge_range_for_geometry_with_budget(
    ctx: &DecodeContext<'_>,
    edge_geometry_range: &EdgeGeometryRange<'_>,
    curve_point_cache: &mut CurvePointCache,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<([f64; 2], bool)>, CodecError> {
    let &EdgeGeometryRange {
        geometry,
        curve,
        range,
        start_position,
        start_tolerance,
        end_position,
        end_tolerance,
        edge_tolerance,
        procedural_curve,
    } = edge_geometry_range;

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
    let id = ctx.format_retained(
        format_args!("nx:container:parasolid#{si}"),
        "nx unknown stream id",
    )?;
    let id = match UnknownId::mint(id) {
        Ok(id) => id,
        Err(error) => {
            return Err(CodecError::Malformed(ctx.format_retained(
                format_args!("{error}"),
                "nx unknown stream id refusal",
            )?));
        }
    };
    let offset = cadmpeg_core::decode::u64_from_index(stream.file_offset);
    match data {
        Some(data) => Ok(UnknownRecord::retained(id, offset, data, Vec::new())),
        None => {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(stream.inflated.len()),
                "hash NX unknown stream",
            )?;
            let digest = ctx.format_retained(
                format_args!("{}", LowerHex(&sha256(&stream.inflated))),
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
        scan.container
            .entry_count(ctx, crate::container::Region::Header)?,
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
            scan.container
                .entry_count(ctx, crate::container::Region::Footer)?,
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
        scan.count(ctx, StreamKind::Partition)?,
    )?;
    insert_source_attribute(
        ctx,
        &mut attributes,
        "deltas_streams",
        scan.count(ctx, StreamKind::Deltas)?,
    )?;
    insert_source_attribute(
        ctx,
        &mut attributes,
        "plain_streams",
        scan.count(ctx, StreamKind::Plain)?,
    )?;
    let external_reference_paths = scan.container.external_reference_paths(ctx)?;
    for (index, path) in ctx
        .admit_iter(
            &external_reference_paths,
            "nx external reference attributes",
        )?
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
    for entry in ctx.admit_iter(&scan.container.entries, "nx source preview entries")? {
        if !ctx.equal_bytes(
            entry.name.as_bytes(),
            b"/Root/images/preview",
            "nx source preview entries",
        )? {
            continue;
        }
        let Some((offset, size)) = entry.file_span() else {
            continue;
        };
        let (Ok(start), Ok(size)) = (usize::try_from(offset), usize::try_from(size)) else {
            continue;
        };
        let Some(end) = start.checked_add(size) else {
            continue;
        };
        let Some(payload) = scan.container.data.get(start..end) else {
            continue;
        };
        let Some((width, height, precision, components)) = jpeg_dimensions(ctx, payload)? else {
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
            LowerHex(&sha256(payload)),
        )?;
        preview_count += 1;
    }
    insert_source_attribute(ctx, &mut attributes, "jpeg_preview_count", preview_count)?;
    for (index, stream) in ctx
        .admit_iter(&scan.streams, "nx source deltas attributes")?
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
    Ok(SourceMeta::classified(
        dialects.try_clone_for_decode(ctx, "copy dialect layers")?,
        cadmpeg_core::text::named_entries_for_decode(ctx, "the nx part", attributes)?,
    ))
}

struct CountBytes(usize);

impl fmt::Write for CountBytes {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0 = self.0.checked_add(text.len()).ok_or(fmt::Error)?;
        Ok(())
    }
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

    let mut key_text = String::new();
    ctx.try_reserve_retained_text(&mut key_text, key_len.0, "nx source attribute text")?;
    write!(&mut key_text, "{key}")
        .map_err(|_| ctx.refuse_codec_limit("nx source attribute text", 0, 1))?;
    let mut value_text = String::new();
    ctx.try_reserve_retained_text(&mut value_text, value_len.0, "nx source attribute text")?;
    write!(&mut value_text, "{value}")
        .map_err(|_| ctx.refuse_codec_limit("nx source attribute text", 0, 1))?;
    ctx.insert_btree_map(attributes, key_text, value_text, "nx source attributes")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::geometry_work::GeometryWorkBudget;
    use super::{source_meta, unknown_stream, unknown_stream_metadata, CurvePointCache};
    use crate::container::Container;
    use crate::decode::Scan;
    use crate::parasolid::Stream;
    use cadmpeg_core::decode::ResourceDimension;
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
                segment_index: None,
                segment_wrappers: Vec::new(),
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
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_retained_bytes = 0;
            },
            |ctx| {
                assert!(matches!(
                    unknown_stream_metadata(ctx, 0, &preview_stream(Vec::new())),
                    Err(CodecError::ResourceLimit(limit))
                        if limit.dimension == ResourceDimension::RetainedBytes
                            && limit.operation == "nx unknown stream id"
                ));
            },
        );
    }

    #[test]
    fn unknown_stream_metadata_refuses_digest_text_at_retained_limit() {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_retained_bytes =
                    cadmpeg_core::decode::u64_from_index("nx:container:parasolid#0".len());
            },
            |ctx| {
                assert!(matches!(
                    unknown_stream_metadata(ctx, 0, &preview_stream(Vec::new())),
                    Err(CodecError::ResourceLimit(limit))
                        if limit.dimension == ResourceDimension::RetainedBytes
                            && limit.operation == "nx unknown stream digest"
                ));
            },
        );
    }

    #[test]
    fn unknown_stream_metadata_refuses_digest_work_at_caller_limit() {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_work_units =
                    2 * cadmpeg_core::decode::u64_from_index("nx:container:parasolid#0".len());
            },
            |ctx| {
                assert!(matches!(
                    unknown_stream_metadata(ctx, 0, &preview_stream(vec![7])),
                    Err(CodecError::ResourceLimit(limit))
                        if limit.dimension == ResourceDimension::WorkUnits
                            && limit.operation == "hash NX unknown stream"
                ));
            },
        );
    }

    #[test]
    fn unknown_stream_metadata_preserves_identity_and_digest_under_service_profile() {
        let stream = preview_stream(vec![7]);

        crate::test_support::with_decode_context(|ctx| {
            let unknown = unknown_stream_metadata(ctx, 0, &stream).unwrap();
            assert_eq!(unknown.id().as_str(), "nx:container:parasolid#0");
            assert_eq!(unknown.offset(), 0);
            assert_eq!(unknown.data(), None);
            let wire = serde_json::to_value(&unknown).unwrap();
            assert_eq!(
                wire["retention"]["sha256"],
                cadmpeg_ir::hash::sha256_hex(&stream.inflated)
            );
        });
    }

    #[test]
    fn source_meta_refuses_first_attribute_node_at_collection_limit() {
        let scan = empty_source_scan();
        let (dialects, _) = crate::test_support::with_decode_context(|ctx| {
            crate::dialect::classify_layers(ctx, &scan)
        })
        .unwrap()
        .into_report_parts();

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_collection_items = 0;
            },
            |ctx| {
                assert!(matches!(
                    source_meta(ctx, &scan, &dialects),
                    Err(CodecError::ResourceLimit(limit))
                        if limit.dimension == ResourceDimension::CollectionItems
                            && limit.operation == "nx source attributes"
                ));
            },
        );
    }

    #[test]
    fn source_meta_refuses_first_attribute_text_at_retained_limit() {
        let scan = empty_source_scan();
        let (dialects, _) = crate::test_support::with_decode_context(|ctx| {
            crate::dialect::classify_layers(ctx, &scan)
        })
        .unwrap()
        .into_report_parts();

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_retained_bytes = 0;
            },
            |ctx| {
                assert!(matches!(
                    source_meta(ctx, &scan, &dialects),
                    Err(CodecError::ResourceLimit(limit))
                        if limit.dimension == ResourceDimension::RetainedBytes
                            && limit.operation == "nx source attribute text"
                ));
            },
        );
    }

    #[test]
    fn source_meta_refuses_second_map_nodes_at_collection_limit() {
        let scan = empty_source_scan();
        let (dialects, _) = crate::test_support::with_decode_context(|ctx| {
            crate::dialect::classify_layers(ctx, &scan)
        })
        .unwrap()
        .into_report_parts();

        crate::test_support::with_decode_context_over(
            &[],
            |_| {},
            |service_ctx| {
                let expected = source_meta(service_ctx, &scan, &dialects).unwrap();
                assert_eq!(expected.attributes["file_size"], "0");

                crate::test_support::with_decode_context_over(
                    &[],
                    |policy| {
                        policy.limits.max_collection_items = cadmpeg_core::decode::u64_from_index(
                            expected.attributes.len() + dialects.primary().declared().len(),
                        );
                    },
                    |limited_ctx| {
                        assert!(matches!(
                            source_meta(limited_ctx, &scan, &dialects),
                            Err(CodecError::ResourceLimit(limit))
                                if limit.dimension == ResourceDimension::CollectionItems
                                    && limit.operation == "named entry map nodes"
                        ));
                    },
                );
            },
        );
    }

    #[test]
    fn unknown_stream_copy_refuses_when_retained_budget_is_exhausted() {
        crate::test_support::with_decode_context_over(
            &[0],
            |policy| {
                policy.limits.max_retained_bytes = 2;
            },
            |ctx| {
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
                    unknown_stream(ctx, 0, &stream),
                    Err(CodecError::ResourceLimit(limit))
                        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                            && limit.operation == "retain NX unknown stream"
                ));
            },
        );
    }

    #[test]
    fn curve_point_cache_reuses_an_exact_parameter_evaluation() {
        crate::test_support::with_decode_context(|geometry_ctx| {
            let curve =
                CurveId::mint("test:model:entity#synthetic:curve").expect("identity grammar");
            let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                    &cadmpeg_test_support::service_decode_context(),
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(1.0, 2.0, 3.0), Point3::new(5.0, 7.0, 9.0)],
                    None,
                    false,
                )
                .expect("fixture constructor admission")
                .expect("valid test curve"),
            ));
            let geometry_budget = GeometryWorkBudget::from_context(
                geometry_ctx,
                cadmpeg_core::decode::u64_from_index(1024),
            );
            crate::test_support::with_decode_context(|ctx| {
                let mut cache = CurvePointCache::new(ctx).expect("cache reservation fits");
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
        });
    }
}
