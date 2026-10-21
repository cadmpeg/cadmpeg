// SPDX-License-Identifier: Apache-2.0
//! Neutral color/visibility overrides and regenerated topology color lists.

use super::{native_property, Provider};
use crate::loss::FreecadLossCode;
use crate::writer::xml;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::Color;
use std::collections::{BTreeMap, HashMap, HashSet};

fn packed(color: Color) -> u32 {
    let byte = |value: f32| -> u32 {
        u32::from(
            (0_u8..=255)
                .min_by(|left, right| {
                    (f32::from(*left) / 255.0 - value)
                        .abs()
                        .total_cmp(&(f32::from(*right) / 255.0 - value).abs())
                })
                .unwrap_or(0),
        )
    };
    byte(color.r()) << 24 | byte(color.g()) << 16 | byte(color.b()) << 8 | byte(color.a())
}

fn object_color(provider: &mut Provider, color: Color) -> Result<(), CodecError> {
    provider.set(
        "ShapeColor",
        "App::PropertyColor",
        "PropertyColor",
        packed(color),
    );
    provider.set(
        "Transparency",
        "App::PropertyPercent",
        "Integer",
        ((1.0 - color.a()) * 100.0).round(),
    );
    Ok(())
}

// App::Material's native constructor defaults (the required fields are
// present on every PropertyMaterial carrier).
const DEFAULT_MATERIAL_SHININESS: f64 = 0.9;
const DEFAULT_MATERIAL_TRANSPARENCY: f64 = 0.0;

fn material(
    provider: &mut Provider,
    appearance: &cadmpeg_ir::appearance::Appearance,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if appearance.base_color.is_none() {
        provider.properties.remove("ShapeMaterial");
        losses.push(FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
            "appearance {} has no diffuse color required by a native material; ShapeMaterial is omitted", appearance.id
        )));
        return Ok(());
    }
    let scalar = |name: &str| {
        appearance
            .properties
            .iter()
            .find(|(key, _)| key.as_str() == name)
            .map(|(_, value)| value.get())
    };
    let shininess = scalar("shininess");
    let transparency = scalar("material_transparency");
    if shininess.is_none() || transparency.is_none() {
        losses.push(FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
            "appearance {} uses required native material defaults for unauthored shininess/transparency", appearance.id
        )));
    }
    let shininess = shininess.unwrap_or(DEFAULT_MATERIAL_SHININESS);
    let transparency = transparency.unwrap_or(DEFAULT_MATERIAL_TRANSPARENCY);
    if let Some(original) = provider.native.get("ShapeMaterial") {
        let mut property = original.clone();
        for value in &mut property.values {
            if value.tag == "PropertyMaterial" {
                if let Some(color) = appearance.base_color {
                    value
                        .attributes
                        .insert("diffuseColor".into(), packed(color).to_string());
                } else {
                    value.attributes.remove("diffuseColor");
                }
                value
                    .attributes
                    .insert("shininess".into(), shininess.to_string());
                value
                    .attributes
                    .insert("transparency".into(), transparency.to_string());
            }
        }
        if let Some(value) = native_property(&property)? {
            provider.properties.insert("ShapeMaterial".into(), value);
        }
    } else {
        let mut attributes = vec![
            ("ambientColor", "0".into()),
            ("specularColor", "0".into()),
            ("emissiveColor", "0".into()),
        ];
        if let Some(color) = appearance.base_color {
            attributes.push(("diffuseColor", packed(color).to_string()));
        }
        attributes.push(("shininess", shininess.to_string()));
        attributes.push(("transparency", transparency.to_string()));
        provider.properties.insert(
            "ShapeMaterial".into(),
            xml::property(
                "ShapeMaterial",
                "App::PropertyMaterial",
                &xml::element("PropertyMaterial", &attributes),
            ),
        );
    }
    Ok(())
}

fn selection(target: &AppearanceTarget) -> Option<(&str, &'static str)> {
    match target {
        AppearanceTarget::Face(id) => Some((id.as_str(), "DiffuseColor")),
        AppearanceTarget::Edge(id) => Some((id.as_str(), "LineColorArray")),
        AppearanceTarget::Vertex(id) => Some((id.as_str(), "PointColorArray")),
        _ => None,
    }
}

