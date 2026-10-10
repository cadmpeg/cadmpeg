// SPDX-License-Identifier: Apache-2.0
//! Resolve edge-selection operands to stable edge identities.

use super::edge_references::edge_operand_reference_edge_sets;
use super::feature_project::unique_feature_match_for_decode;
use crate::ids::{self, native_stream_charged};
use crate::records::{
    feature::{scope::DesignParameterScope, work_geometry::DesignEdgeTreatmentVertexOperand},
    topology::{
        construction::DesignConstructionOperandGroup, edge_identity::DesignEdgeIdentityOperand,
        edge_identity::DesignEdgeOperand,
    },
};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, HashSet};

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

fn sorted_transition_slots(
    slots: &[i64],
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<Vec<i64>, CodecError> {
    if slots.is_empty() { return Ok(Vec::new()); }
    let mut storage = ctx.reserve_scoped(0, "hold F3D sorted transition slots")?;
    let mut sorted = storage.with_storage(|| ctx.copy_slice(slots, operation))?;
    ctx.sort_unstable_by(
        &mut sorted,
        |value| value,
        Ord::cmp,
        "f3d transition slot sort",
    )?;
    ctx.dedup_vec(&mut sorted, "deduplicate F3D transition slots")?;
    storage.commit_value(sorted)
}

fn native_edge_selection(
    group: &DesignConstructionOperandGroup,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
    Ok(cadmpeg_ir::features::EdgeSelection::Native(
        ctx.copy_retained_text(&group.id, "f3d native edge group id")?,
    ))
}

/// Return a recipe state only when every matched operand names it.
fn shared_recipe_state_id(
    operands: &[&DesignEdgeOperand],
    ctx: &DecodeContext<'_>,
) -> Result<Option<i64>, CodecError> {
    let Some(state_id) = operands.first().and_then(|operand| operand.recipe_state_id) else {
        return Ok(None);
    };
    let mut remaining = operands[1..].iter();
    while !remaining.as_slice().is_empty() {
        let Some(operand) =
            ctx.next_charged(&mut remaining, "scan F3D shared edge recipe states")?
        else {
            break;
        };
        if operand.recipe_state_id != Some(state_id) {
            return Ok(None);
        }
    }
    Ok(Some(state_id))
}

fn historical_identity_slots(
    group: &DesignConstructionOperandGroup,
    state: cadmpeg_ir::ids::FeatureInputTopologyId,
    feature_key: &str,
    previous_state_id: i64,
    slots: &[i64],
    ctx: &DecodeContext<'_>,
    (slot_operation, native_operation): (&'static str, &'static str),
) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
    let mut edges = Vec::new();
    let mut prefix_storage = ctx.reserve_scoped(0, "f3d historical edge prefix storage")?;
    if !slots.is_empty() {
    let prefix = prefix_storage.with_storage(|| crate::design::identity::history_input_prefix(ctx, feature_key, previous_state_id))?;
    let mut slot_source = slots.iter();
    while let Some(&slot) = ctx.next_charged(&mut slot_source, "f3d edge projection source")? {
        let edge = crate::design::identity::history_input_edge_id(
            ctx,
            &prefix,
            slot,
            "f3d historical edge identifier",
        )?;
        ctx.push_vec(&mut edges, edge, slot_operation)?;
    }
    }
    let native = ctx.copy_retained_text(&group.id, native_operation)?;
    match cadmpeg_ir::features::EdgeSelection::historical(state, edges, native, ctx)? {
        Ok(selection) => Ok(selection),
        Err(_) => native_edge_selection(group, ctx),
    }
}

pub(super) fn resolved_edge_group(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    ctx: &DecodeContext<'_>,
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
        (EdgeGroupProof::Generic, None),
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
    ctx: &DecodeContext<'_>,
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
    let stream = native_stream_charged(ctx, &group.id, "f3d edge native stream")?;
    let mut member_storage = ctx.reserve_scoped(0, "hold F3D surface patch member index")?;
    let mut member_ids = HashSet::new();
    let mut member_source = group.members().iter();
    while let Some(member) = ctx.next_charged(&mut member_source, "f3d edge projection source")? {
        if !member_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut member_ids,
                member.value,
                "f3d surface patch edge member index",
            )
        })? {
            return fallback();
        }
    }
    let mut matched_storage = ctx.reserve_scoped(0, "hold F3D surface patch matched operands")?;
    let mut matched_operands = Vec::new();
    let mut operand_member_source = group.members().iter();
    while let Some(member) = ctx.next_charged(&mut operand_member_source, "f3d edge projection source")? {
        let matches = unique_feature_match_for_decode(ctx, operands.iter(), |operand| Ok(ctx.equal(&native_stream_charged(ctx, &operand.id, "f3d edge native stream")?, &stream, "f3d edge stream comparison")?
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index() == member.value), "f3d unique edge operand")?;
        let Some(operand) = matches else {
            return fallback();
        };
        ctx.push_scoped_vec(
            &mut matched_storage,
            &mut matched_operands,
            operand,
            "f3d surface patch matched edge operand",
        )?;
    }
    let mut edge_storage =
        ctx.provisional_retained("hold F3D surface patch recipe output candidate")?;
    let edges = match edge_storage
        .with_storage(|| surface_patch_grouped_recipe_edges(&matched_operands, ctx))?
    {
        SurfacePatchRecipeEdges::Absent => return fallback(),
        SurfacePatchRecipeEdges::Inconclusive => {
            return Ok(cadmpeg_ir::features::EdgeSelection::Native(
                ctx.copy_retained_text(&group.id, "f3d surface patch native group id")?,
            ));
        }
        SurfacePatchRecipeEdges::Resolved(edges) => edges,
    };
    let state_id = match previous_state_id {
        Some(state_id) => Some(state_id),
        None => shared_recipe_state_id(&matched_operands, ctx)?,
    };
    let Some(state_id) = state_id else {
        return edge_storage.commit_value(cadmpeg_ir::features::EdgeSelection::Edges(edges));
    };
    let mut slot_storage = ctx.reserve_scoped(0, "hold F3D surface patch stable slots")?;
    let mut edge_slots = Vec::new();
    let mut selected_edge_source = edges.iter();
    while let Some(edge) = ctx.next_charged(&mut selected_edge_source, "f3d edge projection source")? {
        let Some(slot) = stable_edge_slot(edge, ctx)? else {
            return fallback();
        };
        ctx.push_scoped_vec(
            &mut slot_storage,
            &mut edge_slots,
            slot,
            "f3d surface patch stable edge slot",
        )?;
    }
    drop(edges);
    drop(edge_storage);
    let feature_key = crate::design::identity::identity_key(ctx, feature_id.as_str())?;
    let mut historical_edges = Vec::new();
    let (prefix, _prefix_storage) = ctx.with_scoped_storage("f3d historical edge prefix storage", || crate::design::identity::history_input_prefix(ctx, feature_key, state_id))?;
    let mut historical_slot_source = edge_slots.iter();
    while let Some(&edge_slot) = ctx.next_charged(&mut historical_slot_source, "f3d edge projection source")? {
        let id = crate::design::identity::history_input_edge_id(
            ctx,
            &prefix,
            edge_slot,
            "f3d historical edge identifier",
        )?;
        ctx.push_vec(
            &mut historical_edges,
            id,
            "f3d surface patch historical edge",
        )?;
    }
    drop(edge_slots);
    drop(slot_storage);
    let native = ctx.copy_retained_text(&group.id, "f3d surface patch historical group id")?;
    let resolved = cadmpeg_ir::features::EdgeSelection::historical(
        crate::design::identity::feature_input_topology_id(ctx, feature_id, state_id)?,
        historical_edges,
        native,
        ctx,
    )?;
    Ok(match resolved {
        Ok(selection) => selection,
        Err(_) => cadmpeg_ir::features::EdgeSelection::Native(
            ctx.copy_retained_text(&group.id, "f3d surface patch fallback group id")?,
        ),
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
    ctx: &DecodeContext<'_>,
) -> Result<SurfacePatchRecipeEdges, CodecError> {
    if operands.is_empty() {
        return Ok(SurfacePatchRecipeEdges::Absent);
    }
    let mut candidate_storage =
        ctx.provisional_retained("hold F3D surface patch recipe candidate")?;
    let mut edges = Vec::new();
    let mut has_absent_member = false;
    let mut remaining_operands = operands.iter();
    while !remaining_operands.as_slice().is_empty() {
        let Some(operand) = ctx.next_charged(
            &mut remaining_operands,
            "scan F3D surface patch recipe operands",
        )?
        else {
            break;
        };
        let mut edge: Option<&cadmpeg_ir::ids::EdgeId> = None;
        let mut references = operand.recipe_references.iter();
        while !references.as_slice().is_empty() {
            let Some(reference) =
                ctx.next_charged(&mut references, "scan F3D surface patch recipe references")?
            else {
                break;
            };
            if reference.candidate_edges.is_empty() {
                continue;
            }
            let [candidate] = reference.candidate_edges.as_slice() else {
                return Ok(SurfacePatchRecipeEdges::Inconclusive);
            };
            if let Some(first) = edge {
                if !ctx.equal_bytes(
                    first.as_str().as_bytes(),
                    candidate.as_str().as_bytes(),
                    "compare F3D surface patch recipe edge identity",
                )? {
                    return Ok(SurfacePatchRecipeEdges::Inconclusive);
                }
            } else {
                edge = Some(candidate);
            }
        }
        let Some(edge) = edge else {
            has_absent_member = true;
            continue;
        };
        let copied = candidate_storage.with_storage(|| {
            (edge).try_clone_for_decode(ctx, "f3d surface patch recipe edge id")
        })?;
        candidate_storage
            .with_storage(|| ctx.push_vec(&mut edges, copied, "f3d surface patch recipe edge"))?;
    }
    if has_absent_member {
        return Ok(SurfacePatchRecipeEdges::Absent);
    }
    let mut distinct_storage =
        ctx.reserve_scoped(0, "hold F3D surface patch distinct recipe edges")?;
    let mut distinct = HashSet::new();
    let mut distinct_edge_source = edges.iter();
    while let Some(edge) = ctx.next_charged(&mut distinct_edge_source, "f3d edge projection source")? {
        if !distinct_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut distinct,
                edge.as_str(),
                "f3d surface patch distinct recipe edge",
            )
        })? {
            return Ok(SurfacePatchRecipeEdges::Inconclusive);
        }
    }
    candidate_storage.commit_value(SurfacePatchRecipeEdges::Resolved(edges))
}

fn stable_edge_slot(edge: &cadmpeg_ir::ids::EdgeId, ctx: &DecodeContext<'_>) -> Result<Option<i64>, CodecError> {
    let Some((_, suffix)) = ctx.rsplit_once(edge.as_str(), "#", "f3d stable edge slot suffix")? else { return Ok(None); };
    let slot = ctx.split_once(suffix, ":", "f3d stable edge slot prefix")?.map_or(suffix, |(slot, _)| slot);
    Ok(ctx.parse_text::<i64>(slot, "f3d stable edge slot parse")?.ok())
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
    ctx: &DecodeContext<'_>,
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
    let stream = native_stream_charged(ctx, &group.id, "f3d edge native stream")?;
    let mut candidate_storage = ctx.reserve_scoped(0, "collect F3D edge flange candidates")?;
    let mut members = HashSet::new();
    let mut candidate_sets = Vec::new();
    let mut flange_member_source = group.members().iter();
    while let Some(member) = ctx.next_charged(&mut flange_member_source, "f3d edge projection source")? {
        if !candidate_storage.with_storage(|| {
            ctx.insert_hash_set(&mut members, member.value, "f3d edge flange member index")
        })? {
            return Ok(selection);
        }
        let matching = unique_feature_match_for_decode(ctx, operands.iter(), |operand| Ok(ctx.equal(&native_stream_charged(ctx, &operand.id, "f3d edge native stream")?, &stream, "f3d edge stream comparison")?
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index() == member.value), "f3d unique edge operand")?;
        let Some(operand) = matching else {
            return Ok(selection);
        };
        let Some(edge_slot) = edge_flange_updated_edge_candidate(operand, ctx)? else {
            return Ok(selection);
        };
        let mut candidate = candidate_storage
            .with_storage(|| ctx.collection_vec(1, "collect F3D flange updated edge candidate"))?;
        candidate.push(edge_slot);
        candidate_storage.with_storage(|| {
            ctx.push_vec(
                &mut candidate_sets,
                candidate,
                "f3d edge flange candidate set",
            )
        })?;
    }
    let Some(edges) =
        candidate_storage.with_storage(|| unique_bipartite_assignment(&candidate_sets, ctx))?
    else {
        return Ok(selection);
    };
    if edges.is_empty() {
        return Ok(selection);
    }
    let feature_key = crate::design::identity::identity_key(ctx, feature_id.as_str())?;
    let state =
        crate::design::identity::feature_input_topology_id(ctx, feature_id, previous_state_id)?;
    let mut historical_edges = Vec::new();
    let (prefix, _prefix_storage) = ctx.with_scoped_storage("f3d historical edge prefix storage", || crate::design::identity::history_input_prefix(ctx, feature_key, previous_state_id))?;
    for &edge_slot in ctx.admit_iter(&edges, "scan F3D flange selected native slots")? {
        let id = crate::design::identity::history_input_edge_id(
            ctx,
            &prefix,
            edge_slot,
            "f3d historical edge identifier",
        )?;
        ctx.push_vec(&mut historical_edges, id, "f3d edge flange historical edge")?;
    }
    let native = ctx.copy_retained_text(&group.id, "f3d edge flange historical group id")?;
    let historical =
        cadmpeg_ir::features::EdgeSelection::historical(state, historical_edges, native, ctx)?;
    Ok(match historical {
        Ok(selection) => selection,
        Err(_) => EdgeSelection::Native(
            ctx.copy_retained_text(&group.id, "f3d edge flange fallback group id")?,
        ),
    })
}

fn edge_flange_updated_edge_candidate(
    operand: &DesignEdgeOperand,
    ctx: &DecodeContext<'_>,
) -> Result<Option<i64>, CodecError> {
    if !operand.recipe_references.is_empty()
        || !operand.recipe_selectors.is_empty()
        || !operand.recipe_reference_contexts.is_empty()
        || operand.local_topology_references.is_some()
    {
        return Ok(None);
    }
    let [edge] = operand.updated_boundary_edge_slots.as_slice() else {
        return Ok(None);
    };
    if !ctx.contains(
        &operand.preceding_boundary_edge_slots,
        edge,
        "find F3D flange preceding edge",
    )? || !ctx.contains(
        &operand.changed_boundary_edge_slots,
        edge,
        "find F3D flange changed edge",
    )? || ctx.contains(
        &operand.deleted_boundary_edge_slots,
        edge,
        "find F3D flange deleted edge",
    )? || !ctx.contains(
        &operand.result_boundary_edge_slots,
        edge,
        "find F3D flange result edge",
    )? {
        return Ok(None);
    }
    Ok(Some(*edge))
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
    crate::test_support::with_decode_context(|decode_ctx| {
        resolved_edge_treatment_group_with_corners(
            group,
            crate::design::edge_resolve::EdgeTreatmentInputs {
                groups,
                operands,
                identity_operands,
                vertex_operands: &[],
                histories: &[],
                previous_state_id,
                feature_id,
                treatment_radius,
            },
            decode_ctx,
        )
    })
}

#[derive(Clone, Copy)]
pub(crate) struct EdgeTreatmentInputs<'a> {
    pub(crate) groups: &'a [DesignConstructionOperandGroup],
    pub(crate) operands: &'a [DesignEdgeOperand],
    pub(crate) identity_operands: &'a [DesignEdgeIdentityOperand],
    pub(crate) vertex_operands: &'a [DesignEdgeTreatmentVertexOperand],
    pub(crate) histories: &'a [crate::history_records::AsmHistory],
    pub(crate) previous_state_id: Option<i64>,
    pub(crate) feature_id: &'a cadmpeg_ir::features::FeatureId,
    pub(crate) treatment_radius: Option<f64>,
}

