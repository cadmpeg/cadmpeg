// SPDX-License-Identifier: Apache-2.0
//! Actual scoped curve and table backing, independent of scanner grammar.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{nurbs::NurbsCurve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;

use super::{StreamCarrierIndex, StreamCurve};
use crate::brep::CurveCarrier;

const SCRATCH_BYTES: u64 = 1024 * 1024;
const CURVE_BYTES: usize = 6 * std::mem::size_of::<f64>() + 3 * std::mem::size_of::<cadmpeg_ir::features::FinitePoint3>();

fn policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy
}

fn curve<'ctx>(ctx: &'ctx DecodeContext<'_>, attr: u16, offset: usize) -> StreamCurve<'ctx> {
    // Raw positions are fixture input. The result owns the six unchanged raw
    // knots and the three newly converted finite poles under this one guard.
    let points = vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)];
    let mut geometry_storage = ctx.reserve_scoped(0, "test stream curve geometry").unwrap();
    let geometry = geometry_storage.with_storage(|| {
        let mut knots = ctx.collection_vec(6, "test stream curve knots")?;
        knots.extend_from_slice(&[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        NurbsCurve::from_lanes(ctx, 2, knots, points, None, false)
    }).unwrap().unwrap();
    StreamCurve::exact(CurveCarrier {
        attr, offset, end: offset + 2,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(geometry)),
        parameter_range: None,
    }, geometry_storage)
}

fn curve_node_bytes() -> usize {
    // One insertion into an empty tree adds one node in core tree_node_bytes.
    let alignment = std::mem::align_of::<u16>()
        .max(std::mem::align_of::<StreamCurve<'_>>())
        .max(std::mem::align_of::<usize>());
    11 * (std::mem::size_of::<u16>() + std::mem::size_of::<StreamCurve<'_>>())
        + 16 * std::mem::size_of::<usize>() + 2 * alignment
}

fn assert_live_bytes(ctx: &DecodeContext<'_>, expected: usize) -> cadmpeg_core::decode::ResourceLimit {
    let expected = u64::try_from(expected).unwrap();
    let additional = SCRATCH_BYTES - expected + 1;
    let Err(CodecError::ResourceLimit(limit)) = ctx.reserve_scoped(additional, "test live stream backing") else {
        panic!("live backing boundary");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "test live stream backing");
    assert_eq!(limit.used, expected);
    assert_eq!(limit.additional, additional);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    limit
}

#[test]
fn cross_stream_collision_releases_the_losing_geometry_and_source_shell() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let mut target = StreamCarrierIndex::empty(&ctx).unwrap();
    let mut source = StreamCarrierIndex::empty(&ctx).unwrap();
    target.curves.replace(&ctx, 41, curve(&ctx, 41, 10), "test target curves").unwrap();
    source.curves.replace(&ctx, 41, curve(&ctx, 41, 20), "test source curves").unwrap();
    target.merge_missing(&ctx, source).unwrap();
    assert_eq!(target.curves.entries[&41].value.carrier().offset, 10);
    let limit = assert_live_bytes(&ctx, curve_node_bytes() + CURVE_BYTES);
    drop(target);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn cross_stream_survivors_move_without_recommitting_their_geometry() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let mut target = StreamCarrierIndex::empty(&ctx).unwrap();
    let mut source = StreamCarrierIndex::empty(&ctx).unwrap();
    source.curves.replace(&ctx, 41, curve(&ctx, 41, 10), "test source curves").unwrap();
    source.curves.replace(&ctx, 42, curve(&ctx, 42, 20), "test source curves").unwrap();
    target.merge_missing(&ctx, source).unwrap();
    assert_eq!(target.curves.entries.len(), 2);
    assert_eq!(target.curves.entries[&41].value.carrier().offset, 10);
    assert_eq!(target.curves.entries[&42].value.carrier().offset, 20);
    // Every surviving node/child is scoped. Destruction releases the complete
    // backing; no retained publication is involved in the source index.
    drop(target);
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test stream teardown").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn within_stream_replacement_releases_the_old_geometry() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let mut index = StreamCarrierIndex::empty(&ctx).unwrap();
    index.curves.replace(&ctx, 41, curve(&ctx, 41, 10), "test source curves").unwrap();
    index.curves.replace(&ctx, 41, curve(&ctx, 41, 20), "test source curves").unwrap();
    assert_eq!(index.curves.entries[&41].value.carrier().offset, 20);
    let limit = assert_live_bytes(&ctx, curve_node_bytes() + CURVE_BYTES);
    drop(index);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn occupied_entry_keeps_the_first_value_and_releases_the_attempt() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let mut index = StreamCarrierIndex::empty(&ctx).unwrap();
    index.curves.keep_first(&ctx, 41, curve(&ctx, 41, 10), "test source curves").unwrap();
    index.curves.keep_first(&ctx, 41, curve(&ctx, 41, 20), "test source curves").unwrap();
    assert_eq!(index.curves.entries[&41].value.carrier().offset, 10);
    let limit = assert_live_bytes(&ctx, curve_node_bytes() + CURVE_BYTES);
    drop(index);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn merge_preserves_the_original_refusal_before_allocating_destination_nodes() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let mut target = StreamCarrierIndex::empty(&ctx).unwrap();
    let mut source = StreamCarrierIndex::empty(&ctx).unwrap();
    source.curves.replace(&ctx, 41, curve(&ctx, 41, 10), "test source curves").unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(u64::MAX, "test original stream refusal") else {
        panic!("original refusal");
    };
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| target.merge_missing(&ctx, source));
    assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == limit));
    assert_eq!(allocations, 0);
    assert!(target.curves.entries.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(limit));
    drop(target);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn scoped_merge_admits_one_source_entry_before_visiting_a_nonempty_suffix() {
    crate::test_support::work_refusal_at("merge SLDPRT curve carriers", |ctx| {
        let mut target = StreamCarrierIndex::empty(ctx)?;
        let mut source = StreamCarrierIndex::empty(ctx)?;
        source.curves.replace(ctx, 41, curve(ctx, 41, 10), "test source curves")?;
        source.curves.replace(ctx, 42, curve(ctx, 42, 20), "test source curves")?;
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| target.merge_missing(ctx, source));
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(limit.operation, "merge SLDPRT curve carriers");
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.additional, 1);
            assert!(target.curves.entries.is_empty());
            assert_eq!(allocations, 0);
            assert_eq!(ctx.resource_refusal(), Some(*limit));
        }
        result
    });
}
