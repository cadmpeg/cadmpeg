// SPDX-License-Identifier: Apache-2.0
//! Emit source attributes through their canonical neutral values.

use super::xml::{self, Object};
use crate::loss::FreecadLossCode;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, HashMap};

mod collections;
mod numeric;
pub(in crate::writer) mod structured;
pub(super) use numeric::prepare as prepare_numeric;

pub(in crate::writer) fn target<'a>(
    ir: &CadIr,
    target: &'a AttributeTarget,
    names: &'a HashMap<String, String>,
) -> Option<&'a String> {
    match target {
        AttributeTarget::Record { arena, id } if arena.as_str() == "drawings" => ir
            .model
            .drawings
            .iter()
            .find(|record| record.id.as_str() == id)
            .and_then(|record| names.get(&record.object)),
        AttributeTarget::Record { arena, id } if arena.as_str() == "semantic_annotations" => ir
            .model
            .semantic_annotations
            .iter()
            .find(|record| record.id.as_str() == id)
            .and_then(|record| names.get(&record.object)),
        AttributeTarget::Record { arena, id }
            if ir.model.contains_record(*arena, id)
                && matches!(
                    arena.as_str(),
                    "features"
                        | "sketches"
                        | "parameters"
                        | "product_definitions"
                        | "occurrences"
                        | "bodies"
                ) =>
        {
            names.get(id)
        }
        AttributeTarget::Source { source_id } => names.get(source_id),
        AttributeTarget::Feature(id) => names.get(id.as_str()),
        AttributeTarget::Sketch(id) => names.get(id.as_str()),
        AttributeTarget::Parameter(id) => names.get(id.as_str()),
        AttributeTarget::ProductDefinition(id) => names.get(id.as_str()),
        AttributeTarget::Occurrence(id) => names.get(id.as_str()),
        AttributeTarget::Body(id) => names.get(id.as_str()),
        AttributeTarget::Record { arena, id } => {
            let mut native = None;
            cadmpeg_ir::schema::native_links::visit_native_references(
                &ir.model,
                &mut |record_arena, _, record_id, reference| {
                    if record_arena == arena.as_str() && record_id == id {
                        native = reference;
                    }
                },
            );
            native.and_then(|reference| names.get(reference))
        }
        _ => None,
    }
}

