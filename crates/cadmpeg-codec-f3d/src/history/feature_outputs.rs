// SPDX-License-Identifier: Apache-2.0
//! Feature output binding, history states, and sweep result modes.

use super::binding_histories::history_state_indexes;
use super::body_closures::{bodies_touching, changed_topology_family_refs};
use super::face_resolution::historical_brep_source;
use super::historical_index::HistoricalBodyIndex;
use super::history_states::{
    bound_history_state_pair, effective_scope_previous_history_state_id, unique_history_state_pair,
};
use super::output_properties::{
    classify_face_plane, classify_inserted_face, collect_edge_midpoint_slots,
    feature_consumes_tool_body, feature_supports_body_absorption, resolve_split_result_plane,
    resolve_split_tool_plane, EdgeMidpointRequest, FaceSurfaceIndex, InsertedFaceClassification,
    SplitResultPlaneContext, StateFacePlanes, StateSplitCandidates,
    EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2, EPS_HISTORY_BODY_BBOX_GROWTH_E3,
    EPS_HISTORY_BODY_BBOX_UNCHANGED_E4, EPS_HISTORY_VERTEX_ABSORBED_DIST_SQ_E4,
};
use super::topology_rows::KeyedRows;
use crate::history_records::AsmHistory;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub(crate) struct BoundFeatureOutputs {
    pub(crate) losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    pub(crate) sweep_kinds: crate::design::feature_project::sweep_fallback::OutputFacts,
}

