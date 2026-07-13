// SPDX-License-Identifier: Apache-2.0
//! Rhino document properties, selectors, previews, and setting identities.

use cadmpeg_ir::document::CadIr;
use serde::Serialize;

use crate::chunks::{BoundedReader, FramingError};
use crate::container::Scan;
use crate::settings::utf16;
use crate::wire::{scaled_coordinate, Uuid};

const SETTINGS_TABLE: u32 = 0x1000_0015;
const ANNOTATION_SETTINGS: u32 = 0x2000_8034;
const GRID_DEFAULTS: u32 = 0x2000_803f;

#[derive(Debug, Serialize)]
struct RevisionRecord {
    id: String,
    source_offset: u64,
    created_by: String,
    created_utc_fields: [i32; 8],
    last_edited_by: String,
    last_edited_utc_fields: [i32; 8],
    revision_count: i32,
}

#[derive(Debug, Serialize)]
struct NotesRecord {
    id: String,
    source_offset: u64,
    html: bool,
    text: String,
    visible: bool,
    window_rectangle: [i32; 4],
    locked: bool,
}

#[derive(Debug, Serialize)]
struct ApplicationRecord {
    id: String,
    source_offset: u64,
    name: String,
    url: String,
    details: String,
}

#[derive(Debug, Serialize)]
struct DocumentSettingsRecord {
    id: String,
    writer_version: Option<i64>,
    archive_file_name: Option<String>,
    model_url: Option<String>,
    current_layer_index: Option<i64>,
    current_material_index: Option<i32>,
    current_material_source: Option<i32>,
    current_color: Option<[u8; 4]>,
    current_color_source: Option<i32>,
    current_wire_density: Option<i64>,
    current_font_index: Option<i64>,
    current_dimension_style_index: Option<i64>,
}

#[derive(Debug, Serialize)]
struct PreviewRecord {
    id: String,
    source_offset: u64,
    byte_len: u64,
    compressed: bool,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct SettingRecord {
    id: String,
    source_offset: u64,
    byte_len: u64,
    typecode: String,
    sha256: String,
}

#[derive(Debug, Serialize)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent serialized annotation scaling switches"
)]
struct AnnotationSettingsRecord {
    id: String,
    source_offset: u64,
    dimension_scale: f64,
    text_height_mm: f64,
    extension_line_extension_mm: f64,
    extension_line_offset_mm: f64,
    arrow_length_mm: f64,
    arrow_width_mm: f64,
    center_mark_mm: f64,
    dimension_units: u32,
    arrow_type: i32,
    angular_units: i32,
    length_format: i32,
    angle_format: i32,
    obsolete_text_alignment: u32,
    resolution: i32,
    font_face: String,
    world_view_text_scale: Option<f64>,
    annotation_scaling: Option<bool>,
    world_view_hatch_scale: Option<f64>,
    hatch_scaling: Option<bool>,
    model_space_annotation_scaling: Option<bool>,
    layout_space_annotation_scaling: Option<bool>,
    use_dimension_layer: Option<bool>,
    dimension_layer_uuid: Option<String>,
}

#[derive(Debug, Serialize)]
struct GridDefaultsRecord {
    id: String,
    source_offset: u64,
    grid_spacing_mm: f64,
    snap_spacing_mm: f64,
    grid_line_count: i32,
    thick_line_frequency: i32,
    show_grid: bool,
    show_grid_axes: bool,
    show_world_axes: bool,
}

fn structural(reader: &BoundedReader<'_>, message: &str) -> FramingError {
    FramingError::Structural {
        offset: reader.position(),
        message: message.to_string(),
    }
}

fn length(reader: &mut BoundedReader<'_>, scale: f64) -> Result<f64, FramingError> {
    scaled_coordinate(reader.f64()?, scale)
        .ok_or_else(|| structural(reader, "scaled setting length is invalid"))
}

fn flag_i32(reader: &mut BoundedReader<'_>) -> Result<bool, FramingError> {
    match reader.i32()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(structural(reader, "setting flag is invalid")),
    }
}

