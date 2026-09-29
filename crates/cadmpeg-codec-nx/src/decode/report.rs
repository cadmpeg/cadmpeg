// SPDX-License-Identifier: Apache-2.0
//! Geometry-report losses for NX decode.

use super::feature_completeness::operands::{
    body_selection_is_incomplete, face_selection_is_incomplete, path_ref_is_incomplete,
    pattern_feature_is_incomplete,
};
use super::feature_completeness::{
    active_configuration_state_is_incomplete_for_decode, chamfer_definition_is_incomplete,
    combine_definition_is_incomplete, datum_coordinate_system_is_incomplete,
    delete_body_definition_is_incomplete, draft_definition_is_incomplete,
    extend_surface_definition_is_incomplete, extrude_definition_is_incomplete,
    face_blend_definition_is_incomplete, fillet_definition_is_incomplete,
    hole_definition_is_incomplete, incomplete_expression_parameters, loft_definition_is_incomplete,
    offset_surface_definition_is_incomplete, output_free_local_body_construction,
    output_free_native_snapshot, output_free_pattern_construction,
    output_free_trim_surface_construction, projected_curve_direction_is_incomplete,
    replace_face_definition_is_incomplete, revolve_definition_is_incomplete,
    rib_definition_is_incomplete, sew_bodies_definition_is_incomplete,
    shell_definition_is_incomplete, sphere_definition_is_incomplete,
    sweep_definition_is_incomplete, thicken_definition_is_incomplete,
    trim_bodies_definition_is_incomplete, trim_surface_definition_is_incomplete,
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
    BodySelection, BooleanOp, DatumPlaneReference, Feature, FeatureDefinition, FeatureOperation,
    UnresolvedFamily,
};
use cadmpeg_ir::report::loss::LossNote;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display};

fn push_report_loss(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    code: NxLossCode,
    message: fmt::Arguments<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let message = ctx.format_retained(message, "nx geometry report loss text")?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index("nx".len()),
        "nx geometry report loss namespace",
    )?;
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(code.code().len()),
        "nx geometry report loss code",
    )?;
    ctx.reserve_vec(losses, 1, "nx geometry report losses")?;
    losses.push(code.note(message));
    Ok(())
}

struct JoinedLabels<'a> {
    labels: &'a [Option<&'static str>],
    separator: &'static str,
}

impl Display for JoinedLabels<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for label in self.labels.iter().flatten() {
            if !first {
                formatter.write_str(self.separator)?;
            }
            formatter.write_str(label)?;
            first = false;
        }
        Ok(())
    }
}

struct JoinedCounts<'a> {
    counts: &'a BTreeMap<&'static str, usize>,
    separator: &'static str,
}

struct JoinedCountLabels<'a> {
    counts: &'a BTreeMap<&'a str, usize>,
}

impl Display for JoinedCountLabels<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for (family, count) in self.counts {
            if !first {
                formatter.write_str(", ")?;
            }
            write!(formatter, "{family} ({count})")?;
            first = false;
        }
        Ok(())
    }
}

struct ClosureDetail<'a>(Option<&'a str>);

impl Display for ClosureDetail<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(reason) = self.0 {
            write!(
                formatter,
                " Active-feature closure rejected with `{reason}`."
            )?;
        }
        Ok(())
    }
}

