// SPDX-License-Identifier: Apache-2.0
//! Geometry decode unknown-record admission.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

use super::{
    push_unknown_link, reserve_unknown_pair, retain_live_annotations, unknown_stream_metadata,
};

fn geometry_route_limit_error(policy: &DecodePolicy) -> cadmpeg_core::CodecError {
    let bytes = crate::test_support::test_prt::prt_with_partition(
        &crate::test_support::test_streams::topology_partition_stream(),
    );
    let scan_arena = DecodeArena::new();
    let scan_policy = DecodePolicy::service();
    let (scan_ctx, scan_root) = DecodeContext::from_root_bytes(&bytes, &scan_arena, &scan_policy)
        .expect("bounded topology input");
    let scan = crate::decode::scan(&scan_ctx, scan_root).expect("valid topology container");
    let (dialects, _) = crate::dialect::classify_layers(&scan_ctx, &scan)
        .expect("classified topology input")
        .into_report_parts();
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, policy)
        .expect("root input fits policy");
    match super::try_decode_geometry(
        &ctx,
        root,
        &scan,
        &dialects,
        &[],
        &[],
        &mut 0,
    ) {
        Err(error) => error,
        Ok(_) => panic!("geometry route must refuse the low limit"),
    }
}

#[test]
fn geometry_route_refuses_collection_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    assert!(matches!(
        geometry_route_limit_error(&policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
    ));
}

#[test]
fn geometry_route_refuses_retained_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    assert!(matches!(
        geometry_route_limit_error(&policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
    ));
}

#[test]
fn geometry_route_refuses_scoped_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    assert!(matches!(
        geometry_route_limit_error(&policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
    ));
}

#[test]
fn geometry_route_refuses_work_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    assert!(matches!(
        geometry_route_limit_error(&policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
    ));
}

fn preview_stream() -> crate::parasolid::Stream {
    crate::parasolid::Stream {
        file_offset: 0,
        consumed: 0,
        inflated: Vec::new(),
        body: crate::parasolid::StreamBody::Preview,
    }
}

fn preview_unknown() -> cadmpeg_ir::unknown::UnknownRecord {
    crate::test_support::with_decode_context(|ctx| {
        unknown_stream_metadata(ctx, 0, &preview_stream())
            .expect("preview metadata fits the service profile")
    })
}

#[test]
fn live_annotations_refuse_first_identity_at_collection_limit() {
    let unknown = preview_unknown();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
    let error = retain_live_annotations(
        &ctx,
        &cadmpeg_ir::document::CadIr::empty(),
        &[unknown],
        &mut cadmpeg_ir::Annotations::default(),
    )
    .expect_err("one live identity needs one slot");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "nx live annotation identities"
    ));
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
    let mut unknown = preview_unknown();
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
    let mut unknown = preview_unknown();
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
    let mut unknown = preview_unknown();
    crate::test_support::with_decode_context(|ctx| {
        push_unknown_link(ctx, &mut unknown, "test:model:entity#surface")
            .expect("related entity fits the service profile");
    });
    assert_eq!(unknown.links(), ["test:model:entity#surface"]);
}
