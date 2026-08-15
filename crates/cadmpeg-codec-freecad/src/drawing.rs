// SPDX-License-Identifier: Apache-2.0
//! `TechDraw` page and view graph recovery.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::drawings::{Drawing, DrawingId, DrawingKind, DrawingTarget};

use crate::native::{DrawingRecord, ObjectRecord, PropertyRecord, ValueRecord};

pub(crate) fn transfer(
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Result<Vec<DrawingRecord>, CodecError> {
    let by_owner = properties.iter().fold(
        HashMap::<&str, Vec<&PropertyRecord>>::new(),
        |mut map, property| {
            map.entry(&property.owner).or_default().push(property);
            map
        },
    );
    objects
        .iter()
        .filter(|object| object.type_name.starts_with("TechDraw::"))
        .map(|object| {
            let owned = by_owner
                .get(object.id.as_str())
                .cloned()
                .unwrap_or_default();
            let (views, template) = if object.type_name == "TechDraw::DrawPage" {
                let views = typed_links(&owned, "Views", "App::PropertyLinkList")?
                    .into_iter()
                    .filter_map(|link| link.object)
                    .collect();
                let template = typed_single_link(&owned, "Template", "App::PropertyLink")?
                    .and_then(|link| link.object);
                (views, template)
            } else {
                (Vec::new(), None)
            };
            Ok(DrawingRecord {
                id: crate::native::native_id("drawing", &object.name),
                object: object.id.clone(),
                kind: object.type_name.clone(),
                views,
                template,
                sources: ["Source", "References2D", "References3D"]
                    .into_iter()
                    .flat_map(|name| links(&owned, name))
                    .collect(),
                relationships: owned
                    .iter()
                    .filter(|property| !property.links.is_empty())
                    .map(|property| (property.name.clone(), property.links.clone()))
                    .collect(),
                parameters: drawing_parameters(&owned)?,
                side_entries: owned
                    .iter()
                    .flat_map(|property| &property.side_entries)
                    .cloned()
                    .collect(),
            })
        })
        .collect()
}

pub(crate) fn transfer_neutral(
    model: &mut Model,
    records: &[DrawingRecord],
    properties: &[PropertyRecord],
) {
    let neutral_ids = records
        .iter()
        .map(|record| {
            (
                record.object.as_str(),
                crate::native::model_id("drawing", &record.object, "entity"),
            )
        })
        .collect::<HashMap<_, _>>();
    for (order, record) in records.iter().enumerate() {
        let owned = properties
            .iter()
            .filter(|property| property.owner == record.object)
            .collect::<Vec<_>>();
        let relationship = |link: &crate::native::LinkTarget| DrawingTarget {
            target: link
                .document
                .is_none()
                .then(|| {
                    link.object
                        .as_ref()
                        .filter(|object| !object.is_empty())
                        .map(|object| {
                            neutral_ids
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
        let parameter = |name: &str| scalar_property(&owned, name);
        let x = parameter("X");
        let y = parameter("Y");
        model.drawings.push(Drawing {
            id: DrawingId(neutral_ids[record.object.as_str()].clone()),
            object: record.object.clone(),
            kind: classify(&record.kind),
            runtime_type: record.kind.clone(),
            order: order as u32,
            relationships: record
                .relationships
                .iter()
                .map(|(role, targets)| (role.clone(), targets.iter().map(relationship).collect()))
                .collect(),
            template: record.template.as_ref().map(|object| {
                neutral_ids
                    .get(object.as_str())
                    .cloned()
                    .unwrap_or_else(|| object.clone())
            }),
            position: x.zip(y).map(|(x, y)| [x, y]),
            scale: parameter("Scale"),
            direction: record
                .parameters
                .contains_key("Direction")
                .then(|| vector_property(&owned, "Direction"))
                .flatten(),
            rotation_degrees: parameter("Rotation"),
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

fn classify(runtime_type: &str) -> DrawingKind {
    match runtime_type {
        "TechDraw::DrawPage" => DrawingKind::Page,
        "TechDraw::DrawSVGTemplate" | "TechDraw::DrawDXFTemplate" => DrawingKind::Template,
        "TechDraw::DrawViewDimension"
        | "TechDraw::DrawViewDimExtent"
        | "TechDraw::LandmarkDimension" => DrawingKind::Dimension,
        "TechDraw::DrawViewAnnotation"
        | "TechDraw::DrawViewAnnotationPython"
        | "TechDraw::DrawRichAnno"
        | "TechDraw::DrawRichAnnoPython" => DrawingKind::Annotation,
        "TechDraw::DrawViewBalloon" => DrawingKind::Balloon,
        "TechDraw::DrawLeaderLine" | "TechDraw::DrawLeaderLinePython" => DrawingKind::Leader,
        "TechDraw::DrawViewSymbol"
        | "TechDraw::DrawViewSymbolPython"
        | "TechDraw::DrawWeldSymbol"
        | "TechDraw::DrawWeldSymbolPython" => DrawingKind::Symbol,
        "TechDraw::DrawViewDetail" => DrawingKind::Detail,
        "TechDraw::DrawViewSection" => DrawingKind::Section,
        "TechDraw::DrawProjGroup" | "TechDraw::DrawProjGroupItem" => DrawingKind::Projection,
        "TechDraw::DrawViewImage" => DrawingKind::Image,
        "TechDraw::DrawViewPart" | "TechDraw::DrawViewSpreadsheet" | "TechDraw::DrawViewClip" => {
            DrawingKind::View
        }
        _ => DrawingKind::Other,
    }
}

fn scalar_property(properties: &[&PropertyRecord], name: &str) -> Option<f64> {
    let property = unique_property(properties, name).ok().flatten()?;
    let value = unique_value(property).ok().flatten()?;
    scalar_value(value)
}

fn vector_property(properties: &[&PropertyRecord], name: &str) -> Option<[f64; 3]> {
    let property = unique_property(properties, name).ok().flatten()?;
    let value = unique_value(property).ok().flatten()?;
    vector_value(value)
}

fn links(properties: &[&PropertyRecord], name: &str) -> Vec<crate::native::LinkTarget> {
    properties
        .iter()
        .find(|property| property.name == name)
        .map(|property| property.links.clone())
        .unwrap_or_default()
}

fn typed_links(
    properties: &[&PropertyRecord],
    name: &str,
    type_name: &str,
) -> Result<Vec<crate::native::LinkTarget>, CodecError> {
    Ok(typed_property(properties, name, type_name)?
        .map(|property| property.links.clone())
        .unwrap_or_default())
}

fn typed_single_link(
    properties: &[&PropertyRecord],
    name: &str,
    type_name: &str,
) -> Result<Option<crate::native::LinkTarget>, CodecError> {
    let Some(property) = typed_property(properties, name, type_name)? else {
        return Ok(None);
    };
    match property.links.as_slice() {
        [] => Ok(None),
        [link] => Ok(Some(link.clone())),
        _ => Err(CodecError::Malformed(format!("{name} has multiple links"))),
    }
}

fn typed_property<'a>(
    properties: &[&'a PropertyRecord],
    name: &str,
    type_name: &str,
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    let mut matches = properties
        .iter()
        .copied()
        .filter(|property| property.name == name);
    let Some(property) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(CodecError::Malformed(format!(
            "{name} has duplicate carriers"
        )));
    }
    if property.type_name != type_name {
        return Err(CodecError::Malformed(format!(
            "{name} has runtime type {}, expected {type_name}",
            property.type_name
        )));
    }
    Ok(Some(property))
}

fn drawing_parameters(
    properties: &[&PropertyRecord],
) -> Result<BTreeMap<String, String>, CodecError> {
    const NAMES: &[&str] = &[
        "X",
        "Y",
        "Scale",
        "ScaleType",
        "Direction",
        "Rotation",
        "Caption",
        "FormatSpec",
        "MeasureType",
        "ProjectionType",
        "KeepLabel",
        "LockPosition",
    ];
    const NUMERIC_NAMES: &[&str] = &["X", "Y", "Scale", "Rotation"];
    let mut parameters = BTreeMap::new();
    for name in NAMES {
        let Some(property) = unique_property(properties, name)? else {
            continue;
        };
        let Some(value) = unique_value(property)? else {
            return Err(CodecError::Malformed(format!(
                "drawing property {name} has no root value"
            )));
        };
        if *name == "Direction" {
            if vector_value(value).is_none() {
                return Err(CodecError::Malformed(format!(
                    "drawing property {name} has an invalid vector value"
                )));
            }
        } else if NUMERIC_NAMES.contains(name) && scalar_value(value).is_none() {
            return Err(CodecError::Malformed(format!(
                "drawing property {name} has an invalid scalar value"
            )));
        }
        parameters.insert((*name).to_owned(), value.raw_xml.clone());
    }
    Ok(parameters)
}

fn unique_property<'a>(
    properties: &[&'a PropertyRecord],
    name: &str,
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    let mut matches = properties
        .iter()
        .copied()
        .filter(|property| property.name == name);
    let Some(property) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(CodecError::Malformed(format!(
            "drawing property {name} occurs more than once"
        )));
    }
    Ok(Some(property))
}

fn unique_value(property: &PropertyRecord) -> Result<Option<&ValueRecord>, CodecError> {
    match property.values.as_slice() {
        [] => Ok(None),
        [value] => Ok(Some(value)),
        _ => Err(CodecError::Malformed(format!(
            "drawing property {} has multiple root values",
            property.id
        ))),
    }
}

fn scalar_value(value: &ValueRecord) -> Option<f64> {
    value
        .attributes
        .get("value")
        .or_else(|| value.attributes.get("Value"))
        .and_then(|value| value.parse().ok())
}

fn vector_value(value: &ValueRecord) -> Option<[f64; 3]> {
    Some([
        value.attributes.get("valueX")?.parse().ok()?,
        value.attributes.get("valueY")?.parse().ok()?,
        value.attributes.get("valueZ")?.parse().ok()?,
    ])
}

#[cfg(test)]
pub(crate) mod tests;