impl Display for JoinedCounts<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for (family, count) in self.counts {
            if !first {
                formatter.write_str(self.separator)?;
            }
            write!(formatter, "{family} {count}")?;
            first = false;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct PcurveCompletionStatus {
    pub(super) exact_boundary_exhausted: bool,
    pub(super) transfer_exhausted: bool,
    pub(super) geometry_exhausted: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct SupportUvPhaseStatus {
    pub(super) samples_exhausted: bool,
    pub(super) geometry_exhausted: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct CompletionBudgetStatus {
    pub(super) pcurves: PcurveCompletionStatus,
    pub(super) serialized: SupportUvPhaseStatus,
    pub(super) direct: SupportUvPhaseStatus,
    pub(super) coupled: SupportUvPhaseStatus,
    pub(super) support_uv_lane_geometry_exhausted: bool,
    pub(super) transfer_limit: usize,
    pub(super) support_uv_limit: usize,
}

// Keep the independent report facts explicit at the decode/report boundary.

pub(super) struct GeometryReportFacts<'inputs> {
    pub(super) unmatched_delta_tombstone_counts: &'inputs BTreeMap<&'static str, usize>,
    pub(super) counts: &'inputs Counts,
    pub(super) has_topology: bool,
    pub(super) has_unresolved_sub_bodies: bool,
    pub(super) tessellation_count: usize,
    pub(super) completion_budget: CompletionBudgetStatus,
    pub(super) adaptive_geometry_exhausted: bool,
    pub(super) dialect_losses: &'inputs [LossNote],
    pub(super) notes: &'inputs [String],
}

pub(super) fn build_geometry_report(
ctx: &cadmpeg_core::decode::DecodeContext<'_>,
scan: &Scan,
geometry_report_facts: GeometryReportFacts<'_>,
ir: &CadIr,
model: &crate::native::model::NativeModel,
) -> Result<DecodeBody, cadmpeg_core::CodecError> {
    let GeometryReportFacts { unmatched_delta_tombstone_counts, counts, has_topology, has_unresolved_sub_bodies, tessellation_count, completion_budget, adaptive_geometry_exhausted, dialect_losses, notes } = geometry_report_facts;

    let has_untransferred_attribute_fields =
        model.has_untransferred_parasolid_attribute_fields(ctx)?;
    let mut losses = Vec::new();

    push_report_loss(
        ctx,
        &mut losses,
        NxLossCode::CarrierAnalyticCensus,
        format_args!(
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
        ),
    )?;

    if tessellation_count != 0 {
        push_report_loss(ctx, &mut losses, NxLossCode::CarrierTessellationCensus, format_args!(
            "Decoded {tessellation_count} embedded JT display tessellation(s) with scene-node ownership, model-space coordinates, topological triangle connectivity, and corner normals when bound."
        ))?;
    }

    if !has_topology {
        push_report_loss(ctx, &mut losses, NxLossCode::TopologyGraphNotReconstructed, format_args!(
            "The B-rep topology graph (body→shell→face→loop→fin→edge→vertex) was not \
                      reconstructed because the surviving typed records did not form a complete \
                      connected ownership graph. Exact-key supported partition↔deltas replacements \
                      and deletions were applied before graph construction. Required unresolved \
                      records prevent their dependent incidence from being emitted; decoded geometry \
                      then remains unattached.",
        ))?;
    }

    if counts.intersection_rejections.total() > 0 {
        push_report_loss(ctx, &mut losses, NxLossCode::IntersectionRecordsOpaque, format_args!(
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
        ))?;
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
                    .sides()
                    .iter()
                    .filter(|side| {
                        pcurve_requires_completion(
                            side.pcurve.as_ref().map(|pcurve| &pcurve.geometry),
                        )
                    })
                    .count(),
            )
        })
        .sum::<usize>();
    if unresolved_intersection_lanes > 0
        && (completion_budget.pcurves.exact_boundary_exhausted
            || completion_budget.pcurves.transfer_exhausted
            || completion_budget.serialized.samples_exhausted
            || completion_budget.direct.samples_exhausted
            || completion_budget.coupled.samples_exhausted
            || completion_budget.pcurves.geometry_exhausted
            || completion_budget.serialized.geometry_exhausted
            || completion_budget.direct.geometry_exhausted
            || completion_budget.coupled.geometry_exhausted
            || completion_budget.support_uv_lane_geometry_exhausted)
    {
        let bounded_phases = [
            completion_budget
                .pcurves.exact_boundary_exhausted
                .then_some("exact-boundary transfer"),
            completion_budget
                .pcurves.transfer_exhausted
                .then_some("opposite-chart transfer"),
            completion_budget
                .serialized.samples_exhausted
                .then_some("support-UV consistency checks"),
            completion_budget
                .direct.samples_exhausted
                .then_some("EXT11 support-UV fitting"),
            completion_budget
                .coupled.samples_exhausted
                .then_some("coupled EXT11 support-UV fitting"),
            completion_budget
                .pcurves.geometry_exhausted
                .then_some("pcurve geometry fitting"),
            completion_budget
                .serialized.geometry_exhausted
                .then_some("serialized support-UV geometry fitting"),
            completion_budget
                .direct.geometry_exhausted
                .then_some("support-UV geometry fitting"),
            completion_budget
                .coupled.geometry_exhausted
                .then_some("coupled support-UV geometry fitting"),
            completion_budget
                .support_uv_lane_geometry_exhausted
                .then_some("support-UV lane geometry slices"),
        ];
        push_report_loss(ctx, &mut losses, NxLossCode::IntersectionPcurveCompletionBounded, format_args!(
            "Model-wide geometric completion stopped at its bounded work budget for {} ({} exact-boundary transfer samples, {} opposite-chart transfer samples, {} support-UV consistency checks, {} support-UV point fits, {} coupled support-UV point fits, {} pcurve geometry evaluations, {} serialized support-UV geometry evaluations, {} support-UV geometry evaluations, {} coupled support-UV geometry evaluations); {} intersection pcurve lane(s) remain incomplete and were not emitted as completed parameterizations.",
            JoinedLabels { labels: &bounded_phases, separator: " and " },
            MAX_EXACT_BOUNDARY_TRANSFER_SAMPLES,
            completion_budget.transfer_limit,
            completion_budget.support_uv_limit,
            completion_budget.support_uv_limit,
            completion_budget.support_uv_limit,
            MAX_PCURVE_COMPLETION_GEOMETRY_WORK,
            MAX_SERIALIZED_SUPPORT_UV_GEOMETRY_WORK,
            MAX_SUPPORT_UV_COMPLETION_GEOMETRY_WORK,
            MAX_COUPLED_SUPPORT_UV_GEOMETRY_WORK,
            unresolved_intersection_lanes,
        ))?;
    }

