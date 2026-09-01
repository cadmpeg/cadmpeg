// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions};
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::sketches::{SketchConstraintDefinition, SketchEntityId};
use cadmpeg_ir::Exactness;

use crate::container::{self, role, Layout};
use crate::surface::TorusRadius2Encoding;
use crate::test_support::*;
use crate::CreoCodec;

use super::*;

fn ieee8(value: f64) -> Vec<u8> {
    let mut raw = value.to_be_bytes();
    raw[0] = if value.is_sign_negative() { 0x2d } else { 0x46 };
    raw.to_vec()
}
#[test]
fn decodes_constant_outline_coordinate_as_a_model_plane() {
    let mut data = b"srf_array\0\xf8\x01".to_vec();
    data.extend([4, 0x22, 1, 1, 1, 0]);
    data.extend([0x0f; 4]);
    data.extend(ieee8(2.0));
    data.push(0x0f);
    data.extend(ieee8(3.0));
    data.extend(ieee8(-2.0));
    data.push(0x0f);
    data.extend(ieee8(-3.0));
    assert_eq!(
        planes(&data),
        vec![DatumPlane {
            id: 4,
            feature_id: 1,
            normal: [0.0, 1.0, 0.0],
            offset: 0.0,
            corners: [
                [Some(2.0), Some(0.0), Some(3.0)],
                [Some(-2.0), Some(0.0), Some(-3.0)]
            ],
            offset_in_payload: 12
        }]
    );
}

#[test]
fn withholds_positional_outline_with_multiple_held_coordinates() {
    let mut data = b"srf_array\0\xf8\x01".to_vec();
    data.extend([4, 0x22, 1, 1, 1, 0]);
    data.extend([0x0f; 4]);
    data.extend(ieee8(2.0));
    data.extend(ieee8(3.0));
    data.extend(ieee8(4.0));
    data.extend(ieee8(-2.0));
    data.extend(ieee8(3.0));
    data.extend(ieee8(4.0));

    assert!(planes(&data).is_empty());
}

#[test]
fn decodes_named_standard_plane_from_zero_slots() {
    let data = b"\xe0\x01geom_id\0\x02\xe0\x01feat_id\0\x01outline\0\xf9\x02\x03\x18\x46\x08\0\0\0\0\0\0\x46\x08\0\0\0\0\0\0\x18\x46\x08\0\0\0\0\0\0\x46\x08\0\0\0\0\0\0";
    let plane = named_plane(data).expect("required invariant");
    assert_eq!(plane.id, 2);
    assert_eq!(plane.feature_id, 1);
    assert_eq!(plane.normal, [1.0, 0.0, 0.0]);
    assert_eq!(plane.offset, 0.0);
    assert_eq!(
        plane.corners,
        [
            [Some(0.0), Some(3.0), Some(3.0)],
            [Some(0.0), Some(3.0), Some(3.0)]
        ]
    );
}

#[test]
fn withholds_named_plane_with_competing_standalone_zero_axes() {
    let data = b"\xe0\x01geom_id\0\x02\xe0\x01feat_id\0\x01outline\0\xf9\x02\x03\x18\x46\x08\0\0\0\0\0\0\x18\x18\x46\x08\0\0\0\0\0\0\x18";

    assert!(named_plane(data).is_none());
}

#[test]
fn named_outline_41_form_occupies_eight_bytes() {
    let data = b"\xe0\x01geom_id\0\x02\xe0\x01feat_id\0\x01outline\0\xf9\x02\x03\x18\x41\xba\x13\x99\xa9\xb3\xd8\x74\x41\x94\xad\x7e\x6a\xb0\x34\x5e\x18\x93\x29\x5a\xfc\xd5\x60\x69\x8c\x40\x79\xe9\x12\xa5\x83";
    let plane = named_plane(data).expect("named plane");
    assert_eq!(plane.normal, [1.0, 0.0, 0.0]);
    assert_eq!(plane.corners[0][0], Some(0.0));
    assert_eq!(plane.corners[1][0], Some(0.0));
}

