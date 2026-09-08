// SPDX-License-Identifier: Apache-2.0
//! Typed planar-sketch records and closed neutral sketch graphs.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{Angle, DesignParameter, Length, ParameterId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    NativeOperandField, Sketch, SketchConstraint, SketchConstraintDefinitionInput,
    SketchConstraintId, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId, SketchLocus, SketchNativeOperand, SketchPlacement,
};
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "PmDcReferenceScalarMapWire",
    into = "PmDcReferenceScalarMapWire"
)]
pub(crate) struct PmDcReferenceScalarMap {
    // Metadata belongs to the nonempty sequence of reference/value pairs.
    #[allow(clippy::type_complexity)]
    items: Option<([u32; 2], Vec<(PmDcReference, f64)>)>,
}

#[derive(Serialize, Deserialize)]
struct PmDcReferenceScalarMapWire {
    metadata: Option<[u32; 2]>,
    entries: Vec<(PmDcReference, f64)>,
}

impl PmDcReferenceScalarMap {
    fn new(metadata: Option<[u32; 2]>, entries: Vec<(PmDcReference, f64)>) -> Option<Self> {
        Some(Self {
            items: crate::pmdc::paired_items(metadata, entries)?,
        })
    }

    pub(crate) fn metadata(&self) -> Option<[u32; 2]> {
        self.items.as_ref().map(|(metadata, _)| *metadata)
    }

    pub(crate) fn entries(&self) -> &[(PmDcReference, f64)] {
        self.items
            .as_ref()
            .map_or(&[] as &[_], |(_, entries)| entries.as_slice())
    }
}

impl From<PmDcReferenceScalarMap> for PmDcReferenceScalarMapWire {
    fn from(value: PmDcReferenceScalarMap) -> Self {
        match value.items {
            None => Self {
                metadata: None,
                entries: Vec::new(),
            },
            Some((metadata, entries)) => Self {
                metadata: Some(metadata),
                entries,
            },
        }
    }
}

impl TryFrom<PmDcReferenceScalarMapWire> for PmDcReferenceScalarMap {
    type Error = String;

    fn try_from(wire: PmDcReferenceScalarMapWire) -> Result<Self, Self::Error> {
        Self::new(wire.metadata, wire.entries)
            .ok_or_else(|| "PmDc scalar map metadata disagrees with length".to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "PmDcReferencePairMapWire",
    into = "PmDcReferencePairMapWire"
)]
pub(crate) struct PmDcReferencePairMap {
    // Metadata belongs to the nonempty sequence of ordered reference pairs.
    #[allow(clippy::type_complexity)]
    items: Option<([u32; 2], Vec<(PmDcReference, PmDcReference)>)>,
}

#[derive(Serialize, Deserialize)]
struct PmDcReferencePairMapWire {
    metadata: Option<[u32; 2]>,
    entries: Vec<(PmDcReference, PmDcReference)>,
}

impl PmDcReferencePairMap {
    fn new(
        metadata: Option<[u32; 2]>,
        entries: Vec<(PmDcReference, PmDcReference)>,
    ) -> Option<Self> {
        Some(Self {
            items: crate::pmdc::paired_items(metadata, entries)?,
        })
    }

    pub(crate) fn metadata(&self) -> Option<[u32; 2]> {
        self.items.as_ref().map(|(metadata, _)| *metadata)
    }

    pub(crate) fn entries(&self) -> &[(PmDcReference, PmDcReference)] {
        self.items
            .as_ref()
            .map_or(&[] as &[_], |(_, entries)| entries.as_slice())
    }
}

impl From<PmDcReferencePairMap> for PmDcReferencePairMapWire {
    fn from(value: PmDcReferencePairMap) -> Self {
        match value.items {
            None => Self {
                metadata: None,
                entries: Vec::new(),
            },
            Some((metadata, entries)) => Self {
                metadata: Some(metadata),
                entries,
            },
        }
    }
}

impl TryFrom<PmDcReferencePairMapWire> for PmDcReferencePairMap {
    type Error = String;

