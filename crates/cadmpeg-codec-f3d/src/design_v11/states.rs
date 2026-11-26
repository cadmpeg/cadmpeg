// SPDX-License-Identifier: Apache-2.0
//! The stored states between the features of a Fusion timeline, as
//! evaluations of the root scope (§5.4).
//!
//! The scope tail of a timeline item stores the id of the ASM history state
//! after the item, and the id of the state before it. Each model blob with a
//! history partition stores one chain of states, numbered on a counter of
//! its own. The state of an item lies in the one history that stores its id,
//! or, where several histories store it, in the history that the item's
//! parameter scope binds. The states of the items of one history tile its
//! chain: from the head back, the state after each item is the state before
//! the next later item of that history. The state after an item holds, of
//! each history, the state that this walk reaches at the item, with the
//! consumed bodies of the head whose consumer is later in the timeline
//! (§5.5).
//!
//! An item that builds the current geometry and that is not stateless has an
//! evaluation of the state after it, whose base is the state after the next
//! later such item, and `Current` at the scope's current position for the
//! last of them. An item that stores no state id holds the state after the
//! nearest earlier item; where that item has an evaluation of the same
//! state, the item's evaluation has that base and an empty delta. The
//! current position has no evaluation. The walk stops at the first item
//! whose state it does not place, and a loss names each feature whose state
//! it leaves unstated.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::edit::state_view::ResolvedState;
use cadmpeg_ir::design::evaluation::{
    Evaluation, EvaluationStatus, StateBase, StateContent, StateDelta,
};
use cadmpeg_ir::design::ids::EvaluationId;
use cadmpeg_ir::design::refs::{EntityRef, Evaluated, Member, Position, RecordRef};
use cadmpeg_ir::document::{CadIr, Topology};
use cadmpeg_ir::features::{ConfigurationId, FeatureId};
use cadmpeg_ir::ids::{BodyId, Identity, ProductDefinitionId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::schema::{ArenaRecord, EntitySchema};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{RetainedSourceRecord, SourceFidelity};

use crate::container::ContainerScan;
use crate::history::face_resolution::historical_brep_source;
use crate::history::stored_states::{self, Blob, Read, StateRecords, Want};
use crate::loss::F3dLossCode;
use crate::records::bodies::DesignBodyBinding;
use crate::records::feature::scope::DesignScopeTailFlags;

use super::timeline::document_prefix;
use super::StoredStates;

#[cfg(test)]
mod tests;

/// The native arena that holds the body-map pairs.
const BINDINGS: &str = "design_body_bindings";

/// Calls `$apply!` with every arena of `Topology` and its `RecordRef` arm.
macro_rules! topology_arenas {
    ($apply:ident) => {
        $apply! {
            bodies Body, regions Region, shells Shell, faces Face, loops Loop,
            coedges Coedge, edges Edge, vertices Vertex, points Point,
            surfaces Surface, curves Curve, subds Subd, pcurves Pcurve,
            procedural_surfaces ProceduralSurface, procedural_curves ProceduralCurve
        }
    };
}

/// The root scope whose stored states are read.
pub(super) struct Scope {
    pub(super) definition: ProductDefinitionId,
    /// The position of its current state.
    pub(super) at: Position,
    /// The bodies of its current state.
    pub(super) bodies: BTreeSet<BodyId>,
    /// The members of its first timeline.
    pub(super) members: Vec<Member>,
    /// The stream and scope record of the item of each feature.
    pub(super) items: BTreeMap<FeatureId, (String, u64)>,
}

/// The state fields of the scope tail of one timeline item.
#[derive(Clone, Copy)]
pub(super) struct Tail<'a> {
    /// The id of the history state after the item.
    pub(super) state: Option<i64>,
    /// The id of the history state before the item.
    pub(super) previous: Option<i64>,
    pub(super) flags: Option<DesignScopeTailFlags>,
    /// The ASM history that the item's parameter scope binds.
    pub(super) history: Option<&'a str>,
}

/// The scope tails of the timeline items, keyed by stream and record index.
pub(super) type Tails<'a> = HashMap<(&'a str, u64), Tail<'a>>;

/// Pushes to `ir` the evaluations of the stored states of `scope`, and to
/// `losses` a note for each state it does not state. Returns the native
/// records that the evaluations reference and the document does not hold.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    sources: &StoredStates<'_>,
    scope: &Scope,
    tails: &Tails<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<UnknownRecord>, CodecError> {
    let mut evaluations = Vec::new();
    let mut carried = Vec::new();
    let mut reader = Reader {
        ctx,
        ir,
        fidelity: sources.fidelity,
        scope,
        tails,
        losses,
    };
    if let Err(reason) = reader.read(sources.scan, &mut evaluations, &mut carried)? {
        losses.push(F3dLossCode::HistoryStatesUnread.note(format!(
            "scope {}: the states after its timeline items are not stated: {reason}.",
            scope.definition
        )));
    }
    ctx.charge_entities(
        u64_from_index(evaluations.len()),
        "project F3D v11 history states",
    )?;
    ir.design.evaluations.extend(evaluations);
    Ok(carried)
}

/// Where the state after a timeline item lies.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    /// The item stores no state id.
    NoState,
    /// The state `state` of history `history`, whose state before the item
    /// is `previous`.
    Bound {
        history: usize,
        state: i64,
        previous: Option<i64>,
    },
    /// The state id is not placed in one history, for this reason.
    Unresolved(String),
}

