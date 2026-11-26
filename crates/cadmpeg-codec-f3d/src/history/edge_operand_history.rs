// SPDX-License-Identifier: Apache-2.0
//! Edge operand history candidates, transitions, and active edge operands.

use super::binding_histories::{
    history_state_indexes, visit_state_groups_oldest_first, BindingHistories,
};
use super::body_closures::{same_axis_line, stable_ref};
use super::face_boundaries::{
    collect_reference_edge_sets, edge_changes_across_state_chain, edge_recipe_reference_context_by,
    face_boundary_edges, face_changes_across_state_chain, side_one_common_edge,
    side_one_recipe_edge, EdgeBoundaryContext, EdgeChanges, FaceBoundaryArena,
};
use super::face_operand_history::{
    bind_historical_recipe_reference_candidates, historical_recipe_faces,
};
use super::history_states::{
    bound_history_state_pair, effective_scope_previous_history_state_id, history_state_index,
};
use super::projection_was_finalized;
use super::recipe_index::RecipeTopologyCache;
use super::topology_cache::TopologyQueryCache;
use super::treatment_candidates::{
    terminal_edge_recipe_faces, terminal_edge_recipe_reference_faces, treatment_radius_candidates,
};
use super::treatment_index::TreatmentCache;
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology, AsmHistory};
use std::collections::{BTreeSet, HashMap, HashSet};

