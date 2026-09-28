// SPDX-License-Identifier: Apache-2.0

use crate::native::features::feature_projected_curve_references;
use crate::native::features::feature_surface_construction_references;
use crate::native::features::draft::feature_draft_construction_references;

fn reference_container(label: &'static str, payload: Vec<u8>) -> crate::container::Container<'static> {
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], label, payload)], &[],
    );
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    crate::test_support::with_decode_context(move |ctx| crate::container::scan_bytes(ctx, file))
        .expect("synthetic feature reference container")
}

fn projected_curve_container() -> crate::container::Container<'static> {
    let payload = b"\0\x01\x02\xf1\x02\xc8\xf1\x02\xc9\x80\x57\x00\x02\x01\xf1\x02\xca\xff\x01\x02\x02\x7d\0".to_vec();
    reference_container("CPROJ", payload)
}

fn surface_container() -> crate::container::Container<'static> {
    reference_container("SKIN", b"\x3f\x00\x00\x01\x00\xf1\x02\x46\xf1\x02\x47\xf1\x02\x48\x01\x09\x03\x03\x04\x05\x02\x01\x01\x01\x01\x09\xf1\x02\x49\xf1\x02\x4a\xf1\x02\x4b\xf1\x02\x4c\xf1\x02\x4d\xf1\x02\x4e\xf1\x02\x4f\xf1\x02\x50\x00\x03\x03\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xf1\x02\x56\xf1\x02\x57\xf1\x02\x58\x01\x01\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x01\x02".to_vec())
}

fn draft_container() -> crate::container::Container<'static> {
    let mut payload = b"\x67\x00\x00\x01\x00\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\xff\xff\xff\xff\xff\xff\xff\xff\x01\x03\x80\x94\x82\x49".to_vec();
    payload.extend_from_slice(b"\x01\x02\xf1\x1b\x7c\x01\x02\xf1\x1b\x7d\x68\x2f\x70\x62\x4d\xd2\xf1\xa9\xfc\x03\x50\x44\x00\x00\x01\x46\x8a\x2a\x01\xa3\x60\x10\x01\x01\x01\x04\x02\x01\x02\x01\x00\x00\x00\x00\x01\xf1\x1b\x7e\xff\x00\x00\x00\xf1\x1b\x7f\xff");
    payload.extend_from_slice(b"\x81\x5e\x80\xb8\x01\x03\x02\x01\x02\x01\x01\x01\x00\x00\x00\x29\x29\x0c\x00");
    reference_container("DRAFT", payload)
}

fn projected_curve_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = projected_curve_container();
    let records = crate::test_support::with_decode_context(|ctx| {
        feature_projected_curve_references(ctx, &container)
    }).expect("admitted projected curve references");
    assert_eq!(records.len(), 3);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    feature_projected_curve_references(&ctx, &container)
        .expect_err("projected curve reference resource limit")
}

fn reference_route_refusal<T>(
    container: crate::container::Container<'static>,
    expected_count: usize,
    route: for<'ctx, 'input> fn(
        &cadmpeg_core::decode::DecodeContext<'ctx>,
        &crate::container::Container<'input>,
    ) -> Result<Vec<T>, cadmpeg_core::CodecError>,
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let records = crate::test_support::with_decode_context(|ctx| route(ctx, &container))
        .expect("admitted feature references");
    assert_eq!(records.len(), expected_count);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    route(&ctx, &container).err().expect("feature reference resource limit")
}

#[test]
fn surface_reference_route_refuses_collection_limit() {
    let error = reference_route_refusal(surface_container(), 14, feature_surface_construction_references,
        |policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn surface_reference_route_refuses_retained_limit() {
    let error = reference_route_refusal(surface_container(), 14, feature_surface_construction_references,
        |policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn surface_reference_route_refuses_scoped_limit() {
    let error = reference_route_refusal(surface_container(), 14, feature_surface_construction_references,
        |policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn surface_reference_route_refuses_work_limit() {
    let error = reference_route_refusal(surface_container(), 14, feature_surface_construction_references,
        |policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

#[test]
fn draft_reference_route_refuses_collection_limit() {
    let error = reference_route_refusal(draft_container(), 4, feature_draft_construction_references,
        |policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn draft_reference_route_refuses_retained_limit() {
    let error = reference_route_refusal(draft_container(), 4, feature_draft_construction_references,
        |policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn draft_reference_route_refuses_scoped_limit() {
    let error = reference_route_refusal(draft_container(), 4, feature_draft_construction_references,
        |policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn draft_reference_route_refuses_work_limit() {
    let error = reference_route_refusal(draft_container(), 4, feature_draft_construction_references,
        |policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

#[test]
fn projected_curve_reference_route_refuses_collection_limit() {
    let error = projected_curve_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn projected_curve_reference_route_refuses_retained_limit() {
    let error = projected_curve_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn projected_curve_reference_route_refuses_scoped_limit() {
    let error = projected_curve_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn projected_curve_reference_route_refuses_work_limit() {
    let error = projected_curve_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}
