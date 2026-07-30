// SPDX-License-Identifier: Apache-2.0
//! CATIA-native ownership and design records retained outside the neutral model.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
#[cfg(test)]
use std::mem::size_of;

use crate::catalog;
use crate::container;
use crate::entity_table;
#[cfg(test)]
use crate::families::consolidated::records::ConsolidatedEdgeDefinitionData;
use crate::legacy_entity;
use crate::object_graph::{
    self, AliasGroupMembership, AliasLead, HeadToken, ListItem, ObjectPayload, PayloadField,
    PayloadSubtype,
};
use crate::value_block;

/// Current schema version for the CATIA native namespace.
pub const CATIA_NATIVE_VERSION: u32 = 241;
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
/// Native schema version separating configuration schema and entity references.
#[cfg(test)]
pub(crate) const CATIA_CONFIGURATION_SCHEMA_REFERENCE_VERSION: u32 = 232;
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
/// Native schema version retaining complete ordered configuration-row chains.
#[cfg(test)]
pub(crate) const CATIA_CONFIGURATION_ROW_CHAIN_VERSION: u32 = 239;
/// Native schema version retaining terminal-null state on every typed incidence.
#[cfg(test)]
pub(crate) const CATIA_TYPED_INCIDENCE_NULL_VERSION: u32 = 240;
/// Native schema version retaining every exact relation-program reference incidence.
#[cfg(test)]
pub(crate) const CATIA_RELATION_PROGRAM_REFERENCE_INCIDENCE_VERSION: u32 = 241;
#[cfg(test)]
const CATIA_TERMINAL_NULL_REFERENCE_VERSION: u32 = 211;
#[cfg(test)]
const CATIA_DEFINITION_CHAIN_OWNERSHIP_VERSION: u32 = 196;
#[cfg(test)]
const CATIA_TYPED_OWNER_SLOT_VERSION: u32 = 198;
#[cfg(test)]
const CATIA_SUFFIX_FRAMING_VERSION: u32 = 200;
#[cfg(test)]
const CATIA_PARALLEL_REFERENCE_TABLE_VERSION: u32 = 207;
#[cfg(test)]
const CATIA_FORMULA_DEPENDENCY_CANDIDATE_VERSION: u32 = 206;
#[cfg(test)]
const CATIA_OBJECT_GRAPH_SEGMENT_VERSION: u32 = 208;

const CATIA_ARENA_NAMES: &[&str] = &[
    "alias_rows",
    "catalog_entries",
    "catalogs",
    "consolidated_circles",
    "consolidated_class61_records",
    "consolidated_cone_faces",
    "consolidated_cones",
    "consolidated_cylinders",
    "consolidated_embedded_cylinders",
    "consolidated_edge_nodes",
    "consolidated_edge_runs",
    "consolidated_groups",
    "consolidated_line_profiles",
    "consolidated_owner_packets",
    "consolidated_parameter_points",
    "consolidated_pcurves",
    "consolidated_reference_lists",
    "consolidated_revolutions",
    "consolidated_spheres",
    "consolidated_tori",
    "consolidated_vertex_identities",
    "configuration_row_chains",
    "design_objects",
    "entity_records",
    "external_references",
    "finjpl_segments",
    "legacy_entity_runs",
    "object_graph_records",
    "object_graphs",
    "preview_images",
    "value_blocks",
    "value_schema_selections",
    "zero_entity_edge_strides",
    "zero_entity_oriented_use_pairs",
    "zero_entity_ownership_roots",
    "zero_entity_endpoint_pair_candidates",
    "zero_entity_records",
    "zero_entity_support_runs",
    "zero_entity_endpoint_locus_candidates",
    "zero_entity_vertex_incidences",
];

/// Consolidated pcurve framing family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatiaConsolidatedFamily {
    /// A-family frame with a u32 payload length.
    A,
    /// B-family frame with a u8 payload length.
    B,
}

/// Reference dialect used by a consolidated class-`0x62` owner packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatiaOwnerReferenceEncoding {
    /// Strong identities use tagged little-endian `u16` values.
    TaggedU16Strong,
    /// Strong identities use width-coded compact integers.
    WidthCodedStrong,
}

/// Allocation link immediately preceding a consolidated owner packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaOwnerAllocationLink {
    /// Link-record byte offset.
    pub byte_offset: u64,
    /// Complete framed-record byte length.
    pub byte_len: u64,
    /// Width-coded header token.
    pub header_token: u32,
    /// Allocation identity whose successor is the owner's final reference.
    pub target: u32,
}

/// Structurally decoded payload of a class-`0x62` consolidated owner packet.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaOwnerNumericTail {
    /// Five-byte class-specific header.
    pub header: [u8; 5],
    /// Lower coordinate pair of a strictly increasing binary64 box.
    pub lower: [f64; 2],
    /// Upper coordinate pair of a strictly increasing binary64 box.
    pub upper: [f64; 2],
    /// Three strictly increasing binary32 bounds in serialization order.
    pub bounds: [[f32; 2]; 3],
}

/// Structurally decoded payload of a class-`0x62` consolidated owner packet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CatiaOwnerPacketPayload {
    /// Nine alternating strong/weak identities followed by a fixed numeric tail.
    FixedNine {
        /// Reference encoding selected by the packet.
        reference_encoding: CatiaOwnerReferenceEncoding,
        /// Nine persistent identities in serialization order.
        references: [u32; 9],
        /// Structurally decoded 62-byte class-specific numeric tail.
        numeric_tail: CatiaOwnerNumericTail,
    },
    /// Count-selected persistent identities followed by a nonempty tail.
    Counted {
        /// Persistent identities in serialization order.
        references: Vec<u32>,
        /// Complete nonempty class-specific tail.
        #[serde(with = "cadmpeg_ir::bytes")]
        #[schemars(with = "String")]
        tail: Vec<u8>,
    },
}

#[cfg(test)]
impl CatiaOwnerPacketPayload {
    fn final_reference(&self) -> Option<u32> {
        match self {
            Self::FixedNine { references, .. } => references.last().copied(),
            Self::Counted { references, .. } => references.last().copied(),
        }
    }
}

/// Exact class-`0x62` consolidated owner packet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConsolidatedOwnerPacket {
    /// Stable source identity.
    pub id: String,
    /// Record byte offset.
    pub byte_offset: u64,
    /// Width-coded header token.
    pub header_token: u32,
    /// Count-specific reference lane and tail.
    pub payload: CatiaOwnerPacketPayload,
    /// Structurally adjacent allocation link, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allocation_link: Option<CatiaOwnerAllocationLink>,
}

/// One structurally complete consolidated `B:29` cone chart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
    /// Scalar immediately preceding the active angular interval.
    pub pre_angular_range_scalar: f64,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CatiaConsolidatedCylinderPayload {
    /// Complete three-dimensional frame reconstructed from layout `0x52` or `0x5a`.
    Resolved {
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

/// One structurally complete consolidated `B:28` cylinder chart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConsolidatedCylinder {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Payload-layout discriminator (`0x52`, `0x5a`, or `0x62`).
    pub layout: u8,
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

/// One layout-`0x5a` cylinder embedded in a type-3 consolidated group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CatiaConsolidatedParameterPointPayload {
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

/// One complete consolidated `B:18` parameter-space record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConsolidatedParameterPoint {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Complete framed-record length.
    pub byte_len: u64,
    /// Payload-layout discriminator (`0x12`, `0x1a`, or `0x2a`).
    pub layout: u8,
    /// First byte of the two-byte class-specific prefix.
    pub prefix: u8,
    /// Second byte of the two-byte class-specific prefix.
    pub control: u8,
    /// Layout-specific finite scalar lane.
    pub payload: CatiaConsolidatedParameterPointPayload,
}

/// One complete consolidated `B:37` persistent-reference list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
    #[schemars(with = "String")]
    pub tail: Vec<u8>,
}

/// One structurally complete consolidated `B:2a` sphere chart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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

/// Structurally decoded payload of a consolidated class-`0x61` record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CatiaConsolidatedClass61Payload {
    /// Count-selected compact reference lane followed by a class-specific tail.
    Counted {
        /// Compact identities in serialization order.
        references: Vec<u32>,
        /// Complete nonempty tail, including terminal byte `0x03`.
        #[serde(with = "cadmpeg_ir::bytes")]
        #[schemars(with = "String")]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConsolidatedGroup {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Compact group-type code.
    pub group_type: u32,
}

/// One complete consolidated cone-face chart descriptor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConsolidatedConeFace {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Complete framed-record length.
    pub byte_len: u64,
    /// Complete reference-and-control program preceding the scalars.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[schemars(with = "String")]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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

/// One structurally complete width-coded class-`0x5e` edge node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConsolidatedEdgeNode {
    /// Stable native-record identity.
    pub id: String,
    /// Record byte offset.
    pub byte_offset: u64,
    /// Header-token width in bytes.
    pub width: u8,
    /// Independent framing flag.
    pub flag: u8,
    /// Width-coded header token.
    pub header_token: u32,
    /// Allocation-local curve-support reference.
    pub curve_ref: u32,
    /// Global native endpoint identities in edge direction.
    pub vertex_refs: [u32; 2],
    /// Retained vertex-identity records in edge direction.
    pub vertices: [String; 2],
    /// Allocation-local endpoint selectors.
    pub parameter_selectors: [u32; 2],
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConsolidatedAnalyticCircleBinding {
    /// Exact class-`0x18` descriptor frame.
    pub descriptor: CatiaConsolidatedAnalyticCircleDescriptor,
    /// Referenced consolidated circle support.
    pub circle: String,
}

/// Exact class-`0x18` descriptor frame attached to an analytic circle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConsolidatedEdgeUses {
    /// Counted allocation-reference vectors in side order.
    pub references: [[u32; 2]; 2],
    /// Terminal side-use sense bytes in serialized order.
    pub senses: [u8; 2],
}

/// One global endpoint identity retained by consolidated topology edge nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConsolidatedVertexIdentity {
    /// Stable native-record identity assigned in first-incidence order.
    pub id: String,
    /// Global native endpoint identity.
    pub identity: u32,
    /// Incident consolidated edge nodes in source order.
    pub incident_edge_nodes: Vec<String>,
}

/// Exact carrier selected for one side of a consolidated historical edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
    /// Consolidated NURBS carrier with an optional constant normal offset.
    NurbsCarrier {
        /// Carrier record byte offset.
        byte_offset: u64,
        /// Signed normal offset in millimetres.
        offset: f64,
    },
}

/// One complete outer FINJPL segment retained with its framing identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
    #[schemars(with = "String")]
    pub data: Vec<u8>,
}

/// One external CATIA document selected by a storage-property record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
    #[schemars(with = "String")]
    pub data: Vec<u8>,
}

/// One exact outer `01 00 04 00` alias-row core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
}

/// One exact `7C0B` value block adjacent to its source-schema catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
    #[schemars(with = "String")]
    pub payload: Vec<u8>,
    /// Lossless typed fields in payload order.
    #[serde(default)]
    pub fields: Vec<value_block::ValueField>,
    /// Schema selectors in payload order, resolved against the adjacent catalog.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schema_selections: Vec<CatiaValueSchemaSelection>,
}

/// One `0x32` selector from a value block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

/// One exact schema entry selected by a typed entity-value program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaEntitySchemaValue {
    /// Selected source-schema entry.
    pub entry: String,
    /// UTF-8 value stored by the selected entry.
    pub value: String,
}

/// One complete relation-expression value program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaRelationTypeSignature {
    /// Ordered expression-local inputs named inside the signature.
    pub inputs: Vec<CatiaRelationTypeInput>,
    /// Source result type named after the closing parenthesis.
    pub result_type: String,
}

/// One typed input clause in a relation-expression source signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaRelationTypeInput {
    /// Expression-local parameter named before `#In`.
    pub parameter: String,
    /// Source input type named after `#In`.
    pub input_type: String,
}

/// Evaluation state of one complete entity-record suffix value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CatiaEntityEvaluationEncoding {
    /// The evaluation opcode directly precedes its payload.
    Direct,
    /// `E6 00 00 00` precedes the scalar's `E6` opcode.
    ZeroPaddedScalar,
}

/// Payload of one complete entity-record suffix value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CatiaEntitySuffixPayload {
    /// An unset or finite scalar evaluation with exact framing.
    Evaluation {
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CatiaEntitySuffixSelectedValue {
    /// One canonical one-byte atom.
    Atom {
        /// Decoded atom value.
        value: u32,
    },
    /// One direct unset or finite scalar evaluation.
    Evaluation {
        /// Decoded evaluation.
        evaluation: CatiaEntityEvaluation,
    },
    /// One zero-payload `E8` control state.
    ControlE8,
    /// One zero-payload `37` separator.
    Separator37,
    /// One further source-schema selector.
    SchemaSelector {
        /// Stored zero-based source-schema ordinal.
        ordinal: u32,
    },
}

/// Exact trailer framing of one complete entity-record suffix value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CatiaEntitySuffixTrailer {
    /// No trailer bytes follow the payload.
    Empty,
    /// Exact trailer token `81 49`.
    Token8149,
    /// Exact trailer token `81 4A`.
    Token814A,
    /// Exact trailer token `81 52`.
    Token8152,
    /// Exact fixed trailer `FE F6 00{16}`.
    FixedZeroFrame,
}

/// One complete typed value in an entity-record suffix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaEntitySuffixEscapedWord {
    /// Fixed-width little-endian word following the `80` escape.
    pub word: u32,
    /// Exact trailing state.
    pub state: CatiaEntitySuffixEscapedWordState,
}

/// One complete non-value entity-record suffix framing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CatiaEntitySuffixFraming {
    /// One escaped fixed-width word followed by an exact state.
    EscapedWord(CatiaEntitySuffixEscapedWord),
    /// Standalone token `81 49`.
    Token8149,
    /// Standalone fixed frame `FE F6 <payload[16]>`.
    FixedFeF6 {
        /// Exact fixed-width payload.
        #[serde(with = "cadmpeg_ir::bytes")]
        #[schemars(with = "String")]
        payload: Vec<u8>,
    },
    /// One paged compact atom followed by state byte `01`.
    PagedAtomState01 {
        /// Decoded compact-atom value.
        value: u32,
    },
}

/// One suffix selector resolved through its graph's source-schema catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaEntitySuffixSchemaSelection {
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CatiaEntitySuffixSchemaValue {
    /// One canonical compact atom.
    Atom {
        /// Decoded atom value.
        value: u32,
    },
    /// One direct unset or finite scalar evaluation.
    Evaluation {
        /// Decoded evaluation.
        evaluation: CatiaEntityEvaluation,
    },
    /// One zero-payload `E8` control state.
    ControlE8,
    /// One zero-payload `37` separator.
    Separator37,
    /// One nested source-schema selector.
    SchemaSelector {
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaParameterValue {
    /// Stored parameter name.
    pub name: CatiaEntitySchemaValue,
    /// Stored scope, expression, or presentation binding.
    pub binding: CatiaEntitySchemaValue,
    /// Stored evaluation state.
    pub evaluation: CatiaEntityEvaluation,
}

/// Exact framing of one complete constraint-range value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CatiaConstraintRangeFraming {
    /// `CstAttr_Dimension` selected with prefix code `B8`.
    DimensionB8,
    /// `CstAttr_Dimension` selected with prefix code `C1`.
    DimensionC1,
    /// `ComplexCst` selected with prefix code `C9`.
    ComplexC9,
}

/// One complete constraint-range value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConstraintRange {
    /// Exact `Range` role selector.
    pub range: CatiaEntitySchemaValue,
    /// Exact constraint role selector encoded by `framing`.
    pub constraint: CatiaEntitySchemaValue,
    /// Exact role and prefix-code framing.
    pub framing: CatiaConstraintRangeFraming,
    /// Stored evaluation state.
    pub evaluation: CatiaEntityEvaluation,
    /// Exact same-graph payload-reference occurrences selecting this range.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incoming_references: Vec<CatiaConstraintRangeIncomingReference>,
}

/// One exact payload-reference occurrence selecting a constraint range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConstraintRangeIncomingReference {
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

/// One definition-selected entity whose complete value occupies its suffix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaDefinitionChainValue {
    /// Definition repeated by the suffix's fixed-width schema selector.
    pub selector: CatiaEntitySchemaValue,
    /// Second definition carrying the value's role within the selected schema.
    pub role: CatiaEntitySchemaValue,
    /// Stored selected value.
    pub value: CatiaEntitySuffixSchemaValue,
}

/// One complete formula relation stored by an entity and its object payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaFormulaRelation {
    /// Complete relation-expression incidence selected by the second payload reference.
    #[serde(default)]
    pub expression_entity: CatiaEntityReference,
    /// Output parameter incidence selected by the third payload reference.
    #[serde(default)]
    pub output_entity: CatiaEntityReference,
    /// Named parameter records selected by expression-local symbols, in occurrence order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameter_dependencies: Vec<CatiaFormulaParameterDependency>,
}

/// One formula expression symbol and every matching named parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaFormulaParameterDependency {
    /// Exact expression-local symbol occurrence.
    pub symbol: String,
    /// Entity incidences carrying matching named parameter bindings.
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_formula_dependency_candidates"
    )]
    #[schemars(with = "Vec<CatiaEntityReference>")]
    pub candidates: Vec<CatiaEntityReference>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CatiaFormulaDependencyCandidate {
    Reference(CatiaEntityReference),
    LegacyEntity(String),
}

fn deserialize_formula_dependency_candidates<'de, D>(
    deserializer: D,
) -> Result<Vec<CatiaEntityReference>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Vec::<CatiaFormulaDependencyCandidate>::deserialize(deserializer).map(|candidates| {
        candidates
            .into_iter()
            .map(|candidate| match candidate {
                CatiaFormulaDependencyCandidate::Reference(reference) => reference,
                CatiaFormulaDependencyCandidate::LegacyEntity(entity) => CatiaEntityReference {
                    entity: Some(entity),
                    ..CatiaEntityReference::default()
                },
            })
            .collect()
    })
}

/// One exact compound relation-program instance frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaRelationProgramInstance {
    /// Exact object-head and payload production.
    #[serde(default)]
    pub framing: CatiaRelationProgramInstanceFraming,
    /// Entity incidence carried by the frame's program slot.
    #[serde(default)]
    pub program_entity: CatiaEntityReference,
    /// Entity identity stored once as an atom and once as a reference.
    #[serde(default)]
    pub repeated_entity: CatiaEntityReference,
    /// Every reference occurrence in exact payload order, including repeated identities.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference_incidences: Vec<CatiaEntityReference>,
    /// Selected entity when it carries a complete relation-expression program.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation_expression: Option<String>,
    /// Same-graph incidence carried by the `ref(h)` slot of a lead-`12` frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead12_context_entity: Option<CatiaEntityReference>,
    /// Trailing same-graph entity incidence carried only by lead-`54`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead54_trailing_entity: Option<CatiaEntityReference>,
}

/// One stored entity identity and its optional same-graph resolution.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

/// One exact self-defining `Configuration` object production.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConfigurationRecord {
    /// Stored value-schema ordinal selected by the first reference.
    pub schema_ordinal: u32,
    /// Selected schema-catalog entry.
    pub schema_entry: String,
    /// Selected schema-catalog name.
    pub schema_name: String,
    /// Entity selected by the second stored reference.
    pub entity_reference: CatiaEntityReference,
}

/// One exact `configrow` successor-link production.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConfigurationRowLink {
    /// Stored class identity whose catalog name is `configrow`.
    pub class_reference: CatiaEntityReference,
    /// Stored successor identity.
    pub successor: CatiaEntityReference,
}

/// One complete ordered chain formed by exact `configrow` successor links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConfigurationRowChain {
    /// Stable identity derived from the graph and stored class identity.
    pub id: String,
    /// Object graph containing every row link.
    pub object_graph: String,
    /// Stored class identity that selects the root row.
    pub class_reference: CatiaEntityReference,
    /// Row entities in successor order from the selected root.
    pub rows: Vec<CatiaEntityReference>,
    /// First successor identity that does not select another row link.
    pub terminal: CatiaEntityReference,
}

/// Exact framing production for a compound relation-program instance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CatiaRelationProgramInstanceFraming {
    /// Compact `0x12` object head and its 20-token payload.
    #[default]
    Lead12,
    /// Separator-form `0x54` object head and its 18-token payload.
    Lead54,
}

/// Field order used by a repeated-reference schema preamble.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CatiaRepeatedReferenceSchemaOrder {
    /// The binary descriptor precedes the schema ordinal.
    BlobThenSchema,
    /// The schema ordinal precedes the binary descriptor.
    SchemaThenBlob,
}

/// One `7C05` entity-table record paired with a `7C09` object record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
    /// Stored nested `7C06` length.
    pub definition_len: u32,
    /// Exact definition prefix before the `0xEA` identity delimiter.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[schemars(with = "String")]
    pub definition_prefix: Vec<u8>,
    /// Definition selectors resolved against the containing graph's source schema.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub definition_schema_selections: Vec<CatiaDefinitionSchemaSelection>,
    /// Stored identity used by object-record owner and payload references.
    pub entity_id: u32,
    /// Exact definition bytes after the identity.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[schemars(with = "String")]
    pub definition_suffix: Vec<u8>,
    /// Stored nested `7C07` total length.
    pub value_len: u32,
    /// Exact nested `7C07` payload.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[schemars(with = "String")]
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
    /// Exact self-defining `Configuration` object production.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration_record: Option<CatiaConfigurationRecord>,
    /// Exact `configrow` successor-link production.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration_row_link: Option<CatiaConfigurationRowLink>,
    /// Complete formula-to-expression and formula-to-parameter relation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula_relation: Option<CatiaFormulaRelation>,
    /// Exact packets in the value program, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub value_packets: Vec<entity_table::EntityValuePacket>,
    /// Complete numeric tuple when the entire `7C07` payload has that production.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numeric_tuple: Option<entity_table::NumericTuple>,
    /// Complete reference signature when the entire `7C07` payload has that production.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_signature: Option<entity_table::ReferenceSignature>,
    /// Exact bytes after the nested `7C07` frame.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[schemars(with = "String")]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
    #[schemars(with = "Option<String>")]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaDesignClass {
    /// Selected source-schema entry.
    pub entry: String,
    /// UTF-8 class name stored by the entry.
    pub name: String,
}

/// One exact outbound relation occurrence in a grouped design object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaDesignReferenceCell {
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaDesignReferenceRow {
    /// Cells in the order of the table's source fields.
    pub cells: Vec<CatiaDesignReferenceCell>,
    /// Design object containing distinct selected fields whose classes equal every column class.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matching_design_object: Option<String>,
}

/// Equal-cardinality reference lists aligned by list-item ordinal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaDesignParallelReferenceTable {
    /// Source field records forming the table's columns.
    pub columns: Vec<String>,
    /// Exact source field classes aligned with `columns`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub column_classes: Vec<Option<CatiaDesignClass>>,
    /// Row-aligned reference cells.
    pub rows: Vec<CatiaDesignReferenceRow>,
}