pub(super) fn resolved_edge_treatment_group_with_corners(
    group: &DesignConstructionOperandGroup,
    input: EdgeTreatmentInputs<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<cadmpeg_ir::features::EdgeSelection, CodecError> {
    use cadmpeg_ir::features::EdgeSelection;

    let EdgeTreatmentInputs {
        groups,
        operands,
        identity_operands,
        vertex_operands,
        histories,
        previous_state_id,
        feature_id,
        treatment_radius,
    } = input;

    let stream = native_stream_charged(ctx, &group.id, "f3d edge native stream")?;
    let has_group_corner = ctx.any_by(vertex_operands, |operand| Ok(
        ctx.equal(&native_stream_charged(ctx, &operand.id, "f3d edge native stream")?, &stream, "f3d edge stream comparison")?
            && operand.scope_record_index == group.scope_record_index
            && operand.group_record_index == group.record_index
    ), "f3d treatment corner operand source")?;
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
            (
                EdgeGroupProof::Treatment {
                    radius: treatment_radius,
                },
                None,
            ),
            ctx,
        );
    }
    let mut corner_storage = ctx.reserve_scoped(0, "hold F3D treatment corners")?;
    let mut corner_slots = Vec::new();
    let mut member_storage = ctx.reserve_scoped(0, "hold F3D treatment edge members")?;
    let mut edge_members = Vec::new();
    let mut treatment_member_source = group.members().iter().copied().enumerate();
    while let Some((ordinal, member)) = ctx.next_charged(&mut treatment_member_source, "f3d edge projection source")? {
        let edge_matches = |operand: &DesignEdgeOperand| -> Result<bool, CodecError> {
            Ok(ctx.equal(&native_stream_charged(ctx, &operand.id, "f3d edge native stream")?, &stream, "f3d edge stream comparison")?
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index() == member.value)
        };
        let mut edge_source = operands.iter();
        let edge_count = if ctx.find_by(&mut edge_source, |operand| edge_matches(operand), "f3d treatment edge operand source")?.is_some() {
            1 + usize::from(ctx.any_by(edge_source, edge_matches, "f3d treatment edge operand source")?)
        } else { 0 };
        let corner = match u32::try_from(ordinal) {
            Ok(ordinal) => unique_feature_match_for_decode(ctx, vertex_operands.iter(), |operand| Ok(
                ctx.equal(&native_stream_charged(ctx, &operand.id, "f3d edge native stream")?, &stream, "f3d edge stream comparison")?
                    && operand.scope_record_index == group.scope_record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == ordinal
                    && operand.recipe.record_index() == member.value
            ), "f3d unique treatment corner operand")?,
            Err(_) => None,
        };
        match (edge_count, corner) {
            (1, None) => {
                ctx.push_scoped_vec(
                    &mut member_storage,
                    &mut edge_members,
                    member,
                    "f3d treatment edge member",
                )?;
            }
            (0, Some(corner)) => {
                let Some(resolution) = corner.recipe.resolution else {
                    return native_edge_selection(group, ctx);
                };
                if Some(resolution.state_id) != previous_state_id {
                    return native_edge_selection(group, ctx);
                }
                ctx.push_scoped_vec(
                    &mut corner_storage,
                    &mut corner_slots,
                    resolution.vertex_slot(),
                    "f3d treatment corner slot",
                )?;
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
        (
            EdgeGroupProof::Treatment {
                radius: treatment_radius,
            },
            Some(&edge_members),
        ),
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
    let mut topology = None;
    let mut multiple_states = false;
    let mut history_source = histories.iter();
    'histories: while let Some(history) = ctx.next_charged(&mut history_source, "f3d treatment history source")? {
        if !ids::same_native_occurrence(ctx, &history.id, &group.id)? {
            continue;
        }
        let mut state_source = history.states.iter();
        while let Some(state) = ctx.next_charged(&mut state_source, "f3d treatment history state source")? {
            if state.state_id != state_id {
                continue;
            }
            if topology.is_some() {
                multiple_states = true;
                break 'histories;
            }
            let Some(found_topology) = state.topology() else {
                break 'histories;
            };
            topology = Some(found_topology);
        }
    }
    let Some(topology) = topology.filter(|_| !multiple_states) else {
        return native_edge_selection(group, ctx);
    };
    let mut endpoint_storage = ctx.reserve_scoped(0, "hold F3D treatment endpoint vertices")?;
    let mut endpoints = HashSet::new();
    let mut treatment_edge_source = edges.iter();
    while let Some(edge) = ctx.next_charged(&mut treatment_edge_source, "f3d edge projection source")? {
        let slot = ctx.rsplit_once(edge.as_str(), ":", "f3d treatment edge slot suffix")?
            .map_or(edge.as_str(), |(_, slot)| slot);
        let Some(edge_slot) = ctx.parse_text::<i64>(slot, "f3d treatment edge slot parse")?.ok() else {
            return native_edge_selection(group, ctx);
        };
        let Some(edge) = unique_feature_match_for_decode(ctx, topology.edge_vertices.iter(), |edge| Ok(edge.edge == edge_slot), "f3d treatment edge topology source")? else {
            return native_edge_selection(group, ctx);
        };
        endpoint_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut endpoints,
                edge.start_vertex,
                "f3d treatment endpoint vertex",
            )
        })?;
        endpoint_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut endpoints,
                edge.end_vertex,
                "f3d treatment endpoint vertex",
            )
        })?;
    }
    if ctx.all_by(&corner_slots, |corner| ctx.contains_hash_set(&endpoints, corner, "f3d treatment corner endpoint lookup"), "f3d treatment corner endpoint source")? {
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
    (proof, members_override): (
        EdgeGroupProof,
        Option<&[crate::records::identity::Located<u32>]>,
    ),
    ctx: &DecodeContext<'_>,
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

    let feature_key = crate::design::identity::identity_key(ctx, feature_id.as_str())?;
    let unmatched_selection = || -> Result<EdgeSelection, CodecError> {
        if group.lost_edge_references.is_empty() {
            native_edge_selection(group, ctx)
        } else {
            Ok(EdgeSelection::Unresolved)
        }
    };
    let stream = native_stream_charged(ctx, &group.id, "f3d edge native stream")?;
    let has_surface_patch_operand = ctx.any_by(operands, |operand| Ok(
        ctx.equal(&native_stream_charged(ctx, &operand.id, "f3d edge native stream")?, &stream, "f3d edge stream comparison")?
            && operand.scope_record_index == group.scope_record_index
            && ctx.any_by(members, |member| Ok(member.value == operand.record_index()), "f3d surface patch member presence")?
            && operand.surface_patch_recipe_structure.is_some()
    ), "f3d surface patch operand presence")?;
    if has_surface_patch_operand {
        let mut member_storage = ctx.reserve_scoped(0, "hold F3D generic surface patch members")?;
        let mut member_ids = HashSet::new();
        let mut surface_member_source = members.iter();
        while let Some(member) = ctx.next_charged(&mut surface_member_source, "f3d edge projection source")? {
            if !member_storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut member_ids,
                    member.value,
                    "f3d generic surface patch member index",
                )
            })? {
                return unmatched_selection();
            }
        }
        let mut matched_storage =
            ctx.reserve_scoped(0, "hold F3D generic surface patch operands")?;
        let mut matched_operands = Vec::new();
        let mut surface_operand_member_source = members.iter().map(|member| &member.value);
        while let Some(member) = ctx.next_charged(&mut surface_operand_member_source, "f3d edge projection source")? {
            let matches = unique_feature_match_for_decode(ctx, operands.iter(), |operand| Ok(ctx.equal(&native_stream_charged(ctx, &operand.id, "f3d edge native stream")?, &stream, "f3d edge stream comparison")?
                    && operand.scope_record_index == group.scope_record_index
                    && operand.record_index() == *member), "f3d unique edge operand")?;
            let Some(operand) = matches else {
                return unmatched_selection();
            };
            ctx.push_scoped_vec(
                &mut matched_storage,
                &mut matched_operands,
                operand,
                "f3d generic surface patch matched operand",
            )?;
        }
        if ctx.any_by(&matched_operands, |operand| Ok(operand.surface_patch_recipe_structure.is_none()), "f3d surface patch recipe presence")? {
            return unmatched_selection();
        }
        let state_id = match previous_state_id {
            Some(state_id) => Some(state_id),
            None => shared_recipe_state_id(&matched_operands, ctx)?,
        };
        let Some(state_id) = state_id else {
            return unmatched_selection();
        };
        let mut edge_storage = ctx.reserve_scoped(0, "hold F3D generic surface patch slots")?;
        let mut edges = Vec::new();
        let mut surface_operand_source = matched_operands.iter();
        while let Some(operand) = ctx.next_charged(&mut surface_operand_source, "f3d edge projection source")? {
            let Some(edge) = operand.resolved_edge_slot else {
                return unmatched_selection();
            };
            ctx.push_scoped_vec(
                &mut edge_storage,
                &mut edges,
                edge,
                "f3d generic surface patch resolved slot",
            )?;
        }
        let mut resolved_storage =
            ctx.reserve_scoped(0, "hold F3D generic surface patch distinct slots")?;
        let mut resolved_edges = Vec::new();
        let mut surface_slot_source = edges.iter();
        while let Some(&edge_slot) = ctx.next_charged(&mut surface_slot_source, "f3d edge projection source")? {
            if !ctx.contains(
                &resolved_edges,
                &edge_slot,
                "find F3D surface patch resolved edge",
            )? {
                ctx.push_scoped_vec(
                    &mut resolved_storage,
                    &mut resolved_edges,
                    edge_slot,
                    "f3d generic surface patch distinct slot",
                )?;
            }
        }
        drop(edges);
        drop(edge_storage);
        if resolved_edges.is_empty() {
            return unmatched_selection();
        }
        let mut historical_edges = Vec::new();
    let (prefix, _prefix_storage) = ctx.with_scoped_storage("f3d historical edge prefix storage", || crate::design::identity::history_input_prefix(ctx, feature_key, state_id))?;
        let mut surface_historical_slot_source = resolved_edges.iter();
        while let Some(&edge_slot) = ctx.next_charged(&mut surface_historical_slot_source, "f3d edge projection source")? {
            let edge = crate::design::identity::history_input_edge_id(
                ctx,
                &prefix,
                edge_slot,
                "f3d historical edge identifier",
            )?;
            ctx.push_vec(
                &mut historical_edges,
                edge,
                "f3d generic surface patch historical edge",
            )?;
        }
        drop(resolved_edges);
        drop(resolved_storage);
        let native =
            ctx.copy_retained_text(&group.id, "f3d generic surface patch historical group id")?;
        return match cadmpeg_ir::features::EdgeSelection::historical(
            crate::design::identity::feature_input_topology_id(ctx, feature_id, state_id)?,
            historical_edges,
            native,
            ctx,
        )? {
            Ok(selection) => Ok(selection),
            Err(_) => native_edge_selection(group, ctx),
        };
    }
    let mut identity_storage = ctx.reserve_scoped(0, "hold F3D matched edge identities")?;
    let mut identity_matches = Vec::new();
    let mut identities_complete = true;
    let mut identity_member_source = members.iter().map(|member| &member.value);
    while let Some(member) = ctx.next_charged(&mut identity_member_source, "f3d edge projection source")? {
        let matches = unique_feature_match_for_decode(ctx, identity_operands.iter(), |operand| Ok(ctx.equal(&native_stream_charged(ctx, &operand.id, "f3d edge native stream")?, &stream, "f3d edge stream comparison")?
                && operand.scope_record_index == group.scope_record_index
                && operand.group_record_index == group.record_index
                && operand.record_index() == *member), "f3d unique edge operand")?;
        let Some(operand) = matches else {
            identities_complete = false;
            break;
        };
        ctx.push_scoped_vec(
            &mut identity_storage,
            &mut identity_matches,
            operand,
            "f3d edge group matched identity",
        )?;
    }
    let identity_matches = if identities_complete {
        Some(identity_matches)
    } else {
        drop(identity_matches);
        drop(identity_storage);
        None
    };
    let has_recipe_operands = ctx.all_by(members, |member| {
        Ok(unique_feature_match_for_decode(ctx, operands.iter(), |operand| Ok(
            ctx.equal(&native_stream_charged(ctx, &operand.id, "f3d edge native stream")?, &stream, "f3d edge stream comparison")?
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index() == member.value
        ), "f3d unique edge operand")?.is_some())
    }, "f3d edge recipe member source")?;
    let has_unstructured_recipe_operand = ctx.any_by(members, |member| {
        ctx.any_by(operands, |operand| Ok(
            ctx.equal(&native_stream_charged(ctx, &operand.id, "f3d edge native stream")?, &stream, "f3d edge stream comparison")?
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index() == member.value
                && !operand.recipe_program.is_empty()
                && operand.recipe_structure.is_none()
        ), "f3d unstructured edge operand source")
    }, "f3d unstructured edge member source")?;
    if has_recipe_operands && has_unstructured_recipe_operand {
        return unmatched_selection();
    }
    let has_standard_recipe_operands = ctx.any_by(members, |member| {
        ctx.any_by(operands, |operand| Ok(
            ctx.equal(&native_stream_charged(ctx, &operand.id, "f3d edge native stream")?, &stream, "f3d edge stream comparison")?
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index() == member.value
                && operand.recipe_structure.is_some()
        ), "f3d standard edge operand source")
    }, "f3d standard edge member source")?;
    let has_concrete_recipe_evidence = ctx.any_by(members, |member| {
        Ok(unique_feature_match_for_decode(ctx, operands.iter(), |operand| Ok(
            ctx.equal(&native_stream_charged(ctx, &operand.id, "f3d edge native stream")?, &stream, "f3d edge stream comparison")?
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index() == member.value
        ), "f3d unique edge operand")?.is_some_and(|operand| {
            operand.resolved_edge_slot.is_some()
                || !operand.changed_boundary_edge_slots.is_empty()
                || !operand.deleted_boundary_edge_slots.is_empty()
                || !operand.treatment_radius_candidates.is_empty()
        }))
    }, "f3d concrete edge recipe member source")?;
    let mut transition_storage = ctx.reserve_scoped(0, "hold F3D single identity transition")?;
    let identity_transition_slots = if allow_edge_treatment_transition_chain
        && treatment_radius.is_none()
        && members.len() == 1
    {
        match identity_matches.as_deref() {
            Some([operand]) => {
                let edges = transition_storage.with_storage(|| {
                    sorted_transition_slots(
                        &operand.transition_edge_candidates,
                        ctx,
                        "f3d single identity transition slot",
                    )
                })?;
                (!edges.is_empty()).then_some(edges)
            }
            _ => None,
        }
    } else {
        None
    };
    let mut group_transition_storage = ctx.reserve_scoped(0, "hold F3D group identity transition")?;
    let group_transition_slots = match (
        allow_edge_treatment_transition_chain,
        identity_matches.as_ref().and_then(|matches| matches.first()),
        identity_transition_slots.as_ref(),
    ) {
        (true, Some(first), None) if first.layout().is_compact() => {
            Some(group_transition_storage.with_storage(|| sorted_transition_slots(
                &first.transition_edge_candidates, ctx, "f3d group identity transition slot",
            ))?)
        }
        _ => None,
    };
    let first_transition_slots = identity_transition_slots.as_deref().or(group_transition_slots.as_deref());
    let identity_group_transition_slots = match (allow_edge_treatment_transition_chain, identity_matches.as_ref(), first_transition_slots) {
        (true, Some(matches), Some(edges)) if !edges.is_empty() && matches.first().is_some_and(|first| first.layout().is_compact()) => {
            let uniform = ctx.all_by(&matches[1..], |operand| {
                if !operand.layout().is_compact() { return Ok(false); }
                let (candidate, _comparison_storage) = ctx.with_scoped_storage("hold F3D compared identity transition", || {
                    sorted_transition_slots(&operand.transition_edge_candidates, ctx, "f3d compared identity transition slot")
                })?;
                Ok(candidate.len() == edges.len() && ctx.all_by(candidate.iter().zip(edges), |(candidate, edge)| ctx.equal(candidate, edge, "compare F3D identity transition edge"), "scan F3D identity transition equality")?)
            }, "f3d uniform identity transition source")?;
            uniform.then_some(edges)
        }
        _ => None,
    };
    let recipe_supports_transition_chain = |chain: &[i64]| -> Result<bool, CodecError> {
        let mut recipe_storage = ctx.reserve_scoped(0, "hold F3D transition recipe members")?;
        let mut member_operands = Vec::new();
        let mut recipe_member_source = members.iter().map(|member| &member.value);
        while let Some(member) = ctx.next_charged(&mut recipe_member_source, "f3d edge projection source")? {
            let matches = unique_feature_match_for_decode(ctx, operands.iter(), |operand| Ok(ctx.equal(&native_stream_charged(ctx, &operand.id, "f3d edge native stream")?, &stream, "f3d edge stream comparison")?
                    && operand.scope_record_index == group.scope_record_index
                    && operand.record_index() == *member), "f3d unique edge operand")?;
            let Some(operand) = matches else {
                return Ok(false);
            };
            ctx.push_scoped_vec(
                &mut recipe_storage,
                &mut member_operands,
                operand,
                "f3d transition recipe member",
            )?;
        }
        transition_chain_is_supported_by_recipe(chain, members.len(), &member_operands, ctx)
    };
    let identity_transition_is_supported = match identity_transition_slots
        .as_deref()
        .or(identity_group_transition_slots)
    {
        Some(chain) => recipe_supports_transition_chain(chain)?,
        None => false,
    };
    // A cardinality mismatch is a group-level transition proof, not a
    // member-to-edge assignment. Recipe evidence must therefore cover the
    // complete chain before it can replace the unresolved member identities.
    let identity_group_transition_is_admitted = match identity_group_transition_slots {
        // A singleton transition and the uniform group transition both use
        // the first identity's sorted slots. Otherwise the selected chain is
        // the group transition itself. Its recipe proof was already computed.
        Some(edges) => edges.len() == members.len() || identity_transition_is_supported,
        None => false,
    };
    let mut radius_storage = ctx.reserve_scoped(0, "hold F3D identity radius proof")?;
    let identity_radius_slots = match (treatment_radius, identity_matches.as_ref()) {
        (Some(radius), Some(matches)) => radius_storage
            .with_storage(|| radius_edge_identity_group_candidates(matches, radius, ctx))?,
        _ => None,
    };
    let has_complete_identity_selection = match identity_matches.as_ref() {
        Some(operands) if !operands.is_empty() => {
            ctx.all_by(operands, |operand| Ok(operand.resolved_edge_slot.is_some() || !operand.resolved_edge_slots.is_empty()), "f3d complete identity edge source")?
                || identity_transition_slots.is_some()
                || identity_group_transition_is_admitted
                || identity_radius_slots.is_some()
        }
        _ => false,
    };
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
            return unmatched_selection();
        }
        let Some(previous_state_id) = previous_state_id else {
            return unmatched_selection();
        };
        let state =
            crate::design::identity::feature_input_topology_id(ctx, feature_id, previous_state_id)?;
        if ctx.all_by(identity_matches, |operand| Ok(operand.resolved_edge_slot.is_some() || !operand.resolved_edge_slots.is_empty()), "f3d selected identity edge source")? {
            let mut seen_storage = ctx.reserve_scoped(0, "hold F3D identity edge slot index")?;
            let mut seen = HashSet::new();
            let mut edges = Vec::new();
    let (prefix, _prefix_storage) = ctx.with_scoped_storage("f3d historical edge prefix storage", || crate::design::identity::history_input_prefix(ctx, feature_key, previous_state_id))?;
            let mut identity_source = identity_matches.iter();
            while let Some(operand) = ctx.next_charged(&mut identity_source, "f3d identity operand source")? {
                let mut slot_source = operand.resolved_edge_slot.iter().chain(&operand.resolved_edge_slots).copied();
                while let Some(edge_slot) = ctx.next_charged(&mut slot_source, "f3d identity resolved slot source")? {
                if seen_storage.with_storage(|| {
                    ctx.insert_hash_set(&mut seen, edge_slot, "f3d identity edge slot index")
                })? {
                    let edge = crate::design::identity::history_input_edge_id(
                        ctx,
                        &prefix,
                        edge_slot,
                        "f3d historical edge identifier",
                    )?;
                    ctx.push_vec(&mut edges, edge, "f3d identity historical edge")?;
                }
                }
            }
            let native = ctx.copy_retained_text(&group.id, "f3d identity historical group id")?;
            return match cadmpeg_ir::features::EdgeSelection::historical(state, edges, native, ctx)?
            {
                Ok(selection) => Ok(selection),
                Err(_) => native_edge_selection(group, ctx),
            };
        }
        if let Some(edges) = identity_radius_slots.as_ref() {
            return historical_identity_slots(
                group,
                state,
                feature_key,
                previous_state_id,
                edges,
                ctx,
                (
                    "f3d radius identity historical edge",
                    "f3d radius identity historical group id",
                ),
            );
        }
        if let Some(edges) = identity_group_transition_slots.as_ref() {
            return historical_identity_slots(
                group,
                state,
                feature_key,
                previous_state_id,
                edges,
                ctx,
                (
                    "f3d group identity historical edge",
                    "f3d group identity historical group id",
                ),
            );
        }
        if identity_matches.len() == 1 && identity_matches[0].resolved_edge_slot.is_none() {
            if let Some(edges) = identity_transition_slots.as_ref() {
                return historical_identity_slots(
                    group,
                    state,
                    feature_key,
                    previous_state_id,
                    edges,
                    ctx,
                    (
                        "f3d single identity historical edge",
                        "f3d single identity historical group id",
                    ),
                );
            }
        }
        return match partial_historical_edge_selection(
            identity_matches
                .iter()
                .map(|operand| (operand.id.as_str(), operand.resolved_edge_slot)),
            previous_state_id,
            feature_key,
            state,
            &group.id,
            ctx,
        )? {
            Some(selection) => Ok(selection),
            None => native_edge_selection(group, ctx),
        };
    }
    let mut matched_storage = ctx.reserve_scoped(0, "hold F3D matched edge operands")?;
    let mut matched_buffer = Vec::new();
    let mut single_operand = None;
    let singleton;
    let mut member_storage = ctx.reserve_scoped(0, "hold F3D edge group member identities")?;
    let mut member_identities = HashSet::new();
    let mut recipe_operand_member_source = members.iter().map(|member| &member.value);
    while let Some(member) = ctx.next_charged(&mut recipe_operand_member_source, "f3d edge projection source")? {
        if !member_storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut member_identities,
                *member,
                "f3d edge group member identity",
            )
        })? {
            return unmatched_selection();
        }
        let matches = unique_feature_match_for_decode(ctx, operands.iter(), |operand| Ok(ctx.equal(&native_stream_charged(ctx, &operand.id, "f3d edge native stream")?, &stream, "f3d edge stream comparison")?
                && operand.scope_record_index == group.scope_record_index
                && operand.record_index() == *member), "f3d unique edge operand")?;
        let Some(operand) = matches else {
            return unmatched_selection();
        };
        if members.len() == 1 {
            ctx.charge_collection_items(1, "f3d matched edge group operand")?;
            single_operand = Some(operand);
        } else {
            ctx.push_scoped_vec(
                &mut matched_storage,
                &mut matched_buffer,
                operand,
                "f3d matched edge group operand",
            )?;
        }
    }
    let matched_operands = if let Some(operand) = single_operand {
        singleton = [operand];
        singleton.as_slice()
    } else {
        matched_buffer.as_slice()
    };
    let transition_state_id = previous_state_id;
    let state_id = match transition_state_id {
        Some(state_id) => Some(state_id),
        None => shared_recipe_state_id(matched_operands, ctx)?,
    };
    let Some(previous_state_id) = state_id else {
        return if group.lost_edge_references.is_empty() {
            native_edge_selection(group, ctx)
        } else {
            Ok(EdgeSelection::Unresolved)
        };
    };
    let state =
        crate::design::identity::feature_input_topology_id(ctx, feature_id, previous_state_id)?;
    let lost_selection = || unmatched_selection();
    let mut resolved_slot_storage = ctx.reserve_scoped(0, "hold F3D resolved edge group slots")?;
    let mut exact_slots = {
        let mut direct_storage = ctx.reserve_scoped(0, "hold F3D direct edge group slots")?;
        let mut direct_slots = Some(Vec::new());
        let mut recipe_operand_source = matched_operands.iter();
        while let Some(operand) = ctx.next_charged(&mut recipe_operand_source, "f3d edge projection source")? {
            let Some(slot) = resolved_edge_operand(operand, ctx)? else {
                direct_slots = None;
                break;
            };
            if let Some(slots) = direct_slots.as_mut() {
                ctx.push_scoped_vec(
                    &mut direct_storage,
                    slots,
                    slot,
                    "f3d exact edge group slot",
                )?;
            }
        }
        match direct_slots {
            Some(slots) => {
                Some(resolved_slot_storage.with_storage(|| direct_storage.commit_value(slots))?)
            }
            None => None,
        }
    };
    if exact_slots.is_none() {
        exact_slots = resolved_slot_storage
            .with_storage(|| unique_edge_group_assignment(matched_operands, ctx))?;
    }
    if exact_slots.is_none() {
        exact_slots = resolved_slot_storage
            .with_storage(|| changed_reference_edge_group_candidates(matched_operands, ctx))?;
    }
    let transition_slots = || -> Result<Option<Vec<i64>>, CodecError> {
        let mut slots = match treatment_radius {
            Some(radius) => radius_edge_group_candidates(matched_operands, radius, ctx)?,
            None => None,
        };
        if slots.is_none() {
            slots = match (treatment_radius, identity_matches.as_ref()) {
                (Some(radius), Some(operands)) => {
                    radius_edge_identity_group_candidates(operands, radius, ctx)?
                }
                _ => None,
            };
        }
        if slots.is_none() {
            slots = context_only_edge_group_candidates(
                matched_operands.iter().map(|operand| {
                    Ok((
                        resolved_edge_operand(operand, ctx)?,
                        operand.changed_boundary_edge_slots.as_slice(),
                    ))
                }),
                ctx,
            )?;
        }
        if slots.is_none() {
            slots = changed_boundary_count_edge_group_candidates(
                matched_operands
                    .iter()
                    .map(|operand| operand.recipe_selectors.as_slice()),
                ctx,
            )?;
        }
        if slots.is_none() {
            slots = deleted_reference_edge_group_candidates(matched_operands, ctx)?;
        }
        if slots.is_none() {
            slots = common_deleted_edge_group_candidates(
                matched_operands.iter().map(|operand| {
                    (
                        !operand.changed_boundary_edge_slots.is_empty(),
                        operand.deleted_boundary_edge_slots.as_slice(),
                    )
                }),
                ctx,
            )?;
        }
        if slots.is_none() && allow_edge_treatment_transition_chain {
            slots = contextual_deleted_edge_group_candidates(matched_operands, ctx)?;
        }
        if slots.is_none() && allow_edge_treatment_transition_chain {
            slots = result_boundary_reference_edge_group_candidates(matched_operands, ctx)?;
        }
        if slots.is_none() && allow_edge_treatment_transition_chain {
            slots = deleted_boundary_edge_group_candidates(matched_operands, ctx)?;
        }
        if slots.is_none() && groups.len() > 1 {
            slots = scope_partition_edge_group_candidates(group, groups, operands, members, ctx)?;
        }
        Ok(slots)
    };
    let resolved_slots =
        if exact_slots.is_some() || has_standard_recipe_operands || transition_state_id.is_none() {
            exact_slots
        } else {
            resolved_slot_storage.with_storage(transition_slots)?
        };
    let Some(resolved_slots) = resolved_slots else {
        if !group.lost_edge_references.is_empty() {
            return lost_selection();
        }
        if has_standard_recipe_operands {
            return native_edge_selection(group, ctx);
        }
        // A single unresolved recipe without a persistent identity contributes
        // no proved edge. A partial historical selection requires a proved
        // edge and an unresolved member, so this lane remains native.
        if matched_operands.len() == 1 && identity_matches.is_none() {
            return native_edge_selection(group, ctx);
        }
        let mut combined_storage = ctx.reserve_scoped(0, "hold F3D combined edge slots")?;
        let mut combined_edges = Vec::new();
        let mut combined_operand_source = matched_operands.iter().enumerate();
        while let Some((index, operand)) = ctx.next_charged(&mut combined_operand_source, "f3d edge projection source")? {
            let recipe = resolved_edge_operand(operand, ctx)?;
            let identity = identity_matches
                .as_ref()
                .and_then(|identities| identities[index].resolved_edge_slot);
            let combined = match (recipe, identity) {
                (Some(recipe), Some(identity)) if recipe != identity => {
                    return unmatched_selection();
                }
                (recipe, identity) => recipe.or(identity),
            };
            ctx.push_scoped_vec(
                &mut combined_storage,
                &mut combined_edges,
                combined,
                "f3d combined edge group slot",
            )?;
        }
        if ctx.all_by(&combined_edges, |edge| Ok(edge.is_some()), "f3d combined edge selection source")? {
            let mut edges = Vec::new();
    let mut prefix_storage = ctx.reserve_scoped(0, "f3d historical edge prefix storage")?;
    if !combined_edges.is_empty() {
    let prefix = prefix_storage.with_storage(|| crate::design::identity::history_input_prefix(ctx, feature_key, previous_state_id))?;
    let mut seen_storage = ctx.reserve_scoped(0, "f3d combined historical slot storage")?;
    let mut seen_slots = HashSet::new();
            let mut combined_slot_source = combined_edges.into_iter().flatten();
            while let Some(edge_slot) = ctx.next_charged(&mut combined_slot_source, "f3d edge projection source")? {
        if !seen_storage.with_storage(|| ctx.insert_hash_set(&mut seen_slots, edge_slot, "f3d combined historical slot index"))? { continue; }
                let edge = crate::design::identity::history_input_edge_id(
                    ctx,
                    &prefix,
                    edge_slot,
                    "f3d historical edge identifier",
                )?;
        ctx.push_vec(&mut edges, edge, "f3d combined historical edge")?;
    }
    }
            drop(combined_storage);
            let native = ctx.copy_retained_text(&group.id, "f3d combined historical group id")?;
            return match cadmpeg_ir::features::EdgeSelection::historical(state, edges, native, ctx)?
            {
                Ok(selection) => Ok(selection),
                Err(_) => native_edge_selection(group, ctx),
            };
        }
        let mut partial_storage = ctx.reserve_scoped(0, "hold F3D partial edge members")?;
        let mut partial_members = Vec::new();
        let mut partial_operand_source = matched_operands.iter().zip(combined_edges);
        while let Some((operand, resolved)) = ctx.next_charged(&mut partial_operand_source, "f3d edge projection source")? {
            let carries_transition_evidence = identity_matches.is_some()
                || transition_state_id.is_none()
                || !operand.changed_boundary_edge_slots.is_empty();
            if resolved.is_some() || carries_transition_evidence {
                ctx.push_scoped_vec(
                    &mut partial_storage,
                    &mut partial_members,
                    (operand.id.as_str(), resolved),
                    "f3d partial edge group member",
                )?;
            }
        }
        drop(combined_storage);
        let partial = partial_historical_edge_selection(
            partial_members,
            previous_state_id,
            feature_key,
            state,
            &group.id,
            ctx,
        )?;
        drop(partial_storage);
        return match partial {
            Some(selection) => Ok(selection),
            None => native_edge_selection(group, ctx),
        };
    };
    let mut edges = Vec::new();
    let mut prefix_storage = ctx.reserve_scoped(0, "f3d historical edge prefix storage")?;
    if !resolved_slots.is_empty() {
    let prefix = prefix_storage.with_storage(|| crate::design::identity::history_input_prefix(ctx, feature_key, previous_state_id))?;
    let mut seen_storage = ctx.reserve_scoped(0, "f3d resolved historical slot storage")?;
    let mut seen_slots = HashSet::new();
    let mut resolved_slot_source = resolved_slots.iter();
    while let Some(&edge_slot) = ctx.next_charged(&mut resolved_slot_source, "f3d edge projection source")? {
        if !seen_storage.with_storage(|| ctx.insert_hash_set(&mut seen_slots, edge_slot, "f3d resolved historical slot index"))? { continue; }
        let edge = crate::design::identity::history_input_edge_id(
            ctx,
            &prefix,
            edge_slot,
            "f3d historical edge identifier",
        )?;
ctx.push_vec(&mut edges, edge, "f3d resolved edge group historical edge")?;
    }
    }
    drop(resolved_slots);
    drop(resolved_slot_storage);
    if edges.is_empty() {
        native_edge_selection(group, ctx)
    } else {
        let native =
            ctx.copy_retained_text(&group.id, "f3d resolved edge group historical group id")?;
        match cadmpeg_ir::features::EdgeSelection::historical(state, edges, native, ctx)? {
            Ok(selection) => Ok(selection),
            Err(_) => native_edge_selection(group, ctx),
        }
    }
}

