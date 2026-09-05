// SPDX-License-Identifier: Apache-2.0
//! Bounded Rhino document properties, settings, units, and layer metadata.

use std::collections::BTreeSet;
use std::ops::Range;

use cadmpeg_core::decode::View;
use serde::Serialize;

use crate::chunks::{checked_count_bytes, chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::container::{OpaqueRecord, Record, Table};
use crate::objects::{parse_class_wrapper_with_userdata, read_uuid_list, UserdataDescriptor};
use crate::wire::Uuid;

const MAX_STRING_BYTES: usize = 1 << 20;
const MAX_ARRAY_ITEMS: usize = 1 << 16;
const PROPERTIES: u32 = 0x1000_0014;
const SETTINGS: u32 = 0x1000_0015;
const LAYER: u32 = 0x1000_0011;
const LAYER_RECORD: u32 = 0x2000_8050;
const REVISION_HISTORY: u32 = 0x2000_8021;
const NOTES: u32 = 0x2000_8022;
const PREVIEW: u32 = 0x2000_8023;
const COMPRESSED_PREVIEW: u32 = 0x2000_8025;
const APPLICATION: u32 = 0x2000_8024;
const WRITER_VERSION: u32 = 0xa000_0026;
const AS_FILE_NAME: u32 = 0x2000_8027;
const UNITS: u32 = 0x2000_8031;
const PLUGIN_LIST: u32 = 0x2000_8135;
const RENDER_MESH: u32 = 0x2000_8032;
const ANALYSIS_MESH: u32 = 0x2000_8033;
const CURRENT_LAYER: u32 = 0xa000_0038;
const CURRENT_MATERIAL: u32 = 0x2000_8039;
const CURRENT_COLOR: u32 = 0x2000_803a;
const CURRENT_WIRE_DENSITY: u32 = 0xa000_003c;
const MODEL_URL: u32 = 0x2000_8131;
const CURRENT_FONT: u32 = 0xa000_0132;
const CURRENT_DIMSTYLE: u32 = 0xa000_0133;
const ATTRIBUTES: u32 = 0x2000_8134;
const ON_LAYER_UUID: Uuid = Uuid::from_canonical([
    0x95, 0x80, 0x98, 0x13, 0xe9, 0x85, 0x11, 0xd3, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const ANONYMOUS: u32 = 0x4000_8000;
const MODEL_ATTRIBUTES: u32 = 0x4000_8002;
pub(crate) const LAYER_EXTENSIONS: Uuid = Uuid::from_canonical([
    0x3e, 0x49, 0x04, 0xe6, 0xe9, 0x30, 0x4f, 0xbc, 0xaa, 0x42, 0xeb, 0xd4, 0x07, 0xae, 0xfe, 0x3b,
]);
const LAYER_PER_VIEWPORT_ID: u32 = 1;
const LAYER_PER_VIEWPORT_COLOR: u32 = 2;
const LAYER_PER_VIEWPORT_PLOT_COLOR: u32 = 4;
const LAYER_PER_VIEWPORT_PLOT_WEIGHT: u32 = 8;
const LAYER_PER_VIEWPORT_VISIBLE: u32 = 16;
const LAYER_PER_VIEWPORT_PERSISTENT_VISIBILITY: u32 = 32;

/// A source range in the original archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceRange {
    /// Complete chunk range.
    pub(crate) range: Range<usize>,
}

/// A finite three-dimensional point.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub(crate) struct Point3(pub(crate) [f64; 3]);

/// A finite three-dimensional vector.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub(crate) struct Vector3(pub(crate) [f64; 3]);

/// A serialized parameter interval.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub(crate) struct Interval(pub(crate) [f64; 2]);

/// A serialized plane, including its wire equation.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub(crate) struct Plane {
    /// Origin.
    pub(crate) origin: Point3,
    /// X axis.
    pub(crate) xaxis: Vector3,
    /// Y axis.
    pub(crate) yaxis: Vector3,
    /// Z axis.
    pub(crate) zaxis: Vector3,
    /// Serialized plane equation.
    pub(crate) equation: [f64; 4],
}

/// A serialized axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub(crate) struct BoundingBox {
    /// Minimum point.
    pub(crate) minimum: Point3,
    /// Maximum point.
    pub(crate) maximum: Point3,
}

/// A serialized row-major 4×4 transform.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub(crate) struct Xform(pub(crate) [f64; 16]);

/// A UTF-16 UTC time tuple as written by Rhino.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UtcTime {
    /// The eight serialized fields in seconds, minutes, hours, and calendar order.
    pub(crate) fields: [i32; 8],
}

/// Decoded document properties.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub(crate) struct Properties {
    /// Writer version short value.
    pub(crate) writer_version: Option<i64>,
    /// Revision-history payload.
    pub(crate) revision_history: Option<RevisionHistory>,
    /// Notes payload.
    pub(crate) notes: Option<Notes>,
    /// Application payload.
    pub(crate) application: Option<Application>,
    /// As-file-name value.
    pub(crate) as_file_name: Option<String>,
    /// Bounded preview descriptors.
    pub(crate) previews: Vec<PreviewDescriptor>,
}

/// Revision-history property.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct RevisionHistory {
    /// Source range.
    pub(crate) source: SourceRange,
    /// Creator.
    pub(crate) created_by: String,
    /// Creation time.
    pub(crate) created: UtcTime,
    /// Last editor.
    pub(crate) last_edited_by: String,
    /// Last edit time.
    pub(crate) last_edited: UtcTime,
    /// Revision count.
    pub(crate) revision_count: i32,
}

/// Notes property.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct Notes {
    /// Source range.
    pub(crate) source: SourceRange,
    /// HTML flag.
    pub(crate) html: i32,
    /// Text.
    pub(crate) text: String,
    /// Visibility flag.
    pub(crate) visible: i32,
    /// Window rectangle.
    pub(crate) rectangle: [i32; 4],
    /// Lock flag introduced by version 1.1.
    pub(crate) locked: bool,
}

/// Application property.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct Application {
    /// Source range.
    pub(crate) source: SourceRange,
    /// Application name.
    pub(crate) name: String,
    /// Application URL.
    pub(crate) url: String,
    /// Application details.
    pub(crate) details: String,
}

/// Bounded preview metadata without retaining image bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreviewDescriptor {
    /// Complete source range.
    pub(crate) source: SourceRange,
    /// Whether the preview is compressed.
    pub(crate) compressed: bool,
    /// Payload byte length.
    pub(crate) payload_bytes: usize,
}

/// The standard and custom Rhino unit systems.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum UnitSystem {
    /// No unit system.
    None,
    /// A standard unit identified by its archive value.
    Standard(u8),
    /// A custom unit system.
    Custom {
        /// Meters per archive unit.
        meters_per_unit: f64,
        /// Custom display name.
        name: String,
    },
    /// An explicitly unset unit system.
    Unset,
}

/// Units and tolerances.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub(crate) struct UnitsAndTolerances {
    /// Structure version.
    pub(crate) version: i32,
    /// Raw unit enum.
    pub(crate) unit_value: i32,
    /// Unit system.
    pub(crate) unit: UnitSystem,
    /// Millimeters per archive unit.
    pub(crate) millimeters_per_unit: Option<f64>,
    /// Absolute tolerance in native archive units.
    pub(crate) absolute_tolerance: f64,
    /// Absolute tolerance resolved to millimeters for a later IR transfer.
    pub(crate) absolute_tolerance_millimeters: Option<f64>,
    /// Angular tolerance, never scaled.
    pub(crate) angular_tolerance: f64,
    /// Relative tolerance, never scaled.
    pub(crate) relative_tolerance: f64,
    /// Distance display mode.
    pub(crate) distance_display_mode: Option<i32>,
    /// Distance display precision.
    pub(crate) distance_display_precision: Option<i32>,
    /// Source range.
    pub(crate) source: SourceRange,
}

/// One plugin reference stored in the settings plugin list.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub(crate) struct PluginReference {
    /// Complete anonymous-chunk source range.
    pub(crate) source: SourceRange,
    /// Anonymous chunk version.
    pub(crate) version: (i32, i32),
    /// Plugin identity.
    pub(crate) plugin_id: Uuid,
    /// Rhino plugin-type enum ordinal.
    pub(crate) plugin_type: i32,
    /// Plugin display name.
    pub(crate) name: String,
    /// Plugin version string.
    pub(crate) version_string: String,
    /// Plugin executable filename.
    pub(crate) filename: String,
    /// Developer organization.
    pub(crate) developer_organization: Option<String>,
    /// Developer address.
    pub(crate) developer_address: Option<String>,
    /// Developer country.
    pub(crate) developer_country: Option<String>,
    /// Developer phone.
    pub(crate) developer_phone: Option<String>,
    /// Developer email.
    pub(crate) developer_email: Option<String>,
    /// Developer website.
    pub(crate) developer_website: Option<String>,
    /// Developer update URL.
    pub(crate) developer_update_url: Option<String>,
    /// Developer fax.
    pub(crate) developer_fax: Option<String>,
    /// Plugin platform: 0 unknown, 1 C++, 2 .NET.
    pub(crate) platform: Option<i32>,
    /// Plugin SDK version component.
    pub(crate) sdk_version: Option<i32>,
    /// Plugin SDK service-release component.
    pub(crate) sdk_service_release: Option<i32>,
}

/// The settings plugin list and its bounded entries.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub(crate) struct PluginList {
    /// Complete source range.
    pub(crate) source: SourceRange,
    /// Packed list version.
    pub(crate) version: (u8, u8),
    /// Plugin references.
    pub(crate) plugins: Vec<PluginReference>,
}

/// Earth-location anchor nested in the settings-attributes record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EarthAnchorPoint {
    /// Anonymous chunk version.
    pub(crate) version: (i32, i32),
    /// Earth latitude in degrees.
    pub(crate) earth_latitude: f64,
    /// Earth longitude in degrees.
    pub(crate) earth_longitude: f64,
    /// Earth elevation in meters.
    pub(crate) earth_elevation_meters: f64,
    /// Model point corresponding to the earth location.
    pub(crate) model_point: Point3,
    /// Model north vector.
    pub(crate) model_north: Vector3,
    /// Model east vector.
    pub(crate) model_east: Vector3,
    /// Legacy elevation-reference enum stored by versions 1.1 and later.
    pub(crate) legacy_coordinate_system: Option<i32>,
    /// Earth-anchor UUID.
    pub(crate) id: Option<Uuid>,
    /// Earth-anchor name.
    pub(crate) name: Option<String>,
    /// Earth-anchor description.
    pub(crate) description: Option<String>,
    /// Earth-anchor URL.
    pub(crate) url: Option<String>,
    /// Earth-anchor URL tag.
    pub(crate) url_tag: Option<String>,
    /// Current earth-coordinate-system enum stored by version 1.2 and later.
    pub(crate) coordinate_system: Option<u32>,
}

