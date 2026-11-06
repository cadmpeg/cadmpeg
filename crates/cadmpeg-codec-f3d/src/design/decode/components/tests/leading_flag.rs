// SPDX-License-Identifier: Apache-2.0
//! Leading-flag local component-occurrence frames.

use super::super::leading_flag_component_occurrence;
use super::{guid, COMPONENT, OCCURRENCE};
use crate::records::feature::assembly_features::DesignComponentOccurrence;
use crate::test_support::indexed_header;

const STREAM: &str = "f3d:Design/BulkStream.dat";
const MATRIX: [f64; 16] = [
    1.0, 0.0, 0.0, 5.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

fn write_matrix(bytes: &mut [u8], at: usize) {
    for (ordinal, value) in MATRIX.into_iter().enumerate() {
        bytes[at + ordinal * 8..at + ordinal * 8 + 8].copy_from_slice(&value.to_le_bytes());
    }
}

/// A leading-flag frame for component record 202, root reference 11 and owner
/// entity 6, with a matrix when `matrix` is set.
fn frame(matrix: bool) -> Vec<u8> {
    let extra = if matrix { 128 } else { 0 };
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"332", 253);
    bytes.resize(251 + 2 * extra, 0);
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
    bytes[24] = 1;
    bytes[25..33].copy_from_slice(&202_u64.to_le_bytes());
    bytes[35..39].copy_from_slice(&1_u32.to_le_bytes());
    bytes[39] = 1;
    bytes[44] = 1;
    bytes[46..50].copy_from_slice(&1_u32.to_le_bytes());
    bytes[50] = u8::from(!matrix);
    if matrix {
        write_matrix(&mut bytes, 51);
    }
    let body = 51 + extra;
    guid(&mut bytes, body, COMPONENT);
    guid(&mut bytes, body + 76, OCCURRENCE);
    bytes[body + 152] = 1;
    bytes[body + 153..body + 161].copy_from_slice(&11_u64.to_le_bytes());
    bytes[body + 162] = 1;
    bytes[body + 163..body + 167].copy_from_slice(&1_u32.to_le_bytes());
    bytes[body + 168] = 1;
    bytes[body + 169..body + 177].copy_from_slice(&202_u64.to_le_bytes());
    bytes[body + 179] = bytes[50];
    let mut tail = body + 180;
    if matrix {
        write_matrix(&mut bytes, tail);
        tail += 128;
    }
    bytes[tail + 9] = 1;
    bytes[tail + 10..tail + 18].copy_from_slice(&6_u64.to_le_bytes());
    bytes
}

fn parse(bytes: &[u8]) -> Option<DesignComponentOccurrence> {
    leading_flag_component_occurrence(bytes, 0, bytes.len(), STREAM)
}

