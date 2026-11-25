// SPDX-License-Identifier: Apache-2.0
//! A cage body is left out with its cage, and the loss that names the cage
//! names the body (§5.6).

use std::io::Cursor;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::cages::{assert_cage_body_dropped, push_cage_body};

use super::planar_tetrahedron;
use crate::{RhinoArchiveVersion, RhinoCodec};

#[test]
fn a_cage_body_is_left_out_with_its_cage() {
    let mut ir = planar_tetrahedron();
    let cage = push_cage_body(&mut ir, "rhino");

    let mut bytes = Vec::new();
    let report = RhinoCodec
        .plan(
            EncodeInput::new(&ir, None).with_target(TargetRequest::Explicit(
                RhinoArchiveVersion::V7.descriptor().id.as_str(),
            )),
        )
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("the write succeeds");
    assert_cage_body_dropped(&report, &cage, "bodies", 1);
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the written archive decodes");
    assert_eq!(decoded.ir().geometry.bodies.len(), 1);
}
