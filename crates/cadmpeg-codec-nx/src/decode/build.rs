// SPDX-License-Identifier: Apache-2.0
//! Geometry decode, active-body selection, and inactive-topology prune.

use super::emit::{
    annotate_node, canonical_trim_range, curve_tag, decoded_tolerance, emit_topology, keep_marks,
    procedural_curve_owners, retain_marked, retain_unknown_stream_data,
    retain_unresolved_topology_carriers, source_meta, surface_tag, unknown_stream_metadata,
};
use super::geometry_work::{
    GeometryWorkBudget, MAX_ADAPTIVE_GEOMETRY_WORK, MAX_COUPLED_SUPPORT_UV_GEOMETRY_WORK,
    MAX_PCURVE_COMPLETION_GEOMETRY_WORK, MAX_SERIALIZED_SUPPORT_UV_GEOMETRY_WORK,
    MAX_SUPPORT_UV_COMPLETION_GEOMETRY_WORK,
};
use super::offset::{intersection_side, normalize_pcurve_parameters, saved_offset_carriers};
use super::pcurves::{
    completion_transfer_budget_limit, transfer_budget_exhausted, EndpointWitnesses,
    IntersectionEntityStarts, IntersectionIncidenceIndex, MAX_EXACT_BOUNDARY_TRANSFER_SAMPLES,
};
use super::report::{build_geometry_report, CompletionBudgetStatus};
use super::support_uv::{
    assign_ext11_support_uv_with_index,
    attach_completed_intersection_pcurves_for_model_with_budget,
    attach_completed_intersection_pcurves_for_stream_with_budget,
    complete_ext11_support_uv_with_budget, complete_parameterization_equivalent_support_uv,
    complete_support_uv_with_budget_and_endpoint_witnesses,
    invalidate_inconsistent_support_uv_with_validated_lanes_and_status, linear_knots,
    support_uv_budget_exhausted, support_uv_completion_budget_limit,
    validate_serialized_support_uv_with_index, validated_support_uv_endpoint_witnesses,
    IntersectionCompletionSource, SerializedSupportUv,
};
use super::{report_untransferred_streams, Counts, Scan};
use crate::decode::ids::IdScope;
use crate::framing::node_kind::NodeKind;
use crate::loss::NxLossCode;
use crate::topology::{Graph, Node};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::codec::DecodeBody;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    pcurve::Pcurve, BlendCrossSection, BlendRadiusLaw, BlendSupport, Curve, CurveGeometry,
    IntcurveSupportContext, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CurveId, EdgeId, PcurveId, PointId, ProceduralCurveId, ProceduralSurfaceId, RegionId,
    ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{Body, BodyKind, Point, Region, Shell, Vertex};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn ordered_point_candidates<'a>(
    ctx: &DecodeContext<'_>,
    graph: &'a Graph,
) -> Result<Vec<(FinitePoint3, &'a Node)>, CodecError> {
    ordered_fixed_candidates(ctx, graph, [NodeKind::Point], Node::point_position)
}

pub(super) fn ordered_surface_candidates<'a>(
    ctx: &DecodeContext<'_>,
    graph: &'a Graph,
) -> Result<Vec<(SurfaceGeometry, &'a Node)>, CodecError> {
    ordered_fixed_candidates(
        ctx,
        graph,
        [
            NodeKind::Plane,
            NodeKind::Cylinder,
            NodeKind::Cone,
            NodeKind::Sphere,
            NodeKind::Torus,
        ],
        Node::surface_geometry,
    )
}

pub(super) fn ordered_curve_candidates<'a>(
    ctx: &DecodeContext<'_>,
    graph: &'a Graph,
) -> Result<Vec<(CurveGeometry, &'a Node)>, CodecError> {
    ordered_fixed_candidates(
        ctx,
        graph,
        [NodeKind::Line, NodeKind::Circle, NodeKind::Ellipse],
        Node::curve_geometry,
    )
}

fn ordered_fixed_candidates<'a, T, const KINDS: usize>(
    ctx: &DecodeContext<'_>,
    graph: &'a Graph,
    kinds: [NodeKind; KINDS],
    graph_value: impl Fn(&Node) -> Option<T>,
) -> Result<Vec<(T, &'a Node)>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "nx analytic candidate index")?;
    let mut candidates = BTreeMap::new();
    for kind in kinds {
        for node in graph.of_kind(kind) {
            ctx.charge_work(1, "scan NX analytic candidates")?;
            if let Some(value) = graph_value(node) {
                storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut candidates,
                        node.pos(),
                        (value, node),
                        "nx analytic candidate index",
                    )
                })?;
            }
        }
    }
    let mut ordered = ctx.vector_storage(candidates.len(), "nx ordered analytic candidates")?;
    let mut candidates = candidates.into_values();
    for _ in ctx.admit_iter(&(0..candidates.len()), "nx ordered analytic candidates")? {
        let Some(candidate) = candidates.next() else {
            break;
        };
        ctx.push_vec(&mut ordered, candidate, "nx ordered analytic candidates")?;
    }
    Ok(ordered)
}

/// Decode analytic carriers from every Parasolid stream. Returns `None` when no
/// carrier of any kind passes its gate, so the caller falls back to metadata.
type GeometryDecode = (
    CadIr,
    DecodeBody,
    cadmpeg_ir::Annotations,
    Vec<UnknownRecord>,
);

