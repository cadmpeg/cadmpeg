// SPDX-License-Identifier: Apache-2.0
//! Parallel per-state topology measurement for bounded ASM histories inside
//! `bind_feature_outputs`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::body_closures::{bodies_touching, changed_topology_family_refs};
use super::historical_index::HistoricalBodyIndex;
use super::output_properties::{
    classify_face_plane, classify_inserted_face, EdgeMidpointRequest, FaceSurfaceIndex,
    InsertedFaceClassification, StateFacePlanes, StateSplitCandidates,
};
use super::topology_rows::KeyedRows;
use crate::history_records::AsmHistory;

pub(super) struct ParallelMeasureInputs<'a> {
    pub(super) history_index: usize,
    pub(super) history: &'a AsmHistory,
    pub(super) by_node: &'a HashMap<i64, usize>,
    pub(super) order: &'a [usize],
    pub(super) output_states: &'a HashSet<(usize, i64)>,
    pub(super) measured_states: &'a HashSet<(usize, i64)>,
    pub(super) workers: usize,
}

pub(super) type BodyExtent = (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Point3);

pub(super) struct ParallelMeasureTargets<'a> {
    pub(super) treatment_states: &'a mut HashMap<(usize, i64), Vec<EdgeMidpointRequest>>,
    pub(super) losses: &'a mut Vec<cadmpeg_ir::report::loss::LossNote>,
    pub(super) feature_edge_midpoints: &'a mut HashMap<usize, Vec<cadmpeg_ir::math::Point3>>,
    pub(super) state_deleted_bodies: &'a mut HashMap<(usize, i64), Vec<i64>>,
    pub(super) state_face_planes: &'a mut StateFacePlanes,
    pub(super) state_incomplete_body_centers: &'a mut HashMap<(usize, i64), BTreeSet<i64>>,
    pub(super) state_body_vertices:
        &'a mut HashMap<(usize, i64), BTreeMap<i64, Vec<cadmpeg_ir::math::Point3>>>,
    pub(super) state_body_centers:
        &'a mut HashMap<(usize, i64), Vec<(i64, cadmpeg_ir::math::Point3)>>,
    pub(super) state_exact_bodies: &'a mut HashMap<(usize, i64), BTreeSet<i64>>,
    pub(super) state_split_candidates: &'a mut HashMap<(usize, i64), StateSplitCandidates>,
    pub(super) state_body_bounds: &'a mut HashMap<(usize, i64), Vec<(i64, BodyExtent)>>,
    pub(super) state_present_bodies: &'a mut HashMap<(usize, i64), BTreeSet<i64>>,
    pub(super) state_solid_bodies: &'a mut HashMap<(usize, i64), BTreeSet<i64>>,
    pub(super) state_solid_outputs: &'a mut HashMap<(usize, i64), Vec<i64>>,
    pub(super) state_outputs: &'a mut HashMap<(usize, i64), Option<Vec<i64>>>,
}

struct MeasuredState {
    state_key: (usize, i64),
    state_index: usize,
    previous_index: Option<usize>,
    local_losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    local_midpoints: Vec<(usize, Vec<cadmpeg_ir::math::Point3>)>,
    whole_deleted: Vec<i64>,
    whole_deleted_candidates: Option<(i64, Vec<i64>)>,
    face_planes: BTreeMap<i64, (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)>,
    incomplete_centers: BTreeSet<i64>,
    vertices_map: BTreeMap<i64, Vec<cadmpeg_ir::math::Point3>>,
    centers: Vec<(i64, cadmpeg_ir::math::Point3)>,
    exact_bodies: BTreeSet<i64>,
    split_candidates: Option<StateSplitCandidates>,
    bounds_list: Vec<(i64, BodyExtent)>,
    present_bodies: Option<BTreeSet<i64>>,
    solid_bodies: Option<BTreeSet<i64>>,
    successor_bodies: Option<BTreeSet<i64>>,
    affected: Option<(i64, Option<i64>, BTreeSet<i64>, bool)>,
}

