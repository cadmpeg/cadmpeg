// SPDX-License-Identifier: Apache-2.0
//! Borrowed fixed-Fillet query backing ends before final output storage.

use super::*;
use cadmpeg_core::CodecError;

const QUERY_SCRATCH_BYTES: u64 = 4096;

fn variable_scope() -> DesignParameterScope {
    let mut value = serde_json::to_value(fixed_scope()).unwrap();
    let group = &mut value["fixed_fillet_parameters"]["groups"][0];
    group["radii"] = serde_json::json!([1.0, 2.0, 3.0]);
    group["radius_record_indexes"] = serde_json::json!([5, 6, 7]);
    group["radius_offsets"] = serde_json::json!([0, 8, 16]);
    group["intermediate_parameters"] = serde_json::json!([0.5]);
    group["intermediate_parameter_record_indexes"] = serde_json::json!([8]);
    group["intermediate_parameter_offsets"] = serde_json::json!([24]);
    serde_json::from_value(value).unwrap()
}

fn exact_operand(record: u32, edge: i64) -> DesignEdgeOperand {
    let mut operand = recipe_edge_operand(record, &[], &[]);
    operand.recipe_state_id = Some(7);
    operand.resolved_edge_slot = Some(edge);
    operand
}

#[test]
fn fixed_fillet_temporary_rejected_scope_and_complete_queries_refund_backing() {
    let scope = fixed_scope();
    let groups = [group(2, 10), group(3, 11)];
    let operands = [exact_operand(10, 17), exact_operand(11, 19)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = QUERY_SCRATCH_BYTES;
    for complete in [false, true] {
        crate::test_support::with_decode_policy(&policy, |ctx| {
            for _ in 0..7 {
                let input = if complete { operands.as_slice() } else { &[] };
                assert!(project_fixed_fillet(&scope, &groups, input, &[], ctx)
                    .unwrap()
                    .is_none());
                // All query vectors are gone. Their actual backing must have
                // been released before the entire scratch allowance is reused.
                let all = ctx
                    .reserve_scoped(QUERY_SCRATCH_BYTES, "reuse rejected fixed fillet scratch")
                    .unwrap();
                drop(all);
            }
            ctx.charge_retained(0, "rejected fixed fillet output")
                .unwrap();
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}

#[test]
fn fixed_fillet_temporary_rejected_identity_prefix_refunds_backing() {
    let scope = fixed_scope();
    let mut group = group(2, 10);
    group
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
    let identity = identity(10, &[(17, 3.0)]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = QUERY_SCRATCH_BYTES;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..7 {
            assert!(project_fixed_fillet(
                &scope,
                std::slice::from_ref(&group),
                &[],
                std::slice::from_ref(&identity),
                ctx
            )
            .unwrap()
            .is_none());
            let all = ctx
                .reserve_scoped(
                    QUERY_SCRATCH_BYTES,
                    "reuse rejected fixed fillet identity scratch",
                )
                .unwrap();
            drop(all);
        }
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn fixed_fillet_temporary_variable_identity_fallback_skips_unused_sample_construction() {
    let scope = variable_scope();
    let group = group(2, 10);
    let identity = identity(10, &[(17, 3.0)]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = QUERY_SCRATCH_BYTES;
    // The one borrowed scope-group slot is useful. This fallback accepts only
    // a constant law, so no raw or typed radius sample slot is constructed.
    policy.limits.max_collection_items = 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(project_fixed_fillet(
            &scope,
            std::slice::from_ref(&group),
            &[],
            std::slice::from_ref(&identity),
            ctx
        )
        .unwrap()
        .is_none());
        let all = ctx
            .reserve_scoped(
                QUERY_SCRATCH_BYTES,
                "reuse skipped variable fixed fillet scratch",
            )
            .unwrap();
        drop(all);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn fixed_fillet_temporary_discarded_radius_proof_does_not_charge_final_retained_storage() {
    let scope = fixed_scope();
    let group = group(2, 10);
    let identity = identity(10, &[(17, 3.0), (18, 5.0), (19, 3.0)]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = QUERY_SCRATCH_BYTES;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = project_fixed_fillet(
            &scope,
            std::slice::from_ref(&group),
            &[],
            std::slice::from_ref(&identity),
            ctx,
        ) else {
            panic!("final feature identity needs retained storage");
        };
        assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(original.operation, "f3d feature identifier");
        assert_eq!(original.used, 0);
        assert!(original.additional > 0);
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(
            matches!(project_fixed_fillet(&scope, &[], &[], &[], ctx), Err(CodecError::ResourceLimit(later)) if later == original)
        );
    });
}

#[test]
fn fixed_fillet_temporary_each_actual_query_backing_refuses_one_below_its_peak() {
    let scope = fixed_scope();
    let group = group(2, 10);
    let operand = exact_operand(10, 17);
    let identity = identity(10, &[(17, 3.0)]);
    // Core's actual amortized reference vector starts with four pointer slots.
    // The complete and identity vectors overlap the live scope vector.
    let backing =
        u64::try_from(4 * std::mem::size_of::<&DesignConstructionOperandGroup>()).unwrap();
    for (operation, complete, ceiling, used) in [
        ("f3d fixed fillet scope group", false, backing - 1, 0),
        (
            "f3d fixed fillet complete group",
            true,
            2 * backing - 1,
            backing,
        ),
        ("f3d fixed fillet identity", false, 2 * backing - 1, backing),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = ceiling;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let operands = if complete {
                std::slice::from_ref(&operand)
            } else {
                &[]
            };
            let identities = if complete {
                &[]
            } else {
                std::slice::from_ref(&identity)
            };
            let Err(CodecError::ResourceLimit(original)) = project_fixed_fillet(
                &scope,
                std::slice::from_ref(&group),
                operands,
                identities,
                ctx,
            ) else {
                panic!("actual query backing must refuse at {operation}");
            };
            assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(original.operation, operation);
            assert_eq!((original.used, original.additional), (used, backing));
            assert_eq!(ctx.resource_refusal(), Some(original));
            assert!(
                matches!(project_fixed_fillet(&scope, &[], &[], &[], ctx), Err(CodecError::ResourceLimit(later)) if later == original)
            );
        });
    }
}

#[test]
fn fixed_fillet_temporary_final_fillet_keeps_native_identity_radius_and_edge_order() {
    let scope = fixed_scope();
    let group = group(2, 10);
    let identity = identity(10, &[(17, 3.0), (18, 5.0), (19, 3.0)]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = QUERY_SCRATCH_BYTES;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let definition = project_fixed_fillet(
            &scope,
            std::slice::from_ref(&group),
            &[],
            std::slice::from_ref(&identity),
            ctx,
        )
        .unwrap()
        .unwrap();
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Fillet { groups },
        ) = definition
        else {
            panic!("fixed fillet projection");
        };
        let [group] = groups.as_slice() else {
            panic!("one exact group");
        };
        assert_eq!(group.tangency_weight.unwrap().get(), 1.0);
        let cadmpeg_ir::features::edge_treatments::RadiusSpec::Constant { radius } = group.radius
        else {
            panic!("constant radius");
        };
        assert_eq!(radius.get(), 3.0);
        let cadmpeg_ir::features::EdgeSelection::Historical { edges, native, .. } = &group.edges
        else {
            panic!("exact predecessor edges");
        };
        assert_eq!(native.as_str(), "f3d:test:group-2");
        // The final separator of "f3d:test:scope" selects stream "f3d:test".
        // Its colon escapes to "%3A": ten bytes, then kind "Fillet" (six),
        // ordinal1 and record1. The predecessor is state7.
        let prefix = crate::ids::history_input_prefix(
            &cadmpeg_ir::identity_key!("10:f3d%3Atest6:Fillet1:1"),
            7,
        );
        assert_eq!(
            edges.as_slice(),
            [
                crate::ids::history_input_edge_id(&prefix, 17),
                crate::ids::history_input_edge_id(&prefix, 19)
            ]
        );
        drop(groups);
        let all = ctx
            .reserve_scoped(
                ctx.policy().limits.max_materialized_bytes,
                "reuse final fixed fillet query scratch",
            )
            .unwrap();
        drop(all);
    });
}
