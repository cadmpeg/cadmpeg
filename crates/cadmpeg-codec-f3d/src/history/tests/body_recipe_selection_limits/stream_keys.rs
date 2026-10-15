// SPDX-License-Identifier: Apache-2.0
//! Native stream admission through actual operand-key callbacks.

use crate::history::{member_operand_key, scope_operand_key};
use crate::records::topology::body_recipe::DesignOperandOwner;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn scope_operand_stream_key_admits_only_reached_suffix_and_preserves_refusal() {
    for prefix in [String::new(), "流".to_owned(), "a".repeat(4096)] {
        let (_, _, mut operands, _) = super::selection_fixture();
        let stream = format!("f3d:{prefix}");
        let operand = &mut operands[0];
        operand.id = format!("{stream}:r#21");
        operand.owner = DesignOperandOwner::ScopeReference {
            scope_reference_ordinal: 0,
        };
        let mut policy = DecodePolicy::service();
        // The four suffix bytes and their separator are the only visited bytes.
        policy.limits.max_work_units = 5;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            assert_eq!(
                scope_operand_key(ctx, operand).unwrap(),
                Some((Some(stream.as_str()), 10, 21))
            );
            assert_eq!(operand.id, format!("{stream}:r#21"));
            assert!(
                matches!(ctx.charge_work(1, "after F3D scope stream key"), Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits && limit.used == 5 && limit.additional == 1)
            );
        });
        let CodecError::ResourceLimit(original) = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            "split F3D native stream",
            4,
            |ctx| {
                let CodecError::ResourceLimit(original) =
                    scope_operand_key(ctx, operand).unwrap_err()
                else {
                    panic!("scope stream source refusal");
                };
                let CodecError::ResourceLimit(sticky) =
                    scope_operand_key(ctx, operand).unwrap_err()
                else {
                    panic!("sticky scope stream refusal");
                };
                assert_eq!(sticky, original);
                Err::<(), _>(CodecError::ResourceLimit(original))
            },
        ) else {
            panic!("scope stream source refusal");
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, "split F3D native stream");
        assert_eq!(original.used, 4);
        assert_eq!(original.additional, 1);
    }
}

#[test]
fn member_operand_stream_key_admits_only_reached_suffix_and_preserves_refusal() {
    for prefix in [String::new(), "流".to_owned(), "a".repeat(4096)] {
        let (_, _, mut operands, _) = super::selection_fixture();
        let stream = format!("f3d:{prefix}");
        let operand = &mut operands[0];
        operand.id = format!("{stream}:r#21");
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 5;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            assert_eq!(
                member_operand_key(ctx, operand).unwrap(),
                Some((Some(stream.as_str()), 20, 0, 21))
            );
            assert_eq!(operand.id, format!("{stream}:r#21"));
            assert!(
                matches!(ctx.charge_work(1, "after F3D member stream key"), Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits && limit.used == 5 && limit.additional == 1)
            );
        });
        let CodecError::ResourceLimit(original) = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            "split F3D native stream",
            4,
            |ctx| {
                let CodecError::ResourceLimit(original) =
                    member_operand_key(ctx, operand).unwrap_err()
                else {
                    panic!("member stream source refusal");
                };
                let CodecError::ResourceLimit(sticky) =
                    member_operand_key(ctx, operand).unwrap_err()
                else {
                    panic!("sticky member stream refusal");
                };
                assert_eq!(sticky, original);
                Err::<(), _>(CodecError::ResourceLimit(original))
            },
        ) else {
            panic!("member stream source refusal");
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, "split F3D native stream");
        assert_eq!(original.used, 4);
        assert_eq!(original.additional, 1);
    }
}

#[test]
fn ineligible_operand_keys_skip_native_stream_query_without_allocation() {
    let (_, _, mut operands, _) = super::selection_fixture();
    let operand = &mut operands[0];
    operand.id = format!("f3d:{}:r#21", "a".repeat(4096));
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(scope_operand_key(ctx, operand).unwrap(), None);
        operand.owner = DesignOperandOwner::ScopeReference {
            scope_reference_ordinal: 0,
        };
        assert_eq!(member_operand_key(ctx, operand).unwrap(), None);
        assert_eq!(ctx.resource_refusal(), None);
    });
}
