// SPDX-License-Identifier: Apache-2.0
//! Data checks 1, 5, 6 and 7 (#246 IV.2), P-closure (#219 §4.3) and the
//! `Native` kind check (#209 §2.2) over serialized CADIR documents.
//!
//! Each check returns one finding per violation, so a caller names every
//! failing record of every golden in one run.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// One committed decode golden: its repository path and its CADIR document.
#[derive(Debug, Clone)]
pub struct DecodeGolden {
    /// Path relative to the workspace root.
    pub path: String,
    /// The `ir` member of the golden.
    pub ir: Value,
    /// The `source_fidelity` member of the golden, or `null`.
    pub fidelity: Value,
}

/// Every committed decode golden of one workspace crate that holds a decoded
/// document, in path order. A golden of a refused decode holds none.
///
/// A decode golden is a `tests/golden/decode/**.json` snapshot, or for NX a
/// `tests/golden/*.json` snapshot whose `decode` member holds the decode.
///
/// # Panics
///
/// Panics when a golden cannot be read, or holds neither a document nor a
/// `decode_error`.
#[must_use]
pub fn decode_goldens(crate_name: &str) -> Vec<DecodeGolden> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("cadmpeg-test-support lives at crates/<name>");
    let golden = workspace
        .join("crates")
        .join(crate_name)
        .join("tests/golden");
    let mut paths = Vec::new();
    collect_json(&golden.join("decode"), &mut paths);
    if let Ok(entries) = std::fs::read_dir(&golden) {
        paths.extend(
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == "json")
                }),
        );
    }
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let value: Value = serde_json::from_str(&text)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let decode = value.get("decode").unwrap_or(&value);
            let Some(ir) = decode.get("ir") else {
                assert!(
                    decode.get("decode_error").is_some(),
                    "{}: golden holds neither `ir` nor `decode_error`",
                    path.display()
                );
                return None;
            };
            let relative = path.strip_prefix(workspace).unwrap_or(&path);
            Some(DecodeGolden {
                path: relative.display().to_string(),
                ir: ir.clone(),
                fidelity: decode
                    .get("source_fidelity")
                    .cloned()
                    .unwrap_or(Value::Null),
            })
        })
        .collect()
}

fn collect_json(directory: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        if path.is_dir() {
            collect_json(&path, paths);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            paths.push(path);
        }
    }
}

fn records<'a>(document: &'a Value, arena: &str) -> &'a [Value] {
    document
        .get("model")
        .and_then(|model| model.get(arena))
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn id(record: &Value) -> &str {
    record
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("<no id>")
}

/// Check 7: `Feature.ordinal` values are exactly `{0 … n−1}`.
#[must_use]
pub fn feature_ordinal_findings(document: &Value) -> Vec<String> {
    let features = records(document, "features");
    let mut seen = BTreeSet::new();
    let mut findings = Vec::new();
    for feature in features {
        match feature.get("ordinal").and_then(Value::as_u64) {
            Some(ordinal) if seen.insert(ordinal) => {}
            Some(ordinal) => findings.push(format!("{}: ordinal {ordinal} repeats", id(feature))),
            None => findings.push(format!("{}: no integer ordinal", id(feature))),
        }
    }
    let count = features.len();
    for ordinal in seen
        .iter()
        .filter(|ordinal| !usize::try_from(**ordinal).is_ok_and(|ordinal| ordinal < count))
    {
        findings.push(format!(
            "ordinal {ordinal} is outside 0..{count} for {count} feature(s)"
        ));
    }
    findings
}

/// Check 5 for a decoder whose only `needs_recompute` value is `false`.
#[must_use]
pub fn never_stale_findings(document: &Value) -> Vec<String> {
    records(document, "features")
        .iter()
        .filter(|feature| feature.get("needs_recompute") != Some(&Value::Bool(false)))
        .map(|feature| {
            format!(
                "{}: needs_recompute is {}, not false",
                id(feature),
                feature.get("needs_recompute").unwrap_or(&Value::Null)
            )
        })
        .collect()
}

