// SPDX-License-Identifier: Apache-2.0
//! Scope history binding.

use super::face_resolution::{active_brep_face_matches_source, historical_brep_source};
use super::history_states::{
    history_state_pair, insert_scope_history_binding, unique_history_state_in,
};
use crate::history_records::AsmHistory;
use std::collections::{BTreeMap, HashMap, HashSet};

pub(crate) fn bind_scope_histories(
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    body_bindings: &[crate::records::bodies::DesignBodyBinding],
    body_recipe_operands: &[crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    edge_recipes: super::edge_recipe_scope::Sources<'_>,
    histories: &[AsmHistory],
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> HashMap<String, String> {
    let mut candidates = Vec::new();
    for scope in scopes {
        let Some(state_id) = scope.history_state_id() else {
            continue;
        };
        let scope_candidates = if let Some(previous_state_id) = scope.previous_history_state_id() {
            let direct = histories
                .iter()
                .filter(|history| {
                    history_state_pair(history, state_id, previous_state_id, true).is_some()
                })
                .collect::<Vec<_>>();
            if direct.is_empty() {
                histories
                    .iter()
                    .filter(|history| {
                        history_state_pair(history, state_id, previous_state_id, false).is_some()
                    })
                    .collect::<Vec<_>>()
            } else {
                direct
            }
        } else {
            histories
                .iter()
                .filter(|history| unique_history_state_in(history, state_id))
                .collect::<Vec<_>>()
        };
        if !scope_candidates.is_empty() {
            candidates.push((scope, scope_candidates));
        }
    }
    let mut resolved = HashMap::<String, String>::new();
    let mut inferred_scopes = HashSet::<String>::new();
    for (scope, candidates) in &candidates {
        if candidates.len() == 1 {
            insert_scope_history_binding(&mut resolved, &scope.id, &candidates[0].id);
            continue;
        }
        let next_scope_record_index = scopes
            .iter()
            .filter(|candidate| {
                crate::ids::same_native_occurrence(&candidate.id, &scope.id)
                    && candidate.record_index > scope.record_index
            })
            .map(|candidate| candidate.record_index)
            .min();
        let mut output_bindings = body_bindings.iter().filter(|binding| {
            crate::ids::same_native_occurrence(binding.id(), &scope.id)
                && binding.entity_suffix > u64::from(scope.record_index)
                && next_scope_record_index
                    .is_none_or(|next| binding.entity_suffix < u64::from(next))
        });
        if let Some(binding) = output_bindings.next() {
            if output_bindings.next().is_none() {
                let mut matching = candidates.iter().filter(|history| {
                    historical_brep_source(&history.id).is_some_and(|source| {
                        binding.blob_name().strip_prefix("BREP.") == Some(source)
                    })
                });
                if let Some(history) = matching.next().filter(|_| matching.next().is_none()) {
                    insert_scope_history_binding(&mut resolved, &scope.id, &history.id);
                    continue;
                }
            }
        }
        let candidate_faces = body_recipe_operands
            .iter()
            .filter(|operand| {
                crate::ids::same_native_occurrence(&operand.id, &scope.id)
                    && operand.scope_record_index == scope.record_index
            })
            .flat_map(crate::records::topology::body_recipe::DesignBodyRecipeOperand::references)
            .flat_map(|reference| &reference.candidate_faces);
        if candidate_faces.clone().next().is_some() {
            let mut matching = candidates.iter().filter(|history| {
                historical_brep_source(&history.id).is_some_and(|source| {
                    candidate_faces
                        .clone()
                        .any(|face| active_brep_face_matches_source(face, source))
                })
            });
            if let Some(history) = matching.next().filter(|_| matching.next().is_none()) {
                insert_scope_history_binding(&mut resolved, &scope.id, &history.id);
                continue;
            }
        }
        if let Some(history) = super::edge_recipe_scope::select(scope, edge_recipes, candidates) {
            insert_scope_history_binding(&mut resolved, &scope.id, &history.id);
            inferred_scopes.insert(scope.id.clone());
            losses.push(crate::loss::F3dLossCode::HistoricalEdgeRecipeHistoryInferred.note(format!(
                "scope {}: its state pair occurs in several histories; it reads {}, whose BREP blob agrees with every qualified edge-recipe face candidate.",
                scope.id, history.id
            )));
            continue;
        }
        let Some(construction) = scope.base_feature_construction() else {
            continue;
        };
        let Some(referenced_histories) = construction
            .body_entity_suffixes()
            .map(|suffix| {
                let mut bindings = body_bindings.iter().filter(|binding| {
                    crate::ids::same_native_occurrence(binding.id(), &scope.id)
                        && binding.entity_suffix == suffix
                });
                let binding = bindings.next()?;
                if bindings.next().is_some() {
                    return None;
                }
                let mut matching = candidates.iter().filter(|history| {
                    historical_brep_source(&history.id).is_some_and(|source| {
                        binding.blob_name().strip_prefix("BREP.") == Some(source)
                    })
                });
                let history = matching.next()?;
                matching.next().is_none().then_some(history.id.as_str())
            })
            .collect::<Option<Vec<&str>>>()
        else {
            continue;
        };
        let Some(&history_id) = referenced_histories.first() else {
            continue;
        };
        if referenced_histories
            .iter()
            .all(|&candidate| candidate == history_id)
        {
            insert_scope_history_binding(&mut resolved, &scope.id, history_id);
        }
    }
    let mut groups = BTreeMap::<(&str, i64, Option<i64>), Vec<usize>>::new();
    for (index, (scope, _)) in candidates.iter().enumerate() {
        let (Some(stream), Some(state_id)) = (
            crate::ids::native_stream(&scope.id),
            scope.history_state_id(),
        ) else {
            continue;
        };
        let key = (stream, state_id, scope.previous_history_state_id());
        groups.entry(key).or_default().push(index);
    }
    for members in groups.values() {
        let mut candidate_histories = HashSet::new();
        for index in members {
            for history in &candidates[*index].1 {
                candidate_histories.insert(history.id.as_str());
            }
        }
        if candidate_histories.len() != members.len() {
            continue;
        }
        loop {
            let mut assigned = HashSet::new();
            for index in members {
                if let Some(history_id) = resolved.get(&candidates[*index].0.id) {
                    let copy = history_id.to_owned();
                    assigned.insert(copy);
                }
            }
            if assigned.len()
                != members
                    .iter()
                    .filter(|index| resolved.contains_key(&candidates[**index].0.id))
                    .count()
            {
                break;
            }
            let mut progress = false;
            for index in members {
                let (scope, scope_candidates) = &candidates[*index];
                if resolved.contains_key(&scope.id) {
                    continue;
                }
                let mut remaining = scope_candidates
                    .iter()
                    .filter(|history| !assigned.contains(history.id.as_str()));
                if let Some(history) = remaining.next().filter(|_| remaining.next().is_none()) {
                    insert_scope_history_binding(&mut resolved, &scope.id, &history.id);
                    let rests_on_inference = members.iter().any(|other| {
                        let other = &candidates[*other].0.id;
                        other != &scope.id && inferred_scopes.contains(other)
                    });
                    if rests_on_inference {
                        inferred_scopes.insert(scope.id.clone());
                        losses.push(crate::loss::F3dLossCode::HistoricalEdgeRecipeHistoryInferred.note(format!(
                            "scope {}: it reads {}, the only candidate history that no other scope with its state pair holds, and one of those scopes reads its history through edge-recipe face candidates.",
                            scope.id, history.id
                        )));
                    }
                    progress = true;
                }
            }
            if !progress {
                break;
            }
        }
    }
    resolved
}
