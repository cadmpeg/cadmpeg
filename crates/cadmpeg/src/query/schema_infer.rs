// SPDX-License-Identifier: Apache-2.0
//! Per-document field inventory for `cadmpeg query schema FILE ARENA`.
//!
//! Native arena records have no compile-time JSON Schema in this binary.
//! This view walks every record in the named arena and reports each dotted
//! path's presence, JSON type(s), a truncated example, and whether the
//! path holds the record identity (`id`) or identity references
//! (`ref` / `refs`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{bail, Result};
use serde_json::Value;

use cadmpeg_ir::ids::is_valid_identity;

use super::document::CadirDocument;
use super::item::{field_cell, ArenaTarget};
use super::{cell, print_json, Artifact};

/// Cap on the example cell so a byte array does not flood the table.
const EXAMPLE_MAX: usize = 80;

/// Infers a field table from a decoded CADIR document.
pub(crate) fn run(file: &str, arena: Option<&str>, json: bool) -> Result<()> {
    let path = Path::new(file);
    let bytes = super::read_input(path)?;
    let artifact = super::detect(&bytes, path)?;
    match artifact {
        Artifact::Cadir(_) => {}
        Artifact::Report(_) => bail!(
            "{} is a command report; reports have no arenas. Infer native fields \
             from a decoded CADIR document: cadmpeg dump SOURCE -o doc.json && \
             cadmpeg query schema doc.json native.<codec>.<arena>",
            path.display()
        ),
        Artifact::Sidecar(_) => bail!(
            "{} is a decode sidecar (`<stem>.fidelity.json`); sidecars have no \
             arenas. Run `cadmpeg dump SOURCE -o doc.json && cadmpeg query schema \
             doc.json native.<codec>.<arena>`",
            path.display()
        ),
    }

    let doc = CadirDocument::from_bytes(&bytes, path)?;

    let Some(spec) = arena else {
        bail!("{}", need_arena_message(&doc.addressable()));
    };

    let target = ArenaTarget::parse(spec)?;
    let Some(arena_rec) = doc.arenas().iter().find(|arena| arena.target == target) else {
        bail!("{}", unknown_arena_message(&target, &doc.addressable()));
    };

    let entry_count = arena_rec.records.len() as u64;
    let rows = infer_fields(&arena_rec.records, &doc.all_ids());
    if json {
        print_json(
            "schema",
            &json_payload(&target.dotted(), entry_count, &rows),
        );
        return Ok(());
    }
    println!("path\tpresence\ttype\texample\trelation");
    for row in &rows {
        println!(
            "{}\t{}/{}\t{}\t{}\t{}",
            cell(&row.path),
            row.present,
            entry_count,
            cell(&row.type_label),
            cell(&row.example),
            row.relation.map(Relation::as_str).unwrap_or("")
        );
    }
    if entry_count == 0 {
        eprintln!("(arena is empty)");
    } else {
        eprintln!(
            "inferred from {} records in {}",
            entry_count,
            target.dotted()
        );
    }
    eprintln!("relation=ref|refs paths are graph --follow fields and join keys");
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Relation {
    Id,
    Ref,
    Refs,
}

impl Relation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Ref => "ref",
            Self::Refs => "refs",
        }
    }
}

struct FieldRow {
    path: String,
    present: u64,
    type_label: String,
    example: String,
    relation: Option<Relation>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum JsonType {
    Array,
    Boolean,
    Number,
    Object,
    String,
}

impl JsonType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Array => "array",
            Self::Boolean => "boolean",
            Self::Number => "number",
            Self::Object => "object",
            Self::String => "string",
        }
    }

    fn of(value: &Value) -> Option<Self> {
        match value {
            Value::Null => None,
            Value::Bool(_) => Some(Self::Boolean),
            Value::Number(_) => Some(Self::Number),
            Value::String(_) => Some(Self::String),
            Value::Array(_) => Some(Self::Array),
            Value::Object(_) => Some(Self::Object),
        }
    }
}

#[derive(Default)]
struct PathStat {
    present: u64,
    types: BTreeSet<JsonType>,
    example: Option<String>,
    strings: BTreeSet<String>,
}

fn infer_fields(records: &[Value], doc_ids: &BTreeSet<String>) -> Vec<FieldRow> {
    let mut stats: BTreeMap<String, PathStat> = BTreeMap::new();
    for record in records {
        ingest_record(&mut stats, record);
    }
    stats
        .into_iter()
        .map(|(path, stat)| {
            let relation = relation_of(&path, &stat, doc_ids);
            FieldRow {
                path,
                present: stat.present,
                type_label: stat
                    .types
                    .iter()
                    .map(|kind| kind.as_str())
                    .collect::<Vec<_>>()
                    .join("|"),
                example: stat.example.unwrap_or_default(),
                relation,
            }
        })
        .collect()
}