    if adaptive_geometry_exhausted {
        push_report_loss(ctx, &mut losses, NxLossCode::GeometryAdaptiveWorkBounded, format_args!(
            "Model-wide adaptive geometry certification stopped at its {MAX_ADAPTIVE_GEOMETRY_WORK}-unit work bound; \
             unresolved adaptive geometry certification results were left untyped.",
        ))?;
    }

    if scan.count(StreamKind::Deltas) > 0 {
        let unmatched_tombstones = unmatched_delta_tombstone_counts.values().sum::<usize>();
        let unmatched_tombstone_detail = JoinedCounts {
            counts: unmatched_delta_tombstone_counts,
            separator: ", ",
        };
        if unmatched_tombstones == 0 {
            push_report_loss(ctx, &mut losses, NxLossCode::DeltasApplied, format_args!(
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
            ))?;
        } else {
            push_report_loss(ctx, &mut losses, NxLossCode::DeltasUnmatchedTombstones, format_args!(
                "{} Parasolid deltas stream(s) were processed in validated UG_PART segment order. \
                    Equal-schema deltas were paired with the preceding partition. Exact-key revisions in current body-sequence intervals were applied using the last \
                 event for each key, but {unmatched_tombstones} terminal tombstone(s) have no exact \
                 current or earlier-added key and remain unresolved: {unmatched_tombstone_detail}.",
                scan.count(StreamKind::Deltas)
            ))?;
        }
    }

    if has_unresolved_sub_bodies {
        push_report_loss(
            ctx,
            &mut losses,
            NxLossCode::SubBodyCompositionUnresolved,
            format_args!(
                "This part is composed of {} sub-body partition(s); its decoded feature-history \
                 Booleans do not resolve every intermediate body object to a partition image. \
                 Carriers from all sub-bodies are emitted without the unresolved composition that \
                 would remove interior/construction faces.",
                scan.count(StreamKind::Partition)
            ),
        )?;
    }

    append_design_intent_losses(ctx, ir, &mut losses)?;

    if has_untransferred_attribute_fields {
        push_report_loss(
            ctx,
            &mut losses,
            NxLossCode::AttributeValueUnresolved,
            format_args!(
                "A referenced Parasolid attribute value was not transferred because its \
                      complete value relation did not resolve.",
            ),
        )?;
    }

