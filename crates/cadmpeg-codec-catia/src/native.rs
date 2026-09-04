// SPDX-License-Identifier: Apache-2.0
//! CATIA-native ownership and design records retained outside the neutral model.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::ops::Range;

use cadmpeg_core::decode::View;
use cadmpeg_ir::native::catalogue::{Catalogue, FamilyRow, Phase};

use crate::catalog;
use crate::container;
use crate::entity_table;
use crate::legacy_entity;
use crate::object_graph::{
    self, AliasGroupMembership, AliasLead, HeadToken, ListItem, ObjectPayload, PayloadField,
    PayloadSubtype,
};
use crate::value_block;
use crate::wire::records::ConsolidatedRecord;

/// Current schema version for the CATIA native namespace.
pub const CATIA_NATIVE_VERSION: u32 = 288;
/// Native schema version that links width-coded owner-chart supports to alias rows.
#[cfg(test)]
pub(crate) const CATIA_OWNER_CHART_ALIAS_VERSION: u32 = 286;
/// Native schema version that resolves grouped aliases to persistent surface tags.
#[cfg(test)]
pub(crate) const CATIA_ALIAS_SURFACE_TAG_VERSION: u32 = 285;
/// Native schema version associating exact scalar nominals with `Range` intervals.
#[cfg(test)]
pub(crate) const CATIA_RANGE_NOMINAL_VERSION: u32 = 276;
/// Native schema version admitting the `81 93` entity-suffix value trailer.
#[cfg(test)]
pub(crate) const CATIA_SUFFIX_TRAILER_8193_VERSION: u32 = 275;
/// Native schema version retaining complete source-schema `Range` intervals.
#[cfg(test)]
pub(crate) const CATIA_RANGE_INTERVAL_VERSION: u32 = 273;
/// Native schema version retaining incoming incidences for every `Range` interval.
#[cfg(test)]
pub(crate) const CATIA_RANGE_INTERVAL_INCIDENCE_VERSION: u32 = 274;
/// Native schema version using schema-configuration names and derived identities.
#[cfg(test)]
pub(crate) const CATIA_SCHEMA_CONFIGURATION_NAMING_VERSION: u32 = 272;
#[cfg(test)]
const CATIA_LEGACY_IDENTITY_LEAD_VERSION: u32 = 216;
#[cfg(test)]
const CATIA_LEGACY_ROLE_SELECTOR_VERSION: u32 = 212;
#[cfg(test)]
const CATIA_LEGACY_ROLE_FIELD_CODE_VERSION: u32 = 220;
#[cfg(test)]
const CATIA_LEGACY_SCHEMA_IDENTIFIER_VERSION: u32 = 222;
#[cfg(test)]
const CATIA_LEGACY_SCHEMA_BOUNDARY_VERSION: u32 = 223;
#[cfg(test)]
const CATIA_LEGACY_EVALUATED_VALUE_NAME_VERSION: u32 = 224;
#[cfg(test)]
pub(crate) const CATIA_RELATION_PROGRAM_INSTANCE_VERSION: u32 = 228;
/// Native schema version adding the compact relation frame's context incidence.
#[cfg(test)]
pub(crate) const CATIA_RELATION_PROGRAM_CONTEXT_VERSION: u32 = 231;
#[cfg(test)]
pub(crate) const CATIA_CONSTRAINT_RANGE_INCIDENCE_VERSION: u32 = 229;
#[cfg(test)]
const CATIA_CONFIGURATION_INCIDENCE_VERSION: u32 = 230;
/// Native schema version separating schema-configuration selectors and entity references.
#[cfg(test)]
pub(crate) const CATIA_SCHEMA_CONFIGURATION_REFERENCE_VERSION: u32 = 232;
/// Native schema version retaining selected entity classes on typed incidences.
#[cfg(test)]
pub(crate) const CATIA_TYPED_INCIDENCE_CLASS_VERSION: u32 = 233;
/// Native schema version unifying relation-program entity incidences.
#[cfg(test)]
pub(crate) const CATIA_RELATION_TYPED_REFERENCE_VERSION: u32 = 234;
/// Native schema version retaining the source entity of constraint-range incidences.
#[cfg(test)]
pub(crate) const CATIA_CONSTRAINT_RANGE_SOURCE_ENTITY_VERSION: u32 = 235;
/// Native namespace version that unifies formula output incidence.
#[cfg(test)]
pub(crate) const CATIA_FORMULA_OUTPUT_REFERENCE_VERSION: u32 = 236;
/// Native namespace version that unifies formula expression incidence.
#[cfg(test)]
pub(crate) const CATIA_FORMULA_EXPRESSION_REFERENCE_VERSION: u32 = 237;
/// Native namespace version that types formula dependency candidate incidences.
#[cfg(test)]
pub(crate) const CATIA_FORMULA_DEPENDENCY_REFERENCE_VERSION: u32 = 238;
/// Native schema version retaining complete ordered schema-configuration-row chains.
#[cfg(test)]
pub(crate) const CATIA_SCHEMA_CONFIGURATION_ROW_CHAIN_VERSION: u32 = 239;
/// Native schema version retaining terminal-null state on every typed incidence.
#[cfg(test)]
pub(crate) const CATIA_TYPED_INCIDENCE_NULL_VERSION: u32 = 240;
/// Native schema version retaining every exact relation-program reference incidence.
#[cfg(test)]
pub(crate) const CATIA_RELATION_PROGRAM_REFERENCE_INCIDENCE_VERSION: u32 = 241;
/// Native schema version retaining relation-program source-symbol dependencies.
#[cfg(test)]
pub(crate) const CATIA_RELATION_PROGRAM_DEPENDENCY_VERSION: u32 = 242;
/// Native schema version retaining complete ordered relation-program inputs.
#[cfg(test)]
pub(crate) const CATIA_RELATION_PROGRAM_INPUT_VERSION: u32 = 243;
/// Native schema version retaining entities between schema-configuration-row successors.
#[cfg(test)]
pub(crate) const CATIA_SCHEMA_CONFIGURATION_ROW_INTERVAL_VERSION: u32 = 244;
/// Native schema version retaining constraint-range storage incidences.
#[cfg(test)]
pub(crate) const CATIA_CONSTRAINT_RANGE_STORAGE_INCIDENCE_VERSION: u32 = 245;
/// Native schema version retaining each relation-symbol occurrence offset.
#[cfg(test)]
pub(crate) const CATIA_RELATION_DEPENDENCY_OFFSET_VERSION: u32 = 246;
/// Native schema version retaining relation-program reference occurrence offsets.
#[cfg(test)]
pub(crate) const CATIA_RELATION_REFERENCE_OFFSET_VERSION: u32 = 247;
/// Native schema version excluding string-literal contents from relation dependencies.
#[cfg(test)]
pub(crate) const CATIA_RELATION_STRING_LITERAL_DEPENDENCY_VERSION: u32 = 248;
/// Native schema version requiring canonical relation-signature parameter symbols.
#[cfg(test)]
pub(crate) const CATIA_RELATION_SIGNATURE_PARAMETER_VERSION: u32 = 249;
/// Native schema version retaining formula reference occurrence offsets.
#[cfg(test)]
pub(crate) const CATIA_FORMULA_REFERENCE_OFFSET_VERSION: u32 = 250;
/// Native schema version retaining configuration payload occurrence offsets.
#[cfg(test)]
pub(crate) const CATIA_CONFIGURATION_PAYLOAD_OFFSET_VERSION: u32 = 251;
/// Native schema version retaining typed entity-schema selector incidences.
#[cfg(test)]
pub(crate) const CATIA_ENTITY_SCHEMA_VALUE_INCIDENCE_VERSION: u32 = 252;
/// Native schema version retaining suffix schema-selector offsets.
#[cfg(test)]
pub(crate) const CATIA_SUFFIX_SCHEMA_OFFSET_VERSION: u32 = 253;
/// Native schema version retaining suffix evaluation-opcode offsets.
#[cfg(test)]
pub(crate) const CATIA_SUFFIX_EVALUATION_OFFSET_VERSION: u32 = 254;
/// Native schema version retaining ordered schema-configuration-row link incidences.
#[cfg(test)]
pub(crate) const CATIA_SCHEMA_CONFIGURATION_ROW_LINK_INCIDENCE_VERSION: u32 = 255;
/// Native schema version retaining parallel-reference cell offsets.
#[cfg(test)]
pub(crate) const CATIA_PARALLEL_REFERENCE_CELL_OFFSET_VERSION: u32 = 256;
/// Native schema version retaining parallel-reference column incidences.
#[cfg(test)]
pub(crate) const CATIA_PARALLEL_REFERENCE_COLUMN_INCIDENCE_VERSION: u32 = 257;
/// Native schema version requiring exact relation-signature outer whitespace.
#[cfg(test)]
pub(crate) const CATIA_RELATION_SIGNATURE_WHITESPACE_VERSION: u32 = 258;
/// Native schema version retaining reference-signature field incidences.
#[cfg(test)]
pub(crate) const CATIA_REFERENCE_SIGNATURE_INCIDENCE_VERSION: u32 = 259;
/// Native schema version resolving reference-signature entity incidences.
#[cfg(test)]
pub(crate) const CATIA_REFERENCE_SIGNATURE_ENTITY_VERSION: u32 = 260;
/// Native schema version requiring consecutive reference-signature identities.
#[cfg(test)]
pub(crate) const CATIA_REFERENCE_SIGNATURE_PAIR_VERSION: u32 = 263;
/// Native schema version retaining reference-signature cohorts.
#[cfg(test)]
pub(crate) const CATIA_REFERENCE_SIGNATURE_COHORT_VERSION: u32 = 264;
/// Native schema version retaining exact nullable numeric-pair productions.
#[cfg(test)]
pub(crate) const CATIA_NUMERIC_PAIR_VERSION: u32 = 265;
/// Native schema version enforcing canonical reference-signature framing equations.
#[cfg(test)]
pub(crate) const CATIA_REFERENCE_SIGNATURE_FRAME_VERSION: u32 = 267;
/// Native schema version retaining cohort-level descriptor schema incidences.
#[cfg(test)]
pub(crate) const CATIA_REFERENCE_SIGNATURE_SCHEMA_VERSION: u32 = 268;
/// Native schema version assigning canonical identities to graph-derived arenas.
#[cfg(test)]
pub(crate) const CATIA_DERIVED_NATIVE_ID_VERSION: u32 = 269;
/// Native schema version assigning the framing-specific `paramout` result slot.
#[cfg(test)]
pub(crate) const CATIA_RELATION_PROGRAM_OUTPUT_VERSION: u32 = 271;
#[cfg(test)]
const CATIA_TERMINAL_NULL_REFERENCE_VERSION: u32 = 211;
#[cfg(test)]
const CATIA_DEFINITION_CHAIN_OWNERSHIP_VERSION: u32 = 196;
#[cfg(test)]
const CATIA_TYPED_OWNER_SLOT_VERSION: u32 = 198;
#[cfg(test)]
const CATIA_SUFFIX_FRAMING_VERSION: u32 = 200;
#[cfg(test)]
const CATIA_FORMULA_DEPENDENCY_CANDIDATE_VERSION: u32 = 206;
#[cfg(test)]
const CATIA_OBJECT_GRAPH_SEGMENT_VERSION: u32 = 208;

/// Consolidated pcurve framing family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatiaConsolidatedFamily {
    /// A-family frame with a u32 payload length.
    A,
    /// B-family frame with a u8 payload length.
    B,
}

/// Reference dialect used by a consolidated class-`0x62` owner packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatiaOwnerReferenceEncoding {
    /// Strong identities use tagged little-endian `u16` values.
    TaggedU16Strong,
    /// Strong identities use width-coded compact integers.
    WidthCodedStrong,
    /// All nine identities use the compact-integer reference grammar.
    AllCompact,
}

/// Target encoding of a consolidated class-`0x5f` face node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatiaFaceNodeTargetEncoding {
    /// Width-coded compact target.
    Compact,
    /// Strong persistent target encoded as `0x0a <u16le>`.
    TaggedU16Strong,
}

/// Derived class-`0x5f` face-node relation associated with a consolidated
/// class-`0x62` packet within one bounded record source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaFaceNodeRelation {
    /// Face-node record byte offset.
    pub byte_offset: u64,
    /// Complete face-node to packet span.
    pub byte_len: u64,
    /// Width-coded header token.
    pub header_token: u32,
    /// Target encoding selected after the `0x82` lead.
    pub target_encoding: CatiaFaceNodeTargetEncoding,
    /// Class-`0x5f` target retained by the enclosing source-scoped relation.
    pub target: u32,
    /// Two terminal bytes of the face-node payload.
    pub terminal: [u8; 2],
}

/// Structurally decoded payload of a class-`0x62` consolidated owner packet.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaOwnerNumericTail {
    /// Five-byte class-specific header.
    pub header: [u8; 5],
    /// Lower coordinate pair of a strictly increasing binary64 box.
    pub lower: [f64; 2],
    /// Upper coordinate pair of a strictly increasing binary64 box.
    pub upper: [f64; 2],
    /// Three strictly increasing binary32 bounds in serialization order. In
    /// an all-compact owner these are the model-space X, Y, and Z bounds.
    pub bounds: [[f32; 2]; 3],
}

/// One fixed-nine owner identity resolved within its allocation source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaOwnerIdentityTarget {
    /// Zero-based identity slot in the fixed-nine packet.
    pub slot: u8,
    /// Decoded backward distance.
    pub distance: u32,
    /// Byte offset of the selected class-`0x5d` or class-`0x5e` record.
    pub target_byte_offset: u64,
    /// Selected record class.
    pub target_class: u8,
}

/// Parameter axis held constant by selectors `0x05` and `0x09` in a
/// consolidated owner chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatiaOwnerChartSideAxis {
    /// First surface parameter.
    FirstParameter,
    /// Second surface parameter.
    SecondParameter,
}

/// Family-and-class carrier production that opens an owner chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatiaOwnerChartCarrier {
    /// B-family class-`0x28` cylinder carrier.
    B28,
    /// B-family class-`0x2b` torus carrier.
    B2b,
    /// A-family class-`0x32` carrier.
    A32,
}

/// One allocation-local reference in an owner-chart bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaOwnerChartBridgeReference {
    /// Decoded allocation-local value.
    pub value: u32,
    /// Wire addressing form retained from the allocation-reference token.
    pub encoding: CatiaAllocationReferenceEncoding,
    /// Exact outer alias row selected by a unique width-coded support tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias_row: Option<String>,
    /// Canonical persistent surface tag selected through the alias row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_surface_tag: Option<u32>,
}

/// Structurally complete class-`0x37` owner-chart bridge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CatiaOwnerChartBridge {
    /// Five-reference supported-surface construction.
    SupportedSurface {
        /// Record byte offset.
        byte_offset: u64,
        /// Constructed carrier surface.
        carrier_surface: CatiaOwnerChartBridgeReference,
        /// Two supporting surfaces.
        support_surfaces: [CatiaOwnerChartBridgeReference; 2],
        /// Pcurves on the supporting surfaces.
        support_pcurves: [CatiaOwnerChartBridgeReference; 2],
        /// Six construction controls in storage order.
        controls: [u8; 6],
        /// Positive construction radius.
        construction_radius: f64,
    },
    /// Eight-reference A-family production without an assigned object role.
    Extended {
        /// Record byte offset.
        byte_offset: u64,
        /// Counted allocation references in storage order.
        references: [CatiaOwnerChartBridgeReference; 8],
        /// Four controls before the zero lane.
        controls: [u8; 4],
        /// Two terminal controls after the zero lane.
        terminal_controls: [u8; 2],
    },
}

/// Source-closed carrier chart terminated by an owner packet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaOwnerChartRelation {
    /// Carrier record byte offset.
    pub carrier_byte_offset: u64,
    /// Family-and-class carrier production.
    pub carrier: CatiaOwnerChartCarrier,
    /// Immediately following class-`0x37` bridge record.
    pub bridge: CatiaOwnerChartBridge,
    /// Axis held constant by selectors `0x05` and `0x09`.
    pub side_axis: CatiaOwnerChartSideAxis,
    /// Byte offsets of selectors `0x05`, `0x09`, `0x0d`, and `0x11`.
    pub parameter_point_byte_offsets: [u64; 4],
}

/// Structurally decoded payload of a class-`0x62` consolidated owner packet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CatiaOwnerPacketPayload {
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
    },
    /// Count-selected persistent identities followed by a nonempty tail.
    Counted {
        /// Persistent identities in serialization order.
        references: Vec<u32>,
        /// Complete nonempty class-specific tail.
        #[serde(with = "cadmpeg_ir::bytes")]
        #[cfg_attr(feature = "schema", schemars(with = "String"))]
        tail: Vec<u8>,
    },
}

/// One fixed-nine boundary edge retained when four resolved class-`0x5e`
/// targets close one simple owner-local cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaOwnerBoundaryEdge {
    /// Identity slot in the fixed-nine packet.
    pub slot: u8,
    /// Resolved class-`0x5e` edge-record offset.
    pub byte_offset: u64,
    /// Resolved class-`0x5d` endpoint-record offsets, in edge order.
    pub endpoint_records: [u64; 2],
}

/// Owner-local boundary evidence derived from a closed fixed-nine cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaOwnerBoundaryCycle {
    /// Source-scoped class-`0x5f` face node that precedes this boundary
    /// allocation and closes its checked identity, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_node: Option<CatiaFaceNodeRelation>,
    /// Four edge targets in fixed-nine slot order.
    pub edges: [CatiaOwnerBoundaryEdge; 4],
}

/// Exact class-`0x62` consolidated owner packet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedOwnerPacket {
    /// Stable source identity.
    pub id: String,
    /// Record byte offset.
    pub byte_offset: u64,
    /// Zero-based bounded record-source ordinal.
    pub source_index: usize,
    /// Width-coded header token.
    pub header_token: u32,
    /// Count-specific reference lane and tail.
    pub payload: CatiaOwnerPacketPayload,
    /// Backward-distance identities resolved within this packet's contiguous
    /// class-`0x5d`/`0x5e` allocation sequence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub identity_targets: Vec<CatiaOwnerIdentityTarget>,
    /// Source-scoped class-`0x5f` face node, when the packet relation closes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_node: Option<CatiaFaceNodeRelation>,
    /// Complete carrier/reference/side chart that this packet terminates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_chart: Option<CatiaOwnerChartRelation>,
    /// Closed owner-local four-edge boundary, when all four resolved targets
    /// form one simple cycle in the bounded record source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary_cycle: Option<CatiaOwnerBoundaryCycle>,
}

/// One structurally complete consolidated `B:29` cone chart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedCone {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Cone apex.
    pub apex: [f64; 3],
    /// First transverse unit direction.
    pub direction_x: [f64; 3],
    /// Second transverse unit direction.
    pub direction_y: [f64; 3],
    /// Cone-axis unit direction.
    pub axis: [f64; 3],
    /// Cone half-angle in radians.
    pub half_angle: f64,
    /// Reference radius of the conical surface, independent of the active chart ranges.
    pub reference_radius: f64,
    /// Active azimuth interval.
    pub angular_range: [f64; 2],
    /// Native slant-coordinate interval, including zero at the apex.
    pub slant_range: [f64; 2],
    /// Scale from azimuth to stored U parameter.
    pub angular_scale: f64,
    /// Full-turn azimuth chart domain.
    pub angular_domain: [f64; 2],
}

/// One complete consolidated `B:19` arc-length circle support.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedCircle {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Payload-layout discriminator (`0x32..=0x34`).
    pub layout: u8,
    /// Compact persistent record identity.
    pub record_id: u32,
    /// Width-coded frame token.
    pub frame_token: u8,
    /// Two centre coordinates in the host-implied carrier plane.
    pub center_pair: [f64; 2],
    /// Circle radius in millimetres.
    pub radius: f64,
    /// Arc-length parameter interval.
    pub range: [f64; 2],
    /// Whether the interval spans one complete circumference.
    pub full_circle: bool,
    /// Length-valued angular chart shift.
    pub chart_shift: f64,
}

/// Frame-specific payload of one consolidated `B:28` cylinder chart.
#[derive(Debug, Clone, PartialEq)]
pub enum CatiaConsolidatedCylinderPayload {
    /// Complete three-dimensional frame reconstructed from layout `0x52`.
    Layout52 {
        /// Token selecting the serialized frame-vector role.
        frame_token: u8,
        /// Cylinder-axis unit direction.
        axis: [f64; 3],
        /// Unit direction from which the circumferential parameter is measured.
        reference_direction: [f64; 3],
    },
    /// Complete three-dimensional frame reconstructed from layout `0x5a`.
    Layout5a {
        /// Token selecting the serialized frame-vector role.
        frame_token: u8,
        /// Cylinder-axis unit direction.
        axis: [f64; 3],
        /// Unit direction from which the circumferential parameter is measured.
        reference_direction: [f64; 3],
    },
    /// Complete layout-`0x62` frame and its redundant range origin.
    RangeOrigin {
        /// Stored unit vector in the token-defined carrier plane.
        stored_vector: [f64; 2],
        /// Cylinder-axis unit direction.
        axis: [f64; 3],
        /// Unit direction from which the circumferential parameter is measured.
        reference_direction: [f64; 3],
        /// Origin of the stored partial circumferential interval.
        range_origin: f64,
    },
}

impl CatiaConsolidatedCylinderPayload {
    pub(crate) const fn layout(&self) -> u8 {
        match self {
            Self::Layout52 { .. } => 0x52,
            Self::Layout5a { .. } => 0x5a,
            Self::RangeOrigin { .. } => 0x62,
        }
    }
}

/// One structurally complete consolidated `B:28` cylinder chart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    try_from = "CatiaConsolidatedCylinderWire",
    into = "CatiaConsolidatedCylinderWire"
)]
pub struct CatiaConsolidatedCylinder {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Cylinder-axis origin.
    pub origin: [f64; 3],
    /// Cylinder radius.
    pub radius: f64,
    /// Arc-length circumferential interval.
    pub u_range: [f64; 2],
    /// Axial interval.
    pub v_range: [f64; 2],
    /// Layout-specific frame data.
    pub payload: CatiaConsolidatedCylinderPayload,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CatiaConsolidatedCylinderWire {
    id: String,
    byte_offset: u64,
    layout: u8,
    origin: [f64; 3],
    radius: f64,
    u_range: [f64; 2],
    v_range: [f64; 2],
    payload: CatiaConsolidatedCylinderPayloadWire,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CatiaConsolidatedCylinderPayloadWire {
    Resolved {
        frame_token: u8,
        axis: [f64; 3],
        reference_direction: [f64; 3],
    },
    RangeOrigin {
        stored_vector: [f64; 2],
        axis: [f64; 3],
        reference_direction: [f64; 3],
        range_origin: f64,
    },
}

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
            ) => CatiaConsolidatedCylinderPayload::Layout52 {
                frame_token,
                axis,
                reference_direction,
            },
            (
                0x5a,
                CatiaConsolidatedCylinderPayloadWire::Resolved {
                    frame_token,
                    axis,
                    reference_direction,
                },
            ) => CatiaConsolidatedCylinderPayload::Layout5a {
                frame_token,
                axis,
                reference_direction,
            },
            (
                0x62,
                CatiaConsolidatedCylinderPayloadWire::RangeOrigin {
                    stored_vector,
                    axis,
                    reference_direction,
                    range_origin,
                },
            ) => CatiaConsolidatedCylinderPayload::RangeOrigin {
                stored_vector,
                axis,
                reference_direction,
                range_origin,
            },
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
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedEmbeddedCylinder {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the embedded frame, including its varying pre-byte.
    pub byte_offset: u64,
    /// Owning type-3 consolidated group.
    pub group: String,
    /// Compact embedded object identity.
    pub object_id: u32,
    /// Cylinder-axis origin.
    pub origin: [f64; 3],
    /// Cylinder radius.
    pub radius: f64,
    /// Full-turn arc-length circumferential interval.
    pub u_range: [f64; 2],
    /// Axial interval.
    pub v_range: [f64; 2],
    /// Token selecting the serialized frame-vector role.
    pub frame_token: u8,
    /// Cylinder-axis unit direction.
    pub axis: [f64; 3],
    /// Unit direction from which the circumferential parameter is measured.
    pub reference_direction: [f64; 3],
}

/// Layout-specific scalar lane of a consolidated `B:18` parameter-space record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CatiaConsolidatedParameterPointPayload {
    /// One retained scalar after two zero tuple fields are elided.
    Scalar {
        /// Stored scalar.
        value: f64,
    },
    /// Two surface-chart coordinates.
    Uv {
        /// Surface-chart coordinates.
        uv: [f64; 2],
    },
    /// Host-chain station followed by two surface-chart coordinates.
    StationUv {
        /// Host-chain station.
        station: f64,
        /// Surface-chart coordinates.
        uv: [f64; 2],
    },
    /// Unsplit five-scalar lane.
    FiveScalars {
        /// Stored finite scalars.
        values: [f64; 5],
    },
}

impl CatiaConsolidatedParameterPointPayload {
    pub(crate) const fn layout(&self) -> u8 {
        match self {
            Self::Scalar { .. } => 0x0a,
            Self::Uv { .. } => 0x12,
            Self::StationUv { .. } => 0x1a,
            Self::FiveScalars { .. } => 0x2a,
        }
    }
}

#[cfg(test)]
impl CatiaConsolidatedParameterPointPayload {
    fn is_valid(&self) -> bool {
        match self {
            Self::Scalar { value } => value.is_finite(),
            Self::Uv { uv } => uv.iter().all(|value| value.is_finite()),
            Self::StationUv { station, uv } => {
                station.is_finite() && uv.iter().all(|value| value.is_finite())
            }
            Self::FiveScalars { values } => values.iter().all(|value| value.is_finite()),
        }
    }
}

/// One complete consolidated `B:18` parameter-space record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    try_from = "CatiaConsolidatedParameterPointWire",
    into = "CatiaConsolidatedParameterPointWire"
)]
pub struct CatiaConsolidatedParameterPoint {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Complete framed-record length.
    pub byte_len: u64,
    /// First byte of the two-byte class-specific prefix.
    pub prefix: crate::families::b2::records::B2ParameterPointPrefix,
    /// Second byte of the two-byte class-specific prefix.
    pub control: u8,
    /// Layout-specific finite scalar lane.
    pub payload: CatiaConsolidatedParameterPointPayload,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CatiaConsolidatedParameterPointWire {
    id: String,
    byte_offset: u64,
    byte_len: u64,
    layout: u8,
    prefix: u8,
    control: u8,
    payload: CatiaConsolidatedParameterPointPayload,
}

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
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CatiaConsolidatedPlaneCarrierPayload {
    /// Two-coordinate point, two-coordinate direction, and three tail scalars.
    PointDirection2 {
        /// In-plane point with the host-implied third coordinate omitted.
        point: [f64; 2],
        /// In-plane unit direction with its third component omitted.
        direction: [f64; 2],
        /// Complete trailing scalar lane.
        tail: [f64; 3],
    },
    /// Two-coordinate point, three-coordinate direction, and three tail scalars.
    PointDirection3 {
        /// In-plane point with the host-implied third coordinate omitted.
        point: [f64; 2],
        /// In-plane unit direction.
        direction: [f64; 3],
        /// Complete trailing scalar lane.
        tail: [f64; 3],
    },
    /// Two-coordinate point followed by four scalar values with no direction
    /// lane in this layout.
    PointTail {
        /// In-plane point with the host-implied third coordinate omitted.
        point: [f64; 2],
        /// Complete trailing scalar lane.
        tail: [f64; 4],
    },
    /// Finite scalar lane for a selector whose semantic layout is not yet
    /// established.
    ScalarLane {
        /// Second payload byte selecting the scalar layout.
        #[serde(skip, default)]
        selector: u8,
        /// Complete selector-specific scalar lane in source order.
        values: Vec<f64>,
    },
}

impl CatiaConsolidatedPlaneCarrierPayload {
    pub(crate) const fn selector(&self) -> u8 {
        match self {
            Self::PointDirection2 { .. } => 0xe4,
            Self::PointDirection3 { .. } => 0xc4,
            Self::PointTail { .. } => 0xec,
            Self::ScalarLane { selector, .. } => *selector,
        }
    }
}

/// One complete consolidated `B:27` plane-carrier record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    try_from = "CatiaConsolidatedPlaneCarrierWire",
    into = "CatiaConsolidatedPlaneCarrierWire"
)]
pub struct CatiaConsolidatedPlaneCarrier {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Complete framed-record length.
    pub byte_len: u64,
    /// Header-token width in bytes.
    pub width: u8,
    /// Independent frame flag.
    pub flag: u8,
    /// Width-coded frame header token.
    pub header_token: u32,
    /// Selector-specific finite scalar payload.
    pub payload: CatiaConsolidatedPlaneCarrierPayload,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CatiaConsolidatedPlaneCarrierWire {
    id: String,
    byte_offset: u64,
    byte_len: u64,
    width: u8,
    flag: u8,
    header_token: u32,
    selector: u8,
    payload: CatiaConsolidatedPlaneCarrierPayload,
}

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
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedReferenceList {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Compact persistent identities in serialization order.
    pub references: Vec<u32>,
}

/// One structurally complete consolidated `A/B:20` pcurve jet whose support
/// identity has not necessarily been resolved to a native surface record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedPcurve {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Consolidated framing family.
    pub family: CatiaConsolidatedFamily,
    /// Absolute persistent support-surface identity.
    pub support_id: u32,
    /// Parametric curve degree.
    pub degree: u32,
    /// Number of leading extrapolation sites.
    pub extrapolation_sites: u32,
    /// Strictly increasing native parameter sites.
    pub knots: Vec<f64>,
    /// Surface-chart positions at the parameter sites.
    pub points: Vec<[f64; 2]>,
    /// First derivatives at the parameter sites.
    pub first_derivatives: Vec<[f64; 2]>,
    /// Second derivatives at the parameter sites.
    pub second_derivatives: Vec<[f64; 2]>,
    /// Native evaluation interval.
    pub range: [f64; 2],
    /// Bytes following the evaluation interval in the framed payload.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub tail: Vec<u8>,
}

/// One structurally complete consolidated `B:2a` sphere chart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedSphere {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Sphere centre.
    pub center: [f64; 3],
    /// First transverse unit direction.
    pub direction_x: [f64; 3],
    /// Second transverse unit direction.
    pub direction_y: [f64; 3],
    /// Sphere-axis unit direction.
    pub axis: [f64; 3],
    /// Sphere radius.
    pub radius: f64,
    /// Active azimuth interval.
    pub azimuth_range: [f64; 2],
    /// Active latitude interval.
    pub latitude_range: [f64; 2],
}

