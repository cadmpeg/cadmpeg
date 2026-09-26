// SPDX-License-Identifier: Apache-2.0
//! Identity-reference walk for `cadmpeg query graph`.
//!
//! Starts are selected like `query item` (FILE ARENA [ID…], `--head`). Each
//! result is one walk: the start locator, the edge path, and the reached
//! record. Multiple routes to the same record are separate results. A node
//! already on the current path is not expanded.

use std::collections::VecDeque;

use anyhow::{bail, Result};
use clap::Args;
use serde_json::{json, Value};

use super::document::{CadirDocument, RecordRef, RecordSelection};
use super::item::{emit_values, ArenaTarget};
use super::output::OutputArgs;

/// Default cap on emitted walks. Truncation notes on stderr and exits 0.
const DEFAULT_MAX_PATHS: usize = 10_000;

/// Input selection for `query graph`.
#[derive(Debug, Args)]
pub(crate) struct GraphArgs {
    /// JSON file, or `-` for standard input.
    file: std::path::PathBuf,
    /// Arena address: `model.<arena>`, `native.<codec>.<arena>`, or bare
    /// `<arena>` as shorthand for `model.<arena>`. Same dotted names as
    /// `query counts --json`.
    arena: String,
    /// Which records of the arena to walk from.
    #[command(flatten)]
    records: RecordSelection,
    /// Maximum edge length from a start. `0` emits only the start records.
    /// Default is 1 (the start and its immediate references).
    #[arg(long, value_name = "N", default_value_t = 1)]
    hops: usize,
    /// Comma-separated dotted field paths to follow. Omit to follow every
    /// string that equals another record's `id` in this document. Each path
    /// must match an extracted edge field exactly (`links`, `native_ref`,
    /// `definition.parameters.segment_0_object`). Array members use the
    /// array's path (`links`), not `links.0`. An array of objects uses the
    /// nested field path without indices (`pcurves.pcurve`).
    #[arg(long, value_delimiter = ',', value_name = "PATH")]
    follow: Option<Vec<String>>,
    /// Walk incoming references ("what refers to this record?"). Forward is
    /// the default ("what does this record refer to?").
    #[arg(long)]
    reverse: bool,
    /// Stop after this many result paths (default 10000). Truncation prints
    /// a note on standard error and still exits 0.
    #[arg(long, value_name = "N", default_value_t = DEFAULT_MAX_PATHS)]
    max_paths: usize,
    /// Record output selection.
    #[command(flatten)]
    output: OutputArgs,
}

#[derive(Clone, Debug)]
struct AdjEdge {
    field: String,
    to: RecordRef,
}

