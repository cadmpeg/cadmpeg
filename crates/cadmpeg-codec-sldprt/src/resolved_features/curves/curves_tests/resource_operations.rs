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

#[test]
fn a_lane_without_markers_does_not_build_slot_references() {
    let payload = vec![0; 4096];
    let ctx = cadmpeg_test_support::service_decode_context();
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "index SLDPRT slot predecessors",
        None,
    );
    let slots = super::super::SlotReferences::new(&ctx, &payload).unwrap();
    assert!(slots.records.get().is_none());
    assert!(crate::resolved_features::markers::admit_sketch_input_entities(
        &ctx, &payload, "lane"
    ).unwrap().is_empty());
}

#[test]
fn slot_references_scan_only_on_the_first_query() {
    let payload = vec![0; 4096];
    crate::test_support::work_refusal_at("index SLDPRT slot predecessors", |ctx| {
        let slots = super::super::SlotReferences::new(ctx, &payload)?;
        super::super::slot_curve_and_center_indices(ctx, &slots, 0)
    });
    let ctx = cadmpeg_test_support::service_decode_context();
    let slots = super::super::SlotReferences::new(&ctx, &payload).unwrap();
    assert_eq!(super::super::slot_curve_and_center_indices(&ctx, &slots, 0).unwrap(), None);
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "index SLDPRT slot predecessors",
        None,
    );
    assert_eq!(super::super::slot_curve_and_center_indices(&ctx, &slots, 1).unwrap(), None);
}
