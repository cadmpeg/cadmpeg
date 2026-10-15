// SPDX-License-Identifier: Apache-2.0
//! Distinct-membership checks for ordered feature collections.

use std::collections::HashSet;
use std::hash::Hash;

/// Whether the included values are pairwise distinct.
pub(super) fn distinct<T: Eq + Hash>(values: &[T], include: impl Fn(&T) -> bool) -> bool {
    let mut seen = HashSet::new();
    values
        .iter()
        .filter(|value| include(value))
        .all(|value| seen.insert(value))
}

/// Append `value` unless an equal member is present, and return whether it was appended.
pub(super) fn insert<T: PartialEq>(values: &mut Vec<T>, value: T) -> bool {
    if values.contains(&value) {
        return false;
    }
    values.push(value);
    true
}
