// SPDX-License-Identifier: Apache-2.0
//! Fusion ASM construction-history record shapes.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
}

/// Record revision occupying one stable entity slot at an ASM history state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmEntityVersion {
    pub entity_ref: i64,
    pub record_ref: i64,
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
