// SPDX-License-Identifier: Apache-2.0
//! Raw operand-group queries keep the actual reached prefix and source identity.

use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::{DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn query<'source>(
    ctx: &DecodeContext<'_>,
    groups: &'source [DesignConstructionOperandGroup],
    scope: &DesignParameterScope,
) -> Result<Option<&'source DesignConstructionOperandGroup>, CodecError> {
    super::super::super::single_operand_group(ctx, groups, scope, DesignOperandRole::BODIES_A)
}

fn group(record: u32) -> DesignConstructionOperandGroup {
    super::group(77, 0, record, vec![record + 1], DesignOperandRole::BODIES_A)
}

#[test]
fn single_operand_query_preserves_eligibility_order_and_borrowed_native_identity() {
    let scope = super::scope();
    let mut foreign = group(100);
    foreign.id = "f3d:Other/BulkStream.dat:group#100".into();
    let wrong_scope = super::group(78, 0, 101, vec![102], DesignOperandRole::BODIES_A);
    let wrong_role = super::group(77, 0, 102, vec![103], DesignOperandRole::PROFILE);
    let empty = super::group(77, 0, 103, vec![], DesignOperandRole::BODIES_A);
    let selected = group(104);
    let groups = [foreign, wrong_scope, wrong_role, empty, selected];
    let original = groups.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let selected = query(ctx, &groups, &scope).unwrap().unwrap();
        assert!(std::ptr::eq(selected, &raw const groups[4]));
        assert_eq!(selected.id, "f3d:Design/BulkStream.dat:group#104");
        assert_eq!(selected.members()[0].value, 105);
        assert_eq!(selected.scope_reference_ordinal, 0);
        assert_eq!(ctx.resource_refusal(), None);
    });
    assert_eq!(groups, original);
}

#[test]
fn single_operand_query_keeps_absent_and_empty_streams_distinct() {
    for (scope_id, group_id, expected) in [
        ("undelimited", "undelimited", true),
        ("undelimited", ":group#100", false),
        (":scope#77", "undelimited", false),
        (":scope#77", ":group#100", true),
    ] {
        let mut scope = super::scope();
        scope.id = scope_id.into();
        let mut group = group(100);
        group.id = group_id.into();
        crate::test_support::with_decode_context(|ctx| {
            let selected = query(ctx, std::slice::from_ref(&group), &scope).unwrap();
            assert_eq!(selected.is_some(), expected);
            if let Some(selected) = selected {
                assert!(std::ptr::eq(selected, &raw const group));
            }
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}

#[test]
fn single_operand_query_duplicate_stops_before_long_unread_source_tail() {
    let scope = super::scope();
    let first = group(100);
    let second = group(101);
    let mut tail = group(102);
    tail.id = "x".repeat(4096);
    let scope_stream = crate::ids::native_stream(&scope.id).unwrap();
    let group_stream = crate::ids::native_stream(&first.id).unwrap();
    let reached = u64::try_from(
        scope.id.len() - scope_stream.len()
            + 2 * (1 + first.id.len() - group_stream.len() + group_stream.len()),
    )
    .unwrap();
    assert_eq!(first.id.len(), second.id.len());
    let groups = [first, second, tail];
    let original = groups.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = reached;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(query(ctx, &groups, &scope).unwrap().is_none());
        assert_eq!(ctx.resource_refusal(), None);
    });
    policy.limits.max_work_units -= 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = query(ctx, &groups, &scope) else {
            panic!("last reached comparison byte must refuse")
        };
        assert_eq!(original.operation, "match F3D single operand group stream");
        assert_eq!((original.used, original.additional), (reached - 1, 1));
        assert_eq!(ctx.resource_refusal(), Some(original));
        
    });
    assert_eq!(groups, original);
}

