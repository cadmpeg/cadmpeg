// SPDX-License-Identifier: Apache-2.0
//! Transfer of `GuiDocument.xml` object appearance into neutral presentation records.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::AppearanceId;
use cadmpeg_ir::topology::Color;

use crate::brep::ShapePayloadRecord;
use crate::native::{ElementMapRecord, ObjectRecord, PropertyRecord};

pub(crate) fn transfer(
    ir: &mut CadIr,
    bytes: &[u8],
    entries: &BTreeMap<String, Vec<u8>>,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    element_maps: &[ElementMapRecord],
) -> Result<(), CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| CodecError::Malformed("GuiDocument.xml is not UTF-8".into()))?;
    let xml = roxmltree::Document::parse(text)
        .map_err(|error| CodecError::Malformed(format!("invalid GuiDocument.xml: {error}")))?;
    let objects_by_name = objects
        .iter()
        .map(|object| (object.name.as_str(), object.id.as_str()))
        .collect::<HashMap<_, _>>();
    let payloads_by_owner = payloads
        .iter()
        .filter_map(|payload| {
            let owner = properties
                .iter()
                .find(|property| property.id == payload.property)?
                .owner
                .as_str();
            Some((owner, payload.id.as_str()))
        })
        .collect::<Vec<_>>();
    let providers = xml
        .descendants()
        .filter(|node| node.has_tag_name("ViewProvider"))
        .collect::<Vec<_>>();
    if let Some(container) = xml
        .descendants()
        .find(|node| node.has_tag_name("ViewProviderData"))
    {
        let declared = container
            .attribute("Count")
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| CodecError::Malformed("invalid ViewProviderData Count".into()))?;
        if declared != providers.len() {
            return Err(CodecError::Malformed(format!(
                "ViewProviderData Count={declared} but {} records were found",
                providers.len()
            )));
        }
    }
    for provider in providers {
        let Some(name) = provider.attribute("name") else {
            return Err(CodecError::Malformed("ViewProvider has no name".into()));
        };
        let Some(object_id) = objects_by_name.get(name).copied() else {
            continue;
        };
        let property_nodes = provider
            .descendants()
            .filter(|node| node.has_tag_name("Property"))
            .collect::<Vec<_>>();
        let properties_node = provider
            .children()
            .find(|node| node.has_tag_name("Properties"))
            .ok_or_else(|| {
                CodecError::Malformed(format!("ViewProvider {name} has no Properties"))
            })?;
        let declared = properties_node
            .attribute("Count")
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| {
                CodecError::Malformed(format!("ViewProvider {name} has invalid property count"))
            })?;
        if declared != property_nodes.len() {
            return Err(CodecError::Malformed(format!(
                "ViewProvider {name} declares {declared} properties but contains {}",
                property_nodes.len()
            )));
        }
        let values = property_nodes
            .into_iter()
            .filter_map(|property| {
                Some((
                    property.attribute("name")?,
                    property.children().find(roxmltree::Node::is_element)?,
                ))
            })
            .collect::<HashMap<_, _>>();
        let visibility = values
            .get("Visibility")
            .and_then(|value| value.attribute("value"))
            .and_then(parse_bool);
        let transparency = values
            .get("Transparency")
            .and_then(|value| value.attribute("value"))
            .and_then(|value| value.parse::<f32>().ok())
            .map(|percent| (percent / 100.0).clamp(0.0, 1.0));
        let packed_color = values
            .get("ShapeColor")
            .and_then(|value| value.attribute("value"))
            .and_then(|value| value.parse::<u32>().ok());
        let material = values.get("ShapeMaterial");
        let body_ids = payloads_by_owner
            .iter()
            .filter(|(owner, _)| *owner == object_id)
            .flat_map(|(_, payload)| {
                ir.model
                    .bodies
                    .iter()
                    .filter(move |body| body.id.0.starts_with(&format!("{payload}:body#")))
                    .map(|body| body.id.clone())
            })
            .collect::<Vec<_>>();
        for body_id in &body_ids {
            if let Some(body) = ir.model.bodies.iter_mut().find(|body| body.id == *body_id) {
                body.visible = visibility;
                body.color = packed_color.map(|packed| decode_color(packed, transparency));
            }
        }
        if let Some(file) = values
            .get("DiffuseColor")
            .and_then(|value| value.attribute("file"))
        {
            transfer_face_colors(
                ir,
                name,
                object_id,
                file,
                entries,
                properties,
                payloads,
                element_maps,
            )?;
        }
        let Some(packed_color) = packed_color else {
            continue;
        };
        let appearance_id = AppearanceId(format!("fcstd:appearance:object#{name}"));
        let mut material_properties = BTreeMap::new();
        if let Some(material) = material {
            for (source, target) in [
                ("shininess", "shininess"),
                ("transparency", "material_transparency"),
            ] {
                if let Some(value) = material
                    .attribute(source)
                    .and_then(|value| value.parse::<f64>().ok())
                {
                    material_properties.insert(target.into(), value);
                }
            }
        }
        ir.model.appearances.push(Appearance {
            id: appearance_id.clone(),
            name: Some(format!("{name} shape appearance")),
            asset_guid: None,
            visual_guid: None,
            physical_token: None,
            schema: Some("FCStd ViewProvider ShapeMaterial".into()),
            category: None,
            base_color: Some(decode_color(packed_color, transparency)),
            properties: material_properties,
        });
        for (index, body) in body_ids.into_iter().enumerate() {
            ir.model.appearance_bindings.push(AppearanceBinding {
                id: format!("fcstd:appearance:binding#{name}:{index}"),
                target: AppearanceTarget::Body(body),
                appearance: appearance_id.clone(),
                source_entity_id: Some(object_id.to_owned()),
                object_type: Some("ViewProvider".into()),
                channels: BTreeMap::new(),
            });
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn transfer_face_colors(
    ir: &mut CadIr,
    provider_name: &str,
    object_id: &str,
    entry_name: &str,
    entries: &BTreeMap<String, Vec<u8>>,
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    element_maps: &[ElementMapRecord],
) -> Result<(), CodecError> {
    let bytes = entries.get(entry_name).ok_or_else(|| {
        CodecError::Malformed(format!(
            "DiffuseColor references missing entry {entry_name}"
        ))
    })?;
    if bytes.len() < 4 {
        return Err(CodecError::Malformed(format!(
            "DiffuseColor entry {entry_name} is truncated"
        )));
    }
    let count = u32::from_le_bytes(bytes[..4].try_into().expect("four-byte slice")) as usize;
    let expected = 4_usize
        .checked_add(count.checked_mul(4).ok_or_else(|| {
            CodecError::Malformed(format!("DiffuseColor entry {entry_name} count overflows"))
        })?)
        .ok_or_else(|| CodecError::Malformed("DiffuseColor length overflows".into()))?;
    if bytes.len() != expected {
        return Err(CodecError::Malformed(format!(
            "DiffuseColor entry {entry_name} declares {count} colors but has {} bytes",
            bytes.len()
        )));
    }
    let shape_properties = properties
        .iter()
        .filter(|property| property.owner == object_id)
        .filter(|property| {
            payloads
                .iter()
                .any(|payload| payload.property == property.id)
        })
        .map(|property| property.id.as_str())
        .collect::<Vec<_>>();
    let Some(group) = element_maps
        .iter()
        .find(|map| shape_properties.contains(&map.property.as_str()))
        .and_then(|map| map.maps.last())
        .and_then(|map| map.groups.iter().find(|group| group.indexed_name == "Face"))
    else {
        return Ok(());
    };
    if group.names.len() != count {
        return Ok(());
    }
    for (index, (bytes, names)) in bytes[4..].chunks_exact(4).zip(&group.names).enumerate() {
        let packed = u32::from_le_bytes(bytes.try_into().expect("four-byte color"));
        let appearance_id = AppearanceId(format!(
            "fcstd:appearance:face#{provider_name}:{}",
            index + 1
        ));
        ir.model.appearances.push(Appearance {
            id: appearance_id.clone(),
            name: Some(format!("{provider_name} Face{} appearance", index + 1)),
            asset_guid: None,
            visual_guid: None,
            physical_token: None,
            schema: Some("FCStd DiffuseColor".into()),
            category: None,
            base_color: Some(decode_color(packed, None)),
            properties: BTreeMap::new(),
        });
        for topology_id in names
            .iter()
            .flat_map(|name| &name.topology_ids)
            .filter(|id| ir.model.faces.iter().any(|face| face.id.0 == **id))
        {
            ir.model.appearance_bindings.push(AppearanceBinding {
                id: format!(
                    "fcstd:appearance:binding#face:{provider_name}:{}:{}",
                    index + 1,
                    topology_id
                ),
                target: AppearanceTarget::Face(cadmpeg_ir::ids::FaceId(topology_id.clone())),
                appearance: appearance_id.clone(),
                source_entity_id: Some(object_id.to_owned()),
                object_type: Some("ViewProvider Face".into()),
                channels: [("precedence".into(), "face_over_object".into())].into(),
            });
        }
    }
    Ok(())
}

fn decode_color(value: u32, transparency: Option<f32>) -> Color {
    Color {
        r: ((value >> 24) & 0xff) as f32 / 255.0,
        g: ((value >> 16) & 0xff) as f32 / 255.0,
        b: ((value >> 8) & 0xff) as f32 / 255.0,
        a: 1.0 - transparency.unwrap_or(0.0),
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}
