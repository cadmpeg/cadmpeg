// SPDX-License-Identifier: Apache-2.0
#![deny(clippy::disallowed_methods)]
//! Fusion ASM construction-history record shapes.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cadmpeg_ir::math::{Point3, Vector3};

/// Stream-size and history-entry-count pair from an ASM history preamble.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AsmPreamble {
    pub stream_size: i64,
    pub history_entry_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "AsmHistorySerde"))]
#[serde(try_from = "AsmHistorySerde", into = "AsmHistorySerde")]
pub(crate) struct AsmHistory {
    pub id: String,
    pub byte_offset: u64,
    pub preamble: Option<AsmPreamble>,
    /// True when historical topology binding was not attempted because its
    /// state-by-record work estimate exceeded the decoder safety budget.
    pub record_table_binding_budget_exceeded: bool,
    /// Historical projection consumers finished and any temporary complete
    /// topology snapshots were released. A compact plane-selection topology can
    /// remain for late feature projection.
    pub projection_finalized: bool,
    pub states: Vec<AsmDeltaState>,
}

impl AsmHistory {
    pub(crate) fn stream_size(&self) -> Option<i64> {
        self.preamble.map(|preamble| preamble.stream_size)
    }

    pub(crate) fn history_entry_count(&self) -> Option<i64> {
        self.preamble.map(|preamble| preamble.history_entry_count)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct AsmHistorySerde {
    id: String,
    byte_offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stream_size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(alias = "high_water_mark")]
    history_entry_count: Option<i64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    record_table_binding_budget_exceeded: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    projection_finalized: bool,
    states: Vec<AsmDeltaState>,
}

impl TryFrom<AsmHistorySerde> for AsmHistory {
    type Error = String;

    fn try_from(wire: AsmHistorySerde) -> Result<Self, Self::Error> {
        let preamble = match (wire.stream_size, wire.history_entry_count) {
            (Some(stream_size), Some(history_entry_count)) => Some(AsmPreamble {
                stream_size,
                history_entry_count,
            }),
            (None, None) => None,
            _ => {
                return Err(
                    "asm history stream_size and history_entry_count must be paired".into(),
                );
            }
        };
        Ok(Self {
            id: wire.id,
            byte_offset: wire.byte_offset,
            preamble,
            record_table_binding_budget_exceeded: wire.record_table_binding_budget_exceeded,
            projection_finalized: wire.projection_finalized,
            states: wire.states,
        })
    }
}

impl From<AsmHistory> for AsmHistorySerde {
    fn from(history: AsmHistory) -> Self {
        let stream_size = history.stream_size();
        let history_entry_count = history.history_entry_count();
        Self {
            id: history.id,
            byte_offset: history.byte_offset,
            stream_size,
            history_entry_count,
            record_table_binding_budget_exceeded: history.record_table_binding_budget_exceeded,
            projection_finalized: history.projection_finalized,
            states: history.states,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "AsmDeltaStateWire", into = "AsmDeltaStateWire")]
#[cfg_attr(feature = "schema", schemars(with = "AsmDeltaStateWire"))]
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
    /// Topology-entity slot to record-revision map at this state. The decoder
    /// retains this compact map for late persistent-selection binding after
    /// projection caches are finalized.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_versions: Vec<AsmEntityVersion>,
    pub topology_cache: AsmTopologyCache,
    /// Forward change from the state reached by `next_ref` to this state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<AsmHistoricalTransition>,
}

/// Historical topology retained for projection or for late identity resolution.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) enum AsmTopologyCache {
    #[default]
    Absent,
    Complete(AsmHistoricalTopology),
    Retained(AsmHistoricalTopology),
}

impl AsmDeltaState {
    pub(crate) fn topology(&self) -> Option<&AsmHistoricalTopology> {
        match &self.topology_cache {
            AsmTopologyCache::Absent => None,
            AsmTopologyCache::Complete(topology) | AsmTopologyCache::Retained(topology) => Some(topology),
        }
    }

