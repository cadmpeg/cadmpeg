// SPDX-License-Identifier: Apache-2.0
//! Edge-group candidate derivations over complete operand groups.

use super::{group, identity, recipe_edge_operand};
use crate::design::edge_resolve::{
    changed_reference_edge_group_candidates, deleted_reference_edge_group_candidates,
    partition_unique_incomplete_edge_group, resolved_hem_edge_group,
    scope_partition_edge_group_candidates, transition_complete_edge_group_candidates,
    unique_deleted_reference_assignment, unique_edge_group_assignment, EdgeGroupMember,
};
use crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext;
use cadmpeg_ir::features::{EdgeSelection, FeatureId};

fn transition_complete_group() -> (
    [crate::records::topology::edge_identity::DesignEdgeOperand; 3],
    [crate::records::topology::edge_identity::DesignEdgeIdentityOperand; 3],
) {
    let operands = [
        recipe_edge_operand(10, &[], &[101, 102]),
        recipe_edge_operand(11, &[], &[102, 103, 104]),
        recipe_edge_operand(12, &[], &[104, 105]),
    ];
    let identities = [10, 11, 12].map(|index| {
        identity(
            index,
            &[
                (101, 0.35),
                (102, 0.35),
                (103, 0.35),
                (104, 0.35),
                (105, 0.35),
            ],
        )
    });
    (operands, identities)
}

#[test]
fn transition_complete_group_selects_five_edges_covered_by_three_members() {
    let (operands, mut identities) = transition_complete_group();
    identities[1].transition_edge_candidates.reverse();
    assert_eq!(
        transition_complete_edge_group_candidates(
            &operands.each_ref(),
            &identities.each_ref(),
            0.35
        ),
        Some(vec![101, 102, 103, 104, 105])
    );
}

#[test]
fn transition_complete_group_refuses_an_uncovered_edge() {
    let (mut operands, identities) = transition_complete_group();
    operands[2].deleted_boundary_edge_slots = vec![104];
    assert_eq!(
        transition_complete_edge_group_candidates(
            &operands.each_ref(),
            &identities.each_ref(),
            0.35
        ),
        None
    );
}

#[test]
fn transition_complete_single_member_admits_radius_qualified_subset() {
    let operands = [recipe_edge_operand(10, &[], &[974, 977])];
    let identities = [identity(10, &[(977, 2.0)])];
    assert_eq!(
        transition_complete_edge_group_candidates(
            &operands.each_ref(),
            &identities.each_ref(),
            2.0
        ),
        Some(vec![977])
    );
}

#[test]
fn transition_complete_single_member_refuses_competing_radius_evidence() {
    let operands = [recipe_edge_operand(10, &[], &[974, 977])];
    let identities = [identity(10, &[(977, 2.0), (974, 3.0)])];
    assert_eq!(
        transition_complete_edge_group_candidates(
            &operands.each_ref(),
            &identities.each_ref(),
            2.0
        ),
        None
    );
}

#[test]
fn transition_complete_group_refuses_different_identity_transition_sets() {
    let (operands, mut identities) = transition_complete_group();
    identities[1].transition_edge_candidates = vec![101, 102, 103, 104];
    assert_eq!(
        transition_complete_edge_group_candidates(
            &operands.each_ref(),
            &identities.each_ref(),
            0.35
        ),
        None
    );
}

#[test]
fn transition_complete_group_refuses_a_blend_radius_mismatch() {
    let (operands, _) = transition_complete_group();
    let identities = [10, 11, 12].map(|index| {
        identity(
            index,
            &[
                (101, 0.35),
                (102, 0.35),
                (103, 0.35),
                (104, 0.35),
                (105, 0.4),
            ],
        )
    });
    assert_eq!(
        transition_complete_edge_group_candidates(
            &operands.each_ref(),
            &identities.each_ref(),
            0.35
        ),
        None
    );
}