pub(crate) fn bind_edge_operand_history_candidates(
    operands: &mut [crate::records::topology::edge_identity::DesignEdgeOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    recipes: &[crate::records::recipes::ConstructionRecipe],
    histories: BindingHistories<'_>,
    scope_histories: &HashMap<String, String>,
) -> Result<
    Vec<crate::records::topology::historical_context::AsmFaceBoundary>,
    cadmpeg_core::CodecError,
> {
    bind_edge_operand_history_candidates_with_notes(
        operands,
        scopes,
        recipes,
        histories,
        scope_histories,
    )
    .map(|(face_boundaries, _)| face_boundaries)
}

/// [`bind_edge_operand_history_candidates`], with a
/// `history.edge-selection-inferred` note for each operand whose edge is
/// read from the faces its recipe names alone.
pub(crate) fn bind_edge_operand_history_candidates_with_notes(
    operands: &mut [crate::records::topology::edge_identity::DesignEdgeOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    recipes: &[crate::records::recipes::ConstructionRecipe],
    histories: BindingHistories<'_>,
    scope_histories: &HashMap<String, String>,
) -> Result<
    (
        Vec<crate::records::topology::historical_context::AsmFaceBoundary>,
        Vec<cadmpeg_ir::report::loss::LossNote>,
    ),
    cadmpeg_core::CodecError,
> {
    if projection_was_finalized(histories.get()) {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut recipe_record_indices = HashMap::new();
    for recipe in recipes {
        if let Some(index) = recipe.record_index {
            recipe_record_indices.insert(recipe.id.as_str(), index.value);
        }
    }
    let mut active = Vec::new();
    let mut jobs = Vec::new();
    for (position, operand) in operands.iter_mut().enumerate() {
        operand.result_candidate_faces.clear();
        operand.result_boundary_edge_slots.clear();
        operand.preceding_candidate_faces.clear();
        operand.terminal_candidate_faces.clear();
        operand.changed_candidate_faces.clear();
        operand.preceding_boundary_edge_slots.clear();
        operand.terminal_boundary_edge_slots.clear();
        operand.changed_boundary_edge_slots.clear();
        operand.deleted_boundary_edge_slots.clear();
        operand.transition_deleted_edge_slots.clear();
        operand.updated_boundary_edge_slots.clear();
        operand.treatment_radius_candidates.clear();
        operand.changed_boundary_edge_contexts.clear();
        operand.terminal_boundary_edge_contexts.clear();
        operand.recipe_reference_contexts.clear();
        operand.recipe_selectors.clear();
        operand.recipe_state_id = None;
        operand.resolved_edge_slot = None;
        operand.resolved_axis = None;
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
        let Some(state_id) = scope.history_state_id() else {
            active.push((position, scope));
            continue;
        };
        let Some(previous_state_id) =
            effective_scope_previous_history_state_id(scope, histories.get())
        else {
            active.push((position, scope));
            continue;
        };
        let Some((history, state, previous)) = bound_history_state_pair(
            &scope.id,
            state_id,
            previous_state_id,
            scope_histories,
            histories.get(),
        ) else {
            continue;
        };
        let (Some((history_index, state_index)), Some((_, previous_index))) = (
            history_state_indexes(histories.get(), history, state),
            history_state_indexes(histories.get(), history, previous),
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
    if !active.is_empty() {
        let terminal_topologies = terminal_topologies(histories.get());
        let mut topology_queries = TopologyQueryCache::default();
        for (position, scope) in active {
            let Some(operand) = operands.get_mut(position) else {
                continue;
            };
            if let Err(error) = bind_active_edge_operand_for_scope(
                operand,
                scope,
                &terminal_topologies,
                &mut topology_queries,
            ) {
                keep_first_error(&mut first_error, position, error);
            }
        }
    }
    let mut face_boundaries = FaceBoundaryArena::default();
    let mut inferred = Vec::new();
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
            let states = history_state_index(history);
            let mut caches = EdgeOperandCaches::default();
            for (position, scope, state_index) in group {
                let (Some(operand), Some(state)) = (
                    operands.get_mut(position),
                    history.states().get(state_index),
                ) else {
                    continue;
                };
                let Some(result_topology) = history.topology(state) else {
                    continue;
                };
                let transition = EdgeOperandTransition {
                    scope,
                    state,
                    previous,
                    result_topology,
                    topology,
                    states: &states,
                };
                match bind_historical_edge_operand(
                    operand,
                    &transition,
                    &recipe_record_indices,
                    &mut caches,
                    &mut face_boundaries,
                ) {
                    Ok(true) => {
                        if let Some(edge) = operand.resolved_edge_slot {
                            inferred.push((position, operand.id.clone(), previous.state_id, edge));
                        }
                    }
                    Ok(false) => {}
                    Err(error) => keep_first_error(&mut first_error, position, error),
                }
            }
        },
    );
    if let Some((_, error)) = first_error {
        return Err(error);
    }
    inferred.sort_by_key(|(position, ..)| *position);
    let notes = inferred
        .into_iter()
        .map(|(_, operand, state, edge)| inferred_edge_selection_note(&operand, state, edge))
        .collect();
    Ok((face_boundaries.into_records(), notes))
}

/// Keep the error of the earliest item, as a pass that stops at its first
/// failing item would return.
pub(super) fn keep_first_error(
    first: &mut Option<(usize, cadmpeg_core::CodecError)>,
    position: usize,
    error: cadmpeg_core::CodecError,
) {
    if first
        .as_ref()
        .is_none_or(|(earliest, _)| position < *earliest)
    {
        *first = Some((position, error));
    }
}

/// The topology of each history's one terminal state.
pub(super) fn terminal_topologies(histories: &[AsmHistory]) -> Vec<(i64, &AsmHistoricalTopology)> {
    histories
        .iter()
        .filter_map(|history| {
            let state = terminal_state(history)?;
            Some((state.state_id, history.topology(state)?))
        })
        .collect()
}

/// The one state of `history` that no bound transition names as its
/// predecessor.
pub(super) fn terminal_state(history: &AsmHistory) -> Option<&AsmDeltaState> {
    let preceding: HashSet<i64> = history
        .states()
        .iter()
        .filter_map(|state| {
            state
                .transition
                .as_ref()
                .and_then(|transition| transition.previous_state_id)
        })
        .collect();
    let mut terminals = history
        .states()
        .iter()
        .filter(|state| !preceding.contains(&state.state_id));
    let state = terminals.next()?;
    terminals.next().is_none().then_some(state)
}

/// One edge operand's scope and state pair with their topologies.
pub(super) struct EdgeOperandTransition<'a> {
    pub(super) scope: &'a crate::records::feature::scope::DesignParameterScope,
    pub(super) state: &'a AsmDeltaState,
    pub(super) previous: &'a AsmDeltaState,
    pub(super) result_topology: &'a AsmHistoricalTopology,
    pub(super) topology: &'a AsmHistoricalTopology,
    pub(super) states: &'a HashMap<i64, Option<&'a AsmDeltaState>>,
}

/// Indexes over the topologies of one previous state and its results.
#[derive(Default)]
pub(super) struct EdgeOperandCaches<'a> {
    pub(super) treatment: TreatmentCache<'a>,
    pub(super) topology_queries: TopologyQueryCache<'a>,
    pub(super) recipe_topologies: RecipeTopologyCache<'a>,
}

pub(super) fn bind_historical_edge_operand<'a>(
    operand: &mut crate::records::topology::edge_identity::DesignEdgeOperand,
    transition: &EdgeOperandTransition<'a>,
    recipe_record_indices: &HashMap<&str, i32>,
    caches: &mut EdgeOperandCaches<'a>,
    face_boundaries: &mut FaceBoundaryArena,
) -> Result<bool, cadmpeg_core::CodecError> {
    let &EdgeOperandTransition {
        scope,
        state,
        previous,
        result_topology,
        topology,
        states,
    } = transition;
    let previous_state_id = previous.state_id;
    let EdgeOperandCaches {
        treatment: treatment_cache,
        topology_queries,
        recipe_topologies,
    } = caches;
    for reference in &mut operand.recipe_references {
        bind_historical_recipe_reference_candidates(reference, topology, recipe_topologies)?;
    }
    if let Some(recipe_record_index) = recipe_record_indices.get(operand.recipe_id.as_str()) {
        operand.candidate_faces =
            historical_recipe_faces(i64::from(*recipe_record_index), topology, recipe_topologies)?;
    }
    let Some(changed_faces) = face_changes_across_state_chain(state, previous_state_id, states)
    else {
        return Ok(false);
    };
    let Some(EdgeChanges {
        deleted: chain_deleted_edges,
        updated: chain_updated_edges,
    }) = edge_changes_across_state_chain(state, previous_state_id, states)
    else {
        return Ok(false);
    };
    let preceding_faces = super::selection::FamilySlots::new(&topology.faces);
    let inserted_faces = result_topology
        .faces
        .iter()
        .copied()
        .filter(|face| !preceding_faces.contains(*face))
        .collect::<Vec<_>>();
    let result_edges = super::selection::FamilySlots::new(&result_topology.edges);
    let deleted_edges = topology
        .edges
        .iter()
        .copied()
        .filter(|edge| !result_edges.contains(*edge) && chain_deleted_edges.contains(edge))
        .collect::<Vec<_>>();
    let updated_edges = topology
        .edges
        .iter()
        .copied()
        .filter(|edge| {
            result_edges.contains(*edge)
                && (chain_deleted_edges.contains(edge) || chain_updated_edges.contains(edge))
        })
        .collect::<Vec<_>>();
    operand.recipe_state_id = Some(previous_state_id);
    let mut transition_deleted = deleted_edges.clone();
    transition_deleted.sort_unstable();
    operand.transition_deleted_edge_slots = transition_deleted;
    operand.result_candidate_faces =
        super::selection::faces_in_topology(&operand.candidate_faces, result_topology);
    operand.result_boundary_edge_slots = face_boundary_edges(
        &operand.result_candidate_faces,
        result_topology,
        topology_queries,
    );
    operand.preceding_candidate_faces =
        super::selection::faces_in_topology(&operand.candidate_faces, topology);
    operand.changed_candidate_faces = (operand.preceding_candidate_faces.iter().filter(|face| {
        stable_ref(face.as_str()).is_some_and(|slot| changed_faces.contains(&slot))
    }))
    .cloned()
    .collect::<Vec<_>>();
    operand.preceding_boundary_edge_slots = face_boundary_edges(
        &operand.preceding_candidate_faces,
        topology,
        topology_queries,
    );
    let mut changed_edges = HashSet::new();
    for edge in deleted_edges.iter().chain(&updated_edges) {
        changed_edges.insert(*edge);
    }
    operand.changed_boundary_edge_slots = operand
        .preceding_boundary_edge_slots
        .iter()
        .copied()
        .filter(|edge| changed_edges.contains(edge))
        .collect::<Vec<_>>();
    operand.deleted_boundary_edge_slots = super::selection::boundary_edges_in_changes(
        &operand.preceding_boundary_edge_slots,
        &deleted_edges,
    );
    operand.updated_boundary_edge_slots = super::selection::boundary_edges_in_changes(
        &operand.preceding_boundary_edge_slots,
        &updated_edges,
    );
    operand.treatment_radius_candidates = treatment_radius_candidates(
        Some(&operand.result_candidate_faces),
        &inserted_faces,
        result_topology,
        topology,
        &deleted_edges,
        treatment_cache,
    );
    let edge_contexts = topology_queries
        .edge_contexts
        .get(topology, super::selection::EdgeContextIndex::new);
    operand.changed_boundary_edge_contexts = operand
        .changed_boundary_edge_slots
        .iter()
        .map(|&edge| edge_contexts.context(edge))
        .collect::<Vec<_>>();
    // Contexts whose persistent names join their Design references as a
    // union, and contexts whose names select the faces common to all their
    // Design references.
    // Each set interns its boundaries apart; a boundary that no retained
    // context names leaves the arena.
    let mut reference_contexts = Vec::new();
    let mut common_reference_contexts = Vec::new();
    let mut union_boundaries = FaceBoundaryArena::default();
    let mut common_boundaries = FaceBoundaryArena::default();
    for (ordinal, lane) in crate::records::dimensions::reference_lanes(&operand.recipe_references)
        .into_iter()
        .enumerate()
    {
        let Ok(reference_ordinal) = u32::try_from(ordinal) else {
            continue;
        };
        for (names, contexts, boundaries) in [
            (
                super::recipe_index::NameReferences::Union,
                &mut reference_contexts,
                &mut union_boundaries,
            ),
            (
                super::recipe_index::NameReferences::Common,
                &mut common_reference_contexts,
                &mut common_boundaries,
            ),
        ] {
            contexts.push(edge_recipe_reference_context_by(
                reference_ordinal,
                (lane, names),
                EdgeBoundaryContext {
                    state,
                    topology: result_topology,
                    boundary_edges: &operand.result_boundary_edge_slots,
                },
                EdgeBoundaryContext {
                    state: previous,
                    topology,
                    boundary_edges: &operand.preceding_boundary_edge_slots,
                },
                &changed_edges,
                topology_queries,
                boundaries,
            ));
        }
    }
    let union_only_boundaries = face_boundaries.absorb(union_boundaries);
    operand.recipe_reference_contexts = reference_contexts;
    if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfacePatch
        && operand.surface_patch_recipe_structure.is_some()
    {
        operand.resolved_edge_slot = surface_patch_edge_operand_slot(
            operand.surface_patch_recipe_structure.as_ref(),
            &operand.recipe_references,
            topology,
            topology_queries,
        );
        return Ok(false);
    }
    if crate::design::design_feature_family(&scope.kind())
        == Some(crate::design::DesignFeatureFamily::Sweep)
    {
        let reference_faces = terminal_edge_recipe_reference_faces(
            &operand.recipe_references,
            operand.local_topology_references.as_deref(),
        )?;
        let reference_edge_sets =
            collect_reference_edge_sets(&reference_faces, topology, topology_queries);
        let mut candidate_edges = BTreeSet::new();
        for edge in reference_edge_sets.iter().flatten() {
            candidate_edges.insert(*edge);
        }
        let edge_contexts = topology_queries
            .edge_contexts
            .get(topology, super::selection::EdgeContextIndex::new);
        let contexts = candidate_edges
            .into_iter()
            .map(|edge| edge_contexts.context(edge))
            .collect::<Vec<_>>();
        operand.recipe_selectors = super::selection::recipe_selector_candidates(
            operand.recipe_structure.as_ref(),
            &contexts,
        );
        operand.resolved_edge_slot =
            crate::design::edge_resolve::unique_incidence_edge_shared_by_reference_faces(
                &operand.recipe_selectors,
                reference_edge_sets.iter().map(Vec::as_slice),
            );
        return Ok(false);
    }
    if crate::design::design_feature_family(&scope.kind())
        == Some(crate::design::DesignFeatureFamily::Revolve)
    {
        let reference_faces = terminal_edge_recipe_reference_faces(
            &operand.recipe_references,
            operand.local_topology_references.as_deref(),
        )?;
        let reference_edge_sets =
            collect_reference_edge_sets(&reference_faces, topology, topology_queries);
        let mut candidate_edges = BTreeSet::new();
        for edge in reference_edge_sets.iter().flatten() {
            candidate_edges.insert(*edge);
        }
        let edge_contexts = topology_queries
            .edge_contexts
            .get(topology, super::selection::EdgeContextIndex::new);
        let contexts = candidate_edges
            .into_iter()
            .map(|edge| edge_contexts.context(edge))
            .collect::<Vec<_>>();
        operand.recipe_selectors = super::selection::recipe_selector_candidates(
            operand.recipe_structure.as_ref(),
            &contexts,
        );
        operand.resolved_edge_slot =
            crate::design::edge_resolve::resolved_edge_candidate_intersection(
                &operand.recipe_selectors,
                reference_edge_sets.iter().map(Vec::as_slice),
            )
            .or_else(|| {
                side_one_recipe_edge(
                    operand.recipe_structure.as_ref(),
                    &operand.recipe_reference_contexts,
                    &operand.recipe_selectors,
                    &operand.preceding_boundary_edge_slots,
                )
            });
        let resolved_axis = if let Some(edge) = operand.resolved_edge_slot {
            historical_edge_axis(edge, topology)
                .and_then(|(origin, direction)| design_axis(origin, direction))
        } else {
            None
        };
        if let Some(axis) = resolved_axis {
            operand.resolved_axis = Some(axis);
        }
        return Ok(false);
    }
    let edge_contexts = topology_queries
        .edge_contexts
        .get(topology, super::selection::EdgeContextIndex::new);
    let changed_edge_contexts = topology
        .edges
        .iter()
        .copied()
        .filter(|edge| changed_edges.contains(edge))
        .map(|edge| edge_contexts.context(edge))
        .collect::<Vec<_>>();
    operand.recipe_selectors = super::selection::recipe_selector_candidates(
        operand.recipe_structure.as_ref(),
        &changed_edge_contexts,
    );
    // For a Fillet or Chamfer, the faces that carry every Design reference of
    // each name take precedence when they select an edge that the treatment
    // transition deletes; otherwise the union of the references applies.
    operand.resolved_edge_slot = matches!(
        crate::design::design_feature_family(&scope.kind()),
        Some(
            crate::design::DesignFeatureFamily::Fillet
                | crate::design::DesignFeatureFamily::Chamfer
        )
    )
    .then(|| {
        side_one_common_edge(
            operand.recipe_structure.as_ref(),
            &common_reference_contexts,
            &operand.preceding_boundary_edge_slots,
        )
    })
    .flatten()
    .filter(|edge| operand.transition_deleted_edge_slots.contains(edge));
    if operand.resolved_edge_slot.is_some() {
        operand.recipe_reference_contexts = common_reference_contexts;
        face_boundaries.remove(&union_only_boundaries);
        face_boundaries.absorb(common_boundaries);
    } else {
        operand.resolved_edge_slot = side_one_recipe_edge(
            operand.recipe_structure.as_ref(),
            &operand.recipe_reference_contexts,
            &operand.recipe_selectors,
            &operand.preceding_boundary_edge_slots,
        );
    }
    if operand.resolved_edge_slot.is_none()
        && operand.preceding_candidate_faces.is_empty()
        && matches!(
            crate::design::design_feature_family(&scope.kind()),
            Some(
                crate::design::DesignFeatureFamily::Fillet
                    | crate::design::DesignFeatureFamily::Chamfer
            )
        )
    {
        let preceding_edges = super::selection::FamilySlots::new(&topology.edges);
        operand.resolved_edge_slot = operand.recipe_structure.as_ref().and_then(|structure| {
            super::reference_edge::primary_reference_common_edge(structure, |ordinal| {
                let context = operand.recipe_reference_contexts.get(ordinal)?;
                if usize::try_from(context.reference_ordinal).ok()? != ordinal {
                    return None;
                }
                let [face] = context.preceding_faces.as_slice() else {
                    return None;
                };
                let face_slot = stable_ref(face.as_str())?;
                let [boundary_id] = context.preceding_face_boundary_ids.as_slice() else {
                    return None;
                };
                let boundary = face_boundaries.records.get(boundary_id)?;
                if boundary.face_slot != face_slot || boundary.loops.is_empty() {
                    return None;
                }
                let mut edges = Vec::new();
                for face_loop in &boundary.loops {
                    let before = edges.len();
                    for coedge in face_loop.boundary.coedges() {
                        if !preceding_edges.contains(coedge.edge_slot) {
                            return None;
                        }
                        edges.push(coedge.edge_slot);
                    }
                    if edges.len() == before {
                        return None;
                    }
                }
                Some((face_slot, edges))
            })
        });
    }
    if operand.resolved_edge_slot.is_none()
        && matches!(
            crate::design::design_feature_family(&scope.kind()),
            Some(
                crate::design::DesignFeatureFamily::Fillet
                    | crate::design::DesignFeatureFamily::Chamfer
            )
        )
    {
        if let Some(edge) = super::reference_face::side_one_reference_face_edge(
            operand.recipe_structure.as_ref(),
            &operand.recipe_references,
            &operand.preceding_boundary_edge_slots,
            &operand.transition_deleted_edge_slots,
            topology,
            topology_queries,
        ) {
            operand.resolved_edge_slot = Some(edge.slot);
            return Ok(edge.inferred);
        }
    }
    Ok(false)
}

