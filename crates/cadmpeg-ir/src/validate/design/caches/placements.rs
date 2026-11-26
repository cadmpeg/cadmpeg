// SPDX-License-Identifier: Apache-2.0
//! Caches of a placement that in-IR arithmetic decides (part 10, Caches):
//! a `Transformation`'s `resolved` against its composed steps, a
//! `Place.placement`'s `resolved` with `Place.scale` against
//! `Occurrence.transform` (D56), a mirror result's `Occurrence.transform`
//! against its seed's placement reflected across the plane, and an ordered
//! container's `placement` against the `Body.transform` of its bodies. Each
//! is compared within the document's linear and angular tolerances, and
//! each disagreement is a Warning.
//!
//! A step, a factor, or a reference whose value the IR holds only through a
//! parameter, a construction with no `resolved`, a kernel, or a state is not
//! composed, and the cache that needs it is not checked.

use std::collections::BTreeMap;

use super::super::frames::frame_transform;
use super::super::records::DesignRecords;
use super::super::warning;
use crate::design::construction::{AxisValue, PlaneValue};
use crate::design::container::Container;
use crate::design::dimension::Dimension;
use crate::design::feature::FeatureOperation;
use crate::design::ops::assembly::{
    MirrorComponents, MirrorOrientation, MirrorPivot, MirrorReversal, Place, PlacementBase,
};
use crate::design::ops::body::{ScaleFactors, TransformStep, Transformation};
use crate::design::refs::Member;
use crate::design::selection::{PickContext, PickTarget, Picks, StepRef};
use crate::math::portable::Portable;
use crate::math::Vector3;
use crate::report::check::Finding;
use crate::transform::Transform;

/// The document's linear and angular tolerances.
#[derive(Clone, Copy)]
struct Within {
    linear: f64,
    angular: f64,
}

impl Within {
    fn agree(self, cached: Transform, composed: Transform) -> bool {
        cached.agrees_with(composed, self.linear, self.angular)
    }
}

pub(super) fn check_placements(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let tolerances = index.ir.tolerances;
    let within = Within {
        linear: tolerances.linear.get(),
        angular: tolerances.angular.get(),
    };
    let occurrences: BTreeMap<&str, Option<Transform>> = index
        .ir
        .model
        .occurrences
        .iter()
        .map(|occurrence| (occurrence.id.as_str(), occurrence.transform))
        .collect();
    for (holder, _, definition) in index.feature_definitions() {
        match &definition.operation {
            FeatureOperation::TransformBodies(transform) => {
                check_transformation(holder, &transform.transformation, within, findings);
            }
            FeatureOperation::MoveFaces(moved) => {
                check_transformation(holder, &moved.transformation, within, findings);
            }
            FeatureOperation::Place(place) => {
                check_place(holder, place, &occurrences, within, findings);
            }
            FeatureOperation::MirrorComponents(mirror) => {
                check_mirror(holder, mirror, &occurrences, within, findings);
            }
            _ => {}
        }
    }
    check_containers(index, within, findings);
}

/// A transformation's `resolved` is its steps composed in order.
fn check_transformation(
    holder: &str,
    transformation: &Transformation,
    within: Within,
    findings: &mut Vec<Finding>,
) {
    let Some(resolved) = transformation.resolved else {
        return;
    };
    let mut composed = Transform::identity();
    for step in &transformation.steps {
        let Some(step) = step_transform(step) else {
            return;
        };
        let Ok(next) = step.compose(composed) else {
            return;
        };
        composed = next;
    }
    if !within.agree(resolved, composed) {
        warning(
            findings,
            holder,
            crate::rule!(
                "design.transformation-resolved-not-steps-composed",
                "a Transformation's resolved is not its steps composed, {:?}",
                composed.affine_rows()
            ),
        );
    }
}

/// The transform of one step, where its values are literal and its
/// references' constructions state `resolved`.
fn step_transform(step: &TransformStep) -> Option<Transform> {
    match step {
        TransformStep::Translate {
            direction,
            distance,
        } => {
            let direction = direction.resolved.map(Vector3::from)?;
            translation(direction.scale(literal(distance)?.get()))
        }
        TransformStep::Components {
            frame: Some(frame),
            delta,
        } => {
            let frame = frame.resolved.as_ref()?;
            let [x, y, z] = [
                literal(&delta[0])?.get(),
                literal(&delta[1])?.get(),
                literal(&delta[2])?.get(),
            ];
            translation(
                Vector3::from(frame.x_axis).scale(x)
                    + Vector3::from(frame.y_axis).scale(y)
                    + Vector3::from(frame.z_axis).scale(z),
            )
        }
        TransformStep::PointToPoint { from, to } => {
            translation(to.resolved?.get().vector_from(from.resolved?.get()))
        }
        TransformStep::Rotate { axis, angle } => {
            rotation(axis.resolved.as_ref()?, literal(angle)?.get())
        }
        TransformStep::FrameToFrame { from, to } => {
            let from = frame_transform(from.resolved.as_ref()?)?;
            let to = frame_transform(to.resolved.as_ref()?)?;
            to.compose(from.try_inverse_affine().ok()?).ok()
        }
        TransformStep::Fixed(transform) => Some(*transform),
        TransformStep::Components { frame: None, .. }
        | TransformStep::Scale { .. }
        | TransformStep::Constrained { .. }
        | TransformStep::Native(_) => None,
    }
}

