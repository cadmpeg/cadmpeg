// SPDX-License-Identifier: Apache-2.0
//! Face and edge changes, face boundary contexts, and edge recipe reference contexts.

use super::body_closures::stable_ref;
use super::face_resolution::effective_faces;
use super::loop_index::LoopIndex;
use super::topology_cache::TopologyQueryCache;
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};
use std::collections::{BTreeMap, HashMap, HashSet};

pub(super) fn face_changes_across_state_chain<'a>(
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Option<HashSet<i64>> {
    let mut current = state;
    let mut visited = HashSet::new();
    let mut changed = HashSet::new();
    while current.state_id != previous_state_id {
        if !visited.insert(current.state_id) {
            return None;
        }
        let transition = current.transition.as_ref()?;
        for face in transition
            .topology
            .faces
            .deleted
            .iter()
            .chain(&transition.topology.faces.updated)
        {
            changed.insert(*face);
        }
        let previous = transition
            .previous_state_id
            .and_then(|id| states.get(&id).copied().flatten())?;
        current = previous;
    }
    Some(changed)
}

#[derive(Debug, PartialEq)]
pub(super) struct EdgeChanges {
    pub(super) deleted: HashSet<i64>,
    pub(super) updated: HashSet<i64>,
}

pub(super) fn edge_changes_across_state_chain<'a>(
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Option<EdgeChanges> {
    let mut current = state;
    let mut visited = HashSet::new();
    let mut deleted = HashSet::new();
    let mut updated = HashSet::new();
    while current.state_id != previous_state_id {
        if !visited.insert(current.state_id) {
            return None;
        }
        let transition = current.transition.as_ref()?;
        for edge in &transition.topology.edges.deleted {
            deleted.insert(*edge);
        }
        for edge in &transition.topology.edges.updated {
            updated.insert(*edge);
        }
        let previous = transition
            .previous_state_id
            .and_then(|id| states.get(&id).copied().flatten())?;
        current = previous;
    }
    Some(EdgeChanges { deleted, updated })
}

pub(super) fn face_operand_support_candidates<'a>(
    candidates: &'a [cadmpeg_ir::ids::FaceId],
    first_reference: Option<&crate::records::dimensions::DesignRecipeReference>,
    recipe_reference: Option<i32>,
) -> std::borrow::Cow<'a, [cadmpeg_ir::ids::FaceId]> {
    let mut candidates = std::borrow::Cow::Borrowed(candidates);
    for face in first_reference
        .filter(|reference| Some(reference.design_reference) == recipe_reference.map(i64::from))
        .into_iter()
        .flat_map(effective_faces)
    {
        if !candidates.contains(face) {
            candidates.to_mut().push(face.clone());
        }
    }
    candidates
}

pub(super) fn face_boundary_edges<'a>(
    faces: &[cadmpeg_ir::ids::FaceId],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Vec<i64> {
    let boundaries = topology_queries
        .boundaries
        .get(topology, super::topology_cache::FaceBoundaries::new);
    let mut edges = Vec::new();
    for face in faces.iter().filter_map(|face| stable_ref(face.as_str())) {
        boundaries.extend(topology, face, &mut edges);
    }
    edges.sort_unstable_by(Ord::cmp);
    edges.dedup();
    edges
}

pub(super) fn collect_reference_edge_sets<'a>(
    reference_faces: &[Vec<cadmpeg_ir::ids::FaceId>],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Vec<Vec<i64>> {
    let mut sets = Vec::new();
    for faces in reference_faces {
        let faces = super::selection::faces_in_topology(faces, topology);
        let edges = face_boundary_edges(&faces, topology, topology_queries);

        sets.push(edges);
    }
    sets
}

pub(super) fn face_boundary_contexts_for_slots<'a>(
    face_slots: &[i64],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Vec<crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext> {
    let index = topology_queries.loops.get(topology, LoopIndex::new);
    let edge_endpoints = topology_queries
        .endpoints
        .get(topology, super::topology_cache::endpoint_index);
    let mut contexts = Vec::new();
    'faces: for face_slot in face_slots {
        let Some(face_loops) = index.face_loops.get(face_slot).copied().flatten() else {
            continue;
        };
        let mut loops = Vec::new();
        for loop_slot in face_loops {
            let Some(loop_coedges) = index.loop_coedges.get(loop_slot).copied().flatten() else {
                continue 'faces;
            };
            let mut coedges = Vec::new();
            for coedge_slot in loop_coedges {
                let Some(edge_slot) = index.coedge_edges.get(coedge_slot).copied().flatten() else {
                    continue 'faces;
                };

                coedges.push(
                    crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
                        coedge_slot: *coedge_slot,
                        edge_slot,
                    },
                );
            }
            let boundary = historical_loop_boundary(coedges, index, edge_endpoints);

            loops.push(
                crate::records::topology::historical_context::DesignHistoricalFaceLoopContext {
                    loop_slot: *loop_slot,
                    boundary,
                },
            );
        }

        contexts.push(
            crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext {
                face_slot: *face_slot,
                loops,
            },
        );
    }
    contexts
}

