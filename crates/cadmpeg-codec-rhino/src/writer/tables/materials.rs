// SPDX-License-Identifier: Apache-2.0
//! Material tables use current appearances; native-only optical settings survive.

use super::{
    anonymous, anonymous_with_children, class_wrapper, class_wrapper_with_direct, color, component,
    crc_chunk_with_direct, records, unit_color_channel, utf16, wire_index, CadIr, CodecError, Map,
    Value,
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
    textures: &[Value],
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
    payload.extend(texture_array(textures)?);
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
        && textures_complete(&fields)
        && fields.get("physically_based").is_none_or(Value::is_null)
}

/// One parsed `ON_Texture` channel: every field the decoder admits, in wire
/// order. The reader is the single grammar; the validator and the encoder
/// share it, so a record the validator accepts cannot fail the encoder.
struct ParsedTexture {
    id: [u8; 16],
    mapping_channel_id: u32,
    legacy_file_path: String,
    enabled: bool,
    texture_type: u32,
    mode: u32,
    minification_filter: u32,
    magnification_filter: u32,
    wrap: [u32; 3],
    uvw_transform: [f64; 16],
    border_color: [u8; 4],
    transparent_color: [u8; 4],
    transparency_texture_uuid: [u8; 16],
    bump_scale: [f64; 2],
    alpha_blend: [f64; 5],
    rgb_blend_constant: [u8; 4],
    rgb_blend: [f64; 4],
    blend_order: i32,
    file_reference: Option<ParsedFileReference>,
    treat_as_linear: Option<bool>,
}

struct ParsedFileReference {
    full_path: String,
    relative_path: String,
    referenced_byte_count: u64,
    hash_time: u64,
    content_time: u64,
    name_sha1: [u8; 20],
    content_sha1: [u8; 20],
    path_status: u32,
    embedded_file_uuid: [u8; 16],
}

pub(super) fn unhex<const N: usize>(value: &Value) -> Option<[u8; N]> {
    let text = value.as_str()?;
    if text.len() != N * 2 || !text.is_ascii() {
        return None;
    }
    let mut bytes = [0; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * index..2 * index + 2], 16).ok()?;
    }
    Some(bytes)
}

fn texture_uuid(value: Option<&Value>) -> Option<[u8; 16]> {
    let value = value?;
    if value.is_null() {
        return Some([0; 16]);
    }
    crate::presentation::parse_uuid_text(value.as_str()?).map(crate::wire::Uuid::to_wire)
}

fn texture_u32(value: Option<&Value>) -> Option<u32> {
    u32::try_from(value?.as_u64()?).ok()
}

fn texture_i32(value: Option<&Value>) -> Option<i32> {
    i32::try_from(value?.as_i64()?).ok()
}

fn texture_f64(value: Option<&Value>) -> Option<f64> {
    value?.as_f64().filter(|value| value.is_finite())
}

fn texture_u64(value: Option<&Value>) -> Option<u64> {
    value?.as_u64()
}

fn texture_floats<const N: usize>(value: Option<&Value>) -> Option<[f64; N]> {
    let values = value?.as_array()?;
    if values.len() != N {
        return None;
    }
    let mut out = [0.0; N];
    for (target, value) in out.iter_mut().zip(values) {
        *target = texture_f64(Some(value))?;
    }
    Some(out)
}

fn texture_u32s<const N: usize>(value: Option<&Value>) -> Option<[u32; N]> {
    let values = value?.as_array()?;
    if values.len() != N {
        return None;
    }
    let mut out = [0; N];
    for (target, value) in out.iter_mut().zip(values) {
        *target = texture_u32(Some(value))?;
    }
    Some(out)
}

fn texture_xform(value: Option<&Value>) -> Option<[f64; 16]> {
    let rows = value?.as_array()?;
    if rows.len() != 4 {
        return None;
    }
    let mut out = [0.0; 16];
    for (row, target) in rows.iter().zip(out.chunks_exact_mut(4)) {
        let cells = row.as_array()?;
        if cells.len() != 4 {
            return None;
        }
        for (cell, value) in cells.iter().zip(target.iter_mut()) {
            *value = texture_f64(Some(cell))?;
        }
    }
    Some(out)
}

fn read_file_reference(value: &Value) -> Option<ParsedFileReference> {
    let fields = value.as_object()?;
    let get = |name: &str| fields.get(name);
    Some(ParsedFileReference {
        full_path: get("full_path")?.as_str()?.to_owned(),
        relative_path: get("relative_path")?.as_str()?.to_owned(),
        referenced_byte_count: texture_u64(get("referenced_byte_count"))?,
        hash_time: texture_u64(get("hash_time"))?,
        content_time: texture_u64(get("content_time"))?,
        name_sha1: unhex(get("name_sha1")?)?,
        content_sha1: unhex(get("content_sha1")?)?,
        path_status: texture_u32(get("path_status"))?,
        embedded_file_uuid: texture_uuid(get("embedded_file_uuid"))?,
    })
}

