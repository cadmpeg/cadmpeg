// SPDX-License-Identifier: Apache-2.0
//! The §9.3 cross-codec conformance checks (#219) over serialized decodes.

use std::collections::BTreeSet;

use cadmpeg_ir::{ArenaClass, ArenaName};
use serde_json::Value;

const CLASS_NAMES: [&str; 2] = ["model", "design"];

fn class_name(class: ArenaClass) -> &'static str {
    match class {
        ArenaClass::Model => "model",
        ArenaClass::Design => "design",
    }
}

/// The §9.3 scope and transfer findings for one decode at one requested depth.
///
/// `requested` names the classes the request requires; a missing one needs
/// a `depth.not-delivered` loss.
#[must_use]
pub fn scope_findings(document: &Value, report: &Value, requested: &[&str]) -> Vec<String> {
    let mut findings = Vec::new();
    let scope = document.get("scope");
    for class in CLASS_NAMES {
        let held = scope
            .and_then(|scope| scope.get(class))
            .and_then(Value::as_bool);
        let Some(held) = held else {
            findings.push(format!("scope has no {class} flag"));
            continue;
        };
        let populated = document
            .get("model")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter(|(arena, rows)| {
                ArenaName::parse(arena).is_some_and(|name| class_name(name.class()) == class)
                    && rows.as_array().is_some_and(|rows| !rows.is_empty())
            })
            .map(|(arena, _)| arena.as_str())
            .collect::<Vec<_>>();
        if !held && !populated.is_empty() {
            findings.push(format!(
                "scope excludes {class} but {populated:?} hold records"
            ));
        }
        let not_delivered = report
            .get("losses")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|loss| {
                loss.pointer("/code/code").and_then(Value::as_str) == Some("depth.not-delivered")
            });
        if requested.contains(&class) && !held && !not_delivered {
            findings.push(format!("the request requires {class}, scope excludes it, and no depth.not-delivered loss is charged"));
        }
    }
    if report.pointer("/transfer/scope") != scope {
        findings.push(format!(
            "transfer.scope {} differs from ir.scope {}",
            report.pointer("/transfer/scope").unwrap_or(&Value::Null),
            scope.unwrap_or(&Value::Null)
        ));
    }
    findings
}

/// Every expression parameter token names a parameter of the document (Rule D).
#[must_use]
pub fn dangling_token_findings(document: &Value) -> Vec<String> {
    fn walk<'a>(value: &'a Value, found: &mut Vec<&'a str>) {
        match value {
            Value::Object(object) => {
                if let Some(Value::Array(tokens)) = object.get("tokens") {
                    for token in tokens {
                        if token.get("kind").and_then(Value::as_str) == Some("parameter") {
                            if let Some(id) = token.get("value").and_then(Value::as_str) {
                                found.push(id);
                            }
                        }
                    }
                }
                object.values().for_each(|child| walk(child, found));
            }
            Value::Array(items) => items.iter().for_each(|child| walk(child, found)),
            _ => {}
        }
    }
    let parameters: BTreeSet<&str> = document
        .pointer("/model/parameters")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| row.get("id")?.as_str())
        .collect();
    let mut findings = Vec::new();
    let mut tokens = Vec::new();
    if let Some(model) = document.get("model") {
        walk(model, &mut tokens);
    }
    for token in tokens {
        if !parameters.contains(token) {
            findings.push(format!("expression token names absent parameter {token}"));
        }
    }
    findings
}

#[cfg(test)]
mod tests;
