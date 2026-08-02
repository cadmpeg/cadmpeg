// SPDX-License-Identifier: Apache-2.0
//! Neutral evaluation of Siemens NX feature-history effects.

use std::collections::BTreeSet;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{BodySelection, BooleanOp, FeatureDefinition, FeatureId, Length};
use cadmpeg_ir::ids::BodyId;

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

fn active_configuration_is_admitted(ir: &CadIr, saved: &BTreeSet<BodyId>) -> bool {
    if ir.model.configurations.is_empty() {
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
        && !crate::decode::active_configuration_state_is_incomplete(ir, configuration)
}

fn rederived_body_census(
    ir: &CadIr,
) -> Result<BTreeSet<BodyId>, (FeatureId, UnsupportedBodyCensusReason)> {
    let mut bodies = BTreeSet::new();
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
            None => {
                return Err((
                    feature.id.clone(),
                    UnsupportedBodyCensusReason::UnresolvedSuppression,
                ));
            }
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
            | FeatureDefinition::Sketch { .. } => {
                if !feature.outputs.is_empty() {
                    return Err((
                        feature.id.clone(),
                        UnsupportedBodyCensusReason::InvalidOutputLineage,
                    ));
                }
            }
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
                preserve_complete_single_output(feature, &bodies, false)?;
            }
            FeatureDefinition::Block { .. } => {
                return Err((
                    feature.id.clone(),
                    UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                ));
            }
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
            FeatureDefinition::Hole { .. } => {
                preserve_complete_single_output(
                    feature,
                    &bodies,
                    crate::decode::hole_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Chamfer { .. } => {
                preserve_complete_single_output(
                    feature,
                    &bodies,
                    crate::decode::chamfer_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Fillet { .. } => {
                preserve_complete_single_output(
                    feature,
                    &bodies,
                    crate::decode::fillet_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::FaceBlend { .. } => {
                preserve_complete_single_output(
                    feature,
                    &bodies,
                    crate::decode::face_blend_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::OffsetSurface { .. } => {
                preserve_complete_single_output(
                    feature,
                    &bodies,
                    crate::decode::offset_surface_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Thicken { .. } => {
                preserve_complete_single_output(
                    feature,
                    &bodies,
                    crate::decode::thicken_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Draft { .. } => {
                preserve_complete_single_output(
                    feature,
                    &bodies,
                    crate::decode::draft_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::ReplaceFace { .. } => {
                preserve_complete_single_output(
                    feature,
                    &bodies,
                    crate::decode::replace_face_definition_is_incomplete(feature),
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

fn preserve_complete_single_output(
    feature: &cadmpeg_ir::features::Feature,
    bodies: &BTreeSet<BodyId>,
    incomplete: bool,
) -> Result<(), (FeatureId, UnsupportedBodyCensusReason)> {
    if incomplete {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
        ));
    }
    let [output] = feature.outputs.as_slice() else {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    };
    if !bodies.contains(output) {
        return Err((
            feature.id.clone(),
            UnsupportedBodyCensusReason::InvalidOutputLineage,
        ));
    }
    Ok(())
}

fn explicit_body_selection(selection: &BodySelection) -> Option<&[BodyId]> {
    let bodies = match selection {
        BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => bodies,
        BodySelection::Unresolved
        | BodySelection::Historical { .. }
        | BodySelection::Generated { .. }
        | BodySelection::Local { .. }
        | BodySelection::Native(_) => return None,
    };
    (!bodies.is_empty() && bodies.iter().collect::<BTreeSet<_>>().len() == bodies.len())
        .then_some(bodies)
}

fn positive_length(length: Length) -> bool {
    length.0.is_finite() && length.0 > 0.0
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use cadmpeg_ir::features::{
        Angle, BodyRetentionMode, ChamferGroup, ChamferSpec, ConfigurationBodies,
        ConfigurationFeatureState, ConfigurationId, DesignConfiguration, EdgeSelection,
        FaceSelection, Feature, FilletGroup, HoleKind, HolePlacement, RadiusSpec, Termination,
        ThickenSide,
    };
    use cadmpeg_ir::ids::FaceId;
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
            active: true,
            source_index: Some(0),
            name: "Model".to_string(),
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
    fn incomplete_hole_reports_the_feature_semantic_boundary() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        let mut hole = complete_hole(body);
        let FeatureDefinition::Hole { placements, .. } = &mut hole.definition else {
            unreachable!("hole fixture")
        };
        placements.clear();
        ir.model.features.push(hole);

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("hole".to_string())),
                reason: UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
            }
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
        let mut hole = complete_hole(body);
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
    fn delete_body_waits_for_historical_body_identity_evaluation() {
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

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("delete".to_string())),
                reason: UnsupportedBodyCensusReason::UnsupportedFeatureDefinition,
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
    fn incomplete_chamfer_stops_before_applying_its_body_effect() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        ir.model.features.push(body_preserving_feature(
            "chamfer",
            1,
            body,
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

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("chamfer".to_string())),
                reason: UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
            }
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
                pull_direction: Some(Vector3::new(0.0, 0.0, 1.0)),
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
    fn overlapping_replace_face_operands_are_incomplete() {
        let mut ir = complete_block_ir();
        let body = ir.model.bodies[0].id.clone();
        let faces = FaceSelection::Faces(vec![FaceId("face".to_string())]);
        ir.model.features.push(body_preserving_feature(
            "replace-face",
            1,
            body,
            FeatureDefinition::ReplaceFace {
                targets: faces.clone(),
                replacements: faces,
            },
        ));

        assert_eq!(
            evaluate_saved_body_census(&ir),
            BodyCensusEvaluation::Unsupported {
                feature: Some(FeatureId("replace-face".to_string())),
                reason: UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
            }
        );
    }
}