/// Under `Parent`, `Occurrence.transform` is the placement's `resolved`
/// composed with `scale` (D56). A placement relative to its own occurrence is
/// not composed.
fn check_place(
    holder: &str,
    place: &Place,
    occurrences: &BTreeMap<&str, Option<Transform>>,
    within: Within,
    findings: &mut Vec<Finding>,
) {
    if place.base != Some(PlacementBase::Parent) {
        return;
    }
    let Some(placement) = place
        .placement
        .as_ref()
        .and_then(|placement| placement.resolved.as_ref())
        .and_then(frame_transform)
    else {
        return;
    };
    let Some(Some(transform)) = occurrences.get(place.occurrence.as_str()) else {
        return;
    };
    let composed = match &place.scale {
        None => placement,
        Some(scale) => {
            let Some(scale) = scale_transform(scale) else {
                return;
            };
            let Ok(composed) = placement.compose(scale) else {
                return;
            };
            composed
        }
    };
    if !within.agree(*transform, composed) {
        warning(
            findings,
            holder,
            crate::rule!("design.transform-occurrence-not-place-placement-resolved", 
                "the transform of occurrence {} is not its Place.placement's resolved composed with its scale",
                place.occurrence.as_str()
            ),
        );
    }
}

/// A result with no orientation is its seed's placement reflected across the
/// plane; a `SeedAxes` copy about the seed's origin is that reflection with
/// the seed reversed along its axes. A seed is read where its pick names an
/// occurrence of the scope's own definition.
fn check_mirror(
    holder: &str,
    mirror: &MirrorComponents,
    occurrences: &BTreeMap<&str, Option<Transform>>,
    within: Within,
    findings: &mut Vec<Finding>,
) {
    let Some(reflection) = mirror.plane.resolved.as_ref().and_then(reflection) else {
        return;
    };
    let Picks::Named(seeds) = &mirror.seeds.picks else {
        return;
    };
    for (seed, result) in seeds.as_slice().iter().zip(&mirror.results) {
        let reversal = match &result.orientation {
            None => Transform::identity(),
            Some(MirrorOrientation::SeedAxes {
                reversed,
                about: Some(MirrorPivot::Origin),
            }) => reversal(*reversed),
            Some(_) => continue,
        };
        let (PickTarget::Component, Some(PickContext::Occurrence(path))) =
            (&seed.target, &seed.context)
        else {
            continue;
        };
        let [step] = path.0.as_slice() else {
            continue;
        };
        let StepRef::Occurrence(seed_occurrence) = &step.occurrence else {
            continue;
        };
        let (Some(Some(seed_transform)), Some(Some(transform))) = (
            occurrences.get(seed_occurrence.as_str()),
            occurrences.get(result.occurrence.as_str()),
        ) else {
            continue;
        };
        let Some(composed) = reflection
            .compose(*seed_transform)
            .and_then(|reflected| reflected.compose(reversal))
            .ok()
        else {
            continue;
        };
        if !within.agree(*transform, composed) {
            warning(
                findings,
                holder,
                crate::rule!("design.transform-mirror-result-not-seed-placement", 
                    "the transform of mirror result {} is not its seed's placement reflected across the plane",
                    result.occurrence.as_str()
                ),
            );
        }
    }
}

/// An ordered container's `placement` is the `Body.transform` of each body of
/// the definition its features build, where no occurrence places that
/// definition and the body states a transform.
fn check_containers(index: &DesignRecords<'_>, within: Within, findings: &mut Vec<Finding>) {
    let ir = index.ir;
    for container in &ir.design.containers {
        let Container::Ordered {
            id,
            members,
            placement: Some(placement),
            ..
        } = container
        else {
            continue;
        };
        let Some(resolved) = placement.resolved.as_ref().and_then(frame_transform) else {
            continue;
        };
        let mut components = members.iter().filter_map(|member| match member {
            Member::Feature(feature) => index.features.get(feature.as_str())?.component.as_ref(),
            Member::Container(_) => None,
        });
        let Some(component) = components.next() else {
            continue;
        };
        if components.any(|other| other != component) {
            continue;
        }
        if ir
            .model
            .occurrences
            .iter()
            .any(|occurrence| &occurrence.prototype == component)
        {
            continue;
        }
        let Some(definition) = ir
            .model
            .product_definitions
            .iter()
            .find(|definition| &definition.id == component)
        else {
            continue;
        };
        for body in &ir.geometry.bodies {
            let Some(transform) = body.transform else {
                continue;
            };
            if definition.bodies.contains(&body.id) && !within.agree(transform, resolved) {
                warning(
                    findings,
                    id.as_str(),
                    crate::rule!(
                        "design.body-does-not-container-placement-transform",
                        "body {} does not have the container's placement as its transform",
                        body.id.as_str()
                    ),
                );
            }
        }
    }
}

