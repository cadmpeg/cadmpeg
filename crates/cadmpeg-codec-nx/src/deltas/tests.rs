// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::default_trait_access)]
// Fixture builders used Vec::with_capacity and iter::repeat_n while this suite
// lived at crate root; deltas.rs denies those methods for production.
#![allow(clippy::disallowed_methods)]

use crate::test_support::*;

pub(crate) fn deltas_body_revision(node_id: u32) -> Vec<u8> {
    let mut revision = Vec::with_capacity(32);
    revision.extend_from_slice(&12u16.to_be_bytes());
    revision.extend_from_slice(&3u16.to_be_bytes());
    revision.extend_from_slice(&node_id.to_be_bytes());
    for _ in 0..8 {
        revision.extend_from_slice(&0u16.to_be_bytes());
        revision.push(1);
    }
    revision
}

pub(crate) fn deltas_point(xmt: u16, x: f64) -> Vec<u8> {
    let mut point = status_framed_deltas_point_stream();
    point[2..4].copy_from_slice(&xmt.to_be_bytes());
    point[20..28].copy_from_slice(&x.to_be_bytes());
    point
}

#[test]
fn deltas_walks_complete_status_prefixed_entity_51_records() {
    let mut stream = vec![0, 81];
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&10u16.to_be_bytes());
    stream.extend_from_slice(&2u32.to_be_bytes());
    stream.extend_from_slice(&0x21u16.to_be_bytes());
    for (status, reference) in [1, 1, 0, 1, 0, 1].into_iter().zip(3..=8u16) {
        stream.push(status);
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream.push(0);
    let entity_len = stream.len();
    stream.extend(status_framed_deltas_point_stream());

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.records.len(), 2);
    assert_eq!(census.records[0].kind(), 81);
    assert_eq!(census.records[0].xmt, 10);
    assert_eq!(census.records[0].node_id(), None);
    assert_eq!(census.records[0].references, [3, 4, 5, 6, 7, 8]);
    assert_eq!(census.records[0].end, entity_len);
    assert_eq!(census.full_counts["ENTITY_51"], 1);
    assert_eq!(census.bytes_decoded, stream.len());
    let residual = crate::deltas::semantic_residual(&stream);
    let retained = crate::parasolid::entity_51_records(&residual);
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].xmt, 10);
    assert!(residual[..stream.len()].iter().all(|byte| *byte == 0xff));

    stream[entity_len - 1] = 1;
    assert_eq!(
        crate::deltas::walk(&stream)
            .records
            .iter()
            .filter(|record| record.kind() == 81)
            .count(),
        1
    );
    stream[entity_len - 1] = 2;
    assert!(crate::deltas::walk(&stream)
        .records
        .iter()
        .all(|record| record.kind() != 81));
}

#[test]
fn deltas_walks_attribute_records_that_share_a_terminal_zero() {
    let mut stream = vec![0, 84];
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&9u16.to_be_bytes());
    stream.extend_from_slice(b"a\0");
    stream.push(81);
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&10u16.to_be_bytes());
    stream.extend_from_slice(&2u32.to_be_bytes());
    stream.extend_from_slice(&0x21u16.to_be_bytes());
    for (status, reference) in [1, 1, 0, 1, 0, 1].into_iter().zip(3..=8u16) {
        stream.push(status);
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream.push(0);
    stream.push(82);
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&11u16.to_be_bytes());
    stream.extend_from_slice(&12u32.to_be_bytes());

    let census = crate::deltas::walk(&stream);

    assert_eq!(
        census
            .records
            .iter()
            .map(|record| record.kind())
            .collect::<Vec<_>>(),
        [84, 81, 82]
    );
    assert_eq!(census.records[1].offset, census.records[0].end - 1);
    assert_eq!(census.records[2].offset, census.records[1].end - 1);
    assert_eq!(census.bytes_decoded, stream.len());
    assert!(crate::deltas::semantic_residual(&stream)[..stream.len()]
        .iter()
        .all(|byte| *byte == 0xff));
}

#[test]
fn deltas_walks_fixed_record_that_shares_a_terminal_zero() {
    let mut stream = vec![0, 84];
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&9u16.to_be_bytes());
    stream.extend_from_slice(b"a\0");
    let point = status_framed_deltas_point_stream();
    stream.extend_from_slice(&point[1..]);

    let census = crate::deltas::walk(&stream);

    assert_eq!(
        census
            .records
            .iter()
            .map(|record| record.kind())
            .collect::<Vec<_>>(),
        [84, 29]
    );
    assert_eq!(census.records[1].offset, census.records[0].end - 1);
    assert_eq!(census.bytes_decoded, stream.len());
}

#[test]
fn deltas_fixed_records_share_a_terminal_zero_with_their_successor() {
    let mut stream = Vec::new();
    stream.extend_from_slice(&13u16.to_be_bytes());
    stream.extend_from_slice(&47u16.to_be_bytes());
    stream.extend_from_slice(&61u32.to_be_bytes());
    for (reference, status) in [1u16, 2, 1, 3, 1, 1, 4, 3]
        .into_iter()
        .zip([1, 1, 1, 1, 1, 1, 1, 0])
    {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(status);
    }
    let intersection = status_framed_deltas_intersection_stream();
    stream.extend_from_slice(&intersection[1..]);

    let census = crate::deltas::walk(&stream);

    assert_eq!(
        census
            .records
            .iter()
            .map(|record| record.kind())
            .collect::<Vec<_>>(),
        [13, 38]
    );
    assert_eq!(census.records[1].offset, census.records[0].end - 1);
    assert_eq!(census.bytes_decoded, stream.len());
}

#[test]
fn deltas_type_101_record_takes_precedence_over_an_overlapping_fixed_candidate() {
    let mut type_101 = vec![0, 101];
    type_101.extend_from_slice(&2u16.to_be_bytes());
    for reference in 3u16..15 {
        type_101.extend_from_slice(&reference.to_be_bytes());
        type_101.push(1);
    }
    type_101.push(1);
    type_101.extend_from_slice(&[0; 12]);
    for reference in 15u16..18 {
        type_101.extend_from_slice(&reference.to_be_bytes());
        type_101.push(1);
    }

    let mut stream = 13u16.to_be_bytes().to_vec();
    stream.extend(encoded_xmt(256));
    stream.extend_from_slice(&1u32.to_be_bytes());
    for reference in [1u32, 2, 1, 3, 1, 1, 4] {
        stream.extend(encoded_xmt(reference));
        stream.push(1);
    }
    stream.extend_from_slice(&type_101[..3]);
    stream.extend_from_slice(&type_101[3..]);

    let census = crate::deltas::walk(&stream);

    assert_eq!(
        census
            .records
            .iter()
            .map(|record| record.kind())
            .collect::<Vec<_>>(),
        [101]
    );
    assert_eq!(census.records[0].offset, 29);
    assert_eq!(census.bytes_decoded, type_101.len());
}

#[test]
fn deltas_does_not_share_a_consecutive_reference_byte() {
    let mut stream = vec![0, 81];
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&10u16.to_be_bytes());
    stream.extend_from_slice(&2u32.to_be_bytes());
    stream.extend_from_slice(&0x21u16.to_be_bytes());
    for reference in [3u16, 4, 5, 6, 7, 256] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    let point = status_framed_deltas_point_stream();
    stream.extend_from_slice(&point[1..]);

    let census = crate::deltas::walk(&stream);

    assert_eq!(
        census
            .records
            .iter()
            .map(|record| record.kind())
            .collect::<Vec<_>>(),
        [81]
    );
}

#[test]
fn deltas_walks_complete_entity_value_records() {
    let mut stream = vec![0, 82];
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&20u16.to_be_bytes());
    stream.extend_from_slice(&u32::MAX.to_be_bytes());
    stream.extend_from_slice(&[0, 83, 0xff]);
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&21u16.to_be_bytes());
    stream.extend_from_slice(&0.25f64.to_be_bytes());
    stream.extend_from_slice(&[0, 84]);
    stream.extend_from_slice(&3u32.to_be_bytes());
    stream.extend_from_slice(&22u16.to_be_bytes());
    stream.extend_from_slice(b"abc\0");
    let decoded_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc, 0xba]);

    let census = crate::deltas::walk(&stream);
    assert_eq!(
        census
            .records
            .iter()
            .map(|record| record.kind())
            .collect::<Vec<_>>(),
        [82, 83, 84]
    );
    assert_eq!(census.full_counts["ENTITY_52"], 1);
    assert_eq!(census.full_counts["ENTITY_53"], 1);
    assert_eq!(census.full_counts["ENTITY_54"], 1);
    assert_eq!(census.bytes_decoded, decoded_len);

    let residual = crate::deltas::semantic_residual(&stream);
    assert!(residual[..decoded_len].iter().all(|byte| *byte == 0xff));
    assert_eq!(&residual[decoded_len..stream.len()], &[0xfe, 0xdc, 0xba]);
    let value_records = crate::parasolid::entity_value_records(&residual);
    assert_eq!(value_records.integers[0].values, [u32::MAX]);
    assert_eq!(value_records.doubles[0].values, [0.25]);
    assert_eq!(value_records.strings[0].value, "abc");
}

