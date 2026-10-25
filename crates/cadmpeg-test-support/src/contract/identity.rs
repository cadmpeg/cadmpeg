// SPDX-License-Identifier: Apache-2.0
//! Identity-insensitive equality with one bijection for the complete written graph.
use cadmpeg_ir::{ArenaName, CadIr};
use serde_json::Value;
mod diagnostic;
pub(super) mod mark;
mod refine;
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug)]
enum Data {
    Scalar(Value),
    Reference,
    Array(Vec<Self>),
    Object(BTreeMap<String, Self>, usize),
}
impl Data {
    fn agrees(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Scalar(a), Self::Scalar(b)) => cadmpeg_ir::compare::values_agree(a, b).is_ok(),
            (Self::Reference, Self::Reference) => true,
            (Self::Array(a), Self::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.agrees(b))
            }
            (Self::Object(a, n), Self::Object(b, m)) => {
                n == m
                    && a.len() == b.len()
                    && a.iter()
                        .all(|(key, value)| b.get(key).is_some_and(|other| value.agrees(other)))
            }
            _ => false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    Entity(ArenaName),
    External,
    MapEntry,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Step {
    Field(String),
    Index(usize),
    Entry,
    Key,
}
#[derive(Clone, Debug)]
struct Edge {
    node: usize,
    path: Vec<Step>,
}
#[derive(Clone, Debug)]
struct Node {
    kind: Kind,
    data: Data,
    outgoing: Vec<Edge>,
    incoming: Vec<Edge>,
}
impl Node {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            data: Data::Scalar(Value::Null),
            outgoing: Vec::new(),
            incoming: Vec::new(),
        }
    }
}
#[derive(Default)]
struct Graph {
    nodes: Vec<Node>,
    identities: HashMap<String, usize>,
}
impl Graph {
    fn push(&mut self, kind: Kind) -> usize {
        let id = self.nodes.len();
        self.nodes.push(Node::new(kind));
        id
    }
    fn identity(&mut self, id: &str) -> usize {
        if let Some(index) = self.identities.get(id) {
            *index
        } else {
            let index = self.push(Kind::External);
            self.identities.insert(id.to_owned(), index);
            index
        }
    }
    fn edge(&mut self, from: usize, to: usize, path: Vec<Step>) {
        self.nodes[from].outgoing.push(Edge {
            node: to,
            path: path.clone(),
        });
        self.nodes[to].incoming.push(Edge { node: from, path });
    }
    fn data(
        &mut self,
        owner: usize,
        original: &Value,
        marked: &Value,
        markers: &mark::Markers,
        path: Vec<Step>,
    ) -> Data {
        if let (Value::String(id), Value::String(marker)) = (original, marked) {
            if id != marker && markers.keys.contains_key(marker) {
                let target = self.identity(&markers.keys[marker]);
                self.edge(owner, target, path);
                return Data::Reference;
            }
        }
        match (original, marked) {
            (Value::Array(a), Value::Array(b)) => {
                assert_eq!(
                    a.len(),
                    b.len(),
                    "typed identity marking changed array cardinality"
                );
                Data::Array(
                    a.iter()
                        .zip(b)
                        .enumerate()
                        .map(|(i, (a, b))| {
                            let mut next = path.clone();
                            next.push(Step::Index(i));
                            self.data(owner, a, b, markers, next)
                        })
                        .collect(),
                )
            }
            (Value::Object(a), Value::Object(b)) => {
                let mut fields = BTreeMap::new();
                let mut references = 0;
                for (key, value) in a {
                    if let Some(marker) = markers
                        .targets
                        .get(key)
                        .filter(|marker| !b.contains_key(key) && b.contains_key(*marker))
                    {
                        references += 1;
                        let entry = self.push(Kind::MapEntry);
                        let mut next = path.clone();
                        next.push(Step::Entry);
                        self.edge(owner, entry, next);
                        let identity = self.identity(key);
                        self.edge(entry, identity, vec![Step::Key]);
                        let data = self.data(entry, value, &b[marker], markers, Vec::new());
                        self.nodes[entry].data = data;
                    } else {
                        let mut next = path.clone();
                        next.push(Step::Field(key.clone()));
                        fields.insert(key.clone(), self.data(owner, value, &b[key], markers, next));
                    }
                }
                Data::Object(fields, references)
            }
            _ => Data::Scalar(original.clone()),
        }
    }
}
fn graph(ir: &CadIr, arenas: &std::collections::BTreeSet<ArenaName>) -> Graph {
    let (original, marked, markers) = mark::model(ir);
    let mut graph = Graph::default();
    for arena in arenas {
        for record in original[arena.as_str()].as_array().into_iter().flatten() {
            let id = record["id"].as_str().expect("entity identity");
            let node = graph.push(Kind::Entity(*arena));
            assert!(
                graph.identities.insert(id.to_owned(), node).is_none(),
                "duplicate neutral identity {id}"
            );
        }
    }
    for arena in arenas {
        let Some(records) = original.get(arena.as_str()).and_then(Value::as_array) else {
            continue;
        };
        let rewritten = marked[arena.as_str()]
            .as_array()
            .expect("marked registry arena");
        for (record, marked) in records.iter().zip(rewritten) {
            let index = graph.identities[record["id"].as_str().expect("entity identity")];
            let data = graph.data(index, record, marked, &markers, Vec::new());
            graph.nodes[index].data = data;
        }
    }
    graph
}
#[derive(Clone)]
struct Mapping {
    forward: Vec<Option<usize>>,
    reverse: Vec<Option<usize>>,
}
impl Mapping {
    fn bind(&mut self, left: usize, right: usize) {
        self.forward[left] = Some(right);
        self.reverse[right] = Some(left);
    }
}
fn mapped_edges(
    edges: &[Edge],
    mapping: &[Option<usize>],
    current: usize,
    target: usize,
) -> BTreeMap<(Vec<Step>, usize), usize> {
    let mut result = BTreeMap::new();
    for edge in edges {
        let mapped = if edge.node == current {
            Some(target)
        } else {
            mapping[edge.node]
        };
        if let Some(node) = mapped {
            *result.entry((edge.path.clone(), node)).or_default() += 1;
        }
    }
    result
}
fn actual_edges(
    edges: &[Edge],
    reverse: &[Option<usize>],
    current: usize,
) -> BTreeMap<(Vec<Step>, usize), usize> {
    let mut result = BTreeMap::new();
    for edge in edges {
        if edge.node == current || reverse[edge.node].is_some() {
            *result.entry((edge.path.clone(), edge.node)).or_default() += 1;
        }
    }
    result
}
fn feasible(left: &Graph, right: &Graph, mapping: &Mapping, a: usize, b: usize) -> bool {
    if mapping.reverse[b].is_some_and(|source| source != a) {
        return false;
    }
    for (a_edges, b_edges) in [
        (&left.nodes[a].outgoing, &right.nodes[b].outgoing),
        (&left.nodes[a].incoming, &right.nodes[b].incoming),
    ] {
        if mapped_edges(a_edges, &mapping.forward, a, b)
            != actual_edges(b_edges, &mapping.reverse, b)
        {
            return false;
        }
    }
    true
}
fn edge_shape(edges: &[Edge]) -> BTreeMap<Vec<Step>, usize> {
    let mut result = BTreeMap::new();
    for edge in edges {
        *result.entry(edge.path.clone()).or_default() += 1;
    }
    result
}
fn agrees(left: &Graph, right: &Graph) -> bool {
    if left.nodes.len() != right.nodes.len() {
        return false;
    }
    let shapes = |graph: &Graph| {
        graph
            .nodes
            .iter()
            .map(|node| (edge_shape(&node.outgoing), edge_shape(&node.incoming)))
            .collect::<Vec<_>>()
    };
    let left_shapes = shapes(left);
    let right_shapes = shapes(right);
    let mut candidates = left
        .nodes
        .iter()
        .enumerate()
        .map(|(source, a)| {
            right
                .nodes
                .iter()
                .enumerate()
                .filter(|(target, b)| {
                    a.kind == b.kind
                        && a.data.agrees(&b.data)
                        && left_shapes[source] == right_shapes[*target]
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    if candidates.iter().any(Vec::is_empty) || !refine::prune(left, right, &mut candidates) {
        return false;
    }
    let mut forced = Mapping {
        forward: vec![None; left.nodes.len()],
        reverse: vec![None; right.nodes.len()],
    };
    // Singleton candidates are mandatory in every bijection. Bind them in one
    // pass instead of rescanning the whole graph once per forced assignment.
    for (source, possible) in candidates.iter().enumerate() {
        if let [target] = possible.as_slice() {
            if !feasible(left, right, &forced, source, *target) {
                return false;
            }
            forced.bind(source, *target);
        }
    }
    let mut pending = vec![forced];
    while let Some(mut mapping) = pending.pop() {
        loop {
            let mut choice = None;
            for (a, possible) in candidates
                .iter()
                .enumerate()
                .filter(|(a, _)| mapping.forward[*a].is_none())
            {
                let available = possible
                    .iter()
                    .copied()
                    .filter(|b| {
                        mapping.reverse[*b].is_none() && feasible(left, right, &mapping, a, *b)
                    })
                    .collect::<Vec<_>>();
                if choice
                    .as_ref()
                    .is_none_or(|(_, previous): &(usize, Vec<usize>)| {
                        available.len() < previous.len()
                    })
                {
                    choice = Some((a, available));
                }
            }
            let Some((a, available)) = choice else {
                return true;
            };
            if available.is_empty() {
                break;
            }
            if available.len() == 1 {
                mapping.bind(a, available[0]);
                continue;
            }
            for b in available.into_iter().rev() {
                let mut branch = mapping.clone();
                branch.bind(a, b);
                pending.push(branch);
            }
            break;
        }
    }
    false
}
/// Every compared value and all reference aliasing must survive one global renaming.
pub(super) fn assert_agree(
    arenas: &std::collections::BTreeSet<ArenaName>,
    left: &CadIr,
    right: &CadIr,
) {
    let left = graph(left, arenas);
    let right = graph(right, arenas);
    assert!(
        agrees(&left, &right),
        "written neutral content differs: no global identity bijection preserves record counts, values, nested order, and references; {}",
        diagnostic::difference(&left, &right)
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cycles(lengths: &[usize]) -> Graph {
        let mut graph = Graph::default();
        for length in lengths {
            let start = graph.nodes.len();
            for _ in 0..*length {
                graph.push(Kind::MapEntry);
            }
            for i in 0..*length {
                graph.edge(start + i, start + (i + 1) % length, vec![Step::Key]);
            }
        }
        graph
    }
    #[test]
    fn numeric_pruning_preserves_pairwise_nontransitive_tolerance() {
        let tolerance = cadmpeg_ir::compare::FLOAT_TOLERANCE;
        let a = 0.25;
        let b = a + tolerance * 0.75;
        let c = a + tolerance * 1.5;
        assert!(cadmpeg_ir::compare::floats_agree(a, b));
        assert!(cadmpeg_ir::compare::floats_agree(b, c));
        assert!(!cadmpeg_ir::compare::floats_agree(a, c));
        let mut left = cycles(&[3]);
        left.nodes[0].data = Data::Scalar(serde_json::json!(a));
        left.nodes[1].data = Data::Scalar(serde_json::json!(c));
        let mut right = cycles(&[3]);
        right.nodes[0].data = Data::Scalar(serde_json::json!(b));
        right.nodes[1].data = Data::Scalar(serde_json::json!(b));
        assert!(agrees(&left, &right), "both direct scalar pairs agree");
        right.nodes[0].data = Data::Scalar(serde_json::json!(c));
        assert!(!agrees(&left, &right), "a and c must not agree via b");
    }

    fn reference_graph(targets: [usize; 2], alias: Option<usize>) -> Graph {
        let mut graph = Graph::default();
        for label in ["root", "first", "second"] {
            let node = graph.push(Kind::MapEntry);
            graph.nodes[node].data = Data::Scalar(serde_json::json!(label));
        }
        for (index, target) in targets.into_iter().enumerate() {
            graph.edge(
                0,
                target,
                vec![Step::Field("members".into()), Step::Index(index)],
            );
        }
        if let Some(target) = alias {
            graph.edge(0, target, vec![Step::Field("alias".into())]);
        }
        graph
    }

    #[test]
    fn nested_reference_order_and_shared_targets_remain_content() {
        let left = reference_graph([1, 2], None);
        assert!(agrees(&left, &left));
        assert!(
            !agrees(&left, &reference_graph([2, 1], None)),
            "nested ordering is content"
        );
        assert!(
            !agrees(
                &reference_graph([1, 2], Some(1)),
                &reference_graph([1, 2], Some(2))
            ),
            "an alias may not acquire a different target"
        );
    }

    #[test]
    fn symmetric_equal_degree_graphs_require_the_same_connectivity() {
        assert!(agrees(&cycles(&[4, 4]), &cycles(&[4, 4])));
        assert!(!agrees(&cycles(&[4, 4]), &cycles(&[8])));
    }
}
