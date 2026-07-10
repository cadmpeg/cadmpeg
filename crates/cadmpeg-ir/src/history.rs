// SPDX-License-Identifier: Apache-2.0
//! Parametric construction history.

use crate::provenance::EntityMeta;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A named parametric-model variant (e.g. CAD "configuration") with its own
/// material and property overrides.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Configuration {
    /// Source configuration name.
    pub name: String,
    /// Material assigned in this configuration, when overridden; `None` when the
    /// configuration inherits the part's default material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// Source custom-property name/value pairs local to this configuration.
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

/// One parametric construction-history feature (e.g. an extrude or fillet operation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Feature {
    /// Native identifier of this feature, when the source assigned one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    /// Native identifier of this feature's parent in the construction tree, when
    /// the source recorded parent/child feature dependency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_source_id: Option<String>,
    /// Position of this feature in the construction-history timeline, in
    /// regeneration order.
    pub ordinal: u32,
    /// Feature display name.
    pub name: String,
    /// Native feature-type tag (e.g. `"Extrude"`, `"Fillet"`).
    pub kind: String,
    /// Whether this feature is suppressed and excluded from regeneration.
    #[serde(default)]
    pub suppressed: bool,
    /// Source parametric input values keyed by parameter name.
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
    /// Source custom-property name/value pairs local to this feature.
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
    /// Provenance metadata for this feature record.
    pub meta: EntityMeta,
}

/// The full parametric construction-history timeline for a part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureHistory {
    /// Source part display name, when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_name: Option<String>,
    /// Named parametric-model variants defined on this part.
    #[serde(default)]
    pub configurations: Vec<Configuration>,
    /// Ordered construction-history features, in regeneration order.
    #[serde(default)]
    pub features: Vec<Feature>,
    /// Provenance metadata for this history record.
    pub meta: EntityMeta,
}

/// Native feature-input stream retained for parametric replay and rewrite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInputLane {
    /// Stable source-derived identifier for this feature-input record.
    pub id: String,
    /// Configuration this input lane applies to, when the source scoped inputs
    /// per configuration; `None` when the lane applies to all configurations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<String>,
    /// Complete native feature-input byte stream, retained undecoded for
    /// parametric replay and native rewrite.
    pub native_payload: Vec<u8>,
    /// Typed sketch-entity markers located within `native_payload`.
    #[serde(default)]
    pub sketch_entities: Vec<SketchInputEntity>,
    /// Provenance metadata for this input-lane record.
    pub meta: EntityMeta,
}

/// One typed sketch-entity marker inside a native feature-input stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SketchInputEntity {
    /// Position of this marker within the owning `FeatureInputLane`, in stream order.
    pub ordinal: u32,
    /// Byte offset of this marker within `FeatureInputLane::native_payload`.
    pub offset: u64,
    /// Sketch-entity kind this marker identifies.
    pub kind: SketchInputKind,
    /// Provenance metadata for this marker record.
    pub meta: EntityMeta,
}

/// Kind of sketch entity referenced by a native feature-input marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SketchInputKind {
    /// A sketch point.
    Point,
    /// A general sketch curve.
    Curve,
    /// A sketch arc.
    Arc,
    /// A sketch point bound by a geometric constraint.
    ConstrainedPoint,
    /// A native code not in the known vocabulary, preserved verbatim.
    Native(u32),
}

impl SketchInputKind {
    /// Maps a native sketch-entity type code to its typed kind, falling back to
    /// [`SketchInputKind::Native`] for unrecognized codes.
    pub fn from_native_code(code: u32) -> Self {
        match code {
            0 => Self::Point,
            1 => Self::Curve,
            2 => Self::Arc,
            3 => Self::ConstrainedPoint,
            value => Self::Native(value),
        }
    }

    /// Returns the native sketch-entity type code for this kind, the inverse of
    /// [`SketchInputKind::from_native_code`].
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
    /// Declared byte length of the ASM history stream from its preamble, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_size: Option<i64>,
    /// Highest state id watermark recorded in the history preamble, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high_water_mark: Option<i64>,
    /// Linked `delta_state` nodes forming the construction-state chain.
    pub states: Vec<AsmDeltaState>,
    /// Provenance metadata for this history record.
    pub meta: EntityMeta,
}

/// One byte-framed ASM `delta_state` node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AsmDeltaState {
    /// Construction-state id; the head node's `state_id` equals the history
    /// stream preamble's first field.
    pub state_id: i64,
    /// Source version tag on the `delta_state` record; observed constant `1`.
    pub version_flag: i64,
    /// Source state tag on the `delta_state` record; observed constant `0`.
    pub state_flag: i64,
    /// Reference to the previous state in the doubly-linked chain; `None` on the head node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_ref: Option<i64>,
    /// Reference to the next state in the doubly-linked chain; `None` on the tail node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_ref: Option<i64>,
    /// Sequential position of this node in the chain (0, 1, 2, ...).
    pub node_index: i64,
    /// Reference to a partner state, when the source recorded one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partner_ref: Option<i64>,
    /// Reference to the owning entity or container of this state.
    pub owner_ref: i64,
    /// Per-entity insert/delete/update bulletins recorded in this state.
    #[serde(default)]
    pub bulletin_boards: Vec<AsmBulletinBoard>,
    /// State-local SAB entity revisions referenced by the bulletins.
    #[serde(default)]
    pub records: Vec<AsmHistoryRecord>,
    /// Provenance metadata for this delta-state record.
    pub meta: EntityMeta,
}

/// One state-local SAB record retained byte-for-byte for replay and native write.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AsmHistoryRecord {
    /// Index of this record within its owning `delta_state`'s local record table.
    pub index: u64,
    /// Source class/record-type name.
    pub name: String,
    /// Complete undecoded source record bytes, retained for native replay/write.
    pub raw_bytes: Vec<u8>,
    /// Provenance metadata for this record.
    pub meta: EntityMeta,
}

/// One `BulletinBoard` in an ASM construction state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AsmBulletinBoard {
    /// Reference to the entity or container this bulletin board is attached to.
    pub owner_ref: i64,
    /// Sequential bulletin-board number within its owning state.
    pub number: i64,
    /// Per-entity insert/delete/update change bulletins carried by this board.
    pub changes: Vec<AsmEntityChange>,
}

/// Entity revision pair carried by a `BulletinBoard`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AsmEntityChange {
    /// Whether this bulletin records an entity insert, delete, or update.
    pub kind: AsmEntityChangeKind,
    /// Reference to the entity revision before the change; `None` on insert.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_ref: Option<i64>,
    /// Reference to the entity revision after the change; `None` on delete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_ref: Option<i64>,
}

/// The kind of change one ASM bulletin records against an entity revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AsmEntityChangeKind {
    /// A new entity revision was created.
    Insert,
    /// An entity revision was removed.
    Delete,
    /// An entity revision was modified.
    Update,
}