/// The nested settings record controlling linked definitions and textures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IoSettings {
    /// Anonymous chunk version.
    pub(crate) version: (i32, i32),
    /// Whether texture bitmaps are saved in the file.
    pub(crate) save_texture_bitmaps_in_file: bool,
    /// Linked-instance-definition update policy.
    pub(crate) idef_link_update: i32,
}

/// `SubD` display fields nested in mesh parameters version 1.5.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SubDDisplayParameters {
    /// Anonymous chunk minor version.
    pub(crate) version: i32,
    /// Adaptive display density.
    pub(crate) display_density: u32,
    /// Component location enum.
    pub(crate) mesh_location: u32,
    /// Whether the display density is absolute, introduced at version 2.
    pub(crate) display_density_is_absolute: Option<bool>,
    /// Whether curvature is computed, introduced at version 3.
    pub(crate) compute_curvature: Option<bool>,
}

/// Serialized mesh parameters used by settings records.
#[derive(Debug, Clone, PartialEq, Serialize)]
// These independent flags are separate fields in the source wire grammar.
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct MeshParameters {
    /// Packed version.
    pub(crate) version: (u8, u8),
    /// Legacy boolean fields, decoded from nonzero integers.
    pub(crate) compute_curvature: bool,
    /// Whether simple planes are used.
    pub(crate) simple_planes: bool,
    /// Whether refinement is enabled.
    pub(crate) refine: bool,
    /// Whether jagged seams are allowed.
    pub(crate) jagged_seams: bool,
    /// Obsolete weld field retained in the wire layout.
    pub(crate) obsolete_weld: i32,
    /// Meshing tolerance.
    pub(crate) tolerance: f64,
    /// Minimum edge length.
    pub(crate) min_edge_length: f64,
    /// Maximum edge length.
    pub(crate) max_edge_length: f64,
    /// Grid aspect ratio.
    pub(crate) grid_aspect_ratio: f64,
    /// Minimum grid count.
    pub(crate) grid_min_count: i32,
    /// Maximum grid count.
    pub(crate) grid_max_count: i32,
    /// Grid angle in radians.
    pub(crate) grid_angle_radians: f64,
    /// Grid amplification factor.
    pub(crate) grid_amplification: f64,
    /// Refinement angle in radians.
    pub(crate) refine_angle_radians: f64,
    /// Obsolete combine angle retained in the wire layout.
    pub(crate) obsolete_combine_angle: f64,
    /// Face-type enum: 0 mixed, 1 triangles, 2 quads.
    pub(crate) face_type: i32,
    /// Texture-range mode, introduced at minor 1.
    pub(crate) texture_range: Option<u32>,
    /// Custom-settings flag, introduced at minor 2.
    pub(crate) custom_settings: Option<bool>,
    /// Relative tolerance, introduced at minor 2.
    pub(crate) relative_tolerance: Option<f64>,
    /// Mesher selector, introduced at minor 3.
    pub(crate) mesher: Option<u8>,
    /// Custom-settings-enabled flag, introduced at minor 4.
    pub(crate) custom_settings_enabled: Option<bool>,
    /// `SubD` display parameters, introduced at minor 5.
    pub(crate) subd: Option<SubDDisplayParameters>,
}

/// Typed settings-attributes record.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SettingsAttributes {
    /// Complete source range.
    pub(crate) source: SourceRange,
    /// Packed record version.
    pub(crate) version: (u8, u8),
    /// World scale applied to non-solid linetypes for model display.
    pub(crate) linetype_display_scale: f64,
    /// Current plot color bytes.
    pub(crate) current_plot_color: [u8; 4],
    /// Current plot-color source enum.
    pub(crate) current_plot_color_source: i32,
    /// V5 current line-pattern index, or -1 when unset.
    pub(crate) current_line_pattern_index: i32,
    /// Current linetype source enum.
    pub(crate) current_linetype_source: i32,
    /// Page-space units and tolerances, introduced at minor 1.
    pub(crate) page_units: Option<UnitsAndTolerances>,
    /// Active view UUID, introduced at minor 2.
    pub(crate) active_view_id: Option<Uuid>,
    /// Model basepoint, introduced at minor 3.
    pub(crate) model_basepoint: Option<Point3>,
    /// Earth anchor, introduced at minor 3.
    pub(crate) earth_anchor: Option<EarthAnchorPoint>,
    /// Texture-save flag, introduced at minor 4.
    pub(crate) save_texture_bitmaps_in_file: Option<bool>,
    /// IO settings, introduced at minor 5.
    pub(crate) io_settings: Option<IoSettings>,
    /// Custom render mesh settings, introduced at minor 6.
    pub(crate) custom_render_mesh: Option<MeshParameters>,
    /// Current layer UUID, introduced at minor 7.
    pub(crate) current_layer_id: Option<Uuid>,
    /// Current render-material UUID, introduced at minor 7.
    pub(crate) current_render_material_id: Option<Uuid>,
    /// Current line-pattern UUID, introduced at minor 7.
    pub(crate) current_line_pattern_id: Option<Uuid>,
    /// Current text-style UUID, introduced at minor 7.
    pub(crate) current_text_style_id: Option<Uuid>,
    /// Current dimension-style UUID, introduced at minor 7.
    pub(crate) current_dimension_style_id: Option<Uuid>,
    /// Current hatch-pattern UUID, introduced at minor 7.
    pub(crate) current_hatch_pattern_id: Option<Uuid>,
}

/// A bounded unsupported setting payload.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct SettingDescriptor {
    /// Record typecode.
    pub(crate) typecode: u32,
    /// Complete source range.
    pub(crate) source: SourceRange,
    /// Payload byte length.
    pub(crate) payload_bytes: usize,
}

/// Current document selectors, typed settings, and bounded unsupported settings.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub(crate) struct DocumentSettings {
    /// Current layer archive index.
    pub(crate) current_layer: Option<i64>,
    /// Current material archive index.
    pub(crate) current_material: Option<i32>,
    /// Current material source selector.
    pub(crate) current_material_source: Option<i32>,
    /// Current color bytes.
    pub(crate) current_color: Option<[u8; 4]>,
    /// Current color source selector.
    pub(crate) current_color_source: Option<i32>,
    /// Current wire density.
    pub(crate) current_wire_density: Option<i64>,
    /// Current font archive index.
    pub(crate) current_font: Option<i64>,
    /// Current dimstyle archive index.
    pub(crate) current_dimstyle: Option<i64>,
    /// Model URL.
    pub(crate) model_url: Option<String>,
    /// Units and tolerances.
    pub(crate) units: Option<UnitsAndTolerances>,
    /// Plugins that may have saved userdata in the file.
    pub(crate) plugin_list: Option<PluginList>,
    /// Settings attributes.
    pub(crate) attributes: Option<SettingsAttributes>,
    /// Render-mesh settings.
    pub(crate) render_mesh_settings: Option<MeshParameters>,
    /// Analysis-mesh settings.
    pub(crate) analysis_mesh_settings: Option<MeshParameters>,
    /// Unsupported known settings.
    pub(crate) unsupported: Vec<SettingDescriptor>,
}

/// Layer metadata decoded without attributes or geometry.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct LayerRecord {
    /// Complete source range.
    pub(crate) source: SourceRange,
    /// Packed layer version.
    pub(crate) version: (u8, u8),
    /// Obsolete mode.
    pub(crate) obsolete_mode: i32,
    /// Archive layer index.
    pub(crate) index: i32,
    /// IGES level.
    pub(crate) iges_level: i32,
    /// Render material index.
    pub(crate) render_material_index: i32,
    /// Layer color.
    pub(crate) color: [u8; 4],
    /// Layer name.
    pub(crate) name: String,
    /// Source-normalized layer description, when item 37 is nonempty.
    pub(crate) description: Option<String>,
    /// Visibility.
    pub(crate) visible: bool,
    /// Lock state.
    pub(crate) locked: bool,
    /// Layer UUID.
    pub(crate) id: Option<Uuid>,
    /// Parent UUID.
    pub(crate) parent_id: Option<Uuid>,
    /// Expanded state.
    pub(crate) expanded: Option<bool>,
    /// Referenced linetype index.
    pub(crate) linetype_index: Option<i32>,
    /// Plot color.
    pub(crate) plot_color: Option<[u8; 4]>,
    /// Plot weight in millimeters.
    pub(crate) plot_weight: Option<f64>,
    /// Display material UUID.
    pub(crate) display_material_id: Option<Uuid>,
    /// Whether clipping planes are disabled.
    pub(crate) no_clipping_planes: Option<bool>,
    /// Whether per-viewport visibility starts enabled in new detail views.
    pub(crate) visible_in_new_details: Option<bool>,
    /// Bounded rendering payload range.
    pub(crate) rendering_range: Option<Range<usize>>,
    /// Raw extension item IDs successfully consumed.
    pub(crate) extension_items: Vec<u8>,
    /// Direct embedded linetype descriptor.
    pub(crate) embedded_linetype: Option<EmbeddedDescriptor>,
    /// Direct embedded section-style descriptor.
    pub(crate) embedded_section_style: Option<EmbeddedDescriptor>,
    /// Per-viewport layer overrides from class-owned userdata.
    pub(crate) per_viewport_settings: Vec<LayerPerViewportSettings>,
}

/// A source-normalized per-viewport layer override.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LayerPerViewportSettings {
    /// Viewport identity selected by the source entry.
    pub(crate) viewport_id: Uuid,
    /// Effective source settings mask after source defaults and validation.
    pub(crate) settings_mask: u32,
    /// Per-viewport layer color, if effective.
    pub(crate) color: Option<[u8; 4]>,
    /// Per-viewport plot color, if effective.
    pub(crate) plot_color: Option<[u8; 4]>,
    /// Per-viewport plot weight in millimeters, if effective.
    pub(crate) plot_weight_mm: Option<f64>,
    /// Raw source visibility value, 1 for visible and 2 for off.
    pub(crate) visible: Option<u8>,
    /// Raw source persistent-visibility value, 1 or 2, for child layers.
    pub(crate) persistent_visibility: Option<u8>,
}

/// A bounded direct object payload embedded in a layer extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EmbeddedDescriptor {
    /// Complete object chunk range.
    pub(crate) source: SourceRange,
    /// Direct object payload version.
    pub(crate) version: (i32, i32),
}

