// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, WorkBudget};

#[test]
fn preceding_polynomial_row_is_absent_until_real_copy_and_holds_actual_backing() {
    let bytes = 3 * u64::try_from(std::mem::size_of::<f64>()).unwrap();
    let mut policy = DecodePolicy::service(); policy.limits.max_materialized_bytes = bytes;
    policy.limits.max_collection_items = 3; policy.limits.max_work_units = 3;
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let scratch = decode::Scratch::new(&ctx);
    let mut previous = PreviousBasis::new(&scratch, 3).unwrap();
    assert!(previous.as_slice().is_none());
    previous.capture(scratch.admission, &[0.25, 0.5, 0.25]).unwrap();
    assert_eq!(previous.as_slice(), Some(&[0.25, 0.5, 0.25][..]));
    drop(previous);
    // Scratch did not capture this separate preceding-row backing.
    drop(ctx.reserve_scoped_limit(bytes, "reuse destroyed preceding row").unwrap());
    drop(scratch); ctx.finish_session().unwrap();
    for cap in [2, 3] {
        let work = WorkBudget::new(cap);
        EvaluationAdmission::Standard.within_work_slice(&work, |admission| {
            let scratch = decode::Scratch::new(admission);
            let mut previous = PreviousBasis::new(&scratch, 3).unwrap();
            previous.capture(admission, &[0.25, 0.5, 0.25]).unwrap();
            if cap == 2 { assert!(previous.as_slice().is_none()); }
            else { assert_eq!(previous.as_slice(), Some(&[0.25, 0.5, 0.25][..])); }
            Ok::<_, EvaluationFailure<()>>(())
        }).unwrap();
        assert_eq!(work.consumed(), cap);
    }
}

#[test]
fn lost_preceding_cox_terms_do_not_become_a_fourth_zero_theorem() {
    let least = f64::from_bits(1);
    let knots = [-f64::MAX, -f64::MAX, -f64::MAX, -f64::MAX,
        -f64::MAX, -f64::MAX, -f64::MAX, 0.0, least,
        f64::MAX, f64::MAX, f64::MAX, f64::MAX, f64::MAX, f64::MAX, f64::MAX];
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = decode::Scratch::new(admission);
        let actual = rows(&scratch, &knots, 7, 7, 0.0, PositiveReal::new(least), false).unwrap();
        // The q=2 Cox right term least/MAX is nonzero below binary64 range.
        // No available prior-row value proves an exact Fourth zero after that loss.
        assert_eq!(actual.fourth, Err(EvaluationFailure::NoValue));
    }
    ctx.finish_session().unwrap();
}

#[test]
fn second_preceding_row_retains_its_real_copy_when_a_later_cox_row_loses_terms() {
    let least = f64::from_bits(1);
    let knots = [-f64::MAX, -f64::MAX, -f64::MAX, 0.0, least, f64::MAX, f64::MAX, f64::MAX];
    let bytes = 3 * u64::try_from(std::mem::size_of::<f64>()).unwrap();
    let mut policy = DecodePolicy::service(); policy.limits.max_materialized_bytes = bytes;
    policy.limits.max_collection_items = 3; policy.limits.max_work_units = 10; policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = decode::Scratch::new(admission);
        let mut previous = PreviousBasis::new(&scratch, 3).unwrap();
        let mut second_previous = PreviousBasis::new(&scratch, 2).unwrap();
        let mut values = [0.0; 4];
        super::super::fill_bspline_basis_rows(admission, &knots, 3, 3, 0.0, &mut values,
            Some(&mut previous), Some(&mut second_previous), None).unwrap().unwrap();
        // The degree1 row is [1,0]. The next Cox row loses least/MAX;
        // this cannot change the already captured degree1 source row.
        assert_eq!(second_previous.as_slice(), Some(&[1.0, 0.0][..]));
        assert!(previous.as_slice().is_none());
        drop(previous); drop(second_previous);
        if matches!(admission, EvaluationAdmission::Decode(_)) {
            drop(ctx.reserve_scoped_limit(bytes, "destroyed preceding source backing").unwrap());
        }
    }
    ctx.finish_session().unwrap();
}

#[test]
fn captured_polynomial_seeds_share_actual_point_triangle_and_backing_lifetime() {
    // Point basis6 plus the degree2 Third seed3; Fourth/Fifth seeds are inline.
    // Writes: basis initialization6, Cox initial1 + cells15 + saved5,
    // and the actual heap seed copy3, totaling30.
    let bytes = 9 * u64::try_from(std::mem::size_of::<f64>()).unwrap();
    let seed_bytes = 3 * u64::try_from(std::mem::size_of::<f64>()).unwrap();
    let mut policy = DecodePolicy::service(); policy.limits.max_collection_items = 9;
    policy.limits.max_materialized_bytes = bytes; policy.limits.max_work_units = 30;
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let scratch = decode::Scratch::new(&ctx);
    let knots = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    let (point, captured) = point_basis(&scratch, &knots, 5, 5, 0.0, 5).unwrap();
    assert_eq!(&*point, &[1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(captured.values[0].as_ref().unwrap().as_slice(), Some(&[1.0, 0.0, 0.0][..]));
    assert_eq!(captured.values[1].as_ref().unwrap().as_slice(), Some(&[1.0, 0.0][..]));
    assert_eq!(captured.values[2].as_ref().unwrap().as_slice(), Some(&[1.0][..]));
    drop(captured);
    drop(ctx.reserve_scoped_limit(seed_bytes, "destroyed actual captured seed backing").unwrap());
    drop(point); drop(scratch);
    drop(ctx.reserve_scoped_limit(bytes, "destroyed point and seed backings").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn captured_point_triangle_lost_terms_never_prove_zero_higher_orders() {
    let least = f64::from_bits(1);
    let knots = [-f64::MAX, -f64::MAX, -f64::MAX, -f64::MAX,
        -f64::MAX, -f64::MAX, -f64::MAX, 0.0, least,
        f64::MAX, f64::MAX, f64::MAX, f64::MAX, f64::MAX, f64::MAX, f64::MAX];
    let policy = DecodePolicy::service(); let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = decode::Scratch::new(admission);
        let (_point, captured) = point_basis(&scratch, &knots, 7, 7, 0.0, 5).unwrap();
        for value in &captured.values { assert!(value.as_ref().unwrap().as_slice().is_none()); }
        let rows = captured.rows(&scratch, &knots, 7, PositiveReal::new(least).unwrap());
        assert_eq!(rows.third, Err(EvaluationFailure::NoValue));
        assert_eq!(rows.fourth, Err(EvaluationFailure::NoValue));
        assert_eq!(rows.fifth, Err(EvaluationFailure::NoValue));
    }
    ctx.finish_session().unwrap();
}
