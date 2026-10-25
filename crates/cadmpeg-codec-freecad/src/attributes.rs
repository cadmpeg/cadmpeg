// SPDX-License-Identifier: Apache-2.0
//! Neutral ownership of properties not consumed by a typed operation.

use crate::native::{ObjectRecord, PropertyBody, PropertyRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

mod source;
pub(crate) use source::{numeric_kind, source_property_identity, transfer_source};

/// Attributes belong to Model even when their target is a typed design object.
pub(crate) const fn projection_held(ir: &CadIr) -> bool {
    ir.scope.model
}

pub(crate) fn consumed(ir: &CadIr, property: &PropertyRecord, objects: &[ObjectRecord]) -> bool {
    if property.type_name == "App::PropertyExpressionEngine" {
        return false;
    }
    let object_placement = property.name == "Placement"
        && property.type_name == "App::PropertyPlacement"
        && objects
            .iter()
            .any(|object| object.id().as_str() == property.owner);
    if property.type_name == "Part::PropertyPartShape"
        || object_placement
        || crate::application_geometry::holds_property(ir, &property.type_name, &property.owner)
        || crate::property_policy::is_projection_metadata(property)
        || ir
            .model
            .parameters
            .iter()
            .any(|parameter| parameter.native_ref.as_deref() == Some(property.id.as_str()))
    {
        return true;
    }
    // Cells belong to the spreadsheet even when the sheet is empty and has
    // no parameter whose native_ref would otherwise consume the property.
    if property.name == "cells"
        && property.type_name == "Spreadsheet::PropertySheet"
        && (ir
            .model
            .spreadsheets
            .iter()
            .any(|sheet| sheet.native_ref.as_deref() == Some(property.owner.as_str()))
            || (!ir.scope.design
                && objects.iter().any(|object| {
                    object.id().as_str() == property.owner
                        && object.type_name == "Spreadsheet::Sheet"
                })))
    {
        return true;
    }
    if ir.model.drawings.iter().any(|record| {
        record.object == property.owner
            && (matches!(
                property.name.as_str(),
                "X" | "Y" | "Scale" | "Direction" | "Rotation"
            ) || record
                .parameters
                .keys()
                .any(|key| key.as_str() == property.name)
                || record
                    .relationships
                    .keys()
                    .any(|key| key.as_str() == property.name))
    }) || ir.model.semantic_annotations.iter().any(|record| {
        record.object == property.owner
            && crate::annotation::consumes_property(record, &property.name)
    }) {
        return true;
    }
    let source_object = objects
        .iter()
        .find(|object| object.id().as_str() == property.owner);
    if source_object.is_some_and(|object| crate::product::is_occurrence_kind(&object.type_name))
        && (property.name == "LinkTransform"
            || matches!(
                (property.name.as_str(), property.type_name.as_str()),
                ("PlacementList", "App::PropertyPlacementList")
                    | ("ScaleList", "App::PropertyVectorList")
            ))
    {
        return true;
    }
    // These registered construction inputs remain owned by their source
    // producer even when a damaged independent edge record forces fallback.
    if property.name == "Base"
        && source_object.is_some_and(|object| {
            matches!(object.type_name.as_str(), "Part::Fillet" | "Part::Chamfer")
        })
    {
        return true;
    }
    // Ownership is a source-schema fact, independent of requested delivery.
    // Geometry cuts keep genuine extras on actual model targets but do
    // not mint competing copies of controls owned by neutral design at Full.
    if !ir.scope.design {
        if let Some(object) = objects
            .iter()
            .find(|object| object.id().as_str() == property.owner)
        {
            if crate::design::is_design_object(&object.type_name)
                && !crate::application_geometry::projects_object(object)
                && model_owner(ir, &property.owner).is_none()
            {
                return true;
            }
            let dynamic_parameter = property
                .dynamic
                .as_ref()
                .is_some_and(|dynamic| dynamic.group != "CADIR")
                || object.type_name == "App::VarSet";
            if dynamic_parameter
                && (crate::design::is_design_object(&object.type_name)
                    || object.type_name == "App::VarSet")
                && !matches!(
                    property.name.as_str(),
                    "Label" | "Label2" | "Visibility" | "Suppressed"
                )
                && crate::persistence::scalar_value_tag(&property.type_name).is_some()
            {
                return true;
            }
            if crate::design::is_stored_geometry_feature(&object.type_name) {
                return matches!(
                    property.name.as_str(),
                    "Label" | "Suppressed" | "ExpressionEngine"
                );
            }
            if crate::design::is_design_object(&object.type_name) {
                if matches!(
                    property.name.as_str(),
                    "Label" | "Suppressed" | "ExpressionEngine"
                ) {
                    return true;
                }
                // PartDesign::Feature is always StoredGeometry, including
                // inside a Body where it has no separate product projection.
                // FeatureBase can instead fall back to a Native operation.
                let native_only = object.type_name == "PartDesign::FeatureBase";
                let scalar_operand = !matches!(property.name.as_str(), "Depth" | "Length2")
                    || crate::persistence::scalar_value_tag(&property.type_name).is_some();
                if crate::property_policy::has_neutral_form(&property.name)
                    && scalar_operand
                    && (!native_only
                        || crate::persistence::scalar_value_tag(&property.type_name).is_some())
                {
                    return true;
                }
            }
        }
    }
    if source_object
        .is_some_and(|object| crate::design::is_stored_geometry_feature(&object.type_name))
    {
        return matches!(
            property.name.as_str(),
            "Label" | "Suppressed" | "ExpressionEngine"
        );
    }
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(property.owner.as_str()));
    if feature.is_some() && matches!(property.name.as_str(), "Label" | "Suppressed") {
        return true;
    }
    if feature.is_some_and(
        |feature| match feature.evaluation.definition().operation() {
            FeatureOperation::Native { parameters, .. } => {
                parameters.keys().any(|name| name.as_str() == property.name)
            }
            _ => false,
        },
    ) {
        return true;
    }
    if feature.is_some_and(|feature| {
        !matches!(
            feature.evaluation.definition().operation(),
            FeatureOperation::Native { .. }
        )
    }) {
        return crate::property_policy::has_neutral_form(&property.name);
    }
    if ir
        .model
        .sketches
        .iter()
        .any(|sketch| sketch.native_ref.as_deref() == Some(property.owner.as_str()))
    {
        return matches!(
            property.name.as_str(),
            "Label"
                | "Geometry"
                | "Constraints"
                | "ExternalGeometry"
                | "ExternalGeo"
                | "Support"
                | "ExpressionEngine"
        );
    }
    if ir
        .model
        .product_definitions
        .iter()
        .any(|definition| definition.native_ref.as_deref() == Some(property.owner.as_str()))
        || ir
            .model
            .occurrences
            .iter()
            .any(|occurrence| occurrence.native_ref.as_deref() == Some(property.owner.as_str()))
    {
        return crate::property_policy::has_neutral_form(&property.name);
    }
    false
}

