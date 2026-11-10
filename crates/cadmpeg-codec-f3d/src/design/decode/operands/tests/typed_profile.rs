// SPDX-License-Identifier: Apache-2.0
//! Typed-reference profile selections that list Sketch curve loops.

use cadmpeg_core::decode::u64_from_index;

use crate::records::decal::DesignRecordHeader;
use crate::records::identity::Located;
use crate::records::references::DesignClassTag;
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
    DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
    DesignConstructionOperandRole,
};
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::test_support::lp_utf16;

const TYPE: &str = "11111111-2222-3333-4444-555555555555";

fn typed_reference(out: &mut Vec<u8>, target: u32) {
    out.push(1);
    out.extend_from_slice(&u64::from(target).to_le_bytes());
    out.extend_from_slice(&36u32.to_le_bytes());
    out.extend_from_slice(TYPE.as_bytes());
    out.extend_from_slice(&[0, 0]);
}

fn header(out: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32) {
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(class_tag);
    out.extend_from_slice(&record_index.to_le_bytes());
}

fn group() -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: "f3d:Design/BulkStream.dat:construction-group#90".into(),
        scope_record_index: 80,
        scope_reference_ordinal: 0,
        record_index: 90,
        byte_offset: 1000,
        class_tag: DesignClassTag::try_from("289".to_owned()).unwrap(),
        members: vec![Located {
            value: 100,
            offset: 1026,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 1021,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: vec![Located {
                    value: 300,
                    offset: 1100,
                }],
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 100,
                opaque_index_offset: 1128,
                opaque_scalar: 0.5,
                opaque_scalar_offset: 1132,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(DesignOperandRole::PROFILE),
        role_offset: 1110,
        paired_class_tag: DesignClassTag::try_from("259".to_owned()).unwrap(),
        paired_byte_offset: 1200,
    })
    .unwrap()
}

fn entry(out: &mut Vec<u8>, primary_id: u64) {
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&primary_id.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]);
    out.extend_from_slice(&[0; 8]);
}

/// Selection record 100, its paired header, the chain record 101 naming
/// Sketch scope 77, record 102, and identity record 103 with one outer loop
/// of three curves and one inner loop of one curve.
fn chain(inner_loop: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    header(&mut bytes, b"264", 100);
    bytes.extend_from_slice(&[0; 10]);
    typed_reference(&mut bytes, 103);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    lp_utf16(&mut bytes, "75");
    bytes.extend_from_slice(&[0; 8]);
    header(&mut bytes, b"262", 100);
    header(&mut bytes, b"277", 101);
    bytes.extend_from_slice(&[0; 14]);
    bytes.extend_from_slice(&[1, 1, 1, 0, 0, 0]);
    typed_reference(&mut bytes, 77);
    header(&mut bytes, b"273", 102);
    bytes.extend_from_slice(&[0; 22]);
    header(&mut bytes, b"297", 103);
    bytes.extend_from_slice(&[0; 10]);
    typed_reference(&mut bytes, 100);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    for id in [11, 12, 13] {
        entry(&mut bytes, id);
    }
    bytes.push(1);
    if inner_loop {
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        entry(&mut bytes, 14);
        bytes.push(1);
    } else {
        bytes.extend_from_slice(&0u32.to_le_bytes());
    }
    header(&mut bytes, b"275", 103);
    bytes.extend_from_slice(&[0; 8]);
    bytes
}

/// The `chain` records with an identity record of the given body: `chain`
/// builds its identity body at offset `identity_body(&chain(false))`.
fn chain_with_groups(body: &[u8]) -> Vec<u8> {
    let plain = chain(false);
    let start = identity_body(&plain);
    let end = plain
        .windows(7)
        .rposition(|window| window == [3, 0, 0, 0, b'2', b'7', b'5'])
        .unwrap();
    let mut bytes = plain[..start].to_vec();
    bytes.extend_from_slice(body);
    bytes.extend_from_slice(&plain[end..]);
    bytes
}

/// Offset of the region groups in `chain`'s identity record.
fn identity_body(bytes: &[u8]) -> usize {
    bytes
        .windows(7)
        .position(|window| window == [3, 0, 0, 0, b'2', b'9', b'7'])
        .unwrap()
        + crate::layout::typed_reference_profile_identity_record::REGION_GROUP_COUNT
}

