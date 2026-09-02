// SPDX-License-Identifier: Apache-2.0
//! Rhino appearance, grouping, and lighting presentation records.

use std::collections::BTreeMap;
use std::ops::Range;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::LossNote;
use serde::Serialize;

use crate::chunks::{
    checked_count_bytes, chunk_at, direct_checksum_ranges, verify_checksum_ranges, ArchiveVersion,
    BoundedReader, ChecksumStatus, FramingError,
};
use crate::container::{OpaqueRecord, Record, Scan};
use crate::loss::RhinoLossCode;
use crate::objects::{
    apply_attribute_userdata, parse_attribute_userdata, parse_attributes, parse_class_wrapper,
    parse_class_wrapper_with_userdata, parse_user_string_list, AttributeUserdataDescriptor,
    ObjectAttributes, UserdataDescriptor, USER_STRING_LIST,
};
use crate::settings::{self, utf16};
use crate::wire::{scaled_coordinate, Uuid};

const ANONYMOUS: u32 = 0x4000_8000;
const MODEL_ATTRIBUTES: u32 = 0x4000_8002;
const UTF8_STRING_CHUNK: u32 = 0x4000_8001;
const MATERIAL_TABLE: u32 = 0x1000_0010;
const LIGHT_TABLE: u32 = 0x1000_0012;
const LIGHT_RECORD_ATTRIBUTES: u32 = 0x0200_8061;
const LIGHT_RECORD_ATTRIBUTES_USERDATA: u32 = 0x0200_0062;
const LIGHT_RECORD_END: u32 = 0x8200_006f;
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
const PHYSICALLY_BASED_MATERIAL_USERDATA: Uuid = Uuid::from_canonical([
    0x56, 0x94, 0xe1, 0xac, 0x40, 0xe6, 0x44, 0xf4, 0x9c, 0xa9, 0x3b, 0x6d, 0x0e, 0x8c, 0x44, 0x40,
]);
const OPENNURBS6_APPLICATION: Uuid = Uuid::from_canonical([
    0x7b, 0x0b, 0x58, 0x5d, 0x7a, 0x31, 0x45, 0xd0, 0x92, 0x5e, 0xbd, 0xd7, 0xdd, 0xf3, 0xe4, 0xe3,
]);
const RDK_CLASS: Uuid = Uuid::from_canonical([
    0xaf, 0xa8, 0x27, 0x72, 0x15, 0x25, 0x43, 0xdd, 0xa6, 0x3c, 0xc8, 0x4a, 0xc5, 0x80, 0x69, 0x11,
]);
const RDK_USERDATA: Uuid = Uuid::from_canonical([
    0xb6, 0x3e, 0xd0, 0x79, 0xcf, 0x67, 0x41, 0x6c, 0x80, 0x0d, 0x22, 0x02, 0x3a, 0xe1, 0xbe, 0x21,
]);
const RDK_APPLICATION: Uuid = Uuid::from_canonical([
    0x16, 0x59, 0x2d, 0x58, 0x4a, 0x2f, 0x40, 0x1d, 0xbf, 0x5e, 0x3b, 0x87, 0x74, 0x1c, 0x1b, 0x1b,
]);
const UNIVERSAL_RENDER_ENGINE: Uuid = Uuid::from_canonical([
    0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99, 0x99,
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
const V5_DIMSTYLE: Uuid = Uuid::from_canonical([
    0x81, 0xbd, 0x83, 0xd5, 0x71, 0x20, 0x41, 0xc4, 0x9a, 0x57, 0xc4, 0x49, 0x33, 0x6f, 0xf1, 0x2c,
]);
const DIMSTYLE_EXTRA: Uuid = Uuid::from_canonical([
    0x51, 0x3f, 0xde, 0x53, 0x72, 0x84, 0x40, 0x65, 0x86, 0x01, 0x06, 0xce, 0xa8, 0xb2, 0x8d, 0x6f,
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
pub(crate) const TEXTURE_MAPPING: Uuid = Uuid::from_canonical([
    0x32, 0xec, 0x99, 0x7a, 0xc3, 0xbf, 0x4a, 0xe5, 0xab, 0x19, 0xfd, 0x57, 0x2b, 0x8a, 0xd5, 0x54,
]);
pub(crate) const MAPPING_CRC_CACHE: Uuid = Uuid::from_canonical([
    0x5a, 0x49, 0x71, 0xf3, 0xaa, 0x73, 0x49, 0x3c, 0xa3, 0x85, 0x2f, 0x7e, 0xb4, 0x28, 0x89, 0x89,
]);
const TEXT_STYLE: Uuid = Uuid::from_canonical([
    0x4f, 0x0f, 0x51, 0xfb, 0x35, 0xd0, 0x48, 0x65, 0x99, 0x98, 0x6d, 0x2c, 0x6a, 0x99, 0x72, 0x1d,
]);
const TEXTURE: Uuid = Uuid::from_canonical([
    0xd6, 0xff, 0x10, 0x6d, 0x32, 0x9b, 0x4f, 0x29, 0x97, 0xe2, 0xfd, 0x28, 0x2a, 0x61, 0x80, 0x20,
]);
const MAX_DIMSTYLE_EXTRA_FIELDS: usize = 1 << 16;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    archive_index: Option<i32>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    physically_based: Option<PhysicallyBasedMaterialRecord>,
}

#[derive(Debug, Serialize)]
struct PhysicallyBasedMaterialRecord {
    version: i32,
    base_color: [f32; 4],
    brdf: i32,
    subsurface: f64,
    subsurface_scattering_color: [f32; 4],
    subsurface_scattering_radius: f64,
    metallic: f64,
    specular: f64,
    specular_tint: f64,
    roughness: f64,
    anisotropic: f64,
    anisotropic_rotation: f64,
    sheen: f64,
    sheen_tint: f64,
    clearcoat: f64,
    clearcoat_roughness: f64,
    opacity_ior: f64,
    opacity: f64,
    opacity_roughness: f64,
    emission: [f32; 4],
    alpha: f64,
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
    spot_angle_degrees: f64,
    spot_exponent: f64,
    attenuation: [f64; 3],
    shadow_intensity: f64,
    length: [f64; 3],
    width: [f64; 3],
    hotspot: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    attributes: Option<LightAttributesRecord>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    archive_index: Option<i32>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    archive_index: Option<i32>,
    source_uuid: Option<String>,
    name: String,
    fill_type: i32,
    description: String,
    lines: Vec<HatchLineRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pattern_unit_system: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    always_model_distances: Option<bool>,
}

#[derive(Debug, Serialize)]
struct DimensionStyleRecord {
    id: String,
    source_offset: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    archive_index: Option<i32>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    v5_extra: Option<V5DimensionStyleExtraRecord>,
}

#[derive(Debug, Serialize)]
struct V5DimensionStyleExtraRecord {
    parent_style_uuid: Option<String>,
    valid_fields: Vec<bool>,
    tolerance_style: i32,
    tolerance_resolution: i32,
    tolerance_upper_value: f64,
    tolerance_lower_value: f64,
    tolerance_height_scale: f64,
    baseline_spacing_mm: f64,
    draw_text_mask: bool,
    mask_color_source: i32,
    mask_color: [u8; 4],
    dimension_scale: f64,
    dimension_scale_source: i32,
    source_style_uuid: Option<String>,
}

#[derive(Debug, Default, Serialize)]
struct FontRecord {
    characteristics: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    legacy_italic: Option<bool>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    archive_index: Option<i32>,
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
struct RenderingMappingChannel {
    mapping_channel_id: i32,
    mapping_uuid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    object_transform: Option<[[f64; 4]; 4]>,
}

#[derive(Debug, Serialize)]
struct RenderingMappingReference {
    plugin_uuid: String,
    channels: Vec<RenderingMappingChannel>,
}

#[derive(Debug, Default)]
struct RenderingAttributesPresentation {
    materials: Vec<RenderingMaterialReference>,
    mappings: Vec<RenderingMappingReference>,
    casts_shadows: Option<bool>,
    receives_shadows: Option<bool>,
    advanced_texture_preview: Option<bool>,
}

#[derive(Debug, Serialize)]
struct MeshModifiersRecord {
    #[serde(skip_serializing_if = "Option::is_none")]
    displacement: Option<DisplacementRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    edge_softening: Option<EdgeSofteningRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thickening: Option<ThickeningRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    curve_piping: Option<CurvePipingRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shut_lining: Option<ShutLiningRecord>,
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct DisplacementRecord {
    xml_version: i32,
    on: bool,
    texture: Option<String>,
    channel: i32,
    black_point: f64,
    white_point: f64,
    sweep_pitch: i32,
    refine_steps: i32,
    refine_sensitivity: f64,
    face_count_limit_enabled: bool,
    face_count_limit: i32,
    post_weld_angle: f64,
    mesh_memory_limit: i32,
    fairing_enabled: bool,
    fairing_amount: i32,
    sub_object_count: Option<i32>,
    sweep_resolution_formula: i32,
    sub_items: Vec<DisplacementSubItemRecord>,
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct DisplacementSubItemRecord {
    face_index: i32,
    on: bool,
    texture: Option<String>,
    channel: i32,
    black_point: f64,
    white_point: f64,
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct EdgeSofteningRecord {
    xml_version: i32,
    on: bool,
    softening: f64,
    chamfer: bool,
    faceted: bool,
    force_softening: bool,
    edge_angle_threshold: f64,
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct ThickeningRecord {
    xml_version: i32,
    on: bool,
    solid: bool,
    both_sides: bool,
    offset_only: bool,
    distance: f64,
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct CurvePipingRecord {
    xml_version: i32,
    on: bool,
    radius: f64,
    segments: i32,
    faceted: bool,
    accuracy: i32,
    cap_type: String,
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct ShutLiningRecord {
    xml_version: i32,
    on: bool,
    faceted: bool,
    auto_update: bool,
    force_update: bool,
    curves: Vec<ShutLiningCurveRecord>,
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct ShutLiningCurveRecord {
    uuid: Option<String>,
    radius: f64,
    profile: i32,
    enabled: bool,
    pull: bool,
    is_bump: bool,
}

fn mesh_modifiers_record(modifiers: &crate::mesh_modifiers::MeshModifiers) -> MeshModifiersRecord {
    MeshModifiersRecord {
        displacement: modifiers.displacement.as_ref().map(displacement_record),
        edge_softening: modifiers.edge_softening.as_ref().map(edge_softening_record),
        thickening: modifiers.thickening.as_ref().map(thickening_record),
        curve_piping: modifiers.curve_piping.as_ref().map(curve_piping_record),
        shut_lining: modifiers.shut_lining.as_ref().map(shut_lining_record),
    }
}

fn displacement_record(
    displacement: &crate::mesh_modifiers::DisplacementModifier,
) -> DisplacementRecord {
    DisplacementRecord {
        xml_version: displacement.xml_version,
        on: displacement.on,
        texture: displacement.texture.map(|uuid| uuid.to_string()),
        channel: displacement.channel,
        black_point: displacement.black_point,
        white_point: displacement.white_point,
        sweep_pitch: displacement.sweep_pitch,
        refine_steps: displacement.refine_steps,
        refine_sensitivity: displacement.refine_sensitivity,
        face_count_limit_enabled: displacement.face_count_limit_enabled,
        face_count_limit: displacement.face_count_limit,
        post_weld_angle: displacement.post_weld_angle,
        mesh_memory_limit: displacement.mesh_memory_limit,
        fairing_enabled: displacement.fairing_enabled,
        fairing_amount: displacement.fairing_amount,
        sub_object_count: displacement.sub_object_count,
        sweep_resolution_formula: displacement.sweep_resolution_formula,
        sub_items: displacement
            .sub_items
            .iter()
            .map(|item| DisplacementSubItemRecord {
                face_index: item.face_index,
                on: item.on,
                texture: item.texture.map(|uuid| uuid.to_string()),
                channel: item.channel,
                black_point: item.black_point,
                white_point: item.white_point,
            })
            .collect(),
    }
}

fn edge_softening_record(
    edge_softening: &crate::mesh_modifiers::EdgeSofteningModifier,
) -> EdgeSofteningRecord {
    EdgeSofteningRecord {
        xml_version: edge_softening.xml_version,
        on: edge_softening.on,
        softening: edge_softening.softening,
        chamfer: edge_softening.chamfer,
        faceted: edge_softening.faceted,
        force_softening: edge_softening.force_softening,
        edge_angle_threshold: edge_softening.edge_angle_threshold,
    }
}

fn thickening_record(thickening: &crate::mesh_modifiers::ThickeningModifier) -> ThickeningRecord {
    ThickeningRecord {
        xml_version: thickening.xml_version,
        on: thickening.on,
        solid: thickening.solid,
        both_sides: thickening.both_sides,
        offset_only: thickening.offset_only,
        distance: thickening.distance,
    }
}

fn curve_piping_record(
    curve_piping: &crate::mesh_modifiers::CurvePipingModifier,
) -> CurvePipingRecord {
    CurvePipingRecord {
        xml_version: curve_piping.xml_version,
        on: curve_piping.on,
        radius: curve_piping.radius,
        segments: curve_piping.segments,
        faceted: curve_piping.faceted,
        accuracy: curve_piping.accuracy,
        cap_type: curve_piping.cap_type.clone(),
    }
}

fn shut_lining_record(shut_lining: &crate::mesh_modifiers::ShutLiningModifier) -> ShutLiningRecord {
    ShutLiningRecord {
        xml_version: shut_lining.xml_version,
        on: shut_lining.on,
        faceted: shut_lining.faceted,
        auto_update: shut_lining.auto_update,
        force_update: shut_lining.force_update,
        curves: shut_lining
            .curves
            .iter()
            .map(|curve| ShutLiningCurveRecord {
                uuid: curve.uuid.map(|uuid| uuid.to_string()),
                radius: curve.radius,
                profile: curve.profile,
                enabled: curve.enabled,
                pull: curve.pull,
                is_bump: curve.is_bump,
            })
            .collect(),
    }
}

#[derive(Debug, Serialize)]
struct LayerPerViewportPresentationRecord {
    viewport_uuid: String,
    settings_mask: u32,
    color: Option<[u8; 4]>,
    plot_color: Option<[u8; 4]>,
    plot_weight_mm: Option<f64>,
    visible: Option<u8>,
    persistent_visibility: Option<u8>,
}

#[derive(Debug, Serialize)]
struct LayerPresentationRecord {
    id: String,
    source_offset: u64,
    archive_index: i32,
    source_uuid: Option<String>,
    parent_uuid: Option<String>,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    iges_level: Option<i32>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    visible_in_new_details: Option<bool>,
    rendering_materials: Vec<RenderingMaterialReference>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    per_viewport_settings: Vec<LayerPerViewportPresentationRecord>,
}

#[derive(Debug, Serialize)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent serialized object display flags"
)]
struct ObjectAttributesPresentation {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    detail_background_visible: Option<bool>,
    section_fill_rule: u8,
    clipping_plane_label_style: u8,
    rendering_materials: Vec<RenderingMaterialReference>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    rendering_mappings: Vec<RenderingMappingReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    casts_shadows: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    receives_shadows: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    advanced_texture_preview: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    user_strings: Vec<UserStringRecord>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    attribute_user_strings: Vec<UserStringRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    custom_render_mesh: Option<settings::MeshParameters>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mesh_modifiers: Option<MeshModifiersRecord>,
}

#[derive(Debug, Serialize)]
struct ObjectPresentationRecord {
    id: String,
    source_offset: u64,
    #[serde(flatten)]
    attributes: ObjectAttributesPresentation,
    links: Vec<String>,
}

#[derive(Debug, Serialize)]
struct LightAttributesRecord {
    source_offset: u64,
    #[serde(flatten)]
    attributes: ObjectAttributesPresentation,
    #[serde(skip)]
    userdata_requires_opaque: bool,
}

#[derive(Debug, Serialize)]
struct UserStringRecord {
    key: String,
    value: String,
}

fn user_string_records(entries: Vec<(String, String)>) -> Vec<UserStringRecord> {
    entries
        .into_iter()
        .map(|(key, value)| UserStringRecord { key, value })
        .collect()
}

fn first_user_string_records(
    data: &[u8],
    archive: ArchiveVersion,
    class_userdata: &[UserdataDescriptor],
    attribute_userdata: &[AttributeUserdataDescriptor],
    source_offset: usize,
    losses: &mut Vec<LossNote>,
) -> (Vec<UserStringRecord>, Vec<UserStringRecord>) {
    let geometry = class_userdata
        .iter()
        .find(|value| value.class_uuid == USER_STRING_LIST && value.item_uuid == USER_STRING_LIST)
        .and_then(|value| {
            match parse_user_string_list(data, value.payload_range.clone(), archive) {
                Ok(entries) => Some(user_string_records(entries)),
                Err(error) => {
                    losses.push(RhinoLossCode::ObjectDecodeDiagnostic.note(format!(
                        "object user-string userdata at offset {source_offset} could not be transferred: {error}"
                    )));
                    None
                }
            }
        })
        .unwrap_or_default();
    let mut attributes = attribute_userdata
        .iter()
        .find(|value| {
            value.class_uuid == Some(USER_STRING_LIST) && value.item_uuid == Some(USER_STRING_LIST)
        })
        .and_then(|value| {
            let Some(payload_range) = value.payload_range.clone() else {
                losses.push(RhinoLossCode::ObjectDecodeDiagnostic.note(format!(
                    "object-attributes user-string userdata at offset {source_offset} has no payload"
                )));
                return None;
            };
            match parse_user_string_list(data, payload_range, archive) {
                Ok(entries) => Some(user_string_records(entries)),
                Err(error) => {
                    losses.push(RhinoLossCode::ObjectDecodeDiagnostic.note(format!(
                        "object-attributes user-string userdata at offset {source_offset} could not be transferred: {error}"
                    )));
                    None
                }
            }
        })
        .unwrap_or_default();
    if let Some(index) = attributes
        .iter()
        .position(|value| value.key.eq_ignore_ascii_case("$temp_object$"))
    {
        attributes.remove(index);
    }
    (geometry, attributes)
}

#[allow(
    clippy::too_many_arguments,
    reason = "the projection keeps source data, both userdata owners, and loss reporting explicit"
)]
fn object_attributes_presentation(
    data: &[u8],
    attributes: &ObjectAttributes,
    class_userdata: &[UserdataDescriptor],
    attribute_userdata: &[AttributeUserdataDescriptor],
    archive: ArchiveVersion,
    source_offset: usize,
    source_uuid: String,
    losses: &mut Vec<LossNote>,
) -> ObjectAttributesPresentation {
    let rendering = rendering_attributes(
        data,
        attributes.rendering_range.clone(),
        archive,
        settings::RenderingAttributesKind::Object,
    )
    .unwrap_or_else(|error| {
        losses.push(RhinoLossCode::PresentationRecordDropped.note(format!(
            "object rendering attributes at offset {source_offset} could not be transferred: {error}"
        )));
        RenderingAttributesPresentation::default()
    });
    let (user_strings, attribute_user_strings) = first_user_string_records(
        data,
        archive,
        class_userdata,
        attribute_userdata,
        source_offset,
        losses,
    );
    ObjectAttributesPresentation {
        source_uuid,
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
        detail_background_visible: attributes.detail_background_visible.then_some(true),
        section_fill_rule: attributes.section_fill_rule,
        clipping_plane_label_style: attributes.clipping_plane_label_style,
        rendering_materials: rendering.materials,
        rendering_mappings: rendering.mappings,
        casts_shadows: rendering.casts_shadows,
        receives_shadows: rendering.receives_shadows,
        advanced_texture_preview: rendering.advanced_texture_preview,
        user_strings,
        attribute_user_strings,
        custom_render_mesh: attributes.custom_render_mesh.clone(),
        mesh_modifiers: attributes
            .mesh_modifiers
            .as_ref()
            .map(mesh_modifiers_record),
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
    value.is_finite().then_some(value).ok_or_else(|| {
        FramingError::structural(reader.position() - 8, format!("{label} is not finite"))
    })
}

fn read_finite(reader: &mut BoundedReader<'_>, label: &str) -> Result<f64, FramingError> {
    let value = reader.f64()?;
    finite(reader, value, label)
}

fn read_color_f32(reader: &mut BoundedReader<'_>, label: &str) -> Result<[f32; 4], FramingError> {
    let offset = reader.position();
    let color = [reader.f32()?, reader.f32()?, reader.f32()?, reader.f32()?];
    color
        .iter()
        .all(|value| value.is_finite())
        .then_some(color)
        .ok_or_else(|| {
            FramingError::structural(offset, format!("{label} contains a non-finite component"))
        })
}

fn finite3(reader: &mut BoundedReader<'_>, label: &str) -> Result<[f64; 3], FramingError> {
    let value = [reader.f64()?, reader.f64()?, reader.f64()?];
    value
        .iter()
        .all(|value| value.is_finite())
        .then_some(value)
        .ok_or_else(|| {
            FramingError::structural(reader.position() - 24, format!("{label} is not finite"))
        })
}

fn anonymous(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
) -> Result<(BoundedReader<'_>, (i32, i32)), FramingError> {
    let chunk = chunk_at(data, range.start, range.end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(FramingError::structural(
            range.start,
            "presentation wrapper is invalid",
        ));
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
        return Err(FramingError::structural(
            reader.position(),
            "model-component attributes are missing",
        ));
    }
    let mut value = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    let version = (value.i32()?, value.i32()?);
    if version.0 != 1 || version.1 < 0 {
        return Err(FramingError::structural(
            value.position(),
            "model-component version is unsupported",
        ));
    }
    if chunk.typecode == ANONYMOUS {
        let bits = value.u32()?;
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
        value.skip_remaining()?;
        reader.skip(chunk.next_offset - reader.position())?;
        return Ok(Component { index, id, name });
    }
    match value.u8()? {
        0 | 2 => {}
        1 => value.skip(12)?,
        _ => {}
    }
    let id = match value.u8()? {
        0 | 2 => Uuid::nil(),
        1 => uuid(&mut value)?,
        _ => Uuid::nil(),
    };
    match value.u8()? {
        0 | 2 => {}
        1 => value.skip(4)?,
        _ => {}
    }
    let index = match value.u8()? {
        0 | 2 => None,
        1 => Some(value.i32()?),
        _ => None,
    };
    let name = match value.u8()? {
        0 | 2 => String::new(),
        1 => utf16(&mut value)?,
        _ => String::new(),
    };
    value.skip_remaining()?;
    reader.skip(chunk.next_offset - reader.position())?;
    Ok(Component { index, id, name })
}

fn parse_physically_based_material(
    data: &[u8],
    payload_range: Range<usize>,
    archive: ArchiveVersion,
) -> Result<PhysicallyBasedMaterialRecord, FramingError> {
    let (mut reader, (major, version)) = anonymous(data, payload_range, archive)?;
    if major != 1 || !matches!(version, 1 | 2) {
        return Err(FramingError::structural(
            reader.position(),
            "physically based material payload version is unsupported",
        ));
    }
    let base_color = read_color_f32(&mut reader, "base color")?;
    let brdf = reader.i32()?;
    let subsurface = read_finite(&mut reader, "subsurface")?;
    let subsurface_scattering_color = read_color_f32(&mut reader, "subsurface scattering color")?;
    let subsurface_scattering_radius = read_finite(&mut reader, "subsurface scattering radius")?;
    let metallic = read_finite(&mut reader, "metallic")?;
    let specular = read_finite(&mut reader, "specular")?;
    let specular_tint = read_finite(&mut reader, "specular tint")?;
    let roughness = read_finite(&mut reader, "roughness")?;
    let anisotropic = read_finite(&mut reader, "anisotropic")?;
    let anisotropic_rotation = read_finite(&mut reader, "anisotropic rotation")?;
    let sheen = read_finite(&mut reader, "sheen")?;
    let sheen_tint = read_finite(&mut reader, "sheen tint")?;
    let clearcoat = read_finite(&mut reader, "clearcoat")?;
    let clearcoat_roughness = read_finite(&mut reader, "clearcoat roughness")?;
    let opacity_ior = read_finite(&mut reader, "opacity IOR")?;
    let opacity = read_finite(&mut reader, "opacity")?;
    let opacity_roughness = read_finite(&mut reader, "opacity roughness")?;
    let emission = read_color_f32(&mut reader, "emission")?;
    let alpha = if version >= 2 {
        read_finite(&mut reader, "alpha")?
    } else {
        1.0
    };
    reader.skip_remaining()?;
    Ok(PhysicallyBasedMaterialRecord {
        version,
        base_color,
        brdf,
        subsurface,
        subsurface_scattering_color,
        subsurface_scattering_radius,
        metallic,
        specular,
        specular_tint,
        roughness,
        anisotropic,
        anisotropic_rotation,
        sheen,
        sheen_tint,
        clearcoat,
        clearcoat_roughness,
        opacity_ior,
        opacity,
        opacity_roughness,
        emission,
        alpha,
    })
}

fn parse_uuid_text(value: &str) -> Option<Uuid> {
    let mut bytes = [0_u8; 16];
    let mut nibble = None;
    let mut index = 0;
    for byte in value.bytes() {
        if byte == b'-' {
            continue;
        }
        let value = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return None,
        };
        if let Some(high) = nibble.take() {
            if index == bytes.len() {
                return None;
            }
            bytes[index] = (high << 4) | value;
            index += 1;
        } else {
            nibble = Some(value);
        }
    }
    (index == bytes.len() && nibble.is_none()).then_some(Uuid::from_canonical(bytes))
}

fn parse_legacy_rdk_material_instance_id(
    data: &[u8],
    payload_range: Range<usize>,
) -> Result<Option<Uuid>, FramingError> {
    match classify_rdk_material_payload(data, payload_range)? {
        RdkMaterialPayload::Compatibility(instance_id) => Ok(instance_id),
        RdkMaterialPayload::CallbackOwned => Ok(None),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum RdkMaterialPayload {
    Compatibility(Option<Uuid>),
    CallbackOwned,
}

fn classify_rdk_material_payload(
    data: &[u8],
    payload_range: Range<usize>,
) -> Result<RdkMaterialPayload, FramingError> {
    let mut reader = BoundedReader::new(data, payload_range.start, payload_range.end)?;
    if reader.i32()? != 2 {
        return Err(FramingError::structural(
            payload_range.start,
            "legacy RDK material userdata version is unsupported",
        ));
    }
    let length = reader.i32()?;
    if !(0..=1024).contains(&length) {
        return Err(FramingError::InvalidLength {
            offset: reader.position() - 4,
            value: length.into(),
        });
    }
    if length == 0 {
        reader.skip_remaining()?;
        return Ok(RdkMaterialPayload::Compatibility(None));
    }
    let xml = reader.take(length as usize)?.to_vec();
    reader.skip_remaining()?;

    // The legacy writer omits the UTF-8 terminator that ON_XMLUserData::Write
    // includes. This distinguishes the compatibility carrier from callback-
    // owned RDK XML, which remains opaque in CADIR.
    if xml.last() == Some(&0) {
        return Ok(RdkMaterialPayload::CallbackOwned);
    }
    let xml = std::str::from_utf8(&xml).map_err(|_| {
        FramingError::structural(payload_range.start, "legacy RDK XML is not UTF-8")
    })?;
    let document = roxmltree::Document::parse(xml).map_err(|error| {
        FramingError::structural(
            payload_range.start,
            format!("legacy RDK XML is malformed: {error}"),
        )
    })?;
    let root = document.root_element();
    if root.tag_name().name() != "xml" {
        return Err(FramingError::structural(
            payload_range.start,
            "legacy RDK XML root is not xml",
        ));
    }
    let render_data = root
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "render-content-manager-data")
        .ok_or_else(|| {
            FramingError::structural(
                payload_range.start,
                "legacy RDK XML has no render-content-manager-data element",
            )
        })?;
    let material = render_data
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "material")
        .ok_or_else(|| {
            FramingError::structural(
                payload_range.start,
                "legacy RDK XML has no material element",
            )
        })?;
    let instance_id = material.attribute("instance-id").ok_or_else(|| {
        FramingError::structural(
            payload_range.start,
            "legacy RDK material has no instance-id attribute",
        )
    })?;
    let instance_id = parse_uuid_text(instance_id).ok_or_else(|| {
        FramingError::structural(
            payload_range.start,
            "legacy RDK material instance-id is not a UUID",
        )
    })?;
    Ok(RdkMaterialPayload::Compatibility(
        (!instance_id.is_nil()).then_some(instance_id),
    ))
}

fn legacy_rdk_material_instance_id(data: &[u8], userdata: &[UserdataDescriptor]) -> Option<Uuid> {
    userdata
        .iter()
        .filter(|value| {
            value.class_uuid == RDK_CLASS
                && value.item_uuid == RDK_USERDATA
                && (value.application_uuid.is_none()
                    || value.application_uuid == Some(RDK_APPLICATION))
        })
        .filter_map(|value| {
            parse_legacy_rdk_material_instance_id(data, value.payload_range.clone())
                .ok()
                .flatten()
        })
        .next_back()
}

fn rdk_material_userdata_requires_opaque(data: &[u8], userdata: &[UserdataDescriptor]) -> bool {
    userdata
        .iter()
        .filter(|value| {
            value.class_uuid == RDK_CLASS
                && value.item_uuid == RDK_USERDATA
                && (value.application_uuid.is_none()
                    || value.application_uuid == Some(RDK_APPLICATION))
        })
        .any(|value| {
            !matches!(
                classify_rdk_material_payload(data, value.payload_range.clone()),
                Ok(RdkMaterialPayload::Compatibility(_))
            )
        })
}

fn wide_string(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<String, FramingError> {
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != UTF8_STRING_CHUNK || chunk.short {
        return Err(FramingError::structural(
            reader.position(),
            "wide-string wrapper is invalid",
        ));
    }
    let mut value = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    let format = value.u8()?;
    let result = match format {
        0 if value.remaining() == 0 => String::new(),
        1 => std::str::from_utf8(value.take(value.remaining())?)
            .map(str::to_owned)
            .map_err(|_| FramingError::structural(value.position(), "wide string is not UTF-8"))?,
        _ => {
            return Err(FramingError::structural(
                value.position() - 1,
                "wide-string format is unsupported",
            ))
        }
    };
    reader.skip(chunk.next_offset - reader.position())?;
    Ok(result)
}

fn class_data(
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    expected: Uuid,
) -> Result<Range<usize>, FramingError> {
    let class = parse_class_wrapper(data, record.body.clone(), archive, &mut Vec::new())?;
    if class.class_uuid != expected {
        return Err(FramingError::structural(
            record.range.start,
            "table record has the wrong class",
        ));
    }
    Ok(class.class_data_range)
}

fn class_data_prefix(
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    expected: Uuid,
) -> Result<Range<usize>, FramingError> {
    let wrapper = chunk_at(data, record.body.start, record.body.end, archive, false)?;
    let class = parse_class_wrapper(
        data,
        wrapper.header_start..wrapper.next_offset,
        archive,
        &mut Vec::new(),
    )?;
    if class.class_uuid != expected {
        return Err(FramingError::structural(
            record.range.start,
            "table record has the wrong class",
        ));
    }
    Ok(class.class_data_range)
}

fn parse_light_record_attributes(
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    losses: &mut Vec<LossNote>,
) -> Result<Option<LightAttributesRecord>, FramingError> {
    let mut warnings = Vec::new();
    let _ = class_data_prefix(data, record, archive, LIGHT)?;
    let wrapper = chunk_at(data, record.body.start, record.body.end, archive, false)?;
    let mut offset = wrapper.next_offset;
    let mut attributes_chunk = None;
    let mut attributes_body_range = None;
    let mut attributes_userdata_body_range = None;
    let mut phase = 0_u8;
    let mut record_end_seen = false;
    while offset < record.body.end {
        let item = chunk_at(data, offset, record.body.end, archive, false)?;
        if item.typecode == LIGHT_RECORD_END {
            if !item.short || item.value != 0 {
                return Err(FramingError::structural(
                    item.header_start,
                    "light record end must be short with value zero",
                ));
            }
            if item.next_offset != record.body.end {
                return Err(FramingError::structural(
                    item.header_start,
                    "light record end is not final",
                ));
            }
            record_end_seen = true;
            break;
        }
        match item.typecode {
            LIGHT_RECORD_ATTRIBUTES if phase == 0 => {
                if item.short {
                    return Err(FramingError::structural(
                        item.header_start,
                        "light record attributes must be a long chunk",
                    ));
                }
                attributes_chunk = Some(item.clone());
                attributes_body_range = Some(item.body.clone());
                phase = 1;
            }
            LIGHT_RECORD_ATTRIBUTES_USERDATA if phase <= 1 => {
                if item.short {
                    return Err(FramingError::structural(
                        item.header_start,
                        "light attribute userdata must be a long chunk",
                    ));
                }
                attributes_userdata_body_range = Some(item.body.clone());
                phase = 2;
            }
            _ => {
                return Err(FramingError::structural(
                    item.header_start,
                    format!("unexpected light record child {:#x}", item.typecode),
                ));
            }
        }
        offset = item.next_offset;
    }
    if !record_end_seen {
        return Err(FramingError::structural(
            record.body.end,
            "light record is missing light record end",
        ));
    }

    let mut attributes = attributes_body_range
        .as_ref()
        .map(|body_range| {
            parse_attributes(
                data,
                body_range.clone(),
                attributes_chunk
                    .as_ref()
                    .map_or_else(|| body_range.clone(), crate::chunks::Chunk::range),
                archive,
                writer_version,
                &mut warnings,
            )
        })
        .transpose()?;
    let attributes_userdata = attributes_userdata_body_range
        .as_ref()
        .map(|range| parse_attribute_userdata(data, range.clone(), archive, &mut warnings))
        .unwrap_or_default();
    let userdata_requires_opaque = attributes_userdata.iter().any(|descriptor| {
        if !descriptor.known {
            return true;
        }
        let is_user_string = descriptor.class_uuid == Some(USER_STRING_LIST)
            && descriptor.item_uuid == Some(USER_STRING_LIST);
        if !is_user_string {
            return false;
        }
        descriptor
            .payload_range
            .as_ref()
            .is_none_or(|payload_range| {
                parse_user_string_list(data, payload_range.clone(), archive).is_err()
            })
    });
    if attributes.is_none() && !attributes_userdata.is_empty() {
        return Err(FramingError::structural(
            record.range.start,
            "light attribute userdata has no attributes owner",
        ));
    }
    if let Some(item) = attributes_chunk.as_ref() {
        let children = attributes
            .as_ref()
            .and_then(|value| value.rendering_range.clone())
            .into_iter()
            .collect::<Vec<_>>();
        let direct = direct_checksum_ranges(&item.body, &children)?;
        if let Some(note) = match verify_checksum_ranges(data, item, &direct)? {
            ChecksumStatus::Mismatch { expected, actual } => Some(format!(
                "CRC mismatch at offset {} for typecode {:#x}: expected {expected:#x}, got {actual:#x}",
                item.header_start, item.typecode
            )),
            _ => None,
        } {
            warnings.push(note);
        }
    }
    let Some(attributes) = attributes.as_mut() else {
        return Ok(None);
    };
    apply_attribute_userdata(
        data,
        attributes,
        &attributes_userdata,
        archive,
        &mut warnings,
    );
    let presentation = object_attributes_presentation(
        data,
        attributes,
        &[],
        &attributes_userdata,
        archive,
        record.range.start,
        attributes.object_id.to_string(),
        losses,
    );
    for warning in warnings {
        losses.push(RhinoLossCode::ObjectDecodeDiagnostic.note(format!(
            "light record attributes at offset {}: {warning}",
            record.range.start
        )));
    }
    Ok(Some(LightAttributesRecord {
        source_offset: attributes_chunk
            .as_ref()
            .map_or(record.range.start, |chunk| chunk.header_start) as u64,
        attributes: presentation,
        userdata_requires_opaque,
    }))
}

fn class_data_with_userdata(
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    expected: Uuid,
) -> Result<(Range<usize>, Vec<UserdataDescriptor>), FramingError> {
    let (class, userdata) =
        parse_class_wrapper_with_userdata(data, record.body.clone(), archive, &mut Vec::new())?;
    if class.class_uuid != expected {
        return Err(FramingError::structural(
            record.range.start,
            "table record has the wrong class",
        ));
    }
    Ok((class.class_data_range, userdata))
}

fn parse_texture(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    source_offset: usize,
) -> Result<TextureRecord, FramingError> {
    let (mut reader, version) = anonymous(data, range, archive)?;
    if version.0 != 1 || version.1 < 0 {
        return Err(FramingError::structural(
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
    reader.skip_remaining()?;
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
        return Err(FramingError::structural(
            reader.position(),
            "texture array is not anonymous",
        ));
    }
    let mut values = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    let version = (values.i32()?, values.i32()?);
    if version.0 != 1 || version.1 < 0 {
        return Err(FramingError::structural(
            values.position(),
            "texture array version is unsupported",
        ));
    }
    let count = values.i32()?;
    let count = usize::try_from(count)
        .map_err(|_| FramingError::structural(values.position() - 4, "negative texture count"))?;
    if count > 1 << 16 {
        return Err(FramingError::structural(
            values.position() - 4,
            "texture count exceeds limit",
        ));
    }
    let mut textures = Vec::new();
    for _ in 0..count {
        let object = chunk_at(data, values.position(), values.end(), archive, false)?;
        if object.short {
            return Err(FramingError::structural(
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
            return Err(FramingError::structural(
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
    values.skip_remaining()?;
    reader.skip(chunk.next_offset - reader.position())?;
    Ok(textures)
}

fn parse_v2_v3_texture(
    reader: &mut BoundedReader<'_>,
    source_offset: usize,
    texture_type: u32,
    is_bump: bool,
) -> Result<Option<TextureRecord>, FramingError> {
    let legacy_file_path = utf16(reader)?;
    let mode = reader.i32()?;
    let _obsolete_index = reader.i32()?;
    let bump_scale = if is_bump {
        [0.0, read_finite(reader, "legacy bump scale")?]
    } else {
        [0.0, 1.0]
    };
    if legacy_file_path.is_empty() {
        return Ok(None);
    }
    Ok(Some(TextureRecord {
        source_offset: source_offset as u64,
        source_uuid: None,
        mapping_channel_id: 1,
        legacy_file_path,
        enabled: true,
        texture_type,
        mode: if mode == 2 { 2 } else { 1 },
        minification_filter: 1,
        magnification_filter: 1,
        wrap: [0, 0, 0],
        uvw_transform: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        border_color: [255, 255, 255, 255],
        transparent_color: [255, 255, 255, 255],
        transparency_texture_uuid: None,
        bump_scale,
        alpha_blend: [1.0, 1.0, 1.0, 0.0, 0.0],
        rgb_blend_constant: [0, 0, 0, 0],
        rgb_blend: [1.0, 1.0, 0.0, 0.0],
        blend_order: 0,
        file_reference: None,
        treat_as_linear: None,
    }))
}

fn parse_v2_v3_material(
    data: &[u8],
    range: Range<usize>,
    source_offset: usize,
    physically_based: Option<PhysicallyBasedMaterialRecord>,
) -> Result<MaterialRecord, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let packed_version = reader.u8()?;
    if packed_version >> 4 != 1 {
        return Err(FramingError::structural(
            range.start,
            "V2/V3 material version is unsupported",
        ));
    }
    let minor = i32::from(packed_version & 0x0f);
    let ambient = reader.array()?;
    let diffuse = reader.array()?;
    let emission = reader.array()?;
    let specular = reader.array()?;
    let shine = read_finite(&mut reader, "shine")?;
    let transparency = read_finite(&mut reader, "transparency")?;
    reader.skip(4)?;
    let _obsolete_wire_color = reader.array::<4>()?;
    reader.skip(20)?;

    let mut textures = Vec::with_capacity(3);
    if let Some(texture) = parse_v2_v3_texture(&mut reader, source_offset, 1, false)? {
        textures.push(texture);
    }
    if let Some(texture) = parse_v2_v3_texture(&mut reader, source_offset, 2, true)? {
        textures.push(texture);
    }
    if let Some(texture) = parse_v2_v3_texture(&mut reader, source_offset, 86, false)? {
        textures.push(texture);
    }

    let archive_index = reader.i32()?;
    let plugin = uuid(&mut reader)?;
    let _obsolete_library = utf16(&mut reader)?;
    let name = utf16(&mut reader)?;
    let (id, reflection, transparent, index_of_refraction) = if minor >= 1 {
        (
            uuid(&mut reader)?,
            reader.array()?,
            reader.array()?,
            read_finite(&mut reader, "index of refraction")?,
        )
    } else {
        (Uuid::nil(), [255, 255, 255, 0], [255, 255, 255, 0], 1.0)
    };
    reader.skip_remaining()?;
    let key = if id.is_nil() {
        format!("record-{source_offset}")
    } else {
        id.to_string()
    };
    Ok(MaterialRecord {
        id: format!("rhino:presentation:material#{key}"),
        source_offset: source_offset as u64,
        archive_index: Some(archive_index),
        source_uuid: (!id.is_nil()).then(|| id.to_string()),
        name,
        plugin_uuid: plugin.to_string(),
        ambient,
        diffuse,
        emission,
        specular,
        reflection,
        transparent,
        index_of_refraction,
        reflectivity: 0.0,
        shine,
        transparency,
        texture_count: textures.len(),
        textures,
        shareable: false,
        disable_lighting: false,
        fresnel_reflections: false,
        reflection_glossiness: None,
        refraction_glossiness: None,
        fresnel_index_of_refraction: None,
        rdk_instance_uuid: None,
        diffuse_texture_alpha_transparency: None,
        physically_based,
    })
}

fn parse_material(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    source_offset: usize,
    physically_based: Option<PhysicallyBasedMaterialRecord>,
    losses: &mut Vec<LossNote>,
) -> Result<MaterialRecord, FramingError> {
    if matches!(archive, ArchiveVersion::V2 | ArchiveVersion::V3) {
        return parse_v2_v3_material(data, range, source_offset, physically_based);
    }
    let framed = data.get(range.start).copied() == Some(0);
    let (mut reader, component, minor, modern) = if framed {
        let (mut reader, version) = anonymous(data, range, archive)?;
        if version.0 != 1 || version.1 < 0 {
            return Err(FramingError::structural(
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
            return Err(FramingError::structural(
                range.start,
                "legacy material outer version is unsupported",
            ));
        }
        let chunk = chunk_at(data, outer.position(), outer.end(), archive, false)?;
        let mut reader = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
        let version = (reader.i32()?, reader.i32()?);
        if version.0 != 1 || version.1 < 0 {
            return Err(FramingError::structural(
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
    let mut transparent = reader.array()?;
    // A pre-2009 writer stores a bogus [128, 128, 128] transparent color that
    // the diffuse color replaces. Without a stamp the stored color stands, so
    // the emitted color rests on the missing stamp - but only where the two
    // readings disagree. Where diffuse already equals the stored color, both
    // readings give the same IR and nothing was substituted.
    if !modern && transparent[..3] == [128, 128, 128] {
        if writer_version.is_some_and(|version| version < 200_912_010) {
            transparent = diffuse;
        } else if writer_version.is_none() && diffuse != transparent {
            losses.push(crate::loss::writer_stamp_unverified(format!(
                "legacy material at offset {source_offset} kept its stored transparent color instead of the pre-2009 diffuse substitution because the archive has no writer-version stamp"
            )));
        }
    }
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
        reader.bool_with_writer_version(writer_version)?
    } else {
        false
    };
    let disable_lighting = if minor >= 3 || modern {
        reader.bool_with_writer_version(writer_version)?
    } else {
        false
    };
    let fresnel_reflections = if minor >= 4 || modern {
        reader.bool_with_writer_version(writer_version)?
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
        Some(reader.bool_with_writer_version(writer_version)?)
    } else {
        None
    };
    reader.skip_remaining()?;
    let key = if component.id.is_nil() {
        format!("record-{source_offset}")
    } else {
        component.id.to_string()
    };
    Ok(MaterialRecord {
        id: format!("rhino:presentation:material#{key}"),
        source_offset: source_offset as u64,
        archive_index: component.index,
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
        physically_based,
    })
}

fn parse_group(
    data: &[u8],
    range: Range<usize>,
    source_offset: usize,
) -> Result<GroupRecord, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let packed = reader.u8()?;
    if packed >> 4 != 1 {
        return Err(FramingError::structural(
            range.start,
            "group version is unsupported",
        ));
    }
    let index = reader.i32()?;
    let name = utf16(&mut reader)?;
    let id = if packed & 0x0f >= 1 {
        Some(uuid(&mut reader)?)
    } else {
        None
    };
    reader.skip_remaining()?;
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
    if packed >> 4 != 1 {
        return Err(FramingError::structural(
            range.start,
            "light version is unsupported",
        ));
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
    let spot_angle_degrees = read_finite(&mut reader, "spot angle")?;
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
    reader.skip_remaining()?;
    for vector in [&mut location, &mut length, &mut width] {
        for value in vector {
            *value = scaled_coordinate(*value, scale).ok_or_else(|| {
                FramingError::structural(range.start, "scaled light geometry is invalid")
            })?;
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
        spot_angle_degrees,
        spot_exponent,
        attenuation,
        shadow_intensity,
        length,
        width,
        hotspot,
        attributes: None,
        links: link.into_iter().collect(),
    })
}

fn push_light(
    lights: &mut Vec<LightRecord>,
    indexes: &mut BTreeMap<String, usize>,
    mut light: LightRecord,
) {
    if light.source_uuid != Uuid::nil().to_string() {
        if indexes.contains_key(&light.source_uuid) {
            light.id = format!("{}-offset-{}", light.id, light.source_offset);
        } else {
            indexes.insert(light.source_uuid.clone(), lights.len());
        }
    }
    lights.push(light);
}

fn segments(reader: &mut BoundedReader<'_>) -> Result<Vec<LinetypeSegment>, FramingError> {
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
    let component = if version.0 == 1 && version.1 >= 0 {
        let index = reader.i32()?;
        let name = utf16(&mut reader)?;
        let value = Component {
            index: Some(index),
            id: Uuid::nil(),
            name,
        };
        let values = segments(&mut reader)?;
        let id = if version.1 >= 1 {
            uuid(&mut reader)?
        } else {
            Uuid::nil()
        };
        reader.skip_remaining()?;
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
    } else if version.0 == 2 && version.1 >= 0 {
        component(data, &mut reader, archive)?
    } else {
        return Err(FramingError::structural(
            reader.position(),
            "linetype version is unsupported",
        ));
    };
    let mut values = segments(&mut reader)?;
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
                return Err(FramingError::structural(
                    reader.position(),
                    "linetype taper is not finite",
                ));
            }
            item = reader.u8()?;
        }
    }
    if version.1 >= 3 && item == 6 {
        always = reader.bool()?;
        let _next_item = reader.u8()?;
    }
    if always {
        for segment in &mut values {
            segment.length_millimeters = scaled_coordinate(segment.length_millimeters, scale)
                .ok_or_else(|| {
                    FramingError::structural(
                        reader.position(),
                        "scaled model-distance linetype segment is invalid",
                    )
                })?;
        }
    }
    // The source reader consumes an unknown or out-of-order ID and closes
    // the anonymous chunk. Its value has no generic width and remains a
    // bounded suffix.
    reader.skip_remaining()?;
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
        archive_index: component.index,
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
        return Err(FramingError::structural(
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
        *value = scaled_coordinate(*value, scale).ok_or_else(|| {
            FramingError::structural(reader.position(), "scaled hatch line is invalid")
        })?;
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
    let mut pattern_unit_system = None;
    let mut always_model_distances = None;
    let (component, fill_type, description, lines) = if modern {
        let (mut reader, version) = anonymous(data, range, archive)?;
        if version.0 != 1 || version.1 < 0 {
            return Err(FramingError::structural(
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
        let count = usize::try_from(count).map_err(|_| {
            FramingError::structural(line_reader.position() - 4, "negative hatch-line count")
        })?;
        if count > 1 << 16 {
            return Err(FramingError::structural(
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
            let version = (payload.i32()?, payload.i32()?);
            if version.0 != 1 || version.1 < 0 {
                return Err(FramingError::structural(
                    payload.position(),
                    "hatch-line version is unsupported",
                ));
            }
            lines.push(hatch_line_fields(&mut payload, scale)?);
            payload.skip_remaining()?;
            line_reader.skip(line.next_offset - line_reader.position())?;
        }
        line_reader.skip_remaining()?;
        reader.skip(chunk.next_offset - reader.position())?;
        if archive.value() >= 90 {
            pattern_unit_system = Some(reader.u8()?);
            always_model_distances = Some(reader.bool()?);
        }
        reader.skip_remaining()?;
        (component, fill_type, description, lines)
    } else {
        let mut reader = BoundedReader::new(data, range.start, range.end)?;
        let packed = reader.u8()?;
        if packed >> 4 != 1 {
            return Err(FramingError::structural(
                range.start,
                "legacy hatch-pattern version is unsupported",
            ));
        }
        let index = reader.i32()?;
        let fill_type = reader.i32()?;
        let name = utf16(&mut reader)?;
        let description = utf16(&mut reader)?;
        let count = if fill_type == 1 { reader.i32()? } else { 0 };
        let count = usize::try_from(count).map_err(|_| {
            FramingError::structural(reader.position() - 4, "negative hatch-line count")
        })?;
        if count > 1 << 16 {
            return Err(FramingError::structural(
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
        reader.skip_remaining()?;
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
        archive_index: component.index,
        source_uuid: (!component.id.is_nil()).then(|| component.id.to_string()),
        name: component.name,
        fill_type,
        description,
        lines,
        pattern_unit_system,
        always_model_distances,
    })
}

fn scaled_length(
    reader: &mut BoundedReader<'_>,
    scale: f64,
    label: &str,
) -> Result<f64, FramingError> {
    let value = read_finite(reader, label)?;
    scaled_coordinate(value, scale).ok_or_else(|| {
        FramingError::structural(reader.position() - 8, format!("scaled {label} is invalid"))
    })
}

fn named_child(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<serde_json::Value, FramingError> {
    let offset = reader.position();
    let chunk = chunk_at(data, offset, reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(FramingError::structural(
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
    if minor >= 10 {
        put!("use_kerning", reader.bool()?);
    }
    if minor >= 11 {
        put!("line_space_scale", read_finite(reader, "line-space scale")?);
    }
    reader.skip_remaining()?;
    Ok(values)
}

fn parse_v5_dimension_style_extra(
    data: &[u8],
    extra: &UserdataDescriptor,
    archive: ArchiveVersion,
    scale: f64,
) -> Result<V5DimensionStyleExtraRecord, FramingError> {
    let (mut reader, version) = anonymous(data, extra.payload_range.clone(), archive)?;
    if version.0 != 1 || version.1 < 0 {
        return Err(FramingError::structural(
            reader.position() - 8,
            "V5 dimension-style extra version is unsupported",
        ));
    }
    let parent_style_uuid = uuid(&mut reader)?;
    let count_offset = reader.position();
    let count = reader.i32()?;
    let byte_count = checked_count_bytes(
        count,
        1,
        reader.remaining(),
        MAX_DIMSTYLE_EXTRA_FIELDS,
        count_offset,
    )?;
    let valid_fields = reader
        .take(byte_count)?
        .iter()
        .map(|value| *value != 0)
        .collect();
    let tolerance_style = reader.i32()?;
    let tolerance_resolution = reader.i32()?;
    let tolerance_upper_value = read_finite(&mut reader, "tolerance upper value")?;
    let tolerance_lower_value = read_finite(&mut reader, "tolerance lower value")?;
    let tolerance_height_scale = read_finite(&mut reader, "tolerance height scale")?;
    let baseline_spacing_mm = scaled_length(&mut reader, scale, "baseline spacing")?;
    let (draw_text_mask, mask_color_source, mask_color) = if version.1 >= 1 {
        (reader.bool()?, reader.i32()?, reader.array()?)
    } else {
        (false, 0, [255, 255, 255, 0])
    };
    let (dimension_scale, dimension_scale_source) = if version.1 >= 2 {
        (read_finite(&mut reader, "dimension scale")?, reader.i32()?)
    } else {
        (1.0, 0)
    };
    let source_style_uuid = if version.1 >= 3 {
        uuid(&mut reader)?
    } else {
        Uuid::nil()
    };
    reader.skip_remaining()?;
    Ok(V5DimensionStyleExtraRecord {
        parent_style_uuid: (!parent_style_uuid.is_nil()).then(|| parent_style_uuid.to_string()),
        valid_fields,
        tolerance_style,
        tolerance_resolution,
        tolerance_upper_value,
        tolerance_lower_value,
        tolerance_height_scale,
        baseline_spacing_mm,
        draw_text_mask,
        mask_color_source,
        mask_color,
        dimension_scale,
        dimension_scale_source,
        source_style_uuid: (!source_style_uuid.is_nil()).then(|| source_style_uuid.to_string()),
    })
}

fn parse_v5_dimension_style(
    data: &[u8],
    range: Range<usize>,
    scale: f64,
    source_offset: usize,
    extra: Option<V5DimensionStyleExtraRecord>,
) -> Result<DimensionStyleRecord, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let packed = reader.u8()?;
    let major = packed >> 4;
    let minor = packed & 0x0f;
    if major != 1 {
        return Err(FramingError::structural(
            range.start,
            "V5 dimension-style version is unsupported",
        ));
    }
    let archive_index = reader.i32()?;
    let name = utf16(&mut reader)?;
    let extension_line_extension_mm =
        scaled_length(&mut reader, scale, "extension-line extension")?;
    let extension_line_offset_mm = scaled_length(&mut reader, scale, "extension-line offset")?;
    let arrow_size_mm = scaled_length(&mut reader, scale, "arrow size")?;
    let center_mark_size_mm = scaled_length(&mut reader, scale, "center-mark size")?;
    let text_gap_mm = scaled_length(&mut reader, scale, "text gap")?;
    let text_display_mode = reader.u32()?;
    let arrow_type = reader.i32()?;
    let angular_units = reader.i32()?;
    let length_format = reader.u32()?;
    let angle_format = reader.u32()?;
    let length_resolution = reader.i32()?;
    let angle_resolution = reader.i32()?;
    let text_style_index = reader.i32()?;
    let text_height_mm = if minor >= 1 {
        scaled_length(&mut reader, scale, "text height")?
    } else {
        scale
    };
    let mut controls = BTreeMap::new();
    controls.insert(
        "v5_version".to_string(),
        serde_json::json!({ "major": major, "minor": minor }),
    );
    controls.insert("v5_arrow_type".to_string(), serde_json::json!(arrow_type));
    controls.insert(
        "v5_angular_units".to_string(),
        serde_json::json!(angular_units),
    );
    let (
        length_factor,
        alternate_enabled,
        alternate_length_factor,
        alternate_length_format,
        alternate_length_resolution,
        prefix,
        suffix,
        alternate_prefix,
        alternate_suffix,
    ) = if minor >= 2 {
        let length_factor = read_finite(&mut reader, "length factor")?;
        let prefix = utf16(&mut reader)?;
        let suffix = utf16(&mut reader)?;
        let alternate_enabled = reader.bool()?;
        let alternate_length_factor = read_finite(&mut reader, "alternate length factor")?;
        let alternate_length_format = reader.u32()?;
        let alternate_length_resolution = reader.i32()?;
        let alternate_angle_format = reader.u32()?;
        let alternate_angle_resolution = reader.i32()?;
        let alternate_prefix = utf16(&mut reader)?;
        let alternate_suffix = utf16(&mut reader)?;
        let unused = reader.u32()?;
        controls.insert(
            "v5_length_factor".to_string(),
            serde_json::json!(length_factor),
        );
        controls.insert(
            "v5_alternate_angle_format".to_string(),
            serde_json::json!(alternate_angle_format),
        );
        controls.insert(
            "v5_alternate_angle_resolution".to_string(),
            serde_json::json!(alternate_angle_resolution),
        );
        controls.insert("v5_unused".to_string(), serde_json::json!(unused));
        (
            length_factor,
            alternate_enabled,
            alternate_length_factor,
            alternate_length_format,
            alternate_length_resolution,
            prefix,
            suffix,
            alternate_prefix,
            alternate_suffix,
        )
    } else {
        (
            1.0,
            false,
            1.0,
            0,
            2,
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        )
    };
    let id = if minor >= 3 {
        uuid(&mut reader)?
    } else {
        Uuid::nil()
    };
    let dimension_line_extension_mm = if minor >= 4 {
        scaled_length(&mut reader, scale, "dimension-line extension")?
    } else {
        0.0
    };
    let (
        leader_arrow_size_mm,
        leader_arrow_type,
        suppress_extension_line_1,
        suppress_extension_line_2,
    ) = if minor >= 5 {
        (
            scaled_length(&mut reader, scale, "leader arrow size")?,
            reader.i32()?,
            reader.bool()?,
            reader.bool()?,
        )
    } else {
        (scale, 0, false, false)
    };
    reader.skip_remaining()?;
    controls.insert(
        "v5_leader_arrow_type".to_string(),
        serde_json::json!(leader_arrow_type),
    );
    let parent_style_uuid = extra
        .as_ref()
        .and_then(|value| value.parent_style_uuid.clone());
    if let Some(value) = extra.as_ref() {
        controls.insert(
            "v5_extra_dimension_scale".to_string(),
            serde_json::json!(value.dimension_scale),
        );
        controls.insert(
            "v5_extra_dimension_scale_source".to_string(),
            serde_json::json!(value.dimension_scale_source),
        );
    }
    let key = if id.is_nil() {
        format!("record-{source_offset}")
    } else {
        id.to_string()
    };
    Ok(DimensionStyleRecord {
        id: format!("rhino:presentation:dimension_style#{key}"),
        source_offset: source_offset as u64,
        archive_index: Some(archive_index),
        source_uuid: (!id.is_nil()).then(|| id.to_string()),
        name,
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
        parent_style_uuid,
        controls,
        v5_extra: extra,
    })
}

fn parse_dimension_style(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    scale: f64,
    source_offset: usize,
) -> Result<DimensionStyleRecord, FramingError> {
    let (mut reader, version) = anonymous(data, range, archive)?;
    if version.0 != 1 || version.1 < 0 {
        return Err(FramingError::structural(
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
        archive_index: component.index,
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
        v5_extra: None,
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
        .ok_or_else(|| {
            FramingError::structural(reader.position() - 128, "texture transform is not finite")
        })
}

fn parse_embedded_image(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    source_offset: usize,
) -> Result<EmbeddedImageRecord, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let packed = reader.u8()?;
    if packed >> 4 != 1 {
        return Err(FramingError::structural(
            range.start,
            "embedded-image version is unsupported",
        ));
    }
    let file_path = utf16(&mut reader)?;
    let image_crc32 = reader.u32()?;
    let compression_method = reader.i32()?;
    if !matches!(compression_method, 0 | 1) {
        return Err(FramingError::structural(
            reader.position() - 4,
            "embedded-image compression method is unsupported",
        ));
    }
    let buffer_offset = reader.position();
    let uncompressed_byte_len = u64::from(reader.u32()?);
    match compression_method {
        0 => {
            if uncompressed_byte_len != 0 {
                let size = usize::try_from(uncompressed_byte_len)
                    .map_err(|_| FramingError::structural(buffer_offset, "image size overflow"))?;
                reader.skip(size)?;
            }
        }
        1 => {
            if uncompressed_byte_len != 0 {
                reader.skip(4)?;
                let method = reader.u8()?;
                if method > 1 {
                    return Err(FramingError::structural(
                        reader.position() - 1,
                        "embedded-image buffer method is unsupported",
                    ));
                }
                if method == 0 {
                    let size = usize::try_from(uncompressed_byte_len).map_err(|_| {
                        FramingError::structural(buffer_offset, "image size overflow")
                    })?;
                    reader.skip(size)?;
                } else {
                    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
                    if chunk.typecode != ANONYMOUS || chunk.short {
                        return Err(FramingError::structural(
                            reader.position(),
                            "compressed image chunk is invalid",
                        ));
                    }
                    reader.skip(chunk.next_offset - reader.position())?;
                }
            }
        }
        _ => unreachable!("embedded image compression method checked"),
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
    reader.skip_remaining()?;
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

fn bitmap_buffer(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<(Range<usize>, usize), FramingError> {
    let start = reader.position();
    let declared = reader.u32()?;
    let uncompressed_byte_len = usize::try_from(declared)
        .map_err(|_| FramingError::structural(start, "bitmap buffer size overflows usize"))?;
    if uncompressed_byte_len == 0 {
        return Ok((start..reader.position(), uncompressed_byte_len));
    }
    reader.skip(4)?;
    let method = reader.u8()?;
    match method {
        0 => reader.skip(uncompressed_byte_len)?,
        1 => {
            let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
            if chunk.typecode != ANONYMOUS || chunk.short || chunk.body.is_empty() {
                return Err(FramingError::structural(
                    reader.position(),
                    "Windows bitmap compressed buffer chunk is invalid",
                ));
            }
            reader.skip(chunk.next_offset - reader.position())?;
        }
        _ => {
            return Err(FramingError::structural(
                reader.position() - 1,
                "Windows bitmap compressed buffer method is unsupported",
            ));
        }
    }
    Ok((start..reader.position(), uncompressed_byte_len))
}

fn parse_windows_bitmap(
    data: &[u8],
    range: Range<usize>,
    class_uuid: Uuid,
    archive: ArchiveVersion,
    source_offset: usize,
) -> Result<WindowsBitmapRecord, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let file_path = if class_uuid == WINDOWS_BITMAP_EX {
        if reader.u8()? >> 4 != 1 {
            return Err(FramingError::structural(
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
        return Err(FramingError::structural(
            reader.position(),
            "Windows bitmap header is invalid",
        ));
    }
    let palette_color_count = if colors_used != 0 {
        usize::try_from(colors_used).map_err(|_| {
            FramingError::structural(reader.position(), "Windows bitmap palette count overflows")
        })?
    } else {
        match bits_per_pixel {
            1 => 2,
            4 => 16,
            8 => 256,
            _ => 0,
        }
    };
    let palette_byte_len = palette_color_count.checked_mul(4).ok_or_else(|| {
        FramingError::structural(reader.position(), "Windows bitmap palette overflows")
    })?;
    let image_byte_len = usize::try_from(image_byte_len).map_err(|_| {
        FramingError::structural(reader.position(), "Windows bitmap image overflows")
    })?;
    let pixel_buffer_offset = reader.position();
    let pixel_buffer_end = if archive == ArchiveVersion::V1 && class_uuid == WINDOWS_BITMAP {
        let raw_size = palette_byte_len
            .checked_add(image_byte_len)
            .ok_or_else(|| {
                FramingError::structural(pixel_buffer_offset, "Windows bitmap size overflows")
            })?;
        reader.skip(raw_size)?;
        reader.position()
    } else {
        let (first_buffer, first_size) = bitmap_buffer(data, &mut reader, archive)?;
        let combined_size = palette_byte_len
            .checked_add(image_byte_len)
            .ok_or_else(|| {
                FramingError::structural(first_buffer.start, "Windows bitmap size overflows")
            })?;
        if first_size != combined_size {
            if first_size != palette_byte_len || image_byte_len == 0 {
                return Err(FramingError::structural(
                    first_buffer.start,
                    "Windows bitmap buffer size does not match the header",
                ));
            }
            let (_, second_size) = bitmap_buffer(data, &mut reader, archive)?;
            if second_size != image_byte_len {
                return Err(FramingError::structural(
                    reader.position(),
                    "Windows bitmap image buffer size does not match the header",
                ));
            }
        }
        reader.position()
    };
    reader.skip_remaining()?;
    let buffer = &data[pixel_buffer_offset..pixel_buffer_end];
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
        image_byte_len: image_byte_len as i32,
        pixels_per_meter,
        colors_used,
        important_colors,
        pixel_buffer_offset: pixel_buffer_offset as u64,
        pixel_buffer_byte_len: buffer.len() as u64,
        pixel_buffer_sha256: cadmpeg_ir::hash::sha256_hex(buffer),
    })
}

struct ParsedTextureMapping {
    value: TextureMappingRecord,
    cache_requires_opaque: bool,
}

fn parse_mapping_crc_cache(data: &[u8], payload_range: Range<usize>) -> Result<(), FramingError> {
    let mut reader = BoundedReader::new(data, payload_range.start, payload_range.end)?;
    let version = reader.i32()?;
    if version != 1 {
        return Err(FramingError::structural(
            payload_range.start,
            "MappingCRCCache version is unsupported",
        ));
    }
    let _mapping_crc = reader.i32()?;
    reader.skip_remaining()?;
    Ok(())
}

fn parse_texture_mapping(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    source_offset: usize,
) -> Result<ParsedTextureMapping, FramingError> {
    let (mut reader, version) = anonymous(data, range, archive)?;
    if version.0 != 1 || version.1 < 0 {
        return Err(FramingError::structural(
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
    let (primitive_class_uuid, cache_requires_opaque) = if object.short {
        (None, false)
    } else {
        let mut warnings = Vec::new();
        let (value, userdata) =
            parse_class_wrapper_with_userdata(data, object.range(), archive, &mut warnings)?;
        let cache_requires_opaque = userdata.iter().any(|value| {
            value.class_uuid == MAPPING_CRC_CACHE
                && value.item_uuid == MAPPING_CRC_CACHE
                && parse_mapping_crc_cache(data, value.payload_range.clone()).is_err()
        });
        (Some(value.class_uuid.to_string()), cache_requires_opaque)
    };
    reader.skip(object.next_offset - reader.position())?;
    let texture_space = if version.1 >= 1 { reader.u32()? } else { 0 };
    let capped = version.1 >= 1 && reader.bool()?;
    reader.skip_remaining()?;
    let key = if id.is_nil() {
        format!("record-{source_offset}")
    } else {
        id.to_string()
    };
    Ok(ParsedTextureMapping {
        value: TextureMappingRecord {
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
        },
        cache_requires_opaque,
    })
}

fn parse_rendering_mapping_channel(
    data: &[u8],
    start: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<(RenderingMappingChannel, usize), FramingError> {
    let chunk = chunk_at(data, start, end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(FramingError::structural(
            start,
            "rendering mapping channel is not an anonymous long chunk",
        ));
    }
    let mut value = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    if value.i32()? != 1 {
        return Err(FramingError::structural(
            value.position() - 4,
            "rendering mapping channel version is unsupported",
        ));
    }
    let minor = value.i32()?;
    let mapping_channel_id = value.i32()?;
    let mapping_uuid = uuid(&mut value)?.to_string();
    let object_transform = if minor >= 1 {
        Some(xform(&mut value)?)
    } else {
        None
    };
    value.skip_remaining()?;
    Ok((
        RenderingMappingChannel {
            mapping_channel_id,
            mapping_uuid,
            object_transform,
        },
        chunk.next_offset,
    ))
}

fn rendering_attributes(
    data: &[u8],
    range: Option<Range<usize>>,
    archive: ArchiveVersion,
    kind: settings::RenderingAttributesKind,
) -> Result<RenderingAttributesPresentation, FramingError> {
    let Some(range) = range else {
        return Ok(RenderingAttributesPresentation::default());
    };
    (|| {
        let (mut reader, version) = anonymous(data, range, archive)?;
        if version.0 != 1
            || version.1 < 0
            || (matches!(kind, settings::RenderingAttributesKind::Object) && version.1 < 1)
        {
            return Err(FramingError::structural(
                reader.position(),
                "rendering-attributes version is unsupported",
            ));
        }
        let material_count = checked_count_bytes(
            reader.i32()?,
            1,
            reader.remaining(),
            1 << 16,
            reader.position() - 4,
        )?;
        let mut presentation = RenderingAttributesPresentation::default();
        for _ in 0..material_count {
            let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
            let parsed = (|| {
                let mut value = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
                if value.i32()? != 1 {
                    return Err(FramingError::structural(
                        value.position(),
                        "rendering-material version is unsupported",
                    ));
                }
                let minor = value.i32()?;
                let plugin_uuid = uuid(&mut value)?.to_string();
                let front_material_uuid = uuid(&mut value)?.to_string();
                let obsolete_mapping_count = checked_count_bytes(
                    value.i32()?,
                    1,
                    value.remaining(),
                    1 << 16,
                    value.position() - 4,
                )?;
                for _ in 0..obsolete_mapping_count {
                    let (_, next_offset) = parse_rendering_mapping_channel(
                        data,
                        value.position(),
                        value.end(),
                        archive,
                    )?;
                    value.skip(next_offset - value.position())?;
                }
                let (back_material_uuid, material_source) = if minor >= 1 {
                    let id = uuid(&mut value)?;
                    let source = value.u8()?;
                    value.skip(3)?;
                    ((!id.is_nil()).then(|| id.to_string()), Some(source))
                } else {
                    (None, None)
                };
                value.skip_remaining()?;
                Ok(RenderingMaterialReference {
                    plugin_uuid,
                    front_material_uuid,
                    back_material_uuid,
                    material_source,
                })
            })();
            presentation.materials.push(parsed?);
            reader.skip(chunk.next_offset - reader.position())?;
        }
        if matches!(kind, settings::RenderingAttributesKind::Object) {
            let mapping_count = checked_count_bytes(
                reader.i32()?,
                1,
                reader.remaining(),
                1 << 16,
                reader.position() - 4,
            )?;
            for _ in 0..mapping_count {
                let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
                let mut value = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
                if value.i32()? != 1 {
                    return Err(FramingError::structural(
                        value.position() - 4,
                        "rendering mapping version is unsupported",
                    ));
                }
                let _minor = value.i32()?;
                let plugin_uuid = uuid(&mut value)?.to_string();
                let channel_count = checked_count_bytes(
                    value.i32()?,
                    1,
                    value.remaining(),
                    1 << 16,
                    value.position() - 4,
                )?;
                let mut channels = Vec::with_capacity(channel_count);
                for _ in 0..channel_count {
                    let (channel, next_offset) = parse_rendering_mapping_channel(
                        data,
                        value.position(),
                        value.end(),
                        archive,
                    )?;
                    channels.push(channel);
                    value.skip(next_offset - value.position())?;
                }
                value.skip_remaining()?;
                presentation.mappings.push(RenderingMappingReference {
                    plugin_uuid,
                    channels,
                });
                reader.skip(chunk.next_offset - reader.position())?;
            }
        }
        if matches!(kind, settings::RenderingAttributesKind::Object) && version.1 >= 2 {
            if !reader.bool()? {
                presentation.casts_shadows = Some(false);
            }
            if !reader.bool()? {
                presentation.receives_shadows = Some(false);
            }
        }
        if matches!(kind, settings::RenderingAttributesKind::Object)
            && version.1 >= 3
            && reader.bool()?
        {
            presentation.advanced_texture_preview = Some(true);
        }
        reader.skip_remaining()?;
        Ok(presentation)
    })()
}

fn parse_font(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
) -> Result<FontRecord, FramingError> {
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(FramingError::structural(
            reader.position(),
            "font wrapper is invalid",
        ));
    }
    let mut value = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    let (major, minor) = (value.i32()?, value.i32()?);
    if major != 1 || minor < 0 {
        return Err(FramingError::structural(
            value.position(),
            "font version is unsupported",
        ));
    }
    let mut font = FontRecord {
        characteristics: value.u32()?,
        windows_logfont_name: wide_string(data, &mut value, archive)?,
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
        if value.bool_with_writer_version(writer_version)? {
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
            return Err(FramingError::structural(
                value.position(),
                "font PANOSE wrapper is invalid",
            ));
        }
        let mut bytes = BoundedReader::new(data, panose.body.start, panose.body.end)?;
        if bytes.u8()? != 0x10 || bytes.remaining() != 10 {
            return Err(FramingError::structural(
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
    value.skip_remaining()?;
    reader.skip(chunk.next_offset - reader.position())?;
    Ok(font)
}

fn parse_text_style(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    apple_runtime: bool,
    source_offset: usize,
    losses: &mut Vec<LossNote>,
) -> Result<TextStyleRecord, FramingError> {
    if data.get(range.start).copied() != Some(0) {
        let mut reader = BoundedReader::new(data, range.start, range.end)?;
        let packed = reader.u8()?;
        if packed >> 4 != 1 {
            return Err(FramingError::structural(
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
        let named_description =
            !description.is_empty() && !description.eq_ignore_ascii_case("Default");
        let postscript_name = if named_description
            && (apple_runtime || writer_version.is_some_and(|version| version > 201_802_230))
        {
            description.clone()
        } else {
            if named_description && !apple_runtime && writer_version.is_none() {
                losses.push(crate::loss::writer_stamp_unverified(format!(
                    "legacy text style at offset {source_offset} dropped the PostScript font name \"{description}\" because the archive has no writer-version stamp"
                )));
            }
            String::new()
        };
        let mut font = FontRecord {
            windows_logfont_name,
            postscript_name,
            obsolete_description: description.clone(),
            ..FontRecord::default()
        };
        if packed & 0x0f >= 1 {
            font.windows_logfont_weight = Some(reader.i32()?);
            let italic = reader.i32()?;
            if !matches!(italic, 0 | 1) {
                return Err(FramingError::structural(
                    reader.position() - 4,
                    "legacy font italic flag is invalid",
                ));
            }
            let _linefeed_ratio = read_finite(&mut reader, "legacy font linefeed ratio")?;
            font.legacy_italic = Some(italic != 0);
        }
        let id = if packed & 0x0f >= 2 {
            uuid(&mut reader)?
        } else {
            Uuid::nil()
        };
        reader.skip_remaining()?;
        return Ok(TextStyleRecord {
            id: format!("rhino:presentation:text_style#index-{index}-offset-{source_offset}"),
            source_offset: source_offset as u64,
            archive_index: Some(index),
            source_uuid: (!id.is_nil()).then(|| id.to_string()),
            name: description.clone(),
            font_description: description,
            font,
        });
    }

    let (mut reader, version) = anonymous(data, range, archive)?;
    if version.0 != 1 || version.1 < 0 {
        return Err(FramingError::structural(
            reader.position(),
            "text-style version is unsupported",
        ));
    }
    let component = component(data, &mut reader, archive)?;
    let font_description = if reader.bool_with_writer_version(writer_version)? {
        utf16(&mut reader)?
    } else {
        String::new()
    };
    let font = if reader.bool_with_writer_version(writer_version)? {
        parse_font(data, &mut reader, archive, writer_version)?
    } else {
        FontRecord::default()
    };
    let (id, name) = if version.1 >= 1 {
        (uuid(&mut reader)?, utf16(&mut reader)?)
    } else {
        (component.id, component.name)
    };
    reader.skip_remaining()?;
    let index = component.index;
    Ok(TextStyleRecord {
        id: if id.is_nil() {
            index.map_or_else(
                || format!("rhino:presentation:text_style#offset-{source_offset}"),
                |index| {
                    format!("rhino:presentation:text_style#index-{index}-offset-{source_offset}")
                },
            )
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

/// Results of transferring table-owned presentation records.
pub(crate) struct PresentationInstall {
    /// Losses from records that could not be transferred.
    pub(crate) losses: Vec<LossNote>,
    /// Complete records whose registered class payload was not admitted.
    pub(crate) opaque_records: Vec<OpaqueRecord>,
}

pub(crate) fn install(scan: &Scan<'_>, ir: &mut CadIr) -> PresentationInstall {
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
    let mut losses = Vec::new();
    let mut opaque_records = Vec::new();
    for object in &scan.objects {
        if let Some(identity) = &object.identity {
            *object_id_counts.entry(identity.object_id).or_default() += 1;
        }
    }
    for table in &scan.tables {
        let table_type = table.typecode & !0x0000_8000;
        for record in &table.records {
            let recognized = matches!(
                table_type,
                GROUP_TABLE
                    | MATERIAL_TABLE
                    | LIGHT_TABLE
                    | LINETYPE_TABLE
                    | HATCH_PATTERN_TABLE
                    | DIMSTYLE_TABLE
                    | BITMAP_TABLE
                    | TEXTURE_MAPPING_TABLE
                    | FONT_TABLE
            );
            let mut parsed = false;
            if table_type == GROUP_TABLE {
                if let Ok(range) = class_data(scan.data, record, scan.archive, GROUP) {
                    if let Ok(group) = parse_group(scan.data, range, record.range.start) {
                        groups.push(group);
                        parsed = true;
                    }
                }
            } else if table_type == MATERIAL_TABLE {
                if let Ok((range, userdata)) =
                    class_data_with_userdata(scan.data, record, scan.archive, MATERIAL)
                {
                    let mut material_requires_opaque = false;
                    let legacy_rdk_instance_id =
                        legacy_rdk_material_instance_id(scan.data, &userdata);
                    if rdk_material_userdata_requires_opaque(scan.data, &userdata) {
                        material_requires_opaque = true;
                        losses.push(RhinoLossCode::PresentationRecordDropped.note(format!(
                            "RDK material userdata at offset {} could not be transferred: callback-owned or unsupported payload",
                            record.range.start
                        )));
                    }
                    let physically_based = userdata
                        .iter()
                        .find(|value| {
                            value.class_uuid == PHYSICALLY_BASED_MATERIAL_USERDATA
                                && value.item_uuid == PHYSICALLY_BASED_MATERIAL_USERDATA
                                && (value.application_uuid.is_none()
                                    || value.application_uuid == Some(OPENNURBS6_APPLICATION))
                        })
                        .and_then(|value| {
                            match parse_physically_based_material(
                                scan.data,
                                value.payload_range.clone(),
                                scan.archive,
                            ) {
                                Ok(material) => Some(material),
                                Err(error) => {
                                    material_requires_opaque = true;
                                    losses.push(RhinoLossCode::PresentationRecordDropped.note(
                                        format!(
                                            "physically based material userdata at offset {} could not be transferred: {error}",
                                            record.range.start
                                        ),
                                    ));
                                    None
                                }
                            }
                        });
                    if let Ok(mut material) = parse_material(
                        scan.data,
                        range,
                        scan.archive,
                        scan.metadata.properties.writer_version,
                        record.range.start,
                        physically_based,
                        &mut losses,
                    ) {
                        if let Some(instance_id) = legacy_rdk_instance_id {
                            material.plugin_uuid = UNIVERSAL_RENDER_ENGINE.to_string();
                            material.rdk_instance_uuid = Some(instance_id.to_string());
                        }
                        materials.push(material);
                        if material_requires_opaque {
                            opaque_records.push(OpaqueRecord {
                                table_typecode: table.typecode,
                                record: record.clone(),
                            });
                        }
                        parsed = true;
                    }
                }
            } else if table_type == LIGHT_TABLE {
                if let Ok(range) = class_data_prefix(scan.data, record, scan.archive, LIGHT) {
                    if let Ok(mut light) =
                        parse_light(scan.data, range, scale, record.range.start, None)
                    {
                        match parse_light_record_attributes(
                            scan.data,
                            record,
                            scan.archive,
                            scan.metadata.properties.writer_version,
                            &mut losses,
                        ) {
                            Ok(attributes) => {
                                if let Some(value) = attributes {
                                    if value.userdata_requires_opaque {
                                        opaque_records.push(OpaqueRecord {
                                            table_typecode: table.typecode,
                                            record: record.clone(),
                                        });
                                    }
                                    light.attributes = Some(value);
                                }
                            }
                            Err(error) => {
                                losses.push(RhinoLossCode::ObjectAttributesDegraded.note(
                                    format!(
                                        "light attributes at offset {} could not be transferred: {error}",
                                        record.range.start
                                    ),
                                ));
                                opaque_records.push(OpaqueRecord {
                                    table_typecode: table.typecode,
                                    record: record.clone(),
                                });
                            }
                        }
                        push_light(&mut lights, &mut light_indexes, light);
                        parsed = true;
                    }
                }
            } else if table_type == LINETYPE_TABLE {
                if let Ok(range) = class_data(scan.data, record, scan.archive, LINETYPE) {
                    if let Ok(value) =
                        parse_linetype(scan.data, range, scan.archive, scale, record.range.start)
                    {
                        linetypes.push(value);
                        parsed = true;
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
                        parsed = true;
                    }
                }
            } else if table_type == DIMSTYLE_TABLE {
                if scan.archive.value() < 60 {
                    let mut extra_requires_opaque = false;
                    if let Ok((range, userdata)) =
                        class_data_with_userdata(scan.data, record, scan.archive, V5_DIMSTYLE)
                    {
                        let extra = userdata.into_iter().find(|value| {
                            value.class_uuid == DIMSTYLE_EXTRA && value.item_uuid == DIMSTYLE_EXTRA
                        });
                        let extra = match extra {
                            Some(value) => match parse_v5_dimension_style_extra(
                                scan.data,
                                &value,
                                scan.archive,
                                scale,
                            ) {
                                Ok(extra) => Some(extra),
                                Err(error) => {
                                    extra_requires_opaque = true;
                                    losses.push(RhinoLossCode::PresentationRecordDropped.note(
                                        format!(
                                            "V5 dimension-style userdata at offset {} could not be transferred: {error}",
                                            record.range.start
                                        ),
                                    ));
                                    None
                                }
                            },
                            None => None,
                        };
                        if let Ok(value) = parse_v5_dimension_style(
                            scan.data,
                            range,
                            scale,
                            record.range.start,
                            extra,
                        ) {
                            dimension_styles.push(value);
                            if extra_requires_opaque {
                                opaque_records.push(OpaqueRecord {
                                    table_typecode: table.typecode,
                                    record: record.clone(),
                                });
                            }
                            parsed = true;
                        }
                    }
                } else if let Ok(range) = class_data(scan.data, record, scan.archive, DIMSTYLE) {
                    if let Ok(value) = parse_dimension_style(
                        scan.data,
                        range,
                        scan.archive,
                        scale,
                        record.range.start,
                    ) {
                        dimension_styles.push(value);
                        parsed = true;
                    }
                }
            } else if table_type == BITMAP_TABLE {
                if let Ok(range) = class_data(scan.data, record, scan.archive, EMBEDDED_BITMAP) {
                    if let Ok(value) =
                        parse_embedded_image(scan.data, range, scan.archive, record.range.start)
                    {
                        images.push(value);
                        parsed = true;
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
                            scan.archive,
                            record.range.start,
                        ) {
                            windows_bitmaps.push(value);
                            parsed = true;
                        }
                    }
                }
            } else if table_type == TEXTURE_MAPPING_TABLE {
                if let Ok(range) = class_data(scan.data, record, scan.archive, TEXTURE_MAPPING) {
                    if let Ok(value) =
                        parse_texture_mapping(scan.data, range, scan.archive, record.range.start)
                    {
                        texture_mappings.push(value.value);
                        if value.cache_requires_opaque {
                            losses.push(RhinoLossCode::PresentationRecordDropped.note(format!(
                                "MappingCRCCache userdata at offset {} could not be transferred",
                                record.range.start
                            )));
                            opaque_records.push(OpaqueRecord {
                                table_typecode: table.typecode,
                                record: record.clone(),
                            });
                        }
                        parsed = true;
                    }
                }
            } else if table_type == FONT_TABLE {
                if let Ok(range) = class_data(scan.data, record, scan.archive, TEXT_STYLE) {
                    if let Ok(value) =
                        parse_text_style(
                            scan.data,
                            range,
                            scan.archive,
                            scan.metadata.properties.writer_version,
                            scan.metadata.properties.application.as_ref().is_some_and(
                                |application| application.name.to_ascii_lowercase().contains("mac"),
                            ),
                            record.range.start,
                            &mut losses,
                        )
                    {
                        text_styles.push(value);
                        parsed = true;
                    }
                }
            }
            if recognized && !parsed {
                losses.push(RhinoLossCode::PresentationRecordDropped.note(format!(
                    "record at offset {} in table {table_type:#x} could not be transferred",
                    record.range.start
                )));
                opaque_records.push(OpaqueRecord {
                    table_typecode: table.typecode,
                    record: record.clone(),
                });
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
            match parse_light(
                scan.data,
                object.class_data_range.clone(),
                scale,
                object.range.start,
                Some(link),
            ) {
                Ok(light) => push_light(&mut lights, &mut light_indexes, light),
                Err(error) => losses.push(RhinoLossCode::PresentationRecordDropped.note(format!(
                    "light object at offset {} could not be transferred: {error}",
                    object.range.start
                ))),
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
            let attributes_presentation = object_attributes_presentation(
                scan.data,
                attributes,
                &object.userdata,
                &object.attributes_userdata,
                scan.archive,
                object.range.start,
                identity.object_id.to_string(),
                &mut losses,
            );
            object_presentation.push(ObjectPresentationRecord {
                id: format!("rhino:presentation:object#{key}"),
                source_offset: object.range.start as u64,
                attributes: attributes_presentation,
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
        let rendering = rendering_attributes(
            scan.data,
            layer.rendering_range.clone(),
            scan.archive,
            settings::RenderingAttributesKind::Layer,
        )
        .unwrap_or_else(|error| {
            losses.push(RhinoLossCode::PresentationRecordDropped.note(format!(
                "layer rendering attributes at offset {} could not be transferred: {error}",
                layer.source.range.start
            )));
            RenderingAttributesPresentation::default()
        });
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
            description: layer.description.clone(),
            iges_level: (layer.iges_level != -1).then_some(layer.iges_level),
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
            visible_in_new_details: layer.visible_in_new_details,
            rendering_materials: rendering.materials,
            per_viewport_settings: layer
                .per_viewport_settings
                .iter()
                .map(|settings| LayerPerViewportPresentationRecord {
                    viewport_uuid: settings.viewport_id.to_string(),
                    settings_mask: settings.settings_mask,
                    color: settings.color,
                    plot_color: settings.plot_color,
                    plot_weight_mm: settings.plot_weight_mm,
                    visible: settings.visible,
                    persistent_visibility: settings.persistent_visibility,
                })
                .collect(),
        });
    }
    let mut group_index_counts = BTreeMap::<i32, usize>::new();
    for group in &groups {
        *group_index_counts.entry(group.archive_index).or_default() += 1;
    }
    for (index, count) in &group_index_counts {
        if *count > 1 {
            losses.push(RhinoLossCode::PresentationRecordDropped.note(format!(
                "group index {index} occurs {count} times; ambiguous member links were dropped"
            )));
        }
    }
    for group in &mut groups {
        group.links = if group_index_counts.get(&group.archive_index) == Some(&1) {
            group_members
                .remove(&group.archive_index)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        group.links.sort();
    }
    let namespace = ir.native.namespace_mut("rhino");
    namespace.ensure_version_at_least(std::num::NonZeroU32::new(2).unwrap());
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
    PresentationInstall {
        losses,
        opaque_records,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::ArchiveVersion;
    use std::io::Write;

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

    fn anonymous_body(body: &[u8]) -> Vec<u8> {
        let mut payload = body.to_vec();
        payload.extend(crc32fast::hash(&payload).to_le_bytes());
        let mut bytes = 0x4000_8000_u32.to_le_bytes().to_vec();
        bytes.extend((payload.len() as i64).to_le_bytes());
        bytes.extend(payload);
        bytes
    }

    fn physically_based_payload(version: i32, suffix: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        for value in [0.1_f32, 0.2, 0.3, 0.4] {
            body.extend(value.to_le_bytes());
        }
        body.extend(1_i32.to_le_bytes());
        body.extend(0.5_f64.to_le_bytes());
        for value in [0.6_f32, 0.7, 0.8, 0.9] {
            body.extend(value.to_le_bytes());
        }
        for value in 1..=14 {
            body.extend((value as f64).to_le_bytes());
        }
        for value in [0.11_f32, 0.22, 0.33, 0.44] {
            body.extend(value.to_le_bytes());
        }
        if version >= 2 {
            body.extend(0.77_f64.to_le_bytes());
        }
        body.extend(suffix);
        let inner = anonymous(version, &body);
        let mut payload = inner.clone();
        payload.extend(crc32fast::hash(&inner).to_le_bytes());
        let mut bytes = 0x4000_8000_u32.to_le_bytes().to_vec();
        bytes.extend((payload.len() as i64).to_le_bytes());
        bytes.extend(payload);
        bytes
    }

    fn wide_string_chunk(value: &str) -> Vec<u8> {
        let mut payload = vec![1];
        payload.extend(value.as_bytes());
        payload.extend(crc32fast::hash(&payload).to_le_bytes());
        let mut bytes = 0x4000_8001_u32.to_le_bytes().to_vec();
        bytes.extend((payload.len() as i64).to_le_bytes());
        bytes.extend(payload);
        bytes
    }

    fn panose_chunk() -> Vec<u8> {
        let mut payload = vec![0x10];
        payload.extend([2, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        payload.extend(crc32fast::hash(&payload).to_le_bytes());
        let mut bytes = 0x4000_8000_u32.to_le_bytes().to_vec();
        bytes.extend((payload.len() as i64).to_le_bytes());
        bytes.extend(payload);
        bytes
    }

    fn modern_font_chunk(minor: i32, suffix: &[u8]) -> Vec<u8> {
        let mut body = 0x1234_5678_u32.to_le_bytes().to_vec();
        body.extend(wide_string_chunk("Arial"));
        body.extend(utf16("ArialMT"));
        body.extend(utf16("Arial Regular"));
        body.extend(400_i32.to_le_bytes());
        body.extend(0.5_f64.to_le_bytes());
        body.extend(12.0_f64.to_le_bytes());
        body.push(0);
        body.extend(utf16("Arial"));
        for value in [
            "en-US", "ArialMT", "ArialMT", "Arial", "Arial", "Arial", "Arial", "Regular", "Regular",
        ] {
            body.extend(utf16(value));
        }
        body.extend(panose_chunk());
        body.push(2);
        body.extend(suffix);
        anonymous(minor, &body)
    }

    fn model_attributes_chunk(index: i32, name: &str) -> Vec<u8> {
        let mut payload = 1_i32.to_le_bytes().to_vec();
        payload.extend(0_i32.to_le_bytes());
        payload.extend([0, 2, 0, 1]);
        payload.extend(index.to_le_bytes());
        payload.push(1);
        payload.extend(utf16(name));
        payload.extend(crc32fast::hash(&payload).to_le_bytes());
        let mut bytes = MODEL_ATTRIBUTES.to_le_bytes().to_vec();
        bytes.extend((payload.len() as i64).to_le_bytes());
        bytes.extend(payload);
        bytes
    }

    fn model_attributes_status_chunk(statuses: [u8; 5], name: &str, suffix: &[u8]) -> Vec<u8> {
        let mut payload = 1_i32.to_le_bytes().to_vec();
        payload.extend(0_i32.to_le_bytes());
        payload.extend(statuses);
        if statuses[0] == 1 {
            payload.extend([1_u32, 2, 3].into_iter().flat_map(u32::to_le_bytes));
        }
        if statuses[1] == 1 {
            payload.extend([0x22; 16]);
        }
        if statuses[2] == 1 {
            payload.extend(4_u32.to_le_bytes());
        }
        if statuses[3] == 1 {
            payload.extend(5_i32.to_le_bytes());
        }
        if statuses[4] == 1 {
            payload.extend(utf16(name));
        }
        payload.extend(suffix);
        payload.extend(crc32fast::hash(&payload).to_le_bytes());
        let mut bytes = MODEL_ATTRIBUTES.to_le_bytes().to_vec();
        bytes.extend((payload.len() as i64).to_le_bytes());
        bytes.extend(payload);
        bytes
    }

    fn dimension_style_chunk(minor: i32) -> Vec<u8> {
        let mut body = model_attributes_chunk(7, "dimension style");
        for value in [1.0_f64, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0] {
            body.extend(value.to_le_bytes());
        }
        body.extend(1_u32.to_le_bytes());
        body.extend(2_u32.to_le_bytes());
        body.extend(3_u32.to_le_bytes());
        body.extend(4_i32.to_le_bytes());
        body.extend(5_i32.to_le_bytes());
        body.extend((-1_i32).to_le_bytes());
        body.extend(1.0_f64.to_le_bytes());
        body.push(1);
        body.extend(1.5_f64.to_le_bytes());
        body.extend(6_u32.to_le_bytes());
        body.extend(7_i32.to_le_bytes());
        for value in ["<", ">", "[", "]"] {
            body.extend(utf16(value));
        }
        body.extend(8.0_f64.to_le_bytes());
        body.extend([0, 1]);
        body.extend([0x11; 16]);
        body.extend(9_u32.to_le_bytes());
        body.push(0);
        body.extend(10_u32.to_le_bytes());
        body.extend(11_i32.to_le_bytes());
        for value in [12.0_f64, 13.0, 14.0] {
            body.extend(value.to_le_bytes());
        }
        body.extend(15.0_f64.to_le_bytes());
        body.push(1);
        body.extend(16_u32.to_le_bytes());
        body.extend([17, 18, 19, 20]);
        body.extend(21.0_f64.to_le_bytes());
        body.extend(22_i32.to_le_bytes());
        body.extend([0x22; 16]);
        body.extend([23, 24, 25, 26]);
        for color in [
            [27, 28, 29, 30],
            [31, 32, 33, 34],
            [35, 36, 37, 38],
            [39, 40, 41, 42],
        ] {
            body.extend(color);
        }
        body.extend([43, 44, 45, 46]);
        for color in [
            [47, 48, 49, 50],
            [51, 52, 53, 54],
            [55, 56, 57, 58],
            [59, 60, 61, 62],
        ] {
            body.extend(color);
        }
        body.extend([63, 64]);
        for value in [65.0_f64, 66.0, 67.0] {
            body.extend(value.to_le_bytes());
        }
        body.push(1);
        body.extend(68.0_f64.to_le_bytes());
        body.extend(69_i32.to_le_bytes());
        body.extend(70.0_f64.to_le_bytes());
        body.extend([0, 1]);
        body.extend(71_i32.to_le_bytes());
        body.extend(72_i32.to_le_bytes());
        body.extend(73.0_f64.to_le_bytes());
        body.extend(74_u32.to_le_bytes());
        for value in [75.0_f64, 76.0, 77.0] {
            body.extend(value.to_le_bytes());
        }
        body.extend([78_u32, 79, 80, 81].into_iter().flat_map(u32::to_le_bytes));
        body.push(1);
        body.extend([82_u32, 83, 84].into_iter().flat_map(u32::to_le_bytes));
        body.extend([0x66; 16]);
        body.extend([0x77; 16]);
        body.extend([0x88; 16]);

        body.extend(
            [85_u32, 86, 87, 88, 89]
                .into_iter()
                .flat_map(u32::to_le_bytes),
        );
        body.extend(90.0_f64.to_le_bytes());
        body.push(1);
        body.extend(91.0_f64.to_le_bytes());
        body.extend([90_u32, 91].into_iter().flat_map(u32::to_le_bytes));
        body.push(1);
        body.push(0);
        body.extend(anonymous(0, &[]));
        body.extend(92_u32.to_le_bytes());
        body.extend(anonymous(0, &[]));
        body.extend(anonymous(0, &[]));
        for value in 93_u32..105 {
            body.extend(value.to_le_bytes());
        }
        body.push(1);
        body.extend(
            [105_u32, 106, 107, 108, 109]
                .into_iter()
                .flat_map(u32::to_le_bytes),
        );
        body.extend(110_u32.to_le_bytes());
        body.extend(111_u32.to_le_bytes());
        body.push(1);
        body.extend(112_u32.to_le_bytes());
        if minor >= 10 {
            body.push(1);
        }
        if minor >= 11 {
            body.extend(1.75_f64.to_le_bytes());
        }
        body.extend([0xaa, 0xbb]);
        anonymous(minor, &body)
    }

    fn future_dimension_style_chunk() -> Vec<u8> {
        dimension_style_chunk(12)
    }

    fn current_dimension_style_chunk() -> Vec<u8> {
        dimension_style_chunk(11)
    }

    fn v5_dimension_style_chunk() -> Vec<u8> {
        let mut bytes = vec![0x15];
        bytes.extend(7_i32.to_le_bytes());
        bytes.extend(utf16("legacy dimension style"));
        for value in [1.0_f64, 2.0, 3.0, 4.0, 5.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(6_u32.to_le_bytes());
        bytes.extend(7_i32.to_le_bytes());
        bytes.extend(8_i32.to_le_bytes());
        bytes.extend(9_u32.to_le_bytes());
        bytes.extend(10_u32.to_le_bytes());
        bytes.extend(11_i32.to_le_bytes());
        bytes.extend(12_i32.to_le_bytes());
        bytes.extend(13_i32.to_le_bytes());
        bytes.extend(14.0_f64.to_le_bytes());
        bytes.extend(15.0_f64.to_le_bytes());
        bytes.extend(utf16("<"));
        bytes.extend(utf16(">"));
        bytes.push(1);
        bytes.extend(16.0_f64.to_le_bytes());
        bytes.extend(17_u32.to_le_bytes());
        bytes.extend(18_i32.to_le_bytes());
        bytes.extend(19_u32.to_le_bytes());
        bytes.extend(20_i32.to_le_bytes());
        bytes.extend(utf16("["));
        bytes.extend(utf16("]"));
        bytes.extend(21_u32.to_le_bytes());
        bytes.extend([0x33; 16]);
        bytes.extend(22.0_f64.to_le_bytes());
        bytes.extend(23.0_f64.to_le_bytes());
        bytes.extend(24_i32.to_le_bytes());
        bytes.extend([1, 0]);
        bytes.extend([0xaa, 0xbb]);
        bytes
    }

    fn v5_dimension_style_extra_chunk() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend([0x11; 16]);
        body.extend(3_i32.to_le_bytes());
        body.extend([0, 1, 2]);
        body.extend(3_i32.to_le_bytes());
        body.extend(4_i32.to_le_bytes());
        body.extend(0.25_f64.to_le_bytes());
        body.extend((-0.125_f64).to_le_bytes());
        body.extend(1.25_f64.to_le_bytes());
        body.extend(2.5_f64.to_le_bytes());
        body.push(1);
        body.extend(2_i32.to_le_bytes());
        body.extend([11, 22, 33, 44]);
        body.extend(1.75_f64.to_le_bytes());
        body.extend(2_i32.to_le_bytes());
        body.extend([0x22; 16]);
        body.extend([0xcc, 0xdd]);
        anonymous(3, &body)
    }

    fn embedded_bitmap_payload(minor: u8, id: Uuid, compression_method: i32) -> Vec<u8> {
        let mut bytes = vec![0x10 | minor];
        bytes.extend(utf16("image.png"));
        bytes.extend(0x1122_3344_u32.to_le_bytes());
        bytes.extend(compression_method.to_le_bytes());
        if compression_method == 0 {
            bytes.extend(3_u32.to_le_bytes());
            bytes.extend([0x11, 0x22, 0x33]);
        } else {
            bytes.extend(0_u32.to_le_bytes());
        }
        if minor >= 1 {
            bytes.extend(id.to_wire());
            bytes.extend(utf16("preview"));
        }
        bytes.extend([0xaa, 0xbb]);
        bytes
    }

    fn bitmap_header(
        width: i32,
        height: i32,
        bits_per_pixel: u16,
        image_byte_len: i32,
        colors_used: i32,
    ) -> Vec<u8> {
        let mut bytes = 40_i32.to_le_bytes().to_vec();
        bytes.extend(width.to_le_bytes());
        bytes.extend(height.to_le_bytes());
        bytes.extend(1_u16.to_le_bytes());
        bytes.extend(bits_per_pixel.to_le_bytes());
        bytes.extend(0_i32.to_le_bytes());
        bytes.extend(image_byte_len.to_le_bytes());
        bytes.extend(0_i32.to_le_bytes());
        bytes.extend(0_i32.to_le_bytes());
        bytes.extend(colors_used.to_le_bytes());
        bytes.extend(0_i32.to_le_bytes());
        bytes
    }

    fn stored_bitmap_buffer(bytes: &[u8]) -> Vec<u8> {
        let mut buffer = (bytes.len() as u32).to_le_bytes().to_vec();
        if !bytes.is_empty() {
            buffer.extend(crc32fast::hash(bytes).to_le_bytes());
            buffer.push(0);
            buffer.extend(bytes);
        }
        buffer
    }

    fn compressed_bitmap_buffer(bytes: &[u8]) -> Vec<u8> {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).expect("bitmap zlib input");
        let compressed = encoder.finish().expect("bitmap zlib output");
        let mut buffer = (bytes.len() as u32).to_le_bytes().to_vec();
        buffer.extend(crc32fast::hash(bytes).to_le_bytes());
        buffer.push(1);
        buffer.extend(crate::test_support::test_dump::crc_chunk(
            ArchiveVersion::V8,
            ANONYMOUS,
            &compressed,
        ));
        buffer
    }

    fn windows_bitmap_payload(
        class_uuid: Uuid,
        minor: u8,
        path: &str,
        header: Vec<u8>,
        buffers: &[Vec<u8>],
        suffix: &[u8],
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        if class_uuid == WINDOWS_BITMAP_EX {
            bytes.push(0x10 | minor);
            bytes.extend(utf16(path));
        }
        bytes.extend(header);
        for buffer in buffers {
            bytes.extend(buffer);
        }
        bytes.extend(suffix);
        bytes
    }

    #[test]
    fn light_table_class_data_stops_before_record_children() {
        let archive = ArchiveVersion::V5;
        let payload = [0x12, 0xaa, 0xbb];
        let mut body =
            crate::test_support::test_dump::class_wrapper(archive, LIGHT.to_wire(), &payload);
        body.extend(crate::test_support::test_dump::crc_chunk(
            archive,
            0x0200_8061,
            &[],
        ));
        body.extend(crate::test_support::test_dump::short_chunk(
            archive,
            0x8200_006f,
            0,
        ));
        let record = Record {
            typecode: 0x2000_8060,
            range: 0..body.len(),
            body: 0..body.len(),
            short: false,
            value: 0,
        };

        let range = class_data_prefix(&body, &record, archive, LIGHT).expect("light class");
        assert_eq!(&body[range], payload);
    }

    #[test]
    fn light_record_attributes_use_the_object_attribute_projection() {
        let archive = ArchiveVersion::V5;
        let mut attributes = vec![0x20];
        attributes.extend([0; 16]);
        attributes.extend(7_i32.to_le_bytes());
        attributes.push(1);
        attributes.extend(utf16("table light"));
        attributes.push(11);
        attributes.push(0);
        attributes.push(0);
        let mut body =
            crate::test_support::test_dump::class_wrapper(archive, LIGHT.to_wire(), &[0x12, 0xaa]);
        body.extend(crate::test_support::test_dump::crc_chunk(
            archive,
            LIGHT_RECORD_ATTRIBUTES,
            &attributes,
        ));
        let user_string_body = [
            1_i32.to_le_bytes().as_slice(),
            crate::test_support::test_dump::anonymous_chunk(
                archive,
                0,
                &[utf16("attribute key"), utf16("attribute value")].concat(),
            )
            .as_slice(),
        ]
        .concat();
        let userdata = crate::test_support::test_dump::class_userdata_with_payload(
            archive,
            USER_STRING_LIST.to_wire(),
            [0; 16],
            &user_string_body,
        );
        body.extend(crate::test_support::test_dump::long_chunk(
            archive,
            LIGHT_RECORD_ATTRIBUTES_USERDATA,
            &[
                userdata,
                crate::test_support::test_dump::short_chunk(archive, 0x8002_7fff, 0),
            ]
            .concat(),
        ));
        body.extend(crate::test_support::test_dump::short_chunk(
            archive,
            LIGHT_RECORD_END,
            0,
        ));
        let record = Record {
            typecode: 0x2000_8060,
            range: 0..body.len(),
            body: 0..body.len(),
            short: false,
            value: 0,
        };
        let mut losses = Vec::new();
        let value = parse_light_record_attributes(
            &body,
            &record,
            archive,
            Some(2_024_071_000),
            &mut losses,
        )
        .expect("light record attributes")
        .expect("light attributes child");
        assert!(losses.is_empty());
        assert_eq!(value.attributes.layer_index, 7);
        assert_eq!(value.attributes.name, "table light");
        assert!(!value.attributes.visible);
        assert_eq!(value.attributes.source_uuid, Uuid::nil().to_string());
        assert_eq!(
            value.attributes.attribute_user_strings[0].key,
            "attribute key"
        );
        assert_eq!(
            value.attributes.attribute_user_strings[0].value,
            "attribute value"
        );
        assert!(value.source_offset > 0);
    }

    #[test]
    fn embedded_bitmap_minor_gate_preserves_suffix_boundary() {
        let id = Uuid::from_canonical([
            0x77, 0x2e, 0x6f, 0xc1, 0xb1, 0x7b, 0x4f, 0xc4, 0x8f, 0x54, 0x5f, 0xda, 0x51, 0x1d,
            0x76, 0xd2,
        ]);
        let minor_zero_bytes = embedded_bitmap_payload(0, id, 1);
        let minor_zero = parse_embedded_image(
            &minor_zero_bytes,
            0..minor_zero_bytes.len(),
            ArchiveVersion::V8,
            42,
        )
        .expect("minor zero embedded bitmap");
        assert_eq!(minor_zero.source_uuid, None);
        assert_eq!(minor_zero.name, "");
        assert_eq!(minor_zero.buffer_byte_len, 4);

        let minor_one_bytes = embedded_bitmap_payload(1, id, 1);
        let minor_one = parse_embedded_image(
            &minor_one_bytes,
            0..minor_one_bytes.len(),
            ArchiveVersion::V8,
            42,
        )
        .expect("minor one embedded bitmap");
        assert_eq!(minor_one.source_uuid, Some(id.to_string()));
        assert_eq!(minor_one.name, "preview");
        assert_eq!(minor_one.image_crc32, 0x1122_3344);
        assert_eq!(minor_one.compression_method, 1);
        assert_eq!(minor_one.buffer_byte_len, 4);

        let raw_bytes = embedded_bitmap_payload(0, id, 0);
        let raw = parse_embedded_image(&raw_bytes, 0..raw_bytes.len(), ArchiveVersion::V8, 42)
            .expect("raw embedded bitmap");
        assert_eq!(raw.compression_method, 0);
        assert_eq!(raw.uncompressed_byte_len, 3);
        assert_eq!(raw.buffer_byte_len, 7);
    }

    #[test]
    fn windows_bitmap_consumes_source_buffer_variants_and_suffix() {
        let image = vec![0x11; 24];
        let contiguous = windows_bitmap_payload(
            WINDOWS_BITMAP,
            0,
            "",
            bitmap_header(3, 2, 24, image.len() as i32, 0),
            &[stored_bitmap_buffer(&image)],
            &[0xaa, 0xbb],
        );
        let contiguous_record = parse_windows_bitmap(
            &contiguous,
            0..contiguous.len(),
            WINDOWS_BITMAP,
            ArchiveVersion::V8,
            70,
        )
        .expect("contiguous Windows bitmap");
        assert_eq!(contiguous_record.width_pixels, 3);
        assert_eq!(contiguous_record.height_pixels, 2);
        assert_eq!(contiguous_record.pixel_buffer_offset, 40);
        assert_eq!(
            contiguous_record.pixel_buffer_byte_len as usize,
            contiguous.len() - 42
        );

        let palette = vec![0x22; 256 * 4];
        let pixels = vec![0x33; 8];
        let split = windows_bitmap_payload(
            WINDOWS_BITMAP,
            0,
            "",
            bitmap_header(2, 2, 8, pixels.len() as i32, 0),
            &[
                compressed_bitmap_buffer(&palette),
                stored_bitmap_buffer(&pixels),
            ],
            &[0xcc, 0xdd],
        );
        let split_record = parse_windows_bitmap(
            &split,
            0..split.len(),
            WINDOWS_BITMAP,
            ArchiveVersion::V8,
            71,
        )
        .expect("split Windows bitmap");
        assert_eq!(split_record.bits_per_pixel, 8);
        assert_eq!(split_record.colors_used, 0);
        assert_eq!(
            split_record.pixel_buffer_byte_len as usize,
            split.len() - 42
        );

        let ex = windows_bitmap_payload(
            WINDOWS_BITMAP_EX,
            5,
            "relative/example.bmp",
            bitmap_header(2, 2, 24, pixels.len() as i32, 0),
            &[stored_bitmap_buffer(&pixels)],
            &[0xee, 0xff],
        );
        let ex_record =
            parse_windows_bitmap(&ex, 0..ex.len(), WINDOWS_BITMAP_EX, ArchiveVersion::V8, 72)
                .expect("minor-five Windows bitmap Ex");
        assert_eq!(ex_record.file_path, "relative/example.bmp");
        assert_eq!(
            ex_record.pixel_buffer_byte_len as usize,
            ex.len() - 47 - 40 - 2
        );
    }

    #[test]
    fn legacy_windows_bitmap_uses_raw_palette_and_pixels() {
        let palette = vec![0x55; 256 * 4];
        let pixels = vec![0x66; 8];
        let mut raw = palette;
        raw.extend(pixels);
        let bytes = windows_bitmap_payload(
            WINDOWS_BITMAP,
            0,
            "",
            bitmap_header(2, 2, 8, 8, 0),
            &[raw],
            &[0xaa, 0xbb],
        );
        let record = parse_windows_bitmap(
            &bytes,
            0..bytes.len(),
            WINDOWS_BITMAP,
            ArchiveVersion::V1,
            74,
        )
        .expect("legacy raw Windows bitmap");
        assert_eq!(record.pixel_buffer_byte_len, (256 * 4 + 8) as u64);
    }

    #[test]
    fn windows_bitmap_rejects_a_buffer_size_that_disagrees_with_header() {
        let image = [0x44; 24];
        let bytes = windows_bitmap_payload(
            WINDOWS_BITMAP,
            0,
            "",
            bitmap_header(3, 2, 24, image.len() as i32, 0),
            &[stored_bitmap_buffer(&image[..1])],
            &[],
        );
        assert!(parse_windows_bitmap(
            &bytes,
            0..bytes.len(),
            WINDOWS_BITMAP,
            ArchiveVersion::V8,
            73,
        )
        .is_err());
    }

    /// The PostScript name only comes from the description when the stamp says
    /// the writer is newer than 2018-02-23. An unstamped archive drops it.
    #[test]
    fn unstamped_legacy_text_style_charges_the_font_name_stamp_loss() {
        let bytes = legacy_text_style_bytes();
        let mut losses = Vec::new();
        let value = parse_text_style(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V8,
            None,
            false,
            42,
            &mut losses,
        )
        .expect("legacy text style without a writer stamp");
        assert_eq!(value.font.postscript_name, "");
        assert_eq!(losses.len(), 1, "{losses:?}");
        assert_eq!(
            losses[0].code.local_code(),
            RhinoLossCode::SourceWriterStampUnverified.code()
        );

        let mut stamped_losses = Vec::new();
        let stamped = parse_text_style(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V8,
            Some(201_802_231),
            false,
            42,
            &mut stamped_losses,
        )
        .expect("legacy text style with a modern writer stamp");
        assert_eq!(stamped.font.postscript_name, "Helvetica Neue");
        assert!(stamped_losses.is_empty(), "{stamped_losses:?}");
    }

    /// One legacy text style whose description carries a real font name.
    fn legacy_text_style_bytes() -> Vec<u8> {
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
        bytes
    }

    #[test]
    fn legacy_text_style_preserves_font_identity_and_characteristics() {
        let bytes = legacy_text_style_bytes();
        let value = parse_text_style(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V8,
            Some(201_802_231),
            false,
            42,
            &mut Vec::new(),
        )
        .expect("valid legacy text style");
        assert_eq!(value.archive_index, Some(7));
        assert_eq!(value.font.windows_logfont_name, "Helvetica Neue");
        assert_eq!(value.font.windows_logfont_weight, Some(700));
        assert_eq!(value.font.characteristics, 0);
        assert_eq!(value.font.legacy_italic, Some(true));
        assert_eq!(value.source_offset, 42);
    }

    #[test]
    fn modern_font_matches_producer_wide_string_and_future_suffix() {
        let bytes = modern_font_chunk(7, &[0xaa, 0xbb]);
        let value = parse_font(
            &bytes,
            &mut BoundedReader::new(&bytes, 0, bytes.len()).unwrap(),
            ArchiveVersion::V8,
            None,
        )
        .expect("modern font with future minor");
        assert_eq!(value.characteristics, 0x1234_5678);
        assert_eq!(value.windows_logfont_name, "Arial");
        assert_eq!(value.postscript_name, "ArialMT");
        assert_eq!(value.family_name, "Arial");
        assert_eq!(value.panose, Some([2, 1, 2, 3, 4, 5, 6, 7, 8, 9]));
        assert_eq!(value.quartet_member, Some(2));
    }

    #[test]
    fn modern_text_style_preserves_identity_after_future_font_and_outer_suffix() {
        let id = Uuid::from_canonical([
            0x73, 0x8f, 0x5c, 0x29, 0x7f, 0x42, 0x4c, 0x89, 0xa4, 0xf5, 0x34, 0x0a, 0x2d, 0x88,
            0xc1, 0x10,
        ]);
        let mut body = model_attributes_chunk(7, "Arial style");
        body.push(1);
        body.extend(utf16("ArialMT"));
        body.push(1);
        body.extend(modern_font_chunk(7, &[0xcc, 0xdd]));
        body.extend(id.to_wire());
        body.extend(utf16("Arial style"));
        body.extend([0xee, 0xff]);
        let bytes = anonymous(2, &body);
        let value = parse_text_style(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V8,
            None,
            false,
            99,
            &mut Vec::new(),
        )
        .expect("modern text style with future suffixes");
        assert_eq!(value.archive_index, Some(7));
        assert_eq!(value.name, "Arial style");
        assert_eq!(value.font_description, "ArialMT");
        assert_eq!(value.source_uuid, Some(id.to_string()));
        assert_eq!(value.font.windows_logfont_name, "Arial");
        assert_eq!(value.source_offset, 99);
    }

    #[test]
    fn dimension_style_future_minor_preserves_known_prefix_and_suffix() {
        let bytes = future_dimension_style_chunk();
        let value = parse_dimension_style(&bytes, 0..bytes.len(), ArchiveVersion::V8, 1.0, 321)
            .expect("dimension style with future minor");
        assert_eq!(value.archive_index, Some(7));
        assert_eq!(value.name, "dimension style");
        assert_eq!(value.extension_line_extension_mm, 1.0);
        assert_eq!(value.controls["decimal_separator"], serde_json::json!(112));
        assert_eq!(value.controls["use_kerning"], serde_json::json!(true));
        assert_eq!(value.controls["line_space_scale"], serde_json::json!(1.75));
        assert_eq!(
            value.controls["dimension_length_display"],
            serde_json::json!(107)
        );
        assert_eq!(
            value.controls["font_characteristics"]["byte_len"],
            serde_json::json!(24)
        );
        assert_eq!(value.source_offset, 321);
    }

    #[test]
    fn dimension_style_current_minor_transfers_new_text_controls() {
        let bytes = current_dimension_style_chunk();
        let value = parse_dimension_style(&bytes, 0..bytes.len(), ArchiveVersion::V8, 1.0, 654)
            .expect("dimension style with current minor");
        assert_eq!(value.controls["use_kerning"], serde_json::json!(true));
        assert_eq!(value.controls["line_space_scale"], serde_json::json!(1.75));
        assert_eq!(value.source_offset, 654);
    }

    #[test]
    fn v5_dimension_style_and_extra_follow_source_gates_and_scaling() {
        let base = v5_dimension_style_chunk();
        let extra_bytes = v5_dimension_style_extra_chunk();
        let descriptor = UserdataDescriptor {
            range: 0..extra_bytes.len(),
            version: (2, 2),
            class_uuid: DIMSTYLE_EXTRA,
            item_uuid: DIMSTYLE_EXTRA,
            copy_count: 1,
            transform_range: 0..0,
            application_uuid: None,
            last_saved_as_goo: None,
            archive_version: None,
            writer_version: None,
            payload_range: 0..extra_bytes.len(),
            unknown_version: false,
        };
        let extra =
            parse_v5_dimension_style_extra(&extra_bytes, &descriptor, ArchiveVersion::V5, 2.0)
                .expect("V5 dimension-style extra");
        assert_eq!(extra.valid_fields, vec![false, true, true]);
        assert_eq!(extra.baseline_spacing_mm, 5.0);
        assert_eq!(extra.mask_color, [11, 22, 33, 44]);
        assert_eq!(
            extra.source_style_uuid,
            Some(Uuid::from_canonical([0x22; 16]).to_string())
        );

        let value = parse_v5_dimension_style(&base, 0..base.len(), 2.0, 321, Some(extra))
            .expect("V5 dimension style");
        assert_eq!(value.archive_index, Some(7));
        assert_eq!(value.name, "legacy dimension style");
        assert_eq!(value.extension_line_extension_mm, 2.0);
        assert_eq!(value.center_mark_size_mm, 8.0);
        assert_eq!(value.text_height_mm, 28.0);
        assert_eq!(value.leader_arrow_size_mm, 46.0);
        assert_eq!(value.length_factor, 15.0);
        assert_eq!(value.alternate_length_format, 17);
        assert_eq!(
            value.parent_style_uuid,
            Some(Uuid::from_canonical([0x11; 16]).to_string())
        );
        assert_eq!(value.controls["v5_arrow_type"], serde_json::json!(7));
        assert_eq!(
            value.source_uuid,
            Some(Uuid::from_canonical([0x33; 16]).to_string())
        );
        assert!(value.v5_extra.is_some());

        let mut minor_zero_body = Vec::new();
        minor_zero_body.extend([0; 16]);
        minor_zero_body.extend(0_i32.to_le_bytes());
        minor_zero_body.extend(0_i32.to_le_bytes());
        minor_zero_body.extend(4_i32.to_le_bytes());
        minor_zero_body.extend(0.0_f64.to_le_bytes());
        minor_zero_body.extend(0.0_f64.to_le_bytes());
        minor_zero_body.extend(1.0_f64.to_le_bytes());
        minor_zero_body.extend(1.0_f64.to_le_bytes());
        minor_zero_body.extend([0xee, 0xff]);
        let minor_zero = anonymous(0, &minor_zero_body);
        let mut minor_zero_descriptor = descriptor;
        minor_zero_descriptor.payload_range = 0..minor_zero.len();
        let minor_zero = parse_v5_dimension_style_extra(
            &minor_zero,
            &minor_zero_descriptor,
            ArchiveVersion::V5,
            2.0,
        )
        .expect("V5 dimension-style extra minor zero");
        assert_eq!(minor_zero.mask_color, [255, 255, 255, 0]);
        assert_eq!(minor_zero.dimension_scale, 1.0);

        let mut invalid = base;
        invalid[0] = 0x25;
        assert!(parse_v5_dimension_style(&invalid, 0..invalid.len(), 1.0, 322, None).is_err());
    }

    #[test]
    fn user_string_owner_mapping_preserves_order_and_source_cleanup() {
        let geometry = anonymous(
            0,
            &[
                1_i32.to_le_bytes().as_slice(),
                anonymous(0, &[utf16("GeometryKey"), utf16("geometry value")].concat()).as_slice(),
            ]
            .concat(),
        );
        let attributes = anonymous(
            0,
            &[
                3_i32.to_le_bytes().as_slice(),
                anonymous(0, &[utf16("$TEMP_OBJECT$"), utf16("temporary")].concat()).as_slice(),
                anonymous(
                    0,
                    &[utf16("AttributeKey"), utf16("attribute value")].concat(),
                )
                .as_slice(),
                anonymous(0, &[utf16("MixedCase"), utf16("mixed value")].concat()).as_slice(),
            ]
            .concat(),
        );
        let geometry_start = 0;
        let attributes_start = geometry.len();
        let data = [geometry, attributes].concat();
        let descriptor = |range: Range<usize>| UserdataDescriptor {
            range: range.clone(),
            version: (2, 2),
            class_uuid: USER_STRING_LIST,
            item_uuid: USER_STRING_LIST,
            copy_count: 1,
            transform_range: 0..0,
            application_uuid: None,
            last_saved_as_goo: None,
            archive_version: None,
            writer_version: None,
            payload_range: range,
            unknown_version: false,
        };
        let attribute_descriptor = |range: Range<usize>| AttributeUserdataDescriptor {
            range,
            known: true,
            class_uuid: Some(USER_STRING_LIST),
            item_uuid: Some(USER_STRING_LIST),
            application_uuid: None,
            writer_version: None,
            payload_range: Some(attributes_start..data.len()),
        };
        let mut losses = Vec::new();
        let (geometry_values, attribute_values) = first_user_string_records(
            &data,
            ArchiveVersion::V8,
            &[descriptor(geometry_start..attributes_start)],
            &[attribute_descriptor(attributes_start..data.len())],
            42,
            &mut losses,
        );
        assert!(losses.is_empty());
        assert_eq!(geometry_values.len(), 1);
        assert_eq!(geometry_values[0].key, "GeometryKey");
        assert_eq!(geometry_values[0].value, "geometry value");
        assert_eq!(
            attribute_values
                .iter()
                .map(|value| (value.key.as_str(), value.value.as_str()))
                .collect::<Vec<_>>(),
            [
                ("AttributeKey", "attribute value"),
                ("MixedCase", "mixed value")
            ]
        );
    }

    #[test]
    fn absent_component_index_does_not_alias_system_index_minus_one() {
        let record = linetype_record(
            Component {
                index: None,
                id: Uuid::nil(),
                name: String::new(),
            },
            Uuid::nil(),
            Vec::new(),
            7,
            0,
            0,
            0.0,
            0,
            Vec::new(),
            false,
        );
        assert_eq!(record.archive_index, None);
        let json = serde_json::to_value(record).expect("linetype record JSON");
        assert!(json.get("archive_index").is_none());
    }

    #[test]
    fn model_component_readers_follow_source_unknown_mask_and_status_rules() {
        let mut legacy_body = 0x28_u32.to_le_bytes().to_vec();
        legacy_body.extend(utf16("mask-compatible"));
        legacy_body.extend([0xaa, 0xbb]);
        let legacy = anonymous(0, &legacy_body);
        let mut legacy_reader = BoundedReader::new(&legacy, 0, legacy.len()).unwrap();
        let legacy_component = component(&legacy, &mut legacy_reader, ArchiveVersion::V8)
            .expect("unknown legacy mask bit is ignored");
        assert_eq!(legacy_component.index, None);
        assert!(legacy_component.id.is_nil());
        assert_eq!(legacy_component.name, "mask-compatible");
        assert_eq!(legacy_reader.remaining(), 0);

        let modern =
            model_attributes_status_chunk([3, 2, 3, 2, 1], "status-compatible", &[0xcc, 0xdd]);
        let mut modern_reader = BoundedReader::new(&modern, 0, modern.len()).unwrap();
        let modern_component = component(&modern, &mut modern_reader, ArchiveVersion::V8)
            .expect("unknown modern status values are ignored");
        assert_eq!(modern_component.index, None);
        assert!(modern_component.id.is_nil());
        assert_eq!(modern_component.name, "status-compatible");
        assert_eq!(modern_reader.remaining(), 0);
    }

    #[test]
    fn group_preserves_component_identity() {
        let mut bytes = vec![0x1f];
        bytes.extend(7_i32.to_le_bytes());
        bytes.extend(utf16("fixtures"));
        bytes.extend([0x44; 16]);
        bytes.extend([0xaa, 0xbb]);
        let group = parse_group(&bytes, 0..bytes.len(), 120).expect("required invariant");
        assert_eq!(group.archive_index, 7);
        assert_eq!(group.name, "fixtures");
        assert_eq!(
            group.source_uuid.as_deref(),
            Some("44444444-4444-4444-4444-444444444444")
        );
        assert_eq!(group.source_offset, 120);
    }

    fn light_payload(packed: u8, hotspot: f64) -> Vec<u8> {
        let mut bytes = vec![packed];
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
        bytes.extend(hotspot.to_le_bytes());
        bytes.extend([0xaa, 0xbb]);
        bytes
    }

    fn texture_payload(minor: i32, suffix: &[u8]) -> Vec<u8> {
        let mut body = vec![0x11; 16];
        body.extend(7_u32.to_le_bytes());
        body.extend(utf16("texture.png"));
        body.push(1);
        for value in 1..=7_u32 {
            body.extend(value.to_le_bytes());
        }
        for index in 0..16 {
            body.extend((if index % 5 == 0 { 1.0_f64 } else { 0.0 }).to_le_bytes());
        }
        body.extend([1, 2, 3, 4]);
        body.extend([5, 6, 7, 8]);
        body.extend([0x22; 16]);
        for value in [0.25_f64, 1.25] {
            body.extend(value.to_le_bytes());
        }
        for value in [0.1_f64, 0.2, 0.3, 0.4, 0.5] {
            body.extend(value.to_le_bytes());
        }
        body.extend([9, 10, 11, 12]);
        for value in [0.6_f64, 0.7, 0.8, 0.9] {
            body.extend(value.to_le_bytes());
        }
        body.extend(4_i32.to_le_bytes());
        if minor >= 1 {
            body.extend(crate::test_support::test_dump::file_reference(
                ArchiveVersion::V8,
                "/full/source.3dm",
                "source.3dm",
            ));
        }
        if minor >= 2 {
            body.push(1);
        }
        body.extend(suffix);
        anonymous(minor, &body)
    }

    #[test]
    fn light_scales_spatial_values_but_not_direction_or_angles() {
        let bytes = light_payload(0x1f, 0.8);
        let light = parse_light(&bytes, 0..bytes.len(), 10.0, 0, None).expect("required invariant");
        assert_eq!(light.location, [10.0, 20.0, 30.0]);
        assert_eq!(light.direction, [0.0, 0.0, -1.0]);
        assert_eq!(light.length, [40.0, 0.0, 0.0]);
        assert_eq!(light.spot_angle_degrees, 0.25);
        assert_eq!(light.spot_exponent, 16.0);
        assert_eq!(light.hotspot, 0.8);
    }

    #[test]
    fn light_preserves_unset_hotspot_for_exponent_interface() {
        let bytes = light_payload(0x12, -1.234_321_012_343_21e308);
        let light = parse_light(&bytes, 0..bytes.len(), 1.0, 0, None).expect("required invariant");
        assert_eq!(light.spot_angle_degrees, 0.25);
        assert_eq!(light.spot_exponent, 16.0);
        assert_eq!(light.hotspot, -1.234_321_012_343_21e308);
    }

    /// One legacy (outer version 2.0) material whose transparent color is the
    /// bogus [128, 128, 128] that the pre-2009 rule replaces with `diffuse`.
    fn legacy_material_bytes(diffuse: [u8; 4]) -> Vec<u8> {
        let mut body = [[0x11; 16].as_slice(), 2_i32.to_le_bytes().as_slice()].concat();
        body.extend(utf16("steel"));
        body.extend([0x22; 16]);
        for color in [
            [1, 2, 3, 4],
            diffuse,
            [9, 10, 11, 12],
            [13, 14, 15, 16],
            [17, 18, 19, 20],
            [128, 128, 128, 24],
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
        body.push(1);
        for value in [0.9_f64, 0.8, 1.4] {
            body.extend(value.to_le_bytes());
        }
        body.extend([0x33; 16]);
        body.push(1);
        body.extend([0xaa, 0xbb]);
        let inner = anonymous(7, &body);
        let mut bytes = vec![0x20];
        bytes.extend(inner);
        bytes
    }

    #[test]
    fn legacy_material_preserves_core_appearance_and_switches() {
        let bytes = legacy_material_bytes([5, 6, 7, 8]);
        let material = parse_material(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            Some(200_912_009),
            0,
            None,
            &mut Vec::new(),
        )
        .expect("required invariant");
        assert_eq!(material.name, "steel");
        assert_eq!(material.diffuse, [5, 6, 7, 8]);
        assert_eq!(material.transparent, material.diffuse);
        assert_eq!(material.index_of_refraction, 1.5);
        assert!(material.shareable);
        assert!(!material.disable_lighting);
    }

    /// The pre-2009 transparency substitution rests on the stamp.
    ///
    /// The same bytes give diffuse under an old stamp and the stored
    /// [128, 128, 128] under none, so an unstamped archive emits a color the
    /// archive does not vouch for - unless diffuse already equals the stored
    /// color, where both readings agree and nothing was substituted.
    #[test]
    fn unstamped_legacy_material_charges_the_transparency_stamp_loss() {
        let bytes = legacy_material_bytes([5, 6, 7, 8]);
        let mut losses = Vec::new();
        let material = parse_material(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            None,
            0,
            None,
            &mut losses,
        )
        .expect("legacy material without a writer stamp");
        assert_eq!(material.transparent, [128, 128, 128, 24]);
        assert_eq!(losses.len(), 1, "{losses:?}");
        assert_eq!(
            losses[0].code.local_code(),
            RhinoLossCode::SourceWriterStampUnverified.code()
        );

        let mut stamped_losses = Vec::new();
        let stamped = parse_material(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            Some(200_912_010),
            0,
            None,
            &mut stamped_losses,
        )
        .expect("legacy material with a modern writer stamp");
        assert_eq!(stamped.transparent, [128, 128, 128, 24]);
        assert!(stamped_losses.is_empty(), "{stamped_losses:?}");

        // Both readings give the same color, so no color was substituted.
        let agreeing = legacy_material_bytes([128, 128, 128, 24]);
        let mut agreeing_losses = Vec::new();
        let material = parse_material(
            &agreeing,
            0..agreeing.len(),
            ArchiveVersion::V5,
            None,
            0,
            None,
            &mut agreeing_losses,
        )
        .expect("legacy material whose diffuse equals its transparent color");
        assert_eq!(material.transparent, material.diffuse);
        assert!(agreeing_losses.is_empty(), "{agreeing_losses:?}");
    }

    fn v2_v3_material_payload(minor: u8) -> Vec<u8> {
        let mut bytes = vec![0x10 | minor];
        for color in [
            [1, 2, 3, 4],
            [5, 6, 7, 8],
            [9, 10, 11, 12],
            [13, 14, 15, 16],
        ] {
            bytes.extend(color);
        }
        for value in [64.0_f64, 0.25] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend([21, 22, 23, 24]);
        bytes.extend([25, 26, 27, 28]);
        bytes.extend(3_i16.to_le_bytes());
        bytes.extend(4_i16.to_le_bytes());
        bytes.extend(0.5_f64.to_le_bytes());
        bytes.extend(1.5_f64.to_le_bytes());

        bytes.extend(utf16("bitmap.png"));
        bytes.extend(2_i32.to_le_bytes());
        bytes.extend(31_i32.to_le_bytes());
        bytes.extend(utf16("bump.png"));
        bytes.extend(1_i32.to_le_bytes());
        bytes.extend(32_i32.to_le_bytes());
        bytes.extend(2.5_f64.to_le_bytes());
        bytes.extend(utf16("environment.png"));
        bytes.extend(9_i32.to_le_bytes());
        bytes.extend(33_i32.to_le_bytes());

        bytes.extend(7_i32.to_le_bytes());
        bytes.extend([0x44; 16]);
        bytes.extend(utf16("obsolete library"));
        bytes.extend(utf16("old steel"));
        if minor >= 1 {
            bytes.extend([0x55; 16]);
            bytes.extend([41, 42, 43, 44]);
            bytes.extend([45, 46, 47, 48]);
            bytes.extend(1.45_f64.to_le_bytes());
        }
        bytes.extend([0xaa, 0xbb]);
        bytes
    }

    #[test]
    fn v2_v3_material_reads_direct_prefix_and_legacy_textures() {
        for archive in [ArchiveVersion::V2, ArchiveVersion::V3] {
            let bytes = v2_v3_material_payload(1);
            let material = parse_material(
                &bytes,
                0..bytes.len(),
                archive,
                None,
                77,
                None,
                &mut Vec::new(),
            )
            .expect("V2/V3 material payload");
            assert_eq!(material.archive_index, Some(7));
            assert_eq!(material.name, "old steel");
            assert_eq!(
                material.plugin_uuid,
                Uuid::from_wire([0x44; 16]).to_string()
            );
            assert_eq!(material.ambient, [1, 2, 3, 4]);
            assert_eq!(material.diffuse, [5, 6, 7, 8]);
            assert_eq!(material.shine, 64.0);
            assert_eq!(material.transparency, 0.25);
            assert_eq!(material.reflection, [41, 42, 43, 44]);
            assert_eq!(material.transparent, [45, 46, 47, 48]);
            assert_eq!(material.index_of_refraction, 1.45);
            assert_eq!(
                material.source_uuid,
                Some(Uuid::from_wire([0x55; 16]).to_string())
            );
            assert_eq!(material.texture_count, 3);
            assert_eq!(material.textures[0].legacy_file_path, "bitmap.png");
            assert_eq!(material.textures[0].texture_type, 1);
            assert_eq!(material.textures[0].mode, 2);
            assert_eq!(material.textures[1].legacy_file_path, "bump.png");
            assert_eq!(material.textures[1].texture_type, 2);
            assert_eq!(material.textures[1].mode, 1);
            assert_eq!(material.textures[1].bump_scale, [0.0, 2.5]);
            assert_eq!(material.textures[2].legacy_file_path, "environment.png");
            assert_eq!(material.textures[2].texture_type, 86);
            assert_eq!(material.textures[2].mode, 1);
            assert_eq!(material.textures[0].source_offset, 77);
            assert_eq!(material.textures[0].wrap, [0, 0, 0]);
            assert_eq!(material.textures[0].uvw_transform[0], [1.0, 0.0, 0.0, 0.0]);
        }
    }

    #[test]
    fn v2_v3_material_minor_zero_uses_source_defaults_without_fabricating_identity() {
        let bytes = v2_v3_material_payload(0);
        let material = parse_material(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V2,
            None,
            77,
            None,
            &mut Vec::new(),
        )
        .expect("V2 minor-zero material payload");
        assert_eq!(material.id, "rhino:presentation:material#record-77");
        assert_eq!(material.source_uuid, None);
        assert_eq!(material.reflection, [255, 255, 255, 0]);
        assert_eq!(material.transparent, [255, 255, 255, 0]);
        assert_eq!(material.index_of_refraction, 1.0);
    }

    #[test]
    fn physically_based_material_reads_versioned_prefix_and_suffix() {
        let bytes = physically_based_payload(2, &[0xaa, 0xbb]);
        let payload = chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V8, false)
            .expect("outer userdata payload");
        let material = parse_physically_based_material(&bytes, payload.body, ArchiveVersion::V8)
            .expect("physically based material");
        assert_eq!(material.version, 2);
        assert_eq!(material.base_color, [0.1, 0.2, 0.3, 0.4]);
        assert_eq!(material.brdf, 1);
        assert_eq!(material.subsurface, 0.5);
        assert_eq!(material.subsurface_scattering_color, [0.6, 0.7, 0.8, 0.9]);
        assert_eq!(material.subsurface_scattering_radius, 1.0);
        assert_eq!(material.metallic, 2.0);
        assert_eq!(material.specular, 3.0);
        assert_eq!(material.specular_tint, 4.0);
        assert_eq!(material.roughness, 5.0);
        assert_eq!(material.anisotropic, 6.0);
        assert_eq!(material.anisotropic_rotation, 7.0);
        assert_eq!(material.sheen, 8.0);
        assert_eq!(material.sheen_tint, 9.0);
        assert_eq!(material.clearcoat, 10.0);
        assert_eq!(material.clearcoat_roughness, 11.0);
        assert_eq!(material.opacity_ior, 12.0);
        assert_eq!(material.opacity, 13.0);
        assert_eq!(material.opacity_roughness, 14.0);
        assert_eq!(material.emission, [0.11, 0.22, 0.33, 0.44]);
        assert_eq!(material.alpha, 0.77);
    }

    #[test]
    fn physically_based_material_version_one_defaults_alpha() {
        let bytes = physically_based_payload(1, &[0xcc, 0xdd]);
        let payload = chunk_at(&bytes, 0, bytes.len(), ArchiveVersion::V8, false)
            .expect("outer userdata payload");
        let material = parse_physically_based_material(&bytes, payload.body, ArchiveVersion::V8)
            .expect("version one physically based material");
        assert_eq!(material.version, 1);
        assert_eq!(material.alpha, 1.0);
    }

    fn legacy_rdk_payload(xml: &str, terminated: bool, suffix: &[u8]) -> Vec<u8> {
        let mut xml = xml.as_bytes().to_vec();
        if terminated {
            xml.push(0);
        }
        let mut bytes = 2_i32.to_le_bytes().to_vec();
        bytes.extend((xml.len() as i32).to_le_bytes());
        bytes.extend(xml);
        bytes.extend(suffix);
        bytes
    }

    fn legacy_rdk_descriptor(payload_range: Range<usize>) -> UserdataDescriptor {
        UserdataDescriptor {
            range: payload_range.clone(),
            version: (2, 2),
            class_uuid: RDK_CLASS,
            item_uuid: RDK_USERDATA,
            copy_count: 1,
            transform_range: 0..0,
            application_uuid: Some(RDK_APPLICATION),
            last_saved_as_goo: Some(false),
            archive_version: Some(5),
            writer_version: Some(0),
            payload_range,
            unknown_version: false,
        }
    }

    #[test]
    fn legacy_rdk_material_userdata_transfers_uuid_from_unterminated_xml() {
        let xml = "<xml><render-content-manager-data><material instance-id=\"AABBCCDD-EEFF-0011-2233-445566778899\" /></render-content-manager-data></xml>";
        let bytes = legacy_rdk_payload(xml, false, &[0xaa, 0xbb]);
        let userdata = [legacy_rdk_descriptor(0..bytes.len())];
        assert_eq!(
            legacy_rdk_material_instance_id(&bytes, &userdata),
            Some(Uuid::from_canonical([
                0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
                0x88, 0x99,
            ]))
        );
    }

    #[test]
    fn legacy_rdk_material_userdata_ignores_terminated_callback_xml() {
        let xml = "<xml><render-content-manager-data><material instance-id=\"AABBCCDD-EEFF-0011-2233-445566778899\" /></render-content-manager-data></xml>";
        let bytes = legacy_rdk_payload(xml, true, &[]);
        let userdata = [legacy_rdk_descriptor(0..bytes.len())];
        assert_eq!(legacy_rdk_material_instance_id(&bytes, &userdata), None);
    }

    #[test]
    fn legacy_rdk_material_userdata_rejects_malformed_xml() {
        let bytes = legacy_rdk_payload("<xml><render-content-manager-data><material>", false, &[]);
        let error = parse_legacy_rdk_material_instance_id(&bytes, 0..bytes.len())
            .expect_err("malformed legacy XML");
        assert!(matches!(error, FramingError::Structural { .. }));
    }

    #[test]
    fn rendering_attributes_transfer_mapping_channels_and_flags() {
        let mut channel_body = 7_i32.to_le_bytes().to_vec();
        channel_body.extend([0x11; 16]);
        channel_body.extend((0..16).flat_map(|value| f64::from(value).to_le_bytes()));
        channel_body.extend([0xaa, 0xbb]);
        let channel = anonymous(1, &channel_body);
        let mut mapping_body = vec![0x22; 16];
        mapping_body.extend(1_i32.to_le_bytes());
        mapping_body.extend(channel);
        mapping_body.extend([0xcc, 0xdd]);
        let mapping = anonymous(0, &mapping_body);
        let mut rendering_body = 0_i32.to_le_bytes().to_vec();
        rendering_body.extend(1_i32.to_le_bytes());
        rendering_body.extend(mapping);
        rendering_body.extend([0, 0, 1]);
        rendering_body.extend([0xee, 0xff]);
        let bytes = anonymous(3, &rendering_body);

        let value = rendering_attributes(
            &bytes,
            Some(0..bytes.len()),
            ArchiveVersion::V8,
            settings::RenderingAttributesKind::Object,
        )
        .expect("object rendering attributes");
        assert!(value.materials.is_empty());
        assert_eq!(value.mappings.len(), 1);
        assert_eq!(
            value.mappings[0].plugin_uuid,
            Uuid::from_wire([0x22; 16]).to_string()
        );
        assert_eq!(value.mappings[0].channels.len(), 1);
        assert_eq!(value.mappings[0].channels[0].mapping_channel_id, 7);
        assert_eq!(
            value.mappings[0].channels[0].mapping_uuid,
            Uuid::from_wire([0x11; 16]).to_string()
        );
        let transform = value.mappings[0].channels[0]
            .object_transform
            .expect("minor-one mapping transform");
        assert_eq!(transform[0][0], 0.0);
        assert_eq!(transform[3][3], 15.0);
        assert_eq!(value.casts_shadows, Some(false));
        assert_eq!(value.receives_shadows, Some(false));
        assert_eq!(value.advanced_texture_preview, Some(true));
    }

    #[test]
    fn rendering_material_reference_consumes_obsolete_mapping_channels() {
        let mut obsolete_channel_body = 7_i32.to_le_bytes().to_vec();
        obsolete_channel_body.extend([0x33; 16]);
        obsolete_channel_body.extend((0..16).flat_map(|value| f64::from(value).to_le_bytes()));
        let obsolete_channel = anonymous(1, &obsolete_channel_body);

        let mut material_body = vec![0x11; 16];
        material_body.extend([0x22; 16]);
        material_body.extend(1_i32.to_le_bytes());
        material_body.extend(obsolete_channel);
        material_body.extend([0x44; 16]);
        material_body.extend([3, 0, 0, 0]);
        material_body.extend([0xaa, 0xbb]);
        let material = anonymous(1, &material_body);

        let mut rendering_body = 1_i32.to_le_bytes().to_vec();
        rendering_body.extend(material);
        rendering_body.extend(0_i32.to_le_bytes());
        rendering_body.extend([1, 1, 0]);
        let bytes = anonymous(3, &rendering_body);

        let value = rendering_attributes(
            &bytes,
            Some(0..bytes.len()),
            ArchiveVersion::V8,
            settings::RenderingAttributesKind::Object,
        )
        .expect("obsolete material-reference mapping array");
        assert_eq!(value.materials.len(), 1);
        assert_eq!(
            value.materials[0].front_material_uuid,
            Uuid::from_wire([0x22; 16]).to_string()
        );
        assert_eq!(
            value.materials[0].back_material_uuid,
            Some(Uuid::from_wire([0x44; 16]).to_string())
        );
        assert_eq!(value.materials[0].material_source, Some(3));
    }

    #[test]
    fn texture_reads_minor_gates_before_future_suffix() {
        let bytes = texture_payload(2, &[0xaa, 0xbb]);
        let value = parse_texture(&bytes, 0..bytes.len(), ArchiveVersion::V8, 42)
            .expect("texture minor gates and suffix");
        assert_eq!(value.mapping_channel_id, 7);
        assert_eq!(value.legacy_file_path, "texture.png");
        assert_eq!(
            value
                .file_reference
                .as_ref()
                .map(|reference| reference.full_path.as_str()),
            Some("/full/source.3dm")
        );
        assert_eq!(value.treat_as_linear, Some(true));
        assert_eq!(value.source_offset, 42);
    }

    #[test]
    fn texture_array_closes_after_class_items_and_future_suffix() {
        let texture = crate::test_support::test_dump::class_wrapper(
            ArchiveVersion::V8,
            TEXTURE.to_wire(),
            &texture_payload(0, &[]),
        );
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(texture);
        body.extend([0xcc, 0xdd]);
        let bytes = anonymous(4, &body);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("texture array bounds");
        let values = texture_array(&bytes, &mut reader, ArchiveVersion::V8)
            .expect("texture array child and suffix");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].legacy_file_path, "texture.png");
        assert_eq!(values[0].mapping_channel_id, 7);
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn texture_mapping_reads_nested_primitive_class_wrapper() {
        let mut body = crate::test_support::MESH_CLASS.to_vec();
        body.extend(6_u32.to_le_bytes());
        body.extend(1_u32.to_le_bytes());
        for index in 0..16 {
            body.extend((if index % 5 == 0 { 1.0_f64 } else { 0.0 }).to_le_bytes());
        }
        for index in 0..16 {
            body.extend((if index % 5 == 0 { 1.0_f64 } else { 0.0 }).to_le_bytes());
        }
        body.extend(utf16("custom mesh mapping"));
        body.extend(crate::test_support::test_dump::class_wrapper(
            ArchiveVersion::V8,
            crate::test_support::MESH_CLASS,
            &[],
        ));
        body.extend(0_u32.to_le_bytes());
        body.push(0);
        body.extend([0xaa, 0xbb]);
        let bytes = anonymous(1, &body);

        let mapping = parse_texture_mapping(&bytes, 0..bytes.len(), ArchiveVersion::V8, 42)
            .expect("texture mapping with primitive class wrapper")
            .value;
        assert_eq!(mapping.mapping_type, 6);
        assert_eq!(
            mapping.primitive_class_uuid,
            Some(Uuid::from_wire(crate::test_support::MESH_CLASS).to_string())
        );
    }

    #[test]
    fn mapping_crc_cache_reads_version_one_and_bounded_suffix() {
        let mut bytes = 1_i32.to_le_bytes().to_vec();
        bytes.extend((-17_i32).to_le_bytes());
        bytes.extend([0xaa, 0xbb]);
        parse_mapping_crc_cache(&bytes, 0..bytes.len())
            .expect("version-one mapping cache with bounded suffix");
    }

    #[test]
    fn legacy_linetype_preserves_print_lengths_and_wire_segment_tags() {
        let mut body = 4_i32.to_le_bytes().to_vec();
        body.extend(utf16("dash"));
        body.extend(2_i32.to_le_bytes());
        body.extend(2.0_f64.to_le_bytes());
        body.extend(0_u32.to_le_bytes());
        body.extend(1.0_f64.to_le_bytes());
        body.extend(1_u32.to_le_bytes());
        body.extend([0x66; 16]);
        body.extend([0xaa, 0xbb]);
        let bytes = anonymous(15, &body);
        let value = parse_linetype(&bytes, 0..bytes.len(), ArchiveVersion::V5, 10.0, 0)
            .expect("required invariant");
        assert_eq!(value.name, "dash");
        assert_eq!(value.segments[0].length_millimeters, 2.0);
        assert_eq!(value.segments[0].segment_type, 0);
        assert_eq!(value.segments[1].length_millimeters, 1.0);
        assert_eq!(value.segments[1].segment_type, 1);
    }

    #[test]
    fn modern_linetype_scales_only_model_distance_segments() {
        fn modern_linetype(always_model_distance: bool) -> Vec<u8> {
            let mut component = 1_i32.to_le_bytes().to_vec();
            component.extend(0_i32.to_le_bytes());
            component.push(0);
            component.push(1);
            component.extend([0x33; 16]);
            component.push(0);
            component.push(1);
            component.extend(9_i32.to_le_bytes());
            component.push(1);
            component.extend(utf16("modern dash"));
            component.extend(crc32fast::hash(&component).to_le_bytes());
            let mut attributes = MODEL_ATTRIBUTES.to_le_bytes().to_vec();
            attributes.extend((component.len() as i64).to_le_bytes());
            attributes.extend(component);

            let mut body = attributes;
            body.extend(2_i32.to_le_bytes());
            body.extend(2.5_f64.to_le_bytes());
            body.extend(0_u32.to_le_bytes());
            body.extend(1.25_f64.to_le_bytes());
            body.extend(1_u32.to_le_bytes());
            body.extend([1, 1, 2, 2]);
            body.push(3);
            body.extend(2.75_f64.to_le_bytes());
            body.extend([4, 2]);
            body.push(5);
            body.extend(3_i32.to_le_bytes());
            for value in [[0.0_f64, 0.5], [0.35_f64, 1.25], [1.0_f64, 2.5]] {
                body.extend(value[0].to_le_bytes());
                body.extend(value[1].to_le_bytes());
            }
            if always_model_distance {
                body.extend([6, 1]);
            }
            body.push(0);

            let mut payload = 2_i32.to_le_bytes().to_vec();
            payload.extend(3_i32.to_le_bytes());
            payload.extend(body);
            payload.extend(crc32fast::hash(&payload).to_le_bytes());
            let mut bytes = ANONYMOUS.to_le_bytes().to_vec();
            bytes.extend((payload.len() as i64).to_le_bytes());
            bytes.extend(payload);
            bytes
        }

        let model_distance_bytes = modern_linetype(true);
        let model_distance = parse_linetype(
            &model_distance_bytes,
            0..model_distance_bytes.len(),
            ArchiveVersion::V8,
            25.4,
            0,
        )
        .expect("model-distance linetype");
        assert_eq!(model_distance.name, "modern dash");
        assert_eq!(model_distance.archive_index, Some(9));
        assert_eq!(model_distance.segments[0].length_millimeters, 63.5);
        assert_eq!(model_distance.segments[0].segment_type, 0);
        assert_eq!(model_distance.segments[1].length_millimeters, 31.75);
        assert_eq!(model_distance.segments[1].segment_type, 1);
        assert_eq!(model_distance.line_cap, 1);
        assert_eq!(model_distance.line_join, 2);
        assert_eq!(model_distance.width, 2.75);
        assert_eq!(model_distance.width_units, 2);
        assert_eq!(
            model_distance.taper_points,
            vec![[0.0, 0.5], [0.35, 1.25], [1.0, 2.5]]
        );
        assert!(model_distance.always_model_distance);

        let print_distance_bytes = modern_linetype(false);
        let print_distance = parse_linetype(
            &print_distance_bytes,
            0..print_distance_bytes.len(),
            ArchiveVersion::V8,
            25.4,
            0,
        )
        .expect("print-distance linetype");
        assert_eq!(print_distance.segments[0].length_millimeters, 2.5);
        assert_eq!(print_distance.segments[1].length_millimeters, 1.25);
        assert!(!print_distance.always_model_distance);
        assert_eq!(print_distance.taper_points, model_distance.taper_points);
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

    #[test]
    fn modern_hatch_pattern_reads_nested_line_chunks() {
        let mut line = 0.375_f64.to_le_bytes().to_vec();
        for value in [1.25_f64, -2.5, 3.5, 4.75] {
            line.extend(value.to_le_bytes());
        }
        line.extend(3_i32.to_le_bytes());
        for value in [1.25_f64, -0.75, 0.5] {
            line.extend(value.to_le_bytes());
        }
        line.extend([0xa5; 3]);
        let mut line_list = 1_i32.to_le_bytes().to_vec();
        line_list.extend(anonymous(0, &line));
        line_list.extend([0xb6; 5]);

        let mut component = 1_i32.to_le_bytes().to_vec();
        component.extend(0_i32.to_le_bytes());
        component.push(0);
        component.push(1);
        component.extend([0x22; 16]);
        component.push(0);
        component.push(1);
        component.extend(5_i32.to_le_bytes());
        component.push(1);
        component.extend(utf16("modern hatch"));
        let mut component_payload = component.clone();
        component_payload.extend(crc32fast::hash(&component_payload).to_le_bytes());
        let mut component_chunk = MODEL_ATTRIBUTES.to_le_bytes().to_vec();
        component_chunk.extend((component_payload.len() as i64).to_le_bytes());
        component_chunk.extend(component_payload);

        let mut body = component_chunk;
        body.extend(1_i32.to_le_bytes());
        body.extend(utf16("modern description"));
        body.extend(anonymous_body(&line_list));
        let mut v8_body = body.clone();
        v8_body.extend([0xc7; 4]);
        let bytes = anonymous(0, &v8_body);
        let value = parse_hatch_pattern(&bytes, 0..bytes.len(), ArchiveVersion::V8, 10.0, 321)
            .expect("modern hatch pattern");

        assert_eq!(value.archive_index, Some(5));
        assert_eq!(
            value.source_uuid,
            Some(Uuid::from_canonical([0x22; 16]).to_string())
        );
        assert_eq!(value.name, "modern hatch");
        assert_eq!(value.description, "modern description");
        assert_eq!(value.lines[0].angle_radians, 0.375);
        assert_eq!(value.lines[0].base_millimeters, [12.5, -25.0]);
        assert_eq!(value.lines[0].offset_millimeters, [35.0, 47.5]);
        assert_eq!(value.lines[0].dashes_millimeters, [12.5, -7.5, 5.0]);
        assert_eq!(value.pattern_unit_system, None);
        assert_eq!(value.always_model_distances, None);

        let mut v9_body = body;
        v9_body.extend([2, 1]);
        v9_body.extend([0xd8; 4]);
        let v9_bytes = anonymous(0, &v9_body);
        let v9 = parse_hatch_pattern(&v9_bytes, 0..v9_bytes.len(), ArchiveVersion::V9, 10.0, 321)
            .expect("archive-90 hatch pattern");
        assert_eq!(v9.pattern_unit_system, Some(2));
        assert_eq!(v9.always_model_distances, Some(true));
    }
}
