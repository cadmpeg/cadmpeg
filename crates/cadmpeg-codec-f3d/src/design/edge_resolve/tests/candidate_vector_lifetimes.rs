// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use super::{identity, recipe_edge_operand, recipe_reference};
use cadmpeg_core::decode::{refusal_probe::RefusalProbe, DecodePolicy, ResourceDimension};

fn retained(ctx: &DecodeContext<'_>, slots: usize) {
    let Err(CodecError::ResourceLimit(original)) =
        ctx.charge_retained(u64::MAX, "measure edge candidate survivor backing")
    else {
        panic!("retained measurement must refuse");
    };
    assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(
        original.used,
        u64::try_from(slots * std::mem::size_of::<i64>()).unwrap()
    );
    assert_eq!(ctx.resource_refusal(), Some(original));
}

fn no_backing(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy
}

fn context(
    edges: &[i64],
) -> crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
    serde_json::from_value(serde_json::json!({
        "reference_ordinal": 0, "result_faces": [], "result_shared_edge_slots": [],
        "preceding_faces": [], "shared_edge_slots": [], "changed_shared_edge_slots": [],
        "changed_reference_edge_slots": edges,
    }))
    .unwrap()
}

fn boundary(record: u32, deleted: &[i64], references: &[&[i64]]) -> DesignEdgeOperand {
    let mut operand = recipe_edge_operand(record, deleted, deleted);
    operand.preceding_boundary_edge_slots = deleted.to_vec();
    operand.recipe_structure = Some(
        crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
            root: 2,
            sides: Vec::new(),
        },
    );
    operand.recipe_references = references.iter().map(|_| recipe_reference(&[])).collect();
    operand.recipe_reference_contexts = references.iter().map(|edges| context(edges)).collect();
    operand
}

fn result_operand(references: &[&[i64]]) -> DesignEdgeOperand {
    let mut operand = boundary(10, &[], references);
    operand.result_boundary_edge_slots = vec![19];
    operand.recipe_structure = Some(
        crate::records::topology::edge_recipe::DesignEdgeRecipeStructure {
            root: 2,
            sides: (0..2)
                .map(
                    |_| crate::records::topology::edge_recipe::DesignTopologyRecipeSide {
                        header_value: 2,
                        scalars: vec![1, 0],
                        payload_prefix: vec![0],
                        entries: Vec::new(),
                    },
                )
                .collect(),
        },
    );
    operand
}

fn selector(
    edges: &[i64],
) -> crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext {
    crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext {
        selector: 0,
        clauses: vec![None, None],
        incidence_matching_edge_slots: Vec::new(),
        boundary_count_matching_edge_slots: edges.to_vec(),
    }
}

#[test]
fn edge_candidate_lifetime_transition_boolean_refunds_proof_backing() {
    let first = recipe_edge_operand(10, &[17], &[17]);
    let second = recipe_edge_operand(11, &[], &[]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = u64::try_from(4 * std::mem::size_of::<i64>()).unwrap();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert!(!transition_chain_is_supported_by_recipe(
                &[17, 18],
                2,
                &[&first, &second],
                ctx
            )
            .unwrap());
        }
        retained(ctx, 0);
    });
}

#[test]
fn edge_candidate_lifetime_assignment_intersection_retains_actual_capacity() {
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            edge_group_assignment_candidates(
                &[],
                [Some(&[18, 17, 17][..]), Some(&[17, 18][..])],
                ctx
            )
            .unwrap(),
            Some(EdgeAssignmentCandidates::Edges(vec![17, 18]))
        );
        retained(ctx, 4);
    });
}

#[test]
fn edge_candidate_lifetime_rejected_assignment_intersection_refunds_backing() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = u64::try_from(4 * std::mem::size_of::<i64>()).unwrap();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert_eq!(
                edge_group_assignment_candidates(&[], [Some(&[17, 17][..]), Some(&[18][..])], ctx)
                    .unwrap(),
                None
            );
        }
        retained(ctx, 0);
    });
}

