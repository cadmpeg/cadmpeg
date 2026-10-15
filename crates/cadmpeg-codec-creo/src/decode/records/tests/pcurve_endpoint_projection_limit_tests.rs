// SPDX-License-Identifier: Apache-2.0
use crate::decode::records::pcurve_endpoint_records;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

#[test]
fn positional_pcurve_endpoint_refuses_collection_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.curves.pcurves.push(crate::curve::PcurveEndpoints {
        curve_id: 7,
        faces: [None; 2],
        face_0_endpoints: [[0.0; 2]; 2],
        face_1_endpoints: [[0.0; 2]; 2],
        offset: 4,
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo native pcurve endpoint records"),
        |cap| {
            let trial_arena = DecodeArena::new();
            let mut trial_policy = DecodePolicy::service();
            trial_policy.limits.max_collection_items = cap;
            let (trial_ctx, _) =
                DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
            pcurve_endpoint_records(&trial_ctx, &scan).map(|_| ())
        },
    );

    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let Err(error) = pcurve_endpoint_records(&ctx, &scan) else {
        panic!("the endpoint record exceeds the collection limit")
    };
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native pcurve endpoint records"),
        "{error:?}"
    );
}

#[test]
fn prototype_pcurve_endpoint_refuses_collection_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.curves
        .bound_prototype_pcurves
        .push(crate::curve::BoundPrototypePcurve {
            curve_id: 8,
            faces: [None; 2],
            face_0_endpoints: [[0.0; 2]; 2],
            face_1_endpoints: [[0.0; 2]; 2],
            offset: 3,
        });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo native pcurve endpoint records"),
        |cap| {
            let trial_arena = DecodeArena::new();
            let mut trial_policy = DecodePolicy::service();
            trial_policy.limits.max_collection_items = cap;
            let (trial_ctx, _) =
                DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
            pcurve_endpoint_records(&trial_ctx, &scan).map(|_| ())
        },
    );

    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let Err(error) = pcurve_endpoint_records(&ctx, &scan) else {
        panic!("the prototype endpoint record exceeds the collection limit")
    };
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native pcurve endpoint records"),
        "{error:?}"
    );
}
