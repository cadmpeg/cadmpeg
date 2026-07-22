// SPDX-License-Identifier: Apache-2.0
//! Modern Rhino dimension payload decoding.

use std::ops::Range;

use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::objects::{parse_class_wrapper, UserdataDescriptor};
use crate::settings::{plane, utf16, Plane};
use crate::wire::{scaled_coordinate, Uuid};

const ANONYMOUS: u32 = 0x4000_8000;
const V5_DIM_EXTRA: Uuid = Uuid::from_canonical([
    0x8a, 0xd5, 0xb9, 0xfc, 0x0d, 0x5c, 0x47, 0xfb, 0xad, 0xfd, 0x74, 0xc2, 0x8b, 0x6f, 0x66, 0x1e,
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

/// Complete common and family-specific dimension semantics.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Dimension {
    pub(crate) source_range: Range<usize>,
    pub(crate) annotation_type: i32,
    pub(crate) rich_text: String,
    pub(crate) user_text: String,
    pub(crate) dimstyle_id: Option<Uuid>,
    pub(crate) dimstyle_index: Option<i32>,
    pub(crate) plane: Plane,
    pub(crate) horizontal_direction: [f64; 2],
    pub(crate) allow_text_scaling: bool,
    pub(crate) text_display_mode: Option<i32>,
    pub(crate) text_height: Option<f64>,
    pub(crate) justification: Option<i32>,
    pub(crate) use_default_text_point: bool,
    pub(crate) user_text_point: [f64; 2],
    pub(crate) flip_arrows: [bool; 2],
    pub(crate) arrow_position: i32,
    pub(crate) detail_measured: Uuid,
    pub(crate) distance_scale: f64,
    pub(crate) definition: Definition,
    pub(crate) measurement: f64,
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
    )
}

fn scale_plane(mut value: Plane, scale: f64, offset: usize) -> Result<Plane, FramingError> {
    for coordinate in &mut value.origin.0 {
        *coordinate = scaled_coordinate(*coordinate, scale)
            .ok_or_else(|| structural(offset, "scaled dimension plane is invalid"))?;
    }
    value.equation[3] = scaled_coordinate(value.equation[3], scale)
        .ok_or_else(|| structural(offset, "scaled dimension plane is invalid"))?;
    Ok(value)
}

fn structural(offset: usize, message: impl Into<String>) -> FramingError {
    FramingError::Structural {
        offset,
        message: message.into(),
    }
}

fn anonymous(
    data: &[u8],
    offset: usize,
    end: usize,
    archive: ArchiveVersion,
) -> Result<(BoundedReader<'_>, usize, i32), FramingError> {
    let chunk = chunk_at(data, offset, end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(structural(offset, "expected dimension anonymous chunk"));
    }
    let mut reader = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    if reader.i32()? != 1 {
        return Err(structural(
            chunk.body.start,
            "unsupported dimension chunk major version",
        ));
    }
    let version = reader.i32()?;
    if version < 0 {
        return Err(structural(
            chunk.body.start + 4,
            "negative dimension content version",
        ));
    }
    Ok((reader, chunk.next_offset, version))
}

fn uuid(reader: &mut BoundedReader<'_>) -> Result<Uuid, FramingError> {
    Ok(Uuid::from_wire(reader.array()?))
}

