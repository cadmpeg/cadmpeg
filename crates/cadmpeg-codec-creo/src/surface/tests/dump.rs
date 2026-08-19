// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::sketches::{SketchConstraintDefinition, SketchEntityId};
use cadmpeg_ir::Exactness;

use crate::container::{self, role, Layout};
use crate::surface::TorusRadius2Encoding;
use crate::test_support::*;
use crate::CreoCodec;

#[test]
fn decode_transfers_positional_line_extrusion_plane() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x2c, 4, 0x01, 0, 0]);
    for value in [0.0, 0.0, 1.0] {
        push_generated_scalar(&mut payload, value);
    }
    payload.extend_from_slice(&[0x00, 0x0c, 0x9a]);
    for value in [0.0, 0.0, 0.0, 2.0, 0.0, 0.0] {
        push_generated_scalar(&mut payload, value);
    }
    payload.push(0xe3);
    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode");

    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
        .expect("extrusion plane");
    assert!(matches!(
        surface.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3 {
                x: 0.0,
                y: 0.0,
                z: 0.0
            },
            normal: cadmpeg_ir::math::Vector3 {
                x: 0.0,
                y: -1.0,
                z: 0.0
            },
            u_axis: cadmpeg_ir::math::Vector3 {
                x: 1.0,
                y: 0.0,
                z: 0.0
            },
        }
    ));
    let carrier_id = surface.id.clone();
    let construction = result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| surface.surface == carrier_id)
        .expect("extrusion construction");
    assert!(matches!(
        construction.definition,
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Extrusion {
            parameter_interval: None,
            direction: cadmpeg_ir::math::Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
            native_position: None,
            ..
        }
    ));
    let record = &result.ir().native.namespace("creo").unwrap().arenas["surface_parameters"][0];
    assert_eq!(record.fields()["surface_type_byte"], 0x2c);
    assert_eq!(record.fields()["extrusion_direction"][0], 0.0);
    assert_eq!(record.fields()["extrusion_direction"][1], 0.0);
    assert_eq!(record.fields()["extrusion_direction"][2], 1.0);
    assert_eq!(
        result
            .report()
            .coverage
            .get("decoded_positional_extrusion_direction_count")
            .copied(),
        Some(1)
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_withholds_positional_line_extrusion_for_duplicate_surface_id() {
    let mut extrusion = visibgeom_payload(1, 0);
    extrusion.extend_from_slice(&[7, 0x2c, 4, 0x01, 0, 0]);
    for value in [0.0, 0.0, 1.0] {
        push_generated_scalar(&mut extrusion, value);
    }
    extrusion.extend_from_slice(&[0x00, 0x0c, 0x9a]);
    for value in [0.0, 0.0, 0.0, 2.0, 0.0, 0.0] {
        push_generated_scalar(&mut extrusion, value);
    }
    extrusion.push(0xe3);

    let mut plane = visibgeom_payload(1, 0);
    plane.extend_from_slice(&[7, 0x26, 5, 0x01, 0, 0, 0xe4, 0xe3]);
    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt(
                "c",
                &[("ND:0:VisibGeom:0", extrusion), ("ND:1:VisibGeom:0", plane)],
            )),
            &DecodeOptions::default(),
        )
        .expect("decode");

    assert!(result
        .ir()
        .model
        .surfaces
        .iter()
        .all(|surface| surface.id.as_str() != "creo:visibgeom:surface#7"));
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .all(|curve| curve.id.as_str() != "creo:visibgeom:surface_directrix#7"));
    assert!(result
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .all(|surface| surface.id.as_str() != "creo:visibgeom:surface_extrusion#7"));
}

#[test]
fn decode_preserves_type_2c_direction_before_named_record() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x2c, 4, 0x01, 0, 0, 0x0f, 0xe4, 0x0f]);
    payload.extend_from_slice(&[0x00, 0x0c, 0x9a]);
    payload.extend_from_slice(b"\xe0\x01next_record\0");
    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode");

    let record = &result.ir().native.namespace("creo").unwrap().arenas["surface_parameters"][0];
    assert_eq!(record.fields()["boundary"], "named_record");
    assert_eq!(record.fields()["extrusion_direction"][0], 0.0);
    assert_eq!(record.fields()["extrusion_direction"][1], 1.0);
    assert_eq!(record.fields()["extrusion_direction"][2], 0.0);
    assert_unknown_visible_surface(&result.ir().model.surfaces, 7);
}

