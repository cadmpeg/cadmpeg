// SPDX-License-Identifier: Apache-2.0
//! Resolve edge-selection operands to stable edge identities.

use crate::ids::{self, native_stream, neutral_feature_id};
use crate::records::{
    feature::{scope::DesignParameterScope, work_geometry::DesignEdgeTreatmentVertexOperand},
    topology::{
        construction::DesignConstructionOperandGroup, edge_identity::DesignEdgeIdentityOperand,
        edge_identity::DesignEdgeOperand,
    },
};
use cadmpeg_core::decode::{alloc_filled, DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};

const EPS_EDGE_RESOLVE_RADIUS_EDGE_GROUP_CANDIDATES_E9: f64 = 1.0e-9;
const EPS_EDGE_RESOLVE_RADIUS_EDGE_IDENTITY_GROUP_CANDIDATES_E9: f64 = 1.0e-9;

macro_rules! or_none {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

fn push_edge_item<T>(
    ctx: Option<&DecodeContext<'_>>,
    items: &mut Vec<T>,
    item: T,
    operation: &'static str,
) -> Result<(), CodecError> {
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(1, operation)?;
        items.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    }
    items.push(item);
    Ok(())
}

fn sorted_transition_slots(
    slots: &[i64],
    ctx: Option<&DecodeContext<'_>>,
    operation: &'static str,
) -> Result<Vec<i64>, CodecError> {
    let mut sorted = Vec::new();
    for &slot in slots {
        push_edge_item(ctx, &mut sorted, slot, operation)?;
    }
    sorted.sort_unstable();
    sorted.dedup();
    Ok(sorted)
}

fn insert_edge_set<T: Eq + std::hash::Hash>(
    ctx: Option<&DecodeContext<'_>>,
    items: &mut HashSet<T>,
    item: T,
    operation: &'static str,
) -> Result<bool, CodecError> {
    if items.contains(&item) { return Ok(false); }
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(1, operation)?;
        items.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(operation, 0, 1))?;
    }
    Ok(items.insert(item))
}

fn copy_edge_text(
    ctx: Option<&DecodeContext<'_>>,
    value: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    let Some(ctx) = ctx else { return Ok(value.to_owned()); };
    String::from_utf8(ctx.copy_retained(value.as_bytes(), operation)?)
        .map_err(|_| CodecError::malformed("validated edge identity is not UTF-8"))
}

fn copy_edge_id(
    ctx: Option<&DecodeContext<'_>>,
    value: &cadmpeg_ir::ids::EdgeId,
    operation: &'static str,
) -> Result<cadmpeg_ir::ids::EdgeId, CodecError> {
    let text = copy_edge_text(ctx, value.as_str(), operation)?;
    cadmpeg_ir::ids::EdgeId::try_from(text).map_err(CodecError::malformed)
}

fn native_edge_selection(
    group: &DesignConstructionOperandGroup,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
    Ok(cadmpeg_ir::features::EdgeSelection::Native(copy_edge_text(ctx,
        &group.id, "f3d native edge group id")?))
}

pub(super) fn resolved_edge_group(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
    resolved_edge_group_with_transition_chain(
        group,
        groups,
        operands,
        identity_operands,
        EdgeGroupTransition {
            previous_state_id,
            feature_id,
        },
        EdgeGroupProof::Generic,
        None,
        ctx,
    )
}

/// Resolve a modern grouped `SurfacePatch` edge group from its exact
/// recipe references.
///
/// Every nonempty exact reference of one member must identify the same sole
/// edge, and the complete group must map to distinct edges. Conflicting exact
/// references suppress generic reconstruction because their serialized
/// member identity is unresolved.
pub(super) fn resolved_surface_patch_edge_group(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
    let fallback = || {
        resolved_edge_group(
            group,
            groups,
            operands,
            identity_operands,
            previous_state_id,
            feature_id,
            ctx,
        )
    };
    let stream = native_stream(&group.id);
    let mut member_ids = HashSet::new();
    for member in group.members() {
        if !insert_edge_set(ctx, &mut member_ids, member.value,
            "f3d surface patch edge member index")? {
            return fallback();
        }
    }
    let mut matched_operands = Vec::new();
    for member in group.members() {
            let mut matches = operands.iter().filter(|operand| {
                native_stream(&operand.id) == stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.record_index() == member.value
            });
            let Some(operand) = matches.next() else { return fallback(); };
            if matches.next().is_some() { return fallback(); }
            push_edge_item(ctx, &mut matched_operands, operand,
                "f3d surface patch matched edge operand")?;
    }
    let edges = match surface_patch_grouped_recipe_edges(&matched_operands, ctx)? {
        SurfacePatchRecipeEdges::Absent => return fallback(),
        SurfacePatchRecipeEdges::Inconclusive => {
            return Ok(cadmpeg_ir::features::EdgeSelection::Native(
                copy_edge_text(ctx, &group.id, "f3d surface patch native group id")?,
            ));
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
        return Ok(cadmpeg_ir::features::EdgeSelection::Edges(edges));
    };
    let mut edge_slots = Vec::new();
    for edge in &edges {
        let Some(slot) = stable_edge_slot(edge) else { return fallback(); };
        push_edge_item(ctx, &mut edge_slots, slot,
            "f3d surface patch stable edge slot")?;
    }
    let feature_key = feature_id.key();
    let mut historical_edges = Vec::new();
    for edge_slot in edge_slots {
        let id = ids::history_input_edge_id(
            &ids::history_input_prefix(&feature_key, state_id), edge_slot,
        );
        push_edge_item(ctx, &mut historical_edges, id,
            "f3d surface patch historical edge")?;
    }
    let native = copy_edge_text(ctx, &group.id,
        "f3d surface patch historical group id")?;
    let resolved = cadmpeg_ir::features::EdgeSelection::historical(
        feature_input_topology_id(feature_id, state_id),
        historical_edges, native,
    );
    Ok(match resolved {
        Ok(selection) => selection,
        Err(_) => cadmpeg_ir::features::EdgeSelection::Native(copy_edge_text(ctx,
            &group.id, "f3d surface patch fallback group id")?),
    })
}

#[derive(Debug, PartialEq)]
enum SurfacePatchRecipeEdges {
    Absent,
    Inconclusive,
    Resolved(Vec<cadmpeg_ir::ids::EdgeId>),
}

fn surface_patch_grouped_recipe_edges(
    operands: &[&DesignEdgeOperand],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<SurfacePatchRecipeEdges, CodecError> {
    if operands.is_empty() {
        return Ok(SurfacePatchRecipeEdges::Absent);
    }
    let mut edges = Vec::new();
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
            return Ok(SurfacePatchRecipeEdges::Inconclusive);
        };
        if exact.any(|reference| reference.candidate_edges.as_slice() != std::slice::from_ref(edge))
        {
            return Ok(SurfacePatchRecipeEdges::Inconclusive);
        }
        let copied = copy_edge_id(ctx, edge, "f3d surface patch recipe edge id")?;
        push_edge_item(ctx, &mut edges, copied,
            "f3d surface patch recipe edge")?;
    }
    if has_absent_member {
        return Ok(SurfacePatchRecipeEdges::Absent);
    }
    let mut distinct = HashSet::new();
    for edge in &edges {
        if !insert_edge_set(ctx, &mut distinct, edge.as_str(),
            "f3d surface patch distinct recipe edge")? {
            return Ok(SurfacePatchRecipeEdges::Inconclusive);
        }
    }
    Ok(SurfacePatchRecipeEdges::Resolved(edges))
}

fn stable_edge_slot(edge: &cadmpeg_ir::ids::EdgeId) -> Option<i64> {
    edge.as_str()
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
pub(super) fn resolved_edge_flange_group(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
    use cadmpeg_ir::features::EdgeSelection;

    let selection = resolved_edge_group(
        group,
        groups,
        operands,
        identity_operands,
        previous_state_id,
        feature_id,
        ctx,
    )?;
    if !matches!(selection, EdgeSelection::Native(_)) {
        return Ok(selection);
    }
    let Some(previous_state_id) = previous_state_id else {
        return Ok(selection);
    };
    let stream = native_stream(&group.id);
    let mut members = HashSet::new();
    let mut candidate_sets = Vec::new();
    for member in group.members() {
        if !insert_edge_set(ctx, &mut members, member.value,
            "f3d edge flange member index")? {
            return Ok(selection);
        }
            let mut matching = operands.iter().filter(|operand| {
                    native_stream(&operand.id) == stream
                        && operand.scope_record_index == group.scope_record_index
                        && operand.record_index() == member.value
                });
            let Some(operand) = matching.next() else { return Ok(selection); };
            if matching.next().is_some() { return Ok(selection); }
            let Some(candidate) = edge_flange_updated_edge_candidate(operand) else {
                return Ok(selection);
            };
            push_edge_item(ctx, &mut candidate_sets, candidate,
                "f3d edge flange candidate set")?;
    }
    let Some(edges) = unique_bipartite_assignment(&candidate_sets, ctx)? else {
        return Ok(selection);
    };
    if edges.is_empty() {
        return Ok(selection);
    }
    let feature_key = feature_id.key();
    let state = feature_input_topology_id(feature_id, previous_state_id);
    let mut historical_edges = Vec::new();
    for edge_slot in edges {
        let id = ids::history_input_edge_id(
            &ids::history_input_prefix(&feature_key, previous_state_id), edge_slot,
        );
        push_edge_item(ctx, &mut historical_edges, id,
            "f3d edge flange historical edge")?;
    }
    let native = copy_edge_text(ctx, &group.id,
        "f3d edge flange historical group id")?;
    let historical = EdgeSelection::historical(state, historical_edges, native);
    Ok(match historical {
        Ok(selection) => selection,
        Err(_) => EdgeSelection::Native(copy_edge_text(ctx, &group.id,
            "f3d edge flange fallback group id")?),
    })
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
pub(super) fn resolved_edge_treatment_group(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    treatment_radius: Option<f64>,
) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
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
        None,
    )
}

