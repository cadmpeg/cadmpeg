// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::Exactness;

use crate::container::{self};
use crate::test_support::*;
use crate::CreoCodec;

use super::*;

#[test]
fn decodes_complete_positional_line_rows() {
    let payload = b"ent_list(line)\0\xe0\x02end1\0\xf8\x03\x18\xdf\x1d\x84\xe8\xb0\xed\x7b\x46\x19\x87\x25\xdc\x17\x53\xfa\
            \xe0\x00entity(line)\0\xf1\xe3\xf7\x11\xf6\xe2\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\x02\
            \x18\xdf\x1d\x84\xe8\xb0\xed\x7b\x2d\x19\x87\x25\xdc\x17\x53\xfa\
            \x18\x2d\x43\x23\xb0\x9d\x16\x1d\xaf\x2d\x19\x87\x25\xdc\x17\x53\xfa\xe3\
            \xe0\x00entity(text)\0";
    let decoded = lines(payload);
    let [line] = decoded.as_slice() else {
        panic!("one line");
    };
    assert_eq!(line.start[0], 0.0);
    assert_eq!(line.end[0], 0.0);
    assert_ne!(line.start, line.end);
}

#[test]
fn decodes_named_conic_fields_without_classifying_the_conic() {
    let local_body = b"\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6";
    assert!(conic_local_system(local_body, &ScalarCache::from_section(local_body)).is_some());
    let payload = b"ent_list(conic)\0\
            \xe0\x01id\0\x2a\xe0\x01type\0\x1e\
            \xe0\x00gen_info\0\xe2\xf7\x13\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\
            \xe0\x01flip\0\x01\
            \xe0\x02end1\0\xf8\x03\xe4\x0f\x0f\
            \xe0\x02end2\0\xf8\x03\x43\xf0\x00\x0f\x0f\
            \xe0\x02t0\0\x0f\xe0\x02t1\0\x11\
            \xe0\x02c1\0\x43\xf0\x00\xe0\x02c2\0\xe4\
            \xe0\x02local_sys\0\xf9\x04\x03\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6\
            \xf2\xf7\x0e\xe3";

    let decoded = named_conics(payload);
    let [conic] = decoded.as_slice() else {
        panic!("one conic");
    };
    assert_eq!(conic.entity_id, 42);
    assert_eq!(conic.type_id, 30);
    assert_eq!(conic.flip, 1);
    assert_eq!(conic.start, [1.0, 0.0, 0.0]);
    assert_eq!(conic.end, [-1.0, 0.0, 0.0]);
    assert_eq!(conic.parameter_start, Some(0.0));
    assert_eq!(conic.parameter_end, Some(std::f64::consts::PI));
    assert_eq!([conic.coefficient_1, conic.coefficient_2], [-1.0, 1.0]);
    assert_eq!(
        conic.local_system,
        Some([0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0])
    );
}

#[test]
fn named_conic_withholds_duplicate_optional_parameter_fields() {
    let payload = b"ent_list(conic)\0\
            \xe0\x01id\0\x2a\xe0\x01type\0\x1e\xe0\x01flip\0\x01\
            \xe0\x02end1\0\xf8\x03\xe4\x0f\x0f\
            \xe0\x02end2\0\xf8\x03\x43\xf0\x00\x0f\x0f\
            \xe0\x02t0\0\x0f\xe0\x02t0\0\x0f\
            \xe0\x02c1\0\x43\xf0\x00\xe0\x02c2\0\xe4\
            \xe0\x02local_sys\0\xf9\x04\x03\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6\
            \xf2\xf7\x0e\xe3";

    assert!(named_conics(payload).is_empty());
}

#[test]
fn named_conic_opposite_parameter_requires_a_start_parameter() {
    let payload = b"ent_list(conic)\0\
            \xe0\x01id\0\x2a\xe0\x01type\0\x1e\xe0\x01flip\0\x01\
            \xe0\x02end1\0\xf8\x03\xe4\x0f\x0f\
            \xe0\x02end2\0\xf8\x03\x43\xf0\x00\x0f\x0f\
            \xe0\x02t1\0\x11\xe0\x02c1\0\x43\xf0\x00\xe0\x02c2\0\xe4\
            \xe0\x02local_sys\0\xf9\x04\x03\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6\
            \xf2\xf7\x0e\xe3";

    assert!(named_conics(payload).is_empty());
}

