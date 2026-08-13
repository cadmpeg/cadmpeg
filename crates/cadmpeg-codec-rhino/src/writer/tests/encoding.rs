// SPDX-License-Identifier: Apache-2.0
#![allow(unused_imports)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, Encoder};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Color, Point};
use cadmpeg_ir::units::Units;
use sha2::{Digest, Sha256};

use super::*;
use crate::{RhinoArchiveVersion, RhinoCodec, RhinoEncoder};

#[test]
fn empty_utf16_string_has_zero_count_and_no_terminator() {
    assert_eq!(utf16(""), 0_u32.to_le_bytes());
    assert_eq!(utf16("A"), [2, 0, 0, 0, b'A', 0, 0, 0]);
}

#[test]
fn brep_trim_type_distinguishes_boundary_mated_and_seam_uses() {
    assert_eq!(brep_trim_type(1, false), 1);
    assert_eq!(brep_trim_type(2, false), 2);
    assert_eq!(brep_trim_type(2, true), 3);
}

#[test]
fn explicit_loop_role_overrides_face_list_order() {
    use cadmpeg_ir::topology::LoopBoundaryRole;

    assert_eq!(brep_loop_type(LoopBoundaryRole::Inner, true), 2);
    assert_eq!(brep_loop_type(LoopBoundaryRole::Outer, false), 1);
    assert_eq!(brep_loop_type(LoopBoundaryRole::Unspecified, true), 1);
}

#[test]
fn object_attribute_items_are_written_in_ascending_order() {
    let payload = object_attributes_payload(
        "body",
        None,
        Some(Color {
            r: 1.0,
            g: 0.5,
            b: 0.0,
            a: 1.0,
        }),
        Some(false),
    );
    assert_eq!(&payload[21..], &[6, 255, 128, 0, 0, 11, 0, 13, 1, 0]);
}
