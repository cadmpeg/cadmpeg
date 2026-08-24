// SPDX-License-Identifier: Apache-2.0
//! Resolve edge-selection operands to stable edge identities.

use crate::ids::{self, native_stream, neutral_feature_id};
use crate::records::{
    DesignConstructionOperandGroup, DesignEdgeIdentityOperand, DesignEdgeOperand,
    DesignEdgeTreatmentVertexOperand, DesignParameterScope,
};
use std::collections::{HashMap, HashSet};

pub(crate) fn resolved_edge_group(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
) -> cadmpeg_ir::features::EdgeSelection {
    resolved_edge_group_with_transition_chain(
        group,
        groups,
        operands,
        identity_operands,
        previous_state_id,
        feature_id,
        EdgeGroupProof::Generic,
    )
}

/// Resolve a modern grouped `SurfacePatch` edge group from its exact
/// recipe references.
///
/// Every nonempty exact reference of one member must identify the same sole
/// edge, and the complete group must map to distinct edges. Conflicting exact
/// references suppress generic reconstruction because their serialized
/// member identity is unresolved.
pub(crate) fn resolved_surface_patch_edge_group(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
) -> cadmpeg_ir::features::EdgeSelection {
    let fallback = || {
        resolved_edge_group(
            group,
            groups,
            operands,
            identity_operands,
            previous_state_id,
            feature_id,
        )
    };
    let stream = native_stream(&group.id);
    let mut member_ids = HashSet::new();
    if group
        .members
        .iter()
        .any(|member| !member_ids.insert(*member))
    {
        return fallback();
    }
    let matched_operands = group
        .members
        .iter()
        .map(|member| {
            let mut matches = operands.iter().filter(|operand| {
                native_stream(&operand.id) == stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.record_index == *member
            });
            let operand = matches.next()?;
            matches.next().is_none().then_some(operand)
        })
        .collect::<Option<Vec<_>>>();
    let Some(matched_operands) = matched_operands else {
        return fallback();
    };
    let edges = match surface_patch_grouped_recipe_edges(&matched_operands) {
        SurfacePatchRecipeEdges::Absent => return fallback(),
        SurfacePatchRecipeEdges::Inconclusive => {
            return cadmpeg_ir::features::EdgeSelection::Native(group.id.clone());
        }
        SurfacePatchRecipeEdges::Resolved(edges) => edges,
    };
    let state_id = previous_state_id.or_else(|| {
        let mut states = matched_operands
            .iter()
            .filter_map(|operand| operand.recipe_state_id);
        let state_id = states.next()?;
        (states.all(|candidate| candidate == state_id)
            && matched_operands
                .iter()
                .all(|operand| operand.recipe_state_id == Some(state_id)))
        .then_some(state_id)
    });
    let Some(state_id) = state_id else {
        return cadmpeg_ir::features::EdgeSelection::Edges(edges);
    };
    let Some(edge_slots) = edges
        .iter()
        .map(stable_edge_slot)
        .collect::<Option<Vec<_>>>()
    else {
        return fallback();
    };
    let feature_key = feature_id
        .0
        .split_once('#')
        .map_or(feature_id.0.as_str(), |(_, key)| key);
    cadmpeg_ir::features::EdgeSelection::Historical {
        state: feature_input_topology_id(feature_id, state_id),
        edges: edge_slots
            .into_iter()
            .map(|edge_slot| {
                ids::history_input_edge_id(
                    &ids::history_input_prefix(feature_key, state_id),
                    edge_slot,
                )
            })
            .collect(),
        native: group.id.clone(),
    }
}

#[derive(Debug, PartialEq)]
enum SurfacePatchRecipeEdges {
    Absent,
    Inconclusive,
    Resolved(Vec<cadmpeg_ir::ids::EdgeId>),
}

fn surface_patch_grouped_recipe_edges(operands: &[&DesignEdgeOperand]) -> SurfacePatchRecipeEdges {
    if operands.is_empty() {
        return SurfacePatchRecipeEdges::Absent;
    }
    let mut edges = Vec::with_capacity(operands.len());
    let mut has_absent_member = false;
    for operand in operands {
        let mut exact = operand
            .recipe_references
            .iter()
            .filter(|reference| !reference.candidate_edges.is_empty());
        let Some(first) = exact.next() else {
            has_absent_member = true;
            continue;
        };
        let [edge] = first.candidate_edges.as_slice() else {
            return SurfacePatchRecipeEdges::Inconclusive;
        };
        if exact.any(|reference| reference.candidate_edges.as_slice() != std::slice::from_ref(edge))
        {
            return SurfacePatchRecipeEdges::Inconclusive;
        }
        edges.push(edge.clone());
    }
    if has_absent_member {
        return SurfacePatchRecipeEdges::Absent;
    }
    let mut distinct = edges.clone();
    distinct.sort_by(|left, right| left.0.cmp(&right.0));
    distinct.dedup();
    if distinct.len() != edges.len() {
        return SurfacePatchRecipeEdges::Inconclusive;
    }
    SurfacePatchRecipeEdges::Resolved(edges)
}

fn stable_edge_slot(edge: &cadmpeg_ir::ids::EdgeId) -> Option<i64> {
    edge.0
        .rsplit_once('#')?
        .1
        .split(':')
        .next()?
        .parse::<i64>()
        .ok()
}

