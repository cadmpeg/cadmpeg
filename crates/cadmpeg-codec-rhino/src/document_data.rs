// SPDX-License-Identifier: Apache-2.0
//! Rhino document properties, selectors, previews, and setting identities.

use crate::loss::{Diagnostics, ScratchVec};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::scalar::FiniteReal;
use serde::Serialize;
use std::ops::Range;

use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::container::{NativeInstall, OpaqueRecord, Record, Scan};
use crate::objects::{parse_userdata, UserdataDescriptor};
use crate::settings::{utf16_retained, MillimeterScale, UnitBinding};
use crate::wire::{flag_i32, scaled_coordinate, Uuid};

const SETTINGS_TABLE: u32 = 0x1000_0015;
const ANNOTATION_SETTINGS: u32 = 0x2000_8034;
const GRID_DEFAULTS: u32 = 0x2000_803f;
const RENDER_SETTINGS: u32 = 0x2000_803d;
const RENDER_USERDATA: u32 = 0x2000_8136;
const ANONYMOUS: u32 = 0x4000_8000;
const CLASS_USERDATA: u32 = 0x0002_7ffd;
const CLASS_END: u32 = 0x8002_7fff;

#[derive(Debug, PartialEq, Eq)]
struct RenderUserdataDescriptor {
    source: Range<usize>,
    items: Vec<UserdataDescriptor>,
    unknown_chunks: Vec<Range<usize>>,
    suffix: Range<usize>,
}

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
    parse_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct AnnotationSettingsRecord {
    id: String,
    source_offset: u64,
    dimension_scale: FiniteReal,
    text_height_mm: FiniteReal,
    extension_line_extension_mm: FiniteReal,
    extension_line_offset_mm: FiniteReal,
    arrow_length_mm: FiniteReal,
    arrow_width_mm: FiniteReal,
    center_mark_mm: FiniteReal,
    dimension_units: u32,
    arrow_type: i32,
    angular_units: i32,
    length_format: i32,
    angle_format: i32,
    obsolete_text_alignment: u32,
    resolution: i32,
    font_face: String,
    world_view_text_scale: Option<FiniteReal>,
    annotation_scaling: Option<bool>,
    world_view_hatch_scale: Option<FiniteReal>,
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
    grid_spacing_mm: FiniteReal,
    snap_spacing_mm: FiniteReal,
    grid_line_count: i32,
    thick_line_frequency: i32,
    show_grid: bool,
    show_grid_axes: bool,
    show_world_axes: bool,
}

#[derive(Debug, Serialize)]
struct RenderSettingsRecord {
    id: String,
    source_offset: u64,
    #[serde(flatten)]
    image_flags: RenderImageFlags,
    image_width_pixels: i32,
    image_height_pixels: i32,
    image_dpi: Option<f64>,
    image_unit_system: Option<u32>,
    ambient_light: [u8; 4],
    background_style: i32,
    background_color: [u8; 4],
    background_bottom_color: Option<[u8; 4]>,
    background_bitmap_path: String,
    #[serde(flatten)]
    lighting_flags: RenderLightingFlags,
    #[serde(flatten)]
    surface_flags: RenderSurfaceFlags,
    #[serde(flatten)]
    detail_flags: RenderDetailFlags,
    antialias_style: i32,
    shadowmap_style: i32,
    shadowmap_size_pixels: [i32; 2],
    shadowmap_offset_mm: FiniteReal,
    obsolete_focal_blur: Option<[f64; 5]>,
    rendering_source: Option<i32>,
    specific_viewport: String,
    named_view: String,
    snapshot: String,
    force_viewport_aspect_ratio: Option<bool>,
}

#[derive(Debug, Serialize)]
struct RenderImageFlags {
    custom_image_size: bool,
    scale_background_to_fit: bool,
    transparent_background: bool,
}

#[derive(Debug, Serialize)]
struct RenderLightingFlags {
    use_hidden_lights: bool,
    depth_cue: bool,
    flat_shade: bool,
}

#[derive(Debug, Serialize)]
struct RenderSurfaceFlags {
    render_backfaces: bool,
    render_points: bool,
    render_curves: bool,
}

