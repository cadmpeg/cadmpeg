// SPDX-License-Identifier: Apache-2.0

#[cfg(test)]
use super::{
    face_boundary_contexts_for_slots, selection, stable_ref, AsmHistoricalTopology, SideOneEdge,
    TopologyQueryCache,
};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
fn side_one_reference_face_edge<'a>(
    structure: Option<&crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>,
    references: &[crate::records::dimensions::DesignRecipeReference],
    candidate_boundary_edges: &[i64],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Option<SideOneEdge> {
    let lanes = crate::records::dimensions::reference_lanes(references);
    common_edge(structure, candidate_boundary_edges, None, |ordinal| {
        let lane = super::recipe_index::reference_lane_faces(lanes.get(ordinal)?).faces;
        let mut faces = selection::faces_in_topology(&lane, topology);
        faces.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        faces.dedup();
        if faces.is_empty() {
            return None;
        }
        fragment_boundaries(&faces, topology, topology_queries)
    })
}

fn side_one_ordinals(
    structure: Option<&crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>,
) -> Option<BTreeSet<usize>> {
    let structure = structure?;
    if structure.root != 2 || structure.sides.len() != 2 {
        return None;
    }
    let side = structure.sides.first()?;
    let mut ordinals = BTreeSet::new();
    for value in std::iter::once(side.header_value)
        .chain(side.scalars.iter().copied())
        .filter(|value| *value != 0)
    {
        ordinals.insert(usize::try_from(value).ok()?.checked_sub(1)?);
    }
    if ordinals.len() < 2 {
        return None;
    }
    Some(ordinals)
}

fn common_edge(
    structure: Option<&crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>,
    candidate_boundary_edges: &[i64],
    required_edges: Option<&[i64]>,
    mut lane: impl FnMut(usize) -> Option<BTreeMap<i64, BTreeSet<i64>>>,
) -> Option<super::SideOneEdge> {
    let ordinals = side_one_ordinals(structure)?;
    let mut common: Option<BTreeMap<i64, BTreeSet<i64>>> = None;
    for ordinal in ordinals {
        let edges = lane(ordinal)?;
        match &mut common {
            Some(common) => common.retain(|edge, supports| {
                if let Some(faces) = edges.get(edge) {
                    supports.extend(faces);
                    true
                } else {
                    false
                }
            }),
            None => common = Some(edges),
        }
    }
    let common: Vec<i64> = common?
        .into_iter()
        .filter_map(|(edge, supports)| (supports.len() >= 2).then_some(edge))
        .filter(|edge| required_edges.is_none_or(|required| required.contains(edge)))
        .collect();
    if required_edges.is_some() && common.len() != 1 {
        return None;
    }
    let mut proven = common
        .iter()
        .copied()
        .filter(|edge| candidate_boundary_edges.contains(edge));
    match (proven.next(), proven.next(), common.as_slice()) {
        (Some(slot), None, _) => Some(super::SideOneEdge {
            slot,
            inferred: false,
        }),
        (None, _, [slot]) => Some(super::SideOneEdge {
            slot: *slot,
            inferred: true,
        }),
        _ => None,
    }
}

#[cfg(test)]
fn fragment_boundaries<'a>(
    faces: &[cadmpeg_ir::ids::FaceId],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Option<BTreeMap<i64, BTreeSet<i64>>> {
    let live_edges = selection::FamilySlots::new(&topology.edges);
    let mut edges: BTreeMap<i64, BTreeSet<i64>> = BTreeMap::new();
    for face in faces {
        let face = stable_ref(face.as_str())?;
        let [boundary] = face_boundary_contexts_for_slots(&[face], topology, topology_queries)
            .try_into()
            .ok()?;
        if boundary.loops.is_empty() {
            return None;
        }
        for face_loop in &boundary.loops {
            let mut nonempty = false;
            for coedge in face_loop.boundary.coedges() {
                if !live_edges.contains(coedge.edge_slot) {
                    return None;
                }
                edges.entry(coedge.edge_slot).or_default().insert(face);
                nonempty = true;
            }
            if !nonempty {
                return None;
            }
        }
    }
    Some(edges)
}

