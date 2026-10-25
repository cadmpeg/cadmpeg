// SPDX-License-Identifier: Apache-2.0
//! Bounded evidence for a failed global identity comparison.

use super::{edge_shape, refine, Data, Graph, Node};
use std::collections::BTreeMap;

pub(super) fn difference(left: &Graph, right: &Graph) -> String {
    let counts = |graph: &Graph| {
        let mut counts = BTreeMap::<String, usize>::new();
        for node in &graph.nodes {
            *counts.entry(format!("{:?}", node.kind)).or_default() += 1;
        }
        counts
    };
    let mut summary = format!(
        "nodes {} -> {}; kinds {:?} -> {:?}",
        left.nodes.len(),
        right.nodes.len(),
        counts(left),
        counts(right)
    );
    let mut candidates = Vec::with_capacity(left.nodes.len());
    for (index, node) in left.nodes.iter().enumerate() {
        let same_data = right
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, other)| node.kind == other.kind && node.data.agrees(&other.data))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if same_data.is_empty() {
            summary.push_str(&format!(
                "; no value candidate for {} {:?}",
                identity(left, index),
                node.kind
            ));
            for (target, other) in right
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, other)| node.kind == other.kind)
                .take(2)
            {
                summary.push_str(&format!(
                    "; versus {}: {}",
                    identity(right, target),
                    value_difference(&node.data, &other.data, "$".into())
                        .unwrap_or_else(|| "value shape differs".into())
                ));
            }
            return summary;
        }
        let same_edges = same_data
            .iter()
            .copied()
            .filter(|target| shape(node) == shape(&right.nodes[*target]))
            .collect::<Vec<_>>();
        if same_edges.is_empty() {
            summary.push_str(&format!(
                "; no reference-shape candidate for {} {:?}: {}",
                identity(left, index),
                node.kind,
                neighborhood(left, index)
            ));
            let mut closest = same_data;
            closest.sort_by_key(|target| {
                let other = &right.nodes[*target];
                node.incoming.len().abs_diff(other.incoming.len())
                    + node.outgoing.len().abs_diff(other.outgoing.len())
            });
            for target in closest.into_iter().take(2) {
                summary.push_str(&format!(
                    "; versus {}: {}",
                    identity(right, target),
                    neighborhood(right, target)
                ));
            }
            return summary;
        }
        candidates.push(same_edges);
    }
    let initial = candidates.clone();
    if !refine::prune(left, right, &mut candidates) {
        if let Some(index) = candidates.iter().position(Vec::is_empty) {
            summary.push_str(&format!(
                "; reference refinement rejected {} {:?}: {}",
                identity(left, index),
                left.nodes[index].kind,
                neighborhood(left, index)
            ));
            for target in initial[index].iter().take(2) {
                summary.push_str(&format!(
                    "; versus {}: {}",
                    identity(right, *target),
                    neighborhood(right, *target)
                ));
            }
        }
    } else {
        summary.push_str("; candidate values and local references agree; global aliasing differs");
    }
    summary
}

fn shape(
    node: &Node,
) -> (
    BTreeMap<Vec<super::Step>, usize>,
    BTreeMap<Vec<super::Step>, usize>,
) {
    (edge_shape(&node.outgoing), edge_shape(&node.incoming))
}

fn identity(graph: &Graph, index: usize) -> String {
    let mut keys = graph
        .identities
        .iter()
        .filter(|(_, target)| **target == index)
        .map(|(key, _)| key.as_str())
        .collect::<Vec<_>>();
    keys.sort_unstable();
    keys.into_iter()
        .next()
        .map_or_else(|| format!("node#{index}"), |key| brief(key))
}

fn neighborhood(graph: &Graph, index: usize) -> String {
    let node = &graph.nodes[index];
    let edges = |incoming: bool| {
        let edges = if incoming {
            &node.incoming
        } else {
            &node.outgoing
        };
        let details = edges
            .iter()
            .take(4)
            .map(|edge| format!("{:?} -> {}", edge.path, identity(graph, edge.node)))
            .collect::<Vec<_>>();
        format!("{} {:?}", edges.len(), details)
    };
    format!("incoming {}; outgoing {}", edges(true), edges(false))
}

fn value_difference(left: &Data, right: &Data, path: String) -> Option<String> {
    if left.agrees(right) {
        return None;
    }
    match (left, right) {
        (Data::Scalar(left), Data::Scalar(right)) => Some(format!(
            "{path}: {} -> {}",
            brief(&left.to_string()),
            brief(&right.to_string())
        )),
        (Data::Array(left), Data::Array(right)) if left.len() == right.len() => left
            .iter()
            .zip(right)
            .enumerate()
            .find_map(|(index, (left, right))| {
                value_difference(left, right, format!("{path}[{index}]"))
            }),
        (Data::Object(left, _), Data::Object(right, _)) => left
            .iter()
            .find_map(|(key, value)| {
                right.get(key).map_or_else(
                    || Some(format!("{path}.{key}: absent after write")),
                    |other| value_difference(value, other, format!("{path}.{key}")),
                )
            })
            .or_else(|| Some(format!("{path}: object fields differ"))),
        _ => Some(format!("{path}: value shape differs")),
    }
}

fn brief(text: &str) -> String {
    text.chars().take(160).collect()
}
