// SPDX-License-Identifier: Apache-2.0
//! Historical feature selection and persistent identity resolution.

use super::{
    bound_scope_history, design_axis, effective_scope_previous_history_state_id,
    historical_edge_axis, history_state_reaches, projection_was_finalized, same_axis_line,
    single_btree_value, stable_ref, treatment_edge_candidates, unique_history_state_pair,
    EPS_HISTORY_BIND_MIRROR_SELECTION_PLANES_E9, EPS_HISTORY_HISTORICAL_LOOP_PLANE_E9,
    EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E8, EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E9,
    HOLE_SUPPORT_NORMAL_TOLERANCE, HOLE_SUPPORT_POINT_TOLERANCE,
};
use crate::history_records::{AsmHistoricalTopology, AsmHistory};
use crate::records::{
    bodies::DesignBodyBinding,
    recipes::DesignComponentNamingSpace,
    topology::{
        body_recipe::AsmHistoricalEntityKind, edge_identity::DesignEdgeIdentityOperand,
        extrude_selection::DesignExtrudeSelectionMember, extrude_selection::DesignOperandRole,
    },
};
use cadmpeg_ir::scalar::FiniteReal;
use std::collections::{BTreeSet, HashMap, HashSet};

pub(super) fn boundary_edges_in_changes(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    boundary_edges: &[i64],
    changes: &[i64],
) -> Result<Vec<i64>, cadmpeg_core::CodecError> {
    let operation = "collect F3D boundary edges in changes";
    let mut selected = Vec::new();
    for edge in decode.admit_iter(boundary_edges, operation)? {
        if decode.contains(changes, edge, operation)? {
            decode.push_vec(&mut selected, *edge, operation)?;
        }
    }
    Ok(selected)
}

pub(super) fn recipe_selector_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    structure: Option<&crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>,
    contexts: &[crate::records::topology::historical_context::DesignHistoricalEdgeContext],
) -> Result<
    Vec<crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext>,
    cadmpeg_core::CodecError,
> {
    let Some(structure) = structure else {
        return Ok(Vec::new());
    };
    let mut selectors = BTreeSet::new();
    for side in decode.admit_iter(&structure.sides, "scan F3D edge recipe selector sides")? {
        for entry in decode.admit_iter(&side.entries, "scan F3D edge recipe selector entries")? {
            decode
                .insert_btree_set(
                    &mut selectors,
                    entry.selector,
                    "index F3D edge recipe selectors",
                )
                .map(|_| ())?;
        }
    }
    let mut selected = Vec::new();
    for selector in decode.admit_iter(&selectors, "scan F3D edge recipe selectors")? {
        let mut clauses = Vec::new();
        for side in decode.admit_iter(&structure.sides, "scan F3D structure sides")? {
            let entry_index = decode.position_by(
                &side.entries,
                |entry| Ok(entry.selector == *selector),
                "find F3D edge recipe selector entry",
            )?;
            let clause = if let Some(entry_index) = entry_index {
                let entry = &side.entries[entry_index];
                let [first, second] = entry.topology_triplets.each_ref().map(|triplet| {
                    let mut edge_slots = Vec::new();
                    for context in
                        decode.admit_iter(contexts, "scan F3D recipe triplet contexts")?
                    {
                        let has_incident = decode.any_by(
                            &context.incident_loops,
                            |incident| {
                                Ok(
                                    incident.boundary_edge_count == entry.boundary_edge_count.get()
                                        && triplet
                                            .incident
                                            .map(|incident| incident.ordinal)
                                            .is_some_and(|ordinal| {
                                                incident.coedge_ordinal == ordinal
                                            }),
                                )
                            },
                            "find F3D recipe triplet incident loop",
                        )?;
                        if has_incident {
                            decode.push_vec(
                                &mut edge_slots,
                                context.edge_slot,
                                "collect F3D recipe triplet edges",
                            )?;
                        }
                    }
                    Ok::<_, cadmpeg_core::CodecError>(edge_slots)
                });
                Some(
                    crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorClause {
                        entry: *entry,
                        triplet_edge_slots: [first?, second?],
                    },
                )
            } else {
                None
            };

            decode.reserve_vec(&mut clauses, 1, "collect F3D recipe selector clauses")?;
            clauses.push(clause);
        }
        let mut required_storage = decode.reserve_scoped(0, "collect F3D recipe side counts")?;
        let required = required_storage.with_storage(|| {
            decode.collect_vec(
                decode
                    .admit_iter(&clauses, "scan F3D recipe selector clauses")?
                    .map(|entry| {
                        entry
                            .as_ref()
                            .map(|entry| i64::from(entry.entry.boundary_edge_count.get()))
                    }),
                "collect F3D recipe side counts",
            )
        })?;
        let mut boundary_count_matching_edge_slots = Vec::new();
        for context in decode.admit_iter(contexts, "scan F3D recipe selector contexts")? {
            let mut counts_storage =
                decode.reserve_scoped(0, "collect F3D incident loop counts")?;
            let counts = counts_storage.with_storage(|| {
                decode.collect_vec(
                    decode
                        .admit_iter(
                            &context.incident_loops,
                            "scan F3D recipe selector incident loops",
                        )?
                        .map(|incident| i64::from(incident.boundary_edge_count)),
                    "collect F3D incident loop counts",
                )
            })?;
            if incident_loop_counts_satisfy_sides(decode, &counts, &required)? {
                decode.reserve_vec(
                    &mut boundary_count_matching_edge_slots,
                    1,
                    "collect F3D boundary count edge matches",
                )?;
                boundary_count_matching_edge_slots.push(context.edge_slot);
            }
        }
        let mut incidence_matching_edge_slots = Vec::new();
        for context in decode.admit_iter(contexts, "scan F3D incidence edge contexts")? {
            let matches_all_clauses = decode.all_by(
                &clauses,
                |clause| {
                    let Some(clause) = clause else {
                        return Ok(true);
                    };
                    decode.all_by(
                        &clause.entry.topology_triplets,
                        |triplet| {
                            decode.any_by(
                                &context.incident_loops,
                                |incident| {
                                    Ok(incident.boundary_edge_count
                                        == clause.entry.boundary_edge_count.get()
                                        && triplet
                                            .incident
                                            .map(|incident| incident.ordinal)
                                            .is_some_and(|ordinal| {
                                                ordinal == incident.coedge_ordinal
                                            }))
                                },
                                "find F3D incidence incident loop",
                            )
                        },
                        "match F3D incidence clause triplets",
                    )
                },
                "match F3D incidence selector clauses",
            )?;
            if matches_all_clauses {
                decode.push_vec(
                    &mut incidence_matching_edge_slots,
                    context.edge_slot,
                    "collect F3D incidence edge matches",
                )?;
            }
        }

        decode.reserve_vec(&mut selected, 1, "collect F3D edge recipe selectors")?;
        selected.push(
            crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext {
                selector: *selector,
                clauses,
                incidence_matching_edge_slots,
                boundary_count_matching_edge_slots,
            },
        );
    }
    Ok(selected)
}

pub(super) fn historical_edge_context(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    edge: i64,
    topology: &AsmHistoricalTopology,
) -> Result<
    crate::records::topology::historical_context::DesignHistoricalEdgeContext,
    cadmpeg_core::CodecError,
> {
    let operation = "collect F3D historical incident loops";
    let mut incident_loops = Vec::new();
    for coedge in decode.admit_iter(topology.coedge_topology.as_slice(), operation)? {
        if coedge.edge != edge {
            continue;
        }
        let Some(loop_relation) = decode.find_by(
            &topology.loop_coedges,
            |relation| Ok(relation.owner_ref == coedge.owner_loop),
            operation,
        )?
        else {
            continue;
        };
        let Some(ordinal) = decode.position_by(
            &loop_relation.member_refs,
            |candidate| Ok(candidate == &coedge.coedge),
            operation,
        )?
        else {
            continue;
        };
        let Some(boundary_edge_count) = u32::try_from(loop_relation.member_refs.len()).ok() else {
            continue;
        };
        let Some(coedge_ordinal) = u32::try_from(ordinal).ok() else {
            continue;
        };
        let Some(previous_coedge) = loop_relation
            .member_refs
            .get((ordinal + loop_relation.member_refs.len() - 1) % loop_relation.member_refs.len())
        else {
            continue;
        };
        let Some(next_coedge) = loop_relation
            .member_refs
            .get((ordinal + 1) % loop_relation.member_refs.len())
        else {
            continue;
        };
        let edge_for_coedge = |slot: &i64| {
            decode.find_by(
                &topology.coedge_topology,
                |candidate| Ok(candidate.coedge == *slot),
                operation,
            )
        };
        let Some(face_loop) = decode.find_by(
            &topology.face_loops,
            |relation| decode.contains(&relation.member_refs, &coedge.owner_loop, operation),
            operation,
        )?
        else {
            continue;
        };
        let Some(previous_edge_slot) = edge_for_coedge(previous_coedge)? else {
            continue;
        };
        let Some(next_edge_slot) = edge_for_coedge(next_coedge)? else {
            continue;
        };
        decode.push_vec(
            &mut incident_loops,
            crate::records::topology::historical_context::DesignHistoricalEdgeLoopContext {
                coedge_slot: coedge.coedge,
                loop_slot: coedge.owner_loop,
                face_slot: face_loop.owner_ref,
                boundary_edge_count,
                coedge_ordinal,
                previous_edge_slot: previous_edge_slot.edge,
                next_edge_slot: next_edge_slot.edge,
            },
            operation,
        )?;
    }
    decode.stable_sort_by(
        &mut incident_loops,
        |value| &value.coedge_slot,
        Ord::cmp,
        "sort F3D historical incident loops",
    )?;
    Ok(
        crate::records::topology::historical_context::DesignHistoricalEdgeContext {
            edge_slot: edge,
            incident_loops,
        },
    )
}

pub(super) fn incident_loop_counts_satisfy_sides(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    counts: &[i64],
    required: &[Option<i64>],
) -> Result<bool, cadmpeg_core::CodecError> {
    for (ordinal, value) in decode
        .admit_iter(required, "scan F3D recipe selector required sides")?
        .enumerate()
    {
        let Some(value) = value else {
            continue;
        };
        let available = decode
            .admit_iter(counts, "count F3D recipe selector incident loops")?
            .filter(|count| *count == value)
            .count();
        let needed = decode
            .admit_iter(
                &required[..=ordinal],
                "count F3D recipe selector required loop occurrences",
            )?
            .filter(|candidate| *candidate == &Some(*value))
            .count();
        if available < needed {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn bind_face_selection<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    scope: &'a crate::records::feature::scope::DesignParameterScope,
    index: &super::FaceSelectionIndex<'a, 'ctx>,
    updated_face_slots: &[i64],
) -> Result<(), cadmpeg_core::CodecError> {
    let cadmpeg_ir::features::FaceSelection::Native(native) = selection else {
        return Ok(());
    };
    if ctx.equal(
        native.as_str(),
        scope.id.as_str(),
        "compare F3D face selection scope identity",
    )? {
        if let Some(resolved) = crate::design::feature_project::direct_face_selection(
            ctx,
            scope,
            index.inputs.operands,
        )? {
            if !matches!(resolved, cadmpeg_ir::features::FaceSelection::Native(_)) {
                *selection = resolved;
            }
        }
        return Ok(());
    }
    let Some(group) = index.groups.get(ctx, native.as_str())? else {
        return Ok(());
    };
    if group.scope_record_index != scope.record_index {
        return Ok(());
    }
    let stream = super::native_stream_of(ctx, &scope.id)?;
    if !ctx.equal(
        &super::native_stream_of(ctx, &group.id)?,
        &stream,
        "compare F3D face selection group stream",
    )? {
        return Ok(());
    }
    if let Some(resolved) =
        crate::design::face_resolve::resolved_historical_split_face_target_group_with_updated_faces(
            ctx,
            scope,
            scope.previous_history_state_id(),
            group,
            index.inputs.operands,
            updated_face_slots,
        )?
    {
        *selection = resolved;
        return Ok(());
    }
    let Some(stream) = stream else {
        return Ok(());
    };
    let mut seen_storage = ctx.reserve_scoped(0, "index F3D resolved face selection")?;
    let mut seen = HashSet::new();
    let mut faces = Vec::new();
    for member in ctx.admit_iter(group.members(), "scan F3D face selection group members")? {
        let Some(operand) = index
            .face_operands
            .get(ctx, &(Some(stream), scope.record_index, member.value))?
        else {
            return Ok(());
        };
        let candidate = match operand.preceding_candidate_faces.as_slice() {
            [face] => face,
            _ => {
                let [face] = operand.changed_candidate_faces.as_slice() else {
                    return Ok(());
                };
                face
            }
        };
        if ctx.contains_hash_set(&seen, candidate, "index F3D resolved face selection")? {
            continue;
        }
        if !ctx.contains(
            &operand.candidate_faces,
            candidate,
            "find F3D candidate face selection face",
        )? {
            return Ok(());
        }
        seen_storage.with_storage(|| {
            ctx.insert_hash_set(&mut seen, candidate, "index F3D resolved face selection")
        })?;
        ctx.reserve_vec(&mut faces, 1, "collect F3D resolved face selection")?;
        let face = candidate.try_clone_for_decode(ctx, "copy F3D resolved face identity")?;
        faces.push(face);
    }
    if !faces.is_empty() {
        *selection = cadmpeg_ir::features::FaceSelection::Resolved {
            faces,
            native: ctx.copy_retained_text(native, "copy F3D resolved face selection identity")?,
        };
    }
    Ok(())
}

pub(super) fn bind_body_recipe_face_selection<'a, 'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    selection: &mut cadmpeg_ir::features::FaceSelection,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &'a crate::records::feature::scope::DesignParameterScope,
    index: &super::FaceSelectionIndex<'a, 'ctx>,
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::FaceSelection;

    let FaceSelection::Native(native) = selection else {
        return Ok(());
    };
    let Some(group) = index.groups.get(ctx, native.as_str())? else {
        return Ok(());
    };
    if group.scope_record_index != scope.record_index || group.role() != DesignOperandRole::ROLE_0X5
    {
        return Ok(());
    }
    let stream = super::native_stream_of(ctx, &scope.id)?;
    if !ctx.equal(
        &super::native_stream_of(ctx, &group.id)?,
        &stream,
        "compare F3D body recipe face selection group stream",
    )? || group.members().is_empty()
    {
        return Ok(());
    }
    let mut slots_storage = ctx.reserve_scoped(0, "collect F3D body recipe face slots")?;
    let mut seen = HashSet::new();
    let mut slots = Vec::new();
    for (ordinal, member) in ctx
        .admit_iter(
            group.members(),
            "scan F3D body recipe selection group members",
        )?
        .enumerate()
    {
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return Ok(());
        };
        let Some(operand) = index
            .body_recipe_operands
            .get(ctx, &(stream, group.record_index, ordinal, member.value))?
        else {
            return Ok(());
        };
        let Some(slot) = operand.resolved_face_slot else {
            return Ok(());
        };
        slots_storage.with_storage(|| {
            if ctx.insert_hash_set(&mut seen, slot, "collect F3D body recipe face slots")? {
                ctx.push_vec(&mut slots, slot, "collect F3D body recipe face slots")?;
            }
            Ok::<(), cadmpeg_core::CodecError>(())
        })?;
    }
    let state = crate::ids::history_input_state_id_charged(ctx, feature_id, previous_state_id)?;
    let mut faces = ctx.collection_vec(slots.len(), "collect F3D body recipe face identities")?;
    for slot in ctx.admit_iter(&slots, "scan F3D body recipe face slots")? {
        faces.push(crate::ids::history_input_face_id_charged(
            ctx,
            feature_id,
            previous_state_id,
            *slot,
        )?);
    }
    let native = ctx.copy_retained_text(native, "copy F3D body recipe face selection identity")?;
    if let Ok(historical) =
        cadmpeg_ir::features::FaceSelection::historical(state, faces, native, ctx)?
    {
        *selection = historical;
    }
    Ok(())
}

