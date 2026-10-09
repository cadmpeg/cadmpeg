// SPDX-License-Identifier: Apache-2.0
//! Live stream-index backing after intersection source and candidate teardown.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::SolvedCurveGeometry;

use super::publication::{IntersectionSink, LossSink};
use super::stream::{StreamCarrierIndex, StreamCurve, StreamLoss};
use crate::brep::intersection::scan_intersection_carriers_into;

const SCRATCH_BYTES: u64 = 16 * 1024 * 1024;
const GEOMETRY_BYTES: usize = 4 * std::mem::size_of::<f64>()
    + 2 * std::mem::size_of::<cadmpeg_ir::features::FinitePoint3>();
const UV_BYTES: usize = 4 * std::mem::size_of::<cadmpeg_ir::math::Point2>();
const GEOMETRY_LOSS: &str = "intersection chart for attr 9: sldprt intersection chart curve for record attr 9: knots contains a non-finite value; the carrier is not emitted and the entities that reference it fall back to an untyped support.";

fn policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy
}

fn chart(end: f64, scale: f64, tolerance: f64) -> Vec<u8> {
    let mut bytes = vec![0, 0x28];
    bytes.extend_from_slice(&2_u32.to_be_bytes());
    bytes.extend_from_slice(&4_u16.to_be_bytes());
    bytes.extend_from_slice(&0.0_f64.to_be_bytes());
    bytes.extend_from_slice(&scale.to_be_bytes());
    bytes.extend_from_slice(&2_u32.to_be_bytes());
    bytes.extend_from_slice(&tolerance.to_be_bytes());
    bytes.extend_from_slice(&[0; 8]);
    for _ in 0..2 { bytes.extend_from_slice(&(-31_415_800_000_000.0_f64).to_be_bytes()); }
    for value in [0.0_f64, 0.0, 0.0, end, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    bytes
}

fn source(charts: impl IntoIterator<Item = Vec<u8>>, uv: bool) -> Vec<u8> {
    let mut bytes = vec![0, 0x26];
    bytes.extend_from_slice(&9_u16.to_be_bytes());
    bytes.extend_from_slice(&[0; 14]);
    bytes.push(0x2b);
    for reference in [2_u16, 3, 4, 5, 6, 7] { bytes.extend_from_slice(&reference.to_be_bytes()); }
    for chart in charts { bytes.extend(chart); }
    for (attr, point) in [(5_u16, [0.0_f64, 0.0, 0.0]), (6, [2.0, 0.0, 0.0])] {
        bytes.extend_from_slice(&[0, 0x29]);
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(&attr.to_be_bytes());
        bytes.extend_from_slice(b"L?");
        for value in point { bytes.extend_from_slice(&value.to_be_bytes()); }
    }
    if uv {
        bytes.extend_from_slice(&[0, 0xcc]);
        bytes.extend_from_slice(&8_u32.to_be_bytes());
        bytes.extend_from_slice(&7_u16.to_be_bytes());
        bytes.push(4);
        for value in [0.0_f64, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
    }
    bytes
}

fn collect<'ctx>(ctx: &'ctx DecodeContext<'_>, bytes: &[u8], out: &mut StreamCarrierIndex<'ctx>) -> Result<(), CodecError> {
    scan_intersection_carriers_into(ctx, bytes, &mut LossSink::Stream(&mut out.lane_refusals),
        &mut IntersectionSink::Stream(&mut out.curves))
}

fn curve_node_bytes() -> usize {
    let alignment = std::mem::align_of::<u16>()
        .max(std::mem::align_of::<StreamCurve<'_>>())
        .max(std::mem::align_of::<usize>());
    11 * (std::mem::size_of::<u16>() + std::mem::size_of::<StreamCurve<'_>>())
        + 16 * std::mem::size_of::<usize>() + 2 * alignment
}

fn assert_live_bytes(ctx: &DecodeContext<'_>, live: usize) -> cadmpeg_core::decode::ResourceLimit {
    let live = u64::try_from(live).expect("live output backing");
    let additional = SCRATCH_BYTES - live + 1;
    let Err(CodecError::ResourceLimit(limit)) = ctx.reserve_scoped(additional, "test final intersection backing") else {
        panic!("live output exceeds remaining scratch by one byte");
    };
    assert_eq!(limit.operation, "test final intersection backing");
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.used, live);
    assert_eq!(limit.additional, additional);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    limit
}

fn selected_curve(charts: Vec<Vec<u8>>, uv: bool) {
    let bytes = source(charts, uv);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).expect("root context");
    let mut out = StreamCarrierIndex::empty(&ctx).expect("empty index");
    collect(&ctx, &bytes, &mut out).expect("intersection selection");
    assert_eq!(out.curves.entries.len(), 1);
    assert!(out.lane_refusals.entries.is_empty());
    let super::IndexedCurve::Derived(carrier) = &out.curves.entries[&9].value else { panic!("derived intersection"); };
    let Some(SolvedCurveGeometry::Nurbs(curve)) = carrier.carrier.geometry.solved() else { panic!("polyline NURBS"); };
    assert_eq!(curve.knots().as_slice(), [0.0, 0.0, 2.0, 2.0]);
    assert_eq!(curve.pole_count(), 2);
    assert_eq!(carrier.support_data.support_uv.is_some(), uv);
    if let Some(lanes) = &carrier.support_data.support_uv {
        assert_eq!(lanes[0], [cadmpeg_ir::math::Point2::new(0.0, 1.0), cadmpeg_ir::math::Point2::new(4.0, 5.0)]);
        assert_eq!(lanes[1], [cadmpeg_ir::math::Point2::new(2.0, 3.0), cadmpeg_ir::math::Point2::new(6.0, 7.0)]);
    }
    // Parameters, raw controls, source charts/terms/UVs and source shells are
    // gone. Only unchanged knots, converted poles, optional two UV lanes and
    // the actual typed destination node survive.
    let limit = assert_live_bytes(&ctx, curve_node_bytes() + GEOMETRY_BYTES + if uv { UV_BYTES } else { 0 });
    drop(out);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn stream_intersection_releases_parameters_after_optional_uv_construction() {
    selected_curve(vec![chart(2.0, 1.0, 0.00001)], true);
}

#[test]
fn stream_intersection_releases_replaced_candidate_backing() {
    selected_curve(vec![chart(3.0, 1.0, 0.00001), chart(2.0, 1.0, 0.00001)], false);
}

#[test]
fn stream_intersection_releases_worse_candidate_backing() {
    selected_curve(vec![chart(2.0, 1.0, 0.00001), chart(3.0, 1.0, 0.00001)], false);
}

#[test]
fn stream_intersection_ambiguity_releases_all_candidate_and_source_backing() {
    let bytes = source([chart(2.0, 1.0, 0.00001), chart(2.0, 1.0, 0.00001)], false);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).expect("root context");
    let mut out = StreamCarrierIndex::empty(&ctx).expect("empty index");
    collect(&ctx, &bytes, &mut out).expect("structural ambiguity");
    assert!(out.curves.entries.is_empty());
    assert!(out.lane_refusals.entries.is_empty());
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test ambiguous intersection release").expect("all scratch capacity"));
    drop(out);
    ctx.finish_session().expect("structural recovery preserves the session");
}

#[test]
fn stream_intersection_late_tolerance_failure_releases_the_solved_candidate() {
    let bytes = source([chart(2.0, 1.0, f64::MAX)], false);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).expect("root context");
    let mut out = StreamCarrierIndex::empty(&ctx).expect("empty index");
    collect(&ctx, &bytes, &mut out).expect("late tolerance rejection");
    assert!(out.curves.entries.is_empty());
    assert!(out.lane_refusals.entries.is_empty());
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test late tolerance release").expect("all scratch capacity"));
    drop(out);
    ctx.finish_session().expect("structural recovery preserves the session");
}

