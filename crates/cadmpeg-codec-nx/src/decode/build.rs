// SPDX-License-Identifier: Apache-2.0
//! Geometry decode, active-body selection, and inactive-topology prune.

use super::emit::{
    annotate_node, canonical_trim_range, curve_tag, decoded_tolerance, emit_topology,
    retain_unknown_stream_data, retain_unresolved_topology_carriers,
    source_meta, surface_tag, unknown_stream_metadata,
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
use crate::geometry;
use crate::loss::NxLossCode;
use crate::topology::{Graph, Node};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::StreamHandle;
use cadmpeg_ir::codec::DecodeBody;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::NurbsCurve, pcurve::Pcurve, BlendCrossSection, BlendRadiusLaw, BlendSupport, Curve,
    CurveGeometry, IntcurveSupportContext, ProceduralCurve, ProceduralCurveDefinition,
    ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry,
    Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{Body, BodyKind, Point, Region, Shell, Vertex};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn ordered_point_candidates<'a>(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    graph: &'a Graph,
) -> Result<Vec<(FinitePoint3, &'a Node)>, CodecError> {
    ordered_fixed_candidates(
        ctx,
        geometry::points(ctx, stream)?
            .into_iter()
            .map(|point| (point.pos, point.position)),
        graph,
        [NodeKind::Point],
        Node::point_position,
    )
}

pub(super) fn ordered_surface_candidates<'a>(
    ctx: &DecodeContext<'_>,
    stream: &[u8],
    graph: &'a Graph,
) -> Result<Vec<(SurfaceGeometry, &'a Node)>, CodecError> {
    ordered_fixed_candidates(
        ctx,
        geometry::surfaces(ctx, stream)?
            .into_iter()
            .map(|surface| (surface.pos, surface.geometry)),
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
    stream: &[u8],
    graph: &'a Graph,
) -> Result<Vec<(CurveGeometry, &'a Node)>, CodecError> {
    ordered_fixed_candidates(
        ctx,
        geometry::curves(ctx, stream)?
            .into_iter()
            .map(|curve| (curve.pos, curve.geometry)),
        graph,
        [NodeKind::Line, NodeKind::Circle, NodeKind::Ellipse],
        Node::curve_geometry,
    )
}

fn ordered_fixed_candidates<'a, T>(
    ctx: &DecodeContext<'_>,
    fallback: impl IntoIterator<Item = (usize, T)>,
    graph: &'a Graph,
    kinds: impl IntoIterator<Item = NodeKind>,
    graph_value: impl Fn(&Node) -> Option<T>,
) -> Result<Vec<(T, &'a Node)>, CodecError> {
    let mut candidates = BTreeMap::new();
    for (offset, value) in fallback {
        ctx.charge_work(1, "scan NX analytic candidates")?;
        let Some(node) = graph
            .at_pos(offset)
            .filter(|node| graph_value(node).is_some())
        else {
            continue;
        };
        if !candidates.contains_key(&offset) {
            ctx.charge_collection_items(1, "nx analytic candidate index")?;
        }
        candidates.insert(offset, (value, node));
    }
    for node in kinds.into_iter().flat_map(|kind| graph.of_kind(kind)) {
        ctx.charge_work(1, "scan NX analytic candidates")?;
        if let Some(value) = graph_value(node) {
            if !candidates.contains_key(&node.pos) {
                ctx.charge_collection_items(1, "nx analytic candidate index")?;
            }
            candidates.insert(node.pos, (value, node));
        }
    }
    let mut ordered = ctx.collection_vec(candidates.len(), "nx ordered analytic candidates")?;
    ordered.extend(candidates.into_values());
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

fn push_unknown_link(
    ctx: &DecodeContext<'_>,
    unknown: &mut UnknownRecord,
    id: &str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, "nx unknown entity links")?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(id.len()),
        "nx unknown entity link text",
    )?;
    let links = unknown.links_mut();
    links
        .try_reserve(1)
        .map_err(|_| ctx.refuse_codec_limit("nx unknown entity links", 0, 1))?;
    let mut link = String::new();
    link.try_reserve_exact(id.len())
        .map_err(|_| ctx.refuse_codec_limit("nx unknown entity link text", 0, 1))?;
    link.push_str(id);
    links.push(link);
    Ok(())
}