pub(super) fn historical_loop_boundary(
    coedges: Vec<crate::records::topology::historical_context::DesignHistoricalLoopCoedge>,
    index: &LoopIndex<'_>,
    edge_endpoints: &super::topology_cache::EdgeEndpoints,
) -> crate::records::topology::historical_context::DesignHistoricalLoopBoundary {
    use crate::records::topology::{
        historical_context::DesignHistoricalLoopBoundary,
        historical_context::DesignHistoricalLoopPoint,
        historical_context::DesignHistoricalLoopPosition,
        historical_context::DesignHistoricalLoopVertex,
    };
    let mut vertices = Vec::new();
    for (ordinal, coedge) in coedges.iter().enumerate() {
        let previous = coedges[(ordinal + coedges.len() - 1) % coedges.len()].edge_slot;

        let endpoints = |slot| edge_endpoints.get(&slot).copied().flatten();
        let (Some(previous), Some(current)) = (endpoints(previous), endpoints(coedge.edge_slot))
        else {
            return DesignHistoricalLoopBoundary::Coedges(coedges);
        };
        let mut shared = previous
            .into_iter()
            .filter(|vertex| current.contains(vertex));
        let Some(vertex_slot) = shared.next() else {
            return DesignHistoricalLoopBoundary::Coedges(coedges);
        };
        if shared.any(|vertex| vertex != vertex_slot) {
            return DesignHistoricalLoopBoundary::Coedges(coedges);
        }

        vertices.push(DesignHistoricalLoopVertex {
            coedge: coedge.clone(),
            vertex_slot,
        });
    }
    if vertices.is_empty() {
        return DesignHistoricalLoopBoundary::Coedges(coedges);
    }
    let mut points = Vec::new();
    for vertex in &vertices {
        let Some(point_slot) = index
            .vertex_points
            .get(&vertex.vertex_slot)
            .copied()
            .flatten()
        else {
            return DesignHistoricalLoopBoundary::Vertices(vertices);
        };

        points.push(DesignHistoricalLoopPoint {
            vertex: vertex.clone(),
            point_slot,
        });
    }
    let mut positions = Vec::new();
    for point in &points {
        let Some(position) = index.positions.get(&point.point_slot).copied().flatten() else {
            return DesignHistoricalLoopBoundary::Points(points);
        };

        positions.push(DesignHistoricalLoopPosition {
            point: point.clone(),
            position,
        });
    }
    DesignHistoricalLoopBoundary::Positions(positions)
}

pub(super) fn preceding_support_face_slots(
    result_faces: &[cadmpeg_ir::ids::FaceId],
    result_topology: &AsmHistoricalTopology,
    preceding_topology: &AsmHistoricalTopology,
) -> Vec<i64> {
    let mut preceding_faces = HashSet::new();
    for face in &preceding_topology.faces {
        preceding_faces.insert(*face);
    }
    let mut support_faces = Vec::new();
    for result_face in result_faces {
        let Some(result_face) = stable_ref(result_face.as_str()) else {
            continue;
        };
        let mut result_bindings = result_topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == result_face);
        let Some(carrier) = result_bindings.next().map(|binding| binding.carrier) else {
            continue;
        };
        if result_bindings.next().is_some() {
            continue;
        }
        let mut preceding_bindings = preceding_topology.face_surfaces.iter().filter(|binding| {
            binding.carrier == carrier && preceding_faces.contains(&binding.entity)
        });
        let Some(preceding_face) = preceding_bindings.next().map(|binding| binding.entity) else {
            continue;
        };
        if preceding_bindings.next().is_none() && !support_faces.contains(&preceding_face) {
            support_faces.push(preceding_face);
        }
    }
    support_faces
}

#[derive(Clone, Copy)]
pub(super) struct EdgeBoundaryContext<'a, 'b> {
    pub(super) state: &'a AsmDeltaState,
    pub(super) topology: &'a AsmHistoricalTopology,
    pub(super) boundary_edges: &'b [i64],
}

/// Face boundaries named by edge-recipe reference contexts, stored once per
/// historical state and face slot.
#[derive(Default)]
pub(crate) struct FaceBoundaryArena {
    pub(super) records:
        BTreeMap<String, crate::records::topology::historical_context::AsmFaceBoundary>,
    /// Identities of faces whose loop topology is incomplete in their state.
    pub(super) incomplete: HashSet<String>,
}

