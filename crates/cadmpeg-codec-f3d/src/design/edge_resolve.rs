// SPDX-License-Identifier: Apache-2.0
//! Resolve edge-selection operands to stable edge identities.

use crate::ids::{self, native_stream, neutral_feature_id};
use crate::records::{
    DesignConstructionOperandGroup, DesignEdgeIdentityOperand, DesignEdgeOperand,
    DesignParameterScope,
};
use std::collections::{HashMap, HashSet};

pub(crate) fn resolved_edge_group(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    treatment_radius: Option<f64>,
) -> cadmpeg_ir::features::EdgeSelection {
    use cadmpeg_ir::features::EdgeSelection;

    let feature_key = feature_id
        .0
        .split_once('#')
        .map_or(feature_id.0.as_str(), |(_, key)| key);
    let unmatched_selection = |state_id: Option<i64>| {
        if group.lost_edge_references.is_empty() {
            EdgeSelection::Native(group.id.clone())
        } else {
            state_id
                .and_then(|state_id| {
                    partial_historical_edge_selection(
                        group
                            .lost_edge_references
                            .iter()
                            .map(|identity| (identity.as_str(), None)),
                        state_id,
                        feature_key,
                        feature_input_topology_id(feature_id, state_id),
                        &group.id,
                    )
                })
                .unwrap_or(EdgeSelection::Unresolved)
        }
    };
    let stream = native_stream(&group.id);
    let identity_matches = group
        .members
        .iter()
        .map(|member| {
            let mut matches = identity_operands.iter().filter(|operand| {
                native_stream(&operand.id) == stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index == group.record_index
                    && operand.record_index == *member
            });
            let operand = matches.next()?;
            matches.next().is_none().then_some(operand)
        })
        .collect::<Option<Vec<_>>>();
    let has_recipe_operands = group.members.iter().all(|member| {
        let matches = operands
            .iter()
            .filter(|operand| {
                native_stream(&operand.id) == stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.record_index == *member
            })
            .collect::<Vec<_>>();
        matches.len() == 1
    });
    let has_concrete_recipe_evidence = group.members.iter().any(|member| {
        let matches = operands
            .iter()
            .filter(|operand| {
                native_stream(&operand.id) == stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.record_index == *member
            })
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [operand] => {
                operand.resolved_edge_slot.is_some()
                    || !operand.changed_boundary_edge_slots.is_empty()
                    || !operand.deleted_boundary_edge_slots.is_empty()
                    || !operand.treatment_radius_candidates.is_empty()
            }
            _ => false,
        }
    });
    let identity_transition_slots = identity_matches.as_ref().and_then(|operands| {
        let [operand] = operands.as_slice() else {
            return None;
        };
        let mut edges = operand.transition_edge_candidates.clone();
        edges.sort_unstable();
        edges.dedup();
        (!edges.is_empty()).then_some(edges)
    });
    let has_complete_identity_selection = identity_matches.as_ref().is_some_and(|operands| {
        !operands.is_empty()
            && (operands.iter().all(|operand| {
                operand.resolved_edge_slot.is_some() || !operand.resolved_edge_slots.is_empty()
            }) || identity_transition_slots.is_some())
    });
    let recipe_corroborates_identity_transition =
        identity_transition_slots.as_ref().is_some_and(|slots| {
            group.members.len() == 1
                && operands
                    .iter()
                    .filter(|operand| {
                        native_stream(&operand.id) == stream
                            && operand.scope_record_index == group.scope_record_index
                            && operand.record_index == group.members[0]
                    })
                    .all(|operand| {
                        slots.iter().all(|slot| {
                            operand.changed_boundary_edge_slots.contains(slot)
                                || operand.deleted_boundary_edge_slots.contains(slot)
                        })
                    })
        });
    if let Some(identity_matches) = identity_matches.as_ref().filter(|_| {
        !has_recipe_operands
            || (has_complete_identity_selection
                && (!has_concrete_recipe_evidence || recipe_corroborates_identity_transition))
    }) {
        if identity_matches.is_empty() {
            return unmatched_selection(previous_state_id);
        }
        let Some(previous_state_id) = previous_state_id else {
            return unmatched_selection(None);
        };
        let state = feature_input_topology_id(feature_id, previous_state_id);
        if identity_matches.iter().all(|operand| {
            operand.resolved_edge_slot.is_some() || !operand.resolved_edge_slots.is_empty()
        }) {
            let mut seen = HashSet::new();
            let edges = identity_matches
                .iter()
                .flat_map(|operand| {
                    operand
                        .resolved_edge_slot
                        .iter()
                        .copied()
                        .chain(operand.resolved_edge_slots.iter().copied())
                })
                .filter(|edge| seen.insert(*edge))
                .map(|edge_slot| {
                    ids::history_input_edge_id(
                        &ids::history_input_prefix(feature_key, previous_state_id),
                        edge_slot,
                    )
                })
                .collect();
            return EdgeSelection::Historical {
                state,
                edges,
                native: group.id.clone(),
            };
        }
        if identity_matches.len() == 1 && identity_matches[0].resolved_edge_slot.is_none() {
            if let Some(edges) = identity_transition_slots.as_ref() {
                return EdgeSelection::Historical {
                    state,
                    edges: edges
                        .iter()
                        .map(|edge_slot| {
                            ids::history_input_edge_id(
                                &ids::history_input_prefix(feature_key, previous_state_id),
                                edge_slot,
                            )
                        })
                        .collect(),
                    native: group.id.clone(),
                };
            }
        }
        let members = identity_matches
            .iter()
            .map(|operand| (operand.id.as_str(), operand.resolved_edge_slot))
            .collect::<Vec<_>>();
        if members.iter().all(|(_, edge)| edge.is_some()) {
            let edges = members
                .into_iter()
                .filter_map(|(_, edge)| edge)
                .map(|edge_slot| {
                    ids::history_input_edge_id(
                        &ids::history_input_prefix(feature_key, previous_state_id),
                        edge_slot,
                    )
                })
                .collect();
            return EdgeSelection::Historical {
                state,
                edges,
                native: group.id.clone(),
            };
        }
        return partial_historical_edge_selection(
            members,
            previous_state_id,
            feature_key,
            state,
            &group.id,
        )
        .unwrap_or_else(|| EdgeSelection::Native(group.id.clone()));
    }
    let mut matched_operands = Vec::with_capacity(group.members.len());
    let mut member_identities = HashSet::new();
    for member in &group.members {
        if !member_identities.insert(*member) {
            return unmatched_selection(previous_state_id);
        }
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == stream
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index == *member
        });
        let Some(operand) = matches.next() else {
            return unmatched_selection(previous_state_id);
        };
        if matches.next().is_some() {
            return unmatched_selection(previous_state_id);
        }
        matched_operands.push(operand);
    }
    let recipe_state_id = || {
        let mut states = matched_operands
            .iter()
            .filter_map(|operand| operand.recipe_state_id);
        let state = states.next()?;
        (states.all(|candidate| candidate == state)
            && matched_operands
                .iter()
                .all(|operand| operand.recipe_state_id == Some(state)))
        .then_some(state)
    };
    let transition_state_id = previous_state_id;
    let Some(previous_state_id) = transition_state_id.or_else(recipe_state_id) else {
        return if group.lost_edge_references.is_empty() {
            EdgeSelection::Native(group.id.clone())
        } else {
            EdgeSelection::Unresolved
        };
    };
    let state = feature_input_topology_id(feature_id, previous_state_id);
    let lost_selection = || unmatched_selection(Some(previous_state_id));
    let exact_slots = matched_operands
        .iter()
        .map(|operand| resolved_edge_operand(operand))
        .collect::<Option<Vec<_>>>()
        .or_else(|| unique_edge_group_assignment(&matched_operands));
    let transition_slots = || {
        treatment_radius
            .and_then(|radius| radius_edge_group_candidates(&matched_operands, radius))
            .or_else(|| {
                treatment_radius.and_then(|radius| {
                    identity_matches.as_ref().and_then(|operands| {
                        radius_edge_identity_group_candidates(operands, radius)
                    })
                })
            })
            .or_else(|| {
                context_only_edge_group_candidates(matched_operands.iter().map(|operand| {
                    (
                        resolved_edge_operand(operand),
                        operand.changed_boundary_edge_slots.as_slice(),
                    )
                }))
            })
            .or_else(|| {
                changed_boundary_count_edge_group_candidates(
                    matched_operands
                        .iter()
                        .map(|operand| operand.recipe_selectors.as_slice()),
                )
            })
            .or_else(|| {
                common_deleted_edge_group_candidates(matched_operands.iter().map(|operand| {
                    (
                        !operand.changed_boundary_edge_slots.is_empty(),
                        operand.deleted_boundary_edge_slots.as_slice(),
                    )
                }))
            })
            .or_else(|| scope_partition_edge_group_candidates(group, groups, operands))
    };
    let resolved_slots =
        exact_slots.or_else(|| transition_state_id.and_then(|_| transition_slots()));
    let Some(resolved_slots) = resolved_slots else {
        if !group.lost_edge_references.is_empty() {
            return lost_selection();
        }
        let combined_edges = matched_operands
            .iter()
            .enumerate()
            .map(|(index, operand)| {
                let recipe = resolved_edge_operand(operand);
                let identity = identity_matches
                    .as_ref()
                    .and_then(|identities| identities[index].resolved_edge_slot);
                match (recipe, identity) {
                    (Some(recipe), Some(identity)) if recipe != identity => None,
                    (recipe, identity) => Some(recipe.or(identity)),
                }
            })
            .collect::<Option<Vec<_>>>();
        let Some(combined_edges) = combined_edges else {
            return unmatched_selection(Some(previous_state_id));
        };
        if combined_edges.iter().all(Option::is_some) {
            let mut edges = Vec::new();
            for edge_slot in combined_edges.into_iter().flatten() {
                let edge = ids::history_input_edge_id(
                    &ids::history_input_prefix(feature_key, previous_state_id),
                    edge_slot,
                );
                if !edges.contains(&edge) {
                    edges.push(edge);
                }
            }
            return EdgeSelection::Historical {
                state,
                edges,
                native: group.id.clone(),
            };
        }
        let partial_members = matched_operands
            .iter()
            .zip(combined_edges)
            .filter_map(|(operand, resolved)| {
                let carries_transition_evidence = identity_matches.is_some()
                    || transition_state_id.is_none()
                    || !operand.changed_boundary_edge_slots.is_empty();
                (resolved.is_some() || carries_transition_evidence)
                    .then_some((operand.id.as_str(), resolved))
            })
            .collect::<Vec<_>>();
        return partial_historical_edge_selection(
            partial_members,
            previous_state_id,
            feature_key,
            state,
            &group.id,
        )
        .unwrap_or_else(|| EdgeSelection::Native(group.id.clone()));
    };
    let mut edges = Vec::new();
    for edge_slot in resolved_slots {
        let edge = ids::history_input_edge_id(
            &ids::history_input_prefix(feature_key, previous_state_id),
            edge_slot,
        );
        if !edges.contains(&edge) {
            edges.push(edge);
        }
    }
    if edges.is_empty() {
        EdgeSelection::Native(group.id.clone())
    } else {
        EdgeSelection::Historical {
            state,
            edges,
            native: group.id.clone(),
        }
    }
}

