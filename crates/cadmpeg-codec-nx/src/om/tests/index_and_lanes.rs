// SPDX-License-Identifier: Apache-2.0
//! Unit and fixture tests for OM wire parsers owned by `om`.

#![allow(clippy::unwrap_used)]

use crate::test_support::*;

const EPS_NAMED_POINT_ROUNDING: f64 = 1.0e-12;
const EPS_SHIFTED_SCALAR_ROUNDING: f64 = 2.0e-12;

#[test]
fn om_index_pairs_object_ids_with_bounded_entity_records() {
    let bytes = indexed_om_section();
    let sections = super::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].base, 8);
    let records = sections[0].as_fixed().expect("fixed store");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].object_id.0, 0x101);
    assert_eq!(
        records[0].object_id.1 as usize,
        sections[0].object_id_table_offset + 8
    );
    assert_eq!(
        records[0].bytes,
        b"\x04\x01\x0eNX 2027.3102\x00hostglobalvariables"
    );
    assert_eq!(records[1].object_id.0, 0x102);
    assert_eq!(
        records[1].object_id.1 as usize,
        sections[0].object_id_table_offset + 12
    );
    assert!(sections[0].as_offset_only().is_none());
    assert_eq!(sections[0].fields.len(), 1);
    assert_eq!(sections[0].fields[0].name, "m_target");
    assert_eq!(
        records[1].bytes,
        b"\x04\x36p8_CircularPattern_pattern_Circular_Dir_offset_angle\x00\x04\x05120\x00\x99\x04P(Number [degrees]) p8_CircularPattern_pattern_Circular_Dir_offset_angle: 120; \x00\x66\x32\x03\x0cSKETCH_001\0\xe0\x12\x34\x56\x78\xca\xbc\xde\xf0\x01\x02\x90\x00\x00"
    );
}

