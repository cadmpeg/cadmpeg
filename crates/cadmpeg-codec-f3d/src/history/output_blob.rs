// SPDX-License-Identifier: Apache-2.0
//! History binding through the BREP blob that holds a feature's outputs.

/// The state pair of the only history from the BREP blob that holds every
/// body `feature` outputs, or that a Base Feature resolves. Components of
/// one design share state numbers, and each component's bodies live in
/// their own blob.
pub(super) fn output_blob_history_pair<'a>(
    feature: &cadmpeg_ir::features::Feature,
    state_id: i64,
    previous_state_id: i64,
    histories: &'a [super::AsmHistory],
) -> Option<(
    &'a super::AsmHistory,
    &'a super::AsmDeltaState,
    &'a super::AsmDeltaState,
)> {
    let source = output_brep_source(feature)?;
    let mut matching = histories
        .iter()
        .filter(|history| super::historical_brep_source(&history.id) == Some(source))
        .filter_map(|history| {
            super::history_state_pair(history, state_id, previous_state_id, false)
        });
    matching.next().filter(|_| matching.next().is_none())
}

/// The BREP blob that holds every body a feature outputs, or that a Base
/// Feature resolves, when there is exactly one.
fn output_brep_source(feature: &cadmpeg_ir::features::Feature) -> Option<&str> {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    let outputs = feature.evaluation.outputs();
    let bodies: Vec<&str> = if outputs.is_empty() {
        let (FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Resolved { bodies, .. },
        })
        | FeatureDefinition::PostProcess {
            operation:
                FeatureOperation::BaseFeature {
                    bodies: BodySelection::Resolved { bodies, .. },
                },
            ..
        }) = feature.evaluation.definition()
        else {
            return None;
        };
        bodies.iter().map(cadmpeg_ir::ids::BodyId::as_str).collect()
    } else {
        outputs
            .iter()
            .map(cadmpeg_ir::ids::BodyId::as_str)
            .collect()
    };
    let mut sources = bodies.iter().map(|body| {
        body.strip_prefix("f3d:brep/")
            .and_then(|rest| rest.split_once("/brep:"))
            .map(|(source, _)| source)
    });
    let first = sources.next()??;
    sources.all(|source| source == Some(first)).then_some(first)
}
