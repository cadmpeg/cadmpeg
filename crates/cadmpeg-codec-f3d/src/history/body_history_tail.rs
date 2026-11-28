// SPDX-License-Identifier: Apache-2.0
//! Retain body changes after the last feature with a bound result.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::{work_units, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{
    Feature, FeatureContent, FeatureDefinition, FeatureId, FeatureOperation,
};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::report::loss::LossNote;

use super::history_states::PlacedState;
use crate::history_records::{AsmDeltaState, AsmHistory};
use crate::loss::F3dLossCode;

const MAX_HISTORY_TAIL_WORK: usize = 1 << 21;

#[derive(Debug)]
pub(crate) struct BodyHistoryTail {
    body: BodyId,
    previous: FeatureId,
    native: String,
    state: Option<i64>,
    start: TailStart,
    bounded: bool,
}

#[derive(Debug)]
enum TailStart {
    Unread,
    Known(i64),
    Repeated(i64),
}

/// Bodies whose changes after the last feature are retained or not checked.
#[derive(Debug, Default)]
pub(crate) struct BodyHistoryTails {
    pub(crate) tails: Vec<BodyHistoryTail>,
    /// Bodies no decoded history holds, with the last feature that outputs each.
    pub(crate) unread: Vec<(BodyId, FeatureId)>,
}

pub(crate) fn collect(
    features: &[Feature],
    histories: &[AsmHistory],
    placements: &[Option<PlacedState>],
    primary_brep: Option<&str>,
) -> BodyHistoryTails {
    let mut latest = BTreeMap::<&BodyId, (usize, &Feature)>::new();
    for (index, feature) in features
        .iter()
        .enumerate()
        .filter(|(_, feature)| feature.suppressed != Some(true))
    {
        for body in feature.evaluation.outputs() {
            if latest
                .get(body)
                .is_none_or(|(_, previous)| previous.ordinal < feature.ordinal)
            {
                latest.insert(body, (index, feature));
            }
        }
    }
    let mut tails = Vec::new();
    let mut unread = Vec::new();
    let mut state_maps = HashMap::new();
    let mut touch_cache = HashMap::new();
    for (body, (feature_index, feature)) in latest {
        let Some(history_id) = super::history_bodies::output_history(
            std::iter::once(body.as_str()),
            histories,
            primary_brep,
        ) else {
            unread.push((body.clone(), feature.id.clone()));
            continue;
        };
        let Some((history_index, history)) = histories
            .iter()
            .enumerate()
            .find(|(_, history)| history.id == history_id)
        else {
            unread.push((body.clone(), feature.id.clone()));
            continue;
        };
        let unproved = |native: String, start, state, bounded| BodyHistoryTail {
            body: body.clone(),
            previous: feature.id.clone(),
            native,
            state,
            start,
            bounded,
        };
        let Some(head) = history
            .states_oldest_first()
            .and_then(|order| order.last().copied())
        else {
            tails.push(unproved(history.id.clone(), TailStart::Unread, None, false));
            continue;
        };
        let head = &history.states()[head];
        let Some(placement) = placements
            .get(feature_index)
            .and_then(Option::as_ref)
            .filter(|p| p.history == history_index)
        else {
            tails.push(unproved(
                head.id.clone(),
                TailStart::Unread,
                Some(head.state_id),
                false,
            ));
            continue;
        };
        let current = placement.state_id;
        let states = state_maps
            .entry(history_index)
            .or_insert_with(|| super::history_states::history_state_index(history));
        if matches!(states.get(&current), Some(None)) {
            tails.push(unproved(
                head.id.clone(),
                TailStart::Repeated(current),
                Some(head.state_id),
                false,
            ));
            continue;
        }
        if current == head.state_id {
            continue;
        }
        let Some(slot) = super::body_closures::stable_ref(body.as_str()) else {
            tails.push(unproved(
                head.id.clone(),
                TailStart::Known(current),
                Some(head.state_id),
                false,
            ));
            continue;
        };
        let work = WorkBudget::new(MAX_HISTORY_TAIL_WORK);
        if changed_after_with_cache(
            history,
            histories,
            states,
            (current, head.state_id),
            slot,
            &work,
            &mut touch_cache,
        ) {
            tails.push(unproved(
                head.id.clone(),
                TailStart::Known(current),
                Some(head.state_id),
                work.exhausted(),
            ));
        }
    }
    BodyHistoryTails { tails, unread }
}

enum CachedStateTouch {
    NoChange,
    MissingTopology,
    Touched {
        slots: usize,
        bodies: Option<BTreeSet<i64>>,
    },
}

