// SPDX-License-Identifier: Apache-2.0
//! Unit and fixture tests for OM wire parsers owned by `om`.

#![allow(clippy::unwrap_used)]

use crate::test_support::*;

#[test]
fn om_index_pairs_object_ids_with_bounded_entity_records() {
    let bytes = indexed_om_section();
    let sections = super::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].base, 8);
    assert_eq!(sections[0].records.len(), 2);
    assert_eq!(sections[0].records[0].object_id, Some(0x101));
    assert_eq!(
        sections[0].records[0].object_id_offset,
        Some(sections[0].object_id_table_offset + 8)
    );
    assert_eq!(
        sections[0].records[0].bytes,
        b"\x04\x01\x0eNX 2027.3102\x00hostglobalvariables"
    );
    assert_eq!(sections[0].records[1].object_id, Some(0x102));
    assert_eq!(
        sections[0].records[1].object_id_offset,
        Some(sections[0].object_id_table_offset + 12)
    );
    assert_eq!(sections[0].column_storage, None);
    assert_eq!(sections[0].fields.len(), 1);
    assert_eq!(sections[0].fields[0].name, "m_target");
    assert_eq!(
        sections[0].records[1].bytes,
        b"\x04\x36p8_CircularPattern_pattern_Circular_Dir_offset_angle\x00\x04\x05120\x00\x99\x04P(Number [degrees]) p8_CircularPattern_pattern_Circular_Dir_offset_angle: 120; \x00\x66\x32\x03\x0cSKETCH_001\0\xe0\x12\x34\x56\x78\xca\xbc\xde\xf0\x01\x02\x90\x00\x00"
    );
}

#[test]
fn om_compact_index_lane_decodes_direct_extended_and_null_entries() {
    use super::CompactIndex::{Null, Value};

    assert_eq!(
        super::compact_indices(&[0x00, 0x7f, 0x80, 0x80, 0x81, 0x00, 0xfe, 0xff, 0xff]),
        Some(vec![
            Value(0),
            Value(127),
            Value(128),
            Value(256),
            Value(32_511),
            Null,
        ])
    );
    assert_eq!(super::compact_indices(&[0x80]), None);
}

#[test]
fn om_data_block_object_frame_requires_complete_discriminator() {
    let discriminator = [
        0x00, 0x72, 0x01, 0xc0, 0x20, 0x02, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x01,
        0x02, 0x80, 0xa4,
    ];
    let mut bytes = vec![0xaa, 0x81, 0x72];
    bytes.extend_from_slice(&discriminator);
    bytes.push(0xff);

    let references = super::data_block_object_frames(&bytes);
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].object_id, 370);
    assert_eq!(references[0].raw_object_id, [0x81, 0x72]);
    assert_eq!(references[0].offset, 1);

    bytes.extend_from_slice(&[0x73]);
    bytes.extend_from_slice(&discriminator);
    let references = super::data_block_object_frames(&bytes);
    assert_eq!(references.len(), 2);
    assert_eq!(references[1].object_id, 0x73);
    assert_eq!(references[1].raw_object_id, [0x73]);
    assert_eq!(references[1].offset, 22);

    bytes[8] ^= 1;
    let references = super::data_block_object_frames(&bytes);
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].object_id, 0x73);
    let mut null = vec![0xff];
    null.extend_from_slice(&discriminator);
    assert!(super::data_block_object_frames(&null).is_empty());
}

#[test]
fn om_offset_store_counted_index_lane_requires_complete_non_null_members() {
    let bytes = [
        0xaa, 0x01, 0x06, 0x42, 0x62, 0x80, 0x48, 0x80, 0x50, 0x7c, 0x01, 0x11, 0xbb,
    ];
    let lanes = super::offset_store_counted_index_lanes(&bytes);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].offset, 1);
    assert_eq!(lanes[0].declared_count, 6);
    assert_eq!(lanes[0].anchor, 0x42);
    assert_eq!(lanes[0].raw_anchor, [0x42]);
    assert_eq!(lanes[0].anchor_offset, 3);
    assert_eq!(
        lanes[0].members,
        vec![(0x62, 4), (0x48, 5), (0x50, 7), (0x7c, 9)]
    );
    assert_eq!(
        lanes[0].raw_members,
        [vec![0x62], vec![0x80, 0x48], vec![0x80, 0x50], vec![0x7c]]
    );

    assert!(
        super::offset_store_counted_index_lanes(&[0x01, 0x03, 0x42, 0xff, 0x01, 0x11,]).is_empty()
    );
    assert!(
        super::offset_store_counted_index_lanes(&[0x01, 0x03, 0x42, 0x80, 0x01, 0x11,]).is_empty()
    );
    assert!(
        super::offset_store_counted_index_lanes(&[0x01, 0x03, 0x42, 0x62, 0x01, 0x10,]).is_empty()
    );
}

#[test]
fn om_offset_store_abr_lane_requires_sixteen_slots_and_exact_terminator() {
    let mut bytes = vec![0xaa, 0x11];
    bytes.extend_from_slice(&[0xff; 6]);
    bytes.extend_from_slice(&[0x82, 0x83]);
    bytes.extend_from_slice(&[0xff; 9]);
    bytes.extend_from_slice(&[0x02, 0x11, b'A', b'B', b'R', 0xff, 0x03, 0xbb]);

    let lanes = super::offset_store_abr_reference_lanes(&bytes);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].offset, 1);
    assert_eq!(lanes[0].slots.len(), 16);
    assert_eq!(lanes[0].slots[6], (Some(643), 8));
    assert_eq!(lanes[0].raw_slots[6], [0x82, 0x83]);
    assert!(lanes[0]
        .raw_slots
        .iter()
        .enumerate()
        .all(|(slot, raw)| slot == 6 || raw == &[0xff]));
    assert!(lanes[0]
        .slots
        .iter()
        .enumerate()
        .all(|(slot, (value, _))| slot == 6 || value.is_none()));

    bytes[23] = b'X';
    assert!(super::offset_store_abr_reference_lanes(&bytes).is_empty());
    bytes[23] = b'R';
    bytes.remove(18);
    assert!(super::offset_store_abr_reference_lanes(&bytes).is_empty());
}

#[test]
fn om_sketch_scalar_field_requires_exact_frame_and_finite_shifted_value() {
    let bytes = [
        0xaa, 0x50, 0x59, 0x66, 0x64, 0x00, 0x30, 0x43, 0x0c, 0xcc, 0xcc, 0xcc, 0xcd, 0x72, 0xbb,
    ];
    let fields = super::construction_payload_scalar_fields(&bytes);
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].offset, 1);
    assert_eq!(fields[0].field_code, 0x64);
    assert!((fields[0].value - 38.1).abs() < 2.0e-12);

    let mut malformed = bytes;
    malformed[5] = 1;
    assert!(super::construction_payload_scalar_fields(&malformed).is_empty());
    malformed = bytes;
    malformed[6] = 0x70;
    assert!(super::construction_payload_scalar_fields(&malformed).is_empty());
}

#[test]
fn om_sketch_name_field_decodes_direct_and_extended_compact_type_codes() {
    let bytes = [
        0x66, 0x32, 0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1', 0x00, 0xaa, 0x66, 0x80, 0x83,
        0x03, 0x07, b'L', b'i', b'n', b'e', b'2', 0x00,
    ];
    let fields = super::construction_payload_named_fields(&bytes);
    assert_eq!(fields.len(), 2);
    assert_eq!(
        (fields[0].offset, fields[0].type_code, fields[0].value),
        (0, Some(0x32), "Point1")
    );
    assert_eq!(fields[0].raw_type_code, Some(vec![0x32]));
    assert_eq!(fields[0].type_code_offset, Some(1));
    assert_eq!(
        (fields[1].offset, fields[1].type_code, fields[1].value),
        (12, Some(0x83), "Line2")
    );
    assert_eq!(fields[1].raw_type_code, Some(vec![0x80, 0x83]));
    assert_eq!(fields[1].type_code_offset, Some(13));

    assert!(super::construction_payload_named_fields(&[
        0x66, 0xff, 0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1', 0x00,
    ])
    .is_empty());
    assert!(super::construction_payload_named_fields(&[
        0x66, 0x32, 0x03, 0x08, b'P', b'o', b'i', b'n', b't',
    ])
    .is_empty());
}

#[test]
fn om_sketch_name_field_decodes_type_free_payload_leading_form() {
    let fields = super::construction_payload_named_fields(&[
        0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1', 0x00, 0x04,
    ]);
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].offset, 0);
    assert_eq!(fields[0].type_code, None);
    assert_eq!(fields[0].raw_type_code, None);
    assert_eq!(fields[0].type_code_offset, None);
    assert!(fields[0].payload_leading);
    assert_eq!(fields[0].value, "Point1");

    assert!(super::construction_payload_named_fields(&[
        0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1',
    ])
    .is_empty());
}

#[test]
fn om_offset_store_named_point_uses_minimal_consecutive_block_span() {
    let first = [
        0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'7', 0x00, 0x50, 0x59, 0x66, 0x58, 0x00, 0x30,
        0x4c, 0x93, 0x33, 0x33, 0x33, 0x33, 0x07,
    ];
    let second = [
        0x45, 0x04, 0x00, 0x50, 0x59, 0x66, 0x58, 0x00, 0x30, 0x4c, 0x93, 0x33, 0x33, 0x33, 0x33,
        0x07,
    ];
    let point = super::offset_store_named_point(&[&first, &second]).unwrap();
    assert_eq!(point.name, "Point7");
    assert!(point
        .values
        .iter()
        .all(|value| (*value - 57.15).abs() < 1.0e-12));
    let expected_raw: [[u8; 8]; 2] = [
        first[14..22].try_into().unwrap(),
        second[8..16].try_into().unwrap(),
    ];
    assert_eq!(point.raw_values, expected_raw);
    assert_eq!(point.value_offsets, [9, first.len() + 3]);
    assert_eq!(point.block_count, 2);

    let mut same_block = first.to_vec();
    same_block.extend_from_slice(&second);
    assert_eq!(
        super::offset_store_named_point(&[&same_block])
            .unwrap()
            .block_count,
        1
    );
    assert_eq!(
        super::offset_store_named_point(&[&first[..9], &first[9..], &second])
            .unwrap()
            .block_count,
        3
    );
    let third = [
        0x50, 0x59, 0x66, 0x58, 0x00, 0x30, 0x4c, 0x93, 0x33, 0x33, 0x33, 0x33, 0x07,
    ];
    assert!(super::offset_store_named_point(&[&first, &second, &third]).is_none());
    let next_name = [
        0x66, 0x32, 0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'8', 0x00,
    ];
    assert!(super::offset_store_named_point(&[&first, &second, &next_name]).is_none());
    let next_point = [0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'8', 0x00];
    assert_eq!(
        super::offset_store_named_point(&[&first, &second, &next_point])
            .unwrap()
            .block_count,
        2
    );
    let mut zero = first;
    zero[7] = b'0';
    assert!(super::offset_store_named_point(&[&zero, &second]).is_none());
}

#[test]
fn sketch_fixed_pair_parser_reads_signed_q1_55_atoms() {
    let bytes = [
        0x04, 0xe0, 0x48, 0x0e, 0x02, 0x03, 0x80, 0x84, 0x30, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x30, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let pairs = super::sketch_payload_fixed_pairs(&bytes);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].values, [0.5, -0.5]);
    assert_eq!(pairs[0].value_offsets, [8, 17]);
    assert_eq!(pairs[0].raw_values[0], [0x40, 0, 0, 0, 0, 0, 0]);
    assert_eq!(pairs[0].discriminator, bytes[..8]);

    let mut malformed = bytes;
    malformed[16] = 1;
    assert!(super::sketch_payload_fixed_pairs(&malformed).is_empty());
}

#[test]
fn sketch_fixed_pair_parser_accepts_adjacent_short_and_extended_branches() {
    let short = [
        0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x01,
        0x30, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x30, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ];
    let extended = [
        0x08, 0x02, 0x03, 0x01, 0xc0, 0x40, 0x02, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02,
        0x00, 0x01, 0x30, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x30, 0xe0, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00,
    ];

    let short_pair = super::sketch_payload_fixed_pairs(&short);
    assert_eq!(short_pair.len(), 1);
    assert_eq!(short_pair[0].values, [0.5, -0.5]);
    assert_eq!(short_pair[0].value_offsets, [15, 23]);
    assert_eq!(short_pair[0].discriminator, short[..15]);

    let extended_pair = super::sketch_payload_fixed_pairs(&extended);
    assert_eq!(extended_pair.len(), 1);
    assert_eq!(extended_pair[0].values, [0.25, -0.25]);
    assert_eq!(extended_pair[0].value_offsets, [17, 25]);
    assert_eq!(extended_pair[0].discriminator, extended[..17]);

    let mut malformed = short;
    malformed[23] = 0x31;
    assert!(super::sketch_payload_fixed_pairs(&malformed).is_empty());
}

#[test]
fn sketch_fixed_pair_parser_accepts_the_three_member_branch() {
    let discriminator = [
        0x0b, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
    ];
    let mut bytes = discriminator.to_vec();
    bytes.push(0x30);
    bytes.extend_from_slice(&[0x40, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0x00, 0x30]);
    bytes.extend_from_slice(&[0xc0, 0, 0, 0, 0, 0, 0]);

    let pairs = super::sketch_payload_fixed_pairs(&bytes);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].values, [0.5, -0.5]);
    assert_eq!(pairs[0].value_offsets, [15, 24]);
    assert_eq!(pairs[0].discriminator, discriminator);

    bytes[14] = 0x02;
    assert!(super::sketch_payload_fixed_pairs(&bytes).is_empty());
}

#[test]
fn sketch_mixed_pair_parser_requires_q1_55_then_shifted_binary32() {
    let mut bytes = vec![0x04, 0xe0, 0x48, 0x0e, 0x02, 0x03, 0x80, 0x84, 0x30];
    bytes.extend_from_slice(&[0x40, 0, 0, 0, 0, 0, 0]);
    bytes.push(0x00);
    let mut shifted = 3.25_f32.to_be_bytes();
    shifted[0] += 0x10;
    bytes.extend_from_slice(&shifted);

    let pairs = super::sketch_payload_mixed_pairs(&bytes);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].fixed_value, 0.5);
    assert_eq!(pairs[0].binary32_value, 3.25);
    assert_eq!(pairs[0].fixed_raw_value, [0x40, 0, 0, 0, 0, 0, 0]);
    assert_eq!(pairs[0].binary32_raw_value, shifted);
    assert_eq!(pairs[0].value_offsets, [8, 17]);

    let mut malformed = bytes;
    malformed[16] = 1;
    assert!(super::sketch_payload_mixed_pairs(&malformed).is_empty());
}

#[test]
fn datum_csys_fixed_pair_requires_its_exact_branch_discriminator() {
    let mut bytes = vec![
        0x0b, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
        0x30,
    ];
    bytes.extend_from_slice(&[0x40, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0x00, 0x30]);
    bytes.extend_from_slice(&[0xc0, 0, 0, 0, 0, 0, 0]);
    let pairs = super::datum_csys_payload_fixed_pairs(&bytes);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].values, [0.5, -0.5]);
    assert_eq!(pairs[0].value_offsets, [15, 24]);
    assert_eq!(pairs[0].raw_values[0], [0x40, 0, 0, 0, 0, 0, 0]);

    bytes[0] = 0x08;
    assert!(super::datum_csys_payload_fixed_pairs(&bytes).is_empty());
}