pub(super) fn try_decode_geometry(
    ctx: &DecodeContext<'_>,
    scan: &Scan,
    dialects: &DialectLayers,
    dialect_losses: &[LossNote],
    notes: &[String],
    admitted_entities: &mut u64,
) -> Result<Option<GeometryDecode>, CodecError> {
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    let mut stream_unknowns = Vec::new();
    let mut counts = Counts::default();
    let mut selection_storage = ctx.reserve_scoped(0, "nx active body selection scratch")?;
    let mut body_node_ids = BTreeMap::new();
    let mut parsed = crate::native::substrate::ParsedStreams::parse(ctx, scan)?;
    let mut carrier_refusals: Vec<LossNote> = Vec::new();
    let mut topology_losses: Vec<LossNote> = Vec::new();
    let mut native_losses: Vec<LossNote> = Vec::new();
    let rmfastload_ids = scan
        .container
        .rmfastload_object_id_table()
        .map(|(_, table)| table.object_ids.as_slice())
        .unwrap_or_default();
    for (si, stream) in ctx
        .admit_iter(&scan.streams, "nx geometry body node streams")?
        .enumerate()
    {
        if stream.kind().is_parasolid() {
            let stream_bodies = selection_storage.with_storage(|| {
                topology_body_node_ids(ctx, si, &parsed.stream(si).view_for_geometry().graph)
            })?;
            let mut stream_bodies = stream_bodies.into_iter();
            for _ in ctx.admit_iter(&(0..stream_bodies.len()), "nx geometry body node index")? {
                let Some((body, nodes)) = stream_bodies.next() else {
                    break;
                };
                selection_storage.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut body_node_ids,
                        body,
                        nodes,
                        "nx geometry body node index",
                    )
                })?;
            }
        }
    }
    let rmfastload_selected = selection_storage
        .with_storage(|| rmfastload_selected_bodies(ctx, &body_node_ids, rmfastload_ids))?;
    let allow_terminal_lineage =
        rmfastload_allows_terminal_lineage(body_node_ids.len(), &rmfastload_selected);
    let rmfastload_preselection = if body_node_ids.len() > 1
        && !rmfastload_selected.is_empty()
        && rmfastload_selected.len() < body_node_ids.len()
    {
        selection_storage
            .with_storage(|| rmfastload_stream_indices(ctx, &rmfastload_selected))?
            .map(|streams| {
                (
                    rmfastload_selected,
                    streams,
                    "rmfastload_object_id_membership",
                )
            })
    } else {
        None
    };
    let terminal_lineage = if allow_terminal_lineage {
        Some(crate::native::model::extract_segment_lineage(
            ctx,
            &scan.container,
            &scan.streams,
        )?)
    } else {
        None
    };
    let terminal_preselection = match terminal_lineage.as_ref() {
        Some(lineage) => {
            let mut emitted_body_ids = BTreeSet::new();
            for body in ctx
                .admit_iter(&body_node_ids, "nx emitted terminal body index")?
                .map(|(body, _)| body)
            {
                selection_storage.with_storage(|| {
                    let body =
                        body.try_clone_for_decode(ctx, "nx emitted terminal body identity")?;
                    ctx.insert_btree_set(
                        &mut emitted_body_ids,
                        body,
                        "nx emitted terminal body index",
                    )
                })?;
            }
            let selected = selection_storage.with_storage(|| {
                crate::native::model::terminal_feature_body_ids(
                    ctx,
                    &emitted_body_ids,
                    &lineage.bindings,
                    &lineage.statuses,
                )
            })?;
            match selected.filter(|selected| selected.len() < body_node_ids.len()) {
                Some(selected) => selection_storage
                    .with_storage(|| rmfastload_stream_indices(ctx, &selected))?
                    .map(|streams| (selected, streams, "terminal_feature_body_lineage")),
                None => None,
            }
        }
        None => None,
    };
    let preselection = rmfastload_preselection.or(terminal_preselection);
    let stream_is_selected = |si: usize| -> Result<bool, CodecError> {
        match &preselection {
            Some((_, selected, _)) => {
                ctx.contains_btree_set(selected, &si, "nx preselected geometry streams")
            }
            None => Ok(true),
        }
    };
    let mut chart_count = 0usize;
    for (si, stream) in ctx
        .admit_iter(&scan.streams, "nx completion chart count")?
        .enumerate()
    {
        if stream.kind().is_parasolid() && stream_is_selected(si)? {
            chart_count = chart_count
                .checked_add(
                    parsed
                        .stream(si)
                        .view_for_geometry()
                        .intersections
                        .curves
                        .len(),
                )
                .ok_or_else(|| ctx.refuse_codec_limit("nx completion chart count", 0, 1))?;
        }
    }
    let transfer_limit = completion_transfer_budget_limit(ctx, chart_count)?;
    let support_uv_limit = support_uv_completion_budget_limit(ctx, chart_count)?;
    let exact_transfer_budget = ctx.work_budget(cadmpeg_core::decode::u64_from_index(
        MAX_EXACT_BOUNDARY_TRANSFER_SAMPLES,
    ));
    let transfer_budget = ctx.work_budget(cadmpeg_core::decode::u64_from_index(transfer_limit));
    let support_uv_validation_budget =
        ctx.work_budget(cadmpeg_core::decode::u64_from_index(support_uv_limit));
    let support_budget = ctx.work_budget(cadmpeg_core::decode::u64_from_index(support_uv_limit));
    let coupled_support_budget =
        ctx.work_budget(cadmpeg_core::decode::u64_from_index(support_uv_limit));
    let adaptive_geometry_budget = GeometryWorkBudget::from_context(
        ctx,
        cadmpeg_core::decode::u64_from_index(MAX_ADAPTIVE_GEOMETRY_WORK),
    );
    let completion_geometry_budget = GeometryWorkBudget::from_context(
        ctx,
        cadmpeg_core::decode::u64_from_index(MAX_PCURVE_COMPLETION_GEOMETRY_WORK),
    );
    let support_uv_geometry_budget = GeometryWorkBudget::from_context(
        ctx,
        cadmpeg_core::decode::u64_from_index(MAX_SUPPORT_UV_COMPLETION_GEOMETRY_WORK),
    );
    let coupled_support_uv_geometry_budget = GeometryWorkBudget::from_context(
        ctx,
        cadmpeg_core::decode::u64_from_index(MAX_COUPLED_SUPPORT_UV_GEOMETRY_WORK),
    );
    let serialized_support_uv_geometry_budget = GeometryWorkBudget::from_context(
        ctx,
        cadmpeg_core::decode::u64_from_index(MAX_SERIALIZED_SUPPORT_UV_GEOMETRY_WORK),
    );
    let mut support_uv_lane_geometry_exhausted = false;
    let mut intersection_index = IntersectionIncidenceIndex::default();
    let mut witness_storage = ctx.reserve_scoped(0, "nx model endpoint witnesses")?;
    let mut model_endpoint_witnesses = EndpointWitnesses::new();
    let mut completion_streams = Vec::new();
    let container_stream = StreamHandle::new(
        ctx,
        cadmpeg_ir::stream_name!("nx:container"),
        "allocate annotation stream handle",
    )?;

    for (si, stream) in ctx
        .admit_iter(&scan.streams, "nx geometry streams")?
        .enumerate()
    {
        if !stream.kind().is_parasolid() {
            continue;
        }
        let scope = IdScope::stream_charged(ctx, si)?;
        adaptive_geometry_budget.clear_blend_frame_cache();
        completion_geometry_budget.clear_blend_frame_cache();
        support_uv_geometry_budget.clear_blend_frame_cache();
        coupled_support_uv_geometry_budget.clear_blend_frame_cache();
        serialized_support_uv_geometry_budget.clear_blend_frame_cache();
        if !stream_is_selected(si)? {
            let unknown_index = unknowns.len();
            let unknown = unknown_stream_metadata(ctx, si, stream)?;
            annotations.note(
                ctx,
                unknown.id().as_str(),
                &container_stream,
                cadmpeg_core::decode::u64_from_index(stream.file_offset),
                Some(stream.kind().label()),
            )?;
            annotations.exactness(ctx, unknown.id().as_str(), Exactness::Derived)?;
            ctx.push_vec(&mut unknowns, unknown, "nx geometry unknown streams")?;
            ctx.push_vec(
                &mut stream_unknowns,
                (si, unknown_index),
                "nx geometry unknown indices",
            )?;
            continue;
        }
        let crate::nurbs::Parsed {
            surfaces: nurbs_surfaces,
            curves: nurbs_curves,
            pcurves: nurbs_pcurves,
            refusals: nurbs_refusals,
        } = parsed.parse_nurbs(ctx, si)?;
        for refusal in ctx.admit_iter(&nurbs_refusals, "nx carrier refusal losses")? {
            super::charge_loss_code(ctx, NxLossCode::CarrierLanesUnpaired)?;
            let note = NxLossCode::CarrierLanesUnpaired.note(ctx.format_retained(
                format_args!(
                    "parasolid#{si} {} at byte {} states no carrier: {}",
                    refusal.family, refusal.pos, refusal.error
                ),
                "nx carrier refusal loss text",
            )?);
            ctx.push_vec(&mut carrier_refusals, note, "nx carrier refusal losses")?;
        }
        let view = parsed.stream(si).view_for_geometry();
        let stream_name = ctx.format_retained(
            format_args!("nx:parasolid#{si}:{}", stream.kind().label()),
            "nx geometry stream name",
        )?;
        let source_stream = StreamHandle::new(
            ctx,
            cadmpeg_ir::StreamName::try_from(stream_name).map_err(CodecError::malformed)?,
            "allocate annotation stream handle",
        )?;
        ctx.push_vec(
            &mut completion_streams,
            (si, source_stream.clone()),
            "nx completion streams",
        )?;
        let graph = &view.graph;
        let mut stream_storage = ctx.reserve_scoped(0, "nx geometry stream scratch")?;
        let mut points_by_xmt = BTreeMap::new();
        let mut surfaces_by_xmt = BTreeMap::new();
        let mut curves_by_xmt = BTreeMap::new();
        let mut pcurves_by_xmt = BTreeMap::new();
        let mut pcurve_supports_by_xmt: BTreeMap<u32, SurfaceId> = BTreeMap::new();
        let mut trim_ranges = BTreeMap::new();
        let mut pending_blend_supports = Vec::new();
        let mut pending_blend_spines = Vec::new();
        let mut pending_ext11_support_uv = Vec::new();
        // Admission validates each populated lane against its selected support
        // surface. Later topology completion only fills absent pcurves, so
        // this provenance remains valid through the invalidation pass.
        let mut validated_support_uv_lanes = BTreeSet::new();
        let first_surface = ir.model.surfaces.len();
        let first_curve = ir.model.curves.len();
        // The model is accumulated across streams. Completion must not retry
        // unresolved curves that an earlier stream already admitted.
        let procedural_start = ir.model.procedural_curves.len();
        let point_candidates =
            stream_storage.with_storage(|| ordered_point_candidates(ctx, graph))?;
        for (pi, &(position, node)) in ctx
            .admit_iter(&point_candidates, "nx geometry points")?
            .enumerate()
        {
            let pid: PointId = scope.id_charged(ctx, &cadmpeg_ir::identity_component!("pt"), pi)?;
            let vid: VertexId = scope.id_charged(ctx, &cadmpeg_ir::identity_component!("v"), pi)?;
            annotate_node(
                ctx,
                &mut annotations,
                pid.as_str(),
                &source_stream,
                node,
                "POINT",
            )?;
            annotations.derived(ctx, pid.as_str(), "position")?;
            stream_storage.with_storage(|| {
                let key = pid.try_clone_for_decode(ctx, "nx point node index")?;
                ctx.insert_btree_map(&mut points_by_xmt, node.xmt(), key, "nx point node index")
            })?;
            let point = Point::new(
                pid.try_clone_for_decode(ctx, "nx geometry point identity")?,
                position,
                None,
            );
            ctx.push_vec(&mut ir.model.points, point, "nx geometry points")?;
            let vertex = Vertex {
                id: vid,
                point: pid,
                tolerance: None,
            };
            ctx.push_vec(&mut ir.model.vertices, vertex, "nx geometry point vertices")?;
            counts.points += 1;
        }
        drop(point_candidates);
        let surface_candidates =
            stream_storage.with_storage(|| ordered_surface_candidates(ctx, graph))?;
        let mut surface_candidates = surface_candidates.into_iter();
        for fi in ctx.admit_iter(&(0..surface_candidates.len()), "nx geometry surfaces")? {
            let Some((geometry, node)) = surface_candidates.next() else {
                break;
            };
            match &geometry {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => counts.planes += 1,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_)) => {
                    counts.cylinders += 1;
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_)) => counts.cones += 1,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_)) => counts.spheres += 1,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_)) => counts.tori += 1,
                SurfaceGeometry::Solved(
                    SolvedSurfaceGeometry::Nurbs(_)
                    | SolvedSurfaceGeometry::Polygonal(_)
                    | SolvedSurfaceGeometry::Transformed(_)
                    | SolvedSurfaceGeometry::Unknown { .. },
                )
                | SurfaceGeometry::Procedural { .. } => {}
            }
            let id: SurfaceId =
                scope.id_charged(ctx, &cadmpeg_ir::identity_component!("surf"), fi)?;
            annotate_node(
                ctx,
                &mut annotations,
                id.as_str(),
                &source_stream,
                node,
                surface_tag(geometry.solved().ok_or_else(|| {
                    cadmpeg_core::CodecError::NotImplemented(
                        "carrier has no solved geometry".into(),
                    )
                })?),
            )?;
            annotations.derived(ctx, id.as_str(), "geometry")?;
            stream_storage.with_storage(|| {
                let key = id.try_clone_for_decode(ctx, "nx surface node index")?;
                ctx.insert_btree_map(
                    &mut surfaces_by_xmt,
                    node.xmt(),
                    key,
                    "nx surface node index",
                )
            })?;
            let surface = Surface {
                id,
                geometry,
                source_object: None,
            };
            ctx.push_vec(&mut ir.model.surfaces, surface, "nx geometry surfaces")?;
        }
        drop(surface_candidates);
        let mut nurbs_surfaces = nurbs_surfaces.into_iter();
        for fi in ctx.admit_iter(&(0..nurbs_surfaces.len()), "nx geometry NURBS surfaces")? {
            let Some(surf) = nurbs_surfaces.next() else {
                break;
            };
            counts.nurbs_surfaces += 1;
            let id: SurfaceId =
                scope.id_charged(ctx, &cadmpeg_ir::identity_component!("nurbs-surf"), fi)?;
            annotations.note(
                ctx,
                id.as_str(),
                &source_stream,
                cadmpeg_core::decode::u64_from_index(surf.pos),
                Some("B_SPLINE_SURFACE"),
            )?;
            annotations.derived(ctx, id.as_str(), "geometry")?;
            if let Some(node) = graph.at_pos(surf.pos) {
                stream_storage.with_storage(|| {
                    let key = id.try_clone_for_decode(ctx, "nx surface node index")?;
                    ctx.insert_btree_map(
                        &mut surfaces_by_xmt,
                        node.xmt(),
                        key,
                        "nx surface node index",
                    )
                })?;
            }
            let surface = Surface {
                id,
                geometry: surf.geometry,
                source_object: None,
            };
            ctx.push_vec(&mut ir.model.surfaces, surface, "nx geometry surfaces")?;
        }
        let saved_offset_carriers = saved_offset_carriers(
            ctx,
            &ir,
            graph,
            &view.offset_surfaces,
            &surfaces_by_xmt,
            ir.tolerances.linear,
            &adaptive_geometry_budget,
        )?;
        for (oi, offset) in ctx
            .admit_iter(&view.offset_surfaces, "nx offset surfaces")?
            .copied()
            .enumerate()
        {
            let Some(support_ref) = ctx.get_btree_map(
                &surfaces_by_xmt,
                &offset.state.support(),
                "nx surface node index",
            )?
            else {
                continue;
            };
            let support: SurfaceId = support_ref.try_clone_for_decode(ctx, "nx offset support")?;
            let procedural_id: ProceduralSurfaceId =
                scope.id_charged(ctx, &cadmpeg_ir::identity_component!("offset"), oi)?;
            let saved = ctx.get_btree_map(
                &saved_offset_carriers,
                &offset.xmt,
                "nx saved offset carriers",
            )?;
            let (surface_id, cache_fit_tolerance) = if let Some((surface, fit_tolerance)) = saved {
                (
                    stream_storage.with_storage(|| {
                        surface.try_clone_for_decode(ctx, "nx saved offset surface")
                    })?,
                    Some(*fit_tolerance),
                )
            } else {
                let surface_id: SurfaceId =
                    scope.id_charged(ctx, &cadmpeg_ir::identity_component!("offset-surf"), oi)?;
                annotations.note(
                    ctx,
                    surface_id.as_str(),
                    &source_stream,
                    cadmpeg_core::decode::u64_from_index(offset.pos),
                    Some("OFFSET_SURF"),
                )?;
                annotations.derived(ctx, surface_id.as_str(), "geometry")?;
                let owner = stream_storage.with_storage(|| {
                    surface_id.try_clone_for_decode(ctx, "nx offset construction owner")
                })?;
                let surface = Surface {
                    id: surface_id,
                    geometry: SurfaceGeometry::Procedural {
                        construction: procedural_id
                            .try_clone_for_decode(ctx, "nx offset construction identity")?,
                        cache: None,
                    },
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Nx,
                        object_id: cadmpeg_core::text::NonBlankString::for_decode(
                            ctx,
                            ctx.format_retained(
                                format_args!("nx:s{si}:offset-surface-record#{}", offset.xmt),
                                "nx offset source object identity",
                            )?,
                            "validate nonblank text",
                        )?
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "source object_id must not be empty",
                            )
                        })?,
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                };
                ctx.push_vec(&mut ir.model.surfaces, surface, "nx offset surfaces")?;
                (owner, None)
            };
            annotations.note(
                ctx,
                procedural_id.as_str(),
                &source_stream,
                cadmpeg_core::decode::u64_from_index(offset.pos),
                Some("OFFSET_SURF"),
            )?;
            annotations.derived(ctx, procedural_id.as_str(), "definition")?;
            let admitted_payload =
                cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::legacy(
                    support,
                    offset.state.distance(),
                    None,
                    None,
                    true,
                    cadmpeg_ir::geometry::LegacyExtensionFlags::Absent {},
                    None,
                );
            let mut definition = ProceduralSurfaceDefinition::Offset(admitted_payload);
            if let Some(tolerance) = cache_fit_tolerance {
                let cache = cadmpeg_ir::geometry::LegacyCache::try_new(tolerance)
                    .map_err(cadmpeg_core::CodecError::malformed)?;
                definition
                    .set_legacy_cache(Some(cache))
                    .map_err(cadmpeg_core::CodecError::malformed)?;
            }
            let procedural = ProceduralSurface::new(procedural_id, definition, None);

            let _attached = ir
                .model
                .add_procedural_surface(ctx, &surface_id, procedural)?;

            stream_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut surfaces_by_xmt,
                    offset.xmt,
                    surface_id,
                    "nx surface node index",
                )
            })?;
            counts.offset_surfaces += 1;
        }

        for (bi, blend) in ctx
            .admit_iter(&view.blend_surfaces, "nx blend surfaces")?
            .copied()
            .enumerate()
        {
            let surface_id: SurfaceId =
                scope.id_charged(ctx, &cadmpeg_ir::identity_component!("blend-surf"), bi)?;
            let procedural_id: ProceduralSurfaceId =
                scope.id_charged(ctx, &cadmpeg_ir::identity_component!("blend"), bi)?;
            annotations.note(
                ctx,
                surface_id.as_str(),
                &source_stream,
                cadmpeg_core::decode::u64_from_index(blend.pos),
                Some("BLEND_SURF"),
            )?;
            annotations.derived(ctx, surface_id.as_str(), "geometry")?;
            let owner = stream_storage.with_storage(|| {
                surface_id.try_clone_for_decode(ctx, "nx blend construction owner")
            })?;
            let surface = Surface {
                id: surface_id,
                geometry: SurfaceGeometry::Procedural {
                    construction: procedural_id
                        .try_clone_for_decode(ctx, "nx blend construction identity")?,
                    cache: None,
                },
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Nx,
                    object_id: cadmpeg_core::text::NonBlankString::for_decode(
                        ctx,
                        ctx.format_retained(
                            format_args!("nx:s{si}:blend-surface-record#{}", blend.xmt),
                            "nx blend source object identity",
                        )?,
                        "validate nonblank text",
                    )?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            };
            ctx.push_vec(&mut ir.model.surfaces, surface, "nx blend surfaces")?;
            annotations.note(
                ctx,
                procedural_id.as_str(),
                &source_stream,
                cadmpeg_core::decode::u64_from_index(blend.pos),
                Some("BLEND_SURF"),
            )?;
            annotations.derived(ctx, procedural_id.as_str(), "definition")?;
            let procedural_index = ir.model.procedural_surfaces.len();

            let attached = ir.model.add_procedural_surface(
                ctx,
                &owner,
                ProceduralSurface::new(
                    procedural_id,
                    ProceduralSurfaceDefinition::Blend(
                        cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload::try_new(
                            [None, None],
                            None,
                            BlendRadiusLaw::Constant {
                                signed_radius: cadmpeg_ir::scalar::Length::from(
                                    blend.state.first_offset(),
                                ),
                            },
                            BlendCrossSection::Circular,
                            cadmpeg_ir::geometry::CacheContract::from_form(None),
                        )
                        .map_err(cadmpeg_core::CodecError::malformed)?,
                    ),
                    None,
                ),
            )?;
            if attached.is_ok() {
                ctx.push_scoped_vec(
                    &mut stream_storage,
                    &mut pending_blend_supports,
                    (
                        procedural_index,
                        blend.state.support_xmts(),
                        blend.state.offsets(),
                    ),
                    "nx pending blend supports",
                )?;
                if blend.state.spine_xmt() > 1 {
                    ctx.push_scoped_vec(
                        &mut stream_storage,
                        &mut pending_blend_spines,
                        (procedural_index, blend.state.spine_xmt()),
                        "nx pending blend spines",
                    )?;
                }
            }
            stream_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut surfaces_by_xmt,
                    blend.xmt,
                    owner,
                    "nx surface node index",
                )
            })?;
            counts.blend_surfaces += 1;
        }
        for &(procedural_index, support_xmts, offsets) in
            ctx.admit_iter(&pending_blend_supports, "nx pending blend supports")?
        {
            let mut supports = [None, None];
            for (side, support) in supports.iter_mut().enumerate() {
                if let Some(surface) = ctx.get_btree_map(
                    &surfaces_by_xmt,
                    &support_xmts[side],
                    "nx surface node index",
                )? {
                    *support = Some(BlendSupport {
                        surface: surface.try_clone_for_decode(ctx, "nx blend support identity")?,
                        reversed: offsets[side].is_sign_negative(),
                    });
                }
            }
            let Some(procedural) = ir.model.procedural_surfaces.get_mut(procedural_index) else {
                continue;
            };
            procedural.edit_definition(|definition| {
                if let ProceduralSurfaceDefinition::Blend(definition_payload) = definition {
                    definition_payload.set_supports(supports);
                }
            });
        }

        let curve_candidates =
            stream_storage.with_storage(|| ordered_curve_candidates(ctx, graph))?;
        let mut curve_candidates = curve_candidates.into_iter();
        for ci in ctx.admit_iter(&(0..curve_candidates.len()), "nx geometry curves")? {
            let Some((geometry, node)) = curve_candidates.next() else {
                break;
            };
            match &geometry {
                CurveGeometry::Solved(SolvedCurveGeometry::Line(_)) => counts.lines += 1,
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(_)) => counts.circles += 1,
                CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(_)) => counts.ellipses += 1,
                CurveGeometry::Solved(SolvedCurveGeometry::Parabola(_)) => {}
                CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(_)) => {}
                CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(_)) => {}
                CurveGeometry::Solved(SolvedCurveGeometry::Composite { .. }) => {}
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(_)) => {}
                CurveGeometry::Procedural { .. } => {}
                CurveGeometry::Solved(SolvedCurveGeometry::Polyline(_)) => {}
                CurveGeometry::Solved(SolvedCurveGeometry::Transformed(_)) => {}
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. }) => {}
            }
            let id: CurveId = scope.id_charged(ctx, &cadmpeg_ir::identity_component!("crv"), ci)?;
            annotate_node(
                ctx,
                &mut annotations,
                id.as_str(),
                &source_stream,
                node,
                curve_tag(geometry.solved().ok_or_else(|| {
                    cadmpeg_core::CodecError::NotImplemented(
                        "carrier has no solved geometry".into(),
                    )
                })?),
            )?;
            annotations.derived(ctx, id.as_str(), "geometry")?;
            stream_storage.with_storage(|| {
                let key = id.try_clone_for_decode(ctx, "nx curve node index")?;
                ctx.insert_btree_map(&mut curves_by_xmt, node.xmt(), key, "nx curve node index")
            })?;
            let curve = Curve {
                id,
                geometry,
                source_object: None,
            };
            ctx.push_vec(&mut ir.model.curves, curve, "nx geometry curves")?;
        }
        drop(curve_candidates);
        let mut nurbs_curves = nurbs_curves.into_iter();
        for ci in ctx.admit_iter(&(0..nurbs_curves.len()), "nx NURBS curves")? {
            let Some(crv) = nurbs_curves.next() else {
                break;
            };
            counts.nurbs_curves += 1;
            let id: CurveId =
                scope.id_charged(ctx, &cadmpeg_ir::identity_component!("nurbs-crv"), ci)?;
            annotations.note(
                ctx,
                id.as_str(),
                &source_stream,
                cadmpeg_core::decode::u64_from_index(crv.pos),
                Some("B_SPLINE_CURVE"),
            )?;
            annotations.derived(ctx, id.as_str(), "geometry")?;
            if let Some(node) = graph.at_pos(crv.pos) {
                stream_storage.with_storage(|| {
                    let key = id.try_clone_for_decode(ctx, "nx curve node index")?;
                    ctx.insert_btree_map(&mut curves_by_xmt, node.xmt(), key, "nx curve node index")
                })?;
            }
            let curve = Curve {
                id,
                geometry: crv.geometry,
                source_object: None,
            };
            ctx.push_vec(&mut ir.model.curves, curve, "nx NURBS curves")?;
        }

        let mut nurbs_pcurves = nurbs_pcurves.into_iter();
        for pi in ctx.admit_iter(&(0..nurbs_pcurves.len()), "nx NURBS pcurves")? {
            let Some(pcurve) = nurbs_pcurves.next() else {
                break;
            };
            let id: PcurveId =
                scope.id_charged(ctx, &cadmpeg_ir::identity_component!("pcurve"), pi)?;
            annotations.note(
                ctx,
                id.as_str(),
                &source_stream,
                cadmpeg_core::decode::u64_from_index(pcurve.pos),
                Some("B_CURVE_2D"),
            )?;
            annotations.derived(ctx, id.as_str(), "geometry")?;
            if let Some(node) = graph.at_pos(pcurve.pos) {
                stream_storage.with_storage(|| {
                    let key = id.try_clone_for_decode(ctx, "nx pcurve node index")?;
                    ctx.insert_btree_map(
                        &mut pcurves_by_xmt,
                        node.xmt(),
                        key,
                        "nx pcurve node index",
                    )
                })?;
            }
            let pcurve = Pcurve {
                id,
                geometry: pcurve.geometry,
                metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::default(),
            };
            ctx.push_vec(&mut ir.model.pcurves, pcurve, "nx NURBS pcurves")?;
        }
        let intersections = &view.intersections;
        counts
            .intersection_rejections
            .extend(intersections.rejected);
        let mut charted_intersections = BTreeMap::new();
        for curve in ctx.admit_iter(&intersections.curves, "nx charted intersection index")? {
            stream_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut charted_intersections,
                    curve.xmt,
                    curve,
                    "nx charted intersection index",
                )
            })?;
        }
        let mut uncharted_intersections = BTreeMap::new();
        for curve in ctx.admit_iter(&intersections.uncharted, "nx uncharted intersection index")? {
            stream_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut uncharted_intersections,
                    curve.xmt,
                    curve,
                    "nx uncharted intersection index",
                )
            })?;
        }
        let mut intersection_support_uv = BTreeMap::new();
        {
            let mut index_storage = ctx.reserve_scoped(0, "nx intersection support model index")?;
            let model_index = index_storage
                .with_storage_limit(|| cadmpeg_ir::index::ModelIndex::new_model_only(&ir, ctx))?;
            for construction in ctx.admit_iter(
                &intersections.constructions,
                "nx intersection support UV index",
            )? {
                let Some(&charted) = ctx.get_btree_map(
                    &charted_intersections,
                    &construction.xmt,
                    "nx charted intersection index",
                )?
                else {
                    continue;
                };
                let mut sample_storage = ctx.reserve_scoped(0, "nx intersection chart points")?;
                let points = sample_storage.with_storage(|| charted.samples.points_charged(ctx))?;
                let support_uv = stream_storage.with_storage(|| {
                    let mut support_uv = validate_serialized_support_uv_with_index(
                        ctx,
                        &model_index,
                        &crate::decode::support_uv::SerializedSupportUvFit {
                            surfaces_by_xmt: &surfaces_by_xmt,
                            supports: [Some(charted.primary_support), charted.secondary_support],
                            points: &points,
                            fit_tolerance: charted.fit_tolerance.get(),
                            lanes: &charted.support_uv,
                        },
                        &serialized_support_uv_geometry_budget,
                    )?;
                    if let Some(ext_support_uv) = assign_ext11_support_uv_with_index(
                        ctx,
                        &model_index,
                        &crate::decode::support_uv::SerializedSupportUvFit {
                            surfaces_by_xmt: &surfaces_by_xmt,
                            supports: [Some(charted.primary_support), charted.secondary_support],
                            points: &points,
                            fit_tolerance: charted.fit_tolerance.get(),
                            lanes: &charted.ext_support_uv,
                        },
                        &serialized_support_uv_geometry_budget,
                    )? {
                        for side in 0..2 {
                            if support_uv[side].is_none() {
                                support_uv[side] =
                                    copy_support_uv_lane(ctx, ext_support_uv[side].as_ref())?;
                            }
                        }
                    }
                    ctx.insert_btree_map(
                        &mut intersection_support_uv,
                        construction.xmt,
                        support_uv,
                        "nx intersection support UV index",
                    )
                    .map(drop)
                });
                support_uv?;
            }
        }
        for (ci, construction) in ctx
            .admit_iter(&intersections.constructions, "nx intersection curves")?
            .enumerate()
        {
            let curve_id: CurveId = scope.id_charged(
                ctx,
                &cadmpeg_ir::identity_component!("intersection-crv"),
                ci,
            )?;
            let procedural_id: ProceduralCurveId =
                scope.id_charged(ctx, &cadmpeg_ir::identity_component!("intersection"), ci)?;
            let charted = ctx
                .get_btree_map(
                    &charted_intersections,
                    &construction.xmt,
                    "nx charted intersection index",
                )?
                .copied();
            let uncharted = match ctx.get_btree_map(
                &uncharted_intersections,
                &construction.xmt,
                "nx uncharted intersection index",
            )? {
                Some(uncharted) => {
                    let [first, second] = uncharted.supports.references();
                    let first = ctx.get_btree_map(
                        &surfaces_by_xmt,
                        &u32::from(first),
                        "nx surface node index",
                    )?;
                    let second = ctx.get_btree_map(
                        &surfaces_by_xmt,
                        &u32::from(second),
                        "nx surface node index",
                    )?;
                    if let (Some(first), Some(second)) = (first, second) {
                        cadmpeg_ir::geometry::TolerantIntersectionConstruction::from_parts(
                            [
                                first.try_clone_for_decode(ctx, "nx uncharted first support")?,
                                second.try_clone_for_decode(ctx, "nx uncharted second support")?,
                            ],
                            uncharted.endpoints,
                            uncharted.tolerance.into(),
                        )
                        .ok()
                    } else {
                        None
                    }
                }
                None => None,
            };
            let unknown_id: Option<UnknownId> = if charted.is_none() && uncharted.is_none() {
                Some(IdScope::container().id_charged(
                    ctx,
                    &cadmpeg_ir::identity_component!("parasolid"),
                    si,
                )?)
            } else {
                None
            };
            let support_uv = ctx.get_btree_map(
                &intersection_support_uv,
                &construction.xmt,
                "nx intersection support UV index",
            )?;
            let mut sample_storage = ctx.reserve_scoped(0, "nx intersection chart samples")?;
            let parameters = match charted {
                Some(charted) => {
                    Some(sample_storage.with_storage(|| charted.samples.parameters_charged(ctx))?)
                }
                None => None,
            };
            if let Some(charted) = charted {
                if let Some(support_uv) = support_uv {
                    for (side, lane) in support_uv.iter().enumerate() {
                        if lane.is_some() {
                            stream_storage.with_storage(|| {
                                let construction = procedural_id.try_clone_for_decode(
                                    ctx,
                                    "nx validated support UV construction",
                                )?;
                                ctx.insert_btree_set(
                                    &mut validated_support_uv_lanes,
                                    (construction, side),
                                    "nx validated support UV lanes",
                                )
                            })?;
                        }
                    }
                }
                stream_storage.with_storage(|| {
                    let pending = (
                        procedural_id.try_clone_for_decode(ctx, "nx pending EXT11 construction")?,
                        charted.samples.clone_charged(ctx)?,
                        charted.fit_tolerance.get(),
                        SerializedSupportUv {
                            values: [
                                copy_support_uv_lane(ctx, charted.support_uv[0].as_ref())?,
                                copy_support_uv_lane(ctx, charted.support_uv[1].as_ref())?,
                            ],
                            ext11: [
                                copy_support_uv_lane(ctx, charted.ext_support_uv[0].as_ref())?,
                                copy_support_uv_lane(ctx, charted.ext_support_uv[1].as_ref())?,
                            ],
                        },
                    );
                    ctx.push_vec(
                        &mut pending_ext11_support_uv,
                        pending,
                        "nx pending EXT11 support UV",
                    )
                })?;
            }
            annotations.note(
                ctx,
                curve_id.as_str(),
                &source_stream,
                cadmpeg_core::decode::u64_from_index(construction.pos),
                Some("INTERSECTION"),
            )?;
            if charted.is_some() || uncharted.is_some() {
                annotations.derived(ctx, curve_id.as_str(), "geometry")?;
            } else {
                annotations.exactness(ctx, curve_id.as_str(), Exactness::Unknown)?;
            }
            let geometry = match (charted, &parameters) {
                (Some(charted), Some(parameters)) => {
                    let curve = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                        ctx,
                        1,
                        linear_knots(parameters, &adaptive_geometry_budget)?,
                        charted.samples.points_charged(ctx)?,
                        None,
                        false,
                    )?;
                    match curve {
                        Ok(curve) => CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)),
                        Err(error) => {
                            return Err(CodecError::Malformed(ctx.format_retained(
                                format_args!("{error}"),
                                "nx intersection chart carrier error",
                            )?));
                        }
                    }
                }
                _ if uncharted.is_some() => CurveGeometry::Procedural {
                    construction: procedural_id
                        .try_clone_for_decode(ctx, "nx intersection construction identity")?,
                    cache: None,
                },
                _ => CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                    record: unknown_id
                        .as_ref()
                        .map(|unknown| {
                            unknown.try_clone_for_decode(ctx, "nx unknown intersection record")
                        })
                        .transpose()?,
                }),
            };
            let owner = stream_storage.with_storage(|| {
                curve_id.try_clone_for_decode(ctx, "nx intersection owner identity")
            })?;
            let curve = Curve {
                id: curve_id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Nx,
                    object_id: cadmpeg_core::text::NonBlankString::for_decode(
                        ctx,
                        ctx.format_retained(
                            format_args!("nx:s{si}:intersection-record#{}", construction.xmt),
                            "nx intersection source object identity",
                        )?,
                        "validate nonblank text",
                    )?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            };
            ctx.push_vec(&mut ir.model.curves, curve, "nx intersection curves")?;
            annotations.note(
                ctx,
                procedural_id.as_str(),
                &source_stream,
                cadmpeg_core::decode::u64_from_index(construction.pos),
                Some("INTERSECTION"),
            )?;
            if charted.is_some() || uncharted.is_some() {
                annotations.derived(ctx, procedural_id.as_str(), "definition")?;
            } else {
                annotations.exactness(ctx, procedural_id.as_str(), Exactness::Unknown)?;
            }
            let definition = if let (Some(charted), Some(parameters)) = (charted, &parameters) {
                let support_uv = match support_uv {
                    Some(lanes) => [
                        copy_support_uv_lane(ctx, lanes[0].as_ref())?,
                        copy_support_uv_lane(ctx, lanes[1].as_ref())?,
                    ],
                    None => [None, None],
                };
                let first = intersection_side(
                    ctx,
                    &ir,
                    &surfaces_by_xmt,
                    Some(charted.primary_support),
                    support_uv[0]
                        .as_deref()
                        .map(|uv| (uv, parameters.as_slice())),
                    &adaptive_geometry_budget,
                )?;
                let second = intersection_side(
                    ctx,
                    &ir,
                    &surfaces_by_xmt,
                    charted.secondary_support,
                    support_uv[1]
                        .as_deref()
                        .map(|uv| (uv, parameters.as_slice())),
                    &adaptive_geometry_budget,
                )?;
                ProceduralCurveDefinition::Intersection {
                    context: IntcurveSupportContext::try_new(
                        [first, second],
                        charted.samples.parameter_range(),
                        [Vec::new(), Vec::new(), Vec::new()],
                    )
                    .map_err(cadmpeg_core::CodecError::malformed)?,
                    discontinuity_flag: false,
                    cache: None,
                }
            } else if let Some(construction) = uncharted {
                ProceduralCurveDefinition::TolerantIntersection {
                    construction,
                    parameterization: None,
                    cache: None,
                }
            } else {
                ProceduralCurveDefinition::Unknown {
                    native_kind: Some("nx:intersection".into()),
                    record: unknown_id,
                    cache: None,
                }
            };
            drop(parameters);
            drop(sample_storage);
            let mut definition = definition;
            if let Some(charted) = charted {
                definition
                    .set_legacy_cache(cadmpeg_ir::geometry::LegacyCache::new(
                        charted.fit_tolerance,
                    ))
                    .map_err(cadmpeg_core::CodecError::malformed)?;
            }
            let procedural = ProceduralCurve::new(procedural_id, definition);

            let _attached = ir.model.add_procedural_curve(ctx, &owner, procedural)?;

            stream_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut curves_by_xmt,
                    construction.xmt,
                    owner,
                    "nx curve node index",
                )
            })?;
            counts.intersection_curves += 1;
        }
        for &(procedural_index, spine_xmt) in
            ctx.admit_iter(&pending_blend_spines, "nx pending blend spines")?
        {
            let Some(spine_ref) =
                ctx.get_btree_map(&curves_by_xmt, &spine_xmt, "nx curve node index")?
            else {
                continue;
            };
            let spine = spine_ref.try_clone_for_decode(ctx, "nx blend spine identity")?;
            let Some(procedural) = ir.model.procedural_surfaces.get_mut(procedural_index) else {
                continue;
            };
            procedural.edit_definition(|definition| {
                if let ProceduralSurfaceDefinition::Blend(definition_payload) = definition {
                    definition_payload.set_spine(Some(spine));
                }
            });
        }
        let mut invalid_pcurves: BTreeSet<PcurveId> = BTreeSet::new();
        map_stream_carriers(
            ctx,
            &mut stream_storage,
            &mut ir,
            &view.trimmed_curves,
            &view.surface_curves,
            CarrierMaps {
                surfaces: &surfaces_by_xmt,
                curves: &mut curves_by_xmt,
                pcurves: &mut pcurves_by_xmt,
                pcurve_supports: &mut pcurve_supports_by_xmt,
                trim_ranges: &mut trim_ranges,
            },
            &mut invalid_pcurves,
        )?;
        if !invalid_pcurves.is_empty() {
            let marks = keep_marks(
                ctx,
                &mut stream_storage,
                &ir.model.pcurves,
                |candidate| {
                    Ok(!ctx.contains_btree_set(
                        &invalid_pcurves,
                        &candidate.id,
                        "nx invalid pcurves",
                    )?)
                },
                "nx invalid pcurves",
            )?;
            retain_marked(&mut ir.model.pcurves, marks);
            intersection_index.reindex_pcurves_after_prune(ctx, &ir)?;
        }
        retain_unresolved_topology_carriers(
            ctx,
            &mut ir,
            crate::decode::emit::UnresolvedTopologyStream {
                stream_index: si,
                graph,
                surfaces: &mut surfaces_by_xmt,
                curves: &mut curves_by_xmt,
                pcurves: &pcurves_by_xmt,
                source_stream: &source_stream,
                storage: &mut stream_storage,
            },
            &mut annotations,
        )?;
        let intersection_starts = IntersectionEntityStarts {
            loops: ir.model.loops.len(),
            faces: ir.model.faces.len(),
            edges: ir.model.edges.len(),
            coedges: ir.model.coedges.len(),
            pcurves: ir.model.pcurves.len(),
            procedural_curves: procedural_start,
        };
        adaptive_geometry_budget.clear_blend_frame_cache();
        completion_geometry_budget.clear_blend_frame_cache();
        let initial_endpoint_witnesses = emit_topology(
            ctx,
            &mut ir,
            &crate::decode::emit::TopologyStream {
                stream_index: si,
                graph,
                points: &points_by_xmt,
                surfaces: &surfaces_by_xmt,
                curves: &curves_by_xmt,
                pcurves: &pcurves_by_xmt,
                pcurve_supports: &pcurve_supports_by_xmt,
                trim_ranges: &trim_ranges,
                source_stream: &source_stream,
                intersection_starts,
                procedural_start,
            },
            &mut annotations,
            &mut intersection_index,
            &crate::decode::emit::TopologyBudgets {
                exact_transfer: &exact_transfer_budget,
                completion_transfer: &transfer_budget,
                adaptive_geometry: &adaptive_geometry_budget,
                completion_geometry: &completion_geometry_budget,
            },
            &mut topology_losses,
        )?;
        // Topology completion adds incidence and pcurve carriers, but does
        // not change surface or model-curve geometry. Keep its successful
        // blend-geometry certificates for support validation and attachment.
        // Once earlier completion has consumed half the shared allowance,
        // isolate each remaining validation lane. This preserves ordinary
        // whole-lane proofs while preventing one recursive support from
        // starving every later attachment candidate.
        let isolate_validation_lanes =
            completion_geometry_budget.remaining() <= MAX_PCURVE_COMPLETION_GEOMETRY_WORK / 2;
        let support_uv_validation =
            invalidate_inconsistent_support_uv_with_validated_lanes_and_status(
                ctx,
                &mut ir,
                &pending_ext11_support_uv,
                &validated_support_uv_lanes,
                &support_uv_validation_budget,
                &completion_geometry_budget,
                isolate_validation_lanes,
            )?;
        support_uv_lane_geometry_exhausted |= support_uv_validation.lane_geometry_exhausted;
        let newly_validated_endpoint_witnesses = support_uv_validation.endpoint_witnesses;
        serialized_support_uv_geometry_budget.clear_blend_frame_cache();
        complete_ext11_support_uv_with_budget(
            ctx,
            &mut ir,
            &pending_ext11_support_uv,
            &serialized_support_uv_geometry_budget,
        )?;
        support_uv_geometry_budget.clear_blend_frame_cache();
        coupled_support_uv_geometry_budget.clear_blend_frame_cache();
        complete_parameterization_equivalent_support_uv(ctx, &mut ir)?;
        let mut completed_endpoint_witnesses = BTreeMap::new();
        support_uv_lane_geometry_exhausted |=
            complete_support_uv_with_budget_and_endpoint_witnesses(
                ctx,
                &mut ir,
                &pending_ext11_support_uv,
                (&support_budget, &support_uv_geometry_budget),
                (&coupled_support_budget, &coupled_support_uv_geometry_budget),
                &mut completed_endpoint_witnesses,
            )?;
        let mut validated_endpoint_witnesses = initial_endpoint_witnesses;
        extend_endpoint_witnesses(
            ctx,
            &mut validated_endpoint_witnesses,
            validated_support_uv_endpoint_witnesses(
                ctx,
                &ir,
                &pending_ext11_support_uv,
                &validated_support_uv_lanes,
            )?,
        )?;
        extend_endpoint_witnesses(
            ctx,
            &mut validated_endpoint_witnesses,
            newly_validated_endpoint_witnesses,
        )?;
        extend_endpoint_witnesses(
            ctx,
            &mut validated_endpoint_witnesses,
            completed_endpoint_witnesses,
        )?;
        attach_completed_intersection_pcurves_for_stream_with_budget(
            ctx,
            &mut ir,
            crate::decode::support_uv::IntersectionStream {
                graph,
                scope: &scope,
                coedge_start: intersection_starts.coedges,
                procedural_start: intersection_starts.procedural_curves,
                source_stream: source_stream.clone(),
                validated_endpoint_witnesses: &validated_endpoint_witnesses,
            },
            &mut annotations,
            &completion_geometry_budget,
        )?;
        witness_storage.with_storage(|| {
            extend_endpoint_witnesses(
                ctx,
                &mut model_endpoint_witnesses,
                validated_endpoint_witnesses,
            )
        })?;
        // Preserve the whole inflated stream verbatim so nothing is dropped.
        let unknown_index = unknowns.len();
        let mut unknown = unknown_stream_metadata(ctx, si, stream)?;
        for surface in ctx.admit_iter(
            ir.model.surfaces.get(first_surface..).unwrap_or_default(),
            "nx unknown entity links",
        )? {
            let link =
                ctx.copy_retained_text(surface.id.as_str(), "nx unknown entity link text")?;
            ctx.push_vec(unknown.links_mut(), link, "nx unknown entity links")?;
        }
        for curve in ctx.admit_iter(
            ir.model.curves.get(first_curve..).unwrap_or_default(),
            "nx unknown entity links",
        )? {
            let link = ctx.copy_retained_text(curve.id.as_str(), "nx unknown entity link text")?;
            ctx.push_vec(unknown.links_mut(), link, "nx unknown entity links")?;
        }
        annotations.note(
            ctx,
            unknown.id().as_str(),
            &container_stream,
            cadmpeg_core::decode::u64_from_index(stream.file_offset),
            Some(stream.kind().label()),
        )?;
        annotations.exactness(ctx, unknown.id().as_str(), Exactness::Derived)?;
        ctx.push_vec(&mut unknowns, unknown, "nx geometry unknown streams")?;
        ctx.push_vec(
            &mut stream_unknowns,
            (si, unknown_index),
            "nx geometry unknown indices",
        )?;
    }

    intersection_index.complete_from_model(ctx, &mut ir)?;
    let mut completion_storage = ctx.reserve_scoped(0, "nx completion sources")?;
    let mut completion_sources = Vec::new();
    for (si, source_stream) in ctx.admit_iter(&completion_streams, "nx completion sources")? {
        let source = IntersectionCompletionSource {
            scope: completion_storage.with_storage(|| IdScope::stream_charged(ctx, *si))?,
            graph: parsed.stream(*si).view_for_geometry().graph.as_ref(),
            source_stream: source_stream.clone(),
            coedge_start: 0,
            procedural_start: 0,
        };
        ctx.push_scoped_vec(
            &mut completion_storage,
            &mut completion_sources,
            source,
            "nx completion sources",
        )?;
    }
    attach_completed_intersection_pcurves_for_model_with_budget(
        ctx,
        &mut ir,
        &completion_sources,
        &mut annotations,
        &model_endpoint_witnesses,
        &completion_geometry_budget,
    )?;
    drop(completion_sources);
    drop(completion_storage);

    if counts.points == 0 && counts.surfaces() == 0 && counts.curves() == 0 {
        return Ok(None);
    }

    ir.source = Some(source_meta(ctx, scan, dialects)?);

    ctx.admit_entities(
        cadmpeg_core::decode::u64_from_index(ir.model.entity_count()),
        admitted_entities,
        "admit NX entities",
    )?;

    // Extract once: body selection and annotation attachment both read it.
    let model = crate::native::model::NativeModel::extract(
        ctx,
        &scan.container,
        &scan.streams,
        &mut parsed,
        terminal_lineage,
    )?;
    let mut active_body_selection = if let Some((selected, _, source)) = &preselection {
        let selected_hits = selected_hit_count(ctx, selected, &body_node_ids)?;
        apply_preselected_active_body_selection(
            ctx,
            &mut ir,
            selected,
            source,
            Some(selected_hits),
        )?
    } else {
        select_active_body(ctx, &mut ir, &body_node_ids, rmfastload_ids)?
    };
    if !active_body_selection {
        active_body_selection = select_terminal_feature_bodies(ctx, &mut ir, &model)?;
    }
    classify_body_kinds(ctx, &mut ir)?;
    match crate::native::attach_annotations(
        ctx,
        &mut ir,
        &model,
        scan,
        &mut annotations,
        &mut unknowns,
        &mut native_losses,
    ) {
        Ok(()) => {}
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    }
    for &(si, unknown_index) in ctx.admit_iter(&stream_unknowns, "nx retained unknown streams")? {
        retain_unknown_stream_data(ctx, &scan.streams[si], &mut unknowns[unknown_index])?;
    }
    prune_unreferenced_unknown_carriers(ctx, &mut ir)?;
    finalize_point_topology(ctx, &mut ir, &mut annotations)?;
    retain_referenced_pcurves(ctx, &mut ir)?;
    retain_live_unknown_links(ctx, &ir, &mut unknowns, &mut annotations)?;
    let mut annotations = annotations.build();
    retain_live_annotations(ctx, &ir, &unknowns, &mut annotations)?;
    let completion_budget = CompletionBudgetStatus {
        pcurves: crate::decode::report::PcurveCompletionStatus {
            exact_boundary_exhausted: transfer_budget_exhausted(&exact_transfer_budget),
            transfer_exhausted: transfer_budget_exhausted(&transfer_budget),
            geometry_exhausted: completion_geometry_budget.exhausted(),
        },
        serialized: crate::decode::report::SupportUvPhaseStatus {
            samples_exhausted: support_uv_budget_exhausted(&support_uv_validation_budget),
            geometry_exhausted: serialized_support_uv_geometry_budget.exhausted(),
        },
        direct: crate::decode::report::SupportUvPhaseStatus {
            samples_exhausted: support_uv_budget_exhausted(&support_budget),
            geometry_exhausted: support_uv_geometry_budget.exhausted(),
        },
        coupled: crate::decode::report::SupportUvPhaseStatus {
            samples_exhausted: support_uv_budget_exhausted(&coupled_support_budget),
            geometry_exhausted: coupled_support_uv_geometry_budget.exhausted(),
        },
        support_uv_lane_geometry_exhausted,
        transfer_limit,
        support_uv_limit,
    };
    let adaptive_geometry_exhausted = adaptive_geometry_budget.exhausted();
    ctx.charge_work(0, "nx geometry work completion")?;
    let mut report = build_geometry_report(
        ctx,
        scan,
        &crate::decode::report::GeometryReportFacts {
            unmatched_delta_tombstone_counts: parsed.unmatched_tombstone_counts(),
            counts: &counts,
            has_topology: !ir.model.faces.is_empty(),
            has_unresolved_sub_bodies: ir.model.bodies.len() > 1 && !active_body_selection,
            tessellation_count: ir.model.tessellations.len(),
            completion_budget,
            adaptive_geometry_exhausted,
            dialect_losses,
            notes,
        },
        &ir,
        &model,
    )?;
    for losses in [carrier_refusals, topology_losses, native_losses] {
        ctx.extend_vec(&mut report.losses, losses, "nx geometry report losses")?;
    }
    report_untransferred_streams(
        ctx,
        scan,
        &mut report,
        crate::native::TypedNative::Available,
    )?;
    Ok(Some((ir, report, annotations, unknowns)))
}