#[test]
fn positional_outline_decodes_shared_named_coordinate_tokens() {
    let a5 = [0xa5, 1, 2, 3, 4, 5, 6];
    let nine_f = [0x9f, 7, 8, 9, 10, 11, 12];
    let mut data = b"srf_array\0\xf8\x01".to_vec();
    data.extend([4, 0x22, 3, 1, 1, 0]);
    data.extend(a5);
    data.extend(ieee8(2.0));
    data.extend(nine_f);
    data.extend(ieee8(-2.0));
    data.extend(ieee8(3.0));
    data.push(0x18);
    data.extend(a5);
    data.extend(ieee8(-3.0));
    data.push(0x18);
    data.extend(nine_f);

    let positional = &planes(&data)[0];
    assert_eq!(positional.normal, [0.0, 1.0, 0.0]);
    assert_eq!(positional.offset, 0.0);

    let mut named = b"\xe0\x01geom_id\0\x04\xe0\x01feat_id\0\x03outline\0\xf9\x02\x03".to_vec();
    named.extend(ieee8(3.0));
    named.push(0x18);
    named.extend(a5);
    named.extend(ieee8(-3.0));
    named.push(0x18);
    named.extend(nine_f);
    assert_eq!(positional.corners, named_plane(&named).unwrap().corners);
}

#[test]
fn positional_outline_uses_the_bounded_model_coordinate_lane() {
    let negative_x = [0xbb, 1, 2, 3, 4, 5, 6];
    let positive_x = [0x73, 1, 2, 3, 4, 5, 6];
    let lower_z = [0x41, 7, 8, 9, 10, 11, 12, 13];
    let upper_z = [0x8c, 14, 15, 16, 17, 18, 19];
    let mut data = b"srf_array\0\xf8\x01".to_vec();
    data.extend([4, 0x22, 3, 1, 1, 0]);
    data.extend([0x0f; 4]);
    data.extend(negative_x);
    data.push(0x18);
    data.extend(lower_z);
    data.extend(positive_x);
    data.push(0x18);
    data.extend(upper_z);

    let positional = &planes(&data)[0];
    assert_eq!(positional.normal, [0.0, 1.0, 0.0]);
    assert_eq!(positional.offset, 0.0);
    assert_eq!(
        positional.corners,
        [
            [
                Some(f64::from_be_bytes([0xbf, 0xe8, 1, 2, 3, 4, 5, 6])),
                Some(0.0),
                Some(f64::from_be_bytes([0x3f, 7, 8, 9, 10, 11, 12, 13])),
            ],
            [
                Some(f64::from_be_bytes([0x3f, 0xe8, 1, 2, 3, 4, 5, 6])),
                Some(0.0),
                Some(f64::from_be_bytes([0x40, 0x01, 14, 15, 16, 17, 18, 19])),
            ],
        ]
    );
}

#[test]
fn positional_outline_retains_unbacked_coordinate_tokens_without_values() {
    let mut data = b"srf_array\0\xf8\x01".to_vec();
    data.extend([4, 0x22, 3, 1, 1, 0]);
    data.extend([0x0f; 4]);
    data.extend([0x45, 0, 0, 0, 0, 0, 0]);
    data.push(0x18);
    data.extend([0x5c, 0, 0, 0, 0, 0, 0]);
    data.extend([0x5c, 0, 0, 0, 0, 0, 0]);
    data.push(0x18);
    data.extend([0x45, 0, 0, 0, 0, 0, 0]);

    let plane = &planes(&data)[0];
    assert_eq!(plane.normal, [0.0, 1.0, 0.0]);
    assert_eq!(plane.offset, 0.0);
    assert_eq!(
        plane.corners,
        [[None, Some(0.0), None], [None, Some(0.0), None]]
    );
}

#[test]
fn ignores_plane_shaped_bytes_outside_a_counted_surface_array() {
    let mut data = vec![4, 0x22, 1, 1, 1, 0];
    data.extend([0x0f; 4]);
    data.extend(ieee8(2.0));
    data.push(0x0f);
    data.extend(ieee8(3.0));
    data.extend(ieee8(-2.0));
    data.push(0x0f);
    data.extend(ieee8(-3.0));

    assert!(planes(&data).is_empty());
}