#[test]
fn decode_preserves_surface_parameter_slots_in_native_ir() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[0x73, 0xe4, 0x2f, 0x43, 0, 0xe3, 0xe0]);
    payload.push(0xe3);
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode surface parameters");

    let records = &result.ir().native.namespace("creo").unwrap().arenas["surface_parameters"];
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].fields()["surface_id"], 7);
    assert_eq!(records[0].fields()["surface_family"], "torus_or_sphere");
    assert_eq!(records[0].fields()["boundary"], "compound_close");
    assert_eq!(
        records[0].fields()["slots"][0]["value"],
        f64::from_be_bytes([0x3f, 0xe8, 0xe4, 0x2f, 0x43, 0, 0xe3, 0xe0])
    );
    for (index, expected) in [0x73, 0xe4, 0x2f, 0x43, 0, 0xe3, 0xe0]
        .into_iter()
        .enumerate()
    {
        assert_eq!(records[0].fields()["slots"][0]["raw"][index], expected);
    }
    assert_eq!(records[0].fields()["slots"][0]["length"], 7);
    assert_eq!(
        records[0].fields()["opaque_spans"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(records[0].fields()["terminal_scalar_frame"]["offset"], 0);
    assert_eq!(records[0].fields()["scalar_frames"][0]["offset"], 0);
    assert_eq!(
        records[0].fields()["terminal_scalar_frame"]["slots"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let row = &result.ir().native.namespace("creo").unwrap().arenas["surface_rows"][0];
    assert_eq!(row.fields()["surface_id"], 7);
    assert_eq!(row.fields()["type_byte"], 0x26);
    assert_eq!(row.fields()["surface_family"], "torus_or_sphere");
    assert_eq!(row.fields()["feature_id"], 4);
    assert_eq!(row.fields()["reversed"], false);
    assert_eq!(row.fields()["boundary_type"], 0);
    assert_eq!(row.fields()["next_surface"], 0);
    assert_eq!(
        result.source_fidelity().annotations.provenance["creo:visibgeom:surface_row#7"]
            .tag
            .as_deref(),
        Some("surface_namespace_row")
    );
}

#[test]
fn decode_retains_type26_coordinate_envelope_in_native_ir() {
    let body = [
        0x18, 0x18, 0x01, 0x11, 0x2e, 0xb0, 0x12, 0x47, 0x05, 0x33, 0x2d, 0x2d, 0xff, 0xff, 0xff,
        0xff, 0xff, 0x29, 0x47, 0x05, 0x33, 0x2e, 0x05, 0x33, 0x2d, 0x31, 0xa6, 0x66, 0x66, 0x66,
        0x66, 0x66, 0x18,
    ];
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&body);
    payload.push(0xe3);
    let data = build_prt("c", &[("VisibGeom", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode type-26 envelope");
    let record = &result.ir().native.namespace("creo").unwrap().arenas["surface_parameters"][0];
    let envelope = &record.fields()["type26_five_coordinate_envelope"];
    assert_eq!(envelope["offset"], 7);
    let values = envelope["values"].as_array().expect("coordinate values");
    for (actual, expected) in values.iter().zip([-2.65, -15.0, -2.65, 2.65, -17.65]) {
        assert!((actual.as_f64().expect("finite coordinate") - expected).abs() < 1.0e-12);
    }
    assert!(record.fields()["type26_split_coordinate_envelope"].is_null());
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::DECODED_TYPE26_FIVE_COORDINATE_ENVELOPE_COUNT),
        1
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("1 five-coordinate envelope(s)")));
}

#[test]
fn decode_places_complete_positional_torus() {
    let body = [
        40, 141, 7, 27, 210, 101, 111, 108, 24, 148, 63, 2, 112, 22, 190, 252, 0, 18, 32, 71, 19,
        204, 70, 49, 61, 112, 163, 215, 10, 62, 71, 19, 204, 46, 19, 204, 70, 48, 189, 112, 163,
        215, 10, 62, 33, 177, 72, 10, 227, 194, 255, 45, 89, 199, 15, 241, 65, 141, 6, 220, 32,
        138, 77, 219, 24, 229, 16, 40, 141, 6, 220, 32, 138, 77, 219, 194, 255, 45, 89, 199, 15,
        241, 24, 228, 70, 48, 189, 112, 163, 215, 10, 62, 24, 46, 17, 204, 14,
    ];
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend(body);
    payload.push(0xe3);
    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("VisibGeom", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode complete positional torus");

    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
        .expect("positional torus surface");
    assert!(matches!(
        surface.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if (center.x - 1.0).abs() < 1e-12
            && (center.y - 16.74).abs() < 1e-12
            && center.z.abs() < 1e-12
            && axis.x.abs() < 1e-12
            && axis.y.abs() < 1e-12
            && (axis.z - 1.0).abs() < 1e-12
            && (ref_direction.x + 0.999_899_554_583_406_1).abs() < 1e-12
            && (ref_direction.y - 0.014_173_240_416_574_131).abs() < 1e-12
            && ref_direction.z.abs() < 1e-12
            && (major_radius - 4.45).abs() < 1e-12
            && (minor_radius - 0.5).abs() < 1e-12
    ));
    let record = &result.ir().native.namespace("creo").unwrap().arenas["surface_parameters"][0];
    assert!(
        (record.fields()["positional_torus_frame"]["major_radius"]
            .as_f64()
            .expect("major radius")
            - 4.45)
            .abs()
            < 1e-12
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_POSITIONAL_TORUS_COUNT),
        1
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("1 exact positional torus carrier")));
}

#[test]
fn decode_reports_transferred_positional_cylinders() {
    let body = [
        17, 72, 0, 0, 19, 24, 72, 55, 192, 70, 29, 255, 255, 255, 255, 255, 143, 72, 38, 0, 72, 52,
        64, 70, 21, 255, 255, 255, 255, 255, 143, 72, 34, 128,
    ];
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 0]);
    payload.extend(body);
    payload.push(0xe3);
    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("VisibGeom", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode positional cylinder");

    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_POSITIONAL_CYLINDER_COUNT),
        1
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("1 exact positional cylinder carrier")));
}

