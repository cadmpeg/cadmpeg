// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::sab::{Record, Token};

fn tagged(out: &mut Vec<u8>, tag: u8, value: i64) {
    out.push(tag);
    out.extend_from_slice(&value.to_le_bytes());
}

type Board<'a> = (i64, i64, &'a [(i64, i64)]);

fn chain(entries: &[Board<'_>]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for &(owner, number, changes) in entries {
        tagged(&mut bytes, 0x04, 1);
        tagged(&mut bytes, 0x0c, owner);
        tagged(&mut bytes, 0x04, number);
        for &(old, new) in changes {
            tagged(&mut bytes, 0x04, 1);
            tagged(&mut bytes, 0x0c, old);
            tagged(&mut bytes, 0x0c, new);
        }
        tagged(&mut bytes, 0x04, 0);
    }
    tagged(&mut bytes, 0x04, 0);
    bytes
}

fn walk(bytes: &[u8]) -> Option<(Vec<BulletinBoardItem>, usize)> {
    let mut items = Vec::new();
    let end = walk_bulletin_boards(bytes, 0, RefWidth::Eight, |item| items.push(item))?;
    Some((items, end))
}

#[test]
fn walk_classifies_insert_delete_and_update() {
    let bytes = chain(&[(3, 7, &[(-1, 4), (5, -1), (6, 8)])]);
    let (items, end) = walk(&bytes).unwrap();
    assert_eq!(end, bytes.len());
    assert_eq!(
        items,
        [
            BulletinBoardItem::Board {
                offset: 0,
                owner_ref: 3,
                number: 7
            },
            BulletinBoardItem::Change {
                offset: 27,
                kind: EntityChange::Insert { new: 4 }
            },
            BulletinBoardItem::Change {
                offset: 54,
                kind: EntityChange::Delete { old: 5 }
            },
            BulletinBoardItem::Change {
                offset: 81,
                kind: EntityChange::Update { old: 6, new: 8 }
            },
        ]
    );
}

#[test]
fn walk_refuses_a_change_with_two_null_references() {
    assert!(walk(&chain(&[(0, 1, &[(-1, -1)])])).is_none());
}

#[test]
fn walk_refuses_a_truncated_chain() {
    let bytes = chain(&[(0, 1, &[(-1, 2)])]);
    assert!(walk(&bytes[..bytes.len() - 1]).is_none());
}

#[test]
fn walk_returns_an_immediate_record_delimiter_unchanged() {
    let mut items = 0;
    assert_eq!(
        walk_bulletin_boards(&[0x11], 0, RefWidth::Four, |_| items += 1),
        Some(0)
    );
    assert_eq!(items, 0);
}

#[test]
fn walk_reads_four_byte_references() {
    let mut bytes = Vec::new();
    for (tag, value) in [(0x04, 1_i32), (0x0c, 2), (0x04, 3), (0x04, 0), (0x04, 0)] {
        bytes.push(tag);
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let mut items = Vec::new();
    let end = walk_bulletin_boards(&bytes, 0, RefWidth::Four, |item| items.push(item));
    assert_eq!(end, Some(bytes.len()));
    assert_eq!(
        items,
        [BulletinBoardItem::Board {
            offset: 0,
            owner_ref: 2,
            number: 3
        }]
    );
}

fn record(index: usize, refs: &[i64]) -> Record {
    Record {
        index,
        name: "face".into(),
        tokens: refs
            .iter()
            .map(|&r| Token::Ref(r))
            .collect::<Vec<_>>()
            .into(),
        offset: 0,
        len: 0,
    }
}

fn materialize(versions: &[(i64, i64)], records: &[(i64, Record)]) -> Option<Vec<Record>> {
    let versions: BTreeMap<i64, i64> = versions.iter().copied().collect();
    materialize_record_table(&versions, |revision| {
        records
            .iter()
            .find(|(r, _)| *r == revision)
            .map(|(_, record)| record)
    })
}

#[test]
fn materialize_orders_records_by_index() {
    let records = [(10, record(1, &[0, -1])), (11, record(0, &[]))];
    let table = materialize(&[(1, 10), (0, 11)], &records).unwrap();
    let indices: Vec<_> = table.iter().map(|r| r.index).collect();
    assert_eq!(indices, [0, 1]);
}

#[test]
fn materialize_refuses_a_reference_outside_the_slot_set() {
    let records = [(0, record(0, &[5]))];
    assert!(materialize(&[(0, 0)], &records).is_none());
}

#[test]
fn materialize_refuses_an_index_that_differs_from_its_slot() {
    let records = [(0, record(1, &[]))];
    assert!(materialize(&[(0, 0)], &records).is_none());
}

#[test]
fn materialize_refuses_an_empty_map_and_a_missing_revision() {
    assert!(materialize(&[], &[]).is_none());
    assert!(materialize(&[(0, 9)], &[]).is_none());
}