#[test]
fn decodes_compact_width_datum_row_identifiers() {
    let mut data = b"srf_array\0\xf8\x01".to_vec();
    data.extend([0x80, 0x80, 0x22, 0x81, 0x01, 1, 1, 0]);
    data.extend([0x0f; 4]);
    data.extend(ieee8(2.0));
    data.push(0x0f);
    data.extend(ieee8(3.0));
    data.extend(ieee8(-2.0));
    data.push(0x0f);
    data.extend(ieee8(-3.0));

    let decoded = planes(&data);
    assert_eq!(decoded.len(), 1);
    let plane = &decoded[0];
    assert_eq!(plane.id, 128);
    assert_eq!(plane.feature_id, 257);
}

#[test]
fn decodes_compact_width_named_datum_identifiers() {
    let data = b"\xe0\x01geom_id\0\x80\x80\xe0\x01feat_id\0\x81\x01outline\0\xf9\x02\x03\x18\x46\x08\0\0\0\0\0\0\x46\x08\0\0\0\0\0\0\x18\x46\x08\0\0\0\0\0\0\x46\x08\0\0\0\0\0\0";
    let plane = named_plane(data).expect("compact-width named plane");

    assert_eq!(plane.id, 128);
    assert_eq!(plane.feature_id, 257);
    assert_eq!(plane.normal, [1.0, 0.0, 0.0]);
}

#[test]
fn bounds_a_datum_outline_at_the_next_validated_row() {
    let mut data = b"srf_array\0\xf8\x02".to_vec();
    data.extend([4, 0x22, 1, 1, 1, 0]);
    data.extend([0x0f; 4]);
    data.extend([8, 0x22, 2, 1, 1, 0]);
    data.extend([0x0f; 4]);
    data.extend(ieee8(2.0));
    data.push(0x0f);
    data.extend(ieee8(3.0));
    data.extend(ieee8(-2.0));
    data.push(0x0f);
    data.extend(ieee8(-3.0));

    let decoded = planes(&data);
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].id, 8);
    assert_eq!(decoded[0].feature_id, 2);
}

#[test]
fn bounds_a_datum_outline_at_the_end_of_its_surface_array_frame() {
    let mut data = b"srf_array\0\xf8\x01".to_vec();
    data.extend([4, 0x22, 1, 1, 1, 0]);
    data.extend([0x0f; 4]);
    data.extend(b"srf_array\0\xf8\x01");
    data.extend([8, 0x22, 2, 1, 1, 0]);
    data.extend([0x0f; 4]);
    data.extend(ieee8(2.0));
    data.push(0x0f);
    data.extend(ieee8(3.0));
    data.extend(ieee8(-2.0));
    data.push(0x0f);
    data.extend(ieee8(-3.0));

    let decoded = planes(&data);
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].id, 8);
    assert_eq!(decoded[0].feature_id, 2);
}

#[test]
fn rejects_linked_plane_row_as_a_datum() {
    let mut data = b"srf_array\0\xf8\x01".to_vec();
    data.extend([4, 0x22, 1, 1, 1, 7]);
    data.extend([0x0f; 4]);
    data.extend(ieee8(2.0));
    data.push(0x0f);
    data.extend(ieee8(3.0));
    data.extend(ieee8(-2.0));
    data.push(0x0f);
    data.extend(ieee8(-3.0));

    assert!(planes(&data).is_empty());
}

#[test]
fn named_outline_resolves_a_cache_indexed_nonzero_offset() {
    let cached = ieee8(2.5);
    let mut data = b"\xe0\x01geom_id\0\x02\xe0\x01feat_id\0\x01".to_vec();
    data.extend(&cached);
    data.extend(b"outline\0\xf9\x02\x03");
    data.extend([0x18, 0x00]);
    data.extend(ieee8(-3.0));
    data.extend(ieee8(-4.0));
    data.extend([0x18, 0x00]);
    data.extend(ieee8(3.0));
    data.extend(ieee8(4.0));

    let plane = named_plane(&data).expect("cache-indexed named plane");
    assert_eq!(plane.normal, [1.0, 0.0, 0.0]);
    assert_eq!(plane.offset, 2.5);
    assert_eq!(plane.corners[0], [Some(2.5), Some(-3.0), Some(-4.0)]);
    assert_eq!(plane.corners[1], [Some(2.5), Some(3.0), Some(4.0)]);
}

