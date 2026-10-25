// SPDX-License-Identifier: Apache-2.0
//! Current assets supply list values; bindings establish topology assignments.

use super::*;
use cadmpeg_ir::appearance::Appearance;
use cadmpeg_ir::scalar::FiniteBinary32;

fn note(losses: &mut Vec<LossNote>, appearance: &Appearance, field: &str) {
    losses.push(
        crate::loss::FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
            "appearance {} {field} has no exact FreeCAD material representation",
            appearance.id
        )),
    );
}
fn scalar(appearance: &Appearance, key: &str) -> Option<f64> {
    appearance
        .properties
        .iter()
        .find(|(name, _)| name.as_str() == key)
        .map(|(_, value)| value.get())
}
pub(super) fn rewrite(
    ir: &cadmpeg_ir::CadIr,
    property: &crate::native::GuiPropertyRecord,
    entry: &EntryRecord,
    contents: &mut EntryContents,
    losses: &mut Vec<LossNote>,
) -> Result<bool, CodecError> {
    if !ir.scope.model
        || !matches!(
            (property.name.as_str(), property.type_name.as_str()),
            ("ShapeAppearance", "App::PropertyMaterialList")
                | (
                    "DiffuseColor" | "LineColorArray" | "PointColorArray",
                    "App::PropertyColorList"
                )
        )
    {
        return Ok(true);
    }
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(true);
    };
    if !namespace
        .arena_as::<crate::native::GuiViewProviderRecord>("gui_view_providers")?
        .iter()
        .any(|provider| provider.id == property.owner && provider.object.is_some())
    {
        return Ok(true);
    }
    if claims(namespace, entry, &property.type_name)? > 1 {
        return Ok(false);
    }
    let assets = ir
        .model
        .appearances
        .iter()
        .filter(|appearance| appearance.native_ref.as_deref() == Some(entry.id()))
        .collect::<Vec<_>>();
    match contents {
        EntryContents::Color { values } => {
            *values = assets
                .into_iter()
                .filter_map(|appearance| match appearance.base_color {
                    Some(color) => Some(Some(super::super::presentation::colors::packed(color))),
                    None => {
                        note(losses, appearance, "absent base color");
                        None
                    }
                })
                .collect();
        }
        EntryContents::Material { version, values } => {
            let mut current = Vec::new();
            for appearance in assets {
                if let Some(value) = material(ir, appearance, &property.name, *version, losses)? {
                    current.push(value);
                }
            }
            *values = current;
        }
        _ => {}
    }
    Ok(true)
}

pub(in crate::writer) fn claims(
    namespace: &cadmpeg_ir::native::NativeNamespace,
    entry: &EntryRecord,
    kind: &str,
) -> Result<usize, CodecError> {
    let providers =
        namespace.arena_as::<crate::native::GuiViewProviderRecord>("gui_view_providers")?;
    Ok(namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")?
        .iter()
        .filter(|property| {
            property.type_name == kind
                && providers
                    .iter()
                    .any(|provider| provider.id == property.owner && provider.object.is_some())
                && property.values.iter().any(|value| {
                    value
                        .attributes
                        .get("file")
                        .is_some_and(|name| name == entry.name())
                })
        })
        .count())
}

pub(in crate::writer) fn needs_strings(
    ir: &cadmpeg_ir::CadIr,
    appearance: &Appearance,
) -> Result<bool, CodecError> {
    if appearance.asset_guid.is_some() {
        return Ok(true);
    }
    for attribute in &ir.model.attributes {
        if matches!(&attribute.target,cadmpeg_ir::attributes::AttributeTarget::Record { arena,id } if arena.as_str()=="appearances" && id==appearance.id.as_str()) && owns_attribute(ir,attribute)?
            && attribute.values.iter().any(|value|matches!(value,cadmpeg_ir::attributes::AttributeValue::Map(values) if values.iter().any(|value|matches!(value.key.as_str(),"image"|"image_path"|"uuid")))) { return Ok(true); }
    }
    Ok(false)
}
pub(in crate::writer) fn normalize_legacy_strings(
    ir: &cadmpeg_ir::CadIr,
    appearance: &Appearance,
    slot: &mut crate::native::entry_contents::MaterialSlot,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut added = Vec::new();
    for (key, value) in [
        ("image", &mut slot.image),
        ("image_path", &mut slot.image_path),
        ("uuid", &mut slot.uuid),
    ] {
        if value.is_none() {
            *value = Some(String::new());
            added.push(key);
        }
    }
    if !added.is_empty() {
        let mut attributes = Vec::new();
        for attribute in &ir.model.attributes {
            if matches!(&attribute.target,cadmpeg_ir::attributes::AttributeTarget::Record { arena,id } if arena.as_str()=="appearances" && id==appearance.id.as_str())
                && owns_attribute(ir, attribute)?
            {
                attributes.push(attribute.id.as_str());
            }
        }
        losses.push(crate::loss::FreecadLossCode::ExportPresentationNotTransferred.note(format!("appearance {} material attributes {attributes:?} promote legacy known absence to version3 empty string keys {added:?}; decoded target Maps may contain these additional empty keys",appearance.id)));
    }
    Ok(())
}

