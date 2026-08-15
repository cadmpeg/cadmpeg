// SPDX-License-Identifier: Apache-2.0
//! `TechDraw` page and view graph recovery.

use std::collections::{BTreeMap, BTreeSet, HashMap};

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
        .filter(|object| is_registered_drawing_type(&object.type_name))
        .map(|object| {
            let owned = by_owner
                .get(object.id.as_str())
                .cloned()
                .unwrap_or_default();
            ensure_unique_property_names(&owned)?;
            let (views, template) = if matches!(
                object.type_name.as_str(),
                "TechDraw::DrawPage" | "TechDraw::DrawPagePython"
            ) {
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
                sources: [
                    "Source",
                    "XSource",
                    "Sources",
                    "References2D",
                    "References3D",
                    "Source3d",
                ]
                .into_iter()
                .map(|name| source_links(&owned, name))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten()
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
) -> Result<(), CodecError> {
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
        let x = parameter("X")?;
        let y = parameter("Y")?;
        let position = match (x, y) {
            (None, None) => None,
            (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Some([x, y]),
            (Some(_), Some(_)) => {
                return Err(CodecError::Malformed(format!(
                    "drawing {} has a non-finite position",
                    record.id
                )))
            }
            _ => {
                return Err(CodecError::Malformed(format!(
                    "drawing {} position requires both X and Y",
                    record.id
                )))
            }
        };
        let scale = parameter("Scale")?;
        if scale.is_some_and(|scale| !scale.is_finite() || scale <= 0.0) {
            return Err(CodecError::Malformed(format!(
                "drawing {} has a non-positive or non-finite scale",
                record.id
            )));
        }
        let rotation_degrees = parameter("Rotation")?;
        if rotation_degrees.is_some_and(|rotation| !rotation.is_finite()) {
            return Err(CodecError::Malformed(format!(
                "drawing {} has a non-finite rotation",
                record.id
            )));
        }
        let direction = if record.parameters.contains_key("Direction") {
            let direction = vector_property(&owned, "Direction")?.ok_or_else(|| {
                CodecError::Malformed(format!("drawing {} has no direction vector", record.id))
            })?;
            let length_squared = direction
                .iter()
                .map(|component| component * component)
                .sum::<f64>();
            if direction.iter().any(|component| !component.is_finite())
                || length_squared <= f64::EPSILON
            {
                return Err(CodecError::Malformed(format!(
                    "drawing {} has a non-finite or zero direction",
                    record.id
                )));
            }
            Some(direction)
        } else {
            None
        };
        let relationships = record
            .relationships
            .iter()
            .map(|(role, targets)| (role.clone(), targets.iter().map(relationship).collect()))
            .collect::<BTreeMap<_, _>>();
        let template = record
            .relationships
            .get("Template")
            .and_then(|targets| targets.first())
            .and_then(|link| {
                if link.document.is_some() {
                    return None;
                }
                let object = link.object.as_deref().filter(|object| !object.is_empty())?;
                neutral_ids.get(object).cloned()
            });
        model.drawings.push(Drawing {
            id: DrawingId(neutral_ids[record.object.as_str()].clone()),
            object: record.object.clone(),
            kind: classify(&record.kind),
            runtime_type: record.kind.clone(),
            order: order as u32,
            relationships,
            template,
            position,
            scale,
            direction,
            rotation_degrees,
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

fn is_registered_drawing_type(runtime_type: &str) -> bool {
    registered_drawing_kind(runtime_type).is_some()
}

fn classify(runtime_type: &str) -> DrawingKind {
    registered_drawing_kind(runtime_type).unwrap_or(DrawingKind::Other)
}

fn registered_drawing_kind(runtime_type: &str) -> Option<DrawingKind> {
    match runtime_type {
        "TechDraw::DrawPage" | "TechDraw::DrawPagePython" => Some(DrawingKind::Page),
        "TechDraw::DrawTemplate"
        | "TechDraw::DrawTemplatePython"
        | "TechDraw::DrawSVGTemplate"
        | "TechDraw::DrawSVGTemplatePython"
        | "TechDraw::DrawDXFTemplate"
        | "TechDraw::DrawParametricTemplate"
        | "TechDraw::DrawParametricTemplatePython" => Some(DrawingKind::Template),
        "TechDraw::DrawView" | "TechDraw::DrawViewPython" | "TechDraw::DrawViewCollection" => {
            Some(DrawingKind::Other)
        }
        "TechDraw::DrawViewPart"
        | "TechDraw::DrawViewPartPython"
        | "TechDraw::DrawViewMulti"
        | "TechDraw::DrawViewMultiPython"
        | "TechDraw::DrawViewArch"
        | "TechDraw::DrawViewDraft"
        | "TechDraw::DrawViewDraftPython"
        | "TechDraw::DrawViewSpreadsheet"
        | "TechDraw::DrawViewSpreadsheetPython"
        | "TechDraw::DrawViewClip"
        | "TechDraw::DrawViewClipPython"
        | "TechDraw::DrawBrokenView"
        | "TechDraw::DrawBrokenViewPython" => Some(DrawingKind::View),
        "TechDraw::DrawViewDimension"
        | "TechDraw::DrawViewDimExtent"
        | "TechDraw::LandmarkDimension" => Some(DrawingKind::Dimension),
        "TechDraw::DrawViewSection"
        | "TechDraw::DrawViewSectionPython"
        | "TechDraw::DrawComplexSection"
        | "TechDraw::DrawComplexSectionPython" => Some(DrawingKind::Section),
        "TechDraw::DrawProjGroup" | "TechDraw::DrawProjGroupItem" => Some(DrawingKind::Projection),
        "TechDraw::DrawViewDetail" | "TechDraw::DrawViewDetailPython" => Some(DrawingKind::Detail),
        "TechDraw::DrawViewImage" | "TechDraw::DrawViewImagePython" => Some(DrawingKind::Image),
        "TechDraw::DrawViewAnnotation"
        | "TechDraw::DrawViewAnnotationPython"
        | "TechDraw::DrawRichAnno"
        | "TechDraw::DrawRichAnnoPython" => Some(DrawingKind::Annotation),
        "TechDraw::DrawViewBalloon" => Some(DrawingKind::Balloon),
        "TechDraw::DrawLeaderLine" | "TechDraw::DrawLeaderLinePython" => Some(DrawingKind::Leader),
        "TechDraw::DrawViewSymbol"
        | "TechDraw::DrawViewSymbolPython"
        | "TechDraw::DrawWeldSymbol"
        | "TechDraw::DrawWeldSymbolPython" => Some(DrawingKind::Symbol),
        "TechDraw::DrawHatch"
        | "TechDraw::DrawHatchPython"
        | "TechDraw::DrawGeomHatch"
        | "TechDraw::DrawGeomHatchPython"
        | "TechDraw::DrawTile"
        | "TechDraw::DrawTilePython"
        | "TechDraw::DrawTileWeld"
        | "TechDraw::DrawTileWeldPython" => Some(DrawingKind::Other),
        _ => None,
    }
}

fn scalar_property(properties: &[&PropertyRecord], name: &str) -> Result<Option<f64>, CodecError> {
    let Some(property) = unique_property(properties, name)? else {
        return Ok(None);
    };
    let Some(value) = root_value(property, name)? else {
        return Err(CodecError::Malformed(format!(
            "drawing property {name} has no root value"
        )));
    };
    scalar_value(value).map(Some).ok_or_else(|| {
        CodecError::Malformed(format!(
            "drawing property {name} has an invalid scalar value"
        ))
    })
}

fn vector_property(
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<[f64; 3]>, CodecError> {
    let Some(property) = unique_property(properties, name)? else {
        return Ok(None);
    };
    let Some(value) = root_value(property, name)? else {
        return Err(CodecError::Malformed(format!(
            "drawing property {name} has no root value"
        )));
    };
    vector_value(value).map(Some).ok_or_else(|| {
        CodecError::Malformed(format!(
            "drawing property {name} has an invalid vector value"
        ))
    })
}

fn source_links(
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Vec<crate::native::LinkTarget>, CodecError> {
    let Some(property) = unique_property(properties, name)? else {
        return Ok(Vec::new());
    };
    let valid_type = match name {
        "Source" => is_link_carrier_type(&property.type_name),
        "XSource" => property.type_name == "App::PropertyXLinkList",
        "Sources" => property.type_name == "App::PropertyLinkList",
        "References2D" | "References3D" | "Source3d" => {
            property.type_name == "App::PropertyLinkSubList"
        }
        _ => false,
    };
    if !valid_type {
        return Err(CodecError::Malformed(format!(
            "drawing source {name} has runtime type {}, which is not a source carrier",
            property.type_name
        )));
    }
    let is_list = is_link_list_type(&property.type_name);
    if !is_list && property.links.len() > 1 {
        return Err(CodecError::Malformed(format!(
            "drawing source {name} has multiple targets",
        )));
    }
    Ok(property.links.clone())
}

fn is_link_carrier_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "App::PropertyLink"
            | "App::PropertyLinkChild"
            | "App::PropertyLinkGlobal"
            | "App::PropertyLinkHidden"
            | "App::PropertyLinkSub"
            | "App::PropertyLinkSubChild"
            | "App::PropertyLinkSubGlobal"
            | "App::PropertyLinkSubHidden"
            | "App::PropertyLinkList"
            | "App::PropertyLinkListChild"
            | "App::PropertyLinkListGlobal"
            | "App::PropertyLinkListHidden"
            | "App::PropertyLinkSubList"
            | "App::PropertyLinkSubListChild"
            | "App::PropertyLinkSubListGlobal"
            | "App::PropertyLinkSubListHidden"
            | "App::PropertyXLink"
            | "App::PropertyXLinkSub"
            | "App::PropertyXLinkSubHidden"
            | "App::PropertyXLinkList"
            | "App::PropertyXLinkSubList"
    )
}

fn is_link_list_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "App::PropertyLinkList"
            | "App::PropertyLinkListChild"
            | "App::PropertyLinkListGlobal"
            | "App::PropertyLinkListHidden"
            | "App::PropertyLinkSubList"
            | "App::PropertyLinkSubListChild"
            | "App::PropertyLinkSubListGlobal"
            | "App::PropertyLinkSubListHidden"
            | "App::PropertyXLinkList"
            | "App::PropertyXLinkSubList"
    )
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
        "LockPosition",
    ];
    const VALIDATED_ONLY_NAMES: &[&str] = &[
        "XDirection",
        "FormatSpecOverTolerance",
        "FormatSpecUnderTolerance",
        "Type",
        "Perspective",
    ];
    let mut parameters = BTreeMap::new();
    for name in NAMES {
        let Some(property) = unique_property(properties, name)? else {
            continue;
        };
        validate_drawing_property(name, property)?;
        let value = root_value(property, name)?.ok_or_else(|| {
            CodecError::Malformed(format!("drawing property {name} has no root value"))
        })?;
        parameters.insert((*name).to_owned(), value.raw_xml.clone());
    }
    for name in VALIDATED_ONLY_NAMES {
        if let Some(property) = unique_property(properties, name)? {
            validate_drawing_property(name, property)?;
        }
    }
    Ok(parameters)
}

fn validate_drawing_property(name: &str, property: &PropertyRecord) -> Result<(), CodecError> {
    if !drawing_property_type_matches(name, &property.type_name) {
        return Err(CodecError::Malformed(format!(
            "drawing property {name} has runtime type {}, which is not its registered carrier",
            property.type_name
        )));
    }
    let Some(value) = root_value(property, name)? else {
        return Err(CodecError::Malformed(format!(
            "drawing property {name} has no root value"
        )));
    };
    if matches!(name, "Direction" | "XDirection") {
        if vector_value(value).is_none() {
            return Err(CodecError::Malformed(format!(
                "drawing property {name} has an invalid vector value"
            )));
        }
    } else if matches!(name, "X" | "Y" | "Scale" | "Rotation") && scalar_value(value).is_none() {
        return Err(CodecError::Malformed(format!(
            "drawing property {name} has an invalid scalar value"
        )));
    }
    Ok(())
}

fn drawing_property_type_matches(name: &str, type_name: &str) -> bool {
    match name {
        "X" | "Y" => matches!(
            type_name,
            "App::PropertyDistance" | "App::PropertyLength" | "App::PropertyFloat"
        ),
        "Scale" => matches!(
            type_name,
            "App::PropertyFloatConstraint" | "App::PropertyFloat"
        ),
        "Rotation" => matches!(type_name, "App::PropertyAngle" | "App::PropertyFloat"),
        "Direction" | "XDirection" => type_name == "App::PropertyVector",
        "Caption" | "FormatSpec" | "FormatSpecOverTolerance" | "FormatSpecUnderTolerance" => {
            type_name == "App::PropertyString"
        }
        "ScaleType" | "MeasureType" | "Type" | "ProjectionType" => {
            type_name == "App::PropertyEnumeration"
        }
        "LockPosition" | "Perspective" => type_name == "App::PropertyBool",
        _ => false,
    }
}

fn ensure_unique_property_names(properties: &[&PropertyRecord]) -> Result<(), CodecError> {
    let mut names = BTreeSet::new();
    for property in properties {
        if !names.insert(property.name.as_str()) {
            return Err(CodecError::Malformed(format!(
                "drawing property {} occurs more than once",
                property.name
            )));
        }
    }
    Ok(())
}

fn root_value<'a>(
    property: &'a PropertyRecord,
    name: &str,
) -> Result<Option<&'a ValueRecord>, CodecError> {
    let (expected_tag, allowed_extra_tags): (&str, &[&str]) = match name {
        "X" | "Y" | "Scale" | "Rotation" => ("Float", &[]),
        "Direction" | "XDirection" => ("PropertyVector", &[]),
        "Caption" | "FormatSpec" | "FormatSpecOverTolerance" | "FormatSpecUnderTolerance" => {
            ("String", &[])
        }
        "ScaleType" | "MeasureType" | "Type" | "ProjectionType" => ("Integer", &["CustomEnumList"]),
        "LockPosition" | "Perspective" => ("Bool", &[]),
        _ => return Ok(None),
    };
    let xml = roxmltree::Document::parse(&property.raw_xml).map_err(|error| {
        CodecError::Malformed(format!(
            "drawing property {} has invalid XML: {error}",
            property.id
        ))
    })?;
    let property_node = xml.root_element();
    let mut selected_orders = Vec::new();
    let mut order = 0;
    for node in property_node.descendants() {
        if !node.is_element() {
            continue;
        }
        if node.id() == property_node.id() {
            continue;
        }
        if node
            .parent()
            .is_some_and(|parent| parent.id() == property_node.id())
        {
            let tag = node.tag_name().name();
            if tag != expected_tag && !allowed_extra_tags.contains(&tag) {
                return Err(CodecError::Malformed(format!(
                    "drawing property {name} has unexpected root element {tag}"
                )));
            }
            if tag == expected_tag {
                selected_orders.push(order);
            }
        }
        order += 1;
    }
    match selected_orders.as_slice() {
        [] => Ok(None),
        [selected_order] => property
            .values
            .iter()
            .find(|value| value.order == *selected_order)
            .map(Some)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "drawing property {} has an unretained root value",
                    property.id
                ))
            }),
        _ => Err(CodecError::Malformed(format!(
            "drawing property {name} has multiple root values"
        ))),
    }
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
