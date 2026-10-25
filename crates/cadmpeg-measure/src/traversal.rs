// SPDX-License-Identifier: Apache-2.0
//! Ring traversal by oriented vertex identity.
//!
//! A ring lists its coedges in traversal order by contract. When every
//! junction of the listed order is continuous, the listed order is the
//! traversal and no search runs. Otherwise the coedges are the edges of a
//! directed multigraph on vertices, and a depth-first search from the first
//! listed coedge finds the closed traversals that use every coedge once. One
//! traversal is the ring's order; none or several make the ring unavailable.

use crate::report::ListedOrder;

/// One closed traversal of a ring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Traversal {
    /// Listed indices in traversal order, starting at the first listed
    /// coedge.
    pub(crate) order: Vec<usize>,
    /// The listed order against the traversal.
    pub(crate) listed: ListedOrder,
}

/// Why a ring has no traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TraversalError {
    /// No closed traversal uses every coedge once.
    Failed,
    /// Several closed traversals exist.
    Ambiguous,
    /// The search ran out of steps.
    Exhausted,
}

/// Steps of one ring traversal search.
const MAX_RING_CHAIN_STEPS: usize = 1 << 16;

/// The traversal of a ring whose coedges run from `ends[i].0` to `ends[i].1`.
pub(crate) fn traverse<K: PartialEq>(ends: &[(K, K)]) -> Result<Traversal, TraversalError> {
    let mut steps = 0;
    traverse_within(ends, MAX_RING_CHAIN_STEPS, &mut steps)
}

/// The traversal of a ring with a search of at most `steps` steps, and the
/// work it took: one unit per listed coedge for the continuity test plus
/// one per search step.
pub(crate) fn traverse_counted<K: PartialEq>(
    ends: &[(K, K)],
    steps: usize,
) -> (Result<Traversal, TraversalError>, usize) {
    let mut taken = 0;
    let result = traverse_within(ends, steps.min(MAX_RING_CHAIN_STEPS), &mut taken);
    (result, ends.len() + taken)
}

fn traverse_within<K: PartialEq>(
    ends: &[(K, K)],
    max_steps: usize,
    steps: &mut usize,
) -> Result<Traversal, TraversalError> {
    let count = ends.len();
    if count == 0 {
        return Err(TraversalError::Failed);
    }
    let continuous = (0..count).all(|index| ends[index].1 == ends[(index + 1) % count].0);
    if continuous {
        return Ok(Traversal {
            order: (0..count).collect(),
            listed: ListedOrder::Traversal,
        });
    }
    let mut used = vec![false; ends.len()];
    used[0] = true;
    let mut path = vec![0_usize];
    let mut cursors = vec![0_usize];
    let mut found: Vec<Vec<usize>> = Vec::new();
    loop {
        *steps += 1;
        if *steps > max_steps {
            return Err(TraversalError::Exhausted);
        }
        let Some(&tip) = path.last() else {
            break;
        };
        if path.len() == count && ends[tip].1 == ends[0].0 {
            found.push(path.clone());
            if found.len() == 2 {
                break;
            }
        }
        let next = if path.len() == count {
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
                if path.len() == 1 {
                    break;
                }
                if let Some(last) = path.pop() {
                    used[last] = false;
                }
                cursors.pop();
            }
        }
    }
    match found.len() {
        0 => Err(TraversalError::Failed),
        1 => {
            let order = found.swap_remove(0);
            let reversed = order
                .iter()
                .skip(1)
                .rev()
                .enumerate()
                .all(|(position, index)| *index == position + 1);
            let listed = if reversed {
                ListedOrder::Reversed
            } else {
                ListedOrder::Other
            };
            Ok(Traversal { order, listed })
        }
        _ => Err(TraversalError::Ambiguous),
    }
}

#[cfg(test)]
mod tests {
    use super::{traverse, traverse_counted, traverse_within, TraversalError};
    use crate::report::ListedOrder;

    #[test]
    fn a_continuous_listed_order_is_the_traversal() {
        let ends = [("a", "b"), ("b", "c"), ("c", "a")];
        let traversal = traverse(&ends).unwrap();
        assert_eq!(traversal.order, [0, 1, 2]);
        assert_eq!(traversal.listed, ListedOrder::Traversal);
    }