pub(super) fn faces_in_topology(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &[cadmpeg_ir::ids::FaceId],
    topology: &AsmHistoricalTopology,
) -> Result<Vec<cadmpeg_ir::ids::FaceId>, cadmpeg_core::CodecError> {
    let mut faces = HashSet::new();
    for face in decode.admit_iter(&topology.faces, "scan F3D topology faces")? {
        decode.insert_hash_set(&mut faces, *face, "index F3D topology faces")?;
    }
    let mut selected = Vec::new();
    for face in decode.admit_iter(candidates, "scan F3D face candidates")? {
        let Some(slot) = stable_ref(decode, face.as_str())? else {
            continue;
        };
        if !decode.contains_hash_set(&faces, &slot, "find F3D candidate topology face")? {
            continue;
        }
        let face = face.try_clone_for_decode(decode, "copy F3D historical face identity")?;
        decode.push_vec(&mut selected, face, "collect F3D faces in topology")?;
    }
    Ok(selected)
}

/// Topology families and states containing one requested stable ASM slot.
/// A slot found in more than one family has no single kind.
#[derive(Default)]
struct HistoricalIdentityMembership {
    kind: Option<AsmHistoricalEntityKind>,
    kind_is_ambiguous: bool,
    states: Vec<i64>,
}

#[derive(Default)]
struct HistoricalRevisionMembership {
    entity_refs: BTreeSet<i64>,
    states: Vec<i64>,
}

/// Ambiguity-aware ASM identity data restricted to the Design identities a
/// binding pass requests.
pub(super) struct HistoricalIdentityIndex {
    identities: HashMap<i64, HistoricalIdentityMembership>,
    revisions: HashMap<i64, HistoricalRevisionMembership>,
}

impl HistoricalIdentityIndex {
    pub(super) fn build<'a, I>(
        decode: &cadmpeg_core::decode::DecodeContext<'_>,
        histories: impl IntoIterator<Item = &'a AsmHistory>,
        local_ids: cadmpeg_core::decode::scan::AdmittedIter<I>,
        mut project_local_ids: impl FnMut(
            I::Item,
        ) -> std::iter::Chain<
            std::iter::Once<u64>,
            std::option::IntoIter<u64>,
        >,
    ) -> Result<Self, cadmpeg_core::CodecError>
    where
        I: Iterator,
    {
        let histories = decode.collect_vec(histories, "collect F3D identity histories")?;
        let mut record_refs = HashSet::new();
        for source_item in local_ids {
            for local_id in project_local_ids(source_item) {
                let Some(record_ref) = i64::try_from(local_id).ok() else {
                    continue;
                };
                decode.insert_hash_set(
                    &mut record_refs,
                    record_ref,
                    "index F3D identity record refs",
                )?;
            }
        }
        let mut identities = HashMap::<i64, HistoricalIdentityMembership>::new();
        let mut revisions = HashMap::<i64, HistoricalRevisionMembership>::new();
        if record_refs.is_empty() {
            return Ok(Self {
                identities,
                revisions,
            });
        }
        if decode.any_by(
            &histories,
            |history| Ok(history.record_table_binding_budget_exceeded),
            "scan F3D identity history limits",
        )? {
            return Ok(Self {
                identities,
                revisions,
            });
        }
        let ambiguous_states = ambiguous_history_state_ids(decode, &histories)?;
        // Revisions named by an entity version, and every entity a revision
        // reaches; both grow as the revisions are indexed.
        let mut versioned_revisions = HashSet::new();
        let mut revision_entity_refs = HashSet::new();
        for history in decode.admit_iter(&histories, "scan F3D identity revision histories")? {
            for state in decode.admit_iter(&history.states, "scan F3D identity revision states")? {
                if ambiguous_states.contains(&state.state_id) {
                    continue;
                }
                for version in
                    decode.admit_iter(&state.entity_versions, "scan F3D state entity versions")?
                {
                    if record_refs.contains(&version.record_ref) {
                        if !revisions.contains_key(&version.record_ref) {
                            decode.reserve_map(
                                &mut revisions,
                                1,
                                "index F3D revision membership",
                            )?;
                            decode.insert_hash_set(
                                &mut versioned_revisions,
                                version.record_ref,
                                "index F3D versioned revisions",
                            )?;
                        }
                        let membership = revisions.entry(version.record_ref).or_default();
                        decode.insert_btree_set(
                            &mut membership.entity_refs,
                            version.entity_ref,
                            "index F3D revision entity refs",
                        )?;
                        decode.insert_hash_set(
                            &mut revision_entity_refs,
                            version.entity_ref,
                            "index F3D identity entity refs",
                        )?;
                        if !decode.contains(
                            &membership.states,
                            &state.state_id,
                            "find F3D revision membership state",
                        )? {
                            decode.reserve_vec(
                                &mut membership.states,
                                1,
                                "collect F3D revision states",
                            )?;
                            membership.states.push(state.state_id);
                        }
                    }
                }
            }
        }
        let mut reconstructed_revisions = BTreeSet::new();
        // Finalized histories retain the entity-slot membership and the
        // bulletin-board chain after their geometry caches are compacted.
        for history in decode.admit_iter(&histories, "collect F3D finalized identity histories")? {
            if history.states.is_empty() {
                continue;
            }
            let finalized = decode.all_by(
                &history.states,
                |state| match &state.topology_cache {
                    crate::history_records::AsmTopologyCache::Absent
                    | crate::history_records::AsmTopologyCache::Released => Ok(false),
                    crate::history_records::AsmTopologyCache::Complete(_) => Ok(true),
                    crate::history_records::AsmTopologyCache::Retained(_) => {
                        history.projection_finalized(decode)
                    }
                },
                "check F3D finalized identity history states",
            )?;
            if !finalized {
                continue;
            }
            for state in
                decode.admit_iter(&history.states, "scan F3D reconstructed identity states")?
            {
                for board in decode
                    .admit_iter(&state.bulletin_boards, "scan F3D identity bulletin boards")?
                {
                    for change in
                        decode.admit_iter(&board.changes, "scan F3D identity bulletin changes")?
                    {
                        let Some(record_ref) =
                            change.old_ref().filter(|old| record_refs.contains(old))
                        else {
                            continue;
                        };
                        let entity_ref = change.new_ref().unwrap_or(record_ref);
                        if !revisions.contains_key(&record_ref) {
                            decode.reserve_map(
                                &mut revisions,
                                1,
                                "index F3D reconstructed revisions",
                            )?;
                        }
                        let revision = revisions.entry(record_ref).or_default();
                        decode.insert_btree_set(
                            &mut revision.entity_refs,
                            entity_ref,
                            "index F3D reconstructed entity refs",
                        )?;
                        decode.insert_hash_set(
                            &mut revision_entity_refs,
                            entity_ref,
                            "index F3D identity entity refs",
                        )?;
                        if !versioned_revisions.contains(&record_ref) {
                            decode.insert_btree_set(
                                &mut reconstructed_revisions,
                                record_ref,
                                "index F3D reconstructed revision refs",
                            )?;
                        }
                    }
                }
            }
        }
        for history in decode.admit_iter(&histories, "scan F3D identity membership histories")? {
            for state in
                decode.admit_iter(&history.states, "scan F3D identity membership states")?
            {
                if ambiguous_states.contains(&state.state_id) {
                    continue;
                }
                let Some(topology) = state.topology() else {
                    continue;
                };
                let families: [(AsmHistoricalEntityKind, &[i64]); 12] = [
                    (AsmHistoricalEntityKind::Body, &topology.bodies),
                    (AsmHistoricalEntityKind::Region, &topology.regions),
                    (AsmHistoricalEntityKind::Shell, &topology.shells),
                    (AsmHistoricalEntityKind::Face, &topology.faces),
                    (AsmHistoricalEntityKind::Loop, &topology.loops),
                    (AsmHistoricalEntityKind::Coedge, &topology.coedges),
                    (AsmHistoricalEntityKind::Edge, &topology.edges),
                    (AsmHistoricalEntityKind::Vertex, &topology.vertices),
                    (AsmHistoricalEntityKind::Point, &topology.points),
                    (AsmHistoricalEntityKind::Surface, &topology.surfaces),
                    (AsmHistoricalEntityKind::Curve, &topology.curves),
                    (AsmHistoricalEntityKind::Pcurve, &topology.pcurves),
                ];
                for (kind, members) in families {
                    for entity_ref in decode
                        .admit_iter(members, "scan F3D identity topology family members")?
                        .filter(|entity_ref| {
                            record_refs.contains(entity_ref)
                                || revision_entity_refs.contains(entity_ref)
                        })
                    {
                        if !identities.contains_key(entity_ref) {
                            decode.reserve_map(
                                &mut identities,
                                1,
                                "index F3D identity membership",
                            )?;
                        }
                        let membership = identities.entry(*entity_ref).or_default();
                        match membership.kind {
                            None => membership.kind = Some(kind),
                            Some(known) if known != kind => membership.kind_is_ambiguous = true,
                            Some(_) => {}
                        }
                        if !decode.contains(
                            &membership.states,
                            &state.state_id,
                            "find F3D identity membership state",
                        )? {
                            decode.reserve_vec(
                                &mut membership.states,
                                1,
                                "collect F3D identity states",
                            )?;
                            membership.states.push(state.state_id);
                        }
                    }
                }
            }
        }
        for record_ref in decode.admit_iter(
            &reconstructed_revisions,
            "scan F3D reconstructed revision refs",
        )? {
            let Some(revision) = decode.get_mut_hash_map(
                &mut revisions,
                record_ref,
                "find F3D reconstructed revision",
            )?
            else {
                continue;
            };
            let mut revision_states = Vec::new();
            for entity_ref in decode.admit_iter(
                &revision.entity_refs,
                "scan F3D reconstructed revision entity refs",
            )? {
                if let Some(membership) = identities.get(entity_ref) {
                    for state_id in decode.admit_iter(
                        &membership.states,
                        "scan F3D reconstructed revision membership states",
                    )? {
                        decode.push_vec(
                            &mut revision_states,
                            *state_id,
                            "collect F3D reconstructed revision states",
                        )?;
                    }
                }
            }
            revision.states = revision_states;
            decode.sort_unstable_by(
                &mut revision.states,
                |value| value,
                Ord::cmp,
                "sort F3D reconstructed revision states",
            )?;
            decode.dedup_vec(
                &mut revision.states,
                "deduplicate F3D reconstructed revision states",
            )?;
        }
        Ok(Self {
            identities,
            revisions,
        })
    }

    fn identity_kind(
        &self,
        decode: &cadmpeg_core::decode::DecodeContext<'_>,
        local_id: u64,
    ) -> Result<Option<(AsmHistoricalEntityKind, Vec<i64>)>, cadmpeg_core::CodecError> {
        let Some(entity_ref) = i64::try_from(local_id).ok() else {
            return Ok(None);
        };
        let Some(membership) = self.identities.get(&entity_ref) else {
            return Ok(None);
        };
        let Some(kind) = membership.kind.filter(|_| !membership.kind_is_ambiguous) else {
            return Ok(None);
        };
        let states = decode.collect_vec(
            membership.states.iter().copied(),
            "copy F3D identity states",
        )?;
        Ok(Some((kind, states)))
    }

    fn selection_identity_kind(
        &self,
        decode: &cadmpeg_core::decode::DecodeContext<'_>,
        local_id: u64,
    ) -> Result<Option<(AsmHistoricalEntityKind, i64, Vec<i64>)>, cadmpeg_core::CodecError> {
        let Some(record_ref) = i64::try_from(local_id).ok() else {
            return Ok(None);
        };
        let revision = self.revisions.get(&record_ref);
        if let Some((kind, states)) = self.identity_kind(decode, local_id)? {
            let unrevised = match revision {
                None => true,
                Some(revision) => {
                    revision.entity_refs.is_empty()
                        || (revision.entity_refs.len() == 1
                            && decode.contains_btree_set(
                                &revision.entity_refs,
                                &record_ref,
                                "find F3D unrevised identity entity",
                            )?)
                }
            };
            return Ok(unrevised.then_some((kind, record_ref, states)));
        }
        let Some(revision) = revision else {
            return Ok(None);
        };
        if revision.entity_refs.len() != 1 {
            return Ok(None);
        }
        let Some(entity_ref) = decode
            .admit_iter(&revision.entity_refs, "take F3D revised identity entity")?
            .next()
            .copied()
        else {
            return Ok(None);
        };
        let Some(entity_id) = u64::try_from(entity_ref).ok() else {
            return Ok(None);
        };
        let Some((kind, _)) = self.identity_kind(decode, entity_id)? else {
            return Ok(None);
        };
        let states =
            decode.collect_vec(revision.states.iter().copied(), "copy F3D revision states")?;
        Ok(Some((kind, entity_ref, states)))
    }
}

