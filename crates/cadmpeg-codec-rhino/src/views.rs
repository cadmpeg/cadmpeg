// SPDX-License-Identifier: Apache-2.0
//! Saved and active Rhino view presentation records.

use cadmpeg_ir::document::CadIr;
use serde::Serialize;

use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader, FramingError, TCODE_ENDOFTABLE};
use crate::container::{Record, Scan};
use crate::settings::{plane, utf16, Plane};
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
    construction_plane: Option<ConstructionPlane>,
    viewport: Option<Viewport>,
    children: Vec<ViewChild>,
}

#[derive(Debug, Serialize)]
struct ConstructionPlane {
    plane_origin_mm: [f64; 3],
    plane_x_axis: [f64; 3],
    plane_y_axis: [f64; 3],
    plane_z_axis: [f64; 3],
    plane_equation_mm: [f64; 4],
    grid_spacing_mm: f64,
    snap_spacing_mm: f64,
    grid_line_count: i32,
    thick_line_frequency: i32,
    name: String,
    depth_buffer: bool,
}

#[derive(Debug, Serialize)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent serialized viewport validity and lock flags"
)]
struct Viewport {
    version: [u8; 2],
    camera_valid: bool,
    frustum_valid: bool,
    port_valid: bool,
    projection: i32,
    camera_location_mm: [f64; 3],
    camera_direction: [f64; 3],
    camera_up: [f64; 3],
    camera_x_axis: [f64; 3],
    camera_y_axis: [f64; 3],
    camera_z_axis: [f64; 3],
    frustum_mm: [f64; 6],
    port: [i32; 6],
    source_uuid: Option<String>,
    camera_up_locked: bool,
    camera_direction_locked: bool,
    camera_location_locked: bool,
    frustum_left_right_symmetric: bool,
    frustum_top_bottom_symmetric: bool,
    target_millimeters: Option<[f64; 3]>,
    camera_frame_valid: Option<bool>,
    view_scale: Option<[f64; 3]>,
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

fn bool_i32(reader: &mut BoundedReader<'_>, label: &str) -> Result<bool, FramingError> {
    match reader.i32()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(structural(
            reader.position() - 4,
            format!("{label} flag is invalid"),
        )),
    }
}

fn scale3(value: &mut [f64; 3], scale: f64, offset: usize) -> Result<(), FramingError> {
    for coordinate in value {
        *coordinate = scaled_coordinate(*coordinate, scale)
            .ok_or_else(|| structural(offset, "scaled view coordinate is invalid"))?;
    }
    Ok(())
}

fn scaled_plane(mut value: Plane, scale: f64, offset: usize) -> Result<Plane, FramingError> {
    scale3(&mut value.origin.0, scale, offset)?;
    value.equation[3] = scaled_coordinate(value.equation[3], scale)
        .ok_or_else(|| structural(offset, "scaled plane equation is invalid"))?;
    Ok(value)
}

fn parse_cplane(
    data: &[u8],
    body: std::ops::Range<usize>,
    scale: f64,
) -> Result<ConstructionPlane, FramingError> {
    let mut reader = BoundedReader::new(data, body.start, body.end)?;
    let packed = reader.u8()?;
    if packed >> 4 != 1 || packed & 0x0f > 1 {
        return Err(structural(
            body.start,
            "construction-plane version is unsupported",
        ));
    }
    let value = scaled_plane(plane(&mut reader)?, scale, body.start)?;
    let grid_spacing_mm = scaled_coordinate(reader.f64()?, scale)
        .ok_or_else(|| structural(reader.position() - 8, "grid spacing is invalid"))?;
    let snap_spacing_mm = scaled_coordinate(reader.f64()?, scale)
        .ok_or_else(|| structural(reader.position() - 8, "snap spacing is invalid"))?;
    let grid_line_count = reader.i32()?;
    let thick_line_frequency = reader.i32()?;
    let name = utf16(&mut reader)?;
    let depth_buffer = packed & 0x0f < 1 || reader.bool()?;
    if reader.remaining() != 0 {
        return Err(structural(
            reader.position(),
            "construction plane has trailing bytes",
        ));
    }
    Ok(ConstructionPlane {
        plane_origin_mm: value.origin.0,
        plane_x_axis: value.xaxis.0,
        plane_y_axis: value.yaxis.0,
        plane_z_axis: value.zaxis.0,
        plane_equation_mm: value.equation,
        grid_spacing_mm,
        snap_spacing_mm,
        grid_line_count,
        thick_line_frequency,
        name,
        depth_buffer,
    })
}

