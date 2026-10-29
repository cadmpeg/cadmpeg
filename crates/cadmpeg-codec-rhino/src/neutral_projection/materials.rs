// SPDX-License-Identifier: Apache-2.0
//! Project decoded Rhino materials and their object assignments.

use super::{items, Scan};
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::ids::{AppearanceBindingId, AppearanceId, IdentityKey};
use cadmpeg_ir::presentation::PresentationItem;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

fn target(item: &PresentationItem) -> Option<AppearanceTarget> {
    match item {
        PresentationItem::Body { body } => Some(AppearanceTarget::Body(body.clone())),
        PresentationItem::Point { point } => Some(AppearanceTarget::Point(point.clone())),
        PresentationItem::Curve { curve } => Some(AppearanceTarget::Curve(curve.clone())),
        PresentationItem::Surface { surface } => Some(AppearanceTarget::Surface(surface.clone())),
        PresentationItem::Tessellation { tessellation } => {
            Some(AppearanceTarget::Tessellation(tessellation.clone()))
        }
        PresentationItem::Occurrence { occurrence } => {
            Some(AppearanceTarget::Occurrence(occurrence.clone()))
        }
        _ => None,
    }
}

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    scan: &Scan<'_>,
    materials: Vec<crate::presentation::MaterialProjection>,
    ir: &mut CadIr,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    for material in &materials {
        let _object = ctx.enter_nested("copy native field object")?;
        let _textures = ctx.enter_nested("copy native field sequence")?;
        for _ in &material.texture_source_uuids {
            let _texture = ctx.enter_nested("copy native field object")?;
        }
    }
    let mut by_index = BTreeMap::new();
    for (ordinal, material) in materials.into_iter().enumerate() {
        let native_id = material.native_id;
        for (texture_order, source_uuid) in material.texture_source_uuids.iter().enumerate() {
            losses.push(crate::loss::RhinoLossCode::NativeRecordUnprojected.note(format!("{native_id} texture {texture_order} ({:?}): the full UVW transform and texture interpretation are retained natively without a neutral texture mapping", source_uuid.as_deref())));
        }
        let id = AppearanceId::compose(
            &cadmpeg_ir::identity_namespace!("rhino", "model", "material"),
            IdentityKey::zero_padded(cadmpeg_core::decode::u64_from_index(ordinal), 6),
        );
        if let Some(index) = material.archive_index {
            by_index
                .entry(i64::from(index))
                .or_insert_with(Vec::new)
                .push(id.clone());
        }
        let base_color = Some(crate::wire::packed_color(material.diffuse));
        let mut properties = BTreeMap::new();
        for (name, value) in material.properties {
            if let Some(value) = value {
                let name = cadmpeg_core::text::NonBlankString::new(name).ok_or_else(|| {
                    CodecError::malformed("Rhino material property name is empty")
                })?;
                properties.insert(name, value);
            }
        }
        ctx.charge_entities(1, "rhino_materials")?;
        ir.model.appearances.push(Appearance {
            native_ref: Some(native_id.clone()),
            id: id.clone(),
            name: (!material.name.trim().is_empty()).then_some(material.name),
            asset_guid: material.source_uuid,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: Some("Rhino material".to_owned()),
            category: None,
            base_color,
            properties,
            textures: Vec::new(),
        });
    }
    for (source_order, object) in scan.objects.iter().enumerate() {
        let Some(object) = object.framed() else {
            continue;
        };
        let Some(attributes) = object.attributes.parsed() else {
            continue;
        };
        let index = match attributes.material_source {
            0 => {
                let layers = scan
                    .metadata
                    .layers
                    .iter()
                    .filter(|layer| layer.index == attributes.layer_index)
                    .collect::<Vec<_>>();
                let [layer] = layers.as_slice() else {
                    losses.push(
                        crate::loss::RhinoLossCode::NativeRecordUnprojected.note(format!(
                            "{}: material's layer index {} is absent or ambiguous",
                            object.identity.source_id, attributes.layer_index
                        )),
                    );
                    continue;
                };
                layer.render_material_index
            }
            1 => attributes.material_index,
            source => {
                losses.push(
                    crate::loss::RhinoLossCode::NativeRecordUnprojected.note(format!(
                        "{}: material source {source} has no admitted inherited neutral appearance",
                        object.identity.source_id
                    )),
                );
                continue;
            }
        };
        let Some(materials) = by_index.get(&i64::from(index)) else {
            if index >= 0 || attributes.color_source == crate::objects::ColorSource::Material {
                losses.push(
                    crate::loss::RhinoLossCode::NativeRecordUnprojected.note(format!(
                        "{}: material selector {index} has no admitted appearance",
                        object.identity.source_id
                    )),
                );
            }
            continue;
        };
        let [appearance] = materials.as_slice() else {
            losses.push(
                crate::loss::RhinoLossCode::NativeRecordUnprojected.note(format!(
                    "{}: material index {index} is ambiguous",
                    object.identity.source_id
                )),
            );
            continue;
        };
        for (target_order, target) in
            items(ir, source_order, &object.identity.object_id.to_string())
                .iter()
                .filter_map(target)
                .enumerate()
        {
            let key = IdentityKey::try_new(format!("{source_order}-{target_order}"))
                .map_err(CodecError::malformed)?;
            ctx.charge_entities(1, "rhino_material_bindings")?;
            ir.model.appearance_bindings.push(AppearanceBinding {
                native_ref: None,
                id: AppearanceBindingId::compose(
                    &cadmpeg_ir::identity_namespace!("rhino", "model", "material-binding"),
                    key,
                ),
                target,
                appearance: appearance.clone(),
                source_entity_id: None,
                object_type: None,
                visible: None,
                channels: BTreeMap::new(),
            });
        }
    }
    Ok(losses)
}