#[cfg(test)]
pub(super) fn historical_identity_kind(
    histories: &[AsmHistory],
    local_id: u64,
) -> Option<(AsmHistoricalEntityKind, Vec<i64>)> {
    crate::test_support::with_decode_context(|decode_ctx| {
        HistoricalIdentityIndex::build(
            decode_ctx,
            histories,
            decode_ctx
                .admit_iter(
                    std::slice::from_ref(&local_id),
                    "scan F3D identity local IDs",
                )
                .expect("test identity local ID admission"),
            |local_id| std::iter::once(*local_id).chain(None),
        )
        .expect("test identity index allocation")
        .identity_kind(decode_ctx, local_id)
    })
    .expect("test identity kind allocation")
}

pub(super) fn historical_selection_identity_kind(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &[AsmHistory],
    local_id: u64,
) -> Result<Option<(AsmHistoricalEntityKind, i64, Vec<i64>)>, cadmpeg_core::CodecError> {
    HistoricalIdentityIndex::build(
        decode,
        histories,
        decode.admit_iter(
            std::slice::from_ref(&local_id),
            "scan F3D identity local IDs",
        )?,
        |local_id| std::iter::once(*local_id).chain(None),
    )?
    .selection_identity_kind(decode, local_id)
}

fn ambiguous_history_state_ids(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    histories: &[&AsmHistory],
) -> Result<HashSet<i64>, cadmpeg_core::CodecError> {
    let mut unique = HashSet::new();
    let mut ambiguous = HashSet::new();
    for history in decode.admit_iter(histories, "scan F3D ambiguity histories")? {
        for state in decode.admit_iter(&history.states, "scan F3D ambiguity history states")? {
            if !decode.insert_hash_set(
                &mut unique,
                state.state_id,
                "index F3D unique history states",
            )? {
                decode.insert_hash_set(
                    &mut ambiguous,
                    state.state_id,
                    "index F3D ambiguous history states",
                )?;
            }
        }
    }
    Ok(ambiguous)
}

/// Select the complete BREP-history set owned by one component context.
/// `None` means the stream predates component naming-space registrations and
/// retains the aggregate compatibility path. `Some(empty)` is a known
/// component with no history-bearing BREP.
fn component_histories<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    context_id: &str,
    naming_spaces: &[DesignComponentNamingSpace],
    body_bindings: &[DesignBodyBinding],
    histories: &'a [AsmHistory],
) -> Result<Option<Vec<&'a AsmHistory>>, cadmpeg_core::CodecError> {
    if naming_spaces.is_empty() {
        return Ok(None);
    }
    let mut matches_context = |space: &DesignComponentNamingSpace| {
        decode.eq_ignore_ascii_case(
            space.context_uuid.as_str(),
            context_id,
            "compare F3D component context UUID",
        )
    };
    let Some(space_index) = decode.position_by(
        naming_spaces,
        &mut matches_context,
        "find F3D component naming space",
    )?
    else {
        return Ok(Some(Vec::new()));
    };
    if decode
        .position_by(
            &naming_spaces[space_index + 1..],
            &mut matches_context,
            "find F3D component naming space",
        )?
        .is_some()
    {
        return Ok(Some(Vec::new()));
    }
    let space = &naming_spaces[space_index];
    let Some(stream) = crate::ids::native_stream(&space.id) else {
        return Ok(Some(Vec::new()));
    };
    let cluster_end = decode.fold(
        naming_spaces,
        None,
        |minimum: Option<u64>, candidate| {
            let Some(candidate_stream) = crate::ids::native_stream(&candidate.id) else {
                return Ok(minimum);
            };
            if !decode.equal(
                candidate_stream,
                stream,
                "match F3D component naming-space stream",
            )? || candidate.component_record_index <= space.component_record_index
            {
                return Ok(minimum);
            }
            let Some(current_minimum) = minimum else {
                return Ok(Some(candidate.component_record_index));
            };
            Ok(
                match decode.compare(
                    &current_minimum,
                    &candidate.component_record_index,
                    "find F3D component history cluster end",
                )? {
                    std::cmp::Ordering::Greater => Some(candidate.component_record_index),
                    _ => Some(current_minimum),
                },
            )
        },
        "find F3D component history cluster end",
    )?;
    let mut blobs_storage = decode.reserve_scoped(0, "index F3D component history blobs")?;
    let mut blobs = BTreeSet::new();
    for binding in decode.admit_iter(body_bindings, "scan F3D component body bindings")? {
        let Some(binding_stream) = crate::ids::native_stream(binding.id()) else {
            continue;
        };
        if !decode.equal(
            binding_stream,
            stream,
            "match F3D component body binding stream",
        )? || binding.entity_suffix < space.component_record_index
            || cluster_end.is_some_and(|end| binding.entity_suffix >= end)
        {
            continue;
        }
        decode.insert_scoped_btree_set(
            &mut blobs_storage,
            &mut blobs,
            binding.blob_name(),
            "index F3D component history blobs",
            "index F3D component history blobs",
        )?;
    }
    let mut selected = Vec::new();
    for history in decode.admit_iter(histories, "scan F3D component histories")? {
        let Some(history_stream) = crate::ids::native_stream(&history.id) else {
            continue;
        };
        let Some(history_entry) = decode.strip_prefix(
            history_stream,
            crate::ids::SCHEME_PREFIX,
            "strip F3D component history scheme prefix",
        )?
        else {
            continue;
        };
        let Some(encoded_basename) = history_entry.rsplit('/').next() else {
            continue;
        };
        if decode.any_by(
            &blobs,
            |blob| {
                Ok(crate::ids::encoded_identity_key_component_matches(
                    encoded_basename,
                    blob,
                ))
            },
            "match F3D component history blobs",
        )? {
            decode.reserve_vec(&mut selected, 1, "collect F3D component histories")?;
            selected.push(history);
        }
    }
    decode.stable_sort_by(
        &mut selected,
        |value| &value.id,
        Ord::cmp,
        "sort F3D component histories",
    )?;
    decode.dedup_by(
        &mut selected,
        |left, right| decode.equal(&left.id, &right.id, "compare F3D component history IDs"),
        "compact F3D component history IDs",
    )?;
    Ok(Some(selected))
}

pub(crate) fn historical_extrude_selection_identity_kind(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    member: &DesignExtrudeSelectionMember,
    naming_spaces: &[DesignComponentNamingSpace],
    body_bindings: &[DesignBodyBinding],
    histories: &[AsmHistory],
) -> Result<Option<(AsmHistoricalEntityKind, i64, Vec<i64>)>, cadmpeg_core::CodecError> {
    match component_histories(
        decode,
        member.context_id.as_str(),
        naming_spaces,
        body_bindings,
        histories,
    )? {
        Some(selected) => HistoricalIdentityIndex::build(
            decode,
            selected,
            decode.admit_iter(
                std::slice::from_ref(&member.local_id),
                "scan F3D identity local IDs",
            )?,
            |local_id| std::iter::once(*local_id).chain(None),
        )?
        .selection_identity_kind(decode, member.local_id),
        None => historical_selection_identity_kind(decode, histories, member.local_id),
    }
}

