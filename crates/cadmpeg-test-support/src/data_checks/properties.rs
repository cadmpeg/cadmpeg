// SPDX-License-Identifier: Apache-2.0
//! Check 1 (#246 IV.2): neutral `properties` and `source_properties` maps.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

const REMOVED_NAMES: &str = include_str!("../removed_names.rs");
const CODEC_ISSUES: [(&str, &str); 11] = [
    ("220", "sat"),
    ("221", "step"),
    ("222", "iges"),
    ("223", "rhino"),
    ("224", "fcstd"),
    ("225", "f3d"),
    ("226", "sldprt"),
    ("227", "inventor"),
    ("228", "catia"),
    ("229", "creo"),
    ("230", "nx"),
];
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

/// The keys a codec issue deletes under §8, per native namespace. A `.N`
/// suffix in the registry stands for any decimal index.
#[must_use]
pub fn section_8_deletions() -> BTreeMap<&'static str, Vec<&'static str>> {
    let mut result: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut issues = Vec::new();
    for line in REMOVED_NAMES.lines() {
        let line = line.trim();
        if let Some(comment) = line.strip_prefix("///") {
            issues = CODEC_ISSUES
                .iter()
                .filter(|(number, _)| comment.contains(&format!("#{number} §8")))
                .map(|(_, codec)| *codec)
                .collect();
        } else if let Some(name) = line
            .strip_prefix("pub const ")
            .and_then(|rest| rest.split_once("= \""))
            .and_then(|(_, rest)| rest.strip_suffix("\";"))
        {
            for codec in std::mem::take(&mut issues) {
                result.entry(codec).or_default().push(name);
            }
        }
    }
    result
}

fn matches_deletion(key: &str, pattern: &str) -> bool {
    match pattern.strip_suffix(".N") {
        Some(stem) => key
            .strip_prefix(stem)
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|index| {
                !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())
            }),
        None => key == pattern,
    }
}

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
    let deletions = section_8_deletions();
    let deleted = deletions.get(namespace).map_or(&[][..], Vec::as_slice);
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
                    if deleted.iter().any(|pattern| matches_deletion(key, pattern)) {
                        findings.push(format!("{at} is deleted by §8"));
                    }
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