#[test]
fn named_outline_decodes_backed_dictionary_coordinate_forms() {
    let mut data = b"\xe0\x01geom_id\0\x02\xe0\x01feat_id\0\x01outline\0\xf9\x02\x03".to_vec();
    data.extend([0x18]);
    data.extend([0x9f, 0, 0, 0, 0, 0, 0]);
    data.extend([0xa5, 0, 0, 0, 0, 0, 0]);
    data.extend([0x18]);
    data.extend([0xa5, 0, 0, 0, 0, 0, 0]);
    data.extend([0x9f, 0, 0, 0, 0, 0, 0]);

    let plane = named_plane(&data).expect("named plane");
    assert_eq!(plane.normal, [1.0, 0.0, 0.0]);
    assert_eq!(plane.offset, 0.0);
    assert_eq!(
        plane.corners,
        [
            [
                Some(0.0),
                Some(f64::from_be_bytes([0x40, 0x14, 0, 0, 0, 0, 0, 0])),
                Some(f64::from_be_bytes([0xbf, 0xd0, 0, 0, 0, 0, 0, 0]))
            ],
            [
                Some(0.0),
                Some(f64::from_be_bytes([0xbf, 0xd0, 0, 0, 0, 0, 0, 0])),
                Some(f64::from_be_bytes([0x40, 0x14, 0, 0, 0, 0, 0, 0]))
            ]
        ]
    );
}

#[test]
fn named_outline_retains_unbacked_coordinate_tokens_without_values() {
    let mut data = b"\xe0\x01geom_id\0\x02\xe0\x01feat_id\0\x01outline\0\xf9\x02\x03".to_vec();
    data.extend([0x18]);
    data.extend([0x5c, 0, 0, 0, 0, 0, 0]);
    data.extend([0x45, 0, 0, 0, 0, 0, 0]);
    data.extend([0x18]);
    data.extend([0x45, 0, 0, 0, 0, 0, 0]);
    data.extend([0x5c, 0, 0, 0, 0, 0, 0]);

    let plane = named_plane(&data).expect("zero-axis named plane");
    assert_eq!(plane.normal, [1.0, 0.0, 0.0]);
    assert_eq!(plane.offset, 0.0);
    assert_eq!(
        plane.corners,
        [[Some(0.0), None, None], [Some(0.0), None, None],]
    );
}

#[test]
fn scan_discovers_model_space_datum_planes() {
    let mut datum = b"srf_array\0\xf8\x01".to_vec();
    datum.extend([4, 0x22, 1, 1, 1, 0]);
    datum.extend([0x0f; 4]);
    for value in [2.0_f64, 0.0, 3.0, -2.0, 0.0, -3.0] {
        if value == 0.0 {
            datum.push(0x0f);
        } else {
            let mut bytes = value.to_be_bytes();
            bytes[0] = if value.is_sign_negative() { 0x2d } else { 0x46 };
            datum.extend(bytes);
        }
    }
    let scan = container::scan_bytes(build_prt("c", &[("ActDatums", datum)]));
    assert_eq!(scan.planes.datums.len(), 1);
    assert_eq!(scan.planes.datums[0].normal, [0.0, 1.0, 0.0]);
}

#[test]
fn scan_discovers_complete_active_datum_cylinder_carrier() {
    let mut datum = b"srf_array\0\xf8\x01".to_vec();
    datum.extend([8, 0x24, 3, 1, 1, 0]);
    datum.extend([
        0x14, 0x2f, 0x10, 0x00, 0x2d, 0x1f, 0x6a, 0x7a, 0x29, 0x55, 0x38, 0x5e, 0x2f, 0x43, 0x00,
        0x48, 0x29, 0x00, 0x2f, 0x10, 0x00, 0x43, 0xe8, 0x00, 0x48, 0x27, 0x80, 0x2f, 0x43, 0x00,
        0x2a, 0xe8, 0x00,
    ]);
    let scan = container::scan_bytes(build_prt("c", &[("ActDatums", datum)]));
    assert_eq!(scan.planes.datum_cylinders.len(), 1);
    let cylinder = scan.planes.datum_cylinders[0];
    assert_eq!(cylinder.id, 8);
    assert_eq!(cylinder.feature_id, 3);
    assert!(!cylinder.reversed);
    assert_eq!(cylinder.frame.origin, [-12.5, 4.0, 0.0]);
    assert_eq!(cylinder.frame.axis, [0.0, 1.0, 0.0]);
    assert_eq!(cylinder.frame.ref_direction, [1.0, 0.0, 0.0]);
    assert_eq!(cylinder.frame.radius, 0.75);
    assert_eq!(cylinder.frame.length, Some(34.0));
}