/// One structurally complete consolidated `B:2b` torus chart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedTorus {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Torus centre.
    pub center: [f64; 3],
    /// First transverse unit direction.
    pub direction_x: [f64; 3],
    /// Second transverse unit direction.
    pub direction_y: [f64; 3],
    /// Torus-axis unit direction.
    pub axis: [f64; 3],
    /// Major radius.
    pub major_radius: f64,
    /// Minor radius.
    pub minor_radius: f64,
    /// Active major-angle interval.
    pub major_angular_range: [f64; 2],
    /// Full-turn major-angle chart domain.
    pub major_angular_domain: [f64; 2],
    /// Active minor-angle interval.
    pub minor_angular_range: [f64; 2],
    /// Full-turn minor-angle chart domain.
    pub minor_angular_domain: [f64; 2],
    /// Scale from major angle to stored U parameter.
    pub major_scale: f64,
    /// Scale from minor angle to stored V parameter.
    pub minor_scale: f64,
}

/// One exact consolidated B-family metric line profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedLineProfile {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Stored line origin.
    pub origin: [f64; 3],
    /// Unit line direction.
    pub direction: [f64; 3],
    /// Increasing stored parameter interval.
    pub range: [f64; 2],
}

/// One structurally complete consolidated `B:2d` surface-of-revolution record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedRevolution {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Reference-token dialect (`0x08` or `0x0a`).
    pub reference_token: u8,
    /// Unresolved consolidated allocation identity of the profile curve.
    pub profile_allocation_id: u16,
    /// Axis-frame origin.
    pub origin: [f64; 3],
    /// First transverse unit direction.
    pub direction_x: [f64; 3],
    /// Second transverse unit direction.
    pub direction_y: [f64; 3],
    /// Revolution-axis unit direction.
    pub axis: [f64; 3],
    /// Stored full-turn angular parameter interval.
    pub angular_range: [f64; 2],
    /// Stored profile parameter interval.
    pub profile_range: [f64; 2],
    /// Unique consolidated circle with the same stored profile interval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_circle: Option<String>,
    /// Positive scale from revolution angle to stored angular parameter.
    pub angular_scale: f64,
}

/// One structurally complete consolidated class-`0x61` record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedClass61Record {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Width-coded header token.
    pub header_token: u32,
    /// Counted or long-form record payload.
    pub payload: CatiaConsolidatedClass61Payload,
}

/// One complete consolidated B-family class-`0x5b` or class-`0x5c` record.
///
/// These records retain their source-local control lane. The payload has no
/// assigned semantic fields or cross-source identity relation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedClass5b5cRecord {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Zero-based bounded record-source ordinal.
    pub source_index: u64,
    /// Logical offset within the bounded record source.
    pub source_offset: u64,
    /// Complete framed-record byte length.
    pub byte_len: u64,
    /// Header-token width in bytes.
    pub width: u8,
    /// Independent frame flag.
    pub flag: u8,
    /// Record class (`0x5b` or `0x5c`).
    pub class: u8,
    /// Width-coded frame header token.
    pub header_token: u32,
    /// Complete opaque payload in source order.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub payload: Vec<u8>,
}

/// Structurally decoded payload of a consolidated class-`0x61` record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CatiaConsolidatedClass61Payload {
    /// Count-selected compact reference lane followed by a class-specific tail.
    Counted {
        /// Compact identities in serialization order.
        references: Vec<u32>,
        /// Complete nonempty tail, including terminal byte `0x03`.
        #[serde(with = "cadmpeg_ir::bytes")]
        #[cfg_attr(feature = "schema", schemars(with = "String"))]
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
        scalar: f64,
    },
}

/// One typed consolidated class-`0x60` group opener.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedGroup {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Compact group-type code.
    pub group_type: u32,
}

/// One complete consolidated cone-face chart descriptor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedConeFace {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Complete framed-record length.
    pub byte_len: u64,
    /// Complete reference-and-control program preceding the scalars.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub program: Vec<u8>,
    /// Stored angular chart scale.
    pub angular_scale: f64,
    /// Cone half-angle in radians.
    pub half_angle: f64,
    /// Complete immediately following parameter-point run.
    pub parameter_points: Vec<String>,
}

/// One complete consolidated historical edge run referencing two retained
/// pcurve records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedEdgeRun {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the first pcurve frame.
    pub byte_offset: u64,
    /// Retained pcurve identities in serialized side order.
    pub pcurves: [String; 2],
    /// Shared native parameter interval.
    pub parameter_range: [f64; 2],
    /// Shared geometric tolerance.
    pub tolerance: f64,
    /// Exact terminal edge node.
    pub node: String,
    /// Uniquely resolved support carrier for each pcurve side.
    #[serde(default)]
    pub support_bindings: [Option<CatiaConsolidatedSupportBinding>; 2],
    /// Index-aligned 3D loci shared by every resolved support side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_loci: Option<Vec<[f64; 3]>>,
    /// First and last shared loci in endpoint pair direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_loci: Option<[[f64; 3]; 2]>,
}

/// Wire addressing form of one width-coded allocation reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatiaAllocationReferenceEncoding {
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
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatiaOwnerIdentityEncoding {
    /// One token from the allocation-reference grammar.
    Allocation(CatiaAllocationReferenceEncoding),
    /// Raw one-byte weak identity in the width-coded alternating dialect.
    RawU8,
}

/// One structurally complete width-coded class-`0x5e` edge node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedEdgeNode {
    /// Stable native-record identity.
    pub id: String,
    /// Record byte offset.
    pub byte_offset: u64,
    /// Zero-based bounded record-source ordinal.
    pub source_index: usize,
    /// Header-token width in bytes.
    pub width: u8,
    /// Independent framing flag.
    pub flag: u8,
    /// Width-coded header token.
    pub header_token: u32,
    /// Owning compact class-`0x62` packet, when selected by its allocation roster.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allocation_owner: Option<String>,
    /// Zero-based frame ordinal after the compact owner packet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allocation_ordinal: Option<u32>,
    /// Allocation-local curve-support reference.
    pub curve_ref: u32,
    /// Middle reference pair. These are endpoint addresses only when an
    /// allocation walk or complete edge-use run proves that layout.
    pub vertex_refs: [u32; 2],
    /// Resolved structural endpoint records in edge direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_records: Option<[u64; 2]>,
    /// Retained vertex identities in edge direction. Empty strings mean that
    /// the five-reference layout remains unresolved.
    pub vertices: [String; 2],
    /// Final reference pair. Complete edge-use runs interpret these as
    /// allocation-local side selectors; other layouts retain them untyped.
    pub parameter_selectors: [u32; 2],
    /// Wire addressing forms of curve, vertex, and parameter references.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_encodings: Option<[CatiaAllocationReferenceEncoding; 5]>,
    /// Decoded value of the one-byte terminal allocation reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_value: Option<u32>,
    /// Wire addressing form of the terminal allocation reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_encoding: Option<CatiaAllocationReferenceEncoding>,
    /// Terminal layout byte.
    pub tail: u8,
    /// Adjacent class-`0x23..=0x25` edge-definition frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<CatiaConsolidatedEdgeDefinition>,
    /// Adjacent oriented uses whose references close on this edge node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uses: Option<CatiaConsolidatedEdgeUses>,
    /// Analytic circle carrier structurally bound by an adjacent six-record run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analytic_circle: Option<CatiaConsolidatedAnalyticCircleBinding>,
    /// Typed class-`0x18` descriptor bound to a class-`0x25` edge run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class25_descriptor: Option<CatiaConsolidatedClass25Descriptor>,
}

/// Typed class-`0x18` descriptor bound to a class-`0x25` edge definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedClass25Descriptor {
    /// Record byte offset.
    pub byte_offset: u64,
    /// Width-coded allocation identity.
    pub record_id: u32,
    /// Descriptor control byte.
    pub control: u8,
    /// Complete finite scalar lane.
    pub values: Vec<f64>,
}

/// Descriptor and circle relation structurally bound to an analytic edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedAnalyticCircleBinding {
    /// Exact class-`0x18` descriptor frame.
    pub descriptor: CatiaConsolidatedAnalyticCircleDescriptor,
    /// Referenced consolidated circle support.
    pub circle: String,
}

/// Exact class-`0x18` descriptor frame attached to an analytic circle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedAnalyticCircleDescriptor {
    /// Record byte offset.
    pub byte_offset: u64,
    /// Header-token width in bytes.
    pub width: u8,
    /// Independent framing flag.
    pub flag: u8,
    /// Width-coded header token.
    pub header_token: u32,
    /// Complete class-specific payload.
    pub payload: Vec<u8>,
}

/// Exact class-specific edge-definition frame owned by one consolidated edge node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedEdgeDefinition {
    /// Record byte offset.
    pub byte_offset: u64,
    /// Header-token width in bytes.
    pub width: u8,
    /// Independent framing flag.
    pub flag: u8,
    /// Edge-definition class in `0x23..=0x25`.
    pub class: u8,
    /// Width-coded header token.
    pub header_token: u32,
    /// Complete class-specific payload.
    pub payload: Vec<u8>,
    /// Structurally decoded class-specific payload. Reuses the consolidated
    /// family enum directly: it is serialization-identical (same variant and
    /// field names, no id/offset decoration), so no native restatement is
    /// needed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<crate::families::consolidated::records::ConsolidatedEdgeDefinitionData>,
}

/// Exact oriented-use allocation chain owned by one consolidated edge node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedEdgeUses {
    /// Counted allocation-reference vectors in side order.
    pub references: [[u32; 2]; 2],
    /// Terminal side-use sense bytes in serialized order.
    pub senses: [u8; 2],
}

/// One endpoint identity retained by consolidated topology edge nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConsolidatedVertexIdentity {
    /// Stable native-record identity assigned in first-incidence order.
    pub id: String,
    /// First raw endpoint-address operand associated with this identity.
    pub identity: u32,
    /// Bounded record source that owns this identity namespace.
    pub source_index: usize,
    /// Resolved structural endpoint record, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_record: Option<u64>,
    /// Raw endpoint-address operands associated with this identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference_values: Vec<u32>,
    /// Compact allocation scope for the identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allocation_owner: Option<String>,
    /// Incident consolidated edge nodes in source order.
    pub incident_edge_nodes: Vec<String>,
}

/// Exact carrier selected for one side of a consolidated historical edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CatiaConsolidatedSupportBinding {
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
        offset: f64,
    },
}

/// One complete outer FINJPL segment retained with its framing identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaFinjplSegment {
    /// Globally unique segment identity.
    pub id: String,
    /// FINJPL marker offset in the complete file.
    pub byte_offset: u64,
    /// Complete segment byte length.
    pub byte_len: u64,
    /// Big-endian segment type word.
    pub type_word: u32,
    /// Structural type family.
    pub family: String,
    /// Stored primary name, when the printable-ASCII name form is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Complete segment bytes from marker through the byte before the next segment.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub data: Vec<u8>,
}

/// One external CATIA document selected by a storage-property record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaExternalReference {
    /// Globally unique reference identity.
    pub id: String,
    /// File offset of the length-prefixed target string.
    pub byte_offset: u64,
    /// Referenced CATIA document name or path.
    pub target: String,
    /// Containing project-flags FINJPL segment.
    pub segment: String,
}

/// One exact JPEG preview from the outer summary-information segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaPreviewImage {
    /// Globally unique preview identity.
    pub id: String,
    /// JPEG SOI byte offset in the complete file.
    pub byte_offset: u64,
    /// Exact encoded length through JPEG EOI.
    pub byte_len: u64,
    /// Pixel width from the JPEG start-of-frame segment.
    pub width: u16,
    /// Pixel height from the JPEG start-of-frame segment.
    pub height: u16,
    /// JPEG component count.
    pub components: u8,
    /// Exact JPEG byte stream.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub data: Vec<u8>,
}

/// One exact outer `01 00 04 00` alias-row core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaAliasRow {
    /// Globally unique alias-row identity.
    pub id: String,
    /// Byte offset of the four-byte alias marker.
    pub byte_offset: u64,
    /// Classification of the preceding four-byte word.
    pub lead: AliasLead,
    /// Complete preceding four-byte word.
    pub lead_raw: u32,
    /// Low 24 bits of the stored tag word.
    pub tag: u32,
    /// Complete stored tag word.
    pub tag_raw: u32,
    /// Single-byte row flag.
    pub flag: u8,
    /// Complete three-byte F1 field.
    pub f1: [u8; 3],
    /// One-based object-graph record ordinal carried by F1.
    pub entity_record_ordinal: u8,
    /// Primary object graph selected by the valid F1 ordinal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_graph: Option<String>,
    /// One-based F1 ordinal resolved to its exact `7C09` record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_record: Option<String>,
    /// Design object owning the selected record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_object: Option<String>,
    /// First trailing fixed-width field.
    pub f2: u32,
    /// Second trailing fixed-width field.
    pub f3: u32,
    /// Group-allocation header immediately preceding this alias core.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<AliasGroupMembership>,
    /// Canonical persistent surface-roster tag selected by this alias row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_surface_tag: Option<u32>,
}

/// One exact `7C0B` value block adjacent to its source-schema catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaValueBlock {
    /// Globally unique value-block identity.
    pub id: String,
    /// Byte offset of the `7C0B` marker.
    pub byte_offset: u64,
    /// Complete framed extent including the trailing terminator.
    pub byte_len: u64,
    /// Stored length from the marker through the byte before the terminator.
    pub declared_len: u64,
    /// Object graph ending exactly where this value block begins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_graph: Option<String>,
    /// Source-schema catalog that begins immediately after this block.
    pub catalog: String,
    /// Value payload in serialized order.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub payload: Vec<u8>,
    /// Lossless typed fields in payload order.
    #[serde(default)]
    pub fields: Vec<value_block::ValueField>,
    /// Schema selectors in payload order, resolved against the adjacent catalog.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schema_selections: Vec<CatiaValueSchemaSelection>,
}

/// One `0x32` selector from a value block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaValueSchemaSelection {
    /// Globally unique schema-selection identity.
    pub id: String,
    /// Containing [`CatiaValueBlock`] identity.
    pub parent: String,
    /// Byte offset within the value payload.
    pub offset: u64,
    /// Stored zero-based ordinal or terminal absent-schema sentinel.
    pub ordinal: u32,
    /// Selected catalog entry; absent for the terminal sentinel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// UTF-8 source-schema name stored by the selected entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Complete encoded value after this selector and before the next selector.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub encoded_value: Vec<value_block::ValueField>,
}

/// One exact `7C02` source-schema catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaCatalog {
    /// Globally unique catalog identity.
    pub id: String,
    /// Byte offset of the `7C02` marker.
    pub byte_offset: u64,
    /// Total framed byte length.
    pub byte_len: u64,
    /// Stored count, equal to the entry population plus one.
    pub declared_count: u32,
    /// Catalog entries in serialized order.
    #[serde(default)]
    pub entries: Vec<CatiaCatalogEntry>,
}

/// One source-schema name from a [`CatiaCatalog`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaCatalogEntry {
    /// Globally unique catalog-entry identity.
    pub id: String,
    /// Containing [`CatiaCatalog`] identity.
    pub parent: String,
    /// Stable serialized order within the catalog.
    pub ordinal: u32,
    /// Byte offset of the inclusive length field.
    pub byte_offset: u64,
    /// Decoded ASCII schema name.
    pub value: String,
}

/// One definition selector resolved against an object graph's source schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaDefinitionSchemaSelection {
    /// Byte offset of the selector marker within the definition prefix.
    pub offset: u64,
    /// Stored zero-based source-schema ordinal.
    pub ordinal: u32,
    /// Selected catalog entry when the ordinal is in range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// UTF-8 source-schema name stored by the selected entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// One schema selector and its following encoded `7C07` value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaEntityValueSchemaSelection {
    /// Byte offset of the selector marker within the value payload.
    pub offset: u64,
    /// Stored zero-based source-schema ordinal.
    pub ordinal: u32,
    /// Selected catalog entry.
    pub entry: String,
    /// UTF-8 source-schema name stored by the selected entry.
    pub name: String,
    /// Complete token sequence after this selector and before the next selector.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub encoded_value: Vec<value_block::ValueField>,
    /// Exact packets wholly contained by `encoded_value`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packets: Vec<entity_table::EntityValuePacket>,
}

/// One repeated-reference preamble selector resolved through its graph catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaRepeatedReferenceSchemaSelection {
    /// Serialized order of the blob and schema ordinal.
    pub order: CatiaRepeatedReferenceSchemaOrder,
    /// Byte offset of the schema ordinal within the payload.
    pub offset: u64,
    /// Stored zero-based source-schema ordinal.
    pub ordinal: u32,
    /// Selected catalog entry when the ordinal is in range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// UTF-8 source-schema name stored by the selected entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// One exact schema selector used by a typed entity program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaEntitySchemaValue {
    /// Byte offset of the selector within its definition or value payload.
    #[serde(default)]
    pub offset: u64,
    /// Stored zero-based source-schema ordinal.
    #[serde(default)]
    pub ordinal: u32,
    /// Selected source-schema entry.
    pub entry: String,
    /// UTF-8 value stored by the selected entry.
    pub value: String,
}

/// One complete relation-expression value program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaRelationExpression {
    /// Exact wire framing and its framing-specific roles.
    pub framing: CatiaRelationExpressionFraming,
    /// Stored source expression selected by the second value field.
    pub expression: CatiaEntitySchemaValue,
    /// Exact `param` role selector.
    pub parameter_role: CatiaEntitySchemaValue,
    /// Stored source type signature.
    pub type_signature: CatiaEntitySchemaValue,
    /// Parsed parameter and value types when the source signature has the typed form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<CatiaRelationTypeSignature>,
    /// Exact `RelationExpFct` function selector.
    pub function_role: CatiaEntitySchemaValue,
}

/// Mutually exclusive role framing of one relation-expression value program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum CatiaRelationExpressionFraming {
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
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaRelationTypeSignature {
    /// Ordered expression-local inputs named inside the signature.
    pub inputs: Vec<CatiaRelationTypeInput>,
    /// Source result type named after the closing parenthesis.
    pub result_type: String,
}

/// One typed input clause in a relation-expression source signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaRelationTypeInput {
    /// Expression-local parameter named before `#In`.
    pub parameter: String,
    /// Source input type named after `#In`.
    pub input_type: String,
}

/// Evaluation state of one complete entity-record suffix value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum CatiaEntityEvaluation {
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
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum CatiaEntityEvaluationEncoding {
    /// The evaluation opcode directly precedes its payload.
    Direct,
    /// `E6 00 00 00` precedes the scalar's `E6` opcode.
    ZeroPaddedScalar,
}

/// Payload of one complete entity-record suffix value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum CatiaEntitySuffixPayload {
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
        value: CatiaEntitySuffixSelectedValue,
    },
    /// One zero-payload `E8` control state.
    ControlE8,
    /// One zero-payload `E9` control state.
    ControlE9,
    /// One zero-payload `37` separator.
    Separator37,
}

/// Typed value following a source-schema selector in an entity-record suffix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum CatiaEntitySuffixSelectedValue {
    /// One canonical one-byte atom.
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
    /// One further source-schema selector.
    SchemaSelector {
        /// Byte offset of the selector marker within the record suffix.
        #[serde(default)]
        offset: u64,
        /// Stored zero-based source-schema ordinal.
        ordinal: u32,
    },
}

/// Exact trailer framing of one complete entity-record suffix value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum CatiaEntitySuffixTrailer {
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
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaEntitySuffixValue {
    /// Three canonical compact atoms preceding the field code.
    pub prefix_atoms: [u32; 3],
    /// Stored width of each prefix atom.
    pub prefix_atom_widths: [u8; 3],
    /// Exact field code preceding the payload.
    pub prefix_code: u8,
    /// Stored suffix payload.
    pub payload: CatiaEntitySuffixPayload,
    /// Exact framing closing the suffix production.
    pub trailer: CatiaEntitySuffixTrailer,
}

/// State byte following one escaped word in an entity-record suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum CatiaEntitySuffixEscapedWordState {
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
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaEntitySuffixEscapedWord {
    /// Fixed-width little-endian word following the `80` escape.
    pub word: u32,
    /// Exact trailing state.
    pub state: CatiaEntitySuffixEscapedWordState,
}

/// One complete non-value entity-record suffix framing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum CatiaEntitySuffixFraming {
    /// One escaped fixed-width word followed by an exact state.
    EscapedWord(CatiaEntitySuffixEscapedWord),
    /// Standalone token `81 49`.
    Token8149,
    /// Standalone fixed frame `FE F6 <payload[16]>`.
    FixedFeF6 {
        /// Exact fixed-width payload.
        #[serde(with = "cadmpeg_ir::bytes")]
        #[cfg_attr(feature = "schema", schemars(with = "String"))]
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
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaEntitySuffixSchemaSelection {
    /// Byte offset of the selector marker within the record suffix.
    #[serde(default)]
    pub offset: u64,
    /// Stored zero-based source-schema ordinal.
    pub ordinal: u32,
    /// Selected catalog entry.
    pub entry: String,
    /// UTF-8 source-schema name stored by the selected entry.
    pub name: String,
    /// Typed value following the selector, with nested schema resolution.
    pub value: CatiaEntitySuffixSchemaValue,
}

/// Catalog-resolved value following an entity-suffix schema selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum CatiaEntitySuffixSchemaValue {
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
        /// Selected catalog entry when the ordinal is in range.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entry: Option<String>,
        /// UTF-8 source-schema name stored by the selected entry.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
}

/// One complete named parameter-value record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaParameterValue {
    /// Stored parameter name.
    pub name: CatiaEntitySchemaValue,
    /// Stored scope, expression, or presentation binding.
    pub binding: CatiaEntitySchemaValue,
    /// Stored evaluation state.
    pub evaluation: CatiaEntityEvaluation,
    /// Byte offset of the evaluation opcode within the record suffix.
    #[serde(default)]
    pub evaluation_opcode_offset: u64,
}

/// One complete source-schema `Range` interval carried by an entity value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaRangeInterval {
    /// Exact source-schema selector naming `Range`.
    pub range: CatiaEntitySchemaValue,
    /// Complete selected interval framing and nullable slots.
    pub interval: entity_table::RangeInterval,
    /// Finite nominal carried by an admitted scalar suffix dialect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nominal: Option<CatiaRangeNominal>,
    /// Exact same-graph payload-reference occurrences selecting this interval.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incoming_references: Vec<CatiaEntityIncomingReference>,
    /// Exact same-graph object-head storage selectors selecting this interval.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incoming_storage_references: Vec<CatiaEntityIncomingStorageReference>,
}

/// Exact scalar-suffix dialect associating a nominal with a `Range` interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum CatiaRangeNominalFraming {
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
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaRangeNominal {
    /// Exact scalar-suffix dialect.
    pub framing: CatiaRangeNominalFraming,
    /// Exact finite binary64 nominal bits.
    pub bits: u64,
    /// Byte offset of `E6` within the record suffix.
    #[serde(default)]
    pub evaluation_opcode_offset: u64,
}

/// Exact framing of one complete constraint-range value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum CatiaConstraintRangeFraming {
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
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaConstraintRange {
    /// Exact `Range` role selector.
    pub range: CatiaEntitySchemaValue,
    /// Exact constraint role selector encoded by `framing`.
    pub constraint: CatiaEntitySchemaValue,
    /// Exact role and prefix-code framing.
    pub framing: CatiaConstraintRangeFraming,
    /// Stored evaluation state.
    pub evaluation: CatiaEntityEvaluation,
    /// Byte offset of the evaluation opcode within the record suffix.
    #[serde(default)]
    pub evaluation_opcode_offset: u64,
    /// Exact same-graph payload-reference occurrences selecting this range.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incoming_references: Vec<CatiaEntityIncomingReference>,
    /// Exact same-graph object-head storage selectors selecting this range.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incoming_storage_references: Vec<CatiaEntityIncomingStorageReference>,
}

/// One exact payload-reference occurrence selecting an entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaEntityIncomingReference {
    /// Object record carrying the reference occurrence.
    pub object_record: String,
    /// Entity paired with the source object record when that record has an identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_entity: Option<CatiaEntityReference>,
    /// Byte offset of the reference field within that object's payload.
    pub payload_offset: u64,
    /// Structural container of the reference occurrence.
    pub source: CatiaObjectRecordReferenceSource,
}

/// One exact object-head storage selector selecting an entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaEntityIncomingStorageReference {
    /// Object record carrying the storage selector.
    pub object_record: String,
    /// Entity paired with the source object record when that record has an identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_entity: Option<CatiaEntityReference>,
}

/// One definition-selected entity whose complete value occupies its suffix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaDefinitionValue {
    /// Exact source-schema definition selected by the entity.
    pub definition: CatiaEntitySchemaValue,
    /// Complete typed suffix payload bound to the definition.
    pub payload: CatiaEntitySuffixPayload,
    /// Catalog-resolved selector when the payload is schema-selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_selection: Option<CatiaEntitySuffixSchemaSelection>,
}

/// One value selected through a complete two-definition role chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaDefinitionChainValue {
    /// Definition repeated by the suffix's fixed-width schema selector.
    pub selector: CatiaEntitySchemaValue,
    /// Second definition carrying the value's role within the selected schema.
    pub role: CatiaEntitySchemaValue,
    /// Stored selected value.
    pub value: CatiaEntitySuffixSchemaValue,
}

/// One complete formula relation stored by an entity and its object payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaFormulaRelation {
    /// Complete relation-expression incidence selected by the second payload reference.
    #[serde(default, deserialize_with = "deserialize_payload_entity_reference")]
    pub expression_entity: CatiaPayloadEntityReference,
    /// Output parameter incidence selected by the third payload reference.
    #[serde(default, deserialize_with = "deserialize_payload_entity_reference")]
    pub output_entity: CatiaPayloadEntityReference,
    /// Named parameter records selected by expression-local symbols, in occurrence order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameter_dependencies: Vec<CatiaRelationParameterDependency>,
}

/// One relation-expression symbol and every matching named parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaRelationParameterDependency {
    /// UTF-8 byte offset of this occurrence within the source expression.
    #[serde(default)]
    pub source_offset: u64,
    /// Exact expression-local symbol occurrence.
    pub symbol: String,
    /// Entity incidences carrying matching named parameter bindings.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_relation_dependency_candidates"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "Vec<CatiaEntityReference>"))]
    pub candidates: Vec<CatiaEntityReference>,
}

/// One declared relation-program input and its uniquely selected entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaRelationProgramInput {
    /// Expression-local parameter in signature order.
    pub parameter: String,
    /// Declared source value type.
    pub value_type: String,
    /// Unique same-graph named parameter selected by every source occurrence.
    pub entity: CatiaEntityReference,
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
                CatiaRelationDependencyCandidate::LegacyEntity(entity) => CatiaEntityReference {
                    entity: Some(entity),
                    ..CatiaEntityReference::default()
                },
            })
            .collect()
    })
}

/// One exact compound relation-program instance frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    try_from = "CatiaRelationProgramInstanceWire",
    into = "CatiaRelationProgramInstanceWire"
)]
pub struct CatiaRelationProgramInstance {
    /// Exact object-head and payload production with the framing-specific incidence.
    pub framing: CatiaRelationProgramInstanceFraming,
    /// Entity incidence carried by the frame's program slot.
    pub program_entity: CatiaEntityReference,
    /// Entity identity stored once as an atom and once as a reference.
    pub repeated_entity: CatiaEntityReference,
    /// Every reference occurrence in exact payload order, including repeated identities.
    pub reference_incidences: Vec<CatiaPayloadEntityReference>,
    /// Selected entity when it carries a complete relation-expression program.
    pub relation_expression: Option<String>,
    /// Named parameter records selected by expression-local symbols, in occurrence order.
    pub parameter_dependencies: Vec<CatiaRelationParameterDependency>,
    /// Complete declared inputs in signature order; absent when any binding is incomplete.
    pub inputs: Option<Vec<CatiaRelationProgramInput>>,
}

impl CatiaRelationProgramInstance {
    /// Same-graph incidence carried by the `ref(h)` slot of a lead-`12` frame.
    #[must_use]
    pub fn lead12_context_entity(&self) -> Option<&CatiaEntityReference> {
        match &self.framing {
            CatiaRelationProgramInstanceFraming::Lead12 { context_entity } => Some(context_entity),
            CatiaRelationProgramInstanceFraming::Lead54 { .. } => None,
        }
    }

    /// Trailing same-graph entity incidence carried only by lead-`54`.
    #[must_use]
    pub fn lead54_trailing_entity(&self) -> Option<&CatiaEntityReference> {
        match &self.framing {
            CatiaRelationProgramInstanceFraming::Lead54 { trailing_entity } => {
                Some(trailing_entity)
            }
            CatiaRelationProgramInstanceFraming::Lead12 { .. } => None,
        }
    }

    /// Result entity selected by the framing-specific `paramout` slot.
    #[must_use]
    pub fn output_entity(&self) -> Option<&CatiaEntityReference> {
        let slot = match &self.framing {
            CatiaRelationProgramInstanceFraming::Lead12 { context_entity } => context_entity,
            CatiaRelationProgramInstanceFraming::Lead54 { trailing_entity } => trailing_entity,
        };
        (slot.class_name.as_deref() == Some("paramout")).then_some(slot)
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct CatiaRelationProgramInstanceWire {
    #[serde(default)]
    framing: CatiaRelationProgramInstanceFramingTag,
    #[serde(default)]
    program_entity: CatiaEntityReference,
    #[serde(default)]
    repeated_entity: CatiaEntityReference,
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
#[cfg_attr(feature = "schema", derive(JsonSchema))]
enum CatiaRelationProgramInstanceFramingTag {
    #[default]
    Lead12,
    Lead54,
}

impl From<CatiaRelationProgramInstance> for CatiaRelationProgramInstanceWire {
    fn from(value: CatiaRelationProgramInstance) -> Self {
        let output_entity = value.output_entity().cloned();
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
            program_entity: value.program_entity,
            repeated_entity: value.repeated_entity,
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
            program_entity: wire.program_entity,
            repeated_entity: wire.repeated_entity,
            reference_incidences: wire.reference_incidences,
            relation_expression: wire.relation_expression,
            parameter_dependencies: wire.parameter_dependencies,
            inputs: wire.inputs,
        })
    }
}

/// One exact entity-reference occurrence in an object payload.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaPayloadEntityReference {
    /// Byte offset of the reference field within the object payload.
    pub payload_offset: u64,
    /// Stored entity identity and its same-graph resolution.
    pub reference: CatiaEntityReference,
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaEntityReference {
    /// Stored entity identity.
    pub entity_id: u32,
    /// The stored identity is the graph's terminal null identity.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_null: bool,
    /// Same-graph entity selected by that identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity: Option<String>,
    /// Class selected by the same-graph entity when its object record has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
}

/// One complete reference-signature packet and its same-graph entity incidences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaReferenceSignature {
    /// Exact complete packet production.
    #[serde(flatten)]
    pub production: entity_table::ReferenceSignature,
    /// Entity incidence selected by the first fixed-width reference.
    #[serde(default)]
    pub first_entity: CatiaEntityReference,
    /// Entity incidence selected by the second fixed-width reference.
    #[serde(default)]
    pub second_entity: CatiaEntityReference,
}