#[test]
fn decode_places_paired_five_coordinate_sphere_envelopes() {
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
    let mut payload = b"srf_array\0\xf8\x02".to_vec();
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&lower);
    payload.push(0xe3);
    payload.extend_from_slice(&[8, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&upper);
    payload.push(0xe3);
    payload.extend_from_slice(
        b"srf_prim_ptr(torus)\0\xe0\x01radius1\0\x18\xe0\x01radius2\0\x2e\x05\x33\xe3",
    );
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode paired sphere envelopes");
    for id in [7, 8] {
        let surface = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == format!("creo:visibgeom:surface#{id}"))
            .expect("paired sphere surface");
        assert!(matches!(
            surface.geometry,
            cadmpeg_ir::geometry::SurfaceGeometry::Sphere {
                center,
                axis,
                ref_direction,
                radius,
            } if center.x == 0.0
                && center.y == 0.0
                && (center.z + 15.0).abs() < 1.0e-12
                && axis == cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
                && ref_direction == cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
                && radius == 2.65
        ));
    }
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_PAIRED_ENVELOPE_SPHERE_COUNT),
        2
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("Transferred 2 sphere carrier(s) from complementary five-coordinate")
    }));
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("Placement-complete paired sphere envelopes additionally transfer")
    }));
}

#[test]
fn decode_retains_split_type26_coordinate_envelope_in_native_ir() {
    let body = [
        0x28, 0x8d, 0x07, 0x1b, 0xd2, 0x65, 0x6f, 0x6c, 0x18, 0x94, 0x3f, 0x02, 0x70, 0x16, 0xbe,
        0xfc, 0x00, 0x12, 0x20, 0x47, 0x13, 0xcc, 0x46, 0x31, 0x3d, 0x70, 0xa3, 0xd7, 0x0a, 0x3e,
        0x3a, 0xb1, 0x47, 0xba, 0x2e, 0x13, 0xcc, 0x46, 0x30, 0xbd, 0x70, 0xa3, 0xd7, 0x0a, 0x3e,
        0x2e, 0x13, 0xcc,
    ];
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&body);
    payload.push(0xe3);
    let data = build_prt("c", &[("VisibGeom", payload)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode split type-26 envelope");
    let record = &result.ir().native.namespace("creo").unwrap().arenas["surface_parameters"][0];
    let envelope = &record.fields()["type26_split_coordinate_envelope"];
    assert_eq!(envelope["offset"], 19);
    let values = envelope["values"].as_array().expect("coordinate values");
    for (actual, expected) in values.iter().zip([-4.95, 17.24, 16.74, 4.95]) {
        assert!((actual.as_f64().expect("finite coordinate") - expected).abs() < 1.0e-12);
    }
    assert!(record.fields()["type26_five_coordinate_envelope"].is_null());
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::DECODED_TYPE26_SPLIT_COORDINATE_ENVELOPE_COUNT),
        1
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("1 split-coordinate envelope(s)")));
}

