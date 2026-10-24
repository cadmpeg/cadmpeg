// SPDX-License-Identifier: Apache-2.0
//! Traversal order of the oriented edge uses of one face wire.

use std::collections::HashMap;
use std::hash::Hash;

/// Candidate edge uses that the traversal search of one wire examines.
const MAX_WIRE_TRAVERSAL_STEPS: usize = 1 << 16;

/// Serialized positions of the oriented edge uses in traversal order, where
/// use `i` runs from `ends[i].0` to `ends[i].1`.
///
/// The traversal starts at position 0. A continuous serialized order is the
/// traversal. Otherwise the traversal is the unique closed chain that uses
/// each edge use once; with no such chain, several, or a search that examines
/// more candidate uses than its step limit, the serialized order stays.
pub(super) fn traversal_order<K: Eq + Hash>(ends: &[(K, K)]) -> Vec<usize> {
    let serialized = || (0..ends.len()).collect();
    if is_continuous(ends) {
        return serialized();
    }
    unique_closed_chain(ends).unwrap_or_else(serialized)
}

fn is_continuous<K: PartialEq>(ends: &[(K, K)]) -> bool {
    ends.iter()
        .zip(ends.iter().cycle().skip(1))
        .all(|(current, next)| current.1 == next.0)
}

fn unique_closed_chain<K: Eq + Hash>(ends: &[(K, K)]) -> Option<Vec<usize>> {
    let count = ends.len();
    let mut uses_from: HashMap<&K, Vec<usize>> = HashMap::new();
    for (position, (start, _)) in ends.iter().enumerate() {
        uses_from.entry(start).or_default().push(position);
    }
    let candidates = |tip: usize| uses_from.get(&ends[tip].1).map_or(&[][..], Vec::as_slice);
    let mut used = vec![false; ends.len()];
    *used.first_mut()? = true;
    let mut path = vec![0_usize];
    let mut cursors = vec![0_usize];
    let mut found = None;
    let mut steps = 0_usize;
    while let (Some(&tip), Some(cursor)) = (path.last(), cursors.last_mut()) {
        let next = if path.len() == count {
            if ends[tip].1 == ends[0].0 {
                if found.is_some() {
                    return None;
                }
                found = Some(path.clone());
            }
            None
        } else {
            let mut next = None;
            for &candidate in &candidates(tip)[*cursor..] {
                steps += 1;
                if steps > MAX_WIRE_TRAVERSAL_STEPS {
                    return None;
                }
                *cursor += 1;
                if !used[candidate] {
                    next = Some(candidate);
                    break;
                }
            }
            next
        };
        match next {
            Some(candidate) => {
                used[candidate] = true;
                path.push(candidate);
                cursors.push(0);
            }
            None => {
                if let Some(last) = path.pop() {
                    used[last] = false;
                }
                cursors.pop();
            }
        }
    }
    found
}

#[cfg(test)]
mod tests;
