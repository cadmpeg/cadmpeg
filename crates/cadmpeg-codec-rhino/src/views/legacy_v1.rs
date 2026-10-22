// SPDX-License-Identifier: Apache-2.0
//! Version 1 viewport, named-view, and named construction-plane records.

use cadmpeg_ir::scalar::FiniteReal;
use serde::Serialize;

use crate::chunks::{
    chunk_at, ArchiveVersion, BoundedReader, Chunk, FramingError, TCODE_ENDOFTABLE,
};
use crate::settings::MillimeterScale;
use crate::wire::scaled_coordinate;

pub(crate) const TCODE_NAMED_CPLANE: u32 = 0x0200_0004;
pub(crate) const TCODE_NAMED_VIEW: u32 = 0x0200_0005;
pub(crate) const TCODE_VIEWPORT: u32 = 0x0200_0006;
const TCODE_NAME: u32 = 0x0200_0001;
const TCODE_VIEW: u32 = 0x0200_0002;
const TCODE_CPLANE: u32 = 0x0200_0003;
const TCODE_SHOWGRID: u32 = 0x8200_0007;
const TCODE_SHOWGRIDAXES: u32 = 0x8200_0008;
const TCODE_SHOWWORLDAXES: u32 = 0x8200_0009;
const TCODE_VIEWPORT_POSITION: u32 = 0x0200_000a;
const TCODE_VIEWPORT_TRACEINFO: u32 = 0x0200_000b;
const TCODE_SNAPSIZE: u32 = 0x0200_000c;
const TCODE_NEAR_CLIP_PLANE: u32 = 0x0200_000d;
const TCODE_MAXIMIZED_VIEWPORT: u32 = 0x8200_0011;
const TCODE_VIEWPORT_WALLPAPER: u32 = 0x0200_0012;
const TCODE_VIEWPORT_V1_DISPLAYMODE: u32 = 0x8200_0015;
/// Projection selector of a perspective V1 view; every other value is parallel.
const V1_PERSPECTIVE: i32 = 2;
/// Reader replacement for a camera distance outside `(0, DBL_MAX / 2)`.
const DEFAULT_CAMERA_DISTANCE: f64 = 100.0;
/// Reader replacement for a view size outside `(0, DBL_MAX / 2)`.
const DEFAULT_VIEW_SIZE: f64 = 0.125;

/// The kind of one V1 presentation record.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum V1PresentationKind {
    Viewport,
    NamedView,
    NamedConstructionPlane,
}

