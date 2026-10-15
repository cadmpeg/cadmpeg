// SPDX-License-Identifier: Apache-2.0
//! Edge-group candidate derivations over complete operand groups.

use super::{group, recipe_edge_operand};
use crate::design::edge_resolve::{
    changed_reference_edge_group_candidates, deleted_reference_edge_group_candidates,
    partition_unique_incomplete_edge_group, resolved_hem_edge_group,
    scope_partition_edge_group_candidates, unique_deleted_reference_assignment,
    unique_edge_group_assignment, EdgeGroupMember,
};
use crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext;
use cadmpeg_ir::features::{EdgeSelection, FeatureId};

fn changed_reference_context(edge: i64) -> DesignEdgeRecipeReferenceContext {
    serde_json::from_value(serde_json::json!({
        "reference_ordinal": 0,
        "result_faces": [],
        "result_shared_edge_slots": [],
        "preceding_faces": [],
        "shared_edge_slots": [],
        "changed_shared_edge_slots": [],
        "changed_reference_edge_slots": [edge],
    }))
    .unwrap()
}

/// A Hem operand whose first reference changes no edge and whose three
/// support references each change one boundary edge.
fn hem_operand() -> crate::records::topology::edge_identity::DesignEdgeOperand {
    let mut operand = recipe_edge_operand(10, &[63, 106, 110, 167], &[]);
    operand.recipe_reference_contexts = [None, Some(167), Some(106), Some(110)]
        .into_iter()
        .enumerate()
        .map(|(ordinal, edge)| {
            serde_json::from_value(serde_json::json!({
                "reference_ordinal": ordinal,
                "result_faces": if edge.is_some() { vec!["f3d:model:face#1"] } else { Vec::new() },
                "result_shared_edge_slots": [],
                "preceding_faces": [],
                "preceding_support_face_slots": if edge.is_some() { vec![2] } else { Vec::new() },
                "shared_edge_slots": [],
                "changed_shared_edge_slots": [],
                "changed_reference_edge_slots": edge.into_iter().collect::<Vec<_>>(),
            }))
            .unwrap()
        })
        .collect();
    operand
}

#[test]
fn hem_group_selects_the_changed_edge_outside_the_support_boundaries() {
    let selection_group = group(2, 10);
    let operand = hem_operand();
    let feature_id = FeatureId::mint("f3d:model:feature#hem").unwrap();
    let resolve = |previous_state_id| {
        crate::test_support::with_decode_context(|ctx| {
            resolved_hem_edge_group(
                &selection_group,
                std::slice::from_ref(&selection_group),
                std::slice::from_ref(&operand),
                &[],
                previous_state_id,
                &feature_id,
                ctx,
            )
        })
        .unwrap()
    };
    let prefix = crate::ids::feature_input_prefix(&feature_id, 7);
    assert_eq!(
        resolve(Some(7)),
        EdgeSelection::historical(
            crate::ids::feature_input_topology_id(&feature_id, 7),
            vec![crate::ids::history_input_edge_id(&prefix, 63)],
            selection_group.id.clone(),
        )
        .unwrap()
    );
    // Without a prior history state the transition edge has no input state.
    assert!(matches!(resolve(None), EdgeSelection::Native(_)));
}

#[test]
fn scope_partition_assigns_the_unreserved_deleted_edge_to_the_incomplete_group() {
    let deleted = [17, 18];
    let mut source_operand = recipe_edge_operand(10, &[], &deleted);
    source_operand.resolved_edge_slot = Some(17);
    let target_operand = recipe_edge_operand(12, &[], &deleted);
    let scope_groups = [group(2, 10), group(3, 12)];
    let operands = [source_operand, target_operand];
    assert_eq!(
        scope_partition_edge_group_candidates(
            &scope_groups[1],
            &scope_groups,
            &operands,
            scope_groups[1].members(),
        ),
        Some(vec![18])
    );
    let groups = [
        vec![EdgeGroupMember {
            identity: 10,
            resolved_edge: Some(17),
            deleted_boundary_edges: &deleted,
        }],
        vec![EdgeGroupMember {
            identity: 12,
            resolved_edge: None,
            deleted_boundary_edges: &deleted,
        }],
    ];
    assert_eq!(
        partition_unique_incomplete_edge_group(1, &groups),
        Some(vec![18])
    );
}

#[test]
fn reference_assignments_pair_each_operand_with_its_changed_deleted_edge() {
    let mut first = recipe_edge_operand(10, &[], &[17]);
    first.recipe_reference_contexts = vec![changed_reference_context(17)];
    let mut second = recipe_edge_operand(11, &[], &[18]);
    second.recipe_reference_contexts = vec![changed_reference_context(18)];
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            deleted_reference_edge_group_candidates(&[&first, &second], ctx).unwrap(),
            Some(vec![17, 18])
        );
        assert_eq!(
            changed_reference_edge_group_candidates(&[&first, &second], ctx).unwrap(),
            Some(vec![17, 18])
        );
        assert_eq!(
            unique_deleted_reference_assignment(&[vec![17], vec![18]], &[vec![17], vec![18]], ctx)
                .unwrap(),
            Some(vec![17, 18])
        );
    });
}

#[test]
fn unique_group_assignment_keeps_each_resolved_member_edge() {
    let mut first = recipe_edge_operand(10, &[], &[]);
    first.resolved_edge_slot = Some(17);
    let mut second = recipe_edge_operand(11, &[], &[]);
    second.resolved_edge_slot = Some(18);
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            unique_edge_group_assignment(&[&first, &second], ctx)
        })
        .unwrap(),
        Some(vec![17, 18])
    );
}