#[allow(clippy::too_many_arguments)] // The arguments are distinct native operand arenas and resolution context.
pub(super) fn resolved_edge_treatment_group_with_corners(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    vertex_operands: &[DesignEdgeTreatmentVertexOperand],
    histories: &[crate::history_records::AsmHistory],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    treatment_radius: Option<f64>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
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
            EdgeGroupTransition {
                previous_state_id,
                feature_id,
            },
            EdgeGroupProof::Treatment {
                radius: treatment_radius,
            },
            None,
            ctx,
        );
    }
    let mut corner_slots = Vec::new();
    let mut edge_members = Vec::new();
    for (ordinal, member) in group.members().iter().copied().enumerate() {
        let edge_count = operands
            .iter()
            .filter(|operand| {
                native_stream(&operand.id) == stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.record_index() == member.value
            })
            .count();
        let corner = u32::try_from(ordinal).ok().and_then(|ordinal| {
            let mut matches = vertex_operands.iter().filter(|operand| {
                native_stream(&operand.id) == stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == ordinal
                    && operand.recipe.record_index() == member.value
            });
            let corner = matches.next()?;
            matches.next().is_none().then_some(corner)
        });
        match (edge_count, corner) {
            (1, None) => {
                push_edge_item(ctx, &mut edge_members, member,
                    "f3d treatment edge member")?;
            }
            (0, Some(corner)) => {
                let Some(resolution) = corner.recipe.resolution else {
                    return native_edge_selection(group, ctx);
                };
                if Some(resolution.state_id) != previous_state_id {
                    return native_edge_selection(group, ctx);
                }
                push_edge_item(ctx, &mut corner_slots, resolution.vertex_slot(),
                    "f3d treatment corner slot")?;
            }
            _ => return native_edge_selection(group, ctx),
        }
    }
    if edge_members.is_empty() {
        return native_edge_selection(group, ctx);
    }
    let selection = resolved_edge_group_with_transition_chain(
        group,
        groups,
        operands,
        identity_operands,
        EdgeGroupTransition {
            previous_state_id,
            feature_id,
        },
        EdgeGroupProof::Treatment {
            radius: treatment_radius,
        },
        Some(&edge_members),
        ctx,
    )?;
    if corner_slots.is_empty() {
        return Ok(selection);
    }
    let EdgeSelection::Historical { edges, .. } = &selection else {
        return native_edge_selection(group, ctx);
    };
    let Some(state_id) = previous_state_id else {
        return native_edge_selection(group, ctx);
    };
    let mut states = histories
        .iter()
        .filter(|history| ids::same_native_occurrence(&history.id, &group.id))
        .flat_map(|history| &history.states)
        .filter(|state| state.state_id == state_id);
    let Some(topology) = states.next().and_then(|state| state.topology()) else {
        return native_edge_selection(group, ctx);
    };
    if states.next().is_some() {
        return native_edge_selection(group, ctx);
    }
    let mut endpoints = HashSet::new();
    for edge in edges {
        let Some(edge_slot) = edge.as_str().rsplit(':').next()
            .and_then(|slot| slot.parse::<i64>().ok()) else {
            return native_edge_selection(group, ctx);
        };
        let mut matches = topology.edge_vertices.iter()
            .filter(|edge| edge.edge == edge_slot);
        let Some(edge) = matches.next() else { return native_edge_selection(group, ctx); };
        if matches.next().is_some() { return native_edge_selection(group, ctx); }
        insert_edge_set(ctx, &mut endpoints, edge.start_vertex,
            "f3d treatment endpoint vertex")?;
        insert_edge_set(ctx, &mut endpoints, edge.end_vertex,
            "f3d treatment endpoint vertex")?;
    }
    if corner_slots.iter().all(|corner| endpoints.contains(corner)) {
        Ok(selection)
    } else {
        native_edge_selection(group, ctx)
    }
}

#[derive(Clone, Copy)]
enum EdgeGroupProof {
    /// Member-local recipe and persistent-identity proofs only.
    Generic,
    /// Fillet or Chamfer proofs that may consume an exact operation transition.
    Treatment { radius: Option<f64> },
}

#[derive(Clone, Copy)]
struct EdgeGroupTransition<'a> {
    previous_state_id: Option<i64>,
    feature_id: &'a cadmpeg_ir::features::FeatureId,
}