/// All typed metadata produced by a scan.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub(crate) struct DocumentMetadata {
    /// Typed losses raised while decoding metadata.
    pub(crate) losses: Vec<cadmpeg_ir::report::LossNote>,
    /// Document properties.
    pub(crate) properties: Properties,
    /// Document settings.
    pub(crate) settings: DocumentSettings,
    /// Layer records.
    pub(crate) layers: Vec<LayerRecord>,
    /// Complete metadata records whose tagged payload could not be decoded.
    pub(crate) opaque_records: Vec<OpaqueRecord>,
}

fn finite(reader: &BoundedReader<'_>, value: f64, label: &str) -> Result<f64, FramingError> {
    value.is_finite().then_some(value).ok_or_else(|| {
        FramingError::structural(reader.position(), format!("{label} is not finite"))
    })
}

fn finite_f64(reader: &mut BoundedReader<'_>, label: &str) -> Result<f64, FramingError> {
    let value = reader.f64()?;
    finite(reader, value, label)
}

fn finite_array<const N: usize>(
    reader: &BoundedReader<'_>,
    values: [f64; N],
    label: &str,
) -> Result<[f64; N], FramingError> {
    values
        .iter()
        .all(|value| value.is_finite())
        .then_some(values)
        .ok_or_else(|| {
            FramingError::structural(
                reader.position(),
                format!("{label} contains a nonfinite value"),
            )
        })
}

/// Reads a finite point.
#[allow(dead_code)]
pub(crate) fn point(reader: &mut BoundedReader<'_>) -> Result<Point3, FramingError> {
    let values = [reader.f64()?, reader.f64()?, reader.f64()?];
    Ok(Point3(finite_array(reader, values, "point")?))
}

/// Reads a finite vector.
#[allow(dead_code)]
pub(crate) fn vector(reader: &mut BoundedReader<'_>) -> Result<Vector3, FramingError> {
    let values = [reader.f64()?, reader.f64()?, reader.f64()?];
    Ok(Vector3(finite_array(reader, values, "vector")?))
}

/// Reads a finite interval.
#[allow(dead_code)]
pub(crate) fn interval(reader: &mut BoundedReader<'_>) -> Result<Interval, FramingError> {
    let values = [reader.f64()?, reader.f64()?];
    Ok(Interval(finite_array(reader, values, "interval")?))
}

/// Reads a finite plane without reconstructing its serialized equation.
#[allow(dead_code)]
pub(crate) fn plane(reader: &mut BoundedReader<'_>) -> Result<Plane, FramingError> {
    let origin = point(reader)?;
    let xaxis = vector(reader)?;
    let yaxis = vector(reader)?;
    let zaxis = vector(reader)?;
    let equation = [reader.f64()?, reader.f64()?, reader.f64()?, reader.f64()?];
    Ok(Plane {
        origin,
        xaxis,
        yaxis,
        zaxis,
        equation: finite_array(reader, equation, "plane equation")?,
    })
}

/// Reads a finite bounding box.
#[allow(dead_code)]
pub(crate) fn bbox(reader: &mut BoundedReader<'_>) -> Result<BoundingBox, FramingError> {
    Ok(BoundingBox {
        minimum: point(reader)?,
        maximum: point(reader)?,
    })
}

/// Reads a finite row-major transform.
#[allow(dead_code)]
pub(crate) fn xform(reader: &mut BoundedReader<'_>) -> Result<Xform, FramingError> {
    let mut values = [0.0; 16];
    for value in &mut values {
        *value = reader.f64()?;
    }
    Ok(Xform(finite_array(reader, values, "transform")?))
}

/// Decodes an archive UTF-8 string for later plugin/settings records.
#[allow(dead_code)]
pub(crate) fn utf8(reader: &mut BoundedReader<'_>) -> Result<String, FramingError> {
    let count_offset = reader.position();
    let count = usize::try_from(reader.u32()?)
        .map_err(|_| FramingError::structural(reader.position(), "UTF-8 count overflow"))?;
    if count > MAX_STRING_BYTES || count > reader.remaining() {
        return Err(FramingError::structural(
            reader.position(),
            format!("UTF-8 count {count} exceeds bounded string limit"),
        ));
    }
    let bytes = reader.take(count)?;
    if count == 0 {
        return Ok(String::new());
    }
    if bytes.last() != Some(&0) {
        return Err(FramingError::Structural {
            offset: count_offset,
            message: "UTF-8 string is missing NUL terminator".to_string(),
        });
    }
    std::str::from_utf8(&bytes[..count - 1])
        .map(str::to_owned)
        .map_err(|_| FramingError::structural(reader.position(), "invalid UTF-8 string"))
}

pub(crate) fn utf16(reader: &mut BoundedReader<'_>) -> Result<String, FramingError> {
    let count_offset = reader.position();
    let count = usize::try_from(reader.u32()?)
        .map_err(|_| FramingError::structural(reader.position(), "UTF-16 count overflow"))?;
    if count > MAX_STRING_BYTES / 2 || count.checked_mul(2).is_none_or(|n| n > reader.remaining()) {
        return Err(FramingError::structural(
            reader.position(),
            format!("UTF-16 count {count} exceeds bounded string limit"),
        ));
    }
    if count == 0 {
        return Ok(String::new());
    }
    let bytes = reader.take(count.saturating_mul(2))?;
    if View::u16_le_at(bytes, count.saturating_sub(1).saturating_mul(2)) != Some(0) {
        return Err(FramingError::structural(
            count_offset,
            "UTF-16 string is missing NUL terminator",
        ));
    }
    View::utf16le_at(bytes, 0, count.saturating_sub(1))
        .map(|(value, _)| value)
        .ok_or_else(|| {
            FramingError::structural(reader.position(), "invalid UTF-16 surrogate sequence")
        })
}

fn uuid(reader: &mut BoundedReader<'_>) -> Result<Uuid, FramingError> {
    Ok(Uuid::from_wire(
        reader.take(16)?.try_into().expect("length checked"),
    ))
}

fn color(reader: &mut BoundedReader<'_>) -> Result<[u8; 4], FramingError> {
    Ok(reader.take(4)?.try_into().expect("length checked"))
}

fn parse_layer_extensions(
    data: &[u8],
    descriptor: &UserdataDescriptor,
    archive: ArchiveVersion,
    parent_id: Option<Uuid>,
) -> Result<Vec<LayerPerViewportSettings>, FramingError> {
    let outer = chunk_at(
        data,
        descriptor.payload_range().start,
        descriptor.payload_range().end,
        archive,
        false,
    )?;
    if outer.short || outer.typecode != ANONYMOUS {
        return Err(FramingError::structural(
            outer.header_start,
            "layer extensions payload is not a long anonymous chunk",
        ));
    }
    let mut outer_reader = BoundedReader::new(data, outer.body.start, outer.body.end)?;
    let major = outer_reader.i32()?;
    let minor = outer_reader.i32()?;
    if major != 1 || minor < 0 {
        return Err(FramingError::structural(
            outer.body.start,
            "layer extensions version is unsupported",
        ));
    }
    let count = outer_reader.i32()?;
    let count = checked_count_bytes(
        count,
        1,
        outer_reader.remaining(),
        MAX_ARRAY_ITEMS,
        outer_reader.position(),
    )?;
    let parent_is_nil = parent_id.is_none_or(Uuid::is_nil);
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let entry = chunk_at(
            data,
            outer_reader.position(),
            outer.body.end,
            archive,
            false,
        )?;
        if entry.short || entry.typecode != ANONYMOUS {
            return Err(FramingError::structural(
                entry.header_start,
                "layer extensions entry is not a long anonymous chunk",
            ));
        }
        let mut entry_reader = BoundedReader::new(data, entry.body.start, entry.body.end)?;
        let entry_major = entry_reader.i32()?;
        let entry_minor = entry_reader.i32()?;
        if entry_major != 1 || entry_minor < 0 {
            return Err(FramingError::structural(
                entry.body.start,
                "layer extensions entry version is unsupported",
            ));
        }
        let bits = entry_reader.u32()?;
        let viewport_id = if bits & LAYER_PER_VIEWPORT_ID != 0 {
            uuid(&mut entry_reader)?
        } else {
            Uuid::nil()
        };
        let color_value = if bits & LAYER_PER_VIEWPORT_COLOR != 0 {
            Some(color(&mut entry_reader)?)
        } else {
            None
        };
        let plot_color_value = if bits & LAYER_PER_VIEWPORT_PLOT_COLOR != 0 {
            Some(color(&mut entry_reader)?)
        } else {
            None
        };
        let plot_weight_value = if bits & LAYER_PER_VIEWPORT_PLOT_WEIGHT != 0 {
            Some(entry_reader.f64()?)
        } else {
            None
        };
        let (visible_value, compatibility_visible) = if bits & LAYER_PER_VIEWPORT_VISIBLE != 0 {
            let value = entry_reader.u8()?;
            let compatibility_value = (entry_minor >= 1).then(|| entry_reader.u8()).transpose()?;
            (Some(value), compatibility_value)
        } else {
            (None, None)
        };
        let persistent_value =
            if entry_minor >= 2 && bits & LAYER_PER_VIEWPORT_PERSISTENT_VISIBILITY != 0 {
                Some(entry_reader.u8()?)
            } else {
                compatibility_visible
            };
        entry_reader.skip_remaining()?;

        let color = color_value.filter(|value| *value != [u8::MAX; 4]);
        let plot_color = plot_color_value.filter(|value| *value != [u8::MAX; 4]);
        let plot_weight_mm = plot_weight_value
            .filter(|value| value.is_finite() && (*value >= 0.0 || *value == -1.0));
        let visible = visible_value.filter(|value| matches!(value, 1 | 2));
        let persistent_visibility = if parent_is_nil {
            None
        } else {
            persistent_value.filter(|value| matches!(value, 1 | 2))
        };
        let mut settings_mask = 0;
        if !viewport_id.is_nil() {
            if color.is_some() {
                settings_mask |= LAYER_PER_VIEWPORT_COLOR;
            }
            if plot_color.is_some() {
                settings_mask |= LAYER_PER_VIEWPORT_PLOT_COLOR;
            }
            if plot_weight_mm.is_some() {
                settings_mask |= LAYER_PER_VIEWPORT_PLOT_WEIGHT;
            }
            if visible.is_some() {
                settings_mask |= LAYER_PER_VIEWPORT_VISIBLE;
            }
            if persistent_visibility.is_some() {
                settings_mask |= LAYER_PER_VIEWPORT_PERSISTENT_VISIBILITY;
            }
        }
        if settings_mask != 0 {
            values.push(LayerPerViewportSettings {
                viewport_id,
                settings_mask: settings_mask | LAYER_PER_VIEWPORT_ID,
                color,
                plot_color,
                plot_weight_mm,
                visible,
                persistent_visibility,
            });
        }
        outer_reader.skip(entry.next_offset - outer_reader.position())?;
    }
    outer_reader.skip_remaining()?;
    values.sort_by(|a, b| {
        let mut ordering = a.viewport_id.cmp(&b.viewport_id);
        if ordering == std::cmp::Ordering::Equal {
            ordering = a.settings_mask.cmp(&b.settings_mask);
        }
        if ordering == std::cmp::Ordering::Equal
            && a.settings_mask & LAYER_PER_VIEWPORT_VISIBLE != 0
        {
            ordering = a.visible.cmp(&b.visible);
        }
        if ordering == std::cmp::Ordering::Equal
            && a.settings_mask & LAYER_PER_VIEWPORT_PERSISTENT_VISIBILITY != 0
        {
            ordering = a.persistent_visibility.cmp(&b.persistent_visibility);
        }
        if ordering == std::cmp::Ordering::Equal && a.settings_mask & LAYER_PER_VIEWPORT_COLOR != 0
        {
            ordering = a
                .color
                .map(u32::from_le_bytes)
                .cmp(&b.color.map(u32::from_le_bytes));
        }
        if ordering == std::cmp::Ordering::Equal
            && a.settings_mask & LAYER_PER_VIEWPORT_PLOT_COLOR != 0
        {
            ordering = a
                .plot_color
                .map(u32::from_le_bytes)
                .cmp(&b.plot_color.map(u32::from_le_bytes));
        }
        if ordering == std::cmp::Ordering::Equal
            && a.settings_mask & LAYER_PER_VIEWPORT_PLOT_WEIGHT != 0
        {
            ordering = a
                .plot_weight_mm
                .expect("plot weight mask has a value")
                .total_cmp(&b.plot_weight_mm.expect("plot weight mask has a value"));
        }
        ordering
    });
    Ok(values)
}

