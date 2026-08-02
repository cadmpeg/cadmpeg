// SPDX-License-Identifier: Apache-2.0
//! Neutral evaluation of Siemens NX feature-history effects.

use std::collections::BTreeSet;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, Length};
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
    active.next().is_none()
        && saved.is_empty()
        && configuration
            .bodies
            .resolved()
            .is_some_and(<[BodyId]>::is_empty)
        && configuration.feature_states.is_empty()
}

fn rederived_body_census(
    ir: &CadIr,
) -> Result<BTreeSet<BodyId>, (FeatureId, UnsupportedBodyCensusReason)> {
    let mut bodies = BTreeSet::new();
    let mut seen_features = BTreeSet::new();
    let mut previous_ordinal = None;
    for feature in &ir.model.features {
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
            FeatureDefinition::Block { .. } => {
                return Err((
                    feature.id.clone(),
                    UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                ));
            }
            FeatureDefinition::Hole { .. }
                if !crate::decode::hole_definition_is_incomplete(feature) =>
            {
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
            }
            FeatureDefinition::Hole { .. } => {
                return Err((
                    feature.id.clone(),
                    UnsupportedBodyCensusReason::IncompleteFeatureDefinition,
                ));
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

fn positive_length(length: Length) -> bool {
    length.0.is_finite() && length.0 > 0.0
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use cadmpeg_ir::features::{Feature, HoleKind, HolePlacement, Termination};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::{Body, BodyKind};

    use super::*;

    fn complete_block_ir() -> CadIr {
        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let body = BodyId("body".to_string());
        ir.model.bodies.push(Body {
            id: body.clone(),
            kind: BodyKind::Solid,
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
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
            },
            native_ref: None,
        });
        ir
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
}
