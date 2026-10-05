//! Collection operation output and refusal invariants.

#[test]
fn tolerant_rectangle_u_deduplication_preserves_corners_and_refusal() {
    use cadmpeg_ir::math::Point2;
    let points = [
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
        Point2::new(0.0, 1.0),
        Point2::new(1.0, 1.0),
    ];
    let solve = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::super::ordered_tolerant_rectangle_corners(ctx, &points)
    };
    assert_eq!(
        solve(&cadmpeg_test_support::service_decode_context()).unwrap(),
        Some([points[0], points[1], points[3], points[2]])
    );
    crate::test_support::work_refusal_at(
        "deduplicate SLDPRT tolerant rectangle u coordinates",
        solve,
    );
}

#[test]
fn tolerant_rectangle_v_deduplication_preserves_corners_and_refusal() {
    use cadmpeg_ir::math::Point2;
    let points = [
        Point2::new(0.0, 0.0),
        Point2::new(1.0, 0.0),
        Point2::new(0.0, 1.0),
        Point2::new(1.0, 1.0),
    ];
    let solve = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::super::ordered_tolerant_rectangle_corners(ctx, &points)
    };
    assert_eq!(
        solve(&cadmpeg_test_support::service_decode_context()).unwrap(),
        Some([points[0], points[1], points[3], points[2]])
    );
    crate::test_support::work_refusal_at(
        "deduplicate SLDPRT tolerant rectangle v coordinates",
        solve,
    );
}
