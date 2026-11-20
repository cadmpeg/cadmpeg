// SPDX-License-Identifier: Apache-2.0
//! Selected faces of a `MoveFace` or `OffsetFaces` feature, from the transitions
//! that move them and from the bounded-face recipes of the group members.

use super::output_chain::OutputChain;
use super::surface_rows::{Carrier, StateRows};
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology, AsmHistory};
use crate::records::recipes::ConstructionRecipeKind;
use crate::records::topology::face::DesignFaceOperand;
use cadmpeg_ir::features::{FaceMotion, FaceSelection};
use cadmpeg_ir::math::Vector3;
use std::collections::{BTreeSet, HashSet};

/// Millimetres between a stored surface position and the position the motion
/// states.
const FACE_MOTION_POSITION_TOLERANCE: f64 = 1.0e-6;
/// Distance between two unit directions that a motion leaves unchanged.
const FACE_MOTION_DIRECTION_TOLERANCE: f64 = 1.0e-9;

fn same_direction(left: Vector3, right: Vector3) -> Option<bool> {
    Some((left.unit()? - right.unit()?).norm() <= FACE_MOTION_DIRECTION_TOLERANCE)
}

/// Whether a surface row follows the motion. A row carries the rigid copy of
/// its surface, so a translation moves the origin of a plane whose translation
/// lies in the plane.
fn row_follows(before: Carrier<'_>, after: Carrier<'_>, motion: &FaceMotion) -> bool {
    let close = |value: f64| value.abs() <= FACE_MOTION_POSITION_TOLERANCE;
    match (motion, before, after) {
        (
            FaceMotion::Translate {
                direction,
                distance,
            },
            Carrier::Plane(before),
            Carrier::Plane(after),
        ) => {
            let shift = after.origin.vector_from(before.origin);
            same_direction(before.normal, after.normal) == Some(true)
                && (shift - direction.scale(distance.get())).norm()
                    <= FACE_MOTION_POSITION_TOLERANCE
        }
        (FaceMotion::Transform { transform }, Carrier::Plane(before), Carrier::Plane(after)) => {
            let (Some(origin), Some(normal)) = (
                transform.apply_point(before.origin),
                transform.apply_normal(before.normal),
            ) else {
                return false;
            };
            (after.origin.vector_from(before.origin).norm() > FACE_MOTION_POSITION_TOLERANCE
                || same_direction(before.normal, after.normal) == Some(false))
                && origin.get().vector_from(after.origin).norm() <= FACE_MOTION_POSITION_TOLERANCE
                && same_direction(*normal.as_raw(), after.normal) == Some(true)
        }
        (
            FaceMotion::Transform { transform },
            Carrier::Cylinder(before),
            Carrier::Cylinder(after),
        ) => {
            let (Some(origin), Some(axis)) = (
                transform.apply_point(before.origin),
                transform.apply_vector(before.axis),
            ) else {
                return false;
            };
            (after.origin.vector_from(before.origin).norm() > FACE_MOTION_POSITION_TOLERANCE
                || same_direction(before.axis, after.axis) == Some(false))
                && origin.get().vector_from(after.origin).norm() <= FACE_MOTION_POSITION_TOLERANCE
                && same_direction(axis.get(), after.axis) == Some(true)
                && close(after.radius - before.radius)
        }
        (
            FaceMotion::Translate {
                direction,
                distance,
            },
            Carrier::Cylinder(before),
            Carrier::Cylinder(after),
        ) => {
            let shift = after.origin.vector_from(before.origin);
            same_direction(before.axis, after.axis) == Some(true)
                && close(after.radius - before.radius)
                && (shift - direction.scale(distance.get())).norm()
                    <= FACE_MOTION_POSITION_TOLERANCE
        }
        (FaceMotion::Offset { distance }, Carrier::Plane(before), Carrier::Plane(after)) => {
            let Some(normal) = before.normal.unit() else {
                return false;
            };
            same_direction(before.normal, after.normal) == Some(true)
                && close(
                    after.origin.vector_from(before.origin).dot(normal).abs()
                        - distance.get().abs(),
                )
        }
        (FaceMotion::Offset { distance }, Carrier::Cylinder(before), Carrier::Cylinder(after)) => {
            let Some(axis) = before.axis.unit() else {
                return false;
            };
            same_direction(before.axis, after.axis) == Some(true)
                && after.origin.vector_from(before.origin).cross(axis).norm()
                    <= FACE_MOTION_POSITION_TOLERANCE
                && close((after.radius - before.radius).abs() - distance.get().abs())
        }
        _ => false,
    }
}

