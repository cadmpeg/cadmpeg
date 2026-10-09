// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn transform_preflight_local_ceiling_fuses_the_original_caller_session() {
    let mut directory = [
        crate::test_support::directory_target(1, 110),
        crate::test_support::directory_target(3, 124),
        crate::test_support::directory_target(5, 124),
        crate::test_support::directory_target(7, 124),
    ];
    directory[0].transform = 3;
    directory[1].transform = 5;
    directory[2].transform = 7;
    for cap in [2, 3] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = super::super::enforce_transform_depth(&directory, &ctx);
        if cap == 2 {
            let first = match result {
                Err(CodecError::ResourceLimit(first)) => first,
                _ => panic!("expected the three-transform chain to refuse its third step"),
            };
            assert_eq!(first.dimension, ResourceDimension::Codec("iges_transform_depth"));
            assert_eq!(first.operation, "iges_transform_depth");
            assert_eq!((first.limit, first.used, first.additional), (2, 2, 1));
            assert_eq!(ctx.resource_refusal(), Some(first));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            result.unwrap();
            ctx.finish_session().unwrap();
        }
    }
}