/// One region of one exterior loop.
fn region(out: &mut Vec<u8>, ids: &[u64]) {
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&u32::try_from(ids.len()).unwrap().to_le_bytes());
    for id in ids {
        entry(out, *id);
    }
    out.push(1);
}

fn parse(
    bytes: &[u8],
) -> Option<crate::records::topology::entity_selection::DesignProfileCurveSelection> {
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:design-record-header#0".into(),
        record_index: 100,
        class_tag: DesignClassTag::try_from("264".to_owned()).unwrap(),
        byte_offset: 0,
    };
    crate::test_support::with_decode_context(|ctx| {
        super::super::parse_profile_curve_selection(ctx, bytes, &records, &group(), 0, &header)
    })
    .unwrap()
}

#[test]
fn typed_profile_selection_reads_owner_scope_and_curve_loops() {
    let selection = parse(&chain(true)).expect("typed profile selection");
    assert_eq!(selection.owner_text, "75");
    assert_eq!(selection.sketch_scope_record_index, 77);
    let ids = |region: &Vec<crate::records::topology::entity_selection::DesignProfileCurveLoop>| {
        region
            .iter()
            .flat_map(|loop_| &loop_.entries)
            .map(|entry| entry.primary_id)
            .collect::<Vec<_>>()
    };
    let [group] = selection.groups.as_slice() else {
        panic!("one region group");
    };
    let lists = [
        vec![ids(&group.region)],
        group.further.iter().map(ids).collect::<Vec<_>>(),
    ];
    assert_eq!(lists, [vec![vec![11, 12, 13]], vec![vec![14]]]);
    assert!(selection
        .regions()
        .flatten()
        .flat_map(|loop_| &loop_.entries)
        .all(|entry| entry.secondary_id == 0 && entry.entry_fields == [0, 1, 1]));
    let outer = parse(&chain(false)).expect("single-loop selection");
    assert_eq!(outer.groups.len(), 1);
    assert!(outer.groups[0].further.is_empty());
    assert_eq!(u64_from_index(0), outer.byte_offset);
}

#[test]
fn typed_profile_selection_rejects_a_chain_without_the_scope_record_or_exact_end() {
    // A changed chain prefix no longer names the Sketch scope.
    let mut bytes = chain(false);
    let chain_record = bytes
        .windows(7)
        .position(|window| window == b"\x03\x00\x00\x00277")
        .unwrap();
    bytes[chain_record + 25] = 0;
    assert!(parse(&bytes).is_none());

    // A trailing byte before the paired identity header breaks the exact end.
    let mut bytes = chain(false);
    let paired = bytes
        .windows(7)
        .rposition(|window| window == b"\x03\x00\x00\x00275")
        .unwrap();
    bytes.insert(paired, 0);
    assert!(parse(&bytes).is_none());
}

fn region_chain() -> Vec<u8> {
    let mut bytes = chain(false);
    let identity = bytes
        .windows(7)
        .position(|window| window == b"\x03\x00\x00\x00297")
        .unwrap();
    let paired = bytes
        .windows(7)
        .rposition(|window| window == b"\x03\x00\x00\x00275")
        .unwrap();
    let mut lists = Vec::new();
    lists.extend_from_slice(&1u32.to_le_bytes());
    lists.extend_from_slice(&2u32.to_le_bytes());
    lists.extend_from_slice(&1u32.to_le_bytes());
    entry(&mut lists, 14);
    lists.push(0);
    lists.extend_from_slice(&3u32.to_le_bytes());
    for id in [11, 12, 13] {
        entry(&mut lists, id);
    }
    lists.push(1);
    lists.extend_from_slice(&0u32.to_le_bytes());
    bytes.splice(
        identity + crate::layout::typed_reference_profile_identity_record::REGION_GROUP_COUNT
            ..paired,
        lists,
    );
    bytes
}

#[test]
fn typed_profile_selection_reads_region_holes_before_exterior() {
    let selection = parse(&region_chain()).unwrap();
    assert!(selection.valid_layout());
    let region = &selection.groups[0].region;
    assert_eq!(region.len(), 2);
    assert!(!region[0].outer);
    assert!(region[1].outer);
    assert_eq!(region[0].entries[0].primary_id, 14);
    assert_eq!(
        region[1]
            .entries
            .iter()
            .map(|entry| entry.primary_id)
            .collect::<Vec<_>>(),
        [11, 12, 13]
    );
}

