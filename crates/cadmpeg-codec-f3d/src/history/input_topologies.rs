// SPDX-License-Identifier: Apache-2.0
//! Projection of feature input topologies and edge geometry.

use super::binding_histories::{history_state_indexes, visit_states_oldest_first};
use super::history_states::{
    bound_scope_history, effective_scope_previous_history_state_id, unique_history_state,
    unique_history_state_pair,
};
use crate::history_records::{
    AsmDeltaState, AsmHistoricalRelation, AsmHistoricalTopology, AsmHistory,
};
use std::collections::{HashMap, HashSet};

pub(crate) fn project_feature_input_topologies(
    features: &[cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &mut [AsmHistory],
    edge_operands: &[crate::records::topology::edge_identity::DesignEdgeOperand],
    scope_histories: &HashMap<String, String>,
) -> Vec<cadmpeg_ir::features::evaluation::FeatureInputTopology> {
    let mut jobs = Vec::new();
    for (feature_position, feature) in features.iter().enumerate() {
        let histories = &*histories;
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let scoped_history = if scope_histories.contains_key(&scope.id) {
            let Some(history) = bound_scope_history(&scope.id, scope_histories, histories) else {
                continue;
            };
            Some(history)
        } else {
            None
        };
        let scoped_histories = scoped_history.map_or(histories, std::slice::from_ref);
        let Some(previous_state_id) = super::selection::feature_selected_input_state_id(feature)
            .or_else(|| scope.previous_history_state_id())
            .or_else(|| {
                crate::design::feature_project::work_points::work_point_recipe_state_id(
                    scope,
                    edge_operands,
                )
            })
            .or_else(|| crate::design::feature_project::work_plane_recipe_state_id(scope))
            .or_else(|| {
                crate::design::edge_resolve::edge_treatment_recipe_state_id(scope, edge_operands)
            })
            .or_else(|| effective_scope_previous_history_state_id(scope, scoped_histories))
        else {
            continue;
        };
        let Some((history, state)) = scope
            .history_state_id()
            .and_then(|state_id| {
                unique_history_state_pair(scoped_histories, state_id, previous_state_id)
                    .map(|(history, _, previous)| (history, previous))
            })
            .or_else(|| unique_history_state(scoped_histories, previous_state_id))
        else {
            continue;
        };
        let Some((history_index, state_index)) = history_state_indexes(histories, history, state)
        else {
            continue;
        };
        jobs.push((
            history_index,
            state_index,
            (feature_position, previous_state_id),
        ));
    }
    let workers = cadmpeg_core::parallelism().min(6);
    let keep_materialized = histories
        .iter()
        .all(|history| history.states().len() <= super::KEEP_MATERIALIZED_MAX_STATES);
    let mut projected = if keep_materialized && workers > 1 && jobs.len() >= 8 {
        super::output_measure::project_input_topologies_parallel(features, histories, jobs, workers)
    } else {
        let mut projected = Vec::new();
        visit_states_oldest_first(
            histories,
            jobs,
            |history, state, (feature_position, previous_state_id)| {
                let Some(feature) = features.get(feature_position) else {
                    return;
                };
                if let Some(input) =
                    project_input_topology(feature, history, state, previous_state_id)
                {
                    projected.push((feature_position, input));
                }
            },
        );
        projected
    };
    projected.sort_by_key(|(position, _)| *position);
    projected.into_iter().map(|(_, input)| input).collect()
}

pub(super) fn project_input_topology(
    feature: &cadmpeg_ir::features::Feature,
    history: &AsmHistory,
    state: &AsmDeltaState,
    previous_state_id: i64,
) -> Option<cadmpeg_ir::features::evaluation::FeatureInputTopology> {
    let topology = history.topology(state)?;
    let prefix = crate::ids::feature_input_prefix(&feature.id, previous_state_id);
    let bodies = project_input_members(&topology.bodies, |slot| {
        crate::ids::history_input_body_id(&prefix, slot)
    })?;
    let faces = project_input_members(&topology.faces, |slot| {
        crate::ids::history_input_face_id(&prefix, slot)
    })?;
    let edges = project_input_members(&topology.edges, |slot| {
        crate::ids::history_input_edge_id(&prefix, slot)
    })?;
    let vertices = project_input_members(&topology.vertices, |slot| {
        crate::ids::history_input_vertex_id(&prefix, slot)
    })?;
    let mut edge_geometry = project_input_edge_geometry(topology, &prefix);
    super::chamfer_supports::attach(feature, &history.id, topology, &prefix, &mut edge_geometry);
    let face_owners = project_input_face_owners(topology, &prefix);
    let face_geometry = project_input_face_geometry(topology, &prefix);
    let id = crate::ids::history_input_state_id(&prefix);
    let input_of = feature.id.clone();
    let native_ref = state.id.clone();

    Some(cadmpeg_ir::features::evaluation::FeatureInputTopology {
        id,
        input_of,
        bodies,
        faces,
        edges,
        vertices,
        edge_geometry,
        face_owners,
        face_geometry,
        body_writers: Vec::new(),
        native_ref: Some(native_ref),
    })
}

/// The body that holds each face of a state with more than one body.
///
/// A body holds a face through its regions and their shells. A face that no
/// shell lists, or that shells of two bodies list, has no entry. A state with
/// one body gives every face the same owner, so it states none.
fn project_input_face_owners(
    topology: &AsmHistoricalTopology,
    prefix: &cadmpeg_ir::ids::IdentityKey,
) -> Vec<cadmpeg_ir::features::evaluation::HistoricalFaceOwner> {
    fn members(relations: &[AsmHistoricalRelation]) -> HashMap<i64, &[i64]> {
        relations
            .iter()
            .map(|relation| (relation.owner_ref, relation.member_refs.as_slice()))
            .collect()
    }
    if topology.bodies.len() < 2 {
        return Vec::new();
    }
    let region_shells = members(&topology.region_shells);
    let shell_faces = members(&topology.shell_faces);
    let state_faces = topology.faces.iter().copied().collect::<HashSet<_>>();
    let mut owners = HashMap::<i64, Option<i64>>::new();
    for relation in &topology.body_regions {
        if !topology.bodies.contains(&relation.owner_ref) {
            continue;
        }
        for shell in relation
            .member_refs
            .iter()
            .filter_map(|region| region_shells.get(region))
            .flat_map(|shells| shells.iter())
        {
            for face in shell_faces
                .get(shell)
                .into_iter()
                .flat_map(|faces| faces.iter())
            {
                if !state_faces.contains(face) {
                    continue;
                }
                let owner = owners.entry(*face).or_insert(Some(relation.owner_ref));
                if *owner != Some(relation.owner_ref) {
                    *owner = None;
                }
            }
        }
    }
    let mut owned = owners
        .into_iter()
        .filter_map(|(face, body)| Some((face, body?)))
        .collect::<Vec<_>>();
    owned.sort_unstable();
    owned
        .into_iter()
        .map(
            |(face, body)| cadmpeg_ir::features::evaluation::HistoricalFaceOwner {
                face: crate::ids::history_input_face_id(prefix, face),
                body: crate::ids::history_input_body_id(prefix, body),
            },
        )
        .collect()
}

/// The carrier surface of each face of a state that has a face sample.
pub(super) fn project_input_face_geometry(
    topology: &AsmHistoricalTopology,
    prefix: &cadmpeg_ir::ids::IdentityKey,
) -> Vec<cadmpeg_ir::features::evaluation::HistoricalFaceGeometry> {
    if topology.face_samples.is_empty() {
        return Vec::new();
    }
    let samples = topology
        .face_samples
        .iter()
        .map(|sample| (sample.face, sample))
        .collect::<HashMap<_, _>>();
    topology
        .faces
        .iter()
        .filter_map(|slot| {
            let sample = samples.get(slot)?;
            Some(cadmpeg_ir::features::evaluation::HistoricalFaceGeometry {
                face: crate::ids::history_input_face_id(prefix, *slot),
                surface: sample.surface.clone(),
                reversed: sample.reversed,
            })
        })
        .collect()
}

/// Keep the carrier surfaces of the faces that some feature definition
/// selects. A consumer reads them to find a selected face in the bodies it
/// rebuilds.
pub(crate) fn retain_selected_face_geometry(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    input_topologies: &mut [cadmpeg_ir::features::evaluation::FeatureInputTopology],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::schema::rewrite::typed::RewriteIdentities;
    let sampled = input_topologies
        .iter()
        .flat_map(|topology| &topology.face_geometry)
        .map(|geometry| geometry.face.as_str())
        .collect::<HashSet<_>>();
    if sampled.is_empty() {
        return Ok(());
    }
    let mut selected = HashSet::<String>::new();
    for feature in features {
        feature
            .evaluation
            .definition()
            .visit_local_members(ctx, &mut |target| {
                if sampled.contains(target) && !selected.contains(target) {
                    selected.insert(target.to_owned());
                }
                Ok(())
            })?;
    }
    drop(sampled);
    for topology in input_topologies {
        topology
            .face_geometry
            .retain(|geometry| selected.contains(geometry.face.as_str()));
    }
    Ok(())
}

/// Keep the owners of the faces that some feature definition selects. A
/// consumer that rebuilds the state's bodies reads them to find the body a
/// selected face belongs to.
pub(crate) fn retain_selected_face_owners(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    input_topologies: &mut [cadmpeg_ir::features::evaluation::FeatureInputTopology],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::schema::rewrite::typed::RewriteIdentities;
    let owned = input_topologies
        .iter()
        .flat_map(|topology| &topology.face_owners)
        .map(|owner| owner.face.as_str())
        .collect::<HashSet<_>>();
    if owned.is_empty() {
        return Ok(());
    }
    let mut selected = HashSet::<String>::new();
    for feature in features {
        feature
            .evaluation
            .definition()
            .visit_local_members(ctx, &mut |target| {
                if owned.contains(target) && !selected.contains(target) {
                    selected.insert(target.to_owned());
                }
                Ok(())
            })?;
    }
    drop(owned);
    for topology in input_topologies {
        topology
            .face_owners
            .retain(|owner| selected.contains(owner.face.as_str()));
    }
    Ok(())
}

/// Keep the edge geometry of the historical edges that some feature
/// definition selects, and of the edges that bound a face some feature
/// definition selects; consumers locate those edges, and through them the
/// selected faces, in the bodies they rebuild.
pub(crate) fn retain_selected_edge_geometry(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    input_topologies: &mut [cadmpeg_ir::features::evaluation::FeatureInputTopology],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::schema::rewrite::typed::RewriteIdentities;
    let mut sampled = HashSet::new();
    for geometry in input_topologies
        .iter()
        .flat_map(|topology| &topology.edge_geometry)
    {
        sampled.insert(geometry.edge.as_str());
        sampled.extend(
            geometry
                .faces
                .iter()
                .map(cadmpeg_ir::ids::HistoricalFaceId::as_str),
        );
    }
    if sampled.is_empty() {
        return Ok(());
    }
    let mut selected = HashSet::<String>::new();
    for feature in features {
        feature
            .evaluation
            .definition()
            .visit_local_members(ctx, &mut |target| {
                if sampled.contains(target) && !selected.contains(target) {
                    selected.insert(target.to_owned());
                }
                Ok(())
            })?;
    }
    drop(sampled);
    for topology in input_topologies {
        topology.edge_geometry.retain(|geometry| {
            selected.contains(geometry.edge.as_str())
                || geometry
                    .faces
                    .iter()
                    .any(|face| selected.contains(face.as_str()))
        });
    }
    Ok(())
}

/// Project the sampled edges of one historical state with their vertex
/// positions and the state faces whose loops use them.
pub(super) fn project_input_edge_geometry(
    topology: &AsmHistoricalTopology,
    prefix: &cadmpeg_ir::ids::IdentityKey,
) -> Vec<cadmpeg_ir::features::evaluation::HistoricalEdgeGeometry> {
    if topology.edge_samples.is_empty() {
        return Vec::new();
    }
    let positions = topology
        .point_positions
        .iter()
        .map(|point| (point.point, point.position))
        .collect::<HashMap<_, _>>();
    let vertex_positions = topology
        .vertex_points
        .iter()
        .filter_map(|binding| Some((binding.entity, *positions.get(&binding.carrier)?)))
        .collect::<HashMap<_, _>>();
    let endpoints = topology
        .edge_vertices
        .iter()
        .map(|edge| (edge.edge, (edge.start_vertex, edge.end_vertex)))
        .collect::<HashMap<_, _>>();
    let state_faces = topology.faces.iter().copied().collect::<HashSet<_>>();
    let mut loop_faces = HashMap::new();
    for relation in &topology.face_loops {
        if state_faces.contains(&relation.owner_ref) {
            for loop_ in &relation.member_refs {
                loop_faces.insert(*loop_, relation.owner_ref);
            }
        }
    }
    let mut edge_faces = HashMap::<i64, Vec<i64>>::new();
    for coedge in &topology.coedge_topology {
        if let Some(face) = loop_faces.get(&coedge.owner_loop) {
            let faces = edge_faces.entry(coedge.edge).or_default();
            if !faces.contains(face) {
                faces.push(*face);
            }
        }
    }
    let samples = topology
        .edge_samples
        .iter()
        .map(|sample| (sample.edge, sample))
        .collect::<HashMap<_, _>>();
    let position = |vertex: i64| {
        vertex_positions
            .get(&vertex)
            .copied()
            .and_then(cadmpeg_ir::features::geometry::FinitePoint3::new)
    };
    topology
        .edges
        .iter()
        .filter_map(|slot| {
            let sample = samples.get(slot)?;
            let (start, end) = endpoints.get(slot).map_or((None, None), |(start, end)| {
                (position(*start), position(*end))
            });
            let mut faces = edge_faces.get(slot).cloned().unwrap_or_default();
            faces.sort_unstable();
            Some(cadmpeg_ir::features::evaluation::HistoricalEdgeGeometry {
                tangent: sample.tangent.and_then(cadmpeg_ir::units::UnitVector3::new),
                start,
                end,
                faces: project_input_members(&faces, |face| {
                    crate::ids::history_input_face_id(prefix, face)
                })?,
                carrier: sample.carrier.clone(),
                supports: Vec::new(),
                ..cadmpeg_ir::features::evaluation::HistoricalEdgeGeometry::new(
                    crate::ids::history_input_edge_id(prefix, *slot),
                    cadmpeg_ir::features::geometry::FinitePoint3::new(sample.interior)?,
                )
            })
        })
        .collect()
}

pub(super) fn project_input_members<T: Eq + std::hash::Hash>(
    slots: &[i64],
    id: impl FnMut(i64) -> T,
) -> Option<cadmpeg_ir::features::selections::DistinctMembers<T>> {
    let members = slots.iter().copied().map(id).collect::<Vec<_>>();
    cadmpeg_ir::features::selections::DistinctMembers::try_from(members).ok()
}
