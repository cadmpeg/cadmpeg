// SPDX-License-Identifier: Apache-2.0

use super::super::construction_groups::{
    parse_construction_operand_group_in_stream, ConstructionOperandGroupParse, RecordFrame,
};
use crate::metastream::{MetaStream, RecordIndexEntry};
use crate::records::entity_header::{BaseTypeGuid, SegmentTypeData};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::ReferenceRun;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::test_support::{indexed_header, push_marked_reference};
use cadmpeg_core::decode::u64_from_index;

const SCOPE: u32 = 17;
const GROUP: u32 = 100;
const BODY_GROUP: u32 = 200;
const COMPANION: u32 = 300;

fn scope(kind: DesignFeatureKind, references: Vec<u32>) -> DesignParameterScope {
    let mut draft =
        DesignParameterScope::empty("f3d:Design/BulkStream.dat:scope#17", kind, SCOPE).into_draft();
    draft.reference_members = ReferenceRun::unlocated(references);
    draft.layout_fixture_references();
    draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
    draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
    draft.layout_fixture_tail();
    DesignParameterScope::try_new(draft).unwrap()
}

fn append_group(
    bytes: &mut Vec<u8>,
    index: u32,
    member: u32,
    role: DesignOperandRole,
    flags: [u8; 2],
) -> RecordFrame {
    let byte_offset = u64_from_index(bytes.len());
    indexed_header(bytes, *b"256", index);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    push_marked_reference(bytes, member);
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    push_marked_reference(bytes, index + 40);
    bytes.extend_from_slice(&role.raw().to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    push_marked_reference(bytes, COMPANION);
    bytes.extend_from_slice(&flags);
    push_marked_reference(bytes, index + 1);
    bytes.push(0);
    push_marked_reference(bytes, SCOPE);
    indexed_header(bytes, *b"258", index);
    RecordFrame {
        byte_offset,
        class_tag: "256".to_owned().try_into().unwrap(),
        record_index: index,
    }
}

fn fusion_type(guid: &str, base: &str, version: u32, entities: Vec<u64>) -> SegmentTypeData {
    SegmentTypeData {
        byte_offset: 0,
        type_guid: guid.to_owned().try_into().unwrap(),
        type_guid_offset: 0,
        base_type_guid: BaseTypeGuid::Guid {
            value: base.to_owned().try_into().unwrap(),
            offset: 0,
        },
        version,
        version_offset: 0,
        module: "Fusion".into(),
        entities: ReferenceRun::unlocated(entities),
    }
}

fn fixture(flags: [u8; 2]) -> (Vec<u8>, MetaStream, [RecordFrame; 2]) {
    let mut bytes = Vec::new();
    let tools = append_group(
        &mut bytes,
        GROUP,
        GROUP + 2,
        DesignOperandRole::ROLE_0X21,
        flags,
    );
    let targets = append_group(
        &mut bytes,
        BODY_GROUP,
        BODY_GROUP + 2,
        DesignOperandRole::BODIES_A,
        [0, 0],
    );
    let companion_offset = u64_from_index(bytes.len());
    indexed_header(&mut bytes, *b"257", COMPANION);
    let metadata = MetaStream {
        types: vec![
            fusion_type(
                "2CA5A1CD-C99B-4C9B-91AF-57989148E841",
                "F8CD0BB3-2CE6-4236-8C1B-6D0638BF215F",
                1,
                vec![u64::from(GROUP), u64::from(BODY_GROUP)],
            ),
            fusion_type(
                "5FBE538E-139B-46D4-BCAF-CC796D1E1F78",
                "98542EB9-A4F2-4137-A808-DBB5B3CD6159",
                0,
                vec![u64::from(COMPANION)],
            ),
        ],
        records: vec![
            RecordIndexEntry {
                entity_id: u64::from(GROUP),
                bulk_offset: tools.byte_offset,
            },
            RecordIndexEntry {
                entity_id: u64::from(BODY_GROUP),
                bulk_offset: targets.byte_offset,
            },
            RecordIndexEntry {
                entity_id: u64::from(COMPANION),
                bulk_offset: companion_offset,
            },
        ],
        secondary_records: Vec::new(),
    };
    (bytes, metadata, [tools, targets])
}

#[test]
fn compact_split_groups_keep_the_ordered_member_runs() {
    let (bytes, metadata, headers) = fixture([0, 0]);
    let owner = scope(
        DesignFeatureKind::Split,
        vec![GROUP, GROUP + 2, BODY_GROUP, BODY_GROUP + 2],
    );
    for (header, ordinal, role) in [
        (&headers[0], 0, DesignOperandRole::ROLE_0X21),
        (&headers[1], 2, DesignOperandRole::BODIES_A),
    ] {
        let group = parse_construction_operand_group_in_stream(
            &bytes,
            &owner,
            ordinal,
            header,
            None,
            Some(&metadata),
        )
        .complete()
        .expect("the registered compact group closes");
        assert_eq!(group.role(), role);
        assert_eq!(group.scope_reference_ordinal, ordinal);
        assert_eq!(
            group
                .members()
                .iter()
                .map(|member| member.value)
                .collect::<Vec<_>>(),
            [header.record_index + 2]
        );
        assert_eq!(
            group.frame.trailing_records()[0].value,
            header.record_index + 40
        );
        assert!(!group.frame.closing_flag);
    }
}

#[test]
fn compact_split_groups_require_a_live_companion_and_exact_owner_frame() {
    let (bytes, metadata, headers) = fixture([0, 0]);
    let owner = scope(
        DesignFeatureKind::Split,
        vec![GROUP, GROUP + 2, BODY_GROUP, BODY_GROUP + 2],
    );
    let assert_unclosed = |bytes: &[u8],
                           owner: &DesignParameterScope,
                           meta: Option<&MetaStream>| {
        assert!(matches!(
            parse_construction_operand_group_in_stream(bytes, owner, 0, &headers[0], None, meta),
            ConstructionOperandGroupParse::Unclosed
        ));
    };
    assert_unclosed(&bytes, &owner, None);
    let mut wrong_type = metadata.clone();
    wrong_type.types[1].version = 1;
    assert_unclosed(&bytes, &owner, Some(&wrong_type));
    let mut wrong_group = metadata.clone();
    wrong_group.types[0].version = 0;
    assert_unclosed(&bytes, &owner, Some(&wrong_group));
    let mut absent_record = metadata.clone();
    absent_record.records.pop();
    assert_unclosed(&bytes, &owner, Some(&absent_record));
    let mut shifted_header = metadata.clone();
    shifted_header.records[2].bulk_offset += 1;
    assert_unclosed(&bytes, &owner, Some(&shifted_header));
    let wrong_member = scope(
        DesignFeatureKind::Split,
        vec![GROUP, GROUP + 3, BODY_GROUP, BODY_GROUP + 2],
    );
    assert_unclosed(&bytes, &wrong_member, Some(&metadata));
    let (wrong_flags, metadata, _) = fixture([0, 1]);
    assert_unclosed(&wrong_flags, &owner, Some(&metadata));
}