/// The note for an edge operand whose edge the faces its recipe's first
/// side names select alone, off the recipe's candidate faces.
pub(crate) fn inferred_edge_selection_note(
    operand: &str,
    state: i64,
    edge: i64,
) -> cadmpeg_ir::report::loss::LossNote {
    crate::loss::F3dLossCode::HistoricalEdgeSelectionInferred.note(format!(
        "edge operand {operand} reads edge slot {edge} of history state {state} from the faces its recipe's first side names: they share only that edge. The edge bounds none of the recipe's candidate faces, so the reading assumes those candidates are incomplete."
    ))
}

/// An edge read from the faces a recipe's first side names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct SideOneEdge {
    pub(crate) slot: i64,
    /// Whether no proven rule selects the edge: it bounds none of the
    /// recipe's candidate faces and the operation's transition does not
    /// prove it, so the named faces alone select it.
    pub(crate) inferred: bool,
}

/// A resolved axis with finite origin and unit direction.
pub(super) fn design_axis(
    origin: cadmpeg_ir::math::Point3,
    direction: cadmpeg_ir::math::Vector3,
) -> Option<crate::records::feature::patterns::DesignAxis> {
    crate::records::feature::patterns::DesignAxis::from_parts(
        cadmpeg_ir::features::geometry::FinitePoint3::new(origin)?,
        cadmpeg_ir::features::geometry::FiniteVector3::new(direction)?,
    )
}

