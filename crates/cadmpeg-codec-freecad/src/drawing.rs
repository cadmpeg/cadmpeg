// SPDX-License-Identifier: Apache-2.0
//! `TechDraw` page and view graph recovery.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::drawings::{Drawing, DrawingId, DrawingKind};
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
use cadmpeg_ir::units::{FiniteVector, NonzeroVector};
use cadmpeg_ir::{ReferenceSelection, ReferenceTarget};

use crate::native::{DrawingRecord, ObjectRecord, PropertyRecord, TechDrawKind, ValueRecord};

type PropertyIndex<'a> = BTreeMap<&'a str, &'a PropertyRecord>;

fn drawing_malformed(ctx: &DecodeContext<'_>, message: std::fmt::Arguments<'_>) -> CodecError {
    crate::resource::malformed_charged(ctx, message, "fcstd drawing diagnostic")
}

pub(crate) fn transfer(
    ctx: &DecodeContext<'_>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
) -> Result<Vec<DrawingRecord>, CodecError> {
    if objects.is_empty() {
        return Ok(Vec::new());
    }
    if !ctx.any_by(
        objects,
        |object| Ok(is_registered_drawing_type(&object.type_name)),
        "fcstd drawing object search",
    )? {
        return Ok(Vec::new());
    }
    let by_owner = ctx.collect_scoped_btree_groups(
        properties
            .iter()
            .map(|property| (property.owner.as_str(), property)),
        "fcstd drawing owner properties",
    )?;
    let _owner_storage = by_owner.1;
    let by_owner = by_owner.0;
    let mut drawings = Vec::new();
    let mut object_iter = objects.iter();
    while object_iter.len() != 0 {
        let Some(object) = ctx.next_charged(&mut object_iter, "fcstd drawing objects")? else {
            break;
        };
        if !is_registered_drawing_type(&object.type_name) {
            continue;
        }
        let owned = ctx
            .get_btree_map(
                &by_owner,
                object.id().as_str(),
                "fcstd drawing owner lookup",
            )?
            .map_or(&[][..], Vec::as_slice);
        let property_index = ensure_unique_property_names(ctx, owned)?;
        let _property_storage = property_index.1;
        let property_index = property_index.0;
        let kind = if is_page_type(&object.type_name) {
            let view_links =
                typed_property(ctx, &property_index, "Views", "App::PropertyLinkList")?
                    .map_or(&[][..], PropertyRecord::links);
            let mut views = ctx.collection_vec(view_links.len(), "fcstd drawing page views")?;
            let mut view_link_iter = view_links.iter();
            while view_link_iter.len() != 0 {
                let Some(link) =
                    ctx.next_charged(&mut view_link_iter, "fcstd drawing page links")?
                else {
                    break;
                };
                if let Some(name) = link.as_ref().and_then(crate::native::LinkTarget::object) {
                    views.push(ctx.copy_retained_text(name, "fcstd drawing page view")?);
                }
            }
            let template_link =
                typed_single_link(ctx, &property_index, "Template", "App::PropertyLink")?;
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
            append_source_links(ctx, &property_index, name, &mut sources)?;
        }
        let mut relationships = BTreeMap::new();
        let mut relationship_property_iter = owned.iter();
        while relationship_property_iter.len() != 0 {
            let Some(property) = ctx.next_charged(
                &mut relationship_property_iter,
                "fcstd drawing relationship properties",
            )?
            else {
                break;
            };
            if property.links().is_empty() {
                continue;
            }
            let mut links =
                ctx.collection_vec(property.links().len(), "fcstd drawing relationship links")?;
            let mut link_iter = property.links().iter();
            while link_iter.len() != 0 {
                let Some(link) = ctx.next_charged(&mut link_iter, "fcstd drawing link visits")?
                else {
                    break;
                };
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
        let mut asset_property_iter = owned.iter();
        while asset_property_iter.len() != 0 {
            let Some(property) =
                ctx.next_charged(&mut asset_property_iter, "fcstd drawing asset properties")?
            else {
                break;
            };
            let mut side_entry_iter = property.side_entries().iter();
            while side_entry_iter.len() != 0 {
                let Some(name) =
                    ctx.next_charged(&mut side_entry_iter, "fcstd drawing side-entry names")?
                else {
                    break;
                };
                ctx.reserve_vec(&mut side_entries, 1, "fcstd drawing side entries")?;
                side_entries.push(ctx.copy_retained_text(name, "fcstd drawing side entry")?);
            }
        }
        ctx.push_vec(
            &mut drawings,
            DrawingRecord {
                id: crate::native::native_id_charged(ctx, "drawing", object.name())?,
                object: ctx.copy_retained_text(object.id(), "fcstd drawing object")?,
                kind,
                sources,
                relationships,
                parameters: drawing_parameters(ctx, &property_index)?,
                side_entries,
            },
            "fcstd drawing records",
        )?;
    }
    Ok(drawings)
}

pub(crate) fn transfer_neutral(
    ctx: &DecodeContext<'_>,
    model: &mut Model,
    records: &[DrawingRecord],
    properties: &[PropertyRecord],
) -> Result<(), CodecError> {
    let mut lookup_storage = ctx.reserve_scoped(0, "FreeCAD drawing neutral lookup")?;
    if records.is_empty() {
        return Ok(());
    }
    let mut neutral_ids = BTreeMap::new();
    let mut identity_record_iter = records.iter();
    while identity_record_iter.len() != 0 {
        let Some(record) = ctx.next_charged(
            &mut identity_record_iter,
            "fcstd drawing neutral identity records",
        )?
        else {
            break;
        };
        lookup_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut neutral_ids,
                record.object.as_str(),
                DrawingId::mint(crate::native::model_id_charged(
                    ctx,
                    "drawing",
                    &record.object,
                    "entity",
                )?)
                .map_err(CodecError::malformed)?,
                "fcstd drawing neutral identities",
            )
        })?;
    }
    let by_owner = ctx.collect_scoped_btree_groups(
        properties
            .iter()
            .map(|property| (property.owner.as_str(), property)),
        "fcstd neutral drawing properties",
    )?;
    let _owner_storage = by_owner.1;
    let by_owner = by_owner.0;
    let mut record_iter = records.iter().enumerate();
    while record_iter.len() != 0 {
        let Some((order, record)) =
            ctx.next_charged(&mut record_iter, "fcstd neutral drawing records")?
        else {
            break;
        };
        let owned = ctx
            .get_btree_map(
                &by_owner,
                record.object.as_str(),
                "fcstd neutral drawing owner lookup",
            )?
            .map_or(&[][..], Vec::as_slice);
        let property_index = ensure_unique_property_names(ctx, owned)?;
        let _property_storage = property_index.1;
        let property_index = property_index.0;
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
                        ctx.get_btree_map(
                            &neutral_ids,
                            object,
                            "fcstd drawing relationship identity lookup",
                        )?
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
        let parameter = |name: &str| scalar_property(ctx, &property_index, name);
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
        let direction = if ctx.contains_key_btree_map(
            &record.parameters,
            "Direction",
            "fcstd drawing direction parameter",
        )? {
            let value = vector_property(ctx, &property_index, "Direction")?
                .ok_or_else(|| CodecError::malformed("drawing direction is absent"))?;
            Some(NonzeroVector::from_finite(value).ok_or_else(|| {
                CodecError::malformed("drawing direction must be finite and nonzero")
            })?)
        } else {
            None
        };
        let position = position.map(FiniteVector::from);
        let mut map_storage = ctx.reserve_scoped(0, "fcstd drawing neutral map nodes")?;
        let mut relationships = BTreeMap::new();
        let mut role_iter = record.relationships.iter();
        while role_iter.len() != 0 {
            let Some((role, targets)) =
                ctx.next_charged(&mut role_iter, "fcstd drawing relationship roles")?
            else {
                break;
            };
            let mut selections =
                ctx.collection_vec(targets.len(), "fcstd drawing neutral relationships")?;
            let mut target_iter = targets.iter();
            while target_iter.len() != 0 {
                let Some(link) =
                    ctx.next_charged(&mut target_iter, "fcstd drawing relationship targets")?
                else {
                    break;
                };
                selections.push(relationship(link)?);
            }
            let role = ctx.copy_retained_text(role, "fcstd drawing relationship role")?;
            map_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut relationships,
                    role,
                    selections,
                    "fcstd drawing relationship roles",
                )
            })?;
        }
        let template_id = if matches!(record.kind, TechDrawKind::Page { .. }) {
            let link = ctx
                .get_btree_map(
                    &record.relationships,
                    "Template",
                    "fcstd drawing template relationship",
                )?
                .and_then(|targets| targets.first())
                .and_then(Option::as_ref);
            match link {
                Some(link) if link.document().is_none() => match link.object() {
                    Some(object) => ctx.get_btree_map(
                        &neutral_ids,
                        object,
                        "fcstd drawing template identity lookup",
                    )?,
                    None => None,
                },
                _ => None,
            }
        } else {
            None
        };
        let template = template_id
            .map(|id| id.try_clone_for_decode(ctx, "fcstd drawing template identity"))
            .transpose()?;
        ctx.reserve_vec(&mut model.drawings, 1, "fcstd neutral drawings")?;
        let mut parameters = BTreeMap::new();
        let mut parameter_iter = record.parameters.iter();
        while parameter_iter.len() != 0 {
            let Some((name, value)) =
                ctx.next_charged(&mut parameter_iter, "fcstd drawing parameter entries")?
            else {
                break;
            };
            let name = ctx.copy_retained_text(name, "fcstd drawing parameter name")?;
            let value = ctx.copy_retained_text(value, "fcstd drawing parameter value")?;
            map_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut parameters,
                    name,
                    value,
                    "fcstd drawing neutral parameters",
                )
            })?;
        }
        let mut assets = ctx.collection_vec(record.side_entries.len(), "fcstd drawing assets")?;
        let mut asset_iter = record.side_entries.iter();
        while asset_iter.len() != 0 {
            let Some(name) = ctx.next_charged(&mut asset_iter, "fcstd drawing asset names")? else {
                break;
            };
            assets.push(crate::native::native_id_charged(ctx, "entry", name)?);
        }
        model.drawings.push(Drawing {
            id: ctx
                .get_btree_map(
                    &neutral_ids,
                    record.object.as_str(),
                    "fcstd drawing own identity lookup",
                )?
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
    properties: &PropertyIndex<'_>,
    name: &str,
) -> Result<Option<FiniteReal>, CodecError> {
    let Some(property) = ctx
        .get_btree_map(properties, name, "fcstd drawing carrier lookup")?
        .copied()
    else {
        return Ok(None);
    };
    let Some(value) = root_value(ctx, property, name)? else {
        return Err(drawing_malformed(
            ctx,
            format_args!("drawing property {name} has no root value"),
        ));
    };
    scalar_value(ctx, name, &property.type_name, value)?
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
    properties: &PropertyIndex<'_>,
    name: &str,
) -> Result<Option<FiniteVector<3>>, CodecError> {
    let Some(property) = ctx
        .get_btree_map(properties, name, "fcstd drawing carrier lookup")?
        .copied()
    else {
        return Ok(None);
    };
    let Some(value) = root_value(ctx, property, name)? else {
        return Err(drawing_malformed(
            ctx,
            format_args!("drawing property {name} has no root value"),
        ));
    };
    vector_value(ctx, value)?.map(Some).ok_or_else(|| {
        drawing_malformed(
            ctx,
            format_args!("drawing property {name} has an invalid vector value"),
        )
    })
}

fn append_source_links(
    ctx: &DecodeContext<'_>,
    properties: &PropertyIndex<'_>,
    name: &str,
    links: &mut Vec<Option<crate::native::LinkTarget>>,
) -> Result<(), CodecError> {
    let Some(property) = ctx
        .get_btree_map(properties, name, "fcstd drawing carrier lookup")?
        .copied()
    else {
        return Ok(());
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
    ctx.reserve_vec(links, property.links().len(), "fcstd drawing source links")?;
    let mut link_iter = property.links().iter();
    while link_iter.len() != 0 {
        let Some(link) = ctx.next_charged(&mut link_iter, "fcstd drawing source link visits")?
        else {
            break;
        };
        links.push(
            link.as_ref()
                .map(|link| link.clone_with_context(ctx))
                .transpose()?,
        );
    }
    Ok(())
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
    properties: &PropertyIndex<'a>,
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
    properties: &PropertyIndex<'a>,
    name: &str,
    type_name: &str,
) -> Result<Option<&'a PropertyRecord>, CodecError> {
    let Some(property) = ctx
        .get_btree_map(properties, name, "fcstd drawing carrier lookup")?
        .copied()
    else {
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
    properties: &PropertyIndex<'_>,
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
        let Some(property) = ctx
            .get_btree_map(properties, name, "fcstd drawing carrier lookup")?
            .copied()
        else {
            continue;
        };
        let value = validate_drawing_property(ctx, name, property)?;
        ctx.insert_btree_map(
            &mut parameters,
            ctx.copy_retained_text(name, "fcstd drawing parameter name")?,
            ctx.copy_retained_text(&value.raw_xml, "fcstd drawing parameter XML")?,
            "fcstd drawing parameters",
        )?;
    }
    for name in VALIDATED_ONLY_NAMES {
        if let Some(property) = ctx
            .get_btree_map(properties, name, "fcstd drawing carrier lookup")?
            .copied()
        {
            validate_drawing_property(ctx, name, property)?;
        }
    }
    Ok(parameters)
}

fn validate_drawing_property<'a>(
    ctx: &DecodeContext<'_>,
    name: &str,
    property: &'a PropertyRecord,
) -> Result<&'a ValueRecord, CodecError> {
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
        if vector_value(ctx, value)?.is_none() {
            return Err(drawing_malformed(
                ctx,
                format_args!("drawing property {name} has an invalid vector value"),
            ));
        }
    } else if matches!(name, "X" | "Y" | "Scale" | "Rotation")
        && scalar_value(ctx, name, &property.type_name, value)?.is_none()
    {
        return Err(drawing_malformed(
            ctx,
            format_args!("drawing property {name} has an invalid scalar value"),
        ));
    }
    Ok(value)
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