#[test]
fn deltas_walks_every_transformable_value_family() {
    let mut stream = Vec::new();
    for (kind, count, width) in [(85u8, 1u32, 24usize), (86, 1, 24), (87, 2, 24)] {
        stream.extend_from_slice(&[0, kind]);
        stream.extend_from_slice(&count.to_be_bytes());
        stream.extend_from_slice(&u16::from(kind).to_be_bytes());
        stream.resize(stream.len() + usize::try_from(count).unwrap() * width, 0);
    }
    stream.extend_from_slice(&[0, 88]);
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&88u16.to_be_bytes());
    stream.extend_from_slice(&7u32.to_be_bytes());
    stream.extend_from_slice(&[0, 89]);
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&89u16.to_be_bytes());
    stream.resize(stream.len() + 24, 0);
    stream.extend_from_slice(&[0, 98]);
    stream.extend_from_slice(&2u32.to_be_bytes());
    stream.extend_from_slice(&98u16.to_be_bytes());
    stream.extend_from_slice(&[0, b'N', 0, b'X']);
    let census = crate::deltas::walk(&stream);

    assert_eq!(
        census
            .records
            .iter()
            .map(|record| record.kind())
            .collect::<Vec<_>>(),
        [85, 86, 87, 88, 89, 98]
    );
    for family in [
        "ENTITY_55",
        "ENTITY_56",
        "ENTITY_57",
        "ENTITY_58",
        "ENTITY_59",
    ] {
        assert_eq!(census.full_counts[family], 1);
    }
    assert_eq!(census.full_counts["ENTITY_62"], 1);

    let values = crate::parasolid::entity_value_records_at(
        &stream,
        census.records.iter().map(|record| record.offset),
    );
    assert_eq!(values.unicode[0].value, "NX");
}

#[test]
fn deltas_does_not_resynchronize_at_an_unowned_value_marker() {
    let stream = [0xfe, 0x00, 0x62, 0, 0, 0, 2, 0, 98, 0, b'N', 0, b'X'];
    let census = crate::deltas::walk(&stream);
    assert!(census.records.is_empty());
    assert!(census.tombstones.is_empty());
    assert!(!census.full_counts.contains_key("ENTITY_62"));
}

#[test]
fn deltas_admits_a_value_owned_by_a_unique_entity_reference() {
    let stream = [
        0xfe, 0x00, 0x52, 0, 0, 0, 1, 0, 20, 0, 0, 0, 7, 0, 0x51, 0, 0, 0, 1, 0, 41, 0, 0, 0, 1, 0,
        33, 0, 20, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1,
    ];
    let census = crate::deltas::walk(&stream);
    assert_eq!(
        census
            .records
            .iter()
            .map(|record| record.kind())
            .collect::<Vec<_>>(),
        [82, 81]
    );
}

#[test]
fn deltas_walks_complete_type_91_records() {
    fn record(escape: bool, xmt: u32, flag: u32) -> Vec<u8> {
        let mut bytes = vec![0, 91];
        if escape {
            bytes.push(0xff);
        }
        bytes.extend(encoded_xmt(xmt));
        bytes.extend_from_slice(&flag.to_be_bytes());
        for (reference, status) in [(3u16, 1u8), (4, 1), (5, 0), (6, 1), (7, 0), (8, 0)] {
            bytes.extend_from_slice(&reference.to_be_bytes());
            bytes.push(status);
        }
        bytes
    }

    let direct = record(false, 10, 0);
    let escaped = record(true, 11, 1);
    let zero_flag_escaped = record(true, 12, 0);
    let escaped_with_null_tail = vec![
        0, 91, 0xff, 1, 89, 0, 0, 0, 0, 0, 202, 1, 1, 88, 1, 1, 90, 1, 1, 41, 1, 0, 1, 1, 0, 1, 1,
    ];
    let mut stream = direct.clone();
    stream.extend_from_slice(&escaped);
    stream.extend_from_slice(&zero_flag_escaped);
    stream.extend_from_slice(&escaped_with_null_tail);
    let record_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.records.len(), 4);
    assert_eq!(census.records[0].kind(), 91);
    assert_eq!(census.records[0].xmt, 10);
    assert_eq!(census.records[0].node_id(), None);
    assert_eq!(census.records[0].references, [3, 4, 5, 6, 7, 8]);
    assert_eq!(census.records[0].canonical_bytes, direct);
    assert_eq!(census.records[1].xmt, 11);
    assert_eq!(census.records[1].canonical_bytes, escaped);
    assert_eq!(census.records[2].canonical_bytes, zero_flag_escaped);
    assert_eq!(census.records[3].canonical_bytes, escaped_with_null_tail);
    assert_eq!(census.full_counts["TYPE_91"], 4);
    assert_eq!(census.bytes_decoded, record_len);

    let residual = crate::deltas::semantic_residual(&stream);
    assert!(residual[..record_len].iter().all(|byte| *byte == 0xff));
    assert_eq!(&residual[record_len..stream.len()], &[0xfe, 0xdc]);
    assert!(residual.ends_with(&stream[..record_len]));

    let mut invalid = direct;
    invalid[4..8].copy_from_slice(&2u32.to_be_bytes());
    assert!(crate::deltas::walk(&invalid).records.is_empty());
    invalid[4..8].copy_from_slice(&0u32.to_be_bytes());
    invalid[10] = 2;
    assert!(crate::deltas::walk(&invalid).records.is_empty());
}

#[test]
fn deltas_walks_complete_group_records() {
    let mut direct = vec![0, 90, 0xff, 0xfe, 0, 1];
    direct.extend_from_slice(&7u32.to_be_bytes());
    for reference in [3u16, 4, 5, 6] {
        direct.extend_from_slice(&reference.to_be_bytes());
        direct.push(1);
    }
    direct.push(4);
    direct.extend_from_slice(&8u16.to_be_bytes());
    direct.push(0);
    let direct_len = direct.len();
    direct.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&direct);
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind(), 90);
    assert_eq!(census.records[0].xmt, 32_769);
    assert_eq!(census.records[0].node_id(), Some(7));
    assert_eq!(census.records[0].references, [3, 4, 5, 6, 8]);
    assert_eq!(census.records[0].canonical_bytes, direct[..direct_len]);
    assert_eq!(census.full_counts["GROUP"], 1);
    assert_eq!(census.bytes_decoded, direct_len);

    let residual = crate::deltas::semantic_residual(&direct);
    assert!(residual[..direct_len].iter().all(|byte| *byte == 0xff));
    assert_eq!(&residual[direct_len..], &[0xfe, 0xdc]);

    let mut escaped = vec![0, 90, 0xff];
    escaped.extend_from_slice(&10u16.to_be_bytes());
    escaped.extend_from_slice(&11u32.to_be_bytes());
    for reference in [3u16, 4, 5, 6] {
        escaped.extend_from_slice(&reference.to_be_bytes());
        escaped.push(1);
    }
    escaped.push(9);
    escaped.extend_from_slice(&8u16.to_be_bytes());
    escaped.push(1);
    assert_eq!(crate::deltas::walk(&escaped).records[0].xmt, 10);

    escaped[11] = 0;
    assert!(crate::deltas::walk(&escaped).records.is_empty());
    escaped[11] = 1;
    escaped[21] = 3;
    assert!(crate::deltas::walk(&escaped).records.is_empty());
    escaped[21] = 9;
    escaped[24] = 2;
    assert!(crate::deltas::walk(&escaped).records.is_empty());
}

