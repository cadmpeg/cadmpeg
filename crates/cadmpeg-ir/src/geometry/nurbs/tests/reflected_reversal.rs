// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::scalar::FiniteReal;

#[test]
fn reflected_reversal_preserves_the_carrier_on_every_work_refusal() {
    let original = super::curve();
    let knot_count = u64::try_from(original.knots().len()).expect("knots");
    let pole_swaps = u64::try_from(original.pole_count() / 2).expect("pole swaps");
    let knot_swaps = knot_count / 2;
    let total = knot_count * 2 + pole_swaps + knot_swaps;
    let start = FiniteReal::new(2.).expect("finite fixture domain start");
    let end = FiniteReal::new(5.).expect("finite fixture domain end");
    for cap in 0..total {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut reversed = original.clone();
        let Err(CodecError::ResourceLimit(limit)) = reversed.reverse_parameterization_in_range(&ctx, start, end) else {
            panic!("validation and all mutation work require admission");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        let operation = if cap < knot_count {
            "IR NURBS reflected knot validation"
        } else if cap < knot_count + pole_swaps {
            "IR NURBS reflected pole reversal"
        } else if cap < knot_count + pole_swaps + knot_swaps {
            "IR NURBS reflected knot reversal"
        } else {
            "IR NURBS reflected knot edit"
        };
        assert_eq!(limit.operation, operation);
        assert_eq!(reversed, original);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = total;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut reversed = original.clone();
    assert_eq!(reversed.reverse_parameterization_in_range(&ctx, start, end).expect("exact visits"), Some(()));
    assert_eq!(reversed.knots(), original.knots());
    for index in 0..original.pole_count() {
        let reverse = original.pole_count() - index - 1;
        assert_eq!(reversed.pole_rows().point_at(index), original.pole_rows().point_at(reverse));
        assert_eq!(reversed.pole_rows().weight_at(index), original.pole_rows().weight_at(reverse));
    }
    ctx.finish_session().expect("no allocation and no extra pass");
}

#[test]
fn reflected_reversal_admits_invalid_reflection_before_geometric_absence() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut curve = super::curve();
    let original = curve.clone();
    let bound = FiniteReal::new(f64::MAX).expect("finite");
    assert_eq!(curve.reverse_parameterization_in_range(&ctx, bound, bound).expect("one invalid reflected knot"), None);
    assert_eq!(curve, original);
    ctx.finish_session().expect("semantic absence has no resource refusal");
}
