// SPDX-License-Identifier: Apache-2.0
//! Evaluations and current states: the states of a scope.
//!
//! An evaluation is a state of a scope under one configuration, stored as a
//! base with a delta applied. The current state of every scope is held
//! outside evaluations: its geometry in `CadIr.geometry`, its names in
//! `Design.names`, and its poses in [`CurrentState::poses`].

use std::collections::BTreeMap;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ids::EvaluationId;
use super::names::NameEntry;
use super::refs::{EntityRef, Evaluated, Position, RecordRef};
use super::selection::OccurrencePath;
use crate::document::Topology;
use crate::features::ConfigurationId;
use crate::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use crate::transform::Transform;

/// A state of a scope under one configuration: the model at a position, or a
/// feature's tool body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Evaluation {
    /// The evaluation's identity.
    pub id: EvaluationId,
    /// The product definition whose scope the state is a state of.
    pub scope: ProductDefinitionId,
    /// The configuration of the state. `None` only when the scope has no
    /// configurations.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_configuration"
    )]
    pub configuration: Option<ConfigurationId>,
    /// What the evaluation evaluates.
    pub of: Evaluated,
    /// The status the source records for the state.
    pub status: EvaluationStatus,
    /// The stored state. `None` states only the status the source records for
    /// a state it does not store.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_content"
    )]
    pub content: Option<StateContent>,
    /// The source's record of the state, so a writer to the same codec
    /// re-emits the chain from it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// A stored state: its base with a delta applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct StateContent {
    /// The state the delta applies to.
    pub base: StateBase,
    /// The state's difference from its base.
    pub delta: StateDelta,
}

/// The status of stored solved values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum EvaluationStatus {
    /// The source does not flag the values.
    Current,
    /// The values predate an edit of what they depend on.
    Stale,
    /// The source records a failure to compute the values.
    Failed,
}

/// The state a delta applies to. Bases form a forest: no evaluation is its own
/// ancestor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum StateBase {
    /// Nothing.
    Empty,
    /// The extent of the scope's current state at the position.
    Current(Position),
    /// An evaluation with content.
    Evaluation(EvaluationId),
}

/// A state's difference from its base. The state is the base's records, minus
/// `removed`, with each record of `records` inserted or replacing the base's
/// record of the same id; each map sets (`Some`) or removes (`None`) entries
/// relative to the base's.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct StateDelta {
    /// The records the state inserts or replaces.
    pub records: Topology,
    /// The base's records the state does not hold.
    pub removed: Vec<RecordRef>,
    /// The state's name table entries.
    #[serde(with = "cadmpeg_core::distinct_keys::pairs")]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "Vec<(EntityRef, Option<NameEntry>)>")
    )]
    pub names: BTreeMap<EntityRef, Option<NameEntry>>,
    /// The definition that owns a body.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub owners: BTreeMap<BodyId, Option<ProductDefinitionId>>,
    /// The definition that owns an occurrence.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    pub members: BTreeMap<OccurrenceId, Option<ProductDefinitionId>>,
    /// An instance's bodies, keyed by the instance's path.
    #[serde(with = "cadmpeg_core::distinct_keys::pairs")]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "Vec<(OccurrencePath, Option<Vec<BodyId>>)>")
    )]
    pub instance_bodies: BTreeMap<OccurrencePath, Option<Vec<BodyId>>>,
    /// A placed instance's placement in the state, only where it differs from
    /// the placement the state's configuration resolves to.
    #[serde(with = "cadmpeg_core::distinct_keys::pairs")]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "Vec<(OccurrencePath, Option<Transform>)>")
    )]
    pub poses: BTreeMap<OccurrencePath, Option<Transform>>,
}

/// A current state of a scope: a position and the bodies of `CadIr.geometry`
/// that belong to the state there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CurrentState {
    /// The state's position, spelled through its member.
    pub at: Position,
    /// The bodies of `CadIr.geometry` that belong to the state.
    pub bodies: Vec<BodyId>,
    /// A placed instance's placement in the current state where it differs
    /// from the placement its configuration resolves to, keyed by the
    /// instance's path.
    #[serde(with = "cadmpeg_core::distinct_keys::pairs")]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "Vec<(OccurrencePath, Transform)>")
    )]
    pub poses: BTreeMap<OccurrencePath, Transform>,
    /// The status the source records for the state.
    pub status: EvaluationStatus,
    /// The source's record of the state.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

cadmpeg_core::named_optional_field!(deserialize_configuration<T>, T, "configuration");
cadmpeg_core::named_optional_field!(deserialize_content<T>, T, "content");
cadmpeg_core::named_optional_field!(deserialize_native_ref<T>, T, "native_ref");