#[test]
fn deltas_walks_group_records_without_leading_statuses() {
    let mut direct = vec![0, 90];
    direct.extend_from_slice(&10u16.to_be_bytes());
    direct.extend_from_slice(&11u32.to_be_bytes());
    direct.extend_from_slice(
        &[3, 4, 5, 6]
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>(),
    );
    direct.push(4);
    direct.extend_from_slice(&8u16.to_be_bytes());
    direct.push(0);
    let direct_len = direct.len();
    direct.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&direct);
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind(), 90);
    assert_eq!(census.records[0].xmt, 10);
    assert_eq!(census.records[0].node_id(), Some(11));
    assert_eq!(census.records[0].references, [3, 4, 5, 6, 8]);
    assert_eq!(census.records[0].canonical_bytes, direct[..direct_len]);
    assert_eq!(census.records[0].end, direct_len);
    assert_eq!(census.full_counts["GROUP"], 1);
    assert_eq!(census.bytes_decoded, direct_len);

    let residual = crate::deltas::semantic_residual(&direct);
    assert!(residual[..direct_len].iter().all(|byte| *byte == 0xff));
    assert_eq!(&residual[direct_len..], &[0xfe, 0xdc]);

    let mut escaped = vec![0, 90, 0xff];
    escaped.extend_from_slice(&10u16.to_be_bytes());
    escaped.extend_from_slice(&11u32.to_be_bytes());
    escaped.extend_from_slice(
        &[3, 4, 5, 6]
            .into_iter()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>(),
    );
    escaped.push(9);
    escaped.extend_from_slice(&8u16.to_be_bytes());
    escaped.push(1);

    let census = crate::deltas::walk(&escaped);
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].xmt, 10);
    assert_eq!(census.records[0].references, [3, 4, 5, 6, 8]);
    assert_eq!(census.records[0].end, escaped.len());
}

#[test]
fn deltas_walks_complete_attdef_lists() {
    let mut direct = vec![0, 74];
    direct.extend_from_slice(&3u32.to_be_bytes());
    direct.extend_from_slice(&10u16.to_be_bytes());
    direct.extend_from_slice(&2u32.to_be_bytes());
    direct.extend_from_slice(&0u32.to_be_bytes());
    for reference in [1u16, 20, 21, 1] {
        direct.extend_from_slice(&reference.to_be_bytes());
        direct.push(1);
    }
    let direct_len = direct.len();
    direct.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&direct);
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind(), 74);
    assert_eq!(census.records[0].xmt, 10);
    assert_eq!(census.records[0].node_id(), None);
    assert_eq!(census.records[0].references, [1, 20, 21, 1]);
    assert_eq!(census.records[0].canonical_bytes, direct[..direct_len]);
    assert_eq!(census.full_counts["ATTDEF_LIST"], 1);
    assert_eq!(census.bytes_decoded, direct_len);

    let residual = crate::deltas::semantic_residual(&direct);
    assert!(residual[..direct_len].iter().all(|byte| *byte == 0xff));
    assert_eq!(&residual[direct_len..], &[0xfe, 0xdc]);

    let mut escaped = vec![0, 74, 0xff];
    escaped.extend_from_slice(&2u32.to_be_bytes());
    escaped.extend_from_slice(&11u16.to_be_bytes());
    escaped.extend_from_slice(&1u32.to_be_bytes());
    escaped.extend_from_slice(&0u32.to_be_bytes());
    for reference in [1u16, 30, 1] {
        escaped.extend_from_slice(&reference.to_be_bytes());
        escaped.push(1);
    }
    assert_eq!(crate::deltas::walk(&escaped).records[0].xmt, 11);

    escaped[9..13].copy_from_slice(&3u32.to_be_bytes());
    assert!(crate::deltas::walk(&escaped).records.is_empty());
    escaped[9..13].copy_from_slice(&1u32.to_be_bytes());
    escaped[20..22].copy_from_slice(&1u16.to_be_bytes());
    assert!(crate::deltas::walk(&escaped).records.is_empty());
}

#[test]
fn deltas_walks_complete_type_101_records() {
    let mut direct = vec![0, 101];
    direct.extend_from_slice(&2u16.to_be_bytes());
    for reference in 3u16..15 {
        direct.extend_from_slice(&reference.to_be_bytes());
        direct.push(1);
    }
    direct.push(1);
    direct.extend_from_slice(&[0; 12]);
    for reference in 15u16..18 {
        direct.extend_from_slice(&reference.to_be_bytes());
        direct.push(1);
    }
    let direct_len = direct.len();
    direct.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&direct);
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind(), 101);
    assert_eq!(census.records[0].xmt, 2);
    assert_eq!(census.records[0].node_id(), None);
    assert_eq!(census.records[0].references, (3u32..18).collect::<Vec<_>>());
    assert_eq!(census.records[0].canonical_bytes, direct[..direct_len]);
    assert_eq!(census.full_counts["TYPE_101"], 1);
    assert_eq!(census.bytes_decoded, direct_len);

    let residual = crate::deltas::semantic_residual(&direct);
    assert!(residual[..direct_len].iter().all(|byte| *byte == 0xff));
    assert_eq!(&residual[direct_len..], &[0xfe, 0xdc]);

    let mut escaped = vec![0, 101, 0xff];
    escaped.extend_from_slice(&2u16.to_be_bytes());
    for reference in 3u16..15 {
        escaped.extend_from_slice(&reference.to_be_bytes());
        escaped.push(1);
    }
    escaped.push(1);
    escaped.extend_from_slice(&[0; 12]);
    for reference in 15u16..18 {
        escaped.extend_from_slice(&reference.to_be_bytes());
        escaped.push(1);
    }
    assert_eq!(crate::deltas::walk(&escaped).records[0].xmt, 2);

    escaped[41] = 0;
    assert!(crate::deltas::walk(&escaped).records.is_empty());
    escaped[41] = 1;
    escaped[42] = 1;
    assert!(crate::deltas::walk(&escaped).records.is_empty());
}

#[test]
fn deltas_walks_auxiliary_family_tombstones() {
    let mut stream = Vec::new();
    for kind in [41u16, 45, 125, 136, 141, 204] {
        stream.extend_from_slice(&kind.to_be_bytes());
        stream.extend_from_slice(&(-2i16).to_be_bytes());
        stream.extend_from_slice(&1u16.to_be_bytes());
    }

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.records.len(), 0);
    assert_eq!(census.tombstones.len(), 6);
    assert!(census
        .tombstones
        .iter()
        .all(|tombstone| tombstone.xmt == 32_769));
    for family in [
        "TERM_USE",
        "TYPE_45",
        "B_SURFACE_DATA",
        "B_CURVE_DESCRIPTOR",
        "TYPE_141",
        "SUPPORT_UV",
    ] {
        assert_eq!(census.tombstone_counts[family], 1);
    }
    assert_eq!(census.bytes_decoded, stream.len());
    assert!(crate::deltas::semantic_residual(&stream)
        .iter()
        .all(|byte| *byte == 0xff));
}

#[test]
fn deltas_term_use_numeric_tails_follow_the_declared_endpoint_count() {
    fn term_use(count: u32, xmt: u16, form: [u8; 2], value_count: usize) -> Vec<u8> {
        let mut bytes = 41u16.to_be_bytes().to_vec();
        bytes.extend_from_slice(&count.to_be_bytes());
        bytes.extend_from_slice(&xmt.to_be_bytes());
        bytes.extend_from_slice(&form);
        for coordinate in [1.0f64, 2.0, 3.0] {
            bytes.extend_from_slice(&coordinate.to_be_bytes());
        }
        for ordinal in 0..value_count {
            bytes.extend_from_slice(&(ordinal as f64 + 0.25).to_be_bytes());
        }
        bytes
    }

    let first = term_use(1, 20, *b"L?", 8);
    let second = term_use(2, 21, *b"TF", 19);
    let mut stream = first.clone();
    stream.extend_from_slice(&second);
    let census = crate::deltas::walk(&stream);

    assert_eq!(census.records.len(), 2);
    assert_eq!(census.term_use_numeric_tails.len(), 2);
    assert_eq!(census.term_use_numeric_tails[0].term_use_xmt, 20);
    assert_eq!(census.term_use_numeric_tails[0].term_use_count, 1);
    assert_eq!(census.term_use_numeric_tails[0].values.len(), 8);
    assert_eq!(census.term_use_numeric_tails[1].term_use_xmt, 21);
    assert_eq!(census.term_use_numeric_tails[1].term_use_count, 2);
    assert_eq!(census.term_use_numeric_tails[1].values.len(), 19);
    assert_eq!(census.bytes_decoded, stream.len());

    let mut nonfinite = term_use(1, 22, *b"L?", 8);
    nonfinite[34..42].copy_from_slice(&f64::NAN.to_be_bytes());
    let census = crate::deltas::walk(&nonfinite);
    assert_eq!(census.records.len(), 1);
    assert!(census.term_use_numeric_tails.is_empty());
    assert_eq!(census.bytes_decoded, 34);
}