pub(crate) fn partial_historical_edge_selection<'a>(
    members: impl IntoIterator<Item = (&'a str, Option<i64>)>,
    previous_state_id: i64,
    feature_key: &str,
    state: cadmpeg_ir::ids::FeatureInputTopologyId,
    native: &str,
) -> Option<cadmpeg_ir::features::EdgeSelection> {
    use cadmpeg_ir::features::EdgeSelection;

    let mut edges = Vec::new();
    let mut unresolved = Vec::new();
    for (identity, edge) in members {
        if let Some(edge) = edge {
            if !edges.contains(&edge) {
                edges.push(edge);
            }
        } else {
            unresolved.push(identity.to_owned());
        }
    }
    if unresolved.is_empty() || edges.is_empty() {
        return None;
    }
    Some(EdgeSelection::HistoricalPartial {
        state,
        edges: edges
            .into_iter()
            .map(|edge_slot| {
                ids::history_input_edge_id(
                    &ids::history_input_prefix(feature_key, previous_state_id),
                    edge_slot,
                )
            })
            .collect(),
        unresolved,
        native: native.to_owned(),
    })
}

pub(crate) fn context_only_edge_group_candidates<'a>(
    members: impl IntoIterator<Item = (Option<i64>, &'a [i64])>,
) -> Option<Vec<i64>> {
    let mut edges = Vec::new();
    for (resolved, changed_candidates) in members {
        match resolved {
            Some(edge) => {
                if !edges.contains(&edge) {
                    edges.push(edge);
                }
            }
            None if changed_candidates.is_empty() => {}
            None => return None,
        }
    }
    (!edges.is_empty()).then_some(edges)
}

