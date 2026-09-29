//! Charged native feature-object range tests.

use super::{lane_with_position_reference, native_history};
use crate::resolved_features::holes::feature_object_byte_ranges;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

#[test]
fn feature_object_ranges_preserve_named_object_bounds() {
    let histories = [native_history()];
    let lane = lane_with_position_reference(7);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(
        &lane.native_payload,
        &arena,
        &DecodePolicy::service(),
    )
    .expect("test context");
    let ranges = feature_object_byte_ranges(&ctx, &histories, &lane)
        .expect("charged feature ranges");
    assert_eq!(ranges.get("native-hole"), Some(&(0, 0, lane.native_payload.len())));
}

#[test]
fn feature_object_ranges_refuse_collection_limit() {
    let histories = [native_history()];
    let lane = lane_with_position_reference(7);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&lane.native_payload, &arena, &policy)
        .expect("test context");
    let error = feature_object_byte_ranges(&ctx, &histories, &lane)
        .expect_err("feature range index exceeds collection limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index SLDPRT feature object byte ranges"));
}

#[test]
fn feature_object_ranges_refuse_work_limit() {
    let histories = [native_history()];
    let lane = lane_with_position_reference(7);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&lane.native_payload, &arena, &policy)
        .expect("test context");
    let error = feature_object_byte_ranges(&ctx, &histories, &lane)
        .expect_err("feature range scan exceeds work limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "index SLDPRT feature object byte ranges"));
}