#[test]
fn datum_csys_fixed_pair_accepts_the_continuation_branch() {
    let discriminator = [
        0x80, 0x8d, 0x00, 0xff, 0x80, 0x81, 0x01, 0x02, 0x01, 0x00, 0x00, 0x00, 0x87, 0xd7, 0x01,
        0x01, 0x01, 0x01, 0x02, 0xa5, 0x30, 0x21, 0xa5, 0x30, 0x21, 0x01, 0x00, 0x01, 0xaf, 0xff,
        0xdf, 0x02, 0x01, 0x02,
    ];
    let mut bytes = discriminator.to_vec();
    bytes.push(0x30);
    bytes.extend_from_slice(&[0x40, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0x00, 0x30]);
    bytes.extend_from_slice(&[0xc0, 0, 0, 0, 0, 0, 0]);

    let pairs = super::datum_csys_payload_fixed_pairs(&bytes);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].values, [0.5, -0.5]);
    assert_eq!(
        pairs[0].value_offsets,
        [discriminator.len(), discriminator.len() + 9]
    );
    assert_eq!(pairs[0].discriminator, discriminator);

    bytes[1] = 0x8c;
    assert!(super::datum_csys_payload_fixed_pairs(&bytes).is_empty());
}

#[test]
fn om_datum_csys_scalar_field_uses_the_common_shifted_binary64_frame() {
    let mut shifted = 25.4_f64.to_be_bytes();
    shifted[0] -= 0x10;
    let mut payload = vec![0xaa, 0x50, 0x59, 0x66, 0x64, 0x00];
    payload.extend_from_slice(&shifted);
    payload.push(0xbb);

    let fields = super::construction_payload_scalar_fields(&payload);
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].offset, 1);
    assert_eq!(fields[0].field_code, 0x64);
    assert_eq!(fields[0].value, 25.4);
    assert_eq!(fields[0].raw_value, shifted);
}

#[test]
fn om_simple_hole_lane_requires_two_identical_nonempty_scalar_runs() {
    let shifted = |value: f64| {
        let mut bytes = value.to_be_bytes();
        bytes[0] -= 0x10;
        bytes
    };
    let mut payload = Vec::new();
    for value in [508.0, 38.1, 508.0, 38.1] {
        payload.extend_from_slice(&shifted(value));
        payload.push(0x7f);
    }
    payload.extend_from_slice(&[0x04, 0x08]);
    payload.extend_from_slice(b"Hole_X");
    payload.push(0x00);
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 120,
        value: "SIMPLE HOLE",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let record = super::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label,
    };
    let lane = super::simple_hole_repeated_scalar_lane(record).unwrap();
    assert_eq!(lane.values[0], 508.0);
    assert!((lane.values[1] - 38.1).abs() < 2.0e-12);
    assert_eq!(lane.raw_values, [shifted(508.0), shifted(38.1)]);
    assert_eq!(lane.witness_offsets, [vec![200, 209], vec![218, 227]]);

    let mut mismatched = payload.clone();
    mismatched[18 + 7] ^= 1;
    assert!(
        super::simple_hole_repeated_scalar_lane(super::OperationRecord {
            bytes: &mismatched,
            payload: &mismatched,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_simple_hole_lane_accepts_one_repeated_scalar() {
    let mut scalar = 25.4f64.to_be_bytes();
    scalar[0] -= 0x10;
    let mut payload = scalar.to_vec();
    payload.push(0x7f);
    payload.extend_from_slice(&scalar);
    payload.extend_from_slice(&[0x04, 0x08]);
    payload.extend_from_slice(b"Hole_X\0");
    let record = super::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label: super::OperationLabel {
            header_offset: 100,
            offset: 120,
            value: "SIMPLE HOLE",
            object_indices: [None; 4],
            object_index_offsets: [0; 4],
        },
    };
    let lane = super::simple_hole_repeated_scalar_lane(record).unwrap();
    assert_eq!(lane.values, [25.4]);
    assert_eq!(lane.raw_values, [scalar]);
    assert_eq!(lane.witness_offsets, [vec![200], vec![209]]);
}

#[test]
fn om_simple_hole_lane_block_references_follow_both_scalar_runs() {
    let shifted = |value: f64| {
        let mut bytes = value.to_be_bytes();
        bytes[0] -= 0x10;
        bytes
    };
    let mut payload = Vec::new();
    payload.extend_from_slice(&shifted(508.0));
    payload.extend_from_slice(&shifted(38.1));
    payload.extend_from_slice(&[0xf0, 0xe7, 0xf0, 0xe8]);
    payload.extend_from_slice(&shifted(508.0));
    payload.extend_from_slice(&shifted(38.1));
    payload.extend_from_slice(&[0xf0, 0xe9, 0xf0, 0xea]);
    payload.extend_from_slice(&[0x04, 0x08]);
    payload.extend_from_slice(b"Hole_X");
    payload.push(0x00);
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 120,
        value: "SIMPLE HOLE",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let record = super::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label,
    };
    let references = super::simple_hole_repeated_scalar_lane_block_references(record).unwrap();
    assert_eq!(references.first, [231, 232]);
    assert_eq!(references.second, [233, 234]);
    assert_eq!(references.offsets, [[216, 218], [236, 238]]);
    assert_eq!(references.prefixes, [None, None]);

    let first_prefix = [0x50, 0x10, 0x00, 0x04, 0x50, 0x49, 0x66, 0x2e];
    let second_prefix = [0x50, 0x21, 0x66, 0x62, 0x50, 0x49, 0x66, 0x2e];
    let mut wrapped = Vec::new();
    wrapped.extend_from_slice(&shifted(508.0));
    wrapped.extend_from_slice(&shifted(38.1));
    wrapped.extend_from_slice(&first_prefix);
    wrapped.extend_from_slice(&[0xf0, 0xe7, 0xf0, 0xe8]);
    wrapped.extend_from_slice(&shifted(508.0));
    wrapped.extend_from_slice(&shifted(38.1));
    wrapped.extend_from_slice(&second_prefix);
    wrapped.extend_from_slice(&[0xf0, 0xe9, 0xf0, 0xea]);
    wrapped.extend_from_slice(&[0x04, 0x08]);
    wrapped.extend_from_slice(b"Hole_X\0");
    let wrapped_references =
        super::simple_hole_repeated_scalar_lane_block_references(super::OperationRecord {
            bytes: &wrapped,
            payload: &wrapped,
            ..record
        })
        .unwrap();
    assert_eq!(wrapped_references.first, [231, 232]);
    assert_eq!(wrapped_references.second, [233, 234]);
    assert_eq!(wrapped_references.offsets, [[224, 226], [252, 254]]);
    assert_eq!(
        wrapped_references.prefixes,
        [Some(first_prefix), Some(second_prefix)]
    );
    let mut malformed_wrapper = wrapped.clone();
    malformed_wrapper[16] ^= 1;
    assert!(
        super::simple_hole_repeated_scalar_lane_block_references(super::OperationRecord {
            bytes: &malformed_wrapper,
            payload: &malformed_wrapper,
            ..record
        },)
        .is_none()
    );

    let mut null = payload.clone();
    null[16] = 0xff;
    assert!(
        super::simple_hole_repeated_scalar_lane_block_references(super::OperationRecord {
            bytes: &null,
            payload: &null,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_hole_package_lane_retains_the_exact_four_block_group() {
    let payload = [
        0x7e, 0x00, 0x00, 0x01, 0x00, 0x00, 0x46, 0x00, 0x11, 0x00, 0x00, 0x00, 0x00, 0xf0, 0xcd,
        0xf0, 0xce, 0x11, 0x00, 0x00, 0x00, 0x00, 0xf0, 0xcf, 0xf0, 0xd0, 0x00, 0x00, 0xff, 0x7f,
    ];
    let record = super::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label: super::OperationLabel {
            header_offset: 100,
            offset: 120,
            value: "HOLE PACKAGE",
            object_indices: [None; 4],
            object_index_offsets: [0; 4],
        },
    };
    let lane = super::hole_package_construction_group_lane(record).unwrap();
    assert_eq!(lane.offset, 1);
    assert_eq!(lane.selector, 0x46);
    assert_eq!(lane.branch, 0x11);
    assert_eq!(
        lane.references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>(),
        [205, 206, 207, 208]
    );
    assert_eq!(
        lane.references
            .iter()
            .map(|reference| reference.offset)
            .collect::<Vec<_>>(),
        [213, 215, 222, 224]
    );

    let mut mismatched_branch = payload;
    mismatched_branch[17] = 0x12;
    assert!(
        super::hole_package_construction_group_lane(super::OperationRecord {
            bytes: &mismatched_branch,
            payload: &mismatched_branch,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_datum_csys_reference_lane_requires_eight_canonical_indices() {
    let mut payload = vec![
        0x13, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    ];
    for value in 42..50 {
        payload.extend_from_slice(&[0xf0, value]);
    }
    payload.extend_from_slice(&[0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00]);
    let label = super::OperationLabel {
        header_offset: 10,
        offset: 20,
        value: "DATUM_CSYS",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let record = super::OperationRecord {
        offset: 10,
        bytes: &payload,
        payload_offset: 100,
        payload: &payload,
        label,
    };
    let field = super::datum_csys_references(record).unwrap();
    assert_eq!(field.control, 0x13);
    assert_eq!(
        field
            .references
            .each_ref()
            .map(|reference| reference.object_index),
        [42, 43, 44, 45, 46, 47, 48, 49]
    );
    assert_eq!(
        field
            .references
            .each_ref()
            .map(|reference| reference.offset),
        [114, 116, 118, 120, 122, 124, 126, 128]
    );
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.raw_object_index.clone())
            .collect::<Vec<_>>(),
        (42..50).map(|value| vec![0xf0, value]).collect::<Vec<_>>()
    );

    let mut alternate_control = payload.clone();
    alternate_control[0] = 0x1a;
    assert_eq!(
        super::datum_csys_references(super::OperationRecord {
            bytes: &alternate_control,
            payload: &alternate_control,
            ..record
        })
        .unwrap()
        .control,
        0x1a
    );

    let mut malformed = payload.clone();
    malformed[14] = 0x2a;
    assert!(super::datum_csys_references(super::OperationRecord {
        bytes: &malformed,
        payload: &malformed,
        ..record
    })
    .is_none());
}

#[test]
fn om_datum_plane_header_requires_common_prefix_and_nontrivial_count() {
    let payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x03, 0x29, 0x01, 0x02, 0xf1, 0x02, 0xcf,
    ];
    let label = super::OperationLabel {
        header_offset: 10,
        offset: 20,
        value: "DATUM_PLANE",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let record = super::OperationRecord {
        offset: 10,
        bytes: &payload,
        payload_offset: 100,
        payload: &payload,
        label,
    };
    assert_eq!(
        super::datum_plane_payload_header(record),
        Some(super::DatumPlanePayloadHeader {
            control: 0x22,
            declared_count: 3,
            branch_tag: 0x29,
        })
    );
    let mut malformed = payload;
    malformed[6] = 1;
    assert!(super::datum_plane_payload_header(super::OperationRecord {
        bytes: &malformed,
        payload: &malformed,
        ..record
    })
    .is_none());

    let branch_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x02, 0x23, 0x01, 0x02, 0x80, 0x4c, 0x01, 0xf1, 0x02,
        0xbb, 0x00, 0x14, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00,
    ];
    let branch = super::datum_plane_single_reference_branch(super::OperationRecord {
        bytes: &branch_payload,
        payload: &branch_payload,
        ..record
    })
    .unwrap();
    assert_eq!(branch.descriptor_index, 76);
    assert_eq!(branch.raw_descriptor_index, [0x80, 0x4c]);
    assert_eq!(branch.descriptor_offset, 110);
    assert_eq!(branch.object_index, 699);
    assert_eq!(branch.raw_object_index, [0xf1, 0x02, 0xbb]);
    assert_eq!(branch.object_offset, 113);

    let double_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x02, 0x29, 0x01, 0x02, 0xf1, 0x02, 0x77, 0x01, 0x01,
        0x18, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xf1, 0x02, 0x78, 0x01, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x0d,
    ];
    let double = super::datum_plane_double_reference_branch(super::OperationRecord {
        bytes: &double_payload,
        payload: &double_payload,
        ..record
    })
    .unwrap();
    assert_eq!(
        double
            .references
            .each_ref()
            .map(|reference| reference.object_index),
        [631, 632]
    );
    assert_eq!(
        double
            .references
            .each_ref()
            .map(|reference| reference.offset),
        [110, 124]
    );

    let count_three_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x03, 0x29, 0x01, 0x02, 0xf1, 0x02, 0xcf, 0x01, 0x01,
        0x3a, 0x01, 0x02, 0xf1, 0x02, 0xd0, 0x01, 0x17, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
        0xff, 0xff, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d,
    ];
    let count_three = super::datum_plane_double_reference_branch(super::OperationRecord {
        bytes: &count_three_payload,
        payload: &count_three_payload,
        ..record
    })
    .unwrap();
    assert_eq!(
        count_three
            .references
            .each_ref()
            .map(|reference| reference.object_index),
        [719, 720]
    );
    assert_eq!(
        count_three
            .references
            .each_ref()
            .map(|reference| reference.offset),
        [110, 118]
    );

    let descriptor_count_three_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x03, 0x28, 0x01, 0x02, 0x80, 0x4d, 0x01, 0x29, 0x01,
        0x02, 0xf1, 0x02, 0xd1, 0x01, 0x01, 0x07, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff,
        0xff, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d,
    ];
    let descriptor_count_three =
        super::datum_plane_descriptor_reference_branch(super::OperationRecord {
            bytes: &descriptor_count_three_payload,
            payload: &descriptor_count_three_payload,
            ..record
        })
        .unwrap();
    assert_eq!(descriptor_count_three.descriptor_index, 77);
    assert_eq!(descriptor_count_three.raw_descriptor_index, [0x80, 0x4d]);
    assert_eq!(descriptor_count_three.descriptor_offset, 110);
    assert_eq!(descriptor_count_three.object_index, 721);
    assert_eq!(descriptor_count_three.object_offset, 116);
}

#[test]
fn om_datum_plane_object_index_lane_ends_at_logical_payload_boundary() {
    let bytes = [
        0x80, 0xab, 0x01, 0x04, 0x81, 0x01, 0x01, 0x01, 0x00, 0x12, 0x34, 0x56, 0x78,
    ];
    let lanes = super::datum_plane_object_index_lanes(&bytes);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].offset, 2);
    assert_eq!(lanes[0].declared_count, 4);
    assert_eq!(lanes[0].indices, [(257, 4), (1, 6), (1, 7)]);
    assert_eq!(lanes[0].raw_indices, [vec![0x81, 0x01], vec![1], vec![1]]);
    assert_eq!(lanes[0].trailer, 0x1234_5678);

    let mut trailing = bytes.to_vec();
    trailing.push(0);
    assert!(super::datum_plane_object_index_lanes(&trailing).is_empty());
}

#[test]
fn om_datum_plane_object_scalar_pairs_require_the_complete_discriminator() {
    let mut bytes = vec![0x7f, 0x01, 0x01, 0xff];
    bytes.extend_from_slice(&[
        0x6d, 0x00, 0xf0, 0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86,
        0x02, 0x00, 0x03,
    ]);
    bytes.extend_from_slice(&[0x30, 0x24, 0, 0, 0, 0, 0, 0]);
    bytes.push(0);
    bytes.extend_from_slice(&[0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
    let pairs = super::datum_plane_object_scalar_pairs(&bytes);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].offset, 4);
    assert_eq!(pairs[0].value_offsets, [22, 31]);
    assert_eq!(pairs[0].values, [10.0, -20.0]);
    assert_eq!(pairs[0].raw_values[0], [0x30, 0x24, 0, 0, 0, 0, 0, 0]);
    assert_eq!(pairs[0].raw_values[1], [0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
    bytes[10] ^= 1;
    assert!(super::datum_plane_object_scalar_pairs(&bytes).is_empty());
}

#[test]
fn om_datum_plane_descriptor_requires_complete_lowercase_hex_identity() {
    let mut bytes = *b"793487222121a5474a9125451b8e31f5?A\xf0\x1e\xff\x02\x01\x33";
    let descriptor = super::datum_plane_descriptor_block(&bytes).unwrap();
    assert_eq!(descriptor.identity, "793487222121a5474a9125451b8e31f5");
    assert_eq!(descriptor.suffix, b"?A\xf0\x1e\xff\x02\x01\x33");
    assert_eq!(descriptor.schema_index, 28_702);
    assert_eq!(descriptor.label, "3");

    let short_bytes = *b"a75c5f0ed880dd1443b3c5c57908aae?A\xf0\x1f\xff\x02\x01\x66\x33";
    let short = super::datum_plane_descriptor_block(&short_bytes).unwrap();
    assert_eq!(short.identity.len(), 31);
    assert_eq!(short.schema_index, 28_703);
    assert_eq!(short.label, "f3");

    bytes[0] = b'G';
    assert!(super::datum_plane_descriptor_block(&bytes).is_none());
    assert!(super::datum_plane_descriptor_block(&bytes[..39]).is_none());
}

#[test]
fn om_datum_csys_scalar_pairs_require_discriminator_and_separator() {
    let mut bytes = vec![0x2f, 0x2f, 0x41, 0x6d, 0x00, 0xf0];
    bytes.extend_from_slice(&[
        0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
    ]);
    bytes.extend_from_slice(&[0x30, 0x24, 0, 0, 0, 0, 0, 0]);
    bytes.push(0);
    bytes.extend_from_slice(&[0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
    let pairs = super::object_payload_scalar_pairs(&bytes);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].offset, 6);
    assert_eq!(pairs[0].value_offsets, [21, 30]);
    assert_eq!(pairs[0].values, [10.0, -20.0]);
    assert_eq!(pairs[0].raw_values[0], [0x30, 0x24, 0, 0, 0, 0, 0, 0]);
    assert_eq!(pairs[0].raw_values[1], [0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
    assert_eq!(pairs[0].discriminator.len(), 15);

    let mut extended = vec![
        0x08, 0x02, 0x03, 0x01, 0x81, 0x02, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00,
        0x03,
    ];
    extended.extend_from_slice(&[0x30, 0x24, 0, 0, 0, 0, 0, 0]);
    extended.push(0);
    extended.extend_from_slice(&[0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
    let extended_pairs = super::object_payload_scalar_pairs(&extended);
    assert_eq!(extended_pairs.len(), 1);
    assert_eq!(extended_pairs[0].discriminator.len(), 16);
    assert_eq!(extended_pairs[0].value_offsets, [16, 25]);
    assert_eq!(
        extended_pairs[0].raw_values[0],
        [0x30, 0x24, 0, 0, 0, 0, 0, 0]
    );

    bytes[29] = 1;
    assert!(super::object_payload_scalar_pairs(&bytes).is_empty());
}

#[test]
fn om_datum_csys_descriptor_requires_one_maximal_hex_identity() {
    let bytes = b"\x02\x01ae166162820ea2d993e1fdf49091850e?A\x80\xa0\xf0\x26";
    let descriptor = super::datum_csys_descriptor_block(bytes).unwrap();
    assert_eq!(descriptor.prefix, [0x02, 0x01]);
    assert_eq!(descriptor.identity, "ae166162820ea2d993e1fdf49091850e");
    assert_eq!(descriptor.identity_offset, 2);
    assert_eq!(descriptor.suffix, b"?A\x80\xa0\xf0\x26");

    let mut ambiguous = bytes.to_vec();
    ambiguous.extend_from_slice(b"012345678901234567890123456789");
    assert!(super::datum_csys_descriptor_block(&ambiguous).is_none());
}

#[test]
fn om_draft_identity_frames_require_complete_typed_framing() {
    let bytes = b"\x00A\x81\x54\xf0\x38\x02\x01abc123?A\xf0\x27\xff\x02\x01def456?\x00";
    let frames = super::draft_construction_identity_frames(bytes);
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].offset, 1);
    assert_eq!(frames[0].prefix, b"A\x81\x54\xf0\x38\x02\x01");
    assert_eq!(
        frames[0].form,
        super::DraftConstructionIdentityFrameForm::IndexedBranch {
            first_index: 340,
            second_index: Some(56),
            branch: 2,
        }
    );
    assert_eq!(frames[0].identity, "abc123");
    assert_eq!(frames[0].identity_offset, 8);
    assert_eq!(frames[1].offset, 15);
    assert_eq!(frames[1].prefix, b"A\xf0\x27\xff\x02\x01");
    assert_eq!(
        frames[1].form,
        super::DraftConstructionIdentityFrameForm::Tagged { index: Some(39) }
    );
    assert_eq!(frames[1].identity, "def456");

    assert!(
        super::draft_construction_identity_frames(b"A\x81\x54\xf0\x38\x02\x01abc123").is_empty()
    );
    assert!(
        super::draft_construction_identity_frames(b"A\x81\x54\xf0\x38\x04\x01abc123?").is_empty()
    );
    assert!(super::draft_construction_identity_frames(b"A\xf0\x27\xff\x02\x01ABC123?").is_empty());
}

#[test]
fn om_draft_fixed_lanes_require_complete_discriminator_atoms_and_terminator() {
    let discriminator = [
        0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x07, 0x01, 0xc0, 0x45, 0x10, 0x00, 0x80, 0x86, 0x02,
        0x00, 0x01, 0x00,
    ];
    let mut bytes = vec![0xff];
    bytes.extend_from_slice(&discriminator);
    bytes.extend_from_slice(&[0x30, 0x40, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0xb0, 0xc0, 0, 0, 0, 0, 0, 0]);
    bytes.push(0);
    let lanes = super::draft_construction_fixed_lanes(&bytes);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].offset, 1);
    assert_eq!(lanes[0].values, [0.5, -0.5]);
    assert_eq!(lanes[0].markers, [0x30, 0xb0]);
    assert_eq!(lanes[0].value_offsets, [19, 27]);

    bytes.pop();
    assert!(super::draft_construction_fixed_lanes(&bytes).is_empty());
    bytes.truncate(22);
    assert!(super::draft_construction_fixed_lanes(&bytes).is_empty());
    assert!(super::draft_construction_fixed_lanes(&discriminator).is_empty());
}

#[test]
fn om_draft_binary32_lanes_require_complete_typed_atoms_and_terminator() {
    let discriminator = [
        0x90, 0x18, 0x45, 0x01, 0x04, 0x01, 0x04, 0x01, 0xc0, 0x45, 0x04, 0x04, 0x80, 0x86, 0x02,
        0x00, 0x03, 0x00,
    ];
    let mut bytes = vec![0xff];
    bytes.extend_from_slice(&discriminator);
    bytes.extend_from_slice(&[0x4f, 0x80, 0, 0]);
    bytes.extend_from_slice(&[0xcf, 0x80, 0, 0]);
    bytes.push(0);
    let lanes = super::draft_construction_binary32_lanes(&bytes);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].offset, 1);
    assert_eq!(lanes[0].discriminator, discriminator);
    assert_eq!(lanes[0].branch, 4);
    assert_eq!(lanes[0].values, [1.0, -1.0]);
    assert_eq!(lanes[0].value_offsets, [19, 23]);

    bytes.pop();
    assert!(super::draft_construction_binary32_lanes(&bytes).is_empty());
    bytes.truncate(21);
    assert!(super::draft_construction_binary32_lanes(&bytes).is_empty());
    assert!(super::draft_construction_binary32_lanes(&discriminator).is_empty());
}

#[test]
fn om_operation_primary_body_reference_requires_one_complete_field() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 100,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let bytes = [0x01, 0x02, 0x10, 0x90, 0x19, 0x42, 0xff];
    let record = super::OperationRecord {
        offset: 100,
        bytes: &bytes,
        payload_offset: 100,
        payload: &bytes,
        label,
    };
    assert_eq!(
        super::operation_body_reference(record),
        Some(super::OperationBodyReference {
            offset: 103,
            object_index: 6466,
            raw_object_index: vec![0x90, 0x19, 0x42],
        })
    );

    let duplicate = [bytes.as_slice(), bytes.as_slice()].concat();
    assert_eq!(
        super::operation_body_references(super::OperationRecord {
            offset: 100,
            bytes: &duplicate,
            payload_offset: 100,
            payload: &duplicate,
            label,
        }),
        [
            super::OperationBodyReference {
                offset: 103,
                object_index: 6466,
                raw_object_index: vec![0x90, 0x19, 0x42],
            },
            super::OperationBodyReference {
                offset: 110,
                object_index: 6466,
                raw_object_index: vec![0x90, 0x19, 0x42],
            },
        ]
    );
    assert!(super::operation_body_reference(super::OperationRecord {
        offset: 100,
        bytes: &duplicate,
        payload_offset: 100,
        payload: &duplicate,
        label,
    })
    .is_none());
}