#[test]
fn transition_complete_group_refuses_exact_identity_slots() {
    let (operands, mut identities) = transition_complete_group();
    identities[1].resolved_edge_slot = Some(103);
    assert_eq!(
        transition_complete_edge_group_candidates(
            &operands.each_ref(),
            &identities.each_ref(),
            0.35
        ),
        None
    );
    identities[1].resolved_edge_slot = None;
    identities[1].resolved_edge_slots = vec![103, 104];
    assert_eq!(
        transition_complete_edge_group_candidates(
            &operands.each_ref(),
            &identities.each_ref(),
            0.35
        ),
        None
    );
}

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

fn fragment_transition_group() -> (
    Vec<crate::records::topology::edge_identity::DesignEdgeOperand>,
    Vec<crate::records::topology::edge_identity::DesignEdgeIdentityOperand>,
    Vec<crate::records::topology::historical_context::AsmFaceBoundary>,
) {
    let (mut operands, identities) = transition_complete_group();
    let program = [
        -1, -1, 2, 0, -1, 1, -1, 2, -1, 3, 1, -1, 0, -1, 2, -1, 0, 0, -1, 3, 0, -1, 0, -1, 0, -1,
        0, 0, -1,
    ];
    let structure = crate::design::decode::operands::edge_recipe_structure(
        &cadmpeg_test_support::service_decode_context(),
        &program,
    )
    .unwrap()
    .unwrap();
    let mut boundaries = Vec::new();
    for (operand, edge) in operands.iter_mut().zip([101, 103, 105]) {
        operand.recipe_structure = Some(structure.clone());
        operand.recipe_state_id = Some(7);
        let base = i64::from(operand.record_index()) * 10;
        for (face, edges) in [
            (base, vec![edge, 201]),
            (base + 1, vec![202, 203]),
            (base + 2, vec![edge, 204]),
        ] {
            boundaries.push(serde_json::from_value(serde_json::json!({
                "id": format!("face-{face}"), "parent": "state-7", "face_slot": face,
                "loops": [{"loop_slot": face, "coedge_slots": [face*2, face*2+1], "edge_slots": edges}],
            })).unwrap());
        }
        operand.recipe_reference_contexts = [vec![base,base+1],vec![base+2]].into_iter()
            .enumerate().map(|(ordinal,faces)| serde_json::from_value(serde_json::json!({
                "reference_ordinal": ordinal, "result_faces": [], "result_shared_edge_slots": [],
                "preceding_faces": faces.iter().map(|f| format!("f3d:brep:entity#{f}")).collect::<Vec<_>>(),
                "preceding_face_boundary_ids": faces.iter().map(|f| format!("face-{f}")).collect::<Vec<_>>(),
                "shared_edge_slots": [], "changed_shared_edge_slots": [],
            })).unwrap()).collect();
    }
    (operands.to_vec(), identities.to_vec(), boundaries)
}

#[test]
fn complete_transition_selection_precedes_fragment_lane_singletons() {
    let (operands, identities, boundaries) = fragment_transition_group();
    let mut selection_group = group(2, 10);
    selection_group
        .try_set_members(
            [10, 11, 12]
                .into_iter()
                .map(|value| crate::records::identity::Located {
                    value,
                    offset: u64::from(value) * 11,
                })
                .collect(),
        )
        .unwrap();
    let feature = FeatureId::mint("f3d:model:feature#fragment-transition").unwrap();
    let boundaries = boundaries.iter().map(|b| (b.id.as_str(), b)).collect();
    for (operand, edge) in operands.iter().zip([101, 103, 105]) {
        assert_eq!(
            crate::history::fragment_edge::context_edge(operand, &boundaries).map(|e| e.slot),
            Some(edge)
        );
    }
    let mut selection = crate::design::edge_resolve::resolved_edge_treatment_group(
        &selection_group,
        std::slice::from_ref(&selection_group),
        &operands,
        &identities,
        Some(7),
        &feature,
        Some(0.35),
    )
    .unwrap();
    let expected = EdgeSelection::historical(
        crate::ids::feature_input_topology_id(&feature, 7),
        [101, 102, 103, 104, 105]
            .into_iter()
            .map(|edge| {
                crate::ids::history_input_edge_id(
                    &crate::ids::feature_input_prefix(&feature, 7),
                    edge,
                )
            })
            .collect(),
        selection_group.id.clone(),
    )
    .unwrap();
    assert_eq!(selection, expected);
    let mut notes = Vec::new();
    crate::design::edge_resolve::fragment_fallback::apply_group(
        &mut selection,
        &selection_group,
        &operands,
        &identities,
        &boundaries,
        &feature,
        &mut notes,
    );
    assert_eq!(selection, expected);
    assert!(notes.is_empty());
    assert!(operands.iter().all(|o| o.resolved_edge_slot.is_none()));
}

