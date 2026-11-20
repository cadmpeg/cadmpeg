// SPDX-License-Identifier: Apache-2.0
//! Bind body-recipe operands to preceding history bodies.

use super::body_index::CompleteBodyIndex;
use super::{
    active_brep_face_matches_source, bodies_touching, effective_scope_previous_history_state_id,
    face_changes_across_state_chain, historical_brep_source, history_state_index,
    history_state_indexes, projection_was_finalized, selection, stable_ref,
    unique_history_state_pair, visit_state_groups_oldest_first, BindingHistories,
};
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology, AsmHistory};
use std::collections::{BTreeSet, HashMap};

pub(crate) fn bind_body_recipe_operand_history_candidates(
    operands: &mut [crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    recipes: &[crate::records::recipes::ConstructionRecipe],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    mut histories: BindingHistories<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    if projection_was_finalized(histories.get()) {
        return Ok(());
    }
    let mut recipes_by_id =
        HashMap::<_, Option<&crate::records::recipes::ConstructionRecipe>>::new();
    for recipe in recipes {
        recipes_by_id
            .entry(recipe.id.as_str())
            .and_modify(|recipe| *recipe = None)
            .or_insert(Some(recipe));
    }
    let mut jobs = Vec::new();
    for (position, operand) in operands.iter_mut().enumerate() {
        for reference in operand.reference_bindings_mut() {
            reference.preceding_candidate_faces.clear();
            reference.preceding_body_slots.clear();
        }
        operand.resolved_face_slot = None;
        operand.resolved_body_state_id = None;
        operand.resolved_body_slot = None;
        operand.resolved_body_face_slots.clear();
        if let Some((history, state, previous)) =
            body_recipe_operand_state_indexes(operand, scopes, histories.get())
        {
            jobs.push((history, previous, (position, state)));
        }
    }
    visit_state_groups_oldest_first(
        histories.reborrow(),
        jobs,
        |histories, history_index, previous_index, group| {
            let Some(history) = histories.get(history_index) else {
                return;
            };
            let Some(previous) = history.states().get(previous_index) else {
                return;
            };
            let Some(topology) = history.topology(previous) else {
                return;
            };
            let Some(source) = historical_brep_source(&previous.id) else {
                return;
            };
            let states = history_state_index(history);
            let mut bound = Vec::new();
            for (position, state_index) in group {
                let (Some(operand), Some(state)) = (
                    operands.get_mut(position),
                    history.states().get(state_index),
                ) else {
                    continue;
                };
                if face_changes_across_state_chain(state, previous.state_id, &states).is_none() {
                    continue;
                }
                bind_body_recipe_operand_preceding_faces(operand, topology, source);
                bound.push(position);
            }
            let face_slots = bound
                .iter()
                .filter_map(|&position| operands.get(position))
                .flat_map(
                    crate::records::topology::body_recipe::DesignBodyRecipeOperand::references,
                )
                .map(|reference| {
                    reference
                        .preceding_candidate_faces
                        .iter()
                        .filter_map(|face| stable_ref(face.as_str()))
                        .collect::<BTreeSet<_>>()
                })
                .collect::<Vec<_>>();
            let mut body_slots = if face_slots.is_empty() {
                None
            } else {
                bodies_touching(topology, &face_slots.iter().collect::<Vec<_>>())
            }
            .map(Vec::into_iter);
            let mut complete_bodies = None;
            for position in bound {
                let Some(operand) = operands.get_mut(position) else {
                    continue;
                };
                for reference in operand.reference_bindings_mut() {
                    if let Some(bodies) = body_slots.as_mut().and_then(Iterator::next) {
                        *reference.preceding_body_slots = bodies.into_iter().collect();
                    }
                }
                resolve_body_recipe_operand_preceding_body(operand);
                // When every face clause reaches a body of this state, the
                // body whose last persistent design link is the recipe's
                // design pair is the selected body.
                let clauses_reach_state = !operand.references().is_empty()
                    && operand
                        .references()
                        .iter()
                        .all(|reference| !reference.preceding_body_slots.is_empty());
                if let Some(body) = clauses_reach_state
                    .then(|| design_linked_body_slot(operand, &recipes_by_id, topology))
                    .flatten()
                {
                    operand.resolved_body_slot = Some(body);
                }
                // A body operand of this B-rep selects the only body of a state
                // that holds one body.
                if operand.resolved_body_slot.is_none() {
                    if let [body] = topology.bodies.as_slice() {
                        let in_source = operand.references().iter().any(|reference| {
                            reference
                                .candidate_faces
                                .iter()
                                .any(|face| active_brep_face_matches_source(face, source))
                        });
                        if in_source {
                            operand.resolved_body_slot = Some(*body);
                        }
                    }
                }
                let Some(body_slot) = operand.resolved_body_slot else {
                    continue;
                };
                if let Some(faces) = complete_bodies
                    .get_or_insert_with(|| CompleteBodyIndex::new(topology))
                    .faces(body_slot)
                {
                    operand.resolved_body_state_id = Some(previous.state_id);
                    operand.resolved_body_face_slots = faces;
                }
            }
        },
    );
    let identity = |operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand| -> Result<Option<_>, cadmpeg_core::CodecError> {
        let Some(stream) = crate::ids::native_stream(&operand.id) else { return Ok(None) };
        let Some(recipe) = recipes_by_id
            .get(operand.recipe_id.as_str())
            .and_then(|recipe| *recipe) else { return Ok(None) };
        let Some(design) = recipe.design.as_ref() else { return Ok(None) };
        let Some(selector) = design.selector else { return Ok(None) };
        Ok(Some((
            stream.to_owned(),
            operand.asset_id.clone(),
            operand.context_id.clone(),
            operand
                .references()
                .iter()
                .map(|reference| (reference.design_reference, reference.form)).collect::<Vec<_>>(),
            design.id.value.clone(),
            selector.value,
        )))
    };
    let mut resolved_by_identity = HashMap::new();
    for operand in operands.iter() {
        let (Some(identity), Some(body)) = (identity(operand)?, operand.resolved_body_slot) else {
            continue;
        };
        resolved_by_identity
            .entry(identity)
            .and_modify(|resolved| {
                if *resolved != Some(body) {
                    *resolved = None;
                }
            })
            .or_insert(Some(body));
    }
    let mut jobs = Vec::new();
    for (position, operand) in operands.iter_mut().enumerate() {
        if operand.resolved_body_slot.is_some() {
            continue;
        }
        operand.resolved_body_slot = identity(operand)?
            .and_then(|identity| resolved_by_identity.get(&identity).copied())
            .flatten();
        let Some(body_slot) = operand.resolved_body_slot else {
            continue;
        };
        if let Some((history, _, previous)) =
            body_recipe_operand_state_indexes(operand, scopes, histories.get())
        {
            jobs.push((history, previous, (position, body_slot)));
        }
    }
    visit_state_groups_oldest_first(
        histories,
        jobs,
        |histories, history_index, previous_index, group| {
            let Some(history) = histories.get(history_index) else {
                return;
            };
            let Some(previous) = history.states().get(previous_index) else {
                return;
            };
            let Some(topology) = history.topology(previous) else {
                return;
            };
            let complete_bodies = CompleteBodyIndex::new(topology);
            for (position, body_slot) in group {
                let (Some(operand), Some(faces)) =
                    (operands.get_mut(position), complete_bodies.faces(body_slot))
                else {
                    continue;
                };
                operand.resolved_body_state_id = Some(previous.state_id);
                operand.resolved_body_face_slots = faces;
            }
        },
    );
    Ok(())
}

/// The one body of `topology` whose last persistent design link equals the
/// design id and selector of the operand's body recipe.
fn design_linked_body_slot(
    operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand,
    recipes_by_id: &HashMap<&str, Option<&crate::records::recipes::ConstructionRecipe>>,
    topology: &AsmHistoricalTopology,
) -> Option<i64> {
    let recipe = recipes_by_id
        .get(operand.recipe_id.as_str())
        .copied()
        .flatten()?;
    if recipe.kind != crate::records::recipes::ConstructionRecipeKind::Body {
        return None;
    }
    let design = recipe.design.as_ref()?;
    let selector = i64::from(design.selector?.value);
    let mut carriers = topology.body_design_links.iter().filter(|link| {
        link.design_id == design.id.value
            && link.design_reference == selector
            && topology.bodies.contains(&link.body)
    });
    let body = carriers.next()?.body;
    carriers.next().is_none().then_some(body)
}

/// Bind each reference's candidate faces present in the previous-state
/// topology.
fn bind_body_recipe_operand_preceding_faces(
    operand: &mut crate::records::topology::body_recipe::DesignBodyRecipeOperand,
    topology: &AsmHistoricalTopology,
    source: &str,
) {
    for reference in operand.reference_bindings_mut() {
        *reference.preceding_candidate_faces = selection::faces_in_topology(
            reference
                .candidate_faces
                .iter()
                .filter(|face| active_brep_face_matches_source(face, source)),
            topology,
        );
    }
}

/// Resolve the operand's unique face and body from its references'
/// preceding faces and bodies, when they exist.
fn resolve_body_recipe_operand_preceding_body(
    operand: &mut crate::records::topology::body_recipe::DesignBodyRecipeOperand,
) {
    if let [reference] = operand.references().as_slice() {
        if let [face] = reference.preceding_candidate_faces.as_slice() {
            operand.resolved_face_slot = stable_ref(face.as_str());
        }
    }
    let Some(first) = operand.references().first() else {
        return;
    };
    if first.preceding_body_slots.is_empty()
        || operand
            .references()
            .iter()
            .any(|reference| reference.preceding_body_slots.is_empty())
    {
        return;
    }
    let mut intersection = BTreeSet::new();
    for body in &first.preceding_body_slots {
        intersection.insert(*body);
    }
    for reference in &operand.references()[1..] {
        intersection.retain(|body| reference.preceding_body_slots.contains(body));
    }
    if intersection.len() == 1 {
        operand.resolved_body_slot = intersection.into_iter().next();
    }
}

/// The state pair of a body recipe operand, and whether the BREP of its
/// candidate faces selects the history among several that hold the pair.
pub(super) fn body_recipe_operand_history_pair<'a>(
    operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &'a [AsmHistory],
) -> Option<((&'a AsmHistory, &'a AsmDeltaState, &'a AsmDeltaState), bool)> {
    let stream = crate::ids::native_stream(&operand.id)?;
    let mut matching_scopes = scopes.iter().filter(|scope| {
        scope.record_index == operand.scope_record_index
            && crate::ids::native_stream(&scope.id) == Some(stream)
    });
    let scope = matching_scopes.next()?;
    if matching_scopes.next().is_some() {
        return None;
    }
    let state_id = scope.history_state_id()?;
    let previous_state_id = effective_scope_previous_history_state_id(scope, histories)?;
    if let Some(pair) = unique_history_state_pair(histories, state_id, previous_state_id) {
        return Some((pair, false));
    }
    super::linked_states::operand_brep_history_state_pair(
        operand,
        histories,
        state_id,
        previous_state_id,
    )
    .map(|pair| (pair, true))
}

/// History, state and previous-state indexes of a body recipe operand.
fn body_recipe_operand_state_indexes(
    operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
) -> Option<(usize, usize, usize)> {
    let ((history, state, previous), _) =
        body_recipe_operand_history_pair(operand, scopes, histories)?;
    let (history_index, state_index) = history_state_indexes(histories, history, state)?;
    let (_, previous_index) = history_state_indexes(histories, history, previous)?;
    Some((history_index, state_index, previous_index))
}
