use super::*;
use cadmpeg_core::decode::u64_from_index;

fn reference(target: u64) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&target.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes
}

fn scope() -> Vec<u8> {
    let mut bytes = vec![0; 21];
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend(reference(41));
    bytes.extend(reference(42));
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend(reference(57));
    bytes.push(0);
    crate::test_support::lp_utf16(&mut bytes, "12345678-1357-2468-9876-123456789abc");
    bytes.extend_from_slice(&[0; 3]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend(reference(57));
    bytes.extend(reference(96));
    bytes
}

fn read(bytes: &[u8]) -> SeparateScope<'_> {
    let metadata = crate::test_support::streams_fixture::design_metastream_with_records(&[], &[]);
    let meta = crate::metastream::parse(&metadata, "synthetic metadata").unwrap();
    let records =
        MeshOperationRecords::index(View::over_retained(bytes), std::sync::Arc::new(meta));
    crate::test_support::with_decode_context(|ctx| {
        read_scope(ctx, &records, View::over_retained(bytes))
    })
    .unwrap()
}

#[test]
fn separate_preserves_order_and_independent_raw_fields() {
    let bytes = scope();
    let parsed = read(&bytes);
    assert_eq!(parsed.outputs.declared, Some(2));
    assert_eq!(
        parsed
            .outputs
            .slots
            .iter()
            .filter_map(|slot| slot.target())
            .collect::<Vec<_>>(),
        [41, 42]
    );
    assert_eq!(parsed.outputs.slots[0].offset, 25);
    assert_eq!(parsed.outputs.slots[1].offset, 36);
    assert_eq!(parsed.selector, Some(0));
    assert_eq!(parsed.lane, &[0, 0, 0, 0, 1, 0, 0, 0]);
    assert_eq!(parsed.source.unwrap().target(), Some(57));
    assert_eq!(parsed.common.slots[0].target(), Some(57));
    assert_eq!(parsed.common.slots[1].target(), Some(96));
    assert_eq!(parsed.framing, None);
}

#[test]
fn separate_every_truncated_boundary_retains_the_readable_prefix() {
    let bytes = scope();
    for length in 0..bytes.len() {
        let parsed = read(&bytes[..length]);
        assert!(parsed.framing.is_some(), "length {length}");
        if length >= 36 {
            assert_eq!(parsed.outputs.slots[0].target(), Some(41));
        }
        if length >= 47 {
            assert_eq!(parsed.outputs.slots[1].target(), Some(42));
        }
        if length >= 51 {
            assert_eq!(parsed.selector, Some(0));
            assert_eq!(parsed.lane.len(), (length - 51).min(8));
        }
        if length >= 70 {
            assert_eq!(parsed.source.unwrap().target(), Some(57));
        }
    }
}

#[test]
fn separate_null_typed_zero_nonlocal_and_duplicate_slots_keep_positions() {
    let mut bytes = scope();
    bytes.splice(25..36, [0]);
    let parsed = read(&bytes);
    assert_eq!(parsed.outputs.slots[0].value, SlotValue::Null);
    assert_eq!(parsed.outputs.slots[1].target(), Some(42));
    assert_eq!(parsed.selector, Some(0));
    let mut bytes = scope();
    bytes[37..45].copy_from_slice(&41u64.to_le_bytes());
    assert_eq!(read(&bytes).outputs.slots[1].target(), Some(41));
    bytes[26..34].fill(0);
    assert!(matches!(
        read(&bytes).outputs.slots[0].value,
        SlotValue::Invalid(_)
    ));
    assert_eq!(read(&bytes).selector, Some(0));
    let mut bytes = scope();
    let mut typed = reference(41);
    let mut guid = Vec::new();
    crate::test_support::lp_ascii(&mut guid, "abcdefab-cdef-abcd-efab-cdefabcdefab");
    typed.splice(9..9, guid);
    bytes.splice(25..36, typed);
    for length in 26..76 {
        assert!(
            matches!(
                read(&bytes[..length]).outputs.slots[0].value,
                SlotValue::Unread(_)
            ),
            "typed prefix length {length}"
        );
    }
    assert!(matches!(
        read(&bytes).outputs.slots[0].value,
        SlotValue::Present {
            inline_type: Some("abcdefab-cdef-abcd-efab-cdefabcdefab"),
            ..
        }
    ));
    assert_eq!(read(&bytes).selector, Some(0));
    let mut invalid_guid = bytes.clone();
    invalid_guid[38] = b'Z';
    assert!(matches!(
        read(&invalid_guid).outputs.slots[0].value,
        SlotValue::Invalid(_)
    ));
    let mut invalid_marker = bytes;
    invalid_marker[74] = 9;
    assert!(matches!(
        read(&invalid_marker).outputs.slots[0].value,
        SlotValue::Invalid(_)
    ));
    let mut bytes = scope();
    let mut cross = reference(41);
    cross.pop();
    cross.push(1);
    cross.extend_from_slice(&9u32.to_le_bytes());
    bytes.splice(25..36, cross);
    assert!(matches!(
        read(&bytes).outputs.slots[0].value,
        SlotValue::Invalid(_)
    ));
    assert_eq!(read(&bytes).outputs.slots[1].target(), Some(42));
}

