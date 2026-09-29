// SPDX-License-Identifier: Apache-2.0
//! Modern Rhino dimension payload decoding.

use crate::loss::Diagnostics;
use std::fmt;
use std::ops::Range;

use crate::chunks::{
    admitted_vec, checked_count_bytes, chunk_at, ArchiveVersion, BoundedReader, FramingError,
};
use crate::objects::{parse_class_wrapper, UserdataDescriptor};
use crate::settings::{plane, utf16_retained, CoordinateLane, MillimeterScale, Plane};
use crate::wire::{scaled_coordinate, uuid, Uuid};
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal, PositiveAngle, PositiveReal};
use cadmpeg_ir::units::FiniteVector;

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
const ANGULAR: Uuid = Uuid::from_canonical([
    0xd4, 0x17, 0x78, 0x6b, 0xf6, 0xcd, 0x4f, 0x12, 0x9e, 0x1f, 0x06, 0x3f, 0x41, 0x4d, 0xbe, 0xb6,
]);
const RADIAL: Uuid = Uuid::from_canonical([
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
const ORDINATE: Uuid = Uuid::from_canonical([
    0x03, 0x12, 0x48, 0x28, 0x4c, 0x9b, 0x4d, 0x28, 0x9a, 0x82, 0x66, 0x4d, 0xdd, 0xe7, 0xa1, 0x4f,
]);
pub(crate) const V5_ORDINATE: Uuid = Uuid::from_canonical([
    0xc8, 0x28, 0x8d, 0x69, 0x5b, 0xd8, 0x4f, 0x50, 0x9b, 0xaf, 0x52, 0x5a, 0x00, 0x86, 0xb0, 0xc3,
]);
const CENTERMARK: Uuid = Uuid::from_canonical([
    0xd4, 0x67, 0x67, 0xba, 0x7e, 0x8f, 0x4d, 0x9d, 0x9a, 0x92, 0x66, 0x05, 0x02, 0x19, 0xa5, 0xb9,
]);
pub(crate) const V2_ANNOTATION: Uuid = Uuid::from_canonical([
    0xab, 0xaf, 0x58, 0x73, 0x41, 0x45, 0x11, 0xd4, 0x80, 0x0f, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const V2_LINEAR: Uuid = Uuid::from_canonical([
    0x5d, 0xe6, 0xb2, 0x0d, 0x48, 0x6b, 0x11, 0xd4, 0x80, 0x14, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const V2_RADIAL: Uuid = Uuid::from_canonical([
    0x5d, 0xe6, 0xb2, 0x0e, 0x48, 0x6b, 0x11, 0xd4, 0x80, 0x14, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const V2_ANGULAR: Uuid = Uuid::from_canonical([
    0x5d, 0xe6, 0xb2, 0x0f, 0x48, 0x6b, 0x11, 0xd4, 0x80, 0x14, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const V2_TEXT_OBJECT: Uuid = Uuid::from_canonical([
    0x5d, 0xe6, 0xb2, 0x10, 0x48, 0x6b, 0x11, 0xd4, 0x80, 0x14, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const V2_LEADER: Uuid = Uuid::from_canonical([
    0x5d, 0xe6, 0xb2, 0x11, 0x48, 0x6b, 0x11, 0xd4, 0x80, 0x14, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
pub(crate) const V2_REALLY_BIG_NUMBER: f64 = 1.0e150;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OrdinateAxis {
    X,
    Y,
}

impl OrdinateAxis {
    fn value(self) -> i32 {
        match self {
            Self::X => 1,
            Self::Y => 2,
        }
    }
}

/// Dimension family and defining plane-space geometry.
#[derive(Debug, Clone, PartialEq)]
enum Definition {
    Linear {
        definition_point: CoordinateLane<2>,
        dimension_line_point: CoordinateLane<2>,
    },
    Angular {
        first_direction: CoordinateLane<2>,
        second_direction: CoordinateLane<2>,
        first_extension_offset: FiniteReal,
        second_extension_offset: FiniteReal,
        dimension_line_point: CoordinateLane<2>,
    },
    Radial {
        radius_point: CoordinateLane<2>,
        dimension_line_point: CoordinateLane<2>,
        diameter: bool,
    },
    Ordinate {
        definition_point: CoordinateLane<2>,
        leader_point: CoordinateLane<2>,
        measured_direction: OrdinateAxis,
        kink_offsets: [FiniteReal; 2],
    },
    CenterMark {
        radius: NonNegativeReal,
    },
}

/// Style and V2 payload exclusive to one dimension family.
#[derive(Debug, Clone, PartialEq)]
enum DimensionFamily {
    /// Pre-V5 dimension with a table index and inline text style.
    Legacy {
        dimstyle_index: i32,
        text_display_mode: i32,
        text_height: NonNegativeReal,
        justification: i32,
    },
    /// V2 dimension with default text and definition points.
    V2 {
        default_text: String,
        points: Vec<FiniteVector<2>>,
        angular_radius: Option<PositiveReal>,
    },
    /// Modern dimension referencing a dimstyle UUID.
    Modern { dimstyle_id: Uuid },
}

/// Complete common and family-specific dimension semantics.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Dimension {
    pub(crate) source_range: Range<usize>,
    annotation_type: i32,
    rich_text: String,
    user_text: String,
    family: DimensionFamily,
    plane: Plane,
    horizontal_direction: CoordinateLane<2>,
    allow_text_scaling: bool,
    use_default_text_point: bool,
    user_text_point: CoordinateLane<2>,
    flip_arrows: [bool; 2],
    arrow_position: i32,
    detail_measured: Uuid,
    distance_scale: PositiveReal,
    definition: Definition,
    measurement: f64,
    pub(crate) override_present: bool,
}

pub(crate) struct Annotation {
    pub(crate) rich_text: String,
    pub(crate) text_rectangle_width: FiniteReal,
    pub(crate) text_rotation_radians: FiniteReal,
    pub(crate) horizontal_alignment: i32,
    pub(crate) vertical_alignment: i32,
    pub(crate) wrapped: bool,
    pub(crate) dimstyle_id: Uuid,
    pub(crate) plane: Plane,
    pub(crate) kind: i32,
    pub(crate) horizontal_direction: CoordinateLane<2>,
    pub(crate) allow_text_scaling: bool,
    override_present: bool,
}

struct TextContent {
    rich_text: String,
    rectangle_width: FiniteReal,
    rotation_radians: FiniteReal,
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

fn scale_plane(
    mut value: Plane,
    scale: MillimeterScale,
    offset: usize,
) -> Result<Plane, FramingError> {
    let origin = value.origin.get();
    let mut scaled = [FiniteReal::ZERO; 3];
    for index in 0..3 {
        scaled[index] = scaled_coordinate(origin[index], scale)
            .ok_or_else(|| FramingError::structural(offset, "scaled dimension plane is invalid"))?;
    }
    value.origin = crate::settings::CoordinateLane::Admitted(scaled.into());
    let constant = scaled_coordinate(value.equation[3], scale)
        .ok_or_else(|| FramingError::structural(offset, "scaled dimension plane is invalid"))?;
    value.equation = value.equation.with_fourth(constant);
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

fn point2(reader: &mut BoundedReader<'_>) -> Result<FiniteVector<2>, FramingError> {
    let value = [reader.f64()?, reader.f64()?];
    FiniteVector::new(value).ok_or_else(|| {
        FramingError::structural(reader.position() - 16, "dimension point is not finite")
    })
}

fn text_content(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<TextContent, FramingError> {
    let (mut text, next, _version) = anonymous(data, reader.position(), reader.end(), archive)?;
    let rich_text = utf16_retained(ctx, &mut text, "Rhino dimension rich text")?;
    plane(&mut text)?;
    let rectangle_width = text.f64()?;
    let rotation_radians = text.f64()?;
    let (Some(rectangle_width), Some(rotation_radians)) = (
        FiniteReal::new(rectangle_width),
        FiniteReal::new(rotation_radians),
    ) else {
        return Err(FramingError::structural(
            text.position() - 16,
            "text layout contains a nonfinite value",
        ));
    };
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Annotation, FramingError> {
    let (mut annotation, next, version) =
        anonymous(data, reader.position(), reader.end(), archive)?;
    let text = text_content(ctx, data, &mut annotation, archive)?;
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
            let mut warnings = Diagnostics::new();
            parse_class_wrapper(
                ctx,
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
        CoordinateLane::Admitted(point2(&mut annotation)?)
    } else {
        CoordinateLane::Admitted([FiniteReal::ONE, FiniteReal::ZERO].into())
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

fn scaled_point(
    value: FiniteVector<2>,
    scale: MillimeterScale,
    offset: usize,
) -> Result<FiniteVector<2>, FramingError> {
    Ok([
        scaled_coordinate(value[0], scale)
            .ok_or_else(|| FramingError::structural(offset, "scaled dimension point is invalid"))?,
        scaled_coordinate(value[1], scale)
            .ok_or_else(|| FramingError::structural(offset, "scaled dimension point is invalid"))?,
    ]
    .into())
}

fn angular_measurement(first: [f64; 2], second: [f64; 2]) -> f64 {
    let first = first[1].atan2(first[0]);
    (second[1].atan2(second[0]) - first).rem_euclid(std::f64::consts::TAU)
}

pub(crate) struct LegacyAnnotation {
    pub(crate) kind: i32,
    pub(crate) text_display_mode: i32,
    pub(crate) plane: Plane,
    pub(crate) points: Vec<FiniteVector<2>>,
    pub(crate) rich_text: String,
    pub(crate) user_text: String,
    pub(crate) user_positioned_text: bool,
    pub(crate) dimstyle_index: i32,
    pub(crate) allow_text_scaling: bool,
    pub(crate) text_height: NonNegativeReal,
    pub(crate) justification: i32,
}

pub(crate) fn legacy_annotation(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
) -> Result<LegacyAnnotation, FramingError> {
    let (mut annotation, next, minor) = anonymous(data, reader.position(), reader.end(), archive)?;
    let value = legacy_annotation_fields(ctx, &mut annotation, scale, minor, false)?;
    annotation.skip_remaining()?;
    reader.skip(next - reader.position())?;
    Ok(value)
}

/// Reads the direct legacy annotation payload used by archive versions 2, 3,
/// and 4. Those archives store a packed version byte and then the common
/// fields without an anonymous wrapper.
pub(crate) fn legacy_annotation_direct(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
) -> Result<LegacyAnnotation, FramingError> {
    let version = reader.u8()?;
    if version >> 4 != 1 || version & 0x0f != 0 {
        return Err(FramingError::structural(
            reader.position() - 1,
            "unsupported direct legacy annotation version",
        ));
    }
    let value = legacy_annotation_fields(ctx, reader, scale, 0, true)?;
    reader.skip_remaining()?;
    Ok(value)
}

fn legacy_annotation_fields(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    annotation: &mut BoundedReader<'_>,
    scale: MillimeterScale,
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
    let mut points = admitted_vec(ctx, point_count, "Rhino legacy annotation points")?;
    for _ in 0..point_count {
        let offset = annotation.position();
        points.push(scaled_point(point2(annotation)?, scale, offset)?);
    }
    let rich_text = utf16_retained(ctx, annotation, "Rhino legacy annotation rich text")?;
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
    let text_height = NonNegativeReal::from_finite(text_height).ok_or_else(|| {
        FramingError::structural(
            annotation.position() - 8,
            "invalid legacy annotation text height",
        )
    })?;
    let justification = if direct_legacy { 0 } else { annotation.i32()? };
    let stored_text_scaling = (!direct_legacy && minor >= 1)
        .then(|| annotation.bool())
        .transpose()?;
    let allow_text_scaling = legacy_text_scaling(stored_text_scaling);
    let user_text = if !direct_legacy && minor >= 2 {
        utf16_retained(ctx, annotation, "Rhino legacy annotation user text")?
    } else {
        crate::wire::copy_retained_string(ctx, &rich_text, "Rhino legacy annotation user text")?
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
        (
            shifted_plane(plane, [0.0, text_height.get()]),
            (1 << 18) | 1,
        )
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
    let mut origin = plane.origin.get();
    for (index, coordinate) in origin.iter_mut().enumerate() {
        *coordinate += point[0] * plane.xaxis[index] + point[1] * plane.yaxis[index];
    }
    plane.origin = crate::settings::CoordinateLane::Derived(origin);
    let mut equation = plane.equation.get();
    equation[3] = -(equation[0] * origin[0] + equation[1] * origin[1] + equation[2] * origin[2]);
    plane.equation = crate::settings::CoordinateLane::Derived(equation);
    plane
}

fn difference(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn world_horizontal_in_plane(plane: &Plane) -> [f64; 2] {
    // ON_DimLinear::Create projects plane.origin + world X onto the plane.
    // Plane axes are orthonormal, so the plane coordinates are these dot products.
    [plane.xaxis[0], plane.yaxis[0]]
}

fn ordinate_direction(stored: i32, definition: [f64; 2], leader: [f64; 2]) -> Option<OrdinateAxis> {
    match stored {
        0 => Some(OrdinateAxis::X),
        1 => Some(OrdinateAxis::Y),
        -1 => Some(inferred_ordinate_direction(definition, leader)),
        _ => None,
    }
}

fn inferred_ordinate_direction(definition: [f64; 2], leader: [f64; 2]) -> OrdinateAxis {
    if (leader[0] - definition[0]).abs() <= (leader[1] - definition[1]).abs() {
        OrdinateAxis::X
    } else {
        OrdinateAxis::Y
    }
}

/// Common fields serialized by every concrete V2 annotation class.
pub(crate) struct V2Annotation {
    pub(crate) kind: i32,
    pub(crate) plane: Plane,
    pub(crate) points: Vec<FiniteVector<2>>,
    pub(crate) user_text: String,
    pub(crate) default_text: String,
    pub(crate) user_positioned_text: bool,
}

/// Reads the direct packed-1.0 V2 annotation prefix.
///
/// The reader stops after `m_userpositionedtext`. The enclosing class-data
/// range owns every subclass field and any future suffix.
pub(crate) fn v2_annotation_direct(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
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
    let mut points = admitted_vec(ctx, point_bytes / 16, "Rhino V2 annotation points")?;
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
    let user_text = utf16_retained(ctx, reader, "Rhino V2 annotation user text")?;
    let default_text = utf16_retained(ctx, reader, "Rhino V2 annotation default text")?;
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
pub(crate) fn v2_effective_text(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    annotation: &V2Annotation,
) -> Result<String, cadmpeg_core::CodecError> {
    let text = if annotation.user_text.is_empty() {
        &annotation.default_text
    } else {
        &annotation.user_text
    };
    crate::wire::copy_retained_string(
        ctx,
        text.trim_matches(|character: char| character.is_whitespace() || character.is_control()),
        "Rhino V2 effective text",
    )
}

enum LegacyDimensionFields {
    Linear,
    Radial,
    Angular {
        angle: NonNegativeReal,
        radius: FiniteReal,
    },
    Ordinate {
        direction: i32,
        kink_offsets: [FiniteReal; 2],
    },
}

fn decode_legacy(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    class: Uuid,
    range: Range<usize>,
    scale: MillimeterScale,
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
        let annotation = legacy_annotation_fields(ctx, &mut reader, scale, 0, true)?;
        (reader, 0, annotation)
    } else {
        // The class reader closes the family child and the enclosing class-data
        // reader owns any direct suffix after that child.
        let (mut outer, _next, minor) = anonymous(data, range.start, range.end, archive)?;
        let annotation = if class == V5_ORDINATE {
            let (mut wrapper, wrapper_next, _wrapper_minor) =
                anonymous(data, outer.position(), outer.end(), archive)?;
            let annotation = if direct_legacy_common {
                legacy_annotation_direct(ctx, &mut wrapper, scale)?
            } else {
                legacy_annotation(ctx, data, &mut wrapper, scale, archive)?
            };
            wrapper.skip_remaining()?;
            outer.skip(wrapper_next - outer.position())?;
            annotation
        } else {
            legacy_annotation(ctx, data, &mut outer, scale, archive)?
        };
        (outer, minor, annotation)
    };
    // The V5 radial writer appends a fifth copy of the dimension-line point
    // for old readers; the source reader removes exactly that fifth point.
    if class == V5_RADIAL && annotation.points.len() == 5 {
        annotation.points.truncate(4);
    }
    let fields = if class == V5_LINEAR {
        LegacyDimensionFields::Linear
    } else if class == V5_RADIAL {
        LegacyDimensionFields::Radial
    } else if class == V5_ANGULAR {
        let angle = outer.f64()?;
        let radius = scaled_coordinate(outer.f64()?, scale).ok_or_else(|| {
            FramingError::structural(outer.position() - 8, "invalid legacy angular radius")
        })?;
        let Some(angle) = NonNegativeReal::new(angle) else {
            return Err(FramingError::structural(
                outer.position() - 16,
                "invalid legacy angular angle",
            ));
        };
        LegacyDimensionFields::Angular { angle, radius }
    } else {
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
            [FiniteReal::ZERO; 2]
        };
        LegacyDimensionFields::Ordinate {
            direction,
            kink_offsets,
        }
    };
    outer.skip_remaining()?;
    let (plane, definition, user_text_point, measurement) = match fields {
        LegacyDimensionFields::Linear => {
            if !matches!(annotation.kind, 1 | 2) || annotation.points.len() != 5 {
                return Err(FramingError::structural(
                    range.start,
                    "invalid legacy linear definition",
                ));
            }
            let origin = annotation.points[0];
            let definition_point = difference(annotation.points[2].get(), origin.get());
            let arrow_midpoint = [
                (annotation.points[1][0] + annotation.points[3][0]) * 0.5,
                (annotation.points[1][1] + annotation.points[3][1]) * 0.5,
            ];
            let dimension_line_point = difference(arrow_midpoint, origin.get());
            (
                shifted_plane(annotation.plane, origin.get()),
                Definition::Linear {
                    definition_point: CoordinateLane::Derived(definition_point),
                    dimension_line_point: CoordinateLane::Derived(dimension_line_point),
                },
                CoordinateLane::Derived(difference(annotation.points[4].get(), origin.get())),
                definition_point[0].abs(),
            )
        }
        LegacyDimensionFields::Radial => {
            if !matches!(annotation.kind, 4 | 5) || annotation.points.len() != 4 {
                return Err(FramingError::structural(
                    range.start,
                    "invalid legacy radial definition",
                ));
            }
            let origin = annotation.points[0];
            let radius_point = difference(annotation.points[1].get(), origin.get());
            let dimension_line_point = difference(annotation.points[2].get(), origin.get());
            let diameter = annotation.kind == 4;
            (
                shifted_plane(annotation.plane, origin.get()),
                Definition::Radial {
                    radius_point: CoordinateLane::Derived(radius_point),
                    dimension_line_point: CoordinateLane::Derived(dimension_line_point),
                    diameter,
                },
                CoordinateLane::Derived(dimension_line_point),
                radius_point[0].hypot(radius_point[1]) * if diameter { 2.0 } else { 1.0 },
            )
        }
        LegacyDimensionFields::Angular { angle, radius } => {
            if annotation.kind != 3 || annotation.points.len() != 4 {
                return Err(FramingError::structural(
                    range.start,
                    "invalid legacy angular definition",
                ));
            }
            let first_direction = [1.0, 0.0];
            let second_direction = [angle.get().cos(), angle.get().sin()];
            let dimension_line_point = [
                radius.get() * (0.5 * angle.get()).cos(),
                radius.get() * (0.5 * angle.get()).sin(),
            ];
            (
                annotation.plane,
                Definition::Angular {
                    first_direction: CoordinateLane::Derived(first_direction),
                    second_direction: CoordinateLane::Derived(second_direction),
                    // ON_OBSOLETE_V5_DimAngular returns -1 when its optional
                    // ON_AngularDimension2Extra userdata is absent.
                    first_extension_offset: FiniteReal::NEG_ONE,
                    second_extension_offset: FiniteReal::NEG_ONE,
                    dimension_line_point: CoordinateLane::Derived(dimension_line_point),
                },
                CoordinateLane::Admitted(annotation.points[0]),
                angle.get(),
            )
        }
        LegacyDimensionFields::Ordinate {
            direction: stored_direction,
            kink_offsets,
        } => {
            if annotation.kind != 8 || annotation.points.len() != 2 {
                return Err(FramingError::structural(
                    range.start,
                    "invalid legacy ordinate definition",
                ));
            }
            let definition_point = annotation.points[0];
            let leader_point = annotation.points[1];
            let measured_direction =
                ordinate_direction(stored_direction, definition_point.get(), leader_point.get())
                    .ok_or_else(|| {
                        FramingError::structural(range.start, "invalid legacy ordinate direction")
                    })?;
            let measurement = if measured_direction == OrdinateAxis::X {
                definition_point[0].abs()
            } else {
                definition_point[1].abs()
            };
            (
                annotation.plane,
                Definition::Ordinate {
                    definition_point: CoordinateLane::Admitted(definition_point),
                    leader_point: CoordinateLane::Admitted(leader_point),
                    measured_direction,
                    kink_offsets,
                },
                CoordinateLane::Admitted(leader_point),
                measurement,
            )
        }
    };
    if !measurement.is_finite() {
        return Err(FramingError::structural(
            range.start,
            "legacy dimension measurement is invalid",
        ));
    }
    let horizontal_direction = CoordinateLane::Derived(world_horizontal_in_plane(&plane));
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
        distance_scale: PositiveReal::ONE,
        definition,
        measurement,
        override_present: false,
    })
}

fn decode_v2(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    class: Uuid,
    range: Range<usize>,
    scale: MillimeterScale,
) -> Result<Dimension, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let annotation = v2_annotation_direct(ctx, &mut reader, scale)?;
    let kind = annotation.kind;
    let points = &annotation.points;
    let mut angular_radius = None;
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
        let definition_point = difference(points[2].get(), origin.get());
        let arrow_midpoint = [
            (points[1][0] + points[3][0]) * 0.5,
            (points[1][1] + points[3][1]) * 0.5,
        ];
        let user_text_point = points
            .get(4)
            .copied()
            .map_or([0.0, 0.0], |point| difference(point.get(), origin.get()));
        (
            shifted_plane(annotation.plane, origin.get()),
            Definition::Linear {
                definition_point: CoordinateLane::Derived(definition_point),
                dimension_line_point: CoordinateLane::Derived(difference(
                    arrow_midpoint,
                    origin.get(),
                )),
            },
            CoordinateLane::Derived(user_text_point),
            true,
            (points[1][0] - points[3][0]).hypot(points[1][1] - points[3][1]),
        )
    } else if class == V2_RADIAL {
        if !matches!(kind, 4 | 5) || points.len() < 3 {
            return Err(FramingError::structural(
                range.start,
                "invalid V2 radial definition",
            ));
        }
        let origin = points[0];
        let radius_point = difference(points[1].get(), origin.get());
        let dimension_line_point = difference(points[2].get(), origin.get());
        let diameter = kind == 4;
        let user_text_point = points
            .get(3)
            .copied()
            .map_or([0.0, 0.0], |point| difference(point.get(), origin.get()));
        (
            shifted_plane(annotation.plane, origin.get()),
            Definition::Radial {
                radius_point: CoordinateLane::Derived(radius_point),
                dimension_line_point: CoordinateLane::Derived(dimension_line_point),
                diameter,
            },
            CoordinateLane::Derived(user_text_point),
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
        // The scaled radius is admitted finite, and the unit scale is finite
        // and positive, so the stored radius is finite too.
        let angle = PositiveAngle::new(angle).filter(|value| value.get() <= V2_REALLY_BIG_NUMBER);
        let radius = PositiveReal::from_finite(radius);
        if angle.is_none()
            || raw_radius <= 0.0
            || raw_radius > V2_REALLY_BIG_NUMBER
            || radius.is_none()
        {
            return Err(FramingError::structural(
                range.start,
                "invalid V2 angular value",
            ));
        }
        let angle = angle
            .ok_or_else(|| FramingError::structural(range.start, "invalid V2 angular value"))?;
        let radius = radius
            .ok_or_else(|| FramingError::structural(range.start, "invalid V2 angular value"))?;
        angular_radius = Some(radius);
        let user_text_point = points
            .get(2)
            .copied()
            .unwrap_or_else(|| [FiniteReal::ZERO, FiniteReal::ZERO].into());
        (
            annotation.plane,
            Definition::Angular {
                first_direction: CoordinateLane::Admitted(points[0]),
                second_direction: CoordinateLane::Admitted(points[1]),
                first_extension_offset: FiniteReal::NEG_ONE,
                second_extension_offset: FiniteReal::NEG_ONE,
                dimension_line_point: CoordinateLane::Derived([
                    radius.get() * (0.5 * angle.get()).cos(),
                    radius.get() * (0.5 * angle.get()).sin(),
                ]),
            },
            CoordinateLane::Admitted(user_text_point),
            !annotation.user_positioned_text,
            angle.get(),
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
        rich_text: v2_effective_text(ctx, &annotation)?,
        user_text: annotation.user_text,
        family: DimensionFamily::V2 {
            default_text: annotation.default_text,
            points: annotation.points,
            angular_radius,
        },
        plane,
        horizontal_direction: CoordinateLane::Derived(world_horizontal_in_plane(&plane)),
        allow_text_scaling: false,
        use_default_text_point,
        user_text_point,
        flip_arrows: [false, false],
        arrow_position: 0,
        detail_measured: Uuid::nil(),
        distance_scale: PositiveReal::ONE,
        definition,
        measurement,
        override_present: false,
    })
}

#[derive(Clone, Copy)]
enum ArrowFitWire {
    Modern,
    Legacy,
}

fn read_arrow_position(
    reader: &mut BoundedReader<'_>,
    wire: ArrowFitWire,
) -> Result<i32, FramingError> {
    let offset = reader.position();
    let value = reader.i32()?;
    match (wire, value) {
        (ArrowFitWire::Modern | ArrowFitWire::Legacy, 0) => Ok(0),
        (ArrowFitWire::Modern | ArrowFitWire::Legacy, 1) => Ok(1),
        (ArrowFitWire::Modern, 2) | (ArrowFitWire::Legacy, -1) => Ok(-1),
        (ArrowFitWire::Modern, _) => Err(FramingError::structural(
            offset,
            "invalid dimension arrow fit",
        )),
        (ArrowFitWire::Legacy, _) => Err(FramingError::structural(
            offset,
            "invalid V5 dimension arrow position",
        )),
    }
}

/// Decodes one modern linear, angular, or radial dimension.
pub(crate) fn decode(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    class: Uuid,
    range: Range<usize>,
    scale: MillimeterScale,
    archive: ArchiveVersion,
) -> Result<Dimension, FramingError> {
    if matches!(class, V2_LINEAR | V2_ANGULAR | V2_RADIAL) {
        return decode_v2(ctx, data, class, range, scale);
    }
    if matches!(class, V5_LINEAR | V5_ANGULAR | V5_RADIAL | V5_ORDINATE) {
        return decode_legacy(ctx, data, class, range, scale, archive);
    }
    // The class reader closes the family child and the enclosing class-data
    // reader owns any direct suffix after that child.
    let (mut outer, _outer_next, _outer_version) =
        anonymous(data, range.start, range.end, archive)?;
    let (mut common, common_next, common_version) =
        anonymous(data, outer.position(), outer.end(), archive)?;
    let mut annotation = annotation(ctx, data, &mut common, archive)?;
    annotation.plane = scale_plane(annotation.plane, scale, range.start)?;
    let user_text = utf16_retained(ctx, &mut common, "Rhino dimension user text")?;
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
    let arrow_position = read_arrow_position(&mut common, ArrowFitWire::Modern)?;
    let detail_measured = uuid(&mut common)?;
    let distance_scale = PositiveReal::new(common.f64()?).ok_or_else(|| {
        FramingError::structural(common.position() - 8, "dimension distance scale is invalid")
    })?;
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
            definition_point: CoordinateLane::Admitted(definition_point),
            dimension_line_point: CoordinateLane::Admitted(dimension_line_point),
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
            first_direction: CoordinateLane::Admitted(first),
            second_direction: CoordinateLane::Admitted(second),
            first_extension_offset,
            second_extension_offset,
            dimension_line_point: CoordinateLane::Admitted(dimension_line_point),
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
            radius_point: CoordinateLane::Admitted(radius_point),
            dimension_line_point: CoordinateLane::Admitted(dimension_line_point),
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
            inferred_ordinate_direction(definition_point.get(), leader_point.get())
        } else if stored_direction == 1 {
            OrdinateAxis::X
        } else {
            OrdinateAxis::Y
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
            definition_point: CoordinateLane::Admitted(definition_point),
            leader_point: CoordinateLane::Admitted(leader_point),
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
            .and_then(NonNegativeReal::from_finite)
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
        } => definition_point[0].abs() * distance_scale.get(),
        Definition::Angular {
            first_direction,
            second_direction,
            ..
        } => angular_measurement(first_direction.get(), second_direction.get()),
        Definition::Radial {
            radius_point,
            diameter,
            ..
        } => {
            radius_point[0].hypot(radius_point[1])
                * distance_scale.get()
                * if *diameter { 2.0 } else { 1.0 }
        }
        Definition::Ordinate {
            definition_point,
            measured_direction,
            ..
        } => {
            (if *measured_direction == OrdinateAxis::X {
                definition_point[0].abs()
            } else {
                definition_point[1].abs()
            }) * distance_scale.get()
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
        user_text_point: CoordinateLane::Admitted(user_text_point),
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
    scale: MillimeterScale,
    dimension: &mut Dimension,
) -> Result<(), FramingError> {
    if let Definition::Angular {
        first_extension_offset,
        second_extension_offset,
        ..
    } = &mut dimension.definition
    {
        if let Some(extra) =
            userdata
                .iter()
                .filter_map(UserdataDescriptor::known)
                .find(|userdata| {
                    userdata.class_uuid == V5_ANGULAR_EXTRA
                        && userdata.item_uuid == V5_ANGULAR_EXTRA
                })
        {
            let (mut reader, _next, _minor) = anonymous(
                data,
                extra.payload_range.start,
                extra.payload_range.end,
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
    let Some(extra) = userdata
        .iter()
        .filter_map(UserdataDescriptor::known)
        .find(|userdata| userdata.class_uuid == V5_DIM_EXTRA && userdata.item_uuid == V5_DIM_EXTRA)
    else {
        return Ok(());
    };
    let (mut reader, _next, minor) = anonymous(
        data,
        extra.payload_range.start,
        extra.payload_range.end,
        archive,
    )?;
    uuid(&mut reader)?;
    let arrow_position = read_arrow_position(&mut reader, ArrowFitWire::Legacy)?;
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
    let distance_scale = PositiveReal::new(distance_scale).ok_or_else(|| {
        FramingError::structural(reader.position() - 8, "invalid V5 dimension distance scale")
    })?;
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
        dimension.measurement *= distance_scale.get();
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
fn insert_dimension_property(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    parameters: &mut std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, String>,
    key: &'static str,
    value: fmt::Arguments<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    ctx.charge_collection_items(1, "Rhino dimension parameter entries")?;
    let key = crate::wire::copy_retained_string(ctx, key, "Rhino dimension parameter key")?;
    let key = cadmpeg_core::text::NonBlankString::new(key)
        .ok_or_else(|| cadmpeg_core::CodecError::malformed("generated dimension key is blank"))?;
    let value = crate::wire::admitted_format(ctx, value, "Rhino dimension parameter value")?;
    parameters.insert(key, value);
    Ok(())
}

struct CommaValues<'a>(&'a [f64]);

impl fmt::Display for CommaValues<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, value) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(",")?;
            }
            write!(f, "{value}")?;
        }
        Ok(())
    }
}

struct SemicolonPoints<'a>(&'a [FiniteVector<2>]);

impl fmt::Display for SemicolonPoints<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, point) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(";")?;
            }
            write!(f, "{},{}", point[0], point[1])?;
        }
        Ok(())
    }
}

pub(crate) fn project(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    dimension: &Dimension,
    key: &str,
    name: Option<&str>,
    object: &str,
    order: u32,
) -> Result<
    (
        cadmpeg_ir::semantic_annotations::SemanticAnnotation,
        Vec<crate::loss::RhinoLossCode>,
    ),
    cadmpeg_core::CodecError,
> {
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
        Definition::CenterMark { radius } => ("center_mark", radius.get()),
    };
    let mut parameters = BTreeMap::new();
    {
        macro_rules! put {
            ($key:expr, $value:expr) => {
                insert_dimension_property(ctx, &mut parameters, $key, $value)
            };
        }
        put!("measurement", format_args!("{}", dimension.measurement))?;
        put!(
            "annotation_type",
            format_args!("{}", dimension.annotation_type)
        )?;
        put!(
            "detail_measured",
            format_args!("{}", dimension.detail_measured)
        )?;
        put!(
            "distance_scale",
            format_args!("{}", dimension.distance_scale.get())
        )?;
        put!("rich_text", format_args!("{}", dimension.rich_text))?;
        put!("user_text", format_args!("{}", dimension.user_text))?;
        put!(
            "use_default_text_point",
            format_args!("{}", dimension.use_default_text_point)
        )?;
        put!(
            "user_text_point",
            format_args!(
                "{},{}",
                dimension.user_text_point[0], dimension.user_text_point[1]
            )
        )?;
        put!(
            "flip_arrows",
            format_args!("{},{}", dimension.flip_arrows[0], dimension.flip_arrows[1])
        )?;
        put!(
            "arrow_position",
            format_args!("{}", dimension.arrow_position)
        )?;
        put!(
            "allow_text_scaling",
            format_args!("{}", dimension.allow_text_scaling)
        )?;
        put!(
            "plane_origin",
            format_args!("{}", CommaValues(&dimension.plane.origin[..]))
        )?;
        put!(
            "plane_x_axis",
            format_args!("{}", CommaValues(&dimension.plane.xaxis[..]))
        )?;
        put!(
            "plane_y_axis",
            format_args!("{}", CommaValues(&dimension.plane.yaxis[..]))
        )?;
        put!(
            "plane_z_axis",
            format_args!("{}", CommaValues(&dimension.plane.zaxis[..]))
        )?;
        put!(
            "plane_equation",
            format_args!("{}", CommaValues(&dimension.plane.equation[..]))
        )?;
        put!(
            "horizontal_direction",
            format_args!("{}", CommaValues(&dimension.horizontal_direction[..]))
        )?;
        match &dimension.family {
            DimensionFamily::Modern { dimstyle_id } => {
                put!("dimstyle_id", format_args!("{dimstyle_id}"))?;
            }
            DimensionFamily::Legacy {
                dimstyle_index,
                text_display_mode,
                text_height,
                justification,
            } => {
                put!("dimstyle_index", format_args!("{dimstyle_index}"))?;
                put!("text_display_mode", format_args!("{text_display_mode}"))?;
                put!("text_height", format_args!("{}", text_height.get()))?;
                put!("justification", format_args!("{justification}"))?;
            }
            DimensionFamily::V2 {
                default_text,
                points,
                angular_radius,
            } => {
                put!("v2_default_text", format_args!("{default_text}"))?;
                put!("v2_points", format_args!("{}", SemicolonPoints(points)))?;
                if let Some(radius) = angular_radius {
                    let angle = dimension.measurement;
                    put!("v2_angle_radians", format_args!("{angle}"))?;
                    put!(
                        "v2_numeric_value_degrees",
                        format_args!("{}", angle * 180.0 / std::f64::consts::PI)
                    )?;
                    put!("v2_radius", format_args!("{}", radius.get()))?;
                }
            }
        }
        match &dimension.definition {
            Definition::Linear {
                definition_point,
                dimension_line_point,
            } => {
                put!(
                    "definition_point",
                    format_args!("{},{}", definition_point[0], definition_point[1])
                )?;
                put!(
                    "dimension_line_point",
                    format_args!("{},{}", dimension_line_point[0], dimension_line_point[1])
                )?;
            }
            Definition::Angular {
                first_direction,
                second_direction,
                first_extension_offset,
                second_extension_offset,
                dimension_line_point,
            } => {
                put!(
                    "first_direction",
                    format_args!("{},{}", first_direction[0], first_direction[1])
                )?;
                put!(
                    "second_direction",
                    format_args!("{},{}", second_direction[0], second_direction[1])
                )?;
                put!(
                    "first_extension_offset",
                    format_args!("{}", first_extension_offset.get())
                )?;
                put!(
                    "second_extension_offset",
                    format_args!("{}", second_extension_offset.get())
                )?;
                put!(
                    "dimension_line_point",
                    format_args!("{},{}", dimension_line_point[0], dimension_line_point[1])
                )?;
            }
            Definition::Radial {
                radius_point,
                dimension_line_point,
                ..
            } => {
                put!(
                    "radius_point",
                    format_args!("{},{}", radius_point[0], radius_point[1])
                )?;
                put!(
                    "dimension_line_point",
                    format_args!("{},{}", dimension_line_point[0], dimension_line_point[1])
                )?;
            }
            Definition::Ordinate {
                definition_point,
                leader_point,
                measured_direction,
                kink_offsets,
            } => {
                put!(
                    "definition_point",
                    format_args!("{},{}", definition_point[0], definition_point[1])
                )?;
                put!(
                    "leader_point",
                    format_args!("{},{}", leader_point[0], leader_point[1])
                )?;
                put!(
                    "measured_direction",
                    format_args!("{}", measured_direction.value())
                )?;
                put!(
                    "kink_offsets",
                    format_args!("{},{}", kink_offsets[0].get(), kink_offsets[1].get())
                )?;
            }
            Definition::CenterMark { radius } => {
                put!("radius", format_args!("{}", radius.get()))?;
            }
        }
    }
    if let Some(name) = name {
        insert_dimension_property(ctx, &mut parameters, "object_name", format_args!("{name}"))?;
    }

    // Model-space text point via the dimension plane (stored UV, not world xyz).
    let position = (!dimension.use_default_text_point)
        .then(|| {
            let [u, v] = dimension.user_text_point.get();
            let origin = dimension.plane.origin.get();
            let x_axis = dimension.plane.xaxis.get();
            let y_axis = dimension.plane.yaxis.get();
            [0, 1, 2].map(|axis| origin[axis] + u * x_axis[axis] + v * y_axis[axis])
        })
        .filter(|point| point.iter().all(|value| value.is_finite()));

    // dimstyle/detail targets are not resolvable here: nil -> null reference;
    // non-nil -> charge and keep the raw UUID in parameters.
    let mut references = BTreeMap::new();
    let mut unresolved = Vec::new();
    let mut reference = |role: &'static str,
                         id: Option<Uuid>,
                         code: RhinoLossCode|
     -> Result<(), cadmpeg_core::CodecError> {
        match id {
            None => Ok(()),
            Some(id) if id.is_nil() => {
                ctx.charge_collection_items(1, "Rhino dimension reference entries")?;
                let role =
                    crate::wire::copy_retained_string(ctx, role, "Rhino dimension reference key")?;
                let role = cadmpeg_core::text::NonBlankString::new(role).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "generated dimension reference role is blank",
                    )
                })?;
                let mut selections = crate::wire::admitted_collection(
                    ctx,
                    1,
                    "Rhino dimension reference selections",
                )?;
                selections.push(ReferenceSelection::new(ReferenceTarget::Null, Vec::new()));
                references.insert(role, selections);
                Ok(())
            }
            Some(_) => {
                crate::wire::reserve_collection(
                    ctx,
                    &mut unresolved,
                    1,
                    "Rhino unresolved dimension references",
                )?;
                unresolved.push(code);
                Ok(())
            }
        }
    };
    reference(
        "dimstyle_id",
        match &dimension.family {
            DimensionFamily::Modern { dimstyle_id } => Some(*dimstyle_id),
            DimensionFamily::Legacy { .. } | DimensionFamily::V2 { .. } => None,
        },
        RhinoLossCode::DimensionStyleUnresolved,
    )?;
    reference(
        "detail_measured",
        Some(dimension.detail_measured),
        RhinoLossCode::DimensionDetailReferenceUnresolved,
    )?;

    let value = cadmpeg_ir::scalar::FiniteReal::new(value)
        .ok_or_else(|| cadmpeg_core::CodecError::malformed("dimension value must be finite"))?;
    let position = position
        .map(|value| {
            cadmpeg_ir::units::FiniteVector::new(value).ok_or_else(|| {
                cadmpeg_core::CodecError::malformed("dimension position must be finite")
            })
        })
        .transpose()?;
    let key = cadmpeg_ir::ids::IdentityKey::try_new(crate::wire::copy_retained_string(
        ctx,
        key,
        "Rhino dimension identity key",
    )?)
    .map_err(|error| cadmpeg_core::CodecError::malformed(error.to_string()))?;
    let annotation_id = SemanticAnnotationId::try_from(crate::wire::admitted_format(
        ctx,
        format_args!("rhino:dimension:annotation#{}", key.as_str()),
        "Rhino dimension annotation identity",
    )?)
    .map_err(|error| cadmpeg_core::CodecError::malformed(error.to_string()))?;
    let mut text = Vec::new();
    if !dimension.user_text.is_empty() {
        crate::wire::reserve_collection(ctx, &mut text, 1, "Rhino dimension annotation text")?;
        text.push(crate::wire::copy_retained_string(
            ctx,
            &dimension.user_text,
            "Rhino dimension annotation text copy",
        )?);
    }
    let annotation = SemanticAnnotation {
        id: annotation_id,
        object: crate::wire::copy_retained_string(
            ctx,
            object,
            "Rhino dimension annotation object",
        )?,
        kind: SemanticAnnotationKind::Dimension,
        runtime_type: crate::wire::copy_retained_string(
            ctx,
            runtime_type,
            "Rhino dimension runtime type",
        )?,
        order,
        text,
        references,
        value: Some(value),
        format: (!dimension.rich_text.is_empty())
            .then(|| {
                crate::wire::copy_retained_string(
                    ctx,
                    &dimension.rich_text,
                    "Rhino dimension format text",
                )
            })
            .transpose()?,
        position,
        parameters,
        assets: Vec::new(),
        native_ref: crate::wire::copy_retained_string(
            ctx,
            object,
            "Rhino dimension native reference",
        )?,
    };
    Ok((annotation, unresolved))
}

/// Serializes one decoded dimension without source-record identity.
pub(crate) fn semantic_json(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    dimension: &Dimension,
) -> Result<String, cadmpeg_core::CodecError> {
    struct DimensionJson<'a>(&'a cadmpeg_ir::semantic_annotations::SemanticAnnotation);
    impl serde::Serialize for DimensionJson<'_> {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeMap;
            let mut map = serializer.serialize_map(Some(7))?;
            map.serialize_entry("format", &self.0.format)?;
            map.serialize_entry("kind", "dimension")?;
            map.serialize_entry("parameters", &self.0.parameters)?;
            map.serialize_entry("position", &self.0.position)?;
            map.serialize_entry("references", &self.0.references)?;
            map.serialize_entry("runtime_type", &self.0.runtime_type)?;
            map.serialize_entry("value", &self.0.value)?;
            map.end()
        }
    }
    let (annotation, _) = project(ctx, dimension, "embedded-history-dimension", None, "", 0)?;
    crate::wire::admitted_canonical_json(
        ctx,
        &DimensionJson(&annotation),
        "Rhino dimension semantic JSON",
    )
}

#[cfg(test)]
pub(crate) mod tests {

    #[test]
    fn dimension_point_reader_holds_finite_lanes_and_preserves_refusal_offset() {
        let mut bytes = vec![0xa5, 0xa5, 0xa5];
        for value in [2.0_f64, -3.0] {
            bytes.extend(value.to_le_bytes());
        }
        let mut reader = BoundedReader::new(&bytes, 3, bytes.len()).expect("point reader");
        let point = super::point2(&mut reader).expect("finite dimension point");
        assert_eq!(point.get(), [2.0, -3.0]);

        bytes[11..19].copy_from_slice(&f64::NAN.to_le_bytes());
        let mut reader = BoundedReader::new(&bytes, 3, bytes.len()).expect("point reader");
        assert_eq!(
            super::point2(&mut reader).expect_err("nonfinite point"),
            crate::chunks::FramingError::structural(3, "dimension point is not finite")
        );
    }

    #[test]
    fn shifted_plane_keeps_computed_overflow_outside_source_admission() {
        let bytes = plane_bytes(
            [f64::MAX, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        );
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("plane reader");
        let plane = crate::settings::plane(&mut reader).expect("finite source plane");
        let shifted = super::shifted_plane(plane, [f64::MAX, 0.0]);
        assert!(matches!(
            shifted.origin,
            crate::settings::CoordinateLane::Derived(_)
        ));
        assert!(shifted.origin[0].is_infinite());
        assert!(matches!(
            shifted.equation,
            crate::settings::CoordinateLane::Derived(_)
        ));
        assert!(shifted.equation[3].is_nan());
    }

    #[test]
    fn numerical_ranges_v2_linear_dimension_preserves_tiny_length() {
        let bytes = v2_payload(
            1,
            &[[0., 0.], [0., 0.], [1., 0.], [1e-200, 0.], [0., 1.]],
            "",
            "",
            false,
            None,
        );
        let value = test_decode(
            &bytes,
            V2_LINEAR,
            0..bytes.len(),
            crate::test_support::millimeter_scale(1.),
            ArchiveVersion::V4,
        )
        .unwrap();
        assert_eq!(value.measurement, 1e-200);
    }

    use super::{
        angular_measurement, apply_userdata, legacy_text_scaling, modern_annotation_type,
        semantic_json, v2_annotation_direct, v2_effective_text, Definition, DimensionFamily,
        OrdinateAxis, ANGULAR, ANONYMOUS, CENTERMARK, LINEAR, ORDINATE, RADIAL, V2_ANGULAR,
        V2_LINEAR, V2_RADIAL, V2_REALLY_BIG_NUMBER, V5_ANGULAR, V5_ANGULAR_EXTRA, V5_DIM_EXTRA,
        V5_LINEAR, V5_ORDINATE, V5_RADIAL,
    };
    use crate::chunks::{ArchiveVersion, BoundedReader};
    use crate::objects::ClassUserdata;
    use crate::objects::UserdataDescriptor;
    use crate::settings::MillimeterScale;
    use crate::test_support::test_dump::{
        crc_chunk, object_record_with_payload, scan_with_objects, utf16_bytes,
    };
    use crate::wire::Uuid;
    use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};

    fn with_test_context<R>(
        data: &[u8],
        apply: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> R,
    ) -> R {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
            .expect("dimension fixture fits root limit");
        apply(&ctx)
    }

    fn test_decode(
        data: &[u8],
        class: Uuid,
        range: std::ops::Range<usize>,
        scale: MillimeterScale,
        archive: ArchiveVersion,
    ) -> Result<super::Dimension, crate::chunks::FramingError> {
        with_test_context(data, |ctx| {
            super::decode(ctx, data, class, range, scale, archive)
        })
    }

    fn with_collection_limit<R>(
        data: &[u8],
        limit: u64,
        apply: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> R,
    ) -> R {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
            .expect("dimension fixture fits root limit");
        apply(&ctx)
    }

    fn with_retained_limit<R>(
        data: &[u8],
        limit: u64,
        apply: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> R,
    ) -> R {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = limit;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
            .expect("dimension fixture fits root limit");
        apply(&ctx)
    }

    fn assert_resource(error: &crate::chunks::FramingError, operation: &str) {
        assert!(
            matches!(error, crate::chunks::FramingError::Resource(limit) if limit.operation == operation),
            "expected {operation}, got {error:?}"
        );
    }

    #[test]
    fn v2_user_text_refuses_retained_limit() {
        let bytes = v2_payload(7, &[], "user", "default", false, None);
        let error = with_retained_limit(&bytes, 0, |ctx| {
            let mut reader =
                BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded V2 payload");
            v2_annotation_direct(ctx, &mut reader, MillimeterScale::IDENTITY)
                .err()
                .expect("user text exceeds retained limit")
        });
        assert_resource(&error, "Rhino V2 annotation user text");
    }

    #[test]
    fn v2_default_text_refuses_retained_limit() {
        let bytes = v2_payload(7, &[], "user", "default", false, None);
        let error = with_retained_limit(&bytes, 4, |ctx| {
            let mut reader =
                BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded V2 payload");
            v2_annotation_direct(ctx, &mut reader, MillimeterScale::IDENTITY)
                .err()
                .expect("default text exceeds retained limit")
        });
        assert_resource(&error, "Rhino V2 annotation default text");
    }

    #[test]
    fn v2_effective_text_refuses_retained_limit() {
        let bytes = v2_payload(7, &[], "  user  ", "default", false, None);
        let annotation = with_test_context(&bytes, |ctx| {
            let mut reader =
                BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded V2 payload");
            v2_annotation_direct(ctx, &mut reader, MillimeterScale::IDENTITY)
        })
        .expect("V2 annotation admitted");
        let error = with_retained_limit(&bytes, 0, |ctx| {
            v2_effective_text(ctx, &annotation).expect_err("effective copy exceeds retained limit")
        });
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino V2 effective text"
        ));
    }

    #[test]
    fn legacy_rich_text_refuses_retained_limit() {
        let bytes = direct_legacy_payload(7, &[], &[]);
        let error = with_retained_limit(&bytes, 0, |ctx| {
            let mut reader =
                BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded legacy payload");
            super::legacy_annotation_direct(ctx, &mut reader, MillimeterScale::IDENTITY)
                .err()
                .expect("rich text exceeds retained limit")
        });
        assert_resource(&error, "Rhino legacy annotation rich text");
    }

    #[test]
    fn legacy_user_text_copy_refuses_retained_limit() {
        let bytes = direct_legacy_payload(7, &[], &[]);
        let error = with_retained_limit(&bytes, 2, |ctx| {
            let mut reader =
                BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded legacy payload");
            super::legacy_annotation_direct(ctx, &mut reader, MillimeterScale::IDENTITY)
                .err()
                .expect("user text copy exceeds retained limit")
        });
        assert_resource(&error, "Rhino legacy annotation user text");
    }

    #[test]
    fn legacy_user_text_field_refuses_retained_limit() {
        let bytes = legacy_annotation_payload(7, &[]);
        let error = with_retained_limit(&bytes, 2, |ctx| {
            let mut reader =
                BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded legacy payload");
            super::legacy_annotation(
                ctx,
                &bytes,
                &mut reader,
                MillimeterScale::IDENTITY,
                ArchiveVersion::V8,
            )
            .err()
            .expect("user text field exceeds retained limit")
        });
        assert_resource(&error, "Rhino legacy annotation user text");
    }

    #[test]
    fn modern_dimension_rich_text_refuses_retained_limit() {
        let family = [3.0_f64, 4.0, 8.0, 9.0]
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        let bytes = payload(1, &family);
        let error = with_retained_limit(&bytes, 0, |ctx| {
            super::decode(
                ctx,
                &bytes,
                LINEAR,
                0..bytes.len(),
                MillimeterScale::IDENTITY,
                ArchiveVersion::V8,
            )
            .expect_err("rich text exceeds retained limit")
        });
        assert_resource(&error, "Rhino dimension rich text");
    }

    #[test]
    fn modern_dimension_user_text_refuses_retained_limit() {
        let family = [3.0_f64, 4.0, 8.0, 9.0]
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        let bytes =
            dimension_payload_with_arrow_fit(1, &family, [0; 16], &plane(), None, 0, "user");
        let error = with_retained_limit(&bytes, 3, |ctx| {
            super::decode(
                ctx,
                &bytes,
                LINEAR,
                0..bytes.len(),
                MillimeterScale::IDENTITY,
                ArchiveVersion::V8,
            )
            .expect_err("user text exceeds retained limit")
        });
        assert_resource(&error, "Rhino dimension user text");
        assert!(test_decode(
            &bytes,
            LINEAR,
            0..bytes.len(),
            MillimeterScale::IDENTITY,
            ArchiveVersion::V8,
        )
        .is_ok());
    }

    #[test]
    fn legacy_annotation_points_refuse_collection_limit() {
        let bytes = direct_legacy_payload(7, &[[1.0, 2.0], [3.0, 4.0]], &[]);
        let refusal = with_collection_limit(&bytes, 1, |ctx| {
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded payload");
            super::legacy_annotation_direct(ctx, &mut reader, MillimeterScale::IDENTITY)
                .err()
                .expect("two points exceed one collection item")
        });
        assert!(matches!(
            refusal,
            crate::chunks::FramingError::Resource(limit)
                if limit.operation == "Rhino legacy annotation points"
        ));
    }

    #[test]
    fn v2_annotation_points_refuse_collection_limit() {
        let bytes = v2_payload(7, &[[1.0, 2.0], [3.0, 4.0]], "", "", false, None);
        let refusal = with_collection_limit(&bytes, 1, |ctx| {
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("bounded payload");
            v2_annotation_direct(ctx, &mut reader, MillimeterScale::IDENTITY)
                .err()
                .expect("two points exceed one collection item")
        });
        assert!(matches!(
            refusal,
            crate::chunks::FramingError::Resource(limit)
                if limit.operation == "Rhino V2 annotation points"
        ));
    }

    #[test]
    fn dimension_decode_propagates_v2_point_limit() {
        let payload = v2_payload(
            1,
            &[[1.0, 2.0], [0.0, 0.0], [5.0, 0.0], [3.0, 0.0], [7.0, 4.0]],
            "user",
            "default",
            false,
            None,
        );
        let object =
            object_record_with_payload(ArchiveVersion::V5, 1, V2_LINEAR.to_wire(), &payload);
        let scan = scan_with_objects(&[object]);
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 8;
        let (ctx, root) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)
                .expect("fixture fits root limit");
        let refusal = crate::decode::decode(&scan, crate::mesh::MeshExpand::new(&ctx, root))
            .expect_err("dimension points exceed collection limit");
        assert!(matches!(
            refusal,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino V2 annotation points"
        ));
        let service_arena = cadmpeg_core::decode::DecodeArena::new();
        let service_policy = cadmpeg_core::decode::DecodePolicy::service();
        let (service_ctx, service_root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            scan.data,
            &service_arena,
            &service_policy,
        )
        .expect("fixture fits root limit");
        let service = crate::decode::decode(
            &scan,
            crate::mesh::MeshExpand::new(&service_ctx, service_root),
        );
        assert!(service.is_ok(), "service dimension decode: {service:?}");
    }

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

    fn anonymous(version: i32, suffix: &[u8]) -> Vec<u8> {
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(version.to_le_bytes());
        body.extend(suffix);
        crc_chunk(ArchiveVersion::V5, ANONYMOUS, &body)
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
        bytes.extend(utf16_bytes(user_text));
        bytes.extend(utf16_bytes(default_text));
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
        let annotation = with_test_context(&bytes, |ctx| {
            v2_annotation_direct(ctx, &mut reader, crate::test_support::millimeter_scale(2.0))
        })
        .expect("V2 common prefix");
        assert_eq!(annotation.points[0], [2.0, 4.0]);
        assert_eq!(annotation.user_text, "  user <>  ");
        assert_eq!(annotation.default_text, "default");
        assert!(!annotation.user_positioned_text);
        assert_eq!(reader.remaining(), 2);
        assert_eq!(
            v2_effective_text(&cadmpeg_test_support::service_decode_context(), &annotation)
                .expect("effective V2 text"),
            "user <>"
        );
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
        let linear = test_decode(
            &linear_bytes,
            V2_LINEAR,
            0..linear_bytes.len(),
            crate::test_support::millimeter_scale(2.0),
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
        let radial = test_decode(
            &radial_bytes,
            V2_RADIAL,
            0..radial_bytes.len(),
            crate::test_support::millimeter_scale(2.0),
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
        let angular = test_decode(
            &angular_bytes,
            V2_ANGULAR,
            0..angular_bytes.len(),
            crate::test_support::millimeter_scale(2.0),
            ArchiveVersion::V4,
        )
        .expect("V2 angular dimension");
        assert_eq!(angular.measurement, 1.25);
        let DimensionFamily::V2 { angular_radius, .. } = angular.family else {
            panic!("V2 angular dimension");
        };
        assert_eq!(angular_radius.map(|_| angular.measurement), Some(1.25));
        assert_eq!(angular_radius.map(PositiveReal::get), Some(19.0));
        assert!(!angular.use_default_text_point);
        assert_eq!(angular.user_text_point.get(), [4.0, 6.0]);
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
        assert!(test_decode(
            &bytes,
            V2_ANGULAR,
            0..bytes.len(),
            MillimeterScale::IDENTITY,
            ArchiveVersion::V4,
        )
        .is_err());
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
        assert!(test_decode(
            &bytes,
            V2_ANGULAR,
            0..bytes.len(),
            MillimeterScale::IDENTITY,
            ArchiveVersion::V4,
        )
        .is_err());
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
        assert!(test_decode(
            &bytes,
            V2_LINEAR,
            0..bytes.len(),
            MillimeterScale::IDENTITY,
            ArchiveVersion::V4
        )
        .is_err());
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
            "",
        )
    }

    fn dimension_payload_with_arrow_fit(
        annotation_type: i32,
        family: &[u8],
        dimstyle_wire: [u8; 16],
        plane: &[u8],
        text_point: Option<[f64; 2]>,
        arrow_fit: i32,
        user_text: &str,
    ) -> Vec<u8> {
        let mut text = utf16_bytes("<>\n");
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
        common.extend(utf16_bytes(user_text));
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

    #[test]
    fn modern_distance_scale_refuses_zero_and_nonfinite_source_values() {
        let family = [3.0_f64, 4.0, 8.0, 9.0]
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        for refused in [0.0_f64, f64::INFINITY] {
            let mut bytes = payload(3, &family);
            let scale = 2.0_f64.to_le_bytes();
            let offset = bytes
                .windows(scale.len())
                .enumerate()
                .filter_map(|(index, window)| (window == scale).then_some(index))
                .next_back()
                .expect("distance scale in source bytes");
            bytes[offset..offset + 8].copy_from_slice(&refused.to_le_bytes());
            let result = test_decode(
                &bytes,
                RADIAL,
                0..bytes.len(),
                MillimeterScale::IDENTITY,
                ArchiveVersion::V8,
            );
            assert!(matches!(
                result,
                Err(crate::chunks::FramingError::Structural { offset: failed, message })
                    if failed == offset && message == "dimension distance scale is invalid"
            ));
        }
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
        annotation.extend(utf16_bytes("<>"));
        annotation.extend(1_i32.to_le_bytes());
        annotation.extend(4_i32.to_le_bytes());
        annotation.extend(1.5_f64.to_le_bytes());
        annotation.extend(0_i32.to_le_bytes());
        annotation.push(1);
        annotation.extend(utf16_bytes("formula"));
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
        bytes.extend(utf16_bytes("<>"));
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
        let linear = test_decode(
            &linear_bytes,
            LINEAR,
            0..linear_bytes.len(),
            crate::test_support::millimeter_scale(10.0),
            archive,
        )
        .expect("required invariant");
        assert_eq!(linear.measurement, 60.0);
        assert_eq!(linear.horizontal_direction.get(), [1.0, 0.0]);
        let semantic: serde_json::Value = serde_json::from_str(
            &semantic_json(&cadmpeg_test_support::service_decode_context(), &linear)
                .expect("required invariant"),
        )
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
        let radial = test_decode(
            &radial_bytes,
            RADIAL,
            0..radial_bytes.len(),
            MillimeterScale::IDENTITY,
            archive,
        )
        .expect("required invariant");
        assert_eq!(radial.measurement, 20.0);
        let outside_bytes =
            dimension_payload_with_arrow_fit(3, &radial_family, [0; 16], &plane(), None, 2, "");
        let outside = test_decode(
            &outside_bytes,
            RADIAL,
            0..outside_bytes.len(),
            MillimeterScale::IDENTITY,
            archive,
        )
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
        let angular = test_decode(
            &angular_bytes,
            ANGULAR,
            0..angular_bytes.len(),
            MillimeterScale::IDENTITY,
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
        let ordinate = test_decode(
            &ordinate_bytes,
            ORDINATE,
            0..ordinate_bytes.len(),
            crate::test_support::millimeter_scale(10.0),
            archive,
        )
        .expect("required invariant");
        assert_eq!(ordinate.measurement, 60.0);
        assert!(matches!(
            ordinate.definition,
            Definition::Ordinate {
                definition_point,
                leader_point,
                measured_direction: OrdinateAxis::X,
                kink_offsets,
            } if definition_point.get() == [-30.0, 80.0]
                && leader_point.get() == [20.0, 120.0]
                && kink_offsets.map(FiniteReal::get) == [15.0, 7.5]
        ));
    }

    #[test]
    fn dimension_projection_refuses_parameter_limit_and_preserves_semantic_json() {
        let family = [3.0_f64, 4.0, 8.0, 9.0]
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        let bytes = payload(1, &family);
        let dimension = test_decode(
            &bytes,
            LINEAR,
            0..bytes.len(),
            crate::test_support::millimeter_scale(10.0),
            ArchiveVersion::V8,
        )
        .expect("valid linear dimension");
        let refusal = with_collection_limit(&[], 0, |ctx| {
            super::project(ctx, &dimension, "test", None, "", 0)
                .expect_err("first parameter exceeds zero items")
        });
        assert!(matches!(
            refusal,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "Rhino dimension parameter entries"
        ));
        let ctx = cadmpeg_test_support::service_decode_context();
        let (annotation, _) =
            super::project(&ctx, &dimension, "embedded-history-dimension", None, "", 0)
                .expect("service profile admits projection");
        let baseline = serde_json::json!({
            "kind": "dimension",
            "runtime_type": annotation.runtime_type,
            "value": annotation.value,
            "format": annotation.format,
            "position": annotation.position,
            "references": annotation.references,
            "parameters": annotation.parameters,
        })
        .to_string();
        let semantic =
            super::semantic_json(&cadmpeg_test_support::service_decode_context(), &dimension)
                .expect("service profile admits semantic JSON");
        assert_eq!(semantic, baseline);
    }

    #[test]
    fn unknown_modern_arrow_fit_is_malformed_and_default_is_zero() {
        let family = [3.0_f64, 4.0, 8.0, 9.0]
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        for (wire, expected) in [(0, 0), (1, 1), (2, -1)] {
            let bytes =
                dimension_payload_with_arrow_fit(3, &family, [0; 16], &plane(), None, wire, "");
            let dimension = test_decode(
                &bytes,
                RADIAL,
                0..bytes.len(),
                MillimeterScale::IDENTITY,
                ArchiveVersion::V8,
            )
            .expect("admitted arrow fit");
            assert_eq!(dimension.arrow_position, expected);
        }
        let bytes = dimension_payload_with_arrow_fit(3, &family, [0; 16], &plane(), None, 3, "");
        let error = test_decode(
            &bytes,
            RADIAL,
            0..bytes.len(),
            MillimeterScale::IDENTITY,
            ArchiveVersion::V8,
        )
        .expect_err("unknown arrow fit");
        assert!(error.to_string().contains("arrow fit"), "{error}");

        for (wire, expected) in [(-1_i32, Some(-1)), (0, Some(0)), (1, Some(1)), (2, None)] {
            let bytes = wire.to_le_bytes();
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("arrow field");
            assert_eq!(
                super::read_arrow_position(&mut reader, super::ArrowFitWire::Legacy).ok(),
                expected
            );
        }
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
        let modern_dimension = test_decode(
            &modern,
            LINEAR,
            0..modern.len(),
            MillimeterScale::IDENTITY,
            archive,
        )
        .expect("modern class-data suffix is bounded");
        assert_eq!(modern_dimension.measurement, 6.0);

        let mut legacy = legacy_payload(
            1,
            &[[0.0, 0.0], [0.0, 5.0], [3.0, 0.0], [3.0, 5.0], [1.0, 5.0]],
            &[],
        );
        legacy.extend([0x3c, 0xc3]);
        let legacy_dimension = test_decode(
            &legacy,
            V5_LINEAR,
            0..legacy.len(),
            MillimeterScale::IDENTITY,
            archive,
        )
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
        let linear = test_decode(
            &linear_bytes,
            V5_LINEAR,
            0..linear_bytes.len(),
            crate::test_support::millimeter_scale(10.0),
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
                definition_point,
                dimension_line_point
            } if definition_point.get() == [30.0, 0.0]
                && dimension_line_point.get() == [15.0, 50.0]
        ));

        let radial_bytes =
            legacy_payload(4, &[[1.0, 2.0], [4.0, 6.0], [7.0, 8.0], [6.0, 8.0]], &[]);
        let radial = test_decode(
            &radial_bytes,
            V5_RADIAL,
            0..radial_bytes.len(),
            crate::test_support::millimeter_scale(10.0),
            archive,
        )
        .expect("required invariant");
        assert_eq!(radial.measurement, 100.0);
        assert_eq!(radial.annotation_type, 3);
        assert!(matches!(
            radial.definition,
            Definition::Radial {
                radius_point,
                dimension_line_point,
                diameter: true
            } if radius_point.get() == [30.0, 40.0]
                && dimension_line_point.get() == [60.0, 60.0]
        ));

        let angular_bytes = legacy_payload(
            3,
            &[[2.0, 2.0], [2.0, 0.0], [0.0, 3.0], [1.0, 1.0]],
            &[std::f64::consts::FRAC_PI_2, 5.0],
        );
        let angular = test_decode(
            &angular_bytes,
            V5_ANGULAR,
            0..angular_bytes.len(),
            crate::test_support::millimeter_scale(10.0),
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
        assert_eq!(first_direction.get(), [1.0, 0.0]);
        assert!((second_direction[0]).abs() < 1.0e-12);
        assert!((second_direction[1] - 1.0).abs() < 1.0e-12);
        assert!((dimension_line_point[0] - 50.0 / 2.0_f64.sqrt()).abs() < 1.0e-12);
        assert!((dimension_line_point[1] - 50.0 / 2.0_f64.sqrt()).abs() < 1.0e-12);
        assert_eq!(first_extension_offset.get(), -1.0);
        assert_eq!(second_extension_offset.get(), -1.0);

        let center_bytes = payload(8, &4.5_f64.to_le_bytes());
        let center = test_decode(
            &center_bytes,
            CENTERMARK,
            0..center_bytes.len(),
            crate::test_support::millimeter_scale(10.0),
            archive,
        )
        .expect("required invariant");
        assert_eq!(center.measurement, 0.0);
        assert!(matches!(
            center.definition,
            Definition::CenterMark { radius } if radius.get() == 45.0
        ));

        let annotation = legacy_annotation_payload(8, &[[4.0, -7.0], [4.0, 2.0]]);
        let mut wrapped = anonymous(0, &annotation);
        wrapped.extend((-1_i32).to_le_bytes());
        wrapped.extend(1.25_f64.to_le_bytes());
        wrapped.extend(0.5_f64.to_le_bytes());
        let ordinate_bytes = anonymous(1, &wrapped);
        let ordinate = test_decode(
            &ordinate_bytes,
            V5_ORDINATE,
            0..ordinate_bytes.len(),
            crate::test_support::millimeter_scale(10.0),
            archive,
        )
        .expect("required invariant");
        assert_eq!(ordinate.measurement, 40.0);
        assert!(matches!(
            ordinate.definition,
            Definition::Ordinate {
                definition_point,
                leader_point,
                measured_direction: OrdinateAxis::X,
                kink_offsets,
            } if definition_point.get() == [40.0, -70.0]
                && leader_point.get() == [40.0, 20.0]
                && kink_offsets.map(FiniteReal::get) == [12.5, 5.0]
        ));

        let mut extension = [0_u8; 16].to_vec();
        extension.extend((-1_i32).to_le_bytes());
        extension.extend(0_i32.to_le_bytes());
        extension.extend(2.0_f64.to_le_bytes());
        extension.extend([0_u8; 15]);
        extension.push(42);
        let mut extension = anonymous(2, &extension);
        extension.extend([0x4d, 0xd4]);
        let descriptor = UserdataDescriptor::Known(ClassUserdata {
            range: 0..extension.len(),
            version: (1, 0),
            class_uuid: V5_DIM_EXTRA,
            item_uuid: V5_DIM_EXTRA,
            copy_count: 1,
            transform_range: 0..0,
            application_uuid: None,
            save_context: None,
            payload_range: 0..extension.len(),
        });
        let mut radial = radial;
        apply_userdata(
            &extension,
            std::slice::from_ref(&descriptor),
            archive,
            MillimeterScale::IDENTITY,
            &mut radial,
        )
        .expect("required invariant");
        assert_eq!(radial.measurement, 200.0);
        assert_eq!(radial.distance_scale.get(), 2.0);
        assert_eq!(radial.arrow_position, -1);
        assert_eq!(
            radial.detail_measured.to_string(),
            "00000000-0000-0000-0000-00000000002a"
        );

        let mut wrong_item_descriptor = descriptor.clone();
        let UserdataDescriptor::Known(ClassUserdata { item_uuid, .. }) = &mut wrong_item_descriptor
        else {
            panic!("expected known userdata");
        };
        *item_uuid = Uuid::nil();
        let mut wrong_item_radial = test_decode(
            &radial_bytes,
            V5_RADIAL,
            0..radial_bytes.len(),
            MillimeterScale::IDENTITY,
            archive,
        )
        .expect("fresh radial baseline");
        apply_userdata(
            &extension,
            std::slice::from_ref(&wrong_item_descriptor),
            archive,
            MillimeterScale::IDENTITY,
            &mut wrong_item_radial,
        )
        .expect("wrong dimension item UUID is not a matching extension");
        assert_eq!(wrong_item_radial.arrow_position, 0);
        assert_eq!(wrong_item_radial.distance_scale.get(), 1.0);
        assert!(wrong_item_radial.detail_measured.is_nil());

        let mut angular_extension =
            anonymous(0, &[2.5_f64.to_le_bytes(), 4.0_f64.to_le_bytes()].concat());
        angular_extension.extend([0x6e, 0xe6]);
        let angular_descriptor = UserdataDescriptor::Known(ClassUserdata {
            range: 0..angular_extension.len(),
            version: (1, 0),
            class_uuid: V5_ANGULAR_EXTRA,
            item_uuid: V5_ANGULAR_EXTRA,
            copy_count: 1,
            transform_range: 0..0,
            application_uuid: None,
            save_context: None,
            payload_range: 0..angular_extension.len(),
        });
        let mut angular = angular;
        apply_userdata(
            &angular_extension,
            std::slice::from_ref(&angular_descriptor),
            archive,
            crate::test_support::millimeter_scale(10.0),
            &mut angular,
        )
        .expect("required invariant");
        assert!(matches!(
            angular.definition,
            Definition::Angular {
                first_extension_offset,
                second_extension_offset,
                ..
            } if first_extension_offset.get() == 25.0 && second_extension_offset.get() == 40.0
        ));

        let mut wrong_item_descriptor = angular_descriptor.clone();
        let UserdataDescriptor::Known(ClassUserdata { item_uuid, .. }) = &mut wrong_item_descriptor
        else {
            panic!("expected known userdata");
        };
        *item_uuid = Uuid::nil();
        let mut wrong_item_angular = test_decode(
            &angular_bytes,
            V5_ANGULAR,
            0..angular_bytes.len(),
            crate::test_support::millimeter_scale(10.0),
            archive,
        )
        .expect("fresh angular baseline");
        apply_userdata(
            &angular_extension,
            std::slice::from_ref(&wrong_item_descriptor),
            archive,
            crate::test_support::millimeter_scale(10.0),
            &mut wrong_item_angular,
        )
        .expect("wrong item UUID is not a matching extension");
        assert!(matches!(
            wrong_item_angular.definition,
            Definition::Angular {
                first_extension_offset,
                second_extension_offset,
                ..
            } if first_extension_offset.get() == -1.0 && second_extension_offset.get() == -1.0
        ));

        let second_extension =
            anonymous(0, &[9.0_f64.to_le_bytes(), 11.0_f64.to_le_bytes()].concat());
        let second_start = angular_extension.len();
        let mut combined = angular_extension.clone();
        combined.extend(second_extension);
        let mut second_descriptor = angular_descriptor.clone();
        let UserdataDescriptor::Known(ClassUserdata {
            range,
            payload_range,
            ..
        }) = &mut second_descriptor
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
            crate::test_support::millimeter_scale(10.0),
            &mut duplicate_angular,
        )
        .expect("first duplicate extension");
        assert!(matches!(
            duplicate_angular.definition,
            Definition::Angular {
                first_extension_offset,
                second_extension_offset,
                ..
            } if first_extension_offset.get() == 25.0 && second_extension_offset.get() == 40.0
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
        let linear = test_decode(
            &linear_bytes,
            V5_LINEAR,
            0..linear_bytes.len(),
            crate::test_support::millimeter_scale(10.0),
            archive,
        )
        .expect("V4 linear common payload is direct");
        assert_eq!(linear.measurement, 30.0);

        let radial_bytes = direct_legacy_payload(
            4,
            &[[1.0, 2.0], [4.0, 6.0], [7.0, 8.0], [6.0, 8.0], [7.0, 8.0]],
            &[],
        );
        let radial = test_decode(
            &radial_bytes,
            V5_RADIAL,
            0..radial_bytes.len(),
            crate::test_support::millimeter_scale(10.0),
            archive,
        )
        .expect("V4 radial common payload is direct");
        assert_eq!(radial.measurement, 100.0);

        let angular_bytes = direct_legacy_payload(
            3,
            &[[2.0, 2.0], [2.0, 0.0], [0.0, 3.0], [1.0, 1.0]],
            &[std::f64::consts::FRAC_PI_2, 5.0],
        );
        let angular = test_decode(
            &angular_bytes,
            V5_ANGULAR,
            0..angular_bytes.len(),
            crate::test_support::millimeter_scale(10.0),
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
        let ordinate = test_decode(
            &ordinate_outer,
            V5_ORDINATE,
            0..ordinate_outer.len(),
            crate::test_support::millimeter_scale(10.0),
            archive,
        )
        .expect("V4 ordinate keeps its outer wrapper and direct common child");
        assert_eq!(ordinate.measurement, 40.0);
        assert!(matches!(
            ordinate.definition,
            Definition::Ordinate {
                measured_direction: OrdinateAxis::X,
                kink_offsets,
                ..
            } if kink_offsets.map(FiniteReal::get) == [12.5, 5.0]
        ));
    }
}
