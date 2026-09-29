// SPDX-License-Identifier: Apache-2.0
//! Geometry decode unknown-record admission.

use cadmpeg_core::decode::{ResourceDimension};

use super::{retain_live_annotations, unknown_stream_metadata};

fn geometry_route_limit_error(adjust: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy)) -> cadmpeg_core::CodecError {
    let bytes = crate::test_support::test_prt::prt_with_partition(
        &crate::test_support::test_streams::topology_partition_stream(),
    );
    
    
    crate::test_support::with_decode_context_over(&bytes, |_| {}, |scan_ctx| {
let scan_root = cadmpeg_core::decode::View::over_retained(&bytes);

    let scan = crate::decode::scan(scan_ctx, scan_root).expect("valid topology container");
    let (dialects, _) = crate::dialect::classify_layers(scan_ctx, &scan)
        .expect("classified topology input")
        .into_report_parts();
    
    crate::test_support::with_decode_context_over(&bytes, adjust, |ctx| {
let root = cadmpeg_core::decode::View::over_retained(&bytes);

    match super::try_decode_geometry(ctx, root, &scan, &dialects, &[], &[], &mut 0) {
        Err(error) => error,
        Ok(_) => panic!("geometry route must refuse the low limit"),
    }

})

})
}

#[test]
fn geometry_route_refuses_collection_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| { policy.limits.max_collection_items = 0; };
    assert!(matches!(
        geometry_route_limit_error(adjust_policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
    ));
}

#[test]
fn geometry_route_refuses_retained_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| { policy.limits.max_retained_bytes = 0; };
    assert!(matches!(
        geometry_route_limit_error(adjust_policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
    ));
}

#[test]
fn geometry_route_refuses_scoped_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| { policy.limits.max_materialized_bytes = 0; };
    assert!(matches!(
        geometry_route_limit_error(adjust_policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
    ));
}

#[test]
fn geometry_route_refuses_work_limit() {
    let adjust_policy = |policy: &mut cadmpeg_core::decode::DecodePolicy| { policy.limits.max_work_units = 0; };
    assert!(matches!(
        geometry_route_limit_error(adjust_policy),
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
    
    
    
    crate::test_support::with_decode_context_over(&[], |policy| { policy.limits.max_collection_items = 0; }, |ctx| {

    let error = retain_live_annotations(
        ctx,
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

})
}
