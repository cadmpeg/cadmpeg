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

#[test]
fn coordinate_system_enrichment_refuses_path_nesting_limit() {
    let mut record = super::coordinate_system_record(
        "lane", [0.125, -0.25, 0.5], &[[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], [0, 0, 0],
    );
    let prefix = record.origin;
    record.lane.native_payload[prefix + 73..prefix + 80].fill(0xff);
    record.lane.native_payload[prefix + 80..prefix + 84].copy_from_slice(&1u32.to_le_bytes());
    record.lane.native_payload[prefix + 84..prefix + 92].fill(0);
    record.lane.native_payload[prefix + 85] = 2;
    let marker = prefix + 92;
    record.lane.native_payload[marker..marker + 16].copy_from_slice(
        &crate::resolved_features::selections::COMPACT_EDGE_VECTOR_MARKER,
    );
    record.lane.native_payload[marker + 16..marker + 18].fill(0);
    let entry = marker + 18;
    record.lane.native_payload[entry..entry + 4].copy_from_slice(&[1, 0x80, 0, 0]);
    record.lane.native_payload[entry + 4..entry + 16].copy_from_slice(
        &[0x38, 0x80, 0x3b, 0, 0x68, 1, 0, 0, 0xbc, 2, 0, 0],
    );
    record.lane.native_payload[entry + 16..entry + 20].copy_from_slice(&10u32.to_le_bytes());
    let lanes = [record.lane];
    let mut histories = [super::coordinate_system_history()];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&lanes[0].native_payload, &arena, &policy).unwrap();
    let error = super::super::enrich_history_coordinate_systems(&ctx, &mut histories, &lanes).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RecursionDepth
            && limit.operation == "decode SLDPRT sparse component path"));
}