fn annotation_settings(
    data: &[u8],
    body: std::ops::Range<usize>,
    source_offset: usize,
    scale: f64,
) -> Result<AnnotationSettingsRecord, FramingError> {
    let mut reader = BoundedReader::new(data, body.start, body.end)?;
    let packed = reader.u8()?;
    let minor = packed & 0x0f;
    if packed >> 4 != 1 || minor > 4 {
        return Err(structural(
            &reader,
            "annotation-settings version is unsupported",
        ));
    }
    let dimension_scale = reader.f64()?;
    let value = AnnotationSettingsRecord {
        id: "rhino:document:annotation_settings#current".to_string(),
        source_offset: source_offset as u64,
        dimension_scale,
        text_height_mm: length(&mut reader, scale)?,
        extension_line_extension_mm: length(&mut reader, scale)?,
        extension_line_offset_mm: length(&mut reader, scale)?,
        arrow_length_mm: length(&mut reader, scale)?,
        arrow_width_mm: length(&mut reader, scale)?,
        center_mark_mm: length(&mut reader, scale)?,
        dimension_units: reader.u32()?,
        arrow_type: reader.i32()?,
        angular_units: reader.i32()?,
        length_format: reader.i32()?,
        angle_format: reader.i32()?,
        obsolete_text_alignment: reader.u32()?,
        resolution: reader.i32()?,
        font_face: utf16(&mut reader)?,
        world_view_text_scale: (minor >= 1).then(|| reader.f64()).transpose()?,
        annotation_scaling: (minor >= 1).then(|| reader.bool()).transpose()?,
        world_view_hatch_scale: (minor >= 2).then(|| reader.f64()).transpose()?,
        hatch_scaling: (minor >= 2).then(|| reader.bool()).transpose()?,
        model_space_annotation_scaling: (minor >= 3).then(|| reader.bool()).transpose()?,
        layout_space_annotation_scaling: (minor >= 3).then(|| reader.bool()).transpose()?,
        use_dimension_layer: (minor >= 4).then(|| reader.bool()).transpose()?,
        dimension_layer_uuid: if minor >= 4 {
            let id = Uuid::from_wire(reader.array()?);
            (!id.is_nil()).then(|| id.to_string())
        } else {
            None
        },
    };
    if reader.remaining() != 0 {
        return Err(structural(
            &reader,
            "annotation settings have trailing bytes",
        ));
    }
    Ok(value)
}

fn grid_defaults(
    data: &[u8],
    body: std::ops::Range<usize>,
    source_offset: usize,
    scale: f64,
) -> Result<GridDefaultsRecord, FramingError> {
    let mut reader = BoundedReader::new(data, body.start, body.end)?;
    if reader.u8()? != 0x10 {
        return Err(structural(&reader, "grid-default version is unsupported"));
    }
    let value = GridDefaultsRecord {
        id: "rhino:document:grid_defaults#current".to_string(),
        source_offset: source_offset as u64,
        grid_spacing_mm: length(&mut reader, scale)?,
        snap_spacing_mm: length(&mut reader, scale)?,
        grid_line_count: reader.i32()?,
        thick_line_frequency: reader.i32()?,
        show_grid: flag_i32(&mut reader)?,
        show_grid_axes: flag_i32(&mut reader)?,
        show_world_axes: flag_i32(&mut reader)?,
    };
    if reader.remaining() != 0 {
        return Err(structural(&reader, "grid defaults have trailing bytes"));
    }
    Ok(value)
}