/// Faces of the input state that the motion moves, sorted: the updated faces
/// whose plane or cylinder row follows the motion. `None` when an updated face
/// has no plane or cylinder row in either state, the motion is a rotation, or
/// its distance is zero. The list is empty when no updated row follows the
/// motion.
fn moved_faces(
    motion: &FaceMotion,
    updated: &BTreeSet<i64>,
    input: &AsmHistoricalTopology,
    output: &AsmHistoricalTopology,
) -> Option<Vec<i64>> {
    match motion {
        FaceMotion::Offset { distance } | FaceMotion::Translate { distance, .. } => {
            if distance.get().abs() <= FACE_MOTION_POSITION_TOLERANCE {
                return None;
            }
        }
        FaceMotion::Rotate { .. } => return None,
        FaceMotion::Transform { .. } => {}
    }
    let input_rows = StateRows::new(input);
    let output_rows = StateRows::new(output);
    let mut moved = Vec::new();
    for &face in updated {
        if row_follows(
            input_rows.carrier(face)?,
            output_rows.carrier(face)?,
            motion,
        ) {
            moved.push(face);
        }
    }
    Some(moved)
}

/// Faces of the input state that a `MoveFace` or `OffsetFaces` feature selects,
/// sorted.
///
/// The motion moves every selected face, and a face tangent to a selected face
/// can move with it. When no face is deleted and every updated face has a plane
/// or cylinder row, the moved faces therefore hold the whole selection, so a
/// moved set with one face per distinct member is the selection.
fn moved_face_selection(
    members: &[u32],
    motion: &FaceMotion,
    updated: &BTreeSet<i64>,
    input: &AsmHistoricalTopology,
    output: &AsmHistoricalTopology,
) -> Option<Vec<i64>> {
    let moved = moved_faces(motion, updated, input, output)?;
    let distinct_members = members.iter().collect::<HashSet<_>>().len();
    (!moved.is_empty() && moved.len() == distinct_members).then_some(moved)
}

/// Faces of the input state that a one-member group selects when the motion
/// moves several faces of one surface, sorted.
///
/// The faces of one surface move together. The moved set is the selection
/// when every moved face lies on one input surface and one recipe reference of
/// the member names exactly the moved faces.
fn shared_surface_selection(
    members: &[u32],
    moved: &[i64],
    operands: &[&DesignFaceOperand],
    input: &AsmHistoricalTopology,
) -> Option<Vec<i64>> {
    let [member, rest @ ..] = members else {
        return None;
    };
    if rest.iter().any(|other| other != member) || moved.len() < 2 {
        return None;
    }
    let mut matching = operands
        .iter()
        .filter(|operand| operand.record_index() == *member);
    let (Some(operand), None) = (matching.next(), matching.next()) else {
        return None;
    };
    let rows = StateRows::new(input);
    let surfaces = moved
        .iter()
        .map(|face| rows.surface(*face))
        .collect::<Option<BTreeSet<_>>>()?;
    if surfaces.len() != 1 {
        return None;
    }
    let moved_set: BTreeSet<i64> = moved.iter().copied().collect();
    operand
        .recipe_references
        .iter()
        .filter(|reference| reference.design_reference >= 0)
        .any(|reference| {
            super::effective_faces(reference)
                .iter()
                .map(|face| super::stable_ref(face.as_str()))
                .collect::<Option<BTreeSet<_>>>()
                .is_some_and(|named| named == moved_set)
        })
        .then(|| moved.to_vec())
}

/// Faces that the transitions from `previous_state_id` to `state` update, or
/// `None` when one of them deletes a face.
fn updated_faces(
    histories: &[AsmHistory],
    history: &AsmHistory,
    state: &AsmDeltaState,
    previous_state_id: i64,
) -> Option<BTreeSet<i64>> {
    let chain = OutputChain::resolve(
        histories,
        state.state_id,
        previous_state_id,
        Some(&history.id),
    )?;
    let mut updated = BTreeSet::new();
    for state_id in &chain.states {
        let mut matching = history
            .states()
            .iter()
            .filter(|member| member.state_id == *state_id);
        let (Some(member), None) = (matching.next(), matching.next()) else {
            return None;
        };
        let faces = &member.transition.as_ref()?.topology.faces;
        if !faces.deleted.is_empty() {
            return None;
        }
        updated.extend(faces.updated.iter().copied());
    }
    Some(updated)
}