#[test]
fn om_operation_object_relation_requires_complete_canonical_endpoints() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 100,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let payload = [
        0x01, 0x02, 0x17, 0x81, 0x23, 0x97, 0x75, 0x01, 0x02, 0x11, 0x86, 0x45, 0xff, 0x01, 0x02,
        0x10, 0x81, 0x23, 0xff,
    ];
    let record = super::OperationRecord {
        offset: 50,
        bytes: &payload,
        payload_offset: 100,
        payload: &payload,
        label,
    };
    assert_eq!(
        super::operation_object_relations(record),
        [super::OperationObjectRelation {
            offset: 100,
            link_tag: 0x17,
            first_object_index: 0x123,
            raw_first_object_index: vec![0x81, 0x23],
            first_object_index_offset: 103,
            second_object_index: 0x645,
            raw_second_object_index: vec![0x86, 0x45],
            second_object_index_offset: 110,
            end_offset: 113,
        }]
    );

    let mut noncanonical_first = payload;
    noncanonical_first[3] = 0x80;
    assert!(super::operation_object_relations(super::OperationRecord {
        bytes: &noncanonical_first,
        payload: &noncanonical_first,
        ..record
    })
    .is_empty());

    let mut truncated = payload[..13].to_vec();
    truncated.pop();
    assert!(super::operation_object_relations(super::OperationRecord {
        bytes: &truncated,
        payload: &truncated,
        ..record
    })
    .is_empty());

    let direct_body = [0x01, 0x02, 0x10, 0x81, 0x23, 0xff];
    assert!(super::operation_object_relations(super::OperationRecord {
        bytes: &direct_body,
        payload: &direct_body,
        ..record
    })
    .is_empty());

    let nested = [
        0x01, 0x02, 0x11, 0x80, 0xa9, 0x97, 0x75, 0x01, 0x02, 0x11, 0x86, 0x93, 0xff,
    ];
    let nested_relations = super::operation_object_relations(super::OperationRecord {
        bytes: &nested,
        payload: &nested,
        ..record
    });
    assert_eq!(nested_relations.len(), 1);
    assert_eq!(nested_relations[0].link_tag, 0x11);
    assert_eq!(nested_relations[0].first_object_index, 0xa9);
    assert_eq!(nested_relations[0].second_object_index, 0x693);
}

#[test]
fn om_operation_terminal_frame_requires_one_canonical_common_frame() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 100,
        value: "FSET",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let bytes = [
        0x00, 0x81, 0x5f, 0x80, 0xab, 0x01, 0x03, 0x02, 0x01, 0x02, 0x01, 0x01, 0x01, 0x00, 0x00,
        0x00, 0x81, 0x23, 0x81, 0x23, 0xff, 0x00,
    ];
    let record = super::OperationRecord {
        offset: 100,
        bytes: &bytes,
        payload_offset: 104,
        payload: &bytes,
        label,
    };
    assert_eq!(
        super::operation_terminal_frame(record),
        Some(super::OperationTerminalFrame {
            immediate_common_frame_offset: Some(104),
            local_ordinal: 0x0123,
            raw_local_ordinal: vec![0x81, 0x23],
            object_index: None,
            raw_object_index: vec![0xff],
            offset: 120,
            object_index_offset: 124,
        })
    );

    let direct = [
        0x00, 0x81, 0x5f, 0x80, 0xab, 0x01, 0x03, 0x02, 0x01, 0x02, 0x01, 0x01, 0x01, 0x00, 0x00,
        0x00, 0x29, 0x29, 0x41, 0x00,
    ];
    assert_eq!(
        super::operation_terminal_frame(super::OperationRecord {
            offset: 0,
            bytes: &direct,
            payload_offset: 200,
            payload: &direct,
            label,
        }),
        Some(super::OperationTerminalFrame {
            immediate_common_frame_offset: Some(200),
            local_ordinal: 41,
            raw_local_ordinal: vec![0x29],
            object_index: Some(65),
            raw_object_index: vec![0x41],
            offset: 216,
            object_index_offset: 218,
        })
    );

    let noncanonical = [
        0x00, 0x81, 0x5f, 0x80, 0xab, 0x01, 0x03, 0x02, 0x01, 0x02, 0x01, 0x01, 0x01, 0x00, 0x00,
        0x00, 0x80, 0x01, 0x80, 0x01, 0xff, 0x00,
    ];
    assert!(super::operation_terminal_frame(super::OperationRecord {
        offset: 0,
        bytes: &noncanonical,
        payload_offset: 0,
        payload: &noncanonical,
        label,
    })
    .is_none());
    let mismatched = [
        0x00, 0x81, 0x5f, 0x80, 0xab, 0x01, 0x03, 0x02, 0x01, 0x02, 0x01, 0x01, 0x01, 0x00, 0x00,
        0x00, 0x23, 0x24, 0xff, 0x00,
    ];
    assert!(super::operation_terminal_frame(super::OperationRecord {
        offset: 0,
        bytes: &mismatched,
        payload_offset: 0,
        payload: &mismatched,
        label,
    })
    .is_none());

    let delete = [
        0x01, 0x00, 0x00, 0x01, 0x01, 0x01, 0x06, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x29,
        0x29, 0x41, 0x00,
    ];
    let delete_frame = super::operation_terminal_frame(super::OperationRecord {
        offset: 0,
        bytes: &delete,
        payload_offset: 300,
        payload: &delete,
        label: super::OperationLabel {
            value: "DELETE",
            ..label
        },
    })
    .expect("DELETE common-frame variant");
    assert_eq!(delete_frame.immediate_common_frame_offset, Some(300));
    let [delete_common] = super::operation_common_frames(super::OperationRecord {
        offset: 0,
        bytes: &delete,
        payload_offset: 300,
        payload: &delete,
        label: super::OperationLabel {
            value: "DELETE",
            ..label
        },
    })
    .try_into()
    .expect("one DELETE common frame");
    assert_eq!(delete_common.indices, [1, 0, 0]);
    assert_eq!(delete_common.marker, [1, 1, 1]);
    assert_eq!(delete_common.state, [6, 1, 1, 0, 1, 0, 0, 0]);

    let suffix_only = [0x02, 0x02, 0xff, 0x00];
    let suffix = super::operation_terminal_frame(super::OperationRecord {
        offset: 0,
        bytes: &suffix_only,
        payload_offset: 400,
        payload: &suffix_only,
        label,
    })
    .expect("canonical suffix without immediate state prefix");
    assert!(suffix.immediate_common_frame_offset.is_none());
    assert_eq!(suffix.local_ordinal, 2);
    assert_eq!(suffix.offset, 400);

    let mut embedded = direct.to_vec();
    embedded.extend_from_slice(&[0xaa, 0x02, 0x02, 0xff, 0x00]);
    let embedded_record = super::OperationRecord {
        offset: 0,
        bytes: &embedded,
        payload_offset: 500,
        payload: &embedded,
        label,
    };
    let [common] = super::operation_common_frames(embedded_record)
        .try_into()
        .expect("one embedded common frame");
    assert_eq!(common.offset, 500);
    assert_eq!(common.end_offset, 520);
    let outer = super::operation_terminal_frame(embedded_record).expect("outer suffix");
    assert_eq!(outer.offset, 521);
    assert!(outer.immediate_common_frame_offset.is_none());
}