/// Copy one solved support-UV lane into new storage.
fn copy_support_uv_lane(
    ctx: &DecodeContext<'_>,
    lane: Option<&crate::intersection::SupportUvLane>,
) -> Result<Option<crate::intersection::SupportUvLane>, CodecError> {
    lane.map(|lane| {
        crate::intersection::SupportUvLane::from_checked(
            ctx.copy_slice(lane.as_slice(), "NX solved support-UV lane copy")?,
            lane.as_slice().len(),
        )
        .ok_or_else(|| CodecError::malformed("NX copied support-UV lane count"))
    })
    .transpose()
}

/// The stream's carrier lookup tables that trimmed and surface curves extend.
struct CarrierMaps<'m> {
    surfaces: &'m BTreeMap<u32, SurfaceId>,
    curves: &'m mut BTreeMap<u32, CurveId>,
    pcurves: &'m mut BTreeMap<u32, PcurveId>,
    pcurve_supports: &'m mut BTreeMap<u32, SurfaceId>,
    trim_ranges: &'m mut BTreeMap<u32, [f64; 2]>,
}

/// Map trimmed and surface curves to the carriers they reuse, normalize the
/// parameters of each pcurve once against its support surface, and collect
/// the pcurves that cannot be normalized. A pass repeats until it maps no new
/// carrier; each pass admits both curve lists.
fn map_stream_carriers(
    ctx: &DecodeContext<'_>,
    storage: &mut ScopedReservation<'_>,
    ir: &mut CadIr,
    trimmed_curves: &[crate::topology::TrimmedCurve],
    surface_curves: &[crate::topology::SurfaceCurve],
    maps: CarrierMaps<'_>,
    invalid_pcurves: &mut BTreeSet<PcurveId>,
) -> Result<(), CodecError> {
    let CarrierMaps {
        surfaces: surfaces_by_xmt,
        curves: curves_by_xmt,
        pcurves: pcurves_by_xmt,
        pcurve_supports: pcurve_supports_by_xmt,
        trim_ranges,
    } = maps;
    let mut normalized_pcurves: BTreeSet<PcurveId> = BTreeSet::new();
    let mut curve_indices: BTreeMap<&CurveId, usize> = BTreeMap::new();
    for (index, curve) in ctx
        .admit_iter(&ir.model.curves, "nx curve identity index")?
        .enumerate()
    {
        storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut curve_indices,
                &curve.id,
                index,
                "nx curve identity index",
            )
        })?;
    }
    let mut surface_indices: BTreeMap<&SurfaceId, usize> = BTreeMap::new();
    for (index, surface) in ctx
        .admit_iter(&ir.model.surfaces, "nx surface identity index")?
        .enumerate()
    {
        storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut surface_indices,
                &surface.id,
                index,
                "nx surface identity index",
            )
        })?;
    }
    let mut pcurve_indices: BTreeMap<PcurveId, usize> = BTreeMap::new();
    for (index, pcurve) in ctx
        .admit_iter(&ir.model.pcurves, "nx pcurve identity index")?
        .enumerate()
    {
        storage.with_storage(|| {
            let id = pcurve
                .id
                .try_clone_for_decode(ctx, "nx pcurve index identity")?;
            ctx.insert_btree_map(&mut pcurve_indices, id, index, "nx pcurve identity index")
        })?;
    }
    let curves = &ir.model.curves;
    let surfaces = &ir.model.surfaces;
    let pcurves = &mut ir.model.pcurves;
    loop {
        ctx.charge_work(1, "nx geometry carrier mapping pass")?;
        let mapped = curves_by_xmt.len() + pcurves_by_xmt.len() + pcurve_supports_by_xmt.len();
        for trim in ctx.admit_iter(trimmed_curves, "nx trimmed curve mapping")? {
            if let Some(basis) =
                ctx.get_btree_map(curves_by_xmt, &trim.state.basis(), "nx curve node index")?
            {
                let parameters =
                    match ctx.get_btree_map(&curve_indices, basis, "nx curve identity index")? {
                        Some(&index) => curves.get(index).and_then(|curve| {
                            canonical_trim_range(&curve.geometry, trim.state.parameters())
                        }),
                        None => None,
                    };
                let basis = storage.with_storage(|| {
                    basis.try_clone_for_decode(ctx, "nx trimmed curve identity")
                })?;
                storage.with_storage(|| {
                    ctx.insert_btree_map(curves_by_xmt, trim.xmt, basis, "nx trimmed curve index")
                })?;
                if let Some(parameters) = parameters {
                    storage.with_storage(|| {
                        ctx.insert_btree_map(
                            trim_ranges,
                            trim.xmt,
                            parameters,
                            "nx curve trim ranges",
                        )
                    })?;
                }
            }
            if let Some(pcurve) =
                ctx.get_btree_map(pcurves_by_xmt, &trim.state.basis(), "nx pcurve node index")?
            {
                let pcurve = storage.with_storage(|| {
                    pcurve.try_clone_for_decode(ctx, "nx trimmed pcurve identity")
                })?;
                storage.with_storage(|| {
                    ctx.insert_btree_map(
                        pcurves_by_xmt,
                        trim.xmt,
                        pcurve,
                        "nx trimmed pcurve index",
                    )
                })?;
                if let Some(support) = ctx.get_btree_map(
                    pcurve_supports_by_xmt,
                    &trim.state.basis(),
                    "nx pcurve supports",
                )? {
                    let support = storage.with_storage(|| {
                        support.try_clone_for_decode(ctx, "nx trimmed pcurve support")
                    })?;
                    storage.with_storage(|| {
                        ctx.insert_btree_map(
                            pcurve_supports_by_xmt,
                            trim.xmt,
                            support,
                            "nx trimmed pcurve supports",
                        )
                    })?;
                }
                storage.with_storage(|| {
                    ctx.insert_btree_map(
                        trim_ranges,
                        trim.xmt,
                        trim.state.parameters(),
                        "nx curve trim ranges",
                    )
                })?;
            }
        }
        for surface_curve in ctx.admit_iter(surface_curves, "nx surface curve mapping")? {
            if let Some(pcurve_ref) = ctx.get_btree_map(
                pcurves_by_xmt,
                &surface_curve.state.pcurve(),
                "nx pcurve node index",
            )? {
                let pcurve: PcurveId = storage.with_storage(|| {
                    pcurve_ref.try_clone_for_decode(ctx, "nx surface pcurve identity")
                })?;
                let pcurve_index =
                    ctx.get_btree_map(&pcurve_indices, &pcurve, "nx pcurve identity index")?;
                if !ctx.contains_btree_set(&normalized_pcurves, &pcurve, "nx normalized pcurves")? {
                    let support = match ctx.get_btree_map(
                        surfaces_by_xmt,
                        &surface_curve.state.surface(),
                        "nx surface node index",
                    )? {
                        Some(id) => ctx
                            .get_btree_map(&surface_indices, id, "nx surface identity index")?
                            .and_then(|index| surfaces.get(*index))
                            .map(|surface| &surface.geometry),
                        None => None,
                    };
                    let carrier = pcurve_index.and_then(|index| pcurves.get_mut(*index));
                    let normalized = if let (Some(support), Some(carrier)) = (support, carrier) {
                        normalize_pcurve_parameters(ctx, &mut carrier.geometry, support)?.is_some()
                    } else {
                        false
                    };
                    if !normalized {
                        ctx.remove_btree_map(
                            pcurves_by_xmt,
                            &surface_curve.state.pcurve(),
                            "nx pcurve node index",
                        )?;
                        storage.with_storage(|| {
                            ctx.insert_btree_set(invalid_pcurves, pcurve, "nx invalid pcurves")
                        })?;
                        continue;
                    }
                    storage.with_storage(|| {
                        let pcurve = pcurve.try_clone_for_decode(ctx, "nx normalized pcurves")?;
                        ctx.insert_btree_set(
                            &mut normalized_pcurves,
                            pcurve,
                            "nx normalized pcurves",
                        )
                    })?;
                }
                if let Some(carrier) = pcurve_index.and_then(|index| pcurves.get_mut(*index)) {
                    let fit_tolerance = decoded_tolerance(surface_curve.state.tolerance().get())
                        .map(|tolerance| {
                            cadmpeg_ir::geometry::FitTolerance::from(
                                cadmpeg_ir::scalar::NonNegativeReal::from(tolerance),
                            )
                        });
                    match &mut carrier.metadata {
                        cadmpeg_ir::geometry::pcurve::PcurveMetadata::General {
                            form: metadata,
                        } => metadata.set_fit_tolerance(fit_tolerance),
                        cadmpeg_ir::geometry::pcurve::PcurveMetadata::AsmInline {
                            form: inline,
                        } => {
                            if let Some(fit_tolerance) = fit_tolerance {
                                inline.set_fit_tolerance(fit_tolerance);
                            }
                        }
                    }
                }
                storage.with_storage(|| {
                    ctx.insert_btree_map(
                        pcurves_by_xmt,
                        surface_curve.xmt,
                        pcurve,
                        "nx surface pcurve index",
                    )
                })?;
                if let Some(support) = ctx.get_btree_map(
                    surfaces_by_xmt,
                    &surface_curve.state.surface(),
                    "nx surface node index",
                )? {
                    storage.with_storage(|| {
                        let support =
                            support.try_clone_for_decode(ctx, "nx surface pcurve support")?;
                        ctx.insert_btree_map(
                            pcurve_supports_by_xmt,
                            surface_curve.xmt,
                            support,
                            "nx surface pcurve supports",
                        )
                    })?;
                }
            }
            if let Some(original) = surface_curve.state.original() {
                if let Some(original) =
                    ctx.get_btree_map(curves_by_xmt, &original, "nx curve node index")?
                {
                    let original = storage.with_storage(|| {
                        original.try_clone_for_decode(ctx, "nx surface curve identity")
                    })?;
                    storage.with_storage(|| {
                        ctx.insert_btree_map(
                            curves_by_xmt,
                            surface_curve.xmt,
                            original,
                            "nx surface curve index",
                        )
                    })?;
                }
            }
        }
        if curves_by_xmt.len() + pcurves_by_xmt.len() + pcurve_supports_by_xmt.len() == mapped {
            break;
        }
    }
    Ok(())
}