pub(super) fn resolved_hem_edge_group(
    group: &DesignConstructionOperandGroup,
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignEdgeOperand],
    identity_operands: &[DesignEdgeIdentityOperand],
    previous_state_id: Option<i64>,
    feature_id: &cadmpeg_ir::features::FeatureId,
    ctx: &DecodeContext<'_>,
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
    let stream = native_stream_charged(ctx, &group.id, "f3d edge native stream")?;
    let matching_operand = unique_feature_match_for_decode(ctx, operands.iter(), |operand| Ok(ctx.equal(&native_stream_charged(ctx, &operand.id, "f3d edge native stream")?, &stream, "f3d edge stream comparison")?
            && operand.scope_record_index == group.scope_record_index
            && operand.record_index() == *member), "f3d unique Hem edge operand")?;
    let Some(operand) = matching_operand else {
        return Ok(selection);
    };
    let Some(edge) = hem_transition_edge_slot(operand, ctx)? else {
        return Ok(selection);
    };
    let feature_key = crate::design::identity::identity_key(ctx, feature_id.as_str())?;
    let (prefix, _prefix_storage) = ctx.with_scoped_storage("f3d historical edge prefix storage", || crate::design::identity::history_input_prefix(ctx, feature_key, previous_state_id))?;
    let mut edges = ctx.collection_vec(1, "f3d Hem historical edge")?;
    edges.push(crate::design::identity::history_input_edge_id(ctx, &prefix, edge, "f3d historical edge identifier")?);
    Ok(cadmpeg_ir::features::EdgeSelection::historical(
        crate::design::identity::feature_input_topology_id(ctx, feature_id, previous_state_id)?,
        edges,
        ctx.copy_retained_text(&group.id, "f3d hem historical group id")?,
        ctx,
    )?
    .unwrap_or(selection))
}

