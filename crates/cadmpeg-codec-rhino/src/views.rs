// SPDX-License-Identifier: Apache-2.0
//! Saved and active Rhino view presentation records.

use cadmpeg_ir::document::CadIr;
use serde::Serialize;

use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader, FramingError, TCODE_ENDOFTABLE};
use crate::container::{Record, Scan};
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
    attributes: Option<ViewAttributes>,
    construction_plane: Option<ConstructionPlane>,
    viewport: Option<Viewport>,
    trace_image: Option<TraceImage>,
    wallpaper: Option<Wallpaper>,
    children: Vec<ViewChild>,
    parse_warnings: Vec<String>,
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
) -> Result<ImageReference, FramingError> {
    let value = crate::instances::file_reference(data, reader, archive, &mut Vec::new())?;
    Ok(ImageReference {
        full_path: value.full_path,
        relative_path: value.relative_path,
        content_sha1: hex(&value.content_hash.content_sha1),
        embedded_file_uuid: value.embedded_file_id.map(|id| id.to_string()),
    })
}

fn parse_trace_image(
    data: &[u8],
    body: std::ops::Range<usize>,
    archive: ArchiveVersion,
    scale: f64,
) -> Result<TraceImage, FramingError> {
    let mut reader = BoundedReader::new(data, body.start, body.end)?;
    let packed = reader.u8()?;
    let minor = packed & 0x0f;
    if packed >> 4 != 1 || minor > 4 {
        return Err(structural(body.start, "trace-image version is unsupported"));
    }
    let legacy_file_path = utf16(&mut reader)?;
    let width_mm = scaled_coordinate(reader.f64()?, scale)
        .ok_or_else(|| structural(reader.position() - 8, "trace width is invalid"))?;
    let height_mm = scaled_coordinate(reader.f64()?, scale)
        .ok_or_else(|| structural(reader.position() - 8, "trace height is invalid"))?;
    let plane = scaled_plane(plane(&mut reader)?, scale, body.start)?;
    let grayscale = minor < 1 || reader.bool()?;
    let hidden = minor >= 2 && reader.bool()?;
    let filtered = minor >= 3 && reader.bool()?;
    let file_reference = if minor >= 4 {
        Some(image_reference(data, &mut reader, archive)?)
    } else {
        None
    };
    if reader.remaining() != 0 {
        return Err(structural(
            reader.position(),
            "trace image has trailing bytes",
        ));
    }
    Ok(TraceImage {
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
    })
}