pub(crate) fn bind_feature_outputs(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &mut [AsmHistory],
    active_bodies: &[cadmpeg_ir::topology::Body],
    scope_histories: &HashMap<String, String>,
    primary_brep: Option<&str>,
) -> Result<BoundFeatureOutputs, cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::selections::EdgeSelection;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let mut losses = Vec::new();
    let mut sweep_kinds = crate::design::feature_project::sweep_fallback::OutputFacts::default();
    let mut inferred_histories = Vec::new();
    let mut datum_plane_frames = HashMap::<
        cadmpeg_ir::features::FeatureId,
        (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
    >::new();
    for feature in features.iter() {
        let op = match feature.evaluation.definition() {
            FeatureDefinition::Operation(op) => op,
            FeatureDefinition::PostProcess { operation, .. } => operation,
        };
        if let FeatureOperation::DatumPlane { frame } = op {
            if frame.normal().unit().is_some() {
                datum_plane_frames.insert(feature.id.clone(), (*frame.origin(), *frame.normal()));
            }
        }
    }
    let feature_states = features
        .iter()
        .map(|feature| feature_history_states(feature, scopes, histories))
        .collect::<Vec<_>>();
    let feature_sources = features
        .iter()
        .zip(&feature_states)
        .map(|(feature, states)| {
            let (state_id, previous_state_id) = (*states)?;
            let scope = feature.native_ref.as_deref()?;
            let (history, _, previous) = if scope_histories.contains_key(scope) {
                bound_history_state_pair(
                    scope,
                    state_id,
                    previous_state_id,
                    scope_histories,
                    histories,
                )
            } else if let Some(pair) =
                unique_history_state_pair(histories, state_id, previous_state_id)
            {
                Some(pair)
            } else {
                let pair = super::output_blob::output_blob_history_pair(
                    feature,
                    state_id,
                    previous_state_id,
                    histories,
                );
                if let Some((history, _, _)) = pair {
                    inferred_histories.push(crate::loss::F3dLossCode::HistoricalOutputBlobHistoryInferred.note(
                        format!(
                            "feature {}: its scope has no bound history and several histories hold its state pair; it reads {}, the one history of the BREP blob that holds its outputs.",
                            feature.id.as_str(),
                            history.id
                        ),
                    ));
                }
                pair
            }?;
            let (history_index, _) = history_state_indexes(histories, history, previous)?;
            Some(history_index)
        })
        .collect::<Vec<_>>();
    losses.append(&mut inferred_histories);
    let output_chains = features
        .iter()
        .enumerate()
        .map(|(index, feature)| {
            let (current, previous) = feature_states[index]?;
            super::output_chain::OutputChain::resolve(
                histories,
                current,
                previous,
                feature
                    .native_ref
                    .as_deref()
                    .and_then(|id| scope_histories.get(id))
                    .map(String::as_str)
                    .or_else(|| feature_sources[index].map(|source| histories[source].id.as_str())),
            )
        })
        .collect::<Vec<_>>();
    let output_states = feature_sources
        .iter()
        .zip(&feature_states)
        .filter_map(|(source, states)| Some(((*source)?, (*states)?.0)))
        .chain(output_chains.iter().flatten().flat_map(|chain| {
            chain
                .states
                .iter()
                .map(move |&state_id| (chain.history, state_id))
        }))
        .collect::<HashSet<_>>();
    let mut measured_states = output_states.clone();
    for (source, states) in feature_sources.iter().zip(&feature_states) {
        if let (Some(history_index), Some((_, previous_state_id))) = (*source, *states) {
            measured_states.insert((history_index, previous_state_id));
        }
    }
    measured_states.extend(
        histories
            .iter()
            .enumerate()
            .flat_map(|(history_index, history)| {
                history
                    .states()
                    .iter()
                    .map(move |state| (history_index, state))
            })
            .filter(|&(history_index, state)| {
                output_states.contains(&(history_index, state.state_id))
            })
            .filter_map(|(history_index, state)| {
                Some((history_index, state.transition.as_ref()?.previous_state_id?))
            }),
    );
    let mut treatment_states = HashMap::<(usize, i64), Vec<EdgeMidpointRequest>>::new();
    for (feature_index, feature) in features.iter().enumerate() {
        let Some((state_id, previous_state_id)) = feature_states[feature_index] else {
            continue;
        };
        let op = match feature.evaluation.definition() {
            FeatureDefinition::Operation(op) => op,
            FeatureDefinition::PostProcess { operation, .. } => operation,
        };
        let selections: Vec<&EdgeSelection> = match op {
            FeatureOperation::Fillet { groups } => groups.iter().map(|g| &g.edges).collect(),
            FeatureOperation::Chamfer { groups, .. } => groups.iter().map(|g| &g.edges).collect(),
            _ => continue,
        };
        let (slots, has_unresolved) =
            collect_edge_midpoint_slots(&feature.id, previous_state_id, &selections);
        if has_unresolved || slots.is_empty() {
            losses.push(
                crate::loss::F3dLossCode::HistoricalEdgeMidpointsUnresolved.note(format!(
                    "feature {} edge selection cannot resolve historical midpoints for state {state_id}",
                    feature.id
                )),
            );
        }
        if !slots.is_empty() {
            if let Some(history_index) = feature_sources[feature_index] {
                treatment_states
                    .entry((history_index, previous_state_id))
                    .or_default()
                    .push(EdgeMidpointRequest {
                        feature_index,
                        slots,
                    });
            } else {
                losses.push(
                    crate::loss::F3dLossCode::HistoricalEdgeMidpointsUnresolved.note(format!(
                        "feature {} edge selection has no bound input state {previous_state_id}",
                        feature.id
                    )),
                );
            }
        }
    }
    let mut feature_edge_midpoints = HashMap::<usize, Vec<cadmpeg_ir::math::Point3>>::new();
    let mut state_deleted_bodies = HashMap::<(usize, i64), Vec<i64>>::new();
    let mut state_solid_bodies = HashMap::<(usize, i64), BTreeSet<i64>>::new();
    // Every body slot of a measured state, solid or not: a body that a state still holds
    // is not deleted by the feature that produced the state.
    let mut state_present_bodies = HashMap::<(usize, i64), BTreeSet<i64>>::new();
    let mut state_body_centers =
        HashMap::<(usize, i64), Vec<(i64, cadmpeg_ir::math::Point3)>>::new();
    let mut state_incomplete_body_centers = HashMap::<(usize, i64), BTreeSet<i64>>::new();
    let mut state_exact_bodies = HashMap::<(usize, i64), BTreeSet<i64>>::new();
    let mut state_body_bounds = HashMap::<
        (usize, i64),
        Vec<(i64, (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Point3))>,
    >::new();
    let mut state_body_vertices =
        HashMap::<(usize, i64), BTreeMap<i64, Vec<cadmpeg_ir::math::Point3>>>::new();
    let mut state_face_planes = StateFacePlanes::new();
    let mut state_split_candidates = HashMap::<(usize, i64), StateSplitCandidates>::new();
    let mut state_solid_outputs = HashMap::<(usize, i64), Vec<i64>>::new();
    let mut state_outputs = HashMap::<(usize, i64), Option<Vec<i64>>>::new();
    for (history_index, history) in histories.iter_mut().enumerate() {
        let mut by_node = HashMap::new();
        for (index, state) in history.states().iter().enumerate() {
            by_node.insert(state.node_index, index);
        }
        if by_node.len() != history.states().len() {
            continue;
        }
        let order = history
            .states_oldest_first()
            .unwrap_or_else(|| (0..history.states().len()).collect());
        let workers = std::thread::available_parallelism()
            .map_or(1, std::num::NonZeroUsize::get)
            .min(6);
        let keep_materialized = history.states().len() <= super::KEEP_MATERIALIZED_MAX_STATES;
        if keep_materialized && workers > 1 && order.len() >= 8 {
            super::output_measure::measure_history_states_parallel(
                &super::output_measure::ParallelMeasureInputs {
                    history_index,
                    history,
                    by_node: &by_node,
                    order: &order,
                    output_states: &output_states,
                    measured_states: &measured_states,
                    workers,
                },
                &mut super::output_measure::ParallelMeasureTargets {
                    treatment_states: &mut treatment_states,
                    losses: &mut losses,
                    feature_edge_midpoints: &mut feature_edge_midpoints,
                    state_deleted_bodies: &mut state_deleted_bodies,
                    state_face_planes: &mut state_face_planes,
                    state_incomplete_body_centers: &mut state_incomplete_body_centers,
                    state_body_vertices: &mut state_body_vertices,
                    state_body_centers: &mut state_body_centers,
                    state_exact_bodies: &mut state_exact_bodies,
                    state_split_candidates: &mut state_split_candidates,
                    state_body_bounds: &mut state_body_bounds,
                    state_present_bodies: &mut state_present_bodies,
                    state_solid_bodies: &mut state_solid_bodies,
                    state_solid_outputs: &mut state_solid_outputs,
                    state_outputs: &mut state_outputs,
                },
            );
            continue;
        }
        let mut walked_deletions: Option<(usize, Option<BTreeSet<i64>>)> = None;
        for (position, &index) in order.iter().enumerate() {
            let state = &history.states()[index];
            let walked = walked_deletions.take();
            if let Some(Some(&previous)) = state.next_ref.map(|node| by_node.get(&node)) {
                if output_states.contains(&(history_index, state.state_id)) {
                    if let (Some(transition), Some(prev_topology)) = (
                        state.transition.as_ref(),
                        history.topology(&history.states()[previous]),
                    ) {
                        let prev_index = HistoricalBodyIndex::new(prev_topology);
                        let whole_deleted: Vec<i64> = transition
                            .topology
                            .bodies
                            .deleted
                            .iter()
                            .copied()
                            .filter(|&b| prev_index.is_solid(b))
                            .collect();
                        if !whole_deleted.is_empty() {
                            state_deleted_bodies
                                .insert((history_index, state.state_id), whole_deleted);
                        }
                    }
                }
            }
            let deleted_bodies = match state.next_ref.map(|node| by_node.get(&node)) {
                Some(Some(&previous))
                    if output_states.contains(&(history_index, state.state_id)) =>
                {
                    match walked.filter(|(walked, _)| *walked == previous) {
                        Some((_, bodies)) => bodies,
                        None => state
                            .transition
                            .as_ref()
                            .and_then(|transition| {
                                bodies_touching(
                                    history.topology(&history.states()[previous])?,
                                    &[&changed_topology_family_refs(&transition.topology, true)],
                                )
                            })
                            .and_then(|touched| touched.into_iter().next()),
                    }
                }
                _ => None,
            };
            history.advance_topology(index);
            let cur_state_id = history.states()[index].state_id;
            let state_key = (history_index, cur_state_id);
            if let Some(requests) = treatment_states.remove(&state_key) {
                let topology = history.topology(&history.states()[index]);
                let samples = topology.map(|topology| KeyedRows::new(&topology.edge_samples));
                for EdgeMidpointRequest {
                    feature_index,
                    slots,
                } in requests
                {
                    let mut midpoints = Vec::new();
                    for slot in slots {
                        let sample = topology
                            .filter(|topology| topology.edges.contains(&slot))
                            .and_then(|_| samples.as_ref()?.get(slot));
                        if let Some(sample) = sample {
                            midpoints.push(sample.interior);
                        } else {
                            losses.push(
                                crate::loss::F3dLossCode::HistoricalEdgeMidpointsUnresolved.note(
                                    format!(
                                        "historical edge slot {slot} has no carrier sample in bound input state {cur_state_id}"
                                    ),
                                ),
                            );
                        }
                    }
                    if !midpoints.is_empty() {
                        feature_edge_midpoints.insert(feature_index, midpoints);
                    }
                }
            }
            let measured_topology = measured_states
                .contains(&state_key)
                .then(|| history.topology(&history.states()[index]))
                .flatten();
            if let Some(topology) = measured_topology {
                let face_surfaces = FaceSurfaceIndex::new(topology);
                let mut face_planes = BTreeMap::new();
                for &fid in &topology.faces {
                    if let Some(plane) = classify_face_plane(&face_surfaces, fid) {
                        face_planes.insert(fid, plane);
                    }
                }
                if !face_planes.is_empty() {
                    state_face_planes.insert(state_key, face_planes);
                }
                let body_index = HistoricalBodyIndex::new(topology);
                let solid_edges: BTreeMap<i64, Vec<i64>> = topology
                    .bodies
                    .iter()
                    .filter_map(|&b| Some((b, body_index.solid_edges(b)?)))
                    .collect();
                let mut bounds_list = Vec::new();
                let mut center_bounds_list = Vec::new();
                let mut exact_bodies = BTreeSet::new();
                let mut vertices_map = BTreeMap::new();
                let mut body_faces_map = BTreeMap::new();
                for (&b, edges) in &solid_edges {
                    let evidence = body_index.body_bounds(b, edges);
                    let complete_topology = body_index
                        .has_complete_solid_topology(b)
                        .then(|| {
                            Some((
                                body_index.edge_vertex_positions(edges)?,
                                body_index.body_faces(b)?,
                            ))
                        })
                        .flatten();
                    let topology_complete = complete_topology.is_some();
                    if !topology_complete
                        || !evidence.missing_evidence.is_empty()
                        || evidence.bounds.is_none()
                    {
                        state_incomplete_body_centers
                            .entry(state_key)
                            .or_default()
                            .insert(b);
                    }
                    if let Some(bds) = evidence.bounds {
                        if let Some((vpos, faces)) = complete_topology {
                            vertices_map.insert(b, vpos);
                            bounds_list.push((b, bds));
                            body_faces_map.insert(b, faces);
                            if evidence.exact {
                                exact_bodies.insert(b);
                            }
                        }
                        if output_states.contains(&state_key) {
                            center_bounds_list.push((b, bds));
                            if !topology_complete || !evidence.missing_evidence.is_empty() {
                                let detail = if evidence.missing_evidence.is_empty() {
                                    "incomplete topology".to_owned()
                                } else {
                                    evidence.missing_evidence.join("; ")
                                };
                                losses.push(
                                    crate::loss::F3dLossCode::HistoricalBodyCenterPartial.note(format!(
                                        "historical solid body {b} uses partial center bounds in state {cur_state_id}: {detail}"
                                    )),
                                );
                            }
                        }
                    } else {
                        losses.push(
                            crate::loss::F3dLossCode::HistoricalBodyBoundsUnresolved.note(format!(
                                "historical solid body {b} has unresolved bounds in state {cur_state_id}"
                            )),
                        );
                    }
                }
                if !vertices_map.is_empty() {
                    state_body_vertices.insert(state_key, vertices_map);
                }
                if output_states.contains(&state_key) {
                    let centers: Vec<(i64, cadmpeg_ir::math::Point3)> = center_bounds_list
                        .iter()
                        .map(|&(b, (min_pt, max_pt))| {
                            let center = cadmpeg_ir::math::Point3::new(
                                (min_pt.x + max_pt.x) * 0.5,
                                (min_pt.y + max_pt.y) * 0.5,
                                (min_pt.z + max_pt.z) * 0.5,
                            );
                            (b, center)
                        })
                        .collect();
                    if !centers.is_empty() {
                        state_body_centers.insert(state_key, centers);
                        state_exact_bodies.insert(state_key, exact_bodies);
                    }
                    if let Some(transition) = history.states()[index].transition.as_ref() {
                        let mut inserted_planar_faces = Vec::new();
                        let mut all_faces_classified = true;
                        let has_bodies = !topology.bodies.is_empty()
                            || !transition.topology.bodies.inserted.is_empty()
                            || !transition.topology.bodies.updated.is_empty()
                            || !transition.topology.bodies.deleted.is_empty();
                        let all_bodies_complete = !topology.bodies.is_empty()
                            && solid_edges.len() == topology.bodies.len()
                            && bounds_list.len() == topology.bodies.len()
                            && transition
                                .topology
                                .bodies
                                .inserted
                                .iter()
                                .chain(&transition.topology.bodies.updated)
                                .all(|b| solid_edges.contains_key(b));
                        for &fid in &transition.topology.faces.inserted {
                            match classify_inserted_face(&face_surfaces, transition, fid) {
                                InsertedFaceClassification::Plane(origin, normal) => {
                                    inserted_planar_faces.push((fid, origin, normal));
                                }
                                InsertedFaceClassification::NonPlanar => {}
                                InsertedFaceClassification::Unclassifiable => {
                                    all_faces_classified = false;
                                    break;
                                }
                            }
                        }
                        state_split_candidates.insert(
                            state_key,
                            StateSplitCandidates {
                                all_faces_classified,
                                has_bodies,
                                all_bodies_complete,
                                inserted_planar_faces,
                                body_faces: body_faces_map,
                            },
                        );
                    }
                }
                if !bounds_list.is_empty() {
                    state_body_bounds.insert(state_key, bounds_list);
                }
                state_present_bodies.insert(state_key, topology.bodies.iter().copied().collect());
                state_solid_bodies.insert(state_key, solid_edges.into_keys().collect());
            }
            let mut next_deletions = None;
            let outputs = (|| {
                let state = &history.states()[index];
                let previous = match state.next_ref {
                    Some(node) => Some(*by_node.get(&node)?),
                    None => None,
                };
                let transition = state.transition.as_ref()?;
                if transition.previous_state_id
                    != previous.map(|index| history.states()[index].state_id)
                {
                    return None;
                }
                let topology = history.topology(state)?;
                if !output_states.contains(&(history_index, state.state_id)) {
                    return None;
                }
                let successor_deleted = order
                    .get(position + 1)
                    .map(|&next| &history.states()[next])
                    .filter(|next| {
                        next.next_ref.and_then(|node| by_node.get(&node).copied()) == Some(index)
                    })
                    .and_then(|next| next.transition.as_ref())
                    .map(|next| changed_topology_family_refs(&next.topology, true))
                    .unwrap_or_default();
                let Some([mut affected, successor_bodies]) = bodies_touching(
                    topology,
                    &[
                        &changed_topology_family_refs(&transition.topology, false),
                        &successor_deleted,
                    ],
                )
                .and_then(|touched| <[_; 2]>::try_from(touched).ok()) else {
                    next_deletions = Some((index, None));
                    return None;
                };
                next_deletions = Some((index, Some(successor_bodies)));
                if previous.is_some() {
                    affected.extend(deleted_bodies?);
                }
                let prev_solid = previous.and_then(|p| {
                    state_solid_bodies.get(&(history_index, history.states()[p].state_id))
                });
                let cur_solid = state_solid_bodies.get(&(history_index, state.state_id));
                let solid_affected: Vec<i64> = affected
                    .iter()
                    .copied()
                    .filter(|&b| {
                        cur_solid.is_some_and(|cs| cs.contains(&b))
                            || prev_solid.is_some_and(|ps| ps.contains(&b))
                    })
                    .collect();
                Some((
                    state.state_id,
                    affected.into_iter().collect::<Vec<_>>(),
                    solid_affected,
                ))
            })();
            walked_deletions = next_deletions;
            if !keep_materialized {
                history.retain_topology(index);
            }
            let Some((state_id, outputs, solid_affected)) = outputs else {
                continue;
            };
            if !solid_affected.is_empty() {
                state_solid_outputs.insert((history_index, state_id), solid_affected);
            }
            state_outputs
                .entry((history_index, state_id))
                .and_modify(|outputs| *outputs = None)
                .or_insert_with(|| Some(outputs));
        }
    }
    for ((_, state_id), requests) in treatment_states {
        for EdgeMidpointRequest { feature_index, .. } in requests {
            losses.push(
                crate::loss::F3dLossCode::HistoricalEdgeMidpointsUnresolved.note(format!(
                    "feature {} edge selection has no available input state {state_id}",
                    features[feature_index].id
                )),
            );
        }
    }
    let stored_active = super::stored_bodies::ActiveBodies::new(active_bodies, primary_brep);
    let mut ordered_indices: Vec<usize> = (0..features.len()).collect();
    ordered_indices.sort_by_key(|&i| features[i].ordinal);
    let mut prev_feature_solid = HashMap::<usize, &BTreeSet<i64>>::new();
    let mut absorbed_bodies = HashMap::<usize, BTreeSet<i64>>::new();
    for idx in ordered_indices {
        let feature = &mut features[idx];
        if feature
            .native_ref
            .as_deref()
            .and_then(|id| scopes.iter().find(|scope| scope.id == id))
            .and_then(|scope| scope.derived_base_source.as_ref())
            .is_some_and(|source| source.snapshot.is_some())
        {
            continue;
        }
        let Some((state_id, previous_state_id)) = feature_states[idx] else {
            continue;
        };
        let bound_history = feature_sources[idx];
        let history_source = bound_history.and_then(|h| historical_brep_source(&histories[h].id));
        let cur_key = bound_history.map(|h| (h, state_id));
        let prev_key = bound_history.map(|h| (h, previous_state_id));
        if bound_history.is_some_and(|h| {
            histories[h].states().iter().any(|state| {
                state.state_id == state_id
                    && state.transition.as_ref().is_some_and(|transition| {
                        transition.previous_state_id == Some(previous_state_id)
                            && transition.geometry_unchanged
                    })
            })
        }) {
            feature.source_properties.insert(
                cadmpeg_core::nonblank_literal!("transition_geometry"),
                "unchanged".into(),
            );
        }

        let prev_feat_solid = bound_history.and_then(|h| prev_feature_solid.get(&h).copied());
        let feature_scope = feature
            .native_ref
            .as_deref()
            .and_then(|id| scopes.iter().find(|scope| scope.id == id));
        let op_ref = match feature.evaluation.definition() {
            FeatureDefinition::Operation(op) => op,
            FeatureDefinition::PostProcess { operation, .. } => operation,
        };
        let mut newly_absorbed = BTreeSet::new();
        let before_bds = prev_key.and_then(|k| state_body_bounds.get(&k));
        let before_verts = prev_key.and_then(|k| state_body_vertices.get(&k));
        let after_verts = cur_key.and_then(|k| state_body_vertices.get(&k));
        let history_absorbed = bound_history.and_then(|h| absorbed_bodies.get(&h));
        if feature_supports_body_absorption(op_ref) {
            let solid_outputs = cur_key.and_then(|k| state_solid_outputs.get(&k));
            if let (Some(prev_bds), Some(cur_bds)) =
                (before_bds, cur_key.and_then(|k| state_body_bounds.get(&k)))
            {
                for (b1, (cmin, cmax)) in cur_bds {
                    if history_absorbed.is_some_and(|ab| ab.contains(b1))
                        || solid_outputs.is_some_and(|so| !so.contains(b1))
                    {
                        continue;
                    }
                    let Some((_, (pmin, pmax))) = prev_bds.iter().find(|(pb, _)| pb == b1) else {
                        continue;
                    };
                    let b1_grew = (cmax.x - cmin.x)
                        > (pmax.x - pmin.x) + EPS_HISTORY_BODY_BBOX_GROWTH_E3
                        || (cmax.y - cmin.y) > (pmax.y - pmin.y) + EPS_HISTORY_BODY_BBOX_GROWTH_E3
                        || (cmax.z - cmin.z) > (pmax.z - pmin.z) + EPS_HISTORY_BODY_BBOX_GROWTH_E3;
                    if b1_grew {
                        for (b2, (b2_min, b2_max)) in prev_bds {
                            if b2 == b1
                                || stored_active.contains_in_source(*b2, history_source)
                                || history_absorbed.is_some_and(|ab| ab.contains(b2))
                            {
                                continue;
                            }
                            let b2_unchanged = cur_bds.iter().find(|(cb, _)| cb == b2).is_some_and(
                                |(_, (c2_min, c2_max))| {
                                    (c2_min.x - b2_min.x).abs() < EPS_HISTORY_BODY_BBOX_UNCHANGED_E4
                                        && (c2_max.x - b2_max.x).abs()
                                            < EPS_HISTORY_BODY_BBOX_UNCHANGED_E4
                                        && (c2_min.y - b2_min.y).abs()
                                            < EPS_HISTORY_BODY_BBOX_UNCHANGED_E4
                                        && (c2_max.y - b2_max.y).abs()
                                            < EPS_HISTORY_BODY_BBOX_UNCHANGED_E4
                                        && (c2_min.z - b2_min.z).abs()
                                            < EPS_HISTORY_BODY_BBOX_UNCHANGED_E4
                                        && (c2_max.z - b2_max.z).abs()
                                            < EPS_HISTORY_BODY_BBOX_UNCHANGED_E4
                                },
                            );
                            let prev_covered = pmin.x
                                <= b2_min.x + EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                                && pmax.x >= b2_max.x - EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                                && pmin.y <= b2_min.y + EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                                && pmax.y >= b2_max.y - EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                                && pmin.z <= b2_min.z + EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                                && pmax.z >= b2_max.z - EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2;
                            let cur_covers = cmin.x
                                <= b2_min.x + EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                                && cmax.x >= b2_max.x - EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                                && cmin.y <= b2_min.y + EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                                && cmax.y >= b2_max.y - EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                                && cmin.z <= b2_min.z + EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                                && cmax.z >= b2_max.z - EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2;
                            let vertices_absorbed = match (
                                before_verts.and_then(|m| m.get(b1)),
                                after_verts.and_then(|m| m.get(b1)),
                                before_verts.and_then(|m| m.get(b2)),
                            ) {
                                (Some(b1_prev), Some(b1_cur), Some(b2_prev)) => {
                                    let close = |a: cadmpeg_ir::math::Point3,
                                                 b: cadmpeg_ir::math::Point3| {
                                        let dx = a.x - b.x;
                                        let dy = a.y - b.y;
                                        let dz = a.z - b.z;
                                        dx * dx + dy * dy + dz * dz
                                            < EPS_HISTORY_VERTEX_ABSORBED_DIST_SQ_E4
                                    };
                                    let b2_unique: Vec<_> = b2_prev
                                        .iter()
                                        .copied()
                                        .filter(|&v2| !b1_prev.iter().any(|&v1| close(v1, v2)))
                                        .collect();
                                    !b2_unique.is_empty()
                                        && b2_unique
                                            .iter()
                                            .all(|&v2| b1_cur.iter().any(|&vc| close(vc, v2)))
                                }
                                _ => false,
                            };
                            if b2_unchanged
                                && !prev_covered
                                && cur_covers
                                && vertices_absorbed
                                && feature_consumes_tool_body(
                                    op_ref,
                                    feature_scope,
                                    cur_key.and_then(|k| state_deleted_bodies.get(&k)),
                                    *b1,
                                    *b2,
                                )
                            {
                                newly_absorbed.insert(*b2);
                            }
                        }
                    }
                }
            }
        }
        let split_body_plane = if let FeatureOperation::SplitBody { targets, tool, .. } = op_ref {
            resolve_split_tool_plane(
                tool,
                &datum_plane_frames,
                &state_face_planes,
                bound_history,
                previous_state_id,
                state_id,
            )
            .or_else(|| {
                let cur_key = cur_key?;
                let prev_key = prev_key?;
                let candidates = state_split_candidates.get(&cur_key)?;
                let centers = state_body_centers
                    .get(&cur_key)
                    .map_or(&[][..], Vec::as_slice);
                let empty_vertices = BTreeMap::new();
                let vertices = state_body_vertices.get(&cur_key).unwrap_or(&empty_vertices);
                let solid_outputs = state_solid_outputs.get(&cur_key).map(Vec::as_slice);
                let after_solid = state_solid_bodies.get(&cur_key)?;
                let before_solid = state_solid_bodies.get(&prev_key).or(prev_feat_solid);
                let plane = resolve_split_result_plane(&SplitResultPlaneContext {
                    candidates,
                    centers,
                    vertices,
                    solid_outputs,
                    before_solid,
                    after_solid,
                    targets,
                })?;
                let detail = if !candidates.has_bodies && after_solid.is_empty() {
                    "coplanar inserted faces in bodyless state"
                } else if !candidates.all_bodies_complete || centers.len() < 2 {
                    "coplanar inserted faces with incomplete body evidence"
                } else {
                    "result faces and body geometry"
                };
                losses.push(
                    crate::loss::F3dLossCode::HistoricalSplitPlaneInferred.note(format!(
                        "SplitBody feature {} assumes its split_plane from {detail} in state {state_id}",
                        feature.id
                    )),
                );
                Some(plane)
            })
        } else {
            None
        };
        if let Some(midpoints) = feature_edge_midpoints.get(&idx) {
            let formatted = midpoints
                .iter()
                .map(|p| format!("{},{},{}", p.x, p.y, p.z))
                .collect::<Vec<_>>()
                .join(";");
            feature
                .source_properties
                .entry(cadmpeg_core::nonblank_literal!("edge_midpoints"))
                .or_insert(formatted);
        }
        if let Some(bodies) = cur_key.and_then(|k| state_incomplete_body_centers.get(&k)) {
            let unresolved = bodies
                .iter()
                .filter(|b| {
                    !history_absorbed.is_some_and(|ab| ab.contains(b))
                        && !newly_absorbed.contains(*b)
                })
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(";");
            if !unresolved.is_empty() {
                feature
                    .source_properties
                    .entry(cadmpeg_core::nonblank_literal!("body_centers_incomplete"))
                    .or_insert(unresolved);
            }
        }
        if let Some(centers) = cur_key.and_then(|k| state_body_centers.get(&k)) {
            let formatted = centers
                .iter()
                .filter(|(b, _)| {
                    !history_absorbed.is_some_and(|ab| ab.contains(b))
                        && !newly_absorbed.contains(b)
                })
                .map(|(b, c)| format!("{b}:{},{},{}", c.x, c.y, c.z))
                .collect::<Vec<_>>()
                .join(";");
            if !formatted.is_empty() {
                feature
                    .source_properties
                    .entry(cadmpeg_core::nonblank_literal!("body_centers"))
                    .or_insert(formatted);
            }
            let exact = cur_key
                .and_then(|k| state_exact_bodies.get(&k))
                .into_iter()
                .flatten()
                .filter(|b| {
                    !history_absorbed.is_some_and(|ab| ab.contains(b))
                        && !newly_absorbed.contains(*b)
                        && centers.iter().any(|(body, _)| body == *b)
                })
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            if !exact.is_empty() {
                feature
                    .source_properties
                    .entry(cadmpeg_core::nonblank_literal!("body_centers_exact"))
                    .or_insert(exact);
            }
        }
        if let Some((o, n)) = split_body_plane {
            feature
                .source_properties
                .entry(cadmpeg_core::nonblank_literal!("split_plane"))
                .or_insert_with(|| format!("{},{},{};{},{},{}", o.x, o.y, o.z, n.x, n.y, n.z));
        }
        let before_solid = prev_key
            .and_then(|k| state_solid_bodies.get(&k))
            .or(prev_feat_solid);
        let after_solid = cur_key.and_then(|k| state_solid_bodies.get(&k));
        let mut deleted_set = BTreeSet::new();
        let after_present = cur_key.and_then(|k| state_present_bodies.get(&k));
        let still_present = |b: &i64| after_present.is_some_and(|present| present.contains(b));
        if let (Some(before), Some(after)) = (before_solid, after_solid) {
            for &b in before.difference(after) {
                if !still_present(&b) {
                    deleted_set.insert(b);
                }
            }
        } else if let Some(del) = cur_key.and_then(|k| state_deleted_bodies.get(&k)) {
            for &b in del {
                deleted_set.insert(b);
            }
        }
        if let (Some(prev_feat), Some(after)) = (prev_feat_solid, after_solid) {
            for &b in prev_feat.difference(after) {
                if !still_present(&b) {
                    deleted_set.insert(b);
                }
            }
        }
        deleted_set.extend(newly_absorbed.iter().copied());
        if let (Some(h), Some(after)) = (bound_history, after_solid) {
            prev_feature_solid.insert(h, after);
        }
        if !deleted_set.is_empty() {
            let formatted = deleted_set
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            feature
                .source_properties
                .entry(cadmpeg_core::nonblank_literal!("deleted_bodies"))
                .or_insert(formatted);
        }
        if deleted_set.is_empty() && !combine_states_applied_operation(feature, scopes) {
            let mut assumed_cut = false;
            feature.evaluation.edit(|def, _| {
                if let FeatureDefinition::Operation(FeatureOperation::Combine {
                    op,
                    keep_tools,
                    ..
                })
                | FeatureDefinition::PostProcess {
                    operation: FeatureOperation::Combine { op, keep_tools, .. },
                    ..
                } = def
                {
                    if *op == cadmpeg_ir::features::operation_settings::BooleanKind::Join
                        && *keep_tools
                    {
                        *op = cadmpeg_ir::features::operation_settings::BooleanKind::Cut;
                        assumed_cut = true;
                    }
                }
            });
            if assumed_cut {
                losses.push(crate::loss::F3dLossCode::FeatureCombineKeptToolsCutAssumed.note(
                    format!("feature {} assumes Cut for a stored Join with kept tools and no deleted bodies", feature.id),
                ));
            }
        }
        if let Some(solid_outputs) = cur_key.and_then(|k| state_solid_outputs.get(&k)) {
            let mut filtered: Vec<i64> = if let (Some(before), Some(after)) =
                (before_solid, after_solid)
            {
                solid_outputs
                    .iter()
                    .copied()
                    .filter(|b| {
                        !history_absorbed.is_some_and(|ab| ab.contains(b))
                            && (before.contains(b) || after.contains(b) || deleted_set.contains(b))
                    })
                    .collect()
            } else {
                solid_outputs
                    .iter()
                    .copied()
                    .filter(|b| !history_absorbed.is_some_and(|ab| ab.contains(b)))
                    .collect()
            };
            if let (Some(before), Some(after)) = (before_solid, after_solid) {
                for b in after
                    .difference(before)
                    .chain(before.difference(after))
                    .chain(deleted_set.iter())
                {
                    if !history_absorbed.is_some_and(|ab| ab.contains(b)) && !filtered.contains(b) {
                        filtered.push(*b);
                    }
                }
                filtered.sort_unstable();
            }
            if !filtered.is_empty() {
                let formatted = filtered
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                feature
                    .source_properties
                    .entry(cadmpeg_core::nonblank_literal!("affected_bodies"))
                    .or_insert(formatted);
            }
        }
        if let Some(h) = bound_history {
            let entry = absorbed_bodies.entry(h).or_default();
            if let (Some(before), Some(after)) = (before_solid, after_solid) {
                for &b in after.difference(before) {
                    entry.remove(&b);
                }
            }
            entry.extend(newly_absorbed);
        }
        if state_id == previous_state_id {
            continue;
        }
        let chain_outputs = output_chains
            .get(idx)
            .and_then(Option::as_ref)
            .and_then(|chain| Some((chain.history, chain.outputs(&state_outputs)?)));
        let Some((history_index, outputs)) = chain_outputs else {
            losses.push(crate::loss::F3dLossCode::FeatureOutputStateChainUnbound.note(format!(
                "{} has no complete unique output transition chain from state {previous_state_id} to state {state_id}.",
                feature.id
            )));
            continue;
        };
        let outputs = crate::records::feature::derived_base::derived_base_output_order(
            feature,
            scopes,
            history_index,
            &state_outputs,
            outputs,
        );
        if super::stored_bodies::bound_result_bodies(feature).is_some()
            || super::stored_bodies::has_projection_bound_base_feature_outputs(feature, scopes)
        {
            continue;
        }
        let source = historical_brep_source(&histories[history_index].id);
        let mut resolved = Vec::new();
        let mut saved_edges = Vec::new();
        for slot in &outputs {
            if let Some(body) = stored_active.resolve(&[*slot], source) {
                resolved.extend(body.iter().cloned());
            } else if stored_active.is_saved_edge(*slot, source) {
                saved_edges.push(slot.to_string());
            }
        }
        if resolved.len() + saved_edges.len() != outputs.len() {
            losses.push(crate::loss::F3dLossCode::FeatureOutputBodyUnbound.note(format!(
                "{} has {} affected body slot(s) without a unique active body in its BREP namespace.",
                feature.id, outputs.len() - resolved.len() - saved_edges.len()
            )));
        }
        if !saved_edges.is_empty() {
            feature.source_properties.insert(
                cadmpeg_core::nonblank_literal!("affected_saved_edges"),
                saved_edges.join(","),
            );
        }
        let complete_sweep_output = resolved.len() == outputs.len() && saved_edges.is_empty();
        feature.evaluation.set_outputs(
            cadmpeg_ir::features::selections::DistinctMembers::try_from(resolved)
                .map_err(cadmpeg_core::CodecError::from)?,
        );
        sweep_kinds.record(feature, active_bodies, complete_sweep_output);
        if let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|id| scopes.iter().find(|scope| scope.id == id))
        {
            if super::stored_bodies::bind_history_outputs(
                feature,
                scope,
                &outputs,
                source,
                &stored_active,
            ) {
                bind_base_feature_output_selection(feature)?;
            }
        }
    }
    let base_scopes = scopes
        .iter()
        .filter(|scope| {
            scope.kind() == crate::records::feature::scope::DesignFeatureKind::BaseFeature
        })
        .map(|scope| scope.id.as_str())
        .collect::<HashSet<_>>();
    for feature in features {
        let base_scope = feature
            .native_ref
            .as_deref()
            .and_then(|id| base_scopes.get(id).copied());
        super::stored_bodies::finalize_body_sets(feature, base_scope);
    }
    Ok(BoundFeatureOutputs {
        losses,
        sweep_kinds,
    })
}