#[test]
fn active_datum_cylinder_envelope_decodes_direct_and_split_forms() {
    let cases = [
        (
            8_u32,
            false,
            0,
            vec![0xc0; 10],
            [8.0, -13.0, -4.0, 0.0, -11.0, 4.0, 1.0],
            [-12.0, 4.0, 0.0],
            [0.0, -1.0, 0.0],
            [1.0, 0.0, 0.0],
            1.0,
            8.0,
        ),
        (
            10_u32,
            false,
            1,
            {
                let mut prefix = vec![0xc0; 2];
                prefix.extend(ieee8(-8.0));
                prefix.push(0xc1);
                prefix
            },
            [0.0, 11.0, -4.0, 0.0, 13.0, 4.0, 1.0],
            [12.0, 4.0, 0.0],
            [0.0, -1.0, 0.0],
            [-1.0, 0.0, 0.0],
            1.0,
            8.0,
        ),
        (
            12_u32,
            true,
            1,
            {
                let mut prefix = vec![0xc0; 2];
                prefix.extend([0x0f, 0xe4]);
                prefix.push(0xc1);
                prefix
            },
            [6.0, -2.0, 19.0, -3.0, 2.0, 21.0, 3.0],
            [0.0, 19.0, 3.0],
            [0.0, 0.0, -1.0],
            [-1.0, 0.0, 0.0],
            2.0,
            6.0,
        ),
    ];
    for (
        id,
        reversed,
        boundary_type,
        mut prefix,
        values,
        origin,
        axis,
        ref_direction,
        radius,
        length,
    ) in cases
    {
        let mut data = b"srf_array\0\xf8\x01".to_vec();
        data.extend([
            id as u8,
            0x24,
            3,
            if reversed { 0xf6 } else { 1 },
            boundary_type,
            0,
        ]);
        for value in values {
            if value == 0.0 {
                prefix.push(0x0f);
            } else if value == 1.0 {
                prefix.push(0xe4);
            } else {
                prefix.extend(ieee8(value));
            }
        }
        data.extend(prefix);
        let decoded = cylinders(&data);
        let [cylinder] = decoded.as_slice() else {
            panic!("one complete active-datum cylinder expected");
        };
        assert_eq!(cylinder.id, id);
        assert_eq!(cylinder.reversed, reversed);
        assert_eq!(cylinder.frame.origin, origin);
        assert_eq!(cylinder.frame.axis, axis);
        assert_eq!(cylinder.frame.ref_direction, ref_direction);
        assert_eq!(cylinder.frame.radius, radius);
        assert_eq!(cylinder.frame.length, Some(length));
    }
}

#[test]
fn decode_transfers_active_datum_cylinder_with_source_namespace() {
    let mut datum = b"srf_array\0\xf8\x01".to_vec();
    datum.extend([8, 0x24, 3, 1, 1, 0]);
    datum.extend([
        0x14, 0x2f, 0x10, 0x00, 0x2d, 0x1f, 0x6a, 0x7a, 0x29, 0x55, 0x38, 0x5e, 0x2f, 0x43, 0x00,
        0x48, 0x29, 0x00, 0x2f, 0x10, 0x00, 0x43, 0xe8, 0x00, 0x48, 0x27, 0x80, 0x2f, 0x43, 0x00,
        0x2a, 0xe8, 0x00,
    ]);
    let result = CreoCodec
        .decode(
            &mut Cursor::new(build_prt("c", &[("ActDatums", datum)])),
            &DecodeOptions::default(),
        )
        .expect("decode");
    let surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id.as_str() == "creo:actdatums:surface#8")
        .expect("active datum cylinder surface");
    assert!(matches!(
        surface.geometry,
        SurfaceGeometry::Cylinder { radius, .. } if (radius - 0.75).abs() < 1.0e-12
    ));
    assert_eq!(
        surface
            .source_object
            .as_ref()
            .expect("source association")
            .object_id,
        "ActDatums:8"
    );
    let cylinders = &result.ir().native.namespace("creo").unwrap().arenas["datum_cylinders"];
    assert_eq!(cylinders.len(), 1);
    assert_eq!(cylinders[0].fields()["datum_id"], 8);
    assert_eq!(cylinders[0].fields()["radius"], 0.75);
    assert_eq!(
        result.report().coverage["transferred_active_datum_cylinder_count"],
        1
    );
}

