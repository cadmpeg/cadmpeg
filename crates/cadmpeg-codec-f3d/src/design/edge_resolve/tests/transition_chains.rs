// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn sole_compact_identity_group_projects_fixed_fillet_transition_chain() {
    let scope = fixed_scope();
    let group = group(2, 10);
    let identity = identity(10, &[(17, 3.0), (18, 5.0), (19, 3.0)]);
    let definition = crate::test_support::with_decode_context(|decode_ctx| {
        project_fixed_fillet(&scope, &[group], &[], &[identity], decode_ctx)
    })
    .unwrap()
    .expect("fixed Fillet from sole compact identity group");
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Fillet { groups },
    ) = definition
    else {
        panic!("expected Fillet");
    };
    assert!(matches!(
        groups.as_slice(),
        [cadmpeg_ir::features::edge_treatments::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Historical { edges, .. },
            radius: cadmpeg_ir::features::edge_treatments::RadiusSpec::Constant {
                radius: actual_radius
            },
            tangency_weight: Some(weight),
        }] if weight.get() == 1.0 && (edges.len() == 2) && actual_radius.get() == 3.0
    ));
}

#[test]
fn full_layout_identity_does_not_assign_the_fixed_fillet_edge_role() {
    let scope = fixed_scope();
    let group = group(2, 10);
    let mut identity = identity(10, &[(17, 3.0), (19, 3.0)]);
    let mut draft = identity.into_draft();
    draft.layout = crate::records::topology::edge_identity::DesignEdgeIdentityLayout::Full;
    draft.asset_id_offset = draft.byte_offset + draft.layout.local_id_offset() + 18;
    draft.context_id_offset = draft.asset_id_offset + 76;
    identity =
        crate::records::topology::edge_identity::DesignEdgeIdentityOperand::try_new(draft).unwrap();

    assert!(
        crate::test_support::with_decode_context(|decode_ctx| project_fixed_fillet(
            &scope,
            &[group],
            &[],
            &[identity],
            decode_ctx
        ))
        .unwrap()
        .is_none()
    );
}

#[test]
fn only_edge_treatments_use_single_member_transition_chains() {
    let group = group(2, 10);
    let mut generic_identity = identity(10, &[(17, 3.0), (19, 3.0)]);
    let mut draft = generic_identity.into_draft();
    draft.layout = crate::records::topology::edge_identity::DesignEdgeIdentityLayout::Full;
    draft.asset_id_offset = draft.byte_offset + draft.layout.local_id_offset() + 18;
    draft.context_id_offset = draft.asset_id_offset + 76;
    generic_identity =
        crate::records::topology::edge_identity::DesignEdgeIdentityOperand::try_new(draft).unwrap();
    generic_identity.treatment_radius_candidates.clear();
    let generic_feature_id =
        cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#ruled-surface")
            .expect("identity grammar");

    assert!(matches!(
        crate::test_support::with_decode_context(|decode_ctx| resolved_edge_group(
            &group,
            std::slice::from_ref(&group),
            &[],
            &[generic_identity],
            Some(7),
            &generic_feature_id,
            decode_ctx
        ))
        .unwrap(),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
    let treatment_feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#fillet")
        .expect("identity grammar");
    let mut identity = identity(10, &[(17, 3.0), (19, 3.0)]);
    let mut draft = identity.into_draft();
    draft.layout = crate::records::topology::edge_identity::DesignEdgeIdentityLayout::Full;
    draft.asset_id_offset = draft.byte_offset + draft.layout.local_id_offset() + 18;
    draft.context_id_offset = draft.asset_id_offset + 76;
    identity =
        crate::records::topology::edge_identity::DesignEdgeIdentityOperand::try_new(draft).unwrap();
    identity.treatment_radius_candidates.clear();
    assert!(matches!(
        resolved_edge_treatment_group(
            &group,
            std::slice::from_ref(&group),
            &[],
            &[identity],
            Some(7),
            &treatment_feature_id,
            None,
        ).unwrap(),
        cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
            if edges.as_slice() == [
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#6:fillet:7:17").expect("identity grammar"),
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#6:fillet:7:19").expect("identity grammar"),
            ]
    ));
}

#[test]
fn multiple_full_layout_members_do_not_use_the_operation_transition_chain() {
    let mut selection_group = group(2, 10);
    selection_group
        .try_set_members(vec![
            crate::records::identity::Located {
                value: 10,
                offset: 0,
            },
            crate::records::identity::Located {
                value: 11,
                offset: 11,
            },
        ])
        .unwrap();
    let mut first = identity(10, &[(17, 1.0), (18, 1.0), (19, 1.0)]);
    let mut draft = first.into_draft();
    draft.layout = crate::records::topology::edge_identity::DesignEdgeIdentityLayout::Full;
    draft.asset_id_offset = draft.byte_offset + draft.layout.local_id_offset() + 18;
    draft.context_id_offset = draft.asset_id_offset + 76;
    first =
        crate::records::topology::edge_identity::DesignEdgeIdentityOperand::try_new(draft).unwrap();
    let mut second = identity(11, &[(17, 1.0), (18, 1.0), (19, 1.0)]);
    let mut draft = second.into_draft();
    draft.layout = crate::records::topology::edge_identity::DesignEdgeIdentityLayout::Full;
    draft.asset_id_offset = draft.byte_offset + draft.layout.local_id_offset() + 18;
    draft.context_id_offset = draft.asset_id_offset + 76;
    second =
        crate::records::topology::edge_identity::DesignEdgeIdentityOperand::try_new(draft).unwrap();
    second.group_member_ordinal = 1;
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#fillet")
        .expect("identity grammar");

    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &[],
            &[first, second],
            Some(7),
            &feature_id,
            None,
        )
        .unwrap(),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
}

#[test]
fn lost_references_preserve_a_complete_compact_transition_chain() {
    let mut selection_group = group(2, 10);
    selection_group
        .try_set_members(vec![
            crate::records::identity::Located {
                value: 10,
                offset: 0,
            },
            crate::records::identity::Located {
                value: 11,
                offset: 11,
            },
        ])
        .unwrap();
    let first_identity = identity(10, &[(17, 1.0), (18, 1.0)]);
    let mut second_identity = identity(11, &[(17, 1.0), (18, 1.0)]);
    second_identity.group_member_ordinal = 1;
    let recipe_operands = [
        recipe_edge_operand(10, &[19], &[19]),
        recipe_edge_operand(11, &[19], &[19]),
    ];
    let feature_id = cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#chamfer")
        .expect("identity grammar");

    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &recipe_operands,
            &[first_identity.clone(), second_identity.clone()],
            Some(7),
            &feature_id,
            None,
        )
        .unwrap(),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));

    selection_group.lost_edge_references = vec!["f3d:test:lost#0".into()];
    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &recipe_operands,
            &[first_identity.clone(), second_identity.clone()],
            Some(7),
            &feature_id,
            None,
        )
        .unwrap(),
        cadmpeg_ir::features::EdgeSelection::Unresolved
    ));

    selection_group
        .lost_edge_references
        .push("f3d:test:lost#1".into());
    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &recipe_operands,
            &[first_identity, second_identity],
            Some(7),
            &feature_id,
            None,
        ).unwrap(),
        cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
            if edges.as_slice() == [
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#7:chamfer:7:17").expect("identity grammar"),
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#7:chamfer:7:18").expect("identity grammar"),
            ]
    ));
}
