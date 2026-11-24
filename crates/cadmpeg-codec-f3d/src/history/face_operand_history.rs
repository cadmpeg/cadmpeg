// SPDX-License-Identifier: Apache-2.0
//! Face operand history candidates and transitions.

use super::binding_histories::{
    history_state_indexes, visit_state_groups_oldest_first, BindingHistories,
};
use super::body_closures::stable_ref;
use super::cache::SnapshotCache;
use super::edge_operand_history::keep_first_error;
use super::face_boundaries::{face_changes_across_state_chain, face_operand_support_candidates};
use super::face_resolution::{
    effective_faces, grouped_reference_face_candidate, historical_brep_source,
    resolve_bounded_face_recipe_target, resolve_direct_face_recipe_clauses,
    resolve_draft_face_by_surface_transition, resolve_pattern_face_by_surface_radius,
    resolve_split_tool_face, resolve_thread_face_by_transition,
    select_legacy_extrude_face_candidate, LegacyFaceResolution,
};
use super::history_states::{
    bound_history_state_pair, bound_scope_history, effective_scope_previous_history_state_id,
    history_state_index, unique_history_state_pair,
};
use super::profile_face_groups::bind_profile_face_group_cardinality;
use super::projection_was_finalized;
use super::recipe_index::RecipeTopologyCache;
use super::topology_cache::TopologyQueryCache;
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology, AsmHistory};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use crate::records::topology::extrude_selection::DesignOperandRole;
use std::collections::HashMap;

pub(super) fn exact_face_selection_group<'a>(
    operand: &crate::records::topology::face::DesignFaceOperand,
    scope: &crate::records::feature::scope::DesignParameterScope,
    operand_groups: &'a [crate::records::topology::construction::DesignConstructionOperandGroup],
) -> Option<&'a crate::records::topology::construction::DesignConstructionOperandGroup> {
    let stream = crate::ids::native_stream(&operand.id)?;
    if crate::ids::native_stream(&scope.id) != Some(stream) {
        return None;
    }
    let group_record_index = operand.group_record_index()?;
    let group_member_ordinal = usize::try_from(operand.group_member_ordinal()?).ok()?;
    let mut groups = operand_groups.iter().filter(|group| {
        crate::ids::native_stream(&group.id) == Some(stream)
            && group.scope_record_index == scope.record_index
            && group.record_index == group_record_index
            && group.role() == DesignOperandRole::ROLE_0X10
            && group
                .members()
                .get(group_member_ordinal)
                .map(|member| &member.value)
                == Some(&operand.record_index())
    });
    let group = groups.next()?;
    groups.next().is_none().then_some(group)
}

/// Bind one recipe reference to every live face or edge fragment carrying its
/// token and Design reference in the recipe-state topology.
pub(super) fn bind_historical_recipe_reference_candidates<'a>(
    reference: &mut crate::records::dimensions::DesignRecipeReference,
    topology: &'a AsmHistoricalTopology,
    recipe_topologies: &mut RecipeTopologyCache<'a>,
) -> Result<(), cadmpeg_core::CodecError> {
    reference.candidate_faces.clear();
    reference.candidate_edges.clear();
    reference.alternate_selector_faces.clear();
    reference.alternate_selector_edges.clear();
    let index = recipe_topologies.get(topology);
    for tag_index in index.matching_tags(
        &reference.token,
        reference.token_integer,
        reference.design_reference,
    ) {
        let tag = &index.topology.persistent_subentity_tags[*tag_index];
        match tag.entity_kind {
            AsmHistoricalEntityKind::Face => {
                reference
                    .candidate_faces
                    .push(historical_face_id(tag.entity_ref)?);
            }
            AsmHistoricalEntityKind::Edge => {
                reference
                    .candidate_edges
                    .push(historical_edge_id(tag.entity_ref)?);
            }
            _ => {}
        }
    }
    reference
        .candidate_faces
        .sort_by(|left, right| left.as_str().cmp(right.as_str()));
    reference.candidate_faces.dedup();
    reference
        .candidate_edges
        .sort_by(|left, right| left.as_str().cmp(right.as_str()));
    reference.candidate_edges.dedup();
    Ok(())
}

