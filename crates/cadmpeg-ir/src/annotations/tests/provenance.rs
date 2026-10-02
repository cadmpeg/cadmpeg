// SPDX-License-Identifier: Apache-2.0

use super::{AnnotationBuilder, StreamHandle};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const ID: &str = "test:model:point#provenance";

#[test]
fn provenance_construction_preserves_refusals_before_insertion() {
    let stream = StreamHandle::new(&cadmpeg_test_support::service_decode_context(), crate::stream_name!("source"), "fixture stream handle").unwrap();
    for owned in [false, true] {
        for dimension in [ResourceDimension::CollectionItems, ResourceDimension::RetainedBytes,
            ResourceDimension::MaterializedBytes, ResourceDimension::WorkUnits] {
            let mut builder = AnnotationBuilder::new();
            let before = builder.annotations().clone();
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
                _ => unreachable!(),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = if owned && dimension == ResourceDimension::MaterializedBytes {
                ctx.with_scoped_storage("provenance transaction", || builder.note_owned(&ctx, ID.to_owned(), &stream, 7, Some("point"))).map(|_| ())
            } else if owned { builder.note_owned(&ctx, ID.to_owned(), &stream, 7, Some("point")) }
                else { builder.note(&ctx, ID, &stream, 7, Some("point")) };
            let Err(CodecError::ResourceLimit(limit)) = result else { panic!("provenance construction must refuse"); };
            assert_eq!(limit.dimension, dimension);
            assert_eq!(builder.annotations(), &before);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
        }
    }
}

#[test]
fn provenance_replacement_keeps_the_old_location_on_tag_refusal() {
    let setup = cadmpeg_test_support::service_decode_context();
    let stream = StreamHandle::new(&cadmpeg_test_support::service_decode_context(), crate::stream_name!("source"), "fixture stream handle").unwrap();
    for owned in [false, true] {
        let mut builder = AnnotationBuilder::new();
        builder.note(&setup, ID, &stream, 7, Some("original")).unwrap();
        let before = builder.annotations().clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = if owned { builder.note_owned(&ctx, ID.to_owned(), &stream, 11, Some("replacement")) }
            else { builder.note(&ctx, ID, &stream, 11, Some("replacement")) };
        let Err(CodecError::ResourceLimit(limit)) = result else { panic!("replacement tag must refuse"); };
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(limit.operation, "retain source provenance tag");
        assert_eq!(builder.annotations(), &before);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
    }
}

#[test]
fn provenance_owned_identity_moves_without_temporary_storage() {
    let stream = StreamHandle::new(&cadmpeg_test_support::service_decode_context(), crate::stream_name!("source"), "fixture stream handle").unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let identity = ctx.copy_retained_text(ID, "owned provenance identity").unwrap();
    let pointer = identity.as_ptr();
    let mut builder = AnnotationBuilder::new();
    builder.note_owned(&ctx, identity, &stream, 7, Some("point")).unwrap();
    let (identity, provenance) = builder.annotations().provenance.first_key_value().unwrap();
    assert_eq!(identity.as_ptr(), pointer);
    assert_eq!(identity, ID);
    assert_eq!(provenance.stream(), "source");
    assert_eq!(provenance.offset, 7);
    assert_eq!(provenance.tag.as_deref(), Some("point"));
    ctx.finish_session().unwrap();
}
