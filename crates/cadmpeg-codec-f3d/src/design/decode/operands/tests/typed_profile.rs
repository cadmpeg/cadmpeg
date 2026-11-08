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
    chain_fixture(100, inner_loop, true)
}

fn chain_fixture(base: u32, inner_loop: bool, valid: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    header(&mut bytes, b"264", base);
    bytes.extend_from_slice(&[0; 10]);
    typed_reference(&mut bytes, base + 3);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    lp_utf16(&mut bytes, "75");
    bytes.extend_from_slice(&[0; 8]);
    header(&mut bytes, b"262", base);
    header(&mut bytes, b"277", base + 1);
    bytes.extend_from_slice(&[0; 14]);
    bytes.extend_from_slice(&[u8::from(valid), 1, 1, 0, 0, 0]);
    typed_reference(&mut bytes, 77);
    header(&mut bytes, b"273", base + 2);
    bytes.extend_from_slice(&[0; 22]);
    header(&mut bytes, b"297", base + 3);
    bytes.extend_from_slice(&[0; 10]);
    typed_reference(&mut bytes, base);
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
    header(&mut bytes, b"275", base + 3);
    bytes.extend_from_slice(&[0; 8]);
    bytes
}

fn parse(
    bytes: &[u8],
) -> Option<crate::records::topology::entity_selection::DesignProfileCurveSelection> {
    let records = crate::design::test_support::test_primary_frames(bytes);
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:design-record-header#0".into(),
        record_index: 100,
        class_tag: DesignClassTag::try_from("264".to_owned()).unwrap(),
        byte_offset: Some(0),
    };
    crate::test_support::with_decode_context(|ctx| {
        super::super::parse_typed_profile_curve_selection(
            ctx,
            bytes,
            &records,
            &group(),
            0,
            &header,
        )
    })
    .unwrap()
}

#[test]
fn typed_profile_selection_reads_owner_scope_and_curve_loops() {
    let selection = parse(&chain(true)).expect("typed profile selection");
    assert_eq!(selection.owner_text, "75");
    assert_eq!(selection.sketch_scope_record_index, 77);
    let lists: Vec<Vec<Vec<u64>>> = selection
        .loop_lists
        .iter()
        .map(|loops| {
            loops
                .iter()
                .map(|region| {
                    region
                        .iter()
                        .flat_map(|loop_| &loop_.entries)
                        .map(|entry| entry.primary_id)
                        .collect()
                })
                .collect()
        })
        .collect();
    assert_eq!(lists, [vec![vec![11, 12, 13]], vec![vec![14]]]);
    assert!(selection
        .loop_lists
        .iter()
        .flatten()
        .flatten()
        .flat_map(|loop_| &loop_.entries)
        .all(|entry| entry.secondary_id == 0 && entry.entry_fields == [0, 1, 1]));
    let outer = parse(&chain(false)).expect("single-loop selection");
    assert_eq!(outer.loop_lists[0].len(), 1);
    assert!(outer.loop_lists[1].is_empty());
    assert_eq!(u64_from_index(0), outer.byte_offset);
}

#[test]
fn damaged_typed_profile_is_given_up_and_sibling_decodes() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let mut bytes = chain_fixture(100, false, false);
    let second = bytes.len();
    bytes.extend_from_slice(&chain_fixture(200, false, true));
    let headers: Vec<_> = [(100, 0), (200, second)]
        .into_iter()
        .map(|(index, start)| DesignRecordHeader {
            id: crate::ids::native_scoped_id(stream, "record", index),
            record_index: index,
            class_tag: "264".to_owned().try_into().unwrap(),
            byte_offset: Some(u64_from_index(start)),
        })
        .collect();
    let group: DesignConstructionOperandGroup = serde_json::from_value(serde_json::json!({
        "id": crate::ids::native_scoped_id(stream, "operand-group", 90),
        "scope_record_index": 80, "scope_reference_ordinal": 0,
        "record_index": 90, "class_tag": "289", "members": [100, 200],
        "role": DesignOperandRole::PROFILE.raw(), "paired_class_tag": "259",
        "frame": {"opaque_index": 1, "opaque_scalar": 0.0, "variant": false}
    }))
    .unwrap();
    let metadata = crate::test_support::zip_test::indexed_fixture_metastream(
        &bytes,
        &[100, 101, 102, 103, 200, 201, 202, 203],
        &[100, 103, 200, 203],
    );
    let archive = crate::test_support::body_reference_tail_test::archive(&bytes, &metadata);
    crate::test_support::with_decode_context(|ctx| {
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let mut parts = cadmpeg_ir::source_fidelity::parts::PartOutput::new(
                cadmpeg_ir::CodecFormat::F3d,
                ctx,
            );
            let selections = crate::design::decode::operands::decode_profile_curve_selections(
                ctx,
                &mut parts,
                scan,
                &[group],
                &headers,
            )
            .unwrap();
            assert_eq!(selections.len(), 1);
            assert_eq!(selections[0].sketch_scope_record_index, 77);
            let (_, native, _, losses) = parts.into_parts();
            assert_eq!(
                native.namespace("f3d").unwrap().arenas()["unknowns"].len(),
                1
            );
            assert_eq!(losses.len(), 1);
            assert_eq!(losses[0].code.local_code(), "record.unreadable");
        });
    });
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
        identity + crate::layout::typed_reference_profile_identity_record::FIRST_REGION_COUNT
            ..paired,
        lists,
    );
    bytes
}

#[test]
fn typed_profile_selection_reads_region_holes_before_exterior() {
    let selection = parse(&region_chain()).unwrap();
    assert!(selection.valid_layout());
    let region = &selection.loop_lists[0][0];
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
        + crate::layout::typed_reference_profile_identity_record::FIRST_REGION_COUNT
        + 8;
    bytes[count..count + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(parse(&bytes).is_none());
}
