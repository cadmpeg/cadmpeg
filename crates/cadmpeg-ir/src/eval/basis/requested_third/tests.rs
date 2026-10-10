// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::admission::EvaluationAdmission;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

mod fourth;
mod fifth;

fn width() -> ScaledValue {
    let mut sum = ExactSignedSum::default();
    sum.add_factors([1.0]);
    sum.finish().unwrap()
}

#[test]
fn joint_cubic_basis_has_independent_bernstein_orders() {
    let scratch = decode::Scratch::new(EvaluationAdmission::Standard);
    let knots = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    // Direct differentiation of (1-s)^3, 3s(1-s)^2,
    // 3s^2(1-s), s^3 at s=.5, not a second evaluator oracle.
    let actual = rows::<4>(&scratch, &knots, 3, 3, FiniteReal::HALF, width()).unwrap();
    assert_eq!(actual.as_slice(), [
        [0.125, -0.75, 3.0, -6.0], [0.375, -0.75, -3.0, 18.0],
        [0.375, 0.75, -3.0, -18.0], [0.125, 0.75, 3.0, 6.0],
    ]);
}

#[test]
fn joint_heap_basis_retains_only_the_returned_backing_and_reuses_each_degree_buffer() {
    let knots = [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    let one = u64::try_from(5 * std::mem::size_of::<[f64; 4]>()).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 2 * one;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 20;
    // Per recurrence10 initializations +4 degree rows +14 cells =28.
    policy.limits.max_work_units = 56;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let scratch = decode::Scratch::new(&ctx);
    for _ in 0..2 {
        let actual = rows::<4>(&scratch, &knots, 4, 4, FiniteReal::ZERO, width()).unwrap();
        assert_eq!(actual.as_slice(), [
            [1.0, -4.0, 12.0, -24.0], [0.0, 4.0, -24.0, 72.0],
            [0.0, 0.0, 12.0, -72.0], [0.0, 0.0, 0.0, 24.0], [0.0; 4],
        ]);
        // The discarded degree buffer has died; the borrowed returned one
        // still owns its genuine reservation throughout this scope.
        drop(ctx.reserve_scoped_limit(one, "discarded joint buffer released").unwrap());
        drop(actual);
        drop(ctx.reserve_scoped_limit(2 * one, "both joint buffers released").unwrap());
    }
    // Collection visits are monotone, despite temporary byte release.
    let original = ctx.charge_collection_items_limit(1, "joint items stay consumed").unwrap_err();
    assert_eq!(original.dimension, ResourceDimension::CollectionItems);
    assert_eq!((original.limit, original.used, original.additional), (20, 20, 1));
    drop(scratch);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}

#[test]
fn joint_heap_basis_unwind_destroys_its_backing_with_the_reservation() {
    let knots = [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    let bytes = u64::try_from(10 * std::mem::size_of::<[f64; 4]>()).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = bytes;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 10;
    policy.limits.max_work_units = 28;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let scratch = decode::Scratch::new(&ctx);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let actual = rows::<4>(&scratch, &knots, 4, 4, FiniteReal::ZERO, width()).unwrap();
        assert_eq!(actual.as_slice().len(), 5);
        panic!("unwind with actual requested basis backing live");
    }));
    assert!(result.is_err());
    drop(ctx.reserve_scoped_limit(bytes, "unwound joint backing released").unwrap());
    drop(scratch);
    ctx.finish_session().unwrap();
}

#[test]
fn joint_heap_basis_refuses_each_real_initialization_row_and_cell_before_execution() {
    let knots = [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    // Source order: initialize5 twice; then row1/cells2, row1/cells3,
    // row1/cells4, row1/cells5. Fresh vector reservations move no bytes.
    for cap in 0..28 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = decode::Scratch::new(&ctx);
        assert!(rows::<4>(&scratch, &knots, 4, 4, FiniteReal::ZERO, width()).is_none());
        let original = scratch.refused().unwrap();
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!((original.limit, original.used, original.additional), (cap, cap, 1));
        assert_eq!(original.operation, if cap < 10 {
            "IR requested curve basis initialization"
        } else if [10, 13, 17, 22].contains(&cap) {
            "IR requested curve basis row"
        } else { "IR requested curve basis cell" });
        // The original fuse precedes even inspecting this unusable source.
        assert!(rows::<4>(&scratch, &[], 4, 4, FiniteReal::ZERO, width()).is_none());
        assert_eq!(scratch.refused(), Some(original));
        drop(scratch);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
    }
}