fn target_name<'a>(
    target: &'a AppearanceTarget,
    names: &'a HashMap<String, String>,
    body_names: &'a HashMap<String, String>,
) -> Option<&'a String> {
    match target {
        AppearanceTarget::Body(body) => body_names.get(body.as_str()),
        AppearanceTarget::Source { source_id } => names.get(source_id),
        AppearanceTarget::Occurrence(id) => names.get(id.as_str()),
        _ => selection(target).and_then(|(id, _)| names.get(id)),
    }
}

pub(super) fn write(
    ir: &CadIr,
    names: &HashMap<String, String>,
    body_names: &HashMap<String, String>,
    providers: &mut BTreeMap<String, Provider>,
    entries: &mut BTreeMap<String, Vec<u8>>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    // The neutral arenas own both values and absence of their material lanes.
    // Ambient/specular/emissive members of an object material have no neutral
    // object-material form and remain typed native content.
    for provider in providers.values_mut() {
        for name in [
            "ShapeColor",
            "Transparency",
            "DiffuseColor",
            "LineColorArray",
            "PointColorArray",
            "ShapeAppearance",
        ] {
            provider.properties.remove(name);
        }
        if let Some(material) = provider.native.get_mut("ShapeMaterial") {
            for value in &mut material.values {
                if value.tag == "PropertyMaterial" {
                    for name in ["diffuseColor", "transparency", "shininess"] {
                        value.attributes.remove(name);
                    }
                }
            }
            if let Some(value) = native_property(material)? {
                provider.properties.insert("ShapeMaterial".into(), value);
            }
        }
    }
    let mut defaults = HashMap::new();
    for body in &ir.model.bodies {
        let Some(name) = body_names.get(body.id.as_str()) else {
            continue;
        };
        if body.visible.is_none() && body.color.is_none() && !providers.contains_key(name) {
            continue;
        }
        let provider = providers
            .entry(name.clone())
            .or_insert_with(|| Provider::new(name.clone()));
        provider.properties.remove("ShapeColor");
        provider.properties.remove("Transparency");
        let target = AppearanceTarget::Body(body.id.clone());
        if let Some(visible) = ir.effective_visibility(&target) {
            provider.visible(visible);
        }
        if let Some(color) = ir.effective_display_color(&target) {
            object_color(provider, color)?;
            defaults.insert(name.clone(), packed(color));
        }
    }
    let mut arrays = BTreeMap::<(String, &str), BTreeMap<usize, u32>>::new();
    let mut bound = HashSet::new();
    for binding in &ir.model.appearance_bindings {
        let appearance = ir
            .model
            .appearances
            .iter()
            .find(|appearance| appearance.id == binding.appearance)
            .ok_or_else(|| CodecError::malformed("appearance binding references absent asset"))?;
        let Some(name) = target_name(&binding.target, names, body_names) else {
            losses.push(
                FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
                    "appearance binding {} has no writable presentation target",
                    binding.id
                )),
            );
            continue;
        };
        let provider = providers
            .entry(name.clone())
            .or_insert_with(|| Provider::new(name.clone()));
        bound.insert(appearance.id.as_str());
        if let Some(visible) = ir.effective_visibility(&binding.target) {
            provider.visible(visible);
        }
        if let Some(color) = ir.effective_display_color(&binding.target) {
            if let Some((id, property)) = selection(&binding.target) {
                let index = names
                    .get(&format!("{id}:subelement"))
                    .and_then(|value| {
                        value.strip_prefix(if property == "DiffuseColor" {
                            "Face"
                        } else if property == "LineColorArray" {
                            "Edge"
                        } else {
                            "Vertex"
                        })
                    })
                    .and_then(|value| value.parse::<usize>().ok())
                    .filter(|value| *value > 0);
                if let Some(index) = index {
                    arrays
                        .entry((name.clone(), property))
                        .or_default()
                        .insert(index, packed(color));
                } else {
                    losses.push(
                        FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
                            "appearance binding {} has no synthesized topology index",
                            binding.id
                        )),
                    );
                }
            } else {
                object_color(provider, color)?;
                defaults.insert(name.clone(), packed(color));
            }
        }
        material(provider, appearance, losses)?;
        if binding.visible.is_some() || !binding.channels.is_empty() {
            losses.push(FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
                "appearance binding {} independent visibility/channel gates are reduced to the effective native provider state", binding.id
            )));
        }
        for (key, value) in &appearance.properties {
            match key.as_str() {
                "line_width" => provider.set(
                    "LineWidth",
                    "App::PropertyFloatConstraint",
                    "Float",
                    value.get(),
                ),
                "point_size" => provider.set(
                    "PointSize",
                    "App::PropertyFloatConstraint",
                    "Float",
                    value.get(),
                ),
                "shininess" | "material_transparency" => {}
                _ => losses.push(
                    FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
                        "appearance {} property {key} has no FreeCAD GUI encoding",
                        appearance.id
                    )),
                ),
            }
        }
        let (expected_name, expected_schema) =
            if let Some((id, property)) = selection(&binding.target) {
                (
                    format!(
                        "{name} {} appearance",
                        names
                            .get(&format!("{id}:subelement"))
                            .map_or("", String::as_str)
                    ),
                    format!("FCStd {property}"),
                )
            } else {
                (
                    format!("{name} shape appearance"),
                    "FCStd ViewProvider ShapeMaterial".to_owned(),
                )
            };
        if appearance
            .name
            .as_ref()
            .is_some_and(|value| value != &expected_name)
            || appearance
                .schema
                .as_ref()
                .is_some_and(|value| value != &expected_schema)
            || appearance.asset_guid.is_some()
            || appearance.library_id.is_some()
            || appearance.visual_guid.is_some()
            || appearance.physical_token.is_some()
            || appearance.category.is_some()
        {
            losses.push(
                FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
                    "appearance {} catalog identity metadata has no FreeCAD GUI encoding",
                    appearance.id
                )),
            );
        }
        if !appearance.textures.is_empty() {
            losses.push(
                FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
                    "appearance {} texture assets are omitted",
                    appearance.id
                )),
            );
        }
    }
    for appearance in &ir.model.appearances {
        if !bound.contains(appearance.id.as_str()) {
            losses.push(
                FreecadLossCode::ExportAppearanceNotTransferred.note(format!(
                    "unbound appearance {} has no FreeCAD GUI owner and is omitted",
                    appearance.id
                )),
            );
        }
    }
    for ((owner, property), overrides) in arrays {
        let prefix = if property == "DiffuseColor" {
            "Face"
        } else if property == "LineColorArray" {
            "Edge"
        } else {
            "Vertex"
        };
        let count = names
            .iter()
            .filter(|(id, _)| id.ends_with(":subelement"))
            .filter(|(id, _)| names.get(id.trim_end_matches(":subelement")) == Some(&owner))
            .filter_map(|(_, value)| value.strip_prefix(prefix)?.parse::<usize>().ok())
            .max()
            .unwrap_or(0);
        let count = u32::try_from(count)
            .map_err(|_| CodecError::malformed("GUI topology color count exceeds u32"))?;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&count.to_le_bytes());
        for index in 1..=count {
            bytes.extend_from_slice(
                &overrides
                    .get(&usize::try_from(index).map_err(CodecError::malformed)?)
                    .copied()
                    .unwrap_or_else(|| defaults.get(&owner).copied().unwrap_or(0xcccc_ccff))
                    .to_le_bytes(),
            );
        }
        let filename = format!("{owner}.{property}");
        entries.insert(filename.clone(), bytes);
        let provider = providers
            .get_mut(&owner)
            .ok_or_else(|| CodecError::malformed("topology color provider is absent"))?;
        provider.properties.insert(
            property.into(),
            xml::property(
                property,
                "App::PropertyColorList",
                &xml::element("ColorList", &[("file", filename)]),
            ),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_material_facts_are_not_invented_as_default_values() {
        let appearance =
            serde_json::from_value(serde_json::json!({"id":"test:model:appearance#empty"}))
                .unwrap();
        let mut provider = Provider::new("Shape".into());
        let mut losses = Vec::new();
        material(&mut provider, &appearance, &mut losses).unwrap();
        assert!(!provider.properties.contains_key("ShapeMaterial"));
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code.local_code(), "appearance.not-transferred");
        assert_eq!(
            losses[0].message,
            "appearance test:model:appearance#empty has no diffuse color required by a native material; ShapeMaterial is omitted"
        );
    }
}