fn point2(reader: &mut BoundedReader<'_>) -> Result<[f64; 2], FramingError> {
    let value = [reader.f64()?, reader.f64()?];
    if value.iter().all(|value| value.is_finite()) {
        Ok(value)
    } else {
        Err(structural(
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
    let (mut text, next, version) = anonymous(data, reader.position(), reader.end(), archive)?;
    if version != 0 {
        return Err(structural(
            text.position(),
            "unsupported text-content version",
        ));
    }
    let rich_text = utf16(&mut text)?;
    plane(&mut text)?;
    let rectangle_width = text.f64()?;
    let rotation_radians = text.f64()?;
    if !rectangle_width.is_finite() || !rotation_radians.is_finite() {
        return Err(structural(
            text.position() - 16,
            "text layout contains a nonfinite value",
        ));
    }
    let horizontal_alignment = text.i32()?;
    let vertical_alignment = text.i32()?;
    if !text.f64()?.is_finite() {
        return Err(structural(
            text.position() - 8,
            "obsolete text height is not finite",
        ));
    }
    let wrapped = text.bool()?;
    if text.remaining() != 0 {
        return Err(structural(
            text.position(),
            "text content has trailing bytes",
        ));
    }
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
    if version > 4 {
        return Err(structural(
            annotation.position(),
            "unsupported annotation version",
        ));
    }
    let text = text_content(data, &mut annotation, archive)?;
    let dimstyle_id = uuid(&mut annotation)?;
    let plane = plane(&mut annotation)?;
    let annotation_type = if version >= 1 { annotation.i32()? } else { 0 };
    if version >= 2 {
        let (mut overrides, override_next, override_version) =
            anonymous(data, annotation.position(), annotation.end(), archive)?;
        if override_version != 1 {
            return Err(structural(
                overrides.position(),
                "unsupported dimension override version",
            ));
        }
        if overrides.bool()? {
            let wrapper = chunk_at(data, overrides.position(), overrides.end(), archive, false)?;
            let mut warnings = Vec::new();
            parse_class_wrapper(
                data,
                overrides.position()..wrapper.next_offset,
                archive,
                &mut warnings,
            )?;
            overrides.skip(wrapper.next_offset - overrides.position())?;
        }
        if overrides.remaining() != 0 {
            return Err(structural(
                overrides.position(),
                "dimension overrides have trailing bytes",
            ));
        }
        annotation.skip(override_next - annotation.position())?;
    }
    let horizontal_direction = if version >= 3 {
        point2(&mut annotation)?
    } else {
        [1.0, 0.0]
    };
    let allow_text_scaling = version < 4 || annotation.bool()?;
    if annotation.remaining() != 0 {
        return Err(structural(
            annotation.position(),
            "annotation has trailing bytes",
        ));
    }
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
    })
}

fn scaled_point(value: [f64; 2], scale: f64, offset: usize) -> Result<[f64; 2], FramingError> {
    Ok([
        scaled_coordinate(value[0], scale)
            .ok_or_else(|| structural(offset, "scaled dimension point is invalid"))?,
        scaled_coordinate(value[1], scale)
            .ok_or_else(|| structural(offset, "scaled dimension point is invalid"))?,
    ])
}

fn angular_measurement(first: [f64; 2], second: [f64; 2], line: [f64; 2]) -> f64 {
    let first = first[1].atan2(first[0]);
    let counterclockwise = (second[1].atan2(second[0]) - first).rem_euclid(std::f64::consts::TAU);
    let line = (line[1].atan2(line[0]) - first).rem_euclid(std::f64::consts::TAU);
    if line <= counterclockwise {
        counterclockwise
    } else {
        std::f64::consts::TAU - counterclockwise
    }
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
    if minor > 3 {
        return Err(structural(
            annotation.position(),
            "unsupported legacy annotation version",
        ));
    }
    let kind = annotation.i32()?;
    let text_display_mode = annotation.i32()?;
    let plane_offset = annotation.position();
    let plane = scale_plane(plane(&mut annotation)?, scale, plane_offset)?;
    let point_count_offset = annotation.position();
    let point_count = annotation.i32()?;
    let point_count = usize::try_from(point_count)
        .ok()
        .filter(|count| *count <= 1 << 16 && *count <= annotation.remaining() / 16)
        .ok_or_else(|| structural(point_count_offset, "invalid legacy annotation point count"))?;
    let mut points = Vec::with_capacity(point_count);
    for _ in 0..point_count {
        let offset = annotation.position();
        points.push(scaled_point(point2(&mut annotation)?, scale, offset)?);
    }
    let rich_text = utf16(&mut annotation)?;
    let user_positioned_text = match annotation.i32()? {
        0 => false,
        1 => true,
        _ => {
            return Err(structural(
                annotation.position() - 4,
                "invalid legacy user-positioned-text flag",
            ))
        }
    };
    let initial_style_index = annotation.i32()?;
    let text_height = scaled_coordinate(annotation.f64()?, scale)
        .ok_or_else(|| structural(annotation.position() - 8, "invalid legacy text height"))?;
    if !text_height.is_finite() || text_height < 0.0 {
        return Err(structural(
            annotation.position() - 8,
            "invalid legacy annotation text height",
        ));
    }
    let justification = annotation.i32()?;
    let allow_text_scaling = minor < 1 || annotation.bool()?;
    let user_text = if minor >= 2 {
        utf16(&mut annotation)?
    } else {
        rich_text.clone()
    };
    let dimstyle_index = if minor >= 3 {
        annotation.i32()?;
        let dimension_style_index = annotation.i32()?;
        if dimension_style_index >= 0 {
            dimension_style_index
        } else {
            initial_style_index
        }
    } else {
        initial_style_index
    };
    if annotation.remaining() != 0 {
        return Err(structural(
            annotation.position(),
            "legacy annotation has trailing bytes",
        ));
    }
    reader.skip(next - reader.position())?;
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

fn direction(value: [f64; 2], offset: usize) -> Result<[f64; 2], FramingError> {
    let length = value[0].hypot(value[1]);
    if !length.is_finite() || length <= f64::EPSILON {
        return Err(structural(offset, "legacy angular direction is degenerate"));
    }
    Ok([value[0] / length, value[1] / length])
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

fn decode_legacy(
    data: &[u8],
    class: Uuid,
    range: Range<usize>,
    scale: f64,
    archive: ArchiveVersion,
) -> Result<Dimension, FramingError> {
    let (mut outer, next, minor) = anonymous(data, range.start, range.end, archive)?;
    if next != range.end
        || if class == V5_ORDINATE {
            minor > 1
        } else {
            minor != 0
        }
    {
        return Err(structural(
            range.start,
            "unsupported legacy dimension version",
        ));
    }
    let annotation = if class == V5_ORDINATE {
        let (mut wrapper, wrapper_next, wrapper_minor) =
            anonymous(data, outer.position(), outer.end(), archive)?;
        if wrapper_minor != 0 {
            return Err(structural(
                wrapper.position(),
                "unsupported legacy ordinate annotation wrapper",
            ));
        }
        let annotation = legacy_annotation(data, &mut wrapper, scale, archive)?;
        if wrapper.remaining() != 0 {
            return Err(structural(
                wrapper.position(),
                "legacy ordinate annotation wrapper has trailing bytes",
            ));
        }
        outer.skip(wrapper_next - outer.position())?;
        annotation
    } else {
        legacy_annotation(data, &mut outer, scale, archive)?
    };
    let stored_angular = if class == V5_ANGULAR {
        let angle = outer.f64()?;
        let radius = scaled_coordinate(outer.f64()?, scale)
            .ok_or_else(|| structural(outer.position() - 8, "invalid legacy angular radius"))?;
        if !angle.is_finite() || angle < 0.0 {
            return Err(structural(
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
                    structural(outer.position() - 8, "invalid legacy ordinate kink offset")
                })?,
                scaled_coordinate(outer.f64()?, scale).ok_or_else(|| {
                    structural(outer.position() - 8, "invalid legacy ordinate kink offset")
                })?,
            ]
        } else {
            [0.0, 0.0]
        };
        Some((direction, kink_offsets))
    } else {
        None
    };
    if outer.remaining() != 0 {
        return Err(structural(
            outer.position(),
            "legacy dimension has trailing bytes",
        ));
    }
    let (plane, definition, user_text_point, measurement) = if class == V5_LINEAR {
        if !matches!(annotation.kind, 1 | 2) || annotation.points.len() != 5 {
            return Err(structural(range.start, "invalid legacy linear definition"));
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
            if annotation.kind == 1 {
                definition_point[0].abs()
            } else {
                definition_point[0].hypot(definition_point[1])
            },
        )
    } else if class == V5_RADIAL {
        if !matches!(annotation.kind, 4 | 5) || annotation.points.len() != 4 {
            return Err(structural(range.start, "invalid legacy radial definition"));
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
            return Err(structural(range.start, "invalid legacy angular definition"));
        }
        let first_direction = direction(annotation.points[1], range.start)?;
        let second_direction = direction(annotation.points[2], range.start)?;
        let (angle, radius) = stored_angular.expect("angular family has stored fields");
        let line_direction = direction(annotation.points[3], range.start)?;
        let dimension_line_point = [line_direction[0] * radius, line_direction[1] * radius];
        (
            annotation.plane,
            Definition::Angular {
                first_direction,
                second_direction,
                first_extension_offset: 0.0,
                second_extension_offset: 0.0,
                dimension_line_point,
            },
            annotation.points[0],
            angle,
        )
    } else {
        if annotation.kind != 8 || annotation.points.len() != 2 {
            return Err(structural(
                range.start,
                "invalid legacy ordinate definition",
            ));
        }
        let definition_point = annotation.points[0];
        let leader_point = annotation.points[1];
        let (stored_direction, kink_offsets) =
            stored_ordinate.expect("ordinate family has stored fields");
        let measured_direction =
            ordinate_direction(stored_direction, definition_point, leader_point)
                .ok_or_else(|| structural(range.start, "invalid legacy ordinate direction"))?;
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
        return Err(structural(
            range.start,
            "legacy dimension measurement is invalid",
        ));
    }
    Ok(Dimension {
        source_range: range,
        annotation_type: annotation.kind,
        rich_text: annotation.rich_text,
        user_text: annotation.user_text,
        dimstyle_id: None,
        dimstyle_index: Some(annotation.dimstyle_index),
        plane,
        horizontal_direction: [1.0, 0.0],
        allow_text_scaling: annotation.allow_text_scaling,
        text_display_mode: Some(annotation.text_display_mode),
        text_height: Some(annotation.text_height),
        justification: Some(annotation.justification),
        use_default_text_point: !annotation.user_positioned_text,
        user_text_point,
        flip_arrows: [false, false],
        arrow_position: 0,
        detail_measured: Uuid::nil(),
        distance_scale: 1.0,
        definition,
        measurement,
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
    if matches!(class, V5_LINEAR | V5_ANGULAR | V5_RADIAL | V5_ORDINATE) {
        return decode_legacy(data, class, range, scale, archive);
    }
    let (mut outer, outer_next, outer_version) = anonymous(data, range.start, range.end, archive)?;
    if outer_next != range.end || outer_version != 0 {
        return Err(structural(
            range.start,
            "unsupported dimension family version",
        ));
    }
    let (mut common, common_next, common_version) =
        anonymous(data, outer.position(), outer.end(), archive)?;
    if common_version > 1 {
        return Err(structural(
            common.position(),
            "unsupported common dimension version",
        ));
    }
    let mut annotation = annotation(data, &mut common, archive)?;
    annotation.plane = scale_plane(annotation.plane, scale, range.start)?;
    let user_text = utf16(&mut common)?;
    if !common.f64()?.is_finite() {
        return Err(structural(
            common.position() - 8,
            "obsolete text rotation is not finite",
        ));
    }
    let use_default_text_point = common.bool()?;
    let text_offset = common.position();
    let user_text_point = scaled_point(point2(&mut common)?, scale, text_offset)?;
    let flip_arrows = [common.bool()?, common.bool()?];
    common.i32()?;
    let detail_measured = uuid(&mut common)?;
    let distance_scale = common.f64()?;
    if !distance_scale.is_finite() || distance_scale <= 0.0 {
        return Err(structural(
            common.position() - 8,
            "dimension distance scale is invalid",
        ));
    }
    if common_version >= 1 {
        common.i32()?;
    }
    if common.remaining() != 0 {
        return Err(structural(
            common.position(),
            "common dimension has trailing bytes",
        ));
    }
    outer.skip(common_next - outer.position())?;
    let definition = if class == LINEAR {
        if !matches!(annotation.kind, 1 | 5) {
            return Err(structural(
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
            return Err(structural(
                outer.position(),
                "invalid angular annotation type",
            ));
        }
        let first = point2(&mut outer)?;
        let second = point2(&mut outer)?;
        let first_extension_offset = scaled_coordinate(outer.f64()?, scale).ok_or_else(|| {
            structural(outer.position() - 8, "angular extension offset is invalid")
        })?;
        let second_extension_offset = scaled_coordinate(outer.f64()?, scale).ok_or_else(|| {
            structural(outer.position() - 8, "angular extension offset is invalid")
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
            return Err(structural(
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
            return Err(structural(
                outer.position(),
                "invalid ordinate annotation type",
            ));
        }
        let stored_direction = outer.i32()?;
        if !(0..=2).contains(&stored_direction) {
            return Err(structural(
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
            scaled_coordinate(outer.f64()?, scale)
                .ok_or_else(|| structural(outer.position() - 8, "invalid ordinate kink offset"))?,
            scaled_coordinate(outer.f64()?, scale)
                .ok_or_else(|| structural(outer.position() - 8, "invalid ordinate kink offset"))?,
        ];
        Definition::Ordinate {
            definition_point,
            leader_point,
            measured_direction,
            kink_offsets,
        }
    } else if class == CENTERMARK {
        if annotation.kind != 8 {
            return Err(structural(
                outer.position(),
                "invalid center-mark annotation type",
            ));
        }
        let radius = scaled_coordinate(outer.f64()?, scale)
            .filter(|radius| *radius >= 0.0)
            .ok_or_else(|| structural(outer.position() - 8, "invalid center-mark radius"))?;
        Definition::CenterMark { radius }
    } else {
        return Err(structural(range.start, "unsupported dimension class"));
    };
    if outer.remaining() != 0 {
        return Err(structural(outer.position(), "dimension has trailing bytes"));
    }
    let measurement = match &definition {
        Definition::Linear {
            definition_point, ..
        } => definition_point[0].abs() * distance_scale,
        Definition::Angular {
            first_direction,
            second_direction,
            dimension_line_point,
            ..
        } => angular_measurement(*first_direction, *second_direction, *dimension_line_point),
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
        return Err(structural(range.start, "dimension measurement is invalid"));
    }
    Ok(Dimension {
        source_range: range,
        annotation_type: annotation.kind,
        rich_text: annotation.rich_text,
        user_text,
        dimstyle_id: Some(annotation.dimstyle_id),
        dimstyle_index: None,
        plane: annotation.plane,
        horizontal_direction: annotation.horizontal_direction,
        allow_text_scaling: annotation.allow_text_scaling,
        text_display_mode: None,
        text_height: None,
        justification: None,
        use_default_text_point,
        user_text_point,
        flip_arrows,
        arrow_position: 0,
        detail_measured,
        distance_scale,
        definition,
        measurement,
    })
}

/// Applies the built-in V5 dimension extension carried as class userdata.
pub(crate) fn apply_userdata(
    data: &[u8],
    userdata: &[UserdataDescriptor],
    archive: ArchiveVersion,
    dimension: &mut Dimension,
) -> Result<(), FramingError> {
    let Some(extra) = userdata
        .iter()
        .find(|userdata| userdata.class_uuid == V5_DIM_EXTRA)
    else {
        return Ok(());
    };
    let (mut reader, next, minor) = anonymous(
        data,
        extra.payload_range.start,
        extra.payload_range.end,
        archive,
    )?;
    if next != extra.payload_range.end || minor > 2 {
        return Err(structural(
            extra.payload_range.start,
            "unsupported V5 dimension extension version",
        ));
    }
    uuid(&mut reader)?;
    let arrow_position = reader.i32()?;
    if !(-1..=1).contains(&arrow_position) {
        return Err(structural(
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
            return Err(structural(
                reader.position() - 4,
                "invalid V5 dimension text rectangle count",
            ))
        }
    }
    let distance_scale = if minor >= 1 { reader.f64()? } else { 1.0 };
    if !distance_scale.is_finite() || distance_scale <= 0.0 {
        return Err(structural(
            reader.position() - 8,
            "invalid V5 dimension distance scale",
        ));
    }
    let detail_measured = if minor >= 2 {
        uuid(&mut reader)?
    } else {
        Uuid::nil()
    };
    if reader.remaining() != 0 {
        return Err(structural(
            reader.position(),
            "V5 dimension extension has trailing bytes",
        ));
    }
    dimension.arrow_position = arrow_position;
    if dimension.dimstyle_index.is_some() {
        dimension.distance_scale = distance_scale;
    }
    dimension.detail_measured = detail_measured;
    if dimension.dimstyle_index.is_some()
        && !matches!(dimension.definition, Definition::Angular { .. })
    {
        dimension.measurement *= distance_scale;
    }
    Ok(())
}

/// Projects a decoded dimension into one native operation and one evaluated parameter.
pub(crate) fn project(
    dimension: &Dimension,
    key: &str,
    name: Option<String>,
    native_ref: String,
) -> (
    cadmpeg_ir::features::Feature,
    cadmpeg_ir::features::DesignParameter,
) {
    use cadmpeg_ir::features::{
        Angle, DesignParameter, DimensionDisplay, Feature, FeatureDefinition, FeatureId, Length,
        ParameterId, ParameterValue,
    };
    use std::collections::BTreeMap;

    let feature_id = FeatureId(format!("rhino:dimension:feature#{key}"));
    let parameter_id = ParameterId(format!("rhino:dimension:parameter#{key}"));
    let (kind, value, display) = match dimension.definition {
        Definition::Linear { .. } => (
            "linear_dimension",
            ParameterValue::Length(Length(dimension.measurement)),
            None,
        ),
        Definition::Angular { .. } => (
            "angular_dimension",
            ParameterValue::Angle(Angle(dimension.measurement)),
            None,
        ),
        Definition::Radial { diameter, .. } => (
            if diameter {
                "diameter_dimension"
            } else {
                "radius_dimension"
            },
            ParameterValue::Length(Length(dimension.measurement)),
            Some(if diameter {
                DimensionDisplay::Diameter
            } else {
                DimensionDisplay::Radius
            }),
        ),
        Definition::Ordinate { .. } => (
            "ordinate_dimension",
            ParameterValue::Length(Length(dimension.measurement)),
            None,
        ),
        Definition::CenterMark { .. } => ("center_mark", ParameterValue::Length(Length(0.0)), None),
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
    if let Some(id) = dimension.dimstyle_id {
        properties.insert("dimstyle_id".to_string(), id.to_string());
    }
    if let Some(index) = dimension.dimstyle_index {
        properties.insert("dimstyle_index".to_string(), index.to_string());
    }
    if let Some(mode) = dimension.text_display_mode {
        properties.insert("text_display_mode".to_string(), mode.to_string());
    }
    if let Some(height) = dimension.text_height {
        properties.insert("text_height".to_string(), height.to_string());
    }
    if let Some(justification) = dimension.justification {
        properties.insert("justification".to_string(), justification.to_string());
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
    parameters.insert("parameter_id".to_string(), parameter_id.0.clone());
    let feature = Feature {
        id: feature_id.clone(),
        ordinal: u64::try_from(dimension.source_range.start).expect("source offset fits u64"),
        name,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: Some("RhinoDimension".to_string()),
        source_text: None,
        source_content: vec![cadmpeg_ir::features::FeatureSourceContent::Parameter(
            parameter_id.clone(),
        )],
        outputs: Vec::new(),
        definition: FeatureDefinition::Native {
            kind: kind.to_string(),
            parameters,
            properties,
        },
        native_ref: Some(native_ref.clone()),
    };
    let parameter = DesignParameter {
        id: parameter_id,
        owner: feature_id,
        ordinal: 0,
        name: "measurement".to_string(),
        expression: if dimension.user_text.is_empty() {
            dimension.measurement.to_string()
        } else {
            dimension.user_text.clone()
        },
        display,
        value: Some(value),
        dependencies: Vec::new(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some(native_ref),
    };
    (feature, parameter)
}

/// Serializes one decoded dimension without source-record identity.
pub(crate) fn semantic_json(dimension: &Dimension) -> Option<String> {
    let (feature, parameter) = project(
        dimension,
        "embedded-history-dimension",
        None,
        "rhino:history:embedded-dimension".to_string(),
    );
    serde_json::to_string(&serde_json::json!({
        "kind": "dimension",
        "definition": feature.definition,
        "parameter": parameter,
    }))
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive_test_support::crc_chunk;

    #[test]
    fn angular_measurement_selects_the_arc_containing_the_dimension_line() {
        let first = [1.0, 0.0];
        let second = [0.0, 1.0];
        assert_eq!(
            angular_measurement(first, second, [1.0, 1.0]),
            std::f64::consts::FRAC_PI_2
        );
        assert_eq!(
            angular_measurement(first, second, [-1.0, -1.0]),
            3.0 * std::f64::consts::FRAC_PI_2
        );
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

    fn plane() -> Vec<u8> {
        [
            0.0, 0.0, 0.0, // origin
            1.0, 0.0, 0.0, // x axis
            0.0, 1.0, 0.0, // y axis
            0.0, 0.0, 1.0, // z axis
            0.0, 0.0, 1.0, 0.0, // equation
        ]
        .into_iter()
        .flat_map(f64::to_le_bytes)
        .collect()
    }

    fn payload(annotation_type: i32, family: &[u8]) -> Vec<u8> {
        let mut text = utf16("<>\n");
        text.extend(plane());
        text.extend(0.0_f64.to_le_bytes());
        text.extend(0.0_f64.to_le_bytes());
        text.extend(0_i32.to_le_bytes());
        text.extend(0_i32.to_le_bytes());
        text.extend(1.0_f64.to_le_bytes());
        text.push(0);

        let mut annotation = anonymous(0, &text);
        annotation.extend([0; 16]);
        annotation.extend(plane());
        annotation.extend(annotation_type.to_le_bytes());
        annotation.extend(anonymous(1, &[0]));
        annotation.extend(1.0_f64.to_le_bytes());
        annotation.extend(0.0_f64.to_le_bytes());
        annotation.push(1);

        let mut common = anonymous(4, &annotation);
        common.extend(utf16(""));
        common.extend(0.0_f64.to_le_bytes());
        common.push(1);
        common.extend(0.0_f64.to_le_bytes());
        common.extend(0.0_f64.to_le_bytes());
        common.extend([0, 0]);
        common.extend(0_i32.to_le_bytes());
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

    #[test]
    fn decodes_dimension_families_and_measurements() {
        let archive = ArchiveVersion::V8;
        let linear_family = [3.0_f64, 4.0, 8.0, 9.0]
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        let linear_bytes = payload(1, &linear_family);
        let linear = decode(&linear_bytes, LINEAR, 0..linear_bytes.len(), 10.0, archive).unwrap();
        assert_eq!(linear.measurement, 60.0);
        assert_eq!(linear.horizontal_direction, [1.0, 0.0]);
        let semantic: serde_json::Value =
            serde_json::from_str(&semantic_json(&linear).unwrap()).unwrap();
        assert_eq!(semantic["kind"], "dimension");
        assert_eq!(semantic["definition"]["kind"], "linear_dimension");

        let radial_family = [3.0_f64, 4.0, 8.0, 9.0]
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        let radial_bytes = payload(3, &radial_family);
        let radial = decode(&radial_bytes, RADIAL, 0..radial_bytes.len(), 1.0, archive).unwrap();
        assert_eq!(radial.measurement, 20.0);

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
        .unwrap();
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
        .unwrap();
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
        .unwrap();
        assert_eq!(linear.measurement, 30.0);
        assert_eq!(linear.dimstyle_index, Some(17));
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
        .unwrap();
        assert_eq!(radial.measurement, 100.0);
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
        .unwrap();
        assert_eq!(angular.measurement, std::f64::consts::FRAC_PI_2);
        assert!(matches!(
            angular.definition,
            Definition::Angular {
                first_direction: [1.0, 0.0],
                second_direction: [0.0, 1.0],
                first_extension_offset: 0.0,
                second_extension_offset: 0.0,
                ..
            }
        ));

        let center_bytes = payload(8, &4.5_f64.to_le_bytes());
        let center = decode(
            &center_bytes,
            CENTERMARK,
            0..center_bytes.len(),
            10.0,
            archive,
        )
        .unwrap();
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
        .unwrap();
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
        let extension = anonymous(2, &extension);
        let descriptor = UserdataDescriptor {
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
            unknown_version: false,
        };
        let mut radial = radial;
        apply_userdata(&extension, &[descriptor], archive, &mut radial).unwrap();
        assert_eq!(radial.measurement, 200.0);
        assert_eq!(radial.distance_scale, 2.0);
        assert_eq!(radial.arrow_position, -1);
        assert_eq!(
            radial.detail_measured.to_string(),
            "00000000-0000-0000-0000-00000000002a"
        );
    }
}
