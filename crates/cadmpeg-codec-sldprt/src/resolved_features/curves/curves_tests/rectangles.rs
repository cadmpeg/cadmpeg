//! Rectangle coordinate selection and resource refusal tests.

use super::rectangle_limit_markers;
use super::super::{ordered_rectangle_corners, unique_dimensioned_rectangle_markers};
use crate::records::{SketchInputEntity, SketchInputKind};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point2;

#[test]
fn dimensioned_rectangle_refuses_collection_limit() {
    let markers = rectangle_limit_markers();
    let marker_refs = markers.iter().collect::<Vec<_>>();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 3;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = unique_dimensioned_rectangle_markers(&ctx, &marker_refs, &[8.5, 5.5]).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "collect SLDPRT rectangle points"));
}

#[test]
fn dimensioned_rectangle_refuses_work_limit() {
    let markers = rectangle_limit_markers();
    let marker_refs = markers.iter().collect::<Vec<_>>();
    let mut policy = DecodePolicy::service();
    // Four i64 cells, eight bytes each, three bit-length levels plus one, eight work units per byte.
    policy.limits.max_work_units = 4 + 4 * 8 * 4 * 8 - 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = unique_dimensioned_rectangle_markers(&ctx, &marker_refs, &[8.5, 5.5]).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "sldprt rectangle cells u sort"));
}

#[test]
fn compact_rectangle_requires_each_axis_corner_exactly_once() {
    let corners = [
        Point2::new(25.75, 14.15),
        Point2::new(-25.75, -14.15),
        Point2::new(-25.75, 14.15),
        Point2::new(25.75, -14.15),
    ];
    assert_eq!(
        ordered_rectangle_corners(&cadmpeg_test_support::service_decode_context(), &corners)
            .unwrap(),
        Some([
            Point2::new(-25.75, -14.15),
            Point2::new(25.75, -14.15),
            Point2::new(25.75, 14.15),
            Point2::new(-25.75, 14.15),
        ])
    );

    crate::test_support::work_refusal_at("deduplicate SLDPRT rectangle u coordinates", |ctx| {
        ordered_rectangle_corners(ctx, &corners)
    });
    crate::test_support::work_refusal_at("deduplicate SLDPRT rectangle v coordinates", |ctx| {
        ordered_rectangle_corners(ctx, &corners)
    });
    let duplicate = [corners[0], corners[0], corners[2], corners[3]];
    assert_eq!(
        ordered_rectangle_corners(&cadmpeg_test_support::service_decode_context(), &duplicate)
            .unwrap(),
        None
    );
    let non_rectangular = [
        corners[0],
        corners[1],
        corners[2],
        Point2::new(24.0, -14.15),
    ];
    assert_eq!(
        ordered_rectangle_corners(
            &cadmpeg_test_support::service_decode_context(),
            &non_rectangular
        )
        .unwrap(),
        None
    );
}

#[test]
fn dimensioned_rectangle_selects_one_complete_marker_product() {
    let marker = |id: &str, u, v| {
        let marker_id: String = id.into();
        let marker_parent: String = "lane".into();
        let mut constructed_marker =
            SketchInputEntity::new(marker_id, marker_parent, 0, 0, SketchInputKind::Point);
        constructed_marker.feature_ref = Some("feature".into());
        constructed_marker.state_value = None;
        constructed_marker.coordinates_m = cadmpeg_ir::units::FiniteVector::new([u, v]);
        constructed_marker.links = None;
        constructed_marker
    };
    let markers = [
        marker("center", -0.023, 0.0),
        marker("lower-left", -0.02575, -0.00425),
        marker("upper-right", -0.02025, 0.00425),
        marker("lower-right", -0.02025, -0.00425),
        marker("upper-left", -0.02575, 0.00425),
        marker("axis-top", -0.02575, 0.01415),
        marker("axis-bottom", -0.02575, -0.01415),
        marker("origin", 0.0, 0.0),
    ];
    let marker_refs = markers.iter().collect::<Vec<_>>();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    for operation in ["deduplicate SLDPRT dimensioned rectangle u coordinates",
        "deduplicate SLDPRT rectangle v coordinates"] {
        crate::test_support::work_refusal_at(operation, |ctx| {
            unique_dimensioned_rectangle_markers(ctx, &marker_refs, &[8.5, 5.5])
        });
    }
    assert_eq!(
        unique_dimensioned_rectangle_markers(&ctx, &marker_refs, &[8.5, 5.5])
            .unwrap()
            .map(|markers| markers.map(crate::records::SketchInputEntity::id)),
        Some(["lower-left", "lower-right", "upper-right", "upper-left"])
    );
    assert_eq!(
        unique_dimensioned_rectangle_markers(&ctx, &marker_refs, &[8.5]).unwrap(),
        None
    );
    assert_eq!(
        unique_dimensioned_rectangle_markers(&ctx, &marker_refs, &[28.3, 5.5]).unwrap(),
        None
    );

    let second_rectangle = [
        marker("second-lower-left", 0.010, 0.020),
        marker("second-lower-right", 0.0155, 0.020),
        marker("second-upper-right", 0.0155, 0.0285),
        marker("second-upper-left", 0.010, 0.0285),
    ];
    let ambiguous = marker_refs
        .iter()
        .copied()
        .chain(second_rectangle.iter())
        .collect::<Vec<_>>();
    assert_eq!(
        unique_dimensioned_rectangle_markers(&ctx, &ambiguous, &[8.5, 5.5]).unwrap(),
        None
    );
}

