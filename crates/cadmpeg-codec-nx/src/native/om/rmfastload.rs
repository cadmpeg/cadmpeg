// SPDX-License-Identifier: Apache-2.0
//! Semantic tests for `RMFastLoad` membership identities.

use super::RmFastLoadObjectId;
use std::collections::HashMap;

fn assign_with_counts(entries: &mut [RmFastLoadObjectId]) {
    let mut counts = HashMap::<u32, usize>::new();
    for entry in entries.iter() {
        *counts.entry(entry.value).or_default() += 1;
    }
    super::assign_rmfastload_object_id_identities(entries, &counts);
}

#[test]
fn value_identity_ignores_member_order() {
    let make_entries = |values: &[u32]| {
        values
            .iter()
            .enumerate()
            .map(|(ordinal, value)| RmFastLoadObjectId {
                id: format!("nx:test:rmfastload-object-id#{ordinal}"),
                table: "nx:rmfastload:object-id-table#0".into(),
                ordinal: ordinal as u32,
                value: *value,
                stable_identity: None,
                source_offset: ordinal as u64,
            })
            .collect::<Vec<_>>()
    };
    let mut first = make_entries(&[17, 23]);
    let mut reordered = make_entries(&[23, 17]);
    assign_with_counts(&mut first);
    assign_with_counts(&mut reordered);

    for value in [17, 23] {
        let first_identity = first
            .iter()
            .find(|entry| entry.value == value)
            .and_then(|entry| entry.stable_identity.as_deref());
        let reordered_identity = reordered
            .iter()
            .find(|entry| entry.value == value)
            .and_then(|entry| entry.stable_identity.as_deref());
        assert_eq!(first_identity, reordered_identity);
        let expected = format!("nx:rmfastload:object-id-table#0:value#{value}");
        assert_eq!(first_identity, Some(expected.as_str()));
    }
}

#[test]
fn duplicate_values_have_no_stable_identity() {
    let mut entries = [
        RmFastLoadObjectId {
            id: "nx:test:rmfastload-object-id#0".into(),
            table: "nx:rmfastload:object-id-table#0".into(),
            ordinal: 0,
            value: 17,
            stable_identity: None,
            source_offset: 0,
        },
        RmFastLoadObjectId {
            id: "nx:test:rmfastload-object-id#1".into(),
            table: "nx:rmfastload:object-id-table#0".into(),
            ordinal: 1,
            value: 17,
            stable_identity: None,
            source_offset: 4,
        },
    ];
    assign_with_counts(&mut entries);
    assert!(entries.iter().all(|entry| entry.stable_identity.is_none()));
}
