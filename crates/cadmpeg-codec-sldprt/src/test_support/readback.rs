// SPDX-License-Identifier: Apache-2.0
//! Read-back comparison of a written part with the document it was written
//! from: every feature and parameter keyed by name, because ids move across a
//! write.

#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SketchFeatureBinding};
use cadmpeg_ir::CadIr;
use serde_json::Value;

/// The features and parameters of a document, keyed by name, with every id
/// they state replaced by the name of what it names.
struct Summary {
    features: BTreeMap<String, Value>,
    parameters: BTreeMap<String, Value>,
}

fn summary(ir: &CadIr) -> Summary {
    let mut features = ir.model.features.iter().collect::<Vec<_>>();
    features.sort_by_key(|feature| feature.ordinal);
    let mut names = HashMap::<String, String>::new();
    let mut seen = HashMap::<String, usize>::new();
    for feature in &features {
        let name = feature.name.clone().unwrap_or_else(|| "<unnamed>".into());
        let count = seen.entry(name.clone()).or_default();
        *count += 1;
        let key = if *count == 1 {
            name
        } else {
            format!("{name}#{count}")
        };
        names.insert(feature.id.as_str().to_owned(), format!("F:{key}"));
    }
    let mut seen = HashMap::<String, usize>::new();
    for parameter in &ir.model.parameters {
        let owner = parameter
            .owner
            .as_ref()
            .and_then(|owner| names.get(owner.as_str()))
            .cloned()
            .unwrap_or_else(|| "?".into());
        let key = format!("P:{owner}.{}", parameter.name);
        let count = seen.entry(key.clone()).or_default();
        *count += 1;
        let key = if *count == 1 {
            key
        } else {
            format!("{key}#{count}")
        };
        names.insert(parameter.id.as_str().to_owned(), key);
    }
    for feature in &features {
        if let FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(Some(sketch)),
        }) = feature.evaluation.definition()
        {
            let owner = names[feature.id.as_str()].clone();
            names.insert(sketch.as_str().to_owned(), format!("S:{owner}"));
        }
    }
    let canon = |value: Value| canonical(value, &names);
    Summary {
        features: features
            .iter()
            .map(|feature| {
                let value = serde_json::json!({
                    "definition": serde_json::to_value(feature.evaluation.definition()).unwrap(),
                    "suppressed": feature.suppressed,
                    "dependencies": feature
                        .dependencies
                        .iter()
                        .map(cadmpeg_ir::features::FeatureId::as_str)
                        .collect::<Vec<_>>(),
                    "source_tag": feature.source_tag,
                    "source_properties": serde_json::to_value(&feature.source_properties).unwrap(),
                    "source_content": feature.source_content.len(),
                });
                (names[feature.id.as_str()].clone(), canon(value))
            })
            .collect(),
        parameters: ir
            .model
            .parameters
            .iter()
            .map(|parameter| {
                let value = serde_json::json!({
                    "expression": parameter.expression,
                    "value": serde_json::to_value(&parameter.value).unwrap(),
                    "display": serde_json::to_value(parameter.display).unwrap(),
                    "properties": serde_json::to_value(&parameter.properties).unwrap(),
                    "bound": parameter.native_ref.is_some(),
                    "dependencies": parameter
                        .dependencies
                        .iter()
                        .map(cadmpeg_ir::features::ParameterId::as_str)
                        .collect::<Vec<_>>(),
                });
                (names[parameter.id.as_str()].clone(), canon(value))
            })
            .collect(),
    }
}

/// Replaces each id by the name of what it names, and drops the block and
/// lane offsets that a native token embeds, which a write moves.
fn canonical(value: Value, names: &HashMap<String, String>) -> Value {
    match value {
        Value::String(text) => Value::String(canonical_text(text, names)),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| canonical(item, names))
                .collect(),
        ),
        Value::Object(entries) => Value::Object(
            entries
                .into_iter()
                .map(|(key, value)| (canonical_text(key, names), canonical(value, names)))
                .collect(),
        ),
        other => other,
    }
}

fn canonical_text(text: String, names: &HashMap<String, String>) -> String {
    if let Some(name) = names.get(&text) {
        return name.clone();
    }
    if !text.starts_with("sldprt:") {
        return text;
    }
    if let Some((prefix, _)) = text.split_once('#') {
        return format!("{prefix}#");
    }
    text.split(':')
        .map(|segment| {
            if segment.len() >= 4 && segment.bytes().all(|byte| byte.is_ascii_digit()) {
                "N"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join(":")
}

/// Every feature and parameter of `expected` that `written` does not state
/// the same way, other than the features and parameters `skipped` names
/// (`F:<feature name>`, `P:F:<owner name>.<parameter name>`); empty when the
/// write kept everything else.
pub(crate) fn differences(expected: &CadIr, written: &CadIr, skipped: &[&str]) -> Vec<String> {
    let expected = summary(expected);
    let written = summary(written);
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
                Some(read) if read != value => {
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