/// Source-ordered descriptor records sharing one exact reference pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaReferenceSignatureCohort {
    /// Globally unique cohort identity.
    pub id: String,
    /// Containing object graph.
    pub parent: String,
    /// Zero-based order of the cohort's first member within the graph.
    pub ordinal: u64,
    /// First identity shared by every member.
    pub first_reference: u32,
    /// Common same-graph incidence selected by the first identity.
    pub first_entity: CatiaEntityReference,
    /// Consecutive second identity shared by every member.
    pub second_reference: u32,
    /// Common same-graph incidence selected by the second identity.
    pub second_entity: CatiaEntityReference,
    /// Unique schema selected by descriptor-bearing members after `_SpecList`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_selection: Option<CatiaReferenceSignatureSchemaSelection>,
    /// Descriptor-bearing entity records in source order.
    pub members: Vec<String>,
}

/// Cohort-level schema incidence selected after the `_SpecList` marker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaReferenceSignatureSchemaSelection {
    /// Stored zero-based source-schema ordinal.
    pub ordinal: u32,
    /// Selected catalog entry.
    pub entry: String,
    /// UTF-8 source-schema name stored by the selected entry.
    pub name: String,
}

/// One exact self-defining schema-configuration `Configuration` object production.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaSchemaConfigurationRecord {
    /// Byte offset of the schema reference within the object payload.
    #[serde(default)]
    pub schema_payload_offset: u64,
    /// Stored value-schema ordinal selected by the first reference.
    pub schema_ordinal: u32,
    /// Selected schema-catalog entry.
    pub schema_entry: String,
    /// Selected schema-catalog name.
    pub schema_name: String,
    /// Entity selected by the second stored reference.
    #[serde(deserialize_with = "deserialize_payload_entity_reference")]
    pub entity_reference: CatiaPayloadEntityReference,
}

/// One exact schema-configuration `configrow` successor-link production.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaSchemaConfigurationRowLink {
    /// Stored class identity whose catalog name is `configrow`.
    pub class_reference: CatiaEntityReference,
    /// Byte offset of the successor atom within the object payload.
    #[serde(default)]
    pub successor_payload_offset: u64,
    /// Stored successor identity.
    pub successor: CatiaEntityReference,
}

/// One complete ordered schema-configuration chain formed by exact `configrow` links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaSchemaConfigurationRowChain {
    /// Stable identity derived from the graph and stored class identity.
    pub id: String,
    /// Object graph containing every row link.
    pub object_graph: String,
    /// Successor incidences in chain order from the root row.
    #[serde(default)]
    pub links: Vec<CatiaSchemaConfigurationRowChainLink>,
}

/// One ordered edge in a complete schema-configuration-row successor chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaSchemaConfigurationRowChainLink {
    /// Row entity carrying the successor occurrence.
    pub row: CatiaEntityReference,
    /// Byte offset of the successor atom within the row object's payload.
    pub successor_payload_offset: u64,
    /// Stored successor identity and its same-graph resolution.
    pub successor: CatiaEntityReference,
    /// Same-graph entities strictly between the row and successor.
    ///
    /// Absent when the successor does not follow the row in source order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intervening_entities: Option<Vec<CatiaEntityReference>>,
}

/// Exact framing production for a compound relation-program instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatiaRelationProgramInstanceFraming {
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
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum CatiaRepeatedReferenceSchemaOrder {
    /// The binary descriptor precedes the schema ordinal.
    BlobThenSchema,
    /// The schema ordinal precedes the binary descriptor.
    SchemaThenBlob,
}

/// One `7C05` entity-table record paired with a `7C09` object record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaEntityRecord {
    /// Globally unique entity-record identity.
    pub id: String,
    /// Object graph whose record occupies the same table position.
    pub object_graph: String,
    /// Positionally paired `7C09` object record.
    pub object_record: String,
    /// Stable serialized order within the table run.
    pub ordinal: u64,
    /// Byte offset of the `7C05` marker.
    pub byte_offset: u64,
    /// Total framed byte length.
    pub byte_len: u64,
    /// Byte between the `7C05` length and nested `7C06` marker.
    pub lead: u8,
    /// Complete alternate inline body, including its lead byte, when nested
    /// definition and value frames are absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
    pub inline_body: Option<Vec<u8>>,
    /// Stored nested `7C06` length.
    pub definition_len: u32,
    /// Exact definition prefix before the `0xEA` identity delimiter.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub definition_prefix: Vec<u8>,
    /// Definition selectors resolved against the containing graph's source schema.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub definition_schema_selections: Vec<CatiaDefinitionSchemaSelection>,
    /// Stored identity used by object-record owner and payload references.
    pub entity_id: u32,
    /// Exact definition bytes after the identity.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub definition_suffix: Vec<u8>,
    /// Stored nested `7C07` total length.
    pub value_len: u32,
    /// Exact nested `7C07` payload.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub value_payload: Vec<u8>,
    /// Lossless tokenization of the complete `7C07` payload.
    #[serde(default)]
    pub value_fields: Vec<value_block::ValueField>,
    /// Value selectors resolved against the containing graph's source schema.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub value_schema_selections: Vec<CatiaEntityValueSchemaSelection>,
    /// Complete relation-expression program carried by the value selectors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation_expression: Option<CatiaRelationExpression>,
    /// Complete named parameter-value production.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_value: Option<CatiaParameterValue>,
    /// Complete source-schema `Range` interval production.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range_interval: Option<CatiaRangeInterval>,
    /// Complete constraint-range production.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constraint_range: Option<CatiaConstraintRange>,
    /// Complete suffix value bound to the entity's sole definition selector.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_value: Option<CatiaDefinitionValue>,
    /// Complete value bound by a two-definition role chain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_chain_value: Option<CatiaDefinitionChainValue>,
    /// Complete compound relation-program instance frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation_program_instance: Option<CatiaRelationProgramInstance>,
    /// Exact self-defining schema-configuration `Configuration` object production.
    #[serde(
        default,
        alias = "configuration_record",
        skip_serializing_if = "Option::is_none"
    )]
    pub schema_configuration_record: Option<CatiaSchemaConfigurationRecord>,
    /// Exact `configrow` successor-link production.
    #[serde(
        default,
        alias = "configuration_row_link",
        skip_serializing_if = "Option::is_none"
    )]
    pub schema_configuration_row_link: Option<CatiaSchemaConfigurationRowLink>,
    /// Complete formula-to-expression and formula-to-parameter relation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula_relation: Option<CatiaFormulaRelation>,
    /// Exact packets in the value program, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub value_packets: Vec<entity_table::EntityValuePacket>,
    /// Complete nullable numeric pair when the entire `7C07` payload has that production.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numeric_pair: Option<entity_table::NumericPair>,
    /// Complete reference signature when the entire `7C07` payload has that production.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_signature: Option<CatiaReferenceSignature>,
    /// Exact bytes after the nested `7C07` frame.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub record_suffix: Vec<u8>,
    /// Complete typed value production occupying the record suffix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix_value: Option<CatiaEntitySuffixValue>,
    /// Complete non-value framing occupying the record suffix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix_framing: Option<CatiaEntitySuffixFraming>,
    /// Fixed-width suffix selector resolved through the containing graph's catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix_schema_selection: Option<CatiaEntitySuffixSchemaSelection>,
}

/// One outer `7C08` ownership graph in source order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaObjectGraph {
    /// Globally unique graph identity.
    pub id: String,
    /// Byte offset of the `7C08` root.
    pub byte_offset: u64,
    /// Total framed byte length.
    pub byte_len: u64,
    /// Physically containing FINJPL segment, when the graph is not in the outer preamble.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finjpl_segment: Option<String>,
    /// Exact declared outer container whose physical stream contains this graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_container: Option<CatiaOuterContainerBinding>,
    /// Byte offset of the associated schema catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_byte_offset: Option<u64>,
    /// Associated schema catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<String>,
    /// Consecutive `7C09` records in serialized order.
    #[serde(default)]
    pub records: Vec<CatiaObjectRecord>,
}

/// Outer `Data` declaration and its selected physical stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaOuterContainerBinding {
    /// Byte offset of the declaration in the reconstructed outer `Data` stream.
    pub data_offset: u64,
    /// Source ordinal stored by the declaration.
    pub ordinal: u32,
    /// Concrete container class.
    pub class_name: String,
    /// Declared base container class.
    pub base_class: String,
    /// Resolved UUID-derived outer stream name.
    pub stream_name: String,
}

/// One `7C09` object record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaObjectRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Containing [`CatiaObjectGraph`] identity.
    pub parent: String,
    /// Design object selected by this record's owner entity identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_object: Option<String>,
    /// Positionally paired `7C05` entity-table record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_record: Option<String>,
    /// Stored entity-table identity used to select this record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<u32>,
    /// Stable serialized order within the graph.
    pub ordinal: u64,
    /// Byte offset of the `7C09` record.
    pub byte_offset: u64,
    /// Total framed byte length.
    pub byte_len: u64,
    /// First head byte.
    pub lead: u8,
    /// Decoded head tokens in serialized order.
    pub head: Vec<HeadToken>,
    /// Complete alternate inline body when the record has no nested `7C0A`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "Option<String>"))]
    pub inline_body: Option<Vec<u8>>,
    /// Structurally assigned owner slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<CatiaObjectOwner>,
    /// Head role identifying the per-file class ordinal.
    pub class_ref: Option<u32>,
    /// UTF-8 class name resolved through the graph's schema catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    /// Exact schema-catalog entry selected by `class_ref`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_entry: Option<String>,
    /// Head role selecting class-specific storage.
    pub storage_ref: Option<u32>,
    /// Same-graph field record selected by `storage_ref`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_record: Option<String>,
    /// Design object containing the selected storage record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_design_object: Option<String>,
    /// Typed nested payload, empty for an inline record.
    pub payload: ObjectPayload,
    /// Counted reference suffix when the payload repeats its reference prefix exactly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeated_reference_suffix: Option<object_graph::RepeatedReferenceSuffix>,
    /// Repeated-reference preamble selector resolved through the graph catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeated_reference_schema_selection: Option<CatiaRepeatedReferenceSchemaSelection>,
    /// Structural payload classification.
    pub subtype: PayloadSubtype,
    /// Ordered same-graph payload-reference links.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<CatiaObjectRecordReference>,
}

/// Structurally assigned owner role in a `7C09` head.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum CatiaObjectOwner {
    /// Stored entity identity selecting the design object.
    Entity(u32),
    /// Literal occupying the assigned slot without establishing ownership.
    UnassignedLiteral(u8),
}

impl CatiaObjectRecord {
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

/// One typed payload reference from a `7C09` record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaObjectRecordReference {
    /// Stored entity identity.
    pub entity_id: u32,
    /// Byte offset of the reference field within the payload.
    pub payload_offset: u64,
    /// Structural container of the reference occurrence.
    pub source: CatiaObjectRecordReferenceSource,
    /// The stored identity is the graph's terminal null identity.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_null: bool,
    /// Exact selected record; absent when the identity is outside the graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Design object owning the selected record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_object: Option<String>,
}

/// Structural container of one payload-reference occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum CatiaObjectRecordReferenceSource {
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
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaDesignClass {
    /// Selected source-schema entry.
    pub entry: String,
    /// UTF-8 class name stored by the entry.
    pub name: String,
}

/// One exact outbound relation occurrence in a grouped design object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaDesignObjectRelation {
    /// Field record containing the relation.
    pub source_field: String,
    /// Exact schema class of the source field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_class: Option<CatiaDesignClass>,
    /// Structural source of the relation occurrence.
    pub source: CatiaDesignObjectRelationSource,
    /// Stored target entity identity.
    pub target_entity_id: u32,
    /// Exact field record selected by the stored identity.
    pub target_field: String,
    /// Exact schema class of the selected target field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_class: Option<CatiaDesignClass>,
    /// Design object containing the selected field record, when it has an owner group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_design_object: Option<String>,
}

/// Structural source of one exact outbound relation occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum CatiaDesignObjectRelationSource {
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaDesignReferenceCell {
    /// Byte offset of the reference item within the source field's payload.
    #[serde(default)]
    pub payload_offset: u64,
    /// Stored target entity identity.
    pub entity_id: u32,
    /// The stored identity is the graph's terminal null identity.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_null: bool,
    /// Exact field record selected by the stored identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// Exact class of the selected field record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_class: Option<CatiaDesignClass>,
    /// Design object containing the selected field record, when it has an owner group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_object: Option<String>,
}

/// One source-ordered row in a parallel design-object reference table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaDesignReferenceRow {
    /// Cells in the order of the table's source fields.
    pub cells: Vec<CatiaDesignReferenceCell>,
    /// Design object containing distinct selected fields whose classes equal every column class.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matching_design_object: Option<String>,
}

/// One source field and list framing forming a parallel-reference table column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaDesignReferenceColumn {
    /// Source field record containing the reference list.
    pub field: String,
    /// Exact source field class when its schema ordinal resolves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_class: Option<CatiaDesignClass>,
    /// Byte offset of the list tag within the source field's payload.
    #[serde(default)]
    pub list_payload_offset: u64,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaDesignParallelReferenceTable {
    /// Source field and list incidences forming the table's columns.
    #[serde(deserialize_with = "deserialize_design_reference_columns")]
    pub columns: Vec<CatiaDesignReferenceColumn>,
    /// Row-aligned reference cells.
    pub rows: Vec<CatiaDesignReferenceRow>,
}

/// One serialized design object formed by a shared `7C09` owner identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaDesignObject {
    /// Globally unique design-object identity.
    pub id: String,
    /// Containing [`CatiaObjectGraph`] identity.
    pub parent: String,
    /// Zero-based order of this owner group by its first field in the graph.
    pub ordinal: u64,
    /// Byte offset of the first field carrying this owner identity.
    pub first_field_byte_offset: u64,
    /// Owner entity identity stored by every field record.
    pub owner_entity_id: u32,
    /// Record selected by `owner_entity_id` when it lies inside the graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_record: Option<String>,
    /// Design object whose field set contains `owner_record`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_design_object: Option<String>,
    /// Exact class of a separator-form owner declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_class: Option<CatiaDesignClass>,
    /// Class-specific storage selector of a separator-form owner declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_storage_ref: Option<u32>,
    /// Field records carrying this owner identity, in serialized order.
    pub fields: Vec<String>,
    /// Distinct exact field classes, in first field order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field_classes: Vec<CatiaDesignClass>,
    /// Entity records carrying definition-bound values, in field order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub definition_values: Vec<String>,
    /// Entity records carrying two-definition values, in field order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub definition_chain_values: Vec<String>,
    /// Exact inter-object reference occurrences in field and payload order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<CatiaDesignObjectRelation>,
    /// Complete row-aligned table formed by parallel all-reference list fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_reference_table: Option<CatiaDesignParallelReferenceTable>,
}

fn design_objects(
    graphs: &[CatiaObjectGraph],
    entity_records: &[CatiaEntityRecord],
) -> Vec<CatiaDesignObject> {
    let definition_value_entities = entity_records
        .iter()
        .filter(|entity| entity.definition_value.is_some())
        .map(|entity| entity.id.as_str())
        .collect::<HashSet<_>>();
    let definition_chain_value_entities = entity_records
        .iter()
        .filter(|entity| entity.definition_chain_value.is_some())
        .map(|entity| entity.id.as_str())
        .collect::<HashSet<_>>();
    graphs
        .iter()
        .flat_map(|graph| {
            let record_indices = graph
                .records
                .iter()
                .enumerate()
                .filter_map(|(index, record)| Some((record.entity_id?, index)))
                .collect::<HashMap<_, _>>();
            let mut fields = Vec::<(u32, Vec<&CatiaObjectRecord>)>::new();
            let mut owner_indices = HashMap::<u32, usize>::new();
            for record in &graph.records {
                if let Some(owner) = record.owner_entity_id() {
                    let index = owner_indices.get(&owner).copied().unwrap_or_else(|| {
                        let index = fields.len();
                        fields.push((owner, Vec::new()));
                        owner_indices.insert(owner, index);
                        index
                    });
                    fields[index].1.push(record);
                }
            }
            let definition_value_entities = &definition_value_entities;
            let definition_chain_value_entities = &definition_chain_value_entities;
            fields
                .into_iter()
                .enumerate()
                .map(move |(ordinal, (owner_entity_id, records))| {
                    let owner_record = record_indices
                        .get(&owner_entity_id)
                        .and_then(|index| graph.records.get(*index));
                    let id = design_object_id(graph.byte_offset, owner_entity_id);
                    CatiaDesignObject {
                        id: id.clone(),
                        parent: graph.id.clone(),
                        ordinal: ordinal as u64,
                        first_field_byte_offset: records[0].byte_offset,
                        owner_entity_id,
                        owner_record: owner_record.map(|record| record.id.clone()),
                        owner_design_object: owner_record
                            .and_then(CatiaObjectRecord::owner_entity_id)
                            .filter(|owner| {
                                *owner != owner_entity_id && owner_indices.contains_key(owner)
                            })
                            .map(|owner| design_object_id(graph.byte_offset, owner)),
                        owner_class: owner_record
                            .filter(|record| record_has_separator_roles(record))
                            .and_then(design_class),
                        owner_storage_ref: owner_record
                            .filter(|record| record_has_separator_roles(record))
                            .and_then(|record| record.storage_ref),
                        fields: records.iter().map(|record| record.id.clone()).collect(),
                        field_classes: records
                            .iter()
                            .filter_map(|record| design_class(record))
                            .fold(Vec::new(), |mut classes, class| {
                                if !classes.contains(&class) {
                                    classes.push(class);
                                }
                                classes
                            }),
                        definition_values: records
                            .iter()
                            .filter_map(|record| record.entity_record.as_ref())
                            .filter(|entity| definition_value_entities.contains(entity.as_str()))
                            .cloned()
                            .collect(),
                        definition_chain_values: records
                            .iter()
                            .filter_map(|record| record.entity_record.as_ref())
                            .filter(|entity| {
                                definition_chain_value_entities.contains(entity.as_str())
                            })
                            .cloned()
                            .collect(),
                        relations: records
                            .iter()
                            .flat_map(|record| {
                                let storage =
                                    record.storage_record.as_ref().and_then(|target_field| {
                                        let target_record = record_indices
                                            .get(&record.storage_ref?)
                                            .and_then(|index| graph.records.get(*index))?;
                                        let target_design_object =
                                            record.storage_design_object.clone();
                                        Some(CatiaDesignObjectRelation {
                                            source_field: record.id.clone(),
                                            source_class: design_class(record),
                                            source: CatiaDesignObjectRelationSource::Storage,
                                            target_entity_id: record.storage_ref?,
                                            target_field: target_field.clone(),
                                            target_class: design_class(target_record),
                                            target_design_object,
                                        })
                                    });
                                storage
                                    .into_iter()
                                    .chain(record.references.iter().filter_map(|reference| {
                                        let target_field = reference.target.as_ref()?.clone();
                                        let target_record = record_indices
                                            .get(&reference.entity_id)
                                            .and_then(|index| graph.records.get(*index))?;
                                        let target_design_object = reference.design_object.clone();
                                        Some(CatiaDesignObjectRelation {
                                            source_field: record.id.clone(),
                                            source_class: design_class(record),
                                            source: CatiaDesignObjectRelationSource::Payload {
                                                payload_offset: reference.payload_offset,
                                                container: reference.source.clone(),
                                            },
                                            target_entity_id: reference.entity_id,
                                            target_field,
                                            target_class: design_class(target_record),
                                            target_design_object,
                                        })
                                    }))
                            })
                            .collect(),
                        parallel_reference_table: design_parallel_reference_table(
                            &records,
                            graph,
                            &record_indices,
                        ),
                    }
                })
        })
        .collect()
}

