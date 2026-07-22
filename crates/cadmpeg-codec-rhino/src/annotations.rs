// SPDX-License-Identifier: Apache-2.0
//! General Rhino text, leader, and text-dot annotations.

use cadmpeg_ir::document::CadIr;
use serde::Serialize;

use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::container::Scan;
use crate::settings::{utf16, Plane};
use crate::wire::{scaled_coordinate, Uuid};

const ANONYMOUS: u32 = 0x4000_8000;
const TEXT: Uuid = Uuid::from_canonical([
    0x57, 0x37, 0x63, 0x49, 0x62, 0xa9, 0x4a, 0x16, 0xb4, 0x11, 0xa4, 0x6b, 0xcd, 0x54, 0x47, 0x90,
]);
const LEADER: Uuid = Uuid::from_canonical([
    0x94, 0x5b, 0xf5, 0x94, 0x6f, 0xf9, 0x4f, 0x5c, 0xbf, 0xc0, 0xb3, 0xaf, 0x52, 0x8f, 0x29, 0xd2,
]);
const LEGACY_TEXT: Uuid = Uuid::from_canonical([
    0x46, 0xf7, 0x55, 0x41, 0xf4, 0x6b, 0x48, 0xbe, 0xaa, 0x7e, 0xb3, 0x53, 0xbb, 0xe0, 0x68, 0xa7,
]);
const LEGACY_LEADER: Uuid = Uuid::from_canonical([
    0x14, 0x92, 0x2b, 0x7a, 0x5b, 0x65, 0x4f, 0x11, 0x83, 0x45, 0xd4, 0x15, 0xa9, 0x63, 0x71, 0x29,
]);
const TEXT_DOT: Uuid = Uuid::from_canonical([
    0x74, 0x19, 0x83, 0x02, 0xcd, 0xf4, 0x4f, 0x95, 0x96, 0x09, 0x6d, 0x68, 0x4f, 0x22, 0xab, 0x37,
]);

#[derive(Debug, Serialize)]
struct AnnotationRecord {
    id: String,
    source_offset: u64,
    source_uuid: String,
    kind: &'static str,
    rich_text: String,
    plane_origin: [f64; 3],
    plane_x_axis: [f64; 3],
    plane_y_axis: [f64; 3],
    plane_z_axis: [f64; 3],
    plane_equation: [f64; 4],
    dimstyle_uuid: Option<String>,
    annotation_type: i32,
    text_rectangle_width: f64,
    text_rotation_radians: f64,
    horizontal_alignment: i32,
    vertical_alignment: i32,
    wrapped: bool,
    horizontal_direction: [f64; 2],
    allow_text_scaling: bool,
    legacy_text_display_mode: Option<i32>,
    legacy_user_text: Option<String>,
    legacy_user_positioned_text: Option<bool>,
    legacy_style_index: Option<i32>,
    legacy_text_height: Option<f64>,
    legacy_justification: Option<i32>,
    leader_points: Vec<[f64; 2]>,
    links: Vec<String>,
}

#[derive(Debug, Serialize)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent serialized display flags"
)]
struct TextDotRecord {
    id: String,
    source_offset: u64,
    source_uuid: String,
    center: [f64; 3],
    height_points: i32,
    primary_text: String,
    secondary_text: String,
    font_face: String,
    always_on_top: bool,
    transparent: bool,
    bold: bool,
    italic: bool,
    links: Vec<String>,
}

fn structural(offset: usize, message: impl Into<String>) -> FramingError {
    FramingError::Structural {
        offset,
        message: message.into(),
    }
}

fn anonymous(
    data: &[u8],
    range: std::ops::Range<usize>,
    archive: ArchiveVersion,
    expected_minor: i32,
) -> Result<BoundedReader<'_>, FramingError> {
    let chunk = chunk_at(data, range.start, range.end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short || chunk.next_offset != range.end {
        return Err(structural(range.start, "annotation wrapper is invalid"));
    }
    let mut reader = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    if reader.i32()? != 1 || reader.i32()? != expected_minor {
        return Err(structural(
            chunk.body.start,
            "annotation wrapper version is unsupported",
        ));
    }
    Ok(reader)
}

