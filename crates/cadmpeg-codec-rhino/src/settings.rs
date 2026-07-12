// SPDX-License-Identifier: Apache-2.0
//! Bounded Rhino document properties, settings, units, and layer metadata.

use std::collections::BTreeSet;
use std::ops::Range;

use crate::chunks::{ArchiveVersion, BoundedReader, FramingError};
use crate::container::{Record, Table};
use crate::objects::parse_class_wrapper;
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
const CURRENT_LAYER: u32 = 0xa000_0038;
const CURRENT_MATERIAL: u32 = 0x2000_8039;
const CURRENT_COLOR: u32 = 0x2000_803a;
const CURRENT_WIRE_DENSITY: u32 = 0xa000_003c;
const MODEL_URL: u32 = 0x2000_8131;
const CURRENT_FONT: u32 = 0xa000_0132;
const CURRENT_DIMSTYLE: u32 = 0xa000_0133;
const ON_LAYER_UUID: Uuid = Uuid::from_canonical([
    0x95, 0x80, 0x98, 0x13, 0xe9, 0x85, 0x11, 0xd3, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const ANONYMOUS: u32 = 0x4000_8000;
const MODEL_ATTRIBUTES: u32 = 0x4000_8002;

/// A source range in the original archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceRange {
    /// Complete chunk range.
    pub(crate) range: Range<usize>,
}

/// A finite three-dimensional point.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code, reason = "shared bounded wire primitive")]
pub(crate) struct Point3(pub(crate) [f64; 3]);

/// A finite three-dimensional vector.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code, reason = "shared bounded wire primitive")]
pub(crate) struct Vector3(pub(crate) [f64; 3]);

/// A serialized parameter interval.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code, reason = "shared bounded wire primitive")]
pub(crate) struct Interval(pub(crate) [f64; 2]);

/// A serialized plane, including its wire equation.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code, reason = "bounded parsed metadata retained for inspection")]
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
#[allow(dead_code, reason = "bounded parsed metadata retained for inspection")]
pub(crate) struct BoundingBox {
    /// Minimum point.
    pub(crate) minimum: Point3,
    /// Maximum point.
    pub(crate) maximum: Point3,
}

/// A serialized row-major 4×4 transform.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code, reason = "bounded parsed metadata retained for inspection")]
pub(crate) struct Xform(pub(crate) [f64; 16]);

/// A UTF-16 UTC time tuple as written by Rhino.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UtcTime {
    /// The eight serialized fields in seconds, minutes, hours, and calendar order.
    pub(crate) fields: [i32; 8],
}

/// Decoded document properties.
#[derive(Debug, Clone, Default)]
#[allow(
    dead_code,
    reason = "bounded parsed metadata retained for source reporting"
)]
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
#[allow(
    dead_code,
    reason = "bounded parsed metadata retained for source reporting"
)]
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
#[allow(
    dead_code,
    reason = "bounded parsed metadata retained for source reporting"
)]
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
#[allow(
    dead_code,
    reason = "bounded parsed metadata retained for source reporting"
)]
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
#[allow(
    dead_code,
    reason = "bounded parsed metadata retained for source reporting"
)]
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

/// A bounded unsupported setting payload.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "bounded descriptor intentionally omits payload bytes"
)]
pub(crate) struct SettingDescriptor {
    /// Record typecode.
    pub(crate) typecode: u32,
    /// Complete source range.
    pub(crate) source: SourceRange,
    /// Payload byte length.
    pub(crate) payload_bytes: usize,
}

/// Current document selectors and bounded unsupported settings.
#[derive(Debug, Clone, Default)]
#[allow(
    dead_code,
    reason = "bounded parsed metadata retained for source reporting"
)]
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
    /// Unsupported known settings.
    pub(crate) unsupported: Vec<SettingDescriptor>,
}

