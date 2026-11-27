// SPDX-License-Identifier: Apache-2.0
//! A cage body is left out with its cage, and the loss that names the cage
//! names the body (§5.6).

use std::io::Cursor;

use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::cages::{assert_cage_body_dropped, push_cage_body};

use crate::IgesCodec;

#[test]
fn a_cage_body_is_left_out_with_its_cage() {
    let mut ir = super::partial::tetrahedron();
    let cage = push_cage_body(&mut ir, "iges");

    let mut bytes = Vec::new();
    let report = IgesCodec
        .plan(EncodeInput::new(&ir, None))
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("the write succeeds");
    assert_cage_body_dropped(&report, &cage, "186_manifold_solid_brep", 1);
    let round_trip = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("written IGES decodes");
    assert_eq!(round_trip.ir().geometry.bodies.len(), 1);
}