fn read_texture(value: &Value) -> Option<ParsedTexture> {
    let fields = value.as_object()?;
    let get = |name: &str| fields.get(name);
    let file_reference = match get("file_reference") {
        None | Some(Value::Null) => None,
        Some(reference) => Some(read_file_reference(reference)?),
    };
    let treat_as_linear = match get("treat_as_linear") {
        None | Some(Value::Null) => None,
        Some(flag) => Some(flag.as_bool()?),
    };
    Some(ParsedTexture {
        id: texture_uuid(get("source_uuid"))?,
        mapping_channel_id: texture_u32(get("mapping_channel_id"))?,
        legacy_file_path: get("legacy_file_path")?.as_str()?.to_owned(),
        enabled: get("enabled")?.as_bool()?,
        texture_type: texture_u32(get("texture_type"))?,
        mode: texture_u32(get("mode"))?,
        minification_filter: texture_u32(get("minification_filter"))?,
        magnification_filter: texture_u32(get("magnification_filter"))?,
        wrap: texture_u32s(get("wrap"))?,
        uvw_transform: texture_xform(get("uvw_transform"))?,
        border_color: unhex(get("border_color")?)?,
        transparent_color: unhex(get("transparent_color")?)?,
        transparency_texture_uuid: texture_uuid(get("transparency_texture_uuid"))?,
        bump_scale: texture_floats(get("bump_scale"))?,
        alpha_blend: texture_floats(get("alpha_blend"))?,
        rgb_blend_constant: unhex(get("rgb_blend_constant")?)?,
        rgb_blend: texture_floats(get("rgb_blend"))?,
        blend_order: texture_i32(get("blend_order"))?,
        file_reference,
        treat_as_linear,
    })
}

fn texture_minor(texture: &ParsedTexture) -> i32 {
    if texture.treat_as_linear.is_some() {
        2
    } else if texture.file_reference.is_some() {
        1
    } else {
        0
    }
}

fn encode_sha1(digest: &[u8; 20]) -> Result<Vec<u8>, CodecError> {
    anonymous(1, 0, digest)
}

fn encode_file_reference(reference: &ParsedFileReference) -> Result<Vec<u8>, CodecError> {
    let name_hash = encode_sha1(&reference.name_sha1)?;
    let content_hash = encode_sha1(&reference.content_sha1)?;
    let mut hash_body = reference.referenced_byte_count.to_le_bytes().to_vec();
    hash_body.extend(reference.hash_time.to_le_bytes());
    hash_body.extend(reference.content_time.to_le_bytes());
    let first = hash_body.len();
    hash_body.extend(name_hash);
    let first = first..hash_body.len();
    let second = hash_body.len();
    hash_body.extend(content_hash);
    let second = second..hash_body.len();
    let hash = anonymous_with_children(1, 0, &hash_body, &[first, second])?;
    let mut body = utf16(&reference.full_path)?;
    body.extend(utf16(&reference.relative_path)?);
    let start = body.len();
    body.extend(hash);
    let child = start..body.len();
    body.extend(reference.path_status.to_le_bytes());
    let minor = u8::from(reference.embedded_file_uuid != [0; 16]);
    if minor != 0 {
        body.extend(reference.embedded_file_uuid);
    }
    anonymous_with_children(1, i32::from(minor), &body, &[child])
}

fn encode_texture(texture: &ParsedTexture) -> Result<Vec<u8>, CodecError> {
    let mut fields = texture.id.to_vec();
    fields.extend(texture.mapping_channel_id.to_le_bytes());
    fields.extend(utf16(&texture.legacy_file_path)?);
    fields.push(u8::from(texture.enabled));
    fields.extend(texture.texture_type.to_le_bytes());
    fields.extend(texture.mode.to_le_bytes());
    fields.extend(texture.minification_filter.to_le_bytes());
    fields.extend(texture.magnification_filter.to_le_bytes());
    for cell in texture.wrap {
        fields.extend(cell.to_le_bytes());
    }
    for cell in texture.uvw_transform {
        fields.extend(cell.to_le_bytes());
    }
    fields.extend(texture.border_color);
    fields.extend(texture.transparent_color);
    fields.extend(texture.transparency_texture_uuid);
    for cell in texture.bump_scale {
        fields.extend(cell.to_le_bytes());
    }
    for cell in texture.alpha_blend {
        fields.extend(cell.to_le_bytes());
    }
    fields.extend(texture.rgb_blend_constant);
    for cell in texture.rgb_blend {
        fields.extend(cell.to_le_bytes());
    }
    fields.extend(texture.blend_order.to_le_bytes());
    if let Some(reference) = &texture.file_reference {
        fields.extend(encode_file_reference(reference)?);
    }
    if let Some(linear) = texture.treat_as_linear {
        fields.push(u8::from(linear));
    }
    anonymous(1, texture_minor(texture), &fields)
}

pub(in crate::writer) fn textures_complete(fields: &Map<String, Value>) -> bool {
    let Some(textures) = fields.get("textures").and_then(Value::as_array) else {
        return false;
    };
    fields.get("texture_count").and_then(Value::as_u64) == u64::try_from(textures.len()).ok()
        && textures
            .iter()
            .all(|texture| read_texture(texture).is_some())
}

fn texture_array(textures: &[Value]) -> Result<Vec<u8>, CodecError> {
    let count = i32::try_from(textures.len())
        .map_err(|_| CodecError::malformed("Rhino material texture count exceeds i32"))?;
    let mut body = count.to_le_bytes().to_vec();
    let mut children = Vec::new();
    for texture in textures {
        let parsed = read_texture(texture)
            .ok_or_else(|| CodecError::malformed("Rhino material texture fields are incomplete"))?;
        let start = body.len();
        body.extend(class_wrapper(
            crate::wire::Uuid::to_wire(crate::presentation::TEXTURE),
            &encode_texture(&parsed)?,
        )?);
        children.push(start..body.len());
    }
    anonymous_with_children(1, 0, &body, &children)
}

#[cfg(test)]
mod tests;