#[test]
fn separate_unread_and_invalid_envelopes_stop_the_dependent_tail() {
    for marker in [1, 9] {
        let mut bytes = scope();
        bytes.truncate(39);
        bytes[36] = marker;
        let parsed = read(&bytes);
        assert_eq!(parsed.outputs.slots[0].target(), Some(41));
        assert_eq!(parsed.outputs.slots[1].offset, 36);
        assert_eq!(
            matches!(parsed.outputs.slots[1].value, SlotValue::Unread(_)),
            marker == 1
        );
        assert_eq!(
            matches!(parsed.outputs.slots[1].value, SlotValue::Invalid(_)),
            marker == 9
        );
        assert_eq!(parsed.outputs.unread, Some((1, 36, 1)));
        assert_eq!(parsed.selector, None);
    }
}

#[test]
fn separate_flagged_prefix_and_impossible_counts_preserve_raw_facts() {
    let mut bytes = scope();
    bytes[20] = 1;
    bytes.splice(21..21, 77u32.to_le_bytes());
    let parsed = read(&bytes);
    assert_eq!(parsed.prefix_word, Some(77));
    assert_eq!(parsed.outputs.slots[0].offset, 29);
    assert_eq!(parsed.source.unwrap().target(), Some(57));
    let mut bytes = scope();
    bytes.truncate(47);
    bytes[21..25].copy_from_slice(&u32::MAX.to_le_bytes());
    let parsed = read(&bytes);
    assert_eq!(parsed.outputs.declared, Some(u32::MAX));
    assert_eq!(parsed.outputs.slots.len(), 3);
    assert_eq!(parsed.outputs.slots[0].target(), Some(41));
    assert_eq!(parsed.outputs.slots[1].target(), Some(42));
    assert_eq!(parsed.selector, None);
    assert_eq!(parsed.outputs.unread, Some((2, 47, u32::MAX - 2)));
}

#[test]
fn separate_temporary_slots_do_not_consume_retained_budget() {
    let bytes = scope();
    let metadata = crate::test_support::streams_fixture::design_metastream_with_records(&[], &[]);
    let meta = crate::metastream::parse(&metadata, "synthetic metadata").unwrap();
    let records =
        MeshOperationRecords::index(View::over_retained(&bytes), std::sync::Arc::new(meta));
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let result = crate::test_support::with_decode_policy(&policy, |ctx| {
        read_scope(ctx, &records, View::over_retained(&bytes))
    });
    assert!(result.is_ok());
}

#[test]
fn separate_admission_checks_identity_registration_bounds_and_pairing() {
    use crate::metastream::RecordIndexEntry;
    use crate::records::identity::ReferenceRun;
    let mut bytes = scope();
    bytes[..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"256");
    bytes[7..11].copy_from_slice(&88u32.to_le_bytes());
    let metadata = crate::test_support::streams_fixture::design_metastream_with_records(
        &[(
            "6B6FDE2E-D9B9-4866-9D07-3123C919B0D2",
            "90B8D2C6-05BB-4D0F-89EF-83AAC46CA1FF",
            2,
            "Fusion",
            &[88],
        )],
        &[(88, 0)],
    );
    let meta = crate::metastream::parse(&metadata, "synthetic metadata").unwrap();
    for case in 0..9 {
        let mut meta = meta.clone();
        let mut bytes = bytes.clone();
        match case {
            0 => (),
            1 => meta.types[0].version = 3,
            2 => meta.types[0].module = "Other".into(),
            3 => {
                meta.types[0].type_guid = "abcdefab-cdef-abcd-efab-cdefabcdefab"
                    .to_owned()
                    .try_into()
                    .unwrap();
            }
            4 => meta.types[0].entities = ReferenceRun::unlocated(vec![88, 88]),
            5 => meta.records.push(meta.records[0]),
            6 => meta.records[0].bulk_offset = u64_from_index(bytes.len()),
            7 => meta.secondary_records.push(RecordIndexEntry {
                entity_id: 88,
                bulk_offset: 0,
            }),
            _ => bytes[7..11].copy_from_slice(&89u32.to_le_bytes()),
        }
        let records =
            MeshOperationRecords::index(View::over_retained(&bytes), std::sync::Arc::new(meta));
        let admitted =
            crate::test_support::with_decode_context(|ctx| records.separate_scope(ctx, 88))
                .unwrap()
                .is_ok();
        assert_eq!(admitted, case == 0, "case {case}");
    }
}