#[test]
fn edge_candidate_lifetime_assignment_scalar_refuses_before_backing() {
    crate::test_support::with_decode_policy(&no_backing(2), |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            edge_group_assignment_candidates(&[], [Some(&[17][..]), Some(&[17][..])], ctx)
        else {
            panic!("first scalar visit must refuse");
        };
        assert_eq!(original.operation, "scan F3D assignment reference edges");
        assert_eq!((original.used, original.additional), (2, 1));
    });
}

#[test]
fn edge_candidate_lifetime_radius_unmatched_prefix_is_paid_without_backing() {
    let candidates = vec![(17, 5.0); 1024];
    let operand = identity(10, &candidates);
    crate::test_support::with_decode_policy(&no_backing(3), |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            radius_edge_identity_group_candidates(&[&operand], 3.0, ctx)
        else {
            panic!("third radius predicate must refuse");
        };
        assert_eq!(original.operation, "scan F3D radius identity candidates");
        assert_eq!((original.used, original.additional), (3, 1));
    });
}

#[test]
fn edge_candidate_lifetime_radius_rejection_refunds_each_actual_contribution() {
    let mut first = identity(10, &[]);
    first.resolved_edge_slot = Some(17);
    let second = identity(11, &[]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = u64::try_from(8 * std::mem::size_of::<i64>()).unwrap();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert_eq!(
                radius_edge_identity_group_candidates(&[&first, &second], 3.0, ctx).unwrap(),
                None
            );
        }
        retained(ctx, 0);
    });
}

#[test]
fn edge_candidate_lifetime_radius_multiple_members_skip_radius_arrays() {
    let candidates = vec![(99, 3.0); 1024];
    let mut first = identity(10, &candidates);
    first.resolved_edge_slot = Some(17);
    let mut second = identity(11, &candidates);
    second.resolved_edge_slot = Some(18);
    crate::test_support::with_decode_context(|ctx| {
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "scan F3D radius identity candidates",
            None,
        );
        assert_eq!(
            radius_edge_identity_group_candidates(&[&first, &second], 3.0, ctx).unwrap(),
            Some(vec![17, 18])
        );
        retained(ctx, 4);
    });
}

#[test]
fn edge_candidate_lifetime_common_rejection_refunds_actual_overlap() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = u64::try_from(8 * std::mem::size_of::<i64>()).unwrap();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert_eq!(
                common_deleted_edge_group_candidates(
                    [(true, &[17, 18][..]), (true, &[17, 19][..])],
                    ctx
                )
                .unwrap(),
                None
            );
        }
        retained(ctx, 0);
    });
}

#[test]
fn edge_candidate_lifetime_common_overlap_one_below_preserves_original_refusal() {
    let bytes = u64::try_from(4 * std::mem::size_of::<i64>()).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 2 * bytes - 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = common_deleted_edge_group_candidates(
            [(true, &[17, 18][..]), (true, &[17, 19][..])],
            ctx,
        ) else {
            panic!("actual second overlapping backing must refuse");
        };
        assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(original.operation, "f3d common deleted normalized edge");
        assert_eq!((original.used, original.additional), (bytes, bytes));
    });
}

#[test]
fn edge_candidate_lifetime_common_non_edge_prefix_is_paid_without_backing() {
    let members = vec![(false, &[][..]); 1024];
    crate::test_support::with_decode_policy(&no_backing(3), |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            common_deleted_edge_group_candidates(members.iter().copied(), ctx)
        else {
            panic!("fourth rejected member visit must refuse");
        };
        assert_eq!(original.operation, "scan F3D common deleted members");
        assert_eq!((original.used, original.additional), (3, 1));
    });
}

#[test]
fn edge_candidate_lifetime_common_success_keeps_only_candidate_capacity() {
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            common_deleted_edge_group_candidates(
                [(true, &[18, 17, 17][..]), (true, &[17, 18][..])],
                ctx
            )
            .unwrap(),
            Some(vec![17, 18])
        );
        retained(ctx, 4);
    });
}

