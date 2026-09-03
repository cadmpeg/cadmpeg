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
use crate::geometry;
use crate::topology::{Graph, Node};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::DecodeBody;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, BlendSupport, Curve, CurveGeometry, IntcurveSupportContext,
    NurbsCurve, Pcurve, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CurveId, EdgeId, PcurveId, PointId, ProceduralCurveId, ProceduralSurfaceId, RegionId,
    ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::{Body, BodyKind, Point, Region, Shell, Vertex};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn ordered_point_candidates<'a>(
    stream: &[u8],
    graph: &'a Graph,
) -> Vec<(Point3, &'a Node)> {
    ordered_fixed_candidates(
        geometry::points(stream)
            .into_iter()
            .map(|point| (point.pos, point.position)),
        graph,
        29..=29,
        Node::point_position,
    )
}

pub(crate) fn ordered_surface_candidates<'a>(
    stream: &[u8],
    graph: &'a Graph,
) -> Vec<(SurfaceGeometry, &'a Node)> {
    ordered_fixed_candidates(
        geometry::surfaces(stream)
            .into_iter()
            .map(|surface| (surface.pos, surface.geometry)),
        graph,
        50..=54,
        Node::surface_geometry,
    )
}

pub(crate) fn ordered_curve_candidates<'a>(
    stream: &[u8],
    graph: &'a Graph,
) -> Vec<(CurveGeometry, &'a Node)> {
    ordered_fixed_candidates(
        geometry::curves(stream)
            .into_iter()
            .map(|curve| (curve.pos, curve.geometry)),
        graph,
        30..=32,
        Node::curve_geometry,
    )
}

pub(crate) fn ordered_fixed_candidates<T>(
    fallback: impl IntoIterator<Item = (usize, T)>,
    graph: &Graph,
    kinds: std::ops::RangeInclusive<u8>,
    graph_value: impl Fn(&Node) -> Option<T>,
) -> Vec<(T, &Node)> {
    let mut candidates = BTreeMap::new();
    for (offset, value) in fallback {
        let Some(node) = graph
            .at_pos(offset)
            .filter(|node| graph_value(node).is_some())
        else {
            continue;
        };
        candidates.insert(offset, (value, node));
    }
    for node in kinds.flat_map(|kind| graph.of_kind(kind)) {
        if let Some(value) = graph_value(node) {
            candidates.insert(node.pos, (value, node));
        }
    }
    candidates.into_values().collect()
}

/// Decode analytic carriers from every Parasolid stream. Returns `None` when no
/// carrier of any kind passes its gate, so the caller falls back to metadata.
pub(crate) type GeometryDecode = (
    CadIr,
    DecodeBody,
    cadmpeg_ir::Annotations,
    Vec<UnknownRecord>,
);

