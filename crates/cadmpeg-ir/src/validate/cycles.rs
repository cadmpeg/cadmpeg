// SPDX-License-Identifier: Apache-2.0
//! The cycles of a directed graph of record ids, for every rule that
//! refuses or warns on records that reach themselves.

use std::collections::{BTreeMap, BTreeSet};

/// The strongly connected components of `graph` that hold a cycle, each
/// sorted, by Kosaraju's two passes, each walked without recursion.
pub(super) fn cycles<'a>(graph: &BTreeMap<&'a str, BTreeSet<&'a str>>) -> Vec<Vec<&'a str>> {
    let mut order = Vec::with_capacity(graph.len());
    let mut visited = BTreeSet::new();
    for &start in graph.keys() {
        if !visited.insert(start) {
            continue;
        }
        let mut stack = vec![(start, graph[start].iter())];
        while let Some((node, next)) = stack.last_mut() {
            if let Some(&target) = next.next() {
                if graph.contains_key(target) && visited.insert(target) {
                    stack.push((target, graph[target].iter()));
                }
            } else {
                order.push(*node);
                stack.pop();
            }
        }
    }
    let mut reverse: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (&from, targets) in graph {
        for &to in targets {
            reverse.entry(to).or_default().push(from);
        }
    }
    let mut assigned = BTreeSet::new();
    let mut found = Vec::new();
    for &root in order.iter().rev() {
        if !assigned.insert(root) {
            continue;
        }
        let mut component = vec![root];
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            for &from in reverse.get(node).into_iter().flatten() {
                if assigned.insert(from) {
                    component.push(from);
                    pending.push(from);
                }
            }
        }
        let cyclic = component.len() > 1 || graph[root].contains(root);
        if cyclic {
            component.sort_unstable();
            found.push(component);
        }
    }
    found
}

/// The nodes of `graph` that lie on a cycle.
pub(super) fn on_cycles<'a>(graph: &BTreeMap<&'a str, BTreeSet<&'a str>>) -> BTreeSet<&'a str> {
    cycles(graph).into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::on_cycles;

    fn graph<'a>(edges: &[(&'a str, &'a str)]) -> BTreeMap<&'a str, BTreeSet<&'a str>> {
        let mut graph: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for &(from, to) in edges {
            graph.entry(from).or_default().insert(to);
            graph.entry(to).or_default();
        }
        graph
    }

    #[test]
    fn a_node_that_enters_a_cycle_already_walked_is_on_it() {
        let edges = [("a", "b"), ("b", "c"), ("c", "a"), ("a", "d"), ("d", "b")];
        assert_eq!(
            on_cycles(&graph(&edges)),
            BTreeSet::from(["a", "b", "c", "d"])
        );
    }

    #[test]
    fn a_node_that_only_reaches_a_cycle_is_off_it() {
        let edges = [("e", "a"), ("a", "b"), ("b", "a"), ("b", "f")];
        assert_eq!(on_cycles(&graph(&edges)), BTreeSet::from(["a", "b"]));
    }

    #[test]
    fn paths_that_part_and_meet_again_have_no_cycle_and_a_self_edge_is_one() {
        let rejoined = [("a", "b"), ("a", "c"), ("b", "d"), ("c", "d")];
        assert!(on_cycles(&graph(&rejoined)).is_empty());
        assert_eq!(on_cycles(&graph(&[("a", "a")])), BTreeSet::from(["a"]));
    }

    #[test]
    fn an_edge_to_a_node_the_graph_does_not_hold_is_followed_nowhere() {
        let graph = BTreeMap::from([("a", BTreeSet::from(["missing"]))]);
        assert!(on_cycles(&graph).is_empty());
    }
}