#[test]
fn decode_preserves_unframed_surface_parameter_spans() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[0x11, 0xe4, 0x12, 0x13, 0x0d, 0xe3]);
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode surface parameter spans");

    let record = &result.ir().native.namespace("creo").unwrap().arenas["surface_parameters"][0];
    assert_eq!(record.fields()["slots"][0]["offset"], 1);
    assert_eq!(record.fields()["slots"][1]["offset"], 4);
    assert_eq!(record.fields()["opaque_spans"][0]["offset"], 0);
    assert_eq!(record.fields()["opaque_spans"][0]["raw"][0], 0x11);
    assert_eq!(record.fields()["opaque_spans"][1]["offset"], 2);
    assert_eq!(record.fields()["opaque_spans"][1]["length"], 2);
    assert_eq!(record.fields()["terminal_scalar_frame"]["offset"], 4);
    let record_fields = record.fields();
    let frames = record_fields["scalar_frames"].as_array().unwrap();
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0]["offset"], 1);
    assert_eq!(frames[1]["offset"], 4);
    assert_eq!(
        record.fields()["terminal_scalar_frame"]["slots"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn decode_transfers_axis_aligned_plane_from_outline() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    for value in [0.0, 0.0, 0.0, 0.0, -1.0, -1.0, 1.0, 1.0, 2.0, 1.0] {
        push_generated_scalar(&mut payload, value);
    }
    payload.push(0xe3);
    payload.extend_from_slice(&[0x0f, 0xe4, 0x0f, 0xe4, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x0f, 0xe4]);
    payload.push(0xe3);
    let data = build_prt("c", &[("VisibGeom", payload)]);
    let expected_offset = container::scan_bytes(data.clone()).planes.local_systems[0].offset as u64;
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let namespace = result.ir().native.namespace("creo").unwrap();
    assert_eq!(
        namespace.arenas["plane_local_systems"][0].fields()["surface_id"],
        7
    );
    assert_eq!(
        namespace.arenas["plane_envelopes"][0].fields()["surface_id"],
        7
    );
    assert_eq!(
        namespace.arenas["plane_envelopes"][0].fields()["envelope"]["kind"],
        "standard"
    );
    assert_eq!(
        namespace.arenas["outline_planes"][0].fields()["normal"][2],
        -1.0
    );

    assert_eq!(result.ir().model.surfaces.len(), 1);
    let surface = &result.ir().model.surfaces[0];
    assert_eq!(surface.id.as_str(), "creo:visibgeom:surface#7");
    assert_eq!(
        surface.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(3.0, 0.0, 1.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        }
    );
    assert_annotation(
        &result.source_fidelity().annotations,
        surface.id.as_str(),
        "creo:VisibGeom",
        expected_offset,
        "plane_local_system",
        Exactness::Derived,
    );
    assert!(result.report().geometry_transferred);
}

#[test]
fn decode_transfers_plane_from_shared_rank_two_local_system_image() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[0x0f; 10]);
    payload.push(0xe3);
    payload.extend_from_slice(&[
        0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18, 0xe6, 0xe1, 0xe3,
    ]);

    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("VisibGeom", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode");

    assert_eq!(result.ir().model.surfaces.len(), 1);
    assert_eq!(
        result.ir().model.surfaces[0].geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        }
    );
    let coverage = result.report();
    assert_eq!(
        coverage.coverage_count(crate::coverage::VISIBLE_PLANE_SURFACE_ROW_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::TRANSFERRED_VISIBLE_PLANE_SURFACE_ROW_COUNT),
        1
    );
    assert_eq!(
        coverage.coverage_count(crate::coverage::UNTRANSFERRED_VISIBLE_SURFACE_ROW_COUNT),
        0
    );
}