#[test]
fn separate_member_fields_retain_each_complete_value_before_truncation() {
    let mut bytes = vec![0; 21];
    bytes.extend(reference(101));
    bytes.extend(reference(74));
    bytes.extend_from_slice(&19u32.to_le_bytes());
    crate::test_support::lp_utf16(&mut bytes, "12345678-1357-2468-9876-123456789abc");
    crate::test_support::lp_utf16(&mut bytes, "abcdefab-cdef-abcd-efab-cdefabcdefab");
    bytes.extend_from_slice(&23u32.to_le_bytes());
    bytes.extend_from_slice(&29u32.to_le_bytes());
    let metadata = crate::test_support::streams_fixture::design_metastream_with_records(&[], &[]);
    let meta = crate::metastream::parse(&metadata, "synthetic metadata").unwrap();
    let records =
        MeshOperationRecords::index(View::over_retained(&bytes), std::sync::Arc::new(meta));
    for length in 0..=bytes.len() {
        let details = crate::test_support::with_decode_context(|ctx| {
            records.separate_details(ctx, View::over_retained(&bytes[..length]), true)
        })
        .unwrap();
        assert_eq!(details.framing.is_none(), length == bytes.len());
        if length >= 32 {
            assert_eq!(details.references[0].target(), Some(101));
        }
        if length >= 43 {
            assert_eq!(details.references[1].target(), Some(74));
        }
        if length >= 47 {
            assert_eq!(details.words[0], 19);
        }
        if length >= 123 {
            assert_eq!(details.identifiers[0].len(), 72);
        }
        if length >= 199 {
            assert_eq!(details.identifiers[1].len(), 72);
        }
        if length >= 203 {
            assert_eq!(details.words[1], 23);
        }
        if length >= 207 {
            assert_eq!(details.words[2], 29);
        }
    }
}

#[test]
fn separate_malformed_first_slot_keeps_partial_result_without_retained_slot_charge() {
    let mut bytes = vec![0; 21];
    bytes.extend_from_slice(&10_000u32.to_le_bytes());
    bytes.extend_from_slice(&[2; 10_000]);
    let metadata = crate::test_support::streams_fixture::design_metastream_with_records(&[], &[]);
    let meta = crate::metastream::parse(&metadata, "synthetic metadata").unwrap();
    let records =
        MeshOperationRecords::index(View::over_retained(&bytes), std::sync::Arc::new(meta));
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 1;
    let parsed = crate::test_support::with_decode_policy(&policy, |ctx| {
        read_scope(ctx, &records, View::over_retained(&bytes))
    })
    .unwrap();
    assert_eq!(parsed.outputs.declared, Some(10_000));
    assert_eq!(parsed.outputs.slots.len(), 1);
    assert!(matches!(
        parsed.outputs.slots[0].value,
        SlotValue::Invalid(_)
    ));
    assert_eq!(parsed.outputs.slots[0].raw, &bytes[25..]);
    assert_eq!(parsed.outputs.unread, Some((0, 25, 10_000)));
}

#[test]
fn separate_actual_slots_obey_entity_budget() {
    let mut bytes = vec![0; 21];
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    let metadata = crate::test_support::streams_fixture::design_metastream_with_records(&[], &[]);
    let meta = crate::metastream::parse(&metadata, "synthetic metadata").unwrap();
    let records =
        MeshOperationRecords::index(View::over_retained(&bytes), std::sync::Arc::new(meta));
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_entities = 1;
    let result = crate::test_support::with_decode_policy(&policy, |ctx| {
        read_scope(ctx, &records, View::over_retained(&bytes))
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(_))));
}
