// SPDX-License-Identifier: Apache-2.0
//! Per-document field inventory for `cadmpeg query schema FILE ARENA`.
//!
//! Native arena records have no compile-time JSON Schema in this binary.
//! This view walks every record in the named arena and reports each dotted
//! path's presence, JSON type(s), and a truncated example — the inventory
//! `--fields` guessing reconstructed from empty-path errors.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::Value;

use super::item::{field_cell, snapshot_arena, ArenaTarget};
use super::{cell, print_json, Artifact};

/// Cap on the example cell so a byte array does not flood the table.
const EXAMPLE_MAX: usize = 80;

/// Impossible model arena name: matches nothing, so the walk only inventories.
const INVENTORY_ONLY: &str = "";

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

    let text = std::str::from_utf8(&bytes)
        .with_context(|| format!("{} is not valid UTF-8", path.display()))?;

    let Some(spec) = arena else {
        let snap = snapshot_arena(
            text,
            &ArenaTarget::Model {
                arena: INVENTORY_ONLY.to_owned(),
            },
        )?;
        bail!("{}", need_arena_message(&snap.addressable));
    };

    let target = ArenaTarget::parse(spec)?;
    let snap = snapshot_arena(text, &target)?;
    if !snap.found_array {
        bail!("{}", unknown_arena_message(&target, &snap.addressable));
    }

    let rows = infer_fields(&snap.records);
    if json {
        print_json(
            "schema",
            &json_payload(&target.dotted(), snap.entry_count, &rows),
        );
        return Ok(());
    }
    println!("path\tpresence\ttype\texample");
    for row in &rows {
        println!(
            "{}\t{}/{}\t{}\t{}",
            cell(&row.path),
            row.present,
            snap.entry_count,
            cell(&row.type_label),
            cell(&row.example)
        );
    }
    if snap.entry_count == 0 {
        eprintln!("(arena is empty)");
    } else {
        eprintln!(
            "inferred from {} records in {}",
            snap.entry_count,
            target.dotted()
        );
    }
    Ok(())
}

struct FieldRow {
    path: String,
    present: u64,
    type_label: String,
    example: String,
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
}

fn infer_fields(records: &[Value]) -> Vec<FieldRow> {
    let mut stats: BTreeMap<String, PathStat> = BTreeMap::new();
    for record in records {
        ingest_record(&mut stats, record);
    }
    stats
        .into_iter()
        .map(|(path, stat)| FieldRow {
            path,
            present: stat.present,
            type_label: stat
                .types
                .iter()
                .map(|kind| kind.as_str())
                .collect::<Vec<_>>()
                .join("|"),
            example: stat.example.unwrap_or_default(),
        })
        .collect()
}

fn ingest_record(stats: &mut BTreeMap<String, PathStat>, value: &Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                ingest_node(stats, key, child);
            }
        }
        Value::Null => {}
        other => ingest_node(stats, ".", other),
    }
}

fn ingest_node(stats: &mut BTreeMap<String, PathStat>, path: &str, value: &Value) {
    let Some(kind) = JsonType::of(value) else {
        return;
    };
    let stat = stats.entry(path.to_owned()).or_default();
    stat.present += 1;
    stat.types.insert(kind);
    if stat.example.is_none() {
        stat.example = Some(example_cell(value));
    }
    if let Value::Object(map) = value {
        for (key, child) in map {
            ingest_node(stats, &format!("{path}.{key}"), child);
        }
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
        let rows = infer_fields(&records);
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
        let rows = infer_fields(&records);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, "x");
        assert_eq!(rows[0].present, 2);
        assert_eq!(rows[0].type_label, "number|string");
        assert_eq!(rows[0].example, "1");
    }

    #[test]
    fn root_non_object_uses_dot_path() {
        let rows = infer_fields(&[json!(1), json!(2)]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, ".");
        assert_eq!(rows[0].present, 2);
        assert_eq!(rows[0].type_label, "number");
    }

    #[test]
    fn example_truncates_long_values() {
        let long = "n".repeat(EXAMPLE_MAX + 20);
        let rows = infer_fields(&[json!({ "name": long })]);
        assert!(rows[0].example.ends_with("..."));
        assert_eq!(rows[0].example.chars().count(), EXAMPLE_MAX + 3);
    }
}