fn packed(reader: &mut BoundedReader<'_>) -> Result<(u8, u8), FramingError> {
    let value = reader.u8()?;
    Ok((value >> 4, value & 0x0f))
}

fn times(reader: &mut BoundedReader<'_>) -> Result<UtcTime, FramingError> {
    let mut fields = [0; 8];
    for field in &mut fields {
        *field = reader.i32()?;
    }
    Ok(UtcTime { fields })
}

fn finish(reader: &mut BoundedReader<'_>, _label: &str) -> Result<(), FramingError> {
    reader.skip_remaining()?;
    Ok(())
}

fn short_index(record: &Record, label: &str) -> Result<i64, FramingError> {
    if !record.short || record.value < -1 || record.value > i64::from(i32::MAX) {
        return Err(FramingError::Structural {
            offset: record.range.start,
            message: format!("{label} is not a valid short index"),
        });
    }
    Ok(record.value)
}

fn parse_revision(data: &[u8], record: &Record) -> Result<RevisionHistory, FramingError> {
    let mut reader = BoundedReader::new(data, record.body.start, record.body.end)?;
    let version = packed(&mut reader)?;
    if version.0 != 1 {
        return Err(FramingError::structural(
            reader.position(),
            "unsupported revision-history version",
        ));
    }
    let value = RevisionHistory {
        source: SourceRange {
            range: record.range.clone(),
        },
        created_by: utf16(&mut reader)?,
        created: times(&mut reader)?,
        last_edited_by: utf16(&mut reader)?,
        last_edited: times(&mut reader)?,
        revision_count: reader.i32()?,
    };
    reader.skip_remaining()?;
    Ok(value)
}

fn parse_notes(data: &[u8], record: &Record) -> Result<Notes, FramingError> {
    let mut reader = BoundedReader::new(data, record.body.start, record.body.end)?;
    let version = packed(&mut reader)?;
    if version.0 != 1 {
        return Err(FramingError::structural(
            reader.position(),
            "unsupported notes version",
        ));
    }
    let html = reader.i32()?;
    let text = utf16(&mut reader)?;
    let visible = reader.i32()?;
    let rectangle = [reader.i32()?, reader.i32()?, reader.i32()?, reader.i32()?];
    let locked = version.1 >= 1 && reader.bool()?;
    let value = Notes {
        source: SourceRange {
            range: record.range.clone(),
        },
        html,
        text,
        visible,
        rectangle,
        locked,
    };
    reader.skip_remaining()?;
    Ok(value)
}

fn parse_application(data: &[u8], record: &Record) -> Result<Application, FramingError> {
    let mut reader = BoundedReader::new(data, record.body.start, record.body.end)?;
    packed(&mut reader)?;
    let value = Application {
        source: SourceRange {
            range: record.range.clone(),
        },
        name: utf16(&mut reader)?,
        url: utf16(&mut reader)?,
        details: utf16(&mut reader)?,
    };
    reader.skip_remaining()?;
    Ok(value)
}

pub(crate) fn standard_scale(value: i32) -> Option<f64> {
    Some(match value {
        1 => 0.001,
        2 => 1.0,
        3 => 10.0,
        4 => 1000.0,
        5 => 1_000_000.0,
        6 => 0.000_025_4,
        7 => 0.0254,
        8 => 25.4,
        9 => 304.8,
        10 => 1_609_344.0,
        12 => 0.000_000_1,
        13 => 0.000_001,
        14 => 100.0,
        15 => 10_000.0,
        16 => 100_000.0,
        17 => 1_000_000_000.0,
        18 => 1_000_000_000_000.0,
        19 => 914.4,
        20 => 0.352_777_777_777_777_8,
        21 => 4.233_333_333_333_333,
        22 => 1_852_000.0,
        23 => 149_597_870_000_000.0,
        24 => 9.460_730_472_580_8e18,
        25 => 3.085_677_58e19,
        _ => return None,
    })
}

pub(crate) fn parse_units(
    data: &[u8],
    record: &Record,
) -> Result<UnitsAndTolerances, FramingError> {
    let mut reader = BoundedReader::new(data, record.body.start, record.body.end)?;
    parse_units_reader(
        &mut reader,
        SourceRange {
            range: record.range.clone(),
        },
    )
}

fn parse_units_reader(
    reader: &mut BoundedReader<'_>,
    source: SourceRange,
) -> Result<UnitsAndTolerances, FramingError> {
    let version = reader.i32()?;
    let legacy = version == 1;
    if !legacy && !(100..200).contains(&version) {
        return Err(FramingError::structural(
            reader.position(),
            "unsupported units structure version",
        ));
    }
    let unit_value = reader.i32()?;
    let absolute_raw = reader.f64()?;
    let absolute = finite(reader, absolute_raw, "absolute tolerance")?;
    let (relative, angular) = if legacy {
        let relative = reader.f64()?;
        let angular = reader.f64()?;
        (relative, angular)
    } else {
        let angular = reader.f64()?;
        let relative = reader.f64()?;
        (relative, angular)
    };
    let angular = finite(reader, angular, "angular tolerance")?;
    let relative = finite(reader, relative, "relative tolerance")?;
    if absolute <= 0.0 {
        return Err(FramingError::structural(
            reader.position(),
            "absolute tolerance must be positive",
        ));
    }
    if angular <= 0.0 || angular > std::f64::consts::PI {
        return Err(FramingError::structural(
            reader.position(),
            "angular tolerance must be in (0, pi]",
        ));
    }
    if relative <= 0.0 || relative >= 1.0 {
        return Err(FramingError::structural(
            reader.position(),
            "relative tolerance must be in (0, 1)",
        ));
    }
    let mode = (!legacy && version >= 101)
        .then(|| reader.i32())
        .transpose()?;
    let precision = (!legacy && version >= 101)
        .then(|| reader.i32())
        .transpose()?;
    let custom_scale = (!legacy && version >= 102)
        .then(|| reader.f64())
        .transpose()?;
    let custom_name = if !legacy && version >= 102 {
        Some(utf16(reader)?)
    } else {
        None
    };
    let unit = match unit_value {
        0 => UnitSystem::None,
        11 => UnitSystem::Custom {
            meters_per_unit: custom_scale.ok_or_else(|| {
                FramingError::structural(reader.position(), "custom unit has no scale")
            })?,
            name: custom_name.unwrap_or_default(),
        },
        255 => UnitSystem::Unset,
        value if standard_scale(value).is_some() => UnitSystem::Standard(
            u8::try_from(value)
                .map_err(|_| FramingError::structural(reader.position(), "unit value overflow"))?,
        ),
        _ => {
            return Err(FramingError::structural(
                reader.position(),
                "unknown unit enum value",
            ))
        }
    };
    let scale = match &unit {
        UnitSystem::Standard(value) => standard_scale(i32::from(*value)),
        UnitSystem::Custom {
            meters_per_unit, ..
        } if meters_per_unit.is_finite()
            && *meters_per_unit > 0.0
            && (*meters_per_unit * 1000.0).is_finite()
            && *meters_per_unit * 1000.0 > 0.0 =>
        {
            Some(*meters_per_unit * 1000.0)
        }
        UnitSystem::None | UnitSystem::Unset => None,
        UnitSystem::Custom { .. } => {
            return Err(FramingError::structural(
                reader.position(),
                "custom unit scale is invalid",
            ))
        }
    };
    if scale.is_some_and(|factor| !factor.is_finite() || factor <= 0.0) {
        return Err(FramingError::structural(
            reader.position(),
            "unit scale is invalid",
        ));
    }
    let absolute_tolerance_millimeters = scale
        .map(|factor| absolute * factor)
        .filter(|value| value.is_finite() && *value > 0.0);
    if scale.is_some() && absolute_tolerance_millimeters.is_none() {
        return Err(FramingError::structural(
            reader.position(),
            "scaled absolute tolerance is invalid",
        ));
    }
    finish(reader, "units")?;
    Ok(UnitsAndTolerances {
        version,
        unit_value,
        unit,
        millimeters_per_unit: scale,
        absolute_tolerance: absolute,
        absolute_tolerance_millimeters,
        angular_tolerance: angular,
        relative_tolerance: relative,
        distance_display_mode: mode,
        distance_display_precision: precision,
        source,
    })
}

fn anonymous_payload<'a>(
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    label: &str,
) -> Result<(BoundedReader<'a>, Range<usize>), FramingError> {
    let start = reader.position();
    let chunk = chunk_at(data, start, reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(FramingError::structural(
            start,
            format!("{label} must be a long anonymous chunk"),
        ));
    }
    let payload = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    reader.skip(chunk.next_offset - start)?;
    Ok((payload, chunk.range()))
}