#[derive(Debug, Serialize)]
struct RenderDetailFlags {
    render_isoparams: bool,
    render_mesh_edges: bool,
    render_annotations: bool,
}

fn length(
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
) -> Result<FiniteReal, FramingError> {
    scaled_coordinate(reader.f64()?, scale).ok_or_else(|| {
        FramingError::structural(reader.position(), "scaled setting length is invalid")
    })
}

fn annotation_scale(reader: &mut BoundedReader<'_>) -> Result<FiniteReal, FramingError> {
    let value = reader.f64()?;
    FiniteReal::new(value).ok_or_else(|| {
        FramingError::structural(reader.position(), "annotation scale is non-finite")
    })
}

fn annotation_settings(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    body: std::ops::Range<usize>,
    source_offset: usize,
    scale: MillimeterScale,
) -> Result<AnnotationSettingsRecord, FramingError> {
    let mut reader = BoundedReader::new(data, body.start, body.end)?;
    let packed = reader.u8()?;
    let minor = packed & 0x0f;
    if packed >> 4 != 1 {
        return Err(FramingError::structural(
            reader.position(),
            "annotation-settings version is unsupported",
        ));
    }
    let dimension_scale = annotation_scale(&mut reader)?;
    let value = AnnotationSettingsRecord {
        id: ctx.copy_retained_text(
            "rhino:document:annotation_settings#current",
            "Rhino annotation settings ID",
        )?,
        source_offset: cadmpeg_core::decode::u64_from_index(source_offset),
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
        font_face: utf16_retained(ctx, &mut reader, "Rhino annotation font face")?,
        world_view_text_scale: (minor >= 1)
            .then(|| annotation_scale(&mut reader))
            .transpose()?,
        annotation_scaling: (minor >= 1).then(|| reader.bool()).transpose()?,
        world_view_hatch_scale: (minor >= 2)
            .then(|| annotation_scale(&mut reader))
            .transpose()?,
        hatch_scaling: (minor >= 2).then(|| reader.bool()).transpose()?,
        model_space_annotation_scaling: (minor >= 3).then(|| reader.bool()).transpose()?,
        layout_space_annotation_scaling: (minor >= 3).then(|| reader.bool()).transpose()?,
        use_dimension_layer: (minor >= 4).then(|| reader.bool()).transpose()?,
        dimension_layer_uuid: if minor >= 4 {
            let id = Uuid::from_wire(reader.array()?);
            if id.is_nil() {
                None
            } else {
                Some(ctx.format_retained(
                    format_args!("{id}"),
                    "Rhino annotation dimension layer UUID",
                )?)
            }
        } else {
            None
        },
    };
    reader.skip_remaining()?;
    Ok(value)
}

fn grid_defaults(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    body: std::ops::Range<usize>,
    source_offset: usize,
    scale: MillimeterScale,
) -> Result<GridDefaultsRecord, FramingError> {
    let mut reader = BoundedReader::new(data, body.start, body.end)?;
    if reader.u8()? >> 4 != 1 {
        return Err(FramingError::structural(
            reader.position(),
            "grid-default version is unsupported",
        ));
    }
    let value = GridDefaultsRecord {
        id: ctx.copy_retained_text(
            "rhino:document:grid_defaults#current",
            "Rhino grid defaults ID",
        )?,
        source_offset: cadmpeg_core::decode::u64_from_index(source_offset),
        grid_spacing_mm: length(&mut reader, scale)?,
        snap_spacing_mm: length(&mut reader, scale)?,
        grid_line_count: reader.i32()?,
        thick_line_frequency: reader.i32()?,
        show_grid: flag_i32(&mut reader)?,
        show_grid_axes: flag_i32(&mut reader)?,
        show_world_axes: flag_i32(&mut reader)?,
    };
    reader.skip_remaining()?;
    Ok(value)
}

