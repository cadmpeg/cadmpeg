// SPDX-License-Identifier: Apache-2.0

use super::StreamHandle;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn stream_handle_constructor_preserves_the_original_refusal() {
    for dimension in [ResourceDimension::RetainedBytes, ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits, ResourceDimension::MaterializedBytes] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            _ => unreachable!(),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let create = || StreamHandle::new(&ctx, crate::stream_name!("source"), "source handle");
        let result = if dimension == ResourceDimension::MaterializedBytes {
            ctx.with_scoped_storage("temporary source handle", create).map(|_| ())
        } else { create().map(|_| ()) };
        let Err(CodecError::ResourceLimit(limit)) = result else { panic!("stream storage must refuse"); };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, "source handle");
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
    }
}

#[test]
fn stream_handle_moves_owned_name_without_a_temporary_copy() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let text = ctx.copy_retained_text("owned stream", "source name").unwrap();
    let pointer = text.as_ptr();
    let name = crate::StreamName::try_from(text).unwrap();
    let handle = StreamHandle::new(&ctx, name, "source handle").unwrap();
    assert_eq!(handle.0.as_str(), "owned stream");
    assert_eq!(handle.0.as_str().as_ptr(), pointer);
    ctx.finish_session().unwrap();
}