#[cfg(test)]
thread_local! {
    static ADJ_EDGES_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct WalkOutcome {
    results: Vec<Value>,
    truncated: bool,
}

/// Runs `query graph` against one CADIR document.
pub(super) fn run(args: &GraphArgs) -> Result<()> {
    let output = args.output.mode();
    let mut doc = CadirDocument::load(&args.file, "graph")?;
    let target = ArenaTarget::parse(&args.arena)?;
    let (start_nodes, errors) = doc.select_records(&target, &args.records)?;
    if args.hops > 0 && args.max_paths > 0 {
        doc.index_ids();
    }

    let follow = args.follow.as_deref();
    let outcome = walk(
        &doc,
        &start_nodes,
        args.hops,
        follow,
        args.reverse,
        args.max_paths,
    );
    let emit_result = emit_values("graph", output, &outcome.results);
    if outcome.truncated {
        eprintln!(
            "graph truncated at {} paths (--max-paths {}); raise --max-paths to continue",
            args.max_paths, args.max_paths
        );
    }
    match (emit_result, errors.is_empty()) {
        (Ok(()), true) => Ok(()),
        (Ok(()), false) => bail!("{}", errors.join("\n")),
        (Err(err), true) => Err(err),
        (Err(err), false) => bail!("{err}\n{}", errors.join("\n")),
    }
}

fn walk(
    doc: &CadirDocument,
    starts: &[RecordRef],
    hops: usize,
    follow: Option<&[String]>,
    reverse: bool,
    max_paths: usize,
) -> WalkOutcome {
    let mut results = Vec::new();
    let mut truncated = false;

    for start in starts {
        if results.len() >= max_paths {
            truncated = true;
            break;
        }
        results.push(result_value(doc, *start, &[]));
        if hops == 0 {
            continue;
        }

        let mut queue: VecDeque<Vec<AdjEdge>> = VecDeque::new();
        queue.push_back(Vec::new());

        while let Some(path) = queue.pop_front() {
            let node = path.last().map_or(*start, |edge| edge.to);
            let remaining = max_paths - results.len();
            let cap = remaining + usize::from(remaining < usize::MAX);
            let neighbors = neighbors(doc, node, *start, &path, follow, reverse, cap);
            for edge in neighbors {
                if results.len() >= max_paths {
                    truncated = true;
                    break;
                }
                let mut next_path = path.clone();
                next_path.push(edge);
                results.push(result_value(doc, *start, &next_path));
                if next_path.len() < hops {
                    queue.push_back(next_path);
                }
            }
            if truncated {
                break;
            }
        }
        if truncated {
            break;
        }
    }

    WalkOutcome { results, truncated }
}

fn result_value(doc: &CadirDocument, start: RecordRef, path: &[AdjEdge]) -> Value {
    let mut node = start;
    let steps: Vec<Value> = path
        .iter()
        .map(|edge| {
            let step = json!({
                "from": doc.locator(node),
                "field": edge.field,
                "to": doc.locator(edge.to),
            });
            node = edge.to;
            step
        })
        .collect();
    json!({
        "start": doc.locator(start),
        "path": steps,
        "record": doc.record(node),
    })
}

fn neighbors(
    doc: &CadirDocument,
    node: RecordRef,
    start: RecordRef,
    path: &[AdjEdge],
    follow: Option<&[String]>,
    reverse: bool,
    cap: usize,
) -> Vec<AdjEdge> {
    let mut edges = Vec::new();
    let mut visit = |field: &str, to: RecordRef| {
        if to == start || path.iter().any(|step| step.to == to) {
            return true;
        }
        if let Some(paths) = follow {
            if !paths.iter().any(|candidate| candidate == field) {
                return true;
            }
        }
        #[cfg(test)]
        ADJ_EDGES_BUILT.with(|count| count.set(count.get() + 1));
        edges.push(AdjEdge {
            field: field.to_owned(),
            to,
        });
        edges.len() < cap
    };

    if reverse {
        for (from, rec) in doc.records() {
            if from == node {
                continue;
            }
            let mut incoming = |field: &str, to: RecordRef| {
                if to == node {
                    visit(field, from)
                } else {
                    true
                }
            };
            if !visit_edges(rec, "", true, doc, &mut incoming) {
                break;
            }
        }
    } else {
        let mut outgoing = |field: &str, to: RecordRef| {
            if to == node {
                true
            } else {
                visit(field, to)
            }
        };
        visit_edges(doc.record(node), "", true, doc, &mut outgoing);
    }
    edges
}

fn visit_edges(
    value: &Value,
    path: &str,
    skip_top_id: bool,
    doc: &CadirDocument,
    visit: &mut impl FnMut(&str, RecordRef) -> bool,
) -> bool {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if skip_top_id && key == "id" {
                    continue;
                }
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                if !visit_edges(child, &child_path, false, doc, visit) {
                    return false;
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                if !visit_edges(item, path, false, doc, visit) {
                    return false;
                }
            }
        }
        Value::String(s) => {
            if path.is_empty() {
                return true;
            }
            let Some(hits) = doc.id_locations(s) else {
                return true;
            };
            for &record in hits {
                if !visit(path, record) {
                    return false;
                }
            }
        }
        _ => {}
    }
    true
}

#[cfg(test)]
mod tests {
    use super::super::document::{CadirDocument, RecordRef, RecordSelection};
    use super::super::item::ArenaTarget;
    use super::{walk, ADJ_EDGES_BUILT, DEFAULT_MAX_PATHS};
    use serde_json::json;
    use serde_json::Value;

    fn doc(v: &Value) -> CadirDocument {
        let mut document = CadirDocument::from_value(v.clone());
        document.index_ids();
        document
    }

