// SPDX-License-Identifier: Apache-2.0
//! Semantic annotation graph recovery.

use std::collections::HashMap;

use cadmpeg_ir::document::Model;
use cadmpeg_ir::semantic_annotations::{
    SemanticAnnotation, SemanticAnnotationId, SemanticAnnotationKind, SemanticAnnotationTarget,
};

use crate::native::{DrawingRecord, ObjectRecord, PropertyRecord, SemanticAnnotationRecord};

pub(crate) fn transfer(
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Vec<SemanticAnnotationRecord> {
    let by_owner = properties.iter().fold(
        HashMap::<&str, Vec<&PropertyRecord>>::new(),
        |mut map, property| {
            map.entry(&property.owner).or_default().push(property);
            map
        },
    );
    objects
        .iter()
        .filter(|object| is_annotation_type(&object.type_name))
        .map(|object| {
            let mut owned = by_owner
                .get(object.id.as_str())
                .cloned()
                .unwrap_or_default();
            owned.sort_by_key(|property| (property.byte_start, property.byte_end));
            let references = owned
                .iter()
                .filter(|property| !property.links.is_empty())
                .map(|property| (property.name.clone(), property.links.clone()))
                .collect();
            let parameters = owned
                .iter()
                .filter(|property| property.links.is_empty())
                .map(|property| (property.name.clone(), property.raw_xml.clone()))
                .collect();
            SemanticAnnotationRecord {
                id: crate::native::native_id("annotation", &object.name),
                object: object.id.clone(),
                kind: object.type_name.clone(),
                text: owned
                    .iter()
                    .filter(|property| is_text_property(&property.name))
                    .flat_map(|property| property.values.iter())
                    .filter_map(text_value)
                    .collect(),
                references,
                parameters,
                side_entries: owned
                    .iter()
                    .flat_map(|property| &property.side_entries)
                    .cloned()
                    .collect(),
            }
        })
        .collect()
}

pub(crate) fn transfer_neutral(
    model: &mut Model,
    records: &[SemanticAnnotationRecord],
    properties: &[PropertyRecord],
    drawings: &[DrawingRecord],
) {
    let drawing_ids = drawings
        .iter()
        .map(|drawing| {
            (
                drawing.object.as_str(),
                crate::native::model_id("drawing", &drawing.object, "entity"),
            )
        })
        .collect::<HashMap<_, _>>();
    for (order, record) in records.iter().enumerate() {
        let owned = properties
            .iter()
            .filter(|property| property.owner == record.object)
            .collect::<Vec<_>>();
        let target = |link: &crate::native::LinkTarget| SemanticAnnotationTarget {
            target: link
                .document
                .is_none()
                .then(|| {
                    link.object
                        .as_ref()
                        .filter(|object| !object.is_empty())
                        .map(|object| {
                            drawing_ids
                                .get(object.as_str())
                                .cloned()
                                .unwrap_or_else(|| object.clone())
                        })
                })
                .flatten(),
            external_document: link.document.clone(),
            external_object: link.document.as_ref().and(link.object.clone()),
            is_null: link.document.is_none() && link.object.as_deref() == Some(""),
            subelements: link.subelements.clone(),
        };
        let x = scalar_property(&owned, "X");
        let y = scalar_property(&owned, "Y");
        model.semantic_annotations.push(SemanticAnnotation {
            id: SemanticAnnotationId(crate::native::model_id(
                "semantic-annotation",
                &record.object,
                "content",
            )),
            object: record.object.clone(),
            kind: classify(&record.kind),
            runtime_type: record.kind.clone(),
            order: order as u32,
            text: record.text.clone(),
            references: record
                .references
                .iter()
                .map(|(role, references)| (role.clone(), references.iter().map(target).collect()))
                .collect(),
            value: ["Value", "Measurement", "Distance", "Angle"]
                .into_iter()
                .find_map(|name| scalar_property(&owned, name)),
            format: string_property(&owned, "FormatSpec"),
            position: vector_property(&owned, "Position")
                .or_else(|| x.zip(y).map(|(x, y)| [x, y, 0.0])),
            parameters: record.parameters.clone(),
            assets: record
                .side_entries
                .iter()
                .map(|name| crate::native::native_id("entry", name))
                .collect(),
            native_ref: record.id.clone(),
        });
    }
}

pub(crate) fn is_annotation_type(type_name: &str) -> bool {
    let leaf = type_name.rsplit("::").next().unwrap_or(type_name);
    let leaf = leaf.to_ascii_lowercase();
    [
        "annotation",
        "dimension",
        "balloon",
        "leader",
        "symbol",
        "tolerance",
        "datum",
        "richanno",
    ]
    .iter()
    .any(|token| leaf.contains(token))
}

fn classify(runtime_type: &str) -> SemanticAnnotationKind {
    let leaf = runtime_type.rsplit("::").next().unwrap_or(runtime_type);
    let leaf = leaf.to_ascii_lowercase();
    if leaf.contains("dimension") {
        SemanticAnnotationKind::Dimension
    } else if leaf.contains("tolerance") {
        SemanticAnnotationKind::GeometricTolerance
    } else if leaf.contains("datum") {
        SemanticAnnotationKind::Datum
    } else if leaf.contains("balloon") {
        SemanticAnnotationKind::Balloon
    } else if leaf.contains("leader") {
        SemanticAnnotationKind::Leader
    } else if leaf.contains("symbol") {
        SemanticAnnotationKind::Symbol
    } else if leaf.contains("annotation") || leaf.contains("richanno") {
        SemanticAnnotationKind::Text
    } else {
        SemanticAnnotationKind::Other
    }
}

fn scalar_property(properties: &[&PropertyRecord], name: &str) -> Option<f64> {
    property_attribute(properties, name, &["value", "Value"])?
        .parse()
        .ok()
}

fn string_property(properties: &[&PropertyRecord], name: &str) -> Option<String> {
    property_attribute(properties, name, &["value", "Value", "string", "String"]).map(str::to_owned)
}

fn property_attribute<'a>(
    properties: &[&'a PropertyRecord],
    name: &str,
    attributes: &[&str],
) -> Option<&'a str> {
    let value = properties
        .iter()
        .find(|property| property.name == name)?
        .values
        .first()?;
    attributes
        .iter()
        .find_map(|attribute| value.attributes.get(*attribute).map(String::as_str))
        .or(value.text.as_deref())
}

fn vector_property(properties: &[&PropertyRecord], name: &str) -> Option<[f64; 3]> {
    let attributes = &properties
        .iter()
        .find(|property| property.name == name)?
        .values
        .first()?
        .attributes;
    Some([
        attributes.get("valueX")?.parse().ok()?,
        attributes.get("valueY")?.parse().ok()?,
        attributes.get("valueZ")?.parse().ok()?,
    ])
}

fn is_text_property(name: &str) -> bool {
    matches!(
        name,
        "Text" | "TextLines" | "LabelText" | "FormatSpec" | "Caption" | "Title" | "Label"
    )
}

fn text_value(value: &crate::native::ValueRecord) -> Option<String> {
    value
        .attributes
        .iter()
        .find(|(name, _)| matches!(name.as_str(), "value" | "Value" | "string" | "String"))
        .map(|(_, value)| value.clone())
        .or_else(|| value.text.clone())
        .filter(|value| !value.is_empty())
}