fn anonymous_version(
    reader: &mut BoundedReader<'_>,
    label: &str,
) -> Result<(i32, i32), FramingError> {
    let version = (reader.i32()?, reader.i32()?);
    if version.0 != 1 || version.1 < 0 {
        return Err(FramingError::structural(
            reader.position(),
            format!("{label} version is unsupported"),
        ));
    }
    Ok(version)
}

fn parse_plugin_reference<'a>(
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
) -> Result<PluginReference, FramingError> {
    let (mut payload, range) = anonymous_payload(data, reader, archive, "plugin reference")?;
    let version = anonymous_version(&mut payload, "plugin reference")?;
    let plugin_id = uuid(&mut payload)?;
    let plugin_type = payload.i32()?;
    let name = utf16(&mut payload)?;
    let version_string = utf16(&mut payload)?;
    let filename = utf16(&mut payload)?;
    let (
        developer_organization,
        developer_address,
        developer_country,
        developer_phone,
        developer_email,
        developer_website,
        developer_update_url,
        developer_fax,
        platform,
        sdk_version,
        sdk_service_release,
    ) = if version.1 >= 1 {
        let developer_organization = utf16(&mut payload)?;
        let developer_address = utf16(&mut payload)?;
        let developer_country = utf16(&mut payload)?;
        let developer_phone = utf16(&mut payload)?;
        let developer_email = utf16(&mut payload)?;
        let developer_website = utf16(&mut payload)?;
        let developer_update_url = utf16(&mut payload)?;
        let developer_fax = utf16(&mut payload)?;
        let (platform, sdk_version, sdk_service_release) = if version.1 >= 2 {
            (
                Some(payload.i32()?),
                Some(payload.i32()?),
                Some(payload.i32()?),
            )
        } else {
            (None, None, None)
        };
        (
            Some(developer_organization),
            Some(developer_address),
            Some(developer_country),
            Some(developer_phone),
            Some(developer_email),
            Some(developer_website),
            Some(developer_update_url),
            Some(developer_fax),
            platform,
            sdk_version,
            sdk_service_release,
        )
    } else {
        (
            None, None, None, None, None, None, None, None, None, None, None,
        )
    };
    finish(&mut payload, "plugin reference")?;
    Ok(PluginReference {
        source: SourceRange { range },
        version,
        plugin_id,
        plugin_type,
        name,
        version_string,
        filename,
        developer_organization,
        developer_address,
        developer_country,
        developer_phone,
        developer_email,
        developer_website,
        developer_update_url,
        developer_fax,
        platform,
        sdk_version,
        sdk_service_release,
    })
}

pub(crate) fn parse_plugin_list(
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
) -> Result<PluginList, FramingError> {
    let mut reader = BoundedReader::new(data, record.body.start, record.body.end)?;
    let version = packed(&mut reader)?;
    if version.0 != 1 {
        return Err(FramingError::structural(
            reader.position(),
            "unsupported plugin-list version",
        ));
    }
    let count_offset = reader.position();
    let count = reader.i32()?;
    let count = crate::chunks::checked_count_bytes(
        count,
        1,
        reader.remaining(),
        MAX_ARRAY_ITEMS,
        count_offset,
    )?;
    let mut plugins = Vec::with_capacity(count);
    for _ in 0..count {
        plugins.push(parse_plugin_reference(data, &mut reader, archive)?);
    }
    finish(&mut reader, "plugin list")?;
    Ok(PluginList {
        source: SourceRange {
            range: record.range.clone(),
        },
        version,
        plugins,
    })
}

fn parse_earth_anchor<'a>(
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
) -> Result<EarthAnchorPoint, FramingError> {
    let (mut payload, _) = anonymous_payload(data, reader, archive, "earth anchor")?;
    let version = anonymous_version(&mut payload, "earth anchor")?;
    let earth_latitude = payload.f64()?;
    let earth_longitude = payload.f64()?;
    let earth_elevation_meters = payload.f64()?;
    let model_point = point(&mut payload)?;
    let model_north = vector(&mut payload)?;
    let model_east = vector(&mut payload)?;
    let (legacy_coordinate_system, id, name, description, url, url_tag, coordinate_system) =
        if version.1 >= 1 {
            let legacy = payload.i32()?;
            let id = uuid(&mut payload)?;
            let name = utf16(&mut payload)?;
            let description = utf16(&mut payload)?;
            let url = utf16(&mut payload)?;
            let url_tag = utf16(&mut payload)?;
            let coordinate_system = if version.1 >= 2 {
                Some(payload.i32()? as u32)
            } else {
                None
            };
            (
                Some(legacy),
                Some(id),
                Some(name),
                Some(description),
                Some(url),
                Some(url_tag),
                coordinate_system,
            )
        } else {
            (None, None, None, None, None, None, None)
        };
    finish(&mut payload, "earth anchor")?;
    Ok(EarthAnchorPoint {
        version,
        earth_latitude,
        earth_longitude,
        earth_elevation_meters,
        model_point,
        model_north,
        model_east,
        legacy_coordinate_system,
        id,
        name,
        description,
        url,
        url_tag,
        coordinate_system,
    })
}

fn parse_io_settings<'a>(
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
) -> Result<IoSettings, FramingError> {
    let (mut payload, _) = anonymous_payload(data, reader, archive, "IO settings")?;
    let version = anonymous_version(&mut payload, "IO settings")?;
    let save_texture_bitmaps_in_file = payload.bool()?;
    let mut idef_link_update = payload.i32()?;
    if idef_link_update == 0 && archive.value() >= 5 {
        idef_link_update = 1;
    }
    finish(&mut payload, "IO settings")?;
    Ok(IoSettings {
        version,
        save_texture_bitmaps_in_file,
        idef_link_update,
    })
}

fn parse_subd_display_parameters<'a>(
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
) -> Result<SubDDisplayParameters, FramingError> {
    let (mut payload, _) = anonymous_payload(data, reader, archive, "SubD display parameters")?;
    let version = anonymous_version(&mut payload, "SubD display parameters")?.1;
    let display_density = payload.i32()? as u32;
    let mesh_location = payload.i32()? as u32;
    let display_density_is_absolute = if version >= 2 {
        Some(payload.bool()?)
    } else {
        None
    };
    let compute_curvature = if version >= 3 {
        Some(payload.bool()?)
    } else {
        None
    };
    finish(&mut payload, "SubD display parameters")?;
    Ok(SubDDisplayParameters {
        version,
        display_density,
        mesh_location,
        display_density_is_absolute,
        compute_curvature,
    })
}

pub(crate) fn parse_mesh_parameters<'a>(
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    allow_future_minor: bool,
) -> Result<MeshParameters, FramingError> {
    let version = packed(reader)?;
    if version.0 != 1 || (!allow_future_minor && version.1 > 5) {
        return Err(FramingError::structural(
            reader.position(),
            "unsupported mesh-parameters version",
        ));
    }
    let compute_curvature = reader.i32()? != 0;
    let simple_planes = reader.i32()? != 0;
    let refine = reader.i32()? != 0;
    let jagged_seams = reader.i32()? != 0;
    let obsolete_weld = reader.i32()?;
    let tolerance = finite_f64(reader, "mesh tolerance")?;
    let min_edge_length = finite_f64(reader, "minimum mesh edge length")?;
    let max_edge_length = finite_f64(reader, "maximum mesh edge length")?;
    let grid_aspect_ratio = finite_f64(reader, "mesh grid aspect ratio")?;
    let grid_min_count = reader.i32()?;
    let grid_max_count = reader.i32()?;
    let grid_angle_radians = finite_f64(reader, "mesh grid angle")?;
    let grid_amplification = finite_f64(reader, "mesh grid amplification")?;
    let refine_angle_radians = finite_f64(reader, "mesh refine angle")?;
    let obsolete_combine_angle = finite_f64(reader, "mesh combine angle")?;
    let face_type = reader.i32()?;
    let texture_range = if version.1 >= 1 {
        Some(reader.i32()? as u32)
    } else {
        None
    };
    let (custom_settings, relative_tolerance) = if version.1 >= 2 {
        (
            Some(reader.bool()?),
            Some(finite_f64(reader, "mesh relative tolerance")?),
        )
    } else {
        (None, None)
    };
    let mesher = if version.1 >= 3 {
        Some(reader.u8()?)
    } else {
        None
    };
    let custom_settings_enabled = if version.1 >= 4 {
        Some(reader.bool()?)
    } else {
        None
    };
    let subd = if version.1 >= 5 {
        Some(parse_subd_display_parameters(data, reader, archive)?)
    } else {
        None
    };
    Ok(MeshParameters {
        version,
        compute_curvature,
        simple_planes,
        refine,
        jagged_seams,
        obsolete_weld,
        tolerance,
        min_edge_length,
        max_edge_length,
        grid_aspect_ratio,
        grid_min_count,
        grid_max_count,
        grid_angle_radians,
        grid_amplification,
        refine_angle_radians,
        obsolete_combine_angle,
        face_type,
        texture_range,
        custom_settings,
        relative_tolerance,
        mesher,
        custom_settings_enabled,
        subd,
    })
}

pub(crate) fn parse_settings_attributes(
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
) -> Result<SettingsAttributes, FramingError> {
    let mut reader = BoundedReader::new(data, record.body.start, record.body.end)?;
    let version = packed(&mut reader)?;
    if version.0 != 1 {
        return Err(FramingError::structural(
            reader.position(),
            "unsupported settings-attributes version",
        ));
    }
    let linetype_display_scale = finite_f64(&mut reader, "linetype display scale")?;
    let current_plot_color = color(&mut reader)?;
    let current_plot_color_source = reader.i32()?;
    let current_line_pattern_index = reader.i32()?;
    let current_linetype_source = reader.i32()?;
    let page_units = if version.1 >= 1 {
        let (mut payload, page_range) =
            anonymous_payload(data, &mut reader, archive, "settings-attributes page units")?;
        anonymous_version(&mut payload, "settings-attributes page-units wrapper")?;
        let value = parse_units_reader(&mut payload, SourceRange { range: page_range })?;
        Some(value)
    } else {
        None
    };
    let active_view_id = if version.1 >= 2 {
        Some(uuid(&mut reader)?)
    } else {
        None
    };
    let (model_basepoint, earth_anchor) = if version.1 >= 3 {
        let model_basepoint = point(&mut reader)?;
        let earth_anchor = parse_earth_anchor(data, &mut reader, archive)?;
        (Some(model_basepoint), Some(earth_anchor))
    } else {
        (None, None)
    };
    let save_texture_bitmaps_in_file = if version.1 >= 4 {
        Some(reader.bool()?)
    } else {
        None
    };
    let io_settings = if version.1 >= 5 {
        Some(parse_io_settings(data, &mut reader, archive)?)
    } else {
        None
    };
    let custom_render_mesh = if version.1 >= 6 {
        Some(parse_mesh_parameters(data, &mut reader, archive, false)?)
    } else {
        None
    };
    let (
        current_layer_id,
        current_render_material_id,
        current_line_pattern_id,
        current_text_style_id,
        current_dimension_style_id,
        current_hatch_pattern_id,
    ) = if version.1 >= 7 {
        (
            Some(uuid(&mut reader)?),
            Some(uuid(&mut reader)?),
            Some(uuid(&mut reader)?),
            Some(uuid(&mut reader)?),
            Some(uuid(&mut reader)?),
            Some(uuid(&mut reader)?),
        )
    } else {
        (None, None, None, None, None, None)
    };
    finish(&mut reader, "settings attributes")?;
    Ok(SettingsAttributes {
        source: SourceRange {
            range: record.range.clone(),
        },
        version,
        linetype_display_scale,
        current_plot_color,
        current_plot_color_source,
        current_line_pattern_index,
        current_linetype_source,
        page_units,
        active_view_id,
        model_basepoint,
        earth_anchor,
        save_texture_bitmaps_in_file,
        io_settings,
        custom_render_mesh,
        current_layer_id,
        current_render_material_id,
        current_line_pattern_id,
        current_text_style_id,
        current_dimension_style_id,
        current_hatch_pattern_id,
    })
}

