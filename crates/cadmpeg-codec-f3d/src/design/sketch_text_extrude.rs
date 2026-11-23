// SPDX-License-Identifier: Apache-2.0
//! Sketch-text profiles of `Extrude` scopes.

use crate::ids::native_stream;
use crate::records::sketch_geometry::SketchText;
use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::SketchEntity;

/// One sketch-profile operand whose `N+3` record is a text selection.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SketchTextPick {
    /// Record index of the Sketch entity named by the operand.
    pub(crate) sketch_entity: u64,
    /// Record index named by the text selection.
    pub(crate) text_record: u32,
}

/// An Extrude whose every sketch-profile operand carries a text selection.
#[derive(Debug)]
pub(crate) struct SketchTextExtrudeCandidate {
    /// Parameter scope that owns the operands.
    pub(crate) scope_id: String,
    /// The picks, in operand order.
    pub(crate) picks: Vec<SketchTextPick>,
}

/// Give each Extrude whose picks name Sketch texts of the operand's Sketch a
/// profile of those text entities.
///
/// A candidate whose picks do not all name such a text keeps its profile. When
/// it has several operands, it gets the unresolved group-composition loss.
pub(crate) fn bind_sketch_text_extrude_profiles(
    features: &mut [Feature],
    candidates: &[SketchTextExtrudeCandidate],
    texts: &[SketchText],
    sketch_entities: &[SketchEntity],
) -> Vec<LossNote> {
    let mut losses = Vec::new();
    for candidate in candidates {
        let stream = native_stream(&candidate.scope_id);
        let Some(picked) = candidate
            .picks
            .iter()
            .map(|pick| {
                unique(texts.iter().filter(|text| {
                    native_stream(&text.id) == stream
                        && text.record_index == pick.text_record
                        && u64::from(text.owner_reference) == pick.sketch_entity
                }))
            })
            .collect::<Option<Vec<_>>>()
        else {
            if candidate.picks.len() > 1 {
                losses.push(
                    crate::design::decode::operands::sketch_profiles::unresolved_profile_group_loss(
                        candidate.picks.len(),
                    ),
                );
            }
            continue;
        };
        let text_profile = picked
            .iter()
            .map(|text| {
                unique(
                    sketch_entities
                        .iter()
                        .filter(|entity| entity.native_ref.as_deref() == Some(text.id.as_str())),
                )
            })
            .collect::<Option<Vec<_>>>()
            .and_then(|entities| {
                let sketch = &entities.first()?.sketch;
                if entities.iter().any(|entity| &entity.sketch != sketch) {
                    return None;
                }
                PlanarProfileRef::sketch_entities(
                    sketch.clone(),
                    entities.iter().map(|entity| entity.id().clone()).collect(),
                )
                .ok()
            });
        let mut bound = false;
        for feature in features
            .iter_mut()
            .filter(|feature| feature.native_ref.as_deref() == Some(candidate.scope_id.as_str()))
        {
            feature.evaluation.edit(|definition, _| {
                let FeatureDefinition::Operation(FeatureOperation::Extrude { profile, .. }) =
                    definition
                else {
                    return;
                };
                *profile = ProfileRef::Planar(match &text_profile {
                    Some(text_profile) => {
                        bound = true;
                        text_profile.clone()
                    }
                    None => PlanarProfileRef::Native(candidate.scope_id.clone()),
                });
            });
        }
        if !bound {
            losses.push(
                crate::loss::F3dLossCode::FeatureExtrudeSketchTextSkipped.note(format!(
                    "{} sketch-text extrude profile(s) were skipped.",
                    candidate.picks.len(),
                )),
            );
        }
    }
    losses
}

fn unique<T>(mut matches: impl Iterator<Item = T>) -> Option<T> {
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests;
