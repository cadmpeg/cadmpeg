// SPDX-License-Identifier: Apache-2.0
//! Semantic annotation graph recovery.

use std::collections::HashMap;

use cadmpeg_core::CodecError;
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
        .filter_map(|object| annotation_schema(&object.type_name).map(|schema| (object, schema)))
        .map(|(object, schema)| {
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
                    .filter(|property| schema.text.contains(&property.name.as_str()))
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
) -> Result<(), CodecError> {
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
        let schema = annotation_schema(&record.kind).ok_or_else(|| {
            CodecError::Malformed(format!(
                "semantic annotation {} has unsupported runtime type {}",
                record.id, record.kind
            ))
        })?;
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
        model.semantic_annotations.push(SemanticAnnotation {
            id: SemanticAnnotationId(crate::native::model_id(
                "semantic-annotation",
                &record.object,
                "content",
            )),
            object: record.object.clone(),
            kind: schema.kind.clone(),
            runtime_type: record.kind.clone(),
            order: order as u32,
            text: record.text.clone(),
            references: record
                .references
                .iter()
                .map(|(role, references)| (role.clone(), references.iter().map(target).collect()))
                .collect(),
            value: None,
            format: schema.format.and_then(|name| string_property(&owned, name)),
            position: annotation_position(&owned, schema.position)?,
            parameters: record.parameters.clone(),
            assets: record
                .side_entries
                .iter()
                .map(|name| crate::native::native_id("entry", name))
                .collect(),
            native_ref: record.id.clone(),
        });
    }
    Ok(())
}

pub(crate) fn is_annotation_type(type_name: &str) -> bool {
    annotation_schema(type_name).is_some()
}

#[derive(Clone)]
struct AnnotationSchema {
    kind: SemanticAnnotationKind,
    text: &'static [&'static str],
    format: Option<&'static str>,
    position: PositionCarrier,
}

#[derive(Clone, Copy)]
enum PositionCarrier {
    Vector(&'static str),
    Coordinates(&'static str, &'static str),
}

fn annotation_schema(runtime_type: &str) -> Option<AnnotationSchema> {
    use SemanticAnnotationKind as Kind;
    let schema = match runtime_type {
        "App::Annotation" => AnnotationSchema {
            kind: Kind::Text,
            text: &["LabelText"],
            format: None,
            position: PositionCarrier::Vector("Position"),
        },
        "App::AnnotationLabel" => AnnotationSchema {
            kind: Kind::Text,
            text: &["LabelText"],
            format: None,
            position: PositionCarrier::Vector("TextPosition"),
        },
        "TechDraw::DrawViewAnnotation" | "TechDraw::DrawViewAnnotationPython" => AnnotationSchema {
            kind: Kind::Text,
            text: &["Text"],
            format: None,
            position: PositionCarrier::Coordinates("X", "Y"),
        },
        "TechDraw::DrawRichAnno" | "TechDraw::DrawRichAnnoPython" => AnnotationSchema {
            kind: Kind::Text,
            text: &["AnnoText"],
            format: None,
            position: PositionCarrier::Coordinates("X", "Y"),
        },
        "TechDraw::DrawViewDimension"
        | "TechDraw::DrawViewDimExtent"
        | "TechDraw::LandmarkDimension" => AnnotationSchema {
            kind: Kind::Dimension,
            text: &["FormatSpec"],
            format: Some("FormatSpec"),
            position: PositionCarrier::Coordinates("X", "Y"),
        },
        "TechDraw::DrawViewBalloon" => AnnotationSchema {
            kind: Kind::Balloon,
            text: &["Text"],
            format: None,
            position: PositionCarrier::Coordinates("X", "Y"),
        },
        "TechDraw::DrawLeaderLine" | "TechDraw::DrawLeaderLinePython" => AnnotationSchema {
            kind: Kind::Leader,
            text: &[],
            format: None,
            position: PositionCarrier::Coordinates("X", "Y"),
        },
        "TechDraw::DrawViewSymbol"
        | "TechDraw::DrawViewSymbolPython"
        | "TechDraw::DrawWeldSymbol"
        | "TechDraw::DrawWeldSymbolPython" => AnnotationSchema {
            kind: Kind::Symbol,
            text: &["TailText"],
            format: None,
            position: PositionCarrier::Coordinates("X", "Y"),
        },
        _ => return None,
    };
    Some(schema)
}

fn annotation_position(
    properties: &[&PropertyRecord],
    carrier: PositionCarrier,
) -> Result<Option<[f64; 3]>, CodecError> {
    match carrier {
        PositionCarrier::Vector(name) => optional_vector_property(properties, name),
        PositionCarrier::Coordinates(x_name, y_name) => {
            let x = optional_scalar_property(properties, x_name)?;
            let y = optional_scalar_property(properties, y_name)?;
            match (x, y) {
                (None, None) => Ok(None),
                (Some(x), Some(y)) => Ok(Some([x, y, 0.0])),
                _ => Err(CodecError::Malformed(format!(
                    "annotation position requires both {x_name} and {y_name}"
                ))),
            }
        }
    }
}

fn optional_scalar_property(
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<f64>, CodecError> {
    let Some(property) = properties.iter().find(|property| property.name == name) else {
        return Ok(None);
    };
    scalar_property(properties, name).map(Some).ok_or_else(|| {
        CodecError::Malformed(format!(
            "annotation property {} is not a scalar",
            property.id
        ))
    })
}

fn optional_vector_property(
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<[f64; 3]>, CodecError> {
    let Some(property) = properties.iter().find(|property| property.name == name) else {
        return Ok(None);
    };
    vector_property(properties, name).map(Some).ok_or_else(|| {
        CodecError::Malformed(format!(
            "annotation property {} is not a vector",
            property.id
        ))
    })
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

fn text_value(value: &crate::native::ValueRecord) -> Option<String> {
    value
        .attributes
        .iter()
        .find(|(name, _)| matches!(name.as_str(), "value" | "Value" | "string" | "String"))
        .map(|(_, value)| value.clone())
        .or_else(|| value.text.clone())
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::is_annotation_type;

    #[test]
    fn annotation_registry_uses_exact_runtime_types() {
        for runtime_type in [
            "App::Annotation",
            "App::AnnotationLabel",
            "TechDraw::DrawViewAnnotation",
            "TechDraw::DrawViewAnnotationPython",
            "TechDraw::DrawRichAnno",
            "TechDraw::DrawRichAnnoPython",
            "TechDraw::DrawViewDimension",
            "TechDraw::DrawViewDimExtent",
            "TechDraw::LandmarkDimension",
            "TechDraw::DrawViewBalloon",
            "TechDraw::DrawLeaderLine",
            "TechDraw::DrawLeaderLinePython",
            "TechDraw::DrawViewSymbol",
            "TechDraw::DrawViewSymbolPython",
            "TechDraw::DrawWeldSymbol",
            "TechDraw::DrawWeldSymbolPython",
        ] {
            assert!(is_annotation_type(runtime_type), "{runtime_type}");
        }
        for runtime_type in [
            "Custom::AnnotationCache",
            "TechDraw::DrawViewDatum",
            "TechDraw::DrawViewTolerance",
            "TechDraw::DrawViewDraft",
            "PartDesign::FeatureAddSub",
        ] {
            assert!(!is_annotation_type(runtime_type), "{runtime_type}");
        }
    }
}