fn parse_mesh_record(
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
) -> Result<MeshParameters, FramingError> {
    let mut reader = BoundedReader::new(data, record.body.start, record.body.end)?;
    let value = parse_mesh_parameters(data, &mut reader, archive, true)?;
    finish(&mut reader, "mesh settings")?;
    Ok(value)
}

#[derive(Clone, Copy)]
pub(crate) enum RenderingAttributesKind {
    Layer,
    Object,
}

/// Parses and consumes one bounded rendering-attributes payload.
pub(crate) fn parse_rendering_attributes(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    kind: RenderingAttributesKind,
    warnings: &mut Vec<String>,
) -> Result<Range<usize>, FramingError> {
    let start = reader.position();
    let chunk = crate::chunks::chunk_at(data, start, reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(FramingError::structural(
            reader.position(),
            "rendering attributes must be an anonymous chunk",
        ));
    }
    let mut payload = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    let major = payload.i32()?;
    let minor = payload.i32()?;
    if major != 1 || (matches!(kind, RenderingAttributesKind::Object) && minor < 1) {
        return Err(FramingError::structural(
            payload.position(),
            "unsupported rendering-attributes version",
        ));
    }
    let count = payload.i32()?;
    let count_bytes = crate::chunks::checked_count_bytes(
        count,
        1,
        payload.remaining(),
        MAX_ARRAY_ITEMS,
        payload.position(),
    )?;
    let count = count_bytes;
    let mut children = Vec::with_capacity(count);
    for _ in 0..count {
        let material =
            crate::chunks::chunk_at(data, payload.position(), payload.end(), archive, false)?;
        if material.typecode != ANONYMOUS || material.short {
            return Err(FramingError::structural(
                payload.position(),
                "rendering material reference must be anonymous",
            ));
        }
        if let Some(warning) = checksum_warning(data, &material)? {
            warnings.push(warning);
        }
        let mut material_payload =
            BoundedReader::new(data, material.body.start, material.body.end)?;
        let material_major = material_payload.i32()?;
        let material_minor = material_payload.i32()?;
        if material_major != 1 {
            return Err(FramingError::structural(
                material_payload.position(),
                "unsupported rendering material reference version",
            ));
        }
        material_payload.skip(16 + 16)?;
        let obsolete_mapping_count = material_payload.i32()?;
        if obsolete_mapping_count != 0 {
            return Err(FramingError::structural(
                material_payload.position(),
                "rendering material mapping array is not empty",
            ));
        }
        if material_minor >= 1 {
            material_payload.skip(16 + 4)?;
        }
        material_payload.skip_remaining()?;
        children.push(material.range());
        payload.skip(material.next_offset - payload.position())?;
    }
    if matches!(kind, RenderingAttributesKind::Object) {
        let mapping_count = crate::chunks::checked_count_bytes(
            payload.i32()?,
            1,
            payload.remaining(),
            MAX_ARRAY_ITEMS,
            payload.position(),
        )?;
        for _ in 0..mapping_count {
            let mapping =
                crate::chunks::chunk_at(data, payload.position(), payload.end(), archive, false)?;
            if mapping.typecode != ANONYMOUS || mapping.short {
                return Err(FramingError::structural(
                    payload.position(),
                    "rendering mapping reference must be anonymous",
                ));
            }
            let mut mapping_payload =
                BoundedReader::new(data, mapping.body.start, mapping.body.end)?;
            let mapping_major = mapping_payload.i32()?;
            let _mapping_minor = mapping_payload.i32()?;
            if mapping_major != 1 {
                return Err(FramingError::structural(
                    mapping_payload.position() - 4,
                    "unsupported rendering mapping reference version",
                ));
            }
            mapping_payload.skip(16)?;
            let channel_count = crate::chunks::checked_count_bytes(
                mapping_payload.i32()?,
                1,
                mapping_payload.remaining(),
                MAX_ARRAY_ITEMS,
                mapping_payload.position(),
            )?;
            let mut channels = Vec::with_capacity(channel_count);
            for _ in 0..channel_count {
                let channel = crate::chunks::chunk_at(
                    data,
                    mapping_payload.position(),
                    mapping_payload.end(),
                    archive,
                    false,
                )?;
                if channel.typecode != ANONYMOUS || channel.short {
                    return Err(FramingError::structural(
                        mapping_payload.position(),
                        "rendering mapping channel must be anonymous",
                    ));
                }
                let mut channel_payload =
                    BoundedReader::new(data, channel.body.start, channel.body.end)?;
                if channel_payload.i32()? != 1 {
                    return Err(FramingError::structural(
                        channel_payload.position() - 4,
                        "unsupported rendering mapping channel version",
                    ));
                }
                let channel_minor = channel_payload.i32()?;
                channel_payload.skip(4 + 16)?;
                if channel_minor >= 1 {
                    channel_payload.skip(16 * 8)?;
                }
                channel_payload.skip_remaining()?;
                mapping_payload.skip(channel.next_offset - mapping_payload.position())?;
                channels.push(channel.range());
            }
            mapping_payload.skip_remaining()?;
            if let Some(warning) = checksum_warning_excluding(data, &mapping, &channels)? {
                warnings.push(warning);
            }
            children.push(mapping.range());
            payload.skip(mapping.next_offset - payload.position())?;
        }
        if minor >= 2 {
            payload.bool()?;
            payload.bool()?;
        }
        if minor >= 3 {
            payload.bool()?;
        }
    }
    payload.skip_remaining()?;
    if let Some(warning) = checksum_warning_excluding(data, &chunk, &children)? {
        warnings.push(warning);
    }
    reader.skip(chunk.next_offset - reader.position())?;
    Ok(start..reader.position())
}

fn begin_direct_object<'a>(
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    label: &str,
) -> Result<(crate::chunks::Chunk, BoundedReader<'a>, (i32, i32)), FramingError> {
    let chunk = crate::chunks::chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(FramingError::structural(
            reader.position(),
            format!("{label} must be an object chunk"),
        ));
    }
    let mut payload = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    let version = (payload.i32()?, payload.i32()?);
    Ok((chunk, payload, version))
}

