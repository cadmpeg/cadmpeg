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
            points.push(point);
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
pub(crate) fn install(scan: &Scan, ir: &mut CadIr) {
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
                &scan.data,
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
                text_rectangle_width: value.text_rectangle_width * scale,
                text_rotation_radians: value.text_rotation_radians,
                horizontal_alignment: value.horizontal_alignment,
                vertical_alignment: value.vertical_alignment,
                wrapped: value.wrapped,
                horizontal_direction: value.horizontal_direction,
                allow_text_scaling: value.allow_text_scaling,
                leader_points: points
                    .into_iter()
                    .map(|p| [p[0] * scale, p[1] * scale])
                    .collect(),
                links: vec![link],
            });
        } else if object.class_uuid == TEXT_DOT {
            let Ok(mut value) = decode_dot(&scan.data, object.class_data_range.clone(), scale)
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
    use super::decode_dot;

    fn utf16(value: &str) -> Vec<u8> {
        let mut units = value.encode_utf16().collect::<Vec<_>>();
        units.push(0);
        let mut bytes = (units.len() as u32).to_le_bytes().to_vec();
        for unit in units {
            bytes.extend(unit.to_le_bytes());
        }
        bytes
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
        let dot = decode_dot(&bytes, 0..bytes.len(), 10.0).unwrap();
        assert_eq!(dot.center, [10.0, 20.0, 30.0]);
        assert_eq!(dot.primary_text, "primary");
        assert_eq!(dot.secondary_text, "secondary");
        assert!(dot.always_on_top && dot.transparent && dot.bold && dot.italic);
    }
}