/// A built timeline item of a feature of the root document.
#[derive(Debug)]
struct Entry<'a> {
    feature: &'a FeatureId,
    stream: &'a str,
    record: u64,
    /// The index of its member in the timeline.
    position: usize,
    stateless: bool,
    /// The item is at the current position.
    current: bool,
    kind: Kind,
}

impl Entry<'_> {
    /// The item has an evaluation of the state after it.
    fn evaluated(&self) -> bool {
        !self.stateless && !self.current
    }
}

/// An item with an evaluation, and the state of each history after it.
#[derive(Debug, PartialEq, Eq)]
struct Planned {
    entry: usize,
    targets: Vec<Option<i64>>,
}

/// Why the walk stops, and the features whose states it leaves unstated.
#[derive(Debug, PartialEq, Eq)]
struct Stop {
    reason: String,
    features: Vec<FeatureId>,
}

/// The items with evaluations, latest first, and where the walk stops.
#[derive(Debug, PartialEq, Eq)]
struct Plan {
    planned: Vec<Planned>,
    stop: Option<Stop>,
}

/// The states after `entries`, walking back from the `heads` of the
/// histories named `names`.
fn plan(entries: &[Entry<'_>], heads: &[Option<i64>], names: &[&str]) -> Plan {
    let mut at = heads.to_vec();
    let mut set = vec![0_usize; heads.len()];
    let mut planned = Vec::<Planned>::new();
    for (index, entry) in entries.iter().enumerate().rev() {
        let earlier = entries[..index]
            .iter()
            .filter(|entry| entry.evaluated())
            .map(|entry| entry.feature.clone());
        if let Kind::Bound { history, state, .. } = entry.kind {
            if at[history] != Some(state) {
                let left = at[history]
                    .map_or_else(|| "no state".to_owned(), |state| format!("state {state}"));
                let features = planned
                    .drain(set[history]..)
                    .map(|planned| entries[planned.entry].feature.clone())
                    .chain(entry.evaluated().then(|| entry.feature.clone()))
                    .chain(earlier)
                    .collect();
                return Plan {
                    planned,
                    stop: Some(Stop {
                        reason: format!(
                            "feature {} stores state {state} of {}, and the items after it leave that history at {left}",
                            entry.feature, names[history]
                        ),
                        features,
                    }),
                };
            }
        }
        if entry.evaluated() {
            planned.push(Planned {
                entry: index,
                targets: at.clone(),
            });
        }
        match &entry.kind {
            Kind::Bound {
                history, previous, ..
            } => {
                at[*history] = *previous;
                set[*history] = planned.len();
            }
            Kind::NoState => {}
            Kind::Unresolved(_) if entry.stateless => {}
            Kind::Unresolved(reason) => {
                let features = earlier.collect::<Vec<_>>();
                return Plan {
                    planned,
                    stop: (!features.is_empty()).then(|| Stop {
                        reason: format!(
                            "the state of feature {} is not placed: {reason}",
                            entry.feature
                        ),
                        features,
                    }),
                };
            }
        }
    }
    Plan {
        planned,
        stop: None,
    }
}

/// The consumed bodies of the head of one history.
#[derive(Default)]
struct Consumed {
    /// The timeline index of the first consumer of each body, by body-map
    /// key.
    placed: BTreeMap<u64, usize>,
    /// The keys of the bodies that no timeline item consumes.
    unplaced: BTreeSet<u64>,
}

/// The state of each history after each item of `planned`, with the
/// consumed bodies whose consumer is later in the timeline.
fn wants(
    entries: &[Entry<'_>],
    planned: &[Planned],
    consumed: &[Consumed],
) -> Vec<Vec<Option<Want>>> {
    planned
        .iter()
        .map(|planned| {
            let position = entries[planned.entry].position;
            planned
                .targets
                .iter()
                .zip(consumed)
                .map(|(target, consumed)| {
                    target.map(|state| Want {
                        state,
                        consumed: consumed
                            .placed
                            .iter()
                            .filter(|(_, consumer)| **consumer > position)
                            .map(|(key, _)| *key)
                            .collect(),
                    })
                })
                .collect()
        })
        .collect()
}

/// The loss of the states that `stop` leaves unstated, if any.
fn stopped(definition: &ProductDefinitionId, stop: &Stop) -> Option<LossNote> {
    let mut features = stop
        .features
        .iter()
        .map(FeatureId::as_str)
        .collect::<Vec<_>>();
    features.sort_unstable();
    features.dedup();
    (!features.is_empty()).then(|| {
        F3dLossCode::HistoryStateUnread.note(format!(
            "scope {definition}: the states after features {} are not stated: {}.",
            features.join(", "),
            stop.reason
        ))
    })
}

/// The loss of the bodies of the head of the history `stream` that no
/// timeline item consumes, if any.
fn omitted(
    definition: &ProductDefinitionId,
    stream: &str,
    consumed: &Consumed,
    read: &Read,
) -> Option<LossNote> {
    let bodies = consumed
        .unplaced
        .iter()
        .filter_map(|key| read.head_consumed.get(key))
        .map(BodyId::as_str)
        .collect::<BTreeSet<_>>();
    (!bodies.is_empty()).then(|| {
        F3dLossCode::HistoryStateBodyOmitted.note(format!(
            "scope {definition}: the states after its timeline items omit the bodies {} of the head of {stream}: the current geometry omits them, and no timeline item consumes them.",
            bodies.into_iter().collect::<Vec<_>>().join(", ")
        ))
    })
}

/// One model blob with a history partition.
struct History<'s> {
    blob: Blob<'s>,
    /// The basename of the blob.
    name: &'s str,
    /// The prefix of the record ids of the blob.
    space: String,
    /// The native id of each stored state, by state id.
    states: BTreeMap<i64, String>,
}

/// The items of a timeline, their states, and the stored states read.
struct Chain<'c, 'e> {
    entries: &'c [Entry<'e>],
    planned: &'c [Planned],
    wants: &'c [Vec<Option<Want>>],
    histories: &'c [History<'c>],
    heads: &'c [Option<i64>],
    reads: &'c [Result<Read, String>],
    /// The current bodies of each history.
    held: &'c [BTreeSet<BodyId>],
    current: &'c ResolvedState,
}