fn resolved_edge_group_with_transition_chain(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    transition: EdgeGroupTransition<'_>,
    proof: EdgeGroupProof,
    members_override: Option<&[crate::records::identity::Located<u32>]>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
    use cadmpeg_ir::features::EdgeSelection;

    let EdgeGroupTransition {
        previous_state_id,
        feature_id,
    } = transition;
    let members = members_override.unwrap_or_else(|| group.members());

    let (allow_edge_treatment_transition_chain, treatment_radius) = match proof {
        EdgeGroupProof::Generic => (false, None),
        EdgeGroupProof::Treatment { radius } => (true, radius),
    };

    let feature_key = feature_id.key();
    let unmatched_selection = |state_id: Option<i64>| -> Result<EdgeSelection, CodecError> {
        if group.lost_edge_references.is_empty() {
            native_edge_selection(group, ctx)
        } else {
            let Some(state_id) = state_id else { return Ok(EdgeSelection::Unresolved); };
            Ok(partial_historical_edge_selection(
                        group
                            .lost_edge_references
                            .iter()
                            .map(|identity| (identity.as_str(), None)),
                        state_id,
                        &feature_key,
                        feature_input_topology_id(feature_id, state_id),
                        &group.id,
                        ctx,
                    )?.unwrap_or(EdgeSelection::Unresolved))
        }
    };
    let stream = native_stream(&group.id);
    let has_surface_patch_operand = operands.iter().any(|operand| {
        native_stream(&operand.id) == stream
            && operand.scope_record_index == group.scope_record_index
            && members
                .iter()
                .any(|member| member.value == operand.record_index())
            && operand.surface_patch_recipe_structure.is_some()
    });
    if has_surface_patch_operand {
        let mut member_ids = HashSet::new();
        for member in members {
            if !insert_edge_set(ctx, &mut member_ids, member.value,
                "f3d generic surface patch member index")? {
                return unmatched_selection(previous_state_id);
            }
        }
        let mut matched_operands = Vec::new();
        for member in members.iter().map(|member| &member.value) {
                let mut matches = operands.iter().filter(|operand| {
                    native_stream(&operand.id) == stream
                        && operand.scope_record_index == group.scope_record_index
                        && operand.record_index() == *member
                });
                let Some(operand) = matches.next().filter(|_| matches.next().is_none()) else {
                    return unmatched_selection(previous_state_id);
                };
                push_edge_item(ctx, &mut matched_operands, operand,
                    "f3d generic surface patch matched operand")?;
        }
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
        let mut edges = Vec::new();
        for operand in &matched_operands {
            let Some(edge) = operand.resolved_edge_slot else {
                return unmatched_selection(Some(state_id));
            };
            push_edge_item(ctx, &mut edges, edge,
                "f3d generic surface patch resolved slot")?;
        }
        let mut resolved_edges = Vec::new();
        for edge_slot in edges {
            if !resolved_edges.contains(&edge_slot) {
                push_edge_item(ctx, &mut resolved_edges, edge_slot,
                    "f3d generic surface patch distinct slot")?;
            }
        }
        if resolved_edges.is_empty() {
            return unmatched_selection(Some(state_id));
        }
        return Ok(EdgeSelection::historical(
            feature_input_topology_id(feature_id, state_id),
            resolved_edges
                .into_iter()
                .map(|edge_slot| {
                    ids::history_input_edge_id(
                        &ids::history_input_prefix(&feature_key, state_id),
                        edge_slot,
                    )
                })
                .collect(),
            group.id.clone(),
        )
        .unwrap_or_else(|_| EdgeSelection::Native(group.id.clone())));
    }
    let mut identity_matches = Vec::new();
    let mut identities_complete = true;
    for member in members.iter().map(|member| &member.value) {
            let mut matches = identity_operands.iter().filter(|operand| {
                native_stream(&operand.id) == stream
                    && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index == group.record_index
                    && operand.record_index() == *member
            });
            let Some(operand) = matches.next().filter(|_| matches.next().is_none()) else {
                identities_complete = false;
                break;
            };
            push_edge_item(ctx, &mut identity_matches, operand,
                "f3d edge group matched identity")?;
    }
    let identity_matches = identities_complete.then_some(identity_matches);
    let has_recipe_operands = members
        .iter()
        .map(|member| &member.value)
        .all(|member| {
            let mut matches = operands
                .iter()
                .filter(|operand| {
                    native_stream(&operand.id) == stream
                        && operand.scope_record_index == group.scope_record_index
                        && operand.record_index() == *member
                });
            matches.next().is_some() && matches.next().is_none()
        });
    let has_unstructured_recipe_operand =
        members
            .iter()
            .map(|member| &member.value)
            .any(|member| {
                operands.iter().any(|operand| {
                    native_stream(&operand.id) == stream
                        && operand.scope_record_index == group.scope_record_index
                        && operand.record_index() == *member
                        && !operand.recipe_program.is_empty()
                        && operand.recipe_structure.is_none()
                })
            });
    if has_recipe_operands && has_unstructured_recipe_operand {
        return unmatched_selection(previous_state_id);
    }
    let has_standard_recipe_operands =
        members
            .iter()
            .map(|member| &member.value)
            .any(|member| {
                operands.iter().any(|operand| {
                    native_stream(&operand.id) == stream
                        && operand.scope_record_index == group.scope_record_index
                        && operand.record_index() == *member
                        && operand.recipe_structure.is_some()
                })
            });
    let has_concrete_recipe_evidence =
        members
            .iter()
            .map(|member| &member.value)
            .any(|member| {
                let mut matches = operands
                    .iter()
                    .filter(|operand| {
                        native_stream(&operand.id) == stream
                            && operand.scope_record_index == group.scope_record_index
                            && operand.record_index() == *member
                    });
                matches.next().is_some_and(|operand| {
                    matches.next().is_none()
                        && (operand.resolved_edge_slot.is_some()
                            || !operand.changed_boundary_edge_slots.is_empty()
                            || !operand.deleted_boundary_edge_slots.is_empty()
                            || !operand.treatment_radius_candidates.is_empty())
                })
            });
    let identity_transition_slots = if allow_edge_treatment_transition_chain
        && treatment_radius.is_none()
        && members.len() == 1
    {
        match identity_matches.as_deref() {
            Some([operand]) => {
                let edges = sorted_transition_slots(&operand.transition_edge_candidates, ctx,
                    "f3d single identity transition slot")?;
                (!edges.is_empty()).then_some(edges)
            }
            _ => None,
        }
    } else {
        None
    };
    let identity_group_transition_slots = if let Some(first) = identity_matches
        .as_ref()
        .and_then(|matches| matches.first())
    {
        let edges = sorted_transition_slots(&first.transition_edge_candidates, ctx,
            "f3d group identity transition slot")?;
        if allow_edge_treatment_transition_chain && !edges.is_empty() {
            let mut uniform = true;
            if let Some(matches) = identity_matches.as_ref() {
                for operand in matches {
                    if !operand.layout().is_compact() {
                        uniform = false;
                        break;
                    }
                    let candidate = sorted_transition_slots(&operand.transition_edge_candidates,
                        ctx, "f3d compared identity transition slot")?;
                    if candidate != edges {
                        uniform = false;
                        break;
                    }
                }
            }
            uniform.then_some(edges)
        } else {
            None
        }
    } else {
        None
    };
    let recipe_supports_transition_chain = |chain: &[i64]| -> Result<bool, CodecError> {
        let mut member_operands = Vec::new();
        for member in members.iter().map(|member| &member.value) {
                let mut matches = operands.iter().filter(|operand| {
                    native_stream(&operand.id) == stream
                        && operand.scope_record_index == group.scope_record_index
                        && operand.record_index() == *member
                });
                let Some(operand) = matches.next().filter(|_| matches.next().is_none()) else {
                    return Ok(false);
                };
                push_edge_item(ctx, &mut member_operands, operand,
                    "f3d transition recipe member")?;
        }
        transition_chain_is_supported_by_recipe(chain, members.len(), &member_operands, ctx)
    };
    let identity_transition_is_supported = match identity_transition_slots
        .as_deref()
        .or(identity_group_transition_slots.as_deref())
    {
        Some(chain) => recipe_supports_transition_chain(chain)?,
        None => false,
    };
    // A cardinality mismatch is a group-level transition proof, not a
    // member-to-edge assignment. Recipe evidence must therefore cover the
    // complete chain before it can replace the unresolved member identities.
    let identity_group_transition_is_admitted = match identity_group_transition_slots.as_deref() {
        Some(edges) => edges.len() == members.len() || recipe_supports_transition_chain(edges)?,
        None => false,
    };
    let identity_radius_slots = match (treatment_radius, identity_matches.as_ref()) {
        (Some(radius), Some(matches)) => radius_edge_identity_group_candidates(matches, radius, ctx)?,
        _ => None,
    };
    let has_complete_identity_selection = identity_matches.as_ref().is_some_and(|operands| {
        !operands.is_empty()
            && (operands.iter().all(|operand| {
                operand.resolved_edge_slot.is_some() || !operand.resolved_edge_slots.is_empty()
            }) || identity_transition_slots.is_some()
                || identity_group_transition_is_admitted
                || identity_radius_slots.is_some())
    });
    let all_member_identities_are_lost =
        !members.is_empty() && group.lost_edge_references.len() == members.len();
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
                        &ids::history_input_prefix(&feature_key, previous_state_id),
                        edge_slot,
                    )
                })
                .collect();
            return Ok(EdgeSelection::historical(state, edges, group.id.clone())
                .unwrap_or_else(|_| EdgeSelection::Native(group.id.clone())));
        }
        if let Some(edges) = identity_radius_slots.as_ref() {
            return Ok(EdgeSelection::historical(
                state,
                edges
                    .iter()
                    .map(|edge_slot| {
                        ids::history_input_edge_id(
                            &ids::history_input_prefix(&feature_key, previous_state_id),
                            *edge_slot,
                        )
                    })
                    .collect(),
                group.id.clone(),
            )
            .unwrap_or_else(|_| EdgeSelection::Native(group.id.clone())));
        }
        if let Some(edges) = identity_group_transition_slots.as_ref() {
            return Ok(EdgeSelection::historical(
                state,
                edges
                    .iter()
                    .map(|edge_slot| {
                        ids::history_input_edge_id(
                            &ids::history_input_prefix(&feature_key, previous_state_id),
                            *edge_slot,
                        )
                    })
                    .collect(),
                group.id.clone(),
            )
            .unwrap_or_else(|_| EdgeSelection::Native(group.id.clone())));
        }
        if identity_matches.len() == 1 && identity_matches[0].resolved_edge_slot.is_none() {
            if let Some(edges) = identity_transition_slots.as_ref() {
                return Ok(EdgeSelection::historical(
                    state,
                    edges
                        .iter()
                        .map(|edge_slot| {
                            ids::history_input_edge_id(
                                &ids::history_input_prefix(&feature_key, previous_state_id),
                                edge_slot,
                            )
                        })
                        .collect(),
                    group.id.clone(),
                )
                .unwrap_or_else(|_| EdgeSelection::Native(group.id.clone())));
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
                        &ids::history_input_prefix(&feature_key, previous_state_id),
                        edge_slot,
                    )
                })
                .collect();
            return Ok(EdgeSelection::historical(state, edges, group.id.clone())
                .unwrap_or_else(|_| EdgeSelection::Native(group.id.clone())));
        }
        return match partial_historical_edge_selection(
            members,
            previous_state_id,
            &feature_key,
            state,
            &group.id,
            ctx,
        )? {
            Some(selection) => Ok(selection),
            None => native_edge_selection(group, ctx),
        };
    }
    let mut matched_operands = Vec::new();
    let mut member_identities = HashSet::new();
    for member in members.iter().map(|member| &member.value) {
        if !insert_edge_set(ctx, &mut member_identities, *member,
            "f3d edge group member identity")? {
            return unmatched_selection(previous_state_id);
        }
        let mut matches = operands.iter().filter(|operand| {
            native_stream(&operand.id) == stream
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index() == *member
        });
        let Some(operand) = matches.next() else {
            return unmatched_selection(previous_state_id);
        };
        if matches.next().is_some() {
            return unmatched_selection(previous_state_id);
        }
        push_edge_item(ctx, &mut matched_operands, operand,
            "f3d matched edge group operand")?;
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
        return Ok(if group.lost_edge_references.is_empty() {
            EdgeSelection::Native(group.id.clone())
        } else {
            EdgeSelection::Unresolved
        });
    };
    let state = feature_input_topology_id(feature_id, previous_state_id);
    let lost_selection = || unmatched_selection(Some(previous_state_id));
    let mut exact_slots = Some(Vec::new());
    for operand in &matched_operands {
        let Some(slot) = resolved_edge_operand(operand) else {
            exact_slots = None;
            break;
        };
        if let Some(slots) = exact_slots.as_mut() {
            push_edge_item(ctx, slots, slot, "f3d exact edge group slot")?;
        }
    }
    if exact_slots.is_none() {
        exact_slots = unique_edge_group_assignment(&matched_operands, ctx)?;
    }
    if exact_slots.is_none() {
        exact_slots = changed_reference_edge_group_candidates(&matched_operands, ctx)?;
    }
    let transition_slots = || -> Result<Option<Vec<i64>>, CodecError> {
        let mut slots = match treatment_radius {
            Some(radius) => radius_edge_group_candidates(&matched_operands, radius, ctx)?,
            None => None,
        };
        if slots.is_none() {
            slots = match (treatment_radius, identity_matches.as_ref()) {
                (Some(radius), Some(operands)) =>
                    radius_edge_identity_group_candidates(operands, radius, ctx)?,
                _ => None,
            };
        }
        if slots.is_none() {
            slots =
                context_only_edge_group_candidates(matched_operands.iter().map(|operand| {
                    (
                        resolved_edge_operand(operand),
                        operand.changed_boundary_edge_slots.as_slice(),
                    )
                }), ctx)?;
        }
        if slots.is_none() {
            slots =
                changed_boundary_count_edge_group_candidates(
                    matched_operands
                        .iter()
                        .map(|operand| operand.recipe_selectors.as_slice()),
                    ctx,
                )?;
        }
        if slots.is_none() {
            slots = deleted_reference_edge_group_candidates(&matched_operands, ctx)?;
        }
        if slots.is_none() {
            slots = common_deleted_edge_group_candidates(matched_operands.iter().map(|operand| {
                (
                    !operand.changed_boundary_edge_slots.is_empty(),
                    operand.deleted_boundary_edge_slots.as_slice(),
                )
            }), ctx)?;
        }
        if slots.is_none() && allow_edge_treatment_transition_chain {
            slots = contextual_deleted_edge_group_candidates(&matched_operands, ctx)?;
        }
        if slots.is_none() && allow_edge_treatment_transition_chain {
            slots = result_boundary_reference_edge_group_candidates(&matched_operands, ctx)?;
        }
        if slots.is_none() && allow_edge_treatment_transition_chain {
            slots = deleted_boundary_edge_group_candidates(&matched_operands, ctx)?;
        }
        if slots.is_none() {
            slots = scope_partition_edge_group_candidates(group, groups, operands, members, ctx)?;
        }
        Ok(slots)
    };
    let resolved_slots =
        if exact_slots.is_some() || has_standard_recipe_operands || transition_state_id.is_none() {
            exact_slots
        } else {
            transition_slots()?
        };
    let Some(resolved_slots) = resolved_slots else {
        if !group.lost_edge_references.is_empty() {
            return lost_selection();
        }
        if has_standard_recipe_operands {
            return Ok(EdgeSelection::Native(group.id.clone()));
        }
        let mut combined_edges = Vec::new();
        for (index, operand) in matched_operands.iter().enumerate() {
            let recipe = resolved_edge_operand(operand);
            let identity = identity_matches
                .as_ref()
                .and_then(|identities| identities[index].resolved_edge_slot);
            let combined = match (recipe, identity) {
                (Some(recipe), Some(identity)) if recipe != identity => {
                    return unmatched_selection(Some(previous_state_id));
                }
                (recipe, identity) => recipe.or(identity),
            };
            push_edge_item(ctx, &mut combined_edges, combined,
                "f3d combined edge group slot")?;
        }
        if combined_edges.iter().all(Option::is_some) {
            let mut edges = Vec::new();
            for edge_slot in combined_edges.into_iter().flatten() {
                let edge = ids::history_input_edge_id(
                    &ids::history_input_prefix(&feature_key, previous_state_id),
                    edge_slot,
                );
                if !edges.contains(&edge) {
                    edges.push(edge);
                }
            }
            return Ok(EdgeSelection::historical(state, edges, group.id.clone())
                .unwrap_or_else(|_| EdgeSelection::Native(group.id.clone())));
        }
        let mut partial_members = Vec::new();
        for (operand, resolved) in matched_operands.iter().zip(combined_edges) {
                let carries_transition_evidence = identity_matches.is_some()
                    || transition_state_id.is_none()
                    || !operand.changed_boundary_edge_slots.is_empty();
                if resolved.is_some() || carries_transition_evidence {
                    push_edge_item(ctx, &mut partial_members,
                        (operand.id.as_str(), resolved), "f3d partial edge group member")?;
                }
        }
        return match partial_historical_edge_selection(
            partial_members,
            previous_state_id,
            &feature_key,
            state,
            &group.id,
            ctx,
        )? {
            Some(selection) => Ok(selection),
            None => native_edge_selection(group, ctx),
        };
    };
    let mut edges = Vec::new();
    for edge_slot in resolved_slots {
        let edge = ids::history_input_edge_id(
            &ids::history_input_prefix(&feature_key, previous_state_id),
            edge_slot,
        );
        if !edges.contains(&edge) {
            edges.push(edge);
        }
    }
    Ok(if edges.is_empty() {
        EdgeSelection::Native(group.id.clone())
    } else {
        EdgeSelection::historical(state, edges, group.id.clone())
            .unwrap_or_else(|_| EdgeSelection::Native(group.id.clone()))
    })
}

