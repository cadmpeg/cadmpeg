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
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("fixture fits input limit");
    let refused = super::super::zero_entity_records(&ctx, &bytes, 0..bytes.len());
    assert!(matches!(refused, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_native_zero_record_id"));
}

#[test]
fn native_zero_entity_pair_ids_refuse_retained_limit() {
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::math::Point3;
    let point = FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("finite point");
    let pair = crate::families::zero_entity::topology::ZeroEntityEndpointPairCandidate {
        face_record_ordinals: [1, 2], support_record_ordinals: [3, 4],
        model_endpoints: [point, point], model_midpoint: point,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    let refused = super::super::zero_entity_endpoint_pair_candidates(&ctx, vec![pair]);
    assert!(matches!(refused, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_native_zero_face_record_id"));
    let admitted = crate::test_support::with_service_context(|ctx| {
        super::super::zero_entity_endpoint_pair_candidates(ctx, vec![pair])
    }).expect("service profile admits pair IDs");
    assert_eq!(admitted[0].id, "catia:zero-entity:endpoint-pair-candidate#0");
    assert_eq!(admitted[0].face_records[0], "catia:zero-entity:record#1");
}
