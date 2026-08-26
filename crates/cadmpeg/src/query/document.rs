// SPDX-License-Identifier: Apache-2.0
//! CADIR document index for graph, join, and inferred schema.
//!
//! Parses the document as JSON and inventories every array arena under
//! `model` and `native.<codec>.arenas`. Identity lookup is exact string
//! match against each record's top-level `id`.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::Value;

use super::item::{ambiguous_message, miss_id_message, unknown_arena_message, ArenaTarget};
use super::{detect, read_input, Artifact};

/// One addressable JSON-array arena and its records.
#[derive(Debug, Clone)]
pub(crate) struct Arena {
    pub dotted: String,
    pub records: Vec<Value>,
}

/// Indexed CADIR document: arenas, addressable names, and id lookup.
#[derive(Debug, Clone)]
pub(crate) struct CadirDocument {
    pub arenas: Vec<Arena>,
    /// Exact id → every `(arena_index, record_index)` that carries it.
    pub by_id: BTreeMap<String, Vec<(usize, usize)>>,
    pub addressable: Vec<(String, u64)>,
}

impl CadirDocument {
    /// Reads `path`, rejects reports and sidecars, and indexes every array arena.
    pub(crate) fn load(path: &Path, view: &str) -> Result<Self> {
        let bytes = read_input(path)?;
        reject_non_cadir(&bytes, path, view)?;
        Self::from_bytes(&bytes, path)
    }

    pub(crate) fn from_bytes(bytes: &[u8], path: &Path) -> Result<Self> {
        let root: Value = serde_json::from_slice(bytes)
            .with_context(|| format!("parsing the CADIR document {}", path.display()))?;
        Ok(Self::from_value(&root))
    }

    pub(crate) fn from_value(root: &Value) -> Self {
        let mut arenas = Vec::new();
        if let Some(model) = root.get("model").and_then(Value::as_object) {
            for (name, value) in model {
                if let Some(arr) = value.as_array() {
                    arenas.push(Arena {
                        dotted: format!("model.{name}"),
                        records: arr.clone(),
                    });
                }
            }
        }
        if let Some(native) = root.get("native").and_then(Value::as_object) {
            for (codec, namespace) in native {
                let Some(native_arenas) = namespace.get("arenas").and_then(Value::as_object) else {
                    continue;
                };
                for (name, value) in native_arenas {
                    if let Some(arr) = value.as_array() {
                        arenas.push(Arena {
                            dotted: format!("native.{codec}.{name}"),
                            records: arr.clone(),
                        });
                    }
                }
            }
        }

        let mut by_id: BTreeMap<String, Vec<(usize, usize)>> = BTreeMap::new();
        for (ai, arena) in arenas.iter().enumerate() {
            for (ri, rec) in arena.records.iter().enumerate() {
                if let Some(id) = record_id(rec) {
                    by_id.entry(id.to_owned()).or_default().push((ai, ri));
                }
            }
        }

        let addressable = arenas
            .iter()
            .map(|arena| (arena.dotted.clone(), arena.records.len() as u64))
            .collect();

        Self {
            arenas,
            by_id,
            addressable,
        }
    }

    pub(crate) fn require_arena(&self, target: &ArenaTarget) -> Result<&Arena> {
        let dotted = target.dotted();
        self.arenas
            .iter()
            .find(|arena| arena.dotted == dotted)
            .ok_or_else(|| anyhow::anyhow!(unknown_arena_message(target, &self.addressable)))
    }

    pub(crate) fn arena_index(&self, target: &ArenaTarget) -> Result<usize> {
        let dotted = target.dotted();
        self.arenas
            .iter()
            .position(|arena| arena.dotted == dotted)
            .ok_or_else(|| anyhow::anyhow!(unknown_arena_message(target, &self.addressable)))
    }

    pub(crate) fn all_ids(&self) -> std::collections::BTreeSet<String> {
        self.by_id.keys().cloned().collect()
    }

    pub(crate) fn locator(&self, arena: usize, rec: usize) -> String {
        let arena = &self.arenas[arena];
        match record_id(&arena.records[rec]) {
            Some(id) => format!("{}#{id}", arena.dotted),
            None => format!("{}#{rec}", arena.dotted),
        }
    }
}

/// Top-level JSON-string `id`, if present.
pub(crate) fn record_id(record: &Value) -> Option<&str> {
    record.get("id").and_then(Value::as_str)
}