#[test]
fn stream_intersection_collision_releases_the_later_geometry_and_uv() {
    let first = source([chart(2.0, 1.0, 0.00001)], false);
    let later = source([chart(2.0, 1.0, 0.00002)], true);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).expect("root context");
    let mut out = StreamCarrierIndex::empty(&ctx).expect("empty index");
    collect(&ctx, &first, &mut out).expect("first source");
    collect(&ctx, &later, &mut out).expect("occupied source still parsed");
    let super::IndexedCurve::Derived(carrier) = &out.curves.entries[&9].value else { panic!("first derived intersection"); };
    assert_eq!(carrier.support_data.fit_tolerance_mm, 0.01);
    assert!(carrier.support_data.support_uv.is_none());
    let limit = assert_live_bytes(&ctx, curve_node_bytes() + GEOMETRY_BYTES);
    drop(out);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn stream_intersection_loss_survives_without_intermediate_messages_or_geometry() {
    let bytes = source([chart(2.0, f64::MAX, f64::MAX)], false);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).expect("root context");
    let mut out = StreamCarrierIndex::empty(&ctx).expect("empty index");
    collect(&ctx, &bytes, &mut out).expect("geometry rejection before late tolerance");
    assert!(out.curves.entries.is_empty());
    let [loss] = out.lane_refusals.entries.as_slice() else { panic!("one final loss"); };
    assert_eq!(loss.value.message, GEOMETRY_LOSS);
    assert_eq!(loss.value.code, crate::loss::SldprtLossCode::GeometrySplineLanesUnpaired.kind());
    let live = GEOMETRY_LOSS.len() + 4 * std::mem::size_of::<StreamLoss<'_>>();
    let limit = assert_live_bytes(&ctx, live);
    drop(out);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}
