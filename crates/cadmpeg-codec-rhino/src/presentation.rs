// SPDX-License-Identifier: Apache-2.0
//! Rhino appearance, grouping, and lighting presentation records.

use crate::loss::{AdmittedVec, Diagnostics, ScratchVec};
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::ops::Range;

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteBinary32, FiniteReal};
use cadmpeg_ir::SourceProvenance;
use serde::Serialize;

use crate::chunks::{
    checked_count_bytes, chunk_at, direct_checksum_ranges, verify_checksum_ranges, ArchiveVersion,
    BoundedReader, ChecksumStatus, FramingError,
};
use crate::container::{NativeInstall, OpaqueRecord, Record, Scan};
use crate::instances::hex;
use crate::loss::RhinoLossCode;
use crate::objects::{
    apply_attribute_userdata, parse_attribute_userdata, parse_attributes, parse_class_wrapper,
    parse_class_wrapper_with_userdata, parse_user_string_list, AttributeUserdataDescriptor,
    ClassUserdata, ObjectAttributes, UserdataDescriptor, USER_STRING_LIST,
};
use crate::settings::{self, MillimeterScale, StandardUnit, UnitBinding};
use crate::wire::{read_finite, scaled_coordinate, uuid, Uuid};

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

fn push_presentation_loss(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    losses: &mut impl AdmittedVec<LossNote>,
    code: RhinoLossCode,
    message: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    losses.push_admitted(
        ctx,
        crate::wire::admitted_loss(ctx, code, message, "Rhino presentation loss text")?,
        "Rhino presentation losses",
    )
}

fn push_scoped_presentation_loss(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    losses: &mut impl AdmittedVec<LossNote>,
    code: RhinoLossCode,
    message: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    losses.push_with_storage_admitted(
        ctx,
        || crate::wire::admitted_loss(ctx, code, message, "Rhino presentation loss text"),
        "Rhino presentation loss notes",
    )
}

fn push_scoped_file_reference_loss(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    losses: &mut impl AdmittedVec<LossNote>,
    code: RhinoLossCode,
    source_offset: usize,
    message: std::fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    losses.push_with_storage_admitted(
        ctx,
        || {
            let loss = crate::wire::admitted_loss(
                ctx,
                code,
                message,
                "Rhino texture file-reference loss text",
            )?;
            ctx.charge_retained(5, "Rhino texture file-reference provenance format")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index("PRESENTATION/TEXTURE/FILE_REFERENCE".len()),
                "Rhino texture file-reference provenance tag",
            )?;
            Ok::<_, CodecError>(loss.with_provenance(
                SourceProvenance::root(
                    "rhino",
                    cadmpeg_core::decode::u64_from_index(source_offset),
                )
                .with_tag("PRESENTATION/TEXTURE/FILE_REFERENCE"),
            ))
        },
        "Rhino texture file-reference losses",
    )
}

fn push_opaque_record(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    opaque_records: &mut impl AdmittedVec<OpaqueRecord>,
    table_typecode: u32,
    record: &Record,
) -> Result<(), CodecError> {
    opaque_records.push_admitted(
        ctx,
        OpaqueRecord {
            table_typecode,
            record: record.clone(),
        },
        "Rhino opaque presentation records",
    )
}

#[derive(Debug)]
struct Component {
    index: Option<i32>,
    id: Uuid,
    name: String,
}

#[derive(Debug, Serialize)]
struct GroupRecord {
    id: String,
    #[serde(skip)]
    identity: GroupIdentity,
    source_offset: u64,
    archive_index: i32,
    source_uuid: Option<String>,
    name: String,
    links: Vec<String>,
}

#[derive(Debug)]
struct GroupRecordStorage<'ctx> {
    _name: cadmpeg_core::decode::ScopedReservation<'ctx>,
    _source_uuid: Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
    id: Option<cadmpeg_core::decode::ScopedReservation<'ctx>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum GroupIdentity {
    SourceUuid(Uuid),
    ArchiveIndex(i32),
}

impl std::fmt::Display for GroupIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SourceUuid(id) => write!(formatter, "rhino:presentation:group#{id}"),
            Self::ArchiveIndex(index) => write!(formatter, "rhino:presentation:group#index-{index}"),
        }
    }
}

impl cadmpeg_core::decode::cost::DecodeCost for GroupIdentity {
    const FIXED_BYTES: Option<u64> = Some(cadmpeg_core::decode::u64_from_index(
        std::mem::size_of::<Self>(),
    ));

    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(
            std::mem::size_of::<Self>(),
        ))
    }
}

#[derive(Debug, Serialize)]
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
    index_of_refraction: FiniteReal,
    reflectivity: FiniteReal,
    shine: FiniteReal,
    transparency: FiniteReal,
    #[serde(flatten, serialize_with = "serialize_material_textures")]
    textures: Vec<TextureRecord>,
    shareable: bool,
    disable_lighting: bool,
    #[serde(flatten)]
    fresnel: MaterialFresnelSlot,
    rdk_instance_uuid: Option<String>,
    diffuse_texture_alpha_transparency: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    physically_based: Option<PhysicallyBasedMaterialRecord>,
}

#[derive(Debug)]
struct MaterialFresnelSettings {
    reflections: bool,
    reflection_glossiness: FiniteReal,
    refraction_glossiness: FiniteReal,
    index_of_refraction: FiniteReal,
}

#[derive(Debug)]
struct MaterialFresnelSlot(Option<MaterialFresnelSettings>);

impl Serialize for MaterialFresnelSlot {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;

        let settings = self.0.as_ref();
        let mut fields = serializer.serialize_map(Some(4))?;
        fields.serialize_entry(
            "fresnel_reflections",
            &settings.as_ref().is_some_and(|value| value.reflections),
        )?;
        fields.serialize_entry(
            "reflection_glossiness",
            &settings.as_ref().map(|value| value.reflection_glossiness),
        )?;
        fields.serialize_entry(
            "refraction_glossiness",
            &settings.as_ref().map(|value| value.refraction_glossiness),
        )?;
        fields.serialize_entry(
            "fresnel_index_of_refraction",
            &settings.as_ref().map(|value| value.index_of_refraction),
        )?;
        fields.end()
    }
}

fn serialize_material_textures<S: serde::Serializer>(
    textures: &[TextureRecord],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;

    let mut fields = serializer.serialize_map(Some(2))?;
    fields.serialize_entry("texture_count", &textures.len())?;
    fields.serialize_entry("textures", textures)?;
    fields.end()
}

#[derive(Debug, Serialize)]
struct PhysicallyBasedMaterialRecord {
    #[serde(flatten)]
    revision: PhysicallyBasedMaterialRevision,
    base_color: [FiniteBinary32; 4],
    brdf: i32,
    subsurface: FiniteReal,
    subsurface_scattering_color: [FiniteBinary32; 4],
    subsurface_scattering_radius: FiniteReal,
    metallic: FiniteReal,
    specular: FiniteReal,
    specular_tint: FiniteReal,
    roughness: FiniteReal,
    anisotropic: FiniteReal,
    anisotropic_rotation: FiniteReal,
    sheen: FiniteReal,
    sheen_tint: FiniteReal,
    clearcoat: FiniteReal,
    clearcoat_roughness: FiniteReal,
    opacity_ior: FiniteReal,
    opacity: FiniteReal,
    opacity_roughness: FiniteReal,
    emission: [FiniteBinary32; 4],
}

#[derive(Debug)]
enum PhysicallyBasedMaterialRevision {
    V1,
    V2 { alpha: FiniteReal },
}

impl PhysicallyBasedMaterialRevision {
    fn version(&self) -> i32 {
        match self {
            Self::V1 => 1,
            Self::V2 { .. } => 2,
        }
    }

    fn alpha(&self) -> FiniteReal {
        match self {
            Self::V1 => FiniteReal::ONE,
            Self::V2 { alpha } => *alpha,
        }
    }
}

impl Serialize for PhysicallyBasedMaterialRevision {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;

        let mut fields = serializer.serialize_map(Some(2))?;
        fields.serialize_entry("version", &self.version())?;
        fields.serialize_entry("alpha", &self.alpha())?;
        fields.end()
    }
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
    uvw_transform: [[FiniteReal; 4]; 4],
    border_color: [u8; 4],
    transparent_color: [u8; 4],
    transparency_texture_uuid: Option<String>,
    bump_scale: [FiniteReal; 2],
    alpha_blend: [FiniteReal; 5],
    rgb_blend_constant: [u8; 4],
    rgb_blend: [FiniteReal; 4],
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
    intensity: FiniteReal,
    watts: FiniteReal,
    ambient: [u8; 4],
    diffuse: [u8; 4],
    specular: [u8; 4],
    direction: [FiniteReal; 3],
    location: [FiniteReal; 3],
    spot_angle_degrees: FiniteReal,
    spot_exponent: FiniteReal,
    attenuation: [FiniteReal; 3],
    shadow_intensity: FiniteReal,
    length: [FiniteReal; 3],
    width: [FiniteReal; 3],
    hotspot: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    attributes: Option<LightAttributesRecord>,
    links: Vec<String>,
}

struct SourceLinetypeSegment {
    length: FiniteReal,
    segment_type: u32,
}

#[derive(Debug, Serialize)]
struct LinetypeSegment {
    length_millimeters: FiniteReal,
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
    width: FiniteReal,
    width_units: u8,
    taper_points: Vec<[FiniteReal; 2]>,
    always_model_distance: bool,
}

struct SourceHatchLine {
    angle_radians: FiniteReal,
    base: [FiniteReal; 2],
    offset: [FiniteReal; 2],
    dashes: Vec<FiniteReal>,
}

#[derive(Debug, Serialize)]
struct HatchLineRecord {
    angle_radians: FiniteReal,
    base_millimeters: [FiniteReal; 2],
    offset_millimeters: [FiniteReal; 2],
    dashes_millimeters: Vec<FiniteReal>,
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
    #[serde(flatten)]
    distance_settings: Option<HatchPatternDistanceSettings>,
}

#[derive(Debug, Copy, Clone, Serialize)]
struct HatchPatternDistanceSettings {
    pattern_unit_system: u8,
    always_model_distances: bool,
}

#[derive(Debug)]
enum PatternTransferError {
    Framing(FramingError),
    NativeDocumentUnits,
    UnavailableDocumentUnits,
    UnsupportedHatchUnit(u8),
}

impl From<FramingError> for PatternTransferError {
    fn from(error: FramingError) -> Self {
        Self::Framing(error)
    }
}

impl From<CodecError> for PatternTransferError {
    fn from(error: CodecError) -> Self {
        Self::Framing(error.into())
    }
}

impl std::fmt::Display for PatternTransferError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Framing(error) => error.fmt(formatter),
            Self::NativeDocumentUnits => {
                formatter.write_str("document has no physical millimetre binding (native)")
            }
            Self::UnavailableDocumentUnits => {
                formatter.write_str("document has no physical millimetre binding (unavailable)")
            }
            Self::UnsupportedHatchUnit(code) => write!(
                formatter,
                "hatch pattern unit code {code} has no physical length definition",
            ),
        }
    }
}

fn pattern_document_scale(binding: UnitBinding) -> Result<MillimeterScale, PatternTransferError> {
    match binding {
        UnitBinding::Millimeters(scale) => Ok(scale),
        UnitBinding::Native => Err(PatternTransferError::NativeDocumentUnits),
        UnitBinding::Unavailable => Err(PatternTransferError::UnavailableDocumentUnits),
    }
}

#[derive(Debug, Serialize)]
struct DimensionStyleRecord {
    id: String,
    source_offset: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    archive_index: Option<i32>,
    source_uuid: Option<String>,
    name: String,
    extension_line_extension_mm: FiniteReal,
    extension_line_offset_mm: FiniteReal,
    arrow_size_mm: FiniteReal,
    /// Leader arrow size; a V5 record before minor 5 states one unit of the
    /// document scale, which no reader admits.
    leader_arrow_size_mm: f64,
    center_mark_size_mm: FiniteReal,
    text_gap_mm: FiniteReal,
    /// Text height; a V5 record before minor 1 states one unit of the document
    /// scale, which no reader admits.
    text_height_mm: f64,
    text_display_mode: u32,
    angle_format: u32,
    length_format: u32,
    angle_resolution: i32,
    length_resolution: i32,
    text_style_index: i32,
    length_factor: FiniteReal,
    alternate_enabled: bool,
    alternate_length_factor: FiniteReal,
    alternate_length_format: u32,
    alternate_length_resolution: i32,
    prefix: String,
    suffix: String,
    alternate_prefix: String,
    alternate_suffix: String,
    dimension_line_extension_mm: FiniteReal,
    suppress_extension_line_1: bool,
    suppress_extension_line_2: bool,
    #[serde(flatten)]
    details: DimensionStyleDetails,
}

#[derive(Debug)]
enum DimensionStyleDetails {
    V5 {
        controls: DimensionControlEntries,
        extra: Option<V5DimensionStyleExtraRecord>,
    },
    Modern {
        parent_style_uuid: Option<String>,
        controls: DimensionControlEntries,
    },
}

#[derive(Debug, Default)]
struct DimensionControlEntries(Vec<(String, serde_json::Value)>);

impl DimensionControlEntries {
    fn insert_with(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        name: &'static str,
        value: impl FnOnce() -> Result<serde_json::Value, FramingError>,
    ) -> Result<(), FramingError> {
        match self.0.binary_search_by(|(key, _)| key.as_str().cmp(name)) {
            Ok(index) => self.0[index].1 = value()?,
            Err(index) => {
                let key = ctx.copy_retained_text(name, "Rhino dimension control key")?;
                ctx.reserve_vec(&mut self.0, 1, "Rhino dimension controls")?;
                self.0.insert(index, (key, value()?));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
impl std::ops::Index<&str> for DimensionControlEntries {
    type Output = serde_json::Value;

    fn index(&self, name: &str) -> &Self::Output {
        let index = self
            .0
            .binary_search_by(|(key, _)| key.as_str().cmp(name))
            .expect("dimension control exists");
        &self.0[index].1
    }
}

impl DimensionStyleDetails {
    fn parent_style_uuid(&self) -> Option<&String> {
        match self {
            Self::V5 { extra, .. } => extra.as_ref()?.parent_style_uuid.as_ref(),
            Self::Modern {
                parent_style_uuid, ..
            } => parent_style_uuid.as_ref(),
        }
    }

    fn controls(&self) -> &DimensionControlEntries {
        match self {
            Self::V5 { controls, .. } | Self::Modern { controls, .. } => controls,
        }
    }
}

impl Serialize for DimensionStyleDetails {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;

        let extra = match self {
            Self::V5 { extra, .. } => extra.as_ref(),
            Self::Modern { .. } => None,
        };
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("parent_style_uuid", &self.parent_style_uuid())?;
        map.serialize_entry(
            "controls",
            &DimensionStyleControls {
                controls: self.controls(),
                extra,
            },
        )?;
        if let Some(extra) = extra {
            map.serialize_entry("v5_extra", extra)?;
        }
        map.end()
    }
}

struct DimensionStyleControls<'a> {
    controls: &'a DimensionControlEntries,
    extra: Option<&'a V5DimensionStyleExtraRecord>,
}

impl Serialize for DimensionStyleControls<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(None)?;
        for (key, value) in &self.controls.0 {
            if self.extra.is_some()
                && (key == "v5_extra_dimension_scale" || key == "v5_extra_dimension_scale_source")
            {
                continue;
            }
            map.serialize_entry(key, value)?;
        }
        if let Some(extra) = self.extra {
            map.serialize_entry("v5_extra_dimension_scale", &extra.dimension_scale)?;
            map.serialize_entry(
                "v5_extra_dimension_scale_source",
                &extra.dimension_scale_source,
            )?;
        }
        map.end()
    }
}

struct DisplayRef<'a, T: Display>(&'a T);

impl<T: Display> Serialize for DisplayRef<'_, T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self.0)
    }
}

#[derive(Debug, Serialize)]
struct V5DimensionStyleExtraRecord {
    parent_style_uuid: Option<String>,
    valid_fields: Vec<bool>,
    tolerance_style: i32,
    tolerance_resolution: i32,
    tolerance_upper_value: FiniteReal,
    tolerance_lower_value: FiniteReal,
    tolerance_height_scale: FiniteReal,
    baseline_spacing_mm: FiniteReal,
    draw_text_mask: bool,
    mask_color_source: i32,
    mask_color: [u8; 4],
    dimension_scale: FiniteReal,
    dimension_scale_source: i32,
    source_style_uuid: Option<String>,
}

#[derive(Debug, Default, Serialize)]
struct FontRecord {
    characteristics: u32,
    #[serde(flatten)]
    weight: FontWeight,
    windows_logfont_name: String,
    postscript_name: String,
    obsolete_description: String,
    point_size: Option<FiniteReal>,
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

#[derive(Debug, Default)]
enum FontWeight {
    #[default]
    Unspecified,
    Legacy {
        windows: i32,
        italic: bool,
    },
    Modern {
        windows: i32,
        apple: FiniteReal,
    },
}

impl Serialize for FontWeight {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        let (windows, apple) = match self {
            Self::Unspecified => (None, None),
            Self::Legacy { windows, italic } => {
                map.serialize_entry("legacy_italic", italic)?;
                (Some(*windows), None)
            }
            Self::Modern { windows, apple } => (Some(*windows), Some(*apple)),
        };
        map.serialize_entry("windows_logfont_weight", &windows)?;
        map.serialize_entry("apple_weight_trait", &apple)?;
        map.end()
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmbeddedImageCompression {
    Raw,
    Compressed,
}

impl Serialize for EmbeddedImageCompression {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i32(match self {
            Self::Raw => 0,
            Self::Compressed => 1,
        })
    }
}

#[derive(Debug, Serialize)]
struct EmbeddedImageRecord {
    id: String,
    source_offset: u64,
    source_uuid: Option<String>,
    name: String,
    file_path: String,
    image_crc32: u32,
    compression_method: EmbeddedImageCompression,
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
    primitive_transform: [[FiniteReal; 4]; 4],
    uvw_transform: [[FiniteReal; 4]; 4],
    primitive_class_uuid: Option<String>,
    texture_space: u32,
    capped: bool,
}

#[derive(Debug, Serialize)]
struct RenderingMaterialReference {
    plugin_uuid: String,
    front_material_uuid: String,
    #[serde(flatten)]
    back_face: RenderingMaterialBackFaceSlot,
}

#[derive(Debug)]
struct RenderingMaterialBackFace {
    back_material_uuid: Option<String>,
    material_source: u8,
}

#[derive(Debug)]
struct RenderingMaterialBackFaceSlot(Option<RenderingMaterialBackFace>);

impl Serialize for RenderingMaterialBackFaceSlot {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let back_face = self.0.as_ref();
        let mut fields = serializer.serialize_struct("RenderingMaterialBackFace", 2)?;
        fields.serialize_field(
            "back_material_uuid",
            &back_face
                .as_ref()
                .and_then(|value| value.back_material_uuid.as_ref()),
        )?;
        fields.serialize_field(
            "material_source",
            &back_face.as_ref().map(|value| value.material_source),
        )?;
        fields.end()
    }
}

