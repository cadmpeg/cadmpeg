// SPDX-License-Identifier: Apache-2.0
//! Rhino appearance, grouping, and lighting presentation records.

use std::collections::BTreeMap;
use std::ops::Range;

use cadmpeg_ir::document::CadIr;
use serde::Serialize;

use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::container::{Record, Scan};
use crate::objects::parse_class_wrapper;
use crate::settings::utf16;
use crate::wire::{scaled_coordinate, Uuid};

const ANONYMOUS: u32 = 0x4000_8000;
const MODEL_ATTRIBUTES: u32 = 0x4000_8002;
const MATERIAL_TABLE: u32 = 0x1000_0010;
const LIGHT_TABLE: u32 = 0x1000_0012;
const BITMAP_TABLE: u32 = 0x1000_0016;
const GROUP_TABLE: u32 = 0x1000_0018;
const FONT_TABLE: u32 = 0x1000_0019;
const DIMSTYLE_TABLE: u32 = 0x1000_0020;
const HATCH_PATTERN_TABLE: u32 = 0x1000_0022;
const LINETYPE_TABLE: u32 = 0x1000_0023;
const TEXTURE_MAPPING_TABLE: u32 = 0x1000_0025;
const MATERIAL: Uuid = Uuid::from_canonical([
    0x60, 0xb5, 0xdb, 0xbc, 0xe6, 0x60, 0x11, 0xd3, 0xbf, 0xe4, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const LIGHT: Uuid = Uuid::from_canonical([
    0x85, 0xa0, 0x85, 0x13, 0xf3, 0x83, 0x11, 0xd3, 0xbf, 0xe7, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const GROUP: Uuid = Uuid::from_canonical([
    0x72, 0x1d, 0x9f, 0x97, 0x36, 0x45, 0x44, 0xc4, 0x8b, 0xe6, 0xb2, 0xcf, 0x69, 0x7d, 0x25, 0xce,
]);
const HATCH_PATTERN: Uuid = Uuid::from_canonical([
    0x06, 0x4e, 0x7c, 0x91, 0x35, 0xf6, 0x47, 0x34, 0xa4, 0x46, 0x79, 0xff, 0x7c, 0xd6, 0x59, 0xe1,
]);
const LINETYPE: Uuid = Uuid::from_canonical([
    0x26, 0xf1, 0x0a, 0x24, 0x7d, 0x13, 0x4f, 0x05, 0x8f, 0xda, 0x8e, 0x36, 0x4d, 0xaf, 0x8e, 0xa6,
]);
const DIMSTYLE: Uuid = Uuid::from_canonical([
    0x67, 0xaa, 0x51, 0xa5, 0x79, 0x1d, 0x4b, 0xec, 0x8a, 0xed, 0xd2, 0x3b, 0x46, 0x2b, 0x6f, 0x87,
]);
const EMBEDDED_BITMAP: Uuid = Uuid::from_canonical([
    0x77, 0x2e, 0x6f, 0xc1, 0xb1, 0x7b, 0x4f, 0xc4, 0x8f, 0x54, 0x5f, 0xda, 0x51, 0x1d, 0x76, 0xd2,
]);
const WINDOWS_BITMAP: Uuid = Uuid::from_canonical([
    0x39, 0x04, 0x65, 0xeb, 0x37, 0x21, 0x11, 0xd4, 0x80, 0x0b, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const WINDOWS_BITMAP_EX: Uuid = Uuid::from_canonical([
    0x20, 0x3a, 0xfc, 0x17, 0xbc, 0xc9, 0x44, 0xfb, 0xa0, 0x7b, 0x7f, 0x5c, 0x31, 0xbd, 0x5e, 0xd9,
]);
const TEXTURE_MAPPING: Uuid = Uuid::from_canonical([
    0x32, 0xec, 0x99, 0x7a, 0xc3, 0xbf, 0x4a, 0xe5, 0xab, 0x19, 0xfd, 0x57, 0x2b, 0x8a, 0xd5, 0x54,
]);
const TEXT_STYLE: Uuid = Uuid::from_canonical([
    0x4f, 0x0f, 0x51, 0xfb, 0x35, 0xd0, 0x48, 0x65, 0x99, 0x98, 0x6d, 0x2c, 0x6a, 0x99, 0x72, 0x1d,
]);
const TEXTURE: Uuid = Uuid::from_canonical([
    0xd6, 0xff, 0x10, 0x6d, 0x32, 0x9b, 0x4f, 0x29, 0x97, 0xe2, 0xfd, 0x28, 0x2a, 0x61, 0x80, 0x20,
]);

#[derive(Debug)]
struct Component {
    index: Option<i32>,
    id: Uuid,
    name: String,
}

#[derive(Debug, Serialize)]
struct GroupRecord {
    id: String,
    source_offset: u64,
    archive_index: i32,
    source_uuid: Option<String>,
    name: String,
    links: Vec<String>,
}

#[derive(Debug, Serialize)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent material render switches"
)]
struct MaterialRecord {
    id: String,
    source_offset: u64,
    archive_index: i32,
    source_uuid: Option<String>,
    name: String,
    plugin_uuid: String,
    ambient: [u8; 4],
    diffuse: [u8; 4],
    emission: [u8; 4],
    specular: [u8; 4],
    reflection: [u8; 4],
    transparent: [u8; 4],
    index_of_refraction: f64,
    reflectivity: f64,
    shine: f64,
    transparency: f64,
    texture_count: usize,
    textures: Vec<TextureRecord>,
    shareable: bool,
    disable_lighting: bool,
    fresnel_reflections: bool,
    reflection_glossiness: Option<f64>,
    refraction_glossiness: Option<f64>,
    fresnel_index_of_refraction: Option<f64>,
    rdk_instance_uuid: Option<String>,
    diffuse_texture_alpha_transparency: Option<bool>,
}

#[derive(Debug, Serialize)]
struct TextureFileReference {
    full_path: String,
    relative_path: String,
    referenced_byte_count: u64,
    hash_time: u64,
    content_time: u64,
    name_sha1: String,
    content_sha1: String,
    path_status: u32,
    embedded_file_uuid: Option<String>,
}

#[derive(Debug, Serialize)]
struct TextureRecord {
    source_offset: u64,
    source_uuid: Option<String>,
    mapping_channel_id: u32,
    legacy_file_path: String,
    enabled: bool,
    texture_type: u32,
    mode: u32,
    minification_filter: u32,
    magnification_filter: u32,
    wrap: [u32; 3],
    uvw_transform: [[f64; 4]; 4],
    border_color: [u8; 4],
    transparent_color: [u8; 4],
    transparency_texture_uuid: Option<String>,
    bump_scale: [f64; 2],
    alpha_blend: [f64; 5],
    rgb_blend_constant: [u8; 4],
    rgb_blend: [f64; 4],
    blend_order: i32,
    file_reference: Option<TextureFileReference>,
    treat_as_linear: Option<bool>,
}

#[derive(Debug, Serialize)]
struct LightRecord {
    id: String,
    source_offset: u64,
    source_uuid: String,
    archive_index: i32,
    name: String,
    enabled: bool,
    style: i32,
    intensity: f64,
    watts: f64,
    ambient: [u8; 4],
    diffuse: [u8; 4],
    specular: [u8; 4],
    direction: [f64; 3],
    location: [f64; 3],
    spot_angle_radians: f64,
    spot_exponent: f64,
    attenuation: [f64; 3],
    shadow_intensity: f64,
    length: [f64; 3],
    width: [f64; 3],
    hotspot: f64,
    links: Vec<String>,
}

#[derive(Debug, Serialize)]
struct LinetypeSegment {
    length_millimeters: f64,
    segment_type: u32,
}

#[derive(Debug, Serialize)]
struct LinetypeRecord {
    id: String,
    source_offset: u64,
    archive_index: i32,
    source_uuid: Option<String>,
    name: String,
    segments: Vec<LinetypeSegment>,
    line_cap: u8,
    line_join: u8,
    width: f64,
    width_units: u8,
    taper_points: Vec<[f64; 2]>,
    always_model_distance: bool,
}

#[derive(Debug, Serialize)]
struct HatchLineRecord {
    angle_radians: f64,
    base_millimeters: [f64; 2],
    offset_millimeters: [f64; 2],
    dashes_millimeters: Vec<f64>,
}

#[derive(Debug, Serialize)]
struct HatchPatternRecord {
    id: String,
    source_offset: u64,
    archive_index: i32,
    source_uuid: Option<String>,
    name: String,
    fill_type: i32,
    description: String,
    lines: Vec<HatchLineRecord>,
}

#[derive(Debug, Serialize)]
struct DimensionStyleRecord {
    id: String,
    source_offset: u64,
    archive_index: i32,
    source_uuid: Option<String>,
    name: String,
    extension_line_extension_mm: f64,
    extension_line_offset_mm: f64,
    arrow_size_mm: f64,
    leader_arrow_size_mm: f64,
    center_mark_size_mm: f64,
    text_gap_mm: f64,
    text_height_mm: f64,
    text_display_mode: u32,
    angle_format: u32,
    length_format: u32,
    angle_resolution: i32,
    length_resolution: i32,
    text_style_index: i32,
    length_factor: f64,
    alternate_enabled: bool,
    alternate_length_factor: f64,
    alternate_length_format: u32,
    alternate_length_resolution: i32,
    prefix: String,
    suffix: String,
    alternate_prefix: String,
    alternate_suffix: String,
    dimension_line_extension_mm: f64,
    suppress_extension_line_1: bool,
    suppress_extension_line_2: bool,
    parent_style_uuid: Option<String>,
    controls: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Default, Serialize)]
struct FontRecord {
    characteristics: u32,
    windows_logfont_name: String,
    postscript_name: String,
    obsolete_description: String,
    windows_logfont_weight: Option<i32>,
    apple_weight_trait: Option<f64>,
    point_size: Option<f64>,
    family_name: String,
    locale_name: String,
    localized_postscript_name: String,
    english_postscript_name: String,
    localized_logfont_name: String,
    english_logfont_name: String,
    localized_family_name: String,
    english_family_name: String,
    localized_face_name: String,
    english_face_name: String,
    panose: Option<[u8; 10]>,
    quartet_member: Option<u8>,
}

#[derive(Debug, Serialize)]
struct TextStyleRecord {
    id: String,
    source_offset: u64,
    archive_index: i32,
    source_uuid: Option<String>,
    name: String,
    font_description: String,
    font: FontRecord,
}

#[derive(Debug, Serialize)]
struct EmbeddedImageRecord {
    id: String,
    source_offset: u64,
    source_uuid: Option<String>,
    name: String,
    file_path: String,
    image_crc32: u32,
    compression_method: i32,
    uncompressed_byte_len: u64,
    buffer_offset: u64,
    buffer_byte_len: u64,
    buffer_sha256: String,
}

#[derive(Debug, Serialize)]
struct WindowsBitmapRecord {
    id: String,
    source_offset: u64,
    class_uuid: String,
    file_path: String,
    header_size: i32,
    width_pixels: i32,
    height_pixels: i32,
    planes: u16,
    bits_per_pixel: u16,
    compression: i32,
    image_byte_len: i32,
    pixels_per_meter: [i32; 2],
    colors_used: i32,
    important_colors: i32,
    pixel_buffer_offset: u64,
    pixel_buffer_byte_len: u64,
    pixel_buffer_sha256: String,
}

#[derive(Debug, Serialize)]
struct TextureMappingRecord {
    id: String,
    source_offset: u64,
    source_uuid: Option<String>,
    name: String,
    mapping_type: u32,
    projection: u32,
    primitive_transform: [[f64; 4]; 4],
    uvw_transform: [[f64; 4]; 4],
    primitive_class_uuid: Option<String>,
    texture_space: u32,
    capped: bool,
}

#[derive(Debug, Serialize)]
struct RenderingMaterialReference {
    plugin_uuid: String,
    front_material_uuid: String,
    back_material_uuid: Option<String>,
    material_source: Option<u8>,
}

#[derive(Debug, Serialize)]
struct LayerPresentationRecord {
    id: String,
    source_offset: u64,
    archive_index: i32,
    source_uuid: Option<String>,
    parent_uuid: Option<String>,
    name: String,
    visible: bool,
    locked: bool,
    expanded: Option<bool>,
    color: [u8; 4],
    material_index: i32,
    linetype_index: Option<i32>,
    plot_color: Option<[u8; 4]>,
    plot_weight_mm: Option<f64>,
    display_material_uuid: Option<String>,
    clipping_planes_enabled: Option<bool>,
    rendering_materials: Vec<RenderingMaterialReference>,
}

#[derive(Debug, Serialize)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent serialized object display flags"
)]
struct ObjectPresentationRecord {
    id: String,
    source_offset: u64,
    source_uuid: String,
    name: String,
    url: String,
    layer_index: i32,
    material_index: i32,
    linetype_index: i32,
    color: [u8; 4],
    visible: bool,
    object_mode: u8,
    decoration: i32,
    wire_density: i32,
    color_source: u8,
    linetype_source: u8,
    material_source: u8,
    plot_color_source: u8,
    plot_weight_source: u8,
    plot_color: [u8; 4],
    plot_weight_mm: f64,
    group_indexes: Vec<i32>,
    display_materials: Vec<[String; 2]>,
    active_space: u8,
    viewport_uuid: Option<String>,
    display_order: i32,
    clipping_proof: bool,
    clipping_plane_uuids: Vec<String>,
    hatch_pattern_index: i32,
    section_hatch_scale: f64,
    section_hatch_rotation: f64,
    linetype_pattern_scale: f64,
    hatch_background: [u8; 4],
    hatch_boundary_visible: bool,
    section_fill_rule: u8,
    clipping_plane_label_style: u8,
    rendering_materials: Vec<RenderingMaterialReference>,
    links: Vec<String>,
}

fn structural(offset: usize, message: impl Into<String>) -> FramingError {
    FramingError::Structural {
        offset,
        message: message.into(),
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut value, byte| {
            write!(value, "{byte:02x}").expect("writing to String cannot fail");
            value
        })
}

fn uuid(reader: &mut BoundedReader<'_>) -> Result<Uuid, FramingError> {
    Ok(Uuid::from_wire(reader.array()?))
}

fn finite(reader: &BoundedReader<'_>, value: f64, label: &str) -> Result<f64, FramingError> {
    value
        .is_finite()
        .then_some(value)
        .ok_or_else(|| structural(reader.position() - 8, format!("{label} is not finite")))
}

fn read_finite(reader: &mut BoundedReader<'_>, label: &str) -> Result<f64, FramingError> {
    let value = reader.f64()?;
    finite(reader, value, label)
}

fn finite3(reader: &mut BoundedReader<'_>, label: &str) -> Result<[f64; 3], FramingError> {
    let value = [reader.f64()?, reader.f64()?, reader.f64()?];
    value
        .iter()
        .all(|value| value.is_finite())
        .then_some(value)
        .ok_or_else(|| structural(reader.position() - 24, format!("{label} is not finite")))
}

fn anonymous(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
) -> Result<(BoundedReader<'_>, (i32, i32)), FramingError> {
    let chunk = chunk_at(data, range.start, range.end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short || chunk.next_offset != range.end {
        return Err(structural(range.start, "presentation wrapper is invalid"));
    }
    let mut reader = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    let version = (reader.i32()?, reader.i32()?);
    Ok((reader, version))
}

fn component(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Component, FramingError> {
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if !matches!(chunk.typecode, MODEL_ATTRIBUTES | ANONYMOUS) || chunk.short {
        return Err(structural(
            reader.position(),
            "model-component attributes are missing",
        ));
    }
    let mut value = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    if (value.i32()?, value.i32()?) != (1, 0) {
        return Err(structural(
            value.position(),
            "model-component version is unsupported",
        ));
    }
    if chunk.typecode == ANONYMOUS {
        let bits = value.u32()?;
        if bits & !0x1f != 0 {
            return Err(structural(
                value.position() - 4,
                "model-component bits are invalid",
            ));
        }
        let id = if bits & 1 != 0 {
            uuid(&mut value)?
        } else {
            Uuid::nil()
        };
        if bits & 2 != 0 {
            value.skip(16)?;
        }
        let index = if bits & 4 != 0 {
            Some(value.i32()?)
        } else {
            None
        };
        let name = if bits & 8 != 0 {
            utf16(&mut value)?
        } else {
            String::new()
        };
        if bits & 0x10 != 0 {
            value.skip(8)?;
        }
        if value.remaining() != 0 {
            return Err(structural(
                value.position(),
                "model-component attributes have trailing bytes",
            ));
        }
        reader.skip(chunk.next_offset - reader.position())?;
        return Ok(Component { index, id, name });
    }
    match value.u8()? {
        0 | 2 => {}
        1 => value.skip(12)?,
        _ => return Err(structural(value.position() - 1, "invalid serial status")),
    }
    let id = match value.u8()? {
        0 | 2 => Uuid::nil(),
        1 => uuid(&mut value)?,
        _ => return Err(structural(value.position() - 1, "invalid UUID status")),
    };
    match value.u8()? {
        0 | 2 => {}
        1 => value.skip(4)?,
        _ => return Err(structural(value.position() - 1, "invalid type status")),
    }
    let index = match value.u8()? {
        0 | 2 => None,
        1 => Some(value.i32()?),
        _ => return Err(structural(value.position() - 1, "invalid index status")),
    };
    let name = match value.u8()? {
        0 | 2 => String::new(),
        1 => utf16(&mut value)?,
        _ => return Err(structural(value.position() - 1, "invalid name status")),
    };
    if value.remaining() != 0 {
        return Err(structural(
            value.position(),
            "model-component attributes have trailing bytes",
        ));
    }
    reader.skip(chunk.next_offset - reader.position())?;
    Ok(Component { index, id, name })
}

fn class_data(
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    expected: Uuid,
) -> Result<Range<usize>, FramingError> {
    let class = parse_class_wrapper(data, record.body.clone(), archive, &mut Vec::new())?;
    if class.class_uuid != expected {
        return Err(structural(
            record.range.start,
            "table record has the wrong class",
        ));
    }
    Ok(class.class_data_range)
}

fn parse_texture(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    source_offset: usize,
) -> Result<TextureRecord, FramingError> {
    let (mut reader, version) = anonymous(data, range, archive)?;
    if version.0 != 1 || !(0..=2).contains(&version.1) {
        return Err(structural(
            reader.position(),
            "texture version is unsupported",
        ));
    }
    let id = uuid(&mut reader)?;
    let mapping_channel_id = reader.u32()?;
    let legacy_file_path = utf16(&mut reader)?;
    let enabled = reader.bool()?;
    let texture_type = reader.u32()?;
    let mode = reader.u32()?;
    let minification_filter = reader.u32()?;
    let magnification_filter = reader.u32()?;
    let wrap = [reader.u32()?, reader.u32()?, reader.u32()?];
    let uvw_transform = xform(&mut reader)?;
    let border_color = reader.array()?;
    let transparent_color = reader.array()?;
    let transparency = uuid(&mut reader)?;
    let bump_scale = [
        read_finite(&mut reader, "bump scale minimum")?,
        read_finite(&mut reader, "bump scale maximum")?,
    ];
    let alpha_blend = [
        read_finite(&mut reader, "alpha blend constant")?,
        read_finite(&mut reader, "alpha blend coefficient")?,
        read_finite(&mut reader, "alpha blend coefficient")?,
        read_finite(&mut reader, "alpha blend coefficient")?,
        read_finite(&mut reader, "alpha blend coefficient")?,
    ];
    let rgb_blend_constant = reader.array()?;
    let rgb_blend = [
        read_finite(&mut reader, "RGB blend coefficient")?,
        read_finite(&mut reader, "RGB blend coefficient")?,
        read_finite(&mut reader, "RGB blend coefficient")?,
        read_finite(&mut reader, "RGB blend coefficient")?,
    ];
    let blend_order = reader.i32()?;
    let file_reference = if version.1 >= 1 {
        let value = crate::instances::file_reference(data, &mut reader, archive, &mut Vec::new())?;
        Some(TextureFileReference {
            full_path: value.full_path,
            relative_path: value.relative_path,
            referenced_byte_count: value.content_hash.byte_count,
            hash_time: value.content_hash.hash_time,
            content_time: value.content_hash.content_time,
            name_sha1: hex(&value.content_hash.name_sha1),
            content_sha1: hex(&value.content_hash.content_sha1),
            path_status: value.path_status,
            embedded_file_uuid: value.embedded_file_id.map(|id| id.to_string()),
        })
    } else {
        None
    };
    let treat_as_linear = (version.1 >= 2).then(|| reader.bool()).transpose()?;
    if reader.remaining() != 0 {
        return Err(structural(reader.position(), "texture has trailing bytes"));
    }
    Ok(TextureRecord {
        source_offset: source_offset as u64,
        source_uuid: (!id.is_nil()).then(|| id.to_string()),
        mapping_channel_id,
        legacy_file_path,
        enabled,
        texture_type,
        mode,
        minification_filter,
        magnification_filter,
        wrap,
        uvw_transform,
        border_color,
        transparent_color,
        transparency_texture_uuid: (!transparency.is_nil()).then(|| transparency.to_string()),
        bump_scale,
        alpha_blend,
        rgb_blend_constant,
        rgb_blend,
        blend_order,
        file_reference,
        treat_as_linear,
    })
}

fn texture_array(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Vec<TextureRecord>, FramingError> {
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(structural(
            reader.position(),
            "texture array is not anonymous",
        ));
    }
    let mut values = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    if (values.i32()?, values.i32()?) != (1, 0) {
        return Err(structural(
            values.position(),
            "texture array version is unsupported",
        ));
    }
    let count = values.i32()?;
    let count = usize::try_from(count)
        .map_err(|_| structural(values.position() - 4, "negative texture count"))?;
    if count > 1 << 16 {
        return Err(structural(
            values.position() - 4,
            "texture count exceeds limit",
        ));
    }
    let mut textures = Vec::new();
    for _ in 0..count {
        let object = chunk_at(data, values.position(), values.end(), archive, false)?;
        if object.short {
            return Err(structural(
                values.position(),
                "texture object is short-framed",
            ));
        }
        let class = parse_class_wrapper(
            data,
            object.header_start..object.next_offset,
            archive,
            &mut Vec::new(),
        )?;
        if class.class_uuid != TEXTURE {
            return Err(structural(
                values.position(),
                "texture array item has the wrong class",
            ));
        }
        textures.push(parse_texture(
            data,
            class.class_data_range,
            archive,
            object.header_start,
        )?);
        values.skip(object.next_offset - values.position())?;
    }
    if values.remaining() != 0 {
        return Err(structural(
            values.position(),
            "texture array has trailing bytes",
        ));
    }
    reader.skip(chunk.next_offset - reader.position())?;
    Ok(textures)
}

fn parse_material(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    source_offset: usize,
) -> Result<MaterialRecord, FramingError> {
    let framed = data.get(range.start).copied() == Some(0);
    let (mut reader, component, minor, modern) = if framed {
        let (mut reader, version) = anonymous(data, range, archive)?;
        if version.0 != 1 || version.1 != 0 {
            return Err(structural(
                reader.position(),
                "material version is unsupported",
            ));
        }
        let component = component(data, &mut reader, archive)?;
        (reader, component, 6, true)
    } else {
        let mut outer = BoundedReader::new(data, range.start, range.end)?;
        // The first packed byte is the fixed outer material version 2.0.
        if outer.u8()? != 0x20 {
            return Err(structural(
                range.start,
                "legacy material outer version is unsupported",
            ));
        }
        let chunk = chunk_at(data, outer.position(), outer.end(), archive, false)?;
        let mut reader = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
        let version = (reader.i32()?, reader.i32()?);
        if version.0 != 1 || !(0..=6).contains(&version.1) {
            return Err(structural(
                reader.position(),
                "legacy material version is unsupported",
            ));
        }
        let id = uuid(&mut reader)?;
        let index = reader.i32()?;
        let name = utf16(&mut reader)?;
        (
            reader,
            Component {
                index: Some(index),
                id,
                name,
            },
            version.1,
            false,
        )
    };
    let plugin = uuid(&mut reader)?;
    let ambient = reader.array()?;
    let diffuse = reader.array()?;
    let emission = reader.array()?;
    let specular = reader.array()?;
    let reflection = reader.array()?;
    let transparent = reader.array()?;
    let index_of_refraction = read_finite(&mut reader, "index of refraction")?;
    let reflectivity = read_finite(&mut reader, "reflectivity")?;
    let shine = read_finite(&mut reader, "shine")?;
    let transparency = read_finite(&mut reader, "transparency")?;
    let textures = texture_array(data, &mut reader, archive)?;
    let texture_count = textures.len();
    if !modern && minor >= 1 {
        let _obsolete_library = utf16(&mut reader)?;
    }
    if minor >= 2 || modern {
        let count = reader.i32()?;
        let bytes = crate::chunks::checked_count_bytes(
            count,
            20,
            reader.remaining(),
            1 << 16,
            reader.position(),
        )?;
        reader.skip(bytes)?;
    }
    let shareable = if minor >= 3 || modern {
        reader.bool()?
    } else {
        false
    };
    let disable_lighting = if minor >= 3 || modern {
        reader.bool()?
    } else {
        false
    };
    let fresnel_reflections = if minor >= 4 || modern {
        reader.bool()?
    } else {
        false
    };
    let reflection_glossiness = if minor >= 4 || modern {
        Some(read_finite(&mut reader, "reflection glossiness")?)
    } else {
        None
    };
    let refraction_glossiness = if minor >= 4 || modern {
        Some(read_finite(&mut reader, "refraction glossiness")?)
    } else {
        None
    };
    let fresnel_index_of_refraction = if minor >= 4 || modern {
        Some(read_finite(&mut reader, "Fresnel index")?)
    } else {
        None
    };
    let rdk = if minor >= 5 || modern {
        Some(uuid(&mut reader)?)
    } else {
        None
    };
    let alpha = if minor >= 6 || modern {
        Some(reader.bool()?)
    } else {
        None
    };
    if reader.remaining() != 0 {
        return Err(structural(reader.position(), "material has trailing bytes"));
    }
    let key = if component.id.is_nil() {
        format!("record-{source_offset}")
    } else {
        component.id.to_string()
    };
    Ok(MaterialRecord {
        id: format!("rhino:presentation:material#{key}"),
        source_offset: source_offset as u64,
        archive_index: component.index.unwrap_or(-1),
        source_uuid: (!component.id.is_nil()).then(|| component.id.to_string()),
        name: component.name,
        plugin_uuid: plugin.to_string(),
        ambient,
        diffuse,
        emission,
        specular,
        reflection,
        transparent,
        index_of_refraction,
        reflectivity,
        shine,
        transparency,
        texture_count,
        textures,
        shareable,
        disable_lighting,
        fresnel_reflections,
        reflection_glossiness,
        refraction_glossiness,
        fresnel_index_of_refraction,
        rdk_instance_uuid: rdk.filter(|id| !id.is_nil()).map(|id| id.to_string()),
        diffuse_texture_alpha_transparency: alpha,
    })
}

fn parse_group(
    data: &[u8],
    range: Range<usize>,
    source_offset: usize,
) -> Result<GroupRecord, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let packed = reader.u8()?;
    if packed >> 4 != 1 || packed & 0x0f > 1 {
        return Err(structural(range.start, "group version is unsupported"));
    }
    let index = reader.i32()?;
    let name = utf16(&mut reader)?;
    let id = if packed & 0x0f >= 1 {
        Some(uuid(&mut reader)?)
    } else {
        None
    };
    if reader.remaining() != 0 {
        return Err(structural(reader.position(), "group has trailing bytes"));
    }
    let key = id
        .filter(|id| !id.is_nil())
        .map_or_else(|| format!("index-{index}"), |id| id.to_string());
    Ok(GroupRecord {
        id: format!("rhino:presentation:group#{key}"),
        source_offset: source_offset as u64,
        archive_index: index,
        source_uuid: id.filter(|id| !id.is_nil()).map(|id| id.to_string()),
        name,
        links: Vec::new(),
    })
}

fn parse_light(
    data: &[u8],
    range: Range<usize>,
    scale: f64,
    source_offset: usize,
    link: Option<String>,
) -> Result<LightRecord, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let packed = reader.u8()?;
    if packed >> 4 != 1 || packed & 0x0f > 2 {
        return Err(structural(range.start, "light version is unsupported"));
    }
    let enabled = reader.i32()? != 0;
    let style = reader.i32()?;
    let intensity = read_finite(&mut reader, "light intensity")?;
    let watts = read_finite(&mut reader, "light watts")?;
    let ambient = reader.array()?;
    let diffuse = reader.array()?;
    let specular = reader.array()?;
    let direction = finite3(&mut reader, "light direction")?;
    let mut location = finite3(&mut reader, "light location")?;
    let spot_angle_radians = read_finite(&mut reader, "spot angle")?;
    let mut spot_exponent = read_finite(&mut reader, "spot exponent")?;
    let attenuation = finite3(&mut reader, "light attenuation")?;
    let shadow_intensity = read_finite(&mut reader, "shadow intensity")?;
    let index = reader.i32()?;
    let id = uuid(&mut reader)?;
    let name = utf16(&mut reader)?;
    let mut length = [0.0; 3];
    let mut width = [0.0; 3];
    if packed & 0x0f >= 1 {
        length = finite3(&mut reader, "light length")?;
        width = finite3(&mut reader, "light width")?;
    }
    let hotspot = if packed & 0x0f >= 2 {
        read_finite(&mut reader, "light hotspot")?
    } else {
        let value = (1.0 - spot_exponent / 128.0).clamp(0.0, 1.0);
        spot_exponent = 0.0;
        value
    };
    if reader.remaining() != 0 {
        return Err(structural(reader.position(), "light has trailing bytes"));
    }
    for vector in [&mut location, &mut length, &mut width] {
        for value in vector {
            *value = scaled_coordinate(*value, scale)
                .ok_or_else(|| structural(range.start, "scaled light geometry is invalid"))?;
        }
    }
    let key = if id.is_nil() {
        format!("record-{source_offset}")
    } else {
        id.to_string()
    };
    Ok(LightRecord {
        id: format!("rhino:presentation:light#{key}"),
        source_offset: source_offset as u64,
        source_uuid: id.to_string(),
        archive_index: index,
        name,
        enabled,
        style,
        intensity,
        watts,
        ambient,
        diffuse,
        specular,
        direction,
        location,
        spot_angle_radians,
        spot_exponent,
        attenuation,
        shadow_intensity,
        length,
        width,
        hotspot,
        links: link.into_iter().collect(),
    })
}

fn push_light(
    lights: &mut Vec<LightRecord>,
    indexes: &mut BTreeMap<String, usize>,
    mut light: LightRecord,
) {
    if light.source_uuid != Uuid::nil().to_string() {
        if let Some(index) = indexes.get(&light.source_uuid).copied() {
            lights[index].links.append(&mut light.links);
            lights[index].links.sort();
            lights[index].links.dedup();
            return;
        }
        indexes.insert(light.source_uuid.clone(), lights.len());
    }
    lights.push(light);
}

fn segments(
    reader: &mut BoundedReader<'_>,
    scale: f64,
) -> Result<Vec<LinetypeSegment>, FramingError> {
    let count = reader.i32()?;
    let bytes = crate::chunks::checked_count_bytes(
        count,
        12,
        reader.remaining(),
        1 << 16,
        reader.position(),
    )?;
    let mut values = Vec::with_capacity(bytes / 12);
    for _ in 0..bytes / 12 {
        let length = read_finite(reader, "linetype segment length")?;
        let length = scaled_coordinate(length, scale).ok_or_else(|| {
            structural(reader.position() - 8, "scaled linetype segment is invalid")
        })?;
        values.push(LinetypeSegment {
            length_millimeters: length,
            segment_type: reader.u32()?,
        });
    }
    Ok(values)
}

fn parse_linetype(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    scale: f64,
    source_offset: usize,
) -> Result<LinetypeRecord, FramingError> {
    let (mut reader, version) = anonymous(data, range, archive)?;
    let component = if version.0 == 1 && (0..=1).contains(&version.1) {
        let index = reader.i32()?;
        let name = utf16(&mut reader)?;
        let value = Component {
            index: Some(index),
            id: Uuid::nil(),
            name,
        };
        let values = segments(&mut reader, scale)?;
        let id = if version.1 >= 1 {
            uuid(&mut reader)?
        } else {
            Uuid::nil()
        };
        if reader.remaining() != 0 {
            return Err(structural(reader.position(), "linetype has trailing bytes"));
        }
        return Ok(linetype_record(
            value,
            id,
            values,
            source_offset,
            0,
            0,
            1.0,
            0,
            Vec::new(),
            false,
        ));
    } else if version.0 == 2 && (0..=3).contains(&version.1) {
        component(data, &mut reader, archive)?
    } else {
        return Err(structural(
            reader.position(),
            "linetype version is unsupported",
        ));
    };
    let values = segments(&mut reader, scale)?;
    let mut item = if version.1 >= 1 { reader.u8()? } else { 0 };
    let mut cap = 0;
    let mut join = 0;
    let mut width = 1.0;
    let mut width_units = 0;
    let mut taper = Vec::new();
    let mut always = false;
    if item == 1 {
        cap = reader.u8()?;
        item = reader.u8()?;
    }
    if item == 2 {
        join = reader.u8()?;
        item = reader.u8()?;
    }
    if version.1 >= 2 {
        if item == 3 {
            width = read_finite(&mut reader, "linetype width")?;
            item = reader.u8()?;
        }
        if item == 4 {
            width_units = reader.u8()?;
            item = reader.u8()?;
        }
        if item == 5 {
            let count = reader.i32()?;
            let bytes = crate::chunks::checked_count_bytes(
                count,
                16,
                reader.remaining(),
                1 << 16,
                reader.position(),
            )?;
            for _ in 0..bytes / 16 {
                taper.push([reader.f64()?, reader.f64()?]);
            }
            if !taper.iter().flatten().all(|value| value.is_finite()) {
                return Err(structural(
                    reader.position(),
                    "linetype taper is not finite",
                ));
            }
            item = reader.u8()?;
        }
    }
    if version.1 >= 3 && item == 6 {
        always = reader.bool()?;
        item = reader.u8()?;
    }
    if item != 0 || reader.remaining() != 0 {
        return Err(structural(
            reader.position(),
            "linetype extension stream is invalid",
        ));
    }
    let component_id = component.id;
    Ok(linetype_record(
        component,
        component_id,
        values,
        source_offset,
        cap,
        join,
        width,
        width_units,
        taper,
        always,
    ))
}

#[allow(clippy::too_many_arguments)]
fn linetype_record(
    component: Component,
    fallback_id: Uuid,
    segments: Vec<LinetypeSegment>,
    source_offset: usize,
    line_cap: u8,
    line_join: u8,
    width: f64,
    width_units: u8,
    taper_points: Vec<[f64; 2]>,
    always_model_distance: bool,
) -> LinetypeRecord {
    let id = if component.id.is_nil() {
        fallback_id
    } else {
        component.id
    };
    let key = if id.is_nil() {
        format!("record-{source_offset}")
    } else {
        id.to_string()
    };
    LinetypeRecord {
        id: format!("rhino:presentation:linetype#{key}"),
        source_offset: source_offset as u64,
        archive_index: component.index.unwrap_or(-1),
        source_uuid: (!id.is_nil()).then(|| id.to_string()),
        name: component.name,
        segments,
        line_cap,
        line_join,
        width,
        width_units,
        taper_points,
        always_model_distance,
    }
}

fn hatch_line_v5(
    reader: &mut BoundedReader<'_>,
    scale: f64,
) -> Result<HatchLineRecord, FramingError> {
    let packed = reader.u8()?;
    if packed >> 4 != 1 {
        return Err(structural(
            reader.position() - 1,
            "hatch-line version is unsupported",
        ));
    }
    hatch_line_fields(reader, scale)
}

fn hatch_line_fields(
    reader: &mut BoundedReader<'_>,
    scale: f64,
) -> Result<HatchLineRecord, FramingError> {
    let angle_radians = read_finite(reader, "hatch-line angle")?;
    let mut base = [reader.f64()?, reader.f64()?];
    let mut offset = [reader.f64()?, reader.f64()?];
    let count = reader.i32()?;
    let bytes = crate::chunks::checked_count_bytes(
        count,
        8,
        reader.remaining(),
        1 << 16,
        reader.position(),
    )?;
    let mut dashes = Vec::with_capacity(bytes / 8);
    for _ in 0..bytes / 8 {
        dashes.push(read_finite(reader, "hatch dash")?);
    }
    for value in base
        .iter_mut()
        .chain(offset.iter_mut())
        .chain(dashes.iter_mut())
    {
        *value = scaled_coordinate(*value, scale)
            .ok_or_else(|| structural(reader.position(), "scaled hatch line is invalid"))?;
    }
    Ok(HatchLineRecord {
        angle_radians,
        base_millimeters: base,
        offset_millimeters: offset,
        dashes_millimeters: dashes,
    })
}

fn parse_hatch_pattern(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    scale: f64,
    source_offset: usize,
) -> Result<HatchPatternRecord, FramingError> {
    let modern = data.get(range.start).copied() == Some(0);
    let (component, fill_type, description, lines) = if modern {
        let (mut reader, version) = anonymous(data, range, archive)?;
        if version != (1, 0) {
            return Err(structural(
                reader.position(),
                "hatch-pattern version is unsupported",
            ));
        }
        let component = component(data, &mut reader, archive)?;
        let fill_type = reader.i32()?;
        let description = utf16(&mut reader)?;
        let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
        let mut line_reader = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
        let count = line_reader.i32()?;
        let count = usize::try_from(count)
            .map_err(|_| structural(line_reader.position() - 4, "negative hatch-line count"))?;
        if count > 1 << 16 {
            return Err(structural(
                line_reader.position() - 4,
                "hatch-line count exceeds limit",
            ));
        }
        let mut lines = Vec::with_capacity(count);
        for _ in 0..count {
            let line = chunk_at(
                data,
                line_reader.position(),
                line_reader.end(),
                archive,
                false,
            )?;
            let mut payload = BoundedReader::new(data, line.body.start, line.body.end)?;
            if (payload.i32()?, payload.i32()?) != (1, 0) {
                return Err(structural(
                    payload.position(),
                    "hatch-line version is unsupported",
                ));
            }
            lines.push(hatch_line_fields(&mut payload, scale)?);
            if payload.remaining() != 0 {
                return Err(structural(
                    payload.position(),
                    "hatch line has trailing bytes",
                ));
            }
            line_reader.skip(line.next_offset - line_reader.position())?;
        }
        if line_reader.remaining() != 0 {
            return Err(structural(
                line_reader.position(),
                "hatch-line array has trailing bytes",
            ));
        }
        reader.skip(chunk.next_offset - reader.position())?;
        if reader.remaining() != 0 {
            return Err(structural(
                reader.position(),
                "hatch pattern has trailing bytes",
            ));
        }
        (component, fill_type, description, lines)
    } else {
        let mut reader = BoundedReader::new(data, range.start, range.end)?;
        let packed = reader.u8()?;
        if packed >> 4 != 1 || packed & 0x0f > 2 {
            return Err(structural(
                range.start,
                "legacy hatch-pattern version is unsupported",
            ));
        }
        let index = reader.i32()?;
        let fill_type = reader.i32()?;
        let name = utf16(&mut reader)?;
        let description = utf16(&mut reader)?;
        let count = if fill_type == 1 { reader.i32()? } else { 0 };
        let count = usize::try_from(count)
            .map_err(|_| structural(reader.position() - 4, "negative hatch-line count"))?;
        if count > 1 << 16 {
            return Err(structural(
                reader.position() - 4,
                "hatch-line count exceeds limit",
            ));
        }
        let mut lines = Vec::with_capacity(count);
        for _ in 0..count {
            lines.push(hatch_line_v5(&mut reader, scale)?);
        }
        let id = if packed & 0x0f >= 2 {
            uuid(&mut reader)?
        } else {
            Uuid::nil()
        };
        if reader.remaining() != 0 {
            return Err(structural(
                reader.position(),
                "legacy hatch pattern has trailing bytes",
            ));
        }
        (
            Component {
                index: Some(index),
                id,
                name,
            },
            fill_type,
            description,
            lines,
        )
    };
    let key = if component.id.is_nil() {
        format!("record-{source_offset}")
    } else {
        component.id.to_string()
    };
    Ok(HatchPatternRecord {
        id: format!("rhino:presentation:hatch_pattern#{key}"),
        source_offset: source_offset as u64,
        archive_index: component.index.unwrap_or(-1),
        source_uuid: (!component.id.is_nil()).then(|| component.id.to_string()),
        name: component.name,
        fill_type,
        description,
        lines,
    })
}

fn scaled_length(
    reader: &mut BoundedReader<'_>,
    scale: f64,
    label: &str,
) -> Result<f64, FramingError> {
    let value = read_finite(reader, label)?;
    scaled_coordinate(value, scale)
        .ok_or_else(|| structural(reader.position() - 8, format!("scaled {label} is invalid")))
}

fn named_child(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<serde_json::Value, FramingError> {
    let offset = reader.position();
    let chunk = chunk_at(data, offset, reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(structural(
            offset,
            "dimension-style child wrapper is invalid",
        ));
    }
    reader.skip(chunk.next_offset - offset)?;
    Ok(serde_json::json!({
        "offset": offset,
        "byte_len": chunk.next_offset - offset,
        "sha256": cadmpeg_ir::hash::sha256_hex(&data[offset..chunk.next_offset]),
    }))
}

fn dimension_style_controls(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    scale: f64,
    minor: i32,
) -> Result<BTreeMap<String, serde_json::Value>, FramingError> {
    let mut values = BTreeMap::new();
    macro_rules! put {
        ($name:literal, $value:expr) => {{
            values.insert($name.to_string(), serde_json::json!($value));
        }};
    }
    put!("legacy_override_parent_count", reader.u32()?);
    let overrides = reader.bool()?;
    put!("has_field_overrides", overrides);
    if overrides {
        let count = crate::chunks::checked_count_bytes(
            reader.i32()?,
            1,
            reader.remaining(),
            1 << 16,
            reader.position() - 4,
        )?;
        put!("field_override_bits", reader.take(count)?.to_vec());
    }
    put!("tolerance_format", reader.u32()?);
    put!("tolerance_resolution", reader.i32()?);
    put!("tolerance_upper", read_finite(reader, "upper tolerance")?);
    put!("tolerance_lower", read_finite(reader, "lower tolerance")?);
    put!(
        "tolerance_height_scale",
        read_finite(reader, "tolerance height scale")?
    );
    put!(
        "baseline_spacing_mm",
        scaled_length(reader, scale, "baseline spacing")?
    );
    put!("draw_text_mask_legacy", reader.bool()?);
    put!("mask_fill_type_legacy", reader.u32()?);
    put!("mask_color_legacy", reader.array::<4>()?);
    put!("dimension_scale", read_finite(reader, "dimension scale")?);
    put!("dimension_scale_source", reader.i32()?);
    let source = uuid(reader)?;
    put!(
        "source_dimension_style_uuid",
        (!source.is_nil()).then(|| source.to_string())
    );
    put!("color_sources", reader.array::<4>()?);
    put!(
        "colors",
        [
            reader.array::<4>()?,
            reader.array()?,
            reader.array()?,
            reader.array()?
        ]
    );
    put!("plot_color_sources", reader.array::<4>()?);
    put!(
        "plot_colors",
        [
            reader.array::<4>()?,
            reader.array()?,
            reader.array()?,
            reader.array()?
        ]
    );
    put!("plot_weight_sources", reader.array::<2>()?);
    put!(
        "extension_line_plot_weight_mm",
        read_finite(reader, "extension plot weight")?
    );
    put!(
        "dimension_line_plot_weight_mm",
        read_finite(reader, "dimension plot weight")?
    );
    put!(
        "fixed_extension_length_mm",
        scaled_length(reader, scale, "fixed extension length")?
    );
    put!("fixed_extension_length_enabled", reader.bool()?);
    put!(
        "text_rotation_radians",
        read_finite(reader, "text rotation")?
    );
    put!("alternate_tolerance_resolution", reader.i32()?);
    put!(
        "tolerance_text_height_fraction",
        read_finite(reader, "tolerance text fraction")?
    );
    put!("suppress_arrow_1", reader.bool()?);
    put!("suppress_arrow_2", reader.bool()?);
    put!("text_move_leader", reader.i32()?);
    put!("arc_length_symbol", reader.i32()?);
    put!(
        "stack_text_height_fraction",
        read_finite(reader, "stack text fraction")?
    );
    put!("stack_format", reader.u32()?);
    put!(
        "alternate_rounding",
        read_finite(reader, "alternate rounding")?
    );
    put!("rounding", read_finite(reader, "rounding")?);
    put!("angular_rounding", read_finite(reader, "angular rounding")?);
    put!("alternate_zero_suppression", reader.u32()?);
    put!("obsolete_tolerance_zero_suppression", reader.u32()?);
    put!("zero_suppression", reader.u32()?);
    put!("angular_zero_suppression", reader.u32()?);
    put!("alternate_below", reader.bool()?);
    put!("arrow_types", [reader.u32()?, reader.u32()?, reader.u32()?]);
    put!(
        "arrow_block_uuids",
        [
            uuid(reader)?.to_string(),
            uuid(reader)?.to_string(),
            uuid(reader)?.to_string()
        ]
    );
    if minor >= 1 {
        put!("obsolete_leader_content_type", reader.u32()?);
        put!("obsolete_text_vertical_alignment", reader.u32()?);
        put!("obsolete_leader_vertical_alignment", reader.u32()?);
        put!("leader_content_angle_style", reader.u32()?);
        put!("leader_curve_type", reader.u32()?);
        put!(
            "leader_content_angle_radians",
            read_finite(reader, "leader content angle")?
        );
        put!("leader_has_landing", reader.bool()?);
        put!(
            "leader_landing_length_mm",
            scaled_length(reader, scale, "leader landing length")?
        );
        put!("obsolete_text_horizontal_alignment", reader.u32()?);
        put!("obsolete_leader_horizontal_alignment", reader.u32()?);
        put!("draw_forward", reader.bool()?);
        put!("signed_ordinate", reader.bool()?);
        put!("scale_value", named_child(data, reader, archive)?);
        put!("unit_system", reader.u32()?);
    }
    if minor >= 2 {
        put!("font_characteristics", named_child(data, reader, archive)?);
    }
    if minor >= 3 {
        put!("text_mask", named_child(data, reader, archive)?);
    }
    if minor >= 4 {
        for name in [
            "dimension_text_location",
            "radial_text_location",
            "text_vertical_alignment",
            "text_horizontal_alignment",
            "leader_text_vertical_alignment",
            "leader_text_horizontal_alignment",
            "text_orientation",
            "leader_text_orientation",
            "dimension_text_orientation",
            "radial_text_orientation",
            "dimension_text_angle_style",
            "radial_text_angle_style",
        ] {
            values.insert(name.to_string(), serde_json::json!(reader.u32()?));
        }
        put!("text_underlined", reader.bool()?);
    }
    if minor >= 5 {
        put!("dimension_length_unit", reader.u32()?);
        put!("alternate_dimension_length_unit", reader.u32()?);
    }
    if minor >= 6 {
        put!("dimension_length_display", reader.u32()?);
        put!("alternate_dimension_length_display", reader.u32()?);
    }
    if minor >= 7 {
        put!("center_mark_style", reader.u32()?);
    }
    if minor >= 8 {
        put!("force_dimension_line", reader.bool()?);
        put!("text_fit", reader.u32()?);
        put!("arrow_fit", reader.u32()?);
    }
    if minor >= 9 {
        put!("decimal_separator", reader.u32()?);
    }
    if reader.remaining() != 0 {
        return Err(structural(
            reader.position(),
            "dimension style has trailing bytes",
        ));
    }
    Ok(values)
}

fn parse_dimension_style(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    scale: f64,
    source_offset: usize,
) -> Result<DimensionStyleRecord, FramingError> {
    let (mut reader, version) = anonymous(data, range, archive)?;
    if version.0 != 1 || !(0..=9).contains(&version.1) {
        return Err(structural(
            reader.position(),
            "dimension-style version is unsupported",
        ));
    }
    let component = component(data, &mut reader, archive)?;
    let extension_line_extension_mm =
        scaled_length(&mut reader, scale, "extension-line extension")?;
    let extension_line_offset_mm = scaled_length(&mut reader, scale, "extension-line offset")?;
    let arrow_size_mm = scaled_length(&mut reader, scale, "arrow size")?;
    let leader_arrow_size_mm = scaled_length(&mut reader, scale, "leader arrow size")?;
    let center_mark_size_mm = scaled_length(&mut reader, scale, "center-mark size")?;
    let text_gap_mm = scaled_length(&mut reader, scale, "text gap")?;
    let text_height_mm = scaled_length(&mut reader, scale, "text height")?;
    let text_display_mode = reader.u32()?;
    let angle_format = reader.u32()?;
    let length_format = reader.u32()?;
    let angle_resolution = reader.i32()?;
    let length_resolution = reader.i32()?;
    let text_style_index = reader.i32()?;
    let length_factor = read_finite(&mut reader, "length factor")?;
    let alternate_enabled = reader.bool()?;
    let alternate_length_factor = read_finite(&mut reader, "alternate length factor")?;
    let alternate_length_format = reader.u32()?;
    let alternate_length_resolution = reader.i32()?;
    let prefix = utf16(&mut reader)?;
    let suffix = utf16(&mut reader)?;
    let alternate_prefix = utf16(&mut reader)?;
    let alternate_suffix = utf16(&mut reader)?;
    let dimension_line_extension_mm =
        scaled_length(&mut reader, scale, "dimension-line extension")?;
    let suppress_extension_line_1 = reader.bool()?;
    let suppress_extension_line_2 = reader.bool()?;
    let parent = uuid(&mut reader)?;
    let controls = dimension_style_controls(data, &mut reader, archive, scale, version.1)?;
    let key = if component.id.is_nil() {
        format!("record-{source_offset}")
    } else {
        component.id.to_string()
    };
    Ok(DimensionStyleRecord {
        id: format!("rhino:presentation:dimension_style#{key}"),
        source_offset: source_offset as u64,
        archive_index: component.index.unwrap_or(-1),
        source_uuid: (!component.id.is_nil()).then(|| component.id.to_string()),
        name: component.name,
        extension_line_extension_mm,
        extension_line_offset_mm,
        arrow_size_mm,
        leader_arrow_size_mm,
        center_mark_size_mm,
        text_gap_mm,
        text_height_mm,
        text_display_mode,
        angle_format,
        length_format,
        angle_resolution,
        length_resolution,
        text_style_index,
        length_factor,
        alternate_enabled,
        alternate_length_factor,
        alternate_length_format,
        alternate_length_resolution,
        prefix,
        suffix,
        alternate_prefix,
        alternate_suffix,
        dimension_line_extension_mm,
        suppress_extension_line_1,
        suppress_extension_line_2,
        parent_style_uuid: (!parent.is_nil()).then(|| parent.to_string()),
        controls,
    })
}

fn xform(reader: &mut BoundedReader<'_>) -> Result<[[f64; 4]; 4], FramingError> {
    let mut rows = [[0.0; 4]; 4];
    for value in rows.iter_mut().flatten() {
        *value = reader.f64()?;
    }
    rows.iter()
        .flatten()
        .all(|value| value.is_finite())
        .then_some(rows)
        .ok_or_else(|| structural(reader.position() - 128, "texture transform is not finite"))
}

fn parse_embedded_image(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    source_offset: usize,
) -> Result<EmbeddedImageRecord, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let packed = reader.u8()?;
    if packed >> 4 != 1 || packed & 0x0f > 1 {
        return Err(structural(
            range.start,
            "embedded-image version is unsupported",
        ));
    }
    let file_path = utf16(&mut reader)?;
    let image_crc32 = reader.u32()?;
    let compression_method = reader.i32()?;
    let buffer_offset = reader.position();
    let uncompressed_byte_len = u64::from(reader.u32()?);
    if uncompressed_byte_len != 0 {
        reader.skip(4)?;
        let method = reader.u8()?;
        if method > 1 {
            return Err(structural(
                reader.position() - 1,
                "embedded-image buffer method is unsupported",
            ));
        }
        if method == 0 {
            let size = usize::try_from(uncompressed_byte_len)
                .map_err(|_| structural(buffer_offset, "image size overflow"))?;
            reader.skip(size)?;
        } else {
            let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
            if chunk.typecode != ANONYMOUS || chunk.short {
                return Err(structural(
                    reader.position(),
                    "compressed image chunk is invalid",
                ));
            }
            reader.skip(chunk.next_offset - reader.position())?;
        }
    }
    let buffer_end = reader.position();
    let source_uuid = if packed & 0x0f >= 1 {
        Some(uuid(&mut reader)?)
    } else {
        None
    };
    let name = if packed & 0x0f >= 1 {
        utf16(&mut reader)?
    } else {
        String::new()
    };
    if reader.remaining() != 0 {
        return Err(structural(
            reader.position(),
            "embedded image has trailing bytes",
        ));
    }
    let source_uuid = source_uuid.filter(|id| !id.is_nil());
    let key = source_uuid.map_or_else(|| format!("record-{source_offset}"), |id| id.to_string());
    Ok(EmbeddedImageRecord {
        id: format!("rhino:presentation:image#{key}"),
        source_offset: source_offset as u64,
        source_uuid: source_uuid.map(|id| id.to_string()),
        name,
        file_path,
        image_crc32,
        compression_method,
        uncompressed_byte_len,
        buffer_offset: buffer_offset as u64,
        buffer_byte_len: (buffer_end - buffer_offset) as u64,
        buffer_sha256: cadmpeg_ir::hash::sha256_hex(&data[buffer_offset..buffer_end]),
    })
}

fn parse_windows_bitmap(
    data: &[u8],
    range: Range<usize>,
    class_uuid: Uuid,
    source_offset: usize,
) -> Result<WindowsBitmapRecord, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let file_path = if class_uuid == WINDOWS_BITMAP_EX {
        if reader.u8()? != 0x10 {
            return Err(structural(
                reader.position() - 1,
                "Windows bitmap version is unsupported",
            ));
        }
        utf16(&mut reader)?
    } else {
        String::new()
    };
    let header_size = reader.i32()?;
    let width_pixels = reader.i32()?;
    let height_pixels = reader.i32()?;
    let planes = reader.u16()?;
    let bits_per_pixel = reader.u16()?;
    let compression = reader.i32()?;
    let image_byte_len = reader.i32()?;
    let pixels_per_meter = [reader.i32()?, reader.i32()?];
    let colors_used = reader.i32()?;
    let important_colors = reader.i32()?;
    if image_byte_len < 0 || colors_used < 0 {
        return Err(structural(
            reader.position(),
            "Windows bitmap header is invalid",
        ));
    }
    let pixel_buffer_offset = reader.position();
    let buffer = reader.take(reader.remaining())?;
    Ok(WindowsBitmapRecord {
        id: format!("rhino:presentation:windows_bitmap#offset-{source_offset}"),
        source_offset: source_offset as u64,
        class_uuid: class_uuid.to_string(),
        file_path,
        header_size,
        width_pixels,
        height_pixels,
        planes,
        bits_per_pixel,
        compression,
        image_byte_len,
        pixels_per_meter,
        colors_used,
        important_colors,
        pixel_buffer_offset: pixel_buffer_offset as u64,
        pixel_buffer_byte_len: buffer.len() as u64,
        pixel_buffer_sha256: cadmpeg_ir::hash::sha256_hex(buffer),
    })
}

