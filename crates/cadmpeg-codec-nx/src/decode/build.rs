// SPDX-License-Identifier: Apache-2.0
//! Geometry decode, active-body selection, and inactive-topology prune.

use super::emit::{
    annotate_node, canonical_trim_range, curve_tag, decoded_tolerance, emit_topology,
    retain_unknown_stream_data, retain_unresolved_topology_carriers, source_meta, surface_tag,
    unknown_stream_metadata,
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
use cadmpeg_core::decode::{DecodeContext, WorkBudget};
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
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{Body, BodyKind, Point, Region, Shell, Vertex};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn ordered_point_candidates(graph: &Graph) -> Vec<(FinitePoint3, &Node)> {
    ordered_fixed_candidates(graph, [NodeKind::Point], Node::point_position)
}

pub(super) fn ordered_surface_candidates(graph: &Graph) -> Vec<(SurfaceGeometry, &Node)> {
    ordered_fixed_candidates(
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

pub(super) fn ordered_curve_candidates(graph: &Graph) -> Vec<(CurveGeometry, &Node)> {
    ordered_fixed_candidates(
        graph,
        [NodeKind::Line, NodeKind::Circle, NodeKind::Ellipse],
        Node::curve_geometry,
    )
}

fn ordered_fixed_candidates<T>(
    graph: &Graph,
    kinds: impl IntoIterator<Item = NodeKind>,
    graph_value: impl Fn(&Node) -> Option<T>,
) -> Vec<(T, &Node)> {
    let mut candidates = BTreeMap::new();
    for node in kinds.into_iter().flat_map(|kind| graph.of_kind(kind)) {
        if let Some(value) = graph_value(node) {
            candidates.insert(node.pos(), (value, node));
        }
    }
    candidates.into_values().collect()
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
    // This scope appends carriers and preserves their identity and construction links.
    let mut procedural_admission =
        cadmpeg_ir::document::procedural::ProceduralAdmission::new(&ir.model);

    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    let mut stream_unknowns = Vec::new();
    let mut counts = Counts::default();
    let mut body_node_ids = BTreeMap::new();
    let mut parsed = crate::native::substrate::ParsedStreams::parse(scan)?;
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
                topology_body_node_ids(si, &parsed.stream(si).view_for_geometry().graph)?
            {
                body_node_ids.insert(body, nodes);
            }
        }
    }
    let rmfastload_selected = rmfastload_selected_bodies(&body_node_ids, rmfastload_ids);
    let allow_terminal_lineage =
        rmfastload_allows_terminal_lineage(body_node_ids.len(), &rmfastload_selected);
    let rmfastload_preselection = if body_node_ids.len() > 1
        && !rmfastload_selected.is_empty()
        && rmfastload_selected.len() < body_node_ids.len()
    {
        rmfastload_stream_indices(&rmfastload_selected).map(|streams| {
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
        emitted_body_ids.insert(body.clone());
    }
    let terminal_preselection = match terminal_lineage.as_ref() {
        Some(lineage) => crate::native::model::terminal_feature_body_ids(
            &emitted_body_ids,
            &lineage.bindings,
            &lineage.statuses,
        )
        .filter(|selected| selected.len() < body_node_ids.len())
        .and_then(|selected| {
            rmfastload_stream_indices(&selected)
                .map(|streams| (selected, streams, "terminal_feature_body_lineage"))
        }),
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
    let exact_transfer_budget = WorkBudget::new(MAX_EXACT_BOUNDARY_TRANSFER_SAMPLES);
    let transfer_budget = WorkBudget::new(transfer_limit);
    let support_uv_validation_budget = WorkBudget::new(support_uv_limit);
    let support_budget = WorkBudget::new(support_uv_limit);
    let coupled_support_budget = WorkBudget::new(support_uv_limit);
    let adaptive_geometry_budget =
        GeometryWorkBudget::from_context(ctx, MAX_ADAPTIVE_GEOMETRY_WORK);
    let completion_geometry_budget =
        GeometryWorkBudget::from_context(ctx, MAX_PCURVE_COMPLETION_GEOMETRY_WORK);
    let support_uv_geometry_budget =
        GeometryWorkBudget::from_context(ctx, MAX_SUPPORT_UV_COMPLETION_GEOMETRY_WORK);
    let coupled_support_uv_geometry_budget =
        GeometryWorkBudget::from_context(ctx, MAX_COUPLED_SUPPORT_UV_GEOMETRY_WORK);
    let serialized_support_uv_geometry_budget =
        GeometryWorkBudget::from_context(ctx, MAX_SERIALIZED_SUPPORT_UV_GEOMETRY_WORK);
    let mut support_uv_lane_geometry_exhausted = false;
    let mut intersection_index = IntersectionIncidenceIndex::default();
    let mut model_endpoint_witnesses = EndpointWitnesses::new();
    let mut completion_streams = Vec::new();
    let container_stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));

    for (si, stream) in scan.streams.iter().enumerate() {
        if !stream.kind().is_parasolid() {
            continue;
        }
        let scope = IdScope::stream_from_index(si)?;
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
            let unknown = unknown_stream_metadata(si, stream)?;
            annotations.note(
                unknown.id().as_str(),
                &container_stream,
                cadmpeg_core::decode::u64_from_index(stream.file_offset),
                Some(stream.kind().label()),
            );
            annotations.exactness(unknown.id().as_str(), Exactness::Derived);
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
            carrier_refusals.push(NxLossCode::CarrierLanesUnpaired.note(format!(
                "parasolid#{si} {} at byte {} states no carrier: {}",
                refusal.family, refusal.pos, refusal.error
            )));
        }
        let view = parsed.stream(si).view_for_geometry();
        let stream_name = format!("nx:parasolid#{si}:{}", stream.kind().label());
        let source_stream = StreamHandle::new(
            cadmpeg_ir::StreamName::try_from(stream_name).map_err(CodecError::malformed)?,
        );

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
        for (pi, (position, node)) in ordered_point_candidates(graph).into_iter().enumerate() {
            let pid: PointId = scope.id_from_display(&cadmpeg_ir::identity_component!("pt"), pi)?;
            let vid: VertexId = scope.id_from_display(&cadmpeg_ir::identity_component!("v"), pi)?;
            annotate_node(
                &mut annotations,
                pid.as_str(),
                &source_stream,
                node,
                "POINT",
            );
            annotations.derived(pid.as_str(), "position")?;

            ir.model
                .points
                .push(Point::new(pid.clone(), position, None));

            ir.model.vertices.push(Vertex {
                id: vid.clone(),
                point: pid.clone(),
                tolerance: None,
            });
            points_by_xmt.insert(node.xmt(), pid);
            counts.points += 1;
        }
        for (fi, (geometry, node)) in ordered_surface_candidates(graph).into_iter().enumerate() {
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
                scope.id_from_display(&cadmpeg_ir::identity_component!("surf"), fi)?;
            annotate_node(
                &mut annotations,
                id.as_str(),
                &source_stream,
                node,
                surface_tag(geometry.solved().ok_or_else(|| {
                    cadmpeg_core::CodecError::NotImplemented(
                        "carrier has no solved geometry".into(),
                    )
                })?),
            );
            annotations.derived(id.as_str(), "geometry")?;

            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry,
                source_object: None,
            });
            surfaces_by_xmt.insert(node.xmt(), id);
        }
        for (fi, surf) in nurbs_surfaces.into_iter().enumerate() {
            counts.nurbs_surfaces += 1;
            let id: SurfaceId =
                scope.id_from_display(&cadmpeg_ir::identity_component!("nurbs-surf"), fi)?;
            annotations.note(
                id.as_str(),
                &source_stream,
                cadmpeg_core::decode::u64_from_index(surf.pos),
                Some("B_SPLINE_SURFACE"),
            );
            annotations.derived(id.as_str(), "geometry")?;

            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry: surf.geometry,
                source_object: None,
            });
            if let Some(node) = graph.at_pos(surf.pos) {
                surfaces_by_xmt.insert(node.xmt(), id);
            }
        }
        let saved_offset_carriers = saved_offset_carriers(
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
            let support: SurfaceId = support_ref.clone();
            let procedural_id: ProceduralSurfaceId =
                scope.id_from_display(&cadmpeg_ir::identity_component!("offset"), oi)?;
            let (surface_id, cache_fit_tolerance) =
                if let Some((surface, fit_tolerance)) = saved_offset_carriers.get(&offset.xmt) {
                    (surface.clone(), Some(*fit_tolerance))
                } else {
                    let surface_id: SurfaceId = scope
                        .id_from_display(&cadmpeg_ir::identity_component!("offset-surf"), oi)?;
                    annotations.note(
                        surface_id.as_str(),
                        &source_stream,
                        cadmpeg_core::decode::u64_from_index(offset.pos),
                        Some("OFFSET_SURF"),
                    );
                    annotations.derived(surface_id.as_str(), "geometry")?;

                    ir.model.surfaces.push(Surface {
                        id: surface_id.clone(),
                        geometry: SurfaceGeometry::Procedural {
                            construction: procedural_id.clone(),
                            cache: None,
                        },
                        source_object: Some(SourceObjectAssociation {
                            geometry_role: None,
                            format: cadmpeg_ir::CodecFormat::Nx,
                            object_id: cadmpeg_core::text::NonBlankString::new(format!(
                                "nx:s{si}:offset-surface-record#{}",
                                offset.xmt
                            ))
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
            annotations.note(
                procedural_id.as_str(),
                &source_stream,
                cadmpeg_core::decode::u64_from_index(offset.pos),
                Some("OFFSET_SURF"),
            );
            annotations.derived(procedural_id.as_str(), "definition")?;
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

            let _attached =
                procedural_admission.add_surface(&mut ir.model, &surface_id.clone(), procedural)?;

            surfaces_by_xmt.insert(offset.xmt, surface_id);
            counts.offset_surfaces += 1;
        }

        for (bi, blend) in view.blend_surfaces.iter().copied().enumerate() {
            let surface_id: SurfaceId =
                scope.id_from_display(&cadmpeg_ir::identity_component!("blend-surf"), bi)?;
            let procedural_id: ProceduralSurfaceId =
                scope.id_from_display(&cadmpeg_ir::identity_component!("blend"), bi)?;
            annotations.note(
                surface_id.as_str(),
                &source_stream,
                cadmpeg_core::decode::u64_from_index(blend.pos),
                Some("BLEND_SURF"),
            );
            annotations.derived(surface_id.as_str(), "geometry")?;

            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Procedural {
                    construction: procedural_id.clone(),
                    cache: None,
                },
                source_object: Some(SourceObjectAssociation {
                    geometry_role: None,
                    format: cadmpeg_ir::CodecFormat::Nx,
                    object_id: cadmpeg_core::text::NonBlankString::new(format!(
                        "nx:s{si}:blend-surface-record#{}",
                        blend.xmt
                    ))
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
            annotations.note(
                procedural_id.as_str(),
                &source_stream,
                cadmpeg_core::decode::u64_from_index(blend.pos),
                Some("BLEND_SURF"),
            );
            annotations.derived(procedural_id.as_str(), "definition")?;
            let procedural_index = ir.model.procedural_surfaces.len();

            let attached = procedural_admission.add_surface(
                &mut ir.model,
                &surface_id.clone(),
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
                pending_blend_supports.push((
                    procedural_index,
                    blend.state.support_xmts(),
                    blend.state.offsets(),
                ));
                if blend.state.spine_xmt() > 1 {
                    pending_blend_spines.push((procedural_index, blend.state.spine_xmt()));
                }
            }
            surfaces_by_xmt.insert(blend.xmt, surface_id);
            counts.blend_surfaces += 1;
        }
        for (procedural_index, support_xmts, offsets) in pending_blend_supports {
            let [first, second] = [0, 1].map(|side| {
                surfaces_by_xmt
                    .get(&support_xmts[side])
                    .map(|surface| BlendSupport {
                        surface: surface.clone(),
                        reversed: offsets[side].is_sign_negative(),
                    })
            });
            let supports = [first, second];
            let Some(procedural) = ir.model.procedural_surfaces.get_mut(procedural_index) else {
                continue;
            };
            procedural.edit_definition(|definition| {
                if let ProceduralSurfaceDefinition::Blend(definition_payload) = definition {
                    definition_payload.set_supports(supports);
                }
            });
        }

        for (ci, (geometry, node)) in ordered_curve_candidates(graph).into_iter().enumerate() {
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
            let id: CurveId = scope.id_from_display(&cadmpeg_ir::identity_component!("crv"), ci)?;
            annotate_node(
                &mut annotations,
                id.as_str(),
                &source_stream,
                node,
                curve_tag(geometry.solved().ok_or_else(|| {
                    cadmpeg_core::CodecError::NotImplemented(
                        "carrier has no solved geometry".into(),
                    )
                })?),
            );
            annotations.derived(id.as_str(), "geometry")?;

            ir.model.curves.push(Curve {
                parameter_range: None,
                id: id.clone(),
                geometry,
                source_object: None,
            });
            curves_by_xmt.insert(node.xmt(), id);
        }
        for (ci, crv) in nurbs_curves.into_iter().enumerate() {
            counts.nurbs_curves += 1;
            let id: CurveId =
                scope.id_from_display(&cadmpeg_ir::identity_component!("nurbs-crv"), ci)?;
            annotations.note(
                id.as_str(),
                &source_stream,
                cadmpeg_core::decode::u64_from_index(crv.pos),
                Some("B_SPLINE_CURVE"),
            );
            annotations.derived(id.as_str(), "geometry")?;

            ir.model.curves.push(Curve {
                parameter_range: None,
                id: id.clone(),
                geometry: crv.geometry,
                source_object: None,
            });
            if let Some(node) = graph.at_pos(crv.pos) {
                curves_by_xmt.insert(node.xmt(), id);
            }
        }

        for (pi, pcurve) in nurbs_pcurves.into_iter().enumerate() {
            let id: PcurveId =
                scope.id_from_display(&cadmpeg_ir::identity_component!("pcurve"), pi)?;
            annotations.note(
                id.as_str(),
                &source_stream,
                cadmpeg_core::decode::u64_from_index(pcurve.pos),
                Some("B_CURVE_2D"),
            );
            annotations.derived(id.as_str(), "geometry")?;

            ir.model.pcurves.push(Pcurve {
                id: id.clone(),
                geometry: pcurve.geometry,
                metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::default(),
            });
            if let Some(node) = graph.at_pos(pcurve.pos) {
                pcurves_by_xmt.insert(node.xmt(), id);
            }
        }
        let intersection_scan = view.intersections.clone();
        counts
            .intersection_rejections
            .extend(intersection_scan.rejected);
        let intersection_constructions = intersection_scan.constructions;
        let mut charted_intersections = BTreeMap::new();
        for curve in intersection_scan.curves {
            charted_intersections.insert(curve.xmt, curve);
        }
        let mut uncharted_intersections = BTreeMap::new();
        for curve in intersection_scan.uncharted {
            uncharted_intersections.insert(curve.xmt, curve);
        }
        let intersection_support_uv = {
            let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(&ir);
            intersection_constructions
                .iter()
                .map(
                    |construction| -> Result<Option<_>, cadmpeg_core::CodecError> {
                        let Some(charted) = charted_intersections.get(&construction.xmt) else {
                            return Ok(None);
                        };
                        let mut support_uv = validate_serialized_support_uv_with_index(
                            &model_index,
                            &crate::decode::support_uv::SerializedSupportUvFit {
                                surfaces_by_xmt: &surfaces_by_xmt,
                                supports: [
                                    Some(charted.primary_support),
                                    charted.secondary_support,
                                ],
                                points: &charted.samples.points(),
                                fit_tolerance: charted.fit_tolerance.get(),
                                lanes: &charted.support_uv,
                            },
                            &serialized_support_uv_geometry_budget,
                        )?;
                        if let Some(ext_support_uv) = assign_ext11_support_uv_with_index(
                            &model_index,
                            &crate::decode::support_uv::SerializedSupportUvFit {
                                surfaces_by_xmt: &surfaces_by_xmt,
                                supports: [
                                    Some(charted.primary_support),
                                    charted.secondary_support,
                                ],
                                points: &charted.samples.points(),
                                fit_tolerance: charted.fit_tolerance.get(),
                                lanes: &charted.ext_support_uv,
                            },
                            &serialized_support_uv_geometry_budget,
                        )? {
                            for side in 0..2 {
                                if support_uv[side].is_none() {
                                    support_uv[side] = ext_support_uv[side]
                                        .as_ref()
                                        .map(|lane| {
                                            crate::intersection::SupportUvLane::from_checked(
                                                lane.as_slice().to_vec(),
                                                lane.as_slice().len(),
                                            )
                                            .ok_or_else(|| {
                                                CodecError::malformed(
                                                    "NX copied support-UV lane count",
                                                )
                                            })
                                        })
                                        .transpose()?;
                                }
                            }
                        }
                        Ok(Some((construction.xmt, support_uv)))
                    },
                )
                .try_fold(BTreeMap::new(), |mut values, admitted| {
                    if let Some((xmt, support_uv)) = admitted? {
                        values.insert(xmt, support_uv);
                    }
                    Ok::<_, cadmpeg_core::CodecError>(values)
                })?
        };
        for (ci, construction) in intersection_constructions.into_iter().enumerate() {
            let curve_id: CurveId =
                scope.id_from_display(&cadmpeg_ir::identity_component!("intersection-crv"), ci)?;
            let procedural_id: ProceduralCurveId =
                scope.id_from_display(&cadmpeg_ir::identity_component!("intersection"), ci)?;
            let unknown_id: UnknownId = IdScope::container()
                .id_from_display(&cadmpeg_ir::identity_component!("parasolid"), si)?;
            let charted = charted_intersections.get(&construction.xmt);
            let uncharted = if let Some(uncharted) = uncharted_intersections.get(&construction.xmt)
            {
                let [first, second] = uncharted
                    .supports
                    .references()
                    .map(|xmt| surfaces_by_xmt.get(&u32::from(xmt)));
                if let [Some(first), Some(second)] = [first, second] {
                    cadmpeg_ir::geometry::TolerantIntersectionConstruction::from_parts(
                        [first.clone(), second.clone()],
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
                            validated_support_uv_lanes.insert((procedural_id.clone(), side));
                        }
                    }
                }
                let [value_first, value_second] = [0, 1].map(|side| {
                    charted.support_uv[side]
                        .as_ref()
                        .map(|lane| {
                            crate::intersection::SupportUvLane::from_checked(
                                lane.as_slice().to_vec(),
                                lane.as_slice().len(),
                            )
                            .ok_or_else(|| CodecError::malformed("NX copied support-UV lane count"))
                        })
                        .transpose()
                });
                let [ext_first, ext_second] = [0, 1].map(|side| {
                    charted.ext_support_uv[side]
                        .as_ref()
                        .map(|lane| {
                            crate::intersection::SupportUvLane::from_checked(
                                lane.as_slice().to_vec(),
                                lane.as_slice().len(),
                            )
                            .ok_or_else(|| CodecError::malformed("NX copied support-UV lane count"))
                        })
                        .transpose()
                });

                pending_ext11_support_uv.push((
                    procedural_id.clone(),
                    charted.samples.clone(),
                    charted.fit_tolerance.get(),
                    SerializedSupportUv {
                        values: [value_first?, value_second?],
                        ext11: [ext_first?, ext_second?],
                    },
                ));
            }
            annotations.note(
                curve_id.as_str(),
                &source_stream,
                cadmpeg_core::decode::u64_from_index(construction.pos),
                Some("INTERSECTION"),
            );
            if charted.is_some() || uncharted.is_some() {
                annotations.derived(curve_id.as_str(), "geometry")?;
            } else {
                annotations.exactness(curve_id.as_str(), Exactness::Unknown);
            }

            ir.model.curves.push(Curve {
                parameter_range: None,
                id: curve_id.clone(),
                geometry: if let Some(charted) = charted {
                    CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                        cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                            1,
                            linear_knots(&charted.samples.parameters()),
                            charted.samples.points(),
                            None,
                            false,
                        )
                        .map_err(|error| CodecError::Malformed(error.to_string()))?,
                    ))
                } else if uncharted.is_some() {
                    CurveGeometry::Procedural {
                        construction: procedural_id.clone(),
                        cache: None,
                    }
                } else {
                    CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                        record: Some(unknown_id.clone()),
                    })
                },
                source_object: Some(SourceObjectAssociation {
                    geometry_role: None,
                    format: cadmpeg_ir::CodecFormat::Nx,
                    object_id: cadmpeg_core::text::NonBlankString::new(format!(
                        "nx:s{si}:intersection-record#{}",
                        construction.xmt
                    ))
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
            annotations.note(
                procedural_id.as_str(),
                &source_stream,
                cadmpeg_core::decode::u64_from_index(construction.pos),
                Some("INTERSECTION"),
            );
            if charted.is_some() || uncharted.is_some() {
                annotations.derived(procedural_id.as_str(), "definition")?;
            } else {
                annotations.exactness(procedural_id.as_str(), Exactness::Unknown);
            }
            let definition = if let Some(charted) = charted {
                let support_uv = if let Some(lanes) = intersection_support_uv.get(&construction.xmt)
                {
                    let [first, second] = [0, 1].map(|side| {
                        lanes[side]
                            .as_ref()
                            .map(|lane| {
                                crate::intersection::SupportUvLane::from_checked(
                                    lane.as_slice().to_vec(),
                                    lane.as_slice().len(),
                                )
                                .ok_or_else(|| {
                                    CodecError::malformed("NX copied support-UV lane count")
                                })
                            })
                            .transpose()
                    });
                    [first?, second?]
                } else {
                    [None, None]
                };
                let parameters = charted.samples.parameters();
                let first = intersection_side(
                    &ir,
                    &surfaces_by_xmt,
                    Some(charted.primary_support),
                    support_uv[0]
                        .as_deref()
                        .map(|uv| (uv, parameters.as_slice())),
                )?;
                let second = intersection_side(
                    &ir,
                    &surfaces_by_xmt,
                    charted.secondary_support,
                    support_uv[1]
                        .as_deref()
                        .map(|uv| (uv, parameters.as_slice())),
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

            let _attached =
                procedural_admission.add_curve(&mut ir.model, &curve_id.clone(), procedural)?;

            curves_by_xmt.insert(construction.xmt, curve_id);
            counts.intersection_curves += 1;
        }
        for (procedural_index, spine_xmt) in pending_blend_spines {
            let Some(spine_ref) = curves_by_xmt.get(&spine_xmt) else {
                continue;
            };
            let spine = spine_ref.clone();
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
            curve_indices.insert(curve.id.clone(), index);
        }
        let mut surface_indices: BTreeMap<SurfaceId, usize> = BTreeMap::new();
        for (index, surface) in ir.model.surfaces.iter().enumerate() {
            surface_indices.insert(surface.id.clone(), index);
        }
        let mut pcurve_indices: BTreeMap<PcurveId, usize> = BTreeMap::new();
        for (index, pcurve) in ir.model.pcurves.iter().enumerate() {
            pcurve_indices.insert(pcurve.id.clone(), index);
        }
        let surface_curves = &view.surface_curves;
        loop {
            let mapped = curves_by_xmt.len() + pcurves_by_xmt.len() + pcurve_supports_by_xmt.len();
            for trim in trimmed_curves {
                if let Some(basis_ref) = curves_by_xmt.get(&trim.state.basis()) {
                    let basis = basis_ref.clone();
                    let parameters = curve_indices
                        .get(&basis)
                        .and_then(|index| ir.model.curves.get(*index))
                        .and_then(|curve| {
                            canonical_trim_range(&curve.geometry, trim.state.parameters())
                        });
                    curves_by_xmt.insert(trim.xmt, basis);
                    if let Some(parameters) = parameters {
                        trim_ranges.insert(trim.xmt, parameters);
                    }
                }
                if let Some(pcurve_ref) = pcurves_by_xmt.get(&trim.state.basis()) {
                    let pcurve = pcurve_ref.clone();
                    pcurves_by_xmt.insert(trim.xmt, pcurve);
                    if let Some(support) = pcurve_supports_by_xmt.get(&trim.state.basis()) {
                        let support = support.clone();
                        pcurve_supports_by_xmt.insert(trim.xmt, support);
                    }
                    trim_ranges.insert(trim.xmt, trim.state.parameters());
                }
            }
            for surface_curve in surface_curves {
                if let Some(pcurve_ref) = pcurves_by_xmt.get(&surface_curve.state.pcurve()) {
                    let pcurve: PcurveId = pcurve_ref.clone();
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
                            normalize_pcurve_parameters(&mut carrier.geometry, support).is_some()
                        } else {
                            false
                        };
                        if !normalized {
                            pcurves_by_xmt.remove(&surface_curve.state.pcurve());
                            invalid_pcurves.insert(pcurve.clone());
                            continue;
                        }
                        normalized_pcurves.insert(pcurve.clone());
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
                    pcurves_by_xmt.insert(surface_curve.xmt, pcurve);
                    if let Some(support) = surfaces_by_xmt.get(&surface_curve.state.surface()) {
                        pcurve_supports_by_xmt.insert(surface_curve.xmt, support.clone());
                    }
                }
                if let Some(original) = surface_curve
                    .state
                    .original()
                    .and_then(|original| curves_by_xmt.get(&original))
                {
                    let original = original.clone();
                    curves_by_xmt.insert(surface_curve.xmt, original);
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
            intersection_index.reindex_pcurves_after_prune(&ir);
        }
        retain_unresolved_topology_carriers(
            &mut ir,
            crate::decode::emit::UnresolvedTopologyStream {
                stream_index: si,
                graph,
                surfaces: &mut surfaces_by_xmt,
                curves: &mut curves_by_xmt,
                pcurves: &pcurves_by_xmt,
                source_stream: &source_stream,
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
            &mut ir,
            &pending_ext11_support_uv,
            &serialized_support_uv_geometry_budget,
        )?;
        support_uv_geometry_budget.clear_blend_frame_cache();
        coupled_support_uv_geometry_budget.clear_blend_frame_cache();
        complete_parameterization_equivalent_support_uv(&mut ir)?;
        let mut completed_endpoint_witnesses = BTreeMap::new();
        support_uv_lane_geometry_exhausted |=
            complete_support_uv_with_budget_and_endpoint_witnesses(
                &mut ir,
                &pending_ext11_support_uv,
                (&support_budget, &support_uv_geometry_budget),
                (&coupled_support_budget, &coupled_support_uv_geometry_budget),
                &mut completed_endpoint_witnesses,
            )?;
        let mut validated_endpoint_witnesses = initial_endpoint_witnesses;
        extend_endpoint_witnesses(
            &mut validated_endpoint_witnesses,
            validated_support_uv_endpoint_witnesses(
                &ir,
                &pending_ext11_support_uv,
                &validated_support_uv_lanes,
            ),
        );
        extend_endpoint_witnesses(
            &mut validated_endpoint_witnesses,
            newly_validated_endpoint_witnesses,
        );
        extend_endpoint_witnesses(
            &mut validated_endpoint_witnesses,
            completed_endpoint_witnesses,
        );
        copy_endpoint_witnesses(&mut model_endpoint_witnesses, &validated_endpoint_witnesses);
        attach_completed_intersection_pcurves_for_stream_with_budget(
            &mut ir,
            crate::decode::support_uv::IntersectionStream {
                graph,
                scope: &IdScope::stream_from_index(si)?,
                coedge_start: intersection_starts.coedges,
                procedural_start: intersection_starts.procedural_curves,
                source_stream: source_stream.clone(),
                validated_endpoint_witnesses: &validated_endpoint_witnesses,
            },
            &mut annotations,
            &completion_geometry_budget,
        )?;
        // Preserve the whole inflated stream verbatim so nothing is dropped.
        let unknown_index = unknowns.len();
        let mut unknown = unknown_stream_metadata(si, stream)?;
        for surface in &ir.model.surfaces[first_surface..] {
            unknown.links_mut().push(surface.id.as_str().to_owned());
        }
        for curve in &ir.model.curves[first_curve..] {
            unknown.links_mut().push(curve.id.as_str().to_owned());
        }
        annotations.note(
            unknown.id().as_str(),
            &container_stream,
            cadmpeg_core::decode::u64_from_index(stream.file_offset),
            Some(stream.kind().label()),
        );
        annotations.exactness(unknown.id().as_str(), Exactness::Derived);
        unknowns.push(unknown);
        stream_unknowns.push((si, unknown_index));
    }

    drop(procedural_admission);
    intersection_index.complete_from_model(&mut ir);
    let mut completion_sources = Vec::new();
    for (si, source_stream) in &completion_streams {
        completion_sources.push(IntersectionCompletionSource {
            scope: IdScope::stream_from_index(*si)?,
            graph: parsed.stream(*si).view_for_geometry().graph.as_ref(),
            source_stream: source_stream.clone(),
            coedge_start: 0,
            procedural_start: 0,
        });
    }
    attach_completed_intersection_pcurves_for_model_with_budget(
        &mut ir,
        &completion_sources,
        &mut annotations,
        &model_endpoint_witnesses,
        &completion_geometry_budget,
    )?;

    if counts.points == 0 && counts.surfaces() == 0 && counts.curves() == 0 {
        return Ok(None);
    }

    ir.source = Some(source_meta(scan, dialects, Some(&parsed))?);

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
        let selected_hits = selected
            .iter()
            .filter_map(|body| body_node_ids.get(body))
            .map(BTreeSet::len)
            .sum::<usize>();
        apply_preselected_active_body_selection(&mut ir, selected, source, Some(selected_hits))
    } else {
        select_active_body(&mut ir, &body_node_ids, rmfastload_ids)
    };
    if !active_body_selection {
        active_body_selection = select_terminal_feature_bodies(&mut ir, &model);
    }
    classify_body_kinds(&mut ir)?;
    crate::native::attach_annotations(
        ctx,
        &mut ir,
        &model,
        scan,
        &mut annotations,
        &mut unknowns,
        &mut native_losses,
    )?;
    for (si, unknown_index) in stream_unknowns {
        retain_unknown_stream_data(ctx, &scan.streams[si], &mut unknowns[unknown_index])?;
    }
    prune_unreferenced_unknown_carriers(&mut ir);
    finalize_point_topology(&mut ir, &mut annotations)?;
    let mut referenced_pcurves: BTreeSet<PcurveId> = BTreeSet::new();
    for coedge in &ir.model.coedges {
        for pcurve in &coedge.pcurves {
            referenced_pcurves.insert(pcurve.pcurve.clone());
        }
    }
    ir.model
        .pcurves
        .retain(|pcurve| referenced_pcurves.contains(&pcurve.id));
    retain_live_unknown_links(&ir, &mut unknowns, &mut annotations)?;
    let mut annotations = annotations.build();
    retain_live_annotations(&ir, &unknowns, &mut annotations);
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

    let mut report = build_geometry_report(
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
        report.losses.extend(losses);
    }
    report_untransferred_streams(scan, &mut report, crate::native::TypedNative::Available)?;
    Ok(Some((ir, report, annotations, unknowns)))
}

fn extend_endpoint_witnesses(target: &mut EndpointWitnesses, source: EndpointWitnesses) {
    for (key, witnesses) in source {
        target.entry(key).or_default().extend(witnesses);
    }
}

fn copy_endpoint_witnesses(target: &mut EndpointWitnesses, source: &EndpointWitnesses) {
    for ((curve, surface), witnesses) in source {
        let key = (curve.clone(), surface.clone());

        let target_witnesses = match target.entry(key) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => entry.insert(Vec::new()),
        };

        for (geometry, range, endpoints) in witnesses {
            target_witnesses.push((geometry.clone(), *range, *endpoints));
        }
    }
}

fn prune_unreferenced_unknown_carriers(ir: &mut CadIr) {
    let mut used_surfaces: BTreeSet<SurfaceId> = BTreeSet::new();
    for face in &ir.model.faces {
        used_surfaces.insert(face.surface.clone());
    }
    let mut used_curves: BTreeSet<CurveId> = BTreeSet::new();
    for edge in &ir.model.edges {
        if let Some(curve) = edge.curve() {
            used_curves.insert(curve.clone());
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
                    used_surfaces.insert(support.clone());
                }
                ProceduralSurfaceDefinition::Blend(definition_payload) => {
                    let supports = definition_payload.supports();
                    let spine = definition_payload.spine();

                    for support in supports.iter().flatten() {
                        used_surfaces.insert(support.surface.clone());
                    }
                    if let Some(spine) = spine {
                        used_curves.insert(spine.clone());
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
                            used_surfaces.insert(surface.clone());
                        }
                    }
                }
                ProceduralCurveDefinition::SurfaceCurve { family } => {
                    for side in family.context().sides() {
                        if let Some(surface) = &side.surface {
                            used_surfaces.insert(surface.clone());
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
}

fn retain_live_annotations(
    ir: &CadIr,
    unknowns: &[UnknownRecord],
    annotations: &mut cadmpeg_ir::Annotations,
) {
    let mut ids = BTreeSet::new();
    macro_rules! add_ids {
        ($($arena:expr),+ $(,)?) => {
            $(for entity in &$arena {
                ids.insert(entity.id.as_str());
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
        ids.insert(unknown.id().as_str());
    }
    let mut live_ids = ids.iter().peekable();
    annotations.retain_provenance(|id| ordered_live_identity_contains(&mut live_ids, id));
    let mut builder = AnnotationBuilder::resume(std::mem::take(annotations));
    let mut live_ids = ids.iter().peekable();
    builder.retain_exactness(|id| ordered_live_identity_contains(&mut live_ids, id));
    *annotations = builder.build();
}

/// Annotation retention visits B-tree keys in order. Advance the live identity
/// cursor in that same order, without copying model identities or restarting
/// an indexed search for each annotation.
fn ordered_live_identity_contains(
    live_ids: &mut std::iter::Peekable<std::collections::btree_set::Iter<'_, &str>>,
    id: &str,
) -> bool {
    loop {
        let Some(&&live_id) = live_ids.peek() else {
            return false;
        };
        match live_id.cmp(id) {
            std::cmp::Ordering::Less => {
                live_ids.next();
            }
            std::cmp::Ordering::Equal => return true,
            std::cmp::Ordering::Greater => return false,
        }
    }
}

fn retain_live_unknown_links(
    ir: &CadIr,
    unknowns: &mut [UnknownRecord],
    annotations: &mut AnnotationBuilder,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut ids = BTreeSet::new();
    for entity in &ir.model.surfaces {
        ids.insert(entity.id.as_str().to_owned());
    }
    for entity in &ir.model.curves {
        ids.insert(entity.id.as_str().to_owned());
    }
    for entity in &ir.model.pcurves {
        ids.insert(entity.id.as_str().to_owned());
    }
    for entity in &ir.model.procedural_surfaces {
        ids.insert(entity.id.as_str().to_owned());
    }
    for entity in &ir.model.procedural_curves {
        ids.insert(entity.id.as_str().to_owned());
    }
    for unknown in unknowns.iter_mut() {
        unknown.links_mut().retain(|link| ids.contains(link));
        if !unknown.links().is_empty() {
            annotations.derived(unknown.id().as_str(), "links")?;
        }
    }
    Ok(())
}

pub(super) fn topology_body_node_ids(
    stream_index: usize,
    graph: &Graph,
) -> Result<BTreeMap<BodyId, BTreeSet<u32>>, CodecError> {
    let scope = IdScope::stream_from_index(stream_index)?;
    let mut body_xmts = BTreeSet::new();
    for shell in graph.body_shape_shells() {
        if let Some(body_xmt) = shell
            .shell_fields()
            .and_then(|fields| fields.body.map(u32::from))
        {
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
                shells.insert(shell.xmt());
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
                faces.push(face);
                face_xmts.insert(face.xmt());
            }
        }
        let mut loops = BTreeSet::new();
        for loop_ in graph.of_kind(NodeKind::Loop) {
            if loop_.loop_fields().is_some_and(|fields| {
                fields
                    .face
                    .is_some_and(|target| face_xmts.contains(&u32::from(target)))
            }) {
                loops.insert(loop_.xmt());
            }
        }
        let mut fins = Vec::new();
        for fin in graph.of_kind(NodeKind::Fin) {
            if fin.fin_fields().is_some_and(|fields| {
                fields
                    .loop_xmt
                    .is_some_and(|target| loops.contains(&u32::from(target)))
            }) {
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
            edge_xmts.insert(u32::from(edge));
            vertex_xmts.insert(u32::from(vertex));
        }
        let mut ids = BTreeSet::new();
        for face in faces {
            let Some(id) = face.u32_at(4) else {
                continue 'body;
            };
            ids.insert(id);
        }
        let mut edge_count = 0;
        for edge in graph.of_kind(NodeKind::Edge) {
            if edge_xmts.contains(&edge.xmt()) {
                edge_count += 1;
                let Some(id) = edge.u32_at(4) else {
                    continue 'body;
                };
                ids.insert(id);
            }
        }
        if edge_count != edge_xmts.len() {
            continue;
        }
        let mut vertex_count = 0;
        for vertex in graph.of_kind(NodeKind::Vertex) {
            if vertex_xmts.contains(&vertex.xmt()) {
                vertex_count += 1;
                let Some(id) = vertex.u32_at(4) else {
                    continue 'body;
                };
                ids.insert(id);
            }
        }
        if vertex_count != vertex_xmts.len() {
            continue;
        }
        bodies.insert(
            scope.id_from_display::<BodyId>(&cadmpeg_ir::identity_component!("body"), body_xmt)?,
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
    body_node_ids: &BTreeMap<BodyId, BTreeSet<u32>>,
    rmfastload_ids: &[u32],
) -> BTreeSet<BodyId> {
    let mut active = BTreeSet::new();
    for id in rmfastload_ids {
        active.insert(*id);
    }
    let mut selected = BTreeSet::new();
    for (body, ids) in body_node_ids {
        if !ids.is_empty() && ids.is_subset(&active) {
            selected.insert(body.clone());
        }
    }
    selected
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
pub(super) fn rmfastload_stream_indices(selected: &BTreeSet<BodyId>) -> Option<BTreeSet<usize>> {
    let mut streams = BTreeSet::new();
    for body in selected {
        let index = body
            .as_str()
            .strip_prefix("nx:s")
            .and_then(|text| text.split_once(':'))
            .and_then(|(text, _)| text.parse().ok())?;
        streams.insert(index);
    }
    Some(streams)
}

fn apply_preselected_active_body_selection(
    ir: &mut CadIr,
    selected: &BTreeSet<BodyId>,
    selector: &str,
    selected_hits: Option<usize>,
) -> bool {
    if selected.is_empty() {
        return false;
    }
    let mut emitted = BTreeSet::new();
    for body in &ir.model.bodies {
        emitted.insert(body.id.clone());
    }
    if !selected.is_subset(&emitted) {
        return false;
    }
    prune_inactive_topology(ir, selected);
    if let Some(source) = &mut ir.source {
        insert_body_selection_attribute(
            &mut source.attributes,
            cadmpeg_core::nonblank_literal!("active_body_selector"),
            selector,
        );
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
            insert_body_selection_attribute(&mut source.attributes, attribute, selected_hits);
        }
        insert_body_selection_attribute(&mut source.attributes, count_attribute, selected.len());
    }
    true
}

fn insert_body_selection_attribute(
    attributes: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    key: cadmpeg_core::text::NonBlankString,
    value: impl std::fmt::Display,
) {
    attributes.insert(key, value.to_string());
}

pub(super) fn select_active_body(
    ir: &mut CadIr,
    body_node_ids: &BTreeMap<BodyId, BTreeSet<u32>>,
    rmfastload_ids: &[u32],
) -> bool {
    if rmfastload_ids.is_empty() || ir.model.bodies.len() <= 1 {
        return false;
    }
    let selected = rmfastload_selected_bodies(body_node_ids, rmfastload_ids);
    if selected.is_empty() {
        return false;
    }
    let selected_hits = selected
        .iter()
        .filter_map(|body| body_node_ids.get(body))
        .map(BTreeSet::len)
        .sum::<usize>();
    apply_preselected_active_body_selection(
        ir,
        &selected,
        "rmfastload_object_id_membership",
        Some(selected_hits),
    )
}

fn select_terminal_feature_bodies(
    ir: &mut CadIr,
    model: &crate::native::model::NativeModel,
) -> bool {
    if ir.model.bodies.len() <= 1 {
        return false;
    }
    let mut emitted = BTreeSet::new();
    for body in &ir.model.bodies {
        emitted.insert(body.id.clone());
    }
    // A complete terminal mapping resolves composition even when every emitted
    // body is terminal. The absence of pruning is a valid result: it means the
    // retained body images are all final, not that lineage was unresolved.
    let Some(selected) = crate::native::model::terminal_feature_body_ids(
        &emitted,
        &model.segments.body_bindings,
        &model.segments.body_lineage_statuses,
    ) else {
        return false;
    };
    apply_preselected_active_body_selection(ir, &selected, "terminal_feature_body_lineage", None)
}

fn prune_inactive_topology(ir: &mut CadIr, selected: &BTreeSet<BodyId>) {
    ir.model.bodies.retain(|body| selected.contains(&body.id));
    ir.model
        .regions
        .retain(|region| selected.contains(&region.body));
    let mut regions: BTreeSet<RegionId> = BTreeSet::new();
    for region in &ir.model.regions {
        regions.insert(region.id.clone());
    }
    ir.model
        .shells
        .retain(|shell| regions.contains(&shell.region));
    let mut shells: BTreeSet<ShellId> = BTreeSet::new();
    for shell in &ir.model.shells {
        shells.insert(shell.id.clone());
    }
    ir.model.faces.retain(|face| shells.contains(&face.shell));
    let mut faces: BTreeSet<FaceId> = BTreeSet::new();
    for face in &ir.model.faces {
        faces.insert(face.id.clone());
    }
    ir.model.loops.retain(|loop_| faces.contains(&loop_.face));
    let mut loops: BTreeSet<LoopId> = BTreeSet::new();
    for loop_ in &ir.model.loops {
        loops.insert(loop_.id.clone());
    }
    ir.model
        .coedges
        .retain(|coedge| loops.contains(&coedge.owner_loop));
    let mut edges: BTreeSet<EdgeId> = BTreeSet::new();
    for coedge in &ir.model.coedges {
        edges.insert(coedge.edge.clone());
    }
    for shell in &ir.model.shells {
        for edge in shell.wire_edges() {
            edges.insert(edge.clone());
        }
    }
    ir.model.edges.retain(|edge| edges.contains(&edge.id));
    let mut vertices: BTreeSet<VertexId> = BTreeSet::new();
    for edge in &ir.model.edges {
        vertices.insert(edge.start.clone());
        vertices.insert(edge.end.clone());
    }
    for shell in &ir.model.shells {
        for vertex in shell.free_vertices() {
            vertices.insert(vertex.clone());
        }
    }
    ir.model
        .vertices
        .retain(|vertex| vertices.contains(&vertex.id));
    let mut points: BTreeSet<PointId> = BTreeSet::new();
    for vertex in &ir.model.vertices {
        points.insert(vertex.point.clone());
    }
    ir.model.points.retain(|point| points.contains(&point.id));
    prune_inactive_geometry(ir);
}

fn prune_inactive_geometry(ir: &mut CadIr) {
    let mut surfaces: BTreeSet<SurfaceId> = BTreeSet::new();
    for face in &ir.model.faces {
        surfaces.insert(face.surface.clone());
    }
    let mut curves: BTreeSet<CurveId> = BTreeSet::new();
    for edge in &ir.model.edges {
        if let Some(curve) = edge.curve() {
            curves.insert(curve.clone());
        }
    }
    let mut pcurves: BTreeSet<PcurveId> = BTreeSet::new();
    for coedge in &ir.model.coedges {
        for pcurve in &coedge.pcurves {
            pcurves.insert(pcurve.pcurve.clone());
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
                    surfaces.insert(support.clone());
                }
                ProceduralSurfaceDefinition::Blend(definition_payload) => {
                    let supports = definition_payload.supports();
                    let spine = definition_payload.spine();

                    for support in supports.iter().flatten() {
                        surfaces.insert(support.surface.clone());
                    }
                    if let Some(spine) = spine {
                        curves.insert(spine.clone());
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
                            surfaces.insert(surface.clone());
                        }
                    }
                }
                ProceduralCurveDefinition::SurfaceCurve { family } => {
                    for side in family.context().sides() {
                        if let Some(surface) = &side.surface {
                            surfaces.insert(surface.clone());
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
                surface_constructions.insert(construction.clone());
            }
        }
    }
    let mut curve_constructions: BTreeSet<ProceduralCurveId> = BTreeSet::new();
    for curve in &ir.model.curves {
        if curves.contains(&curve.id) {
            if let Some(construction) = curve.geometry.procedural_construction() {
                curve_constructions.insert(construction.clone());
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
}

fn finalize_point_topology(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    let mut referenced_points: BTreeSet<PointId> = BTreeSet::new();
    for vertex in &ir.model.vertices {
        referenced_points.insert(vertex.point.clone());
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
        derived.id_from_display(&cadmpeg_ir::identity_component!("point-body"), 0)?;
    let region_id: RegionId =
        derived.id_from_display(&cadmpeg_ir::identity_component!("point-region"), 0)?;
    let shell_id: ShellId =
        derived.id_from_display(&cadmpeg_ir::identity_component!("point-shell"), 0)?;
    let stream = StreamHandle::new(cadmpeg_ir::stream_name!("nx:container"));
    for id in [body_id.as_str(), region_id.as_str(), shell_id.as_str()] {
        annotations.note(id, &stream, 0, Some("derived_point_topology"));
        annotations.exactness(id, Exactness::Inferred);
    }

    let mut free_vertices = Vec::new();

    for (index, point) in ir.model.points.iter().enumerate() {
        let vertex_id: VertexId =
            derived.id_from_display(&cadmpeg_ir::identity_component!("point-vertex"), index)?;
        annotations.note(
            vertex_id.as_str(),
            &stream,
            0,
            Some("derived_point_topology"),
        );
        annotations.exactness(vertex_id.as_str(), Exactness::Inferred);
        ir.model.vertices.push(Vertex {
            id: vertex_id.clone(),
            point: point.id.clone(),
            tolerance: None,
        });
        free_vertices.push(vertex_id);
    }

    ir.model.shells.push(
        match Shell::new(
            shell_id.clone(),
            region_id.clone(),
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

    let region_shells = vec![shell_id];
    ir.model.regions.push(Region {
        id: region_id.clone(),
        body: body_id.clone(),
        shells: region_shells,
    });

    let body_regions = vec![region_id];
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

fn classify_body_kinds(ir: &mut CadIr) -> Result<(), CodecError> {
    let mut region_bodies: BTreeMap<RegionId, BodyId> = BTreeMap::new();
    for region in &ir.model.regions {
        region_bodies.insert(region.id.clone(), region.body.clone());
    }
    let mut shell_bodies: BTreeMap<ShellId, BodyId> = BTreeMap::new();
    for shell in &ir.model.shells {
        if let Some(body) = region_bodies.get(&shell.region) {
            shell_bodies.insert(shell.id.clone(), body.clone());
        }
    }
    let mut face_bodies: BTreeMap<FaceId, BodyId> = BTreeMap::new();
    for face in &ir.model.faces {
        if let Some(body) = shell_bodies.get(&face.shell) {
            face_bodies.insert(face.id.clone(), body.clone());
        }
    }
    let mut loop_bodies: BTreeMap<LoopId, BodyId> = BTreeMap::new();
    for loop_ in &ir.model.loops {
        if let Some(body) = face_bodies.get(&loop_.face) {
            loop_bodies.insert(loop_.id.clone(), body.clone());
        }
    }
    let mut coedge_bodies: BTreeMap<CoedgeId, BodyId> = BTreeMap::new();
    for coedge in &ir.model.coedges {
        if let Some(body) = loop_bodies.get(&coedge.owner_loop) {
            coedge_bodies.insert(coedge.id.clone(), body.clone());
        }
    }
    let mut edge_uses = BTreeMap::<BodyId, BTreeMap<EdgeId, usize>>::new();
    for coedge in &ir.model.coedges {
        let Some(body) = coedge_bodies.get(&coedge.id) else {
            continue;
        };
        if !edge_uses.contains_key(body) {
            edge_uses.insert(body.clone(), BTreeMap::new());
        }
        let Some(edges) = edge_uses.get_mut(body) else {
            return Err(CodecError::malformed(
                "NX body classification owner missing",
            ));
        };
        if !edges.contains_key(&coedge.edge) {
            edges.insert(coedge.edge.clone(), 0);
        }
        let Some(count) = edges.get_mut(&coedge.edge) else {
            return Err(CodecError::malformed("NX body classification edge missing"));
        };
        *count += 1;
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
