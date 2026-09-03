// SPDX-License-Identifier: Apache-2.0
//! Geometry-report losses for NX decode.

use super::feature_completeness::{
    active_configuration_state_is_incomplete, body_selection_is_incomplete,
    body_selections_overlap, chamfer_definition_is_incomplete, combine_definition_is_incomplete,
    datum_coordinate_system_is_incomplete, datum_plane_is_incomplete,
    delete_body_definition_is_incomplete, draft_definition_is_incomplete,
    extend_surface_definition_is_incomplete, extrude_definition_is_incomplete,
    face_blend_definition_is_incomplete, face_selection_is_incomplete,
    fillet_definition_is_incomplete, finite_feature_point, hole_definition_is_incomplete,
    incomplete_expression_parameters, loft_definition_is_incomplete,
    offset_surface_definition_is_incomplete, output_free_local_body_construction,
    output_free_native_snapshot, output_free_pattern_construction,
    output_free_trim_surface_construction, path_ref_is_incomplete, pattern_feature_is_incomplete,
    positive_feature_length, projected_curve_direction_is_incomplete,
    replace_face_definition_is_incomplete, revolve_definition_is_incomplete,
    rib_definition_is_incomplete, sew_bodies_definition_is_incomplete,
    shell_definition_is_incomplete, sphere_definition_is_incomplete,
    sweep_definition_is_incomplete, thicken_definition_is_incomplete,
    trim_bodies_definition_is_incomplete, trim_surface_definition_is_incomplete,
    valid_feature_direction,
};
use super::geometry_work::{
    MAX_ADAPTIVE_GEOMETRY_WORK, MAX_COUPLED_SUPPORT_UV_GEOMETRY_WORK,
    MAX_PCURVE_COMPLETION_GEOMETRY_WORK, MAX_SERIALIZED_SUPPORT_UV_GEOMETRY_WORK,
    MAX_SUPPORT_UV_COMPLETION_GEOMETRY_WORK,
};
use super::pcurves::MAX_EXACT_BOUNDARY_TRANSFER_SAMPLES;
use super::support_uv::pcurve_requires_completion;
use super::{Counts, Scan};
use crate::loss::NxLossCode;
use crate::parasolid::StreamKind;
use cadmpeg_ir::codec::DecodeBody;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    BodySelection, BooleanOp, DatumPlaneReference, Feature, FeatureDefinition, SketchSpace,
};
use cadmpeg_ir::report::LossNote;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
// Each flag is an independent model-wide phase fact surfaced in the loss
// report; combining them would hide which bounded phase stopped.
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct CompletionBudgetStatus {
    pub(crate) exact_boundary_exhausted: bool,
    pub(crate) transfer_exhausted: bool,
    pub(crate) support_uv_validation_exhausted: bool,
    pub(crate) support_uv_exhausted: bool,
    pub(crate) coupled_support_uv_exhausted: bool,
    pub(crate) completion_geometry_exhausted: bool,
    pub(crate) serialized_support_uv_geometry_exhausted: bool,
    pub(crate) support_uv_geometry_exhausted: bool,
    pub(crate) coupled_support_uv_geometry_exhausted: bool,
    pub(crate) support_uv_lane_geometry_exhausted: bool,
    pub(crate) transfer_limit: usize,
    pub(crate) support_uv_validation_limit: usize,
    pub(crate) support_uv_limit: usize,
    pub(crate) coupled_support_uv_limit: usize,
}