/// A V1 construction plane: frame, grid spacing, and grid line counts.
#[derive(Debug, Serialize)]
struct V1ConstructionPlane {
    origin_mm: [FiniteReal; 3],
    x_axis: [FiniteReal; 3],
    y_axis: [FiniteReal; 3],
    grid_spacing_mm: FiniteReal,
    grid_line_count: i32,
    thick_line_frequency: i32,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum V1Projection {
    Parallel,
    Perspective,
}

/// A V1 Rhino view: projection, target, three view angles, size, and distance.
#[derive(Debug, Serialize)]
struct V1View {
    projection: V1Projection,
    projection_value: i32,
    valid: i32,
    target_mm: [FiniteReal; 3],
    angles_radians: [FiniteReal; 3],
    view_size_mm: FiniteReal,
    camera_distance_mm: FiniteReal,
}

#[derive(Debug, Serialize)]
struct V1TraceImage {
    origin_mm: [FiniteReal; 3],
    x_axis: [FiniteReal; 3],
    y_axis: [FiniteReal; 3],
    width_mm: FiniteReal,
    height_mm: FiniteReal,
    file_path: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum V1DisplayMode {
    Wireframe,
    Shaded,
}

/// One V1 presentation record and the child fields its reader assigns.
#[derive(Debug, Serialize)]
pub(crate) struct V1PresentationRecord {
    id: String,
    source_offset: u64,
    kind: V1PresentationKind,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    construction_plane: Option<V1ConstructionPlane>,
    #[serde(skip_serializing_if = "Option::is_none")]
    view: Option<V1View>,
    #[serde(skip_serializing_if = "Option::is_none")]
    show_construction_grid: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    show_construction_axes: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    show_world_axes: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    window_position: Option<[FiniteReal; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace_image: Option<V1TraceImage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wallpaper_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    maximized: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_mode: Option<V1DisplayMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    near_clip_distance_mm: Option<FiniteReal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    snap_size_mm: Option<FiniteReal>,
}

/// The presentation record kind of a V1 top-level typecode.
pub(crate) fn kind(typecode: u32) -> Option<V1PresentationKind> {
    match typecode {
        TCODE_VIEWPORT => Some(V1PresentationKind::Viewport),
        TCODE_NAMED_VIEW => Some(V1PresentationKind::NamedView),
        TCODE_NAMED_CPLANE => Some(V1PresentationKind::NamedConstructionPlane),
        _ => None,
    }
}

fn finite(reader: &mut BoundedReader<'_>, label: &str) -> Result<FiniteReal, FramingError> {
    let offset = reader.position();
    FiniteReal::new(reader.f64()?)
        .ok_or_else(|| FramingError::structural(offset, format!("V1 {label} is not finite")))
}

fn length(
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    label: &str,
) -> Result<FiniteReal, FramingError> {
    let offset = reader.position();
    let value = reader.f64()?;
    scaled_coordinate(value, scale)
        .ok_or_else(|| FramingError::structural(offset, format!("V1 {label} is invalid")))
}

fn point(
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    label: &str,
) -> Result<[FiniteReal; 3], FramingError> {
    Ok([
        length(reader, scale, label)?,
        length(reader, scale, label)?,
        length(reader, scale, label)?,
    ])
}

fn vector(reader: &mut BoundedReader<'_>, label: &str) -> Result<[FiniteReal; 3], FramingError> {
    Ok([
        finite(reader, label)?,
        finite(reader, label)?,
        finite(reader, label)?,
    ])
}

/// A V1 `TCODE_NAME` string: an `i32` byte count and that many single-byte
/// characters, ending at the first NUL.
fn name(reader: &mut BoundedReader<'_>) -> Result<String, FramingError> {
    let count_offset = reader.position();
    let count = reader.i32()?;
    if count <= 0 {
        return Ok(String::new());
    }
    let count = usize::try_from(count).map_err(|_| FramingError::Overflow {
        offset: count_offset,
    })?;
    let bytes = reader.take(count)?;
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    Ok(bytes[..end].iter().copied().map(char::from).collect())
}

fn construction_plane(
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
) -> Result<V1ConstructionPlane, FramingError> {
    Ok(V1ConstructionPlane {
        origin_mm: point(reader, scale, "construction-plane origin")?,
        x_axis: vector(reader, "construction-plane x axis")?,
        y_axis: vector(reader, "construction-plane y axis")?,
        grid_spacing_mm: length(reader, scale, "grid spacing")?,
        grid_line_count: reader.i32()?,
        thick_line_frequency: reader.i32()?,
    })
}

/// Replaces a length outside `(0, DBL_MAX / 2)` with the reader default.
fn bounded_length(value: f64, default: f64) -> f64 {
    if value <= 0.0 || value >= f64::MAX / 2.0 {
        default
    } else {
        value
    }
}

fn view(reader: &mut BoundedReader<'_>, scale: MillimeterScale) -> Result<V1View, FramingError> {
    let projection_value = reader.i32()?;
    let valid = reader.i32()?;
    let target_mm = point(reader, scale, "view target")?;
    let angles_radians = [
        finite(reader, "view angle")?,
        finite(reader, "view angle")?,
        finite(reader, "view angle")?,
    ];
    let size_offset = reader.position();
    let view_size = bounded_length(reader.f64()?, DEFAULT_VIEW_SIZE);
    let distance_offset = reader.position();
    let camera_distance = bounded_length(reader.f64()?, DEFAULT_CAMERA_DISTANCE);
    Ok(V1View {
        projection: if projection_value == V1_PERSPECTIVE {
            V1Projection::Perspective
        } else {
            V1Projection::Parallel
        },
        projection_value,
        valid,
        target_mm,
        angles_radians,
        view_size_mm: scaled_coordinate(view_size, scale)
            .ok_or_else(|| FramingError::structural(size_offset, "V1 view size is invalid"))?,
        camera_distance_mm: scaled_coordinate(camera_distance, scale).ok_or_else(|| {
            FramingError::structural(distance_offset, "V1 camera distance is invalid")
        })?,
    })
}

/// Parses one V1 viewport, named view, or named construction plane. The
/// record is a sequence of child chunks that ends at `TCODE_ENDOFTABLE`;
/// unrecognized children carry no field.
pub(crate) fn parse(
    data: &[u8],
    record: &Chunk,
    kind: V1PresentationKind,
    scale: MillimeterScale,
) -> Result<V1PresentationRecord, FramingError> {
    let body = record.body();
    let viewport = matches!(kind, V1PresentationKind::Viewport);
    let named_view = matches!(kind, V1PresentationKind::NamedView);
    let mut value = V1PresentationRecord {
        id: format!("rhino:document:v1_presentation#{}", record.header_start),
        source_offset: cadmpeg_core::decode::u64_from_index(record.header_start),
        kind,
        name: String::new(),
        construction_plane: None,
        view: None,
        show_construction_grid: None,
        show_construction_axes: None,
        show_world_axes: None,
        window_position: None,
        trace_image: None,
        wallpaper_path: None,
        maximized: None,
        display_mode: None,
        near_clip_distance_mm: None,
        snap_size_mm: None,
    };
    let mut offset = body.start;
    loop {
        let child = chunk_at(data, offset, body.end, ArchiveVersion::V1, false)?;
        let mut reader = BoundedReader::new(data, child.body().start, child.body().end)?;
        let short = child.short();
        match child.typecode {
            TCODE_ENDOFTABLE => break,
            TCODE_NAME if !short => value.name = name(&mut reader)?,
            TCODE_CPLANE if !short => {
                value.construction_plane = Some(construction_plane(&mut reader, scale)?);
            }
            TCODE_VIEW if !short && !matches!(kind, V1PresentationKind::NamedConstructionPlane) => {
                value.view = Some(view(&mut reader, scale)?);
            }
            TCODE_SHOWGRID if viewport || named_view => {
                value.show_construction_grid = Some(child.value()? != 0);
            }
            TCODE_SHOWGRIDAXES if viewport || named_view => {
                value.show_construction_axes = Some(child.value()? != 0);
            }
            TCODE_SHOWWORLDAXES if viewport || named_view => {
                value.show_world_axes = Some(child.value()? != 0);
            }
            TCODE_NEAR_CLIP_PLANE if viewport && !short => {
                value.near_clip_distance_mm = Some(length(&mut reader, scale, "near clip")?);
            }
            TCODE_SNAPSIZE if viewport && !short => {
                value.snap_size_mm = Some(length(&mut reader, scale, "snap size")?);
            }
            TCODE_VIEWPORT_POSITION if viewport && !short => {
                value.window_position = Some([
                    finite(&mut reader, "window left")?,
                    finite(&mut reader, "window top")?,
                    finite(&mut reader, "window right")?,
                    finite(&mut reader, "window bottom")?,
                ]);
            }
            TCODE_VIEWPORT_TRACEINFO if viewport && !short => {
                value.trace_image = Some(V1TraceImage {
                    origin_mm: point(&mut reader, scale, "trace-image origin")?,
                    x_axis: vector(&mut reader, "trace-image x axis")?,
                    y_axis: vector(&mut reader, "trace-image y axis")?,
                    width_mm: length(&mut reader, scale, "trace-image width")?,
                    height_mm: length(&mut reader, scale, "trace-image height")?,
                    file_path: name(&mut reader)?,
                });
            }
            TCODE_VIEWPORT_WALLPAPER if viewport && !short => {
                value.wallpaper_path = Some(name(&mut reader)?);
            }
            TCODE_MAXIMIZED_VIEWPORT if viewport && child.value()? != 0 => {
                value.maximized = Some(true);
            }
            TCODE_VIEWPORT_V1_DISPLAYMODE if viewport && value.display_mode.is_none() => {
                value.display_mode = match child.value()? {
                    0 => Some(V1DisplayMode::Wireframe),
                    1 => Some(V1DisplayMode::Shaded),
                    _ => None,
                };
            }
            _ => {}
        }
        offset = child.next_offset();
        if offset >= body.end {
            return Err(FramingError::structural(
                body.end,
                "V1 presentation record has no end-of-table child",
            ));
        }
    }
    Ok(value)
}

#[cfg(test)]
mod tests;
