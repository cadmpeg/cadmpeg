//! Scoped coordinate collection output and refusal invariants.

#[test]
fn rectangle_coordinate_collections_preserve_output_and_refusals() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::math::Point2;
    let points = [Point2::new(1.0, 1.0), Point2::new(0.0, 0.0),
        Point2::new(0.0, 1.0), Point2::new(1.0, 0.0)];
    let expected = Some([Point2::new(0.0, 0.0), Point2::new(1.0, 0.0),
        Point2::new(1.0, 1.0), Point2::new(0.0, 1.0)]);
    for (tolerant, operations) in [
        (false, ["collect SLDPRT rectangle u coordinates", "collect SLDPRT rectangle v coordinates"]),
        (true, ["collect SLDPRT tolerant rectangle u coordinates", "collect SLDPRT tolerant rectangle v coordinates"]),
    ] {
        let solve = |ctx: &DecodeContext<'_>| {
            if tolerant { super::super::ordered_tolerant_rectangle_corners(ctx, &points) }
            else { super::super::ordered_rectangle_corners(ctx, &points) }
        };
        assert_eq!(solve(&cadmpeg_test_support::service_decode_context()).unwrap(), expected);
        for operation in operations {
            for dimension in [ResourceDimension::WorkUnits, ResourceDimension::CollectionItems,
                ResourceDimension::MaterializedBytes] {
                cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::service();
                    match dimension {
                        ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                        ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                        ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                        _ => panic!("test dimension"),
                    }
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                    let result = solve(&ctx);
                    if let Err(CodecError::ResourceLimit(limit)) = &result {
                        assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
                    }
                    result
                });
            }
        }
    }
}

#[test]
fn connected_arc_endpoint_deduplication_preserves_native_geometry_and_refusal() {
    const EPS_CONNECTED_ARC: f64 = 1.0e-9;
    let mut entities = super::connected_arc_limit_entities();
    entities.push(cadmpeg_ir::sketches::SketchEntity::new(
        cadmpeg_ir::sketches::SketchEntityId::mint("synthetic:test:id#duplicate-endpoint-arc").unwrap(),
        entities[1].sketch.clone(), entities[1].geometry.clone(),
    ).with_native_ref(Some("duplicate-arc".into()))
        .with_endpoint_refs(vec!["p".into(), "p".into()]));
    let solve = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        let mut candidates = entities.clone();
        super::super::resolve_connected_marker_arcs(ctx, &mut candidates, EPS_CONNECTED_ARC)
            .map(|()| candidates)
    };
    assert_eq!(solve(&cadmpeg_test_support::service_decode_context()).unwrap(), entities);
    crate::test_support::work_refusal_at("deduplicate SLDPRT connected arc endpoints", solve);
}