fn parse_texture_mapping(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    source_offset: usize,
) -> Result<TextureMappingRecord, FramingError> {
    let (mut reader, version) = anonymous(data, range, archive)?;
    if version.0 != 1 || !(0..=1).contains(&version.1) {
        return Err(structural(
            reader.position(),
            "texture-mapping version is unsupported",
        ));
    }
    let id = uuid(&mut reader)?;
    let mapping_type = reader.u32()?;
    let projection = reader.u32()?;
    let primitive_transform = xform(&mut reader)?;
    let uvw_transform = xform(&mut reader)?;
    let name = utf16(&mut reader)?;
    let object = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    let primitive_class_uuid = if object.short {
        None
    } else {
        parse_class_wrapper(data, object.body.clone(), archive, &mut Vec::new())
            .ok()
            .map(|value| value.class_uuid.to_string())
    };
    reader.skip(object.next_offset - reader.position())?;
    let texture_space = if version.1 >= 1 { reader.u32()? } else { 0 };
    let capped = version.1 >= 1 && reader.bool()?;
    if reader.remaining() != 0 {
        return Err(structural(
            reader.position(),
            "texture mapping has trailing bytes",
        ));
    }
    let key = if id.is_nil() {
        format!("record-{source_offset}")
    } else {
        id.to_string()
    };
    Ok(TextureMappingRecord {
        id: format!("rhino:presentation:texture_mapping#{key}"),
        source_offset: source_offset as u64,
        source_uuid: (!id.is_nil()).then(|| id.to_string()),
        name,
        mapping_type,
        projection,
        primitive_transform,
        uvw_transform,
        primitive_class_uuid,
        texture_space,
        capped,
    })
}