/// Installs complete typed document-level metadata and named setting records.
pub(crate) fn install(scan: &Scan, ir: &mut CadIr) {
    let properties = &scan.metadata.properties;
    let revisions = properties
        .revision_history
        .iter()
        .map(|value| RevisionRecord {
            id: "rhino:document:revision#current".to_string(),
            source_offset: value.source.range.start as u64,
            created_by: value.created_by.clone(),
            created_utc_fields: value.created.fields,
            last_edited_by: value.last_edited_by.clone(),
            last_edited_utc_fields: value.last_edited.fields,
            revision_count: value.revision_count,
        })
        .collect::<Vec<_>>();
    let notes = properties
        .notes
        .iter()
        .map(|value| NotesRecord {
            id: "rhino:document:notes#current".to_string(),
            source_offset: value.source.range.start as u64,
            html: value.html != 0,
            text: value.text.clone(),
            visible: value.visible != 0,
            window_rectangle: value.rectangle,
            locked: value.locked,
        })
        .collect::<Vec<_>>();
    let applications = properties
        .application
        .iter()
        .map(|value| ApplicationRecord {
            id: "rhino:document:application#writer".to_string(),
            source_offset: value.source.range.start as u64,
            name: value.name.clone(),
            url: value.url.clone(),
            details: value.details.clone(),
        })
        .collect::<Vec<_>>();
    let settings = &scan.metadata.settings;
    let document_settings = [DocumentSettingsRecord {
        id: "rhino:document:settings#current".to_string(),
        writer_version: properties.writer_version,
        archive_file_name: properties.as_file_name.clone(),
        model_url: settings.model_url.clone(),
        current_layer_index: settings.current_layer,
        current_material_index: settings.current_material,
        current_material_source: settings.current_material_source,
        current_color: settings.current_color,
        current_color_source: settings.current_color_source,
        current_wire_density: settings.current_wire_density,
        current_font_index: settings.current_font,
        current_dimension_style_index: settings.current_dimstyle,
    }];
    let previews = properties
        .previews
        .iter()
        .enumerate()
        .map(|(index, value)| PreviewRecord {
            id: format!("rhino:document:preview#{index:04}"),
            source_offset: value.source.range.start as u64,
            byte_len: value.source.range.len() as u64,
            compressed: value.compressed,
            sha256: cadmpeg_ir::hash::sha256_hex(&scan.data[value.source.range.clone()]),
        })
        .collect::<Vec<_>>();
    let setting_records = settings
        .unsupported
        .iter()
        .enumerate()
        .map(|(index, value)| SettingRecord {
            id: format!("rhino:document:setting#{index:04}"),
            source_offset: value.source.range.start as u64,
            byte_len: value.source.range.len() as u64,
            typecode: format!("{:#010x}", value.typecode),
            sha256: cadmpeg_ir::hash::sha256_hex(&scan.data[value.source.range.clone()]),
        })
        .collect::<Vec<_>>();
    let scale = settings
        .units
        .as_ref()
        .and_then(|value| value.millimeters_per_unit)
        .unwrap_or(1.0);
    let mut annotations = Vec::new();
    let mut grids = Vec::new();
    for table in &scan.tables {
        if table.typecode & !0x0000_8000 != SETTINGS_TABLE {
            continue;
        }
        for record in &table.records {
            if record.typecode == ANNOTATION_SETTINGS {
                if let Ok(value) =
                    annotation_settings(&scan.data, record.body.clone(), record.range.start, scale)
                {
                    annotations.push(value);
                }
            } else if record.typecode == GRID_DEFAULTS {
                if let Ok(value) =
                    grid_defaults(&scan.data, record.body.clone(), record.range.start, scale)
                {
                    grids.push(value);
                }
            }
        }
    }
    let namespace = ir.native.namespace_mut("rhino");
    namespace.version = namespace.version.max(2);
    namespace
        .set_arena("revisions", &revisions)
        .expect("Rhino revisions serialize");
    namespace
        .set_arena("document_notes", &notes)
        .expect("Rhino notes serialize");
    namespace
        .set_arena("applications", &applications)
        .expect("Rhino applications serialize");
    namespace
        .set_arena("document_settings", &document_settings)
        .expect("Rhino settings serialize");
    namespace
        .set_arena("previews", &previews)
        .expect("Rhino previews serialize");
    namespace
        .set_arena("setting_records", &setting_records)
        .expect("Rhino setting records serialize");
    namespace
        .set_arena("annotation_settings", &annotations)
        .expect("Rhino annotation settings serialize");
    namespace
        .set_arena("grid_defaults", &grids)
        .expect("Rhino grid defaults serialize");
}