pub(crate) fn try_decode_geometry(
    ctx: &DecodeContext<'_>,
    root: View<'_>,
    scan: &Scan,
    dialects: &DialectLayers,
    dialect_losses: &[LossNote],
    notes: &[String],
    admitted_entities: &mut u64,
) -> Result<Option<GeometryDecode>, CodecError> {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    let mut stream_unknowns = Vec::new();
    let mut counts = Counts::default();
    let mut body_node_ids = BTreeMap::new();
    let mut parsed = crate::native::ParsedStreams::parse(scan);
    let rmfastload_ids = scan
        .container
        .rmfastload_object_id_table()
        .map(|(_, table)| {
            table
                .object_ids
                .into_iter()
                .map(|object_id| object_id.value)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for (si, stream) in scan.streams.iter().enumerate() {
        if stream.kind.is_parasolid() {
            body_node_ids.extend(topology_body_node_ids(
                si,
                &parsed.stream(si).view_for_geometry().graph,
            ));
        }
    }
    let rmfastload_selected = rmfastload_selected_bodies(&body_node_ids, &rmfastload_ids);
    let rmfastload_preselection = (body_node_ids.len() > 1
        && !rmfastload_selected.is_empty()
        && rmfastload_selected.len() < body_node_ids.len())
    .then(|| {
        rmfastload_stream_indices(&rmfastload_selected).map(|streams| {
            (
                rmfastload_selected.clone(),
                streams,
                "rmfastload_object_id_membership",
            )
        })
    })
    .flatten();
    let terminal_lineage =
        rmfastload_allows_terminal_lineage(body_node_ids.len(), &rmfastload_selected)
            .then(|| crate::native::extract_segment_lineage(&scan.container, &scan.streams));
    let emitted_body_ids = body_node_ids.keys().cloned().collect::<BTreeSet<_>>();
    let terminal_preselection = terminal_lineage
        .as_ref()
        .and_then(|lineage| {
            crate::native::terminal_feature_body_ids(
                &emitted_body_ids,
                &lineage.bindings,
                &lineage.statuses,
            )
        })
        .filter(|selected| selected.len() < body_node_ids.len())
        .and_then(|selected| {
            rmfastload_stream_indices(&selected)
                .map(|streams| (selected, streams, "terminal_feature_body_lineage"))
        });
    let preselection = rmfastload_preselection.or(terminal_preselection);
    let chart_count = scan
        .streams
        .iter()
        .enumerate()
        .filter(|(si, stream)| {
            stream.kind.is_parasolid()
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
        GeometryWorkBudget::from_work_budget(ctx.work_budget(MAX_ADAPTIVE_GEOMETRY_WORK as u64));
    let completion_geometry_budget = GeometryWorkBudget::from_work_budget(
        ctx.work_budget(MAX_PCURVE_COMPLETION_GEOMETRY_WORK as u64),
    );
    let support_uv_geometry_budget = GeometryWorkBudget::from_work_budget(
        ctx.work_budget(MAX_SUPPORT_UV_COMPLETION_GEOMETRY_WORK as u64),
    );
    let coupled_support_uv_geometry_budget = GeometryWorkBudget::from_work_budget(
        ctx.work_budget(MAX_COUPLED_SUPPORT_UV_GEOMETRY_WORK as u64),
    );
    let serialized_support_uv_geometry_budget = GeometryWorkBudget::from_work_budget(
        ctx.work_budget(MAX_SERIALIZED_SUPPORT_UV_GEOMETRY_WORK as u64),
    );
    let mut support_uv_lane_geometry_exhausted = false;
    let mut intersection_index = IntersectionIncidenceIndex::default();
    let mut model_endpoint_witnesses = EndpointWitnesses::new();
    let mut completion_streams = Vec::new();

    for (si, stream) in scan.streams.iter().enumerate() {
        if !stream.kind.is_parasolid() {
            continue;
        }
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
            let unknown = unknown_stream_metadata(si, stream);
            let container_stream = annotations.stream("nx:container");
            annotations
                .note(unknown.id(), container_stream, stream.file_offset as u64)
                .tag(stream.kind.label());
            annotations.exactness(unknown.id(), Exactness::Derived);
            unknowns.push(unknown);
            stream_unknowns.push((si, unknown_index));
            continue;
        }
        let crate::nurbs::Parsed {
            surfaces: nurbs_surfaces,
            curves: nurbs_curves,
            pcurves: nurbs_pcurves,
        } = parsed.take_nurbs(si);
        let view = parsed.stream(si).view_for_geometry();
        let semantic = parsed.semantic_bytes(si);
        let stream_name = format!("parasolid#{si}:{}", stream.kind.label());
        let source_stream = annotations.stream(format!("nx:{stream_name}"));
        completion_streams.push((si, source_stream));
        let graph = &view.graph;
        let mut points_by_xmt = BTreeMap::new();
        let mut surfaces_by_xmt = BTreeMap::new();
        let mut curves_by_xmt = BTreeMap::new();
        let mut pcurves_by_xmt = BTreeMap::new();
        let mut pcurve_supports_by_xmt = BTreeMap::new();
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
        for (pi, (position, node)) in ordered_point_candidates(semantic, graph)
            .into_iter()
            .enumerate()
        {
            let pid = PointId(format!("nx:s{si}:pt#{pi}"));
            let vid = VertexId(format!("nx:s{si}:v#{pi}"));
            annotate_node(&mut annotations, &pid, source_stream, node, "POINT");
            annotations.derived(&pid, "position");
            ir.model.points.push(Point {
                id: pid.clone(),
                position,
                source_object: None,
            });
            ir.model.vertices.push(Vertex {
                id: vid.clone(),
                point: pid.clone(),
                tolerance: None,
            });
            points_by_xmt.insert(node.xmt, pid);
            counts.points += 1;
        }
        for (fi, (geometry, node)) in ordered_surface_candidates(semantic, graph)
            .into_iter()
            .enumerate()
        {
            match &geometry {
                SurfaceGeometry::Plane { .. } => counts.planes += 1,
                SurfaceGeometry::Cylinder { .. } => counts.cylinders += 1,
                SurfaceGeometry::Cone { .. } => counts.cones += 1,
                SurfaceGeometry::Sphere { .. } => counts.spheres += 1,
                SurfaceGeometry::Torus { .. } => counts.tori += 1,
                SurfaceGeometry::Nurbs(_)
                | SurfaceGeometry::Procedural { .. }
                | SurfaceGeometry::Polygonal { .. }
                | SurfaceGeometry::Transformed { .. }
                | SurfaceGeometry::Unknown { .. } => {}
            }
            let id = SurfaceId(format!("nx:s{si}:surf#{fi}"));
            annotate_node(
                &mut annotations,
                &id,
                source_stream,
                node,
                surface_tag(&geometry),
            );
            annotations.derived(&id, "geometry");
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry,
                source_object: None,
            });
            surfaces_by_xmt.insert(node.xmt, id);
        }
        for (fi, surf) in nurbs_surfaces.into_iter().enumerate() {
            counts.nurbs_surfaces += 1;
            let id = SurfaceId(format!("nx:s{si}:nurbs-surf#{fi}"));
            annotations
                .note(&id, source_stream, surf.pos as u64)
                .tag("B_SPLINE_SURFACE");
            annotations.derived(&id, "geometry");
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry: surf.geometry,
                source_object: None,
            });
            if let Some(node) = graph.at_pos(surf.pos) {
                surfaces_by_xmt.insert(node.xmt, id);
            }
        }
        let saved_offset_carriers = saved_offset_carriers(
            &ir,
            graph,
            &view.offset_surfaces,
            &surfaces_by_xmt,
            ir.tolerances.linear,
            &adaptive_geometry_budget,
        );
        for (oi, offset) in view.offset_surfaces.iter().copied().enumerate() {
            let Some(support) = surfaces_by_xmt.get(&offset.support).cloned() else {
                continue;
            };
            let procedural_id = ProceduralSurfaceId(format!("nx:s{si}:offset#{oi}"));
            let (surface_id, cache_fit_tolerance) =
                if let Some((surface, fit_tolerance)) = saved_offset_carriers.get(&offset.xmt) {
                    (surface.clone(), Some(*fit_tolerance))
                } else {
                    let surface_id = SurfaceId(format!("nx:s{si}:offset-surf#{oi}"));
                    annotations
                        .note(&surface_id, source_stream, offset.pos as u64)
                        .tag("OFFSET_SURF");
                    annotations.derived(&surface_id, "geometry");
                    ir.model.surfaces.push(Surface {
                        id: surface_id.clone(),
                        geometry: SurfaceGeometry::Procedural {
                            construction: procedural_id.clone(),
                        },
                        source_object: Some(SourceObjectAssociation {
                            format: "nx".into(),
                            object_id: format!("nx:s{si}:offset-surface-record#{}", offset.xmt),
                            name: None,
                            color: None,
                            visible: None,
                            layer: None,
                            instance_path: Vec::new(),
                        }),
                    });
                    (surface_id, None)
                };
            annotations
                .note(&procedural_id, source_stream, offset.pos as u64)
                .tag("OFFSET_SURF");
            annotations.derived(&procedural_id, "definition");
            if let Ok(procedural) = ProceduralSurface::try_new(
                procedural_id,
                surface_id.clone(),
                ProceduralSurfaceDefinition::Offset {
                    support,
                    distance: offset.distance,
                    // OFFSET_SURF status fields do not select parameter direction.
                    u_sense: None,
                    v_sense: None,
                    support_extension: Some(cadmpeg_ir::geometry::OffsetSupportExtension::Linear),
                    extension_flags: Vec::new(),
                    revision_form: None,
                },
                cache_fit_tolerance,
                None,
            ) {
                ir.model.procedural_surfaces.push(procedural);
            }
            surfaces_by_xmt.insert(offset.xmt, surface_id);
            counts.offset_surfaces += 1;
        }

        for (bi, blend) in view.blend_surfaces.iter().copied().enumerate() {
            let surface_id = SurfaceId(format!("nx:s{si}:blend-surf#{bi}"));
            let procedural_id = ProceduralSurfaceId(format!("nx:s{si}:blend#{bi}"));
            annotations
                .note(&surface_id, source_stream, blend.pos as u64)
                .tag("BLEND_SURF");
            annotations.derived(&surface_id, "geometry");
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Procedural {
                    construction: procedural_id.clone(),
                },
                source_object: Some(SourceObjectAssociation {
                    format: "nx".to_string(),
                    object_id: format!("nx:s{si}:blend-surface-record#{}", blend.xmt),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            annotations
                .note(&procedural_id, source_stream, blend.pos as u64)
                .tag("BLEND_SURF");
            annotations.derived(&procedural_id, "definition");
            let procedural_index = ir.model.procedural_surfaces.len();
            ir.model.procedural_surfaces.push(ProceduralSurface::new(
                procedural_id,
                surface_id.clone(),
                ProceduralSurfaceDefinition::Blend {
                    supports: [None, None],
                    spine: None,
                    radius: BlendRadiusLaw::Constant {
                        signed_radius: blend.offsets[0],
                    },
                    cross_section: BlendCrossSection::Circular,
                    native: None,
                },
                None,
            ));
            pending_blend_supports.push((procedural_index, blend.supports, blend.offsets));
            if blend.spine > 1 {
                pending_blend_spines.push((procedural_index, blend.spine));
            }
            surfaces_by_xmt.insert(blend.xmt, surface_id);
            counts.blend_surfaces += 1;
        }
        for (procedural_index, support_xmts, offsets) in pending_blend_supports {
            let supports = [0, 1].map(|side| {
                surfaces_by_xmt
                    .get(&support_xmts[side])
                    .cloned()
                    .map(|surface| BlendSupport {
                        surface,
                        reversed: offsets[side].is_sign_negative(),
                    })
            });
            let Some(procedural) = ir.model.procedural_surfaces.get_mut(procedural_index) else {
                continue;
            };
            procedural.edit_definition(|definition| {
                if let ProceduralSurfaceDefinition::Blend {
                    supports: slots, ..
                } = definition
                {
                    *slots = supports;
                }
            });
        }

        for (ci, (geometry, node)) in ordered_curve_candidates(semantic, graph)
            .into_iter()
            .enumerate()
        {
            match &geometry {
                CurveGeometry::Line { .. } => counts.lines += 1,
                CurveGeometry::Circle { .. } => counts.circles += 1,
                CurveGeometry::Ellipse { .. } => counts.ellipses += 1,
                CurveGeometry::Parabola { .. }
                | CurveGeometry::Hyperbola { .. }
                | CurveGeometry::Degenerate { .. }
                | CurveGeometry::Composite { .. }
                | CurveGeometry::Nurbs(_)
                | CurveGeometry::Procedural { .. }
                | CurveGeometry::Polyline { .. }
                | CurveGeometry::Transformed { .. }
                | CurveGeometry::Unknown { .. } => {}
            }
            let id = CurveId(format!("nx:s{si}:crv#{ci}"));
            annotate_node(
                &mut annotations,
                &id,
                source_stream,
                node,
                curve_tag(&geometry),
            );
            annotations.derived(&id, "geometry");
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry,
                source_object: None,
            });
            curves_by_xmt.insert(node.xmt, id);
        }
        for (ci, crv) in nurbs_curves.into_iter().enumerate() {
            counts.nurbs_curves += 1;
            let id = CurveId(format!("nx:s{si}:nurbs-crv#{ci}"));
            annotations
                .note(&id, source_stream, crv.pos as u64)
                .tag("B_SPLINE_CURVE");
            annotations.derived(&id, "geometry");
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry: crv.geometry,
                source_object: None,
            });
            if let Some(node) = graph.at_pos(crv.pos) {
                curves_by_xmt.insert(node.xmt, id);
            }
        }

        for (pi, pcurve) in nurbs_pcurves.into_iter().enumerate() {
            let id = PcurveId(format!("nx:s{si}:pcurve#{pi}"));
            annotations
                .note(&id, source_stream, pcurve.pos as u64)
                .tag("B_CURVE_2D");
            annotations.derived(&id, "geometry");
            ir.model.pcurves.push(Pcurve {
                id: id.clone(),
                geometry: pcurve.geometry,
                wrapper_reversed: None,
                native_tail_flags: None,
                parameter_range: None,
                fit_tolerance: None,
            });
            if let Some(node) = graph.at_pos(pcurve.pos) {
                pcurves_by_xmt.insert(node.xmt, id);
            }
        }
        let intersection_scan = view.intersections.clone();
        counts
            .intersection_rejections
            .extend(intersection_scan.rejected);
        let intersection_constructions = intersection_scan.constructions;
        let charted_intersections: BTreeMap<_, _> = intersection_scan
            .curves
            .into_iter()
            .map(|curve| (curve.xmt, curve))
            .collect();
        let uncharted_intersections: BTreeMap<_, _> = intersection_scan
            .uncharted
            .into_iter()
            .map(|curve| (curve.xmt, curve))
            .collect();
        let intersection_support_uv = {
            let model_index = cadmpeg_ir::index::ModelIndex::new_model_only(&ir);
            intersection_constructions
                .iter()
                .filter_map(|construction| {
                    let charted = charted_intersections.get(&construction.xmt)?;
                    let mut support_uv = validate_serialized_support_uv_with_index(
                        &model_index,
                        &surfaces_by_xmt,
                        charted.supports,
                        &charted.points,
                        charted.fit_tolerance,
                        &charted.support_uv,
                        &serialized_support_uv_geometry_budget,
                    );
                    if let Some(ext_support_uv) = assign_ext11_support_uv_with_index(
                        &model_index,
                        &surfaces_by_xmt,
                        charted.supports,
                        &charted.points,
                        charted.fit_tolerance,
                        &charted.ext_support_uv,
                        &serialized_support_uv_geometry_budget,
                    ) {
                        for side in 0..2 {
                            if support_uv[side].is_none() {
                                support_uv[side].clone_from(&ext_support_uv[side]);
                            }
                        }
                    }
                    Some((construction.xmt, support_uv))
                })
                .collect::<BTreeMap<_, _>>()
        };
        for (ci, construction) in intersection_constructions.into_iter().enumerate() {
            let curve_id = CurveId(format!("nx:s{si}:intersection-crv#{ci}"));
            let procedural_id = ProceduralCurveId(format!("nx:s{si}:intersection#{ci}"));
            let unknown_id = UnknownId(format!("nx:container:parasolid#{si}"));
            let charted = charted_intersections.get(&construction.xmt);
            let uncharted = uncharted_intersections
                .get(&construction.xmt)
                .and_then(|uncharted| {
                    let supports = uncharted
                        .supports
                        .each_ref()
                        .map(|xmt| surfaces_by_xmt.get(xmt).cloned());
                    let [Some(first), Some(second)] = supports else {
                        return None;
                    };
                    (first != second).then_some((
                        [first, second],
                        uncharted.endpoints,
                        uncharted.tolerance * 1000.0,
                    ))
                });
            if let Some(charted) = charted {
                if let Some(support_uv) = intersection_support_uv.get(&construction.xmt) {
                    for (side, lane) in support_uv.iter().enumerate() {
                        if lane.is_some() {
                            validated_support_uv_lanes.insert((procedural_id.clone(), side));
                        }
                    }
                }
                pending_ext11_support_uv.push((
                    procedural_id.clone(),
                    charted.points.clone(),
                    charted.parameters.clone(),
                    charted.fit_tolerance,
                    SerializedSupportUv {
                        values: charted.support_uv.clone(),
                        ext11: charted.ext_support_uv.clone(),
                    },
                ));
            }
            annotations
                .note(&curve_id, source_stream, construction.pos as u64)
                .tag("INTERSECTION");
            if charted.is_some() || uncharted.is_some() {
                annotations.derived(&curve_id, "geometry");
            } else {
                annotations.exactness(&curve_id, Exactness::Unknown);
            }
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: if let Some(charted) = charted {
                    CurveGeometry::Nurbs(NurbsCurve {
                        degree: 1,
                        knots: linear_knots(&charted.parameters),
                        control_points: charted.points.clone(),
                        weights: None,
                        periodic: false,
                    })
                } else if uncharted.is_some() {
                    CurveGeometry::Procedural {
                        construction: procedural_id.clone(),
                    }
                } else {
                    CurveGeometry::Unknown {
                        record: Some(unknown_id.clone()),
                    }
                },
                source_object: Some(SourceObjectAssociation {
                    format: "nx".into(),
                    object_id: format!("nx:s{si}:intersection-record#{}", construction.xmt),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            annotations
                .note(&procedural_id, source_stream, construction.pos as u64)
                .tag("INTERSECTION");
            if charted.is_some() || uncharted.is_some() {
                annotations.derived(&procedural_id, "definition");
            } else {
                annotations.exactness(&procedural_id, Exactness::Unknown);
            }
            let definition = if let Some(charted) = charted {
                let support_uv = intersection_support_uv
                    .get(&construction.xmt)
                    .cloned()
                    .unwrap_or([None, None]);
                let first = intersection_side(
                    &ir,
                    &surfaces_by_xmt,
                    charted.supports[0],
                    support_uv[0]
                        .as_deref()
                        .filter(|uv| uv.len() == charted.parameters.len())
                        .map(|uv| (uv, charted.parameters.as_slice())),
                );
                let second = intersection_side(
                    &ir,
                    &surfaces_by_xmt,
                    charted.supports[1],
                    support_uv[1]
                        .as_deref()
                        .filter(|uv| uv.len() == charted.parameters.len())
                        .map(|uv| (uv, charted.parameters.as_slice())),
                );
                ProceduralCurveDefinition::Intersection {
                    context: IntcurveSupportContext {
                        sides: [first, second],
                        parameter_range: [
                            charted.parameters[0],
                            *charted
                                .parameters
                                .last()
                                .expect("validated chart has points"),
                        ],
                        discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                    },
                    discontinuity_flag: false,
                }
            } else if let Some((supports, endpoints, tolerance)) = uncharted {
                ProceduralCurveDefinition::TolerantIntersection {
                    supports,
                    endpoints,
                    tolerance,
                    parameterization: None,
                }
            } else {
                ProceduralCurveDefinition::Unknown {
                    native_kind: Some("nx:intersection".into()),
                    record: Some(unknown_id),
                }
            };
            if let Ok(procedural) = ProceduralCurve::try_new(
                procedural_id,
                curve_id.clone(),
                definition,
                charted.map(|charted| charted.fit_tolerance),
            ) {
                ir.model.procedural_curves.push(procedural);
            }
            curves_by_xmt.insert(construction.xmt, curve_id);
            counts.intersection_curves += 1;
        }
        for (procedural_index, spine_xmt) in pending_blend_spines {
            let Some(spine) = curves_by_xmt.get(&spine_xmt).cloned() else {
                continue;
            };
            let Some(procedural) = ir.model.procedural_surfaces.get_mut(procedural_index) else {
                continue;
            };
            procedural.edit_definition(|definition| {
                if let ProceduralSurfaceDefinition::Blend { spine: slot, .. } = definition {
                    *slot = Some(spine);
                }
            });
        }
        let trimmed_curves = &view.trimmed_curves;
        let mut normalized_pcurves = BTreeSet::new();
        let mut invalid_pcurves = BTreeSet::new();
        let curve_indices: BTreeMap<_, _> = ir
            .model
            .curves
            .iter()
            .enumerate()
            .map(|(index, curve)| (curve.id.clone(), index))
            .collect();
        let surface_indices: BTreeMap<_, _> = ir
            .model
            .surfaces
            .iter()
            .enumerate()
            .map(|(index, surface)| (surface.id.clone(), index))
            .collect();
        let pcurve_indices: BTreeMap<_, _> = ir
            .model
            .pcurves
            .iter()
            .enumerate()
            .map(|(index, pcurve)| (pcurve.id.clone(), index))
            .collect();
        let surface_curves = &view.surface_curves;
        loop {
            let mapped = curves_by_xmt.len() + pcurves_by_xmt.len() + pcurve_supports_by_xmt.len();
            for trim in trimmed_curves {
                if let Some(basis) = curves_by_xmt.get(&trim.basis).cloned() {
                    let parameters = curve_indices
                        .get(&basis)
                        .and_then(|index| ir.model.curves.get(*index))
                        .and_then(|curve| canonical_trim_range(&curve.geometry, trim.parameters));
                    curves_by_xmt.insert(trim.xmt, basis);
                    if let Some(parameters) = parameters {
                        trim_ranges.insert(trim.xmt, parameters);
                    }
                }
                if let Some(pcurve) = pcurves_by_xmt.get(&trim.basis).cloned() {
                    pcurves_by_xmt.insert(trim.xmt, pcurve);
                    if let Some(support) = pcurve_supports_by_xmt.get(&trim.basis).cloned() {
                        pcurve_supports_by_xmt.insert(trim.xmt, support);
                    }
                    trim_ranges.insert(trim.xmt, trim.parameters);
                }
            }
            for surface_curve in surface_curves {
                if let Some(pcurve) = pcurves_by_xmt.get(&surface_curve.pcurve).cloned() {
                    if !normalized_pcurves.contains(&pcurve) {
                        let support = surfaces_by_xmt
                            .get(&surface_curve.surface)
                            .and_then(|id| surface_indices.get(id))
                            .and_then(|index| ir.model.surfaces.get(*index))
                            .map(|surface| surface.geometry.clone());
                        let normalized = if let (Some(support), Some(carrier)) = (
                            support,
                            pcurve_indices
                                .get(&pcurve)
                                .and_then(|index| ir.model.pcurves.get_mut(*index)),
                        ) {
                            normalize_pcurve_parameters(&mut carrier.geometry, &support).is_some()
                        } else {
                            false
                        };
                        if !normalized {
                            pcurves_by_xmt.remove(&surface_curve.pcurve);
                            invalid_pcurves.insert(pcurve.clone());
                            continue;
                        }
                        normalized_pcurves.insert(pcurve.clone());
                    }
                    if let Some(carrier) = pcurve_indices
                        .get(&pcurve)
                        .and_then(|index| ir.model.pcurves.get_mut(*index))
                    {
                        carrier.fit_tolerance = decoded_tolerance(surface_curve.tolerance);
                    }
                    pcurves_by_xmt.insert(surface_curve.xmt, pcurve);
                    if let Some(support) = surfaces_by_xmt.get(&surface_curve.surface).cloned() {
                        pcurve_supports_by_xmt.insert(surface_curve.xmt, support);
                    }
                }
                if let Some(original) = curves_by_xmt.get(&surface_curve.original).cloned() {
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
            si,
            graph,
            &mut surfaces_by_xmt,
            &mut curves_by_xmt,
            &pcurves_by_xmt,
            source_stream,
            &mut annotations,
        );
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
            &mut ir,
            si,
            graph,
            &points_by_xmt,
            &surfaces_by_xmt,
            &curves_by_xmt,
            &pcurves_by_xmt,
            &pcurve_supports_by_xmt,
            &trim_ranges,
            source_stream,
            &mut annotations,
            &mut intersection_index,
            intersection_starts,
            procedural_start,
            &exact_transfer_budget,
            &transfer_budget,
            &adaptive_geometry_budget,
            &completion_geometry_budget,
        );
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
                &mut ir,
                &pending_ext11_support_uv,
                &validated_support_uv_lanes,
                &support_uv_validation_budget,
                &completion_geometry_budget,
                isolate_validation_lanes,
            );
        support_uv_lane_geometry_exhausted |= support_uv_validation.lane_geometry_exhausted;
        let newly_validated_endpoint_witnesses = support_uv_validation.endpoint_witnesses;
        serialized_support_uv_geometry_budget.clear_blend_frame_cache();
        complete_ext11_support_uv_with_budget(
            &mut ir,
            &pending_ext11_support_uv,
            &serialized_support_uv_geometry_budget,
        );
        support_uv_geometry_budget.clear_blend_frame_cache();
        coupled_support_uv_geometry_budget.clear_blend_frame_cache();
        complete_parameterization_equivalent_support_uv(&mut ir);
        let mut completed_endpoint_witnesses = BTreeMap::new();
        support_uv_lane_geometry_exhausted |=
            complete_support_uv_with_budget_and_endpoint_witnesses(
                &mut ir,
                &pending_ext11_support_uv,
                &support_budget,
                &support_uv_geometry_budget,
                &coupled_support_budget,
                &coupled_support_uv_geometry_budget,
                &mut completed_endpoint_witnesses,
            );
        let mut validated_endpoint_witnesses = initial_endpoint_witnesses;
        for (key, witnesses) in validated_support_uv_endpoint_witnesses(
            &ir,
            &pending_ext11_support_uv,
            &validated_support_uv_lanes,
        ) {
            validated_endpoint_witnesses
                .entry(key)
                .or_default()
                .extend(witnesses);
        }
        for (key, witness) in newly_validated_endpoint_witnesses {
            validated_endpoint_witnesses
                .entry(key)
                .or_default()
                .extend(witness);
        }
        for (key, witnesses) in completed_endpoint_witnesses {
            validated_endpoint_witnesses
                .entry(key)
                .or_default()
                .extend(witnesses);
        }
        for (key, witnesses) in &validated_endpoint_witnesses {
            model_endpoint_witnesses
                .entry(key.clone())
                .or_default()
                .extend(witnesses.iter().cloned());
        }
        attach_completed_intersection_pcurves_for_stream_with_budget(
            &mut ir,
            graph,
            &format!("nx:s{si}"),
            intersection_starts.coedges,
            intersection_starts.procedural_curves,
            source_stream,
            &mut annotations,
            &validated_endpoint_witnesses,
            &completion_geometry_budget,
        );
        // Preserve the whole inflated stream verbatim so nothing is dropped.
        let unknown_index = unknowns.len();
        let mut unknown = unknown_stream_metadata(si, stream);
        unknown.links_mut().extend(
            ir.model.surfaces[first_surface..]
                .iter()
                .map(|surface| surface.id.0.clone()),
        );
        unknown.links_mut().extend(
            ir.model.curves[first_curve..]
                .iter()
                .map(|curve| curve.id.0.clone()),
        );
        let container_stream = annotations.stream("nx:container");
        annotations
            .note(unknown.id(), container_stream, stream.file_offset as u64)
            .tag(stream.kind.label());
        annotations.exactness(unknown.id(), Exactness::Derived);
        unknowns.push(unknown);
        stream_unknowns.push((si, unknown_index));
    }

    intersection_index.complete_from_model(&mut ir);
    let completion_sources = completion_streams
        .iter()
        .map(|(si, source_stream)| IntersectionCompletionSource {
            prefix: format!("nx:s{si}"),
            graph: parsed.stream(*si).view_for_geometry().graph.as_ref(),
            source_stream: *source_stream,
            coedge_start: 0,
            procedural_start: 0,
        })
        .collect::<Vec<_>>();
    attach_completed_intersection_pcurves_for_model_with_budget(
        &mut ir,
        &completion_sources,
        &mut annotations,
        &model_endpoint_witnesses,
        &completion_geometry_budget,
    );

    if counts.points == 0 && counts.surfaces() == 0 && counts.curves() == 0 {
        return Ok(None);
    }

    ir.source = Some(source_meta(scan, dialects));

    ctx.admit_entities(
        ir.model.entity_count() as u64,
        admitted_entities,
        "admit NX entities",
    )?;

    // Extract once: body selection and annotation attachment both read it.
    let model = crate::native::NativeModel::extract(
        ctx,
        root,
        &scan.container,
        &scan.streams,
        &mut parsed,
        terminal_lineage,
    );
    let mut active_body_selection = if let Some((selected, _, source)) = &preselection {
        let selected_hits = selected
            .iter()
            .filter_map(|body| body_node_ids.get(body))
            .map(BTreeSet::len)
            .sum::<usize>();
        apply_preselected_active_body_selection(&mut ir, selected, source, Some(selected_hits))
    } else {
        select_active_body(&mut ir, &body_node_ids, &rmfastload_ids)
    };
    if !active_body_selection {
        active_body_selection = select_terminal_feature_bodies(&mut ir, &model);
    }
    classify_body_kinds(&mut ir);
    match crate::native::attach_annotations(
        ctx,
        &mut ir,
        &model,
        scan,
        &mut annotations,
        &mut unknowns,
    ) {
        Ok(()) => {}
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    }
    for (si, unknown_index) in stream_unknowns {
        retain_unknown_stream_data(ctx, &scan.streams[si], &mut unknowns[unknown_index])?;
    }
    prune_unreferenced_unknown_carriers(&mut ir);
    finalize_point_topology(&mut ir, &mut annotations);
    let referenced_pcurves: BTreeSet<_> = ir
        .model
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter().map(|pcurve| pcurve.pcurve.clone()))
        .collect();
    ir.model
        .pcurves
        .retain(|pcurve| referenced_pcurves.contains(&pcurve.id));
    retain_live_unknown_links(&ir, &mut unknowns, &mut annotations);
    let mut annotations = annotations.build();
    retain_live_annotations(&ir, &unknowns, &mut annotations);
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
        support_uv_validation_limit: support_uv_limit,
        support_uv_limit,
        coupled_support_uv_limit: support_uv_limit,
    };
    let mut report = build_geometry_report(
        scan,
        parsed.unmatched_tombstone_counts(),
        &ir,
        &counts,
        !ir.model.faces.is_empty(),
        ir.model.bodies.len() > 1 && !active_body_selection,
        ir.model.tessellations.len(),
        &model,
        completion_budget,
        adaptive_geometry_budget.exhausted(),
        dialect_losses,
        notes,
    );
    report_untransferred_streams(scan, &mut report, true);
    Ok(Some((ir, report, annotations, unknowns)))
}