#[test]
fn edge_candidate_lifetime_deleted_success_keeps_only_deleted_capacity() {
    let first = boundary(10, &[17, 18], &[&[17]]);
    let second = boundary(11, &[17, 18], &[&[18]]);
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            deleted_boundary_edge_group_candidates(&[&first, &second], ctx).unwrap(),
            Some(vec![17, 18])
        );
        retained(ctx, 4);
    });
}

#[test]
fn edge_candidate_lifetime_deleted_coverage_rejection_refunds_both_backings() {
    let first = boundary(10, &[17, 18], &[&[17]]);
    let second = boundary(11, &[17, 18], &[&[17]]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert_eq!(
                deleted_boundary_edge_group_candidates(&[&first, &second], ctx).unwrap(),
                None
            );
        }
        retained(ctx, 0);
    });
}

#[test]
fn edge_candidate_lifetime_contextual_assignment_keeps_only_selected_slots() {
    let first = boundary(10, &[], &[&[17]]);
    let second = boundary(11, &[17, 18], &[&[17], &[18]]);
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            contextual_deleted_edge_group_candidates(&[&first, &second], ctx).unwrap(),
            Some(vec![17, 18])
        );
        retained(ctx, 2);
    });
}

#[test]
fn edge_candidate_lifetime_contextual_ambiguity_refunds_all_candidate_backings() {
    let first = boundary(10, &[], &[&[17, 18]]);
    let second = boundary(11, &[17, 18], &[&[17, 18]]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert_eq!(
                contextual_deleted_edge_group_candidates(&[&first, &second], ctx).unwrap(),
                None
            );
        }
        retained(ctx, 0);
    });
}

#[test]
fn edge_candidate_lifetime_result_empty_context_prefix_is_paid_without_backing() {
    let references = vec![&[][..]; 1024];
    let operand = result_operand(&references);
    crate::test_support::with_decode_policy(&no_backing(2), |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            result_boundary_reference_edge_group_candidates(&[&operand], ctx)
        else {
            panic!("third empty context must refuse");
        };
        assert_eq!(original.operation, "scan F3D result boundary contexts");
        assert_eq!((original.used, original.additional), (2, 1));
    });
}

#[test]
fn edge_candidate_lifetime_result_success_keeps_actual_candidate_capacity() {
    let operand = result_operand(&[&[19], &[19]]);
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            result_boundary_reference_edge_group_candidates(&[&operand], ctx).unwrap(),
            Some(vec![19])
        );
        retained(ctx, 4);
    });
}

#[test]
fn edge_candidate_lifetime_result_single_support_refunds_candidates() {
    let operand = result_operand(&[&[19], &[]]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert_eq!(
                result_boundary_reference_edge_group_candidates(&[&operand], ctx).unwrap(),
                None
            );
        }
        retained(ctx, 0);
    });
}

#[test]
fn edge_candidate_lifetime_boundary_count_empty_scalar_prefix_is_paid() {
    let members = vec![vec![selector(&[])]; 1024];
    crate::test_support::with_decode_policy(&no_backing(3), |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            changed_boundary_count_edge_group_candidates(members.iter().map(Vec::as_slice), ctx)
        else {
            panic!("second empty selector visit must refuse");
        };
        assert_eq!(original.operation, "scan F3D boundary count selectors");
        assert_eq!((original.used, original.additional), (3, 1));
    });
}

#[test]
fn edge_candidate_lifetime_boundary_count_rejection_refunds_candidates() {
    let first = [selector(&[17])];
    let second = [selector(&[17])];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert_eq!(
                changed_boundary_count_edge_group_candidates(
                    [first.as_slice(), second.as_slice()],
                    ctx
                )
                .unwrap(),
                None
            );
        }
        retained(ctx, 0);
    });
}