#[test]
fn deltas_tagged_reference_lanes_require_complete_known_kind_and_xmt_pairs() {
    let stream = [
        0x00, 0x4f, 0x00, 0x0a, // direct type-79 reference
        0x00, 0x50, 0xff, 0xff, 0x00, 0x01, // extended type-80 reference
    ];
    let census = crate::deltas::walk(&stream);
    assert_eq!(census.tagged_reference_lanes.len(), 1);
    assert_eq!(
        census.tagged_reference_lanes[0].references,
        [(79, 10), (80, 32_768)]
    );
    assert_eq!(census.tagged_reference_lanes[0].offset, 0);
    assert_eq!(census.tagged_reference_lanes[0].end, stream.len());
    assert_eq!(census.bytes_decoded, stream.len());

    for invalid in [
        &[0x00, 0x4e, 0x00, 0x0a][..],
        &[0x00, 0x4f, 0x00, 0x01],
        &[0x00, 0x50, 0xff, 0xff, 0x00],
    ] {
        assert!(crate::deltas::walk(invalid)
            .tagged_reference_lanes
            .is_empty());
    }
}

#[test]
fn deltas_point_normalizes_to_partition_record_framing() {
    let record = crate::deltas::walk(&status_framed_deltas_point_stream())
        .records
        .remove(0);
    let mut expected = crate::test_support::record(29, 40);
    put_ref(&mut expected, 2, 50);
    expected[4..8].copy_from_slice(&900u32.to_be_bytes());
    for at in [8, 10, 12, 14] {
        put_ref(&mut expected, at, 1);
    }
    put_vec3(&mut expected, 16, [0.0125, -0.002, 0.004]);
    assert_eq!(record.canonical_bytes, expected);
}

#[test]
fn deltas_intersection_normalizes_during_full_record_merge() {
    let mut stream = status_framed_deltas_intersection_stream();
    stream[10] = 0;
    let record_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);
    let census = crate::deltas::walk(&stream);
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind(), 38);
    assert_eq!(census.bytes_decoded, record_len);

    let merged = crate::deltas::merge_full_records(&[], &stream);
    let intersections = crate::topology::composite_curves(&merged);
    assert_eq!(intersections.len(), 1);
    assert_eq!(intersections[0].xmt, 12);
    assert_eq!(intersections[0].references, [6, 7, 20, 21, 22, 23]);
}

#[test]
fn merge_replaces_a_partition_intersection_by_exact_xmt() {
    let partition = charted_intersection_curve_topology_partition_stream();
    let mut replacement = status_framed_deltas_intersection_stream();
    let sense = replacement
        .iter()
        .position(|byte| *byte == b'+')
        .expect("intersection sense");
    replacement[sense] = b'-';

    let merged = crate::deltas::merge_full_records(&partition, &replacement);
    let [intersection] = crate::topology::composite_curves(&merged)
        .try_into()
        .expect("one current intersection");

    assert_eq!(intersection.xmt, 12);
    assert!(!intersection.sense);
}

#[test]
fn deltas_walks_complete_single_byte_intersection_data_records() {
    let mut stream = crate::topology::TYPE_38_SCHEMA_HEADER.to_vec();
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&7u32.to_be_bytes());
    for reference in [1u16, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'-');
    for reference in [6u16, 7] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    for reference in [15u16, 14, 13] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(0);
    }
    stream.extend_from_slice(&[0, 1, 1]);
    let schema_end = stream.len();
    stream.extend_from_slice(&[0xa5; 100]);

    let record_offset = stream.len();
    stream.extend_from_slice(&[0x5a]);
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&7u32.to_be_bytes());
    for reference in [1u16, 2, 3, 4, 5] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream.push(b'+');
    for reference in [6u16, 6, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    let record_end = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind(), 90);
    assert_eq!(
        crate::deltas::record_family_name(&census.records[0]),
        "INTERSECTION_DATA"
    );
    assert_eq!(census.records[0].xmt, 12);
    assert_eq!(census.records[0].offset, record_offset);
    assert_eq!(
        census.records[0].references,
        [1, 2, 3, 4, 5, 6, 6, 1, 1, 1, 1]
    );
    assert_eq!(
        census.records[0].canonical_bytes,
        stream[record_offset..record_end]
    );
    assert_eq!(census.full_counts["INTERSECTION_DATA"], 1);
    assert_eq!(
        census.bytes_decoded,
        schema_end + (record_end - record_offset)
    );
    let curves = crate::topology::intersection_data_curves(&stream);
    assert_eq!(curves.len(), 1);
    assert_eq!(curves[0].references, [6, 6, 1, 1, 1, 1]);

    let residual = crate::deltas::semantic_residual(&stream);
    assert!(residual[record_offset..record_end]
        .iter()
        .all(|byte| *byte == 0xff));
    let prefix_len = crate::topology::TYPE_38_SCHEMA_HEADER.len() - 1;
    let appended_start = residual.len() - prefix_len - (record_end - record_offset);
    assert_eq!(
        &residual[appended_start..appended_start + prefix_len],
        &crate::topology::TYPE_38_SCHEMA_HEADER[..prefix_len]
    );
    assert_eq!(
        &residual[appended_start + prefix_len..],
        &stream[record_offset..record_end]
    );
}

#[test]
fn semantic_residual_does_not_reemit_historical_intersection_data() {
    let mut stream = deltas_intersection_curve_stream();
    stream.extend_from_slice(&deltas_body_revision(2));

    let residual = crate::deltas::semantic_residual(&stream);

    assert_eq!(residual.len(), stream.len());
}

#[test]
fn deltas_rejects_single_byte_intersection_data_before_its_schema_anchor() {
    let mut stream = vec![0x5a];
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&7u32.to_be_bytes());
    for reference in [1u16, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream.push(b'+');
    for reference in [6u16, 6, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }

    let census = crate::deltas::walk(&stream);
    assert!(census.records.iter().all(|record| record.kind() != 90));
    assert!(!census.full_counts.contains_key("INTERSECTION_DATA"));
    assert!(crate::topology::intersection_data_curves(&stream).is_empty());
}

#[test]
fn deltas_rejects_denormal_topology_tolerance_payload_coincidences() {
    fn edge(tolerance: f64) -> Vec<u8> {
        let mut bytes = 16u16.to_be_bytes().to_vec();
        bytes.extend(encoded_xmt(20));
        bytes.extend_from_slice(&1u32.to_be_bytes());
        bytes.extend(encoded_xmt(1));
        bytes.push(1);
        bytes.extend_from_slice(&tolerance.to_be_bytes());
        for reference in [2u32, 3, 4, 5, 6, 7, 8] {
            bytes.extend(encoded_xmt(reference));
            bytes.push(1);
        }
        bytes
    }

    let valid = edge(1.0e-8);
    let census = crate::deltas::walk(&valid);
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind(), 16);
    assert_eq!(census.bytes_decoded, valid.len());

    let denormal = edge(1.0e-120);
    assert!(crate::deltas::walk(&denormal).records.is_empty());

    let mut vertex = 18u16.to_be_bytes().to_vec();
    vertex.extend(encoded_xmt(20));
    vertex.extend_from_slice(&1u32.to_be_bytes());
    for reference in [2u32, 3, 4, 5, 6] {
        vertex.extend(encoded_xmt(reference));
        vertex.push(1);
    }
    let tolerance_at = vertex.len();
    vertex.extend_from_slice(&1.0e-8f64.to_be_bytes());
    vertex.extend(encoded_xmt(7));
    vertex.push(1);

    let census = crate::deltas::walk(&vertex);
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind(), 18);

    vertex[tolerance_at..tolerance_at + 8].copy_from_slice(&1.0e-120f64.to_be_bytes());
    assert!(crate::deltas::walk(&vertex).records.is_empty());
}

#[test]
fn deltas_rejects_denormal_point_payload_coincidences() {
    let mut point = status_framed_deltas_point_stream();
    let position = point.len() - 24;
    for (ordinal, value) in [f64::from_bits(1), f64::from_bits(2), f64::from_bits(3)]
        .into_iter()
        .enumerate()
    {
        point[position + ordinal * 8..position + (ordinal + 1) * 8]
            .copy_from_slice(&value.to_be_bytes());
    }
    assert!(crate::deltas::walk(&point)
        .records
        .iter()
        .all(|record| record.kind() != 29));

    point[position..position + 8].copy_from_slice(&1.0e-200f64.to_be_bytes());
    point[position + 8..].fill(0);
    assert_eq!(crate::deltas::walk(&point).full_counts["POINT"], 1);
}

