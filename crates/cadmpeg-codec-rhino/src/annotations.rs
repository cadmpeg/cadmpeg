// SPDX-License-Identifier: Apache-2.0
//! General Rhino text, leader, and text-dot annotations.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::SourceProvenance;
use serde::Serialize;

use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::container::Scan;
use crate::loss::RhinoLossCode;
use crate::objects::{ClassUserdata, UserdataDescriptor};
use crate::settings::{utf16, MillimeterScale, Plane, UnitBinding};
use crate::wire::{scaled_coordinate, uuid, Uuid};

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
const V2_TEXT_DOT: Uuid = Uuid::from_canonical([
    0x8b, 0xd9, 0x4e, 0x19, 0x59, 0xe1, 0x11, 0xd4, 0x80, 0x18, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const V2_ANNOTATION_ARROW: Uuid = Uuid::from_canonical([
    0x8b, 0xd9, 0x4e, 0x1a, 0x59, 0xe1, 0x11, 0xd4, 0x80, 0x18, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
const V5_TEXT_EXTRA: Uuid = Uuid::from_canonical([
    0xd9, 0x04, 0x90, 0xa5, 0xdb, 0x86, 0x49, 0xf8, 0xbd, 0xa1, 0x90, 0x80, 0xb1, 0xf4, 0xe9, 0x76,
]);

/// The supported source grammars, before coordinate-unit admission.
enum AnnotationClass {
    Modern { leader: bool },
    Legacy { leader: bool },
    V2,
    Dot { v2: bool },
    V2Arrow,
}

impl AnnotationClass {
    fn from_uuid(uuid: Uuid) -> Option<Self> {
        match uuid {
            TEXT => Some(Self::Modern { leader: false }),
            LEADER => Some(Self::Modern { leader: true }),
            LEGACY_TEXT => Some(Self::Legacy { leader: false }),
            LEGACY_LEADER => Some(Self::Legacy { leader: true }),
            crate::dimensions::V2_ANNOTATION
            | crate::dimensions::V2_TEXT_OBJECT
            | crate::dimensions::V2_LEADER => Some(Self::V2),
            TEXT_DOT => Some(Self::Dot { v2: false }),
            V2_TEXT_DOT => Some(Self::Dot { v2: true }),
            V2_ANNOTATION_ARROW => Some(Self::V2Arrow),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum AnnotationKind {
    Leader,
    Text,
    Annotation,
}

#[derive(Debug, Serialize)]
struct AnnotationRecord {
    id: String,
    source_offset: u64,
    source_uuid: String,
    kind: AnnotationKind,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    v2_default_text: Option<String>,
    #[serde(flatten)]
    v2_text: Option<V2Text>,
    #[serde(skip_serializing_if = "Option::is_none")]
    v5_text_extra: Option<V5TextExtraRecord>,
    leader_points: Vec<[f64; 2]>,
    links: Vec<String>,
}

#[derive(Debug, Serialize)]
struct V5TextExtraRecord {
    parent_text_uuid: Option<String>,
    draw_mask: bool,
    mask_color_source: i32,
    mask_color: [u8; 4],
    border_offset_factor: f64,
}

#[derive(Debug, Serialize)]
struct TextDotRecord {
    id: String,
    source_offset: u64,
    source_uuid: String,
    #[serde(flatten)]
    data: TextDotData,
    links: Vec<String>,
}

/// Whether the dot draws in front of the objects it overlaps.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(into = "bool")]
enum DotDepth {
    /// The dot is occluded by objects in front of it.
    Occluded,
    /// The dot draws over everything.
    AlwaysOnTop,
}

/// Whether the dot paints its background.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(into = "bool")]
enum DotBackground {
    /// The background is filled.
    Opaque,
    /// The background is not painted.
    Transparent,
}

/// The weight the dot renders its text at.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(into = "bool")]
enum FontWeight {
    /// The face's ordinary weight.
    Regular,
    /// The face's bold weight.
    Bold,
}

/// The slant the dot renders its text at.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(into = "bool")]
enum FontSlant {
    /// Upright glyphs.
    Upright,
    /// Slanted glyphs.
    Italic,
}

impl From<DotDepth> for bool {
    fn from(value: DotDepth) -> Self {
        matches!(value, DotDepth::AlwaysOnTop)
    }
}

impl From<DotBackground> for bool {
    fn from(value: DotBackground) -> Self {
        matches!(value, DotBackground::Transparent)
    }
}

impl From<FontWeight> for bool {
    fn from(value: FontWeight) -> Self {
        matches!(value, FontWeight::Bold)
    }
}

impl From<FontSlant> for bool {
    fn from(value: FontSlant) -> Self {
        matches!(value, FontSlant::Italic)
    }
}

/// The display word a V4 text dot carries, one bit per display property.
#[derive(Debug, Clone, Copy)]
struct DotDisplay(i32);

impl DotDisplay {
    fn depth(self) -> DotDepth {
        if self.0 & 1 == 0 {
            DotDepth::Occluded
        } else {
            DotDepth::AlwaysOnTop
        }
    }

    fn background(self) -> DotBackground {
        if self.0 & 2 == 0 {
            DotBackground::Opaque
        } else {
            DotBackground::Transparent
        }
    }

    fn weight(self) -> FontWeight {
        if self.0 & 4 == 0 {
            FontWeight::Regular
        } else {
            FontWeight::Bold
        }
    }

    fn slant(self) -> FontSlant {
        if self.0 & 8 == 0 {
            FontSlant::Upright
        } else {
            FontSlant::Italic
        }
    }
}

#[derive(Debug, Serialize)]
struct TextDotData {
    center: [f64; 3],
    height_points: i32,
    primary_text: String,
    secondary_text: String,
    font_face: String,
    always_on_top: DotDepth,
    transparent: DotBackground,
    bold: FontWeight,
    italic: FontSlant,
}

#[derive(Debug, Serialize)]
struct AnnotationArrowRecord {
    id: String,
    source_offset: u64,
    source_uuid: String,
    tail: [f64; 3],
    head: [f64; 3],
    links: Vec<String>,
}

fn anonymous(
    data: &[u8],
    range: std::ops::Range<usize>,
    archive: ArchiveVersion,
    expected_minor: i32,
) -> Result<BoundedReader<'_>, FramingError> {
    let chunk = chunk_at(data, range.start, range.end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(FramingError::structural(
            range.start,
            "annotation wrapper is invalid",
        ));
    }
    let mut reader = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    let major = reader.i32()?;
    let minor = reader.i32()?;
    if major != 1 || minor < expected_minor {
        return Err(FramingError::structural(
            chunk.body().start,
            "annotation wrapper version is unsupported",
        ));
    }
    Ok(reader)
}

fn parse_v5_text_extra(
    data: &[u8],
    extra: &ClassUserdata,
    archive: ArchiveVersion,
) -> Result<V5TextExtraRecord, FramingError> {
    let mut reader = anonymous(data, extra.payload_range.clone(), archive, 0)?;
    let parent_text_uuid = uuid(&mut reader)?;
    let draw_mask = reader.bool()?;
    let mask_color_source = reader.i32()?;
    let mask_color = reader.array()?;
    let border_offset_factor = reader.f64()?;
    if !border_offset_factor.is_finite() {
        return Err(FramingError::structural(
            reader.position() - 8,
            "V5 text mask border offset is not finite",
        ));
    }
    reader.skip_remaining()?;
    Ok(V5TextExtraRecord {
        parent_text_uuid: (!parent_text_uuid.is_nil()).then(|| parent_text_uuid.to_string()),
        draw_mask,
        mask_color_source,
        mask_color,
        border_offset_factor,
    })
}

fn scaled_plane(
    mut plane: Plane,
    scale: MillimeterScale,
    offset: usize,
) -> Result<Plane, FramingError> {
    for coordinate in &mut plane.origin.0 {
        *coordinate = scaled_coordinate(*coordinate, scale).ok_or_else(|| {
            FramingError::structural(offset, "scaled annotation plane is invalid")
        })?;
    }
    plane.equation[3] = scaled_coordinate(plane.equation[3], scale)
        .ok_or_else(|| FramingError::structural(offset, "scaled annotation equation is invalid"))?;
    Ok(plane)
}

fn decode_annotation(
    data: &[u8],
    range: std::ops::Range<usize>,
    archive: ArchiveVersion,
    scale: MillimeterScale,
    leader: bool,
) -> Result<(crate::dimensions::Annotation, Vec<[f64; 2]>), FramingError> {
    let mut outer = anonymous(data, range.clone(), archive, i32::from(leader))?;
    let mut annotation = crate::dimensions::annotation(data, &mut outer, archive)?;
    annotation.plane = scaled_plane(annotation.plane, scale, range.start)?;
    annotation.text_rectangle_width = scaled_coordinate(annotation.text_rectangle_width, scale)
        .ok_or_else(|| {
            FramingError::structural(range.start, "scaled text rectangle width is invalid")
        })?;
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
                return Err(FramingError::structural(
                    outer.position() - 16,
                    "leader point is not finite",
                ));
            }
            points.push([
                scaled_coordinate(point[0], scale).ok_or_else(|| {
                    FramingError::structural(
                        outer.position() - 16,
                        "scaled leader point is invalid",
                    )
                })?,
                scaled_coordinate(point[1], scale).ok_or_else(|| {
                    FramingError::structural(outer.position() - 8, "scaled leader point is invalid")
                })?,
            ]);
        }
    }
    outer.skip_remaining()?;
    Ok((annotation, points))
}

fn decode_legacy_annotation(
    data: &[u8],
    range: std::ops::Range<usize>,
    archive: ArchiveVersion,
    scale: MillimeterScale,
) -> Result<crate::dimensions::LegacyAnnotation, FramingError> {
    if matches!(
        archive,
        ArchiveVersion::V2 | ArchiveVersion::V3 | ArchiveVersion::V4
    ) {
        let mut reader = BoundedReader::new(data, range.start, range.end)?;
        let value = crate::dimensions::legacy_annotation_direct(&mut reader, scale)?;
        reader.skip_remaining()?;
        return Ok(value);
    }
    let chunk = chunk_at(data, range.start, range.end, archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short() {
        return Err(FramingError::structural(
            range.start,
            "legacy annotation wrapper is invalid",
        ));
    }
    let mut outer = BoundedReader::new(data, chunk.body().start, chunk.body().end)?;
    if outer.i32()? != 1 || outer.i32()? < 0 {
        return Err(FramingError::structural(
            chunk.body().start,
            "legacy annotation wrapper version is unsupported",
        ));
    }
    let value = crate::dimensions::legacy_annotation(data, &mut outer, scale, archive)?;
    outer.skip_remaining()?;
    Ok(value)
}

struct V2AnnotationPayload {
    base: crate::dimensions::V2Annotation,
    text: Option<V2Text>,
}

#[derive(Debug, Serialize)]
struct V2Text {
    #[serde(rename = "v2_face_name")]
    face_name: String,
    #[serde(rename = "v2_font_weight")]
    font_weight: i32,
    #[serde(rename = "v2_text_height")]
    text_height: f64,
}

fn decode_v2_annotation(
    data: &[u8],
    range: std::ops::Range<usize>,
    scale: MillimeterScale,
    class: Uuid,
) -> Result<V2AnnotationPayload, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let base = crate::dimensions::v2_annotation_direct(&mut reader, scale)?;
    let text = if class == crate::dimensions::V2_TEXT_OBJECT {
        if base.kind != 7 {
            return Err(FramingError::structural(
                range.start,
                "V2 text object has a non-text annotation type",
            ));
        }
        let face_name = utf16(&mut reader)?;
        let font_weight = reader.i32()?;
        let raw_text_height = reader.f64()?;
        if !raw_text_height.is_finite()
            || raw_text_height.abs() > crate::dimensions::V2_REALLY_BIG_NUMBER
        {
            return Err(FramingError::structural(
                reader.position() - 8,
                "V2 text height is outside the source bound",
            ));
        }
        let text_height = scaled_coordinate(raw_text_height, scale).ok_or_else(|| {
            FramingError::structural(reader.position() - 8, "scaled V2 text height is invalid")
        })?;
        Some(V2Text {
            face_name,
            font_weight,
            text_height,
        })
    } else {
        if class == crate::dimensions::V2_LEADER && base.kind != 6 {
            return Err(FramingError::structural(
                range.start,
                "V2 leader has a non-leader annotation type",
            ));
        }
        None
    };
    reader.skip_remaining()?;
    Ok(V2AnnotationPayload { base, text })
}

fn decode_dot(
    data: &[u8],
    range: std::ops::Range<usize>,
    scale: MillimeterScale,
) -> Result<TextDotData, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let packed = reader.u8()?;
    if packed >> 4 != 1 {
        return Err(FramingError::structural(
            range.start,
            "text-dot version is unsupported",
        ));
    }
    let mut center = [reader.f64()?, reader.f64()?, reader.f64()?];
    for value in &mut center {
        *value = scaled_coordinate(*value, scale).ok_or_else(|| {
            FramingError::structural(range.start, "scaled text-dot center is invalid")
        })?;
    }
    let height_points = reader.i32()?;
    let primary_text = utf16(&mut reader)?;
    let font_face = utf16(&mut reader)?;
    let display = DotDisplay(reader.i32()?);
    let secondary_text = if packed & 0x0f >= 1 {
        utf16(&mut reader)?
    } else {
        String::new()
    };
    reader.skip_remaining()?;
    Ok(TextDotData {
        center,
        height_points,
        primary_text,
        secondary_text,
        font_face,
        always_on_top: display.depth(),
        transparent: display.background(),
        bold: display.weight(),
        italic: display.slant(),
    })
}

fn v2_version(
    reader: &mut BoundedReader<'_>,
    offset: usize,
    kind: &str,
) -> Result<(), FramingError> {
    if reader.u8()? >> 4 != 1 {
        return Err(FramingError::structural(
            offset,
            format!("V2 {kind} version is unsupported"),
        ));
    }
    Ok(())
}

fn v2_point(
    reader: &mut BoundedReader<'_>,
    scale: MillimeterScale,
    offset: usize,
    kind: &str,
) -> Result<[f64; 3], FramingError> {
    let mut point = [reader.f64()?, reader.f64()?, reader.f64()?];
    for value in &mut point {
        *value = scaled_coordinate(*value, scale).ok_or_else(|| {
            FramingError::structural(offset, format!("scaled V2 {kind} point is invalid"))
        })?;
    }
    Ok(point)
}

fn decode_v2_text_dot(
    data: &[u8],
    range: std::ops::Range<usize>,
    scale: MillimeterScale,
) -> Result<TextDotData, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    v2_version(&mut reader, range.start, "text-dot")?;
    let center = v2_point(&mut reader, scale, range.start, "text-dot")?;
    let primary_text = utf16(&mut reader)?;
    reader.skip_remaining()?;
    Ok(TextDotData {
        center,
        height_points: 0,
        primary_text,
        secondary_text: String::new(),
        font_face: String::new(),
        always_on_top: DotDepth::Occluded,
        transparent: DotBackground::Opaque,
        bold: FontWeight::Regular,
        italic: FontSlant::Upright,
    })
}

fn decode_v2_annotation_arrow(
    data: &[u8],
    range: std::ops::Range<usize>,
    scale: MillimeterScale,
) -> Result<([f64; 3], [f64; 3]), FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    v2_version(&mut reader, range.start, "annotation-arrow")?;
    let tail = v2_point(&mut reader, scale, range.start, "annotation-arrow tail")?;
    let head = v2_point(&mut reader, scale, range.start, "annotation-arrow head")?;
    reader.skip_remaining()?;
    Ok((tail, head))
}

