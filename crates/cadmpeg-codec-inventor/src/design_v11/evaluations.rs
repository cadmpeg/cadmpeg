// SPDX-License-Identifier: Apache-2.0
//! The history states that hold the bodies of Base and Split features as the
//! evaluations of the part's timeline ([CADIR §5.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/cadir-v11.md#54-evaluations),
//! [spec §13.10](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/inventor.md#1310-feature-history-states)).
//!
//! The kernel chains back from the current model, so each evaluation is
//! based on the next later one, and the latest on the current state. A
//! decoded state holds every record of the state in the slot ids of the
//! current geometry, so its delta is the difference of the two record sets.
//! The state at the current position is the current state, which names the
//! history state as its native record (§5.5).

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::container::Container;
use cadmpeg_ir::design::evaluation::{
    Evaluation, EvaluationStatus, StateBase, StateContent, StateDelta,
};
use cadmpeg_ir::design::feature::{Feature as DesignFeature, FeatureOperation};
use cadmpeg_ir::design::ids::{ContainerId, EvaluationId};
use cadmpeg_ir::design::projection::restated::Restater;
use cadmpeg_ir::design::refs::{Evaluated, Member, Position};
use cadmpeg_ir::document::Topology;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::history_body::HistoryBody;
use cadmpeg_ir::ids::{HistoryBodyId, ProductDefinitionId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{CadIr, SourceFidelity, UnknownRecord};
use serde::Serialize;

use crate::feature::history_bodies::StateRecords;
use crate::feature::history_states::{Binding, FeatureHistoryState, StateRef};
use crate::loss::InventorLossCode;

/// The native arena of the history states that evaluations and the
/// current state name.
const HISTORY_STATES: &str = "history_states";

/// One history state that an evaluation or a current state states: the
/// native record its `native_ref` names.
#[derive(Serialize)]
struct HistoryState {
    id: String,
    /// Position in the chain, oldest state first.
    ordinal: u32,
    /// Identifier that the source assigns to the state.
    identifier: i64,
}

impl HistoryState {
    fn new(ordinal: u32, identifier: i64) -> Self {
        Self {
            id: format!("inventor:history:state#{ordinal}"),
            ordinal,
            identifier,
        }
    }
}

/// The place in the timeline of the state that holds a history body.
struct Placed<'a> {
    /// The feature the body is the result or the input of.
    feature: &'a FeatureId,
    /// Index of the position's member in the timeline; `None` for the start.
    index: Option<usize>,
    position: Position,
    ordinal: u32,
}

/// One evaluation to state, and the history bodies its state holds.
struct Planned<'a> {
    position: Position,
    ordinal: u32,
    state: &'a StateRecords,
    identifier: i64,
    bodies: Vec<&'a HistoryBody>,
}

#[derive(Clone, Copy)]
pub(crate) struct Evidence<'a> {
    pub(crate) active: Option<StateRef>,
    pub(crate) features: &'a [FeatureHistoryState],
    pub(crate) states: &'a BTreeMap<u32, StateRecords>,
}