/// Drop pcurves that no coedge references.
fn retain_referenced_pcurves(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "nx referenced pcurves")?;
    let mut referenced_pcurves = BTreeSet::new();
    for coedge in ctx.admit_iter(&ir.model.coedges, "nx referenced pcurves")? {
        for pcurve in ctx.admit_iter(&coedge.pcurves, "nx referenced pcurves")? {
            storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut referenced_pcurves,
                    &pcurve.pcurve,
                    "nx referenced pcurves",
                )
            })?;
        }
    }
    let marks = keep_marks(
        ctx,
        &mut storage,
        &ir.model.pcurves,
        |pcurve| ctx.contains_btree_set(&referenced_pcurves, &pcurve.id, "nx referenced pcurves"),
        "nx referenced pcurves",
    )?;
    drop(referenced_pcurves);
    retain_marked(&mut ir.model.pcurves, marks);
    Ok(())
}

/// Count the topology node identities of the selected bodies.
fn selected_hit_count(
    ctx: &DecodeContext<'_>,
    selected: &BTreeSet<BodyId>,
    body_node_ids: &BTreeMap<BodyId, BTreeSet<u32>>,
) -> Result<usize, CodecError> {
    let mut hits = 0usize;
    for body in ctx.admit_iter(selected, "nx selected body hits")? {
        if let Some(nodes) = ctx.get_btree_map(body_node_ids, body, "nx selected body hits")? {
            hits = hits
                .checked_add(nodes.len())
                .ok_or_else(|| ctx.refuse_codec_limit("nx selected body hits", 0, 1))?;
        }
    }
    Ok(hits)
}

