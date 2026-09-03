// SPDX-License-Identifier: Apache-2.0
//! Saved and active Rhino view presentation records.

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::LossNote;
use serde::Serialize;

use crate::chunks::{
    chunk_at, direct_checksum_ranges, verify_checksum_ranges, ArchiveVersion, BoundedReader,
    ChecksumStatus, FramingError, TCODE_CLASS_END, TCODE_ENDOFTABLE,
};
use crate::container::{OpaqueRecord, Record, Scan};
use crate::objects::parse_userdata;
use crate::settings::{plane, utf16, Plane};
use crate::wire::{scaled_coordinate, Uuid};

const SETTINGS: u32 = 0x1000_0015;
const NAMED_CPLANES: u32 = 0x2000_8035;
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
const CLASS_USERDATA: u32 = 0x0002_7ffd;

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
    window_position: Option<WindowPosition>,
    show_construction_grid: bool,
    show_construction_axes: bool,
    show_world_axes: bool,
    legacy_display_mode: Option<i64>,
    view_type: Option<i32>,
    page_width_mm: Option<f64>,
    page_height_mm: Option<f64>,
    display_mode_uuid: Option<String>,
    attributes_version: Option<[u8; 2]>,
    attributes: Option<ViewAttributes>,
    construction_plane: Option<ConstructionPlane>,
    viewport: Option<Viewport>,
    trace_image: Option<TraceImage>,
    wallpaper: Option<Wallpaper>,
    children: Vec<ViewChild>,
    parse_warnings: Vec<String>,
}

