// SPDX-License-Identifier: Apache-2.0

use crate::curve::{curve_expression_solve_unknowns, SolveUnknown};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceLimit};
use cadmpeg_core::CodecError;

const EMPTY_ROOT_MATERIALIZED_BYTES: u64 = 16 * 1024 * 1024;
const PARENT_BYTES: u64 = 37;

fn unknown_backing_bytes() -> u64 {
    // Two one-byte names and the four slots of initial amortized Vec growth.
    2 + 4 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<SolveUnknown>())
}

fn resource(error: CodecError) -> ResourceLimit {
    match error {
        CodecError::ResourceLimit(limit) => limit,
        other => panic!("expected resource refusal, got {other}"),
    }
}

#[test]
fn solve_unknown_value_custody_preserves_each_retained_boundary() {
    let backing = unknown_backing_bytes();
    for cap in 0..=backing {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        match curve_expression_solve_unknowns(&ctx, "x,y") {
            Ok(Some(unknowns)) => {
                assert_eq!(cap, backing);
                assert_eq!(unknowns.len(), 2);
                assert_eq!(unknowns[0].name, "x");
                assert_eq!(unknowns[1].name, "y");
                assert!(unknowns.iter().all(|unknown| unknown.solution.is_none()));
                assert_eq!(unknowns.capacity(), 4);
                let limit = resource(
                    ctx.charge_retained(1, "probe unknown retained backing")
                        .expect_err("exact retained boundary"),
                );
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                assert_eq!(limit.used, backing);
                assert_eq!(limit.additional, 1);
                assert_eq!(limit.operation, "probe unknown retained backing");
                drop(unknowns);
                assert_eq!(resource(ctx.finish_session().expect_err("sticky session")), limit);
            }
            Ok(None) => panic!("valid distinct unknowns must be retained"),
            Err(error) => {
                assert!(cap < backing);
                let limit = resource(error);
                assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                assert_eq!(limit.limit, cap);
                if cap < 2 {
                    assert_eq!(limit.operation, "creo solve unknown names");
                    assert_eq!(limit.used, 0);
                    assert_eq!(limit.additional, 2);
                } else {
                    assert_eq!(limit.operation, "creo solve unknowns");
                    assert_eq!(limit.used, 2);
                    assert_eq!(limit.additional, backing - 2);
                }
                assert_eq!(
                    resource(ctx.charge_work(0, "later unknown probe").expect_err("sticky")),
                    limit,
                );
                assert_eq!(resource(ctx.finish_session().expect_err("sticky session")), limit);
            }
        }
    }
}

#[test]
fn solve_unknown_value_custody_keeps_actual_parent_overlap() {
    let backing = unknown_backing_bytes();
    for extra in [0, 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = EMPTY_ROOT_MATERIALIZED_BYTES;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut parent = ctx.reserve_scoped(0, "unknown parent storage").expect("parent");
        let parent_value = parent
            .with_storage(|| ctx.copy_retained(&[0_u8; 37], "unknown parent bytes"))
            .expect("actual parent bytes");
        let unknowns = parent
            .with_storage(|| curve_expression_solve_unknowns(&ctx, "x,y"))
            .expect("scoped promotion")
            .expect("distinct unknowns");
        assert_eq!(parent_value.len(), 37);
        assert_eq!(unknowns.len(), 2);
        assert_eq!(unknowns[0].name, "x");
        assert_eq!(unknowns[1].name, "y");
        let probe_bytes = EMPTY_ROOT_MATERIALIZED_BYTES - PARENT_BYTES - backing + extra;
        let refused = match ctx.reserve_scoped(probe_bytes, "probe unknown live backing") {
            Ok(probe) => {
                assert_eq!(extra, 0);
                drop(probe);
                drop(unknowns);
                drop(parent_value);
                drop(parent);
                let probe = ctx
                    .reserve_scoped(EMPTY_ROOT_MATERIALIZED_BYTES, "probe unknown cleanup")
                    .expect("all actual scratch refunded");
                drop(probe);
                ctx.charge_retained(0, "probe zero retained unknown backing")
                    .expect("ambient promotion retained no session bytes");
                None
            }
            Err(error) => {
                assert_eq!(extra, 1);
                let limit = resource(error);
                assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
                assert_eq!(limit.operation, "probe unknown live backing");
                assert_eq!(limit.used, PARENT_BYTES + backing);
                assert_eq!(limit.additional, probe_bytes);
                drop(unknowns);
                drop(parent_value);
                drop(parent);
                assert_eq!(
                    resource(ctx.charge_work(0, "later parent probe").expect_err("sticky")),
                    limit,
                );
                Some(limit)
            }
        };
        match refused {
            Some(limit) => {
                assert_eq!(resource(ctx.finish_session().expect_err("sticky session")), limit);
            }
            None => ctx.finish_session().expect("active session"),
        }
    }
}

#[test]
fn solve_unknown_value_custody_refunds_rejected_duplicate_backing() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = EMPTY_ROOT_MATERIALIZED_BYTES;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut parent = ctx.reserve_scoped(0, "duplicate unknown parent").expect("parent");
    let parent_value = parent
        .with_storage(|| ctx.copy_retained(&[0_u8; 37], "duplicate unknown parent bytes"))
        .expect("actual parent");
    assert!(parent
        .with_storage(|| curve_expression_solve_unknowns(&ctx, "x,X"))
        .expect("case-insensitive duplicate recovery")
        .is_none());
    let probe = ctx
        .reserve_scoped(
            EMPTY_ROOT_MATERIALIZED_BYTES - PARENT_BYTES,
            "probe rejected unknown cleanup",
        )
        .expect("rejected key, index and rows refunded");
    drop(probe);
    drop(parent_value);
    drop(parent);
    ctx.charge_retained(0, "probe duplicate retained cleanup")
        .expect("no rejected session storage");
    ctx.finish_session().expect("active session");
}
