// SPDX-License-Identifier: Apache-2.0
//! Privacy projection of fragment records.
use super::{record, rows};
use cadmpeg_core::CodecError;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Tokens {
    strings: BTreeSet<String>,
    fields: BTreeSet<String>,
}

pub(super) fn strip(
    mut value: Value,
    roots: Vec<String>,
    native_enum_fields: &crate::native::privacy::EnumFields,
    lexical: &BTreeMap<String, String>,
    source_codec: Option<&str>,
) -> Result<(Value, Vec<String>), CodecError> {
    let tokens = tokens(&value)?;
    let mut replacements = lexical.clone();
    let mut scopes = BTreeMap::new();
    let mut row_ids: Vec<String> = rows(&value)
        .iter()
        .filter_map(|row| record(&value, row)["id"].as_str().map(str::to_owned))
        .chain(roots.iter().cloned())
        .collect();
    let mut pending = vec![&value];
    while let Some(value) = pending.pop() {
        match value {
            Value::String(text) if crate::ids::is_valid_identity(text) => {
                row_ids.push(text.clone());
            }
            Value::Array(values) => pending.extend(values),
            Value::Object(fields) => pending.extend(fields.values()),
            _ => {}
        }
    }
    for (index, id) in row_ids.iter().enumerate() {
        {
            let parts: Vec<&str> = id.splitn(3, ':').collect();
            if let [codec, scope, tail] = parts.as_slice() {
                let scope_index = scopes.len();
                let new_scope = scopes.entry((*scope).to_owned()).or_insert_with(|| {
                    lexical
                        .get(*scope)
                        .cloned()
                        .unwrap_or_else(|| format!("repro{scope_index}"))
                });
                let (_, key) = tail
                    .split_once('#')
                    .ok_or_else(|| CodecError::InvalidInput("root identity has no key".into()))?;
                let key = if key
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b':' | b'-' | b'_' | b'+'))
                {
                    key.to_owned()
                } else {
                    format!("record{index}")
                };
                let kind = super::signature::entity_kind(id);
                let codec = if codec
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                {
                    *codec
                } else {
                    "cadir"
                };
                replacements.insert(id.to_owned(), format!("{codec}:{new_scope}:{kind}#{key}"));
            }
        }
    }
    let roots = roots
        .into_iter()
        .map(|id| {
            replacements
                .get(&id)
                .cloned()
                .ok_or_else(|| CodecError::InvalidInput("related root does not resolve".into()))
        })
        .collect::<Result<_, _>>()?;
    replacements.extend(scopes);
    if let (Some(source), Some(codec)) = (value.get_mut("source"), source_codec) {
        *source = serde_json::json!({"identity": {"classification": "unclassified", "format": codec}, "attributes": {}});
    }
    let empty = BTreeSet::new();
    for row in rows(&value)
        .into_iter()
        .filter(|row| row.arena.starts_with("/native/"))
    {
        let original = record(&value, &row)["id"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let fields = native_enum_fields.get(&original).unwrap_or(&empty);
        if let Some(records) = value.pointer_mut(&row.arena).and_then(Value::as_array_mut) {
            sanitize_native(
                &mut records[row.index],
                "",
                "",
                false,
                fields,
                &mut replacements,
            );
        }
    }
    let native = value
        .as_object_mut()
        .and_then(|object| object.remove("native"));
    sanitize(&mut value, "", "", false, &tokens, &mut replacements);
    if let (Some(native), Some(object)) = (native, value.as_object_mut()) {
        object.insert("native".into(), native);
    }
    Ok((value, roots))
}