fn parse_wallpaper(
    data: &[u8],
    body: std::ops::Range<usize>,
    archive: ArchiveVersion,
) -> Result<Wallpaper, FramingError> {
    let mut reader = BoundedReader::new(data, body.start, body.end)?;
    let packed = reader.u8()?;
    let minor = packed & 0x0f;
    if packed >> 4 != 1 || minor > 2 {
        return Err(structural(body.start, "wallpaper version is unsupported"));
    }
    let legacy_file_path = utf16(&mut reader)?;
    let grayscale = reader.bool()?;
    let hidden = minor >= 1 && reader.bool()?;
    let file_reference = if minor >= 2 {
        Some(image_reference(data, &mut reader, archive)?)
    } else {
        None
    };
    if reader.remaining() != 0 {
        return Err(structural(
            reader.position(),
            "wallpaper has trailing bytes",
        ));
    }
    Ok(Wallpaper {
        legacy_file_path,
        grayscale,
        hidden,
        file_reference,
    })
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
    archive: ArchiveVersion,
    scale: f64,
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
            return Err(structural(
                reader.position(),
                "page-settings wrapper is invalid",
            ));
        }
        let mut page = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
        if (page.i32()?, page.i32()?) != (1, 0) {
            return Err(structural(
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
            return Err(structural(page.position(), "page setting is not finite"));
        }
        let printer_name = utf16(&mut page)?;
        if page.remaining() != 0 {
            return Err(structural(
                page.position(),
                "page settings have trailing bytes",
            ));
        }
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
            .ok_or_else(|| structural(count_offset, "clipping-plane count is invalid"))?;
        for _ in 0..count {
            let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
            if chunk.typecode != 0x4000_8000 || chunk.short {
                return Err(structural(
                    reader.position(),
                    "clipping-plane wrapper is invalid",
                ));
            }
            let mut plane = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
            let (major, minor) = (plane.i32()?, plane.i32()?);
            if major != 1 || !(0..=3).contains(&minor) {
                return Err(structural(
                    plane.position(),
                    "clipping-plane version is unsupported",
                ));
            }
            let mut equation = [plane.f64()?, plane.f64()?, plane.f64()?, plane.f64()?];
            if !equation.iter().all(|value| value.is_finite()) {
                return Err(structural(
                    plane.position() - 32,
                    "clipping equation is invalid",
                ));
            }
            equation[3] = scaled_coordinate(equation[3], scale)
                .ok_or_else(|| structural(plane.position() - 8, "clipping equation is invalid"))?;
            let id = uuid(&mut plane)?;
            let enabled = plane.bool()?;
            let depth =
                if minor >= 1 {
                    Some(scaled_coordinate(plane.f64()?, scale).ok_or_else(|| {
                        structural(plane.position() - 8, "clipping depth is invalid")
                    })?)
                } else {
                    None
                };
            let depth_enabled = if minor >= 3 {
                plane.bool()?
            } else {
                depth.is_some_and(|value| value >= 0.0)
            };
            if plane.remaining() != 0 {
                return Err(structural(
                    plane.position(),
                    "clipping plane has trailing bytes",
                ));
            }
            result.clipping_planes.push(ClippingPlane {
                equation_mm: equation,
                plane_uuid: (!id.is_nil()).then(|| id.to_string()),
                enabled,
                depth_mm: depth,
                depth_enabled,
            });
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
        result.focal_blur_distance_mm = Some(
            scaled_coordinate(reader.f64()?, scale)
                .ok_or_else(|| structural(reader.position() - 8, "focal distance is invalid"))?,
        );
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
    if reader.remaining() != 0 {
        return Err(structural(
            reader.position(),
            "view attributes have trailing bytes",
        ));
    }
    Ok(result)
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
    let mut attributes_detail = None;
    let mut construction_plane = None;
    let mut viewport = None;
    let mut trace_image = None;
    let mut wallpaper = None;
    let mut children = Vec::new();
    let mut parse_warnings = Vec::new();
    let mut terminated = false;
    while offset < record.body.end {
        let child = chunk_at(data, offset, record.body.end, archive, false)?;
        children.push(ViewChild {
            typecode: format!("{:#010x}", child.typecode),
            kind: child_kind(child.typecode),
            source_offset: offset as u64,
            byte_len: (child.next_offset - offset) as u64,
            sha256: cadmpeg_ir::wire::hash::sha256_hex(&data[offset..child.next_offset]),
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
                trace_image = Some(parse_trace_image(data, child.body.clone(), archive, scale)?);
            }
            VIEW_WALLPAPER if !child.short => {
                let mut reader = BoundedReader::new(data, child.body.start, child.body.end)?;
                let path = utf16(&mut reader)?;
                if reader.remaining() != 0 {
                    return Err(structural(
                        reader.position(),
                        "wallpaper path has trailing bytes",
                    ));
                }
                wallpaper = Some(Wallpaper {
                    legacy_file_path: path,
                    grayscale: true,
                    hidden: false,
                    file_reference: None,
                });
            }
            VIEW_WALLPAPER_V3 if !child.short => {
                wallpaper = Some(parse_wallpaper(data, child.body.clone(), archive)?);
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
                let attributes = parse_attributes(data, child.body.clone(), archive, scale)?;
                view_type = Some(attributes.view_type);
                page_width = Some(attributes.width);
                page_height = Some(attributes.height);
                display_mode_uuid.clone_from(&attributes.display);
                attributes_version = Some(attributes.version);
                attributes_detail = Some(attributes);
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
        attributes: attributes_detail,
        construction_plane,
        viewport,
        trace_image,
        wallpaper,
        children,
        parse_warnings,
    })
}

fn parse_list(
    data: &[u8],
    record: &Record,
    archive: ArchiveVersion,
    scale: f64,
    kind: &'static str,
) -> Vec<ViewRecord> {
    let Ok(mut reader) = BoundedReader::new(data, record.body.start, record.body.end) else {
        return Vec::new();
    };
    let Ok(signed_count) = reader.i32() else {
        return Vec::new();
    };
    let Ok(count) = usize::try_from(signed_count) else {
        return Vec::new();
    };
    if count > 1 << 16 {
        return Vec::new();
    }
    let mut views = Vec::new();
    for index in 0..count {
        let Ok(view) = chunk_at(data, reader.position(), reader.end(), archive, false) else {
            break;
        };
        let next = view.next_offset;
        if view.typecode == VIEW_RECORD && !view.short {
            views.push(
                parse_view(data, &view, archive, scale, kind, index).unwrap_or_else(|error| {
                    ViewRecord {
                        id: format!("rhino:document:view#{kind}-{index:04}"),
                        source_offset: view.header_start as u64,
                        list_kind: kind,
                        list_index: index,
                        name: String::new(),
                        target_millimeters: None,
                        show_construction_grid: true,
                        show_construction_axes: true,
                        show_world_axes: true,
                        legacy_display_mode: None,
                        view_type: None,
                        page_width_mm: None,
                        page_height_mm: None,
                        display_mode_uuid: None,
                        attributes_version: None,
                        attributes: None,
                        construction_plane: None,
                        viewport: None,
                        trace_image: None,
                        wallpaper: None,
                        children: vec![ViewChild {
                            typecode: format!("{:#010x}", view.typecode),
                            kind: "degraded view record",
                            source_offset: view.header_start as u64,
                            byte_len: (view.next_offset - view.header_start) as u64,
                            sha256: cadmpeg_ir::wire::hash::sha256_hex(
                                &data[view.header_start..view.next_offset],
                            ),
                        }],
                        parse_warnings: vec![format!("view retained: {error}")],
                    }
                }),
            );
        }
        if reader.skip(next - reader.position()).is_err() {
            break;
        }
    }
    views
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
        .ok_or_else(|| structural(count_offset, "named construction-plane count is invalid"))?;
    let mut values = Vec::new();
    for index in 0..count {
        let chunk = chunk_at(data, reader.position(), reader.end(), archive, false)?;
        if chunk.typecode != VIEW_CPLANE || chunk.short {
            return Err(structural(
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
    if reader.remaining() != 0 {
        return Err(structural(
            reader.position(),
            "named construction-plane list has trailing bytes",
        ));
    }
    Ok(values)
}

/// Installs saved and active view records with complete child accounting.
pub(crate) fn install(scan: &Scan<'_>, ir: &mut CadIr) {
    let scale = scan
        .metadata
        .settings
        .units
        .as_ref()
        .and_then(|value| value.millimeters_per_unit)
        .unwrap_or(1.0);
    let mut views = Vec::new();
    let mut cplanes = Vec::new();
    for table in &scan.tables {
        if table.typecode & !0x0000_8000 != SETTINGS {
            continue;
        }
        for record in &table.records {
            if record.typecode == NAMED_CPLANES {
                if let Ok(values) = parse_named_cplanes(scan.data, record, scan.archive, scale) {
                    cplanes.extend(values);
                }
            }
            if record.typecode == NAMED_VIEWS {
                views.extend(parse_list(scan.data, record, scan.archive, scale, "named"));
            }
            if record.typecode == ACTIVE_VIEWS {
                views.extend(parse_list(scan.data, record, scan.archive, scale, "active"));
            }
        }
    }
    let namespace = ir.native.namespace_mut("rhino");
    namespace.version = namespace.version.max(2);
    namespace
        .set_arena("views", &views)
        .expect("Rhino views serialize");
    namespace
        .set_arena("construction_planes", &cplanes)
        .expect("Rhino construction planes serialize");
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