#[test]
fn om_fset_reference_graph_requires_exact_groups_and_bounds() {
    fn record(payload: &[u8]) -> super::OperationRecord<'_> {
        super::OperationRecord {
            offset: 0,
            bytes: payload,
            payload_offset: 100,
            payload,
            label: super::OperationLabel {
                header_offset: 0,
                offset: 0,
                value: "FSET",
                object_indices: [None; 4],
                object_index_offsets: [0; 4],
            },
        }
    }

    let payload = [
        0x01, 0x13, 0x3c, b'T', b';', b':', b'S', b'5', b'6', b'7', b'R', b'8', b'9', b'3', 0x90,
        0x19, 0x40, 0x90, 0x19, 0x41, 0x3e, 0x90, 0x19, 0x30, 0x90, 0x19, 0x31, 0x90, 0x19, 0x32,
        0x00, 0x03, 0x00,
    ];
    let graph = super::fset_payload_reference_graph(record(&payload)).unwrap();
    assert_eq!(graph.selector, "T;:S567R893");
    assert_eq!(graph.offset, 100);
    assert_eq!(
        graph
            .first
            .each_ref()
            .map(|reference| reference.object_index),
        [6464, 6465]
    );
    assert_eq!(
        graph
            .second
            .each_ref()
            .map(|reference| reference.object_index),
        [6448, 6449, 6450]
    );
    assert_eq!(
        graph
            .first
            .each_ref()
            .map(|reference| reference.raw_object_index.as_slice()),
        [[0x90, 0x19, 0x40].as_slice(), [0x90, 0x19, 0x41].as_slice(),]
    );

    let mut wrong_length = payload;
    wrong_length[1] -= 1;
    assert!(super::fset_payload_reference_graph(record(&wrong_length)).is_none());
    let mut wrong_suffix = payload;
    wrong_suffix[31] = 0x04;
    assert!(super::fset_payload_reference_graph(record(&wrong_suffix)).is_none());
    let mut wrong_reference_form = payload;
    wrong_reference_form[14] = 0xf1;
    assert!(super::fset_payload_reference_graph(record(&wrong_reference_form)).is_none());
    let duplicate = [payload.as_slice(), payload.as_slice()].concat();
    assert!(super::fset_payload_reference_graph(record(&duplicate)).is_none());
}

#[test]
fn om_delete_reference_field_requires_five_canonical_nullable_slots() {
    fn record(payload: &[u8]) -> super::OperationRecord<'_> {
        super::OperationRecord {
            offset: 0,
            bytes: payload,
            payload_offset: 100,
            payload,
            label: super::OperationLabel {
                header_offset: 0,
                offset: 0,
                value: "DELETE",
                object_indices: [None; 4],
                object_index_offsets: [0; 4],
            },
        }
    }

    let payload = [
        0x0c, 0x00, 0x00, 0x01, 0x00, 0x01, 0x06, 0xf0, 0x20, 0xff, 0xf1, 0x02, 0x08, 0xf1, 0x02,
        0x09, 0xff, 0x00,
    ];
    let field = super::delete_payload_references(record(&payload)).unwrap();
    assert_eq!(field.control, 0x0c);
    assert_eq!(field.offset, 100);
    assert_eq!(
        field
            .references
            .each_ref()
            .map(|reference| reference.object_index),
        [Some(0x20), None, Some(0x208), Some(0x209), None]
    );
    assert_eq!(
        field
            .references
            .each_ref()
            .map(|reference| reference.offset),
        [107, 109, 110, 113, 116]
    );

    let mut noncanonical = payload;
    noncanonical[11] = 0x00;
    noncanonical[12] = 0x20;
    assert!(super::delete_payload_references(record(&noncanonical)).is_none());
    let truncated = &payload[..payload.len() - 1];
    assert!(super::delete_payload_references(record(truncated)).is_none());
    let mut wrong_count = payload;
    wrong_count[6] = 0x05;
    assert!(super::delete_payload_references(record(&wrong_count)).is_none());
}

#[test]
fn om_data_block_object_references_require_complete_field_frames() {
    let bytes = [
        0x04, 0x00, 0x2a, 0x02, 0x0b, 0xff, 0x04, 0x00, 0x80, 0xc9, 0x02, 0x0b, 0x04, 0x00, 0x90,
        0x19, 0x42, 0x02, 0x0b,
    ];
    assert_eq!(
        super::data_block_object_references(&bytes),
        [
            super::DataBlockObjectReference {
                offset: 2,
                object_index: 42,
                raw_object_index: vec![0x2a],
            },
            super::DataBlockObjectReference {
                offset: 8,
                object_index: 201,
                raw_object_index: vec![0x80, 0xc9],
            },
            super::DataBlockObjectReference {
                offset: 14,
                object_index: 6466,
                raw_object_index: vec![0x90, 0x19, 0x42],
            },
        ]
    );
    assert_eq!(
        super::data_block_object_references(&bytes[..bytes.len() - 1]).len(),
        2
    );
}

#[test]
fn om_size_frame_bounds_its_type_declarations() {
    let bytes = size_framed_om_section();
    let sections = super::sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].offset, 0);
    assert_eq!(sections[0].byte_len, bytes.len());
    assert_eq!(sections[0].types.len(), 2);
    assert_eq!(sections[0].types[0].name, "UGS::FEATURE_RECORD");
    assert_eq!(
        sections[0].types[0].registry_suffix,
        &[0x81, 0x21, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x06]
    );
    assert_eq!(sections[0].types[1].trailing_code, 0x65);
    assert_eq!(sections[0].fields.len(), 2);
    assert_eq!(sections[0].fields[0].name, "m_target");
    assert_eq!(sections[0].fields[1].trailing_code, 0x81);
    assert_eq!(sections[0].record_area, None);

    let mut truncated = bytes;
    truncated.pop();
    assert!(super::sections(&truncated).is_empty());
}

#[test]
fn om_size_frame_accepts_exact_terminal_twelve_byte_envelope() {
    let mut bytes = size_framed_om_section();
    let payload_len = u32::try_from(bytes.len() - 12).expect("short OM fixture");
    bytes[8..12].copy_from_slice(&payload_len.to_be_bytes());
    let sections = super::sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].byte_len, bytes.len());
    assert_eq!(sections[0].types[0].name, "UGS::FEATURE_RECORD");

    bytes.push(0);
    assert!(super::sections(&bytes).is_empty());
}

#[test]
fn om_size_frame_uses_validated_internal_record_area_pointer() {
    let bytes = size_framed_om_section_with_record_area();
    let section = super::sections(&bytes).remove(0);
    let offset = section.record_area_offset.expect("record area");
    assert_eq!(offset, size_framed_om_section().len() + 20);
    assert_eq!(section.record_area.unwrap(), &bytes[offset..]);
    assert_eq!(&bytes[offset + 12..offset + 15], &[0x05, 0x01, 0x0e]);

    let mut invalid = bytes;
    invalid[offset + 12] = 1;
    assert_eq!(super::sections(&invalid)[0].record_area, None);
}

#[test]
fn om_registry_uses_the_bounded_record_area_as_its_registry_end() {
    let mut bytes = size_framed_om_section();
    bytes.extend(std::iter::repeat_n(0xa5, 4097));
    bytes.extend_from_slice(&[
        (b"m_lateField".len() + 1) as u8,
        b'm',
        b'_',
        b'l',
        b'a',
        b't',
        b'e',
        b'F',
        b'i',
        b'e',
        b'l',
        b'd',
        0x82,
    ]);
    let pointer_offset = bytes.len();
    let record_area_offset = pointer_offset + 20;
    bytes.extend_from_slice(&(record_area_offset as u32).to_le_bytes());
    bytes.resize(record_area_offset, 0);
    bytes.extend_from_slice(&[13, 0, 0, 0, 14, 0, 0, 0, 44, 0, 0, 0]);
    bytes.extend_from_slice(b"\x05\x01\x0eNX 2027.3102\0");
    let payload_len = u32::try_from(bytes.len() - 16).expect("synthetic section fits");
    bytes[8..12].copy_from_slice(&payload_len.to_be_bytes());

    let section = super::sections(&bytes).remove(0);
    assert_eq!(
        section.fields.last().expect("late field").name,
        "m_lateField"
    );
    assert_eq!(section.record_area_offset, Some(record_area_offset));
}

#[test]
fn om_operation_labels_require_the_complete_frame() {
    let bytes = b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\x01\x82\x40\x90\x17\xd3\xff\x03\x07UNITE\0\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\x02\x03\xff\xff\x03\x08SKETCH\0";
    let labels = super::operation_labels(bytes, 100);
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[0].offset, 122);
    assert_eq!(labels[0].header_offset, 100);
    assert_eq!(labels[0].value, "UNITE");
    assert_eq!(
        labels[0].object_indices,
        [Some(1), Some(576), Some(6099), None]
    );
    assert_eq!(labels[1].value, "SKETCH");
    assert_eq!(labels[1].object_indices, [Some(2), Some(3), None, None]);

    assert!(super::operation_labels(b"\xff\xff\x03\x07UNITE\0", 0).is_empty());
    let mut invalid = bytes.to_vec();
    invalid[15] = 0x91;
    assert_eq!(super::operation_labels(&invalid, 0).len(), 1);
}

#[test]
fn om_operation_records_use_consecutive_validated_headers() {
    let bytes = b"prefix\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x07UNITE\0payload\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x08SKETCH\0tail";
    let records = super::operation_records(bytes, 10);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].offset, 16);
    assert_eq!(records[0].label.value, "UNITE");
    assert!(records[0].bytes.ends_with(b"payload"));
    assert_eq!(records[0].payload, b"payload");
    assert_eq!(records[0].payload_offset, 43);
    assert_eq!(records[1].label.value, "SKETCH");
    assert!(records[1].bytes.ends_with(b"tail"));
    assert_eq!(records[1].payload, b"tail");
}

#[test]
fn om_operation_payload_strings_require_complete_utf8_frames() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SIMPLE HOLE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x00\x04\x07BLOCK\0\x04\x04\xc3\x97\0\x04\x07BROKEN";
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let strings = super::operation_payload_strings(record);
    assert_eq!(strings.len(), 2);
    assert_eq!(strings[0].offset, 201);
    assert_eq!(strings[0].value, "BLOCK");
    assert_eq!(strings[1].value, "×");
}

#[test]
fn om_surface_payload_strings_require_exact_length_utf8_and_terminator() {
    let bytes = b"\x66\x1b\x03\x05Steel\0\xaa\x66\x1b\x03\x02\xc3\x97\0";
    let strings = super::surface_payload_strings(bytes);
    assert_eq!(strings.len(), 2);
    assert_eq!(strings[0].offset, 0);
    assert_eq!(strings[0].value, "Steel");
    assert_eq!(strings[1].offset, 11);
    assert_eq!(strings[1].value, "×");

    let truncated = b"\x66\x1b\x03\x05Steel";
    assert!(super::surface_payload_strings(truncated).is_empty());
    let invalid_utf8 = b"\x66\x1b\x03\x01\xff\0";
    assert!(super::surface_payload_strings(invalid_utf8).is_empty());
    let control = b"\x66\x1b\x03\x01\n\0";
    assert!(super::surface_payload_strings(control).is_empty());
}