/// Return the one historical edge a single-member Hem operand identifies.
///
/// A directly resolved operand is preferred. The transition proof is the
/// fallback used by the compact recipe form, where the operand carries only
/// the changed support boundaries and the selectorless edge context.
pub(super) fn resolved_hem_edge_slot(
    operand: &DesignEdgeOperand,
    previous_state_id: Option<i64>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<i64>, CodecError> {
    if let Some(edge) = operand.resolved_edge_slot {
        return Ok(Some(edge));
    }
    if previous_state_id.is_none() {
        return Ok(None);
    }
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
fn transition_chain_is_supported_by_recipe(
    chain: &[i64],
    member_count: usize,
    operands: &[&DesignEdgeOperand],
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if operands.len() != member_count {
        return Ok(false);
    }
    let mut storage = ctx.reserve_scoped(0, "hold F3D transition recipe proof")?;
    let mut all_recipe_edges = Vec::new();
    let mut source = operands.iter();
    while !source.as_slice().is_empty() {
        let Some(operand) = ctx.next_charged(&mut source, "scan F3D transition recipe operands")?
        else {
            break;
        };
        let resolved = resolved_edge_operand(operand, ctx)?;
        if let Some(edge) = resolved {
            if ctx.contains(
                &operand.deleted_boundary_edge_slots,
                &edge,
                "find F3D transition deleted recipe edge",
            )? && !ctx.contains(chain, &edge, "find F3D transition resolved recipe edge")?
            {
                return Ok(false);
            }
        }
        if !operand.deleted_boundary_edge_slots.is_empty()
            && !ctx.any_by(
                &operand.deleted_boundary_edge_slots,
                |edge| ctx.contains(chain, edge, "find F3D transition deleted chain edge"),
                "scan F3D transition deleted recipe edges",
            )?
        {
            return Ok(false);
        }
        for boundary in [
            operand.changed_boundary_edge_slots.as_slice(),
            operand.deleted_boundary_edge_slots.as_slice(),
        ] {
            let mut edges = boundary.iter();
            while !edges.as_slice().is_empty() {
                let Some(&edge) =
                    ctx.next_charged(&mut edges, "scan F3D transition boundary edges")?
                else {
                    break;
                };
                ctx.push_scoped_vec(
                    &mut storage,
                    &mut all_recipe_edges,
                    edge,
                    "f3d transition recipe edge",
                )?;
            }
        }
        let mut references = edge_operand_reference_edge_sets(operand);
        while references.len() != 0 {
            let Some(edges) =
                ctx.next_charged(&mut references, "scan F3D transition reference sources")?
            else {
                break;
            };
            let Some(edges) = edges else {
                continue;
            };
            let mut edges = edges.iter();
            while !edges.as_slice().is_empty() {
                let Some(&edge) =
                    ctx.next_charged(&mut edges, "scan F3D transition reference edges")?
                else {
                    break;
                };
                ctx.push_scoped_vec(
                    &mut storage,
                    &mut all_recipe_edges,
                    edge,
                    "f3d transition recipe edge",
                )?;
            }
        }
    }
    ctx.sort_unstable_by(
        &mut all_recipe_edges,
        |value| value,
        Ord::cmp,
        "f3d transition recipe edge sort",
    )?;
    ctx.dedup_vec(
        &mut all_recipe_edges,
        "deduplicate F3D transition recipe edges",
    )?;
    Ok(all_recipe_edges.is_empty()
        || ctx.all_by(
            chain,
            |edge| ctx.contains(&all_recipe_edges, edge, "find F3D transition covered edge"),
            "scan F3D transition chain coverage",
        )?)
}

fn hem_transition_edge_slot(
    operand: &DesignEdgeOperand,
    ctx: &DecodeContext<'_>,
) -> Result<Option<i64>, CodecError> {
    let reference_contexts = operand.recipe_reference_contexts.as_slice();
    let mut remaining = reference_contexts.iter();
    let mut has_empty_context = false;
    while !remaining.as_slice().is_empty() {
        let Some(context) =
            ctx.next_charged(&mut remaining, "scan F3D Hem reference eligibility")?
        else {
            break;
        };
        if context.changed_reference_edge_slots.is_empty() {
            if has_empty_context
                || !context.result_faces.is_empty()
                || !context.preceding_faces.is_empty()
                || !context.preceding_support_face_slots.is_empty()
            {
                return Ok(None);
            }
            has_empty_context = true;
        } else if context.result_faces.is_empty() || context.preceding_support_face_slots.is_empty()
        {
            return Ok(None);
        }
    }
    if !has_empty_context {
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
    ctx: &DecodeContext<'_>,
) -> Result<Option<i64>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "hold F3D Hem reference candidates")?;
    let mut reference_edge_sets: Vec<&'a [i64]> = Vec::new();
    let mut source = reference_edge_sets_input.into_iter();
    let mut empty_count = 0;
    while source.size_hint().1 != Some(0) {
        let Some(edges) = ctx.next_charged(&mut source, "scan F3D Hem reference set sources")?
        else {
            break;
        };
        empty_count += usize::from(edges.is_empty());
        ctx.push_scoped_vec(
            &mut storage,
            &mut reference_edge_sets,
            edges,
            "f3d hem reference edge set",
        )?;
    }
    if reference_edge_sets.is_empty() || empty_count != 1 {
        return Ok(None);
    }
    let mut support_edges = Vec::new();
    let mut sets = reference_edge_sets.iter();
    while !sets.as_slice().is_empty() {
        let Some(edges) = ctx.next_charged(&mut sets, "scan F3D Hem reference support sets")?
        else {
            break;
        };
        let mut edges = edges.iter();
        while !edges.as_slice().is_empty() {
            let Some(&edge) =
                ctx.next_charged(&mut edges, "scan F3D Hem reference support edges")?
            else {
                break;
            };
            ctx.push_scoped_vec(
                &mut storage,
                &mut support_edges,
                edge,
                "f3d hem support edge",
            )?;
        }
    }
    ctx.sort_unstable_by(
        &mut support_edges,
        |value| value,
        Ord::cmp,
        "f3d hem support edge sort",
    )?;
    ctx.dedup_vec(&mut support_edges, "deduplicate F3D Hem support edges")?;
    if support_edges.is_empty()
        || ctx.any_by(
            &support_edges,
            |edge| {
                ctx.contains(
                    changed_boundary_edges,
                    edge,
                    "find F3D Hem changed support edge",
                )
                .map(|found| !found)
            },
            "scan F3D Hem support membership",
        )?
    {
        return Ok(None);
    }
    let mut candidates = Vec::new();
    let mut changed = changed_boundary_edges.iter();
    while !changed.as_slice().is_empty() {
        let Some(&edge) = ctx.next_charged(&mut changed, "scan F3D Hem changed candidate edges")?
        else {
            break;
        };
        if !ctx.contains(&support_edges, &edge, "find F3D Hem support candidate")? {
            ctx.push_scoped_vec(
                &mut storage,
                &mut candidates,
                edge,
                "f3d hem candidate edge",
            )?;
        }
    }
    ctx.sort_unstable_by(
        &mut candidates,
        |value| value,
        Ord::cmp,
        "f3d hem candidate edge sort",
    )?;
    ctx.dedup_vec(&mut candidates, "deduplicate F3D Hem candidate edges")?;
    match candidates.as_slice() {
        [edge] => Ok(Some(*edge)),
        _ => Ok(None),
    }
}

fn partial_historical_edge_selection<'a>(
    members: impl IntoIterator<Item = (&'a str, Option<i64>)>,
    previous_state_id: i64,
    feature_key: &str,
    state: cadmpeg_ir::ids::FeatureInputTopologyId,
    native: &str,
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::EdgeSelection>, CodecError> {
    use cadmpeg_ir::features::EdgeSelection;

    let mut slot_storage = ctx.reserve_scoped(0, "hold F3D partial historical scalar slots")?;
    let mut edges = Vec::new();
    let mut unresolved = Vec::new();
    let mut member_source = members.into_iter();
    while let Some((identity, edge)) = ctx.next_charged(&mut member_source, "f3d edge projection source")? {
        if let Some(edge) = edge {
            if !ctx.contains(&edges, &edge, "find F3D partial edge slot")? {
                ctx.push_scoped_vec(&mut slot_storage, &mut edges, edge, "f3d partial edge slot")?;
            }
        } else {
            ctx.push_scoped_vec(&mut slot_storage, &mut unresolved, identity, "f3d partial unresolved member")?;
        }
    }
    if unresolved.is_empty() || edges.is_empty() {
        return Ok(None);
    }
    let unresolved = ctx.try_collect_vec(
        unresolved.into_iter().map(|identity| ctx.copy_retained_text(identity, "f3d partial unresolved id")),
        "f3d partial unresolved member",
    )?;
    let mut historical_edges = Vec::new();
    let (prefix, _prefix_storage) = ctx.with_scoped_storage("f3d historical edge prefix storage", || crate::design::identity::history_input_prefix(ctx, feature_key, previous_state_id))?;
    let mut historical_slot_source = edges.iter();
    while let Some(&edge_slot) = ctx.next_charged(&mut historical_slot_source, "f3d edge projection source")? {
        let edge = crate::design::identity::history_input_edge_id(
            ctx,
            &prefix,
            edge_slot,
            "f3d historical edge identifier",
        )?;
        ctx.push_vec(&mut historical_edges, edge, "f3d partial historical edge")?;
    }
    drop(edges);
    drop(slot_storage);
    let native_id = ctx.copy_retained_text(native, "f3d partial native id")?;
    Ok(Some(
        match cadmpeg_ir::features::EdgeSelection::historical_partial(
            state,
            historical_edges,
            unresolved,
            native_id,
            ctx,
        )? {
            Ok(selection) => selection,
            Err(_) => EdgeSelection::Native(
                ctx.copy_retained_text(native, "f3d partial native fallback id")?,
            ),
        },
    ))
}

fn context_only_edge_group_candidates<'a>(
    members: impl IntoIterator<Item = Result<(Option<i64>, &'a [i64]), CodecError>>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "hold F3D context-only edge candidates")?;
    let mut edges = Vec::new();
    let mut members = members.into_iter();
    while let Some(member) = ctx.next_charged(&mut members, "scan F3D context-only edge members")? {
        let (resolved, changed_candidates) = member?;
        match resolved {
            Some(edge) => {
                if !ctx.contains(&edges, &edge, "find F3D context-only edge candidate")? {
                    ctx.push_scoped_vec(
                        &mut storage,
                        &mut edges,
                        edge,
                        "f3d context-only edge candidate",
                    )?;
                }
            }
            None if changed_candidates.is_empty() => {}
            None => return Ok(None),
        }
    }
    if edges.is_empty() {
        return Ok(None);
    }
    storage.commit_value(Some(edges))
}

fn unique_edge_group_assignment(
    operands: &[&DesignEdgeOperand],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if operands.is_empty() {
        return Ok(None);
    }
    let mut storage = ctx.reserve_scoped(0, "hold F3D unique edge group candidates")?;
    let mut candidate_sets = Vec::new();
    let mut source = operands.iter();
    while !source.as_slice().is_empty() {
        let Some(operand) = ctx.next_charged(&mut source, "scan F3D unique edge group operands")?
        else {
            break;
        };
        let candidates = storage.with_storage(|| -> Result<_, CodecError> {
            if let Some(edge) = resolved_edge_operand(operand, ctx)? {
                let mut edges = ctx.collection_vec(1, "f3d unique resolved edge candidate")?;
                edges.push(edge);
                Ok(Some(EdgeAssignmentCandidates::Edges(edges)))
            } else {
                edge_group_assignment_candidates(
                    &operand.recipe_selectors,
                    edge_operand_reference_edge_sets(operand),
                    ctx,
                )
            }
        })?;
        let Some(candidates) = candidates else {
            return Ok(None);
        };
        if matches!(candidates, EdgeAssignmentCandidates::Context) {
            continue;
        }
        ctx.push_scoped_vec(
            &mut storage,
            &mut candidate_sets,
            candidates,
            "f3d unique edge candidate set",
        )?;
    }
    unique_edge_assignment_with_context(&candidate_sets, ctx)
}

pub(super) fn changed_reference_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "hold F3D changed reference candidates")?;
    let mut candidate_sets = Vec::new();
    let mut source = operands.iter();
    while !source.as_slice().is_empty() {
        let Some(operand) = ctx.next_charged(&mut source, "scan F3D changed reference operands")?
        else {
            break;
        };
        let mut contexts = operand.recipe_reference_contexts.iter();
        let mut first = None;
        while !contexts.as_slice().is_empty() {
            let Some(context) =
                ctx.next_charged(&mut contexts, "scan F3D changed reference contexts")?
            else {
                break;
            };
            if !context.changed_reference_edge_slots.is_empty() {
                first = Some(context.changed_reference_edge_slots.as_slice());
                break;
            }
        }
        let Some(first) = first else {
            return Ok(None);
        };
        let mut candidates = Vec::new();
        let mut first_edges = first.iter();
        while !first_edges.as_slice().is_empty() {
            let Some(&edge) =
                ctx.next_charged(&mut first_edges, "scan F3D first changed reference edges")?
            else {
                break;
            };
            ctx.push_scoped_vec(
                &mut storage,
                &mut candidates,
                edge,
                "f3d changed reference candidate",
            )?;
        }
        while !contexts.as_slice().is_empty() {
            let Some(context) =
                ctx.next_charged(&mut contexts, "scan F3D changed reference contexts")?
            else {
                break;
            };
            let changed = context.changed_reference_edge_slots.as_slice();
            if changed.is_empty() {
                continue;
            }
            ctx.retain_vec(
                &mut candidates,
                |candidate| {
                    ctx.contains(changed, candidate, "find F3D changed reference candidate")
                },
                "retain F3D changed reference candidates",
            )?;
            if candidates.is_empty() {
                return Ok(None);
            }
        }
        ctx.sort_unstable_by(
            &mut candidates,
            |value| value,
            Ord::cmp,
            "f3d changed reference candidate sort",
        )?;
        ctx.dedup_vec(&mut candidates, "deduplicate F3D changed reference edges")?;
        if candidates.is_empty() {
            return Ok(None);
        }
        ctx.push_scoped_vec(
            &mut storage,
            &mut candidate_sets,
            candidates,
            "f3d changed reference candidate set",
        )?;
    }
    unique_bipartite_assignment(&candidate_sets, ctx)
}

