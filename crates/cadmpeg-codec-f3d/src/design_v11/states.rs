// SPDX-License-Identifier: Apache-2.0
//! The stored states between the features of a Fusion timeline, as
//! evaluations of the root scope (§5.4).
//!
//! The scope tail of a timeline item stores the id of the ASM history state
//! after the item. Each model blob with a history partition stores one chain
//! of states, numbered on a counter of its own. Where the model blobs of the
//! document hold one history, the state after an item is its stored state
//! with the largest id that is not above the item's id, and the empty state
//! where it stores none. An item that builds the current geometry and that
//! is not stateless has an evaluation of the state after it, whose base is
//! the state after the next later such item, and `Current` at the scope's
//! current position for the last of them. The current position has no
//! evaluation.
//!
//! An item after the rollback marker whose tail flags its stored state ids as
//! holding no current result, and whose state id names no stored state, has
//! a `Stale` evaluation with no content. Every other state this module does
//! not state charges a loss that names it.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::evaluation::{
    Evaluation, EvaluationStatus, StateBase, StateContent, StateDelta,
};
use cadmpeg_ir::design::ids::EvaluationId;
use cadmpeg_ir::design::refs::{Evaluated, Member, Position, RecordRef};
use cadmpeg_ir::document::{CadIr, Topology};
use cadmpeg_ir::features::{ConfigurationId, FeatureId};
use cadmpeg_ir::ids::{BodyId, Identity, ProductDefinitionId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::schema::ArenaRecord;

use crate::container::ContainerScan;
use crate::history::stored_states::{self, Blob};
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignScopeTailFlags;

#[cfg(test)]
mod tests;

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
pub(super) struct Tail {
    /// The id of the history state after the item.
    pub(super) state: Option<i64>,
    pub(super) flags: Option<DesignScopeTailFlags>,
}

/// The scope tails of the timeline items, keyed by stream and record index.
pub(super) type Tails<'a> = HashMap<(&'a str, u64), Tail>;

/// Pushes to `ir` the evaluations of the stored states of `scope`, and to
/// `losses` a note for each state it does not state.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    scan: &ContainerScan<'_>,
    scope: &Scope,
    tails: &Tails<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut evaluations = Vec::new();
    let mut reader = Reader {
        ctx,
        ir,
        scope,
        tails,
        losses,
    };
    if let Err(reason) = reader.read(scan, &mut evaluations)? {
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
    Ok(())
}

/// A timeline item of a feature that is not stateless.
struct Item<'a> {
    feature: &'a FeatureId,
    stream: &'a str,
    record: u64,
    tail: Tail,
}

/// A timeline item whose stored state has an evaluation.
struct Candidate<'a> {
    feature: &'a FeatureId,
    stream: &'a str,
    record: u64,
    state: i64,
}

/// One model blob with a history partition.
struct History<'s> {
    blob: Blob<'s>,
    /// The prefix of the record ids of the blob.
    space: String,
    /// The native id of each stored state, by state id.
    states: BTreeMap<i64, String>,
}

/// What reads the stored states of one scope.
struct Reader<'r, 'c> {
    ctx: &'r DecodeContext<'c>,
    ir: &'r CadIr,
    scope: &'r Scope,
    tails: &'r Tails<'r>,
    losses: &'r mut Vec<LossNote>,
}

