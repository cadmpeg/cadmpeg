// SPDX-License-Identifier: Apache-2.0
//! A sketch feature's marker sketch against the profile-stream sketches that
//! read the same profile from a nested Parasolid stream.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, PlanarProfileRef, ProfileRef};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometryDefinition, SketchId,
};

use super::sketch_projection::is_profile_stream_sketch;
use crate::brep::graph::LINEAR_RESOLUTION_MM;
use crate::loss::SldprtLossCode;

/// The sketch entities of the sketches a comparison reads, by id.
pub(super) struct EntityIndex<'a>(HashMap<&'a SketchEntityId, &'a SketchEntity>);

impl<'a> EntityIndex<'a> {
    pub(super) fn new(entities: impl IntoIterator<Item = &'a SketchEntity>) -> Self {
        Self(
            entities
                .into_iter()
                .map(|entity| (entity.id(), entity))
                .collect(),
        )
    }
}

/// Whether `marker`, the sketch a feature's own marker records give, binds in
/// place of `stream`, the profile-stream sketch bound to the feature:
/// `stream` is one circle whose loop `marker` does not repeat, and every
/// entity of `marker` has planar geometry.
pub(super) fn marker_reading_binds(
    marker: &Sketch,
    stream: &Sketch,
    entities: &EntityIndex<'_>,
) -> bool {
    entities.0.values().all(|entity| {
        entity.sketch != marker.id
            || !matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Native { .. }
            )
    }) && is_one_circle(stream, entities)
        && !same_loops(marker, stream, entities)
}

/// Charges `sketch.second-reading` for each profile-stream sketch that no
/// feature or configuration state binds and whose loops repeat those of
/// exactly one sketch of its lane that a sketch feature binds and that no
/// profile stream gives.
pub(crate) fn note_second_readings(model: &Model, losses: &mut Vec<LossNote>) {
    let definitions = model
        .features
        .iter()
        .map(|feature| feature.evaluation.definition())
        .chain(model.configurations.iter().flat_map(|configuration| {
            configuration
                .feature_states
                .values()
                .map(|state| &state.definition)
        }))
        .collect::<Vec<_>>();
    let referenced = bound_sketches(&definitions);
    let mut feature_sketches = HashSet::new();
    for definition in &definitions {
        if let FeatureDefinition::Operation(FeatureOperation::Sketch { sketch }) = definition {
            if let Some(id) = sketch.id().filter(|id| !is_profile_stream_sketch(id)) {
                feature_sketches.insert(id);
            }
        }
    }
    let read = model
        .sketches
        .iter()
        .filter(|sketch| feature_sketches.contains(&sketch.id))
        .collect::<Vec<_>>();
    if read.is_empty() {
        return;
    }
    let entities = EntityIndex::new(&model.sketch_entities);
    for stream in &model.sketches {
        if !is_profile_stream_sketch(&stream.id) || referenced.contains(&stream.id) {
            continue;
        }
        let mut repeated = read.iter().filter(|sketch| {
            sketch.native_ref == stream.native_ref && same_loops(sketch, stream, &entities)
        });
        let (Some(sketch), None) = (repeated.next(), repeated.next()) else {
            continue;
        };
        losses.push(SldprtLossCode::SketchSecondReading.note(format!(
            "sketch {}: a second reading of sketch {}, which a sketch feature binds; its profile loops repeat that sketch's and no feature binds it",
            stream.id, sketch.id
        )));
    }
}

fn bound_sketches<'a>(definitions: &[&'a FeatureDefinition]) -> HashSet<&'a SketchId> {
    let mut bound = HashSet::new();
    for definition in definitions {
        let profile = match definition {
            FeatureDefinition::Operation(FeatureOperation::Sketch { sketch }) => {
                bound.extend(sketch.id());
                continue;
            }
            FeatureDefinition::Operation(FeatureOperation::SketchBlockDefinition { sketch }) => {
                bound.extend(sketch.as_ref());
                continue;
            }
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: ProfileRef::Planar(profile),
                ..
            }) => Some(profile),
            FeatureDefinition::Operation(FeatureOperation::Revolve { construction, .. }) => {
                construction.profile()
            }
            FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) => {
                shape.referenced_profile()
            }
            _ => None,
        };
        if let Some(PlanarProfileRef::Sketch(sketch)) = profile {
            bound.insert(sketch);
        }
    }
    bound
}

fn is_one_circle(sketch: &Sketch, entities: &EntityIndex<'_>) -> bool {
    let [profile] = sketch.profiles.as_slice() else {
        return false;
    };
    let [entity_use] = profile.as_slice() else {
        return false;
    };
    entities.0.get(&entity_use.entity).is_some_and(|entity| {
        matches!(
            entity.geometry.definition(),
            SketchGeometryDefinition::Circle { .. }
        )
    })
}

/// Whether the two sketches close the same number of loops and their loops,
/// ordered by length, have equal lengths within the linear resolution per
/// curve.
fn same_loops(left: &Sketch, right: &Sketch, entities: &EntityIndex<'_>) -> bool {
    let (Some(left), Some(right)) = (loop_lengths(left, entities), loop_lengths(right, entities))
    else {
        return false;
    };
    !left.is_empty()
        && left.len() == right.len()
        && left
            .iter()
            .zip(&right)
            .all(|((left, left_tolerance), (right, right_tolerance))| {
                (left - right).abs() <= left_tolerance + right_tolerance
            })
}

/// The length of each closed loop of `sketch` and its tolerance, ordered by
/// length, when every curve of every loop is a line, an arc or a circle.
fn loop_lengths(sketch: &Sketch, entities: &EntityIndex<'_>) -> Option<Vec<(f64, f64)>> {
    let mut loops = Vec::new();
    for profile in sketch.profiles.as_slice() {
        let mut length = 0.0;
        let mut tolerance = 0.0;
        for entity_use in profile {
            let entity = entities.0.get(&entity_use.entity)?;
            length += match *entity.geometry.definition() {
                SketchGeometryDefinition::Line { start, end } => {
                    let (start, end) = (start.get(), end.get());
                    (end.u - start.u).hypot(end.v - start.v)
                }
                SketchGeometryDefinition::Circle { radius, .. } => {
                    std::f64::consts::TAU * radius.get()
                }
                SketchGeometryDefinition::Arc {
                    radius,
                    start_angle,
                    end_angle,
                    ..
                } => {
                    let sweep =
                        (end_angle.get() - start_angle.get()).rem_euclid(std::f64::consts::TAU);
                    radius.get() * sweep
                }
                _ => return None,
            };
            tolerance += LINEAR_RESOLUTION_MM;
        }
        loops.push((length, tolerance));
    }
    loops.sort_by(|left, right| left.0.total_cmp(&right.0));
    Some(loops)
}

#[cfg(test)]
mod tests;