#[test]
fn deltas_walks_complete_intersection_auxiliary_records() {
    let source = ext11_charted_intersection_curve_stream();
    let blend_source = blend_bound_charted_intersection_curve_stream();
    let chart_pos = crate::intersection::chart_source_records(
        &source,
        crate::intersection::ChartPointLayout::Ext11,
    )[0]
    .pos;
    let (_, chart_end) = crate::intersection::chart_source_record_at(
        &source,
        chart_pos,
        crate::intersection::ChartPointLayout::Ext11,
    )
    .expect("chart");
    let term_pos = crate::intersection::term_use_records(&source)[0].pos;
    let (_, term_end) = crate::intersection::term_use_at(&source, term_pos).expect("term use");
    let support_uv_pos = crate::intersection::support_uv_records(&source)[0].pos;
    let (_, support_uv_end) =
        crate::intersection::support_uv_record_at(&source, support_uv_pos).expect("support UV");
    let blend_bound_pos = crate::intersection::blend_bounds(&blend_source)[0].pos;
    let (_, blend_bound_end) =
        crate::intersection::blend_bound_at(&blend_source, blend_bound_pos).expect("blend bound");

    for (bytes, kind, family) in [
        (&source[chart_pos..chart_end], 40, "CHART"),
        (&source[term_pos..term_end], 41, "TERM_USE"),
        (
            &blend_source[blend_bound_pos..blend_bound_end],
            59,
            "BLEND_BOUND",
        ),
        (&source[support_uv_pos..support_uv_end], 204, "SUPPORT_UV"),
    ] {
        let mut stream = bytes.to_vec();
        stream.extend_from_slice(&[0xfe, 0xdc]);
        let census = crate::deltas::walk(&stream);
        assert_eq!(census.records.len(), 1);
        assert_eq!(census.records[0].kind(), kind);
        assert_eq!(census.records[0].canonical_bytes, bytes);
        assert_eq!(census.full_counts[family], 1);
        assert_eq!(census.bytes_decoded, bytes.len());

        let residual = crate::deltas::semantic_residual(&stream);
        assert!(residual[..bytes.len()].iter().all(|byte| *byte == 0xff));
        assert!(residual.ends_with(bytes));
    }
}

#[test]
fn deltas_walks_status_framed_blend_bound_records() {
    fn record(escape: bool, xmt: u32, surface: u32) -> Vec<u8> {
        let mut bytes = 59u16.to_be_bytes().to_vec();
        if escape {
            bytes.push(0xff);
        }
        bytes.extend(encoded_xmt(xmt));
        bytes.extend_from_slice(&17u32.to_be_bytes());
        for (reference, status) in [(1u32, 1u8), (3, 1), (40_001, 0), (1, 1), (40_002, 0)] {
            bytes.extend(encoded_xmt(reference));
            bytes.push(status);
        }
        bytes.push(b'+');
        bytes.extend(encoded_xmt(0));
        bytes.extend(encoded_xmt(surface));
        bytes.push(1);
        bytes
    }

    let direct = record(false, 24, 40_003);
    let escaped = record(true, 40_004, 40_005);
    let mut stream = direct.clone();
    stream.extend_from_slice(&escaped);

    let census = crate::deltas::walk(&stream);

    assert_eq!(census.full_counts["BLEND_BOUND"], 2);
    assert_eq!(census.bytes_decoded, stream.len());
    assert_eq!(census.records[0].canonical_bytes, direct);
    assert_eq!(
        census.records[0].references,
        [1, 3, 40_001, 1, 40_002, 0, 40_003]
    );
    assert_eq!(census.records[1].canonical_bytes, escaped);
    assert_eq!(
        crate::intersection::blend_bounds(&stream)
            .into_iter()
            .map(|record| record.framing)
            .collect::<Vec<_>>(),
        [
            crate::intersection::BlendBoundFraming::DeltasDirect,
            crate::intersection::BlendBoundFraming::DeltasEscaped,
        ]
    );

    let mut invalid_status = record(false, 24, 40_003);
    *invalid_status.last_mut().expect("terminal status") = 0;
    assert!(crate::deltas::walk(&invalid_status).records.is_empty());
}

#[test]
fn deltas_walks_complete_nurbs_auxiliary_records() {
    let source = bspline_partition_stream();
    for (kind, family) in [
        (125u16, "B_SURFACE_DATA"),
        (126, "B_SURFACE_DESCRIPTOR"),
        (127, "MULTIPLICITIES"),
        (128, "KNOTS"),
        (135, "B_CURVE_DATA"),
        (136, "B_CURVE_DESCRIPTOR"),
    ] {
        let (pos, auxiliary) = (0..source.len())
            .find_map(|pos| {
                let auxiliary = crate::nurbs::auxiliary_record_at(&source, pos)?;
                (auxiliary.kind == kind).then_some((pos, auxiliary))
            })
            .expect("complete NURBS auxiliary record");
        let bytes = &source[pos..auxiliary.end];
        let mut stream = bytes.to_vec();
        stream.extend_from_slice(&[0xfe, 0xdc]);

        let census = crate::deltas::walk(&stream);
        assert_eq!(census.records.len(), 1);
        assert_eq!(census.records[0].kind(), kind);
        assert_eq!(census.records[0].canonical_bytes, bytes);
        assert_eq!(census.full_counts[family], 1);
        assert_eq!(census.bytes_decoded, bytes.len());

        let residual = crate::deltas::semantic_residual(&stream);
        assert!(residual[..bytes.len()].iter().all(|byte| *byte == 0xff));
        assert!(residual.ends_with(bytes));
    }
}

#[test]
fn deltas_walks_complete_status_framed_surface_descriptors() {
    let mut descriptor = 126u16.to_be_bytes().to_vec();
    descriptor.push(0xff);
    descriptor.extend(encoded_xmt(98));
    descriptor.extend_from_slice(&5u32.to_be_bytes());
    descriptor.extend_from_slice(&3u16.to_be_bytes());
    descriptor.extend_from_slice(&30u32.to_be_bytes());
    descriptor.extend_from_slice(&4u32.to_be_bytes());
    descriptor.extend_from_slice(&[6, 5]);
    descriptor.extend_from_slice(&10u32.to_be_bytes());
    descriptor.extend_from_slice(&2u32.to_be_bytes());
    descriptor.extend_from_slice(&1u32.to_be_bytes());
    descriptor.extend_from_slice(&3u16.to_be_bytes());
    for reference in [106u32, 107, 108, 109, 110] {
        descriptor.extend(encoded_xmt(reference));
        descriptor.push(0);
    }
    let descriptor_len = descriptor.len();
    descriptor.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&descriptor);
    assert_eq!(census.records.len(), 1);
    assert_eq!(census.records[0].kind(), 126);
    assert_eq!(census.records[0].xmt, 98);
    assert_eq!(census.records[0].end, descriptor_len);
    assert_eq!(census.full_counts["B_SURFACE_DESCRIPTOR"], 1);
    assert_eq!(census.bytes_decoded, descriptor_len);

    let mut invalid_status = descriptor[..descriptor_len].to_vec();
    *invalid_status.last_mut().expect("final reference status") = 1;
    assert!(crate::deltas::walk(&invalid_status).records.is_empty());
}

#[test]
fn deltas_walks_complete_surface_data_headers() {
    fn record(escape: bool, xmt: u32, marker: u8) -> Vec<u8> {
        let mut bytes = 125u16.to_be_bytes().to_vec();
        if escape {
            bytes.push(0xff);
        }
        bytes.extend(encoded_xmt(xmt));
        for value in [0.0f64, 1.0, -0.25, 0.5, 0.0, 1.0, -0.25, 0.5] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.push(marker);
        bytes.extend(std::iter::repeat_n(b'B', usize::from(marker) * 4));
        bytes.extend(std::iter::repeat_n(b'?', 12 - usize::from(marker) * 4));
        for reference in [1u32, 20, 21, 1] {
            bytes.extend(encoded_xmt(reference));
            bytes.push(1);
        }
        bytes
    }

    let direct = record(false, 20, 1);
    let escaped = record(true, 40_000, 2);
    let mut extended_marker_one = record(false, 21, 1);
    extended_marker_one[73..77].fill(b'B');
    let mut stream = direct.clone();
    stream.extend_from_slice(&escaped);
    stream.extend_from_slice(&extended_marker_one);
    let decoded_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.full_counts["B_SURFACE_DATA"], 3);
    assert_eq!(census.bytes_decoded, decoded_len);
    assert_eq!(census.records[0].canonical_bytes, direct);
    assert_eq!(census.records[1].canonical_bytes, escaped);
    assert_eq!(census.records[2].canonical_bytes, extended_marker_one);

    let mut invalid_marker = record(false, 20, 2);
    invalid_marker[68] = 3;
    assert!(crate::deltas::walk(&invalid_marker).records.is_empty());

    let mut invalid_status = record(false, 20, 1);
    *invalid_status.last_mut().expect("final status") = 0;
    assert!(crate::deltas::walk(&invalid_status).records.is_empty());
}