pub(crate) fn feature_input_topology_id(
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
) -> cadmpeg_ir::ids::FeatureInputTopologyId {
    let feature_key = feature_id
        .0
        .split_once('#')
        .map_or(feature_id.0.as_str(), |(_, key)| key);
    ids::history_input_state_id(&ids::history_input_prefix(feature_key, previous_state_id))
}

fn unique_edge_group_assignment(operands: &[&DesignEdgeOperand]) -> Option<Vec<i64>> {
    if operands.is_empty() {
        return None;
    }
    let candidate_sets = operands
        .iter()
        .map(|operand| {
            if let Some(edge) = resolved_edge_operand(operand) {
                Some(EdgeAssignmentCandidates::Edges(vec![edge]))
            } else {
                edge_group_assignment_candidates(
                    &operand.recipe_selectors,
                    edge_operand_reference_edge_sets(operand),
                )
            }
        })
        .collect::<Option<Vec<_>>>()?;
    unique_edge_assignment_with_context(&candidate_sets)
}

#[derive(Debug, PartialEq)]
pub(crate) enum EdgeAssignmentCandidates {
    Context,
    Edges(Vec<i64>),
}

// `None` means the record claims an edge operand but its proofs do not admit a
// candidate. `Context` means the recipe has no edge-assignment proof and the
// record only contributes topology context to its neighboring operands.
pub(crate) fn edge_group_assignment_candidates<'a>(
    selector_contexts: &[crate::records::DesignEdgeRecipeSelectorContext],
    reference_edge_sets: impl IntoIterator<Item = &'a [i64]>,
) -> Option<EdgeAssignmentCandidates> {
    let reference_edge_sets = reference_edge_sets.into_iter().collect::<Vec<_>>();
    if !selector_contexts.is_empty() {
        return edge_assignment_candidates(selector_contexts, reference_edge_sets)
            .map(EdgeAssignmentCandidates::Edges);
    }
    let [first, second, ..] = reference_edge_sets.as_slice() else {
        return Some(EdgeAssignmentCandidates::Context);
    };
    if first.is_empty() || second.is_empty() {
        return None;
    }
    let mut candidates = first.to_vec();
    candidates.retain(|candidate| second.contains(candidate));
    candidates.sort_unstable();
    candidates.dedup();
    (!candidates.is_empty()).then_some(EdgeAssignmentCandidates::Edges(candidates))
}

