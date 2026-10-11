//! Charged native feature-object range tests.

use super::{lane_with_position_reference, native_history};
use crate::resolved_features::holes::feature_object_byte_ranges;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

#[test]
fn feature_object_ranges_preserve_named_object_bounds() {
    let histories = [native_history()];
    let lane = lane_with_position_reference(7);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&lane.native_payload, &arena, &DecodePolicy::service())
            .expect("test context");
    let ranges =
        feature_object_byte_ranges(&ctx, &histories, &lane).expect("charged feature ranges");
    assert_eq!(
        ranges.get("native-hole"),
        Some(&(0, 0, lane.native_payload.len()))
    );
}

#[test]
fn feature_object_ranges_refuse_collection_limit() {
    let histories = [native_history()];
    let lane = lane_with_position_reference(7);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "index SLDPRT feature object byte ranges",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&lane.native_payload, &arena, &policy)
                .expect("test context");
            feature_object_byte_ranges(&ctx, &histories, &lane)
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "index SLDPRT feature object byte ranges")
    );
}

#[test]
fn feature_object_ranges_refuse_work_limit() {
    let histories = [native_history()];
    let lane = lane_with_position_reference(7);
    let error =
        crate::test_support::work_refusal_at("index SLDPRT feature object byte ranges", |ctx| {
            feature_object_byte_ranges(ctx, &histories, &lane)
        });
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "index SLDPRT feature object byte ranges")
    );
}

#[test]
fn feature_object_ranges_preserve_equal_offset_history_order() {
    let mut history = native_history();
    let mut second = history.features[0].clone();
    second.id = "second-hole".into();
    history.features.push(second);
    let lane = lane_with_position_reference(7);
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&lane.native_payload, &arena, &DecodePolicy::service())
            .unwrap();
    let histories = [history];
    let ranges = feature_object_byte_ranges(&ctx, &histories, &lane).unwrap();
    assert_eq!(ranges.get("native-hole"), Some(&(0, 0, 0)));
    assert_eq!(
        ranges.get("second-hole"),
        Some(&(0, 0, lane.native_payload.len()))
    );
}
