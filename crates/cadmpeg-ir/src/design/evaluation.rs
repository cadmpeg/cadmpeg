// SPDX-License-Identifier: Apache-2.0
//! Evaluations and current states: the states of a scope.
//!
//! An evaluation is a state of a scope under one configuration, stored as a
//! base with a delta applied. The current state of every scope is held
//! outside evaluations: its geometry in `CadIr.geometry`, its names in
//! `Design.names`, and its poses in [`CurrentState::poses`].

use std::collections::BTreeMap;

use cadmpeg_core::decode::u64_from_index;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::ids::EvaluationId;
use super::names::NameEntry;
use super::read;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
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
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum StateBase {
    /// Nothing.
    #[serde(deserialize_with = "cadmpeg_core::tag_only::refuse_value")]
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
        schemars(with = "std::collections::BTreeSet<(EntityRef, Option<NameEntry>)>")
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
        schemars(with = "std::collections::BTreeSet<(OccurrencePath, Option<Vec<BodyId>>)>")
    )]
    pub instance_bodies: BTreeMap<OccurrencePath, Option<Vec<BodyId>>>,
    /// A placed instance's placement in the state, only where it differs from
    /// the placement the state's configuration resolves to.
    #[serde(with = "cadmpeg_core::distinct_keys::pairs")]
    #[cfg_attr(
        feature = "schema",
        schemars(with = "std::collections::BTreeSet<(OccurrencePath, Option<Transform>)>")
    )]
    pub poses: BTreeMap<OccurrencePath, Option<Transform>>,
}

/// A state held in full: its records and its name table.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HeldState {
    /// The state's records, each arena sorted by id.
    pub records: Topology,
    /// The state's name table.
    pub names: BTreeMap<EntityRef, NameEntry>,
}

impl StateDelta {
    /// The delta that turns `base` into `state`: each record of `state` that
    /// `base` does not hold with equal content, a reference to each record of
    /// `base` whose id `state` does not hold, each name entry of `state` that
    /// differs from the base's, and `None` for each entity that `base` names
    /// and `state` does not. A record that both hold with equal content is
    /// shared through the base.
    #[must_use]
    pub fn between(state: &HeldState, base: &HeldState) -> Self {
        let (records, removed) = super::edit::states::difference(&state.records, &base.records);
        let mut names = state
            .names
            .iter()
            .filter(|(entity, entry)| base.names.get(*entity) != Some(*entry))
            .map(|(entity, entry)| (entity.clone(), Some(entry.clone())))
            .collect::<BTreeMap<_, _>>();
        names.extend(
            base.names
                .keys()
                .filter(|entity| !state.names.contains_key(*entity))
                .map(|entity| (entity.clone(), None)),
        );
        Self {
            records,
            removed,
            names,
            ..Self::default()
        }
    }
}

/// The states of one kernel index space of a format that chains back from
/// its latest state (§5.4), stated from the latest to the earliest. Each
/// state is based on the state stated before it, and the first on the base
/// of the chain. The chain holds one state in full: the latest it stated.
#[derive(Debug, Clone)]
pub struct ChainBack {
    base: StateBase,
    later: HeldState,
}

impl ChainBack {
    /// A chain whose latest state is based on `base`, the state `held` holds
    /// in full: `Current(p)` with the extent of the current state at `p`, or
    /// an evaluation with its resolved state.
    #[must_use]
    pub fn new(base: StateBase, held: HeldState) -> Self {
        Self { base, later: held }
    }

