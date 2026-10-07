//! Candidate collection order and admission invariants.

#[test]
fn surface_candidate_ordering_propagates_work_refusal() {
    use crate::records::FeatureInputComponentPathEntry;
    let component = FeatureInputComponentPathEntry {
        instance: None,
        type_signature: [0; 12],
        local_id: Some(7),
    };
    let input = vec![
        (2, vec![component.clone()]),
        (1, vec![component.clone()]),
        (1, vec![component]),
    ];
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut candidates = input.clone();
    super::super::order_surface_candidates(
        &ctx,
        &mut candidates,
        "order synthetic surface candidates",
    )
    .unwrap();
    assert_eq!(candidates, vec![input[1].clone(), input[0].clone()]);
    let mut empty = Vec::new();
    super::super::order_surface_candidates(&ctx, &mut empty, "order empty surface candidates")
        .unwrap();
    assert!(empty.is_empty());
    crate::test_support::work_refusal_at("order synthetic surface candidates", |ctx| {
        let mut candidates = input.clone();
        super::super::order_surface_candidates(
            ctx,
            &mut candidates,
            "order synthetic surface candidates",
        )
    });
}

#[test]
fn cylinder_marker_reference_cost_counts_offset_tag_and_components() {
    use crate::records::FeatureInputComponentPathEntry;
    use crate::resolved_features::selections::CylinderMarkerReference;
    use cadmpeg_core::decode::cost::DecodeCost;
    let marker_bytes = cadmpeg_core::decode::u64_from_index(std::mem::size_of::<usize>());
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        CylinderMarkerReference(7, None)
            .decode_cost(&ctx, "SLDPRT marker reference cost")
            .unwrap(),
        marker_bytes + 1
    );
    let reference = CylinderMarkerReference(
        7,
        Some(vec![
            FeatureInputComponentPathEntry {
                instance: None,
                type_signature: [0; 12],
                local_id: None,
            },
            FeatureInputComponentPathEntry {
                instance: Some(2),
                type_signature: [1; 12],
                local_id: Some(7),
            },
        ]),
    );
    // Offset width, option tag, and components with fourteen and twenty bytes.
    assert_eq!(
        reference
            .decode_cost(&ctx, "SLDPRT marker reference cost")
            .unwrap(),
        marker_bytes + 35
    );
    crate::test_support::work_refusal_at("SLDPRT marker reference cost", |ctx| {
        reference.decode_cost(ctx, "SLDPRT marker reference cost")
    });
}