    for loss in dialect_losses {
        ctx.reserve_vec(&mut losses, 1, "nx geometry report losses")?;
        losses.push(loss.clone_admitted(ctx, "nx geometry report dialect loss")?);
    }
    let mut copied_notes = ctx.collection_vec(notes.len(), "nx geometry report notes")?;
    for note in notes {
        let bytes = ctx.copy_retained(note.as_bytes(), "nx geometry report note text")?;
        copied_notes.push(String::from_utf8(bytes).map_err(cadmpeg_core::CodecError::malformed)?);
    }
    Ok(DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses,
        notes: copied_notes,
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    })
}

pub(crate) fn append_design_intent_losses(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut current_body_ids =
        ctx.collection_vec(ir.model.bodies.len(), "nx report current body identities")?;
    for body in &ir.model.bodies {
        current_body_ids.push(
            body.id
                .try_clone_for_decode(ctx, "nx report current body identity")?,
        );
    }
    // Require a non-BaseFeature writer before treating body-to-history as proven.
    let (active_features, closure_rejection) =
        match crate::native::history::active_feature_closure_for_decode(ctx, ir, &current_body_ids)?
        {
            Ok(active) => (Some(active), None),
            Err(rejection) => (None, Some(rejection.code())),
        };
    let active_features = active_features.filter(|active| {
        active.values().any(|&index| {
            !matches!(
                ir.model.features[index].evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::BaseFeature { .. })
            )
        })
    });
    let suppression_scope = active_features.as_ref().map_or("", |_| "active ");
    let feature_in_active_scope = |feature: &Feature| {
        active_features
            .as_ref()
            .is_none_or(|active| active.contains_key(&feature.id))
    };
    let unresolved_suppression_count = ir
        .model
        .features
        .iter()
        .filter(|feature| {
            feature.suppressed.is_none()
                && active_features
                    .as_ref()
                    .is_none_or(|active| active.contains_key(&feature.id))
        })
        .count();
    if unresolved_suppression_count != 0 {
        let closure_detail = ClosureDetail(closure_rejection);
        push_report_loss(
            ctx,
            losses,
            NxLossCode::FeatureSuppressionUnresolved,
            format_args!(
                "Suppression state remains unresolved for {unresolved_suppression_count} NX \
                 {suppression_scope}feature history operation(s): no admitted \
                 operation-to-state-object-to-typed-value relation is present. Common-frame \
                 state lanes, saved toggles, OM registry declarations, and topology ObjectState \
                 values remain non-suppression evidence.{closure_detail}"
            ),
        )?;
    }

    let active_configuration_count = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.active)
        .count();
    let mut current_bodies = BTreeSet::new();
    for body in &ir.model.bodies {
        ctx.charge_collection_items(1, "nx report current body set")?;
        current_bodies.insert(&body.id);
    }
    let mut incomplete_configuration_count = 0usize;
    for configuration in &ir.model.configurations {
        let incomplete = configuration.bodies.is_none()
            || active_configuration_count != 1
            || (configuration.active
                && configuration.bodies.as_deref().is_none_or(|bodies| {
                    bodies.len() != current_bodies.len()
                        || current_bodies.iter().any(|body| !bodies.contains(body))
                }))
            || (configuration.active
                && active_configuration_state_is_incomplete_for_decode(ctx, ir, configuration)?);
        if incomplete {
            incomplete_configuration_count += 1;
        }
    }
    if incomplete_configuration_count != 0 {
        push_report_loss(
            ctx,
            losses,
            NxLossCode::ConfigurationStateUnresolved,
            format_args!(
                "Activation, complete body membership, evaluated feature state, or evaluated \
                 parameter state remains unresolved for {incomplete_configuration_count} NX \
                 design configuration(s)."
            ),
        )?;
    }

    let incomplete_expression_count = incomplete_expression_parameters(ctx, ir)?.len();
    if incomplete_expression_count != 0 {
        push_report_loss(
            ctx,
            losses,
            NxLossCode::ExpressionParameterIncomplete,
            format_args!(
                "Neutral evaluation or dependency semantics remain incomplete for \
                 {incomplete_expression_count} NX expression parameter(s)."
            ),
        )?;
    }

    let mut native_feature_kinds = BTreeMap::<&str, usize>::new();
    for feature in &ir.model.features {
        if !feature_in_active_scope(feature) {
            continue;
        }
        if let FeatureDefinition::Operation(FeatureOperation::Native { kind, .. }) =
            feature.evaluation.definition()
        {
            match native_feature_kinds.entry(kind.as_str()) {
                std::collections::btree_map::Entry::Occupied(mut entry) => *entry.get_mut() += 1,
                std::collections::btree_map::Entry::Vacant(entry) => {
                    ctx.charge_collection_items(1, "nx report native feature kinds")?;
                    entry.insert(1);
                }
            }
        }
    }
    if !native_feature_kinds.is_empty() {
        let kinds = JoinedCountLabels {
            counts: &native_feature_kinds,
        };
        push_report_loss(
            ctx,
            losses,
            NxLossCode::FeatureNativeKindRetained,
            format_args!(
            "NX feature-history operation(s) remain native-only because their complete neutral \
                 operation semantics are not decoded: {kinds}."
        ),
        )?;
    }

    let mut unresolved_feature_families = BTreeMap::<&str, usize>::new();
    for feature in &ir.model.features {
        if !feature_in_active_scope(feature) {
            continue;
        }
        let family = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Unresolved { family }) => match family {
                UnresolvedFamily::Brep => "brep",
                UnresolvedFamily::DatumPlane => "datum plane",
                UnresolvedFamily::DatumAxis => "datum axis",
                UnresolvedFamily::DatumPoint => "datum point",
                UnresolvedFamily::DatumCoordinateSystem => "datum coordinate system",
                UnresolvedFamily::BridgeCurve => "bridge curve",
                UnresolvedFamily::Loft => "loft",
                UnresolvedFamily::ThroughCurveMesh => "through curve mesh",
                UnresolvedFamily::FreeformSurface => "freeform surface",
                UnresolvedFamily::ExtractFace => "extract face",
                UnresolvedFamily::CopyFace => "copy face",
                UnresolvedFamily::LinkedFace => "linked face",
                UnresolvedFamily::FillHole => "fill hole",
                UnresolvedFamily::MoveFace => "move face",
                UnresolvedFamily::MoveObject => "move object",
                UnresolvedFamily::Cylinder => "cylinder",
                UnresolvedFamily::Cone => "cone",
                UnresolvedFamily::Sphere => "sphere",
                UnresolvedFamily::Thread => "thread",
                UnresolvedFamily::DetailedThread => "detailed thread",
                UnresolvedFamily::Draft => "draft",
                UnresolvedFamily::DeleteFace => "delete face",
                UnresolvedFamily::MirrorFace => "mirror face",
                UnresolvedFamily::SubdivisionBody => "subdivision body",
                UnresolvedFamily::TopologyOptimization => "topology optimization",
                UnresolvedFamily::Extrude
                | UnresolvedFamily::Revolve
                | UnresolvedFamily::Fillet
                | UnresolvedFamily::BoundarySurface => continue,
            },
            _ => continue,
        };
        match unresolved_feature_families.entry(family) {
            std::collections::btree_map::Entry::Occupied(mut entry) => *entry.get_mut() += 1,
            std::collections::btree_map::Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "nx report unresolved feature families")?;
                entry.insert(1);
            }
        }
    }
    if !unresolved_feature_families.is_empty() {
        let families = JoinedCountLabels {
            counts: &unresolved_feature_families,
        };
        push_report_loss(
            ctx,
            losses,
            NxLossCode::FeatureFamilyConstructionUnresolved,
            format_args!(
                "NX feature family identities were transferred, but their neutral construction \
                 semantics remain unresolved: {families}."
            ),
        )?;
    }

    let mut incomplete_feature_output_families = BTreeMap::<&str, usize>::new();
    let mut incomplete_feature_construction_families = BTreeMap::<&str, usize>::new();
    let mut generated_body_outputs = BTreeSet::new();
    for state in &ir.model.feature_result_topologies {
        if !state.bodies().is_empty() {
            ctx.charge_collection_items(1, "nx report generated body outputs")?;
            generated_body_outputs.insert(&state.output_of);
        }
    }
    for feature in &ir.model.features {
        if !feature_in_active_scope(feature) {
            continue;
        }
        let is_exact_empty_base = matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Resolved { bodies, native },
            }) if bodies.is_empty() && !native.trim().is_empty() && feature.evaluation.outputs().is_empty()
        );
        if feature.suppressed != Some(true)
            && !is_exact_empty_base
            && !output_free_native_snapshot(feature)
            && !output_free_local_body_construction(feature)
            && !output_free_pattern_construction(feature)
            && !output_free_trim_surface_construction(feature)
        {
            if let Some(family) =
                feature
                    .evaluation
                    .definition()
                    .body_output_family()
                    .filter(|_| {
                        let current_outputs_are_valid = !feature.evaluation.outputs().is_empty()
                            && feature.evaluation.outputs().iter().all(|output| {
                                ir.model.bodies.iter().any(|body| body.id == *output)
                            });
                        !(current_outputs_are_valid
                            || feature.evaluation.outputs().is_empty()
                                && generated_body_outputs.contains(&feature.id))
                    })
            {
                match incomplete_feature_output_families.entry(family) {
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        *entry.get_mut() += 1;
                    }
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        ctx.charge_collection_items(1, "nx report incomplete output families")?;
                        entry.insert(1);
                    }
                }
                continue;
            }
        }
        let family = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::BaseFeature { bodies })
                if !is_exact_empty_base
                    && !output_free_native_snapshot(feature)
                    && body_selection_is_incomplete(bodies) =>
            {
                "base feature"
            }
            FeatureDefinition::Operation(FeatureOperation::Block {
                dimensions,
                placement,
                op,
            }) if dimensions.is_none()
                || placement.is_none()
                || matches!(op, BooleanOp::Unresolved) =>
            {
                "block"
            }
            FeatureDefinition::Operation(FeatureOperation::Sphere { .. })
                if sphere_definition_is_incomplete(feature) =>
            {
                "sphere"
            }
            FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
                reference, ..
            }) if reference.as_ref().is_none_or(|reference| match reference {
                DatumPlaneReference::Feature { feature: reference } => {
                    ir.model
                        .features
                        .iter()
                        .find(|candidate| candidate.id == *reference)
                        .is_none_or(|source| source.ordinal >= feature.ordinal)
                        || !feature.dependencies.contains(reference)
                }
                DatumPlaneReference::Face { face } => face_selection_is_incomplete(face),
                DatumPlaneReference::ResolvedPlane { .. } => false,
            }) =>
            {
                "datum plane"
            }
            FeatureDefinition::Operation(FeatureOperation::DatumCoordinateSystem { frame })
                if datum_coordinate_system_is_incomplete(
                    frame.x_axis(),
                    frame.y_axis(),
                    frame.z_axis(),
                ) =>
            {
                "datum coordinate system"
            }
            FeatureDefinition::Operation(FeatureOperation::ExtractBody { source })
                if body_selection_is_incomplete(source) =>
            {
                "extract body"
            }
            FeatureDefinition::Operation(FeatureOperation::Sketch { sketch })
                if sketch.id().is_none_or(|sketch| {
                    ir.model
                        .sketches
                        .iter()
                        .find(|candidate| candidate.id == *sketch)
                        .is_none_or(|sketch| {
                            matches!(
                                sketch.placement,
                                cadmpeg_ir::sketches::SketchPlacement::Unresolved {}
                            )
                        })
                }) =>
            {
                "sketch"
            }
            FeatureDefinition::Operation(FeatureOperation::Loft { .. })
                if loft_definition_is_incomplete(feature) =>
            {
                "loft"
            }
            FeatureDefinition::Operation(FeatureOperation::ProjectedCurve {
                source,
                target_faces,
                direction,
                bidirectional,
            }) if path_ref_is_incomplete(source)
                || face_selection_is_incomplete(target_faces)
                || projected_curve_direction_is_incomplete(*direction)
                || bidirectional.is_none() =>
            {
                "projected curve"
            }
            FeatureDefinition::Operation(FeatureOperation::TrimSurface { .. })
                if trim_surface_definition_is_incomplete(feature) =>
            {
                "trim surface"
            }
            FeatureDefinition::Operation(FeatureOperation::ExtendSurface { .. })
                if extend_surface_definition_is_incomplete(feature) =>
            {
                "extend surface"
            }
            FeatureDefinition::Operation(FeatureOperation::CosmeticThread {
                face,
                diameter,
                extent,
            }) if face_selection_is_incomplete(face) || diameter.is_none() || extent.is_none() => {
                "cosmetic thread"
            }
            FeatureDefinition::Operation(FeatureOperation::Hole { .. })
                if hole_definition_is_incomplete(feature) =>
            {
                "hole"
            }
            FeatureDefinition::Operation(FeatureOperation::Rib { .. })
                if rib_definition_is_incomplete(feature) =>
            {
                "rib"
            }
            FeatureDefinition::Operation(FeatureOperation::Chamfer { .. })
                if chamfer_definition_is_incomplete(feature) =>
            {
                "chamfer"
            }
            FeatureDefinition::Operation(FeatureOperation::Fillet { .. })
                if fillet_definition_is_incomplete(feature) =>
            {
                "fillet"
            }
            FeatureDefinition::Operation(FeatureOperation::FaceBlend { .. })
                if face_blend_definition_is_incomplete(feature) =>
            {
                "face blend"
            }
            FeatureDefinition::Operation(FeatureOperation::Shell { .. })
                if shell_definition_is_incomplete(feature.evaluation.definition()) =>
            {
                "shell"
            }
            FeatureDefinition::Operation(FeatureOperation::SewBodies { .. })
                if sew_bodies_definition_is_incomplete(feature) =>
            {
                "sew bodies"
            }
            FeatureDefinition::Operation(FeatureOperation::TrimBodies { .. })
                if trim_bodies_definition_is_incomplete(feature) =>
            {
                "trim bodies"
            }
            FeatureDefinition::Operation(FeatureOperation::Extrude { .. })
                if extrude_definition_is_incomplete(feature) =>
            {
                "extrude"
            }
            FeatureDefinition::Operation(FeatureOperation::Revolve { .. })
                if revolve_definition_is_incomplete(feature) =>
            {
                "revolve"
            }
            FeatureDefinition::Operation(FeatureOperation::Sweep { .. })
                if sweep_definition_is_incomplete(feature) =>
            {
                "sweep"
            }
            FeatureDefinition::Operation(FeatureOperation::OffsetSurface { .. })
                if offset_surface_definition_is_incomplete(feature) =>
            {
                "offset surface"
            }
            FeatureDefinition::Operation(FeatureOperation::Thicken { .. })
                if thicken_definition_is_incomplete(feature) =>
            {
                "thicken"
            }
            FeatureDefinition::Operation(FeatureOperation::Draft { .. })
                if draft_definition_is_incomplete(feature) =>
            {
                "draft"
            }
            FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern })
                if pattern_feature_is_incomplete(seeds, pattern, &feature.dependencies) =>
            {
                "pattern"
            }
            FeatureDefinition::Operation(FeatureOperation::SectionShape {
                operands,
                approximate,
            }) if body_selection_is_incomplete(operands.first())
                || body_selection_is_incomplete(operands.second())
                || approximate.is_none() =>
            {
                "section"
            }
            FeatureDefinition::Operation(FeatureOperation::Combine { .. })
                if combine_definition_is_incomplete(feature) =>
            {
                "body combine"
            }
            FeatureDefinition::Operation(FeatureOperation::DeleteBody { .. })
                if delete_body_definition_is_incomplete(feature) =>
            {
                "delete body"
            }
            FeatureDefinition::Operation(FeatureOperation::ReplaceFace { .. })
                if replace_face_definition_is_incomplete(feature) =>
            {
                "replace face"
            }
            _ => continue,
        };
        match incomplete_feature_construction_families.entry(family) {
            std::collections::btree_map::Entry::Occupied(mut entry) => *entry.get_mut() += 1,
            std::collections::btree_map::Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "nx report incomplete construction families")?;
                entry.insert(1);
            }
        }
    }
    if !incomplete_feature_output_families.is_empty() {
        let families = JoinedCountLabels {
            counts: &incomplete_feature_output_families,
        };
        push_report_loss(
            ctx,
            losses,
            NxLossCode::FeatureOutputLineageIncomplete,
            format_args!(
                "NX typed feature operation output lineage is missing, duplicated, or does not \
                 resolve to a transferred body: {families}."
            ),
        )?;
    }
    if !incomplete_feature_construction_families.is_empty() {
        let families = JoinedCountLabels {
            counts: &incomplete_feature_construction_families,
        };
        push_report_loss(
            ctx,
            losses,
            NxLossCode::FeatureConstructionIncomplete,
            format_args!(
                "NX typed feature operations have incomplete neutral construction fields: \
                 {families}."
            ),
        )?;
    }

    let sketch_feature_count = ir
        .model
        .features
        .iter()
        .filter(|feature| feature_in_active_scope(feature))
        .filter(|feature| {
            matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Sketch { .. })
            )
        })
        .count();
    let unresolved_sketch_feature_count = ir
        .model
        .features
        .iter()
        .filter(|feature| feature_in_active_scope(feature))
        .filter(|feature| {
            matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved
                        | cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
                    ..
                })
            )
        })
        .count();
    if unresolved_sketch_feature_count != 0 {
        push_report_loss(
            ctx,
            losses,
            NxLossCode::SketchGraphUnresolved,
            format_args!(
                "Decoded {sketch_feature_count} NX sketch history feature(s), of which \
                 {unresolved_sketch_feature_count} have no neutral sketch graph because complete \
                 sketch placement and entity semantics are unresolved."
            ),
        )?;
    }

    let mut active_sketch_ids = BTreeSet::<cadmpeg_ir::sketches::SketchId>::new();
    for feature in ir
        .model
        .features
        .iter()
        .filter(|feature| feature_in_active_scope(feature))
    {
        if let FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            ..
        }) = feature.evaluation.definition()
        {
            ctx.charge_collection_items(1, "nx report active sketch identities")?;
            active_sketch_ids
                .insert(sketch.try_clone_for_decode(ctx, "nx report active sketch identity")?);
        }
    }
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
                *entity.geometry.definition(),
                cadmpeg_ir::sketches::SketchGeometryDefinition::Native { .. }
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
                constraint.definition.kind(),
                cadmpeg_ir::sketches::SketchConstraintDefinitionInput::Native { .. }
            )
        })
        .count();
    if native_sketch_entity_count != 0 || native_sketch_constraint_count != 0 {
        push_report_loss(
            ctx,
            losses,
            NxLossCode::SketchNativeSemantics,
            format_args!(
                "Neutral semantics remain unresolved for {native_sketch_entity_count} NX sketch \
                 geometry record(s) and {native_sketch_constraint_count} sketch constraint \
                 record(s)."
            ),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::append_design_intent_losses;
    use cadmpeg_core::decode::{ResourceDimension};
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::ids::BodyId;
    use cadmpeg_ir::topology::{Body, BodyKind};

    fn body_ir() -> CadIr {
        let mut ir = CadIr::empty();
        ir.model.bodies.push(Body {
            id: BodyId::mint("test:model:entity#report-body").expect("identity grammar"),
            kind: BodyKind::Solid,
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        ir
    }

    #[test]
    fn design_intent_report_refuses_body_list_at_collection_limit() {
        
        
        
        crate::test_support::with_decode_context_over(&[], |policy| { policy.limits.max_collection_items = 0; }, |ctx| {

        assert!(matches!(
            append_design_intent_losses(ctx, &body_ir(), &mut Vec::new()),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "nx report current body identities"
        ));
    
})
}

    #[test]
    fn design_intent_report_refuses_body_copy_at_retained_limit() {
        
        
        
        crate::test_support::with_decode_context_over(&[], |policy| { policy.limits.max_retained_bytes = 0; }, |ctx| {

        assert!(matches!(
            append_design_intent_losses(ctx, &body_ir(), &mut Vec::new()),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "nx report current body identity"
        ));
    
})
}
}
