// SPDX-License-Identifier: Apache-2.0
//! Resource admission for zero-entity native records.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn native_zero_entity_records_refuse_collection_limit_before_materialization() {
    let bytes = [0xa9, 0x03, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0];
    let run =
        |ctx: &DecodeContext<'_>| super::super::zero_entity_records(ctx, &bytes, 0..bytes.len());

    let service_arena = DecodeArena::new();
    let service_policy = DecodePolicy::service();
    let (service_ctx, _) = DecodeContext::from_root_bytes(&bytes, &service_arena, &service_policy)
        .expect("fixture fits the input limit");
    assert_eq!(run(&service_ctx).expect("service resource budget").len(), 1);

    let limited_arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (limited_ctx, _) = DecodeContext::from_root_bytes(&bytes, &limited_arena, &policy)
        .expect("fixture fits the input limit");
    let Err(CodecError::ResourceLimit(error)) = run(&limited_ctx) else {
        panic!("native record vector must refuse the collection limit");
    };
    assert_eq!(error.dimension, ResourceDimension::CollectionItems);
    assert_eq!(error.operation, "catia_native_zero_records");
}

#[test]
fn native_zero_entity_record_id_refuses_retained_limit() {
    let bytes = [0xa9, 0x03, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0, 0];
    let refused =
        crate::test_support::with_retained_refusal(&[], "catia_native_zero_record_id", |ctx| {
            super::super::zero_entity_records(ctx, &bytes, 0..bytes.len())
        });
    assert!(matches!(refused, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_native_zero_record_id"));
}

#[test]
fn native_zero_entity_pair_ids_refuse_retained_limit() {
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::math::Point3;
    let point = FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("finite point");
    let pair = crate::families::zero_entity::topology::ZeroEntityEndpointPairCandidate {
        face_record_ordinals: [1, 2],
        support_record_ordinals: [3, 4],
        model_endpoints: [point, point],
        model_midpoint: point,
    };
    let refused = crate::test_support::with_retained_refusal(
        &[],
        "catia_native_zero_face_record_id",
        |ctx| super::super::zero_entity_endpoint_pair_candidates(ctx, std::slice::from_ref(&pair)),
    );
    assert!(matches!(refused, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_native_zero_face_record_id"));
    let admitted = crate::test_support::with_service_context(|ctx| {
        super::super::zero_entity_endpoint_pair_candidates(ctx, std::slice::from_ref(&pair))
    })
    .expect("service profile admits pair IDs");
    assert_eq!(
        admitted[0].id,
        "catia:zero-entity:endpoint-pair-candidate#0"
    );
    assert_eq!(admitted[0].face_records[0], "catia:zero-entity:record#1");
}

#[test]
fn native_zero_entity_edge_stride_scan_propagates_work_refusal() {
    let bytes = crate::test_support::test_zero_entity::zero_entity_topology_stream();
    let admitted = crate::test_support::with_service_context(|ctx| {
        super::super::zero_entity_edge_strides(ctx, &bytes, 0..bytes.len())
    })
    .expect("service profile admits edge strides");
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].allocations, [5, 7, 8, 4, 3]);

    let operation = "catia_native_zero_edge_stride_visits";
    let refused = crate::test_support::with_work_refusal(operation, |ctx| {
        let result = super::super::zero_entity_edge_strides(ctx, &bytes, 0..bytes.len());
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
        }
        result
    });
    assert!(matches!(refused, Err(CodecError::ResourceLimit(limit))
        if limit.operation == operation));
}
