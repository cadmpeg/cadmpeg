// SPDX-License-Identifier: Apache-2.0
//! Fusion ASM construction-history record shapes.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cadmpeg_ir::document::Model;

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
    /// Typed kernel models reconstructed for older linked states. The active
    /// head state's model is the document's top-level model.
    #[serde(default)]
    pub historical_models: Vec<AsmHistoricalModel>,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmHistoryRecord {
    pub id: String,
    pub parent: String,
    pub index: u64,
    /// Byte offset of the record in the decompressed ASM stream.
    #[serde(default)]
    pub byte_offset: u64,
    /// Original ASM `RecordTable` index. History boundary records have no
    /// entity-table identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_index: Option<i64>,
    pub name: String,
    #[serde(with = "cadmpeg_ir::bytes")]
    #[schemars(with = "String")]
    pub raw_bytes: Vec<u8>,
}

/// A kernel model reconstructed at one retained construction-history state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct AsmHistoricalModel {
    pub id: String,
    pub history: String,
    pub state_id: i64,
    pub node_index: i64,
    pub model: Model,
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