pub(super) fn measure_history_states_parallel(
    inputs: &ParallelMeasureInputs<'_>,
    targets: &mut ParallelMeasureTargets<'_>,
) {
    let ParallelMeasureInputs {
        history_index,
        history,
        order,
        workers,
        ..
    } = *inputs;

    let mut state_requests: Vec<Option<Vec<EdgeMidpointRequest>>> =
        (0..order.len()).map(|_| None).collect();
    for (pos, &idx) in order.iter().enumerate() {
        let state_key = (history_index, history.states()[idx].state_id);
        state_requests[pos] = targets.treatment_states.remove(&state_key);
    }
    let work_items: Vec<(usize, usize, Option<Vec<EdgeMidpointRequest>>)> = order
        .iter()
        .copied()
        .enumerate()
        .zip(state_requests)
        .map(|((pos, idx), reqs)| (pos, idx, reqs))
        .collect();
    let chunk_size = work_items.len().div_ceil(workers);
    let measured_chunks = crate::scoped::scope(|scope| {
        let mut iter = work_items.into_iter();
        let mut chunks_vec = Vec::new();
        loop {
            let chunk: Vec<_> = iter.by_ref().take(chunk_size).collect();
            if chunk.is_empty() {
                break;
            }
            chunks_vec.push(chunk);
        }
        let handles: Vec<_> = chunks_vec
            .into_iter()
            .map(|chunk| {
                scope.spawn(move || {
                    let mut chunk_out = Vec::with_capacity(chunk.len());
                    for (position, index, reqs) in chunk {
                        chunk_out.push(measure_one_state(inputs, position, index, reqs));
                    }
                    chunk_out
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect::<Vec<_>>()
    });

    let mut deferred_affected = Vec::new();
    let mut prev_successor: Option<(usize, Option<BTreeSet<i64>>)> = None;
    for item in measured_chunks {
        let walked_prev = prev_successor.take();
        prev_successor = Some((item.state_index, item.successor_bodies));
        let state_key = item.state_key;
        targets.losses.extend(item.local_losses);
        for (feature_index, midpoints) in item.local_midpoints {
            targets
                .feature_edge_midpoints
                .insert(feature_index, midpoints);
        }
        let whole_deleted = if let Some((prev_state_id, deleted)) = item.whole_deleted_candidates {
            if let Some(prev_solid) = targets
                .state_solid_bodies
                .get(&(history_index, prev_state_id))
            {
                deleted
                    .into_iter()
                    .filter(|b| prev_solid.contains(b))
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            item.whole_deleted
        };
        if !whole_deleted.is_empty() {
            targets
                .state_deleted_bodies
                .insert(state_key, whole_deleted);
        }
        if !item.face_planes.is_empty() {
            targets
                .state_face_planes
                .insert(state_key, item.face_planes);
        }
        if !item.incomplete_centers.is_empty() {
            targets
                .state_incomplete_body_centers
                .insert(state_key, item.incomplete_centers);
        }
        if !item.vertices_map.is_empty() {
            targets
                .state_body_vertices
                .insert(state_key, item.vertices_map);
        }
        if !item.centers.is_empty() {
            targets.state_body_centers.insert(state_key, item.centers);
            targets
                .state_exact_bodies
                .insert(state_key, item.exact_bodies);
        }
        if let Some(split) = item.split_candidates {
            targets.state_split_candidates.insert(state_key, split);
        }
        if !item.bounds_list.is_empty() {
            targets
                .state_body_bounds
                .insert(state_key, item.bounds_list);
        }
        if let Some(present) = item.present_bodies {
            targets.state_present_bodies.insert(state_key, present);
        }
        if let Some(solid) = item.solid_bodies {
            targets.state_solid_bodies.insert(state_key, solid);
        }
        if let Some((state_id, prev_state_id, mut affected, needs_prev_handoff)) = item.affected {
            let resolved = if needs_prev_handoff {
                match (item.previous_index, walked_prev) {
                    (Some(prev_idx), Some((walked_idx, Some(deleted))))
                        if walked_idx == prev_idx =>
                    {
                        affected.extend(deleted);
                        true
                    }
                    _ => false,
                }
            } else {
                true
            };
            if resolved {
                deferred_affected.push((
                    state_id,
                    prev_state_id,
                    affected.into_iter().collect::<Vec<_>>(),
                ));
            }
        }
    }
    for (state_id, prev_state_id, outputs) in deferred_affected {
        let prev_solid =
            prev_state_id.and_then(|pid| targets.state_solid_bodies.get(&(history_index, pid)));
        let cur_solid = targets.state_solid_bodies.get(&(history_index, state_id));
        let solid_affected: Vec<i64> = outputs
            .iter()
            .copied()
            .filter(|&b| {
                cur_solid.is_some_and(|cs| cs.contains(&b))
                    || prev_solid.is_some_and(|ps| ps.contains(&b))
            })
            .collect();
        if !solid_affected.is_empty() {
            targets
                .state_solid_outputs
                .insert((history_index, state_id), solid_affected);
        }
        targets
            .state_outputs
            .entry((history_index, state_id))
            .and_modify(|existing| *existing = None)
            .or_insert_with(|| Some(outputs));
    }
}

fn measure_one_state(
    inputs: &ParallelMeasureInputs<'_>,
    position: usize,
    index: usize,
    reqs: Option<Vec<EdgeMidpointRequest>>,
) -> MeasuredState {
    let ParallelMeasureInputs {
        history_index,
        history,
        by_node,
        order,
        output_states,
        measured_states,
        ..
    } = *inputs;
    let state = &history.states()[index];
    let cur_state_id = state.state_id;
    let state_key = (history_index, cur_state_id);
    let mut local_losses = Vec::new();
    let mut local_midpoints = Vec::new();
    let mut whole_deleted = Vec::new();
    let mut whole_deleted_candidates = None;
    let previous = state.next_ref.and_then(|node| by_node.get(&node).copied());
    if let Some(previous) = previous {
        if output_states.contains(&state_key) {
            if let Some(transition) = state.transition.as_ref() {
                if !transition.topology.bodies.deleted.is_empty() {
                    let prev_state_id = history.states()[previous].state_id;
                    if measured_states.contains(&(history_index, prev_state_id)) {
                        whole_deleted_candidates =
                            Some((prev_state_id, transition.topology.bodies.deleted.clone()));
                    } else if let Some(prev_topology) =
                        history.topology(&history.states()[previous])
                    {
                        let prev_index = HistoricalBodyIndex::new(prev_topology);
                        whole_deleted = transition
                            .topology
                            .bodies
                            .deleted
                            .iter()
                            .copied()
                            .filter(|&b| prev_index.is_solid(b))
                            .collect();
                    }
                }
            }
        }
    }
    let has_prev_handoff = position > 0
        && previous.is_some_and(|prev_idx| {
            order[position - 1] == prev_idx && {
                let prev_state = &history.states()[prev_idx];
                let prev_prev_id = prev_state
                    .next_ref
                    .and_then(|node| by_node.get(&node).copied())
                    .map(|pp| history.states()[pp].state_id);
                output_states.contains(&(history_index, prev_state.state_id))
                    && prev_state
                        .transition
                        .as_ref()
                        .is_some_and(|tr| tr.previous_state_id == prev_prev_id)
            }
        });
    let deleted_bodies = if has_prev_handoff {
        None
    } else {
        previous
            .filter(|_| output_states.contains(&state_key))
            .and_then(|previous| {
                let transition = state.transition.as_ref()?;
                bodies_touching(
                    history.topology(&history.states()[previous])?,
                    &[&changed_topology_family_refs(&transition.topology, true)],
                )
                .and_then(|touched| touched.into_iter().next())
            })
    };
    if let Some(requests) = reqs {
        let topology = history.topology(state);
        let edge_slots =
            topology.map(|topology| super::selection::FamilySlots::new(&topology.edges));
        let samples = topology.map(|topology| KeyedRows::new(&topology.edge_samples));
        for EdgeMidpointRequest {
            feature_index,
            slots,
        } in requests
        {
            let mut midpoints = Vec::new();
            for slot in slots {
                let sample = edge_slots
                    .as_ref()
                    .filter(|edge_slots| edge_slots.contains(slot))
                    .and_then(|_| samples.as_ref()?.get(slot));
                if let Some(sample) = sample {
                    midpoints.push(sample.interior);
                } else {
                    local_losses.push(
                        crate::loss::F3dLossCode::HistoricalEdgeMidpointsUnresolved.note(format!(
                            "historical edge slot {slot} has no carrier sample in bound input state {cur_state_id}"
                        )),
                    );
                }
            }
            if !midpoints.is_empty() {
                local_midpoints.push((feature_index, midpoints));
            }
        }
    }
    let mut face_planes = BTreeMap::new();
    let mut incomplete_centers = BTreeSet::new();
    let mut vertices_map = BTreeMap::new();
    let mut centers = Vec::new();
    let mut exact_bodies = BTreeSet::new();
    let mut split_candidates = None;
    let mut bounds_list = Vec::new();
    let mut present_bodies = None;
    let mut solid_bodies = None;
    if measured_states.contains(&state_key) {
        if let Some(topology) = history.topology(state) {
            let face_surfaces = FaceSurfaceIndex::new(topology);
            for &fid in &topology.faces {
                if let Some(plane) = classify_face_plane(&face_surfaces, fid) {
                    face_planes.insert(fid, plane);
                }
            }
            let body_index = HistoricalBodyIndex::new(topology);
            let solid_edges: BTreeMap<i64, Vec<i64>> = topology
                .bodies
                .iter()
                .filter_map(|&b| Some((b, body_index.solid_edges(b)?)))
                .collect();
            let mut center_bounds_list = Vec::new();
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
                    incomplete_centers.insert(b);
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
                            local_losses.push(
                                crate::loss::F3dLossCode::HistoricalBodyCenterPartial.note(format!(
                                    "historical solid body {b} uses partial center bounds in state {cur_state_id}: {detail}"
                                )),
                            );
                        }
                    }
                } else {
                    local_losses.push(
                        crate::loss::F3dLossCode::HistoricalBodyBoundsUnresolved.note(format!(
                            "historical solid body {b} has unresolved bounds in state {cur_state_id}"
                        )),
                    );
                }
            }
            if output_states.contains(&state_key) {
                centers = center_bounds_list
                    .iter()
                    .map(|&(b, (min_pt, max_pt))| {
                        (
                            b,
                            cadmpeg_ir::math::Point3::new(
                                (min_pt.x + max_pt.x) * 0.5,
                                (min_pt.y + max_pt.y) * 0.5,
                                (min_pt.z + max_pt.z) * 0.5,
                            ),
                        )
                    })
                    .collect();
                if let Some(transition) = state.transition.as_ref() {
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
                    split_candidates = Some(StateSplitCandidates {
                        all_faces_classified,
                        has_bodies,
                        all_bodies_complete,
                        inserted_planar_faces,
                        body_faces: body_faces_map,
                    });
                }
            }
            present_bodies = Some(topology.bodies.iter().copied().collect());
            solid_bodies = Some(solid_edges.into_keys().collect());
        }
    }
    let mut successor_bodies = None;
    let affected = (|| {
        let transition = state.transition.as_ref()?;
        if transition.previous_state_id != previous.map(|p| history.states()[p].state_id) {
            return None;
        }
        let topology = history.topology(state)?;
        if !output_states.contains(&state_key) {
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
        let [mut affected, walked_successor] = bodies_touching(
            topology,
            &[
                &changed_topology_family_refs(&transition.topology, false),
                &successor_deleted,
            ],
        )
        .and_then(|touched| <[_; 2]>::try_from(touched).ok())?;
        successor_bodies = Some(walked_successor);
        let needs_prev_handoff = if previous.is_some() {
            if has_prev_handoff {
                true
            } else {
                affected.extend(deleted_bodies?);
                false
            }
        } else {
            false
        };
        Some((
            cur_state_id,
            previous.map(|p| history.states()[p].state_id),
            affected,
            needs_prev_handoff,
        ))
    })();
    MeasuredState {
        state_key,
        state_index: index,
        previous_index: previous,
        local_losses,
        local_midpoints,
        whole_deleted,
        whole_deleted_candidates,
        face_planes,
        incomplete_centers,
        vertices_map,
        centers,
        exact_bodies,
        split_candidates,
        bounds_list,
        present_bodies,
        solid_bodies,
        successor_bodies,
        affected,
    }
}

pub(super) fn project_input_topologies_parallel(
    features: &[cadmpeg_ir::features::Feature],
    histories_ref: &[AsmHistory],
    jobs: Vec<(usize, usize, (usize, i64))>,
    workers: usize,
    include_geometry: bool,
) -> Vec<(
    super::input_topologies::ProjectedTopologySource,
    cadmpeg_ir::features::evaluation::FeatureInputTopology,
)> {
    let chunk_size = jobs.len().div_ceil(workers);
    crate::scoped::scope(|scope| {
        let mut iter = jobs.into_iter();
        let mut chunks = Vec::new();
        loop {
            let chunk: Vec<_> = iter.by_ref().take(chunk_size).collect();
            if chunk.is_empty() {
                break;
            }
            chunks.push(chunk);
        }
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| {
                scope.spawn(move || {
                    let mut out = Vec::with_capacity(chunk.len());
                    for (history_index, state_index, (feature_position, previous_state_id)) in chunk
                    {
                        let Some(history) = histories_ref.get(history_index) else {
                            continue;
                        };
                        let Some(state) = history.states().get(state_index) else {
                            continue;
                        };
                        let Some(feature) = features.get(feature_position) else {
                            continue;
                        };
                        if let Some(input) =
                            super::input_topologies::project_input_topology_with_geometry(
                                feature,
                                history,
                                state,
                                previous_state_id,
                                include_geometry,
                            )
                        {
                            out.push((
                                super::input_topologies::ProjectedTopologySource {
                                    feature_position,
                                    history_index,
                                    state_index,
                                    previous_state_id,
                                },
                                input,
                            ));
                        }
                    }
                    out
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect::<Vec<_>>()
    })
}