pub(crate) fn bind_extrude_selection_history(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    members: &mut [DesignExtrudeSelectionMember],
    naming_spaces: &[DesignComponentNamingSpace],
    body_bindings: &[DesignBodyBinding],
    histories: &[AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    for member in members {
        member.historical = None;
        if let Some((kind, entity_ref, states)) = historical_extrude_selection_identity_kind(
            decode,
            member,
            naming_spaces,
            body_bindings,
            histories,
        )? {
            member.historical = Some(crate::records::topology::fillet::HistoricalBinding {
                kind,
                entity_ref,
                state_ids: states,
            });
        }
    }
    Ok(())
}

/// Resolve both identities in nested entity-selection operands against the
/// owning feature's exact input topology.
pub(crate) fn bind_entity_selection_history(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    let identities = HistoricalIdentityIndex::build(
        decode,
        histories,
        decode.admit_iter(&*operands, "scan F3D selection identity operands")?,
        |operand| {
            std::iter::once(operand.primary_identity).chain(
                operand
                    .secondary()
                    .map(|secondary| secondary.identity.value),
            )
        },
    )?;
    for operand in operands {
        decode.clear_vec(
            &mut operand.historical_edge_candidates,
            "clear F3D selection edge candidates",
        )?;
        decode.clear_vec(
            &mut operand.historical_face_candidates,
            "clear F3D selection face candidates",
        )?;
        operand.resolved_edge_slot = None;
        operand.historical_face_candidates =
            entity_selection_face_candidates(decode, operand.primary_identity, histories)?;
        let stream = crate::ids::native_stream(&operand.id);
        let matches_scope = |scope: &crate::records::feature::scope::DesignParameterScope| {
            if scope.record_index != operand.scope_record_index {
                return Ok(false);
            }
            decode.equal(
                &crate::ids::native_stream(&scope.id),
                &stream,
                "compare F3D selection entity scope stream",
            )
        };
        let Some(scope_index) =
            decode.position_by(scopes, &matches_scope, "find F3D selection entity scope")?
        else {
            continue;
        };
        if decode
            .position_by(
                &scopes[scope_index + 1..],
                matches_scope,
                "find F3D selection entity scope",
            )?
            .is_some()
        {
            continue;
        }
        let scope = &scopes[scope_index];
        let Some(previous_state_id) = scope.previous_history_state_id() else {
            continue;
        };
        let mut matching_state = None;
        let mut ambiguous_state = false;
        'state_search: for history in
            decode.admit_iter(histories, "scan F3D selection previous-state histories")?
        {
            let mut matches_previous = |state: &crate::history_records::AsmDeltaState| {
                decode.equal(
                    &state.state_id,
                    &previous_state_id,
                    "compare F3D selection previous state ID",
                )
            };
            let Some(state_index) = decode.position_by(
                &history.states,
                &mut matches_previous,
                "find F3D selection previous state",
            )?
            else {
                continue;
            };
            if matching_state.is_some()
                || decode
                    .position_by(
                        &history.states[state_index + 1..],
                        &mut matches_previous,
                        "find F3D selection previous state",
                    )?
                    .is_some()
            {
                ambiguous_state = true;
                break 'state_search;
            }
            matching_state = Some(&history.states[state_index]);
        }
        if ambiguous_state {
            continue;
        }
        let Some(state) = matching_state else {
            continue;
        };
        let Some(topology) = state.topology() else {
            continue;
        };
        let selections = std::iter::once((0, operand.primary_identity)).chain(
            operand
                .secondary()
                .map(|secondary| (1, secondary.identity.value)),
        );
        operand.historical_edge_candidates = entity_selection_edge_candidates(
            decode,
            selections,
            previous_state_id,
            &identities,
            topology,
        )?;
        operand.resolved_edge_slot =
            unique_entity_selection_edge(decode, &operand.historical_edge_candidates)?;
    }
    Ok(())
}

/// Resolve direct persistent face selections carried by Hole constructions.
pub(crate) fn bind_hole_selection_history(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scopes: &mut [crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    for scope in scopes {
        let history_state_id = scope.history_state_id();
        let previous_history_state_id = scope.previous_history_state_id();
        let Some(construction) = scope.hole_construction_mut() else {
            continue;
        };
        let Some(selection) = &mut construction.face_selection else {
            continue;
        };
        decode.clear_vec(
            &mut selection.historical_face_candidates,
            "clear F3D historical face candidates",
        )?;
        selection.historical_face_candidates =
            entity_selection_face_candidates(decode, selection.primary_identity, histories)?;
        if selection.historical_face_candidates.is_empty() {
            if let Some(candidate) = hole_transition_face_candidate(
                decode,
                selection,
                construction.position.map(FiniteReal::get),
                construction.direction.map(FiniteReal::get),
                history_state_id,
                previous_history_state_id,
                histories,
            )? {
                decode.reserve_vec(
                    &mut selection.historical_face_candidates,
                    1,
                    "collect F3D hole support candidates",
                )?;
                selection.historical_face_candidates.push(candidate);
            }
        }
    }
    Ok(())
}

/// Resolve the support face of an edge-backed Hole selection from its exact
/// feature transition. Fusion can serialize the support selector as an edge
/// even when the operation changes a planar support face: the transition then
/// inserts the drill cylinder and updates the support boundary. The admission
/// requires one coaxial inserted cylinder and one updated preceding plane whose
/// oriented normal agrees with the Hole direction. Generic edge-to-face
/// selection remains intentionally ambiguous outside this Hole-specific proof.
fn hole_transition_face_candidate(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    selection: &crate::records::feature::hole::DesignHoleFaceSelection,
    position: [f64; 3],
    direction: [f64; 3],
    state_id: Option<i64>,
    previous_state_id: Option<i64>,
    histories: &[AsmHistory],
) -> Result<
    Option<crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate>,
    cadmpeg_core::CodecError,
> {
    use crate::records::topology::{
        body_recipe::AsmHistoricalEntityKind, entity_selection::DesignEntitySelectionFaceCandidate,
    };
    macro_rules! some {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }

    let primary_identity = selection.primary_identity;
    let secondary_identity = selection
        .secondary
        .map(|secondary| secondary.identity.value);

    if secondary_identity.is_some() {
        return Ok(None);
    }
    let (state_id, previous_state_id) = (some!(state_id), some!(previous_state_id));
    let (kind, entity_ref, identity_states) = some!(historical_selection_identity_kind(
        decode,
        histories,
        primary_identity
    )?);
    if kind != AsmHistoricalEntityKind::Edge
        || !decode.contains(
            &identity_states,
            &previous_state_id,
            "find F3D Hole identity state",
        )?
    {
        return Ok(None);
    }
    let (history, result_state, preceding_state) = some!(unique_history_state_pair(
        decode,
        histories,
        state_id,
        previous_state_id
    )?);
    let transition = some!(result_state.transition.as_ref());
    if transition.previous_state_id != Some(previous_state_id) {
        return Ok(None);
    }
    let result_topology = some!(result_state.topology());
    let preceding_topology = some!(preceding_state.topology());
    let point = cadmpeg_ir::math::Point3::new(position[0], position[1], position[2]);
    if position.iter().any(|coordinate| !coordinate.is_finite()) {
        return Ok(None);
    }
    let direction = cadmpeg_ir::math::Vector3::new(direction[0], direction[1], direction[2]);
    let direction = some!(direction.unit());

    let mut cylinder_surfaces = HashSet::new();
    let mut cylinders = Vec::new();
    for face in decode.admit_iter(
        &transition.topology.faces.inserted,
        "scan F3D transition topology faces inserted",
    )? {
        let mut bindings = result_topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == *face);
        let Some(binding) = bindings.next() else {
            continue;
        };
        if bindings.next().is_some()
            || !decode.insert_hash_set(
                &mut cylinder_surfaces,
                binding.carrier,
                "index F3D Hole cylinder surfaces",
            )?
        {
            continue;
        }
        let mut carriers = result_topology
            .surface_cylinders
            .iter()
            .filter(|surface| surface.surface == binding.carrier);
        let Some(cylinder) = carriers.next() else {
            continue;
        };
        if carriers.next().is_some() {
            return Ok(None);
        }

        decode.reserve_vec(&mut cylinders, 1, "collect F3D Hole cylinders")?;
        cylinders.push(cylinder);
    }
    if cylinders.is_empty()
        || decode
            .admit_iter(&cylinders, "scan F3D Hole cylinder axes")?
            .any(|cylinder| {
                let axis = cylinder.axis.unit();
                axis.is_none_or(|axis| !same_axis_line((cylinder.origin, axis), (point, direction)))
            })
    {
        return Ok(None);
    }

    let mut preceding_faces = HashSet::new();
    for face in decode.admit_iter(
        &preceding_topology.faces,
        "scan F3D preceding topology faces",
    )? {
        decode.insert_hash_set(
            &mut preceding_faces,
            *face,
            "index F3D Hole preceding faces",
        )?;
    }
    let surface_scale = decode
        .admit_iter(
            &preceding_topology.surface_planes,
            "measure F3D Hole support surface coordinates",
        )?
        .fold(0.0_f64, |maximum, plane| {
            maximum
                .max(plane.origin.x.abs())
                .max(plane.origin.y.abs())
                .max(plane.origin.z.abs())
        });
    let scale = [point.x.abs(), point.y.abs(), point.z.abs(), surface_scale]
        .into_iter()
        .fold(1.0, f64::max);
    let mut candidates = decode.collect_vec(
        decode
            .admit_iter(
                &transition.topology.faces.updated,
                "scan F3D Hole updated faces",
            )?
            .copied()
            .filter(|face| preceding_faces.contains(face))
            .filter_map(|face| {
                let mut bindings = preceding_topology
                    .face_surfaces
                    .iter()
                    .filter(|binding| binding.entity == face);
                let binding = bindings.next()?;
                if bindings.next().is_some() {
                    return None;
                }
                let mut planes = preceding_topology
                    .surface_planes
                    .iter()
                    .filter(|plane| plane.surface == binding.carrier);
                let plane = planes.next()?;
                if planes.next().is_some() {
                    return None;
                }
                let normal = plane.normal.unit()?;
                let point_distance = point.vector_from(plane.origin).dot(normal).abs();
                ((normal.dot(direction) - 1.0).abs() <= HOLE_SUPPORT_NORMAL_TOLERANCE
                    && point_distance <= HOLE_SUPPORT_POINT_TOLERANCE * scale)
                    .then_some(face)
            }),
        "collect F3D Hole transition faces",
    )?;
    decode.sort_unstable_by(
        &mut candidates,
        |value| value,
        Ord::cmp,
        "sort F3D Hole transition faces",
    )?;
    decode.dedup_vec(&mut candidates, "deduplicate F3D Hole transition faces")?;
    let [face_slot] = candidates.as_slice() else {
        return Ok(None);
    };
    let history_id = {
        let ctx = decode;
        ctx.copy_retained_text(&history.id, "copy F3D Hole history identity")?
    };
    Ok(Some(DesignEntitySelectionFaceCandidate {
        history_id,
        historical: crate::records::topology::fillet::HistoricalBinding {
            kind,
            entity_ref,
            state_ids: vec![previous_state_id],
        },
        face_slot: *face_slot,
    }))
}

/// Resolve persistent circular-pattern axis identities in the feature input topology.
pub(crate) fn bind_circular_pattern_axes(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scopes: &mut [crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    use crate::records::feature::patterns::DesignCircularPatternAxis;
    for scope in scopes.iter_mut() {
        let operation = "find F3D circular pattern history";
        let mut history_matches = |history: &AsmHistory| {
            let history_binding = decode.get_hash_map(
                scope_histories,
                &scope.id,
                "find F3D circular pattern scope binding",
            )?;
            let matches_binding = match history_binding {
                Some(history_id) => {
                    decode.equal(history.id.as_str(), history_id.as_str(), operation)?
                }
                None => false,
            };
            if matches_binding {
                return Ok(true);
            }
            Ok(decode
                .get_hash_map(
                    scope_histories,
                    &scope.id,
                    "check F3D circular pattern fallback binding",
                )?
                .is_none()
                && histories.len() == 1)
        };
        let first_index = decode.position_by(histories, &mut history_matches, operation)?;
        let Some(history_index) = first_index else {
            continue;
        };
        let later_match = decode.position_by(
            &histories[history_index + 1..],
            &mut history_matches,
            operation,
        )?;
        if later_match.is_some() {
            continue;
        }
        let history = &histories[history_index];
        let input_state_id = effective_scope_previous_history_state_id(
            decode,
            scope,
            std::slice::from_ref(history),
        )?;
        let Some(construction) = scope.circular_pattern_construction_mut() else {
            continue;
        };
        let DesignCircularPatternAxis::HistoricalEdge {
            persistent_identity,
            resolved,
            ..
        } = &mut construction.axis
        else {
            continue;
        };
        *resolved = None;
        let identities = HistoricalIdentityIndex::build(
            decode,
            std::slice::from_ref(history),
            decode.admit_iter(
                std::slice::from_ref(&*persistent_identity),
                "scan F3D identity local IDs",
            )?,
            |local_id| std::iter::once(*local_id).chain(None),
        )?;
        let axis_candidates = historical_pattern_identity_axes(
            decode,
            *persistent_identity,
            &identities,
            history,
            input_state_id,
        )?;
        let mut axes =
            decode.admit_iter(&axis_candidates, "scan F3D circular pattern identity axes")?;
        let Some(axis) = axes.next() else {
            continue;
        };
        let axis_line = (axis.origin.get(), *axis.direction.as_raw());
        if axes.any(|candidate| {
            !same_axis_line(
                axis_line,
                (candidate.origin.get(), *candidate.direction.as_raw()),
            )
        }) {
            continue;
        }
        *resolved = Some(*axis);
    }
    Ok(())
}

pub(super) fn historical_pattern_identity_axes(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    identity: u64,
    identities: &HistoricalIdentityIndex,
    history: &AsmHistory,
    input_state_id: Option<i64>,
) -> Result<Vec<crate::records::feature::patterns::DesignAxis>, cadmpeg_core::CodecError> {
    if let Some((kind, entity_ref, state_ids)) =
        identities.selection_identity_kind(decode, identity)?
    {
        let state_ids = if let Some(input_state_id) = input_state_id {
            if !decode.contains(&state_ids, &input_state_id, "find F3D pattern input state")? {
                return Ok(Vec::new());
            }
            vec![input_state_id]
        } else {
            state_ids
        };
        return historical_pattern_identity_axes_for_selection(
            decode,
            Some((kind, entity_ref, &state_ids)),
            history,
        );
    }
    let Some(revision) = snapshot_edge_identity_revision(identity, history) else {
        return Ok(Vec::new());
    };
    let archived = HistoricalIdentityIndex::build(
        decode,
        std::slice::from_ref(history),
        decode.admit_iter(
            std::slice::from_ref(&revision),
            "scan F3D identity local IDs",
        )?,
        |local_id| std::iter::once(*local_id).chain(None),
    )?;
    let Some((kind, entity_ref, state_ids)) = archived.selection_identity_kind(decode, revision)?
    else {
        return Ok(Vec::new());
    };
    let state_ids = if let Some(input_state_id) = input_state_id {
        if !decode.contains(&state_ids, &input_state_id, "find F3D pattern input state")? {
            return Ok(Vec::new());
        }
        vec![input_state_id]
    } else {
        state_ids
    };
    historical_pattern_identity_axes_for_selection(
        decode,
        Some((kind, entity_ref, &state_ids)),
        history,
    )
}

pub(super) fn snapshot_edge_identity_revision(identity: u64, history: &AsmHistory) -> Option<u64> {
    let mut matches = history
        .states
        .iter()
        .flat_map(|state| &state.records)
        .filter(|record| match &record.framing {
            crate::history_records::AsmHistoryRecordFraming::Framed { index, .. } => {
                *index == identity
            }
            crate::history_records::AsmHistoryRecordFraming::Opaque { .. } => identity == 0,
        });
    let record = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    (record.name() == "edge")
        .then_some(record.revision_id?)
        .filter(|revision| *revision > 0)
        .and_then(|revision| u64::try_from(revision).ok())
}

pub(super) fn historical_pattern_identity_axes_for_selection(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    selected: Option<(AsmHistoricalEntityKind, i64, &[i64])>,
    history: &AsmHistory,
) -> Result<Vec<crate::records::feature::patterns::DesignAxis>, cadmpeg_core::CodecError> {
    let Some((kind, entity_ref, state_ids)) = selected else {
        return Ok(Vec::new());
    };
    if state_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut axes = Vec::new();
    let mut matched_state_ids = HashSet::new();
    for state in decode.admit_iter(&history.states, "scan F3D pattern identity states")? {
        if !decode.contains(
            state_ids,
            &state.state_id,
            "find F3D pattern identity state",
        )? {
            continue;
        }
        decode.insert_hash_set(
            &mut matched_state_ids,
            state.state_id,
            "index F3D pattern axis states",
        )?;
        let Some(topology) = state.topology() else {
            return Ok(Vec::new());
        };
        let axis_candidates = historical_pattern_identity_axis_candidates(
            decode,
            Some((kind, entity_ref)),
            topology,
        )?;
        let state_axes = decode.collect_vec(
            decode
                .admit_iter(&axis_candidates, "scan F3D pattern axis candidates")?
                .copied()
                .filter_map(|(origin, direction)| design_axis(origin, direction)),
            "collect F3D pattern state axes",
        )?;
        if state_axes.is_empty() {
            return Ok(Vec::new());
        }
        for axis in decode.admit_iter(&state_axes, "scan F3D pattern state axes")? {
            decode.reserve_vec(&mut axes, 1, "collect F3D pattern axes")?;
            axes.push(*axis);
        }
    }
    if matched_state_ids.len() != state_ids.len() || axes.is_empty() {
        return Ok(Vec::new());
    }
    Ok(axes)
}

fn historical_pattern_identity_axis_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    selected: Option<(AsmHistoricalEntityKind, i64)>,
    topology: &AsmHistoricalTopology,
) -> Result<Vec<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)>, cadmpeg_core::CodecError> {
    let Some((kind, entity_ref)) = selected else {
        return Ok(Vec::new());
    };
    match kind {
        AsmHistoricalEntityKind::Face => {
            match historical_face_surface_axis(decode, entity_ref, topology)? {
                Some(axis) => decode.collect_vec(Some(axis), "collect F3D face axis candidates"),
                None => Ok(Vec::new()),
            }
        }
        AsmHistoricalEntityKind::Surface => {
            match historical_surface_axis(decode, entity_ref, topology)? {
                Some(axis) => decode.collect_vec(Some(axis), "collect F3D surface axis candidates"),
                None => Ok(Vec::new()),
            }
        }
        _ => {
            let mut axes = Vec::new();
            let mut identity_edges_storage =
                decode.reserve_scoped(0, "collect F3D identity edges")?;
            let identity_edges = identity_edges_storage
                .with_storage(|| historical_identity_edges(decode, kind, entity_ref, topology))?;
            for edge in decode.admit_iter(&identity_edges, "scan F3D edge axis candidates")? {
                let Some(axis) = historical_edge_axis(decode, *edge, topology)? else {
                    continue;
                };

                decode.reserve_vec(&mut axes, 1, "collect F3D edge axis candidates")?;
                axes.push(axis);
            }
            Ok(axes)
        }
    }
}

fn historical_face_surface_axis(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    face: i64,
    topology: &AsmHistoricalTopology,
) -> Result<Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)>, cadmpeg_core::CodecError>
{
    let Some(index) = decode.position_by(
        &topology.face_surfaces,
        |binding| Ok(binding.entity == face),
        "find F3D pattern face surface binding",
    )?
    else {
        return Ok(None);
    };
    let Some((binding, remaining)) = topology
        .face_surfaces
        .get(index..)
        .and_then(|bindings| bindings.split_first())
    else {
        return Ok(None);
    };
    if decode.any_by(
        remaining,
        |candidate| Ok(candidate.entity == face),
        "find F3D pattern face surface binding",
    )? {
        return Ok(None);
    }
    historical_surface_axis(decode, binding.carrier, topology)
}

fn historical_surface_axis(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    surface: i64,
    topology: &AsmHistoricalTopology,
) -> Result<Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)>, cadmpeg_core::CodecError>
{
    let axis_index = decode.position_by(
        &topology.surface_axes,
        |axis| Ok(axis.surface == surface),
        "find F3D pattern surface axis",
    )?;
    let axis = if let Some(index) = axis_index {
        let Some((axis, remaining)) = topology
            .surface_axes
            .get(index..)
            .and_then(|axes| axes.split_first())
        else {
            return Ok(None);
        };
        if decode.any_by(
            remaining,
            |candidate| Ok(candidate.surface == surface),
            "find F3D pattern surface axis",
        )? {
            return Ok(None);
        }
        Some((axis.origin, axis.direction))
    } else {
        None
    };
    let Some(plane_index) = decode.position_by(
        &topology.surface_planes,
        |plane| Ok(plane.surface == surface),
        "find F3D pattern surface plane",
    )?
    else {
        return Ok(axis);
    };
    let Some((plane, remaining)) = topology
        .surface_planes
        .get(plane_index..)
        .and_then(|planes| planes.split_first())
    else {
        return Ok(axis);
    };
    if axis.is_some()
        || decode.any_by(
            remaining,
            |candidate| Ok(candidate.surface == surface),
            "find F3D pattern surface plane",
        )?
    {
        return Ok(None);
    }
    Ok(Some((plane.origin, plane.normal)))
}

