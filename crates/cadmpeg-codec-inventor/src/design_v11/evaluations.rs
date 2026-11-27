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
    bodies: Vec<HistoryBodyId>,
    /// The undecoded records of the state that its records reference and
    /// that the document does not hold yet.
    unknowns: Vec<UnknownRecord>,
}

type Why = Cow<'static, str>;

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
    states: &BTreeMap<u32, StateRecords>,
    fidelity: &SourceFidelity,
    retained: &mut Vec<(u32, UnknownRecord)>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if ir.model.history_bodies.is_empty() {
        return Ok(());
    }
    let mut unplaced: Vec<(&HistoryBody, Why)> = Vec::new();
    let Some(Timeline {
        scope,
        at,
        id: container,
        members,
        mut records,
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
        let placed = match place(ir, &container, &members, body) {
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
            unknowns: Vec::new(),
        });
        if entry.ordinal == placed.ordinal {
            entry.bodies.push(body.id.clone());
        } else {
            unplaced.push((body, "another history state holds the position".into()));
        }
    }
    if planned.is_empty() && held.is_empty() {
        charge(&unplaced, losses);
        return Ok(());
    }
    let mut restated: Vec<(HistoryBodyId, String, Vec<&'static str>)> = Vec::new();
    let mut native = BTreeMap::<u32, HistoryState>::new();
    // A history state at the current position whose records are the
    // current geometry's is the current state: the feature that the body
    // is the result or the input of restates the body, and the current
    // state's native record restates the state that holds it.
    let mut current_state = None::<HistoryState>;
    for (body, feature, state) in held {
        let same = StateDelta::between(&records, &state.geometry) == StateDelta::default();
        let written = ir
            .design
            .features
            .binary_search_by(|item| item.id.cmp(feature))
            .is_ok();
        match (same, written) {
            (true, true) => {
                restated.push((body.id.clone(), feature.as_str().to_owned(), vec!["state"]));
                let record = current_state.get_or_insert_with(|| {
                    HistoryState::new(body.state.ordinal, body.state.identifier)
                });
                if record.ordinal == body.state.ordinal {
                    restated.push((
                        body.id.clone(),
                        scope.as_str().to_owned(),
                        all_but_state(body),
                    ));
                }
            }
            (false, _) => {
                unplaced.push((body, "the state differs from the current geometry".into()));
            }
            (true, false) => unplaced.push((body, "the feature has no design record".into())),
        }
    }
    // The document holds one undecoded record per id. A state reads its
    // own record of an id where the document holds none, and shares the
    // document's where that holds the same bytes.
    let wanted = planned
        .values()
        .flat_map(|plan| {
            let state: &StateRecords = plan.state;
            state.unknowns.iter().map(|record| record.id().as_str())
        })
        .collect::<BTreeSet<_>>();
    let mut document = BTreeMap::<String, Option<&[u8]>>::new();
    for record in ir.native_unknowns_iter("inventor") {
        let record = record.map_err(|error| CodecError::malformed(error.to_string()))?;
        if wanted.contains(record.id.as_str()) {
            document.insert(record.id.as_str().to_owned(), None);
        }
    }
    for id in wanted {
        if let Some(record) = fidelity.retained_record(id) {
            document.insert(id.to_owned(), record.data());
        }
    }
    let mut open = Vec::new();
    for (index, plan) in planned.iter_mut().rev() {
        let own = plan
            .state
            .unknowns
            .iter()
            .map(|record| (record.id().as_str(), record))
            .collect::<BTreeMap<_, _>>();
        let targets =
            ir.open_references(ctx, &plan.state.geometry, &own.keys().copied().collect())?;
        let mut kept = Vec::new();
        let mut why = None::<Why>;
        for target in &targets {
            let Some(&record) = own.get(target.as_str()) else {
                why = Some("the state names a record that it does not hold".into());
                break;
            };
            match document.get(target) {
                None => kept.push(record),
                Some(Some(bytes)) if record.data() == Some(*bytes) => {}
                Some(_) => {
                    why = Some(
                        format!(
                            "its undecoded record {target} differs from the document's record of that id"
                        )
                        .into(),
                    );
                    break;
                }
            }
        }
        match why {
            None => {
                document.extend(
                    kept.iter()
                        .map(|record| (record.id().as_str().to_owned(), record.data())),
                );
                plan.unknowns = kept.into_iter().cloned().collect();
            }
            Some(why) => open.push((*index, why)),
        }
    }
    for (index, why) in open {
        if let Some(plan) = planned.remove(&index) {
            unplaced.extend(
                ir.model
                    .history_bodies
                    .iter()
                    .filter(|body| plan.bodies.contains(&body.id))
                    .map(|body| (body, why.clone())),
            );
        }
    }
    charge(&unplaced, losses);
    let mut base = StateBase::Current(at.clone());
    let mut evaluations = Vec::with_capacity(planned.len());
    let mut minted = BTreeMap::<u32, usize>::new();
    let mut unknowns = Vec::new();
    for Planned {
        position,
        ordinal,
        state,
        identifier,
        bodies,
        unknowns: kept,
    } in planned.into_values().rev()
    {
        let delta = StateDelta::between(&records, &state.geometry);
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
        restated.extend(
            bodies
                .into_iter()
                .map(|body| (body, id.as_str().to_owned(), Vec::new())),
        );
        unknowns.extend(kept.into_iter().map(|record| (ordinal, record)));
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
        records.clone_from(&state.geometry);
    }
    if let Some(record) = current_state {
        if let Some(current) = ir
            .design
            .scopes
            .first_mut()
            .and_then(|scope| scope.current.first_mut())
        {
            current.native_ref = Some(record.id.clone());
        }
        native.entry(record.ordinal).or_insert(record);
    }
    if !native.is_empty() {
        let native = native.into_values().collect::<Vec<_>>();
        ir.native
            .namespace_mut(cadmpeg_ir::codec_id!("inventor"))
            .set_arena(ctx, HISTORY_STATES, &native)?;
    }
    ir.design.evaluations.extend(evaluations);
    ir.design.evaluations.sort_by(|a, b| a.id.cmp(&b.id));
    for (body, by, cut) in restated {
        ir.model
            .restated_mut()
            .record(body.as_str(), Restater::Record(by), cut);
    }
    retained.extend(unknowns);
    Ok(())
}

/// The wire keys of the fields of `body` other than its state, which a
/// current state restates only through its native record.
fn all_but_state(body: &HistoryBody) -> Vec<&'static str> {
    let mut keys = vec!["id", "topology"];
    for (key, held) in [
        ("produced_by", body.produced_by.is_some()),
        ("consumed_by", body.consumed_by.is_some()),
        ("state_of", body.state_of.is_some()),
    ] {
        if held {
            keys.push(key);
        }
    }
    keys
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

fn charge(unplaced: &[(&HistoryBody, Why)], losses: &mut Vec<LossNote>) {
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