#[test]
fn named_conic_ignores_field_header_bytes_inside_an_ieee_coordinate() {
    let payload = b"ent_list(conic)\0\
            \xe0\x01id\0\x2a\xe0\x01type\0\x1e\xe0\x01flip\0\x01\
            \xe0\x02end1\0\xf8\x03\x32\xe0\x02c1\0\0\0\x0f\x0f\
            \xe0\x02end2\0\xf8\x03\x43\xf0\x00\x0f\x0f\
            \xe0\x02c1\0\x43\xf0\x00\xe0\x02c2\0\xe4\
            \xe0\x02local_sys\0\xf9\x04\x03\x18\xe4\x0f\xe4\x18\xe5\x0f\x18\xe6\
            \xf2\xf7\x0e\xe3";

    assert_eq!(named_conics(payload).len(), 1);
}

#[test]
fn named_conic_local_system_ignores_a_terminator_inside_an_ieee_coordinate() {
    let payload = b"ent_list(conic)\0\
            \xe0\x01id\0\x2a\xe0\x01type\0\x1e\xe0\x01flip\0\x01\
            \xe0\x02end1\0\xf8\x03\xe4\x0f\x0f\
            \xe0\x02end2\0\xf8\x03\x43\xf0\x00\x0f\x0f\
            \xe0\x02c1\0\x43\xf0\x00\xe0\x02c2\0\xe4\
            \xe0\x02local_sys\0\xf9\x04\x03\
            \x32\xf2\xf7\0\0\0\0\0\x0f\x0f\x0f\x0f\x0f\x0f\x0f\x0f\x0f\x0f\x0f\
            \xf2\xf7\x0e\xe3";

    let decoded = named_conics(payload);
    let [conic] = decoded.as_slice() else {
        panic!("one conic")
    };
    assert!(conic.local_system.is_some());
    assert!(conic.body.ends_with(&[0x0f; 11]));
}

#[test]
fn named_conic_withholds_an_ambiguous_local_system_boundary() {
    let payload = b"ent_list(conic)\0\
            \xe0\x01id\0\x2a\xe0\x01type\0\x1e\xe0\x01flip\0\x01\
            \xe0\x02end1\0\xf8\x03\xe4\x0f\x0f\
            \xe0\x02end2\0\xf8\x03\x43\xf0\x00\x0f\x0f\
            \xe0\x02c1\0\x43\xf0\x00\xe0\x02c2\0\xe4\
            \xe0\x02local_sys\0\xf9\x04\x03\xaa\xf2\xf7\xbb\xf2\xf7\x0e\xe3";

    assert!(named_conics(payload).is_empty());
}

#[test]
fn conic_frame_accepts_positive_seven_byte_origin_and_terminal_zero() {
    let body = [
        0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x4a, 0, 0, 0, 0, 0, 0, 0x0f, 0x18,
    ];

    assert_eq!(
        conic_local_system(&body, &ScalarCache::from_section(&body)),
        Some([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 0.0, 0.0])
    );
}

#[test]
fn decodes_positional_conic_with_an_opposite_endpoint_parameter() {
    let payload = b"ent_list(conic)\0\xf2\xf7\x0e\xe2\x2b\xe3\
            \x2b\x1e\xe2\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\x01\
            \xe4\x0f\x0f\x43\xf0\x00\x0f\x0f\x0f\x11\x43\xf0\x00\xe4\
            \xe4\x0f\x0f\x0f\xe4\x0f\x0f\x0f\xe4\x43\xf0\x00\x0f\x0f\
            \xe2\x2c\xf7\x10\xe3\xe0\x00ent_list(text)\0";

    let decoded = positional_conics(payload);
    let [conic] = decoded.as_slice() else {
        panic!("one positional conic");
    };
    assert_eq!(conic.entity_id, 43);
    assert_eq!(conic.type_id, 30);
    assert_eq!(conic.start, [1.0, 0.0, 0.0]);
    assert_eq!(conic.end, [-1.0, 0.0, 0.0]);
    assert_eq!(conic.parameter_start, Some(0.0));
    assert_eq!(conic.parameter_end, Some(std::f64::consts::PI));
    assert_eq!([conic.coefficient_1, conic.coefficient_2], [-1.0, 1.0]);
    assert_eq!(conic.local_system.expect("complete local system")[9], -1.0);
}

