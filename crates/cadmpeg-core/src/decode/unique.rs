// SPDX-License-Identifier: Apache-2.0
//! One-table indexes that exclude every repeated key.

use std::collections::HashMap;
use std::hash::Hash;

/// Indexes `entries` by key and drops every key that occurs more than once.
///
/// A repeated key becomes a tombstone, so a third occurrence cannot restore
/// it. The result holds exactly the keys that occur once.
pub fn unique_index<K: Eq + Hash, V>(entries: impl IntoIterator<Item = (K, V)>) -> HashMap<K, V> {
    let mut table = HashMap::<K, Option<V>>::new();
    for (key, value) in entries {
        match table.get_mut(&key) {
            Some(previous) => *previous = None,
            None => {
                // discarded-value: the key was absent before this insertion.
                let _ = table.insert(key, Some(value));
            }
        }
    }
    table
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key, value)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::unique_index;

    #[test]
    fn unique_index_removes_all_duplicate_occurrences() {
        let table = unique_index([(1_u32, 2), (1, 3), (1, 4), (2, 5)]);
        assert_eq!(table.len(), 1);
        assert_eq!(table.get(&2), Some(&5));
        assert!(!table.contains_key(&1));
    }

    #[test]
    fn unique_index_keeps_a_single_record() {
        assert_eq!(unique_index([(7_u32, 7)]).get(&7), Some(&7));
    }
}
