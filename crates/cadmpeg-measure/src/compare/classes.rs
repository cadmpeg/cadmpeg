// SPDX-License-Identifier: Apache-2.0
//! Compatibility classes of the bodies of two documents.
//!
//! Compatibility over the bodies of both documents, across and within the
//! two sides, splits them into connected classes. A class is clean when
//! every two of its bodies are compatible; a clean class pairs its bodies of
//! the two documents by multiplicity in ID order and leaves the surplus
//! unmatched. A class that is not clean is ambiguous.

use std::collections::BTreeMap;

/// One class of bodies, by index into the bodies of both documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Class {
    /// Pairs `(a, b)` of a clean class.
    Paired(Vec<(usize, usize)>),
    /// A body with no partner on the other side.
    Unmatched(usize),
    /// A body with no comparable quantity.
    Unmeasured(usize),
    /// The members of a class that is not clean.
    Ambiguous(Vec<usize>),
}

/// The root of `index` in a union-find forest, halving the path to it.
fn root(parent: &mut [usize], mut index: usize) -> usize {
    while parent[index] != index {
        parent[index] = parent[parent[index]];
        index = parent[index];
    }
    index
}

/// The classes of the bodies whose sides are `sides` (`false` for the first
/// document), whose comparability is `measurable`, connected by the
/// compatible pairs `edges` and tested for cleanness by `compatible`.
/// Indices of each side are in ID order.
pub(super) fn classes(
    sides: &[bool],
    measurable: &[bool],
    edges: &[(usize, usize)],
    compatible: &dyn Fn(usize, usize) -> bool,
) -> Vec<Class> {
    let count = sides.len();
    let mut parent: Vec<usize> = (0..count).collect();
    let mut found = Vec::new();
    for (index, measured) in measurable.iter().enumerate() {
        if !measured {
            found.push(Class::Unmeasured(index));
        }
    }
    for &(first, second) in edges {
        if measurable[first] && measurable[second] {
            let (from, to) = (root(&mut parent, first), root(&mut parent, second));
            if from != to {
                parent[to.max(from)] = to.min(from);
            }
        }
    }
    let mut components: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut order = Vec::new();
    for (index, measured) in measurable.iter().enumerate() {
        if !measured {
            continue;
        }
        let label = root(&mut parent, index);
        let members = components.entry(label).or_default();
        if members.is_empty() {
            order.push(label);
        }
        members.push(index);
    }
    for label in order {
        let Some(members) = components.remove(&label) else {
            continue;
        };
        let clean = members.iter().enumerate().all(|(position, first)| {
            members
                .iter()
                .skip(position + 1)
                .all(|second| compatible(*first, *second))
        });
        if !clean {
            found.push(Class::Ambiguous(members));
            continue;
        }
        let a: Vec<usize> = members
            .iter()
            .copied()
            .filter(|member| !sides[*member])
            .collect();
        let b: Vec<usize> = members
            .iter()
            .copied()
            .filter(|member| sides[*member])
            .collect();
        let paired: Vec<(usize, usize)> = a.iter().copied().zip(b.iter().copied()).collect();
        let surplus = if a.len() > b.len() {
            &a[b.len()..]
        } else {
            &b[a.len()..]
        };
        found.extend(surplus.iter().map(|member| Class::Unmatched(*member)));
        if !paired.is_empty() {
            found.push(Class::Paired(paired));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::Class;

    /// The classes under `compatible`, connected by every compatible pair.
    fn classes(
        sides: &[bool],
        measurable: &[bool],
        compatible: &dyn Fn(usize, usize) -> bool,
    ) -> Vec<Class> {
        let count = sides.len();
        let edges: Vec<(usize, usize)> = (0..count)
            .flat_map(|first| (first + 1..count).map(move |second| (first, second)))
            .filter(|(first, second)| compatible(*first, *second))
            .collect();
        super::classes(sides, measurable, &edges, compatible)
    }

    #[test]
    fn coincident_bodies_pair_by_multiplicity() {
        let sides = [false, false, true, true];
        let found = classes(&sides, &[true; 4], &|_, _| true);
        assert_eq!(found, [Class::Paired(vec![(0, 2), (1, 3)])]);
    }

    #[test]
    fn a_surplus_body_is_unmatched() {
        let sides = [false, false, true];
        let found = classes(&sides, &[true; 3], &|_, _| true);
        assert_eq!(found, [Class::Unmatched(1), Class::Paired(vec![(0, 2)])]);
    }

    #[test]
    fn a_chain_that_is_not_transitive_is_ambiguous() {
        let sides = [false, true, true];
        let chain = |first: usize, second: usize| first.abs_diff(second) == 1;
        let found = classes(&sides, &[true; 3], &chain);
        assert_eq!(found, [Class::Ambiguous(vec![0, 1, 2])]);
    }

    #[test]
    fn unmeasurable_and_incompatible_bodies_stay_apart() {
        let sides = [false, true, true];
        let found = classes(&sides, &[false, true, true], &|_, _| false);
        assert_eq!(
            found,
            [
                Class::Unmeasured(0),
                Class::Unmatched(1),
                Class::Unmatched(2)
            ]
        );
    }
}
