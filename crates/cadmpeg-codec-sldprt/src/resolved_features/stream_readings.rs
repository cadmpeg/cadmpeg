// SPDX-License-Identifier: Apache-2.0
//! A sketch feature's marker sketch against the profile-stream sketches that
//! read the same profile from a nested Parasolid stream.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::{
    Feature, FeatureDefinition, FeatureId, FeatureOperation, PlanarProfileRef, ProfileRef,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometryDefinition, SketchId, SketchPlacement,
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

/// How a feature's marker sketch stands beside the profile-stream sketch of
/// one circle that is bound to the feature.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum MarkerReading {
    /// The marker sketch binds the feature, with this placement.
    Binds(SketchPlacement),
    /// The stream sketch stays bound, and the marker sketch joins the model
    /// unbound for this reason.
    Unbound(UnboundReason),
    /// The stream sketch stays bound, and the marker sketch adds nothing.
    Stays,
}

/// Why a feature's marker sketch stays unbound beside its stream circle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnboundReason {
    /// It holds the stream circle's loop among other loops.
    HoldsCircle,
    /// Its normal is not parallel to the stream sketch's.
    OtherPlane,
    /// Its placement or the stream sketch's is unresolved.
    PlacementUnresolved,
}

impl UnboundReason {
    fn text(self) -> &'static str {
        match self {
            Self::HoldsCircle => "it holds that circle's loop among other loops",
            Self::OtherPlane => "its normal is not parallel to that sketch's normal",
            Self::PlacementUnresolved => {
                "its placement or that sketch's is unresolved, so their planes cannot be compared"
            }
        }
    }
}

/// A feature's marker sketch that stays unbound beside the stream circle
/// bound to the feature.
#[derive(Debug, Clone)]
pub(crate) struct UnboundMarkerReading {
    pub(super) marker: SketchId,
    pub(super) stream: SketchId,
    pub(super) feature: FeatureId,
    pub(super) reason: UnboundReason,
}

/// The largest sine of the angle between two normals that are parallel.
const PARALLEL_SINE: f64 = 1.0e-6;

/// How `marker`, the sketch a feature's own marker records give, stands
/// beside `stream`, the profile-stream sketch bound to the feature. Only a
/// stream sketch of one circle gives way, and only to a marker sketch whose
/// every entity is `planar` and that does not repeat the circle's loop. A
/// marker sketch that a revolve takes as its profile binds whatever its
/// plane. Any other binds only when its normal is parallel to the stream
/// sketch's, and then takes the stream sketch's plane.
pub(super) fn marker_reading(
    marker: &Sketch,
    stream: &Sketch,
    entities: &EntityIndex<'_>,
    planar: bool,
    revolve_profile: bool,
) -> MarkerReading {
    if !planar || !is_one_circle(stream, entities) || same_loops(marker, stream, entities) {
        return MarkerReading::Stays;
    }
    if holds_loop_of(marker, stream, entities) {
        return MarkerReading::Unbound(UnboundReason::HoldsCircle);
    }
    match stream_plane_placement(marker, stream) {
        Ok(placement) => MarkerReading::Binds(placement),
        Err(_) if revolve_profile => MarkerReading::Binds(marker.placement),
        Err(reason) => MarkerReading::Unbound(reason),
    }
}

/// The placement of `marker` moved onto the plane of `stream`: the stream
/// sketch's normal and u axis, through the marker sketch's origin projected
/// onto that plane, so the marker sketch keeps its in-plane coordinates.
fn stream_plane_placement(
    marker: &Sketch,
    stream: &Sketch,
) -> Result<SketchPlacement, UnboundReason> {
    let (Some((marker_origin, marker_normal, _)), Some((stream_origin, stream_normal, u_axis))) =
        (marker.resolved_placement(), stream.resolved_placement())
    else {
        return Err(UnboundReason::PlacementUnresolved);
    };
    let (marker_origin, stream_origin) = (marker_origin.get(), stream_origin.get());
    let (Some(marker_normal), Some(stream_normal)) =
        (marker_normal.get().unit(), stream_normal.get().unit())
    else {
        return Err(UnboundReason::PlacementUnresolved);
    };
    if marker_normal.cross(stream_normal).norm() > PARALLEL_SINE {
        return Err(UnboundReason::OtherPlane);
    }
    let offset = stream_origin.vector_from(marker_origin).dot(stream_normal);
    SketchPlacement::try_resolved(
        marker_origin.translated(stream_normal, offset),
        stream_normal,
        u_axis.get(),
    )
    .map_err(|_| UnboundReason::PlacementUnresolved)
}

/// Whether a revolve takes sketch feature `feature`, bound to `stream`, as
/// its profile.
pub(super) fn is_revolve_profile(
    features: &[Feature],
    feature: &Feature,
    stream: &SketchId,
) -> bool {
    features.iter().any(|candidate| {
        let FeatureDefinition::Operation(FeatureOperation::Revolve { construction, .. }) =
            candidate.evaluation.definition()
        else {
            return false;
        };
        match construction.profile() {
            Some(PlanarProfileRef::Unresolved(owner) | PlanarProfileRef::Native(owner)) => {
                feature.native_ref.as_ref() == Some(owner)
            }
            Some(PlanarProfileRef::Feature(id)) => *id == feature.id,
            Some(PlanarProfileRef::Sketch(sketch)) => sketch == stream,
            _ => false,
        }
    })
}

/// Charges `sketch.marker-reading-unbound` for each of the `unbound` marker
/// readings that no feature or configuration state binds, and
/// `sketch.second-reading` for each profile-stream sketch that no feature or
/// configuration state binds and whose loops repeat those of exactly one
/// sketch of its lane that a sketch feature binds and that no profile stream
/// gives.
pub(crate) fn note_stream_readings(
    model: &Model,
    unbound: &[UnboundMarkerReading],
    losses: &mut Vec<LossNote>,
) {
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
    for reading in unbound
        .iter()
        .filter(|reading| !referenced.contains(&reading.marker))
    {
        losses.push(SldprtLossCode::SketchMarkerReadingUnbound.note(format!(
            "sketch {}: the marker records of feature {} give this sketch, and the feature stays bound to profile-stream sketch {} of one circle: {}",
            reading.marker,
            reading.feature,
            reading.stream,
            reading.reason.text()
        )));
    }
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

/// Whether one loop of `marker` has the length of the one loop of `stream`
/// within the linear resolution per curve.
fn holds_loop_of(marker: &Sketch, stream: &Sketch, entities: &EntityIndex<'_>) -> bool {
    let (Some(marker), Some(stream)) = (
        loop_lengths(marker, entities),
        loop_lengths(stream, entities),
    ) else {
        return false;
    };
    let [(circle, circle_tolerance)] = stream.as_slice() else {
        return false;
    };
    marker
        .iter()
        .any(|(length, tolerance)| (length - circle).abs() <= tolerance + circle_tolerance)
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
