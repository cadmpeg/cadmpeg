// SPDX-License-Identifier: Apache-2.0
//! Neutral evaluation of Siemens NX feature-history effects.

use crate::decode::feature_completeness;

use std::collections::BTreeSet;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    BodyRetentionMode, BodySelection, BooleanOp, FeatureDefinition, FeatureId, FeatureOperation,
    PatternSeed, UnresolvedFamily,
};
use cadmpeg_ir::ids::BodyId;

use crate::decode::feature_completeness::{
    output_free_local_body_construction, output_free_native_snapshot,
};
use serde::{Deserialize, Serialize};

/// Why a saved-body census cannot yet be evaluated exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
}

impl UnsupportedBodyCensusReason {
    /// Stable snake-case code for this boundary.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnresolvedSuppression => "unresolved_suppression",
            Self::UnsupportedFeatureDefinition => "unsupported_feature_definition",
            Self::IncompleteFeatureDefinition => "incomplete_feature_definition",
            Self::InvalidOutputLineage => "invalid_output_lineage",
            Self::InvalidHistoryOrder => "invalid_history_order",
        }
    }
}

/// Feature at an unsupported saved-body census boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureBoundary {
    /// Feature identity at the boundary.
    pub id: FeatureId,
    /// Feature display name, if any.
    pub name: Option<String>,
    /// Feature definition family, if any.
    pub family: Option<String>,
    /// Feature history ordinal.
    pub ordinal: u64,
}

/// Result of evaluating neutral history against the saved current-body census.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "BodyCensusEvaluationWire",
    into = "BodyCensusEvaluationWire"
)]
pub enum BodyCensusEvaluation {
    /// Neutral evaluation produced exactly the saved body identities.
    Verified {
        /// Re-derived body identities in canonical order.
        bodies: Vec<BodyId>,
    },
    /// Exact evaluation stopped at an unsupported semantic boundary.
    Unsupported {
        /// Feature at the boundary.
        feature: FeatureBoundary,
        /// Semantic boundary that prevented exact evaluation.
        reason: UnsupportedBodyCensusReason,
    },
    /// Active configuration requires configuration-local evaluation.
    ConfigurationEvaluation,
    /// Evaluation completed, but its body identities differ from the saved model.
    Mismatch {
        /// Re-derived body identities in canonical order.
        rederived: Vec<BodyId>,
        /// Saved body identities in canonical order.
        saved: Vec<BodyId>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum BodyCensusEvaluationWire {
    Verified {
        bodies: Vec<BodyId>,
    },
    Unsupported {
        feature: Option<FeatureBoundary>,
        reason: String,
    },
    Mismatch {
        rederived: Vec<BodyId>,
        saved: Vec<BodyId>,
    },
}

impl TryFrom<BodyCensusEvaluationWire> for BodyCensusEvaluation {
    type Error = String;

    fn try_from(wire: BodyCensusEvaluationWire) -> Result<Self, Self::Error> {
        Ok(match wire {
            BodyCensusEvaluationWire::Verified { bodies } => Self::Verified { bodies },
            BodyCensusEvaluationWire::Mismatch { rederived, saved } => {
                Self::Mismatch { rederived, saved }
            }
            BodyCensusEvaluationWire::Unsupported { feature, reason } => {
                if reason == "configuration_evaluation" {
                    if feature.is_some() {
                        return Err(
                            "feature: configuration evaluation cannot name a feature".into()
                        );
                    }
                    Self::ConfigurationEvaluation
                } else {
                    let reason = UnsupportedBodyCensusReason::deserialize(
                        serde::de::value::StringDeserializer::<serde::de::value::Error>::new(
                            reason,
                        ),
                    )
                    .map_err(|error| format!("reason: {error}"))?;
                    Self::Unsupported {
                        feature: feature.ok_or("feature: required for feature evaluation")?,
                        reason,
                    }
                }
            }
        })
    }
}

impl From<BodyCensusEvaluation> for BodyCensusEvaluationWire {
    fn from(value: BodyCensusEvaluation) -> Self {
        match value {
            BodyCensusEvaluation::Verified { bodies } => Self::Verified { bodies },
            BodyCensusEvaluation::Mismatch { rederived, saved } => {
                Self::Mismatch { rederived, saved }
            }
            BodyCensusEvaluation::Unsupported { feature, reason } => Self::Unsupported {
                feature: Some(feature),
                reason: reason.as_str().into(),
            },
            BodyCensusEvaluation::ConfigurationEvaluation => Self::Unsupported {
                feature: None,
                reason: "configuration_evaluation".into(),
            },
        }
    }
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
            return BodyCensusEvaluation::Unsupported { feature, reason };
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
        return BodyCensusEvaluation::ConfigurationEvaluation;
    }
    BodyCensusEvaluation::Verified {
        bodies: rederived.into_iter().collect(),
    }
}

fn feature_boundary(feature: &cadmpeg_ir::features::Feature) -> FeatureBoundary {
    FeatureBoundary {
        id: feature.id.clone(),
        name: feature.name.clone(),
        family: serde_json::to_value(feature.evaluation.definition())
            .ok()
            .and_then(|definition| definition.get("definition")?.as_str().map(str::to_string)),
        ordinal: feature.ordinal,
    }
}

/// Saved-body census evidence for the profile harness.
#[doc(hidden)]
pub fn saved_body_census_evidence(ir: &CadIr) -> BodyCensusEvaluation {
    evaluate_saved_body_census(ir)
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
        .filter(|configuration| configuration.active);
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
            || !feature_completeness::active_configuration_state_is_incomplete(ir, configuration))
}