fn scaled_plane(mut plane: Plane, scale: f64, offset: usize) -> Result<Plane, FramingError> {
    for coordinate in &mut plane.origin.0 {
        *coordinate = scaled_coordinate(*coordinate, scale)
            .ok_or_else(|| structural(offset, "scaled annotation plane is invalid"))?;
    }
    plane.equation[3] = scaled_coordinate(plane.equation[3], scale)
        .ok_or_else(|| structural(offset, "scaled annotation equation is invalid"))?;
    Ok(plane)
}

fn decode_annotation(
    data: &[u8],
    range: std::ops::Range<usize>,
    archive: ArchiveVersion,
    scale: f64,
    leader: bool,
) -> Result<(crate::dimensions::Annotation, Vec<[f64; 2]>), FramingError> {
    let mut outer = anonymous(data, range.clone(), archive, i32::from(leader))?;
    let mut annotation = crate::dimensions::annotation(data, &mut outer, archive)?;
    annotation.plane = scaled_plane(annotation.plane, scale, range.start)?;
    annotation.text_rectangle_width = scaled_coordinate(annotation.text_rectangle_width, scale)
        .ok_or_else(|| structural(range.start, "scaled text rectangle width is invalid"))?;
    let mut points = Vec::new();
    if leader {
        let count = outer.i32()?;
        let bytes = crate::chunks::checked_count_bytes(
            count,
            16,
            outer.remaining(),
            1 << 20,
            outer.position(),
        )?;
        for _ in 0..bytes / 16 {
            let point = [outer.f64()?, outer.f64()?];
            if !point.iter().all(|value| value.is_finite()) {
                return Err(structural(
                    outer.position() - 16,
                    "leader point is not finite",
                ));
            }
            points.push([
                scaled_coordinate(point[0], scale).ok_or_else(|| {
                    structural(outer.position() - 16, "scaled leader point is invalid")
                })?,
                scaled_coordinate(point[1], scale).ok_or_else(|| {
                    structural(outer.position() - 8, "scaled leader point is invalid")
                })?,
            ]);
        }
    }
    if outer.remaining() != 0 {
        return Err(structural(
            outer.position(),
            "annotation has trailing bytes",
        ));
    }
    Ok((annotation, points))
}

fn decode_legacy_annotation(
    data: &[u8],
    range: std::ops::Range<usize>,
    archive: ArchiveVersion,
    scale: f64,
) -> Result<crate::dimensions::LegacyAnnotation, FramingError> {
    let chunk = chunk_at(data, range.start, range.end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short || chunk.next_offset != range.end {
        return Err(structural(
            range.start,
            "legacy annotation wrapper is invalid",
        ));
    }
    let mut outer = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
    if outer.i32()? != 1 || outer.i32()? != 0 {
        return Err(structural(
            chunk.body.start,
            "legacy annotation wrapper version is unsupported",
        ));
    }
    let value = crate::dimensions::legacy_annotation(data, &mut outer, scale, archive)?;
    if outer.remaining() != 0 {
        return Err(structural(
            outer.position(),
            "legacy annotation has trailing bytes",
        ));
    }
    Ok(value)
}

fn decode_dot(
    data: &[u8],
    range: std::ops::Range<usize>,
    scale: f64,
) -> Result<TextDotRecord, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let packed = reader.u8()?;
    if packed >> 4 != 1 || packed & 0x0f > 1 {
        return Err(structural(range.start, "text-dot version is unsupported"));
    }
    let mut center = [reader.f64()?, reader.f64()?, reader.f64()?];
    for value in &mut center {
        *value = scaled_coordinate(*value, scale)
            .ok_or_else(|| structural(range.start, "scaled text-dot center is invalid"))?;
    }
    let height_points = reader.i32()?;
    let primary_text = utf16(&mut reader)?;
    let font_face = utf16(&mut reader)?;
    let display = reader.i32()?;
    let secondary_text = if packed & 0x0f >= 1 {
        utf16(&mut reader)?
    } else {
        String::new()
    };
    if reader.remaining() != 0 {
        return Err(structural(reader.position(), "text dot has trailing bytes"));
    }
    Ok(TextDotRecord {
        id: String::new(),
        source_offset: range.start as u64,
        source_uuid: String::new(),
        center,
        height_points,
        primary_text,
        secondary_text,
        font_face,
        always_on_top: display & 1 != 0,
        transparent: display & 2 != 0,
        bold: display & 4 != 0,
        italic: display & 8 != 0,
        links: Vec::new(),
    })
}