fn rendering_materials(
    data: &[u8],
    range: Option<Range<usize>>,
    archive: ArchiveVersion,
) -> Vec<RenderingMaterialReference> {
    let Some(range) = range else {
        return Vec::new();
    };
    (|| {
        let (mut reader, version) = anonymous(data, range, archive)?;
        if version.0 != 1 {
            return Err(structural(
                reader.position(),
                "rendering-attributes version is unsupported",
            ));
        }
        let count = reader.i32()?;
        let count = usize::try_from(count)
            .map_err(|_| structural(reader.position() - 4, "negative rendering-material count"))?;
        if count > 1 << 16 {
            return Err(structural(
                reader.position() - 4,
                "rendering-material count exceeds limit",
            ));
        }
        let mut values = Vec::new();
        for _ in 0..count {
            let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
            let parsed = (|| {
                let mut value = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
                if value.i32()? != 1 {
                    return Err(structural(
                        value.position(),
                        "rendering-material version is unsupported",
                    ));
                }
                let minor = value.i32()?;
                let plugin_uuid = uuid(&mut value)?.to_string();
                let front_material_uuid = uuid(&mut value)?.to_string();
                let mapping_count = value.i32()?;
                if mapping_count != 0 {
                    return Err(structural(
                        value.position() - 4,
                        "obsolete rendering mappings are nonempty",
                    ));
                }
                let (back_material_uuid, material_source) = if minor >= 1 {
                    let id = uuid(&mut value)?;
                    let source = value.u8()?;
                    value.skip(3)?;
                    ((!id.is_nil()).then(|| id.to_string()), Some(source))
                } else {
                    (None, None)
                };
                if value.remaining() != 0 {
                    return Err(structural(
                        value.position(),
                        "rendering-material reference has trailing bytes",
                    ));
                }
                Ok(RenderingMaterialReference {
                    plugin_uuid,
                    front_material_uuid,
                    back_material_uuid,
                    material_source,
                })
            })();
            if let Ok(value) = parsed {
                values.push(value);
            }
            reader.skip(chunk.next_offset - reader.position())?;
        }
        if reader.remaining() != 0 {
            return Err(structural(
                reader.position(),
                "rendering attributes have trailing bytes",
            ));
        }
        Ok(values)
    })()
    .unwrap_or_default()
}

