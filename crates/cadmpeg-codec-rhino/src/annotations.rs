// SPDX-License-Identifier: Apache-2.0
//! General Rhino text, leader, and text-dot annotations.

use cadmpeg_ir::native::bytes::NativeBytes;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal};
use cadmpeg_ir::semantic_annotations::SemanticAnnotationKind;
use cadmpeg_ir::SourceProvenance;
use serde::Serialize;
use std::collections::BTreeMap;

use cadmpeg_ir::references::{ReferenceSelection, ReferenceTarget};

use crate::presentation::ResourceIndex;

use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::container::Scan;
use crate::loss::RhinoLossCode;
use crate::objects::{ClassUserdata, UserdataDescriptor};
use crate::settings::{utf16_retained, CoordinateLane, MillimeterScale, Plane, UnitBinding};
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
pub(crate) const TEXT_DOT: Uuid = Uuid::from_canonical([
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

/// Whether the class is one of the general annotation grammars.
pub(crate) fn is_annotation_class(uuid: Uuid) -> bool {
    AnnotationClass::from_uuid(uuid).is_some()
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
    plane_origin: crate::settings::CoordinateLane<3>,
    plane_x_axis: cadmpeg_ir::units::FiniteVector<3>,
    plane_y_axis: cadmpeg_ir::units::FiniteVector<3>,
    plane_z_axis: cadmpeg_ir::units::FiniteVector<3>,
    plane_equation: crate::settings::CoordinateLane<4>,
    dimstyle_uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimstyle_override: Option<serde_json::Value>,
    annotation_type: i32,
    text_rectangle_width: FiniteReal,
    text_rotation_radians: FiniteReal,
    horizontal_alignment: i32,
    vertical_alignment: i32,
    wrapped: bool,
    horizontal_direction: CoordinateLane<2>,
    allow_text_scaling: bool,
    legacy_text_display_mode: Option<i32>,
    legacy_user_text: Option<String>,
    legacy_user_positioned_text: Option<bool>,
    legacy_style_index: Option<i32>,
    legacy_text_height: Option<NonNegativeReal>,
    legacy_justification: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    v2_default_text: Option<String>,
    #[serde(flatten)]
    v2_text: Option<V2Text>,
    #[serde(skip_serializing_if = "Option::is_none")]
    v5_text_extra: Option<V5TextExtraRecord>,
    leader_points: Vec<[FiniteReal; 2]>,
    links: Vec<String>,
}

#[derive(Debug, Serialize)]
struct V5TextExtraRecord {
    parent_text_uuid: Option<String>,
    draw_mask: bool,
    mask_color_source: i32,
    mask_color: NativeBytes<[u8; 4]>,
    border_offset_factor: FiniteReal,
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
    center: [FiniteReal; 3],
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
    tail: [FiniteReal; 3],
    head: [FiniteReal; 3],
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
    let border_offset_factor = FiniteReal::new(reader.f64()?).ok_or_else(|| {
        FramingError::structural(
            reader.position() - 8,
            "V5 text mask border offset is not finite",
        )
    })?;
    reader.skip_remaining()?;
    Ok(V5TextExtraRecord {
        parent_text_uuid: (!parent_text_uuid.is_nil()).then(|| format!("{parent_text_uuid}")),
        draw_mask,
        mask_color_source,
        mask_color: (mask_color).into(),
        border_offset_factor,
    })
}

fn scaled_plane(
    mut plane: Plane,
    scale: MillimeterScale,
    offset: usize,
) -> Result<Plane, FramingError> {
    let origin = plane.origin.get();
    let mut scaled = [cadmpeg_ir::scalar::FiniteReal::ZERO; 3];
    for index in 0..3 {
        scaled[index] = scaled_coordinate(origin[index], scale).ok_or_else(|| {
            FramingError::structural(offset, "scaled annotation plane is invalid")
        })?;
    }
    plane.origin = crate::settings::CoordinateLane::Admitted(scaled.into());
    let constant = scaled_coordinate(plane.equation[3], scale)
        .ok_or_else(|| FramingError::structural(offset, "scaled annotation equation is invalid"))?;
    plane.equation = plane.equation.with_fourth(constant);
    Ok(plane)
}

fn plane_horizontal_direction(plane: Plane) -> CoordinateLane<2> {
    let x = plane.xaxis.finite_components();
    let y = plane.yaxis.finite_components();
    CoordinateLane::Admitted([x[0], y[0]].into())
}

fn decode_annotation(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    range: std::ops::Range<usize>,
    archive: ArchiveVersion,
    scale: MillimeterScale,
    leader: bool,
) -> Result<(crate::dimensions::Annotation, Vec<[FiniteReal; 2]>), FramingError> {
    let mut outer = anonymous(data, range.clone(), archive, i32::from(leader))?;
    let mut annotation = crate::dimensions::annotation(ctx, data, &mut outer, archive)?;
    annotation.plane = scaled_plane(annotation.plane, scale, range.start)?;
    annotation.text_rectangle_width =
        scaled_coordinate(annotation.text_rectangle_width.get(), scale).ok_or_else(|| {
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
        points = Vec::new();
        for _ in 0..bytes / 16 {
            let point = [outer.f64()?, outer.f64()?];
            let point = cadmpeg_ir::units::FiniteVector::new(point).ok_or_else(|| {
                FramingError::structural(outer.position() - 16, "leader point is not finite")
            })?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
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
        let value = crate::dimensions::legacy_annotation_direct(ctx, &mut reader, scale)?;
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
    let value = crate::dimensions::legacy_annotation(ctx, data, &mut outer, scale, archive)?;
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
    text_height: FiniteReal,
}

fn decode_v2_annotation(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    range: std::ops::Range<usize>,
    scale: MillimeterScale,
    class: Uuid,
) -> Result<V2AnnotationPayload, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let base = crate::dimensions::v2_annotation_direct(ctx, &mut reader, scale)?;
    let text = if class == crate::dimensions::V2_TEXT_OBJECT {
        if base.kind != 7 {
            return Err(FramingError::structural(
                range.start,
                "V2 text object has a non-text annotation type",
            ));
        }
        let face_name = utf16_retained(ctx, &mut reader, "Rhino V2 text face name")?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
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
    let raw_center = [reader.f64()?, reader.f64()?, reader.f64()?];
    let mut center = [FiniteReal::ZERO; 3];
    for (target, value) in center.iter_mut().zip(raw_center) {
        *target = scaled_coordinate(value, scale).ok_or_else(|| {
            FramingError::structural(range.start, "scaled text-dot center is invalid")
        })?;
    }
    let height_points = reader.i32()?;
    let primary_text = utf16_retained(ctx, &mut reader, "Rhino text dot primary text")?;
    let font_face = utf16_retained(ctx, &mut reader, "Rhino text dot font face")?;
    let display = DotDisplay(reader.i32()?);
    let secondary_text = if packed & 0x0f >= 1 {
        utf16_retained(ctx, &mut reader, "Rhino text dot secondary text")?
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
) -> Result<[FiniteReal; 3], FramingError> {
    let raw_point = [reader.f64()?, reader.f64()?, reader.f64()?];
    let mut point = [FiniteReal::ZERO; 3];
    for (target, value) in point.iter_mut().zip(raw_point) {
        *target = scaled_coordinate(value, scale).ok_or_else(|| {
            FramingError::structural(offset, format!("scaled V2 {kind} point is invalid"))
        })?;
    }
    Ok(point)
}

fn decode_v2_text_dot(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    data: &[u8],
    range: std::ops::Range<usize>,
    scale: MillimeterScale,
) -> Result<TextDotData, FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    v2_version(&mut reader, range.start, "text-dot")?;
    let center = v2_point(&mut reader, scale, range.start, "text-dot")?;
    let primary_text = utf16_retained(ctx, &mut reader, "Rhino V2 text dot primary text")?;
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
) -> Result<([FiniteReal; 3], [FiniteReal; 3]), FramingError> {
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    v2_version(&mut reader, range.start, "annotation-arrow")?;
    let tail = v2_point(&mut reader, scale, range.start, "annotation-arrow tail")?;
    let head = v2_point(&mut reader, scale, range.start, "annotation-arrow head")?;
    reader.skip_remaining()?;
    Ok((tail, head))
}

fn source_key(identity: &crate::objects::SourceIdentity, source_order: usize) -> String {
    let Some((_, key)) = identity.source_id.rsplit_once('#') else {
        return format!("record-{source_order:06}");
    };
    key.to_owned()
}

fn record_identity(
    identity: &crate::objects::SourceIdentity,
    source_order: usize,
    admitted_key: Option<String>,
) -> (Vec<String>, String, String) {
    let key = match admitted_key {
        Some(key) => key,
        None => source_key(identity, source_order),
    };
    let links = vec![format!("rhino:object:record#{source_order:06}")];
    (links, key, format!("{}", identity.object_id))
}

fn annotation_record_dropped(
    losses: &mut Vec<LossNote>,
    source_id: &str,
    source_offset: usize,
    class_uuid: Uuid,
    error: impl std::fmt::Display,
) {
    let loss = RhinoLossCode::AnnotationRecordDropped.note(format_args!("annotation object {source_id} at offset {source_offset} (class {class_uuid}) could not be transferred: {error}"));
    let tag = format!("ANNOTATION/source={source_id}/class={class_uuid}");
    losses.push(
        loss.with_provenance(
            SourceProvenance::root("rhino", cadmpeg_core::decode::u64_from_index(source_offset))
                .with_tag(tag),
        ),
    );
}

/// The serialized class name of a supported annotation class.
fn runtime_type(class: Uuid) -> &'static str {
    match class {
        TEXT => "ON_Text",
        LEADER => "ON_Leader",
        LEGACY_TEXT => "ON_OBSOLETE_V5_TextObject",
        LEGACY_LEADER => "ON_OBSOLETE_V5_Leader",
        crate::dimensions::V2_TEXT_OBJECT => "ON_OBSOLETE_V2_TextObject",
        crate::dimensions::V2_LEADER => "ON_OBSOLETE_V2_Leader",
        TEXT_DOT => "ON_TextDot",
        V2_TEXT_DOT => "ON_OBSOLETE_V2_TextDot",
        V2_ANNOTATION_ARROW => "ON_OBSOLETE_V2_AnnotationArrow",
        _ => "ON_OBSOLETE_V2_Annotation",
    }
}

/// The style table record an annotation names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StyleReference {
    /// The grammar carries no style reference.
    Absent,
    /// A V6 and later dimension-style UUID; nil names no style.
    Uuid(Uuid),
    /// A legacy dimension-style archive index; a negative index names no style.
    Index(i32),
    /// A legacy text object's text-style (font table) archive index; a
    /// negative index names no style.
    TextStyleIndex(i32),
}

/// The neutral semantic content of one decoded annotation object.
#[derive(Debug)]
pub(crate) struct Projection {
    /// Object-record source order.
    pub(crate) source_order: usize,
    /// Stable record key shared with the native record.
    pub(crate) key: String,
    /// Format-neutral semantic role.
    pub(crate) kind: SemanticAnnotationKind,
    /// Serialized openNURBS class name.
    pub(crate) runtime_type: &'static str,
    /// Plain visible text fragments.
    pub(crate) text: Vec<String>,
    /// Serialized rich-text string, when the grammar stores one.
    pub(crate) format: Option<String>,
    /// Whether `format` is RTF content without a plain-text fragment.
    pub(crate) rich_text_unflattened: bool,
    /// Model-space annotation point in millimetres.
    pub(crate) position: Option<cadmpeg_ir::units::FiniteVector<3>>,
    /// Every typed native field by name.
    pub(crate) parameters: BTreeMap<NonBlankString, String>,
    /// Serialized dimension-style reference.
    pub(crate) style: StyleReference,
}

/// Native annotation arenas, installed after neutral admission.
#[derive(Debug, Default)]
pub(crate) struct NativeArenas {
    annotations: Vec<AnnotationRecord>,
    dots: Vec<TextDotRecord>,
    arrows: Vec<AnnotationArrowRecord>,
}

impl NativeArenas {
    /// Writes the native annotation arenas.
    pub(crate) fn install(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        ir: &mut CadIr,
    ) -> Result<(), CodecError> {
        let namespace = ir.native.namespace_mut("rhino");
        namespace.set_arena(ctx, "annotations", &self.annotations)?;
        namespace.set_arena(ctx, "text_dots", &self.dots)?;
        if !self.arrows.is_empty() {
            namespace.set_arena(ctx, "annotation_arrows", &self.arrows)?;
        }
        Ok(())
    }
}

/// Every supported general annotation, decoded once.
#[derive(Debug, Default)]
pub(crate) struct Collected {
    /// Native records for the annotation arenas.
    pub(crate) arenas: NativeArenas,
    /// Neutral semantic content, one entry per native record.
    pub(crate) projections: Vec<Projection>,
    /// Losses for records and userdata that were not transferred.
    pub(crate) losses: Vec<LossNote>,
}

/// The neutral reference that a serialized style reference resolves to.
#[derive(Debug, Default)]
pub(crate) struct StyleBinding {
    /// Reference role and its target, absent when the grammar has no style.
    pub(crate) reference: Option<(NonBlankString, Vec<ReferenceSelection>)>,
    /// Whether a non-null style reference names no decoded style record.
    pub(crate) unresolved: bool,
}

/// Binds a serialized style reference to its decoded table record.
pub(crate) fn style_binding(
    resources: &ResourceIndex,
    style: StyleReference,
) -> Result<StyleBinding, CodecError> {
    let local = |record: Option<&str>| record.map(|id| ReferenceTarget::Local(id.to_owned()));
    let (role, target) = match style {
        StyleReference::Absent => return Ok(StyleBinding::default()),
        StyleReference::Uuid(id) if id.is_nil() => ("dimstyle_id", Some(ReferenceTarget::Null)),
        StyleReference::Uuid(id) => ("dimstyle_id", local(resources.dimension_styles.by_uuid(id))),
        StyleReference::Index(index) if index < 0 => {
            ("dimstyle_index", Some(ReferenceTarget::Null))
        }
        StyleReference::Index(index) => (
            "dimstyle_index",
            local(resources.dimension_styles.by_index(index)),
        ),
        StyleReference::TextStyleIndex(index) if index < 0 => {
            ("text_style_index", Some(ReferenceTarget::Null))
        }
        StyleReference::TextStyleIndex(index) => (
            "text_style_index",
            local(resources.text_styles.by_index(index)),
        ),
    };
    let Some(target) = target else {
        return Ok(StyleBinding {
            reference: None,
            unresolved: true,
        });
    };
    let role = NonBlankString::new(role)
        .ok_or_else(|| CodecError::malformed("style reference role is blank"))?;
    Ok(StyleBinding {
        reference: Some((role, vec![ReferenceSelection::new(target, Vec::new())])),
        unresolved: false,
    })
}

/// Plain text when `rich` is not RTF content.
/// The visible text of a rich-text string: the string itself when it is not
/// RTF, else the `ON_TextContent` run text when that is exact.
fn plain_text(rich: &str) -> Option<String> {
    if rich.starts_with("{\\rtf") {
        crate::rich_text::plain_text(rich).ok()
    } else {
        Some(rich.to_owned())
    }
}

fn finite_point(value: [f64; 3]) -> Option<cadmpeg_ir::units::FiniteVector<3>> {
    cadmpeg_ir::units::FiniteVector::new(value)
}

/// Every typed field of a native record except identity and source location.
fn record_parameters(
    record: &impl Serialize,
) -> Result<BTreeMap<NonBlankString, String>, CodecError> {
    let serde_json::Value::Object(fields) = serde_json::to_value(record)
        .map_err(|error| CodecError::malformed(format!("annotation record JSON: {error}")))?
    else {
        return Err(CodecError::malformed(
            "annotation record is not a JSON object",
        ));
    };
    let mut parameters = BTreeMap::new();
    for (name, value) in fields {
        if matches!(name.as_str(), "id" | "links" | "source_offset") || value.is_null() {
            continue;
        }
        let value = match value {
            serde_json::Value::String(text) => text,
            other => other.to_string(),
        };
        let name = NonBlankString::new(name)
            .ok_or_else(|| CodecError::malformed("annotation parameter name is blank"))?;
        parameters.insert(name, value);
    }
    Ok(parameters)
}

fn annotation_projection(
    source_order: usize,
    key: String,
    class: Uuid,
    record: &AnnotationRecord,
    plain: Option<String>,
    style: StyleReference,
) -> Result<Projection, CodecError> {
    let kind = match record.kind {
        AnnotationKind::Leader => SemanticAnnotationKind::Leader,
        AnnotationKind::Text => SemanticAnnotationKind::Text,
        AnnotationKind::Annotation => SemanticAnnotationKind::Other,
    };
    let rich_text_unflattened = plain.is_none();
    let text = plain.filter(|text| !text.is_empty());
    Ok(Projection {
        source_order,
        key,
        kind,
        runtime_type: runtime_type(class),
        rich_text_unflattened,
        text: text.into_iter().collect(),
        format: (!record.rich_text.is_empty()).then(|| record.rich_text.clone()),
        position: finite_point(record.plane_origin.get()),
        parameters: record_parameters(record)?,
        style,
    })
}

/// Decodes every supported general annotation into native records and their
/// neutral semantic content.
pub(crate) fn collect(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &Scan<'_>,
) -> Result<Collected, CodecError> {
    let binding = UnitBinding::from_units(scan.metadata.settings.units.as_ref());
    let mut losses = Vec::new();
    let mut annotations = Vec::new();
    let mut dots = Vec::new();
    let mut arrows = Vec::new();
    let mut projections = Vec::new();
    for (source_order, object) in scan.objects.iter().enumerate() {
        let Some(object) = object.framed() else {
            continue;
        };
        let Some(class) = AnnotationClass::from_uuid(object.class_uuid) else {
            continue;
        };
        let identity = &object.identity;
        let scale = binding.model_scale();
        let mut v2_key = if matches!(&class, AnnotationClass::V2) {
            Some(source_key(identity, source_order))
        } else {
            None
        };
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
                    Err(FramingError::Resource(limit)) => {
                        return Err(CodecError::ResourceLimit(limit))
                    }
                    Err(error) => {
                        losses.push(RhinoLossCode::AnnotationUserdataDropped.note(format_args!(
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
                    ctx,
                    scan.data,
                    object.class_data_range.clone(),
                    scan.archive,
                    scale,
                    leader,
                ) {
                    Ok(value) => value,
                    Err(FramingError::Resource(limit)) => {
                        return Err(CodecError::ResourceLimit(limit));
                    }
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

                let (links, key, source_uuid) =
                    record_identity(identity, source_order, v2_key.take());
                let style = StyleReference::Uuid(value.dimstyle_id);
                let dimstyle_override = match value.override_style.clone() {
                    None => None,
                    Some((class, range)) => match crate::presentation::override_dimension_style(
                        ctx,
                        scan.data,
                        class,
                        range,
                        (scan.archive, scan.metadata.properties.writer_version),
                        scale,
                    ) {
                        Ok(style) => Some(style),
                        Err(FramingError::Resource(limit)) => {
                            return Err(CodecError::ResourceLimit(limit))
                        }
                        Err(error) => {
                            losses.push(RhinoLossCode::DimensionOverrideDropped.note(
                                format_args!(
                                    "annotation object at offset {} style override could not be \
                                 decoded: {error}",
                                    object.range.start
                                ),
                            ));
                            None
                        }
                    },
                };
                let record = AnnotationRecord {
                    id: format!("rhino:document:annotation#{key}"),
                    source_offset: cadmpeg_core::decode::u64_from_index(object.range.start),
                    source_uuid,
                    kind: if leader {
                        AnnotationKind::Leader
                    } else {
                        AnnotationKind::Text
                    },
                    rich_text: value.rich_text,
                    plane_origin: value.plane.origin,
                    plane_x_axis: value.plane.xaxis,
                    plane_y_axis: value.plane.yaxis,
                    plane_z_axis: value.plane.zaxis,
                    plane_equation: value.plane.equation,
                    dimstyle_uuid: (!value.dimstyle_id.is_nil())
                        .then(|| format!("{}", value.dimstyle_id)),
                    dimstyle_override,
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
                    links,
                };
                projections.push(annotation_projection(
                    source_order,
                    key,
                    object.class_uuid,
                    &record,
                    plain_text(&record.rich_text),
                    style,
                )?);
                annotations.push(record);
            }
            AnnotationClass::Legacy { leader } => {
                let value = match decode_legacy_annotation(
                    ctx,
                    scan.data,
                    object.class_data_range.clone(),
                    scan.archive,
                    scale,
                ) {
                    Ok(value) => value,
                    Err(FramingError::Resource(limit)) => {
                        return Err(CodecError::ResourceLimit(limit));
                    }
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
                let mut leader_points = Vec::new();
                leader_points.extend(
                    value
                        .points
                        .into_iter()
                        .map(cadmpeg_ir::units::FiniteVector::finite_components),
                );

                let (links, key, source_uuid) =
                    record_identity(identity, source_order, v2_key.take());
                let style = if value.kind == crate::dimensions::LEGACY_TEXT_KIND {
                    StyleReference::TextStyleIndex(value.dimstyle_index)
                } else {
                    StyleReference::Index(value.dimstyle_index)
                };
                let record = AnnotationRecord {
                    id: format!("rhino:document:annotation#{key}"),
                    source_offset: cadmpeg_core::decode::u64_from_index(object.range.start),
                    source_uuid,
                    kind: if leader {
                        AnnotationKind::Leader
                    } else {
                        AnnotationKind::Text
                    },
                    rich_text: value.rich_text,
                    plane_origin: value.plane.origin,
                    plane_x_axis: value.plane.xaxis,
                    plane_y_axis: value.plane.yaxis,
                    plane_z_axis: value.plane.zaxis,
                    plane_equation: value.plane.equation,
                    dimstyle_uuid: None,
                    dimstyle_override: None,
                    annotation_type: value.kind,
                    text_rectangle_width: FiniteReal::ZERO,
                    text_rotation_radians: FiniteReal::ZERO,
                    horizontal_alignment: 0,
                    vertical_alignment: 0,
                    wrapped: false,
                    horizontal_direction: plane_horizontal_direction(value.plane),
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
                    leader_points,
                    links,
                };
                projections.push(annotation_projection(
                    source_order,
                    key,
                    object.class_uuid,
                    &record,
                    plain_text(&record.rich_text),
                    style,
                )?);
                annotations.push(record);
            }
            AnnotationClass::V2 => {
                let value = match decode_v2_annotation(
                    ctx,
                    scan.data,
                    object.class_data_range.clone(),
                    scale,
                    object.class_uuid,
                ) {
                    Ok(value) => value,
                    Err(FramingError::Resource(limit)) => {
                        return Err(CodecError::ResourceLimit(limit));
                    }
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
                    let mut points = Vec::new();
                    points.extend(
                        value
                            .base
                            .points
                            .into_iter()
                            .map(cadmpeg_ir::units::FiniteVector::finite_components),
                    );
                    points
                } else {
                    Vec::new()
                };

                let (links, key, source_uuid) =
                    record_identity(identity, source_order, v2_key.take());
                let record = AnnotationRecord {
                    id: format!("rhino:document:annotation#{key}"),
                    source_offset: cadmpeg_core::decode::u64_from_index(object.range.start),
                    source_uuid,
                    kind,
                    rich_text,
                    plane_origin: value.base.plane.origin,
                    plane_x_axis: value.base.plane.xaxis,
                    plane_y_axis: value.base.plane.yaxis,
                    plane_z_axis: value.base.plane.zaxis,
                    plane_equation: value.base.plane.equation,
                    dimstyle_uuid: None,
                    dimstyle_override: None,
                    annotation_type: value.base.kind,
                    text_rectangle_width: FiniteReal::ZERO,
                    text_rotation_radians: FiniteReal::ZERO,
                    horizontal_alignment: 0,
                    vertical_alignment: 0,
                    wrapped: false,
                    horizontal_direction: plane_horizontal_direction(value.base.plane),
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
                    links,
                };
                projections.push(annotation_projection(
                    source_order,
                    key,
                    object.class_uuid,
                    &record,
                    Some(record.rich_text.clone()),
                    StyleReference::Absent,
                )?);
                annotations.push(record);
            }
            AnnotationClass::Dot { v2 } => {
                let decoded = if v2 {
                    decode_v2_text_dot(ctx, scan.data, object.class_data_range.clone(), scale)
                } else {
                    decode_dot(ctx, scan.data, object.class_data_range.clone(), scale)
                };
                let data = match decoded {
                    Ok(value) => value,
                    Err(FramingError::Resource(limit)) => {
                        return Err(CodecError::ResourceLimit(limit))
                    }
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

                let (links, key, source_uuid) =
                    record_identity(identity, source_order, v2_key.take());
                let record = TextDotRecord {
                    id: format!("rhino:document:text_dot#{key}"),
                    source_offset: cadmpeg_core::decode::u64_from_index(object.range.start),
                    source_uuid,
                    data,
                    links,
                };
                projections.push(Projection {
                    source_order,
                    key,
                    kind: SemanticAnnotationKind::Text,
                    runtime_type: runtime_type(object.class_uuid),
                    text: (!record.data.primary_text.is_empty())
                        .then(|| record.data.primary_text.clone())
                        .into_iter()
                        .collect(),
                    format: None,
                    rich_text_unflattened: false,
                    position: finite_point(record.data.center.map(FiniteReal::get)),
                    parameters: record_parameters(&record)?,
                    style: StyleReference::Absent,
                });
                dots.push(record);
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

                let (links, key, source_uuid) =
                    record_identity(identity, source_order, v2_key.take());
                let record = AnnotationArrowRecord {
                    id: format!("rhino:document:annotation_arrow#{key}"),
                    source_offset: cadmpeg_core::decode::u64_from_index(object.range.start),
                    source_uuid,
                    tail,
                    head,
                    links,
                };
                projections.push(Projection {
                    source_order,
                    key,
                    kind: SemanticAnnotationKind::Other,
                    runtime_type: runtime_type(object.class_uuid),
                    text: Vec::new(),
                    format: None,
                    rich_text_unflattened: false,
                    position: finite_point(record.tail.map(FiniteReal::get)),
                    parameters: record_parameters(&record)?,
                    style: StyleReference::Absent,
                });
                arrows.push(record);
            }
        }
    }
    Ok(Collected {
        arenas: NativeArenas {
            annotations,
            dots,
            arrows,
        },
        projections,
        losses,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        collect, decode_annotation, decode_dot, decode_legacy_annotation, decode_v2_annotation,
        decode_v2_annotation_arrow, decode_v2_text_dot, parse_v5_text_extra, ANONYMOUS,
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
    use cadmpeg_ir::scalar::FiniteReal;
    use cadmpeg_test_support::{wire, EditableDecodeResult};

    /// Collects every annotation and installs its native arenas.
    fn install(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        scan: &crate::container::Scan<'_>,
        ir: &mut CadIr,
    ) -> Result<Vec<cadmpeg_ir::report::loss::LossNote>, cadmpeg_core::CodecError> {
        let collected = collect(ctx, scan)?;
        collected.arenas.install(ctx, ir)?;
        Ok(collected.losses)
    }

    #[test]
    fn style_references_bind_to_their_own_table() {
        use super::{style_binding, StyleReference};
        use cadmpeg_ir::references::ReferenceTarget;

        let style = Uuid::from_wire([0x41; 16]);
        let duplicate = Uuid::from_wire([0x42; 16]);
        let mut resources = crate::presentation::ResourceIndex::default();
        resources.dimension_styles.register(
            "rhino:presentation:dimension_style#a",
            Some(&format!("{style}")),
            Some(0),
        );
        for id in [
            "rhino:presentation:dimension_style#b",
            "rhino:presentation:dimension_style#c",
        ] {
            resources
                .dimension_styles
                .register(id, Some(&format!("{duplicate}")), Some(1));
        }
        resources
            .text_styles
            .register("rhino:presentation:text_style#t", None, Some(2));

        let target = |reference: StyleReference| {
            let binding = style_binding(&resources, reference).expect("style binding");
            (
                binding
                    .reference
                    .map(|(role, targets)| (role.to_string(), targets[0].target.clone())),
                binding.unresolved,
            )
        };
        let local = |id: &str| ReferenceTarget::Local(id.to_owned());
        assert_eq!(target(StyleReference::Absent), (None, false));
        assert_eq!(
            target(StyleReference::Uuid(Uuid::nil())),
            (
                Some(("dimstyle_id".to_owned(), ReferenceTarget::Null)),
                false
            )
        );
        assert_eq!(
            target(StyleReference::Uuid(style)),
            (
                Some((
                    "dimstyle_id".to_owned(),
                    local("rhino:presentation:dimension_style#a")
                )),
                false
            )
        );
        assert_eq!(target(StyleReference::Uuid(duplicate)), (None, true));
        assert_eq!(target(StyleReference::Index(1)), (None, true));
        assert_eq!(
            target(StyleReference::Index(-1)),
            (
                Some(("dimstyle_index".to_owned(), ReferenceTarget::Null)),
                false
            )
        );
        assert_eq!(target(StyleReference::Index(2)), (None, true));
        assert_eq!(
            target(StyleReference::TextStyleIndex(2)),
            (
                Some((
                    "text_style_index".to_owned(),
                    local("rhino:presentation:text_style#t")
                )),
                false
            )
        );
        assert_eq!(target(StyleReference::TextStyleIndex(0)), (None, true));
    }

    #[test]
    fn rtf_rich_text_flattens_to_its_run_text() {
        assert_eq!(super::plain_text("Hello"), Some("Hello".to_owned()));
        assert_eq!(
            super::plain_text("{\\rtf1\\ansi Hello}"),
            Some("Hello".to_owned())
        );
        assert_eq!(super::plain_text("{\\rtf1 {a 1/2}}"), None);
    }

    fn with_decode_context<R>(
        data: &[u8],
        apply: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> R,
    ) -> R {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
            .expect("annotation fixture fits the root limit");
        apply(&ctx)
    }

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
        bytes.extend((i32::try_from(points.len()).expect("fixture value fits i32")).to_le_bytes());
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
    fn complete_annotations_transfer_at_model_scale_without_a_physical_unit() {
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
            for (unit, expected_scale, undeclared, unset) in [
                (Some(2), 1.0, false, false),
                (Some(3), 10.0, false, false),
                (Some(0), 1.0, true, false),
                (Some(255), 1.0, false, true),
                (None, 1.0, false, false),
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
                let decoded = EditableDecodeResult::from(
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
                    1,
                    "class={class} unit={unit:?} losses={:?}",
                    decoded.report().losses
                );
                let count = |code: super::RhinoLossCode| {
                    decoded
                        .report()
                        .losses
                        .iter()
                        .filter(|loss| loss.code == code.kind())
                        .count()
                };
                assert_eq!(
                    count(super::RhinoLossCode::AnnotationRecordDropped),
                    0,
                    "class={class} unit={unit:?}"
                );
                assert_eq!(
                    count(super::RhinoLossCode::DocumentLengthUnitUndeclared),
                    usize::from(undeclared),
                    "class={class} unit={unit:?}"
                );
                assert_eq!(
                    count(super::RhinoLossCode::DocumentLengthUnitUnset),
                    usize::from(unset),
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
                let record = serde_json::to_value(&records[0]).expect("annotation record JSON");
                assert_eq!(
                    record[position_field][0], expected_scale,
                    "class={class} unit={unit:?}"
                );
                assert_eq!(
                    record["source_offset"],
                    cadmpeg_core::decode::u64_from_index(source.range.start)
                );
                assert_eq!(
                    record["links"],
                    serde_json::json!(["rhino:object:record#000000"])
                );
                assert!(record.get("data").is_none());
                assert!(record["id"].as_str().is_some_and(|id| !id.is_empty()));
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
        let value = with_decode_context(&text, |ctx| {
            decode_v2_annotation(
                ctx,
                &text,
                0..text.len(),
                crate::test_support::millimeter_scale(10.0),
                crate::dimensions::V2_TEXT_OBJECT,
            )
        })
        .expect("V2 text object");
        assert_eq!(value.base.user_text, "  text  ");
        assert_eq!(value.base.default_text, "default");
        let text = value
            .text
            .as_ref()
            .expect("V2 text fields are present together");
        assert_eq!(text.face_name, "Witness Sans");
        assert_eq!(text.font_weight, 700);
        assert_eq!(text.text_height, crate::test_support::finite(125.0));
        assert_eq!(value.base.plane.origin.get(), [10.0, 20.0, 30.0]);

        let mut leader = v2_annotation_payload(
            6,
            &[[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]],
            "",
            " leader ",
            true,
        );
        leader.extend([0xa5, 0x5a]);
        let value = with_decode_context(&leader, |ctx| {
            decode_v2_annotation(
                ctx,
                &leader,
                0..leader.len(),
                crate::test_support::millimeter_scale(2.0),
                crate::dimensions::V2_LEADER,
            )
        })
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
        install(
            &cadmpeg_test_support::service_decode_context(),
            &scan,
            &mut ir,
        )
        .expect("annotation installation");

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
        let losses = install(
            &cadmpeg_test_support::service_decode_context(),
            &scan,
            &mut ir,
        )
        .expect("legacy annotation installation");
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
        let losses = install(
            &cadmpeg_test_support::service_decode_context(),
            &scan,
            &mut ir,
        )
        .expect("annotation installation");
        let loss = losses
            .iter()
            .find(|loss| loss.code == super::RhinoLossCode::AnnotationRecordDropped.kind())
            .expect("malformed annotation loss");
        assert!(loss.message.contains(&format!("offset {source_offset}")));
        let provenance = loss.provenance.as_ref().expect("annotation provenance");
        assert_eq!(wire::field::<String>(&provenance, "format"), "rhino");
        assert_eq!(
            provenance.offset,
            cadmpeg_core::decode::u64_from_index(source_offset)
        );
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
        install(
            &cadmpeg_test_support::service_decode_context(),
            &scan,
            &mut ir,
        )
        .expect("annotation installation");
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
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            0..bytes.len(),
            crate::test_support::millimeter_scale(10.0),
        )
        .expect("valid text dot");
        assert_eq!(
            dot.center,
            crate::test_support::finite_array([10.0, 20.0, 30.0])
        );
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
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            0..bytes.len(),
            crate::settings::MillimeterScale::IDENTITY,
        )
        .expect("valid V1.0 text dot");
        assert_eq!(
            dot.center,
            crate::test_support::finite_array([12.5, -3.25, 7.75])
        );
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
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            0..bytes.len(),
            crate::test_support::millimeter_scale(2.0),
        )
        .expect("valid V2 text dot");
        assert_eq!(
            dot.center,
            crate::test_support::finite_array([2.5, -5.0, 9.5])
        );
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
        assert_eq!(tail, crate::test_support::finite_array([10.0, 20.0, 30.0]));
        assert_eq!(
            head,
            crate::test_support::finite_array([-40.0, 50.0, -60.0])
        );
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
        install(
            &cadmpeg_test_support::service_decode_context(),
            &scan,
            &mut ir,
        )
        .expect("annotation installation");

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
    fn decoded_dots_and_arrows_become_semantic_annotations() {
        use cadmpeg_ir::semantic_annotations::SemanticAnnotationKind;

        let mut dot = vec![0x10];
        for value in [1.25_f64, -2.5, 4.75] {
            dot.extend(value.to_le_bytes());
        }
        dot.extend(utf16_bytes("V2 dot"));
        let mut arrow = vec![0x10];
        for value in [1.0_f64, 2.0, 3.0, -4.0, 5.0, -6.0] {
            arrow.extend(value.to_le_bytes());
        }
        let scan = scan_with_objects(&[
            object_record_with_payload(ArchiveVersion::V5, 0x20, V2_TEXT_DOT.to_wire(), &dot),
            object_record_with_payload(
                ArchiveVersion::V5,
                0x20,
                V2_ANNOTATION_ARROW.to_wire(),
                &arrow,
            ),
        ]);
        let result = crate::decode::decode_for_test(&scan);
        let annotations = &result.ir().model.semantic_annotations;
        assert_eq!(annotations.len(), 2);
        assert_eq!(annotations[0].kind, SemanticAnnotationKind::Text);
        assert_eq!(annotations[0].runtime_type, "ON_OBSOLETE_V2_TextDot");
        assert_eq!(annotations[0].text, ["V2 dot"]);
        assert_eq!(
            annotations[0]
                .position
                .map(cadmpeg_ir::units::FiniteVector::get),
            Some([1.25, -2.5, 4.75])
        );
        assert_eq!(annotations[1].kind, SemanticAnnotationKind::Other);
        assert_eq!(
            annotations[1].runtime_type,
            "ON_OBSOLETE_V2_AnnotationArrow"
        );
        assert_eq!(annotations[1].parameters["head"], "[-4.0,5.0,-6.0]");
        assert!(result.report().losses.iter().all(|loss| {
            loss.code != crate::loss::RhinoLossCode::ObjectFamilyNotTransferred.kind()
        }));
    }

    #[test]
    fn modern_text_and_leader_readers_leave_class_data_suffixes_bounded() {
        let text = modern_annotation(false);
        let (text, points) = decode_annotation(
            &cadmpeg_test_support::service_decode_context(),
            &text,
            0..text.len(),
            ArchiveVersion::V8,
            crate::settings::MillimeterScale::IDENTITY,
            false,
        )
        .expect("modern text class-data suffix is bounded");
        assert_eq!(text.rich_text, "rich");
        assert_eq!(text.text_rectangle_width.get(), 1.0);
        assert_eq!(text.text_rotation_radians.get(), 0.25);
        assert!(points.is_empty());

        let leader = modern_annotation(true);
        let (leader, points) = decode_annotation(
            &cadmpeg_test_support::service_decode_context(),
            &leader,
            0..leader.len(),
            ArchiveVersion::V8,
            crate::settings::MillimeterScale::IDENTITY,
            true,
        )
        .expect("modern leader class-data suffix is bounded");
        assert_eq!(leader.rich_text, "rich");
        assert_eq!(
            points
                .into_iter()
                .map(|point| point.map(FiniteReal::get))
                .collect::<Vec<_>>(),
            [[1.0, 2.0], [3.0, 4.0]]
        );
    }

    #[test]
    fn modern_text_layout_refuses_nonfinite_rotation_at_source() {
        let mut text = modern_annotation(false);
        let rotation = 0.25_f64.to_le_bytes();
        let offset = text
            .windows(rotation.len())
            .position(|window| window == rotation)
            .expect("text rotation in source bytes");
        text[offset..offset + 8].copy_from_slice(&f64::NAN.to_le_bytes());
        let result = decode_annotation(
            &cadmpeg_test_support::service_decode_context(),
            &text,
            0..text.len(),
            ArchiveVersion::V8,
            crate::settings::MillimeterScale::IDENTITY,
            false,
        );
        assert!(matches!(
            result,
            Err(crate::chunks::FramingError::Structural { offset: failed, message })
                if failed == offset - 8 && message == "text layout contains a nonfinite value"
        ));
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
        let value = with_decode_context(&bytes, |ctx| {
            decode_legacy_annotation(
                ctx,
                &bytes,
                0..bytes.len(),
                ArchiveVersion::V8,
                crate::test_support::millimeter_scale(10.0),
            )
        })
        .expect("valid legacy leader");
        assert_eq!(value.rich_text, "leader");
        assert_eq!(value.user_text, "formula");
        assert_eq!(value.plane.origin.get(), [10.0, 35.0, 30.0]);
        assert_eq!(value.points, [[10.0, 20.0], [40.0, 80.0]]);
        assert_eq!(value.text_height.get(), 15.0);
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
        let value = with_decode_context(&bytes, |ctx| {
            decode_legacy_annotation(
                ctx,
                &bytes,
                0..bytes.len(),
                ArchiveVersion::V4,
                crate::test_support::millimeter_scale(10.0),
            )
        })
        .expect("valid direct legacy text");
        assert_eq!(value.rich_text, "legacy");
        assert_eq!(value.user_text, "legacy");
        assert_eq!(value.plane.origin.get(), [10.0, 35.0, 30.0]);
        assert_eq!(value.points, [[10.0, 20.0], [40.0, 80.0]]);
        assert_eq!(value.text_height.get(), 15.0);
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
        assert_eq!(value.border_offset_factor.get(), 0.375);
    }
}