fn parse_viewport(
    data: &[u8],
    body: std::ops::Range<usize>,
    scale: f64,
) -> Result<Viewport, FramingError> {
    let mut reader = BoundedReader::new(data, body.start, body.end)?;
    let packed = reader.u8()?;
    let version = [packed >> 4, packed & 0x0f];
    if version[0] != 1 || version[1] > 5 {
        return Err(structural(body.start, "viewport version is unsupported"));
    }
    let camera_valid = bool_i32(&mut reader, "camera-valid")?;
    let frustum_valid = bool_i32(&mut reader, "frustum-valid")?;
    let port_valid = bool_i32(&mut reader, "port-valid")?;
    let projection = reader.i32()?;
    let mut camera_location = [reader.f64()?, reader.f64()?, reader.f64()?];
    scale3(&mut camera_location, scale, reader.position() - 24)?;
    let vector = |reader: &mut BoundedReader<'_>| -> Result<[f64; 3], FramingError> {
        let value = [reader.f64()?, reader.f64()?, reader.f64()?];
        value
            .iter()
            .all(|coordinate| coordinate.is_finite())
            .then_some(value)
            .ok_or_else(|| structural(reader.position() - 24, "viewport vector is invalid"))
    };
    let camera_direction = vector(&mut reader)?;
    let camera_up = vector(&mut reader)?;
    let camera_x_axis = vector(&mut reader)?;
    let camera_y_axis = vector(&mut reader)?;
    let camera_z_axis = vector(&mut reader)?;
    let mut frustum = [0.0; 6];
    for coordinate in &mut frustum {
        *coordinate = scaled_coordinate(reader.f64()?, scale)
            .ok_or_else(|| structural(reader.position() - 8, "viewport frustum is invalid"))?;
    }
    let mut port = [0; 6];
    for coordinate in &mut port {
        *coordinate = reader.i32()?;
    }
    let viewport_id = (version[1] >= 1).then(|| uuid(&mut reader)).transpose()?;
    let mut locks = [false; 5];
    if version[1] >= 2 {
        for lock in &mut locks {
            *lock = reader.bool()?;
        }
    }
    let target = if version[1] >= 3 {
        let mut point = [reader.f64()?, reader.f64()?, reader.f64()?];
        scale3(&mut point, scale, reader.position() - 24)?;
        Some(point)
    } else {
        None
    };
    let camera_frame_valid = (version[1] >= 4).then(|| reader.bool()).transpose()?;
    let view_scale = if version[1] >= 5 {
        let value = [reader.f64()?, reader.f64()?, reader.f64()?];
        if !value
            .iter()
            .all(|coordinate| coordinate.is_finite() && *coordinate > 0.0)
        {
            return Err(structural(
                reader.position() - 24,
                "viewport scale is invalid",
            ));
        }
        Some(value)
    } else {
        None
    };
    if reader.remaining() != 0 {
        return Err(structural(reader.position(), "viewport has trailing bytes"));
    }
    Ok(Viewport {
        version,
        camera_valid,
        frustum_valid,
        port_valid,
        projection,
        camera_location_mm: camera_location,
        camera_direction,
        camera_up,
        camera_x_axis,
        camera_y_axis,
        camera_z_axis,
        frustum_mm: frustum,
        port,
        source_uuid: viewport_id
            .filter(|id| !id.is_nil())
            .map(|id| id.to_string()),
        camera_up_locked: locks[0],
        camera_direction_locked: locks[1],
        camera_location_locked: locks[2],
        frustum_left_right_symmetric: locks[3],
        frustum_top_bottom_symmetric: locks[4],
        target_millimeters: target,
        camera_frame_valid,
        view_scale,
    })
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
    let mut construction_plane = None;
    let mut viewport = None;
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
            VIEW_CPLANE if !child.short => {
                construction_plane = Some(parse_cplane(data, child.body.clone(), scale)?);
            }
            VIEW_VIEWPORT if !child.short => {
                viewport = Some(parse_viewport(data, child.body.clone(), scale)?);
            }
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
        construction_plane,
        viewport,
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

#[cfg(test)]
mod tests {
    use super::{parse_viewport, Viewport};

    fn point(bytes: &mut Vec<u8>, value: [f64; 3]) {
        for coordinate in value {
            bytes.extend(coordinate.to_le_bytes());
        }
    }

    fn viewport() -> Vec<u8> {
        let mut bytes = vec![0x15];
        for value in [1_i32, 1, 1, 2] {
            bytes.extend(value.to_le_bytes());
        }
        point(&mut bytes, [1.0, 2.0, 3.0]);
        for vector in [
            [0.0, 0.0, -1.0],
            [0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ] {
            point(&mut bytes, vector);
        }
        for value in [-2.0_f64, 2.0, -1.0, 1.0, 0.1, 100.0] {
            bytes.extend(value.to_le_bytes());
        }
        for value in [0_i32, 1920, 1080, 0, 0, 1] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend([0x11; 16]);
        bytes.extend([1, 0, 1, 0, 1]);
        point(&mut bytes, [4.0, 5.0, 6.0]);
        bytes.push(1);
        for value in [1.0_f64, 2.0, 3.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn viewport_scales_spatial_state_but_not_frames_or_view_scale() {
        let bytes = viewport();
        let value: Viewport = parse_viewport(&bytes, 0..bytes.len(), 10.0).expect("valid viewport");
        assert_eq!(value.camera_location_mm, [10.0, 20.0, 30.0]);
        assert_eq!(value.camera_direction, [0.0, 0.0, -1.0]);
        assert_eq!(value.frustum_mm, [-20.0, 20.0, -10.0, 10.0, 1.0, 1000.0]);
        assert_eq!(value.target_millimeters, Some([40.0, 50.0, 60.0]));
        assert_eq!(value.view_scale, Some([1.0, 2.0, 3.0]));
        assert!(value.camera_valid && value.camera_location_locked);
    }
}