pub(crate) fn radius_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
    radius: f64,
) -> Option<Vec<i64>> {
    if operands.is_empty() || !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    let tolerance = 1.0e-9 * (1.0 + radius.abs());
    let mut chain = Vec::new();
    for operand in operands {
        if let Some(edge) = resolved_edge_operand(operand) {
            chain.push(edge);
        }
        chain.extend(
            operand
                .treatment_radius_candidates
                .iter()
                .filter(|candidate| (candidate.radius - radius).abs() <= tolerance)
                .map(|candidate| candidate.edge_slot),
        );
    }
    chain.sort_unstable();
    chain.dedup();
    if chain.is_empty() {
        return None;
    }
    for operand in operands {
        let has_radius_candidate = operand
            .treatment_radius_candidates
            .iter()
            .any(|candidate| (candidate.radius - radius).abs() <= tolerance);
        if resolved_edge_operand(operand).is_none()
            && !has_radius_candidate
            && !operand.changed_boundary_edge_slots.is_empty()
            && !operand
                .changed_boundary_edge_slots
                .iter()
                .any(|edge| chain.contains(edge))
        {
            return None;
        }
    }
    Some(chain)
}

fn radius_edge_identity_group_candidates(
    operands: &[&DesignEdgeIdentityOperand],
    radius: f64,
) -> Option<Vec<i64>> {
    if operands.is_empty() || !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    let tolerance = 1.0e-9 * (1.0 + radius.abs());
    let mut chain = operands
        .iter()
        .flat_map(|operand| {
            operand
                .resolved_edge_slot
                .iter()
                .copied()
                .chain(operand.resolved_edge_slots.iter().copied())
                .chain(
                    operand
                        .treatment_radius_candidates
                        .iter()
                        .filter(|candidate| (candidate.radius - radius).abs() <= tolerance)
                        .map(|candidate| candidate.edge_slot),
                )
        })
        .collect::<Vec<_>>();
    chain.sort_unstable();
    chain.dedup();
    if chain.is_empty() {
        return None;
    }
    operands
        .iter()
        .all(|operand| {
            operand.resolved_edge_slot.is_some()
                || !operand.resolved_edge_slots.is_empty()
                || operand
                    .treatment_radius_candidates
                    .iter()
                    .any(|candidate| (candidate.radius - radius).abs() <= tolerance)
                || operand.transition_edge_candidates.is_empty()
                || operand
                    .transition_edge_candidates
                    .iter()
                    .any(|edge| chain.contains(edge))
        })
        .then_some(chain)
}

pub(crate) fn unique_edge_assignment_with_context(
    candidate_sets: &[EdgeAssignmentCandidates],
) -> Option<Vec<i64>> {
    let edge_candidate_sets = candidate_sets
        .iter()
        .filter_map(|candidates| match candidates {
            EdgeAssignmentCandidates::Context => None,
            EdgeAssignmentCandidates::Edges(edges) => Some(edges.clone()),
        })
        .collect::<Vec<_>>();
    unique_bipartite_assignment(&edge_candidate_sets)
}

pub(crate) fn edge_assignment_candidates<'a>(
    selector_contexts: &[crate::records::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: impl IntoIterator<Item = &'a [i64]>,
) -> Option<Vec<i64>> {
    let shared_edge_sets = shared_edge_sets.into_iter().collect::<Vec<_>>();
    if !selector_contexts.is_empty()
        && selector_contexts
            .iter()
            .all(|selector| !selector.incidence_matching_edge_slots.is_empty())
    {
        corroborated_edge_candidates(selector_contexts, shared_edge_sets.iter().copied(), false)
    } else {
        corroborated_edge_candidates(selector_contexts, shared_edge_sets.iter().copied(), true)
    }
}

pub(crate) fn unique_bipartite_assignment(candidate_sets: &[Vec<i64>]) -> Option<Vec<i64>> {
    if candidate_sets.is_empty() {
        return None;
    }
    let mut normalized = candidate_sets.to_vec();
    for candidates in &mut normalized {
        candidates.sort_unstable();
        candidates.dedup();
        if candidates.is_empty() {
            return None;
        }
    }
    let assignment = bipartite_assignment(&normalized, None)?;
    for (member, edge) in assignment.iter().copied().enumerate() {
        if bipartite_assignment(&normalized, Some((member, edge))).is_some() {
            return None;
        }
    }
    Some(assignment)
}

