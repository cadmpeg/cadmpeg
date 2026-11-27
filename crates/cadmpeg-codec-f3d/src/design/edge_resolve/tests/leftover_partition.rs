// SPDX-License-Identifier: Apache-2.0
//! Cross-group partition of one treatment scope's transition edge set.

use super::{group, identity, recipe_edge_operand};
use crate::design::edge_resolve::{
    partition_leftover_treatment_groups, project_fixed_fillet_with_corners,
    resolved_fillet_scope_groups, ScopeGroupSelection, ScopeTreatmentGroup, ScopeTreatmentInputs,
};
use crate::records::identity::Located;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::edge_identity::{
    DesignEdgeIdentityLayout, DesignEdgeIdentityOperand, DesignEdgeOperand,
};
use cadmpeg_ir::features::selections::EdgeSelection;
use cadmpeg_ir::features::FeatureId;

const RADIUS: f64 = 0.5;
const TRANSITION: [i64; 5] = [101, 102, 103, 104, 105];

fn feature() -> FeatureId {
    FeatureId::mint("f3d:model:feature#leftover-partition").unwrap()
}

fn historical(native: &str, slots: &[i64]) -> EdgeSelection {
    let feature = feature();
    EdgeSelection::historical(
        crate::ids::feature_input_topology_id(&feature, 7),
        slots
            .iter()
            .map(|slot| {
                crate::ids::history_input_edge_id(
                    &crate::ids::feature_input_prefix(&feature, 7),
                    *slot,
                )
            })
            .collect(),
        native.to_owned(),
    )
    .unwrap()
}

fn group_with_members(record_index: u32, members: &[u32]) -> DesignConstructionOperandGroup {
    let mut selection_group = group(record_index, members[0]);
    selection_group
        .try_set_members(
            members
                .iter()
                .map(|&value| Located {
                    value,
                    offset: u64::from(value) * 11,
                })
                .collect(),
        )
        .unwrap();
    selection_group
}

fn transition_identity(record_index: u32, group_record_index: u32) -> DesignEdgeIdentityOperand {
    transition_identity_at(record_index, group_record_index, RADIUS)
}

fn transition_identity_at(
    record_index: u32,
    group_record_index: u32,
    radius: f64,
) -> DesignEdgeIdentityOperand {
    let candidates = TRANSITION.map(|edge| (edge, radius));
    let mut draft = identity(record_index, &candidates).into_draft();
    draft.group_record_index = group_record_index;
    draft.layout = DesignEdgeIdentityLayout::Full;
    draft.asset_id_offset = draft.byte_offset + draft.layout.local_id_offset() + 18;
    draft.context_id_offset = draft.asset_id_offset + 76;
    DesignEdgeIdentityOperand::try_new(draft).unwrap()
}

/// A one-member recipe group 2 and a two-member identity group 3 whose
/// identities carry the five-edge transition set.
struct Scope {
    groups: [DesignConstructionOperandGroup; 2],
    operands: Vec<DesignEdgeOperand>,
    identities: Vec<DesignEdgeIdentityOperand>,
}

fn scope() -> Scope {
    Scope {
        groups: [group(2, 10), group_with_members(3, &[20, 21])],
        operands: vec![recipe_edge_operand(10, &[], &TRANSITION)],
        identities: vec![
            transition_identity(10, 2),
            transition_identity(20, 3),
            transition_identity(21, 3),
        ],
    }
}

fn inputs<'a>(scope: &'a Scope, feature: &'a FeatureId) -> ScopeTreatmentInputs<'a> {
    ScopeTreatmentInputs {
        groups: &scope.groups,
        operands: &scope.operands,
        identity_operands: &scope.identities,
        vertex_operands: &[],
        histories: &[],
        previous_state_id: Some(7),
        feature_id: feature,
    }
}

fn scope_groups(scope: &Scope, radii: [Option<f64>; 2]) -> [ScopeTreatmentGroup<'_>; 2] {
    [
        ScopeTreatmentGroup {
            group: Some(&scope.groups[0]),
            radius: radii[0],
        },
        ScopeTreatmentGroup {
            group: Some(&scope.groups[1]),
            radius: radii[1],
        },
    ]
}

