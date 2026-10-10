// SPDX-License-Identifier: Apache-2.0
//! Draft membership preserves first-match presence and original native inputs.

use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::entity_selection::DesignEntitySelectionOperand;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn query(
    ctx: &DecodeContext<'_>,
    scope: &DesignParameterScope,
    group: &DesignConstructionOperandGroup,
    operands: &[DesignEntitySelectionOperand],
) -> Result<bool, CodecError> {
    super::super::super::group_has_entity_selection(ctx, scope, group, operands)
}

#[test]
fn entity_membership_keeps_later_member_ordinals_and_each_eligibility_field() {
    let scope = super::scope();
    let group = super::group(77, 0, 100, vec![101, 102], DesignOperandRole::ROLE_0X21);
    let selected = super::selection(102, 1, 600);
    let mut foreign = selected.clone();
    foreign.id = "f3d:Other/BulkStream.dat:entity-selection#102".into();
    let mut wrong_scope = selected.clone();
    wrong_scope.scope_record_index = 78;
    let mut wrong_group = selected.clone();
    wrong_group.group_record_index = 103;
    let mut wrong_ordinal = selected.clone();
    wrong_ordinal.group_member_ordinal = 2;
    let wrong_record = super::selection(103, 1, 600);
    let operands = [foreign, wrong_scope, wrong_group, wrong_ordinal, wrong_record, selected];
    let original = operands.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(query(ctx, &scope, &group, &operands).unwrap());
        assert!(!query(ctx, &scope, &group, &operands[..5]).unwrap());
        assert_eq!(ctx.resource_refusal(), None);
    });
    assert_eq!(operands, original);
    assert_eq!(group.members().iter().map(|row| row.value).collect::<Vec<_>>(), [101, 102]);
}

#[test]
fn entity_membership_first_match_keeps_duplicate_presence_and_unread_tails() {
    let scope = super::scope();
    let mut members = vec![101];
    members.extend([102; 4096]);
    let group = super::group(77, 0, 100, members, DesignOperandRole::ROLE_0X21);
    let first = super::selection(101, 0, 600);
    let mut tail = super::selection(102, 1, 601);
    tail.id = "x".repeat(4096);
    let operands = [first.clone(), first, tail];
    let original = operands.clone();
    let scope_stream = crate::ids::native_stream(&scope.id).unwrap();
    let operand_stream = crate::ids::native_stream(&operands[0].id).unwrap();
    // Actual scope suffix, one member and operand visit, operand suffix, and
    // equal stream bytes. Presence stops with no next/end probe after match.
    let reached = u64::try_from(scope.id.len() - scope_stream.len() + 2
        + operands[0].id.len() - operand_stream.len() + operand_stream.len()).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = reached;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(query(ctx, &scope, &group, &operands).unwrap());
        assert_eq!(ctx.resource_refusal(), None);
    });
    policy.limits.max_work_units -= 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = query(ctx, &scope, &group, &operands) else {
            panic!("one below the actual reached stream comparison must refuse");
        };
        assert_eq!(original.operation, "match F3D group entity operand stream");
        assert_eq!((original.used, original.additional), (reached - 1, 1));
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(matches!(query(ctx, &scope, &group, &[]),
            Err(CodecError::ResourceLimit(later)) if later == original));
    });
    assert_eq!(operands, original);
    assert_eq!(group.members().len(), 4097);
    assert_eq!(group.members()[0].value, 101);
}

#[test]
fn entity_membership_empty_sources_skip_unused_scope_text_and_storage() {
    let mut scope = super::scope();
    scope.id = "x".repeat(4096);
    let empty = super::group(77, 0, 100, vec![], DesignOperandRole::ROLE_0X21);
    let group = super::group(77, 0, 100, vec![101], DesignOperandRole::ROLE_0X21);
    let operands = [super::selection(101, 0, 600)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(!query(ctx, &scope, &empty, &operands).unwrap());
        assert!(!query(ctx, &scope, &group, &[]).unwrap());
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn entity_membership_absent_scope_stream_differs_from_empty_stream() {
    for (scope_id, operand_id, expected) in [
        ("undelimited", "undelimited", false),
        ("undelimited", ":entity-selection#101", false),
        (":scope#77", "undelimited", false),
        (":scope#77", ":entity-selection#101", true),
    ] {
        let mut scope = super::scope();
        scope.id = scope_id.into();
        let group = super::group(77, 0, 100, vec![101], DesignOperandRole::ROLE_0X21);
        let mut operand = super::selection(101, 0, 600);
        operand.id = operand_id.into();
        crate::test_support::with_decode_context(|ctx| {
            assert_eq!(query(ctx, &scope, &group, std::slice::from_ref(&operand)).unwrap(), expected);
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}

#[test]
fn entity_membership_each_actual_phase_keeps_original_refusal_and_native_inputs() {
    let scope = super::scope();
    let group = super::group(77, 0, 100, vec![101], DesignOperandRole::ROLE_0X21);
    let operands = [super::selection(101, 0, 600)];
    let original = operands.clone();
    for operation in [
        "resolve F3D group entity scope stream",
        "visit F3D group entity member",
        "visit F3D group entity operand",
        "match F3D group entity operand stream",
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, Some(1));
            let Err(CodecError::ResourceLimit(refusal)) = query(ctx, &scope, &group, &operands) else {
                panic!("the actual membership phase must refuse");
            };
            assert_eq!(refusal.operation, operation);
            assert_eq!(refusal.additional, 1);
            assert_eq!(ctx.resource_refusal(), Some(refusal.clone()));
            assert!(matches!(query(ctx, &scope, &group, &[]),
                Err(CodecError::ResourceLimit(later)) if later == refusal));
        });
    }
    assert_eq!(operands, original);
}

#[test]
fn entity_membership_missing_scope_admits_the_real_reverse_scan_end() {
    let mut scope = super::scope();
    scope.id = "x".repeat(4096);
    let group = super::group(77, 0, 100, vec![101], DesignOperandRole::ROLE_0X21);
    let mut operand = super::selection(101, 0, 600);
    operand.id = "y".repeat(4096);
    let operands = [operand];
    let original = operands.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 4097;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(!query(ctx, &scope, &group, &operands).unwrap());
        assert_eq!(ctx.resource_refusal(), None);
    });
    policy.limits.max_work_units = 4096;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(refusal)) = query(ctx, &scope, &group, &operands) else {
            panic!("the actual reverse-scan end probe must refuse");
        };
        assert_eq!(refusal.operation, "resolve F3D group entity scope stream");
        assert_eq!((refusal.used, refusal.additional), (4096, 1));
        assert_eq!(ctx.resource_refusal(), Some(refusal.clone()));
    });
    assert_eq!(operands, original);
}