impl Chain<'_, '_> {
    /// The stop at the item `index` of the plan, which leaves its state, the
    /// states of the earlier items, and the states of `leaves` unstated.
    fn stop(&self, index: usize, leaves: &[(usize, usize)], reason: String) -> Stop {
        Stop {
            reason,
            features: self.planned[index..]
                .iter()
                .map(|planned| planned.entry)
                .chain(leaves.iter().map(|(leaf, _)| self.planned[*leaf].entry))
                .map(|entry| self.entries[entry].feature.clone())
                .collect(),
        }
    }
}

/// What reads the stored states of one scope.
struct Reader<'r, 'c> {
    ctx: &'r DecodeContext<'c>,
    ir: &'r CadIr,
    fidelity: &'r SourceFidelity,
    scope: &'r Scope,
    tails: &'r Tails<'r>,
    losses: &'r mut Vec<LossNote>,
}

impl<'r> Reader<'r, '_> {
    /// Pushes to `out` the evaluations of the scope's stored states, and to
    /// `carried` the native records they reference that the document does
    /// not hold. The inner error says why no state of the scope is stated.
    fn read(
        &mut self,
        scan: &ContainerScan<'_>,
        out: &mut Vec<Evaluation>,
        carried: &mut Vec<UnknownRecord>,
    ) -> Result<Result<(), String>, CodecError> {
        let scope = self.scope;
        let ir = self.ir;
        let configuration = if ir
            .design
            .configurations
            .iter()
            .any(|configuration| configuration.scope == scope.definition)
        {
            let active = ir
                .design
                .scopes
                .iter()
                .find(|design| design.definition == scope.definition)
                .and_then(|design| design.active_configuration.clone());
            let Some(active) = active else {
                return Ok(Err(
                    "the scope has configurations and no active one".to_owned()
                ));
            };
            Some(active)
        } else {
            None
        };
        let built = match &scope.at {
            Position::Start(_) => return Ok(Ok(())),
            Position::After(member) => {
                match scope.members.iter().position(|found| found == member) {
                    Some(at) => &scope.members[..=at],
                    None => return Ok(Ok(())),
                }
            }
        };
        let histories = match histories(scan)? {
            Ok(histories) => histories,
            Err(reason) => return Ok(Err(reason)),
        };
        let current = ir
            .resolved_current_state(&scope.definition, &scope.at)
            .map_err(|error| {
                CodecError::malformed(format_args!(
                    "F3D scope {} has no current state: {error}",
                    scope.definition
                ))
            })?;
        let mut held = vec![BTreeSet::new(); histories.len()];
        for body in &scope.bodies {
            let Some(at) = histories
                .iter()
                .position(|history| body.as_str().starts_with(&history.space))
            else {
                return Ok(Err(format!(
                    "the current body {body} lies in no model blob with a history"
                )));
            };
            held[at].insert(body.clone());
        }
        let mut entries = Vec::new();
        for (position, member) in built.iter().enumerate() {
            let current = position + 1 == built.len();
            if let Some(entry) = self.entry(
                member,
                position,
                current,
                &histories,
                configuration.as_ref(),
            )? {
                entries.push(entry);
            }
        }
        let heads = histories
            .iter()
            .map(|history| history.states.keys().next_back().copied())
            .collect::<Vec<_>>();
        let names = histories
            .iter()
            .map(|history| history.blob.stream)
            .collect::<Vec<_>>();
        let Plan { planned, stop } = plan(&entries, &heads, &names);
        self.losses.extend(
            stop.as_ref()
                .and_then(|stop| stopped(&scope.definition, stop)),
        );
        if planned.is_empty() {
            return Ok(Ok(()));
        }
        let consumed = self.consumed(&histories)?;
        let wants = wants(&entries, &planned, &consumed);
        let reads = self.reads(&histories, &held, &heads, &consumed, &wants, &current)?;
        for ((history, consumed), read) in histories.iter().zip(&consumed).zip(&reads) {
            if let Ok(read) = read {
                self.losses.extend(omitted(
                    &scope.definition,
                    history.blob.stream,
                    consumed,
                    read,
                ));
            }
        }
        let chain = Chain {
            entries: &entries,
            planned: &planned,
            wants: &wants,
            histories: &histories,
            heads: &heads,
            reads: &reads,
            held: &held,
            current: &current,
        };
        if let Some(stop) = self.emit(&chain, configuration.as_ref(), out, carried)? {
            self.losses.extend(stopped(&scope.definition, &stop));
        }
        Ok(Ok(()))
    }

    /// The entry of a member that is a feature of the root document.
    fn entry(
        &self,
        member: &'r Member,
        position: usize,
        current: bool,
        histories: &[History<'_>],
        configuration: Option<&ConfigurationId>,
    ) -> Result<Option<Entry<'r>>, CodecError> {
        let Member::Feature(feature) = member else {
            return Ok(None);
        };
        let Some((stream, record)) = self.scope.items.get(feature) else {
            return Ok(None);
        };
        if !document_prefix(stream).is_empty() {
            return Ok(None);
        }
        let stateless = self
            .ir
            .is_stateless(feature, configuration)
            .map_err(|error| {
                CodecError::malformed(format_args!(
                    "F3D feature {feature} has no stateless reading: {error}"
                ))
            })?;
        let tail = self.tails.get(&(stream.as_str(), *record)).copied();
        let kind = match tail.and_then(|tail| Some((tail, tail.state?))) {
            None => Kind::NoState,
            Some((tail, state)) if tail.flags.is_some_and(|flags| flags.no_result) => {
                Kind::Unresolved(format!(
                    "its scope tail flags state {state} as holding no current result"
                ))
            }
            Some((tail, state)) => bind(histories, &tail, state),
        };
        Ok(Some(Entry {
            feature,
            stream: stream.as_str(),
            record: *record,
            position,
            stateless,
            current,
            kind,
        }))
    }

    /// The consumed bodies of the head of each of `histories`: the
    /// history-only bodies of the root document's body maps that name its
    /// blob.
    fn consumed(&self, histories: &[History<'_>]) -> Result<Vec<Consumed>, CodecError> {
        let mut out = histories
            .iter()
            .map(|_| Consumed::default())
            .collect::<Vec<_>>();
        let Some(namespace) = self.ir.native.namespace("f3d") else {
            return Ok(out);
        };
        let bindings = namespace.arena_as_for_decode::<DesignBodyBinding>(self.ctx, BINDINGS)?;
        let positions = self
            .scope
            .members
            .iter()
            .enumerate()
            .filter_map(|(position, member)| {
                let Member::Feature(feature) = member else {
                    return None;
                };
                let (stream, record) = self.scope.items.get(feature)?;
                Some(((stream.as_str(), *record), position))
            })
            .collect::<HashMap<_, _>>();
        for binding in bindings.iter().filter(|binding| binding.history_only) {
            let Some(stream) = crate::ids::native_stream(binding.id()) else {
                continue;
            };
            if !document_prefix(stream).is_empty() {
                continue;
            }
            let Some(at) = histories
                .iter()
                .position(|history| history.name == binding.blob_name())
            else {
                continue;
            };
            let consumer = binding
                .consumers
                .iter()
                .filter_map(|consumer| positions.get(&(stream, *consumer)).copied())
                .min();
            match consumer {
                Some(consumer) => {
                    let first = out[at]
                        .placed
                        .entry(binding.asm_body_key)
                        .or_insert(consumer);
                    *first = (*first).min(consumer);
                }
                None => {
                    out[at].unplaced.insert(binding.asm_body_key);
                }
            }
        }
        for consumed in &mut out {
            let placed = &consumed.placed;
            consumed.unplaced.retain(|key| !placed.contains_key(key));
        }
        Ok(out)
    }

    /// The stored states of each of `histories` that `wants` names, or why
    /// they are not read.
    fn reads(
        &self,
        histories: &[History<'_>],
        held: &[BTreeSet<BodyId>],
        heads: &[Option<i64>],
        consumed: &[Consumed],
        wants: &[Vec<Option<Want>>],
        current: &ResolvedState,
    ) -> Result<Vec<Result<Read, String>>, CodecError> {
        let scaled = histories
            .iter()
            .try_fold(0_usize, |total, history| {
                total.checked_add(history.blob.bytes.len())
            })
            .and_then(|bytes| bytes.checked_mul(stored_states::STATE_WORK_PER_BYTE))
            .ok_or_else(|| {
                CodecError::malformed("F3D model blobs with a history exceed the state work range")
            })?;
        let work = WorkBudget::new(stored_states::MAX_STATE_WORK.max(scaled));
        let mut out = Vec::with_capacity(histories.len());
        for (index, history) in histories.iter().enumerate() {
            let head = heads[index].map(|state| Want {
                state,
                consumed: BTreeSet::new(),
            });
            if wants.iter().all(|wants| wants[index] == head) {
                out.push(Err("its states are not read".to_owned()));
                continue;
            }
            let wanted = wants
                .iter()
                .filter_map(|wants| wants[index].clone())
                .filter(|want| Some(want) != head.as_ref())
                .collect();
            let keys = consumed[index]
                .placed
                .keys()
                .chain(&consumed[index].unplaced)
                .copied()
                .collect();
            let read = match stored_states::read(
                self.ctx,
                &history.blob,
                &held[index],
                &keys,
                &wanted,
                &self.ir.tolerances,
                &work,
            )? {
                Ok(read) => read,
                Err(reason) => {
                    out.push(Err(reason));
                    continue;
                }
            };
            if Some(read.head) != heads[index] {
                out.push(Err(format!(
                    "its walk starts at state {}, which is not its latest state",
                    read.head
                )));
                continue;
            }
            if ids(&read.head_records, None) != ids(current.records(), Some(&history.space)) {
                out.push(Err(
                    "the records of its head differ from the current geometry".to_owned(),
                ));
                continue;
            }
            out.push(Ok(read));
        }
        Ok(out)
    }

    /// Pushes to `out` the evaluation of the state after each item of the
    /// plan, latest first, each based on the one before it. Returns where
    /// the chain stops.
    fn emit(
        &mut self,
        chain: &Chain<'_, '_>,
        configuration: Option<&ConfigurationId>,
        out: &mut Vec<Evaluation>,
        carried: &mut Vec<UnknownRecord>,
    ) -> Result<Option<Stop>, CodecError> {
        let scope = self.scope;
        let current = chain.current;
        let empty = Topology::default();
        let owners = chain
            .held
            .iter()
            .map(|bodies| {
                let mut found = bodies
                    .iter()
                    .filter_map(|body| current.owners().get(body))
                    .collect::<BTreeSet<_>>()
                    .into_iter();
                let owner = found.next();
                owner.filter(|_| found.next().is_none()).cloned()
            })
            .collect::<Vec<_>>();
        let unstepped = ids(current.records(), None)
            .into_iter()
            .filter(|id| {
                !chain
                    .histories
                    .iter()
                    .any(|history| id.starts_with(&history.space))
            })
            .collect::<BTreeSet<_>>();
        let mut previous = chain
            .heads
            .iter()
            .zip(chain.reads)
            .map(|(head, read)| {
                (
                    head.map(|state| Want {
                        state,
                        consumed: BTreeSet::new(),
                    }),
                    read.as_ref().map_or(&empty, |read| &read.head_records),
                )
            })
            .collect::<Vec<_>>();
        let mut entries = current
            .owners()
            .iter()
            .filter(|(_, owner)| **owner != scope.definition)
            .map(|(body, owner)| (body.clone(), owner.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut named = current.names().keys().cloned().collect::<BTreeSet<_>>();
        let limit = self.ctx.policy().limits.max_state_chain_depth;
        let mut depth = 0_u64;
        let mut base = StateBase::Current(scope.at.clone());
        let mut leaves = Vec::<(usize, usize)>::new();
        for (index, planned) in chain.planned.iter().enumerate() {
            let entry = &chain.entries[planned.entry];
            if entry.kind == Kind::NoState {
                let earlier = chain.planned[index + 1..]
                    .iter()
                    .position(|planned| chain.entries[planned.entry].kind != Kind::NoState)
                    .map(|offset| index + 1 + offset);
                if let Some(earlier) =
                    earlier.filter(|earlier| chain.wants[*earlier] == chain.wants[index])
                {
                    leaves.push((index, earlier));
                    continue;
                }
            }
            if depth >= limit {
                return Ok(Some(chain.stop(
                    index,
                    &leaves,
                    format!("the state chain ceiling {limit} is reached"),
                )));
            }
            let mut delta = StateDelta::default();
            let mut sources = Vec::<&StateRecords>::new();
            for (at, target) in chain.wants[index].iter().enumerate() {
                let records = previous[at].1;
                if previous[at].0 == *target {
                    continue;
                }
                let history = &chain.histories[at];
                let read = match &chain.reads[at] {
                    Ok(read) => read,
                    Err(reason) => {
                        return Ok(Some(chain.stop(
                            index,
                            &leaves,
                            format!("the history of {}: {reason}", history.blob.stream),
                        )))
                    }
                };
                let next = match target {
                    None => &empty,
                    Some(want) if want.state == read.head && want.consumed.is_empty() => {
                        &read.head_records
                    }
                    Some(want) => match read.states.iter().find(|(found, _)| found == want) {
                        Some((_, Ok(next))) => {
                            sources.push(next);
                            &next.records
                        }
                        Some((_, Err(reason))) => {
                            return Ok(Some(chain.stop(
                                index,
                                &leaves,
                                format!(
                                    "state {} of {}: {reason}",
                                    want.state, history.blob.stream
                                ),
                            )))
                        }
                        None => {
                            return Ok(Some(chain.stop(
                                index,
                                &leaves,
                                format!(
                                    "state {} of {} is not read",
                                    want.state, history.blob.stream
                                ),
                            )))
                        }
                    },
                };
                if let Err(reason) = step(
                    records,
                    next,
                    &read.head_records,
                    current.records(),
                    &mut delta,
                ) {
                    return Ok(Some(chain.stop(
                        index,
                        &leaves,
                        format!(
                            "state {} of {}: {reason}",
                            target.as_ref().map_or_else(
                                || "empty".to_owned(),
                                |target| target.state.to_string()
                            ),
                            history.blob.stream
                        ),
                    )));
                }
                let before = records
                    .bodies
                    .iter()
                    .map(|body| &body.id)
                    .collect::<HashSet<_>>();
                let after = next
                    .bodies
                    .iter()
                    .map(|body| &body.id)
                    .collect::<HashSet<_>>();
                for body in records
                    .bodies
                    .iter()
                    .filter(|body| !after.contains(&body.id))
                {
                    if entries.remove(&body.id).is_some() {
                        delta.owners.insert(body.id.clone(), None);
                    }
                }
                for body in next.bodies.iter().filter(|body| !before.contains(&body.id)) {
                    let owner = if chain.held[at].contains(&body.id) {
                        current.owners().get(&body.id)
                    } else {
                        owners[at].as_ref()
                    };
                    let Some(owner) = owner else {
                        return Ok(Some(chain.stop(
                            index,
                            &leaves,
                            format!(
                                "the owner of body {} of {} is not known",
                                body.id, history.blob.stream
                            ),
                        )));
                    };
                    if *owner != scope.definition {
                        delta.owners.insert(body.id.clone(), Some(owner.clone()));
                        entries.insert(body.id.clone(), owner.clone());
                    }
                }
                previous[at] = (target.clone(), next);
            }
            if let Err(reason) = self.carry(&delta, &sources, carried)? {
                return Ok(Some(chain.stop(index, &leaves, reason)));
            }
            let id = evaluation_id(entry.stream, entry.record)?;
            if !current.names().is_empty() {
                let stepped = previous
                    .iter()
                    .map(|(_, records)| ids(records, None))
                    .collect::<Vec<_>>();
                let holds = |id: &str| {
                    unstepped.contains(id) || stepped.iter().any(|records| records.contains(id))
                };
                self.names(&mut delta, &mut named, current, &holds, &id);
            }
            sort(&mut delta.records);
            self.ctx.charge_entities(
                u64_from_index(len(&delta.records) + delta.removed.len()),
                "project F3D v11 history state records",
            )?;
            out.push(Evaluation {
                id: id.clone(),
                scope: scope.definition.clone(),
                configuration: configuration.cloned(),
                of: Evaluated::State(Position::After(Member::Feature(entry.feature.clone()))),
                status: EvaluationStatus::Current,
                content: Some(StateContent {
                    base: std::mem::replace(&mut base, StateBase::Evaluation(id.clone())),
                    delta,
                }),
                native_ref: match entry.kind {
                    Kind::Bound { history, state, .. } => {
                        chain.histories[history].states.get(&state).cloned()
                    }
                    Kind::NoState | Kind::Unresolved(_) => None,
                },
            });
            depth += 1;
            let (flushed, kept) = leaves
                .into_iter()
                .partition::<Vec<_>, _>(|(_, earlier)| *earlier == index);
            leaves = kept;
            if !flushed.is_empty() && depth >= limit {
                return Ok(Some(chain.stop(
                    index + 1,
                    &flushed,
                    format!("the state chain ceiling {limit} is reached"),
                )));
            }
            for (leaf, _) in flushed {
                let leaf = &chain.entries[chain.planned[leaf].entry];
                out.push(Evaluation {
                    id: evaluation_id(leaf.stream, leaf.record)?,
                    scope: scope.definition.clone(),
                    configuration: configuration.cloned(),
                    of: Evaluated::State(Position::After(Member::Feature(leaf.feature.clone()))),
                    status: EvaluationStatus::Current,
                    content: Some(StateContent {
                        base: StateBase::Evaluation(id.clone()),
                        delta: StateDelta::default(),
                    }),
                    native_ref: None,
                });
            }
        }
        Ok(None)
    }

    /// Adds to `carried` each native record of `sources` that a record of
    /// `delta` references and that neither the document nor `carried`
    /// holds. The inner error names a native record whose bytes differ from
    /// those of the record with its id.
    fn carry(
        &self,
        delta: &StateDelta,
        sources: &[&StateRecords],
        carried: &mut Vec<UnknownRecord>,
    ) -> Result<Result<(), String>, CodecError> {
        let held = sources
            .iter()
            .flat_map(|source| &source.unknowns)
            .map(|unknown| (unknown.id().as_str(), unknown))
            .collect::<HashMap<_, _>>();
        if held.is_empty() {
            return Ok(Ok(()));
        }
        let mut referenced = BTreeSet::new();
        macro_rules! visit {
            ($($field:ident $arm:ident),*) => {$(
                for record in &delta.records.$field {
                    record.visit_references(self.ctx, &mut |target| {
                        if held.contains_key(target) {
                            referenced.insert(target.to_owned());
                        }
                        Ok(())
                    })?;
                }
            )*};
        }
        topology_arenas!(visit);
        let native = self
            .ir
            .native
            .namespace("f3d")
            .and_then(|namespace| namespace.arenas().get("unknowns"))
            .into_iter()
            .flatten()
            .map(cadmpeg_ir::native::NativeRecord::id)
            .collect::<HashSet<_>>();
        let mut document = None::<BTreeSet<&str>>;
        for id in referenced {
            let unknown = held[id.as_str()];
            if let Some(retained) = self.fidelity.retained_record(&id) {
                if same_bytes(retained, unknown) {
                    continue;
                }
                return Ok(Err(format!(
                    "its native record {id} differs from the document's record {id}"
                )));
            }
            if native.contains(id.as_str()) {
                return Ok(Err(format!(
                    "the document holds its native record {id} without its bytes"
                )));
            }
            if let Some(earlier) = carried.iter().find(|earlier| earlier.id() == unknown.id()) {
                if earlier.data().is_some() && earlier.data() == unknown.data() {
                    continue;
                }
                return Ok(Err(format!(
                    "two states hold the native record {id} with different bytes"
                )));
            }
            for link in unknown.links() {
                let document = document.get_or_insert_with(|| ids(&self.ir.geometry, None));
                if !(document.contains(link.as_str())
                    || native.contains(link.as_str())
                    || self.fidelity.retained_record(link).is_some())
                {
                    return Ok(Err(format!(
                        "its native record {id} links {link}, which the document does not hold"
                    )));
                }
            }
            carried.push(unknown.clone());
        }
        Ok(Ok(()))
    }

    /// Sets in `delta` the current name of each entity that the state holds
    /// and its base does not name, and removes the name of each entity that
    /// the base names and the state does not hold. A loss names each named
    /// entity whose record the state changes.
    fn names(
        &mut self,
        delta: &mut StateDelta,
        named: &mut BTreeSet<EntityRef>,
        current: &ResolvedState,
        holds: &dyn Fn(&str) -> bool,
        evaluation: &EvaluationId,
    ) {
        let changed = ids(&delta.records, None);
        for (entity, entry) in current.names() {
            let held = holds(entity.id());
            if held && !named.contains(entity) {
                delta.names.insert(entity.clone(), Some(entry.clone()));
                named.insert(entity.clone());
            } else if !held && named.remove(entity) {
                delta.names.insert(entity.clone(), None);
            }
            if held && changed.contains(entity.id()) {
                self.losses.push(F3dLossCode::HistoryStateNameInherited.note(format!(
                    "evaluation {evaluation}: {} keeps its current name, and the state changes its record.",
                    entity.id()
                )));
            }
        }
    }
}

fn same_bytes(retained: &RetainedSourceRecord, unknown: &UnknownRecord) -> bool {
    match (retained.data(), unknown.data()) {
        (Some(retained), Some(unknown)) => retained == unknown,
        (None, Some(unknown)) => retained.sha256() == cadmpeg_ir::hash::sha256_hex(unknown),
        (_, None) => false,
    }
}

/// Where the state `state` after the item with scope tail `tail` lies among
/// `histories`.
fn bind(histories: &[History<'_>], tail: &Tail<'_>, state: i64) -> Kind {
    let stored = histories
        .iter()
        .enumerate()
        .filter(|(_, history)| history.states.contains_key(&state))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let history = match stored.as_slice() {
        [] => return Kind::Unresolved(format!("the file stores no state {state}")),
        [one] => *one,
        _ => {
            let bound =
                tail.history
                    .and_then(historical_brep_source)
                    .and_then(|source| {
                        stored.iter().copied().find(|index| {
                            histories[*index].name.strip_prefix("BREP.") == Some(source)
                        })
                    });
            let Some(bound) = bound else {
                return Kind::Unresolved(format!(
                    "{} ASM histories store state {state}, and its parameter scope binds none of them",
                    stored.len()
                ));
            };
            bound
        }
    };
    let states = &histories[history].states;
    let previous = tail
        .previous
        .filter(|previous| *previous < state && states.contains_key(previous))
        .or_else(|| states.range(..state).next_back().map(|(found, _)| *found));
    Kind::Bound {
        history,
        state,
        previous,
    }
}

/// The model blobs of the document that hold a history partition. The inner
/// error says why one of them is not read.
fn histories<'s>(
    scan: &'s ContainerScan<'_>,
) -> Result<Result<Vec<History<'s>>, String>, CodecError> {
    let selection = crate::design::decode::body::design_model_selection(scan)?;
    let qualified = selection.blob_names.len() > 1;
    let mut out = Vec::new();
    for facts in crate::container::history_breps(scan) {
        let basename = facts.name.rsplit('/').next().unwrap_or(&facts.name);
        if !selection.blob_names.iter().any(|name| name == basename) {
            continue;
        }
        let namespace = if qualified {
            let Some(namespace) = crate::decode::brep_identity_namespace(&facts.name) else {
                return Ok(Err(format!(
                    "the model blob {} has no namespace",
                    facts.name
                )));
            };
            Some(namespace)
        } else {
            None
        };
        let blob = Blob {
            bytes: scan.entry_bytes(&facts.name)?,
            stream: &facts.name,
            width: facts
                .kernel
                .as_ref()
                .and_then(crate::container::KernelFraming::asm_header)
                .map_or(cadmpeg_asm::kernel_header::RefWidth::Eight, |header| {
                    header.width
                }),
            namespace,
        };
        let Some(states) = stored_states::state_ids(&blob) else {
            return Ok(Err(format!(
                "the history of the model blob {} does not decode",
                facts.name
            )));
        };
        let space = match namespace {
            Some(namespace) => format!("f3d:brep/{namespace}/"),
            None => "f3d:brep:".to_owned(),
        };
        out.push(History {
            blob,
            name: basename,
            space,
            states,
        });
    }
    Ok(Ok(out))
}

/// The evaluation of the state after the timeline item whose scope record is
/// `record` in `stream`.
fn evaluation_id(stream: &str, record: u64) -> Result<EvaluationId, CodecError> {
    Identity::new(format!("{stream}:evaluation#after-{record}"))
        .map(EvaluationId::from)
        .map_err(CodecError::malformed)
}

/// The record ids of `topology` that begin with `space`, or all of them.
fn ids<'t>(topology: &'t Topology, space: Option<&str>) -> BTreeSet<&'t str> {
    let mut found = BTreeSet::new();
    macro_rules! collect {
        ($($field:ident $arm:ident),*) => {
            $(found.extend(
                topology.$field.iter()
                    .map(ArenaRecord::record_identity)
                    .filter(|id| space.is_none_or(|space| id.starts_with(space))),
            );)*
        };
    }
    topology_arenas!(collect);
    found
}

