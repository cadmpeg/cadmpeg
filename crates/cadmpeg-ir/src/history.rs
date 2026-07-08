// SPDX-License-Identifier: Apache-2.0
//! Parametric construction history.

use crate::provenance::EntityMeta;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Configuration {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Feature {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_source_id: Option<String>,
    pub ordinal: u32,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub suppressed: bool,
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
    pub meta: EntityMeta,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureHistory {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_name: Option<String>,
    #[serde(default)]
    pub configurations: Vec<Configuration>,
    #[serde(default)]
    pub features: Vec<Feature>,
    pub meta: EntityMeta,
}

/// Native feature-input stream retained for parametric replay and rewrite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInputLane {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<String>,
    pub native_payload: Vec<u8>,
    #[serde(default)]
    pub sketch_entities: Vec<SketchInputEntity>,
    pub meta: EntityMeta,
}

/// One typed sketch-entity marker inside a native feature-input stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SketchInputEntity {
    pub ordinal: u32,
    pub offset: u64,
    pub kind: SketchInputKind,
    pub meta: EntityMeta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SketchInputKind {
    Point,
    Curve,
    Arc,
    ConstrainedPoint,
    Native(u32),
}

impl SketchInputKind {
    pub fn from_native_code(code: u32) -> Self {
        match code {
            0 => Self::Point,
            1 => Self::Curve,
            2 => Self::Arc,
            3 => Self::ConstrainedPoint,
            value => Self::Native(value),
        }
    }

    pub fn native_code(self) -> u32 {
        match self {
            Self::Point => 0,
            Self::Curve => 1,
            Self::Arc => 2,
            Self::ConstrainedPoint => 3,
            Self::Native(value) => value,
        }
    }
}

/// ASM construction-history container and its linked delta states.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AsmHistory {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_water_mark: Option<i64>,
    pub states: Vec<AsmDeltaState>,
    pub meta: EntityMeta,
}

/// One byte-framed ASM `delta_state` node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AsmDeltaState {
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
    /// State-local SAB entity revisions referenced by the bulletins.
    #[serde(default)]
    pub records: Vec<AsmHistoryRecord>,
    pub meta: EntityMeta,
}

/// One state-local SAB record retained byte-for-byte for replay and native write.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AsmHistoryRecord {
    pub index: u64,
    pub name: String,
    pub raw_bytes: Vec<u8>,
    pub meta: EntityMeta,
}

/// One BulletinBoard in an ASM construction state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AsmBulletinBoard {
    pub owner_ref: i64,
    pub number: i64,
    pub changes: Vec<AsmEntityChange>,
}

/// Entity revision pair carried by a BulletinBoard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AsmEntityChange {
    pub kind: AsmEntityChangeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_ref: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_ref: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AsmEntityChangeKind {
    Insert,
    Delete,
    Update,
}