/// One serialized design object formed by a shared `7C09` owner identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
                ..
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
                    ListItem::Reference { value, .. } => Some(*value),
                    ListItem::Atom { .. } => None,
                })
                .collect::<Option<Vec<_>>>()?;
            Some((record.id.clone(), design_class(record), references))
        })
        .collect::<Option<Vec<_>>>()?;
    let row_count = columns.first()?.2.len();
    let terminal_null_entity_id = terminal_null_entity_id(record_indices);
    if columns
        .iter()
        .any(|(_, _, references)| references.len() != row_count)
    {
        return None;
    }
    let rows = (0..row_count)
        .map(|row| {
            let cells = columns
                .iter()
                .map(|(_, _, references)| {
                    let target_entity_id = references[row];
                    let target = record_indices
                        .get(&target_entity_id)
                        .and_then(|index| graph.records.get(*index));
                    CatiaDesignReferenceCell {
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
                    columns
                        .iter()
                        .zip(&cells)
                        .all(|((_, source_class, _), cell)| {
                            source_class.is_some()
                                && cell.field.is_some()
                                && cell.field_class.as_ref() == source_class.as_ref()
                                && cell.design_object.as_ref() == Some(member)
                        })
                        && distinct_fields.len() == cells.len()
                });
            CatiaDesignReferenceRow {
                cells,
                matching_design_object,
            }
        })
        .collect();
    Some(CatiaDesignParallelReferenceTable {
        column_classes: columns.iter().map(|(_, class, _)| class.clone()).collect(),
        columns: columns.into_iter().map(|(field, _, _)| field).collect(),
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

#[cfg(test)]
fn valid_entity_record_shape(record: &CatiaEntityRecord) -> bool {
    let Some(definition_body_len) = u64::try_from(record.definition_prefix.len())
        .ok()
        .and_then(|prefix_len| prefix_len.checked_add(5))
        .and_then(|len| {
            u64::try_from(record.definition_suffix.len())
                .ok()
                .and_then(|suffix_len| len.checked_add(suffix_len))
        })
    else {
        return false;
    };
    let Some(value_len) = u64::try_from(record.value_payload.len())
        .ok()
        .and_then(|len| len.checked_add(6))
    else {
        return false;
    };
    let Some(total_len) = 7_u64
        .checked_add(u64::from(record.definition_len))
        .and_then(|len| len.checked_add(u64::from(record.value_len)))
        .and_then(|len| {
            u64::try_from(record.record_suffix.len())
                .ok()
                .and_then(|suffix_len| len.checked_add(suffix_len))
        })
    else {
        return false;
    };
    u64::from(record.definition_len) == definition_body_len + 6
        && u64::from(record.value_len) == value_len
        && record.byte_len == total_len
        && record.value_fields == value_block::tokenize(&record.value_payload)
        && record.value_packets
            == entity_table::value_packets(&record.value_payload, &record.value_fields)
        && record.numeric_tuple == entity_table::parse_numeric_tuple(&record.value_payload)
        && record.reference_signature
            == entity_table::parse_reference_signature(&record.value_payload)
        && record.suffix_value == entity_suffix_value(&record.record_suffix)
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
    let CatiaEntitySuffixPayload::SchemaSelected { selector, value } = &suffix_value?.payload
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
        CatiaEntitySuffixSelectedValue::Evaluation { evaluation } => {
            CatiaEntitySuffixSchemaValue::Evaluation {
                evaluation: evaluation.clone(),
            }
        }
        CatiaEntitySuffixSelectedValue::ControlE8 => CatiaEntitySuffixSchemaValue::ControlE8,
        CatiaEntitySuffixSelectedValue::Separator37 => CatiaEntitySuffixSchemaValue::Separator37,
        CatiaEntitySuffixSelectedValue::SchemaSelector { ordinal } => {
            let selected = usize::try_from(*ordinal)
                .ok()
                .and_then(|ordinal| catalog?.entries.get(ordinal));
            CatiaEntitySuffixSchemaValue::SchemaSelector {
                ordinal: *ordinal,
                entry: selected.map(|entry| entry.id.clone()),
                name: selected.map(|entry| entry.value.clone()),
            }
        }
    };
    Some(CatiaEntitySuffixSchemaSelection {
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
    let result_type = result_type.trim();
    if result_type.is_empty() {
        return None;
    }
    let inputs = if input_clause.trim().is_empty() {
        Vec::new()
    } else {
        input_clause
            .split(',')
            .map(|clause| {
                let (parameter, input_type) = clause.split_once(':')?;
                let parameter = parameter.trim();
                let input_type = input_type.trim().strip_prefix("#In")?.trim();
                (!parameter.is_empty() && !input_type.is_empty()).then(|| CatiaRelationTypeInput {
                    parameter: parameter.to_string(),
                    input_type: input_type.to_string(),
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
        evaluation,
        encoding: CatiaEntityEvaluationEncoding::Direct,
    } = &suffix_value.payload
    else {
        return None;
    };
    let schema_value = |selection: &CatiaEntityValueSchemaSelection| CatiaEntitySchemaValue {
        entry: selection.entry.clone(),
        value: selection.name.clone(),
    };
    Some(CatiaParameterValue {
        name: schema_value(name),
        binding: schema_value(binding),
        evaluation: evaluation.clone(),
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
    if suffix_value.prefix_atoms != [4, 22, 2]
        || suffix_value.prefix_atom_widths != [1, 1, 1]
        || suffix_value.trailer != CatiaEntitySuffixTrailer::Empty
    {
        return None;
    }
    let framing = match (constraint.name.as_str(), suffix_value.prefix_code) {
        ("CstAttr_Dimension", 0xb8) => CatiaConstraintRangeFraming::DimensionB8,
        ("CstAttr_Dimension", 0xc1) => CatiaConstraintRangeFraming::DimensionC1,
        ("ComplexCst", 0xc9) => CatiaConstraintRangeFraming::ComplexC9,
        _ => return None,
    };
    let CatiaEntitySuffixPayload::Evaluation {
        evaluation,
        encoding: CatiaEntityEvaluationEncoding::Direct,
    } = &suffix_value.payload
    else {
        return None;
    };
    Some(CatiaConstraintRange {
        range: CatiaEntitySchemaValue {
            entry: range.entry.clone(),
            value: range.name.clone(),
        },
        constraint: CatiaEntitySchemaValue {
            entry: constraint.entry.clone(),
            value: constraint.name.clone(),
        },
        framing,
        evaluation: evaluation.clone(),
        incoming_references: Vec::new(),
    })
}

fn constraint_range_incoming_references(
    records: &[CatiaObjectRecord],
    graph_id: &str,
    entity_id: u32,
) -> Vec<CatiaConstraintRangeIncomingReference> {
    records
        .iter()
        .filter(|record| record.parent == graph_id)
        .flat_map(|record| {
            record
                .references
                .iter()
                .filter(move |reference| reference.entity_id == entity_id)
                .map(|reference| CatiaConstraintRangeIncomingReference {
                    object_record: record.id.clone(),
                    source_entity: record.entity_id.map(|entity_id| CatiaEntityReference {
                        entity_id,
                        is_null: false,
                        entity: record.entity_record.clone(),
                        class_name: record.class_name.clone(),
                    }),
                    payload_offset: reference.payload_offset,
                    source: reference.source.clone(),
                })
        })
        .collect()
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
    range.incoming_references = constraint_range_incoming_references(records, graph_id, entity_id);
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
        entry: selector.entry.clone()?,
        value: selector.name.clone()?,
    };
    let role = CatiaEntitySchemaValue {
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
        let bits = u64::from_le_bytes(
            suffix
                .get(payload_offset + 5..payload_offset + 13)?
                .try_into()
                .ok()?,
        );
        f64::from_bits(bits).is_finite().then_some(())?;
        (
            CatiaEntitySuffixPayload::Evaluation {
                evaluation: CatiaEntityEvaluation::Scalar { bits },
                encoding: CatiaEntityEvaluationEncoding::ZeroPaddedScalar,
            },
            payload_offset + 13,
        )
    } else if prefix_code == 0x32 {
        let selector = u32::from_le_bytes(
            suffix
                .get(payload_offset..payload_offset + 4)?
                .try_into()
                .ok()?,
        );
        let value_offset = payload_offset + 4;
        let (value, trailer_offset) = match *suffix.get(value_offset)? {
            0xe6 => {
                let bits = u64::from_le_bytes(
                    suffix
                        .get(value_offset + 1..value_offset + 9)?
                        .try_into()
                        .ok()?,
                );
                f64::from_bits(bits).is_finite().then_some(())?;
                (
                    CatiaEntitySuffixSelectedValue::Evaluation {
                        evaluation: CatiaEntityEvaluation::Scalar { bits },
                    },
                    value_offset + 9,
                )
            }
            0xe7 => (
                CatiaEntitySuffixSelectedValue::Evaluation {
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
                    ordinal: u32::from_le_bytes(
                        suffix
                            .get(value_offset + 1..value_offset + 5)?
                            .try_into()
                            .ok()?,
                    ),
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
            CatiaEntitySuffixPayload::SchemaSelected { selector, value },
            trailer_offset,
        )
    } else {
        match *suffix.get(payload_offset)? {
            0xe7 => (
                CatiaEntitySuffixPayload::Evaluation {
                    evaluation: CatiaEntityEvaluation::Unset,
                    encoding: CatiaEntityEvaluationEncoding::Direct,
                },
                payload_offset + 1,
            ),
            0xe8 => (CatiaEntitySuffixPayload::ControlE8, payload_offset + 1),
            0xe9 => (CatiaEntitySuffixPayload::ControlE9, payload_offset + 1),
            0x37 => (CatiaEntitySuffixPayload::Separator37, payload_offset + 1),
            0xe6 => {
                let bits = u64::from_le_bytes(
                    suffix
                        .get(payload_offset + 1..payload_offset + 9)?
                        .try_into()
                        .ok()?,
                );
                f64::from_bits(bits).is_finite().then_some(())?;
                (
                    CatiaEntitySuffixPayload::Evaluation {
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

fn entity_suffix_framing(suffix: &[u8]) -> Option<CatiaEntitySuffixFraming> {
    match suffix {
        [0x80, word @ .., state] => {
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
                    word: u32::from_le_bytes(word.try_into().ok()?),
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

#[cfg(test)]
mod entity_suffix_framing_tests {
    use super::{
        entity_suffix_framing, CatiaEntitySuffixEscapedWord, CatiaEntitySuffixEscapedWordState,
        CatiaEntitySuffixFraming,
    };

    #[test]
    fn decodes_each_escaped_word_state() {
        for (code, state) in [
            (0x00, CatiaEntitySuffixEscapedWordState::State00),
            (0x01, CatiaEntitySuffixEscapedWordState::State01),
            (0x03, CatiaEntitySuffixEscapedWordState::State03),
            (0x04, CatiaEntitySuffixEscapedWordState::State04),
            (0x09, CatiaEntitySuffixEscapedWordState::State09),
        ] {
            assert_eq!(
                entity_suffix_framing(&[0x80, 0x78, 0x56, 0x34, 0x12, code]),
                Some(CatiaEntitySuffixFraming::EscapedWord(
                    CatiaEntitySuffixEscapedWord {
                        word: 0x1234_5678,
                        state,
                    }
                ))
            );
        }
    }

    #[test]
    fn decodes_each_complete_non_value_framing() {
        assert_eq!(
            entity_suffix_framing(&[0x81, 0x49]),
            Some(CatiaEntitySuffixFraming::Token8149)
        );
        assert_eq!(
            entity_suffix_framing(&[
                0xfe, 0xf6, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b,
                0x0c, 0x0d, 0x0e, 0x0f,
            ]),
            Some(CatiaEntitySuffixFraming::FixedFeF6 {
                payload: (0x00..=0x0f).collect(),
            })
        );
        assert_eq!(
            entity_suffix_framing(&[0xd2, 0x2d, 0x01]),
            Some(CatiaEntitySuffixFraming::PagedAtomState01 { value: 302 })
        );
    }

    #[test]
    fn rejects_other_framing() {
        assert_eq!(entity_suffix_framing(&[0x80, 0x78, 0x56, 0x34, 0x12]), None);
        assert_eq!(
            entity_suffix_framing(&[0x81, 0x78, 0x56, 0x34, 0x12, 0x00]),
            None
        );
        assert_eq!(
            entity_suffix_framing(&[0x80, 0x78, 0x56, 0x34, 0x12, 0x02]),
            None
        );
        assert_eq!(
            entity_suffix_framing(&[0x80, 0x78, 0x56, 0x34, 0x12, 0x00, 0x00]),
            None
        );
        assert_eq!(entity_suffix_framing(&[0xfe, 0xf6, 0x00]), None);
        assert_eq!(entity_suffix_framing(&[0xd2, 0x2d, 0x00]), None);
    }
}

fn relation_program_instance(
    entity_id: u32,
    object: &CatiaObjectRecord,
    entities: &HashMap<(String, u32), String>,
    entity_classes: &CatiaEntityClassByGraphIdentityIndex,
    terminal_nulls: &CatiaTerminalNullByGraphIndex,
    relation_expressions: &HashMap<(String, u32), String>,
) -> Option<CatiaRelationProgramInstance> {
    if object.entity_id != Some(entity_id)
        || object.owner_entity_id().is_none()
        || object.class_ref.is_none()
    {
        return None;
    }
    let (
        framing,
        program_entity_id,
        repeated_reference_entity_id,
        lead12_context_entity,
        lead54_trailing_entity,
    ) = if object.lead == 0x12 && object.storage_ref.is_none() {
        let (program_entity_id, repeated_reference_entity_id, context_entity_id) =
            relation_program_instance_lead_12(entity_id, &object.payload.fields)?;
        (
            CatiaRelationProgramInstanceFraming::Lead12,
            program_entity_id,
            repeated_reference_entity_id,
            Some(entity_reference(
                &object.parent,
                context_entity_id,
                entities,
                entity_classes,
                terminal_nulls,
            )),
            None,
        )
    } else if object.lead == 0x54 && object.storage_ref.is_some() {
        let (program_entity_id, repeated_reference_entity_id, trailing_entity_id) =
            relation_program_instance_lead_54(entity_id, &object.payload.fields)?;
        (
            CatiaRelationProgramInstanceFraming::Lead54,
            program_entity_id,
            repeated_reference_entity_id,
            None,
            Some(entity_reference(
                &object.parent,
                trailing_entity_id,
                entities,
                entity_classes,
                terminal_nulls,
            )),
        )
    } else {
        return None;
    };
    let program_key = (object.parent.clone(), program_entity_id);
    let reference_incidences = object
        .payload
        .fields
        .iter()
        .filter_map(|field| match field {
            PayloadField::Reference { value, .. } => Some(entity_reference(
                &object.parent,
                *value,
                entities,
                entity_classes,
                terminal_nulls,
            )),
            _ => None,
        })
        .collect();
    Some(CatiaRelationProgramInstance {
        framing,
        program_entity: entity_reference(
            &object.parent,
            program_entity_id,
            entities,
            entity_classes,
            terminal_nulls,
        ),
        repeated_entity: entity_reference(
            &object.parent,
            repeated_reference_entity_id,
            entities,
            entity_classes,
            terminal_nulls,
        ),
        reference_incidences,
        relation_expression: relation_expressions.get(&program_key).cloned(),
        lead12_context_entity,
        lead54_trailing_entity,
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

fn configuration_record(
    entity_id: u32,
    object: &CatiaObjectRecord,
    value_schema_selections: &[CatiaEntityValueSchemaSelection],
    entities: &HashMap<(String, u32), String>,
    entity_classes: &CatiaEntityClassByGraphIdentityIndex,
    terminal_nulls: &CatiaTerminalNullByGraphIndex,
) -> Option<CatiaConfigurationRecord> {
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
        ..
    }, PayloadField::Atom { value: 2, .. }, PayloadField::Reference {
        value: referenced_entity_id,
        ..
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
    Some(CatiaConfigurationRecord {
        schema_ordinal: *schema_ordinal,
        schema_entry: selection.entry.clone(),
        schema_name: selection.name.clone(),
        entity_reference: entity_reference(
            &object.parent,
            *referenced_entity_id,
            entities,
            entity_classes,
            terminal_nulls,
        ),
    })
}

fn configuration_row_link(
    entity_id: u32,
    object: &CatiaObjectRecord,
    entities: &HashMap<(String, u32), String>,
    entity_classes: &CatiaEntityClassByGraphIdentityIndex,
    terminal_nulls: &CatiaTerminalNullByGraphIndex,
) -> Option<CatiaConfigurationRowLink> {
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
        ..
    }, PayloadField::Terminator] = object.payload.fields.as_slice()
    else {
        return None;
    };
    Some(CatiaConfigurationRowLink {
        class_reference: entity_reference(
            &object.parent,
            class_entity_id,
            entities,
            entity_classes,
            terminal_nulls,
        ),
        successor: entity_reference(
            &object.parent,
            *successor_entity_id,
            entities,
            entity_classes,
            terminal_nulls,
        ),
    })
}

fn derive_configuration_row_chains(
    records: &[CatiaEntityRecord],
    entities: &HashMap<(String, u32), String>,
    entity_classes: &CatiaEntityClassByGraphIdentityIndex,
    terminal_nulls: &CatiaTerminalNullByGraphIndex,
) -> Vec<CatiaConfigurationRowChain> {
    let row_ids = records
        .iter()
        .filter(|entity| entity.configuration_row_link.is_some())
        .map(|entity| (entity.object_graph.as_str(), entity.entity_id))
        .collect::<HashSet<_>>();
    let mut groups = HashMap::<(&str, u32), Vec<(u32, u32)>>::new();
    for entity in records {
        let Some(link) = &entity.configuration_row_link else {
            continue;
        };
        groups
            .entry((entity.object_graph.as_str(), link.class_reference.entity_id))
            .or_default()
            .push((entity.entity_id, link.successor.entity_id));
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
            while let Some(successor) = successors.get(&current).copied() {
                if !visited.insert(current) {
                    return None;
                }
                row_ids_in_order.push(current);
                current = successor;
            }
            if visited.len() != links.len() || row_ids.contains(&(graph, current)) {
                return None;
            }
            Some(CatiaConfigurationRowChain {
                id: format!("{graph}:configuration-row-chain#{root}"),
                object_graph: graph.to_string(),
                class_reference: entity_reference(
                    graph,
                    root,
                    entities,
                    entity_classes,
                    terminal_nulls,
                ),
                rows: row_ids_in_order
                    .into_iter()
                    .map(|entity_id| {
                        entity_reference(graph, entity_id, entities, entity_classes, terminal_nulls)
                    })
                    .collect(),
                terminal: entity_reference(
                    graph,
                    current,
                    entities,
                    entity_classes,
                    terminal_nulls,
                ),
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
        ..
    }, PayloadField::Reference {
        value: parameter_entity_id,
        ..
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
    let parameter_dependencies = relation_symbols(source)
        .into_iter()
        .map(|symbol| {
            let candidates = parameter_bindings
                .get(&object.parent)
                .and_then(|bindings| bindings.get(&symbol))
                .cloned()
                .unwrap_or_default();
            CatiaFormulaParameterDependency { symbol, candidates }
        })
        .collect();
    Some(CatiaFormulaRelation {
        expression_entity: entity_reference(
            &object.parent,
            *expression_entity_id,
            entity_references.entities,
            entity_references.classes,
            entity_references.terminal_nulls,
        ),
        output_entity: CatiaEntityReference {
            is_null: parameter_reference.is_null,
            ..entity_reference(
                &object.parent,
                *parameter_entity_id,
                entity_references.entities,
                entity_references.classes,
                entity_references.terminal_nulls,
            )
        },
        parameter_dependencies,
    })
}

type CatiaRelationExpressionIndex = HashMap<String, String>;
type CatiaRelationExpressionEntityIndex = HashMap<(String, u32), String>;
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
        .filter(|entity| entity.relation_expression.is_some())
        .map(|entity| {
            (
                (entity.object_graph.clone(), entity.entity_id),
                entity.id.clone(),
            )
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

fn relation_symbols(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut symbols = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
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
            symbols.push(source[start..bare_end].to_string());
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
        symbols.push(source[start..at].to_string());
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
    #[schemars(with = "String")]
    pub data: Vec<u8>,
    /// Complete inclusive-length identifier packets in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub identifiers: Vec<CatiaLegacySchemaIdentifier>,
}

/// Production that closes a compact legacy schema program.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatiaLegacySchemaProgramBoundary {
    /// Fixed vendor footer preceded by the terminal `FE`.
    #[default]
    VendorFooter,
    /// Validated outer stream directory preceded by the terminal `FE`.
    StreamDirectory,
}

/// One complete inclusive-length identifier packet in a compact schema program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaLegacySchemaIdentifier {
    /// Offset of the inclusive-length byte.
    pub byte_offset: u64,
    /// Stored identifier.
    pub value: String,
}

/// Framing production used by a legacy schema text field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatiaLegacyRoleSelectorEncoding {
    /// `80` followed by a nonzero little-endian `u32`.
    FixedU32,
    /// Page byte `D1..E4` followed by one low byte.
    Paged,
}

/// Stored representation of one legacy schema role name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaLegacyRelationParameter {
    /// Expression-local parameter.
    pub parameter: String,
    /// Source value type.
    pub value_type: String,
}

/// One complete legacy expression and type-signature pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaLegacyTypeDescriptor {
    /// Offset of the fixed descriptor prefix.
    pub byte_offset: u64,
    /// Stored containing identity.
    pub entity_id: u32,
    /// Stored literal name or unresolved selector.
    pub value: CatiaLegacyTypeValue,
}

/// Evaluation stored by a complete legacy scalar packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatiaLegacyScalarEncoding {
    /// `FE 84 88 82 FE`.
    Named84,
    /// `FE 85 88 82 FE`.
    Standalone85,
}

/// One complete typed scalar packet in a legacy identity interval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatiaLegacyIntegerEncoding {
    /// One byte stores values zero through 126 as `value + 0x81`.
    Inline,
    /// `80` introduces one signed little-endian 32-bit value.
    WideI32,
}

/// One complete legacy signed-integer packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaZeroEntityEdgeStride {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// One-based global record ordinal in the zero-entity stream.
    pub record_ordinal: u32,
    /// Five allocation values following the fixed tagged-one prefix.
    pub allocations: [u32; 5],
}

/// One positional zero-entity `0638` oriented use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaZeroEntityEndpointPairEndpoint {
    /// Derived endpoint-pair candidate.
    pub endpoint_pair: String,
    /// Zero-based endpoint index in that candidate's oriented endpoint pair.
    pub endpoint_index: u8,
}

/// One geometric endpoint-locus candidate established by a complete endpoint clique.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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

/// CATIA-native records retained outside the format-neutral model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaNative {
    /// Schema version this namespace was written under.
    pub version: u32,
    /// Exact outer alias-row cores in source order.
    #[serde(default)]
    pub alias_rows: Vec<CatiaAliasRow>,
    /// Framed source-schema name catalogs.
    #[serde(default)]
    pub catalogs: Vec<CatiaCatalog>,
    /// Exact consolidated arc-length circle supports.
    #[serde(default)]
    pub consolidated_circles: Vec<CatiaConsolidatedCircle>,
    /// Complete consolidated class-`0x61` records.
    #[serde(default)]
    pub consolidated_class61_records: Vec<CatiaConsolidatedClass61Record>,
    /// Complete consolidated cone-face chart descriptors.
    #[serde(default)]
    pub consolidated_cone_faces: Vec<CatiaConsolidatedConeFace>,
    /// Exact consolidated cone charts.
    #[serde(default)]
    pub consolidated_cones: Vec<CatiaConsolidatedCone>,
    /// Exact consolidated cylinder charts.
    #[serde(default)]
    pub consolidated_cylinders: Vec<CatiaConsolidatedCylinder>,
    /// Exact cylinder charts embedded in type-3 consolidated groups.
    #[serde(default)]
    pub consolidated_embedded_cylinders: Vec<CatiaConsolidatedEmbeddedCylinder>,
    /// Structurally complete consolidated edge nodes.
    #[serde(default)]
    pub consolidated_edge_nodes: Vec<CatiaConsolidatedEdgeNode>,
    /// Complete consolidated historical edge runs.
    #[serde(default)]
    pub consolidated_edge_runs: Vec<CatiaConsolidatedEdgeRun>,
    /// Typed consolidated class-`0x60` group openers.
    #[serde(default)]
    pub consolidated_groups: Vec<CatiaConsolidatedGroup>,
    /// Exact consolidated B-family metric line profiles.
    #[serde(default)]
    pub consolidated_line_profiles: Vec<CatiaConsolidatedLineProfile>,
    /// Exact consolidated owner packets and their allocation links.
    #[serde(default)]
    pub consolidated_owner_packets: Vec<CatiaConsolidatedOwnerPacket>,
    /// Exact consolidated parameter-space records.
    #[serde(default)]
    pub consolidated_parameter_points: Vec<CatiaConsolidatedParameterPoint>,
    /// Consolidated pcurve jets retained before support resolution.
    #[serde(default)]
    pub consolidated_pcurves: Vec<CatiaConsolidatedPcurve>,
    /// Exact consolidated persistent-reference lists.
    #[serde(default)]
    pub consolidated_reference_lists: Vec<CatiaConsolidatedReferenceList>,
    /// Consolidated revolution carriers retained before profile resolution.
    #[serde(default)]
    pub consolidated_revolutions: Vec<CatiaConsolidatedRevolution>,
    /// Exact consolidated sphere charts.
    #[serde(default)]
    pub consolidated_spheres: Vec<CatiaConsolidatedSphere>,
    /// Exact consolidated torus charts.
    #[serde(default)]
    pub consolidated_tori: Vec<CatiaConsolidatedTorus>,
    /// Global endpoint identities and their consolidated edge incidence.
    #[serde(default)]
    pub consolidated_vertex_identities: Vec<CatiaConsolidatedVertexIdentity>,
    /// Complete configuration-row successor chains.
    #[serde(default)]
    pub configuration_row_chains: Vec<CatiaConfigurationRowChain>,
    /// Design objects grouped by their serialized owner entity identity.
    #[serde(default)]
    pub design_objects: Vec<CatiaDesignObject>,
    /// Exact `7C05` entity-table records paired with object records.
    #[serde(default)]
    pub entity_records: Vec<CatiaEntityRecord>,
    /// External CATIA document references in source order.
    #[serde(default)]
    pub external_references: Vec<CatiaExternalReference>,
    /// Complete bounded outer FINJPL segments.
    #[serde(default)]
    pub finjpl_segments: Vec<CatiaFinjplSegment>,
    /// Monotone entity identities in pre-`7C05` design streams.
    #[serde(default)]
    pub legacy_entity_runs: Vec<CatiaLegacyEntityRun>,
    /// Outer ownership graphs.
    #[serde(default)]
    pub object_graphs: Vec<CatiaObjectGraph>,
    /// Exact JPEG previews extracted from summary-information records.
    #[serde(default)]
    pub preview_images: Vec<CatiaPreviewImage>,
    /// Framed value blocks adjacent to source-schema catalogs.
    #[serde(default)]
    pub value_blocks: Vec<CatiaValueBlock>,
    /// Zero-entity edge-stride allocation tuples.
    #[serde(default)]
    pub zero_entity_edge_strides: Vec<CatiaZeroEntityEdgeStride>,
    /// Zero-entity side-pair headers and positional oriented uses.
    #[serde(default)]
    pub zero_entity_oriented_use_pairs: Vec<CatiaZeroEntityOrientedUsePair>,
    /// Complete zero-entity face-roster, shell, and body roots.
    #[serde(default)]
    pub zero_entity_ownership_roots: Vec<CatiaZeroEntityOwnershipRoot>,
    /// Zero-entity endpoint pairs established by radial support occurrences.
    #[serde(default)]
    pub zero_entity_endpoint_pair_candidates: Vec<CatiaZeroEntityEndpointPairCandidate>,
    /// Complete zero-entity framed-record identity namespace.
    #[serde(default)]
    pub zero_entity_records: Vec<CatiaZeroEntityRecord>,
    /// Zero-entity surface carriers and their face-local support tapes.
    #[serde(default)]
    pub zero_entity_support_runs: Vec<CatiaZeroEntitySupportRun>,
    /// Geometric endpoint loci established by complete endpoint-pair endpoint cliques.
    #[serde(default)]
    pub zero_entity_endpoint_locus_candidates: Vec<CatiaZeroEntityEndpointLocusCandidate>,
    /// Zero-entity counted vertex-incidence records.
    #[serde(default)]
    pub zero_entity_vertex_incidences: Vec<CatiaZeroEntityVertexIncidence>,
}

impl Default for CatiaNative {
    fn default() -> Self {
        Self {
            version: CATIA_NATIVE_VERSION,
            alias_rows: Vec::new(),
            catalogs: Vec::new(),
            consolidated_circles: Vec::new(),
            consolidated_class61_records: Vec::new(),
            consolidated_cone_faces: Vec::new(),
            consolidated_cones: Vec::new(),
            consolidated_cylinders: Vec::new(),
            consolidated_embedded_cylinders: Vec::new(),
            consolidated_edge_nodes: Vec::new(),
            consolidated_edge_runs: Vec::new(),
            consolidated_groups: Vec::new(),
            consolidated_line_profiles: Vec::new(),
            consolidated_owner_packets: Vec::new(),
            consolidated_parameter_points: Vec::new(),
            consolidated_pcurves: Vec::new(),
            consolidated_reference_lists: Vec::new(),
            consolidated_revolutions: Vec::new(),
            consolidated_spheres: Vec::new(),
            consolidated_tori: Vec::new(),
            consolidated_vertex_identities: Vec::new(),
            configuration_row_chains: Vec::new(),
            design_objects: Vec::new(),
            entity_records: Vec::new(),
            external_references: Vec::new(),
            finjpl_segments: Vec::new(),
            legacy_entity_runs: Vec::new(),
            object_graphs: Vec::new(),
            preview_images: Vec::new(),
            value_blocks: Vec::new(),
            zero_entity_edge_strides: Vec::new(),
            zero_entity_oriented_use_pairs: Vec::new(),
            zero_entity_ownership_roots: Vec::new(),
            zero_entity_endpoint_pair_candidates: Vec::new(),
            zero_entity_records: Vec::new(),
            zero_entity_support_runs: Vec::new(),
            zero_entity_endpoint_locus_candidates: Vec::new(),
            zero_entity_vertex_incidences: Vec::new(),
        }
    }
}

fn consolidated_circles(bytes: &[u8]) -> Vec<CatiaConsolidatedCircle> {
    crate::families::b2::records::b2_circles(bytes)
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

#[cfg(test)]
fn legacy_schema_identifiers(
    program: &CatiaLegacySchemaProgram,
) -> Option<Vec<CatiaLegacySchemaIdentifier>> {
    let program_offset = usize::try_from(program.byte_offset).ok()?;
    Some(
        legacy_entity::parse_schema_identifiers(&program.data, program_offset)
            .into_iter()
            .map(|identifier| CatiaLegacySchemaIdentifier {
                byte_offset: identifier.offset as u64,
                value: identifier.value,
            })
            .collect(),
    )
}

#[cfg(test)]
fn legacy_value_name(
    roles: &[CatiaLegacyRoleSelector],
    fields: &[CatiaLegacyTextField],
    entity_id: u32,
    value_offset: u64,
) -> Option<(u64, String)> {
    let mut literal_names = fields.iter().filter(|field| {
        field.entity_id == entity_id
            && field
                .role
                .as_ref()
                .is_some_and(|role| role.name.literal() == Some("name"))
    });
    if let Some(name) = literal_names.next() {
        if literal_names.next().is_none() {
            return Some((name.byte_offset, name.value.clone()));
        }
        return None;
    }

    let name = legacy_evaluated_value_name(roles, fields, entity_id, value_offset)?;
    Some((name.byte_offset, name.value.clone()))
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

#[cfg(test)]
fn legacy_schema_boundary_closes_text(
    run: &CatiaLegacyEntityRun,
    field: &CatiaLegacySchemaField,
    role: &CatiaLegacyRoleSelector,
) -> bool {
    run.text_fields.iter().any(|text| {
        text.byte_offset == field.byte_offset
            && text.entity_id == field.entity_id
            && text.encoding == CatiaLegacyTextEncoding::U8InclusiveLengthE3RoleTail
            && text.role.as_ref() == Some(role)
            && text
                .value
                .len()
                .checked_add(1)
                .and_then(|length| u8::try_from(length).ok())
                .is_some_and(|length| {
                    field.payload.first() == Some(&length)
                        && field.payload.get(1..) == Some(text.value.as_bytes())
                })
    })
}

#[cfg(test)]
fn valid_legacy_relation(run: &CatiaLegacyEntityRun, relation: &CatiaLegacyRelation) -> bool {
    let Some(parsed) = legacy_entity::parse_relation_signature(&relation.type_signature) else {
        return false;
    };
    let Some(expression_field) = run.text_fields.iter().find(|field| {
        field.entity_id == relation.entity_id
            && field.byte_offset == relation.expression_offset
            && field.value == relation.expression
    }) else {
        return false;
    };
    let Some(signature_field) = run.text_fields.iter().find(|field| {
        field.entity_id == relation.entity_id
            && field.byte_offset == relation.signature_offset
            && field.value == relation.type_signature
    }) else {
        return false;
    };
    let parameter_entity_id = expression_field
        .role
        .as_ref()
        .zip(signature_field.role.as_ref())
        .and_then(|(owner, parameter)| {
            (owner.name.literal() == Some("body")
                && owner.selector == relation.entity_id
                && parameter.name.literal() == Some("param")
                && run
                    .identities
                    .iter()
                    .any(|identity| identity.entity_id == parameter.selector))
            .then_some(parameter.selector)
        });
    let body_selector = expression_field
        .role
        .as_ref()
        .filter(|role| role.name.literal() == Some("body"))
        .map(|role| role.selector);
    let parameter_selector = signature_field
        .role
        .as_ref()
        .filter(|role| role.name.literal() == Some("param"))
        .map(|role| role.selector);
    valid_legacy_relation_field_pair(run, expression_field, signature_field)
        && relation.body_selector == body_selector
        && relation.parameter_selector == parameter_selector
        && relation.parameter_entity_id == parameter_entity_id
        && (relation.result_type == "VoidType") == relation.output.is_some()
        && parsed.result_type == relation.result_type
        && parsed.inputs.len() == relation.inputs.len()
        && parsed
            .inputs
            .iter()
            .zip(&relation.inputs)
            .all(|(parsed, stored)| {
                parsed.parameter == stored.parameter && parsed.value_type == stored.value_type
            })
        && parsed
            .output
            .as_ref()
            .map(|output| (output.parameter.as_str(), output.value_type.as_str()))
            == relation
                .output
                .as_ref()
                .map(|output| (output.parameter.as_str(), output.value_type.as_str()))
}

#[cfg(test)]
fn valid_legacy_relation_field_pair(
    run: &CatiaLegacyEntityRun,
    expression: &CatiaLegacyTextField,
    signature: &CatiaLegacyTextField,
) -> bool {
    let fields = run
        .text_fields
        .iter()
        .filter(|field| field.entity_id == expression.entity_id)
        .collect::<Vec<_>>();
    let mut body_fields = fields.iter().copied().filter(|field| {
        field
            .role
            .as_ref()
            .is_some_and(|role| role.name.literal() == Some("body"))
    });
    let body = body_fields.next();
    let unique_body = body_fields.next().is_none();
    let mut parameter_fields = fields.iter().copied().filter(|field| {
        field
            .role
            .as_ref()
            .is_some_and(|role| role.name.literal() == Some("param"))
    });
    let parameter = parameter_fields.next();
    let unique_parameter = parameter_fields.next().is_none();
    let role_bound = unique_body
        && unique_parameter
        && body == Some(expression)
        && parameter == Some(signature)
        && expression.byte_offset < signature.byte_offset;
    let selected_role_bound = matches!(
        fields.as_slice(),
        [prelude, selected_expression, selected_signature]
            if prelude.value.is_empty()
                && prelude.role.as_ref().is_none_or(|role| {
                    matches!(&role.name, CatiaLegacyRoleName::Selector(_))
                })
                && prelude.encoding == CatiaLegacyTextEncoding::U8InclusiveLengthE3RoleTail
                && selected_expression.encoding
                    == CatiaLegacyTextEncoding::U8InclusiveLengthE3RoleTail
                && selected_signature.encoding
                    == CatiaLegacyTextEncoding::U8InclusiveLengthE3RoleTail
                && selected_expression.role.as_ref().is_some_and(|role| {
                    matches!(&role.name, CatiaLegacyRoleName::Selector(_))
                })
                && selected_signature.role.as_ref().is_some_and(|role| {
                    matches!(&role.name, CatiaLegacyRoleName::Selector(_))
                })
                && *selected_expression == expression
                && *selected_signature == signature
    );
    let complete_pair = matches!(fields.as_slice(), [first, second] if *first == expression && *second == signature);
    role_bound || selected_role_bound || complete_pair
}

#[cfg(test)]
fn validate_legacy_entity_runs(
    runs: &[CatiaLegacyEntityRun],
    require_field_codes: bool,
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let mut previous_end = None;
    for (index, run) in runs.iter().enumerate() {
        let run_end = run.byte_offset.checked_add(run.byte_len);
        let valid = run.id == format!("catia:legacy:entity-run#{index:08}")
            && run_end == Some(run.catalog_offset)
            && run.schema_program.as_ref().is_none_or(|program| {
                !program.data.is_empty()
                    && program.data.last() == Some(&0xfe)
                    && run.catalog_offset.checked_add(
                        u64::try_from(legacy_entity::SCHEMA_PROGRAM_OFFSET_FROM_CATALOG)
                            .expect("schema-program prefix length fits u64"),
                    ) == Some(program.byte_offset)
                    && u64::try_from(program.data.len())
                        .ok()
                        .and_then(|len| program.byte_offset.checked_add(len))
                        == Some(program.boundary_byte_offset)
                    && legacy_schema_identifiers(program)
                        .is_some_and(|identifiers| program.identifiers == identifiers)
            })
            && previous_end.is_none_or(|end| end <= run.byte_offset)
            && run.identities.first().is_some_and(|identity| {
                identity.byte_offset == run.byte_offset && identity.entity_id == 1
            })
            && run.identities.windows(2).all(|pair| {
                pair[0]
                    .byte_offset
                    .checked_add(6)
                    .is_some_and(|end| end <= pair[1].byte_offset)
                    && pair[0].entity_id < pair[1].entity_id
            })
            && run.identities.iter().all(|identity| {
                matches!(identity.lead, 0x81 | 0x82 | 0xe5 | 0xfd)
                    && identity
                        .byte_offset
                        .checked_add(6)
                        .is_some_and(|end| end <= run.catalog_offset)
            })
            && run
                .role_selectors
                .windows(2)
                .all(|pair| pair[0].byte_offset < pair[1].byte_offset)
            && run.role_selectors.iter().all(|role| {
                role.byte_offset >= run.byte_offset
                    && role.byte_offset < run.catalog_offset
                    && role.selector != 0
                    && match &role.name {
                        CatiaLegacyRoleName::Literal(name) => valid_legacy_identifier(name),
                        CatiaLegacyRoleName::Selector(selector) => *selector != 0,
                    }
                    && run
                        .identities
                        .iter()
                        .rfind(|identity| identity.byte_offset < role.byte_offset)
                        .is_some_and(|identity| {
                            let interval_end = run
                                .identities
                                .iter()
                                .find(|next| next.byte_offset > identity.byte_offset)
                                .map_or(run.catalog_offset, |next| next.byte_offset);
                            identity.entity_id == role.entity_id
                                && role.end_offset().is_none_or(|end| {
                                    end <= interval_end
                                        && role.field_code.is_none_or(|_| {
                                            end.checked_add(4)
                                                .is_some_and(|field_end| field_end <= interval_end)
                                        })
                                })
                        })
            })
            && run
                .text_fields
                .windows(2)
                .all(|pair| pair[0].byte_offset < pair[1].byte_offset)
            && run.text_fields.iter().all(|field| {
                (!field.value.is_empty()
                    || field.encoding == CatiaLegacyTextEncoding::U8InclusiveLengthE3RoleTail)
                    && field.value.chars().all(|character| {
                        !character.is_control() || matches!(character, '\t' | '\n' | '\r')
                    })
                    && field.byte_offset >= run.byte_offset
                    && field.byte_offset < run.catalog_offset
                    && run
                        .identities
                        .iter()
                        .rfind(|identity| identity.byte_offset < field.byte_offset)
                        .is_some_and(|identity| identity.entity_id == field.entity_id)
                    && field.role.as_ref().is_none_or(|role| {
                        role.byte_offset >= run.byte_offset
                            && role.byte_offset < field.byte_offset
                            && role.entity_id == field.entity_id
                            && role.selector != 0
                            && match &role.name {
                                CatiaLegacyRoleName::Literal(name) => valid_legacy_identifier(name),
                                CatiaLegacyRoleName::Selector(selector) => *selector != 0,
                            }
                            && run.role_selectors.contains(role)
                            && role.end_offset().is_none_or(|end| end == field.byte_offset)
                            && (!require_field_codes || role.field_code == Some(0x1200))
                            && run
                                .identities
                                .iter()
                                .rfind(|identity| identity.byte_offset < role.byte_offset)
                                .is_some_and(|identity| identity.entity_id == field.entity_id)
                    })
            })
            && run
                .schema_fields
                .windows(2)
                .all(|pair| pair[0].byte_offset < pair[1].byte_offset)
            && run.schema_fields.iter().all(|field| {
                field.byte_offset >= run.byte_offset
                    && field.byte_offset < run.catalog_offset
                    && field.boundary_role_byte_offset > field.byte_offset
                    && field.byte_offset.checked_add(4).and_then(|payload_offset| {
                        payload_offset.checked_add(u64::try_from(field.payload.len()).ok()?)
                    }) == Some(field.boundary_role_byte_offset)
                    && run.role_selectors.windows(2).any(|roles| {
                        roles[0].byte_offset == field.role_byte_offset
                            && roles[0].entity_id == field.entity_id
                            && roles[0].end_offset() == Some(field.byte_offset)
                            && (!require_field_codes
                                || roles[0].field_code == Some(field.field_code))
                            && roles[1].byte_offset == field.boundary_role_byte_offset
                            && roles[1].entity_id == field.entity_id
                            && (!require_field_codes
                                || roles[1].field_code.is_some()
                                || legacy_schema_boundary_closes_text(run, field, &roles[0]))
                    })
            })
            && run
                .relations
                .iter()
                .all(|relation| valid_legacy_relation(run, relation))
            && run
                .synchronous_states
                .windows(2)
                .all(|pair| pair[0].role_byte_offset < pair[1].role_byte_offset)
            && run.synchronous_states.iter().all(|state| {
                state.role_byte_offset >= run.byte_offset
                    && state.role_byte_offset < run.catalog_offset
                    && run
                        .identities
                        .iter()
                        .rfind(|identity| identity.byte_offset < state.role_byte_offset)
                        .is_some_and(|identity| identity.entity_id == state.entity_id)
                    && run
                        .role_selectors
                        .iter()
                        .filter(|role| {
                            role.byte_offset == state.role_byte_offset
                                && role.entity_id == state.entity_id
                                && (role.name.literal() == Some("synchrone")
                                    || (matches!(&role.name, CatiaLegacyRoleName::Selector(_))
                                        && role
                                            .end_offset()
                                            .and_then(|end| end.checked_add(5))
                                            .is_some_and(|next_role_offset| {
                                                run.role_selectors.iter().any(|next| {
                                                    next.entity_id == state.entity_id
                                                        && next.byte_offset == next_role_offset
                                                })
                                            })))
                                && role.selector == state.selector
                        })
                        .count()
                        == 1
            })
            && run
                .type_descriptors
                .windows(2)
                .all(|pair| pair[0].byte_offset < pair[1].byte_offset)
            && run.type_descriptors.iter().all(|descriptor| {
                descriptor.byte_offset >= run.byte_offset
                    && descriptor.byte_offset < run.catalog_offset
                    && run
                        .identities
                        .iter()
                        .rfind(|identity| identity.byte_offset < descriptor.byte_offset)
                        .is_some_and(|identity| identity.entity_id == descriptor.entity_id)
                    && match &descriptor.value {
                        CatiaLegacyTypeValue::Name { value } => valid_legacy_identifier(value),
                        CatiaLegacyTypeValue::Selector { value } => *value != 0,
                    }
            })
            && run
                .scalar_values
                .windows(2)
                .all(|pair| pair[0].byte_offset < pair[1].byte_offset)
            && run.scalar_values.iter().all(|value| {
                value.id == format!("catia:legacy:scalar#{index:08}-{:016}", value.byte_offset)
                    && value.byte_offset >= run.byte_offset
                    && value.byte_offset < run.catalog_offset
                    && run
                        .identities
                        .iter()
                        .rfind(|identity| identity.byte_offset < value.byte_offset)
                        .is_some_and(|identity| identity.entity_id == value.entity_id)
                    && (value.name.is_none()
                        || run
                            .scalar_values
                            .iter()
                            .filter(|candidate| candidate.entity_id == value.entity_id)
                            .count()
                            == 1)
                    && match (&value.name_field, &value.name) {
                        (Some(name_field), Some(name)) => {
                            legacy_value_name(
                                &run.role_selectors,
                                &run.text_fields,
                                value.entity_id,
                                value.byte_offset,
                            ) == Some((*name_field, name.clone()))
                        }
                        (None, None) => legacy_value_name(
                            &run.role_selectors,
                            &run.text_fields,
                            value.entity_id,
                            value.byte_offset,
                        )
                        .is_none(),
                        (Some(_), None) | (None, Some(_)) => false,
                    }
                    && match value.evaluation {
                        CatiaLegacyScalarEvaluation::Value { bits } => {
                            f64::from_bits(bits).is_finite()
                        }
                        CatiaLegacyScalarEvaluation::Unset => true,
                    }
            })
            && run
                .string_values
                .windows(2)
                .all(|pair| pair[0].byte_offset < pair[1].byte_offset)
            && run.string_values.iter().all(|value| {
                value.id == format!("catia:legacy:string#{index:08}-{:016}", value.byte_offset)
                    && value.byte_offset >= run.byte_offset
                    && value.byte_offset < run.catalog_offset
                    && value.value.chars().all(|character| {
                        !character.is_control() || matches!(character, '\t' | '\n' | '\r')
                    })
                    && run
                        .identities
                        .iter()
                        .rfind(|identity| identity.byte_offset < value.byte_offset)
                        .is_some_and(|identity| identity.entity_id == value.entity_id)
                    && (value.name.is_none()
                        || run
                            .string_values
                            .iter()
                            .filter(|candidate| candidate.entity_id == value.entity_id)
                            .count()
                            == 1)
                    && match (&value.name_field, &value.name) {
                        (Some(name_field), Some(name)) => {
                            legacy_value_name(
                                &run.role_selectors,
                                &run.text_fields,
                                value.entity_id,
                                value.byte_offset,
                            ) == Some((*name_field, name.clone()))
                        }
                        (None, None) => legacy_value_name(
                            &run.role_selectors,
                            &run.text_fields,
                            value.entity_id,
                            value.byte_offset,
                        )
                        .is_none(),
                        (Some(_), None) | (None, Some(_)) => false,
                    }
            })
            && run
                .integer_values
                .windows(2)
                .all(|pair| pair[0].byte_offset < pair[1].byte_offset)
            && run.integer_values.iter().all(|value| {
                value.id == format!("catia:legacy:integer#{index:08}-{:016}", value.byte_offset)
                    && value.byte_offset >= run.byte_offset
                    && value.byte_offset < run.catalog_offset
                    && match value.encoding {
                        CatiaLegacyIntegerEncoding::Inline => (0..=126).contains(&value.value),
                        CatiaLegacyIntegerEncoding::WideI32 => true,
                    }
                    && run
                        .identities
                        .iter()
                        .rfind(|identity| identity.byte_offset < value.byte_offset)
                        .is_some_and(|identity| identity.entity_id == value.entity_id)
                    && (value.name.is_none()
                        || run
                            .integer_values
                            .iter()
                            .filter(|candidate| candidate.entity_id == value.entity_id)
                            .count()
                            == 1)
                    && match (&value.name_field, &value.name) {
                        (Some(name_field), Some(name)) => {
                            legacy_value_name(
                                &run.role_selectors,
                                &run.text_fields,
                                value.entity_id,
                                value.byte_offset,
                            ) == Some((*name_field, name.clone()))
                        }
                        (None, None) => legacy_value_name(
                            &run.role_selectors,
                            &run.text_fields,
                            value.entity_id,
                            value.byte_offset,
                        )
                        .is_none(),
                        (Some(_), None) | (None, Some(_)) => false,
                    }
            });
        if !valid {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "legacy entity run `{}` has an invalid identity sequence",
                run.id
            )));
        }
        previous_end = run_end;
    }
    Ok(())
}

fn consolidated_class61_records(bytes: &[u8]) -> Vec<CatiaConsolidatedClass61Record> {
    let mut records = crate::families::b2::records::b2_counted_61(bytes)
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
            crate::families::b2::records::b2_long_61(bytes)
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
    records.sort_by_key(|(pos, _, _)| *pos);
    records
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

fn consolidated_groups(bytes: &[u8]) -> Vec<CatiaConsolidatedGroup> {
    crate::families::b2::records::b2_groups(bytes)
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
    parameter_points: &[CatiaConsolidatedParameterPoint],
) -> Vec<CatiaConsolidatedConeFace> {
    let point_ids = parameter_points
        .iter()
        .map(|point| (point.byte_offset, point.id.clone()))
        .collect::<HashMap<_, _>>();
    let class18_ends = crate::wire::records::consolidated_records(bytes)
        .into_iter()
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

fn consolidated_cones(bytes: &[u8]) -> Vec<CatiaConsolidatedCone> {
    crate::families::b2::records::b2_cones(bytes)
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
            pre_angular_range_scalar: cone.pre_angular_range_scalar,
            angular_range: cone.angular_range,
            slant_range: cone.slant_range,
            angular_scale: cone.angular_scale,
            angular_domain: cone.angular_domain,
        })
        .collect()
}

fn consolidated_cylinders(bytes: &[u8]) -> Vec<CatiaConsolidatedCylinder> {
    crate::families::b2::records::b2_cylinders(bytes)
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
                    } => CatiaConsolidatedCylinderPayload::Resolved {
                        frame_token: cylinder.frame_token,
                        axis: [axis.x, axis.y, axis.z],
                        reference_direction: [ref_direction.x, ref_direction.y, ref_direction.z],
                    },
                    _ => unreachable!("B2 cylinder parser produced a non-cylinder carrier"),
                }
            };
            CatiaConsolidatedCylinder {
                id: format!("catia:consolidated:cylinder#{index}"),
                byte_offset: cylinder.pos as u64,
                layout: cylinder.layout,
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
    groups: &[CatiaConsolidatedGroup],
) -> Vec<CatiaConsolidatedEmbeddedCylinder> {
    let group_ids = groups
        .iter()
        .map(|group| (group.byte_offset, group.id.as_str()))
        .collect::<HashMap<_, _>>();
    crate::families::b2::records::b2_embedded_cylinders(bytes)
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

fn consolidated_parameter_points(bytes: &[u8]) -> Vec<CatiaConsolidatedParameterPoint> {
    use crate::families::b2::records::B2ParameterPointPayload;

    crate::families::b2::records::b2_parameter_points(bytes)
        .into_iter()
        .enumerate()
        .map(|(index, point)| {
            let payload = match point.payload {
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
                layout: point.layout,
                prefix: point.prefix,
                control: point.control,
                payload,
            }
        })
        .collect()
}

fn consolidated_reference_lists(bytes: &[u8]) -> Vec<CatiaConsolidatedReferenceList> {
    crate::families::b2::records::b2_reference_lists(bytes)
        .into_iter()
        .enumerate()
        .map(|(index, list)| CatiaConsolidatedReferenceList {
            id: format!("catia:consolidated:reference-list#{index}"),
            byte_offset: list.pos as u64,
            references: list.references,
        })
        .collect()
}

fn consolidated_pcurves(bytes: &[u8]) -> Vec<CatiaConsolidatedPcurve> {
    let mut pcurves = crate::families::a5a8::records::a5_pcurves(bytes)
        .into_iter()
        .map(|pcurve| (pcurve, CatiaConsolidatedFamily::A))
        .chain(
            crate::families::b2::records::b2_pcurves(bytes)
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
    circles: &[CatiaConsolidatedCircle],
) -> Vec<CatiaConsolidatedRevolution> {
    let resolved_profiles = crate::families::b2::records::b2_resolved_revolutions(bytes)
        .into_iter()
        .map(|resolved| (resolved.revolution.pos as u64, resolved.profile.pos as u64))
        .collect::<HashMap<_, _>>();
    let circle_ids = circles
        .iter()
        .map(|circle| (circle.byte_offset, circle.id.clone()))
        .collect::<HashMap<_, _>>();
    crate::families::b2::records::b2_revolutions(bytes)
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

fn consolidated_line_profiles(bytes: &[u8]) -> Vec<CatiaConsolidatedLineProfile> {
    crate::families::b2::records::b2_line_profiles(bytes)
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

fn consolidated_spheres(bytes: &[u8]) -> Vec<CatiaConsolidatedSphere> {
    crate::families::b2::records::b2_spheres(bytes)
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

fn consolidated_tori(bytes: &[u8]) -> Vec<CatiaConsolidatedTorus> {
    crate::families::b2::records::b2_tori(bytes)
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

fn zero_entity_edge_strides(bytes: &[u8]) -> Vec<CatiaZeroEntityEdgeStride> {
    crate::families::zero_entity::records::zero_entity_edge_strides(bytes)
        .into_iter()
        .enumerate()
        .map(|(index, record)| CatiaZeroEntityEdgeStride {
            id: format!("catia:zero-entity:edge-stride#{index}"),
            byte_offset: record.pos as u64,
            record_ordinal: record.record_ordinal,
            allocations: record.allocations,
        })
        .collect()
}

fn zero_entity_oriented_use_pairs(bytes: &[u8]) -> Vec<CatiaZeroEntityOrientedUsePair> {
    crate::families::zero_entity::records::zero_entity_oriented_use_pairs(bytes)
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

fn zero_entity_ownership_roots(bytes: &[u8]) -> Vec<CatiaZeroEntityOwnershipRoot> {
    crate::families::zero_entity::records::zero_entity_ownership_root(bytes)
        .into_iter()
        .map(|root| CatiaZeroEntityOwnershipRoot {
            id: "catia:zero-entity:ownership-root#0".to_string(),
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
    records: &[CatiaZeroEntityRecord],
) -> Vec<CatiaZeroEntityVertexIncidence> {
    crate::families::zero_entity::records::zero_entity_vertex_incidences(bytes)
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

fn zero_entity_records(bytes: &[u8]) -> Vec<CatiaZeroEntityRecord> {
    crate::families::zero_entity::records::zero_entity_record_inventory(bytes)
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

fn consolidated_owner_packets(bytes: &[u8]) -> Vec<CatiaConsolidatedOwnerPacket> {
    let links = crate::families::b2::records::b2_linked_owners(bytes)
        .into_iter()
        .map(|linked| (linked.owner.pos, linked.link))
        .chain(
            crate::families::b2::records::b2_linked_counted_owners(bytes)
                .into_iter()
                .map(|linked| (linked.owner.pos, linked.link)),
        )
        .collect::<HashMap<_, _>>();
    let fixed = crate::families::b2::records::b2_owner_packets(bytes);
    let fixed_positions = fixed
        .iter()
        .map(|packet| packet.pos)
        .collect::<HashSet<_>>();
    let mut packets = fixed
        .into_iter()
        .map(|packet| {
            (
                packet.pos,
                packet.header_token,
                CatiaOwnerPacketPayload::FixedNine {
                    reference_encoding: match packet.reference_encoding {
                        crate::families::b2::records::B2OwnerReferenceEncoding::TaggedU16Strong => {
                            CatiaOwnerReferenceEncoding::TaggedU16Strong
                        }
                        crate::families::b2::records::B2OwnerReferenceEncoding::WidthCodedStrong => {
                            CatiaOwnerReferenceEncoding::WidthCodedStrong
                        }
                    },
                    references: packet.references,
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
            crate::families::b2::records::b2_counted_owners(bytes)
                .into_iter()
                .filter(|packet| !fixed_positions.contains(&packet.pos))
                .map(|packet| {
                    (
                        packet.pos,
                        packet.header_token,
                        CatiaOwnerPacketPayload::Counted {
                            references: packet.references,
                            tail: packet.tail,
                        },
                    )
                }),
        )
        .collect::<Vec<_>>();
    packets.sort_by_key(|(pos, _, _)| *pos);
    packets
        .into_iter()
        .map(
            |(pos, header_token, payload)| CatiaConsolidatedOwnerPacket {
                id: format!("catia:consolidated:owner-packet#{pos:010}"),
                byte_offset: pos as u64,
                header_token,
                payload,
                allocation_link: links.get(&pos).map(|link| CatiaOwnerAllocationLink {
                    byte_offset: link.pos as u64,
                    byte_len: (pos - link.pos) as u64,
                    header_token: link.header_token,
                    target: link.target,
                }),
            },
        )
        .collect()
}

fn consolidated_edge_runs(
    bytes: &[u8],
    pcurves: &[CatiaConsolidatedPcurve],
    nodes: &[CatiaConsolidatedEdgeNode],
) -> Vec<CatiaConsolidatedEdgeRun> {
    let pcurve_ids = pcurves
        .iter()
        .map(|pcurve| (pcurve.byte_offset, pcurve.id.clone()))
        .collect::<HashMap<_, _>>();
    let resolved = crate::families::consolidated::records::resolve_consolidated_edge_blocks(bytes)
        .into_iter()
        .map(|block| (block.block.pcurves[0].pos, block))
        .collect::<HashMap<_, _>>();
    let nodes_by_offset = nodes
        .iter()
        .map(|node| (node.byte_offset, node))
        .collect::<HashMap<_, _>>();
    crate::families::consolidated::records::consolidated_topology_edge_runs(bytes)
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
    circles: &[CatiaConsolidatedCircle],
) -> Vec<CatiaConsolidatedEdgeNode> {
    let circle_ids = circles
        .iter()
        .map(|circle| (circle.byte_offset, circle.id.as_str()))
        .collect::<HashMap<_, _>>();
    let frames = crate::wire::records::consolidated_records(bytes)
        .into_iter()
        .filter(|record| {
            record.family == crate::wire::records::ConsolidatedFamily::B && record.class == 0x5e
        })
        .map(|record| (record.range.start, (record.width, record.flag)))
        .collect::<HashMap<_, _>>();
    let use_runs = crate::families::consolidated::records::consolidated_edge_use_runs(bytes)
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
        crate::families::consolidated::records::consolidated_analytic_circle_edge_runs(bytes)
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
        crate::families::consolidated::records::consolidated_class25_edge_runs(bytes)
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
    crate::families::b2::records::b2_edge_nodes(bytes)
        .into_iter()
        .enumerate()
        .filter_map(|(index, node)| {
            let (width, flag) = frames.get(&node.pos)?;
            Some(CatiaConsolidatedEdgeNode {
                id: format!("catia:consolidated:edge-node#{index}"),
                byte_offset: node.pos as u64,
                width: *width,
                flag: *flag,
                header_token: node.header_token,
                curve_ref: node.curve_ref,
                vertex_refs: [node.start_vertex_ref, node.end_vertex_ref],
                vertices: [String::new(), String::new()],
                parameter_selectors: [node.start_parameter_ref, node.end_parameter_ref],
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
    let mut identities = Vec::<CatiaConsolidatedVertexIdentity>::new();
    let mut identity_indices = HashMap::<u32, usize>::new();
    for node in nodes {
        for (endpoint, identity) in node.vertex_refs.into_iter().enumerate() {
            let index = *identity_indices.entry(identity).or_insert_with(|| {
                let index = identities.len();
                identities.push(CatiaConsolidatedVertexIdentity {
                    id: format!("catia:consolidated:vertex-identity#{index}"),
                    identity,
                    incident_edge_nodes: Vec::new(),
                });
                index
            });
            let vertex = &mut identities[index];
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
        crate::families::consolidated::records::ConsolidatedSupportBinding::NurbsCarrier {
            pos,
            offset,
        } => CatiaConsolidatedSupportBinding::NurbsCarrier {
            byte_offset: *pos as u64,
            offset: *offset,
        },
    }
}

#[cfg(test)]
fn validate_consolidated_class61_records(
    records: &[CatiaConsolidatedClass61Record],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, record) in records.iter().enumerate() {
        let expected_id = format!("catia:consolidated:class61-record#{index}");
        let valid_payload = match &record.payload {
            CatiaConsolidatedClass61Payload::Counted { references, tail } => {
                !references.is_empty() && !tail.is_empty() && tail.last() == Some(&0x03)
            }
            CatiaConsolidatedClass61Payload::Long {
                members, scalar, ..
            } => {
                scalar.is_finite()
                    && !members.is_empty()
                    && members.windows(2).all(|pair| pair[0] < pair[1])
            }
        };
        if record.id != expected_id
            || !valid_payload
            || index > 0 && records[index - 1].byte_offset >= record.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated class-0x61 record `{}` is structurally invalid",
                record.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_consolidated_groups(
    groups: &[CatiaConsolidatedGroup],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, group) in groups.iter().enumerate() {
        let expected_id = format!("catia:consolidated:group#{index}");
        if group.id != expected_id
            || index > 0 && groups[index - 1].byte_offset >= group.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated group `{}` is structurally invalid",
                group.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_consolidated_cone_faces(
    faces: &[CatiaConsolidatedConeFace],
    parameter_points: &[CatiaConsolidatedParameterPoint],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let points_by_id = parameter_points
        .iter()
        .map(|point| (point.id.as_str(), point))
        .collect::<HashMap<_, _>>();
    for (index, face) in faces.iter().enumerate() {
        let mut expected_point_offset = face.byte_offset.checked_add(face.byte_len);
        let parameter_run_valid = face.parameter_points.iter().all(|id| {
            match (expected_point_offset, points_by_id.get(id.as_str())) {
                (Some(expected), Some(point)) if point.byte_offset == expected => {
                    expected_point_offset = point.byte_offset.checked_add(point.byte_len);
                    expected_point_offset.is_some()
                }
                _ => false,
            }
        });
        let frame_overhead = face
            .byte_len
            .checked_sub(u64::try_from(face.program.len()).unwrap_or(u64::MAX));
        if face.id != format!("catia:consolidated:cone-face#{index}")
            || face.program.len() < 16
            || face.program.first() != Some(&0x85)
            || !face.program.ends_with(&[0x03, 0x11])
            || !matches!(frame_overhead, Some(21..=23))
            || !face.angular_scale.is_finite()
            || face.half_angle <= 0.0
            || face.half_angle >= std::f64::consts::FRAC_PI_2
            || !parameter_run_valid
            || index > 0 && faces[index - 1].byte_offset >= face.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated cone-face descriptor `{}` is structurally invalid",
                face.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_consolidated_pcurves(
    pcurves: &[CatiaConsolidatedPcurve],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, pcurve) in pcurves.iter().enumerate() {
        let expected_id = format!("catia:consolidated:pcurve#{index}");
        let count = pcurve.knots.len();
        if pcurve.id != expected_id
            || pcurve.degree != 5
            || count < 2
            || pcurve.points.len() != count
            || pcurve.first_derivatives.len() != count
            || pcurve.second_derivatives.len() != count
            || pcurve.knots.windows(2).any(|pair| pair[0] >= pair[1])
            || pcurve.range[0] >= pcurve.range[1]
            || pcurve
                .knots
                .iter()
                .chain(pcurve.points.iter().flatten())
                .chain(pcurve.first_derivatives.iter().flatten())
                .chain(pcurve.second_derivatives.iter().flatten())
                .chain(&pcurve.range)
                .any(|value| !value.is_finite())
            || !matches!(pcurve.tail.as_slice(), [0x07] | [0x07, 0x00])
            || index > 0 && pcurves[index - 1].byte_offset >= pcurve.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated pcurve `{}` is structurally invalid",
                pcurve.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_consolidated_circles(
    circles: &[CatiaConsolidatedCircle],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, circle) in circles.iter().enumerate() {
        let full_circle =
            crate::families::b2::records::circle_range_is_full_turn(circle.radius, circle.range);
        let compact_len = usize::from(circle.layout).checked_sub(5 * size_of::<f64>() + 9);
        let record_id_fits_layout = matches!(
            (compact_len, circle.record_id),
            (Some(1), 0..=63) | (Some(2), 0..=255) | (Some(3), 0..=65_535)
        );
        if circle.id != format!("catia:consolidated:circle#{index}")
            || !(0x32..=0x34).contains(&circle.layout)
            || !record_id_fits_layout
            || circle
                .center_pair
                .iter()
                .chain(&circle.range)
                .chain(&[circle.radius, circle.chart_shift])
                .any(|value| !value.is_finite())
            || circle.center_pair.iter().any(|value| value.abs() > 1e6)
            || circle.radius <= 0.0
            || circle.range[0] >= circle.range[1]
            || circle.full_circle != full_circle
            || index > 0 && circles[index - 1].byte_offset >= circle.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated circle `{}` is structurally invalid",
                circle.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_consolidated_cones(
    cones: &[CatiaConsolidatedCone],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, cone) in cones.iter().enumerate() {
        let expected_id = format!("catia:consolidated:cone#{index}");
        let dot = |first: [f64; 3], second: [f64; 3]| {
            first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
        };
        let cross = [
            cone.direction_x[1] * cone.direction_y[2] - cone.direction_x[2] * cone.direction_y[1],
            cone.direction_x[2] * cone.direction_y[0] - cone.direction_x[0] * cone.direction_y[2],
            cone.direction_x[0] * cone.direction_y[1] - cone.direction_x[1] * cone.direction_y[0],
        ];
        if cone.id != expected_id
            || cone
                .apex
                .iter()
                .chain(&cone.direction_x)
                .chain(&cone.direction_y)
                .chain(&cone.axis)
                .chain(&[
                    cone.half_angle,
                    cone.pre_angular_range_scalar,
                    cone.angular_range[0],
                    cone.angular_range[1],
                    cone.slant_range[0],
                    cone.slant_range[1],
                    cone.angular_scale,
                    cone.angular_domain[0],
                    cone.angular_domain[1],
                ])
                .any(|value| !value.is_finite())
            || [cone.direction_x, cone.direction_y, cone.axis]
                .into_iter()
                .any(|direction| (dot(direction, direction) - 1.0).abs() > 1e-9)
            || cross
                .iter()
                .zip(cone.axis)
                .any(|(cross, axis)| (cross - axis).abs() > 1e-9)
            || cone.half_angle <= 0.0
            || cone.half_angle >= std::f64::consts::FRAC_PI_2
            || !crate::analytic::periodic_angular_range_is_valid(
                cone.angular_range,
                cone.angular_domain,
            )
            || cone.slant_range[0] < 0.0
            || cone.slant_range[0] >= cone.slant_range[1]
            || cone.angular_scale <= 0.0
            || index > 0 && cones[index - 1].byte_offset >= cone.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated cone `{}` is structurally invalid",
                cone.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_consolidated_cylinders(
    cylinders: &[CatiaConsolidatedCylinder],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, cylinder) in cylinders.iter().enumerate() {
        let expected_id = format!("catia:consolidated:cylinder#{index}");
        let squared_length =
            |direction: [f64; 3]| direction.iter().map(|value| value * value).sum::<f64>();
        let dot = |first: [f64; 3], second: [f64; 3]| {
            first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
        };
        let payload_valid = match &cylinder.payload {
            CatiaConsolidatedCylinderPayload::Resolved {
                frame_token,
                axis,
                reference_direction,
            } => {
                let frame_matches_layout = match cylinder.layout {
                    0x52 => {
                        *frame_token == 0x1d
                            && *axis == [1.0, 0.0, 0.0]
                            && *reference_direction == [0.0, 1.0, 0.0]
                    }
                    0x5a => {
                        matches!(*frame_token, 0x19 | 0x1c)
                            && axis[2] == 0.0
                            && *reference_direction == [-axis[1], axis[0], 0.0]
                    }
                    _ => false,
                };
                frame_matches_layout
                    && axis
                        .iter()
                        .chain(reference_direction)
                        .all(|value| value.is_finite())
                    && (squared_length(*axis) - 1.0).abs() <= 1e-9
                    && (squared_length(*reference_direction) - 1.0).abs() <= 1e-9
                    && dot(*axis, *reference_direction).abs() <= 1e-9
                    && crate::families::b2::records::circle_range_is_full_turn(
                        cylinder.radius,
                        cylinder.u_range,
                    )
            }
            CatiaConsolidatedCylinderPayload::RangeOrigin {
                stored_vector,
                axis,
                reference_direction,
                range_origin,
            } => {
                cylinder.layout == 0x62
                    && stored_vector
                        .iter()
                        .chain(std::iter::once(range_origin))
                        .all(|value| value.is_finite())
                    && (stored_vector[0].hypot(stored_vector[1]) - 1.0).abs() <= 1e-9
                    && *axis == [0.0, 1.0, 0.0]
                    && *reference_direction == [stored_vector[0], 0.0, stored_vector[1]]
                    && crate::families::b2::records::circle_range_is_within_full_turn(
                        cylinder.radius,
                        cylinder.u_range,
                    )
                    && range_origin.to_bits()
                        == crate::families::b2::records::cylinder_range_origin(
                            cylinder.radius,
                            cylinder.u_range,
                        )
                        .to_bits()
            }
        };
        if cylinder.id != expected_id
            || cylinder
                .origin
                .iter()
                .chain(&cylinder.u_range)
                .chain(&cylinder.v_range)
                .chain(&[cylinder.radius])
                .any(|value| !value.is_finite())
            || cylinder.radius <= 0.0
            || cylinder.u_range[0] >= cylinder.u_range[1]
            || cylinder.v_range[0] >= cylinder.v_range[1]
            || !payload_valid
            || index > 0 && cylinders[index - 1].byte_offset >= cylinder.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated cylinder `{}` is structurally invalid",
                cylinder.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_consolidated_embedded_cylinders(
    cylinders: &[CatiaConsolidatedEmbeddedCylinder],
    groups: &[CatiaConsolidatedGroup],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let groups = groups
        .iter()
        .enumerate()
        .map(|(index, group)| {
            (
                group.id.as_str(),
                (group, groups.get(index + 1).map(|next| next.byte_offset)),
            )
        })
        .collect::<HashMap<_, _>>();
    for (index, cylinder) in cylinders.iter().enumerate() {
        let squared_length =
            |direction: [f64; 3]| direction.iter().map(|value| value * value).sum::<f64>();
        let dot = |first: [f64; 3], second: [f64; 3]| {
            first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
        };
        let group_valid =
            groups
                .get(cylinder.group.as_str())
                .is_some_and(|(group, next_offset)| {
                    group.group_type == 3
                        && group.byte_offset < cylinder.byte_offset
                        && next_offset.is_none_or(|next| cylinder.byte_offset < next)
                });
        if cylinder.id != format!("catia:consolidated:embedded-cylinder#{index}")
            || !group_valid
            || !cylinder
                .origin
                .iter()
                .chain(&cylinder.u_range)
                .chain(&cylinder.v_range)
                .chain(&cylinder.axis)
                .chain(&cylinder.reference_direction)
                .chain(&[cylinder.radius])
                .all(|value| value.is_finite())
            || cylinder.radius <= 0.0
            || cylinder.u_range[0] >= cylinder.u_range[1]
            || cylinder.v_range[0] >= cylinder.v_range[1]
            || !matches!(cylinder.frame_token, 0x19 | 0x1c)
            || cylinder.axis[2] != 0.0
            || cylinder.reference_direction != [-cylinder.axis[1], cylinder.axis[0], 0.0]
            || (squared_length(cylinder.axis) - 1.0).abs() > 1e-9
            || (squared_length(cylinder.reference_direction) - 1.0).abs() > 1e-9
            || dot(cylinder.axis, cylinder.reference_direction).abs() > 1e-9
            || !crate::families::b2::records::circle_range_is_full_turn(
                cylinder.radius,
                cylinder.u_range,
            )
            || index > 0 && cylinders[index - 1].byte_offset >= cylinder.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated embedded cylinder `{}` is structurally invalid",
                cylinder.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_consolidated_parameter_points(
    points: &[CatiaConsolidatedParameterPoint],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, point) in points.iter().enumerate() {
        let payload_valid = match &point.payload {
            CatiaConsolidatedParameterPointPayload::Uv { uv } => {
                point.layout == 0x12 && uv.iter().all(|value| value.is_finite())
            }
            CatiaConsolidatedParameterPointPayload::StationUv { station, uv } => {
                point.layout == 0x1a
                    && station.is_finite()
                    && uv.iter().all(|value| value.is_finite())
            }
            CatiaConsolidatedParameterPointPayload::FiveScalars { values } => {
                point.layout == 0x2a && values.iter().all(|value| value.is_finite())
            }
        };
        let frame_overhead = point.byte_len.checked_sub(u64::from(point.layout));
        if point.id != format!("catia:consolidated:parameter-point#{index}")
            || !matches!(frame_overhead, Some(5..=7))
            || !matches!(point.prefix, 0x05 | 0x09 | 0x0d | 0x11)
            || !payload_valid
            || index > 0 && points[index - 1].byte_offset >= point.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated parameter point `{}` is structurally invalid",
                point.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_consolidated_reference_lists(
    lists: &[CatiaConsolidatedReferenceList],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, list) in lists.iter().enumerate() {
        if list.id != format!("catia:consolidated:reference-list#{index}")
            || list.references.is_empty()
            || index > 0 && lists[index - 1].byte_offset >= list.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated reference list `{}` is structurally invalid",
                list.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_consolidated_revolutions(
    revolutions: &[CatiaConsolidatedRevolution],
    circles: &[CatiaConsolidatedCircle],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, revolution) in revolutions.iter().enumerate() {
        let mut profile_candidates = circles.iter().filter(|circle| {
            circle.range[0].to_bits() == revolution.profile_range[0].to_bits()
                && circle.range[1].to_bits() == revolution.profile_range[1].to_bits()
        });
        let expected_profile = profile_candidates.next().and_then(|circle| {
            profile_candidates
                .next()
                .is_none()
                .then_some(circle.id.as_str())
        });
        let expected_id = format!("catia:consolidated:revolution#{index}");
        let squared_length = |direction: [f64; 3]| {
            direction
                .iter()
                .map(|component| component * component)
                .sum::<f64>()
        };
        let cross = [
            revolution.direction_x[1] * revolution.direction_y[2]
                - revolution.direction_x[2] * revolution.direction_y[1],
            revolution.direction_x[2] * revolution.direction_y[0]
                - revolution.direction_x[0] * revolution.direction_y[2],
            revolution.direction_x[0] * revolution.direction_y[1]
                - revolution.direction_x[1] * revolution.direction_y[0],
        ];
        if revolution.id != expected_id
            || !matches!(revolution.reference_token, 0x08 | 0x0a)
            || revolution.profile_allocation_id == 0
            || revolution
                .origin
                .iter()
                .chain(&revolution.direction_x)
                .chain(&revolution.direction_y)
                .chain(&revolution.axis)
                .chain(&revolution.angular_range)
                .chain(&revolution.profile_range)
                .chain(&[revolution.angular_scale])
                .any(|value| !value.is_finite())
            || revolution.angular_scale <= 0.0
            || revolution.angular_range[0] >= revolution.angular_range[1]
            || revolution.profile_range[0] >= revolution.profile_range[1]
            || revolution.profile_circle.as_deref() != expected_profile
            || [
                revolution.direction_x,
                revolution.direction_y,
                revolution.axis,
            ]
            .into_iter()
            .any(|direction| (squared_length(direction) - 1.0).abs() > 1e-12)
            || cross
                .iter()
                .zip(revolution.axis)
                .any(|(cross, axis)| (cross - axis).abs() > 1e-12)
            || revolution.angular_range[0] / revolution.angular_scale != 0.5
            || (revolution.angular_range[1] - revolution.angular_range[0])
                / revolution.angular_scale
                != std::f64::consts::TAU
            || index > 0 && revolutions[index - 1].byte_offset >= revolution.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated revolution `{}` is structurally invalid",
                revolution.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_consolidated_line_profiles(
    lines: &[CatiaConsolidatedLineProfile],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, line) in lines.iter().enumerate() {
        let squared_length = line
            .direction
            .iter()
            .map(|component| component * component)
            .sum::<f64>();
        if line.id != format!("catia:consolidated:line-profile#{index}")
            || line
                .origin
                .iter()
                .chain(&line.direction)
                .chain(&line.range)
                .any(|value| !value.is_finite())
            || (squared_length - 1.0).abs() > 1e-12
            || line.range[0] >= line.range[1]
            || index > 0 && lines[index - 1].byte_offset >= line.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated line profile `{}` is structurally invalid",
                line.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_consolidated_spheres(
    spheres: &[CatiaConsolidatedSphere],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, sphere) in spheres.iter().enumerate() {
        let expected_id = format!("catia:consolidated:sphere#{index}");
        let dot = |first: [f64; 3], second: [f64; 3]| {
            first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
        };
        let cross = [
            sphere.direction_x[1] * sphere.direction_y[2]
                - sphere.direction_x[2] * sphere.direction_y[1],
            sphere.direction_x[2] * sphere.direction_y[0]
                - sphere.direction_x[0] * sphere.direction_y[2],
            sphere.direction_x[0] * sphere.direction_y[1]
                - sphere.direction_x[1] * sphere.direction_y[0],
        ];
        if sphere.id != expected_id
            || sphere
                .center
                .iter()
                .chain(&sphere.direction_x)
                .chain(&sphere.direction_y)
                .chain(&sphere.axis)
                .chain(&sphere.azimuth_range)
                .chain(&sphere.latitude_range)
                .chain(&[sphere.radius])
                .any(|value| !value.is_finite())
            || [sphere.direction_x, sphere.direction_y, sphere.axis]
                .into_iter()
                .any(|direction| (dot(direction, direction) - 1.0).abs() > 1e-12)
            || dot(sphere.direction_x, sphere.direction_y).abs() > 1e-12
            || dot(sphere.direction_x, sphere.axis).abs() > 1e-12
            || dot(sphere.direction_y, sphere.axis).abs() > 1e-12
            || cross
                .iter()
                .zip(sphere.axis)
                .any(|(cross, axis)| (cross - axis).abs() > 1e-12)
            || sphere.radius <= 0.0
            || !crate::analytic::sphere_angular_ranges_are_valid(
                sphere.azimuth_range,
                sphere.latitude_range,
            )
            || index > 0 && spheres[index - 1].byte_offset >= sphere.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated sphere `{}` is structurally invalid",
                sphere.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_consolidated_tori(
    tori: &[CatiaConsolidatedTorus],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, torus) in tori.iter().enumerate() {
        let expected_id = format!("catia:consolidated:torus#{index}");
        let dot = |first: [f64; 3], second: [f64; 3]| {
            first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
        };
        let cross = [
            torus.direction_x[1] * torus.direction_y[2]
                - torus.direction_x[2] * torus.direction_y[1],
            torus.direction_x[2] * torus.direction_y[0]
                - torus.direction_x[0] * torus.direction_y[2],
            torus.direction_x[0] * torus.direction_y[1]
                - torus.direction_x[1] * torus.direction_y[0],
        ];
        if torus.id != expected_id
            || torus
                .center
                .iter()
                .chain(&torus.direction_x)
                .chain(&torus.direction_y)
                .chain(&torus.axis)
                .chain(&torus.major_angular_range)
                .chain(&torus.major_angular_domain)
                .chain(&torus.minor_angular_range)
                .chain(&torus.minor_angular_domain)
                .chain(&[
                    torus.major_radius,
                    torus.minor_radius,
                    torus.major_scale,
                    torus.minor_scale,
                ])
                .any(|value| !value.is_finite())
            || [torus.direction_x, torus.direction_y, torus.axis]
                .into_iter()
                .any(|direction| (dot(direction, direction) - 1.0).abs() > 1e-12)
            || dot(torus.direction_x, torus.direction_y).abs() > 1e-12
            || dot(torus.direction_x, torus.axis).abs() > 1e-12
            || dot(torus.direction_y, torus.axis).abs() > 1e-12
            || cross
                .iter()
                .zip(torus.axis)
                .any(|(cross, axis)| (cross - axis).abs() > 1e-12)
            || torus.major_radius <= 0.0
            || torus.minor_radius <= 0.0
            || !crate::analytic::periodic_angular_range_is_valid(
                torus.major_angular_range,
                torus.major_angular_domain,
            )
            || !crate::analytic::periodic_angular_range_is_valid(
                torus.minor_angular_range,
                torus.minor_angular_domain,
            )
            || torus.major_scale <= 0.0
            || torus.minor_scale <= 0.0
            || index > 0 && tori[index - 1].byte_offset >= torus.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated torus `{}` is structurally invalid",
                torus.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_zero_entity_support_runs(
    runs: &[CatiaZeroEntitySupportRun],
    records: &[CatiaZeroEntityRecord],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let face_count = runs.iter().filter(|run| run.face.is_some()).count();
    let face_roster_valid = face_count == 0 || face_count == runs.len();
    let expected_loop_count = runs
        .iter()
        .filter_map(|run| run.face.as_ref())
        .map(|face| face.loop_terminals.len())
        .sum::<usize>();
    let loops = runs
        .iter()
        .filter_map(|run| run.face.as_ref())
        .flat_map(|face| &face.loops)
        .collect::<Vec<_>>();
    let loop_roster_valid = loops.is_empty()
        || loops.len() == expected_loop_count
            && loops
                .windows(2)
                .all(|pair| pair[0].byte_offset < pair[1].byte_offset);
    for (index, run) in runs.iter().enumerate() {
        let support_bindings_valid = run.face.as_ref().is_none_or(|face| {
            let binding_count = face
                .loops
                .iter()
                .filter(|loop_record| !loop_record.support_record_ordinals.is_empty())
                .count();
            if binding_count == 0 {
                return true;
            }
            if binding_count != face.loops.len() {
                return false;
            }
            let mut bound = HashSet::new();
            face.loops.iter().all(|loop_record| {
                loop_record
                    .member_ids
                    .iter()
                    .zip(&loop_record.support_record_ordinals)
                    .all(|(member, record_ordinal)| {
                        let slot = loop_record.terminal_id.checked_sub(*member);
                        bound.insert(*record_ordinal)
                            && run.supports.iter().any(|support| {
                                support.record_ordinal == *record_ordinal
                                    && Some(support.face_local_slot) == slot
                            })
                    })
            }) && bound.len() == run.supports.len()
        });
        let face_valid = run.face.as_ref().is_none_or(|face| {
            let derived_terminals = face.allocations.first().and_then(|first| {
                face.allocations[1..]
                    .iter()
                    .map(|allocation| first.checked_sub(*allocation))
                    .collect::<Option<Vec<_>>>()
            });
            let expected_length = face
                .allocations
                .len()
                .checked_mul(5)
                .and_then(|length| length.checked_add(14));
            face.tag[0] == 0x5f
                && face.allocations.len() >= 2
                && !face.allocations.contains(&0)
                && !face.loop_terminals.contains(&0)
                && face.loop_terminals[1..]
                    .windows(2)
                    .all(|pair| pair[0] < pair[1])
                && matches!(face.terminal_control, 0x03 | 0x05)
                && expected_length == Some(usize::from(face.tag[1]) + 12)
                && derived_terminals.as_ref() == Some(&face.loop_terminals)
                && (face.loops.is_empty()
                    || face.loops.len() == face.loop_terminals.len()
                        && face
                            .loops
                            .first()
                            .is_some_and(|outer| matches!(outer.loop_class, 0x41 | 0xc1))
                        && face.loops[1..].iter().all(|inner| inner.loop_class == 0x50)
                        && face.loops.iter().zip(&face.loop_terminals).all(
                            |(loop_record, terminal)| {
                                let edge_count = loop_record.member_ids.len();
                                let reference_count = edge_count
                                    .checked_mul(2)
                                    .and_then(|count| count.checked_add(1));
                                let packed_length = edge_count
                                    .checked_mul(3)
                                    .and_then(|bits| bits.checked_add(7));
                                let expected_length = reference_count.zip(packed_length).and_then(
                                    |(reference_count, packed_length)| {
                                        reference_count
                                            .checked_mul(5)?
                                            .checked_add(16 + packed_length / 8)
                                    },
                                );
                                loop_record.tag[0] == 0x62
                                    && !loop_record.member_ids.is_empty()
                                    && loop_record.typed_references.len() == edge_count
                                    && !loop_record.typed_references.contains(&0)
                                    && (loop_record.typed_records.is_empty()
                                        || loop_record.typed_records.len() == edge_count
                                            && loop_record
                                                .typed_references
                                                .iter()
                                                .zip(&loop_record.typed_records)
                                                .all(|(ordinal, id)| {
                                                    zero_entity_record(records, *ordinal)
                                                        .is_some_and(|record| &record.id == id)
                                                }))
                                    && (loop_record.support_record_ordinals.is_empty()
                                        || loop_record.support_record_ordinals.len() == edge_count)
                                    && loop_record.forward_senses.len() == edge_count
                                    && {
                                        let endpoints = loop_record
                                            .support_record_ordinals
                                            .iter()
                                            .map(|ordinal| {
                                                run.supports
                                                    .iter()
                                                    .find(|support| {
                                                        support.record_ordinal == *ordinal
                                                    })
                                                    .and_then(|support| support.model_endpoints)
                                            })
                                            .collect::<Vec<_>>();
                                        let expected = crate::families::zero_entity::records::
                                            oriented_closed_model_endpoints(
                                                &endpoints,
                                                &loop_record.forward_senses,
                                            )
                                            .unwrap_or_default();
                                        loop_record.oriented_model_endpoints == expected
                                    }
                                    && loop_record.terminal_id == *terminal
                                    && loop_record.gap != 0
                                    && matches!(loop_record.loop_class, 0x41 | 0x50 | 0xc1)
                                    && loop_record.member_ids.iter().enumerate().all(
                                        |(member_index, member)| {
                                            u32::try_from(member_index).ok().and_then(
                                                |member_index| {
                                                    loop_record
                                                        .terminal_id
                                                        .checked_sub(loop_record.gap)?
                                                        .checked_sub(member_index)
                                                },
                                            ) == Some(*member)
                                        },
                                    )
                                    && expected_length == Some(usize::from(loop_record.tag[1]) + 12)
                                    && zero_entity_record(records, loop_record.record_ordinal)
                                        .is_some_and(|record| {
                                            record.byte_offset == loop_record.byte_offset
                                                && record.tag == loop_record.tag
                                        })
                            },
                        ))
                && zero_entity_record(records, face.record_ordinal).is_some_and(|record| {
                    record.byte_offset == face.byte_offset && record.tag == face.tag
                })
                && support_bindings_valid
                && (index == 0
                    || runs[index - 1]
                        .face
                        .as_ref()
                        .is_none_or(|previous| previous.byte_offset < face.byte_offset))
        });
        let carrier_tag =
            zero_entity_record(records, run.carrier_record_ordinal).map(|record| record.tag);
        let supports_valid = !run.supports.is_empty()
            && run
                .supports
                .iter()
                .enumerate()
                .all(|(support_index, support)| {
                    if support.face_local_slot == 0 {
                        return false;
                    }
                    let endpoints_valid = match (support.tag, support.uv_endpoints) {
                        (
                            [0x21, 0x45 | 0x71 | 0x72 | 0x91 | 0x99 | 0x9f | 0xd6 | 0xe8],
                            Some(endpoints),
                        ) => endpoints.iter().flatten().all(|value| value.is_finite()),
                        ([0x21, 0x45 | 0x71 | 0x72 | 0x91 | 0x99 | 0x9f | 0xd6 | 0xe8], None) => {
                            false
                        }
                        ([0x21, _], None) => true,
                        _ => false,
                    };
                    let model_endpoints_valid = support.model_endpoints.is_none_or(|endpoints| {
                        support.uv_endpoints.is_some()
                            && endpoints.iter().all(|point| {
                                [point.x, point.y, point.z].into_iter().all(f64::is_finite)
                            })
                    });
                    let model_midpoint_valid = support.model_midpoint.is_none_or(|point| {
                        [point.x, point.y, point.z].into_iter().all(f64::is_finite)
                    });
                    let model_curve_valid =
                        validate_zero_entity_model_curve(carrier_tag, support.model_curve.as_ref());
                    let model_curve_construction_valid =
                        validate_zero_entity_model_curve_construction(
                            carrier_tag,
                            support.model_curve.as_ref(),
                            support.model_curve_construction.as_ref(),
                        );
                    let has_model_carrier =
                        support.model_curve.is_some() || support.model_curve_construction.is_some();
                    let has_pcurve = support.pcurve.is_some();
                    let model_parameters_valid =
                        support.model_parameters.is_some_and(|parameters| {
                            parameters.into_iter().all(f64::is_finite)
                                && parameters[0] != parameters[1]
                        }) == has_model_carrier;
                    let pcurve_valid = match (&support.tag, &support.pcurve) {
                        (
                            [0x21, tag @ (0x45 | 0x71 | 0x72 | 0x91 | 0x99 | 0x9f | 0xd6 | 0xe8)],
                            Some(cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
                                degree,
                                knots,
                                control_points,
                                weights,
                                periodic: false,
                            }),
                        ) => {
                            let (
                                expected_degree,
                                expected_controls,
                                expected_multiplicities,
                                rational,
                            ): (u32, usize, &[usize], bool) = match tag {
                                0x45 => (3, 12, &[4, 2, 2, 2, 2, 4], false),
                                0x71 => (1, 2, &[2, 2], false),
                                0x72 => (3, 14, &[4, 2, 2, 2, 2, 2, 4], false),
                                0x91 => (3, 4, &[4, 4], false),
                                0x99 => (2, 3, &[3, 3], true),
                                0x9f => (3, 16, &[4, 2, 2, 2, 2, 2, 2, 4], false),
                                0xd6 => (2, 5, &[3, 2, 3], false),
                                0xe8 => (3, 7, &[4, 1, 1, 1, 4], false),
                                _ => unreachable!(),
                            };
                            *degree == expected_degree
                                && control_points.len() == expected_controls
                                && knots.len() == expected_controls + expected_degree as usize + 1
                                && knots.iter().all(|knot| knot.is_finite())
                                && knots.windows(2).all(|pair| pair[0] <= pair[1])
                                && knots[..=expected_degree as usize]
                                    .iter()
                                    .all(|knot| *knot == knots[0])
                                && knots[expected_controls..]
                                    .iter()
                                    .all(|knot| *knot == knots[expected_controls])
                                && knots[expected_degree as usize] < knots[expected_controls]
                                && knots
                                    .chunk_by(|left, right| left == right)
                                    .map(<[f64]>::len)
                                    .eq(expected_multiplicities.iter().copied())
                                && control_points
                                    .iter()
                                    .all(|point| point.u.is_finite() && point.v.is_finite())
                                && weights.as_ref().is_some_and(|weights| {
                                    rational
                                        && weights.len() == expected_controls
                                        && weights
                                            .iter()
                                            .all(|weight| weight.is_finite() && *weight > 0.0)
                                }) == rational
                        }
                        ([0x21, 0x45 | 0x71 | 0x72 | 0x91 | 0x99 | 0x9f | 0xd6 | 0xe8], _) => false,
                        ([0x21, _], None) => true,
                        _ => false,
                    };
                    let expected_ordinal = u32::try_from(support_index)
                        .ok()
                        .and_then(|index| index.checked_add(1))
                        .and_then(|offset| run.carrier_record_ordinal.checked_add(offset));
                    support.tag[0] == 0x21
                        && zero_entity_record(records, support.record_ordinal).is_some_and(
                            |record| {
                                record.byte_offset == support.byte_offset
                                    && record.tag == support.tag
                            },
                        )
                        && support.byte_offset > run.carrier_byte_offset
                        && Some(support.record_ordinal) == expected_ordinal
                        && (support_index == 0
                            || run.supports[support_index - 1].byte_offset < support.byte_offset)
                        && endpoints_valid
                        && pcurve_valid
                        && model_curve_valid
                        && model_curve_construction_valid
                        && model_parameters_valid
                        && support.model_midpoint.is_some() == has_pcurve
                        && model_midpoint_valid
                        && model_endpoints_valid
                });
        if run.id != format!("catia:zero-entity:support-run#{index}")
            || !supports_valid
            || !face_roster_valid
            || !loop_roster_valid
            || !face_valid
            || run.carrier_record_ordinal == 0
            || zero_entity_record(records, run.carrier_record_ordinal)
                .is_none_or(|record| record.byte_offset != run.carrier_byte_offset)
            || index > 0
                && (runs[index - 1].carrier_byte_offset >= run.carrier_byte_offset
                    || runs[index - 1].carrier_record_ordinal >= run.carrier_record_ordinal)
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "zero-entity support run `{}` is structurally invalid",
                run.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_zero_entity_model_curve_construction(
    carrier_tag: Option<[u8; 2]>,
    model_curve: Option<&cadmpeg_ir::geometry::CurveGeometry>,
    construction: Option<&cadmpeg_ir::geometry::ProceduralCurveDefinition>,
) -> bool {
    let finite_vector = |vector: &cadmpeg_ir::math::Vector3| {
        [vector.x, vector.y, vector.z]
            .into_iter()
            .all(f64::is_finite)
            && vector.x.hypot(vector.y).hypot(vector.z) > 0.0
    };
    let norm = |vector: &cadmpeg_ir::math::Vector3| vector.x.hypot(vector.y).hypot(vector.z);
    let normalized_dot = |left: &cadmpeg_ir::math::Vector3, right: &cadmpeg_ir::math::Vector3| {
        (left.x * right.x + left.y * right.y + left.z * right.z) / (norm(left) * norm(right))
    };
    match (carrier_tag, model_curve, construction) {
        (
            Some([0x29, 0xb8]),
            None,
            Some(cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
                angle_range,
                center,
                major,
                minor,
                pitch,
                apex_factor,
                axis,
            }),
        ) => {
            angle_range.iter().copied().all(f64::is_finite)
                && angle_range[0] < angle_range[1]
                && [center.x, center.y, center.z]
                    .into_iter()
                    .all(f64::is_finite)
                && finite_vector(major)
                && finite_vector(minor)
                && [pitch.x, pitch.y, pitch.z].into_iter().all(f64::is_finite)
                && apex_factor.is_finite()
                && finite_vector(axis)
                && (norm(axis) - 1.0).abs() <= 1e-9
                && (norm(major) - norm(minor)).abs() <= 1e-9 * norm(major).max(norm(minor))
                && normalized_dot(major, minor).abs() <= 1e-9
                && normalized_dot(major, axis).abs() <= 1e-9
                && normalized_dot(minor, axis).abs() <= 1e-9
                && (pitch.x == 0.0 && pitch.y == 0.0 && pitch.z == 0.0
                    || normalized_dot(pitch, axis).abs() >= 1.0 - 1e-9)
                && {
                    let handed_minor = cadmpeg_ir::math::Vector3::new(
                        axis.y * major.z - axis.z * major.y,
                        axis.z * major.x - axis.x * major.z,
                        axis.x * major.y - axis.y * major.x,
                    );
                    normalized_dot(&handed_minor, minor) >= 1.0 - 1e-9
                }
        }
        (_, Some(_), None)
        | (Some([0x28, 0x8a] | [0x29, 0xb8] | [0x2b, 0xc8] | [0x34, 0xc8 | 0x5e]), None, None) => {
            true
        }
        _ => false,
    }
}

#[cfg(test)]
fn validate_zero_entity_model_curve(
    carrier_tag: Option<[u8; 2]>,
    curve: Option<&cadmpeg_ir::geometry::CurveGeometry>,
) -> bool {
    use cadmpeg_ir::geometry::CurveGeometry;

    let finite_point = |point: &cadmpeg_ir::math::Point3| {
        [point.x, point.y, point.z].into_iter().all(f64::is_finite)
    };
    let finite_vector = |vector: &cadmpeg_ir::math::Vector3| {
        [vector.x, vector.y, vector.z]
            .into_iter()
            .all(f64::is_finite)
            && vector.x.hypot(vector.y).hypot(vector.z) > 0.0
    };
    match (carrier_tag, curve) {
        (Some([0x27, 0x6a] | [0x34, 0xc8 | 0x5e]), Some(CurveGeometry::Nurbs(curve))) => {
            let Ok(degree) = usize::try_from(curve.degree) else {
                return false;
            };
            curve.control_points.len() > degree
                && curve.knots.len() == curve.control_points.len() + degree + 1
                && curve.knots.iter().all(|knot| knot.is_finite())
                && curve.knots.windows(2).all(|pair| pair[0] <= pair[1])
                && curve.control_points.iter().all(finite_point)
                && curve.weights.as_ref().is_none_or(|weights| {
                    weights.len() == curve.control_points.len()
                        && weights
                            .iter()
                            .all(|weight| weight.is_finite() && *weight > 0.0)
                })
                && !curve.periodic
        }
        (Some([0x28, 0x8a] | [0x29, 0xb8]), Some(CurveGeometry::Line { origin, direction })) => {
            finite_point(origin) && finite_vector(direction)
        }
        (
            Some([0x28, 0x8a] | [0x29, 0xb8] | [0x2b, 0xc8]),
            Some(CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            }),
        ) => {
            finite_point(center)
                && finite_vector(axis)
                && finite_vector(ref_direction)
                && radius.is_finite()
                && *radius > 0.0
        }
        (Some([0x28, 0x8a] | [0x29, 0xb8] | [0x2b, 0xc8] | [0x34, 0xc8 | 0x5e]), None) => true,
        _ => false,
    }
}

#[cfg(test)]
fn validate_zero_entity_endpoint_pair_candidates(
    endpoint_pairs: &[CatiaZeroEntityEndpointPairCandidate],
    runs: &[CatiaZeroEntitySupportRun],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let expected = zero_entity_endpoint_pair_candidates(derived_zero_entity_endpoint_pairs(runs));
    if endpoint_pairs != expected {
        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "zero-entity endpoint-pair candidates disagree with their radial support occurrences"
                .to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
fn derived_zero_entity_endpoint_pairs(
    runs: &[CatiaZeroEntitySupportRun],
) -> Vec<crate::families::zero_entity::topology::ZeroEntityEndpointPairCandidate> {
    let mut occurrences = Vec::new();
    for run in runs {
        let Some(face) = run.face.as_ref() else {
            continue;
        };
        let midpoints = run
            .supports
            .iter()
            .filter_map(|support| Some((support.record_ordinal, support.model_midpoint?)))
            .collect::<std::collections::HashMap<_, _>>();
        for loop_record in &face.loops {
            for (support_record_ordinal, model_endpoints) in loop_record
                .support_record_ordinals
                .iter()
                .copied()
                .zip(loop_record.oriented_model_endpoints.iter().copied())
            {
                let Some(model_midpoint) = midpoints.get(&support_record_ordinal).copied() else {
                    continue;
                };
                occurrences.push(
                    crate::families::zero_entity::topology::ZeroEntityOrientedOccurrence {
                        face_record_ordinal: face.record_ordinal,
                        support_record_ordinal,
                        model_endpoints,
                        model_midpoint,
                    },
                );
            }
        }
    }
    crate::families::zero_entity::topology::endpoint_pair_candidates(&occurrences)
}

#[cfg(test)]
fn validate_zero_entity_endpoint_locus_candidates(
    endpoint_loci: &[CatiaZeroEntityEndpointLocusCandidate],
    endpoint_pairs: &[CatiaZeroEntityEndpointPairCandidate],
    runs: &[CatiaZeroEntitySupportRun],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let derived_pairs = derived_zero_entity_endpoint_pairs(runs);
    let expected = zero_entity_endpoint_locus_candidates(
        crate::families::zero_entity::topology::endpoint_locus_candidates(&derived_pairs),
        endpoint_pairs,
    );
    if endpoint_loci != expected {
        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "zero-entity endpoint-locus candidates disagree with their endpoint-pair endpoints"
                .to_string(),
        ));
    }
    Ok(())
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

#[cfg(test)]
fn validate_zero_entity_records(
    records: &[CatiaZeroEntityRecord],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let valid = records.iter().enumerate().all(|(index, record)| {
        let ordinal = u32::try_from(index)
            .ok()
            .and_then(|index| index.checked_add(1));
        Some(record.record_ordinal) == ordinal
            && record.id == format!("catia:zero-entity:record#{}", record.record_ordinal)
            && record.logical_end > record.byte_offset
            && (index == 0 || records[index - 1].logical_end <= record.byte_offset)
    });
    if valid {
        Ok(())
    } else {
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "zero-entity record namespace is structurally invalid".to_string(),
        ))
    }
}

#[cfg(test)]
fn validate_zero_entity_ownership_roots(
    roots: &[CatiaZeroEntityOwnershipRoot],
    support_runs: &[CatiaZeroEntitySupportRun],
    records: &[CatiaZeroEntityRecord],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let bound_face_count = support_runs.iter().filter(|run| run.face.is_some()).count();
    let valid = roots.len() <= 1
        && roots.iter().all(|root| {
            root.id == "catia:zero-entity:ownership-root#0"
                && root.face_slots.len() == bound_face_count
                && root
                    .face_slots
                    .iter()
                    .copied()
                    .eq((1..=u32::try_from(bound_face_count).unwrap_or(0)).rev())
                && [
                    (
                        root.face_roster_record_ordinal,
                        root.face_roster_byte_offset,
                        [0x61, 0x42],
                    ),
                    (
                        root.shell_record_ordinal,
                        root.shell_byte_offset,
                        [0x60, 0x06],
                    ),
                    (
                        root.body_record_ordinal,
                        root.body_byte_offset,
                        [0x65, 0x08],
                    ),
                ]
                .into_iter()
                .all(|(ordinal, byte_offset, tag)| {
                    zero_entity_record(records, ordinal).is_some_and(|record| {
                        record.byte_offset == byte_offset && record.tag == tag
                    })
                })
                && root.shell_record_ordinal == root.face_roster_record_ordinal.saturating_add(1)
                && root.body_record_ordinal == root.shell_record_ordinal.saturating_add(1)
        });
    if valid {
        Ok(())
    } else {
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "zero-entity ownership root is structurally invalid".to_string(),
        ))
    }
}

#[cfg(test)]
fn validate_zero_entity_topology_records(
    edge_strides: &[CatiaZeroEntityEdgeStride],
    oriented_use_pairs: &[CatiaZeroEntityOrientedUsePair],
    vertex_incidences: &[CatiaZeroEntityVertexIncidence],
    records: &[CatiaZeroEntityRecord],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let edge_strides_valid = edge_strides.iter().enumerate().all(|(index, record)| {
        record.id == format!("catia:zero-entity:edge-stride#{index}")
            && record.record_ordinal != 0
            && !record.allocations.contains(&0)
            && record.allocations[0].checked_sub(1) == Some(record.allocations[3])
            && record.allocations[0].checked_sub(2) == Some(record.allocations[4])
            && zero_entity_record(records, record.record_ordinal).is_some_and(|source| {
                source.byte_offset == record.byte_offset && source.tag == [0x5e, 0x1a]
            })
            && (index == 0
                || edge_strides[index - 1].byte_offset < record.byte_offset
                    && edge_strides[index - 1].record_ordinal < record.record_ordinal)
    });
    let pairs_valid = oriented_use_pairs.iter().enumerate().all(|(index, pair)| {
        pair.id == format!("catia:zero-entity:oriented-use-pair#{index}")
            && pair.header_record_ordinal != 0
            && zero_entity_record(records, pair.header_record_ordinal).is_some_and(|source| {
                source.byte_offset == pair.header_byte_offset && source.tag == [0x25, 0x69]
            })
            && (index == 0
                || oriented_use_pairs[index - 1].header_byte_offset < pair.header_byte_offset
                    && oriented_use_pairs[index - 1].header_record_ordinal
                        < pair.header_record_ordinal)
            && pair.uses.iter().enumerate().all(|(use_index, use_)| {
                let side = use_index as u32 + 1;
                use_.side == side
                    && !use_.allocations.contains(&0)
                    && zero_entity_record(records, use_.record_ordinal).is_some_and(|source| {
                        source.byte_offset == use_.byte_offset && source.tag == [0x06, 0x38]
                    })
                    && use_.byte_offset > pair.header_byte_offset
                    && (use_index == 0 || pair.uses[use_index - 1].byte_offset < use_.byte_offset)
                    && use_.record_ordinal == pair.header_record_ordinal.saturating_add(side)
                    && use_.allocations
                        == [
                            pair.base_columns[0].saturating_add(side),
                            pair.base_columns[1].saturating_add(side),
                        ]
            })
    });
    let incidences_valid = vertex_incidences.iter().enumerate().all(|(index, record)| {
        let expected_count = match record.tag {
            [0x05, 0x0b] => 2,
            [0x05, 0x10] => 3,
            [0x05, 0x15] => 4,
            _ => return false,
        };
        record.id == format!("catia:zero-entity:vertex-incidence#{index}")
            && record.record_ordinal != 0
            && !record.allocations.contains(&0)
            && zero_entity_record(records, record.record_ordinal).is_some_and(|source| {
                source.byte_offset == record.byte_offset && source.tag == record.tag
            })
            && record.allocations.len() == expected_count
            && record.vertex_record.as_deref()
                == zero_entity_vertex_owner(records, record.record_ordinal)
                    .map(|owner| owner.id.as_str())
            && (index == 0
                || vertex_incidences[index - 1].byte_offset < record.byte_offset
                    && vertex_incidences[index - 1].record_ordinal < record.record_ordinal)
    });
    if edge_strides_valid && pairs_valid && incidences_valid {
        Ok(())
    } else {
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "zero-entity topology records are structurally invalid".to_string(),
        ))
    }
}

#[cfg(test)]
fn validate_consolidated_owner_packets(
    packets: &[CatiaConsolidatedOwnerPacket],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, packet) in packets.iter().enumerate() {
        let valid_link = packet.allocation_link.is_none_or(|link| {
            link.byte_offset.checked_add(link.byte_len) == Some(packet.byte_offset)
                && link.target.checked_add(1) == packet.payload.final_reference()
        });
        let valid_payload = match &packet.payload {
            CatiaOwnerPacketPayload::FixedNine { numeric_tail, .. } => {
                numeric_tail.header[0] == 0x84
                    && matches!(numeric_tail.header[1], 0x41 | 0xc1)
                    && numeric_tail.header[4] == 0x0d
                    && numeric_tail.lower.iter().all(|value| value.is_finite())
                    && numeric_tail.upper.iter().all(|value| value.is_finite())
                    && numeric_tail.lower[0] < numeric_tail.upper[0]
                    && numeric_tail.lower[1] < numeric_tail.upper[1]
                    && numeric_tail.bounds.iter().all(|bounds| {
                        bounds[0].is_finite() && bounds[1].is_finite() && bounds[0] < bounds[1]
                    })
            }
            CatiaOwnerPacketPayload::Counted { references, tail } => {
                !references.is_empty() && !tail.is_empty()
            }
        };
        if packet.id != format!("catia:consolidated:owner-packet#{:010}", packet.byte_offset)
            || !valid_payload
            || !valid_link
            || index > 0 && packets[index - 1].byte_offset >= packet.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated owner packet `{}` is structurally invalid",
                packet.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
struct ConsolidatedSupportArenas<'a> {
    circles: &'a [CatiaConsolidatedCircle],
    cones: &'a [CatiaConsolidatedCone],
    cylinders: &'a [CatiaConsolidatedCylinder],
    embedded_cylinders: &'a [CatiaConsolidatedEmbeddedCylinder],
    groups: &'a [CatiaConsolidatedGroup],
}

#[cfg(test)]
fn validate_consolidated_edge_runs(
    runs: &[CatiaConsolidatedEdgeRun],
    pcurves: &[CatiaConsolidatedPcurve],
    supports: &ConsolidatedSupportArenas<'_>,
    nodes: &[CatiaConsolidatedEdgeNode],
    vertex_identities: &[CatiaConsolidatedVertexIdentity],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let pcurves = pcurves
        .iter()
        .map(|pcurve| (pcurve.id.as_str(), pcurve))
        .collect::<HashMap<_, _>>();
    let nodes_by_id = nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    let circles = supports
        .circles
        .iter()
        .map(|circle| (circle.id.as_str(), circle))
        .collect::<HashMap<_, _>>();
    let circle_offsets = circles
        .values()
        .map(|circle| circle.byte_offset)
        .collect::<HashSet<_>>();
    let cone_offsets = supports
        .cones
        .iter()
        .map(|cone| cone.byte_offset)
        .collect::<HashSet<_>>();
    let cylinder_offsets = supports
        .cylinders
        .iter()
        .map(|cylinder| cylinder.byte_offset)
        .collect::<HashSet<_>>();
    let group_offsets = supports
        .groups
        .iter()
        .map(|group| (group.id.as_str(), group.byte_offset))
        .collect::<HashMap<_, _>>();
    let embedded_cylinder_offsets = supports
        .embedded_cylinders
        .iter()
        .filter_map(|cylinder| {
            Some((
                cylinder.byte_offset,
                *group_offsets.get(cylinder.group.as_str())?,
            ))
        })
        .collect::<HashSet<_>>();
    let mut run_nodes = HashSet::new();
    for (index, node) in nodes.iter().enumerate() {
        let token_limit = 1u32.checked_shl(u32::from(node.width) * 8);
        let uses_valid = node.uses.as_ref().is_none_or(|uses| {
            node.curve_ref
                .checked_sub(2)
                .zip(node.curve_ref.checked_sub(1))
                .is_some_and(|(first, second)| {
                    uses.references == [[first, second], [second, node.curve_ref]]
                })
                && uses.senses == [0x88, 0x84]
                && node.parameter_selectors == [2, 1]
        });
        let definition_valid = node.definition.as_ref().is_none_or(|definition| {
            let token_limit = 1u32.checked_shl(u32::from(definition.width) * 8);
            let expected_data =
                crate::families::consolidated::records::consolidated_edge_definition_data(
                    definition.class,
                    &definition.payload,
                );
            node.uses.is_some()
                && matches!(definition.width, 1..=3)
                && matches!(definition.flag, 0x03 | 0x13 | 0x83)
                && matches!(definition.class, 0x23..=0x25)
                && token_limit.is_some_and(|limit| definition.header_token < limit)
                && !definition.payload.is_empty()
                && definition.byte_offset < node.byte_offset
                && definition.data == expected_data
        });
        let analytic_circle_valid = node.analytic_circle.as_ref().is_none_or(|binding| {
            let definition = node.definition.as_ref();
            let circle = circles.get(binding.circle.as_str());
            node.uses.is_some()
                && definition.is_some_and(|definition| {
                    definition.class == 0x23
                        && matches!(
                            definition.data,
                            Some(ConsolidatedEdgeDefinitionData::Scalar {
                                ref values,
                                ..
                            }) if values.len() == 8
                        )
                        && circle.is_some_and(|circle| {
                            binding.descriptor.byte_offset < circle.byte_offset
                                && circle.byte_offset < definition.byte_offset
                        })
                })
                && matches!(binding.descriptor.width, 1..=3)
                && matches!(binding.descriptor.flag, 0x03 | 0x13 | 0x83)
                && 1u32
                    .checked_shl(u32::from(binding.descriptor.width) * 8)
                    .is_some_and(|limit| binding.descriptor.header_token < limit)
                && !binding.descriptor.payload.is_empty()
        });
        let class25_descriptor_valid = node.class25_descriptor.as_ref().is_none_or(|descriptor| {
            node.uses.is_some()
                && node.definition.as_ref().is_some_and(|definition| {
                    definition.class == 0x25
                        && matches!(
                            definition.data,
                            Some(
                                ConsolidatedEdgeDefinitionData::Scalar25 { .. }
                                    | ConsolidatedEdgeDefinitionData::SegmentedScalar25 { .. }
                            )
                        )
                        && descriptor.byte_offset < definition.byte_offset
                })
                && matches!(descriptor.control, 0x02 | 0x0a)
                && matches!(descriptor.values.len(), 2 | 3)
                && descriptor.values.iter().all(|value| value.is_finite())
        });
        if node.id != format!("catia:consolidated:edge-node#{index}")
            || !matches!(node.width, 1..=3)
            || !matches!(node.flag, 0x03 | 0x13 | 0x83)
            || token_limit.is_some_and(|limit| node.header_token >= limit)
            || !uses_valid
            || !definition_valid
            || !analytic_circle_valid
            || !class25_descriptor_valid
            || index > 0 && nodes[index - 1].byte_offset >= node.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge node `{}` is structurally invalid",
                node.id
            )));
        }
    }
    for (index, run) in runs.iter().enumerate() {
        let expected_id = format!("catia:consolidated:edge-run#{index}");
        let pcurve_offsets = run
            .pcurves
            .each_ref()
            .map(|id| pcurves.get(id.as_str()).map(|pcurve| pcurve.byte_offset));
        let pcurve_ranges = run
            .pcurves
            .each_ref()
            .map(|id| pcurves.get(id.as_str()).map(|pcurve| pcurve.range));
        let Some(node) = nodes_by_id.get(run.node.as_str()) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge run `{}` references missing node `{}`",
                run.id, run.node
            )));
        };
        if !run_nodes.insert(run.node.as_str()) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge node `{}` belongs to multiple runs",
                run.node
            )));
        }
        let loci_valid = run.shared_loci.as_ref().map_or_else(
            || run.endpoint_loci.is_none(),
            |loci| {
                loci.len() >= 2
                    && loci.iter().flatten().all(|value| value.is_finite())
                    && run.endpoint_loci
                        == loci
                            .first()
                            .copied()
                            .zip(loci.last().copied())
                            .map(|(first, last)| [first, last])
            },
        );
        let bindings_valid = run
            .support_bindings
            .iter()
            .flatten()
            .all(|binding| match binding {
                CatiaConsolidatedSupportBinding::Cylinder { byte_offset } => {
                    cylinder_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::EmbeddedCylinder {
                    byte_offset,
                    wrapper_byte_offset,
                } => embedded_cylinder_offsets.contains(&(*byte_offset, *wrapper_byte_offset)),
                CatiaConsolidatedSupportBinding::Circle { byte_offset } => {
                    circle_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::Cone { byte_offset } => {
                    cone_offsets.contains(byte_offset)
                }
                CatiaConsolidatedSupportBinding::NurbsCarrier { offset, .. } => offset.is_finite(),
            });
        if run.id != expected_id
            || pcurve_offsets[0] != Some(run.byte_offset)
            || pcurve_offsets[1].is_none()
            || pcurve_offsets[0] >= pcurve_offsets[1]
            || pcurve_offsets[1].is_some_and(|offset| offset >= node.byte_offset)
            || pcurve_ranges != [Some(run.parameter_range), Some(run.parameter_range)]
            || run.parameter_range[0] >= run.parameter_range[1]
            || !run.parameter_range.iter().all(|value| value.is_finite())
            || !run.tolerance.is_finite()
            || run.tolerance < 0.0
            || node.uses.is_none()
            || !matches!(node.tail, 0x01 | 0x21)
            || !bindings_valid
            || !loci_valid
            || index > 0 && runs[index - 1].byte_offset >= run.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge run `{}` is structurally invalid",
                run.id
            )));
        }
    }
    let mut expected_nodes = nodes.to_vec();
    let expected_identities = consolidated_vertex_identities(&mut expected_nodes);
    if expected_nodes != nodes || expected_identities != vertex_identities {
        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "consolidated vertex identities disagree with edge incidence".to_string(),
        ));
    }
    Ok(())
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

#[cfg(test)]
fn validate_native_links(
    aliases: &[CatiaAliasRow],
    catalogs: &[CatiaCatalog],
    graphs: &[CatiaObjectGraph],
    segments: &[CatiaFinjplSegment],
    value_blocks: &[CatiaValueBlock],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for catalog in catalogs {
        let count_width = if catalog.declared_count <= 0x50 { 1 } else { 2 };
        let Some(mut expected_offset) = catalog.byte_offset.checked_add(6 + count_width) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "catalog `{}` has an overflowing extent",
                catalog.id
            )));
        };
        let catalog_end = catalog.byte_offset.checked_add(catalog.byte_len);
        if catalog.id != format!("catia:outer:catalog#{:010}", catalog.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "catalog `{}` has an invalid source identity",
                catalog.id
            )));
        }
        for (index, entry) in catalog.entries.iter().enumerate() {
            let next_offset = catalog
                .entries
                .get(index + 1)
                .map(|next| next.byte_offset)
                .or(catalog_end);
            let encoded_len = next_offset.and_then(|next| next.checked_sub(entry.byte_offset));
            let value_len = u64::try_from(entry.value.len()).ok();
            if entry.byte_offset != expected_offset
                || entry.id != format!("catia:outer:catalog-entry#{:010}", entry.byte_offset)
                || !encoded_len.zip(value_len).is_some_and(|(encoded, value)| {
                    matches!(encoded.checked_sub(value), Some(1 | 5))
                })
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "catalog entry `{}` has an invalid source extent",
                    entry.id
                )));
            }
            expected_offset = next_offset.expect("validated catalog end");
        }
        if Some(expected_offset) != catalog_end {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "catalog `{}` entries do not cover its frame",
                catalog.id
            )));
        }
    }
    for (index, segment) in segments.iter().enumerate() {
        let parsed = container::finjpl_segments(&segment.data, 0, segment.data.len());
        let expected_id = format!("catia:outer:finjpl#{index}");
        if segment.id != expected_id
            || u64::try_from(segment.data.len()).ok() != Some(segment.byte_len)
            || segment.byte_offset.checked_add(segment.byte_len).is_none()
            || !matches!(parsed.as_slice(), [parsed]
                if parsed.range == (0..segment.data.len())
                    && parsed.type_word == segment.type_word
                    && finjpl_family(parsed.kind) == segment.family
                    && parsed.name == segment.name)
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "FINJPL segment `{}` has an invalid retained view",
                segment.id
            )));
        }
    }
    if segments
        .windows(2)
        .any(|pair| pair[0].byte_offset.checked_add(pair[0].byte_len) != Some(pair[1].byte_offset))
    {
        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "CATIA FINJPL segment extents are not contiguous".to_string(),
        ));
    }
    for block in value_blocks {
        if block.id != format!("catia:outer:value-block#{:010}", block.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` has an invalid source identity",
                block.id
            )));
        }
        let Some(catalog) = catalogs.iter().find(|catalog| catalog.id == block.catalog) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` references missing catalog `{}`",
                block.id, block.catalog
            )));
        };
        if block.byte_offset.checked_add(block.byte_len) != Some(catalog.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` is not adjacent to catalog `{}`",
                block.id, block.catalog
            )));
        }
        let payload_len = u64::try_from(block.payload.len()).ok();
        if block.declared_len.checked_add(1) != Some(block.byte_len)
            || payload_len.and_then(|len| len.checked_add(6)) != Some(block.declared_len)
            || value_block::tokenize(&block.payload) != block.fields
            || value_schema_selections(&block.id, block.byte_offset, &block.fields, catalog)
                != block.schema_selections
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` has an invalid derived view",
                block.id
            )));
        }
        let mut adjacent_graphs = graphs.iter().filter(|graph| {
            graph.byte_offset.checked_add(graph.byte_len) == Some(block.byte_offset)
        });
        let adjacent_graph = adjacent_graphs.next();
        if adjacent_graphs.next().is_some()
            || block.object_graph.as_deref() != adjacent_graph.map(|graph| graph.id.as_str())
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` has an invalid adjacent graph link",
                block.id
            )));
        }
    }
    for graph in graphs {
        let Some(graph_end) = graph.byte_offset.checked_add(graph.byte_len) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` has an overflowing extent",
                graph.id
            )));
        };
        let mut expected_record_offset = graph.byte_offset.checked_add(6);
        if graph.id != format!("catia:outer:object-graph#{:010}", graph.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` has an invalid source identity",
                graph.id
            )));
        }
        if graph.finjpl_segment.as_deref()
            != containing_finjpl_segment(graph.byte_offset, graph.byte_len, segments)
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` has an invalid FINJPL segment link",
                graph.id
            )));
        }
        for record in &graph.records {
            if Some(record.byte_offset) != expected_record_offset
                || record.id != format!("catia:outer:object-record#{:010}", record.byte_offset)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "object record `{}` has an invalid source extent",
                    record.id
                )));
            }
            expected_record_offset = record.byte_offset.checked_add(record.byte_len);
        }
        if expected_record_offset != Some(graph_end) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` records do not cover its frame",
                graph.id
            )));
        }
        let mut candidates = catalogs
            .iter()
            .filter(|catalog| catalog.byte_offset == graph_end)
            .chain(
                value_blocks
                    .iter()
                    .filter(|block| block.byte_offset == graph_end)
                    .filter_map(|block| {
                        catalogs.iter().find(|catalog| catalog.id == block.catalog)
                    }),
            );
        let catalog = candidates.next();
        if candidates.next().is_some()
            || graph.catalog_byte_offset != catalog.map(|catalog| catalog.byte_offset)
            || graph.catalog.as_deref() != catalog.map(|catalog| catalog.id.as_str())
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` has an invalid schema-catalog link",
                graph.id
            )));
        }
        for record in &graph.records {
            let expected_class = catalog.and_then(|catalog| {
                usize::try_from(record.class_ref?).ok().and_then(|ordinal| {
                    catalog
                        .entries
                        .get(ordinal)
                        .map(|entry| (entry.id.as_str(), entry.value.as_str()))
                })
            });
            if record.class_entry.as_deref() != expected_class.map(|(entry, _)| entry)
                || record.class_name.as_deref() != expected_class.map(|(_, value)| value)
                || record.repeated_reference_schema_selection
                    != repeated_reference_schema_selection(
                        record.repeated_reference_suffix.as_ref(),
                        catalog,
                    )
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "object record `{}` has an invalid schema class",
                    record.id
                )));
            }
        }
    }
    let declared_containers = graphs.iter().any(|graph| graph.outer_container.is_some());
    let maximum_records = graphs
        .iter()
        .map(|graph| graph.records.len())
        .max()
        .unwrap_or(0);
    let mut primary_graphs = graphs.iter().filter(|graph| {
        if declared_containers {
            graph
                .outer_container
                .as_ref()
                .is_some_and(|container| container.class_name == "CATPrtCont")
        } else {
            graph.records.len() == maximum_records
        }
    });
    let primary_graph = match (primary_graphs.next(), primary_graphs.next()) {
        (Some(graph), None) => Some(graph),
        _ => None,
    };
    for alias in aliases {
        if alias.id != format!("catia:outer:alias-row#{:010}", alias.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "alias row `{}` has an invalid source identity",
                alias.id
            )));
        }
        let expected = usize::from(alias.entity_record_ordinal)
            .checked_sub(1)
            .and_then(|index| {
                let graph = primary_graph?;
                let record = graph.records.get(index)?;
                Some((
                    graph.id.as_str(),
                    record.id.as_str(),
                    record.design_object.as_deref(),
                ))
            });
        let valid = expected.map_or_else(
            || {
                alias.object_graph.is_none()
                    && alias.object_record.is_none()
                    && alias.design_object.is_none()
            },
            |(graph, record, object)| {
                alias.object_graph.as_deref() == Some(graph)
                    && alias.object_record.as_deref() == Some(record)
                    && alias.design_object.as_deref() == object
            },
        );
        if !valid {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "alias row `{}` has invalid graph, record, or design-object links",
                alias.id
            )));
        }
        if let Some(group) = &alias.group {
            if group.target_slot != (u32::from(alias.f1[2]) | ((alias.f2 & 0x00ff_ffff) << 8))
                || !object_graph::is_alias_group_storage_prefix(&group.storage_prefix)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "alias row `{}` has invalid group storage",
                    alias.id
                )));
            }
        }
    }
    Ok(())
}