/// Resolve a selectorless `EdgeFlange` group from its updated source edges.
///
/// An `EdgeFlange` operation preserves the selected source edge as an updated
/// edge. This fallback is admitted only when each group member has exactly one
/// such edge and carries no selector or reference-context evidence. A
/// multi-edge update therefore remains native instead of assigning every
/// changed boundary edge to the group.
pub(crate) fn resolved_edge_flange_group(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
) -> cadmpeg_ir::features::EdgeSelection {
    use cadmpeg_ir::features::EdgeSelection;

    let selection = resolved_edge_group(
        group,
        groups,
        operands,
        identity_operands,
        previous_state_id,
        feature_id,
    );
    if !matches!(selection, EdgeSelection::Native(_)) {
        return selection;
    }
    let Some(previous_state_id) = previous_state_id else {
        return selection;
    };
    let stream = native_stream(&group.id);
    let mut members = HashSet::new();
    let candidate_sets = group
        .members
        .iter()
        .map(|member| {
            if !members.insert(*member) {
                return None;
            }
            let matching = operands
                .iter()
                .filter(|operand| {
                    native_stream(&operand.id) == stream
                        && operand.scope_record_index == group.scope_record_index
                        && operand.record_index == *member
                })
                .collect::<Vec<_>>();
            let [operand] = matching.as_slice() else {
                return None;
            };
            edge_flange_updated_edge_candidate(operand)
        })
        .collect::<Option<Vec<_>>>();
    let Some(candidate_sets) = candidate_sets else {
        return selection;
    };
    let Some(edges) = unique_bipartite_assignment(&candidate_sets) else {
        return selection;
    };
    if edges.is_empty() {
        return selection;
    }
    let feature_key = feature_id
        .0
        .split_once('#')
        .map_or(feature_id.0.as_str(), |(_, key)| key);
    let state = feature_input_topology_id(feature_id, previous_state_id);
    EdgeSelection::Historical {
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
        native: group.id.clone(),
    }
}

fn edge_flange_updated_edge_candidate(operand: &DesignEdgeOperand) -> Option<Vec<i64>> {
    if !operand.recipe_references.is_empty()
        || !operand.recipe_selectors.is_empty()
        || !operand.recipe_reference_contexts.is_empty()
        || operand.local_topology_references.is_some()
    {
        return None;
    }
    let [edge] = operand.updated_boundary_edge_slots.as_slice() else {
        return None;
    };
    if !operand.preceding_boundary_edge_slots.contains(edge)
        || !operand.changed_boundary_edge_slots.contains(edge)
        || operand.deleted_boundary_edge_slots.contains(edge)
        || !operand.result_boundary_edge_slots.contains(edge)
    {
        return None;
    }
    Some(vec![*edge])
}

/// Resolve an edge-treatment group with the exact transition chain available
/// to Fillet and Chamfer operations.
#[cfg(test)]
pub(crate) fn resolved_edge_treatment_group(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    treatment_radius: Option<f64>,
) -> cadmpeg_ir::features::EdgeSelection {
    resolved_edge_treatment_group_with_corners(
        group,
        groups,
        operands,
        identity_operands,
        &[],
        &[],
        previous_state_id,
        feature_id,
        treatment_radius,
    )
}

#[allow(clippy::too_many_arguments)] // The arguments are distinct native operand arenas and resolution context.
pub(crate) fn resolved_edge_treatment_group_with_corners(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    vertex_operands: &[DesignEdgeTreatmentVertexOperand],
    histories: &[crate::history_records::AsmHistory],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    treatment_radius: Option<f64>,
) -> cadmpeg_ir::features::EdgeSelection {
    use cadmpeg_ir::features::EdgeSelection;

    let stream = native_stream(&group.id);
    let has_group_corner = vertex_operands.iter().any(|operand| {
        native_stream(&operand.id) == stream
            && operand.scope_record_index == group.scope_record_index
            && operand.group_record_index == group.record_index
    });
    if !has_group_corner {
        return resolved_edge_group_with_transition_chain(
            group,
            groups,
            operands,
            identity_operands,
            previous_state_id,
            feature_id,
            EdgeGroupProof::Treatment {
                radius: treatment_radius,
            },
        );
    }
    let mut edge_group = group.clone();
    let mut corner_slots = Vec::new();
    if group.members.len() != group.member_offsets.len() {
        return EdgeSelection::Native(group.id.clone());
    }
    edge_group.members.clear();
    edge_group.member_offsets.clear();
    for (ordinal, (member, offset)) in group
        .members
        .iter()
        .copied()
        .zip(group.member_offsets.iter().copied())
        .enumerate()
    {
        let edge_count = operands
            .iter()
            .filter(|operand| {
                native_stream(&operand.id) == stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.record_index == member
            })
            .count();
        let corner = u32::try_from(ordinal).ok().and_then(|ordinal| {
            let mut matches = vertex_operands.iter().filter(|operand| {
                native_stream(&operand.id) == stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == ordinal
                    && operand.recipe.record_index == member
            });
            let corner = matches.next()?;
            matches.next().is_none().then_some(corner)
        });
        match (edge_count, corner) {
            (1, None) => {
                edge_group.members.push(member);
                edge_group.member_offsets.push(offset);
            }
            (0, Some(corner)) => {
                let (Some(state), Some(vertex)) = (
                    corner.recipe.recipe_state_id,
                    corner.recipe.resolved_vertex_slot,
                ) else {
                    return EdgeSelection::Native(group.id.clone());
                };
                if Some(state) != previous_state_id {
                    return EdgeSelection::Native(group.id.clone());
                }
                corner_slots.push(vertex);
            }
            _ => return EdgeSelection::Native(group.id.clone()),
        }
    }
    if edge_group.members.is_empty() {
        return EdgeSelection::Native(group.id.clone());
    }
    let selection = resolved_edge_group_with_transition_chain(
        &edge_group,
        groups,
        operands,
        identity_operands,
        previous_state_id,
        feature_id,
        EdgeGroupProof::Treatment {
            radius: treatment_radius,
        },
    );
    if corner_slots.is_empty() {
        return selection;
    }
    let EdgeSelection::Historical { edges, .. } = &selection else {
        return EdgeSelection::Native(group.id.clone());
    };
    let Some(state_id) = previous_state_id else {
        return EdgeSelection::Native(group.id.clone());
    };
    let mut states = histories
        .iter()
        .filter(|history| ids::same_native_occurrence(&history.id, &group.id))
        .flat_map(|history| &history.states)
        .filter(|state| state.state_id == state_id);
    let Some(topology) = states.next().and_then(|state| state.topology.as_ref()) else {
        return EdgeSelection::Native(group.id.clone());
    };
    if states.next().is_some() {
        return EdgeSelection::Native(group.id.clone());
    }
    let edge_slots = edges
        .iter()
        .map(|edge| edge.0.rsplit(':').next()?.parse::<i64>().ok())
        .collect::<Option<Vec<_>>>();
    let Some(edge_slots) = edge_slots else {
        return EdgeSelection::Native(group.id.clone());
    };
    let endpoints = edge_slots
        .iter()
        .map(|edge_slot| {
            let mut matches = topology
                .edge_vertices
                .iter()
                .filter(|edge| edge.edge == *edge_slot);
            let edge = matches.next()?;
            matches
                .next()
                .is_none()
                .then_some([edge.start_vertex, edge.end_vertex])
        })
        .collect::<Option<Vec<_>>>();
    let Some(endpoints) = endpoints else {
        return EdgeSelection::Native(group.id.clone());
    };
    let endpoints = endpoints.into_iter().flatten().collect::<HashSet<_>>();
    if corner_slots.iter().all(|corner| endpoints.contains(corner)) {
        selection
    } else {
        EdgeSelection::Native(group.id.clone())
    }
}