/// State each decoded history state that holds a history body as the
/// evaluation of its timeline position, state the history state at the
/// current position as the current state's record where it is the current
/// geometry, and charge each other history body. `fidelity` holds the
/// bytes of the undecoded records the document holds. The undecoded
/// records that the stated states reference and the document does not hold
/// are added to `retained`, with the ordinal of their state.
///
/// # Errors
///
/// Returns a codec error where an evaluation id does not mint, the history
/// states do not serialize, the document's undecoded records do not read,
/// or the decode's resource policy refuses the work.
pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    history: Evidence<'_>,
    fidelity: &SourceFidelity,
    retained: &mut Vec<(u32, UnknownRecord)>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let Evidence {
        active,
        features,
        states,
    } = history;
    if ir.model.history_bodies.is_empty() && active.is_none() {
        return Ok(());
    }
    let mut unplaced: Vec<(&HistoryBody, Cow<'static, str>)> = Vec::new();
    let Some(Timeline {
        scope,
        at,
        id: container,
        members,
        records: current_records,
    }) = timeline(ir)
    else {
        for body in &ir.model.history_bodies {
            unplaced.push((body, "the part has no timeline with a current state".into()));
        }
        charge(&unplaced, losses);
        return Ok(());
    };
    let current = state_index(ir, &members, &at);
    let mut planned = BTreeMap::<Option<usize>, Planned<'_>>::new();
    let mut held = Vec::new();
    for body in &ir.model.history_bodies {
        let placed = match place(ir, &container, &members, body, features) {
            Ok(placed) => placed,
            Err(why) => {
                unplaced.push((body, why.into()));
                continue;
            }
        };
        let Some(state) = states.get(&placed.ordinal) else {
            unplaced.push((body, "the state is not decoded".into()));
            continue;
        };
        if active.is_some_and(|active| {
            active.ordinal == placed.ordinal && active.state_id != body.state.identifier
        }) {
            unplaced.push((
                body,
                "its identifier conflicts with the active state".into(),
            ));
            continue;
        }
        if placed.index > current {
            unplaced.push((body, "the state is after the rollback position".into()));
            continue;
        }
        if placed.index == current {
            held.push((body, placed.feature, state));
            continue;
        }
        let entry = planned.entry(placed.index).or_insert_with(|| Planned {
            position: placed.position,
            ordinal: placed.ordinal,
            state,
            identifier: body.state.identifier,
            bodies: Vec::new(),
        });
        if entry.ordinal == placed.ordinal && entry.identifier == body.state.identifier {
            entry.bodies.push(body);
        } else {
            unplaced.push((body, "another history state holds the position".into()));
        }
    }
    if planned.is_empty() && held.is_empty() && active.is_none() {
        charge(&unplaced, losses);
        return Ok(());
    }
    let wanted = planned
        .values()
        .map(|plan| plan.state)
        .chain(held.iter().map(|&(_, _, state)| state))
        .flat_map(|state| state.unknowns.iter().map(|record| record.id().as_str()))
        .collect::<BTreeSet<_>>();
    let mut document = BTreeMap::<String, Option<&[u8]>>::new();
    if !wanted.is_empty() {
        for record in ir.native_unknowns_iter("inventor") {
            let record = record.map_err(|error| CodecError::malformed(error.to_string()))?;
            if wanted.contains(record.id.as_str()) {
                document.insert(record.id.as_str().to_owned(), None);
            }
        }
    }
    for id in wanted {
        if let Some(record) = fidelity.retained_record(id) {
            document.insert(id.to_owned(), record.data());
        }
    }
    let mut restated = Vec::new();
    let mut native = BTreeMap::<u32, HistoryState>::new();
    if let Some(active) = active {
        native.insert(
            active.ordinal,
            HistoryState::new(active.ordinal, active.state_id),
        );
    }
    let mut unknowns = Vec::new();
    for (body, feature, state) in held {
        if current_records != state.geometry
            && StateDelta::between(&current_records, &state.geometry) != StateDelta::default()
        {
            unplaced.push((body, "the state differs from the current geometry".into()));
            continue;
        }
        let Ok(feature_index) = ir
            .design
            .features
            .binary_search_by(|item| item.id.cmp(feature))
        else {
            unplaced.push((body, "the feature has no design record".into()));
            continue;
        };
        let ordinal = body.state.ordinal;
        if let Some(why) = close_state(ctx, ir, (ordinal, state), &mut document, &mut unknowns)? {
            unplaced.push((body, why));
            continue;
        }
        let same_state = active.is_some_and(|record| record.ordinal == ordinal);
        restate_body(
            &mut restated,
            body,
            state,
            scope.as_str(),
            same_state,
            Some(&mut ir.design.features[feature_index]),
        );
    }
    let mut open = Vec::new();
    for (index, plan) in planned.iter().rev() {
        if let Some(why) = close_state(
            ctx,
            ir,
            (plan.ordinal, plan.state),
            &mut document,
            &mut unknowns,
        )? {
            open.push((*index, why));
        }
    }
    for (index, why) in open {
        if let Some(plan) = planned.remove(&index) {
            unplaced.extend(plan.bodies.into_iter().map(|body| (body, why.clone())));
        }
    }
    charge(&unplaced, losses);
    let mut records = &current_records;
    let mut base = StateBase::Current(at.clone());
    let mut evaluations = Vec::with_capacity(planned.len());
    let mut minted = BTreeMap::<u32, usize>::new();
    for Planned {
        position,
        ordinal,
        state,
        identifier,
        bodies,
    } in planned.into_values().rev()
    {
        let delta = StateDelta::between(records, &state.geometry);
        ctx.charge_entities(1, "state an Inventor history state")?;
        let repeat = minted.entry(ordinal).or_default();
        let id = match *repeat {
            0 => format!("inventor:design:evaluation#{ordinal}"),
            repeat => format!("inventor:design:evaluation#{ordinal}-{repeat}"),
        };
        *repeat += 1;
        let id = EvaluationId::mint(id).map_err(CodecError::malformed)?;
        let native_ref = native
            .entry(ordinal)
            .or_insert_with(|| HistoryState::new(ordinal, identifier))
            .id
            .clone();
        for body in bodies {
            let feature = body.produced_by.as_ref().or(body.consumed_by.as_ref());
            let feature = feature
                .and_then(|feature| {
                    ir.design
                        .features
                        .binary_search_by(|item| item.id.cmp(feature))
                        .ok()
                })
                .map(|index| &mut ir.design.features[index]);
            restate_body(&mut restated, body, state, id.as_str(), true, feature);
        }
        evaluations.push(Evaluation {
            id: id.clone(),
            scope: scope.clone(),
            configuration: None,
            of: Evaluated::State(position),
            status: EvaluationStatus::Current,
            content: Some(StateContent { base, delta }),
            native_ref: Some(native_ref),
        });
        base = StateBase::Evaluation(id);
        records = &state.geometry;
    }
    if let Some(record) = active.and_then(|active| native.get(&active.ordinal)) {
        if let Some(current) = ir
            .design
            .scopes
            .first_mut()
            .and_then(|scope| scope.current.first_mut())
        {
            current.native_ref = Some(record.id.clone());
        }
    }
    if !native.is_empty() {
        let native = native.into_values().collect::<Vec<_>>();
        ir.native
            .namespace_mut(cadmpeg_ir::codec_id!("inventor"))
            .set_arena(ctx, HISTORY_STATES, &native)?;
    }
    ir.design.evaluations.extend(evaluations);
    ir.design.evaluations.sort_by(|a, b| a.id.cmp(&b.id));
    for Restatement { nest, by, cut } in restated {
        ir.model
            .restated_mut()
            .record(nest.as_str(), Restater::Record(by), cut);
    }
    retained.extend(unknowns);
    Ok(())
}

