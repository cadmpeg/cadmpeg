// SPDX-License-Identifier: Apache-2.0
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::build_prt;
use crate::CreoCodec;

#[test]
fn unresolved_round_type26_frames_are_not_admitted_as_constant_tori() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.rows.push(crate::feature::FeatureRow {
        feature_id: 913,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Round),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.surfaces.rows.extend([
        crate::surface::SurfaceRow {
            id: 7,
            kind: crate::surface::SurfaceKind::TorusOrSphere,
            feature_id: 913,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 7,
        },
        crate::surface::SurfaceRow {
            id: 8,
            kind: crate::surface::SurfaceKind::TorusOrSphere,
            feature_id: 913,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 8,
        },
    ]);
    let parameter = |surface_id, minor_radius| crate::surface::SurfaceParameterRecord {
        surface_id,
        body: Vec::new(),
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        terminal_scalar_frame: None,
        carrier: crate::surface::SurfaceParameterCarrier::Resolved(
            crate::surface::InlineSurfaceCarrier::Torus(
                crate::surface::PositionalTorusFrame::new(
                    [0.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0],
                    [1.0, 0.0, 0.0],
                    5.0,
                    minor_radius,
                )
                .expect("valid positional torus frame"),
            ),
        ),
        boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
        offset: surface_id as usize,
        body_offset: surface_id as usize,
    };
    scan.surfaces
        .parameters
        .extend([parameter(7, 1.0), parameter(8, 2.0)]);
    let mut ir = cadmpeg_ir::document::CadIr::empty();

    assert_eq!(
        super::transfer_positional_tori(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        )
        .expect("valid source object identity"),
        0
    );
    assert!(ir.model.surfaces.is_empty());
}

#[test]
fn transfers_an_exact_zero_major_inline_frame_as_a_sphere() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0, 0xe3]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");
    let mut scan =
        crate::container::scan_bytes(build_prt("inline-sphere", &[("ND:0:VisibGeom:0", payload)]));
    assert_eq!(scan.surfaces.rows.len(), 1);
    assert_eq!(scan.surfaces.parameters.len(), 1);
    scan.surfaces.parameters[0].carrier = crate::surface::SurfaceParameterCarrier::Resolved(
        crate::surface::InlineSurfaceCarrier::Torus(
            crate::surface::PositionalTorusFrame::new(
                [2.0, 2.0, 4.0],
                [0.0, 0.0, 1.0],
                [-1.0, 0.0, 0.0],
                0.0,
                2.0,
            )
            .expect("valid positional torus frame"),
        ),
    );
    let mut ir = cadmpeg_ir::document::CadIr::empty();

    assert_eq!(
        super::transfer_positional_tori(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        )
        .expect("valid source object identity"),
        1
    );
    let cadmpeg_ir::geometry::SurfaceGeometry::Sphere { radius, .. } =
        &ir.model.surfaces[0].geometry
    else {
        panic!("zero-major positional frame must transfer as a sphere");
    };
    assert_eq!(*radius, 2.0);
}

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
