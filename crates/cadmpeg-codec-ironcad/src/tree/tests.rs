// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::View;

use super::{read, Kernel};
use crate::test_support::{node, record, record_with};

const ACIS: u32 = 0x8000_0001;
const PARASOLID: u32 = 0x8000_0002;

#[test]
fn records_are_read_in_stored_order_at_their_depth() {
    let payload = node(&[
        node(&[node(&[node(&[])]), record(PARASOLID, b"feature")]),
        record(PARASOLID, b"result"),
        node(&[node(&[node(&[])]), record(ACIS, b"acis")]),
        node(&[]),
        node(&[]),
    ]);
    let records = read(View::over_retained(&payload)).unwrap();
    let summary: Vec<_> = records
        .iter()
        .map(|record| (record.kernel, record.depth, record.bytes.window()))
        .collect();
    assert_eq!(
        summary,
        [
            (Kernel::Parasolid, 2, b"feature".as_slice()),
            (Kernel::Parasolid, 1, b"result".as_slice()),
            (Kernel::Acis, 2, b"acis".as_slice()),
        ]
    );
    assert!(records.iter().all(|record| record.canonical_header));
}

#[test]
fn another_kernel_tag_and_header_words_are_kept() {
    let payload = node(&[record(7, b""), record_with(PARASOLID, [1, 2, 1, 0], b"x")]);
    let records = read(View::over_retained(&payload)).unwrap();
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
