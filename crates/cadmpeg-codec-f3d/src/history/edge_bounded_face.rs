// SPDX-License-Identifier: Apache-2.0
//! Select the face that a bounded-face recipe bounds by its named edges.
//!
//! A bounded-face recipe names the edges that bound its face. When the recipe
//! program has no parsed nodes and the history fallback offers faces that
//! share no edge with the named ones, a named face whose complete boundary is
//! exactly the named edge set is the selected face.

use super::body_closures::stable_ref;
use super::face_boundaries::face_boundary_edges;
use super::face_resolution::effective_faces;
use super::topology_cache::TopologyQueryCache;
use crate::history_records::AsmHistoricalTopology;
use crate::records::topology::face::DesignFaceOperand;
use std::collections::BTreeSet;

/// The unique named face whose boundary is the named edge set, when every
/// `fallback` face slot of `operand` has a boundary that shares no edge with it.
pub(super) fn unique_edge_bounded_face<'a>(
    operand: &DesignFaceOperand,
    fallback: &[i64],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Option<i64> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace
        || !operand.recipe_nodes.is_empty()
        || fallback.is_empty()
    {
        return None;
    }
    let mut named = BTreeSet::new();
    for reference in &operand.recipe_references {
        let edges = if reference.candidate_edges.is_empty() {
            &reference.alternate_selector_edges
        } else {
            &reference.candidate_edges
        };
        for edge in edges {
            named.insert(stable_ref(edge.as_str())?);
        }
    }
    if named.is_empty() {
        return None;
    }
    let named = named.into_iter().collect::<Vec<_>>();
    for slot in fallback {
        let face = crate::ids::brep_face_id(*slot);
        let edges = face_boundary_edges(std::slice::from_ref(&face), topology, topology_queries);
        if edges.is_empty() || edges.iter().any(|edge| named.contains(edge)) {
            return None;
        }
    }
    let referenced = super::selection::faces_in_topology(
        operand.recipe_references.iter().flat_map(effective_faces),
        topology,
    );
    let mut selected = None;
    for face in &referenced {
        let slot = stable_ref(face.as_str())?;
        let edges = face_boundary_edges(std::slice::from_ref(face), topology, topology_queries);
        if edges != named {
            continue;
        }
        if selected.is_some_and(|previous| previous != slot) {
            return None;
        }
        selected = Some(slot);
    }
    selected
}

#[cfg(test)]
mod tests;