    /// The content of `state`, the next earlier state of the chain, which
    /// the evaluation `id` holds. `state` is the base of the next state.
    pub fn push(&mut self, id: EvaluationId, state: HeldState) -> StateContent {
        let delta = StateDelta::between(&state, &self.later);
        self.later = state;
        StateContent {
            base: std::mem::replace(&mut self.base, StateBase::Evaluation(id)),
            delta,
        }
    }
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
        schemars(with = "std::collections::BTreeSet<(OccurrencePath, Transform)>")
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

/// Walks every chain of state bases of `evaluations` and refuses one deeper
/// than
/// [`max_state_chain_depth`](cadmpeg_core::decode::ResourceLimits::max_state_chain_depth)
/// or one that is a cycle.
///
/// An evaluation with content has depth one more than the evaluation its
/// `StateBase::Evaluation` base names, or one where its base is another arm or
/// names no evaluation of `evaluations`. An evaluation without content has
/// depth zero. Returns the depth of each evaluation by its id; of two
/// evaluations with one id, the first keeps it.
pub(crate) fn admit_state_chains(
    evaluations: &[Evaluation],
    cycles: Cycles,
) -> Result<BTreeMap<&str, u64>, String> {
    let mut index = BTreeMap::new();
    for (at, evaluation) in evaluations.iter().enumerate() {
        index.entry(evaluation.id.as_str()).or_insert(at);
    }
    let depths = chain_depths(
        evaluations,
        |evaluation| match &evaluation.content {
            None => ChainLink::Unstored,
            Some(StateContent {
                base: StateBase::Evaluation(base),
                ..
            }) => index
                .get(base.as_str())
                .map_or(ChainLink::Below(0), |&base| ChainLink::Within(base)),
            Some(_) => ChainLink::Below(0),
        },
        |evaluation| format!("evaluation {}", evaluation.id.as_str()),
        cycles,
    )?;
    Ok(index.into_iter().map(|(id, at)| (id, depths[at])).collect())
}

/// What a state of one arena rests on in a chain of state bases.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ChainLink {
    /// The state holds no content, and has depth zero.
    Unstored,
    /// The state's base is the state of the arena at this index.
    Within(usize),
    /// The state's base lies outside the arena and has this depth.
    Below(u64),
}

/// What a walk of state bases does at a state it has already passed through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cycles {
    /// Refuses the chain.
    Refuse,
    /// Ends the chain there (§2.5, Resource ceilings); part 10 reports it.
    End,
}

/// The depth of each of the `states` of one arena, whose bases `link`
/// states, refusing a chain deeper than
/// [`max_state_chain_depth`](cadmpeg_core::decode::ResourceLimits::max_state_chain_depth),
/// and one that is a cycle where `cycles` refuses it.
///
/// A state with content has depth one more than its base. `name` names a
/// state in the refusal of a cycle. The walk is iterative, and visits each
/// state once.
pub(crate) fn chain_depths<T>(
    states: &[T],
    link: impl Fn(&T) -> ChainLink,
    name: impl Fn(&T) -> String,
    cycles: Cycles,
) -> Result<Vec<u64>, String> {
    #[derive(Clone, Copy)]
    enum Mark {
        Unseen,
        OnChain,
        Depth(u64),
    }

    let mut marks = vec![Mark::Unseen; states.len()];
    let mut chain = Vec::new();
    for start in 0..states.len() {
        let mut at = start;
        let tail = loop {
            match marks[at] {
                Mark::Depth(depth) => break depth,
                Mark::OnChain if cycles == Cycles::End => break 0,
                Mark::OnChain => {
                    return Err(format!(
                        "{} is its own ancestor through its state bases",
                        name(&states[at])
                    ));
                }
                Mark::Unseen => {}
            }
            let base = match link(&states[at]) {
                ChainLink::Unstored => {
                    marks[at] = Mark::Depth(0);
                    break 0;
                }
                ChainLink::Within(base) => Ok(base),
                ChainLink::Below(depth) => Err(depth),
            };
            marks[at] = Mark::OnChain;
            chain.push(at);
            read::within_state_chain(u64_from_index(chain.len()))?;
            match base {
                Ok(base) => at = base,
                Err(depth) => break depth,
            }
        };
        let mut depth = tail;
        for at in chain.drain(..).rev() {
            depth = depth
                .checked_add(1)
                .ok_or("the depth of a chain of state bases overflows")?;
            marks[at] = Mark::Depth(depth);
        }
        read::within_state_chain(depth)?;
    }
    Ok(marks
        .into_iter()
        .map(|mark| match mark {
            Mark::Depth(depth) => depth,
            Mark::Unseen | Mark::OnChain => 0,
        })
        .collect())
}

#[cfg(test)]
mod tests;

cadmpeg_core::named_optional_field!(deserialize_configuration<T>, T, "configuration");
cadmpeg_core::named_optional_field!(deserialize_content<T>, T, "content");
cadmpeg_core::named_optional_field!(deserialize_native_ref<T>, T, "native_ref");