#[derive(Clone, Copy)]
enum EdgeGroupProof {
    /// Member-local recipe and persistent-identity proofs only.
    Generic,
    /// Fillet or Chamfer proofs that may consume an exact operation transition.
    Treatment { radius: Option<f64> },
}

fn resolved_edge_group_with_transition_chain(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    proof: EdgeGroupProof,
) -> cadmpeg_ir::features::EdgeSelection {
    use cadmpeg_ir::features::EdgeSelection;

    let (allow_edge_treatment_transition_chain, treatment_radius) = match proof {
        EdgeGroupProof::Generic => (false, None),
        EdgeGroupProof::Treatment { radius } => (true, radius),
    };

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
    let has_surface_patch_operand = operands.iter().any(|operand| {
        native_stream(&operand.id) == stream
            && operand.scope_record_index == group.scope_record_index
            && group.members.contains(&operand.record_index)
            && operand.surface_patch_recipe_structure.is_some()
    });
    if has_surface_patch_operand {
        let mut member_ids = HashSet::new();
        if group
            .members
            .iter()
            .any(|member| !member_ids.insert(*member))
        {
            return unmatched_selection(previous_state_id);
        }
        let matched_operands = group
            .members
            .iter()
            .map(|member| {
                let mut matches = operands.iter().filter(|operand| {
                    native_stream(&operand.id) == stream
                        && operand.scope_record_index == group.scope_record_index
                        && operand.record_index == *member
                });
                let operand = matches.next()?;
                matches.next().is_none().then_some(operand)
            })
            .collect::<Option<Vec<_>>>();
        let Some(matched_operands) = matched_operands else {
            return unmatched_selection(previous_state_id);
        };
        if matched_operands
            .iter()
            .any(|operand| operand.surface_patch_recipe_structure.is_none())
        {
            return unmatched_selection(previous_state_id);
        }
        let state_id = previous_state_id.or_else(|| {
            let mut states = matched_operands
                .iter()
                .filter_map(|operand| operand.recipe_state_id);
            let state_id = states.next()?;
            (states.all(|candidate| candidate == state_id)
                && matched_operands
                    .iter()
                    .all(|operand| operand.recipe_state_id == Some(state_id)))
            .then_some(state_id)
        });
        let Some(state_id) = state_id else {
            return unmatched_selection(previous_state_id);
        };
        let Some(edges) = matched_operands
            .iter()
            .map(|operand| operand.resolved_edge_slot)
            .collect::<Option<Vec<_>>>()
        else {
            return unmatched_selection(Some(state_id));
        };
        let mut resolved_edges = Vec::new();
        for edge_slot in edges {
            if !resolved_edges.contains(&edge_slot) {
                resolved_edges.push(edge_slot);
            }
        }
        if resolved_edges.is_empty() {
            return unmatched_selection(Some(state_id));
        }
        return EdgeSelection::Historical {
            state: feature_input_topology_id(feature_id, state_id),
            edges: resolved_edges
                .into_iter()
                .map(|edge_slot| {
                    ids::history_input_edge_id(
                        &ids::history_input_prefix(feature_key, state_id),
                        edge_slot,
                    )
                })
                .collect(),
            native: group.id.clone(),
        };
    }
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
    let has_unstructured_recipe_operand = group.members.iter().any(|member| {
        operands.iter().any(|operand| {
            native_stream(&operand.id) == stream
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index == *member
                && !operand.recipe_program.is_empty()
                && operand.recipe_structure.is_none()
        })
    });
    if has_recipe_operands && has_unstructured_recipe_operand {
        return unmatched_selection(previous_state_id);
    }
    let has_standard_recipe_operands = group.members.iter().any(|member| {
        operands.iter().any(|operand| {
            native_stream(&operand.id) == stream
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index == *member
                && operand.recipe_structure.is_some()
        })
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
    let identity_transition_slots = (allow_edge_treatment_transition_chain
        && treatment_radius.is_none()
        && group.members.len() == 1)
        .then(|| {
            let [operand] = identity_matches.as_ref()?.as_slice() else {
                return None;
            };
            let mut edges = operand.transition_edge_candidates.clone();
            edges.sort_unstable();
            edges.dedup();
            (!edges.is_empty()).then_some(edges)
        })
        .flatten();
    let identity_group_transition_slots = identity_matches.as_ref().and_then(|identity_operands| {
        let first = identity_operands.first()?;
        let mut edges = first.transition_edge_candidates.clone();
        edges.sort_unstable();
        edges.dedup();
        let is_uniform_compact_transition_chain = allow_edge_treatment_transition_chain
            && !edges.is_empty()
            && identity_operands.iter().all(|operand| {
                if !operand.compact_layout {
                    return false;
                }
                let mut candidate = operand.transition_edge_candidates.clone();
                candidate.sort_unstable();
                candidate.dedup();
                candidate == edges
            });
        is_uniform_compact_transition_chain.then_some(edges)
    });
    let recipe_supports_transition_chain = |chain: &[i64]| {
        let member_operands = group
            .members
            .iter()
            .map(|member| {
                let mut matches = operands.iter().filter(|operand| {
                    native_stream(&operand.id) == stream
                        && operand.scope_record_index == group.scope_record_index
                        && operand.record_index == *member
                });
                let operand = matches.next()?;
                matches.next().is_none().then_some(operand)
            })
            .collect::<Option<Vec<_>>>();
        let Some(member_operands) = member_operands else {
            return false;
        };
        transition_chain_is_supported_by_recipe(chain, group.members.len(), member_operands)
    };
    let identity_transition_is_supported = identity_transition_slots
        .as_deref()
        .or(identity_group_transition_slots.as_deref())
        .is_some_and(recipe_supports_transition_chain);
    // A cardinality mismatch is a group-level transition proof, not a
    // member-to-edge assignment. Recipe evidence must therefore cover the
    // complete chain before it can replace the unresolved member identities.
    let identity_group_transition_is_admitted = identity_group_transition_slots
        .as_deref()
        .is_some_and(|edges| {
            edges.len() == group.members.len() || recipe_supports_transition_chain(edges)
        });
    let identity_radius_slots = treatment_radius.and_then(|radius| {
        radius_edge_identity_group_candidates(identity_matches.as_ref()?, radius)
    });
    let has_complete_identity_selection = identity_matches.as_ref().is_some_and(|operands| {
        !operands.is_empty()
            && (operands.iter().all(|operand| {
                operand.resolved_edge_slot.is_some() || !operand.resolved_edge_slots.is_empty()
            }) || identity_transition_slots.is_some()
                || identity_group_transition_is_admitted
                || identity_radius_slots.is_some())
    });
    let all_member_identities_are_lost =
        !group.members.is_empty() && group.lost_edge_references.len() == group.members.len();
    if let Some(identity_matches) = identity_matches.as_ref().filter(|_| {
        has_complete_identity_selection
            && !has_standard_recipe_operands
            && (!has_recipe_operands
                || !has_concrete_recipe_evidence
                || identity_transition_is_supported
                || all_member_identities_are_lost)
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
        if let Some(edges) = identity_radius_slots.as_ref() {
            return EdgeSelection::Historical {
                state,
                edges: edges
                    .iter()
                    .map(|edge_slot| {
                        ids::history_input_edge_id(
                            &ids::history_input_prefix(feature_key, previous_state_id),
                            *edge_slot,
                        )
                    })
                    .collect(),
                native: group.id.clone(),
            };
        }
        if let Some(edges) = identity_group_transition_slots.as_ref() {
            return EdgeSelection::Historical {
                state,
                edges: edges
                    .iter()
                    .map(|edge_slot| {
                        ids::history_input_edge_id(
                            &ids::history_input_prefix(feature_key, previous_state_id),
                            *edge_slot,
                        )
                    })
                    .collect(),
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
        .or_else(|| unique_edge_group_assignment(&matched_operands))
        .or_else(|| changed_reference_edge_group_candidates(&matched_operands));
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
            .or_else(|| deleted_reference_edge_group_candidates(&matched_operands))
            .or_else(|| {
                common_deleted_edge_group_candidates(matched_operands.iter().map(|operand| {
                    (
                        !operand.changed_boundary_edge_slots.is_empty(),
                        operand.deleted_boundary_edge_slots.as_slice(),
                    )
                }))
            })
            .or_else(|| {
                allow_edge_treatment_transition_chain
                    .then(|| contextual_deleted_edge_group_candidates(&matched_operands))
                    .flatten()
            })
            .or_else(|| {
                allow_edge_treatment_transition_chain
                    .then(|| result_boundary_reference_edge_group_candidates(&matched_operands))
                    .flatten()
            })
            .or_else(|| {
                allow_edge_treatment_transition_chain
                    .then(|| deleted_boundary_edge_group_candidates(&matched_operands))
                    .flatten()
            })
            .or_else(|| scope_partition_edge_group_candidates(group, groups, operands))
    };
    let resolved_slots = exact_slots.or_else(|| {
        (!has_standard_recipe_operands)
            .then(|| transition_state_id.and_then(|_| transition_slots()))
            .flatten()
    });
    let Some(resolved_slots) = resolved_slots else {
        if !group.lost_edge_references.is_empty() {
            return lost_selection();
        }
        if has_standard_recipe_operands {
            return EdgeSelection::Native(group.id.clone());
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

pub(crate) fn resolved_hem_edge_group(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
) -> cadmpeg_ir::features::EdgeSelection {
    use cadmpeg_ir::features::EdgeSelection;

    let selection = resolved_edge_group(
        group,
        groups,
        operands,
        identity_operands,
        previous_state_id,
        feature_id,
    );
    if !matches!(selection, EdgeSelection::Native(_)) {
        return selection;
    }
    let Some(previous_state_id) = previous_state_id else {
        return selection;
    };
    let [member] = group.members.as_slice() else {
        return selection;
    };
    let matching_operands = operands
        .iter()
        .filter(|operand| {
            native_stream(&operand.id) == native_stream(&group.id)
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index == *member
        })
        .collect::<Vec<_>>();
    let [operand] = matching_operands.as_slice() else {
        return selection;
    };
    let Some(edge) = hem_transition_edge_slot(operand) else {
        return selection;
    };
    let feature_key = feature_id
        .0
        .split_once('#')
        .map_or(feature_id.0.as_str(), |(_, key)| key);
    EdgeSelection::Historical {
        state: feature_input_topology_id(feature_id, previous_state_id),
        edges: vec![ids::history_input_edge_id(
            &ids::history_input_prefix(feature_key, previous_state_id),
            edge,
        )],
        native: group.id.clone(),
    }
}

/// Return the one historical edge a single-member Hem operand identifies.
///
/// A directly resolved operand is preferred. The transition proof is the
/// fallback used by the compact recipe form, where the operand carries only
/// the changed support boundaries and the selectorless edge context.
pub(crate) fn resolved_hem_edge_slot(
    operand: &DesignEdgeOperand,
    previous_state_id: Option<i64>,
) -> Option<i64> {
    let mut direct = operand.resolved_edge_slot.into_iter().collect::<Vec<_>>();
    direct.sort_unstable();
    direct.dedup();
    if let [edge] = direct.as_slice() {
        return Some(*edge);
    }
    previous_state_id.and_then(|_| hem_transition_edge_slot(operand))
}

/// Check recipe evidence before an exact edge-treatment transition chain
/// replaces persistent recipe or identity context.
///
/// Changed and deleted recipe boundaries are hard evidence. A deleted edge
/// proven by a recipe but absent from the treatment chain contradicts that
/// chain. A member with deleted boundaries must also share at least one edge
/// with the chain. When any member has changed or deleted boundary evidence,
/// every edge in the exact chain must occur in the group's combined recipe
/// boundary set. The set includes the selected reference contexts because a
/// structured recipe can carry the operation edge on a reference face that is
/// not the operand's primary candidate face.
fn transition_chain_is_supported_by_recipe<'a>(
    chain: &[i64],
    member_count: usize,
    operands: impl IntoIterator<Item = &'a DesignEdgeOperand>,
) -> bool {
    let operands = operands.into_iter().collect::<Vec<_>>();
    if operands.len() != member_count {
        return false;
    }
    let mut all_recipe_edges = Vec::new();
    for operand in &operands {
        let resolved = resolved_edge_operand(operand);
        if let Some(edge) = resolved {
            if operand.deleted_boundary_edge_slots.contains(&edge) && !chain.contains(&edge) {
                return false;
            }
        }
        if !operand.deleted_boundary_edge_slots.is_empty()
            && !operand
                .deleted_boundary_edge_slots
                .iter()
                .any(|edge| chain.contains(edge))
        {
            return false;
        }
        all_recipe_edges.extend(
            operand
                .changed_boundary_edge_slots
                .iter()
                .chain(&operand.deleted_boundary_edge_slots)
                .copied(),
        );
        all_recipe_edges.extend(
            edge_operand_reference_edge_sets(operand)
                .into_iter()
                .flatten()
                .copied(),
        );
    }
    all_recipe_edges.sort_unstable();
    all_recipe_edges.dedup();
    all_recipe_edges.is_empty() || chain.iter().all(|edge| all_recipe_edges.contains(edge))
}

fn hem_transition_edge_slot(operand: &DesignEdgeOperand) -> Option<i64> {
    let reference_contexts = operand.recipe_reference_contexts.as_slice();
    let empty_contexts = reference_contexts
        .iter()
        .filter(|context| context.changed_reference_edge_slots.is_empty())
        .collect::<Vec<_>>();
    let [empty_context] = empty_contexts.as_slice() else {
        return None;
    };
    if !empty_context.result_faces.is_empty()
        || !empty_context.preceding_faces.is_empty()
        || !empty_context.preceding_support_face_slots.is_empty()
        || reference_contexts.iter().any(|context| {
            !context.changed_reference_edge_slots.is_empty()
                && (context.result_faces.is_empty()
                    || context.preceding_support_face_slots.is_empty())
        })
    {
        return None;
    }
    unique_hem_transition_edge_candidate(
        &operand.changed_boundary_edge_slots,
        reference_contexts
            .iter()
            .map(|context| context.changed_reference_edge_slots.as_slice()),
    )
}

pub(crate) fn unique_hem_transition_edge_candidate<'a>(
    changed_boundary_edges: &[i64],
    reference_edge_sets: impl IntoIterator<Item = &'a [i64]>,
) -> Option<i64> {
    let reference_edge_sets = reference_edge_sets.into_iter().collect::<Vec<_>>();
    if reference_edge_sets.is_empty()
        || reference_edge_sets
            .iter()
            .filter(|edges| edges.is_empty())
            .count()
            != 1
    {
        return None;
    }
    let mut support_edges = reference_edge_sets
        .iter()
        .flat_map(|edges| edges.iter().copied())
        .collect::<Vec<_>>();
    support_edges.sort_unstable();
    support_edges.dedup();
    if support_edges.is_empty()
        || support_edges
            .iter()
            .any(|edge| !changed_boundary_edges.contains(edge))
    {
        return None;
    }
    let mut candidates = changed_boundary_edges
        .iter()
        .copied()
        .filter(|edge| !support_edges.contains(edge))
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.dedup();
    match candidates.as_slice() {
        [edge] => Some(*edge),
        _ => None,
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

pub(crate) fn changed_reference_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
) -> Option<Vec<i64>> {
    let candidate_sets = operands
        .iter()
        .map(|operand| {
            let mut changed_sets = operand
                .recipe_reference_contexts
                .iter()
                .map(|context| context.changed_reference_edge_slots.as_slice())
                .filter(|edges| !edges.is_empty());
            let mut candidates = changed_sets.next()?.to_vec();
            for changed in changed_sets {
                candidates.retain(|candidate| changed.contains(candidate));
            }
            candidates.sort_unstable();
            candidates.dedup();
            (!candidates.is_empty()).then_some(candidates)
        })
        .collect::<Option<Vec<_>>>()?;
    unique_bipartite_assignment(&candidate_sets)
}

pub(crate) fn deleted_reference_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
) -> Option<Vec<i64>> {
    let reference_candidates = operands
        .iter()
        .map(|operand| {
            let mut candidates = operand
                .recipe_reference_contexts
                .iter()
                .flat_map(|context| context.changed_reference_edge_slots.iter().copied())
                .collect::<Vec<_>>();
            candidates.sort_unstable();
            candidates.dedup();
            candidates
        })
        .collect::<Vec<_>>();
    let deleted_candidates = operands
        .iter()
        .map(|operand| operand.deleted_boundary_edge_slots.clone())
        .collect::<Vec<_>>();
    unique_deleted_reference_assignment(&reference_candidates, &deleted_candidates)
}

pub(crate) fn unique_deleted_reference_assignment(
    reference_candidates: &[Vec<i64>],
    deleted_candidates: &[Vec<i64>],
) -> Option<Vec<i64>> {
    if reference_candidates.len() != deleted_candidates.len() {
        return None;
    }
    let candidate_sets = reference_candidates
        .iter()
        .zip(deleted_candidates)
        .map(|(references, deleted)| {
            let mut candidates = references
                .iter()
                .copied()
                .filter(|edge| deleted.contains(edge))
                .collect::<Vec<_>>();
            candidates.sort_unstable();
            candidates.dedup();
            (!candidates.is_empty()).then_some(candidates)
        })
        .collect::<Option<Vec<_>>>()?;
    unique_bipartite_assignment(&candidate_sets)
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
    let reference_edge_sets = reference_edge_sets
        .into_iter()
        .filter(|edges| !edges.is_empty())
        .collect::<Vec<_>>();
    if !selector_contexts.is_empty() {
        return edge_assignment_candidates(selector_contexts, reference_edge_sets)
            .map(EdgeAssignmentCandidates::Edges);
    }
    let [first, second, ..] = reference_edge_sets.as_slice() else {
        return Some(EdgeAssignmentCandidates::Context);
    };
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
    // Radius candidates on identity operands describe the complete operation
    // transition, not one member. They prove a group only when that group has
    // one member. A multi-member group requires an exact contribution from
    // every identity; recipe-local radius candidates are handled separately.
    let use_transition_radius = operands.len() == 1;
    let mut chain = Vec::new();
    for operand in operands {
        let mut contribution = operand
            .resolved_edge_slot
            .iter()
            .copied()
            .chain(operand.resolved_edge_slots.iter().copied())
            .collect::<Vec<_>>();
        if use_transition_radius {
            contribution.extend(
                operand
                    .treatment_radius_candidates
                    .iter()
                    .filter(|candidate| (candidate.radius - radius).abs() <= tolerance)
                    .map(|candidate| candidate.edge_slot),
            );
        }
        if contribution.is_empty() {
            return None;
        }
        chain.extend(contribution);
    }
    chain.sort_unstable();
    chain.dedup();
    (!chain.is_empty()).then_some(chain)
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
    let mut assignment =
        cadmpeg_core::decode::alloc_filled(candidate_sets.len(), 0, "f3d edge assignment").ok()?;
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

/// Resolve a treatment group whose recipe members expose the complete deleted
/// predecessor-edge set, but do not expose one edge candidate per member.
///
/// The deleted set is an exact group proof only when every member has a full
/// structured recipe context, every deleted edge is a changed predecessor
/// boundary, each member contributes contextual evidence for at least one
/// deleted edge, and the group-wide set has one edge per member. The context
/// requirement excludes topology deletions that are visible only in the
/// feature transition; the cardinality requirement excludes a member recipe
/// that represents an edge chain rather than one selected edge.
pub(crate) fn deleted_boundary_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
) -> Option<Vec<i64>> {
    if operands.is_empty() {
        return None;
    }
    let mut deleted = Vec::new();
    let mut contextual = Vec::new();
    for operand in operands {
        if operand.recipe_structure.is_none()
            || operand.recipe_references.is_empty()
            || operand.recipe_references.len() != operand.recipe_reference_contexts.len()
            || operand.recipe_reference_contexts.is_empty()
            || operand.deleted_boundary_edge_slots.is_empty()
            || operand.deleted_boundary_edge_slots.iter().any(|edge| {
                !operand.preceding_boundary_edge_slots.contains(edge)
                    || !operand.changed_boundary_edge_slots.contains(edge)
            })
        {
            return None;
        }
        let member_contextual = operand
            .recipe_reference_contexts
            .iter()
            .flat_map(|context| context.changed_reference_edge_slots.iter().copied())
            .filter(|edge| operand.deleted_boundary_edge_slots.contains(edge))
            .collect::<Vec<_>>();
        if member_contextual.is_empty() {
            return None;
        }
        deleted.extend(operand.deleted_boundary_edge_slots.iter().copied());
        contextual.extend(member_contextual);
    }
    deleted.sort_unstable();
    deleted.dedup();
    contextual.sort_unstable();
    contextual.dedup();
    (deleted.len() == operands.len()
        && deleted
            .iter()
            .all(|edge| contextual.binary_search(edge).is_ok()))
    .then_some(deleted)
}

/// Resolve a treatment group when the deleted predecessor-edge set is complete
/// at group level but one or more members lack an operation-level deletion.
///
/// A member is admitted only through a recipe context that names one of the
/// deleted predecessor edges. The contextual candidate sets must have exactly
/// one perfect assignment; otherwise the group remains native. This handles a
/// legacy group whose transition records consolidate one member's deletion
/// while retaining the member-to-edge relation in the recipe references.
pub(crate) fn contextual_deleted_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
) -> Option<Vec<i64>> {
    if operands.is_empty() {
        return None;
    }
    let mut deleted = Vec::new();
    let mut has_deleted_member = false;
    for operand in operands {
        if operand.recipe_structure.is_none()
            || operand.recipe_references.is_empty()
            || operand.recipe_references.len() != operand.recipe_reference_contexts.len()
            || operand.recipe_reference_contexts.is_empty()
        {
            return None;
        }
        for edge in &operand.deleted_boundary_edge_slots {
            if !operand.preceding_boundary_edge_slots.contains(edge)
                || !operand.changed_boundary_edge_slots.contains(edge)
            {
                return None;
            }
            deleted.push(*edge);
            has_deleted_member = true;
        }
    }
    if !has_deleted_member {
        return None;
    }
    deleted.sort_unstable();
    deleted.dedup();
    if deleted.len() != operands.len() {
        return None;
    }

    let candidate_sets = operands
        .iter()
        .map(|operand| {
            let mut candidates = operand
                .recipe_reference_contexts
                .iter()
                .flat_map(|context| context.changed_reference_edge_slots.iter().copied())
                .filter(|edge| deleted.binary_search(edge).is_ok())
                .collect::<Vec<_>>();
            candidates.sort_unstable();
            candidates.dedup();
            candidates
        })
        .collect::<Vec<_>>();
    let mut assignment = unique_bipartite_assignment(&candidate_sets)?;
    assignment.sort_unstable();
    Some(assignment)
}

/// Resolve a legacy single-member treatment whose selected edge persists in
/// the result boundary instead of appearing in the operand boundary delta.
///
/// The zero-payload two-side recipe supplies support references but no direct
/// edge entry. A result-boundary edge named by at least two changed-reference
/// contexts is admitted only when it is the sole such edge and is absent from
/// the operand's preceding candidate boundary. This leaves deleted-edge and
/// ambiguous reference sets native.
pub(crate) fn result_boundary_reference_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
) -> Option<Vec<i64>> {
    let [operand] = operands else {
        return None;
    };
    let structure = operand.recipe_structure.as_ref()?;
    if structure.root != 2
        || structure.sides.len() != 2
        || structure.sides.iter().any(|side| {
            side.field_count.get() != 3
                || side.scalars.len() != 2
                || side.payload_prefix != [0]
                || side.payload_entry_count != 0
                || !side.entries.is_empty()
        })
        || operand.recipe_references.is_empty()
        || operand.recipe_references.len() != operand.recipe_reference_contexts.len()
        || operand.recipe_reference_contexts.is_empty()
        || !operand.changed_boundary_edge_slots.is_empty()
        || !operand.deleted_boundary_edge_slots.is_empty()
    {
        return None;
    }
    let mut candidates = operand
        .recipe_reference_contexts
        .iter()
        .flat_map(|context| context.changed_reference_edge_slots.iter().copied())
        .filter(|edge| operand.result_boundary_edge_slots.contains(edge))
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return None;
    };
    if operand.preceding_boundary_edge_slots.contains(candidate)
        || operand
            .recipe_reference_contexts
            .iter()
            .filter(|context| context.changed_reference_edge_slots.contains(candidate))
            .count()
            < 2
    {
        return None;
    }
    Some(vec![*candidate])
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
    if !operand.recipe_program.is_empty() && operand.recipe_structure.is_none() {
        return None;
    }
    operand
        .resolved_edge_slot
        .or_else(|| primary_terminal_reference_shared_edge(operand))
}

/// Resolve the selected edge of a zero-payload terminal recipe from the two
/// support-face references in its primary side.
fn primary_terminal_reference_shared_edge(operand: &DesignEdgeOperand) -> Option<i64> {
    if !operand.recipe_reference_contexts.is_empty() {
        return None;
    }
    let structure = operand.recipe_structure.as_ref()?;
    if structure.root != 2
        || structure.sides.len() != 2
        || structure.sides.iter().any(|side| {
            side.field_count.get() != 3
                || side.scalars.len() != 2
                || side.payload_prefix != [0]
                || side.payload_entry_count != 0
                || !side.entries.is_empty()
        })
    {
        return None;
    }

    let primary = &structure.sides[0];
    let reference_ordinals = std::iter::once(primary.header_value)
        .chain(primary.scalars.iter().copied())
        .filter(|value| *value != 0)
        .map(|value| usize::try_from(value).ok()?.checked_sub(1))
        .collect::<Option<Vec<_>>>()?;
    let [first_ordinal, second_ordinal] = reference_ordinals.as_slice() else {
        return None;
    };
    if first_ordinal == second_ordinal {
        return None;
    }
    let first = operand.terminal_reference_edge_slots.get(*first_ordinal)?;
    let second = operand.terminal_reference_edge_slots.get(*second_ordinal)?;
    if first.is_empty() || second.is_empty() {
        return None;
    }

    let mut candidates = first
        .iter()
        .copied()
        .filter(|candidate| second.contains(candidate))
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.dedup();
    match candidates.as_slice() {
        [candidate] if operand.terminal_boundary_edge_slots.contains(candidate) => Some(*candidate),
        _ => None,
    }
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

pub(crate) fn resolved_edge_candidate_intersection<'a>(
    selector_contexts: &[crate::records::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: impl IntoIterator<Item = &'a [i64]>,
) -> Option<i64> {
    resolved_edge_candidate_intersection_with_extra_proofs(
        selector_contexts,
        shared_edge_sets,
        [],
        None,
    )
}

pub(crate) fn unique_incidence_edge_shared_by_reference_faces<'a>(
    selector_contexts: &[crate::records::DesignEdgeRecipeSelectorContext],
    reference_edge_sets: impl IntoIterator<Item = &'a [i64]>,
) -> Option<i64> {
    let mut incidence = selector_contexts
        .iter()
        .flat_map(|selector| selector.incidence_matching_edge_slots.iter().copied())
        .collect::<Vec<_>>();
    incidence.sort_unstable();
    incidence.dedup();
    let mut reference_edge_sets = reference_edge_sets
        .into_iter()
        .map(|edges| {
            let mut edges = edges.to_vec();
            edges.sort_unstable();
            edges.dedup();
            edges
        })
        .collect::<Vec<_>>();
    reference_edge_sets.sort();
    reference_edge_sets.dedup();
    let mut candidates = incidence
        .into_iter()
        .filter(|edge| {
            reference_edge_sets
                .iter()
                .filter(|edges| edges.contains(edge))
                .take(2)
                .count()
                == 2
        })
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.dedup();
    match candidates.as_slice() {
        [edge] => Some(*edge),
        _ => None,
    }
}

fn resolved_edge_candidate_intersection_with_extra_proofs<'a, const N: usize>(
    selector_contexts: &[crate::records::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: impl IntoIterator<Item = &'a [i64]>,
    extra_proofs: [Option<i64>; N],
    disjoint_reference_proof: Option<i64>,
) -> Option<i64> {
    let extra_proofs = extra_proofs
        .into_iter()
        .flatten()
        .chain(disjoint_reference_proof)
        .collect::<Vec<_>>();
    let ordered_edge_sets = shared_edge_sets.into_iter().collect::<Vec<_>>();
    let shared_edge_sets = ordered_edge_sets
        .iter()
        .copied()
        .filter(|edges| !edges.is_empty())
        .collect::<Vec<_>>();
    let references_unavailable = !ordered_edge_sets.is_empty() && shared_edge_sets.is_empty();
    let reference_candidates =
        (shared_edge_sets.len() >= 2).then(|| edge_set_intersection(&shared_edge_sets));
    // Disjoint contextual face references do not name a common edge. An exact
    // recipe-clause/history proof remains independent of that context.
    if reference_candidates
        .as_deref()
        .is_some_and(<[i64]>::is_empty)
    {
        let edge = disjoint_reference_proof?;
        return extra_proofs
            .iter()
            .all(|proof| *proof == edge)
            .then_some(edge);
    }
    let reference = match reference_candidates.as_deref() {
        Some(&[edge]) => Some(edge),
        _ => None,
    };
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
    .flatten()
    .chain(extra_proofs)
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
    match candidates.as_slice() {
        [candidate] => Some(*candidate),
        _ => None,
    }
}

/// Edges every reference set shares, ascending and without repeats. An empty
/// result from a nonempty input is a disjoint reference set, which the caller
/// separates from a set that shares more than one edge.
fn edge_set_intersection(edge_sets: &[&[i64]]) -> Vec<i64> {
    let mut sets = edge_sets.iter();
    let Some(first) = sets.next() else {
        return Vec::new();
    };
    let mut candidates = first.to_vec();
    candidates.sort_unstable();
    candidates.dedup();
    for edge_set in sets {
        candidates.retain(|candidate| edge_set.contains(candidate));
        if candidates.is_empty() {
            break;
        }
    }
    candidates
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
    match candidates.as_slice() {
        [candidate] => Some(*candidate),
        _ => None,
    }
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

#[cfg(test)]
pub(crate) fn project_fixed_fillet(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    project_fixed_fillet_with_corners(
        scope,
        construction_groups,
        edge_operands,
        edge_identity_operands,
        &[],
        &[],
    )
}

pub(crate) fn project_fixed_fillet_with_corners(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    vertex_operands: &[DesignEdgeTreatmentVertexOperand],
    histories: &[crate::history_records::AsmHistory],
) -> Option<cadmpeg_ir::features::FeatureDefinition> {
    use cadmpeg_ir::features::{
        FeatureDefinition, FilletGroup, Length, RadiusSpec, VariableRadius,
    };

    let fixed = scope.fixed_fillet_parameters.as_ref()?;
    let stream = native_stream(&scope.id)?;
    let radius_spec = |group: &crate::records::DesignFixedFilletGroup| match group.radii.as_slice()
    {
        [radius] if *radius > 0.0 => Some(RadiusSpec::Constant {
            radius: Length(*radius * 10.0),
        }),
        [first, second, intermediate @ ..]
            if intermediate.len() == group.intermediate_parameters.len() =>
        {
            let mut points = Vec::with_capacity(intermediate.len() + 2);
            points.push(VariableRadius {
                parameter: 0.0,
                radius: Length(*first * 10.0),
            });
            points.extend(intermediate.iter().zip(&group.intermediate_parameters).map(
                |(radius, parameter)| VariableRadius {
                    parameter: *parameter,
                    radius: Length(*radius * 10.0),
                },
            ));
            points.push(VariableRadius {
                parameter: 1.0,
                radius: Length(*second * 10.0),
            });
            Some(RadiusSpec::Variable { points })
        }
        _ => None,
    };
    let mut scope_groups = construction_groups
        .iter()
        .filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && !group.members.is_empty()
        })
        .collect::<Vec<_>>();
    scope_groups.sort_by_key(|group| group.scope_reference_ordinal);
    let complete_edge_groups = scope_groups
        .iter()
        .copied()
        .filter(|group| {
            group.members.iter().all(|member| {
                edge_operands.iter().any(|operand| {
                    native_stream(&operand.id) == Some(stream)
                        && operand.scope_record_index == scope.record_index
                        && operand.record_index == *member
                })
            })
        })
        .collect::<Vec<_>>();
    let edge_groups = if complete_edge_groups.len() == fixed.groups.len() {
        complete_edge_groups
    } else if fixed.groups.len() == 1 && complete_edge_groups.is_empty() {
        let group = {
            // Support-face operands share the compact persistent-identity
            // prefix. A sole construction group cannot be a support group
            // alongside an unrepresented edge group: it is the Fillet's edge
            // selection when every member has one exact identity record and
            // the fixed radius selects a nonempty transition chain.
            let [group] = scope_groups.as_slice() else {
                return None;
            };
            let radius = radius_spec(&fixed.groups[0])?;
            let RadiusSpec::Constant { radius } = radius else {
                return None;
            };
            let identities = group
                .members
                .iter()
                .map(|member| {
                    let matches = edge_identity_operands
                        .iter()
                        .filter(|operand| {
                            native_stream(&operand.id) == Some(stream)
                                && operand.scope_record_index == scope.record_index
                                && operand.group_record_index == group.record_index
                                && operand.record_index == *member
                        })
                        .collect::<Vec<_>>();
                    let [operand] = matches.as_slice() else {
                        return None;
                    };
                    operand.compact_layout.then_some(*operand)
                })
                .collect::<Option<Vec<_>>>()?;
            radius_edge_identity_group_candidates(&identities, radius.0)?;
            *group
        };
        vec![group]
    } else {
        return None;
    };
    let groups = fixed
        .groups
        .iter()
        .zip(edge_groups)
        .map(|(fixed_group, edge_group)| {
            let radius = radius_spec(fixed_group)?;
            let edge_radius = match radius {
                RadiusSpec::Constant { radius } => Some(radius.0),
                RadiusSpec::Chordal { .. }
                | RadiusSpec::Asymmetric { .. }
                | RadiusSpec::Variable { .. }
                | RadiusSpec::Unresolved { .. } => None,
            };
            let edges = resolved_edge_treatment_group_with_corners(
                edge_group,
                construction_groups,
                edge_operands,
                edge_identity_operands,
                vertex_operands,
                histories,
                scope.previous_history_state_id,
                &neutral_feature_id(scope),
                edge_radius,
            );
            Some(FilletGroup {
                edges,
                radius,
                tangency_weight: fixed_group
                    .tangency_weight
                    .as_ref()
                    .map(|tangency| tangency.value),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(FeatureDefinition::Fillet { groups })
}

#[cfg(test)]
mod tests;
