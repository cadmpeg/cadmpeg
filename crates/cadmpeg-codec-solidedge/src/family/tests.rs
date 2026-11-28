// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{active_name, select};
use crate::test_support::{family_info, member_table, member_table_of, RecordSpec};

fn listed(table: &[u8], name: &str) -> Option<std::collections::BTreeSet<u32>> {
    select(table, name).map(|selection| selection.listed)
}

#[test]
fn active_member_is_the_counted_utf16_name() {
    assert_eq!(active_name(&family_info("alpha")).as_deref(), Some("alpha"));
    let mut cut = family_info("alpha");
    cut.truncate(cut.len() - 2);
    assert_eq!(active_name(&cut), None);
    assert_eq!(active_name(&family_info("")), None);
}

#[test]
fn record_lists_its_entries_by_member_name() {
    let table = member_table(&[("alpha", &[7, 9]), ("beta", &[]), ("gamma", &[9])]);
    assert_eq!(
        listed(&table, "alpha")
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        [7, 9]
    );
    assert!(listed(&table, "BETA").unwrap().is_empty());
    assert_eq!(
        listed(&table, "gamma")
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        [9]
    );
    assert_eq!(listed(&table, "delta"), None);
}

#[test]
fn record_with_a_prefix_list_skips_it() {
    let mut table = member_table(&[("alpha", &[7])]);
    let at = table
        .windows(4)
        .position(|window| window == b"QQQ\0")
        .unwrap()
        + 8
        + 4
        + 12;
    // The prefix count follows the name; one entry adds four bytes.
    table[at..at + 4].copy_from_slice(&1_u32.to_le_bytes());
    table.splice(at + 4..at + 4, [9, 0, 0, 0]);
    assert_eq!(
        listed(&table, "alpha")
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        [7]
    );
}

#[test]
fn record_whose_count_exceeds_its_bytes_lists_nothing() {
    let mut table = member_table(&[("alpha", &[7])]);
    let count = table.len() - 8 - (4 + 13 * 8 + 3) - 4;
    table[count] = 2;
    assert_eq!(listed(&table, "alpha"), None);
}

#[test]
fn record_without_the_two_blocks_lists_nothing() {
    let mut table = member_table(&[("alpha", &[7])]);
    let at = table
        .windows(4)
        .position(|window| window == b"QQQ\0")
        .unwrap()
        + 8
        + 4
        + 12
        + 4
        + 12;
    table[at] = 0;
    assert_eq!(listed(&table, "alpha"), None);
}

fn two(first_word: u32, excluded: &[u32]) -> Vec<u8> {
    member_table_of(&[
        RecordSpec {
            name: "alpha",
            excluded: &[],
            listed: &[7, 8, 9],
            word: first_word,
        },
        RecordSpec {
            name: "beta",
            excluded,
            listed: &[7, 8, 9],
            word: 3,
        },
    ])
}

fn ids(set: &std::collections::BTreeSet<u32>) -> Vec<u32> {
    set.iter().copied().collect()
}

#[test]
fn first_member_active_excludes_its_own_first_list() {
    let selection = select(&two(3, &[8]), "alpha").unwrap();
    assert_eq!(ids(&selection.listed), [7, 8, 9]);
    assert!(selection.excluded.is_empty());
    assert!(!selection.unresolved);
}

#[test]
fn first_word_without_the_active_bit_selects_the_second_record() {
    let selection = select(&two(1, &[8]), "alpha").unwrap();
    assert_eq!(ids(&selection.excluded), [8]);
    assert_eq!(ids(&selection.listed), [7, 8, 9]);
    assert!(!selection.unresolved);
    let selection = select(&two(1, &[7, 9]), "alpha").unwrap();
    assert_eq!(ids(&selection.excluded), [7, 9]);
}

#[test]
fn named_second_member_is_read_whatever_the_first_word() {
    let selection = select(&two(1, &[9]), "beta").unwrap();
    assert_eq!(ids(&selection.excluded), [9]);
    assert!(!selection.unresolved);
}

#[test]
fn first_word_without_the_active_bit_among_three_records_is_unresolved() {
    let table = member_table_of(&[
        RecordSpec {
            name: "alpha",
            excluded: &[],
            listed: &[7],
            word: 1,
        },
        RecordSpec {
            name: "beta",
            excluded: &[7],
            listed: &[7],
            word: 3,
        },
        RecordSpec {
            name: "gamma",
            excluded: &[],
            listed: &[7],
            word: 3,
        },
    ]);
    let selection = select(&table, "alpha").unwrap();
    assert!(selection.unresolved);
    assert!(selection.excluded.is_empty());
}

#[test]
fn record_without_a_trailing_word_is_unreadable() {
    let mut table = two(3, &[]);
    table.truncate(table.len() - 8);
    assert_eq!(select(&table, "beta").map(|s| s.unresolved), None);
}