fn extend_endpoint_witnesses(
    ctx: &DecodeContext<'_>,
    target: &mut EndpointWitnesses,
    source: EndpointWitnesses,
) -> Result<(), CodecError> {
    let mut source = source.into_iter();
    for _ in ctx.admit_iter(&(0..source.len()), "nx endpoint witness merge")? {
        let Some((key, witnesses)) = source.next() else {
            break;
        };
        match ctx.get_mut_btree_map(target, &key, "nx endpoint witness index")? {
            Some(target_witnesses) => {
                ctx.extend_vec(target_witnesses, witnesses, "nx endpoint witness merge")?;
            }
            None => {
                ctx.insert_btree_map(target, key, witnesses, "nx endpoint witness index")?;
            }
        }
    }
    Ok(())
}

/// Add every surface and curve reachable from the used carriers through
/// procedural constructions: offset and blend supports, blend spines, and
/// intersection and surface-curve sides. A pass repeats until it adds no
/// carrier; each pass admits both construction arenas.
fn reach_procedural_carriers<'m>(
    ctx: &DecodeContext<'_>,
    storage: &mut ScopedReservation<'_>,
    carriers: (&[Surface], &[Curve]),
    constructions: (&'m [ProceduralSurface], &'m [ProceduralCurve]),
    used_surfaces: &mut BTreeSet<&'m SurfaceId>,
    used_curves: &mut BTreeSet<&'m CurveId>,
    operation: &'static str,
) -> Result<(), CodecError> {
    let (surfaces, curves) = carriers;
    let (procedural_surfaces, procedural_curves) = constructions;
    // A construction carried by more than one carrier has no owner.
    let (surface_owners, _surface_owner_storage) = ctx.unique_index(
        ctx.admit_iter(surfaces, "nx procedural surface owners")?
            .filter_map(|surface| Some((surface.geometry.procedural_construction()?, &surface.id))),
        "nx procedural surface owners",
    )?;
    let (curve_owners, _curve_owner_storage) = procedural_curve_owners(ctx, curves)?;
    loop {
        ctx.charge_work(1, operation)?;
        let previous = (used_surfaces.len(), used_curves.len());
        for procedural in ctx.admit_iter(procedural_surfaces, operation)? {
            let Some(Some(owner)) = ctx.get_hash_map(
                &surface_owners,
                &procedural.id,
                "nx procedural surface owners",
            )?
            else {
                continue;
            };
            if !ctx.contains_btree_set(used_surfaces, *owner, operation)? {
                continue;
            }
            match procedural.definition() {
                ProceduralSurfaceDefinition::Offset(definition_payload) => {
                    let support = definition_payload.support();
                    storage
                        .with_storage(|| ctx.insert_btree_set(used_surfaces, support, operation))?;
                }
                ProceduralSurfaceDefinition::Blend(definition_payload) => {
                    for support in definition_payload.supports().iter().flatten() {
                        storage.with_storage(|| {
                            ctx.insert_btree_set(used_surfaces, &support.surface, operation)
                        })?;
                    }
                    if let Some(spine) = definition_payload.spine() {
                        storage
                            .with_storage(|| ctx.insert_btree_set(used_curves, spine, operation))?;
                    }
                }
                _ => {}
            }
        }
        for procedural in ctx.admit_iter(procedural_curves, operation)? {
            let Some(Some(owner)) =
                ctx.get_hash_map(&curve_owners, &procedural.id, "nx procedural curve owners")?
            else {
                continue;
            };
            if !ctx.contains_btree_set(used_curves, *owner, operation)? {
                continue;
            }
            let sides = match procedural.definition() {
                ProceduralCurveDefinition::Intersection { context, .. } => context.sides(),
                ProceduralCurveDefinition::SurfaceCurve { family } => family.context().sides(),
                _ => continue,
            };
            for side in sides {
                if let Some(surface) = &side.surface {
                    storage
                        .with_storage(|| ctx.insert_btree_set(used_surfaces, surface, operation))?;
                }
            }
        }
        if previous == (used_surfaces.len(), used_curves.len()) {
            break;
        }
    }
    Ok(())
}

