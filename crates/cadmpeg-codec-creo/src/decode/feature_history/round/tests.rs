// SPDX-License-Identifier: Apache-2.0

#[test]
fn chamfer_does_not_use_a_cone_prototype_as_model_space_placement() {
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

    let mut scan = crate::container::scan_bytes(crate::test_support::build_prt(
        "cone-template",
        &[("VisibGeom", payload)],
    ));
    let [prototype] = scan.surfaces.prototype_records.as_slice() else {
        panic!("complete cone prototype");
    };
    assert_eq!(
        crate::decode::surfaces::unique_surface_prototype_associations(&scan).len(),
        1
    );
    let frame = crate::surface::prototype_cone_frame(prototype).expect("prototype frame");
    assert!(scan
        .surfaces
        .parameters
        .iter()
        .find(|record| record.surface_id == 7)
        .is_some_and(|record| record.positional_cone_frame.is_none()));

    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 31,
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 3,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 31,
    });
    scan.planes
        .positional_frames
        .push(crate::surface::OutlinePlane {
            surface_id: 31,
            origin: std::array::from_fn(|index| frame.apex[index] + frame.axis[index]),
            normal: frame.axis,
            u_axis: frame.ref_direction,
            offset: 31,
        });
    scan.features
        .affected_ids
        .push(crate::feature::FeatureAffectedIds {
            feature_id: 4,
            kind: crate::feature::AffectedIdKind::Geometry,
            ids: vec![31],
            offset: 0,
        });

    assert_eq!(super::chamfer_constant_distance(&scan, 4), None);
}
