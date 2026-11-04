// SPDX-License-Identifier: Apache-2.0
//! Enum variants and tag paths from the compiled CADIR schema.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde_json::Value;

#[derive(Default)]
pub(super) struct Inspection {
    pub variants: BTreeSet<String>,
    pub tags: BTreeMap<String, Vec<(String, String)>>,
}

impl Inspection {
    fn extend(&mut self, other: Self) {
        self.variants.extend(other.variants);
        for (path, values) in other.tags {
            self.tags.entry(path).or_default().extend(values);
        }
    }

    fn intersect(&mut self, other: &Self) {
        self.variants
            .retain(|variant| other.variants.contains(variant));
        self.tags.retain(|path, values| {
            let Some(other_values) = other.tags.get(path) else {
                return false;
            };
            values.retain(|value| other_values.contains(value));
            !values.is_empty()
        });
    }
}

fn schema() -> &'static Value {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        serde_json::to_value(cadmpeg_ir::cadir_json_schema()).expect("CADIR schema serializes")
    })
}

fn reference<'a>(root: &'a Value, node: &Value) -> Option<(&'a str, &'a Value)> {
    let reference = node.get("$ref")?.as_str()?;
    let name = reference.strip_prefix("#/$defs/")?;
    let (name, schema) = root.get("$defs")?.as_object()?.get_key_value(name)?;
    Some((name.as_str(), schema))
}

fn resolve<'a>(root: &'a Value, mut node: &'a Value) -> &'a Value {
    for _ in 0..128 {
        let Some((_, next)) = reference(root, node) else {
            break;
        };
        node = next;
    }
    node
}

fn type_matches(schema: &Value, value: &Value) -> bool {
    let matches = |kind: &str| match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "null" => value.is_null(),
        _ => true,
    };
    match schema.get("type") {
        Some(Value::String(kind)) => matches(kind),
        Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).any(matches),
        _ => true,
    }
}

fn compatible(root: &Value, schema: &Value, value: &Value, depth: usize) -> bool {
    if depth >= 128 {
        return true;
    }
    if !type_matches(schema, value)
        || schema
            .get("const")
            .is_some_and(|expected| expected != value)
        || schema
            .get("enum")
            .and_then(Value::as_array)
            .is_some_and(|values| !values.contains(value))
    {
        return false;
    }
    if let Some(fields) = value.as_object() {
        if schema
            .get("required")
            .and_then(Value::as_array)
            .is_some_and(|required| {
                required
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|key| !fields.contains_key(key))
            })
        {
            return false;
        }
        if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
            for (key, child) in fields {
                if let Some(property) = properties.get(key) {
                    if property
                        .get("const")
                        .is_some_and(|expected| expected != child)
                    {
                        return false;
                    }
                }
            }
        }
    }
    if let Some((_, target)) = reference(root, schema) {
        if !compatible(root, target, value, depth + 1) {
            return false;
        }
    }
    for union in ["anyOf", "oneOf"] {
        if let Some(branches) = schema.get(union).and_then(Value::as_array) {
            if !branches
                .iter()
                .any(|branch| compatible(root, branch, value, depth + 1))
            {
                return false;
            }
        }
    }
    true
}

fn child_path(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_owned()
    } else {
        format!("{path}.{key}")
    }
}

fn schema_domain(
    root: &Value,
    schema: &Value,
    tag: Option<&str>,
    depth: usize,
    out: &mut BTreeSet<String>,
) {
    if depth >= 128 {
        return;
    }
    if let Some((_, target)) = reference(root, schema) {
        if let Some(value) = tag
            .and_then(|tag| schema.get("properties")?.get(tag)?.get("const"))
            .and_then(Value::as_str)
        {
            out.insert(value.to_owned());
        } else {
            schema_domain(root, target, tag, depth + 1, out);
        }
        return;
    }
    let mut union = false;
    for key in ["anyOf", "oneOf"] {
        if let Some(branches) = schema.get(key).and_then(Value::as_array) {
            union = true;
            for branch in branches {
                schema_domain(root, branch, tag, depth + 1, out);
            }
        }
    }
    if union {
        return;
    }
    if let Some(tag) = tag {
        if let Some(value) = schema
            .get("properties")
            .and_then(|properties| properties.get(tag))
            .and_then(|property| property.get("const"))
            .and_then(Value::as_str)
        {
            out.insert(value.to_owned());
        }
    } else if let Some(value) = schema.get("const").and_then(Value::as_str) {
        out.insert(value.to_owned());
    } else if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        out.extend(values.iter().filter_map(Value::as_str).map(str::to_owned));
    } else if let (Some(properties), Some(required)) = (
        schema.get("properties").and_then(Value::as_object),
        schema.get("required").and_then(Value::as_array),
    ) {
        if properties.len() == 1 && required.len() == 1 {
            if let Some(key) = required[0]
                .as_str()
                .filter(|key| properties.contains_key(*key))
            {
                out.insert(key.to_owned());
            }
        }
    }
}

