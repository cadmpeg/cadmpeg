// SPDX-License-Identifier: Apache-2.0

use crate::brep::typed::push_record;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::HashSet;

fn typed_record_offset_refusal(dimension: ResourceDimension) {
    let service = cadmpeg_test_support::service_decode_context();
    let mut storage = service.reserve_scoped(0, "test typed offsets").unwrap();
    let mut offsets = HashSet::new();
    let mut records = Vec::new();
    push_record(&service, &mut storage, &mut offsets, &mut records, 17, || Ok(23_u16)).unwrap();
    assert_eq!(offsets, HashSet::from([17]));
    assert_eq!(records, [23]);
    let arena = DecodeArena::new();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        dimension, "index typed Parasolid record offset", |cap| {
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                _ => panic!("record offset dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut storage = ctx.reserve_scoped(0, "test typed offsets").unwrap();
            let mut offsets = HashSet::new();
            let mut records = Vec::new();
            let result = push_record(&ctx, &mut storage, &mut offsets, &mut records, 17, || Ok(23_u16));
            if let Err(CodecError::ResourceLimit(ref limit)) = result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
                assert!(offsets.is_empty());
                assert!(records.is_empty());
            }
            result
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == dimension && limit.operation == "index typed Parasolid record offset"
            && limit.additional > 0));
}

#[test]
fn typed_record_offset_insertion_refuses_collection_before_mutation() {
    typed_record_offset_refusal(ResourceDimension::CollectionItems);
}

#[test]
fn typed_record_offset_insertion_refuses_work_before_mutation() {
    typed_record_offset_refusal(ResourceDimension::WorkUnits);
}

#[test]
fn typed_record_offset_insertion_refuses_scoped_bytes_before_mutation() {
    typed_record_offset_refusal(ResourceDimension::MaterializedBytes);
}