struct ViewportUserdataScan {
    children: Vec<std::ops::Range<usize>>,
    has_untyped_content: bool,
    checksum_warnings: Vec<String>,
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
struct NamedConstructionPlane {
    id: String,
    source_offset: u64,
    list_index: usize,
    value: ConstructionPlane,
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

#[derive(Debug, Serialize)]
struct WindowPosition {
    version: [u8; 2],
    maximized: bool,
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
    floating_viewport: u8,
}

#[derive(Debug, Serialize)]
struct ImageReference {
    full_path: String,
    relative_path: String,
    content_sha1: String,
    embedded_file_uuid: Option<String>,
}

#[derive(Debug, Serialize)]
struct TraceImage {
    legacy_file_path: String,
    width_mm: f64,
    height_mm: f64,
    plane_origin_mm: [f64; 3],
    plane_x_axis: [f64; 3],
    plane_y_axis: [f64; 3],
    grayscale: bool,
    hidden: bool,
    filtered: bool,
    file_reference: Option<ImageReference>,
}

#[derive(Debug, Serialize)]
struct Wallpaper {
    legacy_file_path: String,
    grayscale: bool,
    hidden: bool,
    file_reference: Option<ImageReference>,
}

#[derive(Debug, Serialize)]
struct PageSettings {
    page_number: i32,
    width_mm: f64,
    height_mm: f64,
    margins_mm: [f64; 4],
    printer_name: String,
}

#[derive(Debug, Serialize)]
struct ClippingPlane {
    equation_mm: [f64; 4],
    plane_uuid: Option<String>,
    enabled: bool,
    depth_mm: Option<f64>,
    depth_enabled: bool,
}

#[derive(Debug, Serialize)]
struct ViewAttributes {
    view_type: i32,
    width: f64,
    height: f64,
    display: Option<String>,
    version: [u8; 2],
    page_settings: Option<PageSettings>,
    projection_locked: bool,
    clipping_planes: Vec<ClippingPlane>,
    named_view_uuid: Option<String>,
    show_construction_z_axis: bool,
    focal_blur_distance_mm: Option<f64>,
    focal_blur_aperture: Option<f64>,
    focal_blur_jitter: Option<f64>,
    focal_blur_sample_count: Option<i32>,
    focal_blur_mode: Option<i32>,
    rendering_size_pixels: Option<[i32; 2]>,
    section_behavior: Option<u8>,
}

const UNSET_POSITIVE_FLOAT: f64 = 1.234_321e38;

fn legacy_clipping_depth(value: f64) -> (f64, bool) {
    if value >= 0.0 && value != UNSET_POSITIVE_FLOAT {
        (value, true)
    } else {
        (0.0, false)
    }
}

fn uuid(reader: &mut BoundedReader<'_>) -> Result<Uuid, FramingError> {
    Ok(Uuid::from_wire(reader.array()?))
}

fn bool_i32(reader: &mut BoundedReader<'_>, label: &str) -> Result<bool, FramingError> {
    let _ = label;
    Ok(reader.i32()? != 0)
}

fn scale3(value: &mut [f64; 3], scale: f64, offset: usize) -> Result<(), FramingError> {
    for coordinate in value {
        *coordinate = scaled_coordinate(*coordinate, scale)
            .ok_or_else(|| FramingError::structural(offset, "scaled view coordinate is invalid"))?;
    }
    Ok(())
}

fn scaled_plane(mut value: Plane, scale: f64, offset: usize) -> Result<Plane, FramingError> {
    scale3(&mut value.origin.0, scale, offset)?;
    value.equation[3] = scaled_coordinate(value.equation[3], scale)
        .ok_or_else(|| FramingError::structural(offset, "scaled plane equation is invalid"))?;
    Ok(value)
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

fn image_reference<'a>(
    data: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
) -> Result<(ImageReference, std::ops::Range<usize>), FramingError> {
    let value = crate::instances::file_reference(data, reader, archive, &mut Vec::new())?;
    let source_range = value.source_range.clone();
    Ok((
        ImageReference {
            full_path: value.full_path,
            relative_path: value.relative_path,
            content_sha1: hex(&value.content_hash.content_sha1),
            embedded_file_uuid: value.embedded_file_id.map(|id| id.to_string()),
        },
        source_range,
    ))
}

fn parse_trace_image(
    data: &[u8],
    body: std::ops::Range<usize>,
    archive: ArchiveVersion,
    scale: f64,
) -> Result<(TraceImage, Option<std::ops::Range<usize>>), FramingError> {
    let mut reader = BoundedReader::new(data, body.start, body.end)?;
    let packed = reader.u8()?;
    let minor = packed & 0x0f;
    if packed >> 4 != 1 {
        return Err(FramingError::structural(
            body.start,
            "trace-image version is unsupported",
        ));
    }
    let legacy_file_path = utf16(&mut reader)?;
    let width_mm = scaled_coordinate(reader.f64()?, scale)
        .ok_or_else(|| FramingError::structural(reader.position() - 8, "trace width is invalid"))?;
    let height_mm = scaled_coordinate(reader.f64()?, scale).ok_or_else(|| {
        FramingError::structural(reader.position() - 8, "trace height is invalid")
    })?;
    let plane = scaled_plane(plane(&mut reader)?, scale, body.start)?;
    let grayscale = minor < 1 || reader.bool()?;
    let hidden = minor >= 2 && reader.bool()?;
    let filtered = minor >= 3 && reader.bool()?;
    let (file_reference, file_reference_range) = if minor >= 4 {
        let (value, range) = image_reference(data, &mut reader, archive)?;
        (Some(value), Some(range))
    } else {
        (None, None)
    };
    reader.skip_remaining()?;
    Ok((
        TraceImage {
            legacy_file_path,
            width_mm,
            height_mm,
            plane_origin_mm: plane.origin.0,
            plane_x_axis: plane.xaxis.0,
            plane_y_axis: plane.yaxis.0,
            grayscale,
            hidden,
            filtered,
            file_reference,
        },
        file_reference_range,
    ))
}

fn parse_wallpaper(
    data: &[u8],
    body: std::ops::Range<usize>,
    archive: ArchiveVersion,
) -> Result<(Wallpaper, Option<std::ops::Range<usize>>), FramingError> {
    let mut reader = BoundedReader::new(data, body.start, body.end)?;
    let packed = reader.u8()?;
    let minor = packed & 0x0f;
    if packed >> 4 != 1 {
        return Err(FramingError::structural(
            body.start,
            "wallpaper version is unsupported",
        ));
    }
    let legacy_file_path = utf16(&mut reader)?;
    let grayscale = reader.bool()?;
    let hidden = minor >= 1 && reader.bool()?;
    let (file_reference, file_reference_range) = if minor >= 2 {
        let (value, range) = image_reference(data, &mut reader, archive)?;
        (Some(value), Some(range))
    } else {
        (None, None)
    };
    reader.skip_remaining()?;
    Ok((
        Wallpaper {
            legacy_file_path,
            grayscale,
            hidden,
            file_reference,
        },
        file_reference_range,
    ))
}

fn parse_cplane(
    data: &[u8],
    body: std::ops::Range<usize>,
    scale: f64,
) -> Result<ConstructionPlane, FramingError> {
    let mut reader = BoundedReader::new(data, body.start, body.end)?;
    let packed = reader.u8()?;
    if packed >> 4 != 1 {
        return Err(FramingError::structural(
            body.start,
            "construction-plane version is unsupported",
        ));
    }
    let value = scaled_plane(plane(&mut reader)?, scale, body.start)?;
    let grid_spacing_mm = scaled_coordinate(reader.f64()?, scale).ok_or_else(|| {
        FramingError::structural(reader.position() - 8, "grid spacing is invalid")
    })?;
    let snap_spacing_mm = scaled_coordinate(reader.f64()?, scale).ok_or_else(|| {
        FramingError::structural(reader.position() - 8, "snap spacing is invalid")
    })?;
    let grid_line_count = reader.i32()?;
    let thick_line_frequency = reader.i32()?;
    let name = utf16(&mut reader)?;
    let depth_buffer = packed & 0x0f < 1 || reader.bool()?;
    reader.skip_remaining()?;
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
    if version[0] != 1 {
        return Err(FramingError::structural(
            body.start,
            "viewport version is unsupported",
        ));
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
            .ok_or_else(|| {
                FramingError::structural(reader.position() - 24, "viewport vector is invalid")
            })
    };
    let camera_direction = vector(&mut reader)?;
    let camera_up = vector(&mut reader)?;
    let camera_x_axis = vector(&mut reader)?;
    let camera_y_axis = vector(&mut reader)?;
    let camera_z_axis = vector(&mut reader)?;
    let mut frustum = [0.0; 6];
    for coordinate in &mut frustum {
        *coordinate = scaled_coordinate(reader.f64()?, scale).ok_or_else(|| {
            FramingError::structural(reader.position() - 8, "viewport frustum is invalid")
        })?;
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
            return Err(FramingError::structural(
                reader.position() - 24,
                "viewport scale is invalid",
            ));
        }
        Some(value)
    } else {
        None
    };
    reader.skip_remaining()?;
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

fn parse_window_position(
    data: &[u8],
    body: std::ops::Range<usize>,
) -> Result<WindowPosition, FramingError> {
    let mut reader = BoundedReader::new(data, body.start, body.end)?;
    let packed = reader.u8()?;
    let version = [packed >> 4, packed & 0x0f];
    let mut result = WindowPosition {
        version,
        maximized: false,
        left: 0.0,
        right: 1.0,
        top: 0.0,
        bottom: 1.0,
        floating_viewport: 0,
    };
    if version[0] == 1 {
        result.maximized = reader.i32()? != 0;
        result.left = reader.f64()?;
        result.right = reader.f64()?;
        result.top = reader.f64()?;
        result.bottom = reader.f64()?;
        if version[1] >= 1 {
            result.floating_viewport = reader.u8()?;
        }

        if result.left > result.right {
            std::mem::swap(&mut result.left, &mut result.right);
        }
        if result.left < 0.0 {
            result.left = 0.0;
        }
        if result.right >= 1.0 {
            result.right = 1.0;
        }
        if result.left >= result.right {
            result.left = 0.0;
            result.right = 1.0;
        }
        if result.top > result.bottom {
            std::mem::swap(&mut result.top, &mut result.bottom);
        }
        if result.top < 0.0 {
            result.top = 0.0;
        }
        if result.bottom >= 1.0 {
            result.bottom = 1.0;
        }
        if result.top >= result.bottom {
            result.top = 0.0;
            result.bottom = 1.0;
        }
    }
    reader.skip_remaining()?;
    Ok(result)
}

fn parse_attributes(
    data: &[u8],
    body: std::ops::Range<usize>,
    archive: ArchiveVersion,
    scale: f64,
) -> Result<(ViewAttributes, Vec<std::ops::Range<usize>>), FramingError> {
    let mut reader = BoundedReader::new(data, body.start, body.end)?;
    let mut checksum_children = Vec::new();
    let packed = reader.u8()?;
    let version = [packed >> 4, packed & 0x0f];
    if version[0] != 1 || version[1] < 1 {
        return Err(FramingError::structural(
            body.start,
            "view-attributes version is unsupported",
        ));
    }
    let view_type = reader.i32()?;
    let width = reader.f64()?;
    let height = reader.f64()?;
    if !width.is_finite() || !height.is_finite() {
        return Err(FramingError::structural(
            reader.position() - 16,
            "view page size is not finite",
        ));
    }
    let _obsolete_parent = uuid(&mut reader)?;
    for _ in 0..6 {
        if !reader.f64()?.is_finite() {
            return Err(FramingError::structural(
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
    let mut result = ViewAttributes {
        view_type,
        width,
        height,
        display,
        version,
        page_settings: None,
        projection_locked: false,
        clipping_planes: Vec::new(),
        named_view_uuid: None,
        show_construction_z_axis: false,
        focal_blur_distance_mm: None,
        focal_blur_aperture: None,
        focal_blur_jitter: None,
        focal_blur_sample_count: None,
        focal_blur_mode: None,
        rendering_size_pixels: None,
        section_behavior: None,
    };
    if version[1] >= 2 {
        let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
        if chunk.typecode != 0x4000_8000 || chunk.short {
            return Err(FramingError::structural(
                reader.position(),
                "page-settings wrapper is invalid",
            ));
        }
        let mut page = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
        let page_version = (page.i32()?, page.i32()?);
        if page_version.0 != 1 || page_version.1 < 0 {
            return Err(FramingError::structural(
                page.position(),
                "page-settings version is unsupported",
            ));
        }
        let page_number = page.i32()?;
        let width_mm = page.f64()?;
        let height_mm = page.f64()?;
        let margins_mm = [page.f64()?, page.f64()?, page.f64()?, page.f64()?];
        if ![width_mm, height_mm]
            .into_iter()
            .chain(margins_mm)
            .all(f64::is_finite)
        {
            return Err(FramingError::structural(
                page.position(),
                "page setting is not finite",
            ));
        }
        let printer_name = utf16(&mut page)?;
        page.skip_remaining()?;
        checksum_children.push(chunk.range());
        reader.skip(chunk.next_offset - reader.position())?;
        result.page_settings = Some(PageSettings {
            page_number,
            width_mm,
            height_mm,
            margins_mm,
            printer_name,
        });
    }
    if version[1] >= 3 {
        result.projection_locked = reader.bool()?;
    }
    if version[1] >= 4 {
        let count_offset = reader.position();
        let count = usize::try_from(reader.i32()?)
            .ok()
            .filter(|count| *count <= 1 << 16)
            .ok_or_else(|| {
                FramingError::structural(count_offset, "clipping-plane count is invalid")
            })?;
        for _ in 0..count {
            let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
            if chunk.typecode != 0x4000_8000 || chunk.short {
                return Err(FramingError::structural(
                    reader.position(),
                    "clipping-plane wrapper is invalid",
                ));
            }
            let mut plane = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
            let (major, minor) = (plane.i32()?, plane.i32()?);
            if major != 1 || minor < 0 {
                return Err(FramingError::structural(
                    plane.position(),
                    "clipping-plane version is unsupported",
                ));
            }
            let mut equation = [plane.f64()?, plane.f64()?, plane.f64()?, plane.f64()?];
            if !equation.iter().all(|value| value.is_finite()) {
                return Err(FramingError::structural(
                    plane.position() - 32,
                    "clipping equation is invalid",
                ));
            }
            equation[3] = scaled_coordinate(equation[3], scale).ok_or_else(|| {
                FramingError::structural(plane.position() - 8, "clipping equation is invalid")
            })?;
            let id = uuid(&mut plane)?;
            let enabled = plane.bool()?;
            let (depth, legacy_depth_enabled) = if minor >= 1 {
                let raw_depth = plane.f64()?;
                let (raw_depth, enabled) = if minor <= 2 {
                    legacy_clipping_depth(raw_depth)
                } else {
                    (raw_depth, false)
                };
                (
                    Some(scaled_coordinate(raw_depth, scale).ok_or_else(|| {
                        FramingError::structural(plane.position() - 8, "clipping depth is invalid")
                    })?),
                    enabled,
                )
            } else {
                (None, false)
            };
            let depth_enabled = if minor >= 3 {
                plane.bool()?
            } else {
                legacy_depth_enabled
            };
            result.clipping_planes.push(ClippingPlane {
                equation_mm: equation,
                plane_uuid: (!id.is_nil()).then(|| id.to_string()),
                enabled,
                depth_mm: depth,
                depth_enabled,
            });
            checksum_children.push(chunk.range());
            reader.skip(chunk.next_offset - reader.position())?;
        }
    }
    if version[1] >= 5 {
        let id = uuid(&mut reader)?;
        result.named_view_uuid = (!id.is_nil()).then(|| id.to_string());
    }
    if version[1] >= 6 {
        result.show_construction_z_axis = reader.bool()?;
    }
    if version[1] >= 7 {
        result.focal_blur_distance_mm =
            Some(scaled_coordinate(reader.f64()?, scale).ok_or_else(|| {
                FramingError::structural(reader.position() - 8, "focal distance is invalid")
            })?);
        result.focal_blur_aperture = Some(reader.f64()?);
        result.focal_blur_jitter = Some(reader.f64()?);
        result.focal_blur_sample_count = Some(reader.i32()?);
        result.focal_blur_mode = Some(reader.i32()?);
    }
    if version[1] >= 8 {
        result.rendering_size_pixels = Some([reader.i32()?, reader.i32()?]);
    }
    if version[1] >= 9 {
        result.section_behavior = Some(reader.u8()?);
    }
    reader.skip_remaining()?;
    Ok((result, checksum_children))
}

fn view_child_checksum_warning(
    data: &[u8],
    child: &crate::chunks::Chunk,
    direct_ranges: &[std::ops::Range<usize>],
) -> Result<Option<String>, FramingError> {
    match verify_checksum_ranges(data, child, direct_ranges)? {
        ChecksumStatus::Mismatch { expected, actual } => Ok(Some(format!(
            "CRC mismatch at offset {} for typecode {:#x}: expected {expected:#x}, got {actual:#x}",
            child.header_start, child.typecode
        ))),
        _ => Ok(None),
    }
}

fn direct_view_child_checksum_warning(
    data: &[u8],
    child: &crate::chunks::Chunk,
) -> Result<Option<String>, FramingError> {
    view_child_checksum_warning(data, child, std::slice::from_ref(&child.body))
}

fn view_child_checksum_warning_excluding(
    data: &[u8],
    child: &crate::chunks::Chunk,
    nested_children: &[std::ops::Range<usize>],
) -> Result<Option<String>, FramingError> {
    let direct = direct_checksum_ranges(&child.body, nested_children)?;
    view_child_checksum_warning(data, child, &direct)
}

fn scan_viewport_userdata(
    data: &[u8],
    body: std::ops::Range<usize>,
    archive: ArchiveVersion,
) -> Result<ViewportUserdataScan, FramingError> {
    let mut reader = BoundedReader::new(data, body.start, body.end)?;
    let mut children = Vec::new();
    let mut has_untyped_content = false;
    let mut checksum_warnings = Vec::new();
    loop {
        if reader.position() == reader.end() {
            return Err(FramingError::structural(
                reader.end(),
                "view viewport userdata is missing its class end",
            ));
        }
        let start = reader.position();
        let child = chunk_at(data, start, reader.end(), archive, false)?;
        if child.next_offset <= start {
            return Err(FramingError::structural(
                start,
                "view viewport userdata child did not advance",
            ));
        }
        if children.len() >= 1 << 20 {
            return Err(FramingError::InvalidLength {
                offset: start,
                value: children.len() as i128,
            });
        }
        children.push(child.range());
        reader.skip(child.next_offset - start)?;
        match child.typecode {
            CLASS_USERDATA => {
                if child.short {
                    return Err(FramingError::structural(
                        child.header_start,
                        "view viewport userdata item must be a long chunk",
                    ));
                }
                let mut warnings = Vec::new();
                parse_userdata(data, &child, archive, &mut warnings)?;
                checksum_warnings.extend(warnings);
                has_untyped_content = true;
            }
            TCODE_CLASS_END => {
                if !child.short || child.value != 0 {
                    return Err(FramingError::structural(
                        child.header_start,
                        "view viewport userdata class end must be a short zero chunk",
                    ));
                }
                return Ok(ViewportUserdataScan {
                    children,
                    has_untyped_content,
                    checksum_warnings,
                });
            }
            0 => {
                return Err(FramingError::structural(
                    child.header_start,
                    "view viewport userdata contains a zero typecode",
                ));
            }
            _ => has_untyped_content = true,
        }
    }
}

fn parse_view(
    data: &[u8],
    record: &crate::chunks::Chunk,
    archive: ArchiveVersion,
    scale: f64,
    list_kind: &'static str,
    list_index: usize,
) -> Result<(ViewRecord, Vec<LossNote>), FramingError> {
    let mut offset = record.body.start;
    let mut name = String::new();
    let mut target = None;
    let mut window_position = None;
    let mut show_grid = true;
    let mut show_axes = true;
    let mut show_world_axes = true;
    let mut legacy_display_mode = None;
    let mut view_type = None;
    let mut page_width = None;
    let mut page_height = None;
    let mut display_mode_uuid = None;
    let mut attributes_version = None;
    let mut attributes_detail = None;
    let mut construction_plane = None;
    let mut viewport = None;
    let mut trace_image = None;
    let mut wallpaper = None;
    let mut children = Vec::new();
    let mut checksum_children = Vec::new();
    let mut checksum_warnings: Vec<LossNote> = Vec::new();
    let mut parse_warnings = Vec::new();
    let mut terminated = false;
    while offset < record.body.end {
        let child = chunk_at(data, offset, record.body.end, archive, false)?;
        checksum_children.push(child.range());
        if matches!(
            child.typecode,
            VIEW_VIEWPORT | VIEW_CPLANE | VIEW_TARGET | VIEW_POSITION | VIEW_NAME | VIEW_WALLPAPER
        ) {
            if let Some(warning) = direct_view_child_checksum_warning(data, &child)? {
                checksum_warnings.push(crate::loss::RhinoLossCode::IntegrityFailure.note(warning));
            }
        }
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
                match parse_viewport(data, child.body.clone(), scale) {
                    Ok(value) => viewport = Some(value),
                    Err(error) => parse_warnings.push(format!("viewport retained: {error}")),
                }
            }
            VIEW_TRACE_IMAGE if !child.short => {
                let (value, file_reference_range) =
                    parse_trace_image(data, child.body.clone(), archive, scale)?;
                let nested_children = file_reference_range.into_iter().collect::<Vec<_>>();
                if let Some(warning) =
                    view_child_checksum_warning_excluding(data, &child, &nested_children)?
                {
                    checksum_warnings
                        .push(crate::loss::RhinoLossCode::IntegrityFailure.note(warning));
                }
                trace_image = Some(value);
            }
            VIEW_WALLPAPER if !child.short => {
                let mut reader = BoundedReader::new(data, child.body.start, child.body.end)?;
                let path = utf16(&mut reader)?;
                reader.skip_remaining()?;
                wallpaper = Some(Wallpaper {
                    legacy_file_path: path,
                    grayscale: true,
                    hidden: false,
                    file_reference: None,
                });
            }
            VIEW_WALLPAPER_V3 if !child.short => {
                let (value, file_reference_range) =
                    parse_wallpaper(data, child.body.clone(), archive)?;
                let nested_children = file_reference_range.into_iter().collect::<Vec<_>>();
                if let Some(warning) =
                    view_child_checksum_warning_excluding(data, &child, &nested_children)?
                {
                    checksum_warnings
                        .push(crate::loss::RhinoLossCode::IntegrityFailure.note(warning));
                }
                wallpaper = Some(value);
            }
            VIEW_NAME if !child.short => {
                let mut reader = BoundedReader::new(data, child.body.start, child.body.end)?;
                name = utf16(&mut reader)?;
                reader.skip_remaining()?;
            }
            VIEW_TARGET if !child.short => {
                let mut reader = BoundedReader::new(data, child.body.start, child.body.end)?;
                let mut point = [reader.f64()?, reader.f64()?, reader.f64()?];
                for value in &mut point {
                    *value = scaled_coordinate(*value, scale).ok_or_else(|| {
                        FramingError::structural(
                            reader.position() - 24,
                            "scaled view target is invalid",
                        )
                    })?;
                }
                reader.skip_remaining()?;
                target = Some(point);
            }
            VIEW_POSITION if !child.short => {
                window_position = Some(parse_window_position(data, child.body.clone())?);
            }
            VIEW_SHOW_GRID if child.short => show_grid = child.value != 0,
            VIEW_SHOW_AXES if child.short => show_axes = child.value != 0,
            VIEW_SHOW_WORLD_AXES if child.short => show_world_axes = child.value != 0,
            VIEW_V3_DISPLAY_MODE if child.short => legacy_display_mode = Some(child.value),
            VIEW_ATTRIBUTES if !child.short => {
                let (attributes, nested_children) =
                    parse_attributes(data, child.body.clone(), archive, scale)?;
                if let Some(warning) =
                    view_child_checksum_warning_excluding(data, &child, &nested_children)?
                {
                    checksum_warnings
                        .push(crate::loss::RhinoLossCode::IntegrityFailure.note(warning));
                }
                view_type = Some(attributes.view_type);
                page_width = Some(attributes.width);
                page_height = Some(attributes.height);
                display_mode_uuid.clone_from(&attributes.display);
                attributes_version = Some(attributes.version);
                attributes_detail = Some(attributes);
            }
            VIEW_VIEWPORT_USERDATA => {
                if child.short {
                    checksum_warnings.push(
                        crate::loss::RhinoLossCode::ViewportUserdataDropped.note(format!(
                            "viewport userdata at offset {} must be a long chunk",
                            child.header_start
                        )),
                    );
                } else {
                    match scan_viewport_userdata(data, child.body.clone(), archive) {
                        Ok(scan) => {
                            if let Some(warning) =
                                view_child_checksum_warning_excluding(data, &child, &scan.children)?
                            {
                                checksum_warnings.push(
                                    crate::loss::RhinoLossCode::IntegrityFailure.note(warning),
                                );
                            }
                            for warning in scan.checksum_warnings {
                                checksum_warnings.push(
                                    crate::loss::RhinoLossCode::IntegrityFailure.note(warning),
                                );
                            }
                            if scan.has_untyped_content {
                                checksum_warnings.push(
                                    crate::loss::RhinoLossCode::ViewportUserdataDropped.note(
                                        format!(
                                    "viewport userdata at offset {} has no typed CADIR owner",
                                    child.header_start
                                ),
                                    ),
                                );
                            }
                        }
                        Err(error) => checksum_warnings.push(
                            crate::loss::RhinoLossCode::ViewportUserdataDropped.note(format!(
                                "viewport userdata at offset {} could not be framed: {error}",
                                child.header_start
                            )),
                        ),
                    }
                }
            }
            TCODE_ENDOFTABLE => {
                if !child.short || child.value != 0 {
                    return Err(FramingError::structural(
                        offset,
                        "view end marker is invalid",
                    ));
                }
                terminated = true;
                break;
            }
            _ => {}
        }
        offset = child.next_offset;
    }
    if !terminated {
        return Err(FramingError::structural(
            record.body.end,
            "view is missing its end marker",
        ));
    }
    let direct = direct_checksum_ranges(&record.body, &checksum_children)?;
    let checksum_warning = match verify_checksum_ranges(data, record, &direct)? {
        ChecksumStatus::Mismatch { expected, actual } => Some(format!(
            "CRC mismatch at offset {} for typecode {:#x}: expected {expected:#x}, got {actual:#x}",
            record.header_start, record.typecode
        )),
        _ => None,
    };
    if let Some(warning) = checksum_warning {
        checksum_warnings.push(crate::loss::RhinoLossCode::IntegrityFailure.note(warning));
    }
    Ok((
        ViewRecord {
            id: format!("rhino:document:view#{list_kind}-{list_index:04}"),
            source_offset: record.header_start as u64,
            list_kind,
            list_index,
            name,
            target_millimeters: target,
            window_position,
            show_construction_grid: show_grid,
            show_construction_axes: show_axes,
            show_world_axes,
            legacy_display_mode,
            view_type,
            page_width_mm: page_width,
            page_height_mm: page_height,
            display_mode_uuid,
            attributes_version,
            attributes: attributes_detail,
            construction_plane,
            viewport,
            trace_image,
            wallpaper,
            children,
            parse_warnings,
        },
        checksum_warnings,
    ))
}

fn parse_list(
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    scale: f64,
    kind: &'static str,
) -> (Vec<ViewRecord>, Vec<LossNote>) {
    let mut losses = Vec::new();
    let mut reader = match BoundedReader::new(data, record.body.start, record.body.end) {
        Ok(reader) => reader,
        Err(error) => {
            losses.push(
                crate::loss::RhinoLossCode::PresentationRecordDropped.note(format!(
                    "{kind} view list at offset {} could not be framed: {error}",
                    record.body.start
                )),
            );
            return (Vec::new(), losses);
        }
    };
    let signed_count = match reader.i32() {
        Ok(value) => value,
        Err(error) => {
            losses.push(
                crate::loss::RhinoLossCode::PresentationRecordDropped.note(format!(
                    "{kind} view list at offset {} has no readable count: {error}",
                    record.body.start
                )),
            );
            return (Vec::new(), losses);
        }
    };
    let Ok(count) = usize::try_from(signed_count) else {
        losses.push(
            crate::loss::RhinoLossCode::PresentationRecordDropped.note(format!(
                "{kind} view list at offset {} has a negative count {signed_count}",
                record.body.start
            )),
        );
        return (Vec::new(), losses);
    };
    if count > 1 << 16 {
        losses.push(
            crate::loss::RhinoLossCode::PresentationRecordDropped.note(format!(
                "{kind} view list at offset {} exceeds the 65536-entry bound",
                record.body.start
            )),
        );
        return (Vec::new(), losses);
    }
    let mut views = Vec::new();
    for index in 0..count {
        let child_offset = reader.position();
        let view = match chunk_at(data, reader.position(), reader.end(), archive, false) {
            Ok(view) => view,
            Err(error) => {
                losses.push(
                    crate::loss::RhinoLossCode::PresentationRecordDropped.note(format!(
                        "{kind} view record at offset {} could not be framed: {error}",
                        reader.position()
                    )),
                );
                break;
            }
        };
        if view.typecode != VIEW_RECORD || view.short {
            losses.push(
                crate::loss::RhinoLossCode::PresentationRecordDropped.note(format!(
                    "{kind} view list child at offset {child_offset} has unexpected typecode {:#010x}",
                    view.typecode
                )),
            );
            break;
        }
        let next = view.next_offset;
        match parse_view(data, &view, archive, scale, kind, index) {
            Ok((value, checksum_warnings)) => {
                losses.extend(checksum_warnings);
                views.push(value);
            }
            Err(error) => losses.push(
                crate::loss::RhinoLossCode::PresentationRecordDropped.note(format!(
                    "{kind} view record at offset {} was omitted after child parsing failed: {error}",
                    view.header_start
                )),
            ),
        }
        if let Err(error) = reader.skip(next - reader.position()) {
            losses.push(
                crate::loss::RhinoLossCode::PresentationRecordDropped.note(format!(
                    "{kind} view record at offset {} could not advance to its bounded end: {error}",
                    view.header_start
                )),
            );
            break;
        }
    }
    (views, losses)
}

fn parse_named_cplanes(
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    scale: f64,
) -> Result<Vec<NamedConstructionPlane>, FramingError> {
    let mut reader = BoundedReader::new(data, record.body.start, record.body.end)?;
    let count_offset = reader.position();
    let count = usize::try_from(reader.i32()?)
        .ok()
        .filter(|count| *count <= 1 << 16)
        .ok_or_else(|| {
            FramingError::structural(count_offset, "named construction-plane count is invalid")
        })?;
    let mut values = Vec::new();
    for index in 0..count {
        let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
        if chunk.typecode != VIEW_CPLANE || chunk.short {
            return Err(FramingError::structural(
                reader.position(),
                "named construction-plane record is invalid",
            ));
        }
        values.push(NamedConstructionPlane {
            id: format!("rhino:document:construction_plane#{index:04}"),
            source_offset: chunk.header_start as u64,
            list_index: index,
            value: parse_cplane(data, chunk.body.clone(), scale)?,
        });
        reader.skip(chunk.next_offset - reader.position())?;
    }
    reader.skip_remaining()?;
    Ok(values)
}

/// Result of installing saved and active view records.
pub(crate) struct ViewInstall {
    /// Losses from view records that could not be transferred.
    pub(crate) losses: Vec<LossNote>,
    /// Complete settings records whose view payload was not admitted.
    pub(crate) opaque_records: Vec<OpaqueRecord>,
}

/// Installs saved and active view records with complete child accounting.
pub(crate) fn install(scan: &Scan<'_>, ir: &mut CadIr) -> ViewInstall {
    let scale = scan
        .metadata
        .settings
        .units
        .as_ref()
        .and_then(|value| value.millimeters_per_unit)
        .unwrap_or(1.0);
    let mut views = Vec::new();
    let mut cplanes = Vec::new();
    let mut losses = Vec::new();
    let mut opaque_records = Vec::new();
    for table in &scan.tables {
        if table.typecode & !0x0000_8000 != SETTINGS {
            continue;
        }
        for record in &table.records {
            if record.typecode == NAMED_CPLANES {
                match parse_named_cplanes(scan.data, record, scan.archive, scale) {
                    Ok(values) => cplanes.extend(values),
                    Err(error) => {
                        losses.push(crate::loss::RhinoLossCode::PresentationRecordDropped.note(
                            format!(
                                "named construction-plane list at offset {} was omitted after parsing failed: {error}",
                                record.range.start
                            ),
                        ));
                        opaque_records.push(OpaqueRecord {
                            table_typecode: table.typecode,
                            record: record.clone(),
                        });
                    }
                }
            }
            if record.typecode == NAMED_VIEWS {
                let (parsed, mut parse_losses) =
                    parse_list(scan.data, record, scan.archive, scale, "named");
                if !parse_losses.is_empty() {
                    opaque_records.push(OpaqueRecord {
                        table_typecode: table.typecode,
                        record: record.clone(),
                    });
                }
                views.extend(parsed);
                losses.append(&mut parse_losses);
            }
            if record.typecode == ACTIVE_VIEWS {
                let (parsed, mut parse_losses) =
                    parse_list(scan.data, record, scan.archive, scale, "active");
                if !parse_losses.is_empty() {
                    opaque_records.push(OpaqueRecord {
                        table_typecode: table.typecode,
                        record: record.clone(),
                    });
                }
                views.extend(parsed);
                losses.append(&mut parse_losses);
            }
        }
    }
    let namespace = ir.native.namespace_mut("rhino");
    namespace.ensure_version_at_least(
        std::num::NonZeroU32::new(2).expect("Rhino native version is nonzero"),
    );
    namespace
        .set_arena("views", &views)
        .expect("Rhino views serialize");
    namespace
        .set_arena("construction_planes", &cplanes)
        .expect("Rhino construction planes serialize");
    ViewInstall {
        losses,
        opaque_records,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        legacy_clipping_depth, parse_attributes, parse_cplane, parse_list, parse_trace_image,
        parse_viewport, parse_wallpaper, parse_window_position, Viewport, NAMED_CPLANES,
        UNSET_POSITIVE_FLOAT,
    };
    use crate::chunks::ArchiveVersion;
    use crate::container::Record;
    use crate::test_support::test_dump::{
        anonymous_chunk, class_userdata_v2_with_direct_payload, crc_chunk, crc_chunk_excluding,
        file_reference, long_chunk, short_chunk, utf16_bytes,
    };

    fn point(bytes: &mut Vec<u8>, value: [f64; 3]) {
        for coordinate in value {
            bytes.extend(coordinate.to_le_bytes());
        }
    }

    fn serialized_plane(bytes: &mut Vec<u8>) {
        point(bytes, [0.0, 0.0, 0.0]);
        point(bytes, [1.0, 0.0, 0.0]);
        point(bytes, [0.0, 1.0, 0.0]);
        point(bytes, [0.0, 0.0, 1.0]);
        bytes.extend(
            [0.0_f64, 0.0, 1.0, 0.0]
                .into_iter()
                .flat_map(f64::to_le_bytes),
        );
    }

    fn construction_plane() -> Vec<u8> {
        let mut bytes = vec![0x11];
        point(&mut bytes, [1.0, -2.0, 3.0]);
        point(&mut bytes, [1.0, 0.0, 0.0]);
        point(&mut bytes, [0.0, 0.0, 1.0]);
        point(&mut bytes, [0.0, -1.0, 0.0]);
        bytes.extend(
            [0.0_f64, -1.0, 0.0, -2.0]
                .into_iter()
                .flat_map(f64::to_le_bytes),
        );
        bytes.extend(2.5_f64.to_le_bytes());
        bytes.extend(0.75_f64.to_le_bytes());
        for value in [42_i32, 3] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(utf16_bytes("construction-plane"));
        bytes.push(0);
        bytes.extend([0xde, 0xad]);
        bytes
    }

    #[test]
    fn construction_plane_scales_spatial_fields_and_reads_depth_flag() {
        let bytes = construction_plane();
        let value = parse_cplane(&bytes, 0..bytes.len(), 2.0).expect("construction plane");

        assert_eq!(value.plane_origin_mm, [2.0, -4.0, 6.0]);
        assert_eq!(value.plane_x_axis, [1.0, 0.0, 0.0]);
        assert_eq!(value.plane_y_axis, [0.0, 0.0, 1.0]);
        assert_eq!(value.plane_z_axis, [0.0, -1.0, 0.0]);
        assert_eq!(value.plane_equation_mm, [0.0, -1.0, 0.0, -4.0]);
        assert_eq!(value.grid_spacing_mm, 5.0);
        assert_eq!(value.snap_spacing_mm, 1.5);
        assert_eq!(value.grid_line_count, 42);
        assert_eq!(value.thick_line_frequency, 3);
        assert_eq!(value.name, "construction-plane");
        assert!(!value.depth_buffer);
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
        bytes.extend([0xfa, 0xfb, 0xfc]);
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

    #[test]
    fn legacy_clipping_depth_rejects_negative_and_unset_values() {
        assert_eq!(legacy_clipping_depth(2.5), (2.5, true));
        assert_eq!(legacy_clipping_depth(-1.0), (0.0, false));
        assert_eq!(legacy_clipping_depth(UNSET_POSITIVE_FLOAT), (0.0, false));
    }

    #[test]
    fn window_position_repairs_bounds_and_skips_future_suffix() {
        let mut body = vec![0x12];
        body.extend(1_i32.to_le_bytes());
        for value in [0.9_f64, 0.1, -0.25, 1.5] {
            body.extend(value.to_le_bytes());
        }
        body.push(3);
        body.extend([0xde, 0xad, 0xbe, 0xef]);

        let value = parse_window_position(&body, 0..body.len()).expect("window position");
        assert_eq!(value.version, [1, 2]);
        assert!(value.maximized);
        assert_eq!(value.left, 0.1);
        assert_eq!(value.right, 0.9);
        assert_eq!(value.top, 0.0);
        assert_eq!(value.bottom, 1.0);
        assert_eq!(value.floating_viewport, 3);
    }

    #[test]
    fn unknown_window_position_major_keeps_source_default() {
        let mut body = vec![0x20];
        body.extend(1_i32.to_le_bytes());
        for value in [0.2_f64, 0.8, 0.3, 0.7] {
            body.extend(value.to_le_bytes());
        }
        body.extend([0x13, 0x57, 0x9b, 0xdf]);

        let value = parse_window_position(&body, 0..body.len()).expect("window position");
        assert_eq!(value.version, [2, 0]);
        assert!(!value.maximized);
        assert_eq!(
            [value.left, value.right, value.top, value.bottom],
            [0.0, 1.0, 0.0, 1.0]
        );
        assert_eq!(value.floating_viewport, 0);
    }

    #[test]
    fn view_images_decode_minor_gated_fields_and_suffix() {
        let archive = ArchiveVersion::V5;
        let mut trace = vec![0x13];
        trace.extend(utf16_bytes("trace-witness.png"));
        trace.extend(42.0_f64.to_le_bytes());
        trace.extend(24.0_f64.to_le_bytes());
        serialized_plane(&mut trace);
        trace.extend([0, 1, 1]);
        trace.extend([0xde, 0xad, 0xbe, 0xef]);
        let (trace, _) =
            parse_trace_image(&trace, 0..trace.len(), archive, 1.0).expect("trace image");
        assert_eq!(trace.legacy_file_path, "trace-witness.png");
        assert_eq!([trace.width_mm, trace.height_mm], [42.0, 24.0]);
        assert!(!trace.grayscale);
        assert!(trace.hidden && trace.filtered);
        assert!(trace.file_reference.is_none());

        let mut wallpaper = vec![0x11];
        wallpaper.extend(utf16_bytes("wallpaper-witness.png"));
        wallpaper.extend([0, 1]);
        wallpaper.extend([0xca, 0xfe]);
        let (wallpaper, _) =
            parse_wallpaper(&wallpaper, 0..wallpaper.len(), archive).expect("wallpaper");
        assert_eq!(wallpaper.legacy_file_path, "wallpaper-witness.png");
        assert!(!wallpaper.grayscale && wallpaper.hidden);
        assert!(wallpaper.file_reference.is_none());
    }

    #[test]
    fn failed_view_record_is_omitted_and_emits_typed_loss() {
        let archive = ArchiveVersion::V5;
        let view = long_chunk(archive, super::VIEW_RECORD, &[0]);
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(view);
        let record = Record {
            typecode: super::NAMED_VIEWS,
            range: 0..body.len(),
            body: 0..body.len(),
            short: false,
            value: 0,
        };

        let (views, losses) = parse_list(&body, &record, archive, 1.0, "named");
        assert!(views.is_empty());
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::RhinoLossCode::PresentationRecordDropped.kind()
        );
    }

    #[test]
    fn unexpected_view_child_type_stops_the_counted_list() {
        let archive = ArchiveVersion::V5;
        let child = crc_chunk(archive, NAMED_CPLANES, &[0]);
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(child);
        let record = Record {
            typecode: super::NAMED_VIEWS,
            range: 0..body.len(),
            body: 0..body.len(),
            short: false,
            value: 0,
        };

        let (views, losses) = parse_list(&body, &record, archive, 1.0, "named");
        assert!(views.is_empty());
        assert_eq!(losses.len(), 1);
        assert!(losses[0].message.contains("unexpected typecode"));
    }

    #[test]
    fn view_record_crc_mismatch_is_recoverable_integrity_loss() {
        let archive = ArchiveVersion::V5;
        let end_marker = short_chunk(archive, super::TCODE_ENDOFTABLE, 0);
        let end_marker_range = 0..end_marker.len();
        let mut view = crc_chunk_excluding(
            archive,
            super::VIEW_RECORD,
            &end_marker,
            std::slice::from_ref(&end_marker_range),
        );
        let crc_offset = view.len() - 1;
        view[crc_offset] ^= 1;
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(view);
        let record = Record {
            typecode: super::NAMED_VIEWS,
            range: 0..body.len(),
            body: 0..body.len(),
            short: false,
            value: 0,
        };

        let (views, losses) = parse_list(&body, &record, archive, 1.0, "named");
        assert_eq!(views.len(), 1);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::RhinoLossCode::IntegrityFailure.kind()
        );
        assert!(losses[0].message.contains("CRC mismatch"));
    }

    #[test]
    fn direct_view_child_crc_mismatch_is_recoverable_integrity_loss() {
        let archive = ArchiveVersion::V5;
        let mut target = crc_chunk(archive, super::VIEW_TARGET, &[0; 24]);
        let crc_offset = target.len() - 1;
        target[crc_offset] ^= 1;
        let end_marker = short_chunk(archive, super::TCODE_ENDOFTABLE, 0);
        let mut view_body = target;
        view_body.extend(end_marker);
        let view_body_range = 0..view_body.len();
        let view = crc_chunk_excluding(
            archive,
            super::VIEW_RECORD,
            &view_body,
            std::slice::from_ref(&view_body_range),
        );
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(view);
        let record = Record {
            typecode: super::NAMED_VIEWS,
            range: 0..body.len(),
            body: 0..body.len(),
            short: false,
            value: 0,
        };

        let (views, losses) = parse_list(&body, &record, archive, 1.0, "named");
        assert_eq!(views.len(), 1);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::RhinoLossCode::IntegrityFailure.kind()
        );
        assert!(losses[0].message.contains("0x2000883b"));
    }

    #[test]
    fn view_end_marker_stops_typed_children_before_bounded_suffix() {
        let archive = ArchiveVersion::V5;
        let end_marker = short_chunk(archive, super::TCODE_ENDOFTABLE, 0);
        let end_marker_range = 0..end_marker.len();
        let mut view_body = end_marker;
        view_body.extend([0xaa, 0xbb]);
        let view = crc_chunk_excluding(
            archive,
            super::VIEW_RECORD,
            &view_body,
            std::slice::from_ref(&end_marker_range),
        );
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(view);
        let record = Record {
            typecode: super::NAMED_VIEWS,
            range: 0..body.len(),
            body: 0..body.len(),
            short: false,
            value: 0,
        };

        let (views, losses) = parse_list(&body, &record, archive, 1.0, "named");
        assert_eq!(views.len(), 1);
        assert!(losses.is_empty());
        assert_eq!(views[0].children.len(), 1);
    }

    #[test]
    fn view_attributes_skip_later_clipping_plane_suffix() {
        let archive = ArchiveVersion::V5;
        let mut page = Vec::new();
        page.extend(0_i32.to_le_bytes());
        page.extend(100.0_f64.to_le_bytes());
        page.extend(200.0_f64.to_le_bytes());
        for _ in 0..4 {
            page.extend(0.0_f64.to_le_bytes());
        }
        page.extend(utf16_bytes(""));

        let mut clipping_plane = Vec::new();
        for value in [0.0_f64, 0.0, 1.0, 0.0] {
            clipping_plane.extend(value.to_le_bytes());
        }
        clipping_plane.extend([0x11; 16]);
        clipping_plane.push(1);
        clipping_plane.extend(3.0_f64.to_le_bytes());
        clipping_plane.push(1);
        clipping_plane.extend([0xaa, 0xbb]);

        let mut body = vec![0x14];
        body.extend(0_i32.to_le_bytes());
        body.extend(100.0_f64.to_le_bytes());
        body.extend(200.0_f64.to_le_bytes());
        body.extend([0; 16]);
        for _ in 0..6 {
            body.extend(0.0_f64.to_le_bytes());
        }
        body.extend([0; 16]);
        body.extend(anonymous_chunk(archive, 0, &page));
        body.push(1);
        body.extend(1_i32.to_le_bytes());
        body.extend(anonymous_chunk(archive, 4, &clipping_plane));

        let (value, _) =
            parse_attributes(&body, 0..body.len(), archive, 1.0).expect("view attributes");
        assert_eq!(value.clipping_planes.len(), 1);
        assert_eq!(value.clipping_planes[0].depth_mm, Some(3.0));
        assert!(value.clipping_planes[0].depth_enabled);
    }

    #[test]
    fn view_attributes_decode_page_settings_and_skip_page_suffix() {
        let archive = ArchiveVersion::V5;
        let mut page = Vec::new();
        page.extend(7_i32.to_le_bytes());
        for value in [210.0_f64, 297.0, 10.0, 11.0, 12.0, 13.0] {
            page.extend(value.to_le_bytes());
        }
        page.extend(utf16_bytes("witness-printer"));
        page.extend([0xde, 0xad, 0xbe, 0xef]);

        let mut body = vec![0x13];
        body.extend(1_i32.to_le_bytes());
        body.extend(210.0_f64.to_le_bytes());
        body.extend(297.0_f64.to_le_bytes());
        body.extend([0; 16]);
        for _ in 0..6 {
            body.extend(0.0_f64.to_le_bytes());
        }
        body.extend([0; 16]);
        body.extend(anonymous_chunk(archive, 7, &page));
        body.push(1);
        body.extend([0xa1, 0xb2, 0xc3]);

        let (value, _) =
            parse_attributes(&body, 0..body.len(), archive, 1.0).expect("view attributes");
        assert_eq!(value.view_type, 1);
        assert!(value.projection_locked);
        let page = value.page_settings.expect("page settings");
        assert_eq!(page.page_number, 7);
        assert_eq!(page.width_mm, 210.0);
        assert_eq!(page.height_mm, 297.0);
        assert_eq!(page.margins_mm, [10.0, 11.0, 12.0, 13.0]);
        assert_eq!(page.printer_name, "witness-printer");
    }

    #[test]
    fn view_attributes_crc_excludes_nested_children_and_reports_mismatch() {
        let archive = ArchiveVersion::V5;
        let mut page = Vec::new();
        page.extend(7_i32.to_le_bytes());
        for value in [210.0_f64, 297.0, 10.0, 11.0, 12.0, 13.0] {
            page.extend(value.to_le_bytes());
        }
        page.extend(utf16_bytes("attributes-witness"));

        let mut attributes_body = vec![0x12];
        attributes_body.extend(1_i32.to_le_bytes());
        attributes_body.extend(210.0_f64.to_le_bytes());
        attributes_body.extend(297.0_f64.to_le_bytes());
        attributes_body.extend([0; 16]);
        for _ in 0..6 {
            attributes_body.extend(0.0_f64.to_le_bytes());
        }
        attributes_body.extend([0; 16]);
        let page_start = attributes_body.len();
        attributes_body.extend(anonymous_chunk(archive, 0, &page));
        let page_range = page_start..attributes_body.len();
        attributes_body.extend([0xaa, 0xbb]);
        let attributes = crc_chunk_excluding(
            archive,
            super::VIEW_ATTRIBUTES,
            &attributes_body,
            std::slice::from_ref(&page_range),
        );

        let make_view = |attributes: &[u8]| {
            let end_marker = short_chunk(archive, super::TCODE_ENDOFTABLE, 0);
            let mut view_body = attributes.to_vec();
            view_body.extend(end_marker);
            let view_body_range = 0..view_body.len();
            crc_chunk_excluding(
                archive,
                super::VIEW_RECORD,
                &view_body,
                std::slice::from_ref(&view_body_range),
            )
        };

        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(make_view(&attributes));
        let record = Record {
            typecode: super::NAMED_VIEWS,
            range: 0..body.len(),
            body: 0..body.len(),
            short: false,
            value: 0,
        };
        let (views, losses) = parse_list(&body, &record, archive, 1.0, "named");
        assert_eq!(views.len(), 1);
        assert!(losses.is_empty());

        let mut corrupted_attributes = attributes;
        let crc_offset = corrupted_attributes.len() - 1;
        corrupted_attributes[crc_offset] ^= 1;
        let mut corrupted_body = 1_i32.to_le_bytes().to_vec();
        corrupted_body.extend(make_view(&corrupted_attributes));
        let record = Record {
            typecode: super::NAMED_VIEWS,
            range: 0..corrupted_body.len(),
            body: 0..corrupted_body.len(),
            short: false,
            value: 0,
        };
        let (views, losses) = parse_list(&corrupted_body, &record, archive, 1.0, "named");
        assert_eq!(views.len(), 1);
        assert_eq!(losses.len(), 1);
        assert!(losses[0].message.contains("0x20008c3b"));
    }

    #[test]
    fn view_image_child_crc_excludes_file_reference_and_reports_mismatch() {
        let archive = ArchiveVersion::V6;
        let mut trace_body = vec![0x14];
        trace_body.extend(utf16_bytes("trace-witness.png"));
        trace_body.extend(42.0_f64.to_le_bytes());
        trace_body.extend(24.0_f64.to_le_bytes());
        serialized_plane(&mut trace_body);
        trace_body.extend([0, 1, 1]);
        let trace_reference_start = trace_body.len();
        trace_body.extend(file_reference(archive, "/trace/source.png", "source.png"));
        let trace_reference_range = trace_reference_start..trace_body.len();
        let trace = crc_chunk_excluding(
            archive,
            super::VIEW_TRACE_IMAGE,
            &trace_body,
            std::slice::from_ref(&trace_reference_range),
        );

        let mut wallpaper_body = vec![0x12];
        wallpaper_body.extend(utf16_bytes("wallpaper-witness.png"));
        wallpaper_body.extend([1, 0]);
        let wallpaper_reference_start = wallpaper_body.len();
        wallpaper_body.extend(file_reference(
            archive,
            "/wallpaper/source.png",
            "source.png",
        ));
        let wallpaper_reference_range = wallpaper_reference_start..wallpaper_body.len();
        let wallpaper = crc_chunk_excluding(
            archive,
            super::VIEW_WALLPAPER_V3,
            &wallpaper_body,
            std::slice::from_ref(&wallpaper_reference_range),
        );

        let make_view = |trace: &[u8], wallpaper: &[u8]| {
            let mut view_body = trace.to_vec();
            let wallpaper_start = view_body.len();
            view_body.extend(wallpaper);
            let wallpaper_range = wallpaper_start..view_body.len();
            let end_start = view_body.len();
            view_body.extend(short_chunk(archive, super::TCODE_ENDOFTABLE, 0));
            let end_range = end_start..view_body.len();
            crc_chunk_excluding(
                archive,
                super::VIEW_RECORD,
                &view_body,
                &[0..trace.len(), wallpaper_range, end_range],
            )
        };
        let parse = |view: Vec<u8>| {
            let mut body = 1_i32.to_le_bytes().to_vec();
            body.extend(view);
            let record = Record {
                typecode: super::NAMED_VIEWS,
                range: 0..body.len(),
                body: 0..body.len(),
                short: false,
                value: 0,
            };
            parse_list(&body, &record, archive, 1.0, "named")
        };

        let (views, losses) = parse(make_view(&trace, &wallpaper));
        assert_eq!(views.len(), 1);
        assert!(losses.is_empty());

        let mut corrupted_trace = trace.clone();
        let trace_crc_offset = corrupted_trace.len() - 1;
        corrupted_trace[trace_crc_offset] ^= 1;
        let (views, losses) = parse(make_view(&corrupted_trace, &wallpaper));
        assert_eq!(views.len(), 1);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::RhinoLossCode::IntegrityFailure.kind()
        );
        assert!(losses[0].message.contains("0x2000863b"));

        let mut corrupted_wallpaper = wallpaper.clone();
        let wallpaper_crc_offset = corrupted_wallpaper.len() - 1;
        corrupted_wallpaper[wallpaper_crc_offset] ^= 1;
        let (views, losses) = parse(make_view(&trace, &corrupted_wallpaper));
        assert_eq!(views.len(), 1);
        assert_eq!(losses.len(), 1);
        assert!(losses[0].message.contains("0x2000874b"));
    }

