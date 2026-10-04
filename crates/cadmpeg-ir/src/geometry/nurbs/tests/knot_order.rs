// SPDX-License-Identifier: Apache-2.0

use crate::geometry::nurbs::knots_strictly_increasing;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn strict_knot_order_admits_actual_pairs_and_observes_empty_refusals() {
    for (knots, pairs, ordered) in [
        (vec![], 0, true),
        (vec![f64::NAN], 0, true),
        (vec![0.0, 0.0, 1.0, 2.0], 1, false),
        (vec![1.0, 0.0, 1.0, 2.0], 1, false),
        (vec![0.0, f64::NAN, 1.0, 2.0], 1, false),
        (vec![0.0, 1.0, 2.0, 3.0], 3, true),
    ] {
        for allowance in 0..=pairs {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = allowance;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = knots_strictly_increasing(&knots, |count| {
                ctx.charge_work(count, "test strict knot order")
            });
            if allowance < pairs {
                let Err(CodecError::ResourceLimit(original)) = result else {
                    panic!("each adjacent comparison must be admitted");
                };
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!(original.used, allowance);
                assert_eq!(original.additional, 1);
                assert!(
                    matches!(knots_strictly_increasing(&[], |count| ctx.charge_work(count, "test empty strict knot order")), Err(CodecError::ResourceLimit(sticky)) if sticky == original)
                );
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)
                );
            } else {
                assert_eq!(result.unwrap(), ordered);
                if pairs == 0 {
                    ctx.finish_session().unwrap();
                } else {
                    let Err(CodecError::ResourceLimit(original)) =
                        ctx.charge_work(1, "strict pair budget exhausted")
                    else {
                        panic!("only actual pairs must spend work");
                    };
                    assert_eq!(original.used, pairs);
                    assert!(
                        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original)
                    );
                }
            }
        }
    }
}
