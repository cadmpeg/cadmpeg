// SPDX-License-Identifier: Apache-2.0
//! Arena join for `cadmpeg query join`.
//!
//! Joins two arenas on caller-named key paths. Matching is on the extracted
//! key values only. This is not SQL: no expressions, no WHERE, no three-way
//! join.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, ValueEnum};
use serde_json::{Map, Value};

use super::document::CadirDocument;
use super::item::{emit_values, ArenaTarget};
use super::output::OutputArgs;

/// How to emit matching rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
enum JoinMode {
    /// Inner join: one row per matching pair (one-to-many is pair rows).
    #[default]
    Matched,
    /// Left anti-join: left records with no right match; `right` is null.
    Unmatched,
    /// Every left record; `right` is an array of matches (possibly empty).
    All,
}

/// Input selection for `query join`.
#[derive(Debug, Args)]
pub(crate) struct JoinArgs {
    /// JSON file, or `-` for standard input.
    file: PathBuf,
    /// Left arena (`model.<arena>`, `native.<codec>.<arena>`, or bare
    /// `<arena>`). Same dotted names as `query counts --json`.
    left_arena: String,
    /// Right arena. Same spelling as the left arena.
    right_arena: String,
    /// Dotted path in each left record. Required; there is no default.
    #[arg(long, required = true, value_name = "PATH")]
    left_key: String,
    /// Dotted path in each right record. Required; there is no default.
    #[arg(long, required = true, value_name = "PATH")]
    right_key: String,
    /// `matched` (default) is an inner join. `unmatched` is a left
    /// anti-join. `all` keeps every left record with matching rights as an
    /// array.
    #[arg(long, value_enum, default_value_t = JoinMode::Matched)]
    mode: JoinMode,
    /// Join against this second CADIR document. Matching is by key value
    /// only; local ids are not unique across files.
    #[arg(long, value_name = "FILE")]
    right_file: Option<PathBuf>,
    /// Print the first N output rows (left arena order, then right arena
    /// order).
    #[arg(long, value_name = "N")]
    head: Option<usize>,
    /// Record output selection.
    #[command(flatten)]
    output: OutputArgs,
}

struct JoinSpec<'a> {
    left: &'a [Value],
    right: &'a [Value],
    left_key: &'a str,
    right_key: &'a str,
    left_arena: &'a str,
    right_arena: &'a str,
    mode: JoinMode,
    files: Option<(&'a str, &'a str)>,
    head: Option<usize>,
}

