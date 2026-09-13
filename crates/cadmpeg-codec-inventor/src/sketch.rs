// SPDX-License-Identifier: Apache-2.0
//! Typed planar-sketch records and closed neutral sketch graphs.

use crate::pmdc::unique_by;

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    NativeOperandField, Sketch, SketchConstraint, SketchConstraintDefinitionInput,
    SketchConstraintId, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchLocus, SketchNativeOperand, SketchPlacement,
};
use cadmpeg_ir::{
    features::{DesignParameter, ParameterId},
    scalar::{Angle, Length},
};
use serde::{Deserialize, Serialize};

use crate::compact_matrix::CompactMatrix;
use crate::pmdc::{
    content_header, reference_list, type_id_string, Cursor, PmDcContentHeader, PmDcReference,
    PmDcReferenceList,
};
use crate::record_identity::{Located, RecordPayload};
use crate::record_issue::{RecordIssue, RecordIssueFamily};
use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};

const EPS_SKETCH_LINE_CARRIER_MATCHES_E10: f64 = 1.0e-10;
const EPS_SKETCH_PROJECT_PLACEMENT_E10: f64 = 1.0e-10;

const SKETCH_TYPE: [u8; 16] = inventor_id(0x9087_4d11);
const TRANSFORM_TYPE: [u8; 16] = inventor_id(0x9087_4d18);
const POINT_TYPE: [u8; 16] = sketch_entity_id(0xce52_df35);
const LINE_TYPE: [u8; 16] = sketch_entity_id(0xce52_df3a);
const CIRCLE_TYPE: [u8; 16] = sketch_entity_id(0xce52_df3b);
const DIRECTION_TYPE: [u8; 16] = sketch_entity_id(0xce52_df40);
const ELLIPSE_TYPE: [u8; 16] = [
    0x60, 0xd4, 0x07, 0x45, 0xd1, 0x11, 0xbe, 0xe6, 0x80, 0x00, 0x6f, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const COINCIDENT_TYPE: [u8; 16] = inventor_id(0x9087_4d94);
const PARALLEL_TYPE: [u8; 16] = inventor_id(0x9087_4d95);
const PERPENDICULAR_TYPE: [u8; 16] = inventor_id(0x9087_4d96);
const TANGENT_TYPE: [u8; 16] = inventor_id(0x9087_4d97);
const HORIZONTAL_TYPE: [u8; 16] = inventor_id(0x9087_4d98);
const VERTICAL_TYPE: [u8; 16] = inventor_id(0x9087_4d99);
const HORIZONTAL_DISTANCE_TYPE: [u8; 16] = [
    0x00, 0xc0, 0xac, 0x00, 0xd1, 0x11, 0x5f, 0xe0, 0x80, 0x00, 0x66, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const VERTICAL_DISTANCE_TYPE: [u8; 16] = [
    0x40, 0xff, 0x83, 0x36, 0xd1, 0x11, 0x5f, 0xe0, 0x80, 0x00, 0x66, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const RADIUS_TYPE: [u8; 16] = [
    0x00, 0xb7, 0x1b, 0x67, 0xd1, 0x11, 0x68, 0xe0, 0x80, 0x00, 0x66, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const DIAMETER_TYPE: [u8; 16] = [
    0xe0, 0x96, 0xdf, 0x74, 0xd1, 0x11, 0x69, 0xe0, 0x80, 0x00, 0x66, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const CIRCLE_CENTER_TYPE: [u8; 16] = [
    0x00, 0x8c, 0x10, 0xe1, 0xd1, 0x11, 0x02, 0xe6, 0x80, 0x00, 0x6d, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const EQUAL_RADIUS_TYPE: [u8; 16] = [
    0xd0, 0x7d, 0x2c, 0x44, 0xd1, 0x11, 0x89, 0xe6, 0x80, 0x00, 0x6f, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];

const fn inventor_id(time_low: u32) -> [u8; 16] {
    let first = time_low.to_le_bytes();
    [
        first[0], first[1], first[2], first[3], 0xd0, 0x11, 0xf8, 0xd1, 0x00, 0x08, 0xca, 0xbc,
        0x06, 0x63, 0xdc, 0x09,
    ]
}

const fn sketch_entity_id(time_low: u32) -> [u8; 16] {
    let first = time_low.to_le_bytes();
    [
        first[0], first[1], first[2], first[3], 0xd0, 0x11, 0xd0, 0xd2, 0x00, 0x08, 0xcc, 0xbc,
        0x06, 0x63, 0xdc, 0x09,
    ]
}

#[derive(Debug)]
pub(crate) struct SketchInventory {
    pub(crate) sketches: Vec<PmDcSketch>,
    pub(crate) entities: Vec<PmDcSketchEntity>,
    pub(crate) transforms: Vec<PmDcTransform>,
    pub(crate) directions: Vec<PmDcDirection>,
    pub(crate) constraints: Vec<PmDcSketchConstraint>,
    pub(crate) issues: Vec<RecordIssue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcSketchConstraintPayload {
    pub(crate) save_version_major: u8,
    pub(crate) header: PmDcConstraintHeader,
    pub(crate) kind: PmDcSketchConstraintKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcConstraintHeader {
    pub(crate) content: PmDcContentHeader,
    pub(crate) state: i32,
    pub(crate) group: PmDcReference,
    pub(crate) scalar_map: PmDcReferenceScalarMap,
    pub(crate) reference_map: PmDcReferencePairMap,
    pub(crate) parameter: PmDcReference,
}

pub(crate) type PmDcReferenceScalarMap = crate::pmdc::PmDcPairedMap<f64>;
pub(crate) type PmDcReferencePairMap = crate::pmdc::PmDcPairedMap<PmDcReference>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum PmDcSketchConstraintKind {
    Coincident {
        first: PmDcReference,
        second: PmDcReference,
    },
    Parallel {
        first: PmDcReference,
        second: PmDcReference,
        orientation: u16,
    },
    Perpendicular {
        first: PmDcReference,
        second: PmDcReference,
        orientation: u16,
    },
    Tangent {
        first: PmDcReference,
        second: PmDcReference,
        extension: Option<u32>,
    },
    Horizontal {
        entity: PmDcReference,
        state: u8,
    },
    Vertical {
        entity: PmDcReference,
        state: u8,
    },
    HorizontalDistance {
        first: PmDcReference,
        second: PmDcReference,
        parameter: PmDcReference,
        values: [u32; 4],
    },
    VerticalDistance {
        first: PmDcReference,
        second: PmDcReference,
        parameter: PmDcReference,
        values: [u32; 4],
    },
    Radius {
        state: u32,
        entity: PmDcReference,
        values: [u32; 4],
    },
    Diameter {
        reference: PmDcReference,
        entity: PmDcReference,
        values: [u32; 4],
    },
    CircleCenter {
        entity: PmDcReference,
        center: PmDcReference,
    },
    EqualRadius {
        first: PmDcReference,
        second: PmDcReference,
    },
}

pub(crate) struct SketchProjection {
    pub(crate) sketches: Vec<Sketch>,
    pub(crate) entities: Vec<SketchEntity>,
    pub(crate) constraints: Vec<SketchConstraint>,
    pub(crate) unresolved_sketches: usize,
    pub(crate) unresolved_entities: usize,
    pub(crate) unresolved_constraints: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcSketchPayload {
    pub(crate) save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    pub(crate) state: i32,
    pub(crate) count_value: u32,
    pub(crate) entities: PmDcReferenceList,
    pub(crate) transform: PmDcReference,
    pub(crate) direction: PmDcReference,
    pub(crate) values: [u32; 2],
    pub(crate) auxiliary: Option<PmDcReferenceList>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcSketchEntityPayload {
    pub(crate) save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    pub(crate) entity_flags: u32,
    pub(crate) sketch: PmDcReference,
    pub(crate) kind: PmDcSketchEntityKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum PmDcSketchEntityKind {
    Point {
        position: [f64; 2],
        endpoint_of: PmDcReferenceList,
        center_of: PmDcReferenceList,
        #[serde(flatten)]
        tail: PointTail,
    },
    Line {
        points: PmDcReferenceList,
        auxiliary: Vec<PmDcReferenceList>,
        origin: [f64; 2],
        direction: [f64; 2],
    },
    Circle {
        points: PmDcReferenceList,
        auxiliary: Vec<PmDcReferenceList>,
        center: PmDcReference,
        radius: f64,
        state: u8,
    },
    Ellipse {
        points: PmDcReferenceList,
        auxiliary: Vec<PmDcReferenceList>,
        center: PmDcReference,
        major_direction: [f64; 2],
        major_radius: f64,
        minor_radius: f64,
        state: u8,
    },
}

/// The absent or complete state and association tail of a sketch point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PointTailWire", into = "PointTailWire")]
pub(crate) enum PointTail {
    Absent,
    Present {
        state: u32,
        associations: PmDcReferenceList,
    },
}

#[derive(Serialize, Deserialize)]
struct PointTailWire {
    state: Option<u32>,
    associations: Option<PmDcReferenceList>,
}

impl From<PointTail> for PointTailWire {
    fn from(tail: PointTail) -> Self {
        match tail {
            PointTail::Absent => Self {
                state: None,
                associations: None,
            },
            PointTail::Present {
                state,
                associations,
            } => Self {
                state: Some(state),
                associations: Some(associations),
            },
        }
    }
}

impl TryFrom<PointTailWire> for PointTail {
    type Error = &'static str;

    fn try_from(wire: PointTailWire) -> Result<Self, Self::Error> {
        match (wire.state, wire.associations) {
            (Some(state), Some(associations)) => Ok(Self::Present {
                state,
                associations,
            }),
            (None, None) => Ok(Self::Absent),
            _ => Err("point state and associations must be present together"),
        }
    }
}

const TRANSFORM_PREFIX: u32 = 0x203;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "PmDcTransformPayloadWire",
    into = "PmDcTransformPayloadWire"
)]
pub(crate) struct PmDcTransformPayload {
    pub(crate) save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    pub(crate) prefix_present: bool,
    pub(crate) matrix: CompactMatrix,
}

#[derive(Serialize, Deserialize)]
struct PmDcTransformPayloadWire {
    save_version_major: u8,
    header: PmDcContentHeader,
    prefix: Option<u32>,
    #[serde(flatten)]
    matrix: CompactMatrix,
}

impl From<PmDcTransformPayload> for PmDcTransformPayloadWire {
    fn from(value: PmDcTransformPayload) -> Self {
        Self {
            save_version_major: value.save_version_major,
            header: value.header,
            prefix: value.prefix_present.then_some(TRANSFORM_PREFIX),
            matrix: value.matrix,
        }
    }
}

impl TryFrom<PmDcTransformPayloadWire> for PmDcTransformPayload {
    type Error = &'static str;

    fn try_from(wire: PmDcTransformPayloadWire) -> Result<Self, Self::Error> {
        let prefix_present = match wire.prefix {
            None => false,
            Some(TRANSFORM_PREFIX) => true,
            Some(_) => return Err("transform prefix must be 515 or null"),
        };
        Ok(Self {
            save_version_major: wire.save_version_major,
            header: wire.header,
            prefix_present,
            matrix: wire.matrix,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcDirectionPayload {
    pub(crate) save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    pub(crate) entity_flags: u32,
    pub(crate) parameter: f64,
    pub(crate) extension: Option<u32>,
    pub(crate) direction: [f64; 3],
}

pub(crate) fn inventory(
    ctx: &DecodeContext<'_>,
    document: &RseInventory<'_>,
) -> Result<SketchInventory, CodecError> {
    let mut inventory = SketchInventory {
        sketches: Vec::new(),
        entities: Vec::new(),
        transforms: Vec::new(),
        directions: Vec::new(),
        constraints: Vec::new(),
        issues: Vec::new(),
    };
    for segment in &document.segments {
        if segment.kind != SegmentKind::PmDc {
            continue;
        }
        let Some(version) = segment.registry.map(|join| join.version_major) else {
            continue;
        };
        if !(15..=22).contains(&version) {
            continue;
        }
        let SegmentBulkState::Framed(bulk) = &segment.bulk else {
            continue;
        };
        let RecordFrameState::Framed(table) = &bulk.records else {
            continue;
        };
        for record in &table.records {
            let Some(tag) = SketchRecordTag::from_type_id(record.type_id) else {
                continue;
            };
            let result = match tag {
                SketchRecordTag::Sketch => {
                    parse_sketch(ctx, record.payload, version).map(|value| {
                        inventory.sketches.push(Located::new(
                            value,
                            type_id_string(record.type_id),
                            segment.pair.token.as_str(),
                            record.ordinal,
                        ));
                    })
                }
                SketchRecordTag::Entity(entity) => {
                    parse_entity(ctx, entity, record.payload, version).map(|value| {
                        inventory.entities.push(Located::new(
                            value,
                            type_id_string(record.type_id),
                            segment.pair.token.as_str(),
                            record.ordinal,
                        ));
                    })
                }
                SketchRecordTag::Transform => {
                    parse_transform(record.payload, version).map(|value| {
                        inventory.transforms.push(Located::new(
                            value,
                            type_id_string(record.type_id),
                            segment.pair.token.as_str(),
                            record.ordinal,
                        ));
                    })
                }
                SketchRecordTag::Direction => {
                    parse_direction(record.payload, version).map(|value| {
                        inventory.directions.push(Located::new(
                            value,
                            type_id_string(record.type_id),
                            segment.pair.token.as_str(),
                            record.ordinal,
                        ));
                    })
                }
                SketchRecordTag::Constraint(constraint) => {
                    parse_constraint(ctx, constraint, record.payload, version).map(|value| {
                        inventory.constraints.push(Located::new(
                            value,
                            type_id_string(record.type_id),
                            segment.pair.token.as_str(),
                            record.ordinal,
                        ));
                    })
                }
            };
            if let Err(error) = result {
                inventory.issues.push(RecordIssue {
                    family: RecordIssueFamily::Sketch {
                        type_id: type_id_string(record.type_id),
                    },
                    segment_token: segment.pair.token.as_str().into(),
                    record_ordinal: record.ordinal,
                    detail: crate::issue_detail(error)?,
                });
            }
        }
    }
    ctx.charge_collection_items(
        inventory
            .sketches
            .len()
            .saturating_add(inventory.entities.len())
            .saturating_add(inventory.transforms.len())
            .saturating_add(inventory.directions.len())
            .saturating_add(inventory.constraints.len())
            .saturating_add(inventory.issues.len()) as u64,
        "admit Inventor planar-sketch records",
    )?;
    Ok(inventory)
}

fn parse_sketch(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcSketchPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let state = cursor.u32()? as i32;
    let count_value = cursor.u32()?;
    let entities = reference_list(ctx, &mut cursor, 8, "sketch entity array")?;
    let transform = cursor.reference()?;
    let direction = cursor.reference()?;
    let values = [cursor.u32()?, cursor.u32()?];
    let auxiliary = (cursor.remaining() != 0)
        .then(|| reference_list(ctx, &mut cursor, 2, "sketch auxiliary list"))
        .transpose()?;
    cursor.finish("sketch")?;
    Ok(PmDcSketchPayload {
        save_version_major: version,
        header,
        state,
        count_value,
        entities,
        transform,
        direction,
        values,
        auxiliary,
    })
}

#[derive(Clone, Copy)]
enum SketchEntityTag {
    Point,
    Line,
    Circle,
    Ellipse,
}

impl SketchEntityTag {
    fn from_type_id(type_id: [u8; 16]) -> Option<Self> {
        match type_id {
            POINT_TYPE => Some(Self::Point),
            LINE_TYPE => Some(Self::Line),
            CIRCLE_TYPE => Some(Self::Circle),
            ELLIPSE_TYPE => Some(Self::Ellipse),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
enum SketchConstraintTag {
    Coincident,
    Parallel,
    Perpendicular,
    Tangent,
    Horizontal,
    Vertical,
    HorizontalDistance,
    VerticalDistance,
    Radius,
    Diameter,
    CircleCenter,
    EqualRadius,
}

impl SketchConstraintTag {
    fn from_type_id(type_id: [u8; 16]) -> Option<Self> {
        match type_id {
            COINCIDENT_TYPE => Some(Self::Coincident),
            PARALLEL_TYPE => Some(Self::Parallel),
            PERPENDICULAR_TYPE => Some(Self::Perpendicular),
            TANGENT_TYPE => Some(Self::Tangent),
            HORIZONTAL_TYPE => Some(Self::Horizontal),
            VERTICAL_TYPE => Some(Self::Vertical),
            HORIZONTAL_DISTANCE_TYPE => Some(Self::HorizontalDistance),
            VERTICAL_DISTANCE_TYPE => Some(Self::VerticalDistance),
            RADIUS_TYPE => Some(Self::Radius),
            DIAMETER_TYPE => Some(Self::Diameter),
            CIRCLE_CENTER_TYPE => Some(Self::CircleCenter),
            EQUAL_RADIUS_TYPE => Some(Self::EqualRadius),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
enum SketchRecordTag {
    Sketch,
    Transform,
    Direction,
    Entity(SketchEntityTag),
    Constraint(SketchConstraintTag),
}

impl SketchRecordTag {
    fn from_type_id(type_id: [u8; 16]) -> Option<Self> {
        match type_id {
            SKETCH_TYPE => Some(Self::Sketch),
            TRANSFORM_TYPE => Some(Self::Transform),
            DIRECTION_TYPE => Some(Self::Direction),
            _ => SketchEntityTag::from_type_id(type_id)
                .map(Self::Entity)
                .or_else(|| SketchConstraintTag::from_type_id(type_id).map(Self::Constraint)),
        }
    }
}

fn parse_entity(
    ctx: &DecodeContext<'_>,
    tag: SketchEntityTag,
    source: View<'_>,
    version: u8,
) -> Result<PmDcSketchEntityPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let entity_flags = cursor.u32()?;
    let sketch = cursor.reference()?;
    let kind = match tag {
        SketchEntityTag::Point => parse_point(ctx, &mut cursor)?,
        SketchEntityTag::Line => parse_line(ctx, &mut cursor)?,
        SketchEntityTag::Circle => parse_circle(ctx, &mut cursor)?,
        SketchEntityTag::Ellipse => parse_ellipse(ctx, &mut cursor)?,
    };
    cursor.finish("sketch entity")?;
    Ok(PmDcSketchEntityPayload {
        save_version_major: version,
        header,
        entity_flags,
        sketch,
        kind,
    })
}

fn point2(cursor: &mut Cursor<'_>, field: &str) -> Result<[f64; 2], CodecError> {
    Ok([
        cursor.f64(&format!("{field} u"))?,
        cursor.f64(&format!("{field} v"))?,
    ])
}

fn point3(cursor: &mut Cursor<'_>, field: &str) -> Result<[f64; 3], CodecError> {
    Ok([
        cursor.f64(&format!("{field} x"))?,
        cursor.f64(&format!("{field} y"))?,
        cursor.f64(&format!("{field} z"))?,
    ])
}

fn parse_point(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<PmDcSketchEntityKind, CodecError> {
    let position = point2(cursor, "sketch point")?;
    let endpoint_of = reference_list(ctx, cursor, 2, "point endpoint-of list")?;
    let center_of = reference_list(ctx, cursor, 2, "point center-of list")?;
    let tail = if cursor.remaining() == 0 {
        PointTail::Absent
    } else {
        PointTail::Present {
            state: cursor.u32()?,
            associations: reference_list(ctx, cursor, 2, "point association list")?,
        }
    };
    Ok(PmDcSketchEntityKind::Point {
        position,
        endpoint_of,
        center_of,
        tail,
    })
}

fn edge_prefix(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    fixed_tail: usize,
    field: &str,
) -> Result<(PmDcReferenceList, Vec<PmDcReferenceList>), CodecError> {
    let points = reference_list(ctx, cursor, 2, &format!("{field} point list"))?;
    let mut auxiliary = Vec::new();
    if cursor.remaining() >= fixed_tail.saturating_add(8) && cursor.peek_u32()? == 0x3000_0002 {
        auxiliary.push(reference_list(
            ctx,
            cursor,
            2,
            &format!("{field} auxiliary list 0"),
        )?);
    } else if cursor.remaining() >= fixed_tail.saturating_add(16) {
        let gate = [cursor.u32()?, cursor.u32()?];
        if gate != [1, 0] {
            return Err(CodecError::malformed(format_args!(
                "Inventor PmDc {field} list gate is {gate:?}"
            )));
        }
        auxiliary.push(reference_list(
            ctx,
            cursor,
            2,
            &format!("{field} auxiliary list 0"),
        )?);
        if cursor.remaining() >= fixed_tail.saturating_add(8) && cursor.peek_u32()? == 0x3000_0002 {
            auxiliary.push(reference_list(
                ctx,
                cursor,
                2,
                &format!("{field} auxiliary list 1"),
            )?);
        }
    }
    if cursor.remaining() != fixed_tail {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc {field} has {} bytes before its fixed tail, expected {fixed_tail}",
            cursor.remaining()
        )));
    }
    Ok((points, auxiliary))
}

fn parse_line(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<PmDcSketchEntityKind, CodecError> {
    let (points, auxiliary) = edge_prefix(ctx, cursor, 32, "line")?;
    let origin = point2(cursor, "line origin")?;
    let direction = point2(cursor, "line direction")?;
    Ok(PmDcSketchEntityKind::Line {
        points,
        auxiliary,
        origin,
        direction,
    })
}

fn parse_circle(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<PmDcSketchEntityKind, CodecError> {
    let (points, auxiliary) = edge_prefix(ctx, cursor, 13, "circle")?;
    let center = cursor.reference()?;
    let radius = cursor.f64("circle radius")?;
    let state = cursor.u8()?;
    if radius <= 0.0 {
        return Err(CodecError::Malformed(
            "Inventor PmDc circle radius is not positive".into(),
        ));
    }
    Ok(PmDcSketchEntityKind::Circle {
        points,
        auxiliary,
        center,
        radius,
        state,
    })
}

fn parse_ellipse(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<PmDcSketchEntityKind, CodecError> {
    let (points, auxiliary) = edge_prefix(ctx, cursor, 37, "ellipse")?;
    let center = cursor.reference()?;
    let major_direction = point2(cursor, "ellipse major direction")?;
    let major_radius = cursor.f64("ellipse major radius")?;
    let minor_radius = cursor.f64("ellipse minor radius")?;
    let state = cursor.u8()?;
    if major_radius <= 0.0 || minor_radius <= 0.0 {
        return Err(CodecError::Malformed(
            "Inventor PmDc ellipse radius is not positive".into(),
        ));
    }
    Ok(PmDcSketchEntityKind::Ellipse {
        points,
        auxiliary,
        center,
        major_direction,
        major_radius,
        minor_radius,
        state,
    })
}

fn parse_transform(source: View<'_>, version: u8) -> Result<PmDcTransformPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let prefix_present = cursor.peek_u32()? == TRANSFORM_PREFIX;
    if prefix_present {
        cursor.u32()?;
    }
    let value_mask = cursor.u16()?;
    let zero_mask = cursor.u16()?;
    let matrix = CompactMatrix::try_new(value_mask, zero_mask, |_| {
        cursor.f64("transform explicit value")
    })?;
    cursor.finish("transform")?;
    Ok(PmDcTransformPayload {
        save_version_major: version,
        header,
        prefix_present,
        matrix,
    })
}

fn parse_direction(source: View<'_>, version: u8) -> Result<PmDcDirectionPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let entity_flags = cursor.u32()?;
    let parameter = cursor.f64("direction parameter")?;
    let extension = match cursor.remaining() {
        24 => None,
        28 => Some(cursor.u32()?),
        remaining => {
            return Err(CodecError::malformed(format_args!(
                "Inventor PmDc direction has {remaining} bytes before its vector"
            )));
        }
    };
    let direction = point3(&mut cursor, "direction vector")?;
    cursor.finish("direction")?;
    Ok(PmDcDirectionPayload {
        save_version_major: version,
        header,
        entity_flags,
        parameter,
        extension,
        direction,
    })
}

fn map_header(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    field: &str,
) -> Result<(usize, Option<[u32; 2]>), CodecError> {
    let marker = [cursor.u16()?, cursor.u16()?];
    if marker != [6, 0x3000] {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc {field} marker is {marker:?}"
        )));
    }
    let count = cursor.u32()? as usize;
    ctx.charge_collection_items(count as u64, "admit Inventor sketch constraint map")?;
    let metadata = (count != 0)
        .then(|| Ok::<_, CodecError>([cursor.u32()?, cursor.u32()?]))
        .transpose()?;
    Ok((count, metadata))
}

fn reference_scalar_map(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<PmDcReferenceScalarMap, CodecError> {
    let (count, metadata) = map_header(ctx, cursor, "constraint scalar map")?;
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        entries.push((
            cursor.reference()?,
            cursor.f64(&format!("constraint scalar-map value {index}"))?,
        ));
    }
    PmDcReferenceScalarMap::new(metadata, entries).ok_or_else(|| {
        CodecError::Malformed("Inventor PmDc scalar map metadata disagrees with length".into())
    })
}

fn reference_pair_map(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<PmDcReferencePairMap, CodecError> {
    let (count, metadata) = map_header(ctx, cursor, "constraint reference map")?;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        entries.push((cursor.reference()?, cursor.reference()?));
    }
    PmDcReferencePairMap::new(metadata, entries).ok_or_else(|| {
        CodecError::Malformed("Inventor PmDc pair map metadata disagrees with length".into())
    })
}

fn parse_constraint_header(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    version: u8,
) -> Result<PmDcConstraintHeader, CodecError> {
    let content = content_header(cursor)?;
    let state = cursor.u32()? as i32;
    let group = cursor.reference()?;
    let (scalar_map, reference_map) = if version <= 16 {
        (
            PmDcReferenceScalarMap::new(None, Vec::new()).expect("empty scalar map"),
            PmDcReferencePairMap::new(None, Vec::new()).expect("empty pair map"),
        )
    } else {
        (
            reference_scalar_map(ctx, cursor)?,
            reference_pair_map(ctx, cursor)?,
        )
    };
    Ok(PmDcConstraintHeader {
        content,
        state,
        group,
        scalar_map,
        reference_map,
        parameter: cursor.reference()?,
    })
}

fn parse_constraint(
    ctx: &DecodeContext<'_>,
    tag: SketchConstraintTag,
    source: View<'_>,
    version: u8,
) -> Result<PmDcSketchConstraintPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = parse_constraint_header(ctx, &mut cursor, version)?;
    let kind = match tag {
        SketchConstraintTag::Coincident => PmDcSketchConstraintKind::Coincident {
            first: cursor.reference()?,
            second: cursor.reference()?,
        },
        SketchConstraintTag::Parallel => PmDcSketchConstraintKind::Parallel {
            first: cursor.reference()?,
            second: cursor.reference()?,
            orientation: cursor.u16()?,
        },
        SketchConstraintTag::Perpendicular => PmDcSketchConstraintKind::Perpendicular {
            first: cursor.reference()?,
            second: cursor.reference()?,
            orientation: cursor.u16()?,
        },
        SketchConstraintTag::Tangent => PmDcSketchConstraintKind::Tangent {
            first: cursor.reference()?,
            second: cursor.reference()?,
            extension: (cursor.remaining() == 4)
                .then(|| cursor.u32())
                .transpose()?,
        },
        SketchConstraintTag::Horizontal => PmDcSketchConstraintKind::Horizontal {
            entity: cursor.reference()?,
            state: cursor.u8()?,
        },
        SketchConstraintTag::Vertical => PmDcSketchConstraintKind::Vertical {
            entity: cursor.reference()?,
            state: cursor.u8()?,
        },
        SketchConstraintTag::HorizontalDistance => {
            let (first, second, parameter, values) = distance_constraint_fields(&mut cursor)?;
            PmDcSketchConstraintKind::HorizontalDistance {
                first,
                second,
                parameter,
                values,
            }
        }
        SketchConstraintTag::VerticalDistance => {
            let (first, second, parameter, values) = distance_constraint_fields(&mut cursor)?;
            PmDcSketchConstraintKind::VerticalDistance {
                first,
                second,
                parameter,
                values,
            }
        }
        SketchConstraintTag::Radius => PmDcSketchConstraintKind::Radius {
            state: cursor.u32()?,
            entity: cursor.reference()?,
            values: u32_array::<4>(&mut cursor)?,
        },
        SketchConstraintTag::Diameter => PmDcSketchConstraintKind::Diameter {
            reference: cursor.reference()?,
            entity: cursor.reference()?,
            values: u32_array::<4>(&mut cursor)?,
        },
        SketchConstraintTag::CircleCenter => PmDcSketchConstraintKind::CircleCenter {
            entity: cursor.reference()?,
            center: cursor.reference()?,
        },
        SketchConstraintTag::EqualRadius => PmDcSketchConstraintKind::EqualRadius {
            first: cursor.reference()?,
            second: cursor.reference()?,
        },
    };
    cursor.finish("sketch constraint")?;
    Ok(PmDcSketchConstraintPayload {
        save_version_major: version,
        header,
        kind,
    })
}

fn distance_constraint_fields(
    cursor: &mut Cursor<'_>,
) -> Result<(PmDcReference, PmDcReference, PmDcReference, [u32; 4]), CodecError> {
    let first = cursor.reference()?;
    let second = cursor.reference()?;
    let parameter = cursor.reference()?;
    let values = u32_array::<4>(cursor)?;
    Ok((first, second, parameter, values))
}

fn u32_array<const N: usize>(cursor: &mut Cursor<'_>) -> Result<[u32; N], CodecError> {
    let mut values = [0; N];
    for value in &mut values {
        *value = cursor.u32()?;
    }
    Ok(values)
}

pub(crate) fn project(
    inventory: &SketchInventory,
    parameters: &[DesignParameter],
) -> SketchProjection {
    let raw_sketches = unique_by(&inventory.sketches, |record| {
        (
            record.identity.segment_token.as_str(),
            record.identity.record_ordinal,
        )
    });
    let raw_entities = unique_by(&inventory.entities, |record| {
        (
            record.identity.segment_token.as_str(),
            record.identity.record_ordinal,
        )
    });
    let transforms = unique_by(&inventory.transforms, |record| {
        (
            record.identity.segment_token.as_str(),
            record.identity.record_ordinal,
        )
    });
    let directions = unique_by(&inventory.directions, |record| {
        (
            record.identity.segment_token.as_str(),
            record.identity.record_ordinal,
        )
    });
    let raw_constraints = unique_by(&inventory.constraints, |record| {
        (
            record.identity.segment_token.as_str(),
            record.identity.record_ordinal,
        )
    });
    let parameters = parameters
        .iter()
        .filter_map(|parameter| {
            parameter
                .native_ref
                .as_ref()
                .map(|native| (native.clone(), parameter.id.clone()))
        })
        .collect::<HashMap<_, _>>();

    let mut projected_entities = Vec::new();
    let mut unresolved_entities = 0usize;
    for entity in &inventory.entities {
        let key = (
            entity.identity.segment_token.as_str(),
            entity.identity.record_ordinal,
        );
        if !raw_entities.contains_key(&key) {
            unresolved_entities += 1;
            continue;
        }
        let Some(sketch_ordinal) = entity.sketch.index.checked_sub(1) else {
            unresolved_entities += 1;
            continue;
        };
        let Some(sketch) =
            raw_sketches.get(&(entity.identity.segment_token.as_str(), sketch_ordinal))
        else {
            unresolved_entities += 1;
            continue;
        };
        if !sketch
            .entities
            .references()
            .iter()
            .any(|reference| reference.index == entity.identity.record_ordinal.saturating_add(1))
        {
            unresolved_entities += 1;
            continue;
        }
        let Some(geometry) = project_geometry(entity, &raw_entities) else {
            unresolved_entities += 1;
            continue;
        };
        let (Some(entity_id), Some(sketch_id)) = (entity_id(entity), sketch_id(sketch)) else {
            unresolved_entities += 1;
            continue;
        };
        projected_entities.push(
            SketchEntity::new(entity_id, sketch_id, geometry)
                .with_construction(entity.entity_flags & 0x0408_0040 != 0)
                .with_native_ref(Some(entity.id()))
                .with_endpoint_refs(entity_endpoint_refs(entity, &raw_entities)),
        );
    }

    let projected_by_native = projected_entities
        .iter()
        .filter_map(|entity| entity.native_ref.as_deref().map(|native| (native, entity)))
        .collect::<HashMap<_, _>>();
    let mut sketches = Vec::new();
    let mut unresolved_sketches = 0usize;
    for sketch in &inventory.sketches {
        let key = (
            sketch.identity.segment_token.as_str(),
            sketch.identity.record_ordinal,
        );
        if !raw_sketches.contains_key(&key) {
            unresolved_sketches += 1;
            continue;
        }
        let raw_referenced_entities = sketch
            .entities
            .references()
            .iter()
            .filter_map(|reference| {
                let ordinal = reference.index.checked_sub(1)?;
                raw_entities
                    .get(&(sketch.identity.segment_token.as_str(), ordinal))
                    .copied()
            })
            .collect::<Vec<_>>();
        let referenced_entities = raw_referenced_entities
            .iter()
            .filter_map(|raw| projected_by_native.get(raw.id().as_str()).copied())
            .collect::<Vec<_>>();
        if referenced_entities.len() != raw_referenced_entities.len() {
            unresolved_sketches += 1;
            continue;
        }
        let Some(placement) = project_placement(sketch, &transforms, &directions) else {
            unresolved_sketches += 1;
            continue;
        };
        let Some(id) = sketch_id(sketch) else {
            unresolved_sketches += 1;
            continue;
        };
        let Ok(profiles) =
            cadmpeg_ir::sketches::SketchProfiles::try_from(build_profiles(&referenced_entities))
        else {
            unresolved_sketches += 1;
            continue;
        };
        sketches.push(Sketch {
            id,
            name: None,
            configuration: None,
            visible: None,
            placement,
            profiles,
            native_ref: Some(sketch.id()),
        });
    }
    drop(projected_by_native);
    let projected_sketch_ids = sketches
        .iter()
        .map(|sketch| sketch.id.clone())
        .collect::<HashSet<_>>();
    let previous_entity_count = projected_entities.len();
    projected_entities.retain(|entity| projected_sketch_ids.contains(&entity.sketch));
    unresolved_entities = unresolved_entities
        .saturating_add(previous_entity_count.saturating_sub(projected_entities.len()));
    let projected_by_native = projected_entities
        .iter()
        .filter_map(|entity| entity.native_ref.as_deref().map(|native| (native, entity)))
        .collect::<HashMap<_, _>>();
    let projected_entity_by_key = inventory
        .entities
        .iter()
        .filter_map(|raw| {
            projected_by_native.get(raw.id().as_str()).map(|projected| {
                (
                    (
                        raw.identity.segment_token.as_str(),
                        raw.identity.record_ordinal,
                    ),
                    *projected,
                )
            })
        })
        .collect::<HashMap<_, _>>();
    let mut constraints = inventory
        .constraints
        .iter()
        .filter(|constraint| {
            raw_constraints.contains_key(&(
                constraint.identity.segment_token.as_str(),
                constraint.identity.record_ordinal,
            ))
        })
        .filter_map(|constraint| {
            project_constraint(constraint, &projected_entity_by_key, &parameters)
        })
        .collect::<Vec<_>>();
    let mut unresolved_constraints = inventory
        .constraints
        .len()
        .saturating_sub(constraints.len());
    let projected_entity_native = projected_entities
        .iter()
        .filter_map(|entity| entity.native_ref.as_deref())
        .collect::<HashSet<_>>();
    let projected_constraint_native = constraints
        .iter()
        .filter_map(|constraint| constraint.native_ref.as_deref())
        .collect::<HashSet<_>>();
    let raw_sketch_by_native = inventory
        .sketches
        .iter()
        .map(|sketch| (sketch.id(), sketch))
        .collect::<HashMap<_, _>>();
    let previous_sketch_count = sketches.len();
    sketches.retain(|projected| {
        let Some(raw) = projected
            .native_ref
            .as_deref()
            .and_then(|native| raw_sketch_by_native.get(native).copied())
        else {
            return false;
        };
        let mut seen = HashSet::new();
        raw.entities.references().iter().all(|reference| {
            let Some(ordinal) = reference.index.checked_sub(1) else {
                return false;
            };
            if !seen.insert(ordinal) {
                return false;
            }
            let key = (raw.identity.segment_token.as_str(), ordinal);
            raw_entities
                .get(&key)
                .is_some_and(|entity| projected_entity_native.contains(entity.id().as_str()))
                || raw_constraints.get(&key).is_some_and(|constraint| {
                    projected_constraint_native.contains(constraint.id().as_str())
                })
        })
    });
    unresolved_sketches =
        unresolved_sketches.saturating_add(previous_sketch_count.saturating_sub(sketches.len()));
    let closed_sketch_ids = sketches
        .iter()
        .map(|sketch| sketch.id.clone())
        .collect::<HashSet<_>>();
    let previous_entity_count = projected_entities.len();
    projected_entities.retain(|entity| closed_sketch_ids.contains(&entity.sketch));
    unresolved_entities = unresolved_entities
        .saturating_add(previous_entity_count.saturating_sub(projected_entities.len()));
    let raw_constraint_by_native = inventory
        .constraints
        .iter()
        .map(|constraint| (constraint.id(), constraint))
        .collect::<HashMap<_, _>>();
    let raw_sketch_by_id = inventory
        .sketches
        .iter()
        .filter_map(|sketch| Some((sketch_id(sketch)?, sketch)))
        .collect::<HashMap<_, _>>();
    let previous_constraint_count = constraints.len();
    constraints.retain(|constraint| {
        if !closed_sketch_ids.contains(&constraint.sketch) {
            return false;
        }
        let Some(raw_constraint) = constraint
            .native_ref
            .as_deref()
            .and_then(|native| raw_constraint_by_native.get(native).copied())
        else {
            return false;
        };
        raw_sketch_by_id
            .get(&constraint.sketch)
            .is_some_and(|sketch| {
                sketch.entities.references().iter().any(|reference| {
                    reference.index == raw_constraint.identity.record_ordinal.saturating_add(1)
                })
            })
    });
    unresolved_constraints = unresolved_constraints
        .saturating_add(previous_constraint_count.saturating_sub(constraints.len()));
    SketchProjection {
        sketches,
        entities: projected_entities,
        constraints,
        unresolved_sketches,
        unresolved_entities,
        unresolved_constraints,
    }
}

fn project_constraint(
    constraint: &PmDcSketchConstraint,
    entities: &HashMap<(&str, u32), &SketchEntity>,
    parameters: &HashMap<String, ParameterId>,
) -> Option<SketchConstraint> {
    if constraint.header.scalar_map.metadata().is_some()
        || !constraint.header.scalar_map.entries().is_empty()
        || constraint.header.reference_map.metadata().is_some()
        || !constraint.header.reference_map.entries().is_empty()
    {
        return None;
    }
    let resolve = |reference: PmDcReference| {
        entities
            .get(&(
                constraint.identity.segment_token.as_str(),
                reference.index.checked_sub(1)?,
            ))
            .copied()
    };
    let (definition, orientation, members) = match constraint.kind {
        PmDcSketchConstraintKind::Coincident { first, second } => {
            let members = [resolve(first)?, resolve(second)?];
            (
                SketchConstraintDefinitionInput::Coincident {
                    entities: members.iter().map(|entity| entity.id().clone()).collect(),
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::Parallel {
            first,
            second,
            orientation,
        } => {
            let members = [resolve(first)?, resolve(second)?];
            (
                SketchConstraintDefinitionInput::Parallel {
                    first: members[0].id().clone(),
                    second: members[1].id().clone(),
                },
                Some(u32::from(orientation)),
                members,
            )
        }
        PmDcSketchConstraintKind::Perpendicular {
            first,
            second,
            orientation,
        } => {
            let members = [resolve(first)?, resolve(second)?];
            (
                SketchConstraintDefinitionInput::Perpendicular {
                    first: members[0].id().clone(),
                    second: members[1].id().clone(),
                },
                Some(u32::from(orientation)),
                members,
            )
        }
        PmDcSketchConstraintKind::Tangent {
            first,
            second,
            extension,
        } => {
            let members = [resolve(first)?, resolve(second)?];
            (
                SketchConstraintDefinitionInput::Tangent {
                    first: members[0].id().clone(),
                    second: members[1].id().clone(),
                },
                extension,
                members,
            )
        }
        PmDcSketchConstraintKind::Horizontal { entity, state } => {
            let member = resolve(entity)?;
            (
                SketchConstraintDefinitionInput::Horizontal {
                    entity: member.id().clone(),
                },
                Some(u32::from(state)),
                [member, member],
            )
        }
        PmDcSketchConstraintKind::Vertical { entity, state } => {
            let member = resolve(entity)?;
            (
                SketchConstraintDefinitionInput::Vertical {
                    entity: member.id().clone(),
                },
                Some(u32::from(state)),
                [member, member],
            )
        }
        PmDcSketchConstraintKind::HorizontalDistance {
            first,
            second,
            parameter,
            ..
        } => {
            let members = [resolve(first)?, resolve(second)?];
            let parameter = resolve_parameter(constraint, parameter, parameters)?;
            (
                SketchConstraintDefinitionInput::HorizontalDistance {
                    first: SketchLocus::Entity(members[0].id().clone()),
                    second: SketchLocus::Entity(members[1].id().clone()),
                    parameter,
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::VerticalDistance {
            first,
            second,
            parameter,
            ..
        } => {
            let members = [resolve(first)?, resolve(second)?];
            let parameter = resolve_parameter(constraint, parameter, parameters)?;
            (
                SketchConstraintDefinitionInput::VerticalDistance {
                    first: SketchLocus::Entity(members[0].id().clone()),
                    second: SketchLocus::Entity(members[1].id().clone()),
                    parameter,
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::Radius { entity, .. } => {
            let member = resolve(entity)?;
            let parameter = resolve_parameter(constraint, constraint.header.parameter, parameters)?;
            (
                SketchConstraintDefinitionInput::Radius {
                    entity: member.id().clone(),
                    parameter,
                },
                None,
                [member, member],
            )
        }
        PmDcSketchConstraintKind::Diameter { entity, .. } => {
            let member = resolve(entity)?;
            let parameter = resolve_parameter(constraint, constraint.header.parameter, parameters)?;
            (
                SketchConstraintDefinitionInput::Diameter {
                    entity: member.id().clone(),
                    parameter,
                },
                None,
                [member, member],
            )
        }
        PmDcSketchConstraintKind::CircleCenter { entity, center } => {
            let members = [resolve(entity)?, resolve(center)?];
            (
                SketchConstraintDefinitionInput::Native {
                    native_kind: cadmpeg_ir::products::NonBlankString::new(
                        "circle_center_alignment",
                    )?,
                    native_state: Some(constraint.header.state as u32 as u64),
                    native_flags: Some(u64::from(constraint.header.content.flags)),
                    native_properties: std::collections::BTreeMap::new(),
                    entities: members.iter().map(|entity| entity.id().clone()).collect(),
                    parameter: None,
                    operands: vec![
                        native_operand(constraint, "entity", entity),
                        native_operand(constraint, "center", center),
                    ],
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::EqualRadius { first, second } => {
            let members = [resolve(first)?, resolve(second)?];
            (
                SketchConstraintDefinitionInput::Equal {
                    first: members[0].id().clone(),
                    second: members[1].id().clone(),
                },
                None,
                members,
            )
        }
    };
    if members[0].sketch != members[1].sketch {
        return None;
    }
    Some(SketchConstraint {
        id: SketchConstraintId::mint(format!(
            "inventor:design:sketch-constraint#{}-{}",
            constraint.identity.segment_token, constraint.identity.record_ordinal
        ))
        .ok()?,
        sketch: members[0].sketch.clone(),
        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(definition).ok()?,
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: Some(constraint.id()),
    })
}

fn resolve_parameter(
    constraint: &PmDcSketchConstraint,
    reference: PmDcReference,
    parameters: &HashMap<String, ParameterId>,
) -> Option<ParameterId> {
    let native = format!(
        "inventor:pmdc:parameter#{}-{}",
        constraint.identity.segment_token,
        reference.index.checked_sub(1)?
    );
    parameters.get(&native).cloned()
}

fn native_operand(
    constraint: &PmDcSketchConstraint,
    field: &str,
    reference: PmDcReference,
) -> SketchNativeOperand {
    SketchNativeOperand {
        native_kind: cadmpeg_ir::products::NonBlankString::new("record_reference")
            .expect("source operand kind is nonempty"),
        field: Some(NativeOperandField {
            name: cadmpeg_ir::products::NonBlankString::new(field)
                .expect("source field name is nonempty"),
            role: None,
        }),
        object_index: Some(reference.index),
        native_ref: reference.index.checked_sub(1).map(|ordinal| {
            format!(
                "inventor:pmdc:sketch-entity#{}-{ordinal}",
                constraint.identity.segment_token
            )
        }),
    }
}

fn project_geometry(
    entity: &PmDcSketchEntity,
    entities: &HashMap<(&str, u32), &PmDcSketchEntity>,
) -> Option<SketchGeometry> {
    match &entity.kind {
        PmDcSketchEntityKind::Point { position, .. } => Some(
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: neutral_point(*position),
            })
            .ok()?,
        ),
        PmDcSketchEntityKind::Line {
            points,
            origin,
            direction,
            ..
        } => {
            let [start, end] = points.references() else {
                return None;
            };
            let start = resolve_point(&entity.identity.segment_token, start.index, entities)?;
            let end = resolve_point(&entity.identity.segment_token, end.index, entities)?;
            if !line_carrier_matches(*origin, *direction, start, end) {
                return None;
            }
            Some(
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: neutral_point(start),
                    end: neutral_point(end),
                })
                .ok()?,
            )
        }
        PmDcSketchEntityKind::Circle { center, radius, .. } => {
            let center = resolve_point(&entity.identity.segment_token, center.index, entities)?;
            Some(
                SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                    center: neutral_point(center),
                    radius: Length::new(radius * 10.0)?,
                })
                .ok()?,
            )
        }
        PmDcSketchEntityKind::Ellipse {
            center,
            major_direction,
            major_radius,
            minor_radius,
            ..
        } => {
            let center = resolve_point(&entity.identity.segment_token, center.index, entities)?;
            let norm = major_direction[0].hypot(major_direction[1]);
            if !norm.is_finite() || norm <= f64::EPSILON {
                return None;
            }
            Some(
                SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
                    center: neutral_point(center),
                    major_angle: Angle::new(major_direction[1].atan2(major_direction[0]))?,
                    major_radius: Length::new(major_radius * 10.0)?,
                    minor_radius: Length::new(minor_radius * 10.0)?,
                    bounds: None,
                })
                .ok()?,
            )
        }
    }
}

fn line_carrier_matches(
    origin: [f64; 2],
    direction: [f64; 2],
    start: [f64; 2],
    end: [f64; 2],
) -> bool {
    let norm = direction[0].hypot(direction[1]);
    let span = [end[0] - start[0], end[1] - start[1]];
    let span_norm = span[0].hypot(span[1]);
    if norm <= f64::EPSILON || span_norm <= f64::EPSILON {
        return false;
    }
    let scale = norm * span_norm;
    let parallel_error = (direction[0] * span[1] - direction[1] * span[0]).abs() / scale;
    let from_origin = [start[0] - origin[0], start[1] - origin[1]];
    let origin_scale = norm * from_origin[0].hypot(from_origin[1]).max(1.0);
    let carrier_error =
        (direction[0] * from_origin[1] - direction[1] * from_origin[0]).abs() / origin_scale;
    parallel_error <= EPS_SKETCH_LINE_CARRIER_MATCHES_E10
        && carrier_error <= EPS_SKETCH_LINE_CARRIER_MATCHES_E10
}

fn resolve_point(
    token: &str,
    reference: u32,
    entities: &HashMap<(&str, u32), &PmDcSketchEntity>,
) -> Option<[f64; 2]> {
    let entity = entities.get(&(token, reference.checked_sub(1)?))?;
    let PmDcSketchEntityKind::Point { position, .. } = entity.kind else {
        return None;
    };
    Some(position)
}

fn neutral_point(value: [f64; 2]) -> Point2 {
    Point2::new(value[0] * 10.0, value[1] * 10.0)
}

fn entity_endpoint_refs(
    entity: &PmDcSketchEntity,
    entities: &HashMap<(&str, u32), &PmDcSketchEntity>,
) -> Vec<String> {
    let PmDcSketchEntityKind::Line { points, .. } = &entity.kind else {
        return Vec::new();
    };
    points
        .references()
        .iter()
        .filter_map(|reference| {
            entities
                .get(&(
                    entity.identity.segment_token.as_str(),
                    reference.index.checked_sub(1)?,
                ))
                .map(|value| value.id())
        })
        .collect()
}

fn project_placement(
    sketch: &PmDcSketch,
    transforms: &HashMap<(&str, u32), &PmDcTransform>,
    directions: &HashMap<(&str, u32), &PmDcDirection>,
) -> Option<SketchPlacement> {
    let transform = transforms.get(&(
        sketch.identity.segment_token.as_str(),
        sketch.transform.index.checked_sub(1)?,
    ))?;
    let direction = directions.get(&(
        sketch.identity.segment_token.as_str(),
        sketch.direction.index.checked_sub(1)?,
    ))?;
    let matrix = transform.matrix.rows();
    if matrix[3]
        .iter()
        .zip([0.0, 0.0, 0.0, 1.0])
        .any(|(actual, expected)| (actual - expected).abs() > EPS_SKETCH_PROJECT_PLACEMENT_E10)
    {
        return None;
    }
    let u_axis = Vector3::new(matrix[0][0], matrix[1][0], matrix[2][0]).unit()?;
    let v_axis = Vector3::new(matrix[0][1], matrix[1][1], matrix[2][1]).unit()?;
    let normal = Vector3::new(matrix[0][2], matrix[1][2], matrix[2][2]).unit()?;
    let stored_direction = Vector3::new(
        direction.direction[0],
        direction.direction[1],
        direction.direction[2],
    )
    .unit()?;
    if u_axis.dot(v_axis).abs() > EPS_SKETCH_PROJECT_PLACEMENT_E10
        || u_axis.dot(normal).abs() > EPS_SKETCH_PROJECT_PLACEMENT_E10
        || v_axis.dot(normal).abs() > EPS_SKETCH_PROJECT_PLACEMENT_E10
        || u_axis.cross(v_axis).dot(normal) < 1.0 - EPS_SKETCH_PROJECT_PLACEMENT_E10
        || normal.dot(stored_direction) < 1.0 - EPS_SKETCH_PROJECT_PLACEMENT_E10
    {
        return None;
    }
    SketchPlacement::try_resolved(
        Point3::new(
            matrix[0][3] * 10.0,
            matrix[1][3] * 10.0,
            matrix[2][3] * 10.0,
        ),
        normal,
        u_axis,
    )
    .ok()
}

fn build_profiles(entities: &[&SketchEntity]) -> Vec<Vec<SketchEntityUse>> {
    let source_positions = entities
        .iter()
        .enumerate()
        .map(|(index, entity)| (entity.id().as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut profiles = entities
        .iter()
        .filter(|entity| !entity.construction)
        .filter(|entity| {
            matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Circle { .. } | SketchGeometryDefinition::Ellipse { .. }
            )
        })
        .map(|entity| {
            vec![SketchEntityUse {
                entity: entity.id().clone(),
                reversed: false,
            }]
        })
        .collect::<Vec<_>>();
    let lines = entities
        .iter()
        .copied()
        .filter(|entity| !entity.construction)
        .filter(|entity| {
            matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            )
        })
        .filter(|entity| entity.endpoint_refs.len() == 2)
        .collect::<Vec<_>>();
    let mut adjacency = HashMap::<&str, Vec<usize>>::new();
    for (index, line) in lines.iter().enumerate() {
        adjacency
            .entry(line.endpoint_refs[0].as_str())
            .or_default()
            .push(index);
        adjacency
            .entry(line.endpoint_refs[1].as_str())
            .or_default()
            .push(index);
    }
    let mut visited = HashSet::new();
    for start_index in 0..lines.len() {
        if visited.contains(&start_index) {
            continue;
        }
        let component = line_component(start_index, &lines, &adjacency);
        if component.iter().any(|index| {
            lines[*index]
                .endpoint_refs
                .iter()
                .any(|point| adjacency.get(point.as_str()).map_or(0, Vec::len) != 2)
        }) {
            visited.extend(component);
            continue;
        }
        let first = lines[start_index];
        let start_point = first.endpoint_refs[0].as_str();
        let mut point = first.endpoint_refs[1].as_str();
        let mut current = start_index;
        let mut loop_uses = vec![SketchEntityUse {
            entity: first.id().clone(),
            reversed: false,
        }];
        visited.insert(current);
        while point != start_point {
            let Some(next) = adjacency
                .get(point)
                .and_then(|indices| indices.iter().copied().find(|index| *index != current))
            else {
                loop_uses.clear();
                break;
            };
            if visited.contains(&next) {
                loop_uses.clear();
                break;
            }
            let line = lines[next];
            let reversed = line.endpoint_refs[1] == point;
            point = if reversed {
                line.endpoint_refs[0].as_str()
            } else {
                line.endpoint_refs[1].as_str()
            };
            current = next;
            visited.insert(next);
            loop_uses.push(SketchEntityUse {
                entity: line.id().clone(),
                reversed,
            });
        }
        visited.extend(component);
        if loop_uses.len() >= 3 && point == start_point {
            profiles.push(loop_uses);
        }
    }
    profiles.sort_by_key(|profile| {
        profile
            .iter()
            .filter_map(|entity| source_positions.get(entity.entity.as_str()))
            .copied()
            .min()
            .unwrap_or(usize::MAX)
    });
    profiles
}

fn line_component(
    start: usize,
    lines: &[&SketchEntity],
    adjacency: &HashMap<&str, Vec<usize>>,
) -> HashSet<usize> {
    let mut component = HashSet::new();
    let mut pending = vec![start];
    while let Some(index) = pending.pop() {
        if !component.insert(index) {
            continue;
        }
        for point in &lines[index].endpoint_refs {
            if let Some(neighbours) = adjacency.get(point.as_str()) {
                pending.extend(neighbours);
            }
        }
    }
    component
}

fn sketch_id(sketch: &PmDcSketch) -> Option<SketchId> {
    SketchId::mint(format!(
        "inventor:design:sketch#{}-{}",
        sketch.identity.segment_token, sketch.identity.record_ordinal
    ))
    .ok()
}

fn entity_id(entity: &PmDcSketchEntity) -> Option<SketchEntityId> {
    SketchEntityId::mint(format!(
        "inventor:design:sketch-entity#{}-{}",
        entity.identity.segment_token, entity.identity.record_ordinal
    ))
    .ok()
}

pub(crate) type PmDcSketch = Located<PmDcSketchPayload>;

impl RecordPayload for PmDcSketchPayload {
    const KIND: &'static str = "sketch";
}

pub(crate) type PmDcSketchEntity = Located<PmDcSketchEntityPayload>;

impl RecordPayload for PmDcSketchEntityPayload {
    const KIND: &'static str = "sketch-entity";
}

pub(crate) type PmDcTransform = Located<PmDcTransformPayload>;

impl RecordPayload for PmDcTransformPayload {
    const KIND: &'static str = "transform";
}

pub(crate) type PmDcDirection = Located<PmDcDirectionPayload>;

impl RecordPayload for PmDcDirectionPayload {
    const KIND: &'static str = "direction";
}

pub(crate) type PmDcSketchConstraint = Located<PmDcSketchConstraintPayload>;

impl RecordPayload for PmDcSketchConstraintPayload {
    const KIND: &'static str = "sketch-constraint";
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

    fn content(index: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&index.to_le_bytes()[..2]);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0x0002_0200u32.to_le_bytes());
        bytes.extend_from_slice(&0x8000_0003u32.to_le_bytes());
        bytes.extend_from_slice(&index.to_le_bytes());
        bytes
    }

    fn list(marker: u16, references: &[u32]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&marker.to_le_bytes());
        bytes.extend_from_slice(&0x3000u16.to_le_bytes());
        bytes.extend_from_slice(&(references.len() as u32).to_le_bytes());
        if !references.is_empty() {
            if marker == 8 {
                bytes.extend_from_slice(&0u16.to_le_bytes());
                bytes.extend_from_slice(&0u16.to_le_bytes());
            } else {
                bytes.extend_from_slice(&0u32.to_le_bytes());
                bytes.extend_from_slice(&0u32.to_le_bytes());
            }
            for reference in references {
                bytes.extend_from_slice(&reference.to_le_bytes());
            }
        }
        bytes
    }

    fn parse<T>(bytes: &[u8], parser: impl FnOnce(&DecodeContext<'_>, View<'_>) -> T) -> T {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, source) = DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("view");
        parser(&ctx, source)
    }

    fn entity_prefix(index: u32, sketch: u32, flags: u32) -> Vec<u8> {
        let mut bytes = content(index);
        bytes.extend_from_slice(&flags.to_le_bytes());
        bytes.extend_from_slice(&sketch.to_le_bytes());
        bytes
    }

    fn point_bytes(index: u32, sketch: u32, position: [f64; 2]) -> Vec<u8> {
        let mut bytes = entity_prefix(index, sketch, 0);
        for value in position {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend(list(2, &[]));
        bytes.extend(list(2, &[]));
        bytes
    }

    fn line_bytes(index: u32, sketch: u32, points: [u32; 2]) -> Vec<u8> {
        let mut bytes = entity_prefix(index, sketch, 0);
        bytes.extend(list(2, &points));
        bytes.extend(list(2, &[]));
        bytes.extend_from_slice(&0.0f64.to_le_bytes());
        bytes.extend_from_slice(&0.0f64.to_le_bytes());
        bytes.extend_from_slice(&1.0f64.to_le_bytes());
        bytes.extend_from_slice(&0.0f64.to_le_bytes());
        bytes
    }

    fn constraint_header(index: u32, parameter: u32) -> Vec<u8> {
        let mut bytes = content(index);
        bytes.extend_from_slice(&(-1i32).to_le_bytes());
        bytes.extend_from_slice(&0x8000_000cu32.to_le_bytes());
        bytes.extend(list(6, &[]));
        bytes.extend(list(6, &[]));
        bytes.extend_from_slice(&parameter.to_le_bytes());
        bytes
    }

    fn legacy_constraint_header(index: u32, parameter: u32) -> Vec<u8> {
        let mut bytes = content(index);
        bytes.extend_from_slice(&(-1i32).to_le_bytes());
        bytes.extend_from_slice(&0x8000_000cu32.to_le_bytes());
        bytes.extend_from_slice(&parameter.to_le_bytes());
        bytes
    }

    #[test]
    fn parses_generated_planar_geometry_branches() {
        let point = point_bytes(1, 3, [1.25, -2.5]);
        let parsed = parse(&point, |ctx, source| {
            parse_entity(ctx, SketchEntityTag::Point, source, 22).expect("point")
        });
        assert!(matches!(
            parsed.kind,
            PmDcSketchEntityKind::Point {
                position: [1.25, -2.5],
                ..
            }
        ));

        let line = line_bytes(2, 3, [4, 5]);
        let parsed = parse(&line, |ctx, source| {
            parse_entity(ctx, SketchEntityTag::Line, source, 22).expect("line")
        });
        assert!(matches!(
            parsed.kind,
            PmDcSketchEntityKind::Line { ref points, .. }
                if points.references().iter().map(|value| value.index).collect::<Vec<_>>() == [4, 5]
        ));

        let mut circle = entity_prefix(3, 3, 0);
        circle.extend(list(2, &[]));
        circle.extend(list(2, &[]));
        circle.extend_from_slice(&4u32.to_le_bytes());
        circle.extend_from_slice(&2.5f64.to_le_bytes());
        circle.push(1);
        let parsed = parse(&circle, |ctx, source| {
            parse_entity(ctx, SketchEntityTag::Circle, source, 22).expect("circle")
        });
        assert!(matches!(
            parsed.kind,
            PmDcSketchEntityKind::Circle { radius: 2.5, .. }
        ));

        let mut ellipse = entity_prefix(4, 3, 0);
        ellipse.extend(list(2, &[]));
        ellipse.extend(list(2, &[]));
        ellipse.extend_from_slice(&4u32.to_le_bytes());
        ellipse.extend_from_slice(&1.0f64.to_le_bytes());
        ellipse.extend_from_slice(&0.0f64.to_le_bytes());
        ellipse.extend_from_slice(&3.0f64.to_le_bytes());
        ellipse.extend_from_slice(&2.0f64.to_le_bytes());
        ellipse.push(0);
        let parsed = parse(&ellipse, |ctx, source| {
            parse_entity(ctx, SketchEntityTag::Ellipse, source, 22).expect("ellipse")
        });
        assert!(matches!(
            parsed.kind,
            PmDcSketchEntityKind::Ellipse {
                major_radius: 3.0,
                minor_radius: 2.0,
                ..
            }
        ));
    }

    #[test]
    fn parses_generated_constraint_branches() {
        for (type_id, tail, expected) in [
            (SketchConstraintTag::Coincident, vec![4, 5], "coincident"),
            (SketchConstraintTag::Parallel, vec![4, 5, 0], "parallel"),
            (
                SketchConstraintTag::Perpendicular,
                vec![4, 5, 0],
                "perpendicular",
            ),
            (SketchConstraintTag::Tangent, vec![4, 5, 0], "tangent"),
        ] {
            let mut bytes = constraint_header(9, 0);
            for (index, value) in tail.into_iter().enumerate() {
                if index == 2 && matches!(expected, "parallel" | "perpendicular") {
                    bytes.extend_from_slice(&(value as u16).to_le_bytes());
                } else {
                    bytes.extend_from_slice(&(value as u32).to_le_bytes());
                }
            }
            let parsed = parse(&bytes, |ctx, source| {
                parse_constraint(ctx, type_id, source, 22).expect(expected)
            });
            assert_eq!(
                match parsed.kind {
                    PmDcSketchConstraintKind::Coincident { .. } => "coincident",
                    PmDcSketchConstraintKind::Parallel { .. } => "parallel",
                    PmDcSketchConstraintKind::Perpendicular { .. } => "perpendicular",
                    PmDcSketchConstraintKind::Tangent { .. } => "tangent",
                    _ => "unexpected",
                },
                expected
            );
        }

        for (type_id, expected) in [
            (SketchConstraintTag::Horizontal, true),
            (SketchConstraintTag::Vertical, false),
        ] {
            let mut bytes = constraint_header(10, 0);
            bytes.extend_from_slice(&4u32.to_le_bytes());
            bytes.push(1);
            let parsed = parse(&bytes, |ctx, source| {
                parse_constraint(ctx, type_id, source, 22).expect("axis constraint")
            });
            assert_eq!(
                matches!(parsed.kind, PmDcSketchConstraintKind::Horizontal { .. }),
                expected
            );
        }
    }

    #[test]
    fn parses_generated_legacy_constraint_header_without_maps() {
        let mut bytes = legacy_constraint_header(9, 0);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&5u32.to_le_bytes());
        let parsed = parse(&bytes, |ctx, source| {
            parse_constraint(ctx, SketchConstraintTag::Coincident, source, 16)
                .expect("legacy coincident")
        });
        assert!(parsed.header.scalar_map.entries().is_empty());
        assert!(parsed.header.reference_map.entries().is_empty());
        assert!(matches!(
            parsed.kind,
            PmDcSketchConstraintKind::Coincident { first, second }
                if first.index == 4 && second.index == 5
        ));
    }

    #[test]
    fn parses_generated_dimensional_constraint_branches() {
        for type_id in [
            SketchConstraintTag::HorizontalDistance,
            SketchConstraintTag::VerticalDistance,
        ] {
            let mut bytes = constraint_header(11, 0);
            bytes.extend_from_slice(&4u32.to_le_bytes());
            bytes.extend_from_slice(&5u32.to_le_bytes());
            bytes.extend_from_slice(&12u32.to_le_bytes());
            bytes.extend_from_slice(&[0; 16]);
            parse(&bytes, |ctx, source| {
                parse_constraint(ctx, type_id, source, 22).expect("distance constraint")
            });
        }
        let mut radius = constraint_header(12, 13);
        radius.extend_from_slice(&0u32.to_le_bytes());
        radius.extend_from_slice(&4u32.to_le_bytes());
        radius.extend_from_slice(&[0; 16]);
        parse(&radius, |ctx, source| {
            parse_constraint(ctx, SketchConstraintTag::Radius, source, 22).expect("radius")
        });

        let mut diameter = constraint_header(13, 14);
        diameter.extend_from_slice(&0u32.to_le_bytes());
        diameter.extend_from_slice(&4u32.to_le_bytes());
        diameter.extend_from_slice(&[0; 16]);
        parse(&diameter, |ctx, source| {
            parse_constraint(ctx, SketchConstraintTag::Diameter, source, 22).expect("diameter")
        });

        for type_id in [
            SketchConstraintTag::CircleCenter,
            SketchConstraintTag::EqualRadius,
        ] {
            let mut bytes = constraint_header(14, 0);
            bytes.extend_from_slice(&4u32.to_le_bytes());
            bytes.extend_from_slice(&5u32.to_le_bytes());
            parse(&bytes, |ctx, source| {
                parse_constraint(ctx, type_id, source, 22).expect("circle relation")
            });
        }
    }

    #[test]
    fn projects_generated_closed_square_and_resolved_plane() {
        let mut transform = parse(
            &{
                let mut bytes = content(0);
                bytes.extend_from_slice(&0x8421u16.to_le_bytes());
                bytes.extend_from_slice(&0x7bdeu16.to_le_bytes());
                bytes
            },
            |_, source| parse_transform(source, 22).expect("transform"),
        );
        let direction = parse(
            &{
                let mut bytes = content(1);
                bytes.extend_from_slice(&0u32.to_le_bytes());
                bytes.extend_from_slice(&0.0f64.to_le_bytes());
                bytes.extend_from_slice(&0.0f64.to_le_bytes());
                bytes.extend_from_slice(&0.0f64.to_le_bytes());
                bytes.extend_from_slice(&1.0f64.to_le_bytes());
                bytes
            },
            |_, source| parse_direction(source, 22).expect("direction"),
        );
        let mut sketch_bytes = content(2);
        sketch_bytes.extend_from_slice(&0i32.to_le_bytes());
        sketch_bytes.extend_from_slice(&8u32.to_le_bytes());
        sketch_bytes.extend(list(8, &[4, 5, 6, 7, 8, 9, 10, 11]));
        sketch_bytes.extend_from_slice(&1u32.to_le_bytes());
        sketch_bytes.extend_from_slice(&2u32.to_le_bytes());
        sketch_bytes.extend_from_slice(&[0; 8]);
        sketch_bytes.extend(list(2, &[]));
        let sketch = parse(&sketch_bytes, |ctx, source| {
            parse_sketch(ctx, source, 22).expect("sketch")
        });
        let points = [[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]];
        let mut entities = points
            .into_iter()
            .enumerate()
            .map(|(index, position)| {
                parse(
                    &point_bytes(index as u32 + 3, 3, position),
                    |ctx, source| {
                        parse_entity(ctx, SketchEntityTag::Point, source, 22).expect("point")
                    },
                )
            })
            .collect::<Vec<_>>();
        for (index, endpoints) in [[4, 5], [5, 6], [6, 7], [7, 4]].into_iter().enumerate() {
            let mut line = parse(
                &line_bytes(index as u32 + 7, 3, endpoints),
                |ctx, source| parse_entity(ctx, SketchEntityTag::Line, source, 22).expect("line"),
            );
            let start = points[endpoints[0] as usize - 4];
            let end = points[endpoints[1] as usize - 4];
            let PmDcSketchEntityKind::Line {
                origin, direction, ..
            } = &mut line.kind
            else {
                unreachable!("generated line")
            };
            *origin = start;
            *direction = [end[0] - start[0], end[1] - start[1]];
            entities.push(line);
        }
        transform.header.source_index = 0;
        let transform = Located::new(transform, type_id_string(TRANSFORM_TYPE), "segment", 0);
        let direction = Located::new(direction, type_id_string(DIRECTION_TYPE), "segment", 1);
        let located_sketch =
            Located::new(sketch.clone(), type_id_string(SKETCH_TYPE), "segment", 2);
        let entities = entities
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let type_id = if index < 4 { POINT_TYPE } else { LINE_TYPE };
                Located::new(value, type_id_string(type_id), "segment", index as u32 + 3)
            })
            .collect();
        let mut inventory = SketchInventory {
            sketches: vec![located_sketch],
            entities,
            transforms: vec![transform],
            directions: vec![direction],
            constraints: Vec::new(),
            issues: Vec::new(),
        };
        let projected = project(&inventory, &[]);
        assert_eq!(projected.unresolved_sketches, 0);
        assert_eq!(projected.unresolved_entities, 0);
        assert_eq!(projected.sketches[0].profiles[0].len(), 4);
        assert!(matches!(
            projected.sketches[0].placement,
            SketchPlacement::Resolved { .. }
        ));

        let mut mapped_constraint = content(11);
        mapped_constraint.extend_from_slice(&(-1i32).to_le_bytes());
        mapped_constraint.extend_from_slice(&0u32.to_le_bytes());
        mapped_constraint.extend_from_slice(&6u16.to_le_bytes());
        mapped_constraint.extend_from_slice(&0x3000u16.to_le_bytes());
        mapped_constraint.extend_from_slice(&1u32.to_le_bytes());
        mapped_constraint.extend_from_slice(&[0; 8]);
        mapped_constraint.extend_from_slice(&4u32.to_le_bytes());
        mapped_constraint.extend_from_slice(&0.5f64.to_le_bytes());
        mapped_constraint.extend(list(6, &[]));
        mapped_constraint.extend_from_slice(&0u32.to_le_bytes());
        mapped_constraint.extend_from_slice(&4u32.to_le_bytes());
        mapped_constraint.extend_from_slice(&5u32.to_le_bytes());
        let mapped_constraint = parse(&mapped_constraint, |ctx, source| {
            parse_constraint(ctx, SketchConstraintTag::Coincident, source, 22)
                .expect("mapped coincident")
        });
        inventory.constraints.push(Located::new(
            mapped_constraint,
            type_id_string(COINCIDENT_TYPE),
            "segment",
            11,
        ));
        let mut sketch = sketch;
        let (marker, metadata, mut references) = sketch.entities.clone().into_parts();
        references.push(PmDcReference {
            index: 12,
            qualified: false,
        });
        sketch.entities =
            PmDcReferenceList::new(marker, metadata, references).expect("extended entity list");
        inventory.sketches[0] = Located::new(sketch, type_id_string(SKETCH_TYPE), "segment", 2);
        let incomplete = project(&inventory, &[]);
        assert_eq!(incomplete.unresolved_sketches, 1);
        assert_eq!(incomplete.unresolved_constraints, 1);
        assert!(incomplete.sketches.is_empty());
        assert!(incomplete.entities.is_empty());
    }
    #[test]
    fn point_tail_wire_requires_both_fields() {
        let list = serde_json::json!({"marker": 2, "metadata": null, "references": []});
        let mut wire = serde_json::json!({
            "form": "point", "position": [0.0, 0.0],
            "endpoint_of": list.clone(), "center_of": list.clone(),
            "state": null, "associations": null
        });
        let point: PmDcSketchEntityKind =
            serde_json::from_value(wire.clone()).expect("paired point fixture round-trips");
        assert_eq!(
            serde_json::to_value(point).expect("paired point fixture round-trips"),
            wire
        );
        wire["state"] = serde_json::json!(0);
        assert!(serde_json::from_value::<PmDcSketchEntityKind>(wire.clone()).is_err());
        wire["associations"] = list;
        let point: PmDcSketchEntityKind =
            serde_json::from_value(wire.clone()).expect("paired point fixture round-trips");
        assert_eq!(
            serde_json::to_value(point).expect("paired point fixture round-trips"),
            wire
        );
        wire["state"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<PmDcSketchEntityKind>(wire).is_err());
    }
    #[test]
    fn transform_prefix_wire_is_constant_or_absent() {
        let mut bytes = content(1);
        bytes.extend_from_slice(&0x8421u16.to_le_bytes());
        bytes.extend_from_slice(&0x7bdeu16.to_le_bytes());
        let transform = parse(&bytes, |_, source| {
            parse_transform(source, 22).expect("constant-prefix transform fixture is valid")
        });
        let mut wire =
            serde_json::to_value(transform).expect("constant-prefix transform fixture is valid");
        for (prefix, present) in [
            (serde_json::Value::Null, false),
            (serde_json::json!(515), true),
        ] {
            wire["prefix"] = prefix;
            let parsed: PmDcTransformPayload = serde_json::from_value(wire.clone())
                .expect("constant-prefix transform fixture is valid");
            assert_eq!(parsed.prefix_present, present);
            assert_eq!(
                serde_json::to_value(parsed).expect("constant-prefix transform fixture is valid"),
                wire
            );
        }
        wire["prefix"] = serde_json::json!(516);
        assert!(serde_json::from_value::<PmDcTransformPayload>(wire.clone()).is_err());
        wire.as_object_mut()
            .expect("constant-prefix transform fixture is valid")
            .remove("prefix");
        assert!(
            !serde_json::from_value::<PmDcTransformPayload>(wire)
                .expect("constant-prefix transform fixture is valid")
                .prefix_present
        );
    }
}