fn rederived_body_census(
    ir: &CadIr,
) -> Result<BTreeSet<BodyId>, (FeatureBoundary, UnsupportedBodyCensusReason)> {
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
                feature_boundary(feature),
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
                    feature_boundary(feature),
                    UnsupportedBodyCensusReason::UnresolvedSuppression,
                ));
            }
            None => {}
            Some(true) => continue,
            Some(false) => {}
        }
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(
                FeatureOperation::TreeNode { .. }
                | FeatureOperation::DatumPrincipalPlane { .. }
                | FeatureOperation::DatumPlane { .. }
                | FeatureOperation::Unresolved {
                    family:
                        UnresolvedFamily::DatumPlane
                        | UnresolvedFamily::DatumAxis
                        | UnresolvedFamily::DatumPoint
                        | UnresolvedFamily::DatumCoordinateSystem
                        | UnresolvedFamily::BridgeCurve,
                }
                | FeatureOperation::DatumOffsetPlane { .. }
                | FeatureOperation::DatumAxis { .. }
                | FeatureOperation::DatumPoint { .. }
                | FeatureOperation::DatumCoordinateSystem { .. }
                | FeatureOperation::Sketch { .. }
                | FeatureOperation::ProjectedCurve { .. }
                | FeatureOperation::SectionShape { .. },
            ) => {
                if !feature.evaluation.outputs().is_empty() {
                    return Err((
                        feature_boundary(feature),
                        UnsupportedBodyCensusReason::InvalidOutputLineage,
                    ));
                }
            }
            FeatureDefinition::Operation(FeatureOperation::Native { kind, .. })
                if kind.as_str() == "DELETE"
                    && !feature
                        .source_properties
                        .contains_key("primary_body_object_index") => {}
            FeatureDefinition::Operation(FeatureOperation::Native { kind, .. })
                if kind.as_str() == "FSET" && feature.evaluation.outputs().is_empty() => {}
            FeatureDefinition::Operation(FeatureOperation::Block {
                dimensions: Some(_),
                placement: Some(_),
                op: BooleanOp::NewBody,
            }) => {
                let [output] = feature.evaluation.outputs().as_slice() else {
                    return Err((
                        feature_boundary(feature),
                        UnsupportedBodyCensusReason::InvalidOutputLineage,
                    ));
                };
                if !bodies.insert(output.clone()) {
                    return Err((
                        feature_boundary(feature),
                        UnsupportedBodyCensusReason::InvalidOutputLineage,
                    ));
                }
            }
            FeatureDefinition::Operation(FeatureOperation::Block {
                dimensions: Some(_),
                placement: Some(_),
                op: BooleanOp::Join | BooleanOp::Cut | BooleanOp::Intersect,
            }) => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Block {
                dimensions: Some(_),
                placement: Some(_),
                op: BooleanOp::Unresolved,
            }) if matches!(feature.evaluation.outputs().as_slice(), [output] if bodies.contains(output)) =>
            {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Block { .. }) => {
                return Err((
                    feature_boundary(feature),
                    UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                ));
            }
            FeatureDefinition::Operation(FeatureOperation::Sphere { op, .. }) => {
                apply_complete_boolean_outputs(
                    feature,
                    &mut bodies,
                    *op,
                    feature_completeness::sphere_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::Loft | UnresolvedFamily::FreeformSurface,
            }) if feature.evaluation.outputs().is_empty() => {}
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::Brep,
            }) if feature.evaluation.outputs().is_empty() => {}
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::Brep,
            }) => {
                return Err((
                    feature_boundary(feature),
                    UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                ));
            }
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DeleteFace,
            }) if feature.evaluation.outputs().is_empty() => {}
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DeleteFace,
            }) => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::MirrorFace,
            }) if feature.evaluation.outputs().is_empty() => {}
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::MirrorFace,
            }) => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::SubdivisionBody,
            }) if feature.evaluation.outputs().is_empty() => {}
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::SubdivisionBody,
            }) => {
                return Err((
                    feature_boundary(feature),
                    UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                ));
            }
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::TopologyOptimization,
            }) if feature.evaluation.outputs().is_empty() => {}
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::TopologyOptimization,
            }) => {
                return Err((
                    feature_boundary(feature),
                    UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                ));
            }
            FeatureDefinition::Operation(FeatureOperation::Loft { op, .. }) => {
                apply_complete_boolean_outputs(
                    feature,
                    &mut bodies,
                    *op,
                    feature_completeness::loft_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                op: BooleanOp::Unresolved | BooleanOp::Join | BooleanOp::Cut | BooleanOp::Intersect,
                ..
            }) if matches!(feature.evaluation.outputs().as_slice(), [output] if bodies.contains(output)) =>
            {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Extrude { .. })
                if output_free_local_body_construction(feature) => {}
            FeatureDefinition::Operation(FeatureOperation::Extrude { op, .. }) => {
                apply_complete_boolean_outputs(
                    feature,
                    &mut bodies,
                    *op,
                    feature_completeness::extrude_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::Revolve { .. })
                if output_free_local_body_construction(feature) => {}
            FeatureDefinition::Operation(FeatureOperation::Revolve { op, .. }) => {
                apply_complete_boolean_outputs(
                    feature,
                    &mut bodies,
                    *op,
                    feature_completeness::revolve_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::Rib { .. })
                if output_free_local_body_construction(feature) => {}
            FeatureDefinition::Operation(FeatureOperation::Rib { op, .. }) => {
                apply_complete_boolean_outputs(
                    feature,
                    &mut bodies,
                    *op,
                    feature_completeness::rib_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::Sweep { .. })
                if output_free_local_body_construction(feature) => {}
            FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) => {
                let mode = shape.mode();
                let op = match mode {
                    cadmpeg_ir::features::SweepMode::Solid { op } => op.into(),
                    cadmpeg_ir::features::SweepMode::Surface {} => BooleanOp::NewBody,
                    cadmpeg_ir::features::SweepMode::Unresolved {} => BooleanOp::Unresolved,
                };
                apply_complete_boolean_outputs(
                    feature,
                    &mut bodies,
                    op,
                    feature_completeness::sweep_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::BaseFeature { .. })
                if output_free_native_snapshot(feature) => {}
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Resolved { bodies, .. },
            }) if bodies.is_empty() && feature.evaluation.outputs().is_empty() => {}
            FeatureDefinition::Operation(FeatureOperation::BaseFeature { bodies: selection }) => {
                let Some(selected) = explicit_body_selection(selection) else {
                    return Err((
                        feature_boundary(feature),
                        UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                    ));
                };
                if selected != feature.evaluation.outputs().as_slice()
                    || selected.iter().any(|body| bodies.contains(body))
                {
                    return Err((
                        feature_boundary(feature),
                        UnsupportedBodyCensusReason::InvalidOutputLineage,
                    ));
                }
                bodies.extend(selected.iter().cloned());
            }
            FeatureDefinition::Operation(FeatureOperation::InsertBodies { bodies: selection }) => {
                let selected = feature.evaluation.outputs();
                if !selection.is_resolved()
                    || selected.is_empty()
                    || selected.iter().collect::<BTreeSet<_>>().len() != selected.len()
                {
                    return Err((
                        feature_boundary(feature),
                        UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                    ));
                }
                if selected.iter().any(|body| bodies.contains(body)) {
                    return Err((
                        feature_boundary(feature),
                        UnsupportedBodyCensusReason::InvalidOutputLineage,
                    ));
                }
                bodies.extend(selected.iter().cloned());
            }
            FeatureDefinition::Operation(FeatureOperation::ExtractBody { source }) => {
                if feature.evaluation.outputs().is_empty() && complete_local_body_selection(source)
                {
                    continue;
                }
                let Some(sources) = explicit_body_selection(source) else {
                    return Err((
                        feature_boundary(feature),
                        UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                    ));
                };
                if sources.len() != feature.evaluation.outputs().len()
                    || sources.iter().any(|body| !bodies.contains(body))
                    || feature
                        .evaluation
                        .outputs()
                        .iter()
                        .any(|body| bodies.contains(body))
                    || feature
                        .evaluation
                        .outputs()
                        .iter()
                        .collect::<BTreeSet<_>>()
                        .len()
                        != feature.evaluation.outputs().len()
                {
                    return Err((
                        feature_boundary(feature),
                        UnsupportedBodyCensusReason::InvalidOutputLineage,
                    ));
                }
                bodies.extend(feature.evaluation.outputs().iter().cloned());
            }
            FeatureDefinition::Operation(FeatureOperation::TrimSurface { .. }) => {
                preserve_in_place_outputs(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::ExtendSurface { .. }) => {
                preserve_in_place_outputs(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Hole { .. }) => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Chamfer { .. }) => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Fillet { .. }) => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::FaceBlend { .. }) => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::OffsetSurface { .. }) => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Thicken { .. }) => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Draft { .. }) => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::Draft,
            }) if output_free_local_body_construction(feature) => {}
            FeatureDefinition::Operation(FeatureOperation::ReplaceFace { .. }) => {
                preserve_in_place_single_output(feature, &bodies, &saved_bodies)?;
            }
            FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. })
                if feature.evaluation.outputs().is_empty()
                    && complete_local_or_native_body_selection(operands.target())
                    && complete_local_or_native_body_selection(operands.tools()) => {}
            FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. })
                if local_tool_combine_is_census_invariant(
                    feature,
                    operands.target(),
                    operands.tools(),
                    &bodies,
                ) => {}
            FeatureDefinition::Operation(FeatureOperation::Combine {
                operands,

                keep_tools,
                ..
            }) => {
                let target = operands.target();
                let tools = operands.tools();
                apply_complete_body_combine(
                    feature,
                    &mut bodies,
                    target,
                    tools,
                    if *keep_tools {
                        ToolRetention::Keep
                    } else {
                        ToolRetention::Delete
                    },
                    feature_completeness::combine_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::SewBodies {
                bodies: selection, ..
            }) => {
                if local_body_replacement_is_census_invariant(feature, selection, &bodies) {
                    continue;
                }
                apply_complete_body_replacement(
                    feature,
                    &mut bodies,
                    selection,
                    feature_completeness::sew_bodies_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::TrimBodies { operands, .. }) => {
                let targets = operands.targets();
                let tools = operands.tools();
                if feature.evaluation.outputs().is_empty() {
                    continue;
                }
                preserve_complete_body_targets(
                    feature,
                    &bodies,
                    targets,
                    tools,
                    feature_completeness::trim_bodies_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::DeleteBody {
                bodies: selection,
                mode,
            }) => {
                apply_complete_body_retention(
                    feature,
                    &mut bodies,
                    selection,
                    ResolvedBodyRetentionMode::try_from(*mode)
                        .map_err(|reason| (feature_boundary(feature), reason))?,
                    feature_completeness::operands::body_selection_is_incomplete(selection),
                )?;
            }
            FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) => {
                if feature.evaluation.outputs().is_empty() {
                    continue;
                }
                apply_complete_body_pattern(
                    feature,
                    &mut bodies,
                    seeds,
                    feature_completeness::operands::pattern_occurrence_count(pattern),
                    feature_completeness::operands::pattern_feature_is_incomplete(
                        seeds,
                        pattern,
                        &feature.dependencies,
                    ),
                )?;
            }
            _ => {
                return Err((
                    feature_boundary(feature),
                    UnsupportedBodyCensusReason::UnsupportedFeatureDefinition,
                ));
            }
        }
    }
    Ok(bodies)
}

