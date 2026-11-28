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
//! chain, which the walk follows back from the state after the last item of
//! that history: the state after each item is the state before the next
//! later item of that history. The current state holds the change of the
//! states that a history stores after that of its last item, and a loss
//! names them. The state after an item holds, of each history, the state
//! that this walk reaches at the item, with the consumed bodies of the head
//! whose consumer is later in the timeline (§5.5). A consumed body has the
//! owner of the live bodies that its entity cluster places with it, or of the
//! definition bound to that cluster; a loss names a consumed body that takes
//! the owner of its history, and a state omits one with no owner.
//!
//! The walk takes a joint origin, a grounding, a rigid group, a motion link,
//! a reference image, and a decal as stateless. The current position is the
//! last item that commits a state; the state after it and after each later
//! item is the current state (§5.4 Keys). Where a history stores states
//! above the state of its last item and no later item can hold them, the
//! current position is the scope's current position. An
//! item before the current position that is not stateless and whose state
//! the walk places has an evaluation of the state after it, whose base is
//! the state after the next later such item, and `Current` at the scope's
//! current position for the last of them. An item that stores no state id
//! holds members and poses that the walk does not read: it has no
//! evaluation, and a loss names it. Where such an item moves, creates, or
//! re-owns occurrences, the evaluations before it take the current poses
//! and membership, and a loss names them. An item that stores no scope tail
//! and is the one such item between two items of a history whose stored
//! states do not meet takes the gap: its state is the state before the
//! later item, and the state before it is the state of the earlier one. An
//! item whose state id several histories store lies in the one of them that
//! the walk stands at that state in. A loss names each state so placed.
//! Another item whose state the walk does not place has no evaluation, and
//! a loss names it and the assumption that it commits no state. The walk
//! stops where the stored states of the items of one history do not meet,
//! and a loss names each feature whose state it leaves unstated.
//!
//! The walk is a task of the decode: where its charges reach a limit of the
//! decode, the scope states no evaluation and a loss names it.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::edit::state_view::ResolvedState;
use cadmpeg_ir::design::evaluation::{
    Evaluation, EvaluationStatus, StateBase, StateContent, StateDelta,
};
use cadmpeg_ir::design::feature::{FeatureOperation, OperationFamily};
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
use crate::decode::geometry_admission::Note;
use crate::design::components::HistoryPairOwner;
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
/// The walk runs as a task of `ctx`: where it fails, or its charges reach a
/// limit of the decode, `ir` and `ctx` are unchanged and one loss names the
/// scope.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    sources: &StoredStates<'_>,
    scope: &Scope,
    tails: &Tails<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<UnknownRecord>, CodecError> {
    let arena = DecodeArena::new();
    let local = ctx.checkpoint().resume(&arena);
    let mut evaluations = Vec::new();
    let mut carried = Vec::new();
    let mut found = Vec::new();
    let read = Reader {
        ctx: &local,
        ir,
        fidelity: sources.fidelity,
        scope,
        tails,
        pairs: sources.history_owners,
        losses: &mut found,
    }
    .read(sources.scan, &mut evaluations, &mut carried);
    let unfit = || "the walk does not fit the decode's resource limits left".to_owned();
    let outcome = match read {
        _ if local.resource_refusal().is_some() => Err(unfit()),
        Err(error @ CodecError::Io(_)) => return Err(error),
        Err(error) => Err(error.to_string()),
        Ok(Err(reason)) => Err(reason),
        Ok(Ok(())) if ctx.accept_trace(&local.take_trace()) => Ok(()),
        Ok(Ok(())) => Err(unfit()),
    };
    match outcome {
        Ok(()) => {
            losses.extend(found);
            ir.design.evaluations.extend(evaluations);
            Ok(carried)
        }
        Err(reason) => {
            losses.push(F3dLossCode::HistoryStatesUnread.note(format!(
                "scope {}: the states after its timeline items are not stated: {reason}.",
                scope.definition
            )));
            Ok(Vec::new())
        }
    }
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
    /// The item's parameter scope stores no tail.
    Tailless,
    /// The state id is not placed in one history, for this reason.
    Unresolved(String),
    /// The histories `stored` store the state id `state`, and the parameter
    /// scope binds none of them. `previous` is the preceding state id of the
    /// scope tail.
    Ambiguous {
        state: i64,
        stored: Vec<usize>,
        previous: Option<i64>,
    },
}

/// What the operation of a timeline item changes in a state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Role {
    /// The geometry, or what it changes is not known.
    Geometry,
    /// Nothing: a joint origin, a grounding, a rigid group, a motion link, a
    /// reference image, or a decal.
    Inert,
    /// The poses, existence, or owners of occurrences: a joint, an insert,
    /// a new component, a paste, a component transfer, a capture position,
    /// or a component pattern or mirror. Its ASM history state is null.
    Assembly,
    /// The bodies that it moves, or the occurrences where it stores no
    /// state id.
    Move,
}

impl Role {
    /// An item of this role whose state lies at `kind` moves, creates, or
    /// re-owns occurrences, and stores no history state of that change.
    fn moves(self, kind: &Kind) -> bool {
        self == Role::Assembly || self == Role::Move && *kind == Kind::NoState
    }
}

/// What the operation `operation` of a v11 timeline item changes.
fn role(operation: &FeatureOperation) -> Role {
    match operation {
        FeatureOperation::Unresolved { family } => match family {
            OperationFamily::RigidGroup => Role::Inert,
            OperationFamily::Joint
            | OperationFamily::Place
            | OperationFamily::PlaceSet
            | OperationFamily::ComponentPattern
            | OperationFamily::MirrorComponents => Role::Assembly,
            OperationFamily::TransformBodies => Role::Move,
            _ => Role::Geometry,
        },
        FeatureOperation::Native { kind, .. } => match kind.as_str().strip_prefix("f3d:") {
            Some("JointOrigin" | "Ground" | "MotionLink" | "reference_image" | "Decal") => {
                Role::Inert
            }
            Some(
                "NewComponent"
                | "Paste"
                | "CopyPaste"
                | "BodyToComponent"
                | "component_transfer"
                | "captured_component_positions"
                | "Position"
                | "Assemble"
                | "As-built",
            ) => Role::Assembly,
            _ => Role::Geometry,
        },
        _ => Role::Geometry,
    }
}