#[test]
fn fragment_fallback_keeps_partial_edges_and_resolves_only_unresolved_members() {
    let (operands, _, boundaries) = fragment_transition_group();
    let mut selection_group = group(2, 10);
    selection_group
        .try_set_members(
            [10, 11, 12]
                .into_iter()
                .map(|value| crate::records::identity::Located {
                    value,
                    offset: u64::from(value) * 11,
                })
                .collect(),
        )
        .unwrap();
    let feature = FeatureId::mint("f3d:model:feature#fragment-partial").unwrap();
    let edge = |slot| {
        crate::ids::history_input_edge_id(&crate::ids::feature_input_prefix(&feature, 7), slot)
    };
    let state = crate::ids::feature_input_topology_id(&feature, 7);
    let mut selection = EdgeSelection::historical_partial(
        state.clone(),
        vec![edge(104), edge(102)],
        vec![operands[2].id.clone()],
        selection_group.id.clone(),
    )
    .unwrap();
    let boundaries = boundaries.iter().map(|b| (b.id.as_str(), b)).collect();
    let mut notes = Vec::new();
    let original = selection.clone();
    let mut selected_identity = identity(12, &[]);
    selected_identity.resolved_edge_slot = Some(106);
    crate::design::edge_resolve::fragment_fallback::apply_group(
        &mut selection,
        &selection_group,
        &operands,
        &[selected_identity],
        &boundaries,
        &feature,
        &mut notes,
    );
    assert_eq!(selection, original);
    assert!(notes.is_empty());
    crate::design::edge_resolve::fragment_fallback::apply_group(
        &mut selection,
        &selection_group,
        &operands,
        &[],
        &boundaries,
        &feature,
        &mut notes,
    );
    assert_eq!(
        selection,
        EdgeSelection::historical(
            state,
            vec![edge(104), edge(102), edge(105)],
            selection_group.id
        )
        .unwrap()
    );
    assert_eq!(notes.len(), 1);
    assert!(notes[0].message.contains(&operands[2].id));
    assert!(operands.iter().all(|o| o.resolved_edge_slot.is_none()));
}