fn prune_unreferenced_unknown_carriers(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<(), CodecError> {
    const SURFACES: &str = "nx used surfaces";
    const CURVES: &str = "nx used curves";
    let mut storage = ctx.reserve_scoped(0, "nx used carriers")?;
    let mut used_surfaces = BTreeSet::new();
    for face in ctx.admit_iter(&ir.model.faces, SURFACES)? {
        storage
            .with_storage(|| ctx.insert_btree_set(&mut used_surfaces, &face.surface, SURFACES))?;
    }
    let mut used_curves = BTreeSet::new();
    for edge in ctx.admit_iter(&ir.model.edges, CURVES)? {
        if let Some(curve) = edge.curve() {
            storage.with_storage(|| ctx.insert_btree_set(&mut used_curves, curve, CURVES))?;
        }
    }
    reach_procedural_carriers(
        ctx,
        &mut storage,
        (&ir.model.surfaces, &ir.model.curves),
        (&ir.model.procedural_surfaces, &ir.model.procedural_curves),
        &mut used_surfaces,
        &mut used_curves,
        "nx unknown carrier reachability pass",
    )?;
    let surface_marks = keep_marks(
        ctx,
        &mut storage,
        &ir.model.surfaces,
        |surface| {
            Ok(!matches!(
                surface.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
            ) || ctx.contains_btree_set(&used_surfaces, &surface.id, SURFACES)?)
        },
        SURFACES,
    )?;
    let curve_marks = keep_marks(
        ctx,
        &mut storage,
        &ir.model.curves,
        |curve| {
            Ok(!matches!(
                curve.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
            ) || ctx.contains_btree_set(&used_curves, &curve.id, CURVES)?)
        },
        CURVES,
    )?;
    drop((used_surfaces, used_curves));
    retain_marked(&mut ir.model.surfaces, surface_marks);
    retain_marked(&mut ir.model.curves, curve_marks);
    Ok(())
}

fn retain_live_annotations(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    unknowns: &[UnknownRecord],
    annotations: &mut cadmpeg_ir::Annotations,
) -> Result<(), CodecError> {
    const IDS: &str = "nx live annotation identities";
    let mut storage = ctx.reserve_scoped(0, IDS)?;
    let mut ids = BTreeSet::new();
    macro_rules! add_ids {
        ($($arena:expr),+ $(,)?) => {
            $(for entity in ctx.admit_iter(&$arena, IDS)? {
                storage.with_storage(|| ctx.insert_btree_set(&mut ids, entity.id.as_str(), IDS))?;
            })+
        };
    }
    add_ids!(
        ir.model.bodies,
        ir.model.regions,
        ir.model.shells,
        ir.model.faces,
        ir.model.loops,
        ir.model.coedges,
        ir.model.edges,
        ir.model.vertices,
        ir.model.points,
        ir.model.surfaces,
        ir.model.curves,
        ir.model.pcurves,
        ir.model.procedural_surfaces,
        ir.model.procedural_curves,
        ir.model.features,
    );
    for unknown in ctx.admit_iter(unknowns, IDS)? {
        storage.with_storage(|| ctx.insert_btree_set(&mut ids, unknown.id().as_str(), IDS))?;
    }
    let mut keep = |id: &str| ctx.contains_btree_set(&ids, id, "nx annotation identity lookup");
    annotations.retain_provenance(ctx, &mut keep)?;
    let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
    builder.retain_exactness(ctx, keep)?;
    *annotations = builder.build();
    Ok(())
}

fn retain_live_unknown_links(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    unknowns: &mut [UnknownRecord],
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_core::CodecError> {
    const IDS: &str = "nx live annotation identities";
    let mut storage = ctx.reserve_scoped(0, IDS)?;
    let mut ids = BTreeSet::new();
    macro_rules! add_ids {
        ($($arena:expr),+ $(,)?) => {
            $(for entity in ctx.admit_iter(&$arena, IDS)? {
                storage.with_storage(|| ctx.insert_btree_set(&mut ids, entity.id.as_str(), IDS))?;
            })+
        };
    }
    add_ids!(
        ir.model.surfaces,
        ir.model.curves,
        ir.model.pcurves,
        ir.model.procedural_surfaces,
        ir.model.procedural_curves,
    );
    for index in ctx.admit_iter(&(0..unknowns.len()), "nx live unknown links")? {
        let unknown = &mut unknowns[index];
        ctx.retain_vec(
            unknown.links_mut(),
            |link| ctx.contains_btree_set(&ids, link.as_str(), "nx live unknown links"),
            "nx live unknown links",
        )?;
        if !unknown.links().is_empty() {
            annotations.derived(ctx, unknown.id().as_str(), "links")?;
        }
    }
    Ok(())
}

/// Topology parts that one body's shells reach in a stream graph.
#[derive(Default)]
struct BodyTopology {
    ids: BTreeSet<u32>,
    edges: BTreeSet<u32>,
    vertices: BTreeSet<u32>,
    edge_nodes: usize,
    vertex_nodes: usize,
    complete: bool,
}

/// Owners of one record identity, grouped once per topology level.
type NodeOwners = BTreeMap<u32, BTreeSet<u32>>;

pub(super) fn topology_body_node_ids(
    ctx: &DecodeContext<'_>,
    stream_index: usize,
    graph: &Graph,
) -> Result<BTreeMap<BodyId, BTreeSet<u32>>, CodecError> {
    const OWNERS: &str = "nx topology body owners";
    let scope = IdScope::stream_charged(ctx, stream_index)?;
    let mut storage = ctx.reserve_scoped(0, OWNERS)?;
    let mut bodies = BTreeMap::<u32, BodyTopology>::new();
    for shell in graph.body_shape_shells(ctx)? {
        if let Some(body_xmt) = shell
            .shell_fields()
            .and_then(|fields| fields.body.map(u32::from))
        {
            if !ctx.contains_key_btree_map(&bodies, &body_xmt, "nx topology body nodes")? {
                let body = BodyTopology {
                    complete: true,
                    ..BodyTopology::default()
                };
                storage.with_storage(|| {
                    ctx.insert_btree_map(&mut bodies, body_xmt, body, "nx topology body nodes")
                })?;
            }
        }
    }
    let mut shell_owners = NodeOwners::new();
    for shell in graph.of_kind(NodeKind::Shell) {
        ctx.charge_work(1, "nx topology body shells")?;
        let Some(body) = shell
            .shell_fields()
            .and_then(|fields| fields.body.map(u32::from))
        else {
            continue;
        };
        if ctx.contains_key_btree_map(&bodies, &body, "nx topology body shells")? {
            storage.with_storage(|| {
                ctx.insert_btree_group_set(
                    &mut shell_owners,
                    shell.xmt(),
                    body,
                    "nx topology body shells",
                    "nx topology body shells",
                )
            })?;
        }
    }
    let mut face_owners = NodeOwners::new();
    for face in graph.of_kind(NodeKind::Face) {
        ctx.charge_work(1, "nx topology body faces")?;
        let Some(shell) = face.face_fields().and_then(|fields| fields.shell) else {
            continue;
        };
        let Some(owners) =
            ctx.get_btree_map(&shell_owners, &u32::from(shell), "nx topology body faces")?
        else {
            continue;
        };
        for &body_xmt in ctx.admit_iter(owners, "nx topology body faces")? {
            let Some(body) = ctx.get_mut_btree_map(&mut bodies, &body_xmt, OWNERS)? else {
                continue;
            };
            match face.u32_at(4) {
                Some(id) if body.complete => {
                    ctx.insert_btree_set(&mut body.ids, id, "nx topology body identities")?;
                }
                Some(_) => {}
                None => body.complete = false,
            }
            storage.with_storage(|| {
                ctx.insert_btree_group_set(
                    &mut face_owners,
                    face.xmt(),
                    body_xmt,
                    "nx topology body face nodes",
                    "nx topology body face nodes",
                )
            })?;
        }
    }
    let mut loop_owners = NodeOwners::new();
    for loop_ in graph.of_kind(NodeKind::Loop) {
        ctx.charge_work(1, "nx topology body loops")?;
        let Some(face) = loop_.loop_fields().and_then(|fields| fields.face) else {
            continue;
        };
        let Some(owners) =
            ctx.get_btree_map(&face_owners, &u32::from(face), "nx topology body loops")?
        else {
            continue;
        };
        for &body_xmt in ctx.admit_iter(owners, "nx topology body loops")? {
            storage.with_storage(|| {
                ctx.insert_btree_group_set(
                    &mut loop_owners,
                    loop_.xmt(),
                    body_xmt,
                    "nx topology body loops",
                    "nx topology body loops",
                )
            })?;
        }
    }
    let mut edge_owners = NodeOwners::new();
    let mut vertex_owners = NodeOwners::new();
    for fin in graph.of_kind(NodeKind::Fin) {
        ctx.charge_work(1, "nx topology body fins")?;
        let Some(fields) = fin.fin_fields() else {
            continue;
        };
        let Some(loop_xmt) = fields.loop_xmt else {
            continue;
        };
        let Some(owners) =
            ctx.get_btree_map(&loop_owners, &u32::from(loop_xmt), "nx topology body fins")?
        else {
            continue;
        };
        for &body_xmt in ctx.admit_iter(owners, "nx topology body fins")? {
            let Some(body) = ctx.get_mut_btree_map(&mut bodies, &body_xmt, OWNERS)? else {
                continue;
            };
            let (Some(edge), Some(vertex)) = (fields.edge, fields.vertex) else {
                body.complete = false;
                continue;
            };
            if !body.complete {
                continue;
            }
            let (edge, vertex) = (u32::from(edge), u32::from(vertex));
            storage.with_storage(|| {
                ctx.insert_btree_set(&mut body.edges, edge, "nx topology body edge nodes")?;
                ctx.insert_btree_set(&mut body.vertices, vertex, "nx topology body vertex nodes")?;
                ctx.insert_btree_group_set(
                    &mut edge_owners,
                    edge,
                    body_xmt,
                    "nx topology body edge nodes",
                    "nx topology body edge nodes",
                )?;
                ctx.insert_btree_group_set(
                    &mut vertex_owners,
                    vertex,
                    body_xmt,
                    "nx topology body vertex nodes",
                    "nx topology body vertex nodes",
                )
            })?;
        }
    }
    for (kind, owners) in [
        (NodeKind::Edge, &edge_owners),
        (NodeKind::Vertex, &vertex_owners),
    ] {
        for node in graph.of_kind(kind) {
            ctx.charge_work(1, "nx topology body edge and vertex nodes")?;
            let Some(node_owners) = ctx.get_btree_map(
                owners,
                &node.xmt(),
                "nx topology body edge and vertex nodes",
            )?
            else {
                continue;
            };
            for &body_xmt in
                ctx.admit_iter(node_owners, "nx topology body edge and vertex nodes")?
            {
                let Some(body) = ctx.get_mut_btree_map(&mut bodies, &body_xmt, OWNERS)? else {
                    continue;
                };
                if kind == NodeKind::Edge {
                    body.edge_nodes += 1;
                } else {
                    body.vertex_nodes += 1;
                }
                match node.u32_at(4) {
                    Some(id) if body.complete => {
                        ctx.insert_btree_set(&mut body.ids, id, "nx topology body identities")?;
                    }
                    Some(_) => {}
                    None => body.complete = false,
                }
            }
        }
    }
    let mut topology_bodies = BTreeMap::new();
    let mut bodies = bodies.into_iter();
    for _ in ctx.admit_iter(&(0..bodies.len()), "nx topology body index")? {
        let Some((body_xmt, body)) = bodies.next() else {
            break;
        };
        if !body.complete
            || body.edge_nodes != body.edges.len()
            || body.vertex_nodes != body.vertices.len()
        {
            continue;
        }
        ctx.insert_btree_map(
            &mut topology_bodies,
            scope.id_charged::<BodyId>(ctx, &cadmpeg_ir::identity_component!("body"), body_xmt)?,
            body.ids,
            "nx topology body index",
        )?;
    }
    Ok(topology_bodies)
}

/// Return body images whose complete topology node sets are inside the active
/// `RMFastLoad` membership set. This is the same admission predicate used after
/// topology emission, applied to graph-only body identities before carrier
/// construction.
pub(super) fn rmfastload_selected_bodies(
    ctx: &DecodeContext<'_>,
    body_node_ids: &BTreeMap<BodyId, BTreeSet<u32>>,
    rmfastload_ids: &[u32],
) -> Result<BTreeSet<BodyId>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "nx rmfastload active node ids")?;
    let mut active = BTreeSet::new();
    for &id in ctx.admit_iter(rmfastload_ids, "nx rmfastload active node ids")? {
        storage.with_storage(|| {
            ctx.insert_btree_set(&mut active, id, "nx rmfastload active node ids")
        })?;
    }
    let mut selected = BTreeSet::new();
    for (body, ids) in ctx.admit_iter(body_node_ids, "nx rmfastload selected bodies")? {
        if !ids.is_empty() && ctx.is_subset_btree_set(ids, &active, "nx rmfastload membership")? {
            ctx.insert_btree_set(
                &mut selected,
                body.try_clone_for_decode(ctx, "nx rmfastload selected body identity")?,
                "nx rmfastload selected bodies",
            )?;
        }
    }
    Ok(selected)
}

