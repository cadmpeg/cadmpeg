// SPDX-License-Identifier: Apache-2.0
//! Neutral ownership of properties not consumed by a typed operation.

use crate::loss::FreecadLossCode;
use crate::native::{ObjectRecord, PropertyBody, PropertyRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

/// Attributes belong to Model even when their target is a typed design object.
pub(crate) const fn projection_held(ir: &CadIr) -> bool {
    ir.scope.model
}

pub(crate) fn consumed(ir: &CadIr, property: &PropertyRecord, objects: &[ObjectRecord]) -> bool {
    if matches!(property.name.as_str(), "Shape" | "Placement")
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
    // Ownership is a source-schema fact, independent of requested delivery.
    // Geometry cuts keep genuine extras as Source-targeted attributes but do
    // not mint competing copies of controls owned by neutral design at Full.
    if !ir.scope.design {
        if let Some(object) = objects
            .iter()
            .find(|object| object.id().as_str() == property.owner)
        {
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

pub(crate) fn target(ir: &CadIr, owner: &str) -> AttributeTarget {
    if owner == crate::native::native_id("document", "0") {
        return AttributeTarget::Document;
    }
    if let Some(feature) = ir
        .model
        .features
        .iter()
        .find(|record| record.native_ref.as_deref() == Some(owner))
    {
        return AttributeTarget::Feature(feature.id.clone());
    }
    if let Some(sketch) = ir
        .model
        .sketches
        .iter()
        .find(|record| record.native_ref.as_deref() == Some(owner))
    {
        return AttributeTarget::Sketch(sketch.id.clone());
    }
    if let Some(product) = ir
        .model
        .product_definitions
        .iter()
        .find(|record| record.native_ref.as_deref() == Some(owner))
    {
        return AttributeTarget::ProductDefinition(product.id.clone());
    }
    if let Some(occurrence) = ir
        .model
        .occurrences
        .iter()
        .find(|record| record.native_ref.as_deref() == Some(owner))
    {
        return AttributeTarget::Occurrence(occurrence.id.clone());
    }
    if let Some(drawing) = ir
        .model
        .drawings
        .iter()
        .find(|record| record.object == owner)
    {
        return AttributeTarget::Drawing(drawing.id.clone());
    }
    if let Some(annotation) = ir
        .model
        .semantic_annotations
        .iter()
        .find(|record| record.object == owner)
    {
        return AttributeTarget::SemanticAnnotation(annotation.id.clone());
    }
    AttributeTarget::Source {
        source_id: owner.to_owned(),
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
    if !side_entries.is_empty() {
        return None;
    }
    if !links.is_empty() {
        return links
            .iter()
            .map(|link| {
                link.as_ref()
                    .filter(|link| link.document().is_none() && link.subelements().is_empty())
                    .and_then(|link| {
                        link.object()
                            .map(|object| AttributeValue::Reference(object.to_owned()))
                    })
            })
            .collect();
    }
    let mut projected = Vec::new();
    for value in values {
        let attributes = &value.attributes;
        if value.decoded_json.is_some() {
            return None;
        }
        // An enumeration's choices describe its native grammar; the root
        // Integer is the selected semantic value. Keep all labels in the typed
        // property instead of treating each nested Enum as another operand.
        if property.type_name == "App::PropertyEnumeration"
            && value.tag == "Enum"
            && value
                .parent
                .and_then(|parent| values.get(parent))
                .is_some_and(|parent| parent.tag == "CustomEnumList" && parent.parent.is_none())
        {
            continue;
        }
        let scalar = attributes.get("value").or_else(|| attributes.get("Value"));
        let item = match value.tag.as_str() {
            "String" | "Unicode" => scalar.map(|value| AttributeValue::String(value.clone())),
            "Bool" => scalar
                .and_then(|value| crate::native::parse_bool(value))
                .map(AttributeValue::Boolean),
            "Integer" | "Int" | "Unsigned" => scalar
                .and_then(|value| value.parse().ok())
                .map(AttributeValue::Integer),
            "Float" | "Double" | "Quantity" | "PropertyFloat" => scalar
                .and_then(|value| value.parse().ok())
                .and_then(AttributeValue::float),
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
            _ if scalar.is_none() && value.text.as_deref().is_none_or(str::is_empty) => continue,
            _ => return None,
        }?;
        projected.push(item);
    }
    (!projected.is_empty()).then_some(projected)
}

/// Canonical semantic text for legacy string-valued parameter maps. This is
/// built from admitted values, never from XML source or a byte-range image.
pub(crate) fn semantic_text(property: &PropertyRecord) -> Option<String> {
    let values = values(property)?;
    let values = values
        .into_iter()
        .map(|value| match value {
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
        })
        .collect::<Vec<_>>();
    if let [value] = values.as_slice() {
        return Some(match value {
            serde_json::Value::String(value) => value.clone(),
            value => value.to_string(),
        });
    }
    Some(serde_json::Value::Array(values).to_string())
}

pub(crate) fn document_property(
    property: &PropertyRecord,
) -> Result<Option<SourceAttribute>, CodecError> {
    if matches!(property.name.as_str(), "Shape" | "Placement")
        || crate::property_policy::is_projection_metadata(property)
    {
        return Ok(None);
    }
    let Some(values) = values(property) else {
        return Ok(None);
    };
    Ok(Some(SourceAttribute {
        native_ref: Some(property.id.clone()),
        id: AttributeId::mint(crate::native::model_id(
            "attribute",
            &property.id,
            "property",
        ))
        .map_err(CodecError::malformed)?,
        target: AttributeTarget::Document,
        name: property.name.clone(),
        values,
    }))
}

pub(crate) fn transfer(
    ir: &mut CadIr,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Result<Vec<LossNote>, CodecError> {
    transfer_impl(ir, objects, properties, None)
}

pub(crate) fn transfer_with_document_properties(
    ir: &mut CadIr,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    strings: Option<&crate::persistence::document_properties::RootProperties>,
) -> Result<Vec<LossNote>, CodecError> {
    if strings.is_none() {
        return transfer(ir, objects, properties);
    }
    transfer_impl(ir, objects, properties, strings)
}

fn transfer_impl(
    ir: &mut CadIr,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    strings: Option<&crate::persistence::document_properties::RootProperties>,
) -> Result<Vec<LossNote>, CodecError> {
    let mut attributes = Vec::new();
    let mut losses = Vec::new();
    for property in properties {
        if strings.is_some_and(|strings| strings.sources.contains_key(&property.id)) {
            continue;
        }
        if !projection_held(ir)
            || consumed(ir, property, objects)
            || matches!(
                property.body,
                Some(PropertyBody::Transient) | Some(PropertyBody::Unreadable(_)) | None
            )
        {
            continue;
        }
        if property.owner != crate::native::native_id("document", "0")
            && !objects.iter().any(|object| object.id() == &property.owner)
        {
            continue;
        }
        if let Some(values) = values(property) {
            attributes.push(SourceAttribute {
                native_ref: Some(property.id.clone()),
                id: AttributeId::mint(crate::native::model_id(
                    "attribute",
                    &property.id,
                    "property",
                ))
                .map_err(CodecError::malformed)?,
                target: target(ir, &property.owner),
                name: property.name.clone(),
                values,
            });
        } else {
            losses.push(FreecadLossCode::AttributePropertyUnprojected.note(format!("property {} ({}) has no neutral attribute encoding; its admitted typed native values remain held",property.id,property.type_name)));
        }
    }
    ir.model.attributes.extend(attributes);
    Ok(losses)
}

#[cfg(test)]
mod tests;