fn deleted_reference_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "hold F3D deleted reference candidates")?;
    let mut reference_candidates = Vec::new();
    let mut deleted_candidates = Vec::new();
    let mut operands = operands.iter();
    while !operands.as_slice().is_empty() {
        let Some(operand) =
            ctx.next_charged(&mut operands, "scan F3D deleted reference operands")?
        else {
            break;
        };
        if operand.recipe_reference_contexts.is_empty()
            || operand.deleted_boundary_edge_slots.is_empty()
        {
            return Ok(None);
        }
        let mut candidates = Vec::new();
        let mut contexts = operand.recipe_reference_contexts.iter();
        while !contexts.as_slice().is_empty() {
            let Some(context) =
                ctx.next_charged(&mut contexts, "scan F3D deleted reference contexts")?
            else {
                break;
            };
            let mut edges = context.changed_reference_edge_slots.iter();
            while !edges.as_slice().is_empty() {
                let Some(&edge) =
                    ctx.next_charged(&mut edges, "scan F3D deleted reference edges")?
                else {
                    break;
                };
                ctx.push_scoped_vec(
                    &mut storage,
                    &mut candidates,
                    edge,
                    "f3d deleted reference candidate",
                )?;
            }
        }
        ctx.sort_unstable_by(
            &mut candidates,
            |value| value,
            Ord::cmp,
            "f3d deleted reference candidate sort",
        )?;
        ctx.dedup_vec(&mut candidates, "deduplicate F3D deleted reference edges")?;
        if candidates.is_empty() {
            return Ok(None);
        }
        ctx.push_scoped_vec(
            &mut storage,
            &mut reference_candidates,
            candidates,
            "f3d deleted reference candidate set",
        )?;
        let mut deleted = Vec::new();
        let mut edges = operand.deleted_boundary_edge_slots.iter();
        while !edges.as_slice().is_empty() {
            let Some(&edge) =
                ctx.next_charged(&mut edges, "scan F3D deleted reference boundary edges")?
            else {
                break;
            };
            ctx.push_scoped_vec(
                &mut storage,
                &mut deleted,
                edge,
                "f3d deleted reference boundary edge",
            )?;
        }
        ctx.push_scoped_vec(
            &mut storage,
            &mut deleted_candidates,
            deleted,
            "f3d deleted reference boundary set",
        )?;
    }
    unique_deleted_reference_assignment(&reference_candidates, &deleted_candidates, ctx)
}

fn unique_deleted_reference_assignment(
    reference_candidates: &[Vec<i64>],
    deleted_candidates: &[Vec<i64>],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if reference_candidates.len() != deleted_candidates.len() {
        return Ok(None);
    }
    let mut storage = ctx.reserve_scoped(0, "hold F3D shared deleted reference candidates")?;
    let mut candidate_sets = Vec::new();
    let mut pairs = reference_candidates.iter().zip(deleted_candidates);
    while pairs.len() != 0 {
        let Some((references, deleted)) =
            ctx.next_charged(&mut pairs, "scan F3D deleted reference pairs")?
        else {
            break;
        };
        let mut candidates = Vec::new();
        let mut references = references.iter();
        while !references.as_slice().is_empty() {
            let Some(&edge) =
                ctx.next_charged(&mut references, "scan F3D shared deleted reference edges")?
            else {
                break;
            };
            if ctx.contains(deleted, &edge, "find F3D deleted reference shared edge")? {
                ctx.push_scoped_vec(
                    &mut storage,
                    &mut candidates,
                    edge,
                    "f3d deleted reference shared edge",
                )?;
            }
        }
        ctx.sort_unstable_by(
            &mut candidates,
            |value| value,
            Ord::cmp,
            "f3d deleted reference shared edge sort",
        )?;
        ctx.dedup_vec(
            &mut candidates,
            "deduplicate F3D deleted reference shared edges",
        )?;
        if candidates.is_empty() {
            return Ok(None);
        }
        ctx.push_scoped_vec(
            &mut storage,
            &mut candidate_sets,
            candidates,
            "f3d deleted reference shared set",
        )?;
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
    reference_edge_sets: impl IntoIterator<Item = Option<&'a [i64]>>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<EdgeAssignmentCandidates>, CodecError> {
    let mut reference_edge_sets = reference_edge_sets
        .into_iter()
        .map(|edges| edges.filter(|edges| !edges.is_empty()));
    if !selector_contexts.is_empty() {
        return Ok(
            edge_assignment_candidates(selector_contexts, reference_edge_sets, ctx)?
                .map(EdgeAssignmentCandidates::Edges),
        );
    }
    let mut first = None;
    let mut second = None;
    while reference_edge_sets.size_hint().1 != Some(0) {
        let Some(edges) = ctx.next_charged(
            &mut reference_edge_sets,
            "scan F3D assignment reference sources",
        )?
        else {
            break;
        };
        let Some(edges) = edges else {
            continue;
        };
        if first.is_none() {
            first = Some(edges);
        } else {
            second = Some(edges);
            break;
        }
    }
    let (Some(first), Some(second)) = (first, second) else {
        return Ok(Some(EdgeAssignmentCandidates::Context));
    };
    let mut storage = ctx.reserve_scoped(0, "hold F3D assignment reference candidates")?;
    let mut candidates = Vec::new();
    let mut edges = first.iter();
    while !edges.as_slice().is_empty() {
        let Some(&edge) = ctx.next_charged(&mut edges, "scan F3D assignment reference edges")?
        else {
            break;
        };
        ctx.push_scoped_vec(
            &mut storage,
            &mut candidates,
            edge,
            "f3d edge assignment reference candidate",
        )?;
    }
    ctx.retain_vec(
        &mut candidates,
        |candidate| {
            ctx.contains(
                second,
                candidate,
                "find F3D second assignment reference candidate",
            )
        },
        "retain F3D second assignment reference candidates",
    )?;
    ctx.sort_unstable_by(
        &mut candidates,
        |value| value,
        Ord::cmp,
        "f3d edge assignment candidate sort",
    )?;
    ctx.dedup_vec(
        &mut candidates,
        "deduplicate F3D assignment reference edges",
    )?;
    if candidates.is_empty() {
        return Ok(None);
    }
    storage.commit_value(Some(EdgeAssignmentCandidates::Edges(candidates)))
}

pub(super) fn radius_edge_group_candidates(
    operands: &[&DesignEdgeOperand],
    radius: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if operands.is_empty() || !radius.is_finite() || radius <= 0.0 {
        return Ok(None);
    }
    let tolerance = EPS_EDGE_RESOLVE_RADIUS_EDGE_GROUP_CANDIDATES_E9 * (1.0 + radius.abs());
    let mut storage = ctx.reserve_scoped(0, "hold F3D radius edge candidates")?;
    let mut chain = Vec::new();
    let mut complete = true;
    for operand in ctx.admit_iter(operands, "scan F3D radius edge operands")? {
        let resolved = resolved_edge_operand(operand, ctx)?;
        if let Some(edge) = resolved {
            ctx.push_scoped_vec(&mut storage, &mut chain, edge, "f3d radius recipe edge")?;
        }
        let mut has_radius_candidate = false;
        for candidate in ctx.admit_iter(
            &operand.treatment_radius_candidates,
            "scan F3D treatment radius candidates",
        )? {
            if (candidate.radius.get() - radius).abs() <= tolerance {
                has_radius_candidate = true;
                ctx.push_scoped_vec(
                    &mut storage,
                    &mut chain,
                    candidate.edge_slot,
                    "f3d radius candidate edge",
                )?;
            }
        }
        if resolved.is_none()
            && !has_radius_candidate
            && !operand.changed_boundary_edge_slots.is_empty()
        {
            complete = false;
        }
    }
    if chain.len() > 1 {
        ctx.sort_unstable_by(&mut chain, |value| value, Ord::cmp, "f3d radius edge sort")?;
        ctx.dedup_vec(&mut chain, "deduplicate F3D radius edge candidates")?;
    }
    if chain.is_empty() || !complete {
        return Ok(None);
    }
    storage.commit_value(Some(chain))
}

fn radius_edge_identity_group_candidates(
    operands: &[&DesignEdgeIdentityOperand],
    radius: f64,
    ctx: &DecodeContext<'_>,
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
    let mut chain_storage = ctx.reserve_scoped(0, "hold F3D radius identity chain")?;
    let mut chain = Vec::new();
    let mut source = operands.iter();
    while !source.as_slice().is_empty() {
        let Some(operand) = ctx.next_charged(&mut source, "scan F3D radius identity operands")?
        else {
            break;
        };
        let mut contribution_storage =
            ctx.reserve_scoped(0, "hold F3D radius identity contribution")?;
        let mut contribution = Vec::new();
        if let Some(edge) = operand.resolved_edge_slot {
            ctx.push_scoped_vec(
                &mut contribution_storage,
                &mut contribution,
                edge,
                "f3d radius identity resolved edge",
            )?;
        }
        let mut edges = operand.resolved_edge_slots.iter();
        while !edges.as_slice().is_empty() {
            let Some(&edge) =
                ctx.next_charged(&mut edges, "scan F3D radius identity resolved slots")?
            else {
                break;
            };
            ctx.push_scoped_vec(
                &mut contribution_storage,
                &mut contribution,
                edge,
                "f3d radius identity resolved edge",
            )?;
        }
        if use_transition_radius {
            let mut candidates = operand.treatment_radius_candidates.iter();
            while !candidates.as_slice().is_empty() {
                let Some(candidate) =
                    ctx.next_charged(&mut candidates, "scan F3D radius identity candidates")?
                else {
                    break;
                };
                if (candidate.radius.get() - radius).abs() <= tolerance {
                    ctx.push_scoped_vec(
                        &mut contribution_storage,
                        &mut contribution,
                        candidate.edge_slot,
                        "f3d radius identity candidate edge",
                    )?;
                }
            }
        }
        if contribution.is_empty() {
            return Ok(None);
        }
        let mut edges = contribution.into_iter();
        while edges.len() != 0 {
            let Some(edge) =
                ctx.next_charged(&mut edges, "scan F3D radius identity contribution edges")?
            else {
                break;
            };
            ctx.push_scoped_vec(
                &mut chain_storage,
                &mut chain,
                edge,
                "f3d radius identity chain edge",
            )?;
        }
    }
    ctx.sort_unstable_by(
        &mut chain,
        |value| value,
        Ord::cmp,
        "f3d radius identity edge sort",
    )?;
    ctx.dedup_vec(&mut chain, "deduplicate F3D radius identity edges")?;
    if chain.is_empty() {
        return Ok(None);
    }
    chain_storage.commit_value(Some(chain))
}

fn unique_edge_assignment_with_context(
    candidate_sets: &[EdgeAssignmentCandidates],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if matches!(candidate_sets, [] | [EdgeAssignmentCandidates::Context]) { return Ok(None); }
    unique_assignment_from_source(
        candidate_sets.iter().map(|candidates| match candidates {
            EdgeAssignmentCandidates::Context => None,
            EdgeAssignmentCandidates::Edges(edges) => Some(edges.as_slice()),
        }),
        (
            "f3d unique edge assignment set",
            "f3d unique edge assignment candidate",
        ),
        ctx,
    )
}

fn edge_assignment_candidates<'a>(
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: impl IntoIterator<Item = Option<&'a [i64]>>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let slots = if !selector_contexts.is_empty()
        && ctx.all_by(
            selector_contexts,
            |selector| Ok(!selector.incidence_matching_edge_slots.is_empty()),
            "select F3D edge assignment evidence",
        )? {
        SelectorSlots::Incidence
    } else {
        SelectorSlots::BoundaryCount
    };
    corroborated_edge_candidates(selector_contexts, shared_edge_sets, slots, ctx)
}

fn unique_bipartite_assignment(
    candidate_sets: &[Vec<i64>],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    unique_assignment_from_source(
        candidate_sets.iter().map(|edges| Some(edges.as_slice())),
        (
            "f3d edge normalized groups",
            "f3d edge normalized candidates",
        ),
        ctx,
    )
}

fn unique_assignment_from_source<'a>(
    mut candidate_sets: impl ExactSizeIterator<Item = Option<&'a [i64]>>,
    (group_operation, candidate_operation): (&'static str, &'static str),
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if candidate_sets.len() == 0 {
        return Ok(None);
    }
    let mut normalized_storage = ctx.reserve_scoped(0, "f3d edge normalized storage")?;
    let normalized = normalized_storage.with_storage(|| -> Result<_, CodecError> {
        let mut normalized = Vec::new();
        while let Some(edges) =
            ctx.next_charged(&mut candidate_sets, "f3d edge normalized source")?
        {
            let Some(edges) = edges else { continue };
            if edges.is_empty() { return Ok(None); }
            let mut candidates = ctx.copy_slice(edges, candidate_operation)?;
            ctx.sort_unstable_by(
                &mut candidates,
                |value| value,
                Ord::cmp,
                "f3d edge normalized candidate sort",
            )?;
            ctx.dedup_vec(&mut candidates, "f3d edge resolve candidates dedup")?;
            if candidates.is_empty() {
                return Ok(None);
            }
            ctx.push_vec(&mut normalized, candidates, group_operation)?;
        }
        Ok((!normalized.is_empty()).then_some(normalized))
    })?;
    let Some(normalized) = normalized else {
        return Ok(None);
    };
    let mut assignment_storage = ctx.reserve_scoped(0, "f3d edge assignment")?;
    let assignment =
        assignment_storage.with_storage(|| bipartite_assignment(&normalized, None, ctx))?;
    let Some(assignment) = assignment else {
        return Ok(None);
    };
    let mut assigned_edges = assignment.iter().copied().enumerate();
    while let Some((member, edge)) =
        ctx.next_charged(&mut assigned_edges, "f3d edge assignment uniqueness source")?
    {
        let mut alternative_storage =
            ctx.reserve_scoped(0, "f3d edge alternate assignment storage")?;
        let alternative = alternative_storage.with_storage(|| -> Result<_, CodecError> {
            Ok(bipartite_assignment(&normalized, Some((member, edge)), ctx)?.is_some())
        })?;
        if alternative {
            return Ok(None);
        }
    }
    assignment_storage.commit_value(Some(assignment))
}

