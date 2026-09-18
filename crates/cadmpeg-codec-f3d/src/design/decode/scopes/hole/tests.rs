// SPDX-License-Identifier: Apache-2.0
use super::exact_hole_construction;
use super::exact_hole_face_selection;
use super::HOLE_FACE_SELECTION_TYPE_GUID;
use super::HOLE_POINT_DATA_TYPE_GUID;
use crate::design::test_support::dump::{DesignParameterScope, HashMap, IndexedRecordOffsets};
use crate::design::test_support::indexed_header;
use crate::test_support::lp_utf16;

const EPS_HOLE_TEST_VALUE: f64 = 1.0e-12;

fn assert_f64_array<const N: usize>(actual: [f64; N], expected: [f64; N]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual - expected).abs() < EPS_HOLE_TEST_VALUE);
    }
}

fn hole_point_stream() -> (Vec<u8>, DesignParameterScope, usize, usize) {
    hole_point_stream_version(4)
}

fn hole_point_stream_version(version: u32) -> (Vec<u8>, DesignParameterScope, usize, usize) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    let position_at = bytes.len();
    for value in [1.25_f64, -2.5, 3.75] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0.0_f64, 0.0, 1.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0.125_f64, -0.25] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&19u32.to_le_bytes());
    if version == 4 {
        bytes.push(0x7f);
        for value in [-1.0_f64, -1.0, -1.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    let input_reference_at = bytes.len();
    bytes.push(1);
    bytes.extend_from_slice(&378u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    if version == 1 {
        bytes.extend_from_slice(&[0x55; 13]);
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&55u32.to_le_bytes());

    let mut scope = DesignParameterScope::empty(
        "generated:hole#0",
        crate::records::feature::scope::DesignFeatureKind::Hole,
        12,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = {
                let mut values: Vec<u32> = draft.reference_members.values().copied().collect();
                values.push(55);
                crate::records::identity::ReferenceRun::unlocated(values)
            };
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope, position_at, input_reference_at)
}

#[test]
fn hole_construction_reads_the_versioned_point_and_direction_carrier() {
    let (bytes, scope, position_at, input_reference_at) = hole_point_stream();
    let records = IndexedRecordOffsets::build(&bytes);
    let construction = exact_hole_construction(
        &bytes,
        &records,
        &scope,
        &HashMap::from([(55_u64, (HOLE_POINT_DATA_TYPE_GUID, 4))]),
    )
    .expect("hole point carrier");

    assert_eq!(construction.point_record_index, 55);
    assert_eq!(construction.point_record_byte_offset, 0);
    assert_f64_array(construction.position, [1.25, -2.5, 3.75]);
    assert_eq!(construction.position_offset, position_at as u64);
    assert_f64_array(construction.direction, [0.0, 0.0, 1.0]);
    assert_eq!(construction.direction_offset, (position_at + 24) as u64);
    assert_f64_array(construction.point_parameters, [0.125, -0.25]);
    assert_eq!(construction.reference_type, 19);
    assert_eq!(
        construction
            .tangent_point_data
            .as_ref()
            .map(|tangent| tangent.prefix),
        Some(0x7f)
    );
    assert_f64_array(
        construction
            .tangent_point_data
            .as_ref()
            .map(|tangent| tangent.data.value)
            .expect("version-four tangent point data"),
        [-1.0, -1.0, -1.0],
    );
    assert_eq!(
        construction
            .input_records
            .iter()
            .map(|reference| reference.value)
            .collect::<Vec<_>>(),
        [378]
    );
    assert_eq!(
        construction
            .input_records
            .iter()
            .map(|reference| reference.offset)
            .collect::<Vec<_>>(),
        [(input_reference_at + 1) as u64]
    );
}

#[test]
fn hole_construction_reads_the_legacy_point_and_direction_carrier_without_tangent_data() {
    let (bytes, scope, position_at, input_reference_at) = hole_point_stream_version(1);
    let records = IndexedRecordOffsets::build(&bytes);
    let construction = exact_hole_construction(
        &bytes,
        &records,
        &scope,
        &HashMap::from([(55_u64, (HOLE_POINT_DATA_TYPE_GUID, 1))]),
    )
    .expect("legacy hole point carrier");

    assert_eq!(construction.point_record_index, 55);
    assert_f64_array(construction.position, [1.25, -2.5, 3.75]);
    assert_eq!(construction.position_offset, position_at as u64);
    assert_f64_array(construction.direction, [0.0, 0.0, 1.0]);
    assert_f64_array(construction.point_parameters, [0.125, -0.25]);
    assert_eq!(construction.reference_type, 19);
    assert_eq!(
        construction
            .tangent_point_data
            .as_ref()
            .map(|tangent| tangent.prefix),
        None
    );
    assert_eq!(construction.tangent_point_data, None);
    assert_eq!(
        construction
            .tangent_point_data
            .as_ref()
            .map(|tangent| tangent.data.offset),
        None
    );
    assert_eq!(
        construction
            .input_records
            .iter()
            .map(|reference| reference.value)
            .collect::<Vec<_>>(),
        [378]
    );
    assert_eq!(
        construction
            .input_records
            .iter()
            .map(|reference| reference.offset)
            .collect::<Vec<_>>(),
        [(input_reference_at + 1) as u64]
    );
}

#[test]
fn hole_face_selection_reads_the_direct_persistent_identity_envelope() {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"333", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&103u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "53aa8ab4-194a-434b-bd52-8c6d761dc147");
    lp_utf16(&mut bytes, "8e685642-4d68-4909-96d0-0dd4437491b6");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 7]);
    indexed_header(&mut bytes, *b"265", 100);
    indexed_header(&mut bytes, *b"301", 101);
    indexed_header(&mut bytes, *b"446", 102);
    let identity_at = bytes.len();
    indexed_header(&mut bytes, *b"429", 103);
    bytes.extend_from_slice(&[0; 10]);
    let primary_identity_at = bytes.len();
    bytes.extend_from_slice(&246u64.to_le_bytes());
    let next_at = bytes.len();
    indexed_header(&mut bytes, *b"311", 104);

    let mut scope = DesignParameterScope::empty(
        "generated:hole#0",
        crate::records::feature::scope::DesignFeatureKind::Hole,
        12,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = {
                let mut values: Vec<u32> = draft.reference_members.values().copied().collect();
                values.push(100);
                crate::records::identity::ReferenceRun::unlocated(values)
            };
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let selection = exact_hole_face_selection(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &HashMap::from([(100_u64, (HOLE_FACE_SELECTION_TYPE_GUID, 1))]),
    )
    .expect("direct Hole face selection");

    assert_eq!(selection.record_index, 100);
    assert_eq!(selection.class_tag.as_str(), "333");
    assert_eq!(
        selection.asset_id.as_str(),
        "53aa8ab4-194a-434b-bd52-8c6d761dc147"
    );
    assert_eq!(
        selection.context_id.as_str(),
        "8e685642-4d68-4909-96d0-0dd4437491b6"
    );
    assert_eq!(selection.identity_record_index, 103);
    assert_eq!(selection.identity_record_offset, identity_at as u64);
    assert_eq!(selection.primary_identity, 246);
    assert_eq!(
        selection.primary_identity_offset,
        primary_identity_at as u64
    );
    assert_eq!(selection.next_record_index, 104);
    assert_eq!(selection.next_byte_offset, next_at as u64);
    assert!(selection.historical_face_candidates.is_empty());
}
