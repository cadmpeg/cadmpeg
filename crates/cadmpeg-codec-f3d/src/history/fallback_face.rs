// SPDX-License-Identifier: Apache-2.0
//! Check the faces that a candidate fallback selects for a bounded-face
//! recipe against the faces that the recipe's prefix operands name.
//!
//! A bounded-face recipe names faces around the boundary of its selection, so
//! the selected faces share a boundary vertex with the faces of every prefix
//! operand. A fallback selection that fails that test is withdrawn.

use std::collections::HashSet;

use cadmpeg_ir::report::loss::LossNote;

use super::topology_cache::{endpoint_index, TopologyQueryCache, VertexBoundaryIndex};
use super::{boundary_vertices_for_faces, effective_faces, stable_ref};
use crate::history_records::AsmHistoricalTopology;
use crate::loss::F3dLossCode;
use crate::records::topology::face::{prefix_operand_ordinals, DesignFaceOperand};

/// How a face set meets the prefix-operand lanes of a recipe.
#[derive(Debug, PartialEq)]
pub(super) enum LaneFit {
    /// The faces share a boundary vertex with every lane whose faces can be
    /// read.
    Touches,
    /// The faces share no boundary vertex with the lane of this
    /// prefix-operand ordinal.
    Apart(usize),
    /// The boundary of one of the faces cannot be read.
    Unreadable,
}

/// The faces that each prefix operand of one recipe names in one topology,
/// with their boundary vertices when every one of those faces can be read.
pub(super) struct OperandLanes {
    lanes: Vec<(Vec<i64>, Option<HashSet<i64>>)>,
}

impl OperandLanes {
    /// `None` when the recipe's prefix operands cannot be numbered.
    pub(super) fn of<'a>(
        operand: &DesignFaceOperand,
        topology: &'a AsmHistoricalTopology,
        topology_queries: &mut TopologyQueryCache<'a>,
    ) -> Option<Self> {
        let ordinals = prefix_operand_ordinals(&operand.recipe_references)?;
        let index = topology_queries
            .vertices
            .get(topology, VertexBoundaryIndex::new);
        let edge_endpoints = topology_queries.endpoints.get(topology, endpoint_index);
        let mut faces: Vec<Vec<i64>> = Vec::new();
        for (reference, &ordinal) in operand.recipe_references.iter().zip(&ordinals) {
            if ordinal == faces.len() {
                faces.push(Vec::new());
            }
            let lane = faces.get_mut(ordinal)?;
            if reference.design_reference < 0 {
                continue;
            }
            lane.extend(
                effective_faces(reference)
                    .iter()
                    .filter_map(|face| stable_ref(face.as_str()))
                    .filter(|face| index.faces.contains(face)),
            );
        }
        let lanes = faces
            .into_iter()
            .map(|mut lane| {
                lane.sort_unstable();
                lane.dedup();
                let vertices = lane
                    .iter()
                    .map(|face| {
                        boundary_vertices_for_faces(std::iter::once(*face), index, edge_endpoints)
                    })
                    .collect::<Option<Vec<_>>>()
                    .map(|sets| sets.into_iter().flatten().collect());
                (lane, vertices)
            })
            .collect();
        Some(Self { lanes })
    }

    pub(super) fn fit<'a>(
        &self,
        faces: &[i64],
        topology: &'a AsmHistoricalTopology,
        topology_queries: &mut TopologyQueryCache<'a>,
    ) -> LaneFit {
        let index = topology_queries
            .vertices
            .get(topology, VertexBoundaryIndex::new);
        let edge_endpoints = topology_queries.endpoints.get(topology, endpoint_index);
        let Some(vertices) =
            boundary_vertices_for_faces(faces.iter().copied(), index, edge_endpoints)
        else {
            return LaneFit::Unreadable;
        };
        self.lanes
            .iter()
            .position(|(lane, lane_vertices)| {
                !lane.is_empty()
                    && lane_vertices
                        .as_ref()
                        .is_some_and(|lane_vertices| lane_vertices.is_disjoint(&vertices))
            })
            .map_or(LaneFit::Touches, LaneFit::Apart)
    }
}

/// Withdraw the fallback faces of a bounded-face operand when they share no
/// boundary vertex with the faces of one of its prefix operands.
pub(super) fn check_fallback_faces<'a>(
    operand: &mut DesignFaceOperand,
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Option<LossNote> {
    if operand.resolved_face_slots.is_empty() {
        return None;
    }
    let lanes = OperandLanes::of(operand, topology, topology_queries)?;
    let LaneFit::Apart(lane) = lanes.fit(&operand.resolved_face_slots, topology, topology_queries)
    else {
        return None;
    };
    let fallback = std::mem::take(&mut operand.resolved_face_slots);
    Some(F3dLossCode::FeatureFaceOperandFallbackContradicted.note(format!(
        "Extrude face operand {} drops its fallback faces {fallback:?}: they share no boundary vertex with the faces of recipe operand {lane}.",
        operand.id
    )))
}

#[cfg(test)]
mod tests;