#[test]
fn om_projected_curve_references_require_one_complete_field() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "CPROJ",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload =
        b"\0\x01\x02\xf1\x02\xc8\xf1\x02\xc9\x80\x57\x00\x02\x01\xf1\x02\xca\xff\x01\x02\x02\x7d\0";
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = super::projected_curve_payload_references(record).expect("complete field");
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| (reference.object_index, reference.offset))
            .collect::<Vec<_>>(),
        [(712, 203), (713, 206), (714, 214)]
    );

    let mut malformed = payload.to_vec();
    malformed[17] = 0x00;
    assert!(
        super::projected_curve_payload_references(super::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(
        super::projected_curve_payload_references(super::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_combined_projected_curve_references_require_the_complete_graph() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "CPROJ_CMB",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x3c\x32\x01\x02\x32\x01\x04\x36\x01\x33\xf1\x03\x18\x33\xf1\x03\x19\x00\xf1\x03\x1a\x00\x00\x00\x00\x00\x00\xf1\x03\x1b\x16\x01\x02\xf1\x03\x18\x01\x02\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x03\x1c\x00\x81\x5c\x16\x01\x02\xf1\x03\x19\x01\x02\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x03\x1d\x00\x81\x5c\xff\x01\xff\x01\xf1\x03\x1e\xf1\x03\x1f\x04\x02";
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = super::projected_curve_payload_references(record).expect("complete graph");
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| (reference.object_index, reference.offset))
            .collect::<Vec<_>>(),
        [
            (792, 210),
            (793, 214),
            (794, 218),
            (795, 227),
            (796, 246),
            (797, 268),
            (798, 278),
            (799, 281),
        ]
    );

    let mut inconsistent = payload.to_vec();
    inconsistent[35] = 0x19;
    assert!(
        super::projected_curve_payload_references(super::OperationRecord {
            bytes: &inconsistent,
            payload: &inconsistent,
            ..record
        })
        .is_none()
    );

    let mut malformed = payload.to_vec();
    malformed[84] = 0x00;
    assert!(
        super::projected_curve_payload_references(super::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(
        super::projected_curve_payload_references(super::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_pattern_reference_graph_preserves_nullable_terminal_slot() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "Pattern Geometry",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let nullable = b"\x61\xf1\x1b\x08\xff\x00\xff\x01\xf1\x1b\x09\xf1\x1b\x0a\x61\xf1\x1b\x0b\xff\x00\xff\x01\xf1\x1b\x0c\xf1\x1b\x0d\xff\x62\xf1\x1b\x0e\xf1\x1b\x0f\xff\x00\x00\x01\xf1\x1b\x10\xff\xff\xff\x01";
    let record = super::OperationRecord {
        offset: 100,
        bytes: nullable,
        payload_offset: 200,
        payload: nullable,
        label,
    };
    let field = super::pattern_payload_references(record).expect("complete graph");
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>(),
        (6920..=6928).collect::<Vec<_>>()
    );

    let populated = [&nullable[..nullable.len() - 4], b"\xf1\x1b\x11\xff\xff\x01"].concat();
    let field = super::pattern_payload_references(super::OperationRecord {
        label: super::OperationLabel {
            value: "Pattern Feature",
            ..label
        },
        bytes: &populated,
        payload: &populated,
        ..record
    })
    .expect("populated terminal slot");
    assert_eq!(field.references.len(), 10);
    assert_eq!(field.references[9].object_index, 6929);

    let mut malformed = nullable.to_vec();
    malformed[18] = 0x60;
    assert!(super::pattern_payload_references(super::OperationRecord {
        bytes: &malformed,
        payload: &malformed,
        ..record
    })
    .is_none());
}

#[test]
fn om_pattern_transform_lanes_require_counted_family_rows() {
    let feature_payload = b"\xaa\x01\x03\x60\x01\x00\x00\x50\x54\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x02\x01\x01\x00\x00\xff\x00\x00\x60\x01\x00\x00\xd0\x54\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x9f\xfe\x01\x02\x00\x00\xff\x00\x00\x5f\x00\x00\x01";
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "Pattern Feature",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let record = super::OperationRecord {
        offset: 100,
        bytes: feature_payload,
        payload_offset: 200,
        payload: feature_payload,
        label,
    };
    let lane = super::pattern_payload_transform_lane(record).expect("feature lane");
    assert_eq!(lane.offset, 201);
    assert_eq!(lane.row_schema_index, 0x60);
    assert_eq!(lane.layout, super::PatternTransformLayout::ScalarRows);
    assert_eq!(lane.declared_count, 3);
    assert_eq!(
        lane.encodings,
        [
            super::PatternTransformEncoding::Binary32,
            super::PatternTransformEncoding::Binary32,
        ]
    );
    assert_eq!(lane.values, [3.3125, -3.3125]);
    assert_eq!(lane.value_offsets, [207, 237]);
    assert_eq!(lane.selectors, [2, 8190]);
    assert_eq!(lane.raw_selectors, [vec![0x02], vec![0x9f, 0xfe]]);
    assert_eq!(lane.selector_offsets, [225, 255]);

    let geometry_payload = b"\x01\x03\x60\x01\x00\x00\x00\x00\x01\x00\x30\x60\x80\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x02\x01\x01\x00\x00\xff\x00\x00\x60\x01\x00\x00\x00\x00\x01\x00\x30\x70\x80\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x03\x01\x02\x00\x00\xff\x00\x00\x5f\x00\x00\x01";
    let geometry_record = super::OperationRecord {
        label: super::OperationLabel {
            value: "Pattern Geometry",
            ..label
        },
        bytes: geometry_payload,
        payload: geometry_payload,
        ..record
    };
    let lane = super::pattern_payload_transform_lane(geometry_record).expect("geometry lane");
    assert_eq!(lane.row_schema_index, 0x60);
    assert_eq!(lane.layout, super::PatternTransformLayout::ScalarRows);
    assert_eq!(
        lane.encodings,
        [
            super::PatternTransformEncoding::Binary64,
            super::PatternTransformEncoding::Binary64,
        ]
    );
    assert_eq!(lane.values, [132.0, 264.0]);
    assert_eq!(lane.selectors, [2, 3]);
    assert_eq!(lane.raw_selectors, [vec![0x02], vec![0x03]]);
    assert_eq!(lane.selector_offsets, [228, 262]);

    let schema_relative_payload = b"\x01\x04\
        \x3d\x01\x00\x00\x50\x9e\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x02\x01\x01\x00\x00\xff\x00\x00\
        \x3d\x01\x00\x00\x50\xae\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x03\x01\x02\x00\x00\xff\x00\x00\
        \x3d\x01\x00\x00\x30\xb6\x80\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x04\x01\x03\x00\x00\xff\x00\x00\
        \x3c\x00\x00\x01";
    let relative_lane = super::pattern_payload_transform_lane(super::OperationRecord {
        bytes: schema_relative_payload,
        payload: schema_relative_payload,
        ..record
    })
    .expect("schema-relative feature lane");
    assert_eq!(relative_lane.row_schema_index, 0x3d);
    assert_eq!(
        relative_lane.layout,
        super::PatternTransformLayout::ScalarRows
    );
    assert_eq!(relative_lane.declared_count, 4);
    assert_eq!(
        relative_lane.encodings,
        [
            super::PatternTransformEncoding::Binary32,
            super::PatternTransformEncoding::Binary32,
            super::PatternTransformEncoding::Binary64,
        ]
    );
    assert_eq!(relative_lane.selectors, [2, 3, 4]);

    let wide_payload = b"\x01\x03\
        \x35\x2f\xf3\xc6\xef\x37\x2f\xe9\x60\xb0\x0e\x6f\x0e\x13\x44\x54\xfd\x00\x00\x30\x0e\x6f\x0e\x13\x44\x54\xfd\x2f\xf3\xc6\xef\x37\x2f\xe9\x60\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x02\x01\x01\x00\x00\xff\x00\x00\
        \x35\xb0\x09\xe3\x77\x9b\x97\xf4\xb9\x30\x02\xcf\x23\x04\x75\x5a\x46\x00\x00\xb0\x02\xcf\x23\x04\x75\x5a\x46\xb0\x09\xe3\x77\x9b\x97\xf4\xb9\x00\x00\x00\x00\x50\x0f\xff\xff\x00\x00\x00\x00\x01\x01\x03\x03\x01\x02\x00\x00\xff\x00\x00\
        \x34\x00\x00\x02";
    let wide_lane = super::pattern_payload_transform_lane(super::OperationRecord {
        bytes: wide_payload,
        payload: wide_payload,
        ..record
    })
    .expect("wide feature lane");
    assert_eq!(wide_lane.row_schema_index, 0x35);
    assert_eq!(wide_lane.layout, super::PatternTransformLayout::WideRows);
    assert_eq!(wide_lane.declared_count, 3);
    assert_eq!(wide_lane.values.len(), 10);
    assert_eq!(
        wide_lane.encodings,
        [
            super::PatternTransformEncoding::Binary64,
            super::PatternTransformEncoding::Binary64,
            super::PatternTransformEncoding::Binary64,
            super::PatternTransformEncoding::Binary64,
            super::PatternTransformEncoding::ExactOne,
            super::PatternTransformEncoding::Binary64,
            super::PatternTransformEncoding::Binary64,
            super::PatternTransformEncoding::Binary64,
            super::PatternTransformEncoding::Binary64,
            super::PatternTransformEncoding::Binary32,
        ]
    );
    assert_eq!(wide_lane.selectors, [2, 3]);

    let mut zero_terminal_value = wide_payload.to_vec();
    let terminal_value = zero_terminal_value
        .windows(12)
        .position(|bytes| {
            bytes
                == [
                    0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x03,
                ]
        })
        .expect("exact-one terminal value")
        + 4;
    zero_terminal_value[terminal_value] = 0x00;
    assert!(
        super::pattern_payload_transform_lane(super::OperationRecord {
            bytes: &zero_terminal_value,
            payload: &zero_terminal_value,
            ..record
        })
        .is_none()
    );

    let mut changed_schema = feature_payload.to_vec();
    let second_row = changed_schema
        .windows(4)
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == [0x60, 0x01, 0x00, 0x00]).then_some(offset))
        .nth(1)
        .expect("second row");
    changed_schema[second_row] = 0x61;
    assert!(
        super::pattern_payload_transform_lane(super::OperationRecord {
            bytes: &changed_schema,
            payload: &changed_schema,
            ..record
        })
        .is_none()
    );

    let mut wrong_ordinal = feature_payload.to_vec();
    wrong_ordinal[29] = 2;
    assert!(
        super::pattern_payload_transform_lane(super::OperationRecord {
            bytes: &wrong_ordinal,
            payload: &wrong_ordinal,
            ..record
        })
        .is_none()
    );
    assert!(
        super::pattern_payload_transform_lane(super::OperationRecord {
            bytes: &feature_payload[..feature_payload.len() - 1],
            payload: &feature_payload[..feature_payload.len() - 1],
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_multi_instance_output_lane_requires_consistent_counts_and_groups() {
    let mut payload = b"\xaa\x3a\x00\x00\x01\x00\x00\x00\x00\x25\x01\x07".to_vec();
    let mut row_index = 2;
    for selector in [2, 3, 4] {
        for ordinal in 2..=3 {
            payload.extend_from_slice(b"\x26\x27\x01\x02\x65\x01\x02");
            payload.extend_from_slice(&[selector, 0x28, ordinal, row_index]);
            row_index += 1;
        }
    }
    payload.extend_from_slice(b"\x00\x3b\x90\x3d\xea\x90\x3d\xeb\x01\x03\xbb");
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "Multi Instance Output",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let record = super::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label,
    };
    let lane = super::multi_instance_output_payload_lane(record).expect("complete output lane");
    assert_eq!(lane.offset, 209);
    assert_eq!(lane.declared_count, 7);
    assert_eq!(lane.selectors, [2, 2, 3, 3, 4, 4]);
    assert_eq!(lane.ordinals, [2, 3, 2, 3, 2, 3]);
    assert_eq!(lane.row_indices, [2, 3, 4, 5, 6, 7]);
    assert_eq!(lane.instance_count, 3);
    assert_eq!(lane.selector_offsets, [219, 230, 241, 252, 263, 274]);
    assert_eq!(
        lane.trailing_references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>(),
        [15850, 15851]
    );
    assert_eq!(lane.trailing_references[0].offset, 280);
    assert_eq!(
        lane.trailing_references[0].raw_object_index,
        [0x90, 0x3d, 0xea]
    );

    let mut incomplete_group = payload.clone();
    incomplete_group[76] = 2;
    assert!(
        super::multi_instance_output_payload_lane(super::OperationRecord {
            bytes: &incomplete_group,
            payload: &incomplete_group,
            ..record
        })
        .is_none()
    );
    let mut wrong_row_index = payload.clone();
    wrong_row_index[77] = 6;
    assert!(
        super::multi_instance_output_payload_lane(super::OperationRecord {
            bytes: &wrong_row_index,
            payload: &wrong_row_index,
            ..record
        })
        .is_none()
    );
    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(
        super::multi_instance_output_payload_lane(super::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_identical_instance_output_lane_requires_complete_ordered_rows() {
    let payload = b"\xaa\x34\x13\x01\x04\x14\x15\x01\x02\x16\x80\x20\x00\x02\
          \x14\x15\x01\x02\x16\x0f\x00\x03\
          \x14\x15\x01\x02\x16\x81\x23\x00\x04\
          \x00\x05\xe0\x7f\xff\xff\xff\x00\x00\xbb";
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "IDENTICAL INSTANCE OUTPUT",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let lane = super::identical_instance_output_payload_lane(record)
        .expect("complete identical-instance lane");
    assert_eq!(lane.offset, 201);
    assert_eq!(lane.leading_schema_index, 0x34);
    assert_eq!(lane.count_schema_index, 0x13);
    assert_eq!(lane.row_schema_indices, [0x14, 0x15, 0x16]);
    assert_eq!(lane.declared_count, 4);
    assert_eq!(lane.selectors, [0x20, 0x0f, 0x123]);
    assert_eq!(
        lane.raw_selectors,
        [vec![0x80, 0x20], vec![0x0f], vec![0x81, 0x23]]
    );
    assert_eq!(lane.selector_offsets, [210, 219, 227]);

    let mut wrong_ordinal = payload.to_vec();
    wrong_ordinal[21] = 4;
    assert!(
        super::identical_instance_output_payload_lane(super::OperationRecord {
            bytes: &wrong_ordinal,
            payload: &wrong_ordinal,
            ..record
        })
        .is_none()
    );
    let mut wrong_terminal_count = payload.to_vec();
    wrong_terminal_count[32] = 4;
    assert!(
        super::identical_instance_output_payload_lane(super::OperationRecord {
            bytes: &wrong_terminal_count,
            payload: &wrong_terminal_count,
            ..record
        })
        .is_none()
    );
    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(
        super::identical_instance_output_payload_lane(super::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_geometry_instance_reference_requires_one_complete_field() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "Geometry Instance",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x44\x45\x00\xff\xff\xf1\x03\x21\x01\x02\x00\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x01\x02";
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = super::pattern_payload_references(record).expect("complete field");
    assert_eq!(field.references[0].object_index, 801);
    assert_eq!(field.references[0].offset, 205);

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(super::pattern_payload_references(super::OperationRecord {
        bytes: &ambiguous,
        payload: &ambiguous,
        ..record
    })
    .is_none());
}

#[test]
fn om_point_feature_header_requires_the_complete_leading_envelope() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "POINT",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x72\x00\x00\x01\x00\x00\x00\xf1\x1c\x8f\x00\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x0d\x01\x02\x01\x00\x00\x00\x89\x02\x01\x01\x01\x00\xa5\x57\x95\x01\x00\x00\xff\x02\xc0\x1f\xff\xfd\x01\x00\x00\x01\x01\x01\x03\x02\x01\x01\x01\x00\x00\x00\x00\x00\xaa";
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let header = super::point_feature_payload_header(record).expect("complete header");
    assert_eq!(header.reference.object_index, 7311);
    assert_eq!(header.reference.offset, 207);
    assert_eq!(header.mode, 0x02);

    let mut alternate_mode = payload.to_vec();
    alternate_mode[52] = 0x03;
    assert_eq!(
        super::point_feature_payload_header(super::OperationRecord {
            bytes: &alternate_mode,
            payload: &alternate_mode,
            ..record
        })
        .expect("alternate mode")
        .mode,
        0x03
    );

    for malformed_offset in [0, 10, 51, 72] {
        let mut malformed = payload.to_vec();
        malformed[malformed_offset] ^= 0x01;
        assert!(super::point_feature_payload_header(super::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none());
    }
    let mut unsupported_mode = payload.to_vec();
    unsupported_mode[52] = 0x04;
    assert!(super::point_feature_payload_header(super::OperationRecord {
        bytes: &unsupported_mode,
        payload: &unsupported_mode,
        ..record
    })
    .is_none());
    assert!(super::point_feature_payload_header(super::OperationRecord {
        bytes: &payload[..72],
        payload: &payload[..72],
        ..record
    })
    .is_none());
}

#[test]
fn om_point_feature_scalar_lane_spans_the_preceding_block_atomically() {
    let mut encoded = Vec::new();
    for value in [1.0_f64, -2.0, 3.5, 4.0, 5.25, -6.0] {
        let mut bytes = value.to_be_bytes();
        bytes[0] -= 0x10;
        encoded.extend_from_slice(&bytes);
    }
    let preceding = [vec![0xaa, 0xbb], encoded[..3].to_vec()].concat();
    let mut target = encoded[3..].to_vec();
    target.extend_from_slice(&[
        0x00, 0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x07, 0x01, 0xc0, 0x45, 0x10, 0x00, 0x80, 0x86,
        0x02, 0x00, 0x01, 0x00,
    ]);
    target.push(0xcc);

    let lane = super::point_feature_scalar_lane(&preceding, &target).expect("complete lane");
    assert_eq!(lane.values, [1.0, -2.0, 3.5, 4.0, 5.25, -6.0]);
    assert_eq!(lane.raw_values.concat(), encoded);
    assert_eq!(lane.value_offsets, [2, 10, 18, 26, 34, 42]);

    let mut malformed = target.clone();
    malformed[45] = 0x01;
    assert!(super::point_feature_scalar_lane(&preceding, &malformed).is_none());
    assert!(super::point_feature_scalar_lane(&preceding[..2], &target).is_none());
    assert!(super::point_feature_scalar_lane(&preceding, &target[..63]).is_none());

    let mut nonfinite = target;
    nonfinite[5..13].copy_from_slice(&[0x6f, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    assert!(super::point_feature_scalar_lane(&preceding, &nonfinite).is_none());
}

#[test]
fn om_draft_feature_references_require_one_complete_graph() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "DRAFT",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let prefix = b"\x67\x00\x00\x01\x00\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\xff\xff\xff\xff\xff\xff\xff\xff\x01\x03\x80\x94\x82\x49";
    let graph = b"\x01\x02\xf1\x1b\x7c\x01\x02\xf1\x1b\x7d\x68\x2f\x70\x62\x4d\xd2\xf1\xa9\xfc\x03\x50\x44\x00\x00\x01\x46\x8a\x2a\x01\xa3\x60\x10\x01\x01\x01\x04\x02\x01\x02\x01\x00\x00\x00\x00\x01\xf1\x1b\x7e\xff\x00\x00\x00\xf1\x1b\x7f\xff";
    let terminal = b"\x81\x5e\x80\xb8\x01\x03\x02\x01\x02\x01\x01\x01\x00\x00\x00\x29\x29\x0c\x00";
    let payload = [prefix.as_slice(), graph.as_slice(), terminal.as_slice()].concat();
    let record = super::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label,
    };
    let field = super::draft_feature_payload_references(record).expect("complete graph");
    assert_eq!(
        field
            .references
            .clone()
            .map(|reference| reference.object_index),
        [7036, 7037, 7038, 7039]
    );
    assert_eq!(
        field.references.map(|reference| reference.offset),
        [230, 235, 273, 280]
    );
    let lane = super::draft_feature_leading_index_lane(record).expect("complete index lane");
    assert_eq!(lane.declared_count, 3);
    assert_eq!(lane.indices, vec![(148, 224), (585, 226)]);
    assert_eq!(lane.raw_indices, vec![vec![0x80, 0x94], vec![0x82, 0x49]]);
    let terminal_lane = super::draft_feature_terminal_lane(record).expect("complete terminal lane");
    assert_eq!(terminal_lane.indices, [350, 184]);
    assert_eq!(terminal_lane.raw_indices, [[0x81, 0x5e], [0x80, 0xb8]]);
    assert_eq!(terminal_lane.index_offsets, [284, 286]);
    assert_eq!(terminal_lane.tail, [0x29, 0x29, 0x0c]);
    assert_eq!(terminal_lane.offset, 284);

    let mut malformed = payload.clone();
    malformed[53] = 0x00;
    assert!(
        super::draft_feature_payload_references(super::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );
    let mut malformed_lane = payload.clone();
    malformed_lane[23] = 4;
    assert!(
        super::draft_feature_leading_index_lane(super::OperationRecord {
            bytes: &malformed_lane,
            payload: &malformed_lane,
            ..record
        })
        .is_none()
    );
    let ambiguous = [prefix.as_slice(), graph.as_slice(), graph.as_slice()].concat();
    assert!(
        super::draft_feature_payload_references(super::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
    assert!(
        super::draft_feature_payload_references(super::OperationRecord {
            bytes: &payload[..prefix.len() + graph.len() - 2],
            payload: &payload[..prefix.len() + graph.len() - 2],
            ..record
        })
        .is_none()
    );
    assert!(super::draft_feature_terminal_lane(super::OperationRecord {
        bytes: &payload[..payload.len() - 1],
        payload: &payload[..payload.len() - 1],
        ..record
    })
    .is_none());
}

#[test]
fn om_surface_feature_references_require_the_complete_common_envelope() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SKIN",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x3f\x00\x00\x01\x00\xf1\x02\x46\xf1\x02\x47\xf1\x02\x48\x01\x09\x03\x03\x04\x05\x02\x01\x01\x01\x01\x09\xf1\x02\x49\xf1\x02\x4a\xf1\x02\x4b\xf1\x02\x4c\xf1\x02\x4d\xf1\x02\x4e\xf1\x02\x4f\xf1\x02\x50\x00\x03\x03\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xf1\x02\x56\xf1\x02\x57\xf1\x02\x58\x01\x01\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x01\x02";
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = super::surface_feature_payload_references(record).expect("complete envelope");
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>(),
        [582, 583, 584, 585, 586, 587, 588, 589, 590, 591, 592, 598, 599, 600,]
    );

    let studio_payload = [&[0x14], &payload[1..]].concat();
    let studio = super::OperationRecord {
        label: super::OperationLabel {
            value: "Studio Surface",
            ..label
        },
        bytes: &studio_payload,
        payload: &studio_payload,
        ..record
    };
    assert!(super::surface_feature_payload_references(studio).is_some());

    let mut malformed = payload.to_vec();
    let last = malformed.len() - 1;
    malformed[last] = 0x00;
    assert!(
        super::surface_feature_payload_references(super::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let ambiguous = [payload.as_slice(), &payload[51..]].concat();
    assert!(
        super::surface_feature_payload_references(super::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_surface_feature_branches_require_one_complete_counted_group() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SKIN",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\xa0\x5a\x14\x13\x01\x02\x40\x01\x04\xf1\x1b\xf4\xf1\x1b\xf5\xf1\x1b\xf6\x01\x04\x00\x00\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x1b\xf7\x00\x81\x58\x01\x02\x40\x01\x05\xf1\x1b\xf8\xf1\x1b\xf9\xf1\x1b\xfa\xf1\x1b\xfb\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x1b\xfc\x00\x81\x1c\x00\x00\x00\x01\x03\x00\x00\x00\xff\xff\x01";
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let group = super::surface_feature_payload_branches(record).expect("complete group");
    assert_eq!(group.family, 0x14);
    assert_eq!(group.header_code, 0x13);
    assert_eq!(group.branches.len(), 2);
    assert_eq!(group.branches[0].mode, 0x40);
    assert_eq!(group.branches[0].declared_count, 4);
    assert!(group.branches[0].witnessed);
    assert_eq!(group.branches[0].members.len(), 3);
    assert_eq!(group.branches[0].terminal.object_index, 7159);
    assert_eq!(group.branches[0].suffix, [0x81, 0x58, 0x01, 0x02]);
    assert_eq!(group.branches[1].declared_count, 5);
    assert!(!group.branches[1].witnessed);
    assert_eq!(group.branches[1].members.len(), 4);
    assert_eq!(group.branches[1].terminal.object_index, 7164);
    assert_eq!(group.branches[1].suffix, [0x81, 0x1c]);

    let studio_payload = [
        &payload[..payload.len() - 11],
        &[0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x01],
    ]
    .concat();
    let studio = super::OperationRecord {
        label: super::OperationLabel {
            value: "Studio Surface",
            ..label
        },
        bytes: &studio_payload,
        payload: &studio_payload,
        ..record
    };
    assert!(super::surface_feature_payload_branches(studio).is_some());

    let mut malformed = payload.to_vec();
    malformed[19] = 0x03;
    assert!(
        super::surface_feature_payload_branches(super::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(
        super::surface_feature_payload_branches(super::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_sketch_payload_reference_field_is_counted_ordered_and_canonical() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SKETCH",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x01\x00\x01\x05\xf0\xff\xf1\x01\x00\xf1\x01\x01\xf1\x01\x02\x00\x00\xf1\x01\x03\x01\x00\x00\x00";
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = super::sketch_payload_references(record).unwrap();
    assert_eq!(field.declared_count, 5);
    let references: [super::PayloadObjectReference; 5] =
        field.references.clone().try_into().unwrap();
    assert_eq!(
        references.clone().map(|reference| reference.object_index),
        [255, 256, 257, 258, 259]
    );
    assert_eq!(
        references.map(|reference| reference.offset),
        [204, 206, 209, 212, 217]
    );
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.raw_object_index.as_slice())
            .collect::<Vec<_>>(),
        [
            &[0xf0, 0xff][..],
            &[0xf1, 0x01, 0x00][..],
            &[0xf1, 0x01, 0x01][..],
            &[0xf1, 0x01, 0x02][..],
            &[0xf1, 0x01, 0x03][..],
        ]
    );
    let zero = b"\x01\x00\x00\x00\x00\xf0\x42\x01\x00\x00\x00";
    let field = super::sketch_payload_references(super::OperationRecord {
        payload: zero,
        bytes: zero,
        ..record
    })
    .unwrap();
    assert_eq!(field.declared_count, 0);
    assert_eq!(field.references.len(), 1);
    assert_eq!(field.references[0].object_index, 0x42);
    let two = b"\x01\x00\x01\x02\xf0\x41\x00\x00\xf0\x42\x01\x00\x00\x00";
    let field = super::sketch_payload_references(super::OperationRecord {
        payload: two,
        bytes: two,
        ..record
    })
    .unwrap();
    assert_eq!(field.declared_count, 2);
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>(),
        [0x41, 0x42]
    );

    let mut noncanonical = payload.to_vec();
    noncanonical[7] = 0;
    assert!(super::sketch_payload_references(super::OperationRecord {
        payload: &noncanonical,
        bytes: &noncanonical,
        ..record
    })
    .is_none());
    assert!(super::sketch_payload_references(super::OperationRecord {
        label: super::OperationLabel {
            value: "BLOCK",
            ..label
        },
        ..record
    })
    .is_none());
}

#[test]
fn om_extrude_profile_references_require_matching_witness_field() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x01\x02\x16\x01\x03\xf0\xff\xf1\x01\x00\x01\x03\x79\xaa\x01\x03\xf0\xff\xf1\x01\x00\x00\x00";
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = super::extrude_profile_references(record).unwrap();
    assert!(field.witnessed);
    let references = field.references;
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].object_index, 255);
    assert_eq!(references[0].raw_object_index, [0xf0, 0xff]);
    assert_eq!(references[0].offset, 205);
    assert_eq!(references[1].object_index, 256);
    assert_eq!(references[1].raw_object_index, [0xf1, 0x01, 0x00]);
    assert_eq!(references[1].offset, 207);

    let without_witness = &payload[..14];
    let field = super::extrude_profile_references(super::OperationRecord {
        payload: without_witness,
        bytes: without_witness,
        ..record
    })
    .unwrap();
    assert!(!field.witnessed);
    assert_eq!(field.references.len(), 2);
    assert!(super::extrude_profile_references(super::OperationRecord {
        label: super::OperationLabel {
            value: "SKETCH",
            ..label
        },
        ..record
    })
    .is_none());
}

#[test]
fn om_extrude_header_decodes_shifted_ieee_scalars() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload =
        b"\x0f\x00\x00\x01\x00\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x2f\xa3\x74\xbc\x6a\x7e\xf9\xdb";
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let header = super::extrude_payload_header(record).unwrap();
    assert_eq!(header.offset, 205);
    assert_eq!(header.scalars, [0.04, 0.038]);
    assert_eq!(header.raw_scalars.concat(), payload[5..21]);

    let mut invalid = payload.to_vec();
    invalid[5] = 0xf0;
    assert!(super::extrude_payload_header(super::OperationRecord {
        payload: &invalid,
        bytes: &invalid,
        ..record
    })
    .is_none());
}

#[test]
fn om_operation_terminal_discriminator_requires_one_complete_lane() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x01\x01\x02\x81\x5f\x80\xab\x01\x03\x02\x01\x01\x02\x01\x01\x00\x00\x00\x29\x29\x05\x80\xff\x00";
    let record = super::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let lane = super::operation_terminal_discriminator(record).unwrap();
    assert_eq!(lane.offset, 200);
    assert_eq!(lane.type_indices, [351, 171]);
    assert_eq!(lane.raw_type_indices, [vec![0x81, 0x5f], vec![0x80, 0xab]]);
    assert_eq!(lane.type_index_offsets, [203, 205]);
    assert_eq!(lane.flags, [1, 2, 1, 1]);
    assert_eq!(lane.trailing_indices, [5, 255]);
    assert_eq!(lane.raw_trailing_indices, [vec![0x05], vec![0x80, 0xff]]);
    assert_eq!(lane.trailing_index_offsets, [220, 221]);

    let subtract = super::OperationRecord {
        label: super::OperationLabel {
            value: "SUBTRACT",
            ..label
        },
        ..record
    };
    assert_eq!(
        super::operation_terminal_discriminator(subtract),
        Some(lane.clone())
    );

    let truncated = &payload[..payload.len() - 1];
    assert!(
        super::operation_terminal_discriminator(super::OperationRecord {
            payload: truncated,
            bytes: truncated,
            ..record
        })
        .is_none()
    );

    let mut ambiguous = payload[..payload.len() - 1].to_vec();
    ambiguous.extend_from_slice(payload);
    assert!(
        super::operation_terminal_discriminator(super::OperationRecord {
            payload: &ambiguous,
            bytes: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_operation_body_scalar_clauses_preserve_body_order_and_branch() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "TRIM BODY",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x42\xff\x1c\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\xaa\x01\x02\x10\x43\xff\x11\x30\x00\x00\x00\x00\x00\x00\x00\x00\x00";
    let record = super::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let triples = super::operation_body_scalar_triples(record);
    assert_eq!(triples.len(), 2);
    assert_eq!(triples[0].body_reference_ordinal, 0);
    assert_eq!(triples[0].body_object_index, 66);
    assert_eq!(triples[0].branch, 0x1c);
    assert_eq!(
        triples[0].scalars.each_ref().map(|scalar| scalar.value),
        [0.0, 3.0, -170.0]
    );
    assert_eq!(
        triples[0].scalars.each_ref().map(|scalar| scalar.encoding),
        [
            super::PayloadScalarEncoding::Zero,
            super::PayloadScalarEncoding::Binary32,
            super::PayloadScalarEncoding::Binary64,
        ]
    );
    assert_eq!(
        triples[0].scalars.each_ref().map(|scalar| scalar.offset),
        [106, 107, 111]
    );
    assert_eq!(
        triples[0]
            .scalars
            .each_ref()
            .map(|scalar| scalar.raw_value.as_slice()),
        [&bytes[6..7], &bytes[7..11], &bytes[11..19]]
    );
    assert_eq!(triples[1].body_reference_ordinal, 1);
    assert_eq!(triples[1].body_object_index, 67);
    assert_eq!(triples[1].branch, 0x11);
    assert_eq!(
        triples[1].scalars.each_ref().map(|scalar| scalar.value),
        [2.0, 0.0, 0.0]
    );
    let truncated = &bytes[..bytes.len() - 1];
    let truncated_triples = super::operation_body_scalar_triples(super::OperationRecord {
        bytes: truncated,
        payload: truncated,
        ..record
    });
    assert_eq!(truncated_triples.len(), 1);
    assert_eq!(truncated_triples[0], triples[0]);
}

#[test]
fn om_operation_body_branch_11_decodes_wrapped_member_lane_atomically() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SEW",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x42\xff\x11\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\x01\x03\x2e\x7f\x00\x2e\x80\x01\x00";
    let record = super::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let members = super::operation_body_members(record);
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].body_reference_ordinal, 0);
    assert_eq!(members[0].body_object_index, 66);
    assert_eq!(members[0].member_index, 127);
    assert_eq!(members[0].raw_member_index, [0x7f]);
    assert_eq!(members[0].offset, 122);
    assert_eq!(members[1].member_index, 1);
    assert_eq!(members[1].raw_member_index, [0x80, 0x01]);

    let truncated = &bytes[..bytes.len() - 1];
    assert!(super::operation_body_members(super::OperationRecord {
        bytes: truncated,
        payload: truncated,
        ..record
    })
    .is_empty());
}

#[test]
fn om_trim_body_branch_11_decodes_terminal_continuation_atomically() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "TRIM BODY",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x72\xff\x11\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\x01\x02\x2e\x41\x00\x01\x02\x80\x43\x00\x00\x01\x72\x00\x00";
    let record = super::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let continuations = super::operation_body_11_continuations(record);
    assert_eq!(continuations.len(), 1);
    let continuation = &continuations[0];
    assert_eq!(continuation.body_reference_ordinal, 0);
    assert_eq!(continuation.body_object_index, 114);
    assert_eq!(continuation.continuation_index, 67);
    assert_eq!(continuation.raw_continuation_index, [0x80, 0x43]);
    assert_eq!(continuation.continuation_offset, 126);
    assert_eq!(continuation.terminal_object_index, 114);
    assert_eq!(continuation.raw_terminal_object_index, [0x72]);
    assert_eq!(continuation.terminal_offset, 131);

    let mut distinct_terminal = bytes.to_vec();
    distinct_terminal[31] = 0x71;
    assert_eq!(
        super::operation_body_11_continuations(super::OperationRecord {
            bytes: &distinct_terminal,
            payload: &distinct_terminal,
            ..record
        })[0]
            .terminal_object_index,
        113
    );

    let truncated = &bytes[..bytes.len() - 1];
    assert!(
        super::operation_body_11_continuations(super::OperationRecord {
            bytes: truncated,
            payload: truncated,
            ..record
        })
        .is_empty()
    );
}

#[test]
fn om_operation_body_decodes_homogeneous_unwrapped_reference_lanes() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "OFFSET",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let compact = b"\x01\x02\x10\x6e\xff\x1c\x00\x00\x00\x01\x03\x80\x0d\x69\x00\x00\x0b\x00";
    let record = super::OperationRecord {
        offset: 100,
        bytes: compact,
        payload_offset: 100,
        payload: compact,
        label,
    };
    let lanes = super::operation_body_reference_lanes(record);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].body_object_index, 110);
    assert_eq!(
        lanes[0].encoding,
        super::OperationBodyReferenceLaneEncoding::CompactIndex
    );
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| (value.object_index, value.offset))
            .collect::<Vec<_>>(),
        [(13, 111), (105, 113)]
    );
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| value.raw_value.as_slice())
            .collect::<Vec<_>>(),
        [b"\x80\x0d".as_slice(), b"\x69".as_slice()]
    );

    let objects =
        b"\x01\x02\x10\x70\xff\x1c\x00\x00\x00\x01\x03\xf1\x02\x9e\xf0\x44\x00\x00\x0b\x00";
    let object_record = super::OperationRecord {
        bytes: objects,
        payload: objects,
        ..record
    };
    let lanes = super::operation_body_reference_lanes(object_record);
    assert_eq!(
        lanes[0].encoding,
        super::OperationBodyReferenceLaneEncoding::PayloadObjectIndex
    );
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| value.object_index)
            .collect::<Vec<_>>(),
        [670, 68]
    );
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| value.raw_value.as_slice())
            .collect::<Vec<_>>(),
        [b"\xf1\x02\x9e".as_slice(), b"\xf0\x44".as_slice()]
    );

    let truncated = &objects[..objects.len() - 1];
    assert!(
        super::operation_body_reference_lanes(super::OperationRecord {
            bytes: truncated,
            payload: truncated,
            ..object_record
        })
        .is_empty()
    );

    let branch_11 =
        b"\x01\x02\x10\x70\xff\x11\x00\x00\x00\x01\x03\xf1\x02\x9e\xf0\x44\x00\x00\x0b\x00";
    let lanes = super::operation_body_reference_lanes(super::OperationRecord {
        bytes: branch_11,
        payload: branch_11,
        ..record
    });
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].branch, 0x11);
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| value.object_index)
            .collect::<Vec<_>>(),
        [670, 68]
    );
}

