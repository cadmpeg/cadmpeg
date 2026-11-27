// SPDX-License-Identifier: Apache-2.0
//! The history states that hold the bodies of Base and Split features as the
//! evaluations of the part's timeline ([CADIR §5.4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/cadir-v11.md#54-evaluations),
//! [spec §13.10](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/inventor.md#1310-feature-history-states)).
//!
//! The kernel chains back from the current model, so each evaluation is
//! based on the next later one, and the latest on the current state. A
//! decoded state holds every record of the state in the slot ids of the
//! current geometry, so its delta is the difference of the two record sets.

use std::collections::BTreeMap;

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
use cadmpeg_ir::CadIr;

use crate::loss::InventorLossCode;

/// The place in the timeline of the state that holds a history body.
struct Placed {
    /// Index of the position's member in the timeline; `None` for the start.
    index: Option<usize>,
    position: Position,
    ordinal: u32,
}

/// One evaluation to state, and the history bodies its state holds.
struct Planned {
    position: Position,
    ordinal: u32,
    bodies: Vec<HistoryBodyId>,
}

/// State each decoded history state that holds a history body as the
/// evaluation of its timeline position, restate the body of a Base feature
/// whose state is the current geometry by that feature, and charge each
/// other history body.
///
/// # Errors
///
/// Returns a codec error where an evaluation id does not mint, or the
/// decode's resource policy refuses the work.
pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    states: &BTreeMap<u32, Topology>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if ir.model.history_bodies.is_empty() {
        return Ok(());
    }
    let mut unplaced = Vec::new();
    let Some(Timeline {
        scope,
        at,
        id: container,
        members,
        mut records,
    }) = timeline(ir)
    else {
        for body in &ir.model.history_bodies {
            unplaced.push((body, "the part has no timeline with a current state"));
        }
        charge(&unplaced, losses);
        return Ok(());
    };
    let current = member_index(&members, &at);
    let mut planned = BTreeMap::<Option<usize>, Planned>::new();
    let mut held = Vec::new();
    for body in &ir.model.history_bodies {
        let placed = match place(ir, &container, &members, body) {
            Ok(placed) => placed,
            Err(why) => {
                unplaced.push((body, why));
                continue;
            }
        };
        if !states.contains_key(&placed.ordinal) {
            unplaced.push((body, "the state is not decoded"));
            continue;
        }
        if placed.index > current {
            unplaced.push((body, "the state is after the rollback position"));
            continue;
        }
        if placed.index == current {
            match &body.produced_by {
                Some(feature) => held.push((body, feature, placed.ordinal)),
                None => unplaced.push((body, "the current geometry holds the state")),
            }
            continue;
        }
        let entry = planned.entry(placed.index).or_insert_with(|| Planned {
            position: placed.position,
            ordinal: placed.ordinal,
            bodies: Vec::new(),
        });
        if entry.ordinal == placed.ordinal {
            entry.bodies.push(body.id.clone());
        } else {
            unplaced.push((body, "another history state holds the position"));
        }
    }
    if planned.is_empty() && held.is_empty() {
        charge(&unplaced, losses);
        return Ok(());
    }
    let mut restated = Vec::new();
    // A Base feature whose state is the current state restates its body
    // through its stored bodies where the decoded state is the current
    // geometry record for record.
    for (body, feature, ordinal) in held {
        let same = states
            .get(&ordinal)
            .is_some_and(|state| StateDelta::between(&records, state) == StateDelta::default());
        let written = ir
            .design
            .features
            .binary_search_by(|item| item.id.cmp(feature))
            .is_ok();
        match (same, written) {
            (true, true) => restated.push((body.id.clone(), feature.as_str().to_owned())),
            (false, _) => unplaced.push((body, "the state differs from the current geometry")),
            (true, false) => unplaced.push((body, "the feature has no design record")),
        }
    }
    charge(&unplaced, losses);
    let mut base = StateBase::Current(at.clone());
    let mut evaluations = Vec::with_capacity(planned.len());
    for Planned {
        position,
        ordinal,
        bodies,
    } in planned.into_values().rev()
    {
        let Some(state) = states.get(&ordinal) else {
            continue;
        };
        let delta = StateDelta::between(&records, state);
        ctx.charge_entities(1, "state an Inventor history state")?;
        let id = EvaluationId::mint(format!("inventor:design:evaluation#{ordinal}"))
            .map_err(CodecError::malformed)?;
        restated.extend(
            bodies
                .into_iter()
                .map(|body| (body, id.as_str().to_owned())),
        );
        evaluations.push(Evaluation {
            id: id.clone(),
            scope: scope.clone(),
            configuration: None,
            of: Evaluated::State(position),
            status: EvaluationStatus::Current,
            content: Some(StateContent { base, delta }),
            native_ref: None,
        });
        base = StateBase::Evaluation(id);
        records.clone_from(state);
    }
    ir.design.evaluations.extend(evaluations);
    ir.design.evaluations.sort_by(|a, b| a.id.cmp(&b.id));
    for (body, by) in restated {
        ir.model
            .restated_mut()
            .record(body.as_str(), Restater::Record(by), ["state"]);
    }
    Ok(())
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

/// The index of the member of `members` that `at` is after; `None` for a
/// start position.
fn member_index(members: &[Member], at: &Position) -> Option<usize> {
    match at {
        Position::Start(_) => None,
        Position::After(member) => members.iter().position(|candidate| candidate == member),
    }
}

/// The position whose state holds `body`: the state after a Base, and the
/// state entering a Split, which is after the nearest earlier member that
/// is a feature with a state, else the timeline's start.
fn place(
    ir: &CadIr,
    timeline: &ContainerId,
    members: &[Member],
    body: &HistoryBody,
) -> Result<Placed, &'static str> {
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
            index: Some(index),
            position: Position::After(member),
            ordinal,
        });
    }
    let earlier = members[..index]
        .iter()
        .enumerate()
        .rev()
        .find_map(|(at, member)| {
            let Member::Feature(feature) = member else {
                return None;
            };
            (!stateless(ir, feature)).then(|| (at, member.clone()))
        });
    Ok(match earlier {
        Some((at, member)) => Placed {
            index: Some(at),
            position: Position::After(member),
            ordinal,
        },
        None => Placed {
            index: None,
            position: Position::Start(timeline.clone()),
            ordinal,
        },
    })
}

fn stateless(ir: &CadIr, feature: &FeatureId) -> bool {
    ir.is_stateless(feature, None).unwrap_or(false)
}

fn charge(unplaced: &[(&HistoryBody, &str)], losses: &mut Vec<LossNote>) {
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
