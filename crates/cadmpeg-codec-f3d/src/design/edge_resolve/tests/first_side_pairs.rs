// SPDX-License-Identifier: Apache-2.0
//! Treatment groups whose members' first sides name three face references.

use super::{group, recipe_edge_operand};
use crate::design::edge_resolve::{first_side_pair_edge_group_assignment, EdgeTreatmentInputs};
use crate::records::identity::Located;
use crate::records::topology::edge_identity::DesignEdgeOperand;
use crate::records::topology::edge_recipe::{DesignEdgeRecipeStructure, DesignTopologyRecipeSide};
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::features::selections::EdgeSelection;
use cadmpeg_ir::features::FeatureId;

const DELETED: [i64; 6] = [10, 11, 12, 13, 20, 21];

fn side(header_value: i32, scalars: &[i32]) -> DesignTopologyRecipeSide {
    DesignTopologyRecipeSide {
        header_value,
        scalars: scalars.to_vec(),
        payload_prefix: vec![0],
        entries: Vec::new(),
    }
}

/// A member whose first side names references 1, 3, and 2, with the shared
/// edge lanes listed in reference order and local topology order 3, 1, 2.
fn three_reference_operand(record_index: u32, lanes: [&[i64]; 3]) -> DesignEdgeOperand {
    let mut operand = recipe_edge_operand(record_index, &[], &[]);
    operand.recipe_structure = Some(DesignEdgeRecipeStructure {
        root: 2,
        sides: vec![side(1, &[0, 3, 2]), side(1, &[0, 4])],
    });
    operand.recipe_reference_contexts = lanes
        .iter()
        .enumerate()
        .map(|(ordinal, edges)| {
            json::from_value(serde_json::json!({
                "reference_ordinal": ordinal,
                "result_faces": [],
                "result_shared_edge_slots": [],
                "preceding_faces": [format!("f3d:brep:entity#{}", 100 + ordinal)],
                "shared_edge_slots": edges,
                "changed_shared_edge_slots": edges,
                "changed_reference_edge_slots": edges,
            }))
            .unwrap()
        })
        .collect();
    operand.local_topology_references = Some(
        [3, 1, 2]
            .into_iter()
            .map(|ordinal| std::num::NonZeroU32::new(ordinal).expect("nonzero ordinal"))
            .collect(),
    );
    operand.transition_deleted_edge_slots = DELETED.to_vec();
    operand
}

fn proven_operand(record_index: u32, edge: i64) -> DesignEdgeOperand {
    let mut operand = recipe_edge_operand(record_index, &[], &[]);
    operand.resolved_edge_slot = Some(edge);
    operand
}

/// A proven member on edge 10, a member with candidates 11 and 12, and a
/// member whose only candidate is 12.
fn members() -> Vec<DesignEdgeOperand> {
    vec![
        proven_operand(10, 10),
        three_reference_operand(11, [&[10, 11], &[11, 12], &[12, 13]]),
        three_reference_operand(12, [&[12, 20], &[12, 21], &[10, 13]]),
    ]
}

fn assign(operands: &[DesignEdgeOperand]) -> Option<Vec<i64>> {
    let operands = operands.iter().collect::<Vec<_>>();
    crate::test_support::with_decode_context(|ctx| {
        first_side_pair_edge_group_assignment(&operands, ctx)
    })
    .unwrap()
}

#[test]
fn three_reference_members_take_the_one_distinct_pair_assignment() {
    assert_eq!(assign(&members()), Some(vec![10, 11, 12]));
}

#[test]
fn two_assignments_leave_the_group_unassigned() {
    let operands = members();
    assert_eq!(assign(&operands[..2]), None);
}

#[test]
fn three_faces_that_share_an_edge_are_another_form() {
    let mut operands = members();
    operands[2] = three_reference_operand(12, [&[12, 20], &[12, 21], &[12, 13]]);
    assert_eq!(assign(&operands), None);
}

#[test]
fn pair_candidates_are_transition_deleted_edges() {
    let mut operands = members();
    operands[1].transition_deleted_edge_slots = vec![10, 12, 13];
    assert_eq!(assign(&operands), None);
}

#[test]
fn groups_without_a_three_reference_member_use_other_proofs() {
    assert_eq!(
        assign(&[proven_operand(10, 10), proven_operand(11, 11)]),
        None
    );
}

#[test]
fn treatment_group_charges_the_first_side_pair_assumption() {
    let operands = members();
    let mut selection_group = group(2, 10);
    selection_group
        .try_set_members(
            [10, 11, 12]
                .into_iter()
                .map(|value| Located {
                    value,
                    offset: u64::from(value) * 11,
                })
                .collect(),
        )
        .unwrap();
    let feature = FeatureId::mint("f3d:model:feature#first-side-pairs").unwrap();
    let mut losses = Vec::new();
    let selection = crate::test_support::with_decode_context(|ctx| {
        crate::design::edge_resolve::resolved_edge_treatment_group_with_corners(
            &selection_group,
            EdgeTreatmentInputs {
                groups: std::slice::from_ref(&selection_group),
                operands: &operands,
                identity_operands: &[],
                vertex_operands: &[],
                histories: &[],
                previous_state_id: Some(7),
                feature_id: &feature,
                treatment_radius: None,
            },
            ctx,
            &mut losses,
        )
    })
    .unwrap();
    let prefix = crate::ids::feature_input_prefix(&feature, 7);
    assert_eq!(
        selection,
        EdgeSelection::historical(
            crate::ids::feature_input_topology_id(&feature, 7),
            [10, 11, 12]
                .into_iter()
                .map(|edge| crate::ids::history_input_edge_id(&prefix, edge))
                .collect(),
            selection_group.id.clone(),
        )
        .unwrap()
    );
    assert_eq!(
        losses,
        vec![crate::loss::F3dLossCode::HistoricalEdgeSelectionFirstSidePairAssumed.note(
            "Group f3d:test:group-2 is read with the assumption that each member whose first side names three faces selects an edge that two of those faces share, and that the members select distinct edges."
        )]
    );
}