pub(super) fn configuration<'a>(
    ir: &'a CadIr,
    definition: &ProductDefinitionId,
) -> Result<Option<&'a ConfigurationId>, &'static str> {
    if !ir
        .design
        .configurations
        .iter()
        .any(|row| &row.scope == definition)
    {
        return Ok(None);
    }
    ir.design
        .scopes
        .iter()
        .find(|scope| &scope.definition == definition)
        .and_then(|scope| scope.active_configuration.as_ref())
        .map(Some)
        .ok_or("the scope has configurations and no active one")
}

pub(super) fn eligibility(
    ir: &CadIr,
    feature: &FeatureId,
    configuration: Option<&ConfigurationId>,
) -> Result<(bool, Role), CodecError> {
    let reading = |error| {
        CodecError::malformed(format_args!(
            "F3D feature {feature} has no stateless reading: {error}"
        ))
    };
    let stateless = ir.is_stateless(feature, configuration).map_err(reading)?;
    let definition = ir
        .feature_definition_in(feature, configuration)
        .map_err(reading)?;
    let role = role(&definition.operation);
    Ok((stateless || role == Role::Inert, role))
}

/// A built timeline item of a feature of the root document.
#[derive(Debug)]
struct Entry<'a> {
    feature: &'a FeatureId,
    stream: &'a str,
    record: u64,
    /// The index of its member in the timeline.
    position: usize,
    /// The item changes no state: its feature is stateless (§5.4), or its
    /// role is inert.
    stateless: bool,
    /// The item moves, creates, or re-owns occurrences, and stores no
    /// history state.
    moves: bool,
    /// The state after the item is the current state: it is at or after the
    /// current position.
    current: bool,
    kind: Kind,
}

impl Entry<'_> {
    /// The item has an evaluation of the state after it.
    fn evaluated(&self) -> bool {
        !self.stateless && !self.current && matches!(self.kind, Kind::Bound { .. })
    }

    /// The item changes a state that the walk does not read.
    fn unstated(&self) -> bool {
        !self.stateless && !self.current && self.kind == Kind::NoState
    }
}

/// An item with an evaluation, and the state of each history after it.
#[derive(Debug, PartialEq, Eq)]
struct Planned {
    entry: usize,
    targets: Vec<Option<i64>>,
}

/// Why the walk leaves the states after some features unstated, and those
/// features.
#[derive(Debug, PartialEq, Eq)]
struct Stop {
    reason: String,
    features: Vec<FeatureId>,
}

/// The items with evaluations, latest first, the gaps whose evaluations the
/// walk gives up, and where the walk stops.
#[derive(Debug, PartialEq, Eq)]
struct Plan {
    planned: Vec<Planned>,
    gaps: Vec<Stop>,
    stop: Option<Stop>,
}

/// The state after the last item of each of `histories` that
/// `entries` bind, by index, with that item.
fn last_items<'e, H>(
    entries: &'e [Entry<'e>],
    histories: &[H],
) -> Vec<Option<(i64, &'e FeatureId)>> {
    let mut last = vec![None; histories.len()];
    for entry in entries {
        if let Kind::Bound { history, state, .. } = entry.kind {
            last[history] = Some((state, entry.feature));
        }
    }
    last
}

/// The stored state of `states` before the state `state` of an item whose
/// scope tail stores the preceding state id `previous`.
fn before(states: &BTreeMap<i64, String>, state: i64, previous: Option<i64>) -> Option<i64> {
    previous
        .filter(|previous| *previous < state && states.contains_key(previous))
        .or_else(|| states.range(..state).next_back().map(|(found, _)| *found))
}