    #[cfg(test)]
    pub(crate) fn topology_mut(&mut self) -> Option<&mut AsmHistoricalTopology> {
        match &mut self.topology_cache {
            AsmTopologyCache::Absent => None,
            AsmTopologyCache::Complete(topology) | AsmTopologyCache::Retained(topology) => Some(topology),
        }
    }

    pub(crate) fn record_table_complete(&self) -> bool {
        matches!(self.topology_cache, AsmTopologyCache::Complete(_))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct AsmDeltaStateWire {
    id: String,
    parent: String,
    byte_offset: u64,
    state_id: i64,
    version_flag: i64,
    state_flag: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_ref: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    next_ref: Option<i64>,
    node_index: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    partner_ref: Option<i64>,
    owner_ref: i64,
    #[serde(default)]
    bulletin_boards: Vec<AsmBulletinBoard>,
    #[serde(default)]
    records: Vec<AsmHistoryRecord>,
    /// Topology-entity slot to record-revision map at this state. The decoder
    /// retains this compact map for late persistent-selection binding after
    /// projection caches are finalized.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    entity_versions: Vec<AsmEntityVersion>,
    /// Every selected record frames and every entity reference resolves after
    /// revision identities are normalized to stable entity slots.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    record_table_complete: bool,
    /// Stable `RecordTable` identities emitted by the ordinary B-rep decoder for
    /// this historical state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    topology: Option<AsmHistoricalTopology>,
    /// Forward change from the state reached by `next_ref` to this state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transition: Option<AsmHistoricalTransition>,
}

impl TryFrom<AsmDeltaStateWire> for AsmDeltaState {
    type Error = String;

    fn try_from(wire: AsmDeltaStateWire) -> Result<Self, Self::Error> {
        let topology_cache = match (wire.record_table_complete, wire.topology) {
            (false, None) => AsmTopologyCache::Absent,
            (false, Some(topology)) => AsmTopologyCache::Retained(topology),
            (true, Some(topology)) => AsmTopologyCache::Complete(topology),
            (true, None) => return Err("record_table_complete requires topology".into()),
        };
        Ok(Self {
            id: wire.id,
            parent: wire.parent,
            byte_offset: wire.byte_offset,
            state_id: wire.state_id,
            version_flag: wire.version_flag,
            state_flag: wire.state_flag,
            previous_ref: wire.previous_ref,
            next_ref: wire.next_ref,
            node_index: wire.node_index,
            partner_ref: wire.partner_ref,
            owner_ref: wire.owner_ref,
            bulletin_boards: wire.bulletin_boards,
            records: wire.records,
            entity_versions: wire.entity_versions,
            transition: wire.transition,
            topology_cache,
        })
    }
}

impl From<AsmDeltaState> for AsmDeltaStateWire {
    fn from(state: AsmDeltaState) -> Self {
        let record_table_complete = state.record_table_complete();
        let topology = match state.topology_cache {
            AsmTopologyCache::Absent => None,
            AsmTopologyCache::Complete(topology) | AsmTopologyCache::Retained(topology) => Some(topology),
        };
        Self {
            id: state.id,
            parent: state.parent,
            byte_offset: state.byte_offset,
            state_id: state.state_id,
            version_flag: state.version_flag,
            state_flag: state.state_flag,
            previous_ref: state.previous_ref,
            next_ref: state.next_ref,
            node_index: state.node_index,
            partner_ref: state.partner_ref,
            owner_ref: state.owner_ref,
            bulletin_boards: state.bulletin_boards,
            records: state.records,
            entity_versions: state.entity_versions,
            transition: state.transition,
            record_table_complete,
            topology,
        }
    }
}

/// Record revision occupying one stable entity slot at an ASM history state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct AsmEntityVersion {
    pub entity_ref: i64,
    pub record_ref: i64,
}

/// Stable entity-slot membership of one re-derived historical B-rep.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
    /// Exact right-circular cylinder carriers in this historical state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surface_cylinders: Vec<AsmHistoricalCylinder>,
    /// Exact plane carriers in this historical state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surface_planes: Vec<AsmHistoricalPlane>,
    /// Model-space axes of axis-bearing analytic surface carriers in this state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surface_axes: Vec<AsmHistoricalSurfaceAxis>,
    pub curves: Vec<i64>,
    /// Model-space axes of axis-bearing curve carriers in this state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub curve_axes: Vec<AsmHistoricalCurveAxis>,
    pub pcurves: Vec<i64>,
    /// Persistent tag groups attached to face and edge revisions in this state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub persistent_subentity_tags: Vec<AsmHistoricalPersistentSubentityTag>,
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

/// One persistent tag group attached to a historical face or edge revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct AsmHistoricalPersistentSubentityTag {
    pub entity_kind: crate::records::AsmHistoricalEntityKind,
    pub entity_ref: i64,
    pub selector: i64,
    pub token: String,
    pub design_references: Vec<i64>,
    pub ordinal: u32,
}