pub(super) fn combine_states_applied_operation(
    feature: &cadmpeg_ir::features::Feature,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
) -> bool {
    feature
        .native_ref
        .as_deref()
        .and_then(|id| scopes.iter().find(|scope| scope.id == id))
        .and_then(|scope| scope.combine_operation())
        .is_some_and(|operation| {
            operation.form == crate::records::feature::combine::DesignCombineForm::OperandLists
        })
}

/// The history state a feature's scope produces and the state it follows.
pub(super) fn feature_history_states(
    feature: &cadmpeg_ir::features::Feature,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
) -> Option<(i64, i64)> {
    let scope = feature
        .native_ref
        .as_deref()
        .and_then(|id| scopes.iter().find(|scope| scope.id == id))?;
    if let Some(group) =
        crate::records::feature::derived_base::feature_derived_base_scopes(feature, scopes)
    {
        return crate::records::feature::derived_base::group_history_states(&group);
    }
    let state_id = scope.history_state_id()?;
    let previous_state_id = effective_scope_previous_history_state_id(scope, histories)?;
    Some((state_id, previous_state_id))
}

pub(super) fn bind_base_feature_output_selection(
    feature: &mut cadmpeg_ir::features::Feature,
) -> Result<(), cadmpeg_core::CodecError> {
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::BaseFeature {
            bodies: cadmpeg_ir::features::selections::BodySelection::Native(native),
        },
    ) = feature.evaluation.definition()
    else {
        return Ok(());
    };
    let mut selected = Vec::new();
    for body in feature.evaluation.outputs() {
        let id = body.clone();

        selected.push(id);
    }
    let bodies = cadmpeg_ir::features::selections::BodySelection::Resolved {
        bodies: cadmpeg_ir::features::selections::DistinctMembers::try_from(selected)
            .map_err(cadmpeg_core::CodecError::from)?,
        native: native.to_owned(),
    };
    feature
        .evaluation
        .set_definition(cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::BaseFeature { bodies },
        ));
    Ok(())
}