/// Resolve the referenced unknowns of one state against the document-wide byte
/// images. Publish newly needed records only after every reference closes.
fn close_state<'a>(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    (ordinal, state): (u32, &'a StateRecords),
    document: &mut BTreeMap<String, Option<&'a [u8]>>,
    retained: &mut Vec<(u32, UnknownRecord)>,
) -> Result<Option<Cow<'static, str>>, CodecError> {
    let own = state
        .unknowns
        .iter()
        .map(|record| (record.id().as_str(), record))
        .collect::<BTreeMap<_, _>>();
    let targets = ir.open_references(ctx, &state.geometry, &own.keys().copied().collect())?;
    let mut kept = Vec::new();
    for target in targets {
        let Some(&record) = own.get(target.as_str()) else {
            return Ok(Some(
                "the state names a record that it does not hold".into(),
            ));
        };
        match (document.get(&target), record.data()) {
            (None, _) => kept.push((target, record)),
            (Some(Some(bytes)), Some(own)) if *bytes == own => {}
            (Some(Some(_)), Some(_)) => {
                return Ok(Some(
                    format!(
                        "its undecoded record {target} differs from the document's record of that id"
                    )
                    .into(),
                ));
            }
            (Some(_), _) => {
                return Ok(Some(
                    format!(
                        "its undecoded record {target} cannot be compared without both byte images"
                    )
                    .into(),
                ));
            }
        }
    }
    for (id, record) in kept {
        document.insert(id, record.data());
        retained.push((ordinal, record.clone()));
    }
    Ok(None)
}

struct Restatement {
    nest: HistoryBodyId,
    by: String,
    cut: Vec<&'static str>,
}

