// SPDX-License-Identifier: Apache-2.0
//! A predicate cannot replace the unique query's original resource refusal.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn unique_query_second_match_cannot_hide_predicate_resource_refusal() {
    let values = [5, 5, 99, 99, 99];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "original in F3D unique predicate",
            Some(1),
        );
        let mut visits = 0;
        let result = super::super::super::unique_feature_match_for_decode(
            ctx,
            &values,
            |value| {
                visits += 1;
                if visits == 2 {
                    assert!(matches!(
                        ctx.charge_work(1, "original in F3D unique predicate"),
                        Err(CodecError::ResourceLimit(_))
                    ));
                }
                Ok(*value == 5)
            },
            "visit actual unique callback source",
        );
        let Err(CodecError::ResourceLimit(original)) = result else {
            panic!("the duplicate result must preserve predicate refusal");
        };
        assert_eq!(visits, 2);
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, "original in F3D unique predicate");
        assert_eq!((original.used, original.additional), (2, 1));
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(matches!(
            super::super::super::unique_feature_match_for_decode(
                ctx,
                &[] as &[i32],
                |_| { visits += 1; Ok(true) },
                "later unique callback query",
            ),
            Err(CodecError::ResourceLimit(later)) if later == original
        ));
        assert_eq!(visits, 2);
        assert_eq!(values, [5, 5, 99, 99, 99]);
    });
}

#[test]
fn unique_query_predicate_ordinary_error_cannot_replace_original_refusal() {
    let values = [5, 99, 99];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "original in F3D unique predicate",
            Some(1),
        );
        let mut visits = 0;
        let result = super::super::super::unique_feature_match_for_decode(
            ctx,
            &values,
            |_| {
                visits += 1;
                assert!(matches!(
                    ctx.charge_work(1, "original in F3D unique predicate"),
                    Err(CodecError::ResourceLimit(_))
                ));
                Err(CodecError::malformed("ordinary predicate rejection"))
            },
            "visit actual unique callback source",
        );
        let Err(CodecError::ResourceLimit(original)) = result else {
            panic!("the ordinary error must preserve predicate refusal");
        };
        assert_eq!(visits, 1);
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, "original in F3D unique predicate");
        assert_eq!((original.used, original.additional), (1, 1));
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(matches!(
            super::super::super::unique_feature_match_for_decode(
                ctx,
                &values,
                |_| { visits += 1; Ok(false) },
                "later unique callback query",
            ),
            Err(CodecError::ResourceLimit(later)) if later == original
        ));
        assert_eq!(visits, 1);
        assert_eq!(values, [5, 99, 99]);
    });
}

#[test]
fn unique_query_ordinary_predicate_error_keeps_message_and_unread_suffix() {
    let values = [5, 99, 99];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut visits = 0;
        let result = super::super::super::unique_feature_match_for_decode(
            ctx,
            &values,
            |_| {
                visits += 1;
                Err(CodecError::malformed("ordinary predicate rejection"))
            },
            "visit actual unique callback source",
        );
        assert!(matches!(result, Err(CodecError::Malformed(message))
            if message == "ordinary predicate rejection"));
        assert_eq!(visits, 1);
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(after)) =
            ctx.charge_work(1, "check actual unique callback work")
        else {
            panic!("one admitted source visit must consume the one-unit limit");
        };
        assert_eq!(after.operation, "check actual unique callback work");
        assert_eq!((after.used, after.additional), (1, 1));
        assert_eq!(values, [5, 99, 99]);
    });
}