fn bipartite_assignment(
    candidate_sets: &[Vec<i64>],
    forbidden: Option<(usize, i64)>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    fn augment(
        member: usize,
        (candidate_sets, forbidden): (&[Vec<i64>], Option<(usize, i64)>),
        visited: &mut HashSet<i64>,
        edge_members: &mut BTreeMap<i64, usize>,
        ctx: &DecodeContext<'_>,
        members_storage: &mut ScopedReservation<'_>,
    ) -> Result<bool, CodecError> {
        let _depth = ctx.enter_nested("f3d edge assignment search")?;
        let mut source = candidate_sets[member].iter();
        while let Some(edge) = ctx.next_charged(&mut source, "f3d edge assignment candidate")? {
            if forbidden == Some((member, *edge))
                || ctx.contains_hash_set(visited, edge, "f3d edge assignment visited lookup")?
            {
                continue;
            }
            ctx.insert_hash_set(visited, *edge, "f3d edge assignment visits")?;
            let displaced = ctx
                .get_btree_map(edge_members, edge, "f3d edge assignment member lookup")?
                .copied();
            let assignable = match displaced {
                Some(displaced) => augment(
                    displaced,
                    (candidate_sets, forbidden),
                    visited,
                    edge_members,
                    ctx,
                    members_storage,
                )?,
                None => true,
            };
            if assignable {
                members_storage.with_storage(|| {
                    ctx.insert_btree_map(edge_members, *edge, member, "f3d edge assignment members")
                })?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    let mut members_storage = ctx.reserve_scoped(0, "f3d edge assignment members")?;
    let mut edge_members = BTreeMap::new();
    let mut members = 0..candidate_sets.len();
    while let Some(member) = ctx.next_charged(&mut members, "f3d edge assignment member source")? {
        let mut temporary_visited_reservation =
            ctx.reserve_scoped(0, "f3d edge assignment visits")?;
        let assigned = temporary_visited_reservation.with_storage(|| {
            let mut visited = HashSet::new();
            augment(
                member,
                (candidate_sets, forbidden),
                &mut visited,
                &mut edge_members,
                ctx,
                &mut members_storage,
            )
        })?;
        if !assigned {
            return Ok(None);
        }
    }
    let mut assignment = ctx.alloc_filled(candidate_sets.len(), 0, "f3d edge assignment")?;
    for (&edge, &member) in ctx.admit_iter(&edge_members, "f3d edge assignment output source")? {
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
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let Some(stream) = native_stream_charged(ctx, &target.id, "f3d edge native stream")? else {
        return Ok(None);
    };
    let mut groups_storage = ctx.reserve_scoped(0, "hold F3D scope partition groups")?;
    let mut scope_groups = Vec::new();
    let mut target_ordinal = None;
    let mut remaining_groups = groups.iter();
    while !remaining_groups.as_slice().is_empty() {
        let Some(group) =
            ctx.next_charged(&mut remaining_groups, "scan F3D partition scope groups")?
        else {
            break;
        };
        if !ctx.equal(&native_stream_charged(ctx, &group.id, "f3d edge native stream")?, &Some(stream), "f3d edge stream comparison")?
            || group.scope_record_index != target.scope_record_index
            || !group.lost_edge_references.is_empty()
        {
            continue;
        }
        let is_target = ctx.equal_bytes(
            group.id.as_bytes(),
            target.id.as_bytes(),
            "compare F3D partition group identity",
        )?;
        let group_members = if is_target {
            target_members
        } else {
            group.members()
        };
        if group_members.is_empty() {
            continue;
        }
        let mut members_storage = ctx.reserve_scoped(0, "hold F3D scope partition members")?;
        let mut members = Vec::new();
        let mut complete = true;
        let mut partition_member_source = group_members.iter().map(|member| &member.value);
        while let Some(member) = ctx.next_charged(&mut partition_member_source, "f3d edge projection source")? {
            let matches = unique_feature_match_for_decode(ctx, operands.iter(), |operand| Ok(ctx.equal(&native_stream_charged(ctx, &operand.id, "f3d edge native stream")?, &Some(stream), "f3d edge stream comparison")?
                    && operand.scope_record_index == group.scope_record_index
                    && operand.record_index() == *member), "f3d unique edge operand")?;
            let Some(operand) = matches else {
                complete = false;
                break;
            };
            ctx.push_scoped_vec(
                &mut members_storage,
                &mut members,
                EdgeGroupMember {
                    identity: operand.record_index(),
                    resolved_edge: resolved_edge_operand(operand, ctx)?,
                    deleted_boundary_edges: &operand.deleted_boundary_edge_slots,
                },
                "f3d edge partition member",
            )?;
        }
        if !complete {
            continue;
        }
        if is_target {
            target_ordinal = Some(scope_groups.len());
        }
        groups_storage.absorb(&mut members_storage)?;
        ctx.push_scoped_vec(
            &mut groups_storage,
            &mut scope_groups,
            members,
            "f3d edge partition group",
        )?;
    }
    let Some(target_ordinal) = target_ordinal else {
        return Ok(None);
    };
    partition_unique_incomplete_edge_group(target_ordinal, &scope_groups, ctx)
}

fn partition_unique_incomplete_edge_group(
    target_ordinal: usize,
    groups: &[Vec<EdgeGroupMember<'_>>],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if groups.len() < 2 || target_ordinal >= groups.len() {
        return Ok(None);
    }
    let mut identities_storage = ctx.reserve_scoped(0, "hold F3D partition identities")?;
    let mut identities = HashSet::new();
    let mut universe_storage = ctx.reserve_scoped(0, "hold F3D partition universe")?;
    let mut universe = None::<Vec<i64>>;
    let mut remaining_groups = groups.iter();
    while !remaining_groups.as_slice().is_empty() {
        let Some(group) =
            ctx.next_charged(&mut remaining_groups, "scan F3D partition universe groups")?
        else {
            break;
        };
        let mut members = group.iter();
        while !members.as_slice().is_empty() {
            let Some(member) =
                ctx.next_charged(&mut members, "scan F3D partition universe members")?
            else {
                break;
            };
            if !identities_storage.with_storage(|| {
                ctx.insert_hash_set(
                    &mut identities,
                    member.identity,
                    "f3d edge partition identity",
                )
            })? {
                return Ok(None);
            }
            let mut deleted_storage =
                ctx.reserve_scoped(0, "hold F3D partition member deleted edges")?;
            let mut deleted = Vec::new();
            let mut edges = member.deleted_boundary_edges.iter();
            while !edges.as_slice().is_empty() {
                let Some(&edge) =
                    ctx.next_charged(&mut edges, "scan F3D partition member deleted edges")?
                else {
                    break;
                };
                ctx.push_scoped_vec(
                    &mut deleted_storage,
                    &mut deleted,
                    edge,
                    "f3d edge partition deleted edge",
                )?;
            }
            ctx.sort_unstable_by(
                &mut deleted,
                |value| value,
                Ord::cmp,
                "f3d edge partition deleted edge sort",
            )?;
            ctx.dedup_vec(&mut deleted, "deduplicate F3D partition deleted edges")?;
            if deleted.is_empty() {
                return Ok(None);
            }
            if let Some(universe) = universe.as_ref() {
                if universe.len() != deleted.len()
                    || !ctx.all_by(
                        universe.iter().zip(&deleted),
                        |(left, right)| {
                            ctx.equal(left, right, "compare F3D partition universe edge")
                        },
                        "scan F3D partition universe equality",
                    )?
                {
                    return Ok(None);
                }
            }
            if universe.is_none() {
                universe_storage.absorb(&mut deleted_storage)?;
                universe = Some(deleted);
            }
        }
    }
    let Some(universe) = universe else {
        return Ok(None);
    };
    if identities.len() != universe.len() {
        return Ok(None);
    }
    drop(identities);
    drop(identities_storage);
    let mut incomplete = None;
    let mut remaining_groups = groups.iter().enumerate();
    while remaining_groups.len() != 0 {
        let Some((ordinal, group)) = ctx.next_charged(
            &mut remaining_groups,
            "scan F3D partition incomplete groups",
        )?
        else {
            break;
        };
        let mut members = group.iter();
        while !members.as_slice().is_empty() {
            let Some(member) =
                ctx.next_charged(&mut members, "scan F3D partition incomplete members")?
            else {
                break;
            };
            if member.resolved_edge.is_none() {
                if incomplete.is_some() || ordinal != target_ordinal {
                    return Ok(None);
                }
                incomplete = Some(ordinal);
                break;
            }
        }
    }
    if incomplete != Some(target_ordinal) {
        return Ok(None);
    }
    let mut reserved_storage = ctx.reserve_scoped(0, "hold F3D partition reserved edges")?;
    let mut reserved = Vec::new();
    let mut remaining_groups = groups.iter().enumerate();
    while remaining_groups.len() != 0 {
        let Some((ordinal, group)) =
            ctx.next_charged(&mut remaining_groups, "scan F3D partition reserved groups")?
        else {
            break;
        };
        if ordinal == target_ordinal {
            continue;
        }
        let mut members = group.iter();
        while !members.as_slice().is_empty() {
            let Some(member) =
                ctx.next_charged(&mut members, "scan F3D partition reserved members")?
            else {
                break;
            };
            let Some(resolved) = member.resolved_edge.as_ref() else {
                return Ok(None);
            };
            if !ctx.contains(
                &universe,
                resolved,
                "find F3D partition resolved universe edge",
            )? || ctx.contains(&reserved, resolved, "find F3D partition reserved edge")?
            {
                return Ok(None);
            }
            ctx.push_scoped_vec(
                &mut reserved_storage,
                &mut reserved,
                *resolved,
                "f3d edge partition reserved edge",
            )?;
        }
    }
    let mut target_storage = ctx.reserve_scoped(0, "hold F3D partition target edges")?;
    let mut target = Vec::new();
    let mut candidates = universe.into_iter();
    while candidates.len() != 0 {
        let Some(candidate) =
            ctx.next_charged(&mut candidates, "scan F3D partition universe candidates")?
        else {
            break;
        };
        if !ctx.contains(
            &reserved,
            &candidate,
            "find F3D partition reserved candidate",
        )? {
            ctx.push_scoped_vec(
                &mut target_storage,
                &mut target,
                candidate,
                "f3d edge partition target edge",
            )?;
        }
    }
    drop(candidates);
    drop(universe_storage);
    drop(reserved);
    drop(reserved_storage);
    if target.len() != groups[target_ordinal].len()
        || ctx.any_by(
            &groups[target_ordinal],
            |member| match member.resolved_edge {
                Some(resolved) => ctx
                    .contains(
                        &target,
                        &resolved,
                        "find F3D partition target resolved edge",
                    )
                    .map(|found| !found),
                None => Ok(false),
            },
            "scan F3D partition target proof",
        )?
    {
        return Ok(None);
    }
    target_storage.commit_value(Some(target))
}

fn common_deleted_edge_group_candidates<'a>(
    members: impl IntoIterator<Item = (bool, &'a [i64])>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let mut candidate_sets = members.into_iter();
    let mut first = None;
    while candidate_sets.size_hint().1 != Some(0) {
        let Some((edge_bearing, candidates)) =
            ctx.next_charged(&mut candidate_sets, "scan F3D common deleted members")?
        else {
            break;
        };
        if edge_bearing {
            first = Some(candidates);
            break;
        }
    }
    let Some(first) = first else {
        return Ok(None);
    };
    if first.is_empty() {
        return Ok(None);
    }
    let mut member_count = 1;
    let mut storage = ctx.reserve_scoped(0, "hold F3D common deleted candidates")?;
    let mut candidates = Vec::new();
    let mut edges = first.iter();
    while !edges.as_slice().is_empty() {
        let Some(&candidate) =
            ctx.next_charged(&mut edges, "scan F3D first common deleted edges")?
        else {
            break;
        };
        ctx.push_scoped_vec(
            &mut storage,
            &mut candidates,
            candidate,
            "f3d common deleted candidate",
        )?;
    }
    ctx.sort_unstable_by(
        &mut candidates,
        |value| value,
        Ord::cmp,
        "f3d common deleted candidate sort",
    )?;
    ctx.dedup_vec(&mut candidates, "deduplicate F3D common deleted candidates")?;
    while candidate_sets.size_hint().1 != Some(0) {
        let Some((edge_bearing, candidate_set)) =
            ctx.next_charged(&mut candidate_sets, "scan F3D common deleted members")?
        else {
            break;
        };
        if !edge_bearing {
            continue;
        }
        member_count += 1;
        let mut normalized_storage =
            ctx.reserve_scoped(0, "hold F3D common deleted normalized edges")?;
        let mut normalized = Vec::new();
        let mut edges = candidate_set.iter();
        while !edges.as_slice().is_empty() {
            let Some(&candidate) =
                ctx.next_charged(&mut edges, "scan F3D later common deleted edges")?
            else {
                break;
            };
            ctx.push_scoped_vec(
                &mut normalized_storage,
                &mut normalized,
                candidate,
                "f3d common deleted normalized edge",
            )?;
        }
        ctx.sort_unstable_by(
            &mut normalized,
            |value| value,
            Ord::cmp,
            "f3d common deleted normalized sort",
        )?;
        ctx.dedup_vec(
            &mut normalized,
            "deduplicate F3D common deleted normalized edges",
        )?;
        if normalized.len() != candidates.len()
            || !ctx.all_by(
                normalized.iter().zip(&candidates),
                |(left, right)| ctx.equal(left, right, "compare F3D common deleted edge"),
                "scan F3D common deleted equality",
            )?
        {
            return Ok(None);
        }
    }
    if candidates.len() != member_count {
        return Ok(None);
    }
    storage.commit_value(Some(candidates))
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
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if operands.is_empty() {
        return Ok(None);
    }
    let mut deleted_storage = ctx.reserve_scoped(0, "hold F3D deleted boundary candidates")?;
    let mut deleted = Vec::new();
    let mut contextual_storage =
        ctx.reserve_scoped(0, "hold F3D deleted boundary contextual proof")?;
    let mut contextual = Vec::new();
    let mut source = operands.iter();
    while !source.as_slice().is_empty() {
        let Some(operand) = ctx.next_charged(&mut source, "scan F3D deleted boundary operands")?
        else {
            break;
        };
        if operand.recipe_structure.is_none()
            || operand.recipe_references.is_empty()
            || operand.recipe_references.len() != operand.recipe_reference_contexts.len()
            || operand.recipe_reference_contexts.is_empty()
            || operand.deleted_boundary_edge_slots.is_empty()
            || ctx.any_by(
                &operand.deleted_boundary_edge_slots,
                |edge| {
                    Ok(!ctx.contains(
                        &operand.preceding_boundary_edge_slots,
                        edge,
                        "find F3D deleted boundary predecessor edge",
                    )? || !ctx.contains(
                        &operand.changed_boundary_edge_slots,
                        edge,
                        "find F3D deleted boundary changed edge",
                    )?)
                },
                "scan F3D deleted boundary proof",
            )?
        {
            return Ok(None);
        }
        let mut member_contextual = false;
        let mut contexts = operand.recipe_reference_contexts.iter();
        while !contexts.as_slice().is_empty() {
            let Some(context) =
                ctx.next_charged(&mut contexts, "scan F3D deleted boundary contexts")?
            else {
                break;
            };
            let mut edges = context.changed_reference_edge_slots.iter();
            while !edges.as_slice().is_empty() {
                let Some(&edge) =
                    ctx.next_charged(&mut edges, "scan F3D deleted boundary contextual edges")?
                else {
                    break;
                };
                if !ctx.contains(
                    &operand.deleted_boundary_edge_slots,
                    &edge,
                    "find F3D deleted boundary contextual edge",
                )? {
                    continue;
                }
                member_contextual = true;
                ctx.push_scoped_vec(
                    &mut contextual_storage,
                    &mut contextual,
                    edge,
                    "f3d deleted boundary contextual edge",
                )?;
            }
        }
        if !member_contextual {
            return Ok(None);
        }
        let mut edges = operand.deleted_boundary_edge_slots.iter();
        while !edges.as_slice().is_empty() {
            let Some(&edge) =
                ctx.next_charged(&mut edges, "scan F3D deleted boundary candidates")?
            else {
                break;
            };
            ctx.push_scoped_vec(
                &mut deleted_storage,
                &mut deleted,
                edge,
                "f3d deleted boundary edge",
            )?;
        }
    }
    ctx.sort_unstable_by(
        &mut deleted,
        |value| value,
        Ord::cmp,
        "f3d deleted boundary edge sort",
    )?;
    ctx.dedup_vec(&mut deleted, "deduplicate F3D deleted boundary edges")?;
    ctx.sort_unstable_by(
        &mut contextual,
        |value| value,
        Ord::cmp,
        "f3d deleted boundary contextual sort",
    )?;
    ctx.dedup_vec(
        &mut contextual,
        "deduplicate F3D deleted boundary contextual edges",
    )?;
    if deleted.len() != operands.len() {
        return Ok(None);
    }
    if !ctx.all_by(
        &deleted,
        |edge| {
            ctx.binary_search(&contextual, edge, "find F3D deleted contextual sorted edge")
                .map(|found| found.is_ok())
        },
        "scan F3D deleted contextual coverage",
    )? {
        return Ok(None);
    }
    deleted_storage.commit_value(Some(deleted))
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
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    if operands.is_empty() {
        return Ok(None);
    }
    let mut deleted_storage = ctx.reserve_scoped(0, "hold F3D contextual deleted proof")?;
    let mut deleted = Vec::new();
    let mut has_deleted_member = false;
    let mut source = operands.iter();
    while !source.as_slice().is_empty() {
        let Some(operand) =
            ctx.next_charged(&mut source, "scan F3D contextual deleted operands")?
        else {
            break;
        };
        if operand.recipe_structure.is_none()
            || operand.recipe_references.is_empty()
            || operand.recipe_references.len() != operand.recipe_reference_contexts.len()
            || operand.recipe_reference_contexts.is_empty()
        {
            return Ok(None);
        }
        let mut edges = operand.deleted_boundary_edge_slots.iter();
        while !edges.as_slice().is_empty() {
            let Some(edge) =
                ctx.next_charged(&mut edges, "scan F3D contextual deleted boundary edges")?
            else {
                break;
            };
            if !ctx.contains(
                &operand.preceding_boundary_edge_slots,
                edge,
                "find F3D contextual deleted predecessor edge",
            )? || !ctx.contains(
                &operand.changed_boundary_edge_slots,
                edge,
                "find F3D contextual deleted changed edge",
            )? {
                return Ok(None);
            }
            ctx.push_scoped_vec(
                &mut deleted_storage,
                &mut deleted,
                *edge,
                "f3d contextual deleted edge",
            )?;
            has_deleted_member = true;
        }
    }
    if !has_deleted_member {
        return Ok(None);
    }
    ctx.sort_unstable_by(
        &mut deleted,
        |value| value,
        Ord::cmp,
        "f3d contextual deleted edge sort",
    )?;
    ctx.dedup_vec(&mut deleted, "deduplicate F3D contextual deleted edges")?;
    if deleted.len() != operands.len() {
        return Ok(None);
    }

    let mut sets_storage = ctx.reserve_scoped(0, "hold F3D contextual deleted candidate sets")?;
    let mut candidate_sets = Vec::new();
    let mut source = operands.iter();
    while !source.as_slice().is_empty() {
        let Some(operand) = ctx.next_charged(
            &mut source,
            "scan F3D contextual deleted candidate operands",
        )?
        else {
            break;
        };
        let mut candidates_storage =
            ctx.reserve_scoped(0, "hold F3D contextual deleted member candidates")?;
        let mut candidates = Vec::new();
        let mut contexts = operand.recipe_reference_contexts.iter();
        while !contexts.as_slice().is_empty() {
            let Some(context) =
                ctx.next_charged(&mut contexts, "scan F3D contextual deleted contexts")?
            else {
                break;
            };
            let mut edges = context.changed_reference_edge_slots.iter();
            while !edges.as_slice().is_empty() {
                let Some(&edge) =
                    ctx.next_charged(&mut edges, "scan F3D contextual deleted context edges")?
                else {
                    break;
                };
                if ctx
                    .binary_search(&deleted, &edge, "find F3D contextual deleted sorted edge")?
                    .is_ok()
                {
                    ctx.push_scoped_vec(
                        &mut candidates_storage,
                        &mut candidates,
                        edge,
                        "f3d contextual deleted candidate",
                    )?;
                }
            }
        }
        ctx.sort_unstable_by(
            &mut candidates,
            |value| value,
            Ord::cmp,
            "f3d contextual deleted candidate sort",
        )?;
        ctx.dedup_vec(
            &mut candidates,
            "deduplicate F3D contextual deleted candidates",
        )?;
        sets_storage.absorb(&mut candidates_storage)?;
        ctx.push_scoped_vec(
            &mut sets_storage,
            &mut candidate_sets,
            candidates,
            "f3d contextual deleted candidate set",
        )?;
    }
    let assignment_storage;
    let (assignment, storage) = ctx
        .with_scoped_storage("hold F3D contextual deleted assignment", || {
            unique_bipartite_assignment(&candidate_sets, ctx)
        })?;
    assignment_storage = storage;
    let Some(mut assignment) = assignment else {
        return Ok(None);
    };
    ctx.sort_unstable_by(
        &mut assignment,
        |value| value,
        Ord::cmp,
        "f3d contextual deleted assignment sort",
    )?;
    assignment_storage.commit_value(Some(assignment))
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
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let [operand] = operands else {
        return Ok(None);
    };
    let Some(structure) = operand.recipe_structure.as_ref() else {
        return Ok(None);
    };
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
    let mut storage = ctx.reserve_scoped(0, "hold F3D result boundary candidates")?;
    let mut candidates = Vec::new();
    let mut contexts = operand.recipe_reference_contexts.iter();
    while !contexts.as_slice().is_empty() {
        let Some(context) = ctx.next_charged(&mut contexts, "scan F3D result boundary contexts")?
        else {
            break;
        };
        let mut edges = context.changed_reference_edge_slots.iter();
        while !edges.as_slice().is_empty() {
            let Some(&edge) =
                ctx.next_charged(&mut edges, "scan F3D result boundary context edges")?
            else {
                break;
            };
            if ctx.contains(
                &operand.result_boundary_edge_slots,
                &edge,
                "find F3D result boundary candidate",
            )? {
                ctx.push_scoped_vec(
                    &mut storage,
                    &mut candidates,
                    edge,
                    "f3d result boundary candidate",
                )?;
            }
        }
    }
    ctx.sort_unstable_by(
        &mut candidates,
        |value| value,
        Ord::cmp,
        "f3d result boundary candidate sort",
    )?;
    ctx.dedup_vec(&mut candidates, "deduplicate F3D result boundary edges")?;
    let [candidate] = candidates.as_slice() else {
        return Ok(None);
    };
    if ctx.contains(
        &operand.preceding_boundary_edge_slots,
        candidate,
        "find F3D result candidate predecessor",
    )? {
        return Ok(None);
    }
    let mut supports = 0;
    let mut contexts = operand.recipe_reference_contexts.iter();
    while !contexts.as_slice().is_empty() {
        let Some(context) =
            ctx.next_charged(&mut contexts, "scan F3D result candidate reference support")?
        else {
            break;
        };
        if ctx.contains(
            &context.changed_reference_edge_slots,
            candidate,
            "find F3D result candidate reference support",
        )? {
            supports += 1;
            if supports == 2 {
                return storage.commit_value(Some(candidates));
            }
        }
    }
    Ok(None)
}

fn changed_boundary_count_edge_group_candidates<'a>(
    members: impl IntoIterator<
        Item = &'a [crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    >,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let mut member_count = 0;
    let mut storage = ctx.reserve_scoped(0, "hold F3D boundary count candidates")?;
    let mut candidates = Vec::new();
    let mut source = members.into_iter();
    while source.size_hint().1 != Some(0) {
        let Some(selectors) = ctx.next_charged(&mut source, "scan F3D boundary count members")?
        else {
            break;
        };
        if selectors.is_empty() {
            return Ok(None);
        }
        member_count += 1;
        let mut selectors = selectors.iter();
        while !selectors.as_slice().is_empty() {
            let Some(selector) =
                ctx.next_charged(&mut selectors, "scan F3D boundary count selectors")?
            else {
                break;
            };
            let mut edges = selector.boundary_count_matching_edge_slots.iter();
            while !edges.as_slice().is_empty() {
                let Some(&edge) = ctx.next_charged(&mut edges, "scan F3D boundary count edges")?
                else {
                    break;
                };
                ctx.push_scoped_vec(
                    &mut storage,
                    &mut candidates,
                    edge,
                    "f3d boundary-count candidate",
                )?;
            }
        }
    }
    ctx.sort_unstable_by(
        &mut candidates,
        |value| value,
        Ord::cmp,
        "f3d boundary-count candidate sort",
    )?;
    ctx.dedup_vec(&mut candidates, "deduplicate F3D boundary count edges")?;
    if member_count == 0 || candidates.len() != member_count {
        return Ok(None);
    }
    storage.commit_value(Some(candidates))
}

pub(super) fn resolved_edge_operand(
    operand: &DesignEdgeOperand,
    ctx: &DecodeContext<'_>,
) -> Result<Option<i64>, CodecError> {
    if !operand.recipe_program.is_empty() && operand.recipe_structure.is_none() {
        return Ok(None);
    }
    match operand.resolved_edge_slot {
        Some(edge) => Ok(Some(edge)),
        None => primary_terminal_reference_shared_edge(operand, ctx),
    }
}

/// Resolve the selected edge of a zero-payload terminal recipe from the two
/// support-face references in its primary side.
fn primary_terminal_reference_shared_edge(
    operand: &DesignEdgeOperand,
    ctx: &DecodeContext<'_>,
) -> Result<Option<i64>, CodecError> {
    if !operand.recipe_reference_contexts.is_empty() {
        return Ok(None);
    }
    let Some(structure) = operand.recipe_structure.as_ref() else {
        return Ok(None);
    };
    if structure.root != 2
        || structure.sides.len() != 2
        || structure.sides.iter().any(|side| {
            side.scalars.len() != 2 || side.payload_prefix != [0] || !side.entries.is_empty()
        })
    {
        return Ok(None);
    }

    let primary = &structure.sides[0];
    let mut reference_ordinals = std::iter::once(primary.header_value)
        .chain(primary.scalars.iter().copied())
        .filter(|value| *value != 0)
        .map(|value| usize::try_from(value).ok()?.checked_sub(1));
    let (Some(first_ordinal), Some(second_ordinal)) = (
        reference_ordinals.next().flatten(),
        reference_ordinals.next().flatten(),
    ) else {
        return Ok(None);
    };
    if reference_ordinals.next().is_some() {
        return Ok(None);
    }
    if first_ordinal == second_ordinal {
        return Ok(None);
    }
    let (Some(first), Some(second)) = (
        operand.terminal_reference_edge_slots.get(first_ordinal),
        operand.terminal_reference_edge_slots.get(second_ordinal),
    ) else {
        return Ok(None);
    };
    if first.is_empty() || second.is_empty() {
        return Ok(None);
    }

    let mut candidate = None;
    let mut edges = first.iter();
    while let Some(&edge) = ctx.next_charged(&mut edges, "scan F3D terminal reference edges")? {
        if candidate == Some(edge) {
            continue;
        }
        if ctx.contains(second, &edge, "find F3D terminal shared reference edge")? {
            if candidate.is_some() {
                return Ok(None);
            }
            candidate = Some(edge);
        }
    }
    match candidate {
        Some(edge)
            if ctx.contains(
                &operand.terminal_boundary_edge_slots,
                &edge,
                "find F3D terminal boundary edge",
            )? =>
        {
            Ok(Some(edge))
        }
        _ => Ok(None),
    }
}

pub(crate) fn resolved_edge_candidate_intersection<'a, I>(
    ctx: &DecodeContext<'_>,
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: I,
) -> Result<Option<i64>, CodecError>
where
    I: IntoIterator<Item = &'a [i64]>,
    I::IntoIter: Clone,
{
    let ordered = shared_edge_sets.into_iter();
    let mut search = ordered.clone();
    let mut has_reference_sets = false;
    let first = ctx.find_by(
        search.by_ref(),
        |edges| {
            has_reference_sets = true;
            Ok(!edges.is_empty())
        },
        "f3d edge intersection reference query",
    )?;
    let references_unavailable = has_reference_sets && first.is_none();
    let reference_candidates = if let Some(first) = first {
        let rest = search.clone();
        if ctx
            .find_by(
                search,
                |edges| Ok(!edges.is_empty()),
                "f3d edge intersection reference query",
            )?
            .is_some()
        {
            Some(unique_edge_set_intersection(first, &rest, ctx)?)
        } else {
            None
        }
    } else {
        None
    };
    if reference_candidates == Some(EdgeSetIntersection::Disjoint) {
        return Ok(None);
    }
    let reference = match reference_candidates {
        Some(EdgeSetIntersection::Unique(edge)) => Some(edge),
        _ => None,
    };
    let incidence = if references_unavailable {
        None
    } else {
        corroborated_edge_intersection(selector_contexts, &ordered, SelectorSlots::Incidence, ctx)?
    };
    let boundary_count = if references_unavailable {
        None
    } else {
        corroborated_edge_intersection(
            selector_contexts,
            &ordered,
            SelectorSlots::BoundaryCount,
            ctx,
        )?
    };
    let common_triplet =
        corroborated_triplet_intersection(ctx, selector_contexts, &ordered, TripletProof::Common)?;
    let cross_clause_triplet =
        corroborated_triplet_intersection(ctx, selector_contexts, &ordered, TripletProof::CrossClause)?;
    let mut proofs = [
        reference,
        incidence,
        boundary_count,
        common_triplet,
        cross_clause_triplet,
    ]
    .into_iter()
    .flatten();
    let Some(edge) = proofs.next() else {
        return Ok(None);
    };
    Ok(proofs.all(|proof| proof == edge).then_some(edge))
}

pub(crate) fn unique_incidence_edge_shared_by_reference_faces<'a, I>(
    ctx: &DecodeContext<'_>,
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    reference_edge_sets: I,
) -> Result<Option<i64>, CodecError>
where
    I: IntoIterator<Item = &'a [i64]>,
    I::IntoIter: Clone,
{
    let reference_edge_sets = reference_edge_sets.into_iter();
    let mut candidate = None;
    let mut selectors = selector_contexts.iter();
    while let Some(selector) = ctx.next_charged(&mut selectors, "scan F3D incidence selectors")? {
        let mut edges = selector.incidence_matching_edge_slots.iter();
        while let Some(&edge) = ctx.next_charged(&mut edges, "scan F3D incidence edge candidates")? {
            if candidate == Some(edge) {
                continue;
            }
            let mut distinct_matches = 0;
            let mut references = reference_edge_sets.clone().enumerate();
            while let Some((index, edges)) =
                ctx.next_charged(&mut references, "scan F3D incidence reference sets")?
            {
                if !ctx.contains(edges, &edge, "find F3D incidence reference edge")? {
                    continue;
                }
                let duplicate = ctx.any_by(
                    reference_edge_sets.clone().take(index),
                    |prior| {
                        Ok(ctx.all_by(
                            prior,
                            |value| ctx.contains(edges, value, "compare F3D reference edge sets"),
                            "scan F3D prior reference edges",
                        )? && ctx.all_by(
                            edges,
                            |value| ctx.contains(prior, value, "compare F3D reference edge sets"),
                            "scan F3D reference edges",
                        )?)
                    },
                    "find F3D duplicate reference edge set",
                )?;
                if duplicate {
                    continue;
                }
                distinct_matches += 1;
                if distinct_matches == 2 {
                    break;
                }
            }
            if distinct_matches == 2 {
                if candidate.is_some() {
                    return Ok(None);
                }
                candidate = Some(edge);
            }
        }
    }
    Ok(candidate)
}

#[derive(Clone, Copy)]
enum TripletProof {
    Common,
    CrossClause,
}

fn corroborated_triplet_intersection<'a, 'b>(
    ctx: &DecodeContext<'_>,
    selector_contexts: &'a [crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: &(impl Iterator<Item = &'b [i64]> + Clone),
    proof: TripletProof,
) -> Result<Option<i64>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "f3d triplet intersection candidate storage")?;
    let mut candidates = None::<Vec<i64>>;
    let mut intersect_pair = |left: &'a [i64], right: &'a [i64]| -> Result<bool, CodecError> {
        if let Some(candidates) = candidates.as_mut() {
            if !ctx.any_by(left, |edge| ctx.contains(right, edge, "find F3D triplet pair overlap"), "scan F3D triplet pair overlap")? { return Ok(true); }
            ctx.retain_vec(candidates, |edge| Ok(
                ctx.contains(left, edge, "find F3D corroborating triplet edge")?
                    && ctx.contains(right, edge, "find F3D corroborating triplet edge")?
            ), "scan F3D corroborating triplet pairs")?;
            return Ok(!candidates.is_empty());
        }
        let mut first_candidates = Vec::new();
        let mut edges = left.iter();
        while let Some(&edge) = ctx.next_charged(&mut edges, "scan F3D triplet edge candidates")? {
            if ctx.contains(right, &edge, "find F3D triplet second edge")?
                && !ctx.contains(&first_candidates, &edge, "f3d triplet candidate uniqueness")? {
                ctx.push_scoped_vec(&mut storage, &mut first_candidates, edge, "f3d triplet intersection candidate")?;
            }
        }
        if !first_candidates.is_empty() { candidates = Some(first_candidates); }
        Ok(true)
    };
    let mut selectors = selector_contexts.iter();
    while let Some(selector) = ctx.next_charged(&mut selectors, "scan F3D triplet selectors")? {
        match proof {
            TripletProof::Common => {
                let mut clauses = selector.clauses.iter();
                while let Some(clause) = ctx.next_charged(&mut clauses, "scan F3D triplet selector clauses")? {
                    let Some(clause) = clause else { continue; };
                    if clause.entry.common_incident_edge_ordinal().is_none() { continue; }
                    let [first, second] = &clause.triplet_edge_slots;
                    if !intersect_pair(first, second)? { return Ok(None); }
                }
            }
            TripletProof::CrossClause => {
                let [Some(left), Some(right)] = selector.clauses.as_slice() else { continue; };
                for (left, right) in left.triplet_edge_slots.iter().zip(&right.triplet_edge_slots) {
                    if !intersect_pair(left, right)? { return Ok(None); }
                }
            }
        }
    }
    let Some(candidates) = candidates else { return Ok(None); };
    let mut candidate = None;
    let mut edges = candidates.into_iter();
    while let Some(edge) = ctx.next_charged(&mut edges, "scan F3D triplet edge candidates")? {
        if ctx.all_by(shared_edge_sets.clone(), |set| Ok(set.is_empty() || ctx.contains(set, &edge, "find F3D shared triplet edge")?), "scan F3D shared triplet sets")? {
            if candidate.is_some() { return Ok(None); }
            candidate = Some(edge);
        }
    }
    Ok(candidate)
}