#[test]
fn positional_conic_local_system_requires_its_compound_boundary() {
    let frame = [0x0f; 12];
    let mut body = frame.to_vec();
    body.push(0xe2);
    body.extend([0x2c, 0xf7, 0x10, 0xe3]);

    assert_eq!(
        positional_conic_local_system(&body, 0, &ScalarCache::default()),
        Some((12, [0.0; 12]))
    );

    body[12] = 0xff;
    assert!(positional_conic_local_system(&body, 0, &ScalarCache::default()).is_none());
}

#[test]
fn positional_conic_withholds_non_finite_parameters() {
    let payload = b"ent_list(conic)\0\xf2\xf7\x0e\xe2\x2b\xe3\
            \x2b\x1e\xe2\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\x01\
            \xe4\x0f\x0f\x43\xf0\x00\x0f\x0f\
            \xed\x7f\xf8\x00\x00\x00\x00\x00\x00\x11\x43\xf0\x00\xe4\
            \xe4\x0f\x0f\x0f\xe4\x0f\x0f\x0f\xe4\x43\xf0\x00\x0f\x0f\
            \xe2\x2c\xf7\x10\xe3\xe0\x00ent_list(text)\0";

    assert!(positional_conics(payload).is_empty());
}

#[test]
fn opposite_endpoint_parameter_requires_a_decoded_start_parameter() {
    let body = [0x11];
    assert_eq!(
        conic_parameter(&body, 0, None, &ScalarCache::from_section(&body)),
        Some((None, 1))
    );
}

#[test]
fn derives_ellipse_from_orthonormal_frame_and_non_antipodal_endpoints() {
    let conic = ReferenceConic {
        entity_id: 7,
        type_id: 30,
        flip: 1,
        start: [-3.0, 2.0, 4.0],
        end: [2.0, 4.0, 4.0],
        parameter_start: None,
        parameter_end: None,
        coefficient_1: -5.0,
        coefficient_2: 2.0,
        local_system: Some([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 4.0]),
        body: Vec::new(),
        offset: 10,
    };

    assert_eq!(
        ellipse_carriers(std::slice::from_ref(&conic)),
        [ReferenceEllipse {
            source_entity_id: 7,
            center: [2.0, 2.0, 4.0],
            axis: [0.0, 0.0, 1.0],
            major_direction: [-1.0, 0.0, 0.0],
            major_radius: 5.0,
            minor_radius: 2.0,
            offset: 10,
        }]
    );

    let mut invalid = conic.clone();
    invalid
        .local_system
        .as_mut()
        .expect("complete local system")[3] = 1.0;
    assert!(ellipse_carriers(&[invalid]).is_empty());

    let diagonal = (100.0_f64 / 29.0).sqrt();
    let ambiguous = ReferenceConic {
        local_system: Some([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]),
        start: [diagonal, diagonal, 0.0],
        end: [-diagonal, -diagonal, 0.0],
        ..conic
    };
    assert!(ellipse_carriers(&[ambiguous]).is_empty());
}

#[test]
fn withholds_incomplete_coordinate_suffix() {
    let payload =
        b"ent_list(line)\0\xe0\x00entity(line)\0\xf6\xe2\x02\x18\xe3\xe0\x00entity(text)\0";
    assert!(lines(payload).is_empty());
}

#[test]
fn decodes_signed_coordinate_dictionary_line_rows() {
    let coordinates = b"\x18\x41\x93\x8a\x07\xa0\xe6\xf8\x55\x8c\x3e\x32\xfb\x7f\x13\x0b\
            \x18\x93\x27\x14\x0f\x41\xcd\xf1\x8c\x3e\x32\xfb\x7f\x13\x0b";
    assert!(scalar_suffix(coordinates, 6, &ScalarCache::from_section(coordinates)).is_some());
    let payload = b"ent_list(line)\0\xe0\x00entity(line)\0\xf1\xe3\xf7\x11\
            \xf6\xe2\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\x02\
            \x18\x41\x93\x8a\x07\xa0\xe6\xf8\x55\x8c\x3e\x32\xfb\x7f\x13\x0b\
            \x18\x93\x27\x14\x0f\x41\xcd\xf1\x8c\x3e\x32\xfb\x7f\x13\x0b\
            \xe0\x00entity(text)\0";
    assert_eq!(lines(payload).len(), 1);
}

