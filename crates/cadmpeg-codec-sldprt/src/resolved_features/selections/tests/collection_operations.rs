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
        ResourceDimension::WorkUnits,
        OPERATION,
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            super::super::compact_mixed_component_path(&ctx, &payload, 0, 100, false, OPERATION)
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("work refusal");
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = limit.used + limit.additional;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        super::super::compact_mixed_component_path(&ctx, &payload, 0, 100, false, OPERATION)
            .unwrap(),
        None
    );
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
        assert_eq!(
            super::super::compact_mixed_component_path(&ctx, &[0; 2000], 0, 100, false, OPERATION)
                .unwrap(),
            None
        );
    }
}

#[test]
fn ambiguous_edge_path_storage_is_released_between_projections() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let marker = 12;
    let mut payload = vec![0; 120];
    payload[..4].copy_from_slice(&2u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[marker..marker + 16].copy_from_slice(&super::super::COMPACT_EDGE_VECTOR_MARKER);
    let signature = [0x2a, 0x81, 0x2c, 1, 28, 0, 0, 0, 0x24, 1, 0xd3, 0x48];
    let first = marker + 18;
    payload[first..first + 2].copy_from_slice(&0x8130u16.to_le_bytes());
    payload[first + 4..first + 16].copy_from_slice(&signature);
    let second = first + 24;
    payload[second..second + 2].copy_from_slice(&0x8141u16.to_le_bytes());
    payload[second + 4..second + 16].copy_from_slice(&signature);
    payload[second + 20..second + 24].copy_from_slice(&5u32.to_le_bytes());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        assert_eq!(
            super::super::compact_edge_component_path_at(&ctx, &payload, marker).unwrap(),
            None
        );
    }
}

#[test]
fn rejected_sketch_surface_trailer_releases_component_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let marker = 12;
    let mut payload = Vec::new();
    payload.extend(5u32.to_le_bytes());
    payload.extend([0, 2, 0, 0]);
    payload.extend(7u32.to_le_bytes());
    payload.extend(super::super::COMPACT_EDGE_VECTOR_MARKER);
    payload.extend([0; 2]);
    for (instance, local_id) in [(0x8032u16, 2u32), (0x8033, 1), (0x8034, 0)] {
        payload.extend(instance.to_le_bytes());
        payload.extend([0; 2]);
        payload.extend([1; 12]);
        payload.extend(local_id.to_le_bytes());
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        assert_eq!(
            super::super::compact_sketch_surface_component_path_at(&ctx, &payload, marker).unwrap(),
            None
        );
    }
}

#[test]
fn short_edge_ids_admit_each_output_item_once() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut payload = vec![1, 0, 2, 0];
    payload.extend([0; 16]);
    payload.extend([0xff, 0xfe, 0xff]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        super::super::compact_u16_edge_ids(&ctx, &payload, 0, 2).unwrap(),
        Some(vec![1, 2])
    );
}

#[test]
fn curve_path_admits_each_output_item_once() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut payload = vec![0; 70];
    payload[..4].copy_from_slice(&2u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[4, 2, 0, 0]);
    payload[12..28].copy_from_slice(&super::super::COMPACT_EDGE_VECTOR_MARKER);
    let signature = [0x38, 0x80, 0x3b, 0, 20, 0, 0, 0, 100, 0, 0, 0];
    for (cursor, local_id) in [(30, 7u32), (50, 8)] {
        payload[cursor..cursor + 2].copy_from_slice(&0x8130u16.to_le_bytes());
        payload[cursor + 4..cursor + 16].copy_from_slice(&signature);
        payload[cursor + 16..cursor + 20].copy_from_slice(&local_id.to_le_bytes());
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let components = super::super::component_reference_curve_path_at(&ctx, &payload, 12)
        .unwrap().expect("two curve components");
    assert_eq!(components, vec![
        crate::records::FeatureInputComponentPathEntry {
            instance: Some(0x8130), type_signature: signature, local_id: Some(7),
        },
        crate::records::FeatureInputComponentPathEntry {
            instance: Some(0x8130), type_signature: signature, local_id: Some(8),
        },
    ]);
}

#[test]
fn surface_metadata_is_built_only_by_its_consuming_operation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let lane = crate::records::FeatureInputLane {
        id: "lane".into(), configuration: None, native_payload: vec![0; 128],
        classes: ["moCompSurfaceBody_c", "moPLineProjIdRep_c", "moPLineSurfIdRep_c"]
            .into_iter().enumerate().map(|(ordinal, name)| crate::records::FeatureInputClass {
                id: name.into(), parent: "lane".into(), ordinal: u32::try_from(ordinal).unwrap(),
                offset: 0, name: name.into(),
            }).collect(),
        names: Vec::new(), scalars: Vec::new(), relation_bindings: Vec::new(),
        relation_instances: Vec::new(), body_selections: Vec::new(), edge_selections: Vec::new(),
        surface_selections: Vec::new(), generated_surface_identities: Vec::new(),
        references: Vec::new(), sketch_entities: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    {
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits, "index SLDPRT operation surface classes", None,
        );
        assert!(super::super::compact_surface_selections(&ctx, &[], &[], &lane, &[])
            .unwrap().is_empty());
        let classes = super::super::OperationSurfaceClasses::new(&ctx, &lane, &[]).unwrap();
        assert!(classes.surfaces.get().is_none());
        assert!(classes.split_classes.get().is_none());
        assert!(super::super::operation_surface_selection_candidates(
            &ctx, crate::classification::FeatureClass::CutWithSurface, &lane,
            &classes, 0, 0, None,
        ).unwrap().is_empty());
        assert!(classes.surfaces.get().is_none());
        assert!(classes.split_classes.get().is_none());
    }
    let classes = super::super::OperationSurfaceClasses::new(&ctx, &lane, &[]).unwrap();
    assert!(classes.has_split_classes(&lane).unwrap());
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits, "index SLDPRT operation surface classes", None,
    );
    assert!(classes.has_split_classes(&lane).unwrap());
    assert!(classes.surfaces.get().is_none());
}

#[test]
fn split_object_without_source_does_not_search_split_classes() {
    let lane = crate::records::FeatureInputLane {
        id: "lane".into(), configuration: None, native_payload: Vec::new(), classes: Vec::new(),
        names: Vec::new(), scalars: Vec::new(), relation_bindings: Vec::new(),
        relation_instances: Vec::new(), body_selections: Vec::new(), edge_selections: Vec::new(),
        surface_selections: Vec::new(), generated_surface_identities: Vec::new(),
        references: Vec::new(), sketch_entities: Vec::new(),
    };
    let ctx = cadmpeg_test_support::service_decode_context();
    let classes = super::super::OperationSurfaceClasses::new(&ctx, &lane, &[]).unwrap();
    assert!(super::super::operation_surface_selection_candidates(
        &ctx, crate::classification::FeatureClass::SplitFace, &lane, &classes, 0, 0, None,
    ).unwrap().is_empty());
    assert!(classes.split_classes.get().is_none());
}