pub(crate) fn target(ir: &CadIr, owner: &str) -> Result<Option<AttributeTarget>, CodecError> {
    if owner == crate::native::native_id("document", "0") {
        return Ok(Some(AttributeTarget::Document));
    }
    linked_owner(ir, owner, true)
        .map(|(arena, id)| record_target(arena, id))
        .transpose()
}

fn model_owner<'a>(ir: &'a CadIr, owner: &str) -> Option<(&'static str, &'a str)> {
    linked_owner(ir, owner, false)
}

fn linked_owner<'a>(
    ir: &'a CadIr,
    owner: &str,
    include_design: bool,
) -> Option<(&'static str, &'a str)> {
    let mut candidate = None::<(bool, usize, &'static str, &'a str)>;
    cadmpeg_ir::schema::native_links::visit_native_references(
        &ir.model,
        &mut |arena, _, id, native_ref| {
            if native_ref != Some(owner) {
                return;
            }
            let Some((rank, registered)) = cadmpeg_ir::ArenaName::ALL
                .iter()
                .enumerate()
                .find(|(_, registered)| registered.as_str() == arena)
            else {
                return;
            };
            let design = registered.class() == cadmpeg_ir::ArenaClass::Design;
            if !ir.scope.holds(registered.class()) || (design && !include_design) {
                return;
            }
            let next = (design, rank, arena, id);
            if candidate
                .is_none_or(|current| (design, rank, id) < (current.0, current.1, current.3))
            {
                candidate = Some(next);
            }
        },
    );
    candidate.map(|(_, _, arena, id)| (arena, id))
}

