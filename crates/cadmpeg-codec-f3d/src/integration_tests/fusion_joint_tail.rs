// SPDX-License-Identifier: Apache-2.0
//! A CC0 Fusion save whose `Assemble` scope stores associated tail references
//! and a closing self-reference.

use std::io::Cursor;

use cadmpeg_ir::{Codec, DecodeOptions};

use crate::F3dCodec;

#[test]
fn an_assemble_scope_with_associated_tail_references_projects_its_joint() {
    let bytes = include_bytes!("../../../../corpus/f3d/joint_revolute_limits.f3d");
    let result = F3dCodec
        .decode(&mut Cursor::new(&bytes[..]), &DecodeOptions::default())
        .expect("decode the corpus file");
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    let ir = cadmpeg_test_support::wire::in_memory_json(result.ir());
    let joints = ir["model"]["assembly_joints"]
        .as_array()
        .expect("assembly joints");
    assert_eq!(joints.len(), 1, "{joints:#?}");
    let connectors = joints[0]["operands"]["value"]["connectors"]
        .as_array()
        .expect("joint connectors");
    assert_eq!(connectors.len(), 2, "{connectors:#?}");

    // The associated references name the scope and its second connector's
    // construction.
    let native = crate::test_support::native_test::f3d_native(result.ir());
    let mut joints = native.design_parameter_scopes.iter().filter_map(|scope| {
        scope.assembly_alignment().and_then(|alignment| {
            alignment
                .tail_links
                .as_ref()
                .map(|run| (scope, alignment, run))
        })
    });
    let (scope, alignment, links) = joints.next().expect("a scope with associated references");
    assert!(joints.next().is_none());
    let operand_frames = alignment.operand_frames().expect("operand frames");
    let targets = links
        .references
        .iter()
        .map(|reference| reference.value)
        .collect::<Vec<_>>();
    assert_eq!(
        targets,
        [scope.record_index, operand_frames[1].reference_record_index]
    );

    // The version-6 joint record states its motion in one lane: a revolute
    // joint with limits -30 to 60 degrees.
    assert!(
        result
            .report()
            .losses
            .iter()
            .all(|loss| loss.code != crate::loss::F3dLossCode::AssemblyJointMotionUndecoded.kind()),
        "{:#?}",
        result.report().losses
    );
    let wire = cadmpeg_test_support::wire::in_memory_json(result.ir());
    let kind = &wire["model"]["assembly_joints"][0]["operands"]["value"]["kind"];
    assert_eq!(kind["kind"], "revolute", "{kind:#}");
    let limits = &kind["angular_limits"];
    assert!(
        (limits["minimum"].as_f64().unwrap() + 30f64.to_radians()).abs() < 1e-12,
        "{kind:#}"
    );
    assert!(
        (limits["maximum"].as_f64().unwrap() - 60f64.to_radians()).abs() < 1e-12,
        "{kind:#}"
    );
}