fn inspect(root: &Value, schema: &Value, value: &Value, path: &str, depth: usize) -> Inspection {
    if depth >= 128 {
        return Inspection::default();
    }
    if let Some((name, target)) = reference(root, schema) {
        let mut found = inspect(root, target, value, path, depth + 1);
        if let Some((variant, tag)) =
            crate::carried::typed_variant(name, value).filter(|(_, tag)| {
                let mut actual = BTreeSet::new();
                schema_domain(root, target, tag.as_deref(), depth + 1, &mut actual);
                !actual.is_empty() && crate::carried::variant_domain(name).as_ref() == Some(&actual)
            })
        {
            let tag_path = tag
                .as_deref()
                .map_or_else(|| path.to_owned(), |tag| child_path(path, tag));
            let scalar = tag
                .as_deref()
                .and_then(|tag| value.get(tag))
                .unwrap_or(value);
            if let Some(text) = scalar.as_str() {
                found
                    .tags
                    .entry(tag_path)
                    .or_default()
                    .push((name.to_owned(), text.to_owned()));
            }
            found.variants.insert(variant);
        }
        return found;
    }
    let mut found = Inspection::default();
    for union in ["anyOf", "oneOf"] {
        if let Some(branches) = schema.get(union).and_then(Value::as_array) {
            let mut matches = branches
                .iter()
                .filter(|branch| compatible(root, branch, value, depth + 1));
            if let Some(first) = matches.next() {
                let mut shared = inspect(root, first, value, path, depth + 1);
                for branch in matches {
                    shared.intersect(&inspect(root, branch, value, path, depth + 1));
                }
                found.extend(shared);
            }
        }
    }
    if let Some(branches) = schema.get("allOf").and_then(Value::as_array) {
        for branch in branches {
            found.extend(inspect(root, branch, value, path, depth + 1));
        }
    }
    match value {
        Value::Object(fields) => {
            let properties = schema.get("properties").and_then(Value::as_object);
            for (key, child) in fields {
                if super::DELETED_KEYS.contains(&key.as_str())
                    || (path == "model.drawings[]" && key == "parameters")
                {
                    continue;
                }
                if let Some(property) = properties.and_then(|properties| properties.get(key)) {
                    found.extend(inspect(
                        root,
                        property,
                        child,
                        &child_path(path, key),
                        depth + 1,
                    ));
                }
            }
        }
        Value::Array(items) => {
            if let Some(item_schema) = schema.get("items") {
                let item_path = format!("{path}[]");
                for item in items {
                    found.extend(inspect(root, item_schema, item, &item_path, depth + 1));
                }
            }
        }
        _ => {}
    }
    found
}

pub(super) fn record(arena: &str, row: &Value) -> Inspection {
    let root = schema();
    let Some(model) = root
        .get("properties")
        .and_then(|properties| properties.get("model"))
    else {
        return Inspection::default();
    };
    let model = resolve(root, model);
    let Some(item) = model
        .get("properties")
        .and_then(|properties| properties.get(arena))
        .and_then(|arena| arena.get("items"))
    else {
        return Inspection::default();
    };
    inspect(root, item, row, &format!("model.{arena}[]"), 0)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn distinguishes_enum_fields_from_unrelated_strings() {
        let row = json!({"id":"test:model:drawing#1", "kind":"dimension", "runtime_type":"view"});
        let found = super::record("drawings", &row);
        assert!(
            found.variants.contains("DrawingKind::Dimension"),
            "{:?}",
            found.variants
        );
        assert!(
            !found.variants.contains("DrawingKind::View"),
            "{:?}",
            found.variants
        );
        assert!(found.tags.contains_key("model.drawings[].kind"));
        assert!(!found.tags.contains_key("model.drawings[].runtime_type"));
    }

    #[test]
    fn reads_the_actual_nested_curve_variant() {
        let row = json!({"id":"test:model:curve#1", "geometry":{
            "kind":"line", "origin":{"x":0.0,"y":0.0,"z":0.0},
            "direction":{"x":1.0,"y":0.0,"z":0.0}
        }});
        let found = super::record("curves", &row);
        assert!(
            found.variants.contains("SolvedCurveGeometry::Line"),
            "{:?}",
            found.variants
        );
        assert!(found.tags.contains_key("model.curves[].geometry.kind"));
    }
}