/// Layer metadata decoded without attributes or geometry.
#[derive(Debug, Clone)]
#[allow(
    dead_code,
    reason = "bounded parsed metadata retained for source reporting"
)]
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
    /// Bounded rendering payload range.
    pub(crate) rendering_range: Option<Range<usize>>,
    /// Raw extension item IDs successfully consumed.
    pub(crate) extension_items: Vec<u8>,
    /// Direct embedded linetype descriptor.
    pub(crate) embedded_linetype: Option<EmbeddedDescriptor>,
    /// Direct embedded section-style descriptor.
    pub(crate) embedded_section_style: Option<EmbeddedDescriptor>,
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
#[allow(
    dead_code,
    reason = "bounded parsed metadata retained for source reporting"
)]
pub(crate) struct DocumentMetadata {
    /// Document properties.
    pub(crate) properties: Properties,
    /// Document settings.
    pub(crate) settings: DocumentSettings,
    /// Layer records.
    pub(crate) layers: Vec<LayerRecord>,
}

fn structural(reader: &BoundedReader<'_>, message: impl Into<String>) -> FramingError {
    FramingError::Structural {
        offset: reader.position(),
        message: message.into(),
    }
}

fn finite(reader: &BoundedReader<'_>, value: f64, label: &str) -> Result<f64, FramingError> {
    value
        .is_finite()
        .then_some(value)
        .ok_or_else(|| structural(reader, format!("{label} is not finite")))
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
        .ok_or_else(|| structural(reader, format!("{label} contains a nonfinite value")))
}

/// Reads a finite point.
#[allow(dead_code, reason = "shared bounded wire parser")]
pub(crate) fn point(reader: &mut BoundedReader<'_>) -> Result<Point3, FramingError> {
    let values = [reader.f64()?, reader.f64()?, reader.f64()?];
    Ok(Point3(finite_array(reader, values, "point")?))
}

/// Reads a finite vector.
#[allow(dead_code, reason = "shared bounded wire parser")]
pub(crate) fn vector(reader: &mut BoundedReader<'_>) -> Result<Vector3, FramingError> {
    let values = [reader.f64()?, reader.f64()?, reader.f64()?];
    Ok(Vector3(finite_array(reader, values, "vector")?))
}

/// Reads a finite interval.
#[allow(dead_code, reason = "shared bounded wire parser")]
pub(crate) fn interval(reader: &mut BoundedReader<'_>) -> Result<Interval, FramingError> {
    let values = [reader.f64()?, reader.f64()?];
    Ok(Interval(finite_array(reader, values, "interval")?))
}

/// Reads a finite plane without reconstructing its serialized equation.
#[allow(dead_code, reason = "shared bounded wire parser")]
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
#[allow(dead_code, reason = "shared bounded wire parser")]
pub(crate) fn bbox(reader: &mut BoundedReader<'_>) -> Result<BoundingBox, FramingError> {
    Ok(BoundingBox {
        minimum: point(reader)?,
        maximum: point(reader)?,
    })
}

/// Reads a finite row-major transform.
#[allow(dead_code, reason = "shared bounded wire parser")]
pub(crate) fn xform(reader: &mut BoundedReader<'_>) -> Result<Xform, FramingError> {
    let mut values = [0.0; 16];
    for value in &mut values {
        *value = reader.f64()?;
    }
    Ok(Xform(finite_array(reader, values, "transform")?))
}