fn source_key(identity: &crate::objects::SourceIdentity, source_order: usize) -> String {
    identity.source_id.rsplit_once('#').map_or_else(
        || format!("record-{source_order:06}"),
        |(_, key)| key.to_owned(),
    )
}

fn annotation_record_dropped(
    losses: &mut Vec<LossNote>,
    source_id: &str,
    source_offset: usize,
    class_uuid: Uuid,
    error: impl std::fmt::Display,
) {
    losses.push(
        RhinoLossCode::AnnotationRecordDropped
            .note(format!(
                "annotation object {source_id} at offset {source_offset} (class {class_uuid}) could not be transferred: {error}"
            ))
            .with_provenance(
                SourceProvenance::root("rhino", source_offset as u64)
                    .with_tag(format!("ANNOTATION/source={source_id}/class={class_uuid}")),
            ),
    );
}

/// Projects every supported general annotation into stable native records.
pub(crate) fn install(scan: &Scan<'_>, ir: &mut CadIr) -> Result<Vec<LossNote>, CodecError> {
    let binding = UnitBinding::from_units(scan.metadata.settings.units.as_ref());
    let mut losses = Vec::new();
    let mut annotations = Vec::new();
    let mut dots = Vec::new();
    let mut arrows = Vec::new();
    for (source_order, object) in scan.objects.iter().enumerate() {
        let Some(object) = object.framed() else {
            continue;
        };
        let Some(class) = AnnotationClass::from_uuid(object.class_uuid) else {
            continue;
        };
        let identity = &object.identity;
        let Some(scale) = binding.neutral_scale() else {
            annotation_record_dropped(
                &mut losses,
                &identity.source_id,
                object.range.start,
                object.class_uuid,
                format!("no physical millimetre binding ({})", binding.label()),
            );
            continue;
        };
        let link = format!("rhino:object:record#{source_order:06}");
        let key = source_key(identity, source_order);
        let source_uuid = identity.object_id.to_string();
        let mut v5_text_extra = None;
        if matches!(
            class,
            AnnotationClass::Modern { leader: false } | AnnotationClass::Legacy { leader: false }
        ) {
            if let Some(extra) = object
                .userdata
                .iter()
                .filter_map(UserdataDescriptor::known)
                .find(|userdata| {
                    userdata.class_uuid == V5_TEXT_EXTRA && userdata.item_uuid == V5_TEXT_EXTRA
                })
            {
                match parse_v5_text_extra(scan.data, extra, scan.archive) {
                    Ok(value) => v5_text_extra = Some(value),
                    Err(error) => {
                        losses.push(RhinoLossCode::AnnotationUserdataDropped.note(format!(
                            "V5 text-extra userdata at offset {} could not be transferred: {error}",
                            extra.range.start
                        )));
                    }
                }
            }
        }
        match class {
            AnnotationClass::Modern { leader } => {
                let (value, points) = match decode_annotation(
                    scan.data,
                    object.class_data_range.clone(),
                    scan.archive,
                    scale,
                    leader,
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        annotation_record_dropped(
                            &mut losses,
                            &identity.source_id,
                            object.range.start,
                            object.class_uuid,
                            error,
                        );
                        continue;
                    }
                };
                annotations.push(AnnotationRecord {
                    id: format!("rhino:document:annotation#{key}"),
                    source_offset: object.range.start as u64,
                    source_uuid,
                    kind: if leader {
                        AnnotationKind::Leader
                    } else {
                        AnnotationKind::Text
                    },
                    rich_text: value.rich_text,
                    plane_origin: value.plane.origin.0,
                    plane_x_axis: value.plane.xaxis.0,
                    plane_y_axis: value.plane.yaxis.0,
                    plane_z_axis: value.plane.zaxis.0,
                    plane_equation: value.plane.equation,
                    dimstyle_uuid: (!value.dimstyle_id.is_nil())
                        .then(|| value.dimstyle_id.to_string()),
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
                    v2_default_text: None,
                    v2_text: None,
                    v5_text_extra,
                    leader_points: points,
                    links: vec![link],
                });
            }
            AnnotationClass::Legacy { leader } => {
                let value = match decode_legacy_annotation(
                    scan.data,
                    object.class_data_range.clone(),
                    scan.archive,
                    scale,
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        annotation_record_dropped(
                            &mut losses,
                            &identity.source_id,
                            object.range.start,
                            object.class_uuid,
                            error,
                        );
                        continue;
                    }
                };
                annotations.push(AnnotationRecord {
                    id: format!("rhino:document:annotation#{key}"),
                    source_offset: object.range.start as u64,
                    source_uuid,
                    kind: if leader {
                        AnnotationKind::Leader
                    } else {
                        AnnotationKind::Text
                    },
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
                    horizontal_direction: [value.plane.xaxis.0[0], value.plane.yaxis.0[0]],
                    allow_text_scaling: value.allow_text_scaling,
                    legacy_text_display_mode: Some(value.text_display_mode),
                    legacy_user_text: Some(value.user_text),
                    legacy_user_positioned_text: Some(value.user_positioned_text),
                    legacy_style_index: Some(value.dimstyle_index),
                    legacy_text_height: Some(value.text_height),
                    legacy_justification: Some(value.justification),
                    v2_default_text: None,
                    v2_text: None,
                    v5_text_extra,
                    leader_points: value.points,
                    links: vec![link],
                });
            }
            AnnotationClass::V2 => {
                let value = match decode_v2_annotation(
                    scan.data,
                    object.class_data_range.clone(),
                    scale,
                    object.class_uuid,
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        annotation_record_dropped(
                            &mut losses,
                            &identity.source_id,
                            object.range.start,
                            object.class_uuid,
                            error,
                        );
                        continue;
                    }
                };
                let is_leader = object.class_uuid == crate::dimensions::V2_LEADER
                    || (object.class_uuid == crate::dimensions::V2_ANNOTATION
                        && value.base.kind == 6);
                let is_text = object.class_uuid == crate::dimensions::V2_TEXT_OBJECT
                    || (object.class_uuid == crate::dimensions::V2_ANNOTATION
                        && value.base.kind == 7);
                let kind = if is_leader {
                    AnnotationKind::Leader
                } else if is_text {
                    AnnotationKind::Text
                } else {
                    AnnotationKind::Annotation
                };
                let rich_text = crate::dimensions::v2_effective_text(&value.base);
                let leader_points = if is_leader {
                    value.base.points
                } else {
                    Vec::new()
                };
                annotations.push(AnnotationRecord {
                    id: format!("rhino:document:annotation#{key}"),
                    source_offset: object.range.start as u64,
                    source_uuid,
                    kind,
                    rich_text,
                    plane_origin: value.base.plane.origin.0,
                    plane_x_axis: value.base.plane.xaxis.0,
                    plane_y_axis: value.base.plane.yaxis.0,
                    plane_z_axis: value.base.plane.zaxis.0,
                    plane_equation: value.base.plane.equation,
                    dimstyle_uuid: None,
                    annotation_type: value.base.kind,
                    text_rectangle_width: 0.0,
                    text_rotation_radians: 0.0,
                    horizontal_alignment: 0,
                    vertical_alignment: 0,
                    wrapped: false,
                    horizontal_direction: [
                        value.base.plane.xaxis.0[0],
                        value.base.plane.yaxis.0[0],
                    ],
                    allow_text_scaling: false,
                    legacy_text_display_mode: None,
                    legacy_user_text: Some(value.base.user_text),
                    legacy_user_positioned_text: Some(value.base.user_positioned_text),
                    legacy_style_index: None,
                    legacy_text_height: None,
                    legacy_justification: None,
                    v2_default_text: Some(value.base.default_text),
                    v2_text: value.text,
                    v5_text_extra: None,
                    leader_points,
                    links: vec![link],
                });
            }
            AnnotationClass::Dot { v2 } => {
                let decoded = if v2 {
                    decode_v2_text_dot(scan.data, object.class_data_range.clone(), scale)
                } else {
                    decode_dot(scan.data, object.class_data_range.clone(), scale)
                };
                let data = match decoded {
                    Ok(value) => value,
                    Err(error) => {
                        annotation_record_dropped(
                            &mut losses,
                            &identity.source_id,
                            object.range.start,
                            object.class_uuid,
                            error,
                        );
                        continue;
                    }
                };
                dots.push(TextDotRecord {
                    id: format!("rhino:document:text_dot#{key}"),
                    source_offset: object.range.start as u64,
                    source_uuid,
                    data,
                    links: vec![link],
                });
            }
            AnnotationClass::V2Arrow => {
                let (tail, head) = match decode_v2_annotation_arrow(
                    scan.data,
                    object.class_data_range.clone(),
                    scale,
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        annotation_record_dropped(
                            &mut losses,
                            &identity.source_id,
                            object.range.start,
                            object.class_uuid,
                            error,
                        );
                        continue;
                    }
                };
                arrows.push(AnnotationArrowRecord {
                    id: format!("rhino:document:annotation_arrow#{key}"),
                    source_offset: object.range.start as u64,
                    source_uuid,
                    tail,
                    head,
                    links: vec![link],
                });
            }
        }
    }
    let namespace = ir.native.namespace_mut("rhino");
    namespace.set_arena("annotations", &annotations)?;
    namespace.set_arena("text_dots", &dots)?;
    if !arrows.is_empty() {
        namespace.set_arena("annotation_arrows", &arrows)?;
    }
    Ok(losses)
}

