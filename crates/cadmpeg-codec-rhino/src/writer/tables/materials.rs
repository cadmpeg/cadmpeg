// SPDX-License-Identifier: Apache-2.0
//! Material tables use current appearances; native-only optical settings survive.

use super::{
    anonymous, anonymous_with_children, class_wrapper_with_direct, color, component,
    crc_chunk_with_direct, records, unit_color_channel, wire_index, CadIr, CodecError, Map, Value,
};
use cadmpeg_ir::appearance::Appearance;
mod plan;
pub(in crate::writer) use plan::MaterialPlan;

const MATERIAL_CLASS: [u8; 16] = [
    0xbc, 0xdb, 0xb5, 0x60, 0x60, 0xe6, 0xd3, 0x11, 0xbf, 0xe4, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
];

fn own_material<'a>(
    ir: &'a CadIr,
    appearance: &Appearance,
) -> Option<&'a cadmpeg_ir::NativeRecord> {
    let id = appearance.native_ref.as_deref()?;
    records(ir, "materials")
        .iter()
        .find(|record| record.id() == id)
}

fn scalar(appearance: &Appearance, name: &str, default: f64) -> f64 {
    appearance
        .properties
        .iter()
        .find_map(|(key, value)| (key.as_str() == name).then_some(value.get()))
        .unwrap_or(default)
}

fn encode_material(
    identity: &str,
    index: i32,
    appearance: Option<&Appearance>,
    fields: Option<&Map<String, Value>>,
    authored_uuid: Option<[u8; 16]>,
) -> Result<Vec<u8>, CodecError> {
    let nil = Value::String("00000000-0000-0000-0000-000000000000".to_owned());
    let authored =
        authored_uuid.map(|uuid| Value::String(crate::wire::Uuid::from_wire(uuid).to_string()));
    let source_uuid = authored
        .as_ref()
        .or_else(|| fields.and_then(|fields| fields.get("source_uuid")));
    let source_uuid = if appearance.is_none() && source_uuid.is_some_and(Value::is_null) {
        Some(&nil)
    } else {
        source_uuid
    };
    let mut payload = component(
        identity,
        index,
        match appearance {
            Some(appearance) => appearance.name.as_deref().unwrap_or(""),
            None => fields
                .and_then(|fields| fields.get("name"))
                .and_then(Value::as_str)
                .unwrap_or(""),
        },
        source_uuid,
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
            if let Some(color) = appearance.and_then(|value| value.base_color) {
                [
                    unit_color_channel(color.r())?,
                    unit_color_channel(color.g())?,
                    unit_color_channel(color.b())?,
                    unit_color_channel(1.0 - color.a())?,
                ]
            } else if appearance.is_some() {
                [0, 0, 0, 0]
            } else {
                color(fields, name)?
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
        payload.extend(
            appearance
                .map_or_else(
                    || {
                        fields
                            .and_then(|fields| fields.get(name))
                            .and_then(Value::as_f64)
                            .unwrap_or(default)
                    },
                    |appearance| scalar(appearance, name, default),
                )
                .to_le_bytes(),
        );
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
        payload.extend(
            appearance
                .map_or_else(
                    || {
                        fields
                            .and_then(|fields| fields.get(name))
                            .and_then(Value::as_f64)
                            .unwrap_or(default)
                    },
                    |appearance| scalar(appearance, name, default),
                )
                .to_le_bytes(),
        );
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
    crc_chunk_with_direct(
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
    )
}

fn bound_appearance<'a>(ir: &'a CadIr, identity: &str) -> Option<&'a Appearance> {
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
            AppearanceTarget::TriangleGroup { tessellation, .. } => tessellation.as_str(),
            AppearanceTarget::Record { arena, id } if ir.model.contains_record(*arena, id) => {
                id.as_str()
            }
            AppearanceTarget::Record { .. } => return false,
            AppearanceTarget::Source { source_id } => source_id.as_str(),
        };
        target == identity
    })?;
    ir.model
        .appearances
        .iter()
        .find(|appearance| appearance.id == binding.appearance)
}

pub(in crate::writer) fn native_complete(record: &cadmpeg_ir::NativeRecord) -> bool {
    let Ok(fields) = record.fields() else {
        return false;
    };
    fields.get("name").is_some_and(Value::is_string)
        && fields
            .get("plugin_uuid")
            .and_then(Value::as_str)
            .and_then(crate::presentation::parse_uuid_text)
            .is_some()
        && [
            "ambient",
            "diffuse",
            "emission",
            "specular",
            "reflection",
            "transparent",
        ]
        .iter()
        .all(|name| {
            fields
                .get(*name)
                .is_some_and(|value| crate::wire::native_color(value).is_ok())
        })
        && [
            "index_of_refraction",
            "reflectivity",
            "shine",
            "transparency",
            "reflection_glossiness",
            "refraction_glossiness",
            "fresnel_index_of_refraction",
        ]
        .iter()
        .all(|name| {
            fields
                .get(*name)
                .and_then(Value::as_f64)
                .is_some_and(f64::is_finite)
        })
        && [
            "shareable",
            "disable_lighting",
            "fresnel_reflections",
            "diffuse_texture_alpha_transparency",
        ]
        .iter()
        .all(|name| fields.get(*name).is_some_and(Value::is_boolean))
        && fields.get("source_uuid").is_some_and(|value| {
            value.is_null()
                || value
                    .as_str()
                    .and_then(crate::presentation::parse_uuid_text)
                    .is_some_and(|uuid| !uuid.is_nil())
        })
        && fields.get("rdk_instance_uuid").is_some_and(|value| {
            value.is_null()
                || value
                    .as_str()
                    .and_then(crate::presentation::parse_uuid_text)
                    .is_some_and(|uuid| !uuid.is_nil())
        })
        && fields.get("texture_count").and_then(Value::as_u64) == Some(0)
        && fields
            .get("textures")
            .is_some_and(|value| value.as_array().is_some_and(Vec::is_empty))
        && fields.get("physically_based").is_none_or(Value::is_null)
}

#[cfg(test)]
mod tests;
