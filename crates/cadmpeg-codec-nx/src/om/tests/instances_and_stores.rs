// SPDX-License-Identifier: Apache-2.0
//! Unit and fixture tests for OM wire parsers owned by `om`.

#![allow(clippy::unwrap_used)]

use crate::om::pattern_references::PatternReferences;
use cadmpeg_core::decode::View;

use crate::test_support::*;

fn fixed_indexed_section_with_embedded_section(adjust_outer_bounds: bool) -> Vec<u8> {
    let outer = indexed_om_section();
    let inner = indexed_om_section();
    let table = outer
        .windows(16)
        .enumerate()
        .find_map(|(offset, window)| {
            (View::u32_le_at(window, 0) == Some(3)
                && View::u32_le_at(window, 4) == Some(0x100)
                && View::u32_le_at(window, 8) == Some(0x101)
                && View::u32_le_at(window, 12) == Some(0x102))
            .then_some(offset)
        })
        .expect("outer object-id table");
    let index_start = table - 16;
    let table_end = table + 16;
    let first_offset = usize::try_from(View::u32_le_at(&outer, index_start + 4).unwrap()).unwrap();
    let second_offset = usize::try_from(View::u32_le_at(&outer, index_start + 8).unwrap()).unwrap();
    let base = table_end - first_offset;
    let insertion_at = base + second_offset;
    let mut bytes = outer[..insertion_at].to_vec();
    bytes.extend_from_slice(&inner);
    bytes.extend_from_slice(&outer[insertion_at..]);
    if adjust_outer_bounds {
        for ordinal in 2..=3 {
            let offset = index_start + ordinal * 4;
            let value = View::u32_le_at(&outer, offset).unwrap() as usize + inner.len();
            bytes[offset..offset + 4].copy_from_slice(&(value as u32).to_le_bytes());
        }
    }
    bytes
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
    let label = "Multi Instance Output";
    let record = crate::om::operation_record::OperationPayload::new(&payload, 200, label).unwrap();
    let lane = super::multi_instance_output_payload_lane(record).expect("complete output lane");
    assert_eq!(lane.offset, 209);
    assert_eq!(lane.outputs.selectors().len() + 1, 7);
    assert_eq!(
        lane.outputs
            .selectors()
            .iter()
            .map(|row| row.atom.value())
            .collect::<Vec<_>>(),
        [2, 2, 3, 3, 4, 4]
    );
    assert_eq!(
        lane.outputs.ordinals().collect::<Vec<_>>(),
        [2, 3, 2, 3, 2, 3]
    );
    assert_eq!(lane.outputs.references().len() + 1, 3);
    assert_eq!(
        lane.outputs
            .selectors()
            .iter()
            .map(|row| row.offset)
            .collect::<Vec<_>>(),
        [219, 230, 241, 252, 263, 274]
    );
    assert_eq!(
        lane.outputs
            .references()
            .iter()
            .map(|reference| reference.token.value())
            .collect::<Vec<_>>(),
        [15850, 15851]
    );
    assert_eq!(lane.outputs.references()[0].offset, 280);
    assert_eq!(
        lane.outputs.references()[0].token.raw().to_vec(),
        [0x90, 0x3d, 0xea]
    );

    let mut incomplete_group = payload.clone();
    incomplete_group[76] = 2;
    assert!(super::multi_instance_output_payload_lane(
        crate::om::operation_record::OperationPayload::new(
            &incomplete_group,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
    let mut wrong_row_index = payload.clone();
    wrong_row_index[77] = 6;
    assert!(super::multi_instance_output_payload_lane(
        crate::om::operation_record::OperationPayload::new(
            &wrong_row_index,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(super::multi_instance_output_payload_lane(
        crate::om::operation_record::OperationPayload::new(
            &ambiguous,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
}

#[test]
fn om_identical_instance_output_lane_requires_complete_ordered_rows() {
    let payload = b"\xaa\x34\x13\x01\x04\x14\x15\x01\x02\x16\x80\x20\x00\x02\
          \x14\x15\x01\x02\x16\x0f\x00\x03\
          \x14\x15\x01\x02\x16\x81\x23\x00\x04\
          \x00\x05\xe0\x7f\xff\xff\xff\x00\x00\xbb";
    let label = "IDENTICAL INSTANCE OUTPUT";
    let record = crate::om::operation_record::OperationPayload::new(payload, 200, label).unwrap();
    let lane = super::identical_instance_output_payload_lane(record)
        .expect("complete identical-instance lane");
    assert_eq!(lane.offset, 201);
    assert_eq!(lane.leading_schema_index, 0x34);
    assert_eq!(lane.count_schema_index.value(), 0x13);
    assert_eq!(lane.count_schema_index.row_indices(), [0x14, 0x15, 0x16]);
    assert_eq!(lane.selectors.as_slice().len() + 1, 4);
    assert_eq!(
        lane.selectors
            .as_slice()
            .iter()
            .map(|row| row.atom.value())
            .collect::<Vec<_>>(),
        [0x20, 0x0f, 0x123]
    );
    assert_eq!(
        lane.selectors
            .as_slice()
            .iter()
            .map(|row| row.atom.raw().to_vec())
            .collect::<Vec<_>>(),
        [vec![0x80, 0x20], vec![0x0f], vec![0x81, 0x23]]
    );
    assert_eq!(
        lane.selectors
            .as_slice()
            .iter()
            .map(|row| row.offset)
            .collect::<Vec<_>>(),
        [210, 219, 227]
    );

    let mut wrong_ordinal = payload.to_vec();
    wrong_ordinal[21] = 4;
    assert!(super::identical_instance_output_payload_lane(
        crate::om::operation_record::OperationPayload::new(
            &wrong_ordinal,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
    let mut wrong_terminal_count = payload.to_vec();
    wrong_terminal_count[32] = 4;
    assert!(super::identical_instance_output_payload_lane(
        crate::om::operation_record::OperationPayload::new(
            &wrong_terminal_count,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(super::identical_instance_output_payload_lane(
        crate::om::operation_record::OperationPayload::new(
            &ambiguous,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
}

#[test]
fn om_geometry_instance_reference_requires_one_complete_field() {
    let label = "Geometry Instance";
    let payload = b"\x44\x45\x00\xff\xff\xf1\x03\x21\x01\x02\x00\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x01\x02";
    let record = crate::om::operation_record::OperationPayload::new(payload, 200, label).unwrap();
    let field = PatternReferences::read(record).expect("complete field");
    let references = field.into_references();
    assert_eq!(references[0].token.value(), 801);
    assert_eq!(references[0].offset, 205);

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(PatternReferences::read(
        crate::om::operation_record::OperationPayload::new(
            &ambiguous,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
}

#[test]
fn om_point_feature_header_requires_the_complete_leading_envelope() {
    let label = "POINT";
    let payload = b"\x72\x00\x00\x01\x00\x00\x00\xf1\x1c\x8f\x00\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x0d\x01\x02\x01\x00\x00\x00\x89\x02\x01\x01\x01\x00\xa5\x57\x95\x01\x00\x00\xff\x02\xc0\x1f\xff\xfd\x01\x00\x00\x01\x01\x01\x03\x02\x01\x01\x01\x00\x00\x00\x00\x00\xaa";
    let record = crate::om::operation_record::OperationPayload::new(payload, 200, label).unwrap();
    let header = super::point_feature_payload_header(record).expect("complete header");
    assert_eq!(header.reference.token.value(), 7311);
    assert_eq!(header.reference.offset, 207);
    assert_eq!(u8::from(header.mode), 0x02);

    let mut alternate_mode = payload.to_vec();
    alternate_mode[52] = 0x03;
    assert_eq!(
        super::point_feature_payload_header(
            crate::om::operation_record::OperationPayload::new(
                &alternate_mode,
                record.payload_offset(),
                record.name()
            )
            .unwrap()
        )
        .expect("alternate mode")
        .mode,
        crate::om::discriminators::PointHeaderMode::Form03
    );

    for malformed_offset in [0, 10, 51, 72] {
        let mut malformed = payload.to_vec();
        malformed[malformed_offset] ^= 0x01;
        assert!(super::point_feature_payload_header(
            crate::om::operation_record::OperationPayload::new(
                &malformed,
                record.payload_offset(),
                record.name()
            )
            .unwrap()
        )
        .is_none());
    }
    let mut unsupported_mode = payload.to_vec();
    unsupported_mode[52] = 0x04;
    assert!(super::point_feature_payload_header(
        crate::om::operation_record::OperationPayload::new(
            &unsupported_mode,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
    assert!(super::point_feature_payload_header(
        crate::om::operation_record::OperationPayload::new(
            &payload[..72],
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
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
    assert_eq!(
        lane.values.map(crate::om::scalar::ShiftedBinary64::value),
        [1.0, -2.0, 3.5, 4.0, 5.25, -6.0]
    );
    assert_eq!(
        lane.values
            .map(crate::om::scalar::ShiftedBinary64::raw)
            .concat(),
        encoded
    );
    assert_eq!(lane.value_offsets(), [2, 10, 18, 26, 34, 42]);

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
    let label = "DRAFT";
    let prefix = b"\x67\x00\x00\x01\x00\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\xff\xff\xff\xff\xff\xff\xff\xff\x01\x03\x80\x94\x82\x49";
    let graph = b"\x01\x02\xf1\x1b\x7c\x01\x02\xf1\x1b\x7d\x68\x2f\x70\x62\x4d\xd2\xf1\xa9\xfc\x03\x50\x44\x00\x00\x01\x46\x8a\x2a\x01\xa3\x60\x10\x01\x01\x01\x04\x02\x01\x02\x01\x00\x00\x00\x00\x01\xf1\x1b\x7e\xff\x00\x00\x00\xf1\x1b\x7f\xff";
    let terminal = b"\x81\x5e\x80\xb8\x01\x03\x02\x01\x02\x01\x01\x01\x00\x00\x00\x29\x29\x0c\x00";
    let payload = [prefix.as_slice(), graph.as_slice(), terminal.as_slice()].concat();
    let record = crate::om::operation_record::OperationPayload::new(&payload, 200, label).unwrap();
    let field = crate::om::draft_references::draft_feature_payload_references(record)
        .expect("complete graph");
    assert_eq!(
        field.references().map(|(token, _)| token.value()),
        [7036, 7037, 7038, 7039]
    );
    assert_eq!(
        field.references().map(|(_, offset)| offset),
        [230, 235, 273, 280]
    );
    let lane = crate::om::draft_leading::scan(record).expect("complete index lane");
    assert_eq!(usize::from(lane.declared_count()), 3);
    assert_eq!(
        lane.indices()
            .map(|token| (token.atom.value(), token.offset))
            .collect::<Vec<_>>(),
        vec![(148, 224), (585, 226)]
    );
    assert_eq!(
        lane.indices()
            .map(|token| token.atom.raw().to_vec())
            .collect::<Vec<_>>(),
        vec![vec![0x80, 0x94], vec![0x82, 0x49]]
    );
    let terminal_lane = crate::om::draft_terminal::scan(record).expect("complete terminal lane");
    assert_eq!(
        terminal_lane.indices().map(|token| token.atom.value()),
        [350, 184]
    );
    assert_eq!(
        terminal_lane.indices().map(|token| *token.atom.raw()),
        [[0x81, 0x5e], [0x80, 0xb8]]
    );
    assert_eq!(
        terminal_lane.indices().map(|token| token.offset),
        [284, 286]
    );
    assert_eq!(terminal_lane.tail(), [0x29, 0x29, 0x0c]);

    let mut malformed = payload.clone();
    malformed[53] = 0x00;
    assert!(
        crate::om::draft_references::draft_feature_payload_references(
            crate::om::operation_record::OperationPayload::new(
                &malformed,
                record.payload_offset(),
                record.name()
            )
            .unwrap()
        )
        .is_none()
    );
    let mut malformed_lane = payload.clone();
    malformed_lane[23] = 4;
    assert!(crate::om::draft_leading::scan(
        crate::om::operation_record::OperationPayload::new(
            &malformed_lane,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
    let ambiguous = [prefix.as_slice(), graph.as_slice(), graph.as_slice()].concat();
    assert!(
        crate::om::draft_references::draft_feature_payload_references(
            crate::om::operation_record::OperationPayload::new(
                &ambiguous,
                record.payload_offset(),
                record.name()
            )
            .unwrap()
        )
        .is_none()
    );
    assert!(
        crate::om::draft_references::draft_feature_payload_references(
            crate::om::operation_record::OperationPayload::new(
                &payload[..prefix.len() + graph.len() - 2],
                record.payload_offset(),
                record.name()
            )
            .unwrap()
        )
        .is_none()
    );
    assert!(crate::om::draft_terminal::scan(
        crate::om::operation_record::OperationPayload::new(
            &payload[..payload.len() - 1],
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
}

#[test]
fn om_surface_feature_references_require_the_complete_common_envelope() {
    let label = "SKIN";
    let payload = b"\x3f\x00\x00\x01\x00\xf1\x02\x46\xf1\x02\x47\xf1\x02\x48\x01\x09\x03\x03\x04\x05\x02\x01\x01\x01\x01\x09\xf1\x02\x49\xf1\x02\x4a\xf1\x02\x4b\xf1\x02\x4c\xf1\x02\x4d\xf1\x02\x4e\xf1\x02\x4f\xf1\x02\x50\x00\x03\x03\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xf1\x02\x56\xf1\x02\x57\xf1\x02\x58\x01\x01\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x01\x02";
    let record = crate::om::operation_record::OperationPayload::new(payload, 200, label).unwrap();
    let field = crate::om::surface_envelope::surface_feature_payload_references(record)
        .expect("complete envelope");
    assert_eq!(
        field
            .references()
            .iter()
            .map(|(token, _)| token.value())
            .collect::<Vec<_>>(),
        [582, 583, 584, 585, 586, 587, 588, 589, 590, 591, 592, 598, 599, 600,]
    );

    let studio_payload = [&[0x14], &payload[1..]].concat();
    let studio = crate::om::operation_record::OperationPayload::new(
        &studio_payload,
        record.payload_offset(),
        "Studio Surface",
    )
    .unwrap();
    assert!(crate::om::surface_envelope::surface_feature_payload_references(studio).is_some());

    let mut malformed = payload.to_vec();
    let last = malformed.len() - 1;
    malformed[last] = 0x00;
    assert!(
        crate::om::surface_envelope::surface_feature_payload_references(
            crate::om::operation_record::OperationPayload::new(
                &malformed,
                record.payload_offset(),
                record.name()
            )
            .unwrap()
        )
        .is_none()
    );

    let ambiguous = [payload.as_slice(), &payload[51..]].concat();
    assert!(
        crate::om::surface_envelope::surface_feature_payload_references(
            crate::om::operation_record::OperationPayload::new(
                &ambiguous,
                record.payload_offset(),
                record.name()
            )
            .unwrap()
        )
        .is_none()
    );
}

#[test]
fn om_thru_curve_references_require_the_complete_leading_envelope() {
    let label = "THRU_CURVE";
    let payload = b"\x13\x00\x00\x01\x00\xf1\x01\x21\xf1\x01\x22\xf1\x01\x23\x01\x08\x02\x03\x03\x04\x01\x01\x01\x01\x07\xf1\x01\x24\xf1\x01\x25\xf1\x01\x26\xf1\x01\x27\xf1\x01\x28\xf1\x01\x29\x04\x01\xa0\x5e\x38\x13\x01\x03";
    let record = crate::om::operation_record::OperationPayload::new(payload, 200, label).unwrap();
    let field = crate::om::surface_envelope::thru_curve_payload_references(record)
        .expect("complete envelope");
    assert_eq!(field.discriminator.get(), 0x13);
    assert_eq!(<[u8; 9]>::from(field.controls), [2, 3, 3, 4, 1, 1, 1, 1, 7]);
    assert_eq!(
        field
            .references()
            .iter()
            .map(|(token, _)| token.value())
            .collect::<Vec<_>>(),
        [289, 290, 291, 292, 293, 294, 295, 296, 297]
    );
    assert_eq!(field.references()[0].1, 205);
    assert_eq!(field.references()[8].0.raw().to_vec(), [0xf1, 0x01, 0x29]);
    assert_eq!(field.trailing_control.get(), 1);
    assert_eq!(field.trailing_value, [0x5e, 0x38]);

    let mut alternate = payload.to_vec();
    alternate[0] = 0x17;
    alternate[16..24].copy_from_slice(&[7, 3, 3, 4, 1, 2, 4, 1]);
    alternate[44] = 6;
    alternate[46..48].copy_from_slice(&[0x5d, 0xfc]);
    let alternate = crate::om::surface_envelope::thru_curve_payload_references(
        crate::om::operation_record::OperationPayload::new(
            &alternate,
            record.payload_offset(),
            record.name(),
        )
        .unwrap(),
    )
    .expect("alternate controls");
    assert_eq!(alternate.discriminator.get(), 0x17);
    assert_eq!(
        <[u8; 9]>::from(alternate.controls),
        [7, 3, 3, 4, 1, 2, 4, 1, 7]
    );
    assert_eq!(alternate.trailing_control.get(), 6);
    assert_eq!(alternate.trailing_value, [0x5d, 0xfc]);

    let mut malformed = payload.to_vec();
    malformed[24] = 0x09;
    assert!(crate::om::surface_envelope::thru_curve_payload_references(
        crate::om::operation_record::OperationPayload::new(
            &malformed,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());

    assert!(crate::om::surface_envelope::thru_curve_payload_references(
        crate::om::operation_record::OperationPayload::new(
            &payload[..payload.len() - 2],
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());

    let mut branched = payload[..payload.len() - 1].to_vec();
    branched.push(3);
    let append_standard_branch = |bytes: &mut Vec<u8>, mode, first, terminal| {
        bytes.extend([mode, 1, 2, 0xf0, first, 1, 2]);
        bytes.extend([0; 5]);
        bytes.extend([0xff, 1, 2, 0xf0, terminal, 0, 0x81, 0x58]);
    };
    append_standard_branch(&mut branched, 0x15, 0x31, 0x32);
    append_standard_branch(&mut branched, 0x15, 0x33, 0x34);
    branched.extend([0, 0, 0, 0, 0, 0, 0xff, 0, 0xff, 1]);
    branched.extend([0xaa, 0xbb]);
    let group = crate::om::thru_curve_branches::thru_curve_payload_branch_group(
        crate::om::operation_record::OperationPayload::new(
            &branched,
            record.payload_offset(),
            record.name(),
        )
        .unwrap(),
    )
    .expect("complete branch group");
    assert_eq!(group.branches().declared_count(), 3);
    assert_eq!(group.branches().len(), 2);
    assert_eq!(group.branches().as_slice()[0].mode.get(), 0x15);
    assert_eq!(group.branches().as_slice()[0].members.declared_count(), 2);
    assert_eq!(group.branches().as_slice()[0].members.state_lane(), [0; 5]);
    assert_eq!(
        group.branches().as_slice()[0].members.as_slice()[0]
            .0
            .value(),
        0x31
    );
    assert_eq!(group.branches().as_slice()[0].terminal.0.value(), 0x32);
    assert_eq!(
        <[u8; 2]>::from(group.branches().as_slice()[0].suffix),
        [0x81, 0x58]
    );
    assert_eq!(
        group.terminator().bytes(),
        &[0, 0, 0, 0, 0, 0, 0xff, 0, 0xff, 1]
    );

    let mut extended = payload[..payload.len() - 1].to_vec();
    extended.extend([2, 0x2f, 1, 5]);
    for object_index in 0x41..0x45 {
        extended.extend([0xf0, object_index]);
    }
    extended.extend([1, 5, 0, 0, 0, 0, 1, 5, 2, 3, 3, 2, 1, 5, 0, 1, 1, 1, 0, 0]);
    extended.extend([0xff, 1, 2, 0xf0, 0x45, 0, 0x81, 0x48]);
    extended.extend([0, 0, 0, 0, 0, 0, 0xff, 0xff, 1]);
    let group = crate::om::thru_curve_branches::thru_curve_payload_branch_group(
        crate::om::operation_record::OperationPayload::new(
            &extended,
            record.payload_offset(),
            record.name(),
        )
        .unwrap(),
    )
    .expect("extended branch state");
    assert_eq!(
        group.branches().as_slice()[0].members.state_lane().len(),
        18
    );
    assert_eq!(group.branches().as_slice()[0].members.len(), 4);
    assert_eq!(
        group.terminator().bytes(),
        &[0, 0, 0, 0, 0, 0, 0xff, 0xff, 1]
    );
}

#[test]
fn om_surface_feature_branches_require_one_complete_counted_group() {
    let label = "SKIN";
    let payload = b"\xa0\x5a\x14\x13\x01\x02\x40\x01\x04\xf1\x1b\xf4\xf1\x1b\xf5\xf1\x1b\xf6\x01\x04\x00\x00\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x1b\xf7\x00\x81\x58\x01\x02\x40\x01\x05\xf1\x1b\xf8\xf1\x1b\xf9\xf1\x1b\xfa\xf1\x1b\xfb\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x1b\xfc\x00\x81\x1c\x00\x00\x00\x01\x03\x00\x00\x00\xff\xff\x01";
    let record = crate::om::operation_record::OperationPayload::new(payload, 200, label).unwrap();
    let group = crate::om::surface_branches::surface_feature_payload_branches(record)
        .expect("complete group");
    assert_eq!(u8::from(group.family), 0x14);
    assert_eq!(group.header_code, 0x13);
    let branches = group.into_branches();
    assert_eq!(branches.len(), 2);
    assert_eq!(u8::from(branches.as_slice()[0].mode()), 0x40);
    assert_eq!(branches.as_slice()[0].members().declared_count(), 4);
    assert!(branches.as_slice()[0].witnessed());
    assert_eq!(branches.as_slice()[0].members().len(), 3);
    assert_eq!(branches.as_slice()[0].terminal().0.value(), 7159);
    assert_eq!(
        branches.as_slice()[0].suffix().clone().into_vec(),
        [0x81, 0x58, 0x01, 0x02]
    );
    assert_eq!(branches.as_slice()[1].members().declared_count(), 5);
    assert!(!branches.as_slice()[1].witnessed());
    assert_eq!(branches.as_slice()[1].members().len(), 4);
    assert_eq!(branches.as_slice()[1].terminal().0.value(), 7164);
    assert_eq!(
        branches.as_slice()[1].suffix().clone().into_vec(),
        [0x81, 0x1c]
    );

    let studio_payload = [
        &payload[..payload.len() - 11],
        &[0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x01],
    ]
    .concat();
    let studio = crate::om::operation_record::OperationPayload::new(
        &studio_payload,
        record.payload_offset(),
        "Studio Surface",
    )
    .unwrap();
    assert!(crate::om::surface_branches::surface_feature_payload_branches(studio).is_some());

    let mut malformed = payload.to_vec();
    malformed[19] = 0x03;
    assert!(
        crate::om::surface_branches::surface_feature_payload_branches(
            crate::om::operation_record::OperationPayload::new(
                &malformed,
                record.payload_offset(),
                record.name()
            )
            .unwrap()
        )
        .is_none()
    );

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(
        crate::om::surface_branches::surface_feature_payload_branches(
            crate::om::operation_record::OperationPayload::new(
                &ambiguous,
                record.payload_offset(),
                record.name()
            )
            .unwrap()
        )
        .is_none()
    );
}

#[test]
fn om_sketch_payload_reference_field_is_counted_ordered_and_canonical() {
    let label = "SKETCH";
    let payload = b"\x01\x00\x01\x05\xf0\xff\xf1\x01\x00\xf1\x01\x01\xf1\x01\x02\x00\x00\xf1\x01\x03\x01\x00\x00\x00";
    let record = crate::om::operation_record::OperationPayload::new(payload, 200, label).unwrap();
    let field = super::sketch_payload_references(record).unwrap();
    assert_eq!(field.declared_count(), 5);
    let references: [super::PayloadObjectReference; 5] =
        field.references().to_vec().try_into().unwrap();
    assert_eq!(
        references.clone().map(|reference| reference.token.value()),
        [255, 256, 257, 258, 259]
    );
    assert_eq!(
        references.map(|reference| reference.offset),
        [204, 206, 209, 212, 217]
    );
    assert_eq!(
        field
            .references()
            .iter()
            .map(|reference| reference.token.raw())
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
    let field = super::sketch_payload_references(
        crate::om::operation_record::OperationPayload::new(
            zero,
            record.payload_offset(),
            record.name(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(field.declared_count(), 0);
    assert_eq!(field.references().len(), 1);
    assert_eq!(field.references()[0].token.value(), 0x42);
    let two = b"\x01\x00\x01\x02\xf0\x41\x00\x00\xf0\x42\x01\x00\x00\x00";
    let field = super::sketch_payload_references(
        crate::om::operation_record::OperationPayload::new(
            two,
            record.payload_offset(),
            record.name(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(field.declared_count(), 2);
    assert_eq!(
        field
            .references()
            .iter()
            .map(|reference| reference.token.value())
            .collect::<Vec<_>>(),
        [0x41, 0x42]
    );

    let mut noncanonical = payload.to_vec();
    noncanonical[7] = 0;
    assert!(super::sketch_payload_references(
        crate::om::operation_record::OperationPayload::new(
            &noncanonical,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
    assert!(super::sketch_payload_references(
        crate::om::operation_record::OperationPayload::new(
            record.payload(),
            record.payload_offset(),
            "BLOCK"
        )
        .unwrap()
    )
    .is_none());
}

#[test]
fn om_extrude_profile_references_require_matching_witness_field() {
    let label = "EXTRUDE";
    let payload = b"\x01\x02\x16\x01\x03\xf0\xff\xf1\x01\x00\x01\x03\x79\xaa\x01\x03\xf0\xff\xf1\x01\x00\x00\x00";
    let record = crate::om::operation_record::OperationPayload::new(payload, 200, label).unwrap();
    let field = crate::om::extrude_profile::extrude_profile_references(record).unwrap();
    assert_eq!(field.field_tag(), 0x16);
    let references: Vec<_> = field.references().collect();
    assert_eq!(references[0].2.unwrap(), 216);
    assert_eq!(references[1].2.unwrap(), 218);
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].0.value(), 255);
    assert_eq!(references[0].0.raw().to_vec(), [0xf0, 0xff]);
    assert_eq!(references[0].1, 205);
    assert_eq!(references[1].0.value(), 256);
    assert_eq!(references[1].0.raw().to_vec(), [0xf1, 0x01, 0x00]);
    assert_eq!(references[1].1, 207);

    let without_witness = &payload[..14];
    let field = crate::om::extrude_profile::extrude_profile_references(
        crate::om::operation_record::OperationPayload::new(
            without_witness,
            record.payload_offset(),
            record.name(),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(field.references().all(|row| row.2.is_none()));
    assert_eq!(field.references().count(), 2);
    let mut alternate_tag = payload.to_vec();
    alternate_tag[2] = 0x5d;
    let field = crate::om::extrude_profile::extrude_profile_references(
        crate::om::operation_record::OperationPayload::new(
            &alternate_tag,
            record.payload_offset(),
            record.name(),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(field.field_tag(), 0x5d);
    let mut ambiguous = payload.to_vec();
    ambiguous.extend_from_slice(&alternate_tag);
    assert!(crate::om::extrude_profile::extrude_profile_references(
        crate::om::operation_record::OperationPayload::new(
            &ambiguous,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
    assert!(crate::om::extrude_profile::extrude_profile_references(
        crate::om::operation_record::OperationPayload::new(
            record.payload(),
            record.payload_offset(),
            "SKETCH"
        )
        .unwrap()
    )
    .is_none());
}

#[test]
fn om_extrude_header_decodes_shifted_ieee_scalars() {
    let label = "EXTRUDE";
    let payload =
        b"\x0f\x00\x00\x01\x00\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x2f\xa3\x74\xbc\x6a\x7e\xf9\xdb";
    let record = crate::om::operation_record::OperationPayload::new(payload, 200, label).unwrap();
    let header = super::extrude_payload_header(record).unwrap();
    assert_eq!(header.offset, 205);
    assert_eq!(
        header
            .scalars
            .map(crate::om::scalar::ShiftedBinary64::value),
        [0.04, 0.038]
    );
    assert_eq!(
        header
            .scalars
            .map(crate::om::scalar::ShiftedBinary64::raw)
            .concat(),
        payload[5..21]
    );

    let mut invalid = payload.to_vec();
    invalid[5] = 0xf0;
    assert!(super::extrude_payload_header(
        crate::om::operation_record::OperationPayload::new(
            &invalid,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
}

#[test]
fn om_swp104_leading_branch_preserves_counts_state_and_references() {
    let label = "SWP104";
    let raw_scalar = [0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b];
    let mut payload = vec![0x21, 0, 0, 1, 0];
    for _ in 0..4 {
        payload.extend(raw_scalar);
    }
    payload.extend([0x23, 1, 3, 0xf0, 0x31, 0xf0, 0x32, 1, 4]);
    payload.extend([0, 1, 1, 0, 0, 0, 0]);
    payload.extend([0xff, 1, 2, 0xf0, 0x33, 0, 0xaa]);
    let record = crate::om::operation_record::OperationPayload::new(&payload, 200, label).unwrap();
    let branch = super::swp104_payload_leading_branch(record).expect("leading branch");
    assert_eq!(branch.discriminator.get(), 0x21);
    assert_eq!(
        branch
            .scalars
            .map(crate::om::scalar::ShiftedBinary64::value),
        [0.04; 4]
    );
    assert_eq!(
        branch.scalars.map(crate::om::scalar::ShiftedBinary64::raw),
        [raw_scalar; 4]
    );
    assert!(!branch.leading_zero);
    assert_eq!(branch.mode.get(), 0x23);
    assert_eq!(branch.members.declared_count(), 3);
    assert_eq!(branch.state_lane.witnessed_count(), Some(4));
    assert_eq!(branch.state_lane.bytes(), [0, 1, 1, 0, 0, 0, 0]);
    assert_eq!(
        branch
            .members
            .as_slice()
            .iter()
            .map(|reference| reference.value())
            .collect::<Vec<_>>(),
        [0x31, 0x32]
    );
    assert_eq!(branch.terminal.value(), 0x33);
    assert_eq!(record.payload_offset() + branch.byte_len(), 259);

    let mut malformed_witness = payload.clone();
    malformed_witness[45] = 1;
    assert!(super::swp104_payload_leading_branch(
        crate::om::operation_record::OperationPayload::new(
            &malformed_witness,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());

    let mut unwitnessed = vec![0x21, 0, 0, 1, 0];
    for _ in 0..4 {
        unwitnessed.extend(raw_scalar);
    }
    unwitnessed.extend([0, 0x23, 1, 2, 0xf0, 0x41]);
    unwitnessed.extend([0; 5]);
    unwitnessed.extend([0xff, 1, 2, 0xf0, 0x42, 0]);
    let branch = super::swp104_payload_leading_branch(
        crate::om::operation_record::OperationPayload::new(
            &unwitnessed,
            record.payload_offset(),
            record.name(),
        )
        .unwrap(),
    )
    .expect("unwitnessed leading branch");
    assert!(branch.leading_zero);
    assert_eq!(branch.state_lane.witnessed_count(), None);
    assert_eq!(branch.state_lane.bytes(), [0; 5]);

    unwitnessed[43] = 1;
    assert!(super::swp104_payload_leading_branch(
        crate::om::operation_record::OperationPayload::new(
            &unwitnessed,
            record.payload_offset(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
}

#[test]
fn om_operation_terminal_discriminator_requires_one_complete_lane() {
    let label = "EXTRUDE";
    let payload = b"\x01\x01\x02\x81\x5f\x80\xab\x01\x03\x02\x01\x01\x02\x01\x01\x00\x00\x00\x29\x29\x05\x80\xff\x00";
    let record = crate::om::operation_record::OperationPayload::new(payload, 200, label).unwrap();
    let lane = crate::om::terminal_discriminator::operation_terminal_discriminator(record).unwrap();
    assert_eq!(lane.origin(), 200);
    assert_eq!(
        lane.type_indices().map(|(token, _)| token.value()),
        [351, 171]
    );
    assert_eq!(
        lane.type_indices().map(|(token, _)| token.raw().to_vec()),
        [vec![0x81, 0x5f], vec![0x80, 0xab]]
    );
    assert_eq!(lane.type_indices().map(|(_, offset)| offset), [203, 205]);
    assert_eq!(lane.flags(), [1, 2, 1, 1]);
    assert_eq!(
        lane.trailing_indices()
            .map(|(token, _)| token.value())
            .collect::<Vec<_>>(),
        [5, 255]
    );
    assert_eq!(
        lane.trailing_indices()
            .map(|(token, _)| token.raw().to_vec())
            .collect::<Vec<_>>(),
        [vec![0x05], vec![0x80, 0xff]]
    );
    assert_eq!(
        lane.trailing_indices()
            .map(|(_, offset)| offset)
            .collect::<Vec<_>>(),
        [220, 221]
    );

    let subtract = crate::om::operation_record::OperationPayload::new(
        record.payload(),
        record.payload_offset(),
        "SUBTRACT",
    )
    .unwrap();
    assert_eq!(
        crate::om::terminal_discriminator::operation_terminal_discriminator(subtract),
        Some(lane.clone())
    );

    let truncated = &payload[..payload.len() - 1];
    assert!(
        crate::om::terminal_discriminator::operation_terminal_discriminator(
            crate::om::operation_record::OperationPayload::new(
                truncated,
                record.payload_offset(),
                record.name()
            )
            .unwrap()
        )
        .is_none()
    );

    let mut ambiguous = payload[..payload.len() - 1].to_vec();
    ambiguous.extend_from_slice(payload);
    assert!(
        crate::om::terminal_discriminator::operation_terminal_discriminator(
            crate::om::operation_record::OperationPayload::new(
                &ambiguous,
                record.payload_offset(),
                record.name()
            )
            .unwrap()
        )
        .is_none()
    );
}

#[test]
fn om_operation_body_scalar_clauses_preserve_body_order_and_branch() {
    let label = "TRIM BODY";
    let bytes = b"\x01\x02\x10\x42\xff\x1c\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\xaa\x01\x02\x10\x43\xff\x11\x30\x00\x00\x00\x00\x00\x00\x00\x00\x00";
    let record =
        crate::om::operation_record::OperationBodyInput::new(bytes, 100, 0, label).unwrap();
    let triples = crate::om::body_scalar_triple::operation_body_scalar_triples(record);
    assert_eq!(triples.len(), 2);
    assert_eq!(triples[0].body_reference_ordinal, 0);
    assert_eq!(triples[0].body_object_index, 66);
    assert_eq!(triples[0].branch, 0x1c);
    assert_eq!(
        triples[0]
            .scalars
            .atoms()
            .each_ref()
            .map(|scalar| scalar.value()),
        [0.0, 3.0, -170.0]
    );
    assert_eq!(
        triples[0]
            .scalars
            .atoms()
            .each_ref()
            .map(|scalar| scalar.encoding()),
        [
            crate::om::scalar::PayloadScalarEncoding::Zero,
            crate::om::scalar::PayloadScalarEncoding::Binary32,
            crate::om::scalar::PayloadScalarEncoding::Binary64,
        ]
    );
    assert_eq!(triples[0].scalars.source_offsets(), [106, 107, 111]);
    assert_eq!(
        triples[0]
            .scalars
            .atoms()
            .each_ref()
            .map(crate::om::scalar::PayloadScalarAtom::raw),
        [&bytes[6..7], &bytes[7..11], &bytes[11..19]]
    );
    assert_eq!(triples[1].body_reference_ordinal, 1);
    assert_eq!(triples[1].body_object_index, 67);
    assert_eq!(triples[1].branch, 0x11);
    assert_eq!(
        triples[1]
            .scalars
            .atoms()
            .each_ref()
            .map(|scalar| scalar.value()),
        [2.0, 0.0, 0.0]
    );
    let truncated = &bytes[..bytes.len() - 1];
    let truncated_triples = crate::om::body_scalar_triple::operation_body_scalar_triples(
        crate::om::operation_record::OperationBodyInput::new(
            truncated,
            record.offset(),
            record.payload_start(),
            record.name(),
        )
        .unwrap(),
    );
    assert_eq!(truncated_triples.len(), 1);
    assert_eq!(truncated_triples[0], triples[0]);
}

#[test]
fn om_operation_body_branch_11_decodes_wrapped_member_lane_atomically() {
    let label = "SEW";
    let bytes = b"\x01\x02\x10\x42\xff\x11\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\x01\x03\x2e\x7f\x00\x2e\x80\x01\x00";
    let record =
        crate::om::operation_record::OperationBodyInput::new(bytes, 100, 0, label).unwrap();
    let members = super::operation_body_members(record);
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].members.len(), 2);
    assert_eq!(members[0].body_reference_ordinal, 0);
    assert_eq!(members[0].body_object_index, 66);
    assert_eq!(members[0].members[0].atom.value(), 127);
    assert_eq!(members[0].members[0].atom.raw(), [0x7f]);
    assert_eq!(members[0].members[0].offset, 122);
    assert_eq!(members[0].members[1].atom.value(), 1);
    assert_eq!(members[0].members[1].atom.raw(), [0x80, 0x01]);

    let truncated = &bytes[..bytes.len() - 1];
    assert!(super::operation_body_members(
        crate::om::operation_record::OperationBodyInput::new(
            truncated,
            record.offset(),
            record.payload_start(),
            record.name()
        )
        .unwrap()
    )
    .is_empty());
}

#[test]
fn om_trim_body_branch_11_decodes_terminal_continuation_atomically() {
    let label = "TRIM BODY";
    let bytes = b"\x01\x02\x10\x72\xff\x11\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\x01\x02\x2e\x41\x00\x01\x02\x80\x43\x00\x00\x01\x72\x00\x00";
    let record =
        crate::om::operation_record::OperationBodyInput::new(bytes, 100, 0, label).unwrap();
    let continuations = super::operation_body_11_continuations(record);
    assert_eq!(continuations.len(), 1);
    let continuation = &continuations[0];
    assert_eq!(continuation.body_reference_ordinal, 0);
    assert_eq!(continuation.body_object_index, 114);
    assert_eq!(continuation.continuation.atom.value(), 67);
    assert_eq!(continuation.continuation.atom.raw(), [0x80, 0x43]);
    assert_eq!(continuation.continuation.offset, 126);
    assert_eq!(continuation.terminal.token.value(), 114);
    assert_eq!(continuation.terminal.token.raw(), [0x72]);
    assert_eq!(continuation.terminal.offset, 131);

    let mut distinct_terminal = bytes.to_vec();
    distinct_terminal[31] = 0x71;
    assert_eq!(
        super::operation_body_11_continuations(
            crate::om::operation_record::OperationBodyInput::new(
                &distinct_terminal,
                record.offset(),
                record.payload_start(),
                record.name()
            )
            .unwrap()
        )[0]
        .terminal
        .token
        .value(),
        113
    );

    let truncated = &bytes[..bytes.len() - 1];
    assert!(super::operation_body_11_continuations(
        crate::om::operation_record::OperationBodyInput::new(
            truncated,
            record.offset(),
            record.payload_start(),
            record.name()
        )
        .unwrap()
    )
    .is_empty());
}

#[test]
fn om_operation_body_decodes_homogeneous_unwrapped_reference_lanes() {
    let label = "OFFSET";
    let compact = b"\x01\x02\x10\x6e\xff\x1c\x00\x00\x00\x01\x03\x80\x0d\x69\x00\x00\x0b\x00";
    let record =
        crate::om::operation_record::OperationBodyInput::new(compact, 100, 0, label).unwrap();
    let lanes = super::operation_body_reference_lanes(record);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].body_object_index, 110);
    let super::OperationBodyReferenceLaneValues::CompactIndex(values) = &lanes[0].values else {
        panic!("expected CompactIndex lane")
    };
    assert_eq!(
        values
            .iter()
            .map(|value| (value.atom.value(), value.offset))
            .collect::<Vec<_>>(),
        [(13, 111), (105, 113)]
    );
    assert_eq!(
        values
            .iter()
            .map(|value| value.atom.raw())
            .collect::<Vec<_>>(),
        [b"\x80\x0d".as_slice(), b"\x69".as_slice()]
    );

    let objects =
        b"\x01\x02\x10\x70\xff\x1c\x00\x00\x00\x01\x03\xf1\x02\x9e\xf0\x44\x00\x00\x0b\x00";
    let object_record = crate::om::operation_record::OperationBodyInput::new(
        objects,
        record.offset(),
        record.payload_start(),
        record.name(),
    )
    .unwrap();
    let lanes = super::operation_body_reference_lanes(object_record);
    let super::OperationBodyReferenceLaneValues::PayloadObjectIndex(values) = &lanes[0].values
    else {
        panic!("expected PayloadObjectIndex lane")
    };
    assert_eq!(
        values
            .iter()
            .map(|value| value.token.value())
            .collect::<Vec<_>>(),
        [670, 68]
    );
    assert_eq!(
        values
            .iter()
            .map(|value| value.token.raw())
            .collect::<Vec<_>>(),
        [b"\xf1\x02\x9e".as_slice(), b"\xf0\x44".as_slice()]
    );

    let truncated = &objects[..objects.len() - 1];
    assert!(super::operation_body_reference_lanes(
        crate::om::operation_record::OperationBodyInput::new(
            truncated,
            object_record.offset(),
            object_record.payload_start(),
            object_record.name()
        )
        .unwrap()
    )
    .is_empty());

    let branch_11 =
        b"\x01\x02\x10\x70\xff\x11\x00\x00\x00\x01\x03\xf1\x02\x9e\xf0\x44\x00\x00\x0b\x00";
    let lanes = super::operation_body_reference_lanes(
        crate::om::operation_record::OperationBodyInput::new(
            branch_11,
            record.offset(),
            record.payload_start(),
            record.name(),
        )
        .unwrap(),
    );
    assert_eq!(lanes.len(), 1);
    assert_eq!(
        lanes[0].branch,
        crate::om::discriminators::OperationBodyReferenceBranch::Form11
    );
    let super::OperationBodyReferenceLaneValues::PayloadObjectIndex(values) = &lanes[0].values
    else {
        panic!("expected payload lane")
    };
    assert_eq!(
        values
            .iter()
            .map(|value| value.token.value())
            .collect::<Vec<_>>(),
        [670, 68]
    );
}

#[test]
fn om_extrude_body_32_branch_decodes_counted_lanes() {
    let label = "EXTRUDE";
    let bytes = b"\x01\x02\x10\x73\xff\x32\x00\x00\x30\x77\x7e\x14\x7a\xe1\x47\xb3\x01\x03\x3d\x82\x56\x00\x3d\x82\x57\x00\x01\x04\x80\x2b\x80\x2d\x80\x2c\x01\x03\x80\x2e\x80\x77\x00\x01\x73\x00\x00";
    let record =
        crate::om::operation_record::OperationBodyInput::new(bytes, 100, 0, label).unwrap();
    let branch = crate::om::extrude_32::extrude_payload_32_branch(record).unwrap();
    assert_eq!(branch.origin(), 105);
    assert_eq!(branch.terminal().value(), 115);
    assert!(branch.scalar().value().is_finite());
    assert_eq!(branch.scalar().raw(), bytes[8..16]);
    assert_eq!(
        branch
            .atoms()
            .map(|(token, (), _)| token.raw())
            .collect::<Vec<_>>(),
        [0x3d82_5600, 0x3d82_5700]
    );
    assert_eq!(
        branch
            .atoms()
            .map(|(_, (), offset)| offset)
            .collect::<Vec<_>>(),
        [118, 122]
    );
    assert_eq!(
        branch
            .atoms()
            .map(|(token, (), _)| token.value())
            .collect::<Vec<_>>(),
        [598, 599]
    );
    assert_eq!(
        branch
            .first_indices()
            .map(|(token, (), _)| token.value())
            .collect::<Vec<_>>(),
        [43, 45, 44]
    );
    assert_eq!(
        branch
            .first_indices()
            .map(|(token, (), _)| token.raw().to_vec())
            .collect::<Vec<_>>(),
        [vec![0x80, 0x2b], vec![0x80, 0x2d], vec![0x80, 0x2c]]
    );
    assert_eq!(
        branch
            .first_indices()
            .map(|(_, (), offset)| offset)
            .collect::<Vec<_>>(),
        [128, 130, 132]
    );
    assert_eq!(
        branch
            .second_indices()
            .map(|(token, (), _)| token.value())
            .collect::<Vec<_>>(),
        [46, 119]
    );
    assert_eq!(
        branch
            .second_indices()
            .map(|(token, (), _)| token.raw().to_vec())
            .collect::<Vec<_>>(),
        [vec![0x80, 0x2e], vec![0x80, 0x77]]
    );
    assert_eq!(
        branch
            .second_indices()
            .map(|(_, (), offset)| offset)
            .collect::<Vec<_>>(),
        [136, 138]
    );
    assert_eq!(branch.terminal().value(), 115);
    assert_eq!(branch.terminal().raw(), [0x73]);
    assert_eq!(branch.terminal_offset(), 142);

    let mut invalid = bytes.to_vec();
    invalid[36] = 0xff;
    assert!(crate::om::extrude_32::extrude_payload_32_branch(
        crate::om::operation_record::OperationBodyInput::new(
            &invalid,
            record.offset(),
            record.payload_start(),
            record.name()
        )
        .unwrap()
    )
    .is_none());

    let mut invalid_atom = bytes.to_vec();
    invalid_atom[18] = 0x3c;
    assert!(crate::om::extrude_32::extrude_payload_32_branch(
        crate::om::operation_record::OperationBodyInput::new(
            &invalid_atom,
            record.offset(),
            record.payload_start(),
            record.name()
        )
        .unwrap()
    )
    .is_none());

    let mut wrong_terminal_body = bytes.to_vec();
    wrong_terminal_body[43] = 0x72;
    assert!(crate::om::extrude_32::extrude_payload_32_branch(
        crate::om::operation_record::OperationBodyInput::new(
            &wrong_terminal_body,
            record.offset(),
            record.payload_start(),
            record.name()
        )
        .unwrap()
    )
    .is_none());
}

#[test]
fn om_block_construction_field_decodes_ordered_canonical_references() {
    let label = "BLOCK";
    let mut payload = vec![0x26, 0, 0, 1, 0, 0];
    for value in 1..=18u8 {
        payload.extend([0xf0, value]);
    }
    payload.extend([0x01, 0xf1, 0x01, 0x00]);
    payload.extend([0xff; 11]);
    payload.extend([0; 4]);
    let record = crate::om::operation_record::OperationPayload::new(&payload, 200, label).unwrap();
    let field = crate::om::block_construction::block_construction_references(record).unwrap();
    assert_eq!(field.control(), 0x26);
    assert_eq!(field.references().len(), 19);
    assert_eq!(field.references()[0].0.value(), 1);
    assert_eq!(field.references()[0].0.raw().to_vec(), [0xf0, 0x01]);
    assert_eq!(field.references()[18].0.value(), 256);
    assert_eq!(field.references()[18].0.raw().to_vec(), [0xf1, 0x01, 0x00]);
    assert_eq!(field.references()[0].1, 206);

    let mut invalid = payload.clone();
    invalid[42] = 0xf0;
    assert!(
        crate::om::block_construction::block_construction_references(
            crate::om::operation_record::OperationPayload::new(
                &invalid,
                record.payload_offset(),
                record.name()
            )
            .unwrap()
        )
        .is_none()
    );
}

#[test]
fn om_boolean_operations_decode_counted_target_and_tools() {
    let bytes = b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x0aSUBTRACT\0\x31\x00\x00\x01\x00\x14\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\x00\x00\xe0\x7f\xff\xff\xff\x01\x01\x01\x02\x90\x19\x5e\x00\x01\x05\x90\x19\x5f\x90\x19\x44\x90\x19\x43\x90\x19\x60\x00";
    let operations =
        super::boolean_operations_with_labels(bytes, 100, &super::operation_labels(bytes, 100));
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0].kind, super::BooleanOperationKind::Subtract);
    assert_eq!(operations[0].target.token.value(), 6494);
    assert_eq!(
        operations[0].target.token.raw().to_vec(),
        [0x90, 0x19, 0x5e]
    );
    assert_eq!(
        operations[0].target.offset,
        100 + bytes
            .windows(3)
            .position(|window| window == [0x90, 0x19, 0x5e])
            .unwrap()
    );
    assert_eq!(
        operations[0]
            .tools
            .iter()
            .map(|token| token.token.value())
            .collect::<Vec<_>>(),
        [6495, 6468, 6467, 6496]
    );
    assert_eq!(
        operations[0]
            .tools
            .iter()
            .map(|token| token.token.raw().to_vec())
            .collect::<Vec<_>>(),
        [
            vec![0x90, 0x19, 0x5f],
            vec![0x90, 0x19, 0x44],
            vec![0x90, 0x19, 0x43],
            vec![0x90, 0x19, 0x60],
        ]
    );
    assert_eq!(
        operations[0]
            .tools
            .iter()
            .map(|token| token.offset)
            .collect::<Vec<_>>(),
        [0x5f, 0x44, 0x43, 0x60].map(|low| {
            100 + bytes
                .windows(3)
                .position(|window| window == [0x90, 0x19, low])
                .unwrap()
        })
    );

    let mut invalid = bytes.to_vec();
    *invalid.last_mut().unwrap() = 1;
    assert!(super::boolean_operations_with_labels(
        &invalid,
        0,
        &super::operation_labels(&invalid, 0)
    )
    .is_empty());
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
    assert!(sections[0].as_fixed().expect("fixed store")[0]
        .bytes
        .starts_with(b"\x04\x01\x0fNX 2027.3102 \0"));
}

#[test]
fn om_index_discards_nested_indexed_interpretation() {
    let bytes = fixed_indexed_section_with_embedded_section(true);
    let sections = super::indexed_sections(&bytes);

    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].as_fixed().expect("fixed store").len(), 2);
}

#[test]
fn om_index_retains_disjoint_indexed_sections() {
    let mut bytes = indexed_om_section();
    bytes.extend_from_slice(&indexed_om_section());

    let sections = super::indexed_sections(&bytes);

    assert_eq!(sections.len(), 2);
    assert!(sections[0].entity_index_offset < sections[1].entity_index_offset);
}

#[test]
fn om_index_retains_partially_overlapping_indexed_interpretations() {
    let bytes = fixed_indexed_section_with_embedded_section(false);
    let sections = super::indexed_sections(&bytes);

    assert_eq!(sections.len(), 2);
    assert_ne!(
        sections[0].entity_index_offset,
        sections[1].entity_index_offset
    );
}

#[test]
fn om_store_version_can_follow_control_prefix() {
    let bytes = b"\xff\x00prefix\x04\x01\x0eNX 2027.3102\0tail";
    let version = super::store_version(bytes, 100).expect("store version");
    assert_eq!(version.offset, 108);
    assert_eq!(version.value.as_str(), "NX 2027.3102");
}

#[test]
fn om_offset_only_index_bounds_storage_blocks() {
    let bytes = offset_only_indexed_om_section();
    let sections = super::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].base, 0);
    let (control, column_storage, records) =
        sections[0].as_offset_only().expect("offset-only store");
    assert_eq!(control.bytes, &[0, 0, 0, 0, 0, 1, 0, 0]);
    assert_eq!(records.len(), 2);
    assert_eq!(
        column_storage,
        [records[0].bytes, records[1].bytes].concat()
    );
    assert!(records[0].bytes.starts_with(b"\x04\x01\x0eNX "));
    assert!(records[1].bytes.ends_with(b"\0"));
    let expressions = sections[0].numeric_expressions();
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].name.as_str(), "length");
    assert_eq!(expressions[0].constant_value(), Some(25.0));
}

#[test]
fn om_indexed_layout_materializes_both_store_forms_without_semantic_drift() {
    for bytes in [indexed_om_section(), offset_only_indexed_om_section()] {
        let section = super::indexed_sections(&bytes)
            .into_iter()
            .next()
            .expect("indexed fixture has one section");
        let source = std::sync::Arc::<[u8]>::from(bytes.as_slice());
        let layout =
            crate::om::cache::IndexedSectionLayout::from_section(&section, &source).unwrap();
        assert_eq!(layout.materialize(), section);
    }
}

#[test]
fn om_offset_only_index_accepts_one_root_record_inside_control_block() {
    let bytes = control_root_offset_only_indexed_om_section();
    let sections = super::indexed_sections(&bytes);

    assert_eq!(sections.len(), 1);
    let (control, _, records) = sections[0].as_offset_only().expect("offset-only store");
    assert!(control
        .bytes
        .windows(b"NX 2027.3102".len())
        .any(|window| window == b"NX 2027.3102"));
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].bytes, &[0; 32]);
    assert_eq!(sections[0].numeric_expressions()[0].name.as_str(), "length");
}

#[test]
fn om_offset_only_index_ignores_product_marker_crossing_record_boundary() {
    use cadmpeg_core::decode::View;

    let mut bytes = control_root_offset_only_indexed_om_section();
    let class_name = b"UGS::ModlFeature";
    let class_start = bytes
        .windows(class_name.len())
        .position(|window| window == class_name)
        .expect("class declaration");
    let index_start = class_start + class_name.len() + 1;
    let first = usize::try_from(View::u32_le_at(&bytes, index_start + 4).unwrap()).unwrap();
    let product = b"\x04\x01\x0eNX 2027.3102\0";
    let split = 3;
    bytes[first - split..first].copy_from_slice(&product[..split]);
    bytes[first..first + product.len() - split].copy_from_slice(&product[split..]);

    assert_eq!(super::indexed_sections(&bytes).len(), 1);
}

#[test]
fn om_product_record_count_respects_containment_boundaries() {
    let ranges = [
        super::ProductRecordRange { start: 10, end: 20 },
        super::ProductRecordRange { start: 30, end: 40 },
        super::ProductRecordRange { start: 50, end: 60 },
    ];

    assert_eq!(super::product_record_count_within(&ranges, 10, 20), 1);
    assert_eq!(super::product_record_count_within(&ranges, 11, 20), 0);
    assert_eq!(super::product_record_count_within(&ranges, 10, 19), 0);
    assert_eq!(super::product_record_count_within(&ranges, 20, 60), 2);
    assert_eq!(super::product_record_count_within(&ranges, 20, 50), 1);
}

#[test]
fn om_index_monotone_cache_rejects_a_decrease_inside_a_candidate() {
    let words = [10_u32, 20, 30, 25, 40];
    let bytes = words
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    let edges = crate::om::index_table::DescendingU32Edges::new(&bytes);

    assert!(edges.is_nondecreasing(0, 12));
    assert!(!edges.is_nondecreasing(0, 16));
    assert!(edges.is_nondecreasing(4, 12));
    assert!(edges.is_nondecreasing(12, 20));
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
        super::offset_store_control_values(&[0, 0x34, 0x12, 0, 0, 0xff, 0xff, 0xff]).map(
            |values| values
                .into_iter()
                .map(crate::om::control_word::ControlWord24::value)
                .collect::<Vec<_>>()
        ),
        Some(vec![0x1234, 0x00ff_ffff])
    );
    assert!(super::offset_store_control_values(&[]).is_none());
    assert!(super::offset_store_control_values(&[0, 1, 2]).is_none());
    assert!(super::offset_store_control_values(&[1, 1, 2, 3]).is_none());
}

#[test]
fn om_offset_store_control_form_requires_one_complete_grammar() {
    assert_eq!(
        super::offset_store_control_form(&[0, 0x34, 0x12, 0, 0, 0xff, 0xff, 0xff], None),
        Some(super::OffsetStoreControlForm::ZeroPrefixed {
            values: crate::om::nonempty::NonEmpty::new(
                [0x1234, 0x00ff_ffff]
                    .map(|value| crate::om::control_word::ControlWord24::try_from(value).unwrap())
            )
            .unwrap(),
        })
    );

    let mut product = vec![0, 0];
    product.extend_from_slice(&7u32.to_le_bytes());
    product.extend_from_slice(&0x1020u32.to_le_bytes());
    product.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0");
    assert_eq!(
        super::offset_store_control_form(&product, None),
        Some(super::OffsetStoreControlForm::ProductAnchored {
            leading_value: Some(
                crate::om::control_leading_value::ControlLeadingValue::from_wire(2, 0).unwrap()
            ),
            values: crate::om::nonempty::NonEmpty::new([7, 0x1020]).unwrap(),
        })
    );

    product.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0");
    assert!(super::offset_store_control_form(&product, None).is_none());
    assert!(super::offset_store_control_form(&[1, 2, 3, 4], None).is_none());
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

mod numeric_expressions;

mod registry;
