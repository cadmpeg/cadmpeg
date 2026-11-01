// SPDX-License-Identifier: Apache-2.0
//! Mates decode as assembly joints between B-rep faces and survive a write.

use cadmpeg_ir::products::{AssemblyJoint, JointOperands, PairedJointKind, PlaneSense};

use crate::test_support::{decode, encode};

/// Agreement of a frame component after a write, in millimetres.
const FRAME_TOLERANCE: f64 = 1e-9;

fn fixture() -> String {
    std::fs::read_to_string(format!(
        "{}/tests/golden/fixtures/mates_planar.usda",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture")
}

#[test]
fn a_planar_mate_is_an_opposed_plane_coincidence_on_two_faces() {
    let result = decode(&fixture());
    let model = &result.ir().model;
    let [joint] = model.assembly_joints.as_slice() else {
        panic!("one joint: {:#?}", model.assembly_joints);
    };
    let JointOperands::Pair {
        kind: PairedJointKind::PlaneCoincidence { sense, .. },
        connectors,
        ..
    } = joint.operands()
    else {
        panic!("a plane coincidence");
    };
    assert_eq!(*sense, PlaneSense::Opposed);
    for connector in connectors.iter() {
        let [face] = connector.operand.subelements.as_slice() else {
            panic!("one face");
        };
        assert!(model
            .faces
            .iter()
            .any(|candidate| candidate.id.as_str() == face));
    }
    // The top face of box A: the frame sits at its centroid, z up.
    let rows = connectors[0].frame.affine_rows();
    assert!((rows[2][2] - 1.0).abs() <= FRAME_TOLERANCE);
    assert!((rows[0][3] - 5.0).abs() <= FRAME_TOLERANCE);
    assert!((rows[2][3] - 30.0).abs() <= FRAME_TOLERANCE);
}

#[test]
fn a_mate_survives_a_write_once() {
    let first = decode(&fixture());
    let (text, _) = encode(first.ir());
    assert_eq!(text.matches("def CadPlanarMate").count(), 1, "{text}");
    let second = decode(&text);
    let (before, after) = (
        &first.ir().model.assembly_joints,
        &second.ir().model.assembly_joints,
    );
    assert_eq!(before.len(), after.len());
    for (one, two) in before.iter().zip(after) {
        assert_eq!(one.frame_sense(), two.frame_sense());
        for (left, right) in one.connectors().zip(two.connectors()) {
            for (row_left, row_right) in left
                .frame
                .affine_rows()
                .iter()
                .zip(right.frame.affine_rows())
            {
                for (a, b) in row_left.iter().zip(row_right) {
                    assert!((a - b).abs() <= FRAME_TOLERANCE, "{a} != {b}");
                }
            }
        }
    }
}

/// Agreement of a joint value after a write, per unit of its magnitude.
const VALUE_TOLERANCE: f64 = 1e-12;

/// Whether `left` and `right` agree, each value within `VALUE_TOLERANCE`.
fn same_kind(left: &PairedJointKind, right: &PairedJointKind) -> bool {
    fn close(left: &serde_json::Value, right: &serde_json::Value) -> bool {
        match (left, right) {
            (serde_json::Value::Number(a), serde_json::Value::Number(b)) => {
                let (a, b) = (
                    a.as_f64().unwrap_or(f64::NAN),
                    b.as_f64().unwrap_or(f64::NAN),
                );
                (a - b).abs() <= VALUE_TOLERANCE * a.abs().max(1.0)
            }
            (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| close(a, b))
            }
            (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .all(|(key, a)| b.get(key).is_some_and(|b| close(a, b)))
            }
            (a, b) => a == b,
        }
    }
    let left = serde_json::to_value(left).expect("a kind");
    let right = serde_json::to_value(right).expect("a kind");
    close(&left, &right)
}

#[test]
fn every_joint_family_and_its_values_survive_a_write() {
    let source = decode(&fixture());
    let mut ir = source.ir().clone();
    let base = ir.model.assembly_joints[0].clone();
    let JointOperands::Pair {
        connectors,
        offset_frames,
        ..
    } = base.operands()
    else {
        panic!("a pair");
    };
    let connectors = (**connectors).clone();
    let real = |value: f64| cadmpeg_ir::scalar::FiniteReal::new(value).expect("a real");
    let positive = |value: f64| cadmpeg_ir::scalar::PositiveReal::new(value).expect("positive");
    let limits = |low: Option<f64>, high: Option<f64>| {
        cadmpeg_ir::products::JointLimits::new(low, high).expect("limits")
    };
    let kinds = vec![
        PairedJointKind::Fixed {
            angle: Some(real(0.25)),
            translation_offset: Some([real(1.0), real(-2.0), real(3.0)]),
            angular_limits: Some(limits(Some(-0.5), None)),
            linear_limits: Some(limits(Some(-1.0), Some(4.0))),
        },
        PairedJointKind::Revolute {
            angle: None,
            angular_limits: Some(limits(None, Some(1.5))),
        },
        PairedJointKind::Cylindrical {
            angle: Some(real(0.5)),
            distance: Some(real(12.0)),
            angular_limits: None,
            linear_limits: Some(limits(Some(0.0), Some(30.0))),
        },
        PairedJointKind::PlaneCoincidence {
            sense: PlaneSense::Opposed,
            offset: Some(real(2.5)),
        },
        PairedJointKind::Distance {
            distance: Some(real(7.0)),
        },
        PairedJointKind::Parallel {},
        PairedJointKind::Perpendicular {},
        PairedJointKind::Angle {
            angle: Some(real(0.75)),
        },
        PairedJointKind::AxisCoincidence {},
        PairedJointKind::PointOnAxis {
            distance: Some(real(4.0)),
        },
        PairedJointKind::Insert {
            sense: PlaneSense::Opposed,
            offset: Some(real(-1.5)),
        },
        PairedJointKind::Tangent {
            contact: cadmpeg_ir::products::TangentContact::Inside,
        },
        PairedJointKind::Screw {
            distance: Some(real(2.0)),
        },
        PairedJointKind::Gears {
            distance: Some(real(40.0)),
            distance2: Some(real(20.0)),
            ratio: Some(positive(0.5)),
            coupled_joints: vec![base.id.clone()],
        },
        PairedJointKind::RackPinion {
            distance: None,
            distance2: None,
            ratio: Some(positive(0.02)),
            coupled_joints: Vec::new(),
        },
        PairedJointKind::Belt {
            distance: Some(real(10.0)),
            distance2: Some(real(5.0)),
        },
        PairedJointKind::Native {
            name: "Hinge2".to_owned(),
            angle: Some(real(0.1)),
            translation_offset: None,
            distance: Some(real(3.0)),
            distance2: None,
            angular_limits: None,
            linear_limits: None,
        },
    ];
    for (index, kind) in kinds.into_iter().enumerate() {
        let id = cadmpeg_ir::products::JointId::mint(format!("test:model:joint#{index}"))
            .expect("an id");
        let mut joint = AssemblyJoint::paired(id, kind, connectors.clone(), *offset_frames);
        joint.set_frame_sense(base.frame_sense());
        ir.model.assembly_joints.push(joint);
    }
    let (text, _) = encode(&ir);
    let read = decode(&text);
    let after = &read.ir().model.assembly_joints;
    for expected in &ir.model.assembly_joints {
        let found = after
            .iter()
            .find(|joint| joint.id == expected.id)
            .unwrap_or_else(|| panic!("{}\n{text}", expected.id));
        let (JointOperands::Pair { kind: left, .. }, JointOperands::Pair { kind: right, .. }) =
            (found.operands(), expected.operands())
        else {
            panic!("pairs");
        };
        assert!(same_kind(left, right), "{left:?} != {right:?}");
    }
}
