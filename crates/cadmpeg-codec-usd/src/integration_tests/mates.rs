// SPDX-License-Identifier: Apache-2.0
//! Mates decode as assembly joints between B-rep faces and survive a write.

use cadmpeg_ir::products::{JointOperands, PairedJointKind, PlaneSense};

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
