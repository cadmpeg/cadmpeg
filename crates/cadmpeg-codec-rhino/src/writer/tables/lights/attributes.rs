// SPDX-License-Identifier: Apache-2.0
//! Tagged attributes for light-table records.

use cadmpeg_ir::native::bytes::NativeBytes;
use serde_json::{Map, Value};

use crate::writer::WriteError;

fn field<'a>(fields: &'a Map<String, Value>, name: &str) -> Result<&'a Value, WriteError> {
    fields
        .get(name)
        .ok_or_else(|| WriteError::Omit(format!("light attribute {name} is missing")))
}

fn integer(fields: &Map<String, Value>, name: &str) -> Result<i32, WriteError> {
    field(fields, name)?
        .as_i64()
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| WriteError::Omit(format!("light attribute {name} must fit i32")))
}

fn byte(fields: &Map<String, Value>, name: &str) -> Result<u8, WriteError> {
    field(fields, name)?
        .as_u64()
        .and_then(|value| u8::try_from(value).ok())
        .ok_or_else(|| WriteError::Omit(format!("light attribute {name} must fit u8")))
}

fn string<'a>(fields: &'a Map<String, Value>, name: &str) -> Result<&'a str, WriteError> {
    field(fields, name)?
        .as_str()
        .filter(|value| !value.contains('\0'))
        .ok_or_else(|| WriteError::Omit(format!("light attribute {name} is not writable")))
}

fn color(fields: &Map<String, Value>, name: &str) -> Result<[u8; 4], WriteError> {
    let value: NativeBytes<[u8; 4]> = serde_json::from_value(field(fields, name)?.clone())
        .map_err(|error| WriteError::Omit(format!("light attribute {name}: {error}")))?;
    Ok(value.into_inner())
}

fn absent_or_empty(value: &Value) -> bool {
    value.is_null() || value.as_array().is_some_and(Vec::is_empty)
}

pub(super) fn encode(value: &Value) -> Result<Vec<u8>, WriteError> {
    const FIELDS: &[&str] = &[
        "source_uuid",
        "name",
        "url",
        "layer_index",
        "material_index",
        "linetype_index",
        "color",
        "visible",
        "object_mode",
        "decoration",
        "wire_density",
        "color_source",
        "linetype_source",
        "material_source",
        "plot_color_source",
        "plot_weight_source",
        "plot_color",
        "plot_weight_mm",
        "group_indexes",
        "display_materials",
        "active_space",
        "viewport_uuid",
        "display_order",
        "clipping_proof",
        "clipping_plane_uuids",
        "hatch_pattern_index",
        "section_hatch_scale",
        "section_hatch_rotation",
        "linetype_pattern_scale",
        "hatch_background",
        "hatch_boundary_visible",
        "detail_background_visible",
        "section_fill_rule",
        "clipping_plane_label_style",
        "rendering_materials",
        "rendering_mappings",
        "casts_shadows",
        "receives_shadows",
        "advanced_texture_preview",
        "user_strings",
        "attribute_user_strings",
        "custom_render_mesh",
        "mesh_modifiers",
    ];
    let fields = value
        .as_object()
        .ok_or_else(|| WriteError::Omit("light attributes must be an object".into()))?;
    if let Some(name) = fields.keys().find(|name| !FIELDS.contains(&name.as_str())) {
        return Err(WriteError::Omit(format!(
            "light attribute {name} has no encoder"
        )));
    }
    for name in ["layer_index", "material_index", "linetype_index"] {
        if integer(fields, name)? != -1 {
            return Err(WriteError::Omit(format!(
                "light attribute {name} needs table-index remapping"
            )));
        }
    }
    for name in [
        "group_indexes",
        "display_materials",
        "clipping_plane_uuids",
        "rendering_materials",
        "rendering_mappings",
        "user_strings",
        "attribute_user_strings",
    ] {
        if fields
            .get(name)
            .is_some_and(|value| !absent_or_empty(value))
        {
            return Err(WriteError::Omit(format!(
                "light attribute {name} is not writable yet"
            )));
        }
    }
    for (name, expected) in [
        ("clipping_proof", Value::Bool(false)),
        ("hatch_pattern_index", Value::from(-1)),
        ("section_hatch_scale", Value::from(1.0)),
        ("section_hatch_rotation", Value::from(0.0)),
        ("linetype_pattern_scale", Value::from(1.0)),
        ("hatch_background", Value::from("00000000")),
        ("hatch_boundary_visible", Value::Bool(false)),
        ("section_fill_rule", Value::from(0)),
        ("clipping_plane_label_style", Value::from(0)),
    ] {
        if fields.get(name).is_some_and(|value| value != &expected) {
            return Err(WriteError::Omit(format!(
                "light attribute {name} is not writable yet"
            )));
        }
    }
    for name in [
        "detail_background_visible",
        "casts_shadows",
        "receives_shadows",
        "advanced_texture_preview",
        "custom_render_mesh",
        "mesh_modifiers",
    ] {
        if fields.get(name).is_some_and(|value| !value.is_null()) {
            return Err(WriteError::Omit(format!(
                "light attribute {name} is not writable yet"
            )));
        }
    }
    let id = crate::presentation::parse_uuid_text(string(fields, "source_uuid")?)
        .ok_or_else(|| WriteError::Omit("light attribute source_uuid is invalid".into()))?;
    let mut body = vec![0x21];
    body.extend(id.to_wire());
    body.extend((-1_i32).to_le_bytes());
    for (tag, name) in [(1, "name"), (2, "url")] {
        body.push(tag);
        body.extend(super::utf16(string(fields, name)?)?);
    }
    for (tag, name) in [(3, "linetype_index"), (4, "material_index")] {
        body.push(tag);
        body.extend(integer(fields, name)?.to_le_bytes());
    }
    for (tag, name) in [(6, "color"), (7, "plot_color")] {
        body.push(tag);
        body.extend(color(fields, name)?);
    }
    let weight = field(fields, "plot_weight_mm")?
        .as_f64()
        .filter(|value| value.is_finite())
        .ok_or_else(|| WriteError::Omit("light attribute plot_weight_mm must be finite".into()))?;
    body.push(8);
    body.extend(weight.to_le_bytes());
    body.extend([9, byte(fields, "decoration")?]);
    body.push(10);
    body.extend(integer(fields, "wire_density")?.to_le_bytes());
    let visible = field(fields, "visible")?
        .as_bool()
        .ok_or_else(|| WriteError::Omit("light attribute visible must be boolean".into()))?;
    body.extend([11, u8::from(visible)]);
    for (tag, name) in [
        (12, "object_mode"),
        (13, "color_source"),
        (14, "plot_color_source"),
        (15, "plot_weight_source"),
        (16, "material_source"),
        (17, "linetype_source"),
        (19, "active_space"),
    ] {
        body.extend([tag, byte(fields, name)?]);
    }
    if let Some(value) = fields.get("viewport_uuid").filter(|value| !value.is_null()) {
        let id = value
            .as_str()
            .and_then(crate::presentation::parse_uuid_text)
            .ok_or_else(|| WriteError::Omit("light attribute viewport_uuid is invalid".into()))?;
        body.push(20);
        body.extend(id.to_wire());
    }
    body.push(22);
    body.extend(integer(fields, "display_order")?.to_le_bytes());
    body.push(0);
    Ok(body)
}
