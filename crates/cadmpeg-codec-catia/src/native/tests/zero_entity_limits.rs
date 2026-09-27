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