fn partition(
    scope: &Scope,
    radii: [Option<f64>; 2],
    selections: &mut [EdgeSelection; 2],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let feature = feature();
    let mut resolved = selections.each_mut().map(|edges| ScopeGroupSelection {
        edges: std::mem::replace(edges, EdgeSelection::Unresolved),
        losses: Vec::new(),
    });
    partition_leftover_treatment_groups(
        &scope_groups(scope, radii),
        &mut resolved,
        inputs(scope, &feature),
    );
    let mut losses = Vec::new();
    for (selection, resolved) in selections.iter_mut().zip(resolved) {
        *selection = resolved.edges;
        losses.extend(resolved.losses);
    }
    losses
}

/// The selections and losses of a scope read through the Fillet scope
/// resolution.
fn resolve_scope(scope: &Scope) -> ([EdgeSelection; 2], Vec<cadmpeg_ir::report::loss::LossNote>) {
    let feature = feature();
    let resolved = crate::test_support::with_decode_context(|ctx| {
        resolved_fillet_scope_groups(
            &scope_groups(scope, [Some(RADIUS), Some(RADIUS)]),
            inputs(scope, &feature),
            ctx,
        )
    })
    .unwrap();
    let mut losses = Vec::new();
    let mut selections = Vec::new();
    for selection in resolved {
        let selection = selection.expect("every group is present");
        losses.extend(selection.losses);
        selections.push(selection.edges);
    }
    (selections.try_into().unwrap(), losses)
}

fn leftover_loss(native: &str) -> cadmpeg_ir::report::loss::LossNote {
    crate::loss::F3dLossCode::HistoricalEdgeSelectionLeftoverAssumed.note(format!(
        "Group {native} is read with the assumption that it takes the transition edges that the other groups of its scope do not take."
    ))
}

fn native(scope: &Scope) -> EdgeSelection {
    EdgeSelection::Native(scope.groups[1].id.clone())
}

#[test]
fn native_identity_group_takes_the_transition_edges_of_no_other_group() {
    let scope = scope();
    let mut selections = [historical(&scope.groups[0].id, &[101, 103]), native(&scope)];
    let losses = partition(&scope, [Some(RADIUS), Some(RADIUS)], &mut selections);
    assert_eq!(
        selections[1],
        historical(&scope.groups[1].id, &[102, 104, 105])
    );
    assert_eq!(
        losses,
        vec![crate::loss::F3dLossCode::HistoricalEdgeSelectionLeftoverAssumed.note(
            "Group f3d:test:group-3 is read with the assumption that it takes the transition edges that the other groups of its scope do not take."
        )]
    );
}

#[test]
fn lost_sibling_group_leaves_the_whole_transition_to_the_native_group() {
    let mut scope = scope();
    scope.groups[0].lost_edge_references = vec!["f3d:test:lost-edge-reference#1".to_owned()];
    let mut selections = [EdgeSelection::Unresolved, native(&scope)];
    let losses = partition(&scope, [Some(RADIUS), Some(RADIUS)], &mut selections);
    assert_eq!(selections[1], historical(&scope.groups[1].id, &TRANSITION));
    assert_eq!(losses.len(), 1);
}

#[test]
fn lost_sibling_group_keeps_the_edges_it_resolves() {
    let mut scope = scope();
    scope.groups[0].lost_edge_references = vec!["f3d:test:lost-edge-reference#1".to_owned()];
    let mut selections = [historical(&scope.groups[0].id, &[101, 102]), native(&scope)];
    partition(&scope, [Some(RADIUS), Some(RADIUS)], &mut selections);
    assert_eq!(
        selections[1],
        historical(&scope.groups[1].id, &[103, 104, 105])
    );
}

#[test]
fn leftover_partition_refuses_a_transition_group_outside_the_scope_list() {
    let mut scope = scope();
    scope.identities.push(transition_identity(30, 4));
    let mut selections = [historical(&scope.groups[0].id, &[101]), native(&scope)];
    let losses = partition(&scope, [Some(RADIUS), Some(RADIUS)], &mut selections);
    assert_eq!(selections[1], native(&scope));
    assert!(losses.is_empty());
}