fn is_body_neutral_feature(feature: &cadmpeg_ir::features::Feature) -> bool {
    feature.evaluation.outputs().is_empty()
        && (matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(
                FeatureOperation::TreeNode { .. }
                    | FeatureOperation::DatumPrincipalPlane { .. }
                    | FeatureOperation::DatumPlane { .. }
                    | FeatureOperation::Unresolved {
                        family: UnresolvedFamily::DatumPlane
                            | UnresolvedFamily::DatumAxis
                            | UnresolvedFamily::DatumPoint
                            | UnresolvedFamily::DatumCoordinateSystem
                            | UnresolvedFamily::BridgeCurve,
                    }
                    | FeatureOperation::DatumOffsetPlane { .. }
                    | FeatureOperation::DatumAxis { .. }
                    | FeatureOperation::DatumPoint { .. }
                    | FeatureOperation::DatumCoordinateSystem { .. }
                    | FeatureOperation::Sketch { .. }
                    | FeatureOperation::ProjectedCurve { .. }
                    | FeatureOperation::SectionShape { .. }
            )
        ) || matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { kind, .. })
                if (kind.as_str() == "DELETE"
                    && !feature.source_properties.contains_key("primary_body_object_index"))
                    || kind.as_str() == "FSET"
        ))
}