/// Bind persistent Mirror plane selections to exact planes in the selected
/// historical topology.
pub(crate) fn bind_mirror_selection_planes(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    scopes: &mut [crate::records::feature::scope::DesignParameterScope],
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    face_operands: &[crate::records::topology::face::DesignFaceOperand],
    identities: &[crate::records::topology::construction::DesignConstructionOperandIdentity],
    histories: &[AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    for scope in scopes {
        let stream = crate::ids::native_stream(&scope.id).map(str::to_owned);
        let record_index = scope.record_index;
        let history_state_id = scope.history_state_id();
        let previous_history_state_id = scope.previous_history_state_id();
        let Some(construction) = scope.mirror_construction_mut() else {
            continue;
        };
        construction.plane = None;
        let (Some(selection_record_index), Some(state_id), Some(previous_state_id)) = (
            construction.plane_selection_record_index,
            history_state_id,
            previous_history_state_id,
        ) else {
            continue;
        };
        let Some((history, _, _)) =
            unique_history_state_pair(decode, histories, state_id, previous_state_id)?
        else {
            continue;
        };
        let matches_group =
            |group: &crate::records::topology::construction::DesignConstructionOperandGroup| {
                if !decode.equal(
                    &crate::ids::native_stream(&group.id),
                    &stream.as_deref(),
                    "compare F3D Mirror plane group stream",
                )? {
                    return Ok(false);
                }
                Ok(group.scope_record_index == record_index
                    && group.record_index == construction.plane_group_record_index
                    && group.role() == DesignOperandRole::ROLE_0X5
                    && group
                        .members()
                        .iter()
                        .map(|member| member.value)
                        .eq([selection_record_index]))
            };
        let Some(group_index) =
            decode.position_by(groups, &matches_group, "find F3D Mirror plane group")?
        else {
            continue;
        };
        if decode
            .position_by(
                &groups[group_index + 1..],
                matches_group,
                "find F3D Mirror plane group",
            )?
            .is_some()
        {
            continue;
        }
        let group = &groups[group_index];
        let matches_operand =
            |operand: &crate::records::topology::entity_selection::DesignEntitySelectionOperand| {
                if !decode.equal(
                    &crate::ids::native_stream(&operand.id),
                    &stream.as_deref(),
                    "compare F3D Mirror plane operand stream",
                )? {
                    return Ok(false);
                }
                Ok(operand.scope_record_index == record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == 0
                    && operand.record_index() == selection_record_index)
            };
        let matching_operand = if let Some(index) =
            decode.position_by(operands, &matches_operand, "find F3D Mirror plane operand")?
        {
            if decode
                .position_by(
                    &operands[index + 1..],
                    matches_operand,
                    "find F3D Mirror plane operand",
                )?
                .is_some()
            {
                continue;
            }
            Some(&operands[index])
        } else {
            None
        };
        let matches_face_operand = |operand: &crate::records::topology::face::DesignFaceOperand| {
            if !decode.equal(
                &crate::ids::native_stream(&operand.id),
                &stream.as_deref(),
                "compare F3D Mirror plane face-operand stream",
            )? {
                return Ok(false);
            }
            Ok(operand.scope_record_index == record_index
                && operand.group_record_index() == Some(group.record_index)
                && operand.group_member_ordinal() == Some(0)
                && operand.record_index() == selection_record_index)
        };
        let matching_face_operand = if let Some(index) = decode.position_by(
            face_operands,
            &matches_face_operand,
            "find F3D Mirror plane face operand",
        )? {
            if decode
                .position_by(
                    &face_operands[index + 1..],
                    matches_face_operand,
                    "find F3D Mirror plane face operand",
                )?
                .is_some()
            {
                continue;
            }
            Some(&face_operands[index])
        } else {
            None
        };
        let plane = if let Some(operand) = matching_operand {
            if matching_face_operand.is_some() {
                continue;
            }
            let matches_identity = |identity: &crate::records::topology::construction::DesignConstructionOperandIdentity| {
                if !decode.equal(
                    &crate::ids::native_stream(&identity.id),
                    &stream.as_deref(),
                    "compare F3D Mirror plane identity stream",
                )? {
                    return Ok(false);
                }
                Ok(identity.group_record_index == group.record_index)
            };
            let identity = if let Some(index) = decode.position_by(
                identities,
                &matches_identity,
                "find F3D Mirror plane identity",
            )? {
                if decode
                    .position_by(
                        &identities[index + 1..],
                        matches_identity,
                        "find F3D Mirror plane identity",
                    )?
                    .is_some()
                {
                    continue;
                }
                Some(&identities[index])
            } else {
                None
            };
            let persistent_candidates = if let Some(identity) = identity
                .and_then(crate::records::topology::construction::DesignConstructionOperandIdentity::persistent_identity) {
                entity_selection_face_candidates(decode, identity.local_id, histories)?
            } else {
                Vec::new()
            };
            let primary_candidates =
                entity_selection_face_candidates(decode, operand.primary_identity, histories)?;
            if primary_candidates.is_empty() && persistent_candidates.is_empty() {
                design_geometry_mirror_plane(operand.primary_identity)
            } else {
                let Some(candidate) = unique_mirror_plane_candidate(
                    decode,
                    primary_candidates,
                    persistent_candidates,
                )?
                else {
                    continue;
                };
                historical_mirror_plane(decode, &candidate, previous_state_id, histories)?
            }
        } else if let Some(operand) = matching_face_operand {
            historical_mirror_face_operand_plane(decode, operand, history, previous_state_id)?
        } else {
            continue;
        };
        let Some(plane) = plane else { continue };
        let norm = (plane.normal.x * plane.normal.x
            + plane.normal.y * plane.normal.y
            + plane.normal.z * plane.normal.z)
            .sqrt();
        if !norm.is_finite() || (norm - 1.0).abs() > EPS_HISTORY_BIND_MIRROR_SELECTION_PLANES_E9 {
            continue;
        }
        let (Some(origin), Some(normal)) = (
            cadmpeg_ir::features::FinitePoint3::new(plane.origin),
            cadmpeg_ir::features::FiniteVector3::new(plane.normal),
        ) else {
            continue;
        };
        construction.plane =
            crate::records::feature::patterns::DesignPlane::from_parts(origin, normal);
    }
    Ok(())
}

pub(super) fn historical_mirror_face_operand_plane(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operand: &crate::records::topology::face::DesignFaceOperand,
    history: &AsmHistory,
    previous_state_id: i64,
) -> Result<Option<HistoricalMirrorPlane>, cadmpeg_core::CodecError> {
    if !operand.resolved_face_slots.is_empty() {
        return coincident_mirror_plane(
            decode,
            &operand.resolved_face_slots,
            previous_state_id,
            history,
        );
    }
    if !operand.preceding_candidate_faces.is_empty() {
        return coincident_mirror_face_candidates(
            decode,
            &operand.preceding_candidate_faces,
            previous_state_id,
            history,
            "scan F3D preceding mirror face candidates",
        );
    }
    if operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::Face {
        let mut has_references = false;
        let mut minimum_slot = None;
        for reference in decode.admit_iter(
            &operand.recipe_references,
            "scan F3D mirror recipe references",
        )? {
            has_references |= include_mirror_face_slots(
                decode,
                &reference.candidate_faces,
                &mut minimum_slot,
                "scan F3D mirror recipe candidate faces",
            )?;
            has_references |= include_mirror_face_slots(
                decode,
                &reference.alternate_selector_faces,
                &mut minimum_slot,
                "scan F3D mirror recipe alternate selector faces",
            )?;
        }
        if has_references {
            let Some(first_slot) = minimum_slot else {
                return Ok(None);
            };
            let Some(first) =
                historical_mirror_plane_for_face_slot(first_slot, previous_state_id, history)
            else {
                return Ok(None);
            };
            for reference in decode.admit_iter(
                &operand.recipe_references,
                "scan F3D mirror recipe references",
            )? {
                if !mirror_face_candidates_match_plane(
                    decode,
                    &reference.candidate_faces,
                    &first,
                    previous_state_id,
                    history,
                    "scan F3D mirror recipe candidate faces",
                )? || !mirror_face_candidates_match_plane(
                    decode,
                    &reference.alternate_selector_faces,
                    &first,
                    previous_state_id,
                    history,
                    "scan F3D mirror recipe alternate selector faces",
                )? {
                    return Ok(None);
                }
            }
            return Ok(Some(first));
        }
    }
    coincident_mirror_face_candidates(
        decode,
        crate::design::face_resolve::face_operand_candidates(operand),
        previous_state_id,
        history,
        "scan F3D fallback mirror face candidates",
    )
}

fn coincident_mirror_face_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    faces: &[cadmpeg_ir::ids::FaceId],
    previous_state_id: i64,
    history: &AsmHistory,
    scan_operation: &'static str,
) -> Result<Option<HistoricalMirrorPlane>, cadmpeg_core::CodecError> {
    let mut minimum_slot = None;
    include_mirror_face_slots(decode, faces, &mut minimum_slot, scan_operation)?;
    let Some(first_slot) = minimum_slot else {
        return Ok(None);
    };
    let Some(first) = historical_mirror_plane_for_face_slot(first_slot, previous_state_id, history)
    else {
        return Ok(None);
    };
    if !mirror_face_candidates_match_plane(
        decode,
        faces,
        &first,
        previous_state_id,
        history,
        scan_operation,
    )? {
        return Ok(None);
    }
    Ok(Some(first))
}

fn include_mirror_face_slots(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    faces: &[cadmpeg_ir::ids::FaceId],
    minimum_slot: &mut Option<i64>,
    scan_operation: &'static str,
) -> Result<bool, cadmpeg_core::CodecError> {
    let (has_faces, minimum) = decode.fold(
        faces,
        (false, *minimum_slot),
        |(_, minimum), face| {
            let minimum = match stable_ref(decode, face.as_str())? {
                Some(slot) => Some(minimum.map_or(slot, |current| current.min(slot))),
                None => minimum,
            };
            Ok((true, minimum))
        },
        scan_operation,
    )?;
    *minimum_slot = minimum;
    Ok(has_faces)
}