fn record_target(arena: &str, id: &str) -> Result<AttributeTarget, CodecError> {
    let registered = cadmpeg_ir::document::ArenaName::ALL
        .iter()
        .copied()
        .find(|name| name.as_str() == arena);
    match registered {
        Some(arena) => Ok(AttributeTarget::Record {
            arena,
            id: id.to_owned(),
        }),
        None => Err(CodecError::malformed(
            "attribute target arena is not registered",
        )),
    }
}

/// A property's value order is retained. Container/count and transport fields
/// are framing; only admitted scalar, vector, and reference values project.
pub(crate) fn values(property: &PropertyRecord) -> Option<Vec<AttributeValue>> {
    let Some(PropertyBody::Persisted {
        values,
        links,
        side_entries,
        ..
    }) = &property.body
    else {
        return None;
    };
    if !side_entries.is_empty()
        || (values.has_missing()
            && (property.type_name == "App::PropertyMap"
                || values.iter().any(|value| {
                    value.parent.is_none()
                        && matches!(
                            value.tag.as_str(),
                            "StringList" | "IntegerList" | "IntegerSet"
                        )
                })))
    {
        return None;
    }
    if property.type_name == "App::PropertyExpressionEngine" {
        let mut expressions = values
            .iter()
            .filter(|value| value.tag == "Expression")
            .map(|value| {
                Some(AttributeValue::Expression(
                    cadmpeg_ir::attributes::AttributeExpression {
                        path: value.attributes.get("path")?.clone(),
                        expression: value.attributes.get("expression")?.clone().into(),
                        comment: value.attributes.get("comment").cloned(),
                    },
                ))
            })
            .collect::<Option<Vec<_>>>()?;
        for link in links {
            expressions.push(link_value(link.as_ref()?)?);
        }
        return (!expressions.is_empty()).then_some(expressions);
    }
    if !links.is_empty() {
        return links
            .iter()
            .map(|link| link_value(link.as_ref()?))
            .collect();
    }
    if property.type_name == "App::PropertyMap" {
        let entries = values
            .iter()
            .filter(|value| value.tag == "Item")
            .map(|value| {
                Some(cadmpeg_ir::attributes::AttributeMapEntry {
                    key: value.attributes.get("key")?.clone(),
                    value: value.attributes.get("value")?.clone(),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        return Some(vec![AttributeValue::Map(entries)]);
    }
    let mut projected = Vec::new();
    for value in values {
        let attributes = &value.attributes;
        if let Some(json) = &value.decoded_json {
            projected.push(match &json.value {
                serde_json::Value::Bool(value) => AttributeValue::Boolean(*value),
                serde_json::Value::String(value) => AttributeValue::String(value.clone()),
                serde_json::Value::Number(value) => match value.as_i64() {
                    Some(value) => AttributeValue::Integer(value),
                    None if value.is_f64() => AttributeValue::float(value.as_f64()?)?,
                    None => return None,
                },
                _ => return None,
            });
            continue;
        }
        // An enumeration's choices describe its native grammar; the root
        // Integer is the selected semantic value. Keep all labels in the typed
        // property instead of treating each nested Enum as another operand.
        if property.type_name == "App::PropertyEnumeration"
            && value.tag == "Enum"
            && value
                .parent
                .and_then(|parent| values.iter().find(|value| value.order == parent))
                .is_some_and(|parent| parent.tag == "CustomEnumList" && parent.parent.is_none())
        {
            continue;
        }
        let scalar = if value.tag == "I" {
            attributes.get("v")
        } else {
            attributes.get("value").or_else(|| attributes.get("Value"))
        };
        let item = match value.tag.as_str() {
            "String" | "Unicode" | "Uuid" | "Path" => scalar
                .or(value.text.as_ref())
                .map(|value| AttributeValue::String(value.clone())),
            "Bool" => scalar
                .and_then(|value| crate::native::parse_bool(value))
                .map(AttributeValue::Boolean),
            "Integer" | "Int" | "Unsigned" | "I" | "PropertyColor" => scalar
                .and_then(|value| value.parse().ok())
                .map(AttributeValue::Integer),
            "Float" | "Double" | "Quantity" | "PropertyFloat" => scalar
                .and_then(|value| value.parse().ok())
                .and_then(AttributeValue::float),
            "BoolList" => {
                let text = scalar?;
                for byte in text.bytes() {
                    projected.push(match byte {
                        b'0' => AttributeValue::Boolean(false),
                        b'1' => AttributeValue::Boolean(true),
                        _ => return None,
                    });
                }
                continue;
            }
            "PropertyMatrix" => {
                let coordinates = (1..=4)
                    .flat_map(|row| (1..=4).map(move |column| format!("a{row}{column}")))
                    .map(|key| attributes.get(&key)?.parse::<f64>().ok())
                    .collect::<Option<Vec<_>>>()?;
                AttributeValue::vector(coordinates)
            }
            "PropertyRotation" => {
                let coordinates = ["Ox", "Oy", "Oz", "A"]
                    .map(|key| attributes.get(key)?.parse::<f64>().ok())
                    .into_iter()
                    .collect::<Option<Vec<_>>>()?;
                AttributeValue::vector(coordinates)
            }
            "PropertyPlacement" => {
                let keys = if attributes.contains_key("A") {
                    ["Px", "Py", "Pz", "Ox", "Oy", "Oz", "A"]
                } else {
                    ["Px", "Py", "Pz", "Q0", "Q1", "Q2", "Q3"]
                };
                let coordinates = keys
                    .map(|key| attributes.get(key)?.parse::<f64>().ok())
                    .into_iter()
                    .collect::<Option<Vec<_>>>()?;
                AttributeValue::vector(coordinates)
            }
            "PropertyVector" | "Vector" => {
                let coordinates = ["valueX", "valueY", "valueZ"].map(|key| {
                    attributes
                        .get(key)
                        .and_then(|value| value.parse::<f64>().ok())
                });
                match coordinates {
                    [Some(x), Some(y), Some(z)] => AttributeValue::vector([x, y, z]),
                    _ => None,
                }
            }
            _ if scalar.is_none()
                && value
                    .text
                    .as_deref()
                    .is_none_or(|text| text.chars().all(char::is_whitespace)) =>
            {
                continue
            }
            _ => return None,
        }?;
        projected.push(item);
    }
    if !projected.is_empty()
        || values.iter().any(|value| {
            value.parent.is_none()
                && matches!(
                    value.tag.as_str(),
                    "StringList"
                        | "IntegerList"
                        | "IntegerSet"
                        | "BoolList"
                        | "LinkList"
                        | "LinkSubList"
                        | "XLinkSubList"
                )
                && value
                    .attributes
                    .get("count")
                    .is_none_or(|count| count == "0")
        })
    {
        Some(projected)
    } else {
        None
    }
}

fn link_value(link: &crate::native::LinkTarget) -> Option<AttributeValue> {
    let object = link.object()?.to_owned();
    Some(match link.document() {
        Some(document) => AttributeValue::ExternalReference {
            document: document.as_str().to_owned(),
            object,
            subelements: link.subelements().to_vec(),
        },
        None if link.subelements().is_empty() => AttributeValue::Reference(object),
        None => AttributeValue::SubReference {
            object,
            subelements: link.subelements().to_vec(),
        },
    })
}

/// Canonical semantic text for legacy string-valued parameter maps. This is
/// built from admitted values, never from XML source or a byte-range image.
pub(crate) fn semantic_text(property: &PropertyRecord) -> Option<String> {
    let values = values(property)?;
    let values = values
        .into_iter()
        .map(|value| {
            Some(match value {
                AttributeValue::String(value) | AttributeValue::Reference(value) => {
                    serde_json::Value::String(value)
                }
                AttributeValue::Integer(value) => serde_json::Value::from(value),
                AttributeValue::Float(value) => serde_json::Value::from(value.get()),
                AttributeValue::Boolean(value) => serde_json::Value::Bool(value),
                AttributeValue::Vector(values) => serde_json::Value::Array(
                    values
                        .into_iter()
                        .map(|value| serde_json::Value::from(value.get()))
                        .collect(),
                ),
                AttributeValue::Map(_)
                | AttributeValue::Expression(_)
                | AttributeValue::SubReference { .. }
                | AttributeValue::ExternalReference { .. }
                | AttributeValue::NumericList(_) => return None,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    if let [value] = values.as_slice() {
        return Some(match value {
            serde_json::Value::String(value) => value.clone(),
            value => value.to_string(),
        });
    }
    Some(serde_json::Value::Array(values).to_string())
}

pub(crate) fn document_property_identity_with_json_mode(
    property: &PropertyRecord,
    registered_json: bool,
) -> Option<String> {
    if property.owner != crate::native::native_id("document", "0") {
        return None;
    }
    property_identity_with_json_mode(property, registered_json)
}

fn property_identity_with_json_mode(
    property: &PropertyRecord,
    registered_json: bool,
) -> Option<String> {
    property_identity_with_source_form(property, registered_json, false)
}

fn property_identity_with_source_form(
    property: &PropertyRecord,
    registered_json: bool,
    generic_value: bool,
) -> Option<String> {
    if property.is_transient()
        || crate::property_policy::is_projection_metadata(property)
        || held_unprojectable_link(property)
        || (property.type_name == "App::PropertyPythonObject"
            && values(property).is_none()
            && property
                .values()
                .iter()
                .any(|value| value.decoded_json.is_some()))
    {
        return None;
    }
    let kind = property.type_name.as_str();
    let eligible = kind == "Mesh::PropertyMaterial"
        || generic_value
        || numeric_kind(kind)
        || crate::persistence::scalar_value_tag(kind).is_some()
        || crate::persistence::property_family(kind) == crate::native::PropertyFamily::Link
        || matches!(
            kind,
            "App::PropertyVector"
                | "App::PropertyVectorDistance"
                | "App::PropertyPosition"
                | "App::PropertyDirection"
                | "App::PropertyMatrix"
                | "App::PropertyRotation"
                | "App::PropertyColor"
                | "App::PropertyFont"
                | "App::PropertyString"
                | "App::PropertyFile"
                | "App::PropertyPath"
                | "App::PropertyUUID"
                | "App::PropertyBoolList"
                | "App::PropertyIntegerList"
                | "App::PropertyIntegerSet"
                | "App::PropertyStringList"
                | "App::PropertyMap"
                | "App::PropertyExpressionEngine"
                | "App::PropertyPlacement"
        )
        || (kind == "App::PropertyPythonObject" && registered_json);
    eligible.then(|| crate::native::model_id("attribute", &property.id, "property"))
}

fn held_unprojectable_link(property: &PropertyRecord) -> bool {
    if property
        .links()
        .iter()
        .flatten()
        .any(|link| link.object().is_none())
    {
        return true;
    }
    let grammar = crate::persistence::link_grammar(&property.type_name);
    property.values().iter().any(|value| {
        use crate::persistence::LinkGrammar;
        let key = match (grammar, value.tag.as_str()) {
            (Some(LinkGrammar::Link | LinkGrammar::LinkList), "Link") => "value",
            (Some(LinkGrammar::LinkSub), "LinkSub") => "value",
            (Some(LinkGrammar::LinkSubList), "Link") => "obj",
            (Some(LinkGrammar::XLink | LinkGrammar::XLinkSubList), "XLink") => "name",
            (None, "XLink") if property.type_name == "App::PropertyExpressionEngine" => "name",
            _ => return false,
        };
        value.attributes.get(key).is_some_and(String::is_empty)
    })
}

pub(crate) fn transfer(
    ir: &mut CadIr,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Result<Vec<LossNote>, CodecError> {
    let mut attributes = Vec::new();
    for property in properties {
        if ir
            .model
            .attributes
            .iter()
            .any(|attribute| attribute.native_ref.as_deref() == Some(property.id.as_str()))
        {
            continue;
        }
        if !projection_held(ir)
            || consumed(ir, property, objects)
            || matches!(property.body, Some(PropertyBody::Transient) | None)
        {
            continue;
        }
        if property.owner != crate::native::native_id("document", "0")
            && !objects.iter().any(|object| object.id() == &property.owner)
        {
            continue;
        }
        if let (Some(target), Some(values)) = (target(ir, &property.owner)?, values(property)) {
            attributes.push(SourceAttribute {
                native_ref: Some(property.id.clone()),
                id: AttributeId::mint(crate::native::model_id(
                    "attribute",
                    &property.id,
                    "property",
                ))
                .map_err(CodecError::malformed)?,
                target,
                name: property.name.clone(),
                values,
            });
        }
    }
    ir.model.attributes.extend(attributes);
    Ok(Vec::new())
}

#[cfg(test)]
mod tests;
