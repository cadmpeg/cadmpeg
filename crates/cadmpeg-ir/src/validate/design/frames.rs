// SPDX-License-Identifier: Apache-2.0
//! The frames that design values and geometry are read in (§5.2, §5.6).
//!
//! A body's geometry lies in its definition's frame under the body's
//! `Body.transform`. A feature states its values in the frame of the ordered
//! containers above it: each container's `placement` maps its frame into its
//! parent's, and the topmost into the definition's. A rule that compares a
//! stated value with the geometry reads both in one frame, through rigid
//! maps only.

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
/// gives, where each is rigid and all agree. Every face, edge, and vertex
/// lies in a shell of a region of a body (§10), so an entity that no body
/// holds has no placement.
pub(super) fn held_by<'b>(bodies: impl IntoIterator<Item = &'b Body>) -> Option<Placement> {
    let mut bodies = bodies.into_iter();
    let first = body_placement(bodies.next()?)?;
    bodies
        .all(|body| body_placement(body) == Some(first))
        .then_some(first)
}

/// The map from the frame `member` states its values in into its
/// definition's frame: the `placement` of each ordered container above it,
/// innermost first (§5.2). `None` where a placement is unresolved or not
/// rigid, and where a placement is stated and an occurrence places the
/// member's scope or the definition the feature builds, so that the
/// placement can be that occurrence's transform.
pub(super) fn values_frame<'a>(
    records: &DesignRecords<'a>,
    member: MemberKey<'a>,
) -> Option<Placement> {
    let mut frame: Placement = None;
    let mut above = records.parents.get(&member).copied();
    for _ in 0..=records.containers.len() {
        let Some(container) = above else {
            if frame.is_some() {
                let scope = records.scope_of(member)?;
                let component = match member {
                    MemberKey::Feature(feature) => records
                        .features
                        .get(feature)
                        .and_then(|feature| feature.component.as_ref())
                        .map(crate::ids::ProductDefinitionId::as_str),
                    MemberKey::Container(_) => None,
                };
                let placed = |definition: &str| {
                    records
                        .ir
                        .model
                        .occurrences
                        .iter()
                        .any(|occurrence| occurrence.prototype.as_str() == definition)
                };
                if placed(scope) || component.is_some_and(placed) {
                    return None;
                }
            }
            return Some(frame);
        };
        if let Some(Container::Ordered {
            placement: Some(placement),
            ..
        }) = records.containers.get(container).copied()
        {
            let outer = frame_transform(placement.resolved.as_ref()?)?;
            if !outer.is_proper_rigid() {
                return None;
            }
            frame = Some(match frame {
                None => outer,
                Some(inner) => outer.compose(inner).ok()?,
            });
        }
        above = records
            .parents
            .get(&MemberKey::Container(container))
            .copied();
    }
    None
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
