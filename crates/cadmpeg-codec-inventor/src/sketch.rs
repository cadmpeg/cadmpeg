// SPDX-License-Identifier: Apache-2.0
//! Typed planar-sketch records and closed neutral sketch graphs.

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
    scalar::{Angle, FiniteReal, Length, PositiveReal},
};
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};

use crate::compact_matrix::{CompactMatrix, CompactMatrixWire};
use crate::pmdc::{
    content_header, inventor_id, reference_list, Cursor, PmDcContentHeader, PmDcReference,
    PmDcReferenceList,
};
use crate::record_identity::{push_record, Located, RecordPayload};
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
    save_version_major: u8,
    pub(crate) header: PmDcConstraintHeader,
    pub(crate) kind: PmDcSketchConstraintKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcConstraintHeader {
    pub(crate) content: PmDcContentHeader,
    state: i32,
    pub(crate) group: PmDcReference,
    pub(crate) scalar_map: PmDcReferenceScalarMap,
    pub(crate) reference_map: PmDcReferencePairMap,
    pub(crate) parameter: PmDcReference,
}

type PmDcReferenceScalarMap = crate::pmdc::PmDcPairedMap<FiniteReal>;
type PmDcReferencePairMap = crate::pmdc::PmDcPairedMap<PmDcReference>;

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
    save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    entity_flags: u32,
    pub(crate) sketch: PmDcReference,
    pub(crate) kind: PmDcSketchEntityKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum PmDcSketchEntityKind {
    Point {
        position: [FiniteReal; 2],
        endpoint_of: PmDcReferenceList,
        center_of: PmDcReferenceList,
        #[serde(flatten)]
        tail: PointTail,
    },
    Line {
        points: PmDcReferenceList,
        auxiliary: Vec<PmDcReferenceList>,
        origin: [FiniteReal; 2],
        direction: [FiniteReal; 2],
    },
    Circle {
        points: PmDcReferenceList,
        auxiliary: Vec<PmDcReferenceList>,
        center: PmDcReference,
        radius: PositiveReal,
        state: u8,
    },
    Ellipse {
        points: PmDcReferenceList,
        auxiliary: Vec<PmDcReferenceList>,
        center: PmDcReference,
        major_direction: [FiniteReal; 2],
        major_radius: PositiveReal,
        minor_radius: PositiveReal,
        state: u8,
    },
}

/// The absent or complete state and association tail of a sketch point.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "PointTailWire")]
pub(crate) enum PointTail {
    Absent,
    Present {
        state: u32,
        associations: PmDcReferenceList,
    },
}

#[derive(Serialize)]
struct PointTailRef<'a> {
    state: Option<u32>,
    associations: Option<&'a PmDcReferenceList>,
}

impl Serialize for PointTail {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let wire = match self {
            Self::Absent => PointTailRef {
                state: None,
                associations: None,
            },
            Self::Present {
                state,
                associations,
            } => PointTailRef {
                state: Some(*state),
                associations: Some(associations),
            },
        };
        wire.serialize(serializer)
    }
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PmDcTransformPayload {
    pub(crate) save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    pub(crate) prefix_present: bool,
    pub(crate) matrix: CompactMatrix,
}

impl Serialize for PmDcTransformPayload {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(6))?;
        let (value_mask, zero_mask) = self.matrix.masks();
        map.serialize_entry("save_version_major", &self.save_version_major)?;
        map.serialize_entry("header", &self.header)?;
        map.serialize_entry("prefix", &self.prefix_present.then_some(TRANSFORM_PREFIX))?;
        map.serialize_entry("value_mask", &value_mask)?;
        map.serialize_entry("zero_mask", &zero_mask)?;
        map.serialize_entry("matrix", &self.matrix.rows())?;
        map.end()
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct PmDcTransformPayloadWire {
    save_version_major: u8,
    header: PmDcContentHeader,
    prefix: Option<u32>,
    #[serde(flatten)]
    matrix: CompactMatrixWire,
}