#[test]
fn om_compact_index_lane_decodes_direct_extended_and_null_entries() {
    use crate::om::compact::{CompactIndexAtom, NullableCompactIndex};

    let bytes = [0x00, 0x7f, 0x80, 0x80, 0x81, 0x00, 0xfe, 0xff, 0xff];
    let mut at = 0;
    let mut values = Vec::new();
    while at < bytes.len() {
        let token = NullableCompactIndex::read(&bytes, at).unwrap();
        at += token.raw().len();
        values.push(token.atom.map(CompactIndexAtom::value));
    }
    assert_eq!(
        values,
        vec![Some(0), Some(127), Some(128), Some(256), Some(32_511), None]
    );
    assert_eq!(NullableCompactIndex::read(&[0x80], 0), None);
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
    assert_eq!(references[0].atom.value(), 370);
    assert_eq!(references[0].atom.raw(), [0x81, 0x72]);
    assert_eq!(references[0].offset, 1);

    bytes.extend_from_slice(&[0x73]);
    bytes.extend_from_slice(&discriminator);
    let references = super::data_block_object_frames(&bytes);
    assert_eq!(references.len(), 2);
    assert_eq!(references[1].atom.value(), 0x73);
    assert_eq!(references[1].atom.raw(), [0x73]);
    assert_eq!(references[1].offset, 22);

    bytes[8] ^= 1;
    let references = super::data_block_object_frames(&bytes);
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].atom.value(), 0x73);
    let mut null = vec![0xff];
    null.extend_from_slice(&discriminator);
    assert!(super::data_block_object_frames(&null).is_empty());
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
    assert!((fields[0].scalar.value() - 38.1).abs() < EPS_SHIFTED_SCALAR_ROUNDING);

    let mut malformed = bytes;
    malformed[5] = 1;
    assert!(super::construction_payload_scalar_fields(&malformed).is_empty());
    malformed = bytes;
    malformed[6] = 0x70;
    assert!(super::construction_payload_scalar_fields(&malformed).is_empty());
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
    let point = super::offset_store_named_point([&first[..], &second[..]]).unwrap();
    assert_eq!(point.name, "Point7");
    assert!(point
        .values
        .iter()
        .all(|value| (value.scalar.value() - 57.15).abs() < EPS_NAMED_POINT_ROUNDING));
    let expected_raw: [[u8; 8]; 2] = [
        first[14..22].try_into().unwrap(),
        second[8..16].try_into().unwrap(),
    ];
    assert_eq!(point.values.map(|value| value.scalar.raw()), expected_raw);
    assert_eq!(point.values.map(|value| value.offset), [9, first.len() + 3]);
    assert_eq!(point.block_count, 2);

    let mut same_block = first.to_vec();
    same_block.extend_from_slice(&second);
    assert_eq!(
        super::offset_store_named_point([&same_block[..]])
            .unwrap()
            .block_count,
        1
    );
    assert_eq!(
        super::offset_store_named_point([&first[..9], &first[9..], &second[..]])
            .unwrap()
            .block_count,
        3
    );
    let third = [
        0x50, 0x59, 0x66, 0x58, 0x00, 0x30, 0x4c, 0x93, 0x33, 0x33, 0x33, 0x33, 0x07,
    ];
    assert!(super::offset_store_named_point([&first[..], &second[..], &third[..]]).is_none());
    let next_name = [
        0x66, 0x32, 0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'8', 0x00,
    ];
    let next_name_blocks = [&first[..], &second[..], &next_name[..]];
    assert!(super::offset_store_named_point(next_name_blocks).is_some());
    let next_point = [0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'8', 0x00];
    assert_eq!(
        super::offset_store_named_point([&first[..], &second[..], &next_point[..]])
            .unwrap()
            .block_count,
        2
    );
    let mut zero = first;
    zero[7] = b'0';
    assert!(super::offset_store_named_point([&zero[..], &second[..]]).is_none());
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
    assert_eq!(
        pairs[0].values.map(crate::om::fixed::Q155::value),
        [0.5, -0.5]
    );
    assert_eq!(pairs[0].value_offsets(), [15, 24]);
    assert_eq!(pairs[0].values[0].raw(), [0x40, 0, 0, 0, 0, 0, 0]);

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
    assert_eq!(
        pairs[0].values.map(crate::om::fixed::Q155::value),
        [0.5, -0.5]
    );
    assert_eq!(
        pairs[0].value_offsets(),
        [discriminator.len(), discriminator.len() + 9]
    );
    assert_eq!(pairs[0].discriminator(), discriminator);

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
    assert_eq!(fields[0].scalar.value(), 25.4);
    assert_eq!(fields[0].scalar.raw(), shifted);
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
    let label = "SIMPLE HOLE";
    let record = crate::om::operation_record::OperationPayload::new(&payload, 200, label).unwrap();
    let lane = super::simple_hole_repeated_scalar_lane(record).unwrap();
    assert_eq!(lane.iter().next().unwrap().scalar.value(), 508.0);
    assert!((lane.iter().nth(1).unwrap().scalar.value() - 38.1).abs() < 2.0e-12);
    assert_eq!(
        lane.iter()
            .map(|token| token.scalar.raw())
            .collect::<Vec<_>>(),
        [shifted(508.0), shifted(38.1)]
    );
    assert_eq!(
        [0, 1].map(|i| lane
            .iter()
            .map(|token| token.witness_offsets[i])
            .collect::<Vec<_>>()),
        [vec![200, 209], vec![218, 227]]
    );

    let mut mismatched = payload.clone();
    mismatched[18 + 7] ^= 1;
    assert!(super::simple_hole_repeated_scalar_lane(
        crate::om::operation_record::OperationPayload::new(
            &mismatched,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
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
    let record =
        crate::om::operation_record::OperationPayload::new(&payload, 200, "SIMPLE HOLE").unwrap();
    let lane = super::simple_hole_repeated_scalar_lane(record).unwrap();
    assert_eq!(
        lane.iter()
            .map(|token| token.scalar.value())
            .collect::<Vec<_>>(),
        [25.4]
    );
    assert_eq!(
        lane.iter()
            .map(|token| token.scalar.raw())
            .collect::<Vec<_>>(),
        [scalar]
    );
    assert_eq!(
        [0, 1].map(|i| lane
            .iter()
            .map(|token| token.witness_offsets[i])
            .collect::<Vec<_>>()),
        [vec![200], vec![209]]
    );
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
    let label = "SIMPLE HOLE";
    let record = crate::om::operation_record::OperationPayload::new(&payload, 200, label).unwrap();
    let references =
        crate::om::simple_hole_references::simple_hole_repeated_scalar_lane_block_references(
            record,
        )
        .unwrap();
    assert_eq!(
        references[0].references().map(|(token, _)| token.value()),
        [231, 232]
    );
    assert_eq!(
        references[1].references().map(|(token, _)| token.value()),
        [233, 234]
    );
    assert_eq!(
        references.map(|pair| pair.references().map(|(_, offset)| offset)),
        [[216, 218], [236, 238]]
    );
    assert_eq!(
        references.map(crate::om::simple_hole_references::ReferencePair::wrapped),
        [false, false]
    );

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
        crate::om::simple_hole_references::simple_hole_repeated_scalar_lane_block_references(
            crate::om::operation_record::OperationPayload::new(
                &wrapped,
                record.payload_offset(),
                record.name(),
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        wrapped_references[0]
            .references()
            .map(|(token, _)| token.value()),
        [231, 232]
    );
    assert_eq!(
        wrapped_references[1]
            .references()
            .map(|(token, _)| token.value()),
        [233, 234]
    );
    assert_eq!(
        wrapped_references.map(|pair| pair.references().map(|(_, offset)| offset)),
        [[224, 226], [252, 254]]
    );
    assert_eq!(
        wrapped_references.map(crate::om::simple_hole_references::ReferencePair::wrapped),
        [true, true]
    );
    let mut malformed_wrapper = wrapped.clone();
    malformed_wrapper[16] ^= 1;
    assert!(
        crate::om::simple_hole_references::simple_hole_repeated_scalar_lane_block_references(
            crate::om::operation_record::OperationPayload::new(
                &malformed_wrapper,
                record.payload_offset(),
                record.name()
            )
            .unwrap(),
        )
        .is_none()
    );

    let mut null = payload.clone();
    null[16] = 0xff;
    assert!(
        crate::om::simple_hole_references::simple_hole_repeated_scalar_lane_block_references(
            crate::om::operation_record::OperationPayload::new(
                &null,
                record.payload_offset(),
                record.name()
            )
            .unwrap()
        )
        .is_none()
    );
}

#[test]
fn om_hole_package_lane_retains_the_exact_four_block_group() {
    let payload = [
        0x7e, 0x00, 0x00, 0x01, 0x00, 0x00, 0x46, 0x00, 0x11, 0x00, 0x00, 0x00, 0x00, 0xf0, 0xcd,
        0xf0, 0xce, 0x11, 0x00, 0x00, 0x00, 0x00, 0xf0, 0xcf, 0xf0, 0xd0, 0x00, 0x00, 0xff, 0x7f,
    ];
    let record =
        crate::om::operation_record::OperationPayload::new(&payload, 200, "HOLE PACKAGE").unwrap();
    let lane = super::hole_package_construction_group_lane(record).unwrap();
    assert_eq!(lane.offset, 1);
    assert_eq!(lane.selector.get(), 0x46);
    assert_eq!(lane.branch.get(), 0x11);
    assert_eq!(
        lane.references
            .iter()
            .map(|reference| reference.token.value())
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
    assert!(super::hole_package_construction_group_lane(
        crate::om::operation_record::OperationPayload::new(
            &mismatched_branch,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
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
    let label = "DATUM_CSYS";
    let record = crate::om::operation_record::OperationPayload::new(&payload, 100, label).unwrap();
    let field = crate::om::datum_csys::datum_csys_references(record).unwrap();
    assert_eq!(field.control(), 0x13);
    assert_eq!(
        field.members().each_ref().map(|(token, ())| token.value()),
        [42, 43, 44, 45, 46, 47, 48, 49]
    );
    assert_eq!(field.offsets(), [114, 116, 118, 120, 122, 124, 126, 128]);
    assert_eq!(
        field
            .members()
            .iter()
            .map(|(token, ())| token.raw().to_vec())
            .collect::<Vec<_>>(),
        (42..50).map(|value| vec![0xf0, value]).collect::<Vec<_>>()
    );

    let mut alternate_control = payload.clone();
    alternate_control[0] = 0x1a;
    assert_eq!(
        crate::om::datum_csys::datum_csys_references(
            crate::om::operation_record::OperationPayload::new(
                &alternate_control,
                record.payload_offset(),
                record.name()
            )
            .unwrap()
        )
        .unwrap()
        .control(),
        0x1a
    );

    let mut malformed = payload.clone();
    malformed[14] = 0x2a;
    assert!(crate::om::datum_csys::datum_csys_references(
        crate::om::operation_record::OperationPayload::new(
            &malformed,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
}

#[test]
fn om_datum_plane_header_requires_common_prefix_and_nontrivial_count() {
    let payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x03, 0x29, 0x01, 0x02, 0xf1, 0x02, 0xcf,
    ];
    let label = "DATUM_PLANE";
    let record = crate::om::operation_record::OperationPayload::new(&payload, 100, label).unwrap();
    assert_eq!(
        crate::om::datum_plane_header::datum_plane_payload_header(record),
        Some(crate::om::datum_plane_header::DatumPlanePayloadHeader {
            control: 0x22,
            declared_count: 3,
            branch_tag: 0x29,
        })
    );
    let mut malformed = payload;
    malformed[6] = 1;
    assert!(crate::om::datum_plane_header::datum_plane_payload_header(
        crate::om::operation_record::OperationPayload::new(
            &malformed,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());

    let branch_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x02, 0x23, 0x01, 0x02, 0x80, 0x4c, 0x01, 0xf1, 0x02,
        0xbb, 0x00, 0x14, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00,
    ];
    let branch = crate::om::datum_plane_header::datum_plane_descriptor_reference_branch(
        crate::om::operation_record::OperationPayload::new(
            &branch_payload,
            record.payload_offset(),
            record.name(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(branch.descriptor().unwrap().0.value(), 76);
    assert_eq!(branch.descriptor().unwrap().0.raw().to_vec(), [0x80, 0x4c]);
    assert_eq!(branch.descriptor().unwrap().2, 110);
    assert_eq!(branch.objects().next().unwrap().0.value(), 699);
    assert_eq!(
        branch.objects().next().unwrap().0.raw().to_vec(),
        [0xf1, 0x02, 0xbb]
    );
    assert_eq!(branch.objects().next().unwrap().2, 113);

    let double_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x02, 0x29, 0x01, 0x02, 0xf1, 0x02, 0x77, 0x01, 0x01,
        0x18, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xf1, 0x02, 0x78, 0x01, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x0d,
    ];
    let double = crate::om::datum_plane_header::datum_plane_double_reference_branch(
        crate::om::operation_record::OperationPayload::new(
            &double_payload,
            record.payload_offset(),
            record.name(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        double
            .objects()
            .map(|(token, (), _)| token.value())
            .collect::<Vec<_>>(),
        [631, 632]
    );
    assert_eq!(
        double
            .objects()
            .map(|(_, (), offset)| offset)
            .collect::<Vec<_>>(),
        [110, 124]
    );

    let count_three_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x03, 0x29, 0x01, 0x02, 0xf1, 0x02, 0xcf, 0x01, 0x01,
        0x3a, 0x01, 0x02, 0xf1, 0x02, 0xd0, 0x01, 0x17, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
        0xff, 0xff, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d,
    ];
    let count_three = crate::om::datum_plane_header::datum_plane_double_reference_branch(
        crate::om::operation_record::OperationPayload::new(
            &count_three_payload,
            record.payload_offset(),
            record.name(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        count_three
            .objects()
            .map(|(token, (), _)| token.value())
            .collect::<Vec<_>>(),
        [719, 720]
    );
    assert_eq!(
        count_three
            .objects()
            .map(|(_, (), offset)| offset)
            .collect::<Vec<_>>(),
        [110, 118]
    );

    let descriptor_count_three_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x03, 0x28, 0x01, 0x02, 0x80, 0x4d, 0x01, 0x29, 0x01,
        0x02, 0xf1, 0x02, 0xd1, 0x01, 0x01, 0x07, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff,
        0xff, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d,
    ];
    let descriptor_count_three =
        crate::om::datum_plane_header::datum_plane_descriptor_reference_branch(
            crate::om::operation_record::OperationPayload::new(
                &descriptor_count_three_payload,
                record.payload_offset(),
                record.name(),
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(descriptor_count_three.descriptor().unwrap().0.value(), 77);
    assert_eq!(
        descriptor_count_three
            .descriptor()
            .unwrap()
            .0
            .raw()
            .to_vec(),
        [0x80, 0x4d]
    );
    assert_eq!(descriptor_count_three.descriptor().unwrap().2, 110);
    assert_eq!(
        descriptor_count_three.objects().next().unwrap().0.value(),
        721
    );
    assert_eq!(descriptor_count_three.objects().next().unwrap().2, 116);
}

#[test]
fn om_datum_plane_descriptor_requires_complete_lowercase_hex_identity() {
    let mut bytes = *b"793487222121a5474a9125451b8e31f5?A\xf0\x1e\xff\x02\x01\x33";
    let descriptor = super::datum_plane_descriptor_block(&bytes).unwrap();
    assert_eq!(descriptor.identity(), "793487222121a5474a9125451b8e31f5");
    assert_eq!(descriptor.suffix(), b"?A\xf0\x1e\xff\x02\x01\x33");
    assert_eq!(descriptor.schema_index(), 28_702);
    assert_eq!(descriptor.label(), "3");

    let short_bytes = *b"a75c5f0ed880dd1443b3c5c57908aae?A\xf0\x1f\xff\x02\x01\x66\x33";
    let short = super::datum_plane_descriptor_block(&short_bytes).unwrap();
    assert_eq!(short.identity().len(), 31);
    assert_eq!(short.schema_index(), 28_703);
    assert_eq!(short.label(), "f3");

    bytes[0] = b'G';
    assert!(super::datum_plane_descriptor_block(&bytes).is_none());
    assert!(super::datum_plane_descriptor_block(&bytes[..39]).is_none());
}

#[test]
fn om_datum_csys_descriptor_requires_one_maximal_hex_identity() {
    let bytes = b"\x02\x01ae166162820ea2d993e1fdf49091850e?A\x80\xa0\xf0\x26";
    let descriptor = super::datum_csys_descriptor_block(bytes).unwrap();
    assert_eq!(descriptor.prefix(), [0x02, 0x01]);
    assert_eq!(
        descriptor.identity().as_str(),
        "ae166162820ea2d993e1fdf49091850e"
    );
    assert_eq!(descriptor.prefix().len(), 2);
    assert_eq!(descriptor.suffix(), b"?A\x80\xa0\xf0\x26");

    let mut ambiguous = bytes.to_vec();
    ambiguous.extend_from_slice(b"012345678901234567890123456789");
    assert!(super::datum_csys_descriptor_block(&ambiguous).is_none());
}

#[test]
fn om_draft_identity_frames_require_complete_typed_framing() {
    let bytes = b"\x00A\x81\x54\xf0\x38\x02\x01abc123?A\xf0\x27\xff\x02\x01def456?\x00";
    let frames = super::draft_construction_identity_frames(bytes);
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].offset(), 1);
    assert_eq!(frames[0].prefix(), b"A\x81\x54\xf0\x38\x02\x01");
    assert_eq!(
        frames[0].form(),
        crate::om::draft_identity::DraftIdentityForm::IndexedBranch {
            first_index: 340,
            second_index: Some(56),
            branch: crate::om::discriminators::DraftIdentityBranch::Form02,
        }
    );
    assert_eq!(frames[0].identity(), "abc123");
    assert_eq!(frames[0].identity_offset(), 8);
    assert_eq!(frames[1].offset(), 15);
    assert_eq!(frames[1].prefix(), b"A\xf0\x27\xff\x02\x01");
    assert_eq!(
        frames[1].form(),
        crate::om::draft_identity::DraftIdentityForm::Tagged { index: Some(39) }
    );
    assert_eq!(frames[1].identity(), "def456");

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
    assert_eq!(lanes[0].offset(), 1);
    assert_eq!(
        lanes[0]
            .iter()
            .map(|(_, atom, ())| atom.scalar.value())
            .collect::<Vec<_>>(),
        [0.5, -0.5]
    );
    assert_eq!(
        lanes[0]
            .iter()
            .map(|(_, atom, ())| atom.marker.byte())
            .collect::<Vec<_>>(),
        [0x30, 0xb0]
    );
    assert_eq!(
        lanes[0]
            .iter()
            .map(|(offset, _, ())| offset)
            .collect::<Vec<_>>(),
        [19, 27]
    );

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
    assert_eq!(lanes[0].offset(), 1);
    assert_eq!(lanes[0].form().discriminator(), discriminator);
    assert_eq!(u8::from(lanes[0].form()), 4);
    assert_eq!(
        lanes[0]
            .iter()
            .map(|(_, scalar, ())| scalar.value())
            .collect::<Vec<_>>(),
        [1.0, -1.0]
    );
    assert_eq!(
        lanes[0]
            .iter()
            .map(|(offset, _, ())| offset)
            .collect::<Vec<_>>(),
        [19, 23]
    );

    bytes.pop();
    assert!(super::draft_construction_binary32_lanes(&bytes).is_empty());
    bytes.truncate(21);
    assert!(super::draft_construction_binary32_lanes(&bytes).is_empty());
    assert!(super::draft_construction_binary32_lanes(&discriminator).is_empty());
}

#[test]
fn om_operation_primary_body_reference_requires_one_complete_field() {
    let label = "EXTRUDE";
    let bytes = [0x01, 0x02, 0x10, 0x90, 0x19, 0x42, 0xff];
    let record =
        crate::om::operation_record::OperationBodyInput::new(&bytes, 100, 0, label).unwrap();
    assert_eq!(
        super::operation_body_reference(record),
        Some(super::OperationBodyReference {
            offset: 103,
            object_index: crate::om::reference_index::FeatureReferenceToken::from_wire(
                6466,
                &[0x90, 0x19, 0x42]
            )
            .unwrap(),
        })
    );

    let duplicate = [bytes.as_slice(), bytes.as_slice()].concat();
    assert_eq!(
        super::operation_body_references(
            crate::om::operation_record::OperationBodyInput::new(&duplicate, 100, 0, label)
                .unwrap()
        ),
        [
            super::OperationBodyReference {
                offset: 103,
                object_index: crate::om::reference_index::FeatureReferenceToken::from_wire(
                    6466,
                    &[0x90, 0x19, 0x42]
                )
                .unwrap(),
            },
            super::OperationBodyReference {
                offset: 110,
                object_index: crate::om::reference_index::FeatureReferenceToken::from_wire(
                    6466,
                    &[0x90, 0x19, 0x42]
                )
                .unwrap(),
            },
        ]
    );
    assert!(super::operation_body_reference(
        crate::om::operation_record::OperationBodyInput::new(&duplicate, 100, 0, label).unwrap()
    )
    .is_none());
}

#[test]
fn om_operation_body_write_is_not_a_direct_primary_body_reference() {
    let label = "EXTRUDE";
    let bytes = [
        0x01, 0x02, 0x0b, 0xa0, 0x66, 0xa4, 0x97, 0x75, 0x01, 0x02, 0x10, 0x43, 0xff,
    ];
    let record =
        crate::om::operation_record::OperationBodyInput::new(&bytes, 100, 0, label).unwrap();
    assert!(super::operation_body_reference(record).is_none());
    assert_eq!(
        super::operation_body_write_frames(record.payload_view()),
        [{
            let frame = crate::om::body_write::BodyWriteFrame::<usize>::new(
                0x0b,
                crate::om::body_write::BodyWriteIndex::from_wire(0x66a4, &[0xa0, 0x66, 0xa4])
                    .unwrap(),
                crate::om::body_write::BodyImageTag::try_from(0x10).unwrap(),
                crate::om::body_write::BodyWriteIndex::from_wire(0x43, &[0x43]).unwrap(),
                100,
            )
            .unwrap();
            assert_eq!(frame.group_node_offset(), 103);
            assert_eq!(frame.body_image_offset(), 111);
            assert_eq!(frame.end_offset(), 113);
            frame
        }]
    );

    let mut invalid_endpoint_tag = bytes;
    invalid_endpoint_tag[10] = 0x11;
    let nested_record = crate::om::operation_record::OperationBodyInput::new(
        &invalid_endpoint_tag,
        record.offset(),
        record.payload_start(),
        record.name(),
    )
    .unwrap();
    assert!(super::operation_body_references(nested_record).is_empty());
}

#[test]
fn om_operation_object_relation_requires_complete_canonical_endpoints() {
    let label = "EXTRUDE";
    let payload = [
        0x01, 0x02, 0x17, 0x81, 0x23, 0x97, 0x75, 0x01, 0x02, 0x10, 0x86, 0x45, 0xff, 0x01, 0x02,
        0x10, 0x81, 0x23, 0xff,
    ];
    let record = crate::om::operation_record::OperationPayload::new(&payload, 100, label).unwrap();
    assert_eq!(
        super::operation_body_write_frames(record),
        [{
            let frame = crate::om::body_write::BodyWriteFrame::<usize>::new(
                0x17,
                crate::om::body_write::BodyWriteIndex::from_wire(0x123, &[0x81, 0x23]).unwrap(),
                crate::om::body_write::BodyImageTag::try_from(0x10).unwrap(),
                crate::om::body_write::BodyWriteIndex::from_wire(0x645, &[0x86, 0x45]).unwrap(),
                100,
            )
            .unwrap();
            assert_eq!(frame.group_node_offset(), 103);
            assert_eq!(frame.body_image_offset(), 110);
            assert_eq!(frame.end_offset(), 113);
            frame
        }]
    );

    let mut noncanonical_first = payload;
    noncanonical_first[3] = 0x80;
    assert!(super::operation_body_write_frames(
        crate::om::operation_record::OperationPayload::new(
            &noncanonical_first,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_empty());

    let mut truncated = payload[..13].to_vec();
    truncated.pop();
    assert!(super::operation_body_write_frames(
        crate::om::operation_record::OperationPayload::new(
            &truncated,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_empty());

    let direct_body = [0x01, 0x02, 0x10, 0x81, 0x23, 0xff];
    assert!(super::operation_body_write_frames(
        crate::om::operation_record::OperationPayload::new(
            &direct_body,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_empty());

    let nested = [
        0x01, 0x02, 0x11, 0x80, 0xa9, 0x97, 0x75, 0x01, 0x02, 0x10, 0x86, 0x93, 0xff,
    ];
    let nested_relations = super::operation_body_write_frames(
        crate::om::operation_record::OperationPayload::new(
            &nested,
            record.payload_offset(),
            record.name(),
        )
        .unwrap(),
    );
    assert_eq!(nested_relations.len(), 1);
    assert_eq!(nested_relations[0].body_identity(), 0x11);
    assert_eq!(nested_relations[0].group_node().value(), 0xa9);
    assert_eq!(nested_relations[0].body_image().value(), 0x693);
}

#[test]
fn om_operation_terminal_frame_requires_one_canonical_common_frame() {
    let terminal = |value, raw: &[u8], object, object_raw: &[u8], offset, object_offset| {
        let suffix =
            crate::om::common_frame::CommonFrameSuffix::from_wire(value, raw, object, object_raw)
                .unwrap();
        let frame = crate::om::common_frame::TerminalFrame::<usize>::new(suffix, offset).unwrap();
        assert_eq!(
            frame.offset() + 2 * frame.suffix().raw_local_ordinal().len(),
            object_offset
        );
        frame
    };
    let label = "FSET";
    let bytes = [
        0x00, 0x81, 0x5f, 0x80, 0xab, 0x01, 0x03, 0x02, 0x01, 0x02, 0x01, 0x01, 0x01, 0x00, 0x00,
        0x00, 0x81, 0x23, 0x81, 0x23, 0xff, 0x00,
    ];
    let record = crate::om::operation_record::OperationPayload::new(&bytes, 104, label).unwrap();
    assert_eq!(
        super::operation_terminal_frame(record),
        Some(super::OperationTerminalFrame {
            immediate_common_frame_offset: Some(104),
            frame: terminal(0x0123, &[0x81, 0x23], None, &[0xff], 120, 124),
        })
    );

    let direct = [
        0x00, 0x81, 0x5f, 0x80, 0xab, 0x01, 0x03, 0x02, 0x01, 0x02, 0x01, 0x01, 0x01, 0x00, 0x00,
        0x00, 0x29, 0x29, 0x41, 0x00,
    ];
    assert_eq!(
        super::operation_terminal_frame(
            crate::om::operation_record::OperationPayload::new(&direct, 200, label).unwrap()
        ),
        Some(super::OperationTerminalFrame {
            immediate_common_frame_offset: Some(200),
            frame: terminal(41, &[0x29], Some(65), &[0x41], 216, 218),
        })
    );

    let noncanonical = [
        0x00, 0x81, 0x5f, 0x80, 0xab, 0x01, 0x03, 0x02, 0x01, 0x02, 0x01, 0x01, 0x01, 0x00, 0x00,
        0x00, 0x80, 0x01, 0x80, 0x01, 0xff, 0x00,
    ];
    assert!(super::operation_terminal_frame(
        crate::om::operation_record::OperationPayload::new(&noncanonical, 0, label).unwrap()
    )
    .is_none());
    let mismatched = [
        0x00, 0x81, 0x5f, 0x80, 0xab, 0x01, 0x03, 0x02, 0x01, 0x02, 0x01, 0x01, 0x01, 0x00, 0x00,
        0x00, 0x23, 0x24, 0xff, 0x00,
    ];
    assert!(super::operation_terminal_frame(
        crate::om::operation_record::OperationPayload::new(&mismatched, 0, label).unwrap()
    )
    .is_none());

    let delete = [
        0x01, 0x00, 0x00, 0x01, 0x01, 0x01, 0x06, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x29,
        0x29, 0x41, 0x00,
    ];
    let delete_frame = super::operation_terminal_frame(
        crate::om::operation_record::OperationPayload::new(&delete, 300, "DELETE").unwrap(),
    )
    .expect("DELETE common-frame variant");
    assert_eq!(delete_frame.immediate_common_frame_offset, Some(300));
    let [delete_common] = super::operation_common_frames(
        crate::om::operation_record::OperationPayload::new(&delete, 300, "DELETE").unwrap(),
    )
    .try_into()
    .expect("one DELETE common frame");
    assert_eq!(delete_common.prefix().indices(), [1, 0, 0]);
    assert_eq!(delete_common.prefix().marker(), [1, 1, 1]);
    assert_eq!(delete_common.state(), [6, 1, 1, 0, 1, 0, 0, 0]);

    let suffix_only = [0x02, 0x02, 0xff, 0x00];
    let suffix = super::operation_terminal_frame(
        crate::om::operation_record::OperationPayload::new(&suffix_only, 400, label).unwrap(),
    )
    .expect("canonical suffix without immediate state prefix");
    assert!(suffix.immediate_common_frame_offset.is_none());
    assert_eq!(suffix.frame.suffix().local_ordinal(), 2);
    assert_eq!(suffix.frame.offset(), 400);

    let mut embedded = direct.to_vec();
    embedded.extend_from_slice(&[0xaa, 0x02, 0x02, 0xff, 0x00]);
    let embedded_record =
        crate::om::operation_record::OperationPayload::new(&embedded, 500, label).unwrap();
    let [common] = super::operation_common_frames(embedded_record)
        .try_into()
        .expect("one embedded common frame");
    assert_eq!(common.offset(), 500);
    assert_eq!(common.end_offset(), 520);
    let outer = super::operation_terminal_frame(embedded_record).expect("outer suffix");
    assert_eq!(outer.frame.offset(), 521);
    assert!(outer.immediate_common_frame_offset.is_none());
}

#[test]
fn om_fset_reference_graph_requires_exact_groups_and_bounds() {
    fn record(payload: &[u8]) -> crate::om::operation_record::OperationPayload<'_> {
        crate::om::operation_record::OperationPayload::new(payload, 100, "FSET").unwrap()
    }

    let payload = [
        0x01, 0x13, 0x3c, b'T', b';', b':', b'S', b'5', b'6', b'7', b'R', b'8', b'9', b'3', 0x90,
        0x19, 0x40, 0x90, 0x19, 0x41, 0x3e, 0x90, 0x19, 0x30, 0x90, 0x19, 0x31, 0x90, 0x19, 0x32,
        0x00, 0x03, 0x00,
    ];
    let graph = crate::om::fset_references::FsetReferences::read(record(&payload)).unwrap();
    assert_eq!(graph.selector(), "T;:S567R893");
    assert_eq!(graph.offset(), 100);
    assert_eq!(
        graph
            .first()
            .each_ref()
            .map(|(index, ())| u32::from(*index)),
        [6464, 6465]
    );
    assert_eq!(
        graph
            .second()
            .each_ref()
            .map(|(index, ())| u32::from(*index)),
        [6448, 6449, 6450]
    );
    let raw = graph
        .first()
        .each_ref()
        .map(|(index, ())| crate::om::fset_references::word_reference_bytes(*index));
    assert_eq!(
        raw.each_ref().map(<[u8; 3]>::as_slice),
        [[0x90, 0x19, 0x40].as_slice(), [0x90, 0x19, 0x41].as_slice(),]
    );

    let mut wrong_length = payload;
    wrong_length[1] -= 1;
    assert!(crate::om::fset_references::FsetReferences::read(record(&wrong_length)).is_none());
    let mut wrong_suffix = payload;
    wrong_suffix[31] = 0x04;
    assert!(crate::om::fset_references::FsetReferences::read(record(&wrong_suffix)).is_none());
    let mut wrong_reference_form = payload;
    wrong_reference_form[14] = 0xf1;
    assert!(
        crate::om::fset_references::FsetReferences::read(record(&wrong_reference_form)).is_none()
    );
    let duplicate = [payload.as_slice(), payload.as_slice()].concat();
    assert!(crate::om::fset_references::FsetReferences::read(record(&duplicate)).is_none());
}

#[test]
fn om_delete_reference_field_requires_five_canonical_nullable_slots() {
    fn record(payload: &[u8]) -> crate::om::operation_record::OperationPayload<'_> {
        crate::om::operation_record::OperationPayload::new(payload, 100, "DELETE").unwrap()
    }

    let payload = [
        0x0c, 0x00, 0x00, 0x01, 0x00, 0x01, 0x06, 0xf0, 0x20, 0xff, 0xf1, 0x02, 0x08, 0xf1, 0x02,
        0x09, 0xff, 0x00,
    ];
    let field = crate::om::delete_references::DeleteReferences::read(record(&payload)).unwrap();
    assert_eq!(field.control(), 0x0c);
    assert_eq!(field.offset(), 100);
    assert_eq!(
        field
            .slots()
            .each_ref()
            .map(|reference| reference.as_ref().map(|(token, ())| token.value())),
        [Some(0x20), None, Some(0x208), Some(0x209), None]
    );
    assert_eq!(field.reference_offsets(), [107, 109, 110, 113, 116]);

    let mut noncanonical = payload;
    noncanonical[11] = 0x00;
    noncanonical[12] = 0x20;
    assert!(crate::om::delete_references::DeleteReferences::read(record(&noncanonical)).is_none());
    let truncated = &payload[..payload.len() - 1];
    assert!(crate::om::delete_references::DeleteReferences::read(record(truncated)).is_none());
    let mut wrong_count = payload;
    wrong_count[6] = 0x05;
    assert!(crate::om::delete_references::DeleteReferences::read(record(&wrong_count)).is_none());
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
                object_index: crate::om::reference_index::FeatureReferenceToken::from_wire(
                    42,
                    &[0x2a]
                )
                .unwrap(),
            },
            super::DataBlockObjectReference {
                offset: 8,
                object_index: crate::om::reference_index::FeatureReferenceToken::from_wire(
                    201,
                    &[0x80, 0xc9]
                )
                .unwrap(),
            },
            super::DataBlockObjectReference {
                offset: 14,
                object_index: crate::om::reference_index::FeatureReferenceToken::from_wire(
                    6466,
                    &[0x90, 0x19, 0x42]
                )
                .unwrap(),
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
        &sections[0].types[0].registry_tail[1..],
        &[0x81, 0x21, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x06]
    );
    assert_eq!(sections[0].types[1].registry_tail[0], 0x65);
    assert_eq!(sections[0].fields.len(), 2);
    assert_eq!(sections[0].fields[0].name, "m_target");
    assert_eq!(sections[0].fields[1].registry_tail[0], 0x81);
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
    let offset = section.record_area.expect("record area").offset;
    assert_eq!(offset, size_framed_om_section().len() + 20);
    assert_eq!(
        section.record_area.expect("record area").bytes,
        &bytes[offset..]
    );
    assert_eq!(&bytes[offset + 12..offset + 15], &[0x05, 0x01, 0x0e]);

    let mut invalid = bytes;
    invalid[offset + 12] = 1;
    assert_eq!(super::sections(&invalid)[0].record_area, None);
}

fn legacy_feature_om_section_with_record_area() -> Vec<u8> {
    let mut bytes = vec![0xff; 16];
    bytes[12..14].copy_from_slice(b"OM");
    bytes.extend_from_slice(&[0, 1, 2]);
    let class_name = b"UGS::FEATURE_RECORD";
    bytes.push((class_name.len() + 1) as u8);
    bytes.extend_from_slice(class_name);
    bytes.push(0xa0);
    bytes.extend_from_slice(&[0x81, 0x21, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0x06]);
    let pointer_offset = bytes.len();
    let record_area_offset = pointer_offset + 20;
    bytes.push(0x01);
    bytes.extend_from_slice(&((record_area_offset - 1) as u32).to_le_bytes());
    bytes.resize(record_area_offset, 0);
    bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    bytes.extend_from_slice(b"\x01\x0eNX 1980.1700\0");
    bytes.extend_from_slice(
        b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x07UNITE\0",
    );
    let payload_len = (bytes.len() - 16) as u32;
    bytes[8..12].copy_from_slice(&payload_len.to_be_bytes());
    bytes
}

#[test]
fn om_feature_section_accepts_the_legacy_record_area_pointer_and_product_frame() {
    let bytes = legacy_feature_om_section_with_record_area();
    let section = super::sections(&bytes).remove(0);
    let record_area_offset = section.record_area.expect("record area").offset;
    assert_eq!(
        record_area_offset,
        16 + 3 + 1 + b"UGS::FEATURE_RECORD".len() + 1 + 12 + 20
    );
    assert_eq!(
        section
            .record_area_header()
            .expect("record header")
            .product
            .value
            .as_str(),
        "NX 1980.1700"
    );
    assert_eq!(section.operation_labels().len(), 1);
    assert_eq!(section.operation_labels()[0].value, "UNITE");

    let mut invalid = bytes;
    invalid[record_area_offset + 13] = 0x02;
    assert!(super::sections(&invalid)[0].record_area.is_none());
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
    assert_eq!(
        section.record_area.map(|area| area.offset),
        Some(record_area_offset)
    );
}

#[test]
fn om_operation_labels_require_the_complete_frame() {
    let bytes = b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\x01\x82\x40\x90\x17\xd3\xff\x03\x07UNITE\0\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\x02\x03\xff\xff\x03\x08SKETCH\0";
    let labels = super::operation_labels(bytes, 100);
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[0].header.end_offset(), 122);
    assert_eq!(labels[0].header.offset(), 100);
    assert_eq!(labels[0].value, "UNITE");
    assert_eq!(
        labels[0].header.objects().values(),
        [Some(1), Some(576), Some(6099), None]
    );
    assert_eq!(labels[1].value, "SKETCH");
    assert_eq!(
        labels[1].header.objects().values(),
        [Some(2), Some(3), None, None]
    );

    assert!(super::operation_labels(b"\xff\xff\x03\x07UNITE\0", 0).is_empty());
    let mut invalid = bytes.to_vec();
    invalid[15] = 0x91;
    assert_eq!(super::operation_labels(&invalid, 0).len(), 1);
}

#[test]
fn om_operation_records_use_consecutive_validated_headers() {
    let bytes = b"prefix\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x07UNITE\0payload\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x08SKETCH\0tail";
    let labels = super::operation_labels(bytes, 10);
    let records_with_ordinals =
        super::operation_records_with_labels_and_ordinals(bytes, 10, &labels);
    let records = records_with_ordinals
        .iter()
        .map(|(_, record)| record)
        .collect::<Vec<_>>();
    assert_eq!(records_with_ordinals[0].0, 0);
    assert_eq!(records_with_ordinals[1].0, 1);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].offset(), 16);
    assert_eq!(records[0].label().value, "UNITE");
    assert!(records[0].bytes().ends_with(b"payload"));
    assert_eq!(records[0].payload(), b"payload");
    assert_eq!(records[0].payload_offset(), 43);
    assert_eq!(records[1].label().value, "SKETCH");
    assert!(records[1].bytes().ends_with(b"tail"));
    assert_eq!(records[1].payload(), b"tail");
}

#[test]
fn om_operation_payload_strings_require_complete_utf8_frames() {
    let label = "SIMPLE HOLE";
    let payload = b"\x00\x04\x07BLOCK\0\x04\x04\xc3\x97\0\x04\x07BROKEN";
    let record = crate::om::operation_record::OperationPayload::new(payload, 200, label).unwrap();
    let strings = super::operation_payload_strings(record);
    assert_eq!(strings.len(), 2);
    assert_eq!(strings[0].offset, 201);
    assert_eq!(strings[0].value.as_str(), "BLOCK");
    assert_eq!(strings[1].value.as_str(), "×");
}

#[test]
fn om_operation_payload_text_frames_retain_marker_and_order() {
    let label = "SYMBOLIC_THREAD";
    let payload = b"\x03\x05CUT\0\x04\x06DONE\0\x03\x0bM Profile\0";
    let record = crate::om::operation_record::OperationPayload::new(payload, 200, label).unwrap();
    let frames = super::operation_payload_text_frames(record);
    assert_eq!(
        frames,
        vec![
            super::OperationPayloadTextFrame {
                marker: super::OperationTextMarker::Text,
                offset: 200,
                value: crate::payload_text::PayloadText::new("CUT").unwrap(),
            },
            super::OperationPayloadTextFrame {
                marker: super::OperationTextMarker::String,
                offset: 206,
                value: crate::payload_text::PayloadText::new("DONE").unwrap(),
            },
            super::OperationPayloadTextFrame {
                marker: super::OperationTextMarker::Text,
                offset: 213,
                value: crate::payload_text::PayloadText::new("M Profile").unwrap(),
            },
        ]
    );
}
mod operation_reference_lanes;