#[derive(Clone, Copy, PartialEq)]
enum EdgeSetIntersection {
    Disjoint,
    Unique(i64),
    Ambiguous,
}

fn unique_edge_set_intersection<'a>(
    first: &[i64],
    rest: &(impl Iterator<Item = &'a [i64]> + Clone),
    ctx: &DecodeContext<'_>,
) -> Result<EdgeSetIntersection, CodecError> {
    let mut candidate = None;
    let mut edges = first.iter();
    while let Some(&edge) =
        ctx.next_charged(&mut edges, "scan F3D reference intersection candidates")?
    {
        if ctx.all_by(
            rest.clone(),
            |set| {
                Ok(set.is_empty()
                    || ctx.contains(set, &edge, "find F3D reference intersection edge")?)
            },
            "scan F3D reference intersection sets",
        )? {
            if candidate.is_some_and(|selected| selected != edge) {
                return Ok(EdgeSetIntersection::Ambiguous);
            }
            candidate = Some(edge);
        }
    }
    Ok(candidate.map_or(EdgeSetIntersection::Disjoint, EdgeSetIntersection::Unique))
}

#[derive(Clone, Copy)]
enum SelectorSlots {
    Incidence,
    BoundaryCount,
}

fn corroborated_edge_intersection<'a>(
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: &(impl Iterator<Item = &'a [i64]> + Clone),
    slots: SelectorSlots,
    ctx: &DecodeContext<'_>,
) -> Result<Option<i64>, CodecError> {
    let Some(first) = selector_contexts.first() else {
        return Ok(None);
    };
    let mut candidate = None;
    let mut edges = selector_candidate_edges(first, slots).iter();
    while let Some(&edge) =
        ctx.next_charged(&mut edges, "scan F3D selector intersection candidates")?
    {
        if !(selector_contexts.len() == 1
            || ctx.all_by(
                &selector_contexts[1..],
                |selector| {
                    ctx.contains(
                        selector_candidate_edges(selector, slots),
                        &edge,
                        "find F3D corroborating selector edge",
                    )
                },
                "scan F3D corroborating selectors",
            )?)
            || !ctx.all_by(
                shared_edge_sets.clone(),
                |set| {
                    Ok(set.is_empty()
                        || ctx.contains(
                            set,
                            &edge,
                            "find F3D shared selector edge",
                        )?)
                },
                "scan F3D shared selector sets",
            )?
        {
            continue;
        }
        if candidate.is_some_and(|selected| selected != edge) {
            return Ok(None);
        }
        candidate = Some(edge);
    }
    Ok(candidate)
}