fn bipartite_assignment(
    candidate_sets: &[Vec<i64>],
    forbidden: Option<(usize, i64)>,
) -> Option<Vec<i64>> {
    fn augment(
        member: usize,
        candidate_sets: &[Vec<i64>],
        forbidden: Option<(usize, i64)>,
        visited: &mut HashSet<i64>,
        edge_members: &mut HashMap<i64, usize>,
    ) -> bool {
        for edge in &candidate_sets[member] {
            if forbidden == Some((member, *edge)) || !visited.insert(*edge) {
                continue;
            }
            let displaced = edge_members.get(edge).copied();
            if displaced.is_none_or(|displaced| {
                augment(displaced, candidate_sets, forbidden, visited, edge_members)
            }) {
                edge_members.insert(*edge, member);
                return true;
            }
        }
        false
    }

    let mut edge_members = HashMap::new();
    for member in 0..candidate_sets.len() {
        if !augment(
            member,
            candidate_sets,
            forbidden,
            &mut HashSet::new(),
            &mut edge_members,
        ) {
            return None;
        }
    }
    let mut assignment = vec![0; candidate_sets.len()];
    for (edge, member) in edge_members {
        assignment[member] = edge;
    }
    Some(assignment)
}

/// Members of one construction operand group: `(identity, resolved edge slot,
/// deleted boundary edge slots)`.
type EdgeGroupMembers = Vec<(u32, Option<i64>, Vec<i64>)>;

fn scope_partition_edge_group_candidates(
    target: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
) -> Option<Vec<i64>> {
    let stream = native_stream(&target.id)?;
    let mut scope_groups = Vec::new();
    let mut target_ordinal = None;
    for group in groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream)
            && group.scope_record_index == target.scope_record_index
            && group.lost_edge_references.is_empty()
            && !group.members.is_empty()
    }) {
        let mut members = Vec::with_capacity(group.members.len());
        let mut complete = true;
        for member in &group.members {
            let matches = operands
                .iter()
                .filter(|operand| {
                    native_stream(&operand.id) == Some(stream)
                        && operand.scope_record_index == group.scope_record_index
                        && operand.record_index == *member
                })
                .collect::<Vec<_>>();
            let [operand] = matches.as_slice() else {
                complete = false;
                break;
            };
            members.push((
                operand.record_index,
                resolved_edge_operand(operand),
                operand.deleted_boundary_edge_slots.clone(),
            ));
        }
        if !complete {
            continue;
        }
        if group.id == target.id {
            target_ordinal = Some(scope_groups.len());
        }
        scope_groups.push(members);
    }
    partition_unique_incomplete_edge_group(target_ordinal?, &scope_groups)
}

pub(crate) fn partition_unique_incomplete_edge_group(
    target_ordinal: usize,
    groups: &[EdgeGroupMembers],
) -> Option<Vec<i64>> {
    if groups.len() < 2 || target_ordinal >= groups.len() {
        return None;
    }
    let mut identities = HashSet::new();
    let mut universe = None::<Vec<i64>>;
    for (identity, _, deleted) in groups.iter().flatten() {
        if !identities.insert(*identity) {
            return None;
        }
        let mut deleted = deleted.clone();
        deleted.sort_unstable();
        deleted.dedup();
        if deleted.is_empty()
            || universe
                .as_ref()
                .is_some_and(|universe| *universe != deleted)
        {
            return None;
        }
        universe.get_or_insert(deleted);
    }
    let universe = universe?;
    if identities.len() != universe.len() {
        return None;
    }
    let incomplete = groups
        .iter()
        .enumerate()
        .filter(|(_, group)| group.iter().any(|(_, resolved, _)| resolved.is_none()))
        .map(|(ordinal, _)| ordinal)
        .collect::<Vec<_>>();
    if incomplete.as_slice() != [target_ordinal] {
        return None;
    }
    let mut reserved = Vec::new();
    for (ordinal, group) in groups.iter().enumerate() {
        if ordinal == target_ordinal {
            continue;
        }
        for (_, resolved, _) in group {
            let resolved = resolved.as_ref()?;
            if !universe.contains(resolved) || reserved.contains(resolved) {
                return None;
            }
            reserved.push(*resolved);
        }
    }
    let target = universe
        .into_iter()
        .filter(|candidate| !reserved.contains(candidate))
        .collect::<Vec<_>>();
    if target.len() != groups[target_ordinal].len()
        || groups[target_ordinal]
            .iter()
            .filter_map(|(_, resolved, _)| *resolved)
            .any(|resolved| !target.contains(&resolved))
    {
        return None;
    }
    Some(target)
}

pub(crate) fn common_deleted_edge_group_candidates<'a>(
    members: impl IntoIterator<Item = (bool, &'a [i64])>,
) -> Option<Vec<i64>> {
    let candidate_sets = members
        .into_iter()
        .filter_map(|(edge_bearing, candidates)| edge_bearing.then_some(candidates))
        .collect::<Vec<_>>();
    let member_count = candidate_sets.len();
    if member_count == 0 {
        return None;
    }
    let mut candidate_sets = candidate_sets.into_iter();
    let mut candidates = candidate_sets.next()?.to_vec();
    candidates.sort_unstable();
    candidates.dedup();
    if candidates.len() != member_count {
        return None;
    }
    for candidate_set in candidate_sets {
        let mut normalized = candidate_set.to_vec();
        normalized.sort_unstable();
        normalized.dedup();
        if normalized != candidates {
            return None;
        }
    }
    Some(candidates)
}