pub(super) fn historical_recipe_faces<'a>(
    design_reference: i64,
    topology: &'a AsmHistoricalTopology,
    recipe_topologies: &mut RecipeTopologyCache<'a>,
) -> Result<Vec<cadmpeg_ir::ids::FaceId>, cadmpeg_core::CodecError> {
    let index = recipe_topologies.get(topology);
    let topology = index.topology;
    let mut faces = Vec::new();
    for tag_index in index.face_tags(design_reference) {
        let tag = &topology.persistent_subentity_tags[*tag_index];

        faces.push(historical_face_id(tag.entity_ref)?);
    }
    faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    faces.dedup();
    Ok(faces)
}

pub(super) fn direct_face_recipe_candidates(
    recipe_kind: crate::records::recipes::ConstructionRecipeKind,
    references: &[crate::records::dimensions::DesignRecipeReference],
    recipe_record_index: i32,
) -> Option<Vec<cadmpeg_ir::ids::FaceId>> {
    if recipe_kind != crate::records::recipes::ConstructionRecipeKind::Face {
        return None;
    }
    let mut faces = Vec::new();
    for face in references
        .iter()
        .filter(|reference| reference.design_reference == i64::from(recipe_record_index))
        .flat_map(|reference| &reference.candidate_faces)
    {
        faces.push(face.clone());
    }
    faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    faces.dedup();
    (!faces.is_empty()).then_some(faces)
}

pub(super) fn historical_face_id(
    slot: i64,
) -> Result<cadmpeg_ir::ids::FaceId, cadmpeg_core::CodecError> {
    let text = format!("f3d:brep:entity#{slot}");
    cadmpeg_ir::ids::FaceId::mint(text).map_err(cadmpeg_core::CodecError::malformed)
}

pub(super) fn historical_edge_id(
    slot: i64,
) -> Result<cadmpeg_ir::ids::EdgeId, cadmpeg_core::CodecError> {
    let text = format!("f3d:brep:entity#{slot}");
    cadmpeg_ir::ids::EdgeId::mint(text).map_err(cadmpeg_core::CodecError::malformed)
}

