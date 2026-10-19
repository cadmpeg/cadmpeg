// SPDX-License-Identifier: Apache-2.0
//! Typed scalar and object-selection properties for feature construction.

use super::super::super::xml::{self, Object};
use cadmpeg_core::CodecError;
use serde_json::Value;
use std::collections::HashMap;

pub(super) fn unsupported<T>(message: impl Into<String>) -> Result<T, CodecError> {
    Err(CodecError::UnsupportedContent(message.into()))
}
pub(super) fn tag(value: &Value) -> &str {
    value
        .get("definition")
        .and_then(Value::as_str)
        .or_else(|| value.get("kind").and_then(Value::as_str))
        .unwrap_or("")
}
pub(super) fn number(value: &Value, field: &str) -> Result<f64, CodecError> {
    value
        .as_f64()
        .filter(|number| number.is_finite())
        .ok_or_else(|| CodecError::UnsupportedContent(format!("feature lacks finite {field}")))
}
pub(super) fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str, CodecError> {
    value
        .as_str()
        .ok_or_else(|| CodecError::UnsupportedContent(format!("feature lacks {field}")))
}
pub(super) fn name(id: &str, names: &HashMap<String, String>) -> Result<String, CodecError> {
    names.get(id).cloned().ok_or_else(|| {
        CodecError::UnsupportedContent(format!("feature reference `{id}` has no emitted object"))
    })
}
pub(super) fn length(object: &mut Object, field: &str, value: f64) {
    object.set(
        field,
        xml::scalar(field, "App::PropertyLength", "Float", value),
    );
}
pub(super) fn angle(object: &mut Object, field: &str, value: f64) {
    object.set(
        field,
        xml::scalar(field, "App::PropertyAngle", "Float", value.to_degrees()),
    );
}
pub(super) fn boolean(object: &mut Object, field: &str, value: bool) {
    object.set(
        field,
        xml::scalar(field, "App::PropertyBool", "Bool", value),
    );
}
pub(super) fn enumeration(object: &mut Object, field: &str, value: usize) {
    object.set(
        field,
        xml::scalar(field, "App::PropertyEnumeration", "Integer", value),
    );
}
pub(super) fn selector(value: &Value, variants: &[&str]) -> Result<usize, CodecError> {
    let value = text(value, "enumeration")?;
    variants
        .iter()
        .position(|variant| *variant == value)
        .ok_or_else(|| {
            CodecError::UnsupportedContent(format!(
                "feature enumeration `{value}` has no FreeCAD encoding"
            ))
        })
}
pub(super) fn vector(object: &mut Object, field: &str, value: &Value) -> Result<(), CodecError> {
    let attributes = [("x", "valueX"), ("y", "valueY"), ("z", "valueZ")].map(|(key, attribute)| {
        number(&value[key], key).map(|value| (attribute, value.to_string()))
    });
    let [x, y, z] = attributes;
    object.set(
        field,
        xml::property(
            field,
            "App::PropertyVector",
            &xml::element("PropertyVector", &[x?, y?, z?]),
        ),
    );
    Ok(())
}