#[test]
fn om_extrude_body_32_branch_decodes_counted_lanes() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x73\xff\x32\x00\x00\x30\x77\x7e\x14\x7a\xe1\x47\xb3\x01\x03\x3d\x82\x56\x00\x3d\x82\x57\x00\x01\x04\x80\x2b\x80\x2d\x80\x2c\x01\x03\x80\x2e\x80\x77\x00\x01\x73\x00\x00";
    let record = super::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let branch = super::extrude_payload_32_branch(record).unwrap();
    assert_eq!(branch.offset, 105);
    assert_eq!(branch.body_object_index, 115);
    assert!(branch.scalar.is_finite());
    assert_eq!(branch.raw_scalar, bytes[8..16]);
    assert_eq!(branch.atoms_be, [0x3d82_5600, 0x3d82_5700]);
    assert_eq!(branch.atom_offsets, [118, 122]);
    assert_eq!(branch.atom_indices, [598, 599]);
    assert_eq!(branch.first_indices, [43, 45, 44]);
    assert_eq!(
        branch.raw_first_indices,
        [vec![0x80, 0x2b], vec![0x80, 0x2d], vec![0x80, 0x2c]]
    );
    assert_eq!(branch.first_index_offsets, [128, 130, 132]);
    assert_eq!(branch.second_indices, [46, 119]);
    assert_eq!(
        branch.raw_second_indices,
        [vec![0x80, 0x2e], vec![0x80, 0x77]]
    );
    assert_eq!(branch.second_index_offsets, [136, 138]);
    assert_eq!(branch.terminal_object_index, 115);
    assert_eq!(branch.raw_terminal_object_index, [0x73]);
    assert_eq!(branch.terminal_offset, 142);

    let mut invalid = bytes.to_vec();
    invalid[36] = 0xff;
    assert!(super::extrude_payload_32_branch(super::OperationRecord {
        bytes: &invalid,
        payload: &invalid,
        ..record
    })
    .is_none());

    let mut invalid_atom = bytes.to_vec();
    invalid_atom[18] = 0x3c;
    assert!(super::extrude_payload_32_branch(super::OperationRecord {
        bytes: &invalid_atom,
        payload: &invalid_atom,
        ..record
    })
    .is_none());

    let mut wrong_terminal_body = bytes.to_vec();
    wrong_terminal_body[43] = 0x72;
    assert!(super::extrude_payload_32_branch(super::OperationRecord {
        bytes: &wrong_terminal_body,
        payload: &wrong_terminal_body,
        ..record
    })
    .is_none());
}

