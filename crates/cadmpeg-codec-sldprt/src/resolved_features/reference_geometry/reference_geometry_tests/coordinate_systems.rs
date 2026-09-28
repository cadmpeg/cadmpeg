use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn coordinate_system_error(policy: DecodePolicy) -> CodecError {
    let record = super::coordinate_system_record(
        "lane",
        [0.125, -0.25, 0.5],
        &[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        [0, 0, 0],
    );
    let lanes = [record.lane];
    let mut histories = [super::coordinate_system_history()];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&lanes[0].native_payload, &arena, &policy)
        .unwrap();
    super::super::enrich_history_coordinate_systems(&ctx, &mut histories, &lanes).unwrap_err()
}

#[test]
fn coordinate_system_enrichment_refuses_collection_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error = coordinate_system_error(policy);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "collect SLDPRT coordinate system starts"));
}

#[test]
fn coordinate_system_enrichment_refuses_retained_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let error = coordinate_system_error(policy);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "retain SLDPRT coordinate system origin"));
}

#[test]
fn coordinate_system_enrichment_refuses_work_limit() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let error = coordinate_system_error(policy);
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "scan SLDPRT coordinate system features"));
}
