// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn identity_text_equality_admits_length_and_only_actual_byte_comparisons() {
    for (first, second, work, expected) in [
        ("alpha", "longer", 1, false),
        ("alpha", "blope", 2, false),
        ("alpha", "alpha", 6, true),
        ("é", "ê", 3, false),
        ("", "", 1, true),
    ] {
        for allowance in 0..=work {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = allowance;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            policy.limits.max_recursion_depth = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = super::equal(&ctx, first, second, "actual text equality");
            if allowance < work {
                let original = result.unwrap_err();
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!(original.operation, "actual text equality");
                assert_eq!(original.additional, 1);
                assert_eq!(super::equal(&ctx, "", "different length", "fused text equality").unwrap_err(), original);
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
            } else {
                assert_eq!(result.unwrap(), expected);
                ctx.finish_session().unwrap();
            }
        }
    }
}