pub(super) fn historical_edge_axis(
    edge: i64,
    topology: &AsmHistoricalTopology,
) -> Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)> {
    if let Some(axis) = topology
        .edge_curves
        .iter()
        .find(|binding| binding.entity == edge)
        .and_then(|binding| binding.carrier)
        .and_then(|curve| topology.curve_axes.iter().find(|axis| axis.curve == curve))
    {
        return Some((axis.origin, axis.direction));
    }
    let mut support_surfaces = HashSet::new();
    for carrier in super::selection::historical_edge_context(edge, topology)
        .incident_loops
        .into_iter()
        .filter_map(|context| {
            let mut bindings = topology
                .face_surfaces
                .iter()
                .filter(|binding| binding.entity == context.face_slot);
            let carrier = bindings.next()?.carrier;
            bindings.next().is_none().then_some(carrier)
        })
    {
        support_surfaces.insert(carrier);
    }
    let mut axes = topology
        .surface_axes
        .iter()
        .filter(|axis| support_surfaces.contains(&axis.surface))
        .map(|axis| (axis.origin, axis.direction));
    let first = axes.next()?;
    axes.all(|axis| same_axis_line(first, axis))
        .then_some(first)
}

pub(super) fn bind_active_edge_operand_for_scope<'a>(
    operand: &mut crate::records::topology::edge_identity::DesignEdgeOperand,
    scope: &crate::records::feature::scope::DesignParameterScope,
    terminal_topologies: &[(i64, &'a AsmHistoricalTopology)],
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<(), cadmpeg_core::CodecError> {
    bind_active_edge_operand_candidates(operand, terminal_topologies, topology_queries)?;
    if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfacePatch
        && operand.surface_patch_recipe_structure.is_some()
    {
        operand.recipe_state_id = None;
        operand.resolved_edge_slot = None;
        let mut matched = None;
        let mut ambiguous = false;
        for (state_id, topology) in terminal_topologies {
            if let Some(edge) = surface_patch_edge_operand_slot(
                operand.surface_patch_recipe_structure.as_ref(),
                &operand.recipe_references,
                topology,
                topology_queries,
            ) {
                if matched.is_some() {
                    ambiguous = true;
                    break;
                }
                matched = Some((*state_id, edge));
            }
        }
        if !ambiguous {
            if let Some((state_id, edge)) = matched {
                operand.recipe_state_id = Some(state_id);
                operand.resolved_edge_slot = Some(edge);
            }
        }
    }
    if crate::design::design_feature_family(&scope.kind())
        == Some(crate::design::DesignFeatureFamily::Revolve)
    {
        let topology = operand.recipe_state_id.and_then(|state_id| {
            terminal_topologies
                .iter()
                .find(|(candidate, _)| *candidate == state_id)
                .map(|(_, topology)| *topology)
        });
        let resolved_axis = if let Some((edge, topology)) = operand.resolved_edge_slot.zip(topology)
        {
            historical_edge_axis(edge, topology)
                .and_then(|(origin, direction)| design_axis(origin, direction))
        } else {
            None
        };
        if let Some(axis) = resolved_axis {
            operand.resolved_axis = Some(axis);
        }
    }
    Ok(())
}

pub(super) fn surface_patch_edge_operand_slot<'a>(
    structure: Option<&crate::records::topology::edge_recipe::DesignSurfacePatchRecipeStructure>,
    recipe_references: &[crate::records::dimensions::DesignRecipeReference],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Option<i64> {
    let structure = structure?;
    let [first, second] = &structure.clauses;
    let common_edge_reference = common_surface_patch_reference(
        first.edge_reference_ordinals,
        second.edge_reference_ordinals,
    )?;
    let common_face_reference = common_surface_patch_reference(
        first.face_reference_ordinals,
        second.face_reference_ordinals,
    )?;
    let edge_reference = usize::try_from(common_edge_reference)
        .ok()
        .and_then(|ordinal| recipe_references.get(ordinal))?;
    let face_reference = usize::try_from(common_face_reference)
        .ok()
        .and_then(|ordinal| recipe_references.get(ordinal))?;
    if edge_reference.candidate_edges.is_empty() {
        return None;
    }
    if face_reference.candidate_faces.is_empty()
        || face_reference
            .candidate_faces
            .iter()
            .collect::<HashSet<_>>()
            .len()
            != face_reference.candidate_faces.len()
    {
        return None;
    }
    let faces = super::selection::faces_in_topology(&face_reference.candidate_faces, topology);
    let face_boundary_edges = face_boundary_edges(&faces, topology, topology_queries);
    let mut candidates = edge_reference
        .candidate_edges
        .iter()
        .filter_map(|edge| stable_ref(edge.as_str()))
        .filter(|edge| face_boundary_edges.contains(edge))
        .collect::<Vec<_>>();
    candidates.sort_unstable_by(Ord::cmp);
    candidates.dedup();
    match candidates.as_slice() {
        [edge] => Some(*edge),
        _ => None,
    }
}

pub(super) fn common_surface_patch_reference(left: [u32; 2], right: [u32; 2]) -> Option<u32> {
    let mut common = left.into_iter().collect::<HashSet<_>>();
    common.retain(|reference| right.contains(reference));
    (common.len() == 1)
        .then(|| common.into_iter().next())
        .flatten()
}

/// The faces of one prefix operand: the exact-selector faces of every Design
/// reference of its names, or that reference's alternate-selector faces when
/// its exact set is empty.
pub(super) fn operand_terminal_faces(
    lane: &[crate::records::dimensions::DesignRecipeReference],
) -> Vec<cadmpeg_ir::ids::FaceId> {
    let mut faces = Vec::new();
    for reference in lane {
        let reference_faces = if reference.candidate_faces.is_empty() {
            &reference.alternate_selector_faces
        } else {
            &reference.candidate_faces
        };
        for face in reference_faces {
            if !faces.contains(face) {
                faces.push(face.clone());
            }
        }
    }
    faces
}

/// The one candidate-boundary edge that both edge sets of a zero-payload
/// standard recipe's two first-side ordinals hold.
pub(super) fn primary_side_shared_edge(
    structure: Option<&crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>,
    mut edges: impl FnMut(usize) -> Option<Vec<i64>>,
    boundary_edges: &[i64],
) -> Option<i64> {
    let structure = structure?;
    if structure.root != 2
        || structure.sides.len() != 2
        || structure.sides.iter().any(|side| {
            side.scalars.len() != 2 || side.payload_prefix != [0] || !side.entries.is_empty()
        })
    {
        return None;
    }
    let primary = &structure.sides[0];
    let mut ordinals = Vec::new();
    for value in std::iter::once(primary.header_value)
        .chain(primary.scalars.iter().copied())
        .filter(|value| *value != 0)
    {
        let ordinal = usize::try_from(value).ok()?.checked_sub(1)?;
        if !ordinals.contains(&ordinal) {
            ordinals.push(ordinal);
        }
    }
    let [first, second] = ordinals.as_slice() else {
        return None;
    };
    let first = edges(*first)?;
    let second = edges(*second)?;
    let mut candidate = None;
    for edge in first
        .iter()
        .copied()
        .filter(|edge| second.contains(edge) && boundary_edges.contains(edge))
    {
        if candidate.is_some_and(|selected| selected != edge) {
            return None;
        }
        candidate = Some(edge);
    }
    candidate
}

pub(super) fn bind_active_edge_operand_candidates<'a>(
    operand: &mut crate::records::topology::edge_identity::DesignEdgeOperand,
    topologies: &[(i64, &'a AsmHistoricalTopology)],
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut unique = None;
    for (state_id, topology) in topologies {
        let all_reference_faces =
            terminal_edge_recipe_reference_faces(&operand.recipe_references, None)?;
        let reference_faces = terminal_edge_recipe_reference_faces(
            &operand.recipe_references,
            operand.local_topology_references.as_deref(),
        )?;
        let terminal_faces = terminal_edge_recipe_faces(&operand.candidate_faces, &reference_faces);
        let candidate_faces = super::selection::faces_in_topology(&terminal_faces, topology);
        if topologies.len() != 1 && candidate_faces.is_empty() {
            continue;
        }
        let boundary_edges = face_boundary_edges(&candidate_faces, topology, topology_queries);
        let edge_contexts = topology_queries
            .edge_contexts
            .get(topology, super::selection::EdgeContextIndex::new);
        let contexts = boundary_edges
            .iter()
            .copied()
            .map(|edge| edge_contexts.context(edge))
            .collect::<Vec<_>>();
        let selectors = super::selection::recipe_selector_candidates(
            operand.recipe_structure.as_ref(),
            &contexts,
        );
        let reference_edge_sets =
            collect_reference_edge_sets(&reference_faces, topology, topology_queries);
        let all_reference_edge_sets =
            collect_reference_edge_sets(&all_reference_faces, topology, topology_queries);
        let mut edge = crate::design::edge_resolve::resolved_edge_candidate_intersection(
            &selectors,
            reference_edge_sets.iter().map(Vec::as_slice),
        );
        let reference_primary = primary_side_shared_edge(
            operand.recipe_structure.as_ref(),
            |ordinal| all_reference_edge_sets.get(ordinal).cloned(),
            &boundary_edges,
        );
        if edge.is_none() && reference_primary.is_none() {
            let lanes = crate::records::dimensions::reference_lanes(&operand.recipe_references);
            edge = primary_side_shared_edge(
                operand.recipe_structure.as_ref(),
                |ordinal| {
                    let faces = operand_terminal_faces(lanes.get(ordinal)?);
                    let faces = super::selection::faces_in_topology(&faces, topology);
                    Some(face_boundary_edges(&faces, topology, topology_queries))
                },
                &boundary_edges,
            );
        }
        if unique.is_some() {
            return Ok(());
        }
        unique = Some((
            *state_id,
            edge,
            candidate_faces,
            boundary_edges,
            contexts,
            all_reference_edge_sets,
            selectors,
        ));
    }
    let Some((
        state_id,
        edge,
        candidate_faces,
        boundary_edges,
        contexts,
        all_reference_edge_sets,
        selectors,
    )) = unique
    else {
        return Ok(());
    };
    operand.terminal_candidate_faces = candidate_faces;
    operand.terminal_boundary_edge_slots = boundary_edges;
    operand.terminal_boundary_edge_contexts = contexts;
    operand.terminal_reference_edge_slots = all_reference_edge_sets;
    operand.recipe_selectors = selectors;
    operand.recipe_state_id = Some(state_id);
    operand.resolved_edge_slot = edge;
    Ok(())
}
