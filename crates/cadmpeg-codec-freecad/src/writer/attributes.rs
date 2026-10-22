// SPDX-License-Identifier: Apache-2.0
//! Emit source attributes through their canonical neutral values.

use super::xml::{self, Object};
use crate::loss::FreecadLossCode;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, HashMap};

fn target<'a>(
    target: &'a AttributeTarget,
    names: &'a HashMap<String, String>,
) -> Option<&'a String> {
    match target {
        AttributeTarget::Source { source_id } => names.get(source_id),
        AttributeTarget::Feature(id) => names.get(id.as_str()),
        AttributeTarget::Sketch(id) => names.get(id.as_str()),
        AttributeTarget::Parameter(id) => names.get(id.as_str()),
        AttributeTarget::ProductDefinition(id) => names.get(id.as_str()),
        AttributeTarget::Occurrence(id) => names.get(id.as_str()),
        AttributeTarget::Body(id) => names.get(id.as_str()),
        _ => None,
    }
}

fn property(
    name: &str,
    values: &[AttributeValue],
    names: &HashMap<String, String>,
) -> Option<String> {
    let value = match values {
        [AttributeValue::String(value)] => xml::string(name, value),
        [AttributeValue::Boolean(value)] => {
            xml::scalar(name, "App::PropertyBool", "Bool", u8::from(*value))
        }
        [AttributeValue::Integer(value)] => {
            xml::scalar(name, "App::PropertyInteger", "Integer", value)
        }
        [AttributeValue::Float(value)] => {
            xml::scalar(name, "App::PropertyFloat", "Float", value.get())
        }
        [AttributeValue::Vector(vector)] if vector.len() == 3 => xml::property(
            name,
            "App::PropertyVector",
            &xml::element(
                "PropertyVector",
                &[
                    ("valueX", vector[0].get().to_string()),
                    ("valueY", vector[1].get().to_string()),
                    ("valueZ", vector[2].get().to_string()),
                ],
            ),
        ),
        [AttributeValue::Reference(reference)] => xml::property(
            name,
            "App::PropertyLink",
            &xml::element("Link", &[("value", names.get(reference)?.clone())]),
        ),
        values
            if !values.is_empty()
                && values
                    .iter()
                    .all(|value| matches!(value, AttributeValue::String(_))) =>
        {
            let content = values
                .iter()
                .map(|value| match value {
                    AttributeValue::String(value) => {
                        Some(xml::element("String", &[("value", value.clone())]))
                    }
                    _ => None,
                })
                .collect::<Option<String>>()?;
            xml::property(
                name,
                "App::PropertyStringList",
                &format!(
                    "<StringList count=\"{}\">{content}</StringList>",
                    values.len()
                ),
            )
        }
        _ => return None,
    };
    Some(value.replacen("<Property ", "<Property group=\"Attributes\" ", 1))
}

fn through_schema(
    attribute: &cadmpeg_ir::attributes::SourceAttribute,
    original: &crate::native::PropertyRecord,
    names: &HashMap<String, String>,
) -> Result<Option<String>, cadmpeg_core::CodecError> {
    let mut property = original.clone();
    property.name.clone_from(&attribute.name);
    let Some(old) = crate::attributes::values(original) else {
        return Ok(None);
    };
    if old.len() != attribute.values.len() {
        return Ok(None);
    }
    let Some(values) = property.values_mut() else {
        return Ok(None);
    };
    let mut cursor = 0;
    for value in values {
        let vector = value.attributes.contains_key("valueX");
        let key = if value.attributes.contains_key("value") {
            Some("value")
        } else if value.attributes.contains_key("Value") {
            Some("Value")
        } else {
            None
        };
        if !vector && key.is_none() {
            continue;
        }
        let Some(replacement) = attribute.values.get(cursor) else {
            return Ok(None);
        };
        if std::mem::discriminant(&old[cursor]) != std::mem::discriminant(replacement) {
            return Ok(None);
        }
        let scalar = match replacement {
            AttributeValue::Integer(value) => Some(value.to_string()),
            AttributeValue::Float(value) => Some(value.get().to_string()),
            AttributeValue::String(value) => Some(value.clone()),
            AttributeValue::Boolean(value) => Some(u8::from(*value).to_string()),
            AttributeValue::Reference(value) => Some(value.clone()),
            AttributeValue::Vector(coordinates) if vector && coordinates.len() == 3 => {
                for (key, coordinate) in ["valueX", "valueY", "valueZ"].into_iter().zip(coordinates)
                {
                    value_insert(&mut value.attributes, key, coordinate.get().to_string());
                }
                None
            }
            AttributeValue::Vector(_) => return Ok(None),
        };
        if let Some(scalar) = scalar {
            value
                .attributes
                .insert(key.unwrap_or("value").into(), scalar);
        }
        cursor += 1;
    }
    if cursor != attribute.values.len() {
        return Ok(None);
    }
    xml::native_named(&property, names)
}

fn value_insert(values: &mut BTreeMap<String, String>, key: &str, value: String) {
    values.insert(key.into(), value);
}

pub(super) fn write(
    ir: &CadIr,
    names: &HashMap<String, String>,
    objects: &mut [Object],
    document: &mut BTreeMap<String, String>,
    losses: &mut Vec<LossNote>,
    properties: &[crate::native::PropertyRecord],
) -> Result<(), cadmpeg_core::CodecError> {
    for attribute in &ir.model.attributes {
        let original = properties.iter().find(|property| {
            crate::native::model_id("attribute", &property.id, "property") == attribute.id.as_str()
        });
        let encoded = if let Some(original) = original {
            through_schema(attribute, original, names)?
        } else {
            property(&attribute.name, &attribute.values, names)
        };
        let Some(property) = encoded else {
            losses.push(FreecadLossCode::ExportAttributeNotTransferred.note(format!(
                "attribute {} ({}) has no FreeCAD property encoding",
                attribute.id, attribute.name
            )));
            continue;
        };
        if attribute.target == AttributeTarget::Document {
            document.insert(attribute.name.clone(), property);
            continue;
        }
        let Some(object) = target(&attribute.target, names)
            .and_then(|name| objects.iter_mut().find(|object| object.name == *name))
        else {
            losses.push(FreecadLossCode::ExportAttributeNotTransferred.note(format!(
                "attribute {} target has no emitted object",
                attribute.id
            )));
            continue;
        };
        // A caller may attach an attribute with the same spelling as an
        // operation property. The operation's canonical field owns that lane.
        if object.properties.contains_key(&attribute.name) {
            losses.push(FreecadLossCode::ExportAttributeNotTransferred.note(format!(
                "attribute {} conflicts with operation property {}",
                attribute.id, attribute.name
            )));
            continue;
        }
        object.set(&attribute.name, property);
    }
    Ok(())
}