/// Decodes an archive UTF-8 string for later plugin/settings records.
#[allow(dead_code, reason = "shared bounded wire parser")]
pub(crate) fn utf8(reader: &mut BoundedReader<'_>) -> Result<String, FramingError> {
    let count_offset = reader.position();
    let count =
        usize::try_from(reader.u32()?).map_err(|_| structural(reader, "UTF-8 count overflow"))?;
    if count > MAX_STRING_BYTES || count > reader.remaining() {
        return Err(structural(
            reader,
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
        .map_err(|_| structural(reader, "invalid UTF-8 string"))
}

pub(crate) fn utf16(reader: &mut BoundedReader<'_>) -> Result<String, FramingError> {
    let count_offset = reader.position();
    let count =
        usize::try_from(reader.u32()?).map_err(|_| structural(reader, "UTF-16 count overflow"))?;
    if count > MAX_STRING_BYTES / 2 || count.checked_mul(2).is_none_or(|n| n > reader.remaining()) {
        return Err(structural(
            reader,
            format!("UTF-16 count {count} exceeds bounded string limit"),
        ));
    }
    if count == 0 {
        return Ok(String::new());
    }
    let mut values = Vec::with_capacity(count.saturating_sub(1));
    for _ in 0..count {
        values.push(u16::from_le_bytes(
            reader.take(2)?.try_into().expect("length checked"),
        ));
    }
    if values.pop() != Some(0) {
        return Err(FramingError::Structural {
            offset: count_offset,
            message: "UTF-16 string is missing NUL terminator".to_string(),
        });
    }
    String::from_utf16(&values).map_err(|_| structural(reader, "invalid UTF-16 surrogate sequence"))
}

fn uuid(reader: &mut BoundedReader<'_>) -> Result<Uuid, FramingError> {
    Ok(Uuid::from_wire(
        reader.take(16)?.try_into().expect("length checked"),
    ))
}

fn color(reader: &mut BoundedReader<'_>) -> Result<[u8; 4], FramingError> {
    Ok(reader.take(4)?.try_into().expect("length checked"))
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

fn finish(reader: &BoundedReader<'_>, label: &str) -> Result<(), FramingError> {
    if reader.remaining() != 0 {
        return Err(structural(reader, format!("{label} has trailing bytes")));
    }
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
    if version != (1, 0) {
        return Err(structural(&reader, "unsupported revision-history version"));
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
    finish(&reader, "revision-history")?;
    Ok(value)
}

fn parse_notes(data: &[u8], record: &Record) -> Result<Notes, FramingError> {
    let mut reader = BoundedReader::new(data, record.body.start, record.body.end)?;
    let version = packed(&mut reader)?;
    if version.0 != 1 || version.1 > 1 {
        return Err(structural(&reader, "unsupported notes version"));
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
    finish(&reader, "notes")?;
    Ok(value)
}

fn parse_application(data: &[u8], record: &Record) -> Result<Application, FramingError> {
    let mut reader = BoundedReader::new(data, record.body.start, record.body.end)?;
    let version = packed(&mut reader)?;
    if version != (1, 0) {
        return Err(structural(&reader, "unsupported application version"));
    }
    let value = Application {
        source: SourceRange {
            range: record.range.clone(),
        },
        name: utf16(&mut reader)?,
        url: utf16(&mut reader)?,
        details: utf16(&mut reader)?,
    };
    finish(&reader, "application")?;
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
        23 => 149_597_870_700_000.0,
        24 => 9.460_730_472e18,
        25 => 3.085_677_581_491_367e19,
        _ => return None,
    })
}

pub(crate) fn parse_units(
    data: &[u8],
    record: &Record,
) -> Result<UnitsAndTolerances, FramingError> {
    let mut reader = BoundedReader::new(data, record.body.start, record.body.end)?;
    let version = reader.i32()?;
    let legacy = version == 1;
    if !legacy && !(100..=102).contains(&version) {
        return Err(structural(&reader, "unsupported units structure version"));
    }
    let unit_value = reader.i32()?;
    let absolute_raw = reader.f64()?;
    let absolute = finite(&reader, absolute_raw, "absolute tolerance")?;
    let (relative, angular) = if legacy {
        let relative = reader.f64()?;
        let angular = reader.f64()?;
        (relative, angular)
    } else {
        let angular = reader.f64()?;
        let relative = reader.f64()?;
        (relative, angular)
    };
    let angular = finite(&reader, angular, "angular tolerance")?;
    let relative = finite(&reader, relative, "relative tolerance")?;
    if absolute <= 0.0 {
        return Err(structural(&reader, "absolute tolerance must be positive"));
    }
    if angular <= 0.0 || angular > std::f64::consts::PI {
        return Err(structural(&reader, "angular tolerance must be in (0, pi]"));
    }
    if relative <= 0.0 || relative >= 1.0 {
        return Err(structural(&reader, "relative tolerance must be in (0, 1)"));
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
        Some(utf16(&mut reader)?)
    } else {
        None
    };
    let unit = match unit_value {
        0 => UnitSystem::None,
        11 => UnitSystem::Custom {
            meters_per_unit: custom_scale
                .ok_or_else(|| structural(&reader, "custom unit has no scale"))?,
            name: custom_name.unwrap_or_default(),
        },
        255 => UnitSystem::Unset,
        value if standard_scale(value).is_some() => UnitSystem::Standard(
            u8::try_from(value).map_err(|_| structural(&reader, "unit value overflow"))?,
        ),
        _ => return Err(structural(&reader, "unknown unit enum value")),
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
            return Err(structural(&reader, "custom unit scale is invalid"))
        }
    };
    if scale.is_some_and(|factor| !factor.is_finite() || factor <= 0.0) {
        return Err(structural(&reader, "unit scale is invalid"));
    }
    let absolute_tolerance_millimeters = scale
        .map(|factor| absolute * factor)
        .filter(|value| value.is_finite() && *value > 0.0);
    if scale.is_some() && absolute_tolerance_millimeters.is_none() {
        return Err(structural(&reader, "scaled absolute tolerance is invalid"));
    }
    finish(&reader, "units")?;
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
        source: SourceRange {
            range: record.range.clone(),
        },
    })
}

/// Parses and consumes one bounded rendering-attributes payload.
pub(crate) fn parse_rendering_attributes(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<Range<usize>, FramingError> {
    let start = reader.position();
    let chunk = crate::chunks::chunk_at(data, start, reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(structural(
            reader,
            "rendering attributes must be an anonymous chunk",
        ));
    }
    if let Some(warning) = checksum_warning(data, &chunk)? {
        warnings.push(warning);
    }
    let mut payload = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    let major = payload.i32()?;
    let _minor = payload.i32()?;
    if major != 1 {
        return Err(structural(
            &payload,
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
    for _ in 0..count {
        let material =
            crate::chunks::chunk_at(data, payload.position(), payload.end(), archive, false)?;
        if material.typecode != ANONYMOUS || material.short {
            return Err(structural(
                &payload,
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
            return Err(structural(
                &material_payload,
                "unsupported rendering material reference version",
            ));
        }
        material_payload.skip(16 + 16)?;
        let obsolete_mapping_count = material_payload.i32()?;
        if obsolete_mapping_count != 0 {
            return Err(structural(
                &material_payload,
                "rendering material mapping array is not empty",
            ));
        }
        if material_minor >= 1 {
            material_payload.skip(16 + 4)?;
        }
        finish(&material_payload, "rendering material reference")?;
        payload.skip(material.next_offset - payload.position())?;
    }
    finish(&payload, "rendering attributes")?;
    reader.skip(chunk.next_offset - reader.position())?;
    Ok(start..reader.position())
}

fn begin_direct_object<'a>(
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
    label: &str,
    warnings: &mut Vec<String>,
) -> Result<(crate::chunks::Chunk, BoundedReader<'a>, (i32, i32)), FramingError> {
    let chunk = crate::chunks::chunk_at(data, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(structural(
            reader,
            format!("{label} must be an object chunk"),
        ));
    }
    if let Some(warning) = checksum_warning(data, &chunk)? {
        warnings.push(warning);
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
) -> Result<(), FramingError> {
    let chunk = crate::chunks::chunk_at(data, payload.position(), payload.end(), archive, false)?;
    if chunk.typecode != MODEL_ATTRIBUTES || chunk.short {
        return Err(structural(
            payload,
            "missing model-component attributes chunk",
        ));
    }
    if let Some(warning) = checksum_warning(data, &chunk)? {
        warnings.push(warning);
    }
    payload.skip(chunk.next_offset - payload.position())
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
            return Err(structural(
                &segment_reader,
                "linetype segment length is not finite",
            ));
        }
        let kind = segment_reader.u32()?;
        if kind > 2 {
            return Err(structural(
                &segment_reader,
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
        begin_direct_object(data, reader, archive, "embedded linetype", warnings)?;
    if (archive.value() < 60 && version != (1, 1))
        || (archive.value() >= 60 && (version.0 != 2 || !(1..=3).contains(&version.1)))
    {
        return Err(structural(
            &payload,
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
        skip_model_attributes(data, &mut payload, archive, warnings)?;
        read_segments(&mut payload)?;
        let mut terminated = false;
        while payload.remaining() > 0 {
            let item = payload.u8()?;
            if item == 0 {
                terminated = true;
                break;
            }
            match item {
                1 | 2 | 4 => payload.skip(1)?,
                3 => {
                    let value = payload.f64()?;
                    if !value.is_finite() {
                        return Err(structural(&payload, "linetype width is not finite"));
                    }
                }
                5 => {
                    let count = payload.i32()?;
                    let bytes = crate::chunks::checked_count_bytes(
                        count,
                        16,
                        payload.remaining(),
                        MAX_ARRAY_ITEMS,
                        payload.position(),
                    )?;
                    payload.skip(bytes)?;
                }
                6 => {
                    let _ = payload.bool()?;
                }
                _ => {
                    return Err(structural(
                        &payload,
                        format!("unknown embedded linetype item {item}"),
                    ))
                }
            }
        }
        if !terminated {
            return Err(structural(
                &payload,
                "embedded linetype is missing terminator",
            ));
        }
    }
    finish(&payload, "embedded linetype")?;
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
        begin_direct_object(data, reader, archive, "embedded section style", warnings)?;
    if version.0 != 1 || !(0..=1).contains(&version.1) {
        return Err(structural(
            &payload,
            "unsupported embedded section-style version",
        ));
    }
    skip_model_attributes(data, &mut payload, archive, warnings)?;
    let mut terminated = false;
    while payload.remaining() > 0 {
        let item = payload.u8()?;
        if item == 0 {
            terminated = true;
            break;
        }
        match item {
            1 | 6 => payload.skip(1)?,
            2 | 4 | 10 => payload.skip(8)?,
            3 => {
                let _ = payload.bool()?;
            }
            5 | 8 | 9 => {
                let value = payload.f64()?;
                if !value.is_finite() {
                    return Err(structural(&payload, "section-style value is not finite"));
                }
            }
            7 => {
                let _ = payload.i32()?;
            }
            11 => {
                parse_direct_linetype(data, &mut payload, archive, warnings)?;
            }
            _ => {
                return Err(structural(
                    &payload,
                    format!("unknown embedded section-style item {item}"),
                ))
            }
        }
    }
    if !terminated {
        return Err(structural(
            &payload,
            "embedded section style is missing terminator",
        ));
    }
    finish(&payload, "embedded section style")?;
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

fn parse_layer(
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
    warnings: &mut Vec<String>,
) -> Result<LayerRecord, FramingError> {
    let class = parse_class_wrapper(data, record.body.clone(), archive, warnings)?;
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
        return Err(structural(&reader, "unsupported layer version"));
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
        reader.bool()?
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
        reader.bool()?
    } else {
        obsolete_mode == 2
    };
    let id = (version.1 >= 5).then(|| uuid(&mut reader)).transpose()?;
    let parent_compatible = writer_version.is_some_and(|version| version > 200_505_110);
    let parent_id = if version.1 >= 6 && parent_compatible {
        Some(uuid(&mut reader)?)
    } else {
        None
    };
    let expanded = if version.1 >= 6 && parent_compatible {
        Some(reader.bool()?)
    } else {
        None
    };
    let rendering_range = if version.1 >= 7 {
        Some(
            parse_rendering_attributes(data, &mut reader, archive, warnings)
                .map_err(|error| structural(&reader, format!("rendering: {error}")))?,
        )
    } else {
        None
    };
    let display_material_id = (version.1 >= 8)
        .then(|| uuid(&mut reader))
        .transpose()
        .map_err(|error| structural(&reader, format!("display material: {error}")))?;
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
        rendering_range,
        extension_items: Vec::new(),
        embedded_linetype: None,
        embedded_section_style: None,
    };
    if version.1 >= 10 {
        let mut terminated = false;
        while reader.remaining() > 0 {
            let item = reader.u8()?;
            if item == 0 {
                terminated = true;
                break;
            }
            let minimum_minor = match item {
                28 => 10,
                29..=31 => 11,
                32 => 12,
                33 => 13,
                34 => 14,
                35..=36 => 15,
                _ => {
                    return Err(structural(
                        &reader,
                        format!("unknown future layer extension item {item}"),
                    ))
                }
            };
            if version.1 < minimum_minor {
                return Err(structural(
                    &reader,
                    format!("layer extension item {item} precedes its version gate"),
                ));
            }
            layer.extension_items.push(item);
            match item {
                28 => {
                    layer.no_clipping_planes = Some(reader.bool()?);
                    let count = reader.i32()?;
                    let bytes = crate::chunks::checked_count_bytes(
                        count,
                        16,
                        reader.remaining(),
                        MAX_ARRAY_ITEMS,
                        reader.position(),
                    )?;
                    reader.skip(bytes)?;
                }
                29 => {
                    reader.skip(4)?;
                }
                30 | 31 => {
                    let value = reader.f64()?;
                    finite(&reader, value, "layer extension value")?;
                }
                32 | 34 | 36 => {
                    reader.skip(1)?;
                }
                33 => {
                    layer.embedded_linetype =
                        Some(parse_direct_linetype(data, &mut reader, archive, warnings)?);
                }
                35 => {
                    layer.embedded_section_style = Some(parse_direct_section_style(
                        data,
                        &mut reader,
                        archive,
                        warnings,
                    )?);
                }
                _ => {
                    return Err(structural(
                        &reader,
                        format!("unsupported layer extension item {item}"),
                    ))
                }
            }
        }
        if !terminated {
            return Err(structural(
                &reader,
                "layer extension stream is missing terminator",
            ));
        }
    }
    finish(&reader, "layer payload")?;
    Ok(layer)
}

/// Decodes all metadata records while preserving scan framing.
pub(crate) fn parse_metadata(
    data: &[u8],
    archive: ArchiveVersion,
    tables: &[Table],
    warnings: &mut Vec<String>,
) -> DocumentMetadata {
    let mut metadata = DocumentMetadata::default();
    let mut indexes = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for table in tables {
        let table_type = table.typecode & !0x0000_8000;
        for record in &table.records {
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
                parse_setting(data, record, &mut metadata.settings)
            } else if table_type == LAYER && record.typecode == LAYER_RECORD {
                let writer_version = metadata.properties.writer_version;
                match parse_layer(data, record, archive, writer_version, warnings) {
                    Ok(layer) => {
                        if !indexes.insert(layer.index) {
                            warnings.push(format!("duplicate layer index {}", layer.index));
                        }
                        if let Some(id) = layer.id {
                            if !ids.insert(id) {
                                warnings.push(format!("duplicate layer UUID {id}"));
                            }
                        }
                        metadata.layers.push(layer);
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            } else {
                Ok(())
            };
            if let Err(error) = result {
                warnings.push(format!(
                    "metadata record {:#x} at {} degraded: {}",
                    record.typecode, record.range.start, error
                ));
            }
        }
    }
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

fn utf16_record(data: &[u8], record: &Record) -> Result<String, FramingError> {
    let mut reader = BoundedReader::new(data, record.body.start, record.body.end)?;
    let value = utf16(&mut reader)?;
    finish(&reader, "UTF-16 property")?;
    Ok(value)
}

pub(crate) fn parse_setting(
    data: &[u8],
    record: &Record,
    settings: &mut DocumentSettings,
) -> Result<(), FramingError> {
    match record.typecode {
        UNITS => parse_units(data, record).map(|value| settings.units = Some(value)),
        CURRENT_LAYER => {
            settings.current_layer = Some(short_index(record, "current layer")?);
            Ok(())
        }
        CURRENT_MATERIAL => {
            if record.short || record.body.len() != 8 {
                return Err(FramingError::Structural {
                    offset: record.range.start,
                    message: "current material must be a long eight-byte index/source pair"
                        .to_string(),
                });
            }
            let material_index = i32::from_le_bytes(
                data[record.body.start..record.body.start + 4]
                    .try_into()
                    .expect("length checked"),
            );
            if material_index < -1 {
                return Err(FramingError::Structural {
                    offset: record.range.start,
                    message: "current material index is invalid".to_string(),
                });
            }
            settings.current_material = Some(material_index);
            settings.current_material_source = Some(i32::from_le_bytes(
                data[record.body.start + 4..record.body.end]
                    .try_into()
                    .expect("length checked"),
            ));
            Ok(())
        }
        CURRENT_COLOR => {
            if record.short || record.body.len() != 8 {
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
            settings.current_color_source = Some(i32::from_le_bytes(
                data[record.body.start + 4..record.body.end]
                    .try_into()
                    .expect("length checked"),
            ));
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