fn mirror_face_candidates_match_plane(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    faces: &[cadmpeg_ir::ids::FaceId],
    first: &HistoricalMirrorPlane,
    previous_state_id: i64,
    history: &AsmHistory,
    scan_operation: &'static str,
) -> Result<bool, cadmpeg_core::CodecError> {
    for face in decode.admit_iter(faces, scan_operation)? {
        let Some(slot) = stable_ref(decode, face.as_str())? else {
            continue;
        };
        let Some(candidate) =
            historical_mirror_plane_for_face_slot(slot, previous_state_id, history)
        else {
            return Ok(false);
        };
        if !mirror_planes_coincident(first, &candidate) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn coincident_mirror_plane(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    slots: &[i64],
    previous_state_id: i64,
    history: &AsmHistory,
) -> Result<Option<HistoricalMirrorPlane>, cadmpeg_core::CodecError> {
    let Some(first_slot) = decode
        .min(slots, "find F3D lowest mirror face slot")?
        .copied()
    else {
        return Ok(None);
    };
    let Some(first) = historical_mirror_plane_for_face_slot(first_slot, previous_state_id, history)
    else {
        return Ok(None);
    };
    for slot in decode.admit_iter(slots, "scan F3D coincident mirror face slots")? {
        let Some(candidate) =
            historical_mirror_plane_for_face_slot(*slot, previous_state_id, history)
        else {
            return Ok(None);
        };
        if !mirror_planes_coincident(&first, &candidate) {
            return Ok(None);
        }
    }
    Ok(Some(first))
}

pub(super) fn unique_mirror_plane_candidate(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    mut primary: Vec<
        crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate,
    >,
    mut persistent: Vec<
        crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate,
    >,
) -> Result<
    Option<crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate>,
    cadmpeg_core::CodecError,
> {
    decode.stable_sort_by(
        &mut primary,
        |value| &value.history_id,
        Ord::cmp,
        "f3d mirror plane primary candidates sort",
    )?;
    decode.dedup_vec(&mut primary, "deduplicate F3D primary mirror candidates")?;
    decode.retain_vec(
        &mut persistent,
        |candidate| {
            decode.any_by(
                &primary,
                |context| {
                    decode.equal(
                        context.history_id.as_str(),
                        candidate.history_id.as_str(),
                        "compare F3D persistent mirror candidate history IDs",
                    )
                },
                "find F3D primary mirror candidate history",
            )
        },
        "retain F3D persistent mirror candidates",
    )?;
    decode.stable_sort_by(
        &mut persistent,
        |value| &value.history_id,
        Ord::cmp,
        "f3d mirror plane persistent candidates sort",
    )?;
    decode.dedup_vec(
        &mut persistent,
        "deduplicate F3D persistent mirror candidates",
    )?;
    Ok(match persistent.len() {
        1 => persistent.pop(),
        0 if primary.len() == 1 => primary.pop(),
        _ => None,
    })
}

#[derive(Clone)]
pub(super) struct HistoricalMirrorPlane {
    pub(super) origin: cadmpeg_ir::math::Point3,
    pub(super) normal: cadmpeg_ir::math::Vector3,
}

/// Resolve the three built-in Design Geometry origin planes.
///
/// These identities are outside the ASM history namespace. A numeric identity
/// is admitted here only after both the primary and persistent history lookups
/// have found no candidate, so a topology identity with the same value keeps
/// the normal historical-resolution path.
pub(super) fn design_geometry_mirror_plane(primary_identity: u64) -> Option<HistoricalMirrorPlane> {
    let normal = match primary_identity {
        42 => cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        43 => cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        44 => cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        _ => return None,
    };
    Some(HistoricalMirrorPlane {
        origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        normal,
    })
}

pub(super) fn historical_mirror_plane(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    candidate: &crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate,
    preferred_state_id: i64,
    histories: &[AsmHistory],
) -> Result<Option<HistoricalMirrorPlane>, cadmpeg_core::CodecError> {
    if decode.contains(
        &candidate.historical.state_ids,
        &preferred_state_id,
        "check F3D preferred mirror candidate state",
    )? {
        return historical_mirror_plane_in_state(decode, candidate, preferred_state_id, histories);
    }
    let mut resolved = None;
    for state_id in decode.admit_iter(
        &candidate.historical.state_ids,
        "scan F3D candidate historical state ids",
    )? {
        let Some(plane) =
            historical_mirror_plane_in_state(decode, candidate, *state_id, histories)?
        else {
            return Ok(None);
        };
        if resolved
            .as_ref()
            .is_some_and(|exact: &HistoricalMirrorPlane| !mirror_planes_coincident(exact, &plane))
        {
            return Ok(None);
        }
        resolved = Some(plane);
    }
    Ok(resolved)
}

fn mirror_planes_coincident(left: &HistoricalMirrorPlane, right: &HistoricalMirrorPlane) -> bool {
    let dot = left.normal.dot(right.normal);
    let normal_distance = right.origin.vector_from(left.origin).dot(left.normal);
    (dot.abs() - 1.0).abs() <= EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E9
        && normal_distance.abs() <= EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E8
}

fn historical_mirror_plane_in_state(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    candidate: &crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate,
    state_id: i64,
    histories: &[AsmHistory],
) -> Result<Option<HistoricalMirrorPlane>, cadmpeg_core::CodecError> {
    let mut history_matches = |history: &AsmHistory| {
        decode.equal(
            history.id.as_str(),
            candidate.history_id.as_str(),
            "compare F3D mirror candidate history ID",
        )
    };
    let Some(history_index) = decode.position_by(
        histories,
        &mut history_matches,
        "find F3D mirror candidate history",
    )?
    else {
        return Ok(None);
    };
    if decode
        .position_by(
            &histories[history_index + 1..],
            &mut history_matches,
            "find duplicate F3D mirror candidate history",
        )?
        .is_some()
    {
        return Ok(None);
    }
    let history = &histories[history_index];
    let mut matching_states = history
        .states
        .iter()
        .filter(|state| state.state_id == state_id);
    let Some(state) = matching_states.next() else {
        return Ok(None);
    };
    if matching_states.next().is_some() {
        return Ok(None);
    }
    let Some(topology) = state.topology() else {
        return Ok(None);
    };
    match candidate.historical.kind {
        AsmHistoricalEntityKind::Coedge => {
            historical_mirror_coedge_plane(decode, candidate.historical.entity_ref, topology)
        }
        AsmHistoricalEntityKind::Loop => {
            historical_loop_plane(decode, candidate.historical.entity_ref, topology)
        }
        _ => Ok(historical_mirror_plane_for_face_slot_in_topology(
            candidate.face_slot,
            topology,
        )),
    }
}

fn historical_mirror_plane_for_face_slot(
    face_slot: i64,
    state_id: i64,
    history: &AsmHistory,
) -> Option<HistoricalMirrorPlane> {
    let mut matching_states = history
        .states
        .iter()
        .filter(|state| state.state_id == state_id);
    let state = matching_states.next()?;
    if matching_states.next().is_some() {
        return None;
    }
    let topology = state.topology()?;
    historical_mirror_plane_for_face_slot_in_topology(face_slot, topology)
}

fn historical_mirror_plane_for_face_slot_in_topology(
    face_slot: i64,
    topology: &AsmHistoricalTopology,
) -> Option<HistoricalMirrorPlane> {
    let mut bindings = topology
        .face_surfaces
        .iter()
        .filter(|binding| binding.entity == face_slot);
    let binding = bindings.next()?;
    if bindings.next().is_some() {
        return None;
    }
    let mut planes = topology
        .surface_planes
        .iter()
        .filter(|plane| plane.surface == binding.carrier);
    let plane = planes.next()?;
    planes.next().is_none().then_some(HistoricalMirrorPlane {
        origin: plane.origin,
        normal: plane.normal,
    })
}

macro_rules! mirror_some {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

pub(super) fn historical_loop_plane(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    loop_ref: i64,
    topology: &AsmHistoricalTopology,
) -> Result<Option<HistoricalMirrorPlane>, cadmpeg_core::CodecError> {
    let mut loop_relations = topology
        .loop_coedges
        .iter()
        .filter(|relation| relation.owner_ref == loop_ref);
    let relation = mirror_some!(loop_relations.next());
    if loop_relations.next().is_some() || relation.member_refs.is_empty() {
        return Ok(None);
    }
    let mut planes = Vec::new();
    for coedge_ref in decode.admit_iter(&relation.member_refs, "scan F3D relation member refs")? {
        let mut coedges = topology
            .coedge_topology
            .iter()
            .filter(|coedge| coedge.coedge == *coedge_ref && coedge.owner_loop == loop_ref);
        let coedge = mirror_some!(coedges.next());
        if coedges.next().is_some() {
            return Ok(None);
        }
        let mut bindings = topology
            .edge_curves
            .iter()
            .filter(|binding| binding.entity == coedge.edge);
        let binding = mirror_some!(bindings.next());
        if bindings.next().is_some() {
            return Ok(None);
        }
        let curve = mirror_some!(binding.carrier);
        let mut axes = topology
            .curve_axes
            .iter()
            .filter(|axis| axis.curve == curve);
        let axis = mirror_some!(axes.next());
        if axes.next().is_some() {
            return Ok(None);
        }
        let norm = (axis.direction.x * axis.direction.x
            + axis.direction.y * axis.direction.y
            + axis.direction.z * axis.direction.z)
            .sqrt();
        if !norm.is_finite() || (norm - 1.0).abs() > EPS_HISTORY_HISTORICAL_LOOP_PLANE_E9 {
            return Ok(None);
        }

        decode.reserve_vec(&mut planes, 1, "collect F3D loop mirror planes")?;
        planes.push(HistoricalMirrorPlane {
            origin: axis.origin,
            normal: axis.direction,
        });
    }
    let [first, remaining @ ..] = planes.as_slice() else {
        return Ok(None);
    };
    let planes_coincide = decode.all_by(
        remaining,
        |candidate| Ok(mirror_planes_coincident(first, candidate)),
        "compare F3D loop mirror planes",
    )?;
    Ok(planes_coincide.then_some(first.clone()))
}

/// Resolve a Mirror plane from a selected coedge's complete radial cycle.
///
/// A coedge selects a boundary occurrence, not a face. Its owning face can be
/// non-planar while another face incident to the same topological edge is the
/// selected plane. The cycle and relation checks keep that inference exact:
/// every coedge for the edge must participate in one closed radial cycle, each
/// cycle member must belong to one loop and one face, and the incident faces
/// must expose one coincident set of exact plane carriers.
pub(super) fn historical_mirror_coedge_plane(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    coedge_ref: i64,
    topology: &AsmHistoricalTopology,
) -> Result<Option<HistoricalMirrorPlane>, cadmpeg_core::CodecError> {
    let mut coedges_storage = decode.reserve_scoped(0, "index F3D mirror coedges")?;
    let mut coedges = HashMap::new();
    for coedge in decode.admit_iter(
        &topology.coedge_topology,
        "scan F3D topology coedge topology",
    )? {
        if coedges_storage
            .with_storage(|| {
                decode.insert_hash_map(
                    &mut coedges,
                    coedge.coedge,
                    coedge,
                    "index F3D mirror coedges",
                )
            })?
            .is_some()
        {
            return Ok(None);
        }
    }
    let selected = mirror_some!(coedges.get(&coedge_ref));
    let edge = selected.edge;
    // Coedge references are unique here, so this counts the distinct
    // coedges of the selected edge.
    let same_edge_count = decode
        .admit_iter(&topology.coedge_topology, "scan F3D mirror edge coedges")?
        .filter(|coedge| coedge.edge == edge)
        .count();
    let radial_cycle_operation = "follow F3D mirror radial cycle";
    let radial_cycle_bound = coedges
        .len()
        .checked_add(1)
        .ok_or_else(|| decode.refuse_codec_limit(radial_cycle_operation, u64::MAX - 1, u64::MAX))?;
    let radial_cycle_work = u64::try_from(radial_cycle_bound)
        .map_err(|_| decode.refuse_codec_limit(radial_cycle_operation, u64::MAX - 1, u64::MAX))?;
    decode.charge_work(radial_cycle_work, radial_cycle_operation)?;
    let mut radial_cycle_storage = decode.reserve_scoped(0, "collect F3D mirror radial cycle")?;
    let mut radial_cycle = Vec::new();
    let mut current = coedge_ref;
    loop {
        if current == coedge_ref && !radial_cycle.is_empty() {
            break;
        }
        if decode.contains(
            &radial_cycle,
            &current,
            "check F3D repeated mirror radial coedge",
        )? {
            return Ok(None);
        }
        let coedge = mirror_some!(coedges.get(&current));
        if coedge.edge != edge {
            return Ok(None);
        }

        radial_cycle_storage.with_storage(|| {
            decode.reserve_vec(&mut radial_cycle, 1, "collect F3D mirror radial cycle")
        })?;
        radial_cycle.push(current);
        current = coedge.radial_next;
    }
    // The cycle holds distinct coedges of the selected edge, so it covers
    // every coedge of that edge exactly when the counts agree.
    if radial_cycle.len() != same_edge_count {
        return Ok(None);
    }

    let mut faces_storage = decode.reserve_scoped(0, "index F3D mirror incident faces")?;
    let mut faces = BTreeSet::new();
    for &coedge_ref in decode.admit_iter(&radial_cycle, "scan F3D mirror radial owners")? {
        let coedge = mirror_some!(coedges.get(&coedge_ref));
        let mut loop_relation = None;
        for relation in
            decode.admit_iter(&topology.loop_coedges, "scan F3D mirror loop relations")?
        {
            if relation.owner_ref == coedge.owner_loop
                && decode.contains(
                    &relation.member_refs,
                    &coedge_ref,
                    "find F3D mirror loop member",
                )?
                && loop_relation.replace(relation).is_some()
            {
                return Ok(None);
            }
        }
        let Some(loop_relation) = loop_relation else {
            return Ok(None);
        };
        let loop_member_count = decode
            .admit_iter(
                &loop_relation.member_refs,
                "count F3D mirror loop coedge references",
            )?
            .filter(|member| **member == coedge_ref)
            .count();
        if loop_member_count != 1 {
            return Ok(None);
        }
        let mut face_relation = None;
        for relation in decode.admit_iter(&topology.face_loops, "scan F3D mirror face relations")? {
            if decode.contains(
                &relation.member_refs,
                &coedge.owner_loop,
                "find F3D mirror face loop member",
            )? && face_relation.replace(relation).is_some()
            {
                return Ok(None);
            }
        }
        let Some(face_relation) = face_relation else {
            return Ok(None);
        };
        let face_loop_member_count = decode
            .admit_iter(
                &face_relation.member_refs,
                "count F3D mirror face loop references",
            )?
            .filter(|member| **member == coedge.owner_loop)
            .count();
        if face_loop_member_count != 1 {
            return Ok(None);
        }
        decode.insert_scoped_btree_set(
            &mut faces_storage,
            &mut faces,
            face_relation.owner_ref,
            "index F3D mirror incident faces",
            "index F3D mirror incident faces",
        )?;
    }

    let mut planes_storage = decode.reserve_scoped(0, "collect F3D coedge mirror planes")?;
    let mut planes = Vec::new();
    for &face in decode.admit_iter(&faces, "scan F3D mirror incident faces")? {
        let mut surface_bindings = topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == face);
        let surface = mirror_some!(surface_bindings.next());
        if surface_bindings.next().is_some() {
            return Ok(None);
        }
        let mut surface_planes = topology
            .surface_planes
            .iter()
            .filter(|plane| plane.surface == surface.carrier);
        if let Some(plane) = surface_planes.next() {
            if surface_planes.next().is_some() {
                return Ok(None);
            }

            planes_storage.with_storage(|| {
                decode.reserve_vec(&mut planes, 1, "collect F3D coedge mirror planes")
            })?;
            planes.push(HistoricalMirrorPlane {
                origin: plane.origin,
                normal: plane.normal,
            });
        }
    }
    let [first, remaining @ ..] = planes.as_slice() else {
        return Ok(None);
    };
    let planes_coincide = decode.all_by(
        remaining,
        |candidate| Ok(mirror_planes_coincident(first, candidate)),
        "compare F3D coedge mirror planes",
    )?;
    Ok(planes_coincide.then_some(first.clone()))
}

pub(super) fn entity_selection_face_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    local_id: u64,
    histories: &[AsmHistory],
) -> Result<
    Vec<crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate>,
    cadmpeg_core::CodecError,
> {
    use crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate;

    let mut candidates = Vec::new();
    'histories: for history in
        decode.admit_iter(histories, "scan F3D entity selection histories")?
    {
        let identities = HistoricalIdentityIndex::build(
            decode,
            std::slice::from_ref(history),
            decode.admit_iter(
                std::slice::from_ref(&local_id),
                "scan F3D identity local IDs",
            )?,
            |local_id| std::iter::once(*local_id).chain(None),
        )?;
        let Some((kind, entity_ref, state_ids)) =
            identities.selection_identity_kind(decode, local_id)?
        else {
            continue;
        };
        let mut face_slot = None;
        for state_id in &state_ids {
            let mut states = history
                .states
                .iter()
                .filter(|state| state.state_id == *state_id);
            let Some(state) = states.next() else {
                continue 'histories;
            };
            if states.next().is_some() {
                continue 'histories;
            }
            let Some(topology) = state.topology() else {
                continue 'histories;
            };
            let mut faces_storage = decode.reserve_scoped(0, "collect F3D identity faces")?;
            let faces = faces_storage
                .with_storage(|| historical_identity_faces(decode, kind, entity_ref, topology))?;
            let Some(state_face) =
                single_btree_value(decode, Some(faces), "take F3D identity face")?
            else {
                continue 'histories;
            };
            if face_slot.is_some_and(|face| face != state_face) {
                continue 'histories;
            }
            face_slot = Some(state_face);
        }
        let Some(face_slot) = face_slot else {
            continue;
        };
        let history_id = {
            let ctx = decode;
            ctx.copy_retained_text(&history.id, "copy F3D selection history ID")?
        };

        decode.reserve_vec(&mut candidates, 1, "collect F3D selection face candidates")?;
        candidates.push(DesignEntitySelectionFaceCandidate {
            history_id,
            historical: crate::records::topology::fillet::HistoricalBinding {
                kind,
                entity_ref,
                state_ids,
            },
            face_slot,
        });
    }
    Ok(candidates)
}

pub(super) fn entity_selection_edge_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    identities: impl IntoIterator<Item = (u32, u64)>,
    previous_state_id: i64,
    history_identities: &HistoricalIdentityIndex,
    topology: &AsmHistoricalTopology,
) -> Result<
    Vec<crate::records::topology::entity_selection::DesignEntitySelectionEdgeCandidate>,
    cadmpeg_core::CodecError,
