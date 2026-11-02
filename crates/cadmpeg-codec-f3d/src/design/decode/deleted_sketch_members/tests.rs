// SPDX-License-Identifier: Apache-2.0

use super::{deleted_members, timestamp_entry_offsets};

const SCOPE: u32 = 956;
const DELETED: u32 = 1032;
const STORED: u32 = 1036;
const TIMESTAMP: u64 = 0x0005_da1d_d4a2_1440;

fn header(out: &mut Vec<u8>, tag: [u8; 3], index: u32) {
    out.extend_from_slice(&3_u32.to_le_bytes());
    out.extend_from_slice(&tag);
    out.extend_from_slice(&index.to_le_bytes());
}

fn reference(out: &mut Vec<u8>, index: u32) {
    out.push(1);
    out.extend_from_slice(&index.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
}

fn timestamp_entry(out: &mut Vec<u8>, index: u32) {
    reference(out, index);
    out.extend_from_slice(&TIMESTAMP.to_le_bytes());
}

/// A Sketch scope frame, its paired header, its record-timestamp table, and
/// the stored member record. Returns the bytes and the scope frame.
fn sketch_stream(table_owner: u32) -> (Vec<u8>, std::ops::Range<usize>) {
    let mut bytes = Vec::new();
    header(&mut bytes, *b"397", SCOPE);
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    reference(&mut bytes, DELETED);
    reference(&mut bytes, STORED);
    let paired = bytes.len();
    header(&mut bytes, *b"264", SCOPE);
    bytes.extend_from_slice(&[0; 8]);
    header(&mut bytes, *b"343", SCOPE + 1);
    bytes.extend_from_slice(&[0; 14]);
    bytes.extend_from_slice(&[1, 1]);
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    timestamp_entry(&mut bytes, DELETED);
    timestamp_entry(&mut bytes, STORED);
    timestamp_entry(&mut bytes, table_owner);
    bytes.extend_from_slice(&[0; 8]);
    header(&mut bytes, *b"266", STORED);
    bytes.extend_from_slice(&[0; 8]);
    (bytes, 0..paired)
}

#[test]
fn timestamp_table_lists_entry_index_offsets() {
    let (bytes, frame) = sketch_stream(SCOPE);
    let table = frame.end + 19 + 31;
    assert_eq!(
        timestamp_entry_offsets(&bytes, SCOPE),
        Some(vec![table + 1, table + 20])
    );
}

#[test]
fn timestamp_table_of_another_owner_is_refused() {
    let (bytes, _) = sketch_stream(SCOPE + 2);
    assert_eq!(timestamp_entry_offsets(&bytes, SCOPE), None);
}

#[test]
fn member_named_only_by_its_scope_and_table_is_deleted() {
    let (bytes, frame) = sketch_stream(SCOPE);
    assert_eq!(
        deleted_members(&bytes, frame, SCOPE, &[DELETED]),
        vec![DELETED]
    );
}

#[test]
fn member_named_by_another_record_is_not_deleted() {
    let (mut bytes, frame) = sketch_stream(SCOPE);
    header(&mut bytes, *b"270", 1039);
    reference(&mut bytes, DELETED);
    assert!(deleted_members(&bytes, frame, SCOPE, &[DELETED]).is_empty());
}

#[test]
fn member_with_an_indexed_header_is_not_deleted() {
    let (mut bytes, frame) = sketch_stream(SCOPE);
    header(&mut bytes, *b"361", DELETED);
    assert!(deleted_members(&bytes, frame, SCOPE, &[DELETED]).is_empty());
}

#[test]
fn member_without_a_scope_table_is_not_deleted() {
    let (bytes, frame) = sketch_stream(SCOPE + 2);
    assert!(deleted_members(&bytes, frame, SCOPE, &[DELETED]).is_empty());
}

#[test]
fn second_timestamp_table_of_one_owner_is_refused() {
    let (mut bytes, _) = sketch_stream(SCOPE);
    header(&mut bytes, *b"343", 2000);
    bytes.extend_from_slice(&[0; 14]);
    bytes.extend_from_slice(&[1, 1]);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    timestamp_entry(&mut bytes, SCOPE);
    bytes.extend_from_slice(&[0; 8]);
    assert_eq!(timestamp_entry_offsets(&bytes, SCOPE), None);
}