pub(crate) fn prune_unreferenced_unknown_carriers(ir: &mut CadIr) {
    let mut used_surfaces: BTreeSet<_> = ir
        .model
        .faces
        .iter()
        .map(|face| face.surface.clone())
        .collect();
    let mut used_curves: BTreeSet<_> = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| edge.curve.clone())
        .collect();
    loop {
        let previous = (used_surfaces.len(), used_curves.len());
        for procedural in &ir.model.procedural_surfaces {
            if !used_surfaces.contains(&procedural.surface) {
                continue;
            }
            match procedural.definition() {
                ProceduralSurfaceDefinition::Offset { support, .. } => {
                    used_surfaces.insert(support.clone());
                }
                ProceduralSurfaceDefinition::Blend {
                    supports, spine, ..
                } => {
                    used_surfaces.extend(
                        supports
                            .iter()
                            .flatten()
                            .map(|support| support.surface.clone()),
                    );
                    used_curves.extend(spine.iter().cloned());
                }
                _ => {}
            }
        }
        for procedural in &ir.model.procedural_curves {
            if !used_curves.contains(&procedural.curve) {
                continue;
            }
            match procedural.definition() {
                ProceduralCurveDefinition::Intersection { context, .. }
                | ProceduralCurveDefinition::SurfaceCurve { context, .. } => {
                    used_surfaces
                        .extend(context.sides.iter().filter_map(|side| side.surface.clone()));
                }
                _ => {}
            }
        }
        if previous == (used_surfaces.len(), used_curves.len()) {
            break;
        }
    }
    ir.model.surfaces.retain(|surface| {
        !matches!(surface.geometry, SurfaceGeometry::Unknown { .. })
            || used_surfaces.contains(&surface.id)
    });
    ir.model.curves.retain(|curve| {
        !matches!(curve.geometry, CurveGeometry::Unknown { .. }) || used_curves.contains(&curve.id)
    });
}

