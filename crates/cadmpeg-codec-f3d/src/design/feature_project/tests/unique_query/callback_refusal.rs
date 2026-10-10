// SPDX-License-Identifier: Apache-2.0
//! A query stops at an ordinary predicate error.

use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_core::CodecError;

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
