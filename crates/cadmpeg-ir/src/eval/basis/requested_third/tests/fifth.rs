// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn joint_fifth_basis_uses_quintic_bernstein_law_and_real_backing_lifetime() {
    let knots = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    let bytes = u64::try_from(6 * std::mem::size_of::<[f64; 6]>()).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 2 * bytes;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 12;
    // Two six-cell initializations12, five rows5, cells2+3+4+5+6=20.
    policy.limits.max_work_units = 37;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let scratch = decode::Scratch::new(&ctx);
    let actual = rows::<6>(&scratch, &knots, 5, 5, FiniteReal::ZERO, width()).unwrap();
    assert!(actual.fourth_available() && actual.fifth_available());
    // B_i=C(5,i)s^i(1-s)^(5-i). D5=(-1)^(5-i)*120*C(5,i).
    assert_eq!(actual.as_slice().iter().map(|row| row[5]).collect::<Vec<_>>(),
        [-120.0, 600.0, -1200.0, 1200.0, -600.0, 120.0]);
    drop(ctx.reserve_scoped_limit(bytes, "discarded six lane buffer released").unwrap());
    drop(actual);
    drop(ctx.reserve_scoped_limit(2 * bytes, "both six lane buffers released").unwrap());
    drop(scratch);
    ctx.finish_session().unwrap();
}

#[test]
fn joint_fifth_keeps_original_fuse_at_every_real_triangle_prefix() {
    let knots = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    for cap in 0..37 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = decode::Scratch::new(&ctx);
        assert!(rows::<6>(&scratch, &knots, 5, 5, FiniteReal::ZERO, width()).is_none());
        let original = scratch.refused().unwrap();
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!((original.limit, original.used, original.additional), (cap, cap, 1));
        assert_eq!(original.operation, if cap < 12 { "IR requested curve basis initialization" }
            else if [12, 15, 19, 24, 30].contains(&cap) { "IR requested curve basis row" }
            else { "IR requested curve basis cell" });
        assert!(rows::<6>(&scratch, &[], 5, 5, FiniteReal::ZERO, width()).is_none());
        assert_eq!(scratch.refused(), Some(original));
        drop(scratch);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
    }
}

#[test]
fn recovered_fourth_does_not_fabricate_fifth_from_lost_lower_degree_fourth() {
    let h = 2.0_f64.powi(-342);
    let knots = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, h, 1.0, 2.0, 3.0, 4.0,
        5.0, 5.0, 5.0, 5.0, 5.0, 5.0];
    let scratch = decode::Scratch::new(EvaluationAdmission::Standard);
    let width = crate::math::sum::scaled_finite(h).unwrap();
    let sixth = rows::<6>(&scratch, &knots, 5, 5, FiniteReal::ZERO, width).unwrap();
    let fifth = rows::<5>(&scratch, &knots, 5, 5, FiniteReal::ZERO, width).unwrap();
    assert!(sixth.fourth_available());
    assert!(!sixth.fifth_available());
    for (sixth, fifth) in sixth.as_slice().iter().zip(fifth.as_slice()) {
        assert_eq!(&sixth[..5], fifth.as_slice());
    }
}
