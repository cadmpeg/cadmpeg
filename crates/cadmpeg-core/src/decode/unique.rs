// SPDX-License-Identifier: Apache-2.0
//! One-table indexes that mark every repeated key.

use std::collections::HashMap;
use std::hash::Hash;

/// Indexes `entries` by key, mapping a key that occurs more than once to `None`.
///
/// A repeated key stays in the table as `None`, so a third occurrence cannot
/// restore it and a caller can tell a repeated key from an absent one.
pub fn unique_index<K: Eq + Hash, V>(
    entries: impl IntoIterator<Item = (K, V)>,
) -> HashMap<K, Option<V>> {
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
}

#[cfg(test)]
mod tests {
    use super::unique_index;

    #[test]
    fn unique_index_marks_every_repeated_key() {
        let table = unique_index([(1_u32, 2), (1, 3), (1, 4), (2, 5)]);
        assert_eq!(table.len(), 2);
        assert_eq!(table.get(&2), Some(&Some(5)));
        assert_eq!(table.get(&1), Some(&None));
    }

    #[test]
    fn unique_index_keeps_a_single_record() {
        assert_eq!(unique_index([(7_u32, 7)]).get(&7), Some(&Some(7)));
    }
}
