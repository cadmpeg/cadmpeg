// SPDX-License-Identifier: Apache-2.0
use super::{
    effective_faces, face_boundary_contexts_for_slots, selection, stable_ref,
    AsmHistoricalTopology, SideOneEdge, TopologyQueryCache,
};

/// The one edge of `topology` that every face named by the first side of a
/// standard edge recipe bounds and that the recipe's candidate faces bound.
///
/// Each nonzero first-side value is a one-based prefix-reference ordinal;
/// every persistent name of a counted operand supplies that ordinal's faces.
/// Each operand must stand for exactly one face of the topology with complete
/// loops, and at least two distinct faces must be named. The edge is proven
/// when the intersection of their boundary edges with
/// `candidate_boundary_edges` holds one edge. When that intersection is empty
/// and the named faces share exactly one edge, that edge is also proven when
/// the recipe has two sides under root `2`, the names are not disjoint, and
/// the operation's transition deletes the edge (`transition_deleted_edges`).
/// Otherwise it is the best reading and is returned as inferred.
pub(super) fn side_one_reference_face_edge<'a>(
    structure: Option<&crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>,
    references: &[crate::records::dimensions::DesignRecipeReference],
    candidate_boundary_edges: &[i64],
    transition_deleted_edges: &[i64],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Option<SideOneEdge> {
    let structure = structure?;
    let side = structure.sides.first()?;
    let lanes = crate::records::dimensions::reference_lanes(references);
    let mut ordinals = Vec::new();
    for value in std::iter::once(side.header_value)
        .chain(side.scalars.iter().copied())
        .filter(|value| *value != 0)
    {
        ordinals.push(usize::try_from(value).ok()?.checked_sub(1)?);
    }
    ordinals.sort_unstable_by(Ord::cmp);
    ordinals.dedup();
    let mut faces = Vec::new();
    let mut disjoint_names = false;
    for ordinal in ordinals {
        let references = lanes.get(ordinal)?;
        disjoint_names |= super::recipe_index::reference_lane_faces(references).disjoint_names;
        let lane: Vec<cadmpeg_ir::ids::FaceId> = references
            .iter()
            .flat_map(|reference| effective_faces(reference).iter().cloned())
            .collect();
        if lane.is_empty() {
            return None;
        }
        let mut in_topology = selection::faces_in_topology(&lane, topology);
        in_topology.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        in_topology.dedup();
        let [face] = in_topology.as_slice() else {
            return None;
        };
        let face = stable_ref(face.as_str())?;
        if !faces.contains(&face) {
            faces.push(face);
        }
    }
    if faces.len() < 2 {
        return None;
    }
    let mut common: Vec<i64> = Vec::new();
    for (index, face) in faces.into_iter().enumerate() {
        let [boundary] = face_boundary_contexts_for_slots(&[face], topology, topology_queries)
            .try_into()
            .ok()?;
        if boundary.loops.is_empty() {
            return None;
        }
        let edges: Vec<i64> = boundary
            .loops
            .iter()
            .flat_map(|face_loop| face_loop.boundary.coedges().map(|row| row.edge_slot))
            .collect();
        if index == 0 {
            common = edges;
        } else {
            common.retain(|edge| edges.contains(edge));
        }
    }
    common.sort_unstable_by(Ord::cmp);
    common.dedup();
    let mut proven = common
        .iter()
        .copied()
        .filter(|edge| candidate_boundary_edges.contains(edge));
    match (proven.next(), proven.next(), common.as_slice()) {
        (Some(slot), None, _) => Some(SideOneEdge {
            slot,
            inferred: false,
        }),
        (None, _, [slot]) => Some(SideOneEdge {
            slot: *slot,
            inferred: disjoint_names
                || structure.root != 2
                || structure.sides.len() != 2
                || !transition_deleted_edges.contains(slot),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_records::{AsmHistoricalCoedge, AsmHistoricalRelation};

    #[test]
    fn deleted_singleton_support_edge_has_transition_proof() {
        let structure = serde_json::from_value(serde_json::json!({
            "root": 2,
            "sides": [
                {"field_count": 3, "header_value": 1, "scalars": [0, 2],
                 "payload_prefix": [0], "payload_entry_count": 0, "entries": []},
                {"field_count": 3, "header_value": 0, "scalars": [0, 0],
                 "payload_prefix": [0], "payload_entry_count": 0, "entries": []}
            ]
        }))
        .unwrap();
        let references = [10, 20].map(|face| {
            serde_json::from_value(serde_json::json!({
                "selector": 1, "selector_offset": face, "token": "1", "token_integer": 0,
                "token_offset": face + 4, "design_reference": 1,
                "design_reference_offset": face + 8,
                "candidate_faces": [crate::ids::brep_entity_id(face)]
            }))
            .unwrap()
        });
        for second_edges in [[31, 32], [30, 31]] {
            let mut topology = AsmHistoricalTopology {
                faces: vec![10, 20],
                edges: vec![30, 31, 32],
                ..Default::default()
            };
            for (face, edges) in [(10, [30, 31]), (20, second_edges)] {
                topology.face_loops.push(AsmHistoricalRelation {
                    owner_ref: face,
                    member_refs: vec![face + 100],
                });
                topology.loop_coedges.push(AsmHistoricalRelation {
                    owner_ref: face + 100,
                    member_refs: vec![face + 200, face + 201],
                });
                for (ordinal, edge) in edges.into_iter().enumerate() {
                    let coedge = face + 200 + i64::try_from(ordinal).unwrap();
                    let neighbour = face + 201 - i64::try_from(ordinal).unwrap();
                    topology.coedge_topology.push(AsmHistoricalCoedge {
                        coedge,
                        owner_loop: face + 100,
                        edge,
                        next: neighbour,
                        previous: neighbour,
                        radial_next: coedge,
                    });
                }
            }
            for (candidates, deleted, expected) in [
                (vec![30, 32], vec![], Some((31, true))),
                (vec![30, 32], vec![32], Some((31, true))),
                (vec![30, 32], vec![31], Some((31, false))),
                (vec![], vec![31], Some((31, false))),
            ] {
                let selected = side_one_reference_face_edge(
                    Some(&structure),
                    &references,
                    &candidates,
                    &deleted,
                    &topology,
                    &mut TopologyQueryCache::default(),
                )
                .map(|edge| (edge.slot, edge.inferred));
                let expected = if second_edges == [30, 31] {
                    if candidates.is_empty() {
                        None
                    } else {
                        Some((30, false))
                    }
                } else {
                    expected
                };
                assert_eq!(selected, expected);
                if second_edges == [31, 32] {
                    let mut one_side = structure.clone();
                    one_side.root = 1;
                    one_side.sides.truncate(1);
                    let selected = side_one_reference_face_edge(
                        Some(&one_side),
                        &references,
                        &[],
                        &deleted,
                        &topology,
                        &mut TopologyQueryCache::default(),
                    )
                    .map(|edge| (edge.slot, edge.inferred));
                    assert_eq!(selected, Some((31, true)));
                }
            }
        }
    }
}