#[test]
fn single_operand_query_missing_raw_prefix_and_end_probe_are_admitted() {
    let mut scope = super::scope();
    scope.id = "undelimited".into();
    let mut missing = group(100);
    missing.scope_record_index = 78;
    missing.id = "x".repeat(4096);
    // Both delimiter-free reverse scans include their actual end probes.
    // The raw group visit and failed search end probe each cost one step.
    let reached = u64::try_from(scope.id.len() + 1 + 1 + missing.id.len() + 1 + 1).unwrap();
    let original = missing.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = reached;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(query(ctx, std::slice::from_ref(&missing), &scope)
            .unwrap()
            .is_none());
        assert_eq!(ctx.resource_refusal(), None);
    });
    policy.limits.max_work_units -= 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            query(ctx, std::slice::from_ref(&missing), &scope)
        else {
            panic!("actual raw search end probe must refuse")
        };
        assert_eq!(original.operation, "find F3D single operand group");
        assert_eq!((original.used, original.additional), (reached - 1, 1));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
    assert_eq!(missing, original);
}

#[test]
fn single_operand_query_unique_result_requires_the_actual_second_search_end() {
    let scope = super::scope();
    let group = group(100);
    let scope_stream = crate::ids::native_stream(&scope.id).unwrap();
    let group_stream = crate::ids::native_stream(&group.id).unwrap();
    let reached = u64::try_from(
        scope.id.len() - scope_stream.len() + 1 + group.id.len() - group_stream.len()
            + group_stream.len()
            + 1,
    )
    .unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = reached;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(std::ptr::eq(
            query(ctx, std::slice::from_ref(&group), &scope)
                .unwrap()
                .unwrap(),
            &raw const group
        ));
        assert_eq!(ctx.resource_refusal(), None);
    });
    policy.limits.max_work_units -= 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            query(ctx, std::slice::from_ref(&group), &scope)
        else {
            panic!("unique result requires the real second search end probe")
        };
        assert_eq!(original.operation, "find F3D single operand group");
        assert_eq!((original.used, original.additional), (reached - 1, 1));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn single_operand_query_each_reached_phase_keeps_original_refusal() {
    let scope = super::scope();
    let group = group(100);
    for operation in [
        "resolve F3D single operand group scope stream",
        "find F3D single operand group",
        "match F3D single operand group stream",
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                ResourceDimension::WorkUnits,
                operation,
                Some(1),
            );
            let Err(CodecError::ResourceLimit(original)) =
                query(ctx, std::slice::from_ref(&group), &scope)
            else {
                panic!("actual reached query phase must refuse")
            };
            assert_eq!(original.operation, operation);
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!(original.additional, 1);
            assert_eq!(ctx.resource_refusal(), Some(original));
            
        });
    }
}

#[test]
fn single_operand_query_remove_body_caller_keeps_native_output_and_original_refusal() {
    let scope = super::scope();
    let group = group(100);
    let original = group.clone();
    crate::test_support::with_decode_context(|ctx| {
        let definition =
            super::super::super::project_remove_body(ctx, &scope, std::slice::from_ref(&group))
                .unwrap()
                .unwrap();
        let cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::DeleteBody { bodies, mode },
        ) = definition
        else {
            panic!("exact RemoveBody definition")
        };
        assert_eq!(
            bodies,
            cadmpeg_ir::features::BodySelection::Native(group.id.clone())
        );
        assert_eq!(
            mode,
            cadmpeg_ir::features::BodyRetentionMode::DeleteSelected
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "find F3D single operand group",
            Some(1),
        );
        let Err(CodecError::ResourceLimit(original)) =
            super::super::super::project_remove_body(ctx, &scope, std::slice::from_ref(&group))
        else {
            panic!("actual caller source visit must refuse")
        };
        assert_eq!(original.operation, "find F3D single operand group");
        assert_eq!(ctx.resource_refusal(), Some(original));
        
    });
    assert_eq!(group, original);
}

#[test]
fn single_operand_query_empty_source_does_not_parse_unused_scope_text() {
    let mut scope = super::scope();
    scope.id = "x".repeat(4096);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(query(ctx, &[], &scope).unwrap().is_none());
        assert_eq!(ctx.resource_refusal(), None);
    });
}
