use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::SurfaceGeometry;

use crate::test_support::{build_prt, push_named_analytic_prototype};
use crate::CreoCodec;

#[test]
fn first_instance_type26_radius_override_replaces_prototype_radii() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[
        0x18, 0x0d, 0x41, 0xcf, 0xff, 0xff, 0xff, 0xe5, 0x79, 0x7b, 0x0e, 0x29, 0xdf, 0xff,
    ]);
    payload.push(0xe3);
    push_named_analytic_prototype(&mut payload, "torus", &[("radius1", 1.0), ("radius2", 2.0)]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let data = build_prt("prototype-override", &[("ND:0:VisibGeom:0", payload)]);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
        .expect("first instance surface");
    let SurfaceGeometry::Torus {
        center,
        axis,
        ref_direction,
        major_radius,
        minor_radius,
    } = surface.geometry
    else {
        panic!("first instance geometry: {:?}", surface.geometry);
    };
    assert_eq!(center, [0.0, 0.0, 0.0].into());
    assert_eq!(axis, [1.0, 0.0, 0.0].into());
    assert_eq!(ref_direction, [0.0, 1.0, 0.0].into());
    assert_eq!(major_radius, 0.499_999_999_999_999_94);
    assert_eq!(minor_radius, 0.249_999_999_951_747_04);
}
