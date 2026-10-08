// SPDX-License-Identifier: Apache-2.0
//! CATIA-native ownership and design records retained outside the neutral model.

use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::u64_from_index;

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crate::checked::extents_overlap;
use crate::object_graph::extent_contains;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::native::catalogue::{Catalogue, FamilyRow, Phase};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::units::FiniteVector;

pub(crate) mod projection;
use projection::{
    consolidated_edge_nodes, consolidated_edge_runs, consolidated_owner_packets,
    containing_finjpl_segment, external_reference_views, finjpl_family, native_object_graph,
    preview_views, resolve_alias_surface_tags, resolve_owner_chart_support_aliases,
    zero_entity_record, zero_entity_vertex_owner,
};

pub(crate) mod class5b5c;
use class5b5c::CatiaConsolidatedClass5b5cRecord;

pub(crate) mod edge_definition;

mod edge_node;
use edge_node::{
    consolidated_vertex_identities, edge_node_wires, edge_node_wires_charged, load_edge_nodes,
    CatiaConsolidatedEdgeNode, CatiaConsolidatedEdgeNodeWire,
};

pub(crate) mod entity_record;
use entity_record::{
    CatiaEntityObjectProduction, CatiaEntityRecord, CatiaEntityRecordBody, CatiaEntityRecordWire,
    CatiaEntityValueProduction,
};

pub(crate) mod schema_configuration_chain;
use schema_configuration_chain::{
    derive_schema_configuration_row_chains, CatiaSchemaConfigurationRowChain,
};

pub(crate) mod owner_chart;
pub(crate) mod owner_numeric_tail;
use owner_chart::{
    CatiaOwnerChartAddress, CatiaOwnerChartAliasBinding, CatiaOwnerChartBridge,
    CatiaOwnerChartBridgeReference, CatiaOwnerChartCarrier, CatiaOwnerChartRelation,
};
use owner_numeric_tail::CatiaOwnerNumericTail;

use crate::catalog;
use crate::container;
use crate::entity_table;
use crate::families::zero_entity::topology::EdgeEnd;
use crate::legacy_entity;
use crate::object_graph::{
    self, AliasGroupMembership, AliasLead, HeadToken, ListItem, ObjectPayload, PayloadField,
    PayloadSubtype,
};
use crate::value_block;
use crate::wire::records::{ConsolidatedFrameFlag, ConsolidatedFrameWidth, ConsolidatedRecord};

mod wire_views;

fn slice_is_empty<T>(values: &&[T]) -> bool {
    values.is_empty()
}

/// Consolidated pcurve framing family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CatiaConsolidatedFamily {
    /// A-family frame with a u32 payload length.
    A,
    /// B-family frame with a u8 payload length.
    B,
}

/// Reference dialect used by a consolidated class-`0x62` owner packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CatiaOwnerReferenceEncoding {
    /// Strong identities use tagged little-endian `u16` values.
    TaggedU16Strong,
    /// Strong identities use width-coded compact integers.
    WidthCodedStrong,
    /// All nine identities use the compact-integer reference grammar.
    AllCompact,
}

/// Target encoding of a consolidated class-`0x5f` face node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CatiaFaceNodeTargetEncoding {
    /// Width-coded compact target.
    Compact,
    /// Strong persistent target encoded as `0x0a <u16le>`.
    TaggedU16Strong,
}

/// Derived class-`0x5f` face-node relation associated with a consolidated
/// class-`0x62` packet within one bounded record source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct CatiaFaceNodeRelation {
    /// Face-node record byte offset.
    byte_offset: u64,
    /// Complete face-node to packet span.
    byte_len: u64,
    /// Width-coded header token.
    header_token: u32,
    /// Target encoding selected after the `0x82` lead.
    target_encoding: CatiaFaceNodeTargetEncoding,
    /// Class-`0x5f` target retained by the enclosing source-scoped relation.
    target: u32,
    /// Two terminal bytes of the face-node payload.
    terminal: [u8; 2],
}

/// Selected class of a fixed-nine owner identity target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum CatiaOwnerIdentityClass {
    /// Class-`0x5d` vertex record.
    Vertex,
    /// Class-`0x5e` edge record.
    Edge,
}

impl From<CatiaOwnerIdentityClass> for u8 {
    fn from(value: CatiaOwnerIdentityClass) -> Self {
        match value {
            CatiaOwnerIdentityClass::Vertex => 0x5d,
            CatiaOwnerIdentityClass::Edge => 0x5e,
        }
    }
}

impl TryFrom<u8> for CatiaOwnerIdentityClass {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x5d => Ok(Self::Vertex),
            0x5e => Ok(Self::Edge),
            other => Err(format!("target_class {other:#x} is not 0x5d or 0x5e")),
        }
    }
}

/// One fixed-nine owner identity resolved within its allocation source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct CatiaOwnerIdentityTarget {
    /// Zero-based identity slot in the fixed-nine packet.
    slot: u8,
    /// Decoded backward distance.
    distance: u32,
    /// Byte offset of the selected class-`0x5d` or class-`0x5e` record.
    target_byte_offset: u64,
    /// Selected record class.
    target_class: CatiaOwnerIdentityClass,
}

/// Structurally decoded payload of a class-`0x62` consolidated owner packet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CatiaOwnerPacketPayload {
    /// Nine alternating strong/weak identities followed by a fixed numeric tail.
    FixedNine {
        /// Reference encoding selected by the packet.
        reference_encoding: CatiaOwnerReferenceEncoding,
        /// Nine persistent identities in serialization order.
        references: [u32; 9],
        /// Exact wire addressing form of each identity in source order.
        identity_encodings: [CatiaOwnerIdentityEncoding; 9],
        /// Structurally decoded 62-byte class-specific numeric tail.
        numeric_tail: CatiaOwnerNumericTail,
        /// Backward-distance identities resolved within this packet's contiguous
        /// class-`0x5d`/`0x5e` allocation sequence.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        identity_targets: Vec<CatiaOwnerIdentityTarget>,
        /// Complete carrier/reference/side chart that this packet terminates.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner_chart: Option<Box<CatiaOwnerChartRelation>>,
        /// Closed owner-local four-edge boundary, when all four resolved targets
        /// form one simple cycle in the bounded record source.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        boundary_cycle: Option<Box<CatiaOwnerBoundaryCycle>>,
    },
    /// Count-selected persistent identities followed by a nonempty tail.
    Counted {
        /// Persistent identities in serialization order.
        references: Vec<u32>,
        /// Complete nonempty class-specific tail.
        tail: crate::families::b2::counted_owner_tail::CountedOwnerTail,
    },
}

/// One fixed-nine boundary edge retained when four resolved class-`0x5e`
/// targets close one simple owner-local cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct CatiaOwnerBoundaryEdge {
    /// Identity slot in the fixed-nine packet.
    slot: u8,
    /// Resolved class-`0x5e` edge-record offset.
    byte_offset: u64,
    /// Resolved class-`0x5d` endpoint-record offsets, in edge order.
    endpoint_records: [u64; 2],
}

/// Owner-local boundary evidence derived from a closed fixed-nine cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct CatiaOwnerBoundaryCycle {
    /// Source-scoped class-`0x5f` face node that precedes this boundary
    /// allocation and closes its checked identity, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    face_node: Option<CatiaFaceNodeRelation>,
    /// Four edge targets in fixed-nine slot order.
    edges: [CatiaOwnerBoundaryEdge; 4],
}

/// Exact class-`0x62` consolidated owner packet.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "CatiaConsolidatedOwnerPacketWire")]
pub(crate) struct CatiaConsolidatedOwnerPacket {
    /// Stable source identity.
    id: String,
    /// Record byte offset.
    byte_offset: u64,
    /// Zero-based bounded record-source ordinal.
    source_index: usize,
    /// Width-coded header token.
    header_token: u32,
    /// Count-specific reference lane and tail.
    payload: CatiaOwnerPacketPayload,
    /// Source-scoped class-`0x5f` face node, when the packet relation closes.
    face_node: Option<CatiaFaceNodeRelation>,
}

impl CatiaConsolidatedOwnerPacket {
    #[cfg(test)]
    fn identity_targets(&self) -> &[CatiaOwnerIdentityTarget] {
        match &self.payload {
            CatiaOwnerPacketPayload::FixedNine {
                identity_targets, ..
            } => identity_targets,
            CatiaOwnerPacketPayload::Counted { .. } => &[],
        }
    }

    #[cfg(test)]
    pub(crate) fn owner_chart(&self) -> Option<&CatiaOwnerChartRelation> {
        match &self.payload {
            CatiaOwnerPacketPayload::FixedNine { owner_chart, .. } => owner_chart.as_deref(),
            CatiaOwnerPacketPayload::Counted { .. } => None,
        }
    }

    fn owner_chart_mut(&mut self) -> Option<&mut CatiaOwnerChartRelation> {
        match &mut self.payload {
            CatiaOwnerPacketPayload::FixedNine { owner_chart, .. } => owner_chart.as_deref_mut(),
            CatiaOwnerPacketPayload::Counted { .. } => None,
        }
    }

    #[cfg(test)]
    fn boundary_cycle(&self) -> Option<&CatiaOwnerBoundaryCycle> {
        match &self.payload {
            CatiaOwnerPacketPayload::FixedNine { boundary_cycle, .. } => boundary_cycle.as_deref(),
            CatiaOwnerPacketPayload::Counted { .. } => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct CatiaConsolidatedOwnerPacketWire {
    id: String,
    byte_offset: u64,
    source_index: usize,
    header_token: u32,
    payload: CatiaOwnerPacketPayload,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    identity_targets: Vec<CatiaOwnerIdentityTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    face_node: Option<CatiaFaceNodeRelation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner_chart: Option<CatiaOwnerChartRelation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    boundary_cycle: Option<CatiaOwnerBoundaryCycle>,
}

#[cfg(test)]
impl From<CatiaConsolidatedOwnerPacket> for CatiaConsolidatedOwnerPacketWire {
    fn from(value: CatiaConsolidatedOwnerPacket) -> Self {
        let (identity_targets, owner_chart, boundary_cycle, payload) = match value.payload {
            CatiaOwnerPacketPayload::FixedNine {
                reference_encoding,
                references,
                identity_encodings,
                numeric_tail,
                identity_targets,
                owner_chart,
                boundary_cycle,
            } => (
                identity_targets,
                owner_chart.map(|chart| *chart),
                boundary_cycle.map(|cycle| *cycle),
                CatiaOwnerPacketPayload::FixedNine {
                    reference_encoding,
                    references,
                    identity_encodings,
                    numeric_tail,
                    identity_targets: Vec::new(),
                    owner_chart: None,
                    boundary_cycle: None,
                },
            ),
            payload @ CatiaOwnerPacketPayload::Counted { .. } => (Vec::new(), None, None, payload),
        };
        Self {
            id: value.id,
            byte_offset: value.byte_offset,
            source_index: value.source_index,
            header_token: value.header_token,
            payload,
            identity_targets,
            face_node: value.face_node,
            owner_chart,
            boundary_cycle,
        }
    }
}

impl TryFrom<CatiaConsolidatedOwnerPacketWire> for CatiaConsolidatedOwnerPacket {
    type Error = String;

    fn try_from(wire: CatiaConsolidatedOwnerPacketWire) -> Result<Self, Self::Error> {
        let payload = match wire.payload {
            CatiaOwnerPacketPayload::FixedNine {
                reference_encoding,
                references,
                identity_encodings,
                numeric_tail,
                identity_targets,
                owner_chart,
                boundary_cycle,
            } => {
                if !identity_targets.is_empty() || owner_chart.is_some() || boundary_cycle.is_some()
                {
                    return Err(
                        "fixed-nine metadata must occur only at the packet level".to_owned()
                    );
                }
                CatiaOwnerPacketPayload::FixedNine {
                    reference_encoding,
                    references,
                    identity_encodings,
                    numeric_tail,
                    identity_targets: wire.identity_targets,
                    owner_chart: wire.owner_chart.map(Box::new),
                    boundary_cycle: wire.boundary_cycle.map(Box::new),
                }
            }
            counted @ CatiaOwnerPacketPayload::Counted { .. } => {
                if !wire.identity_targets.is_empty()
                    || wire.owner_chart.is_some()
                    || wire.boundary_cycle.is_some()
                {
                    return Err(
                        "counted owner packets cannot carry fixed-nine chart or boundary data"
                            .to_owned(),
                    );
                }
                counted
            }
        };
        Ok(Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            source_index: wire.source_index,
            header_token: wire.header_token,
            payload,
            face_node: wire.face_node,
        })
    }
}

/// One structurally complete consolidated `B:29` cone chart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "CatiaConsolidatedConeWire")]
pub(crate) struct CatiaConsolidatedCone {
    /// Stable native-record identity.
    id: String,
    /// Byte offset of the framed record.
    byte_offset: u64,
    /// Cone apex.
    apex: FiniteVector<3>,
    /// First transverse unit direction.
    direction_x: crate::checked::RelaxedUnitVector3,
    /// Second transverse unit direction.
    direction_y: crate::checked::RelaxedUnitVector3,
    /// Cone-axis unit direction.
    axis: crate::checked::RelaxedUnitVector3,
    /// Cone half-angle in radians.
    half_angle: cadmpeg_ir::scalar::Angle,
    /// Reference radius of the conical surface, independent of the active chart ranges.
    reference_radius: FiniteReal,
    /// Active azimuth interval.
    angular_range: cadmpeg_ir::topology::IncreasingParameterInterval,
    /// Native slant-coordinate interval, including zero at the apex.
    slant_range: cadmpeg_ir::topology::IncreasingParameterInterval,
    /// Scale from azimuth to stored U parameter.
    angular_scale: cadmpeg_ir::scalar::PositiveReal,
    /// Full-turn azimuth chart domain.
    angular_domain: cadmpeg_ir::topology::IncreasingParameterInterval,
}

#[derive(Deserialize)]
struct CatiaConsolidatedConeWire {
    id: String,
    byte_offset: u64,
    apex: FiniteVector<3>,
    direction_x: crate::checked::RelaxedUnitVector3,
    direction_y: crate::checked::RelaxedUnitVector3,
    axis: crate::checked::RelaxedUnitVector3,
    half_angle: cadmpeg_ir::scalar::Angle,
    reference_radius: FiniteReal,
    angular_range: cadmpeg_ir::topology::IncreasingParameterInterval,
    slant_range: cadmpeg_ir::topology::IncreasingParameterInterval,
    angular_scale: cadmpeg_ir::scalar::PositiveReal,
    angular_domain: cadmpeg_ir::topology::IncreasingParameterInterval,
}

impl TryFrom<CatiaConsolidatedConeWire> for CatiaConsolidatedCone {
    type Error = &'static str;

    fn try_from(wire: CatiaConsolidatedConeWire) -> Result<Self, Self::Error> {
        if crate::checked::UnitFrame3::right_handed(wire.axis, wire.direction_x, wire.direction_y)
            .is_none()
            || !(0.0 < wire.half_angle.get() && wire.half_angle.get() < std::f64::consts::FRAC_PI_2)
            || !crate::analytic::periodic_angular_range_is_valid(
                wire.angular_range.endpoints(),
                wire.angular_domain.endpoints(),
            )
            || wire.slant_range.lower() < 0.0
        {
            return Err("invalid consolidated cone frame or chart");
        }
        Ok(Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            apex: wire.apex,
            direction_x: wire.direction_x,
            direction_y: wire.direction_y,
            axis: wire.axis,
            half_angle: wire.half_angle,
            reference_radius: wire.reference_radius,
            angular_range: wire.angular_range,
            slant_range: wire.slant_range,
            angular_scale: wire.angular_scale,
            angular_domain: wire.angular_domain,
        })
    }
}

/// Payload-layout discriminator of a consolidated arc-length circle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum CatiaCircleLayout {
    /// Identity packed in six bits (`0x32`).
    PackedSix,
    /// Identity packed in one byte (`0x33`).
    Byte,
    /// Identity packed in two bytes (`0x34`).
    Word,
}

impl From<CatiaCircleLayout> for u8 {
    fn from(value: CatiaCircleLayout) -> Self {
        match value {
            CatiaCircleLayout::PackedSix => 0x32,
            CatiaCircleLayout::Byte => 0x33,
            CatiaCircleLayout::Word => 0x34,
        }
    }
}

impl TryFrom<u8> for CatiaCircleLayout {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x32 => Ok(Self::PackedSix),
            0x33 => Ok(Self::Byte),
            0x34 => Ok(Self::Word),
            other => Err(format!("layout {other:#x} is not 0x32..=0x34")),
        }
    }
}

/// One complete consolidated `B:19` arc-length circle support.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "CatiaConsolidatedCircleWire")]
pub(crate) struct CatiaConsolidatedCircle {
    /// Stable native-record identity.
    id: String,
    /// Byte offset of the framed record.
    byte_offset: u64,
    /// Payload-layout discriminator.
    layout: CatiaCircleLayout,
    /// Compact persistent record identity.
    record_id: u32,
    /// Width-coded frame token.
    frame_token: u8,
    /// Two centre coordinates in the host-implied carrier plane.
    pub(crate) center_pair: FiniteVector<2>,
    /// Circle radius in millimetres.
    radius: cadmpeg_ir::scalar::PositiveLength,
    /// Arc-length parameter interval.
    pub(crate) range: cadmpeg_ir::topology::IncreasingParameterInterval,
    /// Length-valued angular chart shift.
    chart_shift: FiniteReal,
}

impl CatiaConsolidatedCircle {
    /// Whether the interval spans one complete circumference.
    fn full_circle(&self) -> bool {
        crate::families::b2::records::circle_range_is_full_turn(
            self.radius.get(),
            self.range.endpoints(),
        )
    }
}
#[derive(Serialize, Deserialize)]
struct CatiaConsolidatedCircleWire {
    id: String,
    byte_offset: u64,
    layout: CatiaCircleLayout,
    record_id: u32,
    frame_token: u8,
    center_pair: FiniteVector<2>,
    radius: cadmpeg_ir::scalar::PositiveLength,
    range: cadmpeg_ir::topology::IncreasingParameterInterval,
    full_circle: bool,
    chart_shift: FiniteReal,
}
impl TryFrom<CatiaConsolidatedCircleWire> for CatiaConsolidatedCircle {
    type Error = String;
    fn try_from(wire: CatiaConsolidatedCircleWire) -> Result<Self, Self::Error> {
        let circle = Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            layout: wire.layout,
            record_id: wire.record_id,
            frame_token: wire.frame_token,
            center_pair: wire.center_pair,
            radius: wire.radius,
            range: wire.range,
            chart_shift: wire.chart_shift,
        };
        if wire.full_circle != circle.full_circle() {
            return Err("full_circle does not match radius and range".into());
        }
        Ok(circle)
    }
}
#[cfg(test)]
impl From<CatiaConsolidatedCircle> for CatiaConsolidatedCircleWire {
    fn from(circle: CatiaConsolidatedCircle) -> Self {
        let full_circle = circle.full_circle();
        Self {
            full_circle,
            id: circle.id,
            byte_offset: circle.byte_offset,
            layout: circle.layout,
            record_id: circle.record_id,
            frame_token: circle.frame_token,
            center_pair: circle.center_pair,
            radius: circle.radius,
            range: circle.range,
            chart_shift: circle.chart_shift,
        }
    }
}

/// Frame-specific payload of one consolidated `B:28` cylinder chart.
#[derive(Debug, Clone, PartialEq)]
enum CatiaConsolidatedCylinderPayload {
    /// Complete three-dimensional frame reconstructed from layout `0x52`.
    Layout52 {
        /// Token selecting the serialized frame-vector role.
        frame_token: u8,
        /// Cylinder-axis unit direction.
        axis: crate::checked::RelaxedHypotUnitVector3,
        /// Unit direction from which the circumferential parameter is measured.
        reference_direction: crate::checked::RelaxedHypotUnitVector3,
    },
    /// Complete three-dimensional frame reconstructed from layout `0x5a`.
    Layout5a {
        /// Token selecting the serialized frame-vector role.
        frame_token: u8,
        /// Cylinder-axis unit direction.
        axis: crate::checked::RelaxedHypotUnitVector3,
        /// Unit direction from which the circumferential parameter is measured.
        reference_direction: crate::checked::RelaxedHypotUnitVector3,
    },
    /// Complete layout-`0x62` frame and its redundant range origin.
    RangeOrigin {
        /// Stored unit vector in the token-defined carrier plane.
        stored_vector: crate::checked::RelaxedUnitVector2,
        /// Cylinder-axis unit direction.
        axis: crate::checked::RelaxedHypotUnitVector3,
        /// Unit direction from which the circumferential parameter is measured.
        reference_direction: crate::checked::RelaxedHypotUnitVector3,
        /// Origin of the stored partial circumferential interval.
        range_origin: f64,
    },
}

impl CatiaConsolidatedCylinderPayload {
    const fn layout(&self) -> u8 {
        match self {
            Self::Layout52 { .. } => 0x52,
            Self::Layout5a { .. } => 0x5a,
            Self::RangeOrigin { .. } => 0x62,
        }
    }

    fn resolved_5a(
        radius: cadmpeg_ir::scalar::PositiveLength,
        u_range: cadmpeg_ir::topology::IncreasingParameterInterval,
        frame_token: u8,
        axis: crate::checked::RelaxedHypotUnitVector3,
        reference_direction: crate::checked::RelaxedHypotUnitVector3,
    ) -> Result<Self, String> {
        let direction = axis.get();
        if !matches!(frame_token, 0x19 | 0x1c)
            || direction[2] != 0.0
            || reference_direction.get() != [-direction[1], direction[0], 0.0]
            || !crate::families::b2::records::circle_range_is_full_turn(
                radius.get(),
                u_range.endpoints(),
            )
        {
            return Err("invalid layout-0x5a cylinder chart".to_owned());
        }
        Ok(Self::Layout5a {
            frame_token,
            axis,
            reference_direction,
        })
    }
}

/// One structurally complete consolidated `B:28` cylinder chart.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "CatiaConsolidatedCylinderWire")]
pub(crate) struct CatiaConsolidatedCylinder {
    /// Stable native-record identity.
    id: String,
    /// Byte offset of the framed record.
    byte_offset: u64,
    /// Cylinder-axis origin.
    origin: FiniteVector<3>,
    /// Cylinder radius.
    radius: cadmpeg_ir::scalar::PositiveLength,
    /// Arc-length circumferential interval.
    u_range: cadmpeg_ir::topology::IncreasingParameterInterval,
    /// Axial interval.
    v_range: cadmpeg_ir::topology::IncreasingParameterInterval,
    /// Layout-specific frame data.
    payload: CatiaConsolidatedCylinderPayload,
}

#[derive(Serialize, Deserialize)]
struct CatiaConsolidatedCylinderWire {
    id: String,
    byte_offset: u64,
    layout: u8,
    origin: FiniteVector<3>,
    radius: cadmpeg_ir::scalar::PositiveLength,
    u_range: cadmpeg_ir::topology::IncreasingParameterInterval,
    v_range: cadmpeg_ir::topology::IncreasingParameterInterval,
    payload: CatiaConsolidatedCylinderPayloadWire,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CatiaConsolidatedCylinderPayloadWire {
    Resolved {
        frame_token: u8,
        axis: crate::checked::RelaxedHypotUnitVector3,
        reference_direction: crate::checked::RelaxedHypotUnitVector3,
    },
    RangeOrigin {
        stored_vector: crate::checked::RelaxedUnitVector2,
        axis: crate::checked::RelaxedHypotUnitVector3,
        reference_direction: crate::checked::RelaxedHypotUnitVector3,
        range_origin: f64,
    },
}

#[cfg(test)]
impl From<CatiaConsolidatedCylinder> for CatiaConsolidatedCylinderWire {
    fn from(value: CatiaConsolidatedCylinder) -> Self {
        let layout = value.payload.layout();
        let payload = match value.payload {
            CatiaConsolidatedCylinderPayload::Layout52 {
                frame_token,
                axis,
                reference_direction,
            }
            | CatiaConsolidatedCylinderPayload::Layout5a {
                frame_token,
                axis,
                reference_direction,
            } => CatiaConsolidatedCylinderPayloadWire::Resolved {
                frame_token,
                axis,
                reference_direction,
            },
            CatiaConsolidatedCylinderPayload::RangeOrigin {
                stored_vector,
                axis,
                reference_direction,
                range_origin,
            } => CatiaConsolidatedCylinderPayloadWire::RangeOrigin {
                stored_vector,
                axis,
                reference_direction,
                range_origin,
            },
        };
        Self {
            id: value.id,
            byte_offset: value.byte_offset,
            layout,
            origin: value.origin,
            radius: value.radius,
            u_range: value.u_range,
            v_range: value.v_range,
            payload,
        }
    }
}

impl TryFrom<CatiaConsolidatedCylinderWire> for CatiaConsolidatedCylinder {
    type Error = String;

    fn try_from(wire: CatiaConsolidatedCylinderWire) -> Result<Self, Self::Error> {
        let payload = match (wire.layout, wire.payload) {
            (
                0x52,
                CatiaConsolidatedCylinderPayloadWire::Resolved {
                    frame_token,
                    axis,
                    reference_direction,
                },
            ) => {
                if frame_token != 0x1d
                    || axis.get() != [1.0, 0.0, 0.0]
                    || reference_direction.get() != [0.0, 1.0, 0.0]
                    || !crate::families::b2::records::circle_range_is_full_turn(
                        wire.radius.get(),
                        wire.u_range.endpoints(),
                    )
                {
                    return Err("invalid layout-0x52 cylinder chart".to_owned());
                }
                CatiaConsolidatedCylinderPayload::Layout52 {
                    frame_token,
                    axis,
                    reference_direction,
                }
            }
            (
                0x5a,
                CatiaConsolidatedCylinderPayloadWire::Resolved {
                    frame_token,
                    axis,
                    reference_direction,
                },
            ) => CatiaConsolidatedCylinderPayload::resolved_5a(
                wire.radius,
                wire.u_range,
                frame_token,
                axis,
                reference_direction,
            )?,
            (
                0x62,
                CatiaConsolidatedCylinderPayloadWire::RangeOrigin {
                    stored_vector,
                    axis,
                    reference_direction,
                    range_origin,
                },
            ) => {
                let vector = stored_vector.get();
                if axis.get() != [0.0, 1.0, 0.0]
                    || reference_direction.get() != [vector[0], 0.0, vector[1]]
                    || !crate::families::b2::records::circle_range_is_within_full_turn(
                        wire.radius.get(),
                        wire.u_range.endpoints(),
                    )
                    || !range_origin.is_finite()
                    || range_origin.to_bits()
                        != crate::families::b2::records::cylinder_range_origin(
                            wire.radius.get(),
                            wire.u_range.endpoints(),
                        )
                        .to_bits()
                {
                    return Err("invalid layout-0x62 cylinder chart".to_owned());
                }
                CatiaConsolidatedCylinderPayload::RangeOrigin {
                    stored_vector,
                    axis,
                    reference_direction,
                    range_origin,
                }
            }
            (layout, _) => {
                return Err(format!(
                    "cylinder layout {layout:#04x} does not match payload"
                ));
            }
        };
        Ok(Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            origin: wire.origin,
            radius: wire.radius,
            u_range: wire.u_range,
            v_range: wire.v_range,
            payload,
        })
    }
}

/// One layout-`0x5a` cylinder embedded in a type-3 consolidated group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "CatiaConsolidatedEmbeddedCylinderWire")]
pub(crate) struct CatiaConsolidatedEmbeddedCylinder {
    /// Stable native-record identity.
    id: String,
    /// Byte offset of the embedded frame, including its varying pre-byte.
    byte_offset: u64,
    /// Owning type-3 consolidated group.
    group: String,
    /// Compact embedded object identity.
    object_id: u32,
    /// Cylinder-axis origin.
    origin: FiniteVector<3>,
    /// Cylinder radius.
    radius: cadmpeg_ir::scalar::PositiveLength,
    /// Full-turn arc-length circumferential interval.
    u_range: cadmpeg_ir::topology::IncreasingParameterInterval,
    /// Axial interval.
    v_range: cadmpeg_ir::topology::IncreasingParameterInterval,
    /// Token selecting the serialized frame-vector role.
    frame_token: u8,
    /// Cylinder-axis unit direction.
    axis: crate::checked::RelaxedHypotUnitVector3,
    /// Unit direction from which the circumferential parameter is measured.
    reference_direction: crate::checked::RelaxedHypotUnitVector3,
}

#[derive(Deserialize)]
struct CatiaConsolidatedEmbeddedCylinderWire {
    id: String,
    byte_offset: u64,
    group: String,
    object_id: u32,
    origin: FiniteVector<3>,
    radius: cadmpeg_ir::scalar::PositiveLength,
    u_range: cadmpeg_ir::topology::IncreasingParameterInterval,
    v_range: cadmpeg_ir::topology::IncreasingParameterInterval,
    frame_token: u8,
    axis: crate::checked::RelaxedHypotUnitVector3,
    reference_direction: crate::checked::RelaxedHypotUnitVector3,
}

impl TryFrom<CatiaConsolidatedEmbeddedCylinderWire> for CatiaConsolidatedEmbeddedCylinder {
    type Error = String;
    fn try_from(wire: CatiaConsolidatedEmbeddedCylinderWire) -> Result<Self, Self::Error> {
        CatiaConsolidatedCylinderPayload::resolved_5a(
            wire.radius,
            wire.u_range,
            wire.frame_token,
            wire.axis,
            wire.reference_direction,
        )?;
        Ok(Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            group: wire.group,
            object_id: wire.object_id,
            origin: wire.origin,
            radius: wire.radius,
            u_range: wire.u_range,
            v_range: wire.v_range,
            frame_token: wire.frame_token,
            axis: wire.axis,
            reference_direction: wire.reference_direction,
        })
    }
}

/// Layout-specific scalar lane of a consolidated `B:18` parameter-space record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CatiaConsolidatedParameterPointPayload {
    /// One retained scalar after two zero tuple fields are elided.
    Scalar {
        /// Stored scalar.
        value: FiniteReal,
    },
    /// Two surface-chart coordinates.
    Uv {
        /// Surface-chart coordinates.
        uv: FiniteVector<2>,
    },
    /// Host-chain station followed by two surface-chart coordinates.
    StationUv {
        /// Host-chain station.
        station: FiniteReal,
        /// Surface-chart coordinates.
        uv: FiniteVector<2>,
    },
    /// Unsplit five-scalar lane.
    FiveScalars {
        /// Stored finite scalars.
        values: FiniteVector<5>,
    },
}

impl CatiaConsolidatedParameterPointPayload {
    const fn layout(&self) -> u8 {
        match self {
            Self::Scalar { .. } => 0x0a,
            Self::Uv { .. } => 0x12,
            Self::StationUv { .. } => 0x1a,
            Self::FiveScalars { .. } => 0x2a,
        }
    }
}

/// One complete consolidated `B:18` parameter-space record.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "CatiaConsolidatedParameterPointWire")]
pub(crate) struct CatiaConsolidatedParameterPoint {
    /// Stable native-record identity.
    id: String,
    /// Byte offset of the framed record.
    byte_offset: u64,
    /// Complete framed-record length.
    byte_len: u64,
    /// First byte of the two-byte class-specific prefix.
    prefix: crate::families::b2::records::B2ParameterPointPrefix,
    /// Second byte of the two-byte class-specific prefix.
    control: u8,
    /// Layout-specific finite scalar lane.
    payload: CatiaConsolidatedParameterPointPayload,
}

#[derive(Serialize, Deserialize)]
struct CatiaConsolidatedParameterPointWire {
    id: String,
    byte_offset: u64,
    byte_len: u64,
    layout: u8,
    prefix: u8,
    control: u8,
    payload: CatiaConsolidatedParameterPointPayload,
}

#[cfg(test)]
impl From<CatiaConsolidatedParameterPoint> for CatiaConsolidatedParameterPointWire {
    fn from(value: CatiaConsolidatedParameterPoint) -> Self {
        Self {
            id: value.id,
            byte_offset: value.byte_offset,
            byte_len: value.byte_len,
            layout: value.payload.layout(),
            prefix: value.prefix.as_u8(),
            control: value.control,
            payload: value.payload,
        }
    }
}

impl TryFrom<CatiaConsolidatedParameterPointWire> for CatiaConsolidatedParameterPoint {
    type Error = String;

    fn try_from(wire: CatiaConsolidatedParameterPointWire) -> Result<Self, Self::Error> {
        if wire.layout != wire.payload.layout() {
            return Err(format!(
                "parameter-point layout {:#04x} does not match payload",
                wire.layout
            ));
        }
        let prefix = crate::families::b2::records::B2ParameterPointPrefix::from_u8(wire.prefix)
            .ok_or_else(|| format!("unknown parameter-point prefix {:#04x}", wire.prefix))?;
        Ok(Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            byte_len: wire.byte_len,
            prefix,
            control: wire.control,
            payload: wire.payload,
        })
    }
}

/// Selector-specific payload of a consolidated `B:27` plane carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CatiaConsolidatedPlaneCarrierPayload {
    /// Two-coordinate point, two-coordinate direction, and three tail scalars.
    PointDirection2 {
        /// In-plane point with the host-implied third coordinate omitted.
        point: FiniteVector<2>,
        /// In-plane direction with its third component omitted; the decoder
        /// stores the first two components of an admitted unit direction.
        direction: [f64; 2],
        /// Complete trailing scalar lane.
        tail: FiniteVector<3>,
    },
    /// Two-coordinate point, three-coordinate direction, and three tail scalars.
    PointDirection3 {
        /// In-plane point with the host-implied third coordinate omitted.
        point: FiniteVector<2>,
        /// In-plane unit direction.
        direction: crate::checked::RelaxedHypotUnitVector3,
        /// Complete trailing scalar lane.
        tail: FiniteVector<3>,
    },
    /// Two-coordinate point followed by four scalar values with no direction
    /// lane in this layout.
    PointTail {
        /// In-plane point with the host-implied third coordinate omitted.
        point: FiniteVector<2>,
        /// Complete trailing scalar lane.
        tail: FiniteVector<4>,
    },
    /// Finite scalar lane for a selector whose semantic layout is not yet
    /// established.
    ScalarLane {
        /// Second payload byte selecting the scalar layout.
        #[serde(skip, default)]
        selector: u8,
        /// Complete selector-specific scalar lane in source order.
        values: Vec<FiniteReal>,
    },
}

impl CatiaConsolidatedPlaneCarrierPayload {
    const fn selector(&self) -> u8 {
        match self {
            Self::PointDirection2 { .. } => 0xe4,
            Self::PointDirection3 { .. } => 0xc4,
            Self::PointTail { .. } => 0xec,
            Self::ScalarLane { selector, .. } => *selector,
        }
    }
}

/// One complete consolidated `B:27` plane-carrier record.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "CatiaConsolidatedPlaneCarrierWire")]
pub(crate) struct CatiaConsolidatedPlaneCarrier {
    /// Stable native-record identity.
    id: String,
    /// Byte offset of the framed record.
    byte_offset: u64,
    /// Complete framed-record length.
    byte_len: u64,
    /// Header-token width in bytes.
    width: ConsolidatedFrameWidth,
    /// Independent frame flag.
    flag: ConsolidatedFrameFlag,
    /// Width-coded frame header token.
    header_token: u32,
    /// Selector-specific finite scalar payload.
    payload: CatiaConsolidatedPlaneCarrierPayload,
}

#[derive(Serialize, Deserialize)]
struct CatiaConsolidatedPlaneCarrierWire {
    id: String,
    byte_offset: u64,
    byte_len: u64,
    width: ConsolidatedFrameWidth,
    flag: ConsolidatedFrameFlag,
    header_token: u32,
    selector: u8,
    payload: CatiaConsolidatedPlaneCarrierPayload,
}

#[cfg(test)]
impl From<CatiaConsolidatedPlaneCarrier> for CatiaConsolidatedPlaneCarrierWire {
    fn from(value: CatiaConsolidatedPlaneCarrier) -> Self {
        Self {
            id: value.id,
            byte_offset: value.byte_offset,
            byte_len: value.byte_len,
            width: value.width,
            flag: value.flag,
            header_token: value.header_token,
            selector: value.payload.selector(),
            payload: value.payload,
        }
    }
}

impl TryFrom<CatiaConsolidatedPlaneCarrierWire> for CatiaConsolidatedPlaneCarrier {
    type Error = String;

    fn try_from(mut wire: CatiaConsolidatedPlaneCarrierWire) -> Result<Self, Self::Error> {
        if let CatiaConsolidatedPlaneCarrierPayload::ScalarLane { selector, .. } = &mut wire.payload
        {
            *selector = wire.selector;
        } else if wire.payload.selector() != wire.selector {
            return Err(format!(
                "plane-carrier selector {:#04x} does not match payload",
                wire.selector
            ));
        }
        Ok(Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            byte_len: wire.byte_len,
            width: wire.width,
            flag: wire.flag,
            header_token: wire.header_token,
            payload: wire.payload,
        })
    }
}

/// One complete consolidated `B:37` persistent-reference list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaConsolidatedReferenceList {
    /// Stable native-record identity.
    id: String,
    /// Byte offset of the framed record.
    byte_offset: u64,
    /// Compact persistent identities in serialization order.
    references: Vec<u32>,
}

/// One structurally complete consolidated `A/B:20` pcurve jet whose support
/// identity has not necessarily been resolved to a native surface record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CatiaConsolidatedPcurve {
    /// Stable native-record identity.
    id: String,
    /// Byte offset of the framed record.
    byte_offset: u64,
    /// Consolidated framing family.
    family: CatiaConsolidatedFamily,
    /// Absolute persistent support-surface identity.
    support_id: u32,
    /// Parametric curve degree.
    degree: u32,
    /// Number of leading extrapolation sites.
    extrapolation_sites: u32,
    /// Native parameter sites; the decoder stores them strictly increasing.
    knots: Vec<FiniteReal>,
    /// Surface-chart positions at the parameter sites.
    points: Vec<FiniteVector<2>>,
    /// First derivatives at the parameter sites.
    first_derivatives: Vec<FiniteVector<2>>,
    /// Second derivatives at the parameter sites.
    second_derivatives: Vec<FiniteVector<2>>,
    /// Native evaluation interval.
    range: cadmpeg_ir::topology::IncreasingParameterInterval,
    /// Bytes following the evaluation interval in the framed payload.
    #[serde(with = "cadmpeg_ir::bytes")]
    tail: Vec<u8>,
}

/// One structurally complete consolidated `B:2a` sphere chart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CatiaConsolidatedSphere {
    /// Stable native-record identity.
    id: String,
    /// Byte offset of the framed record.
    byte_offset: u64,
    /// Sphere centre.
    center: FiniteVector<3>,
    /// First transverse unit direction.
    direction_x: crate::checked::ExactHypotUnitVector3,
    /// Second transverse unit direction.
    direction_y: crate::checked::ExactHypotUnitVector3,
    /// Sphere-axis unit direction.
    axis: crate::checked::ExactHypotUnitVector3,
    /// Sphere radius.
    radius: cadmpeg_ir::scalar::PositiveLength,
    /// Active azimuth interval.
    azimuth_range: cadmpeg_ir::topology::IncreasingParameterInterval,
    /// Active latitude interval.
    latitude_range: cadmpeg_ir::topology::IncreasingParameterInterval,
}

/// One structurally complete consolidated `B:2b` torus chart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CatiaConsolidatedTorus {
    /// Stable native-record identity.
    id: String,
    /// Byte offset of the framed record.
    byte_offset: u64,
    /// Torus centre.
    center: FiniteVector<3>,
    /// First transverse unit direction.
    direction_x: crate::checked::ExactUnitVector3,
    /// Second transverse unit direction.
    direction_y: crate::checked::ExactUnitVector3,
    /// Torus-axis unit direction.
    axis: crate::checked::ExactUnitVector3,
    /// Major radius.
    major_radius: cadmpeg_ir::scalar::PositiveLength,
    /// Minor radius.
    minor_radius: cadmpeg_ir::scalar::PositiveLength,
    /// Active major-angle interval.
    major_angular_range: cadmpeg_ir::topology::IncreasingParameterInterval,
    /// Full-turn major-angle chart domain.
    major_angular_domain: cadmpeg_ir::topology::IncreasingParameterInterval,
    /// Active minor-angle interval.
    minor_angular_range: cadmpeg_ir::topology::IncreasingParameterInterval,
    /// Full-turn minor-angle chart domain.
    minor_angular_domain: cadmpeg_ir::topology::IncreasingParameterInterval,
    /// Scale from major angle to stored U parameter.
    major_scale: cadmpeg_ir::scalar::PositiveReal,
    /// Scale from minor angle to stored V parameter.
    minor_scale: cadmpeg_ir::scalar::PositiveReal,
}

/// One exact consolidated B-family metric line profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CatiaConsolidatedLineProfile {
    /// Stable native-record identity.
    id: String,
    /// Byte offset of the framed record.
    byte_offset: u64,
    /// Stored line origin.
    origin: FiniteVector<3>,
    /// Unit line direction.
    direction: crate::checked::ExactUnitVector3,
    /// Increasing stored parameter interval.
    range: cadmpeg_ir::topology::IncreasingParameterInterval,
}

/// Reference-token dialect of a consolidated surface of revolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub(crate) enum CatiaRevolutionReferenceToken {
    /// Compact object-reference token `0x08`.
    Compact,
    /// Wide object-reference token `0x0a`.
    Wide,
}

impl From<CatiaRevolutionReferenceToken> for u8 {
    fn from(value: CatiaRevolutionReferenceToken) -> Self {
        match value {
            CatiaRevolutionReferenceToken::Compact => 0x08,
            CatiaRevolutionReferenceToken::Wide => 0x0a,
        }
    }
}

impl TryFrom<u8> for CatiaRevolutionReferenceToken {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x08 => Ok(Self::Compact),
            0x0a => Ok(Self::Wide),
            other => Err(format!("reference_token {other:#x} is not 0x08 or 0x0a")),
        }
    }
}

/// One structurally complete consolidated `B:2d` surface-of-revolution record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CatiaConsolidatedRevolution {
    /// Stable native-record identity.
    id: String,
    /// Byte offset of the framed record.
    byte_offset: u64,
    /// Reference-token dialect.
    reference_token: CatiaRevolutionReferenceToken,
    /// Unresolved consolidated allocation identity of the profile curve.
    profile_allocation_id: u16,
    /// Axis-frame origin.
    origin: FiniteVector<3>,
    /// First transverse unit direction.
    direction_x: crate::checked::ExactUnitVector3,
    /// Second transverse unit direction.
    direction_y: crate::checked::ExactUnitVector3,
    /// Revolution-axis unit direction.
    axis: crate::checked::ExactUnitVector3,
    /// Stored full-turn angular parameter interval.
    angular_range: cadmpeg_ir::topology::IncreasingParameterInterval,
    /// Stored profile parameter interval.
    profile_range: cadmpeg_ir::topology::IncreasingParameterInterval,
    /// Unique consolidated circle with the same stored profile interval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    profile_circle: Option<String>,
    /// Positive scale from revolution angle to stored angular parameter.
    angular_scale: cadmpeg_ir::scalar::PositiveReal,
}

/// One structurally complete consolidated class-`0x61` record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CatiaConsolidatedClass61Record {
    /// Stable native-record identity.
    id: String,
    /// Byte offset of the framed record.
    byte_offset: u64,
    /// Width-coded header token.
    header_token: u32,
    /// Counted or long-form record payload.
    payload: CatiaConsolidatedClass61Payload,
}

/// Structurally decoded payload of a consolidated class-`0x61` record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CatiaConsolidatedClass61Payload {
    /// Count-selected compact reference lane followed by a class-specific tail.
    Counted {
        /// Compact identities in serialization order.
        references: Vec<u32>,
        /// Complete nonempty tail, including terminal byte `0x03`.
        #[serde(with = "cadmpeg_ir::bytes")]
        tail: Vec<u8>,
    },
    /// Long form with a monotone member lane and five persistent references.
    Long {
        /// Complete eight-byte prefix preceding the member-list marker.
        prefix: [u8; 8],
        /// Strictly increasing allocation members.
        members: Vec<u16>,
        /// Five persistent identities following the list delimiter.
        references: [u16; 5],
        /// Finite class-specific scalar preceding the terminal byte.
        scalar: FiniteReal,
    },
}

/// One typed consolidated class-`0x60` group opener.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaConsolidatedGroup {
    /// Stable native-record identity.
    id: String,
    /// Byte offset of the framed record.
    byte_offset: u64,
    /// Compact group-type code.
    group_type: u32,
}

/// One complete consolidated cone-face chart descriptor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CatiaConsolidatedConeFace {
    /// Stable native-record identity.
    id: String,
    /// Byte offset of the framed record.
    byte_offset: u64,
    /// Complete framed-record length.
    byte_len: u64,
    /// Complete reference-and-control program preceding the scalars.
    #[serde(with = "cadmpeg_ir::bytes")]
    program: Vec<u8>,
    /// Stored angular chart scale.
    angular_scale: FiniteReal,
    /// Cone half-angle in radians, strictly between zero and a quarter turn.
    half_angle: cadmpeg_ir::scalar::PositiveAngle,
    /// Complete immediately following parameter-point run.
    pub(crate) parameter_points: Vec<String>,
}

/// One complete consolidated historical edge run referencing two retained
/// pcurve records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CatiaConsolidatedEdgeRun {
    /// Stable native-record identity.
    id: String,
    /// Byte offset of the first pcurve frame.
    byte_offset: u64,
    /// Retained pcurve identities in serialized side order.
    pcurves: [String; 2],
    /// Shared native parameter interval.
    parameter_range: cadmpeg_ir::topology::IncreasingParameterInterval,
    /// Shared geometric tolerance.
    tolerance: FiniteReal,
    /// Exact terminal edge node.
    node: String,
    /// Uniquely resolved support carrier for each pcurve side.
    #[serde(default)]
    pub(crate) support_bindings: [Option<CatiaConsolidatedSupportBinding>; 2],
    /// Index-aligned 3D loci shared by every resolved support side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) shared_loci: Option<Vec<[f64; 3]>>,
    /// First and last shared loci in endpoint pair direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) endpoint_loci: Option<[[f64; 3]; 2]>,
}

/// Wire addressing form of one width-coded allocation reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CatiaAllocationReferenceEncoding {
    /// `4n+1` backward framed-record distance.
    BackwardDistance,
    /// `4n+3` zero-based ordinal in the immediately owned allocation.
    OwnedChild,
    /// `4w` followed by a `w`-byte little-endian value.
    WidthCoded,
    /// Untagged `4n+2` selector form.
    Selector2,
    /// `06 <u8>`.
    TaggedU8,
    /// `0a <u16le>`.
    TaggedU16,
}

/// Wire addressing form of one fixed-nine owner identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CatiaOwnerIdentityEncoding {
    /// One token from the allocation-reference grammar.
    Allocation(CatiaAllocationReferenceEncoding),
    /// Raw one-byte weak identity in the width-coded alternating dialect.
    RawU8,
}

/// Typed class-`0x18` descriptor bound to a class-`0x25` edge definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CatiaConsolidatedClass25Descriptor {
    /// Record byte offset.
    byte_offset: u64,
    /// Width-coded allocation identity.
    record_id: u32,
    /// Descriptor control byte.
    pub(crate) control: u8,
    /// Complete finite scalar lane.
    values: Vec<FiniteReal>,
}

/// Descriptor and circle relation structurally bound to an analytic edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CatiaConsolidatedAnalyticCircleBinding {
    /// Exact class-`0x18` descriptor frame.
    descriptor: crate::wire::records::ConsolidatedRawFrame<u64>,
    /// Referenced consolidated circle support.
    pub(crate) circle: String,
}

/// Exact oriented-use allocation chain owned by one consolidated edge node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "CatiaConsolidatedEdgeUsesWire",
    into = "CatiaConsolidatedEdgeUsesWire"
)]
struct CatiaConsolidatedEdgeUses {
    /// Counted allocation-reference vectors in side order.
    references: [[u32; 2]; 2],
}

#[derive(Serialize, Deserialize)]
struct CatiaConsolidatedEdgeUsesWire {
    references: [[u32; 2]; 2],
    senses: [u8; 2],
}

impl From<CatiaConsolidatedEdgeUses> for CatiaConsolidatedEdgeUsesWire {
    fn from(value: CatiaConsolidatedEdgeUses) -> Self {
        Self {
            references: value.references,
            senses: [0x88, 0x84],
        }
    }
}

impl TryFrom<CatiaConsolidatedEdgeUsesWire> for CatiaConsolidatedEdgeUses {
    type Error = String;

    fn try_from(wire: CatiaConsolidatedEdgeUsesWire) -> Result<Self, Self::Error> {
        if wire.senses != [0x88, 0x84] {
            return Err("consolidated edge uses require senses [0x88, 0x84]".to_owned());
        }
        Ok(Self {
            references: wire.references,
        })
    }
}

/// One endpoint identity retained by consolidated topology edge nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaConsolidatedVertexIdentity {
    /// Stable native-record identity assigned in first-incidence order.
    id: String,
    /// First raw endpoint-address operand associated with this identity.
    identity: u32,
    /// Bounded record source that owns this identity namespace.
    source_index: usize,
    /// Resolved structural endpoint record, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    endpoint_record: Option<u64>,
    /// Raw endpoint-address operands associated with this identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    reference_values: Vec<u32>,
    /// Compact allocation scope for the identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allocation_owner: Option<String>,
    /// Incident consolidated edge nodes in source order.
    incident_edge_nodes: Vec<String>,
}

/// Exact carrier selected for one side of a consolidated historical edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub(crate) enum CatiaConsolidatedSupportBinding {
    /// Standalone `b2 03 28` cylinder.
    Cylinder {
        /// Carrier record byte offset.
        byte_offset: u64,
    },
    /// Cylinder frame embedded in a `b2 03 60` wrapper.
    EmbeddedCylinder {
        /// Embedded frame byte offset.
        byte_offset: u64,
        /// Enclosing wrapper byte offset.
        wrapper_byte_offset: u64,
    },
    /// Arc-length `b2 03 19` circle.
    Circle {
        /// Carrier record byte offset.
        byte_offset: u64,
    },
    /// `b2 03 29` cone.
    Cone {
        /// Carrier record byte offset.
        byte_offset: u64,
    },
    /// `b2 03 2a` sphere.
    Sphere {
        /// Carrier record byte offset.
        byte_offset: u64,
    },
    /// Doubly periodic `b2 03 2b` torus.
    Torus {
        /// Carrier record byte offset.
        byte_offset: u64,
    },
    /// Direction-bearing consolidated `b2/b3/b4 03 27` plane carrier.
    Plane {
        /// Carrier record byte offset.
        byte_offset: u64,
    },
    /// Consolidated NURBS carrier with an optional constant normal offset.
    NurbsCarrier {
        /// Carrier record byte offset.
        byte_offset: u64,
        /// Signed normal offset in millimetres.
        offset: FiniteReal,
    },
}

/// One complete outer FINJPL segment retained with its framing identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaFinjplSegment {
    /// Globally unique segment identity.
    pub(crate) id: String,
    /// FINJPL marker offset in the complete file.
    pub(crate) byte_offset: u64,
    /// Complete segment byte length.
    pub(crate) byte_len: u64,
    /// Big-endian segment type word.
    type_word: u32,
    /// Structural type family.
    family: String,
    /// Stored primary name, when the printable-ASCII name form is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// Complete segment bytes from marker through the byte before the next segment.
    #[serde(with = "cadmpeg_ir::bytes")]
    data: Vec<u8>,
}

/// One external CATIA document selected by a storage-property record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaExternalReference {
    /// Globally unique reference identity.
    id: String,
    /// File offset of the length-prefixed target string.
    pub(crate) byte_offset: u64,
    /// Referenced CATIA document name or path.
    pub(crate) target: String,
    /// Containing project-flags FINJPL segment.
    pub(crate) segment: String,
}

/// One exact JPEG preview from the outer summary-information segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaPreviewImage {
    /// Globally unique preview identity.
    id: String,
    /// JPEG SOI byte offset in the complete file.
    byte_offset: u64,
    /// Exact encoded length through JPEG EOI.
    byte_len: u64,
    /// Pixel width from the JPEG start-of-frame segment.
    width: u16,
    /// Pixel height from the JPEG start-of-frame segment.
    height: u16,
    /// JPEG component count.
    components: u8,
    /// Exact JPEG byte stream.
    #[serde(with = "cadmpeg_ir::bytes")]
    data: Vec<u8>,
}

/// One exact outer `01 00 04 00` alias-row core.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "CatiaAliasRowWire")]
pub(crate) struct CatiaAliasRow {
    /// Globally unique alias-row identity.
    id: String,
    /// Byte offset of the four-byte alias marker.
    byte_offset: u64,
    /// Complete preceding four-byte word.
    lead_raw: u32,
    /// Complete stored tag word.
    tag_raw: u32,
    /// Single-byte row flag.
    flag: u8,
    /// Complete three-byte F1 field.
    f1: [u8; 3],
    /// Primary object graph selected by the valid F1 ordinal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    object_graph: Option<String>,
    /// One-based F1 ordinal resolved to its exact `7C09` record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    object_record: Option<String>,
    /// Design object owning the selected record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    design_object: Option<String>,
    /// First trailing fixed-width field.
    f2: u32,
    /// Second trailing fixed-width field.
    f3: u32,
    /// Group-allocation header immediately preceding this alias core.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group: Option<AliasGroupMembership>,
    /// Canonical persistent surface-roster tag selected by this alias row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    canonical_surface_tag: Option<u32>,
}

impl CatiaAliasRow {
    /// Classification of the stored alias lead word.
    fn lead(&self) -> AliasLead {
        AliasLead::from_raw(self.lead_raw)
    }
    /// Low 24 bits of the stored tag word.
    fn tag(&self) -> u32 {
        self.tag_raw & 0x00ff_ffff
    }
    /// Entity-table ordinal from the F1 field.
    fn entity_record_ordinal(&self) -> u8 {
        self.f1[2]
    }

    /// Byte offset of the 24-byte row frame.
    ///
    /// `byte_offset` names the four-byte marker, which sits at
    /// `outer_alias_row::MARKER` inside the row. A marker inside the first
    /// `MARKER` bytes of the image has no row frame, so this answers `None`
    /// instead of aliasing the frame with the head of the file.
    fn row_byte_offset(&self) -> Option<u64> {
        self.byte_offset
            .checked_sub(u64_from_index(crate::layout::outer_alias_row::MARKER))
    }
}

impl DecodeCost for CatiaAliasRow {
    fn decode_cost(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, CodecError> {
        (
            (
                &self.id,
                &self.byte_offset,
                &self.lead_raw,
                &self.tag_raw,
                &self.flag,
                &self.f1,
            ),
            (
                &self.object_graph,
                &self.object_record,
                &self.design_object,
                &self.f2,
                &self.f3,
                &self.group,
            ),
            &self.canonical_surface_tag,
        )
            .decode_cost(ctx, operation)
    }
}

#[derive(Serialize, Deserialize)]
struct CatiaAliasRowWire {
    id: String,
    byte_offset: u64,
    lead: AliasLead,
    lead_raw: u32,
    tag: u32,
    tag_raw: u32,
    flag: u8,
    f1: [u8; 3],
    entity_record_ordinal: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    object_graph: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    object_record: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    design_object: Option<String>,
    f2: u32,
    f3: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group: Option<AliasGroupMembership>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    canonical_surface_tag: Option<u32>,
}

#[derive(Serialize)]
struct CatiaAliasRowWireRef<'a> {
    id: &'a str,
    byte_offset: u64,
    lead: AliasLead,
    lead_raw: u32,
    tag: u32,
    tag_raw: u32,
    flag: u8,
    f1: [u8; 3],
    entity_record_ordinal: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    object_graph: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    object_record: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    design_object: Option<&'a str>,
    f2: u32,
    f3: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    group: &'a Option<AliasGroupMembership>,
    #[serde(skip_serializing_if = "Option::is_none")]
    canonical_surface_tag: Option<u32>,
}

impl Serialize for CatiaAliasRow {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        CatiaAliasRowWireRef {
            id: &self.id,
            byte_offset: self.byte_offset,
            lead: self.lead(),
            lead_raw: self.lead_raw,
            tag: self.tag(),
            tag_raw: self.tag_raw,
            flag: self.flag,
            f1: self.f1,
            entity_record_ordinal: self.entity_record_ordinal(),
            object_graph: self.object_graph.as_deref(),
            object_record: self.object_record.as_deref(),
            design_object: self.design_object.as_deref(),
            f2: self.f2,
            f3: self.f3,
            group: &self.group,
            canonical_surface_tag: self.canonical_surface_tag,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
impl From<CatiaAliasRow> for CatiaAliasRowWire {
    fn from(value: CatiaAliasRow) -> Self {
        Self {
            lead: value.lead(),
            tag: value.tag(),
            entity_record_ordinal: value.entity_record_ordinal(),
            id: value.id,
            byte_offset: value.byte_offset,
            lead_raw: value.lead_raw,
            tag_raw: value.tag_raw,
            flag: value.flag,
            f1: value.f1,
            object_graph: value.object_graph,
            object_record: value.object_record,
            design_object: value.design_object,
            f2: value.f2,
            f3: value.f3,
            group: value.group,
            canonical_surface_tag: value.canonical_surface_tag,
        }
    }
}

impl TryFrom<CatiaAliasRowWire> for CatiaAliasRow {
    type Error = &'static str;
    fn try_from(wire: CatiaAliasRowWire) -> Result<Self, Self::Error> {
        if wire.lead != AliasLead::from_raw(wire.lead_raw) {
            return Err("lead disagrees with source bytes");
        }
        if wire.tag != wire.tag_raw & 0x00ff_ffff {
            return Err("tag disagrees with source bytes");
        }
        if wire.entity_record_ordinal != wire.f1[2] {
            return Err("entity_record_ordinal disagrees with source bytes");
        }
        if wire.byte_offset < u64_from_index(crate::layout::outer_alias_row::MARKER) {
            return Err("byte_offset places the alias marker before its row frame");
        }
        Ok(Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            lead_raw: wire.lead_raw,
            tag_raw: wire.tag_raw,
            flag: wire.flag,
            f1: wire.f1,
            object_graph: wire.object_graph,
            object_record: wire.object_record,
            design_object: wire.design_object,
            f2: wire.f2,
            f3: wire.f3,
            group: wire.group,
            canonical_surface_tag: wire.canonical_surface_tag,
        })
    }
}

/// One exact `7C0B` value block adjacent to its source-schema catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CatiaValueBlockWire", into = "CatiaValueBlockWire")]
pub(crate) struct CatiaValueBlock {
    /// Globally unique value-block identity.
    pub(crate) id: String,
    /// Byte offset of the `7C0B` marker.
    pub(crate) byte_offset: u64,
    /// Object graph ending exactly where this value block begins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) object_graph: Option<String>,
    /// Source-schema catalog that begins immediately after this block.
    pub(crate) catalog: String,
    /// Value payload in serialized order.
    #[serde(with = "cadmpeg_ir::bytes")]
    pub(crate) payload: Vec<u8>,
    /// Schema selectors in payload order, resolved against the adjacent catalog.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) schema_selections: Vec<CatiaValueSchemaSelection>,
}

impl CatiaValueBlock {
    fn declared_len(&self) -> u64 {
        u64_from_index(self.payload.len()) + u64_from_index(crate::layout::value_block_7c0b::LEN)
    }

    fn byte_len(&self) -> u64 {
        self.declared_len() + 1
    }

    pub(crate) fn fields(&self) -> Vec<value_block::ValueField> {
        value_block::tokenize(&self.payload)
    }
}

// Compatibility fields are derived on output and checked once on input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CatiaValueBlockWire {
    /// Globally unique value-block identity.
    id: String,
    /// Byte offset of the `7C0B` marker.
    byte_offset: u64,
    /// Complete framed extent including the trailing terminator.
    byte_len: u64,
    /// Stored length from the marker through the byte before the terminator.
    declared_len: u64,
    /// Object graph ending exactly where this value block begins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    object_graph: Option<String>,
    /// Source-schema catalog that begins immediately after this block.
    catalog: String,
    /// Value payload in serialized order.
    #[serde(with = "cadmpeg_ir::bytes")]
    payload: Vec<u8>,
    /// Lossless typed fields in payload order.
    #[serde(default)]
    fields: Vec<value_block::ValueField>,
    /// Schema selectors in payload order, resolved against the adjacent catalog.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    schema_selections: Vec<CatiaValueSchemaSelection>,
}

impl From<CatiaValueBlock> for CatiaValueBlockWire {
    fn from(block: CatiaValueBlock) -> Self {
        Self {
            byte_len: block.byte_len(),
            declared_len: block.declared_len(),
            fields: block.fields(),
            id: block.id,
            byte_offset: block.byte_offset,
            object_graph: block.object_graph,
            catalog: block.catalog,
            payload: block.payload,
            schema_selections: block.schema_selections,
        }
    }
}

impl CatiaValueBlockWire {
    fn from_charged(ctx: &DecodeContext<'_>, block: CatiaValueBlock) -> Result<Self, CodecError> {
        Ok(Self {
            byte_len: block.byte_len(),
            declared_len: block.declared_len(),
            fields: value_block::tokenize_charged(ctx, &block.payload)?,
            id: block.id,
            byte_offset: block.byte_offset,
            object_graph: block.object_graph,
            catalog: block.catalog,
            payload: block.payload,
            schema_selections: block.schema_selections,
        })
    }
}

impl TryFrom<CatiaValueBlockWire> for CatiaValueBlock {
    type Error = &'static str;

    fn try_from(wire: CatiaValueBlockWire) -> Result<Self, Self::Error> {
        let block = Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            object_graph: wire.object_graph,
            catalog: wire.catalog,
            payload: wire.payload,
            schema_selections: wire.schema_selections,
        };
        if wire.byte_len != block.byte_len()
            || wire.declared_len != block.declared_len()
            || wire.fields != block.fields()
        {
            return Err("value block lengths or fields disagree with payload");
        }
        Ok(block)
    }
}

/// Catalog class and encoded payload of a selected value-block schema selector.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CatiaValueSchemaSelectionValue {
    /// Selected catalog class.
    class: CatiaDesignClass,
    /// Complete encoded value after this selector and before the next selector.
    encoded_value: Vec<value_block::ValueField>,
}

/// One `0x32` selector from a value block.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CatiaValueSchemaSelectionKind {
    /// Catalog class selected by the stored ordinal.
    Selected(CatiaValueSchemaSelectionValue),
    /// Terminal absent-schema sentinel.
    Terminal,
}

/// One `0x32` selector from a value block.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "CatiaValueSchemaSelectionWire")]
pub(crate) struct CatiaValueSchemaSelection {
    /// Globally unique schema-selection identity.
    id: String,
    /// Containing [`CatiaValueBlock`] identity.
    parent: String,
    /// Byte offset within the value payload.
    offset: u64,
    /// Stored zero-based ordinal or terminal absent-schema sentinel.
    ordinal: u32,
    /// Selected class or terminal sentinel.
    kind: CatiaValueSchemaSelectionKind,
}

impl CatiaValueSchemaSelection {
    #[cfg(test)]
    fn entry(&self) -> Option<&str> {
        match &self.kind {
            CatiaValueSchemaSelectionKind::Selected(value) => Some(value.class.entry.as_str()),
            CatiaValueSchemaSelectionKind::Terminal => None,
        }
    }

    #[cfg(test)]
    fn name(&self) -> Option<&str> {
        match &self.kind {
            CatiaValueSchemaSelectionKind::Selected(value) => Some(value.class.name.as_str()),
            CatiaValueSchemaSelectionKind::Terminal => None,
        }
    }

    #[cfg(test)]
    fn encoded_value(&self) -> &[value_block::ValueField] {
        match &self.kind {
            CatiaValueSchemaSelectionKind::Selected(value) => &value.encoded_value,
            CatiaValueSchemaSelectionKind::Terminal => &[],
        }
    }
}

#[derive(Serialize, Deserialize)]
struct CatiaValueSchemaSelectionWire {
    id: String,
    parent: String,
    offset: u64,
    ordinal: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    entry: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    encoded_value: Vec<value_block::ValueField>,
}

#[derive(Serialize)]
struct CatiaValueSchemaSelectionWireRef<'a> {
    id: &'a str,
    parent: &'a str,
    offset: u64,
    ordinal: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    entry: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(skip_serializing_if = "slice_is_empty")]
    encoded_value: &'a [value_block::ValueField],
}

impl Serialize for CatiaValueSchemaSelection {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (entry, name, encoded_value) = match &self.kind {
            CatiaValueSchemaSelectionKind::Selected(value) => (
                Some(value.class.entry.as_str()),
                Some(value.class.name.as_str()),
                value.encoded_value.as_slice(),
            ),
            CatiaValueSchemaSelectionKind::Terminal => (None, None, &[][..]),
        };
        CatiaValueSchemaSelectionWireRef {
            id: &self.id,
            parent: &self.parent,
            offset: self.offset,
            ordinal: self.ordinal,
            entry,
            name,
            encoded_value,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
impl From<CatiaValueSchemaSelection> for CatiaValueSchemaSelectionWire {
    fn from(value: CatiaValueSchemaSelection) -> Self {
        let (entry, name, encoded_value) = match value.kind {
            CatiaValueSchemaSelectionKind::Selected(selected) => (
                Some(selected.class.entry),
                Some(selected.class.name),
                selected.encoded_value,
            ),
            CatiaValueSchemaSelectionKind::Terminal => (None, None, Vec::new()),
        };
        Self {
            id: value.id,
            parent: value.parent,
            offset: value.offset,
            ordinal: value.ordinal,
            entry,
            name,
            encoded_value,
        }
    }
}

impl TryFrom<CatiaValueSchemaSelectionWire> for CatiaValueSchemaSelection {
    type Error = String;

    fn try_from(wire: CatiaValueSchemaSelectionWire) -> Result<Self, Self::Error> {
        let kind = match (wire.entry, wire.name) {
            (None, None) => {
                if !wire.encoded_value.is_empty() {
                    return Err(
                        "value schema terminal sentinel cannot carry an encoded value".to_owned(),
                    );
                }
                CatiaValueSchemaSelectionKind::Terminal
            }
            (Some(entry), Some(name)) => {
                CatiaValueSchemaSelectionKind::Selected(CatiaValueSchemaSelectionValue {
                    class: CatiaDesignClass { entry, name },
                    encoded_value: wire.encoded_value,
                })
            }
            _ => {
                return Err(
                    "value schema selection entry and name must both be present or both absent"
                        .to_owned(),
                );
            }
        };
        Ok(Self {
            id: wire.id,
            parent: wire.parent,
            offset: wire.offset,
            ordinal: wire.ordinal,
            kind,
        })
    }
}

/// One exact `7C02` source-schema catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "CatiaCatalogWire", into = "CatiaCatalogWire")]
pub(crate) struct CatiaCatalog {
    /// Globally unique catalog identity.
    pub(crate) id: String,
    /// Byte offset of the `7C02` marker.
    byte_offset: u64,
    /// Total framed byte length.
    byte_len: u64,
    /// Catalog entries in serialized order.
    #[serde(default)]
    pub(crate) entries: Vec<CatiaCatalogEntry>,
}

// The stored header carries no entries until the entry arena is joined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CatiaCatalogWire {
    /// Globally unique catalog identity.
    id: String,
    /// Byte offset of the `7C02` marker.
    byte_offset: u64,
    /// Total framed byte length.
    byte_len: u64,
    /// Catalog entries in serialized order.
    #[serde(default)]
    entries: Vec<CatiaCatalogEntry>,
}

impl CatiaCatalogWire {
    fn header_charged(ctx: &DecodeContext<'_>, catalog: &CatiaCatalog) -> Result<Self, CodecError> {
        Ok(Self {
            id: ctx.copy_retained_text(&catalog.id, "catia_native_catalog_header_id")?,
            byte_offset: catalog.byte_offset,
            byte_len: catalog.byte_len,
            entries: Vec::new(),
        })
    }
}

impl From<CatiaCatalog> for CatiaCatalogWire {
    fn from(catalog: CatiaCatalog) -> Self {
        Self {
            id: catalog.id,
            byte_offset: catalog.byte_offset,
            byte_len: catalog.byte_len,
            entries: catalog.entries,
        }
    }
}

impl From<CatiaCatalogWire> for CatiaCatalog {
    fn from(wire: CatiaCatalogWire) -> Self {
        Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            byte_len: wire.byte_len,
            entries: wire.entries,
        }
    }
}

/// One source-schema name from a [`CatiaCatalog`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaCatalogEntry {
    /// Globally unique catalog-entry identity.
    pub(crate) id: String,
    /// Containing [`CatiaCatalog`] identity.
    parent: String,
    /// Stable serialized order within the catalog.
    ordinal: u32,
    /// Byte offset of the inclusive length field.
    byte_offset: u64,
    /// Decoded ASCII schema name.
    value: String,
}

/// One definition selector resolved against an object graph's source schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaDefinitionSchemaSelection {
    /// Byte offset of the selector marker within the definition prefix.
    pub(crate) offset: u64,
    /// Stored zero-based source-schema ordinal.
    pub(crate) ordinal: u32,
    /// Selected catalog entry when the ordinal is in range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) entry: Option<String>,
    /// UTF-8 source-schema name stored by the selected entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
}

/// One schema selector and its following encoded `7C07` value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaEntityValueSchemaSelection {
    /// Byte offset of the selector marker within the value payload.
    pub(crate) offset: u64,
    /// Stored zero-based source-schema ordinal.
    pub(crate) ordinal: u32,
    /// Selected catalog entry.
    pub(crate) entry: String,
    /// UTF-8 source-schema name stored by the selected entry.
    pub(crate) name: String,
    /// Complete token sequence after this selector and before the next selector.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) encoded_value: Vec<value_block::ValueField>,
    /// Exact packets wholly contained by `encoded_value`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) packets: Vec<entity_table::EntityValuePacket>,
}

/// One repeated-reference preamble selector resolved through its graph catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaRepeatedReferenceSchemaSelection {
    /// Serialized order of the blob and schema ordinal.
    order: CatiaRepeatedReferenceSchemaOrder,
    /// Byte offset of the schema ordinal within the payload.
    offset: u64,
    /// Stored zero-based source-schema ordinal.
    ordinal: u32,
    /// Selected catalog entry when the ordinal is in range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    entry: Option<String>,
    /// UTF-8 source-schema name stored by the selected entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

/// One exact schema selector used by a typed entity program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaEntitySchemaValue {
    /// Byte offset of the selector within its definition or value payload.
    #[serde(default)]
    pub(crate) offset: u64,
    /// Stored zero-based source-schema ordinal.
    #[serde(default)]
    pub(crate) ordinal: u32,
    /// Selected source-schema entry.
    pub(crate) entry: String,
    /// UTF-8 value stored by the selected entry.
    pub(crate) value: String,
}

/// One complete relation-expression value program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    from = "CatiaRelationExpressionWire",
    into = "CatiaRelationExpressionWire"
)]
pub(crate) struct CatiaRelationExpression {
    /// Exact wire framing and its framing-specific roles.
    pub(crate) framing: CatiaRelationExpressionFraming,
    /// Stored source expression selected by the second value field.
    pub(crate) expression: CatiaEntitySchemaValue,
    /// Exact `param` role selector.
    parameter_role: CatiaEntitySchemaValue,
    /// Stored source type signature.
    type_signature: CatiaEntitySchemaValue,
    /// Exact `RelationExpFct` function selector.
    function_role: CatiaEntitySchemaValue,
}

impl CatiaRelationExpression {
    /// Parsed parameter and value types when the source signature has the typed form.
    pub(crate) fn signature(&self) -> Option<CatiaRelationTypeSignature> {
        let placeholder = match &self.framing {
            CatiaRelationExpressionFraming::PlaceholderState { placeholder, .. } => {
                Some(placeholder.value.as_str())
            }
            CatiaRelationExpressionFraming::ParserVersion { .. }
            | CatiaRelationExpressionFraming::BooleanParserVersion { .. }
            | CatiaRelationExpressionFraming::OpenedBooleanParserVersion { .. } => None,
        };
        relation_type_signature(placeholder, &self.type_signature.value)
    }

    pub(crate) fn signature_charged(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<CatiaRelationTypeSignature>, CodecError> {
        let placeholder = match &self.framing {
            CatiaRelationExpressionFraming::PlaceholderState { placeholder, .. } => {
                Some(placeholder.value.as_str())
            }
            CatiaRelationExpressionFraming::ParserVersion { .. }
            | CatiaRelationExpressionFraming::BooleanParserVersion { .. }
            | CatiaRelationExpressionFraming::OpenedBooleanParserVersion { .. } => None,
        };
        relation_type_signature_charged(ctx, placeholder, &self.type_signature.value)
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct CatiaRelationExpressionWire {
    framing: CatiaRelationExpressionFraming,
    expression: CatiaEntitySchemaValue,
    parameter_role: CatiaEntitySchemaValue,
    type_signature: CatiaEntitySchemaValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    signature: Option<CatiaRelationTypeSignature>,
    function_role: CatiaEntitySchemaValue,
}

impl From<CatiaRelationExpression> for CatiaRelationExpressionWire {
    fn from(value: CatiaRelationExpression) -> Self {
        let signature = value.signature();
        Self::from_with_signature(value, signature)
    }
}

impl CatiaRelationExpressionWire {
    fn from_with_signature(
        value: CatiaRelationExpression,
        signature: Option<CatiaRelationTypeSignature>,
    ) -> Self {
        Self {
            framing: value.framing,
            expression: value.expression,
            parameter_role: value.parameter_role,
            type_signature: value.type_signature,
            signature,
            function_role: value.function_role,
        }
    }
}

impl From<CatiaRelationExpressionWire> for CatiaRelationExpression {
    fn from(wire: CatiaRelationExpressionWire) -> Self {
        Self {
            framing: wire.framing,
            expression: wire.expression,
            parameter_role: wire.parameter_role,
            type_signature: wire.type_signature,
            function_role: wire.function_role,
        }
    }
}

/// Mutually exclusive role framing of one relation-expression value program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CatiaRelationExpressionFraming {
    /// Local placeholder followed by the exact `opened` state selector.
    PlaceholderState {
        /// Expression-local placeholder.
        placeholder: CatiaEntitySchemaValue,
        /// Exact `opened` selector.
        state_role: CatiaEntitySchemaValue,
    },
    /// Exact `ParserVersion` role selector without a prefix role.
    ParserVersion {
        /// Exact `ParserVersion` selector.
        parser_version_role: CatiaEntitySchemaValue,
    },
    /// Exact `Boolean` and `ParserVersion` role selectors.
    BooleanParserVersion {
        /// Exact `Boolean` prefix selector.
        prefix_role: CatiaEntitySchemaValue,
        /// Exact `ParserVersion` selector.
        parser_version_role: CatiaEntitySchemaValue,
    },
    /// Exact `Boolean`, `ParserVersion`, and `opened` role selectors.
    OpenedBooleanParserVersion {
        /// Exact `Boolean` prefix selector.
        prefix_role: CatiaEntitySchemaValue,
        /// Exact `ParserVersion` selector.
        parser_version_role: CatiaEntitySchemaValue,
        /// Exact `opened` selector.
        state_role: CatiaEntitySchemaValue,
    },
}

/// Typed roles in a relation-expression source signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaRelationTypeSignature {
    /// Ordered expression-local inputs named inside the signature.
    pub(crate) inputs: Vec<CatiaRelationTypeInput>,
    /// Source result type named after the closing parenthesis.
    pub(crate) result_type: String,
}

/// One typed input clause in a relation-expression source signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaRelationTypeInput {
    /// Expression-local parameter named before `#In`.
    pub(crate) parameter: String,
    /// Source input type named after `#In`.
    pub(crate) input_type: String,
}

/// Evaluation state of one complete entity-record suffix value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CatiaEntityEvaluation {
    /// The `E7` form carries no evaluated scalar.
    Unset,
    /// The `E6` form carries one finite IEEE-754 binary64 scalar.
    Scalar {
        /// Exact stored binary64 bits.
        bits: u64,
    },
}

/// Wire encoding of one entity-record suffix evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CatiaEntityEvaluationEncoding {
    /// The evaluation opcode directly precedes its payload.
    Direct,
    /// `E6 00 00 00` precedes the scalar's `E6` opcode.
    ZeroPaddedScalar,
}

/// Payload of one complete entity-record suffix value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CatiaEntitySuffixPayload {
    /// An unset or finite scalar evaluation with exact framing.
    Evaluation {
        /// Byte offset of the effective evaluation opcode within the record suffix.
        #[serde(default)]
        opcode_offset: u64,
        /// Stored scalar or unset evaluation.
        evaluation: CatiaEntityEvaluation,
        /// Exact evaluation framing variant.
        encoding: CatiaEntityEvaluationEncoding,
    },
    /// One canonical one-byte atom.
    Atom {
        /// Decoded atom value.
        value: u32,
    },
    /// One source-schema selector followed by one typed value.
    SchemaSelected {
        /// Byte offset of the selector marker within the record suffix.
        #[serde(default)]
        selector_offset: u64,
        /// Stored zero-based source-schema ordinal.
        selector: u32,
        /// Typed value following the selector.
        value: CatiaEntitySuffixSchemaValue,
    },
    /// One zero-payload `E8` control state.
    ControlE8,
    /// One zero-payload `E9` control state.
    ControlE9,
    /// One zero-payload `37` separator.
    Separator37,
}

/// Exact trailer framing of one complete entity-record suffix value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum CatiaEntitySuffixTrailer {
    /// No trailer bytes follow the payload.
    Empty,
    /// Exact trailer token `81 49`.
    Token8149,
    /// Exact trailer token `81 4A`.
    Token814A,
    /// Exact trailer token `81 52`.
    Token8152,
    /// Exact trailer token `81 DB`.
    Token81DB,
    /// Exact trailer token `81 92`.
    Token8192,
    /// Exact trailer token `81 93`.
    Token8193,
    /// Exact fixed trailer `FE F6 00{16}`.
    FixedZeroFrame,
}

/// One complete typed value in an entity-record suffix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaEntitySuffixValue {
    /// Three canonical compact atoms preceding the field code.
    prefix_atoms: [u32; 3],
    /// Stored width of each prefix atom.
    pub(crate) prefix_atom_widths: [u8; 3],
    /// Exact field code preceding the payload.
    prefix_code: u8,
    /// Stored suffix payload.
    pub(crate) payload: CatiaEntitySuffixPayload,
    /// Exact framing closing the suffix production.
    trailer: CatiaEntitySuffixTrailer,
}

/// State byte following one escaped word in an entity-record suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum CatiaEntitySuffixEscapedWordState {
    /// Stored state byte `00`.
    State00,
    /// Stored state byte `01`.
    State01,
    /// Stored state byte `03`.
    State03,
    /// Stored state byte `04`.
    State04,
    /// Stored state byte `09`.
    State09,
}

/// One complete escaped-word entity-record suffix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaEntitySuffixEscapedWord {
    /// Fixed-width little-endian word following the `80` escape.
    word: u32,
    /// Exact trailing state.
    state: CatiaEntitySuffixEscapedWordState,
}

/// One complete non-value entity-record suffix framing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CatiaEntitySuffixFraming {
    /// One escaped fixed-width word followed by an exact state.
    EscapedWord(CatiaEntitySuffixEscapedWord),
    /// Standalone token `81 49`.
    Token8149,
    /// Standalone fixed frame `FE F6 <payload[16]>`.
    FixedFeF6 {
        /// Exact fixed-width payload.
        #[serde(with = "cadmpeg_ir::bytes")]
        payload: Vec<u8>,
    },
    /// One paged compact atom followed by state byte `01`.
    PagedAtomState01 {
        /// Decoded compact-atom value.
        value: u32,
    },
}

/// One suffix selector resolved through its graph's source-schema catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaEntitySuffixSchemaSelection {
    /// Byte offset of the selector marker within the record suffix.
    #[serde(default)]
    pub(crate) offset: u64,
    /// Stored zero-based source-schema ordinal.
    pub(crate) ordinal: u32,
    /// Selected catalog entry.
    pub(crate) entry: String,
    /// UTF-8 source-schema name stored by the selected entry.
    pub(crate) name: String,
    /// Typed value following the selector, with nested schema resolution.
    pub(crate) value: CatiaEntitySuffixSchemaValue,
}

/// Catalog-resolved value following an entity-suffix schema selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CatiaEntitySuffixSchemaValue {
    /// One canonical compact atom.
    Atom {
        /// Decoded atom value.
        value: u32,
    },
    /// One direct unset or finite scalar evaluation.
    Evaluation {
        /// Byte offset of the evaluation opcode within the record suffix.
        #[serde(default)]
        opcode_offset: u64,
        /// Decoded evaluation.
        evaluation: CatiaEntityEvaluation,
    },
    /// One zero-payload `E8` control state.
    ControlE8,
    /// One zero-payload `37` separator.
    Separator37,
    /// One nested source-schema selector.
    SchemaSelector {
        /// Byte offset of the selector marker within the record suffix.
        #[serde(default)]
        offset: u64,
        /// Stored zero-based source-schema ordinal.
        ordinal: u32,
        /// Catalog class selected by `ordinal`.
        #[serde(flatten)]
        resolution: Option<CatiaDesignClass>,
    },
}

/// Unresolved suffix payload value. Same shape as [`CatiaEntitySuffixSchemaValue`].
pub(crate) type CatiaEntitySuffixSelectedValue = CatiaEntitySuffixSchemaValue;

/// One complete named parameter-value record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaParameterValue {
    /// Stored parameter name.
    pub(crate) name: CatiaEntitySchemaValue,
    /// Stored scope, expression, or presentation binding.
    pub(crate) binding: CatiaEntitySchemaValue,
    /// Stored evaluation state.
    pub(crate) evaluation: CatiaEntityEvaluation,
    /// Byte offset of the evaluation opcode within the record suffix.
    #[serde(default)]
    evaluation_opcode_offset: u64,
}

/// One complete source-schema `Range` interval carried by an entity value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaRangeInterval {
    /// Exact source-schema selector naming `Range`.
    pub(crate) range: CatiaEntitySchemaValue,
    /// Complete selected interval framing and nullable slots.
    pub(crate) interval: entity_table::RangeInterval,
    /// Finite nominal carried by an admitted scalar suffix dialect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) nominal: Option<CatiaRangeNominal>,
    /// Exact same-graph payload-reference occurrences selecting this interval.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) incoming_references: Vec<CatiaEntityIncomingReference>,
    /// Exact same-graph object-head storage selectors selecting this interval.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) incoming_storage_references: Vec<CatiaEntityIncomingStorageReference>,
}

/// Exact scalar-suffix dialect associating a nominal with a `Range` interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CatiaRangeNominalFraming {
    /// Prefix code `D8` and trailer `81 93`.
    D8Token8193,
    /// Prefix code `D8` and trailer `81 DB`.
    D8Token81DB,
    /// Prefix code `DC` and trailer `81 DB`.
    DCToken81DB,
    /// Prefix code `DF` and trailer `81 92`.
    DFToken8192,
}

/// One finite nominal associated with a complete `Range` interval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaRangeNominal {
    /// Exact scalar-suffix dialect.
    pub(crate) framing: CatiaRangeNominalFraming,
    /// Exact finite binary64 nominal bits.
    pub(crate) bits: u64,
    /// Byte offset of `E6` within the record suffix.
    #[serde(default)]
    pub(crate) evaluation_opcode_offset: u64,
}

/// Exact framing of one complete constraint-range value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CatiaConstraintRangeFraming {
    /// `CstAttr_Dimension` selected with prefix code `B8`.
    DimensionB8,
    /// `CstAttr_Dimension` selected with prefix code `C1`.
    DimensionC1,
    /// `CstAttr_Dimension` selected with prefix code `DC`.
    DimensionDC,
    /// `CstAttr_Dimension` selected with prefix code `DF` and trailer `81 92`.
    DimensionDF,
    /// `ComplexCst` selected with prefix code `C9`.
    ComplexC9,
}

/// One complete constraint-range value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaConstraintRange {
    /// Exact `Range` role selector.
    pub(crate) range: CatiaEntitySchemaValue,
    /// Exact constraint role selector encoded by `framing`.
    pub(crate) constraint: CatiaEntitySchemaValue,
    /// Exact role and prefix-code framing.
    pub(crate) framing: CatiaConstraintRangeFraming,
    /// Stored evaluation state.
    pub(crate) evaluation: CatiaEntityEvaluation,
    /// Byte offset of the evaluation opcode within the record suffix.
    #[serde(default)]
    pub(crate) evaluation_opcode_offset: u64,
    /// Exact same-graph payload-reference occurrences selecting this range.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) incoming_references: Vec<CatiaEntityIncomingReference>,
    /// Exact same-graph object-head storage selectors selecting this range.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) incoming_storage_references: Vec<CatiaEntityIncomingStorageReference>,
}

/// One exact payload-reference occurrence selecting an entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaEntityIncomingReference {
    /// Object record carrying the reference occurrence.
    pub(crate) object_record: String,
    /// Entity paired with the source object record when that record has an identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_entity: Option<CatiaEntityReference>,
    /// Byte offset of the reference field within that object's payload.
    pub(crate) payload_offset: u64,
    /// Structural container of the reference occurrence.
    pub(crate) source: CatiaObjectRecordReferenceSource,
}

/// One exact object-head storage selector selecting an entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaEntityIncomingStorageReference {
    /// Object record carrying the storage selector.
    pub(crate) object_record: String,
    /// Entity paired with the source object record when that record has an identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_entity: Option<CatiaEntityReference>,
}

/// One definition-selected entity whose complete value occupies its suffix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaDefinitionValue {
    /// Exact source-schema definition selected by the entity.
    pub(crate) definition: CatiaEntitySchemaValue,
    /// Complete typed suffix payload bound to the definition.
    pub(crate) payload: CatiaEntitySuffixPayload,
    /// Catalog-resolved selector when the payload is schema-selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) schema_selection: Option<CatiaEntitySuffixSchemaSelection>,
}

/// One value selected through a complete two-definition role chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaDefinitionChainValue {
    /// Definition repeated by the suffix's fixed-width schema selector.
    pub(crate) selector: CatiaEntitySchemaValue,
    /// Second definition carrying the value's role within the selected schema.
    pub(crate) role: CatiaEntitySchemaValue,
    /// Stored selected value.
    pub(crate) value: CatiaEntitySuffixSchemaValue,
}

/// One complete formula relation stored by an entity and its object payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaFormulaRelation {
    /// Complete relation-expression incidence selected by the second payload reference.
    #[serde(
        default = "default_payload_entity_reference",
        deserialize_with = "deserialize_payload_entity_reference"
    )]
    pub(crate) expression_entity: CatiaPayloadEntityReference,
    /// Output parameter incidence selected by the third payload reference.
    #[serde(
        default = "default_payload_entity_reference",
        deserialize_with = "deserialize_payload_entity_reference"
    )]
    pub(crate) output_entity: CatiaPayloadEntityReference,
    /// Named parameter records selected by expression-local symbols, in occurrence order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) parameter_dependencies: Vec<CatiaRelationParameterDependency>,
}

/// One relation-expression symbol and every matching named parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaRelationParameterDependency {
    /// UTF-8 byte offset of this occurrence within the source expression.
    #[serde(default)]
    source_offset: u64,
    /// Exact expression-local symbol occurrence.
    pub(crate) symbol: String,
    /// Entity incidences carrying matching named parameter bindings.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_relation_dependency_candidates"
    )]
    pub(crate) candidates: Vec<CatiaEntityReference>,
}

/// One declared relation-program input and its uniquely selected entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaRelationProgramInput {
    /// Expression-local parameter in signature order.
    pub(crate) parameter: String,
    /// Declared source value type.
    pub(crate) value_type: String,
    /// Unique same-graph named parameter selected by every source occurrence.
    pub(crate) entity: CatiaEntityReference,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CatiaRelationDependencyCandidate {
    Reference(CatiaEntityReference),
    LegacyEntity(String),
}

fn deserialize_relation_dependency_candidates<'de, D>(
    deserializer: D,
) -> Result<Vec<CatiaEntityReference>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Vec::<CatiaRelationDependencyCandidate>::deserialize(deserializer).map(|candidates| {
        candidates
            .into_iter()
            .map(|candidate| match candidate {
                CatiaRelationDependencyCandidate::Reference(reference) => reference,
                CatiaRelationDependencyCandidate::LegacyEntity(entity) => {
                    CatiaEntityReference::resolved_or_unresolved(0, Some(entity), None)
                }
            })
            .collect()
    })
}

/// One exact compound relation-program instance frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "CatiaRelationProgramInstanceWire",
    into = "CatiaRelationProgramInstanceWire"
)]
pub(crate) struct CatiaRelationProgramInstance {
    /// Exact object-head and payload production with the framing-specific incidence.
    pub(crate) framing: CatiaRelationProgramInstanceFraming,
    /// Entity incidence carried by the frame's program slot.
    pub(crate) program_entity: CatiaEntityReference,
    /// Entity identity stored once as an atom and once as a reference.
    pub(crate) repeated_entity: CatiaEntityReference,
    /// Every reference occurrence in exact payload order, including repeated identities.
    pub(crate) reference_incidences: Vec<CatiaPayloadEntityReference>,
    /// Selected entity when it carries a complete relation-expression program.
    pub(crate) relation_expression: Option<String>,
    /// Named parameter records selected by expression-local symbols, in occurrence order.
    pub(crate) parameter_dependencies: Vec<CatiaRelationParameterDependency>,
    /// Complete declared inputs in signature order; absent when any binding is incomplete.
    pub(crate) inputs: Option<Vec<CatiaRelationProgramInput>>,
}

impl CatiaRelationProgramInstance {
    /// Same-graph incidence carried by the `ref(h)` slot of a lead-`12` frame.
    #[must_use]
    pub(crate) fn lead12_context_entity(&self) -> Option<&CatiaEntityReference> {
        match &self.framing {
            CatiaRelationProgramInstanceFraming::Lead12 { context_entity } => Some(context_entity),
            CatiaRelationProgramInstanceFraming::Lead54 { .. } => None,
        }
    }

    /// Trailing same-graph entity incidence carried only by lead-`54`.
    #[must_use]
    pub(crate) fn lead54_trailing_entity(&self) -> Option<&CatiaEntityReference> {
        match &self.framing {
            CatiaRelationProgramInstanceFraming::Lead54 { trailing_entity } => {
                Some(trailing_entity)
            }
            CatiaRelationProgramInstanceFraming::Lead12 { .. } => None,
        }
    }

    /// Result entity selected by the framing-specific `paramout` slot.
    #[must_use]
    pub(crate) fn output_entity(&self) -> Option<&CatiaEntityReference> {
        let slot = match &self.framing {
            CatiaRelationProgramInstanceFraming::Lead12 { context_entity } => context_entity,
            CatiaRelationProgramInstanceFraming::Lead54 { trailing_entity } => trailing_entity,
        };
        (slot.class_name() == Some("paramout")).then_some(slot)
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct CatiaRelationProgramInstanceWire {
    #[serde(default)]
    framing: CatiaRelationProgramInstanceFramingTag,
    #[serde(default)]
    program_entity: Option<CatiaEntityReference>,
    #[serde(default)]
    repeated_entity: Option<CatiaEntityReference>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_relation_reference_incidences"
    )]
    reference_incidences: Vec<CatiaPayloadEntityReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    relation_expression: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    parameter_dependencies: Vec<CatiaRelationParameterDependency>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    inputs: Option<Vec<CatiaRelationProgramInput>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output_entity: Option<CatiaEntityReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lead12_context_entity: Option<CatiaEntityReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lead54_trailing_entity: Option<CatiaEntityReference>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
enum CatiaRelationProgramInstanceFramingTag {
    #[default]
    Lead12,
    Lead54,
}

impl From<CatiaRelationProgramInstance> for CatiaRelationProgramInstanceWire {
    fn from(value: CatiaRelationProgramInstance) -> Self {
        let output_entity = value.output_entity().cloned();
        Self::from_with_output(value, output_entity)
    }
}

impl CatiaRelationProgramInstanceWire {
    fn from_with_output(
        value: CatiaRelationProgramInstance,
        output_entity: Option<CatiaEntityReference>,
    ) -> Self {
        let (framing, lead12_context_entity, lead54_trailing_entity) = match value.framing {
            CatiaRelationProgramInstanceFraming::Lead12 { context_entity } => (
                CatiaRelationProgramInstanceFramingTag::Lead12,
                Some(context_entity),
                None,
            ),
            CatiaRelationProgramInstanceFraming::Lead54 { trailing_entity } => (
                CatiaRelationProgramInstanceFramingTag::Lead54,
                None,
                Some(trailing_entity),
            ),
        };
        Self {
            framing,
            program_entity: Some(value.program_entity),
            repeated_entity: Some(value.repeated_entity),
            reference_incidences: value.reference_incidences,
            relation_expression: value.relation_expression,
            parameter_dependencies: value.parameter_dependencies,
            inputs: value.inputs,
            output_entity,
            lead12_context_entity,
            lead54_trailing_entity,
        }
    }
}

impl TryFrom<CatiaRelationProgramInstanceWire> for CatiaRelationProgramInstance {
    type Error = String;

    fn try_from(wire: CatiaRelationProgramInstanceWire) -> Result<Self, Self::Error> {
        let framing = match wire.framing {
            CatiaRelationProgramInstanceFramingTag::Lead12 => {
                let context_entity = wire.lead12_context_entity.ok_or_else(|| {
                    "lead-12 relation program requires lead12_context_entity".to_owned()
                })?;
                CatiaRelationProgramInstanceFraming::Lead12 { context_entity }
            }
            CatiaRelationProgramInstanceFramingTag::Lead54 => {
                let trailing_entity = wire.lead54_trailing_entity.ok_or_else(|| {
                    "lead-54 relation program requires lead54_trailing_entity".to_owned()
                })?;
                CatiaRelationProgramInstanceFraming::Lead54 { trailing_entity }
            }
        };
        Ok(Self {
            framing,
            program_entity: wire
                .program_entity
                .unwrap_or(CatiaEntityReference::Unresolved { entity_id: 0 }),
            repeated_entity: wire
                .repeated_entity
                .unwrap_or(CatiaEntityReference::Unresolved { entity_id: 0 }),
            reference_incidences: wire.reference_incidences,
            relation_expression: wire.relation_expression,
            parameter_dependencies: wire.parameter_dependencies,
            inputs: wire.inputs,
        })
    }
}

/// One exact entity-reference occurrence in an object payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaPayloadEntityReference {
    /// Byte offset of the reference field within the object payload.
    payload_offset: u64,
    /// Stored entity identity and its same-graph resolution.
    pub(crate) reference: CatiaEntityReference,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredCatiaPayloadEntityReference {
    Current(CatiaPayloadEntityReference),
    Legacy(CatiaEntityReference),
}

fn deserialize_relation_reference_incidences<'de, D>(
    deserializer: D,
) -> Result<Vec<CatiaPayloadEntityReference>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Vec::<StoredCatiaPayloadEntityReference>::deserialize(deserializer).map(|incidences| {
        incidences
            .into_iter()
            .map(stored_payload_entity_reference)
            .collect()
    })
}

fn deserialize_payload_entity_reference<'de, D>(
    deserializer: D,
) -> Result<CatiaPayloadEntityReference, D::Error>
where
    D: serde::Deserializer<'de>,
{
    StoredCatiaPayloadEntityReference::deserialize(deserializer)
        .map(stored_payload_entity_reference)
}

fn default_payload_entity_reference() -> CatiaPayloadEntityReference {
    CatiaPayloadEntityReference {
        payload_offset: 0,
        reference: CatiaEntityReference::Unresolved { entity_id: 0 },
    }
}

fn stored_payload_entity_reference(
    incidence: StoredCatiaPayloadEntityReference,
) -> CatiaPayloadEntityReference {
    match incidence {
        StoredCatiaPayloadEntityReference::Current(incidence) => incidence,
        StoredCatiaPayloadEntityReference::Legacy(reference) => CatiaPayloadEntityReference {
            payload_offset: 0,
            reference,
        },
    }
}

/// One stored entity identity and its optional same-graph resolution.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(from = "CatiaEntityReferenceWire")]
pub(crate) enum CatiaEntityReference {
    /// The stored identity is the graph's terminal null identity.
    Null { entity_id: u32 },
    /// Stored identity with no same-graph entity.
    Unresolved { entity_id: u32 },
    /// Same-graph entity selected by that identity.
    Resolved {
        entity_id: u32,
        entity: String,
        class_name: Option<String>,
    },
}

impl CatiaEntityReference {
    /// Compares two references after admitting the compared text.
    pub(crate) fn equal_charged(
        &self,
        ctx: &DecodeContext<'_>,
        other: &Self,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        Ok(match (self, other) {
            (Self::Null { entity_id: left }, Self::Null { entity_id: right })
            | (Self::Unresolved { entity_id: left }, Self::Unresolved { entity_id: right }) => {
                left == right
            }
            (
                Self::Resolved {
                    entity_id: left_id,
                    entity: left_entity,
                    class_name: left_class,
                },
                Self::Resolved {
                    entity_id: right_id,
                    entity: right_entity,
                    class_name: right_class,
                },
            ) => {
                left_id == right_id
                    && ctx.equal_bytes(
                        left_entity.as_bytes(),
                        right_entity.as_bytes(),
                        operation,
                    )?
                    && match (left_class, right_class) {
                        (Some(left), Some(right)) => {
                            ctx.equal_bytes(left.as_bytes(), right.as_bytes(), operation)?
                        }
                        (None, None) => true,
                        (Some(_), None) | (None, Some(_)) => false,
                    }
            }
            _ => false,
        })
    }

    /// Stored identity with optional same-graph resolution.
    pub(crate) fn resolved_or_unresolved(
        entity_id: u32,
        entity: Option<String>,
        class_name: Option<String>,
    ) -> Self {
        match entity {
            None => Self::Unresolved { entity_id },
            Some(entity) => Self::Resolved {
                entity_id,
                entity,
                class_name,
            },
        }
    }

    fn copy_charged(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(match self {
            Self::Null { entity_id } => Self::Null {
                entity_id: *entity_id,
            },
            Self::Unresolved { entity_id } => Self::Unresolved {
                entity_id: *entity_id,
            },
            Self::Resolved {
                entity_id,
                entity,
                class_name,
            } => Self::Resolved {
                entity_id: *entity_id,
                entity: ctx.copy_retained_text(entity, "catia_native_reference_entity")?,
                class_name: class_name
                    .as_ref()
                    .map(|class_name| {
                        ctx.copy_retained_text(class_name, "catia_native_reference_class")
                    })
                    .transpose()?,
            },
        })
    }

    pub(crate) fn entity_id(&self) -> u32 {
        match *self {
            Self::Null { entity_id }
            | Self::Unresolved { entity_id }
            | Self::Resolved { entity_id, .. } => entity_id,
        }
    }

    pub(crate) fn is_null(&self) -> bool {
        matches!(self, Self::Null { .. })
    }

    pub(crate) fn entity(&self) -> Option<&str> {
        match self {
            Self::Resolved { entity, .. } => Some(entity.as_str()),
            Self::Null { .. } | Self::Unresolved { .. } => None,
        }
    }

    pub(crate) fn class_name(&self) -> Option<&str> {
        match self {
            Self::Resolved { class_name, .. } => class_name.as_deref(),
            Self::Null { .. } | Self::Unresolved { .. } => None,
        }
    }

    #[cfg(test)]
    fn with_entity_id(self, entity_id: u32) -> Self {
        match self {
            Self::Null { .. } => Self::Null { entity_id },
            Self::Unresolved { .. } => Self::Unresolved { entity_id },
            Self::Resolved {
                entity, class_name, ..
            } => Self::Resolved {
                entity_id,
                entity,
                class_name,
            },
        }
    }

    #[cfg(test)]
    pub(crate) fn without_entity(self) -> Self {
        Self::Unresolved {
            entity_id: self.entity_id(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct CatiaEntityReferenceWire {
    entity_id: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    is_null: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    entity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    class_name: Option<String>,
}

#[derive(Serialize)]
struct CatiaEntityReferenceWireRef<'a> {
    entity_id: u32,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    is_null: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    entity: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    class_name: Option<&'a str>,
}

impl Serialize for CatiaEntityReference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        CatiaEntityReferenceWireRef {
            entity_id: self.entity_id(),
            is_null: self.is_null(),
            entity: self.entity(),
            class_name: self.class_name(),
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
impl From<CatiaEntityReference> for CatiaEntityReferenceWire {
    fn from(value: CatiaEntityReference) -> Self {
        match value {
            CatiaEntityReference::Null { entity_id } => Self {
                entity_id,
                is_null: true,
                entity: None,
                class_name: None,
            },
            CatiaEntityReference::Unresolved { entity_id } => Self {
                entity_id,
                is_null: false,
                entity: None,
                class_name: None,
            },
            CatiaEntityReference::Resolved {
                entity_id,
                entity,
                class_name,
            } => Self {
                entity_id,
                is_null: false,
                entity: Some(entity),
                class_name,
            },
        }
    }
}

impl From<CatiaEntityReferenceWire> for CatiaEntityReference {
    fn from(wire: CatiaEntityReferenceWire) -> Self {
        match (wire.is_null, wire.entity) {
            (true, _) => Self::Null {
                entity_id: wire.entity_id,
            },
            (false, None) => Self::Unresolved {
                entity_id: wire.entity_id,
            },
            (false, Some(entity)) => Self::Resolved {
                entity_id: wire.entity_id,
                entity,
                class_name: wire.class_name,
            },
        }
    }
}

/// One complete reference-signature packet and its same-graph entity incidences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaReferenceSignature {
    /// Exact complete packet production.
    #[serde(flatten)]
    pub(crate) production: entity_table::ReferenceSignature,
    /// Entity incidence selected by the first fixed-width reference.
    pub(crate) first_entity: CatiaEntityReference,
    /// Entity incidence selected by the second fixed-width reference.
    pub(crate) second_entity: CatiaEntityReference,
}

#[derive(Serialize, Deserialize)]
pub(super) struct CatiaReferenceSignatureWire {
    #[serde(flatten)]
    production: entity_table::ReferenceSignatureWire,
    first_entity: CatiaEntityReference,
    second_entity: CatiaEntityReference,
}

impl From<CatiaReferenceSignature> for CatiaReferenceSignatureWire {
    fn from(value: CatiaReferenceSignature) -> Self {
        Self {
            production: value.production.into(),
            first_entity: value.first_entity,
            second_entity: value.second_entity,
        }
    }
}

impl CatiaReferenceSignatureWire {
    fn from_charged(
        ctx: &DecodeContext<'_>,
        value: CatiaReferenceSignature,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            production: entity_table::ReferenceSignatureWire::from_charged(ctx, value.production)?,
            first_entity: value.first_entity,
            second_entity: value.second_entity,
        })
    }
}

impl TryFrom<CatiaReferenceSignatureWire> for CatiaReferenceSignature {
    type Error = String;

    fn try_from(wire: CatiaReferenceSignatureWire) -> Result<Self, Self::Error> {
        Ok(Self {
            production: wire.production.try_into().map_err(str::to_owned)?,
            first_entity: wire.first_entity,
            second_entity: wire.second_entity,
        })
    }
}

/// Source-ordered descriptor records sharing one exact reference pair.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "CatiaReferenceSignatureCohortWire")]
pub(crate) struct CatiaReferenceSignatureCohort {
    references: entity_table::ConsecutiveReferences,
    /// Globally unique cohort identity.
    id: String,
    /// Containing object graph.
    parent: String,
    /// Zero-based order of the cohort's first member within the graph.
    ordinal: u64,
    /// Common same-graph incidence selected by the first identity.
    first_entity: CatiaEntityReference,
    /// Common same-graph incidence selected by the second identity.
    second_entity: CatiaEntityReference,
    /// Unique schema selected by descriptor-bearing members after `_SpecList`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) schema_selection: Option<CatiaReferenceSignatureSchemaSelection>,
    /// Descriptor-bearing entity records in source order.
    pub(crate) members: Vec<String>,
}

impl CatiaReferenceSignatureCohort {
    /// First reference identity shared by the cohort.
    fn first_reference(&self) -> u32 {
        self.references.first()
    }
    /// Second reference identity shared by the cohort.
    fn second_reference(&self) -> u32 {
        self.references.second()
    }
}

#[derive(Serialize, Deserialize)]
struct CatiaReferenceSignatureCohortWire {
    /// Globally unique cohort identity.
    id: String,
    /// Containing object graph.
    parent: String,
    /// Zero-based order of the cohort's first member within the graph.
    ordinal: u64,
    /// First identity shared by every member.
    first_reference: u32,
    /// Common same-graph incidence selected by the first identity.
    first_entity: CatiaEntityReference,
    /// Consecutive second identity shared by every member.
    second_reference: u32,
    /// Common same-graph incidence selected by the second identity.
    second_entity: CatiaEntityReference,
    /// Unique schema selected by descriptor-bearing members after `_SpecList`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    schema_selection: Option<CatiaReferenceSignatureSchemaSelection>,
    /// Descriptor-bearing entity records in source order.
    members: Vec<String>,
}

#[derive(Serialize)]
struct CatiaReferenceSignatureCohortWireRef<'a> {
    id: &'a str,
    parent: &'a str,
    ordinal: u64,
    first_reference: u32,
    first_entity: &'a CatiaEntityReference,
    second_reference: u32,
    second_entity: &'a CatiaEntityReference,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema_selection: &'a Option<CatiaReferenceSignatureSchemaSelection>,
    members: &'a [String],
}

impl Serialize for CatiaReferenceSignatureCohort {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        CatiaReferenceSignatureCohortWireRef {
            id: &self.id,
            parent: &self.parent,
            ordinal: self.ordinal,
            first_reference: self.first_reference(),
            first_entity: &self.first_entity,
            second_reference: self.second_reference(),
            second_entity: &self.second_entity,
            schema_selection: &self.schema_selection,
            members: &self.members,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
impl From<CatiaReferenceSignatureCohort> for CatiaReferenceSignatureCohortWire {
    fn from(value: CatiaReferenceSignatureCohort) -> Self {
        Self {
            first_reference: value.first_reference(),
            second_reference: value.second_reference(),
            id: value.id,
            parent: value.parent,
            ordinal: value.ordinal,
            first_entity: value.first_entity,
            second_entity: value.second_entity,
            schema_selection: value.schema_selection,
            members: value.members,
        }
    }
}
impl TryFrom<CatiaReferenceSignatureCohortWire> for CatiaReferenceSignatureCohort {
    type Error = &'static str;
    fn try_from(wire: CatiaReferenceSignatureCohortWire) -> Result<Self, Self::Error> {
        let references = entity_table::ConsecutiveReferences::new(wire.first_reference)
            .ok_or("first_reference has no consecutive successor")?;
        if references.second() != wire.second_reference {
            return Err("second_reference must follow first_reference");
        }
        Ok(Self {
            references,
            id: wire.id,
            parent: wire.parent,
            ordinal: wire.ordinal,
            first_entity: wire.first_entity,
            second_entity: wire.second_entity,
            schema_selection: wire.schema_selection,
            members: wire.members,
        })
    }
}

/// Cohort-level schema incidence selected after the `_SpecList` marker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaReferenceSignatureSchemaSelection {
    /// Stored zero-based source-schema ordinal.
    ordinal: u32,
    /// Selected catalog entry.
    entry: String,
    /// UTF-8 source-schema name stored by the selected entry.
    name: String,
}

/// One exact self-defining schema-configuration `Configuration` object production.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaSchemaConfigurationRecord {
    /// Byte offset of the schema reference within the object payload.
    #[serde(default)]
    schema_payload_offset: u64,
    /// Stored value-schema ordinal selected by the first reference.
    schema_ordinal: u32,
    /// Selected schema-catalog entry.
    schema_entry: String,
    /// Selected schema-catalog name.
    schema_name: String,
    /// Entity selected by the second stored reference.
    #[serde(deserialize_with = "deserialize_payload_entity_reference")]
    pub(crate) entity_reference: CatiaPayloadEntityReference,
}

/// One exact schema-configuration `configrow` successor-link production.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaSchemaConfigurationRowLink {
    /// Stored class identity whose catalog name is `configrow`.
    pub(crate) class_reference: CatiaEntityReference,
    /// Byte offset of the successor atom within the object payload.
    #[serde(default)]
    successor_payload_offset: u64,
    /// Stored successor identity.
    pub(crate) successor: CatiaEntityReference,
}

/// Exact framing production for a compound relation-program instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CatiaRelationProgramInstanceFraming {
    /// Compact `0x12` object head and its 20-token payload.
    Lead12 {
        /// Same-graph incidence carried by the `ref(h)` slot.
        context_entity: CatiaEntityReference,
    },
    /// Separator-form `0x54` object head and its 18-token payload.
    Lead54 {
        /// Trailing same-graph entity incidence.
        trailing_entity: CatiaEntityReference,
    },
}

/// Field order used by a repeated-reference schema preamble.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum CatiaRepeatedReferenceSchemaOrder {
    /// The binary descriptor precedes the schema ordinal.
    BlobThenSchema,
    /// The schema ordinal precedes the binary descriptor.
    SchemaThenBlob,
}

/// One outer `7C08` ownership graph in source order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaObjectGraph {
    /// Globally unique graph identity.
    pub(crate) id: String,
    /// Byte offset of the `7C08` root.
    pub(crate) byte_offset: u64,
    /// Total framed byte length.
    pub(crate) byte_len: u64,
    /// Physically containing FINJPL segment, when the graph is not in the outer preamble.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) finjpl_segment: Option<String>,
    /// Exact declared outer container whose physical stream contains this graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) outer_container: Option<CatiaOuterContainerBinding>,
    /// Byte offset of the associated schema catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) catalog_byte_offset: Option<u64>,
    /// Associated schema catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) catalog: Option<String>,
    /// Consecutive `7C09` records in serialized order.
    #[serde(default)]
    pub(crate) records: Vec<CatiaObjectRecord>,
}

/// Outer `Data` declaration and its selected physical stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaOuterContainerBinding {
    /// Byte offset of the declaration in the reconstructed outer `Data` stream.
    pub(crate) data_offset: u64,
    /// Source ordinal stored by the declaration.
    pub(crate) ordinal: u32,
    /// Concrete container class.
    pub(crate) class_name: String,
    /// Declared base container class.
    pub(crate) base_class: String,
    /// Resolved UUID-derived outer stream name.
    pub(crate) stream_name: String,
}

/// Paired entity-table identity for one object record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CatiaObjectEntity {
    /// Positionally paired `7C05` entity-table record.
    pub(crate) record: String,
    /// Stored entity-table identity used to select this record.
    pub(crate) id: u32,
}

/// Class role resolved through the graph schema catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CatiaObjectClass {
    /// Head role identifying the per-file class ordinal.
    pub(crate) ordinal: u32,
    /// UTF-8 class name resolved through the graph's schema catalog.
    pub(crate) name: Option<String>,
    /// Exact schema-catalog entry selected by `class_ref`.
    pub(crate) entry: Option<String>,
}

/// Storage role resolved through the same graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CatiaObjectStorage {
    /// Head role selecting class-specific storage.
    pub(crate) reference: u32,
    /// Same-graph field record selected by `storage_ref`.
    pub(crate) record: Option<String>,
    /// Design object containing the selected storage record.
    pub(crate) design_object: Option<String>,
}

/// One `7C09` object record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CatiaObjectRecordWire", into = "CatiaObjectRecordWire")]
pub(crate) struct CatiaObjectRecord {
    /// Globally unique record identity.
    pub(crate) id: String,
    /// Containing [`CatiaObjectGraph`] identity.
    pub(crate) parent: String,
    /// Design object selected by this record's owner entity identity.
    pub(crate) design_object: Option<String>,
    /// Paired entity-table identity.
    pub(crate) entity: Option<CatiaObjectEntity>,
    /// Stable serialized order within the graph.
    pub(crate) ordinal: u64,
    /// Byte offset of the `7C09` record.
    pub(crate) byte_offset: u64,
    /// Total framed byte length.
    pub(crate) byte_len: u64,
    /// First head byte.
    pub(crate) lead: u8,
    /// Decoded head tokens in serialized order.
    pub(crate) head: Vec<HeadToken>,
    /// Complete alternate inline body when the record has no nested `7C0A`.
    pub(crate) inline_body: Option<Vec<u8>>,
    /// Structurally assigned owner slot.
    pub(crate) owner: Option<CatiaObjectOwner>,
    /// Class role when the head carries a class ordinal.
    pub(crate) class: Option<CatiaObjectClass>,
    /// Storage role when the head carries a storage ordinal.
    pub(crate) storage: Option<CatiaObjectStorage>,
    /// Typed nested payload, empty for an inline record.
    pub(crate) payload: ObjectPayload,
    /// Repeated-reference preamble selector resolved through the graph catalog.
    pub(crate) repeated_reference_schema_selection: Option<CatiaRepeatedReferenceSchemaSelection>,
    /// Ordered same-graph payload-reference links.
    pub(crate) references: Vec<CatiaObjectRecordReference>,
}

/// Structurally assigned owner role in a `7C09` head.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CatiaObjectOwner {
    /// Stored entity identity selecting the design object.
    Entity(u32),
    /// Literal occupying the assigned slot without establishing ownership.
    UnassignedLiteral(u8),
}

impl From<object_graph::HeadOwner> for CatiaObjectOwner {
    fn from(owner: object_graph::HeadOwner) -> Self {
        match owner {
            object_graph::HeadOwner::Entity(value) => Self::Entity(value),
            object_graph::HeadOwner::UnassignedLiteral(value) => Self::UnassignedLiteral(value),
        }
    }
}

impl CatiaObjectRecord {
    /// Structural payload classification.
    pub(crate) fn subtype(&self) -> PayloadSubtype {
        object_graph::classify(&self.payload.fields)
    }
    /// Counted reference suffix of the payload.
    pub(crate) fn repeated_reference_suffix(
        &self,
    ) -> Option<object_graph::RepeatedReferenceSuffix> {
        object_graph::repeated_reference_suffix(&self.payload)
    }

    pub(crate) fn entity_record(&self) -> Option<&str> {
        self.entity.as_ref().map(|entity| entity.record.as_str())
    }

    pub(crate) fn entity_id(&self) -> Option<u32> {
        self.entity.as_ref().map(|entity| entity.id)
    }

    fn class_ref(&self) -> Option<u32> {
        self.class.as_ref().map(|class| class.ordinal)
    }

    pub(crate) fn class_name(&self) -> Option<&str> {
        self.class.as_ref().and_then(|class| class.name.as_deref())
    }

    pub(crate) fn class_entry(&self) -> Option<&str> {
        self.class.as_ref().and_then(|class| class.entry.as_deref())
    }

    pub(crate) fn storage_ref(&self) -> Option<u32> {
        self.storage.as_ref().map(|storage| storage.reference)
    }

    pub(crate) fn storage_record(&self) -> Option<&str> {
        self.storage
            .as_ref()
            .and_then(|storage| storage.record.as_deref())
    }

    fn storage_design_object(&self) -> Option<&str> {
        self.storage
            .as_ref()
            .and_then(|storage| storage.design_object.as_deref())
    }

    pub(crate) fn owner_entity_id(&self) -> Option<u32> {
        match self.owner {
            Some(CatiaObjectOwner::Entity(entity_id)) => Some(entity_id),
            Some(CatiaObjectOwner::UnassignedLiteral(_)) | None => None,
        }
    }

    pub(crate) fn has_unassigned_owner(&self) -> bool {
        matches!(self.owner, Some(CatiaObjectOwner::UnassignedLiteral(_)))
    }
}

#[derive(Serialize, Deserialize)]
struct CatiaObjectRecordWire {
    id: String,
    parent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    design_object: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    entity_record: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    entity_id: Option<u32>,
    ordinal: u64,
    byte_offset: u64,
    byte_len: u64,
    lead: u8,
    head: Vec<HeadToken>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    inline_body: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner: Option<CatiaObjectOwner>,
    class_ref: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    class_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    class_entry: Option<String>,
    storage_ref: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    storage_record: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    storage_design_object: Option<String>,
    payload: ObjectPayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    repeated_reference_suffix: Option<object_graph::RepeatedReferenceSuffix>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    repeated_reference_schema_selection: Option<CatiaRepeatedReferenceSchemaSelection>,
    subtype: PayloadSubtype,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    references: Vec<CatiaObjectRecordReference>,
}

impl From<CatiaObjectRecord> for CatiaObjectRecordWire {
    fn from(value: CatiaObjectRecord) -> Self {
        let subtype = value.subtype();
        let repeated_reference_suffix = value.repeated_reference_suffix();
        Self::from_parts(value, subtype, repeated_reference_suffix)
    }
}

impl CatiaObjectRecordWire {
    fn from_charged(ctx: &DecodeContext<'_>, value: CatiaObjectRecord) -> Result<Self, CodecError> {
        let subtype = value.subtype();
        let suffix = object_graph::repeated_reference_suffix_charged(ctx, &value.payload)?;
        Ok(Self::from_parts(value, subtype, suffix))
    }

    fn from_parts(
        value: CatiaObjectRecord,
        subtype: PayloadSubtype,
        repeated_reference_suffix: Option<object_graph::RepeatedReferenceSuffix>,
    ) -> Self {
        let (entity_record, entity_id) = match value.entity {
            Some(entity) => (Some(entity.record), Some(entity.id)),
            None => (None, None),
        };
        let (class_ref, class_name, class_entry) = match value.class {
            Some(class) => (Some(class.ordinal), class.name, class.entry),
            None => (None, None, None),
        };
        let (storage_ref, storage_record, storage_design_object) = match value.storage {
            Some(storage) => (
                Some(storage.reference),
                storage.record,
                storage.design_object,
            ),
            None => (None, None, None),
        };
        Self {
            id: value.id,
            parent: value.parent,
            design_object: value.design_object,
            entity_record,
            entity_id,
            ordinal: value.ordinal,
            byte_offset: value.byte_offset,
            byte_len: value.byte_len,
            lead: value.lead,
            head: value.head,
            inline_body: value.inline_body,
            owner: value.owner,
            class_ref,
            class_name,
            class_entry,
            storage_ref,
            storage_record,
            storage_design_object,
            payload: value.payload,
            repeated_reference_suffix,
            repeated_reference_schema_selection: value.repeated_reference_schema_selection,
            subtype,
            references: value.references,
        }
    }
}

impl TryFrom<CatiaObjectRecordWire> for CatiaObjectRecord {
    type Error = String;

    fn try_from(wire: CatiaObjectRecordWire) -> Result<Self, Self::Error> {
        if wire.subtype != object_graph::classify(&wire.payload.fields) {
            return Err("subtype disagrees with payload".to_owned());
        }
        if wire.repeated_reference_suffix != object_graph::repeated_reference_suffix(&wire.payload)
        {
            return Err("repeated_reference_suffix disagrees with payload".to_owned());
        }

        let entity = match (wire.entity_record, wire.entity_id) {
            (None, None) => None,
            (Some(record), Some(id)) => Some(CatiaObjectEntity { record, id }),
            _ => {
                return Err(
                    "object record entity_record and entity_id must both be present or both absent"
                        .to_owned(),
                );
            }
        };
        let class = match wire.class_ref {
            Some(class_ref) => Some(CatiaObjectClass {
                ordinal: class_ref,
                name: wire.class_name,
                entry: wire.class_entry,
            }),
            None if wire.class_name.is_some() || wire.class_entry.is_some() => {
                return Err("object record class name/entry requires class_ref".to_owned());
            }
            None => None,
        };
        let storage = match wire.storage_ref {
            Some(storage_ref) => Some(CatiaObjectStorage {
                reference: storage_ref,
                record: wire.storage_record,
                design_object: wire.storage_design_object,
            }),
            None if wire.storage_record.is_some() || wire.storage_design_object.is_some() => {
                return Err("object record storage links require storage_ref".to_owned());
            }
            None => None,
        };
        Ok(Self {
            id: wire.id,
            parent: wire.parent,
            design_object: wire.design_object,
            entity,
            ordinal: wire.ordinal,
            byte_offset: wire.byte_offset,
            byte_len: wire.byte_len,
            lead: wire.lead,
            head: wire.head,
            inline_body: wire.inline_body,
            owner: wire.owner,
            class,
            storage,
            payload: wire.payload,
            repeated_reference_schema_selection: wire.repeated_reference_schema_selection,
            references: wire.references,
        })
    }
}

/// One typed payload reference from a `7C09` record.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(from = "CatiaObjectRecordReferenceWire")]
pub(crate) enum CatiaObjectRecordReference {
    /// The stored identity is the graph's terminal null identity.
    Null {
        entity_id: u32,
        payload_offset: u64,
        source: CatiaObjectRecordReferenceSource,
    },
    /// Stored identity with no same-graph target.
    Unresolved {
        entity_id: u32,
        payload_offset: u64,
        source: CatiaObjectRecordReferenceSource,
    },
    /// Same-graph record selected by that identity.
    Resolved {
        entity_id: u32,
        payload_offset: u64,
        source: CatiaObjectRecordReferenceSource,
        target: String,
        design_object: Option<String>,
    },
}

impl CatiaObjectRecordReference {
    pub(crate) fn from_parts(
        entity_id: u32,
        payload_offset: u64,
        source: CatiaObjectRecordReferenceSource,
        is_null: bool,
        target: Option<String>,
        design_object: Option<String>,
    ) -> Self {
        match (is_null, target) {
            (true, _) => Self::Null {
                entity_id,
                payload_offset,
                source,
            },
            (false, None) => Self::Unresolved {
                entity_id,
                payload_offset,
                source,
            },
            (false, Some(target)) => Self::Resolved {
                entity_id,
                payload_offset,
                source,
                target,
                design_object,
            },
        }
    }

    pub(crate) fn entity_id(&self) -> u32 {
        match *self {
            Self::Null { entity_id, .. }
            | Self::Unresolved { entity_id, .. }
            | Self::Resolved { entity_id, .. } => entity_id,
        }
    }

    pub(crate) fn payload_offset(&self) -> u64 {
        match *self {
            Self::Null { payload_offset, .. }
            | Self::Unresolved { payload_offset, .. }
            | Self::Resolved { payload_offset, .. } => payload_offset,
        }
    }

    pub(crate) fn source(&self) -> &CatiaObjectRecordReferenceSource {
        match self {
            Self::Null { source, .. }
            | Self::Unresolved { source, .. }
            | Self::Resolved { source, .. } => source,
        }
    }

    pub(crate) fn is_null(&self) -> bool {
        matches!(self, Self::Null { .. })
    }

    pub(crate) fn target(&self) -> Option<&str> {
        match self {
            Self::Resolved { target, .. } => Some(target.as_str()),
            Self::Null { .. } | Self::Unresolved { .. } => None,
        }
    }

    pub(crate) fn design_object(&self) -> Option<&str> {
        match self {
            Self::Resolved { design_object, .. } => design_object.as_deref(),
            Self::Null { .. } | Self::Unresolved { .. } => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct CatiaObjectRecordReferenceWire {
    entity_id: u32,
    payload_offset: u64,
    source: CatiaObjectRecordReferenceSource,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    is_null: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    design_object: Option<String>,
}

#[derive(Serialize)]
struct CatiaObjectRecordReferenceWireRef<'a> {
    entity_id: u32,
    payload_offset: u64,
    source: &'a CatiaObjectRecordReferenceSource,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    is_null: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    design_object: Option<&'a str>,
}

impl Serialize for CatiaObjectRecordReference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        CatiaObjectRecordReferenceWireRef {
            entity_id: self.entity_id(),
            payload_offset: self.payload_offset(),
            source: self.source(),
            is_null: self.is_null(),
            target: self.target(),
            design_object: self.design_object(),
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
impl From<CatiaObjectRecordReference> for CatiaObjectRecordReferenceWire {
    fn from(value: CatiaObjectRecordReference) -> Self {
        match value {
            CatiaObjectRecordReference::Null {
                entity_id,
                payload_offset,
                source,
            } => Self {
                entity_id,
                payload_offset,
                source,
                is_null: true,
                target: None,
                design_object: None,
            },
            CatiaObjectRecordReference::Unresolved {
                entity_id,
                payload_offset,
                source,
            } => Self {
                entity_id,
                payload_offset,
                source,
                is_null: false,
                target: None,
                design_object: None,
            },
            CatiaObjectRecordReference::Resolved {
                entity_id,
                payload_offset,
                source,
                target,
                design_object,
            } => Self {
                entity_id,
                payload_offset,
                source,
                is_null: false,
                target: Some(target),
                design_object,
            },
        }
    }
}

impl From<CatiaObjectRecordReferenceWire> for CatiaObjectRecordReference {
    fn from(wire: CatiaObjectRecordReferenceWire) -> Self {
        Self::from_parts(
            wire.entity_id,
            wire.payload_offset,
            wire.source,
            wire.is_null,
            wire.target,
            wire.design_object,
        )
    }
}

/// Structural container of one payload-reference occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CatiaObjectRecordReferenceSource {
    /// Standalone compact or fixed-width payload field.
    Field,
    /// Item in one count-framed list.
    ListItem {
        /// Byte offset of the list's `0x3b` tag within the payload.
        list_payload_offset: u64,
        /// Zero-based item position within the list, including atom items.
        item_ordinal: u64,
    },
}

/// One exact schema class retained on a grouped design object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaDesignClass {
    /// Selected source-schema entry.
    pub(crate) entry: String,
    /// UTF-8 class name stored by the entry.
    pub(crate) name: String,
}

/// One exact outbound relation occurrence in a grouped design object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaDesignObjectRelation {
    /// Field record containing the relation.
    pub(crate) source_field: String,
    /// Exact schema class of the source field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_class: Option<CatiaDesignClass>,
    /// Structural source of the relation occurrence.
    pub(crate) source: CatiaDesignObjectRelationSource,
    /// Stored target entity identity.
    pub(crate) target_entity_id: u32,
    /// Exact field record selected by the stored identity.
    pub(crate) target_field: String,
    /// Exact schema class of the selected target field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) target_class: Option<CatiaDesignClass>,
    /// Design object containing the selected field record, when it has an owner group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) target_design_object: Option<String>,
}

/// Structural source of one exact outbound relation occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CatiaDesignObjectRelationSource {
    /// Class-specific storage selector in the field-record head.
    Storage,
    /// Reference occurrence in the field-record payload.
    Payload {
        /// Byte offset of the reference occurrence within the payload.
        payload_offset: u64,
        /// Structural container of the payload reference occurrence.
        container: CatiaObjectRecordReferenceSource,
    },
}

/// One cell in a row-aligned design-object reference table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(from = "CatiaDesignReferenceCellWire")]
pub(crate) enum CatiaDesignReferenceCell {
    /// The stored identity is the graph's terminal null identity.
    Null { payload_offset: u64, entity_id: u32 },
    /// Stored identity with no same-graph field.
    Unresolved { payload_offset: u64, entity_id: u32 },
    /// Same-graph field selected by that identity.
    Resolved {
        payload_offset: u64,
        entity_id: u32,
        field: String,
        field_class: Option<CatiaDesignClass>,
        design_object: Option<String>,
    },
}

impl CatiaDesignReferenceCell {
    fn from_parts(
        payload_offset: u64,
        entity_id: u32,
        is_null: bool,
        field: Option<String>,
        field_class: Option<CatiaDesignClass>,
        design_object: Option<String>,
    ) -> Self {
        match (is_null, field) {
            (true, _) => Self::Null {
                payload_offset,
                entity_id,
            },
            (false, None) => Self::Unresolved {
                payload_offset,
                entity_id,
            },
            (false, Some(field)) => Self::Resolved {
                payload_offset,
                entity_id,
                field,
                field_class,
                design_object,
            },
        }
    }

    #[cfg(test)]
    fn payload_offset(&self) -> u64 {
        match *self {
            Self::Null { payload_offset, .. }
            | Self::Unresolved { payload_offset, .. }
            | Self::Resolved { payload_offset, .. } => payload_offset,
        }
    }

    #[cfg(test)]
    fn entity_id(&self) -> u32 {
        match *self {
            Self::Null { entity_id, .. }
            | Self::Unresolved { entity_id, .. }
            | Self::Resolved { entity_id, .. } => entity_id,
        }
    }

    pub(crate) fn is_null(&self) -> bool {
        matches!(self, Self::Null { .. })
    }

    pub(crate) fn field(&self) -> Option<&str> {
        match self {
            Self::Resolved { field, .. } => Some(field.as_str()),
            Self::Null { .. } | Self::Unresolved { .. } => None,
        }
    }

    pub(crate) fn field_class(&self) -> Option<&CatiaDesignClass> {
        match self {
            Self::Resolved { field_class, .. } => field_class.as_ref(),
            Self::Null { .. } | Self::Unresolved { .. } => None,
        }
    }

    fn design_object(&self) -> Option<&str> {
        match self {
            Self::Resolved { design_object, .. } => design_object.as_deref(),
            Self::Null { .. } | Self::Unresolved { .. } => None,
        }
    }

    #[cfg(test)]
    fn with_payload_offset(self, payload_offset: u64) -> Self {
        match self {
            Self::Null { entity_id, .. } => Self::Null {
                payload_offset,
                entity_id,
            },
            Self::Unresolved { entity_id, .. } => Self::Unresolved {
                payload_offset,
                entity_id,
            },
            Self::Resolved {
                entity_id,
                field,
                field_class,
                design_object,
                ..
            } => Self::Resolved {
                payload_offset,
                entity_id,
                field,
                field_class,
                design_object,
            },
        }
    }

    #[cfg(test)]
    fn with_entity_id(self, entity_id: u32) -> Self {
        match self {
            Self::Null { payload_offset, .. } => Self::Null {
                payload_offset,
                entity_id,
            },
            Self::Unresolved { payload_offset, .. } => Self::Unresolved {
                payload_offset,
                entity_id,
            },
            Self::Resolved {
                payload_offset,
                field,
                field_class,
                design_object,
                ..
            } => Self::Resolved {
                payload_offset,
                entity_id,
                field,
                field_class,
                design_object,
            },
        }
    }
}

#[derive(Serialize, Deserialize)]
struct CatiaDesignReferenceCellWire {
    #[serde(default)]
    payload_offset: u64,
    entity_id: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    is_null: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    field_class: Option<CatiaDesignClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    design_object: Option<String>,
}

#[derive(Serialize)]
struct CatiaDesignReferenceCellWireRef<'a> {
    payload_offset: u64,
    entity_id: u32,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    is_null: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    field: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    field_class: Option<&'a CatiaDesignClass>,
    #[serde(skip_serializing_if = "Option::is_none")]
    design_object: Option<&'a str>,
}

impl Serialize for CatiaDesignReferenceCell {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (payload_offset, entity_id) = match self {
            Self::Null {
                payload_offset,
                entity_id,
            }
            | Self::Unresolved {
                payload_offset,
                entity_id,
            }
            | Self::Resolved {
                payload_offset,
                entity_id,
                ..
            } => (*payload_offset, *entity_id),
        };
        CatiaDesignReferenceCellWireRef {
            payload_offset,
            entity_id,
            is_null: self.is_null(),
            field: self.field(),
            field_class: self.field_class(),
            design_object: self.design_object(),
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
impl From<CatiaDesignReferenceCell> for CatiaDesignReferenceCellWire {
    fn from(value: CatiaDesignReferenceCell) -> Self {
        match value {
            CatiaDesignReferenceCell::Null {
                payload_offset,
                entity_id,
            } => Self {
                payload_offset,
                entity_id,
                is_null: true,
                field: None,
                field_class: None,
                design_object: None,
            },
            CatiaDesignReferenceCell::Unresolved {
                payload_offset,
                entity_id,
            } => Self {
                payload_offset,
                entity_id,
                is_null: false,
                field: None,
                field_class: None,
                design_object: None,
            },
            CatiaDesignReferenceCell::Resolved {
                payload_offset,
                entity_id,
                field,
                field_class,
                design_object,
            } => Self {
                payload_offset,
                entity_id,
                is_null: false,
                field: Some(field),
                field_class,
                design_object,
            },
        }
    }
}

impl From<CatiaDesignReferenceCellWire> for CatiaDesignReferenceCell {
    fn from(wire: CatiaDesignReferenceCellWire) -> Self {
        Self::from_parts(
            wire.payload_offset,
            wire.entity_id,
            wire.is_null,
            wire.field,
            wire.field_class,
            wire.design_object,
        )
    }
}

/// One source-ordered row in a parallel design-object reference table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaDesignReferenceRow {
    /// Cells in the order of the table's source fields.
    pub(crate) cells: Vec<CatiaDesignReferenceCell>,
    /// Design object containing distinct selected fields whose classes equal every column class.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) matching_design_object: Option<String>,
}

/// One source field and list framing forming a parallel-reference table column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaDesignReferenceColumn {
    /// Source field record containing the reference list.
    field: String,
    /// Exact source field class when its schema ordinal resolves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) field_class: Option<CatiaDesignClass>,
    /// Byte offset of the list tag within the source field's payload.
    #[serde(default)]
    list_payload_offset: u64,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredCatiaDesignReferenceColumn {
    Current(CatiaDesignReferenceColumn),
    LegacyField(String),
}

fn deserialize_design_reference_columns<'de, D>(
    deserializer: D,
) -> Result<Vec<CatiaDesignReferenceColumn>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Vec::<StoredCatiaDesignReferenceColumn>::deserialize(deserializer).map(|columns| {
        columns
            .into_iter()
            .map(|column| match column {
                StoredCatiaDesignReferenceColumn::Current(column) => column,
                StoredCatiaDesignReferenceColumn::LegacyField(field) => {
                    CatiaDesignReferenceColumn {
                        field,
                        field_class: None,
                        list_payload_offset: 0,
                    }
                }
            })
            .collect()
    })
}

/// Equal-cardinality reference lists aligned by list-item ordinal.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "CatiaDesignParallelReferenceTableWire")]
pub(crate) struct CatiaDesignParallelReferenceTable {
    columns: Vec<CatiaDesignReferenceColumn>,
    rows: Vec<CatiaDesignReferenceRow>,
}

#[derive(Serialize, Deserialize)]
struct CatiaDesignParallelReferenceTableWire {
    #[serde(deserialize_with = "deserialize_design_reference_columns")]
    columns: Vec<CatiaDesignReferenceColumn>,
    rows: Vec<CatiaDesignReferenceRow>,
}

#[derive(Serialize)]
struct CatiaDesignParallelReferenceTableWireRef<'a> {
    columns: &'a [CatiaDesignReferenceColumn],
    rows: &'a [CatiaDesignReferenceRow],
}

impl Serialize for CatiaDesignParallelReferenceTable {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        CatiaDesignParallelReferenceTableWireRef {
            columns: &self.columns,
            rows: &self.rows,
        }
        .serialize(serializer)
    }
}

impl CatiaDesignParallelReferenceTable {
    fn new(
        ctx: &DecodeContext<'_>,
        columns: Vec<CatiaDesignReferenceColumn>,
        rows: Vec<CatiaDesignReferenceRow>,
    ) -> Result<Option<Self>, CodecError> {
        let rows_match = ctx
            .admit_iter(&rows, "catia_design_reference_table_row_visits")?
            .all(|row| row.cells.len() == columns.len());
        Ok(rows_match.then_some(Self { columns, rows }))
    }

    pub(crate) fn columns(&self) -> &[CatiaDesignReferenceColumn] {
        &self.columns
    }

    pub(crate) fn rows(&self) -> &[CatiaDesignReferenceRow] {
        &self.rows
    }
}

#[cfg(test)]
impl From<CatiaDesignParallelReferenceTable> for CatiaDesignParallelReferenceTableWire {
    fn from(value: CatiaDesignParallelReferenceTable) -> Self {
        Self {
            columns: value.columns,
            rows: value.rows,
        }
    }
}

impl TryFrom<CatiaDesignParallelReferenceTableWire> for CatiaDesignParallelReferenceTable {
    type Error = String;

    fn try_from(wire: CatiaDesignParallelReferenceTableWire) -> Result<Self, Self::Error> {
        if !wire
            .rows
            .iter()
            .all(|row| row.cells.len() == wire.columns.len())
        {
            return Err("parallel reference row width disagrees with column count".to_owned());
        }
        Ok(Self {
            columns: wire.columns,
            rows: wire.rows,
        })
    }
}

/// One serialized design object formed by a shared `7C09` owner identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaDesignObject {
    /// Globally unique design-object identity.
    pub(crate) id: String,
    /// Containing [`CatiaObjectGraph`] identity.
    pub(crate) parent: String,
    /// Zero-based order of this owner group by its first field in the graph.
    pub(crate) ordinal: u64,
    /// Byte offset of the first field carrying this owner identity.
    pub(crate) first_field_byte_offset: u64,
    /// Owner entity identity stored by every field record.
    pub(crate) owner_entity_id: u32,
    /// Record selected by `owner_entity_id` when it lies inside the graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) owner_record: Option<String>,
    /// Design object whose field set contains `owner_record`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) owner_design_object: Option<String>,
    /// Exact class of a separator-form owner declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) owner_class: Option<CatiaDesignClass>,
    /// Class-specific storage selector of a separator-form owner declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) owner_storage_ref: Option<u32>,
    /// Field records carrying this owner identity, in serialized order.
    pub(crate) fields: Vec<String>,
    /// Distinct exact field classes, in first field order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) field_classes: Vec<CatiaDesignClass>,
    /// Entity records carrying definition-bound values, in field order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) definition_values: Vec<String>,
    /// Entity records carrying two-definition values, in field order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) definition_chain_values: Vec<String>,
    /// Exact inter-object reference occurrences in field and payload order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) relations: Vec<CatiaDesignObjectRelation>,
    /// Complete row-aligned table formed by parallel all-reference list fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) parallel_reference_table: Option<CatiaDesignParallelReferenceTable>,
}

/// Record positions of one graph by entity id, where a repeated id keeps its
/// last record, and the graph's terminal null id.
pub(crate) struct GraphRecordIndex {
    positions: HashMap<u32, usize>,
    terminal_null_entity_id: Option<u32>,
}

impl GraphRecordIndex {
    /// Builds the index in the caller's scoped storage.
    pub(crate) fn new(
        ctx: &DecodeContext<'_>,
        storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
        records: &[CatiaObjectRecord],
    ) -> Result<Self, CodecError> {
        let positions = storage.with_storage(|| {
            ctx.collect_hash_map(
                ctx.admit_iter(records, "catia_native_graph_index_source_visits")?
                    .enumerate()
                    .filter_map(|(index, record)| Some((record.entity_id()?, index))),
                "catia_native_graph_record_indices",
            )
        })?;
        Ok(Self {
            positions,
            terminal_null_entity_id: terminal_null_entity_id(ctx, records)?,
        })
    }

    fn record<'r>(
        &self,
        ctx: &DecodeContext<'_>,
        records: &'r [CatiaObjectRecord],
        entity_id: u32,
    ) -> Result<Option<&'r CatiaObjectRecord>, CodecError> {
        Ok(ctx
            .get_hash_map(
                &self.positions,
                &entity_id,
                "catia_native_graph_record_lookup",
            )?
            .and_then(|index| records.get(*index)))
    }
}

/// Design objects of all graphs. Each graph's entities are the contiguous run
/// of entity records naming it, in record order.
#[cfg(test)]
fn design_objects(
    ctx: &DecodeContext<'_>,
    graphs: &[CatiaObjectGraph],
    entity_records: &[CatiaEntityRecord],
) -> Result<Vec<CatiaDesignObject>, CodecError> {
    let mut objects = Vec::new();
    for graph in graphs {
        let start = entity_records
            .iter()
            .position(|entity| entity.object_graph == graph.id)
            .unwrap_or(entity_records.len());
        let count = entity_records[start..]
            .iter()
            .take_while(|entity| entity.object_graph == graph.id)
            .count();
        let mut storage = ctx.reserve_scoped(0, "catia_design_test_index")?;
        let index = GraphRecordIndex::new(ctx, &mut storage, &graph.records)?;
        projection::graph_design_objects(
            ctx,
            graph,
            &entity_records[start..start + count],
            &index,
            &mut objects,
        )?;
    }
    Ok(objects)
}

fn design_object_id(
    ctx: &DecodeContext<'_>,
    graph_offset: u64,
    owner_entity_id: u32,
) -> Result<String, CodecError> {
    ctx.format_retained(
        format_args!("catia:outer:design-object#{graph_offset:010}-{owner_entity_id:010}"),
        "catia_design_object_id",
    )
}

fn visit_payload_references(
    ctx: &DecodeContext<'_>,
    payload: &ObjectPayload,
    mut visit: impl FnMut(u32, usize, CatiaObjectRecordReferenceSource) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    for field in ctx.admit_iter(&payload.fields, "catia_native_payload_reference_fields")? {
        match field {
            PayloadField::Reference { value, offset } => {
                visit(*value, *offset, CatiaObjectRecordReferenceSource::Field)?;
            }
            PayloadField::List {
                declared_count,
                items,
                offset,
            } if usize::try_from(*declared_count).ok() == Some(items.len()) => {
                for (item_ordinal, item) in ctx
                    .admit_iter(items, "catia_native_payload_reference_items")?
                    .enumerate()
                {
                    if let ListItem::Reference {
                        value,
                        offset: item_offset,
                    } = item
                    {
                        visit(
                            *value,
                            *item_offset,
                            CatiaObjectRecordReferenceSource::ListItem {
                                list_payload_offset: u64_from_index(*offset),
                                item_ordinal: u64_from_index(item_ordinal),
                            },
                        )?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn resolved_payload_references(
    ctx: &DecodeContext<'_>,
    payload: &ObjectPayload,
    records: &[CatiaObjectRecord],
    record_index: &GraphRecordIndex,
) -> Result<Vec<CatiaObjectRecordReference>, CodecError> {
    let mut references = Vec::new();
    visit_payload_references(ctx, payload, |entity_id, payload_offset, source| {
        let target = record_index.record(ctx, records, entity_id)?;
        let record = CatiaObjectRecordReference::from_parts(
            entity_id,
            u64_from_index(payload_offset),
            source,
            Some(entity_id) == record_index.terminal_null_entity_id,
            target
                .map(|record| ctx.copy_retained_text(&record.id, "catia_native_reference_target"))
                .transpose()?,
            target
                .and_then(|record| record.design_object.as_deref())
                .map(|id| ctx.copy_retained_text(id, "catia_native_reference_design_object"))
                .transpose()?,
        );
        ctx.push_vec(&mut references, record, "catia_native_payload_references")
    })?;
    Ok(references)
}

/// Returns the id after a graph's largest record entity id: the graph's
/// terminal null.
fn terminal_null_entity_id(
    ctx: &DecodeContext<'_>,
    records: &[CatiaObjectRecord],
) -> Result<Option<u32>, CodecError> {
    Ok(ctx
        .admit_iter(records, "catia_native_terminal_null_entity_id_scan")?
        .filter_map(CatiaObjectRecord::entity_id)
        .max()
        .and_then(|entity_id| entity_id.checked_add(1)))
}

fn resolved_storage_link(
    ctx: &DecodeContext<'_>,
    storage_ref: Option<u32>,
    records: &[CatiaObjectRecord],
    record_index: &GraphRecordIndex,
) -> Result<(Option<String>, Option<String>), CodecError> {
    let Some(storage_ref) = storage_ref else {
        return Ok((None, None));
    };
    let target = record_index.record(ctx, records, storage_ref)?;
    Ok((
        target
            .map(|record| ctx.copy_retained_text(&record.id, "catia_native_storage_record"))
            .transpose()?,
        target
            .and_then(|record| record.design_object.as_deref())
            .map(|id| ctx.copy_retained_text(id, "catia_native_storage_design_object"))
            .transpose()?,
    ))
}

fn definition_schema_selections(
    ctx: &DecodeContext<'_>,
    selectors: &[entity_table::DefinitionSchemaSelector],
    catalog: Option<&CatiaCatalog>,
) -> Result<Vec<CatiaDefinitionSchemaSelection>, CodecError> {
    let mut selections = Vec::new();
    for selector in ctx.admit_iter(selectors, "catia_native_definition_selector_visits")? {
        let catalog_entry = usize::try_from(selector.value)
            .ok()
            .and_then(|ordinal| catalog?.entries.get(ordinal));
        let selection = CatiaDefinitionSchemaSelection {
            offset: u64_from_index(selector.offset),
            ordinal: selector.value,
            entry: catalog_entry
                .map(|entry| ctx.copy_retained_text(&entry.id, "catia_definition_selection_entry"))
                .transpose()?,
            name: catalog_entry
                .map(|entry| {
                    ctx.copy_retained_text(&entry.value, "catia_definition_selection_name")
                })
                .transpose()?,
        };
        ctx.push_vec(
            &mut selections,
            selection,
            "catia_definition_schema_selections",
        )?;
    }
    Ok(selections)
}

fn entity_value_schema_selections(
    ctx: &DecodeContext<'_>,
    fields: &[value_block::ValueField],
    catalog: Option<&CatiaCatalog>,
    packets: &[entity_table::EntityValuePacket],
) -> Result<Vec<CatiaEntityValueSchemaSelection>, CodecError> {
    let Some(catalog) = catalog else {
        return Ok(Vec::new());
    };
    let mut selector_storage = ctx.reserve_scoped(0, "catia_native_value_selector_indices")?;
    let selector_indices = selector_storage.with_storage(|| {
        ctx.collect_vec(
            ctx.admit_iter(fields, "catia_native_value_selector_source_visits")?
                .enumerate()
                .filter_map(|(index, field)| {
                    let value_block::ValueField::SchemaSelector { ordinal, .. } = field else {
                        return None;
                    };
                    usize::try_from(*ordinal)
                        .ok()
                        .filter(|ordinal| *ordinal < catalog.entries.len())
                        .map(|_| index)
                }),
            "catia_native_value_selector_indices",
        )
    })?;
    let mut selections = Vec::new();
    for (rank, index) in ctx
        .admit_iter(
            &selector_indices,
            "catia_native_value_selector_index_visits",
        )?
        .enumerate()
    {
        let value_block::ValueField::SchemaSelector { ordinal, offset } = &fields[*index] else {
            continue;
        };
        let Some(catalog_entry) = usize::try_from(*ordinal)
            .ok()
            .and_then(|ordinal| catalog.entries.get(ordinal))
        else {
            continue;
        };
        let value_end = selector_indices
            .get(rank + 1)
            .copied()
            .unwrap_or(fields.len());
        let value_start_offset = fields.get(index + 1).map(value_field_offset);
        let value_end_offset = fields.get(value_end).map(value_field_offset);
        let mut selected_packets = Vec::new();
        if let Some(start) = value_start_offset {
            const SEARCH: &str = "catia_native_selected_packet_search";
            // Packets are in offset order and each ends after its offset, so a
            // packet inside the value starts in `start..end`.
            let first =
                ctx.partition_point(packets, |packet| Ok(packet_offset(packet) < start), SEARCH)?;
            let last = match value_end_offset {
                Some(end) => {
                    ctx.partition_point(packets, |packet| Ok(packet_offset(packet) < end), SEARCH)?
                }
                None => packets.len(),
            };
            for packet in ctx.admit_iter(
                packets.get(first..last).unwrap_or_default(),
                "catia_native_selected_packet_visits",
            )? {
                if packet.byte_range().is_some_and(|range| {
                    range.start >= start && value_end_offset.is_none_or(|end| range.end <= end)
                }) {
                    ctx.push_vec(
                        &mut selected_packets,
                        packet.copy_charged(ctx)?,
                        "catia_native_selected_packets",
                    )?;
                }
            }
        }
        let selection = CatiaEntityValueSchemaSelection {
            offset: u64_from_index(*offset),
            ordinal: *ordinal,
            entry: ctx
                .copy_retained_text(&catalog_entry.id, "catia_native_value_selection_entry")?,
            name: ctx
                .copy_retained_text(&catalog_entry.value, "catia_native_value_selection_name")?,
            encoded_value: value_block::copy_fields_charged(ctx, &fields[index + 1..value_end])?,
            packets: selected_packets,
        };
        ctx.push_vec(
            &mut selections,
            selection,
            "catia_native_value_schema_selections",
        )?;
    }
    Ok(selections)
}

fn packet_offset(packet: &entity_table::EntityValuePacket) -> usize {
    match packet {
        entity_table::EntityValuePacket::Numeric { offset, .. }
        | entity_table::EntityValuePacket::Compact { offset, .. }
        | entity_table::EntityValuePacket::Layout { offset, .. }
        | entity_table::EntityValuePacket::E9Scalar { offset, .. } => *offset,
    }
}

fn entity_suffix_schema_selection(
    ctx: &DecodeContext<'_>,
    suffix_value: Option<&CatiaEntitySuffixValue>,
    catalog: Option<&CatiaCatalog>,
) -> Result<Option<CatiaEntitySuffixSchemaSelection>, CodecError> {
    let Some(suffix_value) = suffix_value else {
        return Ok(None);
    };
    let CatiaEntitySuffixPayload::SchemaSelected {
        selector_offset,
        selector,
        value,
    } = &suffix_value.payload
    else {
        return Ok(None);
    };
    let Some(entry) = usize::try_from(*selector)
        .ok()
        .and_then(|ordinal| catalog.and_then(|catalog| catalog.entries.get(ordinal)))
    else {
        return Ok(None);
    };
    let value = match value {
        CatiaEntitySuffixSchemaValue::Atom { value } => {
            CatiaEntitySuffixSchemaValue::Atom { value: *value }
        }
        CatiaEntitySuffixSchemaValue::Evaluation {
            opcode_offset,
            evaluation,
        } => CatiaEntitySuffixSchemaValue::Evaluation {
            opcode_offset: *opcode_offset,
            evaluation: evaluation.clone(),
        },
        CatiaEntitySuffixSchemaValue::ControlE8 => CatiaEntitySuffixSchemaValue::ControlE8,
        CatiaEntitySuffixSchemaValue::Separator37 => CatiaEntitySuffixSchemaValue::Separator37,
        CatiaEntitySuffixSchemaValue::SchemaSelector {
            offset, ordinal, ..
        } => {
            let selected = usize::try_from(*ordinal)
                .ok()
                .and_then(|ordinal| catalog.and_then(|catalog| catalog.entries.get(ordinal)));
            CatiaEntitySuffixSchemaValue::SchemaSelector {
                offset: *offset,
                ordinal: *ordinal,
                resolution: selected
                    .map(|entry| -> Result<CatiaDesignClass, CodecError> {
                        Ok(CatiaDesignClass {
                            entry: ctx
                                .copy_retained_text(&entry.id, "catia_suffix_nested_entry")?,
                            name: ctx
                                .copy_retained_text(&entry.value, "catia_suffix_nested_name")?,
                        })
                    })
                    .transpose()?,
            }
        }
    };
    Ok(Some(CatiaEntitySuffixSchemaSelection {
        offset: *selector_offset,
        ordinal: *selector,
        entry: ctx.copy_retained_text(&entry.id, "catia_suffix_selection_entry")?,
        name: ctx.copy_retained_text(&entry.value, "catia_suffix_selection_name")?,
        value,
    }))
}

fn copy_value_schema(
    ctx: &DecodeContext<'_>,
    selection: &CatiaEntityValueSchemaSelection,
) -> Result<CatiaEntitySchemaValue, CodecError> {
    Ok(CatiaEntitySchemaValue {
        offset: selection.offset,
        ordinal: selection.ordinal,
        entry: ctx.copy_retained_text(&selection.entry, "catia_native_value_schema_entry")?,
        value: ctx.copy_retained_text(&selection.name, "catia_native_value_schema_name")?,
    })
}

fn copy_definition_schema(
    ctx: &DecodeContext<'_>,
    selection: &CatiaDefinitionSchemaSelection,
) -> Result<Option<CatiaEntitySchemaValue>, CodecError> {
    let (Some(entry), Some(value)) = (selection.entry.as_ref(), selection.name.as_ref()) else {
        return Ok(None);
    };
    Ok(Some(CatiaEntitySchemaValue {
        offset: selection.offset,
        ordinal: selection.ordinal,
        entry: ctx.copy_retained_text(entry, "catia_native_definition_schema_entry")?,
        value: ctx.copy_retained_text(value, "catia_native_definition_schema_name")?,
    }))
}

fn value_production(
    ctx: &DecodeContext<'_>,
    entity: &CatiaEntityRecord,
    incidences: &mut GraphIncidences<'_, '_>,
    value_fields: &[value_block::ValueField],
) -> Result<Option<CatiaEntityValueProduction>, CodecError> {
    if let Some(relation) = relation_expression(
        ctx,
        &entity.definition_schema_selections,
        &entity.value_schema_selections,
    )? {
        return Ok(Some(CatiaEntityValueProduction::RelationExpression(
            relation,
        )));
    }
    if let Some(parameter) = parameter_value(
        ctx,
        entity.lead,
        &entity.value_schema_selections,
        entity.suffix_value(),
    )? {
        return Ok(Some(CatiaEntityValueProduction::ParameterValue(parameter)));
    }
    if let Some(range) = resolved_constraint_range(
        ctx,
        entity.lead,
        &entity.value_schema_selections,
        entity.suffix_value(),
        incidences,
        entity.entity_id,
    )? {
        return Ok(Some(CatiaEntityValueProduction::ConstraintRange(range)));
    }
    if let Some(definition) = definition_value(
        ctx,
        entity.lead,
        &entity.definition_schema_selections,
        value_fields,
        entity.suffix_value(),
        entity.suffix_schema_selection.as_ref(),
    )? {
        return Ok(Some(CatiaEntityValueProduction::DefinitionValue(
            definition,
        )));
    }
    Ok(definition_chain_value(
        ctx,
        entity.lead,
        &entity.definition_schema_selections,
        value_fields,
        entity.suffix_value(),
        entity.suffix_schema_selection.as_ref(),
    )?
    .map(CatiaEntityValueProduction::DefinitionChainValue))
}

/// Compares optional text after admitting equal-length bytes.
fn equal_optional_text(
    ctx: &DecodeContext<'_>,
    left: Option<&str>,
    right: Option<&str>,
    operation: &'static str,
) -> Result<bool, CodecError> {
    match (left, right) {
        (Some(left), Some(right)) => ctx.equal_bytes(left.as_bytes(), right.as_bytes(), operation),
        (None, None) => Ok(true),
        (Some(_), None) | (None, Some(_)) => Ok(false),
    }
}

fn relation_expression(
    ctx: &DecodeContext<'_>,
    definitions: &[CatiaDefinitionSchemaSelection],
    values: &[CatiaEntityValueSchemaSelection],
) -> Result<Option<CatiaRelationExpression>, CodecError> {
    let [definition0, definition1] = definitions else {
        return Ok(None);
    };
    if definition0.name.as_deref() != Some("body")
        || definition1.name.as_deref() != Some("body")
        || !equal_optional_text(
            ctx,
            definition0.entry.as_deref(),
            definition1.entry.as_deref(),
            "catia_native_relation_definition_entry",
        )?
    {
        return Ok(None);
    }
    let (framing, expression, parameter_role, type_signature, function_role) = match values {
        [prefix_role, expression, parser_version_role, parameter_role, type_signature, state_role, function_role]
            if prefix_role.name == "Boolean"
                && parser_version_role.name == "ParserVersion"
                && parameter_role.name == "param"
                && state_role.name == "opened"
                && function_role.name == "RelationExpFct" =>
        {
            (
                CatiaRelationExpressionFraming::OpenedBooleanParserVersion {
                    prefix_role: copy_value_schema(ctx, prefix_role)?,
                    parser_version_role: copy_value_schema(ctx, parser_version_role)?,
                    state_role: copy_value_schema(ctx, state_role)?,
                },
                expression,
                parameter_role,
                type_signature,
                function_role,
            )
        }
        [placeholder, expression, parameter_role, type_signature, state_role, function_role]
            if parameter_role.name == "param"
                && state_role.name == "opened"
                && function_role.name == "RelationExpFct" =>
        {
            (
                CatiaRelationExpressionFraming::PlaceholderState {
                    placeholder: copy_value_schema(ctx, placeholder)?,
                    state_role: copy_value_schema(ctx, state_role)?,
                },
                expression,
                parameter_role,
                type_signature,
                function_role,
            )
        }
        [prefix_role, expression, parser_version_role, parameter_role, type_signature, function_role]
            if prefix_role.name == "Boolean"
                && parser_version_role.name == "ParserVersion"
                && parameter_role.name == "param"
                && function_role.name == "RelationExpFct" =>
        {
            (
                CatiaRelationExpressionFraming::BooleanParserVersion {
                    prefix_role: copy_value_schema(ctx, prefix_role)?,
                    parser_version_role: copy_value_schema(ctx, parser_version_role)?,
                },
                expression,
                parameter_role,
                type_signature,
                function_role,
            )
        }
        [expression, parser_version_role, parameter_role, type_signature, function_role]
            if parser_version_role.name == "ParserVersion"
                && parameter_role.name == "param"
                && function_role.name == "RelationExpFct" =>
        {
            (
                CatiaRelationExpressionFraming::ParserVersion {
                    parser_version_role: copy_value_schema(ctx, parser_version_role)?,
                },
                expression,
                parameter_role,
                type_signature,
                function_role,
            )
        }
        _ => return Ok(None),
    };
    Ok(Some(CatiaRelationExpression {
        framing,
        expression: copy_value_schema(ctx, expression)?,
        parameter_role: copy_value_schema(ctx, parameter_role)?,
        type_signature: copy_value_schema(ctx, type_signature)?,
        function_role: copy_value_schema(ctx, function_role)?,
    }))
}

fn relation_type_signature(
    placeholder: Option<&str>,
    source: &str,
) -> Option<CatiaRelationTypeSignature> {
    let source = source.strip_suffix('\n').unwrap_or(source);
    let (input_clause, result_type) = source.rsplit_once(") : ")?;
    let input_clause = input_clause.strip_prefix('(')?;
    if result_type.is_empty() || result_type.trim() != result_type {
        return None;
    }
    let inputs = if input_clause.is_empty() {
        Vec::new()
    } else {
        input_clause
            .split(',')
            .map(|clause| {
                let (parameter, input_type) = clause.split_once(':')?;
                let parameter = parameter.trim();
                let input_type = input_type.trim().strip_prefix("#In")?.trim();
                (relation_parameter_symbol(parameter) && !input_type.is_empty()).then(|| {
                    CatiaRelationTypeInput {
                        parameter: parameter.to_string(),
                        input_type: input_type.to_string(),
                    }
                })
            })
            .collect::<Option<Vec<_>>>()?
    };
    if placeholder.is_some_and(|placeholder| {
        inputs.is_empty() && !placeholder.trim().is_empty()
            || inputs
                .first()
                .is_some_and(|input| input.parameter != placeholder.trim())
    }) || inputs
        .iter()
        .map(|input| input.parameter.as_str())
        .collect::<HashSet<_>>()
        .len()
        != inputs.len()
    {
        return None;
    }
    Some(CatiaRelationTypeSignature {
        inputs,
        result_type: result_type.to_string(),
    })
}

fn relation_type_signature_charged(
    ctx: &DecodeContext<'_>,
    placeholder: Option<&str>,
    source: &str,
) -> Result<Option<CatiaRelationTypeSignature>, CodecError> {
    let (signature, storage) = ctx.with_scoped_storage(
        "catia_native_signature_scratch",
        || -> Result<_, CodecError> {
            const OPERATION: &str = "catia_native_signature_scan";
            let source = source.strip_suffix('\n').unwrap_or(source);
            let Some((input_clause, result_type)) = ctx.rsplit_once(source, ") : ", OPERATION)?
            else {
                return Ok(None);
            };
            let Some(input_clause) = input_clause.strip_prefix('(') else {
                return Ok(None);
            };
            if result_type.is_empty()
                || ctx.trim_text(result_type, OPERATION)?.len() != result_type.len()
            {
                return Ok(None);
            }
            let mut inputs = Vec::new();
            let parse_clause =
                |clause: &str| -> Result<Option<CatiaRelationTypeInput>, CodecError> {
                    let Some((parameter, input_type)) = ctx.split_once(clause, ":", OPERATION)?
                    else {
                        return Ok(None);
                    };
                    let parameter = ctx.trim_text(parameter, OPERATION)?;
                    let Some(input_type) =
                        ctx.trim_text(input_type, OPERATION)?.strip_prefix("#In")
                    else {
                        return Ok(None);
                    };
                    let input_type = ctx.trim_text(input_type, OPERATION)?;
                    let Some(digits) = parameter
                        .strip_prefix('#')
                        .and_then(|parameter| parameter.strip_suffix('_'))
                    else {
                        return Ok(None);
                    };
                    if digits.is_empty()
                        || !ctx.all_by(
                            digits.as_bytes(),
                            |byte| Ok(byte.is_ascii_digit()),
                            "catia_native_signature_parameter_digits",
                        )?
                        || input_type.is_empty()
                    {
                        return Ok(None);
                    }
                    Ok(Some(CatiaRelationTypeInput {
                        parameter: ctx
                            .copy_retained_text(parameter, "catia_native_signature_parameter")?,
                        input_type: ctx
                            .copy_retained_text(input_type, "catia_native_signature_input_type")?,
                    }))
                };
            if !input_clause.is_empty() {
                let mut clause_start = 0;
                let mut clause_bytes = input_clause.as_bytes().iter().enumerate();
                while let Some((offset, byte)) =
                    ctx.next_charged(&mut clause_bytes, "catia_native_signature_clause_visits")?
                {
                    if *byte == b',' {
                        let Some(input) = parse_clause(&input_clause[clause_start..offset])? else {
                            return Ok(None);
                        };
                        ctx.push_vec(&mut inputs, input, "catia_native_signature_inputs")?;
                        clause_start = offset + 1;
                    }
                }
                let Some(input) = parse_clause(&input_clause[clause_start..])? else {
                    return Ok(None);
                };
                ctx.push_vec(&mut inputs, input, "catia_native_signature_inputs")?;
            }
            if let Some(placeholder) = placeholder {
                let placeholder = ctx.trim_text(placeholder, OPERATION)?;
                let mismatched = match inputs.first() {
                    None => !placeholder.is_empty(),
                    Some(input) => !ctx.equal_bytes(
                        input.parameter.as_bytes(),
                        placeholder.as_bytes(),
                        OPERATION,
                    )?,
                };
                if mismatched {
                    return Ok(None);
                }
            }
            let mut parameter_storage =
                ctx.reserve_scoped(0, "catia_native_signature_prior_input_checks")?;
            let mut parameters = std::collections::BTreeSet::new();
            let mut input_rows = inputs.iter();
            while let Some(input) = ctx.next_charged(
                &mut input_rows,
                "catia_native_signature_duplicate_input_rows",
            )? {
                if !parameter_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut parameters,
                        input.parameter.as_str(),
                        "catia_native_signature_prior_input_checks",
                    )
                })? {
                    return Ok(None);
                }
            }
            Ok(Some(CatiaRelationTypeSignature {
                inputs,
                result_type: ctx
                    .copy_retained_text(result_type, "catia_native_signature_result_type")?,
            }))
        },
    )?;
    if signature.is_some() {
        storage.commit()?;
    }
    Ok(signature)
}

fn relation_parameter_symbol(parameter: &str) -> bool {
    parameter
        .strip_prefix('#')
        .and_then(|parameter| parameter.strip_suffix('_'))
        .is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn parameter_value(
    ctx: &DecodeContext<'_>,
    lead: u8,
    values: &[CatiaEntityValueSchemaSelection],
    suffix_value: Option<&CatiaEntitySuffixValue>,
) -> Result<Option<CatiaParameterValue>, CodecError> {
    if lead != 2 {
        return Ok(None);
    }
    let [name, binding] = values else {
        return Ok(None);
    };
    let Some(suffix_value) = suffix_value else {
        return Ok(None);
    };
    if !(suffix_value.prefix_atoms == [5, 22, 2]
        && suffix_value.prefix_atom_widths == [1, 1, 1]
        && suffix_value.prefix_code == 0x6a
        && suffix_value.trailer == CatiaEntitySuffixTrailer::Token8152)
    {
        return Ok(None);
    }
    let CatiaEntitySuffixPayload::Evaluation {
        opcode_offset,
        evaluation,
        encoding: CatiaEntityEvaluationEncoding::Direct,
    } = &suffix_value.payload
    else {
        return Ok(None);
    };
    Ok(Some(CatiaParameterValue {
        name: copy_value_schema(ctx, name)?,
        binding: copy_value_schema(ctx, binding)?,
        evaluation: evaluation.clone(),
        evaluation_opcode_offset: *opcode_offset,
    }))
}

fn constraint_range(
    ctx: &DecodeContext<'_>,
    lead: u8,
    values: &[CatiaEntityValueSchemaSelection],
    suffix_value: Option<&CatiaEntitySuffixValue>,
) -> Result<Option<CatiaConstraintRange>, CodecError> {
    if lead != 2 {
        return Ok(None);
    }
    let [range, constraint] = values else {
        return Ok(None);
    };
    if range.name != "Range" {
        return Ok(None);
    }
    let Some(suffix_value) = suffix_value else {
        return Ok(None);
    };
    if suffix_value.prefix_atoms != [4, 22, 2] || suffix_value.prefix_atom_widths != [1, 1, 1] {
        return Ok(None);
    }
    let framing = match (
        constraint.name.as_str(),
        suffix_value.prefix_code,
        suffix_value.trailer,
    ) {
        ("CstAttr_Dimension", 0xb8, CatiaEntitySuffixTrailer::Empty) => {
            CatiaConstraintRangeFraming::DimensionB8
        }
        ("CstAttr_Dimension", 0xc1, CatiaEntitySuffixTrailer::Empty) => {
            CatiaConstraintRangeFraming::DimensionC1
        }
        ("CstAttr_Dimension", 0xdc, CatiaEntitySuffixTrailer::Token81DB) => {
            CatiaConstraintRangeFraming::DimensionDC
        }
        ("CstAttr_Dimension", 0xdf, CatiaEntitySuffixTrailer::Token8192) => {
            CatiaConstraintRangeFraming::DimensionDF
        }
        ("ComplexCst", 0xc9, CatiaEntitySuffixTrailer::Empty) => {
            CatiaConstraintRangeFraming::ComplexC9
        }
        _ => return Ok(None),
    };
    let CatiaEntitySuffixPayload::Evaluation {
        opcode_offset,
        evaluation,
        encoding: CatiaEntityEvaluationEncoding::Direct,
    } = &suffix_value.payload
    else {
        return Ok(None);
    };
    Ok(Some(CatiaConstraintRange {
        range: copy_value_schema(ctx, range)?,
        constraint: copy_value_schema(ctx, constraint)?,
        framing,
        evaluation: evaluation.clone(),
        evaluation_opcode_offset: *opcode_offset,
        incoming_references: Vec::new(),
        incoming_storage_references: Vec::new(),
    }))
}

fn incidence_source_entity(
    ctx: &DecodeContext<'_>,
    record: &CatiaObjectRecord,
) -> Result<Option<CatiaEntityReference>, CodecError> {
    let Some(entity_id) = record.entity_id() else {
        return Ok(None);
    };
    let entity = record
        .entity_record()
        .map(|entity| ctx.copy_retained_text(entity, "catia_native_incidence_source_entity"))
        .transpose()?;
    let class_name = record
        .class_name()
        .map(|class_name| ctx.copy_retained_text(class_name, "catia_native_incidence_source_class"))
        .transpose()?;
    Ok(Some(CatiaEntityReference::resolved_or_unresolved(
        entity_id, entity, class_name,
    )))
}

/// One incoming reference of a graph record: a payload reference or the
/// record's storage reference.
enum GraphIncidence<'g> {
    Payload(&'g CatiaObjectRecord, &'g CatiaObjectRecordReference),
    Storage(&'g CatiaObjectRecord),
}

/// Incoming references of one graph's records, grouped by target entity id in
/// record order. The groups are built on the first lookup.
pub(crate) struct GraphIncidences<'g, 'ctx> {
    records: &'g [CatiaObjectRecord],
    groups: Option<HashMap<u32, Vec<GraphIncidence<'g>>>>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'g, 'ctx> GraphIncidences<'g, 'ctx> {
    pub(crate) fn new(
        ctx: &'ctx DecodeContext<'_>,
        records: &'g [CatiaObjectRecord],
    ) -> Result<Self, CodecError> {
        Ok(Self {
            records,
            groups: None,
            storage: ctx.reserve_scoped(0, "catia_native_incidence_index")?,
        })
    }

    /// The payload and storage references that name an entity, in record order.
    fn of(
        &mut self,
        ctx: &DecodeContext<'_>,
        entity_id: u32,
    ) -> Result<
        (
            Vec<CatiaEntityIncomingReference>,
            Vec<CatiaEntityIncomingStorageReference>,
        ),
        CodecError,
    > {
        const INDEX: &str = "catia_native_incidence_index";
        if self.groups.is_none() {
            let mut groups = HashMap::new();
            let records = self.records;
            self.storage.with_storage(|| -> Result<(), CodecError> {
                for record in ctx.admit_iter(records, "catia_native_incidence_scan")? {
                    for reference in
                        ctx.admit_iter(&record.references, "catia_native_incidence_references")?
                    {
                        ctx.push_hash_group(
                            &mut groups,
                            reference.entity_id(),
                            GraphIncidence::Payload(record, reference),
                            INDEX,
                            INDEX,
                        )?;
                    }
                    if let Some(storage_ref) = record.storage_ref() {
                        ctx.push_hash_group(
                            &mut groups,
                            storage_ref,
                            GraphIncidence::Storage(record),
                            INDEX,
                            INDEX,
                        )?;
                    }
                }
                Ok(())
            })?;
            self.groups = Some(groups);
        }
        let mut incoming_references = Vec::new();
        let mut incoming_storage_references = Vec::new();
        let Some(incidences) = self
            .groups
            .as_ref()
            .map(|groups| ctx.get_hash_map(groups, &entity_id, INDEX))
            .transpose()?
            .flatten()
        else {
            return Ok((incoming_references, incoming_storage_references));
        };
        for incidence in ctx.admit_iter(incidences, "catia_native_incidence_visits")? {
            match incidence {
                GraphIncidence::Payload(record, reference) => {
                    let incidence = CatiaEntityIncomingReference {
                        object_record: ctx
                            .copy_retained_text(&record.id, "catia_native_incidence_record")?,
                        source_entity: incidence_source_entity(ctx, record)?,
                        payload_offset: reference.payload_offset(),
                        source: reference.source().clone(),
                    };
                    ctx.push_vec(
                        &mut incoming_references,
                        incidence,
                        "catia_native_incoming_references",
                    )?;
                }
                GraphIncidence::Storage(record) => {
                    let incidence = CatiaEntityIncomingStorageReference {
                        object_record: ctx
                            .copy_retained_text(&record.id, "catia_native_storage_record")?,
                        source_entity: incidence_source_entity(ctx, record)?,
                    };
                    ctx.push_vec(
                        &mut incoming_storage_references,
                        incidence,
                        "catia_native_incoming_storage",
                    )?;
                }
            }
        }
        Ok((incoming_references, incoming_storage_references))
    }
}

fn range_interval(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    values: &[CatiaEntityValueSchemaSelection],
    suffix_value: Option<&CatiaEntitySuffixValue>,
    incidences: &mut GraphIncidences<'_, '_>,
    entity_id: u32,
) -> Result<Option<CatiaRangeInterval>, CodecError> {
    const OPERATION: &str = "catia_native_range_selection_visits";
    let is_range = |selection: &CatiaEntityValueSchemaSelection| Ok(selection.name == "Range");
    let Some(index) = ctx.position_by(values, is_range, OPERATION)? else {
        return Ok(None);
    };
    if ctx.any_by(&values[index + 1..], is_range, OPERATION)? {
        return Ok(None);
    }
    let range = &values[index];
    let Some(start) = usize::try_from(range.offset)
        .ok()
        .and_then(|offset| offset.checked_add(5))
    else {
        return Ok(None);
    };
    let end = match values.get(index + 1) {
        Some(selection) => {
            let Ok(end) = usize::try_from(selection.offset) else {
                return Ok(None);
            };
            end
        }
        None => payload.len(),
    };
    let Some(interval) = entity_table::parse_range_interval(ctx, payload, start, end)? else {
        return Ok(None);
    };
    let (incoming_references, incoming_storage_references) = incidences.of(ctx, entity_id)?;
    Ok(Some(CatiaRangeInterval {
        range: copy_value_schema(ctx, range)?,
        interval,
        nominal: range_nominal(suffix_value),
        incoming_references,
        incoming_storage_references,
    }))
}

fn range_nominal(suffix_value: Option<&CatiaEntitySuffixValue>) -> Option<CatiaRangeNominal> {
    let suffix = suffix_value?;
    if suffix.prefix_atoms != [4, 22, 2] || suffix.prefix_atom_widths != [1, 1, 1] {
        return None;
    }
    let framing = match (suffix.prefix_code, suffix.trailer) {
        (0xd8, CatiaEntitySuffixTrailer::Token8193) => CatiaRangeNominalFraming::D8Token8193,
        (0xd8, CatiaEntitySuffixTrailer::Token81DB) => CatiaRangeNominalFraming::D8Token81DB,
        (0xdc, CatiaEntitySuffixTrailer::Token81DB) => CatiaRangeNominalFraming::DCToken81DB,
        (0xdf, CatiaEntitySuffixTrailer::Token8192) => CatiaRangeNominalFraming::DFToken8192,
        _ => return None,
    };
    let CatiaEntitySuffixPayload::Evaluation {
        opcode_offset,
        evaluation: CatiaEntityEvaluation::Scalar { bits },
        encoding: CatiaEntityEvaluationEncoding::Direct,
    } = suffix.payload
    else {
        return None;
    };
    Some(CatiaRangeNominal {
        framing,
        bits,
        evaluation_opcode_offset: opcode_offset,
    })
}

fn resolved_constraint_range(
    ctx: &DecodeContext<'_>,
    lead: u8,
    values: &[CatiaEntityValueSchemaSelection],
    suffix_value: Option<&CatiaEntitySuffixValue>,
    incidences: &mut GraphIncidences<'_, '_>,
    entity_id: u32,
) -> Result<Option<CatiaConstraintRange>, CodecError> {
    let Some(mut range) = constraint_range(ctx, lead, values, suffix_value)? else {
        return Ok(None);
    };
    (range.incoming_references, range.incoming_storage_references) =
        incidences.of(ctx, entity_id)?;
    Ok(Some(range))
}

fn copy_suffix_schema_value(
    ctx: &DecodeContext<'_>,
    value: &CatiaEntitySuffixSchemaValue,
) -> Result<CatiaEntitySuffixSchemaValue, CodecError> {
    Ok(match value {
        CatiaEntitySuffixSchemaValue::Atom { value } => {
            CatiaEntitySuffixSchemaValue::Atom { value: *value }
        }
        CatiaEntitySuffixSchemaValue::Evaluation {
            opcode_offset,
            evaluation,
        } => CatiaEntitySuffixSchemaValue::Evaluation {
            opcode_offset: *opcode_offset,
            evaluation: evaluation.clone(),
        },
        CatiaEntitySuffixSchemaValue::ControlE8 => CatiaEntitySuffixSchemaValue::ControlE8,
        CatiaEntitySuffixSchemaValue::Separator37 => CatiaEntitySuffixSchemaValue::Separator37,
        CatiaEntitySuffixSchemaValue::SchemaSelector {
            offset,
            ordinal,
            resolution,
        } => CatiaEntitySuffixSchemaValue::SchemaSelector {
            offset: *offset,
            ordinal: *ordinal,
            resolution: resolution
                .as_ref()
                .map(|class| -> Result<CatiaDesignClass, CodecError> {
                    Ok(CatiaDesignClass {
                        entry: ctx
                            .copy_retained_text(&class.entry, "catia_native_suffix_class_entry")?,
                        name: ctx
                            .copy_retained_text(&class.name, "catia_native_suffix_class_name")?,
                    })
                })
                .transpose()?,
        },
    })
}

fn copy_suffix_payload(
    ctx: &DecodeContext<'_>,
    payload: &CatiaEntitySuffixPayload,
) -> Result<CatiaEntitySuffixPayload, CodecError> {
    Ok(match payload {
        CatiaEntitySuffixPayload::Evaluation {
            opcode_offset,
            evaluation,
            encoding,
        } => CatiaEntitySuffixPayload::Evaluation {
            opcode_offset: *opcode_offset,
            evaluation: evaluation.clone(),
            encoding: *encoding,
        },
        CatiaEntitySuffixPayload::Atom { value } => {
            CatiaEntitySuffixPayload::Atom { value: *value }
        }
        CatiaEntitySuffixPayload::SchemaSelected {
            selector_offset,
            selector,
            value,
        } => CatiaEntitySuffixPayload::SchemaSelected {
            selector_offset: *selector_offset,
            selector: *selector,
            value: copy_suffix_schema_value(ctx, value)?,
        },
        CatiaEntitySuffixPayload::ControlE8 => CatiaEntitySuffixPayload::ControlE8,
        CatiaEntitySuffixPayload::ControlE9 => CatiaEntitySuffixPayload::ControlE9,
        CatiaEntitySuffixPayload::Separator37 => CatiaEntitySuffixPayload::Separator37,
    })
}

fn definition_value(
    ctx: &DecodeContext<'_>,
    lead: u8,
    definitions: &[CatiaDefinitionSchemaSelection],
    value_fields: &[value_block::ValueField],
    suffix_value: Option<&CatiaEntitySuffixValue>,
    suffix_schema_selection: Option<&CatiaEntitySuffixSchemaSelection>,
) -> Result<Option<CatiaDefinitionValue>, CodecError> {
    if lead != 2
        || !matches!(
            value_fields,
            [value_block::ValueField::Terminator { offset: 0 }]
        )
    {
        return Ok(None);
    }
    let [definition] = definitions else {
        return Ok(None);
    };
    let Some(suffix_value) = suffix_value else {
        return Ok(None);
    };
    let Some(definition) = copy_definition_schema(ctx, definition)? else {
        return Ok(None);
    };
    let schema_selection = suffix_schema_selection
        .map(|selection| -> Result<_, CodecError> {
            Ok(CatiaEntitySuffixSchemaSelection {
                offset: selection.offset,
                ordinal: selection.ordinal,
                entry: ctx
                    .copy_retained_text(&selection.entry, "catia_native_definition_suffix_entry")?,
                name: ctx
                    .copy_retained_text(&selection.name, "catia_native_definition_suffix_name")?,
                value: copy_suffix_schema_value(ctx, &selection.value)?,
            })
        })
        .transpose()?;
    Ok(Some(CatiaDefinitionValue {
        definition,
        payload: copy_suffix_payload(ctx, &suffix_value.payload)?,
        schema_selection,
    }))
}

fn definition_chain_value(
    ctx: &DecodeContext<'_>,
    lead: u8,
    definitions: &[CatiaDefinitionSchemaSelection],
    value_fields: &[value_block::ValueField],
    suffix_value: Option<&CatiaEntitySuffixValue>,
    suffix_schema_selection: Option<&CatiaEntitySuffixSchemaSelection>,
) -> Result<Option<CatiaDefinitionChainValue>, CodecError> {
    const OPERATION: &str = "catia_native_definition_chain_selector";
    if lead != 2
        || !matches!(
            value_fields,
            [value_block::ValueField::Terminator { offset: 0 }]
        )
    {
        return Ok(None);
    }
    let [selector, role] = definitions else {
        return Ok(None);
    };
    let (Some(selector_entry), Some(selector_name)) = (&selector.entry, &selector.name) else {
        return Ok(None);
    };
    if role.entry.is_none() || role.name.is_none() {
        return Ok(None);
    }
    let Some(suffix_schema_selection) = suffix_schema_selection else {
        return Ok(None);
    };
    let Some(suffix_value) = suffix_value else {
        return Ok(None);
    };
    let CatiaEntitySuffixPayload::SchemaSelected { .. } = &suffix_value.payload else {
        return Ok(None);
    };
    if !ctx.equal_bytes(
        suffix_schema_selection.entry.as_bytes(),
        selector_entry.as_bytes(),
        OPERATION,
    )? || !ctx.equal_bytes(
        suffix_schema_selection.name.as_bytes(),
        selector_name.as_bytes(),
        OPERATION,
    )? {
        return Ok(None);
    }
    let Some(selector_value) = copy_definition_schema(ctx, selector)? else {
        return Ok(None);
    };
    let Some(role) = copy_definition_schema(ctx, role)? else {
        return Ok(None);
    };
    Ok(Some(CatiaDefinitionChainValue {
        selector: selector_value,
        role,
        value: copy_suffix_schema_value(ctx, &suffix_schema_selection.value)?,
    }))
}

/// Parses the fixed-width suffix value grammar; every read has constant extent.
fn entity_suffix_value(suffix: &[u8]) -> Option<CatiaEntitySuffixValue> {
    macro_rules! some_or_none {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return None,
            }
        };
    }
    let atom = |at: usize| {
        let lead = *suffix.get(at)?;
        match lead {
            0x80..=0xd0 => Some((u32::from(lead - 0x80), 1_u8)),
            0xd1..=0xe4 => Some((
                u32::from(lead - 0xd1) * 256 + u32::from(*suffix.get(at + 1)?) + 1,
                2,
            )),
            _ => None,
        }
    };
    let mut at = 0;
    let (prefix0, width0) = atom(at)?;
    at += usize::from(width0);
    let (prefix1, width1) = atom(at)?;
    at += usize::from(width1);
    let (prefix2, width2) = atom(at)?;
    at += usize::from(width2);
    let prefix_atoms = [prefix0, prefix1, prefix2];
    let prefix_atom_widths = [width0, width1, width2];
    let prefix_code = *some_or_none!(suffix.get(at));
    let payload_offset = at + 1;
    let (payload, trailer_offset) = if suffix.get(payload_offset..payload_offset + 5)
        == Some(&[0xe6, 0x00, 0x00, 0x00, 0xe6])
    {
        let bits = some_or_none!(View::u64_le_at(suffix, payload_offset + 5));
        some_or_none!(f64::from_bits(bits).is_finite().then_some(()));
        (
            CatiaEntitySuffixPayload::Evaluation {
                opcode_offset: u64_from_index(payload_offset + 4),
                evaluation: CatiaEntityEvaluation::Scalar { bits },
                encoding: CatiaEntityEvaluationEncoding::ZeroPaddedScalar,
            },
            payload_offset + 13,
        )
    } else if prefix_code == 0x32 {
        let selector = some_or_none!(View::u32_le_at(suffix, payload_offset));
        let value_offset = payload_offset + 4;
        let (value, trailer_offset) = match *some_or_none!(suffix.get(value_offset)) {
            0xe6 => {
                let bits = some_or_none!(View::u64_le_at(suffix, value_offset + 1));
                some_or_none!(f64::from_bits(bits).is_finite().then_some(()));
                (
                    CatiaEntitySuffixSelectedValue::Evaluation {
                        opcode_offset: u64_from_index(value_offset),
                        evaluation: CatiaEntityEvaluation::Scalar { bits },
                    },
                    value_offset + 9,
                )
            }
            0xe7 => (
                CatiaEntitySuffixSelectedValue::Evaluation {
                    opcode_offset: u64_from_index(value_offset),
                    evaluation: CatiaEntityEvaluation::Unset,
                },
                value_offset + 1,
            ),
            0xe8 => (CatiaEntitySuffixSelectedValue::ControlE8, value_offset + 1),
            0x37 => (
                CatiaEntitySuffixSelectedValue::Separator37,
                value_offset + 1,
            ),
            0x32 => (
                CatiaEntitySuffixSelectedValue::SchemaSelector {
                    offset: u64_from_index(value_offset),
                    ordinal: some_or_none!(View::u32_le_at(suffix, value_offset + 1)),
                    resolution: None,
                },
                value_offset + 5,
            ),
            atom @ 0x80..=0xd0 => (
                CatiaEntitySuffixSelectedValue::Atom {
                    value: u32::from(atom - 0x80),
                },
                value_offset + 1,
            ),
            _ => return None,
        };
        (
            CatiaEntitySuffixPayload::SchemaSelected {
                selector_offset: u64_from_index(at),
                selector,
                value,
            },
            trailer_offset,
        )
    } else {
        match *some_or_none!(suffix.get(payload_offset)) {
            0xe7 => (
                CatiaEntitySuffixPayload::Evaluation {
                    opcode_offset: u64_from_index(payload_offset),
                    evaluation: CatiaEntityEvaluation::Unset,
                    encoding: CatiaEntityEvaluationEncoding::Direct,
                },
                payload_offset + 1,
            ),
            0xe8 => (CatiaEntitySuffixPayload::ControlE8, payload_offset + 1),
            0xe9 => (CatiaEntitySuffixPayload::ControlE9, payload_offset + 1),
            0x37 => (CatiaEntitySuffixPayload::Separator37, payload_offset + 1),
            0xe6 => {
                let bits = some_or_none!(View::u64_le_at(suffix, payload_offset + 1));
                some_or_none!(f64::from_bits(bits).is_finite().then_some(()));
                (
                    CatiaEntitySuffixPayload::Evaluation {
                        opcode_offset: u64_from_index(payload_offset),
                        evaluation: CatiaEntityEvaluation::Scalar { bits },
                        encoding: CatiaEntityEvaluationEncoding::Direct,
                    },
                    payload_offset + 9,
                )
            }
            atom @ 0x80..=0xd0 => (
                CatiaEntitySuffixPayload::Atom {
                    value: u32::from(atom - 0x80),
                },
                payload_offset + 1,
            ),
            _ => return None,
        }
    };
    let trailer = match some_or_none!(suffix.get(trailer_offset..)) {
        [] => CatiaEntitySuffixTrailer::Empty,
        [0x81, 0x49] => CatiaEntitySuffixTrailer::Token8149,
        [0x81, 0x4a] => CatiaEntitySuffixTrailer::Token814A,
        [0x81, 0x52] => CatiaEntitySuffixTrailer::Token8152,
        [0x81, 0xdb] => CatiaEntitySuffixTrailer::Token81DB,
        [0x81, 0x92] => CatiaEntitySuffixTrailer::Token8192,
        [0x81, 0x93] => CatiaEntitySuffixTrailer::Token8193,
        [0xfe, 0xf6, rest @ ..] if rest.len() == 16 => {
            if rest.iter().any(|byte| *byte != 0) {
                return None;
            }
            CatiaEntitySuffixTrailer::FixedZeroFrame
        }
        _ => return None,
    };
    Some(CatiaEntitySuffixValue {
        prefix_atoms,
        prefix_atom_widths,
        prefix_code,
        payload,
        trailer,
    })
}

fn entity_suffix_framing(
    ctx: &DecodeContext<'_>,
    suffix: &[u8],
) -> Result<Option<CatiaEntitySuffixFraming>, CodecError> {
    let framing = match suffix {
        [0x80, _, _, _, _, state] => {
            let state = match state {
                0x00 => CatiaEntitySuffixEscapedWordState::State00,
                0x01 => CatiaEntitySuffixEscapedWordState::State01,
                0x03 => CatiaEntitySuffixEscapedWordState::State03,
                0x04 => CatiaEntitySuffixEscapedWordState::State04,
                0x09 => CatiaEntitySuffixEscapedWordState::State09,
                _ => return Ok(None),
            };
            CatiaEntitySuffixFraming::EscapedWord(CatiaEntitySuffixEscapedWord {
                word: match View::u32_le_at(suffix, 1) {
                    Some(word) => word,
                    None => return Ok(None),
                },
                state,
            })
        }
        [0x81, 0x49] => CatiaEntitySuffixFraming::Token8149,
        [0xfe, 0xf6, payload @ ..] if payload.len() == 16 => CatiaEntitySuffixFraming::FixedFeF6 {
            payload: ctx.copy_slice(payload, "catia_native_fixed_suffix_payload")?,
        },
        [lead @ 0xd1..=0xe4, low, 0x01] => CatiaEntitySuffixFraming::PagedAtomState01 {
            value: u32::from(*lead - 0xd1) * 256 + u32::from(*low) + 1,
        },
        _ => return Ok(None),
    };
    Ok(Some(framing))
}

fn object_production(
    ctx: &DecodeContext<'_>,
    entity: &CatiaEntityRecord,
    object: &CatiaObjectRecord,
    references: &CatiaEntityReferenceIndex<'_>,
    expressions: &CatiaRelationExpressionIndex<'_>,
    expression_entities: &CatiaRelationExpressionEntityIndex<'_>,
    parameters: &CatiaParameterBindingIndex<'_>,
) -> Result<Option<CatiaEntityObjectProduction>, CodecError> {
    if let Some(instance) = relation_program_instance(
        ctx,
        entity.entity_id,
        object,
        references,
        expression_entities,
        parameters,
    )? {
        return Ok(Some(CatiaEntityObjectProduction::RelationProgramInstance(
            instance,
        )));
    }
    if let Some(record) = schema_configuration_record(
        ctx,
        entity.entity_id,
        object,
        &entity.value_schema_selections,
        references,
    )? {
        return Ok(Some(
            CatiaEntityObjectProduction::SchemaConfigurationRecord(record),
        ));
    }
    if let Some(link) = schema_configuration_row_link(ctx, entity.entity_id, object, references)? {
        return Ok(Some(
            CatiaEntityObjectProduction::SchemaConfigurationRowLink(link),
        ));
    }
    Ok(formula_relation(
        ctx,
        &entity.definition_schema_selections,
        entity.entity_id,
        object,
        expressions,
        references,
        parameters,
    )?
    .map(CatiaEntityObjectProduction::FormulaRelation))
}

fn relation_program_instance(
    ctx: &DecodeContext<'_>,
    entity_id: u32,
    object: &CatiaObjectRecord,
    entity_references: &CatiaEntityReferenceIndex<'_>,
    relation_expressions: &CatiaRelationExpressionEntityIndex<'_>,
    parameter_bindings: &CatiaParameterBindingIndex<'_>,
) -> Result<Option<CatiaRelationProgramInstance>, CodecError> {
    if object.entity_id() != Some(entity_id)
        || object.owner_entity_id().is_none()
        || object.class_ref().is_none()
    {
        return Ok(None);
    }
    let (framing, program_entity_id, repeated_reference_entity_id) =
        if object.lead == 0x12 && object.storage_ref().is_none() {
            let Some((program_entity_id, repeated_reference_entity_id, context_entity_id)) =
                relation_program_instance_lead_12(entity_id, &object.payload.fields)
            else {
                return Ok(None);
            };
            (
                CatiaRelationProgramInstanceFraming::Lead12 {
                    context_entity: entity_reference(
                        ctx,
                        &object.parent,
                        context_entity_id,
                        entity_references,
                    )?,
                },
                program_entity_id,
                repeated_reference_entity_id,
            )
        } else if object.lead == 0x54 && object.storage_ref().is_some() {
            let Some((program_entity_id, repeated_reference_entity_id, trailing_entity_id)) =
                relation_program_instance_lead_54(entity_id, &object.payload.fields)
            else {
                return Ok(None);
            };
            (
                CatiaRelationProgramInstanceFraming::Lead54 {
                    trailing_entity: entity_reference(
                        ctx,
                        &object.parent,
                        trailing_entity_id,
                        entity_references,
                    )?,
                },
                program_entity_id,
                repeated_reference_entity_id,
            )
        } else {
            return Ok(None);
        };
    let selected_expression = graph_identity_row(
        ctx,
        relation_expressions,
        &object.parent,
        program_entity_id,
        "catia_native_program_lookup",
    )?;
    let parameter_dependencies = if let Some(expression) = selected_expression {
        relation_parameter_dependencies(ctx, expression.source, &object.parent, parameter_bindings)?
    } else {
        Vec::new()
    };
    let inputs = if let Some(signature) =
        selected_expression.and_then(|expression| expression.signature.as_ref())
    {
        resolved_relation_program_inputs(ctx, signature, &parameter_dependencies)?
    } else {
        None
    };
    let mut reference_incidences = Vec::new();
    for field in ctx.admit_iter(
        &object.payload.fields,
        "catia_native_program_reference_field_visits",
    )? {
        let PayloadField::Reference { value, offset } = field else {
            continue;
        };
        let reference = entity_reference(ctx, &object.parent, *value, entity_references)?;
        ctx.push_vec(
            &mut reference_incidences,
            CatiaPayloadEntityReference {
                payload_offset: u64_from_index(*offset),
                reference,
            },
            "catia_native_program_references",
        )?;
    }
    Ok(Some(CatiaRelationProgramInstance {
        framing,
        program_entity: entity_reference(
            ctx,
            &object.parent,
            program_entity_id,
            entity_references,
        )?,
        repeated_entity: entity_reference(
            ctx,
            &object.parent,
            repeated_reference_entity_id,
            entity_references,
        )?,
        reference_incidences,
        relation_expression: selected_expression
            .map(|expression| {
                ctx.copy_retained_text(expression.entity, "catia_native_program_expression")
            })
            .transpose()?,
        parameter_dependencies,
        inputs,
    }))
}

fn entity_reference(
    ctx: &DecodeContext<'_>,
    graph_id: &str,
    entity_id: u32,
    references: &CatiaEntityReferenceIndex<'_>,
) -> Result<CatiaEntityReference, CodecError> {
    let terminal_null = ctx
        .get_hash_map(
            references.maxima,
            graph_id,
            "catia_native_reference_terminal",
        )?
        .and_then(|maximum| maximum.checked_add(1));
    if terminal_null == Some(entity_id) {
        return Ok(CatiaEntityReference::Null { entity_id });
    }
    let entity = graph_identity_row(
        ctx,
        references.entities,
        graph_id,
        entity_id,
        "catia_native_reference_lookup",
    )?;
    Ok(match entity {
        Some(entity) => {
            let class_name = graph_identity_row(
                ctx,
                references.classes,
                graph_id,
                entity_id,
                "catia_native_reference_class_lookup",
            )?;
            CatiaEntityReference::Resolved {
                entity_id,
                entity: ctx.copy_retained_text(entity, "catia_native_reference_entity")?,
                class_name: class_name
                    .map(|class_name| {
                        ctx.copy_retained_text(class_name, "catia_native_reference_class")
                    })
                    .transpose()?,
            }
        }
        None => CatiaEntityReference::Unresolved { entity_id },
    })
}

#[cfg(test)]
fn reference_signature(
    ctx: &DecodeContext<'_>,
    production: entity_table::ReferenceSignature,
    graph_id: &str,
    entity_references: &CatiaEntityReferenceIndex<'_>,
) -> Result<CatiaReferenceSignature, CodecError> {
    let first_entity = entity_reference(
        ctx,
        graph_id,
        production.first_reference(),
        entity_references,
    )?;
    let second_entity = entity_reference(
        ctx,
        graph_id,
        production.second_reference(),
        entity_references,
    )?;
    Ok(CatiaReferenceSignature {
        production,
        first_entity,
        second_entity,
    })
}

fn derive_reference_signature_cohorts(
    ctx: &DecodeContext<'_>,
    entity_records: &[CatiaEntityRecord],
) -> Result<Vec<CatiaReferenceSignatureCohort>, CodecError> {
    // The pair index, the graph ordinals and the members' shared schema
    // selection are dropped once the cohorts are built.
    let mut scratch = ctx.reserve_scoped(0, "catia_native_cohort_scratch")?;
    let mut cohorts = Vec::<CatiaReferenceSignatureCohort>::new();
    // Whether every member so far has the `_SpecList` shape, and the selection
    // they share.
    let mut selections = Vec::<(bool, Option<&CatiaEntityValueSchemaSelection>)>::new();
    let mut cohort_by_pair = HashMap::<(&str, u32), usize>::new();
    let mut next_ordinal_by_graph = HashMap::<&str, u64>::new();
    for entity in ctx.admit_iter(
        entity_records,
        "catia_native_reference_cohort_entity_visits",
    )? {
        let Some(signature) = &entity.reference_signature else {
            continue;
        };
        let key = (
            entity.object_graph.as_str(),
            signature.production.first_reference(),
        );
        let index = if let Some(index) =
            ctx.get_hash_map(&cohort_by_pair, &key, "catia_native_cohort_pairs")?
        {
            let index = *index;
            let member = ctx.copy_retained_text(&entity.id, "catia_native_cohort_member")?;
            ctx.push_vec(
                &mut cohorts[index].members,
                member,
                "catia_native_cohort_members",
            )?;
            index
        } else {
            let ordinal = if let Some(next) = ctx.get_mut_hash_map(
                &mut next_ordinal_by_graph,
                entity.object_graph.as_str(),
                "catia_native_cohort_ordinal_lookup",
            )? {
                *next = next.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit("catia_native_cohort_ordinal", u64::MAX, u64::MAX)
                })?;
                *next
            } else {
                scratch.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut next_ordinal_by_graph,
                        entity.object_graph.as_str(),
                        0,
                        "catia_native_cohort_ordinals",
                    )
                })?;
                0
            };
            let Some((namespace, graph_key)) =
                ctx.split_once(&entity.object_graph, "#", "catia_native_cohort_graph_key")?
            else {
                continue;
            };
            let Some((format, rest)) =
                ctx.split_once(namespace, ":", "catia_native_cohort_graph_key")?
            else {
                continue;
            };
            let Some((scope, kind)) = ctx.split_once(rest, ":", "catia_native_cohort_graph_key")?
            else {
                continue;
            };
            if ctx.contains_text(kind, ":", "catia_native_cohort_graph_key")? {
                continue;
            }
            let id = ctx.format_retained(
                format_args!(
                    "{format}:{scope}:reference-signature-cohort#{graph_key}:{ordinal:08}"
                ),
                "catia_native_cohort_id",
            )?;
            let index = cohorts.len();
            let parent =
                ctx.copy_retained_text(&entity.object_graph, "catia_native_cohort_parent")?;
            let first_entity = signature.first_entity.copy_charged(ctx)?;
            let second_entity = signature.second_entity.copy_charged(ctx)?;
            let mut members = Vec::new();
            ctx.push_vec(
                &mut members,
                ctx.copy_retained_text(&entity.id, "catia_native_cohort_member")?,
                "catia_native_cohort_members",
            )?;
            ctx.push_vec(
                &mut cohorts,
                CatiaReferenceSignatureCohort {
                    id,
                    parent,
                    ordinal,
                    references: signature.production.references(),
                    first_entity,
                    second_entity,
                    schema_selection: None,
                    members,
                },
                "catia_native_cohorts",
            )?;
            scratch.with_storage(|| -> Result<(), CodecError> {
                ctx.push_vec(
                    &mut selections,
                    (true, None),
                    "catia_native_cohort_selections",
                )?;
                ctx.insert_hash_map(&mut cohort_by_pair, key, index, "catia_native_cohort_pairs")?;
                Ok(())
            })?;
            index
        };
        let (valid, selected) = &mut selections[index];
        if !*valid {
            continue;
        }
        let member_selections = &entity.value_schema_selections;
        if member_selections
            .first()
            .map(|selection| selection.name.as_str())
            != Some("_SpecList")
            || member_selections.len() > 2
        {
            *valid = false;
            continue;
        }
        let Some(selection) = member_selections.get(1) else {
            continue;
        };
        if let Some(previous) = selected {
            const COMPARE: &str = "catia_native_cohort_schema_comparison";
            if previous.ordinal != selection.ordinal
                || !ctx.equal_bytes(
                    previous.entry.as_bytes(),
                    selection.entry.as_bytes(),
                    COMPARE,
                )?
                || !ctx.equal_bytes(previous.name.as_bytes(), selection.name.as_bytes(), COMPARE)?
            {
                *valid = false;
                continue;
            }
        }
        *selected = Some(selection);
    }
    for (cohort, (valid, selected)) in ctx
        .admit_iter(&mut cohorts, "catia_native_cohort_member_visits")?
        .zip(selections)
    {
        cohort.schema_selection = match selected.filter(|_| valid) {
            Some(selection) => Some(CatiaReferenceSignatureSchemaSelection {
                ordinal: selection.ordinal,
                entry: ctx
                    .copy_retained_text(&selection.entry, "catia_native_cohort_schema_entry")?,
                name: ctx.copy_retained_text(&selection.name, "catia_native_cohort_schema_name")?,
            }),
            None => None,
        };
    }
    Ok(cohorts)
}

fn schema_configuration_record(
    ctx: &DecodeContext<'_>,
    entity_id: u32,
    object: &CatiaObjectRecord,
    value_schema_selections: &[CatiaEntityValueSchemaSelection],
    references: &CatiaEntityReferenceIndex<'_>,
) -> Result<Option<CatiaSchemaConfigurationRecord>, CodecError> {
    const OPERATION: &str = "catia_native_configuration_selection_visits";
    if object.entity_id() != Some(entity_id)
        || object.lead != 0x12
        || object.owner_entity_id().is_none()
        || object.class_ref() != Some(entity_id)
        || object.class_name() != Some("Configuration")
        || object.storage_ref().is_some()
    {
        return Ok(None);
    }
    let [PayloadField::Reference {
        value: schema_ordinal,
        offset: schema_offset,
    }, PayloadField::Atom { value: 2, .. }, PayloadField::Reference {
        value: referenced_entity_id,
        offset: entity_offset,
    }, PayloadField::Atom { value: 129, .. }, PayloadField::Terminator] =
        object.payload.fields.as_slice()
    else {
        return Ok(None);
    };
    let matches =
        |selection: &CatiaEntityValueSchemaSelection| Ok(selection.ordinal == *schema_ordinal);
    let Some(index) = ctx.position_by(value_schema_selections, matches, OPERATION)? else {
        return Ok(None);
    };
    if ctx.any_by(&value_schema_selections[index + 1..], matches, OPERATION)? {
        return Ok(None);
    }
    let selection = &value_schema_selections[index];
    Ok(Some(CatiaSchemaConfigurationRecord {
        schema_payload_offset: u64_from_index(*schema_offset),
        schema_ordinal: *schema_ordinal,
        schema_entry: ctx
            .copy_retained_text(&selection.entry, "catia_native_configuration_entry")?,
        schema_name: ctx.copy_retained_text(&selection.name, "catia_native_configuration_name")?,
        entity_reference: CatiaPayloadEntityReference {
            payload_offset: u64_from_index(*entity_offset),
            reference: entity_reference(ctx, &object.parent, *referenced_entity_id, references)?,
        },
    }))
}

fn schema_configuration_row_link(
    ctx: &DecodeContext<'_>,
    entity_id: u32,
    object: &CatiaObjectRecord,
    references: &CatiaEntityReferenceIndex<'_>,
) -> Result<Option<CatiaSchemaConfigurationRowLink>, CodecError> {
    if object.entity_id() != Some(entity_id)
        || object.lead != 0x12
        || object.owner_entity_id().is_none()
        || object.class_name() != Some("configrow")
        || object.storage_ref().is_some()
    {
        return Ok(None);
    }
    let Some(class_entity_id) = object.class_ref() else {
        return Ok(None);
    };
    let [PayloadField::Atom { value: 250, .. }, PayloadField::Atom {
        value: successor_entity_id,
        offset: successor_offset,
    }, PayloadField::Terminator] = object.payload.fields.as_slice()
    else {
        return Ok(None);
    };
    Ok(Some(CatiaSchemaConfigurationRowLink {
        class_reference: entity_reference(ctx, &object.parent, class_entity_id, references)?,
        successor_payload_offset: u64_from_index(*successor_offset),
        successor: entity_reference(ctx, &object.parent, *successor_entity_id, references)?,
    }))
}

fn relation_program_instance_lead_12(
    entity_id: u32,
    fields: &[PayloadField],
) -> Option<(u32, u32, u32)> {
    let [PayloadField::Reference { .. }, PayloadField::Atom { value: 3, .. }, PayloadField::Reference {
        value: repeated_reference,
        ..
    }, PayloadField::Atom { .. }, PayloadField::Atom { .. }, PayloadField::Atom { value: 5, .. }, PayloadField::Atom { value: 89, .. }, PayloadField::Atom {
        value: 1_127_154_762,
        ..
    }, PayloadField::Reference { .. }, PayloadField::Atom {
        value: repeated_target,
        ..
    }, PayloadField::Reference { .. }, PayloadField::Atom { value: 2, .. }, PayloadField::Reference {
        value: repeated_target_reference,
        ..
    }, PayloadField::Reference {
        value: context_entity_id,
        ..
    }, PayloadField::Atom { value: 2, .. }, PayloadField::Reference {
        value: repeated_reference_copy,
        ..
    }, PayloadField::Atom {
        value: program_entity_id,
        ..
    }, PayloadField::Reference { .. }, PayloadField::Atom {
        value: stored_self, ..
    }, PayloadField::Terminator] = fields
    else {
        return None;
    };
    if repeated_reference != repeated_reference_copy
        || repeated_target != repeated_target_reference
        || *stored_self != entity_id
    {
        return None;
    }
    Some((*program_entity_id, *repeated_target, *context_entity_id))
}

fn relation_program_instance_lead_54(
    entity_id: u32,
    fields: &[PayloadField],
) -> Option<(u32, u32, u32)> {
    let [PayloadField::Atom { value: 244, .. }, PayloadField::Atom { value: 2, .. }, PayloadField::Reference {
        value: repeated_reference,
        ..
    }, PayloadField::Atom {
        value: program_entity_id,
        ..
    }, PayloadField::Atom {
        value: 2_142_008_808,
        ..
    }, PayloadField::Atom { value: 247, .. }, PayloadField::Atom {
        value: repeated_target,
        ..
    }, PayloadField::Reference { .. }, PayloadField::Atom {
        value: stored_self, ..
    }, PayloadField::Atom { value: 249, .. }, PayloadField::Atom { value: 2, .. }, PayloadField::Reference {
        value: repeated_target_reference,
        ..
    }, PayloadField::Reference { .. }, PayloadField::Atom { value: 2, .. }, PayloadField::Reference {
        value: repeated_reference_copy,
        ..
    }, PayloadField::Atom {
        value: trailing_entity_id,
        ..
    }, PayloadField::Atom { value: 129, .. }, PayloadField::Terminator] = fields
    else {
        return None;
    };
    if repeated_reference != repeated_reference_copy
        || repeated_target != repeated_target_reference
        || *stored_self != entity_id
    {
        return None;
    }
    Some((*program_entity_id, *repeated_target, *trailing_entity_id))
}

fn formula_relation(
    ctx: &DecodeContext<'_>,
    definitions: &[CatiaDefinitionSchemaSelection],
    entity_id: u32,
    object: &CatiaObjectRecord,
    relation_expressions: &CatiaRelationExpressionIndex<'_>,
    entity_references: &CatiaEntityReferenceIndex<'_>,
    parameter_bindings: &CatiaParameterBindingIndex<'_>,
) -> Result<Option<CatiaFormulaRelation>, CodecError> {
    let [definition0, definition1] = definitions else {
        return Ok(None);
    };
    if definition0.name.as_deref() != Some("Formula")
        || definition1.name.as_deref() != Some("Formula")
        || !equal_optional_text(
            ctx,
            definition0.entry.as_deref(),
            definition1.entry.as_deref(),
            "catia_native_formula_definition_entry",
        )?
    {
        return Ok(None);
    }
    let [PayloadField::Atom { value: 249, .. }, PayloadField::Atom { value: 4, .. }, PayloadField::Reference { value: owner, .. }, PayloadField::Reference {
        value: expression_entity_id,
        offset: expression_offset,
    }, PayloadField::Reference {
        value: parameter_entity_id,
        offset: parameter_offset,
    }, PayloadField::Atom { value: 129, .. }, PayloadField::Terminator] =
        object.payload.fields.as_slice()
    else {
        return Ok(None);
    };
    if *owner != entity_id {
        return Ok(None);
    }
    let [owner_reference, expression_reference, parameter_reference] = object.references.as_slice()
    else {
        return Ok(None);
    };
    if owner_reference.entity_id() != entity_id
        || expression_reference.entity_id() != *expression_entity_id
        || parameter_reference.entity_id() != *parameter_entity_id
        || !equal_optional_text(
            ctx,
            owner_reference.target(),
            Some(&object.id),
            "catia_native_formula_owner_target",
        )?
    {
        return Ok(None);
    }
    let Some(expression_object) = expression_reference.target() else {
        return Ok(None);
    };
    let Some(source) = ctx
        .get_hash_map(
            relation_expressions,
            expression_object,
            "catia_native_formula_expression_lookup",
        )?
        .copied()
    else {
        return Ok(None);
    };
    let parameter_dependencies =
        relation_parameter_dependencies(ctx, source, &object.parent, parameter_bindings)?;
    Ok(Some(CatiaFormulaRelation {
        expression_entity: CatiaPayloadEntityReference {
            payload_offset: u64_from_index(*expression_offset),
            reference: entity_reference(
                ctx,
                &object.parent,
                *expression_entity_id,
                entity_references,
            )?,
        },
        output_entity: CatiaPayloadEntityReference {
            payload_offset: u64_from_index(*parameter_offset),
            reference: {
                if parameter_reference.is_null() {
                    CatiaEntityReference::Null {
                        entity_id: *parameter_entity_id,
                    }
                } else {
                    entity_reference(ctx, &object.parent, *parameter_entity_id, entity_references)?
                }
            },
        },
        parameter_dependencies,
    }))
}

/// Relation expression sources by their object record.
type CatiaRelationExpressionIndex<'a> = HashMap<&'a str, &'a str>;
struct CatiaRelationExpressionEntity<'a> {
    entity: &'a str,
    source: &'a str,
    signature: Option<CatiaRelationTypeSignature>,
}
/// Rows keyed by object graph and then by entity id within that graph, so a
/// lookup hashes the borrowed graph text once and allocates nothing.
type CatiaGraphIdentityIndex<K, V> = HashMap<K, HashMap<u32, V>>;
type CatiaRelationExpressionEntityIndex<'a> =
    CatiaGraphIdentityIndex<&'a str, CatiaRelationExpressionEntity<'a>>;
type CatiaEntityByGraphIdentityIndex<'a> = CatiaGraphIdentityIndex<&'a str, &'a str>;
type CatiaEntityClassByGraphIdentityIndex<'g> = CatiaGraphIdentityIndex<&'g str, &'g str>;
/// The largest entity id of each object graph. The id after it is the graph's
/// terminal null.
type CatiaMaximumEntityByGraphIndex<'a> = HashMap<&'a str, u32>;
/// Named parameter entities by graph and then by binding symbol.
type CatiaParameterBindingIndex<'a> =
    HashMap<&'a str, HashMap<&'a str, Vec<CatiaParameterBinding<'a>>>>;

/// One named parameter entity bound to a symbol.
struct CatiaParameterBinding<'a> {
    entity_id: u32,
    entity: &'a str,
    class_name: Option<&'a str>,
}

/// Lookups over the decoded entity records, borrowed from them and held in
/// the caller's scoped storage.
struct CatiaSemanticIndices<'a> {
    relation_expressions: CatiaRelationExpressionIndex<'a>,
    relation_expression_entities: CatiaRelationExpressionEntityIndex<'a>,
    entities: CatiaEntityByGraphIdentityIndex<'a>,
    maxima: CatiaMaximumEntityByGraphIndex<'a>,
    parameter_bindings: CatiaParameterBindingIndex<'a>,
}

struct CatiaEntityReferenceIndex<'a> {
    entities: &'a CatiaEntityByGraphIdentityIndex<'a>,
    classes: &'a CatiaEntityClassByGraphIdentityIndex<'a>,
    maxima: &'a CatiaMaximumEntityByGraphIndex<'a>,
}

/// Looks up one entity row of a graph-identity index.
fn graph_identity_row<'v, K: std::borrow::Borrow<str> + Eq + std::hash::Hash, V>(
    ctx: &DecodeContext<'_>,
    index: &'v CatiaGraphIdentityIndex<K, V>,
    graph: &str,
    entity_id: u32,
    operation: &'static str,
) -> Result<Option<&'v V>, CodecError> {
    let Some(rows) = ctx.get_hash_map(index, graph, operation)? else {
        return Ok(None);
    };
    ctx.get_hash_map(rows, &entity_id, operation)
}

/// Inserts one entity row of a graph-identity index; a later row for the same
/// entity replaces the earlier one.
fn insert_graph_identity_row<'a, V>(
    ctx: &DecodeContext<'_>,
    index: &mut CatiaGraphIdentityIndex<&'a str, V>,
    graph: &'a str,
    entity_id: u32,
    value: V,
    operation: &'static str,
) -> Result<(), CodecError> {
    let rows = ctx.entry_hash_map(index, graph, operation)?.or_default();
    // discarded-value: a later row for the same entity replaces the earlier one.
    let _ = ctx.insert_hash_map(rows, entity_id, value, operation)?;
    Ok(())
}

fn entity_class_index<'g>(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    graphs: &'g [CatiaObjectGraph],
) -> Result<CatiaEntityClassByGraphIdentityIndex<'g>, CodecError> {
    let mut classes = HashMap::new();
    for graph in ctx.admit_iter(graphs, "catia_native_class_graph_visits")? {
        for record in ctx.admit_iter(&graph.records, "catia_native_class_record_visits")? {
            let (Some(entity_id), Some(class_name)) = (record.entity_id(), record.class_name())
            else {
                continue;
            };
            storage.with_storage(|| {
                insert_graph_identity_row(
                    ctx,
                    &mut classes,
                    record.parent.as_str(),
                    entity_id,
                    class_name,
                    "catia_native_class_index",
                )
            })?;
        }
    }
    Ok(classes)
}

fn semantic_entity_indices<'a>(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    entities: &'a [CatiaEntityRecord],
    entity_classes: &CatiaEntityClassByGraphIdentityIndex<'a>,
) -> Result<CatiaSemanticIndices<'a>, CodecError> {
    let mut indices = CatiaSemanticIndices {
        relation_expressions: HashMap::new(),
        relation_expression_entities: HashMap::new(),
        entities: HashMap::new(),
        maxima: HashMap::new(),
        parameter_bindings: HashMap::new(),
    };
    for entity in ctx.admit_iter(entities, "catia_native_semantic_entity_visits")? {
        let graph = entity.object_graph.as_str();
        storage.with_storage(|| -> Result<(), CodecError> {
            if let Some(expression) = entity.relation_expression() {
                let source = expression.expression.value.as_str();
                // discarded-value: a later expression of the same record replaces the earlier one.
                let _ = ctx.insert_hash_map(
                    &mut indices.relation_expressions,
                    entity.object_record.as_str(),
                    source,
                    "catia_native_expression_index",
                )?;
                let row = CatiaRelationExpressionEntity {
                    entity: &entity.id,
                    source,
                    signature: expression.signature_charged(ctx)?,
                };
                insert_graph_identity_row(
                    ctx,
                    &mut indices.relation_expression_entities,
                    graph,
                    entity.entity_id,
                    row,
                    "catia_native_expression_entity_index",
                )?;
            }
            insert_graph_identity_row(
                ctx,
                &mut indices.entities,
                graph,
                entity.entity_id,
                entity.id.as_str(),
                "catia_native_entity_index",
            )?;
            let maximum = ctx
                .entry_hash_map(&mut indices.maxima, graph, "catia_native_terminal_maxima")?
                .or_insert(entity.entity_id);
            *maximum = (*maximum).max(entity.entity_id);
            let Some(parameter) = entity.parameter_value() else {
                return Ok(());
            };
            let bindings = ctx
                .entry_hash_map(
                    &mut indices.parameter_bindings,
                    graph,
                    "catia_native_binding_graphs",
                )?
                .or_default();
            let references = ctx
                .entry_hash_map(
                    bindings,
                    parameter.binding.value.as_str(),
                    "catia_native_binding_symbols",
                )?
                .or_default();
            let class_name = graph_identity_row(
                ctx,
                entity_classes,
                graph,
                entity.entity_id,
                "catia_native_binding_class_lookup",
            )?
            .copied();
            ctx.push_vec(
                references,
                CatiaParameterBinding {
                    entity_id: entity.entity_id,
                    entity: &entity.id,
                    class_name,
                },
                "catia_native_binding_references",
            )
        })?;
    }
    Ok(indices)
}

/// Builds the class and semantic indexes of decoded records under a service context.
#[cfg(test)]
fn service_semantic_indices<'a>(
    graphs: &'a [CatiaObjectGraph],
    records: &'a [CatiaEntityRecord],
) -> Result<
    (
        CatiaEntityClassByGraphIdentityIndex<'a>,
        CatiaSemanticIndices<'a>,
    ),
    CodecError,
> {
    crate::test_support::with_service_context(|ctx| {
        let mut storage = ctx.reserve_scoped(0, "test semantic indices")?;
        let classes = entity_class_index(ctx, &mut storage, graphs)?;
        let indices = semantic_entity_indices(ctx, &mut storage, records, &classes)?;
        Ok((classes, indices))
    })
}

fn relation_parameter_dependencies(
    ctx: &DecodeContext<'_>,
    source: &str,
    graph: &str,
    parameter_bindings: &CatiaParameterBindingIndex<'_>,
) -> Result<Vec<CatiaRelationParameterDependency>, CodecError> {
    let symbols = relation_symbols(ctx, source)?;
    let graph_bindings = ctx.get_hash_map(
        parameter_bindings,
        graph,
        "catia_native_binding_graph_lookup",
    )?;
    ctx.try_collect_vec(
        symbols
            .into_iter()
            .map(|(source_offset, symbol)| -> Result<_, CodecError> {
                let mut candidates = Vec::new();
                let bound = match graph_bindings {
                    Some(bindings) => ctx.get_hash_map(
                        bindings,
                        symbol.as_str(),
                        "catia_native_binding_symbol_lookup",
                    )?,
                    None => None,
                };
                if let Some(bound) = bound {
                    ctx.reserve_vec(
                        &mut candidates,
                        bound.len(),
                        "catia_native_dependency_candidates",
                    )?;
                    for binding in ctx.admit_iter(bound, "catia_native_bound_candidate_visits")? {
                        candidates.push(CatiaEntityReference::Resolved {
                            entity_id: binding.entity_id,
                            entity: ctx.copy_retained_text(
                                binding.entity,
                                "catia_native_binding_entity",
                            )?,
                            class_name: binding
                                .class_name
                                .map(|class_name| {
                                    ctx.copy_retained_text(class_name, "catia_native_binding_class")
                                })
                                .transpose()?,
                        });
                    }
                }
                Ok(CatiaRelationParameterDependency {
                    source_offset,
                    symbol,
                    candidates,
                })
            }),
        "catia_native_dependencies",
    )
}

/// Borrows the canonical parameter prefix before an optional ordinal suffix.
pub(crate) fn relation_symbol_parameter<'a>(
    ctx: &DecodeContext<'_>,
    symbol: &'a str,
) -> Result<Option<&'a str>, CodecError> {
    let Some(rest) = symbol.strip_prefix('#') else {
        return Ok(None);
    };
    let mut bytes = rest.bytes().enumerate();
    while let Some((offset, byte)) =
        ctx.next_charged(&mut bytes, "catia_native_symbol_parameter_scan")?
    {
        if byte == b'_' {
            return Ok((offset > 0).then_some(&symbol[..offset + 2]));
        }
        if !byte.is_ascii_digit() {
            return Ok(None);
        }
    }
    Ok(None)
}

pub(crate) fn dependency_matches_input(
    ctx: &DecodeContext<'_>,
    dependency: &CatiaRelationParameterDependency,
    input: &CatiaRelationTypeInput,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "catia_native_input_ordinal_visits";
    let Some(suffix) = ctx.strip_prefix(
        dependency.symbol.as_str(),
        input.parameter.as_str(),
        "catia_native_input_prefix_checks",
    )?
    else {
        return Ok(false);
    };
    let suffix = ctx.trim_start_matches(
        suffix,
        |character| Ok(character.is_ascii_whitespace()),
        "catia_native_input_suffix_whitespace",
    )?;
    if suffix.is_empty() {
        return Ok(true);
    }
    let Some(ordinal) = suffix.strip_prefix('/') else {
        return Ok(false);
    };
    Ok(!ordinal.is_empty()
        && ctx.all_by(
            ordinal.as_bytes(),
            |byte| Ok(byte.is_ascii_digit()),
            OPERATION,
        )?)
}

/// Binds each signature input to the entity of the dependencies naming it.
/// Every dependency must name exactly one input; each input must be named,
/// and every dependency naming it must have the same single entity candidate.
fn resolved_relation_program_inputs(
    ctx: &DecodeContext<'_>,
    signature: &CatiaRelationTypeSignature,
    dependencies: &[CatiaRelationParameterDependency],
) -> Result<Option<Vec<CatiaRelationProgramInput>>, CodecError> {
    const OPERATION: &str = "catia_native_input_matching";
    let (input_index, _index_storage) = ctx.unique_index(
        signature
            .inputs
            .iter()
            .enumerate()
            .map(|(index, input)| (input.parameter.as_str(), index)),
        "catia_native_input_signature_index",
    )?;
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let mut groups = scratch.with_storage(|| {
        ctx.collect_vec(
            signature.inputs.iter().map(|_| Vec::<usize>::new()),
            OPERATION,
        )
    })?;
    let mut dependency_rows = dependencies.iter().enumerate();
    while let Some((dependency_index, dependency)) =
        ctx.next_charged(&mut dependency_rows, "catia_native_input_dependency_checks")?
    {
        let Some(parameter) = relation_symbol_parameter(ctx, &dependency.symbol)? else {
            return Ok(None);
        };
        let Some(&Some(index)) = ctx.get_hash_map(
            &input_index,
            parameter,
            "catia_native_input_signature_checks",
        )?
        else {
            return Ok(None);
        };
        if !dependency_matches_input(ctx, dependency, &signature.inputs[index])? {
            return Ok(None);
        }
        scratch.with_storage(|| ctx.push_vec(&mut groups[index], dependency_index, OPERATION))?;
    }
    let mut entity_ids = HashSet::new();
    let mut output_storage = ctx.reserve_scoped(0, "catia_native_program_input_scratch")?;
    let mut inputs = Vec::new();
    let mut input_rows = signature.inputs.iter().enumerate();
    while let Some((input_index, input)) =
        ctx.next_charged(&mut input_rows, "catia_native_input_signature_visits")?
    {
        let mut selected = None::<&CatiaEntityReference>;
        let mut group = groups[input_index].iter();
        while let Some(&dependency_index) =
            ctx.next_charged(&mut group, "catia_native_input_candidate_dependencies")?
        {
            let [candidate] = dependencies[dependency_index].candidates.as_slice() else {
                return Ok(None);
            };
            if candidate.is_null() || candidate.entity().is_none() {
                return Ok(None);
            }
            match selected {
                Some(previous) if !previous.equal_charged(ctx, candidate, OPERATION)? => {
                    return Ok(None)
                }
                Some(_) => {}
                None => selected = Some(candidate),
            }
        }
        let Some(entity) = selected else {
            return Ok(None);
        };
        if !scratch.with_storage(|| {
            ctx.insert_hash_set(
                &mut entity_ids,
                entity.entity_id(),
                "catia_native_input_entity_ids",
            )
        })? {
            return Ok(None);
        }
        let input = output_storage.with_storage(|| -> Result<_, CodecError> {
            Ok(CatiaRelationProgramInput {
                parameter: ctx
                    .copy_retained_text(&input.parameter, "catia_native_input_parameter")?,
                value_type: ctx.copy_retained_text(&input.input_type, "catia_native_input_type")?,
                entity: entity.copy_charged(ctx)?,
            })
        })?;
        output_storage
            .with_storage(|| ctx.push_vec(&mut inputs, input, "catia_native_program_inputs"))?;
    }
    output_storage.commit()?;
    Ok(Some(inputs))
}

pub(crate) fn relation_symbols(
    ctx: &DecodeContext<'_>,
    source: &str,
) -> Result<Vec<(u64, String)>, CodecError> {
    let bytes = source.as_bytes();
    let mut symbols = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let Some(byte) = ctx.next_charged(&mut bytes[at..].iter(), "catia_native_symbol_scan")?
        else {
            break;
        };
        if *byte == b'"' {
            at += 1;
            let mut quoted = bytes[at..].iter();
            while let Some(byte) = ctx.next_charged(&mut quoted, "catia_native_symbol_scan")? {
                if *byte == b'"' {
                    break;
                }
                at += 1;
            }
            at += usize::from(at < bytes.len());
            continue;
        }
        if bytes[at] != b'#' {
            at += 1;
            continue;
        }
        let start = at;
        at += 1;
        let digits_start = at;
        let mut digits = bytes[at..].iter();
        while let Some(byte) = ctx.next_charged(&mut digits, "catia_native_symbol_scan")? {
            if !byte.is_ascii_digit() {
                break;
            }
            at += 1;
        }
        if at == digits_start || bytes.get(at) != Some(&b'_') {
            at = start + 1;
            continue;
        }
        at += 1;
        let bare_end = at;
        let mut whitespace = bytes[at..].iter();
        while let Some(byte) = ctx.next_charged(&mut whitespace, "catia_native_symbol_scan")? {
            if !byte.is_ascii_whitespace() {
                break;
            }
            at += 1;
        }
        if bytes.get(at) != Some(&b'/') {
            let source_offset = u64_from_index(start);
            let symbol =
                ctx.copy_retained_text(&source[start..bare_end], "catia_native_symbol_text")?;
            ctx.push_vec(
                &mut symbols,
                (source_offset, symbol),
                "catia_native_symbols",
            )?;
            at = bare_end;
            continue;
        }
        at += 1;
        let ordinal_start = at;
        let mut digits = bytes[at..].iter();
        while let Some(byte) = ctx.next_charged(&mut digits, "catia_native_symbol_scan")? {
            if !byte.is_ascii_digit() {
                break;
            }
            at += 1;
        }
        if at == ordinal_start {
            at = start + 1;
            continue;
        }
        let source_offset = u64_from_index(start);
        let symbol = ctx.copy_retained_text(&source[start..at], "catia_native_symbol_text")?;
        ctx.push_vec(
            &mut symbols,
            (source_offset, symbol),
            "catia_native_symbols",
        )?;
    }
    Ok(symbols)
}

fn value_field_offset(field: &value_block::ValueField) -> usize {
    match field {
        value_block::ValueField::SchemaSelector { offset, .. }
        | value_block::ValueField::Binary64 { offset, .. }
        | value_block::ValueField::Marker { offset, .. }
        | value_block::ValueField::Opcode { offset, .. }
        | value_block::ValueField::Separator { offset }
        | value_block::ValueField::Inline { offset, .. }
        | value_block::ValueField::ByteString { offset, .. }
        | value_block::ValueField::Atom { offset, .. }
        | value_block::ValueField::Terminator { offset }
        | value_block::ValueField::Literal { offset, .. } => *offset,
    }
}

fn repeated_reference_schema_selection(
    ctx: &DecodeContext<'_>,
    preamble: Option<&object_graph::ReferenceSchemaPreamble>,
    catalog: Option<&CatiaCatalog>,
) -> Result<Option<CatiaRepeatedReferenceSchemaSelection>, CodecError> {
    let Some(preamble) = preamble else {
        return Ok(None);
    };
    let (order, ordinal, offset) = match preamble {
        object_graph::ReferenceSchemaPreamble::BlobThenSchema { schema_ref, offset } => (
            CatiaRepeatedReferenceSchemaOrder::BlobThenSchema,
            *schema_ref,
            *offset,
        ),
        object_graph::ReferenceSchemaPreamble::SchemaThenBlob { schema_ref, offset } => (
            CatiaRepeatedReferenceSchemaOrder::SchemaThenBlob,
            *schema_ref,
            *offset,
        ),
    };
    let catalog_entry = usize::try_from(ordinal)
        .ok()
        .and_then(|ordinal| catalog?.entries.get(ordinal));
    Ok(Some(CatiaRepeatedReferenceSchemaSelection {
        order,
        offset: u64_from_index(offset),
        ordinal,
        entry: catalog_entry
            .map(|entry| ctx.copy_retained_text(&entry.id, "catia_repeated_reference_entry"))
            .transpose()?,
        name: catalog_entry
            .map(|entry| ctx.copy_retained_text(&entry.value, "catia_repeated_reference_name"))
            .transpose()?,
    }))
}

/// One stored entity identity in a pre-`7C05` design stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaLegacyEntityIdentity {
    /// Offset of the `EA` identity delimiter.
    byte_offset: u64,
    /// Little-endian identity following the delimiter.
    pub(crate) entity_id: u32,
    /// Stored record lead following the identity.
    pub(crate) lead: legacy_entity::CatiaLegacyIdentityLead,
}

/// One complete compact legacy schema program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaLegacySchemaProgram {
    /// Offset of the first program byte after the fixed prefix.
    pub(crate) byte_offset: u64,
    /// Offset of the production following the program.
    #[serde(alias = "footer_byte_offset")]
    pub(crate) boundary_byte_offset: u64,
    /// Production that closes the program.
    #[serde(default)]
    pub(crate) boundary: CatiaLegacySchemaProgramBoundary,
    /// Exact program bytes, including the terminal `FE`.
    #[serde(with = "cadmpeg_ir::bytes")]
    pub(crate) data: Vec<u8>,
    /// Complete inclusive-length identifier packets in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) identifiers: Vec<CatiaLegacySchemaIdentifier>,
}

/// Production that closes a compact legacy schema program.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CatiaLegacySchemaProgramBoundary {
    /// Fixed vendor footer preceded by the terminal `FE`.
    #[default]
    VendorFooter,
    /// Validated outer stream directory preceded by the terminal `FE`.
    StreamDirectory,
}

/// One complete inclusive-length identifier packet in a compact schema program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaLegacySchemaIdentifier {
    /// Offset of the inclusive-length byte.
    pub(crate) byte_offset: u64,
    /// Stored identifier.
    pub(crate) value: String,
}

/// Framing production used by a legacy schema text field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CatiaLegacyTextEncoding {
    /// Nonzero one-byte inclusive length.
    U8InclusiveLength,
    /// Zero selector and little-endian `u32` byte length.
    ZeroU32Length,
    /// Nonzero inclusive length followed by an `E3` paged-role tail.
    U8InclusiveLengthE3RoleTail,
}

/// Framing production used by a legacy role selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CatiaLegacyRoleSelectorEncoding {
    /// `80` followed by a nonzero little-endian `u32`.
    FixedU32,
    /// Page byte `D1..E4` followed by one low byte.
    Paged,
}

/// One length-framed legacy schema role and its selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaLegacyRoleSelector {
    /// Offset of the literal length or schema-selector byte.
    byte_offset: u64,
    /// Stored identity whose interval contains the role.
    #[serde(default)]
    pub(crate) entity_id: u32,
    /// Stored literal or unresolved role name.
    pub(crate) name: legacy_entity::LegacyRoleName,
    /// Selector framing production.
    pub(crate) encoding: CatiaLegacyRoleSelectorEncoding,
    /// Stored selector following the role name.
    pub(crate) selector: u32,
    /// Field code when an `E8 <field-code:u16le> 01` opener follows immediately.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) field_code: Option<u16>,
}

impl CatiaLegacyRoleSelector {
    pub(crate) fn end_offset(&self) -> Option<u64> {
        let selector_len = match self.encoding {
            CatiaLegacyRoleSelectorEncoding::FixedU32 => 5,
            CatiaLegacyRoleSelectorEncoding::Paged => 2,
        };
        self.byte_offset
            .checked_add(u64_from_index(self.name.byte_len()))?
            .checked_add(selector_len)
    }
}

/// One complete legacy schema text field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaLegacyTextField {
    /// Offset of the field opener.
    pub(crate) byte_offset: u64,
    /// Stored identity whose interval contains the field.
    pub(crate) entity_id: u32,
    /// Text framing production.
    pub(crate) encoding: CatiaLegacyTextEncoding,
    /// Immediately preceding length-framed role and selector.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) role: Option<CatiaLegacyRoleSelector>,
    /// Decoded UTF-8 value.
    pub(crate) value: String,
}

/// One legacy schema field bounded by consecutive role selectors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaLegacySchemaField {
    /// Offset of the `E8 <field-code:u16le> 01` opener.
    byte_offset: u64,
    /// Stored identity whose interval contains the field.
    entity_id: u32,
    /// Role selector that binds this field.
    role_byte_offset: u64,
    /// Following role selector that closes the payload.
    pub(crate) boundary_role_byte_offset: u64,
    /// Stored schema field code.
    pub(crate) field_code: u16,
    /// Exact bytes after the opener and before the boundary role.
    pub(crate) payload: Vec<u8>,
}

/// One typed parameter role in a legacy relation signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaLegacyRelationParameter {
    /// Expression-local parameter.
    pub(crate) parameter: String,
    /// Source value type.
    pub(crate) value_type: String,
}

/// One complete legacy expression and type-signature pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaLegacyRelation {
    /// Stored owner identity.
    entity_id: u32,
    /// Selector carried by the expression field's `body` role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    body_selector: Option<u32>,
    /// Selector carried by the type-signature field's `param` role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parameter_selector: Option<u32>,
    /// Parameter identity selected by exact self-`body` and target-`param` roles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) parameter_entity_id: Option<u32>,
    /// Expression-field opener offset.
    pub(crate) expression_offset: u64,
    /// Exact expression or rule program.
    pub(crate) expression: String,
    /// Signature-field opener offset.
    signature_offset: u64,
    /// Exact stored type signature.
    type_signature: String,
    /// Ordered input parameters.
    pub(crate) inputs: Vec<CatiaLegacyRelationParameter>,
    /// Output parameter for a `VoidType` relation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) output: Option<CatiaLegacyRelationParameter>,
    /// Source result type.
    pub(crate) result_type: String,
}

/// One complete legacy `synchrone` relation-update field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaLegacyRelationSynchronousState {
    /// Offset of the `synchrone` role-name length byte.
    role_byte_offset: u64,
    /// Stored containing identity.
    entity_id: u32,
    /// Selector carried by the `synchrone` role.
    pub(crate) selector: u32,
    /// Whether the relation updates synchronously.
    pub(crate) synchronous: bool,
}

/// Value selected by one complete legacy type descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CatiaLegacyTypeValue {
    /// Inclusive-length UTF-8 type name.
    Name {
        /// Stored type name.
        value: String,
    },
    /// Compact unresolved selector identity.
    Selector {
        /// Stored selector identity.
        value: u32,
    },
}

/// One complete type descriptor in a legacy identity interval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaLegacyTypeDescriptor {
    /// Offset of the fixed descriptor prefix.
    byte_offset: u64,
    /// Stored containing identity.
    pub(crate) entity_id: u32,
    /// Stored literal name or unresolved selector.
    pub(crate) value: CatiaLegacyTypeValue,
}

/// Evaluation stored by a complete legacy scalar packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CatiaLegacyScalarEvaluation {
    /// Finite binary64 scalar.
    Value {
        /// Exact IEEE-754 bits.
        bits: u64,
    },
    /// Stored unset evaluation.
    Unset,
}

/// Fixed prefix selecting one legacy scalar production.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CatiaLegacyScalarEncoding {
    /// `FE 84 88 82 FE`.
    Named84,
    /// `FE 85 88 82 FE`.
    Standalone85,
}

/// One complete typed scalar packet in a legacy identity interval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaLegacyScalarValue {
    /// Stable native identity derived from the containing run and packet offset.
    pub(crate) id: String,
    /// Offset of the packet prefix.
    pub(crate) byte_offset: u64,
    /// Stored containing identity.
    pub(crate) entity_id: u32,
    /// Fixed scalar-prefix production.
    pub(crate) encoding: CatiaLegacyScalarEncoding,
    /// Unique co-owned `name` text field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name_field: Option<u64>,
    /// Unique co-owned stored name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    /// Stored evaluation.
    pub(crate) evaluation: CatiaLegacyScalarEvaluation,
}

/// One complete legacy UTF-8 string-value packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaLegacyStringValue {
    /// Stable native identity derived from the containing run and packet offset.
    pub(crate) id: String,
    /// Offset of the packet prefix.
    pub(crate) byte_offset: u64,
    /// Stored containing identity.
    pub(crate) entity_id: u32,
    /// Unique co-owned `name` text field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name_field: Option<u64>,
    /// Unique co-owned stored name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    /// Stored UTF-8 value.
    pub(crate) value: String,
}

/// Stored encoding of one complete legacy signed integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CatiaLegacyIntegerEncoding {
    /// One byte stores values zero through 126 as `value + 0x81`.
    Inline,
    /// `80` introduces one signed little-endian 32-bit value.
    WideI32,
}

/// One complete legacy signed-integer packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaLegacyIntegerValue {
    /// Stable native identity derived from the containing run and packet offset.
    pub(crate) id: String,
    /// Offset of the packet prefix.
    pub(crate) byte_offset: u64,
    /// Stored containing identity.
    pub(crate) entity_id: u32,
    /// Stored integer encoding.
    encoding: CatiaLegacyIntegerEncoding,
    /// Unique co-owned `name` text field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name_field: Option<u64>,
    /// Unique co-owned stored name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    /// Stored signed value.
    pub(crate) value: i32,
}

/// A monotonically identified pre-`7C05` run and its terminating catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaLegacyEntityRun {
    /// Stable native identity.
    pub(crate) id: String,
    /// Offset of the first identity delimiter.
    pub(crate) byte_offset: u64,
    /// Bytes from the first identity delimiter to the catalog opener.
    byte_len: u64,
    /// Offset of the fixed schema-catalog opening production.
    pub(crate) catalog_offset: u64,
    /// Complete compact schema program following the catalog opener.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) schema_program: Option<CatiaLegacySchemaProgram>,
    /// Exact declared outer container whose physical stream contains this run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) outer_container: Option<CatiaOuterContainerBinding>,
    /// Stored identities in source order.
    pub(crate) identities: Vec<CatiaLegacyEntityIdentity>,
    /// Complete length-framed role selectors in identity-interval order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) role_selectors: Vec<CatiaLegacyRoleSelector>,
    /// Complete schema text fields in identity-interval order.
    pub(crate) text_fields: Vec<CatiaLegacyTextField>,
    /// Complete role-bounded schema fields in identity-interval order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) schema_fields: Vec<CatiaLegacySchemaField>,
    /// Complete expression/signature pairs.
    pub(crate) relations: Vec<CatiaLegacyRelation>,
    /// Complete `synchrone` relation-update fields.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) synchronous_states: Vec<CatiaLegacyRelationSynchronousState>,
    /// Complete literal or selector type descriptors.
    pub(crate) type_descriptors: Vec<CatiaLegacyTypeDescriptor>,
    /// Complete typed scalar packets.
    pub(crate) scalar_values: Vec<CatiaLegacyScalarValue>,
    /// Complete UTF-8 string-value packets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) string_values: Vec<CatiaLegacyStringValue>,
    /// Complete signed-integer packets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) integer_values: Vec<CatiaLegacyIntegerValue>,
}

/// One zero-entity face-local surface-support occurrence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CatiaZeroEntitySupportOccurrence {
    /// Byte offset of the framed `21xx` record.
    byte_offset: u64,
    /// One-based global record ordinal in the zero-entity stream.
    pub(crate) record_ordinal: u32,
    /// Complete two-byte record tag.
    pub(crate) tag: [u8; 2],
    /// Face-local support slot stored at record offset 12.
    pub(crate) face_local_slot: u32,
    /// Stored UV endpoints when the record family carries them inline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) uv_endpoints: Option<[[FiniteReal; 2]; 2]>,
    /// Complete parameter-space curve carried by the support record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) pcurve: Option<cadmpeg_ir::geometry::pcurve::PcurveGeometry>,
    /// Exact model-space carrier derived from the pcurve and owning surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) model_curve: Option<cadmpeg_ir::geometry::CurveGeometry>,
    /// Exact procedural model-space carrier derived from the pcurve and owning surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) model_curve_construction: Option<cadmpeg_ir::geometry::ProceduralCurveDefinition>,
    /// Model-carrier parameters at the two stored UV endpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) model_parameters: Option<[FiniteReal; 2]>,
    /// Surface point at the midpoint of the bounded pcurve parameter interval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) model_midpoint: Option<FinitePoint3>,
    /// UV endpoints lifted through the owning surface carrier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) model_endpoints: Option<[FinitePoint3; 2]>,
}

/// One counted zero-entity `5fxx` face record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CatiaZeroEntityFace {
    /// Byte offset of the framed face record.
    byte_offset: u64,
    /// One-based global record ordinal.
    pub(crate) record_ordinal: u32,
    /// Complete two-byte record tag.
    tag: [u8; 2],
    /// Counted allocation values in storage order.
    pub(crate) allocations: Vec<u32>,
    /// Ordered loop terminals derived from the allocation lane.
    pub(crate) loop_terminals: Vec<u32>,
    /// Positionally aligned loop records.
    pub(crate) loops: Vec<CatiaZeroEntityLoop>,
    /// Terminal control byte following the allocation lane.
    pub(crate) terminal_control: u8,
}

/// One counted zero-entity `62xx` loop record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CatiaZeroEntityLoop {
    /// Byte offset of the framed loop record.
    byte_offset: u64,
    /// One-based global record ordinal.
    record_ordinal: u32,
    /// Complete two-byte record tag.
    tag: [u8; 2],
    /// Nonterminal even-lane logical member identifiers.
    pub(crate) member_ids: Vec<u32>,
    /// Odd-lane typed references in member order.
    pub(crate) typed_references: Vec<u32>,
    /// Global zero-entity records selected by the typed references.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) typed_records: Vec<String>,
    /// Face-local support record ordinals selected by the logical members.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) support_record_ordinals: Vec<u32>,
    /// Terminal even-lane logical identifier.
    pub(crate) terminal_id: u32,
    /// Difference between the terminal and first member identifiers.
    pub(crate) gap: u32,
    /// Stored loop-class byte.
    pub(crate) loop_class: u8,
    /// Absolute coedge senses in member order; `true` is forward.
    pub(crate) forward_senses: Vec<bool>,
    /// Complete sense-oriented model-space endpoint pairs in member order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) oriented_model_endpoints: Vec<[FinitePoint3; 2]>,
}

/// One zero-entity surface carrier and its maximal following support run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CatiaZeroEntitySupportRun {
    /// Stable native-run identity.
    id: String,
    /// Byte offset of the owning surface-carrier record.
    pub(crate) carrier_byte_offset: u64,
    /// One-based global record ordinal of the owning surface carrier.
    pub(crate) carrier_record_ordinal: u32,
    /// Positionally aligned face record when the complete rosters agree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) face: Option<CatiaZeroEntityFace>,
    /// Face-local support occurrences in storage order.
    pub(crate) supports: Vec<CatiaZeroEntitySupportOccurrence>,
}

/// One zero-entity `5e1a` allocation tuple.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaZeroEntityEdgeStride {
    /// Stable native-record identity.
    id: String,
    /// Byte offset of the framed record.
    byte_offset: u64,
    /// One-based global record ordinal in the zero-entity stream.
    pub(crate) record_ordinal: u32,
    /// Five allocation values following the fixed tagged-one prefix.
    pub(crate) allocations: [u32; 5],
    /// The three allocations in the `0638`/`2569` topology namespace, in
    /// source order `[T, T-1, T-2]`.
    pub(crate) topology_refs: [u32; 3],
    /// The two allocations selecting the adjacent surface-support slots, in
    /// source order `[X, Y]`.
    pub(crate) surface_support_refs: [u32; 2],
}

/// One positional zero-entity `0638` oriented use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaZeroEntityOrientedUse {
    /// Byte offset of the framed record.
    byte_offset: u64,
    /// One-based global record ordinal in the zero-entity stream.
    record_ordinal: u32,
    /// Positional side number.
    side: u32,
    /// Two stored allocation values.
    pub(crate) allocations: [u32; 2],
}

/// One zero-entity `2569` header and its two positional uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaZeroEntityOrientedUsePair {
    /// Stable native-pair identity.
    id: String,
    /// Byte offset of the `2569` header.
    header_byte_offset: u64,
    /// One-based global record ordinal of the `2569` header.
    pub(crate) header_record_ordinal: u32,
    /// Stored base columns.
    pub(crate) base_columns: [u32; 2],
    /// Side-one then side-two oriented uses.
    pub(crate) uses: [CatiaZeroEntityOrientedUse; 2],
}

/// Two zero-entity radial support occurrences with matching bounded model-space witnesses.
///
/// This relation does not establish curve coincidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CatiaZeroEntityEndpointPairCandidate {
    /// Stable derived-pair identity.
    pub(crate) id: String,
    /// Two face-record identities in support-record order.
    pub(crate) face_records: [String; 2],
    /// Two radial support-record identities in ascending ordinal order.
    pub(crate) support_records: [String; 2],
    /// Model-space endpoints oriented by the first support occurrence.
    pub(crate) model_endpoints: [FinitePoint3; 2],
    /// Model-space midpoint witness supplied by the first support occurrence.
    pub(crate) model_midpoint: FinitePoint3,
}

/// One endpoint-pair endpoint incident to a geometric endpoint-locus candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaZeroEntityEndpointPairEndpoint {
    /// Derived endpoint-pair candidate.
    pub(crate) endpoint_pair: String,
    /// Start or end of that candidate's oriented endpoint pair.
    pub(crate) endpoint_index: EdgeEnd,
}

/// One geometric endpoint-locus candidate established by a complete endpoint clique.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CatiaZeroEntityEndpointLocusCandidate {
    /// Stable derived-locus identity.
    pub(crate) id: String,
    /// Incident endpoints in endpoint-pair and endpoint order.
    pub(crate) incident_endpoint_pair_endpoints:
        cadmpeg_ir::features::NonEmptyMembers<CatiaZeroEntityEndpointPairEndpoint>,
    /// Model-space point from the first incident endpoint.
    pub(crate) representative_point: FinitePoint3,
    /// Maximum pairwise distance between incident endpoint coordinates.
    pub(crate) maximum_deviation: cadmpeg_ir::scalar::NonNegativeReal,
}

/// One counted zero-entity `05xx` vertex-incidence record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaZeroEntityVertexIncidence {
    /// Stable native-record identity.
    id: String,
    /// Byte offset of the framed record.
    byte_offset: u64,
    /// One-based global record ordinal in the zero-entity stream.
    pub(crate) record_ordinal: u32,
    /// Complete two-byte record tag.
    tag: [u8; 2],
    /// Stored allocation values.
    pub(crate) allocations: Vec<u32>,
    /// Immediately following `5d06` vertex-owner record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) vertex_record: Option<String>,
}

/// One complete zero-entity face-roster, shell, and body ownership hierarchy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaZeroEntityOwnershipRoot {
    /// Stable native-root identity.
    pub(crate) id: String,
    /// Byte offset of the counted `6142` face-roster record.
    face_roster_byte_offset: u64,
    /// One-based global record ordinal of the face-roster record.
    pub(crate) face_roster_record_ordinal: u32,
    /// Descending one-based face-allocation slots.
    pub(crate) face_slots: Vec<u32>,
    /// Byte offset of the `6006` shell root.
    pub(crate) shell_byte_offset: u64,
    /// One-based global record ordinal of the shell root.
    pub(crate) shell_record_ordinal: u32,
    /// Byte offset of the `6508` body root.
    body_byte_offset: u64,
    /// One-based global record ordinal of the body root.
    pub(crate) body_record_ordinal: u32,
}

/// One framed record in the zero-entity global identity namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CatiaZeroEntityRecord {
    /// Stable native-record identity.
    id: String,
    /// Byte offset of the framed record.
    pub(crate) byte_offset: u64,
    /// Exclusive logical byte end, including any inline continuation.
    pub(crate) logical_end: u64,
    /// Complete two-byte record tag.
    pub(crate) tag: [u8; 2],
    /// One-based global record ordinal.
    pub(crate) record_ordinal: u32,
}

macro_rules! define_catia_arenas {
    (
        $(
            $field:ident: $record:ty {
                $(
                    $(#[$attr:meta])*
                    $vis:vis $stored:ident;
                )?
                $(
                    => $owner:ident.$children:ident;
                )?
            }
        ),+ $(,)?
    ) => {
        /// Complete CATIA native arena manifest in stable order.
        ///
        /// The manifest states what `native_store_paths_cover_every_declared_arena`
        /// checks the store against, and nothing in the decode reads it, so it is
        /// built for that test alone.
        #[cfg(test)]
        const CATIA_ARENA_NAMES: &[&str] = &[
            $(stringify!($field)),+
        ];

        /// CATIA-native records retained outside the format-neutral model.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[serde(try_from = "CatiaNativeWire", into = "CatiaNativeWire")]
        pub(crate) struct CatiaNative {
            $(
                $(
                    $(#[$attr])*
                    #[serde(default)]
                    $vis $field: Vec<$record>,
                )?
            )*
        }

        #[derive(Serialize, Deserialize)]
        struct CatiaNativeWire {
            $($( $(#[$attr])* #[serde(default)]
                $field: define_catia_arenas!(@native_type $field, $stored, $record),
            )?)*
        }

        impl From<CatiaNative> for CatiaNativeWire {
            fn from(mut native: CatiaNative) -> Self {
                let nodes = edge_node_wires(std::mem::take(&mut native.consolidated_edge_nodes), &native.consolidated_vertex_identities);
                Self {
                    $($( $field: define_catia_arenas!(@native_value $field, $stored, native, nodes), )?)*
                }
            }
        }

        impl TryFrom<CatiaNativeWire> for CatiaNative {
            type Error = String;
            fn try_from(mut wire: CatiaNativeWire) -> Result<Self, Self::Error> {
                let nodes = load_edge_nodes(std::mem::take(&mut wire.consolidated_edge_nodes), &wire.consolidated_vertex_identities)?;
                Ok(Self {
                    $($( $field: define_catia_arenas!(@native_value $field, $stored, wire, nodes), )?)*
                })
            }
        }

        /// Owning, flattened arena payload shared by borrowed and consuming stores.
        struct CatiaArenaProjection {
            $(
                $(
                    $field: define_catia_arenas!(@type $field, $stored, $record),
                )?
            )*
            $(
                $(
                    $field: define_catia_arenas!(
                        @flattened_type $field, $owner, $children, $record
                    ),
                )?
            )*
        }

        impl CatiaArenaProjection {
            fn from_owned(
                ctx: &DecodeContext<'_>,
                mut native: CatiaNative,
            ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
                $(
                    $(
                        define_catia_arenas!(
                            @flattened_prepare ctx, native, $field, $owner, $children
                        );
                    )?
                )*
                $(
                    $(
                        define_catia_arenas!(@prepare ctx, $field, native, $stored, $field);
                    )?
                )*
                Ok(Self {
                    $(
                        $(
                            $field: define_catia_arenas!(
                                @stored_value $stored, native, $field, $field
                            ),
                        )?
                    )*
                    $(
                        $(
                            $field: define_catia_arenas!(
                                @flattened_value $owner, $children, $field
                            ),
                        )?
                    )*
                })
            }
        }

        type CatiaFamilyRow =
            FamilyRow<CatiaArenaProjection, (), cadmpeg_ir::NativeNamespace, ()>;

        /// Declarative CATIA native-family catalogue.
        const CATIA_FAMILIES: &[CatiaFamilyRow] = &[
            $(
                $(
                    define_catia_arenas!(@family $stored, $field),
                )?
            )*
            $(
                $(
                    define_catia_arenas!(@flattened_family $owner, $children, $field),
                )?
            )*
        ];

        impl Default for CatiaNative {
            fn default() -> Self {
                Self {
                    $(
                        $(
                            $field: define_catia_arenas!(@default $stored),
                        )?
                    )*
                }
            }
        }
    };
    (@native_type consolidated_edge_nodes, $kind:ident, $record:ty) => { Vec<CatiaConsolidatedEdgeNodeWire> };
    (@native_type $field:ident, $kind:ident, $record:ty) => { Vec<$record> };
    (@native_value consolidated_edge_nodes, $kind:ident, $owner:ident, $nodes:ident) => { $nodes };
    (@native_value $field:ident, $kind:ident, $owner:ident, $nodes:ident) => { $owner.$field };
    (@type consolidated_edge_nodes, $kind:ident, $record:ty) => { Vec<CatiaConsolidatedEdgeNodeWire> };
    (@type entity_records, $kind:ident, $record:ty) => { Vec<CatiaEntityRecordWire> };
    (@type object_graph_records, $kind:ident, $record:ty) => { Vec<CatiaObjectRecordWire> };
    (@type value_blocks, $kind:ident, $record:ty) => { Vec<CatiaValueBlockWire> };
    (@type schema_configuration_row_chains, $kind:ident, $record:ty) => {
        Vec<schema_configuration_chain::ChainWire>
    };
    (@prepare $ctx:ident, consolidated_edge_nodes, $native:ident, $kind:ident, $binding:ident) => {
        let $binding = edge_node_wires_charged($ctx, std::mem::take(&mut $native.consolidated_edge_nodes), &$native.consolidated_vertex_identities)?;
    };
    (@prepare $ctx:ident, entity_records, $native:ident, $kind:ident, $binding:ident) => {
        let $binding = $ctx.try_collect_vec(std::mem::take(&mut $native.entity_records).into_iter()
                .map(|record| CatiaEntityRecordWire::from_charged($ctx, record)), "catia_native_entity_wires")?;
    };
    (@prepare $ctx:ident, object_graph_records, $native:ident, $kind:ident, $binding:ident) => {
        let $binding = $ctx.try_collect_vec(std::mem::take(&mut $native.object_graph_records).into_iter()
                .map(|record| CatiaObjectRecordWire::from_charged($ctx, record)), "catia_native_object_record_wires")?;
    };
    (@prepare $ctx:ident, value_blocks, $native:ident, $kind:ident, $binding:ident) => {
        let $binding = $ctx.try_collect_vec(std::mem::take(&mut $native.value_blocks).into_iter()
                .map(|block| CatiaValueBlockWire::from_charged($ctx, block)), "catia_native_value_block_wires")?;
    };
    (@prepare $ctx:ident, schema_configuration_row_chains, $native:ident, $kind:ident, $binding:ident) => {
        let $binding = $ctx.try_collect_vec(std::mem::take(&mut $native.schema_configuration_row_chains).into_iter()
                .map(|chain| schema_configuration_chain::ChainWire::from_charged($ctx, chain)), "catia_native_configuration_chain_wires")?;
    };
    (@stored_value stored, $native:ident, consolidated_edge_nodes, $binding:ident) => { $binding };
    (@stored_value stored, $native:ident, entity_records, $binding:ident) => { $binding };
    (@stored_value stored, $native:ident, object_graph_records, $binding:ident) => { $binding };
    (@stored_value stored, $native:ident, value_blocks, $binding:ident) => { $binding };
    (@stored_value stored, $native:ident, schema_configuration_row_chains, $binding:ident) => { $binding };
    (@type catalogs, $kind:ident, $record:ty) => {
        Vec<CatiaCatalogWire>
    };
    (@type $field:ident, $kind:ident, $record:ty) => {
        Vec<$record>
    };
    (@flattened_type object_graph_records, $owner:ident, $children:ident, $record:ty) => {
        Vec<CatiaObjectRecordWire>
    };
    (@flattened_type $field:ident, $owner:ident, $children:ident, $record:ty) => {
        Vec<$record>
    };
    (@flattened_collect $ctx:ident, $native:ident, object_graph_records, $owner:ident, $children:ident) => {
        $ctx.try_collect_vec($ctx.admit_iter(&mut $native.$owner, "catia_native_flattened_parent_visits").map_err(CodecError::from)?
                .flat_map(|parent| std::mem::take(&mut parent.$children))
                .map(|record| CatiaObjectRecordWire::from_charged($ctx, record)), "catia_native_object_record_wires")
    };
    (@flattened_collect $ctx:ident, $native:ident, $field:ident, $owner:ident, $children:ident) => {
        $ctx.collect_vec($ctx.admit_iter(&mut $native.$owner, "catia_native_flattened_parent_visits").map_err(CodecError::from)?
                .flat_map(|parent| std::mem::take(&mut parent.$children)), "catia_native_flattened_arena")
    };
    (@flattened_prepare $ctx:ident, $native:ident, $field:ident, $owner:ident, $children:ident) => {
        let $field = define_catia_arenas!(
            @flattened_collect $ctx, $native, $field, $owner, $children
        )?;
    };
    (@prepare $ctx:ident, catalogs, $native:ident, $kind:ident, $binding:ident) => {
        let $binding = $ctx.try_collect_vec(
            $native.catalogs.iter().map(|catalog| CatiaCatalogWire::header_charged($ctx, catalog)),
            "catia_native_catalog_headers",
        )?;
    };
    (@prepare $ctx:ident, $field:ident, $native:ident, $kind:ident, $binding:ident) => {};
    (@stored_value stored, $native:ident, catalogs, $binding:ident) => {
        $binding
    };
    (@stored_value stored, $native:ident, $field:ident, $binding:ident) => {
        $native.$field
    };
    (@flattened_value $owner:ident, $children:ident, $field:ident) => {
        $field
    };
    (@family $kind:ident, $field:ident) => {
        CatiaFamilyRow {
            arena: stringify!($field),
            exactness: (),
            phase: Phase::ArenaOnly,
            emit: |ctx, projection, row, namespace| {
                namespace.set_arena(ctx, row.arena, &projection.$field)
            },
            len: |projection| projection.$field.len(),
            counts_toward_emptiness: true,
        }
    };
    (@flattened_family $owner:ident, $children:ident, $field:ident) => {
        define_catia_arenas!(@family flattened, $field)
    };
    (@default stored) => {
        Vec::new()
    };
}

define_catia_arenas! {
    alias_rows: CatiaAliasRow {
        /// Exact outer alias-row cores in source order.
        pub(crate) stored;
    },
    catalog_entries: CatiaCatalogEntry {
        => catalogs.entries;
    },
    catalogs: CatiaCatalog {
        /// Framed source-schema name catalogs.
        pub(crate) stored;
    },
    consolidated_circles: CatiaConsolidatedCircle {
        /// Exact consolidated arc-length circle supports.
        pub(crate) stored;
    },
    consolidated_class61_records: CatiaConsolidatedClass61Record {
        /// Complete consolidated class-`0x61` records.
        pub(crate) stored;
    },
    consolidated_class5b5c_records: CatiaConsolidatedClass5b5cRecord {
        /// Complete source-local consolidated class-`0x5b`/`0x5c` records.
        pub(crate) stored;
    },
    consolidated_cone_faces: CatiaConsolidatedConeFace {
        /// Complete consolidated cone-face chart descriptors.
        pub(crate) stored;
    },
    consolidated_cones: CatiaConsolidatedCone {
        /// Exact consolidated cone charts.
        pub(crate) stored;
    },
    consolidated_cylinders: CatiaConsolidatedCylinder {
        /// Exact consolidated cylinder charts.
        pub(crate) stored;
    },
    consolidated_embedded_cylinders: CatiaConsolidatedEmbeddedCylinder {
        /// Exact cylinder charts embedded in type-3 consolidated groups.
        pub(crate) stored;
    },
    consolidated_edge_nodes: CatiaConsolidatedEdgeNode {
        /// Structurally complete consolidated edge nodes.
        pub(crate) stored;
    },
    consolidated_edge_runs: CatiaConsolidatedEdgeRun {
        /// Complete consolidated historical edge runs.
        pub(crate) stored;
    },
    consolidated_groups: CatiaConsolidatedGroup {
        /// Typed consolidated class-`0x60` group openers.
        pub(crate) stored;
    },
    consolidated_line_profiles: CatiaConsolidatedLineProfile {
        /// Exact consolidated B-family metric line profiles.
        pub(crate) stored;
    },
    consolidated_owner_packets: CatiaConsolidatedOwnerPacket {
        /// Exact consolidated owner packets and their allocation links.
        pub(crate) stored;
    },
    consolidated_parameter_points: CatiaConsolidatedParameterPoint {
        /// Exact consolidated parameter-space records.
        pub(crate) stored;
    },
    consolidated_plane_carriers: CatiaConsolidatedPlaneCarrier {
        /// Structurally complete consolidated class-`0x27` plane carriers.
        pub(crate) stored;
    },
    consolidated_pcurves: CatiaConsolidatedPcurve {
        /// Consolidated pcurve jets retained before support resolution.
        pub(crate) stored;
    },
    consolidated_reference_lists: CatiaConsolidatedReferenceList {
        /// Exact consolidated persistent-reference lists.
        pub(crate) stored;
    },
    consolidated_revolutions: CatiaConsolidatedRevolution {
        /// Consolidated revolution carriers retained before profile resolution.
        pub(crate) stored;
    },
    consolidated_spheres: CatiaConsolidatedSphere {
        /// Exact consolidated sphere charts.
        pub(crate) stored;
    },
    consolidated_tori: CatiaConsolidatedTorus {
        /// Exact consolidated torus charts.
        pub(crate) stored;
    },
    consolidated_vertex_identities: CatiaConsolidatedVertexIdentity {
        /// Scoped endpoint identities and their consolidated edge incidence.
        pub(crate) stored;
    },
    design_objects: CatiaDesignObject {
        /// Design objects grouped by their serialized owner entity identity.
        pub(crate) stored;
    },
    entity_records: CatiaEntityRecord {
        /// Exact `7C05` entity-table records paired with object records.
        pub(crate) stored;
    },
    external_references: CatiaExternalReference {
        /// External CATIA document references in source order.
        pub(crate) stored;
    },
    finjpl_segments: CatiaFinjplSegment {
        /// Complete bounded outer FINJPL segments.
        pub(crate) stored;
    },
    legacy_entity_runs: CatiaLegacyEntityRun {
        /// Monotone entity identities in pre-`7C05` design streams.
        pub(crate) stored;
    },
    object_graph_records: CatiaObjectRecord {
        => object_graphs.records;
    },
    object_graphs: CatiaObjectGraph {
        /// Outer ownership graphs.
        pub(crate) stored;
    },
    preview_images: CatiaPreviewImage {
        /// Exact JPEG previews extracted from summary-information records.
        pub(crate) stored;
    },
    reference_signature_cohorts: CatiaReferenceSignatureCohort {
        /// Source-ordered descriptor cohorts grouped by exact reference pair.
        pub(crate) stored;
    },
    schema_configuration_row_chains: CatiaSchemaConfigurationRowChain {
        /// Complete schema-configuration-row successor chains.
        pub(crate) stored;
    },
    value_blocks: CatiaValueBlock {
        /// Framed value blocks adjacent to source-schema catalogs.
        pub(crate) stored;
    },
    value_schema_selections: CatiaValueSchemaSelection {
        => value_blocks.schema_selections;
    },
    zero_entity_edge_strides: CatiaZeroEntityEdgeStride {
        /// Zero-entity edge-stride allocation tuples.
        pub(crate) stored;
    },
    zero_entity_oriented_use_pairs: CatiaZeroEntityOrientedUsePair {
        /// Zero-entity side-pair headers and positional oriented uses.
        pub(crate) stored;
    },
    zero_entity_ownership_roots: CatiaZeroEntityOwnershipRoot {
        /// Complete zero-entity face-roster, shell, and body roots.
        pub(crate) stored;
    },
    zero_entity_endpoint_pair_candidates: CatiaZeroEntityEndpointPairCandidate {
        /// Zero-entity endpoint pairs established by radial support occurrences.
        pub(crate) stored;
    },
    zero_entity_records: CatiaZeroEntityRecord {
        /// Complete zero-entity framed-record identity namespace.
        pub(crate) stored;
    },
    zero_entity_support_runs: CatiaZeroEntitySupportRun {
        /// Zero-entity surface carriers and their face-local support tapes.
        pub(crate) stored;
    },
    zero_entity_endpoint_locus_candidates: CatiaZeroEntityEndpointLocusCandidate {
        /// Geometric endpoint loci established by complete endpoint-pair endpoint cliques.
        pub(crate) stored;
    },
    zero_entity_vertex_incidences: CatiaZeroEntityVertexIncidence {
        /// Zero-entity counted vertex-incidence records.
        pub(crate) stored;
    },
}
const CATIA_CATALOGUE: Catalogue<
    'static,
    CatiaArenaProjection,
    (),
    cadmpeg_ir::NativeNamespace,
    (),
> = Catalogue::new(CATIA_FAMILIES);

fn store_projection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    projection: &CatiaArenaProjection,
    namespace: &mut cadmpeg_ir::NativeNamespace,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    CATIA_CATALOGUE.emit_all(ctx, projection, namespace)?;
    Ok(())
}

fn legacy_role_selector(
    ctx: &DecodeContext<'_>,
    role: legacy_entity::LegacyRoleSelector,
) -> Result<CatiaLegacyRoleSelector, CodecError> {
    Ok(CatiaLegacyRoleSelector {
        byte_offset: u64_from_index(role.offset),
        entity_id: role.entity_id,
        name: match role.name {
            legacy_entity::LegacyRoleName::Literal(name) => legacy_entity::LegacyRoleName::Literal(
                ctx.copy_retained_text(&name, "catia_native_legacy_role_name")?,
            ),
            legacy_entity::LegacyRoleName::Selector(selector) => {
                legacy_entity::LegacyRoleName::Selector(selector)
            }
        },
        encoding: match role.encoding {
            legacy_entity::LegacyRoleSelectorEncoding::FixedU32 => {
                CatiaLegacyRoleSelectorEncoding::FixedU32
            }
            legacy_entity::LegacyRoleSelectorEncoding::Paged => {
                CatiaLegacyRoleSelectorEncoding::Paged
            }
        },
        selector: role.selector,
        field_code: role.field_code,
    })
}

fn legacy_entity_runs(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<CatiaLegacyEntityRun>, CodecError> {
    let (runs, _storage) = ctx.with_scoped_storage("catia_native_legacy_run_scratch", || {
        legacy_entity::parse_runs(ctx, bytes)
    })?;
    ctx.try_collect_vec(
        runs
            .into_iter()
            .enumerate()
            .map(|(index, run)| -> Result<_, CodecError> {
                let id = ctx.format_retained(
                    format_args!("catia:legacy:entity-run#{index:08}"),
                    "catia_native_legacy_run_id",
                )?;
                let byte_offset = run.first_identity.offset;
                let identities = ctx.collect_vec(
                    run.identities(ctx)?
                        .map(|identity| CatiaLegacyEntityIdentity {
                            byte_offset: u64_from_index(identity.offset),
                            entity_id: identity.entity_id,
                            lead: identity.lead,
                        }),
                    "catia_native_legacy_identities",
                )?;
                let schema_program = run
                    .schema_program
                    .map(|program| -> Result<_, CodecError> {
                        Ok(CatiaLegacySchemaProgram {
                            byte_offset: u64_from_index(program.offset),
                            boundary_byte_offset: u64_from_index(program.boundary_offset),
                            boundary: match program.boundary {
                                legacy_entity::LegacySchemaProgramBoundary::VendorFooter => {
                                    CatiaLegacySchemaProgramBoundary::VendorFooter
                                }
                                legacy_entity::LegacySchemaProgramBoundary::StreamDirectory => {
                                    CatiaLegacySchemaProgramBoundary::StreamDirectory
                                }
                            },
                            data: ctx.copy_slice(&program.bytes, "catia_native_legacy_schema_data")?,
                            identifiers: ctx.try_collect_vec(
                                program.identifiers.into_iter().map(|identifier| {
                                    Ok::<_, CodecError>(CatiaLegacySchemaIdentifier {
                                        byte_offset: u64_from_index(identifier.offset),
                                        value: ctx.copy_retained_text(&identifier.value, "catia_native_legacy_identifier_value")?,
                                    })
                                }),
                                "catia_native_legacy_schema_identifiers",
                            )?,
                        })
                    })
                    .transpose()?;
                let role_selectors = ctx.try_collect_vec(
                    run.role_selectors.into_iter().map(|role| legacy_role_selector(ctx, role)),
                    "catia_native_legacy_roles",
                )?;
                let text_fields = ctx.try_collect_vec(
                    run.text_fields
                        .into_iter()
                        .map(|field| -> Result<_, CodecError> { Ok(CatiaLegacyTextField {
                            byte_offset: u64_from_index(field.offset),
                            entity_id: field.entity_id,
                            encoding: match field.encoding {
                                legacy_entity::LegacyTextEncoding::U8InclusiveLength => {
                                    CatiaLegacyTextEncoding::U8InclusiveLength
                                }
                                legacy_entity::LegacyTextEncoding::ZeroU32Length => {
                                    CatiaLegacyTextEncoding::ZeroU32Length
                                }
                                legacy_entity::LegacyTextEncoding::U8InclusiveLengthE3RoleTail => {
                                    CatiaLegacyTextEncoding::U8InclusiveLengthE3RoleTail
                                }
                            },
                            role: field.role.map(|role| legacy_role_selector(ctx, role)).transpose()?,
                            value: ctx.copy_retained_text(&field.value, "catia_native_legacy_text_value")?,
                        }) }),
                    "catia_native_legacy_text_fields",
                )?;
                let relations = ctx.try_collect_vec(
                    run.relations
                        .into_iter()
                        .map(|relation| -> Result<_, CodecError> {
                            let ((inputs, output, result_type), _signature_storage) = ctx.with_scoped_storage("catia_native_legacy_signature_scratch", || relation.signature.into_parts(ctx))?;
                            let parameter = |parameter: legacy_entity::LegacyRelationParameter| -> Result<_, CodecError> {
                                Ok(CatiaLegacyRelationParameter {
                                    parameter: ctx.copy_retained_text(&parameter.parameter, "catia_native_legacy_relation_parameter")?,
                                    value_type: ctx.copy_retained_text(&parameter.value_type, "catia_native_legacy_relation_value_type")?,
                                })
                            };
                            Ok(CatiaLegacyRelation {
                                entity_id: relation.entity_id,
                                body_selector: relation.body_selector,
                                parameter_selector: relation.parameter_selector,
                                parameter_entity_id: relation.parameter_entity_id,
                                expression_offset: u64_from_index(relation.expression_offset),
                                expression: ctx.copy_retained_text(&relation.expression, "catia_native_legacy_relation_expression")?,
                                signature_offset: u64_from_index(relation.signature_offset),
                                type_signature: ctx.copy_retained_text(&relation.type_signature, "catia_native_legacy_type_signature")?,
                                inputs: ctx.try_collect_vec(
                                    inputs.into_iter().map(parameter),
                                    "catia_native_legacy_relation_inputs",
                                )?,
                                output: output.map(parameter).transpose()?,
                                result_type: ctx.copy_retained_text(&result_type, "catia_native_legacy_result_type")?,
                            })
                        }),
                    "catia_native_legacy_relations",
                )?;
                let scalar_values = ctx.try_collect_vec(
                    run.scalar_values
                        .into_iter()
                        .map(|value| -> Result<_, CodecError> {
                            Ok(CatiaLegacyScalarValue {
                                id: ctx.format_retained(
                                    format_args!(
                                        "catia:legacy:scalar#{index:08}-{:016}",
                                        value.offset
                                    ),
                                    "catia_native_legacy_scalar_id",
                                )?,
                                byte_offset: u64_from_index(value.offset),
                                entity_id: value.entity_id,
                                encoding: match value.encoding {
                                    legacy_entity::LegacyScalarEncoding::Named84 => {
                                        CatiaLegacyScalarEncoding::Named84
                                    }
                                    legacy_entity::LegacyScalarEncoding::Standalone85 => {
                                        CatiaLegacyScalarEncoding::Standalone85
                                    }
                                },
                                name_field: value.name_offset.map(u64_from_index),
                                name: value.name.as_ref().map(|name| ctx.copy_retained_text(name, "catia_native_legacy_value_name")).transpose()?,
                                evaluation: match value.evaluation {
                                    legacy_entity::LegacyScalarEvaluation::Value(bits) => {
                                        CatiaLegacyScalarEvaluation::Value { bits }
                                    }
                                    legacy_entity::LegacyScalarEvaluation::Unset => {
                                        CatiaLegacyScalarEvaluation::Unset
                                    }
                                },
                            })
                        }),
                    "catia_native_legacy_scalars",
                )?;
                let string_values = ctx.try_collect_vec(
                    run.string_values
                        .into_iter()
                        .map(|value| -> Result<_, CodecError> {
                            Ok(CatiaLegacyStringValue {
                                id: ctx.format_retained(
                                    format_args!(
                                        "catia:legacy:string#{index:08}-{:016}",
                                        value.offset
                                    ),
                                    "catia_native_legacy_string_id",
                                )?,
                                byte_offset: u64_from_index(value.offset),
                                entity_id: value.entity_id,
                                name_field: value.name_offset.map(u64_from_index),
                                name: value.name.as_ref().map(|name| ctx.copy_retained_text(name, "catia_native_legacy_value_name")).transpose()?,
                                value: ctx.copy_retained_text(&value.value, "catia_native_legacy_string_text")?,
                            })
                        }),
                    "catia_native_legacy_strings",
                )?;
                let integer_values = ctx.try_collect_vec(
                    run.integer_values
                        .into_iter()
                        .map(|value| -> Result<_, CodecError> {
                            Ok(CatiaLegacyIntegerValue {
                                id: ctx.format_retained(
                                    format_args!(
                                        "catia:legacy:integer#{index:08}-{:016}",
                                        value.offset
                                    ),
                                    "catia_native_legacy_integer_id",
                                )?,
                                byte_offset: u64_from_index(value.offset),
                                entity_id: value.entity_id,
                                encoding: match value.encoding {
                                    legacy_entity::LegacyIntegerEncoding::Inline => {
                                        CatiaLegacyIntegerEncoding::Inline
                                    }
                                    legacy_entity::LegacyIntegerEncoding::WideI32 => {
                                        CatiaLegacyIntegerEncoding::WideI32
                                    }
                                },
                                name_field: value.name_offset.map(u64_from_index),
                                name: value.name.as_ref().map(|name| ctx.copy_retained_text(name, "catia_native_legacy_value_name")).transpose()?,
                                value: value.value,
                            })
                        }),
                    "catia_native_legacy_integers",
                )?;
                let row = CatiaLegacyEntityRun {
                    id,
                    byte_offset: u64_from_index(byte_offset),
                    byte_len: u64_from_index(run.catalog_offset - byte_offset),
                    catalog_offset: u64_from_index(run.catalog_offset),
                    schema_program,
                    outer_container: None,
                    identities,
                    role_selectors,
                    text_fields,
                    schema_fields: ctx.try_collect_vec(
                        run.schema_fields
                            .into_iter()
                            .map(|field| -> Result<_, CodecError> { Ok(CatiaLegacySchemaField {
                                byte_offset: u64_from_index(field.offset),
                                entity_id: field.entity_id,
                                role_byte_offset: u64_from_index(field.role_offset),
                                boundary_role_byte_offset: u64_from_index(
                                    field.boundary_role_offset,
                                ),
                                field_code: field.field_code,
                                payload: ctx.copy_slice(&field.payload, "catia_native_legacy_schema_payload")?,
                            }) }),
                        "catia_native_legacy_schema_fields",
                    )?,
                    relations,
                    synchronous_states: ctx.collect_vec(
                        run.synchronous_states.into_iter().map(|state| {
                            CatiaLegacyRelationSynchronousState {
                                role_byte_offset: u64_from_index(state.role_offset),
                                entity_id: state.entity_id,
                                selector: state.selector,
                                synchronous: state.synchronous,
                            }
                        }),
                        "catia_native_legacy_synchronous_states",
                    )?,
                    type_descriptors: ctx.try_collect_vec(
                        run.type_descriptors.into_iter().map(|descriptor| -> Result<_, CodecError> {
                            Ok(CatiaLegacyTypeDescriptor {
                                byte_offset: u64_from_index(descriptor.offset),
                                entity_id: descriptor.entity_id,
                                value: match descriptor.value {
                                    legacy_entity::LegacyTypeValue::Name(value) => {
                                        CatiaLegacyTypeValue::Name { value: ctx.copy_retained_text(&value, "catia_native_legacy_type_name")? }
                                    }
                                    legacy_entity::LegacyTypeValue::Selector(value) => {
                                        CatiaLegacyTypeValue::Selector { value }
                                    }
                                },
                            })
                        }),
                        "catia_native_legacy_type_descriptors",
                    )?,
                    scalar_values,
                    string_values,
                    integer_values,
                };
                Ok(row)
            }),
        "catia_native_legacy_runs",
    )
}

/// Keyed lookup of the name bound to a legacy evaluated value. The value's
/// evaluation role is the unique `0x17c4` role of its identity that ends six
/// bytes before the value; its name is the unique `0x1200` identifier field of
/// the same identity before that role. Both tables are built once per run.
pub(crate) struct LegacyEvaluatedValueNames<'run, 'ctx> {
    roles: HashMap<(u32, u64), Option<&'run CatiaLegacyRoleSelector>>,
    /// The two identifier fields with the lowest offsets for each identity.
    names: HashMap<
        u32,
        (
            &'run CatiaLegacyTextField,
            Option<&'run CatiaLegacyTextField>,
        ),
    >,
    _role_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
    _name_storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'run, 'ctx> LegacyEvaluatedValueNames<'run, 'ctx> {
    pub(crate) fn new(
        ctx: &'ctx DecodeContext<'_>,
        roles: &'run [CatiaLegacyRoleSelector],
        fields: &'run [CatiaLegacyTextField],
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "catia_native_legacy_value_name_index";
        let (roles, role_storage) = ctx.unique_index(
            ctx.admit_iter(roles, "catia_native_legacy_value_role_visits")?
                .filter_map(|role| {
                    (role.field_code == Some(0x17c4)).then_some(())?;
                    let value_offset = role.end_offset()?.checked_add(6)?;
                    Some(((role.entity_id, value_offset), role))
                }),
            OPERATION,
        )?;
        let mut name_storage = ctx.reserve_scoped(0, OPERATION)?;
        let mut names =
            HashMap::<u32, (&CatiaLegacyTextField, Option<&CatiaLegacyTextField>)>::new();
        for field in ctx.admit_iter(fields, "catia_native_legacy_value_name_visits")? {
            if field
                .role
                .as_ref()
                .is_none_or(|role| role.field_code != Some(0x1200))
            {
                continue;
            }
            let mut characters = field.value.chars();
            let valid_first = ctx
                .next_charged(&mut characters, OPERATION)?
                .is_some_and(|character| character == '_' || character.is_alphabetic());
            if !valid_first
                || !ctx.all_by(
                    characters,
                    |character| Ok(character == '_' || character.is_alphanumeric()),
                    OPERATION,
                )?
            {
                continue;
            }
            name_storage.with_storage(|| -> Result<(), CodecError> {
                match ctx.entry_hash_map(&mut names, field.entity_id, OPERATION)? {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert((field, None));
                    }
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        let (first, second) = entry.get_mut();
                        if field.byte_offset < first.byte_offset {
                            *second = Some(*first);
                            *first = field;
                        } else if second.is_none_or(|second| field.byte_offset < second.byte_offset)
                        {
                            *second = Some(field);
                        }
                    }
                }
                Ok(())
            })?;
        }
        Ok(Self {
            roles,
            names,
            _role_storage: role_storage,
            _name_storage: name_storage,
        })
    }

    /// The unique name field bound to the evaluated value, if any.
    pub(crate) fn name(
        &self,
        ctx: &DecodeContext<'_>,
        entity_id: u32,
        value_offset: u64,
    ) -> Result<Option<&'run CatiaLegacyTextField>, CodecError> {
        const OPERATION: &str = "catia_native_legacy_value_name_lookup";
        let Some(Some(role)) = ctx
            .get_hash_map(&self.roles, &(entity_id, value_offset), OPERATION)?
            .copied()
        else {
            return Ok(None);
        };
        let Some((first, second)) = ctx
            .get_hash_map(&self.names, &entity_id, OPERATION)?
            .copied()
        else {
            return Ok(None);
        };
        Ok((first.byte_offset < role.byte_offset
            && second.is_none_or(|second| second.byte_offset >= role.byte_offset))
        .then_some(first))
    }
}

fn consolidated_class61_records(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedClass61Record>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia_native_class61_scratch")?;
    let counted = scratch.with_storage(|| {
        crate::families::b2::records::b2_counted_61_from_records(ctx, bytes, records)
    })?;
    let long = scratch.with_storage(|| {
        crate::families::b2::records::b2_long_61_from_records(ctx, bytes, records)
    })?;
    let mut class61_records = Vec::new();
    for record in ctx.admit_iter(&counted, "catia_native_counted_class61_visits")? {
        let payload = CatiaConsolidatedClass61Payload::Counted {
            references: ctx.copy_slice(&record.references, "catia_native_class61_references")?,
            tail: ctx.copy_slice(&record.tail, "catia_native_class61_tail")?,
        };
        scratch.with_storage(|| {
            ctx.push_vec(
                &mut class61_records,
                (record.pos, record.header_token, payload),
                "catia_native_class61_order",
            )
        })?;
    }
    for record in ctx.admit_iter(&long, "catia_native_long_class61_visits")? {
        let payload = CatiaConsolidatedClass61Payload::Long {
            prefix: record.prefix,
            members: ctx.copy_slice(&record.members, "catia_native_class61_members")?,
            references: record.references,
            scalar: record.scalar,
        };
        scratch.with_storage(|| {
            ctx.push_vec(
                &mut class61_records,
                (record.pos, record.header_token, payload),
                "catia_native_class61_order",
            )
        })?;
    }
    ctx.stable_sort_by(
        &mut class61_records,
        |value| &value.0,
        Ord::cmp,
        "catia_native_class61_sort",
    )?;
    ctx.try_collect_vec(
        class61_records.into_iter().enumerate().map(
            |(index, (pos, header_token, payload))| -> Result<_, CodecError> {
                Ok(CatiaConsolidatedClass61Record {
                    id: ctx.format_retained(
                        format_args!("catia:consolidated:class61-record#{index:00}"),
                        "catia_native_class61_id",
                    )?,
                    byte_offset: u64_from_index(pos),
                    header_token,
                    payload,
                })
            },
        ),
        "catia_native_class61_records",
    )
}

fn consolidated_class5b5c_records(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedClass5b5cRecord>, CodecError> {
    let (mut control_records, _storage) = ctx
        .with_scoped_storage("catia_native_class5b5c_scratch", || {
            crate::families::b2::records::b2_class5b5c_records_from_records(ctx, bytes, records)
        })?;
    ctx.stable_sort_by_key(
        &mut control_records,
        |value| (value.source_index, value.source_offset),
        Ord::cmp,
        "catia_native_class5b5c_sort",
    )?;
    ctx.try_collect_vec(
        control_records
            .into_iter()
            .enumerate()
            .map(|(index, record)| -> Result<_, CodecError> {
                Ok(CatiaConsolidatedClass5b5cRecord {
                    id: ctx.format_retained(
                        format_args!("catia:consolidated:class5b5c-record#{index:00}"),
                        "catia_native_class5b5c_id",
                    )?,
                    frame: crate::wire::records::ConsolidatedRawFrame::new(
                        u64_from_index(record.frame.pos),
                        record.frame.width(),
                        record.frame.flag,
                        record.frame.header_token(),
                        ctx.copy_slice(&record.frame.payload, "catia_native_class5b5c_payload")?,
                    )
                    .map_err(CodecError::malformed)?,
                    source_index: u64_from_index(record.source_index),
                    source_offset: u64_from_index(record.source_offset),
                    class: record.class,
                })
            }),
        "catia_native_class5b5c_records",
    )
}

#[cfg(test)]
mod consolidated_class_record_limit_tests {
    use super::{consolidated_class5b5c_records, consolidated_class61_records};
    use cadmpeg_core::CodecError;

    #[test]
    fn native_class61_order_output_and_id_refuse_limits() {
        let mut bytes = crate::test_support::test_b2::b2_counted_61_stream();
        bytes.extend_from_slice(&crate::test_support::test_b2::b2_long_61_stream());
        let records = crate::wire::records::consolidated_records(&bytes);
        for operation in ["catia_native_class61_order", "catia_native_class61_records"] {
            let limited = cadmpeg_test_support::refusal::resource_limit_at(
                cadmpeg_core::decode::ResourceDimension::CollectionItems,
                operation,
                |cap| {
                    crate::test_support::with_collection_limit(cap, |ctx| {
                        consolidated_class61_records(ctx, &bytes, &records)
                    })
                },
            );
            assert!(
                matches!(limited, CodecError::ResourceLimit(error) if error.operation == operation)
            );
        }
        let limited =
            crate::test_support::with_retained_refusal(&[], "catia_native_class61_id", |ctx| {
                consolidated_class61_records(ctx, &bytes, &records)
            });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_class61_id"));
    }

    #[test]
    fn native_class5b5c_output_and_id_refuse_limits() {
        let bytes = crate::test_support::test_b2::b2_class5b5c_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let limited = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "catia_native_class5b5c_records",
            |cap| {
                crate::test_support::with_collection_limit(cap, |ctx| {
                    consolidated_class5b5c_records(ctx, &bytes, &records)
                })
            },
        );
        assert!(
            matches!(limited, CodecError::ResourceLimit(error) if error.operation == "catia_native_class5b5c_records")
        );
        let limited =
            crate::test_support::with_retained_refusal(&[], "catia_native_class5b5c_id", |ctx| {
                consolidated_class5b5c_records(ctx, &bytes, &records)
            });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_class5b5c_id"));
    }
}

#[cfg(test)]
mod consolidated_cone_face_limit_tests {
    use super::{consolidated_cone_faces, projection};
    use cadmpeg_core::CodecError;

    #[test]
    fn native_cone_face_output_and_id_refuse_limits() {
        let bytes = crate::test_support::test_b2::b2_cone_face_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let limited = crate::test_support::with_collection_limit(17, |ctx| {
            consolidated_cone_faces(ctx, &bytes, &records, &[])
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_cone_faces"));
        let limited =
            crate::test_support::with_retained_refusal(&[], "catia_native_cone_face_id", |ctx| {
                consolidated_cone_faces(ctx, &bytes, &records, &[])
            });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_cone_face_id"));
    }

    #[test]
    fn native_cone_face_parameter_links_refuse_collection_limits() {
        let bytes = crate::test_support::test_b2::b2_cone_face_parameter_point_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let points = crate::test_support::with_service_context(|ctx| {
            projection::consolidated_parameter_points(ctx, &bytes, &records)
        })
        .expect("service decode");
        assert_eq!(points.len(), 4);
        let mut refused = std::collections::BTreeSet::new();
        for limit in 0..64 {
            let limited = crate::test_support::with_collection_limit(limit, |ctx| {
                consolidated_cone_faces(ctx, &bytes, &records, &points)
            });
            match limited {
                Err(CodecError::ResourceLimit(error)) => {
                    refused.insert(error.operation);
                }
                Ok(_) => break,
                Err(error) => panic!("unexpected cone-face error: {error}"),
            }
        }
        for operation in [
            "catia_native_cone_face_point_index",
            "catia_native_cone_face_class18_ends",
            "catia_native_cone_face_positions",
            "catia_native_cone_face_parameter_points",
        ] {
            assert!(refused.contains(operation), "no refusal at {operation}");
        }
        let limited = crate::test_support::with_retained_refusal(
            &[],
            "catia_native_cone_face_parameter_id",
            |ctx| consolidated_cone_faces(ctx, &bytes, &records, &points),
        );
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_cone_face_parameter_id"));
    }
}

fn consolidated_cone_faces(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    parameter_points: &[CatiaConsolidatedParameterPoint],
) -> Result<Vec<CatiaConsolidatedConeFace>, CodecError> {
    // The point and class-18 indexes and each face's positions are dropped
    // once the faces are built.
    let mut scratch = ctx.reserve_scoped(0, "catia_native_cone_face_scratch")?;
    let mut point_ids = HashMap::new();
    for point in ctx.admit_iter(
        parameter_points,
        "catia_native_cone_face_parameter_point_visits",
    )? {
        scratch.with_storage(|| {
            ctx.insert_hash_map(
                &mut point_ids,
                point.byte_offset,
                point.id.as_str(),
                "catia_native_cone_face_point_index",
            )
        })?;
    }
    let mut class18_ends = HashMap::new();
    for record in ctx.admit_iter(records, "catia_native_cone_face_record_visits")? {
        if record.family() != crate::wire::records::ConsolidatedFamily::B || record.class() != 0x18
        {
            continue;
        }
        if let Some(range) = record.range() {
            scratch.with_storage(|| {
                ctx.insert_hash_map(
                    &mut class18_ends,
                    range.start,
                    range.end,
                    "catia_native_cone_face_class18_ends",
                )
            })?;
        }
    }
    let faces = crate::families::b2::records::b2_cone_faces(ctx, bytes)?;
    ctx.try_collect_vec(
        faces
            .into_iter()
            .enumerate()
            .map(|(index, face)| -> Result<_, CodecError> {
                const LOOKUP: &str = "catia_native_cone_face_lookups";
                // The class-18 records chained after the face; each link moves forward.
                let mut face_scratch =
                    ctx.reserve_scoped(0, "catia_native_cone_face_local_scratch")?;
                let mut positions = Vec::new();
                let mut next = face.end;
                while let Some(&end) = ctx.get_hash_map(&class18_ends, &next, LOOKUP)? {
                    if end <= next {
                        break;
                    }
                    face_scratch.with_storage(|| {
                        ctx.push_vec(
                            &mut positions,
                            u64_from_index(next),
                            "catia_native_cone_face_positions",
                        )
                    })?;
                    next = end;
                }
                let mut point_names = Vec::new();
                let mut position_rows = positions.iter();
                while let Some(position) =
                    ctx.next_charged(&mut position_rows, "catia_native_cone_face_position_visits")?
                {
                    let Some(id) = ctx.get_hash_map(&point_ids, position, LOOKUP)? else {
                        point_names.clear();
                        break;
                    };
                    face_scratch.with_storage(|| {
                        ctx.push_vec(&mut point_names, *id, "catia_native_cone_face_point_names")
                    })?;
                }
                let bound_points = ctx.try_collect_vec(
                    point_names.iter().map(|id| {
                        ctx.copy_retained_text(id, "catia_native_cone_face_parameter_id")
                    }),
                    "catia_native_cone_face_parameter_points",
                )?;
                let byte_offset = u64_from_index(face.pos);
                let byte_len = u64_from_index(face.end - face.pos);
                Ok(CatiaConsolidatedConeFace {
                    id: ctx.format_retained(
                        format_args!("catia:consolidated:cone-face#{index:00}"),
                        "catia_native_cone_face_id",
                    )?,
                    byte_offset,
                    byte_len,
                    program: face.program,
                    angular_scale: face.angular_scale,
                    half_angle: face.half_angle,
                    parameter_points: bound_points,
                })
            }),
        "catia_native_cone_faces",
    )
}

fn consolidated_cylinders(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedCylinder>, CodecError> {
    let mut cylinders = Vec::new();
    for (index, cylinder) in
        crate::families::b2::records::b2_cylinders_from_records(ctx, bytes, records)?.enumerate()
    {
        let cylinder = cylinder?;
        let payload = match cylinder.layout {
            crate::families::b2::records::B2CylinderLayout::RangeOrigin { stored_vector } => {
                CatiaConsolidatedCylinderPayload::RangeOrigin {
                    stored_vector,
                    axis: cylinder.frame.axis(),
                    reference_direction: cylinder.frame.reference(),
                    range_origin: cylinder.range_origin().unwrap_or_else(|| {
                        crate::families::b2::records::cylinder_range_origin(
                            cylinder.radius.get(),
                            cylinder.u_range.endpoints(),
                        )
                    }),
                }
            }
            crate::families::b2::records::B2CylinderLayout::Full52 => {
                CatiaConsolidatedCylinderPayload::Layout52 {
                    frame_token: cylinder.frame_token(),
                    axis: cylinder.frame.axis(),
                    reference_direction: cylinder.frame.reference(),
                }
            }
            crate::families::b2::records::B2CylinderLayout::Full5a { frame_token } => {
                CatiaConsolidatedCylinderPayload::Layout5a {
                    frame_token,
                    axis: cylinder.frame.axis(),
                    reference_direction: cylinder.frame.reference(),
                }
            }
        };
        let value = CatiaConsolidatedCylinder {
            id: ctx.format_retained(
                format_args!("catia:consolidated:cylinder#{index:00}"),
                "catia_native_cylinder_id",
            )?,
            byte_offset: u64_from_index(cylinder.pos),
            origin: cylinder.origin.coordinates().into(),
            radius: cylinder.radius,
            u_range: cylinder.u_range,
            v_range: cylinder.v_range,
            payload,
        };
        ctx.push_vec(&mut cylinders, value, "catia_native_cylinders")?;
    }
    Ok(cylinders)
}

fn consolidated_cylinder_groups(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<
    (
        Vec<CatiaConsolidatedGroup>,
        Vec<CatiaConsolidatedEmbeddedCylinder>,
    ),
    CodecError,
> {
    let mut groups = Vec::new();
    let mut cylinders = Vec::new();
    let mut embedded =
        crate::families::b2::records::b2_embedded_cylinders_from_records(ctx, bytes, records)?
            .peekable();
    for group in crate::families::b2::records::b2_groups_from_records(ctx, bytes, records)? {
        let group_pos = group.pos;
        let group = CatiaConsolidatedGroup {
            id: ctx.format_retained(
                format_args!("catia:consolidated:group#{:00}", groups.len()),
                "catia_native_group_id",
            )?,
            byte_offset: u64_from_index(group.pos),
            group_type: group.group_type,
        };
        loop {
            let belongs_to_group = match embedded.peek() {
                Some(Ok(entry)) => entry.wrapper_pos == group_pos,
                Some(Err(_)) => true,
                None => false,
            };
            if !belongs_to_group {
                break;
            }
            let Some(entry) = embedded.next() else { break };
            let entry = entry?;
            let value = CatiaConsolidatedEmbeddedCylinder {
                id: ctx.format_retained(
                    format_args!(
                        "catia:consolidated:embedded-cylinder#{:00}",
                        cylinders.len()
                    ),
                    "catia_native_embedded_cylinder_id",
                )?,
                byte_offset: u64_from_index(entry.pos),
                group: ctx.copy_retained_text(&group.id, "catia_native_embedded_cylinder_group")?,
                object_id: entry.object_id,
                origin: entry.cylinder.origin.coordinates().into(),
                radius: entry.cylinder.radius,
                u_range: entry.cylinder.u_range,
                v_range: entry.cylinder.v_range,
                frame_token: entry.cylinder.frame_token(),
                axis: entry.cylinder.frame.axis(),
                reference_direction: entry.cylinder.frame.reference(),
            };
            ctx.push_vec(&mut cylinders, value, "catia_native_embedded_cylinders")?;
        }
        ctx.push_vec(&mut groups, group, "catia_native_cylinder_groups")?;
    }
    Ok((groups, cylinders))
}

#[cfg(test)]
mod consolidated_cylinder_limit_tests {
    use super::{consolidated_cylinder_groups, consolidated_cylinders};
    use cadmpeg_core::CodecError;

    #[test]
    fn native_standalone_cylinder_refuses_uncharged_output_and_id() {
        let bytes = crate::test_support::test_b2::b2_cylinder_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            consolidated_cylinders(ctx, &bytes, &records)
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_cylinders"));
        let limited = crate::test_support::with_retained_limit(0, |ctx| {
            consolidated_cylinders(ctx, &bytes, &records)
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_cylinder_id"));
        let cylinders = crate::test_support::with_service_context(|ctx| {
            consolidated_cylinders(ctx, &bytes, &records)
        })
        .expect("service decode");
        assert_eq!(cylinders.len(), 1);
    }

    #[test]
    fn native_embedded_cylinder_and_group_refuse_collection_limit() {
        let bytes = crate::test_support::test_b2::b2_embedded_cylinder_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            consolidated_cylinder_groups(ctx, &bytes, &records)
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_embedded_cylinders"));
        let limited = crate::test_support::with_collection_limit(1, |ctx| {
            consolidated_cylinder_groups(ctx, &bytes, &records)
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_cylinder_groups"));
        let (groups, cylinders) = crate::test_support::with_service_context(|ctx| {
            consolidated_cylinder_groups(ctx, &bytes, &records)
        })
        .expect("service decode");
        assert_eq!((groups.len(), cylinders.len()), (1, 1));
    }
}

#[cfg(test)]
mod consolidated_analytic_limit_tests {
    use super::projection::{
        self, consolidated_circles, consolidated_cones, consolidated_spheres, consolidated_tori,
    };
    use cadmpeg_core::CodecError;

    #[test]
    fn native_analytic_carriers_refuse_output_and_id_limits() {
        macro_rules! check {
            ($fixture:ident, $decode:ident, $output:literal, $id:literal) => {{
            let bytes = crate::test_support::test_b2::$fixture();
            let records = crate::wire::records::consolidated_records(&bytes);
            let limited = crate::test_support::with_collection_limit(0, |ctx| $decode(ctx, &bytes, &records));
            assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
                if error.operation == $output));
            let limited = crate::test_support::with_retained_limit(0, |ctx| $decode(ctx, &bytes, &records));
            assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
                if error.operation == $id));
            }};
        }
        check!(
            b2_circle_stream,
            consolidated_circles,
            "catia_native_circles",
            "catia_native_circle_id"
        );
        check!(
            b2_cone_stream,
            consolidated_cones,
            "catia_native_cones",
            "catia_native_cone_id"
        );
        check!(
            b2_sphere_stream,
            consolidated_spheres,
            "catia_native_spheres",
            "catia_native_sphere_id"
        );
        check!(
            b2_torus_stream,
            consolidated_tori,
            "catia_native_tori",
            "catia_native_torus_id"
        );
    }

    #[test]
    fn native_consolidated_circle_record_scan_propagates_work_refusal() {
        let bytes = crate::test_support::test_b2::b2_circle_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let service = crate::test_support::with_service_context(|ctx| {
            projection::consolidated_circles(ctx, &bytes, &records)
        })
        .expect("service context admits the consolidated circle");
        assert_eq!(service.len(), 1);

        let operation = "catia_b2_family_record_scan";
        let refused = crate::test_support::with_work_refusal(operation, |ctx| {
            let result = projection::consolidated_circles(ctx, &bytes, &records)
                .map(|circles| circles.len());
            if let Err(CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
            }
            result
        });
        assert!(matches!(
            refused,
            Err(CodecError::ResourceLimit(limit)) if limit.operation == operation
        ));
    }

    #[test]
    fn native_revolution_refuses_profile_map_output_and_id_limits() {
        let bytes = crate::test_support::test_b2::b2_resolved_revolution_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let limited = crate::test_support::with_collection_limit(2, |ctx| {
            projection::consolidated_revolutions(ctx, &bytes, &records, &[])
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_revolution_profile_index"));
        let limited = crate::test_support::with_collection_limit(3, |ctx| {
            projection::consolidated_revolutions(ctx, &bytes, &records, &[])
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_revolutions"));
        let limited = crate::test_support::with_retained_limit(0, |ctx| {
            projection::consolidated_revolutions(ctx, &bytes, &records, &[])
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_revolution_id"));
    }

    #[test]
    fn native_parameter_points_and_line_profiles_refuse_output_and_id_limits() {
        let bytes = crate::test_support::test_b2::b2_parameter_point_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            projection::consolidated_parameter_points(ctx, &bytes, &records)
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_parameter_points"));
        let limited = crate::test_support::with_retained_limit(0, |ctx| {
            projection::consolidated_parameter_points(ctx, &bytes, &records)
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_parameter_point_id"));
        let bytes = crate::test_support::test_b2::b2_line_profile_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            projection::consolidated_line_profiles(ctx, &bytes, &records)
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_line_profiles"));
        let limited = crate::test_support::with_retained_limit(0, |ctx| {
            projection::consolidated_line_profiles(ctx, &bytes, &records)
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_line_profile_id"));
    }

    #[test]
    fn native_reference_lists_refuse_output_and_id_limits() {
        let bytes = crate::test_support::test_b2::b2_reference_list_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let limited = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "catia_native_reference_lists",
            |cap| {
                crate::test_support::with_collection_limit(cap, |ctx| {
                    projection::consolidated_reference_lists(ctx, &bytes, &records)
                })
            },
        );
        assert!(
            matches!(limited, CodecError::ResourceLimit(error) if error.operation == "catia_native_reference_lists")
        );
        let limited = crate::test_support::with_retained_refusal(
            &[],
            "catia_native_reference_list_id",
            |ctx| projection::consolidated_reference_lists(ctx, &bytes, &records),
        );
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_reference_list_id"));
    }

    #[test]
    fn native_plane_carriers_refuse_output_and_id_limits() {
        let bytes = crate::test_support::test_b2::b2_plane_carrier_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let limited = crate::test_support::with_collection_limit(3, |ctx| {
            projection::consolidated_plane_carriers(ctx, &bytes, &records)
        });
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_plane_carriers"));
        let limited = crate::test_support::with_retained_refusal(
            &[],
            "catia_native_plane_carrier_id",
            |ctx| projection::consolidated_plane_carriers(ctx, &bytes, &records),
        );
        assert!(matches!(limited, Err(CodecError::ResourceLimit(error))
            if error.operation == "catia_native_plane_carrier_id"));
    }
}

fn consolidated_pcurves(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Result<Vec<CatiaConsolidatedPcurve>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "catia_native_pcurve_scratch")?;
    let a = scratch.with_storage(|| {
        crate::wire::records::family_pcurves_from_records(
            ctx,
            bytes,
            records,
            crate::wire::records::ConsolidatedFamily::A,
        )
    })?;
    let b = scratch.with_storage(|| {
        crate::wire::records::family_pcurves_from_records(
            ctx,
            bytes,
            records,
            crate::wire::records::ConsolidatedFamily::B,
        )
    })?;
    let mut pcurves = scratch.with_storage(|| {
        ctx.collect_vec(
            a.into_iter()
                .map(|pcurve| (pcurve, CatiaConsolidatedFamily::A))
                .chain(
                    b.into_iter()
                        .map(|pcurve| (pcurve, CatiaConsolidatedFamily::B)),
                ),
            "catia_native_pcurve_ordering",
        )
    })?;
    ctx.stable_sort_by(
        &mut pcurves,
        |value| &value.0.pos,
        Ord::cmp,
        "catia_native_pcurve_sort",
    )?;
    ctx.try_collect_vec(
        pcurves
            .into_iter()
            .enumerate()
            .map(|(index, (pcurve, family))| -> Result<_, CodecError> {
                let (knots, points, first_derivatives, second_derivatives) =
                    pcurve.native_lanes(ctx)?;
                Ok(CatiaConsolidatedPcurve {
                    id: ctx.format_retained(
                        format_args!("catia:consolidated:pcurve#{index:00}"),
                        "catia_native_pcurve_id",
                    )?,
                    byte_offset: u64_from_index(pcurve.pos),
                    family,
                    support_id: pcurve.support_id,
                    degree: crate::wire::records::ConsolidatedPcurve::DEGREE,
                    extrapolation_sites: pcurve.extrapolation_sites,
                    knots,
                    points,
                    first_derivatives,
                    second_derivatives,
                    range: pcurve.range,
                    tail: ctx.copy_slice(&pcurve.tail, "catia_native_pcurve_tail")?,
                })
            }),
        "catia_native_pcurves",
    )
}

#[cfg(test)]
mod consolidated_pcurve_limit_tests {
    #[test]
    fn native_pcurve_id_and_output_refuse_limits() {
        let bytes = crate::test_support::test_a5a8::a5_pcurve_stream();
        let records = crate::wire::records::consolidated_records(&bytes);
        let limited =
            crate::test_support::with_retained_refusal(&[], "catia_native_pcurve_id", |ctx| {
                super::consolidated_pcurves(ctx, &bytes, &records)
            });
        assert!(
            matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == "catia_native_pcurve_id")
        );
        let limited = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "catia_native_pcurves",
            |cap| {
                crate::test_support::with_collection_limit(cap, |ctx| {
                    super::consolidated_pcurves(ctx, &bytes, &records)
                })
            },
        );
        assert!(
            matches!(limited, cadmpeg_core::CodecError::ResourceLimit(error) if error.operation == "catia_native_pcurves")
        );
        let pcurves = crate::test_support::with_service_context(|ctx| {
            super::consolidated_pcurves(ctx, &bytes, &records)
        })
        .expect("service decode");
        assert_eq!(pcurves.len(), 1);
        assert_eq!(pcurves[0].points.len(), 2);
    }
}

fn zero_entity_support_runs(
    ctx: &DecodeContext<'_>,
    runs: Vec<crate::families::zero_entity::records::ZeroEntitySupportRun>,
    records: &[CatiaZeroEntityRecord],
) -> Result<Vec<CatiaZeroEntitySupportRun>, CodecError> {
    ctx.try_collect_vec(
        runs.into_iter()
            .enumerate()
            .map(|(index, run)| -> Result<_, CodecError> {
                let face = if let Some(face) = run.face {
                    let mut loop_terminals = Vec::new();
                    if let Some(&first) = face.allocations.first() {
                        ctx.reserve_vec(
                            &mut loop_terminals,
                            face.allocations.len() - 1,
                            "catia_native_zero_loop_terminals",
                        )?;
                        for allocation in ctx.admit_iter(
                            &face.allocations[1..],
                            "catia_native_zero_face_allocation_visits",
                        )? {
                            if let Some(terminal) = first.checked_sub(*allocation) {
                                loop_terminals.push(terminal);
                            }
                        }
                    }
                    let terminal_control = face.terminal_control.as_byte();
                    let loops = face.loops.unwrap_or_default();
                    let native_loops = ctx.try_collect_vec(
                        loops
                            .into_iter()
                            .map(|loop_record| -> Result<_, CodecError> {
                                // The typed records are kept only when every reference resolves.
                                let mut typed_records = Vec::new();
                                if ctx.all_by(
                                    &loop_record.typed_references,
                                    |ordinal| Ok(zero_entity_record(records, *ordinal).is_some()),
                                    "catia_native_zero_typed_reference_checks",
                                )? {
                                    ctx.reserve_vec(
                                        &mut typed_records,
                                        loop_record.typed_references.len(),
                                        "catia_native_zero_typed_records",
                                    )?;
                                    for ordinal in ctx.admit_iter(
                                        &loop_record.typed_references,
                                        "catia_native_zero_typed_reference_visits",
                                    )? {
                                        let Some(record) = zero_entity_record(records, *ordinal)
                                        else {
                                            continue;
                                        };
                                        typed_records.push(ctx.copy_retained_text(
                                            &record.id,
                                            "catia_native_zero_typed_record_id",
                                        )?);
                                    }
                                }
                                let member_ids = ctx.collect_vec(
                                    loop_record.members.member_ids(),
                                    "catia_native_zero_member_ids",
                                )?;
                                Ok(CatiaZeroEntityLoop {
                                    byte_offset: u64_from_index(loop_record.pos),
                                    record_ordinal: loop_record.record_ordinal,
                                    tag: loop_record.tag,
                                    member_ids,
                                    typed_references: ctx.copy_slice(
                                        &loop_record.typed_references,
                                        "catia_native_zero_loop_typed_references",
                                    )?,
                                    typed_records,
                                    support_record_ordinals: ctx.copy_slice(
                                        &loop_record.support_record_ordinals,
                                        "catia_native_zero_loop_support_ordinals",
                                    )?,
                                    terminal_id: loop_record.members.terminal_id(),
                                    gap: loop_record.members.gap(),
                                    loop_class: loop_record.loop_class.as_byte(),
                                    forward_senses: ctx.copy_slice(
                                        &loop_record.forward_senses,
                                        "catia_native_zero_loop_forward_senses",
                                    )?,
                                    oriented_model_endpoints: ctx.copy_slice(
                                        &loop_record.oriented_model_endpoints,
                                        "catia_native_zero_loop_oriented_endpoints",
                                    )?,
                                })
                            }),
                        "catia_native_zero_face_loops",
                    )?;
                    Some(CatiaZeroEntityFace {
                        byte_offset: u64_from_index(face.pos),
                        record_ordinal: face.record_ordinal,
                        tag: face.tag,
                        allocations: ctx
                            .copy_slice(&face.allocations, "catia_native_zero_face_allocations")?,
                        loop_terminals,
                        loops: native_loops,
                        terminal_control,
                    })
                } else {
                    None
                };
                let supports = ctx.collect_vec(
                    run.supports
                        .into_iter()
                        .map(|support| CatiaZeroEntitySupportOccurrence {
                            byte_offset: u64_from_index(support.pos),
                            record_ordinal: support.record_ordinal,
                            tag: support.tag,
                            face_local_slot: support.face_local_slot,
                            uv_endpoints: support.uv_endpoints,
                            pcurve: support.pcurve,
                            model_curve: support.model_curve,
                            model_curve_construction: support.model_curve_construction,
                            model_parameters: support.model_parameters,
                            model_midpoint: support.model_midpoint,
                            model_endpoints: support.model_endpoints,
                        }),
                    "catia_native_zero_support_occurrences",
                )?;
                Ok(CatiaZeroEntitySupportRun {
                    id: ctx.format_retained(
                        format_args!("catia:zero-entity:support-run#{index}"),
                        "catia_native_zero_support_run_id",
                    )?,
                    carrier_byte_offset: u64_from_index(run.carrier_pos),
                    carrier_record_ordinal: run.carrier_record_ordinal,
                    face,
                    supports,
                })
            }),
        "catia_native_zero_support_runs",
    )
}

fn zero_entity_endpoint_pair_id(
    ctx: &DecodeContext<'_>,
    index: usize,
) -> Result<String, CodecError> {
    ctx.format_retained(
        format_args!("catia:zero-entity:endpoint-pair-candidate#{index}"),
        "catia_native_zero_endpoint_pair_id",
    )
}

fn zero_entity_endpoint_pair_candidates(
    ctx: &DecodeContext<'_>,
    candidates: &[crate::families::zero_entity::topology::ZeroEntityEndpointPairCandidate],
) -> Result<Vec<CatiaZeroEntityEndpointPairCandidate>, CodecError> {
    let mut output = Vec::new();
    ctx.reserve_vec(
        &mut output,
        candidates.len(),
        "catia_native_zero_endpoint_pairs",
    )?;
    for (index, candidate) in ctx
        .admit_iter(
            candidates,
            "catia_native_zero_endpoint_pair_candidate_visits",
        )?
        .enumerate()
    {
        let [face_first, face_second] = candidate.face_record_ordinals.map(|ordinal| {
            ctx.format_retained(
                format_args!("catia:zero-entity:record#{ordinal}"),
                "catia_native_zero_face_record_id",
            )
        });
        let [support_first, support_second] = candidate.support_record_ordinals.map(|ordinal| {
            ctx.format_retained(
                format_args!("catia:zero-entity:record#{ordinal}"),
                "catia_native_zero_support_record_id",
            )
        });
        output.push(CatiaZeroEntityEndpointPairCandidate {
            id: zero_entity_endpoint_pair_id(ctx, index)?,
            face_records: [face_first?, face_second?],
            support_records: [support_first?, support_second?],
            model_endpoints: candidate.model_endpoints,
            model_midpoint: candidate.model_midpoint,
        });
    }
    Ok(output)
}

fn zero_entity_endpoint_locus_candidates(
    ctx: &DecodeContext<'_>,
    candidates: &[crate::families::zero_entity::topology::ZeroEntityEndpointLocusCandidate],
) -> Result<Vec<CatiaZeroEntityEndpointLocusCandidate>, CodecError> {
    let mut output = Vec::new();
    ctx.reserve_vec(
        &mut output,
        candidates.len(),
        "catia_native_zero_endpoint_loci",
    )?;
    for (index, candidate) in ctx
        .admit_iter(
            candidates,
            "catia_native_zero_endpoint_locus_candidate_visits",
        )?
        .enumerate()
    {
        let mut endpoints = Vec::new();
        ctx.reserve_vec(
            &mut endpoints,
            candidate.incident_endpoint_pair_endpoints.len(),
            "catia_native_zero_locus_incidence",
        )?;
        for &(pair, endpoint_index) in ctx.admit_iter(
            &candidate.incident_endpoint_pair_endpoints,
            "catia_native_zero_locus_incidence_visits",
        )? {
            endpoints.push(CatiaZeroEntityEndpointPairEndpoint {
                endpoint_pair: zero_entity_endpoint_pair_id(ctx, pair.ordinal())?,
                endpoint_index,
            });
        }
        output.push(CatiaZeroEntityEndpointLocusCandidate {
            id: ctx.format_retained(
                format_args!("catia:zero-entity:endpoint-locus-candidate#{index}"),
                "catia_native_zero_endpoint_locus_id",
            )?,
            incident_endpoint_pair_endpoints: endpoints
                .try_into()
                .map_err(CodecError::malformed)?,
            representative_point: candidate.representative_point,
            maximum_deviation: cadmpeg_ir::scalar::NonNegativeReal::new(
                candidate.maximum_deviation,
            )
            .ok_or_else(|| {
                CodecError::malformed("endpoint locus deviation must be finite and nonnegative")
            })?,
        });
    }
    Ok(output)
}

fn zero_entity_edge_strides(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    range: Range<usize>,
) -> Result<Vec<CatiaZeroEntityEdgeStride>, CodecError> {
    let (records, _storage) =
        ctx.with_scoped_storage("catia_native_zero_projection_scratch", || {
            crate::families::zero_entity::records::zero_entity_edge_strides_in_range(
                ctx, bytes, range,
            )
        })?;
    let mut output = Vec::new();
    ctx.reserve_vec(&mut output, records.len(), "catia_native_zero_edge_strides")?;
    for (index, record) in ctx
        .admit_iter(&records, "catia_native_zero_edge_stride_visits")?
        .enumerate()
    {
        output.push(CatiaZeroEntityEdgeStride {
            id: ctx.format_retained(
                format_args!("catia:zero-entity:edge-stride#{index}"),
                "catia_native_zero_edge_stride_id",
            )?,
            byte_offset: u64_from_index(record.pos),
            record_ordinal: record.record_ordinal,
            allocations: record.allocations,
            topology_refs: record.topology_refs(),
            surface_support_refs: record.surface_support_refs(),
        });
    }
    Ok(output)
}

fn zero_entity_oriented_use_pairs(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    range: Range<usize>,
) -> Result<Vec<CatiaZeroEntityOrientedUsePair>, CodecError> {
    use crate::families::zero_entity::records::ZeroEntityUseSlot;

    let (pairs, _storage) =
        ctx.with_scoped_storage("catia_native_zero_projection_scratch", || {
            crate::families::zero_entity::records::zero_entity_oriented_use_pairs_in_range(
                ctx, bytes, range,
            )
        })?;
    let mut output = Vec::new();
    ctx.reserve_vec(&mut output, pairs.len(), "catia_native_zero_oriented_pairs")?;
    for (index, pair) in ctx
        .admit_iter(&pairs, "catia_native_zero_oriented_pair_visits")?
        .enumerate()
    {
        output.push(CatiaZeroEntityOrientedUsePair {
            id: ctx.format_retained(
                format_args!("catia:zero-entity:oriented-use-pair#{index}"),
                "catia_native_zero_oriented_pair_id",
            )?,
            header_byte_offset: u64_from_index(pair.header_pos),
            header_record_ordinal: pair.header_record_ordinal,
            base_columns: pair.base_columns(),
            uses: [
                (ZeroEntityUseSlot::First, &pair.uses[0]),
                (ZeroEntityUseSlot::Second, &pair.uses[1]),
            ]
            .map(|(slot, use_)| CatiaZeroEntityOrientedUse {
                byte_offset: u64_from_index(use_.pos),
                record_ordinal: use_.record_ordinal,
                side: slot.side(),
                allocations: pair.allocations(slot),
            }),
        });
    }
    Ok(output)
}

fn zero_entity_ownership_roots(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    range: Range<usize>,
) -> Result<Vec<CatiaZeroEntityOwnershipRoot>, CodecError> {
    let (roots, _storage) =
        ctx.with_scoped_storage("catia_native_zero_projection_scratch", || {
            crate::families::zero_entity::records::zero_entity_ownership_roots_in_range(
                ctx, bytes, range,
            )
        })?;
    ctx.try_collect_vec(
        roots
            .into_iter()
            .enumerate()
            .map(|(index, root)| -> Result<_, CodecError> {
                let shell_record_ordinal = root.shell_record_ordinal()?;
                let body_record_ordinal = root.body_record_ordinal()?;
                Ok(CatiaZeroEntityOwnershipRoot {
                    id: ctx.format_retained(
                        format_args!("catia:zero-entity:ownership-root#{index}"),
                        "catia_native_zero_ownership_root_id",
                    )?,
                    face_roster_byte_offset: u64_from_index(root.face_roster_pos),
                    face_roster_record_ordinal: root.face_roster_record_ordinal,
                    face_slots: ctx.copy_slice(&root.face_slots, "catia_native_zero_face_slots")?,
                    shell_byte_offset: u64_from_index(root.shell_pos),
                    shell_record_ordinal,
                    body_byte_offset: u64_from_index(root.body_pos),
                    body_record_ordinal,
                })
            }),
        "catia_native_zero_ownership_roots",
    )
}

fn zero_entity_vertex_incidences(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    range: Range<usize>,
    records: &[CatiaZeroEntityRecord],
) -> Result<Vec<CatiaZeroEntityVertexIncidence>, CodecError> {
    let (incidences, _storage) =
        ctx.with_scoped_storage("catia_native_zero_projection_scratch", || {
            crate::families::zero_entity::records::zero_entity_vertex_incidences_in_range(
                ctx, bytes, range,
            )
        })?;
    let mut output = Vec::new();
    ctx.reserve_vec(
        &mut output,
        incidences.len(),
        "catia_native_zero_vertex_incidences",
    )?;
    for (index, record) in ctx
        .admit_iter(&incidences, "catia_native_zero_incidence_visits")?
        .enumerate()
    {
        let vertex_record = zero_entity_vertex_owner(records, record.record_ordinal)
            .map(|owner| ctx.copy_retained_text(&owner.id, "catia_native_zero_vertex_owner_id"))
            .transpose()?;
        output.push(CatiaZeroEntityVertexIncidence {
            id: ctx.format_retained(
                format_args!("catia:zero-entity:vertex-incidence#{index}"),
                "catia_native_zero_vertex_incidence_id",
            )?,
            byte_offset: u64_from_index(record.pos),
            record_ordinal: record.record_ordinal,
            tag: record.tag(),
            allocations: ctx.copy_slice(
                record.allocations.as_slice(),
                "catia_native_zero_vertex_allocations",
            )?,
            vertex_record,
        });
    }
    Ok(output)
}

fn zero_entity_records(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    range: Range<usize>,
) -> Result<Vec<CatiaZeroEntityRecord>, CodecError> {
    let (records, _storage) =
        ctx.with_scoped_storage("catia_native_zero_projection_scratch", || {
            crate::families::zero_entity::records::zero_entity_record_inventory_in_range(
                ctx, bytes, range,
            )
        })?;
    let mut output = Vec::new();
    ctx.reserve_vec(&mut output, records.len(), "catia_native_zero_records")?;
    for record in ctx.admit_iter(&records, "catia_native_zero_record_visits")? {
        output.push(CatiaZeroEntityRecord {
            id: ctx.format_retained(
                format_args!("catia:zero-entity:record#{}", record.record_ordinal),
                "catia_native_zero_record_id",
            )?,
            byte_offset: u64_from_index(record.pos),
            logical_end: u64_from_index(record.end),
            tag: record.tag,
            record_ordinal: record.record_ordinal,
        });
    }
    Ok(output)
}

impl CatiaNative {
    /// Decode CATIA-native records using container-bounded consolidated
    /// record sources.
    #[cfg(test)]
    pub(crate) fn decode_with_record_ranges(bytes: &[u8], ranges: &[Range<usize>]) -> Self {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            bytes,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .expect("test record source fits the service profile");
        let consolidated_records =
            crate::wire::records::consolidated_records_in_ranges(bytes, ranges.iter().cloned());
        Self::decode_with_records(
            &ctx,
            bytes,
            &consolidated_records,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("test native records fit the service profile")
    }

    /// Decode CATIA-native records from descriptor-scoped logical sources.
    pub(crate) fn decode_with_record_sources(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        bytes: &[u8],
        sources: &[Vec<crate::wire::records::SourceExtent>],
        refusal: &mut crate::nurbs::LaneRefusals,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let (consolidated_records, _storage) =
            ctx.with_scoped_storage("catia_native_record_source_scratch", || {
                crate::wire::records::consolidated_records_in_sources(
                    ctx,
                    bytes,
                    ctx.admit_iter(sources, "catia_native_consolidated_source_visits")?,
                )
            })?;
        Self::decode_with_records(ctx, bytes, &consolidated_records, refusal)
    }

    fn decode_with_records(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        bytes: &[u8],
        consolidated_records: &[ConsolidatedRecord],
        refusal: &mut crate::nurbs::LaneRefusals,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let (outer_directory, _directory_storage) = ctx
            .with_scoped_storage("catia_native_directory_scratch", || {
                container::parse_outer_stream_directory(ctx, bytes)
            })?;
        let (outer_container_declarations, _container_storage) = ctx.with_scoped_storage(
            "catia_native_container_scratch",
            || -> Result<_, CodecError> {
                Ok(match outer_directory.as_ref() {
                    Some(outer) => container::outer_container_declarations(ctx, bytes, outer)?,
                    None => Vec::new(),
                })
            },
        )?;
        let outer_container_index = outer_directory
            .as_ref()
            .map(|outer| {
                container::outer_container_extent_index(ctx, outer, &outer_container_declarations)
            })
            .transpose()?;
        let (parsed_finjpl, _finjpl_storage) = ctx
            .with_scoped_storage("catia_native_finjpl_scratch", || {
                container::finjpl_segments(ctx, &container::BodyExtent::whole(bytes))
            })?;
        let finjpl_segments = ctx.try_collect_vec(
            parsed_finjpl.into_iter().enumerate().map(
                |(index, segment)| -> Result<_, CodecError> {
                    let id = ctx.format_retained(
                        format_args!("catia:outer:finjpl#{index}"),
                        "catia_native_finjpl_id",
                    )?;
                    let family = ctx.copy_retained_text(
                        finjpl_family(segment.kind()),
                        "catia_native_finjpl_family",
                    )?;
                    let data =
                        ctx.copy_slice(&bytes[segment.range.clone()], "catia_native_finjpl_bytes")?;
                    Ok(CatiaFinjplSegment {
                        id,
                        byte_offset: u64_from_index(segment.range.start),
                        byte_len: u64_from_index(segment.range.end - segment.range.start),
                        type_word: segment.type_word,
                        family,
                        name: segment
                            .name
                            .as_ref()
                            .map(|name| ctx.copy_retained_text(name, "catia_native_finjpl_name"))
                            .transpose()?,
                        data,
                    })
                },
            ),
            "catia_native_finjpl_segments",
        )?;
        let (mut parsed_catalogs, _catalog_storage) = ctx
            .with_scoped_storage("catia_native_catalog_scratch", || {
                catalog::parse(ctx, bytes)
            })?;
        let (entity_runs, _entity_storage) = ctx
            .with_scoped_storage("catia_native_entity_run_scratch", || {
                entity_table::parse_runs(ctx, bytes)
            })?;
        // Run, catalog and graph indexes are dropped before decode returns.
        let mut scratch = ctx.reserve_scoped(0, "catia_native_decode_indexes")?;
        let paired_object_graph_roots = scratch.with_storage(|| {
            ctx.collect_hash_map(
                ctx.admit_iter(&entity_runs, "catia_native_paired_graph_root_source_visits")?
                    .filter_map(|run| {
                        let end = run.last()?.pos.checked_add(run.last()?.total_len())?;
                        (bytes.get(end) == Some(&0xde)).then_some((end + 1, run.len()))
                    }),
                "catia_native_paired_graph_roots",
            )
        })?;
        let (parsed_aliases, _alias_storage) = ctx
            .with_scoped_storage("catia_native_alias_scratch", || {
                object_graph::surface_aliases(ctx, bytes)
            })?;
        let mut alias_rows = ctx.try_collect_vec(
            parsed_aliases
                .into_iter()
                .map(|row| CatiaAliasRow::from_source(ctx, row)),
            "catia_native_alias_rows",
        )?;
        let (mut parsed_object_graphs, _graph_storage) = ctx
            .with_scoped_storage("catia_native_parsed_graph_scratch", || {
                object_graph::parse_all_with_paired_roots(ctx, bytes, &paired_object_graph_roots)
            })?;
        let (mut parsed_value_blocks, _value_storage) = ctx
            .with_scoped_storage("catia_native_value_scratch", || {
                value_block::parse(ctx, bytes)
            })?;
        filter_nested_inventory(
            ctx,
            &mut parsed_object_graphs,
            &mut parsed_value_blocks,
            &mut parsed_catalogs,
        )?;
        let catalogs = ctx.try_collect_vec(
            parsed_catalogs
                .into_iter()
                .map(|catalog| CatiaCatalog::from_source(ctx, catalog)),
            "catia_native_catalogs",
        )?;
        // The first catalog at each offset.
        let mut catalogs_by_offset = HashMap::new();
        for catalog in ctx.admit_iter(&catalogs, "catia_native_catalog_index_visits")? {
            scratch.with_storage(|| {
                ctx.entry_hash_map(
                    &mut catalogs_by_offset,
                    catalog.byte_offset,
                    "catia_native_catalog_index",
                )
                .map(|entry| {
                    entry.or_insert(catalog);
                })
            })?;
        }
        let catalog_at = |offset: u64| -> Result<Option<&CatiaCatalog>, CodecError> {
            Ok(ctx
                .get_hash_map(&catalogs_by_offset, &offset, "catia_native_catalog_lookup")?
                .copied())
        };
        let mut entity_runs = scratch.with_storage(|| {
            ctx.collect_hash_map(
                ctx.admit_iter(entity_runs, "catia_native_entity_run_visits")?
                    .filter_map(|run| {
                        let end = run.last()?.pos.checked_add(run.last()?.total_len())?;
                        (bytes.get(end) == Some(&0xde)).then_some(((end + 1, run.len()), run))
                    }),
                "catia_native_entity_runs",
            )
        })?;
        let mut entity_records = Vec::new();
        let mut object_graphs = Vec::new();
        let mut design_objects = Vec::new();
        // The entity records of graph `i` are `entity_records[graph_entities[i]]`.
        let mut graph_entities = Vec::new();
        for graph in ctx.admit_iter(
            &parsed_object_graphs,
            "catia_native_parsed_object_graph_visits",
        )? {
            let entities = ctx
                .remove_hash_map(
                    &mut entity_runs,
                    &(graph.pos, graph.records.len()),
                    "catia_native_entity_runs",
                )?
                .unwrap_or_default();
            let finjpl_segment = containing_finjpl_segment(
                ctx,
                u64_from_index(graph.pos),
                u64_from_index(graph.total_len),
                &finjpl_segments,
            )?
            .map(|id| ctx.copy_retained_text(id, "catia_native_graph_finjpl"))
            .transpose()?;
            let outer_container = outer_container_index
                .as_ref()
                .map(|index| {
                    container::outer_container_for_extent(
                        ctx,
                        index,
                        u64_from_index(graph.pos),
                        u64_from_index(graph.total_len),
                    )
                })
                .transpose()?
                .flatten()
                .map(|container| CatiaOuterContainerBinding::from_source(ctx, container))
                .transpose()?;
            let mut graph_scratch = ctx.reserve_scoped(0, "catia_native_graph_scratch")?;
            let (mut graph, mut entities, record_index) = native_object_graph(
                ctx,
                &mut graph_scratch,
                graph,
                entities,
                finjpl_segment,
                outer_container,
            )?;
            let catalog = graph
                .catalog_byte_offset
                .map(catalog_at)
                .transpose()?
                .flatten();
            graph.catalog = catalog
                .map(|catalog| ctx.copy_retained_text(&catalog.id, "catia_native_graph_catalog_id"))
                .transpose()?;
            for record in ctx.admit_iter(&mut graph.records, "catia_native_graph_class_visits")? {
                if let Some(class) = &mut record.class {
                    let entry = usize::try_from(class.ordinal)
                        .ok()
                        .and_then(|ordinal| catalog?.entries.get(ordinal));
                    class.entry = entry
                        .map(|entry| {
                            ctx.copy_retained_text(&entry.id, "catia_native_class_entry_id")
                        })
                        .transpose()?;
                    class.name = entry
                        .map(|entry| {
                            ctx.copy_retained_text(&entry.value, "catia_native_class_name")
                        })
                        .transpose()?;
                }
                record.repeated_reference_schema_selection = repeated_reference_schema_selection(
                    ctx,
                    object_graph::repeated_reference_schema_preamble_charged(ctx, &record.payload)?
                        .as_ref(),
                    catalog,
                )?;
            }
            let mut incidences = GraphIncidences::new(ctx, &graph.records)?;
            for entity in ctx.admit_iter(&mut entities, "catia_native_graph_entity_values")? {
                // Selectors, fields and packets are dropped with this entity's pass.
                let (selectors, _selector_storage) =
                    ctx.with_scoped_storage("catia_native_entity_selectors", || {
                        entity_table::parse_definition_schema_selectors(
                            ctx,
                            entity.definition_prefix(),
                        )
                    })?;
                entity.definition_schema_selections =
                    definition_schema_selections(ctx, &selectors, catalog)?;
                let (value_fields, _field_storage) = ctx
                    .with_scoped_storage("catia_native_entity_value_fields", || {
                        entity.value_fields_charged(ctx)
                    })?;
                let (value_packets, _packet_storage) = ctx
                    .with_scoped_storage("catia_native_entity_value_packets", || {
                        entity.value_packets(ctx, &value_fields)
                    })?;
                entity.value_schema_selections =
                    entity_value_schema_selections(ctx, &value_fields, catalog, &value_packets)?;
                entity.parse_suffix(ctx)?;
                entity.suffix_schema_selection =
                    entity_suffix_schema_selection(ctx, entity.suffix_value(), catalog)?;
                entity.value_production =
                    value_production(ctx, entity, &mut incidences, &value_fields)?;
                entity.range_interval = range_interval(
                    ctx,
                    entity.value_payload(),
                    &entity.value_schema_selections,
                    entity.suffix_value(),
                    &mut incidences,
                    entity.entity_id,
                )?;
            }
            drop(incidences);
            projection::graph_design_objects(
                ctx,
                &graph,
                &entities,
                &record_index,
                &mut design_objects,
            )?;
            let start = entity_records.len();
            ctx.append_vec(
                &mut entity_records,
                &mut entities,
                "catia_native_entity_records",
            )?;
            scratch.with_storage(|| {
                ctx.push_vec(
                    &mut graph_entities,
                    start..entity_records.len(),
                    "catia_native_graph_entity_ranges",
                )
            })?;
            ctx.push_vec(&mut object_graphs, graph, "catia_native_object_graphs")?;
        }
        drop(entity_runs);
        let mut index_storage = ctx.reserve_scoped(0, "catia_native_semantic_indices")?;
        let entity_classes_by_graph_identity =
            entity_class_index(ctx, &mut index_storage, &object_graphs)?;
        let semantic = semantic_entity_indices(
            ctx,
            &mut index_storage,
            &entity_records,
            &entity_classes_by_graph_identity,
        )?;
        let entity_references = CatiaEntityReferenceIndex {
            entities: &semantic.entities,
            classes: &entity_classes_by_graph_identity,
            maxima: &semantic.maxima,
        };
        // Reference resolutions and object productions are computed against
        // indexes that borrow the entity records, then installed.
        let (mut signatures, mut productions) =
            index_storage.with_storage(|| -> Result<_, CodecError> {
                Ok((
                    ctx.collection_vec::<Option<(CatiaEntityReference, CatiaEntityReference)>>(
                        entity_records.len(),
                        "catia_native_entity_resolutions",
                    )?,
                    ctx.collection_vec::<Option<CatiaEntityObjectProduction>>(
                        entity_records.len(),
                        "catia_native_entity_resolutions",
                    )?,
                ))
            })?;
        for (graph, range) in ctx
            .admit_iter(&object_graphs, "catia_native_entity_resolution_graphs")?
            .zip(&graph_entities)
        {
            for entity in ctx.admit_iter(
                entity_records.get(range.clone()).unwrap_or_default(),
                "catia_native_entity_resolution_visits",
            )? {
                signatures.push(
                    entity
                        .reference_signature
                        .as_ref()
                        .map(|signature| -> Result<_, CodecError> {
                            Ok((
                                entity_reference(
                                    ctx,
                                    &entity.object_graph,
                                    signature.production.first_reference(),
                                    &entity_references,
                                )?,
                                entity_reference(
                                    ctx,
                                    &entity.object_graph,
                                    signature.production.second_reference(),
                                    &entity_references,
                                )?,
                            ))
                        })
                        .transpose()?,
                );
                // An entity record's object record is the graph record at its ordinal.
                let object = usize::try_from(entity.ordinal)
                    .ok()
                    .and_then(|ordinal| graph.records.get(ordinal));
                productions.push(match object {
                    Some(object) => object_production(
                        ctx,
                        entity,
                        object,
                        &entity_references,
                        &semantic.relation_expressions,
                        &semantic.relation_expression_entities,
                        &semantic.parameter_bindings,
                    )?,
                    None => None,
                });
            }
        }
        let schema_configuration_row_chains = derive_schema_configuration_row_chains(
            ctx,
            &entity_records,
            |index| match productions.get(index) {
                Some(Some(CatiaEntityObjectProduction::SchemaConfigurationRowLink(link))) => {
                    Some(link)
                }
                _ => None,
            },
            &entity_references,
        )?;
        drop(semantic);
        drop(entity_classes_by_graph_identity);
        for ((entity, signature), production) in ctx
            .admit_iter(
                &mut entity_records,
                "catia_native_entity_resolution_updates",
            )?
            .zip(signatures)
            .zip(productions)
        {
            if let (Some(stored), Some((first_entity, second_entity))) =
                (entity.reference_signature.as_mut(), signature)
            {
                stored.first_entity = first_entity;
                stored.second_entity = second_entity;
            }
            entity.object_production = production;
        }
        drop(index_storage);
        let reference_signature_cohorts = derive_reference_signature_cohorts(ctx, &entity_records)?;
        filter_nested_alias_rows(
            ctx,
            &mut alias_rows,
            &object_graphs,
            &parsed_value_blocks,
            &catalogs,
        )?;
        resolve_alias_surface_tags(ctx, &mut alias_rows)?;
        let part_graph = {
            const OPERATION: &str = "catia_native_part_graph_visits";
            let is_part = |graph: &CatiaObjectGraph| -> Result<bool, CodecError> {
                Ok(graph
                    .outer_container
                    .as_ref()
                    .is_some_and(|container| container.class_name == "CATPrtCont"))
            };
            match ctx.position_by(&object_graphs, is_part, OPERATION)? {
                Some(index) if !ctx.any_by(&object_graphs[index + 1..], is_part, OPERATION)? => {
                    Some(&object_graphs[index])
                }
                Some(_) | None => None,
            }
        };
        if let Some(graph) = part_graph {
            for row in ctx.admit_iter(&mut alias_rows, "catia_native_alias_row_bindings")? {
                let Some(index) = usize::from(row.entity_record_ordinal()).checked_sub(1) else {
                    continue;
                };
                let Some(record) = graph.records.get(index) else {
                    continue;
                };
                row.object_graph =
                    Some(ctx.copy_retained_text(&graph.id, "catia_native_alias_object_graph_id")?);
                row.object_record = Some(
                    ctx.copy_retained_text(&record.id, "catia_native_alias_object_record_id")?,
                );
                row.design_object = record
                    .design_object
                    .as_ref()
                    .map(|id| ctx.copy_retained_text(id, "catia_native_alias_design_object_id"))
                    .transpose()?;
            }
        }
        let mut value_blocks = Vec::new();
        if !parsed_value_blocks.is_empty() {
            // The first graph ending at each offset.
            let mut graphs_by_end = HashMap::new();
            for graph in ctx.admit_iter(&object_graphs, "catia_native_graph_end_index_visits")? {
                let Some(end) = graph.byte_offset.checked_add(graph.byte_len) else {
                    continue;
                };
                scratch.with_storage(|| {
                    ctx.entry_hash_map(&mut graphs_by_end, end, "catia_native_graph_end_index")
                        .map(|entry| {
                            entry.or_insert(graph);
                        })
                })?;
            }
            for block in ctx.admit_iter(parsed_value_blocks, "catia_native_value_block_visits")? {
                let Some(catalog_pos) =
                    block.pos.checked_add(block.total_len()).map(u64_from_index)
                else {
                    continue;
                };
                let Some(catalog) = catalog_at(catalog_pos)? else {
                    continue;
                };
                let object_graph = ctx
                    .get_hash_map(
                        &graphs_by_end,
                        &u64_from_index(block.pos),
                        "catia_native_graph_end_index",
                    )?
                    .copied();
                let value = CatiaValueBlock::from_parts(ctx, &block, catalog, object_graph)?;
                ctx.push_vec(&mut value_blocks, value, "catia_native_value_blocks")?;
            }
        }
        let preview_images = preview_views(ctx, &finjpl_segments)?;
        let external_references = external_reference_views(ctx, &finjpl_segments)?;
        let mut legacy_entity_runs = legacy_entity_runs(ctx, bytes)?;
        for run in ctx.admit_iter(
            &mut legacy_entity_runs,
            "catia_native_legacy_run_containers",
        )? {
            run.outer_container = outer_container_index
                .as_ref()
                .map(|index| {
                    container::outer_container_for_extent(ctx, index, run.byte_offset, run.byte_len)
                })
                .transpose()?
                .flatten()
                .map(|container| CatiaOuterContainerBinding::from_source(ctx, container))
                .transpose()?;
        }
        let consolidated_circles =
            projection::consolidated_circles(ctx, bytes, consolidated_records)?;
        let consolidated_class61_records =
            consolidated_class61_records(ctx, bytes, consolidated_records)?;
        let consolidated_class5b5c_records =
            consolidated_class5b5c_records(ctx, bytes, consolidated_records)?;
        let consolidated_parameter_points =
            projection::consolidated_parameter_points(ctx, bytes, consolidated_records)?;
        let consolidated_cone_faces = consolidated_cone_faces(
            ctx,
            bytes,
            consolidated_records,
            &consolidated_parameter_points,
        )?;
        let consolidated_cones = projection::consolidated_cones(ctx, bytes, consolidated_records)?;
        let consolidated_cylinders = consolidated_cylinders(ctx, bytes, consolidated_records)?;
        let (consolidated_groups, consolidated_embedded_cylinders) =
            consolidated_cylinder_groups(ctx, bytes, consolidated_records)?;
        let consolidated_line_profiles =
            projection::consolidated_line_profiles(ctx, bytes, consolidated_records)?;
        let mut consolidated_owner_packets =
            consolidated_owner_packets(ctx, bytes, consolidated_records)?;
        resolve_owner_chart_support_aliases(ctx, &mut consolidated_owner_packets, &alias_rows)?;
        let consolidated_pcurves = consolidated_pcurves(ctx, bytes, consolidated_records)?;
        let consolidated_plane_carriers =
            projection::consolidated_plane_carriers(ctx, bytes, consolidated_records)?;
        let consolidated_reference_lists =
            projection::consolidated_reference_lists(ctx, bytes, consolidated_records)?;
        let consolidated_revolutions = projection::consolidated_revolutions(
            ctx,
            bytes,
            consolidated_records,
            &consolidated_circles,
        )?;
        let consolidated_spheres =
            projection::consolidated_spheres(ctx, bytes, consolidated_records)?;
        let consolidated_tori = projection::consolidated_tori(ctx, bytes, consolidated_records)?;
        let zero_entity_range = container::outer_preamble_range(ctx, bytes)?.unwrap_or_else(|| {
            if bytes.starts_with(container::OUTER_MAGIC) {
                0..0
            } else {
                0..bytes.len()
            }
        });
        let zero_entity_records = zero_entity_records(ctx, bytes, zero_entity_range.clone())?;
        let zero_entity_edge_strides =
            zero_entity_edge_strides(ctx, bytes, zero_entity_range.clone())?;
        let zero_entity_oriented_use_pairs =
            zero_entity_oriented_use_pairs(ctx, bytes, zero_entity_range.clone())?;
        let zero_entity_ownership_roots =
            zero_entity_ownership_roots(ctx, bytes, zero_entity_range.clone())?;
        let (parsed_zero_entity_support_runs, _support_run_storage) =
            ctx.with_scoped_storage("catia_native_zero_support_run_scratch", || {
                crate::families::zero_entity::records::zero_entity_support_runs_in_range(
                    ctx,
                    bytes,
                    zero_entity_range.clone(),
                    refusal,
                )
            })?;
        let (parsed_zero_entity_endpoint_pairs, _endpoint_pair_storage) =
            ctx.with_scoped_storage("catia_native_zero_endpoint_pair_scratch", || {
                crate::families::zero_entity::topology::zero_entity_endpoint_pair_candidates(
                    ctx,
                    &parsed_zero_entity_support_runs,
                )
            })?;
        let zero_entity_endpoint_pair_candidates =
            zero_entity_endpoint_pair_candidates(ctx, &parsed_zero_entity_endpoint_pairs)?;
        let (parsed_zero_entity_endpoint_loci, _endpoint_locus_storage) =
            ctx.with_scoped_storage("catia_native_zero_endpoint_locus_scratch", || {
                crate::families::zero_entity::topology::endpoint_locus_candidates(
                    ctx,
                    &parsed_zero_entity_endpoint_pairs,
                )
            })?;
        let zero_entity_endpoint_locus_candidates =
            zero_entity_endpoint_locus_candidates(ctx, &parsed_zero_entity_endpoint_loci)?;
        let zero_entity_support_runs =
            zero_entity_support_runs(ctx, parsed_zero_entity_support_runs, &zero_entity_records)?;
        let zero_entity_vertex_incidences =
            zero_entity_vertex_incidences(ctx, bytes, zero_entity_range, &zero_entity_records)?;
        let consolidated_edge_nodes =
            consolidated_edge_nodes(ctx, bytes, consolidated_records, &consolidated_circles)?;
        let consolidated_edge_runs = consolidated_edge_runs(
            ctx,
            bytes,
            consolidated_records,
            &consolidated_pcurves,
            &consolidated_edge_nodes,
            refusal,
        )?;
        let consolidated_vertex_identities =
            consolidated_vertex_identities(ctx, &consolidated_edge_nodes)?;
        Ok(Self {
            alias_rows,
            catalogs,
            consolidated_circles,
            consolidated_class61_records,
            consolidated_class5b5c_records,
            consolidated_cone_faces,
            consolidated_cones,
            consolidated_cylinders,
            consolidated_embedded_cylinders,
            consolidated_edge_nodes,
            consolidated_edge_runs,
            consolidated_groups,
            consolidated_line_profiles,
            consolidated_owner_packets,
            consolidated_parameter_points,
            consolidated_plane_carriers,
            consolidated_pcurves,
            consolidated_reference_lists,
            consolidated_revolutions,
            consolidated_spheres,
            consolidated_tori,
            consolidated_vertex_identities,
            design_objects,
            entity_records,
            external_references,
            finjpl_segments,
            legacy_entity_runs,
            object_graphs,
            preview_images,
            reference_signature_cohorts,
            schema_configuration_row_chains,
            value_blocks,
            zero_entity_edge_strides,
            zero_entity_oriented_use_pairs,
            zero_entity_ownership_roots,
            zero_entity_endpoint_pair_candidates,
            zero_entity_records,
            zero_entity_support_runs,
            zero_entity_endpoint_locus_candidates,
            zero_entity_vertex_incidences,
        })
    }

    /// Store this namespace while moving child arenas out of their typed owners.
    pub(crate) fn store_owned(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        namespace: &mut cadmpeg_ir::NativeNamespace,
    ) -> Result<(), cadmpeg_ir::NativeConvertError> {
        let projection = CatiaArenaProjection::from_owned(ctx, self)?;
        store_projection(ctx, &projection, namespace)
    }
}

#[cfg(test)]
mod test_only;
/// Removes alias rows whose frame overlaps a parsed graph, value block, or catalog.
fn filter_nested_alias_rows(
    ctx: &DecodeContext<'_>,
    rows: &mut Vec<CatiaAliasRow>,
    object_graphs: &[CatiaObjectGraph],
    value_blocks: &[value_block::ValueBlock],
    catalogs: &[CatiaCatalog],
) -> Result<(), CodecError> {
    ctx.retain_vec(
        rows,
        |row| {
            // A marker inside the first four bytes of the image has no row
            // frame, so the row is not an independent alias core. Refuse it
            // here rather than aliasing its frame with the file head.
            let Some(row_start) = row.row_byte_offset() else {
                return Ok(false);
            };
            Ok(!(ctx.any_by(
                object_graphs,
                |graph| {
                    Ok(extents_overlap(
                        row_start,
                        24,
                        graph.byte_offset,
                        graph.byte_len,
                    ))
                },
                "catia_native_alias_graph_overlap_scan",
            )? || ctx.any_by(
                value_blocks,
                |block| {
                    Ok(extents_overlap(
                        row_start,
                        24,
                        u64_from_index(block.pos),
                        u64_from_index(block.total_len()),
                    ))
                },
                "catia_native_alias_value_block_overlap_scan",
            )? || ctx.any_by(
                catalogs,
                |catalog| {
                    Ok(extents_overlap(
                        row_start,
                        24,
                        catalog.byte_offset,
                        catalog.byte_len,
                    ))
                },
                "catia_native_alias_catalog_overlap_scan",
            )?))
        },
        "catia_native_alias_rows_retain",
    )
}

/// Removes inventories nested inside another frame, searching each candidate
/// container only until the first one that contains it.
fn filter_nested_inventory(
    ctx: &DecodeContext<'_>,
    graphs: &mut Vec<object_graph::ObjectGraph>,
    blocks: &mut Vec<value_block::ValueBlock>,
    catalogs: &mut Vec<catalog::Catalog>,
) -> Result<(), CodecError> {
    let graph_contains =
        |graphs: &[object_graph::ObjectGraph], pos, len, operation: &'static str| {
            ctx.any_by(
                graphs,
                |graph| Ok(extent_contains(graph.pos, graph.total_len, pos, len)),
                operation,
            )
        };
    let block_contains = |blocks: &[value_block::ValueBlock], pos, len, operation: &'static str| {
        ctx.any_by(
            blocks,
            |block| Ok(extent_contains(block.pos, block.total_len(), pos, len)),
            operation,
        )
    };
    ctx.retain_vec(
        blocks,
        |block| {
            Ok(!graph_contains(
                graphs,
                block.pos,
                block.total_len(),
                "catia_native_inventory_block_graph_overlap_scan",
            )?)
        },
        "catia_native_inventory_blocks_retain",
    )?;
    ctx.retain_vec(
        graphs,
        |graph| {
            Ok(!block_contains(
                blocks,
                graph.pos,
                graph.total_len,
                "catia_native_inventory_graph_block_overlap_scan",
            )?)
        },
        "catia_native_inventory_graphs_retain",
    )?;
    ctx.retain_vec(
        catalogs,
        |catalog| {
            Ok(!(graph_contains(
                graphs,
                catalog.pos,
                catalog.total_len,
                "catia_native_inventory_catalog_graph_overlap_scan",
            )? || block_contains(
                blocks,
                catalog.pos,
                catalog.total_len,
                "catia_native_inventory_catalog_block_overlap_scan",
            )?))
        },
        "catia_native_inventory_catalogs_retain",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
