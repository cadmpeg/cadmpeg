// SPDX-License-Identifier: Apache-2.0
//! Source-bound selection and scalar mutation refusal boundaries.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use super::{CompactCurveFamily, CompactCurvePatch};

const SCRATCH_BYTES: u64 = 16 * 1024 * 1024;
const CIRCLE_VALUES: [f64; 10] = [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.004];

fn policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy.limits.max_retained_bytes = 0;
    policy
}

fn circle() -> Vec<u8> {
    crate::test_support::parasolid::circle_carrier(10, [0.0; 3], [0.0, 0.0, 1.0], 0.002)
}

#[test]
fn compact_write_reuses_selection_without_a_second_carrier_scan() {
    let mut body = circle();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let patch = CompactCurvePatch::new(&ctx, &mut body, 10).unwrap();
    let Some(CompactCurveFamily::Circle(frame)) = patch.family() else { panic!("circle frame"); };
    assert_eq!(*frame.axis().as_raw(), cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test expired patch source index").unwrap());
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "scan SLDPRT analytic carriers", None);
    assert!(patch.apply(&CIRCLE_VALUES).unwrap());
    let Some(crate::brep::Carrier::Curve(carrier)) = crate::brep::parse_carrier(&body, 0) else { panic!("patched circle"); };
    let Some(SolvedCurveGeometry::Circle(curve)) = carrier.geometry.solved() else { panic!("circle family"); };
    assert_eq!(curve.radius().get(), 4.0);
    // The original source-scan target remains armed after the actual write.
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "scan SLDPRT analytic carriers") else { panic!("unvisited rescan"); };
    assert_eq!(limit.additional, 1);
    drop(probe);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn nonanalytic_selection_copies_no_geometry_and_releases_its_source_index() {
    let mut body = crate::test_support::parasolid::nurbs_curve_carrier(10, 11);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let patch = CompactCurvePatch::new(&ctx, &mut body, 10).unwrap();
    assert!(matches!(patch.family(), Some(CompactCurveFamily::Other)));
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test expired NURBS patch source index").unwrap());
    drop(patch);
    ctx.finish_session().unwrap();
}

#[test]
fn scalar_write_refuses_before_mutation_with_the_actual_byte_request() {
    let mut body = circle();
    let original = body.clone();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let patch = CompactCurvePatch::new(&ctx, &mut body, 10).unwrap();
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "patch SLDPRT compact values", None);
    let Err(CodecError::ResourceLimit(limit)) = patch.apply(&CIRCLE_VALUES) else { panic!("scalar write refusal"); };
    drop(probe);
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "patch SLDPRT compact values");
    assert_eq!(limit.additional, u64::try_from(CIRCLE_VALUES.len() * std::mem::size_of::<f64>()).unwrap());
    assert_eq!(body, original);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn source_selection_preserves_prior_refusal_before_allocation() {
    let mut body = circle();
    let original = body.clone();
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_collection_items(1, "test original patch refusal") else { panic!("original refusal"); };
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| CompactCurvePatch::new(&ctx, &mut body, 10));
    assert!(matches!(&result, Err(CodecError::ResourceLimit(actual)) if *actual == limit));
    assert_eq!(allocations, 0);
    drop(result);
    assert_eq!(body, original);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn missing_selection_observes_a_later_original_refusal() {
    let mut body = circle();
    let original = body.clone();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let patch = CompactCurvePatch::new(&ctx, &mut body, 99).unwrap();
    assert!(patch.family().is_none());
    let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "test later patch refusal", Some(1));
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "test later patch refusal") else { panic!("later refusal"); };
    drop(probe);
    assert!(matches!(patch.apply(&[]), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    assert_eq!(body, original);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}
