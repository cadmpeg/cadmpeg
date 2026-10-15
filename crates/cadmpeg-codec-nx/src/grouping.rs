// SPDX-License-Identifier: Apache-2.0
//! Key grouping shared by the native and Parasolid record tiers.

use std::collections::BTreeMap;

/// Groups pairs by key, keeping source order within each group.
pub(crate) fn group_pairs<K: Ord, V>(
    pairs: impl IntoIterator<Item = (K, V)>,
) -> BTreeMap<K, Vec<V>> {
    let mut groups = BTreeMap::<K, Vec<V>>::new();
    for (key, value) in pairs {
        groups.entry(key).or_default().push(value);
    }
    groups
}