    fn try_from(wire: PmDcReferencePairMapWire) -> Result<Self, Self::Error> {
        Self::new(wire.metadata, wire.entries)
            .ok_or_else(|| "PmDc pair map metadata disagrees with length".to_owned())
    }
}

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
        state: Option<u32>,
        associations: Option<PmDcReferenceList>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmDcTransformPayload {
    pub(crate) save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    pub(crate) prefix: Option<u32>,
    pub(crate) value_mask: u16,
    pub(crate) zero_mask: u16,
    pub(crate) matrix: [[f64; 4]; 4],
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
            let result = match record.type_id {
                SKETCH_TYPE => parse_sketch(ctx, record.payload, version).map(|value| {
                    inventory.sketches.push(Located::new(
                        value,
                        type_id_string(record.type_id),
                        segment.pair.token.as_str(),
                        record.ordinal,
                    ));
                }),
                POINT_TYPE | LINE_TYPE | CIRCLE_TYPE | ELLIPSE_TYPE => {
                    parse_entity(ctx, record.type_id, record.payload, version).map(|value| {
                        inventory.entities.push(Located::new(
                            value,
                            type_id_string(record.type_id),
                            segment.pair.token.as_str(),
                            record.ordinal,
                        ));
                    })
                }
                TRANSFORM_TYPE => parse_transform(record.payload, version).map(|value| {
                    inventory.transforms.push(Located::new(
                        value,
                        type_id_string(record.type_id),
                        segment.pair.token.as_str(),
                        record.ordinal,
                    ));
                }),
                DIRECTION_TYPE => parse_direction(record.payload, version).map(|value| {
                    inventory.directions.push(Located::new(
                        value,
                        type_id_string(record.type_id),
                        segment.pair.token.as_str(),
                        record.ordinal,
                    ));
                }),
                COINCIDENT_TYPE
                | PARALLEL_TYPE
                | PERPENDICULAR_TYPE
                | TANGENT_TYPE
                | HORIZONTAL_TYPE
                | VERTICAL_TYPE
                | HORIZONTAL_DISTANCE_TYPE
                | VERTICAL_DISTANCE_TYPE
                | RADIUS_TYPE
                | DIAMETER_TYPE
                | CIRCLE_CENTER_TYPE
                | EQUAL_RADIUS_TYPE => {
                    parse_constraint(ctx, record.type_id, record.payload, version).map(|value| {
                        inventory.constraints.push(Located::new(
                            value,
                            type_id_string(record.type_id),
                            segment.pair.token.as_str(),
                            record.ordinal,
                        ));
                    })
                }
                _ => continue,
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
    let state = cursor.u32("sketch state")? as i32;
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

fn parse_entity(
    ctx: &DecodeContext<'_>,
    type_id: [u8; 16],
    source: View<'_>,
    version: u8,
) -> Result<PmDcSketchEntityPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let entity_flags = cursor.u32("sketch-entity flags")?;
    let sketch = cursor.reference("sketch-entity owner")?;
    let kind = match type_id {
        POINT_TYPE => parse_point(ctx, &mut cursor)?,
        LINE_TYPE => parse_line(ctx, &mut cursor)?,
        CIRCLE_TYPE => parse_circle(ctx, &mut cursor)?,
        ELLIPSE_TYPE => parse_ellipse(ctx, &mut cursor)?,
        _ => unreachable!("caller selects a supported sketch entity"),
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
    let (state, associations) = if cursor.remaining() == 0 {
        (None, None)
    } else {
        (
            Some(cursor.u32("point state")?),
            Some(reference_list(ctx, cursor, 2, "point association list")?),
        )
    };
    Ok(PmDcSketchEntityKind::Point {
        position,
        endpoint_of,
        center_of,
        state,
        associations,
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
    if cursor.remaining() >= fixed_tail.saturating_add(8)
        && cursor.peek_u32(&format!("{field} auxiliary marker"))? == 0x3000_0002
    {
        auxiliary.push(reference_list(
            ctx,
            cursor,
            2,
            &format!("{field} auxiliary list 0"),
        )?);
    } else if cursor.remaining() >= fixed_tail.saturating_add(16) {
        let gate = [
            cursor.u32(&format!("{field} list gate 0"))?,
            cursor.u32(&format!("{field} list gate 1"))?,
        ];
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
        if cursor.remaining() >= fixed_tail.saturating_add(8)
            && cursor.peek_u32(&format!("{field} second auxiliary marker"))? == 0x3000_0002
        {
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
    let center = cursor.reference("circle center")?;
    let radius = cursor.f64("circle radius")?;
    let state = cursor.u8("circle state")?;
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
    let center = cursor.reference("ellipse center")?;
    let major_direction = point2(cursor, "ellipse major direction")?;
    let major_radius = cursor.f64("ellipse major radius")?;
    let minor_radius = cursor.f64("ellipse minor radius")?;
    let state = cursor.u8("ellipse state")?;
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
    let prefix = if cursor.peek_u32("transform prefix")? == 0x203 {
        Some(cursor.u32("transform prefix")?)
    } else {
        None
    };
    let value_mask = cursor.u16("transform value mask")?;
    let zero_mask = cursor.u16("transform zero mask")?;
    let mut matrix = [[0.0; 4]; 4];
    for (row, values) in matrix.iter_mut().enumerate() {
        for (column, value) in values.iter_mut().enumerate() {
            let bit = 1u16 << (column + 4 * row);
            *value = if zero_mask & bit == 0 {
                if value_mask & bit == 0 {
                    cursor.f64("transform explicit value")?
                } else {
                    1.0
                }
            } else if value_mask & bit == 0 {
                0.0
            } else {
                -1.0
            };
        }
    }
    cursor.finish("transform")?;
    Ok(PmDcTransformPayload {
        save_version_major: version,
        header,
        prefix,
        value_mask,
        zero_mask,
        matrix,
    })
}

fn parse_direction(source: View<'_>, version: u8) -> Result<PmDcDirectionPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor)?;
    let entity_flags = cursor.u32("direction flags")?;
    let parameter = cursor.f64("direction parameter")?;
    let extension = match cursor.remaining() {
        24 => None,
        28 => Some(cursor.u32("direction extension")?),
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
    let marker = [
        cursor.u16(&format!("{field} marker kind"))?,
        cursor.u16(&format!("{field} marker form"))?,
    ];
    if marker != [6, 0x3000] {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc {field} marker is {marker:?}"
        )));
    }
    let count = cursor.u32(&format!("{field} count"))? as usize;
    ctx.charge_collection_items(count as u64, "admit Inventor sketch constraint map")?;
    let metadata = (count != 0)
        .then(|| {
            Ok::<_, CodecError>([
                cursor.u32(&format!("{field} metadata 0"))?,
                cursor.u32(&format!("{field} metadata 1"))?,
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
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        entries.push((
            cursor.reference(&format!("constraint scalar-map key {index}"))?,
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
    for index in 0..count {
        entries.push((
            cursor.reference(&format!("constraint reference-map key {index}"))?,
            cursor.reference(&format!("constraint reference-map value {index}"))?,
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
    let state = cursor.u32("constraint state")? as i32;
    let group = cursor.reference("constraint group")?;
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
        parameter: cursor.reference("constraint parameter")?,
    })
}

fn parse_constraint(
    ctx: &DecodeContext<'_>,
    type_id: [u8; 16],
    source: View<'_>,
    version: u8,
) -> Result<PmDcSketchConstraintPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = parse_constraint_header(ctx, &mut cursor, version)?;
    let kind = match type_id {
        COINCIDENT_TYPE => PmDcSketchConstraintKind::Coincident {
            first: cursor.reference("coincident first entity")?,
            second: cursor.reference("coincident second entity")?,
        },
        PARALLEL_TYPE => PmDcSketchConstraintKind::Parallel {
            first: cursor.reference("parallel first entity")?,
            second: cursor.reference("parallel second entity")?,
            orientation: cursor.u16("parallel orientation")?,
        },
        PERPENDICULAR_TYPE => PmDcSketchConstraintKind::Perpendicular {
            first: cursor.reference("perpendicular first entity")?,
            second: cursor.reference("perpendicular second entity")?,
            orientation: cursor.u16("perpendicular orientation")?,
        },
        TANGENT_TYPE => PmDcSketchConstraintKind::Tangent {
            first: cursor.reference("tangent first entity")?,
            second: cursor.reference("tangent second entity")?,
            extension: (cursor.remaining() == 4)
                .then(|| cursor.u32("tangent extension"))
                .transpose()?,
        },
        HORIZONTAL_TYPE => PmDcSketchConstraintKind::Horizontal {
            entity: cursor.reference("horizontal entity")?,
            state: cursor.u8("horizontal state")?,
        },
        VERTICAL_TYPE => PmDcSketchConstraintKind::Vertical {
            entity: cursor.reference("vertical entity")?,
            state: cursor.u8("vertical state")?,
        },
        HORIZONTAL_DISTANCE_TYPE | VERTICAL_DISTANCE_TYPE => {
            let first = cursor.reference("distance first entity")?;
            let second = cursor.reference("distance second entity")?;
            let parameter = cursor.reference("distance parameter")?;
            let values = u32_array::<4>(&mut cursor, "distance values")?;
            if type_id == HORIZONTAL_DISTANCE_TYPE {
                PmDcSketchConstraintKind::HorizontalDistance {
                    first,
                    second,
                    parameter,
                    values,
                }
            } else {
                PmDcSketchConstraintKind::VerticalDistance {
                    first,
                    second,
                    parameter,
                    values,
                }
            }
        }
        RADIUS_TYPE => PmDcSketchConstraintKind::Radius {
            state: cursor.u32("radius state")?,
            entity: cursor.reference("radius entity")?,
            values: u32_array::<4>(&mut cursor, "radius values")?,
        },
        DIAMETER_TYPE => PmDcSketchConstraintKind::Diameter {
            reference: cursor.reference("diameter reference")?,
            entity: cursor.reference("diameter entity")?,
            values: u32_array::<4>(&mut cursor, "diameter values")?,
        },
        CIRCLE_CENTER_TYPE => PmDcSketchConstraintKind::CircleCenter {
            entity: cursor.reference("circle-center entity")?,
            center: cursor.reference("circle-center point")?,
        },
        EQUAL_RADIUS_TYPE => PmDcSketchConstraintKind::EqualRadius {
            first: cursor.reference("equal-radius first entity")?,
            second: cursor.reference("equal-radius second entity")?,
        },
        _ => unreachable!("caller selects a supported sketch constraint"),
    };
    cursor.finish("sketch constraint")?;
    Ok(PmDcSketchConstraintPayload {
        save_version_major: version,
        header,
        kind,
    })
}

fn u32_array<const N: usize>(cursor: &mut Cursor<'_>, field: &str) -> Result<[u32; N], CodecError> {
    let mut values = [0; N];
    for (index, value) in values.iter_mut().enumerate() {
        *value = cursor.u32(&format!("{field} {index}"))?;
    }
    Ok(values)
}

pub(crate) fn project(
    inventory: &SketchInventory,
    parameters: &[DesignParameter],
) -> SketchProjection {
    let raw_sketches = unique(&inventory.sketches, |record| {
        (
            &record.identity.segment_token,
            record.identity.record_ordinal,
        )
    });
    let raw_entities = unique(&inventory.entities, |record| {
        (
            &record.identity.segment_token,
            record.identity.record_ordinal,
        )
    });
    let transforms = unique(&inventory.transforms, |record| {
        (
            &record.identity.segment_token,
            record.identity.record_ordinal,
        )
    });
    let directions = unique(&inventory.directions, |record| {
        (
            &record.identity.segment_token,
            record.identity.record_ordinal,
        )
    });
    let raw_constraints = unique(&inventory.constraints, |record| {
        (
            &record.identity.segment_token,
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
            entity.identity.segment_token.clone(),
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
            raw_sketches.get(&(entity.identity.segment_token.clone(), sketch_ordinal))
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
            sketch.identity.segment_token.clone(),
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
                    .get(&(sketch.identity.segment_token.clone(), ordinal))
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
                        raw.identity.segment_token.clone(),
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
                constraint.identity.segment_token.clone(),
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
            let key = (raw.identity.segment_token.clone(), ordinal);
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
    entities: &HashMap<(String, u32), &SketchEntity>,
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
                constraint.identity.segment_token.clone(),
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
                    native_kind: "circle_center_alignment".into(),
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
        native_kind: cadmpeg_ir::products::NonEmptyString::new("record_reference")
            .expect("source operand kind is nonempty"),
        field: Some(NativeOperandField {
            name: cadmpeg_ir::products::NonEmptyString::new(field)
                .expect("source field name is nonempty"),
            role: None,
        }),
        object_index: reference.index,
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
    entities: &HashMap<(String, u32), &PmDcSketchEntity>,
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
                    radius: Length(radius * 10.0),
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
                    major_angle: Angle(major_direction[1].atan2(major_direction[0])),
                    major_radius: Length(major_radius * 10.0),
                    minor_radius: Length(minor_radius * 10.0),
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
    entities: &HashMap<(String, u32), &PmDcSketchEntity>,
) -> Option<[f64; 2]> {
    let entity = entities.get(&(token.to_string(), reference.checked_sub(1)?))?;
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
    entities: &HashMap<(String, u32), &PmDcSketchEntity>,
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
                    entity.identity.segment_token.clone(),
                    reference.index.checked_sub(1)?,
                ))
                .map(|value| value.id())
        })
        .collect()
}

fn project_placement(
    sketch: &PmDcSketch,
    transforms: &HashMap<(String, u32), &PmDcTransform>,
    directions: &HashMap<(String, u32), &PmDcDirection>,
) -> Option<SketchPlacement> {
    let transform = transforms.get(&(
        sketch.identity.segment_token.clone(),
        sketch.transform.index.checked_sub(1)?,
    ))?;
    let direction = directions.get(&(
        sketch.identity.segment_token.clone(),
        sketch.direction.index.checked_sub(1)?,
    ))?;
    let matrix = transform.matrix;
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
    Some(
        SketchPlacement::try_resolved(
            Point3::new(
                matrix[0][3] * 10.0,
                matrix[1][3] * 10.0,
                matrix[2][3] * 10.0,
            ),
            normal,
            u_axis,
        )
        .ok()?,
    )
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

fn unique<'a, T>(
    values: &'a [T],
    key: impl Fn(&'a T) -> (&'a String, u32),
) -> HashMap<(String, u32), &'a T> {
    let mut counts = HashMap::new();
    for value in values {
        let (token, ordinal) = key(value);
        let entry = counts
            .entry((token.clone(), ordinal))
            .or_insert((value, 0usize));
        entry.1 += 1;
    }
    counts
        .into_iter()
        .filter_map(|(key, (value, count))| (count == 1).then_some((key, value)))
        .collect()
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
            parse_entity(ctx, POINT_TYPE, source, 22).expect("point")
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
            parse_entity(ctx, LINE_TYPE, source, 22).expect("line")
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
            parse_entity(ctx, CIRCLE_TYPE, source, 22).expect("circle")
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
            parse_entity(ctx, ELLIPSE_TYPE, source, 22).expect("ellipse")
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
            (COINCIDENT_TYPE, vec![4, 5], "coincident"),
            (PARALLEL_TYPE, vec![4, 5, 0], "parallel"),
            (PERPENDICULAR_TYPE, vec![4, 5, 0], "perpendicular"),
            (TANGENT_TYPE, vec![4, 5, 0], "tangent"),
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

        for (type_id, expected) in [(HORIZONTAL_TYPE, true), (VERTICAL_TYPE, false)] {
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
            parse_constraint(ctx, COINCIDENT_TYPE, source, 16).expect("legacy coincident")
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
        for type_id in [HORIZONTAL_DISTANCE_TYPE, VERTICAL_DISTANCE_TYPE] {
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
            parse_constraint(ctx, RADIUS_TYPE, source, 22).expect("radius")
        });

        let mut diameter = constraint_header(13, 14);
        diameter.extend_from_slice(&0u32.to_le_bytes());
        diameter.extend_from_slice(&4u32.to_le_bytes());
        diameter.extend_from_slice(&[0; 16]);
        parse(&diameter, |ctx, source| {
            parse_constraint(ctx, DIAMETER_TYPE, source, 22).expect("diameter")
        });

        for type_id in [CIRCLE_CENTER_TYPE, EQUAL_RADIUS_TYPE] {
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
                    |ctx, source| parse_entity(ctx, POINT_TYPE, source, 22).expect("point"),
                )
            })
            .collect::<Vec<_>>();
        for (index, endpoints) in [[4, 5], [5, 6], [6, 7], [7, 4]].into_iter().enumerate() {
            let mut line = parse(
                &line_bytes(index as u32 + 7, 3, endpoints),
                |ctx, source| parse_entity(ctx, LINE_TYPE, source, 22).expect("line"),
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
            parse_constraint(ctx, COINCIDENT_TYPE, source, 22).expect("mapped coincident")
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
}