fn design_parallel_reference_table(
    records: &[&CatiaObjectRecord],
    graph: &CatiaObjectGraph,
    record_indices: &HashMap<u32, usize>,
) -> Option<CatiaDesignParallelReferenceTable> {
    if records.len() < 2 {
        return None;
    }
    let columns = records
        .iter()
        .map(|record| {
            let [PayloadField::List {
                declared_count,
                items,
                offset: list_offset,
            }, middle @ .., PayloadField::Terminator] = record.payload.fields.as_slice()
            else {
                return None;
            };
            if *declared_count < 2
                || usize::try_from(*declared_count).ok() != Some(items.len())
                || !middle
                    .iter()
                    .all(|field| matches!(field, PayloadField::Atom { .. }))
            {
                return None;
            }
            let references = items
                .iter()
                .map(|item| match item {
                    ListItem::Reference { value, offset } => Some((*value, *offset)),
                    ListItem::Atom { .. } => None,
                })
                .collect::<Option<Vec<_>>>()?;
            Some((
                CatiaDesignReferenceColumn {
                    field: record.id.clone(),
                    field_class: design_class(record),
                    list_payload_offset: u64::try_from(*list_offset).ok()?,
                },
                references,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    let row_count = columns.first()?.1.len();
    let terminal_null_entity_id = terminal_null_entity_id(record_indices);
    if columns
        .iter()
        .any(|(_, references)| references.len() != row_count)
    {
        return None;
    }
    let rows = (0..row_count)
        .map(|row| {
            let cells = columns
                .iter()
                .map(|(_, references)| {
                    let (target_entity_id, payload_offset) = references[row];
                    let target = record_indices
                        .get(&target_entity_id)
                        .and_then(|index| graph.records.get(*index));
                    CatiaDesignReferenceCell {
                        payload_offset: u64::try_from(payload_offset)
                            .expect("bounded CATIA list-item offset fits u64"),
                        entity_id: target_entity_id,
                        is_null: Some(target_entity_id) == terminal_null_entity_id,
                        field: target.map(|record| record.id.clone()),
                        field_class: target.and_then(design_class),
                        design_object: target.and_then(|record| record.design_object.clone()),
                    }
                })
                .collect::<Vec<_>>();
            let matching_design_object = cells
                .first()
                .and_then(|cell| cell.design_object.clone())
                .filter(|member| {
                    let distinct_fields = cells
                        .iter()
                        .filter_map(|cell| cell.field.as_deref())
                        .collect::<HashSet<_>>();
                    columns.iter().zip(&cells).all(|((column, _), cell)| {
                        column.field_class.is_some()
                            && cell.field.is_some()
                            && cell.field_class.as_ref() == column.field_class.as_ref()
                            && cell.design_object.as_ref() == Some(member)
                    }) && distinct_fields.len() == cells.len()
                });
            CatiaDesignReferenceRow {
                cells,
                matching_design_object,
            }
        })
        .collect();
    Some(CatiaDesignParallelReferenceTable {
        columns: columns.into_iter().map(|(column, _)| column).collect(),
        rows,
    })
}

fn design_class(record: &CatiaObjectRecord) -> Option<CatiaDesignClass> {
    Some(CatiaDesignClass {
        entry: record.class_entry.clone()?,
        name: record.class_name.clone()?,
    })
}

fn record_has_separator_roles(record: &CatiaObjectRecord) -> bool {
    matches!(record.head.get(1), Some(HeadToken::Separator))
}

fn design_object_id(graph_offset: u64, owner_entity_id: u32) -> String {
    format!("catia:outer:design-object#{graph_offset:010}-{owner_entity_id:010}")
}

fn payload_references(
    payload: &ObjectPayload,
) -> impl Iterator<Item = (u32, usize, CatiaObjectRecordReferenceSource)> + '_ {
    payload.fields.iter().flat_map(|field| match field {
        PayloadField::Reference { value, offset } => {
            vec![(*value, *offset, CatiaObjectRecordReferenceSource::Field)]
        }
        PayloadField::List {
            declared_count,
            items,
            offset: list_offset,
        } if usize::try_from(*declared_count).ok() == Some(items.len()) => items
            .iter()
            .enumerate()
            .filter_map(|(item_ordinal, item)| match item {
                ListItem::Reference { value, offset } => Some((
                    *value,
                    *offset,
                    CatiaObjectRecordReferenceSource::ListItem {
                        list_payload_offset: u64::try_from(*list_offset)
                            .expect("bounded CATIA list offset fits u64"),
                        item_ordinal: u64::try_from(item_ordinal)
                            .expect("bounded CATIA list item ordinal fits u64"),
                    },
                )),
                ListItem::Atom { .. } => None,
            })
            .collect(),
        PayloadField::List { .. } => Vec::new(),
        PayloadField::Atom { .. }
        | PayloadField::Scalar { .. }
        | PayloadField::Blob { .. }
        | PayloadField::BulkTable { .. }
        | PayloadField::Sentinel { .. }
        | PayloadField::Terminator => Vec::new(),
    })
}

fn resolved_payload_references(
    payload: &ObjectPayload,
    record_ids: &[String],
    record_design_objects: &[Option<String>],
    record_indices: &HashMap<u32, usize>,
    terminal_null_entity_id: Option<u32>,
) -> Vec<CatiaObjectRecordReference> {
    payload_references(payload)
        .map(|(entity_id, payload_offset, source)| {
            let index = record_indices.get(&entity_id).copied();
            CatiaObjectRecordReference {
                entity_id,
                payload_offset: u64::try_from(payload_offset)
                    .expect("bounded CATIA payload offset fits u64"),
                source,
                is_null: Some(entity_id) == terminal_null_entity_id,
                target: index.and_then(|index| record_ids.get(index)).cloned(),
                design_object: index
                    .and_then(|index| record_design_objects.get(index))
                    .cloned()
                    .flatten(),
            }
        })
        .collect()
}

fn terminal_null_entity_id(record_indices: &HashMap<u32, usize>) -> Option<u32> {
    record_indices.keys().max()?.checked_add(1)
}

fn resolved_storage_link(
    storage_ref: Option<u32>,
    record_ids: &[String],
    record_design_objects: &[Option<String>],
    record_indices: &HashMap<u32, usize>,
) -> (Option<String>, Option<String>) {
    let Some(index) = storage_ref.and_then(|identity| record_indices.get(&identity).copied())
    else {
        return (None, None);
    };
    (
        record_ids.get(index).cloned(),
        record_design_objects.get(index).cloned().flatten(),
    )
}

fn definition_schema_selections(
    selectors: &[entity_table::DefinitionSchemaSelector],
    catalog: Option<&CatiaCatalog>,
) -> Vec<CatiaDefinitionSchemaSelection> {
    selectors
        .iter()
        .map(|selector| {
            let catalog_entry = usize::try_from(selector.value)
                .ok()
                .and_then(|ordinal| catalog?.entries.get(ordinal));
            CatiaDefinitionSchemaSelection {
                offset: selector.offset as u64,
                ordinal: selector.value,
                entry: catalog_entry.map(|entry| entry.id.clone()),
                name: catalog_entry.map(|entry| entry.value.clone()),
            }
        })
        .collect()
}

fn entity_value_schema_selections(
    fields: &[value_block::ValueField],
    catalog: Option<&CatiaCatalog>,
    packets: &[entity_table::EntityValuePacket],
) -> Vec<CatiaEntityValueSchemaSelection> {
    let Some(catalog) = catalog else {
        return Vec::new();
    };
    let selector_indices = fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| {
            let value_block::ValueField::SchemaSelector { ordinal, .. } = field else {
                return None;
            };
            usize::try_from(*ordinal)
                .ok()
                .filter(|ordinal| *ordinal < catalog.entries.len())
                .map(|_| index)
        })
        .collect::<Vec<_>>();
    selector_indices
        .iter()
        .enumerate()
        .filter_map(|(rank, index)| {
            let value_block::ValueField::SchemaSelector { ordinal, offset } = &fields[*index]
            else {
                return None;
            };
            let catalog_entry = usize::try_from(*ordinal)
                .ok()
                .and_then(|ordinal| catalog.entries.get(ordinal))?;
            let value_end = selector_indices
                .get(rank + 1)
                .copied()
                .unwrap_or(fields.len());
            let value_start_offset = fields.get(index + 1).map_or(usize::MAX, value_field_offset);
            let value_end_offset = fields.get(value_end).map_or(usize::MAX, value_field_offset);
            Some(CatiaEntityValueSchemaSelection {
                offset: *offset as u64,
                ordinal: *ordinal,
                entry: catalog_entry.id.clone(),
                name: catalog_entry.value.clone(),
                encoded_value: fields[index + 1..value_end].to_vec(),
                packets: packets
                    .iter()
                    .filter(|packet| {
                        packet.byte_range().is_some_and(|range| {
                            range.start >= value_start_offset && range.end <= value_end_offset
                        })
                    })
                    .cloned()
                    .collect(),
            })
        })
        .collect()
}

fn entity_suffix_schema_selection(
    suffix_value: Option<&CatiaEntitySuffixValue>,
    catalog: Option<&CatiaCatalog>,
) -> Option<CatiaEntitySuffixSchemaSelection> {
    let CatiaEntitySuffixPayload::SchemaSelected {
        selector_offset,
        selector,
        value,
    } = &suffix_value?.payload
    else {
        return None;
    };
    let entry = usize::try_from(*selector)
        .ok()
        .and_then(|ordinal| catalog?.entries.get(ordinal))?;
    let value = match value {
        CatiaEntitySuffixSelectedValue::Atom { value } => {
            CatiaEntitySuffixSchemaValue::Atom { value: *value }
        }
        CatiaEntitySuffixSelectedValue::Evaluation {
            opcode_offset,
            evaluation,
        } => CatiaEntitySuffixSchemaValue::Evaluation {
            opcode_offset: *opcode_offset,
            evaluation: evaluation.clone(),
        },
        CatiaEntitySuffixSelectedValue::ControlE8 => CatiaEntitySuffixSchemaValue::ControlE8,
        CatiaEntitySuffixSelectedValue::Separator37 => CatiaEntitySuffixSchemaValue::Separator37,
        CatiaEntitySuffixSelectedValue::SchemaSelector { offset, ordinal } => {
            let selected = usize::try_from(*ordinal)
                .ok()
                .and_then(|ordinal| catalog?.entries.get(ordinal));
            CatiaEntitySuffixSchemaValue::SchemaSelector {
                offset: *offset,
                ordinal: *ordinal,
                entry: selected.map(|entry| entry.id.clone()),
                name: selected.map(|entry| entry.value.clone()),
            }
        }
    };
    Some(CatiaEntitySuffixSchemaSelection {
        offset: *selector_offset,
        ordinal: *selector,
        entry: entry.id.clone(),
        name: entry.value.clone(),
        value,
    })
}

fn relation_expression(
    definitions: &[CatiaDefinitionSchemaSelection],
    values: &[CatiaEntityValueSchemaSelection],
) -> Option<CatiaRelationExpression> {
    let [definition0, definition1] = definitions else {
        return None;
    };
    if definition0.name.as_deref() != Some("body")
        || definition1.name.as_deref() != Some("body")
        || definition0.entry != definition1.entry
    {
        return None;
    }
    let schema_value = |selection: &CatiaEntityValueSchemaSelection| CatiaEntitySchemaValue {
        offset: selection.offset,
        ordinal: selection.ordinal,
        entry: selection.entry.clone(),
        value: selection.name.clone(),
    };
    let (framing, expression, parameter_role, type_signature, function_role, signature) =
        match values {
            [prefix_role, expression, parser_version_role, parameter_role, type_signature, state_role, function_role]
                if prefix_role.name == "Boolean"
                    && parser_version_role.name == "ParserVersion"
                    && parameter_role.name == "param"
                    && state_role.name == "opened"
                    && function_role.name == "RelationExpFct" =>
            {
                (
                    CatiaRelationExpressionFraming::OpenedBooleanParserVersion {
                        prefix_role: schema_value(prefix_role),
                        parser_version_role: schema_value(parser_version_role),
                        state_role: schema_value(state_role),
                    },
                    expression,
                    parameter_role,
                    type_signature,
                    function_role,
                    relation_type_signature(None, &type_signature.name),
                )
            }
            [placeholder, expression, parameter_role, type_signature, state_role, function_role]
                if parameter_role.name == "param"
                    && state_role.name == "opened"
                    && function_role.name == "RelationExpFct" =>
            {
                let placeholder = schema_value(placeholder);
                let signature =
                    relation_type_signature(Some(&placeholder.value), &type_signature.name);
                (
                    CatiaRelationExpressionFraming::PlaceholderState {
                        placeholder,
                        state_role: schema_value(state_role),
                    },
                    expression,
                    parameter_role,
                    type_signature,
                    function_role,
                    signature,
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
                        prefix_role: schema_value(prefix_role),
                        parser_version_role: schema_value(parser_version_role),
                    },
                    expression,
                    parameter_role,
                    type_signature,
                    function_role,
                    relation_type_signature(None, &type_signature.name),
                )
            }
            [expression, parser_version_role, parameter_role, type_signature, function_role]
                if parser_version_role.name == "ParserVersion"
                    && parameter_role.name == "param"
                    && function_role.name == "RelationExpFct" =>
            {
                (
                    CatiaRelationExpressionFraming::ParserVersion {
                        parser_version_role: schema_value(parser_version_role),
                    },
                    expression,
                    parameter_role,
                    type_signature,
                    function_role,
                    relation_type_signature(None, &type_signature.name),
                )
            }
            _ => return None,
        };
    Some(CatiaRelationExpression {
        framing,
        expression: schema_value(expression),
        parameter_role: schema_value(parameter_role),
        type_signature: schema_value(type_signature),
        signature,
        function_role: schema_value(function_role),
    })
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

fn relation_parameter_symbol(parameter: &str) -> bool {
    parameter
        .strip_prefix('#')
        .and_then(|parameter| parameter.strip_suffix('_'))
        .is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn parameter_value(
    lead: u8,
    values: &[CatiaEntityValueSchemaSelection],
    suffix_value: Option<&CatiaEntitySuffixValue>,
) -> Option<CatiaParameterValue> {
    if lead != 2 {
        return None;
    }
    let [name, binding] = values else {
        return None;
    };
    let suffix_value = suffix_value?;
    (suffix_value.prefix_atoms == [5, 22, 2]
        && suffix_value.prefix_atom_widths == [1, 1, 1]
        && suffix_value.prefix_code == 0x6a
        && suffix_value.trailer == CatiaEntitySuffixTrailer::Token8152)
        .then_some(())?;
    let CatiaEntitySuffixPayload::Evaluation {
        opcode_offset,
        evaluation,
        encoding: CatiaEntityEvaluationEncoding::Direct,
    } = &suffix_value.payload
    else {
        return None;
    };
    let schema_value = |selection: &CatiaEntityValueSchemaSelection| CatiaEntitySchemaValue {
        offset: selection.offset,
        ordinal: selection.ordinal,
        entry: selection.entry.clone(),
        value: selection.name.clone(),
    };
    Some(CatiaParameterValue {
        name: schema_value(name),
        binding: schema_value(binding),
        evaluation: evaluation.clone(),
        evaluation_opcode_offset: *opcode_offset,
    })
}

fn constraint_range(
    lead: u8,
    values: &[CatiaEntityValueSchemaSelection],
    suffix_value: Option<&CatiaEntitySuffixValue>,
) -> Option<CatiaConstraintRange> {
    if lead != 2 {
        return None;
    }
    let [range, constraint] = values else {
        return None;
    };
    if range.name != "Range" {
        return None;
    }
    let suffix_value = suffix_value?;
    if suffix_value.prefix_atoms != [4, 22, 2] || suffix_value.prefix_atom_widths != [1, 1, 1] {
        return None;
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
        _ => return None,
    };
    let CatiaEntitySuffixPayload::Evaluation {
        opcode_offset,
        evaluation,
        encoding: CatiaEntityEvaluationEncoding::Direct,
    } = &suffix_value.payload
    else {
        return None;
    };
    Some(CatiaConstraintRange {
        range: CatiaEntitySchemaValue {
            offset: range.offset,
            ordinal: range.ordinal,
            entry: range.entry.clone(),
            value: range.name.clone(),
        },
        constraint: CatiaEntitySchemaValue {
            offset: constraint.offset,
            ordinal: constraint.ordinal,
            entry: constraint.entry.clone(),
            value: constraint.name.clone(),
        },
        framing,
        evaluation: evaluation.clone(),
        evaluation_opcode_offset: *opcode_offset,
        incoming_references: Vec::new(),
        incoming_storage_references: Vec::new(),
    })
}

fn entity_incidences(
    records: &[CatiaObjectRecord],
    graph_id: &str,
    entity_id: u32,
) -> (
    Vec<CatiaEntityIncomingReference>,
    Vec<CatiaEntityIncomingStorageReference>,
) {
    let mut incoming_references = Vec::new();
    let mut incoming_storage_references = Vec::new();
    for record in records.iter().filter(|record| record.parent == graph_id) {
        incoming_references.extend(
            record
                .references
                .iter()
                .filter(|reference| reference.entity_id == entity_id)
                .map(|reference| CatiaEntityIncomingReference {
                    object_record: record.id.clone(),
                    source_entity: record.entity_id.map(|entity_id| CatiaEntityReference {
                        entity_id,
                        is_null: false,
                        entity: record.entity_record.clone(),
                        class_name: record.class_name.clone(),
                    }),
                    payload_offset: reference.payload_offset,
                    source: reference.source.clone(),
                }),
        );
        if record.storage_ref == Some(entity_id) {
            incoming_storage_references.push(CatiaEntityIncomingStorageReference {
                object_record: record.id.clone(),
                source_entity: record.entity_id.map(|entity_id| CatiaEntityReference {
                    entity_id,
                    is_null: false,
                    entity: record.entity_record.clone(),
                    class_name: record.class_name.clone(),
                }),
            });
        }
    }
    (incoming_references, incoming_storage_references)
}

fn range_interval(
    payload: &[u8],
    values: &[CatiaEntityValueSchemaSelection],
    suffix_value: Option<&CatiaEntitySuffixValue>,
    records: &[CatiaObjectRecord],
    graph_id: &str,
    entity_id: u32,
) -> Option<CatiaRangeInterval> {
    let mut matches = values
        .iter()
        .enumerate()
        .filter(|(_, selection)| selection.name == "Range");
    let (index, range) = matches.next()?;
    matches.next().is_none().then_some(())?;
    let start = usize::try_from(range.offset).ok()?.checked_add(5)?;
    let end = match values.get(index + 1) {
        Some(selection) => usize::try_from(selection.offset).ok()?,
        None => payload.len(),
    };
    let interval = entity_table::parse_range_interval(payload, start, end)?;
    let (incoming_references, incoming_storage_references) =
        entity_incidences(records, graph_id, entity_id);
    Some(CatiaRangeInterval {
        range: CatiaEntitySchemaValue {
            offset: range.offset,
            ordinal: range.ordinal,
            entry: range.entry.clone(),
            value: range.name.clone(),
        },
        interval,
        nominal: range_nominal(suffix_value),
        incoming_references,
        incoming_storage_references,
    })
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
    lead: u8,
    values: &[CatiaEntityValueSchemaSelection],
    suffix_value: Option<&CatiaEntitySuffixValue>,
    records: &[CatiaObjectRecord],
    graph_id: &str,
    entity_id: u32,
) -> Option<CatiaConstraintRange> {
    let mut range = constraint_range(lead, values, suffix_value)?;
    (range.incoming_references, range.incoming_storage_references) =
        entity_incidences(records, graph_id, entity_id);
    Some(range)
}

fn definition_value(
    lead: u8,
    definitions: &[CatiaDefinitionSchemaSelection],
    value_fields: &[value_block::ValueField],
    suffix_value: Option<&CatiaEntitySuffixValue>,
    suffix_schema_selection: Option<&CatiaEntitySuffixSchemaSelection>,
) -> Option<CatiaDefinitionValue> {
    if lead != 2
        || !matches!(
            value_fields,
            [value_block::ValueField::Terminator { offset: 0 }]
        )
    {
        return None;
    }
    let [definition] = definitions else {
        return None;
    };
    let suffix_value = suffix_value?;
    Some(CatiaDefinitionValue {
        definition: CatiaEntitySchemaValue {
            offset: definition.offset,
            ordinal: definition.ordinal,
            entry: definition.entry.clone()?,
            value: definition.name.clone()?,
        },
        payload: suffix_value.payload.clone(),
        schema_selection: suffix_schema_selection.cloned(),
    })
}

fn definition_chain_value(
    lead: u8,
    definitions: &[CatiaDefinitionSchemaSelection],
    value_fields: &[value_block::ValueField],
    suffix_value: Option<&CatiaEntitySuffixValue>,
    suffix_schema_selection: Option<&CatiaEntitySuffixSchemaSelection>,
) -> Option<CatiaDefinitionChainValue> {
    if lead != 2
        || !matches!(
            value_fields,
            [value_block::ValueField::Terminator { offset: 0 }]
        )
    {
        return None;
    }
    let [selector, role] = definitions else {
        return None;
    };
    let selector_value = CatiaEntitySchemaValue {
        offset: selector.offset,
        ordinal: selector.ordinal,
        entry: selector.entry.clone()?,
        value: selector.name.clone()?,
    };
    let role = CatiaEntitySchemaValue {
        offset: role.offset,
        ordinal: role.ordinal,
        entry: role.entry.clone()?,
        value: role.name.clone()?,
    };
    let suffix_schema_selection = suffix_schema_selection?;
    if suffix_schema_selection.entry != selector_value.entry
        || suffix_schema_selection.name != selector_value.value
    {
        return None;
    }
    let suffix_value = suffix_value?;
    let CatiaEntitySuffixPayload::SchemaSelected { .. } = &suffix_value.payload else {
        return None;
    };
    Some(CatiaDefinitionChainValue {
        selector: selector_value,
        role,
        value: suffix_schema_selection.value.clone(),
    })
}

fn entity_suffix_value(suffix: &[u8]) -> Option<CatiaEntitySuffixValue> {
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
    let prefix_code = *suffix.get(at)?;
    let payload_offset = at + 1;
    let (payload, trailer_offset) = if suffix.get(payload_offset..payload_offset + 5)
        == Some(&[0xe6, 0x00, 0x00, 0x00, 0xe6])
    {
        let bits = View::u64_le_at(suffix, payload_offset + 5)?;
        f64::from_bits(bits).is_finite().then_some(())?;
        (
            CatiaEntitySuffixPayload::Evaluation {
                opcode_offset: u64::try_from(payload_offset + 4).ok()?,
                evaluation: CatiaEntityEvaluation::Scalar { bits },
                encoding: CatiaEntityEvaluationEncoding::ZeroPaddedScalar,
            },
            payload_offset + 13,
        )
    } else if prefix_code == 0x32 {
        let selector = View::u32_le_at(suffix, payload_offset)?;
        let value_offset = payload_offset + 4;
        let (value, trailer_offset) = match *suffix.get(value_offset)? {
            0xe6 => {
                let bits = View::u64_le_at(suffix, value_offset + 1)?;
                f64::from_bits(bits).is_finite().then_some(())?;
                (
                    CatiaEntitySuffixSelectedValue::Evaluation {
                        opcode_offset: u64::try_from(value_offset).ok()?,
                        evaluation: CatiaEntityEvaluation::Scalar { bits },
                    },
                    value_offset + 9,
                )
            }
            0xe7 => (
                CatiaEntitySuffixSelectedValue::Evaluation {
                    opcode_offset: u64::try_from(value_offset).ok()?,
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
                    offset: u64::try_from(value_offset).ok()?,
                    ordinal: View::u32_le_at(suffix, value_offset + 1)?,
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
                selector_offset: u64::try_from(at).ok()?,
                selector,
                value,
            },
            trailer_offset,
        )
    } else {
        match *suffix.get(payload_offset)? {
            0xe7 => (
                CatiaEntitySuffixPayload::Evaluation {
                    opcode_offset: u64::try_from(payload_offset).ok()?,
                    evaluation: CatiaEntityEvaluation::Unset,
                    encoding: CatiaEntityEvaluationEncoding::Direct,
                },
                payload_offset + 1,
            ),
            0xe8 => (CatiaEntitySuffixPayload::ControlE8, payload_offset + 1),
            0xe9 => (CatiaEntitySuffixPayload::ControlE9, payload_offset + 1),
            0x37 => (CatiaEntitySuffixPayload::Separator37, payload_offset + 1),
            0xe6 => {
                let bits = View::u64_le_at(suffix, payload_offset + 1)?;
                f64::from_bits(bits).is_finite().then_some(())?;
                (
                    CatiaEntitySuffixPayload::Evaluation {
                        opcode_offset: u64::try_from(payload_offset).ok()?,
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
    let trailer = match suffix.get(trailer_offset..)? {
        [] => CatiaEntitySuffixTrailer::Empty,
        [0x81, 0x49] => CatiaEntitySuffixTrailer::Token8149,
        [0x81, 0x4a] => CatiaEntitySuffixTrailer::Token814A,
        [0x81, 0x52] => CatiaEntitySuffixTrailer::Token8152,
        [0x81, 0xdb] => CatiaEntitySuffixTrailer::Token81DB,
        [0x81, 0x92] => CatiaEntitySuffixTrailer::Token8192,
        [0x81, 0x93] => CatiaEntitySuffixTrailer::Token8193,
        [0xfe, 0xf6, rest @ ..] if rest.len() == 16 && rest.iter().all(|byte| *byte == 0) => {
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

pub(crate) fn entity_suffix_framing(suffix: &[u8]) -> Option<CatiaEntitySuffixFraming> {
    match suffix {
        [0x80, _, _, _, _, state] => {
            let state = match state {
                0x00 => CatiaEntitySuffixEscapedWordState::State00,
                0x01 => CatiaEntitySuffixEscapedWordState::State01,
                0x03 => CatiaEntitySuffixEscapedWordState::State03,
                0x04 => CatiaEntitySuffixEscapedWordState::State04,
                0x09 => CatiaEntitySuffixEscapedWordState::State09,
                _ => return None,
            };
            Some(CatiaEntitySuffixFraming::EscapedWord(
                CatiaEntitySuffixEscapedWord {
                    word: View::u32_le_at(suffix, 1)?,
                    state,
                },
            ))
        }
        [0x81, 0x49] => Some(CatiaEntitySuffixFraming::Token8149),
        [0xfe, 0xf6, payload @ ..] if payload.len() == 16 => {
            Some(CatiaEntitySuffixFraming::FixedFeF6 {
                payload: payload.to_vec(),
            })
        }
        [lead @ 0xd1..=0xe4, low, 0x01] => Some(CatiaEntitySuffixFraming::PagedAtomState01 {
            value: u32::from(*lead - 0xd1) * 256 + u32::from(*low) + 1,
        }),
        _ => None,
    }
}

fn relation_program_instance(
    entity_id: u32,
    object: &CatiaObjectRecord,
    entity_references: &CatiaEntityReferenceIndex<'_>,
    relation_expressions: &CatiaRelationExpressionEntityIndex,
    parameter_bindings: &CatiaParameterBindingIndex,
) -> Option<CatiaRelationProgramInstance> {
    if object.entity_id != Some(entity_id)
        || object.owner_entity_id().is_none()
        || object.class_ref.is_none()
    {
        return None;
    }
    let (framing, program_entity_id, repeated_reference_entity_id) =
        if object.lead == 0x12 && object.storage_ref.is_none() {
            let (program_entity_id, repeated_reference_entity_id, context_entity_id) =
                relation_program_instance_lead_12(entity_id, &object.payload.fields)?;
            (
                CatiaRelationProgramInstanceFraming::Lead12 {
                    context_entity: entity_reference(
                        &object.parent,
                        context_entity_id,
                        entity_references.entities,
                        entity_references.classes,
                        entity_references.terminal_nulls,
                    ),
                },
                program_entity_id,
                repeated_reference_entity_id,
            )
        } else if object.lead == 0x54 && object.storage_ref.is_some() {
            let (program_entity_id, repeated_reference_entity_id, trailing_entity_id) =
                relation_program_instance_lead_54(entity_id, &object.payload.fields)?;
            (
                CatiaRelationProgramInstanceFraming::Lead54 {
                    trailing_entity: entity_reference(
                        &object.parent,
                        trailing_entity_id,
                        entity_references.entities,
                        entity_references.classes,
                        entity_references.terminal_nulls,
                    ),
                },
                program_entity_id,
                repeated_reference_entity_id,
            )
        } else {
            return None;
        };
    let program_key = (object.parent.clone(), program_entity_id);
    let selected_expression = relation_expressions.get(&program_key);
    let parameter_dependencies = selected_expression
        .map(|expression| {
            relation_parameter_dependencies(&expression.source, &object.parent, parameter_bindings)
        })
        .unwrap_or_default();
    let inputs = selected_expression
        .and_then(|expression| expression.signature.as_ref())
        .and_then(|signature| resolved_relation_program_inputs(signature, &parameter_dependencies));
    let reference_incidences = object
        .payload
        .fields
        .iter()
        .filter_map(|field| match field {
            PayloadField::Reference { value, offset } => Some((*value, *offset)),
            _ => None,
        })
        .map(|(value, offset)| {
            Some(CatiaPayloadEntityReference {
                payload_offset: u64::try_from(offset).ok()?,
                reference: entity_reference(
                    &object.parent,
                    value,
                    entity_references.entities,
                    entity_references.classes,
                    entity_references.terminal_nulls,
                ),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(CatiaRelationProgramInstance {
        framing,
        program_entity: entity_reference(
            &object.parent,
            program_entity_id,
            entity_references.entities,
            entity_references.classes,
            entity_references.terminal_nulls,
        ),
        repeated_entity: entity_reference(
            &object.parent,
            repeated_reference_entity_id,
            entity_references.entities,
            entity_references.classes,
            entity_references.terminal_nulls,
        ),
        reference_incidences,
        relation_expression: selected_expression.map(|expression| expression.entity.clone()),
        parameter_dependencies,
        inputs,
    })
}

fn entity_reference(
    graph_id: &str,
    entity_id: u32,
    entities: &HashMap<(String, u32), String>,
    entity_classes: &CatiaEntityClassByGraphIdentityIndex,
    terminal_nulls: &CatiaTerminalNullByGraphIndex,
) -> CatiaEntityReference {
    let key = (graph_id.to_owned(), entity_id);
    CatiaEntityReference {
        entity_id,
        is_null: terminal_nulls.get(graph_id).copied() == Some(entity_id),
        entity: entities.get(&key).cloned(),
        class_name: entity_classes.get(&key).cloned(),
    }
}

fn reference_signature(
    production: entity_table::ReferenceSignature,
    graph_id: &str,
    entity_references: &CatiaEntityReferenceIndex<'_>,
) -> CatiaReferenceSignature {
    let first_entity = entity_reference(
        graph_id,
        production.first_reference,
        entity_references.entities,
        entity_references.classes,
        entity_references.terminal_nulls,
    );
    let second_entity = entity_reference(
        graph_id,
        production.second_reference,
        entity_references.entities,
        entity_references.classes,
        entity_references.terminal_nulls,
    );
    CatiaReferenceSignature {
        production,
        first_entity,
        second_entity,
    }
}

fn object_graph_derived_id(graph: &str, kind: &str, local_key: &str) -> Option<String> {
    let (namespace, graph_key) = graph.split_once('#')?;
    let mut components = namespace.split(':');
    let format = components.next()?;
    let scope = components.next()?;
    components.next()?;
    components
        .next()
        .is_none()
        .then(|| format!("{format}:{scope}:{kind}#{graph_key}:{local_key}"))
}

fn derive_reference_signature_cohorts(
    entity_records: &[CatiaEntityRecord],
) -> Vec<CatiaReferenceSignatureCohort> {
    let mut cohorts = Vec::<CatiaReferenceSignatureCohort>::new();
    let mut cohort_by_pair = HashMap::<(String, u32, u32), usize>::new();
    let mut next_ordinal_by_graph = HashMap::<String, u64>::new();
    for entity in entity_records {
        let Some(signature) = &entity.reference_signature else {
            continue;
        };
        let key = (
            entity.object_graph.clone(),
            signature.production.first_reference,
            signature.production.second_reference,
        );
        if let Some(index) = cohort_by_pair.get(&key).copied() {
            cohorts[index].members.push(entity.id.clone());
            continue;
        }
        let ordinal = next_ordinal_by_graph
            .entry(entity.object_graph.clone())
            .and_modify(|ordinal| *ordinal += 1)
            .or_insert(0);
        let Some(id) = object_graph_derived_id(
            &entity.object_graph,
            "reference-signature-cohort",
            &format!("{:08}", *ordinal),
        ) else {
            continue;
        };
        let index = cohorts.len();
        cohorts.push(CatiaReferenceSignatureCohort {
            id,
            parent: entity.object_graph.clone(),
            ordinal: *ordinal,
            first_reference: signature.production.first_reference,
            first_entity: signature.first_entity.clone(),
            second_reference: signature.production.second_reference,
            second_entity: signature.second_entity.clone(),
            schema_selection: None,
            members: vec![entity.id.clone()],
        });
        cohort_by_pair.insert(key, index);
    }
    let entities_by_id = entity_records
        .iter()
        .map(|entity| (entity.id.as_str(), entity))
        .collect::<HashMap<_, _>>();
    for cohort in &mut cohorts {
        let mut selected = None::<CatiaReferenceSignatureSchemaSelection>;
        let mut valid = true;
        for member in &cohort.members {
            let Some(entity) = entities_by_id.get(member.as_str()) else {
                valid = false;
                break;
            };
            let selections = &entity.value_schema_selections;
            if selections.first().map(|selection| selection.name.as_str()) != Some("_SpecList")
                || selections.len() > 2
            {
                valid = false;
                break;
            }
            let Some(selection) = selections.get(1) else {
                continue;
            };
            let candidate = CatiaReferenceSignatureSchemaSelection {
                ordinal: selection.ordinal,
                entry: selection.entry.clone(),
                name: selection.name.clone(),
            };
            if selected
                .as_ref()
                .is_some_and(|selected| selected != &candidate)
            {
                valid = false;
                break;
            }
            selected = Some(candidate);
        }
        cohort.schema_selection = valid.then_some(selected).flatten();
    }
    cohorts
}

fn schema_configuration_record(
    entity_id: u32,
    object: &CatiaObjectRecord,
    value_schema_selections: &[CatiaEntityValueSchemaSelection],
    entities: &HashMap<(String, u32), String>,
    entity_classes: &CatiaEntityClassByGraphIdentityIndex,
    terminal_nulls: &CatiaTerminalNullByGraphIndex,
) -> Option<CatiaSchemaConfigurationRecord> {
    if object.entity_id != Some(entity_id)
        || object.lead != 0x12
        || object.owner_entity_id().is_none()
        || object.class_ref != Some(entity_id)
        || object.class_name.as_deref() != Some("Configuration")
        || object.storage_ref.is_some()
    {
        return None;
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
        return None;
    };
    let mut matching_selections = value_schema_selections
        .iter()
        .filter(|selection| selection.ordinal == *schema_ordinal);
    let selection = matching_selections.next()?;
    if matching_selections.next().is_some() {
        return None;
    }
    Some(CatiaSchemaConfigurationRecord {
        schema_payload_offset: u64::try_from(*schema_offset).ok()?,
        schema_ordinal: *schema_ordinal,
        schema_entry: selection.entry.clone(),
        schema_name: selection.name.clone(),
        entity_reference: CatiaPayloadEntityReference {
            payload_offset: u64::try_from(*entity_offset).ok()?,
            reference: entity_reference(
                &object.parent,
                *referenced_entity_id,
                entities,
                entity_classes,
                terminal_nulls,
            ),
        },
    })
}

fn schema_configuration_row_link(
    entity_id: u32,
    object: &CatiaObjectRecord,
    entities: &HashMap<(String, u32), String>,
    entity_classes: &CatiaEntityClassByGraphIdentityIndex,
    terminal_nulls: &CatiaTerminalNullByGraphIndex,
) -> Option<CatiaSchemaConfigurationRowLink> {
    if object.entity_id != Some(entity_id)
        || object.lead != 0x12
        || object.owner_entity_id().is_none()
        || object.class_name.as_deref() != Some("configrow")
        || object.storage_ref.is_some()
    {
        return None;
    }
    let class_entity_id = object.class_ref?;
    let [PayloadField::Atom { value: 250, .. }, PayloadField::Atom {
        value: successor_entity_id,
        offset: successor_offset,
    }, PayloadField::Terminator] = object.payload.fields.as_slice()
    else {
        return None;
    };
    Some(CatiaSchemaConfigurationRowLink {
        class_reference: entity_reference(
            &object.parent,
            class_entity_id,
            entities,
            entity_classes,
            terminal_nulls,
        ),
        successor_payload_offset: u64::try_from(*successor_offset).ok()?,
        successor: entity_reference(
            &object.parent,
            *successor_entity_id,
            entities,
            entity_classes,
            terminal_nulls,
        ),
    })
}

fn derive_schema_configuration_row_chains(
    records: &[CatiaEntityRecord],
    entities: &HashMap<(String, u32), String>,
    entity_classes: &CatiaEntityClassByGraphIdentityIndex,
    terminal_nulls: &CatiaTerminalNullByGraphIndex,
) -> Vec<CatiaSchemaConfigurationRowChain> {
    let row_ids = records
        .iter()
        .filter(|entity| entity.schema_configuration_row_link.is_some())
        .map(|entity| (entity.object_graph.as_str(), entity.entity_id))
        .collect::<HashSet<_>>();
    let mut groups = HashMap::<(&str, u32), Vec<(u32, &CatiaSchemaConfigurationRowLink)>>::new();
    for entity in records {
        let Some(link) = &entity.schema_configuration_row_link else {
            continue;
        };
        groups
            .entry((entity.object_graph.as_str(), link.class_reference.entity_id))
            .or_default()
            .push((entity.entity_id, link));
    }
    let mut groups = groups.into_iter().collect::<Vec<_>>();
    groups.sort_by(
        |((left_graph, left_root), _), ((right_graph, right_root), _)| {
            left_graph.cmp(right_graph).then(left_root.cmp(right_root))
        },
    );

    groups
        .into_iter()
        .filter_map(|((graph, root), links)| {
            let successors = links.iter().copied().collect::<HashMap<_, _>>();
            if successors.len() != links.len() {
                return None;
            }
            let mut row_ids_in_order = Vec::with_capacity(links.len());
            let mut visited = HashSet::new();
            let mut current = root;
            while let Some(link) = successors.get(&current).copied() {
                if !visited.insert(current) {
                    return None;
                }
                row_ids_in_order.push(current);
                current = link.successor.entity_id;
            }
            if visited.len() != links.len() || row_ids.contains(&(graph, current)) {
                return None;
            }
            let links = row_ids_in_order
                .into_iter()
                .map(|row_id| {
                    let link = successors[&row_id];
                    let successor_id = link.successor.entity_id;
                    CatiaSchemaConfigurationRowChainLink {
                        row: entity_reference(
                            graph,
                            row_id,
                            entities,
                            entity_classes,
                            terminal_nulls,
                        ),
                        successor_payload_offset: link.successor_payload_offset,
                        successor: link.successor.clone(),
                        intervening_entities: (row_id < successor_id).then(|| {
                            records
                                .iter()
                                .filter(|entity| {
                                    entity.object_graph == graph
                                        && entity.entity_id > row_id
                                        && entity.entity_id < successor_id
                                })
                                .map(|entity| {
                                    entity_reference(
                                        graph,
                                        entity.entity_id,
                                        entities,
                                        entity_classes,
                                        terminal_nulls,
                                    )
                                })
                                .collect()
                        }),
                    }
                })
                .collect();
            Some(CatiaSchemaConfigurationRowChain {
                id: object_graph_derived_id(
                    graph,
                    "schema-configuration-row-chain",
                    &root.to_string(),
                )?,
                object_graph: graph.to_string(),
                links,
            })
        })
        .collect()
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
    definitions: &[CatiaDefinitionSchemaSelection],
    entity_id: u32,
    object: &CatiaObjectRecord,
    relation_expressions: &HashMap<String, String>,
    entity_references: &CatiaEntityReferenceIndex<'_>,
    parameter_bindings: &CatiaParameterBindingIndex,
) -> Option<CatiaFormulaRelation> {
    let [definition0, definition1] = definitions else {
        return None;
    };
    if definition0.name.as_deref() != Some("Formula")
        || definition1.name.as_deref() != Some("Formula")
        || definition0.entry != definition1.entry
    {
        return None;
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
        return None;
    };
    if *owner != entity_id {
        return None;
    }
    let [owner_reference, expression_reference, parameter_reference] = object.references.as_slice()
    else {
        return None;
    };
    if owner_reference.entity_id != entity_id
        || expression_reference.entity_id != *expression_entity_id
        || parameter_reference.entity_id != *parameter_entity_id
        || owner_reference.target.as_deref() != Some(object.id.as_str())
    {
        return None;
    }
    let expression_object = expression_reference.target.as_ref()?;
    let source = relation_expressions.get(expression_object)?;
    let parameter_dependencies =
        relation_parameter_dependencies(source, &object.parent, parameter_bindings);
    Some(CatiaFormulaRelation {
        expression_entity: CatiaPayloadEntityReference {
            payload_offset: u64::try_from(*expression_offset).ok()?,
            reference: entity_reference(
                &object.parent,
                *expression_entity_id,
                entity_references.entities,
                entity_references.classes,
                entity_references.terminal_nulls,
            ),
        },
        output_entity: CatiaPayloadEntityReference {
            payload_offset: u64::try_from(*parameter_offset).ok()?,
            reference: CatiaEntityReference {
                is_null: parameter_reference.is_null,
                ..entity_reference(
                    &object.parent,
                    *parameter_entity_id,
                    entity_references.entities,
                    entity_references.classes,
                    entity_references.terminal_nulls,
                )
            },
        },
        parameter_dependencies,
    })
}

type CatiaRelationExpressionIndex = HashMap<String, String>;
struct CatiaRelationExpressionEntity {
    entity: String,
    source: String,
    signature: Option<CatiaRelationTypeSignature>,
}
type CatiaRelationExpressionEntityIndex = HashMap<(String, u32), CatiaRelationExpressionEntity>;
type CatiaEntityByGraphIdentityIndex = HashMap<(String, u32), String>;
type CatiaEntityClassByGraphIdentityIndex = HashMap<(String, u32), String>;
type CatiaTerminalNullByGraphIndex = HashMap<String, u32>;
type CatiaParameterBindingIndex = HashMap<String, HashMap<String, Vec<CatiaEntityReference>>>;

struct CatiaEntityReferenceIndex<'a> {
    entities: &'a CatiaEntityByGraphIdentityIndex,
    classes: &'a CatiaEntityClassByGraphIdentityIndex,
    terminal_nulls: &'a CatiaTerminalNullByGraphIndex,
}

fn entity_class_index<'a>(
    records: impl IntoIterator<Item = &'a CatiaObjectRecord>,
) -> CatiaEntityClassByGraphIdentityIndex {
    records
        .into_iter()
        .filter_map(|record| {
            Some((
                (record.parent.clone(), record.entity_id?),
                record.class_name.clone()?,
            ))
        })
        .collect()
}

fn semantic_entity_indices(
    entities: &[CatiaEntityRecord],
    entity_classes: &CatiaEntityClassByGraphIdentityIndex,
) -> (
    CatiaRelationExpressionIndex,
    CatiaRelationExpressionEntityIndex,
    CatiaEntityByGraphIdentityIndex,
    CatiaTerminalNullByGraphIndex,
    CatiaParameterBindingIndex,
) {
    let relation_expressions = entities
        .iter()
        .filter_map(|entity| {
            let expression = entity.relation_expression.as_ref()?;
            Some((
                entity.object_record.clone(),
                expression.expression.value.clone(),
            ))
        })
        .collect();
    let relation_expression_entities = entities
        .iter()
        .filter_map(|entity| {
            let expression = entity.relation_expression.as_ref()?;
            Some((
                (entity.object_graph.clone(), entity.entity_id),
                CatiaRelationExpressionEntity {
                    entity: entity.id.clone(),
                    source: expression.expression.value.clone(),
                    signature: expression.signature.clone(),
                },
            ))
        })
        .collect();
    let entities_by_graph_identity = entities
        .iter()
        .map(|entity| {
            (
                (entity.object_graph.clone(), entity.entity_id),
                entity.id.clone(),
            )
        })
        .collect();
    let terminal_nulls = entities.iter().fold(
        CatiaTerminalNullByGraphIndex::new(),
        |mut terminal_nulls, entity| {
            terminal_nulls
                .entry(entity.object_graph.clone())
                .and_modify(|maximum| *maximum = (*maximum).max(entity.entity_id))
                .or_insert(entity.entity_id);
            terminal_nulls
        },
    );
    let terminal_nulls = terminal_nulls
        .into_iter()
        .filter_map(|(graph, maximum)| maximum.checked_add(1).map(|identity| (graph, identity)))
        .collect();
    let mut parameter_bindings = CatiaParameterBindingIndex::new();
    for entity in entities {
        let Some(parameter) = &entity.parameter_value else {
            continue;
        };
        parameter_bindings
            .entry(entity.object_graph.clone())
            .or_default()
            .entry(parameter.binding.value.clone())
            .or_default()
            .push(CatiaEntityReference {
                entity_id: entity.entity_id,
                is_null: false,
                entity: Some(entity.id.clone()),
                class_name: entity_classes
                    .get(&(entity.object_graph.clone(), entity.entity_id))
                    .cloned(),
            });
    }
    (
        relation_expressions,
        relation_expression_entities,
        entities_by_graph_identity,
        terminal_nulls,
        parameter_bindings,
    )
}

fn relation_parameter_dependencies(
    source: &str,
    graph: &str,
    parameter_bindings: &CatiaParameterBindingIndex,
) -> Vec<CatiaRelationParameterDependency> {
    relation_symbols(source)
        .into_iter()
        .map(|(source_offset, symbol)| {
            let candidates = parameter_bindings
                .get(graph)
                .and_then(|bindings| bindings.get(&symbol))
                .cloned()
                .unwrap_or_default();
            CatiaRelationParameterDependency {
                source_offset,
                symbol,
                candidates,
            }
        })
        .collect()
}

pub(crate) fn dependency_matches_input(
    dependency: &CatiaRelationParameterDependency,
    input: &CatiaRelationTypeInput,
) -> bool {
    dependency
        .symbol
        .strip_prefix(&input.parameter)
        .is_some_and(|suffix| {
            let suffix =
                suffix.trim_start_matches(|character: char| character.is_ascii_whitespace());
            suffix.is_empty()
                || suffix.strip_prefix('/').is_some_and(|ordinal| {
                    !ordinal.is_empty() && ordinal.bytes().all(|byte| byte.is_ascii_digit())
                })
        })
}

pub(crate) fn resolved_relation_program_inputs(
    signature: &CatiaRelationTypeSignature,
    dependencies: &[CatiaRelationParameterDependency],
) -> Option<Vec<CatiaRelationProgramInput>> {
    if dependencies.iter().any(|dependency| {
        signature
            .inputs
            .iter()
            .filter(|input| dependency_matches_input(dependency, input))
            .count()
            != 1
    }) {
        return None;
    }
    let mut entity_ids = HashSet::new();
    signature
        .inputs
        .iter()
        .map(|input| {
            let mut selected = None;
            let mut occurrence_count = 0;
            for dependency in dependencies
                .iter()
                .filter(|dependency| dependency_matches_input(dependency, input))
            {
                occurrence_count += 1;
                let [candidate] = dependency.candidates.as_slice() else {
                    return None;
                };
                if candidate.is_null || candidate.entity.is_none() {
                    return None;
                }
                match &selected {
                    Some(selected) if selected != candidate => return None,
                    Some(_) => {}
                    None => selected = Some(candidate.clone()),
                }
            }
            let entity = (occurrence_count != 0).then_some(selected)??;
            if !entity_ids.insert(entity.entity_id) {
                return None;
            }
            Some(CatiaRelationProgramInput {
                parameter: input.parameter.clone(),
                value_type: input.input_type.clone(),
                entity,
            })
        })
        .collect()
}

pub(crate) fn relation_symbols(source: &str) -> Vec<(u64, String)> {
    let bytes = source.as_bytes();
    let mut symbols = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'"' {
            at += 1;
            while bytes.get(at).is_some_and(|byte| *byte != b'"') {
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
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if at == digits_start || bytes.get(at) != Some(&b'_') {
            at = start + 1;
            continue;
        }
        at += 1;
        let bare_end = at;
        while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
            at += 1;
        }
        if bytes.get(at) != Some(&b'/') {
            symbols.push((start as u64, source[start..bare_end].to_string()));
            at = bare_end;
            continue;
        }
        at += 1;
        let ordinal_start = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if at == ordinal_start {
            at = start + 1;
            continue;
        }
        symbols.push((start as u64, source[start..at].to_string()));
    }
    symbols
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
    suffix: Option<&object_graph::RepeatedReferenceSuffix>,
    catalog: Option<&CatiaCatalog>,
) -> Option<CatiaRepeatedReferenceSchemaSelection> {
    let (order, ordinal, offset) = match suffix?.schema_preamble.as_ref()? {
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
    Some(CatiaRepeatedReferenceSchemaSelection {
        order,
        offset: offset as u64,
        ordinal,
        entry: catalog_entry.map(|entry| entry.id.clone()),
        name: catalog_entry.map(|entry| entry.value.clone()),
    })
}

/// One stored entity identity in a pre-`7C05` design stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaLegacyEntityIdentity {
    /// Offset of the `EA` identity delimiter.
    pub byte_offset: u64,
    /// Little-endian identity following the delimiter.
    pub entity_id: u32,
    /// Stored record lead following the identity.
    #[serde(default)]
    pub lead: u8,
}

/// One complete compact legacy schema program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaLegacySchemaProgram {
    /// Offset of the first program byte after the fixed prefix.
    pub byte_offset: u64,
    /// Offset of the production following the program.
    #[serde(alias = "footer_byte_offset")]
    pub boundary_byte_offset: u64,
    /// Production that closes the program.
    #[serde(default)]
    pub boundary: CatiaLegacySchemaProgramBoundary,
    /// Exact program bytes, including the terminal `FE`.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub data: Vec<u8>,
    /// Complete inclusive-length identifier packets in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub identifiers: Vec<CatiaLegacySchemaIdentifier>,
}

/// Production that closes a compact legacy schema program.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatiaLegacySchemaProgramBoundary {
    /// Fixed vendor footer preceded by the terminal `FE`.
    #[default]
    VendorFooter,
    /// Validated outer stream directory preceded by the terminal `FE`.
    StreamDirectory,
}

/// One complete inclusive-length identifier packet in a compact schema program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaLegacySchemaIdentifier {
    /// Offset of the inclusive-length byte.
    pub byte_offset: u64,
    /// Stored identifier.
    pub value: String,
}

/// Framing production used by a legacy schema text field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatiaLegacyTextEncoding {
    /// Nonzero one-byte inclusive length.
    U8InclusiveLength,
    /// Zero selector and little-endian `u32` byte length.
    ZeroU32Length,
    /// Nonzero inclusive length followed by an `E3` paged-role tail.
    U8InclusiveLengthE3RoleTail,
}

/// Framing production used by a legacy role selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatiaLegacyRoleSelectorEncoding {
    /// `80` followed by a nonzero little-endian `u32`.
    FixedU32,
    /// Page byte `D1..E4` followed by one low byte.
    Paged,
}

/// Stored representation of one legacy schema role name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(untagged)]
pub enum CatiaLegacyRoleName {
    /// Inclusive-length UTF-8 role name.
    Literal(String),
    /// Unresolved one-byte schema selector.
    Selector(u8),
}

impl CatiaLegacyRoleName {
    #[cfg(test)]
    pub(crate) fn literal(&self) -> Option<&str> {
        match self {
            Self::Literal(value) => Some(value),
            Self::Selector(_) => None,
        }
    }

    fn byte_len(&self) -> usize {
        match self {
            Self::Literal(value) => 1 + value.len(),
            Self::Selector(_) => 1,
        }
    }
}

impl From<legacy_entity::LegacyRoleName> for CatiaLegacyRoleName {
    fn from(value: legacy_entity::LegacyRoleName) -> Self {
        match value {
            legacy_entity::LegacyRoleName::Literal(value) => Self::Literal(value),
            legacy_entity::LegacyRoleName::Selector(value) => Self::Selector(value),
        }
    }
}

/// One length-framed legacy schema role and its selector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaLegacyRoleSelector {
    /// Offset of the literal length or schema-selector byte.
    pub byte_offset: u64,
    /// Stored identity whose interval contains the role.
    #[serde(default)]
    pub entity_id: u32,
    /// Stored literal or unresolved role name.
    pub name: CatiaLegacyRoleName,
    /// Selector framing production.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding: Option<CatiaLegacyRoleSelectorEncoding>,
    /// Stored selector following the role name.
    pub selector: u32,
    /// Field code when an `E8 <field-code:u16le> 01` opener follows immediately.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_code: Option<u16>,
}

impl CatiaLegacyRoleSelector {
    pub(crate) fn end_offset(&self) -> Option<u64> {
        let selector_len = match self.encoding? {
            CatiaLegacyRoleSelectorEncoding::FixedU32 => 5,
            CatiaLegacyRoleSelectorEncoding::Paged => 2,
        };
        self.byte_offset
            .checked_add(u64::try_from(self.name.byte_len()).ok()?)?
            .checked_add(selector_len)
    }
}

/// One complete legacy schema text field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaLegacyTextField {
    /// Offset of the field opener.
    pub byte_offset: u64,
    /// Stored identity whose interval contains the field.
    pub entity_id: u32,
    /// Text framing production.
    pub encoding: CatiaLegacyTextEncoding,
    /// Immediately preceding length-framed role and selector.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<CatiaLegacyRoleSelector>,
    /// Decoded UTF-8 value.
    pub value: String,
}

/// One legacy schema field bounded by consecutive role selectors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaLegacySchemaField {
    /// Offset of the `E8 <field-code:u16le> 01` opener.
    pub byte_offset: u64,
    /// Stored identity whose interval contains the field.
    pub entity_id: u32,
    /// Role selector that binds this field.
    pub role_byte_offset: u64,
    /// Following role selector that closes the payload.
    pub boundary_role_byte_offset: u64,
    /// Stored schema field code.
    pub field_code: u16,
    /// Exact bytes after the opener and before the boundary role.
    pub payload: Vec<u8>,
}

/// One typed parameter role in a legacy relation signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaLegacyRelationParameter {
    /// Expression-local parameter.
    pub parameter: String,
    /// Source value type.
    pub value_type: String,
}

/// One complete legacy expression and type-signature pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaLegacyRelation {
    /// Stored owner identity.
    pub entity_id: u32,
    /// Selector carried by the expression field's `body` role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_selector: Option<u32>,
    /// Selector carried by the type-signature field's `param` role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_selector: Option<u32>,
    /// Parameter identity selected by exact self-`body` and target-`param` roles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_entity_id: Option<u32>,
    /// Expression-field opener offset.
    pub expression_offset: u64,
    /// Exact expression or rule program.
    pub expression: String,
    /// Signature-field opener offset.
    pub signature_offset: u64,
    /// Exact stored type signature.
    pub type_signature: String,
    /// Ordered input parameters.
    pub inputs: Vec<CatiaLegacyRelationParameter>,
    /// Output parameter for a `VoidType` relation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<CatiaLegacyRelationParameter>,
    /// Source result type.
    pub result_type: String,
}

/// One complete legacy `synchrone` relation-update field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaLegacyRelationSynchronousState {
    /// Offset of the `synchrone` role-name length byte.
    pub role_byte_offset: u64,
    /// Stored containing identity.
    pub entity_id: u32,
    /// Selector carried by the `synchrone` role.
    pub selector: u32,
    /// Whether the relation updates synchronously.
    pub synchronous: bool,
}

/// Value selected by one complete legacy type descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum CatiaLegacyTypeValue {
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
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaLegacyTypeDescriptor {
    /// Offset of the fixed descriptor prefix.
    pub byte_offset: u64,
    /// Stored containing identity.
    pub entity_id: u32,
    /// Stored literal name or unresolved selector.
    pub value: CatiaLegacyTypeValue,
}

/// Evaluation stored by a complete legacy scalar packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum CatiaLegacyScalarEvaluation {
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
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatiaLegacyScalarEncoding {
    /// `FE 84 88 82 FE`.
    Named84,
    /// `FE 85 88 82 FE`.
    Standalone85,
}

/// One complete typed scalar packet in a legacy identity interval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaLegacyScalarValue {
    /// Stable native identity derived from the containing run and packet offset.
    pub id: String,
    /// Offset of the packet prefix.
    pub byte_offset: u64,
    /// Stored containing identity.
    pub entity_id: u32,
    /// Fixed scalar-prefix production.
    pub encoding: CatiaLegacyScalarEncoding,
    /// Unique co-owned `name` text field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_field: Option<u64>,
    /// Unique co-owned stored name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Stored evaluation.
    pub evaluation: CatiaLegacyScalarEvaluation,
}

/// One complete legacy UTF-8 string-value packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaLegacyStringValue {
    /// Stable native identity derived from the containing run and packet offset.
    pub id: String,
    /// Offset of the packet prefix.
    pub byte_offset: u64,
    /// Stored containing identity.
    pub entity_id: u32,
    /// Unique co-owned `name` text field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_field: Option<u64>,
    /// Unique co-owned stored name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Stored UTF-8 value.
    pub value: String,
}

/// Stored encoding of one complete legacy signed integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CatiaLegacyIntegerEncoding {
    /// One byte stores values zero through 126 as `value + 0x81`.
    Inline,
    /// `80` introduces one signed little-endian 32-bit value.
    WideI32,
}