fn property(
    name: &str,
    values: &[AttributeValue],
    names: &HashMap<String, String>,
) -> Option<String> {
    if let Some(value) = structured::write(name, values, names) {
        return Some(value);
    }
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
    if let Some(value) = structured::through_schema(attribute, original, names)? {
        return Ok(Some(value));
    }
    let mut property = original.clone();
    property.name.clone_from(&attribute.name);
    let Some(old) = crate::attributes::values(original) else {
        return Ok(None);
    };
    if let Some(encoded) = collections::write(attribute, &mut property, names)? {
        return Ok(encoded);
    }
    let bit_list = original.type_name == "App::PropertyBoolList";
    if !bit_list && old.len() != attribute.values.len() {
        return Ok(None);
    }
    if let Some(crate::native::PropertyBody::Persisted { links, .. }) = &mut property.body {
        if !links.is_empty() {
            for (link, replacement) in links.iter_mut().zip(&attribute.values) {
                let AttributeValue::Reference(reference) = replacement else {
                    return Ok(None);
                };
                if !names.contains_key(reference) {
                    return Ok(None);
                }
                let Some(link) = link else {
                    return Ok(None);
                };
                let Some(reference) = cadmpeg_core::text::NonBlankString::new(reference.clone())
                else {
                    return Ok(None);
                };
                link.set_object(reference);
            }
        }
    }
    let Some(values) = property.values_mut() else {
        return Ok(None);
    };
    let mut cursor = 0;
    for value in values {
        if original.type_name == "App::PropertyEnumeration" && value.tag == "Enum" {
            continue;
        }
        if value.tag == "BoolList" && bit_list {
            let Some(bits) = attribute
                .values
                .iter()
                .map(|value| match value {
                    AttributeValue::Boolean(value) => Some(if *value { '1' } else { '0' }),
                    _ => None,
                })
                .collect::<Option<String>>()
            else {
                return Ok(None);
            };
            value.attributes.insert("value".into(), bits);
            cursor = attribute.values.len();
            continue;
        }
        if let Some(json) = &mut value.decoded_json {
            let Some(replacement) = attribute.values.get(cursor) else {
                return Ok(None);
            };
            json.value = match replacement {
                AttributeValue::Integer(value) => serde_json::Value::from(*value),
                AttributeValue::Float(value) => serde_json::Value::from(value.get()),
                AttributeValue::String(value) => serde_json::Value::String(value.clone()),
                AttributeValue::Boolean(value) => serde_json::Value::Bool(*value),
                _ => return Ok(None),
            };
            cursor += 1;
            continue;
        }
        let coordinates = match value.tag.as_str() {
            "PropertyVector" | "Vector" => {
                Some(vec!["valueX".into(), "valueY".into(), "valueZ".into()])
            }
            "PropertyMatrix" => Some(
                (1..=4)
                    .flat_map(|row| (1..=4).map(move |column| format!("a{row}{column}")))
                    .collect::<Vec<_>>(),
            ),
            "PropertyPlacement" => Some(
                if value.attributes.contains_key("A") {
                    ["Px", "Py", "Pz", "Ox", "Oy", "Oz", "A"]
                } else {
                    ["Px", "Py", "Pz", "Q0", "Q1", "Q2", "Q3"]
                }
                .into_iter()
                .map(str::to_owned)
                .collect(),
            ),
            "PropertyRotation" => Some(
                ["Ox", "Oy", "Oz", "A"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
            ),
            _ => None,
        };
        let key = if value.tag == "I" && value.attributes.contains_key("v") {
            Some("v")
        } else if value.attributes.contains_key("value") {
            Some("value")
        } else if value.attributes.contains_key("Value") {
            Some("Value")
        } else {
            None
        };
        let text_string = key.is_none()
            && value.text.is_some()
            && matches!(value.tag.as_str(), "String" | "Unicode" | "Uuid" | "Path");
        if coordinates.is_none() && key.is_none() && !text_string {
            continue;
        }
        let Some(replacement) = attribute.values.get(cursor) else {
            return Ok(None);
        };
        if old
            .get(cursor)
            .is_none_or(|old| std::mem::discriminant(old) != std::mem::discriminant(replacement))
        {
            return Ok(None);
        }
        let scalar = match replacement {
            AttributeValue::Integer(value) => Some(value.to_string()),
            AttributeValue::Float(value) => Some(value.get().to_string()),
            AttributeValue::String(value) => Some(value.clone()),
            AttributeValue::Boolean(value) => Some(u8::from(*value).to_string()),
            AttributeValue::Reference(value) => Some(value.clone()),
            AttributeValue::Vector(replacement)
                if coordinates
                    .as_ref()
                    .is_some_and(|keys| keys.len() == replacement.len()) =>
            {
                for (key, coordinate) in coordinates.into_iter().flatten().zip(replacement) {
                    value_insert(&mut value.attributes, &key, coordinate.get().to_string());
                }
                None
            }
            AttributeValue::Vector(_)
            | AttributeValue::Map(_)
            | AttributeValue::Expression(_)
            | AttributeValue::SubReference { .. }
            | AttributeValue::ExternalReference { .. }
            | AttributeValue::NumericList(_) => return Ok(None),
        };
        if let Some(scalar) = scalar {
            if text_string {
                value.text = Some(scalar);
            } else {
                value
                    .attributes
                    .insert(key.unwrap_or("value").into(), scalar);
            }
        }
        cursor += 1;
    }
    if cursor != attribute.values.len() {
        return Ok(None);
    }
    encode(&property, names)
}

fn encode(
    property: &crate::native::PropertyRecord,
    names: &HashMap<String, String>,
) -> Result<Option<String>, cadmpeg_core::CodecError> {
    for (index, value) in property.values().iter().enumerate() {
        let attributes = xml::value_attributes(value)?;
        let validation = if let Some(parent) = value.parent {
            let Some(parent) = property.values().iter().find(|value| value.order == parent) else {
                return Ok(None);
            };
            crate::gui::value_grammar::validate_child_of(
                &property.type_name,
                &value.tag,
                &parent.tag,
                &attributes,
            )
        } else {
            crate::gui::value_grammar::validate_root(
                &property.type_name,
                &value.tag,
                &attributes,
                property
                    .values()
                    .iter()
                    .filter(|child| child.parent == Some(value.order))
                    .count(),
            )
        }
        .and_then(|()| {
            crate::gui::value_grammar::validate_previous(
                &property.type_name,
                &value.tag,
                value.parent,
                &attributes,
                &property.values()[..index],
            )
        });
        match validation {
            Ok(()) => {}
            Err(cadmpeg_core::CodecError::Malformed(_)) => return Ok(None),
            Err(error) => return Err(error),
        }
    }
    xml::native_named(property, names)
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
    numeric: &numeric::Prepared,
    entries: &mut BTreeMap<String, Vec<u8>>,
    outcomes: &mut cadmpeg_ir::codec::write::NativeDispositions,
) -> Result<(), cadmpeg_core::CodecError> {
    for attribute in &ir.model.attributes {
        if super::property_payloads::appearances::owns_attribute(ir, attribute)? {
            continue;
        }
        let original = properties
            .iter()
            .find(|property| attribute.native_ref.as_deref() == Some(property.id.as_str()));
        let expression_lane = original
            .is_some_and(|property| property.type_name == "App::PropertyExpressionEngine")
            || attribute
                .values
                .iter()
                .any(|value| matches!(value, AttributeValue::Expression(_)));
        if expression_lane
            && attribute.name == "ExpressionEngine"
            && attribute.target != AttributeTarget::Document
        {
            continue;
        }
        let encoded = if expression_lane {
            super::parameters::expressions::property(
                ir,
                attribute,
                names,
                target(ir, &attribute.target, names).map(String::as_str),
                original,
                losses,
            )?
        } else if let Some(encoded) = numeric.xml.get(attribute.id.as_str()) {
            if encoded.is_none() {
                continue;
            }
            encoded.clone()
        } else if let Some(original) = original {
            through_schema(attribute, original, names)?
        } else {
            property(&attribute.name, &attribute.values, names)
        };
        let Some(property) = encoded else {
            numeric.omitted(attribute.id.as_str(), entries);
            losses.push(FreecadLossCode::ExportAttributeNotTransferred.note(format!(
                "attribute {} ({}) has no FreeCAD property encoding",
                attribute.id, attribute.name
            )));
            continue;
        };
        if attribute.target == AttributeTarget::Document {
            document.insert(attribute.name.clone(), property);
            numeric.emitted(attribute.id.as_str(), outcomes);
            continue;
        }
        let Some(object) = target(ir, &attribute.target, names)
            .and_then(|name| objects.iter_mut().find(|object| object.name == *name))
        else {
            numeric.omitted(attribute.id.as_str(), entries);
            losses.push(FreecadLossCode::ExportAttributeNotTransferred.note(format!(
                "attribute {} target has no emitted object",
                attribute.id
            )));
            continue;
        };
        // A caller may attach an attribute with the same spelling as an
        // operation property. The operation's canonical field owns that lane.
        if object.properties.contains_key(&attribute.name) {
            numeric.omitted(attribute.id.as_str(), entries);
            losses.push(FreecadLossCode::ExportAttributeNotTransferred.note(format!(
                "attribute {} conflicts with operation property {}",
                attribute.id, attribute.name
            )));
            continue;
        }
        object.set(&attribute.name, property);
        numeric.emitted(attribute.id.as_str(), outcomes);
    }
    Ok(())
}