#[test]
fn typed_profile_selection_refuses_missing_exterior_and_invalid_counts() {
    let mut bytes = region_chain();
    let parsed = parse(&bytes).unwrap();
    let exterior_flag = usize::try_from(parsed.identity_end_offset).unwrap() - 5;
    bytes[exterior_flag] = 0;
    assert!(parse(&bytes).is_none());
    bytes[exterior_flag] = 2;
    assert!(parse(&bytes).is_none());
    let mut bytes = region_chain();
    let count = usize::try_from(parsed.identity_record_offset).unwrap()
        + crate::layout::typed_reference_profile_identity_record::REGION_GROUP_COUNT
        + 8;
    bytes[count..count + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(parse(&bytes).is_none());
}

fn region_ids(
    region: &[crate::records::topology::entity_selection::DesignProfileCurveLoop],
) -> Vec<u64> {
    region
        .iter()
        .flat_map(|loop_| &loop_.entries)
        .map(|entry| entry.primary_id)
        .collect()
}

#[test]
fn typed_profile_selection_reads_several_region_groups_in_stored_order() {
    let mut body = 2u32.to_le_bytes().to_vec();
    region(&mut body, &[11, 12]);
    body.extend_from_slice(&1u32.to_le_bytes());
    region(&mut body, &[13]);
    region(&mut body, &[14, 15]);
    body.extend_from_slice(&0u32.to_le_bytes());
    let selection = parse(&chain_with_groups(&body)).expect("two region groups");
    assert!(selection.valid_layout());
    assert_eq!(selection.groups.len(), 2);
    assert_eq!(selection.groups[0].further.len(), 1);
    assert!(selection.groups[1].further.is_empty());
    let regions: Vec<Vec<u64>> = selection.regions().map(|r| region_ids(r)).collect();
    assert_eq!(regions, [vec![11, 12], vec![13], vec![14, 15]]);
}

#[test]
fn typed_profile_selection_refuses_region_groups_that_miss_the_record_end() {
    let group = |count: u32, trailing: &[u8]| {
        let mut body = count.to_le_bytes().to_vec();
        region(&mut body, &[11]);
        body.extend_from_slice(&0u32.to_le_bytes());
        region(&mut body, &[12]);
        body.extend_from_slice(&0u32.to_le_bytes());
        body.extend_from_slice(trailing);
        parse(&chain_with_groups(&body))
    };
    assert!(group(2, &[]).is_some());
    // One group leaves the second behind; three run past the record.
    assert!(group(1, &[]).is_none());
    assert!(group(3, &[]).is_none());
    assert!(group(2, &[0]).is_none());
    // No group at all.
    assert!(parse(&chain_with_groups(&0u32.to_le_bytes())).is_none());
}

fn compact_chain() -> Vec<u8> {
    fn reference(out: &mut Vec<u8>, target: u32) {
        out.push(1);
        out.extend_from_slice(&u64::from(target).to_le_bytes());
        out.extend_from_slice(&[0; 2]);
    }
    let mut bytes = Vec::new();
    header(&mut bytes, b"264", 100);
    bytes.extend_from_slice(&[0; 10]);
    reference(&mut bytes, 103);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    lp_utf16(&mut bytes, "75");
    bytes.extend_from_slice(&[0; 8]);
    header(&mut bytes, b"262", 100);
    header(&mut bytes, b"277", 101);
    bytes.extend_from_slice(&[0; 14]);
    bytes.extend_from_slice(&[1, 1, 1, 0, 0, 0]);
    reference(&mut bytes, 77);
    header(&mut bytes, b"273", 102);
    bytes.extend_from_slice(&[0; 22]);
    header(&mut bytes, b"297", 103);
    bytes.extend_from_slice(&[0; 10]);
    reference(&mut bytes, 100);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for id in [11u64, 13] {
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&id.to_le_bytes());
        let secondary = if id == 11 { 21u64 } else { 0 };
        bytes.extend_from_slice(&secondary.to_le_bytes());
        for operand in [0u32, 1, 1] {
            bytes.extend_from_slice(&operand.to_le_bytes());
        }
        bytes.extend_from_slice(&[0; 8]);
        bytes.push(1);
        bytes.extend_from_slice(&0u32.to_le_bytes());
    }
    header(&mut bytes, b"275", 103);
    bytes
}