/// Stable axis-bearing curve carrier value in one historical B-rep state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct AsmHistoricalCurveAxis {
    pub curve: i64,
    pub origin: Point3,
    pub direction: Vector3,
}

/// Stable axis line of one cylinder, cone, or torus carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct AsmHistoricalSurfaceAxis {
    pub surface: i64,
    pub origin: Point3,
    pub direction: Vector3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct AsmHistoricalSurfaceRadius {
    pub surface: i64,
    pub radius: f64,
}

/// Stable geometry of one right-circular cylinder carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct AsmHistoricalCylinder {
    pub surface: i64,
    pub origin: Point3,
    pub axis: Vector3,
    pub radius: f64,
}

/// Stable geometry of one plane carrier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct AsmHistoricalPlane {
    pub surface: i64,
    pub origin: Point3,
    pub normal: Vector3,
}

/// Stable point-carrier value in one historical B-rep state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct AsmHistoricalPoint {
    pub point: i64,
    pub position: Point3,
}

/// Ordered stable entity-slot relation in a historical B-rep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct AsmHistoricalRelation {
    pub owner_ref: i64,
    pub member_refs: Vec<i64>,
}

/// Stable topology links of one historical coedge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct AsmHistoricalCoedge {
    pub coedge: i64,
    pub owner_loop: i64,
    pub edge: i64,
    pub next: i64,
    pub previous: i64,
    pub radial_next: i64,
}

/// Ordered endpoint links of one historical edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct AsmHistoricalEdge {
    pub edge: i64,
    pub start_vertex: i64,
    pub end_vertex: i64,
}

/// Stable binding from a topology entity to its required geometry carrier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct AsmHistoricalCarrierBinding {
    pub entity: i64,
    pub carrier: i64,
}

/// Stable binding from a topology entity to its optional geometry carrier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct AsmHistoricalOptionalCarrierBinding {
    pub entity: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carrier: Option<i64>,
}

/// Forward stable-slot changes from an older ASM state to a newer state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct AsmHistoricalEntityDelta {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inserted: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted: Vec<i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub updated: Vec<i64>,
}

/// Per-family topology changes between two complete historical states.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
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
    /// Framing failure that forced this span to remain opaque.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framing_error: Option<String>,
    /// Ordered `0x0c` entity-reference tokens in the history revision namespace.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entity_references: Vec<i64>,
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    pub raw_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub(crate) struct AsmBulletinBoard {
    pub id: String,
    pub parent: String,
    pub byte_offset: u64,
    pub owner_ref: i64,
    pub number: i64,
    pub changes: Vec<AsmEntityChange>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "AsmEntityChangeSerde"))]
#[serde(try_from = "AsmEntityChangeSerde", into = "AsmEntityChangeSerde")]
pub(crate) struct AsmEntityChange {
    pub id: String,
    pub parent: String,
    pub byte_offset: u64,
    pub kind: AsmEntityChangeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AsmEntityChangeKind {
    Insert { new: i64 },
    Delete { old: i64 },
    Update { old: i64, new: i64 },
}

impl AsmEntityChange {
    pub(crate) fn old_ref(&self) -> Option<i64> {
        match self.kind {
            AsmEntityChangeKind::Insert { .. } => None,
            AsmEntityChangeKind::Delete { old } | AsmEntityChangeKind::Update { old, .. } => {
                Some(old)
            }
        }
    }

