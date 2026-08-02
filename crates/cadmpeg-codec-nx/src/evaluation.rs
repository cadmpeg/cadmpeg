// SPDX-License-Identifier: Apache-2.0
//! Neutral evaluation of Siemens NX feature-history effects.

use std::collections::BTreeSet;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    BodyRetentionMode, BodySelection, BooleanOp, FeatureDefinition, FeatureId, Length,
};
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
            FeatureDefinition::Loft { op, .. } => {
                apply_complete_boolean_outputs(
                    feature,
                    &mut bodies,
                    *op,
                    crate::decode::loft_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Extrude { op, .. } => {
                apply_complete_boolean_outputs(
                    feature,
                    &mut bodies,
                    *op,
                    crate::decode::extrude_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Revolve { op, .. } => {
                apply_complete_boolean_outputs(
                    feature,
                    &mut bodies,
                    *op,
                    crate::decode::revolve_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::Rib { op, .. } => {
                apply_complete_boolean_outputs(
                    feature,
                    &mut bodies,
                    *op,
                    crate::decode::rib_definition_is_incomplete(feature),
                )?;
            }
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
            FeatureDefinition::Combine { target, tools, .. } => {
                apply_complete_body_combine(
                    feature,
                    &mut bodies,
                    target,
                    tools,
                    crate::decode::combine_definition_is_incomplete(feature),
                )?;
            }
            FeatureDefinition::SewBodies {
                bodies: selection, ..
            } => {
                apply_complete_body_replacement(
                    feature,
                    &mut bodies,
                    selection,
                    crate::decode::sew_bodies_definition_is_incomplete(feature),
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
    for tool in tools {
        bodies.remove(tool);
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
        ConfigurationFeatureState, ConfigurationId, CurveProjectionDirection,
        CurveProjectionDirectionState, DesignConfiguration, EdgeSelection, ExtrudeDirection,
        ExtrudeExtent, ExtrudeSide, ExtrudeStart, FaceSelection, Feature, FilletGroup, HoleKind,
        HolePlacement, PathRef, ProfileRef, RadiusSpec, RevolutionConstruction, RibConstruction,
        RibDraft, SketchSpace, SweepMode, Termination, ThickenSide,
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
                profile: None,
                sections: Vec::new(),
                path: None,
                mode: SweepMode::Unresolved,
                orientation: None,
                transition: None,
                transformation: None,
                path_tangent: false,
                linearize: false,
                twist: None,
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