impl<'r> Reader<'r, '_> {
    /// Pushes to `out` the evaluations of the scope's stored states. The
    /// inner error says why the states of its built items are not stated.
    fn read(
        &mut self,
        scan: &ContainerScan<'_>,
        out: &mut Vec<Evaluation>,
    ) -> Result<Result<(), String>, CodecError> {
        let scope = self.scope;
        let configuration = if self
            .ir
            .design
            .configurations
            .iter()
            .any(|configuration| configuration.scope == scope.definition)
        {
            let active = self
                .ir
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
        let (built, rolled_back) = match &scope.at {
            Position::Start(_) => (&scope.members[..0], &scope.members[..]),
            Position::After(member) => {
                match scope.members.iter().position(|found| found == member) {
                    Some(at) => (&scope.members[..at], &scope.members[at + 1..]),
                    None => return Ok(Ok(())),
                }
            }
        };
        let histories = match histories(scan)? {
            Ok(histories) => histories,
            Err(reason) => return Ok(Err(reason)),
        };
        if histories.len() > 1 {
            return Ok(Err(format!(
                "its model blobs hold {} ASM histories, which number their states on separate counters, and the history that the state id of a timeline item selects is not known (open item DR-1064)",
                histories.len()
            )));
        }
        let stored = histories
            .iter()
            .flat_map(|history| history.states.keys().copied())
            .collect::<BTreeSet<_>>();
        for member in rolled_back {
            let Some(Item {
                feature,
                stream,
                record,
                tail,
            }) = self.item(member, configuration.as_ref())?
            else {
                continue;
            };
            let Some(state) = tail.state else {
                continue;
            };
            if stored.contains(&state) || !tail.flags.is_some_and(|flags| flags.no_result) {
                continue;
            }
            self.losses.push(F3dLossCode::HistoryStateUnread.note(format!(
                "feature {feature}: the file stores no state {state} after it, and its scope tail flags it as holding no current result; its evaluation is stale with no content."
            )));
            out.push(Evaluation {
                id: evaluation_id(stream, record)?,
                scope: scope.definition.clone(),
                configuration: configuration.clone(),
                of: Evaluated::State(Position::After(Member::Feature(feature.clone()))),
                status: EvaluationStatus::Stale,
                content: None,
                native_ref: None,
            });
        }
        let mut candidates = Vec::new();
        let mut unstated = Vec::new();
        for member in built {
            let Some(Item {
                feature,
                stream,
                record,
                tail,
            }) = self.item(member, configuration.as_ref())?
            else {
                continue;
            };
            match tail.state {
                None => unstated.push(feature.as_str()),
                Some(state) if tail.flags.is_some_and(|flags| flags.no_result) => {
                    self.losses.push(F3dLossCode::HistoryStateUnread.note(format!(
                        "feature {feature}: its scope tail flags state {state} as holding no current result; the state after it is not stated."
                    )));
                }
                Some(state) if !stored.contains(&state) => {
                    self.losses.push(F3dLossCode::HistoryStateUnread.note(format!(
                        "feature {feature}: the file stores no state {state}; the state after it is not stated."
                    )));
                }
                Some(state) => candidates.push(Candidate {
                    feature,
                    stream,
                    record,
                    state,
                }),
            }
        }
        if !unstated.is_empty() {
            self.losses.push(F3dLossCode::HistoryStateUnread.note(format!(
                "scope {}: {} built timeline item(s) store no state id; the states after them are not stated: {}.",
                scope.definition,
                unstated.len(),
                unstated.join(", ")
            )));
        }
        if candidates.is_empty() {
            return Ok(Ok(()));
        }
        if candidates
            .windows(2)
            .any(|pair| pair[0].state >= pair[1].state)
        {
            return Ok(Err(
                "the state ids of its built items do not increase in timeline order".to_owned(),
            ));
        }
        self.chain(&histories, &candidates, configuration.as_ref(), out)
    }

    /// The item of a member that is a feature of the root document and that
    /// is not stateless.
    fn item(
        &self,
        member: &'r Member,
        configuration: Option<&ConfigurationId>,
    ) -> Result<Option<Item<'r>>, CodecError> {
        let Member::Feature(feature) = member else {
            return Ok(None);
        };
        let Some((stream, record)) = self.scope.items.get(feature) else {
            return Ok(None);
        };
        if !super::timeline::document_prefix(stream).is_empty() {
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
        if stateless {
            return Ok(None);
        }
        let tail = self
            .tails
            .get(&(stream.as_str(), *record))
            .copied()
            .unwrap_or(Tail {
                state: None,
                flags: None,
            });
        Ok(Some(Item {
            feature,
            stream: stream.as_str(),
            record: *record,
            tail,
        }))
    }

    /// Pushes to `out` the evaluation of the state after each candidate,
    /// latest first, each based on the one before it.
    fn chain(
        &mut self,
        histories: &[History<'_>],
        candidates: &[Candidate<'_>],
        configuration: Option<&ConfigurationId>,
        out: &mut Vec<Evaluation>,
    ) -> Result<Result<(), String>, CodecError> {
        let scope = self.scope;
        let current = self
            .ir
            .resolved_current_state(&scope.definition, &scope.at)
            .map_err(|error| {
                CodecError::malformed(format_args!(
                    "F3D scope {} has no current state: {error}",
                    scope.definition
                ))
            })?;
        if !current.names().is_empty() {
            return Ok(Err("its current state holds names".to_owned()));
        }
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
        let owners = held
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
        let targets = histories
            .iter()
            .map(|history| {
                candidates
                    .iter()
                    .map(|candidate| {
                        history
                            .states
                            .range(..=candidate.state)
                            .next_back()
                            .map(|(state, _)| *state)
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
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
        let mut reads = Vec::with_capacity(histories.len());
        for ((history, bodies), wanted) in histories.iter().zip(&held).zip(&targets) {
            let wanted = wanted.iter().flatten().copied().collect();
            let read = match stored_states::read(
                self.ctx,
                &history.blob,
                bodies,
                &wanted,
                &self.ir.tolerances,
                &work,
            )? {
                Ok(read) => read,
                Err(reason) => {
                    return Ok(Err(format!(
                        "the history of {} gives no states: {reason}",
                        history.blob.stream
                    )))
                }
            };
            let space = ids(current.records(), Some(&history.space));
            if ids(&read.head_records, None) != space {
                return Ok(Err(format!(
                    "the records of the head of {} differ from the current geometry",
                    history.blob.stream
                )));
            }
            reads.push(read);
        }
        let empty = Topology::default();
        let mut previous = reads
            .iter()
            .map(|read| (Some(read.head), &read.head_records))
            .collect::<Vec<_>>();
        let mut entries = current
            .owners()
            .iter()
            .filter(|(_, owner)| **owner != scope.definition)
            .map(|(body, owner)| (body.clone(), owner.clone()))
            .collect::<BTreeMap<_, _>>();
        let limit = self.ctx.policy().limits.max_state_chain_depth;
        let mut base = StateBase::Current(scope.at.clone());
        for (depth, (at, candidate)) in candidates.iter().enumerate().rev().enumerate() {
            let stop = |reason: String| {
                F3dLossCode::HistoryStateUnread.note(format!(
                    "scope {}: the states after {} built timeline item(s), from feature {} back, are not stated: {reason}.",
                    scope.definition,
                    at + 1,
                    candidate.feature
                ))
            };
            if u64_from_index(depth) >= limit {
                self.losses
                    .push(stop(format!("the state chain ceiling {limit} is reached")));
                break;
            }
            let mut delta = StateDelta::default();
            let mut unread = None;
            for (index, history) in histories.iter().enumerate() {
                let target = targets[index][at];
                let (state, records) = previous[index];
                if target == state {
                    continue;
                }
                let next = match target {
                    None => &empty,
                    Some(target) => match reads[index]
                        .states
                        .iter()
                        .find(|(found, _)| *found == target)
                    {
                        Some((_, Ok(next))) => next,
                        Some((_, Err(reason))) => {
                            unread = Some(format!(
                                "state {target} of {}: {reason}",
                                history.blob.stream
                            ));
                            break;
                        }
                        None => {
                            unread = Some(format!(
                                "state {target} of {} is not read",
                                history.blob.stream
                            ));
                            break;
                        }
                    },
                };
                if let Err(reason) = step(
                    records,
                    next,
                    &reads[index].head_records,
                    current.records(),
                    &mut delta,
                ) {
                    unread = Some(format!(
                        "state {} of {}: {reason}",
                        target.map_or_else(|| "empty".to_owned(), |target| target.to_string()),
                        history.blob.stream
                    ));
                    break;
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
                    let owner = if held[index].contains(&body.id) {
                        current.owners().get(&body.id)
                    } else {
                        owners[index].as_ref()
                    };
                    let Some(owner) = owner else {
                        unread = Some(format!(
                            "the owner of body {} of {} is not known",
                            body.id, history.blob.stream
                        ));
                        break;
                    };
                    if *owner != scope.definition {
                        delta.owners.insert(body.id.clone(), Some(owner.clone()));
                        entries.insert(body.id.clone(), owner.clone());
                    }
                }
                if unread.is_some() {
                    break;
                }
                previous[index] = (target, next);
            }
            if let Some(reason) = unread {
                self.losses.push(stop(reason));
                break;
            }
            sort(&mut delta.records);
            self.ctx.charge_entities(
                u64_from_index(len(&delta.records) + delta.removed.len()),
                "project F3D v11 history state records",
            )?;
            let id = evaluation_id(candidate.stream, candidate.record)?;
            out.push(Evaluation {
                id: id.clone(),
                scope: scope.definition.clone(),
                configuration: configuration.cloned(),
                of: Evaluated::State(Position::After(Member::Feature(candidate.feature.clone()))),
                status: EvaluationStatus::Current,
                content: Some(StateContent {
                    base: std::mem::replace(&mut base, StateBase::Evaluation(id)),
                    delta,
                }),
                native_ref: histories
                    .iter()
                    .find_map(|history| history.states.get(&candidate.state).cloned()),
            });
        }
        Ok(Ok(()))
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

/// The number of records of `topology`.
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

/// Sorts each arena of `topology` by id.
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
