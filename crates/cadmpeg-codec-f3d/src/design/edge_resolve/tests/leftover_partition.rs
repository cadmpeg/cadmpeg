// SPDX-License-Identifier: Apache-2.0
//! Cross-group partition of one treatment scope's transition edge set.

use super::{group, identity, recipe_edge_operand};
use crate::design::edge_resolve::{
    partition_leftover_treatment_groups, EdgeTreatmentInputs, ScopeTreatmentGroup,
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
    let candidates = TRANSITION.map(|edge| (edge, RADIUS));
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

fn partition(
    scope: &Scope,
    radii: [Option<f64>; 2],
    selections: &mut [EdgeSelection; 2],
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let feature = feature();
    let groups = [
        ScopeTreatmentGroup {
            group: Some(&scope.groups[0]),
            radius: radii[0],
        },
        ScopeTreatmentGroup {
            group: Some(&scope.groups[1]),
            radius: radii[1],
        },
    ];
    let mut losses = Vec::new();
    partition_leftover_treatment_groups(
        &groups,
        selections,
        EdgeTreatmentInputs {
            groups: &scope.groups,
            operands: &scope.operands,
            identity_operands: &scope.identities,
            vertex_operands: &[],
            histories: &[],
            previous_state_id: Some(7),
            feature_id: &feature,
            treatment_radius: None,
        },
        &mut losses,
    );
    losses
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
    other_transition.identities[2].transition_edge_candidates.pop();
    for scope in [resolved, with_recipe, other_transition] {
        let mut selections = [historical(&scope.groups[0].id, &[101]), native(&scope)];
        let losses = partition(&scope, [Some(RADIUS), Some(RADIUS)], &mut selections);
        assert_eq!(selections[1], native(&scope));
        assert!(losses.is_empty());
    }
}

#[test]
fn shared_transition_does_not_prove_a_single_member_group_alone() {
    let scope = scope();
    let resolve = |identities: &[DesignEdgeIdentityOperand]| {
        super::resolved_edge_treatment_group(
            &scope.groups[0],
            &scope.groups,
            &scope.operands,
            identities,
            Some(7),
            &feature(),
            Some(RADIUS),
        )
        .unwrap()
    };
    assert_eq!(
        resolve(&scope.identities[..1]),
        historical(&scope.groups[0].id, &TRANSITION)
    );
    assert_ne!(
        resolve(&scope.identities),
        historical(&scope.groups[0].id, &TRANSITION)
    );
}
