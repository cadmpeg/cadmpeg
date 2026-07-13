// SPDX-License-Identifier: Apache-2.0
//! Saved and active Rhino view presentation records.

use cadmpeg_ir::document::CadIr;
use serde::Serialize;

use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader, FramingError, TCODE_ENDOFTABLE};
use crate::container::{Record, Scan};
use crate::settings::utf16;
use crate::wire::{scaled_coordinate, Uuid};

const SETTINGS: u32 = 0x1000_0015;
const NAMED_VIEWS: u32 = 0x2000_8036;
const ACTIVE_VIEWS: u32 = 0x2000_8037;
const VIEW_RECORD: u32 = 0x2000_803b;
const VIEW_CPLANE: u32 = 0x2000_813b;
const VIEW_VIEWPORT: u32 = 0x2000_823b;
const VIEW_SHOW_GRID: u32 = 0xa000_033b;
const VIEW_SHOW_AXES: u32 = 0xa000_043b;
const VIEW_SHOW_WORLD_AXES: u32 = 0xa000_053b;
const VIEW_TRACE_IMAGE: u32 = 0x2000_863b;
const VIEW_WALLPAPER: u32 = 0x2000_873b;
const VIEW_WALLPAPER_V3: u32 = 0x2000_874b;
const VIEW_TARGET: u32 = 0x2000_883b;
const VIEW_V3_DISPLAY_MODE: u32 = 0xa000_093b;
const VIEW_NAME: u32 = 0x2000_8a3b;
const VIEW_POSITION: u32 = 0x2000_8b3b;
const VIEW_ATTRIBUTES: u32 = 0x2000_8c3b;
const VIEW_VIEWPORT_USERDATA: u32 = 0x2000_8d3b;

#[derive(Debug, Serialize)]
struct ViewChild {
    typecode: String,
    kind: &'static str,
    source_offset: u64,
    byte_len: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct ViewRecord {
    id: String,
    source_offset: u64,
    list_kind: &'static str,
    list_index: usize,
    name: String,
    target_millimeters: Option<[f64; 3]>,
    show_construction_grid: bool,
    show_construction_axes: bool,
    show_world_axes: bool,
    legacy_display_mode: Option<i64>,
    view_type: Option<i32>,
    page_width_mm: Option<f64>,
    page_height_mm: Option<f64>,
    display_mode_uuid: Option<String>,
    attributes_version: Option<[u8; 2]>,
    attributes_extension_offset: Option<u64>,
    attributes_extension_byte_len: Option<u64>,
    attributes_extension_sha256: Option<String>,
    children: Vec<ViewChild>,
}

struct ViewAttributes {
    view_type: i32,
    width: f64,
    height: f64,
    display: Option<String>,
    version: [u8; 2],
    suffix: usize,
}

fn structural(offset: usize, message: impl Into<String>) -> FramingError {
    FramingError::Structural {
        offset,
        message: message.into(),
    }
}

fn uuid(reader: &mut BoundedReader<'_>) -> Result<Uuid, FramingError> {
    Ok(Uuid::from_wire(reader.array()?))
}

fn child_kind(typecode: u32) -> &'static str {
    match typecode {
        VIEW_CPLANE => "construction_plane",
        VIEW_VIEWPORT => "viewport",
        VIEW_SHOW_GRID => "show_construction_grid",
        VIEW_SHOW_AXES => "show_construction_axes",
        VIEW_SHOW_WORLD_AXES => "show_world_axes",
        VIEW_TRACE_IMAGE => "trace_image",
        VIEW_WALLPAPER => "wallpaper_path",
        VIEW_WALLPAPER_V3 => "wallpaper",
        VIEW_TARGET => "target",
        VIEW_V3_DISPLAY_MODE => "legacy_display_mode",
        VIEW_NAME => "name",
        VIEW_POSITION => "window_position",
        VIEW_ATTRIBUTES => "attributes",
        VIEW_VIEWPORT_USERDATA => "viewport_userdata",
        TCODE_ENDOFTABLE => "end",
        _ => "extension",
    }
}

fn parse_attributes(
    data: &[u8],
    body: std::ops::Range<usize>,
) -> Result<ViewAttributes, FramingError> {
    let mut reader = BoundedReader::new(data, body.start, body.end)?;
    let packed = reader.u8()?;
    let version = [packed >> 4, packed & 0x0f];
    if version[0] != 1 || version[1] < 1 {
        return Err(structural(
            body.start,
            "view-attributes version is unsupported",
        ));
    }
    let view_type = reader.i32()?;
    let width = reader.f64()?;
    let height = reader.f64()?;
    if !width.is_finite() || !height.is_finite() {
        return Err(structural(
            reader.position() - 16,
            "view page size is not finite",
        ));
    }
    let _obsolete_parent = uuid(&mut reader)?;
    for _ in 0..6 {
        if !reader.f64()?.is_finite() {
            return Err(structural(
                reader.position() - 8,
                "view bounds are not finite",
            ));
        }
    }
    let display = if version[1] >= 2 {
        let id = uuid(&mut reader)?;
        (!id.is_nil()).then(|| id.to_string())
    } else {
        None
    };
    Ok(ViewAttributes {
        view_type,
        width,
        height,
        display,
        version,
        suffix: reader.position(),
    })
}

