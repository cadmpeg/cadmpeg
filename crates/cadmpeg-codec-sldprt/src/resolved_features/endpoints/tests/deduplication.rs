//! Point candidate deduplication output and refusal.

#[test]
fn inferred_point_candidate_deduplication_preserves_empty_constraints_and_refusal() {
    use crate::records::{FeatureInputLane, SketchInputEntity, SketchInputKind};
    let markers = [0, 1].map(|ordinal| {
        let mut marker = SketchInputEntity::new(
            format!("point-{ordinal}"),
            "lane",
            ordinal,
            u64::from(ordinal),
            SketchInputKind::Point,
        );
        marker.feature_ref = Some("feature".into());
        marker.coordinates_m = cadmpeg_ir::units::FiniteVector::new([0.0, 0.0]);
        marker
    });
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: markers.into(),
    };
    let solve = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        crate::resolved_features::endpoints::inferred_point_coordinates_by_index(
            ctx, &lane, "feature",
        )
    };
    assert!(solve(&cadmpeg_test_support::service_decode_context())
        .unwrap()
        .is_empty());
    crate::test_support::work_refusal_at("deduplicate SLDPRT inferred point coordinates", solve);
}

#[test]
fn empty_solver_graphs_release_storage_without_retaining_output() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use crate::records::FeatureInputLane;
    let lane = FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: Vec::new(),
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        assert!(crate::resolved_features::endpoints::inferred_point_coordinates_by_index(
            &ctx, &lane, "feature",
        ).unwrap().is_empty());
    }
}
