use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::SurfaceGeometry;

use crate::test_support::{
    assert_unknown_visible_surface, build_prt, push_named_analytic_prototype,
};
use crate::CreoCodec;

#[test]
fn first_instance_cone_prototype_requires_a_model_space_placement() {
    let body = [
        197, 251, 126, 24, 209, 212, 112, 107, 81, 235, 133, 30, 184, 70, 125, 251, 126, 24, 209,
        212, 112, 123, 0, 68, 204, 99, 17, 228, 72, 66, 64, 192, 170, 175, 125, 232, 45, 177, 195,
        0, 68, 204, 99, 17, 220, 70, 66, 1, 69, 135, 177, 98, 82, 120, 170, 175, 125, 232, 45, 187,
        65, 200, 122, 225, 71, 174, 20, 128, 227, 24, 228, 15, 24, 15, 24, 16, 24, 228, 70, 66,
        129, 71, 174, 20, 122, 225, 25, 194, 145, 29, 33, 143, 32, 210, 52, 233, 0, 116, 33, 251,
        84, 68, 45, 5,
    ];
    let (local_system, half_angle) = body.split_at(body.len() - 7);
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x25, 4, 0x01, 0, 0]);
    payload.extend_from_slice(b"srf_prim_ptr(cone)\0\xe0\x02local_sys\0\xf9\x04\x03");
    payload.extend_from_slice(local_system);
    payload.extend_from_slice(b"\xe0\x01half_angle\0");
    payload.extend_from_slice(half_angle);
    payload.extend_from_slice(b"\xe0\x00parent_feats\0\xf8\x01\x04");
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let data = build_prt("c", &[("ND:0:VisibGeom:0", payload)]);
    let scan = crate::container::scan_bytes(data.clone());
    let [prototype] = scan.surfaces.prototype_records.as_slice() else {
        panic!("complete cone prototype");
    };
    assert_eq!(
        prototype.family,
        crate::surface::SurfacePrototypeFamily::Cone
    );
    assert!(crate::surface::prototype_cone_frame(prototype).is_some());
    assert_eq!(super::unique_surface_prototype_associations(&scan).len(), 1);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    assert_unknown_visible_surface(&result.ir().model.surfaces, 7);
}

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
