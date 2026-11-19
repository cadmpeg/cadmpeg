// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::products::{AssemblyJoint, JointOperands, PairedJointKind};

use crate::design::components::paths::LocalOccurrencePaths;
use crate::ids::{native_stream, neutral_assembly_joint_id};
use crate::records::feature::joint_motion::DesignJointMotion;
use crate::records::feature::motion_link::ROTATE_Z;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};

pub(super) fn project(
    scopes: &[DesignParameterScope],
    motions: &[DesignJointMotion],
    paths: &LocalOccurrencePaths,
    joints: &mut BTreeMap<String, AssemblyJoint>,
) -> (usize, usize) {
    let links = scopes.iter().filter_map(|scope| match scope.payload() {
        DesignScopePayload::MotionLink(Some(link)) => Some((scope, link)),
        _ => None,
    });
    let mut links = links.peekable();
    if links.peek().is_none() {
        return (0, 0);
    }

    let mut rotates_about_z = HashMap::new();
    for motion in motions {
        if let Some(stream) = native_stream(&motion.id) {
            rotates_about_z
                .entry((stream, motion.joint_record_index))
                .and_modify(|valid| *valid = false)
                .or_insert(matches!(motion.freedoms.as_slice(), [axis] if axis.value == ROTATE_Z));
        }
    }
    let mut joint_scopes = HashMap::new();
    for scope in scopes {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(joint_index) = scope.record_index.checked_sub(1) else {
            continue;
        };
        if rotates_about_z.get(&(stream, joint_index)) == Some(&true) {
            joint_scopes
                .entry((stream, joint_index))
                .and_modify(|stored| *stored = None)
                .or_insert(Some(scope));
        }
    }

    let mut projected = 0;
    let mut refused = 0;
    for (scope, link) in links {
        let coupling = || {
            let stream = native_stream(&scope.id)?;
            let hinge = |index| {
                let source = (*joint_scopes.get(&(stream, index))?)?;
                let joint = joints.get(neutral_assembly_joint_id(source).as_str())?;
                let JointOperands::Pair {
                    kind: PairedJointKind::Revolute { .. },
                    connectors,
                    ..
                } = joint.operands()
                else {
                    return None;
                };
                if connectors.iter().any(|connector| {
                    !paths.has_projected_joint_operand(&connector.operand.container)
                }) {
                    return None;
                }
                Some((joint.id.clone(), connectors[0].clone()))
            };
            let [first, second] = link.joints;
            let (first_id, first_connector) = hinge(first)?;
            let (second_id, second_connector) = hinge(second)?;
            let id = neutral_assembly_joint_id(scope);
            if first_id == second_id || joints.contains_key(id.as_str()) {
                return None;
            }
            let mut joint = AssemblyJoint::paired(
                id,
                PairedJointKind::Gears {
                    distance: None,
                    distance2: None,
                    ratio: Some(link.ratio()?),
                    coupled_joints: vec![first_id, second_id],
                },
                [first_connector, second_connector],
                None,
            );
            joint.native_ref = Some(scope.id.clone());
            Some(joint)
        };
        if let Some(joint) = coupling() {
            joints.insert(joint.id.as_str().to_owned(), joint);
            projected += 1;
        } else {
            refused += 1;
        }
    }
    (projected, refused)
}

#[cfg(test)]
mod tests;