fn parse_font(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<FontRecord, FramingError> {
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(structural(reader.position(), "font wrapper is invalid"));
    }
    let mut value = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    let (major, minor) = (value.i32()?, value.i32()?);
    if major != 1 || !(0..=6).contains(&minor) {
        return Err(structural(value.position(), "font version is unsupported"));
    }
    let mut font = FontRecord {
        characteristics: value.u32()?,
        windows_logfont_name: utf16(&mut value)?,
        postscript_name: utf16(&mut value)?,
        ..FontRecord::default()
    };
    if minor >= 1 {
        font.obsolete_description = utf16(&mut value)?;
    }
    if minor >= 2 {
        font.windows_logfont_weight = Some(value.i32()?);
        font.apple_weight_trait = Some(read_finite(&mut value, "Apple font weight trait")?);
    }
    if minor >= 3 {
        font.point_size = Some(read_finite(&mut value, "font point size")?);
        if value.bool()? {
            value.skip(4 + 16)?;
        }
    }
    if minor >= 4 {
        font.family_name = utf16(&mut value)?;
    }
    if minor >= 5 {
        font.locale_name = utf16(&mut value)?;
        font.localized_postscript_name = utf16(&mut value)?;
        font.english_postscript_name = utf16(&mut value)?;
        font.localized_logfont_name = utf16(&mut value)?;
        font.english_logfont_name = utf16(&mut value)?;
        font.localized_family_name = utf16(&mut value)?;
        font.english_family_name = utf16(&mut value)?;
        font.localized_face_name = utf16(&mut value)?;
        font.english_face_name = utf16(&mut value)?;
        let panose = chunk_at(data, value.position(), value.end(), archive, false)?;
        if panose.typecode != ANONYMOUS || panose.short {
            return Err(structural(
                value.position(),
                "font PANOSE wrapper is invalid",
            ));
        }
        let mut bytes = BoundedReader::new(data, panose.body.start, panose.body.end)?;
        if bytes.u8()? != 0x10 || bytes.remaining() != 10 {
            return Err(structural(
                bytes.position(),
                "font PANOSE version is unsupported",
            ));
        }
        font.panose = Some(bytes.array()?);
        value.skip(panose.next_offset - value.position())?;
    }
    if minor >= 6 {
        font.quartet_member = Some(value.u8()?);
    }
    if value.remaining() != 0 {
        return Err(structural(value.position(), "font has trailing bytes"));
    }
    reader.skip(chunk.next_offset - reader.position())?;
    Ok(font)
}