#[test]
fn decode_uses_support_frame_to_chart_line_shaped_plane_outline() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[0x0f; 10]);
    payload.push(0xe3);
    payload.extend_from_slice(&[0x0f, 0xe4, 0x0f, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f]);
    payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0, 0x0f, 0xe4]);
    payload.push(0xe3);

    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("VisibGeom", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode");

    assert_eq!(result.ir().model.surfaces.len(), 1);
    assert_eq!(
        result.ir().model.surfaces[0].geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(3.0, 0.0, 0.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        }
    );
}

#[test]
fn decode_transfers_held_coordinate_plane_with_canonical_chart() {
    let mut payload = visibgeom_payload(1, 0);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    for value in [0.0, 0.0, 0.0, 0.0, -1.0, -1.0, 1.0, 1.0, 2.0, 1.0] {
        push_generated_scalar(&mut payload, value);
    }
    payload.push(0xe3);

    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("VisibGeom", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode");

    assert_eq!(result.ir().model.surfaces.len(), 1);
    assert_eq!(
        result.ir().model.surfaces[0].geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 1.0),
            normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        }
    );
}

#[test]
fn decode_withholds_unplaced_cylinder_prototype_frame() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 0]);
    payload.extend_from_slice(b"srf_prim_ptr(cylinder)\0\xe0\x01radius\0");
    push_generated_scalar(&mut payload, 1.0);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode");
    assert_unknown_visible_surface(&result.ir().model.surfaces, 7);
}

#[test]
fn decode_places_first_cylinder_instance_from_complete_named_prototype() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 0]);
    push_named_analytic_prototype(&mut payload, "cylinder", &[("radius", 1.0)]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode");
    let cylinder = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
        .expect("first cylinder instance");

    assert_eq!(
        cylinder.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            ref_direction: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
            radius: 1.0,
        }
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.message.contains(
            "first-instance ND plane, cylinder, cone, torus, or interpolation-spline carrier",
        )
    }));
}

#[test]
fn decode_withholds_complete_cylinder_prototype_without_positive_radius() {
    for fields in [Vec::new(), vec![("radius", -1.0)]] {
        let mut payload = b"srf_array\0\xf8\x01".to_vec();
        payload.extend_from_slice(&[7, 0x24, 4, 0x01, 0, 0]);
        push_named_analytic_prototype(&mut payload, "cylinder", &fields);
        payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

        let result = CreoCodec
            .decode(
                &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
                &DecodeOptions::default(),
            )
            .expect("decode");
        assert_unknown_visible_surface(&result.ir().model.surfaces, 7);
    }
}

#[test]
fn decode_places_direct_two_direction_named_prototype_frame() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    payload.extend_from_slice(b"srf_prim_ptr(torus)\0\xe0\x02local_sys\0\xf9\x04\x03");
    for value in [1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, -2.0] {
        push_generated_scalar(&mut payload, value);
    }
    payload.extend_from_slice(b"\xe0\x01radius1\0\xe4\xe0\x01radius2\0\xe4");
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode");
    let torus = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
        .expect("first torus instance");

    assert_eq!(
        torus.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Torus {
            center: cadmpeg_ir::math::Point3::new(2.0, 0.0, -2.0),
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0),
            ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            major_radius: 1.0,
            minor_radius: 1.0,
        }
    );
}

#[test]
fn decode_does_not_promote_untyped_terminal_torus_scalars() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0]);
    push_generated_scalar(&mut payload, 2.0);
    push_generated_scalar(&mut payload, 1.0);
    payload.push(0xe3);
    payload.extend_from_slice(b"srf_prim_ptr(torus)\0\xe0\x02local_sys\0\xf9\x04\x03");
    for value in [1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, -2.0] {
        push_generated_scalar(&mut payload, value);
    }
    payload.extend_from_slice(b"\xe0\x01radius1\0\xe4\xe0\x01radius2\0\xe4");
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode");
    let torus = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
        .expect("first torus instance");

    assert_eq!(
        torus.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Torus {
            center: cadmpeg_ir::math::Point3::new(2.0, 0.0, -2.0),
            axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0),
            ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            major_radius: 1.0,
            minor_radius: 1.0,
        }
    );
}