fn restate_body(
    restated: &mut Vec<Restatement>,
    body: &HistoryBody,
    state: &StateRecords,
    state_writer: &str,
    writes_state_id: bool,
    feature: Option<&mut DesignFeature>,
) {
    let association = body
        .produced_by
        .as_ref()
        .map_or("consumed_by", |_| "produced_by");
    let mut cut = vec![association];
    if !writes_state_id {
        cut.push("state");
    }
    if body.state_of.is_some() {
        cut.push("state_of");
    }
    restated.push(Restatement {
        nest: body.id.clone(),
        by: state_writer.to_owned(),
        cut,
    });
    if let Some(feature) = feature {
        if associates(body, state, feature, writes_state_id) {
            restated.push(Restatement {
                nest: body.id.clone(),
                by: feature.id.as_str().to_owned(),
                cut: body_fields(body)
                    .filter(|&field| field != association)
                    .collect(),
            });
        }
    }
}

fn associates(
    body: &HistoryBody,
    state: &StateRecords,
    feature: &mut DesignFeature,
    can_write_slot: bool,
) -> bool {
    if body.produced_by.is_none() {
        return matches!(feature.definition.operation, FeatureOperation::Split(_));
    }
    let FeatureOperation::Stored { bodies, .. } = &mut feature.definition.operation else {
        return false;
    };
    let Some(slot) = state
        .body_slots
        .get(&body.id)
        .filter(|slot| state.geometry.bodies.iter().any(|body| &body.id == *slot))
    else {
        return false;
    };
    if can_write_slot {
        *bodies = Some(vec![slot.clone()]);
    }
    bodies.as_deref() == Some(std::slice::from_ref(slot))
}

/// Reapply the stored-body links backed by admitted states when validating the
/// operation projection against the legacy arenas.
pub(super) fn restate_stored_links(ir: &CadIr, expected: &mut CadIr) {
    let current = ir
        .design
        .scopes
        .first()
        .and_then(|scope| scope.current.first());
    let current_member = current.and_then(|current| {
        let Container::Ordered { members, .. } = super::timeline_of(ir, &current.at)? else {
            return None;
        };
        state_index(ir, members, &current.at).map(|index| &members[index])
    });
    for body in &ir.model.history_bodies {
        let Some(feature) = &body.produced_by else {
            continue;
        };
        let member = Member::Feature(feature.clone());
        let by_feature = Restater::Record(feature.as_str().to_owned());
        if !ir
            .model
            .restated()
            .get(body.id.as_str())
            .is_some_and(|entry| {
                entry
                    .restaters()
                    .any(|(by, cut)| *by == by_feature && !cut.contains("produced_by"))
            })
        {
            continue;
        }
        let state_id = HistoryState::new(body.state.ordinal, body.state.identifier).id;
        let is_named = |native: Option<&str>| native == Some(state_id.as_str());
        let stated = current_member == Some(&member)
            && current.is_some_and(|current| is_named(current.native_ref.as_deref()))
            || ir.design.evaluations.iter().any(|evaluation| {
                is_named(evaluation.native_ref.as_deref())
                    && evaluation.of == Evaluated::State(Position::After(member.clone()))
                    && evaluation.content.is_some()
            });
        if !stated {
            continue;
        }
        let Ok(index) = expected
            .design
            .features
            .binary_search_by(|item| item.id.cmp(feature))
        else {
            continue;
        };
        let FeatureOperation::Stored { bodies, .. } =
            &mut expected.design.features[index].definition.operation
        else {
            continue;
        };
        if let Some(slot) = crate::feature::history_bodies::original_body_slot(body) {
            *bodies = Some(vec![slot]);
        }
    }
}

fn body_fields(body: &HistoryBody) -> impl Iterator<Item = &'static str> {
    [
        ("id", true),
        ("topology", true),
        ("state", true),
        ("produced_by", body.produced_by.is_some()),
        ("consumed_by", body.consumed_by.is_some()),
        ("state_of", body.state_of.is_some()),
    ]
    .into_iter()
    .filter_map(|(key, held)| held.then_some(key))
}

/// The ordered container that holds the current state of the part scope.
struct Timeline {
    scope: ProductDefinitionId,
    /// The position of the current state.
    at: Position,
    id: ContainerId,
    members: Vec<Member>,
    /// The records of the current state.
    records: Topology,
}