fn ensure_unique_property_names<'ctx, 'prop>(
    ctx: &'ctx DecodeContext<'_>,
    properties: &[&'prop PropertyRecord],
) -> Result<(PropertyIndex<'prop>, ScopedReservation<'ctx>), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "fcstd drawing property index")?;
    let mut names = BTreeMap::new();
    let mut property_iter = properties.iter();
    while property_iter.len() != 0 {
        let Some(property) =
            ctx.next_charged(&mut property_iter, "fcstd drawing unique property visits")?
        else {
            break;
        };
        let previous = storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut names,
                property.name.as_str(),
                *property,
                "fcstd drawing unique property names",
            )
        })?;
        if previous.is_some() {
            return Err(drawing_malformed(
                ctx,
                format_args!("drawing property {} occurs more than once", property.name),
            ));
        }
    }
    Ok((names, storage))
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
    let property_node = ctx.xml_root_element(xml, "fcstd drawing XML root")?;
    let mut selected_order = None;
    let mut order = 0;
    let mut descendants = property_node.descendants();
    while descendants.len() != 0 {
        let Some(node) = ctx.next_charged(&mut descendants, "fcstd drawing XML descendants")?
        else {
            break;
        };
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
            let expected = ctx.xml_has_tag_name(node, expected_tag, "fcstd drawing root tag")?;
            if !expected && !allowed_extra_tags.contains(&tag) {
                return Err(drawing_malformed(
                    ctx,
                    format_args!("drawing property {name} has unexpected root element {tag}"),
                ));
            }
            if expected && selected_order.replace(order).is_some() {
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
        Some(selected_order) => ctx
            .find_by(
                property.values(),
                |value| Ok(value.order == selected_order),
                "fcstd drawing retained root search",
            )?
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

fn scalar_value(
    ctx: &DecodeContext<'_>,
    name: &str,
    type_name: &str,
    value: &ValueRecord,
) -> Result<Option<FiniteReal>, CodecError> {
    let allowed_attributes: &[&str] =
        if name == "Scale" && type_name == "App::PropertyFloatConstraint" {
            &["value", "min", "max", "step"]
        } else {
            &["value"]
        };
    let mut scalar = None;
    let mut attributes = value.attributes.iter();
    while attributes.len() != 0 {
        let Some((attribute, text)) =
            ctx.next_charged(&mut attributes, "fcstd drawing scalar attributes")?
        else {
            break;
        };
        if !allowed_attributes.contains(&attribute.as_str()) {
            return Ok(None);
        }
        let Some(admitted) = ctx
            .parse_text::<f64>(text, "fcstd drawing scalar parsing")?
            .ok()
            .and_then(FiniteReal::new)
        else {
            return Ok(None);
        };
        if attribute == "value" {
            scalar = Some(admitted);
        }
    }
    Ok(scalar)
}

fn vector_value(
    ctx: &DecodeContext<'_>,
    value: &ValueRecord,
) -> Result<Option<FiniteVector<3>>, CodecError> {
    let mut vector = [0.0; 3];
    for (index, name) in ["valueX", "valueY", "valueZ"].into_iter().enumerate() {
        let Some(text) =
            ctx.get_btree_map(&value.attributes, name, "fcstd drawing vector attribute")?
        else {
            return Ok(None);
        };
        let Ok(scalar) = ctx.parse_text(text, "fcstd drawing vector parsing")? else {
            return Ok(None);
        };
        vector[index] = scalar;
    }
    Ok(FiniteVector::new(vector))
}

#[cfg(test)]
pub(crate) mod tests;
