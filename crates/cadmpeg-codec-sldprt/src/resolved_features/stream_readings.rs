// SPDX-License-Identifier: Apache-2.0
//! A sketch feature's marker sketch against the profile-stream sketches that
//! read the same profile from a nested Parasolid stream.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureOperation, PlanarProfileRef, ProfileRef,
};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometryDefinition, SketchId,
};

use super::sketch_projection::is_profile_stream_sketch;
use crate::brep::graph::LINEAR_RESOLUTION_MM;

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

/// A marker sketch that the marker pass bound to the sketch feature named
/// `feature_name`, read from lane `lane`, in place of the profile-stream
/// sketch `displaced` where it had one.
pub(super) struct BoundMarker {
    pub(super) feature_name: String,
    pub(super) lane: String,
    pub(super) marker: SketchId,
    pub(super) displaced: Option<SketchId>,
}

/// Whether `marker`, the sketch a feature's own marker records give, binds in
/// place of `stream`, the profile-stream sketch bound to the feature: it
/// closes more loops, `stream` is one circle, or its loops repeat those of
/// `stream`.
pub(super) fn marker_reading_binds(
    marker: &Sketch,
    stream: &Sketch,
    entities: &EntityIndex<'_>,
) -> bool {
    marker.profiles.as_slice().len() > stream.profiles.as_slice().len()
        || is_one_circle(stream, entities)
        || same_loops(marker, stream, entities)
}

/// The profile-stream sketches that no feature binds and whose loops repeat
/// those of a marker sketch in `bound`, as (stream, marker) pairs. A stream
/// sketch qualifies when it is the one the marker sketch displaced or carries
/// the name of the marker sketch's feature, in the marker sketch's lane.
pub(super) fn second_readings(
    bound: &[BoundMarker],
    features: &[Feature],
    sketches: &[Sketch],
    sketch_entities: &[SketchEntity],
) -> Vec<(SketchId, SketchId)> {
    if bound.is_empty() {
        return Vec::new();
    }
    let referenced = bound_sketches(features);
    let entities = EntityIndex::new(sketch_entities);
    let by_id = sketches
        .iter()
        .map(|sketch| (&sketch.id, sketch))
        .collect::<HashMap<_, _>>();
    let mut readings = Vec::new();
    for binding in bound {
        let Some(marker) = by_id.get(&binding.marker) else {
            continue;
        };
        for stream in sketches {
            if stream.id == binding.marker
                || !is_profile_stream_sketch(&stream.id)
                || referenced.contains(&stream.id)
                || stream.native_ref.as_deref() != Some(binding.lane.as_str())
                || (binding.displaced.as_ref() != Some(&stream.id)
                    && stream.name.as_deref() != Some(binding.feature_name.as_str()))
            {
                continue;
            }
            if same_loops(marker, stream, &entities) {
                readings.push((stream.id.clone(), binding.marker.clone()));
            }
        }
    }
    readings
}

fn bound_sketches(features: &[Feature]) -> HashSet<&SketchId> {
    let mut bound = HashSet::new();
    for feature in features {
        let profile = match feature.evaluation.definition() {
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
