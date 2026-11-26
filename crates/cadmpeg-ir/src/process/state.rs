// SPDX-License-Identifier: Apache-2.0
//! Process states (§7.9).
//!
//! A process state is stored workpiece geometry along the process (ISO 14649
//! `as_is`, `to_be`, `removal`). States use the storage of evaluations: a
//! base with a delta applied, sharing records by id. Bases form a forest.

use std::collections::BTreeMap;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::design::evaluation::{self, ChainLink, EvaluationStatus, StateBase, StateDelta};
use crate::design::ids::{OperationId, ProcessStateId, SetupId};
use crate::features::{ConfigurationId, FeatureId};

/// Stored workpiece geometry at one point of the process.
///
/// At most one state per (`of`, `configuration`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ProcessState {
    /// The state's identity.
    pub id: ProcessStateId,
    /// The point of the process the state holds the workpiece at.
    pub of: ProcessStateOf,
    /// The configuration of the setup's scope the state is of. `None` states
    /// the state of every configuration.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_configuration"
    )]
    pub configuration: Option<ConfigurationId>,
    /// The status the source records for the state.
    pub status: EvaluationStatus,
    /// The stored state. `None` states only the status the source records
    /// for a state whose geometry it does not store.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_content"
    )]
    pub content: Option<ProcessContent>,
    /// The design feature whose result is the volume the operation removes,
    /// on a `Removal` state of a source whose material-removal result is a
    /// design feature.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_feature"
    )]
    pub feature: Option<FeatureId>,
}

/// A stored process state: its base with a delta applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ProcessContent {
    /// The state the delta applies to.
    pub base: ProcessBase,
    /// The state's difference from its base.
    pub delta: StateDelta,
}

/// The point of the process that a process state holds the workpiece at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ProcessStateOf {
    /// The setup's stock solid before its first step.
    Stock(SetupId),
    /// The workpiece after the operation: ISO `to_be` of the operation, and
    /// `as_is` of the next operation in machining order.
    After(OperationId),
    /// The volume the operation removes.
    Removal(OperationId),
}

/// The state a process state's delta applies to.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ProcessBase {
    /// A design state: `Empty`, a current state of the setup's scope, or an
    /// evaluation with content.
    Design(StateBase),
    /// Another process state with content.
    State(ProcessStateId),
}

/// Walks every chain of state bases of `states` and refuses one deeper than
/// the state chain ceiling or one that is a cycle.
///
/// A chain continues through a design evaluation that a `ProcessBase::Design`
/// base names: `evaluations` holds the depth of each evaluation by its id. A
/// state with content has depth one more than its base: the process state a
/// `ProcessBase::State` base names, or that evaluation. A base that names no
/// record of either, or another design arm, has depth zero.
pub(crate) fn admit_state_chains(
    states: &[ProcessState],
    evaluations: &BTreeMap<&str, u64>,
    cycles: evaluation::Cycles,
) -> Result<(), String> {
    let mut index = BTreeMap::new();
    for (at, state) in states.iter().enumerate() {
        index.entry(state.id.as_str()).or_insert(at);
    }
    evaluation::chain_depths(
        states,
        |state| match &state.content {
            None => ChainLink::Unstored,
            Some(ProcessContent {
                base: ProcessBase::State(base),
                ..
            }) => index
                .get(base.as_str())
                .map_or(ChainLink::Below(0), |&base| ChainLink::Within(base)),
            Some(ProcessContent {
                base: ProcessBase::Design(StateBase::Evaluation(base)),
                ..
            }) => ChainLink::Below(evaluations.get(base.as_str()).copied().unwrap_or(0)),
            Some(ProcessContent {
                base: ProcessBase::Design(StateBase::Empty | StateBase::Current(_)),
                ..
            }) => ChainLink::Below(0),
        },
        |state| format!("process state {}", state.id.as_str()),
        cycles,
    )?;
    Ok(())
}

cadmpeg_core::named_optional_field!(deserialize_configuration, ConfigurationId, "configuration");
cadmpeg_core::named_optional_field!(deserialize_content, ProcessContent, "content");
cadmpeg_core::named_optional_field!(deserialize_feature, FeatureId, "feature");