fn relation_of(path: &str, stat: &PathStat, doc_ids: &BTreeSet<String>) -> Option<Relation> {
    let is_ref = |s: &str| doc_ids.contains(s) || is_valid_identity(s);
    if path == "id" && stat.types.contains(&JsonType::String) {
        return Some(Relation::Id);
    }
    if stat.types.contains(&JsonType::Array) && stat.strings.iter().any(|s| is_ref(s)) {
        return Some(Relation::Refs);
    }
    if stat.types.contains(&JsonType::String) && stat.strings.iter().any(|s| is_ref(s)) {
        return Some(Relation::Ref);
    }
    None
}

fn ingest_record(stats: &mut BTreeMap<String, PathStat>, value: &Value) {
    let mut seen = BTreeSet::new();
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                ingest_node(stats, key, child, &mut seen);
            }
        }
        Value::Null => {}
        other => ingest_node(stats, ".", other, &mut seen),
    }
}

fn ingest_node(
    stats: &mut BTreeMap<String, PathStat>,
    path: &str,
    value: &Value,
    seen: &mut BTreeSet<String>,
) {
    let Some(kind) = JsonType::of(value) else {
        return;
    };
    {
        let first = seen.insert(path.to_owned());
        let stat = stats.entry(path.to_owned()).or_default();
        if first {
            stat.present += 1;
        }
        stat.types.insert(kind);
        if stat.example.is_none() {
            stat.example = Some(example_cell(value));
        }
        match value {
            Value::String(s) => {
                stat.strings.insert(s.clone());
            }
            Value::Array(items) => {
                for item in items {
                    if let Value::String(s) = item {
                        stat.strings.insert(s.clone());
                    }
                }
            }
            _ => {}
        }
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                ingest_node(stats, &format!("{path}.{key}"), child, seen);
            }
        }
        Value::Array(items) => {
            // Index-free nested paths, matching graph `--follow` (`pcurves.pcurve`).
            for item in items {
                if let Value::Object(map) = item {
                    for (key, child) in map {
                        ingest_node(stats, &format!("{path}.{key}"), child, seen);
                    }
                }
            }
        }
        _ => {}
    }
}

fn example_cell(value: &Value) -> String {
    let text = field_cell(value);
    if text.chars().count() <= EXAMPLE_MAX {
        return text;
    }
    let mut out = String::new();
    for (i, ch) in text.chars().enumerate() {
        if i >= EXAMPLE_MAX {
            break;
        }
        out.push(ch);
    }
    out.push_str("...");
    out
}

fn json_payload(arena: &str, records: u64, rows: &[FieldRow]) -> Value {
    let fields: Vec<Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "path": row.path,
                "present": row.present,
                "records": records,
                "type": row.type_label,
                "example": row.example,
                "relation": row.relation.map(Relation::as_str),
            })
        })
        .collect();
    serde_json::json!({
        "arena": arena,
        "records": records,
        "inferred": true,
        "fields": fields,
    })
}

fn format_inventory(addressable: &[(String, u64)]) -> String {
    let map: BTreeMap<&str, u64> = addressable
        .iter()
        .map(|(name, count)| (name.as_str(), *count))
        .collect();
    let mut out = String::from("arena\tentries\n");
    if map.is_empty() {
        out.push_str("(none — this document has no array arenas)\n");
        return out;
    }
    for (name, count) in map {
        out.push_str(name);
        out.push('\t');
        out.push_str(&count.to_string());
        out.push('\n');
    }
    out
}

fn need_arena_message(addressable: &[(String, u64)]) -> String {
    format!(
        "`query schema` on a CADIR document needs an arena name. Addressable \
         arenas in this document:\n{}example: cadmpeg query schema FILE \
         native.<codec>.<arena>",
        format_inventory(addressable)
    )
}