#[test]
fn scalar_suffix_withholds_competing_start_offsets() {
    let first_only = [0x46, 0, 0, 0, 0, 0, 0, 0, 0xe4, 0xe4, 0xe4, 0xe4, 0xe4];
    assert!(scalar_suffix(&first_only, 6, &ScalarCache::from_section(&first_only)).is_some());
    let second_only = [0, 0x2c, 0, 0, 0, 0, 0, 0, 0xe4, 0xe4, 0xe4, 0xe4, 0xe4];
    assert!(scalar_suffix(&second_only, 6, &ScalarCache::from_section(&second_only)).is_some());
    let body = [0x46, 0x2c, 0, 0, 0, 0, 0, 0, 0xe4, 0xe4, 0xe4, 0xe4, 0xe4];
    assert!(scalar_suffix(&body, 6, &ScalarCache::from_section(&body)).is_none());
}

#[test]
fn decodes_line3d_with_matching_original_length() {
    let payload = b"ent_list(line3d)\0\x23\xe3\x23\x0d\xe2\x02\x48\x10\x00\
            \x0f\x0f\x0f\xe4\x0f\x0f\xe4";
    let decoded = line3d_lines(payload);
    let [line] = decoded.as_slice() else {
        panic!("one line3d");
    };
    assert_eq!(
        line.kind,
        ReferenceLineKind::Line3d {
            entity_id: 35,
            original_length: 1.0
        }
    );
    assert_eq!(line.start, [0.0; 3]);
    assert_eq!(line.end, [1.0, 0.0, 0.0]);
}

#[test]
fn line3d_row_uses_its_complete_block_bound() {
    let mut payload = b"ent_list(line3d)\0\x23\xe3\x23\x0d\xe2\x02\x48\x10\0\0".to_vec();
    payload.extend(std::iter::repeat_n(0, 385));
    payload.extend_from_slice(b"\x0f\x0f\x0f\xe4\x0f\x0f\xe4");

    assert_eq!(line3d_lines(&payload).len(), 1);
}

#[test]
fn decodes_line3d_with_positive_full_width_coordinates() {
    let payload = b"ent_list(line3d)\0\x23\xe3\x23\x0d\xe2\x02\x48\x10\x00\
            \x0f\x0f\x32\xb3\xa2\x70\xe5\xa0\x3f\xfa\
            \xe4\x0f\x32\xb3\xa2\x70\xe5\xa0\x3f\xfa\xe4";
    let decoded = line3d_lines(payload);
    let [line] = decoded.as_slice() else {
        panic!("one line3d");
    };
    assert_eq!(line.start[2], line.end[2]);
    assert_eq!(line.end[0] - line.start[0], 1.0);
}

#[test]
fn withholds_line3d_with_inconsistent_original_length() {
    let payload = b"ent_list(line3d)\0\x23\xe3\x23\x0d\xe2\x02\
            \x0f\x0f\x0f\xe4\x0f\x0f\x0e";
    assert!(line3d_lines(payload).is_empty());
}

#[test]
fn withholds_line3d_when_endpoint_norm_overflows() {
    let mut body = Vec::new();
    for value in [-f64::MAX, 0.0, 0.0, f64::MAX, 0.0, 0.0, f64::MAX] {
        body.push(0xed);
        body.extend_from_slice(&value.to_be_bytes());
    }
    assert!(line3d_fields(&body, &ScalarCache::from_section(&body)).is_none());
}

#[test]
fn line3d_withholds_competing_scalar_runs() {
    let body = b"\x0f\x0f\x0f\xe4\x0f\x0f\xe4\x0f\x0f\x0f\xe4\x0f\x0f\xe4";

    assert!(line3d_fields(body, &ScalarCache::from_section(body)).is_none());
}

#[test]
fn decodes_arc_z_diameter_rows() {
    let body = b"\x01\xe4\xe4\x0f\x0f\x43\xf0\x00\x0f\x0f";
    let circle = arc_z_fields(body, &ScalarCache::from_section(body), 7).expect("diameter row");
    assert_eq!(circle.entity_id, 7);
    assert_eq!(circle.center, [0.0; 3]);
    assert_eq!(circle.radius, 1.0);
    assert_eq!(circle.start, [1.0, 0.0, 0.0]);
    assert_eq!(circle.end, [-1.0, 0.0, 0.0]);
}

