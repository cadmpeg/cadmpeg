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
}
