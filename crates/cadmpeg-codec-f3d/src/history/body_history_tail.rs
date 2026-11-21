// SPDX-License-Identifier: Apache-2.0
//! Retain body changes after the last feature with a bound result.

use std::collections::{BTreeMap, HashMap};

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

use crate::history_records::AsmHistory;
use crate::loss::F3dLossCode;
use crate::records::feature::scope::DesignParameterScope;

const MAX_HISTORY_TAIL_WORK: usize = 1 << 21;

#[derive(Debug)]
pub(crate) struct BodyHistoryTail {
    body: BodyId,
    previous: FeatureId,
    native: String,
    state: i64,
    previous_state: i64,
    bounded: bool,
}

pub(crate) fn collect(
    features: &[Feature],
    scopes: &[DesignParameterScope],
    histories: &[AsmHistory],
    primary_brep: Option<&str>,
) -> Vec<BodyHistoryTail> {
    let mut latest = BTreeMap::<&BodyId, &Feature>::new();
    for feature in features
        .iter()
        .filter(|feature| feature.suppressed != Some(true))
    {
        for body in feature.evaluation.outputs() {
            if latest
                .get(body)
                .is_none_or(|previous| previous.ordinal < feature.ordinal)
            {
                latest.insert(body, feature);
            }
        }
    }
    let mut tails = Vec::new();
    for (body, feature) in latest {
        let Some(history_id) = super::history_bodies::output_history(
            std::iter::once(body.as_str()),
            histories,
            primary_brep,
        ) else {
            continue;
        };
        let Some(history) = histories.iter().find(|history| history.id == history_id) else {
            continue;
        };
        let Some((current, _)) = super::feature_history_states(feature, scopes, histories) else {
            continue;
        };
        let Some(order) = history.states_oldest_first() else {
            continue;
        };
        let Some(&head) = order.last() else {
            continue;
        };
        let head = &history.states()[head];
        if current == head.state_id {
            continue;
        }
        let Some(slot) = super::stable_ref(body.as_str()) else {
            continue;
        };
        let work = WorkBudget::new(MAX_HISTORY_TAIL_WORK);
        if changed_after(history, histories, current, head.state_id, slot, &work) {
            tails.push(BodyHistoryTail {
                body: body.clone(),
                previous: feature.id.clone(),
                native: head.id.clone(),
                state: head.state_id,
                previous_state: current,
                bounded: work.exhausted(),
            });
        }
    }
    tails
}

fn changed_after(
    history: &AsmHistory,
    histories: &[AsmHistory],
    previous: i64,
    current: i64,
    body: i64,
    work: &WorkBudget,
) -> bool {
    if !work.charge_by(work_units(history.states().len())) {
        return true;
    }
    let Some(chain) =
        super::output_chain::OutputChain::resolve(histories, current, previous, Some(&history.id))
    else {
        return true;
    };
    let states = history
        .states()
        .iter()
        .map(|state| (state.state_id, state))
        .collect::<HashMap<_, _>>();
    for id in chain.states {
        let Some(&state) = states.get(&id) else {
            return true;
        };
        let Some(transition) = state.transition.as_ref() else {
            return true;
        };
        if transition.geometry_unchanged {
            continue;
        }
        let Some(&before) = transition.previous_state_id.and_then(|id| states.get(&id)) else {
            return true;
        };
        let changes = [
            (
                state,
                super::changed_topology_family_refs(&transition.topology, false),
            ),
            (
                before,
                super::changed_topology_family_refs(&transition.topology, true),
            ),
        ];
        for (state, changed) in changes {
            if changed.is_empty() {
                continue;
            }
            let Some(topology) = history.topology(state) else {
                return true;
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
            if !work.charge_by(work_units(slots)) {
                return true;
            }
            let Some(touched) = super::bodies_touching(topology, &[&changed]) else {
                return true;
            };
            if touched.iter().any(|bodies| bodies.contains(&body)) {
                return true;
            }
        }
    }
    false
}

pub(crate) fn append(
    model: &mut Model,
    tails: &[BodyHistoryTail],
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
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
                    "Body {} has {gap} in product definitions; the native history interval from state {} to {} remains unowned and retains the imported final shape.",
                    tail.body, tail.previous_state, tail.state,
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
            "Body {} retains the native history interval from state {} to {} after feature {}.{ownership}{bound}",
            tail.body, tail.previous_state, tail.state, tail.previous,
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