#[test]
fn decodes_arc_z_explicit_center_rows() {
    let body = b"\x01\x2f\x0c\x00\x2f\x24\x00\x48\x10\x00\
            \x2f\x00\x00\x2f\x16\x00\x2f\x24\x00\x48\x10\x00\
            \x2f\x0c\x00\x2f\x20\x00\x48\x10\x00";
    let circle = arc_z_fields(body, &ScalarCache::from_section(body), 8).expect("quarter arc");
    assert_eq!(circle.center, [3.5, 10.0, -4.0]);
    assert_eq!(circle.radius, 2.0);
    assert_eq!(circle.start, [5.5, 10.0, -4.0]);
    assert_eq!(circle.end, [3.5, 8.0, -4.0]);
}

#[test]
fn decodes_arc_z_positive_full_width_coordinate_rows() {
    let body = b"\x48\x3e\x00\x93\x3b\x57\xbb\x8a\x68\xf5\
            \x8c\x6e\x94\xe1\x50\xe8\xf6\x9a\x54\x2f\x35\xcd\x11\x56\
            \x48\x3e\x00\x2d\x19\x9e\xd7\x77\x97\xfd\xfc\
            \x9b\xa7\x3d\x24\xb6\x7b\x09\x48\x3e\x00\
            \x9f\x6b\xf0\x6f\x95\x50\xb9\xa0\xff\x43\xd5\xa5\xa5\x6c";
    let cache = ScalarCache::from_section(body);
    let circle = arc_z_fields(body, &cache, 9).expect("general arc");
    assert_eq!(circle.center[0], -30.0);
    assert_eq!(circle.start[0], -30.0);
    assert_eq!(circle.end[0], -30.0);
    assert!((circle.axis[0].abs() - 1.0).abs() < 1.0e-12);
}

#[test]
fn arc_z_rows_prefer_the_tabulated_first_coordinate_lane() {
    let center_x = [0x46, 0, 0, 0, 0, 0, 0, 0];
    let endpoint_x = [0xed, 0xc0, 0x08, 0, 0, 0, 0, 0, 0];
    let zero = [0x0f];
    let one = [0xe4];
    let mut body = Vec::new();
    body.extend_from_slice(&center_x);
    body.extend_from_slice(&zero);
    body.extend_from_slice(&zero);
    body.extend_from_slice(&one);
    body.extend_from_slice(&endpoint_x);
    body.extend_from_slice(&zero);
    body.extend_from_slice(&zero);
    body.extend_from_slice(&center_x);
    body.extend_from_slice(&one);
    body.extend_from_slice(&zero);

    let circle = arc_z_fields(&body, &ScalarCache::from_section(&body), 10)
        .expect("tabulated-cylinder first-coordinate lane circle");
    assert_eq!(circle.center, [-2.0, 0.0, 0.0]);
    assert_eq!(circle.start, [-3.0, 0.0, 0.0]);
    assert_eq!(circle.end, [-2.0, 1.0, 0.0]);
    assert_eq!(circle.axis, [0.0, 0.0, -1.0]);

    let negative_collision = [0x2d, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(
        arc_z_coordinate(&negative_collision, 0, &ScalarCache::default()),
        Some((2.0, 8))
    );
}

#[test]
fn arc_z_withholds_competing_diameter_runs() {
    let body = b"\xe4\xe4\x0f\x0f\x43\xf0\x00\x0f\x0f\xe4\xe4\x0f\x0f\x43\xf0\x00\x0f\x0f";

    assert!(arc_z_fields(body, &ScalarCache::from_section(body), 7).is_none());
}

#[test]
fn decode_transfers_equation_verified_model_reference_circles() {
    let payload = b"ent_list(arc_z)\0\xe2\x2d\xe3\x2d\x0f\xe2\x01\
        \xe4\xe4\x0f\x0f\x43\xf0\x00\x0f\x0f\xe0\x00ent_list(line3d)\0"
        .to_vec();
    let data = build_prt("c", &[("MdlRefInfo", payload)]);
    let scan = container::scan_bytes(data.clone());
    assert_eq!(scan.references.circles.len(), 1);
    assert_eq!(scan.references.circles[0].center, [0.0; 3]);
    assert_eq!(scan.references.circles[0].radius, 1.0);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    assert!(result.ir().model.curves.iter().any(|curve| matches!(
        curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Circle { radius: 1.0, .. }
    )));
    let circle = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "creo:mdl_ref_info:arc_z#45")
        .expect("canonically identified arc_z");
    assert_eq!(
        circle.source_object.as_ref().unwrap().object_id,
        "MdlRefInfo:arc_z:45"
    );
    let record = &result.ir().native.namespace("creo").unwrap().arenas["reference_circles"][0];
    assert_eq!(record.fields()["entity_id"], 45);
    assert_eq!(record.fields()["center_source"], "endpoint_midpoint");
    assert_annotation(
        &result.source_fidelity().annotations,
        record.id(),
        "creo:MdlRefInfo",
        scan.references.circles[0].offset as u64,
        "reference_circle_record",
        Exactness::Derived,
    );
}