#[cfg(test)]
thread_local! {
    static ROWS_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Runs `query join` against one or two CADIR documents.
pub(super) fn run(args: &JoinArgs) -> Result<()> {
    let output = args.output.mode();
    let left_doc = CadirDocument::load(&args.file, "join")?;
    let left_target = ArenaTarget::parse(&args.left_arena)?;
    let left_arena = left_doc.require_arena(&left_target)?;
    let right_doc = args
        .right_file
        .as_ref()
        .map(|path| CadirDocument::load(path, "join"))
        .transpose()?;
    let right_target = ArenaTarget::parse(&args.right_arena)?;
    let right_arena = match &right_doc {
        Some(doc) => doc.require_arena(&right_target)?,
        None => left_doc.require_arena(&right_target)?,
    };
    let left_dotted = left_arena.target.dotted();
    let right_dotted = right_arena.target.dotted();
    let files = args.right_file.as_ref().map(|right_path| {
        (
            args.file.display().to_string(),
            right_path.display().to_string(),
        )
    });

    let spec = JoinSpec {
        left: &left_arena.records,
        right: &right_arena.records,
        left_key: &args.left_key,
        right_key: &args.right_key,
        left_arena: &left_dotted,
        right_arena: &right_dotted,
        mode: args.mode,
        files: files
            .as_ref()
            .map(|(left, right)| (left.as_str(), right.as_str())),
        head: args.head,
    };
    let rows = join_records(&spec);
    emit_values("join", output, &rows)
}

fn join_records(spec: &JoinSpec<'_>) -> Vec<Value> {
    if let Some(limit) = spec.head {
        return join_records_limited(spec, limit);
    }

    let mut index: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (ri, rec) in spec.right.iter().enumerate() {
        for key in key_values(rec, spec.right_key) {
            let slots = index.entry(key).or_default();
            if !slots.contains(&ri) {
                slots.push(ri);
            }
        }
    }

    let mut rows = Vec::new();
    for left_rec in spec.left {
        let lkeys = key_values(left_rec, spec.left_key);
        let mut pairs: Vec<(usize, String)> = Vec::new();
        let mut seen = BTreeSet::new();
        for key in &lkeys {
            if let Some(rights) = index.get(key) {
                for &ri in rights {
                    if seen.insert((ri, key.clone())) {
                        pairs.push((ri, key.clone()));
                    }
                }
            }
        }
        pairs.sort_by(|(a, ka), (b, kb)| a.cmp(b).then_with(|| ka.cmp(kb)));

        match spec.mode {
            JoinMode::Matched => {
                for (ri, key) in pairs {
                    rows.push(row_pair(spec, left_rec, Some((&spec.right[ri], &key))));
                }
            }
            JoinMode::Unmatched => {
                if pairs.is_empty() {
                    rows.push(row_pair(spec, left_rec, None));
                }
            }
            JoinMode::All => {
                let mut rights = Vec::new();
                let mut seen_ri = BTreeSet::new();
                for (ri, _) in &pairs {
                    if seen_ri.insert(*ri) {
                        rights.push(spec.right[*ri].clone());
                    }
                }
                rows.push(row_all(spec, left_rec, rights));
            }
        }
    }
    rows
}

fn join_records_limited(spec: &JoinSpec<'_>, limit: usize) -> Vec<Value> {
    let mut rows = Vec::new();
    for left_rec in spec.left {
        if rows.len() >= limit {
            break;
        }
        let left_keys: BTreeSet<String> = key_values(left_rec, spec.left_key).into_iter().collect();
        match spec.mode {
            JoinMode::Matched => {
                for right_rec in spec.right {
                    let matching: BTreeSet<String> = key_values(right_rec, spec.right_key)
                        .into_iter()
                        .filter(|key| left_keys.contains(key))
                        .collect();
                    for key in matching {
                        rows.push(row_pair(spec, left_rec, Some((right_rec, &key))));
                        if rows.len() >= limit {
                            return rows;
                        }
                    }
                }
            }
            JoinMode::Unmatched => {
                let found = spec.right.iter().any(|right_rec| {
                    key_values(right_rec, spec.right_key)
                        .iter()
                        .any(|key| left_keys.contains(key))
                });
                if !found {
                    rows.push(row_pair(spec, left_rec, None));
                }
            }
            JoinMode::All => {
                let rights = spec
                    .right
                    .iter()
                    .filter(|right_rec| {
                        key_values(right_rec, spec.right_key)
                            .iter()
                            .any(|key| left_keys.contains(key))
                    })
                    .cloned()
                    .collect();
                rows.push(row_all(spec, left_rec, rights));
            }
        }
    }
    rows
}

fn row_pair(spec: &JoinSpec<'_>, left: &Value, matched: Option<(&Value, &str)>) -> Value {
    #[cfg(test)]
    ROWS_BUILT.with(|count| count.set(count.get() + 1));
    let mut map = Map::new();
    map.insert(
        "left_arena".to_owned(),
        Value::String(spec.left_arena.to_owned()),
    );
    map.insert(
        "right_arena".to_owned(),
        Value::String(spec.right_arena.to_owned()),
    );
    let right = match matched {
        Some((right, key)) => {
            map.insert("key".to_owned(), Value::String(key.to_owned()));
            right.clone()
        }
        None => Value::Null,
    };
    map.insert("left".to_owned(), left.clone());
    map.insert("right".to_owned(), right);
    attach_files(&mut map, spec);
    Value::Object(map)
}

fn row_all(spec: &JoinSpec<'_>, left: &Value, rights: Vec<Value>) -> Value {
    #[cfg(test)]
    ROWS_BUILT.with(|count| count.set(count.get() + 1));
    let mut map = Map::new();
    map.insert(
        "left_arena".to_owned(),
        Value::String(spec.left_arena.to_owned()),
    );
    map.insert(
        "right_arena".to_owned(),
        Value::String(spec.right_arena.to_owned()),
    );
    map.insert("left".to_owned(), left.clone());
    map.insert("right".to_owned(), Value::Array(rights));
    attach_files(&mut map, spec);
    Value::Object(map)
}

fn attach_files(map: &mut Map<String, Value>, spec: &JoinSpec<'_>) {
    if let Some((left_file, right_file)) = spec.files {
        map.insert("left_file".to_owned(), Value::String(left_file.to_owned()));
        map.insert(
            "right_file".to_owned(),
            Value::String(right_file.to_owned()),
        );
    }
}

/// Canonical compare strings for a key path. Missing, null, and object
/// values contribute nothing. An array fans out: string elements are keys,
/// and objects continue the remaining path (`pcurves.pcurve`).
fn key_values(record: &Value, path: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut seen = BTreeSet::new();
    collect_keys(record, path, &mut keys, &mut seen);
    keys
}

fn collect_keys(value: &Value, path: &str, keys: &mut Vec<String>, seen: &mut BTreeSet<String>) {
    if let Value::Array(items) = value {
        for item in items {
            collect_keys(item, path, keys, seen);
        }
        return;
    }
    if path.is_empty() {
        if let Some(key) = canonical_key(value) {
            if seen.insert(key.clone()) {
                keys.push(key);
            }
        }
        return;
    }
    let (head, rest) = match path.split_once('.') {
        Some((head, rest)) => (head, rest),
        None => (path, ""),
    };
    if let Value::Object(map) = value {
        if let Some(child) = map.get(head) {
            collect_keys(child, rest, keys, seen);
        }
    }
}

fn canonical_key(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(true) => Some("true".to_owned()),
        Value::Bool(false) => Some("false".to_owned()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{join_records, JoinMode, JoinSpec, ROWS_BUILT};
    use serde_json::json;
    use serde_json::Value;

    fn spec<'a>(
        left: &'a [Value],
        right: &'a [Value],
        left_key: &'a str,
        right_key: &'a str,
        mode: JoinMode,
        files: Option<(&'a str, &'a str)>,
    ) -> JoinSpec<'a> {
        JoinSpec {
            left,
            right,
            left_key,
            right_key,
            left_arena: "model.features",
            right_arena: "native.rhino.unknowns",
            mode,
            files,
            head: None,
        }
    }

    #[test]
    fn join_scalar_equals_scalar() {
        let left = vec![json!({"id": "f1", "native_ref": "n1"})];
        let right = vec![json!({"id": "n1"}), json!({"id": "n2"})];
        let rows = join_records(&spec(
            &left,
            &right,
            "native_ref",
            "id",
            JoinMode::Matched,
            None,
        ));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["key"], "n1");
        assert_eq!(rows[0]["left"]["id"], "f1");
        assert_eq!(rows[0]["right"]["id"], "n1");
        assert_eq!(rows[0]["left_arena"], "model.features");
        assert_eq!(rows[0]["right_arena"], "native.rhino.unknowns");
        assert!(rows[0].get("left_file").is_none());
    }

    #[test]
    fn join_scalar_array_membership() {
        let left = vec![json!({"id": "f1", "native_ref": "curve#A"})];
        let right = vec![json!({"id": "r1", "links": ["curve#A", "curve#B"]})];
        let rows = join_records(&spec(
            &left,
            &right,
            "native_ref",
            "links",
            JoinMode::Matched,
            None,
        ));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["key"], "curve#A");
        assert_eq!(rows[0]["right"]["id"], "r1");
    }

    #[test]
    fn join_nested_key_path() {
        let left = vec![json!({"id": "f1", "definition": {"native_ref": "n1"}})];
        let right = vec![json!({"id": "n1"})];
        let rows = join_records(&spec(
            &left,
            &right,
            "definition.native_ref",
            "id",
            JoinMode::Matched,
            None,
        ));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["key"], "n1");
    }

    #[test]
    fn join_one_to_many_matched_pairs() {
        let left = vec![json!({"id": "f1", "native_ref": "shared"})];
        let right = vec![
            json!({"id": "r1", "links": ["shared"]}),
            json!({"id": "r2", "links": ["shared", "other"]}),
        ];
        let rows = join_records(&spec(
            &left,
            &right,
            "native_ref",
            "links",
            JoinMode::Matched,
            None,
        ));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["right"]["id"], "r1");
        assert_eq!(rows[1]["right"]["id"], "r2");
        assert_eq!(rows[0]["key"], "shared");
        assert_eq!(rows[1]["key"], "shared");
    }

    #[test]
    fn join_unmatched_anti_join() {
        let left = vec![
            json!({"id": "f1", "native_ref": "n1"}),
            json!({"id": "f2", "native_ref": "missing"}),
        ];
        let right = vec![json!({"id": "n1"})];
        let rows = join_records(&spec(
            &left,
            &right,
            "native_ref",
            "id",
            JoinMode::Unmatched,
            None,
        ));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["left"]["id"], "f2");
        assert_eq!(rows[0]["right"], Value::Null);
        assert!(rows[0].get("key").is_none());
    }

    #[test]
    fn join_two_document_includes_file_identity() {
        let left = vec![json!({"id": "L1", "name": "foo"})];
        let right = vec![json!({"id": "R1", "name": "foo"})];
        let rows = join_records(&spec(
            &left,
            &right,
            "name",
            "name",
            JoinMode::Matched,
            Some(("left.json", "right.json")),
        ));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["key"], "foo");
        assert_eq!(rows[0]["left"]["id"], "L1");
        assert_eq!(rows[0]["right"]["id"], "R1");
        assert_eq!(rows[0]["left_file"], "left.json");
        assert_eq!(rows[0]["right_file"], "right.json");
    }

    #[test]
    fn join_missing_key_does_not_panic() {
        let left = vec![json!({"id": "f1"}), json!({"id": "f2", "native_ref": null})];
        let right = vec![json!({"id": "n1"})];
        let matched = join_records(&spec(
            &left,
            &right,
            "native_ref",
            "id",
            JoinMode::Matched,
            None,
        ));
        assert!(matched.is_empty());
        let unmatched = join_records(&spec(
            &left,
            &right,
            "native_ref",
            "id",
            JoinMode::Unmatched,
            None,
        ));
        assert_eq!(unmatched.len(), 2);
    }

    #[test]
    fn join_array_of_objects_nested_path() {
        let left = vec![json!({"id": "f1", "pcurves": [{"pcurve": "c1"}, {"pcurve": "c2"}]})];
        let right = vec![
            json!({"id": "c1"}),
            json!({"id": "c2"}),
            json!({"id": "c3"}),
        ];
        let rows = join_records(&spec(
            &left,
            &right,
            "pcurves.pcurve",
            "id",
            JoinMode::Matched,
            None,
        ));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["key"], "c1");
        assert_eq!(rows[1]["key"], "c2");
    }

    #[test]
    fn join_all_groups_rights() {
        let left = vec![
            json!({"id": "f1", "native_ref": "shared"}),
            json!({"id": "f2", "native_ref": "missing"}),
        ];
        let right = vec![
            json!({"id": "r1", "links": ["shared"]}),
            json!({"id": "r2", "links": ["shared"]}),
        ];
        let rows = join_records(&spec(
            &left,
            &right,
            "native_ref",
            "links",
            JoinMode::All,
            None,
        ));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["right"].as_array().unwrap().len(), 2);
        assert_eq!(rows[1]["right"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn join_head_one_constructs_only_one_row() {
        let left = vec![json!({"id": "left", "key": "same"})];
        let right: Vec<Value> = (0..4096)
            .map(|i| json!({"id": format!("right-{i}"), "key": "same"}))
            .collect();
        let mut request = spec(&left, &right, "key", "key", JoinMode::Matched, None);
        request.head = Some(1);
        ROWS_BUILT.with(|count| count.set(0));
        let rows = join_records(&request);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["right"]["id"], "right-0");
        ROWS_BUILT.with(|count| assert_eq!(count.get(), 1));
    }

    #[test]
    fn join_limited_rows_match_unlimited_prefix_in_each_mode() {
        let left = vec![
            json!({"id": "left-0", "keys": ["b", "a"]}),
            json!({"id": "left-1", "keys": ["missing"]}),
        ];
        let right = vec![
            json!({"id": "right-0", "keys": ["b", "a"]}),
            json!({"id": "right-1", "keys": ["a"]}),
        ];
        for mode in [JoinMode::Matched, JoinMode::Unmatched, JoinMode::All] {
            let mut request = spec(&left, &right, "keys", "keys", mode, None);
            let unlimited = join_records(&request);
            for head in 0..=unlimited.len() + 1 {
                request.head = Some(head);
                let limited = join_records(&request);
                assert_eq!(limited, unlimited[..head.min(unlimited.len())]);
            }
        }
    }
}