pub(crate) fn changed_boundary_count_edge_group_candidates<'a>(
    members: impl IntoIterator<Item = &'a [crate::records::DesignEdgeRecipeSelectorContext]>,
) -> Option<Vec<i64>> {
    let members = members.into_iter().collect::<Vec<_>>();
    if members.is_empty() || members.iter().any(|selectors| selectors.is_empty()) {
        return None;
    }
    let mut candidates = members
        .iter()
        .flat_map(|selectors| selectors.iter())
        .flat_map(|selector| selector.boundary_count_matching_edge_slots.iter().copied())
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.dedup();
    (candidates.len() == members.len()).then_some(candidates)
}

pub(crate) fn resolved_edge_operand(operand: &DesignEdgeOperand) -> Option<i64> {
    operand
        .resolved_edge_slot
        .or_else(|| resolve_edge_operand_candidates(operand))
}

pub(crate) fn edge_operand_reference_edge_sets(operand: &DesignEdgeOperand) -> Vec<&[i64]> {
    let reference_edge_slots = if operand.recipe_reference_contexts.is_empty() {
        operand
            .terminal_reference_edge_slots
            .iter()
            .map(Vec::as_slice)
            .collect::<Vec<_>>()
    } else {
        operand
            .recipe_reference_contexts
            .iter()
            .map(|context| context.changed_reference_edge_slots.as_slice())
            .collect::<Vec<_>>()
    };
    if let Some(local_topology_references) = &operand.local_topology_references {
        local_topology_references
            .iter()
            .filter_map(|ordinal| {
                reference_edge_slots.get(usize::try_from(ordinal.get()).ok()?.checked_sub(1)?)
            })
            .copied()
            .collect()
    } else {
        reference_edge_slots
    }
}

pub(crate) fn resolve_edge_operand_candidates(operand: &DesignEdgeOperand) -> Option<i64> {
    let deleted_reference = corroborated_deleted_reference_candidate(
        &operand.recipe_selectors,
        edge_operand_reference_edge_sets(operand),
        &operand.deleted_boundary_edge_slots,
    );
    resolved_edge_candidate_intersection_with_deleted_proofs(
        &operand.recipe_selectors,
        edge_operand_reference_edge_sets(operand),
        &operand.deleted_boundary_edge_slots,
        deleted_reference,
    )
}

pub(crate) fn unique_deleted_triplet_candidate(
    selector_contexts: &[crate::records::DesignEdgeRecipeSelectorContext],
    deleted_boundary_edges: &[i64],
) -> Option<i64> {
    if selector_contexts.is_empty() || deleted_boundary_edges.is_empty() {
        return None;
    }
    let mut candidates = selector_contexts
        .iter()
        .flat_map(|selector| selector.clause_triplet_edge_slots.iter())
        .flatten()
        .flat_map(|triplets| triplets.iter())
        .flatten()
        .copied()
        .filter(|edge| deleted_boundary_edges.contains(edge))
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.dedup();
    match candidates.as_slice() {
        [edge] => Some(*edge),
        _ => None,
    }
}

pub(crate) fn corroborated_deleted_reference_candidate<'a>(
    selector_contexts: &[crate::records::DesignEdgeRecipeSelectorContext],
    reference_edge_sets: impl IntoIterator<Item = &'a [i64]>,
    deleted_boundary_edges: &[i64],
) -> Option<i64> {
    let selector_supports = |edge: i64| {
        selector_contexts.iter().any(|selector| {
            selector.incidence_matching_edge_slots.contains(&edge)
                || selector.boundary_count_matching_edge_slots.contains(&edge)
                || selector
                    .clause_triplet_edge_slots
                    .iter()
                    .flatten()
                    .any(|pair| pair.iter().any(|edges| edges.contains(&edge)))
        })
    };
    let mut candidates = reference_edge_sets
        .into_iter()
        .filter_map(|edges| match edges {
            [edge] => Some(*edge),
            _ => None,
        })
        .filter(|edge| deleted_boundary_edges.contains(edge) && selector_supports(*edge))
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.dedup();
    match candidates.as_slice() {
        [edge] => Some(*edge),
        _ => None,
    }
}

pub(crate) fn resolved_edge_candidate_intersection<'a>(
    selector_contexts: &[crate::records::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: impl IntoIterator<Item = &'a [i64]>,
) -> Option<i64> {
    resolved_edge_candidate_intersection_with_extra_proofs(selector_contexts, shared_edge_sets, [])
}

pub(crate) fn resolved_edge_candidate_intersection_with_deleted_proofs<'a>(
    selector_contexts: &[crate::records::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: impl IntoIterator<Item = &'a [i64]>,
    deleted_boundary_edges: &[i64],
    deleted_reference: Option<i64>,
) -> Option<i64> {
    resolved_edge_candidate_intersection_with_extra_proofs(
        selector_contexts,
        shared_edge_sets,
        [
            unique_deleted_triplet_candidate(selector_contexts, deleted_boundary_edges),
            deleted_reference,
        ],
    )
}

