// SPDX-License-Identifier: Apache-2.0
//! Necessary edge constraints reduce candidates without classifying numeric data.

use super::{Edge, Graph};

fn compatible_edges(
    left: &[Edge],
    right: &[Edge],
    candidates: &[Vec<usize>],
    source: usize,
    target: usize,
) -> bool {
    let pair = |a: &Edge, b: &Edge| {
        a.path == b.path
            && (a.node == source) == (b.node == target)
            && candidates[a.node].binary_search(&b.node).is_ok()
    };
    left.iter().all(|a| right.iter().any(|b| pair(a, b)))
        && right.iter().all(|b| left.iter().any(|a| pair(a, b)))
}

/// Remove only pairs that no complete graph bijection could use.
///
/// Candidate relationships remain pairwise: numeric agreement is deliberately
/// not a transitive equivalence relation and must never become a color/hash.
/// Each labeled neighbor needs a surviving counterpart in both directions.
/// This is only pruning; final injectivity, multiplicities and connectivity
/// remain enforced by the complete mapping search.
pub(super) fn prune(left: &Graph, right: &Graph, candidates: &mut [Vec<usize>]) -> bool {
    loop {
        let previous = candidates.to_vec();
        let mut changed = false;
        for (source, possible) in candidates.iter_mut().enumerate() {
            possible.retain(|&target| {
                let a = &left.nodes[source];
                let b = &right.nodes[target];
                compatible_edges(&a.outgoing, &b.outgoing, &previous, source, target)
                    && compatible_edges(&a.incoming, &b.incoming, &previous, source, target)
            });
            if possible.is_empty() {
                return false;
            }
            changed |= possible.len() != previous[source].len();
        }
        if !changed {
            return true;
        }
    }
}
