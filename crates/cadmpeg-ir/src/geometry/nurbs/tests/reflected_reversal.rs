// SPDX-License-Identifier: Apache-2.0
use crate::scalar::FiniteReal;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

#[test]
fn reflected_reversal_preserves_the_carrier_on_every_work_refusal() {
    let original = super::curve();
    let knot_count = u64::try_from(original.knots().len()).expect("knots");
    // The validation search includes its end probe. Mutation visits both complete lanes once.
    let total = knot_count * 2 + 1 + u64::try_from(original.pole_count()).expect("poles");
    let start = FiniteReal::new(2.).expect("finite fixture domain start");
    let end = FiniteReal::new(5.).expect("finite fixture domain end");
    for operation in [
        "IR NURBS reflected knot validation",
        "IR NURBS reflected knot reversal",
        "IR NURBS reflected pole reversal",
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                crate::geometry::tests::budget::with_limit(
                    ResourceDimension::WorkUnits,
                    cap,
                    |ctx| {
                        let mut reversed = original.clone();
                        let result = reversed.reverse_parameterization_in_range(ctx, start, end);
                        assert_eq!(reversed, original);
                        result
                    },
                )
            },
        );
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = total;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut reversed = original.clone();
    assert_eq!(
        reversed
            .reverse_parameterization_in_range(&ctx, start, end)
            .expect("exact visits"),
        Some(())
    );
    assert_eq!(reversed.knots(), original.knots());
    for index in 0..original.pole_count() {
        let reverse = original.pole_count() - index - 1;
        assert_eq!(
            reversed.pole_rows().point_at(index),
            original.pole_rows().point_at(reverse)
        );
        assert_eq!(
            reversed.pole_rows().weight_at(index),
            original.pole_rows().weight_at(reverse)
        );
    }
    ctx.finish_session()
        .expect("no allocation and no extra pass");
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
    assert_eq!(
        curve
            .reverse_parameterization_in_range(&ctx, bound, bound)
            .expect("one invalid reflected knot"),
        None
    );
    assert_eq!(curve, original);
    ctx.finish_session()
        .expect("semantic absence has no resource refusal");
}
