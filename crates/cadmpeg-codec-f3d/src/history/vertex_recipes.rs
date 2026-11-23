// SPDX-License-Identifier: Apache-2.0
//! Vertex recipe and edge treatment vertex history.

use super::body_closures::stable_ref;
use super::face_operand_history::bind_historical_recipe_reference_candidates;
use super::history_states::{
    bound_history_state_pair, effective_scope_previous_history_state_id, unique_history_state,
};
use super::recipe_index::RecipeTopologyCache;
use super::topology_cache::{TopologyQueryCache, VertexBoundaryIndex};
use super::WORK_POINT_POSITION_TOLERANCE;
use crate::history_records::{AsmHistoricalTopology, AsmHistory};
use std::collections::{HashMap, HashSet};

/// Resolve persistent vertex recipes in the last history-bearing feature state
/// that precedes their owning construction in authored timeline order.
pub(crate) fn bind_vertex_recipe_history(
    scopes: &mut [crate::records::feature::scope::DesignParameterScope],
    timelines: &[crate::records::entity_header::DesignFeatureTimeline],
    histories: &[AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut topology_queries = TopologyQueryCache::default();
    let source_ordinals =
        crate::design::feature_project::scope_order::authored_scope_ordinals_per_stream(
            scopes, timelines,
        )?;
    let mut input_states = HashMap::new();
    for scope in scopes.iter().filter(|scope| {
        matches!(
            scope.kind(),
            crate::records::feature::scope::DesignFeatureKind::WorkPlane
                | crate::records::feature::scope::DesignFeatureKind::WorkPoint
        )
    }) {
        let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
        let Some(&ordinal) = source_ordinals.get(&(stream, scope.record_index)) else {
            continue;
        };
        let mut predecessors = scopes.iter().filter_map(|candidate| {
            let candidate_stream =
                crate::ids::native_stream(&candidate.id).unwrap_or(crate::ids::DEFAULT_STREAM);
            let candidate_ordinal =
                *source_ordinals.get(&(candidate_stream, candidate.record_index))?;
            (candidate_stream == stream && candidate_ordinal < ordinal)
                .then_some((candidate_ordinal, candidate.history_state_id()?))
        });
        let Some(predecessor) = predecessors.next() else {
            continue;
        };
        let predecessor = predecessors.fold(predecessor, |latest, candidate| {
            if candidate.0 > latest.0 {
                candidate
            } else {
                latest
            }
        });
        let id = scope.id.clone();
        input_states.insert(id, predecessor.1);
    }

    for scope in scopes.iter_mut().filter(|scope| {
        scope.kind() == crate::records::feature::scope::DesignFeatureKind::WorkPoint
    }) {
        let state_id = input_states.get(&scope.id).copied();
        let Some(construction) = scope.work_point_construction_mut() else {
            continue;
        };
        let solved_position = cadmpeg_ir::math::Point3::new(
            construction.position[0].get() * 10.0,
            construction.position[1].get() * 10.0,
            construction.position[2].get() * 10.0,
        );
        for recipe in construction.rule.vertex_recipes_mut() {
            recipe.resolution = None;
            let Some(state_id) = state_id else {
                continue;
            };
            let Some((history, state)) = unique_history_state(histories, state_id) else {
                continue;
            };
            let Some(topology) = history.topology(state) else {
                continue;
            };
            let Some((vertex, position)) =
                vertex_recipe_candidate(recipe, topology, &mut topology_queries)
            else {
                continue;
            };
            if !point_matches(position, solved_position) {
                continue;
            }
            recipe.resolution = crate::records::feature::work_geometry::DesignVertexResolution::new(
                state_id, vertex,
            );
        }
    }

    for scope in scopes.iter_mut().filter(|scope| {
        scope.kind() == crate::records::feature::scope::DesignFeatureKind::WorkPlane
    }) {
        let transform = scope.work_plane_transform();
        let state_id = input_states.get(&scope.id).copied();
        let Some(construction) = scope.work_plane_construction_mut() else {
            continue;
        };
        construction.clear_resolution();
        let Some(state_id) = state_id else {
            continue;
        };
        let Some((history, state)) = unique_history_state(histories, state_id) else {
            continue;
        };
        let Some(topology) = history.topology(state) else {
            continue;
        };
        let [first, second, third] = construction.inputs();
        let (Some(first), Some(second), Some(third)) = (
            vertex_recipe_candidate(first, topology, &mut topology_queries),
            vertex_recipe_candidate(second, topology, &mut topology_queries),
            vertex_recipe_candidate(third, topology, &mut topology_queries),
        ) else {
            continue;
        };
        let candidates = [first, second, third];
        if !three_point_plane_matches(
            transform.map(crate::records::sketch_placement::SketchPlacementMatrix::rows),
            [first.1, second.1, third.1],
        ) {
            continue;
        }
        let [Some(first), Some(second), Some(third)] = candidates.map(|(vertex, _)| {
            crate::records::feature::work_geometry::DesignVertexResolution::new(state_id, vertex)
        }) else {
            continue;
        };
        construction
            .try_set_resolution([first, second, third])
            .map_err(cadmpeg_core::CodecError::malformed)?;
    }
    Ok(())
}

/// Resolve edge-treatment corner recipes in their bound feature-input state.
pub(crate) fn bind_edge_treatment_vertex_history(
    operands: &mut [crate::records::feature::work_geometry::DesignEdgeTreatmentVertexOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut topology_queries = TopologyQueryCache::default();
    let mut recipe_topologies = RecipeTopologyCache::default();
    for operand in operands {
        operand.recipe.resolution = None;
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
        let (Some(state_id), Some(previous_state_id)) = (
            scope.history_state_id(),
            effective_scope_previous_history_state_id(scope, histories),
        ) else {
            continue;
        };
        let Some((history, _, previous)) = bound_history_state_pair(
            &scope.id,
            state_id,
            previous_state_id,
            scope_histories,
            histories,
        ) else {
            continue;
        };
        let Some(topology) = history.topology(previous) else {
            continue;
        };
        for reference in &mut operand.recipe.recipe_references {
            bind_historical_recipe_reference_candidates(
                reference,
                topology,
                &mut recipe_topologies,
            )?;
        }
        let Some(vertex) =
            recipe_reference_common_vertex(&operand.recipe, topology, &mut topology_queries)
        else {
            continue;
        };
        operand.recipe.resolution =
            crate::records::feature::work_geometry::DesignVertexResolution::new(
                previous_state_id,
                vertex,
            );
    }
    Ok(())
}

pub(super) fn vertex_recipe_candidate<'a>(
    recipe: &crate::records::feature::work_geometry::DesignVertexRecipe,
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Option<(i64, cadmpeg_ir::math::Point3)> {
    let mut face_slots = Vec::new();
    for reference in &recipe.recipe_references {
        let mut slots = reference
            .candidate_faces
            .iter()
            .filter_map(|face| stable_ref(face.as_str()))
            .filter(|face| topology.faces.contains(face))
            .collect::<Vec<_>>();
        slots.sort_unstable_by(Ord::cmp);
        slots.dedup();
        let [slot] = slots.as_slice() else {
            return None;
        };

        face_slots.push(*slot);
    }
    if face_slots.is_empty() {
        return None;
    }
    let vertex = common_face_vertex(&face_slots, topology, topology_queries)?;
    unique_historical_vertex_position(vertex, topology).map(|position| (vertex, position))
}

pub(super) fn three_point_plane_matches(
    transform: Option<[[f64; 4]; 4]>,
    points: [cadmpeg_ir::math::Point3; 3],
) -> bool {
    use cadmpeg_ir::math::{Point3, Vector3};

    let Some(transform) = transform else {
        return false;
    };
    let origin = Point3::new(
        transform[0][3] * 10.0,
        transform[1][3] * 10.0,
        transform[2][3] * 10.0,
    );
    let Some(normal) = Vector3::new(transform[0][2], transform[1][2], transform[2][2]).unit()
    else {
        return false;
    };
    let Some(point_normal) = points[1]
        .vector_from(points[0])
        .cross(points[2].vector_from(points[0]))
        .unit()
    else {
        return false;
    };
    let scale = points
        .iter()
        .flat_map(|point| [point.x.abs(), point.y.abs(), point.z.abs()])
        .fold(1.0_f64, f64::max);
    (normal.dot(point_normal).abs() - 1.0).abs() <= WORK_POINT_POSITION_TOLERANCE
        && points.iter().all(|point| {
            point.vector_from(origin).dot(normal).abs() <= WORK_POINT_POSITION_TOLERANCE * scale
        })
}

pub(super) fn boundary_vertices_for_faces(
    faces: impl IntoIterator<Item = i64>,
    index: &VertexBoundaryIndex,
    edge_endpoints: &super::topology_cache::EdgeEndpoints,
) -> Option<HashSet<i64>> {
    let mut vertices = HashSet::new();
    for face in faces {
        let edges = index.boundaries.get(&face)?;
        for edge_slot in edges {
            let [start, end] = edge_endpoints.get(edge_slot).copied().flatten()?;
            if !index.vertices.contains(&start) || !index.vertices.contains(&end) {
                return None;
            }
            vertices.insert(start);
            vertices.insert(end);
        }
    }
    (!vertices.is_empty()).then_some(vertices)
}

pub(super) fn common_face_vertex<'a>(
    face_slots: &[i64],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Option<i64> {
    let index = topology_queries
        .vertices
        .get(topology, VertexBoundaryIndex::new);
    let edge_endpoints = topology_queries
        .endpoints
        .get(topology, super::topology_cache::endpoint_index);
    let mut common = None::<HashSet<i64>>;
    for face in face_slots {
        let vertices = boundary_vertices_for_faces(std::iter::once(*face), index, edge_endpoints)?;
        if let Some(common) = &mut common {
            common.retain(|vertex| vertices.contains(vertex));
        } else {
            common = Some(vertices);
        }
    }
    let common = common?;
    if common.len() == 1 {
        common.into_iter().next()
    } else {
        None
    }
}

pub(super) fn recipe_reference_common_vertex<'a>(
    recipe: &crate::records::feature::work_geometry::DesignVertexRecipe,
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Option<i64> {
    let index = topology_queries
        .vertices
        .get(topology, VertexBoundaryIndex::new);
    let edge_endpoints = topology_queries
        .endpoints
        .get(topology, super::topology_cache::endpoint_index);
    let mut common = None::<HashSet<i64>>;
    for reference in &recipe.recipe_references {
        let faces = reference
            .candidate_faces
            .iter()
            .filter_map(|face| stable_ref(face.as_str()))
            .filter(|face| index.faces.contains(face));
        let vertices = boundary_vertices_for_faces(faces, index, edge_endpoints)?;
        if let Some(common) = &mut common {
            common.retain(|vertex| vertices.contains(vertex));
        } else {
            common = Some(vertices);
        }
    }
    let common = common?;
    if common.len() == 1 {
        common.into_iter().next()
    } else {
        None
    }
}

pub(super) fn unique_historical_vertex_position(
    vertex: i64,
    topology: &AsmHistoricalTopology,
) -> Option<cadmpeg_ir::math::Point3> {
    let mut bindings = topology
        .vertex_points
        .iter()
        .filter(|binding| binding.entity == vertex);
    let point = bindings.next()?.carrier;
    if bindings.next().is_some() {
        return None;
    }
    let mut positions = topology
        .point_positions
        .iter()
        .filter(|position| position.point == point);
    let position = positions.next()?.position;
    (positions.next().is_none() && position.is_finite()).then_some(position)
}

pub(super) fn point_matches(
    left: cadmpeg_ir::math::Point3,
    right: cadmpeg_ir::math::Point3,
) -> bool {
    [(left.x, right.x), (left.y, right.y), (left.z, right.z)]
        .into_iter()
        .all(|(left, right)| {
            let scale = left.abs().max(right.abs()).max(1.0);
            (left - right).abs() <= WORK_POINT_POSITION_TOLERANCE * scale
        })
}
