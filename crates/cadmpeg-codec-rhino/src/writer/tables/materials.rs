// SPDX-License-Identifier: Apache-2.0
//! Material tables use current appearances; native-only optical settings survive.

use super::*;
use cadmpeg_ir::appearance::Appearance;

const MATERIAL_CLASS: [u8; 16] = [
    0xbc, 0xdb, 0xb5, 0x60, 0x60, 0xe6, 0xd3, 0x11, 0xbf, 0xe4, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];

fn own_material(ir: &CadIr, id: &str) -> Option<Map<String, Value>> {
    records(ir, "materials").iter().find_map(|record| {
        let fields = record.fields().ok()?;
        (fields.get("neutral_ref").and_then(Value::as_str) == Some(id)).then_some(fields)
    })
}

fn scalar(appearance: &Appearance, name: &str, default: f64) -> f64 {
    appearance
        .properties
        .iter()
        .find_map(|(key, value)| (key.as_str() == name).then_some(value.get()))
        .unwrap_or(default)
}

pub(in crate::writer) fn materials(ir: &CadIr) -> Result<Vec<Vec<u8>>, CodecError> {
    let mut output = Vec::new();
    for (index, appearance) in ir.model.appearances.iter().enumerate() {
        let native = own_material(ir, appearance.id.as_str());
        let fields = native.as_ref();
        let index = wire_index(index)?;
        let mut payload = component(
            appearance.id.as_str(),
            index,
            appearance.name.as_deref().unwrap_or(""),
            fields.and_then(|fields| fields.get("source_uuid")),
        )?;
        let component_end = payload.len();
        payload.extend(
            fields
                .and_then(|fields| fields.get("plugin_uuid"))
                .and_then(Value::as_str)
                .and_then(crate::presentation::parse_uuid_text)
                .map_or([0; 16], crate::wire::Uuid::to_wire),
        );
        for name in [
            "ambient",
            "diffuse",
            "emission",
            "specular",
            "reflection",
            "transparent",
        ] {
            let packed = if name == "diffuse" {
                if let Some(color) = appearance.base_color {
                    [
                        unit_color_channel(color.r())?,
                        unit_color_channel(color.g())?,
                        unit_color_channel(color.b())?,
                        unit_color_channel(1.0 - color.a())?,
                    ]
                } else {
                    [0, 0, 0, 0]
                }
            } else {
                color(fields, name)?
            };
            payload.extend(packed);
        }
        for (name, default) in [
            ("index_of_refraction", 1.0),
            ("reflectivity", 0.0),
            ("shine", 0.0),
            ("transparency", 0.0),
        ] {
            payload.extend(scalar(appearance, name, default).to_le_bytes());
        }
        let texture_start = payload.len();
        payload.extend(anonymous(1, 0, &0i32.to_le_bytes())?);
        let texture_end = payload.len();
        payload.extend(0i32.to_le_bytes());
        for name in ["shareable", "disable_lighting", "fresnel_reflections"] {
            payload.push(u8::from(
                fields
                    .and_then(|fields| fields.get(name))
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ));
        }
        for (name, default) in [
            ("reflection_glossiness", 0.0),
            ("refraction_glossiness", 0.0),
            ("fresnel_index_of_refraction", 1.0),
        ] {
            payload.extend(scalar(appearance, name, default).to_le_bytes());
        }
        payload.extend(
            fields
                .and_then(|fields| fields.get("rdk_instance_uuid"))
                .and_then(Value::as_str)
                .and_then(crate::presentation::parse_uuid_text)
                .map_or([0; 16], crate::wire::Uuid::to_wire),
        );
        payload.push(u8::from(
            fields
                .and_then(|fields| fields.get("diffuse_texture_alpha_transparency"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ));
        output.push(crc_chunk_with_direct(
            0x2000_8040,
            &class_wrapper_with_direct(
                MATERIAL_CLASS,
                &anonymous_with_children(
                    1,
                    0,
                    &payload,
                    &[0..component_end, texture_start..texture_end],
                )?,
                &[],
            )?,
            &[],
        )?);
    }
    Ok(output)
}

pub(in crate::writer) fn material_index(ir: &CadIr, identity: &str) -> Option<i32> {
    let binding = ir.model.appearance_bindings.iter().find(|binding| {
        use cadmpeg_ir::appearance::AppearanceTarget;
        let target = match &binding.target {
            AppearanceTarget::Body(id) => id.as_str(),
            AppearanceTarget::Face(id) => id.as_str(),
            AppearanceTarget::Edge(id) => id.as_str(),
            AppearanceTarget::Vertex(id) => id.as_str(),
            AppearanceTarget::Point(id) => id.as_str(),
            AppearanceTarget::Curve(id) => id.as_str(),
            AppearanceTarget::Surface(id) => id.as_str(),
            AppearanceTarget::Occurrence(id) => id.as_str(),
            AppearanceTarget::Tessellation(id) => id.as_str(),
            AppearanceTarget::Source { source_id } => source_id.as_str(),
        };
        target == identity
    })?;
    ir.model
        .appearances
        .iter()
        .position(|appearance| appearance.id == binding.appearance)
        .and_then(|index| i32::try_from(index).ok())
}

pub(in crate::writer) fn native_complete(ir: &CadIr, record: &cadmpeg_ir::NativeRecord) -> bool {
    let Ok(fields) = record.fields() else {
        return false;
    };
    let Some(id) = fields.get("neutral_ref").and_then(Value::as_str) else {
        return false;
    };
    ir.model
        .appearances
        .iter()
        .any(|appearance| appearance.id.as_str() == id && appearance.textures.is_empty())
        && fields
            .get("textures")
            .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty))
        && fields.get("physically_based").is_none_or(Value::is_null)
}

#[cfg(test)]
mod tests;