    #[test]
    fn viewport_userdata_crc_excludes_class_children_and_reports_mismatch() {
        let archive = ArchiveVersion::V5;
        let userdata = class_userdata_v2_with_direct_payload(
            archive,
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            [
                33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48,
            ],
            50,
            202_608_010,
            &[
                2_i32.to_le_bytes().as_slice(),
                0_i32.to_le_bytes().as_slice(),
            ]
            .concat(),
        );
        let userdata_range = 0..userdata.len();
        let class_end = short_chunk(archive, 0x8002_7fff, 0);
        let class_end_range = userdata.len()..userdata.len() + class_end.len();
        let mut userdata_body = userdata;
        userdata_body.extend(class_end);
        userdata_body.extend([0xde, 0xad]);
        let viewport_userdata = crc_chunk_excluding(
            archive,
            super::VIEW_VIEWPORT_USERDATA,
            &userdata_body,
            &[userdata_range, class_end_range],
        );

        let make_view = |viewport_userdata: &[u8]| {
            let end_marker = short_chunk(archive, super::TCODE_ENDOFTABLE, 0);
            let mut view_body = viewport_userdata.to_vec();
            view_body.extend(end_marker);
            let view_body_range = 0..view_body.len();
            crc_chunk_excluding(
                archive,
                super::VIEW_RECORD,
                &view_body,
                std::slice::from_ref(&view_body_range),
            )
        };

        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(make_view(&viewport_userdata));
        let record = Record {
            typecode: super::NAMED_VIEWS,
            range: 0..body.len(),
            body: 0..body.len(),
            short: false,
            value: 0,
        };
        let (views, losses) = parse_list(&body, &record, archive, 1.0, "named");
        assert_eq!(views.len(), 1);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::RhinoLossCode::ViewportUserdataDropped.kind()
        );

        let mut corrupted_userdata = viewport_userdata;
        let crc_offset = corrupted_userdata.len() - 1;
        corrupted_userdata[crc_offset] ^= 1;
        let mut corrupted_body = 1_i32.to_le_bytes().to_vec();
        corrupted_body.extend(make_view(&corrupted_userdata));
        let record = Record {
            typecode: super::NAMED_VIEWS,
            range: 0..corrupted_body.len(),
            body: 0..corrupted_body.len(),
            short: false,
            value: 0,
        };
        let (views, losses) = parse_list(&corrupted_body, &record, archive, 1.0, "named");
        assert_eq!(views.len(), 1);
        assert!(losses.iter().any(|loss| {
            loss.code == crate::loss::RhinoLossCode::ViewportUserdataDropped.kind()
        }));
        assert!(losses
            .iter()
            .any(|loss| loss.message.contains("0x20008d3b")));
    }
}