impl FaceBoundaryArena {
    /// Return the boundary identities of the complete `face_slots` in the
    /// state, in slot order, and store each boundary not yet present.
    pub(super) fn intern<'a>(
        &mut self,
        state: &AsmDeltaState,
        topology: &'a AsmHistoricalTopology,
        face_slots: &[i64],
        topology_queries: &mut TopologyQueryCache<'a>,
    ) -> Vec<String> {
        let mut ids = Vec::new();
        for face_slot in face_slots {
            let Some(id) = crate::records::topology::historical_context::AsmFaceBoundary::identity(
                &state.id,
                state.byte_offset,
                *face_slot,
            ) else {
                continue;
            };
            if self.incomplete.contains(&id) {
                continue;
            }
            if !self.records.contains_key(&id) {
                let Some(boundary) = face_boundary_contexts_for_slots(
                    std::slice::from_ref(face_slot),
                    topology,
                    topology_queries,
                )
                .pop() else {
                    self.incomplete.insert(id);
                    continue;
                };
                self.records.insert(
                    id.clone(),
                    crate::records::topology::historical_context::AsmFaceBoundary {
                        id: id.clone(),
                        parent: state.id.clone(),
                        face_slot: boundary.face_slot,
                        loops: boundary.loops,
                    },
                );
            }
            ids.push(id);
        }
        ids
    }

    /// Store the boundaries of `other` not yet present and return their
    /// identities.
    pub(super) fn absorb(&mut self, other: Self) -> Vec<String> {
        self.incomplete.extend(other.incomplete);
        let mut added = Vec::new();
        for (id, boundary) in other.records {
            if let std::collections::btree_map::Entry::Vacant(entry) = self.records.entry(id) {
                added.push(entry.key().clone());
                entry.insert(boundary);
            }
        }
        added
    }

    /// Remove the boundaries `ids` names.
    pub(super) fn remove(&mut self, ids: &[String]) {
        for id in ids {
            self.records.remove(id);
        }
    }

    /// The stored boundaries in identity order.
    pub(crate) fn into_records(
        self,
    ) -> Vec<crate::records::topology::historical_context::AsmFaceBoundary> {
        self.records.into_values().collect()
    }
}

/// [`edge_recipe_reference_context_by`] with the union of each name's
/// Design references.
#[cfg(test)]
pub(super) fn edge_recipe_reference_context<'a>(
    reference_ordinal: u32,
    lane: &[crate::records::dimensions::DesignRecipeReference],
    result: EdgeBoundaryContext<'a, '_>,
    preceding: EdgeBoundaryContext<'a, '_>,
    changed_edges: &HashSet<i64>,
    topology_queries: &mut TopologyQueryCache<'a>,
    face_boundaries: &mut FaceBoundaryArena,
) -> crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
    edge_recipe_reference_context_by(
        reference_ordinal,
        (lane, super::recipe_index::NameReferences::Union),
        result,
        preceding,
        changed_edges,
        topology_queries,
        face_boundaries,
    )
}

pub(super) fn edge_recipe_reference_context_by<'a>(
    reference_ordinal: u32,
    (lane, names): (
        &[crate::records::dimensions::DesignRecipeReference],
        super::recipe_index::NameReferences,
    ),
    result: EdgeBoundaryContext<'a, '_>,
    preceding: EdgeBoundaryContext<'a, '_>,
    changed_edges: &HashSet<i64>,
    topology_queries: &mut TopologyQueryCache<'a>,
    face_boundaries: &mut FaceBoundaryArena,
) -> crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
    let candidate_faces = super::recipe_index::reference_lane_faces_by(lane, names);
    let candidate_faces = candidate_faces.faces.as_slice();
    let face_slots = |faces: &[cadmpeg_ir::ids::FaceId]| {
        faces
            .iter()
            .filter_map(|face| stable_ref(face.as_str()))
            .collect::<Vec<_>>()
    };
    let result_faces = super::selection::faces_in_topology(candidate_faces, result.topology);
    let result_face_boundary_ids = face_boundaries.intern(
        result.state,
        result.topology,
        &face_slots(&result_faces),
        topology_queries,
    );
    let result_edges = face_boundary_edges(&result_faces, result.topology, topology_queries);
    let result_shared_edge_slots = result
        .boundary_edges
        .iter()
        .copied()
        .filter(|edge| result_edges.contains(edge))
        .collect::<Vec<_>>();
    let preceding_faces = super::selection::faces_in_topology(candidate_faces, preceding.topology);
    let preceding_face_boundary_ids = face_boundaries.intern(
        preceding.state,
        preceding.topology,
        &face_slots(&preceding_faces),
        topology_queries,
    );
    let preceding_support_face_slots =
        preceding_support_face_slots(&result_faces, result.topology, preceding.topology);
    let preceding_support_face_boundary_ids = face_boundaries.intern(
        preceding.state,
        preceding.topology,
        &preceding_support_face_slots,
        topology_queries,
    );
    let preceding_edges =
        face_boundary_edges(&preceding_faces, preceding.topology, topology_queries);
    let shared_edge_slots = preceding
        .boundary_edges
        .iter()
        .copied()
        .filter(|edge| preceding_edges.contains(edge))
        .collect::<Vec<_>>();
    let changed_shared_edge_slots = shared_edge_slots
        .iter()
        .copied()
        .filter(|edge| changed_edges.contains(edge))
        .collect::<Vec<_>>();
    let mut support_edges = HashSet::new();
    for edge in preceding_support_face_boundary_ids
        .iter()
        .filter_map(|id| face_boundaries.records.get(id))
        .flat_map(|face| &face.loops)
        .flat_map(|face_loop| face_loop.boundary.coedges().map(|row| row.edge_slot))
    {
        support_edges.insert(edge);
    }
    let mut changed_reference_edge_slots = preceding_edges
        .iter()
        .copied()
        .chain(support_edges.iter().copied())
        .filter(|edge| changed_edges.contains(edge))
        .collect::<Vec<_>>();
    changed_reference_edge_slots.sort_unstable_by(Ord::cmp);
    changed_reference_edge_slots.dedup();
    crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
        reference_ordinal,
        result_faces,
        result_face_boundary_ids,
        result_shared_edge_slots,
        preceding_faces,
        preceding_face_boundary_ids,
        preceding_support_face_slots,
        preceding_support_face_boundary_ids,
        shared_edge_slots,
        changed_shared_edge_slots,
        changed_reference_edge_slots,
    }
}