fn parse_text_style(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    source_offset: usize,
) -> Result<TextStyleRecord, FramingError> {
    if data.get(range.start).copied() != Some(0) {
        let mut reader = BoundedReader::new(data, range.start, range.end)?;
        let packed = reader.u8()?;
        if packed >> 4 != 1 || packed & 0x0f > 2 {
            return Err(structural(
                range.start,
                "legacy text-style version is unsupported",
            ));
        }
        let index = reader.i32()?;
        let description = utf16(&mut reader)?;
        let mut face_units = [0_u16; 64];
        for unit in &mut face_units {
            *unit = reader.u16()?;
        }
        let face_end = face_units.iter().position(|unit| *unit == 0).unwrap_or(64);
        let windows_logfont_name = String::from_utf16_lossy(&face_units[..face_end]);
        let mut font = FontRecord {
            windows_logfont_name,
            postscript_name: description.clone(),
            obsolete_description: description.clone(),
            ..FontRecord::default()
        };
        if packed & 0x0f >= 1 {
            font.windows_logfont_weight = Some(reader.i32()?);
            let italic = reader.i32()?;
            if !matches!(italic, 0 | 1) {
                return Err(structural(
                    reader.position() - 4,
                    "legacy font italic flag is invalid",
                ));
            }
            let _linefeed_ratio = read_finite(&mut reader, "legacy font linefeed ratio")?;
            font.characteristics = u32::from(italic != 0);
        }
        let id = if packed & 0x0f >= 2 {
            uuid(&mut reader)?
        } else {
            Uuid::nil()
        };
        if reader.remaining() != 0 {
            return Err(structural(
                reader.position(),
                "legacy text style has trailing bytes",
            ));
        }
        return Ok(TextStyleRecord {
            id: format!("rhino:presentation:text_style#index-{index}-offset-{source_offset}"),
            source_offset: source_offset as u64,
            archive_index: index,
            source_uuid: (!id.is_nil()).then(|| id.to_string()),
            name: description.clone(),
            font_description: description,
            font,
        });
    }

    let (mut reader, version) = anonymous(data, range, archive)?;
    if version.0 != 1 || !(0..=1).contains(&version.1) {
        return Err(structural(
            reader.position(),
            "text-style version is unsupported",
        ));
    }
    let component = component(data, &mut reader, archive)?;
    let font_description = if reader.bool()? {
        utf16(&mut reader)?
    } else {
        String::new()
    };
    let font = if reader.bool()? {
        parse_font(data, &mut reader, archive)?
    } else {
        FontRecord::default()
    };
    let (id, name) = if version.1 >= 1 {
        (uuid(&mut reader)?, utf16(&mut reader)?)
    } else {
        (component.id, component.name)
    };
    if reader.remaining() != 0 {
        return Err(structural(
            reader.position(),
            "text style has trailing bytes",
        ));
    }
    let index = component.index.unwrap_or(-1);
    Ok(TextStyleRecord {
        id: if id.is_nil() {
            format!("rhino:presentation:text_style#index-{index}-offset-{source_offset}")
        } else {
            format!("rhino:presentation:text_style#{id}")
        },
        source_offset: source_offset as u64,
        archive_index: index,
        source_uuid: (!id.is_nil()).then(|| id.to_string()),
        name,
        font_description,
        font,
    })
}

