// SPDX-License-Identifier: Apache-2.0

use crate::draft::ModelCheckpoint;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn model_checkpoint_preserves_each_resource_refusal() {
    let draft = super::feature_parents::parent_draft();
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
        ResourceDimension::RecursionDepth,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
            _ => panic!("checkpoint dimensions"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let CodecError::ResourceLimit(first) =
            ModelCheckpoint::capture(draft.model(), &ctx).unwrap_err()
        else {
            panic!("checkpoint must refuse");
        };
        assert_eq!(first.dimension, dimension);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == first)
        );
    }
}

#[test]
fn model_checkpoint_holds_scoped_parents_and_restores_admitted_relations() {
    let draft = super::feature_parents::parent_draft();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 4096;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let checkpoint = ModelCheckpoint::capture(draft.model(), &ctx).unwrap();
    assert!(checkpoint.0.same_state(&checkpoint.0, &ctx).unwrap());
    let mut model = draft.model().clone();
    let CodecError::ResourceLimit(first) =
        checkpoint.0.discard_appended(&mut model, &ctx).unwrap_err()
    else {
        panic!("restored parents need retained storage");
    };
    assert_eq!(first.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(model, *draft.model());
    drop(checkpoint);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == first)
    );

    let arena = DecodeArena::new();
    policy.limits.max_retained_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let checkpoint = ModelCheckpoint::capture(draft.model(), &ctx).unwrap();
    let mut model = draft.model().clone();
    model
        .points
        .push(super::point("test:checkpoint:point#appended"));
    checkpoint.0.discard_appended(&mut model, &ctx).unwrap();
    assert_eq!(model, *draft.model());
    drop(checkpoint);
    let all_storage = ctx
        .reserve_scoped(4096, "checkpoint scratch released")
        .unwrap();
    drop(all_storage);
    ctx.finish_session().unwrap();
}

#[test]
fn model_checkpoint_parent_comparison_preserves_work_refusal() {
    let draft = super::feature_parents::parent_draft();
    let capture_context = cadmpeg_test_support::service_decode_context();
    let checkpoint = ModelCheckpoint::capture(draft.model(), &capture_context).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(first) =
        checkpoint.0.same_state(&checkpoint.0, &ctx).unwrap_err()
    else {
        panic!("comparison must refuse");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == first)
    );
}