pub(crate) fn bind_face_operand_history_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::topology::face::DesignFaceOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    operand_groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    recipes: &[crate::records::recipes::ConstructionRecipe],
    mut histories: BindingHistories<'_>,
    scope_histories: &HashMap<String, String>,
) -> Result<Vec<cadmpeg_ir::report::loss::LossNote>, cadmpeg_core::CodecError> {
    if projection_was_finalized(histories.get()) {
        return Ok(Vec::new());
    }
    let recipe_record_indices = recipes
        .iter()
        .filter_map(|recipe| Some((recipe.id.as_str(), recipe.record_index?.value)))
        .collect::<std::collections::HashMap<_, _>>();
    let mut jobs = Vec::new();
    for (position, operand) in operands.iter_mut().enumerate() {
        operand.preceding_candidate_faces.clear();
        operand.changed_candidate_faces.clear();
        operand.historical_support_contexts.clear();
        operand.resolved_face_slots.clear();
        operand.resolved_active_face = None;
        let stream = crate::ids::native_stream(&operand.id);
        let mut matching_scopes = scopes.iter().filter(|scope| {
            scope.record_index == operand.scope_record_index
                && crate::ids::native_stream(&scope.id) == stream
        });
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let histories = histories.get();
        let scoped_history = if scope_histories.contains_key(&scope.id) {
            let Some(history) = bound_scope_history(&scope.id, scope_histories, histories) else {
                continue;
            };
            Some(history)
        } else {
            None
        };
        let scoped_histories = scoped_history.map_or(histories, std::slice::from_ref);
        let Some(state_id) = scope.history_state_id() else {
            continue;
        };
        let Some(previous_state_id) =
            effective_scope_previous_history_state_id(scope, scoped_histories)
        else {
            continue;
        };
        let Some((history, state, previous)) = (if scoped_history.is_some() {
            bound_history_state_pair(
                &scope.id,
                state_id,
                previous_state_id,
                scope_histories,
                histories,
            )
        } else {
            unique_history_state_pair(histories, state_id, previous_state_id)
        }) else {
            continue;
        };
        let (Some((history_index, state_index)), Some((_, previous_index))) = (
            history_state_indexes(histories, history, state),
            history_state_indexes(histories, history, previous),
        ) else {
            continue;
        };
        jobs.push((
            history_index,
            previous_index,
            (position, scope, state_index),
        ));
    }
    let mut first_error = None;
    let mut withdrawals = super::fallback_face::Withdrawals::default();
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
            let states = history_state_index(history);
            let mut caches = FaceOperandCaches::default();
            for (position, scope, state_index) in group {
                let (Some(operand), Some(state)) = (
                    operands.get_mut(position),
                    history.states().get(state_index),
                ) else {
                    continue;
                };
                let transition = FaceOperandTransition {
                    scope,
                    history,
                    state,
                    previous,
                    topology,
                    states: &states,
                };
                if let Err(error) = bind_historical_face_operand_candidates(
                    operand,
                    &transition,
                    operand_groups,
                    &recipe_record_indices,
                    &mut caches,
                    &mut withdrawals,
                ) {
                    keep_first_error(&mut first_error, position, error);
                } else if let Some(loss) = super::face_recipe::bind_operation_operand(
                    operand,
                    &scope.kind(),
                    topology,
                    &mut caches.face_recipes,
                ) {
                    withdrawals.losses.push(loss);
                }
            }
        },
    );
    if let Some((_, error)) = first_error {
        return Err(error);
    }
    bind_profile_face_group_cardinality(
        decode,
        operands,
        scopes,
        operand_groups,
        histories.get(),
        scope_histories,
        &mut withdrawals,
    )?;
    Ok(withdrawals.losses)
}

/// One face operand's scope and state pair with the previous-state topology.
pub(super) struct FaceOperandTransition<'a> {
    pub(super) scope: &'a crate::records::feature::scope::DesignParameterScope,
    pub(super) history: &'a AsmHistory,
    pub(super) state: &'a AsmDeltaState,
    pub(super) previous: &'a AsmDeltaState,
    pub(super) topology: &'a AsmHistoricalTopology,
    pub(super) states: &'a HashMap<i64, Option<&'a AsmDeltaState>>,
}

/// Indexes over the topologies of one previous state and its results.
#[derive(Default)]
pub(super) struct FaceOperandCaches<'a> {
    pub(super) topology_queries: TopologyQueryCache<'a>,
    pub(super) recipe_topologies: RecipeTopologyCache<'a>,
    pub(super) face_recipes:
        SnapshotCache<'a, AsmHistoricalTopology, super::face_recipe::FaceRecipeIndex>,
}

