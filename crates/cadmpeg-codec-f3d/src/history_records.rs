// SPDX-License-Identifier: Apache-2.0
#![deny(clippy::disallowed_methods)]
//! Fusion ASM construction-history record shapes.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cadmpeg_ir::math::{Point3, Vector3};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmHistory {
    pub id: String,
    pub byte_offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(alias = "high_water_mark")]
    pub history_entry_count: Option<i64>,
    pub states: Vec<AsmDeltaState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmDeltaState {
    pub id: String,
    pub parent: String,
    pub byte_offset: u64,
    pub state_id: i64,
    pub version_flag: i64,
    pub state_flag: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_ref: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_ref: Option<i64>,
    pub node_index: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partner_ref: Option<i64>,
    pub owner_ref: i64,
    #[serde(default)]
    pub bulletin_boards: Vec<AsmBulletinBoard>,
    #[serde(default)]
    pub records: Vec<AsmHistoryRecord>,
    /// Complete entity-slot to record-revision map at this state. Empty when
    /// the history does not form a complete reversible chain.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_versions: Vec<AsmEntityVersion>,
    /// Every selected record frames and every entity reference resolves after
    /// revision identities are normalized to stable entity slots.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub record_table_complete: bool,
    /// Stable `RecordTable` identities emitted by the ordinary B-rep decoder for
    /// this historical state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topology: Option<AsmHistoricalTopology>,
    /// Forward change from the state reached by `next_ref` to this state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<AsmHistoricalTransition>,
}

/// Record revision occupying one stable entity slot at an ASM history state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmEntityVersion {
    pub entity_ref: i64,
    pub record_ref: i64,
}

/// Stable entity-slot membership of one re-derived historical B-rep.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmHistoricalTopology {
    pub bodies: Vec<i64>,
    pub regions: Vec<i64>,
    pub shells: Vec<i64>,
    pub faces: Vec<i64>,
    pub loops: Vec<i64>,
    pub coedges: Vec<i64>,
    pub edges: Vec<i64>,
    pub vertices: Vec<i64>,
    pub points: Vec<i64>,
    pub surfaces: Vec<i64>,
    /// Characteristic radii of analytic or constant-radius blend carriers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surface_radii: Vec<AsmHistoricalSurfaceRadius>,
    pub curves: Vec<i64>,
    /// Model-space axes of axis-bearing curve carriers in this state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub curve_axes: Vec<AsmHistoricalCurveAxis>,
    pub pcurves: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body_regions: Vec<AsmHistoricalRelation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub region_shells: Vec<AsmHistoricalRelation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shell_faces: Vec<AsmHistoricalRelation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shell_wire_edges: Vec<AsmHistoricalRelation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shell_free_vertices: Vec<AsmHistoricalRelation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub face_loops: Vec<AsmHistoricalRelation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loop_coedges: Vec<AsmHistoricalRelation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub coedge_topology: Vec<AsmHistoricalCoedge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edge_vertices: Vec<AsmHistoricalEdge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub face_surfaces: Vec<AsmHistoricalCarrierBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edge_curves: Vec<AsmHistoricalOptionalCarrierBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub coedge_pcurves: Vec<AsmHistoricalOptionalCarrierBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vertex_points: Vec<AsmHistoricalCarrierBinding>,
    /// Model-space values of the point carriers in this historical state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub point_positions: Vec<AsmHistoricalPoint>,
}

/// Stable axis-bearing curve carrier value in one historical B-rep state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmHistoricalCurveAxis {
    pub curve: i64,
    pub origin: Point3,
    pub direction: Vector3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmHistoricalSurfaceRadius {
    pub surface: i64,
    pub radius: f64,
}

/// Stable point-carrier value in one historical B-rep state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmHistoricalPoint {
    pub point: i64,
    pub position: Point3,
}

/// Ordered stable entity-slot relation in a historical B-rep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmHistoricalRelation {
    pub owner_ref: i64,
    pub member_refs: Vec<i64>,
}

/// Stable topology links of one historical coedge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmHistoricalCoedge {
    pub coedge: i64,
    pub owner_loop: i64,
    pub edge: i64,
    pub next: i64,
    pub previous: i64,
    pub radial_next: i64,
}

/// Ordered endpoint links of one historical edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmHistoricalEdge {
    pub edge: i64,
    pub start_vertex: i64,
    pub end_vertex: i64,
}

/// Stable binding from a topology entity to its required geometry carrier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmHistoricalCarrierBinding {
    pub entity: i64,
    pub carrier: i64,
}

/// Stable binding from a topology entity to its optional geometry carrier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmHistoricalOptionalCarrierBinding {
    pub entity: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carrier: Option<i64>,
}

/// Forward stable-slot changes from an older ASM state to a newer state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmHistoricalTransition {
    /// Older state identity; absent only at the end of the reverse-history chain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_state_id: Option<i64>,
    /// Changes across the complete normalized `RecordTable`.
    pub records: AsmHistoricalEntityDelta,
    /// Changes restricted to each normalized topology family.
    pub topology: AsmHistoricalTopologyDelta,
}

/// Stable entity slots inserted, deleted, or assigned a different record revision.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmHistoricalEntityDelta {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inserted: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub updated: Vec<i64>,
}

/// Per-family topology changes between two complete historical states.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmHistoricalTopologyDelta {
    pub bodies: AsmHistoricalEntityDelta,
    pub regions: AsmHistoricalEntityDelta,
    pub shells: AsmHistoricalEntityDelta,
    pub faces: AsmHistoricalEntityDelta,
    pub loops: AsmHistoricalEntityDelta,
    pub coedges: AsmHistoricalEntityDelta,
    pub edges: AsmHistoricalEntityDelta,
    pub vertices: AsmHistoricalEntityDelta,
    pub points: AsmHistoricalEntityDelta,
    pub surfaces: AsmHistoricalEntityDelta,
    pub curves: AsmHistoricalEntityDelta,
    pub pcurves: AsmHistoricalEntityDelta,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmHistoryRecord {
    pub id: String,
    pub parent: String,
    /// Construction-history revision identity paired from the ordered
    /// old-reference run; absent only for the stream terminator or an opaque
    /// snapshot whose pairing cannot be established.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_id: Option<i64>,
    /// Snapshot-local record ordinal. This is not the revision identity.
    pub index: u64,
    /// Byte offset of the record in the decompressed ASM stream.
    #[serde(default)]
    pub byte_offset: u64,
    pub name: String,
    /// Ordered `0x0c` entity-reference tokens in the history revision namespace.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_references: Vec<i64>,
    #[serde(with = "cadmpeg_ir::bytes")]
    #[schemars(with = "String")]
    pub raw_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmBulletinBoard {
    pub id: String,
    pub parent: String,
    pub byte_offset: u64,
    pub owner_ref: i64,
    pub number: i64,
    pub changes: Vec<AsmEntityChange>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmEntityChange {
    pub id: String,
    pub parent: String,
    pub byte_offset: u64,
    pub kind: AsmEntityChangeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_ref: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_ref: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AsmEntityChangeKind {
    Insert,
    Delete,
    Update,
}