pub(super) fn resolved_hem_edge_group(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
    use cadmpeg_ir::features::EdgeSelection;

    let selection = resolved_edge_group(
        group,
        groups,
        operands,
        identity_operands,
        previous_state_id,
        feature_id,
        ctx,
    )?;
    if !matches!(selection, EdgeSelection::Native(_)) {
        return Ok(selection);
    }
    let Some(previous_state_id) = previous_state_id else {
        return Ok(selection);
    };
    let [crate::records::identity::Located { value: member, .. }] = group.members() else {
        return Ok(selection);
    };
    let mut matching_operands = operands.iter().filter(|operand| {
            native_stream(&operand.id) == native_stream(&group.id)
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index() == *member
        });
    let Some(operand) = matching_operands.next().filter(|_| matching_operands.next().is_none()) else {
        return Ok(selection);
    };
    let Some(edge) = hem_transition_edge_slot(operand, ctx)? else {
        return Ok(selection);
    };
    let feature_key = feature_id.key();
    Ok(EdgeSelection::historical(
        feature_input_topology_id(feature_id, previous_state_id),
        vec![ids::history_input_edge_id(
            &ids::history_input_prefix(&feature_key, previous_state_id),
            edge,
        )],
        copy_edge_text(ctx, &group.id, "f3d hem historical group id")?,
    )
    .unwrap_or_else(|_| selection))
}