#[test]
fn deleted_recipe_singleton_completes_a_member_with_a_resolved_identity() {
    for (deleted, candidate, admitted) in [(true, false, true), (false, true, false)] {
        for multiple in [false, true] {
            let (mut operands, _, boundaries) = fragment_transition_group();
            if deleted {
                operands[2].transition_deleted_edge_slots = vec![105];
            }
            if candidate {
                operands[2].preceding_boundary_edge_slots = vec![105];
            }
            let mut selection_group = group(2, 10);
            selection_group
                .try_set_members(
                    [10, 11, 12]
                        .into_iter()
                        .map(|value| crate::records::identity::Located {
                            value,
                            offset: u64::from(value) * 11,
                        })
                        .collect(),
                )
                .unwrap();
            let feature = FeatureId::mint("f3d:model:feature#recipe-priority").unwrap();
            let edge = |slot| {
                crate::ids::history_input_edge_id(
                    &crate::ids::feature_input_prefix(&feature, 7),
                    slot,
                )
            };
            let state = crate::ids::feature_input_topology_id(&feature, 7);
            let mut selection = EdgeSelection::historical_partial(
                state.clone(),
                vec![edge(104), edge(102)],
                vec![operands[2].id.clone()],
                selection_group.id.clone(),
            )
            .unwrap();
            let original = selection.clone();
            let mut selected_identity = identity(12, &[]);
            if multiple {
                selected_identity.resolved_edge_slots = vec![106, 107];
            } else {
                selected_identity.resolved_edge_slot = Some(106);
            }
            let boundaries = boundaries.iter().map(|b| (b.id.as_str(), b)).collect();
            let mut notes = Vec::new();
            crate::design::edge_resolve::fragment_fallback::apply_group(
                &mut selection,
                &selection_group,
                &operands,
                &[selected_identity],
                &boundaries,
                &feature,
                &mut notes,
            );
            let expected = if admitted {
                EdgeSelection::historical(
                    state,
                    vec![edge(104), edge(102), edge(105)],
                    selection_group.id,
                )
                .unwrap()
            } else {
                original
            };
            assert_eq!(selection, expected);
            assert!(notes.is_empty());
        }
    }
}

/// A member whose two referenced faces hold the changed edges listed.
fn two_side_operand(
    record_index: u32,
    sides: [(&str, &[i64]); 2],
    deleted: &[i64],
) -> crate::records::topology::edge_identity::DesignEdgeOperand {
    let mut operand = recipe_edge_operand(record_index, &[], &[]);
    let side = || crate::records::topology::edge_recipe::DesignTopologyRecipeSide {
        header_value: 0,
        scalars: Vec::new(),
        payload_prefix: Vec::new(),
        entries: Vec::new(),
    };
    operand.recipe_structure = Some(
        crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
            root: 2,
            sides: vec![side(), side()],
        },
    );
    operand.recipe_reference_contexts = sides
        .iter()
        .enumerate()
        .map(|(ordinal, (face, edges))| {
            serde_json::from_value(serde_json::json!({
                "reference_ordinal": ordinal,
                "result_faces": [],
                "result_shared_edge_slots": [],
                "preceding_faces": [face],
                "shared_edge_slots": [],
                "changed_shared_edge_slots": [],
                "changed_reference_edge_slots": edges,
            }))
            .unwrap()
        })
        .collect();
    operand.transition_deleted_edge_slots = deleted.to_vec();
    operand
}

#[test]
fn a_two_reference_group_selects_the_edges_its_transition_deletes() {
    use crate::design::edge_resolve::shared_reference_deleted_edge_group_candidates;
    let deleted = [10, 11, 12];
    // Two members share the faces of edges 10 and 12; the third names edge 11.
    let operands = [
        two_side_operand(
            1,
            [
                ("f3d:brep:entity#1", &[10, 11, 12]),
                ("f3d:brep:entity#2", &[10, 12]),
            ],
            &deleted,
        ),
        two_side_operand(
            2,
            [
                ("f3d:brep:entity#1", &[10, 11, 12]),
                ("f3d:brep:entity#2", &[10, 12]),
            ],
            &deleted,
        ),
        two_side_operand(
            3,
            [
                ("f3d:brep:entity#1", &[10, 11, 12]),
                ("f3d:brep:entity#3", &[11]),
            ],
            &deleted,
        ),
    ];
    let resolve = |operands: &[crate::records::topology::edge_identity::DesignEdgeOperand]| {
        let operands = operands.iter().collect::<Vec<_>>();
        crate::test_support::with_decode_context(|ctx| {
            shared_reference_deleted_edge_group_candidates(&operands, ctx)
        })
        .unwrap()
    };
    assert_eq!(resolve(&operands), Some(vec![10, 11, 12]));
    // A transition that deletes another edge does not state the group.
    let mut extra = operands.clone();
    for operand in &mut extra {
        operand.transition_deleted_edge_slots = vec![10, 11, 12, 13];
    }
    assert_eq!(resolve(&extra), None);
    // Members that leave a deleted edge uncovered state nothing.
    let mut uncovered = operands.clone();
    uncovered[2] = two_side_operand(
        3,
        [
            ("f3d:brep:entity#1", &[10, 11, 12]),
            ("f3d:brep:entity#2", &[10, 12]),
        ],
        &deleted,
    );
    assert_eq!(resolve(&uncovered), None);
}

