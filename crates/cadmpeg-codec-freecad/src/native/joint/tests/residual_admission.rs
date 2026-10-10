// SPDX-License-Identifier: Apache-2.0
//! Prefix-only admission for checked joint parameters.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::JointParameters;

#[test]
fn checked_joint_empty_parameters_need_no_work_or_storage_and_keep_fuse() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert_eq!(
        JointParameters::from_raw_charged(&ctx, BTreeMap::new(), "unused").unwrap(),
        JointParameters::default()
    );
    assert_eq!(ctx.resource_refusal(), None);
    let CodecError::ResourceLimit(first) = ctx.charge_work(1, "prior refusal").unwrap_err() else {
        panic!("fuse");
    };
    assert!(
        matches!(JointParameters::from_raw_charged(&ctx, BTreeMap::new(), "unused"),
        Err(CodecError::ResourceLimit(limit)) if limit == first)
    );
}

#[test]
fn checked_joint_first_malformed_parameter_does_not_precharge_suffix() {
    let joint_id = "fcstd:native:joint#Joint";
    let message = format!("joint {joint_id}: joint parameter Angle has an invalid value \"NaN\"");
    // One source visit, the scalar input bytes, and two diagnostic formatting passes.
    let work = 1 + 3 + 2 * cadmpeg_core::decode::u64_from_index(message.len());
    let mut parameters = BTreeMap::from([("Angle".into(), "NaN".into())]);
    for index in 0..1024 {
        parameters.insert(format!("Z{index:04}"), "unused".into());
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let error = JointParameters::from_raw_charged(&ctx, parameters, joint_id)
        .expect_err("first scalar is malformed");
    assert!(matches!(error, CodecError::Malformed(ref value) if value == &message));
    assert_eq!(ctx.resource_refusal(), None);
    let CodecError::ResourceLimit(limit) = ctx.charge_work(1, "after diagnostic").unwrap_err()
    else {
        panic!("prefix exhausted work");
    };
    assert_eq!(limit.used, work);
}

#[test]
fn checked_joint_first_parameter_storage_refusal_does_not_precharge_suffix() {
    let CodecError::ResourceLimit(boundary) = crate::test_support::refusal_at(
        ResourceDimension::WorkUnits,
        &[],
        "fcstd joint checked parameters",
        |ctx| {
            JointParameters::from_raw_charged(
                ctx,
                BTreeMap::from([("Native".into(), "value".into())]),
                "fcstd:native:joint#Joint",
            )
        },
    ) else {
        panic!("tree insertion boundary");
    };
    let mut parameters = BTreeMap::from([("Native".into(), "value".into())]);
    for index in 0..4096 {
        parameters.insert(format!("Z{index:04}"), "unused".into());
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Source visit plus empty-tree insertion work before collection-slot admission.
    policy.limits.max_work_units = boundary.used + boundary.additional;
    assert!(policy.limits.max_work_units < cadmpeg_core::decode::u64_from_index(parameters.len()));
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let CodecError::ResourceLimit(limit) =
        JointParameters::from_raw_charged(&ctx, parameters, "fcstd:native:joint#Joint")
            .expect_err("first map slot refuses")
    else {
        panic!("collection refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "fcstd joint checked parameters");
    assert_eq!(ctx.resource_refusal(), Some(limit));
}