pub(super) fn try_decode_geometry(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
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
    for (si, stream) in scan.streams.iter().enumerate() {
        if stream.kind().is_parasolid() {
            for (body, nodes) in
                topology_body_node_ids(ctx, si, &parsed.stream(si).view_for_geometry().graph)?
            {
                ctx.charge_collection_items(1, "nx geometry body node index")?;
                body_node_ids.insert(body, nodes);
            }
        }
    }
    let rmfastload_selected = rmfastload_selected_bodies(ctx, &body_node_ids, rmfastload_ids)?;
    let allow_terminal_lineage =
        rmfastload_allows_terminal_lineage(body_node_ids.len(), &rmfastload_selected);
    let rmfastload_preselection = if body_node_ids.len() > 1
        && !rmfastload_selected.is_empty()
        && rmfastload_selected.len() < body_node_ids.len()
    {
        rmfastload_stream_indices(ctx, &rmfastload_selected)?.map(|streams| {
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
    let mut emitted_body_ids = BTreeSet::new();
    for body in body_node_ids.keys() {
        ctx.charge_collection_items(1, "nx emitted terminal body index")?;
        emitted_body_ids.insert(body.try_clone_for_decode(ctx, "nx emitted terminal body identity")?);
    }
    let terminal_preselection = match terminal_lineage.as_ref() {
        Some(lineage) => crate::native::model::terminal_feature_body_ids(
            ctx,
            &emitted_body_ids,
            &lineage.bindings,
            &lineage.statuses,
        )?
        .filter(|selected| selected.len() < body_node_ids.len())
        .map(|selected| {
            rmfastload_stream_indices(ctx, &selected).map(|streams| {
                streams.map(|streams| (selected, streams, "terminal_feature_body_lineage"))
            })
        })
        .transpose()?
        .flatten(),
        None => None,
    };
    let preselection = rmfastload_preselection.or(terminal_preselection);
    let chart_count = scan
        .streams
        .iter()
        .enumerate()
        .filter(|(si, stream)| {
            stream.kind().is_parasolid()
                && preselection
                    .as_ref()
                    .is_none_or(|(_, selected, _)| selected.contains(si))
        })
        .map(|(si, _)| {
            parsed
                .stream(si)
                .view_for_geometry()
                .intersections
                .curves
                .len()
        })
        .sum::<usize>();
    let transfer_limit = completion_transfer_budget_limit(chart_count);
    let support_uv_limit = support_uv_completion_budget_limit(chart_count);
    let exact_transfer_budget = ctx.work_budget(MAX_EXACT_BOUNDARY_TRANSFER_SAMPLES as u64);
    let transfer_budget = ctx.work_budget(transfer_limit as u64);
    let support_uv_validation_budget = ctx.work_budget(support_uv_limit as u64);
    let support_budget = ctx.work_budget(support_uv_limit as u64);
    let coupled_support_budget = ctx.work_budget(support_uv_limit as u64);
    let adaptive_geometry_budget =
        GeometryWorkBudget::from_context(ctx, MAX_ADAPTIVE_GEOMETRY_WORK as u64);
    let completion_geometry_budget =
        GeometryWorkBudget::from_context(ctx, MAX_PCURVE_COMPLETION_GEOMETRY_WORK as u64);
    let support_uv_geometry_budget =
        GeometryWorkBudget::from_context(ctx, MAX_SUPPORT_UV_COMPLETION_GEOMETRY_WORK as u64);
    let coupled_support_uv_geometry_budget =
        GeometryWorkBudget::from_context(ctx, MAX_COUPLED_SUPPORT_UV_GEOMETRY_WORK as u64);
    let serialized_support_uv_geometry_budget =
        GeometryWorkBudget::from_context(ctx, MAX_SERIALIZED_SUPPORT_UV_GEOMETRY_WORK as u64);
    let mut support_uv_lane_geometry_exhausted = false;
    let mut intersection_index = IntersectionIncidenceIndex::default();
    let mut model_endpoint_witnesses = EndpointWitnesses::new();
    let mut completion_streams = Vec::new();
    let container_stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));

    for (si, stream) in scan.streams.iter().enumerate() {
        if !stream.kind().is_parasolid() {
            continue;
        }
        let scope = IdScope::stream_charged(ctx, si)?;
        adaptive_geometry_budget.clear_blend_frame_cache();
        completion_geometry_budget.clear_blend_frame_cache();
        support_uv_geometry_budget.clear_blend_frame_cache();
        coupled_support_uv_geometry_budget.clear_blend_frame_cache();
        serialized_support_uv_geometry_budget.clear_blend_frame_cache();
        if preselection
            .as_ref()
            .is_some_and(|(_, selected, _)| !selected.contains(&si))
        {
            let unknown_index = unknowns.len();
            { ctx.reserve_vec(&mut unknowns, 1, "nx geometry unknown streams")?; ctx.reserve_vec(&mut stream_unknowns, 1, "nx geometry unknown indices") }?;
            let unknown = unknown_stream_metadata(ctx, si, stream)?;
            super::annotations::note(
                ctx,
                &mut annotations,
                unknown.id().as_str(),
                &container_stream,
                stream.file_offset as u64,
                stream.kind().label(),
            )?;
            super::annotations::exactness(
                ctx,
                &mut annotations,
                unknown.id().as_str(),
                Exactness::Derived,
            )?;
            unknowns.push(unknown);
            stream_unknowns.push((si, unknown_index));
            continue;
        }
        let crate::nurbs::Parsed {
            surfaces: nurbs_surfaces,
            curves: nurbs_curves,
            pcurves: nurbs_pcurves,
            refusals: nurbs_refusals,
        } = parsed.parse_nurbs(si);
        for refusal in nurbs_refusals {
            ctx.reserve_vec(&mut carrier_refusals, 1, "nx carrier refusal losses")?;
            super::charge_loss_code(ctx, NxLossCode::CarrierLanesUnpaired)?;
            carrier_refusals.push(NxLossCode::CarrierLanesUnpaired.note(ctx.format_retained(format_args!(
                    "parasolid#{si} {} at byte {} states no carrier: {}",
                    refusal.family, refusal.pos, refusal.error
                ), "nx carrier refusal loss text")?));
        }
        let view = parsed.stream(si).view_for_geometry();
        let semantic = parsed.semantic_bytes(si);
        let stream_name = ctx.format_retained(format_args!("nx:parasolid#{si}:{}", stream.kind().label()), "nx geometry stream name")?;
        ctx.charge_collection_items(1, "nx geometry stream handles")?;
        let source_stream = StreamHandle::new(
            cadmpeg_ir::StreamName::try_from(stream_name).map_err(CodecError::malformed)?,
        );
        ctx.reserve_vec(&mut completion_streams, 1, "nx completion streams")?;
        completion_streams.push((si, source_stream.clone()));
        let graph = &view.graph;
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
        for (pi, (position, node)) in ordered_point_candidates(ctx, semantic, graph)?
            .into_iter()
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
            super::annotations::derived(ctx, &mut annotations, pid.as_str(), "position")?;
            ctx.reserve_vec(&mut ir.model.points, 1, "nx geometry points")?;
            ir.model.points.push(Point::new(
                pid.try_clone_for_decode(ctx, "nx geometry point identity")?,
                position,
                None,
            ));
            ctx.reserve_vec(&mut ir.model.vertices, 1, "nx geometry point vertices")?;
            ir.model.vertices.push(Vertex {
                id: vid.try_clone_for_decode(ctx, "nx point vertex identity")?,
                point: pid.try_clone_for_decode(ctx, "nx vertex point identity")?,
                tolerance: None,
            });
            ctx.charge_collection_items(1, "nx point node index")?;
            points_by_xmt.insert(node.xmt, pid);
            counts.points += 1;
        }
        for (fi, (geometry, node)) in ordered_surface_candidates(ctx, semantic, graph)?
            .into_iter()
            .enumerate()
        {
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
            super::annotations::derived(ctx, &mut annotations, id.as_str(), "geometry")?;
            ctx.reserve_vec(&mut ir.model.surfaces, 1, "nx geometry surfaces")?;
            ir.model.surfaces.push(Surface {
                id: id.try_clone_for_decode(ctx, "nx geometry surface identity")?,
                geometry,
                source_object: None,
            });
            ctx.charge_collection_items(1, "nx surface node index")?;
            surfaces_by_xmt.insert(node.xmt, id);
        }
        for (fi, surf) in nurbs_surfaces.into_iter().enumerate() {
            counts.nurbs_surfaces += 1;
            let id: SurfaceId =
                scope.id_charged(ctx, &cadmpeg_ir::identity_component!("nurbs-surf"), fi)?;
            super::annotations::note(
                ctx,
                &mut annotations,
                id.as_str(),
                &source_stream,
                surf.pos as u64,
                "B_SPLINE_SURFACE",
            )?;
            super::annotations::derived(ctx, &mut annotations, id.as_str(), "geometry")?;
            ctx.reserve_vec(&mut ir.model.surfaces, 1, "nx geometry surfaces")?;
            ir.model.surfaces.push(Surface {
                id: id.try_clone_for_decode(ctx, "nx NURBS surface identity")?,
                geometry: surf.geometry,
                source_object: None,
            });
            if let Some(node) = graph.at_pos(surf.pos) {
                ctx.charge_collection_items(1, "nx surface node index")?;
                surfaces_by_xmt.insert(node.xmt, id);
            }
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
        for (oi, offset) in view.offset_surfaces.iter().copied().enumerate() {
            let Some(support_ref) = surfaces_by_xmt.get(&offset.state.support()) else {
                continue;
            };
            let support: SurfaceId = support_ref.try_clone_for_decode(ctx, "nx offset support")?;
            let procedural_id: ProceduralSurfaceId =
                scope.id_charged(ctx, &cadmpeg_ir::identity_component!("offset"), oi)?;
            let (surface_id, cache_fit_tolerance) = if let Some((surface, fit_tolerance)) =
                saved_offset_carriers.get(&offset.xmt)
            {
                (
                    surface.try_clone_for_decode(ctx, "nx saved offset surface")?,
                    Some(*fit_tolerance),
                )
            } else {
                let surface_id: SurfaceId =
                    scope.id_charged(ctx, &cadmpeg_ir::identity_component!("offset-surf"), oi)?;
                super::annotations::note(
                    ctx,
                    &mut annotations,
                    surface_id.as_str(),
                    &source_stream,
                    offset.pos as u64,
                    "OFFSET_SURF",
                )?;
                super::annotations::derived(
                    ctx,
                    &mut annotations,
                    surface_id.as_str(),
                    "geometry",
                )?;
                ctx.reserve_vec(&mut ir.model.surfaces, 1, "nx offset surfaces")?;
                ir.model.surfaces.push(Surface {
                    id: surface_id.try_clone_for_decode(ctx, "nx offset surface identity")?,
                    geometry: SurfaceGeometry::Procedural {
                        construction: procedural_id.try_clone_for_decode(ctx, "nx offset construction identity")?,
                        cache: None,
                    },
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Nx,
                        object_id: cadmpeg_core::text::NonBlankString::new(ctx.format_retained(format_args!("nx:s{si}:offset-surface-record#{}", offset.xmt), "nx offset source object identity")?)
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
                });
                (surface_id, None)
            };
            super::annotations::note(
                ctx,
                &mut annotations,
                procedural_id.as_str(),
                &source_stream,
                offset.pos as u64,
                "OFFSET_SURF",
            )?;
            super::annotations::derived(
                ctx,
                &mut annotations,
                procedural_id.as_str(),
                "definition",
            )?;
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

            ctx.reserve_vec(&mut ir.model.procedural_surfaces, 1, "nx offset constructions")?;
            let _attached = ir.model.add_procedural_surface(
                &surface_id.try_clone_for_decode(ctx, "nx offset construction owner")?,
                procedural,
            );

            ctx.charge_collection_items(1, "nx surface node index")?;
            surfaces_by_xmt.insert(offset.xmt, surface_id);
            counts.offset_surfaces += 1;
        }

        for (bi, blend) in view.blend_surfaces.iter().copied().enumerate() {
            let surface_id: SurfaceId =
                scope.id_charged(ctx, &cadmpeg_ir::identity_component!("blend-surf"), bi)?;
            let procedural_id: ProceduralSurfaceId =
                scope.id_charged(ctx, &cadmpeg_ir::identity_component!("blend"), bi)?;
            super::annotations::note(
                ctx,
                &mut annotations,
                surface_id.as_str(),
                &source_stream,
                blend.pos as u64,
                "BLEND_SURF",
            )?;
            super::annotations::derived(ctx, &mut annotations, surface_id.as_str(), "geometry")?;
            ctx.reserve_vec(&mut ir.model.surfaces, 1, "nx blend surfaces")?;
            ir.model.surfaces.push(Surface {
                id: surface_id.try_clone_for_decode(ctx, "nx blend surface identity")?,
                geometry: SurfaceGeometry::Procedural {
                    construction: procedural_id.try_clone_for_decode(ctx, "nx blend construction identity")?,
                    cache: None,
                },
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Nx,
                    object_id: cadmpeg_core::text::NonBlankString::new(ctx.format_retained(format_args!("nx:s{si}:blend-surface-record#{}", blend.xmt), "nx blend source object identity")?)
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            super::annotations::note(
                ctx,
                &mut annotations,
                procedural_id.as_str(),
                &source_stream,
                blend.pos as u64,
                "BLEND_SURF",
            )?;
            super::annotations::derived(
                ctx,
                &mut annotations,
                procedural_id.as_str(),
                "definition",
            )?;
            let procedural_index = ir.model.procedural_surfaces.len();
            ctx.reserve_vec(&mut ir.model.procedural_surfaces, 1, "nx blend constructions")?;
            let attached = ir.model.add_procedural_surface(
                &surface_id.try_clone_for_decode(ctx, "nx blend construction owner")?,
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
            );
            if attached.is_ok() {
                ctx.reserve_vec(&mut pending_blend_supports, 1, "nx pending blend supports")?;
                pending_blend_supports.push((
                    procedural_index,
                    blend.state.support_xmts(),
                    blend.state.offsets(),
                ));
                if blend.state.spine_xmt() > 1 {
                    ctx.reserve_vec(&mut pending_blend_spines, 1, "nx pending blend spines")?;
                    pending_blend_spines.push((procedural_index, blend.state.spine_xmt()));
                }
            }
            ctx.charge_collection_items(1, "nx surface node index")?;
            surfaces_by_xmt.insert(blend.xmt, surface_id);
            counts.blend_surfaces += 1;
        }
        for (procedural_index, support_xmts, offsets) in pending_blend_supports {
            let [first, second] = [0, 1].map(|side| {
                surfaces_by_xmt
                    .get(&support_xmts[side])
                    .map(|surface| {
                        surface.try_clone_for_decode(ctx, "nx blend support identity")
                    })
                    .transpose()
                    .map(|surface| {
                        surface.map(|surface| BlendSupport {
                            surface,
                            reversed: offsets[side].is_sign_negative(),
                        })
                    })
            });
            let supports = [first?, second?];
            let Some(procedural) = ir.model.procedural_surfaces.get_mut(procedural_index) else {
                continue;
            };
            procedural.edit_definition(|definition| {
                if let ProceduralSurfaceDefinition::Blend(definition_payload) = definition {
                    definition_payload.set_supports(supports);
                }
            });
        }

        for (ci, (geometry, node)) in ordered_curve_candidates(ctx, semantic, graph)?
            .into_iter()
            .enumerate()
        {
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
            super::annotations::derived(ctx, &mut annotations, id.as_str(), "geometry")?;
            ctx.reserve_vec(&mut ir.model.curves, 1, "nx geometry curves")?;
            ir.model.curves.push(Curve {
                id: id.try_clone_for_decode(ctx, "nx geometry curve identity")?,
                geometry,
                source_object: None,
            });
            ctx.charge_collection_items(1, "nx curve node index")?;
            curves_by_xmt.insert(node.xmt, id);
        }
        for (ci, crv) in nurbs_curves.into_iter().enumerate() {
            counts.nurbs_curves += 1;
            let id: CurveId =
                scope.id_charged(ctx, &cadmpeg_ir::identity_component!("nurbs-crv"), ci)?;
            super::annotations::note(
                ctx,
                &mut annotations,
                id.as_str(),
                &source_stream,
                crv.pos as u64,
                "B_SPLINE_CURVE",
            )?;
            super::annotations::derived(ctx, &mut annotations, id.as_str(), "geometry")?;
            ctx.reserve_vec(&mut ir.model.curves, 1, "nx NURBS curves")?;
            ir.model.curves.push(Curve {
                id: id.try_clone_for_decode(ctx, "nx NURBS curve identity")?,
                geometry: crv.geometry,
                source_object: None,
            });
            if let Some(node) = graph.at_pos(crv.pos) {
                ctx.charge_collection_items(1, "nx curve node index")?;
                curves_by_xmt.insert(node.xmt, id);
            }
        }

        for (pi, pcurve) in nurbs_pcurves.into_iter().enumerate() {
            let id: PcurveId =
                scope.id_charged(ctx, &cadmpeg_ir::identity_component!("pcurve"), pi)?;
            super::annotations::note(
                ctx,
                &mut annotations,
                id.as_str(),
                &source_stream,
                pcurve.pos as u64,
                "B_CURVE_2D",
            )?;
            super::annotations::derived(ctx, &mut annotations, id.as_str(), "geometry")?;
            ctx.reserve_vec(&mut ir.model.pcurves, 1, "nx NURBS pcurves")?;
            ir.model.pcurves.push(Pcurve {
                id: id.try_clone_for_decode(ctx, "nx NURBS pcurve identity")?,
                geometry: pcurve.geometry,
                metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::default(),
            });
            if let Some(node) = graph.at_pos(pcurve.pos) {
                ctx.charge_collection_items(1, "nx pcurve node index")?;
                pcurves_by_xmt.insert(node.xmt, id);
            }
        }
        let intersection_scan = view.intersections.try_clone_for_decode(ctx)?;
        counts
            .intersection_rejections
            .extend(intersection_scan.rejected);
        let intersection_constructions = intersection_scan.constructions;
        let mut charted_intersections = BTreeMap::new();
        for curve in intersection_scan.curves {
            ctx.charge_collection_items(1, "nx charted intersection index")?;
            charted_intersections.insert(curve.xmt, curve);
        }
        let mut uncharted_intersections = BTreeMap::new();
        for curve in intersection_scan.uncharted {
            ctx.charge_collection_items(1, "nx uncharted intersection index")?;
            uncharted_intersections.insert(curve.xmt, curve);
        }
        let intersection_support_uv = {
            let model_index =
                cadmpeg_ir::index::ModelIndex::try_new_model_only_for_decode(&ir, ctx)?;
            intersection_constructions
                .iter()
                .map(
                    |construction| -> Result<Option<_>, cadmpeg_core::CodecError> {
                        let Some(charted) = charted_intersections.get(&construction.xmt) else {
                            return Ok(None);
                        };
                        let mut support_uv = validate_serialized_support_uv_with_index(
                            ctx,
                            &model_index,
                            &surfaces_by_xmt,
                            [Some(charted.primary_support), charted.secondary_support],
                            &charted.samples.points_charged(ctx)?,
                            charted.fit_tolerance.get(),
                            &charted.support_uv,
                            &serialized_support_uv_geometry_budget,
                        )?;
                        if let Some(ext_support_uv) = assign_ext11_support_uv_with_index(
                            ctx,
                            &model_index,
                            &surfaces_by_xmt,
                            [Some(charted.primary_support), charted.secondary_support],
                            &charted.samples.points_charged(ctx)?,
                            charted.fit_tolerance.get(),
                            &charted.ext_support_uv,
                            &serialized_support_uv_geometry_budget,
                        )? {
                            for side in 0..2 {
                                if support_uv[side].is_none() {
                                    support_uv[side] = ext_support_uv[side]
                                        .as_ref()
                                        .map(|lane| lane.clone_charged(ctx))
                                        .transpose()?;
                                }
                            }
                        }
                        Ok(Some((construction.xmt, support_uv)))
                    },
                )
                .try_fold(BTreeMap::new(), |mut values, admitted| {
                    if let Some((xmt, support_uv)) = admitted? {
                        ctx.charge_collection_items(1, "nx intersection support UV index")?;
                        values.insert(xmt, support_uv);
                    }
                    Ok::<_, cadmpeg_core::CodecError>(values)
                })?
        };
        for (ci, construction) in intersection_constructions.into_iter().enumerate() {
            let curve_id: CurveId = scope.id_charged(
                ctx,
                &cadmpeg_ir::identity_component!("intersection-crv"),
                ci,
            )?;
            let procedural_id: ProceduralCurveId =
                scope.id_charged(ctx, &cadmpeg_ir::identity_component!("intersection"), ci)?;
            let unknown_id: UnknownId = IdScope::container().id_charged(
                ctx,
                &cadmpeg_ir::identity_component!("parasolid"),
                si,
            )?;
            let charted = charted_intersections.get(&construction.xmt);
            let uncharted = if let Some(uncharted) = uncharted_intersections.get(&construction.xmt)
            {
                let [first, second] = uncharted
                    .supports
                    .references()
                    .map(|xmt| surfaces_by_xmt.get(&u32::from(xmt)));
                if let [Some(first), Some(second)] = [first, second] {
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
            } else {
                None
            };
            if let Some(charted) = charted {
                if let Some(support_uv) = intersection_support_uv.get(&construction.xmt) {
                    for (side, lane) in support_uv.iter().enumerate() {
                        if lane.is_some() {
                            ctx.charge_collection_items(1, "nx validated support UV lanes")?;
                            validated_support_uv_lanes.insert((
                                procedural_id.try_clone_for_decode(ctx, "nx validated support UV construction")?,
                                side,
                            ));
                        }
                    }
                }
                let [value_first, value_second] = [0, 1].map(|side| {
                    charted.support_uv[side]
                        .as_ref()
                        .map(|lane| lane.clone_charged(ctx))
                        .transpose()
                });
                let [ext_first, ext_second] = [0, 1].map(|side| {
                    charted.ext_support_uv[side]
                        .as_ref()
                        .map(|lane| lane.clone_charged(ctx))
                        .transpose()
                });
                ctx.reserve_vec(&mut pending_ext11_support_uv, 1, "nx pending EXT11 support UV")?;
                pending_ext11_support_uv.push((
                    procedural_id.try_clone_for_decode(ctx, "nx pending EXT11 construction")?,
                    charted.samples.clone_charged(ctx)?,
                    charted.fit_tolerance.get(),
                    SerializedSupportUv {
                        values: [value_first?, value_second?],
                        ext11: [ext_first?, ext_second?],
                    },
                ));
            }
            super::annotations::note(
                ctx,
                &mut annotations,
                curve_id.as_str(),
                &source_stream,
                construction.pos as u64,
                "INTERSECTION",
            )?;
            if charted.is_some() || uncharted.is_some() {
                super::annotations::derived(ctx, &mut annotations, curve_id.as_str(), "geometry")?;
            } else {
                super::annotations::exactness(
                    ctx,
                    &mut annotations,
                    curve_id.as_str(),
                    Exactness::Unknown,
                )?;
            }
            ctx.reserve_vec(&mut ir.model.curves, 1, "nx intersection curves")?;
            ir.model.curves.push(Curve {
                id: curve_id.try_clone_for_decode(ctx, "nx intersection curve identity")?,
                geometry: if let Some(charted) = charted {
                    CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                        NurbsCurve::from_lanes(
                            1,
                            linear_knots(
                                &charted.samples.parameters_charged(ctx)?,
                                &adaptive_geometry_budget,
                            )?,
                            charted.samples.points_charged(ctx)?,
                            None,
                            false,
                        )
                        .map_err(|error| CodecError::Malformed(error.to_string()))?,
                    ))
                } else if uncharted.is_some() {
                    CurveGeometry::Procedural {
                        construction: procedural_id.try_clone_for_decode(ctx, "nx intersection construction identity")?,
                        cache: None,
                    }
                } else {
                    CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                        record: Some(unknown_id.try_clone_for_decode(ctx, "nx unknown intersection record")?),
                    })
                },
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Nx,
                    object_id: cadmpeg_core::text::NonBlankString::new(ctx.format_retained(format_args!("nx:s{si}:intersection-record#{}", construction.xmt), "nx intersection source object identity")?)
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            super::annotations::note(
                ctx,
                &mut annotations,
                procedural_id.as_str(),
                &source_stream,
                construction.pos as u64,
                "INTERSECTION",
            )?;
            if charted.is_some() || uncharted.is_some() {
                super::annotations::derived(
                    ctx,
                    &mut annotations,
                    procedural_id.as_str(),
                    "definition",
                )?;
            } else {
                super::annotations::exactness(
                    ctx,
                    &mut annotations,
                    procedural_id.as_str(),
                    Exactness::Unknown,
                )?;
            }
            let definition = if let Some(charted) = charted {
                let support_uv = if let Some(lanes) = intersection_support_uv.get(&construction.xmt)
                {
                    let [first, second] = [0, 1].map(|side| {
                        lanes[side]
                            .as_ref()
                            .map(|lane| lane.clone_charged(ctx))
                            .transpose()
                    });
                    [first?, second?]
                } else {
                    [None, None]
                };
                let parameters = charted.samples.parameters_charged(ctx)?;
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
                    record: Some(unknown_id),
                    cache: None,
                }
            };
            let mut definition = definition;
            if let Some(charted) = charted {
                definition
                    .set_legacy_cache(cadmpeg_ir::geometry::LegacyCache::new(
                        charted.fit_tolerance,
                    ))
                    .map_err(cadmpeg_core::CodecError::malformed)?;
            }
            let procedural = ProceduralCurve::new(procedural_id, definition);

            ctx.reserve_vec(&mut ir.model.procedural_curves, 1, "nx intersection constructions")?;
            let _attached = ir.model.add_procedural_curve(
                &curve_id.try_clone_for_decode(ctx, "nx intersection owner identity")?,
                procedural,
            );

            ctx.charge_collection_items(1, "nx curve node index")?;
            curves_by_xmt.insert(construction.xmt, curve_id);
            counts.intersection_curves += 1;
        }
        for (procedural_index, spine_xmt) in pending_blend_spines {
            let Some(spine_ref) = curves_by_xmt.get(&spine_xmt) else {
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
        let trimmed_curves = &view.trimmed_curves;
        let mut normalized_pcurves: BTreeSet<PcurveId> = BTreeSet::new();
        let mut invalid_pcurves: BTreeSet<PcurveId> = BTreeSet::new();
        let mut curve_indices: BTreeMap<CurveId, usize> = BTreeMap::new();
        for (index, curve) in ir.model.curves.iter().enumerate() {
            ctx.charge_collection_items(1, "nx curve identity index")?;
            curve_indices.insert(
                curve.id.try_clone_for_decode(ctx, "nx curve index identity")?,
                index,
            );
        }
        let mut surface_indices: BTreeMap<SurfaceId, usize> = BTreeMap::new();
        for (index, surface) in ir.model.surfaces.iter().enumerate() {
            ctx.charge_collection_items(1, "nx surface identity index")?;
            surface_indices.insert(
                surface.id.try_clone_for_decode(ctx, "nx surface index identity")?,
                index,
            );
        }
        let mut pcurve_indices: BTreeMap<PcurveId, usize> = BTreeMap::new();
        for (index, pcurve) in ir.model.pcurves.iter().enumerate() {
            ctx.charge_collection_items(1, "nx pcurve identity index")?;
            pcurve_indices.insert(
                pcurve.id.try_clone_for_decode(ctx, "nx pcurve index identity")?,
                index,
            );
        }
        let surface_curves = &view.surface_curves;
        loop {
            let mapped = curves_by_xmt.len() + pcurves_by_xmt.len() + pcurve_supports_by_xmt.len();
            for trim in trimmed_curves {
                if let Some(basis_ref) = curves_by_xmt.get(&trim.state.basis()) {
                    let basis =
                        basis_ref.try_clone_for_decode(ctx, "nx trimmed curve identity")?;
                    let parameters = curve_indices
                        .get(&basis)
                        .and_then(|index| ir.model.curves.get(*index))
                        .and_then(|curve| {
                            canonical_trim_range(&curve.geometry, trim.state.parameters())
                        });
                    ctx.charge_collection_items(1, "nx trimmed curve index")?;
                    curves_by_xmt.insert(trim.xmt, basis);
                    if let Some(parameters) = parameters {
                        ctx.charge_collection_items(1, "nx curve trim ranges")?;
                        trim_ranges.insert(trim.xmt, parameters);
                    }
                }
                if let Some(pcurve_ref) = pcurves_by_xmt.get(&trim.state.basis()) {
                    let pcurve =
                        pcurve_ref.try_clone_for_decode(ctx, "nx trimmed pcurve identity")?;
                    ctx.charge_collection_items(1, "nx trimmed pcurve index")?;
                    pcurves_by_xmt.insert(trim.xmt, pcurve);
                    if let Some(support) = pcurve_supports_by_xmt.get(&trim.state.basis()) {
                        ctx.charge_collection_items(1, "nx trimmed pcurve supports")?;
                        pcurve_supports_by_xmt.insert(
                            trim.xmt,
                            support.try_clone_for_decode(ctx, "nx trimmed pcurve support")?,
                        );
                    }
                    ctx.charge_collection_items(1, "nx curve trim ranges")?;
                    trim_ranges.insert(trim.xmt, trim.state.parameters());
                }
            }
            for surface_curve in surface_curves {
                if let Some(pcurve_ref) = pcurves_by_xmt.get(&surface_curve.state.pcurve()) {
                    let pcurve: PcurveId =
                        pcurve_ref.try_clone_for_decode(ctx, "nx surface pcurve identity")?;
                    if !normalized_pcurves.contains(&pcurve) {
                        let support = surfaces_by_xmt
                            .get(&surface_curve.state.surface())
                            .and_then(|id| surface_indices.get(id))
                            .and_then(|index| ir.model.surfaces.get(*index))
                            .map(|surface| &surface.geometry);
                        let normalized = if let (Some(support), Some(carrier)) = (
                            support,
                            pcurve_indices
                                .get(&pcurve)
                                .and_then(|index| ir.model.pcurves.get_mut(*index)),
                        ) {
                            normalize_pcurve_parameters(ctx, &mut carrier.geometry, support)?
                                .is_some()
                        } else {
                            false
                        };
                        if !normalized {
                            pcurves_by_xmt.remove(&surface_curve.state.pcurve());
                            insert_retained_id(
                                ctx,
                                &mut invalid_pcurves,
                                pcurve.as_str(),
                                "nx invalid pcurves",
                            )?;
                            continue;
                        }
                        insert_retained_id(
                            ctx,
                            &mut normalized_pcurves,
                            pcurve.as_str(),
                            "nx normalized pcurves",
                        )?;
                    }
                    if let Some(carrier) = pcurve_indices
                        .get(&pcurve)
                        .and_then(|index| ir.model.pcurves.get_mut(*index))
                    {
                        let fit_tolerance = decoded_tolerance(
                            surface_curve.state.tolerance().get(),
                        )
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
                    ctx.charge_collection_items(1, "nx surface pcurve index")?;
                    pcurves_by_xmt.insert(surface_curve.xmt, pcurve);
                    if let Some(support) = surfaces_by_xmt.get(&surface_curve.state.surface()) {
                        ctx.charge_collection_items(1, "nx surface pcurve supports")?;
                        pcurve_supports_by_xmt.insert(
                            surface_curve.xmt,
                            support.try_clone_for_decode(ctx, "nx surface pcurve support")?,
                        );
                    }
                }
                if let Some(original) = surface_curve
                    .state
                    .original()
                    .and_then(|original| curves_by_xmt.get(&original))
                {
                    ctx.charge_collection_items(1, "nx surface curve index")?;
                    curves_by_xmt.insert(
                        surface_curve.xmt,
                        original.try_clone_for_decode(ctx, "nx surface curve identity")?,
                    );
                }
            }
            if curves_by_xmt.len() + pcurves_by_xmt.len() + pcurve_supports_by_xmt.len() == mapped {
                break;
            }
        }
        if !invalid_pcurves.is_empty() {
            ir.model
                .pcurves
                .retain(|candidate| !invalid_pcurves.contains(&candidate.id));
            intersection_index.reindex_pcurves_after_prune(ctx, &ir)?;
        }
        retain_unresolved_topology_carriers(
            ctx,
            &mut ir,
            si,
            graph,
            &mut surfaces_by_xmt,
            &mut curves_by_xmt,
            &pcurves_by_xmt,
            &source_stream,
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
            si,
            graph,
            &points_by_xmt,
            &surfaces_by_xmt,
            &curves_by_xmt,
            &pcurves_by_xmt,
            &pcurve_supports_by_xmt,
            &trim_ranges,
            &source_stream,
            &mut annotations,
            &mut intersection_index,
            intersection_starts,
            procedural_start,
            &exact_transfer_budget,
            &transfer_budget,
            &adaptive_geometry_budget,
            &completion_geometry_budget,
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
        copy_endpoint_witnesses(
            ctx,
            &mut model_endpoint_witnesses,
            &validated_endpoint_witnesses,
        )?;
        attach_completed_intersection_pcurves_for_stream_with_budget(
            ctx,
            &mut ir,
            graph,
            &IdScope::stream_charged(ctx, si)?,
            intersection_starts.coedges,
            intersection_starts.procedural_curves,
            source_stream.clone(),
            &mut annotations,
            &validated_endpoint_witnesses,
            &completion_geometry_budget,
        )?;
        // Preserve the whole inflated stream verbatim so nothing is dropped.
        let unknown_index = unknowns.len();
        { ctx.reserve_vec(&mut unknowns, 1, "nx geometry unknown streams")?; ctx.reserve_vec(&mut stream_unknowns, 1, "nx geometry unknown indices") }?;
        let mut unknown = unknown_stream_metadata(ctx, si, stream)?;
        for surface in &ir.model.surfaces[first_surface..] {
            push_unknown_link(ctx, &mut unknown, surface.id.as_str())?;
        }
        for curve in &ir.model.curves[first_curve..] {
            push_unknown_link(ctx, &mut unknown, curve.id.as_str())?;
        }
        super::annotations::note(
            ctx,
            &mut annotations,
            unknown.id().as_str(),
            &container_stream,
            stream.file_offset as u64,
            stream.kind().label(),
        )?;
        super::annotations::exactness(
            ctx,
            &mut annotations,
            unknown.id().as_str(),
            Exactness::Derived,
        )?;
        unknowns.push(unknown);
        stream_unknowns.push((si, unknown_index));
    }

    intersection_index.complete_from_model(ctx, &mut ir)?;
    let mut completion_sources = ctx.collection_vec(completion_streams.len(), "nx completion sources")?;
    for (si, source_stream) in &completion_streams {
        completion_sources.push(IntersectionCompletionSource {
            scope: IdScope::stream_charged(ctx, *si)?,
            graph: parsed.stream(*si).view_for_geometry().graph.as_ref(),
            source_stream: source_stream.clone(),
            coedge_start: 0,
            procedural_start: 0,
        });
    }
    attach_completed_intersection_pcurves_for_model_with_budget(
        ctx,
        &mut ir,
        &completion_sources,
        &mut annotations,
        &model_endpoint_witnesses,
        &completion_geometry_budget,
    )?;

    if counts.points == 0 && counts.surfaces() == 0 && counts.curves() == 0 {
        return Ok(None);
    }

    ir.source = Some(source_meta(ctx, scan, dialects)?);

    ctx.admit_entities(
        ir.model.entity_count() as u64,
        admitted_entities,
        "admit NX entities",
    )?;

    // Extract once: body selection and annotation attachment both read it.
    let model = crate::native::model::NativeModel::extract(
        ctx,
        root,
        &scan.container,
        &scan.streams,
        &mut parsed,
        terminal_lineage,
    )?;
    let mut active_body_selection = if let Some((selected, _, source)) = &preselection {
        let selected_hits = selected
            .iter()
            .filter_map(|body| body_node_ids.get(body))
            .map(BTreeSet::len)
            .sum::<usize>();
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
    for (si, unknown_index) in stream_unknowns {
        retain_unknown_stream_data(ctx, &scan.streams[si], &mut unknowns[unknown_index])?;
    }
    prune_unreferenced_unknown_carriers(ctx, &mut ir)?;
    finalize_point_topology(ctx, &mut ir, &mut annotations)?;
    let mut referenced_pcurves: BTreeSet<PcurveId> = BTreeSet::new();
    for coedge in &ir.model.coedges {
        for pcurve in &coedge.pcurves {
            insert_retained_id(
                ctx,
                &mut referenced_pcurves,
                pcurve.pcurve.as_str(),
                "nx referenced pcurves",
            )?;
        }
    }
    ir.model
        .pcurves
        .retain(|pcurve| referenced_pcurves.contains(&pcurve.id));
    retain_live_unknown_links(ctx, &ir, &mut unknowns, &mut annotations)?;
    let mut annotations = annotations.build();
    retain_live_annotations(ctx, &ir, &unknowns, &mut annotations)?;
    let completion_budget = CompletionBudgetStatus {
        exact_boundary_exhausted: transfer_budget_exhausted(&exact_transfer_budget),
        transfer_exhausted: transfer_budget_exhausted(&transfer_budget),
        support_uv_validation_exhausted: support_uv_budget_exhausted(&support_uv_validation_budget),
        support_uv_exhausted: support_uv_budget_exhausted(&support_budget),
        coupled_support_uv_exhausted: support_uv_budget_exhausted(&coupled_support_budget),
        completion_geometry_exhausted: completion_geometry_budget.exhausted(),
        serialized_support_uv_geometry_exhausted: serialized_support_uv_geometry_budget.exhausted(),
        support_uv_geometry_exhausted: support_uv_geometry_budget.exhausted(),
        coupled_support_uv_geometry_exhausted: coupled_support_uv_geometry_budget.exhausted(),
        support_uv_lane_geometry_exhausted,
        transfer_limit,
        support_uv_limit,
    };
    let adaptive_geometry_exhausted = adaptive_geometry_budget.exhausted();
    ctx.charge_work(0, "nx geometry work completion")?;
    let mut report = build_geometry_report(
        ctx,
        scan,
        parsed.unmatched_tombstone_counts(),
        &ir,
        &counts,
        !ir.model.faces.is_empty(),
        ir.model.bodies.len() > 1 && !active_body_selection,
        ir.model.tessellations.len(),
        &model,
        completion_budget,
        adaptive_geometry_exhausted,
        dialect_losses,
        notes,
    )?;
    for losses in [carrier_refusals, topology_losses, native_losses] {
        ctx.reserve_vec(&mut report.losses, losses.len(), "nx geometry report losses")?;
        report.losses.extend(losses);
    }
    report_untransferred_streams(
        ctx,
        scan,
        &mut report,
        crate::native::TypedNative::Available,
    )?;
    Ok(Some((ir, report, annotations, unknowns)))
}

fn insert_retained_id<T>(
    ctx: &DecodeContext<'_>,
    ids: &mut BTreeSet<T>,
    identity: &str,
    operation: &'static str,
) -> Result<(), CodecError>
where
    T: TryFrom<String> + Ord,
    T::Error: std::fmt::Display,
{
    ctx.charge_collection_items(1, operation)?;
    ids.insert(T::try_from(ctx.copy_retained_text(identity, operation)?).map_err(CodecError::malformed)?);
    Ok(())
}

fn extend_endpoint_witnesses(
    ctx: &DecodeContext<'_>,
    target: &mut EndpointWitnesses,
    source: EndpointWitnesses,
) -> Result<(), CodecError> {
    for (key, witnesses) in source {
        let target_witnesses = match target.entry(key) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "nx endpoint witness index")?;
                entry.insert(Vec::new())
            }
        };
        let count = cadmpeg_core::decode::u64_from_index(witnesses.len());
        ctx.charge_collection_items(count, "nx endpoint witness merge")?;
        target_witnesses
            .try_reserve(witnesses.len())
            .map_err(|_| ctx.refuse_codec_limit("nx endpoint witness merge", 0, count))?;
        target_witnesses.extend(witnesses);
    }
    Ok(())
}

fn copy_endpoint_witnesses(
    ctx: &DecodeContext<'_>,
    target: &mut EndpointWitnesses,
    source: &EndpointWitnesses,
) -> Result<(), CodecError> {
    for ((curve, surface), witnesses) in source {
        let key = (
            curve.try_clone_for_decode(ctx, "nx endpoint witness curve")?,
            surface.try_clone_for_decode(ctx, "nx endpoint witness surface")?,
        );
        let target_witnesses = match target.entry(key) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "nx model endpoint witness index")?;
                entry.insert(Vec::new())
            }
        };
        let count = cadmpeg_core::decode::u64_from_index(witnesses.len());
        ctx.charge_collection_items(count, "nx model endpoint witnesses")?;
        target_witnesses
            .try_reserve(witnesses.len())
            .map_err(|_| ctx.refuse_codec_limit("nx model endpoint witnesses", 0, count))?;
        for (geometry, range, endpoints) in witnesses {
            target_witnesses.push((
                geometry.try_clone_for_decode(ctx, "nx endpoint witness geometry")?,
                *range,
                *endpoints,
            ));
        }
    }
    Ok(())
}