fn len(topology: &Topology) -> usize {
    let mut count = 0;
    macro_rules! add {
        ($($field:ident $arm:ident),*) => {
            $(count += topology.$field.len();)*
        };
    }
    topology_arenas!(add);
    count
}

fn sort(topology: &mut Topology) {
    macro_rules! order {
        ($($field:ident $arm:ident),*) => {
            $(topology.$field.sort_by(|left, right| {
                left.record_identity().cmp(right.record_identity())
            });)*
        };
    }
    topology_arenas!(order);
}

/// Adds to `delta` the change from the state `before` to the state `after`
/// of one blob: each record of `after` that `before` does not hold as it is,
/// and the removal of each record of `before` that `after` does not hold. A
/// record that the blob's head holds as it is takes the current geometry's
/// version. The error names a procedural construction of the change whose
/// owner `after` does not state.
fn step(
    before: &Topology,
    after: &Topology,
    head: &Topology,
    current: &Topology,
    delta: &mut StateDelta,
) -> Result<(), String> {
    macro_rules! diff {
        ($($field:ident $arm:ident),*) => {$({
            let earlier = before.$field.iter()
                .map(|record| (record.record_identity(), record))
                .collect::<HashMap<_, _>>();
            let headed = head.$field.iter()
                .map(|record| (record.record_identity(), record))
                .collect::<HashMap<_, _>>();
            let present = current.$field.iter()
                .map(|record| (record.record_identity(), record))
                .collect::<HashMap<_, _>>();
            let mut kept = HashSet::new();
            for record in &after.$field {
                let id = record.record_identity();
                kept.insert(id);
                if earlier.get(id) == Some(&record) {
                    continue;
                }
                let value = match (headed.get(id), present.get(id)) {
                    (Some(&held), Some(&version)) if held == record => version,
                    _ => record,
                };
                delta.records.$field.push(value.clone());
            }
            for record in &before.$field {
                if !kept.contains(record.record_identity()) {
                    delta.removed.push(RecordRef::$arm(record.id.clone()));
                }
            }
        })*};
    }
    topology_arenas!(diff);
    carry_owners(after, head, current, delta)
}

