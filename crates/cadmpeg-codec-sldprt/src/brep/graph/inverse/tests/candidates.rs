// SPDX-License-Identifier: Apache-2.0
//! Inverse candidates own temporary pair backing through scalar selection.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::brep::graph::inverse::sampled_parameter_minima;
use crate::brep::graph::inverse::InverseResolution;

const SCRATCH_BYTES: u64 = 16 * 1024 * 1024;
const SPAN: [f64; 2] = [0.0, 1.0];

fn policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy.limits.max_retained_bytes = 0;
    policy
}

#[test]
fn inverse_candidates_keep_only_the_copied_sample_pairs_live() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let candidates = sampled_parameter_minima(&ctx, &SPAN, SPAN, |parameter| Ok(Some(parameter))).unwrap().unwrap();
    // A strictly increasing objective has no interior sampled minimum. Core
    // reserves exactly the required 33 slots when extending an empty pair Vec.
    let expected = u64::try_from(33 * std::mem::size_of::<(f64, f64)>()).unwrap();
    assert_eq!(candidates.values.len(), 33);
    assert_eq!(candidates.values.first(), Some(&(0.0, 0.0)));
    assert_eq!(candidates.values.last(), Some(&(1.0, 1.0)));
    let additional = SCRATCH_BYTES - expected + 1;
    let Err(CodecError::ResourceLimit(limit)) = ctx.reserve_scoped(additional, "test live inverse candidate pairs") else { panic!("live candidate backing"); };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.limit, SCRATCH_BYTES);
    assert_eq!((limit.used, limit.additional), (expected, additional));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    drop(candidates);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn inverse_scalar_selection_releases_every_candidate_and_sample_slot() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let candidates = sampled_parameter_minima(&ctx, &SPAN, SPAN, |parameter| Ok(Some(parameter))).unwrap().unwrap();
    assert!(matches!(candidates.resolve(0.0, SPAN).unwrap(), InverseResolution::Unique(0.0)));
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test released inverse candidates").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn inverse_objective_rejection_releases_span_and_partial_candidate_storage() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let mut calls = 0;
    let result = sampled_parameter_minima(&ctx, &[0.0, 1.0, 2.0], [0.0, 2.0], |parameter| {
        calls += 1;
        Ok((parameter < 1.5).then_some(parameter))
    }).unwrap();
    assert!(result.is_none());
    drop(result);
    // The first span has 33 samples; the second reaches 1.5 on sample 16.
    assert_eq!(calls, 33 + 17);
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test released rejected inverse candidates").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn inverse_span_backing_refuses_before_any_allocation_or_objective_call() {
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut calls = 0;
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| sampled_parameter_minima(&ctx, &SPAN, SPAN, |parameter| {
        calls += 1;
        Ok(Some(parameter))
    }));
    let CodecError::ResourceLimit(limit) = result.err().expect("span backing refusal") else { panic!("resource refusal"); };
    assert_eq!(calls, 0);
    assert_eq!(allocations, 0);
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "sample Parasolid inverse span");
    assert_eq!((limit.used, limit.additional), (0, u64::try_from(33 * std::mem::size_of::<(f64, f64)>()).unwrap()));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn inverse_empty_source_preserves_the_original_sticky_refusal() {
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "test original inverse refusal") else { panic!("original refusal"); };
    let result = sampled_parameter_minima(&ctx, &[], SPAN, |_| panic!("no objective after refusal"));
    assert!(matches!(&result, Err(CodecError::ResourceLimit(actual)) if *actual == limit));
    drop(result);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}