fn two_face_treatment_group(
) -> crate::records::topology::construction::DesignConstructionOperandGroup {
    let mut selection_group = group(2, 10);
    selection_group
        .try_set_members(
            [10, 11]
                .into_iter()
                .map(|value| crate::records::identity::Located {
                    value,
                    offset: u64::from(value) * 11,
                })
                .collect(),
        )
        .unwrap();
    selection_group
}

fn resolve_two_face_treatment(
    operands: &[crate::records::topology::edge_identity::DesignEdgeOperand],
) -> (EdgeSelection, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let group = two_face_treatment_group();
    let feature = FeatureId::mint("f3d:model:feature#two-face-treatment").unwrap();
    let mut losses = Vec::new();
    let selection = crate::test_support::with_decode_context(|ctx| {
        super::resolved_edge_treatment_group_with_corners(
            &group,
            crate::design::edge_resolve::EdgeTreatmentInputs {
                groups: std::slice::from_ref(&group),
                operands,
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
    (selection, losses)
}

fn two_face_treatment_operands() -> [crate::records::topology::edge_identity::DesignEdgeOperand; 2]
{
    [10, 11].map(|index| {
        two_side_operand(
            index,
            [
                ("f3d:brep:entity#1", &[10, 11]),
                ("f3d:brep:entity#2", &[10, 11]),
            ],
            &[10, 11],
        )
    })
}

#[test]
fn two_face_deleted_set_preserves_a_resolved_surviving_edge() {
    let mut operands = two_face_treatment_operands();
    operands[0].resolved_edge_slot = Some(99);
    let (selection, losses) = resolve_two_face_treatment(&operands);
    let feature = FeatureId::mint("f3d:model:feature#two-face-treatment").unwrap();
    assert_eq!(
        selection,
        EdgeSelection::historical_partial(
            crate::ids::feature_input_topology_id(&feature, 7),
            vec![crate::ids::history_input_edge_id(
                &crate::ids::feature_input_prefix(&feature, 7),
                99
            )],
            vec![operands[1].id.clone()],
            two_face_treatment_group().id,
        )
        .unwrap()
    );
    assert!(losses.is_empty());
}

#[test]
fn two_face_deleted_set_charges_one_assumption_note_per_group() {
    let operands = two_face_treatment_operands();
    let (selection, losses) = resolve_two_face_treatment(&operands);
    let feature = FeatureId::mint("f3d:model:feature#two-face-treatment").unwrap();
    assert_eq!(
        selection,
        EdgeSelection::historical(
            crate::ids::feature_input_topology_id(&feature, 7),
            [10, 11]
                .into_iter()
                .map(|edge| crate::ids::history_input_edge_id(
                    &crate::ids::feature_input_prefix(&feature, 7),
                    edge
                ))
                .collect(),
            two_face_treatment_group().id.clone(),
        )
        .unwrap()
    );
    assert_eq!(losses.len(), 1);
    let expected = crate::loss::F3dLossCode::HistoricalEdgeSelectionDeletedSetAssumed.note(
        "Group f3d:test:group-2 is read with the assumption that the treated edge set equals the transition-deleted set."
    );
    assert_eq!(losses[0], expected);
}

#[test]
fn exact_two_face_members_charge_no_deleted_set_assumption() {
    let mut operands = two_face_treatment_operands();
    operands[0].resolved_edge_slot = Some(10);
    operands[1].resolved_edge_slot = Some(11);
    let (selection, losses) = resolve_two_face_treatment(&operands);
    assert!(matches!(selection, EdgeSelection::Historical { .. }));
    assert!(losses.is_empty());
}
