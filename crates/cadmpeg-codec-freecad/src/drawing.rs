// SPDX-License-Identifier: Apache-2.0
//! `TechDraw` page and view graph recovery.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::drawings::{Drawing, DrawingId, DrawingKind};
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
use cadmpeg_ir::units::{FiniteVector, NonzeroVector};
use cadmpeg_ir::{ReferenceSelection, ReferenceTarget};

use crate::native::{
    sole_named_property, DrawingRecord, ObjectRecord, PropertyRecord, TechDrawKind, ValueRecord,
};

fn drawing_malformed(ctx: &DecodeContext<'_>, message: std::fmt::Arguments<'_>) -> CodecError {
    crate::resource::malformed_charged(ctx, message, "fcstd drawing diagnostic")
}

pub(crate) fn transfer(
    ctx: &DecodeContext<'_>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Result<Vec<DrawingRecord>, CodecError> {
    let mut by_owner = HashMap::<&str, Vec<&PropertyRecord>>::new();
    for property in properties {
        if !by_owner.contains_key(property.owner.as_str()) {
            ctx.reserve_map(&mut by_owner, 1, "fcstd drawing owner index")?;
            by_owner.insert(&property.owner, Vec::new());
        }
        if let Some(owned) = by_owner.get_mut(property.owner.as_str()) {
            ctx.reserve_vec(owned, 1, "fcstd drawing owner properties")?;
            owned.push(property);
        }
    }
    let mut drawings = ctx.collection_vec(objects.len(), "fcstd drawing records")?;
    for object in objects
        .iter()
        .filter(|object| is_registered_drawing_type(&object.type_name))
    {
        let source = by_owner
            .get(object.id.as_str())
            .map_or(&[][..], Vec::as_slice);
        let mut owned = ctx.collection_vec(source.len(), "fcstd drawing selected properties")?;
        owned.extend_from_slice(source);
        ensure_unique_property_names(ctx, &owned)?;
        let kind = if is_page_type(&object.type_name) {
            let view_links = typed_property(ctx, &owned, "Views", "App::PropertyLinkList")?
                .map_or(&[][..], PropertyRecord::links);
            let mut views = ctx.collection_vec(view_links.len(), "fcstd drawing page views")?;
            for link in view_links.iter().flatten() {
                if let Some(name) = link.object() {
                    views.push(ctx.copy_retained_text(name, "fcstd drawing page view")?);
                }
            }
            let template_link = typed_single_link(ctx, &owned, "Template", "App::PropertyLink")?;
            let template = template_link
                .and_then(|link| link.object())
                .map(|name| ctx.copy_retained_text(name, "fcstd drawing page template"))
                .transpose()?;
            TechDrawKind::Page {
                runtime: if object.type_name == "TechDraw::DrawPage" {
                    crate::native::TechDrawPageKind::Page
                } else {
                    crate::native::TechDrawPageKind::Python
                },
                views,
                template,
            }
        } else {
            TechDrawKind::try_new(
                ctx.copy_retained_text(&object.type_name, "fcstd drawing runtime type")?,
                Vec::new(),
                None,
            )
            .map_err(CodecError::malformed)?
        };
        let mut sources = Vec::new();
        for name in [
            "Source",
            "XSource",
            "Sources",
            "References2D",
            "References3D",
            "Source3d",
        ] {
            let links = source_links(ctx, &owned, name)?;
            ctx.reserve_vec(&mut sources, links.len(), "fcstd drawing source links")?;
            sources.extend(links);
        }
        let mut relationships = BTreeMap::new();
        for property in owned.iter().filter(|property| !property.links().is_empty()) {
            let mut links =
                ctx.collection_vec(property.links().len(), "fcstd drawing relationship links")?;
            for link in property.links() {
                links.push(
                    link.as_ref()
                        .map(|link| link.clone_with_context(ctx))
                        .transpose()?,
                );
            }
            ctx.insert_btree_map(
                &mut relationships,
                ctx.copy_retained_text(&property.name, "fcstd drawing relationship name")?,
                links,
                "fcstd drawing relationships",
            )?;
        }
        let mut side_entries = Vec::new();
        for property in &owned {
            for name in property.side_entries() {
                ctx.reserve_vec(&mut side_entries, 1, "fcstd drawing side entries")?;
                side_entries.push(ctx.copy_retained_text(name, "fcstd drawing side entry")?);
            }
        }
        drawings.push(DrawingRecord {
            id: crate::native::native_id_charged(ctx, "drawing", &object.name)?,
            object: ctx.copy_retained_text(&object.id, "fcstd drawing object")?,
            kind,
            sources,
            relationships,
            parameters: drawing_parameters(ctx, &owned)?,
            side_entries,
        });
    }
    Ok(drawings)
}