fn render_settings(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    body: std::ops::Range<usize>,
    source_offset: usize,
    archive: ArchiveVersion,
    scale: MillimeterScale,
) -> Result<RenderSettingsRecord, FramingError> {
    let modern = data.get(body.start).copied() == Some(0);
    let (mut reader, minor, legacy_version) = if modern {
        let chunk = chunk_at(data, body.start, body.end, archive, false)?;
        if chunk.typecode != ANONYMOUS || chunk.short() {
            return Err(FramingError::Structural {
                offset: body.start,
                message: "render-settings wrapper is invalid".to_string(),
            });
        }
        let mut reader = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
        let (major, minor) = (reader.i32()?, reader.i32()?);
        if major != 1 || minor < 0 {
            return Err(FramingError::structural(
                reader.position(),
                "render-settings version is unsupported",
            ));
        }
        (reader, Some(minor), None)
    } else {
        let mut reader = BoundedReader::new(data, body.start, body.end)?;
        let version = reader.i32()?;
        if !(100..200).contains(&version) {
            return Err(FramingError::structural(
                reader.position(),
                "legacy render-settings version is unsupported",
            ));
        }
        (reader, None, Some(version))
    };
    let custom_image_size = if modern {
        reader.bool()?
    } else {
        flag_i32(&mut reader)?
    };
    let image_width_pixels = reader.i32()?;
    let image_height_pixels = reader.i32()?;
    let (image_dpi, image_unit_system) = if modern {
        (Some(reader.f64()?), Some(reader.u32()?))
    } else {
        (None, None)
    };
    let ambient_light = reader.array()?;
    let background_style = reader.i32()?;
    let background_color = reader.array()?;
    let background_bottom_color = modern.then(|| reader.array()).transpose()?;
    let background_bitmap_path =
        utf16_retained(ctx, &mut reader, "Rhino render background bitmap path")?;
    let read_flag = |reader: &mut BoundedReader<'_>| {
        if modern {
            reader.bool()
        } else {
            flag_i32(reader)
        }
    };
    let use_hidden_lights = read_flag(&mut reader)?;
    let depth_cue = read_flag(&mut reader)?;
    let flat_shade = read_flag(&mut reader)?;
    let render_backfaces = read_flag(&mut reader)?;
    let render_points = read_flag(&mut reader)?;
    let render_curves = read_flag(&mut reader)?;
    let render_isoparams = read_flag(&mut reader)?;
    let render_mesh_edges = read_flag(&mut reader)?;
    let render_annotations = read_flag(&mut reader)?;
    let (scale_background_to_fit, transparent_background) = if modern {
        (reader.bool()?, reader.bool()?)
    } else {
        (false, false)
    };
    let antialias_style = reader.i32()?;
    let shadowmap_style = reader.i32()?;
    let shadowmap_size_pixels = [reader.i32()?, reader.i32()?];
    let shadowmap_offset_mm = length(&mut reader, scale)?;
    let obsolete_focal_blur = if minor.is_some_and(|minor| minor >= 1) {
        Some([
            f64::from(reader.i32()?),
            reader.f64()?,
            reader.f64()?,
            reader.f64()?,
            f64::from(reader.i32()?),
        ])
    } else {
        None
    };
    let (rendering_source, specific_viewport, named_view, snapshot) =
        if minor.is_some_and(|minor| minor >= 2) {
            (
                Some(reader.i32()?),
                utf16_retained(ctx, &mut reader, "Rhino render specific viewport")?,
                utf16_retained(ctx, &mut reader, "Rhino render named view")?,
                utf16_retained(ctx, &mut reader, "Rhino render snapshot")?,
            )
        } else {
            (None, String::new(), String::new(), String::new())
        };
    let force_viewport_aspect_ratio = if minor.is_some_and(|minor| minor >= 3) {
        Some(reader.bool()?)
    } else {
        None
    };
    let (image_dpi, image_unit_system, background_bottom_color, scale_background_to_fit) = if modern
    {
        (
            image_dpi,
            image_unit_system,
            background_bottom_color,
            scale_background_to_fit,
        )
    } else {
        let version = legacy_version.ok_or_else(|| {
            FramingError::structural(
                reader.position(),
                "legacy render-settings branch has no parsed version",
            )
        })?;
        let dpi = (version >= 101).then(|| reader.f64()).transpose()?;
        let units = (version >= 101).then(|| reader.u32()).transpose()?;
        let bottom = (version >= 102).then(|| reader.array()).transpose()?;
        let fit = if version >= 103 {
            reader.bool()?
        } else {
            false
        };
        (dpi, units, bottom, fit)
    };
    reader.skip_remaining()?;
    Ok(RenderSettingsRecord {
        id: ctx.copy_retained_text(
            "rhino:document:render_settings#current",
            "Rhino render settings ID",
        )?,
        source_offset: cadmpeg_core::decode::u64_from_index(source_offset),
        image_flags: RenderImageFlags {
            custom_image_size,
            scale_background_to_fit,
            transparent_background,
        },
        image_width_pixels,
        image_height_pixels,
        image_dpi,
        image_unit_system,
        ambient_light,
        background_style,
        background_color,
        background_bottom_color,
        background_bitmap_path,
        lighting_flags: RenderLightingFlags {
            use_hidden_lights,
            depth_cue,
            flat_shade,
        },
        surface_flags: RenderSurfaceFlags {
            render_backfaces,
            render_points,
            render_curves,
        },
        detail_flags: RenderDetailFlags {
            render_isoparams,
            render_mesh_edges,
            render_annotations,
        },
        antialias_style,
        shadowmap_style,
        shadowmap_size_pixels,
        shadowmap_offset_mm,
        obsolete_focal_blur,
        rendering_source,
        specific_viewport,
        named_view,
        snapshot,
        force_viewport_aspect_ratio,
    })
}