pub(crate) fn context_edge(
    operand: &crate::records::topology::edge_identity::DesignEdgeOperand,
    boundaries: &std::collections::HashMap<
        &str,
        &crate::records::topology::historical_context::AsmFaceBoundary,
    >,
) -> Option<super::SideOneEdge> {
    let lanes = crate::records::dimensions::reference_lanes(&operand.recipe_references);
    let disjoint_names = side_one_ordinals(operand.recipe_structure.as_ref())?
        .into_iter()
        .filter_map(|ordinal| lanes.get(ordinal))
        .any(|lane| super::recipe_index::reference_lane_faces(lane).disjoint_names);
    common_edge(
        operand.recipe_structure.as_ref(),
        &operand.preceding_boundary_edge_slots,
        disjoint_names.then_some(operand.transition_deleted_edge_slots.as_slice()),
        |ordinal| {
            let context = operand.recipe_reference_contexts.get(ordinal)?;
            if usize::try_from(context.reference_ordinal).ok()? != ordinal
                || context.preceding_faces.is_empty()
                || context.preceding_faces.len() != context.preceding_face_boundary_ids.len()
            {
                return None;
            }
            let mut edges: BTreeMap<i64, BTreeSet<i64>> = BTreeMap::new();
            for (face, id) in context
                .preceding_faces
                .iter()
                .zip(&context.preceding_face_boundary_ids)
            {
                let face = super::stable_ref(face.as_str())?;
                let boundary = boundaries.get(id.as_str())?;
                if boundary.face_slot != face || boundary.loops.is_empty() {
                    return None;
                }
                for face_loop in &boundary.loops {
                    let mut nonempty = false;
                    for coedge in face_loop.boundary.coedges() {
                        edges.entry(coedge.edge_slot).or_default().insert(face);
                        nonempty = true;
                    }
                    if !nonempty {
                        return None;
                    }
                }
            }
            Some(edges)
        },
    )
    .map(|mut edge| {
        edge.inferred |= disjoint_names;
        edge
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_records::{AsmHistoricalCoedge, AsmHistoricalRelation};
    use crate::records::dimensions::DesignRecipeReference;
    use cadmpeg_ir::ids::FaceId;

    #[test]
    fn recipe_reference_disjoint_names_require_one_deleted_common_edge() {
        let mut operand: crate::records::topology::edge_identity::DesignEdgeOperand =
            serde_json::from_value(serde_json::json!({
                "id":"f3d:Design/BulkStream.dat:edge-operand#7", "scope_record_index":42,
                "scope_reference_ordinal":0, "record_index":7, "byte_offset":0, "class_tag":"297",
                "paired_byte_offset":16, "paired_class_tag":"259", "recipe_record_index":10,
                "recipe_record_byte_offset":32, "recipe_id":"f3d:test:recipe", "recipe_prefix_offset":43,
                "recipe_prefix_bytes":"", "recipe_references":[], "recipe_program_offset":0,
                "recipe_program":[], "next_record_index":11, "next_byte_offset":160,
            })).unwrap();
        let program = [
            -1, -1, 2, 0, -1, 1, -1, 2, -1, 3, 1, -1, 0, -1, 2, -1, 0, 0, -1, 3, 0, -1, 0, -1, 0,
            -1, 0, 0, -1,
        ];
        operand.recipe_structure = crate::design::decode::operands::edge_recipe_structure(
            &cadmpeg_test_support::service_decode_context(),
            &program,
        )
        .unwrap();
        operand.recipe_references = [(0, 0, 10), (0, 40, 20), (80, 80, 40)]
            .into_iter()
            .map(
                |(operand_offset, selector_offset, face)| DesignRecipeReference {
                    selector: 1,
                    selector_offset,
                    operand_offset: Some(operand_offset),
                    token: "2".into(),
                    token_integer: 0,
                    token_offset: selector_offset + 8,
                    design_reference: 301,
                    design_reference_offset: selector_offset + 17,
                    candidate_faces: vec![FaceId::mint(crate::ids::brep_entity_id(face)).unwrap()],
                    candidate_edges: vec![],
                    alternate_selector_faces: vec![],
                    alternate_selector_edges: vec![],
                },
            )
            .collect();
        operand.recipe_reference_contexts = crate::records::dimensions::reference_lanes(&operand.recipe_references)
            .into_iter().enumerate().map(|(ordinal, lane)| {
                let faces = super::super::recipe_index::reference_lane_faces(lane).faces;
                serde_json::from_value(serde_json::json!({
                    "reference_ordinal":ordinal, "result_faces":[], "result_shared_edge_slots":[],
                    "preceding_face_boundary_ids":faces.iter().map(|f| format!("boundary-{}", super::super::stable_ref(f.as_str()).unwrap())).collect::<Vec<_>>(),
                    "preceding_faces":faces, "shared_edge_slots":[], "changed_shared_edge_slots":[],
                })).unwrap()
            }).collect();
        let boundaries: Vec<crate::records::topology::historical_context::AsmFaceBoundary> =
            [(10, vec![31]), (20, vec![32]), (40, vec![31,32])].into_iter().map(|(face, edges)| {
                serde_json::from_value(serde_json::json!({
                    "id":format!("boundary-{face}"), "parent":"state-1", "face_slot":face,
                    "loops":[{"loop_slot":face,"coedge_slots":edges.iter().map(|e| e*10+face).collect::<Vec<_>>(),"edge_slots":edges}],
                })).unwrap()
            }).collect();
        let index = boundaries.iter().map(|b| (b.id.as_str(), b)).collect();
        for (deleted, expected) in [
            (vec![31], Some((31, true))),
            (vec![32], Some((32, true))),
            (vec![], None),
            (vec![31, 32], None),
        ] {
            operand.transition_deleted_edge_slots = deleted;
            assert_eq!(
                context_edge(&operand, &index).map(|e| (e.slot, e.inferred)),
                expected
            );
        }
    }

    #[test]
    fn fragment_lane_union_selects_one_edge_and_retains_multiple_edges_native() {
        let program = [
            -1, -1, 2, 0, -1, 1, -1, 2, -1, 3, 1, -1, 0, -1, 2, -1, 0, 0, -1, 3, 0, -1, 0, -1, 0,
            -1, 0, 0, -1,
        ];
        let structure = crate::design::decode::operands::edge_recipe_structure(
            &cadmpeg_test_support::service_decode_context(),
            &program,
        )
        .unwrap()
        .unwrap();
        for (other_edges, expected) in [([31, 33], Some((31, true))), ([31, 32], None)] {
            let mut topology = AsmHistoricalTopology::default();
            for (face, edges) in [(10, [30, 31]), (20, [32, 34]), (40, other_edges)] {
                let loop_slot = face * 10;
                let coedges = [face * 100, face * 100 + 1];
                topology.faces.push(face);
                topology.loops.push(loop_slot);
                topology.edges.extend(edges);
                topology.coedges.extend(coedges);
                topology.face_loops.push(AsmHistoricalRelation {
                    owner_ref: face,
                    member_refs: vec![loop_slot],
                });
                topology.loop_coedges.push(AsmHistoricalRelation {
                    owner_ref: loop_slot,
                    member_refs: coedges.to_vec(),
                });
                for index in 0..2 {
                    topology.coedge_topology.push(AsmHistoricalCoedge {
                        coedge: coedges[index],
                        owner_loop: loop_slot,
                        edge: edges[index],
                        next: coedges[1 - index],
                        previous: coedges[1 - index],
                        radial_next: coedges[index],
                    });
                }
            }
            topology.edges.sort_unstable();
            topology.edges.dedup();
            let references =
                [(1, vec![10, 20]), (2, vec![40])].map(|(ordinal, faces)| DesignRecipeReference {
                    selector: 1,
                    selector_offset: ordinal * 100,
                    token: ordinal.to_string(),
                    token_integer: 0,
                    token_offset: ordinal * 100 + 8,
                    design_reference: 50,
                    design_reference_offset: 0,
                    candidate_faces: faces
                        .into_iter()
                        .map(|face| FaceId::mint(crate::ids::brep_entity_id(face)).unwrap())
                        .collect(),
                    candidate_edges: Vec::new(),
                    alternate_selector_faces: Vec::new(),
                    alternate_selector_edges: Vec::new(),
                    operand_offset: Some(ordinal * 100),
                });
            let resolve = |candidate_edges: &[i64]| {
                side_one_reference_face_edge(
                    Some(&structure),
                    &references,
                    candidate_edges,
                    &topology,
                    &mut TopologyQueryCache::default(),
                )
                .map(|edge| (edge.slot, edge.inferred))
            };
            assert_eq!(resolve(&[]), expected);
            assert_eq!(resolve(&[31]), Some((31, false)));
            let mut same_face = references.clone();
            same_face[1].candidate_faces =
                vec![FaceId::mint(crate::ids::brep_entity_id(10)).unwrap()];
            assert_eq!(
                side_one_reference_face_edge(
                    Some(&structure),
                    &same_face,
                    &[],
                    &topology,
                    &mut TopologyQueryCache::default(),
                ),
                None,
            );
            let mut incomplete = topology.clone();
            incomplete.edges.retain(|edge| *edge != 31);
            assert_eq!(
                side_one_reference_face_edge(
                    Some(&structure),
                    &references,
                    &[],
                    &incomplete,
                    &mut TopologyQueryCache::default(),
                ),
                None,
            );
        }
    }
}