pub(crate) fn transfer_neutral(
    ctx: &DecodeContext<'_>,
    model: &mut Model,
    records: &[DrawingRecord],
    properties: &[PropertyRecord],
) -> Result<(), CodecError> {
    let mut neutral_ids = HashMap::new();
    ctx.reserve_map(
        &mut neutral_ids,
        records.len(),
        "fcstd drawing neutral identities",
    )?;
    for record in records {
        neutral_ids.insert(
            record.object.as_str(),
            DrawingId::mint(crate::native::model_id_charged(
                ctx,
                "drawing",
                &record.object,
                "entity",
            )?)
            .map_err(CodecError::malformed)?,
        );
    }
    for (order, record) in records.iter().enumerate() {
        let count = properties
            .iter()
            .filter(|property| property.owner == record.object)
            .count();
        let mut owned = ctx.collection_vec(count, "fcstd neutral drawing properties")?;
        owned.extend(
            properties
                .iter()
                .filter(|property| property.owner == record.object),
        );
        let relationship = |link: &Option<crate::native::LinkTarget>| {
            let Some(link) = link.as_ref() else {
                return Ok(ReferenceSelection::new(ReferenceTarget::Null, Vec::new()));
            };
            let target = match (link.document_name(), link.object()) {
                (Some(document), Some(object)) => ReferenceTarget::External {
                    document: ctx
                        .copy_retained_text(document, "fcstd drawing external document")?,
                    object: ctx.copy_retained_text(object, "fcstd drawing external object")?,
                },
                (None, None) => ReferenceTarget::Null,
                (None, Some(object)) => ReferenceTarget::Local(
                    ctx.copy_retained_text(
                        neutral_ids
                            .get(object)
                            .map_or(object, cadmpeg_ir::drawings::DrawingId::as_str),
                        "fcstd drawing local relationship",
                    )?,
                ),
                _ => {
                    return Err(CodecError::malformed(
                        "drawing relationship has no complete target",
                    ));
                }
            };
            Ok(ReferenceSelection::new(
                target,
                ctx.copy_retained_strings(
                    link.subelements(),
                    "fcstd drawing relationship subelements",
                )?,
            ))
        };
        let parameter = |name: &str| scalar_property(ctx, &owned, name);
        let x = parameter("X")?;
        let y = parameter("Y")?;
        let position = match (x, y) {
            (None, None) => None,
            (Some(x), Some(y)) => Some([x, y]),
            _ => {
                return Err(drawing_malformed(
                    ctx,
                    format_args!("drawing {} position requires both X and Y", record.id),
                ))
            }
        };
        let scale = parameter("Scale")?
            .map(|value| {
                PositiveReal::from_finite(value).ok_or_else(|| {
                    CodecError::malformed("drawing scale must be positive and finite")
                })
            })
            .transpose()?;
        let rotation_degrees = parameter("Rotation")?;
        let direction = if record.parameters.contains_key("Direction") {
            let value = vector_property(ctx, &owned, "Direction")?
                .ok_or_else(|| CodecError::malformed("drawing direction is absent"))?;
            Some(NonzeroVector::from_finite(value).ok_or_else(|| {
                CodecError::malformed("drawing direction must be finite and nonzero")
            })?)
        } else {
            None
        };
        let position = position.map(FiniteVector::from);
        let mut relationships = BTreeMap::new();
        for (role, targets) in &record.relationships {
            let mut selections =
                ctx.collection_vec(targets.len(), "fcstd drawing neutral relationships")?;
            for link in targets {
                selections.push(relationship(link)?);
            }
            ctx.insert_btree_map(
                &mut relationships,
                ctx.copy_retained_text(role, "fcstd drawing relationship role")?,
                selections,
                "fcstd drawing relationship roles",
            )?;
        }
        let template_id = if matches!(record.kind, TechDrawKind::Page { .. }) {
            record
                .relationships
                .get("Template")
                .and_then(|targets| targets.first())
                .and_then(Option::as_ref)
                .and_then(|link| {
                    if link.document().is_some() {
                        return None;
                    }
                    let object = link.object()?;
                    neutral_ids.get(object)
                })
        } else {
            None
        };
        let template = template_id
            .map(|id| id.try_clone_for_decode(ctx, "fcstd drawing template identity"))
            .transpose()?;
        ctx.reserve_vec(&mut model.drawings, 1, "fcstd neutral drawings")?;
        let mut parameters = BTreeMap::new();
        for (name, value) in &record.parameters {
            ctx.insert_btree_map(
                &mut parameters,
                ctx.copy_retained_text(name, "fcstd drawing parameter name")?,
                ctx.copy_retained_text(value, "fcstd drawing parameter value")?,
                "fcstd drawing neutral parameters",
            )?;
        }
        let mut assets = ctx.collection_vec(record.side_entries.len(), "fcstd drawing assets")?;
        for name in &record.side_entries {
            assets.push(crate::native::native_id_charged(ctx, "entry", name)?);
        }
        model.drawings.push(Drawing {
            id: neutral_ids
                .get(record.object.as_str())
                .ok_or_else(|| {
                    drawing_malformed(
                        ctx,
                        format_args!("drawing {} has no admitted neutral identity", record.id),
                    )
                })
                .and_then(|id| id.try_clone_for_decode(ctx, "fcstd drawing neutral identity"))?,
            object: ctx.copy_retained_text(&record.object, "fcstd neutral drawing object")?,
            kind: classify(record.kind.as_str()),
            runtime_type: ctx
                .copy_retained_text(record.kind.as_str(), "fcstd drawing neutral runtime type")?,
            order: u32::try_from(order).map_err(|_| {
                ctx.refuse_codec_limit(
                    "FreeCAD ordinal",
                    u64::from(u32::MAX),
                    cadmpeg_core::decode::u64_from_index(order),
                )
            })?,
            visible: None,
            relationships: cadmpeg_core::text::named_entries_for_decode(
                ctx,
                &record.object,
                relationships,
            )?,
            template,
            position,
            scale,
            direction,
            rotation_degrees,
            parameters: cadmpeg_core::text::named_entries_for_decode(
                ctx,
                &record.object,
                parameters,
            )?,
            assets,
            native_ref: ctx.copy_retained_text(&record.id, "fcstd drawing native reference")?,
        });
    }
    Ok(())
}

