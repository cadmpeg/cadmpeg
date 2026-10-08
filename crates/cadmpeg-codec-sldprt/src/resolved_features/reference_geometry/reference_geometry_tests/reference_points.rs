use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn reference_point_error(policy: DecodePolicy) -> CodecError {
    let lanes = [super::reference_point_lane(243, 4, [0.125, -0.25, 0.0])];
    let mut histories = [super::reference_point_history()];
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&lanes[0].native_payload, &arena, &policy).unwrap();
    super::super::enrich_history_reference_points(&ctx, &mut histories, &lanes).unwrap_err()
}

#[test]
fn reference_point_enrichment_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "collect SLDPRT reference point starts",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            Err::<(), CodecError>(reference_point_error(policy))
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "collect SLDPRT reference point starts"));
}

#[test]
fn reference_point_enrichment_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain SLDPRT reference point position",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            Err::<(), cadmpeg_core::CodecError>(reference_point_error(policy))
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT reference point position"));
}

#[test]
fn reference_point_enrichment_refuses_work_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "scan SLDPRT reference point features",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            Err::<(), CodecError>(reference_point_error(policy))
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "scan SLDPRT reference point features"));
}
