// SPDX-License-Identifier: Apache-2.0
//! Modern Rhino dimension payload decoding.

use std::ops::Range;

use crate::chunks::{checked_count_bytes, chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::objects::{parse_class_wrapper, UserdataDescriptor};
use crate::settings::{plane, utf16, Plane};
use crate::wire::{scaled_coordinate, Uuid};

const ANONYMOUS: u32 = 0x4000_8000;
pub(crate) const V5_DIM_EXTRA: Uuid = Uuid::from_canonical([
    0x8a, 0xd5, 0xb9, 0xfc, 0x0d, 0x5c, 0x47, 0xfb, 0xad, 0xfd, 0x74, 0xc2, 0x8b, 0x6f, 0x66, 0x1e,
]);
pub(crate) const V5_ANGULAR_EXTRA: Uuid = Uuid::from_canonical([
    0xa6, 0x8b, 0x15, 0x1f, 0xc7, 0x78, 0x4a, 0x6e, 0xbc, 0xb4, 0x23, 0xdd, 0xd1, 0x83, 0x56, 0x77,
]);
pub(crate) const LINEAR: Uuid = Uuid::from_canonical([
    0xe5, 0x50, 0x88, 0x2b, 0xf4, 0x4d, 0x41, 0x54, 0xa1, 0xef, 0x6e, 0x50, 0xcb, 0xbb, 0xf5, 0x43,
]);
pub(crate) const ANGULAR: Uuid = Uuid::from_canonical([
    0xd4, 0x17, 0x78, 0x6b, 0xf6, 0xcd, 0x4f, 0x12, 0x9e, 0x1f, 0x06, 0x3f, 0x41, 0x4d, 0xbe, 0xb6,
]);
pub(crate) const RADIAL: Uuid = Uuid::from_canonical([
    0xfc, 0x74, 0x9c, 0x2f, 0x4c, 0x00, 0x41, 0xfd, 0x98, 0x40, 0x26, 0xd9, 0x4f, 0x04, 0x7a, 0xd3,
]);
pub(crate) const V5_LINEAR: Uuid = Uuid::from_canonical([
    0xbd, 0x57, 0xf3, 0x3b, 0xa1, 0xb2, 0x46, 0xe9, 0x9c, 0x6e, 0xaf, 0x09, 0xd3, 0x0f, 0xfd, 0xde,
]);
pub(crate) const V5_RADIAL: Uuid = Uuid::from_canonical([
    0xb2, 0xb6, 0x83, 0xfc, 0x79, 0x64, 0x4e, 0x96, 0xb1, 0xf9, 0x9b, 0x35, 0x6a, 0x76, 0xb0, 0x8b,
]);
pub(crate) const V5_ANGULAR: Uuid = Uuid::from_canonical([
    0x84, 0x1b, 0xc4, 0x0b, 0xa9, 0x71, 0x4a, 0x8e, 0x94, 0xe5, 0xbb, 0xa2, 0x6d, 0x67, 0x34, 0x8e,
]);
pub(crate) const ORDINATE: Uuid = Uuid::from_canonical([
    0x03, 0x12, 0x48, 0x28, 0x4c, 0x9b, 0x4d, 0x28, 0x9a, 0x82, 0x66, 0x4d, 0xdd, 0xe7, 0xa1, 0x4f,
]);
pub(crate) const V5_ORDINATE: Uuid = Uuid::from_canonical([
    0xc8, 0x28, 0x8d, 0x69, 0x5b, 0xd8, 0x4f, 0x50, 0x9b, 0xaf, 0x52, 0x5a, 0x00, 0x86, 0xb0, 0xc3,
]);
pub(crate) const CENTERMARK: Uuid = Uuid::from_canonical([
    0xd4, 0x67, 0x67, 0xba, 0x7e, 0x8f, 0x4d, 0x9d, 0x9a, 0x92, 0x66, 0x05, 0x02, 0x19, 0xa5, 0xb9,
]);
pub(crate) const V2_ANNOTATION: Uuid = Uuid::from_canonical([
    0xab, 0xaf, 0x58, 0x73, 0x41, 0x45, 0x11, 0xd4, 0x80, 0x0f, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const V2_LINEAR: Uuid = Uuid::from_canonical([
    0x5d, 0xe6, 0xb2, 0x0d, 0x48, 0x6b, 0x11, 0xd4, 0x80, 0x14, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const V2_RADIAL: Uuid = Uuid::from_canonical([
    0x5d, 0xe6, 0xb2, 0x0e, 0x48, 0x6b, 0x11, 0xd4, 0x80, 0x14, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const V2_ANGULAR: Uuid = Uuid::from_canonical([
    0x5d, 0xe6, 0xb2, 0x0f, 0x48, 0x6b, 0x11, 0xd4, 0x80, 0x14, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const V2_TEXT_OBJECT: Uuid = Uuid::from_canonical([
    0x5d, 0xe6, 0xb2, 0x10, 0x48, 0x6b, 0x11, 0xd4, 0x80, 0x14, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const V2_LEADER: Uuid = Uuid::from_canonical([
    0x5d, 0xe6, 0xb2, 0x11, 0x48, 0x6b, 0x11, 0xd4, 0x80, 0x14, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const V2_REALLY_BIG_NUMBER: f64 = 1.0e150;

/// Dimension family and defining plane-space geometry.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Definition {
    Linear {
        definition_point: [f64; 2],
        dimension_line_point: [f64; 2],
    },
    Angular {
        first_direction: [f64; 2],
        second_direction: [f64; 2],
        first_extension_offset: f64,
        second_extension_offset: f64,
        dimension_line_point: [f64; 2],
    },
    Radial {
        radius_point: [f64; 2],
        dimension_line_point: [f64; 2],
        diameter: bool,
    },
    Ordinate {
        definition_point: [f64; 2],
        leader_point: [f64; 2],
        measured_direction: i32,
        kink_offsets: [f64; 2],
    },
    CenterMark {
        radius: f64,
    },
}

/// Style and V2 payload exclusive to one dimension family.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DimensionFamily {
    /// Pre-V5 dimension with a table index and inline text style.
    Legacy {
        dimstyle_index: i32,
        text_display_mode: i32,
        text_height: f64,
        justification: i32,
    },
    /// V2 dimension with default text and definition points.
    V2 {
        default_text: String,
        points: Vec<[f64; 2]>,
        angle: Option<f64>,
        radius: Option<f64>,
    },
    /// Modern dimension referencing a dimstyle UUID.
    Modern { dimstyle_id: Uuid },
}

/// Complete common and family-specific dimension semantics.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Dimension {
    pub(crate) source_range: Range<usize>,
    pub(crate) annotation_type: i32,
    pub(crate) rich_text: String,
    pub(crate) user_text: String,
    pub(crate) family: DimensionFamily,
    pub(crate) plane: Plane,
    pub(crate) horizontal_direction: [f64; 2],
    pub(crate) allow_text_scaling: bool,
    pub(crate) use_default_text_point: bool,
    pub(crate) user_text_point: [f64; 2],
    pub(crate) flip_arrows: [bool; 2],
    pub(crate) arrow_position: i32,
    pub(crate) detail_measured: Uuid,
    pub(crate) distance_scale: f64,
    pub(crate) definition: Definition,
    pub(crate) measurement: f64,
    pub(crate) override_present: bool,
}

pub(crate) struct Annotation {
    pub(crate) rich_text: String,
    pub(crate) text_rectangle_width: f64,
    pub(crate) text_rotation_radians: f64,
    pub(crate) horizontal_alignment: i32,
    pub(crate) vertical_alignment: i32,
    pub(crate) wrapped: bool,
    pub(crate) dimstyle_id: Uuid,
    pub(crate) plane: Plane,
    pub(crate) kind: i32,
    pub(crate) horizontal_direction: [f64; 2],
    pub(crate) allow_text_scaling: bool,
    pub(crate) override_present: bool,
}

struct TextContent {
    rich_text: String,
    rectangle_width: f64,
    rotation_radians: f64,
    horizontal_alignment: i32,
    vertical_alignment: i32,
    wrapped: bool,
}

pub(crate) fn supported_class(class: Uuid) -> bool {
    matches!(
        class,
        LINEAR
            | ANGULAR
            | RADIAL
            | ORDINATE
            | CENTERMARK
            | V5_LINEAR
            | V5_ANGULAR
            | V5_RADIAL
            | V5_ORDINATE
            | V2_LINEAR
            | V2_ANGULAR
            | V2_RADIAL
    )
}

fn scale_plane(mut value: Plane, scale: f64, offset: usize) -> Result<Plane, FramingError> {
    for coordinate in &mut value.origin.0 {
        *coordinate = scaled_coordinate(*coordinate, scale)
            .ok_or_else(|| FramingError::structural(offset, "scaled dimension plane is invalid"))?;
    }
    value.equation[3] = scaled_coordinate(value.equation[3], scale)
        .ok_or_else(|| FramingError::structural(offset, "scaled dimension plane is invalid"))?;
    Ok(value)
}

fn anonymous(
    data: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<(BoundedReader<'_>, usize, i32), FramingError> {
    let chunk = chunk_at(data, offset, end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(FramingError::structural(
            offset,
            "expected dimension anonymous chunk",
        ));
    }
    let mut reader = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    if reader.i32()? != 1 {
        return Err(FramingError::structural(
            chunk.body().start,
            "unsupported dimension chunk major version",
        ));
    }
    let version = reader.i32()?;
    if version < 0 {
        return Err(FramingError::structural(
            chunk.body().start + 4,
            "negative dimension content version",
        ));
    }
    Ok((reader, chunk.next_offset(), version))
}

fn uuid(reader: &mut BoundedReader<'_>) -> Result<Uuid, FramingError> {
    Ok(Uuid::from_wire(reader.array()?))
}

fn point2(reader: &mut BoundedReader<'_>) -> Result<[f64; 2], FramingError> {
    let value = [reader.f64()?, reader.f64()?];
    if value.iter().all(|value| value.is_finite()) {
        Ok(value)
    } else {
        Err(FramingError::structural(
            reader.position() - 16,
            "dimension point is not finite",
        ))
    }
}

fn text_content(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<TextContent, FramingError> {
    let (mut text, next, _version) = anonymous(data, reader.position(), reader.end(), archive)?;
    let rich_text = utf16(&mut text)?;
    plane(&mut text)?;
    let rectangle_width = text.f64()?;
    let rotation_radians = text.f64()?;
    if !rectangle_width.is_finite() || !rotation_radians.is_finite() {
        return Err(FramingError::structural(
            text.position() - 16,
            "text layout contains a nonfinite value",
        ));
    }
    let horizontal_alignment = text.i32()?;
    let vertical_alignment = text.i32()?;
    if !text.f64()?.is_finite() {
        return Err(FramingError::structural(
            text.position() - 8,
            "obsolete text height is not finite",
        ));
    }
    let wrapped = text.bool()?;
    text.skip_remaining()?;
    reader.skip(next - reader.position())?;
    Ok(TextContent {
        rich_text,
        rectangle_width,
        rotation_radians,
        horizontal_alignment,
        vertical_alignment,
        wrapped,
    })
}

pub(crate) fn annotation(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Annotation, FramingError> {
    let (mut annotation, next, version) =
        anonymous(data, reader.position(), reader.end(), archive)?;
    let text = text_content(data, &mut annotation, archive)?;
    let dimstyle_id = uuid(&mut annotation)?;
    let plane = plane(&mut annotation)?;
    let annotation_type = if version >= 1 { annotation.i32()? } else { 0 };
    let mut override_present = false;
    if version >= 2 {
        let (mut overrides, override_next, _override_version) =
            anonymous(data, annotation.position(), annotation.end(), archive)?;
        if overrides.bool()? {
            override_present = true;
            let wrapper = chunk_at(data, overrides.position(), overrides.end(), archive, false)?;
            let mut warnings = Vec::new();
            parse_class_wrapper(
                data,
                overrides.position()..wrapper.next_offset(),
                archive,
                &mut warnings,
            )?;
            overrides.skip(wrapper.next_offset() - overrides.position())?;
        }
        overrides.skip_remaining()?;
        annotation.skip(override_next - annotation.position())?;
    }
    let horizontal_direction = if version >= 3 {
        point2(&mut annotation)?
    } else {
        [1.0, 0.0]
    };
    let allow_text_scaling = version < 4 || annotation.bool()?;
    annotation.skip_remaining()?;
    reader.skip(next - reader.position())?;
    Ok(Annotation {
        rich_text: text.rich_text,
        text_rectangle_width: text.rectangle_width,
        text_rotation_radians: text.rotation_radians,
        horizontal_alignment: text.horizontal_alignment,
        vertical_alignment: text.vertical_alignment,
        wrapped: text.wrapped,
        dimstyle_id,
        plane,
        kind: annotation_type,
        horizontal_direction,
        allow_text_scaling,
        override_present,
    })
}

fn scaled_point(value: [f64; 2], scale: f64, offset: usize) -> Result<[f64; 2], FramingError> {
    Ok([
        scaled_coordinate(value[0], scale)
            .ok_or_else(|| FramingError::structural(offset, "scaled dimension point is invalid"))?,
        scaled_coordinate(value[1], scale)
            .ok_or_else(|| FramingError::structural(offset, "scaled dimension point is invalid"))?,
    ])
}

fn angular_measurement(first: [f64; 2], second: [f64; 2]) -> f64 {
    let first = first[1].atan2(first[0]);
    (second[1].atan2(second[0]) - first).rem_euclid(std::f64::consts::TAU)
}

pub(crate) struct LegacyAnnotation {
    pub(crate) kind: i32,
    pub(crate) text_display_mode: i32,
    pub(crate) plane: Plane,
    pub(crate) points: Vec<[f64; 2]>,
    pub(crate) rich_text: String,
    pub(crate) user_text: String,
    pub(crate) user_positioned_text: bool,
    pub(crate) dimstyle_index: i32,
    pub(crate) allow_text_scaling: bool,
    pub(crate) text_height: f64,
    pub(crate) justification: i32,
}

pub(crate) fn legacy_annotation(
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: f64,
    archive: ArchiveVersion,
) -> Result<LegacyAnnotation, FramingError> {
    let (mut annotation, next, minor) = anonymous(data, reader.position(), reader.end(), archive)?;
    let value = legacy_annotation_fields(&mut annotation, scale, minor, false)?;
    annotation.skip_remaining()?;
    reader.skip(next - reader.position())?;
    Ok(value)
}

/// Reads the direct legacy annotation payload used by archive versions 2, 3,
/// and 4. Those archives store a packed version byte and then the common
/// fields without an anonymous wrapper.
pub(crate) fn legacy_annotation_direct(
    reader: &mut BoundedReader<'_>,
    scale: f64,
) -> Result<LegacyAnnotation, FramingError> {
    let version = reader.u8()?;
    if version >> 4 != 1 || version & 0x0f != 0 {
        return Err(FramingError::structural(
            reader.position() - 1,
            "unsupported direct legacy annotation version",
        ));
    }
    let value = legacy_annotation_fields(reader, scale, 0, true)?;
    reader.skip_remaining()?;
    Ok(value)
}

fn legacy_annotation_fields(
    annotation: &mut BoundedReader<'_>,
    scale: f64,
    minor: i32,
    direct_legacy: bool,
) -> Result<LegacyAnnotation, FramingError> {
    let kind = annotation.i32()?;
    let text_display_mode = annotation.i32()?;
    let plane_offset = annotation.position();
    let plane = scale_plane(plane(annotation)?, scale, plane_offset)?;
    let point_count_offset = annotation.position();
    let point_count = annotation.i32()?;
    let point_count = usize::try_from(point_count)
        .ok()
        .filter(|count| *count <= 1 << 16 && *count <= annotation.remaining() / 16)
        .ok_or_else(|| {
            FramingError::structural(point_count_offset, "invalid legacy annotation point count")
        })?;
    let mut points = Vec::with_capacity(point_count);
    for _ in 0..point_count {
        let offset = annotation.position();
        points.push(scaled_point(point2(annotation)?, scale, offset)?);
    }
    let rich_text = utf16(annotation)?;
    let user_positioned_text = match annotation.i32()? {
        0 => false,
        1 => true,
        _ => {
            return Err(FramingError::structural(
                annotation.position() - 4,
                "invalid legacy user-positioned-text flag",
            ))
        }
    };
    let initial_style_index = annotation.i32()?;
    let text_height = scaled_coordinate(annotation.f64()?, scale).ok_or_else(|| {
        FramingError::structural(annotation.position() - 8, "invalid legacy text height")
    })?;
    if !text_height.is_finite() || text_height < 0.0 {
        return Err(FramingError::structural(
            annotation.position() - 8,
            "invalid legacy annotation text height",
        ));
    }
    let justification = if direct_legacy { 0 } else { annotation.i32()? };
    let stored_text_scaling = (!direct_legacy && minor >= 1)
        .then(|| annotation.bool())
        .transpose()?;
    let allow_text_scaling = legacy_text_scaling(stored_text_scaling);
    let user_text = if !direct_legacy && minor >= 2 {
        utf16(annotation)?
    } else {
        rich_text.clone()
    };
    let dimstyle_index = if !direct_legacy && minor >= 3 {
        let text_style_index = annotation.i32()?;
        let dimension_style_index = annotation.i32()?;
        if kind == 7 {
            [text_style_index, initial_style_index, dimension_style_index]
                .into_iter()
                .find(|index| *index >= 0)
                .unwrap_or(initial_style_index)
        } else {
            [dimension_style_index, initial_style_index]
                .into_iter()
                .find(|index| *index >= 0)
                .unwrap_or(initial_style_index)
        }
    } else {
        initial_style_index
    };
    let (plane, justification) = if kind == 7 && justification == 0 {
        (shifted_plane(plane, [0.0, text_height]), (1 << 18) | 1)
    } else {
        (plane, justification)
    };
    Ok(LegacyAnnotation {
        kind,
        text_display_mode,
        plane,
        points,
        rich_text,
        user_text,
        user_positioned_text,
        dimstyle_index,
        allow_text_scaling,
        text_height,
        justification,
    })
}

fn modern_annotation_type(legacy: i32) -> i32 {
    match legacy {
        1 => 5,  // linear -> rotated
        2 => 1,  // aligned
        3 => 2,  // angular
        4 => 3,  // diameter
        5 => 4,  // radius
        6 => 10, // leader
        7 => 9,  // text
        8 => 6,  // ordinate
        _ => 0,
    }
}

fn legacy_text_scaling(stored: Option<bool>) -> bool {
    stored.unwrap_or(false)
}

fn shifted_plane(mut plane: Plane, point: [f64; 2]) -> Plane {
    for index in 0..3 {
        plane.origin.0[index] += point[0] * plane.xaxis.0[index] + point[1] * plane.yaxis.0[index];
    }
    plane.equation[3] = -(plane.equation[0] * plane.origin.0[0]
        + plane.equation[1] * plane.origin.0[1]
        + plane.equation[2] * plane.origin.0[2]);
    plane
}

fn difference(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn world_horizontal_in_plane(plane: &Plane) -> [f64; 2] {
    // ON_DimLinear::Create projects plane.origin + world X onto the plane.
    // Plane axes are orthonormal, so the plane coordinates are these dot products.
    [plane.xaxis.0[0], plane.yaxis.0[0]]
}

fn ordinate_direction(stored: i32, definition: [f64; 2], leader: [f64; 2]) -> Option<i32> {
    match stored {
        0 | 1 => Some(stored + 1),
        -1 => Some(
            if (leader[0] - definition[0]).abs() <= (leader[1] - definition[1]).abs() {
                1
            } else {
                2
            },
        ),
        _ => None,
    }
}

/// Common fields serialized by every concrete V2 annotation class.
pub(crate) struct V2Annotation {
    pub(crate) kind: i32,
    pub(crate) plane: Plane,
    pub(crate) points: Vec<[f64; 2]>,
    pub(crate) user_text: String,
    pub(crate) default_text: String,
    pub(crate) user_positioned_text: bool,
}

/// Reads the direct packed-1.0 V2 annotation prefix.
///
/// The reader stops after `m_userpositionedtext`. The enclosing class-data
/// range owns every subclass field and any future suffix.
pub(crate) fn v2_annotation_direct(
    reader: &mut BoundedReader<'_>,
    scale: f64,
) -> Result<V2Annotation, FramingError> {
    let version_offset = reader.position();
    if reader.u8()? >> 4 != 1 {
        return Err(FramingError::structural(
            version_offset,
            "unsupported direct V2 annotation version",
        ));
    }
    let kind = reader.i32()?;
    let plane_offset = reader.position();
    let raw_plane = plane(reader)?;
    if raw_plane
        .origin
        .0
        .iter()
        .any(|value| value.abs() > V2_REALLY_BIG_NUMBER)
    {
        return Err(FramingError::structural(
            plane_offset,
            "V2 annotation plane origin is outside the source bound",
        ));
    }
    let plane = scale_plane(raw_plane, scale, plane_offset)?;
    let point_count_offset = reader.position();
    let point_count = reader.i32()?;
    let point_bytes = checked_count_bytes(
        point_count,
        16,
        reader.remaining(),
        1 << 20,
        point_count_offset,
    )?;
    let mut points = Vec::with_capacity(point_bytes / 16);
    for _ in 0..point_bytes / 16 {
        let point_offset = reader.position();
        let raw_point = point2(reader)?;
        if raw_point
            .iter()
            .any(|value| value.abs() > V2_REALLY_BIG_NUMBER)
        {
            return Err(FramingError::structural(
                point_offset,
                "V2 annotation point is outside the source bound",
            ));
        }
        points.push(scaled_point(raw_point, scale, point_offset)?);
    }
    let user_text = utf16(reader)?;
    let default_text = utf16(reader)?;
    let user_positioned_text = reader.i32()? != 0;
    Ok(V2Annotation {
        kind,
        plane,
        points,
        user_text,
        default_text,
        user_positioned_text,
    })
}

/// Applies the source conversion's user-text selection and trimming rule.
pub(crate) fn v2_effective_text(annotation: &V2Annotation) -> String {
    let text = if annotation.user_text.is_empty() {
        &annotation.default_text
    } else {
        &annotation.user_text
    };
    text.trim_matches(|character: char| character.is_whitespace() || character.is_control())
        .to_owned()
}

fn decode_legacy(
    data: &[u8],
    class: Uuid,
    range: Range<usize>,
    scale: f64,
    archive: ArchiveVersion,
) -> Result<Dimension, FramingError> {
    // V2–V4 linear, radial, and angular classes call the common writer
    // directly; their ordinate class still has the always-present outer 1.1
    // family wrapper.
    // Every V5+ class uses the bounded anonymous family wrapper.
    let direct_legacy_common = matches!(
        archive,
        ArchiveVersion::V2 | ArchiveVersion::V3 | ArchiveVersion::V4
    );
    let direct_legacy_family = direct_legacy_common && class != V5_ORDINATE;
    let (mut outer, minor, mut annotation) = if direct_legacy_family {
        let mut reader = BoundedReader::new(data, range.start, range.end)?;
        let version = reader.u8()?;
        if version >> 4 != 1 || version & 0x0f != 0 {
            return Err(FramingError::structural(
                range.start,
                "unsupported direct legacy dimension version",
            ));
        }
        let annotation = legacy_annotation_fields(&mut reader, scale, 0, true)?;
        (reader, 0, annotation)
    } else {
        // The class reader closes the family child and the enclosing class-data
        // reader owns any direct suffix after that child.
        let (mut outer, _next, minor) = anonymous(data, range.start, range.end, archive)?;
        let annotation = if class == V5_ORDINATE {
            let (mut wrapper, wrapper_next, _wrapper_minor) =
                anonymous(data, outer.position(), outer.end(), archive)?;
            let annotation = if direct_legacy_common {
                legacy_annotation_direct(&mut wrapper, scale)?
            } else {
                legacy_annotation(data, &mut wrapper, scale, archive)?
            };
            wrapper.skip_remaining()?;
            outer.skip(wrapper_next - outer.position())?;
            annotation
        } else {
            legacy_annotation(data, &mut outer, scale, archive)?
        };
        (outer, minor, annotation)
    };
    // The V5 radial writer appends a fifth copy of the dimension-line point
    // for old readers; the source reader removes exactly that fifth point.
    if class == V5_RADIAL && annotation.points.len() == 5 {
        annotation.points.truncate(4);
    }
    let stored_angular = if class == V5_ANGULAR {
        let angle = outer.f64()?;
        let radius = scaled_coordinate(outer.f64()?, scale).ok_or_else(|| {
            FramingError::structural(outer.position() - 8, "invalid legacy angular radius")
        })?;
        if !angle.is_finite() || angle < 0.0 {
            return Err(FramingError::structural(
                outer.position() - 16,
                "invalid legacy angular angle",
            ));
        }
        Some((angle, radius))
    } else {
        None
    };
    let stored_ordinate = if class == V5_ORDINATE {
        let direction = outer.i32()?;
        let kink_offsets = if minor >= 1 {
            [
                scaled_coordinate(outer.f64()?, scale).ok_or_else(|| {
                    FramingError::structural(
                        outer.position() - 8,
                        "invalid legacy ordinate kink offset",
                    )
                })?,
                scaled_coordinate(outer.f64()?, scale).ok_or_else(|| {
                    FramingError::structural(
                        outer.position() - 8,
                        "invalid legacy ordinate kink offset",
                    )
                })?,
            ]
        } else {
            [0.0, 0.0]
        };
        Some((direction, kink_offsets))
    } else {
        None
    };
    outer.skip_remaining()?;
    let (plane, definition, user_text_point, measurement) = if class == V5_LINEAR {
        if !matches!(annotation.kind, 1 | 2) || annotation.points.len() != 5 {
            return Err(FramingError::structural(
                range.start,
                "invalid legacy linear definition",
            ));
        }
        let origin = annotation.points[0];
        let definition_point = difference(annotation.points[2], origin);
        let arrow_midpoint = [
            (annotation.points[1][0] + annotation.points[3][0]) * 0.5,
            (annotation.points[1][1] + annotation.points[3][1]) * 0.5,
        ];
        let dimension_line_point = difference(arrow_midpoint, origin);
        (
            shifted_plane(annotation.plane, origin),
            Definition::Linear {
                definition_point,
                dimension_line_point,
            },
            difference(annotation.points[4], origin),
            definition_point[0].abs(),
        )
    } else if class == V5_RADIAL {
        if !matches!(annotation.kind, 4 | 5) || annotation.points.len() != 4 {
            return Err(FramingError::structural(
                range.start,
                "invalid legacy radial definition",
            ));
        }
        let origin = annotation.points[0];
        let radius_point = difference(annotation.points[1], origin);
        let dimension_line_point = difference(annotation.points[2], origin);
        let diameter = annotation.kind == 4;
        (
            shifted_plane(annotation.plane, origin),
            Definition::Radial {
                radius_point,
                dimension_line_point,
                diameter,
            },
            dimension_line_point,
            radius_point[0].hypot(radius_point[1]) * if diameter { 2.0 } else { 1.0 },
        )
    } else if class == V5_ANGULAR {
        if annotation.kind != 3 || annotation.points.len() != 4 {
            return Err(FramingError::structural(
                range.start,
                "invalid legacy angular definition",
            ));
        }
        let (angle, radius) = stored_angular.expect("angular family has stored fields");
        let first_direction = [1.0, 0.0];
        let second_direction = [angle.cos(), angle.sin()];
        let dimension_line_point = [radius * (0.5 * angle).cos(), radius * (0.5 * angle).sin()];
        (
            annotation.plane,
            Definition::Angular {
                first_direction,
                second_direction,
                // ON_OBSOLETE_V5_DimAngular returns -1 when its optional
                // ON_AngularDimension2Extra userdata is absent.
                first_extension_offset: -1.0,
                second_extension_offset: -1.0,
                dimension_line_point,
            },
            annotation.points[0],
            angle,
        )
    } else {
        if annotation.kind != 8 || annotation.points.len() != 2 {
            return Err(FramingError::structural(
                range.start,
                "invalid legacy ordinate definition",
            ));
        }
        let definition_point = annotation.points[0];
        let leader_point = annotation.points[1];
        let (stored_direction, kink_offsets) =
            stored_ordinate.expect("ordinate family has stored fields");
        let measured_direction =
            ordinate_direction(stored_direction, definition_point, leader_point).ok_or_else(
                || FramingError::structural(range.start, "invalid legacy ordinate direction"),
            )?;
        let measurement = if measured_direction == 1 {
            definition_point[0].abs()
        } else {
            definition_point[1].abs()
        };
        (
            annotation.plane,
            Definition::Ordinate {
                definition_point,
                leader_point,
                measured_direction,
                kink_offsets,
            },
            leader_point,
            measurement,
        )
    };
    if !measurement.is_finite() {
        return Err(FramingError::structural(
            range.start,
            "legacy dimension measurement is invalid",
        ));
    }
    let horizontal_direction = world_horizontal_in_plane(&plane);
    Ok(Dimension {
        source_range: range,
        annotation_type: modern_annotation_type(annotation.kind),
        rich_text: annotation.rich_text,
        user_text: annotation.user_text,
        family: DimensionFamily::Legacy {
            dimstyle_index: annotation.dimstyle_index,
            text_display_mode: annotation.text_display_mode,
            text_height: annotation.text_height,
            justification: annotation.justification,
        },
        plane,
        horizontal_direction,
        allow_text_scaling: annotation.allow_text_scaling,
        use_default_text_point: !annotation.user_positioned_text,
        user_text_point,
        flip_arrows: [false, false],
        arrow_position: 0,
        detail_measured: Uuid::nil(),
        distance_scale: 1.0,
        definition,
        measurement,
        override_present: false,
    })
}

fn decode_v2(
    data: &[u8],
    class: Uuid,
    range: Range<usize>,
    scale: f64,
) -> Result<Dimension, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let annotation = v2_annotation_direct(&mut reader, scale)?;
    let kind = annotation.kind;
    let points = &annotation.points;
    let mut v2_angle = None;
    let mut v2_radius = None;
    let (plane, definition, user_text_point, use_default_text_point, measurement) = if class
        == V2_LINEAR
    {
        if !matches!(kind, 1 | 2) || points.len() < 4 {
            return Err(FramingError::structural(
                range.start,
                "invalid V2 linear definition",
            ));
        }
        let origin = points[0];
        let definition_point = difference(points[2], origin);
        let arrow_midpoint = [
            (points[1][0] + points[3][0]) * 0.5,
            (points[1][1] + points[3][1]) * 0.5,
        ];
        let user_text_point = points
            .get(4)
            .copied()
            .map_or([0.0, 0.0], |point| difference(point, origin));
        (
            shifted_plane(annotation.plane, origin),
            Definition::Linear {
                definition_point,
                dimension_line_point: difference(arrow_midpoint, origin),
            },
            user_text_point,
            true,
            difference(points[1], points[3])
                .into_iter()
                .map(|value| value * value)
                .sum::<f64>()
                .sqrt(),
        )
    } else if class == V2_RADIAL {
        if !matches!(kind, 4 | 5) || points.len() < 3 {
            return Err(FramingError::structural(
                range.start,
                "invalid V2 radial definition",
            ));
        }
        let origin = points[0];
        let radius_point = difference(points[1], origin);
        let dimension_line_point = difference(points[2], origin);
        let diameter = kind == 4;
        let user_text_point = points
            .get(3)
            .copied()
            .map_or([0.0, 0.0], |point| difference(point, origin));
        (
            shifted_plane(annotation.plane, origin),
            Definition::Radial {
                radius_point,
                dimension_line_point,
                diameter,
            },
            user_text_point,
            !annotation.user_positioned_text,
            radius_point[0].hypot(radius_point[1]) * if diameter { 2.0 } else { 1.0 },
        )
    } else if class == V2_ANGULAR {
        if kind != 3 || points.len() < 2 {
            return Err(FramingError::structural(
                range.start,
                "invalid V2 angular definition",
            ));
        }
        let angle = reader.f64()?;
        let radius_offset = reader.position();
        let raw_radius = reader.f64()?;
        let radius = scaled_coordinate(raw_radius, scale)
            .ok_or_else(|| FramingError::structural(radius_offset, "invalid V2 angular radius"))?;
        if !angle.is_finite()
            || angle <= 0.0
            || angle > V2_REALLY_BIG_NUMBER
            || !raw_radius.is_finite()
            || raw_radius <= 0.0
            || raw_radius > V2_REALLY_BIG_NUMBER
            || !radius.is_finite()
            || radius <= 0.0
        {
            return Err(FramingError::structural(
                range.start,
                "invalid V2 angular value",
            ));
        }
        v2_angle = Some(angle);
        v2_radius = Some(radius);
        let user_text_point = points.get(2).copied().unwrap_or([0.0, 0.0]);
        (
            annotation.plane,
            Definition::Angular {
                first_direction: points[0],
                second_direction: points[1],
                first_extension_offset: -1.0,
                second_extension_offset: -1.0,
                dimension_line_point: [radius * (0.5 * angle).cos(), radius * (0.5 * angle).sin()],
            },
            user_text_point,
            !annotation.user_positioned_text,
            angle,
        )
    } else {
        return Err(FramingError::structural(
            range.start,
            "unsupported V2 dimension class",
        ));
    };
    reader.skip_remaining()?;
    if !measurement.is_finite() {
        return Err(FramingError::structural(
            range.start,
            "V2 dimension measurement is invalid",
        ));
    }
    Ok(Dimension {
        source_range: range,
        annotation_type: modern_annotation_type(kind),
        rich_text: v2_effective_text(&annotation),
        user_text: annotation.user_text,
        family: DimensionFamily::V2 {
            default_text: annotation.default_text,
            points: annotation.points,
            angle: v2_angle,
            radius: v2_radius,
        },
        plane,
        horizontal_direction: world_horizontal_in_plane(&plane),
        allow_text_scaling: false,
        use_default_text_point,
        user_text_point,
        flip_arrows: [false, false],
        arrow_position: 0,
        detail_measured: Uuid::nil(),
        distance_scale: 1.0,
        definition,
        measurement,
        override_present: false,
    })
}

/// Decodes one modern linear, angular, or radial dimension.
pub(crate) fn decode(
    data: &[u8],
    class: Uuid,
    range: Range<usize>,
    scale: f64,
    archive: ArchiveVersion,
) -> Result<Dimension, FramingError> {
    if matches!(class, V2_LINEAR | V2_ANGULAR | V2_RADIAL) {
        return decode_v2(data, class, range, scale);
    }
    if matches!(class, V5_LINEAR | V5_ANGULAR | V5_RADIAL | V5_ORDINATE) {
        return decode_legacy(data, class, range, scale, archive);
    }
    // The class reader closes the family child and the enclosing class-data
    // reader owns any direct suffix after that child.
    let (mut outer, _outer_next, _outer_version) =
        anonymous(data, range.start, range.end, archive)?;
    let (mut common, common_next, common_version) =
        anonymous(data, outer.position(), outer.end(), archive)?;
    let mut annotation = annotation(data, &mut common, archive)?;
    annotation.plane = scale_plane(annotation.plane, scale, range.start)?;
    let user_text = utf16(&mut common)?;
    if !common.f64()?.is_finite() {
        return Err(FramingError::structural(
            common.position() - 8,
            "obsolete text rotation is not finite",
        ));
    }
    let use_default_text_point = common.bool()?;
    let text_offset = common.position();
    let user_text_point = scaled_point(point2(&mut common)?, scale, text_offset)?;
    let flip_arrows = [common.bool()?, common.bool()?];
    let arrow_position = match common.i32()? {
        1 => 1,
        2 => -1,
        _ => 0,
    };
    let detail_measured = uuid(&mut common)?;
    let distance_scale = common.f64()?;
    if !distance_scale.is_finite() || distance_scale <= 0.0 {
        return Err(FramingError::structural(
            common.position() - 8,
            "dimension distance scale is invalid",
        ));
    }
    if common_version >= 1 {
        common.i32()?;
    }
    common.skip_remaining()?;
    outer.skip(common_next - outer.position())?;
    let definition = if class == LINEAR {
        if !matches!(annotation.kind, 1 | 5) {
            return Err(FramingError::structural(
                outer.position(),
                "invalid linear annotation type",
            ));
        }
        let offset = outer.position();
        let definition_point = scaled_point(point2(&mut outer)?, scale, offset)?;
        let offset = outer.position();
        let dimension_line_point = scaled_point(point2(&mut outer)?, scale, offset)?;
        Definition::Linear {
            definition_point,
            dimension_line_point,
        }
    } else if class == ANGULAR {
        if !matches!(annotation.kind, 2 | 11) {
            return Err(FramingError::structural(
                outer.position(),
                "invalid angular annotation type",
            ));
        }
        let first = point2(&mut outer)?;
        let second = point2(&mut outer)?;
        let first_extension_offset = scaled_coordinate(outer.f64()?, scale).ok_or_else(|| {
            FramingError::structural(outer.position() - 8, "angular extension offset is invalid")
        })?;
        let second_extension_offset = scaled_coordinate(outer.f64()?, scale).ok_or_else(|| {
            FramingError::structural(outer.position() - 8, "angular extension offset is invalid")
        })?;
        let offset = outer.position();
        let line = point2(&mut outer)?;
        let dimension_line_point = scaled_point(line, scale, offset)?;
        Definition::Angular {
            first_direction: first,
            second_direction: second,
            first_extension_offset,
            second_extension_offset,
            dimension_line_point,
        }
    } else if class == RADIAL {
        if !matches!(annotation.kind, 3 | 4) {
            return Err(FramingError::structural(
                outer.position(),
                "invalid radial annotation type",
            ));
        }
        let offset = outer.position();
        let radius_point = scaled_point(point2(&mut outer)?, scale, offset)?;
        let offset = outer.position();
        let dimension_line_point = scaled_point(point2(&mut outer)?, scale, offset)?;
        Definition::Radial {
            radius_point,
            dimension_line_point,
            diameter: annotation.kind == 3,
        }
    } else if class == ORDINATE {
        if annotation.kind != 6 {
            return Err(FramingError::structural(
                outer.position(),
                "invalid ordinate annotation type",
            ));
        }
        let stored_direction = outer.i32()?;
        if !(0..=2).contains(&stored_direction) {
            return Err(FramingError::structural(
                outer.position() - 4,
                "invalid ordinate measured direction",
            ));
        }
        let offset = outer.position();
        let definition_point = scaled_point(point2(&mut outer)?, scale, offset)?;
        let offset = outer.position();
        let leader_point = scaled_point(point2(&mut outer)?, scale, offset)?;
        let measured_direction = if stored_direction == 0 {
            ordinate_direction(-1, definition_point, leader_point)
                .expect("inferred ordinate direction")
        } else {
            stored_direction
        };
        let kink_offsets = [
            scaled_coordinate(outer.f64()?, scale).ok_or_else(|| {
                FramingError::structural(outer.position() - 8, "invalid ordinate kink offset")
            })?,
            scaled_coordinate(outer.f64()?, scale).ok_or_else(|| {
                FramingError::structural(outer.position() - 8, "invalid ordinate kink offset")
            })?,
        ];
        Definition::Ordinate {
            definition_point,
            leader_point,
            measured_direction,
            kink_offsets,
        }
    } else if class == CENTERMARK {
        if annotation.kind != 8 {
            return Err(FramingError::structural(
                outer.position(),
                "invalid center-mark annotation type",
            ));
        }
        let radius = scaled_coordinate(outer.f64()?, scale)
            .filter(|radius| *radius >= 0.0)
            .ok_or_else(|| {
                FramingError::structural(outer.position() - 8, "invalid center-mark radius")
            })?;
        Definition::CenterMark { radius }
    } else {
        return Err(FramingError::structural(
            range.start,
            "unsupported dimension class",
        ));
    };
    outer.skip_remaining()?;
    let measurement = match &definition {
        Definition::Linear {
            definition_point, ..
        } => definition_point[0].abs() * distance_scale,
        Definition::Angular {
            first_direction,
            second_direction,
            ..
        } => angular_measurement(*first_direction, *second_direction),
        Definition::Radial {
            radius_point,
            diameter,
            ..
        } => {
            radius_point[0].hypot(radius_point[1])
                * distance_scale
                * if *diameter { 2.0 } else { 1.0 }
        }
        Definition::Ordinate {
            definition_point,
            measured_direction,
            ..
        } => {
            (if *measured_direction == 1 {
                definition_point[0].abs()
            } else {
                definition_point[1].abs()
            }) * distance_scale
        }
        Definition::CenterMark { .. } => 0.0,
    };
    if !measurement.is_finite() {
        return Err(FramingError::structural(
            range.start,
            "dimension measurement is invalid",
        ));
    }
    Ok(Dimension {
        source_range: range,
        annotation_type: annotation.kind,
        rich_text: annotation.rich_text,
        user_text,
        family: DimensionFamily::Modern {
            dimstyle_id: annotation.dimstyle_id,
        },
        plane: annotation.plane,
        horizontal_direction: annotation.horizontal_direction,
        allow_text_scaling: annotation.allow_text_scaling,
        use_default_text_point,
        user_text_point,
        flip_arrows,
        arrow_position,
        detail_measured,
        distance_scale,
        definition,
        measurement,
        override_present: annotation.override_present,
    })
}

/// Applies the built-in V5 dimension extension carried as class userdata.
pub(crate) fn apply_userdata(
    data: &[u8],
    userdata: &[UserdataDescriptor],
    archive: ArchiveVersion,
    scale: f64,
    dimension: &mut Dimension,
) -> Result<(), FramingError> {
    if let Definition::Angular {
        first_extension_offset,
        second_extension_offset,
        ..
    } = &mut dimension.definition
    {
        if let Some(extra) = userdata.iter().find(|userdata| {
            userdata.class_uuid() == V5_ANGULAR_EXTRA && userdata.item_uuid() == V5_ANGULAR_EXTRA
        }) {
            let (mut reader, _next, _minor) = anonymous(
                data,
                extra.payload_range().start,
                extra.payload_range().end,
                archive,
            )?;
            *first_extension_offset = scaled_coordinate(reader.f64()?, scale).ok_or_else(|| {
                FramingError::structural(
                    reader.position() - 8,
                    "invalid V5 angular extension offset",
                )
            })?;
            *second_extension_offset =
                scaled_coordinate(reader.f64()?, scale).ok_or_else(|| {
                    FramingError::structural(
                        reader.position() - 8,
                        "invalid V5 angular extension offset",
                    )
                })?;
            reader.skip_remaining()?;
        }
    }
    let Some(extra) = userdata.iter().find(|userdata| {
        userdata.class_uuid() == V5_DIM_EXTRA && userdata.item_uuid() == V5_DIM_EXTRA
    }) else {
        return Ok(());
    };
    let (mut reader, _next, minor) = anonymous(
        data,
        extra.payload_range().start,
        extra.payload_range().end,
        archive,
    )?;
    uuid(&mut reader)?;
    let arrow_position = reader.i32()?;
    if !(-1..=1).contains(&arrow_position) {
        return Err(FramingError::structural(
            reader.position() - 4,
            "invalid V5 dimension arrow position",
        ));
    }
    let rectangle_count = reader.i32()?;
    match rectangle_count {
        0 => {}
        7 => {
            for _ in 0..28 {
                reader.i32()?;
            }
        }
        _ => {
            return Err(FramingError::structural(
                reader.position() - 4,
                "invalid V5 dimension text rectangle count",
            ))
        }
    }
    let distance_scale = if minor >= 1 { reader.f64()? } else { 1.0 };
    if !distance_scale.is_finite() || distance_scale <= 0.0 {
        return Err(FramingError::structural(
            reader.position() - 8,
            "invalid V5 dimension distance scale",
        ));
    }
    let detail_measured = if minor >= 2 {
        uuid(&mut reader)?
    } else {
        Uuid::nil()
    };
    reader.skip_remaining()?;
    dimension.arrow_position = arrow_position;
    if matches!(dimension.family, DimensionFamily::Legacy { .. }) {
        dimension.distance_scale = distance_scale;
    }
    dimension.detail_measured = detail_measured;
    if matches!(dimension.family, DimensionFamily::Legacy { .. })
        && !matches!(dimension.definition, Definition::Angular { .. })
    {
        dimension.measurement *= distance_scale;
    }
    Ok(())
}

/// Projects a decoded dimension into one measured semantic annotation.
///
/// `object` is the 3DM object-record identity (also used as `native_ref`).
/// `order` must be a globally unique dense `u32`.
///
/// Returns the annotation and the codes for every reference the annotation could
/// not carry.
pub(crate) fn project(
    dimension: &Dimension,
    key: &str,
    name: Option<String>,
    object: &str,
    order: u32,
) -> (
    cadmpeg_ir::semantic_annotations::SemanticAnnotation,
    Vec<crate::loss::RhinoLossCode>,
) {
    use crate::loss::RhinoLossCode;
    use cadmpeg_ir::semantic_annotations::{
        SemanticAnnotation, SemanticAnnotationId, SemanticAnnotationKind,
    };
    use cadmpeg_ir::{ReferenceSelection, ReferenceTarget};
    use std::collections::BTreeMap;

    let (runtime_type, value) = match dimension.definition {
        Definition::Linear { .. } => ("linear_dimension", dimension.measurement),
        Definition::Angular { .. } => ("angular_dimension", dimension.measurement),
        Definition::Radial { diameter, .. } => (
            if diameter {
                "diameter_dimension"
            } else {
                "radius_dimension"
            },
            dimension.measurement,
        ),
        Definition::Ordinate { .. } => ("ordinate_dimension", dimension.measurement),
        // A center mark measures nothing, so `measurement` is zero by
        // construction. Its radius is the one persisted numeric it does carry.
        Definition::CenterMark { radius } => ("center_mark", radius),
    };
    let mut parameters =
        BTreeMap::from([("measurement".to_string(), dimension.measurement.to_string())]);
    let mut properties = BTreeMap::from([
        (
            "annotation_type".to_string(),
            dimension.annotation_type.to_string(),
        ),
        (
            "detail_measured".to_string(),
            dimension.detail_measured.to_string(),
        ),
        (
            "distance_scale".to_string(),
            dimension.distance_scale.to_string(),
        ),
        ("rich_text".to_string(), dimension.rich_text.clone()),
        ("user_text".to_string(), dimension.user_text.clone()),
        (
            "use_default_text_point".to_string(),
            dimension.use_default_text_point.to_string(),
        ),
        (
            "user_text_point".to_string(),
            format!(
                "{},{}",
                dimension.user_text_point[0], dimension.user_text_point[1]
            ),
        ),
        (
            "flip_arrows".to_string(),
            format!("{},{}", dimension.flip_arrows[0], dimension.flip_arrows[1]),
        ),
        (
            "arrow_position".to_string(),
            dimension.arrow_position.to_string(),
        ),
        (
            "allow_text_scaling".to_string(),
            dimension.allow_text_scaling.to_string(),
        ),
        (
            "plane_origin".to_string(),
            dimension
                .plane
                .origin
                .0
                .iter()
                .map(f64::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
        (
            "plane_x_axis".to_string(),
            dimension
                .plane
                .xaxis
                .0
                .iter()
                .map(f64::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
        (
            "plane_y_axis".to_string(),
            dimension
                .plane
                .yaxis
                .0
                .iter()
                .map(f64::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
        (
            "plane_z_axis".to_string(),
            dimension
                .plane
                .zaxis
                .0
                .iter()
                .map(f64::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
        (
            "plane_equation".to_string(),
            dimension
                .plane
                .equation
                .iter()
                .map(f64::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
        (
            "horizontal_direction".to_string(),
            dimension
                .horizontal_direction
                .iter()
                .map(f64::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
    ]);
    match &dimension.family {
        DimensionFamily::Modern { dimstyle_id } => {
            properties.insert("dimstyle_id".to_string(), dimstyle_id.to_string());
        }
        DimensionFamily::Legacy {
            dimstyle_index,
            text_display_mode,
            text_height,
            justification,
        } => {
            properties.insert("dimstyle_index".to_string(), dimstyle_index.to_string());
            properties.insert(
                "text_display_mode".to_string(),
                text_display_mode.to_string(),
            );
            properties.insert("text_height".to_string(), text_height.to_string());
            properties.insert("justification".to_string(), justification.to_string());
        }
        DimensionFamily::V2 {
            default_text,
            points,
            angle,
            radius,
        } => {
            properties.insert("v2_default_text".to_string(), default_text.clone());
            properties.insert(
                "v2_points".to_string(),
                points
                    .iter()
                    .map(|point| format!("{},{}", point[0], point[1]))
                    .collect::<Vec<_>>()
                    .join(";"),
            );
            if let Some(angle) = *angle {
                properties.insert("v2_angle_radians".to_string(), angle.to_string());
                properties.insert(
                    "v2_numeric_value_degrees".to_string(),
                    (angle * 180.0 / std::f64::consts::PI).to_string(),
                );
            }
            if let Some(radius) = *radius {
                properties.insert("v2_radius".to_string(), radius.to_string());
            }
        }
    }
    match &dimension.definition {
        Definition::Linear {
            definition_point,
            dimension_line_point,
        } => {
            properties.insert(
                "definition_point".to_string(),
                format!("{},{}", definition_point[0], definition_point[1]),
            );
            properties.insert(
                "dimension_line_point".to_string(),
                format!("{},{}", dimension_line_point[0], dimension_line_point[1]),
            );
        }
        Definition::Angular {
            first_direction,
            second_direction,
            first_extension_offset,
            second_extension_offset,
            dimension_line_point,
        } => {
            properties.insert(
                "first_direction".to_string(),
                format!("{},{}", first_direction[0], first_direction[1]),
            );
            properties.insert(
                "second_direction".to_string(),
                format!("{},{}", second_direction[0], second_direction[1]),
            );
            properties.insert(
                "first_extension_offset".to_string(),
                first_extension_offset.to_string(),
            );
            properties.insert(
                "second_extension_offset".to_string(),
                second_extension_offset.to_string(),
            );
            properties.insert(
                "dimension_line_point".to_string(),
                format!("{},{}", dimension_line_point[0], dimension_line_point[1]),
            );
        }
        Definition::Radial {
            radius_point,
            dimension_line_point,
            ..
        } => {
            properties.insert(
                "radius_point".to_string(),
                format!("{},{}", radius_point[0], radius_point[1]),
            );
            properties.insert(
                "dimension_line_point".to_string(),
                format!("{},{}", dimension_line_point[0], dimension_line_point[1]),
            );
        }
        Definition::Ordinate {
            definition_point,
            leader_point,
            measured_direction,
            kink_offsets,
        } => {
            properties.insert(
                "definition_point".to_string(),
                format!("{},{}", definition_point[0], definition_point[1]),
            );
            properties.insert(
                "leader_point".to_string(),
                format!("{},{}", leader_point[0], leader_point[1]),
            );
            properties.insert(
                "measured_direction".to_string(),
                measured_direction.to_string(),
            );
            properties.insert(
                "kink_offsets".to_string(),
                format!("{},{}", kink_offsets[0], kink_offsets[1]),
            );
        }
        Definition::CenterMark { radius } => {
            properties.insert("radius".to_string(), radius.to_string());
        }
    }
    if let Some(name) = name {
        parameters.insert("object_name".to_string(), name);
    }
    parameters.extend(properties);

    // Model-space text point via the dimension plane (stored UV, not world xyz).
    let position = (!dimension.use_default_text_point)
        .then(|| {
            let [u, v] = dimension.user_text_point;
            let origin = dimension.plane.origin.0;
            let x_axis = dimension.plane.xaxis.0;
            let y_axis = dimension.plane.yaxis.0;
            [0, 1, 2].map(|axis| origin[axis] + u * x_axis[axis] + v * y_axis[axis])
        })
        .filter(|point| point.iter().all(|value| value.is_finite()));

    // dimstyle/detail targets are not resolvable here: nil -> null reference;
    // non-nil -> charge and keep the raw UUID in parameters.
    let mut references = BTreeMap::new();
    let mut unresolved = Vec::new();
    let mut reference = |role: &str, id: Option<Uuid>, code: RhinoLossCode| match id {
        None => {}
        Some(id) if id.is_nil() => {
            references.insert(
                role.to_string(),
                vec![ReferenceSelection::new(ReferenceTarget::Null, Vec::new())],
            );
        }
        Some(_) => unresolved.push(code),
    };
    reference(
        "dimstyle_id",
        match &dimension.family {
            DimensionFamily::Modern { dimstyle_id } => Some(*dimstyle_id),
            DimensionFamily::Legacy { .. } | DimensionFamily::V2 { .. } => None,
        },
        RhinoLossCode::DimensionStyleUnresolved,
    );
    reference(
        "detail_measured",
        Some(dimension.detail_measured),
        RhinoLossCode::DimensionDetailReferenceUnresolved,
    );

    let annotation = SemanticAnnotation {
        id: SemanticAnnotationId(format!("rhino:dimension:annotation#{key}")),
        object: object.to_string(),
        kind: SemanticAnnotationKind::Dimension,
        runtime_type: runtime_type.to_string(),
        order,
        text: (!dimension.user_text.is_empty())
            .then(|| dimension.user_text.clone())
            .into_iter()
            .collect(),
        references,
        value: Some(value),
        format: (!dimension.rich_text.is_empty()).then(|| dimension.rich_text.clone()),
        position,
        parameters,
        assets: Vec::new(),
        native_ref: object.to_string(),
    };
    (annotation, unresolved)
}

/// Serializes one decoded dimension without source-record identity.
pub(crate) fn semantic_json(dimension: &Dimension) -> Option<String> {
    let (annotation, _) = project(dimension, "embedded-history-dimension", None, "", 0);
    serde_json::to_string(&serde_json::json!({
        "kind": "dimension",
        "runtime_type": annotation.runtime_type,
        "value": annotation.value,
        "format": annotation.format,
        "position": annotation.position,
        "references": annotation.references,
        "parameters": annotation.parameters,
    }))
    .ok()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::test_support::crc_chunk;

    #[test]
    fn angular_measurement_uses_counterclockwise_extension_sweep() {
        let first = [1.0, 0.0];
        let second = [0.0, 1.0];
        assert_eq!(
            angular_measurement(first, second),
            std::f64::consts::FRAC_PI_2
        );
    }

    #[test]
    fn legacy_annotation_defaults_and_type_mapping_match_v5_reader() {
        assert!(!legacy_text_scaling(None));
        assert!(legacy_text_scaling(Some(true)));
        assert_eq!(modern_annotation_type(1), 5);
        assert_eq!(modern_annotation_type(2), 1);
        assert_eq!(modern_annotation_type(3), 2);
        assert_eq!(modern_annotation_type(4), 3);
        assert_eq!(modern_annotation_type(5), 4);
        assert_eq!(modern_annotation_type(8), 6);
    }

    fn utf16(value: &str) -> Vec<u8> {
        let mut units = value.encode_utf16().collect::<Vec<_>>();
        units.push(0);
        let mut bytes = (units.len() as u32).to_le_bytes().to_vec();
        for unit in units {
            bytes.extend(unit.to_le_bytes());
        }
        bytes
    }

    fn anonymous(version: i32, suffix: &[u8]) -> Vec<u8> {
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(version.to_le_bytes());
        body.extend(suffix);
        crc_chunk(ANONYMOUS, &body)
    }

    fn anonymous_v4(version: i32, suffix: &[u8]) -> Vec<u8> {
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(version.to_le_bytes());
        body.extend(suffix);
        crate::test_support::test_dump::crc_chunk(ArchiveVersion::V4, ANONYMOUS, &body)
    }

    fn plane() -> Vec<u8> {
        plane_bytes(
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        )
    }

    /// One `ON_Plane` with an explicit origin, x axis, y axis, and equation.
    ///
    /// The z axis is the right-handed cross product of the supplied axes.
    pub(crate) fn plane_bytes(
        origin: [f64; 3],
        x_axis: [f64; 3],
        y_axis: [f64; 3],
        equation: [f64; 4],
    ) -> Vec<u8> {
        let z_axis = [
            x_axis[1] * y_axis[2] - x_axis[2] * y_axis[1],
            x_axis[2] * y_axis[0] - x_axis[0] * y_axis[2],
            x_axis[0] * y_axis[1] - x_axis[1] * y_axis[0],
        ];
        origin
            .into_iter()
            .chain(x_axis)
            .chain(y_axis)
            .chain(z_axis)
            .chain(equation)
            .flat_map(f64::to_le_bytes)
            .collect()
    }

    fn v2_payload(
        kind: i32,
        points: &[[f64; 2]],
        user_text: &str,
        default_text: &str,
        user_positioned: bool,
        angular: Option<(f64, f64)>,
    ) -> Vec<u8> {
        let mut bytes = vec![0x10];
        bytes.extend(kind.to_le_bytes());
        bytes.extend(plane());
        bytes.extend((points.len() as i32).to_le_bytes());
        for point in points {
            bytes.extend(point[0].to_le_bytes());
            bytes.extend(point[1].to_le_bytes());
        }
        bytes.extend(utf16(user_text));
        bytes.extend(utf16(default_text));
        bytes.extend(i32::from(user_positioned).to_le_bytes());
        if let Some((angle, radius)) = angular {
            bytes.extend(angle.to_le_bytes());
            bytes.extend(radius.to_le_bytes());
        }
        bytes.extend([0xa5, 0x5a]);
        bytes
    }

    #[test]
    fn v2_common_reader_preserves_subclass_boundary_and_source_text_selection() {
        let bytes = v2_payload(
            1,
            &[[1.0, 2.0], [0.0, 0.0], [5.0, 0.0], [3.0, 0.0], [7.0, 4.0]],
            "  user <>  ",
            "default",
            false,
            None,
        );
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded V2 payload");
        let annotation = v2_annotation_direct(&mut reader, 2.0).expect("V2 common prefix");
        assert_eq!(annotation.points[0], [2.0, 4.0]);
        assert_eq!(annotation.user_text, "  user <>  ");
        assert_eq!(annotation.default_text, "default");
        assert!(!annotation.user_positioned_text);
        assert_eq!(reader.remaining(), 2);
        assert_eq!(v2_effective_text(&annotation), "user <>");
    }

    #[test]
    fn v2_dimension_families_use_stored_values_and_preserve_all_points() {
        let linear_bytes = v2_payload(
            1,
            &[[1.0, 2.0], [0.0, 0.0], [5.0, 0.0], [3.0, 0.0], [7.0, 4.0]],
            "user",
            "default",
            false,
            None,
        );
        let linear = decode(
            &linear_bytes,
            V2_LINEAR,
            0..linear_bytes.len(),
            2.0,
            ArchiveVersion::V4,
        )
        .expect("V2 linear dimension");
        assert_eq!(linear.annotation_type, 5);
        assert_eq!(linear.measurement, 6.0);
        assert_eq!(linear.user_text, "user");
        assert_eq!(linear.rich_text, "user");
        let DimensionFamily::V2 {
            default_text,
            points,
            ..
        } = &linear.family
        else {
            panic!("V2 linear dimension");
        };
        assert_eq!(default_text.as_str(), "default");
        assert_eq!(points.len(), 5);
        assert!(linear.use_default_text_point);

        let radial_bytes = v2_payload(
            4,
            &[[1.0, 2.0], [5.0, 2.0], [8.0, 4.0], [9.0, 6.0]],
            "",
            "radius",
            true,
            None,
        );
        let radial = decode(
            &radial_bytes,
            V2_RADIAL,
            0..radial_bytes.len(),
            2.0,
            ArchiveVersion::V4,
        )
        .expect("V2 radial dimension");
        assert_eq!(radial.annotation_type, 3);
        assert_eq!(radial.measurement, 16.0);
        assert_eq!(radial.rich_text, "radius");
        assert!(!radial.use_default_text_point);

        let angular_bytes = v2_payload(
            3,
            &[[1.0, 0.0], [0.0, 1.0], [2.0, 3.0]],
            "angle",
            "default angle",
            true,
            Some((1.25, 9.5)),
        );
        let angular = decode(
            &angular_bytes,
            V2_ANGULAR,
            0..angular_bytes.len(),
            2.0,
            ArchiveVersion::V4,
        )
        .expect("V2 angular dimension");
        assert_eq!(angular.measurement, 1.25);
        let DimensionFamily::V2 { angle, radius, .. } = angular.family else {
            panic!("V2 angular dimension");
        };
        assert_eq!(angle, Some(1.25));
        assert_eq!(radius, Some(19.0));
        assert!(!angular.use_default_text_point);
        assert_eq!(angular.user_text_point, [4.0, 6.0]);
    }

    #[test]
    fn v2_angular_reader_rejects_nonpositive_stored_values() {
        let bytes = v2_payload(
            3,
            &[[1.0, 0.0], [0.0, 1.0]],
            "angle",
            "default",
            false,
            Some((0.0, 9.5)),
        );
        assert!(decode(&bytes, V2_ANGULAR, 0..bytes.len(), 1.0, ArchiveVersion::V4,).is_err());
    }

    #[test]
    fn v2_angular_reader_enforces_source_upper_bound() {
        let bytes = v2_payload(
            3,
            &[[1.0, 0.0], [0.0, 1.0]],
            "angle",
            "default",
            false,
            Some((V2_REALLY_BIG_NUMBER.next_up(), 9.5)),
        );
        assert!(decode(&bytes, V2_ANGULAR, 0..bytes.len(), 1.0, ArchiveVersion::V4,).is_err());
    }

    #[test]
    fn v2_common_reader_enforces_source_coordinate_upper_bound() {
        let mut bytes = v2_payload(
            1,
            &[[1.0, 2.0], [0.0, 0.0], [5.0, 0.0], [3.0, 0.0]],
            "user",
            "default",
            false,
            None,
        );
        let point_offset = 1 + 4 + 16 * 8 + 4;
        bytes[point_offset..point_offset + 8]
            .copy_from_slice(&V2_REALLY_BIG_NUMBER.next_up().to_le_bytes());
        assert!(decode(&bytes, V2_LINEAR, 0..bytes.len(), 1.0, ArchiveVersion::V4).is_err());
    }

    fn payload(annotation_type: i32, family: &[u8]) -> Vec<u8> {
        dimension_payload(annotation_type, family, [0; 16], &plane(), None)
    }

    /// One V6+ dimension record payload with parameterized identity and layout.
    ///
    /// `dimstyle_wire` is the mixed-endian style UUID, `plane` the dimension
    /// plane, and `text_point` the authored plane-space text point; `None`
    /// leaves `use_default_text_point` set.
    pub(crate) fn dimension_payload(
        annotation_type: i32,
        family: &[u8],
        dimstyle_wire: [u8; 16],
        plane: &[u8],
        text_point: Option<[f64; 2]>,
    ) -> Vec<u8> {
        dimension_payload_with_arrow_fit(
            annotation_type,
            family,
            dimstyle_wire,
            plane,
            text_point,
            0,
        )
    }

    fn dimension_payload_with_arrow_fit(
        annotation_type: i32,
        family: &[u8],
        dimstyle_wire: [u8; 16],
        plane: &[u8],
        text_point: Option<[f64; 2]>,
        arrow_fit: i32,
    ) -> Vec<u8> {
        let mut text = utf16("<>\n");
        text.extend(self::plane());
        text.extend(0.0_f64.to_le_bytes());
        text.extend(0.0_f64.to_le_bytes());
        text.extend(0_i32.to_le_bytes());
        text.extend(0_i32.to_le_bytes());
        text.extend(1.0_f64.to_le_bytes());
        text.push(0);

        let mut annotation = anonymous(0, &text);
        annotation.extend(dimstyle_wire);
        annotation.extend(plane);
        annotation.extend(annotation_type.to_le_bytes());
        annotation.extend(anonymous(1, &[0]));
        annotation.extend(1.0_f64.to_le_bytes());
        annotation.extend(0.0_f64.to_le_bytes());
        annotation.push(1);

        let mut common = anonymous(4, &annotation);
        common.extend(utf16(""));
        common.extend(0.0_f64.to_le_bytes());
        common.push(u8::from(text_point.is_none()));
        for value in text_point.unwrap_or([0.0, 0.0]) {
            common.extend(value.to_le_bytes());
        }
        common.extend([0, 0]);
        common.extend(arrow_fit.to_le_bytes());
        common.extend([0; 16]);
        common.extend(2.0_f64.to_le_bytes());
        common.extend(0_i32.to_le_bytes());

        let mut outer = anonymous(1, &common);
        outer.extend(family);
        anonymous(0, &outer)
    }

    fn legacy_annotation_payload(kind: i32, points: &[[f64; 2]]) -> Vec<u8> {
        let mut annotation = kind.to_le_bytes().to_vec();
        annotation.extend(0_i32.to_le_bytes());
        annotation.extend(plane());
        annotation.extend((points.len() as i32).to_le_bytes());
        for point in points {
            annotation.extend(point[0].to_le_bytes());
            annotation.extend(point[1].to_le_bytes());
        }
        annotation.extend(utf16("<>"));
        annotation.extend(1_i32.to_le_bytes());
        annotation.extend(4_i32.to_le_bytes());
        annotation.extend(1.5_f64.to_le_bytes());
        annotation.extend(0_i32.to_le_bytes());
        annotation.push(1);
        annotation.extend(utf16("formula"));
        annotation.extend((-1_i32).to_le_bytes());
        annotation.extend(17_i32.to_le_bytes());
        anonymous(3, &annotation)
    }

    fn legacy_payload(kind: i32, points: &[[f64; 2]], family: &[f64]) -> Vec<u8> {
        let mut outer = legacy_annotation_payload(kind, points);
        for value in family {
            outer.extend(value.to_le_bytes());
        }
        anonymous(0, &outer)
    }

    fn direct_legacy_payload(kind: i32, points: &[[f64; 2]], family: &[f64]) -> Vec<u8> {
        let mut bytes = vec![0x10];
        bytes.extend(kind.to_le_bytes());
        bytes.extend(0_i32.to_le_bytes());
        bytes.extend(plane());
        bytes.extend((points.len() as i32).to_le_bytes());
        for point in points {
            bytes.extend(point[0].to_le_bytes());
            bytes.extend(point[1].to_le_bytes());
        }
        bytes.extend(utf16("<>"));
        bytes.extend(0_i32.to_le_bytes());
        bytes.extend(4_i32.to_le_bytes());
        bytes.extend(1.5_f64.to_le_bytes());
        for value in family {
            bytes.extend(value.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn decodes_dimension_families_and_measurements() {
        let archive = ArchiveVersion::V8;
        let linear_family = [3.0_f64, 4.0, 8.0, 9.0]
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        let linear_bytes = payload(1, &linear_family);
        let linear = decode(&linear_bytes, LINEAR, 0..linear_bytes.len(), 10.0, archive)
            .expect("required invariant");
        assert_eq!(linear.measurement, 60.0);
        assert_eq!(linear.horizontal_direction, [1.0, 0.0]);
        let semantic: serde_json::Value =
            serde_json::from_str(&semantic_json(&linear).expect("required invariant"))
                .expect("required invariant");
        assert_eq!(semantic["kind"], "dimension");
        assert_eq!(semantic["runtime_type"], "linear_dimension");
        assert!(
            (semantic["value"].as_f64().expect("required invariant") - 60.0).abs() < 1.0e-12,
            "{semantic:?}"
        );

        let radial_family = [3.0_f64, 4.0, 8.0, 9.0]
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        let radial_bytes = payload(3, &radial_family);
        let radial = decode(&radial_bytes, RADIAL, 0..radial_bytes.len(), 1.0, archive)
            .expect("required invariant");
        assert_eq!(radial.measurement, 20.0);
        let outside_bytes =
            dimension_payload_with_arrow_fit(3, &radial_family, [0; 16], &plane(), None, 2);
        let outside = decode(&outside_bytes, RADIAL, 0..outside_bytes.len(), 1.0, archive)
            .expect("arrows-outside dimension");
        assert_eq!(outside.arrow_position, -1);

        let angular_family = [
            1.0_f64, 0.0, 0.0, 1.0, // directions
            2.0, 3.0, // extension offsets
            1.0, 1.0, // dimension-line point
        ]
        .into_iter()
        .flat_map(f64::to_le_bytes)
        .collect::<Vec<_>>();
        let angular_bytes = payload(2, &angular_family);
        let angular = decode(
            &angular_bytes,
            ANGULAR,
            0..angular_bytes.len(),
            1.0,
            archive,
        )
        .expect("required invariant");
        assert_eq!(angular.measurement, std::f64::consts::FRAC_PI_2);

        let mut ordinate_family = 1_i32.to_le_bytes().to_vec();
        ordinate_family.extend(
            [
                -3.0_f64, 8.0, // definition
                2.0, 12.0, // leader
                1.5, 0.75, // kink offsets
            ]
            .into_iter()
            .flat_map(f64::to_le_bytes),
        );
        let ordinate_bytes = payload(6, &ordinate_family);
        let ordinate = decode(
            &ordinate_bytes,
            ORDINATE,
            0..ordinate_bytes.len(),
            10.0,
            archive,
        )
        .expect("required invariant");
        assert_eq!(ordinate.measurement, 60.0);
        assert!(matches!(
            ordinate.definition,
            Definition::Ordinate {
                definition_point: [-30.0, 80.0],
                leader_point: [20.0, 120.0],
                measured_direction: 1,
                kink_offsets: [15.0, 7.5]
            }
        ));
    }

    #[test]
    fn dimension_family_readers_leave_class_data_suffixes_bounded() {
        let archive = ArchiveVersion::V8;
        let family = [3.0_f64, 4.0, 8.0, 9.0]
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();

        let mut modern = payload(1, &family);
        modern.extend([0xa5, 0x5a]);
        let modern_dimension = decode(&modern, LINEAR, 0..modern.len(), 1.0, archive)
            .expect("modern class-data suffix is bounded");
        assert_eq!(modern_dimension.measurement, 6.0);

        let mut legacy = legacy_payload(
            1,
            &[[0.0, 0.0], [0.0, 5.0], [3.0, 0.0], [3.0, 5.0], [1.0, 5.0]],
            &[],
        );
        legacy.extend([0x3c, 0xc3]);
        let legacy_dimension = decode(&legacy, V5_LINEAR, 0..legacy.len(), 1.0, archive)
            .expect("legacy class-data suffix is bounded");
        assert_eq!(legacy_dimension.measurement, 3.0);
    }

    #[test]
    fn decodes_legacy_dimension_families_into_common_semantics() {
        let archive = ArchiveVersion::V8;
        let linear_bytes = legacy_payload(
            1,
            &[[0.0, 0.0], [0.0, 5.0], [3.0, 0.0], [3.0, 5.0], [1.0, 5.0]],
            &[],
        );
        let linear = decode(
            &linear_bytes,
            V5_LINEAR,
            0..linear_bytes.len(),
            10.0,
            archive,
        )
        .expect("required invariant");
        assert_eq!(linear.measurement, 30.0);
        assert_eq!(linear.annotation_type, 5);
        assert!(linear.allow_text_scaling);
        let DimensionFamily::Legacy { dimstyle_index, .. } = linear.family else {
            panic!("legacy linear dimension");
        };
        assert_eq!(dimstyle_index, 17);
        assert_eq!(linear.user_text, "formula");
        assert!(matches!(
            linear.definition,
            Definition::Linear {
                definition_point: [30.0, 0.0],
                dimension_line_point: [15.0, 50.0]
            }
        ));

        let radial_bytes =
            legacy_payload(4, &[[1.0, 2.0], [4.0, 6.0], [7.0, 8.0], [6.0, 8.0]], &[]);
        let radial = decode(
            &radial_bytes,
            V5_RADIAL,
            0..radial_bytes.len(),
            10.0,
            archive,
        )
        .expect("required invariant");
        assert_eq!(radial.measurement, 100.0);
        assert_eq!(radial.annotation_type, 3);
        assert!(matches!(
            radial.definition,
            Definition::Radial {
                radius_point: [30.0, 40.0],
                dimension_line_point: [60.0, 60.0],
                diameter: true
            }
        ));

        let angular_bytes = legacy_payload(
            3,
            &[[2.0, 2.0], [2.0, 0.0], [0.0, 3.0], [1.0, 1.0]],
            &[std::f64::consts::FRAC_PI_2, 5.0],
        );
        let angular = decode(
            &angular_bytes,
            V5_ANGULAR,
            0..angular_bytes.len(),
            10.0,
            archive,
        )
        .expect("required invariant");
        assert_eq!(angular.measurement, std::f64::consts::FRAC_PI_2);
        let Definition::Angular {
            first_direction,
            second_direction,
            dimension_line_point,
            first_extension_offset,
            second_extension_offset,
        } = angular.definition
        else {
            panic!("expected angular definition");
        };
        assert_eq!(first_direction, [1.0, 0.0]);
        assert!((second_direction[0]).abs() < 1.0e-12);
        assert!((second_direction[1] - 1.0).abs() < 1.0e-12);
        assert!((dimension_line_point[0] - 50.0 / 2.0_f64.sqrt()).abs() < 1.0e-12);
        assert!((dimension_line_point[1] - 50.0 / 2.0_f64.sqrt()).abs() < 1.0e-12);
        assert_eq!(first_extension_offset, -1.0);
        assert_eq!(second_extension_offset, -1.0);

        let center_bytes = payload(8, &4.5_f64.to_le_bytes());
        let center = decode(
            &center_bytes,
            CENTERMARK,
            0..center_bytes.len(),
            10.0,
            archive,
        )
        .expect("required invariant");
        assert_eq!(center.measurement, 0.0);
        assert!(matches!(
            center.definition,
            Definition::CenterMark { radius: 45.0 }
        ));

        let annotation = legacy_annotation_payload(8, &[[4.0, -7.0], [4.0, 2.0]]);
        let mut wrapped = anonymous(0, &annotation);
        wrapped.extend((-1_i32).to_le_bytes());
        wrapped.extend(1.25_f64.to_le_bytes());
        wrapped.extend(0.5_f64.to_le_bytes());
        let ordinate_bytes = anonymous(1, &wrapped);
        let ordinate = decode(
            &ordinate_bytes,
            V5_ORDINATE,
            0..ordinate_bytes.len(),
            10.0,
            archive,
        )
        .expect("required invariant");
        assert_eq!(ordinate.measurement, 40.0);
        assert!(matches!(
            ordinate.definition,
            Definition::Ordinate {
                definition_point: [40.0, -70.0],
                leader_point: [40.0, 20.0],
                measured_direction: 1,
                kink_offsets: [12.5, 5.0]
            }
        ));

        let mut extension = [0_u8; 16].to_vec();
        extension.extend((-1_i32).to_le_bytes());
        extension.extend(0_i32.to_le_bytes());
        extension.extend(2.0_f64.to_le_bytes());
        extension.extend([0_u8; 15]);
        extension.push(42);
        let mut extension = anonymous(2, &extension);
        extension.extend([0x4d, 0xd4]);
        let descriptor = UserdataDescriptor::Known {
            range: 0..extension.len(),
            version: (1, 0),
            class_uuid: V5_DIM_EXTRA,
            item_uuid: V5_DIM_EXTRA,
            copy_count: 1,
            transform_range: 0..0,
            application_uuid: None,
            last_saved_as_goo: None,
            archive_version: None,
            writer_version: None,
            payload_range: 0..extension.len(),
        };
        let mut radial = radial;
        apply_userdata(
            &extension,
            std::slice::from_ref(&descriptor),
            archive,
            1.0,
            &mut radial,
        )
        .expect("required invariant");
        assert_eq!(radial.measurement, 200.0);
        assert_eq!(radial.distance_scale, 2.0);
        assert_eq!(radial.arrow_position, -1);
        assert_eq!(
            radial.detail_measured.to_string(),
            "00000000-0000-0000-0000-00000000002a"
        );

        let mut wrong_item_descriptor = descriptor.clone();
        let crate::objects::UserdataDescriptor::Known { item_uuid, .. } =
            &mut wrong_item_descriptor
        else {
            panic!("expected known userdata");
        };
        *item_uuid = Uuid::nil();
        let mut wrong_item_radial = decode(
            &radial_bytes,
            V5_RADIAL,
            0..radial_bytes.len(),
            1.0,
            archive,
        )
        .expect("fresh radial baseline");
        apply_userdata(
            &extension,
            std::slice::from_ref(&wrong_item_descriptor),
            archive,
            1.0,
            &mut wrong_item_radial,
        )
        .expect("wrong dimension item UUID is not a matching extension");
        assert_eq!(wrong_item_radial.arrow_position, 0);
        assert_eq!(wrong_item_radial.distance_scale, 1.0);
        assert!(wrong_item_radial.detail_measured.is_nil());

        let mut angular_extension =
            anonymous(0, &[2.5_f64.to_le_bytes(), 4.0_f64.to_le_bytes()].concat());
        angular_extension.extend([0x6e, 0xe6]);
        let angular_descriptor = UserdataDescriptor::Known {
            range: 0..angular_extension.len(),
            version: (1, 0),
            class_uuid: V5_ANGULAR_EXTRA,
            item_uuid: V5_ANGULAR_EXTRA,
            copy_count: 1,
            transform_range: 0..0,
            application_uuid: None,
            last_saved_as_goo: None,
            archive_version: None,
            writer_version: None,
            payload_range: 0..angular_extension.len(),
        };
        let mut angular = angular;
        apply_userdata(
            &angular_extension,
            std::slice::from_ref(&angular_descriptor),
            archive,
            10.0,
            &mut angular,
        )
        .expect("required invariant");
        assert!(matches!(
            angular.definition,
            Definition::Angular {
                first_extension_offset: 25.0,
                second_extension_offset: 40.0,
                ..
            }
        ));

        let mut wrong_item_descriptor = angular_descriptor.clone();
        let crate::objects::UserdataDescriptor::Known { item_uuid, .. } =
            &mut wrong_item_descriptor
        else {
            panic!("expected known userdata");
        };
        *item_uuid = Uuid::nil();
        let mut wrong_item_angular = decode(
            &angular_bytes,
            V5_ANGULAR,
            0..angular_bytes.len(),
            10.0,
            archive,
        )
        .expect("fresh angular baseline");
        apply_userdata(
            &angular_extension,
            std::slice::from_ref(&wrong_item_descriptor),
            archive,
            10.0,
            &mut wrong_item_angular,
        )
        .expect("wrong item UUID is not a matching extension");
        assert!(matches!(
            wrong_item_angular.definition,
            Definition::Angular {
                first_extension_offset: -1.0,
                second_extension_offset: -1.0,
                ..
            }
        ));

        let second_extension =
            anonymous(0, &[9.0_f64.to_le_bytes(), 11.0_f64.to_le_bytes()].concat());
        let second_start = angular_extension.len();
        let mut combined = angular_extension.clone();
        combined.extend(second_extension);
        let mut second_descriptor = angular_descriptor.clone();
        let crate::objects::UserdataDescriptor::Known {
            range,
            payload_range,
            ..
        } = &mut second_descriptor
        else {
            panic!("expected known userdata");
        };
        *range = second_start..combined.len();
        *payload_range = second_start..combined.len();
        let mut duplicate_angular = angular;
        apply_userdata(
            &combined,
            &[angular_descriptor, second_descriptor],
            archive,
            10.0,
            &mut duplicate_angular,
        )
        .expect("first duplicate extension");
        assert!(matches!(
            duplicate_angular.definition,
            Definition::Angular {
                first_extension_offset: 25.0,
                second_extension_offset: 40.0,
                ..
            }
        ));
    }

    #[test]
    fn v4_legacy_dimension_writer_bands_match_source() {
        let archive = ArchiveVersion::V4;
        let linear_bytes = direct_legacy_payload(
            1,
            &[[0.0, 0.0], [0.0, 5.0], [3.0, 0.0], [3.0, 5.0], [1.0, 5.0]],
            &[],
        );
        let linear = decode(
            &linear_bytes,
            V5_LINEAR,
            0..linear_bytes.len(),
            10.0,
            archive,
        )
        .expect("V4 linear common payload is direct");
        assert_eq!(linear.measurement, 30.0);

        let radial_bytes = direct_legacy_payload(
            4,
            &[[1.0, 2.0], [4.0, 6.0], [7.0, 8.0], [6.0, 8.0], [7.0, 8.0]],
            &[],
        );
        let radial = decode(
            &radial_bytes,
            V5_RADIAL,
            0..radial_bytes.len(),
            10.0,
            archive,
        )
        .expect("V4 radial common payload is direct");
        assert_eq!(radial.measurement, 100.0);

        let angular_bytes = direct_legacy_payload(
            3,
            &[[2.0, 2.0], [2.0, 0.0], [0.0, 3.0], [1.0, 1.0]],
            &[std::f64::consts::FRAC_PI_2, 5.0],
        );
        let angular = decode(
            &angular_bytes,
            V5_ANGULAR,
            0..angular_bytes.len(),
            10.0,
            archive,
        )
        .expect("V4 angular common payload and suffix are direct");
        assert_eq!(angular.measurement, std::f64::consts::FRAC_PI_2);

        let direct_common = direct_legacy_payload(8, &[[4.0, -7.0], [4.0, 2.0]], &[]);
        let inner = anonymous_v4(0, &direct_common);
        let mut ordinate_body = inner;
        ordinate_body.extend((-1_i32).to_le_bytes());
        ordinate_body.extend(1.25_f64.to_le_bytes());
        ordinate_body.extend(0.5_f64.to_le_bytes());
        let ordinate_outer = anonymous_v4(1, &ordinate_body);
        let ordinate = decode(
            &ordinate_outer,
            V5_ORDINATE,
            0..ordinate_outer.len(),
            10.0,
            archive,
        )
        .expect("V4 ordinate keeps its outer wrapper and direct common child");
        assert_eq!(ordinate.measurement, 40.0);
        assert!(matches!(
            ordinate.definition,
            Definition::Ordinate {
                measured_direction: 1,
                kink_offsets: [12.5, 5.0],
                ..
            }
        ));
    }
}
