// SPDX-License-Identifier: Apache-2.0
//! Signed start offsets of selected-face Extrude starts.
//!
//! The stored offset of a selected-face start runs along the outward normal
//! of the start face. The neutral offset runs along the travel direction.

use std::collections::{BTreeSet, HashMap};

use cadmpeg_ir::features::extents::{ExtrudeDirection, ExtrudeStart};
use cadmpeg_ir::features::profiles::ProfileRef;
use cadmpeg_ir::features::selections::FaceSelection;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::sketches::Sketch;

use crate::history_records::{AsmHistoricalTopology, AsmHistory};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;

/// The unit travel direction of an Extrude.
pub(crate) fn travel(
    profile: &ProfileRef,
    direction: &ExtrudeDirection,
    sketches: &[Sketch],
) -> Option<Vector3> {
    let sweep = match direction {
        ExtrudeDirection::Explicit { vector, .. } => vector.get(),
        ExtrudeDirection::ProfileNormal {} | ExtrudeDirection::ReversedProfileNormal {} => {
            let sketch = crate::design::face_resolve::extrude_profile_sketch_id(profile)
                .and_then(|id| sketches.iter().find(|sketch| sketch.id == *id))?;
            let (_, normal, _) = sketch.resolved_placement()?;
            if matches!(direction, ExtrudeDirection::ReversedProfileNormal {}) {
                normal.get().scale(-1.0)
            } else {
                normal.get()
            }
        }
        ExtrudeDirection::Unresolved {} => return None,
    };
    sweep.unit()
}

/// The offset along `travel` of a start offset stored along `outward`, when
/// the two directions are parallel or antiparallel.
pub(crate) fn along_travel(
    stored: Length,
    outward: Vector3,
    travel: Vector3,
    angular_tolerance: f64,
) -> Option<Length> {
    let cosine = outward.unit()?.dot(travel.unit()?);
    if (cosine.abs() - 1.0).abs() > angular_tolerance {
        return None;
    }
    Length::new(stored.get() * cosine.signum())
}

fn sole<T>(mut rows: impl Iterator<Item = T>) -> Option<T> {
    let row = rows.next()?;
    rows.next().is_none().then_some(row)
}

/// The outward normal of a planar face slot in a historical topology.
fn historical_outward(topology: &AsmHistoricalTopology, slot: i64) -> Option<Vector3> {
    if !topology.faces.contains(&slot) {
        return None;
    }
    let surface = sole(
        topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == slot)
            .map(|binding| binding.carrier),
    )?;
    let normal = sole(
        topology
            .surface_planes
            .iter()
            .filter(|plane| plane.surface == surface)
            .map(|plane| plane.normal),
    )?;
    let forward = sole(
        topology
            .face_senses
            .iter()
            .filter(|sense| sense.face == slot)
            .map(|sense| sense.forward),
    )?;
    Some(if forward { normal } else { normal.scale(-1.0) })
}

fn start_slot(face: &FaceSelection) -> Option<i64> {
    match face {
        FaceSelection::Resolved { faces, .. } => match faces.as_slice() {
            [face] => super::body_closures::stable_ref(face.as_str()),
            _ => None,
        },
        FaceSelection::Historical { faces, .. } => match faces.as_slice() {
            [face] => face.as_str().rsplit_once(':')?.1.parse().ok(),
            _ => None,
        },
        _ => None,
    }
}

/// Rewrite the start offset of each Extrude whose start face resolves in the
/// feature's input state, and return the ids of the rewritten features.
pub(crate) fn orient_historical_starts(
    features: &mut [Feature],
    sketches: &[Sketch],
    scopes: &[DesignParameterScope],
    scope_histories: &HashMap<String, String>,
    histories: &[AsmHistory],
    angular_tolerance: f64,
) -> BTreeSet<String> {
    let mut oriented = BTreeSet::new();
    for feature in features {
        let FeatureDefinition::Operation(FeatureOperation::Extrude {
            start:
                ExtrudeStart::FromFace {
                    face,
                    offset: Some(stored),
                },
            ..
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        if stored.get() == 0.0 {
            continue;
        }
        let Some(slot) = start_slot(face) else {
            continue;
        };
        let stored = *stored;
        let Some(state) = super::face_selections::face_selection_state(
            feature,
            scopes,
            scope_histories,
            histories,
        ) else {
            continue;
        };
        let history = &histories[state.history];
        let Some(outward) = history
            .topology(&history.states()[state.previous])
            .and_then(|before| historical_outward(before, slot))
        else {
            continue;
        };
        let mut rewritten = false;
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile,
                direction,
                start: ExtrudeStart::FromFace { offset, .. },
                ..
            }) = definition
            else {
                return;
            };
            let Some(along) = travel(profile, direction, sketches)
                .and_then(|travel| along_travel(stored, outward, travel, angular_tolerance))
            else {
                return;
            };
            *offset = Some(along);
            rewritten = true;
        });
        if rewritten {
            oriented.insert(feature.id.as_str().to_owned());
        }
    }
    oriented
}

/// One loss for each selected-face start offset that keeps its stored sign.
pub(crate) fn unoriented_start_losses(
    features: &[Feature],
    oriented: &BTreeSet<String>,
) -> Vec<LossNote> {
    features
        .iter()
        .filter(|feature| {
            matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Extrude {
                    start: ExtrudeStart::FromFace { offset: Some(offset), .. },
                    ..
                }) if offset.get() != 0.0
            ) && !oriented.contains(feature.id.as_str())
        })
        .map(|feature| {
            F3dLossCode::FeatureExtrudeStartOffsetDirectionAssumed.note(format!(
                "Extrude {} reads its start-face offset along the travel direction because the start face normal is not proven parallel to the travel (DR-1064).",
                feature.id
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests;
