// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{Cluster, SpaceMap, TypeTable};
use crate::test_support::{cluster_and_map, type_table, Object};

fn objects() -> Vec<Object> {
    vec![
        Object {
            id: 1,
            form: 0,
            flags: 0,
            body: vec![1; 5],
        },
        Object {
            id: 2,
            form: 1,
            flags: 0x2000,
            body: vec![2; 9],
        },
        Object {
            id: 3,
            form: 7,
            flags: 0,
            body: vec![3; 4],
        },
    ]
}

#[test]
fn map_cluster_and_type_table_resolve_each_object() {
    let (cluster_bytes, map_bytes) = cluster_and_map(&objects());
    let map = SpaceMap::parse(&map_bytes).unwrap();
    assert_eq!(map.items().len(), 3);
    assert_eq!(map.items()[1].id, 2);
    assert_eq!(map.items()[1].flags, 0x2000);
    let cluster = Cluster::parse(&cluster_bytes, &map).unwrap();
    let all: Vec<_> = cluster.objects().collect();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].bytes.len(), 8 + 5);
    assert_eq!(all[1].bytes.len(), 8 + 9);
    assert_eq!(all[2].bytes.len(), 8 + 4);
    let second = cluster.object(2).unwrap();
    assert_eq!((second.id, second.form, second.flags), (2, 1, 0x2000));
    assert_eq!(&second.bytes[8..], &[2; 9]);
    assert!(cluster.object(99).is_none());
}

#[test]
fn type_table_parses_declarations_and_compact_records() {
    let bytes = type_table(&["Alpha", "Beta"], &[1, 0xFFFF, 0]);
    let table = TypeTable::parse(&bytes).unwrap();
    assert_eq!(table.class_name(0, 0), Some("Beta"));
    assert_eq!(table.class_name(2, 0), Some("Alpha"));
    let empty = TypeTable::parse(&type_table(&[], &[])).unwrap();
    assert_eq!(empty.class_name(0, 0), None);
}

#[test]
fn type_table_accepts_the_declaration_link_word() {
    let mut bytes = type_table(&["Alpha", "Beta"], &[1, 0]);
    // The `u16` at declaration offset 22 does not take part in the class rule.
    bytes[20 + 22..20 + 24].copy_from_slice(&0x000e_u16.to_le_bytes());
    let table = TypeTable::parse(&bytes).unwrap();
    assert_eq!(table.class_name(0, 0), Some("Beta"));
    assert_eq!(table.class_name(1, 0), Some("Alpha"));
}

#[test]
fn class_rule_has_three_declaration_branches() {
    let names = ["A", "B", "C", "D", "E", "F"];
    let table = TypeTable::parse(&type_table(&names, &[2, 0xFFFF, 1])).unwrap();
    // A compact record links the form to its declaration.
    assert_eq!(table.class_name(0, 0), Some("C"));
    assert_eq!(table.class_name(2, 0), Some("B"));
    // A compact value of 0xFFFF names declaration `f` itself.
    assert_eq!(table.class_name(1, 0), Some("B"));
    // A form at or above the compact count names declaration `f`.
    assert_eq!(table.class_name(5, 0), Some("F"));
    // Bit 0x2000 of the flags names declaration `f` despite the link.
    assert_eq!(table.class_name(0, 0x2000), Some("A"));
    assert_eq!(table.class_name(0, 0x2001), Some("A"));
    // A declaration that does not exist has no class.
    assert_eq!(table.class_name(9, 0), None);
    assert_eq!(table.class_name(0, 0x4000), Some("C"));
}

#[test]
fn space_map_rejects_what_does_not_walk() {
    let (_, map) = cluster_and_map(&objects());
    assert!(SpaceMap::parse(&map).is_some());
    let mut bad_magic = map.clone();
    bad_magic[0] = b'x';
    assert!(SpaceMap::parse(&bad_magic).is_none());
    let mut trailing = map.clone();
    trailing.push(0);
    assert!(SpaceMap::parse(&trailing).is_none());
    let mut inflated = map.clone();
    inflated[4..6].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(SpaceMap::parse(&inflated).is_none());
    let mut short_item = map.clone();
    short_item[12..14].copy_from_slice(&4_u16.to_le_bytes());
    assert!(SpaceMap::parse(&short_item).is_none());
    let mut free_list = map.clone();
    free_list[10..12].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(SpaceMap::parse(&free_list).is_none());
    // No prefix of the stream other than the whole of it parses, and none
    // panics.
    for len in 0..map.len() {
        assert!(SpaceMap::parse(&map[..len]).is_none());
    }
}

#[test]
fn space_map_skips_the_free_list_and_item_bodies() {
    let (_, plain) = cluster_and_map(&objects());
    // Two free serials follow the header; the first item gains a 3-byte body.
    let mut map = plain[..12].to_vec();
    map[10..12].copy_from_slice(&2_u16.to_le_bytes());
    map.extend_from_slice(&[9, 0, 8, 0]);
    let mut items = plain[12..].to_vec();
    items[0..2].copy_from_slice(&19_u16.to_le_bytes());
    items.splice(18..18, [7, 7, 7]);
    map.extend_from_slice(&items);
    let parsed = SpaceMap::parse(&map).unwrap();
    assert_eq!(parsed.items().len(), 3);
    assert_eq!(parsed.items()[2].id, 3);
}

#[test]
fn cluster_must_match_its_map_and_a_bad_frame_loses_only_its_object() {
    let (cluster_bytes, map_bytes) = cluster_and_map(&objects());
    let map = SpaceMap::parse(&map_bytes).unwrap();
    let mut wrong_count = cluster_bytes.clone();
    wrong_count[4] = 9;
    assert!(Cluster::parse(&wrong_count, &map).is_none());
    let mut wrong_magic = cluster_bytes.clone();
    wrong_magic[0] ^= 1;
    assert!(Cluster::parse(&wrong_magic, &map).is_none());
    // The second object's frame carries another identifier.
    let second = map.items()[1].offset;
    let mut damaged = cluster_bytes.clone();
    damaged[second + 4] ^= 0xff;
    let cluster = Cluster::parse(&damaged, &map).unwrap();
    assert!(cluster.object(2).is_none());
    assert_eq!(cluster.objects().count(), 2);
    assert!(cluster.object(3).is_some());
    // A truncated stream loses the objects past the cut.
    let cut = Cluster::parse(&cluster_bytes[..second + 3], &map).unwrap();
    assert_eq!(cut.objects().count(), 1);
}

#[test]
fn type_table_rejects_what_does_not_walk() {
    let bytes = type_table(&["Alpha", "Beta"], &[1, 0]);
    assert!(TypeTable::parse(&bytes).is_some());
    for len in 0..bytes.len() {
        assert!(TypeTable::parse(&bytes[..len]).is_none(), "prefix {len}");
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(TypeTable::parse(&trailing).is_none());
    let mut bad_index = bytes.clone();
    bad_index[20 + 16] = 5;
    assert!(TypeTable::parse(&bad_index).is_none());
    let mut unequal = bytes.clone();
    unequal[14] = 9;
    assert!(TypeTable::parse(&unequal).is_none());
    let mut huge = bytes.clone();
    huge[10..12].copy_from_slice(&u16::MAX.to_le_bytes());
    huge[14..16].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(TypeTable::parse(&huge).is_none());
    let mut no_terminator = bytes.clone();
    // The first name ends at declaration offset 30 + 6.
    no_terminator[20 + 30 + 5] = b'!';
    assert!(TypeTable::parse(&no_terminator).is_none());
}