/// One complete legacy signed-integer packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaLegacyIntegerValue {
    /// Stable native identity derived from the containing run and packet offset.
    pub id: String,
    /// Offset of the packet prefix.
    pub byte_offset: u64,
    /// Stored containing identity.
    pub entity_id: u32,
    /// Stored integer encoding.
    pub encoding: CatiaLegacyIntegerEncoding,
    /// Unique co-owned `name` text field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_field: Option<u64>,
    /// Unique co-owned stored name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Stored signed value.
    pub value: i32,
}

/// A monotonically identified pre-`7C05` run and its terminating catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaLegacyEntityRun {
    /// Stable native identity.
    pub id: String,
    /// Offset of the first identity delimiter.
    pub byte_offset: u64,
    /// Bytes from the first identity delimiter to the catalog opener.
    pub byte_len: u64,
    /// Offset of the fixed schema-catalog opening production.
    pub catalog_offset: u64,
    /// Complete compact schema program following the catalog opener.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_program: Option<CatiaLegacySchemaProgram>,
    /// Exact declared outer container whose physical stream contains this run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_container: Option<CatiaOuterContainerBinding>,
    /// Stored identities in source order.
    pub identities: Vec<CatiaLegacyEntityIdentity>,
    /// Complete length-framed role selectors in identity-interval order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub role_selectors: Vec<CatiaLegacyRoleSelector>,
    /// Complete schema text fields in identity-interval order.
    pub text_fields: Vec<CatiaLegacyTextField>,
    /// Complete role-bounded schema fields in identity-interval order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schema_fields: Vec<CatiaLegacySchemaField>,
    /// Complete expression/signature pairs.
    pub relations: Vec<CatiaLegacyRelation>,
    /// Complete `synchrone` relation-update fields.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub synchronous_states: Vec<CatiaLegacyRelationSynchronousState>,
    /// Complete literal or selector type descriptors.
    pub type_descriptors: Vec<CatiaLegacyTypeDescriptor>,
    /// Complete typed scalar packets.
    pub scalar_values: Vec<CatiaLegacyScalarValue>,
    /// Complete UTF-8 string-value packets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub string_values: Vec<CatiaLegacyStringValue>,
    /// Complete signed-integer packets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub integer_values: Vec<CatiaLegacyIntegerValue>,
}

/// One zero-entity face-local surface-support occurrence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaZeroEntitySupportOccurrence {
    /// Byte offset of the framed `21xx` record.
    pub byte_offset: u64,
    /// One-based global record ordinal in the zero-entity stream.
    pub record_ordinal: u32,
    /// Complete two-byte record tag.
    pub tag: [u8; 2],
    /// Face-local support slot stored at record offset 12.
    pub face_local_slot: u32,
    /// Stored UV endpoints when the record family carries them inline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uv_endpoints: Option<[[f64; 2]; 2]>,
    /// Complete parameter-space curve carried by the support record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pcurve: Option<cadmpeg_ir::geometry::PcurveGeometry>,
    /// Exact model-space carrier derived from the pcurve and owning surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_curve: Option<cadmpeg_ir::geometry::CurveGeometry>,
    /// Exact procedural model-space carrier derived from the pcurve and owning surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_curve_construction: Option<cadmpeg_ir::geometry::ProceduralCurveDefinition>,
    /// Model-carrier parameters at the two stored UV endpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_parameters: Option<[f64; 2]>,
    /// Surface point at the midpoint of the bounded pcurve parameter interval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_midpoint: Option<cadmpeg_ir::math::Point3>,
    /// UV endpoints lifted through the owning surface carrier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_endpoints: Option<[cadmpeg_ir::math::Point3; 2]>,
}

/// One counted zero-entity `5fxx` face record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaZeroEntityFace {
    /// Byte offset of the framed face record.
    pub byte_offset: u64,
    /// One-based global record ordinal.
    pub record_ordinal: u32,
    /// Complete two-byte record tag.
    pub tag: [u8; 2],
    /// Counted allocation values in storage order.
    pub allocations: Vec<u32>,
    /// Ordered loop terminals derived from the allocation lane.
    pub loop_terminals: Vec<u32>,
    /// Positionally aligned loop records.
    pub loops: Vec<CatiaZeroEntityLoop>,
    /// Terminal control byte following the allocation lane.
    pub terminal_control: u8,
}

/// One counted zero-entity `62xx` loop record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaZeroEntityLoop {
    /// Byte offset of the framed loop record.
    pub byte_offset: u64,
    /// One-based global record ordinal.
    pub record_ordinal: u32,
    /// Complete two-byte record tag.
    pub tag: [u8; 2],
    /// Nonterminal even-lane logical member identifiers.
    pub member_ids: Vec<u32>,
    /// Odd-lane typed references in member order.
    pub typed_references: Vec<u32>,
    /// Global zero-entity records selected by the typed references.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub typed_records: Vec<String>,
    /// Face-local support record ordinals selected by the logical members.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub support_record_ordinals: Vec<u32>,
    /// Terminal even-lane logical identifier.
    pub terminal_id: u32,
    /// Difference between the terminal and first member identifiers.
    pub gap: u32,
    /// Stored loop-class byte.
    pub loop_class: u8,
    /// Absolute coedge senses in member order; `true` is forward.
    pub forward_senses: Vec<bool>,
    /// Complete sense-oriented model-space endpoint pairs in member order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub oriented_model_endpoints: Vec<[cadmpeg_ir::math::Point3; 2]>,
}

/// One zero-entity surface carrier and its maximal following support run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaZeroEntitySupportRun {
    /// Stable native-run identity.
    pub id: String,
    /// Byte offset of the owning surface-carrier record.
    pub carrier_byte_offset: u64,
    /// One-based global record ordinal of the owning surface carrier.
    pub carrier_record_ordinal: u32,
    /// Positionally aligned face record when the complete rosters agree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<CatiaZeroEntityFace>,
    /// Face-local support occurrences in storage order.
    pub supports: Vec<CatiaZeroEntitySupportOccurrence>,
}

/// One zero-entity `5e1a` allocation tuple.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaZeroEntityEdgeStride {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// One-based global record ordinal in the zero-entity stream.
    pub record_ordinal: u32,
    /// Five allocation values following the fixed tagged-one prefix.
    pub allocations: [u32; 5],
    /// The three allocations in the `0638`/`2569` topology namespace, in
    /// source order `[T, T-1, T-2]`.
    pub topology_refs: [u32; 3],
    /// The two allocations selecting the adjacent surface-support slots, in
    /// source order `[X, Y]`.
    pub surface_support_refs: [u32; 2],
}

/// One positional zero-entity `0638` oriented use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaZeroEntityOrientedUse {
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// One-based global record ordinal in the zero-entity stream.
    pub record_ordinal: u32,
    /// Positional side number.
    pub side: u32,
    /// Two stored allocation values.
    pub allocations: [u32; 2],
}

/// One zero-entity `2569` header and its two positional uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaZeroEntityOrientedUsePair {
    /// Stable native-pair identity.
    pub id: String,
    /// Byte offset of the `2569` header.
    pub header_byte_offset: u64,
    /// One-based global record ordinal of the `2569` header.
    pub header_record_ordinal: u32,
    /// Stored base columns.
    pub base_columns: [u32; 2],
    /// Side-one then side-two oriented uses.
    pub uses: [CatiaZeroEntityOrientedUse; 2],
}

/// Two zero-entity radial support occurrences with matching bounded model-space witnesses.
///
/// This relation does not establish curve coincidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaZeroEntityEndpointPairCandidate {
    /// Stable derived-pair identity.
    pub id: String,
    /// Two face-record identities in support-record order.
    pub face_records: [String; 2],
    /// Two radial support-record identities in ascending ordinal order.
    pub support_records: [String; 2],
    /// Model-space endpoints oriented by the first support occurrence.
    pub model_endpoints: [cadmpeg_ir::math::Point3; 2],
    /// Model-space midpoint witness supplied by the first support occurrence.
    pub model_midpoint: cadmpeg_ir::math::Point3,
}

/// One endpoint-pair endpoint incident to a geometric endpoint-locus candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaZeroEntityEndpointPairEndpoint {
    /// Derived endpoint-pair candidate.
    pub endpoint_pair: String,
    /// Zero-based endpoint index in that candidate's oriented endpoint pair.
    pub endpoint_index: u8,
}

/// One geometric endpoint-locus candidate established by a complete endpoint clique.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaZeroEntityEndpointLocusCandidate {
    /// Stable derived-locus identity.
    pub id: String,
    /// Incident endpoints in endpoint-pair and endpoint order.
    pub incident_endpoint_pair_endpoints: Vec<CatiaZeroEntityEndpointPairEndpoint>,
    /// Model-space point from the first incident endpoint.
    pub representative_point: cadmpeg_ir::math::Point3,
    /// Maximum pairwise distance between incident endpoint coordinates.
    pub maximum_deviation: f64,
}

/// One counted zero-entity `05xx` vertex-incidence record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaZeroEntityVertexIncidence {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// One-based global record ordinal in the zero-entity stream.
    pub record_ordinal: u32,
    /// Complete two-byte record tag.
    pub tag: [u8; 2],
    /// Stored allocation values.
    pub allocations: Vec<u32>,
    /// Immediately following `5d06` vertex-owner record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertex_record: Option<String>,
}

/// One complete zero-entity face-roster, shell, and body ownership hierarchy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaZeroEntityOwnershipRoot {
    /// Stable native-root identity.
    pub id: String,
    /// Byte offset of the counted `6142` face-roster record.
    pub face_roster_byte_offset: u64,
    /// One-based global record ordinal of the face-roster record.
    pub face_roster_record_ordinal: u32,
    /// Descending one-based face-allocation slots.
    pub face_slots: Vec<u32>,
    /// Byte offset of the `6006` shell root.
    pub shell_byte_offset: u64,
    /// One-based global record ordinal of the shell root.
    pub shell_record_ordinal: u32,
    /// Byte offset of the `6508` body root.
    pub body_byte_offset: u64,
    /// One-based global record ordinal of the body root.
    pub body_record_ordinal: u32,
}