fn unknown_arena_message(target: &ArenaTarget, addressable: &[(String, u64)]) -> String {
    format!(
        "unknown arena {}; addressable arenas in this document:\n{}infer fields \
         with `cadmpeg query schema FILE ARENA` using a name from the table",
        target.dotted(),
        format_inventory(addressable)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn infers_presence_types_and_nested_paths() {
        let records = vec![
            json!({
                "id": "a",
                "meta": {"tag": "x", "note": "one"},
                "layout_prefix": [129, 33],
                "flag": true
            }),
            json!({
                "id": "b",
                "meta": {"tag": "y"},
                "count": 3
            }),
            json!(null),
        ];
        let rows = infer_fields(&records, &BTreeSet::new());
        let by_path: BTreeMap<&str, &FieldRow> =
            rows.iter().map(|row| (row.path.as_str(), row)).collect();

        assert_eq!(by_path["id"].present, 2);
        assert_eq!(by_path["id"].type_label, "string");
        assert_eq!(by_path["id"].example, "a");

        assert_eq!(by_path["meta"].present, 2);
        assert_eq!(by_path["meta"].type_label, "object");
        assert_eq!(by_path["meta.tag"].present, 2);
        assert_eq!(by_path["meta.note"].present, 1);
        assert_eq!(by_path["meta.note"].example, "one");

        assert_eq!(by_path["layout_prefix"].present, 1);
        assert_eq!(by_path["layout_prefix"].type_label, "array");
        assert_eq!(by_path["layout_prefix"].example, "[129,33]");
        assert!(!by_path.contains_key("layout_prefix.0"));

        assert_eq!(by_path["flag"].type_label, "boolean");
        assert_eq!(by_path["count"].type_label, "number");
    }

    #[test]
    fn mixed_types_join_and_null_is_absent() {
        let records = vec![json!({"x": 1}), json!({"x": "a"}), json!({"x": null})];
        let rows = infer_fields(&records, &BTreeSet::new());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, "x");
        assert_eq!(rows[0].present, 2);
        assert_eq!(rows[0].type_label, "number|string");
        assert_eq!(rows[0].example, "1");
    }

    #[test]
    fn root_non_object_uses_dot_path() {
        let rows = infer_fields(&[json!(1), json!(2)], &BTreeSet::new());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, ".");
        assert_eq!(rows[0].present, 2);
        assert_eq!(rows[0].type_label, "number");
    }

    #[test]
    fn example_truncates_long_values() {
        let long = "n".repeat(EXAMPLE_MAX + 20);
        let rows = infer_fields(&[json!({ "name": long })], &BTreeSet::new());
        assert!(rows[0].example.ends_with("..."));
        assert_eq!(rows[0].example.chars().count(), EXAMPLE_MAX + 3);
    }

    #[test]
    fn schema_relation_marks_id_ref_refs_and_empty() {
        let records = vec![json!({
            "id": "feat#1",
            "native_ref": "nat#1",
            "links": ["nat#1", "nat#2"],
            "numeric_links": [1, 2],
            "name": "plain",
            "identity_shaped": "cadmpeg:model:feature#zz",
            "child": {"id": "not-an-id"}
        })];
        let mut doc_ids = BTreeSet::new();
        doc_ids.insert("feat#1".to_owned());
        doc_ids.insert("nat#1".to_owned());
        doc_ids.insert("nat#2".to_owned());
        let rows = infer_fields(&records, &doc_ids);
        let by_path: BTreeMap<&str, Option<Relation>> = rows
            .iter()
            .map(|row| (row.path.as_str(), row.relation))
            .collect();
        assert_eq!(by_path["id"], Some(Relation::Id));
        assert_eq!(by_path["native_ref"], Some(Relation::Ref));
        assert_eq!(by_path["links"], Some(Relation::Refs));
        assert_eq!(by_path["numeric_links"], None);
        assert_eq!(by_path["name"], None);
        assert_eq!(by_path["identity_shaped"], Some(Relation::Ref));
        assert_eq!(by_path["child.id"], None);
        assert!(!by_path.contains_key("links.0"));
    }

    #[test]
    fn array_of_objects_emits_index_free_nested_paths() {
        let records = vec![json!({
            "id": "f1",
            "pcurves": [
                {"pcurve": "c1", "isoparametric": true},
                {"pcurve": "c2"}
            ]
        })];
        let mut doc_ids = BTreeSet::new();
        doc_ids.insert("f1".to_owned());
        doc_ids.insert("c1".to_owned());
        doc_ids.insert("c2".to_owned());
        let rows = infer_fields(&records, &doc_ids);
        let by_path: BTreeMap<&str, &FieldRow> =
            rows.iter().map(|row| (row.path.as_str(), row)).collect();

        assert_eq!(by_path["pcurves"].present, 1);
        assert_eq!(by_path["pcurves"].type_label, "array");
        assert_eq!(by_path["pcurves"].relation, None);
        assert_eq!(by_path["pcurves.pcurve"].present, 1);
        assert_eq!(by_path["pcurves.pcurve"].type_label, "string");
        assert_eq!(by_path["pcurves.pcurve"].relation, Some(Relation::Ref));
        assert_eq!(by_path["pcurves.isoparametric"].present, 1);
        assert!(!by_path.contains_key("pcurves.0"));
        assert!(!by_path.contains_key("pcurves.0.pcurve"));
    }
}
