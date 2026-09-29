// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn segment_om_links_refusal(configure: impl FnOnce(&mut DecodePolicy)) -> cadmpeg_core::CodecError {
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        crate::test_support::test_om::segment_om_payload(false),
    )]);
    let container =
        crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, file))
            .expect("valid segment OM container");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
    super::super::segment_om_links(&ctx, &container).unwrap_err()
}

#[test]
fn segment_om_links_refuse_collection_limit() {
    let error = segment_om_links_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems)
    );
}

#[test]
fn segment_om_links_refuse_retained_limit() {
    let error = segment_om_links_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes)
    );
}

#[test]
fn segment_om_links_refuse_work_limit() {
    let error = segment_om_links_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits)
    );
}
