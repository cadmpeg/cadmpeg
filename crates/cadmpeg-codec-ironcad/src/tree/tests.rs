// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::View;

use super::{read, Assembly, Kernel};
use crate::test_support::{node, record, record_with};

const ACIS: u32 = 0x8000_0001;
const PARASOLID: u32 = 0x8000_0002;

#[test]
fn records_are_read_in_stored_order() {
    let payload = node(&[
        node(&[node(&[node(&[])]), record(PARASOLID, b"feature")]),
        record(PARASOLID, b"result"),
        node(&[node(&[node(&[])]), record(ACIS, b"acis")]),
        node(&[]),
        node(&[]),
    ]);
    let records = read(View::over_retained(&payload)).unwrap().records;
    let summary: Vec<_> = records
        .iter()
        .map(|record| (record.kernel, record.bytes.window()))
        .collect();
    assert_eq!(
        summary,
        [
            (Kernel::Parasolid, b"feature".as_slice()),
            (Kernel::Parasolid, b"result".as_slice()),
            (Kernel::Acis, b"acis".as_slice()),
        ]
    );
    assert!(records.iter().all(|record| record.canonical_header));
}

#[test]
fn a_record_after_a_node_that_holds_records_follows_a_node() {
    let payload = node(&[
        node(&[
            node(&[node(&[])]),
            record(PARASOLID, b"first"),
            node(&[node(&[])]),
            record(PARASOLID, b"second"),
        ]),
        record(PARASOLID, b"result"),
        node(&[]),
        record(ACIS, b"after empty node"),
        node(&[
            node(&[node(&[]), record(ACIS, b"nested")]),
            record(ACIS, b"inner"),
        ]),
    ]);
    let records = read(View::over_retained(&payload)).unwrap().records;
    let follows: Vec<_> = records
        .iter()
        .map(|record| (record.bytes.window(), record.follows_node))
        .collect();
    assert_eq!(
        follows,
        [
            (b"first".as_slice(), false),
            (b"second".as_slice(), false),
            (b"result".as_slice(), true),
            (b"after empty node".as_slice(), false),
            (b"nested".as_slice(), false),
            (b"inner".as_slice(), true),
        ]
    );
}

#[test]
fn another_kernel_tag_and_header_words_are_kept() {
    let payload = node(&[record(7, b""), record_with(PARASOLID, [1, 2, 1, 0], b"x")]);
    let records = read(View::over_retained(&payload)).unwrap().records;
    assert_eq!(records[0].kernel, Kernel::Other(7));
    assert!(records[0].canonical_header);
    assert!(!records[1].canonical_header);
}

#[test]
fn record_shorter_than_its_header_is_rejected() {
    let mut short = 4_u32.to_le_bytes().to_vec();
    short.extend_from_slice(&0x0009_b401_u32.to_le_bytes());
    assert!(read(View::over_retained(&node(&[short]))).is_err());
}

#[test]
fn item_that_crosses_its_node_end_is_rejected() {
    let mut payload = node(&[node(&[record(PARASOLID, b"abc")])]);
    payload[8..16].copy_from_slice(&20_u64.to_le_bytes());
    assert!(read(View::over_retained(&payload)).is_err());
    let mut payload = node(&[node(&[])]);
    payload[8..16].copy_from_slice(&9_u64.to_le_bytes());
    assert!(read(View::over_retained(&payload)).is_err());
}

#[test]
fn node_shorter_than_its_length_field_is_rejected() {
    let mut payload = node(&[node(&[])]);
    payload[8..16].copy_from_slice(&7_u64.to_le_bytes());
    assert!(read(View::over_retained(&payload)).is_err());
}

#[test]
fn trailing_bytes_shorter_than_an_item_are_rejected() {
    let mut payload = node(&[]);
    payload.extend_from_slice(&[0; 4]);
    assert!(read(View::over_retained(&payload)).is_err());
}

/// A part pair: a child node that holds a feature record, then the result
/// record.
fn pair(result: &[u8]) -> [Vec<u8>; 2] {
    [
        node(&[node(&[node(&[])]), record(ACIS, b"feature")]),
        record(ACIS, result),
    ]
}

#[test]
fn nodes_that_hold_pairs_are_assemblies_numbered_in_closing_order() {
    let inner = node(&[pair(b"c"), pair(b"d")].concat());
    let outer = node(&[pair(b"b").to_vec(), vec![inner]].concat());
    let payload = node(&[pair(b"a").to_vec(), vec![outer]].concat());
    let tree = read(View::over_retained(&payload)).unwrap();
    let results: Vec<_> = tree
        .records
        .iter()
        .filter(|record| record.follows_node)
        .map(|record| (record.bytes.window(), record.assembly))
        .collect();
    assert_eq!(
        results,
        [
            (b"a".as_slice(), None),
            (b"b".as_slice(), Some(1)),
            (b"c".as_slice(), Some(0)),
            (b"d".as_slice(), Some(0)),
        ]
    );
    assert_eq!(
        tree.assemblies,
        [
            Assembly {
                parent: Some(1),
                end: tree.assemblies[0].end,
            },
            Assembly {
                parent: None,
                end: tree.assemblies[1].end,
            }
        ]
    );
}

#[test]
fn a_record_is_selected_by_a_nonzero_marker_of_any_value() {
    let mut older = record(PARASOLID, b"older");
    older[4..8].copy_from_slice(&0x0008_6801_u32.to_le_bytes());
    let payload = node(&[
        node(&[node(&[node(&[])]), older]),
        record(PARASOLID, b"result"),
    ]);
    let records = read(View::over_retained(&payload)).unwrap().records;
    let bodies: Vec<_> = records.iter().map(|record| record.bytes.window()).collect();
    assert_eq!(bodies, [b"older".as_slice(), b"result".as_slice()]);
    assert!(records.iter().all(|record| record.canonical_header));
}

#[test]
fn a_pair_in_the_child_node_of_a_pair_belongs_to_that_part() {
    let mut items = vec![node(&[node(&[])]), record(ACIS, b"feature")];
    items.extend(pair(b"inner"));
    let child = node(&items);
    let payload = node(&[node(&[child, record(ACIS, b"result")])]);
    let tree = read(View::over_retained(&payload)).unwrap();
    let results: Vec<_> = tree
        .records
        .iter()
        .filter(|record| record.follows_node)
        .map(|record| (record.bytes.window(), record.assembly))
        .collect();
    assert_eq!(results, [(b"result".as_slice(), Some(0))]);
    assert_eq!(tree.assemblies.len(), 1);
    assert_eq!(tree.assemblies[0].parent, None);
}

#[test]
fn a_marker_with_another_low_byte_is_read_and_marked_noncanonical() {
    let mut odd = record(PARASOLID, b"odd");
    odd[4..8].copy_from_slice(&0x0009_b402_u32.to_le_bytes());
    let payload = node(&[odd]);
    let records = read(View::over_retained(&payload)).unwrap().records;
    assert_eq!(records[0].bytes.window(), b"odd");
    assert!(!records[0].canonical_header);
}

#[test]
fn a_node_that_holds_only_an_assembly_node_is_an_assembly() {
    let inner = node(&[pair(b"a"), pair(b"b")].concat());
    let outer = node(&[inner]);
    let payload = node(&[pair(b"root").to_vec(), vec![outer]].concat());
    let tree = read(View::over_retained(&payload)).unwrap();
    let parents: Vec<_> = tree
        .assemblies
        .iter()
        .map(|assembly| assembly.parent)
        .collect();
    assert_eq!(parents, [Some(1), None]);
    assert_eq!(tree.assemblies[0].end, tree.assemblies[1].end);
}
