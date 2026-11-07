// SPDX-License-Identifier: Apache-2.0

use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::smbh_header_test::smbh_header_prefix;
use crate::test_support::tokens_test::{
    push_u8_string, t_end, t_ident, t_long, t_pos, t_ref, t_subident, t_vec,
};
use crate::test_support::zip_test::f3d_with_smbh;

fn plane(bytes: &mut Vec<u8>, origin: [f64; 3]) {
    t_subident(bytes, "plane");
    t_ident(bytes, "surface");
    t_ref(bytes, -1);
    t_long(bytes, -1);
    t_ref(bytes, -1);
    t_pos(bytes, origin);
    t_vec(bytes, [0.0, 0.0, 1.0]);
    t_vec(bytes, [1.0, 0.0, 0.0]);
    bytes.push(0x0b);
    t_end(bytes);
}

fn bodyless(records: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut bytes = smbh_header_prefix();
    t_ident(&mut bytes, "asmheader");
    push_u8_string(&mut bytes, "231.6.3.65535");
    t_end(&mut bytes);
    records(&mut bytes);
    t_ident(&mut bytes, "delta_state");
    f3d_with_smbh(&bytes)
}

#[test]
fn detached_point_records_survive_in_a_bodyless_document() {
    let input = bodyless(|bytes| {
        for point in [[0.1, 0.2, 0.3], [0.4, 0.5, 0.6], [-0.1, 0.0, 0.2]] {
            t_ident(bytes, "point");
            t_ref(bytes, -1);
            t_long(bytes, -1);
            t_ref(bytes, -1);
            t_pos(bytes, point);
            t_end(bytes);
        }
    });
    let decoded = super::decode(input);
    assert_eq!(decoded.ir().model.points.len(), 3);
    for (point, expected) in
        decoded
            .ir()
            .model
            .points
            .iter()
            .zip([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [-1.0, 0.0, 2.0]])
    {
        let actual = point.position().get();
        for (actual, expected) in [actual.x, actual.y, actual.z].into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-10);
        }
        assert!(point.source_object.is_some());
    }
    assert!(decoded.ir().model.bodies.is_empty());
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.local_code() != "geometry.not-transferred"));
    super::assert_valid(&decoded);
}

#[test]
fn detached_plane_survives_in_a_bodyless_document() {
    let decoded = super::decode(bodyless(|bytes| plane(bytes, [0.3, 0.0, 0.1])));
    assert_eq!(decoded.ir().model.surfaces.len(), 1);
    let cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Plane(plane),
    ) = &decoded.ir().model.surfaces[0].geometry
    else {
        panic!("expected a plane surface");
    };
    let origin = plane.origin().get();
    assert!((origin.x - 3.0).abs() < 1e-10);
    assert!(origin.y.abs() < 1e-10);
    assert!((origin.z - 1.0).abs() < 1e-10);
    assert!(decoded.ir().model.surfaces[0].source_object.is_some());
    assert!(decoded.ir().model.faces.is_empty());
    super::assert_valid(&decoded);
}

#[test]
fn detached_plane_survives_alongside_a_face_surface() {
    let mut bytes = synthetic_geometry_smbh();
    let mut boundary = Vec::new();
    t_ident(&mut boundary, "delta_state");
    let at = bytes
        .windows(boundary.len())
        .position(|window| window == boundary)
        .unwrap();
    let mut extra = Vec::new();
    plane(&mut extra, [0.3, 0.0, 0.1]);
    bytes.splice(at..at, extra);
    let decoded = super::decode(f3d_with_smbh(&bytes));
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(decoded.ir().model.surfaces.len(), 2);
    super::assert_valid(&decoded);
}

#[test]
fn detached_spline_surface_survives_in_a_bodyless_document() {
    let source = crate::test_support::smbh_surfaces_test::synthetic_cyl_spl_sur_smbh();
    let start = cadmpeg_asm::asm_header::record_stream_start(&source).unwrap();
    let limit = cadmpeg_asm::asm_header::solved_record_limit(&source).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &source,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let record = &records[9];
    let input = bodyless(|bytes| {
        bytes.extend_from_slice(&source[record.offset..record.offset + record.len])
    });
    let decoded = super::decode(input);
    assert_eq!(decoded.ir().model.surfaces.len(), 1);
    assert!(decoded.ir().model.faces.is_empty());
    assert!(decoded.ir().model.surfaces[0].source_object.is_some());
    assert!(!decoded.ir().model.procedural_surfaces.is_empty());
    super::assert_valid(&decoded);
}