fn skip_model_attributes(
    data: &[u8],
    payload: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<Range<usize>, FramingError> {
    let chunk = crate::chunks::chunk_at(data, payload.position(), payload.end(), archive, false)?;
    if chunk.typecode != MODEL_ATTRIBUTES || chunk.short {
        return Err(FramingError::structural(
            payload.position(),
            "missing model-component attributes chunk",
        ));
    }
    if let Some(warning) = checksum_warning(data, &chunk)? {
        warnings.push(warning);
    }
    payload.skip(chunk.next_offset - payload.position())?;
    Ok(chunk.range())
}

fn read_segments(payload: &mut BoundedReader<'_>) -> Result<(), FramingError> {
    let count = payload.i32()?;
    let bytes = crate::chunks::checked_count_bytes(
        count,
        12,
        payload.remaining(),
        MAX_ARRAY_ITEMS,
        payload.position(),
    )?;
    let mut segment_reader = payload.unread()?;
    for _ in 0..(bytes / 12) {
        let length = segment_reader.f64()?;
        if !length.is_finite() {
            return Err(FramingError::structural(
                segment_reader.position(),
                "linetype segment length is not finite",
            ));
        }
        let kind = segment_reader.u32()?;
        if kind > 2 {
            return Err(FramingError::structural(
                segment_reader.position(),
                "linetype segment type is invalid",
            ));
        }
    }
    payload.skip(bytes)
}

/// Parses one direct embedded linetype object.
pub(crate) fn parse_direct_linetype<'a>(
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<EmbeddedDescriptor, FramingError> {
    let (chunk, mut payload, version) =
        begin_direct_object(data, reader, archive, "embedded linetype")?;
    let mut children = Vec::new();
    if (archive.value() < 60 && version != (1, 1))
        || (archive.value() >= 60 && (version.0 != 2 || version.1 < 1))
    {
        return Err(FramingError::structural(
            payload.position(),
            "unsupported embedded linetype version",
        ));
    }
    if version.0 == 1 {
        payload.i32()?;
        utf16(&mut payload)?;
        read_segments(&mut payload)?;
        if version.1 >= 1 {
            uuid(&mut payload)?;
        }
    } else {
        children.push(skip_model_attributes(
            data,
            &mut payload,
            archive,
            warnings,
        )?);
        read_segments(&mut payload)?;
        if version.1 >= 1 {
            // ON_Linetype::Read() consumes extension IDs through an ordered
            // cascade. A duplicate, out-of-order, or future ID ends the
            // typed scan; its value has no generic width.
            let mut item = payload.u8()?;
            if item == 1 {
                payload.skip(1)?;
                item = payload.u8()?;
            }
            if item == 2 {
                payload.skip(1)?;
                item = payload.u8()?;
            }
            if version.1 >= 2 {
                if item == 3 {
                    let value = payload.f64()?;
                    if !value.is_finite() {
                        return Err(FramingError::structural(
                            payload.position(),
                            "linetype width is not finite",
                        ));
                    }
                    item = payload.u8()?;
                }
                if item == 4 {
                    payload.skip(1)?;
                    item = payload.u8()?;
                }
                if item == 5 {
                    let count = payload.i32()?;
                    let bytes = crate::chunks::checked_count_bytes(
                        count,
                        16,
                        payload.remaining(),
                        MAX_ARRAY_ITEMS,
                        payload.position(),
                    )?;
                    payload.skip(bytes)?;
                    item = payload.u8()?;
                }
            }
            if version.1 >= 3 && item == 6 {
                let _ = payload.bool()?;
                let _next_item = payload.u8()?;
            }
        }
    }
    finish(&mut payload, "embedded linetype")?;
    if let Some(warning) = checksum_warning_excluding(data, &chunk, &children)? {
        warnings.push(warning);
    }
    reader.skip(chunk.next_offset - reader.position())?;
    Ok(EmbeddedDescriptor {
        source: SourceRange {
            range: chunk.range(),
        },
        version,
    })
}

/// Parses one direct embedded section-style object.
pub(crate) fn parse_direct_section_style<'a>(
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<EmbeddedDescriptor, FramingError> {
    let (chunk, mut payload, version) =
        begin_direct_object(data, reader, archive, "embedded section style")?;
    if version.0 != 1 {
        return Err(FramingError::structural(
            payload.position(),
            "unsupported embedded section-style version",
        ));
    }
    let mut children = vec![skip_model_attributes(
        data,
        &mut payload,
        archive,
        warnings,
    )?];
    // ON_SectionStyle::Read() is an ordered cascade, not a general item
    // loop. Each recognized item reads the next item ID and only the later
    // IDs in the cascade can consume it. A duplicate or out-of-order ID has
    // no source-defined value width and remains bounded suffix data.
    let mut item = payload.u8()?;
    if item == 1 {
        payload.skip(1)?;
        item = payload.u8()?;
    }
    if item == 2 {
        payload.skip(8)?;
        item = payload.u8()?;
    }
    if item == 3 {
        let _ = payload.bool()?;
        item = payload.u8()?;
    }
    if item == 4 {
        payload.skip(8)?;
        item = payload.u8()?;
    }
    if item == 5 {
        let value = payload.f64()?;
        if !value.is_finite() {
            return Err(FramingError::structural(
                payload.position(),
                "section-style value is not finite",
            ));
        }
        item = payload.u8()?;
    }
    if item == 6 {
        payload.skip(1)?;
        item = payload.u8()?;
    }
    if item == 7 {
        let _ = payload.i32()?;
        item = payload.u8()?;
    }
    if item == 8 {
        let value = payload.f64()?;
        if !value.is_finite() {
            return Err(FramingError::structural(
                payload.position(),
                "section-style value is not finite",
            ));
        }
        item = payload.u8()?;
    }
    if item == 9 {
        let value = payload.f64()?;
        if !value.is_finite() {
            return Err(FramingError::structural(
                payload.position(),
                "section-style value is not finite",
            ));
        }
        item = payload.u8()?;
    }
    if item == 10 {
        payload.skip(8)?;
        item = payload.u8()?;
    }
    if item == 11 {
        children.push(
            parse_direct_linetype(data, &mut payload, archive, warnings)?
                .source
                .range,
        );
        // The source reader consumes the following ID to decide whether the
        // cascade can continue, but does not need to interpret it: no later
        // section-style item follows code 11.
        let _next_item = payload.u8()?;
    }
    // Extension items have no length prefix. The source reader consumes only
    // the ID and lets the anonymous-chunk boundary discard the value bytes it
    // cannot type. A lower or duplicate known ID has the same bounded-suffix
    // result because the cascade has passed it.
    finish(&mut payload, "embedded section style")?;
    if let Some(warning) = checksum_warning_excluding(data, &chunk, &children)? {
        warnings.push(warning);
    }
    reader.skip(chunk.next_offset - reader.position())?;
    Ok(EmbeddedDescriptor {
        source: SourceRange {
            range: chunk.range(),
        },
        version,
    })
}

fn checksum_warning(
    data: &[u8],
    chunk: &crate::chunks::Chunk,
) -> Result<Option<String>, FramingError> {
    match crate::chunks::verify_checksum(data, chunk)? {
        crate::chunks::ChecksumStatus::Mismatch { expected, actual } => Ok(Some(format!(
            "CRC mismatch at offset {} for typecode {:#x}: expected {expected:#x}, got {actual:#x}",
            chunk.header_start, chunk.typecode
        ))),
        _ => Ok(None),
    }
}

fn checksum_warning_excluding(
    data: &[u8],
    chunk: &crate::chunks::Chunk,
    children: &[Range<usize>],
) -> Result<Option<String>, FramingError> {
    let ranges = crate::chunks::direct_checksum_ranges(&chunk.body, children)?;
    match crate::chunks::verify_checksum_ranges(data, chunk, &ranges)? {
        crate::chunks::ChecksumStatus::Mismatch { expected, actual } => Ok(Some(format!(
            "CRC mismatch at offset {} for typecode {:#x}: expected {expected:#x}, got {actual:#x}",
            chunk.header_start, chunk.typecode
        ))),
        _ => Ok(None),
    }
}

fn parse_layer(
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    warnings: &mut Vec<String>,
    losses: &mut Vec<cadmpeg_ir::report::LossNote>,
) -> Result<(LayerRecord, bool), FramingError> {
    let (class, userdata) =
        parse_class_wrapper_with_userdata(data, record.body.clone(), archive, warnings)?;
    if class.class_uuid != ON_LAYER_UUID {
        return Err(FramingError::Structural {
            offset: record.range.start,
            message: format!("layer record has class UUID {}", class.class_uuid),
        });
    }
    let mut reader = BoundedReader::new(
        data,
        class.class_data_range.start,
        class.class_data_range.end,
    )?;
    let version = packed(&mut reader)?;
    if version.0 != 1 || version.1 > 15 {
        return Err(FramingError::structural(
            reader.position(),
            "unsupported layer version",
        ));
    }
    let obsolete_mode = reader.i32()?;
    let index = reader.i32()?;
    let iges_level = reader.i32()?;
    let render_material_index = reader.i32()?;
    let _obsolete_model_index = reader.i32()?;
    let layer_color = color(&mut reader)?;
    let _obsolete_line_style = reader.i16()?;
    let _obsolete_line_style_index = reader.i16()?;
    let thickness_raw = reader.f64()?;
    let _obsolete_thickness = finite(&reader, thickness_raw, "layer thickness")?;
    let scale_raw = reader.f64()?;
    let _obsolete_scale = finite(&reader, scale_raw, "layer scale")?;
    let name = utf16(&mut reader)?;
    let visible = if version.1 >= 1 {
        reader.bool_with_writer_version(writer_version)?
    } else {
        obsolete_mode != 1
    };
    let linetype_index = (version.1 >= 2).then(|| reader.i32()).transpose()?;
    let plot_color = if version.1 >= 3 {
        Some(color(&mut reader)?)
    } else {
        None
    };
    let plot_weight = if version.1 >= 3 {
        let plot_weight_raw = reader.f64()?;
        Some(finite(&reader, plot_weight_raw, "plot weight")?)
    } else {
        None
    };
    let locked = if version.1 >= 4 {
        reader.bool_with_writer_version(writer_version)?
    } else {
        obsolete_mode == 2
    };
    let id = (version.1 >= 5).then(|| uuid(&mut reader)).transpose()?;
    let parent_compatible = writer_version.is_some_and(|version| version > 200_505_110);
    if version.1 >= 6 && writer_version.is_none() {
        losses.push(crate::loss::writer_stamp_unverified(
            "layer parent link and expanded state were not read because the archive has no writer-version stamp",
        ));
    }
    let parent_id = if version.1 >= 6 && parent_compatible {
        Some(uuid(&mut reader)?)
    } else {
        None
    };
    let expanded = if version.1 >= 6 && parent_compatible {
        Some(reader.bool_with_writer_version(writer_version)?)
    } else {
        None
    };
    let rendering_range = if version.1 >= 7 {
        Some(
            parse_rendering_attributes(
                data,
                &mut reader,
                archive,
                RenderingAttributesKind::Layer,
                warnings,
            )
            .map_err(|error| {
                FramingError::structural(reader.position(), format!("rendering: {error}"))
            })?,
        )
    } else {
        None
    };
    let display_material_id = (version.1 >= 8)
        .then(|| uuid(&mut reader))
        .transpose()
        .map_err(|error| {
            FramingError::structural(reader.position(), format!("display material: {error}"))
        })?;
    if version.1 == 9 {
        reader.skip(2)?;
    }
    let mut layer = LayerRecord {
        source: SourceRange {
            range: record.range.clone(),
        },
        version,
        obsolete_mode,
        index,
        iges_level,
        render_material_index,
        color: layer_color,
        name,
        description: None,
        visible,
        locked,
        id,
        parent_id,
        expanded,
        linetype_index,
        plot_color,
        plot_weight,
        display_material_id,
        no_clipping_planes: None,
        visible_in_new_details: None,
        rendering_range,
        extension_items: Vec::new(),
        embedded_linetype: None,
        embedded_section_style: None,
        per_viewport_settings: Vec::new(),
    };
    let mut userdata_degraded = false;
    if let Some(descriptor) = userdata.iter().find(|descriptor| {
        descriptor.class_uuid() == LAYER_EXTENSIONS && descriptor.item_uuid() == LAYER_EXTENSIONS
    }) {
        match parse_layer_extensions(data, descriptor, archive, layer.parent_id) {
            Ok(settings) => layer.per_viewport_settings = settings,
            Err(error) => {
                userdata_degraded = true;
                warnings.push(format!(
                    "layer per-viewport userdata at offset {} could not be transferred: {error}",
                    descriptor.range().start
                ));
            }
        }
    }
    if version.1 >= 10 {
        // ON_Layer::Read() consumes extension IDs through an ascending,
        // version-gated cascade. A duplicate, out-of-order, or future ID is
        // consumed only as an ID; its value has no generic width.
        let mut item = reader.u8()?;
        if item == 28 {
            layer.extension_items.push(item);
            layer.no_clipping_planes = Some(reader.bool_with_writer_version(writer_version)?);
            read_uuid_list(&mut reader, archive)?;
            item = reader.u8()?;
        }
        if version.1 > 10 {
            if item == 29 {
                layer.extension_items.push(item);
                reader.skip(4)?;
                item = reader.u8()?;
            }
            if item == 30 {
                layer.extension_items.push(item);
                let value = reader.f64()?;
                finite(&reader, value, "layer extension value")?;
                item = reader.u8()?;
            }
            if item == 31 {
                layer.extension_items.push(item);
                let value = reader.f64()?;
                finite(&reader, value, "layer extension value")?;
                item = reader.u8()?;
            }
        }
        if version.1 > 11 && item == 32 {
            layer.extension_items.push(item);
            reader.skip(1)?;
            item = reader.u8()?;
        }
        if version.1 > 12 && item == 33 {
            layer.extension_items.push(item);
            layer.embedded_linetype =
                Some(parse_direct_linetype(data, &mut reader, archive, warnings)?);
            item = reader.u8()?;
        }
        if version.1 > 13 && item == 34 {
            layer.extension_items.push(item);
            layer.visible_in_new_details = Some(reader.bool_with_writer_version(writer_version)?);
            item = reader.u8()?;
        }
        if version.1 > 14 {
            if item == 35 {
                layer.extension_items.push(item);
                layer.embedded_section_style = Some(parse_direct_section_style(
                    data,
                    &mut reader,
                    archive,
                    warnings,
                )?);
                item = reader.u8()?;
            }
            if item == 36 {
                layer.extension_items.push(item);
                reader.skip(1)?;
                item = reader.u8()?;
            }
            if item == 37 {
                layer.extension_items.push(item);
                let description = utf16(&mut reader)?;
                let description = description
                    .trim_matches(|character: char| {
                        matches!(
                            character as u32,
                            0x0001..=0x0020
                                | 0x007f
                                | 0x0080..=0x009f
                                | 0x00a0
                                | 0x2000..=0x200b
                                | 0x200e..=0x200f
                                | 0x2028..=0x202f
                                | 0x2066..=0x2069
                        )
                    })
                    .to_owned();
                layer.description = (!description.is_empty()).then_some(description);
                let _next_item = reader.u8()?;
            }
        }
    }
    finish(&mut reader, "layer payload")?;
    Ok((layer, userdata_degraded))
}

/// Decodes all metadata records while preserving scan framing.
pub(crate) fn parse_metadata(
    data: &[u8],
    archive: ArchiveVersion,
    tables: &[Table],
    warnings: &mut Vec<String>,
) -> DocumentMetadata {
    let mut metadata = DocumentMetadata::default();
    let mut ids = BTreeSet::new();
    let mut property_singletons = BTreeSet::new();
    let mut setting_singletons = BTreeSet::new();
    let mut opaque_records = Vec::new();
    for table in tables {
        let table_type = table.typecode & !0x0000_8000;
        for record in &table.records {
            let singleton = match table_type {
                PROPERTIES => matches!(
                    record.typecode,
                    WRITER_VERSION | REVISION_HISTORY | NOTES | APPLICATION | AS_FILE_NAME
                ),
                SETTINGS => matches!(
                    record.typecode,
                    PLUGIN_LIST
                        | UNITS
                        | RENDER_MESH
                        | ANALYSIS_MESH
                        | ATTRIBUTES
                        | CURRENT_LAYER
                        | CURRENT_MATERIAL
                        | CURRENT_COLOR
                        | CURRENT_WIRE_DENSITY
                        | CURRENT_FONT
                        | CURRENT_DIMSTYLE
                        | MODEL_URL
                ),
                _ => false,
            };
            let duplicate_singleton = singleton
                && match table_type {
                    PROPERTIES => property_singletons.contains(&record.typecode),
                    SETTINGS => setting_singletons.contains(&record.typecode),
                    _ => false,
                };
            let result = if table_type == PROPERTIES {
                match record.typecode {
                    WRITER_VERSION if record.short => {
                        metadata.properties.writer_version = Some(record.value);
                        Ok(())
                    }
                    REVISION_HISTORY => parse_revision(data, record)
                        .map(|value| metadata.properties.revision_history = Some(value)),
                    NOTES => parse_notes(data, record)
                        .map(|value| metadata.properties.notes = Some(value)),
                    APPLICATION => parse_application(data, record)
                        .map(|value| metadata.properties.application = Some(value)),
                    AS_FILE_NAME => utf16_record(data, record)
                        .map(|value| metadata.properties.as_file_name = Some(value)),
                    PREVIEW | COMPRESSED_PREVIEW => {
                        metadata.properties.previews.push(PreviewDescriptor {
                            source: SourceRange {
                                range: record.range.clone(),
                            },
                            compressed: record.typecode == COMPRESSED_PREVIEW,
                            payload_bytes: record.body.len(),
                        });
                        Ok(())
                    }
                    _ => Ok(()),
                }
            } else if table_type == SETTINGS {
                parse_setting(data, record, &mut metadata.settings, archive)
            } else if table_type == LAYER && record.typecode == LAYER_RECORD {
                let writer_version = metadata.properties.writer_version;
                match parse_layer(
                    data,
                    record,
                    archive,
                    writer_version,
                    warnings,
                    &mut metadata.losses,
                ) {
                    Ok((layer, userdata_degraded)) => {
                        if let Some(id) = layer.id {
                            if !ids.insert(id) {
                                warnings.push(format!(
                                    "duplicate layer UUID {id}; first record owns archive identity"
                                ));
                            }
                        }
                        metadata.layers.push(layer);
                        if userdata_degraded {
                            opaque_records.push(OpaqueRecord {
                                table_typecode: table.typecode,
                                record: record.clone(),
                            });
                        }
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            } else {
                Ok(())
            };
            if result.is_ok() && singleton {
                match table_type {
                    PROPERTIES => {
                        property_singletons.insert(record.typecode);
                    }
                    SETTINGS => {
                        setting_singletons.insert(record.typecode);
                    }
                    _ => {}
                }
                if duplicate_singleton {
                    warnings.push(format!(
                        "duplicate singleton metadata record {:#x}; later record wins",
                        record.typecode
                    ));
                }
            }
            if let Err(error) = result {
                if matches!(table_type, PROPERTIES | SETTINGS | LAYER)
                    && (table_type != LAYER || record.typecode == LAYER_RECORD)
                {
                    opaque_records.push(OpaqueRecord {
                        table_typecode: table.typecode,
                        record: record.clone(),
                    });
                }
                warnings.push(format!(
                    "metadata record {:#x} at {} degraded: {}",
                    record.typecode, record.range.start, error
                ));
            }
        }
    }
    reassign_duplicate_layer_indices(&mut metadata.layers, warnings);
    metadata.opaque_records = opaque_records;
    let known_ids: BTreeSet<Uuid> = metadata
        .layers
        .iter()
        .filter_map(|layer| layer.id)
        .collect();
    for layer in &metadata.layers {
        if let Some(parent) = layer.parent_id {
            if !parent.is_nil() && !known_ids.contains(&parent) {
                warnings.push(format!(
                    "layer {} references missing parent UUID {parent}",
                    layer.index
                ));
            }
        }
    }
    metadata
}

fn reassign_duplicate_layer_indices(layers: &mut [LayerRecord], warnings: &mut Vec<String>) {
    let mut used = layers
        .iter()
        .map(|layer| layer.index)
        .collect::<BTreeSet<_>>();
    let mut owners = BTreeSet::new();
    for layer in layers {
        let original_index = layer.index;
        if owners.insert(original_index) {
            continue;
        }
        let new_index = next_layer_index(&used);
        layer.index = new_index;
        used.insert(new_index);
        warnings.push(format!(
            "duplicate layer index {original_index}; later record assigned new index {new_index}; first record owns archive references"
        ));
    }
}

fn next_layer_index(used: &BTreeSet<i32>) -> i32 {
    let mut candidate = used
        .iter()
        .copied()
        .filter(|index| *index >= 0)
        .max()
        .unwrap_or(-1);
    while let Some(next) = candidate.checked_add(1) {
        if !used.contains(&next) {
            return next;
        }
        candidate = next;
    }
    let mut candidate = -2;
    while used.contains(&candidate) {
        candidate = candidate
            .checked_sub(1)
            .expect("finite layer index set leaves an available index");
    }
    candidate
}

fn utf16_record(data: &[u8], record: &Record) -> Result<String, FramingError> {
    let mut reader = BoundedReader::new(data, record.body.start, record.body.end)?;
    let value = utf16(&mut reader)?;
    finish(&mut reader, "UTF-16 property")?;
    Ok(value)
}

pub(crate) fn parse_setting(
    data: &[u8],
    record: &Record,
    settings: &mut DocumentSettings,
    archive: ArchiveVersion,
) -> Result<(), FramingError> {
    match record.typecode {
        PLUGIN_LIST => {
            parse_plugin_list(data, record, archive).map(|value| settings.plugin_list = Some(value))
        }
        UNITS => parse_units(data, record).map(|value| settings.units = Some(value)),
        RENDER_MESH => parse_mesh_record(data, record, archive)
            .map(|value| settings.render_mesh_settings = Some(value)),
        ANALYSIS_MESH => parse_mesh_record(data, record, archive)
            .map(|value| settings.analysis_mesh_settings = Some(value)),
        ATTRIBUTES => parse_settings_attributes(data, record, archive)
            .map(|value| settings.attributes = Some(value)),
        CURRENT_LAYER => {
            settings.current_layer = Some(short_index(record, "current layer")?);
            Ok(())
        }
        CURRENT_MATERIAL => {
            if record.short || record.body.len() < 8 {
                return Err(FramingError::Structural {
                    offset: record.range.start,
                    message: "current material must be a long eight-byte index/source pair"
                        .to_string(),
                });
            }
            let material_index = View::i32_le_at(data, record.body.start).expect("length checked");
            settings.current_material = Some(material_index);
            settings.current_material_source =
                Some(View::i32_le_at(data, record.body.start + 4).expect("length checked"));
            Ok(())
        }
        CURRENT_COLOR => {
            if record.short || record.body.len() < 8 {
                return Err(FramingError::Structural {
                    offset: record.range.start,
                    message: "current color must be a long color/source pair".to_string(),
                });
            }
            settings.current_color = Some(
                data[record.body.start..record.body.start + 4]
                    .try_into()
                    .expect("length checked"),
            );
            settings.current_color_source =
                Some(View::i32_le_at(data, record.body.start + 4).expect("length checked"));
            Ok(())
        }
        CURRENT_WIRE_DENSITY => {
            if !record.short || record.value < -2 || record.value > i64::from(i32::MAX) {
                return Err(FramingError::Structural {
                    offset: record.range.start,
                    message: "current wire density is not a valid short value".to_string(),
                });
            }
            settings.current_wire_density = Some(record.value);
            Ok(())
        }
        CURRENT_FONT => {
            settings.current_font = Some(short_index(record, "current font")?);
            Ok(())
        }
        CURRENT_DIMSTYLE => {
            settings.current_dimstyle = Some(short_index(record, "current dimstyle")?);
            Ok(())
        }
        MODEL_URL => utf16_record(data, record).map(|value| settings.model_url = Some(value)),
        _ => {
            settings.unsupported.push(SettingDescriptor {
                typecode: record.typecode,
                source: SourceRange {
                    range: record.range.clone(),
                },
                payload_bytes: record.body.len(),
            });
            Ok(())
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