#[test]
fn edge_candidate_lifetime_partition_success_keeps_only_actual_target_capacity() {
    let deleted = [17, 18, 19, 20];
    let member = |identity, resolved_edge| EdgeGroupMember {
        identity,
        resolved_edge,
        deleted_boundary_edges: &deleted,
    };
    let groups = [
        vec![member(10, Some(17)), member(11, Some(19))],
        vec![member(12, None), member(13, None)],
    ];
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            partition_unique_incomplete_edge_group(1, &groups, ctx).unwrap(),
            Some(vec![18, 20])
        );
        retained(ctx, 4);
    });
}

#[test]
fn edge_candidate_lifetime_partition_duplicate_identity_refunds_all_backings() {
    let deleted = [17, 18];
    let groups = [
        vec![EdgeGroupMember {
            identity: 10,
            resolved_edge: Some(17),
            deleted_boundary_edges: &deleted,
        }],
        vec![EdgeGroupMember {
            identity: 10,
            resolved_edge: None,
            deleted_boundary_edges: &deleted,
        }],
    ];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert_eq!(
                partition_unique_incomplete_edge_group(1, &groups, ctx).unwrap(),
                None
            );
        }
        retained(ctx, 0);
    });
}

#[test]
fn edge_candidate_lifetime_each_reached_raw_source_preserves_original_refusal() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_materialized_bytes = u64::MAX;
    for (owner, operations) in [
        (
            "transition_chain_is_supported_by_recipe",
            &[
                "scan F3D transition recipe operands",
                "scan F3D transition boundary edges",
            ][..],
        ),
        (
            "edge_group_assignment_candidates",
            &["scan F3D assignment reference edges"][..],
        ),
        (
            "radius_edge_identity_group_candidates",
            &[
                "scan F3D radius identity operands",
                "scan F3D radius identity candidates",
                "scan F3D radius identity contribution edges",
            ][..],
        ),
        (
            "common_deleted_edge_group_candidates",
            &[
                "scan F3D common deleted members",
                "scan F3D first common deleted edges",
                "scan F3D later common deleted edges",
            ][..],
        ),
        (
            "deleted_boundary_edge_group_candidates",
            &[
                "scan F3D deleted boundary operands",
                "scan F3D deleted boundary contexts",
                "scan F3D deleted boundary contextual edges",
                "scan F3D deleted boundary candidates",
            ][..],
        ),
        (
            "contextual_deleted_edge_group_candidates",
            &[
                "scan F3D contextual deleted operands",
                "scan F3D contextual deleted boundary edges",
                "scan F3D contextual deleted candidate operands",
                "scan F3D contextual deleted contexts",
                "scan F3D contextual deleted context edges",
            ][..],
        ),
        (
            "result_boundary_reference_edge_group_candidates",
            &[
                "scan F3D result boundary contexts",
                "scan F3D result boundary context edges",
            ][..],
        ),
        (
            "changed_boundary_count_edge_group_candidates",
            &[
                "scan F3D boundary count members",
                "scan F3D boundary count selectors",
                "scan F3D boundary count edges",
            ][..],
        ),
        (
            "partition_unique_incomplete_edge_group",
            &[
                "scan F3D partition universe groups",
                "scan F3D partition universe members",
                "scan F3D partition member deleted edges",
                "scan F3D partition incomplete groups",
                "scan F3D partition incomplete members",
                "scan F3D partition reserved groups",
                "scan F3D partition reserved members",
                "scan F3D partition universe candidates",
            ][..],
        ),
    ] {
        for &operation in operations {
            crate::test_support::with_decode_policy(&policy, |ctx| {
                super::scalar_edits::run_case(owner, ctx).unwrap();
                let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
                let Err(CodecError::ResourceLimit(original)) =
                    super::scalar_edits::run_case(owner, ctx)
                else {
                    panic!("reached {operation} must refuse");
                };
                assert_eq!(original.operation, operation);
                assert_eq!(ctx.resource_refusal(), Some(original));
            });
        }
    }
}