fn resolved_edge_candidate_intersection_with_extra_proofs<'a, const N: usize>(
    selector_contexts: &[crate::records::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: impl IntoIterator<Item = &'a [i64]>,
    extra_proofs: [Option<i64>; N],
) -> Option<i64> {
    let ordered_edge_sets = shared_edge_sets.into_iter().collect::<Vec<_>>();
    let shared_edge_sets = ordered_edge_sets
        .iter()
        .copied()
        .filter(|edges| !edges.is_empty())
        .collect::<Vec<_>>();
    let references_unavailable = !ordered_edge_sets.is_empty() && shared_edge_sets.is_empty();
    let reference = (shared_edge_sets.len() >= 2)
        .then(|| unique_edge_set_intersection(&shared_edge_sets))
        .flatten();
    let incidence = (!references_unavailable)
        .then(|| corroborated_edge_intersection(selector_contexts, &shared_edge_sets, false))
        .flatten();
    let boundary_count = (!references_unavailable)
        .then(|| corroborated_edge_intersection(selector_contexts, &shared_edge_sets, true))
        .flatten();
    let common_triplet =
        corroborated_common_triplet_intersection(selector_contexts, &shared_edge_sets);
    let cross_clause_triplet =
        corroborated_cross_clause_triplet_intersection(selector_contexts, &shared_edge_sets);
    let proofs = [
        reference,
        incidence,
        boundary_count,
        common_triplet,
        cross_clause_triplet,
    ]
    .into_iter()
    .chain(extra_proofs)
    .flatten()
    .collect::<Vec<_>>();
    let edge = *proofs.first()?;
    proofs.iter().all(|proof| *proof == edge).then_some(edge)
}

