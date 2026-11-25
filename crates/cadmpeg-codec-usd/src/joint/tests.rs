// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::design::construction::FrameAxis;
use cadmpeg_ir::design::ops::shared::{AxisLimits, JointKind, JointMotion};
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{
    self, AssemblyJoint, JointId, JointLimits, JointOperand, JointOperands, OperandContainer,
    PairedJointKind,
};
use cadmpeg_ir::transform::Transform;

use super::{alignment, assembly, design, turned, unturned, Side};

const AXES: [FrameAxis; 3] = [FrameAxis::X, FrameAxis::Y, FrameAxis::Z];

fn index(axis: products::FrameAxis) -> usize {
    match axis {
        products::FrameAxis::X => 0,
        products::FrameAxis::Y => 1,
        products::FrameAxis::Z => 2,
    }
}

#[test]
fn the_turned_frame_names_each_source_axis_by_the_column_that_holds_it() {
    for axis in AXES {
        let turn = alignment(axis);
        for (source, unit) in AXES.into_iter().zip([0, 1, 2]) {
            let column = index(turned(source, axis));
            let held: Vec<f64> = turn.iter().map(|row| row[column]).collect();
            let mut expected = vec![0.0; 3];
            expected[unit] = 1.0;
            assert_eq!(held, expected, "{axis:?} {source:?}");
            assert_eq!(unturned(turned(source, axis), axis), source);
        }
        assert_eq!(turned(axis, axis), products::FrameAxis::Z);
    }
}

fn frame(rows: [[f64; 4]; 3]) -> Transform {
    Transform::affine(rows).unwrap()
}

#[test]
fn a_revolute_joint_on_x_keeps_its_frames_and_limits_through_its_design_form() {
    let raw = [
        frame([
            [0.0, -1.0, 0.0, 10.0],
            [1.0, 0.0, 0.0, -5.0],
            [0.0, 0.0, 1.0, 2.5],
        ]),
        frame([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 40.0],
            [0.0, 0.0, 1.0, 0.0],
        ]),
    ];
    let turn = Transform::affine(alignment(FrameAxis::X)).unwrap();
    let base = OccurrenceId::mint("test:model:occurrence#base").unwrap();
    let link = OccurrenceId::mint("test:model:occurrence#link").unwrap();
    let id = "test:model:joint#hinge";
    let limits = JointLimits::new(Some(-0.5), Some(1.25));
    let joint = AssemblyJoint::paired(
        JointId::mint(id).unwrap(),
        PairedJointKind::Revolute {
            angle: None,
            angular_limits: limits.clone(),
        },
        [
            products::JointConnector {
                operand: JointOperand::occurrence(base.clone(), String::new(), Vec::new()),
                frame: raw[0].compose(turn).unwrap(),
                detached: false,
            },
            products::JointConnector {
                operand: JointOperand::occurrence(link.clone(), String::new(), Vec::new()),
                frame: raw[1].compose(turn).unwrap(),
                detached: false,
            },
        ],
        None,
    );
    let stated = design(
        &joint,
        FrameAxis::X,
        [
            Side {
                body: Some(vec![base.clone()]),
                frame: raw[0],
            },
            Side {
                body: Some(vec![link.clone()]),
                frame: raw[1],
            },
        ],
        &|_| None,
    )
    .unwrap();
    let JointKind::Motion {
        motion,
        limits: axis_limits,
        ..
    } = &stated.kind
    else {
        panic!("{:?}", stated.kind);
    };
    assert_eq!(*motion, JointMotion::Revolute { axis: FrameAxis::X });
    assert!(matches!(
        axis_limits.as_slice(),
        [AxisLimits::Rotation {
            axis: FrameAxis::X,
            limits: Some(_),
            rest: None
        }]
    ));
    let written = assembly(&FeatureId::mint(id).unwrap(), &stated).unwrap();
    assert_eq!(written.dropped, 0);
    assert_eq!(written.joint.id, joint.id);
    let JointOperands::Pair {
        kind, connectors, ..
    } = written.joint.operands()
    else {
        panic!("{:?}", written.joint.operands());
    };
    assert_eq!(
        *kind,
        PairedJointKind::Revolute {
            angle: None,
            angular_limits: limits,
        }
    );
    let original: Vec<_> = joint.connectors().collect();
    for ((connector, expected), body) in connectors.iter().zip(original).zip([&base, &link]) {
        assert_eq!(connector.frame, expected.frame);
        assert!(matches!(
            &connector.operand.container,
            OperandContainer::Occurrence { occurrence, occurrence_path }
                if occurrence == body && occurrence_path.is_empty()
        ));
    }
}

#[test]
fn a_fixed_joint_to_the_world_is_grounded_at_its_world_frame() {
    let world = frame([
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ]);
    let body = OccurrenceId::mint("test:model:occurrence#base").unwrap();
    let id = "test:model:joint#anchor";
    let joint = AssemblyJoint::grounded(
        JointId::mint(id).unwrap(),
        products::JointConnector {
            operand: JointOperand::occurrence(body.clone(), String::new(), Vec::new()),
            frame: Transform::identity(),
            detached: false,
        },
        Some(world),
    );
    let stated = design(
        &joint,
        FrameAxis::Z,
        [
            Side {
                body: None,
                frame: world,
            },
            Side {
                body: Some(vec![body.clone()]),
                frame: Transform::identity(),
            },
        ],
        &|_| None,
    )
    .unwrap();
    assert!(matches!(
        stated.kind,
        JointKind::Motion {
            motion: JointMotion::Rigid,
            ..
        }
    ));
    let written = assembly(&FeatureId::mint(id).unwrap(), &stated).unwrap();
    assert_eq!(written.joint.operands(), joint.operands());
}