#[test]
fn leftover_partition_requires_complete_sibling_selections() {
    let scope = scope();
    for sibling in [
        EdgeSelection::Unresolved,
        EdgeSelection::Native(scope.groups[0].id.clone()),
        EdgeSelection::historical_partial(
            crate::ids::feature_input_topology_id(&feature(), 7),
            vec![crate::ids::history_input_edge_id(
                &crate::ids::feature_input_prefix(&feature(), 7),
                101,
            )],
            vec![scope.operands[0].id.clone()],
            scope.groups[0].id.clone(),
        )
        .unwrap(),
    ] {
        let mut selections = [sibling, native(&scope)];
        let losses = partition(&scope, [Some(RADIUS), Some(RADIUS)], &mut selections);
        assert_eq!(selections[1], native(&scope));
        assert!(losses.is_empty());
    }
}

#[test]
fn leftover_partition_refuses_sibling_edges_outside_the_transition() {
    let scope = scope();
    let mut selections = [historical(&scope.groups[0].id, &[101, 199]), native(&scope)];
    let losses = partition(&scope, [Some(RADIUS), Some(RADIUS)], &mut selections);
    assert_eq!(selections[1], native(&scope));
    assert!(losses.is_empty());
}

#[test]
fn leftover_partition_requires_one_edge_per_member() {
    let scope = scope();
    let mut selections = [
        historical(&scope.groups[0].id, &[101, 102, 103, 104]),
        native(&scope),
    ];
    let losses = partition(&scope, [Some(RADIUS), Some(RADIUS)], &mut selections);
    assert_eq!(selections[1], native(&scope));
    assert!(losses.is_empty());
}

#[test]
fn leftover_partition_requires_the_group_radius_on_every_leftover_edge() {
    let scope = scope();
    let mut selections = [historical(&scope.groups[0].id, &[101]), native(&scope)];
    let losses = partition(&scope, [Some(RADIUS), Some(2.0 * RADIUS)], &mut selections);
    assert_eq!(selections[1], native(&scope));
    assert!(losses.is_empty());
}

#[test]
fn leftover_partition_refuses_resolved_or_recipe_backed_target_members() {
    let mut resolved = scope();
    resolved.identities[1].resolved_edge_slot = Some(102);
    let mut with_recipe = scope();
    with_recipe
        .operands
        .push(recipe_edge_operand(20, &[], &TRANSITION));
    let mut other_transition = scope();
    other_transition.identities[2]
        .transition_edge_candidates
        .pop();
    for scope in [resolved, with_recipe, other_transition] {
        let mut selections = [historical(&scope.groups[0].id, &[101]), native(&scope)];
        let losses = partition(&scope, [Some(RADIUS), Some(RADIUS)], &mut selections);
        assert_eq!(selections[1], native(&scope));
        assert!(losses.is_empty());
    }
}

#[test]
fn bounded_face_group_keeps_the_edge_that_an_edge_group_selects() {
    let mut scope = scope();
    scope.identities[1].resolved_edge_slots = vec![101, 102, 103];
    scope.identities[2].resolved_edge_slots = vec![101, 104, 105];
    let face_group = super::resolved_edge_treatment_group(
        &scope.groups[1],
        &scope.groups,
        &scope.operands,
        &scope.identities,
        Some(7),
        &feature(),
        Some(RADIUS),
    )
    .unwrap();
    let face_selection = historical(&scope.groups[1].id, &[101, 102, 103, 104, 105]);
    assert_eq!(face_group, face_selection);
    let mut selections = [historical(&scope.groups[0].id, &[101]), face_group];
    let losses = partition(&scope, [Some(RADIUS), Some(RADIUS)], &mut selections);
    assert_eq!(selections[1], face_selection);
    assert!(losses.is_empty());
}

#[test]
fn leftover_partition_requires_a_constant_target_radius() {
    let scope = scope();
    let mut selections = [historical(&scope.groups[0].id, &[101]), native(&scope)];
    let losses = partition(&scope, [Some(RADIUS), None], &mut selections);
    assert_eq!(selections[1], native(&scope));
    assert!(losses.is_empty());
}

#[test]
fn leftover_partition_requires_distinct_target_members() {
    let mut scope = scope();
    scope.groups[1]
        .try_set_members([0, 11].map(|offset| Located { value: 20, offset }).to_vec())
        .unwrap();
    scope.identities.pop();
    let mut selections = [historical(&scope.groups[0].id, &[101]), native(&scope)];
    let losses = partition(&scope, [Some(RADIUS), Some(RADIUS)], &mut selections);
    assert_eq!(selections[1], native(&scope));
    assert!(losses.is_empty());
}