pub(crate) fn retain_live_annotations(
    ir: &CadIr,
    unknowns: &[UnknownRecord],
    annotations: &mut cadmpeg_ir::Annotations,
) {
    let mut ids = BTreeSet::new();
    macro_rules! add_ids {
        ($($arena:expr),+ $(,)?) => {
            $(ids.extend($arena.iter().map(|entity| entity.id.to_string()));)+
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
    ids.extend(unknowns.iter().map(|unknown| unknown.id().to_string()));
    annotations.provenance.retain(|id, _| ids.contains(id));
    annotations.exactness.retain(|id, _| ids.contains(id));
}

pub(crate) fn retain_live_unknown_links(
    ir: &CadIr,
    unknowns: &mut [UnknownRecord],
    annotations: &mut AnnotationBuilder,
) {
    let mut ids = BTreeSet::new();
    ids.extend(ir.model.surfaces.iter().map(|entity| entity.id.to_string()));
    ids.extend(ir.model.curves.iter().map(|entity| entity.id.to_string()));
    ids.extend(ir.model.pcurves.iter().map(|entity| entity.id.to_string()));
    ids.extend(
        ir.model
            .procedural_surfaces
            .iter()
            .map(|entity| entity.id.to_string()),
    );
    ids.extend(
        ir.model
            .procedural_curves
            .iter()
            .map(|entity| entity.id.to_string()),
    );
    for unknown in unknowns.iter_mut() {
        unknown.links_mut().retain(|link| ids.contains(link));
        if !unknown.links().is_empty() {
            annotations.derived(unknown.id(), "links");
        }
    }
}

pub(crate) fn topology_body_node_ids(
    stream_index: usize,
    graph: &Graph,
) -> BTreeMap<BodyId, BTreeSet<u32>> {
    let prefix = format!("nx:s{stream_index}");
    let body_xmts: BTreeSet<_> = graph
        .body_shape_shells()
        .into_iter()
        .filter_map(|shell| shell.shell_fields().map(|fields| fields.body))
        .collect();
    body_xmts
        .into_iter()
        .filter_map(|body_xmt| {
            let shells: BTreeSet<_> = graph
                .of_kind(13)
                .filter(|shell| {
                    shell
                        .shell_fields()
                        .is_some_and(|fields| fields.body == body_xmt)
                })
                .map(|shell| shell.xmt)
                .collect();
            let faces: Vec<_> = graph
                .of_kind(14)
                .filter(|face| {
                    face.face_fields()
                        .is_some_and(|fields| shells.contains(&fields.shell))
                })
                .collect();
            let face_xmts: BTreeSet<_> = faces.iter().map(|face| face.xmt).collect();
            let loops: BTreeSet<_> = graph
                .of_kind(15)
                .filter(|loop_| {
                    loop_
                        .loop_fields()
                        .is_some_and(|fields| face_xmts.contains(&fields.face))
                })
                .map(|loop_| loop_.xmt)
                .collect();
            let fins: Vec<_> = graph
                .of_kind(17)
                .filter(|fin| {
                    fin.fin_fields()
                        .is_some_and(|fields| loops.contains(&fields.loop_xmt))
                })
                .collect();
            let edge_xmts: BTreeSet<_> = fins
                .iter()
                .filter_map(|fin| fin.fin_fields().map(|fields| fields.edge))
                .collect();
            let vertex_xmts: BTreeSet<_> = fins
                .iter()
                .filter_map(|fin| fin.fin_fields().map(|fields| fields.vertex))
                .collect();
            let face_ids = faces
                .iter()
                .map(|face| face.u32_at(4))
                .collect::<Option<BTreeSet<_>>>()?;
            let edges = graph
                .of_kind(16)
                .filter(|edge| edge_xmts.contains(&edge.xmt))
                .collect::<Vec<_>>();
            if edges.len() != edge_xmts.len() {
                return None;
            }
            let edge_ids = edges
                .iter()
                .map(|edge| edge.u32_at(4))
                .collect::<Option<BTreeSet<_>>>()?;
            let vertices = graph
                .of_kind(18)
                .filter(|vertex| vertex_xmts.contains(&vertex.xmt))
                .collect::<Vec<_>>();
            if vertices.len() != vertex_xmts.len() {
                return None;
            }
            let vertex_ids = vertices
                .iter()
                .map(|vertex| vertex.u32_at(4))
                .collect::<Option<BTreeSet<_>>>()?;
            let ids = face_ids
                .into_iter()
                .chain(edge_ids)
                .chain(vertex_ids)
                .collect();
            Some((BodyId(format!("{prefix}:body#{body_xmt}")), ids))
        })
        .collect()
}

/// Return body images whose complete topology node sets are inside the active
/// `RMFastLoad` membership set. This is the same admission predicate used after
/// topology emission, applied to graph-only body identities before carrier
/// construction.
pub(crate) fn rmfastload_selected_bodies(
    body_node_ids: &BTreeMap<BodyId, BTreeSet<u32>>,
    rmfastload_ids: &[u32],
) -> BTreeSet<BodyId> {
    let active = rmfastload_ids.iter().copied().collect::<BTreeSet<_>>();
    body_node_ids
        .iter()
        .filter(|(_, ids)| !ids.is_empty() && ids.is_subset(&active))
        .map(|(body, _)| body.clone())
        .collect()
}

pub(crate) fn rmfastload_allows_terminal_lineage(
    body_count: usize,
    rmfastload_selected: &BTreeSet<BodyId>,
) -> bool {
    body_count > 1 && rmfastload_selected.is_empty()
}

/// Return the stream ordinals that can contain selected body images. A
/// malformed body identity disables preselection rather than guessing a
/// stream owner.
pub(crate) fn rmfastload_stream_indices(selected: &BTreeSet<BodyId>) -> Option<BTreeSet<usize>> {
    selected
        .iter()
        .map(|body| body.0.strip_prefix("nx:s")?.split_once(':')?.0.parse().ok())
        .collect()
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
    let emitted = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<BTreeSet<_>>();
    if !selected.is_subset(&emitted) {
        return false;
    }
    prune_inactive_topology(ir, selected);
    if let Some(source) = &mut ir.source {
        source
            .attributes
            .insert("active_body_selector".to_string(), selector.to_string());
        let (hit_attribute, count_attribute) = match selector {
            "rmfastload_object_id_membership" => {
                (Some("rmfastload_hits"), "rmfastload_active_body_count")
            }
            "terminal_feature_body_lineage" => (None, "feature_terminal_body_count"),
            _ => (None, "active_body_count"),
        };
        if let (Some(attribute), Some(selected_hits)) = (hit_attribute, selected_hits) {
            source
                .attributes
                .insert(attribute.to_string(), selected_hits.to_string());
        }
        source
            .attributes
            .insert(count_attribute.to_string(), selected.len().to_string());
    }
    true
}

pub(crate) fn select_active_body(
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

pub(crate) fn select_terminal_feature_bodies(
    ir: &mut CadIr,
    model: &crate::native::NativeModel,
) -> bool {
    if ir.model.bodies.len() <= 1 {
        return false;
    }
    let emitted = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<BTreeSet<_>>();
    // A complete terminal mapping resolves composition even when every emitted
    // body is terminal. The absence of pruning is a valid result: it means the
    // retained body images are all final, not that lineage was unresolved.
    let Some(selected) = crate::native::terminal_feature_body_ids(
        &emitted,
        &model.segments.segment_body_bindings,
        &model.segments.segment_body_lineage_statuses,
    ) else {
        return false;
    };
    apply_preselected_active_body_selection(ir, &selected, "terminal_feature_body_lineage", None)
}

pub(crate) fn prune_inactive_topology(ir: &mut CadIr, selected: &BTreeSet<BodyId>) {
    ir.model.bodies.retain(|body| selected.contains(&body.id));
    ir.model
        .regions
        .retain(|region| selected.contains(&region.body));
    let regions: BTreeSet<_> = ir
        .model
        .regions
        .iter()
        .map(|region| region.id.clone())
        .collect();
    ir.model
        .shells
        .retain(|shell| regions.contains(&shell.region));
    let shells: BTreeSet<_> = ir
        .model
        .shells
        .iter()
        .map(|shell| shell.id.clone())
        .collect();
    ir.model.faces.retain(|face| shells.contains(&face.shell));
    let faces: BTreeSet<_> = ir.model.faces.iter().map(|face| face.id.clone()).collect();
    ir.model.loops.retain(|loop_| faces.contains(&loop_.face));
    let loops: BTreeSet<_> = ir
        .model
        .loops
        .iter()
        .map(|loop_| loop_.id.clone())
        .collect();
    ir.model
        .coedges
        .retain(|coedge| loops.contains(&coedge.owner_loop));
    let edges: BTreeSet<_> = ir
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.edge.clone())
        .chain(
            ir.model
                .shells
                .iter()
                .flat_map(|shell| shell.wire_edges.iter().cloned()),
        )
        .collect();
    ir.model.edges.retain(|edge| edges.contains(&edge.id));
    let vertices: BTreeSet<_> = ir
        .model
        .edges
        .iter()
        .flat_map(|edge| [edge.start.clone(), edge.end.clone()])
        .chain(
            ir.model
                .shells
                .iter()
                .flat_map(|shell| shell.free_vertices.iter().cloned()),
        )
        .collect();
    ir.model
        .vertices
        .retain(|vertex| vertices.contains(&vertex.id));
    let points: BTreeSet<_> = ir
        .model
        .vertices
        .iter()
        .map(|vertex| vertex.point.clone())
        .collect();
    ir.model.points.retain(|point| points.contains(&point.id));
    prune_inactive_geometry(ir);
}

pub(crate) fn prune_inactive_geometry(ir: &mut CadIr) {
    let mut surfaces: BTreeSet<_> = ir
        .model
        .faces
        .iter()
        .map(|face| face.surface.clone())
        .collect();
    let mut curves: BTreeSet<_> = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| edge.curve.clone())
        .collect();
    let pcurves: BTreeSet<_> = ir
        .model
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter().map(|pcurve| pcurve.pcurve.clone()))
        .collect();

    loop {
        let old_surface_count = surfaces.len();
        let old_curve_count = curves.len();
        for procedural in &ir.model.procedural_surfaces {
            if !surfaces.contains(&procedural.surface) {
                continue;
            }
            match procedural.definition() {
                ProceduralSurfaceDefinition::Offset { support, .. } => {
                    surfaces.insert(support.clone());
                }
                ProceduralSurfaceDefinition::Blend {
                    supports, spine, ..
                } => {
                    surfaces.extend(
                        supports
                            .iter()
                            .flatten()
                            .map(|support| support.surface.clone()),
                    );
                    curves.extend(spine.iter().cloned());
                }
                _ => {}
            }
        }
        for procedural in &ir.model.procedural_curves {
            if !curves.contains(&procedural.curve) {
                continue;
            }
            match procedural.definition() {
                ProceduralCurveDefinition::Intersection { context, .. }
                | ProceduralCurveDefinition::SurfaceCurve { context, .. } => {
                    surfaces.extend(context.sides.iter().filter_map(|side| side.surface.clone()));
                }
                _ => {}
            }
        }
        if surfaces.len() == old_surface_count && curves.len() == old_curve_count {
            break;
        }
    }

    ir.model
        .procedural_surfaces
        .retain(|procedural| surfaces.contains(&procedural.surface));
    ir.model
        .procedural_curves
        .retain(|procedural| curves.contains(&procedural.curve));
    ir.model
        .surfaces
        .retain(|surface| surfaces.contains(&surface.id));
    ir.model.curves.retain(|curve| curves.contains(&curve.id));
    ir.model
        .pcurves
        .retain(|pcurve| pcurves.contains(&pcurve.id));
}