> {
    use crate::records::topology::entity_selection::DesignEntitySelectionEdgeCandidate;

    let mut candidates = Vec::new();
    for (identity_ordinal, local_id) in identities {
        let Some((kind, entity_ref, states)) =
            history_identities.selection_identity_kind(decode, local_id)?
        else {
            continue;
        };
        if !decode.contains(
            &states,
            &previous_state_id,
            "find F3D selected identity state",
        )? {
            continue;
        }
        let mut identity_edges_storage = decode.reserve_scoped(0, "collect F3D identity edges")?;
        let identity_edges = identity_edges_storage
            .with_storage(|| historical_identity_edges(decode, kind, entity_ref, topology))?;
        // The ordered set yields the edge slots already sorted.
        let mut edge_slots = Vec::new();
        for edge in decode.admit_iter(
            &identity_edges,
            "scan F3D selected identity edge candidates",
        )? {
            decode.push_vec(
                &mut edge_slots,
                *edge,
                "collect F3D selected identity edges",
            )?;
        }
        if edge_slots.is_empty() {
            continue;
        }

        decode.reserve_vec(&mut candidates, 1, "collect F3D selection edge candidates")?;
        candidates.push(DesignEntitySelectionEdgeCandidate {
            identity_ordinal,
            local_id,
            historical_entity_kind: kind,
            historical_entity_ref: entity_ref,
            edge_slots,
        });
    }
    Ok(candidates)
}

pub(super) fn unique_entity_selection_edge(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &[crate::records::topology::entity_selection::DesignEntitySelectionEdgeCandidate],
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(first) = candidates.first() else {
        return Ok(None);
    };
    let mut unique = None;
    for edge in decode.admit_iter(&first.edge_slots, "scan F3D candidate edge slots")? {
        let mut matches_all_candidates = true;
        for candidate in decode.admit_iter(&candidates[1..], "scan F3D edge candidates")? {
            if !decode.contains(
                &candidate.edge_slots,
                edge,
                "find F3D common candidate edge",
            )? {
                matches_all_candidates = false;
                break;
            }
        }
        if matches_all_candidates {
            if unique.is_some_and(|prior| prior != *edge) {
                return Ok(None);
            }
            unique = Some(*edge);
        }
    }
    Ok(unique)
}

pub(crate) fn bind_edge_identity_history(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [DesignEdgeIdentityOperand],
    identities: &[crate::records::topology::construction::DesignConstructionOperandIdentity],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    struct EdgeTreatmentTransitionCandidates {
        radii: Vec<crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate>,
        treatment_edges: Vec<i64>,
        deleted_edges: Vec<i64>,
    }

    if projection_was_finalized(decode, histories)? {
        return Ok(());
    }
    let mut compact_group_counts_storage =
        decode.reserve_scoped(0, "index F3D compact edge groups")?;
    let mut compact_group_counts = HashMap::<(String, u32, u32), Option<usize>>::new();
    for operand in decode.admit_iter(&*operands, "scan F3D operands")? {
        let Some(stream) = crate::ids::native_stream(&operand.id) else {
            continue;
        };
        compact_group_counts_storage.with_storage(|| {
            let stream = decode.copy_retained_text(stream, "copy F3D compact edge group stream")?;
            let key = (
                stream,
                operand.scope_record_index,
                operand.group_record_index,
            );
            match decode.entry_hash_map(
                &mut compact_group_counts,
                key,
                "index F3D compact edge groups",
            )? {
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    let count = entry.get_mut();
                    *count = match *count {
                        Some(count) if operand.layout().is_compact() => {
                            Some(count.checked_add(1).ok_or_else(|| {
                                decode.refuse_codec_limit(
                                    "count F3D compact edge group members",
                                    u64::MAX - 1,
                                    u64::MAX,
                                )
                            })?)
                        }
                        Some(_) => None,
                        None => None,
                    };
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(operand.layout().is_compact().then_some(1));
                }
            }
            Ok::<(), cadmpeg_core::CodecError>(())
        })?;
    }
    let local_ids = decode.collect_vec(
        operands
            .iter()
            .map(|operand| operand.local_id)
            .chain(identities.iter().filter_map(|identity| {
                identity
                    .persistent_identity()
                    .map(|persistent| persistent.local_id)
            })),
        "collect F3D edge identity local ids",
    )?;
    let history_identities = HistoricalIdentityIndex::build(
        decode,
        histories,
        decode.admit_iter(&local_ids, "scan F3D identity local IDs")?,
        |local_id| std::iter::once(*local_id).chain(None),
    )?;
    let mut identities_by_history_storage =
        decode.reserve_scoped(0, "index F3D scoped history identities")?;
    let mut identities_by_history = HashMap::new();
    for history in decode.admit_iter(histories, "scan F3D edge identity histories")? {
        identities_by_history_storage.with_storage(|| {
            let index = HistoricalIdentityIndex::build(
                decode,
                std::slice::from_ref(history),
                decode.admit_iter(&local_ids, "scan F3D identity local IDs")?,
                |local_id| std::iter::once(*local_id).chain(None),
            )?;

            decode
                .insert_hash_map(
                    &mut identities_by_history,
                    history.id.as_str(),
                    index,
                    "index F3D scoped history identities",
                )
                .map(|_| ())
        })?;
    }
    let mut treatment_candidates_by_transition_storage =
        decode.reserve_scoped(0, "index F3D treatment transitions")?;
    let mut treatment_candidates_by_transition =
        HashMap::<(String, i64, i64), EdgeTreatmentTransitionCandidates>::new();
    for operand in operands {
        operand.historical = None;
        decode.clear_vec(
            &mut operand.treatment_radius_candidates,
            "clear F3D edge treatment radius candidates",
        )?;
        operand.transition_edge_candidates.clear();
        operand.resolved_edge_slots.clear();
        operand.resolved_edge_slot = None;
        operand.resolution_identity_id = None;
        let Some(stream) = crate::ids::native_stream(&operand.id) else {
            continue;
        };
        let mut matching_scopes = scopes.iter().filter(|scope| {
            crate::ids::native_stream(&scope.id) == Some(stream)
                && scope.record_index == operand.scope_record_index
        });
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let Some(previous_state_id) = scope.previous_history_state_id() else {
            continue;
        };
        let current_state_id = scope.history_state_id();
        let bound_history = bound_scope_history(decode, &scope.id, scope_histories, histories)?;
        let scoped_identities = if let Some(history) = bound_history {
            match decode.get_hash_map(
                &identities_by_history,
                history.id.as_str(),
                "find F3D scoped history identities",
            )? {
                Some(identities) => identities,
                None => &history_identities,
            }
        } else {
            &history_identities
        };
        if let Some((kind, entity_ref, states)) =
            scoped_identities.selection_identity_kind(decode, operand.local_id)?
        {
            if decode.contains(&states, &previous_state_id, "find F3D edge identity state")? {
                operand.historical = Some(crate::records::topology::fillet::HistoricalBinding {
                    kind,
                    entity_ref,
                    state_ids: states,
                });
            }
        }
        let Some(history) = bound_history else {
            continue;
        };
        let mut previous_states = history
            .states
            .iter()
            .filter(|state| state.state_id == previous_state_id);
        let Some(previous_state) = previous_states.next() else {
            continue;
        };
        if previous_states.next().is_some() {
            continue;
        }
        let Some(topology) = previous_state.topology() else {
            continue;
        };
        if let Some(current_state_id) = current_state_id {
            let mut current_states = history
                .states
                .iter()
                .filter(|state| state.state_id == current_state_id);
            let current_state = current_states.next();
            let current_state_is_unique = current_states.next().is_none();
            let current_reaches_previous = if current_state_is_unique {
                match current_state {
                    Some(state) => {
                        history_state_reaches(decode, history, state, previous_state_id)?
                    }
                    None => false,
                }
            } else {
                false
            };
            if current_reaches_previous {
                let lookup_key = decode.with_scoped_storage(
                    "lookup F3D treatment transition candidates",
                    || {
                        Ok::<_, cadmpeg_core::CodecError>((
                            decode.copy_retained_text(
                                &history.id,
                                "copy F3D treatment transition history id",
                            )?,
                            current_state_id,
                            previous_state_id,
                        ))
                    },
                )?;
                let _key_storage = lookup_key.1;
                let key = lookup_key.0;
                if !decode.contains_key_hash_map(
                    &treatment_candidates_by_transition,
                    &key,
                    "check F3D treatment transition candidates",
                )? {
                    if let Some(result) = current_state.and_then(|state| state.topology()) {
                        treatment_candidates_by_transition_storage.with_storage(|| {
                            let mut preceding_faces_storage =
                                decode.reserve_scoped(0, "index F3D treatment preceding faces")?;
                            let mut preceding_faces = HashSet::new();
                            for face in
                                decode.admit_iter(&topology.faces, "scan F3D topology faces")?
                            {
                                preceding_faces_storage.with_storage(|| {
                                    decode.insert_hash_set(
                                        &mut preceding_faces,
                                        *face,
                                        "index F3D treatment preceding faces",
                                    )
                                })?;
                            }
                            let mut inserted_faces_storage =
                                decode.reserve_scoped(0, "collect F3D treatment inserted faces")?;
                            let mut inserted_faces = Vec::new();
                            for face in decode.admit_iter(
                                result.faces.as_slice(),
                                "scan F3D inserted treatment faces",
                            )? {
                                if !decode.contains_hash_set(
                                    &preceding_faces,
                                    face,
                                    "check F3D preceding treatment face membership",
                                )? {
                                    inserted_faces_storage.with_storage(|| {
                                        decode.push_vec(
                                            &mut inserted_faces,
                                            *face,
                                            "collect F3D treatment inserted faces",
                                        )
                                    })?;
                                }
                            }
                            let mut result_edges_storage =
                                decode.reserve_scoped(0, "index F3D treatment result edges")?;
                            let mut result_edges = HashSet::new();
                            for edge in decode.admit_iter(&result.edges, "scan F3D result edges")? {
                                result_edges_storage.with_storage(|| {
                                    decode.insert_hash_set(
                                        &mut result_edges,
                                        *edge,
                                        "index F3D treatment result edges",
                                    )
                                })?;
                            }
                            let mut deleted_edges = Vec::new();
                            for edge in decode.admit_iter(
                                topology.edges.as_slice(),
                                "scan F3D deleted treatment edges",
                            )? {
                                if !decode.contains_hash_set(
                                    &result_edges,
                                    edge,
                                    "check F3D result edge membership",
                                )? {
                                    decode.push_vec(
                                        &mut deleted_edges,
                                        *edge,
                                        "collect F3D treatment deleted edges",
                                    )?;
                                }
                            }
                            let (radii, treatment_edges) = treatment_edge_candidates(
                                decode,
                                None,
                                &inserted_faces,
                                result,
                                topology,
                                &deleted_edges,
                            )?;

                            decode
                                .insert_hash_map(
                                    &mut treatment_candidates_by_transition,
                                    (
                                        decode.copy_retained_text(
                                            &history.id,
                                            "copy F3D treatment transition key",
                                        )?,
                                        current_state_id,
                                        previous_state_id,
                                    ),
                                    EdgeTreatmentTransitionCandidates {
                                        radii,
                                        treatment_edges,
                                        deleted_edges,
                                    },
                                    "index F3D treatment transitions",
                                )
                                .map(|_| ())
                        })?;
                    }
                }
                if let Some(candidates) = decode.get_hash_map(
                    &treatment_candidates_by_transition,
                    &key,
                    "find F3D treatment transition candidates",
                )? {
                    operand.treatment_radius_candidates = decode.collect_vec(
                        candidates.radii.iter().cloned(),
                        "copy F3D treatment radius candidates",
                    )?;
                    let mut treatment_edges = decode.collect_vec(
                        candidates.treatment_edges.iter().copied(),
                        "copy F3D treatment edges",
                    )?;
                    // The geometric chain is transition-scoped. The deleted-
                    // set fallback is group-scoped because its proof depends
                    // on this operand group's compact member count.
                    if treatment_edges.is_empty() {
                        let is_edge_treatment = matches!(
                            crate::design::design_feature_family(&scope.kind()),
                            Some(
                                crate::design::DesignFeatureFamily::Fillet
                                    | crate::design::DesignFeatureFamily::Chamfer
                            )
                        );
                        let compact_member_count = decode
                            .get_hash_map(
                                &compact_group_counts,
                                &(
                                    decode.copy_retained_text(
                                        stream,
                                        "copy F3D compact group lookup stream",
                                    )?,
                                    operand.scope_record_index,
                                    operand.group_record_index,
                                ),
                                "find F3D compact edge group count",
                            )?
                            .copied()
                            .flatten();
                        treatment_edges = complete_compact_edge_treatment_deletions(
                            decode,
                            is_edge_treatment,
                            compact_member_count,
                            &candidates.deleted_edges,
                        )?;
                    }
                    operand.transition_edge_candidates = decode
                        .collect_vec(treatment_edges, "copy F3D transition edge candidates")?;
                }
            }
        }
        let direct = if let Some(binding) = operand.historical.as_ref() {
            if decode.contains(
                &binding.state_ids,
                &previous_state_id,
                "find F3D direct edge identity state",
            )? {
                historical_identity_edge(decode, binding.kind, binding.entity_ref, topology)?
            } else {
                None
            }
        } else {
            None
        };
        if let Some(edge) = direct {
            operand.resolved_edge_slot = Some(edge);
            operand.resolution_identity_id =
                Some(decode.copy_retained_text(&operand.id, "copy F3D edge resolution identity")?);
            continue;
        }
        let mut resolved = Vec::new();
        for identity in identities.iter().filter(|identity| {
            crate::ids::native_stream(&identity.id) == Some(stream)
                && identity.group_record_index == operand.group_record_index
        }) {
            let Some(persistent) = identity.persistent_identity() else {
                continue;
            };
            let Some((kind, entity_ref, states)) =
                scoped_identities.selection_identity_kind(decode, persistent.local_id)?
            else {
                continue;
            };
            if !decode.contains(
                &states,
                &previous_state_id,
                "find F3D resolved edge identity state",
            )? {
                continue;
            }
            let Some(edge) = historical_identity_edge(decode, kind, entity_ref, topology)? else {
                continue;
            };

            decode.reserve_vec(&mut resolved, 1, "collect F3D resolved edge identities")?;
            resolved.push((edge, identity.id.as_str()));
        }
        let mut resolved = resolved.into_iter();
        let Some((edge, identity_id)) = resolved.next() else {
            continue;
        };
        if resolved.any(|candidate| candidate.0 != edge) {
            continue;
        }
        operand.resolved_edge_slot = Some(edge);
        operand.resolution_identity_id =
            Some(decode.copy_retained_text(identity_id, "copy F3D edge resolution identity")?);
    }
    Ok(())
}