fn is_registered_drawing_type(runtime_type: &str) -> bool {
    registered_drawing_kind(runtime_type).is_some()
}

fn is_page_type(runtime_type: &str) -> bool {
    matches!(
        runtime_type,
        "TechDraw::DrawPage" | "TechDraw::DrawPagePython"
    )
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

fn scalar_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<FiniteReal>, CodecError> {
    let Some(property) = sole_named_property(ctx, "drawing", properties, name)? else {
        return Ok(None);
    };
    let Some(value) = root_value(ctx, property, name)? else {
        return Err(drawing_malformed(
            ctx,
            format_args!("drawing property {name} has no root value"),
        ));
    };
    scalar_value(name, &property.type_name, value)
        .map(Some)
        .ok_or_else(|| {
            drawing_malformed(
                ctx,
                format_args!("drawing property {name} has an invalid scalar value"),
            )
        })
}

fn vector_property(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Option<FiniteVector<3>>, CodecError> {
    let Some(property) = sole_named_property(ctx, "drawing", properties, name)? else {
        return Ok(None);
    };
    let Some(value) = root_value(ctx, property, name)? else {
        return Err(drawing_malformed(
            ctx,
            format_args!("drawing property {name} has no root value"),
        ));
    };
    vector_value(value).map(Some).ok_or_else(|| {
        drawing_malformed(
            ctx,
            format_args!("drawing property {name} has an invalid vector value"),
        )
    })
}

fn source_links(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
    name: &str,
) -> Result<Vec<Option<crate::native::LinkTarget>>, CodecError> {
    let Some(property) = sole_named_property(ctx, "drawing", properties, name)? else {
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
        return Err(drawing_malformed(
            ctx,
            format_args!(
                "drawing source {name} has runtime type {}, which is not a source carrier",
                property.type_name
            ),
        ));
    }
    let is_list = is_link_list_type(&property.type_name);
    if !is_list && property.links().len() > 1 {
        return Err(drawing_malformed(
            ctx,
            format_args!("drawing source {name} has multiple targets"),
        ));
    }
    let mut links = ctx.collection_vec(
        property.links().len(),
        "fcstd drawing source property links",
    )?;
    for link in property.links() {
        links.push(
            link.as_ref()
                .map(|link| link.clone_with_context(ctx))
                .transpose()?,
        );
    }
    Ok(links)
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

fn typed_single_link<'a>(
    ctx: &DecodeContext<'_>,
    properties: &[&'a PropertyRecord],
    name: &str,
    type_name: &str,
) -> Result<Option<&'a crate::native::LinkTarget>, CodecError> {
    let Some(property) = typed_property(ctx, properties, name, type_name)? else {
        return Ok(None);
    };
    match property.links() {
        [] => Ok(None),
        [link] => Ok(link.as_ref()),
        _ => Err(drawing_malformed(
            ctx,
            format_args!("{name} has multiple links"),
        )),
    }
}

fn typed_property<'a>(
    ctx: &DecodeContext<'_>,
    properties: &[&'a PropertyRecord],
    name: &str,
    type_name: &str,
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    let Some(property) = sole_named_property(ctx, "drawing", properties, name)? else {
        return Ok(None);
    };
    if property.type_name != type_name {
        return Err(drawing_malformed(
            ctx,
            format_args!(
                "{name} has runtime type {}, expected {type_name}",
                property.type_name
            ),
        ));
    }
    Ok(Some(property))
}

fn drawing_parameters(
    ctx: &DecodeContext<'_>,
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
        let Some(property) = sole_named_property(ctx, "drawing", properties, name)? else {
            continue;
        };
        validate_drawing_property(ctx, name, property)?;
        let value = root_value(ctx, property, name)?.ok_or_else(|| {
            drawing_malformed(
                ctx,
                format_args!("drawing property {name} has no root value"),
            )
        })?;
        ctx.insert_btree_map(
            &mut parameters,
            ctx.copy_retained_text(name, "fcstd drawing parameter name")?,
            ctx.copy_retained_text(&value.raw_xml, "fcstd drawing parameter XML")?,
            "fcstd drawing parameters",
        )?;
    }
    for name in VALIDATED_ONLY_NAMES {
        if let Some(property) = sole_named_property(ctx, "drawing", properties, name)? {
            validate_drawing_property(ctx, name, property)?;
        }
    }
    Ok(parameters)
}

fn validate_drawing_property(
    ctx: &DecodeContext<'_>,
    name: &str,
    property: &PropertyRecord,
) -> Result<(), CodecError> {
    if !drawing_property_type_matches(name, &property.type_name) {
        return Err(drawing_malformed(
            ctx,
            format_args!(
                "drawing property {name} has runtime type {}, which is not its registered carrier",
                property.type_name
            ),
        ));
    }
    let Some(value) = root_value(ctx, property, name)? else {
        return Err(drawing_malformed(
            ctx,
            format_args!("drawing property {name} has no root value"),
        ));
    };
    if matches!(name, "Direction" | "XDirection") {
        if vector_value(value).is_none() {
            return Err(drawing_malformed(
                ctx,
                format_args!("drawing property {name} has an invalid vector value"),
            ));
        }
    } else if matches!(name, "X" | "Y" | "Scale" | "Rotation")
        && scalar_value(name, &property.type_name, value).is_none()
    {
        return Err(drawing_malformed(
            ctx,
            format_args!("drawing property {name} has an invalid scalar value"),
        ));
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

fn ensure_unique_property_names(
    ctx: &DecodeContext<'_>,
    properties: &[&PropertyRecord],
) -> Result<(), CodecError> {
    let mut names = BTreeSet::new();
    for property in properties {
        if names.contains(property.name.as_str()) {
            return Err(drawing_malformed(
                ctx,
                format_args!("drawing property {} occurs more than once", property.name),
            ));
        }
        ctx.insert_btree_set(
            &mut names,
            property.name.as_str(),
            "fcstd drawing unique property names",
        )?;
    }
    Ok(())
}

fn root_value<'a>(
    ctx: &DecodeContext<'_>,
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
    let admitted_xml = ctx
        .parse_xml(property.xml.text(), "FreeCAD XML tree")
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            drawing_malformed(
                ctx,
                format_args!("drawing property {} has invalid XML: {error}", property.id),
            )
        })?;
    let xml = admitted_xml.document();
    let property_node = xml.root_element();
    let mut selected_order = None;
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
                return Err(drawing_malformed(
                    ctx,
                    format_args!("drawing property {name} has unexpected root element {tag}"),
                ));
            }
            if tag == expected_tag && selected_order.replace(order).is_some() {
                return Err(drawing_malformed(
                    ctx,
                    format_args!("drawing property {name} has multiple root values"),
                ));
            }
        }
        order += 1;
    }
    match selected_order {
        None => Ok(None),
        Some(selected_order) => property
            .values()
            .iter()
            .find(|value| value.order == selected_order)
            .map(Some)
            .ok_or_else(|| {
                drawing_malformed(
                    ctx,
                    format_args!(
                        "drawing property {} has an unretained root value",
                        property.id
                    ),
                )
            }),
    }
}

fn scalar_value(name: &str, type_name: &str, value: &ValueRecord) -> Option<FiniteReal> {
    let allowed_attributes: &[&str] =
        if name == "Scale" && type_name == "App::PropertyFloatConstraint" {
            &["value", "min", "max", "step"]
        } else {
            &["value"]
        };
    let mut scalar = None;
    for (attribute, text) in &value.attributes {
        if !allowed_attributes.contains(&attribute.as_str()) {
            return None;
        }
        let admitted = FiniteReal::new(text.parse().ok()?)?;
        if attribute == "value" {
            scalar = Some(admitted);
        }
    }
    scalar
}

fn vector_value(value: &ValueRecord) -> Option<FiniteVector<3>> {
    let vector = [
        value.attributes.get("valueX")?.parse().ok()?,
        value.attributes.get("valueY")?.parse().ok()?,
        value.attributes.get("valueZ")?.parse().ok()?,
    ];
    FiniteVector::new(vector)
}

#[cfg(test)]
pub(crate) mod tests;