#[test]
fn deltas_walks_complete_curve_data_headers() {
    fn record(escape: bool, xmt: u32, mode: u8, reference: u32) -> Vec<u8> {
        let mut bytes = 135u16.to_be_bytes().to_vec();
        if escape {
            bytes.push(0xff);
        }
        bytes.extend(encoded_xmt(xmt));
        bytes.push(mode);
        bytes.extend(encoded_xmt(reference));
        bytes.push(1);
        bytes
    }

    let direct = record(false, 20, 2, 1);
    let escaped = record(true, 40_000, 1, 21);
    let mut stream = direct.clone();
    stream.extend_from_slice(&escaped);
    let decoded_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.full_counts["B_CURVE_DATA"], 2);
    assert_eq!(census.bytes_decoded, decoded_len);
    assert_eq!(census.records[0].canonical_bytes, direct);
    assert_eq!(census.records[1].canonical_bytes, escaped);

    let mut invalid_marker = record(false, 20, 2, 1);
    invalid_marker[4] = 3;
    assert!(crate::deltas::walk(&invalid_marker).records.is_empty());

    let mut invalid_status = record(false, 20, 2, 1);
    *invalid_status.last_mut().expect("final status") = 0;
    assert!(crate::deltas::walk(&invalid_status).records.is_empty());
}

#[test]
fn deltas_walks_complete_type_141_records() {
    fn record(escape: bool, xmt: u32, references: [u32; 4], boundary_statuses: [u8; 2]) -> Vec<u8> {
        let mut bytes = 141u16.to_be_bytes().to_vec();
        if escape {
            bytes.push(0xff);
        }
        bytes.extend(encoded_xmt(xmt));
        for (reference, status) in
            references
                .into_iter()
                .zip([boundary_statuses[0], 0, 0, boundary_statuses[1]])
        {
            bytes.extend(encoded_xmt(reference));
            bytes.push(status);
        }
        bytes
    }

    let direct = record(false, 3158, [646, 3943, 3165, 131], [0, 1]);
    let direct_extended = record(false, 33_000, [646, 3943, 3165, 131], [1, 0]);
    let escaped = record(true, 40_000, [40_001, 1, 0, 40_002], [1, 1]);
    let ambiguous_escaped = record(true, 325, [317, 44, 44, 8], [1, 1]);
    let mut stream = direct.clone();
    stream.extend_from_slice(&direct_extended);
    stream.extend_from_slice(&escaped);
    stream.extend_from_slice(&ambiguous_escaped);
    let decoded_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.full_counts["TYPE_141"], 4);
    assert_eq!(census.bytes_decoded, decoded_len);
    assert_eq!(census.records[0].canonical_bytes, direct);
    assert_eq!(census.records[1].canonical_bytes, direct_extended);
    assert_eq!(census.records[1].xmt, 33_000);
    assert_eq!(census.records[2].canonical_bytes, escaped);
    assert_eq!(census.records[2].xmt, 40_000);
    assert_eq!(census.records[2].references, [40_001, 1, 0, 40_002]);
    assert_eq!(census.records[3].canonical_bytes, ambiguous_escaped);
    assert_eq!(census.records[3].xmt, 325);
    assert_eq!(census.records[3].references, [317, 44, 44, 8]);

    let residual = crate::deltas::semantic_residual(&stream);
    assert!(residual[..decoded_len].iter().all(|byte| *byte == 0xff));
    assert!(residual.ends_with(&[direct, direct_extended, escaped, ambiguous_escaped].concat()));
}

#[test]
fn deltas_walks_complete_type_45_records() {
    fn record(escape: bool, xmt: u32, values: &[f64], count_offset: usize) -> Vec<u8> {
        let mut bytes = 45u16.to_be_bytes().to_vec();
        if escape {
            bytes.push(0xff);
        }
        bytes.extend_from_slice(
            &u32::try_from(values.len() - count_offset)
                .expect("test value count")
                .to_be_bytes(),
        );
        bytes.extend(encoded_xmt(xmt));
        for value in values {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes
    }

    let direct = record(false, 33_000, &[1.0, -2.0, 3.0, 4.0, 5.0], 1);
    let escaped = record(true, 40_000, &[0.0, 0.25, -0.5, 0.75, 1.0], 1);
    let mut stream = direct.clone();
    stream.extend_from_slice(&escaped);
    let decoded_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.full_counts["TYPE_45"], 2);
    assert_eq!(census.bytes_decoded, decoded_len);
    assert_eq!(census.records[0].canonical_bytes, direct);
    assert_eq!(census.records[0].xmt, 33_000);
    assert_eq!(census.records[1].canonical_bytes, escaped);
    assert_eq!(census.records[1].xmt, 40_000);

    let residual = crate::deltas::semantic_residual(&stream);
    assert!(residual[..decoded_len].iter().all(|byte| *byte == 0xff));
    assert!(residual.ends_with(&[direct, escaped].concat()));

    let mut counted = record(false, 41_000, &[1.0, 2.0, 3.0], 0);
    let counted_end = counted.len();
    let mut surface_header = 125u16.to_be_bytes().to_vec();
    surface_header.extend(encoded_xmt(42_000));
    for value in [0.0f64, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0] {
        surface_header.extend_from_slice(&value.to_be_bytes());
    }
    surface_header.extend_from_slice(&[2, b'B', b'B', b'B', b'B', b'B', b'B', b'B', b'B']);
    surface_header.extend_from_slice(b"????");
    for reference in [1u32, 1, 1, 1] {
        surface_header.extend(encoded_xmt(reference));
        surface_header.push(1);
    }
    counted.extend_from_slice(&surface_header);
    let census = crate::deltas::walk(&counted);
    assert_eq!(
        census
            .records
            .iter()
            .map(|record| (record.kind(), record.offset, record.end))
            .collect::<Vec<_>>(),
        [(45, 0, counted_end), (125, counted_end, counted.len())]
    );

    let mut counted = record(false, 41_001, &[1.0, 2.0, 3.0], 0);
    let counted_end = counted.len();
    let mut curve_header = 135u16.to_be_bytes().to_vec();
    curve_header.extend(encoded_xmt(42_001));
    curve_header.push(2);
    curve_header.extend(encoded_xmt(1));
    curve_header.push(1);
    counted.extend_from_slice(&curve_header);
    let census = crate::deltas::walk(&counted);
    assert_eq!(
        census
            .records
            .iter()
            .map(|record| (record.kind(), record.offset, record.end))
            .collect::<Vec<_>>(),
        [(45, 0, counted_end), (135, counted_end, counted.len())]
    );

    let mut nonfinite = record(false, 12, &[1.0, 2.0, 3.0, 4.0, f64::NAN], 1);
    nonfinite.extend_from_slice(&[0xfe, 0xdc]);
    assert!(crate::deltas::walk(&nonfinite).records.is_empty());

    let subnormal = record(false, 12, &[1.0, 2.0, f64::from_bits(1)], 0);
    assert!(crate::deltas::walk(&subnormal).records.is_empty());
}

#[test]
fn deltas_walks_complete_type_70_records() {
    fn record(escape: bool, xmt: u32, count: u16, trailing_reference: u32) -> Vec<u8> {
        let mut bytes = 70u16.to_be_bytes().to_vec();
        if escape {
            bytes.push(0xff);
        }
        bytes.extend(encoded_xmt(xmt));
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.push(4);
        for reference in [3u32, 1, 1, 0] {
            bytes.push(1);
            bytes.extend(encoded_xmt(reference));
        }
        bytes.extend_from_slice(&count.to_be_bytes());
        bytes.extend_from_slice(&20u32.to_be_bytes());
        bytes.extend_from_slice(&1u32.to_be_bytes());
        for _ in 0..2 {
            bytes.extend(encoded_xmt(trailing_reference));
            bytes.push(0);
        }
        bytes
    }

    let direct = record(false, 7, 11, 52);
    let escaped = record(true, 40_000, 14, 40_001);
    let mut stream = direct.clone();
    stream.extend_from_slice(&escaped);

    let census = crate::deltas::walk(&stream);

    assert_eq!(census.full_counts["TYPE_70"], 2);
    assert_eq!(census.bytes_decoded, stream.len());
    assert_eq!(census.records[0].canonical_bytes, direct);
    assert_eq!(census.records[0].node_id(), Some(0));
    assert_eq!(census.records[0].references, [3, 1, 1, 0, 52, 52]);
    assert_eq!(census.records[1].canonical_bytes, escaped);
    assert_eq!(census.records[1].xmt, 40_000);

    let mut mismatched = record(false, 7, 11, 52);
    let end = mismatched.len();
    mismatched[end - 2] = 53;
    assert!(crate::deltas::walk(&mismatched).records.is_empty());
}

