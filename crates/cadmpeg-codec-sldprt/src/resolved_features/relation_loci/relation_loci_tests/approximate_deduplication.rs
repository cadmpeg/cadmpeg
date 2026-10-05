//! Coincident canonical profile point selection and refusal.

#[test]
fn canonical_profile_point_deduplication_preserves_identity_order_and_refusal() {
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::{
        SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
        SketchLocus,
    };
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let input = ["z", "a"].map(|suffix| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#{suffix}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(0.0, 0.0),
            })
            .unwrap(),
        )
    });
    let solve = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::super::canonical_profile_loci(ctx, &sketch, &input)
    };
    assert_eq!(
        solve(&cadmpeg_test_support::service_decode_context()).unwrap(),
        vec![(
            Point2::new(0.0, 0.0),
            SketchLocus::Entity(input[1].id().clone())
        )]
    );
    crate::test_support::work_refusal_at("deduplicate SLDPRT canonical profile loci", solve);
}