pub(crate) fn install(scan: &Scan<'_>, ir: &mut CadIr) {
    let scale = scan
        .metadata
        .settings
        .units
        .as_ref()
        .and_then(|units| units.millimeters_per_unit)
        .unwrap_or(1.0);
    let mut groups = Vec::new();
    let mut materials = Vec::new();
    let mut lights = Vec::new();
    let mut light_indexes = BTreeMap::new();
    let mut linetypes = Vec::new();
    let mut hatch_patterns = Vec::new();
    let mut dimension_styles = Vec::new();
    let mut images = Vec::new();
    let mut windows_bitmaps = Vec::new();
    let mut texture_mappings = Vec::new();
    let mut text_styles = Vec::new();
    let mut layers = Vec::new();
    let mut object_presentation = Vec::new();
    let mut object_id_counts = BTreeMap::<Uuid, usize>::new();
    for object in &scan.objects {
        if let Some(identity) = &object.identity {
            *object_id_counts.entry(identity.object_id).or_default() += 1;
        }
    }
    for table in &scan.tables {
        let table_type = table.typecode & !0x0000_8000;
        for record in &table.records {
            if table_type == GROUP_TABLE {
                if let Ok(range) = class_data(scan.data, record, scan.archive, GROUP) {
                    if let Ok(group) = parse_group(scan.data, range, record.range.start) {
                        groups.push(group);
                    }
                }
            } else if table_type == MATERIAL_TABLE {
                if let Ok(range) = class_data(scan.data, record, scan.archive, MATERIAL) {
                    if let Ok(material) =
                        parse_material(scan.data, range, scan.archive, record.range.start)
                    {
                        materials.push(material);
                    }
                }
            } else if table_type == LIGHT_TABLE {
                if let Ok(range) = class_data(scan.data, record, scan.archive, LIGHT) {
                    if let Ok(light) =
                        parse_light(scan.data, range, scale, record.range.start, None)
                    {
                        push_light(&mut lights, &mut light_indexes, light);
                    }
                }
            } else if table_type == LINETYPE_TABLE {
                if let Ok(range) = class_data(scan.data, record, scan.archive, LINETYPE) {
                    if let Ok(value) =
                        parse_linetype(scan.data, range, scan.archive, scale, record.range.start)
                    {
                        linetypes.push(value);
                    }
                }
            } else if table_type == HATCH_PATTERN_TABLE {
                if let Ok(range) = class_data(scan.data, record, scan.archive, HATCH_PATTERN) {
                    if let Ok(value) = parse_hatch_pattern(
                        scan.data,
                        range,
                        scan.archive,
                        scale,
                        record.range.start,
                    ) {
                        hatch_patterns.push(value);
                    }
                }
            } else if table_type == DIMSTYLE_TABLE {
                if let Ok(range) = class_data(scan.data, record, scan.archive, DIMSTYLE) {
                    if let Ok(value) = parse_dimension_style(
                        scan.data,
                        range,
                        scan.archive,
                        scale,
                        record.range.start,
                    ) {
                        dimension_styles.push(value);
                    }
                }
            } else if table_type == BITMAP_TABLE {
                if let Ok(range) = class_data(scan.data, record, scan.archive, EMBEDDED_BITMAP) {
                    if let Ok(value) =
                        parse_embedded_image(scan.data, range, scan.archive, record.range.start)
                    {
                        images.push(value);
                    }
                } else if let Ok(class) = parse_class_wrapper(
                    scan.data,
                    record.body.clone(),
                    scan.archive,
                    &mut Vec::new(),
                ) {
                    if matches!(class.class_uuid, WINDOWS_BITMAP | WINDOWS_BITMAP_EX) {
                        if let Ok(value) = parse_windows_bitmap(
                            scan.data,
                            class.class_data_range,
                            class.class_uuid,
                            record.range.start,
                        ) {
                            windows_bitmaps.push(value);
                        }
                    }
                }
            } else if table_type == TEXTURE_MAPPING_TABLE {
                if let Ok(range) = class_data(scan.data, record, scan.archive, TEXTURE_MAPPING) {
                    if let Ok(value) =
                        parse_texture_mapping(scan.data, range, scan.archive, record.range.start)
                    {
                        texture_mappings.push(value);
                    }
                }
            } else if table_type == FONT_TABLE {
                if let Ok(range) = class_data(scan.data, record, scan.archive, TEXT_STYLE) {
                    if let Ok(value) =
                        parse_text_style(scan.data, range, scan.archive, record.range.start)
                    {
                        text_styles.push(value);
                    }
                }
            }
        }
    }
    let mut group_members = BTreeMap::<i32, Vec<String>>::new();
    for (source_order, object) in scan.objects.iter().enumerate() {
        if let Some(attributes) = &object.attributes {
            for group in &attributes.groups {
                group_members
                    .entry(*group)
                    .or_default()
                    .push(format!("rhino:object:record#{source_order:06}"));
            }
        }
        if object.class_uuid == LIGHT {
            let link = format!("rhino:object:record#{source_order:06}");
            if let Ok(light) = parse_light(
                scan.data,
                object.class_data_range.clone(),
                scale,
                object.range.start,
                Some(link),
            ) {
                push_light(&mut lights, &mut light_indexes, light);
            }
        }
        if let (Some(identity), Some(attributes)) = (&object.identity, &object.attributes) {
            let key = if identity.object_id.is_nil()
                || object_id_counts.get(&identity.object_id).copied() != Some(1)
            {
                format!("record-{source_order:06}")
            } else {
                identity.object_id.to_string()
            };
            object_presentation.push(ObjectPresentationRecord {
                id: format!("rhino:presentation:object#{key}"),
                source_offset: object.range.start as u64,
                source_uuid: identity.object_id.to_string(),
                name: attributes.name.clone(),
                url: attributes.url.clone(),
                layer_index: attributes.layer_index,
                material_index: attributes.material_index,
                linetype_index: attributes.linetype_index,
                color: attributes.color,
                visible: attributes.visible,
                object_mode: attributes.object_mode,
                decoration: attributes.decoration,
                wire_density: attributes.wire_density,
                color_source: attributes.color_source,
                linetype_source: attributes.linetype_source,
                material_source: attributes.material_source,
                plot_color_source: attributes.plot_color_source,
                plot_weight_source: attributes.plot_weight_source,
                plot_color: attributes.plot_color,
                plot_weight_mm: attributes.plot_weight,
                group_indexes: attributes.groups.clone(),
                display_materials: attributes
                    .display_materials
                    .iter()
                    .map(|(viewport, material)| [viewport.to_string(), material.to_string()])
                    .collect(),
                active_space: attributes.active_space,
                viewport_uuid: (!attributes.viewport_id.is_nil())
                    .then(|| attributes.viewport_id.to_string()),
                display_order: attributes.display_order,
                clipping_proof: attributes.clipping_proof,
                clipping_plane_uuids: attributes
                    .clipping_plane_ids
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                hatch_pattern_index: attributes.hatch_pattern_index,
                section_hatch_scale: attributes.section_hatch_scale,
                section_hatch_rotation: attributes.section_hatch_rotation,
                linetype_pattern_scale: attributes.linetype_pattern_scale,
                hatch_background: attributes.hatch_background,
                hatch_boundary_visible: attributes.hatch_boundary_visible,
                section_fill_rule: attributes.section_fill_rule,
                clipping_plane_label_style: attributes.clipping_plane_label_style,
                rendering_materials: rendering_materials(
                    scan.data,
                    attributes.rendering_range.clone(),
                    scan.archive,
                ),
                links: vec![format!("rhino:object:record#{source_order:06}")],
            });
        }
    }
    let mut layer_id_counts = BTreeMap::<Uuid, usize>::new();
    for layer in &scan.metadata.layers {
        if let Some(id) = layer.id {
            *layer_id_counts.entry(id).or_default() += 1;
        }
    }
    for layer in &scan.metadata.layers {
        let key = layer
            .id
            .filter(|id| layer_id_counts.get(id).copied() == Some(1))
            .map_or_else(
                || format!("index-{}-offset-{}", layer.index, layer.source.range.start),
                |id| id.to_string(),
            );
        layers.push(LayerPresentationRecord {
            id: format!("rhino:presentation:layer#{key}"),
            source_offset: layer.source.range.start as u64,
            archive_index: layer.index,
            source_uuid: layer.id.map(|id| id.to_string()),
            parent_uuid: layer
                .parent_id
                .filter(|id| !id.is_nil())
                .map(|id| id.to_string()),
            name: layer.name.clone(),
            visible: layer.visible,
            locked: layer.locked,
            expanded: layer.expanded,
            color: layer.color,
            material_index: layer.render_material_index,
            linetype_index: layer.linetype_index,
            plot_color: layer.plot_color,
            plot_weight_mm: layer.plot_weight,
            display_material_uuid: layer
                .display_material_id
                .filter(|id| !id.is_nil())
                .map(|id| id.to_string()),
            clipping_planes_enabled: layer.no_clipping_planes.map(|value| !value),
            rendering_materials: rendering_materials(
                scan.data,
                layer.rendering_range.clone(),
                scan.archive,
            ),
        });
    }
    for group in &mut groups {
        group.links = group_members
            .remove(&group.archive_index)
            .unwrap_or_default();
        group.links.sort();
    }
    let namespace = ir.native.namespace_mut("rhino");
    namespace.version = namespace.version.max(2);
    namespace
        .set_arena("groups", &groups)
        .expect("Rhino groups serialize");
    namespace
        .set_arena("materials", &materials)
        .expect("Rhino materials serialize");
    namespace
        .set_arena("lights", &lights)
        .expect("Rhino lights serialize");
    namespace
        .set_arena("linetypes", &linetypes)
        .expect("Rhino linetypes serialize");
    namespace
        .set_arena("hatch_patterns", &hatch_patterns)
        .expect("Rhino hatch patterns serialize");
    namespace
        .set_arena("dimension_styles", &dimension_styles)
        .expect("Rhino dimension styles serialize");
    namespace
        .set_arena("embedded_images", &images)
        .expect("Rhino images serialize");
    namespace
        .set_arena("windows_bitmaps", &windows_bitmaps)
        .expect("Rhino Windows bitmaps serialize");
    namespace
        .set_arena("texture_mappings", &texture_mappings)
        .expect("Rhino texture mappings serialize");
    namespace
        .set_arena("text_styles", &text_styles)
        .expect("Rhino text styles serialize");
    namespace
        .set_arena("layers", &layers)
        .expect("Rhino layers serialize");
    namespace
        .set_arena("object_presentation", &object_presentation)
        .expect("Rhino object presentation serializes");
}