#[test]
fn deltas_offset_surface_normalizes_exact_record_envelope() {
    let stream = deltas_offset_surface_partition_stream();
    let record = crate::deltas::walk(&stream).records.remove(0);
    assert_eq!(record.canonical_bytes.len(), 39);
    assert_eq!(
        crate::topology::offset_surfaces(&record.canonical_bytes)[0].distance,
        4.5
    );

    let mut finite_state = stream.clone();
    let state = finite_state.len() - 8;
    put_f64(&mut finite_state, state, 4.0);
    assert_eq!(crate::deltas::walk(&finite_state).records.len(), 1);
    put_f64(&mut finite_state, state, f64::NAN);
    assert!(crate::deltas::walk(&finite_state).records.is_empty());

    let mut invalid_status = stream.clone();
    let offset = invalid_status
        .windows(4)
        .position(|window| window == [0, 60, 0, 12])
        .expect("OFFSET_SURF record");
    invalid_status[offset + 28] = 2;
    assert!(!crate::deltas::walk(&invalid_status)
        .records
        .iter()
        .any(|record| record.kind() == 60));

    let mut truncated = stream;
    truncated.pop();
    assert!(!crate::deltas::walk(&truncated)
        .records
        .iter()
        .any(|record| record.kind() == 60));
}

#[test]
fn deltas_procedural_wrappers_normalize_complete_record_envelopes() {
    for (stream, family, kind, byte_len) in [
        (
            deltas_blend_surface_partition_stream(),
            "BLEND_SURF",
            56,
            66,
        ),
        (
            deltas_trimmed_curve_partition_stream(),
            "TRIMMED_CURVE",
            133,
            85,
        ),
        (deltas_surface_curve_partition_stream(), "SP_CURVE", 137, 33),
    ] {
        let census = crate::deltas::walk(&stream);
        assert_eq!(census.full_counts.get(family), Some(&1));
        let record = census
            .records
            .iter()
            .find(|record| record.kind() == kind)
            .expect("procedural wrapper");
        assert_eq!(record.canonical_bytes.len(), byte_len);
        assert!(crate::topology::Graph::parse(&record.canonical_bytes)
            .get(kind as u8, 12)
            .is_some());
    }

    let mut invalid_blend = deltas_blend_surface_partition_stream();
    let blend = invalid_blend
        .windows(4)
        .position(|window| window == [0, 56, 0, 12])
        .expect("BLEND_SURF record");
    invalid_blend[blend + 24] = b'X';
    assert!(!crate::deltas::walk(&invalid_blend)
        .records
        .iter()
        .any(|record| record.kind() == 56));
}

#[test]
fn deltas_fixed_record_boundary_accepts_known_auxiliary_tag() {
    let mut stream = deltas_bspline_curve_wrapper_stream();
    let wrapper_len = stream.len();
    stream.extend_from_slice(&[0, 141, 0xfe]);

    let census = crate::deltas::walk(&stream);
    let wrapper = census
        .records
        .iter()
        .find(|record| record.kind() == 134)
        .expect("B_CURVE wrapper");
    assert_eq!(wrapper.end, wrapper_len);
    assert_eq!(wrapper.canonical_bytes.len(), 23);
}

#[test]
fn deltas_fixed_records_accept_direct_extended_and_escaped_envelopes() {
    fn fin(escape: bool, xmt: u32) -> (Vec<u8>, Vec<u8>) {
        let mut source = 17u16.to_be_bytes().to_vec();
        let mut canonical = source.clone();
        if escape {
            source.push(0xff);
            canonical.push(0xff);
        }
        let encoded_identity = encoded_xmt(xmt);
        source.extend_from_slice(&encoded_identity);
        canonical.extend_from_slice(&encoded_identity);
        for reference in 20..29 {
            let encoded_reference = encoded_xmt(reference);
            source.extend_from_slice(&encoded_reference);
            source.push(1);
            canonical.extend_from_slice(&encoded_reference);
        }
        source.push(b'+');
        canonical.push(b'+');
        (source, canonical)
    }

    let (direct_extended, direct_canonical) = fin(false, 32_768);
    let (escaped, escaped_canonical) = fin(true, 40);
    let mut stream = direct_extended.clone();
    stream.extend_from_slice(&escaped);
    let mut escaped_point = vec![0, 29, 0xff, 0, 41];
    escaped_point.extend_from_slice(&42u32.to_be_bytes());
    for reference in 43..47 {
        escaped_point.extend(encoded_xmt(reference));
        escaped_point.push(1);
    }
    for coordinate in [1.0f64, 2.0, 3.0] {
        escaped_point.extend_from_slice(&coordinate.to_be_bytes());
    }
    stream.extend_from_slice(&escaped_point);
    let decoded_len = stream.len();
    stream.extend_from_slice(&[0xfe, 0xdc]);

    let census = crate::deltas::walk(&stream);
    assert_eq!(census.full_counts["FIN"], 2);
    assert_eq!(census.full_counts["POINT"], 1);
    assert_eq!(census.bytes_decoded, decoded_len);
    assert_eq!(census.records[0].xmt, 32_768);
    assert_eq!(census.records[0].canonical_bytes, direct_canonical);
    assert_eq!(census.records[1].xmt, 40);
    assert_eq!(census.records[1].canonical_bytes, escaped_canonical);
    assert_eq!(census.records[2].xmt, 41);
    assert_eq!(census.records[2].node_id(), Some(42));
    assert_eq!(census.records[2].family.position(), Some([1.0, 2.0, 3.0]));
}

#[test]
fn merged_deltas_full_record_replaces_partition_node() {
    let partition = topology_partition_stream();
    let mut deltas = status_framed_deltas_point_stream();
    deltas[2..4].copy_from_slice(&11u16.to_be_bytes());
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    let points = crate::geometry::points(&merged);
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].position.x, 12.5);
    assert_eq!(points[0].position.y, -2.0);
    assert_eq!(points[0].position.z, 4.0);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_some());
}

#[test]
fn merged_tombstone_preserves_a_topology_referenced_carrier() {
    let partition = topology_partition_stream();
    let mut tombstone = Vec::new();
    tombstone.extend_from_slice(&29u16.to_be_bytes());
    tombstone.extend_from_slice(&11u16.to_be_bytes());
    tombstone.extend_from_slice(&[0, 1]);
    let census = crate::deltas::walk(&tombstone);
    assert_eq!(census.tombstones.len(), 1);
    assert_eq!(census.tombstones[0].kind, 29);
    assert_eq!(census.tombstones[0].xmt, 11);
    let merged = crate::deltas::merge_full_records(&partition, &tombstone);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_some());
    assert_eq!(crate::geometry::points(&merged)[0].position.x, 10.0);
}

#[test]
fn merged_exact_key_tombstone_removes_unreferenced_partition_node() {
    let mut partition = record(29, 40);
    put_ref(&mut partition, 2, 11);
    put_vec3(&mut partition, 16, [0.01, 0.02, 0.03]);
    let tombstone = [0, 29, 0, 11, 0, 1];
    let merged = crate::deltas::merge_full_records(&partition, &tombstone);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_none());
}

#[test]
fn merged_deltas_uses_last_full_or_tombstone_event() {
    let partition = topology_partition_stream();
    let tombstone = [0, 29, 0, 11, 0, 1];
    let mut full = status_framed_deltas_point_stream();
    full[2..4].copy_from_slice(&11u16.to_be_bytes());

    let mut delete_then_replace = tombstone.to_vec();
    delete_then_replace.extend_from_slice(&full);
    let merged = crate::deltas::merge_full_records(&partition, &delete_then_replace);
    assert_eq!(crate::geometry::points(&merged)[0].position.x, 12.5);

    let mut replace_then_delete = full;
    replace_then_delete.extend_from_slice(&tombstone);
    let merged = crate::deltas::merge_full_records(&partition, &replace_then_delete);
    assert_eq!(crate::geometry::points(&merged)[0].position.x, 10.0);
}

#[test]
fn final_body_revision_scopes_deltas_overlay_events() {
    let mut partition = record(29, 40);
    put_ref(&mut partition, 2, 11);
    put_vec3(&mut partition, 16, [0.01, 0.02, 0.03]);
    let known_tombstone = [0, 29, 0, 11, 0, 1];

    let mut historical_delete = deltas_body_revision(1);
    historical_delete.extend_from_slice(&known_tombstone);
    historical_delete.extend_from_slice(&deltas_body_revision(2));
    let merged = crate::deltas::merge_full_records(&partition, &historical_delete);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_some());

    let mut current_delete = historical_delete;
    current_delete.extend_from_slice(&known_tombstone);
    let merged = crate::deltas::merge_full_records(&partition, &current_delete);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_none());
}