    fn start_nodes(document: &CadirDocument, arena: &str, ids: &[&str]) -> Vec<RecordRef> {
        let target = ArenaTarget::parse(arena).unwrap();
        let ids: Vec<String> = ids.iter().map(|s| (*s).to_owned()).collect();
        let selection = crate::query::document::RequestedIds::new(ids)
            .map_or(RecordSelection::Head(1), RecordSelection::Ids);
        let (recs, errors) = document.select_records(&target, &selection).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        recs
    }

    fn walk_ids(
        document: &CadirDocument,
        arena: &str,
        ids: &[&str],
        hops: usize,
        follow: Option<&[String]>,
        reverse: bool,
    ) -> Vec<Value> {
        let starts = start_nodes(document, arena, ids);
        walk(document, &starts, hops, follow, reverse, DEFAULT_MAX_PATHS).results
    }

    fn reached_ids(results: &[Value]) -> Vec<String> {
        results
            .iter()
            .map(|r| {
                r["record"]
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned()
            })
            .collect()
    }

    fn paths_to(results: &[Value], id: &str) -> Vec<Vec<String>> {
        results
            .iter()
            .filter(|r| r["record"].get("id").and_then(Value::as_str) == Some(id))
            .map(|r| {
                r["path"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|step| step["field"].as_str().unwrap().to_owned())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn graph_follows_links_across_arenas() {
        let document = doc(&json!({
            "model": {"features": [{"id": "f1", "links": ["n1"]}]},
            "native": {"rhino": {"unknowns": [{"id": "n1", "kind": "curve"}]}}
        }));
        let results = walk_ids(&document, "features", &["f1"], 1, None, false);
        assert_eq!(reached_ids(&results), vec!["f1", "n1"]);
        assert_eq!(paths_to(&results, "n1"), vec![vec!["links"]]);
    }

    #[test]
    fn graph_follows_nested_parameter_object() {
        let document = doc(&json!({
            "model": {"features": [{
                "id": "f1",
                "definition": {"parameters": {"segment_0_object": "n1"}}
            }]},
            "native": {"rhino": {"unknowns": [{"id": "n1"}]}}
        }));
        let results = walk_ids(&document, "features", &["f1"], 1, None, false);
        assert_eq!(
            paths_to(&results, "n1"),
            vec![vec!["definition.parameters.segment_0_object"]]
        );
    }

    #[test]
    fn graph_follows_array_of_ids() {
        let document = doc(&json!({
            "model": {
                "bodies": [{"id": "b1", "regions": ["r1", "r2"]}],
                "regions": [{"id": "r1"}, {"id": "r2"}]
            }
        }));
        let results = walk_ids(&document, "bodies", &["b1"], 1, None, false);
        let mut ids = reached_ids(&results);
        ids.sort();
        assert_eq!(ids, vec!["b1", "r1", "r2"]);
        assert_eq!(paths_to(&results, "r1"), vec![vec!["regions"]]);
        assert_eq!(paths_to(&results, "r2"), vec![vec!["regions"]]);
    }

    #[test]
    fn graph_hops_1_vs_2() {
        let document = doc(&json!({
            "model": {
                "bodies": [{"id": "b1", "regions": ["r1"]}],
                "regions": [{"id": "r1", "shells": ["s1"]}],
                "shells": [{"id": "s1"}]
            }
        }));
        let h1 = walk_ids(&document, "bodies", &["b1"], 1, None, false);
        assert_eq!(reached_ids(&h1), vec!["b1", "r1"]);
        let h2 = walk_ids(&document, "bodies", &["b1"], 2, None, false);
        assert_eq!(reached_ids(&h2), vec!["b1", "r1", "s1"]);
        let h0 = walk_ids(&document, "bodies", &["b1"], 0, None, false);
        assert_eq!(reached_ids(&h0), vec!["b1"]);
        assert!(h0[0]["path"].as_array().unwrap().is_empty());
    }

    #[test]
    fn graph_multiple_paths_both_appear() {
        let document = doc(&json!({
            "model": {
                "features": [{
                    "id": "a",
                    "links": ["c"],
                    "via": ["b"]
                }],
                "mid": [{"id": "b", "links": ["c"]}],
                "leaves": [{"id": "c"}]
            }
        }));
        let results = walk_ids(&document, "features", &["a"], 2, None, false);
        let to_c = paths_to(&results, "c");
        assert!(to_c.contains(&vec!["links".to_owned()]), "{to_c:?}");
        assert!(
            to_c.contains(&vec!["via".to_owned(), "links".to_owned()]),
            "{to_c:?}"
        );
        assert_eq!(to_c.len(), 2);
    }

    #[test]
    fn graph_reverse_finds_incoming() {
        let document = doc(&json!({
            "model": {"features": [{"id": "f1", "links": ["n1"]}]},
            "native": {"rhino": {"unknowns": [{"id": "n1"}]}}
        }));
        let results = walk_ids(&document, "native.rhino.unknowns", &["n1"], 1, None, true);
        assert_eq!(reached_ids(&results), vec!["n1", "f1"]);
        assert_eq!(paths_to(&results, "f1"), vec![vec!["links"]]);
    }

    #[test]
    fn graph_own_id_is_not_an_edge() {
        let document = doc(&json!({
            "model": {"features": [{"id": "f1", "name": "solo"}]}
        }));
        let results = walk_ids(&document, "features", &["f1"], 1, None, false);
        assert_eq!(results.len(), 1);
        assert_eq!(reached_ids(&results), vec!["f1"]);
    }

    #[test]
    fn graph_follow_restricts_fields() {
        let document = doc(&json!({
            "model": {"features": [{
                "id": "f1",
                "links": ["n1"],
                "native_ref": "n2"
            }]},
            "native": {"rhino": {"unknowns": [
                {"id": "n1"},
                {"id": "n2"}
            ]}}
        }));
        let follow = vec!["links".to_owned()];
        let results = walk_ids(&document, "features", &["f1"], 1, Some(&follow), false);
        assert_eq!(reached_ids(&results), vec!["f1", "n1"]);
    }

    #[test]
    fn graph_cycle_does_not_loop() {
        let document = doc(&json!({
            "model": {
                "nodes": [
                    {"id": "a", "links": ["b"]},
                    {"id": "b", "links": ["a"]}
                ]
            }
        }));
        let results = walk_ids(&document, "nodes", &["a"], 10, None, false);
        assert_eq!(results.len(), 2);
        assert_eq!(reached_ids(&results), vec!["a", "b"]);
    }

    #[test]
    fn graph_skips_dangling_and_does_not_suffix_match_stored_values() {
        let document = doc(&json!({
            "model": {
                "features": [{"id": "ns:item#802", "links": ["802", "missing"]}],
                "other": [{"id": "ns:item#802x"}]
            }
        }));
        let results = walk_ids(&document, "features", &["ns:item#802"], 1, None, false);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn graph_array_of_objects_uses_nested_path_without_indices() {
        let document = doc(&json!({
            "model": {
                "faces": [{"id": "f1", "pcurves": [{"pcurve": "c1"}]}],
                "curves": [{"id": "c1"}]
            }
        }));
        let results = walk_ids(&document, "faces", &["f1"], 1, None, false);
        assert_eq!(paths_to(&results, "c1"), vec![vec!["pcurves.pcurve"]]);
    }

    #[test]
    fn graph_max_paths_one_constructs_only_one_candidate_edge() {
        let links: Vec<String> = (0..4096).map(|i| format!("leaf-{i}")).collect();
        let leaves: Vec<Value> = links.iter().map(|id| json!({"id": id})).collect();
        let value = json!({"model": {
            "roots": [{"id": "root", "links": links}],
            "leaves": leaves
        }});
        let document = doc(&value);
        let starts = start_nodes(&document, "roots", &["root"]);
        ADJ_EDGES_BUILT.with(|count| count.set(0));
        let outcome = walk(&document, &starts, 1, None, false, 1);
        assert_eq!(reached_ids(&outcome.results), vec!["root"]);
        assert!(outcome.truncated);
        ADJ_EDGES_BUILT.with(|count| assert_eq!(count.get(), 1));
    }
}
