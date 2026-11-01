// SPDX-License-Identifier: Apache-2.0
//! Typed planar-sketch records and closed neutral sketch graphs.

use cadmpeg_core::decode::unique_index;
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

use crate::compact_matrix::CompactMatrix;
use crate::pmdc::{
    content_header, inventor_id, reference_list, state_content_header, Cursor, PmDcContentHeader,
    PmDcReference, PmDcReferenceList,
};
use crate::record_identity::{push_record, Located, RecordPayload};
use crate::record_issue::{RecordIssue, RecordIssueFamily};
use crate::rse::{RecordFrameState, RseInventory, SegmentBulkState, SegmentKind};

pub(crate) mod curve;
pub(crate) mod point_tail;

use point_tail::PointPairedAssociation;

const EPS_SKETCH_LINE_CARRIER_MATCHES_E10: f64 = 1.0e-10;
const EPS_SKETCH_PROJECT_PLACEMENT_E10: f64 = 1.0e-10;
/// Largest distance, in internal centimetres, between an arc end point and the
/// stored circle. It equals the kernel linear tolerance, so the projected arc
/// ends meet their point records within the document tolerance.
const EPS_SKETCH_ARC_END_ON_CIRCLE_CM: f64 = 1.0e-6;
/// First save version whose midpoint relation ends with a u32 value.
const MIDPOINT_VALUE_VERSION: u8 = 20;
/// Entity-flag bits that every planar curve form carries on curves of profile
/// wire bodies.
const PROFILE_CURVE_FLAGS: u32 = 0x40 | 0x1000 | 0x4_0000;
/// Additional profile-wire flag bit of circles, ellipses, and splines.
const NON_LINE_PROFILE_FLAG: u32 = 0x8_0000;
/// Additional profile-wire flag bit of lines and circles.
const LINE_CIRCLE_PROFILE_FLAG: u32 = 0x200;
/// Entity-flag bit of a line that a distance dimension measures as an axis:
/// the dimension parameter is twice the distance between the operands.
/// Profile wire bodies also use lines with this bit.
const AXIS_LINE_FLAG: u32 = 0x8000;
/// Additional profile-wire flag bit of splines.
const SPLINE_PROFILE_FLAG: u32 = 0x4;
/// Entity-flag bits that mark a line as construction geometry. No line with
/// one of these bits bounds a profile loop.
const LINE_CONSTRUCTION_FLAGS: u32 = 0x8_0000 | 0x20_0000;

/// Entity-flag bits whose meaning is resolved for the curve form of `entity`:
/// the bits that profile wire bodies carry and the construction bits.
fn resolved_curve_flags(entity: &PmDcSketchEntity) -> u32 {
    match entity.kind {
        PmDcSketchEntityKind::Line { .. } => {
            PROFILE_CURVE_FLAGS
                | LINE_CIRCLE_PROFILE_FLAG
                | AXIS_LINE_FLAG
                | LINE_CONSTRUCTION_FLAGS
        }
        PmDcSketchEntityKind::Circle { .. } => {
            PROFILE_CURVE_FLAGS | NON_LINE_PROFILE_FLAG | LINE_CIRCLE_PROFILE_FLAG
        }
        PmDcSketchEntityKind::Spline { .. } => {
            PROFILE_CURVE_FLAGS | NON_LINE_PROFILE_FLAG | SPLINE_PROFILE_FLAG
        }
        _ => PROFILE_CURVE_FLAGS | NON_LINE_PROFILE_FLAG,
    }
}

/// Whether the record is a line whose entity flags carry a construction bit.
/// The projection and the validator share this predicate.
pub(crate) fn marks_construction(entity: &PmDcSketchEntity) -> bool {
    matches!(entity.kind, PmDcSketchEntityKind::Line { .. })
        && entity.entity_flags & LINE_CONSTRUCTION_FLAGS != 0
}

/// Whether the record is a curve whose entity flags carry a bit with an
/// unresolved construction meaning.
fn has_unresolved_flags(entity: &PmDcSketchEntity) -> bool {
    matches!(
        entity.kind,
        PmDcSketchEntityKind::Line { .. }
            | PmDcSketchEntityKind::Circle { .. }
            | PmDcSketchEntityKind::Ellipse { .. }
            | PmDcSketchEntityKind::Spline { .. }
    ) && entity.entity_flags & !resolved_curve_flags(entity) != 0
}

/// Largest departure of a block-instance placement from a rigid motion.
const EPS_SKETCH_BLOCK_PLACEMENT_RIGID: f64 = 1.0e-9;
const CIRCLE_STATE_ARC: u8 = 0;
const CIRCLE_STATE_FULL: u8 = 1;

pub(crate) mod spline;

const SKETCH_TYPE: [u8; 16] = inventor_id(0x9087_4d11);
pub(crate) const TRANSFORM_TYPE: [u8; 16] = inventor_id(0x9087_4d18);
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
const EQUAL_LENGTH_TYPE: [u8; 16] = [
    0xe0, 0xc2, 0x81, 0xc6, 0xd1, 0x11, 0x10, 0xe6, 0x80, 0x00, 0x6e, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const EQUAL_DISTANCE_TYPE: [u8; 16] = [
    0xbc, 0x14, 0x4b, 0x4e, 0xd2, 0x11, 0xfe, 0xa4, 0x60, 0x00, 0x10, 0xb3, 0x89, 0x32, 0xed, 0xb0,
];
const BLOCK_INSTANCE_TYPE: [u8; 16] = [
    0x4c, 0x12, 0xf1, 0x4c, 0x1d, 0x4f, 0x0f, 0xb4, 0xd3, 0xf8, 0x0f, 0x82, 0x44, 0x52, 0x8f, 0xcb,
];
const BLOCK_MEMBER_TYPE: [u8; 16] = [
    0x7e, 0x71, 0xa9, 0x18, 0xee, 0x49, 0x61, 0x2a, 0x26, 0x8c, 0x54, 0x9c, 0x65, 0x84, 0xf7, 0xfe,
];
const BLOCK_GROUP_TYPE: [u8; 16] = [
    0xcf, 0x8d, 0x56, 0xf2, 0x32, 0x4a, 0x27, 0xdd, 0xb6, 0x66, 0xf1, 0x89, 0x4a, 0x9d, 0x16, 0x17,
];
const POLYGON_TYPE: [u8; 16] = [
    0x38, 0x48, 0x53, 0x52, 0x9f, 0x44, 0x52, 0x55, 0xe1, 0xe1, 0xda, 0xb4, 0x10, 0xeb, 0x06, 0xc4,
];
const POLYGON_RELATION_TYPE: [u8; 16] = [
    0x1f, 0x89, 0xdb, 0xfb, 0xd8, 0x47, 0x78, 0xb6, 0xb0, 0x96, 0x72, 0x85, 0xc7, 0xec, 0x8b, 0xb9,
];
const DISTANCE_TYPE: [u8; 16] = [
    0x58, 0x85, 0x05, 0x11, 0xd2, 0x11, 0xe3, 0x95, 0x60, 0x00, 0x0c, 0xb3, 0x89, 0x32, 0xed, 0xb0,
];
const ANGLE_TYPE: [u8; 16] = [
    0x10, 0x0a, 0x0d, 0x59, 0xd1, 0x11, 0xca, 0xe6, 0x80, 0x00, 0x6f, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const MIDPOINT_TYPE: [u8; 16] = sketch_entity_id(0x21e8_70bf);
const FIX_TYPE: [u8; 16] = [
    0xb0, 0x7b, 0x17, 0x0f, 0xd1, 0x11, 0x96, 0xe6, 0x80, 0x00, 0x6f, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const HORIZONTAL_ALIGNMENT_TYPE: [u8; 16] = [
    0xc0, 0xa3, 0x55, 0x8f, 0xd1, 0x11, 0x5e, 0xe0, 0x80, 0x00, 0x66, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const VERTICAL_ALIGNMENT_TYPE: [u8; 16] = [
    0x50, 0x52, 0xda, 0x64, 0xd1, 0x11, 0x5e, 0xe0, 0x80, 0x00, 0x66, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const COLLINEAR_TYPE: [u8; 16] = [
    0xe0, 0x7b, 0x9a, 0x5a, 0xd1, 0x11, 0x5d, 0xe0, 0x80, 0x00, 0x66, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
];
const SYMMETRIC_TYPE: [u8; 16] = [
    0x10, 0x7c, 0xdd, 0x0d, 0xd1, 0x11, 0x9c, 0xe6, 0x80, 0x00, 0x6f, 0xb1, 0xe1, 0x35, 0x54, 0xc7,
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
    /// Absent in a sketch-block group, whose list follows the maps.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_parameter"
    )]
    pub(crate) parameter: Option<PmDcReference>,
}

cadmpeg_core::named_optional_field!(deserialize_parameter, PmDcReference, "parameter");

type PmDcReferenceScalarMap = crate::pmdc::PmDcPairedMap<FiniteReal>;
type PmDcReferencePairMap = crate::pmdc::PmDcPairedMap<PmDcReference>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum PmDcSketchConstraintKind {
    SplinePoint {
        spline: PmDcReference,
        point: PmDcReference,
    },
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
    EqualLength {
        first: PmDcReference,
        second: PmDcReference,
    },
    Distance {
        value: u32,
        first: PmDcReference,
        second: PmDcReference,
        values: [FiniteReal; 6],
    },
    Angle {
        value: u32,
        first: PmDcReference,
        second: PmDcReference,
        values: [FiniteReal; 2],
        flags: u16,
    },
    Midpoint {
        entity: PmDcReference,
        point: PmDcReference,
        /// The trailing value of save version 20 and later.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<u32>,
    },
    Fix {
        entity: PmDcReference,
    },
    HorizontalAlignment {
        first: PmDcReference,
        second: PmDcReference,
    },
    VerticalAlignment {
        first: PmDcReference,
        second: PmDcReference,
    },
    Collinear {
        first: PmDcReference,
        second: PmDcReference,
        state: u16,
    },
    Symmetric {
        first: PmDcReference,
        second: PmDcReference,
        axis: PmDcReference,
    },
    PolygonRelation {
        first: PmDcReference,
        second: PmDcReference,
        reference: PmDcReference,
        polygon: [PmDcReference; 2],
        values: [u32; 2],
    },
    BlockGroup {
        members: PmDcReferenceList,
    },
    EqualDistance {
        first: [PmDcReference; 2],
        second: [PmDcReference; 2],
    },
}

pub(crate) struct SketchProjection {
    pub(crate) sketches: Vec<Sketch>,
    pub(crate) entities: Vec<SketchEntity>,
    pub(crate) constraints: Vec<SketchConstraint>,
    pub(crate) unresolved_sketches: usize,
    pub(crate) unresolved_entities: usize,
    pub(crate) unresolved_constraints: usize,
    /// Projected curves whose construction state is unresolved; they stay out
    /// of the sketch profile tables.
    pub(crate) unresolved_construction: usize,
    /// Distance dimensions measured to an axis line; they stay native.
    pub(crate) native_axis_distances: usize,
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
#[serde(try_from = "point_tail::SketchEntityWire")]
pub(crate) struct PmDcSketchEntityPayload {
    save_version_major: u8,
    pub(crate) header: PmDcContentHeader,
    entity_flags: u32,
    pub(crate) sketch: PmDcReference,
    pub(crate) kind: PmDcSketchEntityKind,
}

impl PmDcSketchEntityPayload {
    /// Whether this is a line that a distance dimension measures as an axis.
    pub(crate) fn is_axis_line(&self) -> bool {
        matches!(self.kind, PmDcSketchEntityKind::Line { .. })
            && self.entity_flags & AXIS_LINE_FLAG != 0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "form", rename_all = "snake_case")]
pub(crate) enum PmDcSketchEntityKind {
    Spline {
        data: Box<spline::SplineRecord>,
    },
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
    Polygon {
        reference: PmDcReference,
        value: u32,
    },
    BlockInstance {
        definition: PmDcReference,
        group: PmDcReference,
        masks: [u16; 2],
        placement: [[FiniteReal; 3]; 3],
        value: u32,
    },
    BlockMember {
        instances: PmDcReferenceList,
        member: PmDcReference,
    },
}

impl PmDcSketchEntityKind {
    pub(crate) fn record_type_id(&self) -> [u8; 16] {
        match self {
            Self::Spline { .. } => spline::TYPE,
            Self::Point { .. } => POINT_TYPE,
            Self::Line { .. } => LINE_TYPE,
            Self::Circle { .. } => CIRCLE_TYPE,
            Self::Ellipse { .. } => ELLIPSE_TYPE,
            Self::Polygon { .. } => POLYGON_TYPE,
            Self::BlockInstance { .. } => BLOCK_INSTANCE_TYPE,
            Self::BlockMember { .. } => BLOCK_MEMBER_TYPE,
        }
    }
}

cadmpeg_core::named_optional_field!(deserialize_point_links, PmDcReferenceList, "links");
cadmpeg_core::named_optional_field!(deserialize_point_paired, PointPairedAssociation, "paired");

/// The absent, ordinary, or paired association tail of a sketch point.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "PointTailWire")]
pub(crate) enum PointTail {
    /// The record ends after the center-of list.
    Absent,
    /// A state and a type-2 association list; from segment major version 25,
    /// also a type-2 link list.
    Present {
        state: u32,
        associations: PmDcReferenceList,
        links: Option<PmDcReferenceList>,
    },
    /// The state-2 form of segment major version 25 and later.
    Paired(PointPairedAssociation),
}

#[derive(Serialize)]
struct PointTailRef<'a> {
    state: Option<u32>,
    associations: Option<&'a PmDcReferenceList>,
    #[serde(skip_serializing_if = "Option::is_none")]
    paired: Option<&'a PointPairedAssociation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    links: Option<&'a PmDcReferenceList>,
}

impl Serialize for PointTail {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let wire = match self {
            Self::Absent => PointTailRef {
                state: None,
                associations: None,
                paired: None,
                links: None,
            },
            Self::Present {
                state,
                associations,
                links,
            } => PointTailRef {
                state: Some(*state),
                associations: Some(associations),
                paired: None,
                links: links.as_ref(),
            },
            Self::Paired(paired) => PointTailRef {
                state: Some(2),
                associations: None,
                paired: Some(paired),
                links: None,
            },
        };
        wire.serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct PointTailWire {
    state: Option<u32>,
    associations: Option<PmDcReferenceList>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_point_paired"
    )]
    paired: Option<PointPairedAssociation>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_point_links"
    )]
    links: Option<PmDcReferenceList>,
}

impl From<PointTail> for PointTailWire {
    fn from(tail: PointTail) -> Self {
        match tail {
            PointTail::Absent => Self {
                state: None,
                associations: None,
                paired: None,
                links: None,
            },
            PointTail::Present {
                state,
                associations,
                links,
            } => Self {
                state: Some(state),
                associations: Some(associations),
                paired: None,
                links,
            },
            PointTail::Paired(paired) => Self {
                state: Some(2),
                associations: None,
                paired: Some(paired),
                links: None,
            },
        }
    }
}

impl TryFrom<PointTailWire> for PointTail {
    type Error = &'static str;

    fn try_from(wire: PointTailWire) -> Result<Self, Self::Error> {
        match (wire.state, wire.associations, wire.paired, wire.links) {
            (Some(2), None, Some(paired), None) => Ok(Self::Paired(paired)),
            (Some(state), Some(associations), None, links) if state != 2 || links.is_none() => {
                Ok(Self::Present {
                    state,
                    associations,
                    links,
                })
            }
            (None, None, None, None) => Ok(Self::Absent),
            _ => Err("point state and association form disagree"),
        }
    }
}

