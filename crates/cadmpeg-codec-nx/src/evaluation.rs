// SPDX-License-Identifier: Apache-2.0
//! Neutral evaluation of Siemens NX feature-history effects.

use std::collections::BTreeSet;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    BodyRetentionMode, BodySelection, BooleanOp, FeatureDefinition, FeatureId, Length, PatternSeed,
};
use cadmpeg_ir::ids::BodyId;

use crate::decode::{output_free_local_body_construction, output_free_native_snapshot};

/// Why a saved-body census cannot yet be evaluated exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnsupportedBodyCensusReason {
    /// A feature's active or suppressed state is unresolved.
    UnresolvedSuppression,
    /// The neutral feature family has no admitted body-effect evaluator.
    UnsupportedFeatureDefinition,
    /// Required construction semantics remain incomplete.
    IncompleteFeatureDefinition,
    /// Feature outputs do not form a coherent body-identity transition.
    InvalidOutputLineage,
    /// Feature ordinals or dependency directions do not form a replay order.
    InvalidHistoryOrder,
    /// The active configuration requires configuration-local evaluation.
    ConfigurationEvaluation,
}

/// Result of evaluating neutral history against the saved current-body census.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BodyCensusEvaluation {
    /// Neutral evaluation produced exactly the saved body identities.
    Verified {
        /// Re-derived body identities in canonical order.
        bodies: Vec<BodyId>,
    },
    /// Exact evaluation stopped at an unsupported semantic boundary.
    Unsupported {
        /// Feature at the boundary, or `None` for configuration-level state.
        feature: Option<FeatureId>,
        /// Semantic boundary that prevented exact evaluation.
        reason: UnsupportedBodyCensusReason,
    },
    /// Evaluation completed, but its body identities differ from the saved model.
    Mismatch {
        /// Re-derived body identities in canonical order.
        rederived: Vec<BodyId>,
        /// Saved body identities in canonical order.
        saved: Vec<BodyId>,
    },
}

impl BodyCensusEvaluation {
    /// Whether neutral evaluation exactly reproduced the saved body census.
    pub const fn is_verified(&self) -> bool {
        matches!(self, Self::Verified { .. })
    }
}

/// Evaluate admitted neutral feature effects and compare their body identities
/// with the saved current model.
///
/// The caller must validate the IR first. This evaluator checks replay order,
/// operation completeness, and body lineage; it does not repeat topology or
/// selection-target validation.
pub fn evaluate_saved_body_census(ir: &CadIr) -> BodyCensusEvaluation {
    let rederived = match rederived_body_census(ir) {
        Ok(bodies) => bodies,
        Err((feature, reason)) => {
            return BodyCensusEvaluation::Unsupported {
                feature: Some(feature),
                reason,
            };
        }
    };
    let saved = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<BTreeSet<_>>();
    if saved.len() != ir.model.bodies.len() || rederived != saved {
        return BodyCensusEvaluation::Mismatch {
            rederived: rederived.into_iter().collect(),
            saved: saved.into_iter().collect(),
        };
    }

    if !active_configuration_is_admitted(ir, &saved) {
        return BodyCensusEvaluation::Unsupported {
            feature: None,
            reason: UnsupportedBodyCensusReason::ConfigurationEvaluation,
        };
    }
    BodyCensusEvaluation::Verified {
        bodies: rederived.into_iter().collect(),
    }
}

/// Saved-body census evidence for the profile harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyCensusEvidence {
    /// Whether neutral evaluation exactly reproduced the saved body census.
    pub verified: bool,
    /// Semantic boundary or mismatch code; absent when verified.
    pub reason: Option<String>,
    /// Feature identity at the boundary, if any.
    pub feature: Option<String>,
    /// Feature display name at the boundary, if any.
    pub feature_name: Option<String>,
    /// Feature definition family at the boundary, if any.
    pub feature_family: Option<String>,
    /// Feature history ordinal at the boundary, if any.
    pub feature_ordinal: Option<u64>,
}

/// Saved-body census evidence for the profile harness.
#[doc(hidden)]
pub fn saved_body_census_evidence(ir: &CadIr) -> BodyCensusEvidence {
    let evaluation = evaluate_saved_body_census(ir);
    let verified = evaluation.is_verified();
    match evaluation {
        BodyCensusEvaluation::Verified { .. } => BodyCensusEvidence {
            verified,
            reason: None,
            feature: None,
            feature_name: None,
            feature_family: None,
            feature_ordinal: None,
        },
        BodyCensusEvaluation::Mismatch { .. } => BodyCensusEvidence {
            verified,
            reason: Some("saved_body_census_mismatch".to_string()),
            feature: None,
            feature_name: None,
            feature_family: None,
            feature_ordinal: None,
        },
        BodyCensusEvaluation::Unsupported { feature, reason } => {
            let boundary_feature = feature.as_ref().and_then(|id| {
                ir.model
                    .features
                    .iter()
                    .find(|candidate| candidate.id == *id)
            });
            let feature_name = boundary_feature.and_then(|feature| feature.name.clone());
            let feature_family = boundary_feature.and_then(|feature| {
                serde_json::to_value(&feature.definition)
                    .ok()?
                    .get("definition")?
                    .as_str()
                    .map(str::to_string)
            });
            let feature_ordinal = boundary_feature.map(|feature| feature.ordinal);
            let reason = match reason {
                UnsupportedBodyCensusReason::UnresolvedSuppression => "unresolved_suppression",
                UnsupportedBodyCensusReason::UnsupportedFeatureDefinition => {
                    "unsupported_feature_definition"
                }
                UnsupportedBodyCensusReason::IncompleteFeatureDefinition => {
                    "incomplete_feature_definition"
                }
                UnsupportedBodyCensusReason::InvalidOutputLineage => "invalid_output_lineage",
                UnsupportedBodyCensusReason::InvalidHistoryOrder => "invalid_history_order",
                UnsupportedBodyCensusReason::ConfigurationEvaluation => "configuration_evaluation",
            };
            BodyCensusEvidence {
                verified,
                reason: Some(reason.to_string()),
                feature: feature.map(|id| id.0),
                feature_name,
                feature_family,
                feature_ordinal,
            }
        }
    }
}

fn active_configuration_is_admitted(ir: &CadIr, saved: &BTreeSet<BodyId>) -> bool {
    if ir.model.configurations.is_empty()
        || (saved.is_empty() && ir.model.features.iter().all(is_body_neutral_feature))
    {
        return true;
    }
    let mut active = ir
        .model
        .configurations
        .iter()
        .filter(|configuration| configuration.active.is_active());
    let Some(configuration) = active.next() else {
        return false;
    };
    let Some(configuration_bodies) = configuration.bodies.resolved() else {
        return false;
    };
    active.next().is_none()
        && configuration_bodies.len() == saved.len()
        && configuration_bodies.iter().collect::<BTreeSet<_>>()
            == saved.iter().collect::<BTreeSet<_>>()
        && (ir.model.features.iter().all(is_body_neutral_feature)
            || !crate::decode::active_configuration_state_is_incomplete(ir, configuration))
}

