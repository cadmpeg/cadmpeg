// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::records::feature::motion_link::DesignLinkedRotations;
use crate::records::feature::scope::{DesignFeatureKind, DesignScopePayloadMut};
use crate::records::identity::Located;
use cadmpeg_ir::products::{JointConnector, JointOperand};
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::transform::Transform;

#[test]
fn motion_link_binds_the_projected_revolute_joints_in_record_order() {
    let [first, second] = [
        DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#100",
            DesignFeatureKind::AsBuilt,
            24,
        ),
        DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#500",
            DesignFeatureKind::AsBuilt,
            31,
        ),
    ];
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1200",
        DesignFeatureKind::MotionLink,
        36,
    );
    let DesignScopePayloadMut::MotionLink(slot) = scope.payload_mut() else {
        panic!("MotionLink scope");
    };
    *slot = Some(DesignLinkedRotations {
        joints: [30, 23],
        angles: [0.5, 1.25].map(|value| PositiveReal::new(value).unwrap()),
    });
    let [first_id, second_id, coupling_id] =
        [&first, &second, &scope].map(neutral_assembly_joint_id);
    let hinge = |id: cadmpeg_ir::products::JointId, name| {
        AssemblyJoint::paired(
            id,
            PairedJointKind::Revolute {
                angle: None,
                angular_limits: None,
            },
            [name, "support"].map(|object| JointConnector {
                operand: JointOperand::root(object, Vec::new()),
                frame: Transform::identity(),
                detached: false,
            }),
            None,
        )
    };
    let mut joints = BTreeMap::from([
        (
            first_id.as_str().to_owned(),
            hinge(first_id.clone(), "first gear"),
        ),
        (
            second_id.as_str().to_owned(),
            hinge(second_id.clone(), "second gear"),
        ),
    ]);
    let motions = [23, 30].map(|index| DesignJointMotion {
        id: format!("f3d:Design/BulkStream.dat:design-joint-motion#{index}"),
        joint_record_index: index,
        joint_byte_offset: 0,
        motion_record_index: index + 100,
        motion_byte_offset: 0,
        freedoms: vec![Located {
            value: ROTATE_Z,
            offset: 0,
        }],
        entry_words: Vec::new(),
        limits: None,
    });
    let scopes = [first, second, scope];
    assert_eq!(project(&scopes, &motions[..1], &mut joints), (0, 1));
    assert_eq!(project(&scopes, &motions, &mut joints), (1, 0));
    let coupling = &joints[coupling_id.as_str()];
    let JointOperands::Pair {
        kind:
            PairedJointKind::Gears {
                ratio: Some(ratio),
                coupled_joints,
                ..
            },
        connectors,
        ..
    } = coupling.operands()
    else {
        panic!("gear coupling");
    };
    assert_eq!(coupled_joints, &[second_id.clone(), first_id.clone()]);
    assert!((ratio.get() - 2.5).abs() < f64::EPSILON);
    assert_eq!(
        connectors.each_ref(),
        [
            joints[second_id.as_str()].connectors().next().unwrap(),
            joints[first_id.as_str()].connectors().next().unwrap(),
        ]
    );
    assert_eq!(coupling.native_ref.as_deref(), Some(scopes[2].id.as_str()));
}