pub(super) fn references(
    value: &Value,
    names: &HashMap<String, String>,
) -> Result<Vec<String>, CodecError> {
    match value {
        Value::Null => Ok(Vec::new()),
        Value::String(id) => {
            if let Some(target) = names.get(id) {
                return Ok(vec![target.clone()]);
            }
            let mut targets = Vec::new();
            let (base, range) = if let Some((base, range)) = id.rsplit_once(":links:") {
                let (first, end) = range.split_once("..").ok_or_else(|| {
                    CodecError::UnsupportedContent("native selection range is invalid".into())
                })?;
                let first = first.parse::<usize>().map_err(CodecError::malformed)?;
                let end = end.parse::<usize>().map_err(CodecError::malformed)?;
                (base, first..end)
            } else {
                (id.as_str(), 0..names.len())
            };
            for index in range {
                let Some(target) = names.get(&format!("{base}:link:{index}")) else {
                    break;
                };
                targets.push(target.clone());
            }
            if targets.is_empty() {
                return unsupported(format!("feature reference `{id}` has no emitted object"));
            }
            Ok(targets)
        }
        Value::Array(values) => {
            let mut result = Vec::new();
            for value in values {
                result.extend(references(value, names)?);
            }
            Ok(result)
        }
        Value::Object(fields) => {
            if tag(value) == "unresolved" {
                return unsupported("unresolved feature selection");
            }
            for key in [
                "value",
                "feature",
                "sketch",
                "bodies",
                "body",
                "edges",
                "faces",
                "members",
                "native",
                "reference",
            ] {
                if let Some(value) = fields.get(key) {
                    return references(value, names);
                }
            }
            unsupported("feature selection cannot identify an emitted object")
        }
        _ => unsupported("feature selection is not an object identity"),
    }
}
pub(super) fn link(
    object: &mut Object,
    field: &str,
    value: &Value,
    names: &HashMap<String, String>,
) -> Result<(), CodecError> {
    let targets = references(value, names)?;
    if targets.len() != 1 {
        return unsupported(format!("{field} needs exactly one feature target"));
    }
    object.set(
        field,
        xml::scalar(field, "App::PropertyLink", "Link", &targets[0]),
    );
    Ok(())
}
pub(super) fn links(
    object: &mut Object,
    field: &str,
    value: &Value,
    names: &HashMap<String, String>,
) -> Result<(), CodecError> {
    let targets = references(value, names)?;
    link_names(object, field, &targets);
    Ok(())
}
pub(super) fn link_names(object: &mut Object, field: &str, targets: &[String]) {
    let mut content = format!("<LinkList count=\"{}\">", targets.len());
    for target in targets {
        content.push_str(&xml::element("Link", &[("value", target.clone())]));
    }
    content.push_str("</LinkList>");
    object.set(
        field,
        xml::property(field, "App::PropertyLinkList", &content),
    );
}
pub(super) fn selection(
    object: &mut Object,
    field: &str,
    value: &Value,
    names: &HashMap<String, String>,
) -> Result<(), CodecError> {
    let targets = references(value, names)?;
    let mut ids = Vec::new();
    selection_ids(value, &mut ids);
    let mut subelements = Vec::new();
    for id in ids {
        if let Some(selector) = names.get(&format!("{id}:subelement")) {
            subelements.push(selector.clone());
        }
        for link in 0..names.len() {
            if link != 0 && !names.contains_key(&format!("{id}:link:{link}")) {
                break;
            }
            for index in 0..names.len() {
                let Some(selector) = names.get(&format!("{id}:link:{link}:subelement:{index}"))
                else {
                    break;
                };
                subelements.push(selector.clone());
            }
        }
    }
    let Some(target) = targets.first() else {
        return unsupported(format!("{field} has no selected object"));
    };
    if targets.iter().any(|candidate| candidate != target) {
        return unsupported(format!("{field} selects multiple objects"));
    }
    let mut content = format!(
        "<LinkSub value=\"{}\" count=\"{}\">",
        xml::escaped(target),
        subelements.len()
    );
    for subelement in subelements {
        content.push_str(&xml::element("Sub", &[("value", subelement)]));
    }
    content.push_str("</LinkSub>");
    object.set(
        field,
        xml::property(field, "App::PropertyLinkSub", &content),
    );
    Ok(())
}

fn selection_ids<'a>(value: &'a Value, ids: &mut Vec<&'a str>) {
    match value {
        Value::String(id) => ids.push(id),
        Value::Array(values) => {
            for value in values {
                selection_ids(value, ids);
            }
        }
        Value::Object(fields) => {
            for key in [
                "value",
                "feature",
                "sketch",
                "bodies",
                "body",
                "edges",
                "faces",
                "members",
                "native",
                "reference",
            ] {
                if let Some(value) = fields.get(key) {
                    selection_ids(value, ids);
                    break;
                }
            }
        }
        _ => {}
    }
}