fn corroborated_common_triplet_intersection(
    selector_contexts: &[crate::records::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: &[&[i64]],
) -> Option<i64> {
    let edge_sets = selector_contexts.iter().flat_map(|selector| {
        selector
            .clause_entries
            .iter()
            .zip(&selector.clause_triplet_edge_slots)
            .filter_map(|(entry, triplet_edges)| {
                entry.as_ref()?.common_incident_edge_ordinal?;
                let [first, second] = triplet_edges.as_ref()?;
                let mut common = first.clone();
                common.retain(|edge| second.contains(edge));
                common.sort_unstable();
                common.dedup();
                (!common.is_empty()).then_some(common)
            })
    });
    corroborated_edge_set_intersection(edge_sets, shared_edge_sets)
}

fn corroborated_cross_clause_triplet_intersection(
    selector_contexts: &[crate::records::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: &[&[i64]],
) -> Option<i64> {
    let edge_sets = selector_contexts.iter().flat_map(|selector| {
        let [Some(left), Some(right)] = selector.clause_triplet_edge_slots.as_slice() else {
            return Vec::new();
        };
        left.iter()
            .zip(right)
            .filter_map(|(left, right)| {
                let mut common = left.clone();
                common.retain(|edge| right.contains(edge));
                common.sort_unstable();
                common.dedup();
                (!common.is_empty()).then_some(common)
            })
            .collect::<Vec<_>>()
    });
    corroborated_edge_set_intersection(edge_sets, shared_edge_sets)
}

fn corroborated_edge_set_intersection(
    mut edge_sets: impl Iterator<Item = Vec<i64>>,
    shared_edge_sets: &[&[i64]],
) -> Option<i64> {
    let mut candidates = edge_sets.next()?;
    for edges in edge_sets {
        candidates.retain(|candidate| edges.contains(candidate));
        if candidates.is_empty() {
            return None;
        }
    }
    for edges in shared_edge_sets {
        candidates.retain(|candidate| edges.contains(candidate));
        if candidates.is_empty() {
            return None;
        }
    }
    (candidates.len() == 1).then_some(candidates[0])
}

fn unique_edge_set_intersection(edge_sets: &[&[i64]]) -> Option<i64> {
    let mut sets = edge_sets.iter();
    let mut candidates = sets.next()?.to_vec();
    candidates.sort_unstable();
    candidates.dedup();
    for edge_set in sets {
        candidates.retain(|candidate| edge_set.contains(candidate));
        if candidates.is_empty() {
            return None;
        }
    }
    (candidates.len() == 1).then_some(candidates[0])
}

fn corroborated_edge_intersection(
    selector_contexts: &[crate::records::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: &[&[i64]],
    boundary_counts_only: bool,
) -> Option<i64> {
    let candidates = corroborated_edge_candidates(
        selector_contexts,
        shared_edge_sets.iter().copied(),
        boundary_counts_only,
    )?;
    (candidates.len() == 1).then_some(candidates[0])
}

fn corroborated_edge_candidates<'a>(
    selector_contexts: &[crate::records::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: impl IntoIterator<Item = &'a [i64]>,
    boundary_counts_only: bool,
) -> Option<Vec<i64>> {
    let mut selectors = selector_contexts.iter();
    let first = selector_candidate_edges(selectors.next()?, boundary_counts_only);
    if first.is_empty() {
        return None;
    }
    let mut candidates = first.to_vec();
    candidates.sort_unstable();
    candidates.dedup();
    for selector in selectors {
        let selector_edges = selector_candidate_edges(selector, boundary_counts_only);
        if selector_edges.is_empty() {
            return None;
        }
        candidates.retain(|candidate| selector_edges.contains(candidate));
        if candidates.is_empty() {
            return None;
        }
    }
    for shared_edges in shared_edge_sets {
        candidates.retain(|candidate| shared_edges.contains(candidate));
        if candidates.is_empty() {
            return None;
        }
    }
    Some(candidates)
}

fn selector_candidate_edges(
    selector: &crate::records::DesignEdgeRecipeSelectorContext,
    boundary_counts_only: bool,
) -> &[i64] {
    if boundary_counts_only {
        &selector.boundary_count_matching_edge_slots
    } else {
        &selector.incidence_matching_edge_slots
    }
}

pub(crate) fn project_fixed_fillet(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        FeatureDefinition, FilletGroup, Length, RadiusSpec, VariableRadius,
    };

    let fixed = scope.fixed_fillet_parameters.as_ref()?;
    let stream = native_stream(&scope.id)?;
    let groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && !group.members.is_empty()
                && group.members.iter().all(|member| {
                    edge_operands.iter().any(|operand| {
                        native_stream(&operand.id) == Some(stream)
                            && operand.scope_record_index == scope.record_index
                            && operand.record_index == *member
                    }) || edge_identity_operands.iter().any(|operand| {
                        native_stream(&operand.id) == Some(stream)
                            && operand.scope_record_index == scope.record_index
                            && operand.group_record_index == group.record_index
                            && operand.record_index == *member
                    })
                })
        })
        .collect::<Vec<_>>();
    let [group] = groups.as_slice() else {
        return None;
    };
    let radius = match fixed.radii.as_slice() {
        [radius] if *radius > 0.0 => RadiusSpec::Constant {
            radius: Length(*radius * 10.0),
        },
        [first, second, intermediate @ ..]
            if intermediate.len() == fixed.intermediate_parameters.len() =>
        {
            let mut points = Vec::with_capacity(intermediate.len() + 2);
            points.push(VariableRadius {
                parameter: 0.0,
                radius: Length(*first * 10.0),
            });
            points.extend(intermediate.iter().zip(&fixed.intermediate_parameters).map(
                |(radius, parameter)| VariableRadius {
                    parameter: *parameter,
                    radius: Length(*radius * 10.0),
                },
            ));
            points.push(VariableRadius {
                parameter: 1.0,
                radius: Length(*second * 10.0),
            });
            RadiusSpec::Variable { points }
        }
        _ => return None,
    };
    let edge_radius = match radius {
        RadiusSpec::Constant { radius } => Some(radius.0),
        RadiusSpec::Chordal { .. } => None,
        _ => None,
    };
    let edges = resolved_edge_group(
        group,
        construction_groups,
        edge_operands,
        edge_identity_operands,
        scope.previous_history_state_id,
        &neutral_feature_id(scope),
        edge_radius,
    );
    Some(FeatureDefinition::Fillet {
        groups: vec![FilletGroup {
            edges,
            radius,
            tangency_weight: Some(fixed.tangency_weight),
        }],
    })
}

#[cfg(test)]
mod radius_identity_tests {
    use super::radius_edge_identity_group_candidates;
    use crate::records::DesignEdgeIdentityOperand;

    fn identity(record_index: u32, candidates: &[(i64, f64)]) -> DesignEdgeIdentityOperand {
        serde_json::from_value(serde_json::json!({
            "id": format!("f3d:test:identity#{record_index}"),
            "scope_record_index": 1,
            "group_record_index": 2,
            "group_member_ordinal": record_index,
            "record_index": record_index,
            "byte_offset": 0,
            "class_tag": "277",
            "local_id": record_index,
            "local_id_offset": 0,
            "asset_id": "asset",
            "asset_id_offset": 0,
            "context_id": "context",
            "context_id_offset": 0,
            "transition_edge_candidates": candidates
                .iter()
                .map(|(edge, _)| *edge)
                .collect::<Vec<_>>(),
            "treatment_radius_candidates": candidates
                .iter()
                .map(|(edge, radius)| serde_json::json!({
                    "edge_slot": edge,
                    "radius": radius
                }))
                .collect::<Vec<_>>()
        }))
        .expect("edge identity")
    }

    #[test]
    fn identity_radius_candidates_select_only_the_matching_law() {
        let first = identity(10, &[(17, 3.0), (18, 5.0)]);
        let second = identity(11, &[(19, 3.0), (20, 5.0)]);
        assert_eq!(
            radius_edge_identity_group_candidates(&[&first, &second], 3.0),
            Some(vec![17, 19])
        );
        assert_eq!(
            radius_edge_identity_group_candidates(&[&first, &second], 4.0),
            None
        );
    }
}
