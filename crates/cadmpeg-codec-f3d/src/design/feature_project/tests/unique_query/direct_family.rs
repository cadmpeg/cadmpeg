// SPDX-License-Identifier: Apache-2.0
//! Actual feature queries retain source identity and the first refusal.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut};
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, FeatureOperation, ThickenSide};

mod fixtures;
mod boundaries;
mod operands;

fn assert_refusal<T>(operation: &'static str, project: impl FnOnce(&DecodeContext<'_>) -> Result<T, CodecError>) {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, Some(1));
        let Err(CodecError::ResourceLimit(original)) = project(ctx) else {
            panic!("actual feature source phase must refuse");
        };
        assert_eq!(original.operation, operation);
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(matches!(ctx.charge_work(0, "later feature query boundary"), Err(CodecError::ResourceLimit(later)) if later == original));
    });
}

fn thread(operation: &'static str) {
    let scope = super::scope();
    let groups = [super::group(77, 0, 100, vec![101], DesignOperandRole::ROLE_0X10)];
    let original = groups.clone();
    assert_refusal(operation, |ctx| super::super::super::project_thread_face_selection(ctx, &scope, &[100], &groups, &[]));
    assert_eq!(groups, original);
}

#[test]
fn direct_unique_thread_scope_stream_preserves_original_refusal() {
    thread("resolve F3D Thread face query scope stream");
}

#[test]
fn direct_unique_thread_source_preserves_original_refusal() {
    thread("visit F3D Thread face group source");
}

#[test]
fn direct_unique_thread_stream_preserves_original_refusal() {
    thread("match F3D Thread face group stream");
}

#[test]
fn direct_unique_thread_duplicate_skips_unvisited_suffix_and_all_output_storage() {
    let scope = super::scope();
    let first = super::group(77, 0, 100, vec![101], DesignOperandRole::ROLE_0X10);
    let mut tail = first.clone();
    tail.id = format!("f3d:Other/BulkStream.dat:{}", "x".repeat(4096));
    let groups = [first.clone(), first, tail];
    let original = groups.clone();
    let stream = crate::ids::native_stream(&scope.id).unwrap();
    let group_stream = crate::ids::native_stream(&groups[0].id).unwrap();
    let reached = u64::try_from(scope.id.len() - stream.len()
        + 2 * (1 + groups[0].id.len() - group_stream.len() + stream.len())).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = reached;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(super::super::super::project_thread_face_selection(ctx, &scope, &[100], &groups, &[]).unwrap(), FaceSelection::Unresolved);
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "observe Thread reached work") else { panic!("source-derived reached work"); };
        assert_eq!((original.used, original.additional), (reached, 1));
    });
    policy.limits.max_work_units -= 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = super::super::super::project_thread_face_selection(ctx, &scope, &[100], &groups, &[]) else { panic!("actual last compared byte must refuse"); };
        assert_eq!(original.operation, "match F3D Thread face group stream");
        assert_eq!((original.used, original.additional), (reached - 1, 1));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
    assert_eq!(groups, original);
}

