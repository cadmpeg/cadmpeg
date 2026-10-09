// SPDX-License-Identifier: Apache-2.0

use super::{DecodeArena, DecodeContext, DecodePolicy, Point3, ResourceDimension};
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;

const OP_RAW: &str = "test offset raw control storage";
const OP_ADMITTED: &str = "test offset admitted control storage";

fn policy(count: usize, work: usize) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64_from_index(count * std::mem::size_of::<Point3>());
    policy.limits.max_retained_bytes = u64_from_index(count * std::mem::size_of::<FinitePoint3>());
    policy.limits.max_collection_items = u64_from_index(2 * count);
    policy.limits.max_work_units = u64_from_index(work);
    policy
}

#[test]
fn consumed_offset_raw_storage_is_free_while_admitted_points_remain_live() {
    for count in [1, 64] {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(count, count)).unwrap();
        let storage;
        let (mut controls, raw_storage) = ctx.temporary_vec(count, OP_RAW).unwrap();
        storage = raw_storage;
        controls.resize(count, Point3::new(1.0, 2.0, 3.0));
        let admitted = super::super::admit_offset_controls(&ctx, storage, controls, OP_ADMITTED)
            .unwrap().unwrap();
        assert_eq!(admitted.len(), count);
        assert!(admitted.iter().all(|point| point.get() == Point3::new(1.0, 2.0, 3.0)));
        let reuse = ctx.reserve_scoped(
            u64_from_index(count * std::mem::size_of::<Point3>()),
            "test consumed offset raw storage reuse",
        ).unwrap();
        assert_eq!(admitted.last().unwrap().get(), Point3::new(1.0, 2.0, 3.0));
        drop((reuse, admitted));
        ctx.finish_session().unwrap();
    }
}

#[test]
fn offset_raw_storage_refuses_before_allocating_the_first_or_last_slot() {
    for count in [1, 64] {
        let mut policy = policy(count, count);
        let bytes = u64_from_index(count * std::mem::size_of::<Point3>());
        policy.limits.max_materialized_bytes = bytes - 1;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let CodecError::ResourceLimit(limit) = ctx.temporary_vec::<Point3>(count, OP_RAW).unwrap_err()
        else { panic!("expected raw control storage refusal"); };
        assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(limit.operation, OP_RAW);
        assert_eq!(limit.used, 0);
        assert_eq!(limit.additional, bytes);
        assert_eq!(limit.limit, bytes - 1);
        let CodecError::ResourceLimit(finished) = ctx.finish_session().unwrap_err()
        else { panic!("expected original storage refusal at finish"); };
        assert_eq!(finished, limit);
    }
}

#[test]
fn offset_control_refusal_preserves_the_original_first_or_last_visit() {
    for count in [1, 64] {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(count, count - 1)).unwrap();
        let replay_storage = ctx.reserve_scoped(0, "test fused empty offset controls").unwrap();
        let storage;
        let (mut controls, raw_storage) = ctx.temporary_vec(count, OP_RAW).unwrap();
        storage = raw_storage;
        controls.resize(count, Point3::new(1.0, 2.0, 3.0));
        let CodecError::ResourceLimit(limit) = super::super::admit_offset_controls(
            &ctx, storage, controls, OP_ADMITTED,
        ).unwrap_err() else { panic!("expected control visit refusal"); };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "iges offset control admission");
        assert_eq!(limit.used, u64_from_index(count - 1));
        assert_eq!(limit.additional, 1);
        assert_eq!(limit.limit, u64_from_index(count - 1));
        let CodecError::ResourceLimit(replayed) = super::super::admit_offset_controls(
            &ctx, replay_storage, Vec::new(), OP_ADMITTED,
        ).unwrap_err() else { panic!("expected original control refusal on replay"); };
        assert_eq!(replayed, limit);
        let CodecError::ResourceLimit(finished) = ctx.finish_session().unwrap_err()
        else { panic!("expected original control refusal at finish"); };
        assert_eq!(finished, limit);
    }
}

#[test]
fn rejected_offset_controls_release_the_unvisited_raw_tail() {
    const COUNT: usize = 4097;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(COUNT, 1)).unwrap();
    let storage;
    let (mut controls, raw_storage) = ctx.temporary_vec(COUNT, OP_RAW).unwrap();
    storage = raw_storage;
    controls.resize(COUNT, Point3::new(1.0, 2.0, 3.0));
    controls[0] = Point3::new(f64::INFINITY, 0.0, 0.0);
    assert!(super::super::admit_offset_controls(&ctx, storage, controls, OP_ADMITTED)
        .unwrap().is_none());
    let reuse = ctx.reserve_scoped(
        u64_from_index(COUNT * std::mem::size_of::<Point3>()),
        "test rejected offset raw storage reuse",
    ).unwrap();
    drop(reuse);
    ctx.finish_session().unwrap();
}

#[test]
fn empty_offset_controls_need_no_work_or_storage() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(0, 0)).unwrap();
    let storage = ctx.reserve_scoped(0, OP_RAW).unwrap();
    let admitted = super::super::admit_offset_controls(&ctx, storage, Vec::new(), OP_ADMITTED)
        .unwrap().unwrap();
    assert!(admitted.is_empty());
    ctx.finish_session().unwrap();
}