pub(super) fn rmfastload_allows_terminal_lineage(
    body_count: usize,
    rmfastload_selected: &BTreeSet<BodyId>,
) -> bool {
    body_count > 1 && rmfastload_selected.is_empty()
}

/// Return the stream ordinals that can contain selected body images. A
/// malformed body identity disables preselection rather than guessing a
/// stream owner.
pub(super) fn rmfastload_stream_indices(
    ctx: &DecodeContext<'_>,
    selected: &BTreeSet<BodyId>,
) -> Result<Option<BTreeSet<usize>>, CodecError> {
    const STREAMS: &str = "nx rmfastload stream indices";
    let mut streams = BTreeSet::new();
    for body in ctx.admit_iter(selected, STREAMS)? {
        let Some(text) = ctx.strip_prefix(body.as_str(), "nx:s", STREAMS)? else {
            return Ok(None);
        };
        let Some((text, _)) = ctx.split_once(text, ":", STREAMS)? else {
            return Ok(None);
        };
        let Ok(index) = ctx.parse_text::<usize>(text, "nx rmfastload stream index")? else {
            return Ok(None);
        };
        ctx.insert_btree_set(&mut streams, index, STREAMS)?;
    }
    Ok(Some(streams))
}

fn apply_preselected_active_body_selection(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    selected: &BTreeSet<BodyId>,
    selector: &str,
    selected_hits: Option<usize>,
) -> Result<bool, CodecError> {
    if selected.is_empty() {
        return Ok(false);
    }
    {
        let mut storage = ctx.reserve_scoped(0, "nx emitted body selection")?;
        let mut emitted = BTreeSet::new();
        for body in ctx.admit_iter(&ir.model.bodies, "nx emitted body selection")? {
            storage.with_storage(|| {
                ctx.insert_btree_set(&mut emitted, &body.id, "nx emitted body selection")
            })?;
        }
        for body in selected {
            ctx.charge_work(1, "nx emitted body selection")?;
            if !ctx.contains_btree_set(&emitted, body, "nx emitted body selection")? {
                return Ok(false);
            }
        }
    }
    prune_inactive_topology(ctx, ir, selected)?;
    if let Some(source) = &mut ir.source {
        insert_body_selection_attribute(
            ctx,
            &mut source.attributes,
            cadmpeg_core::nonblank_literal!("active_body_selector"),
            selector,
        )?;
        let (hit_attribute, count_attribute) = match selector {
            "rmfastload_object_id_membership" => (
                Some(cadmpeg_core::nonblank_literal!("rmfastload_hits")),
                cadmpeg_core::nonblank_literal!("rmfastload_active_body_count"),
            ),
            "terminal_feature_body_lineage" => (
                None,
                cadmpeg_core::nonblank_literal!("feature_terminal_body_count"),
            ),
            _ => (None, cadmpeg_core::nonblank_literal!("active_body_count")),
        };
        if let (Some(attribute), Some(selected_hits)) = (hit_attribute, selected_hits) {
            insert_body_selection_attribute(ctx, &mut source.attributes, attribute, selected_hits)?;
        }
        insert_body_selection_attribute(
            ctx,
            &mut source.attributes,
            count_attribute,
            selected.len(),
        )?;
    }
    Ok(true)
}

fn insert_body_selection_attribute(
    ctx: &DecodeContext<'_>,
    attributes: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    key: cadmpeg_core::text::NonBlankString,
    value: impl std::fmt::Display,
) -> Result<(), CodecError> {
    let value = ctx.format_retained(format_args!("{value}"), "nx body selection attribute")?;
    ctx.insert_btree_map(attributes, key, value, "nx body selection attributes")?;
    Ok(())
}

pub(super) fn select_active_body(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    body_node_ids: &BTreeMap<BodyId, BTreeSet<u32>>,
    rmfastload_ids: &[u32],
) -> Result<bool, CodecError> {
    if rmfastload_ids.is_empty() || ir.model.bodies.len() <= 1 {
        return Ok(false);
    }
    let mut storage = ctx.reserve_scoped(0, "nx rmfastload selected bodies")?;
    let selected =
        storage.with_storage(|| rmfastload_selected_bodies(ctx, body_node_ids, rmfastload_ids))?;
    if selected.is_empty() {
        return Ok(false);
    }
    let selected_hits = selected_hit_count(ctx, &selected, body_node_ids)?;
    apply_preselected_active_body_selection(
        ctx,
        ir,
        &selected,
        "rmfastload_object_id_membership",
        Some(selected_hits),
    )
}

fn select_terminal_feature_bodies(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    model: &crate::native::model::NativeModel,
) -> Result<bool, CodecError> {
    if ir.model.bodies.len() <= 1 {
        return Ok(false);
    }
    let mut storage = ctx.reserve_scoped(0, "nx terminal body selection index")?;
    let mut emitted = BTreeSet::new();
    for body in ctx.admit_iter(&ir.model.bodies, "nx terminal body selection index")? {
        storage.with_storage(|| {
            let body = body
                .id
                .try_clone_for_decode(ctx, "nx terminal body selection identity")?;
            ctx.insert_btree_set(&mut emitted, body, "nx terminal body selection index")
        })?;
    }
    // A complete terminal mapping resolves composition even when every emitted
    // body is terminal. The absence of pruning is a valid result: it means the
    // retained body images are all final, not that lineage was unresolved.
    let Some(selected) = storage.with_storage(|| {
        crate::native::model::terminal_feature_body_ids(
            ctx,
            &emitted,
            &model.segments.body_bindings,
            &model.segments.body_lineage_statuses,
        )
    })?
    else {
        return Ok(false);
    };
    apply_preselected_active_body_selection(
        ctx,
        ir,
        &selected,
        "terminal_feature_body_lineage",
        None,
    )
}

fn prune_inactive_topology(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    selected: &BTreeSet<BodyId>,
) -> Result<(), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "nx active topology")?;
    let marks = keep_marks(
        ctx,
        &mut storage,
        &ir.model.bodies,
        |body| ctx.contains_btree_set(selected, &body.id, "nx active bodies"),
        "nx active bodies",
    )?;
    retain_marked(&mut ir.model.bodies, marks);
    let marks = keep_marks(
        ctx,
        &mut storage,
        &ir.model.regions,
        |region| ctx.contains_btree_set(selected, &region.body, "nx active regions"),
        "nx active regions",
    )?;
    retain_marked(&mut ir.model.regions, marks);
    let mut regions = BTreeSet::new();
    for region in ctx.admit_iter(&ir.model.regions, "nx active regions")? {
        storage
            .with_storage(|| ctx.insert_btree_set(&mut regions, &region.id, "nx active regions"))?;
    }
    let marks = keep_marks(
        ctx,
        &mut storage,
        &ir.model.shells,
        |shell| ctx.contains_btree_set(&regions, &shell.region, "nx active shells"),
        "nx active shells",
    )?;
    drop(regions);
    retain_marked(&mut ir.model.shells, marks);
    let mut shells = BTreeSet::new();
    for shell in ctx.admit_iter(&ir.model.shells, "nx active shells")? {
        storage
            .with_storage(|| ctx.insert_btree_set(&mut shells, &shell.id, "nx active shells"))?;
    }
    let marks = keep_marks(
        ctx,
        &mut storage,
        &ir.model.faces,
        |face| ctx.contains_btree_set(&shells, &face.shell, "nx active faces"),
        "nx active faces",
    )?;
    drop(shells);
    retain_marked(&mut ir.model.faces, marks);
    let mut faces = BTreeSet::new();
    for face in ctx.admit_iter(&ir.model.faces, "nx active faces")? {
        storage.with_storage(|| ctx.insert_btree_set(&mut faces, &face.id, "nx active faces"))?;
    }
    let marks = keep_marks(
        ctx,
        &mut storage,
        &ir.model.loops,
        |loop_| ctx.contains_btree_set(&faces, &loop_.face, "nx active loops"),
        "nx active loops",
    )?;
    drop(faces);
    retain_marked(&mut ir.model.loops, marks);
    let mut loops = BTreeSet::new();
    for loop_ in ctx.admit_iter(&ir.model.loops, "nx active loops")? {
        storage.with_storage(|| ctx.insert_btree_set(&mut loops, &loop_.id, "nx active loops"))?;
    }
    let marks = keep_marks(
        ctx,
        &mut storage,
        &ir.model.coedges,
        |coedge| ctx.contains_btree_set(&loops, &coedge.owner_loop, "nx active coedges"),
        "nx active coedges",
    )?;
    drop(loops);
    retain_marked(&mut ir.model.coedges, marks);
    let mut edges = BTreeSet::new();
    for coedge in ctx.admit_iter(&ir.model.coedges, "nx active edges")? {
        storage
            .with_storage(|| ctx.insert_btree_set(&mut edges, &coedge.edge, "nx active edges"))?;
    }
    for shell in ctx.admit_iter(&ir.model.shells, "nx active edges")? {
        for edge in ctx.admit_iter(shell.wire_edges(), "nx active edges")? {
            storage.with_storage(|| ctx.insert_btree_set(&mut edges, edge, "nx active edges"))?;
        }
    }
    let marks = keep_marks(
        ctx,
        &mut storage,
        &ir.model.edges,
        |edge| ctx.contains_btree_set(&edges, &edge.id, "nx active edges"),
        "nx active edges",
    )?;
    drop(edges);
    retain_marked(&mut ir.model.edges, marks);
    let mut vertices = BTreeSet::new();
    for edge in ctx.admit_iter(&ir.model.edges, "nx active vertices")? {
        storage.with_storage(|| {
            ctx.insert_btree_set(&mut vertices, &edge.start, "nx active vertices")?;
            ctx.insert_btree_set(&mut vertices, &edge.end, "nx active vertices")
        })?;
    }
    for shell in ctx.admit_iter(&ir.model.shells, "nx active vertices")? {
        for vertex in ctx.admit_iter(shell.free_vertices(), "nx active vertices")? {
            storage.with_storage(|| {
                ctx.insert_btree_set(&mut vertices, vertex, "nx active vertices")
            })?;
        }
    }
    let marks = keep_marks(
        ctx,
        &mut storage,
        &ir.model.vertices,
        |vertex| ctx.contains_btree_set(&vertices, &vertex.id, "nx active vertices"),
        "nx active vertices",
    )?;
    drop(vertices);
    retain_marked(&mut ir.model.vertices, marks);
    let mut points = BTreeSet::new();
    for vertex in ctx.admit_iter(&ir.model.vertices, "nx active points")? {
        storage.with_storage(|| {
            ctx.insert_btree_set(&mut points, &vertex.point, "nx active points")
        })?;
    }
    let marks = keep_marks(
        ctx,
        &mut storage,
        &ir.model.points,
        |point| ctx.contains_btree_set(&points, &point.id, "nx active points"),
        "nx active points",
    )?;
    drop(points);
    retain_marked(&mut ir.model.points, marks);
    drop(storage);
    prune_inactive_geometry(ctx, ir)?;
    Ok(())
}