#[test]
fn decode_replays_a_unique_section_prototype_minor_radius_at_type26_row_end() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x26, 4, 0x01, 0, 0, 0x18, 0x0c]);
    payload.extend_from_slice(&[0x29, 0xc9, 0x99]);
    payload.push(0xe3);
    payload.extend_from_slice(b"srf_prim_ptr(torus)\0\xe0\x02local_sys\0\xf9\x04\x03");
    for value in [1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, -2.0] {
        push_generated_scalar(&mut payload, value);
    }
    payload.extend_from_slice(b"\xe0\x01radius1\0\xe4\xe0\x01radius2\0\x29\xc9\x99");
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode");
    let native = &result.ir().native.namespace("creo").unwrap().arenas["surface_parameters"][0];
    assert_eq!(
        native.fields()["replayed_torus_minor_radius"],
        0.199_999_999_999_999_98
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::DECODED_TYPE26_REPLAYED_MINOR_RADIUS_COUNT),
        1
    );
}

#[test]
fn decode_places_first_plane_instance_from_named_prototype() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    push_named_analytic_prototype(&mut payload, "plane", &[]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode");
    let plane = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
        .expect("first plane instance");

    assert_eq!(
        plane.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            normal: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            u_axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        }
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FIRST_INSTANCE_PROTOTYPE_SURFACE_COUNT),
        1
    );
}

#[test]
fn decode_places_named_prototype_before_its_surface_row() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    push_named_analytic_prototype(&mut payload, "plane", &[]);
    payload.push(0xe3);
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode");

    let plane = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
        .expect("following first plane instance");
    assert!(matches!(
        plane.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Plane { .. }
    ));
}

#[test]
fn decode_does_not_cross_counted_surface_array_frames_for_prototypes() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    push_named_analytic_prototype(&mut payload, "plane", &[]);
    payload.push(0xe3);
    payload.extend_from_slice(b"srf_array\0\xf8\x01");
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode");

    assert_unknown_visible_surface(&result.ir().model.surfaces, 7);
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FIRST_INSTANCE_PROTOTYPE_SURFACE_COUNT),
        0
    );
}

#[test]
fn decode_does_not_use_incomplete_frame_for_prototype_join() {
    let mut payload = b"srf_array\0\xf8\x02".to_vec();
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    push_named_analytic_prototype(&mut payload, "plane", &[]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode");

    assert_unknown_visible_surface(&result.ir().model.surfaces, 7);
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FIRST_INSTANCE_PROTOTYPE_SURFACE_COUNT),
        0
    );
}

#[test]
fn decode_binds_prototype_between_same_family_rows_to_the_preceding_instance() {
    let mut payload = b"srf_array\0\xf8\x02".to_vec();
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    push_named_analytic_prototype(&mut payload, "plane", &[]);
    payload.push(0xe3);
    payload.extend_from_slice(&[8, 0x22, 4, 0x01, 0, 0]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode");

    assert!(result
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "creo:visibgeom:surface#7"));
    assert_unknown_visible_surface(&result.ir().model.surfaces, 8);
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_FIRST_INSTANCE_PROTOTYPE_SURFACE_COUNT),
        1
    );
}

#[test]
fn decode_withholds_competing_named_prototypes_for_one_surface_row() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
    push_named_analytic_prototype(&mut payload, "plane", &[]);
    push_named_analytic_prototype(&mut payload, "plane", &[]);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode");

    assert_unknown_visible_surface(&result.ir().model.surfaces, 7);
}