fn literal<T: Copy>(dimension: &Dimension<T>) -> Option<T> {
    match dimension {
        Dimension::Literal(value) => Some(*value),
        Dimension::Parameter(_) | Dimension::Negated(_) => None,
    }
}

fn translation(by: Vector3) -> Option<Transform> {
    Transform::affine([
        [1.0, 0.0, 0.0, by.x],
        [0.0, 1.0, 0.0, by.y],
        [0.0, 0.0, 1.0, by.z],
    ])
}

/// A turn by `angle` about `axis`, right-handed about its direction.
fn rotation(axis: &AxisValue, angle: f64) -> Option<Transform> {
    let k = Vector3::from(axis.direction);
    let (sin, cos) = angle.portable_sin_cos();
    let linear = [[0usize, 1, 2], [0, 1, 2], [0, 1, 2]];
    let component = |v: Vector3, i: usize| [v.x, v.y, v.z][i];
    let mut rows = [[0.0; 4]; 3];
    for (row, columns) in linear.iter().enumerate() {
        for &column in columns {
            let identity = if row == column { 1.0 } else { 0.0 };
            let cross = match (row, column) {
                (0, 1) => -k.z,
                (0, 2) => k.y,
                (1, 0) => k.z,
                (1, 2) => -k.x,
                (2, 0) => -k.y,
                (2, 1) => k.x,
                _ => 0.0,
            };
            rows[row][column] = cos * identity
                + sin * cross
                + (1.0 - cos) * component(k, row) * component(k, column);
        }
    }
    let about = Transform::affine(rows)?;
    let origin = axis.origin.get();
    let to = translation(Vector3::new(origin.x, origin.y, origin.z))?;
    let from = translation(Vector3::new(-origin.x, -origin.y, -origin.z))?;
    to.compose(about).ok()?.compose(from).ok()
}

/// The reflection across `plane`.
fn reflection(plane: &PlaneValue) -> Option<Transform> {
    let n = Vector3::from(plane.normal);
    let origin = plane.origin.get();
    let offset = 2.0 * (origin.x * n.x + origin.y * n.y + origin.z * n.z);
    let component = |i: usize| [n.x, n.y, n.z][i];
    let mut rows = [[0.0; 4]; 3];
    for (row, values) in rows.iter_mut().enumerate() {
        for (column, value) in values.iter_mut().take(3).enumerate() {
            let identity = if row == column { 1.0 } else { 0.0 };
            *value = identity - 2.0 * component(row) * component(column);
        }
        values[3] = offset * component(row);
    }
    Transform::affine(rows)
}

/// The reversal of the seed's own axes through its origin.
fn reversal(reversed: MirrorReversal) -> Transform {
    let signs = match reversed {
        MirrorReversal::X => [-1.0, 1.0, 1.0],
        MirrorReversal::Y => [1.0, -1.0, 1.0],
        MirrorReversal::Z => [1.0, 1.0, -1.0],
        MirrorReversal::All => [-1.0, -1.0, -1.0],
    };
    Transform::affine([
        [signs[0], 0.0, 0.0, 0.0],
        [0.0, signs[1], 0.0, 0.0],
        [0.0, 0.0, signs[2], 0.0],
    ])
    .unwrap_or_default()
}

/// A scale with literal factors, applied in the placed definition's own axes.
fn scale_transform(scale: &ScaleFactors) -> Option<Transform> {
    let [x, y, z] = match scale {
        ScaleFactors::Uniform(factor) => {
            let factor = literal(factor)?.get();
            [factor; 3]
        }
        ScaleFactors::PerAxis {
            factors,
            frame: None,
        } => [
            literal(&factors[0])?.get(),
            literal(&factors[1])?.get(),
            literal(&factors[2])?.get(),
        ],
        ScaleFactors::PerAxis { frame: Some(_), .. } => return None,
    };
    Transform::affine([[x, 0.0, 0.0, 0.0], [0.0, y, 0.0, 0.0], [0.0, 0.0, z, 0.0]])
}

#[cfg(test)]
mod tests;