/// Whether the state `previous` before an item can follow the last item of
/// `earlier` bound to the history `history`: where there is such an item,
/// `previous` is a state that is not below the state of that item.
fn meets(earlier: &[Entry<'_>], history: usize, previous: Option<i64>) -> bool {
    let last = earlier.iter().rev().find_map(|entry| match entry.kind {
        Kind::Bound {
            history: found,
            state,
            ..
        } if found == history => Some(state),
        _ => None,
    });
    match (last, previous) {
        (None, _) => true,
        (Some(last), Some(previous)) => previous >= last,
        (Some(_), None) => false,
    }
}

/// Binds to one of `histories`, whose heads are `heads`, the state of each
/// item of `entries` that its scope tail does not place, walking latest
/// first from each head. An item with no scope tail that is the one such
/// item between two items of a history whose stored states do not meet, or
/// between the last item of a history and a head above its state, takes
/// that gap. An item whose state id several histories store takes a unique
/// compatible history. A later cursor at the item's state takes precedence
/// over a forward gap. The walk
/// goes on past a gap that no single such item takes, and ends where the
/// state before a later item of a history is below the state of an earlier
/// one, or is none. Returns each item so bound, by index, with why.
fn resolve(
    entries: &mut [Entry<'_>],
    heads: &[Option<i64>],
    histories: &[History<'_>],
) -> Vec<(usize, String)> {
    let mut at = heads.to_vec();
    let mut tailless = vec![Vec::<usize>::new(); heads.len()];
    let mut later = vec![None::<usize>; heads.len()];
    let mut placed = BTreeSet::new();
    let mut out = Vec::new();
    for index in (0..entries.len()).rev() {
        let entry = &entries[index];
        let shared = match &entry.kind {
            Kind::Tailless if !entry.stateless => {
                for found in &mut tailless {
                    found.push(index);
                }
                None
            }
            Kind::Ambiguous {
                state,
                stored,
                previous,
            } if !entry.stateless => {
                let mut exact = Vec::new();
                let mut forward = Vec::new();
                for &history in stored {
                    let Some(upper) = at[history].filter(|upper| upper >= state) else {
                        continue;
                    };
                    let previous = before(&histories[history].states, *state, *previous);
                    if meets(&entries[..index], history, previous) {
                        let candidates = if upper == *state {
                            &mut exact
                        } else {
                            &mut forward
                        };
                        candidates.push((history, upper, previous));
                    }
                }
                match (exact.as_slice(), forward.as_slice()) {
                    ([(history, upper, previous)], _) | ([], [(history, upper, previous)]) => {
                        Some((*history, *state, *upper, *previous, stored.len()))
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some((history, state, upper, previous, count)) = shared {
            let stream = histories[history].blob.stream;
            let evidence = if upper == state {
                format!("the items after it leave {stream} at that state")
            } else {
                format!("{stream} is the only compatible history and the walk reaches state {upper} across a forward gap")
            };
            out.push((
                index,
                format!(
                    "{count} ASM histories store its state id {state}, its parameter scope binds none of them, and {evidence}"
                ),
            ));
            entries[index].kind = Kind::Bound {
                history,
                state,
                previous,
            };
        }
        let Kind::Bound {
            history,
            state,
            previous,
        } = entries[index].kind
        else {
            continue;
        };
        if at[history] != Some(state) {
            let open = tailless[history]
                .iter()
                .copied()
                .filter(|found| !placed.contains(found))
                .collect::<Vec<_>>();
            match (at[history], open.as_slice(), later[history]) {
                (Some(gap), [one], _) if gap > state => {
                    let stream = histories[history].blob.stream;
                    out.push((
                        *one,
                        match later[history] {
                            Some(next) => format!(
                                "it is the one item with no scope tail between feature {} and feature {} of {stream}, whose stored states {state} and {gap} do not meet; its state is the state before feature {}, and the state before it is the state after feature {}",
                                entries[index].feature,
                                entries[next].feature,
                                entries[next].feature,
                                entries[index].feature
                            ),
                            None => format!(
                                "it is the one item with no scope tail after feature {}, the last item of {stream}, whose state {state} is below the head {gap}; its state is the head, and the state before it is the state after feature {}",
                                entries[index].feature, entries[index].feature
                            ),
                        },
                    ));
                    entries[*one].kind = Kind::Bound {
                        history,
                        state: gap,
                        previous: Some(state),
                    };
                    placed.insert(*one);
                }
                (_, _, None) => {}
                (Some(gap), _, Some(_)) if gap > state => {}
                (_, _, Some(_)) => break,
            }
        }
        at[history] = previous;
        tailless[history].clear();
        later[history] = Some(index);
    }
    out
}

/// The states after `entries`, walking back through the histories named
/// `names` from the state after the last item of each, or from its head in
/// `heads` where no item stores one of its states. Where the state of an
/// item of a history is below the state before the next item of that
/// history, the states between them have no position: the walk gives up
/// the evaluations of the items between the two and goes on from the
/// earlier item's state. It stops where the state before the later item is
/// below the earlier item's state, or is none.
fn plan(entries: &[Entry<'_>], heads: &[Option<i64>], names: &[&str]) -> Plan {
    let mut at = last_items(entries, heads)
        .into_iter()
        .zip(heads)
        .map(|(last, head)| last.map(|(state, _)| state).or(*head))
        .collect::<Vec<_>>();
    let mut set = vec![0_usize; heads.len()];
    let mut later = vec![None::<&FeatureId>; heads.len()];
    let mut planned = Vec::<Planned>::new();
    let mut gaps = Vec::new();
    for (index, entry) in entries.iter().enumerate().rev() {
        let Kind::Bound {
            history,
            state,
            previous,
        } = entry.kind
        else {
            continue;
        };
        if let (Some(gap), Some(next)) = (at[history].filter(|gap| *gap > state), later[history]) {
            gaps.push(Stop {
                reason: format!(
                    "no timeline item joins the states of {} after state {state} of feature {} up to state {gap}, which feature {next} stores as the state before it, so the state of that history after the items between them is not known",
                    names[history], entry.feature
                ),
                features: planned
                    .drain(set[history]..)
                    .map(|planned| entries[planned.entry].feature.clone())
                    .collect(),
            });
            for set in &mut set {
                *set = (*set).min(planned.len());
            }
            at[history] = Some(state);
        }
        if at[history] != Some(state) {
            let reason = format!(
                "feature {} stores state {state} of {}, and the items after it leave that history at {}",
                entry.feature,
                names[history],
                at[history].map_or_else(|| "no state".to_owned(), |state| format!("state {state}"))
            );
            let features = planned
                .drain(set[history]..)
                .map(|planned| entries[planned.entry].feature.clone())
                .chain(
                    entries[..=index]
                        .iter()
                        .rev()
                        .filter(|entry| entry.evaluated())
                        .map(|entry| entry.feature.clone()),
                )
                .collect();
            return Plan {
                planned,
                gaps,
                stop: Some(Stop { reason, features }),
            };
        }
        if entry.evaluated() {
            planned.push(Planned {
                entry: index,
                targets: at.clone(),
            });
        }
        at[history] = previous;
        set[history] = planned.len();
        later[history] = Some(entry.feature);
    }
    Plan {
        planned,
        gaps,
        stop: None,
    }
}

/// Marks the items of `entries` whose state is the current state: the item
/// at the current position and the items after it. The current position is
/// the last item that commits a state, or the member at the last position of
/// `others` where that is later; every item is current where none commits
/// one. An item is taken to commit no state where it is stateless, or where
/// the walk does not place its state and every history's head is the state
/// of its last item. Where a history's head is above the state of its last
/// item and no item after that one leaves its state unplaced, the current
/// position is `end`, the scope's current position.
fn mark_current(entries: &mut [Entry<'_>], heads: &[Option<i64>], others: &[usize], end: usize) {
    let mut last = vec![None; heads.len()];
    for entry in entries.iter() {
        if let Kind::Bound { history, state, .. } = entry.kind {
            last[history] = Some((state, entry.position));
        }
    }
    let open = last
        .iter()
        .zip(heads)
        .filter_map(|(last, head)| last.filter(|(state, _)| Some(*state) != *head))
        .map(|(_, position)| position)
        .collect::<Vec<_>>();
    let unplaced = |entry: &Entry<'_>| {
        !entry.stateless
            && matches!(
                entry.kind,
                Kind::Tailless | Kind::Unresolved(_) | Kind::Ambiguous { .. }
            )
    };
    let holders = entries
        .iter()
        .filter(|entry| unplaced(entry))
        .map(|entry| entry.position)
        .chain(others.iter().copied())
        .max();
    let at = if open
        .iter()
        .any(|position| holders.is_none_or(|holder| holder < *position))
    {
        Some(end)
    } else {
        entries
            .iter()
            .filter(|entry| {
                !entry.stateless
                    && (matches!(entry.kind, Kind::Bound { .. } | Kind::NoState)
                        || !open.is_empty() && unplaced(entry))
            })
            .map(|entry| entry.position)
            .chain(others.iter().copied())
            .max()
    };
    for entry in entries {
        entry.current = at.is_none_or(|at| entry.position >= at);
    }
}

/// How a body of a state takes an owner that the file does not state for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Rule {
    /// The owner that its entity cluster gives it, as for a live body with no
    /// Body-container owner.
    Cluster,
    /// The one owner of the current bodies of its history, which the file
    /// does not state for it.
    Assumed,
}

/// The consumed bodies of the head of one history.
#[derive(Default)]
struct Consumed {
    /// The timeline index of the first consumer of each body, by body-map
    /// key.
    placed: BTreeMap<u64, usize>,
    /// The keys of the bodies that no timeline item consumes.
    unplaced: BTreeSet<u64>,
    /// The owner of each placed body, and how it takes it, by key.
    owners: BTreeMap<u64, (ProductDefinitionId, Rule)>,
    /// The keys of the consumed bodies whose owner is not known.
    unowned: BTreeSet<u64>,
    /// The ids of the body-map pairs of each key.
    pairs: BTreeMap<u64, BTreeSet<String>>,
}

/// The one owner of the current bodies of each history in `held`, if any.
fn history_owners(
    held: &[BTreeSet<BodyId>],
    current: &ResolvedState,
) -> Vec<Option<ProductDefinitionId>> {
    held.iter()
        .map(|bodies| {
            let mut found = bodies
                .iter()
                .filter_map(|body| current.owners().get(body))
                .collect::<BTreeSet<_>>()
                .into_iter();
            let owner = found.next();
            owner.filter(|_| found.next().is_none()).cloned()
        })
        .collect()
}

impl Consumed {
    /// Gives each placed body the one owner in `current` of the live bodies
    /// that `pairs` places with it, by key. Where `current` holds none of
    /// them, it takes the definition that `pairs` binds to its entity
    /// cluster where `defined` holds it, and else the owner `history` of the
    /// current bodies of its history. A placed body with no such owner
    /// becomes unowned.
    fn own(
        &mut self,
        pairs: &BTreeMap<u64, HistoryPairOwner>,
        defined: &BTreeSet<&ProductDefinitionId>,
        history: Option<&ProductDefinitionId>,
        current: &ResolvedState,
    ) {
        for key in self.placed.keys() {
            let pair = pairs.get(key);
            let mut found = pair
                .into_iter()
                .flat_map(|pair| &pair.peers)
                .filter_map(|body| current.owners().get(body))
                .collect::<BTreeSet<_>>()
                .into_iter();
            let owner = match (found.next(), found.next()) {
                (Some(owner), None) => Some((owner, Rule::Cluster)),
                (Some(_), Some(_)) => None,
                (None, _) => pair
                    .and_then(|pair| pair.definition.as_ref())
                    .filter(|definition| defined.contains(definition))
                    .map(|definition| (definition, Rule::Cluster))
                    .or(history.map(|owner| (owner, Rule::Assumed))),
            };
            match owner {
                Some((owner, rule)) => {
                    self.owners.insert(*key, (owner.clone(), rule));
                }
                None => {
                    self.unowned.insert(*key);
                }
            }
        }
        let unowned = &self.unowned;
        self.placed.retain(|key, _| !unowned.contains(key));
    }

    /// The keys of every consumed body of the head.
    fn keys(&self) -> BTreeSet<u64> {
        self.placed
            .keys()
            .chain(&self.unplaced)
            .chain(&self.unowned)
            .copied()
            .collect()
    }
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

/// The loss of the items of `entries` that change a state the walk does not
/// read, if any.
fn unstated(definition: &ProductDefinitionId, entries: &[Entry<'_>]) -> Option<LossNote> {
    let features = entries
        .iter()
        .filter(|entry| entry.unstated())
        .map(|entry| entry.feature.as_str())
        .collect::<BTreeSet<_>>();
    (!features.is_empty()).then(|| {
        F3dLossCode::HistoryStateUnread.note(format!(
            "scope {definition}: the states after features {} are not stated: they store no history state, and the members and poses after them are not read.",
            join(features)
        ))
    })
}

/// One loss for each item of `entries` before the current position whose
/// state the walk does not place. The walk takes such an item to commit no
/// state, and the check between the items of each history gives up the
/// states between two of them whose stored states do not meet.
fn unresolved(definition: &ProductDefinitionId, entries: &[Entry<'_>]) -> Vec<LossNote> {
    entries
        .iter()
        .filter(|entry| !entry.stateless && !entry.current)
        .filter_map(|entry| {
            let reason = match &entry.kind {
                Kind::Unresolved(reason) => reason.clone(),
                Kind::Tailless => "its scope tail is not read".to_owned(),
                Kind::Ambiguous { state, stored, .. } => format!(
                    "{} ASM histories store state {state}, its parameter scope binds none of them, and the timeline does not establish a unique history with a consistent state sequence",
                    stored.len()
                ),
                Kind::Bound { .. } | Kind::NoState => return None,
            };
            Some(F3dLossCode::HistoryStateUnread.note(format!(
                "scope {definition}: the state after feature {} is not stated: {reason}. The item is taken to commit no history state, and the states of the other items are their own stored states.",
                entry.feature
            )))
        })
        .collect()
}

/// One loss for each item of `inferred`, an index into `entries` with why
/// the walk bound its state to a history of `histories`.
fn placed(
    definition: &ProductDefinitionId,
    entries: &[Entry<'_>],
    histories: &[History<'_>],
    inferred: &[(usize, String)],
) -> Vec<LossNote> {
    inferred
        .iter()
        .filter_map(|(index, why)| {
            let entry = &entries[*index];
            let Kind::Bound {
                history, state, ..
            } = entry.kind
            else {
                return None;
            };
            Some(F3dLossCode::HistoryStateInferred.note(format!(
                "scope {definition}: the state after feature {} is taken to be state {state} of {}: {why}.",
                entry.feature, histories[history].blob.stream
            )))
        })
        .collect()
}

/// The loss of the evaluations of `emitted` that lie before an item of
/// `entries` that moves, creates, or re-owns occurrences, if any: they take
/// the current poses and occurrence membership.
fn poses_assumed(
    definition: &ProductDefinitionId,
    entries: &[Entry<'_>],
    emitted: &[Planned],
) -> Option<LossNote> {
    let movers = entries
        .iter()
        .filter(|entry| entry.moves && !entry.stateless)
        .collect::<Vec<_>>();
    let last = movers.iter().map(|entry| entry.position).max()?;
    let before = emitted
        .iter()
        .map(|planned| &entries[planned.entry])
        .filter(|entry| entry.position < last)
        .collect::<Vec<_>>();
    let first = before.iter().map(|entry| entry.position).min()?;
    let features = before
        .iter()
        .map(|entry| entry.feature.as_str())
        .collect::<BTreeSet<_>>();
    let after = movers
        .iter()
        .filter(|entry| entry.position > first)
        .map(|entry| entry.feature.as_str())
        .collect::<BTreeSet<_>>();
    Some(F3dLossCode::HistoryStatePosesAssumed.note(format!(
        "scope {definition}: the states after features {} take the current occurrence poses and membership: features {} after them move, create, or re-own occurrences, and the decoder reads no earlier pose or membership.",
        join(features),
        join(after)
    )))
}

/// The loss of the states that each of `histories` stores after the state
/// of its last item in `entries`, which the walk takes the current state to
/// hold.
fn tails_assumed(
    definition: &ProductDefinitionId,
    entries: &[Entry<'_>],
    histories: &[History<'_>],
) -> Vec<LossNote> {
    histories
        .iter()
        .zip(last_items(entries, histories))
        .filter_map(|(history, last)| {
            let (state, feature) = last?;
            let later = history
                .states
                .range((std::ops::Bound::Excluded(state), std::ops::Bound::Unbounded))
                .map(|(later, _)| later.to_string())
                .collect::<Vec<_>>();
            (!later.is_empty()).then(|| {
                F3dLossCode::HistoryStateTailAssumed.note(format!(
                    "scope {definition}: no timeline position joins the states {} of {}, which it stores after state {state} of feature {feature}. They are the states of scopes that the timeline does not list, or of items whose scope tail is not read; the current state is taken to hold their change.",
                    later.join(", "),
                    history.blob.stream
                ))
            })
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

/// The losses of the bodies of the head of the history `stream` that the
/// states omit: the consumed bodies that no timeline item consumes, those
/// whose owner is not known, and the bodies that no body-map pair names; and
/// of the body-map pairs whose keys select no body of the head.
fn omitted(
    definition: &ProductDefinitionId,
    stream: &str,
    consumed: &Consumed,
    read: &Read,
) -> Vec<LossNote> {
    let mut out = [
        (
            &consumed.unplaced,
            "the current geometry omits them, and no timeline item consumes them",
        ),
        (
            &consumed.unowned,
            "the owner of their body-map pairs is not known",
        ),
    ]
    .into_iter()
    .map(|(keys, reason)| {
        let bodies = keys
            .iter()
            .filter_map(|key| read.head_consumed.get(key))
            .collect::<BTreeSet<_>>();
        (bodies, reason)
    })
    .chain([(
        read.head_unnamed.iter().collect(),
        "no body-map pair of the current model or of a consuming scope names them",
    )])
    .filter(|(bodies, _)| !bodies.is_empty())
    .map(|(bodies, reason)| {
        F3dLossCode::HistoryStateBodyOmitted.note(format!(
            "scope {definition}: the states after its timeline items omit the bodies {} of the head of {stream}: {reason}.",
            join(bodies.into_iter().map(BodyId::as_str))
        ))
    })
    .collect::<Vec<_>>();
    let unselected = consumed
        .keys()
        .into_iter()
        .filter(|key| !read.head_consumed.contains_key(key))
        .flat_map(|key| consumed.pairs.get(&key).into_iter().flatten())
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if !unselected.is_empty() {
        out.push(F3dLossCode::HistoryStateBodyOmitted.note(format!(
            "scope {definition}: the states after its timeline items join no body to the body-map pairs {} of {stream}: their keys select no body of its head.",
            join(unselected)
        )));
    }
    out
}

/// The bodies that the states give an owner the file does not state for
/// them, by history index, rule, and owner.
type Marks = BTreeMap<(usize, Rule, ProductDefinitionId), BTreeSet<BodyId>>;

/// The losses of `marks`.
fn marked(
    definition: &ProductDefinitionId,
    histories: &[History<'_>],
    marks: &Marks,
) -> Vec<LossNote> {
    marks
        .iter()
        .map(|((history, rule, owner), bodies)| {
            let stream = histories[*history].blob.stream;
            let bodies = join(bodies.iter().map(BodyId::as_str));
            match rule {
                Rule::Cluster => F3dLossCode::ComponentBodyOwnerFallback.note(format!(
                    "scope {definition}: the states after its timeline items give the consumed bodies {bodies} of {stream} the owner {owner} that their entity cluster places them in, because their body-map pairs state no Body-container component owner."
                )),
                Rule::Assumed => F3dLossCode::HistoryStateOwnerAssumed.note(format!(
                    "scope {definition}: the states after its timeline items give the bodies {bodies} of {stream} the owner {owner} of the current bodies of that history; the file states no owner for them."
                )),
            }
        })
        .collect()
}

/// The current bodies of each history, whose record space is the entry of
/// `spaces` at its index, and the current bodies in no history.
fn held<'b>(
    bodies: &'b BTreeSet<BodyId>,
    spaces: &[&str],
) -> (Vec<BTreeSet<BodyId>>, BTreeSet<&'b str>) {
    let mut held = vec![BTreeSet::new(); spaces.len()];
    let mut inherited = BTreeSet::new();
    for body in bodies {
        match spaces
            .iter()
            .position(|space| body.as_str().starts_with(space))
        {
            Some(at) => {
                held[at].insert(body.clone());
            }
            None => {
                inherited.insert(body.as_str());
            }
        }
    }
    (held, inherited)
}

/// The loss of the current bodies `inherited` of the scope `definition`,
/// which lie in no history.
fn inherited_bodies(
    definition: &ProductDefinitionId,
    inherited: &BTreeSet<&str>,
) -> Option<LossNote> {
    (!inherited.is_empty()).then(|| {
        F3dLossCode::HistoryStateBodyInherited.note(format!(
            "scope {definition}: the states after its timeline items hold the current bodies {} as the current geometry does, also before the item that makes them: they lie in no model blob with a history, and no timeline item is joined to them.",
            join(inherited.iter().copied())
        ))
    })
}

/// `items`, separated by commas.
fn join<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    items.into_iter().collect::<Vec<_>>().join(", ")
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
    /// The one owner of the current bodies of each history, if any.
    owners: &'c [Option<ProductDefinitionId>],
    consumed: &'c [Consumed],
    current: &'c ResolvedState,
}

impl Chain<'_, '_> {
    /// The stop at the item `index` of the plan, which leaves its state and
    /// the states of the earlier items unstated.
    fn stop(&self, index: usize, reason: String) -> Stop {
        Stop {
            reason,
            features: self.planned[index..]
                .iter()
                .map(|planned| self.entries[planned.entry].feature.clone())
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
    /// The owner that the component-ownership rule gives each history-only
    /// body-map pair, by pair id.
    pairs: &'r HashMap<String, HistoryPairOwner>,
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
        let configuration = match configuration(ir, &scope.definition) {
            Ok(configuration) => configuration,
            Err(reason) => return Ok(Err(reason.to_owned())),
        };
        let (end, built) = match &scope.at {
            Position::Start(_) => return Ok(Ok(())),
            Position::After(member) => {
                match scope.members.iter().position(|found| found == member) {
                    Some(at) => (at, &scope.members[..=at]),
                    None => {
                        return Ok(Err(
                            "the position of its current state is not read".to_owned()
                        ))
                    }
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
        let spaces = histories
            .iter()
            .map(|history| history.space.as_str())
            .collect::<Vec<_>>();
        let (held, inherited) = held(&scope.bodies, &spaces);
        let mut entries = Vec::new();
        let mut others = Vec::new();
        for (position, member) in built.iter().enumerate() {
            match self.entry(member, position, &histories, configuration)? {
                Some(entry) => entries.push(entry),
                None if self.stateless(member, configuration) => {}
                None => others.push(position),
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
        let inferred = resolve(&mut entries, &heads, &histories);
        mark_current(&mut entries, &heads, &others, end);
        self.losses.extend(unstated(&scope.definition, &entries));
        self.losses.extend(unresolved(&scope.definition, &entries));
        let Plan {
            planned,
            gaps,
            stop,
        } = plan(&entries, &heads, &names);
        self.losses.extend(
            gaps.iter()
                .chain(&stop)
                .filter_map(|stop| stopped(&scope.definition, stop)),
        );
        if planned.is_empty() {
            return Ok(Ok(()));
        }
        self.losses
            .extend(placed(&scope.definition, &entries, &histories, &inferred));
        self.losses
            .extend(tails_assumed(&scope.definition, &entries, &histories));
        self.losses
            .extend(inherited_bodies(&scope.definition, &inherited));
        let owners = history_owners(&held, &current);
        let consumed = self.consumed(&histories, &owners, &current)?;
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
            owners: &owners,
            consumed: &consumed,
            current: &current,
        };
        let mut marks = Marks::new();
        let stop = self.emit(&chain, configuration, &mut marks, out, carried)?;
        if let Some(stop) = stop {
            self.losses.extend(stopped(&scope.definition, &stop));
        }
        self.losses
            .extend(marked(&scope.definition, &histories, &marks));
        self.losses.extend(poses_assumed(
            &scope.definition,
            &entries,
            &planned[..out.len()],
        ));
        Ok(Ok(()))
    }

    /// Whether `member`, which has no entry, changes no state: a feature
    /// that is stateless in `configuration`. A container, or a feature with
    /// no stateless reading, is taken to change one.
    fn stateless(&self, member: &Member, configuration: Option<&ConfigurationId>) -> bool {
        match member {
            Member::Feature(feature) => self
                .ir
                .is_stateless(feature, configuration)
                .unwrap_or(false),
            Member::Container(_) => false,
        }
    }

    /// The entry of a member that is a feature of the root document.
    fn entry(
        &self,
        member: &'r Member,
        position: usize,
        histories: &[History<'_>],
        configuration: Option<&ConfigurationId>,
    ) -> Result<Option<Entry<'r>>, CodecError> {
        let Member::Feature(feature) = member else {
            return Ok(None);
        };
        let Some((stream, record)) = self.scope.items.get(feature) else {
            return Ok(None);
        };
        let (stateless, role) = eligibility(self.ir, feature, configuration)?;
        let tail = self.tails.get(&(stream.as_str(), *record)).copied();
        let kind = match tail.map(|tail| (tail, tail.state)) {
            _ if !document_prefix(stream).is_empty() => {
                Kind::Unresolved("its timeline item lies in another document".to_owned())
            }
            None if role == Role::Assembly => Kind::NoState,
            None => Kind::Tailless,
            Some((_, None)) => Kind::NoState,
            Some((tail, Some(state))) if tail.flags.is_some_and(|flags| flags.no_result) => {
                Kind::Unresolved(format!(
                    "its scope tail flags state {state} as holding no current result"
                ))
            }
            Some((tail, Some(state))) => bind(histories, &tail, state),
        };
        Ok(Some(Entry {
            feature,
            stream: stream.as_str(),
            record: *record,
            position,
            stateless,
            moves: role.moves(&kind),
            current: false,
            kind,
        }))
    }

    /// The consumed bodies of the head of each of `histories`: the
    /// history-only bodies of the root document's body maps that name its
    /// blob, with their owners in `current`, where `owners` holds the one
    /// owner of the current bodies of each history.
    fn consumed(
        &self,
        histories: &[History<'_>],
        owners: &[Option<ProductDefinitionId>],
        current: &ResolvedState,
    ) -> Result<Vec<Consumed>, CodecError> {
        let mut out = histories
            .iter()
            .map(|_| Consumed::default())
            .collect::<Vec<_>>();
        let mut keyed = vec![
            BTreeMap::<u64, (BTreeSet<BodyId>, BTreeSet<&ProductDefinitionId>)>::new();
            histories.len()
        ];
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
            let key = binding.asm_body_key;
            out[at]
                .pairs
                .entry(key)
                .or_default()
                .insert(binding.id().clone());
            let entry = keyed[at].entry(key).or_default();
            if let Some(pair) = self.pairs.get(binding.id().as_str()) {
                entry.0.extend(pair.peers.iter().cloned());
                entry.1.extend(&pair.definition);
            }
            let consumer = binding
                .consumers
                .iter()
                .filter_map(|consumer| positions.get(&(stream, *consumer)).copied())
                .min();
            match consumer {
                Some(consumer) => {
                    let first = out[at].placed.entry(key).or_insert(consumer);
                    *first = (*first).min(consumer);
                }
                None => {
                    out[at].unplaced.insert(key);
                }
            }
        }
        let defined = self
            .ir
            .model
            .product_definitions
            .iter()
            .map(|definition| &definition.id)
            .collect::<BTreeSet<_>>();
        for ((consumed, keyed), history) in out.iter_mut().zip(keyed).zip(owners) {
            let placed = &consumed.placed;
            consumed.unplaced.retain(|key| !placed.contains_key(key));
            let pairs = keyed
                .into_iter()
                .map(|(key, (peers, definitions))| {
                    let mut definitions = definitions.into_iter();
                    let definition = definitions.next().filter(|_| definitions.next().is_none());
                    (
                        key,
                        HistoryPairOwner {
                            peers,
                            definition: definition.cloned(),
                        },
                    )
                })
                .collect();
            consumed.own(&pairs, &defined, history.as_ref(), current);
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
        let budget = stored_states::Budget::new(stored_states::MAX_STATE_WORK.max(scaled));
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
            let keys = consumed[index].keys();
            let read = match stored_states::read(
                self.ctx,
                &history.blob,
                &held[index],
                &keys,
                &wanted,
                &self.ir.tolerances,
                &budget,
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
    /// plan, latest first, each based on the one before it, and to `marks`
    /// each body that takes an owner the file does not state for it. The
    /// chain stops before an evaluation whose native records, with those of
    /// the evaluations after it, can cost more retained bytes to attach to
    /// the document than the allowance left, and before an evaluation whose
    /// records exceed the entity limit left. Returns where the chain stops.
    fn emit(
        &mut self,
        chain: &Chain<'_, '_>,
        configuration: Option<&ConfigurationId>,
        marks: &mut Marks,
        out: &mut Vec<Evaluation>,
        carried: &mut Vec<UnknownRecord>,
    ) -> Result<Option<Stop>, CodecError> {
        let scope = self.scope;
        let current = chain.current;
        let empty = Topology::default();
        let consumed = chain
            .reads
            .iter()
            .zip(chain.consumed)
            .map(|(read, consumed)| {
                read.as_ref()
                    .map(|read| {
                        consumed
                            .owners
                            .iter()
                            .filter_map(|(key, owner)| Some((read.head_consumed.get(key)?, owner)))
                            .collect::<HashMap<_, _>>()
                    })
                    .unwrap_or_default()
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
        let mut owed = 0_u64;
        for (index, planned) in chain.planned.iter().enumerate() {
            let entry = &chain.entries[planned.entry];
            if depth >= limit {
                return Ok(Some(chain.stop(
                    index,
                    format!("the state chain ceiling {limit} is reached"),
                )));
            }
            let mut delta = StateDelta::default();
            let mut sources = Vec::<&StateRecords>::new();
            let mut marking = Vec::new();
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
                                format!(
                                    "state {} of {}: {reason}",
                                    want.state, history.blob.stream
                                ),
                            )))
                        }
                        None => {
                            return Ok(Some(chain.stop(
                                index,
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
                        current.owners().get(&body.id).map(|owner| (owner, None))
                    } else if let Some((owner, rule)) = consumed[at].get(&body.id) {
                        Some((owner, Some(*rule)))
                    } else {
                        chain.owners[at]
                            .as_ref()
                            .map(|owner| (owner, Some(Rule::Assumed)))
                    };
                    let Some((owner, rule)) = owner else {
                        return Ok(Some(chain.stop(
                            index,
                            format!(
                                "the owner of body {} of {} is not known",
                                body.id, history.blob.stream
                            ),
                        )));
                    };
                    if let Some(rule) = rule {
                        marking.push(((at, rule, owner.clone()), body.id.clone()));
                    }
                    if *owner != scope.definition {
                        delta.owners.insert(body.id.clone(), Some(owner.clone()));
                        entries.insert(body.id.clone(), owner.clone());
                    }
                }
                previous[at] = (target.clone(), next);
            }
            let held = carried.len();
            if let Err(reason) = self.carry(&delta, &sources, carried)? {
                return Ok(Some(chain.stop(index, reason)));
            }
            let owing = attach_bound(&carried[held..]).and_then(|cost| owed.checked_add(cost));
            match owing.filter(|owing| {
                let remaining = self.ctx.retained_allowance();
                self.ctx.observe_retained_allowance(remaining, *owing);
                *owing <= remaining
            }) {
                Some(owing) => owed = owing,
                None => {
                    carried.truncate(held);
                    return Ok(Some(chain.stop(
                        index,
                        "its native records do not fit the retained-byte allowance left".to_owned(),
                    )));
                }
            }
            let count = u64_from_index(len(&delta.records) + delta.removed.len() + 1);
            if let Err(reason) = stored_states::task(self.ctx, "the evaluation", |ctx| {
                ctx.charge_entities(count, "project F3D v11 history states")
                    .map(Ok)
            })? {
                carried.truncate(held);
                return Ok(Some(chain.stop(index, reason)));
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
            self.notes(&delta.records, current.records(), &sources, &id);
            sort(&mut delta.records);
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
                    Kind::NoState
                    | Kind::Tailless
                    | Kind::Unresolved(_)
                    | Kind::Ambiguous { .. } => None,
                },
            });
            for (key, body) in marking {
                marks.entry(key).or_default().insert(body);
            }
            depth += 1;
        }
        Ok(None)
    }

    /// Pushes the loss of each admission note of `sources` whose record
    /// `records` states from the state, where the current geometry holds
    /// no equal record, keyed by the evaluation `evaluation` (§3.2).
    fn notes(
        &mut self,
        records: &Topology,
        current: &Topology,
        sources: &[&StateRecords],
        evaluation: &EvaluationId,
    ) {
        let noted = sources
            .iter()
            .flat_map(|source| &source.notes)
            .map(Note::record)
            .collect::<HashSet<_>>();
        if noted.is_empty() {
            return;
        }
        let stated = stated(records, current, &noted);
        for note in sources.iter().flat_map(|source| &source.notes) {
            if stated.contains(note.record()) {
                self.losses
                    .push(note.loss(&format!("{evaluation} {}", note.record())));
            }
        }
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
        let mut additions = Vec::new();
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
            additions.push(unknown);
        }
        carried.extend(additions.into_iter().cloned());
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

/// A bound of the retained bytes that attaching `records` to a document
/// charges: the compact JSON text of each record's id and links, with each
/// character escaped to at most six bytes.
/// `None` where the bound exceeds `u64`.
fn attach_bound(records: &[UnknownRecord]) -> Option<u64> {
    let text = |length: usize| u64_from_index(length).checked_mul(6);
    records.iter().try_fold(0_u64, |total, record| {
        let id = text(record.id().as_str().len())?.checked_add(32)?;
        let links = record.links().iter().try_fold(0_u64, |sum, link| {
            sum.checked_add(text(link.len())?)?.checked_add(3)
        })?;
        total.checked_add(id)?.checked_add(links)
    })
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
        [one] => Some(*one),
        _ => tail
            .history
            .and_then(historical_brep_source)
            .and_then(|source| {
                stored
                    .iter()
                    .copied()
                    .find(|index| histories[*index].name.strip_prefix("BREP.") == Some(source))
            }),
    };
    let Some(history) = history else {
        return Kind::Ambiguous {
            state,
            stored,
            previous: tail.previous,
        };
    };
    Kind::Bound {
        history,
        state,
        previous: before(&histories[history].states, state, tail.previous),
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

/// The ids of `noted` whose record `delta` holds and `current` does not
/// hold as it is.
fn stated<'t>(delta: &'t Topology, current: &Topology, noted: &HashSet<&str>) -> HashSet<&'t str> {
    let mut out = HashSet::new();
    macro_rules! find {
        ($($field:ident $arm:ident),*) => {$({
            let held = delta.$field.iter()
                .filter(|record| noted.contains(record.record_identity()))
                .collect::<Vec<_>>();
            if !held.is_empty() {
                let present = current.$field.iter()
                    .filter(|record| noted.contains(record.record_identity()))
                    .map(|record| (record.record_identity(), record))
                    .collect::<HashMap<_, _>>();
                for record in held {
                    if present.get(record.record_identity()) != Some(&record) {
                        out.insert(record.record_identity());
                    }
                }
            }
        })*};
    }
    topology_arenas!(find);
    out
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