    pub(crate) fn new_ref(&self) -> Option<i64> {
        match self.kind {
            AsmEntityChangeKind::Delete { .. } => None,
            AsmEntityChangeKind::Insert { new } | AsmEntityChangeKind::Update { new, .. } => {
                Some(new)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct AsmEntityChangeSerde {
    id: String,
    parent: String,
    byte_offset: u64,
    kind: AsmEntityChangeKindWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    old_ref: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    new_ref: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
enum AsmEntityChangeKindWire {
    Insert,
    Delete,
    Update,
}

impl TryFrom<AsmEntityChangeSerde> for AsmEntityChange {
    type Error = String;

    fn try_from(wire: AsmEntityChangeSerde) -> Result<Self, Self::Error> {
        let kind = match (wire.kind, wire.old_ref, wire.new_ref) {
            (AsmEntityChangeKindWire::Insert, None, Some(new)) => {
                AsmEntityChangeKind::Insert { new }
            }
            (AsmEntityChangeKindWire::Delete, Some(old), None) => {
                AsmEntityChangeKind::Delete { old }
            }
            (AsmEntityChangeKindWire::Update, Some(old), Some(new)) => {
                AsmEntityChangeKind::Update { old, new }
            }
            _ => {
                return Err("asm entity change kind disagrees with old_ref/new_ref".into());
            }
        };
        Ok(Self {
            id: wire.id,
            parent: wire.parent,
            byte_offset: wire.byte_offset,
            kind,
        })
    }
}

impl From<AsmEntityChange> for AsmEntityChangeSerde {
    fn from(change: AsmEntityChange) -> Self {
        let (kind, old_ref, new_ref) = match change.kind {
            AsmEntityChangeKind::Insert { new } => {
                (AsmEntityChangeKindWire::Insert, None, Some(new))
            }
            AsmEntityChangeKind::Delete { old } => {
                (AsmEntityChangeKindWire::Delete, Some(old), None)
            }
            AsmEntityChangeKind::Update { old, new } => {
                (AsmEntityChangeKindWire::Update, Some(old), Some(new))
            }
        };
        Self {
            id: change.id,
            parent: change.parent,
            byte_offset: change.byte_offset,
            kind,
            old_ref,
            new_ref,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AsmDeltaState, AsmHistoricalTopology, AsmTopologyCache};

    #[test]
    fn topology_cache_wire_preserves_three_states_and_rejects_complete_absence() {
        let prefix = r#"{"id":"state","parent":"history","byte_offset":0,"state_id":1,"version_flag":1,"state_flag":0,"node_index":1,"owner_ref":0,"bulletin_boards":[],"records":[]"#;
        let topology = serde_json::to_string(&AsmHistoricalTopology::default()).unwrap();
        for (complete, fields) in [
            (false, String::new()),
            (false, format!(",\"topology\":{topology}")),
            (true, format!(",\"record_table_complete\":true,\"topology\":{topology}")),
        ] {
            let wire = format!("{prefix}{fields}}}");
            let state: AsmDeltaState = serde_json::from_str(&wire).unwrap();
            assert_eq!(state.record_table_complete(), complete);
            assert_eq!(state.topology().is_some(), !fields.is_empty());
            match (&state.topology_cache, complete, fields.is_empty()) {
                (AsmTopologyCache::Absent, false, true)
                | (AsmTopologyCache::Retained(_), false, false)
                | (AsmTopologyCache::Complete(_), true, false) => {}
                other => panic!("unexpected topology cache: {other:?}"),
            }
            assert_eq!(serde_json::to_string(&state).unwrap(), wire);
        }
        let invalid = format!("{prefix},\"record_table_complete\":true}}");
        let error = serde_json::from_str::<AsmDeltaState>(&invalid).unwrap_err().to_string();
        assert!(error.contains("record_table_complete"));
        assert!(error.contains("topology"));
    }
}
