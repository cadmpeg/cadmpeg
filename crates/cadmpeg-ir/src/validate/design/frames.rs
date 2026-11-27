// SPDX-License-Identifier: Apache-2.0
//! The frames that design values and geometry are read in (§5.2, §5.6).
//!
//! A body's geometry lies in its definition's frame under the body's
//! `Body.transform`. A feature states its values in the frame of the nearest
//! ordered container that holds it. That container's `placement`, resolved
//! in the value's configuration, maps the frame into the definition's; the
//! frame is the definition's where the `placement` is `None`, and where an
//! occurrence places the definition and none of its bodies states a
//! transform (the `App::Link` case of §5.2). A rule that compares a stated
//! value with the geometry reads both in the definition's frame, through
//! rigid maps only.

use crate::design::construction::FrameValue;
use crate::design::container::Container;
use crate::features::geometry::{FinitePoint3, FiniteVector3};
use crate::math::{Point3, Vector3};
use crate::topology::Body;
use crate::transform::Transform;

use super::records::{DesignRecords, MemberKey};

/// A rigid map from one frame into another; `None` is the identity.
pub(super) type Placement = Option<Transform>;

/// The placement a body gives its geometry, where it is rigid.
fn body_placement(body: &Body) -> Option<Placement> {
    match body.transform {
        None => Some(None),
        Some(transform) => transform.is_proper_rigid().then_some(Some(transform)),
    }
}

/// The placement the bodies that hold an entity give it: the one every body
/// gives, where each is rigid and all agree. `Face.shell` is required
/// (§5.6), and a wire edge or a free vertex belongs to exactly one shell
/// (§10). An entity that no body holds, such as an edge that no coedge uses
/// and no shell lists, has no placement.
pub(super) fn held_by<'b>(bodies: impl IntoIterator<Item = &'b Body>) -> Option<Placement> {
    let mut bodies = bodies.into_iter();
    let first = body_placement(bodies.next()?)?;
    bodies
        .all(|body| body_placement(body) == Some(first))
        .then_some(first)
}

/// The map from the frame that `feature` states its values in into its
/// definition's frame (§5.2). `configuration` names the configuration whose
/// value is read; `None` is `resolved`. `None` where the placement holds no
/// value for the configuration, or its value is not rigid.
pub(super) fn values_frame(
    records: &DesignRecords<'_>,
    feature: &str,
    configuration: Option<&str>,
) -> Option<Placement> {
    let (&feature, record) = records.features.get_key_value(feature)?;
    let mut above = records.parents.get(&MemberKey::Feature(feature)).copied();
    let mut placement = None;
    for _ in 0..=records.containers.len() {
        let Some(container) = above else {
            return Some(None);
        };
        match records.containers.get(container).copied()? {
            Container::Ordered {
                placement: held, ..
            } => {
                placement = Some(held.as_deref());
                break;
            }
            Container::Unordered { .. } => {
                above = records
                    .parents
                    .get(&MemberKey::Container(container))
                    .copied();
            }
        }
    }
    let Some(placement) = placement? else {
        return Some(None);
    };
    let definition = match &record.component {
        Some(component) => component.as_str(),
        None => records.scope_of(MemberKey::Feature(feature))?,
    };
    if linked(records, definition) {
        return Some(None);
    }
    let value = match configuration {
        None => placement.resolved.as_ref(),
        Some(configuration) => placement
            .resolved_in
            .iter()
            .find(|(id, _)| id.as_str() == configuration)
            .map(|(_, value)| value),
    }?;
    let transform = frame_transform(value)?;
    transform.is_proper_rigid().then_some(Some(transform))
}

/// Whether an occurrence places `definition` and none of its bodies states
/// a transform: the container's object is a definition of its own, and its
/// `placement` is the occurrence's transform (§5.2).
fn linked(records: &DesignRecords<'_>, definition: &str) -> bool {
    if !records.is_placed(definition) {
        return false;
    }
    let Some(record) = records.definitions.get(definition).copied() else {
        return false;
    };
    !record
        .bodies
        .iter()
        .any(|body| records.has_transformed_body(body.as_str()))
}

/// `placement` read from the frame that `frame` maps into the same target:
/// the inverse of `frame` after `placement`.
pub(super) fn relative(frame: Placement, placement: Placement) -> Option<Placement> {
    let Some(frame) = frame else {
        return Some(placement);
    };
    let inverse = frame.try_inverse_affine().ok()?;
    Some(Some(match placement {
        None => inverse,
        Some(placement) => inverse.compose(placement).ok()?,
    }))
}

/// The affine map a frame value states: its axes as columns, then its
/// origin.
pub(super) fn frame_transform(frame: &FrameValue) -> Option<Transform> {
    let [x, y, z] = [
        Vector3::from(frame.x_axis),
        Vector3::from(frame.y_axis),
        Vector3::from(frame.z_axis),
    ];
    let origin = frame.origin.get();
    Transform::affine([
        [x.x, y.x, z.x, origin.x],
        [x.y, y.y, z.y, origin.y],
        [x.z, y.z, z.z, origin.z],
    ])
}

pub(super) fn point(placement: Placement, point: Point3) -> Option<Point3> {
    match placement {
        None => Some(point),
        Some(transform) => transform.apply_point(point).map(FinitePoint3::get),
    }
}

pub(super) fn vector(placement: Placement, vector: Vector3) -> Option<Vector3> {
    match placement {
        None => Some(vector),
        Some(transform) => transform.apply_vector(vector).map(FiniteVector3::get),
    }
}