#[test]
fn edge_candidate_lifetime_transition_reference_scalar_preserves_original_refusal() {
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.recipe_reference_contexts = vec![context(&[17])];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_materialized_bytes = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(transition_chain_is_supported_by_recipe(&[17], 1, &[&operand], ctx).unwrap());
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "scan F3D transition reference edges",
            None,
        );
        let Err(CodecError::ResourceLimit(original)) =
            transition_chain_is_supported_by_recipe(&[17], 1, &[&operand], ctx)
        else {
            panic!("reached reference scalar must refuse");
        };
        assert_eq!(original.operation, "scan F3D transition reference edges");
    });
}

#[test]
fn edge_candidate_lifetime_radius_resolved_array_preserves_original_refusal() {
    let mut operand = identity(10, &[]);
    operand.resolved_edge_slots = vec![17];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_materialized_bytes = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            radius_edge_identity_group_candidates(&[&operand], 3.0, ctx).unwrap(),
            Some(vec![17])
        );
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "scan F3D radius identity resolved slots",
            None,
        );
        let Err(CodecError::ResourceLimit(original)) =
            radius_edge_identity_group_candidates(&[&operand], 3.0, ctx)
        else {
            panic!("reached resolved slot must refuse");
        };
        assert_eq!(
            original.operation,
            "scan F3D radius identity resolved slots"
        );
    });
}

#[test]
fn edge_candidate_lifetime_scope_partition_keeps_only_native_target_capacity() {
    let groups = [super::group(2, 10), super::group(3, 12)];
    let mut source = recipe_edge_operand(10, &[], &[17, 18]);
    source.resolved_edge_slot = Some(17);
    let target = recipe_edge_operand(12, &[], &[17, 18]);
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            scope_partition_edge_group_candidates(
                &groups[1],
                &groups,
                &[source, target],
                groups[1].members(),
                ctx
            )
            .unwrap(),
            Some(vec![18])
        );
        retained(ctx, 4);
    });
}

#[test]
fn edge_candidate_lifetime_boundary_count_success_keeps_actual_candidate_capacity() {
    let first = [selector(&[17])];
    let second = [selector(&[18])];
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            changed_boundary_count_edge_group_candidates(
                [first.as_slice(), second.as_slice()],
                ctx
            )
            .unwrap(),
            Some(vec![17, 18])
        );
        retained(ctx, 4);
    });
}

#[test]
fn edge_candidate_lifetime_single_scope_caller_skips_unused_partition_query() {
    let group = super::group(2, 10);
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.preceding_boundary_edge_slots = vec![17, 18, 19];
    operand.changed_boundary_edge_slots = vec![17, 18];
    operand.updated_boundary_edge_slots = vec![17];
    operand.result_boundary_edge_slots = vec![17, 20];
    let feature =
        cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#main-edge-group").unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_materialized_bytes = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "scan F3D partition scope groups",
            None,
        );
        assert!(
            matches!(resolved_edge_group(&group, std::slice::from_ref(&group),
            std::slice::from_ref(&operand), &[], Some(7), &feature, ctx).unwrap(),
            cadmpeg_ir::features::EdgeSelection::Native(id) if id == group.id)
        );
        assert_eq!(ctx.resource_refusal(), None);
        let selection = resolved_edge_flange_group(
            &group,
            std::slice::from_ref(&group),
            std::slice::from_ref(&operand),
            &[],
            Some(7),
            &feature,
            ctx,
        )
        .unwrap();
        let cadmpeg_ir::features::EdgeSelection::Historical { edges, .. } = selection else {
            panic!("the unchanged native updated edge proves a historical selection");
        };
        let prefix =
            crate::ids::history_input_prefix(&cadmpeg_ir::identity_key!("main-edge-group"), 7);
        assert_eq!(
            edges.as_slice(),
            [crate::ids::history_input_edge_id(&prefix, 17)]
        );
        assert_eq!(ctx.resource_refusal(), None);
        drop(probe);
    });
}
