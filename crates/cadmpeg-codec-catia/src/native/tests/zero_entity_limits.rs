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
fn native_zero_entity_allocation_ids_refuse_retained_limit() {
    let bytes = crate::test_support::test_zero_entity::zero_entity_allocation_stream();
    let (topology, records) = crate::test_support::with_service_context(|ctx| {
        let runs = crate::families::zero_entity::records::zero_entity_support_runs_in_range(
            ctx,
            &bytes,
            0..bytes.len(),
            &mut crate::nurbs::LaneRefusals::new(),
        )?;
        let topology =
            crate::families::zero_entity::allocation::parse(ctx, &bytes, 0..bytes.len(), &runs)?;
        let records = super::super::zero_entity_records(ctx, &bytes, 0..bytes.len())?;
        Ok::<_, CodecError>((topology, records))
    })
    .expect("complete source graph");
    let refused = crate::test_support::with_retained_refusal(
        &[],
        "catia_native_zero_allocation_target",
        |ctx| super::super::zero_entity_allocation::derive(ctx, topology.clone(), &records),
    );
    assert!(matches!(refused, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_native_zero_allocation_target"));
    let admitted = crate::test_support::with_service_context(|ctx| {
        super::super::zero_entity_allocation::derive(ctx, topology, &records)
    })
    .expect("service profile admits allocation IDs");
    assert_eq!(admitted.len(), 3);
    assert_eq!(admitted[0].id, "catia:zero-entity:allocation-edge#24");
}
