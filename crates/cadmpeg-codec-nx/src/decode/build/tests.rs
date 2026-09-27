// SPDX-License-Identifier: Apache-2.0
//! Geometry decode unknown-record admission.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

use super::{push_unknown_link, reserve_unknown_pair, unknown_stream_metadata};

fn preview_stream() -> crate::parasolid::Stream {
    crate::parasolid::Stream {
        file_offset: 0,
        consumed: 0,
        inflated: Vec::new(),
        body: crate::parasolid::StreamBody::Preview,
    }
}

#[test]
fn geometry_unknown_records_refuse_first_collection_slot() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
    let error = reserve_unknown_pair(&ctx, &mut Vec::new(), &mut Vec::new())
        .expect_err("unknown record needs one slot");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "nx geometry unknown streams"
    ));
}

#[test]
fn geometry_unknown_indices_refuse_second_collection_slot() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
    let error = reserve_unknown_pair(&ctx, &mut Vec::new(), &mut Vec::new())
        .expect_err("stream index needs a second slot");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "nx geometry unknown indices"
    ));
}

#[test]
fn unknown_entity_links_refuse_collection_limit() {
    let mut unknown = unknown_stream_metadata(0, &preview_stream());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
    let error = push_unknown_link(&ctx, &mut unknown, "test:model:entity#surface")
        .expect_err("one related entity needs one slot");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "nx unknown entity links"
    ));
}

#[test]
fn unknown_entity_links_refuse_retained_text_limit() {
    let mut unknown = unknown_stream_metadata(0, &preview_stream());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
    let error = push_unknown_link(&ctx, &mut unknown, "test:model:entity#surface")
        .expect_err("related entity identity needs retained bytes");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "nx unknown entity link text"
    ));
}

#[test]
fn unknown_entity_links_keep_identity_under_service_profile() {
    let mut unknown = unknown_stream_metadata(0, &preview_stream());
    crate::test_support::with_decode_context(|ctx| {
        push_unknown_link(ctx, &mut unknown, "test:model:entity#surface")
            .expect("related entity fits the service profile");
    });
    assert_eq!(unknown.links(), ["test:model:entity#surface"]);
}
