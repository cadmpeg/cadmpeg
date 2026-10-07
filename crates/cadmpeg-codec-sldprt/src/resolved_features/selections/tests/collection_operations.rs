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

#[test]
fn invalid_component_prefix_does_not_visit_the_remaining_count() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    const OPERATION: &str = "decode SLDPRT mixed component path";
    let payload = [0; 2000];
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits, OPERATION, |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            super::super::compact_mixed_component_path(&ctx, &payload, 0, 100, false, OPERATION)
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else { panic!("work refusal"); };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = limit.used + limit.additional;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(super::super::compact_mixed_component_path(&ctx, &payload, 0, 100, false, OPERATION).unwrap(), None);
}

#[test]
fn rejected_component_storage_is_released_between_candidates() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    const OPERATION: &str = "decode SLDPRT mixed component path";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 8;
    policy.limits.max_materialized_bytes = 16384;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        assert_eq!(super::super::compact_mixed_component_path(&ctx, &[0; 2000], 0, 100, false, OPERATION).unwrap(), None);
    }
}