impl PmDcTransformPayloadWire {
    pub(crate) fn into_payload(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<PmDcTransformPayload, CodecError> {
        let prefix_present = match self.prefix {
            None => false,
            Some(TRANSFORM_PREFIX) => true,
            Some(_) => {
                return Err(CodecError::Malformed(ctx.copy_retained_text(
                    "transform prefix must be 515 or null",
                    "retain invalid Inventor PmDc transform prefix",
                )?));
            }
        };
        Ok(PmDcTransformPayload {
            save_version_major: self.save_version_major,
            header: self.header,
            prefix_present,
            matrix: self.matrix.into_matrix(ctx)?,
        })
    }
}

impl From<PmDcTransformPayload> for PmDcTransformPayloadWire {
    fn from(value: PmDcTransformPayload) -> Self {
        Self {
            save_version_major: value.save_version_major,
            header: value.header,
            prefix: value.prefix_present.then_some(TRANSFORM_PREFIX),
            matrix: value.matrix.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcDirectionPayload {
    pub(crate) save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    pub(crate) entity_flags: u32,
    pub(crate) parameter: FiniteReal,
    pub(crate) extension: Option<u32>,
    pub(crate) direction: [FiniteReal; 3],
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
    for segment in ctx.admit_iter(&document.segments, "visit Inventor sketch items")? {
        if !ctx.equal(
            &segment.kind,
            &SegmentKind::PmDc,
            "select Inventor PmDc sketch segment",
        )? {
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
        for record in ctx.admit_iter(&table.records, "visit Inventor sketch items")? {
            let Some(tag) = SketchRecordTag::from_type_id(record.type_id) else {
                continue;
            };
            let result = match tag {
                SketchRecordTag::Sketch => {
                    parse_sketch(ctx, record.payload, version).and_then(|value| {
                        push_record(
                            ctx,
                            &mut inventory.sketches,
                            value,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc sketch record",
                        )
                    })
                }
                SketchRecordTag::Entity(entity) => {
                    parse_entity(ctx, entity, record.payload, version).and_then(|value| {
                        push_record(
                            ctx,
                            &mut inventory.entities,
                            value,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc sketch entity record",
                        )
                    })
                }
                SketchRecordTag::Transform => parse_transform(ctx, record.payload, version)
                    .and_then(|value| {
                        push_record(
                            ctx,
                            &mut inventory.transforms,
                            value,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc transform record",
                        )
                    }),
                SketchRecordTag::Direction => parse_direction(ctx, record.payload, version)
                    .and_then(|value| {
                        push_record(
                            ctx,
                            &mut inventory.directions,
                            value,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc direction record",
                        )
                    }),
                SketchRecordTag::Constraint(constraint) => {
                    parse_constraint(ctx, constraint, record.payload, version).and_then(|value| {
                        push_record(
                            ctx,
                            &mut inventory.constraints,
                            value,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc sketch constraint record",
                        )
                    })
                }
            };
            if let Err(error) = result {
                if matches!(error, CodecError::ResourceLimit(_)) {
                    return Err(error);
                }
                ctx.charge_entities(1, "admit Inventor PmDc sketch issue")?;
                ctx.push_vec(
                    &mut inventory.issues,
                    RecordIssue {
                        family: RecordIssueFamily::Sketch {
                            type_id: crate::record_identity::RecordTypeId::from_bytes(
                                ctx,
                                record.type_id,
                                "retain Inventor PmDc sketch issue type id",
                            )?,
                        },
                        segment_token: segment.pair.token.key().try_clone_for_decode(
                            ctx,
                            "retain Inventor PmDc sketch issue segment token",
                        )?,
                        record_ordinal: record.ordinal,
                        detail: crate::issue_detail(
                            ctx,
                            error,
                            "retain Inventor PmDc sketch issue detail",
                        )?,
                    },
                    "admit Inventor PmDc sketch issue",
                )?;
            }
        }
    }
    Ok(inventory)
}

fn parse_sketch(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcSketchPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let state = cursor.u32("sketch state")?.cast_signed();
    let count_value = cursor.u32("sketch count value")?;
    let entities = reference_list(ctx, &mut cursor, 8, "sketch entity array")?;
    let transform = cursor.reference("sketch transform reference")?;
    let direction = cursor.reference("sketch direction reference")?;
    let values = [cursor.u32("sketch value 0")?, cursor.u32("sketch value 1")?];
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
    let entity_flags = cursor.u32("sketch entity flags")?;
    let sketch = cursor.reference("sketch entity sketch reference")?;
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

fn point2(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    field: &str,
) -> Result<[FiniteReal; 2], CodecError> {
    Ok([
        {
            let (field, _storage) = ctx.format_scoped(
                format_args!("{field} u"),
                "format Inventor sketch coordinate field",
            )?;
            cursor.f64(&field)?
        },
        {
            let (field, _storage) = ctx.format_scoped(
                format_args!("{field} v"),
                "format Inventor sketch coordinate field",
            )?;
            cursor.f64(&field)?
        },
    ])
}

fn point3(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    field: &str,
) -> Result<[FiniteReal; 3], CodecError> {
    Ok([
        {
            let (field, _storage) = ctx.format_scoped(
                format_args!("{field} x"),
                "format Inventor sketch coordinate field",
            )?;
            cursor.f64(&field)?
        },
        {
            let (field, _storage) = ctx.format_scoped(
                format_args!("{field} y"),
                "format Inventor sketch coordinate field",
            )?;
            cursor.f64(&field)?
        },
        {
            let (field, _storage) = ctx.format_scoped(
                format_args!("{field} z"),
                "format Inventor sketch coordinate field",
            )?;
            cursor.f64(&field)?
        },
    ])
}

fn parse_point(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<PmDcSketchEntityKind, CodecError> {
    let position = point2(ctx, cursor, "sketch point")?;
    let endpoint_of = reference_list(ctx, cursor, 2, "point endpoint-of list")?;
    let center_of = reference_list(ctx, cursor, 2, "point center-of list")?;
    let tail = if cursor.remaining() == 0 {
        PointTail::Absent
    } else {
        PointTail::Present {
            state: cursor.u32("point tail state")?,
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
    let points = {
        let (name, _storage) = ctx.format_scoped(
            format_args!("{field} point list"),
            "format Inventor edge point field",
        )?;
        reference_list(ctx, cursor, 2, &name)?
    };
    let mut auxiliary = Vec::new();
    let tail8 = fixed_tail
        .checked_add(8)
        .ok_or_else(|| CodecError::malformed("Inventor edge tail size overflow"))?;
    let tail16 = fixed_tail
        .checked_add(16)
        .ok_or_else(|| CodecError::malformed("Inventor edge tail size overflow"))?;
    if cursor.remaining() >= tail8 && cursor.peek_u32("edge auxiliary-list marker")? == 0x3000_0002
    {
        {
            let (name, _storage) = ctx.format_scoped(
                format_args!("{field} auxiliary list 0"),
                "format Inventor edge auxiliary field",
            )?;
            let list = reference_list(ctx, cursor, 2, &name)?;
            ctx.push_vec(&mut auxiliary, list, "collect Inventor sketch items")?;
        }
    } else if cursor.remaining() >= tail16 {
        let gate = [
            cursor.u32("edge list gate 0")?,
            cursor.u32("edge list gate 1")?,
        ];
        if gate != [1, 0] {
            return Err(CodecError::malformed(format_args!(
                "Inventor PmDc {field} list gate is {gate:?}"
            )));
        }
        {
            let (name, _storage) = ctx.format_scoped(
                format_args!("{field} auxiliary list 0"),
                "format Inventor edge auxiliary field",
            )?;
            let list = reference_list(ctx, cursor, 2, &name)?;
            ctx.push_vec(&mut auxiliary, list, "collect Inventor sketch items")?;
        }
        if cursor.remaining() >= tail8
            && cursor.peek_u32("edge auxiliary-list marker")? == 0x3000_0002
        {
            {
                let (name, _storage) = ctx.format_scoped(
                    format_args!("{field} auxiliary list 1"),
                    "format Inventor edge auxiliary field",
                )?;
                let list = reference_list(ctx, cursor, 2, &name)?;
                ctx.push_vec(&mut auxiliary, list, "collect Inventor sketch items")?;
            }
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
    let origin = point2(ctx, cursor, "line origin")?;
    let direction = point2(ctx, cursor, "line direction")?;
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
    let center = cursor.reference("circle center reference")?;
    let radius = cursor.f64("circle radius")?;
    let state = cursor.u8("circle state")?;
    let radius = PositiveReal::new(radius.get()).ok_or_else(|| {
        CodecError::Malformed("Inventor PmDc circle radius is not positive".into())
    })?;
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
    let center = cursor.reference("ellipse center reference")?;
    let major_direction = point2(ctx, cursor, "ellipse major direction")?;
    let major_radius = cursor.f64("ellipse major radius")?;
    let minor_radius = cursor.f64("ellipse minor radius")?;
    let state = cursor.u8("ellipse state")?;
    let major_radius = PositiveReal::new(major_radius.get()).ok_or_else(|| {
        CodecError::Malformed("Inventor PmDc ellipse radius is not positive".into())
    })?;
    let minor_radius = PositiveReal::new(minor_radius.get()).ok_or_else(|| {
        CodecError::Malformed("Inventor PmDc ellipse radius is not positive".into())
    })?;
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

fn parse_transform(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcTransformPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let prefix_present = cursor.peek_u32("transform prefix")? == TRANSFORM_PREFIX;
    if prefix_present {
        cursor.u32("transform prefix")?;
    }
    let value_mask = cursor.u16("transform value mask")?;
    let zero_mask = cursor.u16("transform zero mask")?;
    let matrix = CompactMatrix::try_new(ctx, value_mask, zero_mask, |_| {
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

fn parse_direction(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    version: u8,
) -> Result<PmDcDirectionPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let entity_flags = cursor.u32("direction entity flags")?;
    let parameter = cursor.f64("direction parameter")?;
    let extension = match cursor.remaining() {
        24 => None,
        28 => Some(cursor.u32("direction extension value")?),
        remaining => {
            return Err(CodecError::malformed(format_args!(
                "Inventor PmDc direction has {remaining} bytes before its vector"
            )));
        }
    };
    let direction = point3(ctx, &mut cursor, "direction vector")?;
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
    let marker = [
        cursor.u16("constraint map marker 0")?,
        cursor.u16("constraint map marker 1")?,
    ];
    if marker != [6, 0x3000] {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc {field} marker is {marker:?}"
        )));
    }
    let count = usize::try_from(cursor.u32("constraint map count")?)
        .map_err(|_| CodecError::Malformed("Inventor numeric value exceeds target range".into()))?;
    ctx.charge_collection_items(
        cadmpeg_core::decode::u64_from_index(count),
        "admit Inventor sketch constraint map",
    )?;
    let metadata = (count != 0)
        .then(|| {
            Ok::<_, CodecError>([
                cursor.u32("constraint map metadata 0")?,
                cursor.u32("constraint map metadata 1")?,
            ])
        })
        .transpose()?;
    Ok((count, metadata))
}

fn reference_scalar_map(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<PmDcReferenceScalarMap, CodecError> {
    let (count, metadata) = map_header(ctx, cursor, "constraint scalar map")?;
    let mut entries = ctx.vector_storage(count, "admit Inventor sketch constraint map")?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(count),
        "read Inventor sketch constraint scalar map",
    )?;
    for index in 0..count {
        let (field, _storage) = ctx.format_scoped(
            format_args!("constraint scalar-map value {index}"),
            "format Inventor constraint scalar-map field",
        )?;
        entries.push((
            cursor.reference("constraint scalar-map key")?,
            cursor.f64(&field)?,
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
    let mut entries = ctx.vector_storage(count, "admit Inventor sketch constraint map")?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(count),
        "read Inventor sketch constraint reference map",
    )?;
    for _ in 0..count {
        entries.push((
            cursor.reference("constraint reference-map key")?,
            cursor.reference("constraint reference-map value")?,
        ));
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
    let state = cursor.u32("constraint state")?.cast_signed();
    let group = cursor.reference("constraint group reference")?;
    let (scalar_map, reference_map) = if version <= 16 {
        (
            PmDcReferenceScalarMap::empty(),
            PmDcReferencePairMap::empty(),
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
        parameter: cursor.reference("constraint parameter reference")?,
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
            first: cursor.reference("coincident constraint first reference")?,
            second: cursor.reference("coincident constraint second reference")?,
        },
        SketchConstraintTag::Parallel => PmDcSketchConstraintKind::Parallel {
            first: cursor.reference("parallel constraint first reference")?,
            second: cursor.reference("parallel constraint second reference")?,
            orientation: cursor.u16("parallel constraint orientation")?,
        },
        SketchConstraintTag::Perpendicular => PmDcSketchConstraintKind::Perpendicular {
            first: cursor.reference("perpendicular constraint first reference")?,
            second: cursor.reference("perpendicular constraint second reference")?,
            orientation: cursor.u16("perpendicular constraint orientation")?,
        },
        SketchConstraintTag::Tangent => PmDcSketchConstraintKind::Tangent {
            first: cursor.reference("tangent constraint first reference")?,
            second: cursor.reference("tangent constraint second reference")?,
            extension: (cursor.remaining() == 4)
                .then(|| cursor.u32("tangent constraint extension"))
                .transpose()?,
        },
        SketchConstraintTag::Horizontal => PmDcSketchConstraintKind::Horizontal {
            entity: cursor.reference("horizontal constraint entity reference")?,
            state: cursor.u8("horizontal constraint state")?,
        },
        SketchConstraintTag::Vertical => PmDcSketchConstraintKind::Vertical {
            entity: cursor.reference("vertical constraint entity reference")?,
            state: cursor.u8("vertical constraint state")?,
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
            state: cursor.u32("radius constraint state")?,
            entity: cursor.reference("radius constraint entity reference")?,
            values: cursor.u32_array::<4>("radius constraint values")?,
        },
        SketchConstraintTag::Diameter => PmDcSketchConstraintKind::Diameter {
            reference: cursor.reference("diameter constraint reference")?,
            entity: cursor.reference("diameter constraint entity reference")?,
            values: cursor.u32_array::<4>("diameter constraint values")?,
        },
        SketchConstraintTag::CircleCenter => PmDcSketchConstraintKind::CircleCenter {
            entity: cursor.reference("circle-center constraint entity reference")?,
            center: cursor.reference("circle-center constraint center reference")?,
        },
        SketchConstraintTag::EqualRadius => PmDcSketchConstraintKind::EqualRadius {
            first: cursor.reference("equal-radius constraint first reference")?,
            second: cursor.reference("equal-radius constraint second reference")?,
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
    let first = cursor.reference("distance constraint first reference")?;
    let second = cursor.reference("distance constraint second reference")?;
    let parameter = cursor.reference("distance constraint parameter reference")?;
    let values = cursor.u32_array::<4>("distance constraint values")?;
    Ok((first, second, parameter, values))
}

fn count_removed(
    ctx: &DecodeContext<'_>,
    unresolved: usize,
    before: usize,
    after: usize,
    operation: &'static str,
) -> Result<usize, CodecError> {
    let removed = before
        .checked_sub(after)
        .ok_or_else(|| CodecError::malformed("Inventor projected count exceeds prior count"))?;
    unresolved
        .checked_add(removed)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))
}

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    inventory: &SketchInventory,
    parameters: &[DesignParameter],
) -> Result<SketchProjection, CodecError> {
    let (raw_sketches, _raw_sketches_storage) = ctx.unique_index(
        ctx.admit_iter(&inventory.sketches, "visit Inventor sketches")?
            .map(|record| {
                (
                    {
                        (
                            record.identity.segment_token.as_str(),
                            record.identity.record_ordinal,
                        )
                    },
                    record,
                )
            }),
        "index Inventor sketches",
    )?;
    let (raw_entities, _raw_entities_storage) = ctx.unique_index(
        ctx.admit_iter(&inventory.entities, "visit Inventor sketch entities")?
            .map(|record| {
                (
                    {
                        (
                            record.identity.segment_token.as_str(),
                            record.identity.record_ordinal,
                        )
                    },
                    record,
                )
            }),
        "index Inventor sketch entities",
    )?;
    let (transforms, _transforms_storage) = ctx.unique_index(
        ctx.admit_iter(&inventory.transforms, "visit Inventor sketch transforms")?
            .map(|record| {
                (
                    {
                        (
                            record.identity.segment_token.as_str(),
                            record.identity.record_ordinal,
                        )
                    },
                    record,
                )
            }),
        "index Inventor sketch transforms",
    )?;
    let (directions, _directions_storage) = ctx.unique_index(
        ctx.admit_iter(&inventory.directions, "visit Inventor sketch directions")?
            .map(|record| {
                (
                    {
                        (
                            record.identity.segment_token.as_str(),
                            record.identity.record_ordinal,
                        )
                    },
                    record,
                )
            }),
        "index Inventor sketch directions",
    )?;
    let (raw_constraints, _raw_constraints_storage) = ctx.unique_index(
        ctx.admit_iter(&inventory.constraints, "visit Inventor sketch constraints")?
            .map(|record| {
                (
                    {
                        (
                            record.identity.segment_token.as_str(),
                            record.identity.record_ordinal,
                        )
                    },
                    record,
                )
            }),
        "index Inventor sketch constraints",
    )?;
    let mut parameter_index_storage = ctx.reserve_scoped(0, "index Inventor sketch parameter")?;
    let mut parameter_index = HashMap::new();
    for parameter in ctx.admit_iter(parameters, "visit Inventor sketch items")? {
        if let Some(native) = &parameter.native_ref {
            let id = parameter_index_storage.with_storage(|| {
                parameter
                    .id
                    .try_clone_for_decode(ctx, "retain Inventor sketch parameter id")
            })?;
            let native = parameter_index_storage
                .with_storage(|| ctx.copy_retained_text(native, "copy Inventor sketch.rs text"))?;
            parameter_index_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut parameter_index,
                    native,
                    id,
                    "index Inventor sketch parameter",
                )
            })?;
        }
    }

    let mut projected_entities = Vec::new();
    let mut unresolved_entities = 0usize;
    for entity in ctx.admit_iter(&inventory.entities, "visit Inventor sketch items")? {
        let key = (
            entity.identity.segment_token.as_str(),
            entity.identity.record_ordinal,
        );
        if !ctx.contains_key_hash_map(&raw_entities, &key, "access Inventor sketch records")? {
            unresolved_entities += 1;
            continue;
        }
        let Some(sketch_ordinal) = entity.sketch.index().checked_sub(1) else {
            unresolved_entities += 1;
            continue;
        };
        let Some(sketch) = ctx
            .get_hash_map(
                &raw_sketches,
                &(entity.identity.segment_token.as_str(), sketch_ordinal),
                "access Inventor sketch records",
            )?
            .and_then(Option::as_ref)
        else {
            unresolved_entities += 1;
            continue;
        };
        let entity_reference_found = ctx
            .admit_iter(
                sketch.entities.references(),
                "find Inventor sketch entity reference",
            )?
            .find_map(|reference| {
                entity
                    .identity
                    .record_ordinal
                    .checked_add(1)
                    .is_some_and(|next| reference.index() == next)
                    .then_some(Ok::<bool, CodecError>(true))
            })
            .transpose()?
            .unwrap_or(false);
        if !entity_reference_found {
            unresolved_entities += 1;
            continue;
        }
        let Some(geometry) = project_geometry(ctx, entity, &raw_entities)? else {
            unresolved_entities += 1;
            continue;
        };
        let (Some(entity_id), Some(sketch_id)) = (entity_id(ctx, entity)?, sketch_id(ctx, sketch)?)
        else {
            unresolved_entities += 1;
            continue;
        };
        ctx.charge_entities(1, "project Inventor sketch entity")?;
        ctx.push_vec(
            &mut projected_entities,
            SketchEntity::new(entity_id, sketch_id, geometry)
                .with_construction(entity.entity_flags & 0x0408_0040 != 0)
                .with_native_ref(Some(entity.id(ctx)?))
                .with_endpoint_refs(entity_endpoint_refs(ctx, entity, &raw_entities)?),
            "collect Inventor sketch items",
        )?;
    }

    let mut projected_by_native_storage =
        ctx.reserve_scoped(0, "index projected Inventor sketch entities")?;
    let projected_by_native = projected_by_native_storage.with_storage(|| {
        ctx.collect_hash_map(
            ctx.admit_iter(
                &projected_entities,
                "index projected Inventor sketch entities",
            )?
            .filter_map(|entity| entity.native_ref.as_deref().map(|native| (native, entity))),
            "index projected Inventor sketch entities",
        )
    })?;
    let mut sketches = Vec::new();
    let mut unresolved_sketches = 0usize;
    for sketch in ctx.admit_iter(&inventory.sketches, "visit Inventor sketch items")? {
        let key = (
            sketch.identity.segment_token.as_str(),
            sketch.identity.record_ordinal,
        );
        if !ctx.contains_key_hash_map(&raw_sketches, &key, "access Inventor sketch records")? {
            unresolved_sketches += 1;
            continue;
        }
        let (mut raw_referenced_entities, mut raw_referenced_storage) =
            ctx.temporary_vec(0, "collect Inventor raw sketch reference")?;
        for reference in
            ctx.admit_iter(sketch.entities.references(), "visit Inventor sketch items")?
        {
            let Some(ordinal) = reference.index().checked_sub(1) else {
                continue;
            };
            if let Some(raw) = ctx
                .get_hash_map(
                    &raw_entities,
                    &(sketch.identity.segment_token.as_str(), ordinal),
                    "access Inventor sketch records",
                )?
                .and_then(Option::as_ref)
                .copied()
            {
                raw_referenced_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut raw_referenced_entities,
                        raw,
                        "collect Inventor raw sketch reference",
                    )
                })?;
            }
        }
        let (mut referenced_entities, mut referenced_storage) =
            ctx.temporary_vec(0, "collect Inventor projected sketch reference")?;
        for raw in ctx.admit_iter(&raw_referenced_entities, "visit Inventor sketch items")? {
            let (native, _native_reservation) = ctx.format_scoped(
                format_args!(
                    "inventor:pmdc:sketch-entity#{}-{}",
                    raw.identity.segment_token, raw.identity.record_ordinal
                ),
                "resolve Inventor sketch entity native id",
            )?;
            if let Some(projected) = ctx
                .get_hash_map(
                    &projected_by_native,
                    native.as_str(),
                    "access Inventor sketch records",
                )?
                .copied()
            {
                referenced_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut referenced_entities,
                        projected,
                        "collect Inventor projected sketch reference",
                    )
                })?;
            }
        }
        if referenced_entities.len() != raw_referenced_entities.len() {
            unresolved_sketches += 1;
            continue;
        }
        let Some(placement) = project_placement(ctx, sketch, &transforms, &directions)? else {
            unresolved_sketches += 1;
            continue;
        };
        let Some(id) = sketch_id(ctx, sketch)? else {
            unresolved_sketches += 1;
            continue;
        };
        let Ok(profiles) = cadmpeg_ir::sketches::SketchProfiles::try_from(build_profiles(
            ctx,
            &referenced_entities,
        )?) else {
            unresolved_sketches += 1;
            continue;
        };
        ctx.charge_entities(1, "project Inventor sketch")?;
        ctx.push_vec(
            &mut sketches,
            Sketch {
                id,
                name: None,
                configuration: None,
                visible: None,
                placement,
                profiles,
                native_ref: Some(sketch.id(ctx)?),
            },
            "collect Inventor sketch items",
        )?;
    }
    drop(projected_by_native);
    drop(projected_by_native_storage);
    let mut projected_sketch_ids_storage =
        ctx.reserve_scoped(0, "collect projected Inventor sketch ids")?;
    let projected_sketch_ids = projected_sketch_ids_storage.with_storage(|| {
        ctx.collect_hash_set(
            ctx.admit_iter(&sketches, "collect projected Inventor sketch ids")?
                .map(|sketch| &sketch.id),
            "collect projected Inventor sketch ids",
        )
    })?;
    let previous_entity_count = projected_entities.len();
    ctx.retain_vec(
        &mut projected_entities,
        |entity| {
            ctx.contains_hash_set(
                &projected_sketch_ids,
                &entity.sketch,
                "retain entities of projected Inventor sketches",
            )
        },
        "retain entities of projected Inventor sketches",
    )?;
    unresolved_entities = count_removed(
        ctx,
        unresolved_entities,
        previous_entity_count,
        projected_entities.len(),
        "Inventor unresolved sketch entities",
    )?;
    drop(projected_sketch_ids);
    drop(projected_sketch_ids_storage);
    let mut projected_by_native_storage =
        ctx.reserve_scoped(0, "reindex projected Inventor sketch entities")?;
    let projected_by_native = projected_by_native_storage.with_storage(|| {
        ctx.collect_hash_map(
            ctx.admit_iter(
                &projected_entities,
                "reindex projected Inventor sketch entities",
            )?
            .filter_map(|entity| entity.native_ref.as_deref().map(|native| (native, entity))),
            "reindex projected Inventor sketch entities",
        )
    })?;
    let mut projected_entity_by_key_storage =
        ctx.reserve_scoped(0, "index projected Inventor sketch entity key")?;
    let mut projected_entity_by_key = HashMap::new();
    for raw in ctx.admit_iter(&inventory.entities, "visit Inventor sketch items")? {
        let (native_id, _native_reservation) = ctx.format_scoped(
            format_args!(
                "inventor:pmdc:sketch-entity#{}-{}",
                raw.identity.segment_token, raw.identity.record_ordinal
            ),
            "resolve projected Inventor sketch native id",
        )?;
        if let Some(projected) = ctx.get_hash_map(
            &projected_by_native,
            native_id.as_str(),
            "access Inventor sketch records",
        )? {
            projected_entity_by_key_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut projected_entity_by_key,
                    (
                        raw.identity.segment_token.as_str(),
                        raw.identity.record_ordinal,
                    ),
                    *projected,
                    "index projected Inventor sketch entity key",
                )
            })?;
        }
    }
    drop(projected_by_native);
    drop(projected_by_native_storage);
    let mut constraints = Vec::new();
    let mut projected_constraint_keys_storage =
        ctx.reserve_scoped(0, "index projected Inventor sketch constraint")?;
    let mut projected_constraint_keys = HashSet::new();
    for constraint in ctx.admit_iter(&inventory.constraints, "visit Inventor sketch items")? {
        if ctx.contains_key_hash_map(
            &raw_constraints,
            &(
                constraint.identity.segment_token.as_str(),
                constraint.identity.record_ordinal,
            ),
            "access Inventor sketch records",
        )? {
            if let Some(projected) =
                project_constraint(ctx, constraint, &projected_entity_by_key, &parameter_index)
                    .transpose()?
            {
                projected_constraint_keys_storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut projected_constraint_keys,
                        (
                            constraint.identity.segment_token.as_str(),
                            constraint.identity.record_ordinal,
                        ),
                        "index projected Inventor sketch constraint",
                    )
                })?;
                ctx.push_vec(&mut constraints, projected, "collect Inventor sketch items")?;
            }
        }
    }
    let mut unresolved_constraints = inventory
        .constraints
        .len()
        .checked_sub(constraints.len())
        .ok_or_else(|| CodecError::malformed("Inventor constraints exceed inventory"))?;
    let mut projected_entity_keys_storage =
        ctx.reserve_scoped(0, "index projected Inventor sketch entity closure keys")?;
    let projected_entity_keys = projected_entity_keys_storage.with_storage(|| {
        ctx.collect_hash_set(
            ctx.admit_iter(
                &projected_entity_by_key,
                "index projected Inventor sketch entity closure keys",
            )?
            .map(|(key, _)| *key),
            "index projected Inventor sketch entity closure keys",
        )
    })?;
    drop(projected_entity_by_key);
    drop(projected_entity_by_key_storage);
    let mut raw_sketch_by_native_storage =
        ctx.reserve_scoped(0, "index Inventor raw sketch native refs")?;
    let mut raw_sketch_by_native = HashMap::new();
    for sketch in ctx.admit_iter(&inventory.sketches, "visit Inventor sketch items")? {
        let native = raw_sketch_by_native_storage.with_storage(|| sketch.id(ctx))?;
        raw_sketch_by_native_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut raw_sketch_by_native,
                native,
                sketch,
                "index Inventor raw sketch native refs",
            )
        })?;
    }
    let previous_sketch_count = sketches.len();
    ctx.retain_vec(
        &mut sketches,
        |projected| {
            let Some(native) = projected.native_ref.as_deref() else {
                return Ok(false);
            };
            let Some(raw) = ctx.get_hash_map(
                &raw_sketch_by_native,
                native,
                "access Inventor sketch records",
            )?
            else {
                return Ok(false);
            };
            let mut seen_storage = ctx.reserve_scoped(0, "access Inventor sketch records")?;
            let mut seen = HashSet::new();
            let closed = ctx
                .admit_iter(raw.entities.references(), "access Inventor sketch records")?
                .find_map(|reference| {
                    let resolved = (|| -> Result<bool, CodecError> {
                        let Some(ordinal) = reference.index().checked_sub(1) else {
                            return Ok(false);
                        };
                        if !seen_storage.with_storage(|| {
                            ctx.insert_hash_set(
                                &mut seen,
                                ordinal,
                                "access Inventor sketch records",
                            )
                        })? {
                            return Ok(false);
                        }
                        let key = (raw.identity.segment_token.as_str(), ordinal);
                        Ok(ctx.contains_hash_set(
                            &projected_entity_keys,
                            &key,
                            "access Inventor sketch records",
                        )? || ctx.contains_hash_set(
                            &projected_constraint_keys,
                            &key,
                            "access Inventor sketch records",
                        )?)
                    })();
                    match resolved {
                        Ok(true) => None,
                        Ok(false) => Some(Ok(false)),
                        Err(error) => Some(Err(error)),
                    }
                })
                .transpose()?
                .unwrap_or(true);
            Ok(closed)
        },
        "retain closed Inventor sketches",
    )?;
    unresolved_sketches = count_removed(
        ctx,
        unresolved_sketches,
        previous_sketch_count,
        sketches.len(),
        "Inventor unresolved sketches",
    )?;
    drop(raw_sketch_by_native);
    drop(raw_sketch_by_native_storage);
    drop(projected_entity_keys);
    drop(projected_entity_keys_storage);
    let mut closed_sketch_ids_storage =
        ctx.reserve_scoped(0, "index closed Inventor sketch ids")?;
    let closed_sketch_ids = closed_sketch_ids_storage.with_storage(|| {
        ctx.collect_hash_set(
            ctx.admit_iter(&sketches, "index closed Inventor sketch ids")?
                .map(|sketch| &sketch.id),
            "index closed Inventor sketch ids",
        )
    })?;
    let previous_entity_count = projected_entities.len();
    ctx.retain_vec(
        &mut projected_entities,
        |entity| {
            ctx.contains_hash_set(
                &closed_sketch_ids,
                &entity.sketch,
                "retain entities of closed Inventor sketches",
            )
        },
        "retain entities of closed Inventor sketches",
    )?;
    unresolved_entities = count_removed(
        ctx,
        unresolved_entities,
        previous_entity_count,
        projected_entities.len(),
        "Inventor unresolved sketch entities",
    )?;
    let mut raw_sketch_by_id_storage =
        ctx.reserve_scoped(0, "index Inventor raw sketch projected ids")?;
    let mut raw_sketch_by_id = HashMap::new();
    for sketch in ctx.admit_iter(&inventory.sketches, "visit Inventor sketch items")? {
        let id = raw_sketch_by_id_storage.with_storage(|| sketch_id(ctx, sketch))?;
        if let Some(id) = id {
            raw_sketch_by_id_storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut raw_sketch_by_id,
                    id,
                    sketch,
                    "index Inventor raw sketch projected ids",
                )
            })?;
        }
    }
    let mut raw_constraint_by_native_storage =
        ctx.reserve_scoped(0, "index Inventor raw constraint native refs")?;
    let mut raw_constraint_by_native = HashMap::new();
    for constraint in ctx.admit_iter(&inventory.constraints, "visit Inventor sketch items")? {
        let id = raw_constraint_by_native_storage.with_storage(|| constraint.id(ctx))?;
        raw_constraint_by_native_storage.with_storage(|| {
            ctx.insert_hash_map(
                &mut raw_constraint_by_native,
                id,
                constraint,
                "index Inventor raw constraint native refs",
            )
        })?;
    }
    let previous_constraint_count = constraints.len();
    ctx.retain_vec(
        &mut constraints,
        |constraint| {
            if !ctx.contains_hash_set(
                &closed_sketch_ids,
                &constraint.sketch,
                "retain closed Inventor constraints",
            )? {
                return Ok(false);
            }
            let Some(native) = constraint.native_ref.as_deref() else {
                return Ok(false);
            };
            let Some(raw_constraint) = ctx
                .get_hash_map(
                    &raw_constraint_by_native,
                    native,
                    "access Inventor sketch records",
                )?
                .copied()
            else {
                return Ok(false);
            };
            let Some(sketch) = ctx.get_hash_map(
                &raw_sketch_by_id,
                &constraint.sketch,
                "access Inventor sketch records",
            )?
            else {
                return Ok(false);
            };
            Ok(ctx
                .admit_iter(
                    sketch.entities.references(),
                    "access Inventor sketch records",
                )?
                .find_map(|reference| {
                    raw_constraint
                        .identity
                        .record_ordinal
                        .checked_add(1)
                        .is_some_and(|next| reference.index() == next)
                        .then_some(Ok::<bool, CodecError>(true))
                })
                .transpose()?
                .unwrap_or(false))
        },
        "retain closed Inventor constraints",
    )?;
    unresolved_constraints = count_removed(
        ctx,
        unresolved_constraints,
        previous_constraint_count,
        constraints.len(),
        "Inventor unresolved sketch constraints",
    )?;
    Ok(SketchProjection {
        sketches,
        entities: projected_entities,
        constraints,
        unresolved_sketches,
        unresolved_entities,
        unresolved_constraints,
    })
}