    #[test]
    fn the_freecad_cut_face_pattern_chains_in_another_order() {
        // A (0,20,10)→(0,0,10), B (30,20,10)→(0,20,10), C (0,0,10)→(30,0,10),
        // D (30,0,10)→(30,20,10), listed A, B, C, D.
        let p = |x: i32, y: i32| (x, y);
        let ends = [
            (p(0, 20), p(0, 0)),
            (p(30, 20), p(0, 20)),
            (p(0, 0), p(30, 0)),
            (p(30, 0), p(30, 20)),
        ];
        let traversal = traverse(&ends).unwrap();
        assert_eq!(traversal.order, [0, 2, 3, 1]);
        assert_eq!(traversal.listed, ListedOrder::Other);
    }

    #[test]
    fn a_reversed_listing_is_reported() {
        // Coedges 1, 2, 3, 4 whose traversal is 1, 4, 3, 2.
        let ends = [("a", "b"), ("d", "a"), ("c", "d"), ("b", "c")];
        let traversal = traverse(&ends).unwrap();
        assert_eq!(traversal.order, [0, 3, 2, 1]);
        assert_eq!(traversal.listed, ListedOrder::Reversed);
    }

    #[test]
    fn a_seam_loop_chains_uniquely_from_any_listing() {
        // bottom circle v→v, seam v→w, top circle w→w, seam w→v.
        let listings = [
            [("v", "v"), ("v", "w"), ("w", "w"), ("w", "v")],
            [("w", "v"), ("v", "v"), ("v", "w"), ("w", "w")],
            [("w", "v"), ("w", "w"), ("v", "w"), ("v", "v")],
        ];
        for ends in listings {
            let traversal = traverse(&ends).unwrap();
            assert_eq!(traversal.order.len(), 4);
            for pair in traversal.order.windows(2) {
                assert_eq!(ends[pair[0]].1, ends[pair[1]].0);
            }
        }
    }

    #[test]
    fn a_continuous_ring_with_several_orders_keeps_the_listed_order() {
        // Four seam coedges of a whole torus at one vertex.
        let ends = [("v", "v"), ("v", "v"), ("v", "v"), ("v", "v")];
        let traversal = traverse(&ends).unwrap();
        assert_eq!(traversal.order, [0, 1, 2, 3]);
        assert_eq!(traversal.listed, ListedOrder::Traversal);
    }

    #[test]
    fn parallel_coedges_out_of_order_are_ambiguous() {
        let ends = [("x", "y"), ("x", "y"), ("y", "x"), ("y", "x")];
        let reordered = [ends[0], ends[1], ends[2], ends[3]];
        assert_eq!(traverse(&reordered), Err(TraversalError::Ambiguous));
    }

    #[test]
    fn an_unbalanced_vertex_fails() {
        let ends = [("a", "b"), ("b", "c"), ("c", "d")];
        assert_eq!(traverse(&ends), Err(TraversalError::Failed));
    }

    #[test]
    fn the_search_stops_at_its_step_cap() {
        let ends = [("x", "y"), ("x", "y"), ("y", "x"), ("y", "x")];
        let mut steps = 0;
        assert_eq!(
            traverse_within(&ends, 2, &mut steps),
            Err(TraversalError::Exhausted)
        );
        assert_eq!(steps, 3);
    }

    #[test]
    fn a_counted_traversal_charges_the_listing_and_the_search() {
        let continuous = [("a", "b"), ("b", "c"), ("c", "a")];
        assert_eq!(traverse_counted(&continuous, 0).1, 3);
        let reversed = [("a", "b"), ("d", "a"), ("c", "d"), ("b", "c")];
        let (result, work) = traverse_counted(&reversed, usize::MAX);
        assert_eq!(result.unwrap().listed, ListedOrder::Reversed);
        assert!(work > reversed.len());
        let (result, work) = traverse_counted(&reversed, 1);
        assert_eq!(result, Err(TraversalError::Exhausted));
        assert_eq!(work, reversed.len() + 2);
    }
}