/// Faces of the input state that the bounded-face recipes of a `MoveFace` or
/// `OffsetFaces` group select, sorted: one distinct face per distinct member.
fn recipe_face_selection(
    members: &[u32],
    operands: &[&DesignFaceOperand],
    input: &AsmHistoricalTopology,
) -> Option<Vec<i64>> {
    let mut seen = HashSet::new();
    let mut faces = BTreeSet::new();
    for member in members {
        if !seen.insert(member) {
            continue;
        }
        let mut matching = operands
            .iter()
            .filter(|operand| operand.record_index() == *member);
        let (Some(operand), None) = (matching.next(), matching.next()) else {
            return None;
        };
        if operand.recipe_kind != ConstructionRecipeKind::BoundedFace {
            return None;
        }
        let [face] = operand.resolved_face_slots.as_slice() else {
            return None;
        };
        if !input.faces.contains(face) || !faces.insert(*face) {
            return None;
        }
    }
    (!faces.is_empty()).then(|| faces.into_iter().collect())
}

/// The selection of a group from its direct transition reading, its member
/// recipes, and the shared-surface reading. The shared-surface reading is
/// taken only when neither of the others states a selection.
fn group_selection(
    direct: Option<Vec<i64>>,
    recipes: Option<Vec<i64>>,
    shared: impl FnOnce() -> Option<Vec<i64>>,
) -> Option<Vec<i64>> {
    let moved = match (direct, &recipes) {
        (Some(direct), _) => Some(direct),
        (None, None) => shared(),
        (None, Some(_)) => None,
    };
    agreed_selection(moved, recipes)
}

/// The selection that the transitions and the member recipes state. Two
/// different selections state none.
fn agreed_selection(moved: Option<Vec<i64>>, recipes: Option<Vec<i64>>) -> Option<Vec<i64>> {
    match (moved, recipes) {
        (Some(moved), Some(recipes)) => (moved == recipes).then_some(moved),
        (moved, recipes) => moved.or(recipes),
    }
}

/// The states that one `MoveFace` or `OffsetFaces` feature spans.
pub(super) struct MoveFaceStates<'a> {
    pub(super) histories: &'a [AsmHistory],
    pub(super) history: &'a AsmHistory,
    pub(super) state: &'a AsmDeltaState,
    pub(super) previous: &'a AsmDeltaState,
}

/// Bind a selection that still names its operand group to the faces that its
/// transitions move or its member recipes select. When both state a selection,
/// they agree or the group stays native.
pub(super) fn bind_move_face_selection(
    selection: &mut FaceSelection,
    motion: &FaceMotion,
    feature_id: &cadmpeg_ir::features::FeatureId,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[DesignFaceOperand],
    states: &MoveFaceStates<'_>,
) {
    let MoveFaceStates {
        histories,
        history,
        state,
        previous,
    } = *states;
    let FaceSelection::Native(native) = selection else {
        return;
    };
    let mut matching = groups.iter().filter(|group| group.id == *native);
    let (Some(group), None) = (matching.next(), matching.next()) else {
        return;
    };
    if group.scope_record_index != scope.record_index {
        return;
    }
    let members: Vec<u32> = group.members().iter().map(|member| member.value).collect();
    let Some(input) = history.topology(previous) else {
        return;
    };
    let stream = crate::ids::native_stream(&scope.id);
    let group_operands: Vec<_> = operands
        .iter()
        .filter(|operand| {
            crate::ids::native_stream(&operand.id) == stream
                && operand.scope_record_index == scope.record_index
                && operand.group_record_index() == Some(group.record_index)
        })
        .collect();
    let recipes = recipe_face_selection(&members, &group_operands, input);
    let output = history.topology(state);
    let updated = output.and_then(|_| updated_faces(histories, history, state, previous.state_id));
    let direct = output.zip(updated.as_ref()).and_then(|(output, updated)| {
        moved_face_selection(&members, motion, updated, input, output)
    });
    let Some(faces) = group_selection(direct, recipes, || {
        let (output, updated) = output.zip(updated.as_ref())?;
        let moved = moved_faces(motion, updated, input, output)?;
        shared_surface_selection(&members, &moved, &group_operands, input)
    }) else {
        return;
    };
    let prefix = crate::ids::feature_input_prefix(feature_id, previous.state_id);
    let selected = faces
        .into_iter()
        .map(|slot| {
            crate::ids::history_input_face_id(&prefix, cadmpeg_ir::ids::IdentityKey::from(slot))
        })
        .collect();
    if let Ok(historical) = FaceSelection::historical(
        crate::ids::history_input_state_id(&prefix),
        selected,
        native.clone(),
    ) {
        *selection = historical;
    }
}

#[cfg(test)]
mod tests;