impl CatiaNative {
    /// Decode CATIA-native records directly from the complete file image.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Self {
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
        let mut alias_rows = object_graph::surface_aliases(bytes)
            .into_iter()
            .map(CatiaAliasRow::from)
            .collect::<Vec<_>>();
        let mut parsed_object_graphs = object_graph::parse_all(bytes);
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
        for entity in &mut entity_records {
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
                &entities_by_graph_identity,
                &entity_classes_by_graph_identity,
                &terminal_nulls_by_graph,
                &relation_expression_entities,
            );
            entity.configuration_record = configuration_record(
                entity.entity_id,
                object,
                &entity.value_schema_selections,
                &entities_by_graph_identity,
                &entity_classes_by_graph_identity,
                &terminal_nulls_by_graph,
            );
            entity.configuration_row_link = configuration_row_link(
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
                &CatiaEntityReferenceIndex {
                    entities: &entities_by_graph_identity,
                    classes: &entity_classes_by_graph_identity,
                    terminal_nulls: &terminal_nulls_by_graph,
                },
                &parameter_bindings,
            );
        }
        let configuration_row_chains = derive_configuration_row_chains(
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
        let fragment_primary_graph = outer_container_declarations.is_empty().then(|| {
            let maximum_records = object_graphs
                .iter()
                .map(|graph| graph.records.len())
                .max()
                .unwrap_or(0);
            let mut graphs = object_graphs
                .iter()
                .filter(|graph| graph.records.len() == maximum_records);
            let graph = graphs.next()?;
            graphs.next().is_none().then_some(graph)
        });
        if let Some(graph) = part_graph.or(fragment_primary_graph.flatten()) {
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
        let consolidated_circles = consolidated_circles(bytes);
        let consolidated_class61_records = consolidated_class61_records(bytes);
        let consolidated_parameter_points = consolidated_parameter_points(bytes);
        let consolidated_cone_faces =
            consolidated_cone_faces(bytes, &consolidated_parameter_points);
        let consolidated_cones = consolidated_cones(bytes);
        let consolidated_cylinders = consolidated_cylinders(bytes);
        let consolidated_groups = consolidated_groups(bytes);
        let consolidated_embedded_cylinders =
            consolidated_embedded_cylinders(bytes, &consolidated_groups);
        let consolidated_line_profiles = consolidated_line_profiles(bytes);
        let consolidated_owner_packets = consolidated_owner_packets(bytes);
        let consolidated_pcurves = consolidated_pcurves(bytes);
        let consolidated_reference_lists = consolidated_reference_lists(bytes);
        let consolidated_revolutions = consolidated_revolutions(bytes, &consolidated_circles);
        let consolidated_spheres = consolidated_spheres(bytes);
        let consolidated_tori = consolidated_tori(bytes);
        let zero_entity_records = zero_entity_records(bytes);
        let zero_entity_edge_strides = zero_entity_edge_strides(bytes);
        let zero_entity_oriented_use_pairs = zero_entity_oriented_use_pairs(bytes);
        let zero_entity_ownership_roots = zero_entity_ownership_roots(bytes);
        let parsed_zero_entity_support_runs =
            crate::families::zero_entity::records::zero_entity_support_runs(bytes);
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
            zero_entity_vertex_incidences(bytes, &zero_entity_records);
        let mut consolidated_edge_nodes = consolidated_edge_nodes(bytes, &consolidated_circles);
        let consolidated_edge_runs =
            consolidated_edge_runs(bytes, &consolidated_pcurves, &consolidated_edge_nodes);
        let consolidated_vertex_identities =
            consolidated_vertex_identities(&mut consolidated_edge_nodes);
        Self {
            version: CATIA_NATIVE_VERSION,
            alias_rows,
            catalogs,
            consolidated_circles,
            consolidated_class61_records,
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
            consolidated_pcurves,
            consolidated_reference_lists,
            consolidated_revolutions,
            consolidated_spheres,
            consolidated_tori,
            consolidated_vertex_identities,
            configuration_row_chains,
            design_objects,
            entity_records,
            external_references,
            finjpl_segments,
            legacy_entity_runs,
            object_graphs,
            preview_images,
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

    /// Load the typed CATIA namespace from generic native arenas.
    #[cfg(test)]
    pub fn load(
        namespace: &cadmpeg_ir::NativeNamespace,
    ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
        let mut catalogs: Vec<CatiaCatalog> = namespace.arena_as("catalogs")?;
        let entries: Vec<CatiaCatalogEntry> = namespace.arena_as("catalog_entries")?;
        let catalog_ids = catalogs
            .iter()
            .map(|catalog| catalog.id.as_str())
            .collect::<HashSet<_>>();
        if catalog_ids.len() != catalogs.len() {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "duplicate CATIA catalog identity".to_string(),
            ));
        }
        if let Some(entry) = entries
            .iter()
            .find(|entry| !catalog_ids.contains(entry.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "catalog entry `{}` references missing catalog `{}`",
                entry.id, entry.parent
            )));
        }
        for catalog in &mut catalogs {
            catalog.entries = entries
                .iter()
                .filter(|entry| entry.parent == catalog.id)
                .cloned()
                .collect();
            catalog.entries.sort_by_key(|entry| entry.ordinal);
            if u32::try_from(catalog.entries.len())
                .ok()
                .and_then(|count| count.checked_add(1))
                != Some(catalog.declared_count)
                || catalog
                    .entries
                    .iter()
                    .enumerate()
                    .any(|(ordinal, entry)| usize::try_from(entry.ordinal).ok() != Some(ordinal))
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "catalog `{}` has an invalid entry sequence",
                    catalog.id
                )));
            }
        }
        let mut graphs: Vec<CatiaObjectGraph> = namespace.arena_as("object_graphs")?;
        let mut records: Vec<CatiaObjectRecord> = namespace.arena_as("object_graph_records")?;
        if namespace.version < CATIA_TYPED_OWNER_SLOT_VERSION {
            for record in &mut records {
                let roles = object_graph::head_roles(record.lead, &record.head);
                record.owner = roles
                    .owner_ref
                    .map(CatiaObjectOwner::Entity)
                    .or_else(|| roles.owner_literal.map(CatiaObjectOwner::UnassignedLiteral));
            }
        }
        let mut entity_records: Vec<CatiaEntityRecord> = namespace.arena_as("entity_records")?;
        let mut configuration_row_chains: Vec<CatiaConfigurationRowChain> =
            namespace.arena_as("configuration_row_chains")?;
        if namespace.version < CATIA_SUFFIX_FRAMING_VERSION {
            for entity in &mut entity_records {
                entity.suffix_framing = entity_suffix_framing(&entity.record_suffix);
            }
        }
        let graph_ids = graphs
            .iter()
            .map(|graph| graph.id.as_str())
            .collect::<HashSet<_>>();
        if graph_ids.len() != graphs.len() {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "duplicate CATIA object-graph identity".to_string(),
            ));
        }
        if let Some(record) = records
            .iter()
            .find(|record| !graph_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object record `{}` references missing graph `{}`",
                record.id, record.parent
            )));
        }
        let record_ids = records
            .iter()
            .map(|record| record.id.as_str())
            .collect::<HashSet<_>>();
        let entity_record_ids = entity_records
            .iter()
            .map(|record| record.id.as_str())
            .collect::<HashSet<_>>();
        if record_ids.len() != records.len() || entity_record_ids.len() != entity_records.len() {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "duplicate CATIA object or entity-record identity".to_string(),
            ));
        }
        if let Some(entity) = entity_records.iter().find(|entity| {
            !graph_ids.contains(entity.object_graph.as_str())
                || !record_ids.contains(entity.object_record.as_str())
        }) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "entity record `{}` has a missing graph or object-record link",
                entity.id
            )));
        }
        let entity_classes_by_graph_identity = entity_class_index(&records);
        let (
            relation_expressions,
            relation_expression_entities,
            entities_by_graph_identity,
            terminal_nulls_by_graph,
            parameter_bindings,
        ) = semantic_entity_indices(&entity_records, &entity_classes_by_graph_identity);
        if namespace.version < CATIA_TERMINAL_NULL_REFERENCE_VERSION {
            for graph in &graphs {
                let terminal_null = entity_records
                    .iter()
                    .filter(|entity| entity.object_graph == graph.id)
                    .map(|entity| entity.entity_id)
                    .max()
                    .and_then(|entity_id| entity_id.checked_add(1));
                for record in records
                    .iter_mut()
                    .filter(|record| record.parent == graph.id)
                {
                    for reference in &mut record.references {
                        reference.is_null = Some(reference.entity_id) == terminal_null;
                    }
                }
            }
        }
        if namespace.version < CATIA_FORMULA_DEPENDENCY_CANDIDATE_VERSION
            || namespace.version < CATIA_TERMINAL_NULL_REFERENCE_VERSION
            || namespace.version < CATIA_FORMULA_OUTPUT_REFERENCE_VERSION
            || namespace.version < CATIA_FORMULA_EXPRESSION_REFERENCE_VERSION
            || namespace.version < CATIA_FORMULA_DEPENDENCY_REFERENCE_VERSION
            || namespace.version < CATIA_TYPED_INCIDENCE_NULL_VERSION
        {
            let records_by_id = records
                .iter()
                .map(|record| (record.id.as_str(), record))
                .collect::<HashMap<_, _>>();
            for entity in &mut entity_records {
                entity.formula_relation = records_by_id
                    .get(entity.object_record.as_str())
                    .and_then(|object| {
                        formula_relation(
                            &entity.definition_schema_selections,
                            entity.entity_id,
                            object,
                            &relation_expressions,
                            &CatiaEntityReferenceIndex {
                                entities: &entities_by_graph_identity,
                                classes: &entity_classes_by_graph_identity,
                                terminal_nulls: &terminal_nulls_by_graph,
                            },
                            &parameter_bindings,
                        )
                    });
            }
        }
        if namespace.version < CATIA_RELATION_PROGRAM_INSTANCE_VERSION
            || namespace.version < CATIA_RELATION_PROGRAM_CONTEXT_VERSION
            || namespace.version < CATIA_TYPED_INCIDENCE_CLASS_VERSION
            || namespace.version < CATIA_RELATION_TYPED_REFERENCE_VERSION
            || namespace.version < CATIA_TYPED_INCIDENCE_NULL_VERSION
            || namespace.version < CATIA_RELATION_PROGRAM_REFERENCE_INCIDENCE_VERSION
        {
            let records_by_id = records
                .iter()
                .map(|record| (record.id.as_str(), record))
                .collect::<HashMap<_, _>>();
            for entity in &mut entity_records {
                entity.relation_program_instance = records_by_id
                    .get(entity.object_record.as_str())
                    .and_then(|object| {
                        relation_program_instance(
                            entity.entity_id,
                            object,
                            &entities_by_graph_identity,
                            &entity_classes_by_graph_identity,
                            &terminal_nulls_by_graph,
                            &relation_expression_entities,
                        )
                    });
            }
        }
        if namespace.version < CATIA_CONSTRAINT_RANGE_INCIDENCE_VERSION
            || namespace.version < CATIA_CONSTRAINT_RANGE_SOURCE_ENTITY_VERSION
        {
            for entity in &mut entity_records {
                if let Some(range) = &mut entity.constraint_range {
                    range.incoming_references = constraint_range_incoming_references(
                        &records,
                        &entity.object_graph,
                        entity.entity_id,
                    );
                }
            }
        }
        if namespace.version < CATIA_CONFIGURATION_INCIDENCE_VERSION
            || namespace.version < CATIA_CONFIGURATION_SCHEMA_REFERENCE_VERSION
            || namespace.version < CATIA_TYPED_INCIDENCE_CLASS_VERSION
            || namespace.version < CATIA_TYPED_INCIDENCE_NULL_VERSION
        {
            let records_by_id = records
                .iter()
                .map(|record| (record.id.as_str(), record))
                .collect::<HashMap<_, _>>();
            for entity in &mut entity_records {
                entity.configuration_record = records_by_id
                    .get(entity.object_record.as_str())
                    .and_then(|object| {
                        configuration_record(
                            entity.entity_id,
                            object,
                            &entity.value_schema_selections,
                            &entities_by_graph_identity,
                            &entity_classes_by_graph_identity,
                            &terminal_nulls_by_graph,
                        )
                    });
                entity.configuration_row_link = records_by_id
                    .get(entity.object_record.as_str())
                    .and_then(|object| {
                        configuration_row_link(
                            entity.entity_id,
                            object,
                            &entities_by_graph_identity,
                            &entity_classes_by_graph_identity,
                            &terminal_nulls_by_graph,
                        )
                    });
            }
        }
        let expected_configuration_row_chains = derive_configuration_row_chains(
            &entity_records,
            &entities_by_graph_identity,
            &entity_classes_by_graph_identity,
            &terminal_nulls_by_graph,
        );
        if namespace.version < CATIA_CONFIGURATION_ROW_CHAIN_VERSION
            || namespace.version < CATIA_TYPED_INCIDENCE_NULL_VERSION
        {
            configuration_row_chains = expected_configuration_row_chains;
        } else if configuration_row_chains != expected_configuration_row_chains {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "configuration-row chains do not match their successor links".to_string(),
            ));
        }
        for graph in &mut graphs {
            graph.records = records
                .iter()
                .filter(|record| record.parent == graph.id)
                .cloned()
                .collect();
            graph.records.sort_by_key(|record| record.ordinal);
            let mut graph_entities = entity_records
                .iter()
                .filter(|entity| entity.object_graph == graph.id)
                .collect::<Vec<_>>();
            graph_entities.sort_by_key(|entity| entity.ordinal);
            let catalog = graph
                .catalog
                .as_ref()
                .and_then(|catalog_id| catalogs.iter().find(|catalog| catalog.id == *catalog_id));
            if !graph_entities.is_empty()
                && (graph_entities.len() != graph.records.len()
                    || graph_entities
                        .iter()
                        .enumerate()
                        .any(|(ordinal, entity)| entity.ordinal != ordinal as u64)
                    || graph_entities
                        .windows(2)
                        .any(|pair| pair[0].entity_id >= pair[1].entity_id)
                    || graph_entities
                        .iter()
                        .any(|entity| !valid_entity_record_shape(entity))
                    || graph_entities.iter().any(|entity| {
                        entity.definition_schema_selections
                            != definition_schema_selections(
                                &entity_table::parse_definition_schema_selectors(
                                    &entity.definition_prefix,
                                ),
                                catalog,
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.value_schema_selections
                            != entity_value_schema_selections(
                                &entity.value_fields,
                                catalog,
                                &entity.value_packets,
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.relation_expression
                            != relation_expression(
                                &entity.definition_schema_selections,
                                &entity.value_schema_selections,
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.suffix_value != entity_suffix_value(&entity.record_suffix)
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.suffix_framing != entity_suffix_framing(&entity.record_suffix)
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.suffix_schema_selection
                            != entity_suffix_schema_selection(entity.suffix_value.as_ref(), catalog)
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.parameter_value
                            != parameter_value(
                                entity.lead,
                                &entity.value_schema_selections,
                                entity.suffix_value.as_ref(),
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.constraint_range
                            != resolved_constraint_range(
                                entity.lead,
                                &entity.value_schema_selections,
                                entity.suffix_value.as_ref(),
                                &graph.records,
                                &graph.id,
                                entity.entity_id,
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.definition_value
                            != definition_value(
                                entity.lead,
                                &entity.definition_schema_selections,
                                &entity.value_fields,
                                entity.suffix_value.as_ref(),
                                entity.suffix_schema_selection.as_ref(),
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        entity.definition_chain_value
                            != definition_chain_value(
                                entity.lead,
                                &entity.definition_schema_selections,
                                &entity.value_fields,
                                entity.suffix_value.as_ref(),
                                entity.suffix_schema_selection.as_ref(),
                            )
                    })
                    || graph_entities.iter().any(|entity| {
                        let object = graph
                            .records
                            .iter()
                            .find(|record| record.id == entity.object_record);
                        entity.relation_program_instance
                            != object.and_then(|object| {
                                relation_program_instance(
                                    entity.entity_id,
                                    object,
                                    &entities_by_graph_identity,
                                    &entity_classes_by_graph_identity,
                                    &terminal_nulls_by_graph,
                                    &relation_expression_entities,
                                )
                            })
                    })
                    || graph_entities.iter().any(|entity| {
                        let object = graph
                            .records
                            .iter()
                            .find(|record| record.id == entity.object_record);
                        entity.configuration_record
                            != object.and_then(|object| {
                                configuration_record(
                                    entity.entity_id,
                                    object,
                                    &entity.value_schema_selections,
                                    &entities_by_graph_identity,
                                    &entity_classes_by_graph_identity,
                                    &terminal_nulls_by_graph,
                                )
                            })
                    })
                    || graph_entities.iter().any(|entity| {
                        let object = graph
                            .records
                            .iter()
                            .find(|record| record.id == entity.object_record);
                        entity.configuration_row_link
                            != object.and_then(|object| {
                                configuration_row_link(
                                    entity.entity_id,
                                    object,
                                    &entities_by_graph_identity,
                                    &entity_classes_by_graph_identity,
                                    &terminal_nulls_by_graph,
                                )
                            })
                    })
                    || graph_entities.iter().any(|entity| {
                        let object = graph
                            .records
                            .iter()
                            .find(|record| record.id == entity.object_record);
                        entity.formula_relation
                            != object.and_then(|object| {
                                formula_relation(
                                    &entity.definition_schema_selections,
                                    entity.entity_id,
                                    object,
                                    &relation_expressions,
                                    &CatiaEntityReferenceIndex {
                                        entities: &entities_by_graph_identity,
                                        classes: &entity_classes_by_graph_identity,
                                        terminal_nulls: &terminal_nulls_by_graph,
                                    },
                                    &parameter_bindings,
                                )
                            })
                    })
                    || graph_entities.windows(2).any(|pair| {
                        pair[0].byte_offset.checked_add(pair[0].byte_len)
                            != Some(pair[1].byte_offset)
                    })
                    || graph_entities.last().and_then(|entity| {
                        entity
                            .byte_offset
                            .checked_add(entity.byte_len)?
                            .checked_add(1)
                    }) != Some(graph.byte_offset))
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "object graph `{}` has an invalid entity-table sequence",
                    graph.id
                )));
            }
            let record_ids = graph
                .records
                .iter()
                .map(|record| record.id.clone())
                .collect::<Vec<_>>();
            let record_design_objects = graph
                .records
                .iter()
                .map(|record| record.design_object.clone())
                .collect::<Vec<_>>();
            let record_indices = graph
                .records
                .iter()
                .enumerate()
                .filter_map(|(index, record)| Some((record.entity_id?, index)))
                .collect::<HashMap<_, _>>();
            let terminal_null_entity_id = terminal_null_entity_id(&record_indices);
            if record_indices.len()
                != graph
                    .records
                    .iter()
                    .filter(|record| record.entity_id.is_some())
                    .count()
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "object graph `{}` has duplicate entity identities",
                    graph.id
                )));
            }
            for (ordinal, record) in graph.records.iter().enumerate() {
                let expected_head_roles = object_graph::head_roles(record.lead, &record.head);
                let expected_owner = expected_head_roles
                    .owner_ref
                    .map(CatiaObjectOwner::Entity)
                    .or_else(|| {
                        expected_head_roles
                            .owner_literal
                            .map(CatiaObjectOwner::UnassignedLiteral)
                    });
                let expected_design_object = record
                    .owner_entity_id()
                    .map(|owner| design_object_id(graph.byte_offset, owner));
                let paired_entity = graph_entities.get(ordinal).copied();
                let expected_storage = resolved_storage_link(
                    record.storage_ref,
                    &record_ids,
                    &record_design_objects,
                    &record_indices,
                );
                if usize::try_from(record.ordinal).ok() != Some(ordinal)
                    || record.owner != expected_owner
                    || (record.class_ref, record.storage_ref)
                        != (
                            expected_head_roles.class_ref,
                            expected_head_roles.storage_ref,
                        )
                    || record.design_object != expected_design_object
                    || record.entity_record != paired_entity.map(|entity| entity.id.clone())
                    || record.entity_id != paired_entity.map(|entity| entity.entity_id)
                    || paired_entity.is_some_and(|entity| entity.object_record != record.id)
                    || (
                        record.storage_record.as_ref(),
                        record.storage_design_object.as_ref(),
                    ) != (expected_storage.0.as_ref(), expected_storage.1.as_ref())
                    || record.repeated_reference_suffix
                        != object_graph::repeated_reference_suffix(&record.payload)
                    || record.inline_body.as_ref().is_some_and(|body| {
                        !object_graph::is_inline_body(body)
                            || record.lead != 0x10
                            || !record.head.is_empty()
                            || record.owner.is_some()
                            || record.class_ref.is_some()
                            || record.storage_ref.is_some()
                            || record.payload.size != 0
                            || !record.payload.fields.is_empty()
                            || record.subtype != PayloadSubtype::Empty
                    })
                    || record.inline_body.is_none() && record.head.is_empty()
                    || record.references
                        != resolved_payload_references(
                            &record.payload,
                            &record_ids,
                            &record_design_objects,
                            &record_indices,
                            terminal_null_entity_id,
                        )
                {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                        "object graph `{}` has an invalid record sequence",
                        graph.id
                    )));
                }
            }
        }
        let mut value_blocks: Vec<CatiaValueBlock> = namespace.arena_as("value_blocks")?;
        let value_schema_selections: Vec<CatiaValueSchemaSelection> =
            namespace.arena_as("value_schema_selections")?;
        let value_block_ids = value_blocks
            .iter()
            .map(|block| block.id.clone())
            .collect::<HashSet<_>>();
        if value_block_ids.len() != value_blocks.len() {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "duplicate CATIA value-block identity".to_string(),
            ));
        }
        let mut selections_by_block = HashMap::<String, Vec<CatiaValueSchemaSelection>>::new();
        for selection in value_schema_selections {
            if !value_block_ids.contains(&selection.parent) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "value selection `{}` references missing block `{}`",
                    selection.id, selection.parent
                )));
            }
            selections_by_block
                .entry(selection.parent.clone())
                .or_default()
                .push(selection);
        }
        for block in &mut value_blocks {
            block.schema_selections = selections_by_block.remove(&block.id).unwrap_or_default();
            block
                .schema_selections
                .sort_by_key(|selection| selection.offset);
        }
        let design_objects = design_objects(&graphs, &entity_records);
        if namespace.arenas.contains_key("design_objects") {
            let mut stored: Vec<CatiaDesignObject> = namespace.arena_as("design_objects")?;
            if namespace.version < CATIA_DEFINITION_CHAIN_OWNERSHIP_VERSION {
                let derived_by_id = design_objects
                    .iter()
                    .map(|object| (object.id.as_str(), object))
                    .collect::<HashMap<_, _>>();
                for object in &mut stored {
                    if let Some(derived) = derived_by_id.get(object.id.as_str()) {
                        object
                            .definition_chain_values
                            .clone_from(&derived.definition_chain_values);
                    }
                }
            }
            if namespace.version < CATIA_PARALLEL_REFERENCE_TABLE_VERSION
                || namespace.version < CATIA_TERMINAL_NULL_REFERENCE_VERSION
            {
                let derived_by_id = design_objects
                    .iter()
                    .map(|object| (object.id.as_str(), object))
                    .collect::<HashMap<_, _>>();
                for object in &mut stored {
                    if let Some(derived) = derived_by_id.get(object.id.as_str()) {
                        object
                            .parallel_reference_table
                            .clone_from(&derived.parallel_reference_table);
                    }
                }
            }
            let stored_by_id = stored
                .iter()
                .map(|object| (object.id.as_str(), object))
                .collect::<HashMap<_, _>>();
            if stored_by_id.len() != stored.len()
                || stored.len() != design_objects.len()
                || design_objects
                    .iter()
                    .any(|object| stored_by_id.get(object.id.as_str()).copied() != Some(object))
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    "stored CATIA design objects disagree with their object graph".to_string(),
                ));
            }
        }
        let mut finjpl_segments: Vec<CatiaFinjplSegment> =
            if namespace.arenas.contains_key("finjpl_segments") {
                namespace.arena_as("finjpl_segments")?
            } else {
                Vec::new()
            };
        finjpl_segments.sort_by_key(|segment| segment.byte_offset);
        if namespace.version < CATIA_OBJECT_GRAPH_SEGMENT_VERSION {
            for graph in &mut graphs {
                graph.finjpl_segment =
                    containing_finjpl_segment(graph.byte_offset, graph.byte_len, &finjpl_segments)
                        .map(str::to_owned);
            }
        }
        let mut external_references: Vec<CatiaExternalReference> =
            if namespace.arenas.contains_key("external_references") {
                namespace.arena_as("external_references")?
            } else {
                Vec::new()
            };
        external_references.sort_by_key(|reference| reference.byte_offset);
        let expected_external_references = external_reference_views(&finjpl_segments);
        if external_references != expected_external_references {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "stored CATIA external references disagree with their project-flags segments"
                    .to_string(),
            ));
        }
        let external_references = expected_external_references;
        let mut legacy_entity_runs: Vec<CatiaLegacyEntityRun> =
            if namespace.arenas.contains_key("legacy_entity_runs") {
                namespace.arena_as("legacy_entity_runs")?
            } else {
                Vec::new()
            };
        if namespace.version < CATIA_LEGACY_IDENTITY_LEAD_VERSION {
            for identity in legacy_entity_runs
                .iter_mut()
                .flat_map(|run| &mut run.identities)
            {
                identity.lead = 0x81;
            }
        }
        if namespace.version < CATIA_LEGACY_ROLE_SELECTOR_VERSION {
            for run in &mut legacy_entity_runs {
                for field in &mut run.text_fields {
                    if let Some(role) = &mut field.role {
                        role.entity_id = field.entity_id;
                        run.role_selectors.push(role.clone());
                    }
                }
                run.role_selectors.sort_by_key(|role| role.byte_offset);
                run.role_selectors.dedup_by_key(|role| role.byte_offset);
            }
        }
        if namespace.version < CATIA_LEGACY_SCHEMA_IDENTIFIER_VERSION {
            for program in legacy_entity_runs
                .iter_mut()
                .filter_map(|run| run.schema_program.as_mut())
            {
                program.identifiers = legacy_schema_identifiers(program).ok_or_else(|| {
                    cadmpeg_ir::NativeConvertError::InvalidOwner(
                        "legacy schema-program offset exceeds the platform index range".to_string(),
                    )
                })?;
            }
        }
        if namespace.version < CATIA_LEGACY_SCHEMA_BOUNDARY_VERSION {
            for program in legacy_entity_runs
                .iter_mut()
                .filter_map(|run| run.schema_program.as_mut())
            {
                program.boundary = CatiaLegacySchemaProgramBoundary::VendorFooter;
            }
        }
        if namespace.version < CATIA_LEGACY_EVALUATED_VALUE_NAME_VERSION {
            for run in &mut legacy_entity_runs {
                for index in 0..run.scalar_values.len() {
                    let entity_id = run.scalar_values[index].entity_id;
                    let value_offset = run.scalar_values[index].byte_offset;
                    let name = (run
                        .scalar_values
                        .iter()
                        .filter(|value| value.entity_id == entity_id)
                        .count()
                        == 1)
                        .then(|| {
                            legacy_value_name(
                                &run.role_selectors,
                                &run.text_fields,
                                entity_id,
                                value_offset,
                            )
                        })
                        .flatten();
                    run.scalar_values[index].name_field = name.as_ref().map(|(offset, _)| *offset);
                    run.scalar_values[index].name = name.map(|(_, name)| name);
                }
                for index in 0..run.string_values.len() {
                    let entity_id = run.string_values[index].entity_id;
                    let value_offset = run.string_values[index].byte_offset;
                    let name = (run
                        .string_values
                        .iter()
                        .filter(|value| value.entity_id == entity_id)
                        .count()
                        == 1)
                        .then(|| {
                            legacy_value_name(
                                &run.role_selectors,
                                &run.text_fields,
                                entity_id,
                                value_offset,
                            )
                        })
                        .flatten();
                    run.string_values[index].name_field = name.as_ref().map(|(offset, _)| *offset);
                    run.string_values[index].name = name.map(|(_, name)| name);
                }
                for index in 0..run.integer_values.len() {
                    let entity_id = run.integer_values[index].entity_id;
                    let value_offset = run.integer_values[index].byte_offset;
                    let name = (run
                        .integer_values
                        .iter()
                        .filter(|value| value.entity_id == entity_id)
                        .count()
                        == 1)
                        .then(|| {
                            legacy_value_name(
                                &run.role_selectors,
                                &run.text_fields,
                                entity_id,
                                value_offset,
                            )
                        })
                        .flatten();
                    run.integer_values[index].name_field = name.as_ref().map(|(offset, _)| *offset);
                    run.integer_values[index].name = name.map(|(_, name)| name);
                }
            }
        }
        legacy_entity_runs.sort_by_key(|run| run.byte_offset);
        validate_legacy_entity_runs(
            &legacy_entity_runs,
            namespace.version >= CATIA_LEGACY_ROLE_FIELD_CODE_VERSION,
        )?;
        let mut preview_images: Vec<CatiaPreviewImage> =
            if namespace.arenas.contains_key("preview_images") {
                namespace.arena_as("preview_images")?
            } else {
                Vec::new()
            };
        preview_images.sort_by_key(|preview| preview.byte_offset);
        let expected_preview_images = preview_views(&finjpl_segments);
        if preview_images != expected_preview_images {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "stored CATIA previews disagree with their summary segments".to_string(),
            ));
        }
        let preview_images = expected_preview_images;
        let alias_rows: Vec<CatiaAliasRow> = namespace.arena_as("alias_rows")?;
        let mut consolidated_circles: Vec<CatiaConsolidatedCircle> =
            namespace.arena_as("consolidated_circles")?;
        consolidated_circles.sort_by_key(|circle| circle.byte_offset);
        validate_consolidated_circles(&consolidated_circles)?;
        let mut consolidated_class61_records: Vec<CatiaConsolidatedClass61Record> =
            namespace.arena_as("consolidated_class61_records")?;
        consolidated_class61_records.sort_by_key(|record| record.byte_offset);
        validate_consolidated_class61_records(&consolidated_class61_records)?;
        let mut consolidated_cone_faces: Vec<CatiaConsolidatedConeFace> =
            namespace.arena_as("consolidated_cone_faces")?;
        consolidated_cone_faces.sort_by_key(|face| face.byte_offset);
        let mut consolidated_cones: Vec<CatiaConsolidatedCone> =
            namespace.arena_as("consolidated_cones")?;
        consolidated_cones.sort_by_key(|cone| cone.byte_offset);
        validate_consolidated_cones(&consolidated_cones)?;
        let mut consolidated_cylinders: Vec<CatiaConsolidatedCylinder> =
            namespace.arena_as("consolidated_cylinders")?;
        consolidated_cylinders.sort_by_key(|cylinder| cylinder.byte_offset);
        validate_consolidated_cylinders(&consolidated_cylinders)?;
        let mut consolidated_groups: Vec<CatiaConsolidatedGroup> =
            namespace.arena_as("consolidated_groups")?;
        consolidated_groups.sort_by_key(|group| group.byte_offset);
        validate_consolidated_groups(&consolidated_groups)?;
        let mut consolidated_embedded_cylinders: Vec<CatiaConsolidatedEmbeddedCylinder> =
            namespace.arena_as("consolidated_embedded_cylinders")?;
        consolidated_embedded_cylinders.sort_by_key(|cylinder| cylinder.byte_offset);
        validate_consolidated_embedded_cylinders(
            &consolidated_embedded_cylinders,
            &consolidated_groups,
        )?;
        let mut consolidated_line_profiles: Vec<CatiaConsolidatedLineProfile> =
            namespace.arena_as("consolidated_line_profiles")?;
        consolidated_line_profiles.sort_by_key(|line| line.byte_offset);
        validate_consolidated_line_profiles(&consolidated_line_profiles)?;
        let mut consolidated_owner_packets: Vec<CatiaConsolidatedOwnerPacket> =
            namespace.arena_as("consolidated_owner_packets")?;
        consolidated_owner_packets.sort_by_key(|packet| packet.byte_offset);
        validate_consolidated_owner_packets(&consolidated_owner_packets)?;
        let mut consolidated_parameter_points: Vec<CatiaConsolidatedParameterPoint> =
            namespace.arena_as("consolidated_parameter_points")?;
        consolidated_parameter_points.sort_by_key(|point| point.byte_offset);
        validate_consolidated_parameter_points(&consolidated_parameter_points)?;
        validate_consolidated_cone_faces(&consolidated_cone_faces, &consolidated_parameter_points)?;
        let mut consolidated_pcurves: Vec<CatiaConsolidatedPcurve> =
            namespace.arena_as("consolidated_pcurves")?;
        consolidated_pcurves.sort_by_key(|pcurve| pcurve.byte_offset);
        validate_consolidated_pcurves(&consolidated_pcurves)?;
        let mut consolidated_reference_lists: Vec<CatiaConsolidatedReferenceList> =
            namespace.arena_as("consolidated_reference_lists")?;
        consolidated_reference_lists.sort_by_key(|list| list.byte_offset);
        validate_consolidated_reference_lists(&consolidated_reference_lists)?;
        let mut consolidated_revolutions: Vec<CatiaConsolidatedRevolution> =
            namespace.arena_as("consolidated_revolutions")?;
        consolidated_revolutions.sort_by_key(|revolution| revolution.byte_offset);
        validate_consolidated_revolutions(&consolidated_revolutions, &consolidated_circles)?;
        let mut consolidated_spheres: Vec<CatiaConsolidatedSphere> =
            namespace.arena_as("consolidated_spheres")?;
        consolidated_spheres.sort_by_key(|sphere| sphere.byte_offset);
        validate_consolidated_spheres(&consolidated_spheres)?;
        let mut consolidated_tori: Vec<CatiaConsolidatedTorus> =
            namespace.arena_as("consolidated_tori")?;
        consolidated_tori.sort_by_key(|torus| torus.byte_offset);
        validate_consolidated_tori(&consolidated_tori)?;
        let mut consolidated_edge_runs: Vec<CatiaConsolidatedEdgeRun> =
            namespace.arena_as("consolidated_edge_runs")?;
        consolidated_edge_runs.sort_by_key(|run| run.byte_offset);
        let mut consolidated_edge_nodes: Vec<CatiaConsolidatedEdgeNode> =
            namespace.arena_as("consolidated_edge_nodes")?;
        consolidated_edge_nodes.sort_by_key(|node| node.byte_offset);
        let consolidated_vertex_identities: Vec<CatiaConsolidatedVertexIdentity> =
            namespace.arena_as("consolidated_vertex_identities")?;
        let mut zero_entity_edge_strides: Vec<CatiaZeroEntityEdgeStride> =
            namespace.arena_as("zero_entity_edge_strides")?;
        zero_entity_edge_strides.sort_by_key(|record| record.byte_offset);
        let mut zero_entity_oriented_use_pairs: Vec<CatiaZeroEntityOrientedUsePair> =
            namespace.arena_as("zero_entity_oriented_use_pairs")?;
        zero_entity_oriented_use_pairs.sort_by_key(|pair| pair.header_byte_offset);
        let zero_entity_ownership_roots: Vec<CatiaZeroEntityOwnershipRoot> =
            namespace.arena_as("zero_entity_ownership_roots")?;
        let zero_entity_endpoint_pair_candidates: Vec<CatiaZeroEntityEndpointPairCandidate> =
            namespace.arena_as("zero_entity_endpoint_pair_candidates")?;
        let mut zero_entity_records: Vec<CatiaZeroEntityRecord> =
            namespace.arena_as("zero_entity_records")?;
        zero_entity_records.sort_by_key(|record| record.record_ordinal);
        validate_zero_entity_records(&zero_entity_records)?;
        let mut zero_entity_support_runs: Vec<CatiaZeroEntitySupportRun> =
            namespace.arena_as("zero_entity_support_runs")?;
        zero_entity_support_runs.sort_by_key(|run| run.carrier_byte_offset);
        validate_zero_entity_support_runs(&zero_entity_support_runs, &zero_entity_records)?;
        validate_zero_entity_ownership_roots(
            &zero_entity_ownership_roots,
            &zero_entity_support_runs,
            &zero_entity_records,
        )?;
        let zero_entity_endpoint_locus_candidates: Vec<CatiaZeroEntityEndpointLocusCandidate> =
            namespace.arena_as("zero_entity_endpoint_locus_candidates")?;
        validate_zero_entity_endpoint_pair_candidates(
            &zero_entity_endpoint_pair_candidates,
            &zero_entity_support_runs,
        )?;
        validate_zero_entity_endpoint_locus_candidates(
            &zero_entity_endpoint_locus_candidates,
            &zero_entity_endpoint_pair_candidates,
            &zero_entity_support_runs,
        )?;
        let mut zero_entity_vertex_incidences: Vec<CatiaZeroEntityVertexIncidence> =
            namespace.arena_as("zero_entity_vertex_incidences")?;
        zero_entity_vertex_incidences.sort_by_key(|record| record.byte_offset);
        validate_zero_entity_topology_records(
            &zero_entity_edge_strides,
            &zero_entity_oriented_use_pairs,
            &zero_entity_vertex_incidences,
            &zero_entity_records,
        )?;
        validate_consolidated_edge_runs(
            &consolidated_edge_runs,
            &consolidated_pcurves,
            &ConsolidatedSupportArenas {
                circles: &consolidated_circles,
                cones: &consolidated_cones,
                cylinders: &consolidated_cylinders,
                embedded_cylinders: &consolidated_embedded_cylinders,
                groups: &consolidated_groups,
            },
            &consolidated_edge_nodes,
            &consolidated_vertex_identities,
        )?;
        validate_native_links(
            &alias_rows,
            &catalogs,
            &graphs,
            &finjpl_segments,
            &value_blocks,
        )?;
        Ok(Self {
            version: namespace.version,
            alias_rows,
            catalogs,
            consolidated_circles,
            consolidated_class61_records,
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
            consolidated_pcurves,
            consolidated_reference_lists,
            consolidated_revolutions,
            consolidated_spheres,
            consolidated_tori,
            consolidated_vertex_identities,
            configuration_row_chains,
            design_objects,
            entity_records,
            external_references,
            finjpl_segments,
            legacy_entity_runs,
            object_graphs: graphs,
            preview_images,
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

    /// Store the typed CATIA namespace into generic native arenas.
    #[cfg(test)]
    pub fn store(
        &self,
        namespace: &mut cadmpeg_ir::NativeNamespace,
    ) -> Result<(), cadmpeg_ir::NativeConvertError> {
        namespace.version = CATIA_NATIVE_VERSION;
        let catalogs = self
            .catalogs
            .iter()
            .cloned()
            .map(|mut catalog| {
                catalog.entries.clear();
                catalog
            })
            .collect::<Vec<_>>();
        let entries = self
            .catalogs
            .iter()
            .flat_map(|catalog| catalog.entries.iter().cloned())
            .collect::<Vec<_>>();
        let graphs = self
            .object_graphs
            .iter()
            .cloned()
            .map(|mut graph| {
                graph.records.clear();
                graph
            })
            .collect::<Vec<_>>();
        let records = self
            .object_graphs
            .iter()
            .flat_map(|graph| graph.records.iter().cloned())
            .collect::<Vec<_>>();
        let value_blocks = self
            .value_blocks
            .iter()
            .cloned()
            .map(|mut block| {
                block.schema_selections.clear();
                block
            })
            .collect::<Vec<_>>();
        let value_schema_selections = self
            .value_blocks
            .iter()
            .flat_map(|block| block.schema_selections.iter().cloned())
            .collect::<Vec<_>>();
        namespace.set_arena("catalogs", &catalogs)?;
        namespace.set_arena("consolidated_circles", &self.consolidated_circles)?;
        namespace.set_arena(
            "consolidated_class61_records",
            &self.consolidated_class61_records,
        )?;
        namespace.set_arena("consolidated_cone_faces", &self.consolidated_cone_faces)?;
        namespace.set_arena("consolidated_cones", &self.consolidated_cones)?;
        namespace.set_arena("consolidated_cylinders", &self.consolidated_cylinders)?;
        namespace.set_arena(
            "consolidated_embedded_cylinders",
            &self.consolidated_embedded_cylinders,
        )?;
        namespace.set_arena("consolidated_edge_nodes", &self.consolidated_edge_nodes)?;
        namespace.set_arena("consolidated_edge_runs", &self.consolidated_edge_runs)?;
        namespace.set_arena("consolidated_groups", &self.consolidated_groups)?;
        namespace.set_arena(
            "consolidated_line_profiles",
            &self.consolidated_line_profiles,
        )?;
        namespace.set_arena(
            "consolidated_owner_packets",
            &self.consolidated_owner_packets,
        )?;
        namespace.set_arena(
            "consolidated_parameter_points",
            &self.consolidated_parameter_points,
        )?;
        namespace.set_arena("consolidated_pcurves", &self.consolidated_pcurves)?;
        namespace.set_arena(
            "consolidated_reference_lists",
            &self.consolidated_reference_lists,
        )?;
        namespace.set_arena("consolidated_revolutions", &self.consolidated_revolutions)?;
        namespace.set_arena("consolidated_spheres", &self.consolidated_spheres)?;
        namespace.set_arena("consolidated_tori", &self.consolidated_tori)?;
        namespace.set_arena(
            "consolidated_vertex_identities",
            &self.consolidated_vertex_identities,
        )?;
        namespace.set_arena("configuration_row_chains", &self.configuration_row_chains)?;
        namespace.set_arena("design_objects", &self.design_objects)?;
        namespace.set_arena("entity_records", &self.entity_records)?;
        namespace.set_arena("external_references", &self.external_references)?;
        namespace.set_arena("finjpl_segments", &self.finjpl_segments)?;
        namespace.set_arena("legacy_entity_runs", &self.legacy_entity_runs)?;
        namespace.set_arena("alias_rows", &self.alias_rows)?;
        namespace.set_arena("catalog_entries", &entries)?;
        namespace.set_arena("object_graphs", &graphs)?;
        namespace.set_arena("object_graph_records", &records)?;
        namespace.set_arena("preview_images", &self.preview_images)?;
        namespace.set_arena("value_blocks", &value_blocks)?;
        namespace.set_arena("value_schema_selections", &value_schema_selections)?;
        namespace.set_arena("zero_entity_edge_strides", &self.zero_entity_edge_strides)?;
        namespace.set_arena(
            "zero_entity_oriented_use_pairs",
            &self.zero_entity_oriented_use_pairs,
        )?;
        namespace.set_arena(
            "zero_entity_ownership_roots",
            &self.zero_entity_ownership_roots,
        )?;
        namespace.set_arena(
            "zero_entity_endpoint_pair_candidates",
            &self.zero_entity_endpoint_pair_candidates,
        )?;
        namespace.set_arena("zero_entity_records", &self.zero_entity_records)?;
        namespace.set_arena("zero_entity_support_runs", &self.zero_entity_support_runs)?;
        namespace.set_arena(
            "zero_entity_endpoint_locus_candidates",
            &self.zero_entity_endpoint_locus_candidates,
        )?;
        namespace.set_arena(
            "zero_entity_vertex_incidences",
            &self.zero_entity_vertex_incidences,
        )?;
        debug_assert!(CATIA_ARENA_NAMES
            .iter()
            .all(|name| namespace.arenas.contains_key(*name)));
        Ok(())
    }

    /// Store this namespace while moving child arenas out of their typed owners.
    ///
    /// Decode paths use this form so large object graphs are not cloned while
    /// converting them into generic native records.
    pub fn store_owned(
        self,
        namespace: &mut cadmpeg_ir::NativeNamespace,
    ) -> Result<(), cadmpeg_ir::NativeConvertError> {
        let Self {
            version: _,
            alias_rows,
            mut catalogs,
            consolidated_circles,
            consolidated_class61_records,
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
            consolidated_pcurves,
            consolidated_reference_lists,
            consolidated_revolutions,
            consolidated_spheres,
            consolidated_tori,
            consolidated_vertex_identities,
            configuration_row_chains,
            design_objects,
            entity_records,
            external_references,
            finjpl_segments,
            legacy_entity_runs,
            mut object_graphs,
            preview_images,
            mut value_blocks,
            zero_entity_edge_strides,
            zero_entity_oriented_use_pairs,
            zero_entity_ownership_roots,
            zero_entity_endpoint_pair_candidates,
            zero_entity_records,
            zero_entity_support_runs,
            zero_entity_endpoint_locus_candidates,
            zero_entity_vertex_incidences,
        } = self;
        let entries = catalogs
            .iter_mut()
            .flat_map(|catalog| std::mem::take(&mut catalog.entries))
            .collect::<Vec<_>>();
        let records = object_graphs
            .iter_mut()
            .flat_map(|graph| std::mem::take(&mut graph.records))
            .collect::<Vec<_>>();
        let value_schema_selections = value_blocks
            .iter_mut()
            .flat_map(|block| std::mem::take(&mut block.schema_selections))
            .collect::<Vec<_>>();

        namespace.version = CATIA_NATIVE_VERSION;
        namespace.set_arena("catalogs", &catalogs)?;
        namespace.set_arena("consolidated_circles", &consolidated_circles)?;
        namespace.set_arena(
            "consolidated_class61_records",
            &consolidated_class61_records,
        )?;
        namespace.set_arena("consolidated_cone_faces", &consolidated_cone_faces)?;
        namespace.set_arena("consolidated_cones", &consolidated_cones)?;
        namespace.set_arena("consolidated_cylinders", &consolidated_cylinders)?;
        namespace.set_arena(
            "consolidated_embedded_cylinders",
            &consolidated_embedded_cylinders,
        )?;
        namespace.set_arena("consolidated_edge_nodes", &consolidated_edge_nodes)?;
        namespace.set_arena("consolidated_edge_runs", &consolidated_edge_runs)?;
        namespace.set_arena("consolidated_groups", &consolidated_groups)?;
        namespace.set_arena("consolidated_line_profiles", &consolidated_line_profiles)?;
        namespace.set_arena("consolidated_owner_packets", &consolidated_owner_packets)?;
        namespace.set_arena(
            "consolidated_parameter_points",
            &consolidated_parameter_points,
        )?;
        namespace.set_arena("consolidated_pcurves", &consolidated_pcurves)?;
        namespace.set_arena(
            "consolidated_reference_lists",
            &consolidated_reference_lists,
        )?;
        namespace.set_arena("consolidated_revolutions", &consolidated_revolutions)?;
        namespace.set_arena("consolidated_spheres", &consolidated_spheres)?;
        namespace.set_arena("consolidated_tori", &consolidated_tori)?;
        namespace.set_arena(
            "consolidated_vertex_identities",
            &consolidated_vertex_identities,
        )?;
        namespace.set_arena("configuration_row_chains", &configuration_row_chains)?;
        namespace.set_arena("design_objects", &design_objects)?;
        namespace.set_arena("entity_records", &entity_records)?;
        namespace.set_arena("external_references", &external_references)?;
        namespace.set_arena("catalog_entries", &entries)?;
        namespace.set_arena("object_graphs", &object_graphs)?;
        namespace.set_arena("object_graph_records", &records)?;
        namespace.set_arena("finjpl_segments", &finjpl_segments)?;
        namespace.set_arena("legacy_entity_runs", &legacy_entity_runs)?;
        namespace.set_arena("alias_rows", &alias_rows)?;
        namespace.set_arena("preview_images", &preview_images)?;
        namespace.set_arena("value_blocks", &value_blocks)?;
        namespace.set_arena("value_schema_selections", &value_schema_selections)?;
        namespace.set_arena("zero_entity_edge_strides", &zero_entity_edge_strides)?;
        namespace.set_arena(
            "zero_entity_oriented_use_pairs",
            &zero_entity_oriented_use_pairs,
        )?;
        namespace.set_arena("zero_entity_ownership_roots", &zero_entity_ownership_roots)?;
        namespace.set_arena(
            "zero_entity_endpoint_pair_candidates",
            &zero_entity_endpoint_pair_candidates,
        )?;
        namespace.set_arena("zero_entity_records", &zero_entity_records)?;
        namespace.set_arena("zero_entity_support_runs", &zero_entity_support_runs)?;
        namespace.set_arena(
            "zero_entity_endpoint_locus_candidates",
            &zero_entity_endpoint_locus_candidates,
        )?;
        namespace.set_arena(
            "zero_entity_vertex_incidences",
            &zero_entity_vertex_incidences,
        )?;
        debug_assert!(CATIA_ARENA_NAMES
            .iter()
            .all(|name| namespace.arenas.contains_key(*name)));
        Ok(())
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
            let value_fields = value_block::tokenize(&entity.value_payload);
            let value_packets = entity_table::value_packets(&entity.value_payload, &value_fields);
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
                definition_len: entity.definition_len,
                definition_prefix: entity.definition_prefix,
                definition_schema_selections: Vec::new(),
                entity_id: entity.entity_id,
                definition_suffix: entity.definition_suffix,
                value_len: entity.value_len,
                value_payload: entity.value_payload,
                value_fields,
                value_schema_selections: Vec::new(),
                relation_expression: None,
                parameter_value: None,
                constraint_range: None,
                definition_value: None,
                definition_chain_value: None,
                relation_program_instance: None,
                configuration_record: None,
                configuration_row_link: None,
                formula_relation: None,
                value_packets,
                numeric_tuple: entity.numeric_tuple,
                reference_signature: entity.reference_signature,
                record_suffix: entity.record_suffix,
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