#[derive(Debug, Serialize)]
struct RenderingMappingChannel {
    mapping_channel_id: i32,
    mapping_uuid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    object_transform: Option<[[FiniteReal; 4]; 4]>,
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
struct DisplacementRecord {
    xml_version: i32,
    on: bool,
    texture: Option<String>,
    channel: i32,
    black_point: FiniteReal,
    white_point: FiniteReal,
    sweep_pitch: i32,
    refine_steps: i32,
    refine_sensitivity: FiniteReal,
    face_count_limit_enabled: bool,
    face_count_limit: i32,
    post_weld_angle: FiniteReal,
    mesh_memory_limit: i32,
    fairing_enabled: bool,
    fairing_amount: i32,
    sub_object_count: Option<i32>,
    sweep_resolution_formula: i32,
    sub_items: Vec<DisplacementSubItemRecord>,
}

#[derive(Debug, Serialize)]
struct DisplacementSubItemRecord {
    face_index: i32,
    on: bool,
    texture: Option<String>,
    channel: i32,
    black_point: FiniteReal,
    white_point: FiniteReal,
}

#[derive(Debug, Serialize)]
struct EdgeSofteningRecord {
    xml_version: i32,
    on: bool,
    softening: FiniteReal,
    #[serde(flatten)]
    options: crate::mesh_modifiers::EdgeSofteningOptions,
    edge_angle_threshold: FiniteReal,
}

#[derive(Debug, Serialize)]
struct ThickeningRecord {
    xml_version: i32,
    on: bool,
    #[serde(flatten)]
    options: crate::mesh_modifiers::ThickeningOptions,
    distance: FiniteReal,
}

#[derive(Debug, Serialize)]
struct CurvePipingRecord {
    xml_version: i32,
    on: bool,
    radius: FiniteReal,
    segments: i32,
    faceted: bool,
    accuracy: i32,
    cap_type: crate::mesh_modifiers::CapType,
}

#[derive(Debug, Serialize)]
struct ShutLiningRecord {
    xml_version: i32,
    on: bool,
    #[serde(flatten)]
    options: crate::mesh_modifiers::ShutLiningOptions,
    curves: Vec<ShutLiningCurveRecord>,
}

#[derive(Debug, Serialize)]
struct ShutLiningCurveRecord {
    uuid: Option<String>,
    radius: FiniteReal,
    profile: i32,
    enabled: bool,
    pull: bool,
    is_bump: bool,
}

fn mesh_modifiers_record(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    modifiers: &crate::mesh_modifiers::MeshModifiers,
) -> Result<MeshModifiersRecord, CodecError> {
    Ok(MeshModifiersRecord {
        displacement: modifiers
            .displacement
            .as_ref()
            .map(|value| displacement_record(ctx, value))
            .transpose()?,
        edge_softening: modifiers.edge_softening.as_ref().map(edge_softening_record),
        thickening: modifiers.thickening.as_ref().map(thickening_record),
        curve_piping: modifiers.curve_piping.as_ref().map(curve_piping_record),
        shut_lining: modifiers
            .shut_lining
            .as_ref()
            .map(|value| shut_lining_record(ctx, value))
            .transpose()?,
    })
}

fn displacement_record(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    displacement: &crate::mesh_modifiers::DisplacementModifier,
) -> Result<DisplacementRecord, CodecError> {
    let mut sub_items = ctx.collection_vec(
        displacement.sub_items.len(),
        "Rhino projected displacement sub-items",
    )?;
    ctx.charge_work(0, "Rhino displacement record traversal")?;
    let mut projection_source = displacement.sub_items.iter();
    for _ in 0..projection_source.len() {
        let item = ctx.next_charged(&mut projection_source, "Rhino displacement record traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        sub_items.push(DisplacementSubItemRecord {
            face_index: item.face_index,
            on: item.on,
            texture: item
                .texture
                .map(|uuid| {
                    ctx.format_retained(
                        format_args!("{uuid}"),
                        "Rhino projected displacement sub-item texture UUID",
                    )
                })
                .transpose()?,
            channel: item.channel,
            black_point: item.black_point,
            white_point: item.white_point,
        });
    }
    Ok(DisplacementRecord {
        xml_version: displacement.xml_version,
        on: displacement.on,
        texture: displacement
            .texture
            .map(|uuid| {
                ctx.format_retained(
                    format_args!("{uuid}"),
                    "Rhino projected displacement texture UUID",
                )
            })
            .transpose()?,
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
        sub_items,
    })
}

fn edge_softening_record(
    edge_softening: &crate::mesh_modifiers::EdgeSofteningModifier,
) -> EdgeSofteningRecord {
    EdgeSofteningRecord {
        xml_version: edge_softening.xml_version,
        on: edge_softening.on,
        softening: edge_softening.softening,
        options: edge_softening.options.clone(),
        edge_angle_threshold: edge_softening.edge_angle_threshold,
    }
}

fn thickening_record(thickening: &crate::mesh_modifiers::ThickeningModifier) -> ThickeningRecord {
    ThickeningRecord {
        xml_version: thickening.xml_version,
        on: thickening.on,
        options: thickening.options.clone(),
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
        cap_type: curve_piping.cap_type,
    }
}

fn shut_lining_record(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    shut_lining: &crate::mesh_modifiers::ShutLiningModifier,
) -> Result<ShutLiningRecord, CodecError> {
    let mut curves = ctx.collection_vec(
        shut_lining.curves.len(),
        "Rhino projected shut-lining curves",
    )?;
    ctx.charge_work(0, "Rhino shut lining record traversal")?;
    let mut projection_source = shut_lining.curves.iter();
    for _ in 0..projection_source.len() {
        let curve = ctx.next_charged(&mut projection_source, "Rhino shut lining record traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        curves.push(ShutLiningCurveRecord {
            uuid: curve
                .uuid
                .map(|uuid| {
                    ctx.format_retained(
                        format_args!("{uuid}"),
                        "Rhino projected shut-lining curve UUID",
                    )
                })
                .transpose()?,
            radius: curve.radius,
            profile: curve.profile,
            enabled: curve.enabled,
            pull: curve.pull,
            is_bump: curve.is_bump,
        });
    }
    Ok(ShutLiningRecord {
        xml_version: shut_lining.xml_version,
        on: shut_lining.on,
        options: shut_lining.options.clone(),
        curves,
    })
}

impl Serialize for settings::LayerPerViewportSettings {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut record = serializer.serialize_struct("LayerPerViewportPresentationRecord", 7)?;
        record.serialize_field("viewport_uuid", &DisplayRef(&self.viewport_id))?;
        record.serialize_field("settings_mask", &self.settings_mask())?;
        record.serialize_field("color", &self.color)?;
        record.serialize_field("plot_color", &self.plot_color)?;
        record.serialize_field("plot_weight_mm", &self.plot_weight_mm)?;
        record.serialize_field(
            "visible",
            &self.visible.map(settings::LayerVisibility::as_u8),
        )?;
        record.serialize_field(
            "persistent_visibility",
            &self
                .persistent_visibility
                .map(settings::LayerVisibility::as_u8),
        )?;
        record.end()
    }
}

#[derive(Debug)]
struct LayerHierarchySlot(Option<settings::LayerHierarchy>);

impl Serialize for LayerHierarchySlot {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let hierarchy = self.0.as_ref();
        let mut record = serializer.serialize_struct("LayerHierarchy", 2)?;
        record.serialize_field(
            "parent_uuid",
            &hierarchy
                .filter(|value| !value.parent_id.is_nil())
                .map(|value| DisplayRef(&value.parent_id)),
        )?;
        record.serialize_field("expanded", &hierarchy.map(|value| value.expanded))?;
        record.end()
    }
}

#[derive(Debug)]
struct LayerPlotSlot(Option<settings::LayerPlot>);

impl Serialize for LayerPlotSlot {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let plot = self.0.as_ref();
        let mut record = serializer.serialize_struct("LayerPlot", 2)?;
        record.serialize_field("plot_color", &plot.map(|value| value.color))?;
        record.serialize_field("plot_weight_mm", &plot.map(|value| value.weight_mm))?;
        record.end()
    }
}

#[derive(Debug, Serialize)]
struct LayerPresentationRecord {
    id: String,
    source_offset: u64,
    archive_index: i32,
    source_uuid: Option<String>,
    #[serde(flatten)]
    hierarchy: LayerHierarchySlot,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    iges_level: Option<i32>,
    visible: bool,
    locked: bool,
    color: [u8; 4],
    material_index: i32,
    linetype_index: Option<i32>,
    #[serde(flatten)]
    plot: LayerPlotSlot,
    display_material_uuid: Option<String>,
    clipping_planes_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    visible_in_new_details: Option<bool>,
    rendering_materials: Vec<RenderingMaterialReference>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    per_viewport_settings: Vec<settings::LayerPerViewportSettings>,
}

#[derive(Debug, Serialize)]
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
    plot_weight_mm: FiniteReal,
    group_indexes: Vec<i32>,
    display_materials: Vec<[String; 2]>,
    active_space: u8,
    viewport_uuid: Option<String>,
    display_order: i32,
    clipping_proof: bool,
    clipping_plane_uuids: Vec<String>,
    hatch_pattern_index: i32,
    section_hatch_scale: FiniteReal,
    section_hatch_rotation: FiniteReal,
    linetype_pattern_scale: FiniteReal,
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

fn user_string_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    entries: Vec<(String, String)>,
) -> Result<Vec<UserStringRecord>, CodecError> {
    let mut records = ctx.collection_vec(entries.len(), "Rhino projected user-string entries")?;
    for (key, value) in ctx.admit_iter(entries, "Rhino user string projection traversal")? {
        records.push(UserStringRecord { key, value });
    }
    Ok(records)
}

#[derive(Clone, Copy)]
enum UserStringSource {
    Object,
    Attributes,
}

fn read_user_string_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    archive: ArchiveVersion,
    payload_range: Option<Range<usize>>,
    source_offset: usize,
    source: UserStringSource,
    losses: &mut impl AdmittedVec<LossNote>,
) -> Result<Vec<UserStringRecord>, CodecError> {
    let (label, selection) = match source {
        UserStringSource::Object => (
            "object user-string userdata",
            crate::objects::UserStringSelection::All,
        ),
        UserStringSource::Attributes => (
            "object-attributes user-string userdata",
            crate::objects::UserStringSelection::ExcludeFirstTempObject,
        ),
    };
    let Some(payload_range) = payload_range else {
        return Ok(Vec::new());
    };
    match parse_user_string_list(ctx, data, payload_range, archive, selection) {
        Ok(parsed) => user_string_records(ctx, parsed.entries),
        Err(FramingError::Resource(limit)) => Err(CodecError::ResourceLimit(limit)),
        Err(error) => {
            push_scoped_presentation_loss(
                ctx,
                losses,
                RhinoLossCode::ObjectDecodeDiagnostic,
                format_args!("{label} at offset {source_offset} could not be transferred: {error}"),
            )?;
            Ok(Vec::new())
        }
    }
}

fn first_user_string_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    archive: ArchiveVersion,
    class_userdata: &[UserdataDescriptor],
    attribute_userdata: &[AttributeUserdataDescriptor],
    source_offset: usize,
    losses: &mut impl AdmittedVec<LossNote>,
) -> Result<(Vec<UserStringRecord>, Vec<UserStringRecord>), CodecError> {
    let mut geometry_range = None;
    ctx.charge_work(0, "Rhino first user string records traversal")?;
    let mut source = class_userdata.iter();
    for _ in 0..source.len() {
        let raw = ctx.next_charged(&mut source, "Rhino first user string records traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        let Some(value) = UserdataDescriptor::known(raw) else {
            continue;
        };
        if value.class_uuid == USER_STRING_LIST && value.item_uuid == USER_STRING_LIST {
            geometry_range = Some(value.payload_range.clone());
            break;
        }
    }
    let geometry = read_user_string_records(
        ctx,
        data,
        archive,
        geometry_range,
        source_offset,
        UserStringSource::Object,
        losses,
    )?;
    let mut attributes_range = None;
    ctx.charge_work(0, "Rhino first user string records traversal")?;
    let mut source = attribute_userdata.iter();
    for _ in 0..source.len() {
        let raw = ctx.next_charged(&mut source, "Rhino first user string records traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        let Some(value) = AttributeUserdataDescriptor::known(raw) else {
            continue;
        };
        if value.class_uuid == USER_STRING_LIST && value.item_uuid == USER_STRING_LIST {
            attributes_range = Some(value.payload_range.clone());
            break;
        }
    }
    let attributes = read_user_string_records(
        ctx,
        data,
        archive,
        attributes_range,
        source_offset,
        UserStringSource::Attributes,
        losses,
    )?;
    Ok((geometry, attributes))
}

#[derive(Clone, Copy)]
struct ObjectPresentationSource {
    archive: ArchiveVersion,
    offset: usize,
    uuid: Uuid,
}

fn object_attributes_presentation(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    attributes: &ObjectAttributes,
    class_userdata: &[UserdataDescriptor],
    attribute_userdata: &[AttributeUserdataDescriptor],
    source: ObjectPresentationSource,
    losses: &mut impl AdmittedVec<LossNote>,
) -> Result<ObjectAttributesPresentation, CodecError> {
    let ObjectPresentationSource {
        archive,
        offset: source_offset,
        uuid: source_uuid,
    } = source;
    let rendering = match rendering_attributes(
        ctx,
        data,
        attributes.rendering_range.clone(),
        archive,
        settings::RenderingAttributesKind::Object,
    ) {
        Ok(rendering) => rendering,
        Err(FramingError::Resource(limit)) => return Err(CodecError::ResourceLimit(limit)),
        Err(error) => {
            push_scoped_presentation_loss(ctx, losses, RhinoLossCode::PresentationRecordDropped, format_args!(
                "object rendering attributes at offset {source_offset} could not be transferred: {error}"
            ))?;
            RenderingAttributesPresentation::default()
        }
    };
    let (user_strings, attribute_user_strings) = first_user_string_records(
        ctx,
        data,
        archive,
        class_userdata,
        attribute_userdata,
        source_offset,
        losses,
    )?;
    let name = ctx.copy_retained_text(&attributes.name, "Rhino projected object name")?;
    let url = ctx.copy_retained_text(&attributes.url, "Rhino projected object URL")?;
    let mut group_indexes = Vec::new();
    ctx.extend_from_slice(
        &mut group_indexes,
        &attributes.groups,
        "Rhino projected object groups",
    )?;
    let mut display_materials = ctx.collection_vec(
        attributes.display_materials.len(),
        "Rhino projected display materials",
    )?;
    ctx.charge_work(0, "Rhino object attributes presentation traversal")?;
    let mut projection_source = attributes.display_materials.iter();
    for _ in 0..projection_source.len() {
        let (viewport, material) = ctx.next_charged(&mut projection_source, "Rhino object attributes presentation traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        display_materials.push([
            ctx.format_retained(
                format_args!("{viewport}"),
                "Rhino projected display viewport UUID",
            )?,
            ctx.format_retained(
                format_args!("{material}"),
                "Rhino projected display material UUID",
            )?,
        ]);
    }
    let viewport_uuid = if attributes.viewport_id.is_nil() {
        None
    } else {
        Some(ctx.format_retained(
            format_args!("{}", attributes.viewport_id),
            "Rhino projected active viewport UUID",
        )?)
    };
    let mut clipping_plane_uuids = ctx.collection_vec(
        attributes.clipping_plane_ids.len(),
        "Rhino projected clipping plane UUIDs",
    )?;
    ctx.charge_work(0, "Rhino object attributes presentation traversal")?;
    let mut projection_source = attributes.clipping_plane_ids.iter();
    for _ in 0..projection_source.len() {
        let id = ctx.next_charged(&mut projection_source, "Rhino object attributes presentation traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        clipping_plane_uuids.push(ctx.format_retained(
            format_args!("{id}"),
            "Rhino projected clipping plane UUID text",
        )?);
    }
    Ok(ObjectAttributesPresentation {
        source_uuid: ctx
            .format_retained(format_args!("{source_uuid}"), "Rhino projected source UUID")?,
        name,
        url,
        layer_index: attributes.layer_index,
        material_index: attributes.material_index,
        linetype_index: attributes.linetype_index,
        color: attributes.color,
        visible: attributes.visible,
        object_mode: attributes.object_mode,
        decoration: attributes.decoration,
        wire_density: attributes.wire_density,
        color_source: attributes.color_source.as_byte(),
        linetype_source: attributes.linetype_source,
        material_source: attributes.material_source,
        plot_color_source: attributes.plot_color_source,
        plot_weight_source: attributes.plot_weight_source,
        plot_color: attributes.plot_color,
        plot_weight_mm: attributes.plot_weight,
        group_indexes,
        display_materials,
        active_space: attributes.active_space,
        viewport_uuid,
        display_order: attributes.display_order,
        clipping_proof: attributes.clipping.proof,
        clipping_plane_uuids,
        hatch_pattern_index: attributes.hatch_pattern_index,
        section_hatch_scale: attributes.section_hatch_scale,
        section_hatch_rotation: attributes.section_hatch_rotation,
        linetype_pattern_scale: attributes.linetype_pattern_scale,
        hatch_background: attributes.hatch_background,
        hatch_boundary_visible: attributes.display.hatch_boundary_visible,
        detail_background_visible: attributes.display.detail_background_visible.then_some(true),
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
            .map(|value| mesh_modifiers_record(ctx, value))
            .transpose()?,
    })
}

fn read_color_f32(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    label: &str,
) -> Result<[FiniteBinary32; 4], FramingError> {
    let offset = reader.position();
    let color = [reader.f32()?, reader.f32()?, reader.f32()?, reader.f32()?];
    let [Some(red), Some(green), Some(blue), Some(alpha)] = color.map(FiniteBinary32::new) else {
        return Err(FramingError::structural(
            offset,
            ctx.format_retained(
                format_args!("{label} contains a non-finite component"),
                "Rhino read_color_f32 text",
            )?,
        ));
    };
    Ok([red, green, blue, alpha])
}

fn finite3(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    label: &str,
) -> Result<[FiniteReal; 3], FramingError> {
    let offset = reader.position();
    let value = [reader.f64()?, reader.f64()?, reader.f64()?];
    let [Some(x), Some(y), Some(z)] = value.map(FiniteReal::new) else {
        return Err(FramingError::structural(
            offset,
            ctx.format_retained(format_args!("{label} is not finite"), "Rhino finite3 text")?,
        ));
    };
    Ok([x, y, z])
}

fn anonymous(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
) -> Result<(BoundedReader<'_>, (i32, i32)), FramingError> {
    let chunk = chunk_at(data, range.start, range.end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(FramingError::structural(
            range.start,
            "presentation wrapper is invalid",
        ));
    }
    let mut reader = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    let version = (reader.i32()?, reader.i32()?);
    Ok((reader, version))
}

fn component(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Component, FramingError> {
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if !matches!(chunk.typecode, MODEL_ATTRIBUTES | ANONYMOUS) || chunk.short() {
        return Err(FramingError::structural(
            reader.position(),
            "model-component attributes are missing",
        ));
    }
    let mut value = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
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
            crate::settings::utf16_retained(ctx, &mut value, "Rhino component name")?
        } else {
            String::new()
        };
        if bits & 0x10 != 0 {
            value.skip(8)?;
        }
        value.skip_remaining()?;
        reader.skip(chunk.next_offset() - reader.position())?;
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
        1 => crate::settings::utf16_retained(ctx, &mut value, "Rhino component name")?,
        _ => String::new(),
    };
    value.skip_remaining()?;
    reader.skip(chunk.next_offset() - reader.position())?;
    Ok(Component { index, id, name })
}

fn parse_physically_based_material(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
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
    let base_color = read_color_f32(ctx, &mut reader, "base color")?;
    let brdf = reader.i32()?;
    let subsurface = read_finite(ctx, &mut reader, "subsurface")?;
    let subsurface_scattering_color =
        read_color_f32(ctx, &mut reader, "subsurface scattering color")?;
    let subsurface_scattering_radius =
        read_finite(ctx, &mut reader, "subsurface scattering radius")?;
    let metallic = read_finite(ctx, &mut reader, "metallic")?;
    let specular = read_finite(ctx, &mut reader, "specular")?;
    let specular_tint = read_finite(ctx, &mut reader, "specular tint")?;
    let roughness = read_finite(ctx, &mut reader, "roughness")?;
    let anisotropic = read_finite(ctx, &mut reader, "anisotropic")?;
    let anisotropic_rotation = read_finite(ctx, &mut reader, "anisotropic rotation")?;
    let sheen = read_finite(ctx, &mut reader, "sheen")?;
    let sheen_tint = read_finite(ctx, &mut reader, "sheen tint")?;
    let clearcoat = read_finite(ctx, &mut reader, "clearcoat")?;
    let clearcoat_roughness = read_finite(ctx, &mut reader, "clearcoat roughness")?;
    let opacity_ior = read_finite(ctx, &mut reader, "opacity IOR")?;
    let opacity = read_finite(ctx, &mut reader, "opacity")?;
    let opacity_roughness = read_finite(ctx, &mut reader, "opacity roughness")?;
    let emission = read_color_f32(ctx, &mut reader, "emission")?;
    let revision = if version == 2 {
        PhysicallyBasedMaterialRevision::V2 {
            alpha: read_finite(ctx, &mut reader, "alpha")?,
        }
    } else {
        PhysicallyBasedMaterialRevision::V1
    };
    reader.skip_remaining()?;
    Ok(PhysicallyBasedMaterialRecord {
        revision,
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
    })
}

fn parse_uuid_text(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &str,
) -> Result<Option<Uuid>, CodecError> {
    let mut bytes = [0_u8; 16];
    let mut nibble = None;
    let mut index = 0;
    let mut source = value.bytes();
    ctx.charge_work(0, "Rhino UUID text traversal")?;
    for _ in 0..source.len() {
        let byte = ctx.next_charged(&mut source, "Rhino UUID text traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino UUID text source ended early"))?;
        if byte == b'-' {
            continue;
        }
        let value = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return Ok(None),
        };
        if let Some(high) = nibble.take() {
            if index == bytes.len() {
                return Ok(None);
            }
            bytes[index] = (high << 4) | value;
            index += 1;
        } else {
            nibble = Some(value);
        }
    }
    Ok((index == bytes.len() && nibble.is_none()).then_some(Uuid::from_canonical(bytes)))
}