pub(super) fn complete_compact_edge_treatment_deletions(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    is_edge_treatment: bool,
    compact_member_count: Option<usize>,
    deleted_edges: &[i64],
) -> Result<Vec<i64>, cadmpeg_core::CodecError> {
    if is_edge_treatment
        && !deleted_edges.is_empty()
        && compact_member_count == Some(deleted_edges.len())
    {
        decode.collect_vec(
            deleted_edges.iter().copied(),
            "copy F3D compact treatment deletions",
        )
    } else {
        Ok(Vec::new())
    }
}

/// Resolve a class-297 edge-treatment member whose persistent local identity
/// names the member's embedded bounded-face recipe. The rule selects every
/// deleted treatment edge on the recipe's exact preceding support face.
pub(crate) fn bind_edge_identity_bounded_face_rules(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [DesignEdgeIdentityOperand],
    face_operands: &[crate::records::topology::face::DesignFaceOperand],
) -> Result<(), cadmpeg_core::CodecError> {
    use crate::records::recipes::ConstructionRecipeKind;

    for operand in operands {
        operand.resolved_edge_slots.clear();
        if operand.resolved_edge_slot.is_some() {
            continue;
        }
        let mut matches_face = |face: &crate::records::topology::face::DesignFaceOperand| {
            Ok(decode.equal(
                &crate::ids::native_stream(&face.id),
                &crate::ids::native_stream(&operand.id),
                "compare F3D bounded treatment operand streams",
            )? && face.scope_record_index == operand.scope_record_index
                && face.group_record_index() == Some(operand.group_record_index)
                && face.group_member_ordinal() == Some(operand.group_member_ordinal)
                && face.record_index() == operand.record_index()
                && decode.equal(
                    &face.class_tag,
                    &operand.class_tag,
                    "compare F3D bounded treatment operand class tags",
                )?
                && face.recipe_kind == ConstructionRecipeKind::BoundedFace
                && u64::from(face.recipe_record_index()) == operand.local_id)
        };
        let Some(face_index) = decode.position_by(
            face_operands,
            &mut matches_face,
            "find F3D bounded treatment face operand",
        )?
        else {
            continue;
        };
        if decode
            .position_by(
                &face_operands[face_index + 1..],
                &mut matches_face,
                "find additional F3D bounded treatment face operand",
            )?
            .is_some()
        {
            continue;
        }
        let face = &face_operands[face_index];
        let [support] = face.historical_support_contexts.as_slice() else {
            continue;
        };
        let mut unique_faces = HashSet::new();
        for face in decode.admit_iter(
            &support.preceding_face_slots,
            "scan F3D support preceding face slots",
        )? {
            decode.insert_hash_set(
                &mut unique_faces,
                *face,
                "index F3D bounded treatment faces",
            )?;
        }
        if support.preceding_face_slots.is_empty()
            || !decode.equal(
                &support.changed_preceding_face_slots,
                &support.preceding_face_slots,
                "compare F3D bounded treatment preceding faces",
            )?
            || support.preceding_face_boundaries.len() != support.preceding_face_slots.len()
            || unique_faces.len() != support.preceding_face_slots.len()
        {
            continue;
        }
        let mut invalid_boundary = false;
        for boundary in decode.admit_iter(
            &support.preceding_face_boundaries,
            "scan F3D bounded treatment boundaries",
        )? {
            let duplicate_count = decode
                .admit_iter(
                    &support.preceding_face_boundaries,
                    "scan F3D duplicate treatment boundaries",
                )?
                .filter(|candidate| candidate.face_slot == boundary.face_slot)
                .count();
            if duplicate_count != 1 {
                invalid_boundary = true;
                break;
            }
            if !decode.contains(
                &support.preceding_face_slots,
                &boundary.face_slot,
                "find F3D treatment boundary face",
            )? {
                invalid_boundary = true;
                break;
            }
            if decode
                .admit_iter(&boundary.loops, "scan F3D treatment boundary loops")?
                .any(|loop_| loop_.boundary.coedges().next().is_none())
            {
                invalid_boundary = true;
                break;
            }
        }
        if invalid_boundary {
            continue;
        }
        let mut transition = HashSet::new();
        for edge in decode.admit_iter(
            &operand.transition_edge_candidates,
            "scan F3D operand transition edge candidates",
        )? {
            decode.insert_hash_set(&mut transition, *edge, "index F3D bounded treatment edges")?;
        }
        if transition.is_empty() {
            continue;
        }
        let mut seen = HashSet::new();
        let mut resolved = Vec::new();
        {
            let mut insert_resolved_edge = |edge: i64| -> Result<(), cadmpeg_core::CodecError> {
                if transition.contains(&edge)
                    && decode.insert_hash_set(
                        &mut seen,
                        edge,
                        "index F3D bounded resolved edges",
                    )?
                {
                    decode.reserve_vec(&mut resolved, 1, "collect F3D bounded resolved edges")?;
                    resolved.push(edge);
                }
                Ok(())
            };
            for boundary in decode.admit_iter(
                &support.preceding_face_boundaries,
                "scan F3D bounded treatment edge boundaries",
            )? {
                for loop_ in
                    decode.admit_iter(&boundary.loops, "scan F3D bounded treatment edge loops")?
                {
                    match &loop_.boundary {
                        crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Coedges(
                            rows,
                        ) => {
                            for row in decode.admit_iter(
                                rows,
                                "scan F3D bounded treatment boundary members",
                            )? {
                                insert_resolved_edge(row.edge_slot)?;
                            }
                        }
                        crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Vertices(
                            rows,
                        ) => {
                            for row in decode.admit_iter(
                                rows,
                                "scan F3D bounded treatment boundary members",
                            )? {
                                insert_resolved_edge(row.coedge.edge_slot)?;
                            }
                        }
                        crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Points(
                            rows,
                        ) => {
                            for row in decode.admit_iter(
                                rows,
                                "scan F3D bounded treatment boundary members",
                            )? {
                                insert_resolved_edge(row.vertex.coedge.edge_slot)?;
                            }
                        }
                        crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Positions(
                            rows,
                        ) => {
                            for row in decode.admit_iter(
                                rows,
                                "scan F3D bounded treatment boundary members",
                            )? {
                                insert_resolved_edge(row.point.vertex.coedge.edge_slot)?;
                            }
                        }
                    }
                }
            }
        }
        operand.resolved_edge_slots = resolved;
        if !operand.resolved_edge_slots.is_empty() {
            operand.resolution_identity_id =
                Some(decode.copy_retained_text(&face.id, "copy F3D bounded edge identity")?);
        }
    }
    Ok(())
}

pub(super) fn historical_identity_edge(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    kind: AsmHistoricalEntityKind,
    entity_ref: i64,
    topology: &AsmHistoricalTopology,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let mut candidates_storage = decode.reserve_scoped(0, "collect F3D identity edges")?;
    let candidates = candidates_storage
        .with_storage(|| historical_identity_edges(decode, kind, entity_ref, topology))?;
    single_btree_value(decode, Some(candidates), "take F3D identity edge")
}

fn historical_identity_edges(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    kind: AsmHistoricalEntityKind,
    entity_ref: i64,
    topology: &AsmHistoricalTopology,
) -> Result<BTreeSet<i64>, cadmpeg_core::CodecError> {
    let mut candidates = BTreeSet::new();
    match kind {
        AsmHistoricalEntityKind::Edge => {
            if decode.contains(&topology.edges, &entity_ref, "find F3D identity edge")? {
                decode.insert_btree_set(
                    &mut candidates,
                    entity_ref,
                    "collect F3D identity edges",
                )?;
            }
        }
        AsmHistoricalEntityKind::Coedge => {
            for edge in decode
                .admit_iter(
                    &topology.coedge_topology,
                    "scan F3D identity coedge topology",
                )?
                .filter(|coedge| coedge.coedge == entity_ref)
                .map(|coedge| coedge.edge)
            {
                decode.insert_btree_set(&mut candidates, edge, "collect F3D identity edges")?;
            }
        }
        AsmHistoricalEntityKind::Pcurve => {
            let mut coedges = HashSet::new();
            for coedge in decode
                .admit_iter(
                    &topology.coedge_pcurves,
                    "scan F3D identity pcurve bindings",
                )?
                .filter(|binding| binding.carrier == Some(entity_ref))
                .map(|binding| binding.entity)
            {
                decode.insert_hash_set(
                    &mut coedges,
                    coedge,
                    "index F3D identity pcurve coedges",
                )?;
            }
            for edge in decode
                .admit_iter(&topology.coedge_topology, "scan F3D pcurve coedge topology")?
                .filter(|coedge| coedges.contains(&coedge.coedge))
                .map(|coedge| coedge.edge)
            {
                decode.insert_btree_set(&mut candidates, edge, "collect F3D identity edges")?;
            }
        }
        AsmHistoricalEntityKind::Curve => {
            for edge in decode
                .admit_iter(&topology.edge_curves, "scan F3D identity edge curves")?
                .filter(|binding| binding.carrier == Some(entity_ref))
                .map(|binding| binding.entity)
            {
                decode.insert_btree_set(&mut candidates, edge, "collect F3D identity edges")?;
            }
        }
        AsmHistoricalEntityKind::Vertex | AsmHistoricalEntityKind::Point => {
            let mut vertices = HashSet::new();
            if kind == AsmHistoricalEntityKind::Vertex {
                decode.insert_hash_set(&mut vertices, entity_ref, "index F3D identity vertices")?;
            } else {
                for vertex in decode
                    .admit_iter(&topology.vertex_points, "scan F3D identity vertex points")?
                    .filter(|binding| binding.carrier == entity_ref)
                    .map(|binding| binding.entity)
                {
                    decode.insert_hash_set(&mut vertices, vertex, "index F3D identity vertices")?;
                }
            }
            for edge in decode
                .admit_iter(&topology.edge_vertices, "scan F3D identity edge vertices")?
                .filter(|edge| {
                    vertices.contains(&edge.start_vertex) || vertices.contains(&edge.end_vertex)
                })
                .map(|edge| edge.edge)
            {
                decode.insert_btree_set(&mut candidates, edge, "collect F3D identity edges")?;
            }
        }
        AsmHistoricalEntityKind::Body
        | AsmHistoricalEntityKind::Region
        | AsmHistoricalEntityKind::Shell
        | AsmHistoricalEntityKind::Face
        | AsmHistoricalEntityKind::Loop
        | AsmHistoricalEntityKind::Surface => {}
    }
    Ok(candidates)
}

fn historical_identity_faces(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    kind: AsmHistoricalEntityKind,
    entity_ref: i64,
    topology: &AsmHistoricalTopology,
) -> Result<BTreeSet<i64>, cadmpeg_core::CodecError> {
    let mut carriers = BTreeSet::new();
    match kind {
        AsmHistoricalEntityKind::Face => {
            decode.insert_btree_set(&mut carriers, entity_ref, "collect F3D identity faces")?;
            return Ok(carriers);
        }
        AsmHistoricalEntityKind::Loop => {
            decode.insert_btree_set(&mut carriers, entity_ref, "index F3D identity face loops")?;
        }
        AsmHistoricalEntityKind::Coedge => {
            for relation in
                decode.admit_iter(&topology.loop_coedges, "scan F3D identity loop coedges")?
            {
                if decode.contains(
                    &relation.member_refs,
                    &entity_ref,
                    "find F3D identity face loop",
                )? {
                    decode.insert_btree_set(
                        &mut carriers,
                        relation.owner_ref,
                        "index F3D identity face loops",
                    )?;
                }
            }
        }
        AsmHistoricalEntityKind::Pcurve => {
            let mut coedges = HashSet::new();
            for coedge in decode
                .admit_iter(&topology.coedge_pcurves, "scan F3D identity face pcurves")?
                .filter(|binding| binding.carrier == Some(entity_ref))
                .map(|binding| binding.entity)
            {
                decode.insert_hash_set(&mut coedges, coedge, "index F3D identity face pcurves")?;
            }
            for relation in decode.admit_iter(
                &topology.loop_coedges,
                "scan F3D identity pcurve loop coedges",
            )? {
                let contains_coedge = decode.any_by(
                    &relation.member_refs,
                    |coedge| {
                        decode.contains_hash_set(
                            &coedges,
                            coedge,
                            "find F3D identity pcurve loop member",
                        )
                    },
                    "scan F3D identity pcurve loop members",
                )?;
                if contains_coedge {
                    decode.insert_btree_set(
                        &mut carriers,
                        relation.owner_ref,
                        "index F3D identity face loops",
                    )?;
                }
            }
        }
        AsmHistoricalEntityKind::Surface => {
            for face in decode
                .admit_iter(&topology.face_surfaces, "scan F3D identity face surfaces")?
                .filter(|binding| binding.carrier == entity_ref)
                .map(|binding| binding.entity)
            {
                decode.insert_btree_set(&mut carriers, face, "collect F3D identity faces")?;
            }
            return Ok(carriers);
        }
        AsmHistoricalEntityKind::Body
        | AsmHistoricalEntityKind::Region
        | AsmHistoricalEntityKind::Shell
        | AsmHistoricalEntityKind::Edge
        | AsmHistoricalEntityKind::Vertex
        | AsmHistoricalEntityKind::Point
        | AsmHistoricalEntityKind::Curve => return Ok(BTreeSet::new()),
    }
    let mut faces = BTreeSet::new();
    for relation in decode.admit_iter(&topology.face_loops, "scan F3D identity face loops")? {
        let contains_loop = decode.any_by(
            &relation.member_refs,
            |loop_slot| {
                decode.contains_btree_set(
                    &carriers,
                    loop_slot,
                    "find F3D identity face loop member",
                )
            },
            "scan F3D identity face loop members",
        )?;
        if contains_loop {
            decode.insert_btree_set(
                &mut faces,
                relation.owner_ref,
                "collect F3D identity faces",
            )?;
        }
    }
    Ok(faces)
}