fn timeline(ir: &CadIr) -> Option<Timeline> {
    let scope = ir.design.scopes.first()?;
    let at = scope.current.first()?.at.clone();
    let Container::Ordered { id, members, .. } = super::timeline_of(ir, &at)? else {
        return None;
    };
    let records = ir
        .resolved_current_state(&scope.definition, &at)
        .ok()?
        .records()
        .clone();
    Some(Timeline {
        scope: scope.definition.clone(),
        at,
        id: id.clone(),
        members: members.clone(),
        records,
    })
}

/// The index of the member whose state is the state at `at`: the nearest
/// member at or before the member that `at` is after that has a state of
/// its own. `None` for a start position, for a member that is not in
/// `members`, and where no such member has a state.
fn state_index(ir: &CadIr, members: &[Member], at: &Position) -> Option<usize> {
    let Position::After(member) = at else {
        return None;
    };
    let index = members.iter().position(|candidate| candidate == member)?;
    members[..=index]
        .iter()
        .rposition(|member| has_state(ir, member))
}

/// The position whose state holds `body`: the state after a Base, and the
/// state entering a Split, which is after the nearest earlier member that
/// is a feature with a state, else the timeline's start.
fn place<'a>(
    ir: &CadIr,
    timeline: &ContainerId,
    members: &[Member],
    body: &'a HistoryBody,
    bindings: &[FeatureHistoryState],
) -> Result<Placed<'a>, &'static str> {
    let (feature, entering) = match (&body.produced_by, &body.consumed_by) {
        (Some(feature), None) => (feature, false),
        (None, Some(feature)) => (feature, true),
        _ => return Err("the history body names no single feature"),
    };
    let member = Member::Feature(feature.clone());
    let index = members
        .iter()
        .position(|candidate| candidate == &member)
        .ok_or("the feature is no member of the timeline")?;
    let ordinal = body.state.ordinal;
    if !entering {
        if stateless(ir, feature) {
            return Err("the feature is stateless");
        }
        return Ok(Placed {
            feature,
            index: Some(index),
            position: Position::After(member),
            ordinal,
        });
    }
    let earlier = members[..index]
        .iter()
        .rposition(|member| has_state(ir, member));
    let bound = |member: &Member| {
        let Member::Feature(id) = member else {
            return None;
        };
        let feature = ir
            .design
            .features
            .binary_search_by(|item| item.id.cmp(id))
            .ok()?;
        let native = ir.design.features[feature].native_ref.as_deref()?;
        let mut rows = bindings
            .iter()
            .filter(|row| row.feature == native && row.binding == Binding::Bound);
        let row = rows.next()?;
        rows.next().is_none().then_some(row)
    };
    if bound(&member).is_some_and(|row| row.state_before != Some(ordinal))
        || earlier
            .and_then(|at| bound(&members[at]))
            .and_then(|row| row.state_after)
            .is_some_and(|after| after.ordinal > ordinal)
    {
        return Err("the Split input ordinal conflicts with its timeline boundary");
    }
    Ok(match earlier {
        Some(at) => Placed {
            feature,
            index: Some(at),
            position: Position::After(members[at].clone()),
            ordinal,
        },
        None => Placed {
            feature,
            index: None,
            position: Position::Start(timeline.clone()),
            ordinal,
        },
    })
}

fn stateless(ir: &CadIr, feature: &FeatureId) -> bool {
    ir.is_stateless(feature, None).unwrap_or(false)
}

/// Whether `member` is a feature that is not stateless.
fn has_state(ir: &CadIr, member: &Member) -> bool {
    matches!(member, Member::Feature(feature) if !stateless(ir, feature))
}

fn charge(unplaced: &[(&HistoryBody, Cow<'static, str>)], losses: &mut Vec<LossNote>) {
    for (body, why) in unplaced {
        let feature = body
            .produced_by
            .as_ref()
            .or(body.consumed_by.as_ref())
            .map_or("no feature", FeatureId::as_str);
        losses.push(InventorLossCode::FeatureHistoryStateUnplaced.note(format!(
            "History body {} of {feature} in history state {} has no evaluation: {why}.",
            body.id, body.state.ordinal
        )));
    }
}

#[cfg(test)]
mod tests;