fn prune_unreferenced_unknown_carriers(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<(), CodecError> {
    let mut used_surfaces: BTreeSet<SurfaceId> = BTreeSet::new();
    for face in &ir.model.faces {
        insert_retained_id(
            ctx,
            &mut used_surfaces,
            face.surface.as_str(),
            "nx used surfaces",
        )?;
    }
    let mut used_curves: BTreeSet<CurveId> = BTreeSet::new();
    for edge in &ir.model.edges {
        if let Some(curve) = edge.curve() {
            insert_retained_id(ctx, &mut used_curves, curve.as_str(), "nx used curves")?;
        }
    }
    loop {
        let previous = (used_surfaces.len(), used_curves.len());
        for procedural in &ir.model.procedural_surfaces {
            let Some(owner) = ir.model.procedural_surface_owner(&procedural.id) else {
                continue;
            };
            if !used_surfaces.contains(owner) {
                continue;
            }
            match procedural.definition() {
                ProceduralSurfaceDefinition::Offset(definition_payload) => {
                    let support = definition_payload.support();
                    insert_retained_id(
                        ctx,
                        &mut used_surfaces,
                        support.as_str(),
                        "nx used surfaces",
                    )?;
                }
                ProceduralSurfaceDefinition::Blend(definition_payload) => {
                    let supports = definition_payload.supports();
                    let spine = definition_payload.spine();

                    for support in supports.iter().flatten() {
                        insert_retained_id(
                            ctx,
                            &mut used_surfaces,
                            support.surface.as_str(),
                            "nx used surfaces",
                        )?;
                    }
                    if let Some(spine) = spine {
                        insert_retained_id(
                            ctx,
                            &mut used_curves,
                            spine.as_str(),
                            "nx used curves",
                        )?;
                    }
                }
                _ => {}
            }
        }
        for procedural in &ir.model.procedural_curves {
            let Some(owner) = ir.model.procedural_curve_owner(&procedural.id) else {
                continue;
            };
            if !used_curves.contains(owner) {
                continue;
            }
            match procedural.definition() {
                ProceduralCurveDefinition::Intersection { context, .. } => {
                    for side in context.sides() {
                        if let Some(surface) = &side.surface {
                            insert_retained_id(
                                ctx,
                                &mut used_surfaces,
                                surface.as_str(),
                                "nx used surfaces",
                            )?;
                        }
                    }
                }
                ProceduralCurveDefinition::SurfaceCurve { family } => {
                    for side in family.context().sides() {
                        if let Some(surface) = &side.surface {
                            insert_retained_id(
                                ctx,
                                &mut used_surfaces,
                                surface.as_str(),
                                "nx used surfaces",
                            )?;
                        }
                    }
                }
                _ => {}
            }
        }
        if previous == (used_surfaces.len(), used_curves.len()) {
            break;
        }
    }
    ir.model.surfaces.retain(|surface| {
        !matches!(
            surface.geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
        ) || used_surfaces.contains(&surface.id)
    });
    ir.model.curves.retain(|curve| {
        !matches!(
            curve.geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
        ) || used_curves.contains(&curve.id)
    });
    Ok(())
}

fn retain_live_annotations(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    unknowns: &[UnknownRecord],
    annotations: &mut cadmpeg_ir::Annotations,
) -> Result<(), CodecError> {
    let mut ids = BTreeSet::new();
    macro_rules! add_ids {
        ($($arena:expr),+ $(,)?) => {
            $(for entity in &$arena {
                insert_live_identity(ctx, &mut ids, entity.id.as_str())?;
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
    for unknown in unknowns {
        insert_live_identity(ctx, &mut ids, unknown.id().as_str())?;
    }
    annotations.provenance.retain(|id, _| ids.contains(id));
    let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
    builder.retain_exactness(|id| ids.contains(id));
    *annotations = builder.build();
    Ok(())
}

fn insert_live_identity(
    ctx: &DecodeContext<'_>,
    ids: &mut BTreeSet<String>,
    identity: &str,
) -> Result<(), CodecError> {
    ctx.charge_collection_items(1, "nx live annotation identities")?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(identity.len()),
        "nx live annotation identity text",
    )?;
    let mut copy = String::new();
    copy.try_reserve_exact(identity.len()).map_err(|_| {
        ctx.refuse_codec_limit(
            "nx live annotation identity text",
            0,
            cadmpeg_core::decode::u64_from_index(identity.len()),
        )
    })?;
    copy.push_str(identity);
    ids.insert(copy);
    Ok(())
}

fn retain_live_unknown_links(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    unknowns: &mut [UnknownRecord],
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut ids = BTreeSet::new();
    for entity in &ir.model.surfaces {
        insert_live_identity(ctx, &mut ids, entity.id.as_str())?;
    }
    for entity in &ir.model.curves {
        insert_live_identity(ctx, &mut ids, entity.id.as_str())?;
    }
    for entity in &ir.model.pcurves {
        insert_live_identity(ctx, &mut ids, entity.id.as_str())?;
    }
    for entity in &ir.model.procedural_surfaces {
        insert_live_identity(ctx, &mut ids, entity.id.as_str())?;
    }
    for entity in &ir.model.procedural_curves {
        insert_live_identity(ctx, &mut ids, entity.id.as_str())?;
    }
    for unknown in unknowns.iter_mut() {
        unknown.links_mut().retain(|link| ids.contains(link));
        if !unknown.links().is_empty() {
            super::annotations::derived(ctx, annotations, unknown.id().as_str(), "links")?;
        }
    }
    Ok(())
}

pub(super) fn topology_body_node_ids(
    ctx: &DecodeContext<'_>,
    stream_index: usize,
    graph: &Graph,
) -> Result<BTreeMap<BodyId, BTreeSet<u32>>, CodecError> {
    let scope = IdScope::stream_charged(ctx, stream_index)?;
    let mut body_xmts = BTreeSet::new();
    for shell in graph.body_shape_shells() {
        if let Some(body_xmt) = shell
            .shell_fields()
            .and_then(|fields| fields.body.map(u32::from))
        {
            ctx.charge_collection_items(1, "nx topology body nodes")?;
            body_xmts.insert(body_xmt);
        }
    }
    let mut bodies = BTreeMap::new();
    'body: for body_xmt in body_xmts {
        let mut shells = BTreeSet::new();
        for shell in graph.of_kind(NodeKind::Shell) {
            if shell
                .shell_fields()
                .is_some_and(|fields| fields.body.map(u32::from) == Some(body_xmt))
            {
                ctx.charge_collection_items(1, "nx topology body shells")?;
                shells.insert(shell.xmt);
            }
        }
        let mut faces = Vec::new();
        let mut face_xmts = BTreeSet::new();
        for face in graph.of_kind(NodeKind::Face) {
            if face.face_fields().is_some_and(|fields| {
                fields
                    .shell
                    .is_some_and(|target| shells.contains(&u32::from(target)))
            }) {
                ctx.reserve_vec(&mut faces, 1, "nx topology body faces")?;
                faces.push(face);
                ctx.charge_collection_items(1, "nx topology body face nodes")?;
                face_xmts.insert(face.xmt);
            }
        }
        let mut loops = BTreeSet::new();
        for loop_ in graph.of_kind(NodeKind::Loop) {
            if loop_.loop_fields().is_some_and(|fields| {
                fields
                    .face
                    .is_some_and(|target| face_xmts.contains(&u32::from(target)))
            }) {
                ctx.charge_collection_items(1, "nx topology body loops")?;
                loops.insert(loop_.xmt);
            }
        }
        let mut fins = Vec::new();
        for fin in graph.of_kind(NodeKind::Fin) {
            if fin.fin_fields().is_some_and(|fields| {
                fields
                    .loop_xmt
                    .is_some_and(|target| loops.contains(&u32::from(target)))
            }) {
                ctx.reserve_vec(&mut fins, 1, "nx topology body fins")?;
                fins.push(fin);
            }
        }
        let mut edge_xmts = BTreeSet::new();
        let mut vertex_xmts = BTreeSet::new();
        for fin in fins {
            let Some(fields) = fin.fin_fields() else {
                continue;
            };
            let (Some(edge), Some(vertex)) = (fields.edge, fields.vertex) else {
                continue 'body;
            };
            ctx.charge_collection_items(1, "nx topology body edge nodes")?;
            edge_xmts.insert(u32::from(edge));
            ctx.charge_collection_items(1, "nx topology body vertex nodes")?;
            vertex_xmts.insert(u32::from(vertex));
        }
        let mut ids = BTreeSet::new();
        for face in faces {
            let Some(id) = face.u32_at(4) else {
                continue 'body;
            };
            ctx.charge_collection_items(1, "nx topology body identities")?;
            ids.insert(id);
        }
        let mut edge_count = 0;
        for edge in graph.of_kind(NodeKind::Edge) {
            if edge_xmts.contains(&edge.xmt) {
                edge_count += 1;
                let Some(id) = edge.u32_at(4) else {
                    continue 'body;
                };
                ctx.charge_collection_items(1, "nx topology body identities")?;
                ids.insert(id);
            }
        }
        if edge_count != edge_xmts.len() {
            continue;
        }
        let mut vertex_count = 0;
        for vertex in graph.of_kind(NodeKind::Vertex) {
            if vertex_xmts.contains(&vertex.xmt) {
                vertex_count += 1;
                let Some(id) = vertex.u32_at(4) else {
                    continue 'body;
                };
                ctx.charge_collection_items(1, "nx topology body identities")?;
                ids.insert(id);
            }
        }
        if vertex_count != vertex_xmts.len() {
            continue;
        }
        ctx.charge_collection_items(1, "nx topology body index")?;
        bodies.insert(
            scope.id_charged::<BodyId>(ctx, &cadmpeg_ir::identity_component!("body"), body_xmt)?,
            ids,
        );
    }
    Ok(bodies)
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
    let mut active = BTreeSet::new();
    for id in rmfastload_ids {
        ctx.charge_collection_items(1, "nx rmfastload active node ids")?;
        active.insert(*id);
    }
    let mut selected = BTreeSet::new();
    for (body, ids) in body_node_ids {
        if !ids.is_empty() && ids.is_subset(&active) {
            ctx.charge_collection_items(1, "nx rmfastload selected bodies")?;
            selected.insert(body.try_clone_for_decode(ctx, "nx rmfastload selected body identity")?);
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
    let mut streams = BTreeSet::new();
    for body in selected {
        let Some(index) = body
            .as_str()
            .strip_prefix("nx:s")
            .and_then(|text| text.split_once(':'))
            .and_then(|(text, _)| text.parse().ok())
        else {
            return Ok(None);
        };
        ctx.charge_collection_items(1, "nx rmfastload stream indices")?;
        streams.insert(index);
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
    let mut emitted = BTreeSet::new();
    for body in &ir.model.bodies {
        insert_retained_id(
            ctx,
            &mut emitted,
            body.id.as_str(),
            "nx emitted body selection",
        )?;
    }
    if !selected.is_subset(&emitted) {
        return Ok(false);
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
    let value = ctx.format_retained(format_args!("{}", value), "nx body selection attribute")?;
    ctx.charge_collection_items(1, "nx body selection attributes")?;
    attributes.insert(key, value);
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
    let selected = rmfastload_selected_bodies(ctx, body_node_ids, rmfastload_ids)?;
    if selected.is_empty() {
        return Ok(false);
    }
    let selected_hits = selected
        .iter()
        .filter_map(|body| body_node_ids.get(body))
        .map(BTreeSet::len)
        .sum::<usize>();
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
    let mut emitted = BTreeSet::new();
    for body in &ir.model.bodies {
        ctx.charge_collection_items(1, "nx terminal body selection index")?;
        emitted.insert(body.id.try_clone_for_decode(ctx, "nx terminal body selection identity")?);
    }
    // A complete terminal mapping resolves composition even when every emitted
    // body is terminal. The absence of pruning is a valid result: it means the
    // retained body images are all final, not that lineage was unresolved.
    let Some(selected) = crate::native::model::terminal_feature_body_ids(
        ctx,
        &emitted,
        &model.segments.segment_body_bindings,
        &model.segments.segment_body_lineage_statuses,
    )?
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
    ir.model.bodies.retain(|body| selected.contains(&body.id));
    ir.model
        .regions
        .retain(|region| selected.contains(&region.body));
    let mut regions: BTreeSet<RegionId> = BTreeSet::new();
    for region in &ir.model.regions {
        insert_retained_id(ctx, &mut regions, region.id.as_str(), "nx active regions")?;
    }
    ir.model
        .shells
        .retain(|shell| regions.contains(&shell.region));
    let mut shells: BTreeSet<ShellId> = BTreeSet::new();
    for shell in &ir.model.shells {
        insert_retained_id(ctx, &mut shells, shell.id.as_str(), "nx active shells")?;
    }
    ir.model.faces.retain(|face| shells.contains(&face.shell));
    let mut faces: BTreeSet<FaceId> = BTreeSet::new();
    for face in &ir.model.faces {
        insert_retained_id(ctx, &mut faces, face.id.as_str(), "nx active faces")?;
    }
    ir.model.loops.retain(|loop_| faces.contains(&loop_.face));
    let mut loops: BTreeSet<LoopId> = BTreeSet::new();
    for loop_ in &ir.model.loops {
        insert_retained_id(ctx, &mut loops, loop_.id.as_str(), "nx active loops")?;
    }
    ir.model
        .coedges
        .retain(|coedge| loops.contains(&coedge.owner_loop));
    let mut edges: BTreeSet<EdgeId> = BTreeSet::new();
    for coedge in &ir.model.coedges {
        insert_retained_id(ctx, &mut edges, coedge.edge.as_str(), "nx active edges")?;
    }
    for shell in &ir.model.shells {
        for edge in shell.wire_edges() {
            insert_retained_id(ctx, &mut edges, edge.as_str(), "nx active edges")?;
        }
    }
    ir.model.edges.retain(|edge| edges.contains(&edge.id));
    let mut vertices: BTreeSet<VertexId> = BTreeSet::new();
    for edge in &ir.model.edges {
        insert_retained_id(
            ctx,
            &mut vertices,
            edge.start.as_str(),
            "nx active vertices",
        )?;
        insert_retained_id(ctx, &mut vertices, edge.end.as_str(), "nx active vertices")?;
    }
    for shell in &ir.model.shells {
        for vertex in shell.free_vertices() {
            insert_retained_id(ctx, &mut vertices, vertex.as_str(), "nx active vertices")?;
        }
    }
    ir.model
        .vertices
        .retain(|vertex| vertices.contains(&vertex.id));
    let mut points: BTreeSet<PointId> = BTreeSet::new();
    for vertex in &ir.model.vertices {
        insert_retained_id(ctx, &mut points, vertex.point.as_str(), "nx active points")?;
    }
    ir.model.points.retain(|point| points.contains(&point.id));
    prune_inactive_geometry(ctx, ir)?;
    Ok(())
}

fn prune_inactive_geometry(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
    let mut surfaces: BTreeSet<SurfaceId> = BTreeSet::new();
    for face in &ir.model.faces {
        insert_retained_id(
            ctx,
            &mut surfaces,
            face.surface.as_str(),
            "nx active surfaces",
        )?;
    }
    let mut curves: BTreeSet<CurveId> = BTreeSet::new();
    for edge in &ir.model.edges {
        if let Some(curve) = edge.curve() {
            insert_retained_id(ctx, &mut curves, curve.as_str(), "nx active curves")?;
        }
    }
    let mut pcurves: BTreeSet<PcurveId> = BTreeSet::new();
    for coedge in &ir.model.coedges {
        for pcurve in &coedge.pcurves {
            insert_retained_id(
                ctx,
                &mut pcurves,
                pcurve.pcurve.as_str(),
                "nx active pcurves",
            )?;
        }
    }

    loop {
        let old_surface_count = surfaces.len();
        let old_curve_count = curves.len();
        for procedural in &ir.model.procedural_surfaces {
            let Some(owner) = ir.model.procedural_surface_owner(&procedural.id) else {
                continue;
            };
            if !surfaces.contains(owner) {
                continue;
            }
            match procedural.definition() {
                ProceduralSurfaceDefinition::Offset(definition_payload) => {
                    let support = definition_payload.support();
                    insert_retained_id(ctx, &mut surfaces, support.as_str(), "nx active surfaces")?;
                }
                ProceduralSurfaceDefinition::Blend(definition_payload) => {
                    let supports = definition_payload.supports();
                    let spine = definition_payload.spine();

                    for support in supports.iter().flatten() {
                        insert_retained_id(
                            ctx,
                            &mut surfaces,
                            support.surface.as_str(),
                            "nx active surfaces",
                        )?;
                    }
                    if let Some(spine) = spine {
                        insert_retained_id(ctx, &mut curves, spine.as_str(), "nx active curves")?;
                    }
                }
                _ => {}
            }
        }
        for procedural in &ir.model.procedural_curves {
            let Some(owner) = ir.model.procedural_curve_owner(&procedural.id) else {
                continue;
            };
            if !curves.contains(owner) {
                continue;
            }
            match procedural.definition() {
                ProceduralCurveDefinition::Intersection { context, .. } => {
                    for side in context.sides() {
                        if let Some(surface) = &side.surface {
                            insert_retained_id(
                                ctx,
                                &mut surfaces,
                                surface.as_str(),
                                "nx active surfaces",
                            )?;
                        }
                    }
                }
                ProceduralCurveDefinition::SurfaceCurve { family } => {
                    for side in family.context().sides() {
                        if let Some(surface) = &side.surface {
                            insert_retained_id(
                                ctx,
                                &mut surfaces,
                                surface.as_str(),
                                "nx active surfaces",
                            )?;
                        }
                    }
                }
                _ => {}
            }
        }
        if surfaces.len() == old_surface_count && curves.len() == old_curve_count {
            break;
        }
    }

    let mut surface_constructions: BTreeSet<ProceduralSurfaceId> = BTreeSet::new();
    for surface in &ir.model.surfaces {
        if surfaces.contains(&surface.id) {
            if let Some(construction) = surface.geometry.procedural_construction() {
                insert_retained_id(
                    ctx,
                    &mut surface_constructions,
                    construction.as_str(),
                    "nx active surface constructions",
                )?;
            }
        }
    }
    let mut curve_constructions: BTreeSet<ProceduralCurveId> = BTreeSet::new();
    for curve in &ir.model.curves {
        if curves.contains(&curve.id) {
            if let Some(construction) = curve.geometry.procedural_construction() {
                insert_retained_id(
                    ctx,
                    &mut curve_constructions,
                    construction.as_str(),
                    "nx active curve constructions",
                )?;
            }
        }
    }
    ir.model
        .procedural_surfaces
        .retain(|procedural| surface_constructions.contains(&procedural.id));
    ir.model
        .procedural_curves
        .retain(|procedural| curve_constructions.contains(&procedural.id));
    ir.model
        .surfaces
        .retain(|surface| surfaces.contains(&surface.id));
    ir.model.curves.retain(|curve| curves.contains(&curve.id));
    ir.model
        .pcurves
        .retain(|pcurve| pcurves.contains(&pcurve.id));
    Ok(())
}

fn finalize_point_topology(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    let mut referenced_points: BTreeSet<PointId> = BTreeSet::new();
    for vertex in &ir.model.vertices {
        ctx.charge_collection_items(1, "nx referenced points")?;
        referenced_points.insert(vertex.point.try_clone_for_decode(ctx, "nx referenced point identity")?);
    }
    if !ir.model.bodies.is_empty() {
        ir.model
            .points
            .retain(|point| referenced_points.contains(&point.id));
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
    let stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
    for id in [body_id.as_str(), region_id.as_str(), shell_id.as_str()] {
        super::annotations::note(ctx, annotations, id, &stream, 0, "derived_point_topology")?;
        super::annotations::exactness(ctx, annotations, id, Exactness::Inferred)?;
    }

    let point_count = ir.model.points.len();
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(point_count),
        "nx point topology free vertices",
    )?;
    let mut free_vertices = ctx.collection_vec(point_count, "nx point topology vertices")?;
    ir.model.vertices.try_reserve(point_count).map_err(|_| {
        ctx.refuse_codec_limit(
            "nx point topology vertices",
            0,
            cadmpeg_core::decode::u64_from_index(point_count),
        )
    })?;
    for (index, point) in ir.model.points.iter().enumerate() {
        let vertex_id: VertexId =
            derived.id_charged(ctx, &cadmpeg_ir::identity_component!("point-vertex"), index)?;
        super::annotations::note(
            ctx,
            annotations,
            vertex_id.as_str(),
            &stream,
            0,
            "derived_point_topology",
        )?;
        super::annotations::exactness(ctx, annotations, vertex_id.as_str(), Exactness::Inferred)?;
        ir.model.vertices.push(Vertex {
            id: vertex_id.try_clone_for_decode(ctx, "nx point vertex identity copy")?,
            point: point.id.try_clone_for_decode(ctx, "nx point reference identity")?,
            tolerance: None,
        });
        free_vertices.push(vertex_id);
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

fn insert_body_relation<K>(
    ctx: &DecodeContext<'_>,
    map: &mut BTreeMap<K, BodyId>,
    key: &str,
    body: &BodyId,
) -> Result<(), CodecError>
where
    K: Ord + TryFrom<String>,
    K::Error: std::fmt::Display,
{
    ctx.charge_collection_items(1, "nx body classification relations")?;
    map.insert(
        K::try_from(ctx.copy_retained_text(key, "nx body classification relation identity")?).map_err(CodecError::malformed)?,
        body.try_clone_for_decode(ctx, "nx body classification owner identity")?,
    );
    Ok(())
}

fn classify_body_kinds(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
    let mut region_bodies: BTreeMap<RegionId, BodyId> = BTreeMap::new();
    for region in &ir.model.regions {
        insert_body_relation(ctx, &mut region_bodies, region.id.as_str(), &region.body)?;
    }
    let mut shell_bodies: BTreeMap<ShellId, BodyId> = BTreeMap::new();
    for shell in &ir.model.shells {
        if let Some(body) = region_bodies.get(&shell.region) {
            insert_body_relation(ctx, &mut shell_bodies, shell.id.as_str(), body)?;
        }
    }
    let mut face_bodies: BTreeMap<FaceId, BodyId> = BTreeMap::new();
    for face in &ir.model.faces {
        if let Some(body) = shell_bodies.get(&face.shell) {
            insert_body_relation(ctx, &mut face_bodies, face.id.as_str(), body)?;
        }
    }
    let mut loop_bodies: BTreeMap<LoopId, BodyId> = BTreeMap::new();
    for loop_ in &ir.model.loops {
        if let Some(body) = face_bodies.get(&loop_.face) {
            insert_body_relation(ctx, &mut loop_bodies, loop_.id.as_str(), body)?;
        }
    }
    let mut coedge_bodies: BTreeMap<CoedgeId, BodyId> = BTreeMap::new();
    for coedge in &ir.model.coedges {
        if let Some(body) = loop_bodies.get(&coedge.owner_loop) {
            insert_body_relation(ctx, &mut coedge_bodies, coedge.id.as_str(), body)?;
        }
    }
    let mut edge_uses = BTreeMap::<BodyId, BTreeMap<EdgeId, usize>>::new();
    for coedge in &ir.model.coedges {
        let Some(body) = coedge_bodies.get(&coedge.id) else {
            continue;
        };
        if !edge_uses.contains_key(body) {
            ctx.charge_collection_items(1, "nx body classification edge owners")?;
            edge_uses.insert(
                body.try_clone_for_decode(ctx, "nx body classification edge owner identity")?,
                BTreeMap::new(),
            );
        }
        let Some(edges) = edge_uses.get_mut(body) else {
            return Err(CodecError::malformed(
                "NX body classification owner missing",
            ));
        };
        if !edges.contains_key(&coedge.edge) {
            ctx.charge_collection_items(1, "nx body classification edge uses")?;
            edges.insert(
                coedge.edge.try_clone_for_decode(ctx, "nx body classification edge identity")?,
                0,
            );
        }
        let Some(count) = edges.get_mut(&coedge.edge) else {
            return Err(CodecError::malformed("NX body classification edge missing"));
        };
        *count = count
            .checked_add(1)
            .ok_or_else(|| ctx.refuse_codec_limit("nx body classification edge uses", 0, 1))?;
    }
    for body in &mut ir.model.bodies {
        body.kind = if edge_uses
            .get(&body.id)
            .is_some_and(|uses| !uses.is_empty() && uses.values().all(|use_count| *use_count == 2))
        {
            BodyKind::Solid
        } else {
            BodyKind::Sheet
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests;