/// Return the one historical edge a single-member Hem operand identifies.
///
/// A directly resolved operand is preferred. The transition proof is the
/// fallback used by the compact recipe form, where the operand carries only
/// the changed support boundaries and the selectorless edge context.
pub(super) fn resolved_hem_edge_slot(
    operand: &DesignEdgeOperand,
    previous_state_id: Option<i64>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<i64>, CodecError> {
    if let Some(edge) = operand.resolved_edge_slot {
        return Ok(Some(edge));
    }
    if previous_state_id.is_none() { return Ok(None); }
    hem_transition_edge_slot(operand, ctx)
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
    operands: &[&'a DesignEdgeOperand],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<bool, CodecError> {
    if operands.len() != member_count {
        return Ok(false);
    }
    let mut all_recipe_edges = Vec::new();
    for operand in operands {
        let resolved = resolved_edge_operand(operand);
        if let Some(edge) = resolved {
            if operand.deleted_boundary_edge_slots.contains(&edge) && !chain.contains(&edge) {
                return Ok(false);
            }
        }
        if !operand.deleted_boundary_edge_slots.is_empty()
            && !operand
                .deleted_boundary_edge_slots
                .iter()
                .any(|edge| chain.contains(edge))
        {
            return Ok(false);
        }
        for edge in operand.changed_boundary_edge_slots.iter()
            .chain(&operand.deleted_boundary_edge_slots).copied()
            .chain(edge_operand_reference_edge_sets(operand).flatten().copied()) {
            push_edge_item(ctx, &mut all_recipe_edges, edge,
                "f3d transition recipe edge")?;
        }
    }
    all_recipe_edges.sort_unstable();
    all_recipe_edges.dedup();
    Ok(all_recipe_edges.is_empty() || chain.iter().all(|edge| all_recipe_edges.contains(edge)))
}

fn hem_transition_edge_slot(
    operand: &DesignEdgeOperand,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<i64>, CodecError> {
    let reference_contexts = operand.recipe_reference_contexts.as_slice();
    let mut empty_contexts = reference_contexts.iter()
        .filter(|context| context.changed_reference_edge_slots.is_empty());
    let Some(empty_context) = empty_contexts.next().filter(|_| empty_contexts.next().is_none()) else {
        return Ok(None);
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
        return Ok(None);
    }
    unique_hem_transition_edge_candidate(
        &operand.changed_boundary_edge_slots,
        reference_contexts
            .iter()
            .map(|context| context.changed_reference_edge_slots.as_slice()),
        ctx,
    )
}

fn unique_hem_transition_edge_candidate<'a>(
    changed_boundary_edges: &[i64],
    reference_edge_sets_input: impl IntoIterator<Item = &'a [i64]>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<i64>, CodecError> {
    let mut reference_edge_sets: Vec<&'a [i64]> = Vec::new();
    for edges in reference_edge_sets_input {
        push_edge_item(ctx, &mut reference_edge_sets, edges,
            "f3d hem reference edge set")?;
    }
    if reference_edge_sets.is_empty()
        || reference_edge_sets
            .iter()
            .filter(|edges| edges.is_empty())
            .count()
            != 1
    {
        return Ok(None);
    }
    let mut support_edges = Vec::new();
    for edge in reference_edge_sets.iter().flat_map(|edges| edges.iter().copied()) {
        push_edge_item(ctx, &mut support_edges, edge, "f3d hem support edge")?;
    }
    support_edges.sort_unstable();
    support_edges.dedup();
    if support_edges.is_empty()
        || support_edges
            .iter()
            .any(|edge| !changed_boundary_edges.contains(edge))
    {
        return Ok(None);
    }
    let mut candidates = Vec::new();
    for edge in changed_boundary_edges.iter().copied()
        .filter(|edge| !support_edges.contains(edge)) {
        push_edge_item(ctx, &mut candidates, edge, "f3d hem candidate edge")?;
    }
    candidates.sort_unstable();
    candidates.dedup();
    match candidates.as_slice() {
        [edge] => Ok(Some(*edge)),
        _ => Ok(None),
    }
}

fn partial_historical_edge_selection<'a>(
    members: impl IntoIterator<Item = (&'a str, Option<i64>)>,
    previous_state_id: i64,
    feature_key: &cadmpeg_ir::ids::IdentityKey,
    state: cadmpeg_ir::ids::FeatureInputTopologyId,
    native: &str,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<cadmpeg_ir::features::EdgeSelection>, CodecError> {
    use cadmpeg_ir::features::EdgeSelection;

    let mut edges = Vec::new();
    let mut unresolved = Vec::new();
    for (identity, edge) in members {
        if let Some(edge) = edge {
            if !edges.contains(&edge) {
                push_edge_item(ctx, &mut edges, edge, "f3d partial edge slot")?;
            }
        } else {
            let id = copy_edge_text(ctx, identity, "f3d partial unresolved id")?;
            push_edge_item(ctx, &mut unresolved, id, "f3d partial unresolved member")?;
        }
    }
    if unresolved.is_empty() || edges.is_empty() {
        return Ok(None);
    }
    let mut historical_edges = Vec::new();
    for edge_slot in edges {
        let edge = ids::history_input_edge_id(
            &ids::history_input_prefix(feature_key, previous_state_id), edge_slot);
        push_edge_item(ctx, &mut historical_edges, edge, "f3d partial historical edge")?;
    }
    let native_id = copy_edge_text(ctx, native, "f3d partial native id")?;
    Ok(Some(
        match EdgeSelection::historical_partial(
            state,
            historical_edges,
            unresolved,
            native_id,
        ) {
            Ok(selection) => selection,
            Err(_) => EdgeSelection::Native(copy_edge_text(ctx, native,
                "f3d partial native fallback id")?),
        },
    ))
}

fn context_only_edge_group_candidates<'a>(
    members: impl IntoIterator<Item = (Option<i64>, &'a [i64])>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let mut edges = Vec::new();
    for (resolved, changed_candidates) in members {
        match resolved {
            Some(edge) => {
                if !edges.contains(&edge) {
                    push_edge_item(ctx, &mut edges, edge,
                        "f3d context-only edge candidate")?;
                }
            }
            None if changed_candidates.is_empty() => {}
            None => return Ok(None),
        }
    }
    Ok((!edges.is_empty()).then_some(edges))
}

pub(crate) fn feature_input_topology_id(
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
) -> cadmpeg_ir::ids::FeatureInputTopologyId {
    let feature_key = feature_id.key();
    ids::history_input_state_id(&ids::history_input_prefix(&feature_key, previous_state_id))
}

fn unique_edge_group_assignment(
    operands: &[&DesignEdgeOperand],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if operands.is_empty() {
        return Ok(None);
    }
    let mut candidate_sets = Vec::new();
    for operand in operands {
        let candidates = if let Some(edge) = resolved_edge_operand(operand) {
            Some(EdgeAssignmentCandidates::Edges(vec![edge]))
        } else {
            edge_group_assignment_candidates(
                    &operand.recipe_selectors,
                    edge_operand_reference_edge_sets(operand),
                )
        };
        let Some(candidates) = candidates else { return Ok(None); };
        push_edge_item(ctx, &mut candidate_sets, candidates,
            "f3d unique edge candidate set")?;
    }
    unique_edge_assignment_with_context(&candidate_sets, ctx)
}

pub(super) fn changed_reference_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let mut candidate_sets = Vec::new();
    for operand in operands {
        let mut changed_sets = operand.recipe_reference_contexts.iter()
            .map(|context| context.changed_reference_edge_slots.as_slice())
            .filter(|edges| !edges.is_empty());
        let Some(first) = changed_sets.next() else { return Ok(None); };
        let mut candidates = Vec::new();
        for edge in first {
            push_edge_item(ctx, &mut candidates, *edge, "f3d changed reference candidate")?;
        }
        for changed in changed_sets {
            candidates.retain(|candidate| changed.contains(candidate));
        }
        candidates.sort_unstable();
        candidates.dedup();
        if candidates.is_empty() { return Ok(None); }
        push_edge_item(ctx, &mut candidate_sets, candidates,
            "f3d changed reference candidate set")?;
    }
    unique_bipartite_assignment(&candidate_sets, ctx)
}

fn deleted_reference_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let mut reference_candidates = Vec::new();
    let mut deleted_candidates = Vec::new();
    for operand in operands {
        let mut candidates = Vec::new();
        for edge in operand.recipe_reference_contexts.iter()
            .flat_map(|context| context.changed_reference_edge_slots.iter().copied()) {
            push_edge_item(ctx, &mut candidates, edge,
                "f3d deleted reference candidate")?;
        }
        candidates.sort_unstable();
        candidates.dedup();
        push_edge_item(ctx, &mut reference_candidates, candidates,
            "f3d deleted reference candidate set")?;
        let mut deleted = Vec::new();
        for edge in &operand.deleted_boundary_edge_slots {
            push_edge_item(ctx, &mut deleted, *edge,
                "f3d deleted reference boundary edge")?;
        }
        push_edge_item(ctx, &mut deleted_candidates, deleted,
            "f3d deleted reference boundary set")?;
    }
    unique_deleted_reference_assignment(&reference_candidates, &deleted_candidates, ctx)
}

fn unique_deleted_reference_assignment(
    reference_candidates: &[Vec<i64>],
    deleted_candidates: &[Vec<i64>],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if reference_candidates.len() != deleted_candidates.len() {
        return Ok(None);
    }
    let mut candidate_sets = Vec::new();
    for (references, deleted) in reference_candidates.iter().zip(deleted_candidates) {
        let mut candidates = Vec::new();
        for edge in references.iter().copied().filter(|edge| deleted.contains(edge)) {
            push_edge_item(ctx, &mut candidates, edge,
                "f3d deleted reference shared edge")?;
        }
        candidates.sort_unstable();
        candidates.dedup();
        if candidates.is_empty() { return Ok(None); }
        push_edge_item(ctx, &mut candidate_sets, candidates,
            "f3d deleted reference shared set")?;
    }
    unique_bipartite_assignment(&candidate_sets, ctx)
}

#[derive(Debug, PartialEq)]
enum EdgeAssignmentCandidates {
    Context,
    Edges(Vec<i64>),
}

// `None` means the record claims an edge operand but its proofs do not admit a
// candidate. `Context` means the recipe has no edge-assignment proof and the
// record only contributes topology context to its neighboring operands.
fn edge_group_assignment_candidates<'a>(
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
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

pub(super) fn radius_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
    radius: f64,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if operands.is_empty() || !radius.is_finite() || radius <= 0.0 {
        return Ok(None);
    }
    let tolerance = EPS_EDGE_RESOLVE_RADIUS_EDGE_GROUP_CANDIDATES_E9 * (1.0 + radius.abs());
    let mut chain = Vec::new();
    for operand in operands {
        if let Some(edge) = resolved_edge_operand(operand) {
            push_edge_item(ctx, &mut chain, edge, "f3d radius recipe edge")?;
        }
        for edge in operand
                .treatment_radius_candidates
                .iter()
                .filter(|candidate| (candidate.radius.get() - radius).abs() <= tolerance)
                .map(|candidate| candidate.edge_slot) {
            push_edge_item(ctx, &mut chain, edge, "f3d radius candidate edge")?;
        }
    }
    chain.sort_unstable();
    chain.dedup();
    if chain.is_empty() {
        return Ok(None);
    }
    for operand in operands {
        let has_radius_candidate = operand
            .treatment_radius_candidates
            .iter()
            .any(|candidate| (candidate.radius.get() - radius).abs() <= tolerance);
        if resolved_edge_operand(operand).is_none()
            && !has_radius_candidate
            && !operand.changed_boundary_edge_slots.is_empty()
        {
            return Ok(None);
        }
    }
    Ok(Some(chain))
}