#[cfg(feature = "schema")]
fn tokens(value: &Value) -> Result<Tokens, CodecError> {
    static SCHEMA: std::sync::OnceLock<Result<Value, String>> = std::sync::OnceLock::new();
    let schema = SCHEMA
        .get_or_init(|| {
            serde_json::to_value(crate::cadir_json_schema()).map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(CodecError::malformed)?;
    let mut tokens = Tokens::default();
    collect_tokens(schema, schema, value, "", &mut BTreeSet::new(), &mut tokens);
    Ok(tokens)
}

#[cfg(not(feature = "schema"))]
fn tokens(_value: &Value) -> Result<Tokens, CodecError> {
    Err(CodecError::NotImplemented(
        "fragment privacy requires the cadmpeg-ir schema feature".into(),
    ))
}

#[cfg(feature = "schema")]
fn applies(schema: &Value, value: &Value) -> bool {
    if let Some(constant) = schema.get("const") {
        return constant == value;
    }
    if let Some(required) = schema["required"].as_array() {
        if required
            .iter()
            .filter_map(Value::as_str)
            .any(|key| value.get(key).is_none())
        {
            return false;
        }
    }
    if let (Some(properties), Some(object)) = (schema["properties"].as_object(), value.as_object())
    {
        for (key, child) in properties {
            if let (Some(constant), Some(actual)) = (child.get("const"), object.get(key)) {
                if constant != actual {
                    return false;
                }
            }
        }
    }
    true
}

#[cfg(feature = "schema")]
fn collect_tokens(
    root: &Value,
    schema: &Value,
    value: &Value,
    path: &str,
    seen: &mut BTreeSet<(String, String)>,
    tokens: &mut Tokens,
) {
    if !applies(schema, value) {
        return;
    }
    if let Some(reference) = schema["$ref"].as_str() {
        if seen.insert((path.into(), reference.into())) {
            if let Some(target) = reference
                .strip_prefix('#')
                .and_then(|pointer| root.pointer(pointer))
            {
                collect_tokens(root, target, value, path, seen, tokens);
            }
        }
    }
    for union in ["oneOf", "anyOf", "allOf"] {
        if let Some(branches) = schema[union].as_array() {
            for branch in branches {
                collect_tokens(root, branch, value, path, seen, tokens);
            }
        }
    }
    if value.is_string()
        && (schema.get("const") == Some(value)
            || schema["enum"]
                .as_array()
                .is_some_and(|values| values.contains(value)))
    {
        tokens.strings.insert(path.into());
    }
    if let Some(object) = value.as_object() {
        for (key, value) in object {
            if schema["properties"].get(key).is_some() {
                let key = key.replace('~', "~0").replace('/', "~1");
                tokens.fields.insert(format!("{path}/{key}"));
            }
            let child = schema["properties"]
                .get(key)
                .or_else(|| schema.get("additionalProperties"));
            if let Some(child) = child {
                let key = key.replace('~', "~0").replace('/', "~1");
                collect_tokens(root, child, value, &format!("{path}/{key}"), seen, tokens);
            }
        }
    }
    if let Some(array) = value.as_array() {
        for (index, value) in array.iter().enumerate() {
            let child = schema["prefixItems"]
                .get(index)
                .or_else(|| schema.get("items"));
            if let Some(child) = child {
                collect_tokens(root, child, value, &format!("{path}/{index}"), seen, tokens);
            }
        }
    }
}

fn private_field(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("name")
        || key.ends_with("_properties")
        || key.ends_with("_content")
        || key.contains("path")
        || key.contains("text")
        || key.contains("label")
        || key.contains("description")
        || key.contains("comment")
        || key.contains("title")
        || key.contains("author")
        || key.contains("uri")
        || matches!(
            key.as_str(),
            "metadata"
                | "attributes"
                | "properties"
                | "expression"
                | "formula"
                | "content"
                | "contents"
                | "member"
                | "font_family"
        )
}

fn sanitize(
    value: &mut Value,
    path: &str,
    key: &str,
    private: bool,
    tokens: &Tokens,
    replacements: &mut BTreeMap<String, String>,
) {
    let private = private || private_field(key);
    match value {
        Value::String(text) => {
            if text.is_empty()
                || tokens.strings.contains(path)
                || (key == "ir_version" && text == crate::document::IR_VERSION)
                || (key == "format" && crate::CodecFormat::from_registry_id(text).is_some())
            {
                return;
            }
            if let Some(replacement) = replacements.get(text) {
                *text = replacement.clone();
            } else {
                let ordinal = replacements.len();
                let replacement = format!("redacted{ordinal}");
                replacements.insert(text.clone(), replacement.clone());
                *text = replacement;
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter_mut().enumerate() {
                sanitize(
                    value,
                    &format!("{path}/{index}"),
                    key,
                    private,
                    tokens,
                    replacements,
                );
            }
        }
        Value::Object(values) => {
            if private {
                let old = std::mem::take(values);
                for (ordinal, (field, mut value)) in old.into_iter().enumerate() {
                    let pointer = field.replace('~', "~0").replace('/', "~1");
                    let field_path = format!("{path}/{pointer}");
                    sanitize(&mut value, &field_path, &field, true, tokens, replacements);
                    let replacement = if tokens.fields.contains(&field_path) {
                        field
                    } else {
                        replacements
                            .entry(field)
                            .or_insert_with(|| format!("key{ordinal}"))
                            .clone()
                    };
                    values.insert(replacement, value);
                }
            } else {
                for (field, value) in values.iter_mut() {
                    let pointer = field.replace('~', "~0").replace('/', "~1");
                    sanitize(
                        value,
                        &format!("{path}/{pointer}"),
                        field,
                        false,
                        tokens,
                        replacements,
                    );
                }
            }
        }
        _ => {}
    }
}

fn sanitize_native(
    value: &mut Value,
    path: &str,
    key: &str,
    private: bool,
    enum_fields: &BTreeSet<String>,
    replacements: &mut BTreeMap<String, String>,
) {
    let private = private || private_field(key);
    match value {
        Value::String(text) => {
            if text.is_empty()
                || (!private
                    && (enum_fields.contains(path)
                        || text.bytes().all(|byte| byte.is_ascii_digit())
                        || is_guid(text)))
            {
                return;
            }
            if let Some(replacement) = replacements.get(text) {
                *text = replacement.clone();
            } else {
                let ordinal = replacements.len();
                let replacement = if text.len() == 36
                    && text
                        .chars()
                        .all(|character| character.is_ascii_hexdigit() || character == '-')
                {
                    format!("00000000-0000-0000-0000-{ordinal:012x}")
                } else {
                    format!("redacted{ordinal}")
                };
                replacements.insert(text.clone(), replacement.clone());
                *text = replacement;
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter_mut().enumerate() {
                sanitize_native(
                    value,
                    &format!("{path}/{index}"),
                    key,
                    private,
                    enum_fields,
                    replacements,
                );
            }
        }
        Value::Object(values) => {
            if matches!(key, "metadata" | "attributes" | "properties")
                || key.ends_with("_properties")
            {
                let old = std::mem::take(values);
                for (ordinal, (field, mut value)) in old.into_iter().enumerate() {
                    let pointer = field.replace('~', "~0").replace('/', "~1");
                    sanitize_native(
                        &mut value,
                        &format!("{path}/{pointer}"),
                        &field,
                        true,
                        enum_fields,
                        replacements,
                    );
                    values.insert(format!("key{ordinal}"), value);
                }
                return;
            }
            for (field, value) in values.iter_mut() {
                let pointer = field.replace('~', "~0").replace('/', "~1");
                sanitize_native(
                    value,
                    &format!("{path}/{pointer}"),
                    field,
                    private,
                    enum_fields,
                    replacements,
                );
            }
        }
        _ => {}
    }
}

fn is_guid(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}
