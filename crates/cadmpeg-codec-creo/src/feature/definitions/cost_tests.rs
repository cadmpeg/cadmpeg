use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::{FeatureSegment, FeatureSegmentKind};

#[test]
fn feature_segment_cost_counts_variant_options_and_owned_bytes() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("root context");
    let segment = FeatureSegment {
        kind: FeatureSegmentKind::Arc([7, 9]),
        directions: [Some(2), None, Some(4)],
        center_id: None,
        arc_orientation: Some(1),
        vertical_horizontal: None,
        radius_ref: Some(8),
        radius2_ref: None,
        external_id: 12,
        body: vec![0, 1, 2, 3],
        offset: 6,
    };

    assert_eq!(
        DecodeCost::decode_cost(&FeatureSegmentKind::Line([7, 9]), &ctx, "feature segment kind")
            .expect("line kind cost"),
        9
    );
    assert_eq!(
        DecodeCost::decode_cost(&FeatureSegmentKind::Arc([7, 9]), &ctx, "feature segment kind")
            .expect("arc kind cost"),
        9
    );
    assert_eq!(
        DecodeCost::decode_cost(&FeatureSegmentKind::Point(7), &ctx, "feature segment kind")
            .expect("point kind cost"),
        5
    );
    assert_eq!(
        DecodeCost::decode_cost(&segment, &ctx, "feature segment field cost")
            .expect("segment field cost"),
        41 + cadmpeg_core::decode::u64_from_index(std::mem::size_of::<usize>())
    );
}

#[test]
fn feature_segment_equality_refuses_before_comparing_option_fields() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root context");
    let segment = FeatureSegment {
        kind: FeatureSegmentKind::Line([1, 2]),
        directions: [Some(1), None, Some(2)],
        center_id: None,
        arc_orientation: None,
        vertical_horizontal: None,
        radius_ref: None,
        radius2_ref: None,
        external_id: 4,
        body: Vec::new(),
        offset: 0,
    };

    let error = ctx
        .equal(&segment, &segment, "creo feature segment equality")
        .expect_err("the option-field scan exceeds the work budget");
    let CodecError::ResourceLimit(resource) = error else {
        panic!("expected a work refusal");
    };
    assert_eq!(resource.dimension, ResourceDimension::WorkUnits);
    assert_eq!(resource.operation, "creo feature segment equality");
}