/// One framed record in the zero-entity global identity namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CatiaZeroEntityRecord {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Exclusive logical byte end, including any inline continuation.
    pub logical_end: u64,
    /// Complete two-byte record tag.
    pub tag: [u8; 2],
    /// One-based global record ordinal.
    pub record_ordinal: u32,
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
        pub(crate) const CATIA_ARENA_NAMES: &[&str] = &[
            $(stringify!($field)),+
        ];

        /// CATIA-native records retained outside the format-neutral model.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        pub struct CatiaNative {
            /// Schema version this namespace was written under.
            pub version: u32,
            $(
                $(
                    $(#[$attr])*
                    #[serde(default)]
                    $vis $field: Vec<$record>,
                )?
            )*
        }

        /// Owning, flattened arena payload shared by borrowed and consuming stores.
        pub(crate) struct CatiaArenaProjection {
            $(
                $(
                    $field: define_catia_arenas!(@type $stored, $record),
                )?
            )*
            $(
                $(
                    $field: define_catia_arenas!(
                        @flattened_type $owner, $children, $record
                    ),
                )?
            )*
        }

        impl From<&CatiaNative> for CatiaArenaProjection {
            fn from(native: &CatiaNative) -> Self {
                Self::from((*native).clone())
            }
        }

        impl From<CatiaNative> for CatiaArenaProjection {
            fn from(mut native: CatiaNative) -> Self {
                $(
                    $(
                        let $field = native
                            .$owner
                            .iter_mut()
                            .flat_map(|parent| std::mem::take(&mut parent.$children))
                            .collect();
                    )?
                )*
                Self {
                    $(
                        $(
                            $field: define_catia_arenas!(
                                @stored_value $stored, native, $field
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
                }
            }
        }

        type CatiaFamilyRow =
            FamilyRow<CatiaArenaProjection, (), cadmpeg_ir::NativeNamespace, ()>;

        /// Declarative CATIA native-family catalogue.
        pub(crate) const CATIA_FAMILIES: &[CatiaFamilyRow] = &[
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
                    version: CATIA_NATIVE_VERSION,
                    $(
                        $(
                            $field: define_catia_arenas!(@default $stored),
                        )?
                    )*
                }
            }
        }
    };
    (@type $kind:ident, $record:ty) => {
        Vec<$record>
    };
    (@flattened_type $owner:ident, $children:ident, $record:ty) => {
        Vec<$record>
    };
    (@stored_value stored, $native:ident, $field:ident) => {
        $native.$field
    };
    (@flattened_value $owner:ident, $children:ident, $field:ident) => {
        $field
    };
    (@family $kind:ident, $field:ident) => {
        CatiaFamilyRow {
            arena: stringify!($field),
            tag: None,
            exactness: (),
            phase: Phase::ArenaOnly,
            emit: |projection, row, namespace| {
                namespace.set_arena(row.arena, &projection.$field)
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
        pub stored;
    },
    catalog_entries: CatiaCatalogEntry {
        => catalogs.entries;
    },
    catalogs: CatiaCatalog {
        /// Framed source-schema name catalogs.
        pub stored;
    },
    consolidated_circles: CatiaConsolidatedCircle {
        /// Exact consolidated arc-length circle supports.
        pub stored;
    },
    consolidated_class61_records: CatiaConsolidatedClass61Record {
        /// Complete consolidated class-`0x61` records.
        pub stored;
    },
    consolidated_class5b5c_records: CatiaConsolidatedClass5b5cRecord {
        /// Complete source-local consolidated class-`0x5b`/`0x5c` records.
        pub stored;
    },
    consolidated_cone_faces: CatiaConsolidatedConeFace {
        /// Complete consolidated cone-face chart descriptors.
        pub stored;
    },
    consolidated_cones: CatiaConsolidatedCone {
        /// Exact consolidated cone charts.
        pub stored;
    },
    consolidated_cylinders: CatiaConsolidatedCylinder {
        /// Exact consolidated cylinder charts.
        pub stored;
    },
    consolidated_embedded_cylinders: CatiaConsolidatedEmbeddedCylinder {
        /// Exact cylinder charts embedded in type-3 consolidated groups.
        pub stored;
    },
    consolidated_edge_nodes: CatiaConsolidatedEdgeNode {
        /// Structurally complete consolidated edge nodes.
        pub stored;
    },
    consolidated_edge_runs: CatiaConsolidatedEdgeRun {
        /// Complete consolidated historical edge runs.
        pub stored;
    },
    consolidated_groups: CatiaConsolidatedGroup {
        /// Typed consolidated class-`0x60` group openers.
        pub stored;
    },
    consolidated_line_profiles: CatiaConsolidatedLineProfile {
        /// Exact consolidated B-family metric line profiles.
        pub stored;
    },
    consolidated_owner_packets: CatiaConsolidatedOwnerPacket {
        /// Exact consolidated owner packets and their allocation links.
        pub stored;
    },
    consolidated_parameter_points: CatiaConsolidatedParameterPoint {
        /// Exact consolidated parameter-space records.
        pub stored;
    },
    consolidated_plane_carriers: CatiaConsolidatedPlaneCarrier {
        /// Structurally complete consolidated class-`0x27` plane carriers.
        pub stored;
    },
    consolidated_pcurves: CatiaConsolidatedPcurve {
        /// Consolidated pcurve jets retained before support resolution.
        pub stored;
    },
    consolidated_reference_lists: CatiaConsolidatedReferenceList {
        /// Exact consolidated persistent-reference lists.
        pub stored;
    },
    consolidated_revolutions: CatiaConsolidatedRevolution {
        /// Consolidated revolution carriers retained before profile resolution.
        pub stored;
    },
    consolidated_spheres: CatiaConsolidatedSphere {
        /// Exact consolidated sphere charts.
        pub stored;
    },
    consolidated_tori: CatiaConsolidatedTorus {
        /// Exact consolidated torus charts.
        pub stored;
    },
    consolidated_vertex_identities: CatiaConsolidatedVertexIdentity {
        /// Scoped endpoint identities and their consolidated edge incidence.
        pub stored;
    },
    design_objects: CatiaDesignObject {
        /// Design objects grouped by their serialized owner entity identity.
        pub stored;
    },
    entity_records: CatiaEntityRecord {
        /// Exact `7C05` entity-table records paired with object records.
        pub stored;
    },
    external_references: CatiaExternalReference {
        /// External CATIA document references in source order.
        pub stored;
    },
    finjpl_segments: CatiaFinjplSegment {
        /// Complete bounded outer FINJPL segments.
        pub stored;
    },
    legacy_entity_runs: CatiaLegacyEntityRun {
        /// Monotone entity identities in pre-`7C05` design streams.
        pub stored;
    },
    object_graph_records: CatiaObjectRecord {
        => object_graphs.records;
    },
    object_graphs: CatiaObjectGraph {
        /// Outer ownership graphs.
        pub stored;
    },
    preview_images: CatiaPreviewImage {
        /// Exact JPEG previews extracted from summary-information records.
        pub stored;
    },
    reference_signature_cohorts: CatiaReferenceSignatureCohort {
        /// Source-ordered descriptor cohorts grouped by exact reference pair.
        pub stored;
    },
    schema_configuration_row_chains: CatiaSchemaConfigurationRowChain {
        /// Complete schema-configuration-row successor chains.
        pub stored;
    },
    value_blocks: CatiaValueBlock {
        /// Framed value blocks adjacent to source-schema catalogs.
        pub stored;
    },
    value_schema_selections: CatiaValueSchemaSelection {
        => value_blocks.schema_selections;
    },
    zero_entity_edge_strides: CatiaZeroEntityEdgeStride {
        /// Zero-entity edge-stride allocation tuples.
        pub stored;
    },
    zero_entity_oriented_use_pairs: CatiaZeroEntityOrientedUsePair {
        /// Zero-entity side-pair headers and positional oriented uses.
        pub stored;
    },
    zero_entity_ownership_roots: CatiaZeroEntityOwnershipRoot {
        /// Complete zero-entity face-roster, shell, and body roots.
        pub stored;
    },
    zero_entity_endpoint_pair_candidates: CatiaZeroEntityEndpointPairCandidate {
        /// Zero-entity endpoint pairs established by radial support occurrences.
        pub stored;
    },
    zero_entity_records: CatiaZeroEntityRecord {
        /// Complete zero-entity framed-record identity namespace.
        pub stored;
    },
    zero_entity_support_runs: CatiaZeroEntitySupportRun {
        /// Zero-entity surface carriers and their face-local support tapes.
        pub stored;
    },
    zero_entity_endpoint_locus_candidates: CatiaZeroEntityEndpointLocusCandidate {
        /// Geometric endpoint loci established by complete endpoint-pair endpoint cliques.
        pub stored;
    },
    zero_entity_vertex_incidences: CatiaZeroEntityVertexIncidence {
        /// Zero-entity counted vertex-incidence records.
        pub stored;
    },
}
const CATIA_CATALOGUE: Catalogue<
    'static,
    CatiaArenaProjection,
    (),
    cadmpeg_ir::NativeNamespace,
    (),
> = Catalogue::new(CATIA_FAMILIES, None);

fn store_projection(
    projection: &CatiaArenaProjection,
    namespace: &mut cadmpeg_ir::NativeNamespace,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    namespace.set_version(
        std::num::NonZeroU32::new(CATIA_NATIVE_VERSION).expect("CATIA native version is nonzero"),
    );
    CATIA_CATALOGUE.emit_all(projection, namespace)?;
    debug_assert!(CATIA_ARENA_NAMES
        .iter()
        .all(|name| namespace.arenas.contains_key(*name)));
    Ok(())
}

fn consolidated_circles(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<CatiaConsolidatedCircle> {
    crate::families::b2::records::b2_circles_from_records(bytes, records)
        .into_iter()
        .enumerate()
        .map(|(index, circle)| CatiaConsolidatedCircle {
            id: format!("catia:consolidated:circle#{index}"),
            byte_offset: circle.pos as u64,
            layout: circle.layout,
            record_id: circle.record_id,
            frame_token: circle.frame_token,
            center_pair: circle.center_pair,
            radius: circle.radius,
            range: circle.range,
            full_circle: circle.full_circle,
            chart_shift: circle.chart_shift,
        })
        .collect()
}

fn legacy_entity_runs(bytes: &[u8]) -> Vec<CatiaLegacyEntityRun> {
    legacy_entity::parse_runs(bytes)
        .into_iter()
        .enumerate()
        .map(|(index, run)| {
            let id = format!("catia:legacy:entity-run#{index:08}");
            let byte_offset = run
                .identities
                .first()
                .expect("legacy run has identity one")
                .offset;
            CatiaLegacyEntityRun {
                id: id.clone(),
                byte_offset: byte_offset as u64,
                byte_len: (run.catalog_offset - byte_offset) as u64,
                catalog_offset: run.catalog_offset as u64,
                schema_program: run.schema_program.map(|program| CatiaLegacySchemaProgram {
                    byte_offset: program.offset as u64,
                    boundary_byte_offset: program.boundary_offset as u64,
                    boundary: match program.boundary {
                        legacy_entity::LegacySchemaProgramBoundary::VendorFooter => {
                            CatiaLegacySchemaProgramBoundary::VendorFooter
                        }
                        legacy_entity::LegacySchemaProgramBoundary::StreamDirectory => {
                            CatiaLegacySchemaProgramBoundary::StreamDirectory
                        }
                    },
                    data: program.bytes,
                    identifiers: program
                        .identifiers
                        .into_iter()
                        .map(|identifier| CatiaLegacySchemaIdentifier {
                            byte_offset: identifier.offset as u64,
                            value: identifier.value,
                        })
                        .collect(),
                }),
                outer_container: None,
                identities: run
                    .identities
                    .into_iter()
                    .map(|identity| CatiaLegacyEntityIdentity {
                        byte_offset: identity.offset as u64,
                        entity_id: identity.entity_id,
                        lead: identity.lead,
                    })
                    .collect(),
                role_selectors: run
                    .role_selectors
                    .into_iter()
                    .map(|role| CatiaLegacyRoleSelector {
                        byte_offset: role.offset as u64,
                        entity_id: role.entity_id,
                        name: role.name.into(),
                        encoding: Some(match role.encoding {
                            legacy_entity::LegacyRoleSelectorEncoding::FixedU32 => {
                                CatiaLegacyRoleSelectorEncoding::FixedU32
                            }
                            legacy_entity::LegacyRoleSelectorEncoding::Paged => {
                                CatiaLegacyRoleSelectorEncoding::Paged
                            }
                        }),
                        selector: role.selector,
                        field_code: role.field_code,
                    })
                    .collect(),
                text_fields: run
                    .text_fields
                    .into_iter()
                    .map(|field| CatiaLegacyTextField {
                        byte_offset: field.offset as u64,
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
                        role: field.role.map(|role| CatiaLegacyRoleSelector {
                            byte_offset: role.offset as u64,
                            entity_id: role.entity_id,
                            name: role.name.into(),
                            encoding: Some(match role.encoding {
                                legacy_entity::LegacyRoleSelectorEncoding::FixedU32 => {
                                    CatiaLegacyRoleSelectorEncoding::FixedU32
                                }
                                legacy_entity::LegacyRoleSelectorEncoding::Paged => {
                                    CatiaLegacyRoleSelectorEncoding::Paged
                                }
                            }),
                            selector: role.selector,
                            field_code: role.field_code,
                        }),
                        value: field.value,
                    })
                    .collect(),
                schema_fields: run
                    .schema_fields
                    .into_iter()
                    .map(|field| CatiaLegacySchemaField {
                        byte_offset: field.offset as u64,
                        entity_id: field.entity_id,
                        role_byte_offset: field.role_offset as u64,
                        boundary_role_byte_offset: field.boundary_role_offset as u64,
                        field_code: field.field_code,
                        payload: field.payload,
                    })
                    .collect(),
                relations: run
                    .relations
                    .into_iter()
                    .map(|relation| {
                        let parameter = |parameter: legacy_entity::LegacyRelationParameter| {
                            CatiaLegacyRelationParameter {
                                parameter: parameter.parameter,
                                value_type: parameter.value_type,
                            }
                        };
                        CatiaLegacyRelation {
                            entity_id: relation.entity_id,
                            body_selector: relation.body_selector,
                            parameter_selector: relation.parameter_selector,
                            parameter_entity_id: relation.parameter_entity_id,
                            expression_offset: relation.expression_offset as u64,
                            expression: relation.expression,
                            signature_offset: relation.signature_offset as u64,
                            type_signature: relation.type_signature,
                            inputs: relation
                                .signature
                                .inputs
                                .into_iter()
                                .map(parameter)
                                .collect(),
                            output: relation.signature.output.map(parameter),
                            result_type: relation.signature.result_type,
                        }
                    })
                    .collect(),
                synchronous_states: run
                    .synchronous_states
                    .into_iter()
                    .map(|state| CatiaLegacyRelationSynchronousState {
                        role_byte_offset: state.role_offset as u64,
                        entity_id: state.entity_id,
                        selector: state.selector,
                        synchronous: state.synchronous,
                    })
                    .collect(),
                type_descriptors: run
                    .type_descriptors
                    .into_iter()
                    .map(|descriptor| CatiaLegacyTypeDescriptor {
                        byte_offset: descriptor.offset as u64,
                        entity_id: descriptor.entity_id,
                        value: match descriptor.value {
                            legacy_entity::LegacyTypeValue::Name(value) => {
                                CatiaLegacyTypeValue::Name { value }
                            }
                            legacy_entity::LegacyTypeValue::Selector(value) => {
                                CatiaLegacyTypeValue::Selector { value }
                            }
                        },
                    })
                    .collect(),
                scalar_values: run
                    .scalar_values
                    .into_iter()
                    .map(|value| CatiaLegacyScalarValue {
                        id: format!("catia:legacy:scalar#{index:08}-{:016}", value.offset),
                        byte_offset: value.offset as u64,
                        entity_id: value.entity_id,
                        encoding: match value.encoding {
                            legacy_entity::LegacyScalarEncoding::Named84 => {
                                CatiaLegacyScalarEncoding::Named84
                            }
                            legacy_entity::LegacyScalarEncoding::Standalone85 => {
                                CatiaLegacyScalarEncoding::Standalone85
                            }
                        },
                        name_field: value.name_offset.map(|offset| offset as u64),
                        name: value.name,
                        evaluation: match value.evaluation {
                            legacy_entity::LegacyScalarEvaluation::Value(bits) => {
                                CatiaLegacyScalarEvaluation::Value { bits }
                            }
                            legacy_entity::LegacyScalarEvaluation::Unset => {
                                CatiaLegacyScalarEvaluation::Unset
                            }
                        },
                    })
                    .collect(),
                string_values: run
                    .string_values
                    .into_iter()
                    .map(|value| CatiaLegacyStringValue {
                        id: format!("catia:legacy:string#{index:08}-{:016}", value.offset),
                        byte_offset: value.offset as u64,
                        entity_id: value.entity_id,
                        name_field: value.name_offset.map(|offset| offset as u64),
                        name: value.name,
                        value: value.value,
                    })
                    .collect(),
                integer_values: run
                    .integer_values
                    .into_iter()
                    .map(|value| CatiaLegacyIntegerValue {
                        id: format!("catia:legacy:integer#{index:08}-{:016}", value.offset),
                        byte_offset: value.offset as u64,
                        entity_id: value.entity_id,
                        encoding: match value.encoding {
                            crate::legacy_entity::LegacyIntegerEncoding::Inline => {
                                CatiaLegacyIntegerEncoding::Inline
                            }
                            crate::legacy_entity::LegacyIntegerEncoding::WideI32 => {
                                CatiaLegacyIntegerEncoding::WideI32
                            }
                        },
                        name_field: value.name_offset.map(|offset| offset as u64),
                        name: value.name,
                        value: value.value,
                    })
                    .collect(),
            }
        })
        .collect()
}

fn valid_legacy_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_alphabetic())
        && characters.all(|character| character == '_' || character.is_alphanumeric())
}

pub(crate) fn legacy_evaluated_value_name<'a>(
    roles: &[CatiaLegacyRoleSelector],
    fields: &'a [CatiaLegacyTextField],
    entity_id: u32,
    value_offset: u64,
) -> Option<&'a CatiaLegacyTextField> {
    let mut evaluation_roles = roles.iter().filter(|role| {
        role.entity_id == entity_id
            && role.field_code == Some(0x17c4)
            && role.end_offset().and_then(|offset| offset.checked_add(6)) == Some(value_offset)
    });
    let evaluation_role = evaluation_roles.next()?;
    if evaluation_roles.next().is_some() {
        return None;
    }
    let mut names = fields.iter().filter(|field| {
        field.entity_id == entity_id
            && field.byte_offset < evaluation_role.byte_offset
            && valid_legacy_identifier(&field.value)
            && field
                .role
                .as_ref()
                .is_some_and(|role| role.field_code == Some(0x1200))
    });
    let name = names.next()?;
    names.next().is_none().then_some(name)
}