/// Selects start records the same way `query item` does.
///
/// Missing or ambiguous IDs go into the error list; the caller emits any
/// resolved records and then fails if that list is not empty.
pub(crate) fn select_records(
    arena: &Arena,
    ids: &[String],
    head: Option<usize>,
) -> (Vec<usize>, Vec<String>) {
    if ids.is_empty() {
        let n = head.unwrap_or(1);
        let end = n.min(arena.records.len());
        return ((0..end).collect(), Vec::new());
    }

    let indexed: Vec<(Option<&str>, usize)> = arena
        .records
        .iter()
        .enumerate()
        .map(|(i, rec)| (record_id(rec), i))
        .collect();
    let all_ids: Vec<String> = indexed
        .iter()
        .filter_map(|(id, _)| id.map(str::to_owned))
        .collect();

    let mut indices = Vec::new();
    let mut errors = Vec::new();
    for request in ids {
        match resolve_one(request, &indexed) {
            Ok(i) => indices.push(i),
            Err(ResolveError::Ambiguous(matches)) => {
                errors.push(ambiguous_message(request, &arena.dotted, &matches));
            }
            Err(ResolveError::Missing) => {
                errors.push(miss_id_message(
                    &arena.dotted,
                    request,
                    arena.records.len() as u64,
                    &all_ids,
                ));
            }
        }
    }
    (indices, errors)
}

enum ResolveError {
    Missing,
    Ambiguous(Vec<String>),
}

fn resolve_one(request: &str, indexed: &[(Option<&str>, usize)]) -> Result<usize, ResolveError> {
    for (id, i) in indexed {
        if *id == Some(request) {
            return Ok(*i);
        }
    }
    let mut suffix: Vec<(String, usize)> = Vec::new();
    for (id, i) in indexed {
        if let Some(id) = id {
            if id.ends_with(request) {
                suffix.push(((*id).to_owned(), *i));
            }
        }
    }
    match suffix.len() {
        0 => Err(ResolveError::Missing),
        1 => Ok(suffix[0].1),
        _ => Err(ResolveError::Ambiguous(
            suffix.into_iter().map(|(id, _)| id).collect(),
        )),
    }
}

pub(crate) fn reject_non_cadir(bytes: &[u8], path: &Path, view: &str) -> Result<()> {
    let artifact = detect(bytes, path)?;
    match artifact {
        Artifact::Cadir(_) => Ok(()),
        Artifact::Report(_) => bail!(
            "{} is a command report; reports have no arenas. Use \
             `cadmpeg query findings` / `cadmpeg query losses` on the report, or \
             `cadmpeg dump SOURCE -o doc.json && cadmpeg query {view} doc.json …`",
            path.display()
        ),
        Artifact::Sidecar(_) => bail!(
            "{} is a decode sidecar (`<stem>.fidelity.json`); sidecars have no \
             arenas. Run `cadmpeg dump SOURCE -o doc.json && cadmpeg query {view} \
             doc.json …`",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn indexes_model_and_native_arenas_and_skips_non_arrays() {
        let doc = CadirDocument::from_value(&json!({
            "ir_version": "4",
            "model": {
                "faces": [{"id": "f1"}, {"id": "f2"}],
                "empty": [],
                "null_arena": null
            },
            "native": {
                "rhino": {"arenas": {"unknowns": [{"id": "n1"}]}}
            }
        }));
        let names: Vec<&str> = doc.arenas.iter().map(|a| a.dotted.as_str()).collect();
        assert!(names.contains(&"model.faces"));
        assert!(names.contains(&"model.empty"));
        assert!(names.contains(&"native.rhino.unknowns"));
        assert!(!names.iter().any(|n| n.contains("null")));
        let (ai, ri) = doc.by_id["f1"][0];
        assert_eq!(doc.arenas[ai].dotted, "model.faces");
        assert_eq!(ri, 0);
        assert_eq!(doc.locator(ai, ri), "model.faces#f1");
    }

    #[test]
    fn id_collision_keeps_every_hit() {
        let doc = CadirDocument::from_value(&json!({
            "model": {
                "a": [{"id": "dup"}],
                "b": [{"id": "dup"}]
            }
        }));
        assert_eq!(doc.by_id["dup"].len(), 2);
    }

    #[test]
    fn select_head_and_suffix_and_ambiguous() {
        let doc = CadirDocument::from_value(&json!({
            "model": {"faces": [
                {"id": "other:face#1"},
                {"id": "other:face#2"},
                {"id": "other:face#802"},
                {"id": "other:coedge#802"}
            ]}
        }));
        let arena = doc
            .require_arena(&ArenaTarget::parse("faces").unwrap())
            .unwrap();
        let (idx, err) = select_records(arena, &[], Some(2));
        assert_eq!(idx, vec![0, 1]);
        assert!(err.is_empty());

        let (idx, err) = select_records(arena, &["face#2".to_owned()], None);
        assert_eq!(idx, vec![1]);
        assert!(err.is_empty());

        let (_, err) = select_records(arena, &["#802".to_owned()], None);
        assert_eq!(err.len(), 1);
        assert!(err[0].contains("ambiguous"));
    }
}