#[cfg(test)]
fn changed_after(
    history: &AsmHistory,
    histories: &[AsmHistory],
    previous: i64,
    current: i64,
    body: i64,
    work: &WorkBudget,
) -> bool {
    let states = super::history_states::history_state_index(history);
    let mut touch_cache = HashMap::new();
    changed_after_with_cache(
        history,
        histories,
        &states,
        (previous, current),
        body,
        work,
        &mut touch_cache,
    )
}

fn changed_after_with_cache(
    history: &AsmHistory,
    histories: &[AsmHistory],
    states: &HashMap<i64, Option<&AsmDeltaState>>,
    (previous, current): (i64, i64),
    body: i64,
    work: &WorkBudget,
    touch_cache: &mut HashMap<(*const AsmDeltaState, bool), CachedStateTouch>,
) -> bool {
    if !work.charge_by(work_units(history.states().len())) {
        return true;
    }
    let Some(chain) =
        super::output_chain::OutputChain::resolve(histories, current, previous, Some(&history.id))
    else {
        return true;
    };
    for id in chain.states {
        let Some(state) = states.get(&id).copied().flatten() else {
            return true;
        };
        let Some(transition) = state.transition.as_ref() else {
            return true;
        };
        if transition.geometry_unchanged {
            continue;
        }
        let Some(before) = transition
            .previous_state_id
            .and_then(|id| states.get(&id).copied().flatten())
        else {
            return true;
        };
        if attribute_only(history, before, state, work) {
            continue;
        }
        for (target_state, deleted) in [(state, false), (before, true)] {
            let cached = touch_cache
                .entry((std::ptr::from_ref(state), deleted))
                .or_insert_with(|| {
                    let changed = super::body_closures::changed_topology_family_refs(
                        &transition.topology,
                        deleted,
                    );
                    if changed.is_empty() {
                        return CachedStateTouch::NoChange;
                    }
                    let Some(topology) = history.topology(target_state) else {
                        return CachedStateTouch::MissingTopology;
                    };
                    let slots = [
                        &topology.bodies,
                        &topology.regions,
                        &topology.shells,
                        &topology.faces,
                        &topology.loops,
                        &topology.coedges,
                        &topology.edges,
                        &topology.vertices,
                        &topology.points,
                        &topology.surfaces,
                        &topology.curves,
                        &topology.pcurves,
                    ]
                    .iter()
                    .map(|family| family.len())
                    .sum();
                    let bodies = super::body_closures::bodies_touching(topology, &[&changed])
                        .and_then(|touched| touched.into_iter().next());
                    CachedStateTouch::Touched { slots, bodies }
                });
            match cached {
                CachedStateTouch::NoChange => {}
                CachedStateTouch::MissingTopology => return true,
                CachedStateTouch::Touched { slots, bodies } => {
                    if !work.charge_by(work_units(*slots)) {
                        return true;
                    }
                    let Some(bodies) = bodies else {
                        return true;
                    };
                    if bodies.contains(&body) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn attribute_only(
    history: &AsmHistory,
    before: &crate::history_records::AsmDeltaState,
    after: &crate::history_records::AsmDeltaState,
    work: &WorkBudget,
) -> bool {
    let Some(transition) = &after.transition else {
        return false;
    };
    let mut topology = transition.topology.clone();
    topology.bodies.updated.clear();
    if topology != crate::history_records::AsmHistoricalTopologyDelta::default() {
        return false;
    }
    let Some(cached) = history.cached_record_archive() else {
        return false;
    };
    if !work.charge_by(work_units(
        transition.records.updated.len()
            + transition.records.inserted.len()
            + transition.records.deleted.len(),
    )) {
        return false;
    }
    let Some(order) = history.states_oldest_first() else {
        return false;
    };
    let mut versions = HashMap::new();
    let mut old = None;
    let mut new = None;
    for index in order {
        let state = &history.states()[index];
        let Some(delta) = &state.transition else {
            return false;
        };
        if !work.charge_by(work_units(
            delta.records.deleted.len() + delta.record_revisions.len(),
        )) {
            return false;
        }
        for slot in &delta.records.deleted {
            versions.remove(slot);
        }
        for row in &delta.record_revisions {
            versions.insert(row.entity_ref, row.record_ref);
        }
        if state.state_id == before.state_id {
            if !work.charge_by(work_units(versions.len())) {
                return false;
            }
            old = Some(versions.clone());
        }
        if state.state_id == after.state_id {
            new = Some(versions);
            break;
        }
    }
    let (Some(old), Some(new)) = (old, new) else {
        return false;
    };
    let record = |versions: &HashMap<i64, i64>, slot| {
        versions
            .get(&slot)
            .and_then(|revision| cached.archive.get(revision))
    };
    for slot in &transition.records.updated {
        let (Some(left), Some(right)) = (record(&old, *slot), record(&new, *slot)) else {
            return false;
        };
        if transition.topology.bodies.updated.contains(slot) {
            if !same_body_shape(left, right) {
                return false;
            }
        } else if !left.name.ends_with("-attrib") || !right.name.ends_with("-attrib") {
            return false;
        }
    }
    transition
        .records
        .deleted
        .iter()
        .all(|slot| record(&old, *slot).is_some_and(|record| record.name.ends_with("-attrib")))
        && transition
            .records
            .inserted
            .iter()
            .all(|slot| record(&new, *slot).is_some_and(|record| record.name.ends_with("-attrib")))
}

fn same_body_shape(left: &cadmpeg_asm::sab::Record, right: &cadmpeg_asm::sab::Record) -> bool {
    use cadmpeg_asm::sab::Token;
    let fields = |record: &cadmpeg_asm::sab::Record| {
        if record.name != "body" || record.chunk_len() != 6 {
            return None;
        }
        match (
            record.chunk(0),
            record.chunk(1),
            record.chunk(2),
            record.chunk(3),
            record.chunk(4),
            record.chunk(5),
        ) {
            (
                Some(Token::Ref(_)),
                Some(Token::Long(key)),
                Some(Token::Ref(pattern)),
                Some(Token::Ref(lump)),
                Some(Token::Ref(wire)),
                Some(Token::Ref(transform)),
            ) => Some((*key, *pattern, *lump, *wire, *transform)),
            _ => None,
        }
    };
    fields(left).is_some_and(|left| Some(left) == fields(right))
}
pub(crate) fn append(
    model: &mut Model,
    found: &BodyHistoryTails,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    for (body, feature) in &found.unread {
        losses.push(F3dLossCode::FeatureBodyHistoryUnread.note(format!(
            "No single decoded history holds body {body}; changes after feature {feature} are not checked."
        )));
    }
    let tails = &found.tails;
    let mut ordinal = model
        .features
        .iter()
        .map(|feature| feature.ordinal)
        .max()
        .unwrap_or(0);
    for tail in tails {
        ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| CodecError::malformed("body history ordinal"))?;
        let start = match tail.start {
            TailStart::Unread => "an unread state".into(),
            TailStart::Known(state) => format!("state {state}"),
            TailStart::Repeated(state) => format!("an ambiguous occurrence of state {state}"),
        };
        let end = tail
            .state
            .map_or_else(|| "an unread state".into(), |state| state.to_string());
        let mut owners = model
            .product_definitions
            .iter()
            .filter(|definition| definition.bodies.contains(&tail.body));
        let (owner, ownership) = match (owners.next(), owners.next()) {
            (Some(definition), None) => (
                Some(definition.id.clone()),
                format!(
                    " Product definition {} holds the body and owns the native history interval.",
                    definition.id
                ),
            ),
            (first, _) => {
                let gap = if first.is_some() {
                    "several owners"
                } else {
                    "no owner"
                };
                losses.push(F3dLossCode::FeatureBodyHistoryOwnerUnresolved.note(format!(
                    "Body {} has {gap} in product definitions; the native history interval from {start} to {end} remains unowned and retains the imported final shape.",
                    tail.body,
                )));
                (None, String::new())
            }
        };
        let id = crate::ids::body_history_tail_feature_id(&tail.body);
        model.features.push(Feature {
            id,
            ordinal,
            product_definition: owner,
            name: Some("Unprojected body history".into()),
            suppressed: Some(false),
            dependencies: DistinctMembers::try_from(vec![tail.previous.clone()])
                .map_err(CodecError::from)?,
            source_properties: BTreeMap::new(),
            source_tag: Some("Body history change".into()),
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::new(
                FeatureDefinition::Operation(FeatureOperation::Native {
                    kind: "Body history change".into(),
                    parameters: BTreeMap::new(),
                }),
                DistinctMembers::try_from(vec![tail.body.clone()]).map_err(CodecError::from)?,
            ),
            native_ref: Some(tail.native.clone()),
        });
        let bound = if tail.bounded {
            " MAX_HISTORY_TAIL_WORK bounds the interval proof."
        } else {
            ""
        };
        losses.push(F3dLossCode::FeatureBodyHistoryIncomplete.note(format!(
            "Body {} retains the native history interval from {start} to {end} after feature {}.{ownership}{bound}",
            tail.body,
            tail.previous,
        )));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