pub(crate) fn finalize_point_topology(ir: &mut CadIr, annotations: &mut AnnotationBuilder) {
    let referenced_points: BTreeSet<_> = ir
        .model
        .vertices
        .iter()
        .map(|vertex| vertex.point.clone())
        .collect();
    if !ir.model.bodies.is_empty() {
        ir.model
            .points
            .retain(|point| referenced_points.contains(&point.id));
        return;
    }

    if ir.model.points.is_empty() {
        return;
    }

    let body_id = BodyId("nx:derived:point-body#0".to_string());
    let region_id = RegionId("nx:derived:point-region#0".to_string());
    let shell_id = ShellId("nx:derived:point-shell#0".to_string());
    let stream = annotations.stream("nx:container");
    for id in [&body_id.0, &region_id.0, &shell_id.0] {
        annotations
            .note(id, stream, 0)
            .tag("derived_point_topology");
        annotations.exactness(id, Exactness::Inferred);
    }

    let mut free_vertices = Vec::with_capacity(ir.model.points.len());
    for (index, point) in ir.model.points.iter().enumerate() {
        let vertex_id = VertexId(format!("nx:derived:point-vertex#{index}"));
        annotations
            .note(&vertex_id, stream, 0)
            .tag("derived_point_topology");
        annotations.exactness(&vertex_id, Exactness::Inferred);
        ir.model.vertices.push(Vertex {
            id: vertex_id.clone(),
            point: point.id.clone(),
            tolerance: None,
        });
        free_vertices.push(vertex_id);
    }
    ir.model.shells.push(Shell {
        id: shell_id.clone(),
        region: region_id.clone(),
        faces: Vec::new(),
        wire_edges: Vec::new(),
        free_vertices,
    });
    ir.model.regions.push(Region {
        id: region_id.clone(),
        body: body_id.clone(),
        shells: vec![shell_id],
    });
    ir.model.bodies.push(Body {
        id: body_id,
        kind: BodyKind::General,
        regions: vec![region_id],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
}

pub(crate) fn classify_body_kinds(ir: &mut CadIr) {
    let region_bodies: BTreeMap<_, _> = ir
        .model
        .regions
        .iter()
        .map(|region| (region.id.clone(), region.body.clone()))
        .collect();
    let shell_bodies: BTreeMap<_, _> = ir
        .model
        .shells
        .iter()
        .filter_map(|shell| {
            region_bodies
                .get(&shell.region)
                .cloned()
                .map(|body| (shell.id.clone(), body))
        })
        .collect();
    let face_bodies: BTreeMap<_, _> = ir
        .model
        .faces
        .iter()
        .filter_map(|face| {
            shell_bodies
                .get(&face.shell)
                .cloned()
                .map(|body| (face.id.clone(), body))
        })
        .collect();
    let loop_bodies: BTreeMap<_, _> = ir
        .model
        .loops
        .iter()
        .filter_map(|loop_| {
            face_bodies
                .get(&loop_.face)
                .cloned()
                .map(|body| (loop_.id.clone(), body))
        })
        .collect();
    let coedge_bodies: BTreeMap<_, _> = ir
        .model
        .coedges
        .iter()
        .filter_map(|coedge| {
            loop_bodies
                .get(&coedge.owner_loop)
                .cloned()
                .map(|body| (coedge.id.clone(), body))
        })
        .collect();
    let mut edge_uses = BTreeMap::<BodyId, BTreeMap<EdgeId, usize>>::new();
    for coedge in &ir.model.coedges {
        let Some(body) = coedge_bodies.get(&coedge.id) else {
            continue;
        };
        *edge_uses
            .entry(body.clone())
            .or_default()
            .entry(coedge.edge.clone())
            .or_default() += 1;
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
}