fn radius_edge_identity_group_candidates(
    operands: &[&DesignEdgeIdentityOperand],
    radius: f64,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if operands.is_empty() || !radius.is_finite() || radius <= 0.0 {
        return Ok(None);
    }
    let tolerance =
        EPS_EDGE_RESOLVE_RADIUS_EDGE_IDENTITY_GROUP_CANDIDATES_E9 * (1.0 + radius.abs());
    // Radius candidates on identity operands describe the complete operation
    // transition, not one member. They prove a group only when that group has
    // one member. A multi-member group requires an exact contribution from
    // every identity; recipe-local radius candidates are handled separately.
    let use_transition_radius = operands.len() == 1;
    let mut chain = Vec::new();
    for operand in operands {
        let mut contribution = Vec::new();
        for edge in operand
            .resolved_edge_slot
            .iter()
            .copied()
            .chain(operand.resolved_edge_slots.iter().copied())
        {
            push_edge_item(ctx, &mut contribution, edge,
                "f3d radius identity resolved edge")?;
        }
        if use_transition_radius {
            for edge in operand
                    .treatment_radius_candidates
                    .iter()
                    .filter(|candidate| (candidate.radius.get() - radius).abs() <= tolerance)
                    .map(|candidate| candidate.edge_slot) {
                push_edge_item(ctx, &mut contribution, edge,
                    "f3d radius identity candidate edge")?;
            }
        }
        if contribution.is_empty() {
            return Ok(None);
        }
        for edge in contribution {
            push_edge_item(ctx, &mut chain, edge,
                "f3d radius identity chain edge")?;
        }
    }
    chain.sort_unstable();
    chain.dedup();
    Ok((!chain.is_empty()).then_some(chain))
}

fn unique_edge_assignment_with_context(
    candidate_sets: &[EdgeAssignmentCandidates],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let mut edge_candidate_sets = Vec::new();
    for candidates in candidate_sets {
        let EdgeAssignmentCandidates::Edges(edges) = candidates else { continue; };
        let mut copied_edges = Vec::new();
        for edge in edges {
            push_edge_item(ctx, &mut copied_edges, *edge,
                "f3d unique edge assignment candidate")?;
        }
        push_edge_item(ctx, &mut edge_candidate_sets, copied_edges,
            "f3d unique edge assignment set")?;
    }
    unique_bipartite_assignment(&edge_candidate_sets, ctx)
}

fn edge_assignment_candidates<'a>(
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: impl IntoIterator<Item = &'a [i64]>,
) -> Option<Vec<i64>> {
    let shared_edge_sets = shared_edge_sets.into_iter().collect::<Vec<_>>();
    if !selector_contexts.is_empty()
        && selector_contexts
            .iter()
            .all(|selector| !selector.incidence_matching_edge_slots.is_empty())
    {
        corroborated_edge_candidates(
            selector_contexts,
            shared_edge_sets.iter().copied(),
            SelectorSlots::Incidence,
        )
    } else {
        corroborated_edge_candidates(
            selector_contexts,
            shared_edge_sets.iter().copied(),
            SelectorSlots::BoundaryCount,
        )
    }
}

fn unique_bipartite_assignment(
    candidate_sets: &[Vec<i64>],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if candidate_sets.is_empty() {
        return Ok(None);
    }
    let mut normalized = match ctx {
        Some(ctx) => ctx.alloc_filled(
            candidate_sets.len(),
            Vec::<i64>::new(),
            "f3d edge normalized groups",
        )?,
        None => alloc_filled(
            candidate_sets.len(),
            Vec::<i64>::new(),
            "f3d edge normalized groups",
        )?,
    };
    for (source, candidates) in candidate_sets.iter().zip(&mut normalized) {
        *candidates = match ctx {
            Some(ctx) => ctx.alloc_filled(source.len(), 0, "f3d edge normalized candidates")?,
            None => alloc_filled(source.len(), 0, "f3d edge normalized candidates")?,
        };
        candidates.copy_from_slice(source);
        candidates.sort_unstable();
        candidates.dedup();
        if candidates.is_empty() {
            return Ok(None);
        }
    }
    let Some(assignment) = bipartite_assignment(&normalized, None, ctx)? else {
        return Ok(None);
    };
    for (member, edge) in assignment.iter().copied().enumerate() {
        if bipartite_assignment(&normalized, Some((member, edge)), ctx)?.is_some() {
            return Ok(None);
        }
    }
    Ok(Some(assignment))
}

fn bipartite_assignment(
    candidate_sets: &[Vec<i64>],
    forbidden: Option<(usize, i64)>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<Vec<i64>>, CodecError> {
    fn augment(
        member: usize,
        candidate_sets: &[Vec<i64>],
        forbidden: Option<(usize, i64)>,
        visited: &mut HashSet<i64>,
        edge_members: &mut HashMap<i64, usize>,
        visited_reservation: &mut Option<ScopedReservation<'_>>,
        members_reservation: &mut Option<ScopedReservation<'_>>,
        ctx: Option<&DecodeContext<'_>>,
    ) -> Result<bool, CodecError> {
        let _depth = ctx
            .map(|ctx| ctx.enter_nested("f3d edge assignment search"))
            .transpose()?;
        for edge in &candidate_sets[member] {
            if let Some(ctx) = ctx {
                ctx.charge_work(1, "f3d edge assignment candidate")?;
            }
            if forbidden == Some((member, *edge)) || visited.contains(edge) {
                continue;
            }
            if let Some(reservation) = visited_reservation {
                reservation.grow(u64::from(i64::BITS / 8))?;
            }
            if let Some(ctx) = ctx {
                visited.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("f3d edge assignment visit allocation", 0, 1)
                })?;
            }
            visited.insert(*edge);
            let displaced = edge_members.get(edge).copied();
            let assignable = match displaced {
                Some(displaced) => augment(
                    displaced,
                    candidate_sets,
                    forbidden,
                    visited,
                    edge_members,
                    visited_reservation,
                    members_reservation,
                    ctx,
                )?,
                None => true,
            };
            if assignable {
                if displaced.is_none() {
                    if let Some(ctx) = ctx {
                        let bytes = u64::try_from(std::mem::size_of::<(i64, usize)>()).map_err(
                            |_| ctx.refuse_codec_limit("f3d edge assignment member bytes", 0, 1),
                        )?;
                        if let Some(reservation) = members_reservation {
                            reservation.grow(bytes)?;
                        }
                        edge_members.try_reserve(1).map_err(|_| {
                            ctx.refuse_codec_limit("f3d edge assignment member allocation", 0, 1)
                        })?;
                    }
                }
                edge_members.insert(*edge, member);
                return Ok(true);
            }
        }
        Ok(false)
    }

    let mut edge_members = HashMap::new();
    let mut members_reservation = ctx
        .map(|ctx| ctx.reserve_scoped(0, "f3d edge assignment members"))
        .transpose()?;
    for member in 0..candidate_sets.len() {
        let mut visited = HashSet::new();
        let mut visited_reservation = ctx
            .map(|ctx| ctx.reserve_scoped(0, "f3d edge assignment visits"))
            .transpose()?;
        if !augment(
            member,
            candidate_sets,
            forbidden,
            &mut visited,
            &mut edge_members,
            &mut visited_reservation,
            &mut members_reservation,
            ctx,
        )? {
            return Ok(None);
        }
    }
    let mut assignment = match ctx {
        Some(ctx) => ctx.alloc_filled(candidate_sets.len(), 0, "f3d edge assignment")?,
        None => alloc_filled(candidate_sets.len(), 0, "f3d edge assignment")?,
    };
    for (edge, member) in edge_members {
        assignment[member] = edge;
    }
    Ok(Some(assignment))
}

/// Members of one construction operand group: `(identity, resolved edge slot,
/// deleted boundary edge slots)`.
#[derive(Clone)]
struct EdgeGroupMember<'a> {
    identity: u32,
    resolved_edge: Option<i64>,
    deleted_boundary_edges: &'a [i64],
}

fn scope_partition_edge_group_candidates<'a>(
    target: &DesignConstructionOperandGroup,
    groups: &'a [DesignConstructionOperandGroup],
    operands: &'a [DesignEdgeOperand],
    target_members: &[crate::records::identity::Located<u32>],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let Some(stream) = native_stream(&target.id) else { return Ok(None); };
    let mut scope_groups = Vec::new();
    let mut target_ordinal = None;
    for group in groups.iter().filter(|group| {
        native_stream(&group.id) == Some(stream)
            && group.scope_record_index == target.scope_record_index
            && group.lost_edge_references.is_empty()
            && !(if group.id == target.id { target_members } else { group.members() }).is_empty()
    }) {
        let group_members = if group.id == target.id { target_members } else { group.members() };
        let mut members = Vec::new();
        let mut complete = true;
        for member in group_members.iter().map(|member| &member.value) {
            let mut matches = operands.iter().filter(|operand| {
                    native_stream(&operand.id) == Some(stream)
                        && operand.scope_record_index == group.scope_record_index
                        && operand.record_index() == *member
                });
            let Some(operand) = matches.next().filter(|_| matches.next().is_none()) else {
                complete = false;
                break;
            };
            push_edge_item(ctx, &mut members, EdgeGroupMember {
                identity: operand.record_index(),
                resolved_edge: resolved_edge_operand(operand),
                deleted_boundary_edges: &operand.deleted_boundary_edge_slots,
            }, "f3d edge partition member")?;
        }
        if !complete {
            continue;
        }
        if group.id == target.id {
            target_ordinal = Some(scope_groups.len());
        }
        push_edge_item(ctx, &mut scope_groups, members, "f3d edge partition group")?;
    }
    let Some(target_ordinal) = target_ordinal else { return Ok(None); };
    partition_unique_incomplete_edge_group(target_ordinal, &scope_groups, ctx)
}

