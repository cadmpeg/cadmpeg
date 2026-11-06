// SPDX-License-Identifier: Apache-2.0
//! Check the faces that a candidate fallback selects for a bounded-face
//! recipe against the faces that the recipe's prefix operands name.
//!
//! The nodes of a bounded-face recipe name, by prefix-operand ordinal, faces
//! around the boundary of the recipe's face. For a recipe whose nodes have no
//! unique binding, the check takes every prefix operand as naming such faces:
//! a fallback selection that shares no boundary vertex with the faces of one
//! prefix operand is withdrawn.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::report::loss::LossNote;

use super::topology_cache::{endpoint_index, TopologyQueryCache, VertexBoundaryIndex};
use super::{boundary_vertices_for_faces, stable_ref};
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

/// The faces that each prefix operand of one recipe names in one topology.
/// A lane has boundary vertices only when every positive Design reference of
/// its operand names faces, every named face lies in the topology, and every
/// one of those faces can be read.
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
        let mut faces: Vec<(Vec<i64>, bool)> = Vec::new();
        for (reference, &ordinal) in operand.recipe_references.iter().zip(&ordinals) {
            if ordinal == faces.len() {
                faces.push((Vec::new(), true));
            }
            let (lane, complete) = faces.get_mut(ordinal)?;
            if reference.design_reference <= 0 {
                continue;
            }
            let named: Vec<Option<i64>> = reference
                .candidate_faces
                .iter()
                .map(|face| stable_ref(face.as_str()).filter(|face| index.faces.contains(face)))
                .collect();
            *complete &= !named.is_empty() && named.iter().all(Option::is_some);
            lane.extend(named.into_iter().flatten());
        }
        let lanes = faces
            .into_iter()
            .map(|(mut lane, complete)| {
                lane.sort_unstable();
                lane.dedup();
                let vertices = complete
                    .then(|| {
                        lane.iter()
                            .map(|face| {
                                boundary_vertices_for_faces(
                                    std::iter::once(*face),
                                    index,
                                    edge_endpoints,
                                )
                            })
                            .collect::<Option<Vec<_>>>()
                    })
                    .flatten()
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

    /// The faces that the prefix operand of ordinal `lane` names.
    fn names(&self, lane: usize) -> &[i64] {
        self.lanes.get(lane).map_or(&[], |(faces, _)| faces)
    }
}

/// The fallback faces that the check withdrew, by operand, and their losses.
#[derive(Default)]
pub(super) struct Withdrawals {
    pub(super) losses: Vec<LossNote>,
    pub(super) faces: HashMap<String, Vec<i64>>,
}

impl Withdrawals {
    /// Whether `faces` hold a face withdrawn from one of `members`.
    pub(super) fn holds<'a>(
        &self,
        faces: &[i64],
        mut members: impl Iterator<Item = &'a str>,
    ) -> bool {
        members.any(|member| {
            self.faces
                .get(member)
                .is_some_and(|dropped| dropped.iter().any(|face| faces.contains(face)))
        })
    }
}

/// Withdraw the fallback faces of a bounded-face operand when they share no
/// boundary vertex with the faces of one of its prefix operands.
pub(super) fn check_fallback_faces<'a>(
    operand: &mut DesignFaceOperand,
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
    withdrawals: &mut Withdrawals,
) {
    if operand.resolved_face_slots.is_empty() {
        return;
    }
    let Some(lanes) = OperandLanes::of(operand, topology, topology_queries) else {
        return;
    };
    let LaneFit::Apart(lane) = lanes.fit(&operand.resolved_face_slots, topology, topology_queries)
    else {
        return;
    };
    let fallback = std::mem::take(&mut operand.resolved_face_slots);
    withdrawals.losses.push(F3dLossCode::FeatureFaceOperandFallbackContradicted.note(format!(
        "Extrude face operand {} drops its fallback faces {fallback:?}: they share no boundary vertex with the faces {:?} that its recipe operand {lane} names, taken as faces around the selected face's boundary.",
        operand.id,
        lanes.names(lane),
    )));
    withdrawals.faces.insert(operand.id.clone(), fallback);
}

#[cfg(test)]
mod tests;
