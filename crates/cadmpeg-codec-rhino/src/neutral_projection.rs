// SPDX-License-Identifier: Apache-2.0
//! Neutral homes for typed Rhino document and object presentation.

use crate::container::Scan;
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::{AttributeId, IdentityKey, LayerId};
use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{CadIr, SourceObjectAssociation};
use std::collections::BTreeMap;

mod annotations;
mod document;
mod materials;
mod views;

fn layer_id(ordinal: usize) -> LayerId {
    LayerId::from(cadmpeg_ir::ids::Identity::compose(
        &cadmpeg_ir::identity_namespace!("rhino", "model", "layer"),
        IdentityKey::zero_padded(cadmpeg_core::decode::u64_from_index(ordinal), 6),
    ))
}

fn source_matches(source: Option<&SourceObjectAssociation>, identity: &str) -> bool {
    source.is_some_and(|source| source.object_id.as_str() == identity)
}

fn items(ir: &CadIr, source_order: usize, identity: &str) -> Vec<PresentationItem> {
    let mut items = Vec::new();
    let source_record = format!("rhino:object:record#{source_order:06}");
    if let Some(records) = ir
        .native
        .namespace("rhino")
        .and_then(|namespace| namespace.arenas().get("product_occurrences"))
    {
        for record in records {
            let owns_object = record
                .field("links")
                .and_then(|value| {
                    value.as_array().map(|links| {
                        links
                            .iter()
                            .any(|link| link.as_str() == Some(source_record.as_str()))
                    })
                })
                .unwrap_or(false);
            if owns_object {
                for occurrence in &ir.model.occurrences {
                    if occurrence.native_ref.as_deref() == Some(record.id()) {
                        items.push(PresentationItem::Occurrence {
                            occurrence: occurrence.id.clone(),
                        });
                    }
                }
            }
        }
    }
    if !items.is_empty() {
        return items;
    }
    if let Some(record) = ir
        .native
        .namespace("rhino")
        .and_then(|namespace| namespace.arenas().get("unknowns"))
        .and_then(|records| records.iter().find(|record| record.id() == source_record))
    {
        if let Some(serde_json::Value::Array(links)) = record.field("links") {
            for link in links.iter().filter_map(serde_json::Value::as_str) {
                if let Some(body) = ir.model.bodies.iter().find(|body| body.id.as_str() == link) {
                    items.push(PresentationItem::Body {
                        body: body.id.clone(),
                    });
                }
            }
        }
    }
    // A source object represented by a body owns its subordinate carriers.
    // Its one layer membership belongs to that body.
    if !items.is_empty() {
        return items;
    }
    for point in &ir.model.points {
        if source_matches(point.source_object.as_ref(), identity) {
            items.push(PresentationItem::Point {
                point: point.id.clone(),
            });
        }
    }
    for curve in &ir.model.curves {
        if source_matches(curve.source_object.as_ref(), identity) {
            items.push(PresentationItem::Curve {
                curve: curve.id.clone(),
            });
        }
    }
    for surface in &ir.model.surfaces {
        if source_matches(surface.source_object.as_ref(), identity) {
            items.push(PresentationItem::Surface {
                surface: surface.id.clone(),
            });
        }
    }
    for mesh in &ir.model.tessellations {
        if source_matches(mesh.source_object.as_ref(), identity) {
            items.push(PresentationItem::Tessellation {
                tessellation: mesh.id.to_string(),
            });
        }
    }
    items
}