#[test]
fn decode_transfers_exact_datum_plane_carrier() {
    let mut datum = b"srf_array\0\xf8\x01".to_vec();
    datum.extend([4, 0x22, 1, 1, 1, 0]);
    datum.extend([0x0f; 4]);
    for value in [2.0_f64, 0.0, 3.0, -2.0, 0.0, -3.0] {
        if value == 0.0 {
            datum.push(0x0f);
        } else {
            let mut bytes = value.to_be_bytes();
            bytes[0] = if value.is_sign_negative() { 0x2d } else { 0x46 };
            datum.extend(bytes);
        }
    }
    let mut reader = Cursor::new(build_prt("c", &[("ActDatums", datum)]));
    let result = CreoCodec
        .decode(&mut reader, &DecodeOptions::default())
        .unwrap();
    assert!(result.report().geometry_transferred());
    let records = &result.ir().native.namespace("creo").unwrap().arenas["datum_planes"];
    assert_eq!(records[0].fields()["datum_id"], 4);
    assert_eq!(records[0].fields()["owner_feature_id"], 1);
    assert_eq!(records[0].fields()["normal"][1], 1.0);
    assert_eq!(records[0].fields()["plane_offset"], 0.0);
    assert_eq!(result.ir().model.surfaces.len(), 1);
    assert_eq!(result.ir().model.features.len(), 1);
    let feature = &result.ir().model.features[0];
    assert_eq!(feature.id.as_str(), "creo:model:feature#1");
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::DatumPlane { .. }
    ));
}

#[test]
fn decode_merges_datum_geometry_and_operation_history_by_feature_id() {
    let mut datum = b"srf_array\0\xf8\x01".to_vec();
    datum.extend([4, 0x22, 4, 1, 1, 0]);
    datum.extend([0x0f; 4]);
    for value in [2.0_f64, 0.0, 3.0, -2.0, 0.0, -3.0] {
        if value == 0.0 {
            datum.push(0x0f);
        } else {
            let mut bytes = value.to_be_bytes();
            bytes[0] = if value.is_sign_negative() { 0x2d } else { 0x46 };
            datum.extend(bytes);
        }
    }
    let data = build_prt(
        "c",
        &[
            ("ActDatums", datum),
            ("MdlStatus", b"Round id 3\0Datum Plane id 4\0".to_vec()),
        ],
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    assert_eq!(result.ir().model.features.len(), 2);
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("datum feature");
    assert_eq!(feature.id.as_str(), "creo:model:feature#4");
    assert_eq!(feature.ordinal, 1);
    assert_eq!(feature.name.as_deref(), Some("Datum Plane id 4"));
    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::DatumPlane { .. }
    ));
    assert_eq!(
        result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.id.as_str() == "creo:model:feature#3")
            .expect("preceding round")
            .ordinal,
        0
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_withholds_competing_standalone_datum_planes() {
    let mut row = vec![4, 0x22, 4, 1, 1, 0];
    row.extend([0x0f; 4]);
    for value in [2.0_f64, 0.0, 3.0, -2.0, 0.0, -3.0] {
        if value == 0.0 {
            row.push(0x0f);
        } else {
            let mut bytes = value.to_be_bytes();
            bytes[0] = if value.is_sign_negative() { 0x2d } else { 0x46 };
            row.extend(bytes);
        }
    }
    let mut datum = b"srf_array\0\xf8\x02".to_vec();
    datum.extend(row.clone());
    datum.extend(row);
    let data = build_prt("c", &[("ActDatums", datum)]);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    assert!(result.ir().model.features.is_empty());
    assert_eq!(
        result
            .ir()
            .native
            .namespace("creo")
            .unwrap()
            .arenas
            .get("datum_planes")
            .map_or(0, Vec::len),
        0
    );
}

#[test]
fn decode_retains_named_datum_plane_with_unresolved_placement() {
    let data = build_prt("c", &[("MdlStatus", b"Datum Plane id 4\0".to_vec())]);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let feature = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("named datum feature");

    assert!(matches!(
        feature.definition,
        cadmpeg_ir::features::FeatureDefinition::DatumPlaneUnresolved
    ));
}