fn parse_view(
    data: &[u8],
    record: &crate::chunks::Chunk,
    archive: ArchiveVersion,
    scale: f64,
    list_kind: &'static str,
    list_index: usize,
) -> Result<ViewRecord, FramingError> {
    let mut offset = record.body.start;
    let mut name = String::new();
    let mut target = None;
    let mut show_grid = true;
    let mut show_axes = true;
    let mut show_world_axes = true;
    let mut legacy_display_mode = None;
    let mut view_type = None;
    let mut page_width = None;
    let mut page_height = None;
    let mut display_mode_uuid = None;
    let mut attributes_version = None;
    let mut extension_offset = None;
    let mut extension_len = None;
    let mut extension_sha = None;
    let mut children = Vec::new();
    let mut terminated = false;
    while offset < record.body.end {
        let child = chunk_at(data, offset, record.body.end, archive, false)?;
        children.push(ViewChild {
            typecode: format!("{:#010x}", child.typecode),
            kind: child_kind(child.typecode),
            source_offset: offset as u64,
            byte_len: (child.next_offset - offset) as u64,
            sha256: cadmpeg_ir::hash::sha256_hex(&data[offset..child.next_offset]),
        });
        match child.typecode {
            VIEW_NAME if !child.short => {
                let mut reader = BoundedReader::new(data, child.body.start, child.body.end)?;
                name = utf16(&mut reader)?;
                if reader.remaining() != 0 {
                    return Err(structural(
                        reader.position(),
                        "view name has trailing bytes",
                    ));
                }
            }
            VIEW_TARGET if !child.short => {
                let mut reader = BoundedReader::new(data, child.body.start, child.body.end)?;
                let mut point = [reader.f64()?, reader.f64()?, reader.f64()?];
                for value in &mut point {
                    *value = scaled_coordinate(*value, scale).ok_or_else(|| {
                        structural(reader.position() - 24, "scaled view target is invalid")
                    })?;
                }
                target = Some(point);
            }
            VIEW_SHOW_GRID if child.short => show_grid = child.value != 0,
            VIEW_SHOW_AXES if child.short => show_axes = child.value != 0,
            VIEW_SHOW_WORLD_AXES if child.short => show_world_axes = child.value != 0,
            VIEW_V3_DISPLAY_MODE if child.short => legacy_display_mode = Some(child.value),
            VIEW_ATTRIBUTES if !child.short => {
                let attributes = parse_attributes(data, child.body.clone())?;
                view_type = Some(attributes.view_type);
                page_width = Some(attributes.width);
                page_height = Some(attributes.height);
                display_mode_uuid = attributes.display;
                attributes_version = Some(attributes.version);
                if attributes.suffix < child.body.end {
                    extension_offset = Some(attributes.suffix as u64);
                    extension_len = Some((child.body.end - attributes.suffix) as u64);
                    extension_sha = Some(cadmpeg_ir::hash::sha256_hex(
                        &data[attributes.suffix..child.body.end],
                    ));
                }
            }
            TCODE_ENDOFTABLE => {
                if !child.short || child.value != 0 || child.next_offset != record.body.end {
                    return Err(structural(offset, "view end marker is invalid"));
                }
                terminated = true;
            }
            _ => {}
        }
        offset = child.next_offset;
    }
    if !terminated {
        return Err(structural(
            record.body.end,
            "view is missing its end marker",
        ));
    }
    Ok(ViewRecord {
        id: format!("rhino:document:view#{list_kind}-{list_index:04}"),
        source_offset: record.header_start as u64,
        list_kind,
        list_index,
        name,
        target_millimeters: target,
        show_construction_grid: show_grid,
        show_construction_axes: show_axes,
        show_world_axes,
        legacy_display_mode,
        view_type,
        page_width_mm: page_width,
        page_height_mm: page_height,
        display_mode_uuid,
        attributes_version,
        attributes_extension_offset: extension_offset,
        attributes_extension_byte_len: extension_len,
        attributes_extension_sha256: extension_sha,
        children,
    })
}

fn parse_list(
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    scale: f64,
    kind: &'static str,
) -> Vec<ViewRecord> {
    (|| {
        let mut reader = BoundedReader::new(data, record.body.start, record.body.end)?;
        let count = reader.i32()?;
        let count = usize::try_from(count)
            .map_err(|_| structural(reader.position() - 4, "negative view count"))?;
        if count > 1 << 16 {
            return Err(structural(
                reader.position() - 4,
                "view count exceeds limit",
            ));
        }
        let mut views = Vec::with_capacity(count);
        for index in 0..count {
            let view = chunk_at(data, reader.position(), reader.end(), archive, false)?;
            if view.typecode != VIEW_RECORD || view.short {
                return Err(structural(reader.position(), "view record is invalid"));
            }
            views.push(parse_view(data, &view, archive, scale, kind, index)?);
            reader.skip(view.next_offset - reader.position())?;
        }
        if reader.remaining() != 0 {
            return Err(structural(
                reader.position(),
                "view list has trailing bytes",
            ));
        }
        Ok(views)
    })()
    .unwrap_or_default()
}

/// Installs saved and active view records with complete child accounting.
pub(crate) fn install(scan: &Scan, ir: &mut CadIr) {
    let scale = scan
        .metadata
        .settings
        .units
        .as_ref()
        .and_then(|value| value.millimeters_per_unit)
        .unwrap_or(1.0);
    let mut views = Vec::new();
    for table in &scan.tables {
        if table.typecode & !0x0000_8000 != SETTINGS {
            continue;
        }
        for record in &table.records {
            if record.typecode == NAMED_VIEWS {
                views.extend(parse_list(&scan.data, record, scan.archive, scale, "named"));
            }
            if record.typecode == ACTIVE_VIEWS {
                views.extend(parse_list(
                    &scan.data,
                    record,
                    scan.archive,
                    scale,
                    "active",
                ));
            }
        }
    }
    let namespace = ir.native.namespace_mut("rhino");
    namespace.version = namespace.version.max(2);
    namespace
        .set_arena("views", &views)
        .expect("Rhino views serialize");
}