fn partition_unique_incomplete_edge_group(
    target_ordinal: usize,
    groups: &[Vec<EdgeGroupMember<'_>>],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if groups.len() < 2 || target_ordinal >= groups.len() {
        return Ok(None);
    }
    let mut identities = HashSet::new();
    let mut universe = None::<Vec<i64>>;
    for member in groups.iter().flatten() {
        if !insert_edge_set(ctx, &mut identities, member.identity, "f3d edge partition identity")? {
            return Ok(None);
        }
        let mut deleted = Vec::new();
        for edge in member.deleted_boundary_edges {
            push_edge_item(ctx, &mut deleted, *edge, "f3d edge partition deleted edge")?;
        }
        deleted.sort_unstable();
        deleted.dedup();
        if deleted.is_empty()
            || universe
                .as_ref()
                .is_some_and(|universe| *universe != deleted)
        {
            return Ok(None);
        }
        universe.get_or_insert(deleted);
    }
    let Some(universe) = universe else { return Ok(None); };
    if identities.len() != universe.len() {
        return Ok(None);
    }
    let mut incomplete = groups.iter().enumerate()
        .filter(|(_, group)| group.iter().any(|member| member.resolved_edge.is_none()))
        .map(|(ordinal, _)| ordinal);
    if incomplete.next() != Some(target_ordinal) || incomplete.next().is_some() {
        return Ok(None);
    }
    let mut reserved = Vec::new();
    for (ordinal, group) in groups.iter().enumerate() {
        if ordinal == target_ordinal {
            continue;
        }
        for member in group {
            let Some(resolved) = member.resolved_edge.as_ref() else { return Ok(None); };
            if !universe.contains(resolved) || reserved.contains(resolved) {
                return Ok(None);
            }
            push_edge_item(ctx, &mut reserved, *resolved, "f3d edge partition reserved edge")?;
        }
    }
    let mut target = Vec::new();
    for candidate in universe.into_iter().filter(|candidate| !reserved.contains(candidate)) {
        push_edge_item(ctx, &mut target, candidate, "f3d edge partition target edge")?;
    }
    if target.len() != groups[target_ordinal].len()
        || groups[target_ordinal]
            .iter()
            .filter_map(|member| member.resolved_edge)
            .any(|resolved| !target.contains(&resolved))
    {
        return Ok(None);
    }
    Ok(Some(target))
}

fn common_deleted_edge_group_candidates<'a>(
    members: impl IntoIterator<Item = (bool, &'a [i64])>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let mut candidate_sets = members.into_iter()
        .filter_map(|(edge_bearing, candidates)| edge_bearing.then_some(candidates));
    let Some(first) = candidate_sets.next() else { return Ok(None); };
    let mut member_count = 1;
    let mut candidates = Vec::new();
    for candidate in first {
        push_edge_item(ctx, &mut candidates, *candidate,
            "f3d common deleted candidate")?;
    }
    candidates.sort_unstable();
    candidates.dedup();
    for candidate_set in candidate_sets {
        member_count += 1;
        let mut normalized = Vec::new();
        for candidate in candidate_set {
            push_edge_item(ctx, &mut normalized, *candidate,
                "f3d common deleted normalized edge")?;
        }
        normalized.sort_unstable();
        normalized.dedup();
        if normalized != candidates {
            return Ok(None);
        }
    }
    Ok((candidates.len() == member_count).then_some(candidates))
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
fn deleted_boundary_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if operands.is_empty() {
        return Ok(None);
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
            return Ok(None);
        }
        let mut member_contextual = false;
        for edge in operand.recipe_reference_contexts.iter()
            .flat_map(|context| context.changed_reference_edge_slots.iter().copied())
            .filter(|edge| operand.deleted_boundary_edge_slots.contains(edge)) {
            member_contextual = true;
            push_edge_item(ctx, &mut contextual, edge,
                "f3d deleted boundary contextual edge")?;
        }
        if !member_contextual { return Ok(None); }
        for edge in &operand.deleted_boundary_edge_slots {
            push_edge_item(ctx, &mut deleted, *edge,
                "f3d deleted boundary edge")?;
        }
    }
    deleted.sort_unstable();
    deleted.dedup();
    contextual.sort_unstable();
    contextual.dedup();
    Ok((deleted.len() == operands.len()
        && deleted
            .iter()
            .all(|edge| contextual.binary_search(edge).is_ok()))
    .then_some(deleted))
}

/// Resolve a treatment group when the deleted predecessor-edge set is complete
/// at group level but one or more members lack an operation-level deletion.
///
/// A member is admitted only through a recipe context that names one of the
/// deleted predecessor edges. The contextual candidate sets must have exactly
/// one perfect assignment; otherwise the group remains native. This handles a
/// legacy group whose transition records consolidate one member's deletion
/// while retaining the member-to-edge relation in the recipe references.
fn contextual_deleted_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if operands.is_empty() {
        return Ok(None);
    }
    let mut deleted = Vec::new();
    let mut has_deleted_member = false;
    for operand in operands {
        if operand.recipe_structure.is_none()
            || operand.recipe_references.is_empty()
            || operand.recipe_references.len() != operand.recipe_reference_contexts.len()
            || operand.recipe_reference_contexts.is_empty()
        {
            return Ok(None);
        }
        for edge in &operand.deleted_boundary_edge_slots {
            if !operand.preceding_boundary_edge_slots.contains(edge)
                || !operand.changed_boundary_edge_slots.contains(edge)
            {
                return Ok(None);
            }
            push_edge_item(ctx, &mut deleted, *edge,
                "f3d contextual deleted edge")?;
            has_deleted_member = true;
        }
    }
    if !has_deleted_member {
        return Ok(None);
    }
    deleted.sort_unstable();
    deleted.dedup();
    if deleted.len() != operands.len() {
        return Ok(None);
    }

    let mut candidate_sets = Vec::new();
    for operand in operands {
        let mut candidates = Vec::new();
        for edge in operand.recipe_reference_contexts.iter()
            .flat_map(|context| context.changed_reference_edge_slots.iter().copied())
            .filter(|edge| deleted.binary_search(edge).is_ok()) {
            push_edge_item(ctx, &mut candidates, edge,
                "f3d contextual deleted candidate")?;
        }
        candidates.sort_unstable();
        candidates.dedup();
        push_edge_item(ctx, &mut candidate_sets, candidates,
            "f3d contextual deleted candidate set")?;
    }
    let Some(mut assignment) = unique_bipartite_assignment(&candidate_sets, ctx)? else {
        return Ok(None);
    };
    assignment.sort_unstable();
    Ok(Some(assignment))
}

/// Resolve a legacy single-member treatment whose selected edge persists in
/// the result boundary instead of appearing in the operand boundary delta.
///
/// The zero-payload two-side recipe supplies support references but no direct
/// edge entry. A result-boundary edge named by at least two changed-reference
/// contexts is admitted only when it is the sole such edge and is absent from
/// the operand's preceding candidate boundary. This leaves deleted-edge and
/// ambiguous reference sets native.
fn result_boundary_reference_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let [operand] = operands else {
        return Ok(None);
    };
    let Some(structure) = operand.recipe_structure.as_ref() else { return Ok(None); };
    if structure.root != 2
        || structure.sides.len() != 2
        || structure.sides.iter().any(|side| {
            side.scalars.len() != 2 || side.payload_prefix != [0] || !side.entries.is_empty()
        })
        || operand.recipe_references.is_empty()
        || operand.recipe_references.len() != operand.recipe_reference_contexts.len()
        || operand.recipe_reference_contexts.is_empty()
        || !operand.changed_boundary_edge_slots.is_empty()
        || !operand.deleted_boundary_edge_slots.is_empty()
    {
        return Ok(None);
    }
    let mut candidates = Vec::new();
    for edge in operand.recipe_reference_contexts.iter()
        .flat_map(|context| context.changed_reference_edge_slots.iter().copied())
        .filter(|edge| operand.result_boundary_edge_slots.contains(edge)) {
        push_edge_item(ctx, &mut candidates, edge,
            "f3d result boundary candidate")?;
    }
    candidates.sort_unstable();
    candidates.dedup();
    let [candidate] = candidates.as_slice() else {
        return Ok(None);
    };
    if operand.preceding_boundary_edge_slots.contains(candidate)
        || operand
            .recipe_reference_contexts
            .iter()
            .filter(|context| context.changed_reference_edge_slots.contains(candidate))
            .count()
            < 2
    {
        return Ok(None);
    }
    Ok(Some(vec![*candidate]))
}

fn changed_boundary_count_edge_group_candidates<'a>(
    members: impl IntoIterator<
        Item = &'a [crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    >,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let mut member_count = 0;
    let mut candidates = Vec::new();
    for selectors in members {
        if selectors.is_empty() { return Ok(None); }
        member_count += 1;
        for edge in selectors.iter()
            .flat_map(|selector| selector.boundary_count_matching_edge_slots.iter().copied()) {
            push_edge_item(ctx, &mut candidates, edge,
                "f3d boundary-count candidate")?;
        }
    }
    candidates.sort_unstable();
    candidates.dedup();
    Ok((member_count > 0 && candidates.len() == member_count).then_some(candidates))
}