fn corroborated_edge_candidates<'a>(
    selector_contexts: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    shared_edge_sets: impl IntoIterator<Item = Option<&'a [i64]>>,
    slots: SelectorSlots,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<i64>>, CodecError> {
    let Some(first_selector) = selector_contexts.first() else {
        return Ok(None);
    };
    let first = selector_candidate_edges(first_selector, slots);
    if first.is_empty() {
        return Ok(None);
    }
    let mut storage = ctx.reserve_scoped(0, "hold F3D corroborated edge candidates")?;
    let mut candidates = Vec::new();
    for &edge in ctx.admit_iter(first, "scan F3D corroborated candidate source")? {
        ctx.push_scoped_vec(
            &mut storage,
            &mut candidates,
            edge,
            "f3d corroborated edge candidate",
        )?;
    }
    ctx.sort_unstable_by(
        &mut candidates,
        |value| value,
        Ord::cmp,
        "f3d corroborated edge candidate sort",
    )?;
    ctx.dedup_vec(
        &mut candidates,
        "deduplicate F3D corroborated edge candidates",
    )?;
    let mut selectors = selector_contexts[1..].iter();
    while let Some(selector) =
        ctx.next_charged(&mut selectors, "scan F3D corroborated candidate selectors")?
    {
        let selector_edges = selector_candidate_edges(selector, slots);
        if selector_edges.is_empty() {
            return Ok(None);
        }
        ctx.retain_vec(
            &mut candidates,
            |candidate| {
                ctx.contains(
                    selector_edges,
                    candidate,
                    "find F3D corroborated selector candidate",
                )
            },
            "retain F3D corroborated selector candidates",
        )?;
        if candidates.is_empty() {
            return Ok(None);
        }
    }
    let mut shared = shared_edge_sets.into_iter();
    while let Some(shared_edges) =
        ctx.next_charged(&mut shared, "scan F3D corroborated shared sets")?
    {
        let Some(shared_edges) = shared_edges else {
            continue;
        };
        ctx.retain_vec(
            &mut candidates,
            |candidate| {
                ctx.contains(
                    shared_edges,
                    candidate,
                    "find F3D corroborated shared candidate",
                )
            },
            "retain F3D corroborated shared candidates",
        )?;
        if candidates.is_empty() {
            return Ok(None);
        }
    }
    storage.commit_value(Some(candidates))
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
    ctx: &DecodeContext<'_>,
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
    ctx: &DecodeContext<'_>,
) -> Result<Option<cadmpeg_ir::features::FeatureDefinition>, CodecError> {
    use cadmpeg_ir::features::{
        edge_treatments::{FilletGroup, RadiusSpec, VariableRadius},
        FeatureDefinition, FeatureOperation,
    };
    use cadmpeg_ir::scalar::Length;

    let fixed = or_none!(scope.fixed_fillet_parameters());
    let stream = or_none!(native_stream_charged(ctx, &scope.id, "f3d edge native stream")?);
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
                let mut sample_storage = ctx.reserve_scoped(0, "f3d fixed fillet radius point")?;
                let mut points = Vec::new();
                let Some(start_radius) = Length::new(start.value.get() * 10.0) else { return Ok(None); };
                ctx.push_scoped_vec(&mut sample_storage, &mut points, VariableRadius {
                    parameter: 0.0,
                    radius: start_radius,
                }, "f3d fixed fillet radius point")?;
                let mut sample_source = intermediate.iter();
                while let Some(row) = ctx.next_charged(&mut sample_source, "f3d edge projection source")? {
                    let Some(radius) = Length::new(row.radius.value.get() * 10.0) else { return Ok(None); };
                    ctx.push_scoped_vec(&mut sample_storage, &mut points, VariableRadius {
                        parameter: row.parameter.value.get(),
                        radius,
                    }, "f3d fixed fillet radius point")?;
                }
                let Some(end_radius) = Length::new(end.value.get() * 10.0) else { return Ok(None); };
                ctx.push_scoped_vec(&mut sample_storage, &mut points, VariableRadius {
                    parameter: 1.0,
                    radius: end_radius,
                }, "f3d fixed fillet radius point")?;
                let mut candidate_storage = ctx.provisional_retained("F3D fixed fillet typed radius candidate")?;
                let converted = candidate_storage.with_storage(|| {
                    cadmpeg_ir::features::edge_treatments::VariableRadii::new(points, ctx)
                })?;
                let Ok(points) = converted else { return Ok(None); };
                let points = candidate_storage.commit_value(points)?;
                Some(RadiusSpec::Variable {
                    points,
                })
            }
        }) };
    let mut scope_storage = ctx.reserve_scoped(0, "hold F3D fixed fillet scope groups")?;
    let mut scope_groups = Vec::new();
    let mut group_source = construction_groups.iter();
    while let Some(group) = ctx.next_charged(&mut group_source, "f3d fixed fillet construction group source")? {
        if !ctx.equal(&native_stream_charged(ctx, &group.id, "f3d edge native stream")?, &Some(stream), "f3d edge stream comparison")?
            || group.scope_record_index != scope.record_index || group.members().is_empty() { continue; }
        ctx.push_scoped_vec(
            &mut scope_storage,
            &mut scope_groups,
            group,
            "f3d fixed fillet scope group",
        )?;
    }
    ctx.stable_sort_by_key(
        &mut scope_groups[..],
        |value| {
            let group = value;
            {
                group.scope_reference_ordinal
            }
        },
        Ord::cmp,
        "sort f3d design edge_resolve 1",
    )?;
    let mut complete_storage = ctx.reserve_scoped(0, "hold F3D fixed fillet complete groups")?;
    let mut complete_edge_groups = Vec::new();
    let mut complete_source = scope_groups.iter().copied();
    while let Some(group) = ctx.next_charged(&mut complete_source, "f3d fixed fillet complete group source")? {
        if !ctx.all_by(group.members(), |member| {
            ctx.any_by(edge_operands, |operand| Ok(
                ctx.equal(&native_stream_charged(ctx, &operand.id, "f3d edge native stream")?, &Some(stream), "f3d edge stream comparison")?
                    && operand.scope_record_index == scope.record_index
                    && operand.record_index() == member.value
            ), "f3d fixed fillet operand presence")
        }, "f3d fixed fillet member presence")? { continue; }
        ctx.push_scoped_vec(
            &mut complete_storage,
            &mut complete_edge_groups,
            group,
            "f3d fixed fillet complete group",
        )?;
    }
    let singleton;
    let edge_groups = if complete_edge_groups.len() == fixed.groups.len() {
        complete_edge_groups.as_slice()
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
            // This fallback needs a constant radius. Do not build variable
            // samples when its fixed law cannot select an identity chain.
            if !matches!(
                fixed.groups[0].law(),
                crate::records::feature::fixed_parameters::DesignFixedFilletLaw::Constant(_)
            ) {
                return Ok(None);
            }
            let radius = radius_spec(&fixed.groups[0])?;
            let Some(RadiusSpec::Constant { radius }) = radius else {
                return Ok(None);
            };
            let mut identity_storage = ctx.reserve_scoped(0, "hold F3D fixed fillet identities")?;
            let mut identities = Vec::new();
            let mut fillet_identity_member_source = group.members().iter().map(|member| &member.value);
            while let Some(member) = ctx.next_charged(&mut fillet_identity_member_source, "f3d edge projection source")? {
                let matches = unique_feature_match_for_decode(ctx, edge_identity_operands.iter(), |operand| Ok(ctx.equal(&native_stream_charged(ctx, &operand.id, "f3d edge native stream")?, &Some(stream), "f3d edge stream comparison")?
                        && operand.scope_record_index == scope.record_index
                        && operand.group_record_index == group.record_index
                        && operand.record_index() == *member), "f3d unique edge operand")?;
                let Some(operand) = matches else {
                    return Ok(None);
                };
                if !operand.layout().is_compact() {
                    return Ok(None);
                }
                ctx.push_scoped_vec(
                    &mut identity_storage,
                    &mut identities,
                    operand,
                    "f3d fixed fillet identity",
                )?;
            }
            let mut candidate_storage =
                ctx.reserve_scoped(0, "hold F3D fixed fillet radius proof")?;
            let candidates = candidate_storage.with_storage(|| {
                radius_edge_identity_group_candidates(&identities, radius.get(), ctx)
            })?;
            if candidates.is_none() {
                return Ok(None);
            }
            // The proof keeps no slots. Destroy its actual vector before the
            // lease refunds the candidate backing.
            drop(candidates);
            drop(candidate_storage);
            *group
        };
        singleton = [group];
        singleton.as_slice()
    } else {
        return Ok(None);
    };
    let mut groups = Vec::new();
    let mut fillet_group_source = fixed.groups.iter().zip(edge_groups);
    while let Some((fixed_group, edge_group)) = ctx.next_charged(&mut fillet_group_source, "f3d edge projection source")? {
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
            crate::design::edge_resolve::EdgeTreatmentInputs {
                groups: construction_groups,
                operands: edge_operands,
                identity_operands: edge_identity_operands,
                vertex_operands,
                histories,
                previous_state_id: scope.previous_history_state_id(),
                feature_id: &crate::design::identity::neutral_feature_id(ctx, scope)?,
                treatment_radius: edge_radius,
            },
            ctx,
        )?;
        ctx.push_vec(
            &mut groups,
            FilletGroup {
                edges,
                radius,
                tangency_weight: fixed_group.tangency_weight().map(|tangency| tangency.value),
            },
            "f3d fixed fillet output group",
        )?;
    }
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Fillet {
            groups: or_none!(groups.try_into().ok()),
        },
    )))
}

#[cfg(test)]
mod tests;
