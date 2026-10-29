// SPDX-License-Identifier: Apache-2.0
//! Check 1 (#246 IV.2): neutral `properties` and `source_properties` maps.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

const CHECK_2_SUFFIXES: [&str; 14] = [
    "count",
    "len",
    "length",
    "size",
    "offset",
    "records",
    "rows",
    "runs",
    "spans",
    "block",
    "blocks",
    "sections",
    "instances",
    "pairs",
];
const MAPS: [&str; 2] = ["properties", "source_properties"];

/// The check 2 key pattern (#246 IV.2) applied to a property key.
#[must_use]
pub fn is_source_layout_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    let digest = lower
        .match_indices("sha")
        .any(|(at, _)| lower[at + 3..].starts_with(|c: char| c.is_ascii_digit()));
    let fragments = [
        "digest",
        "_offset",
        "byte",
        "stamp",
        "baseline",
        "synthesised",
        "synthesized",
        "fingerprint",
    ];
    let tail = CHECK_2_SUFFIXES.iter().any(|suffix| {
        lower
            .strip_suffix(suffix)
            .is_some_and(|stem| stem.is_empty() || stem.ends_with(['.', '_']))
    });
    let raw = lower.starts_with("raw_") || lower.contains(".raw_") || lower.contains("_raw_");
    digest
        || fragments.iter().any(|fragment| lower.contains(fragment))
        || tail
        || raw
        || lower.starts_with("section.")
        || lower.starts_with("layer.")
}

fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// Check 1 findings for one decoded document of one native namespace.
#[must_use]
pub fn property_map_findings(namespace: &str, document: &Value) -> Vec<String> {
    let mut identities = BTreeSet::new();
    let mut native = BTreeMap::new();
    let mut owned: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for rows in document
        .get("model")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|model| model.values())
    {
        identities.extend(
            rows.as_array()
                .into_iter()
                .flatten()
                .filter_map(|row| row.get("id")?.as_str()),
        );
    }
    for (space, arenas) in document
        .get("native")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        for (arena, rows) in arenas.as_object().into_iter().flatten() {
            for row in rows.as_array().into_iter().flatten() {
                let Some(id) = row.get("id").and_then(Value::as_str) else {
                    continue;
                };
                identities.insert(id);
                native.insert(id, row);
                if space == "fcstd" && matches!(arena.as_str(), "properties" | "gui_properties") {
                    if let (Some(owner), Some(name)) = (
                        row.get("owner").and_then(Value::as_str),
                        row.get("name").and_then(Value::as_str),
                    ) {
                        owned.entry(owner).or_default().insert(name);
                    }
                }
            }
        }
    }
    let mut findings = Vec::new();
    for (arena, rows) in document
        .get("model")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        for row in rows.as_array().into_iter().flatten() {
            let id = row.get("id").and_then(Value::as_str).unwrap_or("<no id>");
            let holder = row.get("native_ref").and_then(Value::as_str);
            for map in MAPS {
                let Some(entries) = row.get(map).and_then(Value::as_object) else {
                    continue;
                };
                if map == "source_properties"
                    && matches!(namespace, "fcstd" | "sldprt")
                    && !entries.is_empty()
                {
                    findings.push(format!(
                        "{arena} {id}: {namespace} source_properties is not empty"
                    ));
                }
                for (key, value) in entries {
                    let at = format!("{arena} {id}: {map}[{key:?}]");
                    if is_source_layout_key(key) {
                        findings.push(format!("{at} matches the check 2 pattern"));
                    }
                    let shown = text(value);
                    if let Some(shown) = &shown {
                        if identities.contains(shown.as_str()) {
                            findings.push(format!("{at} holds the record id {shown}"));
                        }
                        if shown.starts_with('<') {
                            findings.push(format!("{at} holds markup {shown:?}"));
                        }
                    }
                    let Some(record) = holder.and_then(|holder| native.get(holder)) else {
                        continue;
                    };
                    if record.get(key).is_some() {
                        findings.push(format!("{at} is a field of its native record"));
                    }
                    let in_map = record.as_object().into_iter().flatten().any(|(_, field)| {
                        field
                            .as_object()
                            .and_then(|map| map.get(key))
                            .and_then(text)
                            .is_some_and(|held| Some(&held) == shown.as_ref())
                    });
                    if in_map {
                        findings.push(format!("{at} is an entry of a native map field"));
                    }
                    if holder
                        .and_then(|holder| owned.get(holder))
                        .is_some_and(|names| names.contains(key.as_str()))
                    {
                        findings.push(format!(
                            "{at} is a FreeCAD property record of its native record"
                        ));
                    }
                }
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests;