/// Adds to `delta` the owning surface or curve, from `after`, of each
/// procedural construction that it holds without its owner: a record states
/// its procedural construction together with its owner.
fn carry_owners(
    after: &Topology,
    head: &Topology,
    current: &Topology,
    delta: &mut StateDelta,
) -> Result<(), String> {
    let mut surfaces = Vec::new();
    for procedural in &delta.records.procedural_surfaces {
        if delta
            .records
            .procedural_surface_owner(&procedural.id)
            .is_some()
        {
            continue;
        }
        let owner = after
            .procedural_surface_owner(&procedural.id)
            .and_then(|owner| after.surfaces.iter().find(|surface| surface.id == *owner))
            .ok_or_else(|| {
                format!(
                    "the procedural surface {} has no unique owning surface",
                    procedural.id
                )
            })?;
        if delta
            .records
            .surfaces
            .iter()
            .any(|held| held.id == owner.id)
        {
            return Err(format!(
                "the surface {} of the change does not own its procedural surface {}",
                owner.id, procedural.id
            ));
        }
        surfaces.push(version(owner, &head.surfaces, &current.surfaces).clone());
    }
    let mut curves = Vec::new();
    for procedural in &delta.records.procedural_curves {
        if delta
            .records
            .procedural_curve_owner(&procedural.id)
            .is_some()
        {
            continue;
        }
        let owner = after
            .procedural_curve_owner(&procedural.id)
            .and_then(|owner| after.curves.iter().find(|curve| curve.id == *owner))
            .ok_or_else(|| {
                format!(
                    "the procedural curve {} has no unique owning curve",
                    procedural.id
                )
            })?;
        if delta.records.curves.iter().any(|held| held.id == owner.id) {
            return Err(format!(
                "the curve {} of the change does not own its procedural curve {}",
                owner.id, procedural.id
            ));
        }
        curves.push(version(owner, &head.curves, &current.curves).clone());
    }
    delta.records.surfaces.extend(surfaces);
    delta.records.curves.extend(curves);
    Ok(())
}

/// The version of `record` of a state that a delta states: the current
/// geometry's record where the head holds `record` as it is.
fn version<'t, R: ArenaRecord + PartialEq>(record: &'t R, head: &[R], current: &'t [R]) -> &'t R {
    let id = record.record_identity();
    let held = head.iter().find(|held| held.record_identity() == id);
    match (
        held,
        current.iter().find(|found| found.record_identity() == id),
    ) {
        (Some(held), Some(found)) if held == record => found,
        _ => record,
    }
}
