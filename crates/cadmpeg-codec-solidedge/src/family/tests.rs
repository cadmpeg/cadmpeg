// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{active_name, listed_occurrences};
use crate::test_support::{family_info, member_table};

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
        listed_occurrences(&table, "alpha")
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        [7, 9]
    );
    assert!(listed_occurrences(&table, "BETA").unwrap().is_empty());
    assert_eq!(
        listed_occurrences(&table, "gamma")
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        [9]
    );
    assert_eq!(listed_occurrences(&table, "delta"), None);
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
        listed_occurrences(&table, "alpha")
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
    assert_eq!(listed_occurrences(&table, "alpha"), None);
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
    assert_eq!(listed_occurrences(&table, "alpha"), None);
}