fn attribute_target(item: &PresentationItem) -> Option<AttributeTarget> {
    match item {
        PresentationItem::Point { point } => Some(AttributeTarget::Point(point.clone())),
        PresentationItem::Curve { curve } => Some(AttributeTarget::Curve(curve.clone())),
        PresentationItem::Surface { surface } => Some(AttributeTarget::Surface(surface.clone())),
        PresentationItem::Tessellation { tessellation } => {
            Some(AttributeTarget::Tessellation(tessellation.clone()))
        }
        _ => None,
    }
}

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    scan: &Scan<'_>,
    materials: Vec<crate::presentation::MaterialProjection>,
    ir: &mut CadIr,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = materials::project(ctx, scan, materials, ir)?;
    losses.extend(document::project(ctx, scan, ir)?);
    losses.extend(annotations::project(ctx, ir)?);
    losses.extend(views::project(ctx, ir)?);
    let layers = &scan.metadata.layers;
    let mut by_uuid = BTreeMap::new();
    for (ordinal, layer) in layers.iter().enumerate() {
        if let Some(uuid) = layer.id.filter(|uuid| !uuid.is_nil()) {
            by_uuid
                .entry(uuid.to_string())
                .or_insert_with(Vec::new)
                .push(layer_id(ordinal));
        }
    }
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(layers.len()),
        "rhino_presentation_layers",
    )?;
    for (ordinal, layer) in layers.iter().enumerate() {
        let parent = layer.hierarchy.and_then(|hierarchy| {
            let matches = by_uuid.get(&hierarchy.parent_id.to_string())?;
            (matches.len() == 1).then(|| matches[0].clone())
        });
        if parent.is_none()
            && layer
                .hierarchy
                .is_some_and(|hierarchy| !hierarchy.parent_id.is_nil())
        {
            losses.push(
                crate::loss::RhinoLossCode::NativeRecordUnprojected.note(format!(
                    "layer {} ({:?}): parent identity is absent or ambiguous",
                    layer.index, layer.name
                )),
            );
        }
        let native_ref = ir
            .native
            .namespace("rhino")
            .and_then(|namespace| namespace.arenas().get("layers"))
            .and_then(|records| records.get(ordinal))
            .map(|record| record.id().to_owned());
        ir.model.presentation_layers.push(PresentationLayer {
            native_ref,
            id: layer_id(ordinal),
            name: layer.name.clone(),
            parent,
            description: layer.description.clone(),
            visible: Some(layer.visible),
            items: Vec::new(),
        });
    }
    for (source_order, object) in scan.objects.iter().enumerate() {
        let Some(object) = object.framed() else {
            continue;
        };
        let targets = items(ir, source_order, &object.identity.object_id.to_string());
        let candidates = layers
            .iter()
            .enumerate()
            .filter(|(_, layer)| layer.index == object.identity.layer_index)
            .map(|(ordinal, _)| ordinal)
            .collect::<Vec<_>>();
        if let [ordinal] = candidates.as_slice() {
            if let Some(layer) = ir
                .model
                .presentation_layers
                .iter_mut()
                .find(|layer| layer.id == layer_id(*ordinal))
            {
                layer.items.extend(targets.iter().cloned());
            }
        } else if !targets.is_empty() {
            losses.push(
                crate::loss::RhinoLossCode::NativeRecordUnprojected.note(format!(
                    "{}: layer index {} is absent or ambiguous",
                    object.identity.source_id, object.identity.layer_index
                )),
            );
        }
        if let Some(attributes) = object.attributes.parsed() {
            for body in &mut ir.model.bodies {
                if targets.iter().any(|target| matches!(target, PresentationItem::Body { body: id } if *id == body.id)) {
                    // Object visibility and layer visibility are independent.
                    body.visible = Some(attributes.visible);
                }
            }
        }
        let non_body_targets = targets
            .iter()
            .filter(|target| !matches!(target, PresentationItem::Body { .. }));
        if non_body_targets.clone().next().is_some() {
            if let Some(attributes) = object.attributes.parsed() {
                for (ordinal, item) in non_body_targets.clone().enumerate() {
                    let target = match item {
                        PresentationItem::Point { point } => point.to_string(),
                        PresentationItem::Curve { curve } => curve.to_string(),
                        PresentationItem::Surface { surface } => surface.to_string(),
                        PresentationItem::Tessellation { tessellation } => tessellation.clone(),
                        _ => continue,
                    };
                    ctx.charge_entities(1, "rhino_object_visibility")?;
                    ir.model
                        .view_presentations
                        .push(cadmpeg_ir::presentation::ViewPresentation {
                            id: cadmpeg_ir::presentation::PresentationId::mint(format!(
                                "rhino:model:object-view#{source_order}-{ordinal}"
                            ))
                            .map_err(CodecError::malformed)?,
                            object: Some(target),
                            order: u32::try_from(ir.model.view_presentations.len())
                                .map_err(CodecError::malformed)?,
                            expanded: None,
                            visible: Some(attributes.visible),
                            display_mode: None,
                            selection_style: None,
                            line_width: None,
                            point_size: None,
                            properties: BTreeMap::new(),
                            native_ref: None,
                        });
                }
                if matches!(
                    attributes.color_source,
                    crate::objects::ColorSource::Object | crate::objects::ColorSource::Layer
                ) {
                    if let Some(bytes) = object.identity.effective_color {
                        let key = match attributes.color_source {
                            crate::objects::ColorSource::Layer => {
                                format!("layer-{}", attributes.layer_index)
                            }
                            _ => format!("object-{source_order}"),
                        };
                        let appearance = cadmpeg_ir::ids::AppearanceId::mint(format!(
                            "rhino:model:display-color#{key}"
                        ))
                        .map_err(CodecError::malformed)?;
                        if !ir
                            .model
                            .appearances
                            .iter()
                            .any(|record| record.id == appearance)
                        {
                            ctx.charge_entities(1, "rhino_display_colors")?;
                            ir.model
                                .appearances
                                .push(cadmpeg_ir::appearance::Appearance {
                                    native_ref: None,
                                    id: appearance.clone(),
                                    name: None,
                                    asset_guid: None,
                                    library_id: None,
                                    visual_guid: None,
                                    physical_token: None,
                                    schema: None,
                                    category: None,
                                    base_color: Some(crate::wire::packed_color(bytes)),
                                    properties: BTreeMap::new(),
                                    textures: Vec::new(),
                                });
                        }
                        for (ordinal, item) in non_body_targets.enumerate() {
                            let target = match attribute_target(item) {
                                Some(AttributeTarget::Point(id)) => {
                                    cadmpeg_ir::appearance::AppearanceTarget::Point(id)
                                }
                                Some(AttributeTarget::Curve(id)) => {
                                    cadmpeg_ir::appearance::AppearanceTarget::Curve(id)
                                }
                                Some(AttributeTarget::Surface(id)) => {
                                    cadmpeg_ir::appearance::AppearanceTarget::Surface(id)
                                }
                                Some(AttributeTarget::Tessellation(id)) => {
                                    cadmpeg_ir::appearance::AppearanceTarget::Tessellation(id)
                                }
                                _ => continue,
                            };
                            ctx.charge_entities(1, "rhino_display_color_bindings")?;
                            ir.model.appearance_bindings.push(
                                cadmpeg_ir::appearance::AppearanceBinding {
                                    native_ref: None,
                                    id: cadmpeg_ir::ids::AppearanceBindingId::mint(format!(
                                    "rhino:model:display-color-binding#{source_order}-{ordinal}"
                                ))
                                    .map_err(CodecError::malformed)?,
                                    target,
                                    appearance: appearance.clone(),
                                    source_entity_id: None,
                                    object_type: None,
                                    visible: None,
                                    channels: BTreeMap::new(),
                                },
                            );
                        }
                    }
                }
            }
        }
        if !object.identity.name.trim().is_empty() {
            for (target_order, target) in targets.iter().filter_map(attribute_target).enumerate() {
                ctx.charge_entities(1, "rhino_object_names")?;
                let key = IdentityKey::try_new(format!("{source_order}-{target_order}"))
                    .map_err(CodecError::malformed)?;
                ir.model.attributes.push(SourceAttribute::new(
                    ctx,
                    AttributeId::compose(
                        &cadmpeg_ir::identity_namespace!("rhino", "model", "object-name"),
                        key,
                    ),
                    None,
                    target,
                    "name".to_owned(),
                    vec![AttributeValue::String(object.identity.name.clone())],
                )?);
            }
        }
    }
    Ok(losses)
}
