// SPDX-License-Identifier: Apache-2.0
//! Parameter samples own their complete backing through fit checks.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::Point3;

use super::{nurbs_curve_sample_parameters, NurbsPcurveFailure};

const SCRATCH_BYTES: u64 = 16 * 1024 * 1024;

fn curve() -> NurbsCurve {
    NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None, false,
    ).unwrap().unwrap()
}

fn policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy
}

#[test]
fn parameter_samples_release_all_backing_after_fit_use() {
    let curve = curve();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let samples = nurbs_curve_sample_parameters(&ctx, &curve, [0.0, 1.0]).unwrap().unwrap();
    assert_eq!(samples.values, [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0]);
    drop(samples);
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test released parameter samples").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn parameter_samples_keep_seed_and_growth_backing_live() {
    let curve = curve();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let samples = nurbs_curve_sample_parameters(&ctx, &curve, [0.0, 1.0]).unwrap().unwrap();
    let expected = u64::try_from(16 * std::mem::size_of::<f64>()).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.reserve_scoped(SCRATCH_BYTES - expected + 1, "test live parameter samples") else {
        panic!("live sample storage");
    };
    // Two initial endpoint slots plus nine span positions require eleven
    // entries before deduplication. Exact seed capacity2 grows4,8,16 through
    // core's amortized f64 growth. Dedup retains the sixteen-slot backing.
    assert_eq!(limit.used, expected);
    assert_eq!(limit.additional, SCRATCH_BYTES - expected + 1);
    assert_eq!(limit.limit, SCRATCH_BYTES);
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    drop(samples);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn parameter_sample_endpoints_refuse_before_backing_allocation() {
    let curve = curve();
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(||
        nurbs_curve_sample_parameters(&ctx, &curve, [0.0, 1.0]));
    let Some(NurbsPcurveFailure::Resource(CodecError::ResourceLimit(limit))) = result.err() else {
        panic!("endpoint storage refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "collect NURBS sample endpoints");
    assert_eq!((limit.used, limit.additional), (0, u64::try_from(2 * std::mem::size_of::<f64>()).unwrap()));
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn rejected_sample_domain_needs_no_storage_or_scan_work() {
    let curve = curve();
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(nurbs_curve_sample_parameters(&ctx, &curve, [1.0, 0.0]).unwrap().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn rejected_sample_domain_preserves_the_original_sticky_refusal() {
    let curve = curve();
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "test original sample refusal") else {
        panic!("original work refusal");
    };
    let result = nurbs_curve_sample_parameters(&ctx, &curve, [1.0, 0.0]);
    assert!(matches!(result, Err(NurbsPcurveFailure::Resource(CodecError::ResourceLimit(actual))) if actual == limit));
    drop(result);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}
