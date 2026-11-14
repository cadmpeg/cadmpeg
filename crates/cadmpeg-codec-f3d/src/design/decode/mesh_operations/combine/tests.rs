use super::*;

fn reference(target: u32) -> [u8; 11] {
    let mut bytes = [0; 11];
    bytes[0] = 1;
    bytes[1..5].copy_from_slice(&target.to_le_bytes());
    bytes
}

fn scope(version: u32, flagged: bool) -> Vec<u8> {
    let mut bytes = vec![0; 21];
    if flagged {
        bytes[20] = 1;
        bytes.extend_from_slice(&77u32.to_le_bytes());
    }
    bytes.extend_from_slice(&2u32.to_le_bytes());
    if version == 2 {
        bytes.extend_from_slice(&19u32.to_le_bytes());
    }
    bytes.push(0);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&reference(43));
    bytes.extend_from_slice(&reference(41));
    bytes.extend_from_slice(&reference(51));
    bytes.extend_from_slice(&reference(52));
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&reference(42));
    bytes
}

fn read(bytes: &[u8], version: u32) -> MeshCombineScope<'_> {
    crate::test_support::with_decode_context(|ctx| {
        read_scope(ctx, View::over_retained(bytes), version)
    })
    .unwrap()
}

#[test]
fn combine_versions_and_flags_keep_controls_and_order() {
    for version in [1, 2] {
        for flagged in [false, true] {
            let bytes = scope(version, flagged);
            let parsed = read(&bytes, version);
            assert_eq!(parsed.version, version);
            assert_eq!(parsed.flag, Some(u8::from(flagged)));
            assert_eq!(parsed.prefix_word, flagged.then_some(77));
            assert_eq!(parsed.control, Some(2));
            assert_eq!(parsed.extra_word, (version == 2).then_some(19));
            assert_eq!(parsed.raw_byte, Some(0));
            assert_eq!(parsed.tool_count, Some(2));
            assert_eq!(
                parsed.tools,
                [CombineReference::Local(43), CombineReference::Local(41)]
            );
            assert_eq!(parsed.output_geometry, Some(CombineReference::Local(51)));
            assert_eq!(parsed.input_geometry, Some(CombineReference::Local(52)));
            assert_eq!(parsed.tail_word, Some(0));
            assert_eq!(parsed.target, Some(CombineReference::Local(42)));
            assert_eq!(parsed.framing, None);
        }
    }
}

#[test]
fn combine_null_tool_keeps_the_later_tool_and_tail() {
    let mut bytes = scope(2, false);
    bytes.splice(34..45, [0]);
    let parsed = read(&bytes, 2);
    assert_eq!(
        parsed.tools,
        [CombineReference::Null, CombineReference::Local(41)]
    );
    assert_eq!(parsed.target, Some(CombineReference::Local(42)));
    assert_eq!(parsed.framing, None);
}

#[test]
fn combine_truncated_reference_keeps_the_readable_tool_prefix() {
    let bytes = scope(2, false);
    let parsed = read(&bytes[..49], 2);
    assert_eq!(parsed.control, Some(2));
    assert_eq!(parsed.tool_count, Some(2));
    assert_eq!(parsed.tools, [CombineReference::Local(43)]);
    assert_eq!(parsed.output_geometry, None);
    assert!(parsed.framing.is_some());
}

#[test]
fn combine_oversized_count_keeps_the_declared_count_and_readable_prefix() {
    let mut bytes = scope(2, false);
    bytes[30..34].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes.truncate(45);
    let parsed = read(&bytes, 2);
    assert_eq!(parsed.tool_count, Some(u32::MAX));
    assert_eq!(parsed.tools, [CombineReference::Local(43)]);
    assert!(parsed.framing.is_some());
    assert_eq!(parsed.target, None);
}

#[test]
fn combine_partial_tail_preserves_both_tools_and_output_reference() {
    let bytes = scope(2, false);
    let parsed = read(&bytes[..70], 2);
    assert_eq!(
        parsed.tools,
        [CombineReference::Local(43), CombineReference::Local(41)]
    );
    assert_eq!(parsed.output_geometry, Some(CombineReference::Local(51)));
    assert_eq!(parsed.input_geometry, None);
    assert_eq!(parsed.target, None);
    assert!(parsed.framing.is_some());
}

#[test]
fn combine_unknown_flag_retains_the_flag_and_stops_at_its_boundary() {
    let mut bytes = scope(1, false);
    bytes[20] = 9;
    let parsed = read(&bytes, 1);
    assert_eq!(parsed.flag, Some(9));
    assert_eq!(parsed.control, None);
    assert_eq!(parsed.tools, []);
    assert_eq!(parsed.framing, Some(MeshOperationFraming("combine flag")));
}

#[test]
fn combine_registered_versions_select_their_own_grammar() {
    use crate::metastream::{MetaStream, RecordIndexEntry};
    use crate::records::entity_header::{BaseTypeGuid, SegmentTypeData};
    use crate::records::identity::ReferenceRun;
    for version in [1, 2, 3] {
        let mut bytes = scope(version, false);
        bytes[..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"256");
        bytes[7..11].copy_from_slice(&7u32.to_le_bytes());
        let meta = std::sync::Arc::new(MetaStream {
            types: vec![SegmentTypeData {
                byte_offset: 0,
                type_guid: "EE0E56E8-5894-4D65-BAF4-9DAAF811307F"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                type_guid_offset: 0,
                base_type_guid: BaseTypeGuid::Guid {
                    value: "90B8D2C6-05BB-4D0F-89EF-83AAC46CA1FF"
                        .to_owned()
                        .try_into()
                        .unwrap(),
                    offset: 0,
                },
                version,
                version_offset: 0,
                module: "Fusion".into(),
                entities: ReferenceRun::unlocated(vec![7]),
            }],
            records: vec![RecordIndexEntry {
                entity_id: 7,
                bulk_offset: 0,
            }],
            secondary_records: Vec::new(),
        });
        let records = MeshOperationRecords::index(View::over_retained(&bytes), meta);
        let parsed =
            crate::test_support::with_decode_context(|ctx| records.combine_scope(ctx, 7)).unwrap();
        if version == 3 {
            assert_eq!(parsed, Err(MeshOperationFraming("combine scope record")));
        } else {
            let parsed = parsed.unwrap();
            assert_eq!(parsed.version, version);
            assert_eq!(parsed.control, Some(2));
            assert_eq!(parsed.target, Some(CombineReference::Local(42)));
            assert_eq!(parsed.framing, None);
        }
    }
}

#[test]
fn combine_inline_group_identity_keeps_its_target_type_and_following_fields() {
    let mut bytes = scope(2, false);
    let mut typed = vec![1];
    typed.extend_from_slice(&43u64.to_le_bytes());
    typed.extend_from_slice(&36u32.to_le_bytes());
    typed.extend_from_slice(b"22975A91-16AD-4CC9-A5E4-B4F8B4C27881");
    typed.extend_from_slice(&[0, 0]);
    bytes.splice(34..45, typed);
    let parsed = read(&bytes, 2);
    assert_eq!(
        parsed.tools,
        [
            CombineReference::TypedLocal {
                target: 43,
                inline_type: "22975A91-16AD-4CC9-A5E4-B4F8B4C27881"
            },
            CombineReference::Local(41),
        ]
    );
    assert_eq!(parsed.target, Some(CombineReference::Local(42)));
    assert_eq!(parsed.framing, None);
}