fn rederived_body_census(
    ir: &CadIr,
) -> Result<BTreeSet<BodyId>, (FeatureId, UnsupportedBodyCensusReason)> {
    let mut bodies = BTreeSet::new();
    let saved_bodies = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<BTreeSet<_>>();
    let mut seen_features = BTreeSet::new();
    let mut previous_ordinal = None;
    let mut features = ir.model.features.iter().collect::<Vec<_>>();
    features.sort_by_key(|feature| feature.ordinal);
    for feature in features {
        if seen_features.contains(&feature.id)
            || previous_ordinal.is_some_and(|ordinal| feature.ordinal <= ordinal)
            || feature
                .dependencies
                .iter()
                .any(|dependency| !seen_features.contains(dependency))
        {
            return Err((
                feature.id.clone(),
                UnsupportedBodyCensusReason::InvalidHistoryOrder,
            ));
        }
        previous_ordinal = Some(feature.ordinal);
        seen_features.insert(feature.id.clone());
        match feature.suppressed {
            None if !is_body_neutral_feature(feature)
                && !suppression_is_body_census_invariant(feature, &bodies) =>
            {
                return Err((
                    feature.id.clone(),
                    UnsupportedBodyCensusReason::UnresolvedSuppression,
                ));
            }
            None => {}
            Some(true) => continue,
            Some(false) => {}
        }
        match &feature.definition {
            FeatureDefinition::TreeNode { .. }
            | FeatureDefinition::DatumPrincipalPlane { .. }
            | FeatureDefinition::DatumPlane { .. }
            | FeatureDefinition::DatumPlaneUnresolved
            | FeatureDefinition::DatumOffsetPlane { .. }
            | FeatureDefinition::DatumAxis { .. }
            | FeatureDefinition::DatumPoint { .. }
            | FeatureDefinition::DatumPointUnresolved
            | FeatureDefinition::DatumCoordinateSystem { .. }
            | FeatureDefinition::DatumCoordinateSystemUnresolved
            | FeatureDefinition::Sketch { .. }
            | FeatureDefinition::ProjectedCurve { .. }
            | FeatureDefinition::SectionShape { .. } => {
                if !feature.outputs.is_empty() {
                    return Err((
                        feature.id.clone(),
                        UnsupportedBodyCensusReason::InvalidOutputLineage,
                    ));
                }
            }
            FeatureDefinition::Native { kind, .. }
                if kind == "DELETE"
                    && !feature
                        .source_properties
                        .contains_key("primary_body_object_index") => {}
            FeatureDefinition::Native { kind, .. }
                if kind == "FSET" && feature.outputs.is_empty() => {}
            FeatureDefinition::Block {
                dimensions: Some(dimensions),
                placement: Some(placement),
                op: BooleanOp::NewBody,
            } if dimensions.iter().copied().all(positive_length) && placement.is_proper_rigid() => {
                let [output] = feature.outputs.as_slice() else {
                    return Err((
                        feature.id.clone(),
                        UnsupportedBodyCensusReason::InvalidOutputLineage,
                    ));
                };
                if !bodies.insert(output.clone()) {
                    return Err((
                        feature.id.clone(),
                        UnsupportedBodyCensusReason::InvalidOutputLineage,
                    ));
                }
            }
            FeatureDefinition::Block {
                dimensions: Some(dimensions),
                placement: Some(placement),
                op: BooleanOp::Join | BooleanOp::Cut | BooleanOp::Intersect,
            } if dimensions.iter().copied().all(positive_length) && placement.is_proper_rigid() => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Block {
                dimensions: Some(dimensions),
                placement: Some(placement),
                op: BooleanOp::Unresolved,
            } if dimensions.iter().copied().all(positive_length)
                && placement.is_proper_rigid()
                && matches!(feature.outputs.as_slice(), [output] if bodies.contains(output)) =>
            {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Block { .. } => {
                return Err((
                    feature.id.clone(),
                    UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                ));
            }
            FeatureDefinition::LoftUnresolved | FeatureDefinition::FreeformSurfaceUnresolved
                if feature.outputs.is_empty() => {}
            FeatureDefinition::Loft { op, .. } => {
                apply_complete_boolean_outputs(
                    feature,
                    &mut bodies,
                    *op,
                    crate::decode::loft_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Extrude {
                op: BooleanOp::Unresolved | BooleanOp::Join | BooleanOp::Cut | BooleanOp::Intersect,
                ..
            } if matches!(feature.outputs.as_slice(), [output] if bodies.contains(output)) => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Extrude { .. } if output_free_local_body_construction(feature) => {}
            FeatureDefinition::Extrude { op, .. } => {
                apply_complete_boolean_outputs(
                    feature,
                    &mut bodies,
                    *op,
                    crate::decode::extrude_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Revolve { .. } if output_free_local_body_construction(feature) => {}
            FeatureDefinition::Revolve { op, .. } => {
                apply_complete_boolean_outputs(
                    feature,
                    &mut bodies,
                    *op,
                    crate::decode::revolve_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Rib { .. } if output_free_local_body_construction(feature) => {}
            FeatureDefinition::Rib { op, .. } => {
                apply_complete_boolean_outputs(
                    feature,
                    &mut bodies,
                    *op,
                    crate::decode::rib_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Sweep { .. } if output_free_local_body_construction(feature) => {}
            FeatureDefinition::Sweep { mode, .. } => {
                let op = match mode {
                    cadmpeg_ir::features::SweepMode::Solid { op } => *op,
                    cadmpeg_ir::features::SweepMode::Surface => BooleanOp::NewBody,
                    cadmpeg_ir::features::SweepMode::Unresolved => BooleanOp::Unresolved,
                };
                apply_complete_boolean_outputs(
                    feature,
                    &mut bodies,
                    op,
                    crate::decode::sweep_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::BaseFeature { .. } if output_free_native_snapshot(feature) => {}
            FeatureDefinition::BaseFeature {
                bodies: BodySelection::Resolved { bodies, .. },
            } if bodies.is_empty() && feature.outputs.is_empty() => {}
            FeatureDefinition::BaseFeature { bodies: selection }
            | FeatureDefinition::InsertBodies { bodies: selection } => {
                let Some(selected) = explicit_body_selection(selection) else {
                    return Err((
                        feature.id.clone(),
                        UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                    ));
                };
                if selected != feature.outputs.as_slice()
                    || selected.iter().any(|body| bodies.contains(body))
                {
                    return Err((
                        feature.id.clone(),
                        UnsupportedBodyCensusReason::InvalidOutputLineage,
                    ));
                }
                bodies.extend(selected.iter().cloned());
            }
            FeatureDefinition::ExtractBody { source } => {
                if feature.outputs.is_empty() && complete_local_body_selection(source) {
                    continue;
                }
                let Some(sources) = explicit_body_selection(source) else {
                    return Err((
                        feature.id.clone(),
                        UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                    ));
                };
                if sources.len() != feature.outputs.len()
                    || sources.iter().any(|body| !bodies.contains(body))
                    || feature.outputs.iter().any(|body| bodies.contains(body))
                    || feature.outputs.iter().collect::<BTreeSet<_>>().len()
                        != feature.outputs.len()
                {
                    return Err((
                        feature.id.clone(),
                        UnsupportedBodyCensusReason::InvalidOutputLineage,
                    ));
                }
                bodies.extend(feature.outputs.iter().cloned());
            }
            FeatureDefinition::TrimSurface { .. } => {
                preserve_in_place_outputs(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::ExtendSurface { .. } => {
                preserve_in_place_outputs(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Hole { .. } => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Chamfer { .. } => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Fillet { .. } => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::FaceBlend { .. } => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::OffsetSurface { .. } => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Thicken { .. } => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Draft { .. } => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::DraftUnresolved if output_free_local_body_construction(feature) => {}
            FeatureDefinition::ReplaceFace { .. } => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Combine { target, tools, .. }
                if feature.outputs.is_empty()
                    && complete_local_or_native_body_selection(target)
                    && complete_local_or_native_body_selection(tools) => {}
            FeatureDefinition::Combine { target, tools, .. }
                if local_tool_combine_is_census_invariant(feature, target, tools, &bodies) => {}
            FeatureDefinition::Combine {
                target,
                tools,
                keep_tools,
                ..
            } => {
                apply_complete_body_combine(
                    feature,
                    &mut bodies,
                    target,
                    tools,
                    *keep_tools,
                    crate::decode::combine_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::SewBodies {
                bodies: selection, ..
            } => {
                if local_body_replacement_is_census_invariant(feature, selection, &bodies) {
                    continue;
                }
                apply_complete_body_replacement(
                    feature,
                    &mut bodies,
                    selection,
                    crate::decode::sew_bodies_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::TrimBodies { targets, tools, .. } => {
                if feature.outputs.is_empty() {
                    continue;
                }
                preserve_complete_body_targets(
                    feature,
                    &bodies,
                    targets,
                    tools,
                    crate::decode::trim_bodies_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::DeleteBody {
                bodies: selection,
                mode,
            } => {
                apply_complete_body_retention(
                    feature,
                    &mut bodies,
                    selection,
                    *mode,
                    crate::decode::delete_body_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Pattern { seeds, pattern } => {
                if feature.outputs.is_empty() {
                    continue;
                }
                apply_complete_body_pattern(
                    feature,
                    &mut bodies,
                    seeds,
                    crate::decode::pattern_occurrence_count(pattern),
                    crate::decode::pattern_feature_is_incomplete(
                        seeds,
                        pattern,
                        &feature.dependencies,
                    ),
                )?;
            }
            _ => {
                return Err((
                    feature.id.clone(),
                    UnsupportedBodyCensusReason::UnsupportedFeatureDefinition,
                ));
            }
        }
    }
    Ok(bodies)
}

fn is_body_neutral_feature(feature: &cadmpeg_ir::features::Feature) -> bool {
    feature.outputs.is_empty()
        && (matches!(
            feature.definition,
            FeatureDefinition::TreeNode { .. }
                | FeatureDefinition::DatumPrincipalPlane { .. }
                | FeatureDefinition::DatumPlane { .. }
                | FeatureDefinition::DatumPlaneUnresolved
                | FeatureDefinition::DatumOffsetPlane { .. }
                | FeatureDefinition::DatumAxis { .. }
                | FeatureDefinition::DatumPoint { .. }
                | FeatureDefinition::DatumPointUnresolved
                | FeatureDefinition::DatumCoordinateSystem { .. }
                | FeatureDefinition::DatumCoordinateSystemUnresolved
                | FeatureDefinition::Sketch { .. }
                | FeatureDefinition::ProjectedCurve { .. }
                | FeatureDefinition::SectionShape { .. }
        ) || matches!(
            &feature.definition,
            FeatureDefinition::Native { kind, .. }
                if (kind == "DELETE"
                    && !feature.source_properties.contains_key("primary_body_object_index"))
                    || kind == "FSET"
        ))
}

fn suppression_is_body_census_invariant(
    feature: &cadmpeg_ir::features::Feature,
    bodies: &BTreeSet<BodyId>,
) -> bool {
    let deletes_only_local_bodies = matches!(
        &feature.definition,
        FeatureDefinition::DeleteBody {
            bodies: BodySelection::Local { bodies, native },
            mode: BodyRetentionMode::DeleteSelected,
        } if !native.trim().is_empty()
            && !bodies.is_empty()
            && bodies.iter().all(|body| !body.trim().is_empty())
            && bodies.iter().collect::<BTreeSet<_>>().len() == bodies.len()
    );
    let extracts_only_local_bodies = feature.outputs.is_empty()
        && matches!(
            &feature.definition,
            FeatureDefinition::ExtractBody { source }
                if complete_local_body_selection(source)
        );
    let sews_only_local_bodies = matches!(
        &feature.definition,
        FeatureDefinition::SewBodies { bodies: selection, .. }
            if local_body_replacement_is_census_invariant(feature, selection, bodies)
    );
    let output_free_trim = feature.outputs.is_empty()
        && matches!(&feature.definition, FeatureDefinition::TrimBodies { .. });
    let output_free_pattern = matches!(&feature.definition, FeatureDefinition::Pattern { .. })
        && (crate::decode::output_free_pattern_construction(feature)
            || crate::decode::output_free_local_body_construction(feature));
    let output_free_combine = feature.outputs.is_empty()
        && matches!(
            &feature.definition,
            FeatureDefinition::Combine { target, tools, .. }
                if complete_local_or_native_body_selection(target)
                    && complete_local_or_native_body_selection(tools)
        );
    let local_tool_combine = matches!(
        &feature.definition,
        FeatureDefinition::Combine { target, tools, .. }
            if local_tool_combine_is_census_invariant(feature, target, tools, bodies)
    );
    let output_free_boolean_construction = output_free_local_body_construction(feature)
        && matches!(
            &feature.definition,
            FeatureDefinition::Extrude { .. }
                | FeatureDefinition::Revolve { .. }
                | FeatureDefinition::Rib { .. }
                | FeatureDefinition::Sweep { .. }
        );
    let in_place_unresolved_extrude = feature.outputs.len() == 1
        && bodies.contains(&feature.outputs[0])
        && matches!(
            &feature.definition,
            FeatureDefinition::Extrude {
                op: BooleanOp::Unresolved | BooleanOp::Join | BooleanOp::Cut | BooleanOp::Intersect,
                ..
            }
        );
    let output_free_local_in_place = output_free_local_body_construction(feature)
        && matches!(&feature.definition, FeatureDefinition::DraftUnresolved);
    let output_free_snapshot = output_free_native_snapshot(feature);
    deletes_only_local_bodies
        || extracts_only_local_bodies
        || sews_only_local_bodies
        || output_free_trim
        || output_free_pattern
        || output_free_combine
        || local_tool_combine
        || output_free_boolean_construction
        || in_place_unresolved_extrude
        || output_free_local_in_place
        || output_free_snapshot
        || ((feature.outputs.is_empty()
            || (feature.outputs.len() == 1 && bodies.contains(&feature.outputs[0])))
            && matches!(
                feature.definition,
                FeatureDefinition::TrimSurface { .. }
                    | FeatureDefinition::LoftUnresolved
                    | FeatureDefinition::FreeformSurfaceUnresolved
                    | FeatureDefinition::ExtendSurface { .. }
                    | FeatureDefinition::Hole { .. }
                    | FeatureDefinition::Chamfer { .. }
                    | FeatureDefinition::Fillet { .. }
                    | FeatureDefinition::FaceBlend { .. }
                    | FeatureDefinition::OffsetSurface { .. }
                    | FeatureDefinition::Thicken { .. }
                    | FeatureDefinition::Draft { .. }
                    | FeatureDefinition::ReplaceFace { .. }
            ))
}

fn complete_local_or_native_body_selection(selection: &BodySelection) -> bool {
    match selection {
        BodySelection::Local { .. } => complete_local_body_selection(selection),
        BodySelection::Native(native) => !native.trim().is_empty(),
        BodySelection::NativeSet(native) => {
            !native.is_empty()
                && native.iter().all(|body| !body.trim().is_empty())
                && native.iter().collect::<BTreeSet<_>>().len() == native.len()
        }
        _ => false,
    }
}

fn local_tool_combine_is_census_invariant(
    feature: &cadmpeg_ir::features::Feature,
    target: &BodySelection,
    tools: &BodySelection,
    bodies: &BTreeSet<BodyId>,
) -> bool {
    let Some([target]) = explicit_body_selection(target) else {
        return false;
    };
    let BodySelection::Local {
        bodies: local_tools,
        native,
    } = tools
    else {
        return false;
    };
    feature.outputs.as_slice() == std::slice::from_ref(target)
        && bodies.contains(target)
        && !native.trim().is_empty()
        && !local_tools.is_empty()
        && local_tools.iter().all(|tool| !tool.trim().is_empty())
        && local_tools.iter().collect::<BTreeSet<_>>().len() == local_tools.len()
}

/// Validate the exact body-identity effect of an in-place edit independently
/// of construction semantics that cannot alter that identity transition.
fn preserve_in_place_single_output(
    feature: &cadmpeg_ir::features::Feature,
    bodies: &BTreeSet<BodyId>,
    saved_bodies: &BTreeSet<BodyId>,
) -> Result<(), (FeatureId, UnsupportedBodyCensusReason)> {
    preserve_in_place_outputs(feature, bodies, saved_bodies)?;
    if feature.outputs.len() > 1 {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    Ok(())
}

fn preserve_in_place_outputs(
    feature: &cadmpeg_ir::features::Feature,
    bodies: &BTreeSet<BodyId>,
    saved_bodies: &BTreeSet<BodyId>,
) -> Result<(), (FeatureId, UnsupportedBodyCensusReason)> {
    // A retained body image can be the final saved output of an in-place edit
    // even when no replay writer has established it yet. In-place operations
    // never create a body, so accept that terminal identity without inserting
    // it into the replay set. An identity absent from both sets remains an
    // invalid output lineage.
    if feature.outputs.iter().collect::<BTreeSet<_>>().len() != feature.outputs.len()
        || feature
            .outputs
            .iter()
            .any(|output| !bodies.contains(output) && !saved_bodies.contains(output))
    {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    Ok(())
}

fn apply_complete_boolean_outputs(
    feature: &cadmpeg_ir::features::Feature,
    bodies: &mut BTreeSet<BodyId>,
    op: BooleanOp,
    incomplete: bool,
) -> Result<(), (FeatureId, UnsupportedBodyCensusReason)> {
    if incomplete || matches!(op, BooleanOp::Unresolved) {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    }
    if feature.outputs.is_empty()
        || feature.outputs.iter().collect::<BTreeSet<_>>().len() != feature.outputs.len()
    {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    match op {
        BooleanOp::NewBody
            if feature
                .outputs
                .iter()
                .all(|output| !bodies.contains(output)) =>
        {
            bodies.extend(feature.outputs.iter().cloned());
        }
        BooleanOp::Join | BooleanOp::Cut | BooleanOp::Intersect
            if feature.outputs.iter().all(|output| bodies.contains(output)) => {}
        _ => {
            return Err((
                feature.id.clone(),
                UnsupportedBodyCensusReason::InvalidOutputLineage,
            ));
        }
    }
    Ok(())
}

fn apply_complete_body_combine(
    feature: &cadmpeg_ir::features::Feature,
    bodies: &mut BTreeSet<BodyId>,
    target: &BodySelection,
    tools: &BodySelection,
    keep_tools: bool,
    incomplete: bool,
) -> Result<(), (FeatureId, UnsupportedBodyCensusReason)> {
    if incomplete {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    }
    let (Some([target]), Some(tools)) = (
        explicit_body_selection(target),
        explicit_body_selection(tools),
    ) else {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    };
    if feature.outputs.as_slice() != std::slice::from_ref(target)
        || !bodies.contains(target)
        || tools.iter().any(|tool| !bodies.contains(tool))
    {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    if !keep_tools {
        for tool in tools {
            bodies.remove(tool);
        }
    }
    Ok(())
}

fn apply_complete_body_replacement(
    feature: &cadmpeg_ir::features::Feature,
    bodies: &mut BTreeSet<BodyId>,
    inputs: &BodySelection,
    incomplete: bool,
) -> Result<(), (FeatureId, UnsupportedBodyCensusReason)> {
    if incomplete {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    }
    let Some(inputs) = explicit_body_selection(inputs) else {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    };
    let input_set = inputs.iter().cloned().collect::<BTreeSet<_>>();
    if inputs.iter().any(|input| !bodies.contains(input))
        || feature.outputs.is_empty()
        || feature.outputs.iter().collect::<BTreeSet<_>>().len() != feature.outputs.len()
        || feature
            .outputs
            .iter()
            .any(|output| bodies.contains(output) && !input_set.contains(output))
    {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    for input in inputs {
        bodies.remove(input);
    }
    bodies.extend(feature.outputs.iter().cloned());
    Ok(())
}

fn apply_complete_body_retention(
    feature: &cadmpeg_ir::features::Feature,
    bodies: &mut BTreeSet<BodyId>,
    selection: &BodySelection,
    mode: BodyRetentionMode,
    incomplete: bool,
) -> Result<(), (FeatureId, UnsupportedBodyCensusReason)> {
    if incomplete {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    }
    if mode == BodyRetentionMode::DeleteSelected
        && matches!(selection, BodySelection::Local { .. })
        && feature.outputs.is_empty()
    {
        return Ok(());
    }
    let Some(selected) = explicit_body_selection(selection) else {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    };
    if !feature.outputs.is_empty() || selected.iter().any(|body| !bodies.contains(body)) {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    match mode {
        BodyRetentionMode::DeleteSelected => {
            for body in selected {
                bodies.remove(body);
            }
        }
        BodyRetentionMode::KeepSelected => bodies.retain(|body| selected.contains(body)),
        BodyRetentionMode::Unresolved => unreachable!("incomplete retention mode returned above"),
    }
    Ok(())
}

fn preserve_complete_body_targets(
    feature: &cadmpeg_ir::features::Feature,
    bodies: &BTreeSet<BodyId>,
    targets: &BodySelection,
    tools: &BodySelection,
    incomplete: bool,
) -> Result<(), (FeatureId, UnsupportedBodyCensusReason)> {
    if incomplete {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    }
    let (Some(targets), Some(tools)) = (
        explicit_body_selection(targets),
        explicit_body_selection(tools),
    ) else {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    };
    if feature.outputs.as_slice() != targets
        || targets.iter().any(|target| !bodies.contains(target))
        || tools.iter().any(|tool| !bodies.contains(tool))
    {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    Ok(())
}

fn apply_complete_body_pattern(
    feature: &cadmpeg_ir::features::Feature,
    bodies: &mut BTreeSet<BodyId>,
    seeds: &[PatternSeed],
    occurrence_count: Option<usize>,
    incomplete: bool,
) -> Result<(), (FeatureId, UnsupportedBodyCensusReason)> {
    if incomplete {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    }
    let Some(occurrence_count) = occurrence_count else {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    };
    if seeds
        .iter()
        .any(|seed| !matches!(seed, PatternSeed::Bodies(_)))
    {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::UnsupportedFeatureDefinition,
        ));
    }
    let Some(seed_bodies) = seeds
        .iter()
        .map(|seed| match seed {
            PatternSeed::Bodies(selection) => explicit_body_selection(selection),
            PatternSeed::Feature(_) | PatternSeed::Faces(_) | PatternSeed::Occurrences(_) => None,
        })
        .collect::<Option<Vec<_>>>()
    else {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    };
    let seed_bodies = seed_bodies.into_iter().flatten().collect::<Vec<_>>();
    let expected_outputs = seed_bodies
        .len()
        .checked_mul(occurrence_count.saturating_sub(1));
    if expected_outputs != Some(feature.outputs.len())
        || seed_bodies.iter().collect::<BTreeSet<_>>().len() != seed_bodies.len()
        || seed_bodies.iter().any(|body| !bodies.contains(*body))
        || feature.outputs.iter().collect::<BTreeSet<_>>().len() != feature.outputs.len()
        || feature.outputs.iter().any(|output| bodies.contains(output))
    {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    bodies.extend(feature.outputs.iter().cloned());
    Ok(())
}

fn explicit_body_selection(selection: &BodySelection) -> Option<&[BodyId]> {
    let bodies = match selection {
        BodySelection::Bodies(bodies)
        | BodySelection::Resolved { bodies, .. }
        | BodySelection::ResolvedSet { bodies, .. } => bodies,
        BodySelection::Unresolved
        | BodySelection::Historical { .. }
        | BodySelection::HistoricalSet { .. }
        | BodySelection::HistoricalUnorderedSet { .. }
        | BodySelection::Generated { .. }
        | BodySelection::Local { .. }
        | BodySelection::Native(_)
        | BodySelection::NativeSet(_) => return None,
    };
    (!bodies.is_empty() && bodies.iter().collect::<BTreeSet<_>>().len() == bodies.len())
        .then_some(bodies)
}

fn complete_local_body_selection(selection: &BodySelection) -> bool {
    matches!(
        selection,
        BodySelection::Local { bodies, native }
            if !native.trim().is_empty()
                && !bodies.is_empty()
                && bodies.iter().all(|body| !body.trim().is_empty())
                && bodies.iter().collect::<BTreeSet<_>>().len() == bodies.len()
    )
}

fn local_body_replacement_is_census_invariant(
    feature: &cadmpeg_ir::features::Feature,
    selection: &BodySelection,
    bodies: &BTreeSet<BodyId>,
) -> bool {
    complete_local_body_selection(selection)
        && (feature.outputs.is_empty()
            || (feature.outputs.iter().collect::<BTreeSet<_>>().len() == feature.outputs.len()
                && feature.outputs.iter().all(|output| bodies.contains(output))))
}

fn positive_length(length: Length) -> bool {
    length.0.is_finite() && length.0 > 0.0
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use cadmpeg_ir::features::{
        Angle, BodyRetentionMode, BodyTrimSide, ChamferGroup, ChamferSpec, ConfigurationBodies,
        ConfigurationFeatureState, ConfigurationId, CurveProjectionDirection,
        CurveProjectionDirectionState, DesignConfiguration, EdgeSelection, ExtrudeDirection,
        ExtrudeExtent, ExtrudeSide, ExtrudeStart, FaceSelection, Feature, FilletGroup, HoleKind,
        HolePlacement, PathRef, PatternKind, ProfileRef, RadiusSpec, RevolutionConstruction,
        RibConstruction, RibDraft, SketchSpace, SurfaceExtension, SweepMode, SweepSection,
        Termination, ThickenSide, TrimRegion,
    };
    use cadmpeg_ir::ids::{CurveId, FaceId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::{Body, BodyKind};

    use super::*;

    fn model_body(id: &str) -> Body {
        Body {
            id: BodyId(id.to_string()),
            kind: BodyKind::Solid,
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        }
    }

    fn complete_block_ir() -> CadIr {
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let body = BodyId("body".to_string());
        ir.model.bodies.push(model_body(&body.0));
        ir.model.features.push(Feature {
            id: FeatureId("block".to_string()),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: vec![body],
            definition: FeatureDefinition::Block {
                dimensions: Some([Length(1.0), Length(2.0), Length(3.0)]),
                placement: Some(cadmpeg_ir::transform::Transform::identity()),
                op: BooleanOp::NewBody,
            },
            native_ref: None,
        });
        ir
    }

    fn attach_complete_active_configuration(ir: &mut CadIr) {
        let feature_states = ir
            .model
            .features
            .iter()
            .map(|feature| {
                (
                    feature.id.clone(),
                    ConfigurationFeatureState {
                        suppressed: false,
                        dependencies: feature.dependencies.clone(),
                        outputs: feature.outputs.clone(),
                        definition: feature.definition.clone(),
                    },
                )
            })
            .collect();
        ir.model.configurations.push(DesignConfiguration {
            id: ConfigurationId("active".to_string()),
            ordinal: 0,
            active: true.into(),
            source_index: Some(0),
            name: "Model".into(),
            material: None,
            properties: BTreeMap::new(),
            parameter_overrides: BTreeMap::new(),
            suppressed_features: Vec::new(),
            bodies: ConfigurationBodies::Resolved(
                ir.model.bodies.iter().map(|body| body.id.clone()).collect(),
            ),
            parameter_values: BTreeMap::new(),
            feature_states,
            native_ref: None,
        });
    }

    fn complete_hole(body: BodyId) -> Feature {
        Feature {
            id: FeatureId("hole".to_string()),
            ordinal: 1,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: vec![body],
            definition: FeatureDefinition::Hole {
                profile: None,
                profile_filter: None,
                face: None,
                position: None,
                direction: None,
                placements: vec![HolePlacement::Directed {
                    position: Point3::new(0.0, 0.0, 0.0),
                    direction: Vector3::new(0.0, 0.0, 1.0),
                }],
                kind: HoleKind::Simple,
                exit_kind: None,
                diameter: Some(Length(0.5)),
                extent: Some(Termination::ThroughAll),
                bottom: None,
                taper_angle: None,
                specification: None,
                allow_multi_profile_faces: None,
            },
            native_ref: None,
        }
    }

    fn body_preserving_feature(
        id: &str,
        ordinal: u64,
        body: BodyId,
        definition: FeatureDefinition,
    ) -> Feature {
        Feature {
            id: FeatureId(id.to_string()),
            ordinal,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: vec![body],
            definition,
            native_ref: None,
        }
    }

    fn body_neutral_feature(id: &str, ordinal: u64, definition: FeatureDefinition) -> Feature {
        Feature {
            id: FeatureId(id.to_string()),
            ordinal,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        }
    }

    fn complete_extrude_feature(
        id: &str,
        ordinal: u64,
        profile: FeatureId,
        outputs: Vec<BodyId>,
        op: BooleanOp,
    ) -> Feature {
        let mut feature = body_neutral_feature(
            id,
            ordinal,
            FeatureDefinition::Extrude {
                profile: ProfileRef::Feature(profile.clone()),
                direction: ExtrudeDirection::ProfileNormal,
                start: ExtrudeStart::ProfilePlane,
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: Termination::Blind {
                            length: Length(1.0),
                        },
                        draft: None,
                        offset: None,
                    },
                },
                op,
                direction_source: None,
                solid: Some(true),
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            },
        );
        feature.dependencies.push(profile);
        feature.outputs = outputs;
        feature
    }

    #[test]
    fn complete_hole_preserves_the_existing_body_identity() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        ir.model.features.push(complete_hole(body.clone()));

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }

    #[test]
    fn exact_empty_replay_input_precedes_a_new_body_construction() {
        let mut ir = complete_block_ir();
        ir.model.features[0].ordinal = 1;
        ir.model.features.insert(
            0,
            Feature {
                id: FeatureId("initial-bodies".to_string()),
                ordinal: 0,
                name: Some("Retained history input".to_string()),
                suppressed: Some(false),
                parent: None,
                dependencies: Vec::new(),
                source_properties: BTreeMap::new(),
                source_tag: None,
                source_text: None,
                source_content: Vec::new(),
                outputs: Vec::new(),
                definition: FeatureDefinition::BaseFeature {
                    bodies: BodySelection::Resolved {
                        bodies: Vec::new(),
                        native: "nx:segment-body-bindings".to_string(),
                    },
                },
                native_ref: None,
            },
        );

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified {
                bodies: vec![BodyId("body".to_string())],
            }
        );
    }

    #[test]
    fn in_place_edit_can_precede_a_saved_body_image_creator() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        ir.model.features[0].ordinal = 1;
        ir.model.features.push(complete_hole(body.clone()));
        ir.model.features[1].ordinal = 0;

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }

    #[test]
    fn curve_construction_families_do_not_change_the_body_census() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        ir.model.features.extend([
            body_neutral_feature(
                "projected-curve",
                1,
                FeatureDefinition::ProjectedCurve {
                    source: PathRef::Unresolved("source".to_string()),
                    target_faces: FaceSelection::Unresolved,
                    direction: CurveProjectionDirection::State(
                        CurveProjectionDirectionState::Unresolved,
                    ),
                    bidirectional: None,
                },
            ),
            body_neutral_feature(
                "section",
                2,
                FeatureDefinition::SectionShape {
                    first: BodySelection::Unresolved,
                    second: BodySelection::Unresolved,
                    approximate: None,
                },
            ),
        ]);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }

    #[test]
    fn curve_construction_family_cannot_claim_a_body_output() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        let mut section = body_neutral_feature(
            "section",
            1,
            FeatureDefinition::SectionShape {
                first: BodySelection::Unresolved,
                second: BodySelection::Unresolved,
                approximate: None,
            },
        );
        section.outputs.push(body);
        ir.model.features.push(section);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("section".to_string())),
                reason: UnsupportedBodyCensusReason::InvalidOutputLineage,
            }
        );
    }

    #[test]
    fn unresolved_block_result_mode_stops_before_body_effect_evaluation() {
        let mut ir = complete_block_ir();
        let FeatureDefinition::Block { op, .. } = &mut ir.model.features[0].definition else {
            unreachable!("block fixture")
        };
        *op = BooleanOp::Unresolved;

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("block".to_string())),
                reason: UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
            }
        );
    }

    #[test]
    fn boolean_block_preserves_its_existing_output_body() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        ir.model.features.push(body_preserving_feature(
            "joined-block",
            1,
            body.clone(),
            FeatureDefinition::Block {
                dimensions: Some([Length(0.5), Length(0.5), Length(0.5)]),
                placement: Some(cadmpeg_ir::transform::Transform::identity()),
                op: BooleanOp::Join,
            },
        ));

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }

    #[test]
    fn unresolved_block_mode_preserves_a_proven_existing_output() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        ir.model.features.push(body_preserving_feature(
            "existing-block",
            1,
            body.clone(),
            FeatureDefinition::Block {
                dimensions: Some([Length(1.0), Length(2.0), Length(3.0)]),
                placement: Some(cadmpeg_ir::transform::Transform::identity()),
                op: BooleanOp::Unresolved,
            },
        ));

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }

    #[test]
    fn complete_extrudes_apply_new_body_and_boolean_output_lineage() {
        let mut ir = complete_block_ir();
        let existing = ir.model.bodies[0].id.clone();
        let created = BodyId("extruded".to_string());
        ir.model.bodies.push(model_body(&created.0));
        let profile = FeatureId("profile".to_string());
        ir.model.features.push(body_neutral_feature(
            &profile.0,
            1,
            FeatureDefinition::Sketch {
                space: SketchSpace::Unresolved,
                sketch: None,
            },
        ));
        ir.model.features.extend([
            complete_extrude_feature(
                "new-extrude",
                2,
                profile.clone(),
                vec![created.clone()],
                BooleanOp::NewBody,
            ),
            complete_extrude_feature(
                "joined-extrude",
                3,
                profile,
                vec![existing.clone()],
                BooleanOp::Join,
            ),
        ]);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified {
                bodies: vec![existing, created],
            }
        );
    }

    #[test]
    fn new_body_operation_cannot_reuse_an_existing_body_identity() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        let profile = FeatureId("profile".to_string());
        ir.model.features.push(body_neutral_feature(
            &profile.0,
            1,
            FeatureDefinition::Sketch {
                space: SketchSpace::Unresolved,
                sketch: None,
            },
        ));
        ir.model.features.push(complete_extrude_feature(
            "extrude",
            2,
            profile,
            vec![body],
            BooleanOp::NewBody,
        ));

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("extrude".to_string())),
                reason: UnsupportedBodyCensusReason::InvalidOutputLineage,
            }
        );
    }

    #[test]
    fn unresolved_extrude_preserves_an_existing_output_without_construction_replay() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        let mut extrude = complete_extrude_feature(
            "extrude",
            1,
            FeatureId("missing-profile".to_string()),
            vec![body.clone()],
            BooleanOp::Unresolved,
        );
        extrude.suppressed = None;
        extrude.dependencies.clear();
        ir.model.features.push(extrude);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }

    #[test]
    fn profile_driven_families_report_incomplete_construction_before_lineage() {
        let definitions = [
            FeatureDefinition::Loft {
                sections: Vec::new(),
                guides: Vec::new(),
                centerline: None,
                op: BooleanOp::NewBody,
                closed: false,
                solid: true,
                ruled: false,
                linearize: false,
                max_degree: None,
                check_compatibility: None,
                allow_multi_profile_faces: None,
            },
            complete_extrude_feature(
                "fixture",
                1,
                FeatureId("profile".to_string()),
                Vec::new(),
                BooleanOp::NewBody,
            )
            .definition,
            FeatureDefinition::Revolve {
                construction: RevolutionConstruction {
                    profile: None,
                    axis: None,
                    extent: None,
                    axis_reference: None,
                    solid: None,
                    face_maker_class: None,
                    fuse_order: None,
                    allow_multi_profile_faces: None,
                },
                op: BooleanOp::NewBody,
            },
            FeatureDefinition::Rib {
                construction: RibConstruction {
                    profile: None,
                    direction: None,
                    thickness: None,
                    side: None,
                    draft: RibDraft::Unresolved,
                },
                op: BooleanOp::Join,
            },
            FeatureDefinition::Sweep {
                section: SweepSection::Unresolved(None),
                sections: Vec::new(),
                path: None,
                mode: SweepMode::Unresolved,
                orientation: None,
                transition: None,
                transformation: None,
                path_tangent: false,
                linearize: false,
                twist: None,
                path_extent: None,
                guide_rail: None,
                taper: None,
                scale: None,
                allow_multi_profile_faces: None,
            },
        ];
        for (index, definition) in definitions.into_iter().enumerate() {
            let mut ir = complete_block_ir();
            let id = format!("incomplete-{index}");
            ir.model
                .features
                .push(body_neutral_feature(&id, 1, definition));

            assert_eq!(
                evaluate_saved_body_census(&ir),
                BodyCensusEvaluation::Unsupported {
                    feature: Some(FeatureId(id)),
                    reason: UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                }
            );
        }
    }

    #[test]
    fn complete_active_configuration_admits_the_rederived_model() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        attach_complete_active_configuration(&mut ir);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }

    #[test]
    fn incomplete_active_configuration_remains_an_evaluation_boundary() {
        let mut ir = complete_block_ir();
        attach_complete_active_configuration(&mut ir);
        ir.model.configurations[0].feature_states.clear();

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: None,
                reason: UnsupportedBodyCensusReason::ConfigurationEvaluation,
            }
        );
    }

    #[test]
    fn body_neutral_history_needs_only_exact_configuration_body_membership() {
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let mut datum = body_neutral_feature(
            "datum",
            0,
            FeatureDefinition::DatumCoordinateSystemUnresolved,
        );
        datum.suppressed = None;
        ir.model.features.push(datum);
        attach_complete_active_configuration(&mut ir);
        ir.model.configurations[0].feature_states.clear();

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: Vec::new() }
        );
    }

    #[test]
    fn empty_body_neutral_model_does_not_need_an_active_configuration_identity() {
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        ir.model.features.push(body_neutral_feature(
            "datum",
            0,
            FeatureDefinition::DatumCoordinateSystemUnresolved,
        ));
        attach_complete_active_configuration(&mut ir);
        ir.model.configurations[0].active = false.into();
        ir.model.configurations[0].bodies = ConfigurationBodies::Unresolved;

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: Vec::new() }
        );
    }

    #[test]
    fn incomplete_hole_construction_does_not_change_its_body_identity_effect() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        let mut hole = complete_hole(body.clone());
        let FeatureDefinition::Hole { placements, .. } = &mut hole.definition else {
            unreachable!("hole fixture")
        };
        placements.clear();
        ir.model.features.push(hole);

        assert!(crate::decode::hole_definition_is_incomplete(
            &ir.model.features[1]
        ));
        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }

    #[test]
    fn hole_cannot_modify_a_body_absent_from_prior_history() {
        let mut ir = complete_block_ir();
        ir.model
            .features
            .push(complete_hole(BodyId("other".to_string())));

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("hole".to_string())),
                reason: UnsupportedBodyCensusReason::InvalidOutputLineage,
            }
        );
    }

    #[test]
    fn replay_requires_dependencies_to_precede_their_consumers() {
        let mut ir = complete_block_ir();
        ir.model.features[0]
            .dependencies
            .push(FeatureId("later".to_string()));

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("block".to_string())),
                reason: UnsupportedBodyCensusReason::InvalidHistoryOrder,
            }
        );
    }

    #[test]
    fn replay_uses_stable_feature_ordinals_independently_of_storage_order() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        ir.model.features.push(complete_hole(body.clone()));
        ir.model.features.reverse();

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }

    #[test]
    fn replay_rejects_duplicate_feature_ordinals() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        let mut hole = complete_hole(body.clone());
        hole.ordinal = 0;
        ir.model.features.push(hole);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("hole".to_string())),
                reason: UnsupportedBodyCensusReason::InvalidHistoryOrder,
            }
        );
    }

    #[test]
    fn base_feature_introduces_its_complete_selected_outputs() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        ir.model.features[0].definition = FeatureDefinition::BaseFeature {
            bodies: BodySelection::Bodies(vec![body.clone()]),
        };

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }

    #[test]
    fn extract_body_copies_each_existing_source_to_one_new_output() {
        let mut ir = complete_block_ir();
        let source = ir.model.bodies[0].id.clone();
        let extracted = BodyId("extracted".to_string());
        ir.model.bodies.push(model_body(&extracted.0));
        ir.model.features.push(Feature {
            id: FeatureId("extract".to_string()),
            ordinal: 1,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: vec![extracted.clone()],
            definition: FeatureDefinition::ExtractBody {
                source: BodySelection::Bodies(vec![source]),
            },
            native_ref: None,
        });

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified {
                bodies: vec![BodyId("body".to_string()), extracted],
            }
        );
    }

    #[test]
    fn output_free_local_extract_does_not_change_the_saved_body_census() {
        let mut ir = complete_block_ir();
        ir.model.features.push(Feature {
            id: FeatureId("extract-local".to_string()),
            ordinal: 1,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::ExtractBody {
                source: BodySelection::Local {
                    bodies: vec!["nx:om-data-blocks-2:block#736".to_string()],
                    native: "nx:om-object-index#736".to_string(),
                },
            },
            native_ref: None,
        });

        assert!(matches!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies }
                if bodies == [BodyId("body".to_string())]
        ));
    }

    #[test]
    fn delete_body_removes_an_existing_selected_body() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        ir.model.features.push(Feature {
            id: FeatureId("delete".to_string()),
            ordinal: 1,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::DeleteBody {
                bodies: BodySelection::Bodies(vec![body]),
                mode: BodyRetentionMode::DeleteSelected,
            },
            native_ref: None,
        });
        ir.model.bodies.clear();

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: Vec::new() }
        );
    }

    #[test]
    fn delete_body_ignores_a_complete_feature_local_body() {
        let mut ir = complete_block_ir();
        ir.model.features.push(Feature {
            id: FeatureId("delete-local".to_string()),
            ordinal: 1,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::DeleteBody {
                bodies: BodySelection::Local {
                    bodies: vec!["input-body".to_string()],
                    native: "native-selection".to_string(),
                },
                mode: BodyRetentionMode::DeleteSelected,
            },
            native_ref: None,
        });

        assert!(matches!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies }
                if bodies == [BodyId("body".to_string())]
        ));
    }

    #[test]
    fn unresolved_suppression_of_a_resolved_delete_remains_a_boundary() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        ir.model.features.push(Feature {
            id: FeatureId("delete".to_string()),
            ordinal: 1,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::DeleteBody {
                bodies: BodySelection::Bodies(vec![body]),
                mode: BodyRetentionMode::DeleteSelected,
            },
            native_ref: None,
        });

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("delete".to_string())),
                reason: UnsupportedBodyCensusReason::UnresolvedSuppression,
            }
        );
    }

    #[test]
    fn keep_selected_removes_every_unselected_body() {
        let mut ir = complete_block_ir();
        let retained = ir.model.bodies[0].id.clone();
        let removed = BodyId("removed".to_string());
        ir.model.features[0].outputs.push(removed.clone());
        ir.model.features[0].definition = FeatureDefinition::BaseFeature {
            bodies: BodySelection::Bodies(vec![retained.clone(), removed.clone()]),
        };
        ir.model.features.push(body_neutral_feature(
            "retain",
            1,
            FeatureDefinition::DeleteBody {
                bodies: BodySelection::Bodies(vec![retained.clone()]),
                mode: BodyRetentionMode::KeepSelected,
            },
        ));

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified {
                bodies: vec![retained]
            }
        );
    }

    #[test]
    fn combine_consumes_tools_and_preserves_the_target_identity() {
        let mut ir = complete_block_ir();
        let target = ir.model.bodies[0].id.clone();
        let tool = BodyId("tool".to_string());
        ir.model.features[0].outputs.push(tool.clone());
        ir.model.features[0].definition = FeatureDefinition::BaseFeature {
            bodies: BodySelection::Bodies(vec![target.clone(), tool.clone()]),
        };
        ir.model.features.push(body_preserving_feature(
            "combine",
            1,
            target.clone(),
            FeatureDefinition::Combine {
                target: BodySelection::Bodies(vec![target.clone()]),
                tools: BodySelection::Bodies(vec![tool]),
                op: BooleanOp::Join,
                keep_tools: false,
            },
        ));

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified {
                bodies: vec![target]
            }
        );
    }

    #[test]
    fn combine_preserves_tools_when_requested() {
        let mut ir = complete_block_ir();
        let target = ir.model.bodies[0].id.clone();
        let tool = BodyId("tool".to_string());
        ir.model.bodies.push(model_body(&tool.0));
        ir.model.features[0].outputs.push(tool.clone());
        ir.model.features[0].definition = FeatureDefinition::BaseFeature {
            bodies: BodySelection::Bodies(vec![target.clone(), tool.clone()]),
        };
        ir.model.features.push(body_preserving_feature(
            "combine",
            1,
            target.clone(),
            FeatureDefinition::Combine {
                target: BodySelection::Bodies(vec![target.clone()]),
                tools: BodySelection::Bodies(vec![tool.clone()]),
                op: BooleanOp::Join,
                keep_tools: true,
            },
        ));

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified {
                bodies: vec![target, tool]
            }
        );
    }

    #[test]
    fn combine_with_exact_local_tools_preserves_its_retained_target() {
        let mut ir = complete_block_ir();
        let target = ir.model.bodies[0].id.clone();
        let mut combine = body_preserving_feature(
            "combine",
            1,
            target.clone(),
            FeatureDefinition::Combine {
                target: BodySelection::Bodies(vec![target.clone()]),
                tools: BodySelection::Local {
                    bodies: vec!["local-tool".to_string()],
                    native: "native-tools".to_string(),
                },
                op: BooleanOp::Cut,
                keep_tools: false,
            },
        );
        combine.suppressed = None;
        ir.model.features.push(combine);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified {
                bodies: vec![target]
            }
        );
    }

    #[test]
    fn combine_with_invalid_local_tool_identity_is_not_admitted() {
        let mut ir = complete_block_ir();
        let target = ir.model.bodies[0].id.clone();
        ir.model.features.push(body_preserving_feature(
            "combine",
            1,
            target.clone(),
            FeatureDefinition::Combine {
                target: BodySelection::Bodies(vec![target]),
                tools: BodySelection::Local {
                    bodies: vec![String::new()],
                    native: "native-tools".to_string(),
                },
                op: BooleanOp::Cut,
                keep_tools: false,
            },
        ));

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("combine".to_string())),
                reason: UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
            }
        );
    }

    #[test]
    fn output_free_combine_with_exact_native_operands_is_local_to_history() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        let mut combine = body_neutral_feature(
            "local-combine",
            1,
            FeatureDefinition::Combine {
                target: BodySelection::Native("native-target".to_string()),
                tools: BodySelection::NativeSet(vec!["native-tool".to_string()]),
                op: BooleanOp::Intersect,
                keep_tools: false,
            },
        );
        combine.suppressed = None;
        ir.model.features.push(combine);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }

    #[test]
    fn trim_bodies_preserves_all_targets_and_tools() {
        let mut ir = complete_block_ir();
        let first = ir.model.bodies[0].id.clone();
        let second = BodyId("second".to_string());
        let tool = BodyId("tool".to_string());
        ir.model.bodies.push(model_body(&second.0));
        ir.model.bodies.push(model_body(&tool.0));
        ir.model.features[0].outputs = vec![first.clone(), second.clone(), tool.clone()];
        ir.model.features[0].definition = FeatureDefinition::BaseFeature {
            bodies: BodySelection::Bodies(vec![first.clone(), second.clone(), tool.clone()]),
        };
        let mut trim = body_neutral_feature(
            "trim",
            1,
            FeatureDefinition::TrimBodies {
                targets: BodySelection::Bodies(vec![first.clone(), second.clone()]),
                tools: BodySelection::Bodies(vec![tool.clone()]),
                keep: BodyTrimSide::Forward,
            },
        );
        trim.outputs = vec![first.clone(), second.clone()];
        ir.model.features.push(trim);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified {
                bodies: vec![first, second, tool]
            }
        );
    }

    #[test]
    fn trim_bodies_rejects_outputs_that_do_not_match_its_targets() {
        let mut ir = complete_block_ir();
        let target = ir.model.bodies[0].id.clone();
        let tool = BodyId("tool".to_string());
        ir.model.bodies.push(model_body(&tool.0));
        ir.model.features[0].outputs.push(tool.clone());
        ir.model.features[0].definition = FeatureDefinition::BaseFeature {
            bodies: BodySelection::Bodies(vec![target.clone(), tool.clone()]),
        };
        ir.model.features.push(body_preserving_feature(
            "trim",
            1,
            tool.clone(),
            FeatureDefinition::TrimBodies {
                targets: BodySelection::Bodies(vec![target]),
                tools: BodySelection::Bodies(vec![tool]),
                keep: BodyTrimSide::Reverse,
            },
        ));

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("trim".to_string())),
                reason: UnsupportedBodyCensusReason::InvalidOutputLineage,
            }
        );
    }

    #[test]
    fn trim_bodies_requires_a_resolved_retained_side_before_lineage() {
        let mut ir = complete_block_ir();
        let target = ir.model.bodies[0].id.clone();
        ir.model.features.push(body_preserving_feature(
            "trim",
            1,
            target.clone(),
            FeatureDefinition::TrimBodies {
                targets: BodySelection::Bodies(vec![target]),
                tools: BodySelection::Bodies(vec![BodyId("tool".to_string())]),
                keep: BodyTrimSide::Unresolved,
            },
        ));

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("trim".to_string())),
                reason: UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
            }
        );
    }

    #[test]
    fn output_free_trim_is_body_census_neutral_without_resolved_roles() {
        let mut ir = complete_block_ir();
        ir.model.features.push(Feature {
            id: FeatureId("trim-local".to_string()),
            ordinal: 1,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::TrimBodies {
                targets: BodySelection::Unresolved,
                tools: BodySelection::Unresolved,
                keep: BodyTrimSide::Unresolved,
            },
            native_ref: None,
        });

        assert!(matches!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies }
                if bodies == [BodyId("body".to_string())]
        ));
    }

    #[test]
    fn sew_replaces_all_inputs_with_its_declared_outputs() {
        let mut ir = complete_block_ir();
        let first = ir.model.bodies[0].id.clone();
        let second = BodyId("second".to_string());
        let sewn = BodyId("sewn".to_string());
        ir.model.bodies[0] = model_body(&sewn.0);
        ir.model.features[0].outputs = vec![first.clone(), second.clone()];
        ir.model.features[0].definition = FeatureDefinition::BaseFeature {
            bodies: BodySelection::Bodies(vec![first.clone(), second.clone()]),
        };
        let mut feature = body_preserving_feature(
            "sew",
            1,
            sewn.clone(),
            FeatureDefinition::SewBodies {
                bodies: BodySelection::Bodies(vec![first, second]),
                gap_tolerance: None,
            },
        );
        feature.outputs = vec![sewn.clone()];
        ir.model.features.push(feature);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![sewn] }
        );
    }

    #[test]
    fn local_sew_with_an_already_retained_output_is_census_invariant() {
        let mut ir = complete_block_ir();
        let output = ir.model.bodies[0].id.clone();
        ir.model.features.push(Feature {
            id: FeatureId("sew-local".to_string()),
            ordinal: 1,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: vec![output.clone()],
            definition: FeatureDefinition::SewBodies {
                bodies: BodySelection::Local {
                    bodies: vec!["historical-sheet".to_string()],
                    native: "native-selection".to_string(),
                },
                gap_tolerance: None,
            },
            native_ref: None,
        });

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified {
                bodies: vec![output]
            }
        );
    }

    #[test]
    fn combine_rejects_a_tool_absent_from_prior_history() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        ir.model.features.push(body_preserving_feature(
            "combine",
            1,
            body.clone(),
            FeatureDefinition::Combine {
                target: BodySelection::Bodies(vec![body]),
                tools: BodySelection::Bodies(vec![BodyId("missing".to_string())]),
                op: BooleanOp::Cut,
                keep_tools: false,
            },
        ));

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("combine".to_string())),
                reason: UnsupportedBodyCensusReason::InvalidOutputLineage,
            }
        );
    }

    #[test]
    fn sew_rejects_an_output_identity_owned_by_an_unconsumed_body() {
        let mut ir = complete_block_ir();
        let first = ir.model.bodies[0].id.clone();
        let second = BodyId("second".to_string());
        let unrelated = BodyId("unrelated".to_string());
        ir.model.bodies = vec![model_body(&unrelated.0)];
        ir.model.features[0].outputs = vec![first.clone(), second.clone(), unrelated.clone()];
        ir.model.features[0].definition = FeatureDefinition::BaseFeature {
            bodies: BodySelection::Bodies(vec![first.clone(), second.clone(), unrelated.clone()]),
        };
        ir.model.features.push(body_preserving_feature(
            "sew",
            1,
            unrelated,
            FeatureDefinition::SewBodies {
                bodies: BodySelection::Bodies(vec![first, second]),
                gap_tolerance: None,
            },
        ));

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("sew".to_string())),
                reason: UnsupportedBodyCensusReason::InvalidOutputLineage,
            }
        );
    }

    #[test]
    fn native_body_selection_is_an_incomplete_semantic_boundary() {
        let mut ir = complete_block_ir();
        ir.model.features[0].definition = FeatureDefinition::BaseFeature {
            bodies: BodySelection::Native("selection".to_string()),
        };

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("block".to_string())),
                reason: UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
            }
        );
    }

    #[test]
    fn completed_history_reports_a_saved_body_census_mismatch() {
        let mut ir = complete_block_ir();
        ir.model.bodies.clear();

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Mismatch {
                rederived: vec![BodyId("body".to_string())],
                saved: Vec::new(),
            }
        );
    }

    #[test]
    fn complete_chamfer_and_fillet_preserve_the_existing_body() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        ir.model.features.push(body_preserving_feature(
            "chamfer",
            1,
            body.clone(),
            FeatureDefinition::Chamfer {
                groups: vec![ChamferGroup {
                    edges: EdgeSelection::All,
                    spec: ChamferSpec::Distance {
                        distance: Length(0.25),
                    },
                }],
                flip_direction: false,
            },
        ));
        ir.model.features.push(body_preserving_feature(
            "fillet",
            2,
            body.clone(),
            FeatureDefinition::Fillet {
                groups: vec![FilletGroup {
                    edges: EdgeSelection::All,
                    radius: RadiusSpec::Constant {
                        radius: Length(0.2),
                    },
                    tangency_weight: Some(1.0),
                }],
            },
        ));

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }

    #[test]
    fn incomplete_chamfer_construction_does_not_change_its_body_identity_effect() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        ir.model.features.push(body_preserving_feature(
            "chamfer",
            1,
            body.clone(),
            FeatureDefinition::Chamfer {
                groups: vec![ChamferGroup {
                    edges: EdgeSelection::Unresolved,
                    spec: ChamferSpec::Distance {
                        distance: Length(0.25),
                    },
                }],
                flip_direction: false,
            },
        ));

        assert!(crate::decode::chamfer_definition_is_incomplete(
            &ir.model.features[1]
        ));
        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }

    #[test]
    fn complete_single_body_dress_up_families_preserve_identity() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        let first = FaceSelection::Faces(vec![FaceId("first".to_string())]);
        let second = FaceSelection::Faces(vec![FaceId("second".to_string())]);
        let definitions = [
            FeatureDefinition::FaceBlend {
                first_faces: first.clone(),
                second_faces: second.clone(),
                radius: RadiusSpec::Constant {
                    radius: Length(0.2),
                },
            },
            FeatureDefinition::OffsetSurface {
                faces: first.clone(),
                distance: Some(Length(0.1)),
            },
            FeatureDefinition::Thicken {
                faces: first.clone(),
                thickness: Some(Length(0.3)),
                side: Some(ThickenSide::Forward),
            },
            FeatureDefinition::Draft {
                faces: first.clone(),
                neutral_plane: second.clone(),
                parting_tool: None,
                pull_direction: Some(Vector3::new(0.0, 0.0, 1.0)),
                pull_plane: None,
                angle: Some(Angle(0.1)),
                outward: Some(false),
            },
            FeatureDefinition::ReplaceFace {
                targets: first,
                replacements: second,
            },
        ];
        for (index, definition) in definitions.into_iter().enumerate() {
            ir.model.features.push(body_preserving_feature(
                &format!("dress-up-{index}"),
                u64::try_from(index).expect("five dress-up fixtures") + 1,
                body.clone(),
                definition,
            ));
        }

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }

    #[test]
    fn complete_surface_edits_preserve_every_declared_body_identity() {
        let mut ir = complete_block_ir();
        let first = ir.model.bodies[0].id.clone();
        let second = BodyId("second".to_string());
        ir.model.bodies.push(model_body(&second.0));
        ir.model.features[0].outputs.push(second.clone());
        ir.model.features[0].definition = FeatureDefinition::BaseFeature {
            bodies: BodySelection::Bodies(vec![first.clone(), second.clone()]),
        };
        let faces = FaceSelection::Faces(vec![FaceId("face".to_string())]);
        let mut trim = body_neutral_feature(
            "trim-surface",
            1,
            FeatureDefinition::TrimSurface {
                faces: faces.clone(),
                tool: PathRef::Curves(vec![CurveId("trim-curve".to_string())]),
                keep: TrimRegion::Inside,
            },
        );
        trim.outputs = vec![first.clone(), second.clone()];
        let mut extend = body_neutral_feature(
            "extend-surface",
            2,
            FeatureDefinition::ExtendSurface {
                faces,
                distance: Some(Length(0.5)),
                method: SurfaceExtension::Natural,
            },
        );
        extend.outputs = vec![first.clone(), second.clone()];
        ir.model.features.extend([trim, extend]);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified {
                bodies: vec![first, second]
            }
        );
    }

    #[test]
    fn output_free_surface_edits_are_body_identity_neutral() {
        let definitions = [
            FeatureDefinition::TrimSurface {
                faces: FaceSelection::Unresolved,
                tool: PathRef::Unresolved("trim".to_string()),
                keep: TrimRegion::Unresolved,
            },
            FeatureDefinition::ExtendSurface {
                faces: FaceSelection::Unresolved,
                distance: None,
                method: SurfaceExtension::Unresolved,
            },
        ];
        for (ordinal, definition) in definitions.into_iter().enumerate() {
            let mut ir = complete_block_ir();
            ir.model.features.push(body_neutral_feature(
                "surface-edit",
                u64::try_from(ordinal).expect("two surface edit families") + 1,
                definition,
            ));
            assert!(match &ir.model.features[1].definition {
                FeatureDefinition::TrimSurface { .. } => {
                    crate::decode::trim_surface_definition_is_incomplete(&ir.model.features[1])
                }
                FeatureDefinition::ExtendSurface { .. } => {
                    crate::decode::extend_surface_definition_is_incomplete(&ir.model.features[1])
                }
                _ => unreachable!("surface edit fixture"),
            });
            assert_eq!(
                evaluate_saved_body_census(&ir),
                BodyCensusEvaluation::Verified {
                    bodies: vec![ir.model.bodies[0].id.clone()],
                }
            );
        }
    }

    #[test]
    fn output_free_unresolved_loft_is_body_census_neutral() {
        let mut ir = complete_block_ir();
        ir.model.features.push(Feature {
            id: FeatureId("loft".to_string()),
            ordinal: 1,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::LoftUnresolved,
            native_ref: None,
        });

        assert!(matches!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies }
                if bodies == [BodyId("body".to_string())]
        ));
    }

    #[test]
    fn output_free_unresolved_freeform_surface_is_body_census_neutral() {
        let mut ir = complete_block_ir();
        ir.model.features.push(Feature {
            id: FeatureId("freeform".to_string()),
            ordinal: 1,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::FreeformSurfaceUnresolved,
            native_ref: None,
        });

        assert!(matches!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies }
                if bodies == [BodyId("body".to_string())]
        ));
    }

    #[test]
    fn complete_surface_edit_rejects_an_output_absent_from_prior_history() {
        let mut ir = complete_block_ir();
        let missing = BodyId("missing".to_string());
        let mut trim = body_neutral_feature(
            "trim-surface",
            1,
            FeatureDefinition::TrimSurface {
                faces: FaceSelection::Faces(vec![FaceId("face".to_string())]),
                tool: PathRef::Curves(vec![CurveId("trim-curve".to_string())]),
                keep: TrimRegion::Outside,
            },
        );
        trim.outputs = vec![missing];
        ir.model.features.push(trim);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("trim-surface".to_string())),
                reason: UnsupportedBodyCensusReason::InvalidOutputLineage,
            }
        );
    }

    #[test]
    fn body_pattern_adds_one_copy_per_non_original_occurrence() {
        let mut ir = complete_block_ir();
        let seed = ir.model.bodies[0].id.clone();
        let first_copy = BodyId("copy-1".to_string());
        let second_copy = BodyId("copy-2".to_string());
        ir.model.bodies.push(model_body(&first_copy.0));
        ir.model.bodies.push(model_body(&second_copy.0));
        let mut pattern = body_neutral_feature(
            "pattern",
            1,
            FeatureDefinition::Pattern {
                seeds: vec![PatternSeed::Bodies(BodySelection::Bodies(vec![
                    seed.clone()
                ]))],
                pattern: PatternKind::Linear {
                    direction: Some(Vector3::new(1.0, 0.0, 0.0)),
                    spacing: Length(2.0),
                    count: 3,
                    second: None,
                },
            },
        );
        pattern.outputs = vec![first_copy.clone(), second_copy.clone()];
        ir.model.features.push(pattern);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified {
                bodies: vec![seed, first_copy, second_copy]
            }
        );
    }

    #[test]
    fn output_free_unresolved_pattern_is_body_census_neutral() {
        let mut ir = complete_block_ir();
        ir.model.features.push(Feature {
            id: FeatureId("pattern".to_string()),
            ordinal: 1,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::Unresolved { form: None },
            },
            native_ref: None,
        });

        assert!(matches!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies }
                if bodies == [BodyId("body".to_string())]
        ));
    }

    #[test]
    fn unresolved_suppression_is_irrelevant_to_output_free_construction() {
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        ir.model.features.push(Feature {
            id: FeatureId("datum".to_string()),
            ordinal: 0,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::DatumCoordinateSystemUnresolved,
            native_ref: None,
        });

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: Vec::new() }
        );
    }

    #[test]
    fn output_free_boolean_construction_has_no_retained_body_effect() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        let mut extrude = complete_extrude_feature(
            "transient-extrude",
            1,
            FeatureId("unresolved-profile".to_string()),
            Vec::new(),
            BooleanOp::NewBody,
        );
        extrude.suppressed = None;
        extrude.dependencies.clear();
        extrude.source_properties.insert(
            "primary_body_reference".to_string(),
            "reference".to_string(),
        );
        ir.model.features.push(extrude);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }

    #[test]
    fn output_free_fset_is_body_census_neutral() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        let mut fset = body_neutral_feature(
            "fset",
            1,
            FeatureDefinition::Native {
                kind: "FSET".to_string(),
                parameters: BTreeMap::new(),
                properties: BTreeMap::new(),
            },
        );
        fset.suppressed = None;
        ir.model.features.push(fset);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }

    #[test]
    fn output_free_native_snapshot_is_local_to_history() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        let mut snapshot = body_neutral_feature(
            "snapshot",
            1,
            FeatureDefinition::BaseFeature {
                bodies: BodySelection::Unresolved,
            },
        );
        snapshot.name = Some("MASTER SNAPSHOT BODY".to_string());
        snapshot.suppressed = None;
        snapshot
            .source_properties
            .insert("operation_record".to_string(), "native-record".to_string());
        ir.model.features.push(snapshot);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }

    #[test]
    fn unresolved_suppression_still_blocks_a_body_effect() {
        let mut ir = complete_block_ir();
        ir.model.features[0].suppressed = None;

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("block".to_string())),
                reason: UnsupportedBodyCensusReason::UnresolvedSuppression,
            }
        );
    }

    #[test]
    fn unresolved_suppression_does_not_block_a_complete_in_place_edit() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        let mut hole = complete_hole(body.clone());
        hole.suppressed = None;
        ir.model.features.push(hole);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }

    #[test]
    fn output_free_hole_is_body_identity_neutral_regardless_of_suppression() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        let mut hole = complete_hole(body.clone());
        hole.suppressed = None;
        if let FeatureDefinition::Hole { placements, .. } = &mut hole.definition {
            placements.clear();
        }
        hole.outputs.clear();
        ir.model.features.push(hole);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }

    #[test]
    fn native_delete_without_a_primary_body_is_body_neutral() {
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let mut deletion = Feature {
            id: FeatureId("delete".to_string()),
            ordinal: 0,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Native {
                kind: "DELETE".to_string(),
                parameters: BTreeMap::new(),
                properties: BTreeMap::new(),
            },
            native_ref: None,
        };
        ir.model.features.push(deletion.clone());

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: Vec::new() }
        );

        deletion
            .source_properties
            .insert("primary_body_object_index".to_string(), "7".to_string());
        ir.model.features[0] = deletion;
        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("delete".to_string())),
                reason: UnsupportedBodyCensusReason::UnresolvedSuppression,
            }
        );
    }

    #[test]
    fn body_pattern_requires_exact_copy_cardinality_and_new_identities() {
        let mut ir = complete_block_ir();
        let seed = ir.model.bodies[0].id.clone();
        ir.model.features.push(body_preserving_feature(
            "pattern",
            1,
            seed.clone(),
            FeatureDefinition::Pattern {
                seeds: vec![PatternSeed::Bodies(BodySelection::Bodies(vec![seed]))],
                pattern: PatternKind::Mirror {
                    plane_origin: Point3::new(0.0, 0.0, 0.0),
                    plane_normal: Vector3::new(1.0, 0.0, 0.0),
                },
            },
        ));

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("pattern".to_string())),
                reason: UnsupportedBodyCensusReason::InvalidOutputLineage,
            }
        );
    }

    #[test]
    fn feature_seed_pattern_remains_an_explicit_body_effect_boundary() {
        let mut ir = complete_block_ir();
        let seed = ir.model.features[0].id.clone();
        let body = ir.model.bodies[0].id.clone();
        let mut pattern = body_preserving_feature(
            "pattern",
            1,
            body,
            FeatureDefinition::Pattern {
                seeds: vec![PatternSeed::Feature(seed.clone())],
                pattern: PatternKind::Mirror {
                    plane_origin: Point3::new(0.0, 0.0, 0.0),
                    plane_normal: Vector3::new(1.0, 0.0, 0.0),
                },
            },
        );
        pattern.dependencies.push(seed);
        ir.model.features.push(pattern);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("pattern".to_string())),
                reason: UnsupportedBodyCensusReason::UnsupportedFeatureDefinition,
            }
        );
    }

    #[test]
    fn overlapping_replace_face_operands_do_not_change_the_body_identity_effect() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        let faces = FaceSelection::Faces(vec![FaceId("face".to_string())]);
        ir.model.features.push(body_preserving_feature(
            "replace-face",
            1,
            body.clone(),
            FeatureDefinition::ReplaceFace {
                targets: faces.clone(),
                replacements: faces,
            },
        ));

        assert!(crate::decode::replace_face_definition_is_incomplete(
            &ir.model.features[1]
        ));
        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Verified { bodies: vec![body] }
        );
    }
}