#[test]
fn leading_flag_frames_state_the_occurrence_and_its_offsets() {
    for matrix in [false, true] {
        let extra = if matrix { 128 } else { 0 };
        let record = parse(&frame(matrix)).unwrap();
        assert_eq!(record.record_index, 253);
        assert_eq!(record.component_record_index, 202);
        assert_eq!(record.owner_entity, 6);
        assert_eq!(record.component_guid.as_str(), COMPONENT);
        assert_eq!(record.occurrence_guid.as_str(), OCCURRENCE);
        assert_eq!(record.occurrence_ordinal(), 1);
        assert_eq!(
            record.path_discriminator.map(std::num::NonZeroU32::get),
            Some(1)
        );
        assert_eq!(
            record.transform().map(|transform| transform.offset),
            matrix.then_some(51)
        );
        if let Some(transform) = record.transform() {
            let expected: crate::records::sketch_placement::SketchPlacementMatrix = [
                [1.0, 0.0, 0.0, 5.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]
            .try_into()
            .unwrap();
            assert_eq!(transform.value, expected);
        }
        let wire = serde_json::to_value(&record).unwrap();
        assert_eq!(wire["component_guid_offset"], 55 + extra);
        assert_eq!(wire["occurrence_guid_offset"], 131 + extra);
        assert_eq!(wire["owner_entity_offset"], 241 + 2 * extra);
        assert_eq!(wire["leading_flag"]["ordinal_prefix"], 1);
        assert_eq!(wire["leading_flag"]["record_reference"], 11);
        let decoded: DesignComponentOccurrence = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(decoded, record);
        for field in [
            "component_guid_offset",
            "occurrence_guid_offset",
            "owner_entity_offset",
        ] {
            let mut changed = wire.clone();
            changed[field] = serde_json::json!(wire[field].as_u64().unwrap() + 1);
            assert!(serde_json::from_value::<DesignComponentOccurrence>(changed).is_err());
        }
    }
}

#[test]
fn a_leading_flag_frame_with_one_wrong_field_states_no_occurrence() {
    for matrix in [false, true] {
        let extra = if matrix { 128 } else { 0 };
        let body = 51 + extra;
        let tail = body + 180 + extra;
        let mut cases: Vec<Vec<u8>> = Vec::new();
        let mut edit = |change: &dyn Fn(&mut Vec<u8>)| {
            let mut bytes = frame(matrix);
            change(&mut bytes);
            cases.push(bytes);
        };
        edit(&|bytes| bytes[40..44].copy_from_slice(&1_u32.to_le_bytes()));
        edit(&|bytes| bytes[46..50].copy_from_slice(&0_u32.to_le_bytes()));
        edit(&|bytes| bytes[50] = 2);
        edit(&|bytes| bytes[33] = 1);
        edit(&|bytes| bytes[body + 152] = 0);
        edit(&|bytes| bytes[body + 161] = 1);
        edit(&|bytes| bytes[body + 163..body + 167].copy_from_slice(&2_u32.to_le_bytes()));
        edit(&|bytes| bytes[body + 169..body + 177].copy_from_slice(&203_u64.to_le_bytes()));
        edit(&|bytes| bytes[body + 177] = 1);
        edit(&|bytes| bytes[body + 179] ^= 1);
        edit(&|bytes| bytes[tail] = 1);
        edit(&|bytes| bytes[tail + 9] = 0);
        edit(&|bytes| bytes.push(0));
        edit(&|bytes| {
            bytes.pop();
        });
        if matrix {
            edit(&|bytes| {
                bytes[body + 180 + 24..body + 180 + 32].copy_from_slice(&6.0_f64.to_le_bytes());
            });
        }
        for bytes in cases {
            assert!(parse(&bytes).is_none());
        }
    }
}

/// The bytes registered as entity 253 at offset 0 under one Component type.
fn registered_under(type_guid: &str) -> crate::metastream::MetaStream {
    use crate::metastream::{MetaStream, RecordIndexEntry};
    use crate::records::entity_header::{BaseTypeGuid, SegmentTypeData};
    use crate::records::identity::ReferenceRun;
    MetaStream {
        types: vec![SegmentTypeData {
            byte_offset: 0,
            type_guid: type_guid.to_owned().try_into().unwrap(),
            type_guid_offset: 0,
            base_type_guid: BaseTypeGuid::Absent,
            version: 2,
            version_offset: 0,
            module: "Component".to_owned(),
            entities: ReferenceRun::unlocated(vec![253]),
        }],
        records: vec![RecordIndexEntry {
            entity_id: 253,
            bulk_offset: 0,
        }],
        secondary_records: Vec::new(),
    }
}

/// Discover and admit the occurrences of one stream as the decoder does.
fn discover(
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> (
    Vec<DesignComponentOccurrence>,
    Vec<cadmpeg_ir::report::loss::LossNote>,
    usize,
) {
    let key = |entity: u64, start: usize| (entity, u64::try_from(start).unwrap());
    let frames = crate::metastream::primary_record_frames(meta, bytes.len()).unwrap();
    let typed = super::super::occurrence_frames(bytes, meta).unwrap();
    let primary = frames
        .iter()
        .map(|frame| key(frame.entity_id, frame.start))
        .collect();
    let typed_primary = typed
        .iter()
        .map(|frame| key(frame.entity_id, frame.start))
        .collect();
    let frame_ends = frames
        .iter()
        .map(|frame| (frame.start, frame.member_end))
        .collect();
    let (mut occurrences, losses) = super::super::discover_occurrences(
        bytes,
        STREAM,
        &frame_ends,
        &typed_primary,
        &super::super::occurrence_source_entities(meta),
    );
    let superseded =
        super::super::admit_occurrence_envelopes(&mut occurrences, &primary, &typed_primary);
    (occurrences, losses, superseded)
}

#[test]
fn a_leading_flag_frame_is_a_carrier_only_under_the_occurrence_placement_type() {
    const OWNER_TYPE: &str = "904F885E-D859-4755-A7EF-6EBED8732FAD";
    let placement = registered_under(super::super::OCCURRENCE_PLACEMENT_TYPE_GUID);
    let other = registered_under(OWNER_TYPE);
    for matrix in [false, true] {
        let mut bytes = frame(matrix);
        bytes[4..7].copy_from_slice(b"256");
        let (occurrences, losses, superseded) = discover(&bytes, &placement);
        assert_eq!(occurrences.len(), 1);
        assert_eq!(occurrences[0].component_record_index, 202);
        assert!(losses.is_empty());
        assert_eq!(superseded, 0);
        let (occurrences, losses, superseded) = discover(&bytes, &other);
        assert!(occurrences.is_empty());
        assert!(losses.is_empty());
        assert_eq!(superseded, 0);
    }
    let mut later = frame(false);
    later[4..7].copy_from_slice(b"256");
    later[46..50].copy_from_slice(&2_u32.to_le_bytes());
    assert_eq!(discover(&later, &placement).1.len(), 1);
    assert!(discover(&later, &other).1.is_empty());
}