pub(in crate::writer) fn material(
    ir: &cadmpeg_ir::CadIr,
    appearance: &Appearance,
    name: &str,
    version: u32,
    losses: &mut Vec<LossNote>,
) -> Result<Option<crate::native::entry_contents::MaterialSlot>, CodecError> {
    use crate::native::entry_contents::{BinaryMaterial, MaterialSlot};
    let packed =
        |key: &str| scalar(appearance, key).and_then(|value| value.to_string().parse::<u32>().ok());
    let number = |key: &str| {
        scalar(appearance, key)
            .and_then(|value| value.to_string().parse::<f32>().ok())
            .and_then(FiniteBinary32::new)
    };
    let (
        Some(ambient),
        Some(diffuse),
        Some(specular),
        Some(emissive),
        Some(shininess),
        Some(transparency),
    ) = (
        packed("ambient_packed"),
        packed("diffuse_packed"),
        packed("specular_packed"),
        packed("emissive_packed"),
        number("shininess"),
        number("transparency"),
    )
    else {
        note(losses, appearance, "missing material coefficient");
        return Ok(None);
    };
    let mut material = BinaryMaterial {
        ambient,
        diffuse,
        specular,
        emissive,
        shininess,
        transparency,
    };
    if let Some(color) = appearance.base_color {
        material.diffuse =
            (super::super::presentation::colors::packed(color) & !0xff) | (material.diffuse & 0xff);
    } else if (0.0..=1.0).contains(&transparency.get()) {
        note(losses, appearance, "absent base color");
        return Ok(None);
    }
    let mut image = None;
    let mut image_path = None;
    let mut uuid = None;
    for attribute in &ir.model.attributes {
        if !matches!(&attribute.target,cadmpeg_ir::attributes::AttributeTarget::Record{arena,id} if arena.as_str()=="appearances" && id==appearance.id.as_str())
            || !owns_attribute(ir, attribute)?
        {
            continue;
        }
        if attribute.name != name {
            losses.push(crate::loss::FreecadLossCode::ExportAppearanceNotTransferred.note(format!("attribute {} name {} cannot rename functional material property {name}; current material strings are transferred",attribute.id,attribute.name)));
        }
        for value in &attribute.values {
            if let cadmpeg_ir::attributes::AttributeValue::Map(values) = value {
                for value in values {
                    match value.key.as_str() {
                        "image" | "image_path" | "uuid" if value.value.contains('\0') => {
                            losses.push(crate::loss::FreecadLossCode::ExportAppearanceNotTransferred.note(format!("attribute {} material Map key {} contains NUL and its material is omitted", attribute.id,value.key)));
                            return Ok(None);
                        }
                        "image" => image = Some(value.value.clone()),
                        "image_path" => image_path = Some(value.value.clone()),
                        "uuid" => uuid = Some(value.value.clone()),
                        _ => losses.push(crate::loss::FreecadLossCode::ExportAppearanceNotTransferred.note(format!("attribute {} material Map key {} has no FreeCAD material field and is omitted", attribute.id, value.key))),
                    }
                }
            }
        }
    }
    if let Some(current) = &appearance.asset_guid {
        if current.contains('\0') {
            note(losses, appearance, "asset GUID contains NUL");
            return Ok(None);
        }
        if uuid.as_ref().is_some_and(|uuid| uuid != current) {
            note(losses, appearance, "conflicting UUID map and asset GUID");
        }
        uuid = Some(current.clone());
    }
    if version != 3 {
        image = None;
        image_path = None;
        uuid = None;
    }
    Ok(Some(MaterialSlot {
        fixed: Some(material),
        image,
        image_path,
        uuid,
    }))
}

pub(in crate::writer) fn owns_attribute(
    ir: &cadmpeg_ir::CadIr,
    attribute: &cadmpeg_ir::attributes::SourceAttribute,
) -> Result<bool, CodecError> {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
    let AttributeTarget::Record { arena, id } = &attribute.target else {
        return Ok(false);
    };
    if arena.as_str() != "appearances"
        || !matches!(attribute.values.as_slice(), [AttributeValue::Map(_)])
    {
        return Ok(false);
    }
    let Some(appearance) = ir
        .model
        .appearances
        .iter()
        .find(|appearance| appearance.id.as_str() == id)
    else {
        return Ok(false);
    };
    let Some(entry_id) = attribute
        .native_ref
        .as_deref()
        .filter(|entry| appearance.native_ref.as_deref() == Some(*entry))
    else {
        return Ok(false);
    };
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(false);
    };
    let entries = namespace.arena_as::<EntryRecord>("entries")?;
    let Some(entry) = entries.iter().find(|entry| {
        entry.id() == entry_id
            && entry
                .contents()
                .iter()
                .any(|contents| matches!(contents, EntryContents::Material { .. }))
    }) else {
        return Ok(false);
    };
    let providers =
        namespace.arena_as::<crate::native::GuiViewProviderRecord>("gui_view_providers")?;
    Ok(namespace
        .arena_as::<crate::native::GuiPropertyRecord>("gui_properties")?
        .iter()
        .any(|property| {
            property.name == "ShapeAppearance"
                && providers
                    .iter()
                    .any(|provider| provider.id == property.owner && provider.object.is_some())
                && property.type_name == "App::PropertyMaterialList"
                && property
                    .side_entries
                    .iter()
                    .any(|name| name == entry.name())
                && property.values.iter().any(|value| {
                    value.tag == "MaterialList"
                        && value
                            .attributes
                            .get("file")
                            .is_some_and(|name| name == entry.name())
                })
        }))
}

#[cfg(test)]
mod tests;
