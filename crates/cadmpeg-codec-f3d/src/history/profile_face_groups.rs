// SPDX-License-Identifier: Apache-2.0
//! Profile face group cardinality.

use super::cache::SnapshotCache;
use super::face_boundaries::face_changes_across_state_chain;
use super::face_operand_history::historical_face_id;
use super::history_states::{
    bound_history_state_pair, bound_scope_history, effective_scope_previous_history_state_id,
    history_state_index, unique_history_state_pair,
};
use crate::history_records::{AsmHistoricalTopology, AsmHistory};
use std::collections::{HashMap, HashSet};

pub(super) fn bind_profile_face_group_cardinality(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::topology::face::DesignFaceOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    operand_groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
    withdrawals: &mut super::fallback_face::Withdrawals,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut state_indexes = SnapshotCache::default();
    for scope in scopes {
        let scoped_history = if scope_histories.contains_key(&scope.id) {
            let Some(history) = bound_scope_history(&scope.id, scope_histories, histories) else {
                continue;
            };
            Some(history)
        } else {
            None
        };
        let scoped_histories = scoped_history.map_or(histories, std::slice::from_ref);
        let Some(profile_groups) = crate::design::face_resolve::extrude_profile_group_roots(
            decode,
            scope,
            operand_groups,
        )?
        else {
            continue;
        };
        for group in profile_groups {
            let Some(indices) = crate::design::face_resolve::extrude_profile_group_operand_indices(
                decode,
                group,
                operand_groups,
                operands,
            )?
            else {
                continue;
            };
            if group.members().len() != indices.len()
                || indices.iter().any(|index| {
                    let operand = &operands[*index];
                    !operand.resolved_face_slots.is_empty()
                        || operand.resolved_active_face.is_some()
                })
            {
                continue;
            }
            let (Some(state_id), Some(previous_state_id)) = (
                scope.history_state_id(),
                effective_scope_previous_history_state_id(scope, scoped_histories),
            ) else {
                continue;
            };
            let history_pair = if scoped_history.is_some() {
                bound_history_state_pair(
                    &scope.id,
                    state_id,
                    previous_state_id,
                    scope_histories,
                    histories,
                )
            } else {
                unique_history_state_pair(histories, state_id, previous_state_id)
            };
            let Some((history, state, previous)) = history_pair else {
                continue;
            };
            let states = state_indexes.get(history, history_state_index);
            let changed_faces = face_changes_across_state_chain(state, previous_state_id, states);
            let (Some(topology), Some(changed_faces)) = (history.topology(previous), changed_faces)
            else {
                continue;
            };
            let paired_aggregate = crate::design::face_resolve::is_paired_extrude_profile_aggregate(
                group,
                operand_groups,
                operands,
            );
            let faces =
                if paired_aggregate {
                    if let Some(transition) = state.transition.as_ref().filter(|transition| {
                        transition.previous_state_id == Some(previous_state_id)
                    }) {
                        let mut preceding_faces = HashSet::new();
                        for face in &topology.faces {
                            preceding_faces.insert(*face);
                        }
                        let mut deleted = transition.topology.faces.deleted.clone();
                        deleted.sort_unstable_by(Ord::cmp);
                        deleted.dedup();
                        (deleted.len() == group.members().len()
                            && deleted.len() == transition.topology.faces.deleted.len()
                            && deleted.iter().all(|face| {
                                preceding_faces.contains(face)
                                    && topology
                                        .face_surfaces
                                        .iter()
                                        .filter(|binding| binding.entity == *face)
                                        .count()
                                        == 1
                            }))
                        .then_some(deleted)
                    } else {
                        None
                    }
                } else {
                    profile_face_group_cardinality_candidates(
                        topology,
                        &changed_faces,
                        group.members().len(),
                    )
                };
            let Some(faces) = faces else {
                continue;
            };
            let mut topology_queries = super::topology_cache::TopologyQueryCache::default();
            let pairs: Vec<_> = indices
                .iter()
                .map(|index| &operands[*index])
                .zip(faces.iter().copied())
                .collect();
            if !withdrawals.admits_pairs(&pairs, topology, &mut topology_queries) {
                withdrawals.refuse(
                    &format!("Extrude profile group {}", group.id),
                    &format!("faces {faces:?} in stable face-slot member order"),
                    "profile-group carrier rule",
                );
                continue;
            }
            for (index, face) in indices.into_iter().zip(faces) {
                let face_id = historical_face_id(face)?;
                let preceding_id = face_id.clone();
                operands[index].preceding_candidate_faces =
                    std::iter::once(preceding_id).collect::<Vec<_>>();
                operands[index].changed_candidate_faces =
                    std::iter::once(face_id).collect::<Vec<_>>();
                operands[index].resolved_face_slots = vec![face];
            }
        }
    }
    Ok(())
}

pub(super) fn profile_face_group_cardinality_candidates(
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
    member_count: usize,
) -> Option<Vec<i64>> {
    let preceding_faces = super::selection::FamilySlots::new(&topology.faces);
    let mut faces_by_carrier = HashMap::<i64, Vec<i64>>::new();
    for face in changed_faces
        .iter()
        .copied()
        .filter(|face| preceding_faces.contains(*face))
    {
        let mut bindings = topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == face);
        let Some(carrier) = bindings.next().map(|binding| binding.carrier) else {
            continue;
        };
        if bindings.next().is_none() {
            faces_by_carrier.entry(carrier).or_default().push(face);
        }
    }
    let mut candidates = faces_by_carrier
        .into_values()
        .filter(|faces| faces.len() == member_count);
    let mut faces = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    faces.sort_unstable_by(Ord::cmp);
    faces.dedup();
    (faces.len() == member_count).then_some(faces)
}