#[test]
fn decode_places_first_interpolation_spline_instance_from_named_prototype() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x28, 4, 0x01, 0, 0]);
    payload.extend_from_slice(b"srf_prim_ptr(splsrf)\0\xe0\x02i_points\0\xf9\x04\x03");
    for point in [
        [0.0, 0.0, 0.0],
        [0.0, 1.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 2.0],
    ] {
        for value in point {
            push_generated_scalar(&mut payload, value);
        }
    }
    payload.extend_from_slice(b"\xe0\x02end_u_tangts\0\xf9\x04\x03");
    for _ in 0..4 {
        for value in [1.0, 0.0, 1.0] {
            push_generated_scalar(&mut payload, value);
        }
    }
    payload.extend_from_slice(b"\xe0\x02end_v_tangts\0\xf9\x04\x03");
    for _ in 0..4 {
        for value in [0.0, 1.0, 1.0] {
            push_generated_scalar(&mut payload, value);
        }
    }
    payload.extend_from_slice(b"\xe0\x02end_uv_deriv\0\xf9\x04\x03");
    for _ in 0..12 {
        push_generated_scalar(&mut payload, 0.0);
    }
    for name in ["u_params", "v_params"] {
        payload.extend_from_slice(&[0xe0, 0x02]);
        payload.extend_from_slice(name.as_bytes());
        payload.extend_from_slice(&[0, 0xf8, 0x02]);
        push_generated_scalar(&mut payload, 0.0);
        push_generated_scalar(&mut payload, 1.0);
    }
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let data = build_prt("c", &[("ND:0:VisibGeom:0", payload)]);
    let scan = container::scan_bytes(data.clone());
    assert_eq!(scan.surfaces.rows.len(), 1);
    assert_eq!(scan.surfaces.prototype_records.len(), 1);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
        .expect("first interpolation spline instance");
    let cadmpeg_ir::geometry::SurfaceGeometry::Nurbs(nurbs) = &surface.geometry else {
        panic!("expected NURBS surface");
    };

    assert_eq!((nurbs.u_degree, nurbs.v_degree), (3, 3));
    assert_eq!((nurbs.u_count, nurbs.v_count), (4, 4));
    assert_eq!(
        nurbs.control_points[0],
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
    );
    assert_eq!(
        nurbs.control_points[15],
        cadmpeg_ir::math::Point3::new(1.0, 1.0, 2.0)
    );
}

#[test]
fn decode_places_first_sphere_and_torus_instances_from_named_prototypes() {
    let cases = [
        (
            0x26,
            "torus",
            vec![("radius1", 0.0), ("radius2", 1.0)],
            cadmpeg_ir::geometry::SurfaceGeometry::Sphere {
                center: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                ref_direction: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
                radius: 1.0,
            },
        ),
        (
            0x26,
            "torus",
            vec![("radius1", 2.0), ("radius2", 1.0)],
            cadmpeg_ir::geometry::SurfaceGeometry::Torus {
                center: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                ref_direction: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
                major_radius: 2.0,
                minor_radius: 1.0,
            },
        ),
    ];

    for (kind, family, fields, expected) in cases {
        let mut payload = b"srf_array\0\xf8\x01".to_vec();
        payload.extend_from_slice(&[7, kind, 4, 0x01, 0, 0]);
        push_named_analytic_prototype(&mut payload, family, &fields);
        payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");
        let result = CreoCodec
            .decode(
                &mut Cursor::new(build_prt("c", &[("ND:0:VisibGeom:0", payload)])),
                &DecodeOptions::default(),
            )
            .expect("decode");
        let surface = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#7")
            .unwrap_or_else(|| panic!("first {family} instance"));
        assert_eq!(surface.geometry, expected);
    }
}