/// The one candidate edge that every face set named by the first side of a
/// standard edge recipe bounds, with no incidence fallback.
pub(super) fn side_one_common_edge(
    structure: Option<&crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>,
    reference_contexts: &[crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext],
    candidate_edges: &[i64],
) -> Option<i64> {
    let side = structure.and_then(|structure| structure.sides.first())?;
    let mut ordinals = Vec::new();
    for value in std::iter::once(side.header_value)
        .chain(side.scalars.iter().copied())
        .filter(|value| *value != 0)
    {
        ordinals.push(usize::try_from(value).ok()?.checked_sub(1)?);
    }
    ordinals.sort_unstable_by(Ord::cmp);
    ordinals.dedup();
    if ordinals.len() < 2 {
        return None;
    }
    let mut candidates: Option<Vec<i64>> = None;
    for ordinal in ordinals {
        let context = reference_contexts.get(ordinal)?;
        if Some(context.reference_ordinal) != u32::try_from(ordinal).ok() {
            return None;
        }
        let edges = &context.shared_edge_slots;
        match &mut candidates {
            None => candidates = Some(edges.clone()),
            Some(candidates) => candidates.retain(|edge| edges.contains(edge)),
        }
    }
    let mut candidates = candidates?;
    candidates.retain(|edge| candidate_edges.contains(edge));
    candidates.sort_unstable_by(Ord::cmp);
    candidates.dedup();
    match candidates.as_slice() {
        [edge] => Some(*edge),
        _ => None,
    }
}

/// Resolve the unique candidate edge shared by the non-null face references
/// in the first side of a standard edge recipe.
pub(super) fn side_one_recipe_edge(
    structure: Option<&crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>,
    reference_contexts: &[crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext],
    selectors: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    candidate_edges: &[i64],
) -> Option<i64> {
    let side = structure.and_then(|structure| structure.sides.first())?;
    let mut ordinals = Vec::new();
    for value in std::iter::once(side.header_value)
        .chain(side.scalars.iter().copied())
        .filter(|value| *value != 0)
    {
        let ordinal = usize::try_from(value)
            .ok()
            .and_then(|value| value.checked_sub(1))?;

        ordinals.push(ordinal);
    }
    ordinals.sort_unstable_by(Ord::cmp);
    ordinals.dedup();
    let mut edge_sets = Vec::new();
    for ordinal in ordinals {
        let context = reference_contexts.get(ordinal)?;
        if Some(context.reference_ordinal) != u32::try_from(ordinal).ok() {
            return None;
        }

        edge_sets.push(context.shared_edge_slots.as_slice());
    }
    if edge_sets.iter().any(|edges| edges.is_empty()) {
        return None;
    }
    let first = edge_sets.first()?;
    let mut candidates = first.to_vec();
    for edges in edge_sets.iter().skip(1) {
        candidates.retain(|candidate| edges.contains(candidate));
    }
    candidates.retain(|candidate| candidate_edges.contains(candidate));
    candidates.sort_unstable_by(Ord::cmp);
    candidates.dedup();
    match candidates.as_slice() {
        [edge] => Some(*edge),
        _ => {
            crate::design::edge_resolve::resolved_edge_candidate_intersection(selectors, edge_sets)
        }
    }
}
