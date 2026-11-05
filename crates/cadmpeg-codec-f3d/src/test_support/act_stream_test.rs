// SPDX-License-Identifier: Apache-2.0
//! Synthetic ACT MetaStream and BulkStream payloads.

use super::streams_test::segment_metastream;
use crate::test_support::{lp_ascii, lp_utf16};
use cadmpeg_core::decode::u64_from_index;

pub(crate) fn generated_act_metastream(records: &[(u64, u64)]) -> Vec<u8> {
    let entity_1 = [1];
    let entity_2 = [2];
    let entity_7 = [7];
    let entity_9 = records
        .iter()
        .map(|&(id, _)| id)
        .filter(|id| ![1, 2, 7].contains(id))
        .collect::<Vec<_>>();
    let types = [
        (
            "00000000-0000-0000-0000-000000000100",
            "",
            1,
            "ACT",
            &entity_2[..],
        ),
        (
            "00000000-0000-0000-0000-000000000101",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000102",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000103",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000104",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000105",
            "",
            1,
            "ACT",
            &entity_7[..],
        ),
        (
            "00000000-0000-0000-0000-000000000106",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000107",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000108",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-000000000109",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-00000000010a",
            "",
            1,
            "ACT",
            &[][..],
        ),
        (
            "00000000-0000-0000-0000-00000000010b",
            "",
            1,
            "ACT",
            &entity_9[..],
        ),
        (
            "00000000-0000-0000-0000-00000000010c",
            "",
            1,
            "ACT",
            &entity_1[..],
        ),
    ];
    segment_metastream(
        "FusionACTSegmentType",
        "FusionACTSegmentType",
        "Fusion",
        &types,
        records,
    )
}

pub(crate) fn generated_act_bulkstream() -> (Vec<u8>, Vec<(u64, u64)>) {
    let mut out = Vec::new();
    let mut records = vec![(2, u64_from_index(out.len()))];
    lp_ascii(&mut out, "256");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(b"decoy:ACTTable");
    out.extend_from_slice(&[0; 6]);
    records.push((1, u64_from_index(out.len())));
    lp_ascii(&mut out, "268");
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    lp_ascii(&mut out, "ACTTable");
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.push(1);
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 6]);
    lp_utf16(&mut out, "0_985");
    lp_utf16(&mut out, "eeeeeeee-1111-2222-3333-ffffffffffff");
    out.extend_from_slice(&1u32.to_le_bytes());
    out.push(1);
    out.extend_from_slice(&9u32.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
    out.extend_from_slice(&2u32.to_le_bytes());
    for (name, guid) in [
        ("Appearance", "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb"),
        ("PhysicalMaterial", "cccccccc-1111-2222-3333-dddddddddddd"),
    ] {
        lp_ascii(&mut out, name);
        lp_utf16(&mut out, guid);
    }
    records.push((9, u64_from_index(out.len())));
    lp_ascii(&mut out, "267");
    out.extend_from_slice(&9u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 10]);
    out.push(1);
    out.extend_from_slice(&12u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 6]);
    lp_utf16(&mut out, "0_3");
    out.push(1);
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 5]);
    out.push(1);
    out.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut out, "(Unsaved)");
    out.push(0);
    out.push(1);
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 6]);
    records.push((7, u64_from_index(out.len())));
    lp_ascii(&mut out, "261");
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 10]);
    out.extend_from_slice(&2u32.to_le_bytes());
    for (name, guid) in [
        ("Appearance", "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb"),
        ("PhysicalMaterial", "cccccccc-1111-2222-3333-dddddddddddd"),
    ] {
        lp_ascii(&mut out, name);
        lp_utf16(&mut out, guid);
    }
    lp_utf16(&mut out, "0_985");
    (out, records)
}
