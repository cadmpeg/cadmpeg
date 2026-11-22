// SPDX-License-Identifier: Apache-2.0
//! Read-back comparison of a written part with the document it was written
//! from: every feature and parameter keyed by name, because ids move across a
//! write.

use cadmpeg_ir::CadIr;
use serde_json::Value;

use crate::history::write::read_back::{same_read_back, PartSummary};

/// The definition of feature `key` (`F:<feature name>`) in `expected` and in
/// `written`, with ids replaced by names, when the two differ.
pub(crate) fn definition_difference(
    expected: &CadIr,
    written: &CadIr,
    key: &str,
) -> Option<(Value, Value)> {
    let definition = |ir: &CadIr| {
        PartSummary::of(ir)
            .features
            .get(key)
            .and_then(|feature| feature.get("definition"))
            .cloned()
            .unwrap_or(Value::Null)
    };
    let (expected, written) = (definition(expected), definition(written));
    (!same_read_back(&expected, &written, false)).then_some((expected, written))
}

/// Every feature and parameter of `expected` that `written` does not state
/// the same way, other than the features and parameters `skipped` names
/// (`F:<feature name>`, `P:F:<owner name>.<parameter name>`); empty when the
/// write kept everything else.
pub(crate) fn differences(expected: &CadIr, written: &CadIr, skipped: &[&str]) -> Vec<String> {
    let expected = PartSummary::of(expected);
    let written = PartSummary::of(written);
    let mut differences = Vec::new();
    for (arena, expected, written) in [
        ("feature", &expected.features, &written.features),
        ("parameter", &expected.parameters, &written.parameters),
    ] {
        for (key, value) in expected {
            if skipped.contains(&key.as_str()) {
                continue;
            }
            match written.get(key) {
                None => differences.push(format!("{arena} {key} is missing")),
                Some(read) if !same_read_back(read, value, false) => {
                    differences.push(format!("{arena} {key}: {value} reads back {read}"));
                }
                Some(_) => {}
            }
        }
        for key in written.keys() {
            if !expected.contains_key(key) && !skipped.contains(&key.as_str()) {
                differences.push(format!("{arena} {key} is added"));
            }
        }
    }
    differences
}

/// Each source property of feature `key` (`F:<feature name>`) in `expected`
/// that `written` does not state.
pub(crate) fn missing_source_properties(
    expected: &CadIr,
    written: &CadIr,
    key: &str,
) -> Vec<String> {
    let properties = |ir: &CadIr| {
        PartSummary::of(ir)
            .features
            .get(key)
            .and_then(|feature| feature.get("source_properties"))
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
    };
    let written = properties(written);
    properties(expected)
        .keys()
        .filter(|property| !written.contains_key(*property))
        .cloned()
        .collect()
}
