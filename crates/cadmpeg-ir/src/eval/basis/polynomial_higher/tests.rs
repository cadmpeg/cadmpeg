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
            Some(&mut previous), Some(&mut second_previous)).unwrap().unwrap();
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