#[test]
fn body_revision_scopes_keep_each_monotonic_sequence_current() {
    let mut deltas = deltas_body_revision(1);
    deltas.extend(deltas_point(50, 0.001));
    deltas.extend(deltas_body_revision(2));
    deltas.extend(deltas_point(50, 0.002));
    deltas.extend(deltas_body_revision(1));
    deltas.extend(deltas_point(51, 0.003));
    deltas.extend(deltas_body_revision(2));
    deltas.extend(deltas_point(51, 0.004));

    let merged = crate::deltas::merge_full_records(&[], &deltas);
    let graph = crate::topology::Graph::parse(&merged);
    assert!(graph.get(29, 50).is_some());
    assert!(graph.get(29, 51).is_some());
    let points = crate::geometry::points(&merged);
    assert!(points
        .iter()
        .any(|point| (point.position.x - 2.0).abs() <= 1.0e-12));
    assert!(points
        .iter()
        .any(|point| (point.position.x - 4.0).abs() <= 1.0e-12));
    assert!(!points
        .iter()
        .any(|point| (point.position.x - 1.0).abs() <= 1.0e-12));
    assert!(!points
        .iter()
        .any(|point| (point.position.x - 3.0).abs() <= 1.0e-12));
}

#[test]
fn body_revision_scopes_accept_reverse_serialized_counter_direction() {
    let mut deltas = deltas_body_revision(4);
    deltas.extend(deltas_point(50, 0.001));
    deltas.extend(deltas_body_revision(3));
    deltas.extend(deltas_point(50, 0.002));
    deltas.extend(deltas_body_revision(4));
    deltas.extend(deltas_point(51, 0.003));
    deltas.extend(deltas_body_revision(3));
    deltas.extend(deltas_point(51, 0.004));

    let merged = crate::deltas::merge_full_records(&[], &deltas);
    let graph = crate::topology::Graph::parse(&merged);
    assert!(graph.get(29, 50).is_some());
    assert!(graph.get(29, 51).is_some());
    let points = crate::geometry::points(&merged);
    assert!(points
        .iter()
        .any(|point| (point.position.x - 2.0).abs() <= 1.0e-12));
    assert!(points
        .iter()
        .any(|point| (point.position.x - 4.0).abs() <= 1.0e-12));
    assert!(!points
        .iter()
        .any(|point| (point.position.x - 1.0).abs() <= 1.0e-12));
    assert!(!points
        .iter()
        .any(|point| (point.position.x - 3.0).abs() <= 1.0e-12));
}

#[test]
fn unmatched_tombstones_are_scoped_to_the_final_body_revision() {
    let partition = topology_partition_stream();
    let unknown_tombstone = [0, 29, 0, 99, 0, 1];
    let mut historical_delete = deltas_body_revision(1);
    historical_delete.extend_from_slice(&unknown_tombstone);
    historical_delete.extend_from_slice(&deltas_body_revision(2));
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &historical_delete),
        0
    );

    historical_delete.extend_from_slice(&unknown_tombstone);
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &historical_delete),
        1
    );
}

#[test]
fn unmatched_tombstones_are_scoped_per_body_revision_sequence() {
    let partition = topology_partition_stream();
    let historical_first = [0, 29, 0, 98, 0, 1];
    let historical_second = [0, 29, 0, 99, 0, 1];
    let mut deltas = deltas_body_revision(1);
    deltas.extend_from_slice(&historical_first);
    deltas.extend_from_slice(&deltas_body_revision(2));
    deltas.extend_from_slice(&historical_first);
    deltas.extend_from_slice(&deltas_body_revision(1));
    deltas.extend_from_slice(&historical_second);
    deltas.extend_from_slice(&deltas_body_revision(2));

    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &deltas),
        1
    );
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones_by_family(&partition, &deltas).get("POINT"),
        Some(&1)
    );
}

#[test]
fn semantic_residual_masks_historical_body_revisions() {
    let mut deltas = deltas_body_revision(1);
    let historical_len = deltas.len();
    deltas.extend_from_slice(&[0, 38, 0xaa, 0xbb, 0xcc]);
    deltas.extend_from_slice(&deltas_body_revision(2));
    deltas.extend_from_slice(&[0, 38, 0x11, 0x22, 0x33]);

    let residual = crate::deltas::semantic_residual(&deltas);
    assert!(residual[..historical_len + 5]
        .iter()
        .all(|byte| *byte == 0xff));
    assert!(residual.ends_with(&[0, 38, 0x11, 0x22, 0x33]));
}

#[test]
fn semantic_residual_with_census_matches_the_standalone_transform() {
    let mut deltas = deltas_body_revision(1);
    deltas.extend_from_slice(&status_framed_deltas_intersection_stream());
    deltas.extend_from_slice(&deltas_body_revision(2));
    deltas.extend_from_slice(&status_framed_deltas_intersection_stream());

    let census = crate::deltas::walk(&deltas);
    assert_eq!(
        crate::deltas::semantic_residual_with_census(&deltas, &census),
        crate::deltas::semantic_residual(&deltas)
    );
}

#[test]
fn merge_masks_historical_interleaved_body_sequences() {
    let mut first_historical = status_framed_deltas_intersection_stream();
    first_historical[4..8].copy_from_slice(&1u32.to_be_bytes());
    let mut first_current = status_framed_deltas_intersection_stream();
    first_current[4..8].copy_from_slice(&2u32.to_be_bytes());
    let mut second_historical = status_framed_deltas_intersection_stream();
    second_historical[2..4].copy_from_slice(&13u16.to_be_bytes());
    second_historical[4..8].copy_from_slice(&3u32.to_be_bytes());
    let mut second_current = second_historical.clone();
    second_current[4..8].copy_from_slice(&4u32.to_be_bytes());

    let mut deltas = deltas_body_revision(1);
    deltas.extend_from_slice(&first_historical);
    deltas.extend_from_slice(&deltas_body_revision(2));
    deltas.extend_from_slice(&first_current);
    deltas.extend_from_slice(&deltas_body_revision(1));
    deltas.extend_from_slice(&second_historical);
    deltas.extend_from_slice(&deltas_body_revision(2));
    deltas.extend_from_slice(&second_current);

    let merged = crate::deltas::merge_full_records(&[], &deltas);
    let mut expected = crate::deltas::walk(&first_current).records[0]
        .canonical_bytes
        .clone();
    expected.extend_from_slice(&crate::deltas::walk(&second_current).records[0].canonical_bytes);
    assert!(merged.ends_with(&expected));
    assert!(!merged
        .windows(first_historical.len())
        .any(|window| window == first_historical));
    assert!(!merged
        .windows(second_historical.len())
        .any(|window| window == second_historical));
}

#[test]
fn unmatched_delta_tombstones_follow_exact_last_event_identity() {
    let partition = topology_partition_stream();
    let known = [0, 29, 0, 11, 0, 1];
    let unknown = [0, 29, 0, 99, 0, 1];
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &known),
        0
    );
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &unknown),
        1
    );
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones_by_family(&partition, &unknown).get("POINT"),
        Some(&1)
    );

    let mut full = status_framed_deltas_point_stream();
    full[2..4].copy_from_slice(&99u16.to_be_bytes());
    let mut add_then_delete = full.clone();
    add_then_delete.extend_from_slice(&unknown);
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &add_then_delete),
        0
    );

    let mut delete_then_add = unknown.to_vec();
    delete_then_add.extend_from_slice(&full);
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &delete_then_add),
        0
    );
}

#[test]
fn merged_result_preserves_tombstone_accounting() {
    let partition = topology_partition_stream();
    let known = [0, 29, 0, 11, 0, 1];
    let unknown = [0, 29, 0, 99, 0, 1];
    let mut full = status_framed_deltas_point_stream();
    full[2..4].copy_from_slice(&99u16.to_be_bytes());
    let mut add_then_delete = full.clone();
    add_then_delete.extend_from_slice(&unknown);
    let mut delete_then_add = unknown.to_vec();
    delete_then_add.extend_from_slice(&full);

    for deltas in [
        known.as_slice(),
        unknown.as_slice(),
        &add_then_delete,
        &delete_then_add,
    ] {
        let census = crate::deltas::walk(deltas);
        let result =
            crate::deltas::merge_full_records_with_census(&partition, deltas, &census, true);
        assert_eq!(
            result.unmatched_tombstones,
            crate::deltas::unmatched_terminal_tombstones_by_family(&partition, deltas)
        );
        assert_eq!(
            result.merged,
            crate::deltas::merge_full_records(&partition, deltas)
        );
    }
}
mod reference_and_tombstone_packets;