fn render_userdata(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
) -> Result<RenderUserdataDescriptor, FramingError> {
    let mut offset = record.body().start;
    let mut items = Vec::new();
    let mut unknown_chunks = Vec::new();
    while offset < record.body().end {
        ctx.charge_work(1, "Rhino render userdata framing walk")?;
        let chunk = chunk_at(data, offset, record.body().end, archive, false)?;
        match chunk.typecode {
            CLASS_USERDATA => {
                if chunk.short() {
                    return Err(FramingError::structural(
                        chunk.header_start,
                        "render userdata item must be a long chunk",
                    ));
                }
                ctx.reserve_vec(&mut items, 1, "Rhino render userdata items")
                    .map_err(crate::chunks::FramingError::from)?;
                let mut checksum_warnings = Diagnostics::new();
                let item = parse_userdata(ctx, data, &chunk, archive, &mut checksum_warnings)?;
                items.push(item);
                offset = chunk.next_offset();
            }
            CLASS_END => {
                if !chunk.short() || chunk.value()? != 0 {
                    return Err(FramingError::structural(
                        chunk.header_start,
                        "render userdata class end must be a short zero chunk",
                    ));
                }
                return Ok(RenderUserdataDescriptor {
                    source: record.range.clone(),
                    items,
                    unknown_chunks,
                    suffix: chunk.next_offset()..record.body().end,
                });
            }
            0 => {
                return Err(FramingError::structural(
                    chunk.header_start,
                    "render userdata contains a zero typecode",
                ));
            }
            _ => {
                ctx.reserve_vec(
                    &mut unknown_chunks,
                    1,
                    "Rhino render userdata unknown chunks",
                )
                .map_err(crate::chunks::FramingError::from)?;
                unknown_chunks.push(chunk.range());
                offset = chunk.next_offset();
            }
        }
    }
    Err(FramingError::structural(
        record.body().end,
        "render userdata is missing its class end",
    ))
}

fn retained_numbered_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    prefix: &str,
    index: usize,
    operation: &'static str,
) -> Result<String, CodecError> {
    ctx.format_retained(format_args!("{prefix}{index:04}"), operation)
}

fn retained_typecode(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    typecode: u32,
) -> Result<String, CodecError> {
    ctx.format_retained(format_args!("{typecode:#010x}"), "Rhino setting typecode")
}

fn retained_sha256(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    operation: &'static str,
) -> Result<String, CodecError> {
    ctx.charge_work(cadmpeg_core::decode::u64_from_index(bytes.len()), operation)?;
    let digest = cadmpeg_ir::hash::sha256(bytes);
    ctx.format_retained(
        format_args!("{}", cadmpeg_ir::hash::LowerHex(&digest)),
        operation,
    )
}

