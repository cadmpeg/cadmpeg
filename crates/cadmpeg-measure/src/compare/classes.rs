// SPDX-License-Identifier: Apache-2.0
//! Compatibility classes of the bodies of two documents.
//!
//! Compatibility over the bodies of both documents, across and within the
//! two sides, splits them into connected classes. A class is clean when
//! every two of its bodies are compatible; a clean class pairs its bodies of
//! the two documents by multiplicity in ID order and leaves the surplus
//! unmatched. A class that is not clean is ambiguous.

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

/// The classes of the bodies whose sides are `sides` (`false` for the first
/// document), whose comparability is `measurable`, under `compatible`.
/// Indices of each side are in ID order.
pub(super) fn classes(
    sides: &[bool],
    measurable: &[bool],
    compatible: &dyn Fn(usize, usize) -> bool,
) -> Vec<Class> {
    let count = sides.len();
    let mut component: Vec<usize> = (0..count).collect();
    let mut found = Vec::new();
    for (index, measured) in measurable.iter().enumerate() {
        if !measured {
            found.push(Class::Unmeasured(index));
        }
    }
    for first in 0..count {
        for second in first + 1..count {
            if measurable[first] && measurable[second] && compatible(first, second) {
                let (from, to) = (component[first], component[second]);
                if from != to {
                    for label in &mut component {
                        if *label == to {
                            *label = from;
                        }
                    }
                }
            }
        }
    }
    let mut seen = Vec::new();
    for index in 0..count {
        if !measurable[index] || seen.contains(&component[index]) {
            continue;
        }
        let label = component[index];
        seen.push(label);
        let members: Vec<usize> = (index..count)
            .filter(|member| measurable[*member] && component[*member] == label)
            .collect();
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
    use super::{classes, Class};

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
