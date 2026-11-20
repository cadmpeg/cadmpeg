// SPDX-License-Identifier: Apache-2.0
//! Removed faces of a `Shell` feature, from the faces that its transition walls.

use super::surface_rows::{Carrier, StateRows};
use crate::history_records::{AsmHistoricalTopology, AsmHistoricalTransition};
use cadmpeg_ir::features::{FaceSelection, ShellMode};
use cadmpeg_ir::math::Vector3;
use std::collections::{BTreeSet, HashSet};

/// Millimetres between a wall offset and the offset that the thickness states.
const SHELL_WALL_POSITION_TOLERANCE: f64 = 1.0e-6;
/// Cross product of two unit directions that a wall keeps parallel.
const SHELL_WALL_DIRECTION_TOLERANCE: f64 = 1.0e-9;

fn parallel(left: Vector3, right: Vector3) -> bool {
    match (left.unit(), right.unit()) {
        (Some(left), Some(right)) => left.cross(right).norm() <= SHELL_WALL_DIRECTION_TOLERANCE,
        _ => false,
    }
}

/// Whether `wall` is the wall of `face` at `offset`: a parallel plane at that
/// distance along the normal, or a cylinder on the same axis line whose radius
/// differs by that distance.
fn walls(face: Carrier<'_>, wall: Carrier<'_>, offset: f64) -> bool {
    let close = |value: f64| value.abs() <= SHELL_WALL_POSITION_TOLERANCE;
    match (face, wall) {
        (Carrier::Plane(face), Carrier::Plane(wall)) => {
            let Some(normal) = face.normal.unit() else {
                return false;
            };
            parallel(face.normal, wall.normal)
                && close(wall.origin.vector_from(face.origin).dot(normal).abs() - offset)
        }
        (Carrier::Cylinder(face), Carrier::Cylinder(wall)) => {
            let Some(axis) = face.axis.unit() else {
                return false;
            };
            parallel(face.axis, wall.axis)
                && wall.origin.vector_from(face.origin).cross(axis).norm()
                    <= SHELL_WALL_POSITION_TOLERANCE
                && close((wall.radius - face.radius).abs() - offset)
        }
        _ => false,
    }
}

/// Faces of the input state that a `Shell` feature removes, sorted.
///
/// The shell walls every face that it keeps with a face at the wall offset,
/// and it opens every removed face, so the transition updates or deletes it.
/// When every face that the transition updates or deletes has a plane or
/// cylinder row, the changed faces without a wall therefore hold the removed
/// faces, so a set with one face per distinct member is the selection.
fn removed_face_selection(
    members: &[u32],
    offset: f64,
    transition: &AsmHistoricalTransition,
    input: &AsmHistoricalTopology,
    output: &AsmHistoricalTopology,
) -> Option<Vec<i64>> {
    if offset <= SHELL_WALL_POSITION_TOLERANCE {
        return None;
    }
    let input_rows = StateRows::new(input);
    let output_rows = StateRows::new(output);
    let mut output_carriers = Vec::new();
    for &face in &output.faces {
        if let Some(carrier) = output_rows.carrier(face) {
            output_carriers.push(carrier);
        }
    }
    let changed = transition
        .topology
        .faces
        .updated
        .iter()
        .chain(&transition.topology.faces.deleted)
        .copied()
        .collect::<BTreeSet<_>>();
    let mut removed = Vec::new();
    for face in changed {
        let carrier = input_rows.carrier(face)?;
        if !output_carriers
            .iter()
            .any(|wall| walls(carrier, *wall, offset))
        {
            removed.push(face);
        }
    }
    let distinct_members = members.iter().collect::<HashSet<_>>().len();
    (!removed.is_empty() && removed.len() == distinct_members).then_some(removed)
}

/// The wall offset that one side of a `Shell` adds to each kept face.
fn wall_offset(thickness: f64, mode: Option<ShellMode>) -> Option<f64> {
    match mode {
        None => Some(thickness),
        Some(ShellMode::BothSides) => Some(thickness / 2.0),
        Some(ShellMode::Skin | ShellMode::Pipe) => None,
    }
}

/// The wall and the states of one `Shell` feature.
pub(super) struct ShellWalls<'a> {
    pub(super) thickness: f64,
    pub(super) mode: Option<ShellMode>,
    pub(super) previous_state_id: i64,
    pub(super) transition: &'a AsmHistoricalTransition,
    pub(super) input: Option<&'a AsmHistoricalTopology>,
    pub(super) output: Option<&'a AsmHistoricalTopology>,
}

/// Bind a selection that still names its removed-face group to the faces that
/// its transition opens.
pub(super) fn bind_shell_removed_faces(
    selection: &mut FaceSelection,
    feature_id: &cadmpeg_ir::features::FeatureId,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    walls: &ShellWalls<'_>,
) -> Option<cadmpeg_ir::report::loss::LossNote> {
    let native = match selection {
        FaceSelection::Native(native) => native.as_str(),
        FaceSelection::Historical { native, .. } => native.as_str(),
        _ => return None,
    };
    let mut matching = groups.iter().filter(|group| group.id == native);
    let (Some(group), None) = (matching.next(), matching.next()) else {
        return None;
    };
    if group.scope_record_index != scope.record_index
        || crate::ids::native_stream(&group.id) != crate::ids::native_stream(&scope.id)
    {
        return None;
    }
    let (Some(input), Some(output)) = (walls.input, walls.output) else {
        return None;
    };
    let offset = wall_offset(walls.thickness, walls.mode)?;
    let members: Vec<u32> = group.members().iter().map(|member| member.value).collect();
    let faces = removed_face_selection(&members, offset, walls.transition, input, output)?;
    let prefix = crate::ids::feature_input_prefix(feature_id, walls.previous_state_id);
    let selected: Vec<_> = faces
        .into_iter()
        .map(|slot| {
            crate::ids::history_input_face_id(&prefix, cadmpeg_ir::ids::IdentityKey::from(slot))
        })
        .collect();
    if let FaceSelection::Historical { faces, .. } = selection {
        let recipe_set = faces.iter().collect::<BTreeSet<_>>();
        let wall_set = selected.iter().collect::<BTreeSet<_>>();
        return (recipe_set != wall_set).then(|| {
            crate::loss::F3dLossCode::FeatureShellRemovedFacesAssumed.note(format!(
                "Shell {} has recipe removed faces {faces:?} and wall-derived removed faces {selected:?}; the recipe faces are retained (DR-223).",
                scope.id
            ))
        });
    }
    let FaceSelection::Native(native) = selection else {
        return None;
    };
    if let Ok(historical) = FaceSelection::historical(
        crate::ids::history_input_state_id(&prefix),
        selected,
        native.clone(),
    ) {
        *selection = historical;
    }
    None
}

#[cfg(test)]
mod tests;