/// Installs complete typed document-level metadata and named setting records.
///
/// The returned records are complete settings records whose payload was not
/// admitted by a registered owner.
pub(crate) fn install<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    scan: &Scan<'_>,
    ir: &mut CadIr,
) -> Result<NativeInstall<'ctx>, CodecError> {
    let properties = &scan.metadata.properties;
    let (native_records, mut native_storage) =
        ctx.with_scoped_storage("Rhino document native workspace", || {
            let mut revisions = ctx.collection_vec(
                usize::from(properties.revision_history.is_some()),
                "Rhino document revisions",
            )?;
            if let Some(value) = &properties.revision_history {
                revisions.push(RevisionRecord {
                    id: ctx.copy_retained_text(
                        "rhino:document:revision#current",
                        "Rhino revision ID",
                    )?,
                    source_offset: cadmpeg_core::decode::u64_from_index(value.source.range.start),
                    created_by: ctx
                        .copy_retained_text(&value.created_by, "Rhino revision creator")?,
                    created_utc_fields: value.created.fields,
                    last_edited_by: ctx
                        .copy_retained_text(&value.last_edited_by, "Rhino revision editor")?,
                    last_edited_utc_fields: value.last_edited.fields,
                    revision_count: value.revision_count,
                });
            }
            let mut notes = ctx.collection_vec(
                usize::from(properties.notes.is_some()),
                "Rhino document notes",
            )?;
            if let Some(value) = &properties.notes {
                notes.push(NotesRecord {
                    id: ctx.copy_retained_text("rhino:document:notes#current", "Rhino notes ID")?,
                    source_offset: cadmpeg_core::decode::u64_from_index(value.source.range.start),
                    html: value.html,
                    text: ctx.copy_retained_text(&value.text, "Rhino notes text")?,
                    visible: value.visible,
                    window_rectangle: value.rectangle,
                    locked: value.locked,
                });
            }
            let mut applications = ctx.collection_vec(
                usize::from(properties.application.is_some()),
                "Rhino document applications",
            )?;
            if let Some(value) = &properties.application {
                applications.push(ApplicationRecord {
                    id: ctx.copy_retained_text(
                        "rhino:document:application#writer",
                        "Rhino application ID",
                    )?,
                    source_offset: cadmpeg_core::decode::u64_from_index(value.source.range.start),
                    name: ctx.copy_retained_text(&value.name, "Rhino application name")?,
                    url: ctx.copy_retained_text(&value.url, "Rhino application URL")?,
                    details: ctx.copy_retained_text(&value.details, "Rhino application details")?,
                });
            }
            let settings = &scan.metadata.settings;
            let document_settings = [DocumentSettingsRecord {
                id: ctx.copy_retained_text(
                    "rhino:document:settings#current",
                    "Rhino document settings ID",
                )?,
                writer_version: properties.writer_version,
                archive_file_name: properties
                    .as_file_name
                    .as_deref()
                    .map(|value| ctx.copy_retained_text(value, "Rhino archive file name"))
                    .transpose()?,
                model_url: settings
                    .model_url
                    .as_deref()
                    .map(|value| ctx.copy_retained_text(value, "Rhino model URL"))
                    .transpose()?,
                current_layer_index: settings.current_layer,
                current_material_index: settings.current_material.map(|selection| selection.value),
                current_material_source: settings
                    .current_material
                    .map(|selection| selection.source),
                current_color: settings.current_color.map(|selection| selection.value),
                current_color_source: settings.current_color.map(|selection| selection.source),
                current_wire_density: settings.current_wire_density,
                current_font_index: settings.current_font,
                current_dimension_style_index: settings.current_dimstyle,
            }];
            let mut previews =
                ctx.collection_vec(properties.previews.len(), "Rhino document previews")?;
            for (index, value) in ctx
                .admit_iter(&properties.previews, "Rhino install traversal")?
                .enumerate()
            {
                previews.push(PreviewRecord {
                    id: retained_numbered_id(
                        ctx,
                        "rhino:document:preview#",
                        index,
                        "Rhino preview ID",
                    )?,
                    source_offset: cadmpeg_core::decode::u64_from_index(value.source.range.start),
                    byte_len: cadmpeg_core::decode::u64_from_index(value.source.range.len()),
                    compressed: value.compressed,
                    sha256: retained_sha256(
                        ctx,
                        &scan.data[value.source.range.clone()],
                        "Rhino preview SHA-256",
                    )?,
                });
            }
            let mut setting_records = ctx.collection_vec(
                settings.unsupported.len(),
                "Rhino unsupported setting records",
            )?;
            for (index, value) in ctx
                .admit_iter(&settings.unsupported, "Rhino install traversal")?
                .enumerate()
            {
                setting_records.push(SettingRecord {
                    id: retained_numbered_id(
                        ctx,
                        "rhino:document:setting#",
                        index,
                        "Rhino setting ID",
                    )?,
                    source_offset: cadmpeg_core::decode::u64_from_index(value.source.range.start),
                    byte_len: cadmpeg_core::decode::u64_from_index(value.source.range.len()),
                    typecode: retained_typecode(ctx, value.typecode)?,
                    sha256: retained_sha256(
                        ctx,
                        &scan.data[value.source.range.clone()],
                        "Rhino setting SHA-256",
                    )?,
                    parse_error: None,
                });
            }
            Ok::<_, CodecError>((
                revisions,
                notes,
                applications,
                document_settings,
                previews,
                setting_records,
            ))
        })?;
    let (revisions, notes, applications, document_settings, previews, mut setting_records) =
        native_records;
    let settings = &scan.metadata.settings;
    let binding = UnitBinding::from_units(settings.units.as_ref());
    let mut annotations = Vec::new();
    let mut grids = Vec::new();
    let mut renders = Vec::new();
    let mut losses = ScratchVec::new(ctx, "Rhino document setting loss Vec")?;
    let mut opaque_records = ScratchVec::new(ctx, "Rhino document source Vec")?;
    let mut render_settings_seen = false;
    for table in ctx.admit_iter(&scan.tables, "Rhino install traversal")? {
        if table.typecode & !0x0000_8000 != SETTINGS_TABLE {
            continue;
        }
        for record in ctx.admit_iter(&table.records, "Rhino install traversal")? {
            if matches!(
                record.typecode,
                ANNOTATION_SETTINGS | GRID_DEFAULTS | RENDER_SETTINGS
            ) && binding.neutral_scale().is_none()
            {
                let message = native_storage.with_storage(|| ctx.format_retained(format_args!(
                    "setting record {:#010x} at offset {} was retained as complete source because the document has no physical millimetre binding ({})",
                    record.typecode,
                    record.range.start,
                    binding.label()
                ), "Rhino unit-binding setting message"))?;
                losses.push_admitted(
                    ctx,
                    crate::loss::RhinoLossCode::PresentationRecordDropped
                        .note(ctx.copy_retained_text(&message, "Rhino unit-binding loss message")?),
                    "Rhino document setting losses",
                )?;
                opaque_records.push_admitted(
                    ctx,
                    OpaqueRecord {
                        table_typecode: table.typecode,
                        record: record.clone(),
                    },
                    "Rhino opaque setting records",
                )?;
                native_storage.with_storage(|| {
                    ctx.reserve_vec(&mut setting_records, 1, "Rhino retained setting records")?;
                    setting_records.push(SettingRecord {
                        id: retained_numbered_id(
                            ctx,
                            "rhino:document:setting#unit-binding-",
                            setting_records.len(),
                            "Rhino retained setting ID",
                        )?,
                        source_offset: cadmpeg_core::decode::u64_from_index(record.range.start),
                        byte_len: cadmpeg_core::decode::u64_from_index(record.range.len()),
                        typecode: retained_typecode(ctx, record.typecode)?,
                        sha256: retained_sha256(
                            ctx,
                            &scan.data[record.range.clone()],
                            "Rhino setting SHA-256",
                        )?,
                        parse_error: Some(message),
                    });
                    Ok::<_, CodecError>(())
                })?;
                continue;
            }
            let mut candidate_storage = ctx.reserve_scoped(0, "Rhino setting candidate")?;
            let result = if record.typecode == ANNOTATION_SETTINGS {
                let Some(scale) = binding.neutral_scale() else {
                    continue;
                };
                match candidate_storage.with_storage(|| {
                    annotation_settings(ctx, scan.data, record.body(), record.range.start, scale)
                }) {
                    Ok(value) => {
                        ctx.reserve_scoped_vec(
                            &mut native_storage,
                            &mut annotations,
                            1,
                            "Rhino annotation settings",
                        )?;
                        native_storage.absorb(&mut candidate_storage)?;
                        annotations.push(value);
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            } else if record.typecode == GRID_DEFAULTS {
                let Some(scale) = binding.neutral_scale() else {
                    continue;
                };
                match candidate_storage.with_storage(|| {
                    grid_defaults(ctx, scan.data, record.body(), record.range.start, scale)
                }) {
                    Ok(value) => {
                        ctx.reserve_scoped_vec(
                            &mut native_storage,
                            &mut grids,
                            1,
                            "Rhino grid defaults",
                        )?;
                        native_storage.absorb(&mut candidate_storage)?;
                        grids.push(value);
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            } else if record.typecode == RENDER_SETTINGS {
                let Some(scale) = binding.neutral_scale() else {
                    continue;
                };
                match candidate_storage.with_storage(|| {
                    render_settings(
                        ctx,
                        scan.data,
                        record.body(),
                        record.range.start,
                        scan.archive,
                        scale,
                    )
                }) {
                    Ok(value) => {
                        ctx.reserve_scoped_vec(
                            &mut native_storage,
                            &mut renders,
                            1,
                            "Rhino render settings",
                        )?;
                        native_storage.absorb(&mut candidate_storage)?;
                        renders.push(value);
                        render_settings_seen = true;
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            } else if record.typecode == RENDER_USERDATA {
                if render_settings_seen {
                    match candidate_storage
                        .with_storage(|| render_userdata(ctx, scan.data, record, scan.archive))
                    {
                        Ok(_) => {
                            opaque_records.push_admitted(
                                ctx,
                                OpaqueRecord {
                                    table_typecode: table.typecode,
                                    record: record.clone(),
                                },
                                "Rhino opaque setting records",
                            )?;
                            continue;
                        }
                        Err(FramingError::Resource(limit)) => {
                            return Err(CodecError::ResourceLimit(limit))
                        }
                        Err(error) => Err(error),
                    }
                } else {
                    opaque_records.push_admitted(
                        ctx,
                        OpaqueRecord {
                            table_typecode: table.typecode,
                            record: record.clone(),
                        },
                        "Rhino opaque setting records",
                    )?;
                    continue;
                }
            } else {
                continue;
            };
            if let Err(error) = result {
                if let FramingError::Resource(limit) = &error {
                    return Err(CodecError::ResourceLimit(*limit));
                }
                opaque_records.push_admitted(
                    ctx,
                    OpaqueRecord {
                        table_typecode: table.typecode,
                        record: record.clone(),
                    },
                    "Rhino opaque setting records",
                )?;
                native_storage.with_storage(|| {
                    ctx.reserve_vec(&mut setting_records, 1, "Rhino retained setting records")?;
                    setting_records.push(SettingRecord {
                        id: retained_numbered_id(
                            ctx,
                            "rhino:document:setting#error-",
                            setting_records.len(),
                            "Rhino retained setting ID",
                        )?,
                        source_offset: cadmpeg_core::decode::u64_from_index(record.range.start),
                        byte_len: cadmpeg_core::decode::u64_from_index(record.range.len()),
                        typecode: retained_typecode(ctx, record.typecode)?,
                        sha256: retained_sha256(
                            ctx,
                            &scan.data[record.range.clone()],
                            "Rhino setting SHA-256",
                        )?,
                        parse_error: Some(ctx.format_retained(
                            format_args!("{error}"),
                            "Rhino setting parse error",
                        )?),
                    });
                    Ok::<_, CodecError>(())
                })?;
            }
        }
    }
    let namespace = ir.native.namespace_mut("rhino");
    namespace.set_arena(ctx, "revisions", &revisions)?;
    namespace.set_arena(ctx, "document_notes", &notes)?;
    namespace.set_arena(ctx, "applications", &applications)?;
    namespace.set_arena(ctx, "document_settings", &document_settings)?;
    namespace.set_arena(ctx, "previews", &previews)?;
    namespace.set_arena(ctx, "setting_records", &setting_records)?;
    namespace.set_arena(ctx, "annotation_settings", &annotations)?;
    namespace.set_arena(ctx, "grid_defaults", &grids)?;
    namespace.set_arena(ctx, "render_settings", &renders)?;
    Ok(NativeInstall {
        losses,
        opaque_records,
    })
}

#[cfg(test)]
mod tests;
