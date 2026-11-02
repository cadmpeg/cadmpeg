// SPDX-License-Identifier: Apache-2.0
//! Emit source attributes through their canonical neutral values.

use super::xml::{self, Object};
use crate::loss::FreecadLossCode;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, HashMap};

fn target<'a>(
    ir: &CadIr,
    target: &'a AttributeTarget,
    names: &'a HashMap<String, String>,
) -> Option<&'a String> {
    match target {
        AttributeTarget::Record { arena, id } if ir.model.contains_record(*arena, id) => {
            names.get(id)
        }
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
    values: &[Option<AttributeValue>],
    names: &HashMap<String, String>,
) -> Option<String> {
    let value = match values {
        [Some(AttributeValue::String(value))] => xml::string(name, value),
        [Some(AttributeValue::Boolean(value))] => {
            xml::scalar(name, "App::PropertyBool", "Bool", u8::from(*value))
        }
        [Some(AttributeValue::Integer(value))] => {
            xml::scalar(name, "App::PropertyInteger", "Integer", value)
        }
        [Some(AttributeValue::Float(value))] => {
            xml::scalar(name, "App::PropertyFloat", "Float", value.get())
        }
        [Some(AttributeValue::Vector(vector))] if vector.len() == 3 => xml::property(
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
        [Some(AttributeValue::Reference(reference))] => xml::property(
            name,
            "App::PropertyLink",
            &xml::element("Link", &[("value", names.get(reference)?.clone())]),
        ),
        values
            if !values.is_empty()
                && values.iter().all(|value| {
                    value.is_none() || matches!(value, Some(AttributeValue::String(_)))
                }) =>
        {
            let content = values
                .iter()
                .filter(|value| value.is_some())
                .map(|value| match value {
                    Some(AttributeValue::String(value)) => {
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
                    values.iter().filter(|value| value.is_some()).count()
                ),
            )
        }
        _ => return None,
    };
    Some(value.replacen("<Property ", "<Property group=\"Attributes\" ", 1))
}

// The native list type selects the grammar; a missing scalar value cannot
// promote its property to a list. Resolve each retained local object through
// the same emitted-object name map used by scalar links.
fn ordered_property(
    attribute: &cadmpeg_ir::attributes::SourceAttribute,
    original: &crate::native::PropertyRecord,
    names: &HashMap<String, String>,
    links: bool,
) -> Option<String> {
    let content = attribute
        .values
        .iter()
        .flatten()
        .map(|value| match (links, value) {
            (true, AttributeValue::Reference(object)) => Some(xml::element(
                "Link",
                &[("value", names.get(object)?.clone())],
            )),
            (false, AttributeValue::String(value)) => {
                Some(xml::element("String", &[("value", value.clone())]))
            }
            _ => None,
        })
        .collect::<Option<String>>()?;
    let root = if links { "LinkList" } else { "StringList" };
    Some(xml::native_property(
        original,
        &attribute.name,
        &format!(
            "<{root} count=\"{}\">{content}</{root}>",
            attribute.values.iter().flatten().count()
        ),
    ))
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
        let Some(replacement) = attribute.values.get(cursor).and_then(Option::as_ref) else {
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
            AttributeValue::Reference(value) => {
                let Some(name) = names.get(value.as_str()) else {
                    return Ok(None);
                };
                Some(name.clone())
            }
            AttributeValue::Vector(coordinates) if vector && coordinates.len() == 3 => {
                for (key, coordinate) in ["valueX", "valueY", "valueZ"].into_iter().zip(coordinates)
                {
                    value_insert(&mut value.attributes, key, coordinate.get().to_string());
                }
                None
            }
            AttributeValue::Vector(_)
            | AttributeValue::Map(_)
            | AttributeValue::Expression(_)
            | AttributeValue::SubReference { .. }
            | AttributeValue::ExternalReference { .. }
            | AttributeValue::EmptyReference
            | AttributeValue::Json(_) => return Ok(None),
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
    write_matching(
        ir,
        names,
        objects,
        document,
        losses,
        properties,
        |target| !is_product_or_occurrence(target),
    )
}

/// Attributes owned by product definitions and occurrences resolve after
/// [`super::products::write`] emits their objects.
pub(super) fn write_products(
    ir: &CadIr,
    names: &HashMap<String, String>,
    objects: &mut [Object],
    document: &mut BTreeMap<String, String>,
    losses: &mut Vec<LossNote>,
    properties: &[crate::native::PropertyRecord],
) -> Result<(), cadmpeg_core::CodecError> {
    write_matching(
        ir,
        names,
        objects,
        document,
        losses,
        properties,
        is_product_or_occurrence,
    )
}

fn is_product_or_occurrence(target: &AttributeTarget) -> bool {
    match target {
        AttributeTarget::ProductDefinition(_) | AttributeTarget::Occurrence(_) => true,
        AttributeTarget::Record { arena, .. } => {
            matches!(arena.as_str(), "product_definitions" | "occurrences")
        }
        _ => false,
    }
}

fn write_matching(
    ir: &CadIr,
    names: &HashMap<String, String>,
    objects: &mut [Object],
    document: &mut BTreeMap<String, String>,
    losses: &mut Vec<LossNote>,
    properties: &[crate::native::PropertyRecord],
    include: impl Fn(&AttributeTarget) -> bool,
) -> Result<(), cadmpeg_core::CodecError> {
    for attribute in &ir.model.attributes {
        if !include(&attribute.target) {
            continue;
        }
        let original = match crate::native::unique_property(properties.iter(), |property| {
            attribute.native_ref.as_deref() == Some(property.id.as_str())
        }) {
            Ok(None) if attribute.native_ref.is_some() => {
                losses.push(FreecadLossCode::ExportAttributeNotTransferred.note(format!(
                    "attribute {} ({}) has no native property owner {}{}",
                    attribute.id,
                    attribute.name,
                    attribute.native_ref.as_deref().unwrap_or_default(),
                    MissingPositions(&attribute.values)
                )));
                continue;
            }
            Ok(original) => original,
            Err(_) => {
                losses.push(FreecadLossCode::ExportAttributeNotTransferred.note(format!(
                    "attribute {} ({}) has an ambiguous native property owner {:?}{}",
                    attribute.id,
                    attribute.name,
                    attribute.native_ref,
                    MissingPositions(&attribute.values)
                )));
                continue;
            }
        };
        let holes = attribute.values.iter().any(Option::is_none);
        let ordered_strings = original
            .is_some_and(|property| property.type_name == "App::PropertyStringList")
            || (original.is_none()
                && attribute.values.len() > 1
                && attribute
                    .values
                    .iter()
                    .any(|value| matches!(value, Some(AttributeValue::String(_))))
                && attribute.values.iter().all(|value| {
                    value.is_none() || matches!(value, Some(AttributeValue::String(_)))
                }));
        let ordered_links = original.filter(|property| {
            matches!(
                property.type_name.as_str(),
                "App::PropertyLinkList"
                    | "App::PropertyLinkListChild"
                    | "App::PropertyLinkListGlobal"
                    | "App::PropertyLinkListHidden"
            )
        });
        let encoded = if holes && ordered_strings {
            if let Some(original) = original {
                ordered_property(attribute, original, names, false)
            } else {
                property(&attribute.name, &attribute.values, names)
            }
        } else if let Some(original) = ordered_links.filter(|_| holes) {
            ordered_property(attribute, original, names, true)
        } else if holes {
            None
        } else if let Some(original) = original {
            through_schema(attribute, original, names)?
        } else {
            property(&attribute.name, &attribute.values, names)
        };
        let Some(property) = encoded else {
            losses.push(FreecadLossCode::ExportAttributeNotTransferred.note(format!(
                "attribute {} ({}) has no FreeCAD property encoding{}",
                attribute.id,
                attribute.name,
                MissingPositions(&attribute.values)
            )));
            continue;
        };
        if attribute.target == AttributeTarget::Document {
            note_holes(attribute, losses);
            document.insert(attribute.name.clone(), property);
            continue;
        }
        let Some(object) = target(ir, &attribute.target, names)
            .and_then(|name| objects.iter_mut().find(|object| object.name == *name))
        else {
            losses.push(FreecadLossCode::ExportAttributeNotTransferred.note(format!(
                "attribute {} target has no emitted object{}",
                attribute.id,
                MissingPositions(&attribute.values)
            )));
            continue;
        };
        // A caller may attach an attribute with the same spelling as an
        // operation property. The operation's canonical field owns that lane.
        if object.properties.contains_key(&attribute.name) {
            losses.push(FreecadLossCode::ExportAttributeNotTransferred.note(format!(
                "attribute {} conflicts with operation property {}{}",
                attribute.id,
                attribute.name,
                MissingPositions(&attribute.values)
            )));
            continue;
        }
        note_holes(attribute, losses);
        object.set(&attribute.name, property);
    }
    Ok(())
}

struct MissingPositions<'a>(&'a [Option<AttributeValue>]);

impl std::fmt::Display for MissingPositions<'_> {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.0.iter().any(Option::is_none) {
            return Ok(());
        }
        output.write_str("; unavailable value positions [")?;
        let mut separator = "";
        for (index, value) in self.0.iter().enumerate() {
            if value.is_none() {
                write!(output, "{separator}{index}")?;
                separator = ", ";
            }
        }
        output.write_str("]")
    }
}

fn note_holes(attribute: &cadmpeg_ir::attributes::SourceAttribute, losses: &mut Vec<LossNote>) {
    if attribute.values.iter().any(Option::is_none) {
        losses.push(FreecadLossCode::ExportAttributeNotTransferred.note(format!(
            "attribute {} omits unavailable ordered entries{}",
            attribute.id,
            MissingPositions(&attribute.values)
        )));
    }
}

#[cfg(test)]
mod tests;
