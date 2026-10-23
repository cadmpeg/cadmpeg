// SPDX-License-Identifier: Apache-2.0
//! Traversal order of the oriented edge uses of one face wire.

/// Search steps for the traversal of one wire.
const MAX_WIRE_TRAVERSAL_STEPS: usize = 1 << 16;

/// Serialized positions of the oriented edge uses in traversal order, where
/// use `i` runs from `ends[i].0` to `ends[i].1`.
///
/// The traversal starts at position 0. A continuous serialized order is the
/// traversal. Otherwise the traversal is the unique closed chain that uses
/// each edge use once; with no such chain, several, or a search longer than
/// its step limit, the serialized order stays.
pub(super) fn traversal_order<K: PartialEq>(ends: &[(K, K)]) -> Vec<usize> {
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

fn unique_closed_chain<K: PartialEq>(ends: &[(K, K)]) -> Option<Vec<usize>> {
    let count = ends.len();
    let mut used = vec![false; ends.len()];
    *used.first_mut()? = true;
    let mut path = vec![0_usize];
    let mut cursors = vec![0_usize];
    let mut found = None;
    let mut steps = 0_usize;
    while let Some(&tip) = path.last() {
        steps += 1;
        if steps > MAX_WIRE_TRAVERSAL_STEPS {
            return None;
        }
        let next = if path.len() == count {
            if ends[tip].1 == ends[0].0 {
                if found.is_some() {
                    return None;
                }
                found = Some(path.clone());
            }
            None
        } else {
            let from = cursors.last().copied().unwrap_or(count);
            (from..count).find(|&candidate| !used[candidate] && ends[candidate].0 == ends[tip].1)
        };
        match next {
            Some(candidate) => {
                if let Some(cursor) = cursors.last_mut() {
                    *cursor = candidate + 1;
                }
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