#[test]
fn om_block_construction_field_decodes_ordered_canonical_references() {
    let label = super::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "BLOCK",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let mut payload = vec![0x26, 0, 0, 1, 0, 0];
    for value in 1..=18u8 {
        payload.extend([0xf0, value]);
    }
    payload.extend([0x01, 0xf1, 0x01, 0x00]);
    payload.extend([0xff; 11]);
    payload.extend([0; 4]);
    let record = super::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label,
    };
    let field = super::block_construction_references(record).unwrap();
    assert_eq!(field.control, 0x26);
    assert_eq!(field.references.len(), 19);
    assert_eq!(field.references[0].object_index, 1);
    assert_eq!(field.references[0].raw_object_index, [0xf0, 0x01]);
    assert_eq!(field.references[18].object_index, 256);
    assert_eq!(field.references[18].raw_object_index, [0xf1, 0x01, 0x00]);
    assert_eq!(field.references[0].offset, 206);

    let mut invalid = payload.clone();
    invalid[42] = 0xf0;
    assert!(
        super::block_construction_references(super::OperationRecord {
            bytes: &invalid,
            payload: &invalid,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_boolean_operations_decode_counted_target_and_tools() {
    let bytes = b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x0aSUBTRACT\0\x31\x00\x00\x01\x00\x14\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\x00\x00\xe0\x7f\xff\xff\xff\x01\x01\x01\x02\x90\x19\x5e\x00\x01\x05\x90\x19\x5f\x90\x19\x44\x90\x19\x43\x90\x19\x60\x00";
    let operations = super::boolean_operations(bytes, 100);
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0].kind, super::BooleanOperationKind::Subtract);
    assert_eq!(operations[0].target, 6494);
    assert_eq!(operations[0].raw_target, [0x90, 0x19, 0x5e]);
    assert_eq!(
        operations[0].target_offset,
        100 + bytes
            .windows(3)
            .position(|window| window == [0x90, 0x19, 0x5e])
            .unwrap()
    );
    assert_eq!(operations[0].tools, [6495, 6468, 6467, 6496]);
    assert_eq!(
        operations[0].raw_tools,
        [
            vec![0x90, 0x19, 0x5f],
            vec![0x90, 0x19, 0x44],
            vec![0x90, 0x19, 0x43],
            vec![0x90, 0x19, 0x60],
        ]
    );
    assert_eq!(
        operations[0].tool_offsets,
        [0x5f, 0x44, 0x43, 0x60].map(|low| {
            100 + bytes
                .windows(3)
                .position(|window| window == [0x90, 0x19, low])
                .unwrap()
        })
    );

    let mut invalid = bytes.to_vec();
    *invalid.last_mut().unwrap() = 1;
    assert!(super::boolean_operations(&invalid, 0).is_empty());
}

#[test]
fn om_index_accepts_length_framed_root_version_text() {
    let mut bytes = indexed_om_section();
    let marker = bytes
        .windows(b"\x04\x01\x0eNX 2027.3102\0".len())
        .position(|window| window == b"\x04\x01\x0eNX 2027.3102\0")
        .expect("root record");
    bytes[marker + 2] = 0x0f;
    bytes.insert(marker + 3 + 12, b' ');
    let index = bytes
        .windows(4)
        .position(|window| window == 0u32.to_le_bytes())
        .expect("index");
    for ordinal in 2..4 {
        let at = index + ordinal * 4;
        let value = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) + 1;
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let sections = super::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert!(sections[0].records[0]
        .bytes
        .starts_with(b"\x04\x01\x0fNX 2027.3102 \0"));
}

#[test]
fn om_store_version_can_follow_control_prefix() {
    let bytes = b"\xff\x00prefix\x04\x01\x0eNX 2027.3102\0tail";
    let version = super::store_version(bytes, 100).expect("store version");
    assert_eq!(version.offset, 108);
    assert_eq!(version.value, "NX 2027.3102");
}

#[test]
fn om_offset_only_index_bounds_storage_blocks() {
    let bytes = offset_only_indexed_om_section();
    let sections = super::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].base, 0);
    assert_eq!(
        sections[0].control.as_ref().unwrap().bytes,
        &[0, 0, 0, 0, 0, 1, 0, 0]
    );
    assert_eq!(sections[0].records.len(), 2);
    assert_eq!(
        sections[0].column_storage.unwrap(),
        [sections[0].records[0].bytes, sections[0].records[1].bytes].concat()
    );
    assert_eq!(sections[0].records[0].object_id, None);
    assert!(sections[0].records[0].bytes.starts_with(b"\x04\x01\x0eNX "));
    assert_eq!(sections[0].records[1].object_id, None);
    assert!(sections[0].records[1].bytes.ends_with(b"\0"));
    let expressions = sections[0].numeric_expressions();
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].name, "length");
    assert_eq!(expressions[0].value, Some(25.0));
}

#[test]
fn om_offset_only_index_accepts_one_root_record_inside_control_block() {
    let bytes = control_root_offset_only_indexed_om_section();
    let sections = super::indexed_sections(&bytes);

    assert_eq!(sections.len(), 1);
    assert!(sections[0]
        .control
        .as_ref()
        .unwrap()
        .bytes
        .windows(b"NX 2027.3102".len())
        .any(|window| window == b"NX 2027.3102"));
    assert_eq!(sections[0].records.len(), 2);
    assert_eq!(sections[0].records[0].bytes, &[0; 32]);
    assert_eq!(sections[0].numeric_expressions()[0].name, "length");
}

#[test]
fn om_offset_only_index_requires_one_supported_product_record() {
    let mut duplicate = control_root_offset_only_indexed_om_section();
    let first_column = duplicate
        .windows(32)
        .position(|window| window == [0; 32])
        .expect("zero first column");
    let duplicate_product = b"\x04\x01\x0eNX 2027.3102\0";
    duplicate[first_column..first_column + duplicate_product.len()]
        .copy_from_slice(duplicate_product);
    assert!(super::indexed_sections(&duplicate).is_empty());

    let mut unsupported = control_root_offset_only_indexed_om_section();
    let product = unsupported
        .windows(b"\x05\x01\x0eNX 2027.3102\0".len())
        .position(|window| window == b"\x05\x01\x0eNX 2027.3102\0")
        .expect("product record");
    unsupported[product] = 0x03;
    assert!(super::indexed_sections(&unsupported).is_empty());
}

#[test]
fn om_offset_store_control_values_require_complete_zero_prefixed_words() {
    assert_eq!(
        super::offset_store_control_values(&[0, 0x34, 0x12, 0, 0, 0xff, 0xff, 0xff]),
        Some(vec![0x1234, 0x00ff_ffff])
    );
    assert!(super::offset_store_control_values(&[]).is_none());
    assert!(super::offset_store_control_values(&[0, 1, 2]).is_none());
    assert!(super::offset_store_control_values(&[1, 1, 2, 3]).is_none());
}

#[test]
fn om_offset_store_control_form_requires_one_complete_grammar() {
    assert_eq!(
        super::offset_store_control_form(&[0, 0x34, 0x12, 0, 0, 0xff, 0xff, 0xff]),
        Some(super::OffsetStoreControlForm::ZeroPrefixed {
            values: vec![0x1234, 0x00ff_ffff],
        })
    );

    let mut product = vec![0, 0];
    product.extend_from_slice(&7u32.to_le_bytes());
    product.extend_from_slice(&0x1020u32.to_le_bytes());
    product.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0");
    assert_eq!(
        super::offset_store_control_form(&product),
        Some(super::OffsetStoreControlForm::ProductAnchored {
            leading_value: Some((2, 0)),
            values: vec![7, 0x1020],
        })
    );

    product.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0");
    assert!(super::offset_store_control_form(&product).is_none());
    assert!(super::offset_store_control_form(&[1, 2, 3, 4]).is_none());
}