#[test]
fn decode_retains_line3d_original_length() {
    let payload = b"ent_list(line3d)\0\x23\xe3\x23\x0d\xe2\x02\x48\x10\x00\
        \x0f\x0f\x0f\xe4\x0f\x0f\xe4"
        .to_vec();
    let data = build_prt("c", &[("MdlRefInfo", payload)]);
    let scan = container::scan_bytes(data.clone());
    let [line] = scan.references.lines.as_slice() else {
        panic!("one line3d");
    };
    assert_eq!(
        line.kind,
        crate::reference::ReferenceLineKind::Line3d {
            entity_id: 35,
            original_length: 1.0
        }
    );

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let record = &result.ir().native.namespace("creo").unwrap().arenas["reference_lines"][0];
    assert_eq!(record.fields()["family"], "line3d");
    assert_eq!(record.fields()["entity_id"], 35);
    assert_eq!(record.fields()["original_length"], 1.0);
    let curve = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "creo:mdl_ref_info:line3d#35")
        .expect("canonically identified line3d");
    assert_eq!(
        curve.source_object.as_ref().unwrap().object_id,
        "MdlRefInfo:line3d:35"
    );
}

#[test]
fn decode_disambiguates_repeated_line3d_entity_ids() {
    let payload = b"ent_list(line3d)\0\x23\xe3\x23\x0d\xe2\x02\x48\x10\x00\
        \x0f\x0f\x0f\xe4\x0f\x0f\xe4\
        \x23\xe3\x23\x0d\xe2\x02\x48\x10\x00\
        \x0f\x0f\x0f\x43\xf0\x00\x0f\x0f\xe4"
        .to_vec();
    let data = build_prt("c", &[("MdlRefInfo", payload)]);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let ids = result
        .ir()
        .model
        .curves
        .iter()
        .filter(|curve| {
            curve
                .id
                .as_str()
                .starts_with("creo:mdl_ref_info:line3d#35@")
        })
        .map(|curve| curve.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
}

#[test]
fn decode_reports_and_retains_invariant_complete_reference_ellipses() {
    let payload = b"ent_list(conic)\0\xf2\xf7\x0e\xe2\x2b\xe3\
        \x2b\x1e\xe2\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\x01\
        \xe4\x0f\x0f\x43\xf0\x00\x0f\x0f\x0f\xe4\x43\xf0\x00\xe4\
        \xe4\x0f\x0f\x0f\xe4\x0f\x0f\x0f\xe4\x0f\x0f\x0f\
        \xe2\x2c\xf7\x10\xe3\xe0\x00ent_list(text)\0"
        .to_vec();
    let data = build_prt("c", &[("MdlRefInfo", payload)]);
    let scan = container::scan_bytes(data.clone());
    assert_eq!(scan.references.conics.len(), 1);
    assert_eq!(scan.references.ellipses.len(), 1);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    assert!(result.ir().model.curves.iter().any(|curve| matches!(
        curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Ellipse {
            major_radius: 1.0,
            minor_radius: 1.0,
            ..
        }
    )));
    let record = &result.ir().native.namespace("creo").unwrap().arenas["reference_ellipses"][0];
    assert_eq!(record.fields()["source_entity_id"], 43);
    assert_eq!(record.fields()["major_radius"], 1.0);
    assert_eq!(record.fields()["minor_radius"], 1.0);
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_REFERENCE_ELLIPSE_COUNT),
        1
    );
    let ellipse = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id.as_str() == "creo:mdl_ref_info:conic#43")
        .expect("canonically identified conic");
    assert_eq!(
        ellipse.source_object.as_ref().unwrap().object_id,
        "MdlRefInfo:conic:43"
    );
    assert!(result.report().losses.iter().any(|loss| {
        loss.message
            .contains("Transferred 1 elliptical reference carrier")
    }));
    assert_annotation(
        &result.source_fidelity().annotations,
        record.id(),
        "creo:MdlRefInfo",
        scan.references.ellipses[0].offset as u64,
        "reference_ellipse_carrier",
        Exactness::Derived,
    );
}
