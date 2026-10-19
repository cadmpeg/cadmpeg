// SPDX-License-Identifier: Apache-2.0
//! Distinct-membership checks for ordered feature collections.

use std::collections::HashSet;
use std::hash::Hash;

/// Collections up to this length compare members pairwise instead of hashing them.
const PAIRWISE_DISTINCT_MAX: usize = 16;

/// Whether the included values are pairwise distinct.
pub(super) fn distinct<T: Eq + Hash>(values: &[T], include: impl Fn(&T) -> bool) -> bool {
    if values.len() <= PAIRWISE_DISTINCT_MAX {
        let mut included = [None; PAIRWISE_DISTINCT_MAX];
        for (count, value) in values.iter().filter(|value| include(value)).enumerate() {
            if included[..count].contains(&Some(value)) {
                return false;
            }
            included[count] = Some(value);
        }
        return true;
    }
    let mut seen = HashSet::with_capacity(values.len());
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

#[cfg(test)]
mod tests {
    use super::{distinct, PAIRWISE_DISTINCT_MAX};

    #[test]
    fn distinct_matches_on_both_sides_of_the_pairwise_bound() {
        for len in [
            0,
            1,
            2,
            PAIRWISE_DISTINCT_MAX,
            PAIRWISE_DISTINCT_MAX + 1,
            200,
        ] {
            let unique: Vec<_> = (0..len)
                .map(|value| format!("test:model:point#{value}"))
                .collect();
            assert!(distinct(&unique, |_| true), "{len}");
            if len < 2 {
                continue;
            }
            let mut repeated = unique.clone();
            repeated[len - 1] = repeated[0].clone();
            assert!(!distinct(&repeated, |_| true), "{len}");
            let first = repeated[0].clone();
            assert!(distinct(&repeated, |value| *value != first), "{len}");
        }
    }
}