#[test]
fn om_offset_store_index_rows_require_complete_exact_frames() {
    let first =
        b"\x2d\x02\x0b\x2a\x93\x8a\x03\x80\x18\x20\x20\x41\x00\x47\x04\x04\x01\xc0\x44\x04\x00";
    let second = b"\x2d\x02\x0b\x83\xb6\x93\x8a\x07\x80\x18\x20\x80\x4d\x41\x00\x47\x04\x04\x01\xc0\x44\x04\x00";
    let mut bytes = b"prefix".to_vec();
    bytes.extend_from_slice(first);
    bytes.extend_from_slice(b"gap");
    bytes.extend_from_slice(second);

    let rows = super::offset_store_index_rows(&bytes);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].offset, 6);
    assert_eq!(rows[0].first_index, 42);
    assert_eq!(rows[0].raw_first_index, [0x2a]);
    assert_eq!(rows[0].flag, 3);
    assert_eq!(rows[0].indices, [(24, 13), (32, 15), (32, 16), (65, 17)]);
    assert_eq!(
        rows[0].raw_indices,
        [vec![0x80, 0x18], vec![0x20], vec![0x20], vec![0x41]]
    );
    assert_eq!(rows[1].first_index, 950);
    assert_eq!(rows[1].raw_first_index, [0x83, 0xb6]);
    assert_eq!(rows[1].flag, 7);
    assert_eq!(rows[1].indices, [(24, 38), (32, 40), (77, 41), (65, 43)]);
    assert_eq!(
        rows[1].raw_indices,
        [vec![0x80, 0x18], vec![0x20], vec![0x80, 0x4d], vec![0x41]]
    );

    let mut null = first.to_vec();
    null[3] = 0xff;
    assert!(super::offset_store_index_rows(&null).is_empty());
    let mut other_flag = first.to_vec();
    other_flag[6] = 0x04;
    assert!(super::offset_store_index_rows(&other_flag).is_empty());
    let mut overlong = first.to_vec();
    overlong.insert(12, 0x01);
    assert!(super::offset_store_index_rows(&overlong).is_empty());
    assert!(super::offset_store_index_rows(&first[..first.len() - 1]).is_empty());
}

#[test]
fn om_color_table_requires_complete_names_indices_and_rgb_atoms() {
    let mut bytes = vec![0x02, 0x80, 0xd9, 0x01];
    for ordinal in 0..=216 {
        let name = if ordinal == 0 {
            "Background".to_string()
        } else {
            format!("Color {ordinal}")
        };
        bytes.push(u8::try_from(name.len() + 2).unwrap());
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
    }
    bytes.extend_from_slice(&[
        0x02, 0x14, 0xff, 0x06, 0x00, 0xf0, 0x02, 0x80, 0x9d, 0x80, 0xc7, 0x00, 0xc0, 0x13, 0x0a,
        0xc6, 0x01, 0x80, 0xd9, 0x80, 0xc8, 0x01, 0x01, 0x01,
    ]);
    for color_index in 1u16..=216 {
        bytes.push(0x05);
        if color_index < 128 {
            bytes.push(color_index as u8);
        } else {
            bytes.extend_from_slice(&[0x80, (color_index - 1) as u8]);
        }
        bytes.extend_from_slice(&[0x01, 0x80, 0xc8]);
        if color_index == 2 {
            bytes.extend_from_slice(&shifted_f64_bytes(2.0));
            let mut binary32 = 1.0_f32.to_be_bytes();
            binary32[0] += 0x10;
            bytes.extend_from_slice(&binary32);
            bytes.push(0x00);
        } else {
            bytes.extend_from_slice(&[0x01, 0x01, 0x01]);
        }
    }

    let tables = super::color_tables(&bytes);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].background_name, "Background");
    assert_eq!(tables[0].background_rgb, [1.0, 1.0, 1.0]);
    assert_eq!(tables[0].definitions.len(), 216);
    assert_eq!(tables[0].definitions[0].name, "Color 1");
    assert_eq!(tables[0].definitions[0].rgb, [1.0, 1.0, 1.0]);
    assert_eq!(tables[0].definitions[1].rgb, [0.5, 0.25, 0.0]);
    assert_eq!(tables[0].definitions[127].raw_color_index, [0x80, 0x7f]);

    let mut malformed = bytes.clone();
    *malformed.last_mut().unwrap() = 0x02;
    assert!(super::color_tables(&malformed).is_empty());
    let truncated = &bytes[..bytes.len() - 1];
    assert!(super::color_tables(truncated).is_empty());
}

#[test]
fn om_offset_store_linked_index_rows_require_complete_exact_frames() {
    let row = b"\x02\x0b\x83\x93\x93\x8c\x16\x24\xff\xff\x90\xfe\x20\x20\x41\x00\x47\x03\x04\x01\xc0\x44\x04\x00";
    let rows = super::offset_store_linked_index_rows(row);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].first_index, (915, 2));
    assert_eq!(rows[0].raw_first_index, [0x83, 0x93]);
    assert_eq!(rows[0].discriminator, 0x16);
    assert_eq!(rows[0].target_index, (36, 7));
    assert_eq!(rows[0].raw_target_index, [0x24]);
    assert_eq!(rows[0].indices, [(32, 12), (32, 13), (65, 14)]);
    assert_eq!(rows[0].raw_indices, [vec![0x20], vec![0x20], vec![0x41]]);
    assert_eq!(rows[0].flag, 3);
    assert_eq!(rows[0].mode, 4);

    let mut null = row.to_vec();
    null[7] = 0xff;
    assert!(super::offset_store_linked_index_rows(&null).is_empty());
    let mut discriminator = row.to_vec();
    discriminator[6] = 0x15;
    assert!(super::offset_store_linked_index_rows(&discriminator).is_empty());
    let mut flag = row.to_vec();
    flag[17] = 0x04;
    assert!(super::offset_store_linked_index_rows(&flag).is_empty());
    let mut mode = row.to_vec();
    mode[18] = 0x06;
    assert!(super::offset_store_linked_index_rows(&mode).is_empty());
    let mut mode_seven = row.to_vec();
    mode_seven[18] = 0x07;
    assert_eq!(
        super::offset_store_linked_index_rows(&mode_seven)[0].mode,
        7
    );
    assert!(super::offset_store_linked_index_rows(&row[..row.len() - 1]).is_empty());
}

#[test]
fn om_offset_store_target_index_rows_require_complete_exact_frames() {
    let row =
        b"\x02\x01\x01\x01\x16\x3e\xff\xff\x90\xfe\x1e\x20\x58\x00\x47\x03\x07\x01\xc0\x44\x04\x00";
    let rows = super::offset_store_target_index_rows(row);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].target_index, (62, 5));
    assert_eq!(rows[0].raw_target_index, [0x3e]);
    assert_eq!(rows[0].indices, [(30, 10), (32, 11), (88, 12)]);
    assert_eq!(rows[0].raw_indices, [vec![0x1e], vec![0x20], vec![0x58]]);
    assert_eq!(rows[0].mode, 7);

    let mut null = row.to_vec();
    null[5] = 0xff;
    assert!(super::offset_store_target_index_rows(&null).is_empty());
    let mut discriminator = row.to_vec();
    discriminator[4] = 0x17;
    assert!(super::offset_store_target_index_rows(&discriminator).is_empty());
    let mut suffix = row.to_vec();
    suffix[16] = 0x03;
    assert!(super::offset_store_target_index_rows(&suffix).is_empty());
    let mut mode_four = row.to_vec();
    mode_four[16] = 0x04;
    assert_eq!(super::offset_store_target_index_rows(&mode_four)[0].mode, 4);
    assert!(super::offset_store_target_index_rows(&row[..row.len() - 1]).is_empty());
}

#[test]
fn om_offset_store_control_class_lane_is_a_distinct_in_range_prefix() {
    let encode = |values: &[u32]| {
        values
            .iter()
            .flat_map(|value| {
                let bytes = value.to_le_bytes();
                [0, bytes[0], bytes[1], bytes[2]]
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        super::offset_store_control_class_ordinals(&encode(&[2, 0, 4, 8, 3])),
        Some(vec![2, 0])
    );
    assert!(super::offset_store_control_class_ordinals(&encode(&[2, 2, 4])).is_none());
    assert!(super::offset_store_control_class_ordinals(&encode(&[2, 4, 1])).is_none());
    assert_eq!(
        super::offset_store_control_class_ordinals(&encode(&[4, 8])),
        Some(vec![4])
    );
}

#[test]
fn om_registry_uses_length_framing_and_stays_outside_entity_payloads() {
    let mut bytes = indexed_om_section();
    bytes.extend_from_slice(b"\x10UGS::PayloadText");
    let sections = super::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].types.len(), 1);
    assert_eq!(sections[0].types[0].name, "UGS::EXP_expression");
    assert_eq!(sections[0].types[0].trailing_code, 0x81);
    assert_eq!(sections[0].types[0].offset, 8);
}

#[test]
fn om_numeric_expression_retains_identity_name_unit_and_value() {
    let bytes = indexed_om_section();
    let section = super::indexed_sections(&bytes).remove(0);
    let expression_records = section.numeric_expression_records();
    assert_eq!(expression_records[0].0, 1);
    let expressions = expression_records
        .iter()
        .map(|(_, expression)| expression)
        .collect::<Vec<_>>();
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].object_id, Some(0x102));
    assert_eq!(
        expressions[0].name,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(expressions[0].parameter_index, Some(8));
    assert_eq!(
        expressions[0].qualifier,
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(expressions[0].unit, super::ExpressionUnit::Degree);
    assert_eq!(expressions[0].expression, "120");
    assert_eq!(expressions[0].value, Some(120.0));
    let declaration = super::expression_declaration_name(section.records[1].bytes).unwrap();
    assert_eq!(
        declaration.value,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(declaration.parameter_index, 8);
    assert_eq!(
        declaration.qualifier,
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(declaration.literal, Some("120"));
    let declaration =
        super::expression_declaration_name(b"\x04\x04p1\0\x04\x0a-5.1 * 2\0").unwrap();
    assert_eq!(declaration.value, "p1");
    assert_eq!(declaration.literal, Some("-5.1 * 2"));
    let declaration =
        super::expression_declaration_name(b"\x04\x04p1\0\x04\x055.1\0\x04\x05120\0").unwrap();
    assert_eq!(declaration.literal, None);
    assert!(super::expression_declaration_name(b"\x04\x04p1\0\x04\x04p2\0").is_none());
    assert!(super::expression_declaration_name(b"\x04\x05p1-\0").is_none());
}

#[test]
fn om_numeric_expression_types_only_canonical_parameter_names() {
    for name in ["p12foo", "p12_", "p4294967296_radius"] {
        let text = format!("(Number [mm]) {name}: 5; ");
        let mut bytes = b"hostglobalvariables".to_vec();
        bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);

        let expressions = super::numeric_expressions(&bytes);
        assert_eq!(expressions.len(), 1);
        assert_eq!(expressions[0].name, name);
        assert_eq!(expressions[0].parameter_index, None);
        assert_eq!(expressions[0].qualifier, None);
    }
    assert!(super::expression_declaration_name(b"\x04\x08p12foo\0").is_none());
    assert!(super::expression_declaration_name(b"\x04\x06p12_\0").is_none());
}

#[test]
fn om_numeric_expression_evaluates_constant_arithmetic_formula() {
    let text = b"(Number [mm]) p9: (193.94 - 6) / 2 + 1.5e1; ";
    let mut bytes = b"hostglobalvariables".to_vec();
    bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
    bytes.extend_from_slice(text);
    bytes.push(0);

    let expressions = super::numeric_expressions(&bytes);
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].expression, "(193.94 - 6) / 2 + 1.5e1");
    assert_eq!(expressions[0].value, Some(108.97));
}

#[test]
fn om_numeric_expression_applies_power_before_unary_sign() {
    for (formula, expected) in [
        ("-2^2", -4.0),
        ("(-2)^2", 4.0),
        ("2^-2", 0.25),
        ("2^3^2", 512.0),
    ] {
        assert_eq!(
            super::evaluate_constant_expression(formula),
            Some(expected),
            "{formula}"
        );
    }
}

#[test]
fn om_string_value_requires_marker_length_printability_and_terminator() {
    let bytes = b"\x66\x32\x03\x0cSKETCH_001\0\x66\x32\x03\x03A\0\x66\x32\x03\x03A\x01";
    let values = super::string_values(bytes, 100);
    assert_eq!(values.len(), 2);
    assert_eq!(values[0].offset, 100);
    assert_eq!(values[0].value, "SKETCH_001");
    assert_eq!(values[1].value, "A");
}

#[test]
fn om_tagged_references_preserve_family_value_order_and_bounds() {
    let bytes = b"\xe0\x12\x34\x56\x78\xca\xbc\xde\xf0\xe0\x01";
    let references = super::references(bytes, 20);
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].offset, 20);
    assert_eq!(references[0].kind, super::ReferenceKind::PersistentHandle);
    assert_eq!(references[0].value, 0x1234_5678);
    assert_eq!(references[1].offset, 25);
    assert_eq!(references[1].kind, super::ReferenceKind::Tagged28);
    assert_eq!(references[1].value, 0x0abc_def0);
}

#[test]
fn om_counted_record_references_require_a_complete_in_bounds_run() {
    let bytes = b"\xff\x01\x03\x90\x00\x02\x90\x00\x04\x01\x02\x90\x00\x05";
    let references = super::counted_record_references(bytes, 100, 5);
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].offset, 103);
    assert_eq!(references[0].kind, super::ReferenceKind::RecordOrdinal16);
    assert_eq!(references[0].value, 2);
    assert_eq!(references[1].value, 4);
}

#[test]
fn om_record_references_require_adjacent_persistent_tagged_pairs() {
    let mut dense = b"ordinary-prefix".to_vec();
    for value in 1..=8u32 {
        dense.push(0xe0);
        dense.extend_from_slice(&value.to_be_bytes());
        dense.extend_from_slice(&(0xc000_0000 | value).to_be_bytes());
    }
    let references = super::record_references(&dense, 100);
    assert_eq!(references.len(), 16);
    assert_eq!(references[0].offset, 115);

    let mut unpaired = dense;
    unpaired.extend_from_slice(&[0xc0, 0, 0, 2]);
    assert_eq!(
        super::record_references(&unpaired, 0)
            .into_iter()
            .filter(|reference| reference.kind == super::ReferenceKind::Tagged28)
            .count(),
        8
    );

    let lone_tagged = [0xc0, 0, 0, 1];
    assert!(super::record_references(&lone_tagged, 0)
        .into_iter()
        .all(|reference| reference.kind != super::ReferenceKind::Tagged28));
}

#[test]
fn om_numeric_expression_table_is_independent_of_entity_indexing() {
    let bytes = b"hostglobalvariables\x99\x04P(Number [degrees]) p8_CircularPattern_pattern_Circular_Dir_offset_angle: 120; \x00";
    let expressions = super::numeric_expressions(bytes);
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].object_id, None);
    assert_eq!(
        expressions[0].name,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(expressions[0].parameter_index, Some(8));
    assert_eq!(
        expressions[0].qualifier,
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(expressions[0].value, Some(120.0));
}
