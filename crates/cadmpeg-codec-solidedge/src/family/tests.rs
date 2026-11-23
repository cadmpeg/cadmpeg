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
fn record_trailer_word_is_not_an_entry() {
    let mut table = member_table(&[("alpha", &[7])]);
    let last = table.len() - 4;
    table[last] = 1;
    assert_eq!(
        listed_occurrences(&table, "alpha")
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        [7]
    );
}

#[test]
fn record_whose_count_disagrees_with_its_entries_lists_nothing() {
    let mut table = member_table(&[("alpha", &[7])]);
    let count = table.len() - 4 - 12 - 4;
    table[count] = 2;
    assert_eq!(listed_occurrences(&table, "alpha"), None);
}
