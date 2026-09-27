#[test]
fn standard_topology_entity_limit_propagates_before_fallback() {
    let bytes = crate::test_support::test_container::tetrahedron_topology_catpart();
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, bytes.clone())
    })
    .expect("service resource budget");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    // One retained carrier, four surfaces, four points, four vertices,
    // four faces, and one body, region, and shell precede the first edge.
    policy.limits.max_entities = 20;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("topology fixture fits the input-byte limit");
    let result = crate::families::standard::decode::try_decode_standard(
        &ctx,
        &scan,
        &mut crate::nurbs::LaneRefusals::new(),
    );
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = result else {
        panic!("first topology entity must exceed the entity limit");
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::Entities
    );
    assert_eq!(limit.used, 20);
    assert_eq!(limit.operation, "admit CATIA family model entity");
}

#[test]
fn standard_topology_charges_deferred_and_ordered_endpoint_arrays() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let bytes = crate::test_support::test_container::tetrahedron_topology_catpart();
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, bytes.clone())
    })
    .expect("service resource budget");
    let mut operations = BTreeSet::new();
    let mut limit = 0;
    let mut completed = false;
    for _ in 0..4096 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("topology fixture fits the input-byte limit");
        match crate::families::standard::decode::try_decode_standard(
            &ctx,
            &scan,
            &mut crate::nurbs::LaneRefusals::new(),
        ) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                operations.insert(error.operation);
                let next = error.used + error.additional;
                assert!(next > limit, "a refusal must advance the collection cap");
                limit = next;
                if operations.contains("catia_deferred_port_edges")
                    && operations.contains("catia_ordered_endpoint_pairs")
                {
                    limit = DecodePolicy::service().limits.max_collection_items;
                }
            }
            Ok(Some(_)) => {
                completed = true;
                break;
            }
            Ok(None) => panic!("tetrahedron fixture must decode as standard topology"),
            Err(error) => panic!("unexpected topology refusal: {error}"),
        }
    }
    assert!(
        completed,
        "adaptive caps must admit the tetrahedron fixture"
    );
    assert!(operations.contains("catia_deferred_port_edges"));
    assert!(operations.contains("catia_ordered_endpoint_pairs"));
}

#[test]
fn standard_topology_candidate_maps_refuse_before_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let bytes = crate::test_support::test_container::tetrahedron_topology_catpart();
    let scan = crate::test_support::with_service_context(|ctx| crate::container::scan_bytes(ctx, bytes.clone()))
        .expect("service resource budget");
    let expected = [
        "catia_standard_surface_indices",
        "catia_standard_endpoint_candidate_rows",
        "catia_face_incidence_points",
        "catia_face_incidence_rows",
        "catia_incidence_right_points",
        "catia_incidence_shared_points",
        "catia_incidence_candidate_rows",
        "catia_incidence_candidate_copy",
        "catia_native_port_options",
    ];
    let mut operations = BTreeSet::new();
    let mut limit = 0;
    let mut completed = false;
    for _ in 0..4096 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("topology fixture fits input limit");
        match crate::families::standard::decode::try_decode_standard(&ctx, &scan, &mut crate::nurbs::LaneRefusals::new()) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                operations.insert(error.operation);
                let next = error.used + error.additional;
                assert!(next > limit, "a refusal advances the collection cap");
                limit = next;
                if expected.iter().all(|operation| operations.contains(operation)) {
                    limit = DecodePolicy::service().limits.max_collection_items;
                }
            }
            Ok(Some(_)) => { completed = true; break; }
            Ok(None) => panic!("tetrahedron fixture must decode"),
            Err(error) => panic!("unexpected topology refusal: {error}"),
        }
    }
    assert!(completed, "adaptive caps must admit the fixture");
    for operation in expected {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn standard_topology_serialized_faces_refuse_before_collection_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let bytes = crate::test_support::test_container::tetrahedron_topology_catpart();
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, bytes.clone())
    })
    .expect("service budget admits the topology input");
    let mut limit = 0;
    let mut found = false;
    for _ in 0..4096 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("topology input fits the byte limit");
        match crate::families::standard::decode::try_decode_standard(
            &ctx,
            &scan,
            &mut crate::nurbs::LaneRefusals::new(),
        ) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                if error.operation == "catia_standard_serialized_edge_faces" {
                    found = true;
                    limit = DecodePolicy::service().limits.max_collection_items;
                } else {
                    limit = error.used + error.additional;
                }
            }
            Ok(Some(_)) => break,
            Ok(None) => panic!("tetrahedron input must decode"),
            Err(error) => panic!("unexpected topology refusal: {error}"),
        }
    }
    assert!(found, "serialized edge faces must be charged before allocation");
}