/// Projects every supported general annotation into stable native records.
pub(crate) fn install(scan: &Scan<'_>, ir: &mut CadIr) {
    let Some(scale) = scan
        .metadata
        .settings
        .units
        .as_ref()
        .and_then(|units| units.millimeters_per_unit)
    else {
        return;
    };
    let mut annotations = Vec::new();
    let mut dots = Vec::new();
    for (source_order, object) in scan.objects.iter().enumerate() {
        let Some(identity) = &object.identity else {
            continue;
        };
        let link = format!("rhino:object:record#{source_order:06}");
        let key = if identity.object_id.is_nil() {
            format!("record-{source_order:06}")
        } else {
            identity.object_id.to_string()
        };
        if matches!(object.class_uuid, TEXT | LEADER) {
            let leader = object.class_uuid == LEADER;
            let Ok((value, points)) = decode_annotation(
                scan.data,
                object.class_data_range.clone(),
                scan.archive,
                scale,
                leader,
            ) else {
                continue;
            };
            annotations.push(AnnotationRecord {
                id: format!("rhino:document:annotation#{key}"),
                source_offset: object.range.start as u64,
                source_uuid: identity.object_id.to_string(),
                kind: if leader { "leader" } else { "text" },
                rich_text: value.rich_text,
                plane_origin: value.plane.origin.0,
                plane_x_axis: value.plane.xaxis.0,
                plane_y_axis: value.plane.yaxis.0,
                plane_z_axis: value.plane.zaxis.0,
                plane_equation: value.plane.equation,
                dimstyle_uuid: (!value.dimstyle_id.is_nil()).then(|| value.dimstyle_id.to_string()),
                annotation_type: value.kind,
                text_rectangle_width: value.text_rectangle_width,
                text_rotation_radians: value.text_rotation_radians,
                horizontal_alignment: value.horizontal_alignment,
                vertical_alignment: value.vertical_alignment,
                wrapped: value.wrapped,
                horizontal_direction: value.horizontal_direction,
                allow_text_scaling: value.allow_text_scaling,
                legacy_text_display_mode: None,
                legacy_user_text: None,
                legacy_user_positioned_text: None,
                legacy_style_index: None,
                legacy_text_height: None,
                legacy_justification: None,
                leader_points: points,
                links: vec![link],
            });
        } else if matches!(object.class_uuid, LEGACY_TEXT | LEGACY_LEADER) {
            let leader = object.class_uuid == LEGACY_LEADER;
            let Ok(value) = decode_legacy_annotation(
                scan.data,
                object.class_data_range.clone(),
                scan.archive,
                scale,
            ) else {
                continue;
            };
            annotations.push(AnnotationRecord {
                id: format!("rhino:document:annotation#{key}"),
                source_offset: object.range.start as u64,
                source_uuid: identity.object_id.to_string(),
                kind: if leader { "leader" } else { "text" },
                rich_text: value.rich_text,
                plane_origin: value.plane.origin.0,
                plane_x_axis: value.plane.xaxis.0,
                plane_y_axis: value.plane.yaxis.0,
                plane_z_axis: value.plane.zaxis.0,
                plane_equation: value.plane.equation,
                dimstyle_uuid: None,
                annotation_type: value.kind,
                text_rectangle_width: 0.0,
                text_rotation_radians: 0.0,
                horizontal_alignment: 0,
                vertical_alignment: 0,
                wrapped: false,
                horizontal_direction: [1.0, 0.0],
                allow_text_scaling: value.allow_text_scaling,
                legacy_text_display_mode: Some(value.text_display_mode),
                legacy_user_text: Some(value.user_text),
                legacy_user_positioned_text: Some(value.user_positioned_text),
                legacy_style_index: Some(value.dimstyle_index),
                legacy_text_height: Some(value.text_height),
                legacy_justification: Some(value.justification),
                leader_points: value.points,
                links: vec![link],
            });
        } else if object.class_uuid == TEXT_DOT {
            let Ok(mut value) = decode_dot(scan.data, object.class_data_range.clone(), scale)
            else {
                continue;
            };
            value.id = format!("rhino:document:text_dot#{key}");
            value.source_offset = object.range.start as u64;
            value.source_uuid = identity.object_id.to_string();
            value.links.push(link);
            dots.push(value);
        }
    }
    let namespace = ir.native.namespace_mut("rhino");
    namespace.version = namespace.version.max(2);
    namespace
        .set_arena("annotations", &annotations)
        .expect("Rhino annotations serialize");
    namespace
        .set_arena("text_dots", &dots)
        .expect("Rhino text dots serialize");
}