#[test]
fn lost_sibling_group_contributes_its_partial_selection() {
    let mut scope = scope();
    scope.groups[0].lost_edge_references = vec!["f3d:test:lost-edge-reference#1".to_owned()];
    let partial = EdgeSelection::historical_partial(
        crate::ids::feature_input_topology_id(&feature(), 7),
        vec![crate::ids::history_input_edge_id(
            &crate::ids::feature_input_prefix(&feature(), 7),
            101,
        )],
        vec!["f3d:test:lost-edge-reference#1".to_owned()],
        scope.groups[0].id.clone(),
    )
    .unwrap();
    let mut selections = [partial, native(&scope)];
    let losses = partition(&scope, [Some(RADIUS), Some(RADIUS)], &mut selections);
    assert_eq!(
        selections[1],
        historical(&scope.groups[1].id, &[102, 103, 104, 105])
    );
    assert_eq!(losses, vec![leftover_loss(&scope.groups[1].id)]);
}

/// Two one-member groups whose identities carry the five-edge transition
/// set; the first identity names edge 101 exactly when `exact` holds.
fn one_member_scope(exact: bool) -> Scope {
    let mut first = transition_identity(10, 2);
    first.resolved_edge_slot = exact.then_some(101);
    Scope {
        groups: [group(2, 10), group(3, 20)],
        operands: Vec::new(),
        identities: vec![first, transition_identity(20, 3)],
    }
}

#[test]
fn fillet_scope_gives_the_native_group_the_leftover_of_a_shared_transition() {
    let scope = one_member_scope(true);
    let (selections, losses) = resolve_scope(&scope);
    assert_eq!(
        selections,
        [
            historical(&scope.groups[0].id, &[101]),
            historical(&scope.groups[1].id, &[102, 103, 104, 105]),
        ]
    );
    assert_eq!(losses, vec![leftover_loss(&scope.groups[1].id)]);
}

#[test]
fn fillet_scope_without_a_leftover_partition_keeps_radius_candidate_proofs() {
    let scope = one_member_scope(false);
    let (selections, losses) = resolve_scope(&scope);
    assert_eq!(
        selections,
        [
            historical(&scope.groups[0].id, &TRANSITION),
            historical(&scope.groups[1].id, &TRANSITION),
        ]
    );
    assert!(losses.is_empty());
}

#[test]
fn fixed_fillet_pair_gives_the_face_group_the_leftover_of_a_shared_transition() {
    let fixed = super::fixed_scope();
    let feature = crate::ids::neutral_feature_id(&fixed);
    let radius = 3.0;
    let mut edge_group = group(2, 10);
    edge_group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            crate::records::topology::extrude_selection::DesignOperandRole::BODIES_B,
        );
    let face_group = group(3, 20);
    let mut edge_identity = transition_identity_at(10, 2, radius);
    edge_identity.resolved_edge_slot = Some(101);
    let groups = [edge_group, face_group];
    let identities = [edge_identity, transition_identity_at(20, 3, radius)];
    let mut losses = Vec::new();
    let definition = crate::test_support::with_decode_context(|ctx| {
        project_fixed_fillet_with_corners(
            &fixed,
            &groups,
            &[],
            &identities,
            &[],
            &[],
            (ctx, &mut losses),
        )
    })
    .unwrap()
    .expect("fixed Fillet pair");
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Fillet { groups: fillet },
    ) = definition
    else {
        panic!("expected Fillet");
    };
    let expected = |native: &str, slots: &[i64]| {
        EdgeSelection::historical(
            crate::ids::feature_input_topology_id(&feature, 7),
            slots
                .iter()
                .map(|slot| {
                    crate::ids::history_input_edge_id(
                        &crate::ids::feature_input_prefix(&feature, 7),
                        *slot,
                    )
                })
                .collect(),
            native.to_owned(),
        )
        .unwrap()
    };
    assert_eq!(
        fillet.iter().map(|group| &group.edges).collect::<Vec<_>>(),
        [
            &expected(&groups[0].id, &[101]),
            &expected(&groups[1].id, &[102, 103, 104, 105]),
        ]
    );
    assert_eq!(losses, vec![leftover_loss(&groups[1].id)]);
}