fn parse_legacy_rdk_material_instance_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    payload_range: Range<usize>,
) -> Result<Option<Uuid>, FramingError> {
    match classify_rdk_material_payload(ctx, data, payload_range)? {
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
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
    let xml = reader.take(usize::try_from(length).map_err(|_| {
        FramingError::structural(reader.position(), "XML length exceeds address space")
    })?)?;
    reader.skip_remaining()?;

    // The legacy writer omits the UTF-8 terminator that ON_XMLUserData::Write
    // includes. This distinguishes the compatibility carrier from callback-
    // owned RDK XML, which remains opaque in CADIR.
    if xml.last() == Some(&0) {
        return Ok(RdkMaterialPayload::CallbackOwned);
    }
    let xml = ctx
        .validate_utf8(xml, "validate Rhino RDK XML UTF-8")?
        .map_err(|_| {
            FramingError::structural(payload_range.start, "legacy RDK XML is not UTF-8")
        })?;
    let admitted_document = ctx
        .parse_xml(xml, "Rhino legacy RDK XML tree")
        .or_else(|error| {
            let CodecError::Malformed(error) = error else {
                return Err(error.into());
            };
            Err(FramingError::structural(
                payload_range.start,
                ctx.format_retained(
                    format_args!("legacy RDK XML is malformed: {error}"),
                    "Rhino classify_rdk_material_payload text",
                )?,
            ))
        })?;
    let document = admitted_document.document();
    let root = ctx.xml_root_element(document, "Rhino RDK root search")?;
    if root.tag_name().name() != "xml" {
        return Err(FramingError::structural(
            payload_range.start,
            "legacy RDK XML root is not xml",
        ));
    }
    let mut render_data = None;
    ctx.charge_work(0, "Rhino RDK render data search")?;
    if let Some(last_child) = root.last_child() {
        let mut source = root.children();
        loop {
            let node = ctx.next_charged(&mut source, "Rhino RDK render data search")?
                .ok_or_else(|| CodecError::malformed("Rhino XML child source ended before its last child"))?;
            if node.is_element() && node.tag_name().name() == "render-content-manager-data" {
                render_data = Some(node);
                break;
            }
            if node == last_child {
                break;
            }
        }
    }
    let render_data = render_data.ok_or_else(|| {
        FramingError::structural(
            payload_range.start,
            "legacy RDK XML has no render-content-manager-data element",
        )
    })?;
    let mut material = None;
    ctx.charge_work(0, "Rhino RDK material search")?;
    if let Some(last_child) = render_data.last_child() {
        let mut source = render_data.children();
        loop {
            let node = ctx.next_charged(&mut source, "Rhino RDK material search")?
                .ok_or_else(|| CodecError::malformed("Rhino XML child source ended before its last child"))?;
            if node.is_element() && node.tag_name().name() == "material" {
                material = Some(node);
                break;
            }
            if node == last_child {
                break;
            }
        }
    }
    let material = material.ok_or_else(|| {
        FramingError::structural(
            payload_range.start,
            "legacy RDK XML has no material element",
        )
    })?;
    let mut instance_id = None;
    ctx.charge_work(0, "Rhino RDK instance attribute search")?;
    let mut source = material.attributes();
    for _ in 0..source.len() {
        let attribute = ctx.next_charged(&mut source, "Rhino RDK instance attribute search")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        if attribute.name() == "instance-id" {
            instance_id = Some(attribute.value());
            break;
        }
    }
    let instance_id = instance_id.ok_or_else(|| {
        FramingError::structural(
            payload_range.start,
            "legacy RDK material has no instance-id attribute",
        )
    })?;
    let instance_id = parse_uuid_text(ctx, instance_id)?.ok_or_else(|| {
        FramingError::structural(
            payload_range.start,
            "legacy RDK material instance-id is not a UUID",
        )
    })?;
    Ok(RdkMaterialPayload::Compatibility(
        (!instance_id.is_nil()).then_some(instance_id),
    ))
}

fn legacy_rdk_material_instance_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    userdata: &[UserdataDescriptor],
) -> Result<Option<Uuid>, CodecError> {
    ctx.charge_work(0, "Rhino legacy rdk material instance id traversal")?;
    let mut source = userdata.iter().rev();
    for _ in 0..source.len() {
        let raw = ctx.next_charged(&mut source, "Rhino legacy rdk material instance id traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        let Some(value) = raw.known() else {
            continue;
        };
        if value.class_uuid != RDK_CLASS
            || value.item_uuid != RDK_USERDATA
            || (value.application_uuid.is_some()
                && value.application_uuid != Some(RDK_APPLICATION))
        {
            continue;
        }
        match parse_legacy_rdk_material_instance_id(ctx, data, value.payload_range.clone()) {
            Ok(Some(value)) => return Ok(Some(value)),
            Err(FramingError::Resource(limit)) => return Err(CodecError::ResourceLimit(limit)),
            Ok(None) | Err(_) => {}
        }
    }
    Ok(None)
}

fn rdk_material_userdata_requires_opaque(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    userdata: &[UserdataDescriptor],
) -> Result<bool, CodecError> {
    ctx.charge_work(0, "Rhino rdk material userdata requires opaque traversal")?;
    let mut source = userdata.iter();
    for _ in 0..source.len() {
        let raw = ctx.next_charged(&mut source, "Rhino rdk material userdata requires opaque traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        let Some(value) = raw.known() else {
            continue;
        };
        if value.class_uuid != RDK_CLASS
            || value.item_uuid != RDK_USERDATA
            || (value.application_uuid.is_some()
                && value.application_uuid != Some(RDK_APPLICATION))
        {
            continue;
        }
        match classify_rdk_material_payload(ctx, data, value.payload_range.clone()) {
            Ok(RdkMaterialPayload::Compatibility(_)) => {}
            Err(FramingError::Resource(limit)) => return Err(CodecError::ResourceLimit(limit)),
            Ok(RdkMaterialPayload::CallbackOwned) | Err(_) => return Ok(true),
        }
    }
    Ok(false)
}