fn prune_inactive_geometry(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "nx active geometry")?;
    let mut surfaces = BTreeSet::new();
    for face in ctx.admit_iter(&ir.model.faces, "nx active surfaces")? {
        storage.with_storage(|| {
            ctx.insert_btree_set(&mut surfaces, &face.surface, "nx active surfaces")
        })?;
    }
    let mut curves = BTreeSet::new();
    for edge in ctx.admit_iter(&ir.model.edges, "nx active curves")? {
        if let Some(curve) = edge.curve() {
            storage
                .with_storage(|| ctx.insert_btree_set(&mut curves, curve, "nx active curves"))?;
        }
    }
    let mut pcurves = BTreeSet::new();
    for coedge in ctx.admit_iter(&ir.model.coedges, "nx active pcurves")? {
        for pcurve in ctx.admit_iter(&coedge.pcurves, "nx active pcurves")? {
            storage.with_storage(|| {
                ctx.insert_btree_set(&mut pcurves, &pcurve.pcurve, "nx active pcurves")
            })?;
        }
    }
    reach_procedural_carriers(
        ctx,
        &mut storage,
        (&ir.model.surfaces, &ir.model.curves),
        (&ir.model.procedural_surfaces, &ir.model.procedural_curves),
        &mut surfaces,
        &mut curves,
        "nx active geometry reachability pass",
    )?;

    let mut surface_constructions = BTreeSet::new();
    for surface in ctx.admit_iter(&ir.model.surfaces, "nx active surface constructions")? {
        if ctx.contains_btree_set(&surfaces, &surface.id, "nx active surface constructions")? {
            if let Some(construction) = surface.geometry.procedural_construction() {
                storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut surface_constructions,
                        construction,
                        "nx active surface constructions",
                    )
                })?;
            }
        }
    }
    let mut curve_constructions = BTreeSet::new();
    for curve in ctx.admit_iter(&ir.model.curves, "nx active curve constructions")? {
        if ctx.contains_btree_set(&curves, &curve.id, "nx active curve constructions")? {
            if let Some(construction) = curve.geometry.procedural_construction() {
                storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut curve_constructions,
                        construction,
                        "nx active curve constructions",
                    )
                })?;
            }
        }
    }
    let procedural_surface_marks = keep_marks(
        ctx,
        &mut storage,
        &ir.model.procedural_surfaces,
        |procedural| {
            ctx.contains_btree_set(
                &surface_constructions,
                &procedural.id,
                "nx active procedural surfaces",
            )
        },
        "nx active procedural surfaces",
    )?;
    let procedural_curve_marks = keep_marks(
        ctx,
        &mut storage,
        &ir.model.procedural_curves,
        |procedural| {
            ctx.contains_btree_set(
                &curve_constructions,
                &procedural.id,
                "nx active procedural curves",
            )
        },
        "nx active procedural curves",
    )?;
    let surface_marks = keep_marks(
        ctx,
        &mut storage,
        &ir.model.surfaces,
        |surface| ctx.contains_btree_set(&surfaces, &surface.id, "nx active surfaces"),
        "nx active surfaces",
    )?;
    let curve_marks = keep_marks(
        ctx,
        &mut storage,
        &ir.model.curves,
        |curve| ctx.contains_btree_set(&curves, &curve.id, "nx active curves"),
        "nx active curves",
    )?;
    let pcurve_marks = keep_marks(
        ctx,
        &mut storage,
        &ir.model.pcurves,
        |pcurve| ctx.contains_btree_set(&pcurves, &pcurve.id, "nx active pcurves"),
        "nx active pcurves",
    )?;
    drop((
        surfaces,
        curves,
        pcurves,
        surface_constructions,
        curve_constructions,
    ));
    retain_marked(&mut ir.model.procedural_surfaces, procedural_surface_marks);
    retain_marked(&mut ir.model.procedural_curves, procedural_curve_marks);
    retain_marked(&mut ir.model.surfaces, surface_marks);
    retain_marked(&mut ir.model.curves, curve_marks);
    retain_marked(&mut ir.model.pcurves, pcurve_marks);
    Ok(())
}

fn finalize_point_topology(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    if !ir.model.bodies.is_empty() {
        let mut storage = ctx.reserve_scoped(0, "nx referenced points")?;
        let mut referenced_points = BTreeSet::new();
        for vertex in ctx.admit_iter(&ir.model.vertices, "nx referenced points")? {
            storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut referenced_points,
                    &vertex.point,
                    "nx referenced points",
                )
            })?;
        }
        let marks = keep_marks(
            ctx,
            &mut storage,
            &ir.model.points,
            |point| ctx.contains_btree_set(&referenced_points, &point.id, "nx referenced points"),
            "nx referenced points",
        )?;
        drop(referenced_points);
        retain_marked(&mut ir.model.points, marks);
        return Ok(());
    }

    if ir.model.points.is_empty() {
        return Ok(());
    }

    let derived = IdScope::derived();
    let body_id: BodyId =
        derived.id_charged(ctx, &cadmpeg_ir::identity_component!("point-body"), 0)?;
    let region_id: RegionId =
        derived.id_charged(ctx, &cadmpeg_ir::identity_component!("point-region"), 0)?;
    let shell_id: ShellId =
        derived.id_charged(ctx, &cadmpeg_ir::identity_component!("point-shell"), 0)?;
    let stream = StreamHandle::new(
        ctx,
        cadmpeg_ir::stream_name!("nx:container"),
        "allocate annotation stream handle",
    )?;
    for id in [body_id.as_str(), region_id.as_str(), shell_id.as_str()] {
        annotations.note(ctx, id, &stream, 0, Some("derived_point_topology"))?;
        annotations.exactness(ctx, id, Exactness::Inferred)?;
    }

    let point_count = ir.model.points.len();
    let mut free_vertices = ctx.vector_storage(point_count, "nx point topology free vertices")?;
    ctx.reserve_capacity(
        &mut ir.model.vertices,
        point_count,
        "nx point topology vertices",
    )?;
    for (index, point) in ctx
        .admit_iter(&ir.model.points, "nx point topology vertices")?
        .enumerate()
    {
        let vertex_id: VertexId =
            derived.id_charged(ctx, &cadmpeg_ir::identity_component!("point-vertex"), index)?;
        annotations.note(
            ctx,
            vertex_id.as_str(),
            &stream,
            0,
            Some("derived_point_topology"),
        )?;
        annotations.exactness(ctx, vertex_id.as_str(), Exactness::Inferred)?;
        let vertex = Vertex {
            id: vertex_id.try_clone_for_decode(ctx, "nx point vertex identity copy")?,
            point: point
                .id
                .try_clone_for_decode(ctx, "nx point reference identity")?,
            tolerance: None,
        };
        ctx.push_vec(&mut ir.model.vertices, vertex, "nx point topology vertices")?;
        ctx.push_vec(
            &mut free_vertices,
            vertex_id,
            "nx point topology free vertices",
        )?;
    }
    ctx.reserve_vec(&mut ir.model.shells, 1, "nx point topology shells")?;
    ir.model.shells.push(
        match Shell::new(
            shell_id.try_clone_for_decode(ctx, "nx point shell identity copy")?,
            region_id.try_clone_for_decode(ctx, "nx point shell region identity")?,
            Vec::new(),
            Vec::new(),
            free_vertices,
        ) {
            Ok(shell) => shell,
            Err(_) => {
                return Ok(());
            }
        },
    );
    ctx.reserve_vec(&mut ir.model.regions, 1, "nx point topology regions")?;
    let mut region_shells = Vec::new();
    ctx.reserve_vec(&mut region_shells, 1, "nx point region shells")?;
    region_shells.push(shell_id);
    ir.model.regions.push(Region {
        id: region_id.try_clone_for_decode(ctx, "nx point region identity copy")?,
        body: body_id.try_clone_for_decode(ctx, "nx point region body identity")?,
        shells: region_shells,
    });
    ctx.reserve_vec(&mut ir.model.bodies, 1, "nx point topology bodies")?;
    let mut body_regions = Vec::new();
    ctx.reserve_vec(&mut body_regions, 1, "nx point body regions")?;
    body_regions.push(region_id);
    ir.model.bodies.push(Body {
        id: body_id,
        kind: BodyKind::General,
        regions: body_regions,
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    Ok(())
}

fn classify_body_kinds(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
    const RELATIONS: &str = "nx body classification relations";
    let mut storage = ctx.reserve_scoped(0, RELATIONS)?;
    let mut region_bodies = BTreeMap::new();
    for region in ctx.admit_iter(&ir.model.regions, RELATIONS)? {
        storage.with_storage(|| {
            ctx.insert_btree_map(&mut region_bodies, &region.id, &region.body, RELATIONS)
        })?;
    }
    let mut shell_bodies = BTreeMap::new();
    for shell in ctx.admit_iter(&ir.model.shells, RELATIONS)? {
        if let Some(&body) = ctx.get_btree_map(&region_bodies, &shell.region, RELATIONS)? {
            storage.with_storage(|| {
                ctx.insert_btree_map(&mut shell_bodies, &shell.id, body, RELATIONS)
            })?;
        }
    }
    let mut face_bodies = BTreeMap::new();
    for face in ctx.admit_iter(&ir.model.faces, RELATIONS)? {
        if let Some(&body) = ctx.get_btree_map(&shell_bodies, &face.shell, RELATIONS)? {
            storage.with_storage(|| {
                ctx.insert_btree_map(&mut face_bodies, &face.id, body, RELATIONS)
            })?;
        }
    }
    let mut loop_bodies = BTreeMap::new();
    for loop_ in ctx.admit_iter(&ir.model.loops, RELATIONS)? {
        if let Some(&body) = ctx.get_btree_map(&face_bodies, &loop_.face, RELATIONS)? {
            storage.with_storage(|| {
                ctx.insert_btree_map(&mut loop_bodies, &loop_.id, body, RELATIONS)
            })?;
        }
    }
    let mut coedge_bodies = BTreeMap::new();
    for coedge in ctx.admit_iter(&ir.model.coedges, RELATIONS)? {
        if let Some(&body) = ctx.get_btree_map(&loop_bodies, &coedge.owner_loop, RELATIONS)? {
            storage.with_storage(|| {
                ctx.insert_btree_map(&mut coedge_bodies, &coedge.id, body, RELATIONS)
            })?;
        }
    }
    let mut edge_uses = BTreeMap::<&BodyId, BTreeMap<&EdgeId, usize>>::new();
    for coedge in ctx.admit_iter(&ir.model.coedges, "nx body classification edge uses")? {
        let Some(&body) = ctx.get_btree_map(&coedge_bodies, &coedge.id, RELATIONS)? else {
            continue;
        };
        if !ctx.contains_key_btree_map(&edge_uses, body, "nx body classification edge owners")? {
            storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut edge_uses,
                    body,
                    BTreeMap::new(),
                    "nx body classification edge owners",
                )
            })?;
        }
        let Some(edges) =
            ctx.get_mut_btree_map(&mut edge_uses, body, "nx body classification edge owners")?
        else {
            return Err(CodecError::malformed(
                "NX body classification owner missing",
            ));
        };
        match ctx.get_mut_btree_map(edges, &coedge.edge, "nx body classification edge uses")? {
            Some(count) => {
                *count = count.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit("nx body classification edge uses", 0, 1)
                })?;
            }
            None => {
                storage.with_storage(|| {
                    ctx.insert_btree_map(edges, &coedge.edge, 1, "nx body classification edge uses")
                })?;
            }
        }
    }
    let mut kinds = Vec::new();
    for body in ctx.admit_iter(&ir.model.bodies, "nx body classification kinds")? {
        let solid = match ctx.get_btree_map(&edge_uses, &body.id, "nx body classification kinds")? {
            Some(uses) if !uses.is_empty() => {
                let mut every_edge_twice = true;
                for &use_count in uses.values() {
                    ctx.charge_work(1, "nx body classification kinds")?;
                    if use_count != 2 {
                        every_edge_twice = false;
                        break;
                    }
                }
                every_edge_twice
            }
            _ => false,
        };
        ctx.push_scoped_vec(
            &mut storage,
            &mut kinds,
            if solid {
                BodyKind::Solid
            } else {
                BodyKind::Sheet
            },
            "nx body classification kinds",
        )?;
    }
    drop((
        region_bodies,
        shell_bodies,
        face_bodies,
        loop_bodies,
        coedge_bodies,
        edge_uses,
    ));
    let mut kinds = kinds.into_iter();
    for index in ctx.admit_iter(&(0..ir.model.bodies.len()), "nx body classification kinds")? {
        let (Some(body), Some(kind)) = (ir.model.bodies.get_mut(index), kinds.next()) else {
            break;
        };
        body.kind = kind;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
