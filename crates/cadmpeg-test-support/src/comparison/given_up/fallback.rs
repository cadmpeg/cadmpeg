// SPDX-License-Identifier: Apache-2.0
//! Typed fallback fields discovered from the compiled IR schema.

use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::OnceLock;

fn schema() -> &'static Value {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    SCHEMA.get_or_init(|| cadmpeg_ir::schema::query::cadir().0.as_value().clone())
}

fn resolve(mut node: &Value) -> &Value {
    for _ in 0..128 {
        let Some(reference) = node
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|reference| reference.strip_prefix('#'))
        else {
            break;
        };
        let Some(target) = schema().pointer(reference) else {
            break;
        };
        node = target;
    }
    node
}

// This is schema selection, not a permissive data validator. Both snapshots
// were constructed as typed CadIr. Constants, required fields and container
// shapes select the actual compiled enum arm, including untagged wrappers.
fn accepts(node: &Value, value: &Value, depth: usize) -> bool {
    if depth == 0 {
        return false;
    }
    let node = resolve(node);
    if node == &Value::Bool(false) {
        return false;
    }
    if let Some(constant) = node.get("const") {
        return constant == value;
    }
    if let Some(variants) = node.get("enum").and_then(Value::as_array) {
        if !variants.contains(value) {
            return false;
        }
    }
    for keyword in ["anyOf", "oneOf"] {
        if let Some(arms) = node.get(keyword).and_then(Value::as_array) {
            if !arms.iter().any(|arm| accepts(arm, value, depth - 1)) {
                return false;
            }
        }
    }
    if let Some(arms) = node.get("allOf").and_then(Value::as_array) {
        if !arms.iter().all(|arm| accepts(arm, value, depth - 1)) {
            return false;
        }
    }
    if let Some(kind) = node.get("type").and_then(Value::as_str) {
        let valid = match kind {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "number" => value.is_number(),
            "integer" => value.is_i64() || value.is_u64(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => false,
        };
        if !valid {
            return false;
        }
    }
    if let Some(required) = node.get("required").and_then(Value::as_array) {
        if !required
            .iter()
            .all(|key| key.as_str().is_some_and(|key| value.get(key).is_some()))
        {
            return false;
        }
    }
    if let (Some(properties), Some(fields)) = (
        node.get("properties").and_then(Value::as_object),
        value.as_object(),
    ) {
        if !fields.iter().all(|(key, value)| {
            properties
                .get(key)
                .is_none_or(|child| accepts(child, value, depth - 1))
        }) {
            return false;
        }
    }
    true
}

fn native_arm(node: &Value, value: &Value, depth: usize) -> Result<bool, String> {
    if depth == 0 {
        return Err("typed fallback schema exceeds bounded nesting".into());
    }
    let node = resolve(node);
    if node
        .get("properties")
        .and_then(Value::as_object)
        .is_some_and(|fields| {
            fields.iter().any(|(key, field)| {
                let Some(tag) = resolve(field).get("const").and_then(Value::as_str) else {
                    return false;
                };
                let semantic = tag
                    .chars()
                    .filter(|ch| *ch != '_')
                    .flat_map(char::to_lowercase)
                    .collect::<String>();
                (semantic == "native" || semantic == "nativeset")
                    && value.get(key).and_then(Value::as_str) == Some(tag)
            })
        })
    {
        return Ok(true);
    }
    // Untagged wrappers select a schema for the same value, not a child field.
    for keyword in ["anyOf", "oneOf"] {
        if let Some(arms) = node.get(keyword).and_then(Value::as_array) {
            return arms
                .iter()
                .find(|arm| accepts(arm, value, 128))
                .map_or(Ok(false), |arm| native_arm(arm, value, depth - 1));
        }
    }
    if let Some(arms) = node.get("allOf").and_then(Value::as_array) {
        for arm in arms {
            if accepts(arm, value, 128) && native_arm(arm, value, depth - 1)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub(super) fn has_missing_reference(value: &Value, missing: &BTreeSet<String>) -> bool {
    match value {
        Value::String(id) => missing.contains(id),
        Value::Array(items) => items
            .iter()
            .any(|value| has_missing_reference(value, missing)),
        Value::Object(fields) => fields
            .iter()
            .any(|(key, value)| missing.contains(key) || has_missing_reference(value, missing)),
        _ => false,
    }
}

fn mask(
    node: &Value,
    before: &mut Value,
    after: &Value,
    missing: &BTreeSet<String>,
    depth: usize,
) -> Result<usize, String> {
    if depth == 0 {
        return Err("typed fallback schema exceeds bounded nesting".into());
    }
    let node = resolve(node);
    for keyword in ["anyOf", "oneOf"] {
        if let Some(arms) = node.get(keyword).and_then(Value::as_array) {
            let old = arms.iter().position(|arm| accepts(arm, before, 128));
            let new = arms.iter().position(|arm| accepts(arm, after, 128));
            if let (Some(old), Some(new)) = (old, new) {
                if old != new
                    && !native_arm(&arms[old], before, depth - 1)?
                    && native_arm(&arms[new], after, depth - 1)?
                {
                    if has_missing_reference(after, missing) {
                        return Err("Native fallback names a disappeared record".into());
                    }
                    *before = after.clone();
                    return Ok(1);
                }
                if old == new {
                    return mask(&arms[old], before, after, missing, depth - 1);
                }
            }
            return Ok(0);
        }
    }
    let mut count = 0;
    if let Some(arms) = node.get("allOf").and_then(Value::as_array) {
        for arm in arms {
            count += mask(arm, before, after, missing, depth - 1)?;
        }
    }
    match (before, after) {
        (Value::Object(old), Value::Object(new)) => {
            if let Some(fields) = node.get("properties").and_then(Value::as_object) {
                for (key, child) in fields {
                    if let (Some(old), Some(new)) = (old.get_mut(key), new.get(key)) {
                        count += mask(child, old, new, missing, depth - 1)?;
                    }
                }
            }
        }
        (Value::Array(old), Value::Array(new)) => {
            if let Some(items) = node.get("items") {
                for (old, new) in old.iter_mut().zip(new) {
                    count += mask(items, old, new, missing, depth - 1)?;
                }
            }
        }
        _ => {}
    }
    Ok(count)
}

pub(super) fn mask_fields(
    arena: &str,
    before: &mut Value,
    after: &Value,
    missing: &BTreeSet<String>,
) -> Result<usize, String> {
    let Some(arena) = arena.strip_prefix("model/") else {
        return Ok(0);
    };
    let Some(model) = schema()
        .get("properties")
        .and_then(|fields| fields.get("model"))
    else {
        return Ok(0);
    };
    let Some(rows) = resolve(model)
        .get("properties")
        .and_then(|fields| fields.get(arena))
    else {
        return Ok(0);
    };
    let Some(row) = resolve(rows).get("items") else {
        return Ok(0);
    };
    mask(row, before, after, missing, 128)
}
