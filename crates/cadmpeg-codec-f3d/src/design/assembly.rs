// SPDX-License-Identifier: Apache-2.0
//! Project exact Design assembly alignments into neutral joints.

use std::collections::BTreeMap;

use cadmpeg_ir::products::{AssemblyJoint, JointKind, JointOperand};

use crate::ids::native_stream;
use crate::records::{DesignComponentOccurrence, DesignParameterScope};

/// Project assembly scopes whose connector frames and occurrence paths are complete.
pub(crate) fn project_assembly_joints(
    scopes: &[DesignParameterScope],
    native_occurrences: &[DesignComponentOccurrence],
) -> Vec<AssemblyJoint> {
    let mut occurrences = BTreeMap::new();
    for occurrence in native_occurrences {
        let Some(stream) = native_stream(&occurrence.id) else {
            continue;
        };
        occurrences
            .entry((stream, occurrence.occurrence_guid.to_ascii_lowercase()))
            .and_modify(|candidate| *candidate = None)
            .or_insert(Some(occurrence));
    }
    let mut joints = BTreeMap::new();
    for scope in scopes {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(alignment) = &scope.assembly_alignment else {
            continue;
        };
        let (Some(frames), Some(paths)) = (&alignment.operand_frames, &alignment.operand_paths)
        else {
            continue;
        };
        let operands = paths
            .iter()
            .map(|path| {
                let root_guid = path.occurrence_guids.first()?;
                let root = occurrences
                    .get(&(stream, root_guid.to_ascii_lowercase()))
                    .copied()
                    .flatten()?;
                Some(JointOperand {
                    component: Some(crate::ids::neutral_component_id(&root.component_guid)),
                    external_document: None,
                    object: Some(root_guid.to_ascii_lowercase()),
                    subelements: path.occurrence_guids[1..]
                        .iter()
                        .map(|guid| guid.to_ascii_lowercase())
                        .collect(),
                })
            })
            .collect::<Option<Vec<_>>>();
        let Some(operands) = operands else {
            continue;
        };
        let id = crate::ids::neutral_assembly_joint_id(scope);
        joints.entry(id.0.clone()).or_insert_with(|| AssemblyJoint {
            id,
            kind: JointKind::Fixed,
            operands,
            frames: frames
                .iter()
                .map(|frame| neutral_transform(frame.transform))
                .collect(),
            offset_frames: Vec::new(),
            suppressed: false,
            detached: [false; 2],
            angle: Some(alignment.angle),
            translation_offset: Some(alignment.offset.map(|value| value * 10.0)),
            distance: None,
            distance2: None,
            angular_limits: None,
            linear_limits: None,
            properties: BTreeMap::new(),
            native_ref: Some(scope.id.clone()),
        });
    }
    joints.into_values().collect()
}

fn neutral_transform(mut transform: [[f64; 4]; 4]) -> [[f64; 4]; 4] {
    for row in &mut transform[..3] {
        row[3] *= 10.0;
    }
    transform
}

#[cfg(test)]
mod tests {
    #[test]
    fn assembly_frame_conversion_scales_only_translation() {
        let transform = [
            [0.0, -1.0, 0.0, 1.25],
            [1.0, 0.0, 0.0, -2.5],
            [0.0, 0.0, 1.0, 3.75],
            [0.0, 0.0, 0.0, 1.0],
        ];
        assert_eq!(
            super::neutral_transform(transform),
            [
                [0.0, -1.0, 0.0, 12.5],
                [1.0, 0.0, 0.0, -25.0],
                [0.0, 0.0, 1.0, 37.5],
                [0.0, 0.0, 0.0, 1.0],
            ]
        );
    }
}