pub(crate) fn bind_sweep_result_modes(
    features: &mut [cadmpeg_ir::features::Feature],
    bodies: &[cadmpeg_ir::topology::Body],
) {
    use cadmpeg_ir::features::sweeps::{SweepMode, SweepShape};
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    use cadmpeg_ir::topology::BodyKind;

    let mut body_kinds = HashMap::new();
    for body in bodies {
        body_kinds.insert(&body.id, body.kind);
    }
    for feature in features {
        let FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: SweepShape::Unresolved { .. },
            ..
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        if feature.evaluation.outputs().is_empty() {
            continue;
        }
        let mut all_sheet = true;
        let mut all_solid = true;
        for output in feature.evaluation.outputs() {
            match body_kinds.get(output) {
                Some(BodyKind::Sheet) => all_solid = false,
                Some(BodyKind::Solid) => all_sheet = false,
                _ => {
                    all_sheet = false;
                    all_solid = false;
                }
            }
        }
        let mode = if all_sheet {
            SweepMode::Surface {}
        } else if all_solid {
            SweepMode::Solid {
                op: cadmpeg_ir::features::sweeps::SolidSweepOperation::NewBody,
            }
        } else {
            SweepMode::Unresolved {}
        };
        let mut solid_sections = Vec::new();
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) = definition
            else {
                return;
            };
            let SweepShape::Unresolved { section, sections } =
                std::mem::replace(shape, SweepShape::unresolved())
            else {
                return;
            };
            *shape = match mode {
                SweepMode::Solid { op } => {
                    solid_sections.extend(sections.into_iter().map(Into::into));
                    SweepShape::Solid {
                        op,
                        section: section.into(),
                        sections: solid_sections,
                    }
                }
                _ => SweepShape::sheet_sections(mode, section, sections),
            };
        });
    }
}