#[test]
fn compact_profile_selection_reads_counted_whole_loop_groups() {
    let selection = parse(&compact_chain()).unwrap();
    assert!(selection.valid_layout());
    assert!(selection.compact_references());
    assert!(selection.complete_loop_groups());
    assert_eq!(selection.owner_text, "75");
    assert_eq!(selection.sketch_scope_record_index, 77);
    assert_eq!(selection.groups.len(), 2);
    assert_eq!(selection.groups[0].region[0].entries[0].primary_id, 11);
    assert_eq!(selection.groups[0].region[0].entries[0].secondary_id, 21);
    assert_eq!(selection.groups[1].region[0].entries[0].primary_id, 13);
    assert_eq!(
        selection.region_group_count_offset(),
        Some(selection.identity_record_offset + 32)
    );
}

#[test]
fn compact_profile_selection_retains_region_descendants() {
    let mut bytes = compact_chain();
    let selection = parse(&bytes).unwrap();
    let kind_at =
        usize::try_from(selection.groups[0].region[0].entries[0].primary_id_offset).unwrap() - 4;
    let further_count_at = kind_at + 41;
    bytes[further_count_at..further_count_at + 4].copy_from_slice(&1u32.to_le_bytes());
    let mut descendant = Vec::new();
    region(&mut descendant, &[17]);
    descendant[8..12].copy_from_slice(&3u32.to_le_bytes());
    bytes.splice(further_count_at + 4..further_count_at + 4, descendant);
    let selection = parse(&bytes).unwrap();
    assert!(selection.valid_layout());
    assert!(!selection.complete_loop_groups());
    assert_eq!(selection.groups[0].further.len(), 1);
    assert_eq!(selection.groups[0].further[0][0].entries[0].primary_id, 17);
}

#[test]
fn compact_profile_selection_requires_its_entry_kind_and_complete_counts() {
    let bytes = compact_chain();
    let selection = parse(&bytes).unwrap();
    let kind_at =
        usize::try_from(selection.groups[0].region[0].entries[0].primary_id_offset).unwrap() - 4;
    let mut wrong_kind = bytes.clone();
    wrong_kind[kind_at..kind_at + 4].copy_from_slice(&2u32.to_le_bytes());
    assert!(parse(&wrong_kind).is_none());
    let count_at = usize::try_from(selection.region_group_count_offset().unwrap()).unwrap();
    for count in [0u32, 1, 3, u32::MAX] {
        let mut changed = bytes.clone();
        changed[count_at..count_at + 4].copy_from_slice(&count.to_le_bytes());
        assert!(parse(&changed).is_none());
    }
    let mut wrong_companion = bytes.clone();
    let companion_at = usize::try_from(selection.identity_end_offset).unwrap();
    wrong_companion[companion_at + 7..companion_at + 11].copy_from_slice(&104u32.to_le_bytes());
    assert!(parse(&wrong_companion).is_none());
    let mut wrong_flag = bytes;
    wrong_flag[kind_at + 40] = 2;
    assert!(parse(&wrong_flag).is_none());
}

#[test]
fn compact_profile_selection_preserves_the_counted_member_reading() {
    let mut bytes = compact_chain();
    let selection = parse(&bytes).unwrap();
    let count_at = usize::try_from(selection.region_group_count_offset().unwrap()).unwrap();
    bytes[count_at..count_at + 4].copy_from_slice(&1u32.to_le_bytes());
    bytes[count_at + 4..count_at + 8].copy_from_slice(&2u32.to_le_bytes());
    let primary_at =
        usize::try_from(selection.groups[0].region[0].entries[0].primary_id_offset).unwrap();
    bytes[primary_at + 8..primary_at + 16].copy_from_slice(&0u64.to_le_bytes());
    let second_primary_at =
        usize::try_from(selection.groups[1].region[0].entries[0].primary_id_offset).unwrap();
    bytes[second_primary_at + 36] = 0;
    bytes.drain(primary_at + 37..primary_at + 45);
    let paired_at = bytes
        .windows(7)
        .position(|window| window == b"\x03\x00\x00\x00262")
        .unwrap();
    let counted =
        super::super::parse_sketch_profile_region_selection(&bytes, 100, paired_at).unwrap();
    assert_eq!(counted.regions.len(), 2);
    assert_eq!(counted.regions[0].members[0].curve_primary_id.get(), 11);
    assert_eq!(counted.regions[1].members[0].curve_primary_id.get(), 13);
    assert!(parse(&bytes).is_none());
}
