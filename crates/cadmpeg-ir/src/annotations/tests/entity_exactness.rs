// SPDX-License-Identifier: Apache-2.0

use super::{AnnotationBuilder, Exactness};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const ID: &str = "test:model:point#exactness";

#[test]
fn entity_exactness_preserves_each_refusal_before_insertion() {
    for owned in [false, true] {
        for dimension in [ResourceDimension::CollectionItems, ResourceDimension::MaterializedBytes,
            ResourceDimension::RetainedBytes, ResourceDimension::WorkUnits] {
            let mut builder = AnnotationBuilder::new();
            let before = builder.annotations().clone();
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = if owned && dimension == ResourceDimension::MaterializedBytes {
                ctx.with_scoped_storage("entity exactness transaction", || {
                    builder.exactness_owned(&ctx, ID.to_owned(), Exactness::Derived).map(|_| ())
                }).map(|_| ())
            } else if owned { builder.exactness_owned(&ctx, ID.to_owned(), Exactness::Derived).map(|_| ()) }
                else { builder.exactness(&ctx, ID, Exactness::Derived).map(|_| ()) };
            let Err(CodecError::ResourceLimit(limit)) = result else { panic!("entity exactness admission must refuse"); };
            assert_eq!(limit.dimension, dimension);
            assert_eq!(builder.annotations(), &before);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
        }
    }
}

#[test]
fn entity_exactness_owned_key_needs_no_temporary_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let identity = ctx.copy_retained_text(ID, "entity exactness owned identity").unwrap();
    let pointer = identity.as_ptr();
    let mut builder = AnnotationBuilder::new();
    builder.exactness_owned(&ctx, identity, Exactness::Derived).unwrap();
    let (identity, note) = builder.annotations().exactness().first_key_value().unwrap();
    assert_eq!(identity.as_ptr(), pointer);
    assert_eq!(identity, ID);
    assert_eq!(note.entity(), Exactness::Derived);
    ctx.finish_session().unwrap();
}

#[test]
fn entity_exactness_keeps_field_overrides_on_work_refusal() {
    for owned in [false, true] {
        let setup = cadmpeg_test_support::service_decode_context();
        let mut builder = AnnotationBuilder::new();
        builder.field_exactness(&setup, ID, "position", Exactness::Derived).unwrap();
        let before = builder.annotations().clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = if owned { builder.exactness_owned(&ctx, ID.to_owned(), Exactness::Derived) }
            else { builder.exactness(&ctx, ID, Exactness::Derived) };
        let Err(CodecError::ResourceLimit(limit)) = result else { panic!("field retention work must refuse"); };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(builder.annotations(), &before);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
    }
}