fn suppression_is_body_census_invariant(
    feature: &cadmpeg_ir::features::Feature,
    bodies: &BTreeSet<BodyId>,
) -> bool {
    let deletes_only_local_bodies = matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            bodies: BodySelection::Local { bodies, native },
            mode: BodyRetentionMode::DeleteSelected,
        }) if !native.trim().is_empty()
            && !bodies.is_empty()
            && bodies.iter().all(|body| !body.trim().is_empty())
            && bodies.iter().collect::<BTreeSet<_>>().len() == bodies.len()
    );
    let extracts_only_local_bodies = feature.evaluation.outputs().is_empty()
        && matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::ExtractBody { source })
                if complete_local_body_selection(source)
        );
    let sews_only_local_bodies = matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::SewBodies { bodies: selection, .. })
            if local_body_replacement_is_census_invariant(feature, selection, bodies)
    );
    let output_free_trim = feature.evaluation.outputs().is_empty()
        && matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::TrimBodies { .. })
        );
    let output_free_pattern =
        matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Pattern { .. })
        ) && (feature_completeness::output_free_pattern_construction(feature)
            || feature_completeness::output_free_local_body_construction(feature));
    let output_free_combine = feature.evaluation.outputs().is_empty()
        && matches!(
            feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Combine { operands,  .. }) if matches!((operands.target(), operands.tools(),), (target, tools,) if complete_local_or_native_body_selection(target)
                    && complete_local_or_native_body_selection(tools)));
    let local_tool_combine = matches!(
        feature.evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Combine { operands,  .. }) if matches!((operands.target(), operands.tools(),), (target, tools,) if local_tool_combine_is_census_invariant(feature, target, tools, bodies)));
    let output_free_boolean_construction = output_free_local_body_construction(feature)
        && matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(
                FeatureOperation::Extrude { .. }
                    | FeatureOperation::Revolve { .. }
                    | FeatureOperation::Rib { .. }
                    | FeatureOperation::Sweep { .. }
            )
        );
    let in_place_unresolved_extrude = feature.evaluation.outputs().len() == 1
        && bodies.contains(&feature.evaluation.outputs()[0])
        && matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                op: BooleanOp::Unresolved | BooleanOp::Join | BooleanOp::Cut | BooleanOp::Intersect,
                ..
            })
        );
    let output_free_local_in_place = output_free_local_body_construction(feature)
        && matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::Draft
            })
        );
    let output_free_snapshot = output_free_native_snapshot(feature);
    let output_free_brep = feature.evaluation.outputs().is_empty()
        && matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::Brep
            })
        );
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
        || output_free_brep
        || ((feature.evaluation.outputs().is_empty()
            || (feature.evaluation.outputs().len() == 1
                && bodies.contains(&feature.evaluation.outputs()[0])))
            && matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(
                    FeatureOperation::TrimSurface { .. }
                        | FeatureOperation::Unresolved {
                            family: UnresolvedFamily::Loft
                                | UnresolvedFamily::FreeformSurface
                                | UnresolvedFamily::DeleteFace
                                | UnresolvedFamily::MirrorFace
                                | UnresolvedFamily::SubdivisionBody
                                | UnresolvedFamily::TopologyOptimization,
                        }
                        | FeatureOperation::ExtendSurface { .. }
                        | FeatureOperation::Hole { .. }
                        | FeatureOperation::Chamfer { .. }
                        | FeatureOperation::Fillet { .. }
                        | FeatureOperation::FaceBlend { .. }
                        | FeatureOperation::OffsetSurface { .. }
                        | FeatureOperation::Thicken { .. }
                        | FeatureOperation::Draft { .. }
                        | FeatureOperation::ReplaceFace { .. }
                )
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
    let Some(selected_target) = explicit_body_selection(target) else {
        return false;
    };
    let [target] = selected_target.as_slice() else {
        return false;
    };
    let BodySelection::Local {
        bodies: local_tools,
        native,
    } = tools
    else {
        return false;
    };
    feature.evaluation.outputs().as_slice() == std::slice::from_ref(target)
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
) -> Result<(), (FeatureBoundary, UnsupportedBodyCensusReason)> {
    preserve_in_place_outputs(feature, bodies, saved_bodies)?;
    if feature.evaluation.outputs().len() > 1 {
        return Err((
            feature_boundary(feature),
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    Ok(())
}

fn preserve_in_place_outputs(
    feature: &cadmpeg_ir::features::Feature,
    bodies: &BTreeSet<BodyId>,
    saved_bodies: &BTreeSet<BodyId>,
) -> Result<(), (FeatureBoundary, UnsupportedBodyCensusReason)> {
    // A retained body image can be the final saved output of an in-place edit
    // even when no replay writer has established it yet. In-place operations
    // never create a body, so accept that terminal identity without inserting
    // it into the replay set. An identity absent from both sets remains an
    // invalid output lineage.
    if feature
        .evaluation
        .outputs()
        .iter()
        .collect::<BTreeSet<_>>()
        .len()
        != feature.evaluation.outputs().len()
        || feature
            .evaluation
            .outputs()
            .iter()
            .any(|output| !bodies.contains(output) && !saved_bodies.contains(output))
    {
        return Err((
            feature_boundary(feature),
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
) -> Result<(), (FeatureBoundary, UnsupportedBodyCensusReason)> {
    if incomplete || matches!(op, BooleanOp::Unresolved) {
        return Err((
            feature_boundary(feature),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    }
    if feature.evaluation.outputs().is_empty()
        || feature
            .evaluation
            .outputs()
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            != feature.evaluation.outputs().len()
    {
        return Err((
            feature_boundary(feature),
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    match op {
        BooleanOp::NewBody
            if feature
                .evaluation
                .outputs()
                .iter()
                .all(|output| !bodies.contains(output)) =>
        {
            bodies.extend(feature.evaluation.outputs().iter().cloned());
        }
        BooleanOp::Join | BooleanOp::Cut | BooleanOp::Intersect
            if feature
                .evaluation
                .outputs()
                .iter()
                .all(|output| bodies.contains(output)) => {}
        _ => {
            return Err((
                feature_boundary(feature),
                UnsupportedBodyCensusReason::InvalidOutputLineage,
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToolRetention {
    Keep,
    Delete,
}

fn apply_complete_body_combine(
    feature: &cadmpeg_ir::features::Feature,
    bodies: &mut BTreeSet<BodyId>,
    target: &BodySelection,
    tools: &BodySelection,
    tool_retention: ToolRetention,
    incomplete: bool,
) -> Result<(), (FeatureBoundary, UnsupportedBodyCensusReason)> {
    if incomplete {
        return Err((
            feature_boundary(feature),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    }
    let (Some(targets), Some(tools)) = (
        explicit_body_selection(target),
        explicit_body_selection(tools),
    ) else {
        return Err((
            feature_boundary(feature),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    };
    let [target] = targets.as_slice() else {
        return Err((
            feature_boundary(feature),
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    };
    if feature.evaluation.outputs().as_slice() != std::slice::from_ref(target)
        || !bodies.contains(target)
        || tools.iter().any(|tool| !bodies.contains(tool))
    {
        return Err((
            feature_boundary(feature),
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    if tool_retention == ToolRetention::Delete {
        for tool in tools {
            bodies.remove(&tool);
        }
    }
    Ok(())
}

fn apply_complete_body_replacement(
    feature: &cadmpeg_ir::features::Feature,
    bodies: &mut BTreeSet<BodyId>,
    inputs: &BodySelection,
    incomplete: bool,
) -> Result<(), (FeatureBoundary, UnsupportedBodyCensusReason)> {
    if incomplete {
        return Err((
            feature_boundary(feature),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    }
    let Some(inputs) = explicit_body_selection(inputs) else {
        return Err((
            feature_boundary(feature),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    };
    let input_set = inputs.iter().cloned().collect::<BTreeSet<_>>();
    if inputs.iter().any(|input| !bodies.contains(input))
        || feature.evaluation.outputs().is_empty()
        || feature
            .evaluation
            .outputs()
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            != feature.evaluation.outputs().len()
        || feature
            .evaluation
            .outputs()
            .iter()
            .any(|output| bodies.contains(output) && !input_set.contains(output))
    {
        return Err((
            feature_boundary(feature),
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    for input in inputs {
        bodies.remove(&input);
    }
    bodies.extend(feature.evaluation.outputs().iter().cloned());
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResolvedBodyRetentionMode {
    DeleteSelected,
    KeepSelected,
}

impl TryFrom<BodyRetentionMode> for ResolvedBodyRetentionMode {
    type Error = UnsupportedBodyCensusReason;

    fn try_from(mode: BodyRetentionMode) -> Result<Self, Self::Error> {
        match mode {
            BodyRetentionMode::DeleteSelected => Ok(Self::DeleteSelected),
            BodyRetentionMode::KeepSelected => Ok(Self::KeepSelected),
            BodyRetentionMode::Unresolved => {
                Err(UnsupportedBodyCensusReason::IncompleteFeatureDefinition)
            }
        }
    }
}

fn apply_complete_body_retention(
    feature: &cadmpeg_ir::features::Feature,
    bodies: &mut BTreeSet<BodyId>,
    selection: &BodySelection,
    mode: ResolvedBodyRetentionMode,
    incomplete: bool,
) -> Result<(), (FeatureBoundary, UnsupportedBodyCensusReason)> {
    if incomplete {
        return Err((
            feature_boundary(feature),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    }
    if mode == ResolvedBodyRetentionMode::DeleteSelected
        && matches!(selection, BodySelection::Local { .. })
        && feature.evaluation.outputs().is_empty()
    {
        return Ok(());
    }
    let Some(selected) = explicit_body_selection(selection) else {
        return Err((
            feature_boundary(feature),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    };
    if !feature.evaluation.outputs().is_empty()
        || selected.iter().any(|body| !bodies.contains(body))
    {
        return Err((
            feature_boundary(feature),
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    match mode {
        ResolvedBodyRetentionMode::DeleteSelected => {
            for body in selected {
                bodies.remove(&body);
            }
        }
        ResolvedBodyRetentionMode::KeepSelected => bodies.retain(|body| selected.contains(body)),
    }
    Ok(())
}

fn preserve_complete_body_targets(
    feature: &cadmpeg_ir::features::Feature,
    bodies: &BTreeSet<BodyId>,
    targets: &BodySelection,
    tools: &BodySelection,
    incomplete: bool,
) -> Result<(), (FeatureBoundary, UnsupportedBodyCensusReason)> {
    if incomplete {
        return Err((
            feature_boundary(feature),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    }
    let (Some(targets), Some(tools)) = (
        explicit_body_selection(targets),
        explicit_body_selection(tools),
    ) else {
        return Err((
            feature_boundary(feature),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    };
    if feature.evaluation.outputs().as_slice() != targets
        || targets.iter().any(|target| !bodies.contains(target))
        || tools.iter().any(|tool| !bodies.contains(tool))
    {
        return Err((
            feature_boundary(feature),
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
) -> Result<(), (FeatureBoundary, UnsupportedBodyCensusReason)> {
    if incomplete {
        return Err((
            feature_boundary(feature),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    }
    let Some(occurrence_count) = occurrence_count else {
        return Err((
            feature_boundary(feature),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    };
    if seeds
        .iter()
        .any(|seed| !matches!(seed, PatternSeed::Bodies(_)))
    {
        return Err((
            feature_boundary(feature),
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
            feature_boundary(feature),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    };
    let seed_bodies = seed_bodies.into_iter().flatten().collect::<Vec<_>>();
    let expected_outputs = seed_bodies
        .len()
        .checked_mul(occurrence_count.saturating_sub(1));
    if expected_outputs != Some(feature.evaluation.outputs().len())
        || seed_bodies.iter().collect::<BTreeSet<_>>().len() != seed_bodies.len()
        || seed_bodies.iter().any(|body| !bodies.contains(body))
        || feature
            .evaluation
            .outputs()
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            != feature.evaluation.outputs().len()
        || feature
            .evaluation
            .outputs()
            .iter()
            .any(|output| bodies.contains(output))
    {
        return Err((
            feature_boundary(feature),
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    bodies.extend(feature.evaluation.outputs().iter().cloned());
    Ok(())
}

fn explicit_body_selection(selection: &BodySelection) -> Option<Vec<BodyId>> {
    let bodies = match selection {
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => bodies.clone(),
        BodySelection::ResolvedSet { members } => members.bodies().cloned().collect(),
        BodySelection::Unresolved
        | BodySelection::Historical { .. }
        | BodySelection::HistoricalSet { .. }
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
        && (feature.evaluation.outputs().is_empty()
            || (feature
                .evaluation
                .outputs()
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                == feature.evaluation.outputs().len()
                && feature
                    .evaluation
                    .outputs()
                    .iter()
                    .all(|output| bodies.contains(output))))
}

#[cfg(test)]
mod tests;