#[cfg(test)]
mod tests {
    use super::{
        parse_group, parse_hatch_pattern, parse_light, parse_linetype, parse_material,
        parse_text_style,
    };
    use crate::chunks::ArchiveVersion;

    fn utf16(value: &str) -> Vec<u8> {
        let mut units = value.encode_utf16().collect::<Vec<_>>();
        units.push(0);
        let mut bytes = (units.len() as u32).to_le_bytes().to_vec();
        for unit in units {
            bytes.extend(unit.to_le_bytes());
        }
        bytes
    }

    fn anonymous(minor: i32, body: &[u8]) -> Vec<u8> {
        let mut payload = 1_i32.to_le_bytes().to_vec();
        payload.extend(minor.to_le_bytes());
        payload.extend(body);
        payload.extend(crc32fast::hash(&payload).to_le_bytes());
        let mut bytes = 0x4000_8000_u32.to_le_bytes().to_vec();
        bytes.extend((payload.len() as i64).to_le_bytes());
        bytes.extend(payload);
        bytes
    }

    #[test]
    fn legacy_text_style_preserves_font_identity_and_characteristics() {
        let mut bytes = vec![0x12];
        bytes.extend(7_i32.to_le_bytes());
        bytes.extend(utf16("Helvetica Neue"));
        let mut face = [0_u16; 64];
        for (target, source) in face.iter_mut().zip("Helvetica Neue".encode_utf16()) {
            *target = source;
        }
        for unit in face {
            bytes.extend(unit.to_le_bytes());
        }
        bytes.extend(700_i32.to_le_bytes());
        bytes.extend(1_i32.to_le_bytes());
        bytes.extend(1.6_f64.to_le_bytes());
        bytes.extend([0x11; 16]);
        let value = parse_text_style(&bytes, 0..bytes.len(), ArchiveVersion::V8, 42)
            .expect("valid legacy text style");
        assert_eq!(value.archive_index, 7);
        assert_eq!(value.font.windows_logfont_name, "Helvetica Neue");
        assert_eq!(value.font.windows_logfont_weight, Some(700));
        assert_eq!(value.font.characteristics, 1);
        assert_eq!(value.source_offset, 42);
    }

