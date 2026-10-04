//! Candidate collection order and admission invariants.

#[test]
fn surface_candidate_drain_propagates_scoped_refusal() {
    use crate::records::FeatureInputComponentPathEntry;
    let component = FeatureInputComponentPathEntry {
        instance: None,
        type_signature: [0; 12],
        local_id: Some(7),
    };
    let input = vec![(2, vec![component.clone()]), (1, vec![component.clone()]), (1, vec![component])];
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut candidates = input.clone();
    super::super::order_surface_candidates(&ctx, &mut candidates, "order synthetic surface candidates").unwrap();
    assert_eq!(candidates, vec![input[1].clone(), input[0].clone()]);
    let mut empty = Vec::new();
    super::super::order_surface_candidates(&ctx, &mut empty, "order empty surface candidates").unwrap();
    assert!(empty.is_empty());
    for dimension in [
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
    ] {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        dimension,
        "drain SLDPRT surface selection candidates",
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            match dimension {
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                cadmpeg_core::decode::ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                _ => {
                    assert_eq!(dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
                    policy.limits.max_work_units = cap;
                }
            }
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut candidates = input.clone();
            let result = super::super::order_surface_candidates(&ctx, &mut candidates, "order synthetic surface candidates");
            if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
            }
            result
        },
    );
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == dimension
            && limit.operation == "drain SLDPRT surface selection candidates"));
    }
}
