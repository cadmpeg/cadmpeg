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
/// and the named faces share exactly one edge, that edge is the best reading
/// and is returned as inferred.
pub(super) fn side_one_reference_face_edge<'a>(
    structure: Option<&crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>,
    references: &[crate::records::dimensions::DesignRecipeReference],
    candidate_boundary_edges: &[i64],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Option<SideOneEdge> {
    let side = structure.and_then(|structure| structure.sides.first())?;
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
    for ordinal in ordinals {
        let lane: Vec<cadmpeg_ir::ids::FaceId> = lanes
            .get(ordinal)?
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
            inferred: true,
        }),
        _ => None,
    }
}
