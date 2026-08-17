// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::build_prt;
use crate::CreoCodec;

#[test]
fn paired_envelope_spheres_do_not_join_rows_from_neighboring_surface_frames() {
    let lower = [
        0x18, 0x18, 0x01, 0x11, 0x2e, 0xb0, 0x12, 0x47, 0x05, 0x33, 0x2d, 0x2d, 0xff, 0xff, 0xff,
        0xff, 0xff, 0x29, 0x47, 0x05, 0x33, 0x2e, 0x05, 0x33, 0x2d, 0x31, 0xa6, 0x66, 0x66, 0x66,
        0x66, 0x66, 0x18,
    ];
    let upper = [
        0x18, 0x18, 0x01, 0x11, 0x2e, 0xb8, 0x12, 0x47, 0x05, 0x33, 0x2d, 0x28, 0xb3, 0x33, 0x33,
        0x33, 0x33, 0x33, 0x47, 0x05, 0x33, 0x2e, 0x05, 0x33, 0x2d, 0x2e, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xd7, 0x18,
    ];
    let prototype = b"srf_prim_ptr(torus)\0\xe0\x01radius1\0\x18\xe0\x01radius2\0\x2e\x05\x33\xe3";
    let mut payload = Vec::new();
    payload.extend_from_slice(b"srf_array\0\xf8\x01");
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&lower);
    payload.push(0xe3);
    payload.extend_from_slice(prototype);
    payload.extend_from_slice(b"srf_array\0\xf8\x01");
    payload.extend_from_slice(&[8, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&upper);
    payload.push(0xe3);
    payload.extend_from_slice(prototype);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt(
                "neighboring-frames",
                &[("ND:0:VisibGeom:0", payload)],
            )),
            &DecodeOptions::default(),
        )
        .expect("decode neighboring surface frames");

    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_PAIRED_ENVELOPE_SPHERE_COUNT),
        0
    );
}

#[test]
fn paired_envelope_spheres_do_not_join_rows_from_two_prototypes_in_one_frame() {
    let lower = [
        0x18, 0x18, 0x01, 0x11, 0x2e, 0xb0, 0x12, 0x47, 0x05, 0x33, 0x2d, 0x2d, 0xff, 0xff, 0xff,
        0xff, 0xff, 0x29, 0x47, 0x05, 0x33, 0x2e, 0x05, 0x33, 0x2d, 0x31, 0xa6, 0x66, 0x66, 0x66,
        0x66, 0x66, 0x18,
    ];
    let upper = [
        0x18, 0x18, 0x01, 0x11, 0x2e, 0xb8, 0x12, 0x47, 0x05, 0x33, 0x2d, 0x28, 0xb3, 0x33, 0x33,
        0x33, 0x33, 0x33, 0x47, 0x05, 0x33, 0x2e, 0x05, 0x33, 0x2d, 0x2e, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xd7, 0x18,
    ];
    let prototype = b"srf_prim_ptr(torus)\0\xe0\x01radius1\0\x18\xe0\x01radius2\0\x2e\x05\x33\xe3";
    let mut payload = Vec::new();
    payload.extend_from_slice(b"srf_array\0\xf8\x02");
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&lower);
    payload.push(0xe3);
    payload.extend_from_slice(prototype);
    payload.extend_from_slice(&[8, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&upper);
    payload.push(0xe3);
    payload.extend_from_slice(prototype);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let data = build_prt("two-prototypes-one-frame", &[("ND:0:VisibGeom:0", payload)]);
    let scan = crate::container::scan_bytes(data.clone());
    assert_eq!(scan.surfaces.rows.len(), 2);
    assert_eq!(scan.surfaces.parameters.len(), 2);
    assert_eq!(scan.surfaces.prototype_records.len(), 2);
    assert_eq!(
        super::super::prototypes::unique_surface_prototype_associations(&scan).len(),
        2
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode two prototypes in one surface frame");

    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_PAIRED_ENVELOPE_SPHERE_COUNT),
        0
    );
}