// Keep the independent report facts explicit at the decode/report boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_geometry_report(
    scan: &Scan,
    unmatched_delta_tombstone_counts: &BTreeMap<&'static str, usize>,
    ir: &CadIr,
    counts: &Counts,
    has_topology: bool,
    has_unresolved_sub_bodies: bool,
    tessellation_count: usize,
    model: &crate::native::NativeModel,
    completion_budget: CompletionBudgetStatus,
    adaptive_geometry_exhausted: bool,
    dialect_losses: &[LossNote],
    notes: &[String],
) -> DecodeBody {
    let has_untransferred_attribute_fields = model.has_untransferred_parasolid_attribute_fields();
    let mut losses = Vec::new();

    losses.push(NxLossCode::CarrierAnalyticCensus.note(format!(
        "Decoded {} POINT carrier(s) verbatim from Parasolid POINT records (3×f64 big-endian, \
             metres → millimetres), {} analytic surface carrier(s) ({} plane, {} cylinder, {} \
             cone, {} sphere, {} torus), and {} analytic curve carrier(s) ({} line, {} circle, {} \
             ellipse). All parameters are byte-exact at the document's millimetre scale.",
        counts.points,
        counts.surfaces(),
        counts.planes,
        counts.cylinders,
        counts.cones,
        counts.spheres,
        counts.tori,
        counts.curves(),
        counts.lines,
        counts.circles,
        counts.ellipses,
    )));

    if tessellation_count != 0 {
        losses.push(NxLossCode::CarrierTessellationCensus.note(format!(
            "Decoded {tessellation_count} embedded JT display tessellation(s) with scene-node ownership, model-space coordinates, topological triangle connectivity, and corner normals when bound."
        )));
    }

    if !has_topology {
        losses.push(NxLossCode::TopologyGraphNotReconstructed.note(
            "The B-rep topology graph (body→shell→face→loop→fin→edge→vertex) was not \
                      reconstructed because the surviving typed records did not form a complete \
                      connected ownership graph. Exact-key supported partition↔deltas replacements \
                      and deletions were applied before graph construction. Required unresolved \
                      records prevent their dependent incidence from being emitted; decoded geometry \
                      then remains unattached.",
        ));
    }

    if counts.intersection_rejections.total() > 0 {
        losses.push(NxLossCode::IntersectionRecordsOpaque.note(format!(
            "{} surface-intersection record(s) without a complete validated CHART_s and \
                 term-endpoint witness remain opaque constructions. Support-UV values govern \
                 optional pcurve attachment and do not invalidate a witnessed 3D carrier. Each \
                 Parasolid stream is preserved verbatim as an unknown passthrough record so the \
                 unresolved source bytes remain available. Rejections: {} missing chart, {} missing \
                 start term, {} missing end term, {} endpoint mismatch.",
            counts.intersection_rejections.total(),
            counts.intersection_rejections.missing_chart,
            counts.intersection_rejections.missing_start_term,
            counts.intersection_rejections.missing_end_term,
            counts.intersection_rejections.endpoint_mismatch,
        )));
    }

    let unresolved_intersection_lanes = ir
        .model
        .procedural_curves
        .iter()
        .filter_map(|procedural| {
            let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
                procedural.definition()
            else {
                return None;
            };
            Some(
                context
                    .sides
                    .iter()
                    .filter(|side| pcurve_requires_completion(side.pcurve.as_ref()))
                    .count(),
            )
        })
        .sum::<usize>();
    if unresolved_intersection_lanes > 0
        && (completion_budget.exact_boundary_exhausted
            || completion_budget.transfer_exhausted
            || completion_budget.support_uv_validation_exhausted
            || completion_budget.support_uv_exhausted
            || completion_budget.coupled_support_uv_exhausted
            || completion_budget.completion_geometry_exhausted
            || completion_budget.serialized_support_uv_geometry_exhausted
            || completion_budget.support_uv_geometry_exhausted
            || completion_budget.coupled_support_uv_geometry_exhausted
            || completion_budget.support_uv_lane_geometry_exhausted)
    {
        let mut bounded_phases = Vec::new();
        if completion_budget.exact_boundary_exhausted {
            bounded_phases.push("exact-boundary transfer");
        }
        if completion_budget.transfer_exhausted {
            bounded_phases.push("opposite-chart transfer");
        }
        if completion_budget.support_uv_validation_exhausted {
            bounded_phases.push("support-UV consistency checks");
        }
        if completion_budget.support_uv_exhausted {
            bounded_phases.push("EXT11 support-UV fitting");
        }
        if completion_budget.coupled_support_uv_exhausted {
            bounded_phases.push("coupled EXT11 support-UV fitting");
        }
        if completion_budget.completion_geometry_exhausted {
            bounded_phases.push("pcurve geometry fitting");
        }
        if completion_budget.serialized_support_uv_geometry_exhausted {
            bounded_phases.push("serialized support-UV geometry fitting");
        }
        if completion_budget.support_uv_geometry_exhausted {
            bounded_phases.push("support-UV geometry fitting");
        }
        if completion_budget.coupled_support_uv_geometry_exhausted {
            bounded_phases.push("coupled support-UV geometry fitting");
        }
        if completion_budget.support_uv_lane_geometry_exhausted {
            bounded_phases.push("support-UV lane geometry slices");
        }
        losses.push(NxLossCode::IntersectionPcurveCompletionBounded.note(format!(
            "Model-wide geometric completion stopped at its bounded work budget for {} ({} exact-boundary transfer samples, {} opposite-chart transfer samples, {} support-UV consistency checks, {} support-UV point fits, {} coupled support-UV point fits, {} pcurve geometry evaluations, {} serialized support-UV geometry evaluations, {} support-UV geometry evaluations, {} coupled support-UV geometry evaluations); {} intersection pcurve lane(s) remain incomplete and were not emitted as completed parameterizations.",
            bounded_phases.join(" and "),
            MAX_EXACT_BOUNDARY_TRANSFER_SAMPLES,
            completion_budget.transfer_limit,
            completion_budget.support_uv_validation_limit,
            completion_budget.support_uv_limit,
            completion_budget.coupled_support_uv_limit,
            MAX_PCURVE_COMPLETION_GEOMETRY_WORK,
            MAX_SERIALIZED_SUPPORT_UV_GEOMETRY_WORK,
            MAX_SUPPORT_UV_COMPLETION_GEOMETRY_WORK,
            MAX_COUPLED_SUPPORT_UV_GEOMETRY_WORK,
            unresolved_intersection_lanes,
        )));
    }

    if adaptive_geometry_exhausted {
        losses.push(NxLossCode::GeometryAdaptiveWorkBounded.note(format!(
            "Model-wide adaptive geometry certification stopped at its {MAX_ADAPTIVE_GEOMETRY_WORK}-unit work bound; \
             unresolved adaptive geometry certification results were left untyped.",
        )));
    }

    if scan.count(StreamKind::Deltas) > 0 {
        let unmatched_tombstones = unmatched_delta_tombstone_counts.values().sum::<usize>();
        let unmatched_tombstone_detail = unmatched_delta_tombstone_counts
            .iter()
            .map(|(family, count)| format!("{family} {count}"))
            .collect::<Vec<_>>()
            .join(", ");
        if unmatched_tombstones == 0 {
            losses.push(NxLossCode::DeltasApplied.note(format!(
                "{} Parasolid deltas stream(s) were processed in validated UG_PART segment order. \
                 Equal-schema deltas were paired with the preceding partition. Exact-key \
                 BODY, SHELL, FACE, LOOP, FIN, EDGE, VERTEX, REGION, POINT, LINE, CIRCLE, ELLIPSE, PLANE, CYLINDER, CONE, SPHERE, TORUS, INTERSECTION, BLEND_SURF, OFFSET_SURF, B_SURFACE, TRIMMED_CURVE, B_CURVE, and SP_CURVE full records and compact \
                 non-topology replacements and tombstones were applied using the last event for \
                 each key within each current body-sequence interval. Validated partition topology remained authoritative, including any \
                 point, curve, or surface carrier still referenced by surviving topology. Complete \
                 ENTITY_51, ENTITY_52, ENTITY_53, and ENTITY_54 records were retained for native \
                 attribute extraction. Every completely bounded full record, compact tombstone, \
                 and BODY revision envelope was retained as an individually identified native event \
                 with its source bounds and decoded identities; BODY state tails retain exact \
                 bounded bytes and digests. Complete transmit headers retain their description, \
                 schema, consecutive identities, and exact bytes. Terminal two- and \
                 four-null-reference trailers retain their exact stream boundary and bytes. \
                 Count-selected numeric tails after \
                 term-use endpoints were retained with their ordered finite binary64 values. Maximal \
                 event gaps containing only typed stream-local references, framed reference/type \
                 maps, and complete four-reference state packets, reference-marker packets, and inline schema \
                 declarations were retained in order. \
                 Spans outside those events were retained with exact inflated-stream bounds and \
                 digests. Semantic intersection and NURBS records were retained in the semantic \
                 lane. Every \
                 terminal tombstone resolved to an exact current or earlier-added key.",
                scan.count(StreamKind::Deltas)
            )));
        } else {
            losses.push(NxLossCode::DeltasUnmatchedTombstones.note(format!(
                "{} Parasolid deltas stream(s) were processed in validated UG_PART segment order. \
                    Equal-schema deltas were paired with the preceding partition. Exact-key revisions in current body-sequence intervals were applied using the last \
                 event for each key, but {unmatched_tombstones} terminal tombstone(s) have no exact \
                 current or earlier-added key and remain unresolved: {unmatched_tombstone_detail}.",
                scan.count(StreamKind::Deltas)
            )));
        }
    }

    if has_unresolved_sub_bodies {
        losses.push(NxLossCode::SubBodyCompositionUnresolved.note(format!(
            "This part is composed of {} sub-body partition(s); its decoded feature-history \
                 Booleans do not resolve every intermediate body object to a partition image. \
                 Carriers from all sub-bodies are emitted without the unresolved composition that \
                 would remove interior/construction faces.",
            scan.count(StreamKind::Partition)
        )));
    }

    append_design_intent_losses(ir, &mut losses);

    if has_untransferred_attribute_fields {
        losses.push(NxLossCode::AttributeValueUnresolved.note(
            "A referenced Parasolid attribute value was not transferred because its \
                      complete value relation did not resolve.",
        ));
    }

    losses.extend_from_slice(dialect_losses);
    DecodeBody {
        geometry_transferred: true,
        coverage: cadmpeg_ir::Coverage::default(),
        losses,
        notes: notes.to_vec(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
    }
}

pub(crate) fn append_design_intent_losses(ir: &CadIr, losses: &mut Vec<LossNote>) {
    let current_body_ids = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    // Require a non-BaseFeature writer before treating body-to-history as proven.
    let (active_features, closure_rejection) =
        match crate::native::history::active_feature_closure(ir, &current_body_ids) {
            Ok(active) => (Some(active), None),
            Err(rejection) => (None, Some(rejection.code())),
        };
    let active_features = active_features.filter(|active| {
        active.iter().any(|id| {
            ir.model.features.iter().any(|feature| {
                feature.id == *id
                    && !matches!(&feature.definition, FeatureDefinition::BaseFeature { .. })
            })
        })
    });
    let suppression_scope = active_features.as_ref().map_or("", |_| "active ");
    let feature_in_active_scope = |feature: &Feature| {
        active_features
            .as_ref()
            .is_none_or(|active| active.contains(&feature.id))
    };
    let unresolved_suppression_count = ir
        .model
        .features
        .iter()
        .filter(|feature| {
            feature.suppressed.is_none()
                && active_features
                    .as_ref()
                    .is_none_or(|active| active.contains(&feature.id))
        })
        .count();
    if unresolved_suppression_count != 0 {
        let closure_detail = closure_rejection
            .map(|reason| format!(" Active-feature closure rejected with `{reason}`."))
            .unwrap_or_default();
        losses.push(NxLossCode::FeatureSuppressionUnresolved.note(format!(
            "Suppression state remains unresolved for {unresolved_suppression_count} NX \
                 {suppression_scope}feature history operation(s): no admitted \
                 operation-to-state-object-to-typed-value relation is present. Common-frame \
                 state lanes, saved toggles, OM registry declarations, and topology ObjectState \
                 values remain non-suppression evidence.{closure_detail}"
        )));
    }

    let active_configuration_count = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.active.is_active())
        .count();
    let current_bodies = ir
        .model
        .bodies
        .iter()
        .map(|body| &body.id)
        .collect::<BTreeSet<_>>();
    let incomplete_configuration_count = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| {
            configuration.bodies.is_unresolved()
                || active_configuration_count != 1
                || (configuration.active.is_active()
                    && configuration.bodies.resolved().is_none_or(|bodies| {
                        bodies.len() != current_bodies.len()
                            || bodies.iter().collect::<BTreeSet<_>>() != current_bodies
                    }))
                || (configuration.active.is_active()
                    && active_configuration_state_is_incomplete(ir, configuration))
        })
        .count();
    if incomplete_configuration_count != 0 {
        losses.push(NxLossCode::ConfigurationStateUnresolved.note(format!(
            "Activation, complete body membership, evaluated feature state, or evaluated \
                 parameter state remains unresolved for {incomplete_configuration_count} NX \
                 design configuration(s)."
        )));
    }

    let incomplete_expression_count = incomplete_expression_parameters(ir).len();
    if incomplete_expression_count != 0 {
        losses.push(NxLossCode::ExpressionParameterIncomplete.note(format!(
            "Neutral evaluation or dependency semantics remain incomplete for \
                 {incomplete_expression_count} NX expression parameter(s)."
        )));
    }

    let mut native_feature_kinds = BTreeMap::<&str, usize>::new();
    for feature in &ir.model.features {
        if !feature_in_active_scope(feature) {
            continue;
        }
        if let FeatureDefinition::Native { kind, .. } = &feature.definition {
            *native_feature_kinds.entry(kind.as_str()).or_default() += 1;
        }
    }
    if !native_feature_kinds.is_empty() {
        let kinds = native_feature_kinds
            .into_iter()
            .map(|(kind, count)| format!("{kind} ({count})"))
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(NxLossCode::FeatureNativeKindRetained.note(format!(
            "NX feature-history operation(s) remain native-only because their complete neutral \
                 operation semantics are not decoded: {kinds}."
        )));
    }

    let mut unresolved_feature_families = BTreeMap::<&str, usize>::new();
    for feature in &ir.model.features {
        if !feature_in_active_scope(feature) {
            continue;
        }
        let family = match feature.definition {
            FeatureDefinition::BrepUnresolved => "brep",
            FeatureDefinition::DatumPlaneUnresolved => "datum plane",
            FeatureDefinition::DatumAxisUnresolved => "datum axis",
            FeatureDefinition::DatumPointUnresolved => "datum point",
            FeatureDefinition::DatumCoordinateSystemUnresolved => "datum coordinate system",
            FeatureDefinition::BridgeCurveUnresolved => "bridge curve",
            FeatureDefinition::LoftUnresolved => "loft",
            FeatureDefinition::ThroughCurveMeshUnresolved => "through curve mesh",
            FeatureDefinition::FreeformSurfaceUnresolved => "freeform surface",
            FeatureDefinition::ExtractFaceUnresolved => "extract face",
            FeatureDefinition::CopyFaceUnresolved => "copy face",
            FeatureDefinition::LinkedFaceUnresolved => "linked face",
            FeatureDefinition::FillHoleUnresolved => "fill hole",
            FeatureDefinition::MoveFaceUnresolved => "move face",
            FeatureDefinition::MoveObjectUnresolved => "move object",
            FeatureDefinition::CylinderUnresolved => "cylinder",
            FeatureDefinition::ConeUnresolved => "cone",
            FeatureDefinition::SphereUnresolved => "sphere",
            FeatureDefinition::ThreadUnresolved => "thread",
            FeatureDefinition::DetailedThreadUnresolved => "detailed thread",
            FeatureDefinition::DraftUnresolved => "draft",
            FeatureDefinition::DeleteFaceUnresolved => "delete face",
            FeatureDefinition::MirrorFaceUnresolved => "mirror face",
            FeatureDefinition::SubdivisionBodyUnresolved => "subdivision body",
            FeatureDefinition::TopologyOptimizationUnresolved => "topology optimization",
            _ => continue,
        };
        *unresolved_feature_families.entry(family).or_default() += 1;
    }
    if !unresolved_feature_families.is_empty() {
        let families = unresolved_feature_families
            .into_iter()
            .map(|(family, count)| format!("{family} ({count})"))
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(
            NxLossCode::FeatureFamilyConstructionUnresolved.note(format!(
                "NX feature family identities were transferred, but their neutral construction \
                 semantics remain unresolved: {families}."
            )),
        );
    }

    let mut incomplete_feature_output_families = BTreeMap::<&str, usize>::new();
    let mut incomplete_feature_construction_families = BTreeMap::<&str, usize>::new();
    let generated_body_outputs = ir
        .model
        .feature_result_topologies
        .iter()
        .filter(|state| !state.bodies.is_empty())
        .map(|state| &state.output_of)
        .collect::<BTreeSet<_>>();
    for feature in &ir.model.features {
        if !feature_in_active_scope(feature) {
            continue;
        }
        let is_exact_empty_base = matches!(
            &feature.definition,
            FeatureDefinition::BaseFeature {
                bodies: BodySelection::Resolved { bodies, native },
            } if bodies.is_empty() && !native.trim().is_empty() && feature.outputs.is_empty()
        );
        if feature.suppressed != Some(true)
            && !is_exact_empty_base
            && !output_free_native_snapshot(feature)
            && !output_free_local_body_construction(feature)
            && !output_free_pattern_construction(feature)
            && !output_free_trim_surface_construction(feature)
        {
            if let Some(family) = feature.definition.body_output_family().filter(|_| {
                let current_outputs_are_valid = !feature.outputs.is_empty()
                    && feature.outputs.iter().collect::<BTreeSet<_>>().len()
                        == feature.outputs.len()
                    && feature
                        .outputs
                        .iter()
                        .all(|output| ir.model.bodies.iter().any(|body| body.id == *output));
                !(current_outputs_are_valid
                    || feature.outputs.is_empty() && generated_body_outputs.contains(&feature.id))
            }) {
                *incomplete_feature_output_families
                    .entry(family)
                    .or_default() += 1;
                continue;
            }
        }
        let family = match &feature.definition {
            FeatureDefinition::BaseFeature { bodies }
                if !is_exact_empty_base
                    && !output_free_native_snapshot(feature)
                    && body_selection_is_incomplete(bodies) =>
            {
                "base feature"
            }
            FeatureDefinition::Block {
                dimensions,
                placement,
                op,
            } if dimensions.is_none_or(|dimensions| {
                dimensions
                    .into_iter()
                    .any(|dimension| !positive_feature_length(dimension))
            }) || placement.is_none_or(|placement| !placement.is_proper_rigid())
                || matches!(op, BooleanOp::Unresolved) =>
            {
                "block"
            }
            FeatureDefinition::Sphere { .. } if sphere_definition_is_incomplete(feature) => {
                "sphere"
            }
            FeatureDefinition::DatumOffsetPlane {
                reference,
                distance,
            } if !distance.0.is_finite()
                || reference.as_ref().is_none_or(|reference| match reference {
                    DatumPlaneReference::Feature(reference) => {
                        ir.model
                            .features
                            .iter()
                            .find(|candidate| candidate.id == *reference)
                            .is_none_or(|source| source.ordinal >= feature.ordinal)
                            || !feature.dependencies.contains(reference)
                    }
                    DatumPlaneReference::Face { face, .. } => face_selection_is_incomplete(face),
                }) =>
            {
                "datum plane"
            }
            FeatureDefinition::DatumPlane {
                origin,
                normal,
                u_axis,
            } if datum_plane_is_incomplete(*origin, *normal, *u_axis) => "datum plane",
            FeatureDefinition::DatumAxis { origin, direction }
                if !finite_feature_point(*origin) || !valid_feature_direction(*direction) =>
            {
                "datum axis"
            }
            FeatureDefinition::DatumPoint { position, .. } if !finite_feature_point(*position) => {
                "datum point"
            }
            FeatureDefinition::DatumCoordinateSystem {
                origin,
                x_axis,
                y_axis,
                z_axis,
            } if datum_coordinate_system_is_incomplete(*origin, *x_axis, *y_axis, *z_axis) => {
                "datum coordinate system"
            }
            FeatureDefinition::ExtractBody { source } if body_selection_is_incomplete(source) => {
                "extract body"
            }
            FeatureDefinition::Sketch { space, sketch }
                if !matches!(space, SketchSpace::Planar)
                    || sketch.as_ref().is_none_or(|sketch| {
                        ir.model
                            .sketches
                            .iter()
                            .find(|candidate| candidate.id == *sketch)
                            .is_none_or(|sketch| {
                                matches!(
                                    sketch.placement,
                                    cadmpeg_ir::sketches::SketchPlacement::Unresolved
                                )
                            })
                    }) =>
            {
                "sketch"
            }
            FeatureDefinition::Loft { .. } if loft_definition_is_incomplete(feature) => "loft",
            FeatureDefinition::ProjectedCurve {
                source,
                target_faces,
                direction,
                bidirectional,
            } if path_ref_is_incomplete(source)
                || face_selection_is_incomplete(target_faces)
                || projected_curve_direction_is_incomplete(*direction)
                || bidirectional.is_none() =>
            {
                "projected curve"
            }
            FeatureDefinition::TrimSurface { .. }
                if trim_surface_definition_is_incomplete(feature) =>
            {
                "trim surface"
            }
            FeatureDefinition::ExtendSurface { .. }
                if extend_surface_definition_is_incomplete(feature) =>
            {
                "extend surface"
            }
            FeatureDefinition::CosmeticThread {
                face,
                diameter,
                extent,
            } if face_selection_is_incomplete(face) || diameter.is_none() || extent.is_none() => {
                "cosmetic thread"
            }
            FeatureDefinition::Hole { .. } if hole_definition_is_incomplete(feature) => "hole",
            FeatureDefinition::Rib { .. } if rib_definition_is_incomplete(feature) => "rib",
            FeatureDefinition::Chamfer { .. } if chamfer_definition_is_incomplete(feature) => {
                "chamfer"
            }
            FeatureDefinition::Fillet { .. } if fillet_definition_is_incomplete(feature) => {
                "fillet"
            }
            FeatureDefinition::FaceBlend { .. } if face_blend_definition_is_incomplete(feature) => {
                "face blend"
            }
            FeatureDefinition::Shell { .. }
                if shell_definition_is_incomplete(&feature.definition) =>
            {
                "shell"
            }
            FeatureDefinition::SewBodies { .. } if sew_bodies_definition_is_incomplete(feature) => {
                "sew bodies"
            }
            FeatureDefinition::TrimBodies { .. }
                if trim_bodies_definition_is_incomplete(feature) =>
            {
                "trim bodies"
            }
            FeatureDefinition::Extrude { .. } if extrude_definition_is_incomplete(feature) => {
                "extrude"
            }
            FeatureDefinition::Revolve { .. } if revolve_definition_is_incomplete(feature) => {
                "revolve"
            }
            FeatureDefinition::Sweep { .. } if sweep_definition_is_incomplete(feature) => "sweep",
            FeatureDefinition::OffsetSurface { .. }
                if offset_surface_definition_is_incomplete(feature) =>
            {
                "offset surface"
            }
            FeatureDefinition::Thicken { .. } if thicken_definition_is_incomplete(feature) => {
                "thicken"
            }
            FeatureDefinition::Draft { .. } if draft_definition_is_incomplete(feature) => "draft",
            FeatureDefinition::Pattern { seeds, pattern }
                if pattern_feature_is_incomplete(seeds, pattern, &feature.dependencies) =>
            {
                "pattern"
            }
            FeatureDefinition::SectionShape {
                first,
                second,
                approximate,
            } if body_selection_is_incomplete(first)
                || body_selection_is_incomplete(second)
                || body_selections_overlap(first, second)
                || approximate.is_none() =>
            {
                "section"
            }
            FeatureDefinition::Combine { .. } if combine_definition_is_incomplete(feature) => {
                "body combine"
            }
            FeatureDefinition::DeleteBody { .. }
                if delete_body_definition_is_incomplete(feature) =>
            {
                "delete body"
            }
            FeatureDefinition::ReplaceFace { .. }
                if replace_face_definition_is_incomplete(feature) =>
            {
                "replace face"
            }
            _ => continue,
        };
        *incomplete_feature_construction_families
            .entry(family)
            .or_default() += 1;
    }
    if !incomplete_feature_output_families.is_empty() {
        let families = incomplete_feature_output_families
            .into_iter()
            .map(|(family, count)| format!("{family} ({count})"))
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(NxLossCode::FeatureOutputLineageIncomplete.note(format!(
            "NX typed feature operation output lineage is missing, duplicated, or does not \
                 resolve to a transferred body: {families}."
        )));
    }
    if !incomplete_feature_construction_families.is_empty() {
        let families = incomplete_feature_construction_families
            .into_iter()
            .map(|(family, count)| format!("{family} ({count})"))
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(NxLossCode::FeatureConstructionIncomplete.note(format!(
            "NX typed feature operations have incomplete neutral construction fields: \
                 {families}."
        )));
    }

    let sketch_feature_count = ir
        .model
        .features
        .iter()
        .filter(|feature| feature_in_active_scope(feature))
        .filter(|feature| matches!(feature.definition, FeatureDefinition::Sketch { .. }))
        .count();
    let unresolved_sketch_feature_count = ir
        .model
        .features
        .iter()
        .filter(|feature| feature_in_active_scope(feature))
        .filter(|feature| {
            matches!(
                feature.definition,
                FeatureDefinition::Sketch { sketch: None, .. }
            )
        })
        .count();
    if unresolved_sketch_feature_count != 0 {
        losses.push(NxLossCode::SketchGraphUnresolved.note(format!(
            "Decoded {sketch_feature_count} NX sketch history feature(s), of which \
                 {unresolved_sketch_feature_count} have no neutral sketch graph because complete \
                 sketch placement and entity semantics are unresolved."
        )));
    }

    let active_sketch_ids = ir
        .model
        .features
        .iter()
        .filter(|feature| feature_in_active_scope(feature))
        .filter_map(|feature| match &feature.definition {
            FeatureDefinition::Sketch {
                sketch: Some(sketch),
                ..
            } => Some(sketch.clone()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let sketch_in_active_scope = |sketch: &cadmpeg_ir::sketches::SketchId| {
        active_features.is_none() || active_sketch_ids.contains(sketch)
    };
    let native_sketch_entity_count = ir
        .model
        .sketch_entities
        .iter()
        .filter(|entity| sketch_in_active_scope(&entity.sketch))
        .filter(|entity| {
            matches!(
                entity.geometry,
                cadmpeg_ir::sketches::SketchGeometry::Native { .. }
            )
        })
        .count();
    let native_sketch_constraint_count = ir
        .model
        .sketch_constraints
        .iter()
        .filter(|constraint| sketch_in_active_scope(&constraint.sketch))
        .filter(|constraint| {
            matches!(
                constraint.definition,
                cadmpeg_ir::sketches::SketchConstraintDefinition::Native { .. }
            )
        })
        .count();
    if native_sketch_entity_count != 0 || native_sketch_constraint_count != 0 {
        losses.push(NxLossCode::SketchNativeSemantics.note(format!(
            "Neutral semantics remain unresolved for {native_sketch_entity_count} NX sketch \
                 geometry record(s) and {native_sketch_constraint_count} sketch constraint \
                 record(s)."
        )));
    }
}