/// Check 5 for `FreeCAD`: a feature needs recompute exactly when its native
/// object's status is Touched.
#[must_use]
pub fn freecad_stale_findings(document: &Value) -> Vec<String> {
    let objects: BTreeMap<&str, &Value> = document
        .pointer("/native/fcstd/objects")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| Some((row.get("id")?.as_str()?, row)))
        .collect();
    records(document, "features")
        .iter()
        .filter_map(|feature| {
            let touched = feature
                .get("native_ref")
                .and_then(Value::as_str)
                .and_then(|native| objects.get(native))
                .and_then(|object| object.pointer("/attributes/Touched"))
                .and_then(Value::as_str)
                .is_some_and(|value| matches!(value, "1" | "true" | "True"));
            let stale = feature.get("needs_recompute") == Some(&Value::Bool(true));
            (touched != stale).then(|| {
                format!(
                    "{}: needs_recompute is {stale}, its object Touched is {touched}",
                    id(feature)
                )
            })
        })
        .collect()
}

/// Check 5 for `SolidWorks`: every feature needs recompute exactly when the
/// active configuration's `swConfigurationNeedsUpdate` is `YES`.
#[must_use]
pub fn solidworks_stale_findings(document: &Value) -> Vec<String> {
    let attributes = document.pointer("/source/attributes");
    let needs_update = records(document, "configurations")
        .iter()
        .filter(|configuration| configuration.get("active") == Some(&Value::Bool(true)))
        .filter_map(|configuration| configuration.get("source_index")?.as_u64())
        .any(|slot| {
            attributes
                .and_then(|attributes| {
                    attributes.get(format!("sw_configuration_{slot}_needs_update"))
                })
                .and_then(Value::as_str)
                .is_some_and(|value| value.eq_ignore_ascii_case("YES"))
        });
    records(document, "features")
        .iter()
        .filter(|feature| feature.get("needs_recompute") != Some(&Value::Bool(needs_update)))
        .map(|feature| format!("{}: needs_recompute is not {needs_update}", id(feature)))
        .collect()
}

/// Check 6, first rule: no text token, trimmed, spells a parameter name,
/// `name@owner`, or one of the caller's native equation identities.
#[must_use]
pub fn expression_text_findings(document: &Value, equation_ids: &BTreeSet<String>) -> Vec<String> {
    let owners: BTreeMap<&str, &str> = records(document, "features")
        .iter()
        .filter_map(|feature| Some((id(feature), feature.get("name")?.as_str()?)))
        .collect();
    let mut spellings = equation_ids.clone();
    for parameter in records(document, "parameters") {
        let Some(name) = parameter.get("name").and_then(Value::as_str) else {
            continue;
        };
        spellings.insert(name.to_owned());
        if let Some(owner) = parameter
            .get("owner")
            .and_then(Value::as_str)
            .and_then(|owner| owners.get(owner))
        {
            spellings.insert(format!("{name}@{owner}"));
        }
    }
    let mut findings = Vec::new();
    if let Some(model) = document.get("model") {
        text_tokens(model, &mut String::from("model"), &spellings, &mut findings);
    }
    findings
}

/// Every `SolidWorks` equation identity the document holds, under a parameter
/// property or a native field.
#[must_use]
pub fn equation_identities(document: &Value) -> BTreeSet<String> {
    fn walk(value: &Value, found: &mut BTreeSet<String>) {
        match value {
            Value::Object(object) => {
                for (key, child) in object {
                    if matches!(key.as_str(), "EquationId" | "equation_id") {
                        if let Some(text) = child.as_str() {
                            found.insert(text.trim().to_owned());
                        }
                    }
                    walk(child, found);
                }
            }
            Value::Array(items) => items.iter().for_each(|child| walk(child, found)),
            _ => {}
        }
    }
    let mut found = BTreeSet::new();
    walk(document, &mut found);
    found.remove("");
    found
}

fn text_tokens(
    value: &Value,
    path: &mut String,
    spellings: &BTreeSet<String>,
    findings: &mut Vec<String>,
) {
    match value {
        Value::Object(object) => {
            if let Some(Value::Array(tokens)) = object.get("tokens") {
                for token in tokens {
                    if token.get("kind").and_then(Value::as_str) == Some("text") {
                        if let Some(text) = token.get("value").and_then(Value::as_str) {
                            if spellings.contains(text.trim()) {
                                findings
                                    .push(format!("{path}: text token {text:?} names a parameter"));
                            }
                        }
                    }
                }
            }
            for (key, child) in object {
                let length = path.len();
                path.push('/');
                path.push_str(key);
                text_tokens(child, path, spellings, findings);
                path.truncate(length);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                let length = path.len();
                path.push('/');
                path.push_str(&index.to_string());
                text_tokens(child, path, spellings, findings);
                path.truncate(length);
            }
        }
        _ => {}
    }
}

pub mod conformance;
pub mod p_closure;
pub mod properties;

#[cfg(test)]
mod tests;