fn project_constraint(
    ctx: &DecodeContext<'_>,
    constraint: &PmDcSketchConstraint,
    entities: &HashMap<(&str, u32), &SketchEntity>,
    parameters: &HashMap<String, ParameterId>,
) -> Option<Result<SketchConstraint, CodecError>> {
    macro_rules! admit {
        ($result:expr) => {
            if let Err(error) = $result {
                return Some(Err(error));
            }
        };
    }

    macro_rules! admitted_value {
        ($result:expr) => {
            match $result {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            }
        };
    }
    if constraint.header.scalar_map.metadata().is_some()
        || !constraint.header.scalar_map.entries().is_empty()
        || constraint.header.reference_map.metadata().is_some()
        || !constraint.header.reference_map.entries().is_empty()
    {
        return None;
    }
    let resolve = |reference: PmDcReference| -> Result<_, CodecError> {
        let Some(ordinal) = reference.index().checked_sub(1) else {
            return Ok(None);
        };
        Ok(ctx
            .get_hash_map(
                entities,
                &(constraint.identity.segment_token.as_str(), ordinal),
                "resolve Inventor sketch constraint entity",
            )?
            .copied())
    };
    let (definition, orientation, members) = match constraint.kind {
        PmDcSketchConstraintKind::Coincident { first, second } => {
            let members = [
                admitted_value!(resolve(first))?,
                admitted_value!(resolve(second))?,
            ];
            (
                SketchConstraintDefinitionInput::Coincident {
                    entities: admitted_value!(ctx.try_collect_retained_with(
                        members,
                        "collect Inventor coincident constraint members",
                        |entity| entity.id().try_clone_for_decode(
                            ctx,
                            "retain Inventor sketch constraint entity id"
                        )
                    )),
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
            let members = [
                admitted_value!(resolve(first))?,
                admitted_value!(resolve(second))?,
            ];
            (
                SketchConstraintDefinitionInput::Parallel {
                    first: admitted_value!(members[0]
                        .id()
                        .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id")),
                    second: admitted_value!(members[1]
                        .id()
                        .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id")),
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
            let members = [
                admitted_value!(resolve(first))?,
                admitted_value!(resolve(second))?,
            ];
            (
                SketchConstraintDefinitionInput::Perpendicular {
                    first: admitted_value!(members[0]
                        .id()
                        .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id")),
                    second: admitted_value!(members[1]
                        .id()
                        .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id")),
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
            let members = [
                admitted_value!(resolve(first))?,
                admitted_value!(resolve(second))?,
            ];
            (
                SketchConstraintDefinitionInput::Tangent {
                    first: admitted_value!(members[0]
                        .id()
                        .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id")),
                    second: admitted_value!(members[1]
                        .id()
                        .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id")),
                },
                extension,
                members,
            )
        }
        PmDcSketchConstraintKind::Horizontal { entity, state } => {
            let member = admitted_value!(resolve(entity))?;
            (
                SketchConstraintDefinitionInput::Horizontal {
                    entity: admitted_value!(member
                        .id()
                        .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id")),
                },
                Some(u32::from(state)),
                [member, member],
            )
        }
        PmDcSketchConstraintKind::Vertical { entity, state } => {
            let member = admitted_value!(resolve(entity))?;
            (
                SketchConstraintDefinitionInput::Vertical {
                    entity: admitted_value!(member
                        .id()
                        .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id")),
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
            let members = [
                admitted_value!(resolve(first))?,
                admitted_value!(resolve(second))?,
            ];
            let parameter =
                admitted_value!(resolve_parameter(ctx, constraint, parameter, parameters)?);
            (
                SketchConstraintDefinitionInput::HorizontalDistance {
                    first: SketchLocus::Entity(admitted_value!(members[0]
                        .id()
                        .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id"))),
                    second: SketchLocus::Entity(admitted_value!(members[1]
                        .id()
                        .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id"))),
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
            let members = [
                admitted_value!(resolve(first))?,
                admitted_value!(resolve(second))?,
            ];
            let parameter =
                admitted_value!(resolve_parameter(ctx, constraint, parameter, parameters)?);
            (
                SketchConstraintDefinitionInput::VerticalDistance {
                    first: SketchLocus::Entity(admitted_value!(members[0]
                        .id()
                        .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id"))),
                    second: SketchLocus::Entity(admitted_value!(members[1]
                        .id()
                        .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id"))),
                    parameter,
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::Radius { entity, .. } => {
            let member = admitted_value!(resolve(entity))?;
            let parameter = admitted_value!(resolve_parameter(
                ctx,
                constraint,
                constraint.header.parameter,
                parameters
            )?);
            (
                SketchConstraintDefinitionInput::Radius {
                    entity: admitted_value!(member
                        .id()
                        .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id")),
                    parameter,
                },
                None,
                [member, member],
            )
        }
        PmDcSketchConstraintKind::Diameter { entity, .. } => {
            let member = admitted_value!(resolve(entity))?;
            let parameter = admitted_value!(resolve_parameter(
                ctx,
                constraint,
                constraint.header.parameter,
                parameters
            )?);
            (
                SketchConstraintDefinitionInput::Diameter {
                    entity: admitted_value!(member
                        .id()
                        .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id")),
                    parameter,
                },
                None,
                [member, member],
            )
        }
        PmDcSketchConstraintKind::CircleCenter { entity, center } => {
            let members = [
                admitted_value!(resolve(entity))?,
                admitted_value!(resolve(center))?,
            ];
            (
                SketchConstraintDefinitionInput::Native {
                    native_kind: cadmpeg_core::nonblank_literal!("circle_center_alignment"),
                    native_state: Some(u64::from(constraint.header.state.cast_unsigned())),
                    native_flags: Some(u64::from(constraint.header.content.flags)),
                    native_properties: std::collections::BTreeMap::new(),
                    entities: admitted_value!(ctx.try_collect_retained_with(
                        members,
                        "collect Inventor circle center entities",
                        |entity| entity.id().try_clone_for_decode(
                            ctx,
                            "retain Inventor sketch constraint entity id"
                        )
                    )),
                    parameter: None,
                    operands: vec![
                        admitted_value!(native_operand(
                            ctx,
                            constraint,
                            cadmpeg_core::nonblank_literal!("entity"),
                            entity,
                        )),
                        admitted_value!(native_operand(
                            ctx,
                            constraint,
                            cadmpeg_core::nonblank_literal!("center"),
                            center,
                        )),
                    ],
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::EqualRadius { first, second } => {
            let members = [
                admitted_value!(resolve(first))?,
                admitted_value!(resolve(second))?,
            ];
            (
                SketchConstraintDefinitionInput::Equal {
                    first: admitted_value!(members[0]
                        .id()
                        .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id")),
                    second: admitted_value!(members[1]
                        .id()
                        .try_clone_for_decode(ctx, "retain Inventor sketch constraint entity id")),
                },
                None,
                members,
            )
        }
    };
    if !admitted_value!(ctx.equal(
        &members[0].sketch,
        &members[1].sketch,
        "compare Inventor sketch constraint owners",
    )) {
        return None;
    }
    admit!(ctx.charge_entities(1, "project Inventor sketch constraint"));
    let id = admitted_value!(ctx.format_retained(
        format_args!(
            "inventor:design:sketch-constraint#{}-{}",
            constraint.identity.segment_token, constraint.identity.record_ordinal
        ),
        "retain projected Inventor sketch constraint id",
    ));
    let key_len = match id
        .len()
        .checked_sub("inventor:design:sketch-constraint#".len())
    {
        Some(value) => value,
        None => {
            return Some(Err(CodecError::malformed(
                "formatted Inventor constraint id is shorter than its prefix",
            )))
        }
    };
    let validation_work = match cadmpeg_core::decode::u64_from_index(id.len())
        .checked_add(cadmpeg_core::decode::u64_from_index(key_len))
    {
        Some(value) => value,
        None => {
            return Some(Err(ctx.refuse_codec_limit(
                "validate projected Inventor identity",
                u64::MAX,
                u64::MAX,
            )))
        }
    };
    admit!(ctx.charge_work(validation_work, "validate projected Inventor identity"));
    Some(Ok(SketchConstraint {
        id: SketchConstraintId::mint(id).ok()?,
        sketch: admitted_value!(members[0]
            .sketch
            .try_clone_for_decode(ctx, "retain Inventor sketch constraint owner id")),
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
        native_ref: Some(admitted_value!(constraint.id(ctx))),
    }))
}

fn resolve_parameter(
    ctx: &DecodeContext<'_>,
    constraint: &PmDcSketchConstraint,
    reference: PmDcReference,
    parameters: &HashMap<String, ParameterId>,
) -> Option<Result<ParameterId, CodecError>> {
    let ordinal = reference.index().checked_sub(1)?;
    let (native, _storage) = match ctx.format_scoped(
        format_args!(
            "inventor:pmdc:parameter#{}-{ordinal}",
            constraint.identity.segment_token
        ),
        "resolve Inventor sketch constraint parameter",
    ) {
        Ok(value) => value,
        Err(error) => return Some(Err(error)),
    };
    let parameter = match ctx.get_hash_map(
        parameters,
        native.as_str(),
        "access Inventor sketch records",
    ) {
        Ok(value) => value?,
        Err(error) => return Some(Err(error)),
    };
    Some(parameter.try_clone_for_decode(ctx, "retain Inventor sketch constraint parameter id"))
}

fn native_operand(
    ctx: &DecodeContext<'_>,
    constraint: &PmDcSketchConstraint,
    field: cadmpeg_core::text::NonBlankString,
    reference: PmDcReference,
) -> Result<SketchNativeOperand, CodecError> {
    Ok(SketchNativeOperand {
        native_kind: cadmpeg_core::nonblank_literal!("record_reference"),
        field: Some(NativeOperandField {
            name: field,
            role: None,
        }),
        object_index: Some(reference.index()),
        native_ref: reference
            .index()
            .checked_sub(1)
            .map(|ordinal| {
                ctx.format_retained(
                    format_args!(
                        "inventor:pmdc:sketch-entity#{}-{ordinal}",
                        constraint.identity.segment_token
                    ),
                    "retain Inventor sketch constraint operand native id",
                )
            })
            .transpose()?,
    })
}

fn project_geometry(
    ctx: &DecodeContext<'_>,
    entity: &PmDcSketchEntity,
    entities: &HashMap<(&str, u32), Option<&PmDcSketchEntity>>,
) -> Result<Option<SketchGeometry>, CodecError> {
    let definition = match &entity.kind {
        PmDcSketchEntityKind::Point { position, .. } => SketchGeometryDefinition::Point {
            position: neutral_point(*position),
        },
        PmDcSketchEntityKind::Line {
            points,
            origin,
            direction,
            ..
        } => {
            let [start, end] = points.references() else {
                return Ok(None);
            };
            let Some(start) = resolve_point(
                ctx,
                entity.identity.segment_token.as_str(),
                start.index(),
                entities,
            )?
            else {
                return Ok(None);
            };
            let Some(end) = resolve_point(
                ctx,
                entity.identity.segment_token.as_str(),
                end.index(),
                entities,
            )?
            else {
                return Ok(None);
            };
            if !line_carrier_matches(
                origin.map(FiniteReal::get),
                direction.map(FiniteReal::get),
                start.map(FiniteReal::get),
                end.map(FiniteReal::get),
            ) {
                return Ok(None);
            }
            SketchGeometryDefinition::Line {
                start: neutral_point(start),
                end: neutral_point(end),
            }
        }
        PmDcSketchEntityKind::Circle { center, radius, .. } => {
            let Some(center) = resolve_point(
                ctx,
                entity.identity.segment_token.as_str(),
                center.index(),
                entities,
            )?
            else {
                return Ok(None);
            };
            let Some(radius) = Length::new(radius.get() * 10.0) else {
                return Ok(None);
            };
            SketchGeometryDefinition::Circle {
                center: neutral_point(center),
                radius,
            }
        }
        PmDcSketchEntityKind::Ellipse {
            center,
            major_direction,
            major_radius,
            minor_radius,
            ..
        } => {
            let Some(center) = resolve_point(
                ctx,
                entity.identity.segment_token.as_str(),
                center.index(),
                entities,
            )?
            else {
                return Ok(None);
            };
            let norm = major_direction[0].get().hypot(major_direction[1].get());
            if !norm.is_finite() || norm <= f64::EPSILON {
                return Ok(None);
            }
            let Some(major_angle) =
                Angle::new(major_direction[1].get().atan2(major_direction[0].get()))
            else {
                return Ok(None);
            };
            let Some(major_radius) = Length::new(major_radius.get() * 10.0) else {
                return Ok(None);
            };
            let Some(minor_radius) = Length::new(minor_radius.get() * 10.0) else {
                return Ok(None);
            };
            SketchGeometryDefinition::Ellipse {
                center: neutral_point(center),
                major_angle,
                radii: cadmpeg_ir::sketches::EllipseRadii {
                    major_radius,
                    minor_radius,
                },
                bounds: None,
            }
        }
    };
    Ok(SketchGeometry::try_from(definition).ok())
}

fn line_carrier_matches(
    origin: [f64; 2],
    direction: [f64; 2],
    start: [f64; 2],
    end: [f64; 2],
) -> bool {
    let Some(unit) =
        cadmpeg_ir::features::FiniteVector3::new(Vector3::new(direction[0], direction[1], 0.0))
            .and_then(cadmpeg_ir::features::FiniteVector3::unit_nonzero)
    else {
        return false;
    };
    let span = Vector3::new(end[0] - start[0], end[1] - start[1], 0.0);
    let span_scale = span.x.abs().max(span.y.abs());
    let Some(span) = cadmpeg_ir::features::FiniteVector3::new(span)
        .and_then(cadmpeg_ir::features::FiniteVector3::unit_nonzero)
    else {
        return false;
    };
    let parallel_error = (unit.x * span.y - unit.y * span.x).abs();
    if parallel_error > EPS_SKETCH_LINE_CARRIER_MATCHES_E10 {
        return false;
    }
    let from_origin = Vector3::new(start[0] - origin[0], start[1] - origin[1], 0.0);
    if !from_origin.x.is_finite() || !from_origin.y.is_finite() {
        return false;
    }
    let offset_scale = from_origin.x.abs().max(from_origin.y.abs());
    if offset_scale == 0.0 {
        return true;
    }
    // Position agreement uses the finite segment scale, which stays unchanged
    // when the stored origin moves along the same infinite line.
    // Preserve the stored direction ratio when measuring position: rounding a
    // unit direction can rotate a distant incident point away from the line.
    let Some(exponent) =
        cadmpeg_ir::math::power_of_two_bound(direction[0].abs().max(direction[1].abs()))
    else {
        return false;
    };
    let [Some(dx), Some(dy)] = direction.map(|value| {
        cadmpeg_ir::math::scale_power_of_two(value, -exponent)
            .map(cadmpeg_ir::scalar::FiniteReal::get)
    }) else {
        return false;
    };
    // Scale position and span separately so a subnormal perpendicular offset
    // survives the determinant and a distant origin cannot overflow it.
    let (Some(offset_exponent), Some(span_exponent)) = (
        cadmpeg_ir::math::power_of_two_bound(offset_scale),
        cadmpeg_ir::math::power_of_two_bound(span_scale),
    ) else {
        return false;
    };
    let [Some(x), Some(y)] = [from_origin.x, from_origin.y].map(|value| {
        cadmpeg_ir::math::scale_power_of_two(value, -offset_exponent)
            .map(cadmpeg_ir::scalar::FiniteReal::get)
    }) else {
        return false;
    };
    let Some(scaled_span) = cadmpeg_ir::math::scale_power_of_two(span_scale, -span_exponent)
        .map(cadmpeg_ir::scalar::FiniteReal::get)
    else {
        return false;
    };
    let right = dy * x;
    let determinant = dx.mul_add(y, -right) - dy.mul_add(x, -right);
    let carrier_error = determinant.abs() / dx.hypot(dy) / scaled_span;
    cadmpeg_ir::math::scale_power_of_two(carrier_error, offset_exponent - span_exponent)
        .is_some_and(|error| error.get() <= EPS_SKETCH_LINE_CARRIER_MATCHES_E10)
}

fn resolve_point(
    ctx: &DecodeContext<'_>,
    token: &str,
    reference: u32,
    entities: &HashMap<(&str, u32), Option<&PmDcSketchEntity>>,
) -> Result<Option<[FiniteReal; 2]>, CodecError> {
    let Some(ordinal) = reference.checked_sub(1) else {
        return Ok(None);
    };
    let Some(entity) = ctx
        .get_hash_map(entities, &(token, ordinal), "resolve Inventor sketch point")?
        .and_then(Option::as_ref)
    else {
        return Ok(None);
    };
    Ok(match entity.kind {
        PmDcSketchEntityKind::Point { position, .. } => Some(position),
        _ => None,
    })
}

fn neutral_point(value: [FiniteReal; 2]) -> Point2 {
    Point2::new(value[0].get() * 10.0, value[1].get() * 10.0)
}

fn entity_endpoint_refs(
    ctx: &DecodeContext<'_>,
    entity: &PmDcSketchEntity,
    entities: &HashMap<(&str, u32), Option<&PmDcSketchEntity>>,
) -> Result<Vec<String>, CodecError> {
    let PmDcSketchEntityKind::Line { points, .. } = &entity.kind else {
        return Ok(Vec::new());
    };
    let mut endpoint_refs = Vec::new();
    for reference in ctx.admit_iter(points.references(), "visit Inventor sketch items")? {
        let value = match reference.index().checked_sub(1) {
            Some(ordinal) => ctx
                .get_hash_map(
                    entities,
                    &(entity.identity.segment_token.as_str(), ordinal),
                    "resolve Inventor sketch endpoint",
                )?
                .and_then(Option::as_ref),
            None => None,
        };
        if let Some(value) = value {
            ctx.push_vec(
                &mut endpoint_refs,
                value.id(ctx)?,
                "collect Inventor sketch items",
            )?;
        }
    }
    Ok(endpoint_refs)
}

fn project_placement(
    ctx: &DecodeContext<'_>,
    sketch: &PmDcSketch,
    transforms: &HashMap<(&str, u32), Option<&PmDcTransform>>,
    directions: &HashMap<(&str, u32), Option<&PmDcDirection>>,
) -> Result<Option<SketchPlacement>, CodecError> {
    let Some(transform_ordinal) = sketch.transform.index().checked_sub(1) else {
        return Ok(None);
    };
    let Some(direction_ordinal) = sketch.direction.index().checked_sub(1) else {
        return Ok(None);
    };
    let Some(transform) = ctx
        .get_hash_map(
            transforms,
            &(sketch.identity.segment_token.as_str(), transform_ordinal),
            "resolve Inventor sketch placement transform",
        )?
        .and_then(Option::as_ref)
    else {
        return Ok(None);
    };
    let Some(direction) = ctx
        .get_hash_map(
            directions,
            &(sketch.identity.segment_token.as_str(), direction_ordinal),
            "resolve Inventor sketch placement direction",
        )?
        .and_then(Option::as_ref)
    else {
        return Ok(None);
    };
    let matrix = transform.matrix.rows();
    let expected_last_row = [0.0, 0.0, 0.0, 1.0];
    if matrix[3]
        .iter()
        .zip(expected_last_row)
        .any(|(actual, expected)| (actual - expected).abs() > EPS_SKETCH_PROJECT_PLACEMENT_E10)
    {
        return Ok(None);
    }
    let Some(u_axis) = Vector3::new(matrix[0][0], matrix[1][0], matrix[2][0]).unit() else {
        return Ok(None);
    };
    let Some(v_axis) = Vector3::new(matrix[0][1], matrix[1][1], matrix[2][1]).unit() else {
        return Ok(None);
    };
    let Some(normal) = Vector3::new(matrix[0][2], matrix[1][2], matrix[2][2]).unit() else {
        return Ok(None);
    };
    let Some(stored_direction) = Vector3::new(
        direction.direction[0].get(),
        direction.direction[1].get(),
        direction.direction[2].get(),
    )
    .unit() else {
        return Ok(None);
    };
    if u_axis.dot(v_axis).abs() > EPS_SKETCH_PROJECT_PLACEMENT_E10
        || u_axis.dot(normal).abs() > EPS_SKETCH_PROJECT_PLACEMENT_E10
        || v_axis.dot(normal).abs() > EPS_SKETCH_PROJECT_PLACEMENT_E10
        || u_axis.cross(v_axis).dot(normal) < 1.0 - EPS_SKETCH_PROJECT_PLACEMENT_E10
        || normal.dot(stored_direction) < 1.0 - EPS_SKETCH_PROJECT_PLACEMENT_E10
    {
        return Ok(None);
    }
    Ok(SketchPlacement::try_resolved(
        Point3::new(
            matrix[0][3] * 10.0,
            matrix[1][3] * 10.0,
            matrix[2][3] * 10.0,
        ),
        normal,
        u_axis,
    )
    .ok())
}

fn build_profiles(
    ctx: &DecodeContext<'_>,
    entities: &[&SketchEntity],
) -> Result<Vec<Vec<SketchEntityUse>>, CodecError> {
    let mut source_positions_storage =
        ctx.reserve_scoped(0, "index Inventor profile source positions")?;
    let source_positions = source_positions_storage.with_storage(|| {
        ctx.collect_hash_map(
            ctx.admit_iter(entities, "index Inventor profile source positions")?
                .enumerate()
                .map(|(index, entity)| (entity.id().as_str(), index)),
            "index Inventor profile source positions",
        )
    })?;
    let mut profiles = Vec::new();
    let mut profiles_storage = ctx.reserve_scoped(0, "collect Inventor sketch items")?;
    for entity in ctx
        .admit_iter(entities, "visit Inventor sketch entity items")?
        .filter(|entity| !entity.construction)
        .filter(|entity| {
            matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Circle { .. } | SketchGeometryDefinition::Ellipse { .. }
            )
        })
    {
        let mut profile = Vec::new();
        ctx.push_vec(
            &mut profile,
            SketchEntityUse {
                entity: entity
                    .id()
                    .try_clone_for_decode(ctx, "retain Inventor circular profile entity id")?,
                reversed: false,
            },
            "project Inventor circular profile use",
        )?;
        profiles_storage.with_storage(|| {
            ctx.push_vec(&mut profiles, profile, "collect Inventor sketch items")
        })?;
    }
    let mut lines_storage = ctx.reserve_scoped(0, "collect Inventor profile lines")?;
    let lines = lines_storage.with_storage(|| {
        ctx.collect_vec(
            ctx.admit_iter(entities, "collect Inventor profile lines")?
                .copied()
                .filter(|entity| !entity.construction)
                .filter(|entity| {
                    matches!(
                        *entity.geometry.definition(),
                        SketchGeometryDefinition::Line { .. }
                    )
                })
                .filter(|entity| entity.endpoint_refs.len() == 2),
            "collect Inventor profile lines",
        )
    })?;
    let mut adjacency_storage = ctx.reserve_scoped(0, "index Inventor profile endpoint")?;
    let mut adjacency = HashMap::<&str, Vec<usize>>::new();
    for (index, line) in ctx
        .admit_iter(&lines, "visit Inventor sketch items")?
        .enumerate()
    {
        for endpoint in ctx.admit_iter(&line.endpoint_refs, "visit Inventor sketch items")? {
            adjacency_storage.with_storage(|| {
                ctx.push_hash_group(
                    &mut adjacency,
                    endpoint.as_str(),
                    index,
                    "index Inventor profile endpoint",
                    "link Inventor profile line endpoint",
                )
            })?;
        }
    }
    let mut visited_storage = ctx.reserve_scoped(0, "visit Inventor profile line")?;
    let mut visited = HashSet::new();
    for (start_index, _) in ctx
        .admit_iter(&lines, "traverse Inventor profile lines")?
        .enumerate()
    {
        if ctx.contains_hash_set(&visited, &start_index, "visit Inventor profile line")? {
            continue;
        }
        let (component, _component_storage) = line_component(ctx, start_index, &lines, &adjacency)?;
        let mut open_component = false;
        for &index in ctx.admit_iter(&component, "visit Inventor line component")? {
            let endpoint_has_wrong_degree = ctx
                .admit_iter(
                    &lines[index].endpoint_refs,
                    "check Inventor profile endpoint degree",
                )?
                .find_map(|point| {
                    match ctx
                        .get_hash_map(&adjacency, point.as_str(), "access Inventor sketch records")
                        .map(|indices| indices.map_or(0, Vec::len) != 2)
                    {
                        Ok(true) => Some(Ok(true)),
                        Ok(false) => None,
                        Err(error) => Some(Err(error)),
                    }
                })
                .transpose()?
                .unwrap_or(false);
            if endpoint_has_wrong_degree {
                open_component = true;
                break;
            }
        }
        if open_component {
            for &index in ctx.admit_iter(&component, "visit Inventor line component")? {
                visited_storage.with_storage(|| {
                    ctx.insert_hash_set(&mut visited, index, "visit Inventor profile line")
                })?;
            }
            continue;
        }
        let first = lines[start_index];
        let start_point = first.endpoint_refs[0].as_str();
        let mut point = first.endpoint_refs[1].as_str();
        let mut current = start_index;
        let (mut loop_uses, mut loop_uses_storage) =
            ctx.temporary_vec(0, "collect Inventor profile use")?;
        loop_uses_storage.with_storage(|| {
            ctx.push_vec(
                &mut loop_uses,
                SketchEntityUse {
                    entity: first
                        .id()
                        .try_clone_for_decode(ctx, "retain Inventor profile use id")?,
                    reversed: false,
                },
                "collect Inventor sketch items",
            )
        })?;
        visited_storage.with_storage(|| {
            ctx.insert_hash_set(&mut visited, current, "visit Inventor profile line")
        })?;
        while !ctx.equal(point, start_point, "close Inventor profile loop")? {
            ctx.charge_work(1, "traverse Inventor profile loop")?;
            let Some(indices) =
                ctx.get_hash_map(&adjacency, point, "access Inventor sketch records")?
            else {
                ctx.clear_vec(&mut loop_uses, "discard open Inventor profile loop")?;
                break;
            };
            let Some(next) = ctx
                .admit_iter(indices, "find next Inventor profile line")?
                .copied()
                .find(|index| *index != current)
            else {
                ctx.clear_vec(&mut loop_uses, "discard open Inventor profile loop")?;
                break;
            };
            if ctx.contains_hash_set(&visited, &next, "visit Inventor profile line")? {
                ctx.clear_vec(&mut loop_uses, "discard open Inventor profile loop")?;
                break;
            }
            let line = lines[next];
            let reversed = ctx.equal(
                line.endpoint_refs[1].as_str(),
                point,
                "orient Inventor profile line",
            )?;
            point = if reversed {
                line.endpoint_refs[0].as_str()
            } else {
                line.endpoint_refs[1].as_str()
            };
            current = next;
            visited_storage.with_storage(|| {
                ctx.insert_hash_set(&mut visited, next, "visit Inventor profile line")
            })?;
            loop_uses_storage.with_storage(|| {
                ctx.push_vec(
                    &mut loop_uses,
                    SketchEntityUse {
                        entity: line
                            .id()
                            .try_clone_for_decode(ctx, "retain Inventor profile use id")?,
                        reversed,
                    },
                    "collect Inventor sketch items",
                )
            })?;
        }
        for &index in ctx.admit_iter(&component, "visit Inventor line component")? {
            visited_storage.with_storage(|| {
                ctx.insert_hash_set(&mut visited, index, "visit Inventor profile line")
            })?;
        }
        if loop_uses.len() >= 3 && ctx.equal(point, start_point, "close Inventor profile loop")? {
            loop_uses_storage.commit()?;
            profiles_storage.with_storage(|| {
                ctx.push_vec(&mut profiles, loop_uses, "collect Inventor sketch items")
            })?;
        }
    }
    drop(adjacency);
    drop(adjacency_storage);
    drop(visited);
    drop(visited_storage);
    drop(lines);
    drop(lines_storage);
    let mut sort_storage = ctx.reserve_scoped(0, "order Inventor line profiles")?;
    let mut ordered = sort_storage.with_storage(|| {
        ctx.try_collect_vec(
            profiles.into_iter().map(|profile| {
                let mut first = None;
                for entity in ctx.admit_iter(&profile, "locate Inventor profile source order")? {
                    if let Some(&position) = ctx.get_hash_map(
                        &source_positions,
                        entity.entity.as_str(),
                        "locate Inventor profile source position",
                    )? {
                        first = Some(first.map_or(position, |prior: usize| prior.min(position)));
                    }
                }
                Ok::<_, CodecError>((first.is_none(), first, profile))
            }),
            "order Inventor line profiles",
        )
    })?;
    drop(profiles_storage);
    ctx.sort_unstable_by_key(
        &mut ordered,
        |value| (value.0, value.1),
        Ord::cmp,
        "Inventor line profiles sort",
    )?;
    let profiles = ctx.collect_vec(
        ordered.into_iter().map(|(_, _, profile)| profile),
        "collect Inventor ordered profiles",
    )?;
    Ok(profiles)
}

fn line_component<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    start: usize,
    lines: &[&SketchEntity],
    adjacency: &HashMap<&str, Vec<usize>>,
) -> Result<
    (
        HashSet<usize>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    let (mut pending, mut pending_storage) = ctx.temporary_vec(0, "queue Inventor profile line")?;
    pending_storage
        .with_storage(|| ctx.push_vec(&mut pending, start, "queue Inventor profile line"))?;
    let (mut component, mut component_storage) =
        ctx.temporary_set(0, "collect Inventor profile component")?;
    while !pending.is_empty() {
        ctx.charge_work(1, "scan Inventor profile component")?;
        let Some(index) = pending.pop() else {
            break;
        };
        if !component_storage.with_storage(|| {
            ctx.insert_hash_set(&mut component, index, "collect Inventor profile component")
        })? {
            continue;
        }
        for point in ctx.admit_iter(&lines[index].endpoint_refs, "visit Inventor sketch items")? {
            if let Some(neighbours) =
                ctx.get_hash_map(adjacency, point.as_str(), "access Inventor sketch records")?
            {
                for &neighbour in ctx.admit_iter(neighbours, "queue Inventor profile neighbours")? {
                    pending_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut pending,
                            neighbour,
                            "queue Inventor profile neighbours",
                        )
                    })?;
                }
            }
        }
    }
    drop(pending);
    drop(pending_storage);
    Ok((component, component_storage))
}

fn sketch_id(ctx: &DecodeContext<'_>, sketch: &PmDcSketch) -> Result<Option<SketchId>, CodecError> {
    let id = ctx.format_retained(
        format_args!(
            "inventor:design:sketch#{}-{}",
            sketch.identity.segment_token, sketch.identity.record_ordinal
        ),
        "retain projected Inventor sketch_id identity",
    )?;
    let key_len = id
        .len()
        .checked_sub("inventor:design:sketch#".len())
        .ok_or_else(|| {
            CodecError::malformed("formatted Inventor sketch id is shorter than its prefix")
        })?;
    let validation_work = cadmpeg_core::decode::u64_from_index(id.len())
        .checked_add(cadmpeg_core::decode::u64_from_index(key_len))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("validate projected Inventor identity", u64::MAX, u64::MAX)
        })?;
    ctx.charge_work(validation_work, "validate projected Inventor identity")?;
    Ok(SketchId::mint(id).ok())
}

fn entity_id(
    ctx: &DecodeContext<'_>,
    entity: &PmDcSketchEntity,
) -> Result<Option<SketchEntityId>, CodecError> {
    let id = ctx.format_retained(
        format_args!(
            "inventor:design:sketch-entity#{}-{}",
            entity.identity.segment_token, entity.identity.record_ordinal
        ),
        "retain projected Inventor entity_id identity",
    )?;
    let key_len = id
        .len()
        .checked_sub("inventor:design:sketch-entity#".len())
        .ok_or_else(|| {
            CodecError::malformed("formatted Inventor sketch entity id is shorter than its prefix")
        })?;
    let validation_work = cadmpeg_core::decode::u64_from_index(id.len())
        .checked_add(cadmpeg_core::decode::u64_from_index(key_len))
        .ok_or_else(|| {
            ctx.refuse_codec_limit("validate projected Inventor identity", u64::MAX, u64::MAX)
        })?;
    ctx.charge_work(validation_work, "validate projected Inventor identity")?;
    Ok(SketchEntityId::mint(id).ok())
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
    use super::{
        inventory, parse_constraint, parse_direction, parse_entity, parse_sketch, parse_transform,
        project, PmDcSketchConstraintKind, PmDcSketchEntityKind, PmDcTransformPayload,
        PmDcTransformPayloadWire, SketchConstraintTag, SketchEntityTag, SketchInventory,
        COINCIDENT_TYPE, DIRECTION_TYPE, HORIZONTAL_TYPE, LINE_TYPE, POINT_TYPE, SKETCH_TYPE,
        TRANSFORM_TYPE,
    };
    use crate::container::InventorContainer;
    use crate::pmdc::{PmDcReference, PmDcReferenceList};
    use crate::record_identity::Located;
    use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
    use crate::test_support::test_fixtures::{content, parse, primary_envelope_fixture};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::scalar::FiniteReal;
    use cadmpeg_ir::sketches::SketchPlacement;

    fn real(value: f64) -> FiniteReal {
        FiniteReal::new(value).expect("finite test scalar")
    }

    fn fixture_record_type_id(value: [u8; 16]) -> crate::record_identity::RecordTypeId {
        crate::record_identity::RecordTypeId::from_bytes(
            &cadmpeg_test_support::service_decode_context(),
            value,
            "retain Inventor sketch fixture record type id",
        )
        .expect("service context admits fixture record type id")
    }

    #[test]
    fn profile_builder_refuses_collection_limit_before_source_index() {
        let entity = circle_profile_entity();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("profile context");
        assert!(matches!(
            super::build_profiles(&ctx, &[&entity]),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "index Inventor profile source positions"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service profile context");
        assert_eq!(
            super::build_profiles(&ctx, &[&entity])
                .expect("circle profile")
                .len(),
            1
        );
    }

    fn circle_profile_entity() -> cadmpeg_ir::sketches::SketchEntity {
        cadmpeg_ir::sketches::SketchEntity::new(
            cadmpeg_ir::sketches::SketchEntityId::mint("inventor:test:entity#1")
                .expect("entity id"),
            cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1").expect("sketch id"),
            cadmpeg_ir::sketches::SketchGeometry::try_from(
                cadmpeg_ir::sketches::SketchGeometryDefinition::Circle {
                    center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                    radius: cadmpeg_ir::scalar::Length::new(1.0).expect("positive radius"),
                },
            )
            .expect("circle geometry"),
        )
    }

    #[test]
    fn profile_builder_refuses_collection_limits_before_circular_profile_allocations() {
        let entity = circle_profile_entity();
        let arena = DecodeArena::new();
        // The count includes the source-position index, circular use, and outer profile slot.
        for (cap, operation) in [
            (1, "project Inventor circular profile use"),
            (2, "collect Inventor sketch items"),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("profile context");
            assert!(matches!(
                super::build_profiles(&ctx, &[&entity]),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::CollectionItems
                        && limit.operation == operation
            ));
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service profile context");
        assert_eq!(
            super::build_profiles(&ctx, &[&entity])
                .expect("circle profile")
                .len(),
            1
        );
    }

    #[test]
    fn line_component_refuses_collection_limit_before_queue_creation() {
        let entity = cadmpeg_ir::sketches::SketchEntity::new(
            cadmpeg_ir::sketches::SketchEntityId::mint("inventor:test:entity#2")
                .expect("entity id"),
            cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1").expect("sketch id"),
            cadmpeg_ir::sketches::SketchGeometry::try_from(
                cadmpeg_ir::sketches::SketchGeometryDefinition::Line {
                    start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                    end: cadmpeg_ir::math::Point2::new(1.0, 0.0),
                },
            )
            .expect("line geometry"),
        )
        .with_endpoint_refs(vec!["point-a".into(), "point-b".into()]);
        let adjacency = std::collections::HashMap::from([
            ("point-a", vec![0_usize]),
            ("point-b", vec![0_usize]),
        ]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("profile context");
        assert!(matches!(
            super::line_component(&ctx, 0, &[&entity], &adjacency),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "queue Inventor profile line"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service profile context");
        let (component, _storage) =
            super::line_component(&ctx, 0, &[&entity], &adjacency).expect("line component");
        assert_eq!(component.len(), 1);
    }

    #[test]
    fn sketch_projection_refuses_collection_limit_before_entity_index() {
        let point = parse(&point_bytes(0, 1, [0.0, 0.0]), |ctx, source| {
            parse_entity(ctx, SketchEntityTag::Point, source, 22).expect("point record")
        });
        let inventory = SketchInventory {
            sketches: Vec::new(),
            entities: vec![Located::new(
                point,
                fixture_record_type_id(POINT_TYPE),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                0,
            )],
            transforms: Vec::new(),
            directions: Vec::new(),
            constraints: Vec::new(),
            issues: Vec::new(),
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        assert!(matches!(
            project(&ctx, &inventory, &[]),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "index Inventor sketch entities"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert_eq!(
            project(&ctx, &inventory, &[])
                .expect("sketch projection")
                .unresolved_entities,
            1
        );
    }

    #[test]
    fn sketch_projection_refuses_entity_limit_before_entity_creation() {
        let mut sketch_bytes = content(0);
        sketch_bytes.extend_from_slice(&0_i32.to_le_bytes());
        sketch_bytes.extend_from_slice(&1_u32.to_le_bytes());
        sketch_bytes.extend(list(8, &[2]));
        sketch_bytes.extend_from_slice(&0_u32.to_le_bytes());
        sketch_bytes.extend_from_slice(&0_u32.to_le_bytes());
        sketch_bytes.extend_from_slice(&[0; 8]);
        sketch_bytes.extend(list(2, &[]));
        let sketch = parse(&sketch_bytes, |ctx, source| {
            parse_sketch(ctx, source, 22).expect("sketch record")
        });
        let point = parse(&point_bytes(1, 1, [0.0, 0.0]), |ctx, source| {
            parse_entity(ctx, SketchEntityTag::Point, source, 22).expect("point record")
        });
        let inventory = SketchInventory {
            sketches: vec![Located::new(
                sketch,
                fixture_record_type_id(SKETCH_TYPE),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                0,
            )],
            entities: vec![Located::new(
                point,
                fixture_record_type_id(POINT_TYPE),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                1,
            )],
            transforms: Vec::new(),
            directions: Vec::new(),
            constraints: Vec::new(),
            issues: Vec::new(),
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        assert!(matches!(
            project(&ctx, &inventory, &[]),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "project Inventor sketch entity"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert!(project(&ctx, &inventory, &[]).is_ok());
    }

    #[test]
    fn sketch_reference_cannot_name_the_maximum_record_ordinal() {
        let mut sketch_bytes = content(0);
        sketch_bytes.extend_from_slice(&0_i32.to_le_bytes());
        sketch_bytes.extend_from_slice(&1_u32.to_le_bytes());
        sketch_bytes.extend(list(8, &[u32::MAX]));
        sketch_bytes.extend_from_slice(&0_u32.to_le_bytes());
        sketch_bytes.extend_from_slice(&0_u32.to_le_bytes());
        sketch_bytes.extend_from_slice(&[0; 8]);
        sketch_bytes.extend(list(2, &[]));
        let sketch = parse(&sketch_bytes, |ctx, source| {
            parse_sketch(ctx, source, 22).expect("sketch record")
        });
        let point = parse(&point_bytes(1, 1, [0.0, 0.0]), |ctx, source| {
            parse_entity(ctx, SketchEntityTag::Point, source, 22).expect("point record")
        });
        let inventory = SketchInventory {
            sketches: vec![Located::new(
                sketch,
                fixture_record_type_id(SKETCH_TYPE),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                0,
            )],
            entities: vec![Located::new(
                point,
                fixture_record_type_id(POINT_TYPE),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                u32::MAX,
            )],
            transforms: Vec::new(),
            directions: Vec::new(),
            constraints: Vec::new(),
            issues: Vec::new(),
        };
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        let projection = project(&ctx, &inventory, &[]).expect("sketch projection");
        assert_eq!(projection.unresolved_entities, 1);
    }

    #[test]
    fn sketch_projection_refuses_entity_limit_before_sketch_creation() {
        let mut transform = parse(
            &{
                let mut bytes = content(0);
                bytes.extend_from_slice(&0x8421_u16.to_le_bytes());
                bytes.extend_from_slice(&0x7bde_u16.to_le_bytes());
                bytes
            },
            |ctx, source| parse_transform(ctx, source, 22).expect("transform"),
        );
        transform.header.source_index = 0;
        let direction = parse(
            &{
                let mut bytes = content(1);
                bytes.extend_from_slice(&0_u32.to_le_bytes());
                bytes.extend_from_slice(&0.0_f64.to_le_bytes());
                bytes.extend_from_slice(&0.0_f64.to_le_bytes());
                bytes.extend_from_slice(&0.0_f64.to_le_bytes());
                bytes.extend_from_slice(&1.0_f64.to_le_bytes());
                bytes
            },
            |ctx, source| parse_direction(ctx, source, 22).expect("direction"),
        );
        let mut sketch_bytes = content(2);
        sketch_bytes.extend_from_slice(&0_i32.to_le_bytes());
        sketch_bytes.extend_from_slice(&0_u32.to_le_bytes());
        sketch_bytes.extend(list(8, &[]));
        sketch_bytes.extend_from_slice(&1_u32.to_le_bytes());
        sketch_bytes.extend_from_slice(&2_u32.to_le_bytes());
        sketch_bytes.extend_from_slice(&[0; 8]);
        sketch_bytes.extend(list(2, &[]));
        let sketch = parse(&sketch_bytes, |ctx, source| {
            parse_sketch(ctx, source, 22).expect("sketch")
        });
        let inventory = SketchInventory {
            sketches: vec![Located::new(
                sketch,
                fixture_record_type_id(SKETCH_TYPE),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                2,
            )],
            entities: Vec::new(),
            transforms: vec![Located::new(
                transform,
                fixture_record_type_id(TRANSFORM_TYPE),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                0,
            )],
            directions: vec![Located::new(
                direction,
                fixture_record_type_id(DIRECTION_TYPE),
                cadmpeg_ir::identity_key!("segment")
                    .try_clone_for_decode(
                        &cadmpeg_test_support::service_decode_context(),
                        "Inventor located fixture token",
                    )
                    .expect("service fixture token"),
                1,
            )],
            constraints: Vec::new(),
            issues: Vec::new(),
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        assert!(matches!(
            project(&ctx, &inventory, &[]),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "project Inventor sketch"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert_eq!(
            project(&ctx, &inventory, &[])
                .expect("service sketch projection")
                .sketches
                .len(),
            1
        );
    }

    #[test]
    fn sketch_constraint_projection_refuses_retained_limit_before_entity_copy() {
        let mut bytes = constraint_header(0, 0);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.push(1);
        let payload = parse(&bytes, |ctx, source| {
            parse_constraint(ctx, SketchConstraintTag::Horizontal, source, 22)
                .expect("horizontal constraint")
        });
        let constraint = Located::new(
            payload,
            fixture_record_type_id(HORIZONTAL_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            0,
        );
        let entity = cadmpeg_ir::sketches::SketchEntity::new(
            cadmpeg_ir::sketches::SketchEntityId::mint("inventor:test:entity#1")
                .expect("entity id"),
            cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1").expect("sketch id"),
            cadmpeg_ir::sketches::SketchGeometry::try_from(
                cadmpeg_ir::sketches::SketchGeometryDefinition::Point {
                    position: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                },
            )
            .expect("point geometry"),
        );
        let entities = std::collections::HashMap::from([(("segment", 0), &entity)]);
        let parameters = std::collections::HashMap::new();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes =
            cadmpeg_core::decode::u64_from_index(entity.id().as_str().len()) - 1;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        assert!(matches!(
            super::project_constraint(&ctx, &constraint, &entities, &parameters),
            Some(Err(CodecError::ResourceLimit(limit)))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor sketch constraint entity id"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert!(
            super::project_constraint(&ctx, &constraint, &entities, &parameters)
                .expect("horizontal constraint projects")
                .is_ok()
        );
    }

    #[test]
    fn sketch_constraint_projection_refuses_entity_limit_before_creation() {
        let mut bytes = constraint_header(0, 0);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.push(1);
        let payload = parse(&bytes, |ctx, source| {
            parse_constraint(ctx, SketchConstraintTag::Horizontal, source, 22)
                .expect("horizontal constraint")
        });
        let constraint = Located::new(
            payload,
            fixture_record_type_id(HORIZONTAL_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            0,
        );
        let entity = cadmpeg_ir::sketches::SketchEntity::new(
            cadmpeg_ir::sketches::SketchEntityId::mint("inventor:test:entity#1")
                .expect("entity id"),
            cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1").expect("sketch id"),
            cadmpeg_ir::sketches::SketchGeometry::try_from(
                cadmpeg_ir::sketches::SketchGeometryDefinition::Point {
                    position: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                },
            )
            .expect("point geometry"),
        );
        let entities = std::collections::HashMap::from([(("segment", 0), &entity)]);
        let parameters = std::collections::HashMap::new();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("projection context");
        assert!(matches!(
            super::project_constraint(&ctx, &constraint, &entities, &parameters),
            Some(Err(CodecError::ResourceLimit(limit)))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "project Inventor sketch constraint"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service projection context");
        assert!(matches!(
            super::project_constraint(&ctx, &constraint, &entities, &parameters),
            Some(Ok(_))
        ));
    }

    fn inventory_with_record(
        type_id: [u8; 16],
        payload: &[u8],
        policy: DecodePolicy,
    ) -> Result<SketchInventory, CodecError> {
        let bytes = primary_envelope_fixture();
        let payload = payload.to_vec();
        let arena = DecodeArena::new();
        let (setup_ctx, source) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("envelope view");
        let mut container = InventorContainer::open(&setup_ctx, source).expect("framed envelope");
        let segment = &mut container.rse.segments[0];
        segment.kind = SegmentKind::PmDc;
        let SegmentBulkState::Framed(bulk) = &mut segment.bulk else {
            panic!("framed bulk fixture");
        };
        let RecordFrameState::Framed(table) = &mut bulk.records else {
            panic!("framed record fixture");
        };
        table.records[0].type_id = type_id;
        table.records[0].payload = View::over_retained(&payload);
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("input view");
        inventory(&ctx, &container.rse)
    }

    #[test]
    fn sketch_transform_record_refuses_collection_limit_before_push() {
        let mut payload = content(0);
        payload.extend_from_slice(&0x8421u16.to_le_bytes());
        payload.extend_from_slice(&0x7bdeu16.to_le_bytes());
        assert_eq!(
            inventory_with_record(TRANSFORM_TYPE, &payload, DecodePolicy::service())
                .expect("transform is admitted")
                .transforms
                .len(),
            1
        );
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        assert!(matches!(
            inventory_with_record(TRANSFORM_TYPE, &payload, policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "admit Inventor PmDc transform record"
                    && limit.used == 0
        ));
    }

    #[test]
    fn sketch_transform_record_refuses_retained_limit_before_identity_copy() {
        let mut payload = content(0);
        payload.extend_from_slice(&0x8421u16.to_le_bytes());
        payload.extend_from_slice(&0x7bdeu16.to_le_bytes());
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 31;
        assert!(matches!(
            inventory_with_record(TRANSFORM_TYPE, &payload, policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor PmDc record type id"
                    && limit.used == 0
        ));
        policy.limits.max_retained_bytes = 32;
        assert!(matches!(
            inventory_with_record(TRANSFORM_TYPE, &payload, policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor PmDc record segment token"
                    && limit.used == 32
        ));
    }

    #[test]
    fn sketch_parse_issue_refuses_collection_limit_before_push() {
        let admitted = inventory_with_record(TRANSFORM_TYPE, &[], DecodePolicy::service())
            .expect("truncated transform becomes an issue");
        assert_eq!(admitted.issues.len(), 1);
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        assert!(matches!(
            inventory_with_record(TRANSFORM_TYPE, &[], policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "admit Inventor PmDc sketch issue"
                    && limit.used == 0
        ));
    }

    #[test]
    fn sketch_parse_issue_refuses_entity_limit_before_push() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_entities = 0;
        assert!(matches!(
            inventory_with_record(TRANSFORM_TYPE, &[], policy),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "admit Inventor PmDc sketch issue"
        ));
        assert_eq!(
            inventory_with_record(TRANSFORM_TYPE, &[], DecodePolicy::service())
                .expect("service issue")
                .issues
                .len(),
            1
        );
    }

    #[test]
    fn sketch_parse_issue_copies_refuse_retained_limits_before_creation() {
        let issue = inventory_with_record(TRANSFORM_TYPE, &[], DecodePolicy::service())
            .expect("truncated transform becomes an issue")
            .issues
            .remove(0);
        let detail_len = issue.detail.len();
        let token_len = issue.segment_token.as_str().len();
        let type_id_len = 32;
        // Retained bytes count the owned type id, segment token, detail, and four issue slots.
        for (limit_bytes, operation, used) in [
            (
                type_id_len - 1,
                "retain Inventor PmDc sketch issue type id",
                0,
            ),
            (
                type_id_len + token_len - 1,
                "retain Inventor PmDc sketch issue segment token",
                type_id_len,
            ),
            (
                type_id_len + token_len + detail_len - 1,
                "retain Inventor PmDc sketch issue detail",
                type_id_len + token_len,
            ),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(limit_bytes);
            assert!(matches!(
                inventory_with_record(TRANSFORM_TYPE, &[], policy),
                Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                        && limit.operation == operation
                        && limit.used == cadmpeg_core::decode::u64_from_index(used)
            ));
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(
            type_id_len
                + token_len
                + detail_len
                + 4 * std::mem::size_of::<crate::record_issue::RecordIssue>(),
        );
        assert_eq!(
            inventory_with_record(TRANSFORM_TYPE, &[], policy)
                .expect("type id, token, and detail fit their retained allowance")
                .issues
                .len(),
            1
        );
    }

    #[test]
    fn sketch_record_forms_refuse_collection_limit_before_push() {
        let mut sketch = content(0);
        sketch.extend_from_slice(&0u32.to_le_bytes());
        sketch.extend_from_slice(&0u32.to_le_bytes());
        sketch.extend(list(8, &[]));
        sketch.extend_from_slice(&[0; 16]);
        let mut direction = content(0);
        direction.extend_from_slice(&[0; 36]);
        let mut constraint = constraint_header(0, 0);
        constraint.extend_from_slice(&0u32.to_le_bytes());
        constraint.push(1);
        for (type_id, payload, operation) in [
            (SKETCH_TYPE, sketch, "admit Inventor PmDc sketch record"),
            (
                POINT_TYPE,
                point_bytes(0, 0, [0.0, 0.0]),
                "admit Inventor PmDc sketch entity record",
            ),
            (
                DIRECTION_TYPE,
                direction,
                "admit Inventor PmDc direction record",
            ),
            (
                HORIZONTAL_TYPE,
                constraint,
                "admit Inventor PmDc sketch constraint record",
            ),
        ] {
            let admitted = inventory_with_record(type_id, &payload, DecodePolicy::service())
                .expect("sketch record is admitted");
            assert_eq!(
                admitted.sketches.len()
                    + admitted.entities.len()
                    + admitted.transforms.len()
                    + admitted.directions.len()
                    + admitted.constraints.len(),
                1
            );
            assert!(admitted.issues.is_empty());
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = 0;
            assert!(matches!(
                inventory_with_record(type_id, &payload, policy),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::CollectionItems
                        && limit.operation == operation
                        && limit.used == 0
            ));
        }
    }

    fn list(marker: u16, references: &[u32]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&marker.to_le_bytes());
        bytes.extend_from_slice(&0x3000u16.to_le_bytes());
        bytes.extend_from_slice(
            &(u32::try_from(references.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
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
                position,
                ..
            } if position.map(FiniteReal::get) == [1.25, -2.5]
        ));

        let line = line_bytes(2, 3, [4, 5]);
        let parsed = parse(&line, |ctx, source| {
            parse_entity(ctx, SketchEntityTag::Line, source, 22).expect("line")
        });
        assert!(matches!(
            parsed.kind,
            PmDcSketchEntityKind::Line { ref points, .. }
                if points.references().iter().map(|value| value.index()).collect::<Vec<_>>() == [4, 5]
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
            PmDcSketchEntityKind::Circle { radius, .. } if radius.get() == 2.5
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
                major_radius,
                minor_radius,
                ..
            } if major_radius.get() == 3.0 && minor_radius.get() == 2.0
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
                    bytes.extend_from_slice(
                        &(u16::try_from(value).expect("fixture value fits u16")).to_le_bytes(),
                    );
                } else {
                    bytes.extend_from_slice(
                        &(u32::try_from(value).expect("fixture value fits u32")).to_le_bytes(),
                    );
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
                if first.index() == 4 && second.index() == 5
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
            |ctx, source| parse_transform(ctx, source, 22).expect("transform"),
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
            |ctx, source| parse_direction(ctx, source, 22).expect("direction"),
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
                    &point_bytes(
                        u32::try_from(index).expect("fixture value fits u32") + 3,
                        3,
                        position,
                    ),
                    |ctx, source| {
                        parse_entity(ctx, SketchEntityTag::Point, source, 22).expect("point")
                    },
                )
            })
            .collect::<Vec<_>>();
        for (index, endpoints) in [[4, 5], [5, 6], [6, 7], [7, 4]].into_iter().enumerate() {
            let mut line = parse(
                &line_bytes(
                    u32::try_from(index).expect("fixture value fits u32") + 7,
                    3,
                    endpoints,
                ),
                |ctx, source| parse_entity(ctx, SketchEntityTag::Line, source, 22).expect("line"),
            );
            let start = points[cadmpeg_core::decode::index_from_u32(endpoints[0]) - 4];
            let end = points[cadmpeg_core::decode::index_from_u32(endpoints[1]) - 4];
            let PmDcSketchEntityKind::Line {
                origin, direction, ..
            } = &mut line.kind
            else {
                unreachable!("generated line")
            };
            *origin = start.map(real);
            *direction = [real(end[0] - start[0]), real(end[1] - start[1])];
            entities.push(line);
        }
        transform.header.source_index = 0;
        let transform = Located::new(
            transform,
            fixture_record_type_id(TRANSFORM_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            0,
        );
        let direction = Located::new(
            direction,
            fixture_record_type_id(DIRECTION_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            1,
        );
        let located_sketch = Located::new(
            sketch.clone(),
            fixture_record_type_id(SKETCH_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            2,
        );
        let entities = entities
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let type_id = if index < 4 { POINT_TYPE } else { LINE_TYPE };
                Located::new(
                    value,
                    fixture_record_type_id(type_id),
                    cadmpeg_ir::identity_key!("segment")
                        .try_clone_for_decode(
                            &cadmpeg_test_support::service_decode_context(),
                            "Inventor located fixture token",
                        )
                        .expect("service fixture token"),
                    u32::try_from(index).expect("fixture value fits u32") + 3,
                )
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
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        let projected = project(&ctx, &inventory, &[]).expect("sketch projection");
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
            fixture_record_type_id(COINCIDENT_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            11,
        ));
        let mut sketch = sketch;
        let (marker, metadata, mut references) = sketch.entities.clone().into_parts();
        references.push(PmDcReference::new(12, false).expect("test reference index fits 31 bits"));
        sketch.entities =
            PmDcReferenceList::new(marker, metadata, references).expect("extended entity list");
        inventory.sketches[0] = Located::new(
            sketch,
            fixture_record_type_id(SKETCH_TYPE),
            cadmpeg_ir::identity_key!("segment")
                .try_clone_for_decode(
                    &cadmpeg_test_support::service_decode_context(),
                    "Inventor located fixture token",
                )
                .expect("service fixture token"),
            2,
        );
        let incomplete = project(&ctx, &inventory, &[]).expect("incomplete sketch projection");
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
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut bytes = content(1);
        bytes.extend_from_slice(&0x8421u16.to_le_bytes());
        bytes.extend_from_slice(&0x7bdeu16.to_le_bytes());
        let transform = parse(&bytes, |ctx, source| {
            parse_transform(ctx, source, 22).expect("constant-prefix transform fixture is valid")
        });
        let mut wire =
            serde_json::to_value(transform).expect("constant-prefix transform fixture is valid");
        for (prefix, present) in [
            (serde_json::Value::Null, false),
            (serde_json::json!(515), true),
        ] {
            wire["prefix"] = prefix;
            let parsed: PmDcTransformPayload =
                serde_json::from_value::<PmDcTransformPayloadWire>(wire.clone())
                    .expect("constant-prefix transform fixture is valid")
                    .into_payload(&ctx)
                    .expect("constant-prefix transform fixture is valid");
            assert_eq!(parsed.prefix_present, present);
            assert_eq!(
                serde_json::to_value(parsed).expect("constant-prefix transform fixture is valid"),
                wire
            );
        }
        wire["prefix"] = serde_json::json!(516);
        assert!(
            serde_json::from_value::<PmDcTransformPayloadWire>(wire.clone())
                .expect("transform wire fixture is valid")
                .into_payload(&ctx)
                .is_err()
        );
        wire.as_object_mut()
            .expect("constant-prefix transform fixture is valid")
            .remove("prefix");
        assert!(
            !serde_json::from_value::<PmDcTransformPayloadWire>(wire)
                .expect("constant-prefix transform fixture is valid")
                .into_payload(&ctx)
                .expect("constant-prefix transform fixture is valid")
                .prefix_present
        );
    }
    #[test]
    fn large_collinear_sketch_carrier_matches() {
        assert!(super::line_carrier_matches(
            [0., 0.],
            [1e200, 1e200],
            [1e200, 1e200],
            [2e200, 2e200]
        ));
        assert!(!super::line_carrier_matches(
            [0., 0.],
            [1e200, 0.],
            [0., 0.],
            [0., 1e200]
        ));
    }

    #[test]
    fn sketch_carrier_agreement_is_independent_of_scale() {
        for scale in [f64::from_bits(1), 5e-11, 1.0, 1e200] {
            assert!(super::line_carrier_matches(
                [0.0, 0.0],
                [scale, 0.0],
                [scale, 0.0],
                [2.0 * scale, 0.0],
            ));
            assert!(super::line_carrier_matches(
                [0.0, 0.0],
                [scale, 0.0],
                [0.0, 0.0],
                [scale, 0.0],
            ));
            assert!(!super::line_carrier_matches(
                [0.0, 0.0],
                [scale, 0.0],
                [0.0, scale],
                [scale, scale],
            ));
        }
        for invalid in [0.0, f64::NAN, f64::INFINITY] {
            assert!(!super::line_carrier_matches(
                [0.0, 0.0],
                [invalid, 0.0],
                [0.0, 0.0],
                [1.0, 0.0],
            ));
        }
    }
    #[test]
    fn audit_regression_distant_parallel_segment_is_not_on_carrier() {
        for origin in [[0., 0.], [1e12, 0.]] {
            assert!(!super::line_carrier_matches(
                origin,
                [1., 0.],
                [1e12, 1.],
                [1e12 + 1., 1.]
            ));
            assert!(super::line_carrier_matches(
                origin,
                [1., 0.],
                [1e12, 0.],
                [1e12 + 1., 0.]
            ));
        }
    }
    #[test]
    fn audit_regression_distant_oblique_point_keeps_stored_direction_ratio() {
        assert!(super::line_carrier_matches(
            [0., 0.],
            [1., 12.],
            [1e12, 12e12],
            [1e12 + 1., 12e12 + 12.]
        ));
        assert!(!super::line_carrier_matches(
            [0., 0.],
            [1., 12.],
            [1e12, 12e12 + 1.],
            [1e12 + 1., 12e12 + 13.]
        ));
    }

    mod serialization;
}
