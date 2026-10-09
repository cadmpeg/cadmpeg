// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn joint_fourth_basis_uses_true_quartic_bernstein_derivatives_and_owned_backing() {
    let knots = [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    let bytes = u64::try_from(5 * std::mem::size_of::<[f64; 5]>()).unwrap();
    let mut policy = DecodePolicy::service(); policy.limits.max_materialized_bytes = 2 * bytes;
    policy.limits.max_retained_bytes = 0; policy.limits.max_collection_items = 10; policy.limits.max_work_units = 28;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let scratch = decode::Scratch::new(&ctx);
    let actual = rows::<5>(&scratch, &knots, 4, 4, FiniteReal::ZERO, width()).unwrap();
    assert!(actual.fourth_available());
    assert_eq!(actual.as_slice(), [
        [1.0, -4.0, 12.0, -24.0, 24.0], [0.0, 4.0, -24.0, 72.0, -96.0],
        [0.0, 0.0, 12.0, -72.0, 144.0], [0.0, 0.0, 0.0, 24.0, -96.0], [0.0, 0.0, 0.0, 0.0, 24.0],
    ]);
    drop(ctx.reserve_scoped_limit(bytes, "discarded five lane buffer released").unwrap());
    drop(actual); drop(ctx.reserve_scoped_limit(2 * bytes, "both five lane buffers released").unwrap());
    drop(scratch); ctx.finish_session().unwrap();
}

#[test]
fn joint_fourth_refuses_the_same_real_initialization_row_and_cell_prefixes() {
    let knots = [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    for cap in 0..28 {
        let mut policy = DecodePolicy::service(); policy.limits.max_work_units = cap;
        let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = decode::Scratch::new(&ctx);
        assert!(rows::<5>(&scratch, &knots, 4, 4, FiniteReal::ZERO, width()).is_none());
        let original = scratch.refused().unwrap();
        assert_eq!((original.limit, original.used, original.additional), (cap, cap, 1));
        assert_eq!(original.operation, if cap < 10 { "IR requested curve basis initialization" }
            else if [10, 13, 17, 22].contains(&cap) { "IR requested curve basis row" }
            else { "IR requested curve basis cell" });
        assert!(rows::<5>(&scratch, &[], 4, 4, FiniteReal::ZERO, width()).is_none());
        assert_eq!(scratch.refused(), Some(original));
        drop(scratch); assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
    }
}

#[test]
fn actual_short_selected_span_loses_only_fourth_basis_without_erasing_third() {
    let h = 2.0_f64.powi(-400);
    let knots = [0.0, 0.0, 0.0, 0.0, 0.0, h, 1.0, 2.0, 3.0, 4.0, 4.0, 4.0, 4.0, 4.0];
    let scratch = decode::Scratch::new(EvaluationAdmission::Standard);
    let width = crate::math::sum::scaled_finite(h).unwrap();
    let third = rows::<4>(&scratch, &knots, 4, 4, FiniteReal::ZERO, width).unwrap();
    let fourth = rows::<5>(&scratch, &knots, 4, 4, FiniteReal::ZERO, width).unwrap();
    // The last basis is t^4/(h*1*2*3). Its normalized fourth is4h^3,
    // which is nonzero mathematically and below the binary64 grid.
    assert!(!fourth.fourth_available());
    for (third, fourth) in third.as_slice().iter().zip(fourth.as_slice()) {
        assert_eq!(third.as_slice(), &fourth[..4]);
    }
}

#[test]
fn final_fourth_recovers_from_lower_degree_fourth_loss_using_the_completed_third_row() {
    let h = 2.0_f64.powi(-342);
    let knots = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, h, 1.0, 2.0, 3.0, 4.0,
        5.0, 5.0, 5.0, 5.0, 5.0, 5.0];
    let scratch = decode::Scratch::new(EvaluationAdmission::Standard);
    let width = crate::math::sum::scaled_finite(h).unwrap();
    // At degree4 the last basis is t^4/(6h): local D4=4h^3,
    // below MIN_POSITIVE. At degree5 B4 starts (5/6)t^4/h:
    // local D4=20h^3, normal, and B5 starts with t^5 so D4=0.
    let actual = rows::<5>(&scratch, &knots, 5, 5, FiniteReal::ZERO, width).unwrap();
    assert!(actual.fourth_available());
    assert_eq!(actual.as_slice()[4][4], 20.0 * f64::MIN_POSITIVE / 16.0);
    assert_eq!(actual.as_slice()[5][4], 0.0);
    let third = rows::<4>(&scratch, &knots, 5, 5, FiniteReal::ZERO, width).unwrap();
    for (third, fourth) in third.as_slice().iter().zip(actual.as_slice()) {
        assert_eq!(third.as_slice(), &fourth[..4]);
    }
}