#[cfg(test)]
mod tests {
    use super::{
        decode_annotation, decode_dot, decode_legacy_annotation, decode_v2_annotation,
        decode_v2_annotation_arrow, decode_v2_text_dot, install, parse_v5_text_extra, ANONYMOUS,
        LEGACY_TEXT, V2_ANNOTATION_ARROW, V2_TEXT_DOT, V5_TEXT_EXTRA,
    };
    use crate::chunks::ArchiveVersion;
    use crate::objects::ClassUserdata;
    use crate::test_support::test_dump::utf16_bytes;
    use crate::test_support::test_dump::{
        object_record_with_payload, scan_with_objects, set_identity,
    };
    use crate::wire::Uuid;
    use cadmpeg_ir::document::CadIr;

    fn anonymous(minor: i32, suffix: &[u8]) -> Vec<u8> {
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend(minor.to_le_bytes());
        body.extend(suffix);
        crate::test_support::test_dump::crc_chunk(
            crate::chunks::ArchiveVersion::V5,
            ANONYMOUS,
            &body,
        )
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

    fn modern_annotation(leader: bool) -> Vec<u8> {
        let mut text = utf16_bytes("rich");
        text.extend(plane());
        text.extend(1.0_f64.to_le_bytes());
        text.extend(0.25_f64.to_le_bytes());
        text.extend(0_i32.to_le_bytes());
        text.extend(1_i32.to_le_bytes());
        text.extend(2.0_f64.to_le_bytes());
        text.push(1);

        let mut annotation = anonymous(0, &text);
        annotation.extend([0; 16]);
        annotation.extend(plane());
        annotation.extend(9_i32.to_le_bytes());
        annotation.extend(anonymous(1, &[0]));
        annotation.extend(1.0_f64.to_le_bytes());
        annotation.extend(0.0_f64.to_le_bytes());
        annotation.push(1);

        let mut outer_body = anonymous(4, &annotation);
        if leader {
            outer_body.extend(2_i32.to_le_bytes());
            for point in [[1.0_f64, 2.0], [3.0, 4.0]] {
                outer_body.extend(point[0].to_le_bytes());
                outer_body.extend(point[1].to_le_bytes());
            }
        }
        let mut outer = anonymous(i32::from(leader), &outer_body);
        outer.extend([0xfa, 0xce]);
        outer
    }

    fn v2_annotation_payload(
        kind: i32,
        points: &[[f64; 2]],
        user_text: &str,
        default_text: &str,
        user_positioned: bool,
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
        bytes
    }

    fn legacy_text_payload() -> Vec<u8> {
        let mut fields = 7_i32.to_le_bytes().to_vec();
        fields.extend(0_i32.to_le_bytes());
        fields.extend(plane());
        fields.extend(0_i32.to_le_bytes());
        fields.extend(utf16_bytes("legacy text"));
        fields.extend(0_i32.to_le_bytes());
        fields.extend(0_i32.to_le_bytes());
        fields.extend(1.5_f64.to_le_bytes());
        fields.extend(0_i32.to_le_bytes());
        fields.push(0);
        fields.extend(utf16_bytes("legacy text"));
        fields.extend((-1_i32).to_le_bytes());
        fields.extend(12_i32.to_le_bytes());
        let base = anonymous(3, &fields);
        anonymous(0, &base)
    }

    #[test]
    fn complete_annotations_require_physical_units_and_keep_located_source_losses() {
        use crate::test_support::test_dump as support;
        use cadmpeg_ir::codec::{Codec, DecodeOptions};

        let mut v2_text = v2_annotation_payload(7, &[], "text", "default", false);
        v2_text.extend(utf16_bytes("Witness Sans"));
        v2_text.extend(700_i32.to_le_bytes());
        v2_text.extend(12.5_f64.to_le_bytes());
        let mut dot = vec![0x10];
        for coordinate in [1.0_f64, 2.0, 3.0] {
            dot.extend(coordinate.to_le_bytes());
        }
        dot.extend(12_i32.to_le_bytes());
        dot.extend(utf16_bytes("dot"));
        dot.extend(utf16_bytes("Witness Sans"));
        dot.extend(0_i32.to_le_bytes());
        let mut v2_dot = vec![0x10];
        for coordinate in [1.0_f64, 2.0, 3.0] {
            v2_dot.extend(coordinate.to_le_bytes());
        }
        v2_dot.extend(utf16_bytes("V2 dot"));
        let mut arrow = vec![0x10];
        for coordinate in [1.0_f64, 2.0, 3.0, 4.0, 5.0, 6.0] {
            arrow.extend(coordinate.to_le_bytes());
        }
        // Enumerate source UUIDs independently of AnnotationClass::from_uuid.
        let fixtures = [
            (
                super::TEXT,
                modern_annotation(false),
                "annotations",
                "plane_origin",
            ),
            (
                super::LEADER,
                modern_annotation(true),
                "annotations",
                "plane_origin",
            ),
            (
                LEGACY_TEXT,
                legacy_text_payload(),
                "annotations",
                "plane_origin",
            ),
            (
                super::LEGACY_LEADER,
                legacy_text_payload(),
                "annotations",
                "plane_origin",
            ),
            (
                crate::dimensions::V2_ANNOTATION,
                v2_annotation_payload(123, &[], "base", "default", false),
                "annotations",
                "plane_origin",
            ),
            (
                crate::dimensions::V2_TEXT_OBJECT,
                v2_text,
                "annotations",
                "plane_origin",
            ),
            (
                crate::dimensions::V2_LEADER,
                v2_annotation_payload(6, &[[1.0, 2.0], [3.0, 4.0]], "leader", "default", false),
                "annotations",
                "plane_origin",
            ),
            (super::TEXT_DOT, dot, "text_dots", "center"),
            (V2_TEXT_DOT, v2_dot, "text_dots", "center"),
            (V2_ANNOTATION_ARROW, arrow, "annotation_arrows", "tail"),
        ];
        let archive = ArchiveVersion::V8;
        for (class, payload, arena, position_field) in fixtures {
            for (unit, expected_scale, binding_label) in [
                (Some(2), Some(1.0), "millimeters"),
                (Some(3), Some(10.0), "millimeters"),
                (Some(0), None, "native"),
                (Some(255), None, "unavailable"),
                (None, None, "unavailable"),
            ] {
                let object = object_record_with_payload(archive, 0x20, class.to_wire(), &payload);
                let point = object_record_with_payload(
                    archive,
                    1,
                    support::POINT_CLASS,
                    &support::point_payload([1.0, 2.0, 3.0]),
                );
                let settings = unit
                    .map(|unit| vec![support::units_record(archive, unit)])
                    .unwrap_or_default();
                let bytes = support::minimal_document(
                    "80",
                    &[
                        support::table(archive, 0x1000_0014, &[]),
                        support::table(archive, 0x1000_0015, &settings),
                        support::table(archive, 0x1000_0013, &[object.clone(), point]),
                    ],
                );
                let scan = crate::container::scan_owned(bytes.clone())
                    .expect("annotation archive framing");
                let source = scan.objects[0].framed().expect("framed annotation");
                let decoded = cadmpeg_test_support::EditableDecodeResult::from(
                    crate::RhinoCodec
                        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
                        .expect("complete annotation decode"),
                );
                let ir: CadIr = serde_json::from_slice(
                    &serde_json::to_vec(decoded.ir()).expect("annotation CADIR serialization"),
                )
                .expect("annotation CADIR admission");
                let namespace = ir
                    .native
                    .namespace("rhino")
                    .expect("Rhino native namespace");
                let records = namespace.arenas().get(arena).map_or(&[][..], Vec::as_slice);
                assert_eq!(
                    records.len(),
                    usize::from(expected_scale.is_some()),
                    "class={class} unit={unit:?} losses={:?}",
                    decoded.report().losses
                );
                let losses = decoded
                    .report()
                    .losses
                    .iter()
                    .filter(|loss| {
                        loss.code == super::RhinoLossCode::AnnotationRecordDropped.kind()
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    losses.len(),
                    usize::from(expected_scale.is_none()),
                    "class={class} unit={unit:?}"
                );
                assert_eq!(
                    decoded
                        .source_fidelity()
                        .retained_record("rhino:object:record#000000")
                        .expect("retained annotation source")
                        .data(),
                    Some(object.as_slice())
                );
                if let Some(scale) = expected_scale {
                    let record = serde_json::to_value(&records[0]).expect("annotation record JSON");
                    assert_eq!(
                        record[position_field][0], scale,
                        "class={class} unit={unit:?}"
                    );
                    assert_eq!(record["source_offset"], source.range.start as u64);
                    assert_eq!(
                        record["links"],
                        serde_json::json!(["rhino:object:record#000000"])
                    );
                    assert!(record.get("data").is_none());
                    assert!(record["id"].as_str().is_some_and(|id| !id.is_empty()));
                } else {
                    let loss = losses[0];
                    assert!(loss.message.contains(&source.identity.source_id));
                    assert!(loss
                        .message
                        .contains(&format!("no physical millimetre binding ({binding_label})")));
                    let provenance = loss.provenance.as_ref().expect("located annotation loss");
                    assert_eq!(provenance.offset, source.range.start as u64);
                    assert_eq!(
                        provenance.tag.as_deref(),
                        Some(
                            format!(
                                "ANNOTATION/source={}/class={class}",
                                source.identity.source_id
                            )
                            .as_str()
                        )
                    );
                }
            }
        }
    }

    #[test]
    fn v2_text_and_leader_readers_preserve_subclass_fields_and_suffixes() {
        let mut text = v2_annotation_payload(7, &[], "  text  ", "default", false);
        text.extend(utf16_bytes("Witness Sans"));
        text.extend(700_i32.to_le_bytes());
        text.extend(12.5_f64.to_le_bytes());
        text.extend([0xd1, 0xce]);
        let value = decode_v2_annotation(
            &text,
            0..text.len(),
            crate::test_support::millimeter_scale(10.0),
            crate::dimensions::V2_TEXT_OBJECT,
        )
        .expect("V2 text object");
        assert_eq!(value.base.user_text, "  text  ");
        assert_eq!(value.base.default_text, "default");
        let text = value
            .text
            .as_ref()
            .expect("V2 text fields are present together");
        assert_eq!(text.face_name, "Witness Sans");
        assert_eq!(text.font_weight, 700);
        assert_eq!(text.text_height, 125.0);
        assert_eq!(value.base.plane.origin.0, [10.0, 20.0, 30.0]);

        let mut leader = v2_annotation_payload(
            6,
            &[[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]],
            "",
            " leader ",
            true,
        );
        leader.extend([0xa5, 0x5a]);
        let value = decode_v2_annotation(
            &leader,
            0..leader.len(),
            crate::test_support::millimeter_scale(2.0),
            crate::dimensions::V2_LEADER,
        )
        .expect("V2 leader");
        assert_eq!(value.base.points, [[2.0, 4.0], [6.0, 8.0], [10.0, 12.0]]);
        assert!(value.base.user_positioned_text);
        assert!(value.text.is_none());
    }

    #[test]
    fn install_transfers_v2_text_leader_and_base_annotations() {
        let mut text = v2_annotation_payload(7, &[], "  text  ", "default", false);
        text.extend(utf16_bytes("Witness Sans"));
        text.extend(700_i32.to_le_bytes());
        text.extend(12.5_f64.to_le_bytes());
        text.extend([0xd1, 0xce]);

        let leader = v2_annotation_payload(6, &[[1.0, 2.0], [3.0, 4.0]], "", " leader ", true);
        let base = v2_annotation_payload(7, &[], "base", "unused", false);
        let unknown_base = v2_annotation_payload(123, &[], "unknown", "unused", false);
        let scan = scan_with_objects(&[
            object_record_with_payload(
                ArchiveVersion::V5,
                0x20,
                crate::dimensions::V2_TEXT_OBJECT.to_wire(),
                &text,
            ),
            object_record_with_payload(
                ArchiveVersion::V5,
                0x20,
                crate::dimensions::V2_LEADER.to_wire(),
                &leader,
            ),
            object_record_with_payload(
                ArchiveVersion::V5,
                0x20,
                crate::dimensions::V2_ANNOTATION.to_wire(),
                &base,
            ),
            object_record_with_payload(
                ArchiveVersion::V5,
                0x20,
                crate::dimensions::V2_ANNOTATION.to_wire(),
                &unknown_base,
            ),
        ]);
        let mut ir = CadIr::empty();
        install(&scan, &mut ir).expect("annotation installation");

        let namespace = ir.native.namespace("rhino").expect("Rhino namespace");
        assert_eq!(namespace.arenas()["annotations"].len(), 4);
        let text = serde_json::to_value(&namespace.arenas()["annotations"][0]).expect("text JSON");
        assert_eq!(text["kind"], "text");
        assert_eq!(text["rich_text"], "text");
        assert_eq!(text["v2_default_text"], "default");
        assert_eq!(text["v2_face_name"], "Witness Sans");
        assert_eq!(text["v2_font_weight"], 700);
        assert_eq!(text["v2_text_height"], 12.5);
        let leader =
            serde_json::to_value(&namespace.arenas()["annotations"][1]).expect("leader JSON");
        assert_eq!(leader["kind"], "leader");
        assert_eq!(leader["rich_text"], "leader");
        assert_eq!(leader["leader_points"][1][1], 4.0);
        let base = serde_json::to_value(&namespace.arenas()["annotations"][2]).expect("base JSON");
        assert_eq!(base["kind"], "text");
        assert_eq!(base["rich_text"], "base");
        let unknown =
            serde_json::to_value(&namespace.arenas()["annotations"][3]).expect("unknown JSON");
        assert_eq!(unknown["kind"], "annotation");
        assert_eq!(unknown["annotation_type"], 123);
    }

    #[test]
    fn install_uses_the_legacy_grammar_for_v5_text_objects() {
        let payload = legacy_text_payload();
        let scan = scan_with_objects(&[object_record_with_payload(
            ArchiveVersion::V5,
            0x20,
            LEGACY_TEXT.to_wire(),
            &payload,
        )]);
        let mut ir = CadIr::empty();
        let losses = install(&scan, &mut ir).expect("legacy annotation installation");
        assert!(losses.is_empty());

        let record = &ir
            .native
            .namespace("rhino")
            .expect("Rhino namespace")
            .arenas()["annotations"][0];
        assert_eq!(
            record.field("legacy_user_text"),
            Some(serde_json::json!("legacy text"))
        );
        assert_eq!(
            record.field("legacy_text_height"),
            Some(serde_json::json!(1.5))
        );
    }

    #[test]
    fn malformed_supported_annotation_is_reported_with_source_provenance() {
        let scan = scan_with_objects(&[object_record_with_payload(
            ArchiveVersion::V5,
            0x20,
            crate::dimensions::V2_TEXT_OBJECT.to_wire(),
            &[],
        )]);
        let source_offset = scan.objects[0].range().start;
        let source_id = scan.objects[0]
            .identity()
            .expect("test object identity")
            .source_id
            .clone();
        let mut ir = CadIr::empty();
        let losses = install(&scan, &mut ir).expect("annotation installation");
        let loss = losses
            .iter()
            .find(|loss| loss.code == super::RhinoLossCode::AnnotationRecordDropped.kind())
            .expect("malformed annotation loss");
        assert!(loss.message.contains(&format!("offset {source_offset}")));
        let provenance = loss.provenance.as_ref().expect("annotation provenance");
        assert_eq!(provenance.format(), "rhino");
        assert_eq!(provenance.offset, source_offset as u64);
        let expected_tag =
            format!("ANNOTATION/source={source_id}/class=5de6b210-486b-11d4-8014-0010830122f0");
        assert_eq!(provenance.tag.as_deref(), Some(expected_tag.as_str()));
        assert!(ir
            .native
            .namespace("rhino")
            .expect("Rhino namespace")
            .arenas()["annotations"]
            .is_empty());
    }

    #[test]
    fn duplicate_annotation_uuids_use_the_resolved_source_keys() {
        let payload = v2_annotation_payload(7, &[], "first", "unused", false);
        let mut scan = scan_with_objects(&[
            object_record_with_payload(
                ArchiveVersion::V5,
                0x20,
                crate::dimensions::V2_ANNOTATION.to_wire(),
                &payload,
            ),
            object_record_with_payload(
                ArchiveVersion::V5,
                0x20,
                crate::dimensions::V2_ANNOTATION.to_wire(),
                &payload,
            ),
        ]);
        let duplicate_id = [0x42; 16];
        set_identity(&mut scan, 0, duplicate_id, "first", None, true);
        set_identity(&mut scan, 1, duplicate_id, "second", None, true);

        let mut ir = CadIr::empty();
        install(&scan, &mut ir).expect("annotation installation");
        let records = &ir
            .native
            .namespace("rhino")
            .expect("Rhino namespace")
            .arenas()["annotations"];
        let ids = records
            .iter()
            .map(|record| record.id().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "rhino:document:annotation#first",
                "rhino:document:annotation#second"
            ]
        );
        assert!(records.iter().all(|record| {
            record.field("source_uuid")
                == Some(serde_json::json!(Uuid::from_wire(duplicate_id).to_string()))
        }));
    }

    #[test]
    fn text_dot_preserves_text_style_flags_and_scaled_location() {
        let mut bytes = vec![0x11];
        for value in [1.0_f64, 2.0, 3.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(14_i32.to_le_bytes());
        bytes.extend(utf16_bytes("primary"));
        bytes.extend(utf16_bytes("Arial"));
        bytes.extend(15_i32.to_le_bytes());
        bytes.extend(utf16_bytes("secondary"));
        let dot = decode_dot(
            &bytes,
            0..bytes.len(),
            crate::test_support::millimeter_scale(10.0),
        )
        .expect("valid text dot");
        assert_eq!(dot.center, [10.0, 20.0, 30.0]);
        assert_eq!(dot.primary_text, "primary");
        assert_eq!(dot.secondary_text, "secondary");
        assert!(
            bool::from(dot.always_on_top)
                && bool::from(dot.transparent)
                && bool::from(dot.bold)
                && bool::from(dot.italic)
        );
    }

    #[test]
    fn text_dot_v10_omits_secondary_text_and_skips_suffix() {
        let mut bytes = vec![0x10];
        for value in [12.5_f64, -3.25, 7.75] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(23_i32.to_le_bytes());
        bytes.extend(utf16_bytes("primary"));
        bytes.extend(utf16_bytes("Courier New"));
        bytes.extend(0_i32.to_le_bytes());
        bytes.extend([0xde, 0xad]);
        let dot = decode_dot(
            &bytes,
            0..bytes.len(),
            crate::settings::MillimeterScale::IDENTITY,
        )
        .expect("valid V1.0 text dot");
        assert_eq!(dot.center, [12.5, -3.25, 7.75]);
        assert_eq!(dot.height_points, 23);
        assert_eq!(dot.primary_text, "primary");
        assert_eq!(dot.secondary_text, "");
        assert_eq!(dot.font_face, "Courier New");
        assert!(
            !bool::from(dot.always_on_top)
                && !bool::from(dot.transparent)
                && !bool::from(dot.bold)
                && !bool::from(dot.italic)
        );
    }

    #[test]
    fn v2_text_dot_reads_point_and_utf16_text_and_skips_class_data_suffix() {
        let mut bytes = vec![0x1f];
        for value in [1.25_f64, -2.5, 4.75] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(utf16_bytes("V2 dot"));
        bytes.extend([0xd1, 0xce]);
        let dot = decode_v2_text_dot(
            &bytes,
            0..bytes.len(),
            crate::test_support::millimeter_scale(2.0),
        )
        .expect("valid V2 text dot");
        assert_eq!(dot.center, [2.5, -5.0, 9.5]);
        assert_eq!(dot.primary_text, "V2 dot");
        assert_eq!(dot.height_points, 0);
        assert_eq!(dot.font_face, "");
        assert!(
            !bool::from(dot.always_on_top)
                && !bool::from(dot.transparent)
                && !bool::from(dot.bold)
                && !bool::from(dot.italic)
        );
    }

    #[test]
    fn v2_annotation_arrow_reads_tail_and_head_and_skips_class_data_suffix() {
        let mut bytes = vec![0x10];
        for value in [[1.0_f64, 2.0, 3.0], [-4.0_f64, 5.0, -6.0]] {
            for coordinate in value {
                bytes.extend(coordinate.to_le_bytes());
            }
        }
        bytes.extend([0xa5, 0x5a]);
        let (tail, head) = decode_v2_annotation_arrow(
            &bytes,
            0..bytes.len(),
            crate::test_support::millimeter_scale(10.0),
        )
        .expect("valid V2 arrow");
        assert_eq!(tail, [10.0, 20.0, 30.0]);
        assert_eq!(head, [-40.0, 50.0, -60.0]);
    }

    #[test]
    fn install_transfers_v2_compatibility_annotations_to_native_arenas() {
        let mut dot = vec![0x1f];
        for value in [1.25_f64, -2.5, 4.75] {
            dot.extend(value.to_le_bytes());
        }
        dot.extend(utf16_bytes("V2 dot"));
        dot.extend([0xd1, 0xce]);

        let mut arrow = vec![0x10];
        for value in [[1.0_f64, 2.0, 3.0], [-4.0_f64, 5.0, -6.0]] {
            for coordinate in value {
                arrow.extend(coordinate.to_le_bytes());
            }
        }
        arrow.extend([0xa5, 0x5a]);

        let scan = scan_with_objects(&[
            object_record_with_payload(ArchiveVersion::V5, 0x20, V2_TEXT_DOT.to_wire(), &dot),
            object_record_with_payload(
                ArchiveVersion::V5,
                0x20,
                V2_ANNOTATION_ARROW.to_wire(),
                &arrow,
            ),
        ]);
        let mut ir = CadIr::empty();
        install(&scan, &mut ir).expect("annotation installation");

        let namespace = ir.native.namespace("rhino").expect("Rhino namespace");
        assert_eq!(namespace.arenas()["text_dots"].len(), 1);
        assert_eq!(namespace.arenas()["annotation_arrows"].len(), 1);
        let dot = serde_json::to_value(&namespace.arenas()["text_dots"][0]).expect("dot JSON");
        assert_eq!(dot["primary_text"], "V2 dot");
        assert_eq!(dot["center"][0], 1.25);
        let arrow =
            serde_json::to_value(&namespace.arenas()["annotation_arrows"][0]).expect("arrow JSON");
        assert_eq!(arrow["tail"][2], 3.0);
        assert_eq!(arrow["head"][0], -4.0);
    }

    #[test]
    fn modern_text_and_leader_readers_leave_class_data_suffixes_bounded() {
        let text = modern_annotation(false);
        let (text, points) = decode_annotation(
            &text,
            0..text.len(),
            ArchiveVersion::V8,
            crate::settings::MillimeterScale::IDENTITY,
            false,
        )
        .expect("modern text class-data suffix is bounded");
        assert_eq!(text.rich_text, "rich");
        assert!(points.is_empty());

        let leader = modern_annotation(true);
        let (leader, points) = decode_annotation(
            &leader,
            0..leader.len(),
            ArchiveVersion::V8,
            crate::settings::MillimeterScale::IDENTITY,
            true,
        )
        .expect("modern leader class-data suffix is bounded");
        assert_eq!(leader.rich_text, "rich");
        assert_eq!(points, [[1.0, 2.0], [3.0, 4.0]]);
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
        common.extend(utf16_bytes("leader"));
        common.extend(0_i32.to_le_bytes());
        common.extend(12_i32.to_le_bytes());
        common.extend(1.5_f64.to_le_bytes());
        common.extend(0_i32.to_le_bytes());
        common.push(1);
        common.extend(utf16_bytes("formula"));
        common.extend((-1_i32).to_le_bytes());
        common.extend(12_i32.to_le_bytes());
        let inner = anonymous(3, &common);
        let bytes = anonymous(0, &inner);
        let value = decode_legacy_annotation(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V8,
            crate::test_support::millimeter_scale(10.0),
        )
        .expect("valid legacy leader");
        assert_eq!(value.rich_text, "leader");
        assert_eq!(value.user_text, "formula");
        assert_eq!(value.plane.origin.0, [10.0, 35.0, 30.0]);
        assert_eq!(value.points, [[10.0, 20.0], [40.0, 80.0]]);
        assert_eq!(value.text_height, 15.0);
        assert_eq!(value.dimstyle_index, 12);
        assert_eq!(value.justification, (1 << 18) | 1);
    }

    #[test]
    fn direct_legacy_text_uses_packed_version_and_base_fields() {
        let mut bytes = vec![0x10];
        bytes.extend(7_i32.to_le_bytes());
        bytes.extend(2_i32.to_le_bytes());
        bytes.extend(plane());
        bytes.extend(2_i32.to_le_bytes());
        for value in [1.0_f64, 2.0, 4.0, 8.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(utf16_bytes("legacy"));
        bytes.extend(0_i32.to_le_bytes());
        bytes.extend((-1_i32).to_le_bytes());
        bytes.extend(1.5_f64.to_le_bytes());
        let value = decode_legacy_annotation(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V4,
            crate::test_support::millimeter_scale(10.0),
        )
        .expect("valid direct legacy text");
        assert_eq!(value.rich_text, "legacy");
        assert_eq!(value.user_text, "legacy");
        assert_eq!(value.plane.origin.0, [10.0, 35.0, 30.0]);
        assert_eq!(value.points, [[10.0, 20.0], [40.0, 80.0]]);
        assert_eq!(value.text_height, 15.0);
        assert_eq!(value.dimstyle_index, -1);
        assert_eq!(value.justification, (1 << 18) | 1);
        assert!(!value.allow_text_scaling);
    }

    #[test]
    fn v5_text_extra_reads_mask_fields_without_unit_scaling() {
        let parent = Uuid::from_canonical([
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ]);
        let mut payload = parent.to_wire().to_vec();
        payload.push(1);
        payload.extend(1_i32.to_le_bytes());
        payload.extend([0x11, 0x22, 0x33, 0x44]);
        payload.extend(0.375_f64.to_le_bytes());
        payload.extend([0xaa, 0xbb]);
        let bytes = anonymous(0, &payload);
        let descriptor = ClassUserdata {
            range: 0..bytes.len(),
            version: (2, 2),
            class_uuid: V5_TEXT_EXTRA,
            item_uuid: V5_TEXT_EXTRA,
            copy_count: 1,
            transform_range: 0..0,
            application_uuid: None,
            save_context: None,
            payload_range: 0..bytes.len(),
        };
        let value = parse_v5_text_extra(&bytes, &descriptor, ArchiveVersion::V8)
            .expect("valid V5 text extra");
        assert_eq!(value.parent_text_uuid, Some(parent.to_string()));
        assert!(value.draw_mask);
        assert_eq!(value.mask_color_source, 1);
        assert_eq!(value.mask_color, [0x11, 0x22, 0x33, 0x44]);
        assert_eq!(value.border_offset_factor, 0.375);
    }
}