fn consolidated_class61_records(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<CatiaConsolidatedClass61Record> {
    let mut class61_records =
        crate::families::b2::records::b2_counted_61_from_records(bytes, records)
            .into_iter()
            .map(|record| {
                (
                    record.pos,
                    record.header_token,
                    CatiaConsolidatedClass61Payload::Counted {
                        references: record.references,
                        tail: record.tail,
                    },
                )
            })
            .chain(
                crate::families::b2::records::b2_long_61_from_records(bytes, records)
                    .into_iter()
                    .map(|record| {
                        (
                            record.pos,
                            record.header_token,
                            CatiaConsolidatedClass61Payload::Long {
                                prefix: record.prefix,
                                members: record.members,
                                references: record.references,
                                scalar: record.scalar,
                            },
                        )
                    }),
            )
            .collect::<Vec<_>>();
    class61_records.sort_by_key(|(pos, _, _)| *pos);
    class61_records
        .into_iter()
        .enumerate()
        .map(
            |(index, (pos, header_token, payload))| CatiaConsolidatedClass61Record {
                id: format!("catia:consolidated:class61-record#{index}"),
                byte_offset: pos as u64,
                header_token,
                payload,
            },
        )
        .collect()
}

fn consolidated_class5b5c_records(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<CatiaConsolidatedClass5b5cRecord> {
    let mut control_records =
        crate::families::b2::records::b2_class5b5c_records_from_records(bytes, records);
    control_records.sort_by_key(|record| (record.source_index, record.source_offset));
    control_records
        .into_iter()
        .enumerate()
        .map(|(index, record)| CatiaConsolidatedClass5b5cRecord {
            id: format!("catia:consolidated:class5b5c-record#{index}"),
            byte_offset: record.pos as u64,
            source_index: record.source_index as u64,
            source_offset: record.source_offset as u64,
            byte_len: record.byte_len as u64,
            width: record.width,
            flag: record.flag,
            class: record.class,
            header_token: record.header_token,
            payload: record.payload,
        })
        .collect()
}

fn consolidated_groups(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<CatiaConsolidatedGroup> {
    crate::families::b2::records::b2_groups_from_records(bytes, records)
        .into_iter()
        .enumerate()
        .map(|(index, group)| CatiaConsolidatedGroup {
            id: format!("catia:consolidated:group#{index}"),
            byte_offset: group.pos as u64,
            group_type: group.group_type,
        })
        .collect()
}

fn consolidated_cone_faces(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    parameter_points: &[CatiaConsolidatedParameterPoint],
) -> Vec<CatiaConsolidatedConeFace> {
    let point_ids = parameter_points
        .iter()
        .map(|point| (point.byte_offset, point.id.clone()))
        .collect::<HashMap<_, _>>();
    let class18_ends = records
        .iter()
        .filter(|record| {
            record.family == crate::wire::records::ConsolidatedFamily::B && record.class == 0x18
        })
        .map(|record| (record.range.start, record.range.end))
        .collect::<HashMap<_, _>>();
    crate::families::b2::records::b2_cone_faces(bytes)
        .into_iter()
        .enumerate()
        .map(|(index, face)| {
            let mut positions = Vec::new();
            let mut next = face.end;
            while let Some(&end) = class18_ends.get(&next) {
                positions.push(next as u64);
                next = end;
            }
            let parameter_points = positions
                .iter()
                .map(|position| point_ids.get(position).cloned())
                .collect::<Option<Vec<_>>>()
                .unwrap_or_default();
            CatiaConsolidatedConeFace {
                id: format!("catia:consolidated:cone-face#{index}"),
                byte_offset: face.pos as u64,
                byte_len: (face.end - face.pos) as u64,
                program: face.program,
                angular_scale: face.angular_scale,
                half_angle: face.half_angle,
                parameter_points,
            }
        })
        .collect()
}

fn consolidated_cones(bytes: &[u8], records: &[ConsolidatedRecord]) -> Vec<CatiaConsolidatedCone> {
    crate::families::b2::records::b2_cones_from_records(bytes, records)
        .into_iter()
        .enumerate()
        .map(|(index, cone)| CatiaConsolidatedCone {
            id: format!("catia:consolidated:cone#{index}"),
            byte_offset: cone.pos as u64,
            apex: cone.apex,
            direction_x: cone.t1,
            direction_y: cone.t2,
            axis: cone.axis,
            half_angle: cone.half_angle,
            reference_radius: cone.reference_radius,
            angular_range: cone.angular_range,
            slant_range: cone.slant_range,
            angular_scale: cone.angular_scale,
            angular_domain: cone.angular_domain,
        })
        .collect()
}

fn consolidated_cylinders(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<CatiaConsolidatedCylinder> {
    crate::families::b2::records::b2_cylinders_from_records(bytes, records)
        .into_iter()
        .enumerate()
        .map(|(index, cylinder)| {
            let payload = if cylinder.layout == 0x62 {
                let cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
                    axis,
                    ref_direction,
                    ..
                } = cylinder.geometry
                else {
                    unreachable!("B2 cylinder parser produced a non-cylinder carrier")
                };
                CatiaConsolidatedCylinderPayload::RangeOrigin {
                    stored_vector: cylinder
                        .stored_vector
                        .expect("range-origin cylinder has its stored vector"),
                    axis: [axis.x, axis.y, axis.z],
                    reference_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
                    range_origin: cylinder
                        .range_origin
                        .expect("range-origin cylinder has its range origin"),
                }
            } else {
                match cylinder.geometry {
                    cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
                        axis,
                        ref_direction,
                        ..
                    } => {
                        let frame = (
                            cylinder.frame_token,
                            [axis.x, axis.y, axis.z],
                            [ref_direction.x, ref_direction.y, ref_direction.z],
                        );
                        match cylinder.layout {
                            0x52 => CatiaConsolidatedCylinderPayload::Layout52 {
                                frame_token: frame.0,
                                axis: frame.1,
                                reference_direction: frame.2,
                            },
                            _ => CatiaConsolidatedCylinderPayload::Layout5a {
                                frame_token: frame.0,
                                axis: frame.1,
                                reference_direction: frame.2,
                            },
                        }
                    }
                    _ => unreachable!("B2 cylinder parser produced a non-cylinder carrier"),
                }
            };
            CatiaConsolidatedCylinder {
                id: format!("catia:consolidated:cylinder#{index}"),
                byte_offset: cylinder.pos as u64,
                origin: cylinder.origin,
                radius: cylinder.radius,
                u_range: cylinder.u_range,
                v_range: cylinder.v_range,
                payload,
            }
        })
        .collect()
}

fn consolidated_embedded_cylinders(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    groups: &[CatiaConsolidatedGroup],
) -> Vec<CatiaConsolidatedEmbeddedCylinder> {
    let group_ids = groups
        .iter()
        .map(|group| (group.byte_offset, group.id.as_str()))
        .collect::<HashMap<_, _>>();
    crate::families::b2::records::b2_embedded_cylinders_from_records(bytes, records)
        .into_iter()
        .enumerate()
        .map(|(index, embedded)| {
            let cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
                axis,
                ref_direction,
                ..
            } = embedded.cylinder.geometry
            else {
                unreachable!("embedded B2 cylinder parser produced a non-cylinder carrier")
            };
            CatiaConsolidatedEmbeddedCylinder {
                id: format!("catia:consolidated:embedded-cylinder#{index}"),
                byte_offset: embedded.pos as u64,
                group: group_ids
                    .get(&(embedded.wrapper_pos as u64))
                    .expect("embedded cylinder owner came from the same group parse")
                    .to_string(),
                object_id: embedded.object_id,
                origin: embedded.cylinder.origin,
                radius: embedded.cylinder.radius,
                u_range: embedded.cylinder.u_range,
                v_range: embedded.cylinder.v_range,
                frame_token: embedded.cylinder.frame_token,
                axis: [axis.x, axis.y, axis.z],
                reference_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
            }
        })
        .collect()
}

fn consolidated_parameter_points(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<CatiaConsolidatedParameterPoint> {
    use crate::families::b2::records::B2ParameterPointPayload;

    crate::families::b2::records::b2_parameter_points_from_records(bytes, records)
        .into_iter()
        .enumerate()
        .map(|(index, point)| {
            let payload = match point.payload {
                B2ParameterPointPayload::Scalar { value } => {
                    CatiaConsolidatedParameterPointPayload::Scalar { value }
                }
                B2ParameterPointPayload::Uv { uv } => {
                    CatiaConsolidatedParameterPointPayload::Uv { uv }
                }
                B2ParameterPointPayload::StationUv { station, uv } => {
                    CatiaConsolidatedParameterPointPayload::StationUv { station, uv }
                }
                B2ParameterPointPayload::FiveScalars { values } => {
                    CatiaConsolidatedParameterPointPayload::FiveScalars { values }
                }
            };
            CatiaConsolidatedParameterPoint {
                id: format!("catia:consolidated:parameter-point#{index}"),
                byte_offset: point.pos as u64,
                byte_len: (point.end - point.pos) as u64,
                prefix: point.prefix,
                control: point.control,
                payload,
            }
        })
        .collect()
}

fn consolidated_plane_carriers(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<CatiaConsolidatedPlaneCarrier> {
    use crate::families::b2::records::B2PlaneCarrierPayload;

    crate::families::b2::records::b2_plane_carriers_from_records(bytes, records)
        .into_iter()
        .enumerate()
        .map(|(index, carrier)| {
            let payload = match carrier.payload {
                B2PlaneCarrierPayload::PointDirection2 {
                    point,
                    direction,
                    tail,
                } => CatiaConsolidatedPlaneCarrierPayload::PointDirection2 {
                    point,
                    direction,
                    tail,
                },
                B2PlaneCarrierPayload::PointDirection3 {
                    point,
                    direction,
                    tail,
                } => CatiaConsolidatedPlaneCarrierPayload::PointDirection3 {
                    point,
                    direction,
                    tail,
                },
                B2PlaneCarrierPayload::PointTail { point, tail } => {
                    CatiaConsolidatedPlaneCarrierPayload::PointTail { point, tail }
                }
                B2PlaneCarrierPayload::ScalarLane { selector, values } => {
                    CatiaConsolidatedPlaneCarrierPayload::ScalarLane { selector, values }
                }
            };
            CatiaConsolidatedPlaneCarrier {
                id: format!("catia:consolidated:plane-carrier#{index}"),
                byte_offset: carrier.pos as u64,
                byte_len: (carrier.end - carrier.pos) as u64,
                width: carrier.width,
                flag: carrier.flag,
                header_token: carrier.header_token,
                payload,
            }
        })
        .collect()
}

fn consolidated_reference_lists(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<CatiaConsolidatedReferenceList> {
    crate::families::b2::records::b2_reference_lists_from_records(bytes, records)
        .into_iter()
        .enumerate()
        .map(|(index, list)| CatiaConsolidatedReferenceList {
            id: format!("catia:consolidated:reference-list#{index}"),
            byte_offset: list.pos as u64,
            references: list.references,
        })
        .collect()
}

fn consolidated_pcurves(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<CatiaConsolidatedPcurve> {
    let mut pcurves = crate::families::a5a8::records::a5_pcurves_from_records(bytes, records)
        .into_iter()
        .map(|pcurve| (pcurve, CatiaConsolidatedFamily::A))
        .chain(
            crate::families::b2::records::b2_pcurves_from_records(bytes, records)
                .into_iter()
                .map(|pcurve| (pcurve, CatiaConsolidatedFamily::B)),
        )
        .collect::<Vec<_>>();
    pcurves.sort_by_key(|(pcurve, _)| pcurve.pos);
    pcurves
        .into_iter()
        .enumerate()
        .map(|(index, (pcurve, family))| CatiaConsolidatedPcurve {
            id: format!("catia:consolidated:pcurve#{index}"),
            byte_offset: pcurve.pos as u64,
            family,
            support_id: pcurve.support_id,
            degree: pcurve.degree,
            extrapolation_sites: pcurve.extrapolation_sites,
            knots: pcurve.knots,
            points: pcurve.points,
            first_derivatives: pcurve.first_derivatives,
            second_derivatives: pcurve.second_derivatives,
            range: pcurve.range,
            tail: pcurve.tail,
        })
        .collect()
}

fn consolidated_revolutions(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    circles: &[CatiaConsolidatedCircle],
) -> Vec<CatiaConsolidatedRevolution> {
    let resolved_profiles =
        crate::families::b2::records::b2_resolved_revolutions_from_records(bytes, records)
            .into_iter()
            .map(|resolved| (resolved.revolution.pos as u64, resolved.profile.pos as u64))
            .collect::<HashMap<_, _>>();
    let circle_ids = circles
        .iter()
        .map(|circle| (circle.byte_offset, circle.id.clone()))
        .collect::<HashMap<_, _>>();
    crate::families::b2::records::b2_revolutions_from_records(bytes, records)
        .into_iter()
        .enumerate()
        .map(|(index, revolution)| CatiaConsolidatedRevolution {
            id: format!("catia:consolidated:revolution#{index}"),
            byte_offset: revolution.pos as u64,
            reference_token: revolution.reference_token,
            profile_allocation_id: revolution.profile_allocation_id,
            origin: revolution.origin,
            direction_x: revolution.direction_x,
            direction_y: revolution.direction_y,
            axis: revolution.axis,
            angular_range: revolution.angular_range,
            profile_range: revolution.profile_range,
            profile_circle: resolved_profiles
                .get(&(revolution.pos as u64))
                .and_then(|offset| circle_ids.get(offset))
                .cloned(),
            angular_scale: revolution.angular_scale,
        })
        .collect()
}

fn consolidated_line_profiles(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<CatiaConsolidatedLineProfile> {
    crate::families::b2::records::b2_line_profiles_from_records(bytes, records)
        .into_iter()
        .enumerate()
        .map(|(index, line)| CatiaConsolidatedLineProfile {
            id: format!("catia:consolidated:line-profile#{index}"),
            byte_offset: line.pos as u64,
            origin: line.origin,
            direction: line.direction,
            range: line.range,
        })
        .collect()
}

fn consolidated_spheres(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<CatiaConsolidatedSphere> {
    crate::families::b2::records::b2_spheres_from_records(bytes, records)
        .into_iter()
        .enumerate()
        .map(|(index, sphere)| CatiaConsolidatedSphere {
            id: format!("catia:consolidated:sphere#{index}"),
            byte_offset: sphere.pos as u64,
            center: sphere.center,
            direction_x: sphere.direction_x,
            direction_y: sphere.direction_y,
            axis: sphere.axis,
            radius: sphere.radius,
            azimuth_range: sphere.azimuth_range,
            latitude_range: sphere.latitude_range,
        })
        .collect()
}

fn consolidated_tori(bytes: &[u8], records: &[ConsolidatedRecord]) -> Vec<CatiaConsolidatedTorus> {
    crate::families::b2::records::b2_tori_from_records(bytes, records)
        .into_iter()
        .enumerate()
        .map(|(index, torus)| CatiaConsolidatedTorus {
            id: format!("catia:consolidated:torus#{index}"),
            byte_offset: torus.pos as u64,
            center: torus.center,
            direction_x: torus.direction_x,
            direction_y: torus.direction_y,
            axis: torus.axis,
            major_radius: torus.major_radius,
            minor_radius: torus.minor_radius,
            major_angular_range: torus.major_angular_range,
            major_angular_domain: torus.major_angular_domain,
            minor_angular_range: torus.minor_angular_range,
            minor_angular_domain: torus.minor_angular_domain,
            major_scale: torus.major_scale,
            minor_scale: torus.minor_scale,
        })
        .collect()
}

fn zero_entity_support_runs(
    runs: Vec<crate::families::zero_entity::records::ZeroEntitySupportRun>,
    records: &[CatiaZeroEntityRecord],
) -> Vec<CatiaZeroEntitySupportRun> {
    runs.into_iter()
        .enumerate()
        .map(|(index, run)| CatiaZeroEntitySupportRun {
            id: format!("catia:zero-entity:support-run#{index}"),
            carrier_byte_offset: run.carrier_pos as u64,
            carrier_record_ordinal: run.carrier_record_ordinal,
            face: run.face.map(|face| CatiaZeroEntityFace {
                byte_offset: face.pos as u64,
                record_ordinal: face.record_ordinal,
                tag: face.tag,
                allocations: face.allocations,
                loop_terminals: face.loop_terminals,
                loops: face
                    .loops
                    .into_iter()
                    .map(|loop_record| {
                        let typed_records = loop_record
                            .typed_references
                            .iter()
                            .map(|ordinal| {
                                zero_entity_record(records, *ordinal)
                                    .map(|record| record.id.clone())
                            })
                            .collect::<Option<Vec<_>>>()
                            .unwrap_or_default();
                        CatiaZeroEntityLoop {
                            byte_offset: loop_record.pos as u64,
                            record_ordinal: loop_record.record_ordinal,
                            tag: loop_record.tag,
                            member_ids: loop_record.member_ids,
                            typed_references: loop_record.typed_references,
                            typed_records,
                            support_record_ordinals: loop_record.support_record_ordinals,
                            terminal_id: loop_record.terminal_id,
                            gap: loop_record.gap,
                            loop_class: loop_record.loop_class,
                            forward_senses: loop_record.forward_senses,
                            oriented_model_endpoints: loop_record.oriented_model_endpoints,
                        }
                    })
                    .collect(),
                terminal_control: face.terminal_control,
            }),
            supports: run
                .supports
                .into_iter()
                .map(|support| CatiaZeroEntitySupportOccurrence {
                    byte_offset: support.pos as u64,
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
                })
                .collect(),
        })
        .collect()
}

fn zero_entity_endpoint_pair_candidates(
    candidates: Vec<crate::families::zero_entity::topology::ZeroEntityEndpointPairCandidate>,
) -> Vec<CatiaZeroEntityEndpointPairCandidate> {
    candidates
        .into_iter()
        .enumerate()
        .map(|(index, candidate)| CatiaZeroEntityEndpointPairCandidate {
            id: format!("catia:zero-entity:endpoint-pair-candidate#{index}"),
            face_records: candidate
                .face_record_ordinals
                .map(|ordinal| format!("catia:zero-entity:record#{ordinal}")),
            support_records: candidate
                .support_record_ordinals
                .map(|ordinal| format!("catia:zero-entity:record#{ordinal}")),
            model_endpoints: candidate.model_endpoints,
            model_midpoint: candidate.model_midpoint,
        })
        .collect()
}

fn zero_entity_endpoint_locus_candidates(
    candidates: Vec<crate::families::zero_entity::topology::ZeroEntityEndpointLocusCandidate>,
    endpoint_pairs: &[CatiaZeroEntityEndpointPairCandidate],
) -> Vec<CatiaZeroEntityEndpointLocusCandidate> {
    candidates
        .into_iter()
        .enumerate()
        .map(|(index, candidate)| CatiaZeroEntityEndpointLocusCandidate {
            id: format!("catia:zero-entity:endpoint-locus-candidate#{index}"),
            incident_endpoint_pair_endpoints: candidate
                .incident_endpoint_pair_endpoints
                .into_iter()
                .map(
                    |(pair, endpoint_index)| CatiaZeroEntityEndpointPairEndpoint {
                        endpoint_pair: endpoint_pairs[pair].id.clone(),
                        endpoint_index,
                    },
                )
                .collect(),
            representative_point: candidate.representative_point,
            maximum_deviation: candidate.maximum_deviation,
        })
        .collect()
}

fn zero_entity_edge_strides(bytes: &[u8], range: Range<usize>) -> Vec<CatiaZeroEntityEdgeStride> {
    crate::families::zero_entity::records::zero_entity_edge_strides_in_range(bytes, range)
        .into_iter()
        .enumerate()
        .map(|(index, record)| CatiaZeroEntityEdgeStride {
            id: format!("catia:zero-entity:edge-stride#{index}"),
            byte_offset: record.pos as u64,
            record_ordinal: record.record_ordinal,
            allocations: record.allocations,
            topology_refs: record.topology_refs,
            surface_support_refs: record.surface_support_refs,
        })
        .collect()
}

fn zero_entity_oriented_use_pairs(
    bytes: &[u8],
    range: Range<usize>,
) -> Vec<CatiaZeroEntityOrientedUsePair> {
    crate::families::zero_entity::records::zero_entity_oriented_use_pairs_in_range(bytes, range)
        .into_iter()
        .enumerate()
        .map(|(index, pair)| CatiaZeroEntityOrientedUsePair {
            id: format!("catia:zero-entity:oriented-use-pair#{index}"),
            header_byte_offset: pair.header_pos as u64,
            header_record_ordinal: pair.header_record_ordinal,
            base_columns: pair.base_columns,
            uses: pair.uses.map(|use_| CatiaZeroEntityOrientedUse {
                byte_offset: use_.pos as u64,
                record_ordinal: use_.record_ordinal,
                side: use_.side,
                allocations: use_.allocations,
            }),
        })
        .collect()
}

fn zero_entity_ownership_roots(
    bytes: &[u8],
    range: Range<usize>,
) -> Vec<CatiaZeroEntityOwnershipRoot> {
    crate::families::zero_entity::records::zero_entity_ownership_roots_in_range(bytes, range)
        .into_iter()
        .enumerate()
        .map(|(index, root)| CatiaZeroEntityOwnershipRoot {
            id: format!("catia:zero-entity:ownership-root#{index}"),
            face_roster_byte_offset: root.face_roster_pos as u64,
            face_roster_record_ordinal: root.face_roster_record_ordinal,
            face_slots: root.face_slots,
            shell_byte_offset: root.shell_pos as u64,
            shell_record_ordinal: root.shell_record_ordinal,
            body_byte_offset: root.body_pos as u64,
            body_record_ordinal: root.body_record_ordinal,
        })
        .collect()
}

fn zero_entity_vertex_incidences(
    bytes: &[u8],
    range: Range<usize>,
    records: &[CatiaZeroEntityRecord],
) -> Vec<CatiaZeroEntityVertexIncidence> {
    crate::families::zero_entity::records::zero_entity_vertex_incidences_in_range(bytes, range)
        .into_iter()
        .enumerate()
        .map(|(index, record)| {
            let vertex_record = zero_entity_vertex_owner(records, record.record_ordinal)
                .map(|owner| owner.id.clone());
            CatiaZeroEntityVertexIncidence {
                id: format!("catia:zero-entity:vertex-incidence#{index}"),
                byte_offset: record.pos as u64,
                record_ordinal: record.record_ordinal,
                tag: record.tag,
                allocations: record.allocations,
                vertex_record,
            }
        })
        .collect()
}

fn zero_entity_records(bytes: &[u8], range: Range<usize>) -> Vec<CatiaZeroEntityRecord> {
    crate::families::zero_entity::records::zero_entity_record_inventory_in_range(bytes, range)
        .into_iter()
        .map(|record| CatiaZeroEntityRecord {
            id: format!("catia:zero-entity:record#{}", record.record_ordinal),
            byte_offset: record.pos as u64,
            logical_end: record.end as u64,
            tag: record.tag,
            record_ordinal: record.record_ordinal,
        })
        .collect()
}

fn consolidated_owner_packets(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
) -> Vec<CatiaConsolidatedOwnerPacket> {
    let owner_charts = crate::families::b2::records::b2_owner_charts_from_records(bytes, records)
        .into_iter()
        .map(|chart| {
            let native_reference =
                |reference: crate::families::b2::records::B2OwnerChartBridgeReference| {
                    CatiaOwnerChartBridgeReference {
                        value: reference.value,
                        encoding: native_allocation_reference_encoding(reference.encoding),
                        alias_row: None,
                        canonical_surface_tag: None,
                    }
                };
            (
                (chart.source_index, chart.owner_pos),
                CatiaOwnerChartRelation {
                    carrier_byte_offset: chart.carrier_pos as u64,
                    carrier: match chart.carrier {
                        crate::families::b2::records::B2OwnerChartCarrier::B28 => {
                            CatiaOwnerChartCarrier::B28
                        }
                        crate::families::b2::records::B2OwnerChartCarrier::B2b => {
                            CatiaOwnerChartCarrier::B2b
                        }
                        crate::families::b2::records::B2OwnerChartCarrier::A32 => {
                            CatiaOwnerChartCarrier::A32
                        }
                    },
                    bridge: match chart.bridge {
                        crate::families::b2::records::B2OwnerChartBridge::SupportedSurface {
                            pos,
                            carrier_surface,
                            support_surfaces,
                            support_pcurves,
                            controls,
                            construction_radius,
                        } => CatiaOwnerChartBridge::SupportedSurface {
                            byte_offset: pos as u64,
                            carrier_surface: native_reference(carrier_surface),
                            support_surfaces: support_surfaces.map(native_reference),
                            support_pcurves: support_pcurves.map(native_reference),
                            controls,
                            construction_radius,
                        },
                        crate::families::b2::records::B2OwnerChartBridge::Extended {
                            pos,
                            references,
                            controls,
                            terminal_controls,
                        } => CatiaOwnerChartBridge::Extended {
                            byte_offset: pos as u64,
                            references: references.map(native_reference),
                            controls,
                            terminal_controls,
                        },
                    },
                    side_axis: match chart.side_axis {
                        crate::families::b2::records::B2OwnerChartSideAxis::FirstParameter => {
                            CatiaOwnerChartSideAxis::FirstParameter
                        }
                        crate::families::b2::records::B2OwnerChartSideAxis::SecondParameter => {
                            CatiaOwnerChartSideAxis::SecondParameter
                        }
                    },
                    parameter_point_byte_offsets: chart
                        .parameter_points
                        .map(|point| point.pos as u64),
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let mut identity_targets = HashMap::<(usize, usize), Vec<CatiaOwnerIdentityTarget>>::new();
    for target in
        crate::families::b2::records::b2_owner_identity_targets_from_records(bytes, records)
    {
        identity_targets
            .entry((target.source_index, target.owner_pos))
            .or_default()
            .push(CatiaOwnerIdentityTarget {
                slot: target.slot,
                distance: target.distance,
                target_byte_offset: target.target_pos as u64,
                target_class: target.target_class,
            });
    }
    let boundary_cycles =
        crate::families::consolidated::records::consolidated_owner_boundary_cycles_from_records(
            bytes, records,
        )
        .into_iter()
        .map(|cycle| {
            (
                (cycle.source_index, cycle.owner_pos),
                CatiaOwnerBoundaryCycle {
                    face_node: cycle.face_node.map(|face_node| CatiaFaceNodeRelation {
                        byte_offset: face_node.pos as u64,
                        byte_len: (cycle.owner_pos - face_node.pos) as u64,
                        header_token: face_node.header_token,
                        target_encoding: match face_node.target_encoding {
                            crate::families::b2::records::B2FaceNode5fTargetEncoding::Compact => {
                                CatiaFaceNodeTargetEncoding::Compact
                            }
                            crate::families::b2::records::B2FaceNode5fTargetEncoding::TaggedU16Strong => {
                                CatiaFaceNodeTargetEncoding::TaggedU16Strong
                            }
                        },
                        target: face_node.target,
                        terminal: face_node.terminal,
                    }),
                    edges: cycle.edges.map(|edge| CatiaOwnerBoundaryEdge {
                        slot: edge.slot,
                        byte_offset: edge.target_pos as u64,
                        endpoint_records: edge.endpoint_records.map(|pos| pos as u64),
                    }),
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let face_nodes =
        crate::families::b2::records::b2_adjacent_face_owners_from_records(bytes, records)
            .into_iter()
            .map(|linked| {
                (
                    (linked.owner.source_index, linked.owner.pos),
                    linked.face_node,
                )
            })
            .chain(
                crate::families::b2::records::b2_adjacent_face_counted_owners_from_records(
                    bytes, records,
                )
                .into_iter()
                .map(|linked| {
                    (
                        (linked.owner.source_index, linked.owner.pos),
                        linked.face_node,
                    )
                }),
            )
            .collect::<HashMap<_, _>>();
    let fixed = crate::families::b2::records::b2_owner_packets_from_records(bytes, records);
    let fixed_positions = fixed
        .iter()
        .map(|packet| (packet.source_index, packet.pos))
        .collect::<HashSet<_>>();
    let mut packets = fixed
        .into_iter()
        .map(|packet| {
            (
                packet.pos,
                packet.source_index,
                packet.header_token,
                CatiaOwnerPacketPayload::FixedNine {
                    reference_encoding: match packet.reference_encoding {
                        crate::families::b2::records::B2OwnerReferenceEncoding::TaggedU16Strong => {
                            CatiaOwnerReferenceEncoding::TaggedU16Strong
                        }
                        crate::families::b2::records::B2OwnerReferenceEncoding::WidthCodedStrong => {
                            CatiaOwnerReferenceEncoding::WidthCodedStrong
                        }
                        crate::families::b2::records::B2OwnerReferenceEncoding::AllCompact => {
                            CatiaOwnerReferenceEncoding::AllCompact
                        }
                    },
                    references: packet.references,
                    identity_encodings: packet.identity_encodings.map(|encoding| match encoding {
                        crate::families::b2::records::B2OwnerIdentityEncoding::Allocation(
                            encoding,
                        ) => CatiaOwnerIdentityEncoding::Allocation(
                            native_allocation_reference_encoding(encoding),
                        ),
                        crate::families::b2::records::B2OwnerIdentityEncoding::RawU8 => {
                            CatiaOwnerIdentityEncoding::RawU8
                        }
                    }),
                    numeric_tail: CatiaOwnerNumericTail {
                        header: packet.numeric_tail.header,
                        lower: packet.numeric_tail.lower,
                        upper: packet.numeric_tail.upper,
                        bounds: packet.numeric_tail.bounds,
                    },
                },
            )
        })
        .chain(
            crate::families::b2::records::b2_counted_owners_from_records(bytes, records)
                .into_iter()
                .filter(|packet| !fixed_positions.contains(&(packet.source_index, packet.pos)))
                .map(|packet| {
                    (
                        packet.pos,
                        packet.source_index,
                        packet.header_token,
                        CatiaOwnerPacketPayload::Counted {
                            references: packet.references,
                            tail: packet.tail,
                        },
                    )
                }),
        )
        .collect::<Vec<_>>();
    packets.sort_by_key(|(pos, source_index, _, _)| (*pos, *source_index));
    packets
        .into_iter()
        .map(
            |(pos, source_index, header_token, payload)| CatiaConsolidatedOwnerPacket {
                id: format!("catia:consolidated:owner-packet#{pos:010}"),
                byte_offset: pos as u64,
                source_index,
                header_token,
                payload,
                identity_targets: identity_targets
                    .remove(&(source_index, pos))
                    .unwrap_or_default(),
                face_node: face_nodes
                    .get(&(source_index, pos))
                    .map(|face_node| CatiaFaceNodeRelation {
                    byte_offset: face_node.pos as u64,
                    byte_len: (pos - face_node.pos) as u64,
                    header_token: face_node.header_token,
                    target_encoding: match face_node.target_encoding {
                        crate::families::b2::records::B2FaceNode5fTargetEncoding::Compact => {
                            CatiaFaceNodeTargetEncoding::Compact
                        }
                        crate::families::b2::records::B2FaceNode5fTargetEncoding::TaggedU16Strong => {
                            CatiaFaceNodeTargetEncoding::TaggedU16Strong
                        }
                    },
                    target: face_node.target,
                        terminal: face_node.terminal,
                    }),
                owner_chart: owner_charts.get(&(source_index, pos)).cloned(),
                boundary_cycle: boundary_cycles.get(&(source_index, pos)).copied(),
            },
        )
        .collect()
}

fn consolidated_edge_runs(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    pcurves: &[CatiaConsolidatedPcurve],
    nodes: &[CatiaConsolidatedEdgeNode],
) -> Vec<CatiaConsolidatedEdgeRun> {
    let pcurve_ids = pcurves
        .iter()
        .map(|pcurve| (pcurve.byte_offset, pcurve.id.clone()))
        .collect::<HashMap<_, _>>();
    let resolved =
        crate::families::consolidated::records::resolve_consolidated_edge_blocks_from_records(
            bytes, records,
        )
        .into_iter()
        .map(|block| (block.block.pcurves[0].pos, block))
        .collect::<HashMap<_, _>>();
    let nodes_by_offset = nodes
        .iter()
        .map(|node| (node.byte_offset, node))
        .collect::<HashMap<_, _>>();
    crate::families::consolidated::records::consolidated_topology_edge_runs_from_records(
        bytes, records,
    )
    .into_iter()
    .filter_map(|run| {
        if !run.edge.co_parametric || !run.identity_chain_consistent {
            return None;
        }
        let pcurve_offsets = run.edge.pcurves.each_ref().map(|pcurve| pcurve.pos as u64);
        Some((run, pcurve_offsets))
    })
    .enumerate()
    .filter_map(|(index, (run, pcurve_offsets))| {
        let resolved = resolved.get(&run.edge.pcurves[0].pos);
        let node = nodes_by_offset.get(&(run.node.pos as u64))?;
        node.uses.as_ref()?;
        Some(CatiaConsolidatedEdgeRun {
            id: format!("catia:consolidated:edge-run#{index}"),
            byte_offset: pcurve_offsets[0],
            pcurves: [
                pcurve_ids.get(&pcurve_offsets[0])?.clone(),
                pcurve_ids.get(&pcurve_offsets[1])?.clone(),
            ],
            parameter_range: run.edge.parameters.range,
            tolerance: run.edge.parameters.tolerance,
            node: node.id.clone(),
            support_bindings: resolved.map_or([None, None], |resolved| {
                resolved
                    .supports
                    .each_ref()
                    .map(|binding| binding.as_ref().map(native_consolidated_support_binding))
            }),
            shared_loci: resolved
                .and_then(|resolved| resolved.shared_loci.as_ref())
                .map(|points| points.iter().map(point_coordinates).collect()),
            endpoint_loci: resolved
                .and_then(|resolved| resolved.endpoint_loci.as_ref())
                .map(|points| points.map(|point| point_coordinates(&point))),
        })
    })
    .collect()
}

fn consolidated_edge_nodes(
    bytes: &[u8],
    records: &[ConsolidatedRecord],
    circles: &[CatiaConsolidatedCircle],
) -> Vec<CatiaConsolidatedEdgeNode> {
    let circle_ids = circles
        .iter()
        .map(|circle| (circle.byte_offset, circle.id.as_str()))
        .collect::<HashMap<_, _>>();
    let frames = records
        .iter()
        .filter(|record| {
            record.family == crate::wire::records::ConsolidatedFamily::B && record.class == 0x5e
        })
        .map(|record| {
            (
                record.range.start,
                (record.width, record.flag, record.source_index),
            )
        })
        .collect::<HashMap<_, _>>();
    let owned_nodes =
        crate::families::consolidated::records::consolidated_owned_edge_nodes_from_records(
            bytes, records,
        )
        .into_iter()
        .map(|owned| (owned.node.pos, (owned.owner_pos, owned.allocation_ordinal)))
        .collect::<HashMap<_, _>>();
    let compact_endpoints =
        crate::families::consolidated::records::consolidated_compact_edge_endpoints_from_records(
            bytes, records,
        )
        .into_iter()
        .map(|binding| {
            (
                binding.node.pos,
                binding.endpoint_records.map(|pos| pos as u64),
            )
        })
        .collect::<HashMap<_, _>>();
    let use_runs = crate::families::consolidated::records::consolidated_edge_use_runs_from_records(
        bytes, records,
    )
    .into_iter()
    .filter_map(|run| {
        if !run.identity_chain_consistent {
            return None;
        }
        Some((
            run.node.pos,
            (
                native_consolidated_edge_uses(&run.uses)?,
                run.definition.map(native_consolidated_edge_definition),
            ),
        ))
    })
    .collect::<HashMap<_, _>>();
    let analytic_circles =
        crate::families::consolidated::records::consolidated_analytic_circle_edge_runs_from_records(
            bytes, records,
        )
            .into_iter()
            .filter(|run| run.identity_chain_consistent)
            .filter_map(|run| {
                let circle = circle_ids.get(&(run.circle.pos as u64))?;
                Some((
                    run.node.pos,
                    CatiaConsolidatedAnalyticCircleBinding {
                        descriptor: CatiaConsolidatedAnalyticCircleDescriptor {
                            byte_offset: run.descriptor.pos as u64,
                            width: run.descriptor.width,
                            flag: run.descriptor.flag,
                            header_token: run.descriptor.header_token,
                            payload: run.descriptor.payload,
                        },
                        circle: (*circle).to_string(),
                    },
                ))
            })
            .collect::<HashMap<_, _>>();
    let class25_descriptors =
        crate::families::consolidated::records::consolidated_class25_edge_runs_from_records(
            bytes, records,
        )
        .into_iter()
        .filter(|run| run.identity_chain_consistent)
        .map(|run| {
            (
                run.node.pos,
                CatiaConsolidatedClass25Descriptor {
                    byte_offset: run.descriptor.pos as u64,
                    record_id: run.descriptor.record_id,
                    control: run.descriptor.control,
                    values: run.descriptor.values,
                },
            )
        })
        .collect::<HashMap<_, _>>();
    crate::families::b2::records::b2_edge_nodes_from_records(bytes, records)
        .into_iter()
        .enumerate()
        .filter_map(|(index, node)| {
            let (width, flag, source_index) = frames.get(&node.pos)?;
            let owner = owned_nodes.get(&node.pos);
            Some(CatiaConsolidatedEdgeNode {
                id: format!("catia:consolidated:edge-node#{index}"),
                byte_offset: node.pos as u64,
                source_index: *source_index,
                width: *width,
                flag: *flag,
                header_token: node.header_token,
                allocation_owner: owner
                    .map(|(pos, _)| format!("catia:consolidated:owner-packet#{pos:010}")),
                allocation_ordinal: owner.map(|(_, ordinal)| *ordinal),
                curve_ref: node.curve_ref,
                vertex_refs: [node.start_vertex_ref, node.end_vertex_ref],
                endpoint_records: compact_endpoints.get(&node.pos).copied(),
                vertices: [String::new(), String::new()],
                parameter_selectors: [node.start_parameter_ref, node.end_parameter_ref],
                reference_encodings: Some(
                    node.reference_encodings
                        .map(native_allocation_reference_encoding),
                ),
                terminal_value: Some(node.terminal_value),
                terminal_encoding: Some(native_allocation_reference_encoding(
                    node.terminal_encoding,
                )),
                tail: node.tail,
                definition: use_runs.get(&node.pos).and_then(|(_, value)| value.clone()),
                uses: use_runs.get(&node.pos).map(|(value, _)| value.clone()),
                analytic_circle: analytic_circles.get(&node.pos).cloned(),
                class25_descriptor: class25_descriptors.get(&node.pos).cloned(),
            })
        })
        .collect()
}

fn native_consolidated_edge_definition(
    definition: crate::families::consolidated::records::ConsolidatedEdgeDefinition,
) -> CatiaConsolidatedEdgeDefinition {
    CatiaConsolidatedEdgeDefinition {
        byte_offset: definition.pos as u64,
        width: definition.width,
        flag: definition.flag,
        class: definition.class,
        header_token: definition.header_token,
        payload: definition.payload,
        data: definition.data,
    }
}

fn native_allocation_reference_encoding(
    encoding: crate::wire::bytes::AllocationReferenceEncoding,
) -> CatiaAllocationReferenceEncoding {
    match encoding {
        crate::wire::bytes::AllocationReferenceEncoding::BackwardDistance => {
            CatiaAllocationReferenceEncoding::BackwardDistance
        }
        crate::wire::bytes::AllocationReferenceEncoding::OwnedChild => {
            CatiaAllocationReferenceEncoding::OwnedChild
        }
        crate::wire::bytes::AllocationReferenceEncoding::WidthCoded => {
            CatiaAllocationReferenceEncoding::WidthCoded
        }
        crate::wire::bytes::AllocationReferenceEncoding::Selector2 => {
            CatiaAllocationReferenceEncoding::Selector2
        }
        crate::wire::bytes::AllocationReferenceEncoding::TaggedU8 => {
            CatiaAllocationReferenceEncoding::TaggedU8
        }
        crate::wire::bytes::AllocationReferenceEncoding::TaggedU16 => {
            CatiaAllocationReferenceEncoding::TaggedU16
        }
    }
}

fn native_consolidated_edge_uses(
    uses: &[crate::families::b2::records::B2UseMetadata; 2],
) -> Option<CatiaConsolidatedEdgeUses> {
    let references = uses
        .iter()
        .map(|use_| use_.references.as_deref()?.try_into().ok())
        .collect::<Option<Vec<[u32; 2]>>>()?
        .try_into()
        .ok()?;
    let senses = uses
        .each_ref()
        .map(|use_| match use_.sense? {
            crate::families::b2::records::B2UseSense::Sense84 => Some(0x84),
            crate::families::b2::records::B2UseSense::Sense88 => Some(0x88),
        })
        .into_iter()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    (senses == [0x88, 0x84]).then_some(CatiaConsolidatedEdgeUses { references, senses })
}

fn consolidated_vertex_identities(
    nodes: &mut [CatiaConsolidatedEdgeNode],
) -> Vec<CatiaConsolidatedVertexIdentity> {
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    enum IdentityKey {
        EndpointRecord(u64),
        Unresolved(usize, Option<String>, u32),
    }

    let mut identities = Vec::<CatiaConsolidatedVertexIdentity>::new();
    let mut identity_indices = HashMap::<IdentityKey, usize>::new();
    for node in nodes {
        if node.endpoint_records.is_none() && node.uses.is_none() {
            continue;
        }
        for (endpoint, identity) in node.vertex_refs.into_iter().enumerate() {
            let endpoint_record = node.endpoint_records.map(|records| records[endpoint]);
            let key = endpoint_record.map_or_else(
                || {
                    IdentityKey::Unresolved(
                        node.source_index,
                        node.allocation_owner.clone(),
                        identity,
                    )
                },
                IdentityKey::EndpointRecord,
            );
            let index = *identity_indices.entry(key.clone()).or_insert_with(|| {
                let index = identities.len();
                identities.push(CatiaConsolidatedVertexIdentity {
                    id: format!("catia:consolidated:vertex-identity#{index}"),
                    identity,
                    source_index: node.source_index,
                    endpoint_record,
                    reference_values: vec![identity],
                    allocation_owner: node.allocation_owner.clone(),
                    incident_edge_nodes: Vec::new(),
                });
                index
            });
            let vertex = &mut identities[index];
            if !vertex.reference_values.contains(&identity) {
                vertex.reference_values.push(identity);
            }
            node.vertices[endpoint].clone_from(&vertex.id);
            if vertex.incident_edge_nodes.last() != Some(&node.id) {
                vertex.incident_edge_nodes.push(node.id.clone());
            }
        }
    }
    identities
}

fn point_coordinates(point: &cadmpeg_ir::math::Point3) -> [f64; 3] {
    [point.x, point.y, point.z]
}

fn native_consolidated_support_binding(
    binding: &crate::families::consolidated::records::ConsolidatedSupportBinding,
) -> CatiaConsolidatedSupportBinding {
    match binding {
        crate::families::consolidated::records::ConsolidatedSupportBinding::Cylinder { pos } => {
            CatiaConsolidatedSupportBinding::Cylinder {
                byte_offset: *pos as u64,
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::EmbeddedCylinder {
            pos,
            wrapper_pos,
        } => CatiaConsolidatedSupportBinding::EmbeddedCylinder {
            byte_offset: *pos as u64,
            wrapper_byte_offset: *wrapper_pos as u64,
        },
        crate::families::consolidated::records::ConsolidatedSupportBinding::Circle { pos } => {
            CatiaConsolidatedSupportBinding::Circle {
                byte_offset: *pos as u64,
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::Cone { pos } => {
            CatiaConsolidatedSupportBinding::Cone {
                byte_offset: *pos as u64,
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::Sphere { pos } => {
            CatiaConsolidatedSupportBinding::Sphere {
                byte_offset: *pos as u64,
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::Torus { pos } => {
            CatiaConsolidatedSupportBinding::Torus {
                byte_offset: *pos as u64,
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::Plane { pos } => {
            CatiaConsolidatedSupportBinding::Plane {
                byte_offset: *pos as u64,
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::NurbsCarrier {
            pos,
            offset,
        } => CatiaConsolidatedSupportBinding::NurbsCarrier {
            byte_offset: *pos as u64,
            offset: *offset,
        },
    }
}

fn zero_entity_record(
    records: &[CatiaZeroEntityRecord],
    ordinal: u32,
) -> Option<&CatiaZeroEntityRecord> {
    let index = usize::try_from(ordinal.checked_sub(1)?).ok()?;
    records.get(index)
}

fn zero_entity_vertex_owner(
    records: &[CatiaZeroEntityRecord],
    incidence_ordinal: u32,
) -> Option<&CatiaZeroEntityRecord> {
    let incidence = zero_entity_record(records, incidence_ordinal)?;
    let owner = zero_entity_record(records, incidence_ordinal.checked_add(1)?)?;
    (incidence.logical_end == owner.byte_offset && owner.tag == [0x5d, 0x06]).then_some(owner)
}

fn contains_extent(
    owner_start: usize,
    owner_len: usize,
    candidate_start: usize,
    candidate_len: usize,
) -> bool {
    owner_start < candidate_start
        && owner_start
            .checked_add(owner_len)
            .zip(candidate_start.checked_add(candidate_len))
            .is_some_and(|(owner_end, candidate_end)| candidate_end <= owner_end)
}

fn extents_overlap(first_start: u64, first_len: u64, second_start: u64, second_len: u64) -> bool {
    first_start
        .checked_add(first_len)
        .zip(second_start.checked_add(second_len))
        .is_some_and(|(first_end, second_end)| first_start < second_end && second_start < first_end)
}

fn finjpl_family(kind: container::FinjplKind) -> &'static str {
    match kind {
        container::FinjplKind::Storage => "storage",
        container::FinjplKind::ProjectFlags => "project-flags",
        container::FinjplKind::Other => "other",
    }
}

fn containing_finjpl_segment(
    byte_offset: u64,
    byte_len: u64,
    segments: &[CatiaFinjplSegment],
) -> Option<&str> {
    let byte_end = byte_offset.checked_add(byte_len)?;
    let mut containing = segments.iter().filter(|segment| {
        segment.byte_offset <= byte_offset
            && segment
                .byte_offset
                .checked_add(segment.byte_len)
                .is_some_and(|segment_end| byte_end <= segment_end)
    });
    let segment = containing.next()?;
    containing.next().is_none().then_some(segment.id.as_str())
}

fn preview_views(segments: &[CatiaFinjplSegment]) -> Vec<CatiaPreviewImage> {
    segments
        .iter()
        .flat_map(|segment| {
            container::preview_images(&segment.data)
                .into_iter()
                .filter_map(move |preview| {
                    Some((
                        segment
                            .byte_offset
                            .checked_add(u64::try_from(preview.range.start).ok()?)?,
                        preview,
                        segment,
                    ))
                })
        })
        .enumerate()
        .map(
            |(index, (byte_offset, preview, segment))| CatiaPreviewImage {
                id: format!("catia:outer:preview#{index}"),
                byte_offset,
                byte_len: (preview.range.end - preview.range.start) as u64,
                width: preview.width,
                height: preview.height,
                components: preview.components,
                data: segment.data[preview.range].to_vec(),
            },
        )
        .collect()
}

fn external_reference_views(segments: &[CatiaFinjplSegment]) -> Vec<CatiaExternalReference> {
    segments
        .iter()
        .flat_map(|segment| {
            container::external_references(&segment.data)
                .into_iter()
                .filter_map(move |reference| {
                    Some((
                        segment
                            .byte_offset
                            .checked_add(u64::try_from(reference.offset).ok()?)?,
                        reference,
                        segment,
                    ))
                })
        })
        .enumerate()
        .map(
            |(index, (byte_offset, reference, segment))| CatiaExternalReference {
                id: format!("catia:outer:external-reference#{index}"),
                byte_offset,
                target: reference.target,
                segment: segment.id.clone(),
            },
        )
        .collect()
}

fn resolve_alias_surface_tags(rows: &mut [CatiaAliasRow]) {
    let mut stored_by_group = HashMap::<(u32, u32), Option<u32>>::new();
    for row in rows.iter() {
        let Some(group) = row.group.as_ref() else {
            continue;
        };
        if row.lead != AliasLead::SurfaceSupportStorage {
            continue;
        }
        stored_by_group
            .entry((group.prototype, group.group_id))
            .and_modify(|stored| *stored = None)
            .or_insert(Some(row.tag));
    }
    for row in rows {
        row.canonical_surface_tag = match row.lead {
            AliasLead::SurfaceSupportStorage => Some(row.tag),
            AliasLead::NonSurfaceAlias => row.group.as_ref().and_then(|group| {
                stored_by_group
                    .get(&(group.prototype, group.group_id))
                    .copied()
                    .flatten()
            }),
            _ => None,
        };
    }
}

fn resolve_owner_chart_support_aliases(
    packets: &mut [CatiaConsolidatedOwnerPacket],
    aliases: &[CatiaAliasRow],
) {
    let mut unique_by_tag = HashMap::<u32, Option<&CatiaAliasRow>>::new();
    for alias in aliases {
        unique_by_tag
            .entry(alias.tag)
            .and_modify(|unique| *unique = None)
            .or_insert(Some(alias));
    }
    let resolve = |reference: &mut CatiaOwnerChartBridgeReference| {
        reference.alias_row = None;
        reference.canonical_surface_tag = None;
        if reference.encoding != CatiaAllocationReferenceEncoding::WidthCoded {
            return;
        }
        let Some(alias) = unique_by_tag.get(&reference.value).copied().flatten() else {
            return;
        };
        reference.alias_row = Some(alias.id.clone());
        reference.canonical_surface_tag = alias.canonical_surface_tag;
    };
    for packet in packets {
        let Some(chart) = packet.owner_chart.as_mut() else {
            continue;
        };
        let CatiaOwnerChartBridge::SupportedSurface {
            support_surfaces,
            support_pcurves,
            ..
        } = &mut chart.bridge
        else {
            continue;
        };
        for reference in support_surfaces.iter_mut().chain(support_pcurves) {
            resolve(reference);
        }
    }
}

#[cfg(test)]
fn validate_alias_surface_tags(
    rows: &[CatiaAliasRow],
    required: bool,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    if !required {
        return Ok(());
    }
    let mut expected = rows.to_vec();
    resolve_alias_surface_tags(&mut expected);
    if rows
        .iter()
        .zip(expected)
        .all(|(row, expected)| row.canonical_surface_tag == expected.canonical_surface_tag)
    {
        Ok(())
    } else {
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "alias rows have invalid canonical surface tags".to_string(),
        ))
    }
}

#[cfg(test)]
fn validate_owner_chart_support_aliases(
    packets: &[CatiaConsolidatedOwnerPacket],
    aliases: &[CatiaAliasRow],
    required: bool,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    if !required {
        return Ok(());
    }
    let mut expected = packets.to_vec();
    resolve_owner_chart_support_aliases(&mut expected, aliases);
    if packets == expected {
        Ok(())
    } else {
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "owner-chart support references have invalid alias links".to_string(),
        ))
    }
}

#[cfg(test)]
fn validate_alias_links(
    rows: &[CatiaAliasRow],
    packets: &[CatiaConsolidatedOwnerPacket],
    version: u32,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    validate_alias_surface_tags(rows, version >= CATIA_ALIAS_SURFACE_TAG_VERSION)?;
    validate_owner_chart_support_aliases(packets, rows, version >= CATIA_OWNER_CHART_ALIAS_VERSION)
}

impl CatiaNative {
    /// Decode CATIA-native records using container-bounded consolidated
    /// record sources.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn decode_with_record_ranges(bytes: &[u8], ranges: &[Range<usize>]) -> Self {
        let consolidated_records =
            crate::wire::records::consolidated_records_in_ranges(bytes, ranges.iter().cloned());
        Self::decode_with_records(bytes, &consolidated_records)
    }

    /// Decode CATIA-native records from descriptor-scoped logical sources.
    #[must_use]
    pub(crate) fn decode_with_record_sources(bytes: &[u8], sources: &[Vec<Range<usize>>]) -> Self {
        let consolidated_records =
            crate::wire::records::consolidated_records_in_sources(bytes, sources.iter().cloned());
        Self::decode_with_records(bytes, &consolidated_records)
    }

    fn decode_with_records(bytes: &[u8], consolidated_records: &[ConsolidatedRecord]) -> Self {
        let outer_directory = container::parse_outer_stream_directory(bytes);
        let outer_container_declarations =
            outer_directory.as_ref().map_or_else(Vec::new, |outer| {
                container::outer_container_declarations(bytes, outer)
            });
        let finjpl_segments = container::finjpl_segments(bytes, 0, bytes.len())
            .into_iter()
            .enumerate()
            .map(|(index, segment)| CatiaFinjplSegment {
                id: format!("catia:outer:finjpl#{index}"),
                byte_offset: segment.range.start as u64,
                byte_len: (segment.range.end - segment.range.start) as u64,
                type_word: segment.type_word,
                family: finjpl_family(segment.kind).to_string(),
                name: segment.name,
                data: bytes[segment.range].to_vec(),
            })
            .collect::<Vec<_>>();
        let mut parsed_catalogs = catalog::parse(bytes);
        let entity_runs = entity_table::parse_runs(bytes);
        let paired_object_graph_roots = entity_runs
            .iter()
            .filter_map(|run| {
                let end = run.last()?.pos.checked_add(run.last()?.total_len)?;
                (bytes.get(end) == Some(&0xde)).then_some((end + 1, run.len()))
            })
            .collect::<HashMap<_, _>>();
        let mut alias_rows = object_graph::surface_aliases(bytes)
            .into_iter()
            .map(CatiaAliasRow::from)
            .collect::<Vec<_>>();
        let mut parsed_object_graphs =
            object_graph::parse_all_with_paired_roots(bytes, &paired_object_graph_roots);
        let mut parsed_value_blocks = value_block::parse(bytes);
        parsed_value_blocks.retain(|block| {
            !parsed_object_graphs.iter().any(|graph| {
                contains_extent(graph.pos, graph.total_len, block.pos, block.total_len)
            })
        });
        parsed_object_graphs.retain(|graph| {
            !parsed_value_blocks.iter().any(|block| {
                contains_extent(block.pos, block.total_len, graph.pos, graph.total_len)
            })
        });
        parsed_catalogs.retain(|catalog| {
            !parsed_object_graphs.iter().any(|graph| {
                contains_extent(graph.pos, graph.total_len, catalog.pos, catalog.total_len)
            }) && !parsed_value_blocks.iter().any(|block| {
                contains_extent(block.pos, block.total_len, catalog.pos, catalog.total_len)
            })
        });
        let catalogs: Vec<CatiaCatalog> = parsed_catalogs
            .into_iter()
            .map(CatiaCatalog::from)
            .collect();
        let mut entity_runs = entity_runs
            .into_iter()
            .filter_map(|run| {
                let end = run.last()?.pos.checked_add(run.last()?.total_len)?;
                (bytes.get(end) == Some(&0xde)).then_some(((end + 1, run.len()), run))
            })
            .collect::<HashMap<_, _>>();
        let mut entity_records = Vec::new();
        let mut object_graphs = parsed_object_graphs
            .into_iter()
            .map(|graph| {
                let entities = entity_runs
                    .remove(&(graph.pos, graph.records.len()))
                    .unwrap_or_default();
                let finjpl_segment = containing_finjpl_segment(
                    graph.pos as u64,
                    graph.total_len as u64,
                    &finjpl_segments,
                )
                .map(str::to_owned);
                let outer_container = outer_directory
                    .as_ref()
                    .and_then(|outer| {
                        container::outer_container_for_extent(
                            outer,
                            &outer_container_declarations,
                            graph.pos as u64,
                            graph.total_len as u64,
                        )
                    })
                    .map(CatiaOuterContainerBinding::from);
                let (graph, mut entities) =
                    native_object_graph(graph, entities, finjpl_segment, outer_container);
                entity_records.append(&mut entities);
                graph
            })
            .collect::<Vec<_>>();
        for graph in &mut object_graphs {
            let catalog = graph.catalog_byte_offset.and_then(|offset| {
                catalogs
                    .iter()
                    .find(|catalog| catalog.byte_offset == offset)
            });
            graph.catalog = catalog.map(|catalog| catalog.id.clone());
            for record in &mut graph.records {
                record.class_entry = record.class_ref.and_then(|ordinal| {
                    usize::try_from(ordinal)
                        .ok()
                        .and_then(|ordinal| catalog?.entries.get(ordinal))
                        .map(|entry| entry.id.clone())
                });
                record.repeated_reference_schema_selection = repeated_reference_schema_selection(
                    record.repeated_reference_suffix.as_ref(),
                    catalog,
                );
            }
            for entity in entity_records
                .iter_mut()
                .filter(|entity| entity.object_graph == graph.id)
            {
                entity.definition_schema_selections = definition_schema_selections(
                    &entity_table::parse_definition_schema_selectors(&entity.definition_prefix),
                    catalog,
                );
                entity.value_schema_selections = entity_value_schema_selections(
                    &entity.value_fields,
                    catalog,
                    &entity.value_packets,
                );
                entity.relation_expression = relation_expression(
                    &entity.definition_schema_selections,
                    &entity.value_schema_selections,
                );
                entity.suffix_value = entity_suffix_value(&entity.record_suffix);
                entity.suffix_framing = entity_suffix_framing(&entity.record_suffix);
                entity.suffix_schema_selection =
                    entity_suffix_schema_selection(entity.suffix_value.as_ref(), catalog);
                entity.parameter_value = parameter_value(
                    entity.lead,
                    &entity.value_schema_selections,
                    entity.suffix_value.as_ref(),
                );
                entity.range_interval = range_interval(
                    &entity.value_payload,
                    &entity.value_schema_selections,
                    entity.suffix_value.as_ref(),
                    &graph.records,
                    &graph.id,
                    entity.entity_id,
                );
                entity.constraint_range = resolved_constraint_range(
                    entity.lead,
                    &entity.value_schema_selections,
                    entity.suffix_value.as_ref(),
                    &graph.records,
                    &graph.id,
                    entity.entity_id,
                );
                entity.definition_value = definition_value(
                    entity.lead,
                    &entity.definition_schema_selections,
                    &entity.value_fields,
                    entity.suffix_value.as_ref(),
                    entity.suffix_schema_selection.as_ref(),
                );
                entity.definition_chain_value = definition_chain_value(
                    entity.lead,
                    &entity.definition_schema_selections,
                    &entity.value_fields,
                    entity.suffix_value.as_ref(),
                    entity.suffix_schema_selection.as_ref(),
                );
            }
        }
        let entity_classes_by_graph_identity =
            entity_class_index(object_graphs.iter().flat_map(|graph| &graph.records));
        let (
            relation_expressions,
            relation_expression_entities,
            entities_by_graph_identity,
            terminal_nulls_by_graph,
            parameter_bindings,
        ) = semantic_entity_indices(&entity_records, &entity_classes_by_graph_identity);
        let entity_references = CatiaEntityReferenceIndex {
            entities: &entities_by_graph_identity,
            classes: &entity_classes_by_graph_identity,
            terminal_nulls: &terminal_nulls_by_graph,
        };
        for entity in &mut entity_records {
            if let Some(signature) = entity.reference_signature.take() {
                entity.reference_signature = Some(reference_signature(
                    signature.production,
                    &entity.object_graph,
                    &entity_references,
                ));
            }
            let Some(object) = object_graphs
                .iter()
                .find(|graph| graph.id == entity.object_graph)
                .and_then(|graph| {
                    graph
                        .records
                        .iter()
                        .find(|record| record.id == entity.object_record)
                })
            else {
                continue;
            };
            entity.relation_program_instance = relation_program_instance(
                entity.entity_id,
                object,
                &entity_references,
                &relation_expression_entities,
                &parameter_bindings,
            );
            entity.schema_configuration_record = schema_configuration_record(
                entity.entity_id,
                object,
                &entity.value_schema_selections,
                &entities_by_graph_identity,
                &entity_classes_by_graph_identity,
                &terminal_nulls_by_graph,
            );
            entity.schema_configuration_row_link = schema_configuration_row_link(
                entity.entity_id,
                object,
                &entities_by_graph_identity,
                &entity_classes_by_graph_identity,
                &terminal_nulls_by_graph,
            );
            entity.formula_relation = formula_relation(
                &entity.definition_schema_selections,
                entity.entity_id,
                object,
                &relation_expressions,
                &entity_references,
                &parameter_bindings,
            );
        }
        let reference_signature_cohorts = derive_reference_signature_cohorts(&entity_records);
        let schema_configuration_row_chains = derive_schema_configuration_row_chains(
            &entity_records,
            &entities_by_graph_identity,
            &entity_classes_by_graph_identity,
            &terminal_nulls_by_graph,
        );
        alias_rows.retain(|row| {
            let row_start = row.byte_offset.saturating_sub(4);
            !object_graphs
                .iter()
                .any(|graph| extents_overlap(row_start, 24, graph.byte_offset, graph.byte_len))
                && !parsed_value_blocks.iter().any(|block| {
                    extents_overlap(row_start, 24, block.pos as u64, block.total_len as u64)
                })
                && !catalogs.iter().any(|catalog| {
                    extents_overlap(row_start, 24, catalog.byte_offset, catalog.byte_len)
                })
        });
        resolve_alias_surface_tags(&mut alias_rows);
        let design_objects = design_objects(&object_graphs, &entity_records);
        let part_graph = {
            let mut graphs = object_graphs.iter().filter(|graph| {
                graph
                    .outer_container
                    .as_ref()
                    .is_some_and(|container| container.class_name == "CATPrtCont")
            });
            match (graphs.next(), graphs.next()) {
                (Some(graph), None) => Some(graph),
                _ => None,
            }
        };
        if let Some(graph) = part_graph {
            for row in &mut alias_rows {
                let Some(index) = usize::from(row.entity_record_ordinal).checked_sub(1) else {
                    continue;
                };
                let Some(record) = graph.records.get(index) else {
                    continue;
                };
                row.object_graph = Some(graph.id.clone());
                row.object_record = Some(record.id.clone());
                row.design_object.clone_from(&record.design_object);
            }
        }
        let value_blocks = parsed_value_blocks
            .into_iter()
            .filter_map(|block| {
                let catalog_pos = block.pos + block.total_len;
                let catalog = catalogs
                    .iter()
                    .find(|catalog| catalog.byte_offset == catalog_pos as u64)?;
                let object_graph = object_graphs.iter().find(|graph| {
                    graph
                        .byte_offset
                        .checked_add(graph.byte_len)
                        .is_some_and(|end| end == block.pos as u64)
                });
                Some(CatiaValueBlock::from_parts(block, catalog, object_graph))
            })
            .collect();
        let preview_images = preview_views(&finjpl_segments);
        let external_references = external_reference_views(&finjpl_segments);
        let mut legacy_entity_runs = legacy_entity_runs(bytes);
        for run in &mut legacy_entity_runs {
            run.outer_container = outer_directory
                .as_ref()
                .and_then(|outer| {
                    container::outer_container_for_extent(
                        outer,
                        &outer_container_declarations,
                        run.byte_offset,
                        run.byte_len,
                    )
                })
                .map(CatiaOuterContainerBinding::from);
        }
        let consolidated_circles = consolidated_circles(bytes, consolidated_records);
        let consolidated_class61_records =
            consolidated_class61_records(bytes, consolidated_records);
        let consolidated_class5b5c_records =
            consolidated_class5b5c_records(bytes, consolidated_records);
        let consolidated_parameter_points =
            consolidated_parameter_points(bytes, consolidated_records);
        let consolidated_cone_faces =
            consolidated_cone_faces(bytes, consolidated_records, &consolidated_parameter_points);
        let consolidated_cones = consolidated_cones(bytes, consolidated_records);
        let consolidated_cylinders = consolidated_cylinders(bytes, consolidated_records);
        let consolidated_groups = consolidated_groups(bytes, consolidated_records);
        let consolidated_embedded_cylinders =
            consolidated_embedded_cylinders(bytes, consolidated_records, &consolidated_groups);
        let consolidated_line_profiles = consolidated_line_profiles(bytes, consolidated_records);
        let mut consolidated_owner_packets =
            consolidated_owner_packets(bytes, consolidated_records);
        resolve_owner_chart_support_aliases(&mut consolidated_owner_packets, &alias_rows);
        let consolidated_pcurves = consolidated_pcurves(bytes, consolidated_records);
        let consolidated_plane_carriers = consolidated_plane_carriers(bytes, consolidated_records);
        let consolidated_reference_lists =
            consolidated_reference_lists(bytes, consolidated_records);
        let consolidated_revolutions =
            consolidated_revolutions(bytes, consolidated_records, &consolidated_circles);
        let consolidated_spheres = consolidated_spheres(bytes, consolidated_records);
        let consolidated_tori = consolidated_tori(bytes, consolidated_records);
        let zero_entity_range = container::outer_preamble_range(bytes).unwrap_or_else(|| {
            if bytes.starts_with(container::OUTER_MAGIC) {
                0..0
            } else {
                0..bytes.len()
            }
        });
        let zero_entity_records = zero_entity_records(bytes, zero_entity_range.clone());
        let zero_entity_edge_strides = zero_entity_edge_strides(bytes, zero_entity_range.clone());
        let zero_entity_oriented_use_pairs =
            zero_entity_oriented_use_pairs(bytes, zero_entity_range.clone());
        let zero_entity_ownership_roots =
            zero_entity_ownership_roots(bytes, zero_entity_range.clone());
        let parsed_zero_entity_support_runs =
            crate::families::zero_entity::records::zero_entity_support_runs_in_range(
                bytes,
                zero_entity_range.clone(),
            );
        let parsed_zero_entity_endpoint_pairs =
            crate::families::zero_entity::topology::zero_entity_endpoint_pair_candidates(
                &parsed_zero_entity_support_runs,
            );
        let zero_entity_endpoint_pair_candidates =
            zero_entity_endpoint_pair_candidates(parsed_zero_entity_endpoint_pairs.clone());
        let parsed_zero_entity_endpoint_loci =
            crate::families::zero_entity::topology::endpoint_locus_candidates(
                &parsed_zero_entity_endpoint_pairs,
            );
        let zero_entity_endpoint_locus_candidates = zero_entity_endpoint_locus_candidates(
            parsed_zero_entity_endpoint_loci,
            &zero_entity_endpoint_pair_candidates,
        );
        let zero_entity_support_runs =
            zero_entity_support_runs(parsed_zero_entity_support_runs, &zero_entity_records);
        let zero_entity_vertex_incidences =
            zero_entity_vertex_incidences(bytes, zero_entity_range, &zero_entity_records);
        let mut consolidated_edge_nodes =
            consolidated_edge_nodes(bytes, consolidated_records, &consolidated_circles);
        let consolidated_edge_runs = consolidated_edge_runs(
            bytes,
            consolidated_records,
            &consolidated_pcurves,
            &consolidated_edge_nodes,
        );
        let consolidated_vertex_identities =
            consolidated_vertex_identities(&mut consolidated_edge_nodes);
        Self {
            version: CATIA_NATIVE_VERSION,
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
        }
    }

    /// Store this namespace while moving child arenas out of their typed owners.
    pub fn store_owned(
        self,
        namespace: &mut cadmpeg_ir::NativeNamespace,
    ) -> Result<(), cadmpeg_ir::NativeConvertError> {
        store_projection(&CatiaArenaProjection::from(self), namespace)
    }
}

fn value_schema_selections(
    block_id: &str,
    block_byte_offset: u64,
    fields: &[value_block::ValueField],
    catalog: &CatiaCatalog,
) -> Vec<CatiaValueSchemaSelection> {
    let selector_indices = fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| {
            let value_block::ValueField::SchemaSelector { ordinal, .. } = field else {
                return None;
            };
            usize::try_from(*ordinal)
                .ok()
                .filter(|ordinal| *ordinal <= catalog.entries.len())
                .map(|_| index)
        })
        .collect::<Vec<_>>();
    selector_indices
        .iter()
        .enumerate()
        .filter_map(|(selector_rank, index)| match &fields[*index] {
            value_block::ValueField::SchemaSelector { ordinal, offset } => {
                let ordinal_index = usize::try_from(*ordinal).ok()?;
                if ordinal_index > catalog.entries.len() {
                    return None;
                }
                let catalog_entry = catalog.entries.get(ordinal_index);
                let entry = catalog_entry.map(|entry| entry.id.clone());
                let value_end = selector_indices
                    .get(selector_rank + 1)
                    .copied()
                    .unwrap_or(fields.len());
                let encoded_value = if entry.is_some() {
                    fields[index + 1..value_end].to_vec()
                } else {
                    Vec::new()
                };
                let byte_offset = block_byte_offset
                    .checked_add(6)?
                    .checked_add(u64::try_from(*offset).ok()?)?;
                Some(CatiaValueSchemaSelection {
                    id: format!("catia:outer:value-selection#{byte_offset:010}"),
                    parent: block_id.to_string(),
                    offset: *offset as u64,
                    ordinal: *ordinal,
                    encoded_value,
                    entry,
                    name: catalog_entry.map(|entry| entry.value.clone()),
                })
            }
            _ => None,
        })
        .collect()
}

impl CatiaValueBlock {
    fn from_parts(
        block: value_block::ValueBlock,
        catalog: &CatiaCatalog,
        object_graph: Option<&CatiaObjectGraph>,
    ) -> Self {
        let id = format!("catia:outer:value-block#{:010}", block.pos);
        let schema_selections =
            value_schema_selections(&id, block.pos as u64, &block.fields, catalog);
        Self {
            id,
            byte_offset: block.pos as u64,
            byte_len: block.total_len as u64,
            declared_len: block.declared_len as u64,
            object_graph: object_graph.map(|graph| graph.id.clone()),
            catalog: catalog.id.clone(),
            payload: block.payload,
            fields: block.fields,
            schema_selections,
        }
    }
}

impl From<object_graph::SurfaceAlias> for CatiaAliasRow {
    fn from(row: object_graph::SurfaceAlias) -> Self {
        Self {
            id: format!("catia:outer:alias-row#{:010}", row.pos),
            byte_offset: row.pos as u64,
            lead: row.lead,
            lead_raw: row.lead_raw,
            tag: row.tag,
            tag_raw: row.tag_raw,
            flag: row.flag,
            f1: row.f1,
            entity_record_ordinal: row.entity_record_ordinal,
            object_graph: None,
            object_record: None,
            design_object: None,
            f2: row.f2,
            f3: row.f3,
            group: row.group,
            canonical_surface_tag: None,
        }
    }
}

impl From<catalog::Catalog> for CatiaCatalog {
    fn from(catalog: catalog::Catalog) -> Self {
        let id = format!("catia:outer:catalog#{:010}", catalog.pos);
        let entries = catalog
            .entries
            .into_iter()
            .map(|entry| CatiaCatalogEntry {
                id: format!("catia:outer:catalog-entry#{:010}", entry.pos),
                parent: id.clone(),
                ordinal: entry.ordinal,
                byte_offset: entry.pos as u64,
                value: entry.value,
            })
            .collect();
        Self {
            id,
            byte_offset: catalog.pos as u64,
            byte_len: catalog.total_len as u64,
            declared_count: catalog.declared_count,
            entries,
        }
    }
}

fn native_object_graph(
    graph: object_graph::ObjectGraph,
    entity_records: Vec<entity_table::EntityRecord>,
    finjpl_segment: Option<String>,
    outer_container: Option<CatiaOuterContainerBinding>,
) -> (CatiaObjectGraph, Vec<CatiaEntityRecord>) {
    let id = format!("catia:outer:object-graph#{:010}", graph.pos);
    let mut records = graph
        .records
        .into_iter()
        .enumerate()
        .map(|(ordinal, record)| {
            let entity = entity_records.get(ordinal);
            CatiaObjectRecord {
                id: format!("catia:outer:object-record#{:010}", record.pos),
                parent: id.clone(),
                design_object: None,
                entity_record: entity
                    .map(|entity| format!("catia:outer:entity-record#{:010}", entity.pos)),
                entity_id: entity.map(|entity| entity.entity_id),
                ordinal: record.index as u64,
                byte_offset: record.pos as u64,
                byte_len: record.total_len as u64,
                lead: record.lead,
                head: record.head,
                inline_body: record.inline_body,
                owner: record.owner_ref.map(CatiaObjectOwner::Entity).or_else(|| {
                    record
                        .owner_literal
                        .map(CatiaObjectOwner::UnassignedLiteral)
                }),
                class_ref: record.class_ref,
                class_name: record.class_name,
                class_entry: None,
                storage_ref: record.storage_ref,
                storage_record: None,
                storage_design_object: None,
                payload: record.payload,
                repeated_reference_suffix: record.repeated_reference_suffix,
                repeated_reference_schema_selection: None,
                subtype: record.subtype,
                references: Vec::new(),
            }
        })
        .collect::<Vec<_>>();
    for record in &mut records {
        record.design_object = record
            .owner_entity_id()
            .map(|owner| design_object_id(graph.pos as u64, owner));
    }
    let record_ids = records
        .iter()
        .map(|record| record.id.clone())
        .collect::<Vec<_>>();
    let record_design_objects = records
        .iter()
        .map(|record| record.design_object.clone())
        .collect::<Vec<_>>();
    let record_indices = records
        .iter()
        .enumerate()
        .filter_map(|(index, record)| Some((record.entity_id?, index)))
        .collect::<HashMap<_, _>>();
    let terminal_null_entity_id = terminal_null_entity_id(&record_indices);
    for record in &mut records {
        (record.storage_record, record.storage_design_object) = resolved_storage_link(
            record.storage_ref,
            &record_ids,
            &record_design_objects,
            &record_indices,
        );
        record.references = resolved_payload_references(
            &record.payload,
            &record_ids,
            &record_design_objects,
            &record_indices,
            terminal_null_entity_id,
        );
    }
    let entities = entity_records
        .into_iter()
        .enumerate()
        .filter_map(|(ordinal, entity)| {
            let object_record = records.get(ordinal)?;
            let numeric_pair = entity.numeric_pair();
            let reference_signature = entity.reference_signature();
            let (
                inline_body,
                definition_len,
                definition_prefix,
                definition_suffix,
                value_len,
                value_payload,
                record_suffix,
            ) = match entity.body {
                entity_table::EntityBody::Inline(bytes) => (
                    Some(bytes),
                    0,
                    Vec::new(),
                    Vec::new(),
                    0,
                    Vec::new(),
                    Vec::new(),
                ),
                entity_table::EntityBody::Nested {
                    definition_len,
                    prefix,
                    suffix,
                    value_len,
                    value_payload,
                    record_suffix,
                    ..
                } => (
                    None,
                    definition_len,
                    prefix,
                    suffix,
                    value_len,
                    value_payload,
                    record_suffix,
                ),
            };
            let value_fields = value_block::tokenize(&value_payload);
            let value_packets = entity_table::value_packets(&value_payload, &value_fields);
            Some(CatiaEntityRecord {
                id: format!("catia:outer:entity-record#{:010}", entity.pos),
                object_graph: id.clone(),
                object_record: object_record.id.clone(),
                ordinal: u64::try_from(ordinal).expect("bounded entity-table ordinal fits u64"),
                byte_offset: u64::try_from(entity.pos)
                    .expect("bounded entity-table offset fits u64"),
                byte_len: u64::try_from(entity.total_len)
                    .expect("bounded entity-table length fits u64"),
                lead: entity.lead,
                inline_body,
                definition_len,
                definition_prefix,
                definition_schema_selections: Vec::new(),
                entity_id: entity.entity_id,
                definition_suffix,
                value_len,
                value_payload,
                value_fields,
                value_schema_selections: Vec::new(),
                relation_expression: None,
                parameter_value: None,
                range_interval: None,
                constraint_range: None,
                definition_value: None,
                definition_chain_value: None,
                relation_program_instance: None,
                schema_configuration_record: None,
                schema_configuration_row_link: None,
                formula_relation: None,
                value_packets,
                numeric_pair,
                reference_signature: reference_signature.map(|production| {
                    CatiaReferenceSignature {
                        production,
                        first_entity: CatiaEntityReference::default(),
                        second_entity: CatiaEntityReference::default(),
                    }
                }),
                record_suffix,
                suffix_value: None,
                suffix_framing: None,
                suffix_schema_selection: None,
            })
        })
        .collect();
    (
        CatiaObjectGraph {
            id,
            byte_offset: graph.pos as u64,
            byte_len: graph.total_len as u64,
            finjpl_segment,
            outer_container,
            catalog_byte_offset: graph.catalog_pos.map(|pos| pos as u64),
            catalog: None,
            records,
        },
        entities,
    )
}

impl From<&container::OuterContainerDeclaration> for CatiaOuterContainerBinding {
    fn from(declaration: &container::OuterContainerDeclaration) -> Self {
        Self {
            data_offset: declaration.data_offset as u64,
            ordinal: declaration.ordinal,
            class_name: declaration.class_name.clone(),
            base_class: declaration.base_class.clone(),
            stream_name: declaration.stream_name.clone(),
        }
    }
}

#[cfg(test)]
mod test_only;
#[cfg(test)]
mod tests;