pub(super) fn bind_historical_face_operand_candidates<'a>(
    operand: &mut crate::records::topology::face::DesignFaceOperand,
    transition: &FaceOperandTransition<'a>,
    operand_groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    recipe_record_indices: &HashMap<&str, i32>,
    caches: &mut FaceOperandCaches<'a>,
    withdrawals: &mut super::fallback_face::Withdrawals,
) -> Result<(), cadmpeg_core::CodecError> {
    let &FaceOperandTransition {
        scope,
        history,
        state,
        previous,
        topology,
        states,
    } = transition;
    let previous_state_id = previous.state_id;
    let FaceOperandCaches {
        topology_queries,
        recipe_topologies,
        face_recipes,
    } = caches;
    let stream = crate::ids::native_stream(&operand.id);
    let feature_family = crate::design::design_feature_family(&scope.kind());
    let (extrude_face_role, selected_start_group) =
        if feature_family == Some(crate::design::DesignFeatureFamily::Extrude) {
            let mut extrude_face_groups = operand_groups.iter().filter(|group| {
                crate::ids::native_stream(&group.id) == stream
                    && group.scope_record_index == scope.record_index
                    && Some(group.record_index) == operand.group_record_index()
                    && group.extrude_face_role().is_some()
                    && operand
                        .group_member_ordinal()
                        .and_then(|ordinal| usize::try_from(ordinal).ok())
                        .and_then(|ordinal| group.members().get(ordinal))
                        .is_some_and(|member| member.value == operand.record_index())
            });
            extrude_face_groups
                .next()
                .filter(|_| extrude_face_groups.next().is_none())
                .map_or((None, false), |group| {
                    (
                        group.extrude_face_role(),
                        group.role() == DesignOperandRole::ROLE_0X5,
                    )
                })
        } else {
            (None, false)
        };
    let has_extrude_face_group = extrude_face_role.is_some();
    let alternate_start_face = if selected_start_group
        && extrude_face_role
            == Some(crate::records::topology::extrude_selection::DesignExtrudeFaceRole::Start)
        && operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::BoundedFace
    {
        recipe_record_indices
            .get(operand.recipe_id.as_str())
            .and_then(|record_index| {
                recipe_topologies.get(topology).unique_alternate_face(
                    &operand.recipe_references,
                    *record_index,
                    &operand.alternate_selector_candidate_faces,
                    historical_brep_source(&history.id),
                )
            })
    } else {
        None
    };
    for reference in &mut operand.recipe_references {
        bind_historical_recipe_reference_candidates(reference, topology, recipe_topologies)?;
    }
    let has_extrude_profile_group = super::recipe_fill::is_extrude_profile_member(
        operand,
        scope,
        operand_groups,
        feature_family == Some(crate::design::DesignFeatureFamily::Extrude),
    );
    let resolved_recipe_face = face_recipes
        .get(topology, super::face_recipe::FaceRecipeIndex::new)
        .resolve(operand);
    let closed_edge_face = resolved_recipe_face.filter(|_| operand.has_closed_edge_node());
    let recipe_face = resolved_recipe_face.filter(|_| !operand.has_closed_edge_node());
    if scope.kind() == crate::records::feature::scope::DesignFeatureKind::DeleteFace {
        operand.resolved_face_slots = resolved_recipe_face.into_iter().collect();
        return Ok(());
    }
    if let Some(recipe_record_index) = recipe_record_indices.get(operand.recipe_id.as_str()) {
        operand.candidate_faces =
            historical_recipe_faces(i64::from(*recipe_record_index), topology, recipe_topologies)?;
        operand.unreferenced_candidate_faces = (operand.candidate_faces.iter().filter(|face| {
            !operand
                .recipe_references
                .iter()
                .flat_map(|reference| &reference.candidate_faces)
                .any(|candidate| candidate == *face)
        }))
        .cloned()
        .collect::<Vec<_>>();
        operand.alternate_selector_candidate_faces.clear();
    }
    let Some(changed_faces) = face_changes_across_state_chain(state, previous_state_id, states)
    else {
        return Ok(());
    };
    let direct_face_candidates =
        if let Some(record_index) = recipe_record_indices.get(operand.recipe_id.as_str()) {
            direct_face_recipe_candidates(
                operand.recipe_kind,
                &operand.recipe_references,
                *record_index,
            )
        } else {
            None
        };
    let thread_face_candidates = if feature_family
        == Some(crate::design::DesignFeatureFamily::Thread)
        && exact_face_selection_group(operand, scope, operand_groups).is_some()
    {
        operand
            .recipe_references
            .first()
            .and_then(|reference| {
                let candidates = effective_faces(reference);
                (!candidates.is_empty()).then_some(candidates)
            })
            .map(<[cadmpeg_ir::ids::FaceId]>::to_vec)
    } else {
        None
    };
    let nested_split_face_candidates = if scope.kind()
        == crate::records::feature::scope::DesignFeatureKind::SplitFace
        && exact_face_selection_group(operand, scope, operand_groups).is_some()
    {
        crate::design::face_resolve::nested_bounded_face_history_candidates(operand)
    } else {
        None
    };
    let grouped_reference_face_candidates = (!matches!(
        feature_family,
        Some(
            crate::design::DesignFeatureFamily::Thread | crate::design::DesignFeatureFamily::Split
        )
    ))
    .then(|| grouped_reference_face_candidate(operand, topology, &changed_faces))
    .flatten()
    .map(|face| vec![face]);
    let legacy_face_candidates = (|| {
        if feature_family != Some(crate::design::DesignFeatureFamily::Extrude) {
            return None;
        }
        let group_record_index = operand.group_record_index()?;
        let mut groups = operand_groups.iter().filter(|group| {
                crate::ids::native_stream(&group.id) == stream
                    && group.scope_record_index == scope.record_index
                    && group.record_index == group_record_index
                    && group.extrude_role().is_some_and(|role| {
                        matches!(
                            role,
                            crate::records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_)
                        )
                    })
                    && group.extrude_face_role().is_some()
            });
        groups.next()?;
        if groups.next().is_some() {
            return None;
        }
        let recipe_record_index = recipe_record_indices.get(operand.recipe_id.as_str())?;
        crate::design::face_resolve::legacy_face_recipe_reference_candidates(
            operand,
            *recipe_record_index,
        )
    })();
    let fallback_candidates;
    let history_candidates = if let Some(candidates) = direct_face_candidates
        .as_deref()
        .or(thread_face_candidates.as_deref())
        .or(nested_split_face_candidates.as_deref())
        .or(grouped_reference_face_candidates.as_deref())
    {
        candidates
    } else {
        fallback_candidates =
            crate::design::face_resolve::historical_face_operand_candidates(operand);
        &fallback_candidates
    };
    operand.preceding_candidate_faces =
        super::selection::faces_in_topology(history_candidates, topology);
    operand.changed_candidate_faces = (operand.preceding_candidate_faces.iter().filter(|face| {
        stable_ref(face.as_str()).is_some_and(|slot| changed_faces.contains(&slot))
    }))
    .cloned()
    .collect::<Vec<_>>();
    let support_candidates = if feature_family == Some(crate::design::DesignFeatureFamily::Extrude)
        && operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::BoundedFace
    {
        face_operand_support_candidates(
            history_candidates,
            operand.recipe_references.first(),
            recipe_record_indices
                .get(operand.recipe_id.as_str())
                .copied(),
        )
    } else {
        std::borrow::Cow::Borrowed(history_candidates)
    };
    operand.historical_support_contexts = super::face_support::historical_face_support_contexts(
        &support_candidates,
        history,
        topology,
        &changed_faces,
        topology_queries,
    );
    if direct_face_candidates.is_some() {
        operand.resolved_face_slots = operand
            .preceding_candidate_faces
            .iter()
            .filter_map(|face| stable_ref(face.as_str()))
            .collect::<Vec<_>>();
        return Ok(());
    }
    let preserves_stable_face_set = feature_family
            == Some(crate::design::DesignFeatureFamily::Shell)
            || operand
                .group_record_index()
                .is_some_and(|group_record_index| {
                    let mut groups = operand_groups.iter().filter(|group| {
                        crate::ids::native_stream(&group.id) == stream
                            && group.scope_record_index == scope.record_index
                            && group.record_index == group_record_index
                    });
                    let Some(group) = groups.next() else {
                        return false;
                    };
                    groups.next().is_none()
                        && group.extrude_face_role()
                            == Some(crate::records::topology::extrude_selection::DesignExtrudeFaceRole::Termination)
                });
    operand.resolved_face_slots = match &scope.payload() {
        crate::records::feature::scope::DesignScopePayload::OffsetFaces(Some(_)) => {
            let direct = resolve_direct_face_recipe_clauses(
                &operand.recipe_references,
                topology,
                &changed_faces,
            );
            if direct.is_empty() {
                crate::design::face_resolve::resolve_face_operand_history_candidates(operand)
                    .into_iter()
                    .collect()
            } else {
                direct
            }
        }
        _ => {
            let direct =
                crate::design::face_resolve::resolve_face_operand_history_candidates(operand);
            if let Some(direct) = direct {
                vec![direct]
            } else if scope.kind()
                == crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
            {
                crate::design::face_resolve::resolve_surface_delete_face_history_set(operand)
                    .unwrap_or_default()
            } else if preserves_stable_face_set {
                if let Some(stable) =
                    crate::design::face_resolve::resolve_stable_bounded_face_history_set(operand)
                {
                    stable
                } else {
                    crate::design::face_resolve::resolve_bounded_face_history_candidates(operand)
                        .unwrap_or_default()
                }
            } else if let Some(bounded) =
                crate::design::face_resolve::resolve_bounded_face_history_candidates(operand)
            {
                bounded
            } else {
                let pattern = if feature_family
                    == Some(crate::design::DesignFeatureFamily::CircularPattern)
                {
                    if let Some(result) = history.topology(state) {
                        resolve_pattern_face_by_surface_radius(
                            crate::design::face_resolve::face_operand_candidates(operand),
                            topology,
                            result,
                            &changed_faces,
                        )
                    } else {
                        None
                    }
                } else {
                    None
                };
                pattern.into_iter().collect()
            }
        }
    };
    let fallback_slots = operand.resolved_face_slots.clone();
    if let Some(candidates) = &thread_face_candidates {
        // Thread's first prefix reference is its exclusive selection lane.
        // An unresolved lane must not fall back to construction context.
        operand.resolved_face_slots =
            crate::design::face_resolve::resolve_face_operand_history_candidate_from(
                operand, candidates,
            )
            .or_else(|| {
                resolve_thread_face_by_transition(
                    scope,
                    candidates,
                    history,
                    topology,
                    &changed_faces,
                )
            })
            .into_iter()
            .collect();
    }
    if let Some(candidates) = &grouped_reference_face_candidates {
        // A grouped frame's unique changed topology-face reference is its
        // exact historical selection lane.
        operand.resolved_face_slots =
            crate::design::face_resolve::resolve_face_operand_history_candidate_from(
                operand, candidates,
            )
            .into_iter()
            .collect();
    }
    if feature_family == Some(crate::design::DesignFeatureFamily::Split) {
        operand.resolved_face_slots = resolve_split_tool_face(operand, topology)
            .into_iter()
            .collect();
    }
    if feature_family == Some(crate::design::DesignFeatureFamily::Loft)
        && operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::BoundedFace
        && state
            .transition
            .as_ref()
            .is_some_and(|transition| transition.previous_state_id == Some(previous_state_id))
    {
        if let Some((result, transition)) = history.topology(state).zip(state.transition.as_ref()) {
            if let Some(face) = resolve_bounded_face_recipe_target(
                operand,
                topology,
                result,
                &transition.topology.bodies.inserted,
                topology_queries,
            )? {
                operand.resolved_face_slots = vec![face];
            }
        }
    }
    if recipe_face.is_none()
        && feature_family == Some(crate::design::DesignFeatureFamily::Extrude)
        && operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::BoundedFace
        && grouped_reference_face_candidates.is_none()
        && operand.alternate_selector_candidate_faces.is_empty()
        && operand.resolved_face_slots == fallback_slots
    {
        super::fallback_face::check_fallback_faces(
            operand,
            topology,
            topology_queries,
            withdrawals,
        );
    }
    if operand.resolved_face_slots.is_empty()
        && scope.kind() == crate::records::feature::scope::DesignFeatureKind::Draft
    {
        if let Some(result) = history.topology(state) {
            if let Some(face) = resolve_draft_face_by_surface_transition(operand, topology, result)
            {
                operand.resolved_face_slots = vec![face];
            }
        }
    }
    if operand.resolved_face_slots.is_empty() {
        if let Some(candidates) = legacy_face_candidates {
            match select_legacy_extrude_face_candidate(
                candidates,
                topology,
                &changed_faces,
                historical_brep_source(&history.id),
            ) {
                Some(LegacyFaceResolution::Historical(slot))
                    if withdrawals.admits(operand, &[slot], topology, topology_queries) =>
                {
                    operand.resolved_face_slots = vec![slot];
                }
                Some(LegacyFaceResolution::Active(face)) if !withdrawals.withdrew(operand) => {
                    operand.resolved_active_face = Some(face);
                }
                Some(LegacyFaceResolution::Historical(slot)) => withdrawals.refuse(
                    &format!("Extrude face operand {}", operand.id),
                    &format!("face {slot}"),
                    "legacy start and termination rule",
                ),
                Some(LegacyFaceResolution::Active(face)) => withdrawals.refuse(
                    &format!("Extrude face operand {}", operand.id),
                    &format!("active face {}", face.as_str()),
                    "legacy start and termination rule",
                ),
                None => {}
            }
        }
    }
    if let Some(face) = recipe_face.or(closed_edge_face) {
        // Every recipe node matches one boundary edge of this face and the
        // face fans around both of its ends. An earlier selection that no
        // check withdrew stays: the recipe fills an empty selection only.
        if super::recipe_fill::recipe_face_replaces_selection(
            operand.has_closed_edge_node()
                && feature_family == Some(crate::design::DesignFeatureFamily::Extrude)
                && (has_extrude_profile_group || has_extrude_face_group),
            operand.resolved_face_slots.is_empty(),
            !(has_extrude_profile_group && changed_faces.contains(&face)),
        ) {
            operand.resolved_face_slots = vec![face];
        }
    }
    if feature_family == Some(crate::design::DesignFeatureFamily::Extrude)
        && operand.resolved_face_slots.is_empty()
        && operand.resolved_active_face.is_none()
        && has_extrude_face_group
    {
        if let Some(face) = face_recipes
            .get(topology, super::face_recipe::FaceRecipeIndex::new)
            .resolve_loop(operand)
        {
            if super::recipe_fill::recipe_face_replaces_selection(
                operand.has_closed_edge_node(),
                true,
                !(has_extrude_profile_group && changed_faces.contains(&face)),
            ) {
                operand.resolved_face_slots = vec![face];
            }
        }
    }
    let alternate_start_face = alternate_start_face.filter(|face| {
        operand.resolved_face_slots.is_empty()
            && operand.resolved_active_face.is_none()
            && withdrawals.admits(
                operand,
                std::slice::from_ref(face),
                topology,
                topology_queries,
            )
    });
    if let Some(face) = alternate_start_face {
        let selected = vec![historical_face_id(face)?];
        operand.preceding_candidate_faces.clone_from(&selected);
        operand.changed_candidate_faces = if changed_faces.contains(&face) {
            selected.clone()
        } else {
            Vec::new()
        };
        operand.historical_support_contexts = super::face_support::historical_face_support_contexts(
            &selected,
            history,
            topology,
            &changed_faces,
            topology_queries,
        );
        operand.alternate_selector_candidate_faces = selected;
        operand.resolved_face_slots = vec![face];
    }

    if has_extrude_profile_group {
        super::recipe_fill::drop_unconsumed_closed_edge_profile(
            operand,
            &super::recipe_fill::Transition {
                state,
                previous_state_id,
                states,
            },
            super::recipe_fill::is_new_body_extrude(scope),
        );
    }
    Ok(())
}