#[test]
fn decode_places_x_axis_cylinder_from_outline_bound_cap_pair() {
    fn world(payload: &mut Vec<u8>, value: f64) {
        let raw = value.to_be_bytes();
        payload.push(match raw[0] {
            0x40 => 0x46,
            0xc0 => 0x2d,
            _ => panic!("generated FC05 value must use a world-token exponent"),
        });
        payload.extend_from_slice(&raw[1..]);
    }
    fn plane_row(payload: &mut Vec<u8>, id: u8, next: u8, x: f64) {
        payload.extend_from_slice(&[id, 0x22, 4, 0x01, 0, next]);
        for value in [0.0, 1.0, 0.0, 1.0, x, -1.0, -1.0, x, 1.0, 2.0] {
            push_generated_scalar(payload, value);
        }
        payload.push(0xe3);
    }
    fn circle_row(
        payload: &mut Vec<u8>,
        curve: u8,
        plane: u8,
        ordinate: f64,
        preserve_parameters: bool,
    ) {
        payload.extend_from_slice(&[curve, 0x09, 4, 0x01, 0xf6, 0xfc, 0x05]);
        for [a, b, parameter] in [
            [4.0, 5.0, 2.0],
            [3.0, 6.0, 2.0 + std::f64::consts::FRAC_PI_2],
            [2.0, 5.0, 2.0 + std::f64::consts::PI],
            [3.0, 4.0, 2.0 + 3.0 * std::f64::consts::FRAC_PI_2],
        ] {
            world(payload, a);
            world(payload, b);
            world(payload, if preserve_parameters { parameter } else { 2.0 });
            world(payload, ordinate);
        }
        payload.push(0xff);
        payload.extend_from_slice(&[10, plane, curve, curve, 0, 0, 0xe3]);
        payload.extend_from_slice(&[0xe1, 0xf5, 0x05, 0xf6, 0xe3]);
    }

    let mut payload = b"srf_array\0\xf8\x03".to_vec();
    payload.extend_from_slice(&[10, 0x24, 4, 0x01, 0, 11]);
    plane_row(&mut payload, 11, 12, 2.0);
    plane_row(&mut payload, 12, 0, -2.0);
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\x02topol_ref_data\0");
    let mut one_cap_payload = payload.clone();
    circle_row(&mut one_cap_payload, 20, 11, -5.0, true);
    let one_cap = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("VisibGeom", one_cap_payload)])),
            &DecodeOptions::default(),
        )
        .expect("one-cap decode");
    let one_cap_cylinder = one_cap
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#10")
        .expect("placed one-cap cylinder");
    assert!(matches!(
        one_cap_cylinder.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
            origin: cadmpeg_ir::math::Point3 {
                x: 2.0,
                y: 5.0,
                z: 3.0
            },
            radius: 1.0,
            ..
        }
    ));
    let one_cap_circle = one_cap
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "creo:visibgeom:curve#20")
        .expect("placed one-cap circle");
    assert!(matches!(
        one_cap_circle.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Circle {
            center: cadmpeg_ir::math::Point3 {
                x: 2.0,
                y: 5.0,
                z: 3.0
            },
            axis: cadmpeg_ir::math::Vector3 {
                x: -1.0,
                y: 0.0,
                z: 0.0
            },
            radius: 1.0,
            ..
        }
    ));
    let mut neutral_chart_payload = payload.clone();
    circle_row(&mut neutral_chart_payload, 22, 11, -5.0, false);
    let neutral_chart = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("VisibGeom", neutral_chart_payload)])),
            &DecodeOptions::default(),
        )
        .expect("neutral-chart decode");
    let neutral_circle = neutral_chart
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "creo:visibgeom:curve#22")
        .expect("circle with neutral sample chart");
    assert!(matches!(
        neutral_circle.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Circle {
            ref_direction: cadmpeg_ir::math::Vector3 {
                x: 0.0,
                y: 0.0,
                z: 1.0
            },
            ..
        }
    ));

    circle_row(&mut payload, 20, 11, 2.0, true);
    circle_row(&mut payload, 21, 12, -2.0, true);
    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("VisibGeom", payload)])),
            &DecodeOptions::default(),
        )
        .expect("decode");
    let cap_pairs =
        &result.ir().native.namespace("creo").unwrap().arenas["fc05_cylinder_cap_pairs"];
    assert_eq!(cap_pairs.len(), 1);
    assert_eq!(cap_pairs[0].fields()["surface_id"], 10);
    assert_eq!(cap_pairs[0].fields()["curve_ids"][0], 20);
    assert_eq!(cap_pairs[0].fields()["curve_ids"][1], 21);
    assert_eq!(cap_pairs[0].fields()["radius_mm"], 1.0);
    let cylinder = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:visibgeom:surface#10")
        .expect("placed cylinder");
    assert_eq!(
        cylinder.geometry,
        cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
            origin: cadmpeg_ir::math::Point3::new(2.0, 5.0, 3.0),
            axis: cadmpeg_ir::math::Vector3::new(-1.0, 0.0, 0.0),
            ref_direction: cadmpeg_ir::math::Vector3::new(0.0, (-2.0_f64).sin(), (-2.0_f64).cos(),),
            radius: 1.0,
        }
    );
    assert_eq!(result.ir().model.curves.len(), 2);
    assert!(result.ir().model.curves.iter().all(|curve| matches!(
        curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Circle {
            axis: cadmpeg_ir::math::Vector3 {
                x: -1.0,
                y: 0.0,
                z: 0.0
            },
            radius: 1.0,
            ..
        }
    )));
}