const TRANSFORM_PREFIX: u32 = 0x203;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "PmDcTransformPayloadWire")]
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
    for segment in &document.segments {
        if segment.kind != SegmentKind::PmDc {
            continue;
        }
        let Some(version) = segment.registry.map(|join| join.version_major) else {
            continue;
        };
        if !crate::pmdc::record_version_supported(version) {
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
                    parse_sketch(record.payload, version).and_then(|value| {
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
                SketchRecordTag::Spline => match spline::parse(ctx, record.payload, version) {
                    Ok(value) => push_record(
                        ctx,
                        &mut inventory.entities,
                        value,
                        record.type_id,
                        segment.pair.token.key(),
                        record.ordinal,
                        "admit Inventor PmDc spline record",
                    ),
                    Err(CodecError::NotImplemented(_)) => continue,
                    Err(error) => Err(error),
                },
                SketchRecordTag::Entity(entity) => parse_entity(entity, record.payload, version)
                    .and_then(|value| {
                        push_record(
                            ctx,
                            &mut inventory.entities,
                            value,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc sketch entity record",
                        )
                    }),
                SketchRecordTag::Transform => {
                    parse_transform(record.payload, version).and_then(|value| {
                        push_record(
                            ctx,
                            &mut inventory.transforms,
                            value,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc transform record",
                        )
                    })
                }
                SketchRecordTag::Direction => {
                    parse_direction(record.payload, version).and_then(|value| {
                        push_record(
                            ctx,
                            &mut inventory.directions,
                            value,
                            record.type_id,
                            segment.pair.token.key(),
                            record.ordinal,
                            "admit Inventor PmDc direction record",
                        )
                    })
                }
                SketchRecordTag::Constraint(constraint) => {
                    parse_constraint(constraint, record.payload, version).and_then(|value| {
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
                inventory.issues.push(RecordIssue {
                    family: RecordIssueFamily::Sketch {
                        type_id: crate::record_identity::RecordTypeId::from_bytes(record.type_id),
                    },
                    segment_token: segment.pair.token.key().clone(),
                    record_ordinal: record.ordinal,
                    detail: crate::issue_detail(error)?,
                });
            }
        }
    }
    Ok(inventory)
}

fn parse_sketch(source: View<'_>, version: u8) -> Result<PmDcSketchPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = state_content_header(&mut cursor, version)?;
    let state = cursor.u32("sketch state")?.cast_signed();
    let count_value = cursor.u32("sketch count value")?;
    let entities = reference_list(&mut cursor, 8, "sketch entity array")?;
    let transform = cursor.reference("sketch transform reference")?;
    let direction = cursor.reference("sketch direction reference")?;
    let values = [cursor.u32("sketch value 0")?, cursor.u32("sketch value 1")?];
    let auxiliary = (cursor.remaining() != 0)
        .then(|| reference_list(&mut cursor, 2, "sketch auxiliary list"))
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
    Polygon,
    BlockInstance,
    BlockMember,
}

impl SketchEntityTag {
    fn from_type_id(type_id: [u8; 16]) -> Option<Self> {
        match type_id {
            POINT_TYPE => Some(Self::Point),
            LINE_TYPE => Some(Self::Line),
            CIRCLE_TYPE => Some(Self::Circle),
            ELLIPSE_TYPE => Some(Self::Ellipse),
            POLYGON_TYPE => Some(Self::Polygon),
            BLOCK_INSTANCE_TYPE => Some(Self::BlockInstance),
            BLOCK_MEMBER_TYPE => Some(Self::BlockMember),
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
    EqualLength,
    Distance,
    Angle,
    Midpoint,
    Fix,
    HorizontalAlignment,
    VerticalAlignment,
    Collinear,
    Symmetric,
    PolygonRelation,
    BlockGroup,
    EqualDistance,
    SplinePoint,
}

impl SketchConstraintTag {
    fn from_type_id(type_id: [u8; 16]) -> Option<Self> {
        match type_id {
            spline::POINT_RELATION_TYPE => Some(Self::SplinePoint),
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
            EQUAL_LENGTH_TYPE => Some(Self::EqualLength),
            DISTANCE_TYPE => Some(Self::Distance),
            ANGLE_TYPE => Some(Self::Angle),
            MIDPOINT_TYPE => Some(Self::Midpoint),
            FIX_TYPE => Some(Self::Fix),
            HORIZONTAL_ALIGNMENT_TYPE => Some(Self::HorizontalAlignment),
            VERTICAL_ALIGNMENT_TYPE => Some(Self::VerticalAlignment),
            COLLINEAR_TYPE => Some(Self::Collinear),
            SYMMETRIC_TYPE => Some(Self::Symmetric),
            POLYGON_RELATION_TYPE => Some(Self::PolygonRelation),
            BLOCK_GROUP_TYPE => Some(Self::BlockGroup),
            EQUAL_DISTANCE_TYPE => Some(Self::EqualDistance),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
enum SketchRecordTag {
    Spline,
    Sketch,
    Transform,
    Direction,
    Entity(SketchEntityTag),
    Constraint(SketchConstraintTag),
}

impl SketchRecordTag {
    fn from_type_id(type_id: [u8; 16]) -> Option<Self> {
        match type_id {
            spline::TYPE => Some(Self::Spline),
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
    tag: SketchEntityTag,
    source: View<'_>,
    version: u8,
) -> Result<PmDcSketchEntityPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let entity_flags = cursor.u32("sketch entity flags")?;
    let sketch = cursor.reference("sketch entity sketch reference")?;
    let kind = match tag {
        SketchEntityTag::Point => parse_point(&mut cursor, version)?,
        SketchEntityTag::Line => parse_line(&mut cursor)?,
        SketchEntityTag::Circle => parse_circle(&mut cursor)?,
        SketchEntityTag::Ellipse => parse_ellipse(&mut cursor)?,
        SketchEntityTag::Polygon => PmDcSketchEntityKind::Polygon {
            reference: cursor.reference("polygon reference entity")?,
            value: cursor.u32("polygon value")?,
        },
        SketchEntityTag::BlockInstance => {
            let definition = cursor.reference("block instance definition reference")?;
            let group = cursor.reference("block instance group reference")?;
            let value_mask = cursor.u16("block instance value mask")?;
            let zero_mask = cursor.u16("block instance zero mask")?;
            let mut placement = [[FiniteReal::ZERO; 3]; 3];
            for (index, cell) in placement.iter_mut().flatten().enumerate() {
                let bit = 1u16 << index;
                *cell = match (value_mask & bit != 0, zero_mask & bit != 0) {
                    (false, false) => cursor.f64("block instance placement value")?,
                    (true, false) => FiniteReal::ONE,
                    (false, true) => FiniteReal::ZERO,
                    (true, true) => FiniteReal::ONE.negated(),
                };
            }
            if (value_mask | zero_mask) >> 9 != 0 {
                return Err(CodecError::Malformed(
                    "Inventor PmDc block instance mask names a cell outside its 3-by-3 placement"
                        .into(),
                ));
            }
            PmDcSketchEntityKind::BlockInstance {
                definition,
                group,
                masks: [value_mask, zero_mask],
                placement,
                value: cursor.u32("block instance value")?,
            }
        }
        SketchEntityTag::BlockMember => PmDcSketchEntityKind::BlockMember {
            instances: reference_list(&mut cursor, 2, "block member instance list")?,
            member: cursor.reference("block member entity reference")?,
        },
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

fn point2(cursor: &mut Cursor<'_>, field: &str) -> Result<[FiniteReal; 2], CodecError> {
    Ok([
        cursor.f64(&format!("{field} u"))?,
        cursor.f64(&format!("{field} v"))?,
    ])
}

fn point3(cursor: &mut Cursor<'_>, field: &str) -> Result<[FiniteReal; 3], CodecError> {
    Ok([
        cursor.f64(&format!("{field} x"))?,
        cursor.f64(&format!("{field} y"))?,
        cursor.f64(&format!("{field} z"))?,
    ])
}

fn parse_point(cursor: &mut Cursor<'_>, version: u8) -> Result<PmDcSketchEntityKind, CodecError> {
    let position = point2(cursor, "sketch point")?;
    let endpoint_of = reference_list(cursor, 2, "point endpoint-of list")?;
    let center_of = reference_list(cursor, 2, "point center-of list")?;
    let tail = point_tail::parse_tail(cursor, version)?;
    Ok(PmDcSketchEntityKind::Point {
        position,
        endpoint_of,
        center_of,
        tail,
    })
}

fn edge_prefix(
    cursor: &mut Cursor<'_>,
    fixed_tail: usize,
    field: &str,
) -> Result<(PmDcReferenceList, Vec<PmDcReferenceList>), CodecError> {
    let points = reference_list(cursor, 2, &format!("{field} point list"))?;
    let mut auxiliary = Vec::new();
    let tail8 = fixed_tail
        .checked_add(8)
        .ok_or_else(|| CodecError::malformed("Inventor edge tail size overflow"))?;
    if cursor.remaining() >= tail8 && cursor.peek_u32("edge auxiliary-list marker")? == 0x3000_0002
    {
        auxiliary.push(reference_list(
            cursor,
            2,
            &format!("{field} auxiliary list 0"),
        )?);
    } else if cursor.remaining() >= tail8 {
        let gate = [
            cursor.u32("edge list gate 0")?,
            cursor.u32("edge list gate 1")?,
        ];
        match gate {
            // A closed gate stores no auxiliary list.
            [0, 0] => return close_edge_prefix(cursor, fixed_tail, field, points, auxiliary),
            [1, 0] => {}
            _ => {
                return Err(CodecError::malformed(format_args!(
                    "Inventor PmDc {field} list gate is {gate:?}"
                )));
            }
        }
        auxiliary.push(reference_list(
            cursor,
            2,
            &format!("{field} auxiliary list 0"),
        )?);
        if cursor.remaining() >= tail8
            && cursor.peek_u32("edge auxiliary-list marker")? == 0x3000_0002
        {
            auxiliary.push(reference_list(
                cursor,
                2,
                &format!("{field} auxiliary list 1"),
            )?);
        }
    }
    close_edge_prefix(cursor, fixed_tail, field, points, auxiliary)
}

fn close_edge_prefix(
    cursor: &Cursor<'_>,
    fixed_tail: usize,
    field: &str,
    points: PmDcReferenceList,
    auxiliary: Vec<PmDcReferenceList>,
) -> Result<(PmDcReferenceList, Vec<PmDcReferenceList>), CodecError> {
    if cursor.remaining() != fixed_tail {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc {field} has {} bytes before its fixed tail, expected {fixed_tail}",
            cursor.remaining()
        )));
    }
    Ok((points, auxiliary))
}

fn parse_line(cursor: &mut Cursor<'_>) -> Result<PmDcSketchEntityKind, CodecError> {
    let (points, auxiliary) = edge_prefix(cursor, 32, "line")?;
    let origin = point2(cursor, "line origin")?;
    let direction = point2(cursor, "line direction")?;
    Ok(PmDcSketchEntityKind::Line {
        points,
        auxiliary,
        origin,
        direction,
    })
}

fn parse_circle(cursor: &mut Cursor<'_>) -> Result<PmDcSketchEntityKind, CodecError> {
    let (points, auxiliary) = edge_prefix(cursor, 13, "circle")?;
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

fn parse_ellipse(cursor: &mut Cursor<'_>) -> Result<PmDcSketchEntityKind, CodecError> {
    let (points, auxiliary) = edge_prefix(cursor, 37, "ellipse")?;
    let center = cursor.reference("ellipse center reference")?;
    let major_direction = point2(cursor, "ellipse major direction")?;
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

fn parse_transform(source: View<'_>, version: u8) -> Result<PmDcTransformPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = content_header(&mut cursor, version)?;
    let prefix_present = cursor.peek_u32("transform prefix")? == TRANSFORM_PREFIX;
    if prefix_present {
        cursor.u32("transform prefix")?;
    }
    let value_mask = cursor.u16("transform value mask")?;
    let zero_mask = cursor.u16("transform zero mask")?;
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
    let header = content_header(&mut cursor, version)?;
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

fn reference_scalar_map(cursor: &mut Cursor<'_>) -> Result<PmDcReferenceScalarMap, CodecError> {
    let (count, metadata) = map_header(cursor, "constraint scalar map")?;
    let mut entries = Vec::new();
    for index in 0..count {
        entries.push((
            cursor.reference("constraint scalar-map key")?,
            cursor.f64(&format!("constraint scalar-map value {index}"))?,
        ));
    }
    PmDcReferenceScalarMap::new(metadata, entries).ok_or_else(|| {
        CodecError::Malformed("Inventor PmDc scalar map metadata disagrees with length".into())
    })
}

fn reference_pair_map(cursor: &mut Cursor<'_>) -> Result<PmDcReferencePairMap, CodecError> {
    let (count, metadata) = map_header(cursor, "constraint reference map")?;
    let mut entries = Vec::new();
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
    cursor: &mut Cursor<'_>,
    version: u8,
    with_parameter: bool,
) -> Result<PmDcConstraintHeader, CodecError> {
    let content = state_content_header(cursor, version)?;
    let state = cursor.u32("constraint state")?.cast_signed();
    let group = cursor.reference("constraint group reference")?;
    let (scalar_map, reference_map) = if version <= 16 {
        (
            PmDcReferenceScalarMap::empty(),
            PmDcReferencePairMap::empty(),
        )
    } else {
        (reference_scalar_map(cursor)?, reference_pair_map(cursor)?)
    };
    Ok(PmDcConstraintHeader {
        content,
        state,
        group,
        scalar_map,
        reference_map,
        parameter: with_parameter
            .then(|| cursor.reference("constraint parameter reference"))
            .transpose()?,
    })
}

fn parse_constraint(
    tag: SketchConstraintTag,
    source: View<'_>,
    version: u8,
) -> Result<PmDcSketchConstraintPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header = parse_constraint_header(
        &mut cursor,
        version,
        !matches!(tag, SketchConstraintTag::BlockGroup),
    )?;
    let kind = match tag {
        SketchConstraintTag::SplinePoint => PmDcSketchConstraintKind::SplinePoint {
            spline: cursor.reference("spline point curve reference")?,
            point: cursor.reference("spline point reference")?,
        },
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
        SketchConstraintTag::EqualLength => PmDcSketchConstraintKind::EqualLength {
            first: cursor.reference("equal-length constraint first reference")?,
            second: cursor.reference("equal-length constraint second reference")?,
        },
        SketchConstraintTag::Distance => PmDcSketchConstraintKind::Distance {
            value: cursor.u32("distance dimension value")?,
            first: cursor.reference("distance dimension first reference")?,
            second: cursor.reference("distance dimension second reference")?,
            values: f64_array(&mut cursor, "distance dimension values")?,
        },
        SketchConstraintTag::Angle => PmDcSketchConstraintKind::Angle {
            value: cursor.u32("angle dimension value")?,
            first: cursor.reference("angle dimension first reference")?,
            second: cursor.reference("angle dimension second reference")?,
            values: f64_array(&mut cursor, "angle dimension values")?,
            flags: cursor.u16("angle dimension flags")?,
        },
        SketchConstraintTag::Midpoint => PmDcSketchConstraintKind::Midpoint {
            entity: cursor.reference("midpoint constraint entity reference")?,
            point: cursor.reference("midpoint constraint point reference")?,
            value: (version >= MIDPOINT_VALUE_VERSION)
                .then(|| cursor.u32("midpoint constraint value"))
                .transpose()?,
        },
        SketchConstraintTag::Fix => PmDcSketchConstraintKind::Fix {
            entity: cursor.reference("fix constraint entity reference")?,
        },
        SketchConstraintTag::HorizontalAlignment => PmDcSketchConstraintKind::HorizontalAlignment {
            first: cursor.reference("horizontal alignment first reference")?,
            second: cursor.reference("horizontal alignment second reference")?,
        },
        SketchConstraintTag::VerticalAlignment => PmDcSketchConstraintKind::VerticalAlignment {
            first: cursor.reference("vertical alignment first reference")?,
            second: cursor.reference("vertical alignment second reference")?,
        },
        SketchConstraintTag::Collinear => PmDcSketchConstraintKind::Collinear {
            first: cursor.reference("collinear constraint first reference")?,
            second: cursor.reference("collinear constraint second reference")?,
            state: cursor.u16("collinear constraint state")?,
        },
        SketchConstraintTag::Symmetric => PmDcSketchConstraintKind::Symmetric {
            first: cursor.reference("symmetric constraint first reference")?,
            second: cursor.reference("symmetric constraint second reference")?,
            axis: cursor.reference("symmetric constraint axis reference")?,
        },
        SketchConstraintTag::EqualDistance => PmDcSketchConstraintKind::EqualDistance {
            first: [
                cursor.reference("equal-distance first pair first reference")?,
                cursor.reference("equal-distance first pair second reference")?,
            ],
            second: [
                cursor.reference("equal-distance second pair first reference")?,
                cursor.reference("equal-distance second pair second reference")?,
            ],
        },
        SketchConstraintTag::BlockGroup => PmDcSketchConstraintKind::BlockGroup {
            members: reference_list(&mut cursor, 2, "block group member list")?,
        },
        SketchConstraintTag::PolygonRelation => PmDcSketchConstraintKind::PolygonRelation {
            first: cursor.reference("polygon relation first reference")?,
            second: cursor.reference("polygon relation second reference")?,
            reference: cursor.reference("polygon relation reference entity")?,
            polygon: [
                cursor.reference("polygon relation polygon reference")?,
                cursor.reference("polygon relation second polygon reference")?,
            ],
            values: cursor.u32_array::<2>("polygon relation values")?,
        },
    };
    cursor.finish("sketch constraint")?;
    Ok(PmDcSketchConstraintPayload {
        save_version_major: version,
        header,
        kind,
    })
}

fn f64_array<const N: usize>(
    cursor: &mut Cursor<'_>,
    field: &str,
) -> Result<[FiniteReal; N], CodecError> {
    let mut values = [FiniteReal::ZERO; N];
    for value in &mut values {
        *value = cursor.f64(field)?;
    }
    Ok(values)
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

fn count_removed(unresolved: usize, before: usize, after: usize) -> Result<usize, CodecError> {
    let removed = before
        .checked_sub(after)
        .ok_or_else(|| CodecError::malformed("Inventor projected count exceeds prior count"))?;
    Ok(unresolved + removed)
}

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    inventory: &SketchInventory,
    parameters: &[DesignParameter],
) -> Result<SketchProjection, CodecError> {
    let raw_sketches = unique_index(inventory.sketches.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    let raw_entities = unique_index(inventory.entities.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    let transforms = unique_index(inventory.transforms.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    let directions = unique_index(inventory.directions.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    let raw_constraints = unique_index(inventory.constraints.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    let mut parameter_index = HashMap::new();
    for parameter in parameters {
        if let Some(native) = &parameter.native_ref {
            let id = parameter.id.clone();
            parameter_index.insert(native.clone(), id);
        }
    }

    let mut projected_entities = Vec::new();
    let mut unresolved_entities = 0usize;
    for entity in &inventory.entities {
        let key = (
            entity.identity.segment_token.as_str(),
            entity.identity.record_ordinal,
        );
        if raw_entities.get(&key).and_then(Option::as_ref).is_none() {
            unresolved_entities += 1;
            continue;
        }
        let Some(sketch_ordinal) = entity.sketch.index().checked_sub(1) else {
            unresolved_entities += 1;
            continue;
        };
        let Some(sketch) = raw_sketches
            .get(&(entity.identity.segment_token.as_str(), sketch_ordinal))
            .and_then(Option::as_ref)
        else {
            unresolved_entities += 1;
            continue;
        };
        if !sketch.entities.references().iter().any(|reference| {
            entity
                .identity
                .record_ordinal
                .checked_add(1)
                .is_some_and(|next| reference.index() == next)
        }) {
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
        ctx.charge_entities(1, "project Inventor sketch entity")?;
        projected_entities.push(
            SketchEntity::new(entity_id, sketch_id, geometry)
                .with_construction(marks_construction(entity))
                .with_native_ref(Some(entity.id()))
                .with_endpoint_refs(entity_endpoint_refs(entity, &raw_entities)),
        );
    }

    // Placed block curves stay out of the native-keyed indexes: two instances
    // of one definition place copies of the same curve records.
    let mut block_curves = Vec::new();
    for entity in &projected_entities {
        let Some(instance) = entity.native_ref.as_deref().and_then(|native| {
            inventory.entities.iter().find(|raw| {
                matches!(raw.kind, PmDcSketchEntityKind::BlockInstance { .. }) && raw.id() == native
            })
        }) else {
            continue;
        };
        let count = block_definition_curves(instance, &inventory.entities).len();
        ctx.charge_entities(
            u64::try_from(count).map_err(CodecError::malformed)?,
            "project Inventor sketch block curves",
        )?;
        if let Some(placed) = place_block_curves(instance, &entity.sketch, inventory, &raw_entities)
        {
            block_curves.push(placed);
        }
    }
    let projected_by_native = projected_entities
        .iter()
        .filter_map(|entity| entity.native_ref.as_deref().map(|native| (native, entity)))
        .collect::<HashMap<_, _>>();
    let construction_unresolved = inventory
        .entities
        .iter()
        .filter(|entity| has_unresolved_flags(entity))
        .map(Located::id)
        .collect::<HashSet<_>>();
    let mut sketches = Vec::new();
    let mut unresolved_sketches = 0usize;
    for sketch in &inventory.sketches {
        let key = (
            sketch.identity.segment_token.as_str(),
            sketch.identity.record_ordinal,
        );
        if raw_sketches.get(&key).and_then(Option::as_ref).is_none() {
            unresolved_sketches += 1;
            continue;
        }
        let mut raw_referenced_entities = Vec::new();
        for reference in sketch.entities.references() {
            if let Some(raw) = reference.index().checked_sub(1).and_then(|ordinal| {
                raw_entities
                    .get(&(sketch.identity.segment_token.as_str(), ordinal))
                    .and_then(Option::as_ref)
                    .copied()
            }) {
                raw_referenced_entities.push(raw);
            }
        }
        let mut referenced_entities = Vec::new();
        for raw in &raw_referenced_entities {
            let native = raw.id();
            if let Some(projected) = projected_by_native.get(native.as_str()).copied() {
                referenced_entities.push(projected);
            }
        }
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
        let construction_unknown = |entity: &&SketchEntity| {
            entity
                .native_ref
                .as_deref()
                .is_some_and(|native| construction_unresolved.contains(native))
        };
        let profile_entities = referenced_entities
            .iter()
            .copied()
            .filter(|entity| !construction_unknown(entity))
            .collect::<Vec<_>>();
        let mut loops = build_profiles(&profile_entities);
        for placed in block_curves
            .iter()
            .filter(|placed| placed.first().is_some_and(|curve| curve.sketch == id))
        {
            loops.extend(build_profiles(
                &placed
                    .iter()
                    .filter(|entity| !construction_unknown(entity))
                    .collect::<Vec<_>>(),
            ));
        }
        let Ok(profiles) = cadmpeg_ir::sketches::SketchProfiles::try_from(loops) else {
            unresolved_sketches += 1;
            continue;
        };
        ctx.charge_entities(1, "project Inventor sketch")?;
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
        .map(|sketch| &sketch.id)
        .collect::<HashSet<_>>();
    let previous_entity_count = projected_entities.len();
    projected_entities.retain(|entity| projected_sketch_ids.contains(&entity.sketch));
    unresolved_entities = count_removed(
        unresolved_entities,
        previous_entity_count,
        projected_entities.len(),
    )?;
    let projected_by_native = projected_entities
        .iter()
        .filter_map(|entity| entity.native_ref.as_deref().map(|native| (native, entity)))
        .collect::<HashMap<_, _>>();
    let mut projected_entity_by_key = HashMap::new();
    for raw in &inventory.entities {
        if let Some(projected) = projected_by_native.get(raw.id().as_str()) {
            projected_entity_by_key.insert(
                (
                    raw.identity.segment_token.as_str(),
                    raw.identity.record_ordinal,
                ),
                *projected,
            );
        }
    }
    let axis_lines = inventory
        .entities
        .iter()
        .filter(|entity| entity.is_axis_line())
        .map(Located::id)
        .collect::<HashSet<_>>();
    let mut native_axis_distances = 0usize;
    let mut constraints = Vec::new();
    let mut projected_constraint_keys = HashSet::new();
    for constraint in &inventory.constraints {
        if raw_constraints
            .get(&(
                constraint.identity.segment_token.as_str(),
                constraint.identity.record_ordinal,
            ))
            .and_then(Option::as_ref)
            .is_some()
        {
            if let Some(projected) = project_constraint(
                ctx,
                constraint,
                &projected_entity_by_key,
                &parameter_index,
                &axis_lines,
            )
            .transpose()?
            {
                projected_constraint_keys.insert((
                    constraint.identity.segment_token.as_str(),
                    constraint.identity.record_ordinal,
                ));
                if let PmDcSketchConstraintKind::Distance { first, second, .. } = constraint.kind {
                    let operands = [first, second].map(|operand| {
                        let ordinal = operand.index().checked_sub(1)?;
                        projected_entity_by_key
                            .get(&(constraint.identity.segment_token.as_str(), ordinal))
                            .copied()
                    });
                    if let [Some(first), Some(second)] = operands {
                        native_axis_distances +=
                            usize::from(measures_axis_line(&[first, second], &axis_lines));
                    }
                }
                constraints.push(projected);
            }
        }
    }
    let mut unresolved_constraints = inventory
        .constraints
        .len()
        .checked_sub(constraints.len())
        .ok_or_else(|| CodecError::malformed("Inventor constraints exceed inventory"))?;
    let projected_entity_keys = projected_entity_by_key
        .keys()
        .copied()
        .collect::<HashSet<_>>();
    let mut raw_sketch_by_native = HashMap::new();
    for sketch in &inventory.sketches {
        raw_sketch_by_native.insert(sketch.id(), sketch);
    }
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
            let Some(ordinal) = reference.index().checked_sub(1) else {
                return false;
            };
            if !seen.insert(ordinal) {
                return false;
            }
            let key = (raw.identity.segment_token.as_str(), ordinal);
            projected_entity_keys.contains(&key) || projected_constraint_keys.contains(&key)
        })
    });
    unresolved_sketches =
        count_removed(unresolved_sketches, previous_sketch_count, sketches.len())?;
    let closed_sketch_ids = sketches
        .iter()
        .map(|sketch| &sketch.id)
        .collect::<HashSet<_>>();
    let previous_entity_count = projected_entities.len();
    projected_entities.retain(|entity| closed_sketch_ids.contains(&entity.sketch));
    unresolved_entities = count_removed(
        unresolved_entities,
        previous_entity_count,
        projected_entities.len(),
    )?;
    projected_entities.extend(
        block_curves
            .into_iter()
            .flatten()
            .filter(|curve| closed_sketch_ids.contains(&curve.sketch)),
    );
    let mut raw_constraint_by_native = HashMap::new();
    for constraint in &inventory.constraints {
        raw_constraint_by_native.insert(constraint.id(), constraint);
    }
    let mut raw_sketch_by_id = HashMap::new();
    for sketch in &inventory.sketches {
        if let Some(id) = sketch_id(sketch) {
            raw_sketch_by_id.insert(id, sketch);
        }
    }
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
                    raw_constraint
                        .identity
                        .record_ordinal
                        .checked_add(1)
                        .is_some_and(|next| reference.index() == next)
                })
            })
    });
    unresolved_constraints = count_removed(
        unresolved_constraints,
        previous_constraint_count,
        constraints.len(),
    )?;
    let unresolved_construction = projected_entities
        .iter()
        .filter(|entity| {
            entity
                .native_ref
                .as_deref()
                .is_some_and(|native| construction_unresolved.contains(native))
        })
        .count();
    Ok(SketchProjection {
        sketches,
        entities: projected_entities,
        constraints,
        unresolved_sketches,
        unresolved_entities,
        unresolved_constraints,
        unresolved_construction,
        native_axis_distances,
    })
}

/// The loss for distance dimensions that stay native because they are
/// measured to an axis line.
pub(crate) fn axis_distance_loss(count: usize) -> Option<cadmpeg_ir::report::loss::LossNote> {
    (count != 0).then(|| {
        crate::loss::InventorLossCode::SketchAxisDistanceNative.note(format!(
            "Retained {count} Inventor sketch distance dimension(s) measured to an axis line natively: the parameter is twice the distance between the operands."
        ))
    })
}

/// Whether a distance dimension has an axis line as an operand. Its
/// parameter is then twice the distance between the operands.
fn measures_axis_line(members: &[&SketchEntity; 2], axis_lines: &HashSet<String>) -> bool {
    members.iter().any(|member| {
        member
            .native_ref
            .as_deref()
            .is_some_and(|native| axis_lines.contains(native))
    })
}

pub(crate) fn constraint_maps_supported(constraint: &PmDcSketchConstraint) -> bool {
    (constraint.header.scalar_map.metadata().is_none()
        && constraint.header.scalar_map.entries().is_empty()
        && constraint.header.reference_map.metadata().is_none()
        && constraint.header.reference_map.entries().is_empty())
        || solver_hint(constraint)
}

fn project_constraint(
    ctx: &DecodeContext<'_>,
    constraint: &PmDcSketchConstraint,
    entities: &HashMap<(&str, u32), &SketchEntity>,
    parameters: &HashMap<String, ParameterId>,
    axis_lines: &HashSet<String>,
) -> Option<Result<SketchConstraint, CodecError>> {
    macro_rules! admit {
        ($result:expr) => {
            if let Err(error) = $result {
                return Some(Err(error));
            }
        };
    }

    if !constraint_maps_supported(constraint) {
        return None;
    }
    let resolve = |reference: PmDcReference| {
        entities
            .get(&(
                constraint.identity.segment_token.as_str(),
                reference.index().checked_sub(1)?,
            ))
            .copied()
    };
    let (definition, orientation, members) = match constraint.kind {
        PmDcSketchConstraintKind::SplinePoint { spline, point } => {
            let members = [resolve(spline)?, resolve(point)?];
            (
                SketchConstraintDefinitionInput::Native {
                    native_kind: cadmpeg_core::nonblank_literal!("spline_point"),
                    native_state: Some(u64::from(constraint.header.state.cast_unsigned())),
                    native_flags: Some(u64::from(constraint.header.content.flags)),
                    native_properties: std::collections::BTreeMap::new(),
                    entities: members.iter().map(|entity| entity.id().clone()).collect(),
                    parameter: None,
                    operands: vec![
                        native_operand(
                            constraint,
                            cadmpeg_core::nonblank_literal!("spline"),
                            spline,
                        ),
                        native_operand(constraint, cadmpeg_core::nonblank_literal!("point"), point),
                    ],
                },
                None,
                members,
            )
        }
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
            // The solver hint locates the tangency; it does not change the relation.
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
            let parameter =
                resolve_parameter(constraint, constraint.header.parameter?, parameters)?;
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
            let parameter =
                resolve_parameter(constraint, constraint.header.parameter?, parameters)?;
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
                    native_kind: cadmpeg_core::text::NonBlankString::new(
                        "circle_center_alignment",
                    )?,
                    native_state: Some(u64::from(constraint.header.state.cast_unsigned())),
                    native_flags: Some(u64::from(constraint.header.content.flags)),
                    native_properties: std::collections::BTreeMap::new(),
                    entities: members.iter().map(|entity| entity.id().clone()).collect(),
                    parameter: None,
                    operands: vec![
                        native_operand(
                            constraint,
                            cadmpeg_core::nonblank_literal!("entity"),
                            entity,
                        ),
                        native_operand(
                            constraint,
                            cadmpeg_core::nonblank_literal!("center"),
                            center,
                        ),
                    ],
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::EqualRadius { first, second } => {
            let members = [resolve(first)?, resolve(second)?];
            if !members.iter().all(|member| {
                matches!(
                    *member.geometry.definition(),
                    SketchGeometryDefinition::Circle { .. } | SketchGeometryDefinition::Arc { .. }
                )
            }) {
                return None;
            }
            (
                SketchConstraintDefinitionInput::Equal {
                    first: members[0].id().clone(),
                    second: members[1].id().clone(),
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::EqualLength { first, second } => {
            let members = [resolve(first)?, resolve(second)?];
            if !members.iter().all(|member| {
                matches!(
                    *member.geometry.definition(),
                    SketchGeometryDefinition::Line { .. }
                )
            }) {
                return None;
            }
            (
                SketchConstraintDefinitionInput::Equal {
                    first: members[0].id().clone(),
                    second: members[1].id().clone(),
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::Distance { first, second, .. } => {
            let members = [resolve(first)?, resolve(second)?];
            let parameter =
                resolve_parameter(constraint, constraint.header.parameter?, parameters)?;
            let definition = if measures_axis_line(&members, axis_lines) {
                SketchConstraintDefinitionInput::Native {
                    native_kind: cadmpeg_core::nonblank_literal!("axis_distance_dimension"),
                    native_state: Some(u64::from(constraint.header.state.cast_unsigned())),
                    native_flags: Some(u64::from(constraint.header.content.flags)),
                    native_properties: std::collections::BTreeMap::new(),
                    entities: members.iter().map(|entity| entity.id().clone()).collect(),
                    parameter: Some(parameter),
                    operands: vec![
                        native_operand(constraint, cadmpeg_core::nonblank_literal!("first"), first),
                        native_operand(
                            constraint,
                            cadmpeg_core::nonblank_literal!("second"),
                            second,
                        ),
                    ],
                }
            } else {
                SketchConstraintDefinitionInput::DistanceLoci {
                    first: SketchLocus::Entity(members[0].id().clone()),
                    second: SketchLocus::Entity(members[1].id().clone()),
                    parameter,
                }
            };
            (definition, None, members)
        }
        PmDcSketchConstraintKind::Angle {
            first,
            second,
            flags,
            ..
        } => {
            let members = [resolve(first)?, resolve(second)?];
            let parameter =
                resolve_parameter(constraint, constraint.header.parameter?, parameters)?;
            (
                SketchConstraintDefinitionInput::Native {
                    native_kind: cadmpeg_core::text::NonBlankString::new("angle_dimension")?,
                    native_state: Some(u64::from(constraint.header.state.cast_unsigned())),
                    // The record's own flags hold the supplement selection.
                    native_flags: Some(u64::from(flags)),
                    native_properties: std::collections::BTreeMap::new(),
                    entities: members.iter().map(|entity| entity.id().clone()).collect(),
                    parameter: Some(parameter),
                    operands: vec![
                        native_operand(constraint, cadmpeg_core::nonblank_literal!("first"), first),
                        native_operand(
                            constraint,
                            cadmpeg_core::nonblank_literal!("second"),
                            second,
                        ),
                    ],
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::Midpoint { entity, point, .. } => {
            let members = [resolve(point)?, resolve(entity)?];
            (
                curve::midpoint_definition(constraint, entity, point, members[0], members[1]),
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::Fix { entity } => {
            let member = resolve(entity)?;
            (
                SketchConstraintDefinitionInput::Fixed {
                    entity: member.id().clone(),
                },
                None,
                [member, member],
            )
        }
        PmDcSketchConstraintKind::HorizontalAlignment { first, second }
        | PmDcSketchConstraintKind::VerticalAlignment { first, second } => {
            let members = [resolve(first)?, resolve(second)?];
            let axis = if matches!(
                constraint.kind,
                PmDcSketchConstraintKind::HorizontalAlignment { .. }
            ) {
                cadmpeg_ir::sketches::SketchCoordinateAxis::V
            } else {
                cadmpeg_ir::sketches::SketchCoordinateAxis::U
            };
            (
                SketchConstraintDefinitionInput::SameCoordinate {
                    relation: cadmpeg_ir::sketches::SketchSameCoordinate::try_new(
                        SketchLocus::Entity(members[0].id().clone()),
                        SketchLocus::Entity(members[1].id().clone()),
                        axis,
                    )
                    .ok()?,
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::Collinear {
            first,
            second,
            state,
        } => {
            let members = [resolve(first)?, resolve(second)?];
            (
                SketchConstraintDefinitionInput::Collinear {
                    first: members[0].id().clone(),
                    second: members[1].id().clone(),
                },
                Some(u32::from(state)),
                members,
            )
        }
        PmDcSketchConstraintKind::EqualDistance { first, second } => {
            let pair = |[a, b]: [PmDcReference; 2]| Some([resolve(a)?, resolve(b)?]);
            let (first, second) = (pair(first)?, pair(second)?);
            if second
                .iter()
                .chain(&first[1..])
                .any(|entity| entity.sketch != first[0].sketch)
            {
                return None;
            }
            let locus = |entity: &SketchEntity| SketchLocus::Entity(entity.id().clone());
            (
                SketchConstraintDefinitionInput::EqualDistance {
                    first: cadmpeg_ir::sketches::SketchDistancePair {
                        first: locus(first[0]),
                        second: locus(first[1]),
                    },
                    second: cadmpeg_ir::sketches::SketchDistancePair {
                        first: locus(second[0]),
                        second: locus(second[1]),
                    },
                },
                None,
                first,
            )
        }
        PmDcSketchConstraintKind::BlockGroup { ref members } => {
            let entities = members
                .references()
                .iter()
                .map(|member| resolve(*member))
                .collect::<Option<Vec<_>>>()?;
            let (first, rest) = entities.split_first()?;
            if rest.iter().any(|entity| entity.sketch != first.sketch) {
                return None;
            }
            (
                SketchConstraintDefinitionInput::Native {
                    native_kind: cadmpeg_core::text::NonBlankString::new("sketch_block")?,
                    native_state: Some(u64::from(constraint.header.state.cast_unsigned())),
                    native_flags: Some(u64::from(constraint.header.content.flags)),
                    native_properties: std::collections::BTreeMap::new(),
                    entities: entities.iter().map(|entity| entity.id().clone()).collect(),
                    parameter: None,
                    operands: members
                        .references()
                        .iter()
                        .map(|member| {
                            native_operand(
                                constraint,
                                cadmpeg_core::nonblank_literal!("member"),
                                *member,
                            )
                        })
                        .collect(),
                },
                None,
                [*first, *first],
            )
        }
        PmDcSketchConstraintKind::PolygonRelation {
            first,
            second,
            reference,
            polygon,
            ..
        } => {
            let members = [resolve(first)?, resolve(second)?];
            let [polygon, repeated] = polygon;
            if polygon != repeated {
                return None;
            }
            let (reference, polygon) = (resolve(reference)?, resolve(polygon)?);
            if [reference, polygon]
                .iter()
                .any(|entity| entity.sketch != members[0].sketch)
            {
                return None;
            }
            (
                SketchConstraintDefinitionInput::Polygon {
                    polygon: cadmpeg_ir::sketches::SketchPolygon::try_new(
                        [members[0], members[1], reference, polygon]
                            .iter()
                            .map(|entity| entity.id().clone())
                            .collect(),
                    )
                    .ok()?,
                },
                None,
                members,
            )
        }
        PmDcSketchConstraintKind::Symmetric {
            first,
            second,
            axis,
        } => {
            let members = [resolve(first)?, resolve(second)?];
            let axis = resolve(axis)?;
            if axis.sketch != members[0].sketch {
                return None;
            }
            (
                SketchConstraintDefinitionInput::Symmetric {
                    first: SketchLocus::Entity(members[0].id().clone()),
                    second: SketchLocus::Entity(members[1].id().clone()),
                    axis: axis.id().clone(),
                },
                None,
                members,
            )
        }
    };
    if members[0].sketch != members[1].sketch {
        return None;
    }
    admit!(ctx.charge_entities(1, "project Inventor sketch constraint"));
    Some(Ok(SketchConstraint {
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
    }))
}

/// Largest unqualified reference-map value of a tangent relation's solver
/// hint: 0, 1, or 2.
const TANGENT_HINT_MAX_CODE: u32 = 2;

/// Whether the constraint maps hold only the relation's solver hint: in each
/// map, at most one entry for each of its two operands. Reference-map values
/// are null; a tangent relation's values can also be the unqualified codes 1
/// and 2.
fn solver_hint(constraint: &PmDcSketchConstraint) -> bool {
    let (operands, max_code) = match constraint.kind {
        PmDcSketchConstraintKind::Coincident { first, second }
        | PmDcSketchConstraintKind::Distance { first, second, .. } => ([first, second], 0),
        PmDcSketchConstraintKind::Tangent { first, second, .. } => {
            ([first, second], TANGENT_HINT_MAX_CODE)
        }
        PmDcSketchConstraintKind::CircleCenter { entity, center } => ([entity, center], 0),
        PmDcSketchConstraintKind::Midpoint { entity, point, .. } => ([entity, point], 0),
        _ => return false,
    };
    let header = &constraint.header;
    distinct_operand_keys(
        header.scalar_map.entries().iter().map(|(key, _)| *key),
        operands,
    ) && header
        .reference_map
        .entries()
        .iter()
        .all(|(_, value)| !value.qualified() && value.index() <= max_code)
        && distinct_operand_keys(
            header.reference_map.entries().iter().map(|(key, _)| *key),
            operands,
        )
}

/// Whether every key names one of the two operands, and no operand twice.
fn distinct_operand_keys(
    keys: impl IntoIterator<Item = PmDcReference>,
    operands: [PmDcReference; 2],
) -> bool {
    let mut seen = [false; 2];
    keys.into_iter().all(|key| {
        key.index() != 0
            && operands
                .iter()
                .position(|operand| *operand == key)
                .is_some_and(|slot| !std::mem::replace(&mut seen[slot], true))
    })
}

fn resolve_parameter(
    constraint: &PmDcSketchConstraint,
    reference: PmDcReference,
    parameters: &HashMap<String, ParameterId>,
) -> Option<ParameterId> {
    let ordinal = reference.index().checked_sub(1)?;
    let native = format!(
        "inventor:pmdc:parameter#{}-{}",
        constraint.identity.segment_token, ordinal
    );
    parameters.get(&native).cloned()
}

fn native_operand(
    constraint: &PmDcSketchConstraint,
    field: cadmpeg_core::text::NonBlankString,
    reference: PmDcReference,
) -> SketchNativeOperand {
    SketchNativeOperand {
        native_kind: cadmpeg_core::nonblank_literal!("record_reference"),
        field: Some(NativeOperandField {
            name: field,
            role: None,
        }),
        object_index: Some(reference.index()),
        native_ref: reference.index().checked_sub(1).map(|ordinal| {
            format!(
                "inventor:pmdc:sketch-entity#{}-{ordinal}",
                constraint.identity.segment_token
            )
        }),
    }
}

pub(crate) fn project_geometry(
    entity: &PmDcSketchEntity,
    entities: &HashMap<(&str, u32), Option<&PmDcSketchEntity>>,
) -> Option<SketchGeometry> {
    project_placed_geometry(entity, entities, &BlockPlacement::IDENTITY)
}

/// The solved geometry of an entity, moved by a block placement.
fn project_placed_geometry(
    entity: &PmDcSketchEntity,
    entities: &HashMap<(&str, u32), Option<&PmDcSketchEntity>>,
    placement: &BlockPlacement,
) -> Option<SketchGeometry> {
    let resolve_point =
        |token: &str, reference: u32| placement.point(resolve_point(token, reference, entities)?);
    match &entity.kind {
        PmDcSketchEntityKind::Spline { .. } => {
            let Some(mut curve) = spline::resolved_curve(entity, entities) else {
                return Some(SketchGeometry::native(cadmpeg_core::nonblank_literal!(
                    "spline"
                )));
            };
            curve
                .try_map_control_points(|_, point| {
                    let point = point.get();
                    let placed = placement
                        .point([
                            FiniteReal::new(point.u).ok_or(())?,
                            FiniteReal::new(point.v).ok_or(())?,
                        ])
                        .ok_or(())?;
                    cadmpeg_ir::units::FinitePoint2::new(Point2::new(
                        placed[0].get() * 10.0,
                        placed[1].get() * 10.0,
                    ))
                    .ok_or(())
                })
                .ok()?;
            Some(SketchGeometry::nurbs(curve))
        }
        PmDcSketchEntityKind::Point { position, .. } => Some(
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: neutral_point(placement.point(*position)?),
            })
            .ok()?,
        ),
        PmDcSketchEntityKind::Line {
            points,
            origin,
            direction,
            ..
        } => {
            let [start, end, ..] = points.references() else {
                return None;
            };
            let start = resolve_point(entity.identity.segment_token.as_str(), start.index())?;
            let end = resolve_point(entity.identity.segment_token.as_str(), end.index())?;
            if !line_carrier_matches(
                placement.point(*origin)?.map(FiniteReal::get),
                placement.direction(*direction)?.map(FiniteReal::get),
                start.map(FiniteReal::get),
                end.map(FiniteReal::get),
            ) {
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
        PmDcSketchEntityKind::Circle {
            points,
            center,
            radius,
            state,
            ..
        } => {
            let token = entity.identity.segment_token.as_str();
            let center = resolve_point(token, center.index())?;
            let definition = match (curve_ends(points)?, *state) {
                (CurveEnds::Closed, CIRCLE_STATE_FULL) => SketchGeometryDefinition::Circle {
                    center: neutral_point(center),
                    radius: Length::new(radius.get() * 10.0)?,
                },
                (CurveEnds::Bounded(start, end), CIRCLE_STATE_ARC) => {
                    let start = resolve_point(token, start.index())?;
                    let end = resolve_point(token, end.index())?;
                    // A mirror reverses the arc's counterclockwise sense.
                    let (start, end) = if placement.mirrored() {
                        (end, start)
                    } else {
                        (start, end)
                    };
                    let start_angle = arc_end_angle(center, radius.get(), start)?;
                    let end_angle = arc_end_angle(center, radius.get(), end)?;
                    let sweep = (end_angle - start_angle).rem_euclid(std::f64::consts::TAU);
                    if sweep == 0.0 {
                        return None;
                    }
                    SketchGeometryDefinition::Arc {
                        center: neutral_point(center),
                        radius: Length::new(radius.get() * 10.0)?,
                        start_angle: Angle::new(start_angle)?,
                        end_angle: Angle::new(start_angle + sweep)?,
                    }
                }
                _ => return None,
            };
            SketchGeometry::try_from(definition).ok()
        }
        PmDcSketchEntityKind::Ellipse {
            center,
            major_direction,
            major_radius,
            minor_radius,
            ..
        } => {
            if !curve::closed_ellipse(entity) {
                return Some(SketchGeometry::native(cadmpeg_core::nonblank_literal!(
                    "ellipse"
                )));
            }
            let center = resolve_point(entity.identity.segment_token.as_str(), center.index())?;
            let major_direction = placement.direction(*major_direction)?;
            let norm = major_direction[0].get().hypot(major_direction[1].get());
            if !norm.is_finite() || norm <= f64::EPSILON {
                return None;
            }
            Some(
                SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
                    center: neutral_point(center),
                    major_angle: Angle::new(
                        major_direction[1].get().atan2(major_direction[0].get()),
                    )?,
                    radii: cadmpeg_ir::sketches::EllipseRadii {
                        major_radius: Length::new(major_radius.get() * 10.0)?,
                        minor_radius: Length::new(minor_radius.get() * 10.0)?,
                    },
                    bounds: None,
                })
                .ok()?,
            )
        }
        PmDcSketchEntityKind::BlockInstance { .. } => Some(SketchGeometry::native(
            cadmpeg_core::nonblank_literal!("sketch_block_instance"),
        )),
        PmDcSketchEntityKind::BlockMember { .. } => Some(SketchGeometry::native(
            cadmpeg_core::nonblank_literal!("sketch_block_member"),
        )),
        PmDcSketchEntityKind::Polygon { .. } => Some(SketchGeometry::native(
            cadmpeg_core::nonblank_literal!("polygon"),
        )),
    }
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
    token: &str,
    reference: u32,
    entities: &HashMap<(&str, u32), Option<&PmDcSketchEntity>>,
) -> Option<[FiniteReal; 2]> {
    let entity = entities
        .get(&(token, reference.checked_sub(1)?))
        .and_then(Option::as_ref)?;
    let PmDcSketchEntityKind::Point { position, .. } = entity.kind else {
        return None;
    };
    Some(position)
}

/// The first two members of a curve's point list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CurveEnds {
    /// Two null references: the curve is closed.
    Closed,
    /// The start and end point references of a bounded curve.
    Bounded(PmDcReference, PmDcReference),
}

fn curve_ends(points: &PmDcReferenceList) -> Option<CurveEnds> {
    let [start, end, ..] = points.references() else {
        return None;
    };
    let null = |reference: &PmDcReference| reference.index() == 0 && !reference.qualified();
    match (null(start), null(end)) {
        (true, true) => Some(CurveEnds::Closed),
        (false, false) if start.index() != 0 && end.index() != 0 => {
            Some(CurveEnds::Bounded(*start, *end))
        }
        _ => None,
    }
}

/// The polar angle of an arc end about the arc center, when the end lies on the circle.
fn arc_end_angle(center: [FiniteReal; 2], radius: f64, end: [FiniteReal; 2]) -> Option<f64> {
    let du = end[0].get() - center[0].get();
    let dv = end[1].get() - center[1].get();
    ((du.hypot(dv) - radius).abs() <= EPS_SKETCH_ARC_END_ON_CIRCLE_CM).then(|| dv.atan2(du))
}

fn neutral_point(value: [FiniteReal; 2]) -> Point2 {
    Point2::new(value[0].get() * 10.0, value[1].get() * 10.0)
}

pub(crate) fn entity_endpoint_refs(
    entity: &PmDcSketchEntity,
    entities: &HashMap<(&str, u32), Option<&PmDcSketchEntity>>,
) -> Vec<String> {
    placed_endpoint_refs(entity, entities, &BlockPlacement::IDENTITY)
}

/// The endpoint records of an entity in the direction of its placed geometry.
fn placed_endpoint_refs(
    entity: &PmDcSketchEntity,
    entities: &HashMap<(&str, u32), Option<&PmDcSketchEntity>>,
    placement: &BlockPlacement,
) -> Vec<String> {
    let ends = match &entity.kind {
        PmDcSketchEntityKind::Spline { data }
            if spline::resolved_curve(entity, entities).is_some_and(|curve| !curve.periodic()) =>
        {
            data.points.references()
        }
        PmDcSketchEntityKind::Line { points, .. } => points.references().get(..2).unwrap_or(&[]),
        PmDcSketchEntityKind::Circle { points, .. } => match curve_ends(points) {
            Some(CurveEnds::Bounded(..)) => &points.references()[..2],
            _ => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    let mut endpoint_refs = Vec::new();
    for reference in ends {
        if let Some(value) = reference.index().checked_sub(1).and_then(|ordinal| {
            entities
                .get(&(entity.identity.segment_token.as_str(), ordinal))
                .and_then(Option::as_ref)
        }) {
            endpoint_refs.push(value.id());
        }
    }
    if placement.mirrored() && matches!(entity.kind, PmDcSketchEntityKind::Circle { .. }) {
        endpoint_refs.reverse();
    }
    endpoint_refs
}

pub(crate) fn project_placement(
    sketch: &PmDcSketch,
    transforms: &HashMap<(&str, u32), Option<&PmDcTransform>>,
    directions: &HashMap<(&str, u32), Option<&PmDcDirection>>,
) -> Option<SketchPlacement> {
    let transform = transforms
        .get(&(
            sketch.identity.segment_token.as_str(),
            sketch.transform.index().checked_sub(1)?,
        ))
        .and_then(Option::as_ref)?;
    let direction = directions
        .get(&(
            sketch.identity.segment_token.as_str(),
            sketch.direction.index().checked_sub(1)?,
        ))
        .and_then(Option::as_ref)?;
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
        direction.direction[0].get(),
        direction.direction[1].get(),
        direction.direction[2].get(),
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
    let mut profiles = Vec::new();
    for entity in entities
        .iter()
        .filter(|entity| !entity.construction)
        .filter(|entity| {
            matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Circle { .. } | SketchGeometryDefinition::Ellipse { .. }
            ) || spline::is_closed(entity)
        })
    {
        profiles.push(vec![SketchEntityUse {
            entity: entity.id().clone(),
            reversed: false,
        }]);
    }
    let lines = entities
        .iter()
        .copied()
        .filter(|entity| !entity.construction)
        .filter(|entity| {
            matches!(
                *entity.geometry.definition(),
                SketchGeometryDefinition::Line { .. } | SketchGeometryDefinition::Arc { .. }
            )
        })
        .filter(|entity| entity.endpoint_refs.len() == 2)
        .collect::<Vec<_>>();
    let mut adjacency = HashMap::<&str, Vec<usize>>::new();
    for (index, line) in lines.iter().enumerate() {
        for endpoint in &line.endpoint_refs {
            adjacency.entry(endpoint.as_str()).or_default().push(index);
        }
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
            for index in component {
                visited.insert(index);
            }
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
        // Two lines between the same two points bound no area; an arc with
        // one other curve does.
        let encloses = loop_uses.len() >= 3
            || (loop_uses.len() == 2
                && component.iter().any(|index| {
                    matches!(
                        *lines[*index].geometry.definition(),
                        SketchGeometryDefinition::Arc { .. }
                    )
                }));
        for index in component {
            visited.insert(index);
        }
        if encloses && point == start_point {
            profiles.push(loop_uses);
        }
    }
    profiles.sort_unstable_by(|left, right| {
        let key = |profile: &Vec<SketchEntityUse>| {
            let first = profile
                .iter()
                .filter_map(|entity| source_positions.get(entity.entity.as_str()))
                .copied()
                .min();
            // A profile whose entities state no source position sorts after every
            // profile that states one.
            (first.is_none(), first)
        };
        key(left).cmp(&key(right))
    });
    profiles
}

fn line_component(
    start: usize,
    lines: &[&SketchEntity],
    adjacency: &HashMap<&str, Vec<usize>>,
) -> HashSet<usize> {
    let mut component = HashSet::from([start]);
    let mut expanded = HashSet::new();
    let mut pending = vec![start];
    while let Some(index) = pending.pop() {
        for point in &lines[index].endpoint_refs {
            if !expanded.insert(point.as_str()) {
                continue;
            }
            for &neighbour in adjacency.get(point.as_str()).into_iter().flatten() {
                if component.insert(neighbour) {
                    pending.push(neighbour);
                }
            }
        }
    }
    component
}

/// A rigid placement of block-definition geometry in its consuming sketch,
/// in internal centimetres.
#[derive(Debug, Clone, Copy)]
struct BlockPlacement {
    linear: [[f64; 2]; 2],
    offset: [f64; 2],
}

impl BlockPlacement {
    const IDENTITY: Self = Self {
        linear: [[1.0, 0.0], [0.0, 1.0]],
        offset: [0.0, 0.0],
    };

    /// The placement of a block-instance matrix, when the matrix is a rigid
    /// motion of the plane.
    fn new(matrix: &[[FiniteReal; 3]; 3]) -> Option<Self> {
        let m = matrix.map(|row| row.map(FiniteReal::get));
        let near = |value: f64, expected: f64| {
            (value - expected).abs() <= EPS_SKETCH_BLOCK_PLACEMENT_RIGID
        };
        let rigid = near(m[2][0], 0.0)
            && near(m[2][1], 0.0)
            && near(m[2][2], 1.0)
            && near(m[0][0].hypot(m[1][0]), 1.0)
            && near(m[0][1].hypot(m[1][1]), 1.0)
            && near(m[0][0].mul_add(m[0][1], m[1][0] * m[1][1]), 0.0);
        rigid.then_some(Self {
            linear: [[m[0][0], m[0][1]], [m[1][0], m[1][1]]],
            offset: [m[0][2], m[1][2]],
        })
    }

    /// Whether the placement reverses orientation.
    fn mirrored(&self) -> bool {
        self.linear[0][0].mul_add(self.linear[1][1], -(self.linear[0][1] * self.linear[1][0])) < 0.0
    }

    fn direction(&self, value: [FiniteReal; 2]) -> Option<[FiniteReal; 2]> {
        let [x, y] = value.map(FiniteReal::get);
        Some([
            FiniteReal::new(self.linear[0][0].mul_add(x, self.linear[0][1] * y))?,
            FiniteReal::new(self.linear[1][0].mul_add(x, self.linear[1][1] * y))?,
        ])
    }

    fn point(&self, value: [FiniteReal; 2]) -> Option<[FiniteReal; 2]> {
        let [x, y] = self.direction(value)?.map(FiniteReal::get);
        Some([
            FiniteReal::new(x + self.offset[0])?,
            FiniteReal::new(y + self.offset[1])?,
        ])
    }
}

/// The curves of a block instance's definition, in record order.
pub(crate) fn block_definition_curves<'a>(
    instance: &PmDcSketchEntity,
    entities: impl IntoIterator<Item = &'a PmDcSketchEntity>,
) -> Vec<&'a PmDcSketchEntity> {
    let PmDcSketchEntityKind::BlockInstance { definition, .. } = instance.kind else {
        return Vec::new();
    };
    let mut curves = entities
        .into_iter()
        .filter(|entity| {
            entity.identity.segment_token == instance.identity.segment_token
                && entity.sketch.index() == definition.index()
                && matches!(
                    entity.kind,
                    PmDcSketchEntityKind::Line { .. }
                        | PmDcSketchEntityKind::Circle { .. }
                        | PmDcSketchEntityKind::Ellipse { .. }
                )
        })
        .collect::<Vec<_>>();
    curves.sort_by_key(|entity| entity.identity.record_ordinal);
    curves
}

/// The offset of each definition curve, in `curves` order, from the first
/// associative id that a block instance gives its placed geometry.
///
/// Full circles come first, one id each. Each other curve then takes one id,
/// followed by one id for each of its two end points that has none yet.
pub(crate) fn block_curve_offsets(curves: &[&PmDcSketchEntity]) -> Option<Vec<u32>> {
    let full_circle = |entity: &PmDcSketchEntity| {
        matches!(
            &entity.kind,
            PmDcSketchEntityKind::Circle { points, .. }
                if curve_ends(points) == Some(CurveEnds::Closed)
        )
    };
    let mut offsets = vec![0; curves.len()];
    let mut next = 0u32;
    for (offset, _) in offsets
        .iter_mut()
        .zip(curves)
        .filter(|(_, curve)| full_circle(curve))
    {
        *offset = next;
        next = next.checked_add(1)?;
    }
    let mut numbered = HashSet::new();
    for (offset, curve) in offsets.iter_mut().zip(curves) {
        if full_circle(curve) {
            continue;
        }
        let (PmDcSketchEntityKind::Line { points, .. }
        | PmDcSketchEntityKind::Circle { points, .. }) = &curve.kind
        else {
            return None;
        };
        let [start, end, ..] = points.references() else {
            return None;
        };
        *offset = next;
        next = next.checked_add(1)?;
        for point in [start, end] {
            if numbered.insert(point.index()) {
                next = next.checked_add(1)?;
            }
        }
    }
    Some(offsets)
}

/// The neutral id of one definition curve placed by one block instance.
pub(crate) fn block_curve_id(
    instance: &PmDcSketchEntity,
    curve: &PmDcSketchEntity,
) -> Option<SketchEntityId> {
    SketchEntityId::mint(format!(
        "inventor:design:sketch-entity#{}-{}-{}",
        instance.identity.segment_token,
        instance.identity.record_ordinal,
        curve.identity.record_ordinal
    ))
    .ok()
}

/// The definition curves that one projected block instance places in its
/// sketch, or nothing when one of them does not project.
fn place_block_curves(
    instance: &PmDcSketchEntity,
    sketch: &SketchId,
    inventory: &SketchInventory,
    entities: &HashMap<(&str, u32), Option<&PmDcSketchEntity>>,
) -> Option<Vec<SketchEntity>> {
    let PmDcSketchEntityKind::BlockInstance { placement, .. } = &instance.kind else {
        return None;
    };
    let placement = BlockPlacement::new(placement)?;
    block_definition_curves(instance, &inventory.entities)
        .into_iter()
        .map(|curve| {
            Some(
                SketchEntity::new(
                    block_curve_id(instance, curve)?,
                    sketch.clone(),
                    project_placed_geometry(curve, entities, &placement)?,
                )
                .with_construction(marks_construction(curve))
                .with_native_ref(Some(curve.id()))
                .with_endpoint_refs(placed_endpoint_refs(curve, entities, &placement)),
            )
        })
        .collect()
}

fn sketch_id(sketch: &PmDcSketch) -> Option<SketchId> {
    SketchId::mint(format!(
        "inventor:design:sketch#{}-{}",
        sketch.identity.segment_token, sketch.identity.record_ordinal
    ))
    .ok()
}

pub(crate) fn entity_id(entity: &PmDcSketchEntity) -> Option<SketchEntityId> {
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
    use super::{
        block_curve_offsets, block_definition_curves, distinct_operand_keys, entity_endpoint_refs,
        inventory, parse_constraint, parse_direction, parse_entity, parse_sketch, parse_transform,
        placed_endpoint_refs, project, project_placed_geometry, solver_hint, BlockPlacement,
        PmDcSketchConstraint, PmDcSketchConstraintKind, PmDcSketchEntity, PmDcSketchEntityKind,
        PmDcSketchEntityPayload, PmDcSketchPayload, PmDcTransformPayload, PointTail,
        SketchConstraintTag, SketchEntityTag, SketchInventory, BLOCK_INSTANCE_TYPE, CIRCLE_TYPE,
        COINCIDENT_TYPE, DIRECTION_TYPE, HORIZONTAL_TYPE, LINE_TYPE, MIDPOINT_TYPE, POINT_TYPE,
        SKETCH_TYPE, TRANSFORM_TYPE,
    };
    use crate::container::InventorContainer;
    use crate::pmdc::{PmDcReference, PmDcReferenceList};
    use crate::record_identity::Located;
    use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
    use crate::test_support::test_fixtures::{
        content, content_at, parse, primary_envelope_fixture,
    };
    use cadmpeg_core::decode::unique_index;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::scalar::FiniteReal;
    use cadmpeg_ir::sketches::SketchPlacement;

    fn real(value: f64) -> FiniteReal {
        FiniteReal::new(value).expect("finite test scalar")
    }

    #[test]
    fn profile_builder_projects_a_circle() {
        let entity = circle_profile_entity();
        assert_eq!(super::build_profiles(&[&entity]).len(), 1);
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
    fn line_component_collects_a_single_line() {
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
        assert_eq!(super::line_component(0, &[&entity], &adjacency).len(), 1);
    }

    #[test]
    fn lines_sharing_one_endpoint_form_one_component_and_no_profile() {
        const LINES: u32 = 1 << 14;
        let lines = (0..LINES)
            .map(|ordinal| {
                cadmpeg_ir::sketches::SketchEntity::new(
                    cadmpeg_ir::sketches::SketchEntityId::mint(format!(
                        "inventor:test:entity#{ordinal}"
                    ))
                    .expect("entity id"),
                    cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1")
                        .expect("sketch id"),
                    cadmpeg_ir::sketches::SketchGeometry::try_from(
                        cadmpeg_ir::sketches::SketchGeometryDefinition::Line {
                            start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                            end: cadmpeg_ir::math::Point2::new(1.0, f64::from(ordinal)),
                        },
                    )
                    .expect("line geometry"),
                )
                .with_endpoint_refs(vec!["hub".to_owned(), format!("tip-{ordinal}")])
            })
            .collect::<Vec<_>>();
        let entities = lines.iter().collect::<Vec<_>>();
        let mut adjacency = std::collections::HashMap::<&str, Vec<usize>>::new();
        for (index, line) in lines.iter().enumerate() {
            for endpoint in &line.endpoint_refs {
                adjacency.entry(endpoint.as_str()).or_default().push(index);
            }
        }

        let component = super::line_component(0, &entities, &adjacency);

        assert_eq!(component, (0..entities.len()).collect());
        assert!(super::build_profiles(&entities).is_empty());
    }

    #[test]
    fn sketch_projection_reports_an_entity_outside_any_sketch() {
        let point = parse(&point_bytes(0, 1, [0.0, 0.0]), |_, source| {
            parse_entity(SketchEntityTag::Point, source, 22).expect("point record")
        });
        let inventory = SketchInventory {
            sketches: Vec::new(),
            entities: vec![Located::new(
                point,
                crate::record_identity::RecordTypeId::from_bytes(POINT_TYPE),
                cadmpeg_ir::identity_key!("segment"),
                0,
            )],
            transforms: Vec::new(),
            directions: Vec::new(),
            constraints: Vec::new(),
            issues: Vec::new(),
        };
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("build the decode context");
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
        let sketch = parse(&sketch_bytes, |_, source| {
            parse_sketch(source, 22).expect("sketch record")
        });
        let point = parse(&point_bytes(1, 1, [0.0, 0.0]), |_, source| {
            parse_entity(SketchEntityTag::Point, source, 22).expect("point record")
        });
        let inventory = SketchInventory {
            sketches: vec![Located::new(
                sketch,
                crate::record_identity::RecordTypeId::from_bytes(SKETCH_TYPE),
                cadmpeg_ir::identity_key!("segment"),
                0,
            )],
            entities: vec![Located::new(
                point,
                crate::record_identity::RecordTypeId::from_bytes(POINT_TYPE),
                cadmpeg_ir::identity_key!("segment"),
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
            .expect("build the decode context");
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
        let sketch = parse(&sketch_bytes, |_, source| {
            parse_sketch(source, 22).expect("sketch record")
        });
        let point = parse(&point_bytes(1, 1, [0.0, 0.0]), |_, source| {
            parse_entity(SketchEntityTag::Point, source, 22).expect("point record")
        });
        let inventory = SketchInventory {
            sketches: vec![Located::new(
                sketch,
                crate::record_identity::RecordTypeId::from_bytes(SKETCH_TYPE),
                cadmpeg_ir::identity_key!("segment"),
                0,
            )],
            entities: vec![Located::new(
                point,
                crate::record_identity::RecordTypeId::from_bytes(POINT_TYPE),
                cadmpeg_ir::identity_key!("segment"),
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
            |_, source| parse_transform(source, 22).expect("transform"),
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
            |_, source| parse_direction(source, 22).expect("direction"),
        );
        let mut sketch_bytes = content(2);
        sketch_bytes.extend_from_slice(&0_i32.to_le_bytes());
        sketch_bytes.extend_from_slice(&0_u32.to_le_bytes());
        sketch_bytes.extend(list(8, &[]));
        sketch_bytes.extend_from_slice(&1_u32.to_le_bytes());
        sketch_bytes.extend_from_slice(&2_u32.to_le_bytes());
        sketch_bytes.extend_from_slice(&[0; 8]);
        sketch_bytes.extend(list(2, &[]));
        let sketch = parse(&sketch_bytes, |_, source| {
            parse_sketch(source, 22).expect("sketch")
        });
        let inventory = SketchInventory {
            sketches: vec![Located::new(
                sketch,
                crate::record_identity::RecordTypeId::from_bytes(SKETCH_TYPE),
                cadmpeg_ir::identity_key!("segment"),
                2,
            )],
            entities: Vec::new(),
            transforms: vec![Located::new(
                transform,
                crate::record_identity::RecordTypeId::from_bytes(TRANSFORM_TYPE),
                cadmpeg_ir::identity_key!("segment"),
                0,
            )],
            directions: vec![Located::new(
                direction,
                crate::record_identity::RecordTypeId::from_bytes(DIRECTION_TYPE),
                cadmpeg_ir::identity_key!("segment"),
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
            .expect("build the decode context");
        assert_eq!(
            project(&ctx, &inventory, &[])
                .expect("project the sketch inventory")
                .sketches
                .len(),
            1
        );
    }

    #[test]
    fn sketch_constraint_projection_refuses_entity_limit_before_creation() {
        let mut bytes = constraint_header(0, 0);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.push(1);
        let payload = parse(&bytes, |_, source| {
            parse_constraint(SketchConstraintTag::Horizontal, source, 22)
                .expect("horizontal constraint")
        });
        let constraint = Located::new(
            payload,
            crate::record_identity::RecordTypeId::from_bytes(HORIZONTAL_TYPE),
            cadmpeg_ir::identity_key!("segment"),
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
            super::project_constraint(&ctx, &constraint, &entities, &parameters, &std::collections::HashSet::new()),
            Some(Err(CodecError::ResourceLimit(limit)))
                if limit.dimension == ResourceDimension::Entities
                    && limit.operation == "project Inventor sketch constraint"
        ));
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("build the decode context");
        assert!(matches!(
            super::project_constraint(
                &ctx,
                &constraint,
                &entities,
                &parameters,
                &std::collections::HashSet::new()
            ),
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
    fn sketch_transform_record_is_admitted() {
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
    }

    #[test]
    fn sketch_parse_issue_is_recorded() {
        let admitted = inventory_with_record(TRANSFORM_TYPE, &[], DecodePolicy::service())
            .expect("truncated transform becomes an issue");
        assert_eq!(admitted.issues.len(), 1);
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
                .expect("build the record inventory")
                .issues
                .len(),
            1
        );
    }

    #[test]
    fn sketch_record_forms_are_admitted() {
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
        for (type_id, payload) in [
            (SKETCH_TYPE, sketch),
            (POINT_TYPE, point_bytes(0, 0, [0.0, 0.0])),
            (DIRECTION_TYPE, direction),
            (HORIZONTAL_TYPE, constraint),
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

    fn line_bytes(index: u32, sketch: u32, points: &[u32]) -> Vec<u8> {
        let mut bytes = entity_prefix(index, sketch, 0);
        bytes.extend(list(2, points));
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
    fn parses_newer_segment_version_entity_forms() {
        // A version 29 point: a u32 after the flags, two stamps, and a link
        // list after the association list.
        let mut point = content_at(4, 29, &[-1, 2908]);
        point.extend_from_slice(&0u32.to_le_bytes());
        point.extend_from_slice(&0x8000_0003u32.to_le_bytes());
        for value in [1.0f64, 2.0] {
            point.extend_from_slice(&value.to_le_bytes());
        }
        point.extend(list(2, &[0x8000_0005]));
        point.extend(list(2, &[]));
        point.extend_from_slice(&0u32.to_le_bytes());
        point.extend(list(2, &[]));
        point.extend(list(2, &[0x8000_0006]));
        let parsed = parse(&point, |_, source| {
            parse_entity(SketchEntityTag::Point, source, 29).expect("point")
        });
        assert_eq!(parsed.header.flags_extension, Some(0));
        assert_eq!(parsed.header.stamps, [-1, 2908]);
        assert!(matches!(
            parsed.kind,
            PmDcSketchEntityKind::Point {
                tail: PointTail::Present { links: Some(ref links), .. }, ..
            }
                if links.references().iter().map(|value| value.index()).collect::<Vec<_>>() == [6]
        ));

        // A version 25 line whose closed gate stores no auxiliary list.
        let line = |gate: [u32; 2]| {
            let mut bytes = content_at(7, 25, &[-1, 2502]);
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.extend_from_slice(&0x8000_0003u32.to_le_bytes());
            bytes.extend(list(2, &[0x8000_0004, 0x8000_0005]));
            for value in gate {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            for value in [0.0f64, 0.0, 1.0, 0.0] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes
        };
        let parsed = parse(&line([0, 0]), |_, source| {
            parse_entity(SketchEntityTag::Line, source, 25).expect("line")
        });
        assert!(matches!(
            parsed.kind,
            PmDcSketchEntityKind::Line { ref points, ref auxiliary, .. }
                if points.references().len() == 2 && auxiliary.is_empty()
        ));
        parse(&line([2, 0]), |_, source| {
            assert!(parse_entity(SketchEntityTag::Line, source, 25).is_err());
        });

        // A version 25 circle whose closed gate stores no auxiliary list.
        let mut circle = content_at(8, 25, &[-1, 2502]);
        circle.extend_from_slice(&0u32.to_le_bytes());
        circle.extend_from_slice(&0x8000_0003u32.to_le_bytes());
        circle.extend(list(2, &[]));
        circle.extend_from_slice(&[0; 8]);
        circle.extend_from_slice(&4u32.to_le_bytes());
        circle.extend_from_slice(&2.5f64.to_le_bytes());
        circle.push(1);
        let parsed = parse(&circle, |_, source| {
            parse_entity(SketchEntityTag::Circle, source, 25).expect("gated circle")
        });
        assert!(matches!(
            parsed.kind,
            PmDcSketchEntityKind::Circle { ref auxiliary, radius, .. }
                if auxiliary.is_empty() && radius.get() == 2.5
        ));

        // A version 23 entity stores one stamp; a stamp below -1 is refused.
        let one_stamp = |stamp: i32| {
            let mut bytes = content_at(7, 23, &[stamp]);
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.extend_from_slice(&0x8000_0003u32.to_le_bytes());
            bytes.extend(list(2, &[0x8000_0004, 0x8000_0005]));
            for value in [0.0f64, 0.0, 1.0, 0.0] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes
        };
        let parsed = parse(&one_stamp(2301), |_, source| {
            parse_entity(SketchEntityTag::Line, source, 23).expect("one-stamp line")
        });
        assert_eq!(parsed.header.flags_extension, None);
        assert_eq!(parsed.header.stamps, [2301]);
        parse(&one_stamp(-2), |_, source| {
            assert!(parse_entity(SketchEntityTag::Line, source, 23).is_err());
        });

        // From version 28 the u32 after the flags is 0.
        let mut extended = content_at(4, 29, &[-1, 2908]);
        extended[14..18].copy_from_slice(&1u32.to_le_bytes());
        extended.extend_from_slice(&0u32.to_le_bytes());
        extended.extend_from_slice(&0x8000_0003u32.to_le_bytes());
        for value in [1.0f64, 2.0] {
            extended.extend_from_slice(&value.to_le_bytes());
        }
        extended.extend(list(2, &[]));
        extended.extend(list(2, &[]));
        parse(&extended, |_, source| {
            assert!(parse_entity(SketchEntityTag::Point, source, 29).is_err());
        });
    }

    #[test]
    fn parses_generated_planar_geometry_branches() {
        let point = point_bytes(1, 3, [1.25, -2.5]);
        let parsed = parse(&point, |_, source| {
            parse_entity(SketchEntityTag::Point, source, 22).expect("point")
        });
        assert!(matches!(
            parsed.kind,
            PmDcSketchEntityKind::Point {
                position,
                ..
            } if position.map(FiniteReal::get) == [1.25, -2.5]
        ));

        let line = line_bytes(2, 3, &[4, 5]);
        let parsed = parse(&line, |_, source| {
            parse_entity(SketchEntityTag::Line, source, 22).expect("line")
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
        let parsed = parse(&circle, |_, source| {
            parse_entity(SketchEntityTag::Circle, source, 22).expect("circle")
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
        let parsed = parse(&ellipse, |_, source| {
            parse_entity(SketchEntityTag::Ellipse, source, 22).expect("ellipse")
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
            let parsed = parse(&bytes, |_, source| {
                parse_constraint(type_id, source, 22).expect(expected)
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
            let parsed = parse(&bytes, |_, source| {
                parse_constraint(type_id, source, 22).expect("axis constraint")
            });
            assert_eq!(
                matches!(parsed.kind, PmDcSketchConstraintKind::Horizontal { .. }),
                expected
            );
        }
    }

    #[test]
    fn a_midpoint_relation_ends_with_a_value_from_save_version_20() {
        let mut bytes = constraint_header(9, 0);
        bytes.extend_from_slice(&(0x8000_0000u32 | 6).to_le_bytes());
        bytes.extend_from_slice(&(0x8000_0000u32 | 4).to_le_bytes());
        let parsed = parse(&bytes, |_, source| {
            parse_constraint(SketchConstraintTag::Midpoint, source, 19).expect("version 19")
        });
        assert!(matches!(
            parsed.kind,
            PmDcSketchConstraintKind::Midpoint { value: None, .. }
        ));
        bytes.extend_from_slice(&7u32.to_le_bytes());
        for version in [20, 21] {
            let parsed = parse(&bytes, |_, source| {
                parse_constraint(SketchConstraintTag::Midpoint, source, version)
                    .expect("version 20 or later")
            });
            assert!(matches!(
                parsed.kind,
                PmDcSketchConstraintKind::Midpoint { value: Some(7), .. }
            ));
        }
    }

    #[test]
    fn parses_generated_legacy_constraint_header_without_maps() {
        let mut bytes = legacy_constraint_header(9, 0);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&5u32.to_le_bytes());
        let parsed = parse(&bytes, |_, source| {
            parse_constraint(SketchConstraintTag::Coincident, source, 16)
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
    fn parses_generated_relation_and_dimension_records() {
        let references = |values: &[u32]| {
            values
                .iter()
                .flat_map(|value| (0x8000_0000 | value).to_le_bytes())
                .collect::<Vec<_>>()
        };
        let reals = |values: &[f64]| {
            values
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>()
        };
        let mut distance = 0u32.to_le_bytes().to_vec();
        distance.extend(references(&[4, 5]));
        distance.extend(reals(&[0.0, 0.0, f64::MAX, 0.0, f64::MAX, 0.0]));
        let mut angle = 0u32.to_le_bytes().to_vec();
        angle.extend(references(&[6, 7]));
        angle.extend(reals(&[0.5, 1.5]));
        angle.extend_from_slice(&0x0au16.to_le_bytes());
        let mut collinear = references(&[6, 7]);
        collinear.extend_from_slice(&0u16.to_le_bytes());
        let mut midpoint = references(&[6, 4]);
        midpoint.extend_from_slice(&0u32.to_le_bytes());
        let mut polygon_relation = references(&[4, 5, 6, 7, 7]);
        polygon_relation.extend_from_slice(&1u32.to_le_bytes());
        polygon_relation.extend_from_slice(&0u32.to_le_bytes());
        for (tag, parameter, tail, expected) in [
            (SketchConstraintTag::Distance, 3, distance, "distance"),
            (SketchConstraintTag::Angle, 3, angle, "angle"),
            (SketchConstraintTag::Midpoint, 0, midpoint, "midpoint"),
            (SketchConstraintTag::Fix, 0, references(&[4]), "fix"),
            (
                SketchConstraintTag::HorizontalAlignment,
                0,
                references(&[4, 5]),
                "horizontal_alignment",
            ),
            (
                SketchConstraintTag::VerticalAlignment,
                0,
                references(&[4, 5]),
                "vertical_alignment",
            ),
            (SketchConstraintTag::Collinear, 0, collinear, "collinear"),
            (
                SketchConstraintTag::Symmetric,
                0,
                references(&[4, 5, 6]),
                "symmetric",
            ),
            (
                SketchConstraintTag::PolygonRelation,
                0,
                polygon_relation,
                "polygon_relation",
            ),
            (
                SketchConstraintTag::EqualDistance,
                0,
                references(&[4, 5, 6, 7]),
                "equal_distance",
            ),
        ] {
            let mut bytes = constraint_header(9, parameter);
            bytes.extend(tail);
            let parsed = parse(&bytes, |_, source| {
                parse_constraint(tag, source, 22).expect(expected)
            });
            let form = serde_json::to_value(parsed.kind).expect("constraint kind")["form"].clone();
            assert_eq!(form, expected);
        }
        let mut polygon = entity_prefix(7, 3, 0);
        polygon.extend(references(&[4]));
        polygon.extend_from_slice(&0u32.to_le_bytes());
        let parsed = parse(&polygon, |_, source| {
            parse_entity(SketchEntityTag::Polygon, source, 22).expect("polygon")
        });
        assert!(matches!(
            parsed.kind,
            PmDcSketchEntityKind::Polygon { reference, value: 0 } if reference.index() == 4
        ));

        // A block instance with a translation-only 3-by-3 placement, a member
        // link, and the block group, which has no parameter reference.
        let mut instance = entity_prefix(8, 3, 0);
        instance.extend(references(&[30, 10]));
        instance.extend_from_slice(&0x0111u16.to_le_bytes());
        instance.extend_from_slice(&0x00cau16.to_le_bytes());
        instance.extend(reals(&[2.5, -1.0]));
        instance.extend_from_slice(&1u32.to_le_bytes());
        let parsed = parse(&instance, |_, source| {
            parse_entity(SketchEntityTag::BlockInstance, source, 22).expect("block instance")
        });
        let PmDcSketchEntityKind::BlockInstance { placement, .. } = parsed.kind else {
            panic!("block instance");
        };
        assert_eq!(
            placement.map(|row| row.map(FiniteReal::get)),
            [[1.0, 0.0, 2.5], [0.0, 1.0, -1.0], [0.0, 0.0, 1.0]]
        );
        let mut member = entity_prefix(9, 3, 0);
        member.extend(list(2, &[0x8000_0009]));
        member.extend(references(&[4]));
        let parsed = parse(&member, |_, source| {
            parse_entity(SketchEntityTag::BlockMember, source, 22).expect("block member")
        });
        assert!(matches!(
            parsed.kind,
            PmDcSketchEntityKind::BlockMember { .. }
        ));
        let mut group = constraint_header(10, 0);
        group.truncate(group.len() - 4);
        group.extend(list(2, &[0x8000_0009, 0x8000_000a]));
        let parsed = parse(&group, |_, source| {
            parse_constraint(SketchConstraintTag::BlockGroup, source, 22).expect("block group")
        });
        assert!(parsed.header.parameter.is_none());
        assert!(matches!(
            parsed.kind,
            PmDcSketchConstraintKind::BlockGroup { .. }
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
            parse(&bytes, |_, source| {
                parse_constraint(type_id, source, 22).expect("distance constraint")
            });
        }
        let mut radius = constraint_header(12, 13);
        radius.extend_from_slice(&0u32.to_le_bytes());
        radius.extend_from_slice(&4u32.to_le_bytes());
        radius.extend_from_slice(&[0; 16]);
        parse(&radius, |_, source| {
            parse_constraint(SketchConstraintTag::Radius, source, 22).expect("radius")
        });

        let mut diameter = constraint_header(13, 14);
        diameter.extend_from_slice(&0u32.to_le_bytes());
        diameter.extend_from_slice(&4u32.to_le_bytes());
        diameter.extend_from_slice(&[0; 16]);
        parse(&diameter, |_, source| {
            parse_constraint(SketchConstraintTag::Diameter, source, 22).expect("diameter")
        });

        for type_id in [
            SketchConstraintTag::CircleCenter,
            SketchConstraintTag::EqualRadius,
            SketchConstraintTag::EqualLength,
        ] {
            let mut bytes = constraint_header(14, 0);
            bytes.extend_from_slice(&4u32.to_le_bytes());
            bytes.extend_from_slice(&5u32.to_le_bytes());
            parse(&bytes, |_, source| {
                parse_constraint(type_id, source, 22).expect("circle relation")
            });
        }
    }

    /// Four points and four lines closing a square, listed by one sketch placed
    /// on a resolved plane, and that sketch's payload.
    fn closed_square_inventory() -> (SketchInventory, PmDcSketchPayload) {
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
        let sketch = parse(&sketch_bytes, |_, source| {
            parse_sketch(source, 22).expect("sketch")
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
                    |_, source| parse_entity(SketchEntityTag::Point, source, 22).expect("point"),
                )
            })
            .collect::<Vec<_>>();
        for (index, endpoints) in [[4, 5], [5, 6], [6, 7], [7, 4]].into_iter().enumerate() {
            let mut line = parse(
                &line_bytes(
                    u32::try_from(index).expect("fixture value fits u32") + 7,
                    3,
                    &endpoints,
                ),
                |_, source| parse_entity(SketchEntityTag::Line, source, 22).expect("line"),
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
            crate::record_identity::RecordTypeId::from_bytes(TRANSFORM_TYPE),
            cadmpeg_ir::identity_key!("segment"),
            0,
        );
        let direction = Located::new(
            direction,
            crate::record_identity::RecordTypeId::from_bytes(DIRECTION_TYPE),
            cadmpeg_ir::identity_key!("segment"),
            1,
        );
        let located_sketch = Located::new(
            sketch.clone(),
            crate::record_identity::RecordTypeId::from_bytes(SKETCH_TYPE),
            cadmpeg_ir::identity_key!("segment"),
            2,
        );
        let entities = entities
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let type_id = if index < 4 { POINT_TYPE } else { LINE_TYPE };
                Located::new(
                    value,
                    crate::record_identity::RecordTypeId::from_bytes(type_id),
                    cadmpeg_ir::identity_key!("segment"),
                    u32::try_from(index).expect("fixture value fits u32") + 3,
                )
            })
            .collect();
        let inventory = SketchInventory {
            sketches: vec![located_sketch],
            entities,
            transforms: vec![transform],
            directions: vec![direction],
            constraints: Vec::new(),
            issues: Vec::new(),
        };
        (inventory, sketch)
    }

    #[test]
    fn a_curve_with_an_unknown_flag_bit_stays_out_of_the_profile_table() {
        let (mut inventory, _) = closed_square_inventory();
        let line = inventory
            .entities
            .iter()
            .position(|entity| matches!(entity.kind, PmDcSketchEntityKind::Line { .. }))
            .expect("square side");
        let mut payload = PmDcSketchEntityPayload::clone(&inventory.entities[line]);
        payload.entity_flags = 0x100;
        let identity = inventory.entities[line].identity.clone();
        inventory.entities[line] = Located::new(
            payload,
            identity.type_id.clone(),
            identity.segment_token.clone(),
            identity.record_ordinal,
        );
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        let projected = project(&ctx, &inventory, &[]).expect("sketch projection");
        assert_eq!(projected.unresolved_construction, 1);
        assert_eq!(projected.entities.len(), 8);
        assert!(projected.sketches[0].profiles.is_empty());
    }

    #[test]
    fn open_cubic_projection_requires_a_closed_owning_graph() {
        for pole_count in [4, 5, 8] {
            for incompatible_arc in [false, true] {
                let (mut inventory, mut sketch) = closed_square_inventory();
                inventory.entities =
                    crate::test_support::sketch_curves::open_cubic_entities_with(pole_count, 2.0)
                        .into_iter()
                        .map(|entity| {
                            located(
                                PmDcSketchEntityPayload::clone(&entity),
                                entity.kind.record_type_id(),
                                entity.identity.record_ordinal,
                            )
                        })
                        .collect();
                let mut references = vec![4, 5, 8];
                if incompatible_arc {
                    let arc = parse(&circle_bytes(8, 3, [4, 5], 4, 0), |_, source| {
                        parse_entity(SketchEntityTag::Circle, source, 22).expect("arc")
                    });
                    inventory.entities.push(located(arc, CIRCLE_TYPE, 8));
                    references.push(9);
                }
                let (marker, metadata, _) = sketch.entities.clone().into_parts();
                sketch.entities = PmDcReferenceList::new(
                    marker,
                    metadata,
                    references
                        .into_iter()
                        .map(PmDcReference::from_packed)
                        .collect(),
                )
                .expect("cubic member list");
                inventory.sketches[0] = Located::new(
                    sketch,
                    crate::record_identity::RecordTypeId::from_bytes(SKETCH_TYPE),
                    cadmpeg_ir::identity_key!("segment"),
                    2,
                );
                let arena = DecodeArena::new();
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
                        .expect("projection context");
                let projected = project(&ctx, &inventory, &[]).expect("cubic projection");
                if incompatible_arc {
                    assert_eq!(projected.unresolved_sketches, 1);
                    assert_eq!(projected.unresolved_entities, 4);
                    assert!(projected.sketches.is_empty());
                    assert!(projected.entities.is_empty());
                } else {
                    assert_eq!(projected.unresolved_sketches, 0);
                    assert_eq!(projected.unresolved_entities, 0);
                    assert_eq!(projected.sketches.len(), 1);
                    assert_eq!(projected.entities.len(), 3);
                    assert!(matches!(
                        projected.entities[2].geometry.definition(),
                        cadmpeg_ir::sketches::SketchGeometryDefinition::Nurbs { .. }
                    ));
                }
            }
        }
    }

    #[test]
    fn projects_generated_closed_square_and_resolved_plane() {
        let (mut inventory, sketch) = closed_square_inventory();
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
        // The scalar-map key is not an operand, so the map is not a solver hint.
        mapped_constraint.extend_from_slice(&6u32.to_le_bytes());
        mapped_constraint.extend_from_slice(&0.5f64.to_le_bytes());
        mapped_constraint.extend(list(6, &[]));
        mapped_constraint.extend_from_slice(&0u32.to_le_bytes());
        mapped_constraint.extend_from_slice(&4u32.to_le_bytes());
        mapped_constraint.extend_from_slice(&5u32.to_le_bytes());
        let mapped_constraint = parse(&mapped_constraint, |_, source| {
            parse_constraint(SketchConstraintTag::Coincident, source, 22)
                .expect("mapped coincident")
        });
        inventory.constraints.push(Located::new(
            mapped_constraint,
            crate::record_identity::RecordTypeId::from_bytes(COINCIDENT_TYPE),
            cadmpeg_ir::identity_key!("segment"),
            11,
        ));
        let mut sketch = sketch;
        let (marker, metadata, mut references) = sketch.entities.clone().into_parts();
        references.push(PmDcReference::new(12, false).expect("test reference index fits 31 bits"));
        sketch.entities =
            PmDcReferenceList::new(marker, metadata, references).expect("extended entity list");
        inventory.sketches[0] = Located::new(
            sketch,
            crate::record_identity::RecordTypeId::from_bytes(SKETCH_TYPE),
            cadmpeg_ir::identity_key!("segment"),
            2,
        );
        let incomplete = project(&ctx, &inventory, &[]).expect("incomplete sketch projection");
        assert_eq!(incomplete.unresolved_sketches, 1);
        assert_eq!(incomplete.unresolved_constraints, 1);
        assert!(incomplete.sketches.is_empty());
        assert!(incomplete.entities.is_empty());
    }

    fn circle_bytes(index: u32, sketch: u32, points: [u32; 2], center: u32, state: u8) -> Vec<u8> {
        let mut bytes = entity_prefix(index, sketch, 0);
        bytes.extend(list(2, &points));
        bytes.extend(list(2, &[]));
        bytes.extend_from_slice(&center.to_le_bytes());
        bytes.extend_from_slice(&1.0f64.to_le_bytes());
        bytes.push(state);
        bytes
    }

    /// A unit half disc: an arc from (1, 0) counterclockwise to (-1, 0) about
    /// the origin, closed by a line back to (1, 0). The line's point list also
    /// names the origin, which lies on the line.
    fn half_disc_inventory() -> SketchInventory {
        let (mut inventory, mut sketch) = closed_square_inventory();
        let (marker, metadata, _) = sketch.entities.clone().into_parts();
        let references = [4, 5, 6, 7, 8, 9]
            .map(|index| PmDcReference::new(index, false).expect("test reference index fits"));
        sketch.entities = PmDcReferenceList::new(marker, metadata, references.to_vec())
            .expect("half-disc entity list");
        inventory.sketches[0] = Located::new(
            sketch,
            crate::record_identity::RecordTypeId::from_bytes(SKETCH_TYPE),
            cadmpeg_ir::identity_key!("segment"),
            2,
        );
        let point = |index, position| {
            parse(&point_bytes(index, 3, position), |_, source| {
                parse_entity(SketchEntityTag::Point, source, 22).expect("point")
            })
        };
        let mut line = parse(&line_bytes(6, 3, &[6, 5, 4]), |_, source| {
            parse_entity(SketchEntityTag::Line, source, 22).expect("line")
        });
        let PmDcSketchEntityKind::Line {
            origin, direction, ..
        } = &mut line.kind
        else {
            unreachable!("generated line")
        };
        *origin = [real(-1.0), real(0.0)];
        *direction = [real(2.0), real(0.0)];
        let arc = parse(&circle_bytes(7, 3, [5, 6], 4, 0), |_, source| {
            parse_entity(SketchEntityTag::Circle, source, 22).expect("arc")
        });
        inventory.entities = [
            (point(3, [0.0, 0.0]), POINT_TYPE),
            (point(4, [1.0, 0.0]), POINT_TYPE),
            (point(5, [-1.0, 0.0]), POINT_TYPE),
            (line, LINE_TYPE),
            (arc, CIRCLE_TYPE),
        ]
        .into_iter()
        .zip(3..)
        .map(|((value, type_id), ordinal)| {
            Located::new(
                value,
                crate::record_identity::RecordTypeId::from_bytes(type_id),
                cadmpeg_ir::identity_key!("segment"),
                ordinal,
            )
        })
        .collect();
        // The origin is the midpoint of the closing line.
        let mut midpoint = constraint_header(8, 0);
        midpoint.extend_from_slice(&(0x8000_0000u32 | 7).to_le_bytes());
        midpoint.extend_from_slice(&(0x8000_0000u32 | 4).to_le_bytes());
        midpoint.extend_from_slice(&0u32.to_le_bytes());
        let midpoint = parse(&midpoint, |_, source| {
            parse_constraint(SketchConstraintTag::Midpoint, source, 22).expect("midpoint")
        });
        inventory.constraints = vec![Located::new(
            midpoint,
            crate::record_identity::RecordTypeId::from_bytes(MIDPOINT_TYPE),
            cadmpeg_ir::identity_key!("segment"),
            8,
        )];
        inventory
    }

    #[test]
    fn projects_a_bounded_circle_record_as_a_counterclockwise_arc_in_a_profile() {
        let inventory = half_disc_inventory();
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        let projected = project(&ctx, &inventory, &[]).expect("sketch projection");
        assert_eq!(projected.unresolved_sketches, 0);
        assert_eq!(projected.unresolved_entities, 0);
        let arc = projected
            .entities
            .iter()
            .find_map(|entity| match *entity.geometry.definition() {
                cadmpeg_ir::sketches::SketchGeometryDefinition::Arc {
                    radius,
                    start_angle,
                    end_angle,
                    ..
                } => Some((radius.get(), start_angle.get(), end_angle.get())),
                _ => None,
            })
            .expect("projected arc");
        assert_eq!(arc, (10.0, 0.0, std::f64::consts::PI));
        assert!(matches!(
            projected
                .constraints
                .iter()
                .map(|constraint| constraint.definition.kind())
                .collect::<Vec<_>>()[..],
            [cadmpeg_ir::sketches::SketchConstraintDefinitionInput::Midpoint { .. }]
        ));
        assert_eq!(projected.sketches[0].profiles.len(), 1);
        assert_eq!(projected.sketches[0].profiles[0].len(), 2);
    }

    fn located(
        value: PmDcSketchEntityPayload,
        type_id: [u8; 16],
        ordinal: u32,
    ) -> PmDcSketchEntity {
        Located::new(
            value,
            crate::record_identity::RecordTypeId::from_bytes(type_id),
            cadmpeg_ir::identity_key!("segment"),
            ordinal,
        )
    }

    /// A block instance in sketch record 2 at `ordinal`, placing definition
    /// record 40 with the explicit cells of `placement`.
    fn block_instance(ordinal: u32, masks: [u16; 2], placement: &[f64]) -> PmDcSketchEntity {
        let mut bytes = entity_prefix(ordinal, 3, 0);
        for reference in [41u32, 50] {
            bytes.extend_from_slice(&(0x8000_0000 | reference).to_le_bytes());
        }
        bytes.extend_from_slice(&masks[0].to_le_bytes());
        bytes.extend_from_slice(&masks[1].to_le_bytes());
        for value in placement {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&1u32.to_le_bytes());
        let parsed = parse(&bytes, |_, source| {
            parse_entity(SketchEntityTag::BlockInstance, source, 22).expect("block instance")
        });
        located(parsed, BLOCK_INSTANCE_TYPE, ordinal)
    }

    /// Block-definition records 20 through 25: a full circle of radius 1
    /// about the definition origin and a line from (1, 0) to (3, 0).
    fn block_definition() -> Vec<PmDcSketchEntity> {
        let point = |ordinal, position| {
            let parsed = parse(&point_bytes(ordinal, 41, position), |_, source| {
                parse_entity(SketchEntityTag::Point, source, 22).expect("point")
            });
            located(parsed, POINT_TYPE, ordinal)
        };
        let circle = parse(&circle_bytes(21, 41, [0, 0], 21, 1), |_, source| {
            parse_entity(SketchEntityTag::Circle, source, 22).expect("circle")
        });
        let mut line = parse(&line_bytes(24, 41, &[23, 24]), |_, source| {
            parse_entity(SketchEntityTag::Line, source, 22).expect("line")
        });
        let PmDcSketchEntityKind::Line {
            origin, direction, ..
        } = &mut line.kind
        else {
            unreachable!("generated line")
        };
        *origin = [real(1.0), real(0.0)];
        *direction = [real(2.0), real(0.0)];
        vec![
            point(20, [0.0, 0.0]),
            located(circle, CIRCLE_TYPE, 21),
            point(22, [1.0, 0.0]),
            point(23, [3.0, 0.0]),
            located(line, LINE_TYPE, 24),
        ]
    }

    #[test]
    fn a_block_instance_places_its_definition_curves_in_its_sketch() {
        let mut inventory = half_disc_inventory();
        let mut sketch = PmDcSketchPayload::clone(&inventory.sketches[0]);
        let (marker, metadata, mut references) = sketch.entities.clone().into_parts();
        references.push(PmDcReference::new(10, false).expect("instance reference"));
        sketch.entities =
            PmDcReferenceList::new(marker, metadata, references).expect("sketch entity list");
        inventory.sketches[0] = Located::new(
            sketch,
            crate::record_identity::RecordTypeId::from_bytes(SKETCH_TYPE),
            cadmpeg_ir::identity_key!("segment"),
            2,
        );
        // A quarter turn counterclockwise, then 5 cm along the sketch u axis.
        inventory
            .entities
            .push(block_instance(9, [0x010a, 0x00d3], &[5.0, 0.0]));
        inventory.entities.extend(block_definition());
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        let projected = project(&ctx, &inventory, &[]).expect("sketch projection");
        assert_eq!(projected.unresolved_sketches, 0);
        let placed = |ordinal: u32| {
            projected
                .entities
                .iter()
                .find(|entity| {
                    entity.id().as_str()
                        == format!("inventor:design:sketch-entity#segment-9-{ordinal}")
                })
                .expect("placed block curve")
        };
        let circle = placed(21);
        assert_eq!(circle.sketch, projected.sketches[0].id);
        assert_eq!(
            circle.native_ref.as_deref(),
            Some("inventor:pmdc:sketch-entity#segment-21")
        );
        assert!(matches!(
            *circle.geometry.definition(),
            cadmpeg_ir::sketches::SketchGeometryDefinition::Circle { center, radius }
                if center == Point2::new(50.0, 0.0) && radius.get() == 10.0
        ));
        assert!(matches!(
            *placed(24).geometry.definition(),
            cadmpeg_ir::sketches::SketchGeometryDefinition::Line { start, end }
                if start == Point2::new(50.0, 10.0) && end == Point2::new(50.0, 30.0)
        ));
        // The placed circle is a profile loop of the consuming sketch.
        assert!(projected.sketches[0]
            .profiles
            .iter()
            .any(|profile| profile.len() == 1 && profile[0].entity == *circle.id()));
    }

    #[test]
    fn a_mirrored_block_placement_reverses_a_placed_arc() {
        let inventory = half_disc_inventory();
        let entities = unique_index(inventory.entities.iter().map(|record| {
            (
                (
                    record.identity.segment_token.as_str(),
                    record.identity.record_ordinal,
                ),
                record,
            )
        }));
        let arc = &inventory.entities[4];
        let mirror = [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]].map(|row| row.map(real));
        let placement = BlockPlacement::new(&mirror).expect("rigid placement");
        let geometry =
            project_placed_geometry(arc, &entities, &placement).expect("placed arc geometry");
        // The upper half arc from (1, 0) to (-1, 0) becomes the lower half arc
        // from (-1, 0) to (1, 0).
        assert!(matches!(
            *geometry.definition(),
            cadmpeg_ir::sketches::SketchGeometryDefinition::Arc { start_angle, end_angle, .. }
                if start_angle.get() == std::f64::consts::PI
                    && end_angle.get() == std::f64::consts::TAU
        ));
        assert_eq!(
            placed_endpoint_refs(arc, &entities, &placement),
            entity_endpoint_refs(arc, &entities)
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
        );
        let shear = [[1.0, 0.5, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].map(|row| row.map(real));
        assert!(BlockPlacement::new(&shear).is_none());
    }

    #[test]
    fn block_curve_offsets_number_full_circles_first() {
        let definition = block_definition();
        let mut arc = parse(&circle_bytes(25, 41, [24, 23], 21, 0), |_, source| {
            parse_entity(SketchEntityTag::Circle, source, 22).expect("arc")
        });
        arc.sketch = PmDcReference::new(41, true).expect("definition reference");
        let second_circle = parse(&circle_bytes(26, 41, [0, 0], 21, 1), |_, source| {
            parse_entity(SketchEntityTag::Circle, source, 22).expect("circle")
        });
        let mut entities = definition;
        entities.push(located(arc, CIRCLE_TYPE, 25));
        entities.push(located(second_circle, CIRCLE_TYPE, 26));
        let instance = block_instance(9, [0x0111, 0x00ca], &[0.0, 0.0]);
        let curves = block_definition_curves(&instance, &entities);
        assert_eq!(
            curves
                .iter()
                .map(|curve| curve.identity.record_ordinal)
                .collect::<Vec<_>>(),
            [21, 24, 25, 26]
        );
        // Circles 21 and 26 take 0 and 1; line 24 takes 2 and its end points
        // 3 and 4; the arc takes 5 and its new end point 6.
        assert_eq!(block_curve_offsets(&curves), Some(vec![0, 2, 5, 1]));
    }

    #[test]
    fn a_solver_hint_map_names_each_operand_at_most_once() {
        let reference = |index| PmDcReference::new(index, true).expect("test reference");
        let operands = [reference(7), reference(4)];
        assert!(distinct_operand_keys([], operands));
        assert!(distinct_operand_keys([reference(7)], operands));
        assert!(distinct_operand_keys(
            [reference(4), reference(7)],
            operands
        ));
        assert!(!distinct_operand_keys(
            [reference(7), reference(7)],
            operands
        ));
        assert!(!distinct_operand_keys([reference(9)], operands));
        assert!(!distinct_operand_keys(
            [PmDcReference::new(0, false).expect("null reference")],
            operands
        ));
    }

    /// A relation keyed by line 8 and point 4 whose maps hold one hint entry
    /// for the line, with reference-map value `code`.
    fn hinted_relation(code: u32) -> PmDcSketchConstraint {
        let mut bytes = content(11);
        bytes.extend_from_slice(&(-1i32).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        let map_prefix = |bytes: &mut Vec<u8>| {
            bytes.extend_from_slice(&6u16.to_le_bytes());
            bytes.extend_from_slice(&0x3000u16.to_le_bytes());
            bytes.extend_from_slice(&1u32.to_le_bytes());
            bytes.extend_from_slice(&[0; 8]);
            bytes.extend_from_slice(&8u32.to_le_bytes());
        };
        map_prefix(&mut bytes);
        bytes.extend_from_slice(&1.0f64.to_le_bytes());
        map_prefix(&mut bytes);
        bytes.extend_from_slice(&code.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&4u32.to_le_bytes());
        let parsed = parse(&bytes, |_, source| {
            parse_constraint(SketchConstraintTag::Coincident, source, 22).expect("relation")
        });
        Located::new(
            parsed,
            crate::record_identity::RecordTypeId::from_bytes(COINCIDENT_TYPE),
            cadmpeg_ir::identity_key!("segment"),
            11,
        )
    }

    /// The same record read as a tangent relation.
    fn as_tangent(relation: &PmDcSketchConstraint) -> PmDcSketchConstraint {
        let mut wire = serde_json::to_value(relation).expect("relation wire");
        wire["kind"] = serde_json::json!({
            "form": "tangent",
            "first": wire["kind"]["first"].clone(),
            "second": wire["kind"]["second"].clone(),
        });
        serde_json::from_value(wire).expect("tangent wire")
    }

    #[test]
    fn a_tangent_hint_can_name_a_curve_end() {
        assert!(solver_hint(&hinted_relation(0)));
        // Only a tangent hint names a curve end.
        assert!(!solver_hint(&hinted_relation(2)));
        assert!(solver_hint(&as_tangent(&hinted_relation(2))));
        assert!(solver_hint(&as_tangent(&hinted_relation(1))));
        assert!(!solver_hint(&as_tangent(&hinted_relation(3))));
    }

    fn test_entity(
        ordinal: u32,
        definition: cadmpeg_ir::sketches::SketchGeometryDefinition,
    ) -> cadmpeg_ir::sketches::SketchEntity {
        cadmpeg_ir::sketches::SketchEntity::new(
            cadmpeg_ir::sketches::SketchEntityId::mint(format!("inventor:test:entity#{ordinal}"))
                .expect("entity id"),
            cadmpeg_ir::sketches::SketchId::mint("inventor:test:sketch#1").expect("sketch id"),
            cadmpeg_ir::sketches::SketchGeometry::try_from(definition).expect("entity geometry"),
        )
    }

    fn test_line() -> cadmpeg_ir::sketches::SketchGeometryDefinition {
        cadmpeg_ir::sketches::SketchGeometryDefinition::Line {
            start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            end: cadmpeg_ir::math::Point2::new(1.0, 0.0),
        }
    }

    fn test_arc() -> cadmpeg_ir::sketches::SketchGeometryDefinition {
        cadmpeg_ir::sketches::SketchGeometryDefinition::Arc {
            center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            radius: cadmpeg_ir::scalar::Length::new(1.0).expect("radius"),
            start_angle: cadmpeg_ir::scalar::Angle::new(0.0).expect("start angle"),
            end_angle: cadmpeg_ir::scalar::Angle::new(1.0).expect("end angle"),
        }
    }

    fn test_circle() -> cadmpeg_ir::sketches::SketchGeometryDefinition {
        cadmpeg_ir::sketches::SketchGeometryDefinition::Circle {
            center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            radius: cadmpeg_ir::scalar::Length::new(1.0).expect("radius"),
        }
    }

    /// Project `constraint` against entities at ordinals 7 and 3 (references
    /// 8 and 4) with the given geometries.
    fn projects_with(
        constraint: &PmDcSketchConstraint,
        first: cadmpeg_ir::sketches::SketchGeometryDefinition,
        second: cadmpeg_ir::sketches::SketchGeometryDefinition,
    ) -> bool {
        let first = test_entity(7, first);
        let second = test_entity(3, second);
        let entities =
            std::collections::HashMap::from([(("segment", 7), &first), (("segment", 3), &second)]);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        match super::project_constraint(
            &ctx,
            constraint,
            &entities,
            &std::collections::HashMap::new(),
            &std::collections::HashSet::new(),
        ) {
            Some(Ok(_)) => true,
            None => false,
            Some(Err(error)) => panic!("projection error: {error}"),
        }
    }

    #[test]
    fn a_tangent_hint_code_does_not_gate_the_relation() {
        for code in [0, 1, 2] {
            let tangent = as_tangent(&hinted_relation(code));
            assert!(projects_with(&tangent, test_line(), test_arc()));
            assert!(projects_with(&tangent, test_arc(), test_line()));
        }
    }

    #[test]
    fn a_tangent_hint_code_is_not_validated_as_a_reference() {
        let coincident = hinted_relation(2);
        let tangent = as_tangent(&coincident);
        let references = crate::validate::sketch_constraint_references;
        // Both kinds store the same operands; only the coincident map value
        // counts as a reference.
        assert_eq!(
            references(&coincident).len(),
            references(&tangent).len() + 1
        );
        assert_eq!(references(&coincident).last(), references(&tangent).last());
    }

    #[test]
    fn equal_relations_require_their_operand_kinds() {
        let equal = |tag| {
            let mut bytes = constraint_header(11, 0);
            bytes.extend_from_slice(&8u32.to_le_bytes());
            bytes.extend_from_slice(&4u32.to_le_bytes());
            let payload = parse(&bytes, |_, source| {
                parse_constraint(tag, source, 22).expect("equal relation")
            });
            Located::new(
                payload,
                crate::record_identity::RecordTypeId::from_bytes(COINCIDENT_TYPE),
                cadmpeg_ir::identity_key!("segment"),
                11,
            )
        };
        let length = equal(SketchConstraintTag::EqualLength);
        assert!(projects_with(&length, test_line(), test_line()));
        assert!(!projects_with(&length, test_line(), test_circle()));
        assert!(!projects_with(&length, test_arc(), test_arc()));
        let radius = equal(SketchConstraintTag::EqualRadius);
        assert!(projects_with(&radius, test_circle(), test_arc()));
        assert!(!projects_with(&radius, test_circle(), test_line()));
    }

    #[test]
    fn a_coincident_relation_with_a_solver_hint_projects() {
        let (mut inventory, mut sketch) = closed_square_inventory();
        // Point 4 lies at the start of line 7; the maps hold the hint keyed
        // by the line.
        let mut hinted = content(11);
        hinted.extend_from_slice(&(-1i32).to_le_bytes());
        hinted.extend_from_slice(&0u32.to_le_bytes());
        let map_prefix = |bytes: &mut Vec<u8>| {
            bytes.extend_from_slice(&6u16.to_le_bytes());
            bytes.extend_from_slice(&0x3000u16.to_le_bytes());
            bytes.extend_from_slice(&1u32.to_le_bytes());
            bytes.extend_from_slice(&[0; 8]);
            bytes.extend_from_slice(&8u32.to_le_bytes());
        };
        map_prefix(&mut hinted);
        hinted.extend_from_slice(&0.0f64.to_le_bytes());
        map_prefix(&mut hinted);
        hinted.extend_from_slice(&0u32.to_le_bytes());
        hinted.extend_from_slice(&0u32.to_le_bytes());
        hinted.extend_from_slice(&8u32.to_le_bytes());
        hinted.extend_from_slice(&4u32.to_le_bytes());
        let hinted = parse(&hinted, |_, source| {
            parse_constraint(SketchConstraintTag::Coincident, source, 22).expect("coincident")
        });
        assert_eq!(hinted.header.scalar_map.entries().len(), 1);
        assert_eq!(hinted.header.reference_map.entries().len(), 1);
        inventory.constraints.push(Located::new(
            hinted,
            crate::record_identity::RecordTypeId::from_bytes(COINCIDENT_TYPE),
            cadmpeg_ir::identity_key!("segment"),
            11,
        ));
        let (marker, metadata, mut references) = sketch.entities.clone().into_parts();
        references.push(PmDcReference::new(12, false).expect("test reference index fits 31 bits"));
        sketch.entities =
            PmDcReferenceList::new(marker, metadata, references).expect("extended entity list");
        inventory.sketches[0] = Located::new(
            sketch,
            crate::record_identity::RecordTypeId::from_bytes(SKETCH_TYPE),
            cadmpeg_ir::identity_key!("segment"),
            2,
        );
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        let projected = project(&ctx, &inventory, &[]).expect("sketch projection");
        assert_eq!(projected.unresolved_sketches, 0);
        assert_eq!(projected.unresolved_constraints, 0);
        assert_eq!(projected.constraints.len(), 1);
    }

    /// The closed square with a horizontal constraint on its first line, the
    /// constraint listed by the sketch.
    fn constrained_square_inventory() -> SketchInventory {
        let (mut inventory, mut sketch) = closed_square_inventory();
        let mut horizontal = constraint_header(11, 0);
        horizontal.extend_from_slice(&8_u32.to_le_bytes());
        horizontal.push(1);
        let horizontal = parse(&horizontal, |_, source| {
            parse_constraint(SketchConstraintTag::Horizontal, source, 22)
                .expect("horizontal constraint")
        });
        inventory.constraints.push(Located::new(
            horizontal,
            crate::record_identity::RecordTypeId::from_bytes(HORIZONTAL_TYPE),
            cadmpeg_ir::identity_key!("segment"),
            11,
        ));
        let (marker, metadata, mut references) = sketch.entities.clone().into_parts();
        references.push(PmDcReference::new(12, false).expect("test reference index fits 31 bits"));
        sketch.entities =
            PmDcReferenceList::new(marker, metadata, references).expect("extended entity list");
        inventory.sketches[0] = Located::new(
            sketch,
            crate::record_identity::RecordTypeId::from_bytes(SKETCH_TYPE),
            cadmpeg_ir::identity_key!("segment"),
            2,
        );
        inventory
    }

    #[test]
    fn duplicated_sketch_record_keys_are_unresolved_and_not_projected() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("projection context");
        let unique = project(&ctx, &constrained_square_inventory(), &[]).expect("projection");
        assert_eq!(
            (
                unique.sketches.len(),
                unique.entities.len(),
                unique.constraints.len()
            ),
            (1, 8, 1)
        );

        let duplications: [fn(&mut SketchInventory); 3] = [
            |inventory| inventory.entities.push(inventory.entities[4].clone()),
            |inventory| inventory.sketches.push(inventory.sketches[0].clone()),
            |inventory| inventory.constraints.push(inventory.constraints[0].clone()),
        ];
        for duplicate in duplications {
            let mut inventory = constrained_square_inventory();
            duplicate(&mut inventory);
            let projected = project(&ctx, &inventory, &[]).expect("projection");
            assert!(projected.sketches.is_empty());
            assert!(projected.entities.is_empty());
            assert!(projected.constraints.is_empty());
            assert_eq!(
                (
                    projected.unresolved_sketches,
                    projected.unresolved_entities,
                    projected.unresolved_constraints
                ),
                (
                    inventory.sketches.len(),
                    inventory.entities.len(),
                    inventory.constraints.len()
                )
            );
        }
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

    mod axis_distance;
    mod construction_flags;
    mod point_tail_projection;
    mod serialization;
}