#[cfg(test)]
mod tests {
    use super::{decode_dot, decode_legacy_annotation, ANONYMOUS};
    use crate::chunks::ArchiveVersion;

    fn utf16(value: &str) -> Vec<u8> {
        let mut units = value.encode_utf16().collect::<Vec<_>>();
        units.push(0);
        let mut bytes = (units.len() as u32).to_le_bytes().to_vec();
        for unit in units {
            bytes.extend(unit.to_le_bytes());
        }
        bytes
    }

    fn anonymous(minor: i32, suffix: &[u8]) -> Vec<u8> {
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(minor.to_le_bytes());
        body.extend(suffix);
        crate::archive_test_support::crc_chunk(ANONYMOUS, &body)
    }

    fn plane() -> Vec<u8> {
        [
            1.0, 2.0, 3.0, // origin
            1.0, 0.0, 0.0, // x axis
            0.0, 1.0, 0.0, // y axis
            0.0, 0.0, 1.0, // z axis
            0.0, 0.0, 1.0, -3.0, // equation
        ]
        .into_iter()
        .flat_map(f64::to_le_bytes)
        .collect()
    }

    #[test]
    fn text_dot_preserves_text_style_flags_and_scaled_location() {
        let mut bytes = vec![0x11];
        for value in [1.0_f64, 2.0, 3.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(14_i32.to_le_bytes());
        bytes.extend(utf16("primary"));
        bytes.extend(utf16("Arial"));
        bytes.extend(15_i32.to_le_bytes());
        bytes.extend(utf16("secondary"));
        let dot = decode_dot(&bytes, 0..bytes.len(), 10.0).expect("valid text dot");
        assert_eq!(dot.center, [10.0, 20.0, 30.0]);
        assert_eq!(dot.primary_text, "primary");
        assert_eq!(dot.secondary_text, "secondary");
        assert!(dot.always_on_top && dot.transparent && dot.bold && dot.italic);
    }

    #[test]
    fn legacy_leader_reuses_dimension_annotation_grammar() {
        let mut common = 7_i32.to_le_bytes().to_vec();
        common.extend(2_i32.to_le_bytes());
        common.extend(plane());
        common.extend(2_i32.to_le_bytes());
        for value in [1.0_f64, 2.0, 4.0, 8.0] {
            common.extend(value.to_le_bytes());
        }
        common.extend(utf16("leader"));
        common.extend(0_i32.to_le_bytes());
        common.extend(12_i32.to_le_bytes());
        common.extend(1.5_f64.to_le_bytes());
        common.extend(4_i32.to_le_bytes());
        common.push(1);
        common.extend(utf16("formula"));
        common.extend((-1_i32).to_le_bytes());
        common.extend(12_i32.to_le_bytes());
        let inner = anonymous(3, &common);
        let bytes = anonymous(0, &inner);
        let value = decode_legacy_annotation(&bytes, 0..bytes.len(), ArchiveVersion::V8, 10.0)
            .expect("valid legacy leader");
        assert_eq!(value.rich_text, "leader");
        assert_eq!(value.user_text, "formula");
        assert_eq!(value.plane.origin.0, [10.0, 20.0, 30.0]);
        assert_eq!(value.points, [[10.0, 20.0], [40.0, 80.0]]);
        assert_eq!(value.text_height, 15.0);
        assert_eq!(value.dimstyle_index, 12);
    }
}
