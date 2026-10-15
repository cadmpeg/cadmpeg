// SPDX-License-Identifier: Apache-2.0
use crate::scalar::FiniteReal;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn reflected_reversal_admits_invalid_reflection_before_geometric_absence() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
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

#[test]
fn reflected_reversal_reverses_poles_and_reflects_knots() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let original = super::curve();
    let mut reversed = original.clone();
    let start = FiniteReal::new(2.0).expect("finite");
    let end = FiniteReal::new(5.0).expect("finite");
    assert_eq!(
        reversed
            .reverse_parameterization_in_range(&ctx, start, end)
            .expect("reversal admission"),
        Some(())
    );
    assert_eq!(reversed.knots(), original.knots());
    let count = original.pole_count();
    for index in 0..count {
        let mirror = count - index - 1;
        assert_eq!(
            reversed.pole_rows().point_at(index),
            original.pole_rows().point_at(mirror)
        );
        assert_eq!(
            reversed.pole_rows().weight_at(index),
            original.pole_rows().weight_at(mirror)
        );
    }
}