pub(super) fn resolved_edge_operand(operand: &DesignEdgeOperand) -> Option<i64> {
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
            side.scalars.len() != 2 || side.payload_prefix != [0] || !side.entries.is_empty()
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

    let mut candidate = None;
    for edge in first.iter().copied().filter(|edge| second.contains(edge)) {
        if candidate.is_some_and(|selected| selected != edge) {
            return None;
        }
        candidate = Some(edge);
    }
    candidate.filter(|edge| operand.terminal_boundary_edge_slots.contains(edge))
}

pub(super) fn edge_operand_reference_edge_sets(
    operand: &DesignEdgeOperand,
) -> impl Iterator<Item = &[i64]> + '_ {
    let terminal = operand.recipe_reference_contexts.is_empty();
    let source_count = if terminal {
        operand.terminal_reference_edge_slots.len()
    } else {
        operand.recipe_reference_contexts.len()
    };
    let selected_count = operand.local_topology_references.as_ref()
        .map_or(source_count, Vec::len);
    (0..selected_count).filter_map(move |selected| {
        let source = match &operand.local_topology_references {
            Some(ordinals) => usize::try_from(ordinals.get(selected)?.get()).ok()?.checked_sub(1)?,
            None => selected,
        };
        if terminal {
            operand.terminal_reference_edge_slots.get(source).map(Vec::as_slice)
        } else {
            operand.recipe_reference_contexts.get(source)
                .map(|context| context.changed_reference_edge_slots.as_slice())
        }
    })
}

pub(crate) fn resolved_edge_candidate_intersection<'a>(
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
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
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
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
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
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
        .then(|| {
            corroborated_edge_intersection(
                selector_contexts,
                &shared_edge_sets,
                SelectorSlots::Incidence,
            )
        })
        .flatten();
    let boundary_count = (!references_unavailable)
        .then(|| {
            corroborated_edge_intersection(
                selector_contexts,
                &shared_edge_sets,
                SelectorSlots::BoundaryCount,
            )
        })
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
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: &[&[i64]],
) -> Option<i64> {
    let edge_sets = selector_contexts.iter().flat_map(|selector| {
        selector.clauses.iter().flatten().filter_map(|clause| {
            clause.entry.common_incident_edge_ordinal()?;
            let [first, second] = &clause.triplet_edge_slots;
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
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: &[&[i64]],
) -> Option<i64> {
    let edge_sets = selector_contexts.iter().flat_map(|selector| {
        let [Some(left), Some(right)] = selector.clauses.as_slice() else {
            return Vec::new();
        };
        left.triplet_edge_slots
            .iter()
            .zip(&right.triplet_edge_slots)
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

#[derive(Clone, Copy)]
enum SelectorSlots {
    Incidence,
    BoundaryCount,
}

fn corroborated_edge_intersection(
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: &[&[i64]],
    slots: SelectorSlots,
) -> Option<i64> {
    let candidates =
        corroborated_edge_candidates(selector_contexts, shared_edge_sets.iter().copied(), slots)?;
    match candidates.as_slice() {
        [candidate] => Some(*candidate),
        _ => None,
    }
}

fn corroborated_edge_candidates<'a>(
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: impl IntoIterator<Item = &'a [i64]>,
    slots: SelectorSlots,
) -> Option<Vec<i64>> {
    let mut selectors = selector_contexts.iter();
    let first = selector_candidate_edges(selectors.next()?, slots);
    if first.is_empty() {
        return None;
    }
    let mut candidates = first.to_vec();
    candidates.sort_unstable();
    candidates.dedup();
    for selector in selectors {
        let selector_edges = selector_candidate_edges(selector, slots);
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
    selector: &crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext,
    slots: SelectorSlots,
) -> &[i64] {
    match slots {
        SelectorSlots::BoundaryCount => &selector.boundary_count_matching_edge_slots,
        SelectorSlots::Incidence => &selector.incidence_matching_edge_slots,
    }
}

#[cfg(test)]
fn project_fixed_fillet(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    project_fixed_fillet_with_corners(
        scope,
        construction_groups,
        edge_operands,
        edge_identity_operands,
        &[],
        &[],
        ctx,
    )
}

pub(super) fn project_fixed_fillet_with_corners(
    scope: &DesignParameterScope,
    construction_groups: &[DesignConstructionOperandGroup],
    edge_operands: &[DesignEdgeOperand],
    edge_identity_operands: &[DesignEdgeIdentityOperand],
    vertex_operands: &[DesignEdgeTreatmentVertexOperand],
    histories: &[crate::history_records::AsmHistory],
    ctx: Option<&DecodeContext<'_>>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        edge_treatments::{FilletGroup, RadiusSpec, VariableRadius},
        FeatureDefinition, FeatureOperation,
    };
    use cadmpeg_ir::scalar::Length;

    let fixed = or_none!(scope.fixed_fillet_parameters());
    let stream = or_none!(native_stream(&scope.id));
    let radius_spec =
        |group: &crate::records::feature::fixed_parameters::DesignFixedFilletGroup| -> Result<Option<RadiusSpec>, CodecError> { Ok(match group.law() {
            crate::records::feature::fixed_parameters::DesignFixedFilletLaw::Constant(radius) => {
                let Some(radius) = cadmpeg_ir::scalar::PositiveLength::new(radius.value.get() * 10.0) else { return Ok(None); };
                Some(RadiusSpec::Constant {
                    radius,
                })
            }
            crate::records::feature::fixed_parameters::DesignFixedFilletLaw::Variable {
                start,
                end,
                intermediate,
            } => {
                let mut points = Vec::new();
                let Some(start_radius) = Length::new(start.value.get() * 10.0) else { return Ok(None); };
                push_edge_item(ctx, &mut points, VariableRadius {
                    parameter: 0.0,
                    radius: start_radius,
                }, "f3d fixed fillet radius point")?;
                for row in intermediate {
                    let Some(radius) = Length::new(row.radius.value.get() * 10.0) else { return Ok(None); };
                    push_edge_item(ctx, &mut points, VariableRadius {
                        parameter: row.parameter.value.get(),
                        radius,
                    }, "f3d fixed fillet radius point")?;
                }
                let Some(end_radius) = Length::new(end.value.get() * 10.0) else { return Ok(None); };
                push_edge_item(ctx, &mut points, VariableRadius {
                    parameter: 1.0,
                    radius: end_radius,
                }, "f3d fixed fillet radius point")?;
                let Some(points) = cadmpeg_ir::features::edge_treatments::VariableRadii::new(points).ok() else { return Ok(None); };
                Some(RadiusSpec::Variable {
                    points,
                })
            }
        }) };
    let mut scope_groups = Vec::new();
    for group in construction_groups.iter().filter(|group| {
            native_stream(&group.id) == Some(stream)
                && group.scope_record_index == scope.record_index
                && !group.members().is_empty()
        }) {
        push_edge_item(ctx, &mut scope_groups, group, "f3d fixed fillet scope group")?;
    }
    scope_groups.sort_by_key(|group| group.scope_reference_ordinal);
    let mut complete_edge_groups = Vec::new();
    for group in scope_groups.iter().copied().filter(|group| {
            group
                .members()
                .iter()
                .map(|member| &member.value)
                .all(|member| {
                    edge_operands.iter().any(|operand| {
                        native_stream(&operand.id) == Some(stream)
                            && operand.scope_record_index == scope.record_index
                            && operand.record_index() == *member
                    })
                })
        }) {
        push_edge_item(ctx, &mut complete_edge_groups, group, "f3d fixed fillet complete group")?;
    }
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
                return Ok(None);
            };
            let radius = radius_spec(&fixed.groups[0])?;
            let Some(RadiusSpec::Constant { radius }) = radius else {
                return Ok(None);
            };
            let mut identities = Vec::new();
            for member in group.members().iter().map(|member| &member.value) {
                    let mut matches = edge_identity_operands.iter().filter(|operand| {
                            native_stream(&operand.id) == Some(stream)
                                && operand.scope_record_index == scope.record_index
                                && operand.group_record_index == group.record_index
                                && operand.record_index() == *member
                        });
                    let Some(operand) = matches.next().filter(|_| matches.next().is_none()) else {
                        return Ok(None);
                    };
                    if !operand.layout().is_compact() { return Ok(None); }
                    push_edge_item(ctx, &mut identities, operand, "f3d fixed fillet identity")?;
            }
            or_none!(radius_edge_identity_group_candidates(
                &identities,
                radius.get(), ctx
            )?);
            *group
        };
        vec![group]
    } else {
        return Ok(None);
    };
    let mut groups = Vec::new();
    for (fixed_group, edge_group) in fixed.groups.iter().zip(edge_groups) {
        let radius = or_none!(radius_spec(fixed_group)?);
        let edge_radius = match radius {
            RadiusSpec::Constant { radius } => Some(radius.get()),
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
            scope.previous_history_state_id(),
            &neutral_feature_id(scope),
            edge_radius,
            ctx,
        )?;
        push_edge_item(ctx, &mut groups, FilletGroup {
            edges,
            radius,
            tangency_weight: fixed_group.tangency_weight().map(|tangency| tangency.value),
        }, "f3d fixed fillet output group")?;
    }
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Fillet {
            groups: or_none!(groups.try_into().ok()),
        },
    )))
}

#[cfg(test)]
mod tests;
