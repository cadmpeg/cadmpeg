// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

fn intersection_surface_source() -> Vec<u8> {
    use crate::test_support::parasolid::{
        be16, be32, bef64, nurbs_surface_carrier, plane_carrier, triangle_body,
    };
    let mut body = triangle_body();
    let bridge = body
        .windows(2)
        .position(|window| window == [0, 0x0e])
        .unwrap();
    body[bridge + 26..bridge + 28].copy_from_slice(&180_u16.to_be_bytes());
    let edge = body
        .windows(2)
        .position(|window| window == [0, 0x10])
        .unwrap();
    body[edge + 24..edge + 26].copy_from_slice(&190_u16.to_be_bytes());
    body.extend(nurbs_surface_carrier(180, 181, 10));
    body.extend(plane_carrier(
        200,
        [0.0; 3],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
    ));
    body.extend_from_slice(&[0, 0x26]);
    be16(&mut body, 190);
    body.extend_from_slice(&[0; 14]);
    body.push(0x2b);
    for reference in [180, 200, 194, 195, 196, 0] {
        be16(&mut body, reference);
    }
    body.extend_from_slice(&[0, 0x28]);
    be32(&mut body, 3);
    be16(&mut body, 194);
    bef64(&mut body, 0.0);
    bef64(&mut body, 1.0);
    be32(&mut body, 3);
    bef64(&mut body, 0.001);
    body.extend_from_slice(&[0; 8]);
    for _ in 0..2 {
        bef64(&mut body, -31_415_800_000_000.0);
    }
    for point in [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0], [1.0, 0.0, 0.0]] {
        for coordinate in point {
            bef64(&mut body, coordinate);
        }
    }
    for (reference, point) in [(195, [0.0, 0.0, 0.0]), (196, [1.0, 0.0, 0.0])] {
        body.extend_from_slice(&[0, 0x29]);
        be32(&mut body, 1);
        be16(&mut body, reference);
        body.extend_from_slice(b"L?");
        for coordinate in point {
            bef64(&mut body, coordinate);
        }
    }
    crate::test_support::container::sldprt_with_body(&body)
}

#[test]
fn geometry_surface_solver_derives_intersection_pcurve() {
    let source = intersection_surface_source();
    let options = DecodeOptions {
        policy: DecodePolicy::service(),
        ..DecodeOptions::default()
    };
    let decoded = cadmpeg_test_support::EditableDecodeResult::from(
        crate::SldprtCodec
            .decode(&mut Cursor::new(&source), &options)
            .unwrap(),
    );
    assert!(decoded.ir().model.pcurves.iter().any(|pcurve| {
        decoded
            .source_fidelity()
            .annotations
            .provenance
            .get(pcurve.id.as_str())
            .and_then(|note| note.tag.as_deref())
            == Some("derived_intersection_nurbs_uv")
    }));
}
