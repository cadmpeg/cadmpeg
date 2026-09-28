// SPDX-License-Identifier: Apache-2.0

use crate::native::features::feature_projected_curve_references;

fn projected_curve_container() -> crate::container::Container<'static> {
    let payload = b"\0\x01\x02\xf1\x02\xc8\xf1\x02\xc9\x80\x57\x00\x02\x01\xf1\x02\xca\xff\x01\x02\x02\x7d\0".to_vec();
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], "CPROJ", payload)], &[],
    );
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    crate::test_support::with_decode_context(move |ctx| crate::container::scan_bytes(ctx, file))
        .expect("synthetic projected curve container")
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