fn wide_string(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<String, FramingError> {
    ctx.charge_work(0, "Rhino wide string")?;
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != UTF8_STRING_CHUNK || chunk.short() {
        return Err(FramingError::structural(
            reader.position(),
            "wide-string wrapper is invalid",
        ));
    }
    let mut value = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    let format = value.u8()?;
    let result = match format {
        0 if value.remaining() == 0 => String::new(),
        1 => {
            let bytes = value.take(value.remaining())?;
            let text = ctx.validate_utf8(bytes, "validate Rhino wide string UTF-8")?
                .map_err(|_| {
                    FramingError::structural(value.position(), "wide string is not UTF-8")
                })?;
            ctx.copy_retained_text(text, "Rhino wide string")?
        }
        _ => {
            return Err(FramingError::structural(
                value.position() - 1,
                "wide-string format is unsupported",
            ))
        }
    };
    reader.skip(chunk.next_offset() - reader.position())?;
    Ok(result)
}

fn class_data(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    expected: Uuid,
) -> Result<Range<usize>, FramingError> {
    let class = parse_class_wrapper(ctx, data, record.body(), archive, &mut Diagnostics::new())?;
    if class.class_uuid != expected {
        return Err(FramingError::structural(
            record.range.start,
            "table record has the wrong class",
        ));
    }
    Ok(class.class_data_range)
}

fn class_data_prefix(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    expected: Uuid,
) -> Result<Range<usize>, FramingError> {
    let wrapper = chunk_at(data, record.body().start, record.body().end, archive, false)?;
    let class = parse_class_wrapper(
        ctx,
        data,
        wrapper.header_start..wrapper.next_offset(),
        archive,
        &mut Diagnostics::new(),
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    losses: &mut impl AdmittedVec<LossNote>,
) -> Result<Option<LightAttributesRecord>, FramingError> {
    let mut warnings = Diagnostics::new();
    let mut diagnostics_storage =
        ctx.reserve_scoped(0, "Rhino light attribute parse diagnostics")?;
    let parsed = diagnostics_storage.with_storage(|| {
        // discarded-value: the class prefix is checked and skipped; ? states the refusal and its range has no reader
        let _ = class_data_prefix(ctx, data, record, archive, LIGHT)?;
        let wrapper = chunk_at(data, record.body().start, record.body().end, archive, false)?;
        let mut offset = wrapper.next_offset();
        let mut attributes_chunk = None;
        let mut attributes_userdata_body_range = None;
        let mut phase = 0_u8;
        let mut record_end_seen = false;
        while offset < record.body().end {
            ctx.charge_work(1, "Rhino presentation cursor traversal")
                .map_err(FramingError::from)?;
            let item = chunk_at(data, offset, record.body().end, archive, false)?;
            if item.typecode == LIGHT_RECORD_END {
                if !item.short() || item.value()? != 0 {
                    return Err(FramingError::structural(
                        item.header_start,
                        "light record end must be short with value zero",
                    ));
                }
                if item.next_offset() != record.body().end {
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
                    if item.short() {
                        return Err(FramingError::structural(
                            item.header_start,
                            "light record attributes must be a long chunk",
                        ));
                    }
                    attributes_chunk = Some(item.clone());
                    phase = 1;
                }
                LIGHT_RECORD_ATTRIBUTES_USERDATA if phase <= 1 => {
                    if item.short() {
                        return Err(FramingError::structural(
                            item.header_start,
                            "light attribute userdata must be a long chunk",
                        ));
                    }
                    attributes_userdata_body_range = Some(item.body().clone());
                    phase = 2;
                }
                _ => {
                    return Err(FramingError::structural(
                        item.header_start,
                        format!("unexpected light record child {:#x}", item.typecode),
                    ));
                }
            }
            offset = item.next_offset();
        }
        if !record_end_seen {
            return Err(FramingError::structural(
                record.body().end,
                "light record is missing light record end",
            ));
        }

        let attributes = attributes_chunk
            .as_ref()
            .map(|chunk| {
                parse_attributes(
                    ctx,
                    data,
                    chunk.body(),
                    chunk.range(),
                    archive,
                    writer_version,
                    &mut warnings,
                )
            })
            .transpose()?;
        let attributes_userdata = attributes_userdata_body_range
            .as_ref()
            .map(|range| parse_attribute_userdata(ctx, data, range.clone(), archive, &mut warnings))
            .transpose()?
            .unwrap_or_default();
        let mut userdata_requires_opaque = false;
        ctx.charge_work(0, "Rhino parse light record attributes traversal")?;
        let mut source = attributes_userdata.iter();
        for _ in 0..source.len() {
            let raw = ctx.next_charged(&mut source, "Rhino parse light record attributes traversal")?
                .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
            let Some(descriptor) = raw.known() else {
                userdata_requires_opaque = true;
                break;
            };
            if descriptor.class_uuid != USER_STRING_LIST || descriptor.item_uuid != USER_STRING_LIST {
                continue;
            }
            match parse_user_string_list(
                ctx, data, descriptor.payload_range.clone(), archive,
                crate::objects::UserStringSelection::ValidateOnly,
            ) {
                Ok(_) => {}
                Err(FramingError::Resource(limit)) => return Err(FramingError::Resource(limit)),
                Err(_) => {
                    userdata_requires_opaque = true;
                    break;
                }
            }
        }
        if attributes.is_none() && !attributes_userdata.is_empty() {
            return Err(FramingError::structural(
                record.range.start,
                "light attribute userdata has no attributes owner",
            ));
        }
        if let Some(item) = attributes_chunk.as_ref() {
            let child = attributes
                .as_ref()
                .and_then(|value| value.rendering_range.clone());
            let direct = direct_checksum_ranges(ctx, &item.body(), child.as_slice())?;
            if let ChecksumStatus::Mismatch { expected, actual } =
                verify_checksum_ranges(ctx, data, item, &direct)?
            {
                warnings.push_coded_admitted(
                    ctx,
                    RhinoLossCode::IntegrityFailure,
                    format_args!(
                        "CRC mismatch at offset {} for typecode {:#x}: expected {expected:#x}, got {actual:#x}",
                        item.header_start, item.typecode
                    ),
                )?;
            }
        }
        let Some(mut attributes) = attributes else {
            return Ok(None);
        };
        apply_attribute_userdata(
            ctx,
            data,
            &mut attributes,
            &attributes_userdata,
            archive,
            &mut warnings,
        )?;
        Ok(Some((
            attributes_chunk
                .as_ref()
                .map_or(record.range.start, |chunk| chunk.header_start),
            attributes,
            attributes_userdata,
            userdata_requires_opaque,
        )))
    })?;
    let Some((source_offset, attributes, attributes_userdata, userdata_requires_opaque)) = parsed
    else {
        drop(diagnostics_storage);
        return Ok(None);
    };
    let presentation = object_attributes_presentation(
        ctx,
        data,
        &attributes,
        &[],
        &attributes_userdata,
        ObjectPresentationSource {
            archive,
            offset: record.range.start,
            uuid: attributes.object_id,
        },
        losses,
    )
    .map_err(FramingError::from)?;
    ctx.fold(
        &warnings[..],
        (),
        |(), warning| {
            push_scoped_presentation_loss(
                ctx,
                losses,
                warning
                    .code
                    .unwrap_or(RhinoLossCode::ObjectDecodeDiagnostic),
                format_args!(
                    "light record attributes at offset {}: {}",
                    record.range.start, warning.message
                ),
            )?;
            Ok(())
        },
        "Rhino light diagnostic traversal",
    )?;
    drop(diagnostics_storage);
    Ok(Some(LightAttributesRecord {
        source_offset: cadmpeg_core::decode::u64_from_index(source_offset),
        attributes: presentation,
        userdata_requires_opaque,
    }))
}

fn class_data_with_userdata(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    expected: Uuid,
) -> Result<(Range<usize>, Vec<UserdataDescriptor>), FramingError> {
    let (class, userdata) = parse_class_wrapper_with_userdata(
        ctx,
        data,
        record.body(),
        archive,
        &mut Diagnostics::new(),
    )?;
    if class.class_uuid != expected {
        return Err(FramingError::structural(
            record.range.start,
            "table record has the wrong class",
        ));
    }
    Ok((class.class_data_range, userdata))
}

fn optional_malformed<T>(value: Result<T, FramingError>) -> Result<Option<T>, CodecError> {
    match value {
        Ok(value) => Ok(Some(value)),
        Err(FramingError::Resource(limit)) => Err(CodecError::ResourceLimit(limit)),
        Err(_) => Ok(None),
    }
}

fn append_file_reference_diagnostics(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    losses: &mut impl AdmittedVec<LossNote>,
    diagnostics: &Diagnostics,
    source_offset: usize,
) -> Result<(), FramingError> {
    ctx.fold(
        &diagnostics[..],
        (),
        |(), diagnostic| {
            let code = diagnostic.code.unwrap_or(RhinoLossCode::IntegrityFailure);
            push_scoped_file_reference_loss(
                ctx,
                losses,
                code,
                source_offset,
                format_args!(
                    "texture file reference at offset {}: {}",
                    source_offset, diagnostic.message
                ),
            )?;
            Ok(())
        },
        "Rhino file reference diagnostic traversal",
    )?;
    Ok(())
}

fn parse_texture(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    source_offset: usize,
    losses: &mut impl AdmittedVec<LossNote>,
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
    let legacy_file_path = crate::settings::utf16_deferred(ctx, &mut reader)?;
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
        read_finite(ctx, &mut reader, "bump scale minimum")?,
        read_finite(ctx, &mut reader, "bump scale maximum")?,
    ];
    let alpha_blend = [
        read_finite(ctx, &mut reader, "alpha blend constant")?,
        read_finite(ctx, &mut reader, "alpha blend coefficient")?,
        read_finite(ctx, &mut reader, "alpha blend coefficient")?,
        read_finite(ctx, &mut reader, "alpha blend coefficient")?,
        read_finite(ctx, &mut reader, "alpha blend coefficient")?,
    ];
    let rgb_blend_constant = reader.array()?;
    let rgb_blend = [
        read_finite(ctx, &mut reader, "RGB blend coefficient")?,
        read_finite(ctx, &mut reader, "RGB blend coefficient")?,
        read_finite(ctx, &mut reader, "RGB blend coefficient")?,
        read_finite(ctx, &mut reader, "RGB blend coefficient")?,
    ];
    let blend_order = reader.i32()?;
    let file_reference = if version.1 >= 1 {
        let mut diagnostics = Diagnostics::new();
        let value = match crate::instances::file_reference(
            ctx,
            data,
            &mut reader,
            archive,
            &mut diagnostics,
        ) {
            Ok(value) => value,
            Err(error) => {
                if matches!(error, FramingError::Resource(_)) {
                    return Err(error);
                }
                append_file_reference_diagnostics(ctx, losses, &diagnostics, source_offset)?;
                return Err(error);
            }
        };
        append_file_reference_diagnostics(ctx, losses, &diagnostics, value.source_range.start)?;
        Some(TextureFileReference {
            full_path: value.full_path,
            relative_path: value.relative_path,
            referenced_byte_count: value.content_hash.byte_count,
            hash_time: value.content_hash.hash_time,
            content_time: value.content_hash.content_time,
            name_sha1: hex(
                ctx,
                &value.content_hash.name_sha1,
                "Rhino texture name SHA-1",
            )?,
            content_sha1: hex(
                ctx,
                &value.content_hash.content_sha1,
                "Rhino texture content SHA-1",
            )?,
            path_status: value.path_status,
            embedded_file_uuid: value
                .embedded_file_id
                .map(|id| {
                    ctx.format_retained(format_args!("{id}"), "Rhino texture embedded-file UUID")
                })
                .transpose()?,
        })
    } else {
        None
    };
    let legacy_file_path = legacy_file_path.admit(ctx, "Rhino texture legacy path")?;
    let treat_as_linear = (version.1 >= 2).then(|| reader.bool()).transpose()?;
    reader.skip_remaining()?;
    Ok(TextureRecord {
        source_offset: cadmpeg_core::decode::u64_from_index(source_offset),
        source_uuid: (!id.is_nil())
            .then(|| ctx.format_retained(format_args!("{id}"), "Rhino texture source UUID"))
            .transpose()?,
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
        transparency_texture_uuid: (!transparency.is_nil())
            .then(|| {
                ctx.format_retained(
                    format_args!("{transparency}"),
                    "Rhino texture transparency UUID",
                )
            })
            .transpose()?,
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    losses: &mut impl AdmittedVec<LossNote>,
) -> Result<Vec<TextureRecord>, FramingError> {
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(FramingError::structural(
            reader.position(),
            "texture array is not anonymous",
        ));
    }
    let mut values = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
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
    let mut textures = ctx
        .collection_vec(count, "Rhino material textures")
        .map_err(crate::chunks::FramingError::from)?;
    for _ in 0..count {
        ctx.charge_work(1, "Rhino presentation cursor traversal")
            .map_err(FramingError::from)?;
        let object = chunk_at(data, values.position(), values.end(), archive, false)?;
        if object.short() {
            return Err(FramingError::structural(
                values.position(),
                "texture object is short-framed",
            ));
        }
        let class = parse_class_wrapper(
            ctx,
            data,
            object.header_start..object.next_offset(),
            archive,
            &mut Diagnostics::new(),
        )?;
        if class.class_uuid != TEXTURE {
            return Err(FramingError::structural(
                values.position(),
                "texture array item has the wrong class",
            ));
        }
        textures.push(parse_texture(
            ctx,
            data,
            class.class_data_range,
            archive,
            object.header_start,
            losses,
        )?);
        values.skip(object.next_offset() - values.position())?;
    }
    values.skip_remaining()?;
    reader.skip(chunk.next_offset() - reader.position())?;
    Ok(textures)
}

#[derive(Clone, Copy)]
enum LegacyTextureKind {
    Bitmap,
    Bump,
    Environment,
}

impl LegacyTextureKind {
    fn texture_type(self) -> u32 {
        match self {
            Self::Bitmap => 1,
            Self::Bump => 2,
            Self::Environment => 86,
        }
    }
}

fn parse_v2_v3_texture(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    source_offset: usize,
    kind: LegacyTextureKind,
) -> Result<Option<TextureRecord>, FramingError> {
    let legacy_file_path =
        crate::settings::utf16_retained(ctx, reader, "Rhino V2/V3 texture path")?;
    let mode = reader.i32()?;
    let _obsolete_index = reader.i32()?;
    let bump_scale = if matches!(kind, LegacyTextureKind::Bump) {
        [
            FiniteReal::ZERO,
            read_finite(ctx, reader, "legacy bump scale")?,
        ]
    } else {
        [FiniteReal::ZERO, FiniteReal::ONE]
    };
    if legacy_file_path.is_empty() {
        return Ok(None);
    }
    Ok(Some(TextureRecord {
        source_offset: cadmpeg_core::decode::u64_from_index(source_offset),
        source_uuid: None,
        mapping_channel_id: 1,
        legacy_file_path,
        enabled: true,
        texture_type: kind.texture_type(),
        mode: if mode == 2 { 2 } else { 1 },
        minification_filter: 1,
        magnification_filter: 1,
        wrap: [0, 0, 0],
        uvw_transform: [
            [
                FiniteReal::ONE,
                FiniteReal::ZERO,
                FiniteReal::ZERO,
                FiniteReal::ZERO,
            ],
            [
                FiniteReal::ZERO,
                FiniteReal::ONE,
                FiniteReal::ZERO,
                FiniteReal::ZERO,
            ],
            [
                FiniteReal::ZERO,
                FiniteReal::ZERO,
                FiniteReal::ONE,
                FiniteReal::ZERO,
            ],
            [
                FiniteReal::ZERO,
                FiniteReal::ZERO,
                FiniteReal::ZERO,
                FiniteReal::ONE,
            ],
        ],
        border_color: [255, 255, 255, 255],
        transparent_color: [255, 255, 255, 255],
        transparency_texture_uuid: None,
        bump_scale,
        alpha_blend: [
            FiniteReal::ONE,
            FiniteReal::ONE,
            FiniteReal::ONE,
            FiniteReal::ZERO,
            FiniteReal::ZERO,
        ],
        rgb_blend_constant: [0, 0, 0, 0],
        rgb_blend: [
            FiniteReal::ONE,
            FiniteReal::ONE,
            FiniteReal::ZERO,
            FiniteReal::ZERO,
        ],
        blend_order: 0,
        file_reference: None,
        treat_as_linear: None,
    }))
}

fn parse_v2_v3_material(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
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
    let shine = read_finite(ctx, &mut reader, "shine")?;
    let transparency = read_finite(ctx, &mut reader, "transparency")?;
    reader.skip(4)?;
    let _obsolete_wire_color = reader.array::<4>()?;
    reader.skip(20)?;

    let mut textures = Vec::new();
    if let Some(texture) =
        parse_v2_v3_texture(ctx, &mut reader, source_offset, LegacyTextureKind::Bitmap)?
    {
        ctx.reserve_vec(&mut textures, 1, "Rhino V2/V3 material textures")
            .map_err(crate::chunks::FramingError::from)?;
        textures.push(texture);
    }
    if let Some(texture) =
        parse_v2_v3_texture(ctx, &mut reader, source_offset, LegacyTextureKind::Bump)?
    {
        ctx.reserve_vec(&mut textures, 1, "Rhino V2/V3 material textures")
            .map_err(crate::chunks::FramingError::from)?;
        textures.push(texture);
    }
    if let Some(texture) = parse_v2_v3_texture(
        ctx,
        &mut reader,
        source_offset,
        LegacyTextureKind::Environment,
    )? {
        ctx.reserve_vec(&mut textures, 1, "Rhino V2/V3 material textures")
            .map_err(crate::chunks::FramingError::from)?;
        textures.push(texture);
    }

    let archive_index = reader.i32()?;
    let plugin = uuid(&mut reader)?;
    crate::settings::utf16_deferred(ctx, &mut reader)?;
    let name = crate::settings::utf16_retained(ctx, &mut reader, "Rhino V2/V3 material name")?;
    let (id, reflection, transparent, index_of_refraction) = if minor >= 1 {
        (
            uuid(&mut reader)?,
            reader.array()?,
            reader.array()?,
            read_finite(ctx, &mut reader, "index of refraction")?,
        )
    } else {
        (
            Uuid::nil(),
            [255, 255, 255, 0],
            [255, 255, 255, 0],
            FiniteReal::ONE,
        )
    };
    reader.skip_remaining()?;
    Ok(MaterialRecord {
        id: if id.is_nil() {
            ctx.format_retained(
                format_args!("rhino:presentation:material#record-{source_offset}"),
                "Rhino material ID",
            )?
        } else {
            ctx.format_retained(
                format_args!("rhino:presentation:material#{id}"),
                "Rhino material ID",
            )?
        },
        source_offset: cadmpeg_core::decode::u64_from_index(source_offset),
        archive_index: Some(archive_index),
        source_uuid: (!id.is_nil())
            .then(|| ctx.format_retained(format_args!("{id}"), "Rhino material source UUID"))
            .transpose()?,
        name,
        plugin_uuid: ctx.format_retained(format_args!("{plugin}"), "Rhino material plugin UUID")?,
        ambient,
        diffuse,
        emission,
        specular,
        reflection,
        transparent,
        index_of_refraction,
        reflectivity: FiniteReal::ZERO,
        shine,
        transparency,
        textures,
        shareable: false,
        disable_lighting: false,
        fresnel: MaterialFresnelSlot(None),
        rdk_instance_uuid: None,
        diffuse_texture_alpha_transparency: None,
        physically_based,
    })
}

struct MaterialParseInput {
    range: Range<usize>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    source_offset: usize,
    physically_based: Option<PhysicallyBasedMaterialRecord>,
}

fn parse_material(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    input: MaterialParseInput,
    losses: &mut impl AdmittedVec<LossNote>,
) -> Result<MaterialRecord, FramingError> {
    let MaterialParseInput {
        range,
        archive,
        writer_version,
        source_offset,
        physically_based,
    } = input;
    if matches!(archive, ArchiveVersion::V2 | ArchiveVersion::V3) {
        return parse_v2_v3_material(ctx, data, range, source_offset, physically_based);
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
        let component = component(ctx, data, &mut reader, archive)?;
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
        let mut reader = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
        let version = (reader.i32()?, reader.i32()?);
        if version.0 != 1 || version.1 < 0 {
            return Err(FramingError::structural(
                reader.position(),
                "legacy material version is unsupported",
            ));
        }
        let id = uuid(&mut reader)?;
        let index = reader.i32()?;
        let name = crate::settings::utf16_retained(ctx, &mut reader, "Rhino material name")?;
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
            losses.push_with_storage_admitted(
                ctx,
                || crate::wire::admitted_loss(
                    ctx,
                    RhinoLossCode::SourceWriterStampUnverified,
                    format_args!("legacy material at offset {source_offset} kept its stored transparent color instead of the pre-2009 diffuse substitution because the archive has no writer-version stamp"),
                    "Rhino material writer-stamp loss text",
                ),
                "Rhino material writer-stamp losses",
            )
            .map_err(crate::chunks::FramingError::from)?;
        }
    }
    let index_of_refraction = read_finite(ctx, &mut reader, "index of refraction")?;
    let reflectivity = read_finite(ctx, &mut reader, "reflectivity")?;
    let shine = read_finite(ctx, &mut reader, "shine")?;
    let transparency = read_finite(ctx, &mut reader, "transparency")?;
    let textures = texture_array(ctx, data, &mut reader, archive, losses)?;
    if !modern && minor >= 1 {
        crate::settings::utf16_deferred(ctx, &mut reader)?;
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
    let fresnel = if minor >= 4 || modern {
        Some(MaterialFresnelSettings {
            reflections: reader.bool_with_writer_version(writer_version)?,
            reflection_glossiness: read_finite(ctx, &mut reader, "reflection glossiness")?,
            refraction_glossiness: read_finite(ctx, &mut reader, "refraction glossiness")?,
            index_of_refraction: read_finite(ctx, &mut reader, "Fresnel index")?,
        })
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
    Ok(MaterialRecord {
        id: if component.id.is_nil() {
            ctx.format_retained(
                format_args!("rhino:presentation:material#record-{source_offset}"),
                "Rhino material ID",
            )?
        } else {
            ctx.format_retained(
                format_args!("rhino:presentation:material#{}", component.id),
                "Rhino material ID",
            )?
        },
        source_offset: cadmpeg_core::decode::u64_from_index(source_offset),
        archive_index: component.index,
        source_uuid: (!component.id.is_nil())
            .then(|| {
                ctx.format_retained(
                    format_args!("{}", component.id),
                    "Rhino material source UUID",
                )
            })
            .transpose()?,
        name: component.name,
        plugin_uuid: ctx.format_retained(format_args!("{plugin}"), "Rhino material plugin UUID")?,
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
        textures,
        shareable,
        disable_lighting,
        fresnel: MaterialFresnelSlot(fresnel),
        rdk_instance_uuid: rdk
            .filter(|id| !id.is_nil())
            .map(|id| ctx.format_retained(format_args!("{id}"), "Rhino material RDK UUID"))
            .transpose()?,
        diffuse_texture_alpha_transparency: alpha,
        physically_based,
    })
}

fn parse_group<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    range: Range<usize>,
    source_offset: usize,
) -> Result<(GroupRecord, GroupRecordStorage<'ctx>), FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let packed = reader.u8()?;
    if packed >> 4 != 1 {
        return Err(FramingError::structural(
            range.start,
            "group version is unsupported",
        ));
    }
    let index = reader.i32()?;
    let name_bytes = crate::settings::utf16_payload(&mut reader)?;
    let (name_storage, name) = ctx
        .utf16le_scoped_text(
            name_bytes,
            name_bytes.len() / 2,
            false,
            "Rhino group name",
        )
        .map(|(name, storage)| (storage, name))?;
    let id = if packed & 0x0f >= 1 {
        Some(uuid(&mut reader)?)
    } else {
        None
    };
    reader.skip_remaining()?;
    let id = id.filter(|id| !id.is_nil());
    let (source_uuid_storage, source_uuid) = match id {
        Some(id) => {
            let (text, storage) =
                ctx.format_scoped(format_args!("{id}"), "Rhino group source UUID")?;
            (Some(storage), Some(text))
        }
        None => (None, None),
    };
    Ok((
        GroupRecord {
            id: String::new(),
            identity: id.map_or(GroupIdentity::ArchiveIndex(index), GroupIdentity::SourceUuid),
            source_offset: cadmpeg_core::decode::u64_from_index(source_offset),
            archive_index: index,
            source_uuid,
            name,
            links: Vec::new(),
        },
        GroupRecordStorage {
            _name: name_storage,
            _source_uuid: source_uuid_storage,
            id: None,
        },
    ))
}

/// Makes source identities unique when the archive repeats a group UUID or
/// archive index. The serialized identity remains in `source_uuid` and
/// `archive_index`; the suffix identifies the particular source record.
fn assign_group_id<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    group: &mut GroupRecord,
    storage: Option<&mut GroupRecordStorage<'ctx>>,
    id: std::fmt::Arguments<'_>,
    operation: &'static str,
) -> Result<(), CodecError> {
    if let Some(storage) = storage {
        group.id = String::new();
        storage.id = None;
        let (id, id_storage) = ctx.format_scoped(id, operation)?;
        group.id = id;
        storage.id = Some(id_storage);
    } else {
        group.id = ctx.format_retained(id, operation)?;
    }
    Ok(())
}

fn disambiguate_group_ids<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    groups: &mut [GroupRecord],
    mut staging: Option<&mut [GroupRecordStorage<'ctx>]>,
) -> Result<usize, CodecError> {
    let mut workspace = ctx.reserve_scoped(0, "Rhino group identity workspace")?;
    let mut counts = HashMap::<GroupIdentity, usize>::new();
    ctx.charge_work(0, "Rhino disambiguate group ids traversal")?;
    let mut projection_source = groups[..].iter();
    for _ in 0..projection_source.len() {
        let group = ctx.next_charged(&mut projection_source, "Rhino disambiguate group ids traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        let count = workspace
            .with_storage(|| {
                ctx.entry_hash_map(
                    &mut counts,
                    group.identity,
                    "Rhino group identity counts",
                )
            })?
            .or_default();
        *count += 1;
    }
    let mut index_workspace = ctx.reserve_scoped(0, "Rhino duplicate group workspace")?;
    let mut duplicate_indices = Vec::new();
    ctx.charge_work(0, "Rhino disambiguate group ids traversal")?;
    let mut projection_source = groups[..].iter();
    for order in 0..projection_source.len() {
        let group = ctx.next_charged(&mut projection_source, "Rhino disambiguate group ids traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        if ctx
            .get_hash_map(&counts, &group.identity, "Rhino group identity lookup")?
            .copied()
            != Some(1)
        {
            index_workspace.with_storage(|| {
                ctx.reserve_vec(&mut duplicate_indices, 1, "Rhino duplicate group indices")
            })?;
            duplicate_indices.push(order);
        }
    }
    drop(counts);
    drop(workspace);
    let changed = duplicate_indices.len();
    ctx.charge_work(0, "Rhino duplicate group traversal")?;
    let mut projection_source = duplicate_indices.into_iter();
    for _ in 0..projection_source.len() {
        let order = ctx.next_charged(&mut projection_source, "Rhino duplicate group traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        let group = &mut groups[order];
        let identity = group.identity;
        let source_offset = group.source_offset;
        let id = format_args!(
            "{identity}-source-offset-{source_offset:016x}-record-{order:06}"
        );
        assign_group_id(
            ctx,
            group,
            staging.as_deref_mut().and_then(|values| values.get_mut(order)),
            id,
            "Rhino disambiguated group ID",
        )?;
    }
    ctx.charge_work(0, "Rhino group identity assignment")?;
    let mut projection_source = groups[..].iter_mut();
    for order in 0..projection_source.len() {
        let group = ctx.next_charged(&mut projection_source, "Rhino group identity assignment")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        if !group.id.is_empty() {
            continue;
        }
        let identity = group.identity;
        let id = format_args!("{identity}");
        assign_group_id(
            ctx,
            group,
            staging.as_deref_mut().and_then(|values| values.get_mut(order)),
            id,
            "Rhino group ID",
        )?;
    }
    Ok(changed)
}

fn parse_light(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    range: Range<usize>,
    scale: MillimeterScale,
    source_offset: usize,
    link_order: Option<usize>,
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
    let intensity = read_finite(ctx, &mut reader, "light intensity")?;
    let watts = read_finite(ctx, &mut reader, "light watts")?;
    let ambient = reader.array()?;
    let diffuse = reader.array()?;
    let specular = reader.array()?;
    let direction = finite3(ctx, &mut reader, "light direction")?;
    let mut location = finite3(ctx, &mut reader, "light location")?;
    let spot_angle_degrees = read_finite(ctx, &mut reader, "spot angle")?;
    let mut spot_exponent = read_finite(ctx, &mut reader, "spot exponent")?;
    let attenuation = finite3(ctx, &mut reader, "light attenuation")?;
    let shadow_intensity = read_finite(ctx, &mut reader, "shadow intensity")?;
    let index = reader.i32()?;
    let id = uuid(&mut reader)?;
    let name = crate::settings::utf16_retained(ctx, &mut reader, "Rhino light name")?;
    let mut length = [FiniteReal::ZERO; 3];
    let mut width = [FiniteReal::ZERO; 3];
    if packed & 0x0f >= 1 {
        length = finite3(ctx, &mut reader, "light length")?;
        width = finite3(ctx, &mut reader, "light width")?;
    }
    // A stored hotspot is admitted finite; an older record derives it from the
    // spot exponent, a value clamped to `[0, 1]` that no reader admits.
    let hotspot = if packed & 0x0f >= 2 {
        read_finite(ctx, &mut reader, "light hotspot")?.get()
    } else {
        let value = (1.0 - spot_exponent.get() / 128.0).clamp(0.0, 1.0);
        spot_exponent = FiniteReal::ZERO;
        value
    };
    reader.skip_remaining()?;
    for vector in [&mut location, &mut length, &mut width] {
        for value in vector {
            *value = scaled_coordinate(value.get(), scale).ok_or_else(|| {
                FramingError::structural(range.start, "scaled light geometry is invalid")
            })?;
        }
    }
    let mut links = Vec::new();
    if let Some(order) = link_order {
        let link = ctx.format_retained(
            format_args!("rhino:object:record#{order:06}"),
            "Rhino light object link",
        )?;
        ctx.reserve_vec(&mut links, 1, "Rhino light links")?;
        links.push(link);
    }
    Ok(LightRecord {
        id: if id.is_nil() {
            ctx.format_retained(
                format_args!("rhino:presentation:light#record-{source_offset}"),
                "Rhino light ID",
            )?
        } else {
            ctx.format_retained(
                format_args!("rhino:presentation:light#{id}"),
                "Rhino light ID",
            )?
        },
        source_offset: cadmpeg_core::decode::u64_from_index(source_offset),
        source_uuid: ctx.format_retained(format_args!("{id}"), "Rhino light source UUID")?,
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
        links,
    })
}

fn push_light(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    workspace: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    staging: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    lights: &mut Vec<LightRecord>,
    identities: &mut HashSet<Uuid>,
    mut light: LightRecord,
) -> Result<(), CodecError> {
    let source_id = parse_uuid_text(ctx, &light.source_uuid)?
        .ok_or_else(|| CodecError::malformed("light source UUID is invalid"))?;
    if !source_id.is_nil()
        && !workspace.with_storage(|| {
            ctx.insert_hash_set(identities, source_id, "Rhino light identity index")
        })?
    {
        light.id = ctx.format_retained(
            format_args!("{}-offset-{}", light.id, light.source_offset),
            "Rhino duplicate light ID",
        )?;
    }
    staging.with_storage(|| ctx.reserve_vec(lights, 1, "Rhino lights"))?;
    lights.push(light);
    Ok(())
}

fn segments(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
) -> Result<Vec<SourceLinetypeSegment>, FramingError> {
    let count = reader.i32()?;
    let bytes = crate::chunks::checked_count_bytes(
        count,
        12,
        reader.remaining(),
        1 << 16,
        reader.position(),
    )?;
    let mut values = ctx
        .collection_vec(bytes / 12, "Rhino linetype segments")
        .map_err(crate::chunks::FramingError::from)?;
    for _ in 0..bytes / 12 {
        ctx.charge_work(1, "Rhino presentation cursor traversal")
            .map_err(FramingError::from)?;
        let length = read_finite(ctx, reader, "linetype segment length")?;
        values.push(SourceLinetypeSegment {
            length,
            segment_type: reader.u32()?,
        });
    }
    Ok(values)
}

fn parse_linetype(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    binding: UnitBinding,
    source_offset: usize,
) -> Result<LinetypeRecord, PatternTransferError> {
    let (mut reader, version) = anonymous(data, range, archive)?;
    let mut cap = 0;
    let mut join = 0;
    let mut width = FiniteReal::ONE;
    let mut width_units = 0;
    let mut taper = Vec::new();
    let mut always = false;
    let (component, values) = if version.0 == 1 && version.1 >= 0 {
        let index = reader.i32()?;
        let name = crate::settings::utf16_retained(ctx, &mut reader, "Rhino legacy linetype name")?;
        let values = segments(ctx, &mut reader)?;
        let id = if version.1 >= 1 {
            uuid(&mut reader)?
        } else {
            Uuid::nil()
        };
        (
            Component {
                index: Some(index),
                id,
                name,
            },
            values,
        )
    } else if version.0 == 2 && version.1 >= 0 {
        let component = component(ctx, data, &mut reader, archive)?;
        let values = segments(ctx, &mut reader)?;
        let mut item = if version.1 >= 1 { reader.u8()? } else { 0 };
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
                width = read_finite(ctx, &mut reader, "linetype width")?;
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
                ctx.reserve_vec(&mut taper, bytes / 16, "Rhino linetype taper points")
                    .map_err(crate::chunks::FramingError::from)?;
                let mut invalid = false;
                for _ in 0..bytes / 16 {
                    ctx.charge_work(1, "Rhino presentation cursor traversal")
                        .map_err(FramingError::from)?;
                    let first = reader.f64()?;
                    let second = reader.f64()?;
                    match (FiniteReal::new(first), FiniteReal::new(second)) {
                        (Some(first), Some(second)) if !invalid => taper.push([first, second]),
                        _ => invalid = true,
                    }
                }
                if invalid {
                    return Err(FramingError::structural(
                        reader.position(),
                        "linetype taper is not finite",
                    )
                    .into());
                }
                item = reader.u8()?;
            }
        }
        if version.1 >= 3 && item == 6 {
            always = reader.bool()?;
            let _next_item = reader.u8()?;
        }
        (component, values)
    } else {
        return Err(
            FramingError::structural(reader.position(), "linetype version is unsupported").into(),
        );
    };
    // An unknown or out-of-order extension has no generic width. The source
    // reader consumes its identifier and leaves a bounded suffix.
    reader.skip_remaining()?;
    let mut segments = ctx
        .collection_vec(values.len(), "Rhino projected linetype segments")
        .map_err(crate::chunks::FramingError::from)?;
    ctx.charge_work(0, "Rhino linetype projection traversal")?;
    let mut projection_source = values.into_iter();
    for _ in 0..projection_source.len() {
        let segment = ctx.next_charged(&mut projection_source, "Rhino linetype projection traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        let length_millimeters = if always {
            let scale = pattern_document_scale(binding)?;
            scaled_coordinate(segment.length.get(), scale).ok_or_else(|| {
                FramingError::structural(
                    source_offset,
                    "scaled model-distance linetype segment is invalid",
                )
            })?
        } else {
            segment.length
        };
        segments.push(LinetypeSegment {
            length_millimeters,
            segment_type: segment.segment_type,
        });
    }
    let id = component.id;
    Ok(LinetypeRecord {
        id: if id.is_nil() {
            ctx.format_retained(
                format_args!("rhino:presentation:linetype#record-{source_offset}"),
                "Rhino linetype ID",
            )?
        } else {
            ctx.format_retained(
                format_args!("rhino:presentation:linetype#{id}"),
                "Rhino linetype ID",
            )?
        },
        source_offset: cadmpeg_core::decode::u64_from_index(source_offset),
        archive_index: component.index,
        source_uuid: (!id.is_nil())
            .then(|| ctx.format_retained(format_args!("{id}"), "Rhino linetype source UUID"))
            .transpose()?,
        name: component.name,
        segments,
        line_cap: cap,
        line_join: join,
        width,
        width_units,
        taper_points: taper,
        always_model_distance: always,
    })
}

fn hatch_line_v5(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
) -> Result<SourceHatchLine, FramingError> {
    let packed = reader.u8()?;
    if packed >> 4 != 1 {
        return Err(FramingError::structural(
            reader.position() - 1,
            "hatch-line version is unsupported",
        ));
    }
    hatch_line_fields(ctx, reader)
}

fn hatch_line_fields(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
) -> Result<SourceHatchLine, FramingError> {
    let angle_radians = read_finite(ctx, reader, "hatch-line angle")?;
    let base = [
        read_finite(ctx, reader, "hatch-line base")?,
        read_finite(ctx, reader, "hatch-line base")?,
    ];
    let offset = [
        read_finite(ctx, reader, "hatch-line offset")?,
        read_finite(ctx, reader, "hatch-line offset")?,
    ];
    let count = reader.i32()?;
    let bytes = crate::chunks::checked_count_bytes(
        count,
        8,
        reader.remaining(),
        1 << 16,
        reader.position(),
    )?;
    let mut dashes = ctx
        .collection_vec(bytes / 8, "Rhino hatch line dashes")
        .map_err(crate::chunks::FramingError::from)?;
    for _ in 0..bytes / 8 {
        ctx.charge_work(1, "Rhino presentation cursor traversal")
            .map_err(FramingError::from)?;
        dashes.push(read_finite(ctx, reader, "hatch dash")?);
    }
    Ok(SourceHatchLine {
        angle_radians,
        base,
        offset,
        dashes,
    })
}

/// The pattern selector defines length units independently of display mode.
fn hatch_pattern_scale(
    settings: Option<HatchPatternDistanceSettings>,
    binding: UnitBinding,
) -> Result<MillimeterScale, PatternTransferError> {
    match settings.map(|settings| settings.pattern_unit_system) {
        None | Some(0) => pattern_document_scale(binding),
        Some(code) => StandardUnit::from_value(i32::from(code))
            .map(MillimeterScale::from)
            .ok_or(PatternTransferError::UnsupportedHatchUnit(code)),
    }
}

impl SourceHatchLine {
    fn into_millimeters(
        mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scale: MillimeterScale,
        source_offset: usize,
    ) -> Result<HatchLineRecord, FramingError> {
        ctx.charge_work(0, "Rhino hatch dash projection traversal")?;
        for value in self.base.iter_mut().chain(self.offset.iter_mut()) {
            *value = scaled_coordinate(value.get(), scale).ok_or_else(|| {
                FramingError::structural(source_offset, "scaled hatch line is invalid")
            })?;
        }
        let mut dashes = self.dashes.iter_mut();
        for _ in 0..dashes.len() {
            let value = ctx.next_charged(&mut dashes, "Rhino hatch dash projection traversal")?
                .ok_or_else(|| CodecError::malformed("Rhino hatch dash source ended early"))?;
            *value = scaled_coordinate(value.get(), scale).ok_or_else(|| {
                FramingError::structural(source_offset, "scaled hatch line is invalid")
            })?;
        }
        Ok(HatchLineRecord {
            angle_radians: self.angle_radians,
            base_millimeters: self.base,
            offset_millimeters: self.offset,
            dashes_millimeters: self.dashes,
        })
    }
}

fn parse_hatch_pattern(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    binding: UnitBinding,
    source_offset: usize,
) -> Result<HatchPatternRecord, PatternTransferError> {
    let modern = data.get(range.start).copied() == Some(0);
    let mut distance_settings = None;
    let (component, fill_type, description, lines) = if modern {
        let (mut reader, version) = anonymous(data, range, archive)?;
        if version.0 != 1 || version.1 < 0 {
            return Err(FramingError::structural(
                reader.position(),
                "hatch-pattern version is unsupported",
            )
            .into());
        }
        let component = component(ctx, data, &mut reader, archive)?;
        let fill_type = reader.i32()?;
        let description =
            crate::settings::utf16_retained(ctx, &mut reader, "Rhino hatch description")?;
        let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
        let mut line_reader = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
        let count = line_reader.i32()?;
        let count = usize::try_from(count).map_err(|_| {
            FramingError::structural(line_reader.position() - 4, "negative hatch-line count")
        })?;
        if count > 1 << 16 {
            return Err(FramingError::structural(
                line_reader.position() - 4,
                "hatch-line count exceeds limit",
            )
            .into());
        }
        let mut lines = ctx
            .collection_vec(count, "Rhino modern hatch lines")
            .map_err(crate::chunks::FramingError::from)?;
        for _ in 0..count {
            ctx.charge_work(1, "Rhino presentation cursor traversal")
                .map_err(FramingError::from)?;
            let line = chunk_at(
                data,
                line_reader.position(),
                line_reader.end(),
                archive,
                false,
            )?;
            let mut payload = BoundedReader::new(data, line.body().start, line.body().end)?;
            let version = (payload.i32()?, payload.i32()?);
            if version.0 != 1 || version.1 < 0 {
                return Err(FramingError::structural(
                    payload.position(),
                    "hatch-line version is unsupported",
                )
                .into());
            }
            lines.push(hatch_line_fields(ctx, &mut payload)?);
            payload.skip_remaining()?;
            line_reader.skip(line.next_offset() - line_reader.position())?;
        }
        line_reader.skip_remaining()?;
        reader.skip(chunk.next_offset() - reader.position())?;
        if archive.value() >= 90 {
            distance_settings = Some(HatchPatternDistanceSettings {
                pattern_unit_system: reader.u8()?,
                always_model_distances: reader.bool()?,
            });
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
            )
            .into());
        }
        let index = reader.i32()?;
        let fill_type = reader.i32()?;
        let name = crate::settings::utf16_retained(ctx, &mut reader, "Rhino hatch name")?;
        let description =
            crate::settings::utf16_retained(ctx, &mut reader, "Rhino hatch description")?;
        let count = if fill_type == 1 { reader.i32()? } else { 0 };
        let count = usize::try_from(count).map_err(|_| {
            FramingError::structural(reader.position() - 4, "negative hatch-line count")
        })?;
        if count > 1 << 16 {
            return Err(FramingError::structural(
                reader.position() - 4,
                "hatch-line count exceeds limit",
            )
            .into());
        }
        let mut lines = ctx
            .collection_vec(count, "Rhino legacy hatch lines")
            .map_err(crate::chunks::FramingError::from)?;
        for _ in 0..count {
            ctx.charge_work(1, "Rhino presentation cursor traversal")
                .map_err(FramingError::from)?;
            lines.push(hatch_line_v5(ctx, &mut reader)?);
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
    let lines = if lines.is_empty() {
        Vec::new()
    } else {
        let scale = hatch_pattern_scale(distance_settings, binding)?;
        let mut projected = ctx
            .collection_vec(lines.len(), "Rhino projected hatch lines")
            .map_err(crate::chunks::FramingError::from)?;
        ctx.charge_work(0, "Rhino hatch pattern projection traversal")?;
        let mut projection_source = lines.into_iter();
        for _ in 0..projection_source.len() {
            let line = ctx.next_charged(&mut projection_source, "Rhino hatch pattern projection traversal")?
                .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
            projected.push(line.into_millimeters(ctx, scale, source_offset)?);
        }
        projected
    };
    Ok(HatchPatternRecord {
        id: if component.id.is_nil() {
            ctx.format_retained(
                format_args!("rhino:presentation:hatch_pattern#record-{source_offset}"),
                "Rhino hatch ID",
            )?
        } else {
            ctx.format_retained(
                format_args!("rhino:presentation:hatch_pattern#{}", component.id),
                "Rhino hatch ID",
            )?
        },
        source_offset: cadmpeg_core::decode::u64_from_index(source_offset),
        archive_index: component.index,
        source_uuid: (!component.id.is_nil())
            .then(|| {
                ctx.format_retained(format_args!("{}", component.id), "Rhino hatch source UUID")
            })
            .transpose()?,
        name: component.name,
        fill_type,
        description,
        lines,
        distance_settings,
    })
}

fn scaled_length(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    label: &str,
) -> Result<FiniteReal, FramingError> {
    let value = read_finite(ctx, reader, label)?.get();
    scaled_coordinate(value, scale).map_or_else(
        || {
            Err(FramingError::structural(
                reader.position() - 8,
                ctx.format_retained(
                    format_args!("scaled {label} is invalid"),
                    "Rhino scaled_length text",
                )?,
            ))
        },
        Ok,
    )
}

fn named_child(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<serde_json::Value, FramingError> {
    let offset = reader.position();
    let chunk = chunk_at(data, offset, reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(FramingError::structural(
            offset,
            "dimension-style child wrapper is invalid",
        ));
    }
    reader.skip(chunk.next_offset() - offset)?;
    Ok(serde_json::json!({
        "offset": offset,
        "byte_len": chunk.next_offset() - offset,
        "sha256": String::from(cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(ctx, &data[offset..chunk.next_offset()], "Rhino dimension child SHA-256")?),
    }))
}

fn dimension_style_controls(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    scale: MillimeterScale,
    minor: i32,
) -> Result<DimensionControlEntries, FramingError> {
    let mut values = DimensionControlEntries::default();
    macro_rules! put {
        ($name:literal, $value:expr) => {{
            values.insert_with(ctx, $name, || Ok(serde_json::json!($value)))?;
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
        let mut bits = ctx
            .collection_vec(count, "Rhino dimension override bits")
            .map_err(crate::chunks::FramingError::from)?;
        for bit in ctx
            .admit_iter(
                reader.take(count)?,
                "Rhino dimension style controls borrowed traversal",
            )
            .map_err(cadmpeg_core::CodecError::from)?
        {
            bits.push(serde_json::Value::from(*bit));
        }
        values.insert_with(ctx, "field_override_bits", || {
            Ok(serde_json::Value::Array(bits))
        })?;
    }
    put!("tolerance_format", reader.u32()?);
    put!("tolerance_resolution", reader.i32()?);
    put!(
        "tolerance_upper",
        read_finite(ctx, reader, "upper tolerance")?
    );
    put!(
        "tolerance_lower",
        read_finite(ctx, reader, "lower tolerance")?
    );
    put!(
        "tolerance_height_scale",
        read_finite(ctx, reader, "tolerance height scale")?
    );
    put!(
        "baseline_spacing_mm",
        scaled_length(ctx, reader, scale, "baseline spacing")?
    );
    put!("draw_text_mask_legacy", reader.bool()?);
    put!("mask_fill_type_legacy", reader.u32()?);
    put!("mask_color_legacy", reader.array::<4>()?);
    put!(
        "dimension_scale",
        read_finite(ctx, reader, "dimension scale")?
    );
    put!("dimension_scale_source", reader.i32()?);
    let source = uuid(reader)?;
    put!(
        "source_dimension_style_uuid",
        (!source.is_nil())
            .then(|| ctx.format_retained(
                format_args!("{source}"),
                "Rhino dimension control source UUID"
            ))
            .transpose()?
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
        read_finite(ctx, reader, "extension plot weight")?
    );
    put!(
        "dimension_line_plot_weight_mm",
        read_finite(ctx, reader, "dimension plot weight")?
    );
    put!(
        "fixed_extension_length_mm",
        scaled_length(ctx, reader, scale, "fixed extension length")?
    );
    put!("fixed_extension_length_enabled", reader.bool()?);
    put!(
        "text_rotation_radians",
        read_finite(ctx, reader, "text rotation")?
    );
    put!("alternate_tolerance_resolution", reader.i32()?);
    put!(
        "tolerance_text_height_fraction",
        read_finite(ctx, reader, "tolerance text fraction")?
    );
    put!("suppress_arrow_1", reader.bool()?);
    put!("suppress_arrow_2", reader.bool()?);
    put!("text_move_leader", reader.i32()?);
    put!("arc_length_symbol", reader.i32()?);
    put!(
        "stack_text_height_fraction",
        read_finite(ctx, reader, "stack text fraction")?
    );
    put!("stack_format", reader.u32()?);
    put!(
        "alternate_rounding",
        read_finite(ctx, reader, "alternate rounding")?
    );
    put!("rounding", read_finite(ctx, reader, "rounding")?);
    put!(
        "angular_rounding",
        read_finite(ctx, reader, "angular rounding")?
    );
    put!("alternate_zero_suppression", reader.u32()?);
    put!("obsolete_tolerance_zero_suppression", reader.u32()?);
    put!("zero_suppression", reader.u32()?);
    put!("angular_zero_suppression", reader.u32()?);
    put!("alternate_below", reader.bool()?);
    put!("arrow_types", [reader.u32()?, reader.u32()?, reader.u32()?]);
    put!(
        "arrow_block_uuids",
        [
            ctx.format_retained(
                format_args!("{}", uuid(reader)?),
                "Rhino dimension arrow UUID"
            )?,
            ctx.format_retained(
                format_args!("{}", uuid(reader)?),
                "Rhino dimension arrow UUID"
            )?,
            ctx.format_retained(
                format_args!("{}", uuid(reader)?),
                "Rhino dimension arrow UUID"
            )?
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
            read_finite(ctx, reader, "leader content angle")?
        );
        put!("leader_has_landing", reader.bool()?);
        put!(
            "leader_landing_length_mm",
            scaled_length(ctx, reader, scale, "leader landing length")?
        );
        put!("obsolete_text_horizontal_alignment", reader.u32()?);
        put!("obsolete_leader_horizontal_alignment", reader.u32()?);
        put!("draw_forward", reader.bool()?);
        put!("signed_ordinate", reader.bool()?);
        put!("scale_value", named_child(ctx, data, reader, archive)?);
        put!("unit_system", reader.u32()?);
    }
    if minor >= 2 {
        put!(
            "font_characteristics",
            named_child(ctx, data, reader, archive)?
        );
    }
    if minor >= 3 {
        put!("text_mask", named_child(ctx, data, reader, archive)?);
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
            values.insert_with(ctx, name, || Ok(serde_json::json!(reader.u32()?)))?;
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
        put!(
            "line_space_scale",
            read_finite(ctx, reader, "line-space scale")?
        );
    }
    reader.skip_remaining()?;
    Ok(values)
}

fn parse_v5_dimension_style_extra(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    extra: &ClassUserdata,
    archive: ArchiveVersion,
    scale: MillimeterScale,
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
    let mut valid_fields = ctx
        .collection_vec(byte_count, "Rhino V5 dimension valid fields")
        .map_err(crate::chunks::FramingError::from)?;
    for value in ctx
        .admit_iter(
            reader.take(byte_count)?,
            "Rhino parse v5 dimension style extra borrowed traversal",
        )
        .map_err(cadmpeg_core::CodecError::from)?
    {
        valid_fields.push(*value != 0);
    }
    let tolerance_style = reader.i32()?;
    let tolerance_resolution = reader.i32()?;
    let tolerance_upper_value = read_finite(ctx, &mut reader, "tolerance upper value")?;
    let tolerance_lower_value = read_finite(ctx, &mut reader, "tolerance lower value")?;
    let tolerance_height_scale = read_finite(ctx, &mut reader, "tolerance height scale")?;
    let baseline_spacing_mm = scaled_length(ctx, &mut reader, scale, "baseline spacing")?;
    let (draw_text_mask, mask_color_source, mask_color) = if version.1 >= 1 {
        (reader.bool()?, reader.i32()?, reader.array()?)
    } else {
        (false, 0, [255, 255, 255, 0])
    };
    let (dimension_scale, dimension_scale_source) = if version.1 >= 2 {
        (
            read_finite(ctx, &mut reader, "dimension scale")?,
            reader.i32()?,
        )
    } else {
        (FiniteReal::ONE, 0)
    };
    let source_style_uuid = if version.1 >= 3 {
        uuid(&mut reader)?
    } else {
        Uuid::nil()
    };
    reader.skip_remaining()?;
    Ok(V5DimensionStyleExtraRecord {
        parent_style_uuid: (!parent_style_uuid.is_nil())
            .then(|| {
                ctx.format_retained(
                    format_args!("{parent_style_uuid}"),
                    "Rhino V5 dimension parent UUID",
                )
            })
            .transpose()?,
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
        source_style_uuid: (!source_style_uuid.is_nil())
            .then(|| {
                ctx.format_retained(
                    format_args!("{source_style_uuid}"),
                    "Rhino V5 dimension source UUID",
                )
            })
            .transpose()?,
    })
}

fn parse_v5_dimension_style(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    range: Range<usize>,
    scale: MillimeterScale,
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
    let name = crate::settings::utf16_retained(ctx, &mut reader, "Rhino V5 dimension name")?;
    let extension_line_extension_mm =
        scaled_length(ctx, &mut reader, scale, "extension-line extension")?;
    let extension_line_offset_mm = scaled_length(ctx, &mut reader, scale, "extension-line offset")?;
    let arrow_size_mm = scaled_length(ctx, &mut reader, scale, "arrow size")?;
    let center_mark_size_mm = scaled_length(ctx, &mut reader, scale, "center-mark size")?;
    let text_gap_mm = scaled_length(ctx, &mut reader, scale, "text gap")?;
    let text_display_mode = reader.u32()?;
    let arrow_type = reader.i32()?;
    let angular_units = reader.i32()?;
    let length_format = reader.u32()?;
    let angle_format = reader.u32()?;
    let length_resolution = reader.i32()?;
    let angle_resolution = reader.i32()?;
    let text_style_index = reader.i32()?;
    let text_height_mm = if minor >= 1 {
        scaled_length(ctx, &mut reader, scale, "text height")?.get()
    } else {
        scale.value()
    };
    let mut controls = DimensionControlEntries::default();
    controls.insert_with(ctx, "v5_version", || {
        Ok(serde_json::json!({ "major": major, "minor": minor }))
    })?;
    controls.insert_with(ctx, "v5_arrow_type", || Ok(serde_json::json!(arrow_type)))?;
    controls.insert_with(ctx, "v5_angular_units", || {
        Ok(serde_json::json!(angular_units))
    })?;
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
        let length_factor = read_finite(ctx, &mut reader, "length factor")?;
        let prefix =
            crate::settings::utf16_retained(ctx, &mut reader, "Rhino V5 dimension prefix")?;
        let suffix =
            crate::settings::utf16_retained(ctx, &mut reader, "Rhino V5 dimension suffix")?;
        let alternate_enabled = reader.bool()?;
        let alternate_length_factor = read_finite(ctx, &mut reader, "alternate length factor")?;
        let alternate_length_format = reader.u32()?;
        let alternate_length_resolution = reader.i32()?;
        let alternate_angle_format = reader.u32()?;
        let alternate_angle_resolution = reader.i32()?;
        let alternate_prefix = crate::settings::utf16_retained(
            ctx,
            &mut reader,
            "Rhino V5 dimension alternate prefix",
        )?;
        let alternate_suffix = crate::settings::utf16_retained(
            ctx,
            &mut reader,
            "Rhino V5 dimension alternate suffix",
        )?;
        let unused = reader.u32()?;
        controls.insert_with(ctx, "v5_length_factor", || {
            Ok(serde_json::json!(length_factor))
        })?;
        controls.insert_with(ctx, "v5_alternate_angle_format", || {
            Ok(serde_json::json!(alternate_angle_format))
        })?;
        controls.insert_with(ctx, "v5_alternate_angle_resolution", || {
            Ok(serde_json::json!(alternate_angle_resolution))
        })?;
        controls.insert_with(ctx, "v5_unused", || Ok(serde_json::json!(unused)))?;
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
            FiniteReal::ONE,
            false,
            FiniteReal::ONE,
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
        scaled_length(ctx, &mut reader, scale, "dimension-line extension")?
    } else {
        FiniteReal::ZERO
    };
    let (
        leader_arrow_size_mm,
        leader_arrow_type,
        suppress_extension_line_1,
        suppress_extension_line_2,
    ) = if minor >= 5 {
        (
            scaled_length(ctx, &mut reader, scale, "leader arrow size")?.get(),
            reader.i32()?,
            reader.bool()?,
            reader.bool()?,
        )
    } else {
        (scale.value(), 0, false, false)
    };
    reader.skip_remaining()?;
    controls.insert_with(ctx, "v5_leader_arrow_type", || {
        Ok(serde_json::json!(leader_arrow_type))
    })?;
    Ok(DimensionStyleRecord {
        id: if id.is_nil() {
            ctx.format_retained(
                format_args!("rhino:presentation:dimension_style#record-{source_offset}"),
                "Rhino dimension style ID",
            )?
        } else {
            ctx.format_retained(
                format_args!("rhino:presentation:dimension_style#{id}"),
                "Rhino dimension style ID",
            )?
        },
        source_offset: cadmpeg_core::decode::u64_from_index(source_offset),
        archive_index: Some(archive_index),
        source_uuid: (!id.is_nil())
            .then(|| ctx.format_retained(format_args!("{id}"), "Rhino dimension style source UUID"))
            .transpose()?,
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
        details: DimensionStyleDetails::V5 { controls, extra },
    })
}

fn parse_dimension_style(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    scale: MillimeterScale,
    source_offset: usize,
) -> Result<DimensionStyleRecord, FramingError> {
    let (mut reader, version) = anonymous(data, range, archive)?;
    if version.0 != 1 || version.1 < 0 {
        return Err(FramingError::structural(
            reader.position(),
            "dimension-style version is unsupported",
        ));
    }
    let component = component(ctx, data, &mut reader, archive)?;
    let extension_line_extension_mm =
        scaled_length(ctx, &mut reader, scale, "extension-line extension")?;
    let extension_line_offset_mm = scaled_length(ctx, &mut reader, scale, "extension-line offset")?;
    let arrow_size_mm = scaled_length(ctx, &mut reader, scale, "arrow size")?;
    let leader_arrow_size_mm = scaled_length(ctx, &mut reader, scale, "leader arrow size")?.get();
    let center_mark_size_mm = scaled_length(ctx, &mut reader, scale, "center-mark size")?;
    let text_gap_mm = scaled_length(ctx, &mut reader, scale, "text gap")?;
    let text_height_mm = scaled_length(ctx, &mut reader, scale, "text height")?.get();
    let text_display_mode = reader.u32()?;
    let angle_format = reader.u32()?;
    let length_format = reader.u32()?;
    let angle_resolution = reader.i32()?;
    let length_resolution = reader.i32()?;
    let text_style_index = reader.i32()?;
    let length_factor = read_finite(ctx, &mut reader, "length factor")?;
    let alternate_enabled = reader.bool()?;
    let alternate_length_factor = read_finite(ctx, &mut reader, "alternate length factor")?;
    let alternate_length_format = reader.u32()?;
    let alternate_length_resolution = reader.i32()?;
    let prefix = crate::settings::utf16_retained(ctx, &mut reader, "Rhino dimension prefix")?;
    let suffix = crate::settings::utf16_retained(ctx, &mut reader, "Rhino dimension suffix")?;
    let alternate_prefix =
        crate::settings::utf16_retained(ctx, &mut reader, "Rhino dimension alternate prefix")?;
    let alternate_suffix =
        crate::settings::utf16_retained(ctx, &mut reader, "Rhino dimension alternate suffix")?;
    let dimension_line_extension_mm =
        scaled_length(ctx, &mut reader, scale, "dimension-line extension")?;
    let suppress_extension_line_1 = reader.bool()?;
    let suppress_extension_line_2 = reader.bool()?;
    let parent = uuid(&mut reader)?;
    let controls = dimension_style_controls(ctx, data, &mut reader, archive, scale, version.1)?;
    Ok(DimensionStyleRecord {
        id: if component.id.is_nil() {
            ctx.format_retained(
                format_args!("rhino:presentation:dimension_style#record-{source_offset}"),
                "Rhino dimension style ID",
            )?
        } else {
            ctx.format_retained(
                format_args!("rhino:presentation:dimension_style#{}", component.id),
                "Rhino dimension style ID",
            )?
        },
        source_offset: cadmpeg_core::decode::u64_from_index(source_offset),
        archive_index: component.index,
        source_uuid: (!component.id.is_nil())
            .then(|| {
                ctx.format_retained(
                    format_args!("{}", component.id),
                    "Rhino dimension style source UUID",
                )
            })
            .transpose()?,
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
        details: DimensionStyleDetails::Modern {
            parent_style_uuid: (!parent.is_nil())
                .then(|| {
                    ctx.format_retained(format_args!("{parent}"), "Rhino dimension parent UUID")
                })
                .transpose()?,
            controls,
        },
    })
}

fn xform(reader: &mut BoundedReader<'_>) -> Result<[[FiniteReal; 4]; 4], FramingError> {
    let offset = reader.position();
    let mut rows = [[0.0; 4]; 4];
    for value in rows.iter_mut().flatten() {
        *value = reader.f64()?;
    }
    let mut admitted = [[FiniteReal::ZERO; 4]; 4];
    for (target, value) in admitted
        .iter_mut()
        .flatten()
        .zip(rows.into_iter().flatten())
    {
        *target = FiniteReal::new(value)
            .ok_or_else(|| FramingError::structural(offset, "texture transform is not finite"))?;
    }
    Ok(admitted)
}

fn parse_embedded_image(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
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
    let file_path = crate::settings::utf16_retained(ctx, &mut reader, "Rhino image path")?;
    let image_crc32 = reader.u32()?;
    let compression_method = match reader.i32()? {
        0 => EmbeddedImageCompression::Raw,
        1 => EmbeddedImageCompression::Compressed,
        _ => {
            return Err(FramingError::structural(
                reader.position() - 4,
                "embedded-image compression method is unsupported",
            ));
        }
    };
    let buffer_offset = reader.position();
    let uncompressed_byte_len = u64::from(reader.u32()?);
    match compression_method {
        EmbeddedImageCompression::Raw => {
            if uncompressed_byte_len != 0 {
                let size = usize::try_from(uncompressed_byte_len)
                    .map_err(|_| FramingError::structural(buffer_offset, "image size overflow"))?;
                reader.skip(size)?;
            }
        }
        EmbeddedImageCompression::Compressed => {
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
                    if chunk.typecode != ANONYMOUS || chunk.short() {
                        return Err(FramingError::structural(
                            reader.position(),
                            "compressed image chunk is invalid",
                        ));
                    }
                    reader.skip(chunk.next_offset() - reader.position())?;
                }
            }
        }
    }
    let buffer_end = reader.position();
    let source_uuid = if packed & 0x0f >= 1 {
        Some(uuid(&mut reader)?)
    } else {
        None
    };
    let name = if packed & 0x0f >= 1 {
        crate::settings::utf16_retained(ctx, &mut reader, "Rhino image name")?
    } else {
        String::new()
    };
    reader.skip_remaining()?;
    let source_uuid = source_uuid.filter(|id| !id.is_nil());
    Ok(EmbeddedImageRecord {
        id: if let Some(id) = source_uuid {
            ctx.format_retained(
                format_args!("rhino:presentation:image#{id}"),
                "Rhino image ID",
            )?
        } else {
            ctx.format_retained(
                format_args!("rhino:presentation:image#record-{source_offset}"),
                "Rhino image ID",
            )?
        },
        source_offset: cadmpeg_core::decode::u64_from_index(source_offset),
        source_uuid: source_uuid
            .map(|id| ctx.format_retained(format_args!("{id}"), "Rhino image source UUID"))
            .transpose()?,
        name,
        file_path,
        image_crc32,
        compression_method,
        uncompressed_byte_len,
        buffer_offset: cadmpeg_core::decode::u64_from_index(buffer_offset),
        buffer_byte_len: cadmpeg_core::decode::u64_from_index(buffer_end - buffer_offset),
        buffer_sha256: String::from(cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
            ctx,
            &data[buffer_offset..buffer_end],
            "Rhino image SHA-256",
        )?),
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
            if chunk.typecode != ANONYMOUS || chunk.short() || chunk.body().is_empty() {
                return Err(FramingError::structural(
                    reader.position(),
                    "Windows bitmap compressed buffer chunk is invalid",
                ));
            }
            reader.skip(chunk.next_offset() - reader.position())?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
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
        crate::settings::utf16_retained(ctx, &mut reader, "Rhino Windows bitmap path")?
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
        id: ctx.format_retained(
            format_args!("rhino:presentation:windows_bitmap#offset-{source_offset}"),
            "Rhino Windows bitmap ID",
        )?,
        source_offset: cadmpeg_core::decode::u64_from_index(source_offset),
        class_uuid: ctx.format_retained(
            format_args!("{class_uuid}"),
            "Rhino Windows bitmap class UUID",
        )?,
        file_path,
        header_size,
        width_pixels,
        height_pixels,
        planes,
        bits_per_pixel,
        compression,
        image_byte_len: i32::try_from(image_byte_len).map_err(|_| {
            FramingError::structural(reader.position(), "Windows bitmap image exceeds i32")
        })?,
        pixels_per_meter,
        colors_used,
        important_colors,
        pixel_buffer_offset: cadmpeg_core::decode::u64_from_index(pixel_buffer_offset),
        pixel_buffer_byte_len: cadmpeg_core::decode::u64_from_index(buffer.len()),
        pixel_buffer_sha256: String::from(
            cadmpeg_ir::hash::digest::Sha256Digest::digest_for_decode(
                ctx,
                buffer,
                "Rhino Windows bitmap SHA-256",
            )?,
        ),
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
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
    let name = crate::settings::utf16_retained(ctx, &mut reader, "Rhino texture mapping name")?;
    let object = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    let (primitive_class_uuid, cache_requires_opaque) = if object.short() {
        (None, false)
    } else {
        let mut warnings = Diagnostics::new();
        let (value, userdata) =
            parse_class_wrapper_with_userdata(ctx, data, object.range(), archive, &mut warnings)?;
        let mut cache_requires_opaque = false;
        ctx.charge_work(0, "Rhino parse texture mapping traversal")?;
        let mut source = userdata.iter();
        for _ in 0..source.len() {
            let raw = ctx.next_charged(&mut source, "Rhino parse texture mapping traversal")?
                .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
            let Some(value) = UserdataDescriptor::known(raw) else {
                continue;
            };
            if value.class_uuid == MAPPING_CRC_CACHE && value.item_uuid == MAPPING_CRC_CACHE
                && parse_mapping_crc_cache(data, value.payload_range.clone()).is_err()
            {
                cache_requires_opaque = true;
                break;
            }
        }
        (
            Some(ctx.format_retained(
                format_args!("{}", value.class_uuid),
                "Rhino texture mapping primitive UUID",
            )?),
            cache_requires_opaque,
        )
    };
    reader.skip(object.next_offset() - reader.position())?;
    let texture_space = if version.1 >= 1 { reader.u32()? } else { 0 };
    let capped = version.1 >= 1 && reader.bool()?;
    reader.skip_remaining()?;
    Ok(ParsedTextureMapping {
        value: TextureMappingRecord {
            id: if id.is_nil() {
                ctx.format_retained(
                    format_args!("rhino:presentation:texture_mapping#record-{source_offset}"),
                    "Rhino texture mapping ID",
                )?
            } else {
                ctx.format_retained(
                    format_args!("rhino:presentation:texture_mapping#{id}"),
                    "Rhino texture mapping ID",
                )?
            },
            source_offset: cadmpeg_core::decode::u64_from_index(source_offset),
            source_uuid: (!id.is_nil())
                .then(|| {
                    ctx.format_retained(format_args!("{id}"), "Rhino texture mapping source UUID")
                })
                .transpose()?,
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    start: usize,
    end: usize,
    archive: ArchiveVersion,
    retain_uuid: bool,
) -> Result<(RenderingMappingChannel, usize), FramingError> {
    let chunk = chunk_at(data, start, end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(FramingError::structural(
            start,
            "rendering mapping channel is not an anonymous long chunk",
        ));
    }
    let mut value = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    if value.i32()? != 1 {
        return Err(FramingError::structural(
            value.position() - 4,
            "rendering mapping channel version is unsupported",
        ));
    }
    let minor = value.i32()?;
    if minor < 0 {
        return Err(FramingError::structural(
            value.position() - 4,
            "rendering mapping channel minor version is negative",
        ));
    }
    let mapping_channel_id = value.i32()?;
    let mapping_uuid = uuid(&mut value)?;
    let object_transform = if minor >= 1 {
        Some(xform(&mut value)?)
    } else {
        None
    };
    value.skip_remaining()?;
    Ok((
        RenderingMappingChannel {
            mapping_channel_id,
            mapping_uuid: if retain_uuid {
                ctx.format_retained(
                    format_args!("{mapping_uuid}"),
                    "Rhino rendering channel UUID",
                )?
            } else {
                String::new()
            },
            object_transform,
        },
        chunk.next_offset(),
    ))
}

fn rendering_attributes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
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
        let mut presentation = RenderingAttributesPresentation {
            materials: ctx
                .collection_vec(material_count, "Rhino projected rendering materials")
                .map_err(crate::chunks::FramingError::from)?,
            ..RenderingAttributesPresentation::default()
        };
        for _ in 0..material_count {
            ctx.charge_work(1, "Rhino presentation cursor traversal")
                .map_err(FramingError::from)?;
            let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
            let parsed = (|| {
                let mut value = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
                if value.i32()? != 1 {
                    return Err(FramingError::structural(
                        value.position(),
                        "rendering-material version is unsupported",
                    ));
                }
                let minor = value.i32()?;
                if minor < 0 {
                    return Err(FramingError::structural(
                        value.position() - 4,
                        "rendering material minor version is negative",
                    ));
                }
                let plugin = uuid(&mut value)?;
                let plugin_uuid = ctx.format_retained(
                    format_args!("{plugin}"),
                    "Rhino rendering material plugin UUID",
                )?;
                let front = uuid(&mut value)?;
                let front_material_uuid = ctx.format_retained(
                    format_args!("{front}"),
                    "Rhino rendering front material UUID",
                )?;
                let obsolete_mapping_count = checked_count_bytes(
                    value.i32()?,
                    1,
                    value.remaining(),
                    1 << 16,
                    value.position() - 4,
                )?;
                for _ in 0..obsolete_mapping_count {
                    ctx.charge_work(1, "Rhino presentation cursor traversal")
                        .map_err(FramingError::from)?;
                    let (_, next_offset) = parse_rendering_mapping_channel(
                        ctx,
                        data,
                        value.position(),
                        value.end(),
                        archive,
                        false,
                    )?;
                    value.skip(next_offset - value.position())?;
                }
                let back_face = if minor >= 1 {
                    let id = uuid(&mut value)?;
                    let source = value.u8()?;
                    value.skip(3)?;
                    Some(RenderingMaterialBackFace {
                        back_material_uuid: (!id.is_nil())
                            .then(|| {
                                ctx.format_retained(
                                    format_args!("{id}"),
                                    "Rhino rendering back material UUID",
                                )
                            })
                            .transpose()?,
                        material_source: source,
                    })
                } else {
                    None
                };
                value.skip_remaining()?;
                Ok(RenderingMaterialReference {
                    plugin_uuid,
                    front_material_uuid,
                    back_face: RenderingMaterialBackFaceSlot(back_face),
                })
            })();
            presentation.materials.push(parsed?);
            reader.skip(chunk.next_offset() - reader.position())?;
        }
        if matches!(kind, settings::RenderingAttributesKind::Object) {
            let mapping_count = checked_count_bytes(
                reader.i32()?,
                1,
                reader.remaining(),
                1 << 16,
                reader.position() - 4,
            )?;
            presentation.mappings = ctx
                .collection_vec(mapping_count, "Rhino projected rendering mappings")
                .map_err(crate::chunks::FramingError::from)?;
            for _ in 0..mapping_count {
                ctx.charge_work(1, "Rhino presentation cursor traversal")
                    .map_err(FramingError::from)?;
                let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
                let mut value = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
                if value.i32()? != 1 {
                    return Err(FramingError::structural(
                        value.position() - 4,
                        "rendering mapping version is unsupported",
                    ));
                }
                let minor = value.i32()?;
                if minor < 0 {
                    return Err(FramingError::structural(
                        value.position() - 4,
                        "rendering mapping minor version is negative",
                    ));
                }
                let plugin = uuid(&mut value)?;
                let plugin_uuid = ctx.format_retained(
                    format_args!("{plugin}"),
                    "Rhino rendering mapping plugin UUID",
                )?;
                let channel_count = checked_count_bytes(
                    value.i32()?,
                    1,
                    value.remaining(),
                    1 << 16,
                    value.position() - 4,
                )?;
                let mut channels = ctx
                    .collection_vec(channel_count, "Rhino projected rendering channels")
                    .map_err(crate::chunks::FramingError::from)?;
                for _ in 0..channel_count {
                    ctx.charge_work(1, "Rhino presentation cursor traversal")
                        .map_err(FramingError::from)?;
                    let (channel, next_offset) = parse_rendering_mapping_channel(
                        ctx,
                        data,
                        value.position(),
                        value.end(),
                        archive,
                        true,
                    )?;
                    channels.push(channel);
                    value.skip(next_offset - value.position())?;
                }
                value.skip_remaining()?;
                presentation.mappings.push(RenderingMappingReference {
                    plugin_uuid,
                    channels,
                });
                reader.skip(chunk.next_offset() - reader.position())?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
) -> Result<FontRecord, FramingError> {
    let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(FramingError::structural(
            reader.position(),
            "font wrapper is invalid",
        ));
    }
    let mut value = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    let (major, minor) = (value.i32()?, value.i32()?);
    if major != 1 || minor < 0 {
        return Err(FramingError::structural(
            value.position(),
            "font version is unsupported",
        ));
    }
    let mut font = FontRecord {
        characteristics: value.u32()?,
        windows_logfont_name: wide_string(ctx, data, &mut value, archive)?,
        postscript_name: crate::settings::utf16_retained(
            ctx,
            &mut value,
            "Rhino font PostScript name",
        )?,
        ..FontRecord::default()
    };
    if minor >= 1 {
        font.obsolete_description =
            crate::settings::utf16_retained(ctx, &mut value, "Rhino font obsolete description")?;
    }
    if minor >= 2 {
        font.weight = FontWeight::Modern {
            windows: value.i32()?,
            apple: read_finite(ctx, &mut value, "Apple font weight trait")?,
        };
    }
    if minor >= 3 {
        font.point_size = Some(read_finite(ctx, &mut value, "font point size")?);
        if value.bool_with_writer_version(writer_version)? {
            value.skip(4 + 16)?;
        }
    }
    if minor >= 4 {
        font.family_name =
            crate::settings::utf16_retained(ctx, &mut value, "Rhino font family name")?;
    }
    if minor >= 5 {
        font.locale_name =
            crate::settings::utf16_retained(ctx, &mut value, "Rhino font locale name")?;
        font.localized_postscript_name = crate::settings::utf16_retained(
            ctx,
            &mut value,
            "Rhino font localized PostScript name",
        )?;
        font.english_postscript_name =
            crate::settings::utf16_retained(ctx, &mut value, "Rhino font English PostScript name")?;
        font.localized_logfont_name =
            crate::settings::utf16_retained(ctx, &mut value, "Rhino font localized LOGFONT name")?;
        font.english_logfont_name =
            crate::settings::utf16_retained(ctx, &mut value, "Rhino font English LOGFONT name")?;
        font.localized_family_name =
            crate::settings::utf16_retained(ctx, &mut value, "Rhino font localized family name")?;
        font.english_family_name =
            crate::settings::utf16_retained(ctx, &mut value, "Rhino font English family name")?;
        font.localized_face_name =
            crate::settings::utf16_retained(ctx, &mut value, "Rhino font localized face name")?;
        font.english_face_name =
            crate::settings::utf16_retained(ctx, &mut value, "Rhino font English face name")?;
        let panose = chunk_at(data, value.position(), value.end(), archive, false)?;
        if panose.typecode != ANONYMOUS || panose.short() {
            return Err(FramingError::structural(
                value.position(),
                "font PANOSE wrapper is invalid",
            ));
        }
        let mut bytes = BoundedReader::new(data, panose.body().start, panose.body().end)?;
        if bytes.u8()? != 0x10 || bytes.remaining() != 10 {
            return Err(FramingError::structural(
                bytes.position(),
                "font PANOSE version is unsupported",
            ));
        }
        font.panose = Some(bytes.array()?);
        value.skip(panose.next_offset() - value.position())?;
    }
    if minor >= 6 {
        font.quartet_member = Some(value.u8()?);
    }
    value.skip_remaining()?;
    reader.skip(chunk.next_offset() - reader.position())?;
    Ok(font)
}

struct TextStyleParseInput {
    range: Range<usize>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    apple_runtime: bool,
    source_offset: usize,
}

fn parse_text_style(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    input: TextStyleParseInput,
    losses: &mut impl AdmittedVec<LossNote>,
) -> Result<TextStyleRecord, FramingError> {
    let TextStyleParseInput {
        range,
        archive,
        writer_version,
        apple_runtime,
        source_offset,
    } = input;
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
        let description = crate::settings::utf16_retained(
            ctx,
            &mut reader,
            "Rhino legacy text style description",
        )?;
        let face_bytes = reader.take(128)?;
        let windows_logfont_name =
            ctx.utf16le_lossy_text(face_bytes, 64, true, "Rhino legacy font face")?;
        let named_description = !description.is_empty() && !description.eq_ignore_ascii_case("Default");
        let postscript_name = if named_description
            && (apple_runtime || writer_version.is_some_and(|version| version > 201_802_230))
        {
            ctx.copy_retained_text(&description, "Rhino legacy PostScript name")?
        } else {
            if named_description && !apple_runtime && writer_version.is_none() {
                losses.push_with_storage_admitted(
                    ctx,
                    || crate::wire::admitted_loss(
                        ctx,
                        RhinoLossCode::SourceWriterStampUnverified,
                        format_args!("legacy text style at offset {source_offset} dropped the PostScript font name \"{description}\" because the archive has no writer-version stamp"),
                        "Rhino text style writer-stamp loss text",
                    ),
                    "Rhino text style writer-stamp losses",
                )
                .map_err(crate::chunks::FramingError::from)?;
            }
            String::new()
        };
        let mut font = FontRecord {
            windows_logfont_name,
            postscript_name,
            obsolete_description: ctx
                .copy_retained_text(&description, "Rhino legacy font description")?,
            ..FontRecord::default()
        };
        if packed & 0x0f >= 1 {
            let windows = reader.i32()?;
            let italic = reader.i32()?;
            if !matches!(italic, 0 | 1) {
                return Err(FramingError::structural(
                    reader.position() - 4,
                    "legacy font italic flag is invalid",
                ));
            }
            let _linefeed_ratio = read_finite(ctx, &mut reader, "legacy font linefeed ratio")?;
            font.weight = FontWeight::Legacy {
                windows,
                italic: italic != 0,
            };
        }
        let id = if packed & 0x0f >= 2 {
            uuid(&mut reader)?
        } else {
            Uuid::nil()
        };
        reader.skip_remaining()?;
        return Ok(TextStyleRecord {
            id: ctx.format_retained(
                format_args!("rhino:presentation:text_style#index-{index}-offset-{source_offset}"),
                "Rhino text style ID",
            )?,
            source_offset: cadmpeg_core::decode::u64_from_index(source_offset),
            archive_index: Some(index),
            source_uuid: (!id.is_nil())
                .then(|| ctx.format_retained(format_args!("{id}"), "Rhino text style source UUID"))
                .transpose()?,
            name: ctx.copy_retained_text(&description, "Rhino text style name")?,
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
    let component = component(ctx, data, &mut reader, archive)?;
    let font_description = if reader.bool_with_writer_version(writer_version)? {
        crate::settings::utf16_retained(ctx, &mut reader, "Rhino text style font description")?
    } else {
        String::new()
    };
    let font = if reader.bool_with_writer_version(writer_version)? {
        parse_font(ctx, data, &mut reader, archive, writer_version)?
    } else {
        FontRecord::default()
    };
    let (id, name) = if version.1 >= 1 {
        (
            uuid(&mut reader)?,
            crate::settings::utf16_retained(ctx, &mut reader, "Rhino text style name")?,
        )
    } else {
        (component.id, component.name)
    };
    reader.skip_remaining()?;
    let index = component.index;
    Ok(TextStyleRecord {
        id: if id.is_nil() {
            index.map_or_else(
                || {
                    ctx.format_retained(
                        format_args!("rhino:presentation:text_style#offset-{source_offset}"),
                        "Rhino text style ID",
                    )
                },
                |index| {
                    ctx.format_retained(
                        format_args!(
                            "rhino:presentation:text_style#index-{index}-offset-{source_offset}"
                        ),
                        "Rhino text style ID",
                    )
                },
            )
        } else {
            ctx.format_retained(
                format_args!("rhino:presentation:text_style#{id}"),
                "Rhino text style ID",
            )
        }?,
        source_offset: cadmpeg_core::decode::u64_from_index(source_offset),
        archive_index: index,
        source_uuid: (!id.is_nil())
            .then(|| ctx.format_retained(format_args!("{id}"), "Rhino text style source UUID"))
            .transpose()?,
        name,
        font_description,
        font,
    })
}

/// Results of transferring table-owned presentation records.
fn retain_unbound_presentation_record(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    losses: &mut impl AdmittedVec<LossNote>,
    opaque_records: &mut impl AdmittedVec<OpaqueRecord>,
    table_typecode: u32,
    record: &Record,
    binding: UnitBinding,
    kind: &str,
) -> Result<(), CodecError> {
    push_presentation_loss(ctx, losses, RhinoLossCode::PresentationRecordDropped, format_args!(
        "{kind} record at offset {} was retained as complete source because the document has no physical millimetre binding ({})",
        record.range.start,
        binding.label()
    ))?;
    push_opaque_record(ctx, opaque_records, table_typecode, record)?;
    Ok(())
}

/// Stages one object's group membership. Keys, link slots and link text stay
/// scoped through native serialization, which copies the group records.
fn admit_group_member(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    workspace: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    group_members: &mut HashMap<i32, Vec<String>>,
    group: i32,
    source_order: usize,
) -> Result<(), CodecError> {
    workspace.with_storage(|| {
        let members = ctx
            .entry_hash_map(group_members, group, "Rhino group member keys")
            .map(std::collections::hash_map::Entry::or_default)?;
        let link = ctx.format_retained(
            format_args!("rhino:object:record#{source_order:06}"),
            "Rhino group member link",
        )?;
        ctx.push_vec(members, link, "Rhino group member links")
    })
}

fn retain_presentation_record_storage<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    guard_storage: &mut cadmpeg_core::decode::ScopedReservation<'ctx>,
    guards: &mut Vec<cadmpeg_core::decode::ScopedReservation<'ctx>>,
    guard: cadmpeg_core::decode::ScopedReservation<'ctx>,
) -> Result<(), CodecError> {
    guard_storage.with_storage(|| {
        ctx.reserve_vec(guards, 1, "Rhino presentation record guards")
    })?;
    guards.push(guard);
    Ok(())
}

pub(crate) fn install<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    scan: &Scan<'_>,
    ir: &mut CadIr,
) -> Result<NativeInstall<'ctx>, CodecError> {
    let binding = UnitBinding::from_units(scan.metadata.settings.units.as_ref());
    let physical_scale = binding.neutral_scale();
    let mut groups_storage = ctx.reserve_scoped(0, "Rhino group staging records")?;
    let mut group_scope_storage = ctx.reserve_scoped(0, "Rhino group staging reservations")?;
    let mut staging = ctx.reserve_scoped(0, "Rhino presentation staging records")?;
    let mut record_guard_storage =
        ctx.reserve_scoped(0, "Rhino presentation record guard storage")?;
    let mut group_member_workspace;
    let mut group_storages = Vec::new();
    let mut groups = Vec::new();
    let mut record_storages = Vec::new();
    let mut materials = Vec::new();
    let mut lights = Vec::new();
    let mut light_index_workspace = ctx.reserve_scoped(0, "Rhino light identity workspace")?;
    let mut light_identities = HashSet::new();
    let mut linetypes = Vec::new();
    let mut hatch_patterns = Vec::new();
    let mut dimension_styles = Vec::new();
    let mut images = Vec::new();
    let mut windows_bitmaps = Vec::new();
    let mut texture_mappings = Vec::new();
    let mut text_styles = Vec::new();
    let mut layers = Vec::new();
    let mut object_presentation = Vec::new();
    let mut object_count_workspace = ctx.reserve_scoped(0, "Rhino object identity workspace")?;
    let mut object_id_counts = HashMap::<Uuid, usize>::new();
    let mut losses = ScratchVec::new(ctx, "Rhino presentation loss Vec")?;
    let mut opaque_records = ScratchVec::new(ctx, "Rhino presentation source Vec")?;
    let mut apple_runtime = None;
    ctx.charge_work(0, "Rhino install traversal")?;
    let mut projection_source = scan.objects[..].iter();
    for _ in 0..projection_source.len() {
        let object = ctx.next_charged(&mut projection_source, "Rhino install traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        if let Some(identity) = object.identity() {
            let count = object_count_workspace
                .with_storage(|| {
                    ctx.entry_hash_map(
                        &mut object_id_counts,
                        identity.object_id,
                        "Rhino object identity counts",
                    )
                })?
                .or_default();
            *count += 1;
        }
    }
    ctx.charge_work(0, "Rhino install traversal")?;
    let mut projection_source = scan.tables[..].iter();
    for _ in 0..projection_source.len() {
        let table = ctx.next_charged(&mut projection_source, "Rhino install traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        let table_type = table.typecode & !0x0000_8000;
        ctx.charge_work(0, "Rhino install traversal")?;
        let mut projection_source = table.records[..].iter();
        for _ in 0..projection_source.len() {
            let record = ctx.next_charged(&mut projection_source, "Rhino install traversal")?
                .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
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
                if let Some(range) =
                    optional_malformed(class_data(ctx, scan.data, record, scan.archive, GROUP))?
                {
                    match parse_group(ctx, scan.data, range, record.range.start) {
                        Ok((group, group_storage)) => {
                            if let Err(error) = groups_storage
                                .with_storage(|| ctx.reserve_vec(&mut groups, 1, "Rhino groups"))
                            {
                                drop(group);
                                drop(group_storage);
                                return Err(error);
                            }
                            if let Err(error) = group_scope_storage.with_storage(|| {
                                ctx.reserve_vec(
                                    &mut group_storages,
                                    1,
                                    "Rhino group staging reservations",
                                )
                            }) {
                                drop(group);
                                drop(group_storage);
                                return Err(error);
                            }
                            groups.push(group);
                            group_storages.push(group_storage);
                            parsed = true;
                        }
                        Err(FramingError::Resource(limit)) => return Err(CodecError::ResourceLimit(limit)),
                        Err(_) => {}
                    }
                }
            } else if table_type == MATERIAL_TABLE {
                if let Some((range, userdata)) = optional_malformed(class_data_with_userdata(
                    ctx,
                    scan.data,
                    record,
                    scan.archive,
                    MATERIAL,
                ))? {
                    let mut material_requires_opaque = false;
                    let legacy_rdk_instance_id =
                        legacy_rdk_material_instance_id(ctx, scan.data, &userdata)?;
                    if rdk_material_userdata_requires_opaque(ctx, scan.data, &userdata)? {
                        material_requires_opaque = true;
                        push_presentation_loss(ctx, &mut losses, RhinoLossCode::PresentationRecordDropped, format_args!(
                            "RDK material userdata at offset {} could not be transferred: callback-owned or unsupported payload",
                            record.range.start
                        ))?;
                    }
                    let mut physically_based_userdata = None;
                    ctx.charge_work(0, "Rhino PBR userdata search")?;
                    let mut source = userdata.iter();
                    for _ in 0..source.len() {
                        let raw = ctx.next_charged(&mut source, "Rhino PBR userdata search")?
                            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
                        if let Some(value) = raw.known().filter(|value| {
                            value.class_uuid == PHYSICALLY_BASED_MATERIAL_USERDATA
                                && value.item_uuid == PHYSICALLY_BASED_MATERIAL_USERDATA
                                && (value.application_uuid.is_none()
                                    || value.application_uuid == Some(OPENNURBS6_APPLICATION))
                        }) {
                            physically_based_userdata = Some(value);
                            break;
                        }
                    }
                    let physically_based = if let Some(value) = physically_based_userdata {
                        match parse_physically_based_material(
                            ctx,
                            scan.data,
                            value.payload_range.clone(),
                            scan.archive,
                        ) {
                            Ok(material) => Some(material),
                            Err(FramingError::Resource(limit)) => {
                                return Err(CodecError::ResourceLimit(limit))
                            }
                            Err(error) => {
                                material_requires_opaque = true;
                                push_presentation_loss(ctx, &mut losses, RhinoLossCode::PresentationRecordDropped, format_args!(
                                    "physically based material userdata at offset {} could not be transferred: {error}",
                                    record.range.start
                                ))?;
                                None
                            }
                        }
                    } else {
                        None
                    };
                    let mut record_losses =
                        ScratchVec::new(ctx, "Rhino material parse loss Vec")?;
                    let (parsed_material, mut record_storage) = ctx.with_scoped_storage(
                        "Rhino material record staging",
                        || {
                            Ok::<_, CodecError>(parse_material(
                                ctx,
                                scan.data,
                                MaterialParseInput {
                                    range,
                                    archive: scan.archive,
                                    writer_version: scan.metadata.properties.writer_version,
                                    source_offset: record.range.start,
                                    physically_based,
                                },
                                &mut record_losses,
                            ))
                        },
                    )?;
                    match parsed_material {
                        Ok(mut material) => {
record_losses.append_admitted(
                                ctx,
                                &mut losses,
                                "Rhino material parse loss copies",
                            )?;
                            if let Some(instance_id) = legacy_rdk_instance_id {
                                record_storage.with_storage(|| {
                                    material.plugin_uuid = ctx.format_retained(
                                        format_args!("{UNIVERSAL_RENDER_ENGINE}"),
                                        "Rhino material render-engine UUID",
                                    )?;
                                    material.rdk_instance_uuid = Some(ctx.format_retained(
                                        format_args!("{instance_id}"),
                                        "Rhino material instance UUID",
                                    )?);
                                    Ok::<_, CodecError>(())
                                })?;
                            }
                            staging.with_storage(|| {
                                ctx.reserve_vec(&mut materials, 1, "Rhino materials")
                            })?;
                            materials.push(material);
                            retain_presentation_record_storage(
                                ctx,
                                &mut record_guard_storage,
                                &mut record_storages,
                                record_storage,
                            )?;
                            if material_requires_opaque {
                                push_opaque_record(
                                    ctx,
                                    &mut opaque_records,
                                    table.typecode,
                                    record,
                                )?;
                            }
                            parsed = true;
                        }
                        Err(FramingError::Resource(limit)) => {
                            drop(record_storage);
                            return Err(CodecError::ResourceLimit(limit));
                        }
                        Err(_) => {
                            drop(record_storage);
record_losses.append_admitted(
                                ctx,
                                &mut losses,
                                "Rhino material parse loss copies",
                            )?;
                        }
                    }
                }
            } else if table_type == LIGHT_TABLE {
                let Some(scale) = physical_scale else {
                    retain_unbound_presentation_record(
                        ctx,
                        &mut losses,
                        &mut opaque_records,
                        table.typecode,
                        record,
                        binding,
                        "light",
                    )?;
                    continue;
                };
                if let Some(range) = optional_malformed(class_data_prefix(
                    ctx,
                    scan.data,
                    record,
                    scan.archive,
                    LIGHT,
                ))? {
                    let mut light_storage =
                        ctx.reserve_scoped(0, "Rhino light record staging")?;
                    let light = light_storage.with_storage(|| {
                        optional_malformed(parse_light(
                            ctx,
                            scan.data,
                            range,
                            scale,
                            record.range.start,
                            None,
                        ))
                    })?;
                    let mut attribute_storage;
                    if let Some(mut light) = light {
                        let mut attribute_losses =
                            ScratchVec::new(ctx, "Rhino light attribute loss Vec")?;
                        let mut parsing_storage =
                            ctx.reserve_scoped(0, "Rhino light attribute staging")?;
                        let parsed_attributes = parsing_storage.with_storage(|| {
                                parse_light_record_attributes(
                                    ctx,
                                    scan.data,
                                    record,
                                    scan.archive,
                                    scan.metadata.properties.writer_version,
                                    &mut attribute_losses,
                                )
                            });
                        attribute_storage = Some(parsing_storage);
                        match parsed_attributes {
                            Ok(attributes) => {
attribute_losses.append_admitted(
                                    ctx,
                                    &mut losses,
                                    "Rhino light attribute loss copies",
                                )?;
                                if let Some(value) = attributes {
                                    if value.userdata_requires_opaque {
                                        push_opaque_record(
                                            ctx,
                                            &mut opaque_records,
                                            table.typecode,
                                            record,
                                        )?;
                                    }
                                    light.attributes = Some(value);
                                } else {
                                    drop(attribute_storage.take());
                                }
                            }
                            Err(FramingError::Resource(limit)) => {
                                return Err(CodecError::ResourceLimit(limit));
                            }
                            Err(error) => {
                                attribute_losses.append_admitted(
                                    ctx,
                                    &mut losses,
                                    "Rhino light attribute loss copies",
                                )?;
                                push_presentation_loss(
                                    ctx,
                                    &mut losses,
                                    RhinoLossCode::ObjectAttributesDegraded,
                                    format_args!(
                                        "light attributes at offset {} could not be transferred: {error}",
                                        record.range.start
                                    ),
                                )?;
                                drop(error);
                                drop(attribute_storage.take());
                                push_opaque_record(
                                    ctx,
                                    &mut opaque_records,
                                    table.typecode,
                                    record,
                                )?;
                            }
                        }
                        light_storage.with_storage(|| {
                            push_light(
                                ctx,
                                &mut light_index_workspace,
                                &mut staging,
                                &mut lights,
                                &mut light_identities,
                                light,
                            )
                        })?;
                        retain_presentation_record_storage(
                            ctx,
                            &mut record_guard_storage,
                            &mut record_storages,
                            light_storage,
                        )?;
                        if let Some(storage) = attribute_storage.take() {
                            retain_presentation_record_storage(
                                ctx,
                                &mut record_guard_storage,
                                &mut record_storages,
                                storage,
                            )?;
                        }
                        parsed = true;
                    } else {
                        drop(light_storage);
                    }
                }
            } else if table_type == LINETYPE_TABLE {
                if let Some(range) =
                    optional_malformed(class_data(ctx, scan.data, record, scan.archive, LINETYPE))?
                {
                    match ctx.with_scoped_storage("Rhino linetype staging", || {
                        parse_linetype(
                            ctx,
                            scan.data,
                            range,
                            scan.archive,
                            binding,
                            record.range.start,
                        )
                    }) {
                        Ok((value, storage)) => {
                            staging.with_storage(|| {
                                ctx.reserve_vec(&mut linetypes, 1, "Rhino linetypes")
                            })?;
                            linetypes.push(value);
                            retain_presentation_record_storage(
                                ctx,
                                &mut record_guard_storage,
                                &mut record_storages,
                                storage,
                            )?;
                        }
                        Err(PatternTransferError::Framing(FramingError::Resource(limit))) => {
                            return Err(CodecError::ResourceLimit(limit));
                        }
                        Err(error) => {
                            push_presentation_loss(ctx, &mut losses, RhinoLossCode::PresentationRecordDropped, format_args!(
                                "linetype record at offset {} was retained as complete source: {error}",
                                record.range.start,
                            ))?;
                            push_opaque_record(ctx, &mut opaque_records, table.typecode, record)?;
                        }
                    }
                    parsed = true;
                }
            } else if table_type == HATCH_PATTERN_TABLE {
                if let Some(range) = optional_malformed(class_data(
                    ctx,
                    scan.data,
                    record,
                    scan.archive,
                    HATCH_PATTERN,
                ))? {
                    match ctx.with_scoped_storage("Rhino hatch-pattern staging", || {
                        parse_hatch_pattern(
                            ctx,
                            scan.data,
                            range,
                            scan.archive,
                            binding,
                            record.range.start,
                        )
                    }) {
                        Ok((value, storage)) => {
                            staging.with_storage(|| {
                                ctx.reserve_vec(&mut hatch_patterns, 1, "Rhino hatch patterns")
                            })?;
                            hatch_patterns.push(value);
                            retain_presentation_record_storage(
                                ctx,
                                &mut record_guard_storage,
                                &mut record_storages,
                                storage,
                            )?;
                        }
                        Err(PatternTransferError::Framing(FramingError::Resource(limit))) => {
                            return Err(CodecError::ResourceLimit(limit));
                        }
                        Err(error) => {
                            push_presentation_loss(ctx, &mut losses, RhinoLossCode::PresentationRecordDropped, format_args!(
                                "hatch pattern record at offset {} was retained as complete source: {error}",
                                record.range.start,
                            ))?;
                            push_opaque_record(ctx, &mut opaque_records, table.typecode, record)?;
                        }
                    }
                    parsed = true;
                }
            } else if table_type == DIMSTYLE_TABLE {
                let Some(scale) = physical_scale else {
                    retain_unbound_presentation_record(
                        ctx,
                        &mut losses,
                        &mut opaque_records,
                        table.typecode,
                        record,
                        binding,
                        "dimension style",
                    )?;
                    continue;
                };
                if scan.archive.value() < 60 {
                    let mut extra_requires_opaque = false;
                    if let Some((range, userdata)) = optional_malformed(class_data_with_userdata(
                        ctx,
                        scan.data,
                        record,
                        scan.archive,
                        V5_DIMSTYLE,
                    ))? {
                        let mut extra = None;
                        ctx.charge_work(0, "Rhino dimension style userdata search")?;
                        let mut source = userdata.iter();
                        for _ in 0..source.len() {
                            let raw = ctx.next_charged(&mut source, "Rhino dimension style userdata search")?
                                .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
                            if let Some(value) = raw.known().filter(|value| {
                                value.class_uuid == DIMSTYLE_EXTRA && value.item_uuid == DIMSTYLE_EXTRA
                            }) {
                                extra = Some(value);
                                break;
                            }
                        }
                        let mut extra_storage = None;
                        let extra = match extra {
                            Some(value) => {
                                let (parsed_extra, storage) = ctx.with_scoped_storage(
                                    "Rhino V5 dimension-style extra staging",
                                    || {
                                        Ok::<_, CodecError>(parse_v5_dimension_style_extra(
                                            ctx,
                                            scan.data,
                                            value,
                                            scan.archive,
                                            scale,
                                        ))
                                    },
                                )?;
                                match parsed_extra {
                                    Ok(extra) => {
                                        extra_storage = Some(storage);
                                        Some(extra)
                                    }
                                    Err(FramingError::Resource(limit)) => {
                                        return Err(CodecError::ResourceLimit(limit));
                                    }
                                    Err(error) => {
                                        drop(storage);
                                        extra_requires_opaque = true;
                                        push_presentation_loss(ctx, &mut losses, RhinoLossCode::PresentationRecordDropped, format_args!(
                                            "V5 dimension-style userdata at offset {} could not be transferred: {error}",
                                            record.range.start
                                        ))?;
                                        None
                                    }
                                }
                            }
                            None => None,
                        };
                        let (value, storage) = ctx.with_scoped_storage(
                            "Rhino V5 dimension-style staging",
                            || {
                                optional_malformed(parse_v5_dimension_style(
                                    ctx,
                                    scan.data,
                                    range,
                                    scale,
                                    record.range.start,
                                    extra,
                                ))
                            },
                        )?;
                        if let Some(value) = value {
                            staging.with_storage(|| {
                                ctx.reserve_vec(&mut dimension_styles, 1, "Rhino dimension styles")
                            })?;
                            dimension_styles.push(value);
                            retain_presentation_record_storage(
                                ctx,
                                &mut record_guard_storage,
                                &mut record_storages,
                                storage,
                            )?;
                            if let Some(extra_storage) = extra_storage {
                                retain_presentation_record_storage(
                                    ctx,
                                    &mut record_guard_storage,
                                    &mut record_storages,
                                    extra_storage,
                                )?;
                            }
                            if extra_requires_opaque {
                                push_opaque_record(
                                    ctx,
                                    &mut opaque_records,
                                    table.typecode,
                                    record,
                                )?;
                            }
                            parsed = true;
                        }
                    }
                } else if let Some(range) =
                    optional_malformed(class_data(ctx, scan.data, record, scan.archive, DIMSTYLE))?
                {
                    let (value, storage) = ctx.with_scoped_storage(
                        "Rhino dimension-style staging",
                        || {
                            optional_malformed(parse_dimension_style(
                                ctx,
                                scan.data,
                                range,
                                scan.archive,
                                scale,
                                record.range.start,
                            ))
                        },
                    )?;
                    if let Some(value) = value {
                        staging.with_storage(|| {
                            ctx.reserve_vec(&mut dimension_styles, 1, "Rhino dimension styles")
                        })?;
                        dimension_styles.push(value);
                        retain_presentation_record_storage(
                            ctx,
                            &mut record_guard_storage,
                            &mut record_storages,
                            storage,
                        )?;
                        parsed = true;
                    }
                }
            } else if table_type == BITMAP_TABLE {
                if let Some(range) = optional_malformed(class_data(
                    ctx,
                    scan.data,
                    record,
                    scan.archive,
                    EMBEDDED_BITMAP,
                ))? {
                    let (value, storage) = ctx.with_scoped_storage(
                        "Rhino embedded image staging",
                        || {
                            optional_malformed(parse_embedded_image(
                                ctx,
                                scan.data,
                                range,
                                scan.archive,
                                record.range.start,
                            ))
                        },
                    )?;
                    if let Some(value) = value {
                        staging.with_storage(|| ctx.reserve_vec(&mut images, 1, "Rhino images"))?;
                        images.push(value);
                        retain_presentation_record_storage(
                            ctx,
                            &mut record_guard_storage,
                            &mut record_storages,
                            storage,
                        )?;
                        parsed = true;
                    }
                } else if let Some(class) = optional_malformed(parse_class_wrapper(
                    ctx,
                    scan.data,
                    record.body(),
                    scan.archive,
                    &mut Diagnostics::new(),
                ))? {
                    if matches!(class.class_uuid, WINDOWS_BITMAP | WINDOWS_BITMAP_EX) {
                        let (value, storage) = ctx.with_scoped_storage(
                            "Rhino Windows bitmap staging",
                            || {
                                optional_malformed(parse_windows_bitmap(
                                    ctx,
                                    scan.data,
                                    class.class_data_range,
                                    class.class_uuid,
                                    scan.archive,
                                    record.range.start,
                                ))
                            },
                        )?;
                        if let Some(value) = value {
                            staging.with_storage(|| {
                                ctx.reserve_vec(
                                    &mut windows_bitmaps,
                                    1,
                                    "Rhino Windows bitmaps",
                                )
                            })?;
                            windows_bitmaps.push(value);
                            retain_presentation_record_storage(
                                ctx,
                                &mut record_guard_storage,
                                &mut record_storages,
                                storage,
                            )?;
                            parsed = true;
                        }
                    }
                }
            } else if table_type == TEXTURE_MAPPING_TABLE {
                if let Some(range) = optional_malformed(class_data(
                    ctx,
                    scan.data,
                    record,
                    scan.archive,
                    TEXTURE_MAPPING,
                ))? {
                    let (value, storage) = ctx.with_scoped_storage(
                        "Rhino texture-mapping staging",
                        || {
                            optional_malformed(parse_texture_mapping(
                                ctx,
                                scan.data,
                                range,
                                scan.archive,
                                record.range.start,
                            ))
                        },
                    )?;
                    if let Some(value) = value {
                        staging.with_storage(|| {
                            ctx.reserve_vec(&mut texture_mappings, 1, "Rhino texture mappings")
                        })?;
                        texture_mappings.push(value.value);
                        retain_presentation_record_storage(
                            ctx,
                            &mut record_guard_storage,
                            &mut record_storages,
                            storage,
                        )?;
                        if value.cache_requires_opaque {
                            push_presentation_loss(
                                ctx,
                                &mut losses,
                                RhinoLossCode::PresentationRecordDropped,
                                format_args!(
                                "MappingCRCCache userdata at offset {} could not be transferred",
                                record.range.start
                            ),
                            )?;
                            push_opaque_record(ctx, &mut opaque_records, table.typecode, record)?;
                        }
                        parsed = true;
                    }
                }
            } else if table_type == FONT_TABLE {
                if let Some(range) = optional_malformed(class_data(
                    ctx,
                    scan.data,
                    record,
                    scan.archive,
                    TEXT_STYLE,
                ))? {
                    let mut record_losses =
                        ScratchVec::new(ctx, "Rhino text-style parse loss Vec")?;
                    let (parsed_text_style, record_storage) = ctx.with_scoped_storage(
                        "Rhino text-style record staging",
                        || {
                            Ok::<_, CodecError>(parse_text_style(
                                ctx,
                                scan.data,
                                TextStyleParseInput {
                                    range,
                                    archive: scan.archive,
                                    writer_version: scan.metadata.properties.writer_version,
                                    apple_runtime: match apple_runtime {
                                        Some(value) => value,
                                        None => {
                                            let value = match &scan.metadata.properties.application {
                                                Some(application) => {
                                                    let mut is_apple = false;
                                                    ctx.charge_work(0, "Rhino font application search")?;
                                                    let mut source = application.name.as_bytes().windows(3);
                                                    for _ in 0..source.len() {
                                                        let part = ctx.next_charged(&mut source, "Rhino font application search")?
                                                            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
                                                        if part.eq_ignore_ascii_case(b"mac") {
                                                            is_apple = true;
                                                            break;
                                                        }
                                                    }
                                                    is_apple
                                                }
                                                None => false,
                                            };
                                            apple_runtime = Some(value);
                                            value
                                        }
                                    },
                                    source_offset: record.range.start,
                                },
                                &mut record_losses,
                            ))
                        },
                    )?;
                    match parsed_text_style {
                        Ok(value) => {
record_losses.append_admitted(
                                ctx,
                                &mut losses,
                                "Rhino text-style parse loss copies",
                            )?;
                            staging.with_storage(|| {
                                ctx.reserve_vec(&mut text_styles, 1, "Rhino text styles")
                            })?;
                            text_styles.push(value);
                            retain_presentation_record_storage(
                                ctx,
                                &mut record_guard_storage,
                                &mut record_storages,
                                record_storage,
                            )?;
                            parsed = true;
                        }
                        Err(FramingError::Resource(limit)) => {
                            drop(record_storage);
                            return Err(CodecError::ResourceLimit(limit))
                        }
                        Err(_) => {
                            drop(record_storage);
record_losses.append_admitted(
                                ctx,
                                &mut losses,
                                "Rhino text-style parse loss copies",
                            )?;
                        }
                    }
                }
            }
            if recognized && !parsed {
                push_presentation_loss(
                    ctx,
                    &mut losses,
                    RhinoLossCode::PresentationRecordDropped,
                    format_args!(
                        "record at offset {} in table {table_type:#x} could not be transferred",
                        record.range.start
                    ),
                )?;
                push_opaque_record(ctx, &mut opaque_records, table.typecode, record)?;
            }
        }
    }
    let (group_index_workspace, group_index_counts) = crate::settings::index_occurrences(
        ctx,
        &groups,
        |group| group.archive_index,
        "Rhino group index counts",
    ).map(|(value, storage)| (storage, value))?;
    let mut group_members = HashMap::<i32, Vec<String>>::new();
    group_member_workspace = ctx.reserve_scoped(0, "Rhino group member workspace")?;
    ctx.charge_work(0, "Rhino install traversal")?;
    let mut projection_source = scan.objects[..].iter();
    for source_order in 0..projection_source.len() {
        let object = ctx.next_charged(&mut projection_source, "Rhino install traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        let Some(object) = object.framed() else {
            continue;
        };
        if let Some(attributes) = object.attributes.parsed() {
            ctx.charge_work(0, "Rhino install traversal")?;
            let mut projection_source = attributes.groups[..].iter();
            for _ in 0..projection_source.len() {
                let group = ctx.next_charged(&mut projection_source, "Rhino install traversal")?
                    .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
                let unique = ctx
                    .binary_search_by(
                        &group_index_counts,
                        |(index, _)| Ok(index.cmp(group)),
                        "Rhino group membership index lookup",
                    )?
                    .ok()
                    .is_some_and(|index| group_index_counts[index].1 == 1);
                if !unique {
                    continue;
                }
                admit_group_member(
                    ctx,
                    &mut group_member_workspace,
                    &mut group_members,
                    *group,
                    source_order,
                )?;
            }
        }
        if object.class_uuid == LIGHT {
            if let Some(scale) = physical_scale {
                let mut light_storage =
                    ctx.reserve_scoped(0, "Rhino light object staging")?;
                match light_storage.with_storage(|| {
                    parse_light(
                        ctx,
                        scan.data,
                        object.class_data_range.clone(),
                        scale,
                        object.range.start,
                        Some(source_order),
                    )
                }) {
                    Ok(light) => {
                        light_storage.with_storage(|| {
                            push_light(
                                ctx,
                                &mut light_index_workspace,
                                &mut staging,
                                &mut lights,
                                &mut light_identities,
                                light,
                            )
                        })?;
                        retain_presentation_record_storage(
                            ctx,
                            &mut record_guard_storage,
                            &mut record_storages,
                            light_storage,
                        )?;
                    }
                    Err(FramingError::Resource(limit)) => {
                        drop(light_storage);
                        return Err(CodecError::ResourceLimit(limit));
                    }
                    Err(error) => {
                        push_presentation_loss(
                            ctx,
                            &mut losses,
                            RhinoLossCode::PresentationRecordDropped,
                            format_args!(
                                "light object at offset {} could not be transferred: {error}",
                                object.range.start
                            ),
                        )?;
                        drop(error);
                        drop(light_storage);
                    }
                }
            } else {
                push_presentation_loss(ctx, &mut losses, RhinoLossCode::PresentationRecordDropped, format_args!(
                    "light object at offset {} was retained as complete source because the document has no physical millimetre binding ({})",
                    object.range.start,
                    binding.label()
                ))?;
            }
        }
        if let Some(attributes) = object.attributes.parsed() {
            let identity = &object.identity;
            let mut record_losses =
                ScratchVec::new(ctx, "Rhino object presentation loss Vec")?;
            let mut record_storage =
                ctx.reserve_scoped(0, "Rhino object presentation record")?;
            let attributes_presentation = record_storage.with_storage(|| {
                object_attributes_presentation(
                    ctx,
                    scan.data,
                    attributes,
                    &object.userdata,
                    &object.attributes_userdata,
                    ObjectPresentationSource {
                        archive: scan.archive,
                        offset: object.range.start,
                        uuid: identity.object_id,
                    },
                    &mut record_losses,
                )
            })?;
record_losses.append_admitted(
                ctx,
                &mut losses,
                "Rhino object presentation loss copies",
            )?;
            let record = record_storage.with_storage(|| {
                let mut links = ctx.collection_vec(1, "Rhino object presentation links")?;
                links.push(ctx.format_retained(
                    format_args!("rhino:object:record#{source_order:06}"),
                    "Rhino object presentation link",
                )?);
                Ok::<_, CodecError>(ObjectPresentationRecord {
                    id: if identity.object_id.is_nil()
                        || ctx
                            .get_hash_map(
                                &object_id_counts,
                                &identity.object_id,
                                "Rhino object identity lookup",
                            )?
                            .copied()
                            != Some(1)
                    {
                        ctx.format_retained(
                            format_args!("rhino:presentation:object#record-{source_order:06}"),
                            "Rhino object presentation ID",
                        )?
                    } else {
                        ctx.format_retained(
                            format_args!("rhino:presentation:object#{}", identity.object_id),
                            "Rhino object presentation ID",
                        )?
                    },
                    source_offset: cadmpeg_core::decode::u64_from_index(object.range.start),
                    attributes: attributes_presentation,
                    links,
                })
            })?;
            staging.with_storage(|| {
                ctx.reserve_vec(
                    &mut object_presentation,
                    1,
                    "Rhino object presentation records",
                )
            })?;
            object_presentation.push(record);
            retain_presentation_record_storage(
                ctx,
                &mut record_guard_storage,
                &mut record_storages,
                record_storage,
            )?;
        }
    }
    let mut layer_count_workspace = ctx.reserve_scoped(0, "Rhino layer identity workspace")?;
    let mut layer_id_counts = HashMap::<Uuid, usize>::new();
    ctx.charge_work(0, "Rhino install traversal")?;
    let mut projection_source = scan.metadata.layers[..].iter();
    for _ in 0..projection_source.len() {
        let layer = ctx.next_charged(&mut projection_source, "Rhino install traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        if let Some(id) = layer.id {
            let count = layer_count_workspace
                .with_storage(|| {
                    ctx.entry_hash_map(&mut layer_id_counts, id, "Rhino layer identity counts")
                })?
                .or_default();
            *count += 1;
        }
    }
    ctx.charge_work(0, "Rhino install traversal")?;
    let mut projection_source = scan.metadata.layers[..].iter();
    for _ in 0..projection_source.len() {
        let layer = ctx.next_charged(&mut projection_source, "Rhino install traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        let (rendering_result, rendering_storage) = ctx.with_scoped_storage(
            "Rhino layer rendering staging",
            || {
                Ok::<_, CodecError>(rendering_attributes(
                    ctx,
                    scan.data,
                    layer.rendering_range.clone(),
                    scan.archive,
                    settings::RenderingAttributesKind::Layer,
                ))
            },
        )?;
        let mut rendering_storage = Some(rendering_storage);
        let rendering = match rendering_result {
            Ok(rendering) => rendering,
            Err(FramingError::Resource(limit)) => return Err(CodecError::ResourceLimit(limit)),
            Err(error) => {
                push_presentation_loss(
                    ctx,
                    &mut losses,
                    RhinoLossCode::PresentationRecordDropped,
                    format_args!(
                        "layer rendering attributes at offset {} could not be transferred: {error}",
                        layer.source.range.start
                    ),
                )?;
                drop(error);
                drop(rendering_storage.take());
                RenderingAttributesPresentation::default()
            }
        };
        let unique_id = match layer.id {
            Some(id)
                if ctx
                    .get_hash_map(&layer_id_counts, &id, "Rhino layer identity lookup")?
                    .copied()
                    == Some(1) =>
            {
                Some(id)
            }
            _ => None,
        };
        let mut layer_storage = ctx.reserve_scoped(0, "Rhino layer presentation staging")?;
        let record = layer_storage.with_storage(|| {
            let mut per_viewport_settings = Vec::new();
            ctx.extend_from_slice(
                &mut per_viewport_settings,
                &layer.per_viewport_settings,
                "Rhino layer presentation viewport settings",
            )?;
            Ok::<_, CodecError>(LayerPresentationRecord {
                id: if let Some(id) = unique_id {
                    ctx.format_retained(
                        format_args!("rhino:presentation:layer#{id}"),
                        "Rhino layer presentation ID",
                    )?
                } else {
                    ctx.format_retained(
                        format_args!(
                            "rhino:presentation:layer#index-{}-offset-{}",
                            layer.index, layer.source.range.start
                        ),
                        "Rhino layer presentation ID",
                    )?
                },
                source_offset: cadmpeg_core::decode::u64_from_index(layer.source.range.start),
                archive_index: layer.index,
                source_uuid: layer
                    .id
                    .map(|id| {
                        ctx.format_retained(
                            format_args!("{id}"),
                            "Rhino layer presentation source UUID",
                        )
                    })
                    .transpose()?,
                hierarchy: LayerHierarchySlot(layer.hierarchy),
                name: ctx.copy_retained_text(&layer.name, "Rhino layer presentation name")?,
                description: layer
                    .description
                    .as_deref()
                    .map(|description| {
                        ctx.copy_retained_text(
                            description,
                            "Rhino layer presentation description",
                        )
                    })
                    .transpose()?,
                iges_level: layer.iges_level,
                visible: layer.visible,
                locked: layer.locked,
                color: layer.color,
                material_index: layer.render_material_index,
                linetype_index: layer.linetype_index,
                plot: LayerPlotSlot(layer.plot),
                display_material_uuid: layer
                    .display_material_id
                    .filter(|id| !id.is_nil())
                    .map(|id| {
                        ctx.format_retained(
                            format_args!("{id}"),
                            "Rhino layer presentation display material UUID",
                        )
                    })
                    .transpose()?,
                clipping_planes_enabled: layer.no_clipping_planes.map(|value| !value),
                visible_in_new_details: layer.visible_in_new_details,
                rendering_materials: rendering.materials,
                per_viewport_settings,
            })
        })?;
        staging.with_storage(|| {
            ctx.reserve_vec(&mut layers, 1, "Rhino layer presentation records")
        })?;
        layers.push(record);
        retain_presentation_record_storage(
            ctx,
            &mut record_guard_storage,
            &mut record_storages,
            layer_storage,
        )?;
        if let Some(rendering_storage) = rendering_storage {
            retain_presentation_record_storage(
                ctx,
                &mut record_guard_storage,
                &mut record_storages,
                rendering_storage,
            )?;
        }
    }
    ctx.charge_work(0, "Rhino install traversal")?;
    let mut projection_source = group_index_counts[..].iter();
    for _ in 0..projection_source.len() {
        let (index, count) = ctx.next_charged(&mut projection_source, "Rhino install traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        if *count > 1 {
            push_presentation_loss(
                ctx,
                &mut losses,
                RhinoLossCode::PresentationRecordDropped,
                format_args!(
                    "group index {index} occurs {count} times; ambiguous member links were dropped"
                ),
            )?;
        }
    }
    drop(group_index_counts);
    drop(group_index_workspace);
    let disambiguated_group_count =
        disambiguate_group_ids(ctx, &mut groups, Some(&mut group_storages))?;
    if disambiguated_group_count != 0 {
        push_presentation_loss(ctx, &mut losses, RhinoLossCode::DuplicateRecordResolved, format_args!(
            "{disambiguated_group_count} group source identities were disambiguated by source offset"
        ))?;
    }
    ctx.charge_work(0, "Rhino group link traversal")?;
    let mut projection_source = groups[..].iter_mut();
    for _ in 0..projection_source.len() {
        let group = ctx.next_charged(&mut projection_source, "Rhino group link traversal")?
            .ok_or_else(|| CodecError::malformed("Rhino presentation traversal source ended early"))?;
        group.links = ctx
            .remove_hash_map(
                &mut group_members,
                &group.archive_index,
                "Rhino group member removal",
            )?
            .unwrap_or_default();
        ctx.stable_sort_by(
            &mut group.links,
            |value| value,
            Ord::cmp,
            "Rhino group link sort",
        )?;
    }
    let namespace = ir.native.namespace_mut("rhino");
    namespace.set_arena(ctx, "groups", &groups)?;
    namespace.set_arena(ctx, "materials", &materials)?;
    namespace.set_arena(ctx, "lights", &lights)?;
    namespace.set_arena(ctx, "linetypes", &linetypes)?;
    namespace.set_arena(ctx, "hatch_patterns", &hatch_patterns)?;
    namespace.set_arena(ctx, "dimension_styles", &dimension_styles)?;
    namespace.set_arena(ctx, "embedded_images", &images)?;
    namespace.set_arena(ctx, "windows_bitmaps", &windows_bitmaps)?;
    namespace.set_arena(ctx, "texture_mappings", &texture_mappings)?;
    namespace.set_arena(ctx, "text_styles", &text_styles)?;
    namespace.set_arena(ctx, "layers", &layers)?;
    namespace.set_arena(ctx, "object_presentation", &object_presentation)?;
    Ok(NativeInstall {
        losses,
        opaque_records,
    })
}

#[cfg(test)]
mod tests;