    #[test]
    fn group_preserves_component_identity() {
        let mut bytes = vec![0x11];
        bytes.extend(7_i32.to_le_bytes());
        bytes.extend(utf16("fixtures"));
        bytes.extend([0x44; 16]);
        let group = parse_group(&bytes, 0..bytes.len(), 120).expect("required invariant");
        assert_eq!(group.archive_index, 7);
        assert_eq!(group.name, "fixtures");
        assert_eq!(group.source_offset, 120);
    }

    #[test]
    fn light_scales_spatial_values_but_not_direction_or_angles() {
        let mut bytes = vec![0x12];
        bytes.extend(1_i32.to_le_bytes());
        bytes.extend(4_i32.to_le_bytes());
        bytes.extend(0.5_f64.to_le_bytes());
        bytes.extend(20.0_f64.to_le_bytes());
        bytes.extend([1, 2, 3, 4]);
        bytes.extend([5, 6, 7, 8]);
        bytes.extend([9, 10, 11, 12]);
        for value in [0.0_f64, 0.0, -1.0, 1.0, 2.0, 3.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(0.25_f64.to_le_bytes());
        bytes.extend(16.0_f64.to_le_bytes());
        for value in [1.0_f64, 0.0, 0.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(0.75_f64.to_le_bytes());
        bytes.extend(3_i32.to_le_bytes());
        bytes.extend([0x55; 16]);
        bytes.extend(utf16("key"));
        for value in [4.0_f64, 0.0, 0.0, 0.0, 5.0, 0.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(0.8_f64.to_le_bytes());
        let light = parse_light(&bytes, 0..bytes.len(), 10.0, 0, None).expect("required invariant");
        assert_eq!(light.location, [10.0, 20.0, 30.0]);
        assert_eq!(light.direction, [0.0, 0.0, -1.0]);
        assert_eq!(light.length, [40.0, 0.0, 0.0]);
        assert_eq!(light.spot_angle_radians, 0.25);
    }

    #[test]
    fn legacy_material_preserves_core_appearance_and_switches() {
        let mut body = [[0x11; 16].as_slice(), 2_i32.to_le_bytes().as_slice()].concat();
        body.extend(utf16("steel"));
        body.extend([0x22; 16]);
        for color in [
            [1, 2, 3, 4],
            [5, 6, 7, 8],
            [9, 10, 11, 12],
            [13, 14, 15, 16],
            [17, 18, 19, 20],
            [21, 22, 23, 24],
        ] {
            body.extend(color);
        }
        for value in [1.5_f64, 0.25, 64.0, 0.1] {
            body.extend(value.to_le_bytes());
        }
        body.extend(anonymous(0, &0_i32.to_le_bytes()));
        body.extend(utf16(""));
        body.extend(0_i32.to_le_bytes());
        body.extend([1, 0]);
        let inner = anonymous(3, &body);
        let mut bytes = vec![0x20];
        bytes.extend(inner);
        let material = parse_material(&bytes, 0..bytes.len(), ArchiveVersion::V5, 0)
            .expect("required invariant");
        assert_eq!(material.name, "steel");
        assert_eq!(material.diffuse, [5, 6, 7, 8]);
        assert_eq!(material.index_of_refraction, 1.5);
        assert!(material.shareable);
        assert!(!material.disable_lighting);
    }

    #[test]
    fn legacy_linetype_scales_pattern_lengths() {
        let mut body = 4_i32.to_le_bytes().to_vec();
        body.extend(utf16("dash"));
        body.extend(2_i32.to_le_bytes());
        body.extend(2.0_f64.to_le_bytes());
        body.extend(0_u32.to_le_bytes());
        body.extend(1.0_f64.to_le_bytes());
        body.extend(1_u32.to_le_bytes());
        body.extend([0x66; 16]);
        let bytes = anonymous(1, &body);
        let value = parse_linetype(&bytes, 0..bytes.len(), ArchiveVersion::V5, 10.0, 0)
            .expect("required invariant");
        assert_eq!(value.name, "dash");
        assert_eq!(value.segments[0].length_millimeters, 20.0);
        assert_eq!(value.segments[1].segment_type, 1);
    }

    #[test]
    fn legacy_hatch_pattern_scales_line_offsets_and_dashes() {
        let mut bytes = vec![0x12];
        bytes.extend(3_i32.to_le_bytes());
        bytes.extend(1_i32.to_le_bytes());
        bytes.extend(utf16("cross"));
        bytes.extend(utf16("cross hatch"));
        bytes.extend(1_i32.to_le_bytes());
        bytes.push(0x11);
        bytes.extend(0.5_f64.to_le_bytes());
        for value in [1.0_f64, 2.0, 3.0, 4.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(2_i32.to_le_bytes());
        bytes.extend(5.0_f64.to_le_bytes());
        bytes.extend((-2.0_f64).to_le_bytes());
        bytes.extend([0x77; 16]);
        let value = parse_hatch_pattern(&bytes, 0..bytes.len(), ArchiveVersion::V5, 10.0, 0)
            .expect("required invariant");
        assert_eq!(value.lines[0].base_millimeters, [10.0, 20.0]);
        assert_eq!(value.lines[0].offset_millimeters, [30.0, 40.0]);
        assert_eq!(value.lines[0].dashes_millimeters, [50.0, -20.0]);
        assert_eq!(value.lines[0].angle_radians, 0.5);
    }
}
